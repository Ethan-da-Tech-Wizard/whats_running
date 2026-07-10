use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub enum Field<T> {
    Value(T),
    NotCollected,
    PermissionDenied,
    Vanished,
    Unsupported,
    ParseError,
    IoError,
}

impl<T> Field<T> {
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

    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }
}
impl<T: Copy> Field<T> {
    pub fn copied(&self) -> Option<T> {
        self.value().copied()
    }
}

#[derive(Debug)]
pub struct Process {
    pub pid: u32,
    pub ppid: Field<u32>,
    pub uid: Field<u32>,
    pub state: Field<char>,
    pub name: Field<String>,
    pub command: Field<String>,
    pub executable: Field<String>,
    pub start_ticks: Field<u64>,
    pub cpu_user_ticks: Field<u64>,
    pub cpu_system_ticks: Field<u64>,
    pub rss_kib: Field<u64>,
    pub read_bytes: Field<u64>,
    pub write_bytes: Field<u64>,
    pub thread_count: Field<u64>,
    pub cgroup: Field<String>,
    pub systemd_unit: Field<String>,
    pub threads: Field<Vec<Thread>>,
    pub namespaces: Field<Vec<(String, String)>>,
}

#[derive(Debug)]
pub struct Thread {
    pub tid: u32,
    pub name: String,
    pub state: char,
    pub start_ticks: u64,
    pub cpu_user_ticks: u64,
    pub cpu_system_ticks: u64,
}

impl Process {
    pub fn visibility_summary(&self) -> String {
        let fields = [
            ("name", &self.name),
            ("command", &self.command),
            ("exe", &self.executable),
            ("cgroup", &self.cgroup),
        ];
        let failures: Vec<String> = fields
            .into_iter()
            .filter(|(_, field)| !matches!(field, Field::Value(_)))
            .map(|(name, field)| format!("{name}:{}", field.status()))
            .collect();
        if failures.is_empty() {
            "complete".into()
        } else {
            failures.join(",")
        }
    }
}

pub struct Snapshot {
    pub observer_pid: u32,
    pub started_unix_ms: u128,
    pub duration_micros: u128,
    pub enumeration_errors: usize,
    pub processes: Vec<Process>,
    pub system_cpu_ticks: Option<u64>,
    pub cpu_count: usize,
    pub memory: Memory,
}

#[derive(Debug, Default)]
pub struct Memory {
    pub total_kib: Option<u64>,
    pub available_kib: Option<u64>,
    pub swap_total_kib: Option<u64>,
    pub swap_free_kib: Option<u64>,
}

pub fn collect_snapshot(collect_commands: bool) -> io::Result<Snapshot> {
    collect_from(Path::new("/proc"), std::process::id(), collect_commands)
}

fn collect_from(root: &Path, observer_pid: u32, collect_commands: bool) -> io::Result<Snapshot> {
    let timer = Instant::now();
    let started_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut processes = Vec::new();
    let mut enumeration_errors = 0;

    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                enumeration_errors += 1;
                continue;
            }
        };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        processes.push(read_process(root, pid, collect_commands));
    }
    processes.sort_unstable_by_key(|process| process.pid);
    let system_stat = read_bytes(root.join("stat"));
    let (system_cpu_ticks, cpu_count) = system_stat
        .value()
        .map(|v| parse_system_cpu(v))
        .unwrap_or((None, 0));
    let memory = read_bytes(root.join("meminfo"))
        .value()
        .map(|v| parse_memory(v))
        .unwrap_or_default();

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

