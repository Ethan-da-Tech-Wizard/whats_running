// ==============================================================================
// Program: procfs.rs
// Programmer: Ethan-da-Tech-Wizard
// Last Modified: November 18, 2026
// Purpose: Linux /proc filesystem inspector and parser for What's Running.
//          Collects system uptime, memory metrics (/proc/meminfo), CPU time
//          (/proc/stat), and per-process telemetry (/proc/[pid]/stat, status,
//          cmdline, exe, cgroup, io) with granular typed error attribution (Field<T>).
// ==============================================================================

// Everything here reads Linux /proc through plain std::fs: no libc, no external crates, no
// process is ever signalled or modified. Reads are racy by nature (a process can exit mid-scan),
// which the Field type below exists to represent.
// Imports: fs for every /proc read and read_link, io for error kinds (field_error maps them onto Field),
// Path/PathBuf for the injectable /proc root, and Instant/SystemTime for the snapshot timing fields.
//
// Called by: main.rs (CLI snapshot output), tui.rs and gui.rs (periodic refresh), check.rs (health probes).
// Calls: nothing outside std. Tests below build a fake /proc under the OS temp dir.
//
// Known limits: Linux only. Values are instantaneous and not atomic across files, so a row can mix
// data from slightly different moments (for example stat read before status).
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Algebraic data type modeling granular telemetry field provenance and error attribution.
///
/// Observability Architecture Rationale:
/// Standard `Option<T>` conflates missing fields, permission denials (EACCES/EPERM under SELinux/DAC),
/// process termination races (ESRCH/Vanished), and parser discrepancies. `Field<T>` explicitly preserves
/// error causality so UI layers can render accurate diagnostics rather than silent zeroes or blank values.
#[derive(Debug, Clone)]
// Why an enum and not Result: read failures here are expected, frequent, and part of the data being
// displayed (another user's process is normal), so they are values to render, not errors to handle.
//
// All variants except `Value` carry no payload; `map` and `map_failure` below convert between payload types.
pub enum Field<T> {
    /// The datum was read and parsed successfully.
    Value(T),
    /// Deliberately skipped (e.g. command lines when the user did not opt in).
    NotCollected,
    /// EACCES/EPERM: typically another user's process or a hidepid-mounted /proc.
    PermissionDenied,
    /// The file disappeared (ENOENT): the process exited between enumeration and read.
    Vanished,
    /// This kernel/environment does not provide the datum.
    Unsupported,
    /// The file was read but did not have the expected format.
    ParseError,
    /// Any other I/O failure.
    IoError,
}

impl<T> Field<T> {
    /// Stable snake_case tag, used in the UI and in `visibility_summary`.
    // The match is exhaustive on purpose: adding a Field variant forces this table (and map/map_failure) to be updated.
    pub fn status(&self) -> &'static str {
        match self {
            Self::Value(_) => "value",
            Self::NotCollected => "not_collected",
            Self::PermissionDenied => "permission_denied",
            Self::Vanished => "vanished",
            Self::Unsupported => "unsupported",
            Self::ParseError => "parse_error",
            Self::IoError => "io_error",
        }
    }

    /// Collapses every failure variant to None; use `status()` when the reason matters.
    // Used where the reason for absence does not matter, for example sorting and totals.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }
}

// Only available for Copy payloads (numbers, chars), which avoids cloning String-valued fields by accident.
impl<T: Copy> Field<T> {
    /// Convenience for numeric columns: returns the number itself instead of a reference to it.
    pub fn copied(&self) -> Option<T> {
        self.value().copied()
    }
}

/// One row per /proc/<pid>. Every attribute except `pid` is a `Field` since each comes from a
/// separate file that may independently fail.
#[derive(Debug)]
// Memory layout is irrelevant here; rows are created once per scan and dropped at the next refresh.
// Units: *_ticks are clock ticks (CLK_TCK, normally 100 per second); *_kib are kibibytes; *_bytes are bytes.
pub struct Process {
    /// Process ID from the directory name; the only datum that cannot fail, which is why it is not a Field.
    pub pid: u32,
    /// Parent PID (stat field 4); used by the UI to build the process tree.
    pub ppid: Field<u32>,
    /// Real user ID from the `Uid:` line of /proc/<pid>/status; the UI maps it to a user name.
    pub uid: Field<u32>,
    /// Scheduler state letter from stat field 3 (R running, S sleeping, D uninterruptible, Z zombie, T stopped, ...).
    pub state: Field<char>,
    /// The `comm` name from stat (max 15 bytes, set by the program); not the full command line.
    pub name: Field<String>,
    /// Full command line; NotCollected unless the user opted in, because arguments can contain secrets.
    pub command: Field<String>,
    /// Target of the /proc/<pid>/exe symlink: the binary actually running, which can differ from `name`.
    pub executable: Field<String>,
    /// Process start time in clock ticks since boot (stat field 22); convert with the system tick rate (CLK_TCK).
    pub start_ticks: Field<u64>,
    /// utime/stime from /proc/<pid>/stat (fields 14, 15), in clock ticks.
    pub cpu_user_ticks: Field<u64>,
    /// Time in kernel mode, in clock ticks; user plus system gives the process's total CPU time.
    pub cpu_system_ticks: Field<u64>,
    /// Resident set size from the `VmRSS:` line of /proc/<pid>/status, in KiB.
    pub rss_kib: Field<u64>,
    /// Bytes actually fetched from / sent to storage per /proc/<pid>/io. That file is usually readable only for own processes.
    pub read_bytes: Field<u64>,
    /// Counterpart of `read_bytes`; bytes sent to storage (page-cache writeback counted when flushed).
    pub write_bytes: Field<u64>,
    /// The `Threads:` line of status; cheap to read even when the full `threads` list is not needed.
    pub thread_count: Field<u64>,
    /// Control-group path from /proc/<pid>/cgroup; identifies the service, container or session a process belongs to.
    pub cgroup: Field<String>,
    /// Derived from the cgroup path (see find_systemd_unit); Unsupported when no unit appears in it.
    pub systemd_unit: Field<String>,
    /// Individual threads from /proc/<pid>/task; used for per-thread CPU breakdowns.
    pub threads: Field<Vec<Thread>>,
    /// (kind, link target) pairs like ("pid", "pid:[4026531836]"); equal targets mean shared namespaces.
    pub namespaces: Field<Vec<(String, String)>>,
}

/// Per-thread data from /proc/<pid>/task/<tid>/stat; plain values because unreadable threads are skipped.
#[derive(Debug)]
pub struct Thread {
    /// Kernel thread ID (the main thread's tid equals the process pid).
    pub tid: u32,
    /// Thread name; threads can rename themselves, so it may differ from the process `comm`.
    pub name: String,
    /// Same state letters as `Process::state`, but for this thread only.
    pub state: char,
    /// Start time of the thread in clock ticks since boot.
    pub start_ticks: u64,
    /// Thread user-mode CPU time in clock ticks.
    pub cpu_user_ticks: u64,
    /// Thread kernel-mode CPU time in clock ticks.
    pub cpu_system_ticks: u64,
}

impl Process {
    /// Reports which of the four identity fields could not be read, e.g. `exe:permission_denied`;
    /// "complete" when all were. Gives users an explanation instead of silently blank columns.
    // The four fields checked here are the ones the UI needs for identity (who is it, what is it running, where
    // does it live); numeric columns are omitted because they rarely fail independently of these.
    //
    // Example outputs: `complete`, or `exe:permission_denied,cgroup:vanished`.
    pub fn visibility_summary(&self) -> String {
        // Array of (label, field) pairs so the loop below can report names and statuses without four repeated blocks.
        let fields = [
            ("name", &self.name),
            ("command", &self.command),
            ("exe", &self.executable),
            ("cgroup", &self.cgroup),
        ];
        let failures: Vec<String> = fields
            .into_iter()
            // Keep only the failures; `Field::Value` means that column was fine.
            .filter(|(_, field)| !matches!(field, Field::Value(_)))
            .map(|(name, field)| format!("{name}:{}", field.status()))
            .collect();

        // Explicit "complete" (rather than an empty string) so the UI can show a positive status.
        if failures.is_empty() {
            "complete".into()
        } else {
            failures.join(",")
        }
    }
}

/// A point-in-time view of the machine: all processes plus system-wide CPU and memory totals.
// Not Debug/Clone on purpose: a snapshot can hold thousands of rows, so callers should move or borrow it
// rather than copy it. Consecutive snapshots are compared by the activity module to produce CPU % and I/O rates.
pub struct Snapshot {
    /// PID of this program, so the UI can flag or exclude the observer itself.
    pub observer_pid: u32,
    /// Wall-clock time the scan began (ms since the Unix epoch); lets consumers timestamp history and logs.
    pub started_unix_ms: u128,
    /// How long the whole scan took, measured with a monotonic clock; shown so users can see the tool's own cost.
    pub duration_micros: u128,
    /// Count of /proc directory entries that could not be read at all.
    pub enumeration_errors: usize,
    pub processes: Vec<Process>,
    /// Sum of the aggregate `cpu` line in /proc/stat; use deltas between snapshots to compute CPU %.
    pub system_cpu_ticks: Option<u64>,
    /// Number of `cpuN` lines in /proc/stat (logical CPUs); divides total CPU time into per-core percentages.
    pub cpu_count: usize,
    /// System-wide memory figures; see `Memory`.
    pub memory: Memory,
}