fn read_process(root: &Path, pid: u32, collect_commands: bool) -> Process {
    let directory = root.join(pid.to_string());
    let stat = read_bytes(directory.join("stat"));
    let parsed = stat.value().and_then(|bytes| parse_stat(bytes));
    let name = match &parsed {
        Some(stat) => Field::Value(stat.name.clone()),
        None => map_failure(&stat),
    };
    let command = if collect_commands {
        read_bytes(directory.join("cmdline")).map(|bytes| parse_cmdline(&bytes))
    } else {
        Field::NotCollected
    };
    let executable = match fs::read_link(directory.join("exe")) {
        Ok(path) => Field::Value(path.to_string_lossy().into_owned()),
        Err(error) => field_error(error),
    };
    let status = read_bytes(directory.join("status"));
    let uid = parsed_number(&status, b"Uid:").map(|value| value as u32);
    let rss_kib = parsed_number(&status, b"VmRSS:");
    let io = read_bytes(directory.join("io"));
    let process_read_bytes = parsed_number(&io, b"read_bytes:");
    let write_bytes = parsed_number(&io, b"write_bytes:");
    let thread_count = parsed_number(&status, b"Threads:");
    let cgroup = read_bytes(directory.join("cgroup")).map(|bytes| parse_cgroup(&bytes));
    let systemd_unit = match &cgroup {
        Field::Value(path) => match find_systemd_unit(path) {
            Some(unit) => Field::Value(unit),
            None => Field::Unsupported,
        },
        other => map_failure(other),
    };
    let threads = read_threads(&directory);
    let namespaces = read_namespaces(&directory);

    Process {
        pid,
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

fn read_bytes(path: PathBuf) -> Field<Vec<u8>> {
    match fs::read(path) {
        Ok(value) => Field::Value(value),
        Err(error) => field_error(error),
    }
}

fn field_error<T>(error: io::Error) -> Field<T> {
    match error.kind() {
        io::ErrorKind::PermissionDenied => Field::PermissionDenied,
        io::ErrorKind::NotFound => Field::Vanished,
        io::ErrorKind::Unsupported => Field::Unsupported,
        _ => Field::IoError,
    }
}

fn map_failure<T, U>(field: &Field<T>) -> Field<U> {
    match field {
        Field::Value(_) => Field::ParseError,
        Field::NotCollected => Field::NotCollected,
        Field::PermissionDenied => Field::PermissionDenied,
        Field::Vanished => Field::Vanished,
        Field::Unsupported => Field::Unsupported,
        Field::ParseError => Field::ParseError,
        Field::IoError => Field::IoError,
    }
}

fn parsed_number(source: &Field<Vec<u8>>, key: &[u8]) -> Field<u64> {
    match source {
        Field::Value(bytes) => parse_status_number(bytes, key)
            .map(Field::Value)
            .unwrap_or(Field::ParseError),
        other => map_failure(other),
    }
}

fn read_threads(process: &Path) -> Field<Vec<Thread>> {
    let entries = match fs::read_dir(process.join("task")) {
        Ok(v) => v,
        Err(e) => return field_error(e),
    };
    let mut threads = Vec::new();
    for entry in entries.flatten() {
        let Some(tid) = entry
            .file_name()
            .to_str()
            .and_then(|v| v.parse::<u32>().ok())
        else {
            continue;
        };
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
    threads.sort_unstable_by_key(|thread| thread.tid);
    Field::Value(threads)
}
fn read_namespaces(process: &Path) -> Field<Vec<(String, String)>> {
    let mut values = Vec::new();
    for name in ["pid", "mnt", "net", "user", "cgroup"] {
        match fs::read_link(process.join("ns").join(name)) {
            Ok(value) => values.push((name.into(), value.to_string_lossy().into_owned())),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                return Field::PermissionDenied;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Field::Vanished,
            Err(_) => return Field::IoError,
        }
    }
    Field::Value(values)
}

struct ParsedStat {
    name: String,
    state: char,
    ppid: u32,
    user_ticks: u64,
    system_ticks: u64,
    start_ticks: u64,
}

fn parse_stat(bytes: &[u8]) -> Option<ParsedStat> {
    let open = bytes.iter().position(|byte| *byte == b'(')?;
    let close = bytes.iter().rposition(|byte| *byte == b')')?;
    if close <= open {
        return None;
    }
    let name = String::from_utf8_lossy(&bytes[open + 1..close]).into_owned();
    let remainder = std::str::from_utf8(bytes.get(close + 2..)?).ok()?;
    let fields: Vec<&str> = remainder.split_whitespace().collect();
    Some(ParsedStat {
        name,
        state: fields.first()?.chars().next()?,
        ppid: fields.get(1)?.parse().ok()?,
        user_ticks: fields.get(11)?.parse().ok()?,
        system_ticks: fields.get(12)?.parse().ok()?,
        start_ticks: fields.get(19)?.parse().ok()?,
    })
}

fn parse_cmdline(bytes: &[u8]) -> String {
    let arguments: Vec<String> = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    if arguments.is_empty() {
        "[kernel thread or empty command]".into()
    } else {
        arguments.join(" ")
    }
}

fn parse_status_number(bytes: &[u8], key: &[u8]) -> Option<u64> {
    bytes
        .split(|byte| *byte == b'\n')
        .find(|line| line.starts_with(key))
        .and_then(|line| std::str::from_utf8(&line[key.len()..]).ok())
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

fn parse_system_cpu(bytes: &[u8]) -> (Option<u64>, usize) {
    let mut total = None;
    let mut count = 0;
    for line in bytes.split(|b| *b == b'\n') {
        if line.starts_with(b"cpu ") {
            total = std::str::from_utf8(&line[4..]).ok().and_then(|s| {
                // guest and guest_nice are already included in user and nice.
                // Linux CPU totals therefore use only the first eight counters.
                s.split_whitespace().take(8).try_fold(0_u64, |sum, v| {
                    v.parse::<u64>().ok().and_then(|n| sum.checked_add(n))
                })
            });
        } else if line.starts_with(b"cpu") && line.get(3).is_some_and(u8::is_ascii_digit) {
            count += 1;
        }
    }
    (total, count)
}

fn parse_memory(bytes: &[u8]) -> Memory {
    Memory {
        total_kib: parse_status_number(bytes, b"MemTotal:"),
        available_kib: parse_status_number(bytes, b"MemAvailable:"),
        swap_total_kib: parse_status_number(bytes, b"SwapTotal:"),
        swap_free_kib: parse_status_number(bytes, b"SwapFree:"),
    }
}

fn parse_cgroup(bytes: &[u8]) -> String {
    bytes
        .split(|byte| *byte == b'\n')
        .find_map(|line| line.splitn(3, |byte| *byte == b':').nth(2))
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .unwrap_or_default()
}

fn find_systemd_unit(path: &str) -> Option<String> {
    path.split('/')
        .rev()
        .find(|part| {
            [".service", ".scope", ".slice"]
                .iter()
                .any(|suffix| part.ends_with(suffix))
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_parser_handles_spaces_and_parentheses_in_name() {
        let input = b"42 (odd) process) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21";
        let stat = parse_stat(input).unwrap();
        assert_eq!(stat.name, "odd) process");
        assert_eq!(stat.state, 'S');
        assert_eq!(stat.ppid, 1);
        assert_eq!(stat.user_ticks, 11);
        assert_eq!(stat.system_ticks, 12);
        assert_eq!(stat.start_ticks, 19);
    }

    #[test]
    fn command_line_uses_nul_boundaries() {
        assert_eq!(parse_cmdline(b"hello\0two words\0"), "hello two words");
    }

    #[test]
    fn reads_status_values() {
        let status = b"Name:\tx\nUid:\t1000\t1000\nVmRSS:\t1234 kB\n";
        assert_eq!(parse_status_number(status, b"Uid:"), Some(1000));
        assert_eq!(parse_status_number(status, b"VmRSS:"), Some(1234));
    }

    #[test]
    fn reads_system_totals() {
        assert_eq!(
            parse_system_cpu(b"cpu  1 2 3 4\ncpu0 1\ncpu1 1\n"),
            (Some(10), 2)
        );
        assert_eq!(
            parse_system_cpu(b"cpu  1 2 3 4 5 6 7 8 900 1000\ncpu0 1\n"),
            (Some(36), 1)
        );
        assert_eq!(
            parse_memory(b"MemTotal: 100 kB\nMemAvailable: 60 kB\n").available_kib,
            Some(60)
        );
    }

    #[test]
    fn reads_cgroup_and_unit() {
        let path = parse_cgroup(b"0::/user.slice/user-1000.slice/app.slice/foot.service\n");
        assert_eq!(path, "/user.slice/user-1000.slice/app.slice/foot.service");
        assert_eq!(find_systemd_unit(&path).as_deref(), Some("foot.service"));
    }

    #[test]
    fn synthetic_proc_tree_collects_many_processes_and_tolerates_missing_fields() {
        let root =
            std::env::temp_dir().join(format!("whats-running-fixture-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("stat"), b"cpu 1 2 3 4\ncpu0 1\n").unwrap();
        fs::write(
            root.join("meminfo"),
            b"MemTotal: 100 kB\nMemAvailable: 50 kB\n",
        )
        .unwrap();
        for pid in 1..=2_000_u32 {
            let dir = root.join(pid.to_string());
            fs::create_dir(&dir).unwrap();
            fs::write(
                dir.join("stat"),
                format!("{pid} (fixture {pid}) S 0 0 0 0 0 0 0 0 0 0 1 2 0 0 0 0 0 0 3 0"),
            )
            .unwrap();
            fs::write(dir.join("status"), b"Uid: 1000\nVmRSS: 4 kB\n").unwrap();
            fs::write(dir.join("cmdline"), b"fixture\0").unwrap();
            fs::write(dir.join("io"), b"read_bytes: 10\nwrite_bytes: 20\n").unwrap();
        }
        let snapshot = collect_from(&root, 100, false).unwrap();
        assert_eq!(snapshot.processes.len(), 2_000);
        assert!(
            snapshot.duration_micros < 250_000,
            "collection exceeded the initial 250 ms budget: {} us",
            snapshot.duration_micros
        );
        assert_eq!(snapshot.processes[99].name.value().unwrap(), "fixture 100");
        assert_eq!(snapshot.processes[0].executable.status(), "vanished");
        assert_eq!(snapshot.processes[0].command.status(), "not_collected");
        fs::remove_dir_all(root).unwrap();
    }
}