/// All values in KiB as reported by /proc/meminfo; None when a line is absent.
#[derive(Debug, Default)]
// Option (not Field) because these come from one file with a flat format, where a missing line is the only failure.
pub struct Memory {
    /// `MemTotal:` physical RAM visible to the kernel.
    pub total_kib: Option<u64>,
    /// `MemAvailable:` the kernel's estimate of RAM usable without swapping; a better "free" number than MemFree.
    pub available_kib: Option<u64>,
    /// `SwapTotal:` configured swap; 0 means no swap.
    pub swap_total_kib: Option<u64>,
    /// `SwapFree:` unused swap; (total - free) is swap in use.
    pub swap_free_kib: Option<u64>,
}

/// Entry point for real use. `collect_commands` is the opt-in for reading command lines, which can
/// contain secrets (tokens, passwords) passed as arguments.
// Design note: the real /proc path and the PID of this process are fixed here; everything else (the
// scan itself) lives in `collect_from` so tests can inject a fake root and observer pid.
//
// Errors: only a failure to list /proc itself is returned as `Err`; every per-process problem is
// recorded inside the `Field`s of the affected row.
//
// Called by: main.rs (one-shot CLI output) and the tui/gui refresh loops (about once a second).
pub fn collect_snapshot(collect_commands: bool) -> io::Result<Snapshot> {
    collect_from(Path::new("/proc"), std::process::id(), collect_commands)
}

/// Root is a parameter (not hard-coded /proc) so tests can scan a synthetic directory tree.
// Walks `root` once, reading one directory per numeric entry. Complexity is O(processes) file reads,
// roughly 8-12 small reads per process, which is why the 2000-process test has a time budget.
//
// Ordering guarantee: the returned processes are sorted by pid, so callers may binary-search by pid.
fn collect_from(root: &Path, observer_pid: u32, collect_commands: bool) -> io::Result<Snapshot> {
    // Monotonic timer for the collection duration (immune to wall-clock changes).
    // Wall-clock start (for display) and monotonic timer (for duration) are separate on purpose.
    let timer = Instant::now();
    let started_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut processes = Vec::new();
    let mut enumeration_errors = 0;

    // Only the top-level read_dir is fatal (`?`): if /proc itself cannot be listed there is nothing to show.
    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            // Unreadable entries are counted rather than aborting the scan.
            Err(_) => {
                enumeration_errors += 1;
                continue;
            }
        };
        // Only all-numeric directory names are processes; skips `self`, `net`, `meminfo`, etc.
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };

        // Per-process failures never propagate; read_process records them inside each Field instead.
        processes.push(read_process(root, pid, collect_commands));
    }
    // read_dir order is arbitrary; sort for stable display and indexing.
    processes.sort_unstable_by_key(|process| process.pid);

    // System totals come from /proc/stat and /proc/meminfo, read once per snapshot (not per process).
    let system_stat = read_bytes(root.join("stat"));
    // Failure to read /proc/stat degrades to (None, 0) rather than failing the whole snapshot.
    let (system_cpu_ticks, cpu_count) = system_stat
        .value()
        .map(|v| parse_system_cpu(v))
        .unwrap_or((None, 0));
    let memory = read_bytes(root.join("meminfo"))
        .value()
        .map(|v| parse_memory(v))
        .unwrap_or_default();

    // duration is sampled last so it includes the totals and the sort.
    Ok(Snapshot {
        observer_pid,
        started_unix_ms,
        duration_micros: timer.elapsed().as_micros(),
        enumeration_errors,
        processes,
        system_cpu_ticks,
        cpu_count,
        memory,
    })
}

/// Each file is read independently so a failure in one (say `io`) leaves the others intact.
// Never fails and never panics: unreadable data becomes a `Field` failure variant. A process that
// exits halfway through yields a row where later fields are `Vanished` (earlier ones may still hold values).
//
// Concurrency note: there is no locking; /proc is a live view, so two files of the same process can
// disagree by a few milliseconds of CPU time. That is acceptable for monitoring.
fn read_process(root: &Path, pid: u32, collect_commands: bool) -> Process {
    // Reads below are deliberately separate files: the kernel generates each one on demand, and some (io, exe)
    // have stricter permissions than others, so each needs its own failure status.
    let directory = root.join(pid.to_string());

    // stat is the richest single file (name, state, ppid, CPU ticks, start time); it is parsed once into
    // `parsed` and every derived field below falls back to the same failure reason when that fails.
    let stat = read_bytes(directory.join("stat"));
    let parsed = stat.value().and_then(|bytes| parse_stat(bytes));
    // If stat could not be read OR parsed, derive the failure reason from the original read result.
    // Name prefers the parsed comm; on failure we re-use the read error so the UI can say *why* it is blank.
    let name = match &parsed {
        Some(stat) => Field::Value(stat.name.clone()),
        None => map_failure(&stat),
    };

    // cmdline is only read on explicit opt-in (privacy): arguments commonly carry tokens and passwords.
    let command = if collect_commands {
        read_bytes(directory.join("cmdline")).map(|bytes| parse_cmdline(&bytes))
    } else {
        Field::NotCollected
    };

    // exe uses read_link rather than read: it is a symlink to a binary, not a file with contents.
    let executable = match fs::read_link(directory.join("exe")) {
        Ok(path) => Field::Value(path.to_string_lossy().into_owned()),
        // /proc/<pid>/exe is a symlink; readlink fails with EACCES for other users' processes and
        // for kernel threads, which is reported as a Field status rather than hidden.
        Err(error) => field_error(error),
    };

    // status is read once and reused for uid, VmRSS and Threads to avoid three reads of the same file.
    let status = read_bytes(directory.join("status"));
    // `Uid:` has four columns (real, effective, saved, fs); parsed_number takes the first (real UID).
    // The cast is safe: UIDs are 32-bit on Linux, and parsed_number returns u64 only because it is shared with byte counters.
    let uid = parsed_number(&status, b"Uid:").map(|value| value as u32);
    let rss_kib = parsed_number(&status, b"VmRSS:");

    // /proc/<pid>/io needs ptrace-level access, so for other users' processes these become PermissionDenied.
    let io = read_bytes(directory.join("io"));
    let process_read_bytes = parsed_number(&io, b"read_bytes:");
    let write_bytes = parsed_number(&io, b"write_bytes:");
    let thread_count = parsed_number(&status, b"Threads:");

    // cgroup is cheap to read and gives the service/container grouping that the process tree view relies on.
    let cgroup = read_bytes(directory.join("cgroup")).map(|bytes| parse_cgroup(&bytes));
    // Unit names are inferred from the cgroup path, not by asking systemd.
    // A cgroup that exists but names no unit (for example a plain container path) is Unsupported, not an error.
    let systemd_unit = match &cgroup {
        Field::Value(path) => match find_systemd_unit(path) {
            Some(unit) => Field::Value(unit),
            None => Field::Unsupported,
        },
        // Failures pass through unchanged so the original reason (permission, vanished...) survives to the UI.
        other => map_failure(other),
    };

    // Thread and namespace listings cost extra directory reads per process; the callers accept this for 100s of processes.
    let threads = read_threads(&directory);
    let namespaces = read_namespaces(&directory);

    // Struct built field by field so each stat-derived column can independently fall back to `map_failure`.
    Process {
        pid,
        // `parsed` is None on any stat failure; each stat-derived field then re-reports the same failure reason.
        // Repeated `parsed.as_ref().map(...).unwrap_or_else(|| map_failure(&stat))` pattern: take the parsed
        // value if stat parsed, otherwise re-type the original read failure for this field's type.
        ppid: parsed
            .as_ref()
            .map(|value| Field::Value(value.ppid))
            .unwrap_or_else(|| map_failure(&stat)),
        uid,
        state: parsed
            .as_ref()
            .map(|value| Field::Value(value.state))
            .unwrap_or_else(|| map_failure(&stat)),
        name,
        command,
        executable,
        start_ticks: parsed
            .as_ref()
            .map(|value| Field::Value(value.start_ticks))
            .unwrap_or_else(|| map_failure(&stat)),
        cpu_user_ticks: parsed
            .as_ref()
            .map(|value| Field::Value(value.user_ticks))
            .unwrap_or_else(|| map_failure(&stat)),
        cpu_system_ticks: parsed
            .as_ref()
            .map(|value| Field::Value(value.system_ticks))
            .unwrap_or_else(|| map_failure(&stat)),
        // The remaining fields were already computed as Fields above and are moved in unchanged.
        rss_kib,
        read_bytes: process_read_bytes,
        write_bytes,
        thread_count,
        cgroup,
        systemd_unit,
        threads,
        namespaces,
    }
}

impl<T> Field<T> {
    /// Transforms the payload of a `Value` and leaves every failure variant untouched, so parsing steps can be
    /// chained without losing the reason a read failed.
    pub fn map<U>(self, convert: impl FnOnce(T) -> U) -> Field<U> {
        match self {
            Self::Value(value) => Field::Value(convert(value)),
            Self::NotCollected => Field::NotCollected,
            Self::PermissionDenied => Field::PermissionDenied,
            Self::Vanished => Field::Vanished,
            Self::Unsupported => Field::Unsupported,
            Self::ParseError => Field::ParseError,
            Self::IoError => Field::IoError,
        }
    }
}

/// Reads whole file as bytes (not String): /proc files may contain non-UTF-8 data, e.g. cmdline.
// Takes PathBuf by value because every caller builds a fresh path with join(); no clone needed.
fn read_bytes(path: PathBuf) -> Field<Vec<u8>> {
    // fs::read (whole file) is fine: /proc files are generated by the kernel and are at most a few KB (cmdline can be larger).
    match fs::read(path) {
        Ok(value) => Field::Value(value),
        Err(error) => field_error(error),
    }
}

/// Translates an `io::Error` into the matching `Field` failure variant (permission, vanished, unsupported, other).
fn field_error<T>(error: io::Error) -> Field<T> {
    match error.kind() {
        io::ErrorKind::PermissionDenied => Field::PermissionDenied,
        // In /proc, ENOENT almost always means the process exited after being listed.
        io::ErrorKind::NotFound => Field::Vanished,
        // Reached on kernels or sandboxes that do not provide the file at all (for example some container runtimes).
        io::ErrorKind::Unsupported => Field::Unsupported,
        // Everything else (EIO, EINVAL, ...) is lumped together; the UI shows it as a generic read failure.
        _ => Field::IoError,
    }
}

/// Re-types a failed Field so one read failure can be propagated to differently typed derived fields.
/// `Value` maps to ParseError: reaching here with a Value means parsing the value failed.
fn map_failure<T, U>(field: &Field<T>) -> Field<U> {
    match field {
        // A Value reaching map_failure means the read worked but the content did not parse, hence ParseError.
        Field::Value(_) => Field::ParseError,
        Field::NotCollected => Field::NotCollected,
        Field::PermissionDenied => Field::PermissionDenied,
        Field::Vanished => Field::Vanished,
        Field::Unsupported => Field::Unsupported,
        Field::ParseError => Field::ParseError,
        Field::IoError => Field::IoError,
    }
}

/// Finds a `Key:   123 ...` line in a status-like file and returns the first number.
// `key` includes the trailing colon, e.g. `b"VmRSS:"`. Returns ParseError when the key is absent, for
// example VmRSS on kernel threads (they have no user memory), which the UI shows as blank.
fn parsed_number(source: &Field<Vec<u8>>, key: &[u8]) -> Field<u64> {
    match source {
        // Parse failure on a successfully read file (missing key or non-numeric value) becomes ParseError, distinct from read errors.
        Field::Value(bytes) => parse_status_number(bytes, key)
            .map(Field::Value)
            .unwrap_or(Field::ParseError),
        other => map_failure(other),
    }
}

/// Threads are listed in /proc/<pid>/task/<tid>; unreadable or unparsable threads are skipped
/// (they typically exit mid-scan) rather than failing the list.
// A thread that exits between read_dir and the stat read is simply absent from the list.
// Cost: one read_dir plus one file read per thread, so processes with hundreds of threads (browsers, JVMs)
// dominate scan time.
fn read_threads(process: &Path) -> Field<Vec<Thread>> {
    // `task` is unreadable for the same reasons as other per-process files, so the failure kind is kept on the field.
    let entries = match fs::read_dir(process.join("task")) {
        Ok(v) => v,
        Err(e) => return field_error(e),
    };
    let mut threads = Vec::new();

    // flatten() skips unreadable directory entries; a thread vanishing mid-listing is normal on busy systems.
    for entry in entries.flatten() {
        let Some(tid) = entry
            .file_name()
            .to_str()
            .and_then(|v| v.parse::<u32>().ok())
        else {
            continue;
        };

        // Thread stat has the same layout as process stat, so parse_stat is reused.
        let bytes = match read_bytes(entry.path().join("stat")) {
            Field::Value(v) => v,
            _ => continue,
        };
        if let Some(stat) = parse_stat(&bytes) {
            threads.push(Thread {
                tid,
                name: stat.name,
                state: stat.state,
                start_ticks: stat.start_ticks,
                cpu_user_ticks: stat.user_ticks,
                cpu_system_ticks: stat.system_ticks,
            });
        }
    }

    // read_dir order is arbitrary; sorting by tid gives stable output (the main thread comes first).
    threads.sort_unstable_by_key(|thread| thread.tid);
    Field::Value(threads)
}
/// All-or-nothing: any single unreadable namespace link fails the whole field, because a partial
/// list could mislead isolation analysis.
// Reads symlinks, not file contents: `ns/<kind>` links look like `net:[4026531992]`.
// Processes with equal link text share that namespace, which is how containers are told apart from the host.
fn read_namespaces(process: &Path) -> Field<Vec<(String, String)>> {
    let mut values = Vec::new();

    // These five namespaces are the ones that matter for container/isolation analysis; others (ipc, uts, time)
    // are omitted to keep the per-process cost to five readlink calls.
    for name in ["pid", "mnt", "net", "user", "cgroup"] {
        match fs::read_link(process.join("ns").join(name)) {
            // The link text looks like `pid:[4026531836]`; the number is the namespace inode, equal for processes that share it.
            Ok(value) => values.push((name.into(), value.to_string_lossy().into_owned())),
            // Distinct failure kinds are mapped by hand (not field_error) because the early return needs the exact Field type.
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                return Field::PermissionDenied;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Field::Vanished,
            // Any other error also aborts: a partial namespace list is worse than none (see doc comment above).
            Err(_) => return Field::IoError,
        }
    }
    Field::Value(values)
}

/// The subset of /proc/<pid>/stat this tool needs.
// Private on purpose: callers get the data through `Process`/`Thread`; this exists only so the stat
// parser can return several values at once without a tuple.
struct ParsedStat {
    // Plain String (not Field): parse_stat either yields a complete record or nothing, so a partial ParsedStat never exists.
    name: String,
    state: char,
    // Fields below mirror the same-named columns of `Process` so the struct literal in read_process is a direct copy.
    ppid: u32,
    user_ticks: u64,
    system_ticks: u64,
    start_ticks: u64,
}

/// Parses /proc/<pid>/stat into the handful of fields this tool uses; None if the line is malformed.
///
/// The name field is parsed separately from the rest because it is the only free-text field; every other
/// field is a number or single letter, so whitespace splitting is safe after it.
fn parse_stat(bytes: &[u8]) -> Option<ParsedStat> {
    // The command name sits between the FIRST '(' and the LAST ')', and may itself contain spaces and
    // parentheses, so naive whitespace splitting would break. rposition handles `(odd) process)`.
    // First '(' and last ')': the name is the only field that can contain parentheses and spaces, so it is the anchor for splitting.
    let open = bytes.iter().position(|byte| *byte == b'(')?;
    let close = bytes.iter().rposition(|byte| *byte == b')')?;

    // A ')' before the '(' means the line is corrupt; bail out rather than slicing with a reversed range.
    if close <= open {
        return None;
    }

    // Lossy conversion: a process may set a non-UTF-8 name; showing a replacement character beats dropping the row.
    let name = String::from_utf8_lossy(&bytes[open + 1..close]).into_owned();
    // Skip the `) ` after the name; everything after it is plain ASCII numbers and a state letter.
    // get(close + 2..) skips ") "; None here (line ends right after the name) makes the whole parse fail instead of panicking.
    let remainder = std::str::from_utf8(bytes.get(close + 2..)?).ok()?;

    // Collected once so the individual get(n) lookups below are O(1).
    let fields: Vec<&str> = remainder.split_whitespace().collect();
    Some(ParsedStat {
        name,
        // Indexes below are relative to the text after the name: [0]=state, [1]=ppid, [11]=utime,
        // [12]=stime, [19]=starttime (proc(5) fields 3, 4, 14, 15, 22).
        // `?` on every lookup: any missing or non-numeric column makes the whole parse return None.
        state: fields.first()?.chars().next()?,
        ppid: fields.get(1)?.parse().ok()?,
        user_ticks: fields.get(11)?.parse().ok()?,
        system_ticks: fields.get(12)?.parse().ok()?,
        start_ticks: fields.get(19)?.parse().ok()?,
    })
}

/// cmdline arguments are NUL-separated; joined with spaces for display (so original quoting is lost).
// Input is raw bytes because arguments are arbitrary byte strings; the lossy decode happens per argument.
fn parse_cmdline(bytes: &[u8]) -> String {
    let arguments: Vec<String> = bytes
        .split(|byte| *byte == 0)
        // Empty pieces come from the trailing NUL and from empty arguments; dropping them keeps the display tidy.
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    if arguments.is_empty() {
        // Placeholder text, not an empty string, so the Command column never looks like a read failure.
        // Kernel threads and zombies have an empty cmdline.
        "[kernel thread or empty command]".into()
    } else {
        // Original quoting is lost (see doc comment), which is acceptable for display; never re-execute this string.
        arguments.join(" ")
    }
}

/// Also used for /proc/meminfo, which has the same `Key:  value unit` shape.
// Generic over any `Key: value` file (status, meminfo, io): returns None when the key is absent or the value is not a number.
fn parse_status_number(bytes: &[u8], key: &[u8]) -> Option<u64> {
    bytes
        .split(|byte| *byte == b'\n')
        // First matching prefix wins; `key` includes the colon (e.g. `Uid:`) so `VmRSS:` cannot match `VmRSSx:`.
        .find(|line| line.starts_with(key))
        .and_then(|line| std::str::from_utf8(&line[key.len()..]).ok())
        // Take only the first token so a trailing unit such as `kB` and extra columns (Uid has four) are ignored.
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

/// Returns (total jiffies from the aggregate `cpu` line, number of `cpuN` lines).
// Total jiffies = user+nice+system+idle+iowait+irq+softirq+steal (eight counters). Dividing the delta of
// this value between two snapshots by the delta of a process's ticks gives that process's CPU share.
fn parse_system_cpu(bytes: &[u8]) -> (Option<u64>, usize) {
    // None when the `cpu` line is missing or any counter fails to parse.
    // `total` stays None unless the aggregate `cpu ` line (note the trailing space) parses completely.
    let mut total = None;
    let mut count = 0;

    // /proc/stat has one line per CPU plus other counters (intr, ctxt, btime); the loop ignores all but the cpu lines.
    for line in bytes.split(|b| *b == b'\n') {
        if line.starts_with(b"cpu ") {
            // Skips the 4-byte `cpu ` prefix; checked_add makes an overflow produce None instead of wrapping silently.
            total = std::str::from_utf8(&line[4..]).ok().and_then(|s| {
                // guest and guest_nice are already included in user and nice.
                // Linux CPU totals therefore use only the first eight counters.
                s.split_whitespace().take(8).try_fold(0_u64, |sum, v| {
                    v.parse::<u64>().ok().and_then(|n| sum.checked_add(n))
                })
            });

        // `cpu0`, `cpu1`, ...: count only lines whose 4th byte is a digit so `cpu ` (aggregate) and `cpufreq`-like lines are excluded.
        } else if line.starts_with(b"cpu") && line.get(3).is_some_and(u8::is_ascii_digit) {
            count += 1;
        }
    }
    (total, count)
}

/// Extracts the four meminfo lines the UI shows; each is independently optional (older kernels omit some).
fn parse_memory(bytes: &[u8]) -> Memory {
    Memory {
        total_kib: parse_status_number(bytes, b"MemTotal:"),
        available_kib: parse_status_number(bytes, b"MemAvailable:"),
        swap_total_kib: parse_status_number(bytes, b"SwapTotal:"),
        swap_free_kib: parse_status_number(bytes, b"SwapFree:"),
    }
}

/// Takes the first line. Format is `hierarchy-id:controllers:path`; splitn(3) keeps colons inside the
/// path intact. On cgroup v2 there is one line, `0::/path`.
// v1 has one line per controller (hierarchy id, controller list, path); v2 (unified) has a single `0::/path`.
// Taking the first line with a third field works for both.
fn parse_cgroup(bytes: &[u8]) -> String {
    bytes
        .split(|byte| *byte == b'\n')
        .find_map(|line| line.splitn(3, |byte| *byte == b':').nth(2))
        // nth(2) is the path column; lossy decode because cgroup paths are arbitrary bytes in theory.
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .unwrap_or_default()
}

/// Searches from the deepest path component up so the most specific unit (e.g. foot.service) wins
/// over its parent slices.
// Systemd names its cgroups `<name>.service`, `.scope` (transient, e.g. login sessions) and `.slice` (groups).
fn find_systemd_unit(path: &str) -> Option<String> {
    path.split('/')
        // rev() = deepest path component first, so `app.slice/foot.service` yields foot.service before app.slice.
        .rev()
        .find(|part| {
            [".service", ".scope", ".slice"]
                .iter()
                .any(|suffix| part.ends_with(suffix))
        })
        .map(str::to_owned)
}

#[cfg(test)]
// Test strategy: the parsers are pure functions over bytes, so most tests feed literal /proc text. One
// integration-style test builds a fake /proc tree to exercise the directory scan and the Field status paths.
mod tests {
    // Brings the private parsers (parse_stat, parse_status_number, ...) into scope; they are not part of the public API.
    use super::*;

    /// Regression test for the name-parsing edge case: the name is `odd) process`.
    #[test]
    fn stat_parser_handles_spaces_and_parentheses_in_name() {
        // Fields after the name are 1..21; only state (S), ppid (1), utime (11), stime (12) and starttime (19) are checked.
        let input = b"42 (odd) process) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21";
        let stat = parse_stat(input).unwrap();
        assert_eq!(stat.name, "odd) process");
        assert_eq!(stat.state, 'S');

        // The expected numbers equal the field's position in the fixture, which is how the index mapping in parse_stat is pinned.
        assert_eq!(stat.ppid, 1);
        assert_eq!(stat.user_ticks, 11);
        assert_eq!(stat.system_ticks, 12);
        assert_eq!(stat.start_ticks, 19);
    }

    #[test]
    /// Arguments are separated by NUL, not spaces: "two words" must stay one argument rather than be split in two.
    fn command_line_uses_nul_boundaries() {
        assert_eq!(parse_cmdline(b"hello\0two words\0"), "hello two words");
    }

    #[test]
    /// Pins the `Key:\tvalue unit` parse: only the first number is returned (Uid has real/effective/saved/fs columns).
    fn reads_status_values() {
        // Tab-separated like the real file; the `kB` suffix must be ignored.
        let status = b"Name:\tx\nUid:\t1000\t1000\nVmRSS:\t1234 kB\n";
        assert_eq!(parse_status_number(status, b"Uid:"), Some(1000));
        assert_eq!(parse_status_number(status, b"VmRSS:"), Some(1234));
    }

    #[test]
    /// Pins the CPU total rule (first eight counters only; guest time is already inside user/nice, so the 900 and 1000 must
    /// be ignored) and the per-CPU line count and meminfo lookup.
    fn reads_system_totals() {
        assert_eq!(
            // Aggregate line has a double space after `cpu` in real kernels; split_whitespace makes that irrelevant.
            parse_system_cpu(b"cpu  1 2 3 4\ncpu0 1\ncpu1 1\n"),
            (Some(10), 2)
        );
        assert_eq!(
            // Ten counters: 1..8 sum to 36; the last two (guest, guest_nice) must not be added again.
            parse_system_cpu(b"cpu  1 2 3 4 5 6 7 8 900 1000\ncpu0 1\n"),
            (Some(36), 1)
        );
        assert_eq!(
            parse_memory(b"MemTotal: 100 kB\nMemAvailable: 60 kB\n").available_kib,
            Some(60)
        );
    }

    #[test]
    /// Pins cgroup v2 parsing (`0::/path`) and that the deepest unit name wins over its parent slices.
    fn reads_cgroup_and_unit() {
        let path = parse_cgroup(b"0::/user.slice/user-1000.slice/app.slice/foot.service\n");
        assert_eq!(path, "/user.slice/user-1000.slice/app.slice/foot.service");
        assert_eq!(find_systemd_unit(&path).as_deref(), Some("foot.service"));
    }

    /// Builds a fake /proc with 2000 processes: checks ordering, graceful handling of absent files
    /// (exe -> vanished, command -> not_collected) and a loose performance budget.
    #[test]
    fn synthetic_proc_tree_collects_many_processes_and_tolerates_missing_fields() {
        let root =
            std::env::temp_dir().join(format!("whats-running-fixture-{}", std::process::id()));

        // Remove leftovers from a previous crashed run, so the process count assertion below is exact.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();

        // The fake root needs the two system files too, or collect_from would fall back to the (None, 0) defaults.
        fs::write(root.join("stat"), b"cpu 1 2 3 4\ncpu0 1\n").unwrap();
        fs::write(
            root.join("meminfo"),
            b"MemTotal: 100 kB\nMemAvailable: 50 kB\n",
        )
        .unwrap();

        // 2000 processes approximates a busy desktop; each gets only the files the test needs, so exe and cgroup are absent on purpose.
        for pid in 1..=2_000_u32 {
            let dir = root.join(pid.to_string());
            fs::create_dir(&dir).unwrap();
            fs::write(
                dir.join("stat"),
                // A name with a space ("fixture N") makes every row exercise the parenthesis-aware name parser.
                format!("{pid} (fixture {pid}) S 0 0 0 0 0 0 0 0 0 0 1 2 0 0 0 0 0 0 3 0"),
            )
            .unwrap();
            fs::write(dir.join("status"), b"Uid: 1000\nVmRSS: 4 kB\n").unwrap();
            fs::write(dir.join("cmdline"), b"fixture\0").unwrap();

            // Present for all fake processes so read/write byte parsing is covered; exe and cgroup are intentionally missing.
            fs::write(dir.join("io"), b"read_bytes: 10\nwrite_bytes: 20\n").unwrap();
        }
        let snapshot = collect_from(&root, 100, false).unwrap();

        // Every directory with a numeric name must become a row: none dropped by a parse failure.
        assert_eq!(snapshot.processes.len(), 2_000);

        // Loose wall-clock budget (250 ms). This is the known slow check on loaded machines; it is about
        // catching accidental O(n^2) behaviour, not benchmarking.
        assert!(
            snapshot.duration_micros < 250_000,
            "collection exceeded the initial 250 ms budget: {} us",
            snapshot.duration_micros
        );

        // Sorted by pid, so index 99 is pid 100; this proves the sort and the stat name parse together.
        assert_eq!(snapshot.processes[99].name.value().unwrap(), "fixture 100");
        assert_eq!(snapshot.processes[0].executable.status(), "vanished");
        assert_eq!(snapshot.processes[0].command.status(), "not_collected");

        // Clean up the 2000-directory fixture so repeated runs do not fill the temp dir.
        fs::remove_dir_all(root).unwrap();
    }
}
