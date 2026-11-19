// ==============================================================================
// Program: main.rs
// Programmer: Ethan-da-Tech-Wizard
// Last Modified: November 18, 2026
// Purpose: Main entry point and CLI option dispatcher for What's Running.
//          Parses command-line arguments, orchestrates execution modes including
//          table view, streaming JSON, interactive TUI, native Iced GUI,
//          quick check summaries, inventory reporting, and event recordings.
// ==============================================================================

/// Module map: procfs = data collection from /proc; activity = sockets/mounts/cgroups/systemd inventory;
/// events = Netlink proc-connector capture; recording = JSONL writer; check = quick report;
/// tui/gui = interactive front ends. This file only parses arguments and formats CLI output.
mod activity;
mod check;
mod events;
mod gui;
mod procfs;
mod recording;
mod tui;

use procfs::{Field, Process, Snapshot, collect_snapshot};
// VecDeque holds the bounded sample history; HashMap/HashSet key processes by (pid, start_ticks).
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy)]
/// Output style for the one-shot/sampling CLI modes.
enum Format {
    /// Human-readable fixed-width table (default).
    Table,
    /// Machine-readable JSON, selected with --json.
    Json,
}

/// Parsed command-line options. Defaults are set in `parse_args`.
struct Options {
    format: Format,
    /// Opt-in (--show-command): command lines may contain secrets, so they are not collected by default.
    show_command: bool,
    /// Number of snapshots to take; CPU/IO rates need at least two.
    samples: usize,
    /// Delay between samples, and the refresh period in TUI mode.
    interval_ms: u64,
    tui: bool,
    gui: bool,
    /// Some(n): capture n process lifecycle events from the kernel instead of snapshotting.
    events: Option<usize>,
    inventory: bool,
    check: bool,
    /// Path to write captured events as JSONL (needs --events).
    record: Option<String>,
    /// Hard cap on recording size; default is 10 MiB.
    max_record_bytes: u64,
}

/// Thin wrapper: all logic lives in `run`, which returns errors as plain strings that are printed
/// to stderr here, giving a consistent `whats-running: <message>` format and exit status.
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("whats-running: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Dispatches on the selected mode. Order matters: GUI, TUI, event capture, quick check, inventory,
/// then the default sampling loop. The first matching mode wins and returns.
fn run() -> Result<(), String> {
    // parse_args returns None when it already handled --help/--version.
    let Some(options) = parse_args()? else {
        return Ok(());
    };
    if options.gui {
        return gui::run().map_err(|e| e.to_string());
    }
    // Reject incompatible flag combinations before doing any work.
    if options.tui && options.record.is_some() {
        return Err("--record is for finite --events capture, not TUI mode".into());
    }
    if options.tui {
        return tui::run(options.interval_ms, options.show_command, options.events)
            .map_err(|error| error.to_string());
    }
    if options.record.is_some() && options.events.is_none() {
        return Err("--record currently requires --events N".into());
    }
    if let Some(limit) = options.events {
        // Event mode takes a snapshot before and after the capture window. Comparing them (`changes`)
        // reconciles what the kernel events reported with what actually appeared/disappeared, exposing
        // missed events. Commands are not collected (false).
        let before = collect_snapshot(false).map_err(|e| e.to_string())?;
        let capture =
            // 5 s is the receive timeout: capture ends early if no event arrives within that long.
            events::capture(limit, Duration::from_secs(5)).map_err(|error| error.to_string())?;
        let after = collect_snapshot(false).map_err(|e| e.to_string())?;
        // (started, exited) process counts seen between the two snapshots.
        let reconciliation = changes(&after, Some(&before));
        if let Some(path) = options.record {
            // write_events caps the file at --max-record-bytes and reports how many events actually fit.
            let (count, bytes) = recording::write_events(
                std::path::Path::new(&path),
                &capture.events,
                options.max_record_bytes,
            )
            .map_err(|e| e.to_string())?;
            println!("recorded {count} events ({bytes} bytes) to {path}");
        } else {
            println!(
                "{{\"record_type\":\"coverage\",\"source\":\"linux-proc-connector\",\"events\":{},\"sequence_gaps\":{},\"reconciled_started\":{},\"reconciled_exited\":{},\"observer_pid\":{}}}",
                capture.events.len(),
                capture.sequence_gaps,
                reconciliation.0,
                reconciliation.1,
                after.observer_pid
            );
            for event in capture.events {
                println!("{}", event.json());
            }
        }
        return Ok(());
    }
    if options.check {
        let snapshot = collect_snapshot(false).map_err(|e| e.to_string())?;
        let inventory = activity::collect(&snapshot);
        check::report(&snapshot, &inventory);
        return Ok(());
    }
    if options.inventory {
        let snapshot = collect_snapshot(false).map_err(|e| e.to_string())?;
        let inventory = activity::collect(&snapshot);
        if matches!(options.format, Format::Json) {
            print_inventory_json(&snapshot, &inventory);
            return Ok(());
        }
        // "boundary" reminds readers that results reflect only what this user/namespace can see.
        println!("Background inventory (boundary: current namespaces and permissions)");
        println!(
            "processes={} threads={} associated_units={} system_units={} user_units={} sockets={} cgroups={} mounts={} system_timers={} user_timers={}",
            snapshot.processes.len(),
            snapshot
                .processes
                .iter()
                .filter_map(|p| p.thread_count.copied())
                .sum::<u64>(),
            inventory.units.len(),
            count_field(&inventory.system_units),
            count_field(&inventory.user_units),
            count_field(&inventory.sockets),
            display_field(&inventory.cgroup_count),
            count_field(&inventory.mounts),
            count_field(&inventory.system_timers),
            count_field(&inventory.user_timers)
        );
        if let Field::Value(sockets) = inventory.sockets {
            // Text inventory lists only LISTEN sockets (state 0A) and at most 50 entries per section
            // to keep the output readable; use --json for the complete data. Note that UDP sockets, having no
            // LISTEN state, are therefore not shown here.
            for socket in sockets.iter().filter(|s| s.state == "0A").take(50) {
                println!(
                    "listen {:<5} {:<30} remote={:<30} inode={:?} owners={:?} path={}",
                    socket.protocol,
                    socket.local,
                    socket.remote,
                    socket.inode,
                    socket.owners,
                    socket.path.as_deref().unwrap_or("-")
                );
            }
        }
        if let Field::Value(mounts) = inventory.mounts {
            for mount in mounts.iter().take(50) {
                println!(
                    "mount {:<12} {:<30} source={}",
                    mount.fs_type, mount.mount_point, mount.source
                );
            }
        }
        if let Field::Value(cgroups) = inventory.cgroups {
            for cgroup in cgroups.iter().take(50) {
                println!("cgroup {cgroup}");
            }
        }
        return Ok(());
    }
    // Capped so output and the in-memory history stay bounded.
    if options.samples > 120 {
        return Err("--samples is capped at 120 to bound memory and output".into());
    }
    // Only the most recent snapshot is needed for rates (`history.back()`); the deque is kept
    // bounded in case more history is used later.
    let mut history: VecDeque<Snapshot> = VecDeque::with_capacity(120);
    if matches!(options.format, Format::Json) {
        // Streaming JSON: the array is written incrementally, one sample at a time, so output appears
        // while sampling; commas are emitted between samples below.
        println!("{{\n  \"schema_version\": 3,\n  \"samples\": [");
    }
    for index in 0..options.samples {
        let snapshot = collect_snapshot(options.show_command).map_err(|error| error.to_string())?;
        // None on the first sample, in which case rates print as unavailable (`?` / null).
        let previous = history.back();
        match options.format {
            Format::Table => print_table(&snapshot, previous, options.show_command),
            Format::Json => {
                print_json(&snapshot, previous, options.show_command);
                // Trailing comma only between samples, so the JSON array stays valid.
                println!("{}", if index + 1 < options.samples { "," } else { "" });
            }
        }
        history.push_back(snapshot);
        if history.len() > 120 {
            history.pop_front();
        }
        if index + 1 < options.samples {
            thread::sleep(Duration::from_millis(options.interval_ms));
            if matches!(options.format, Format::Table) {
                println!();
            }
        }
    }
    if matches!(options.format, Format::Json) {
        println!("  ]\n}}");
    }
    Ok(())
}
/// Hand-writes the inventory as JSON (no serde). Each section is either {status:"value",value:..}
/// or {status:"<reason>"} so consumers can tell "empty" from "not permitted".
fn print_inventory_json(snapshot: &Snapshot, inventory: &activity::Inventory) {
    println!(
        "{{\"schema_version\":1,\"record_type\":\"background_inventory\",\"observer_pid\":{},\"boundary\":\"current namespaces and permissions\",",
        snapshot.observer_pid
    );
    print!("\"associated_units\":");
    json_string_vec(&inventory.units);
    println!(",");
    print!("\"system_units\":");
    json_field_vec(&inventory.system_units);
    println!(",");
    print!("\"user_units\":");
    json_field_vec(&inventory.user_units);
    println!(",");
    print!("\"system_timers\":");
    json_field_vec(&inventory.system_timers);
    println!(",");
    print!("\"user_timers\":");
    json_field_vec(&inventory.user_timers);
    println!(",");
    print!("\"cgroups\":");
    json_field_vec(&inventory.cgroups);
    println!(",");
    print!("\"sockets\":");
    match &inventory.sockets {
        Field::Value(values) => {
            println!("{{\"status\":\"value\",\"value\":[");
            for (index, socket) in values.iter().enumerate() {
                print!(
                    "{{\"protocol\":\"{}\",\"local\":\"{}\",\"remote\":\"{}\",\"state\":\"{}\",\"inode\":{},\"owners\":[{}],\"path\":{}}}",
                    socket.protocol,
                    json_escape(&socket.local),
                    json_escape(&socket.remote),
                    json_escape(&socket.state),
                    number(socket.inode),
                    socket
                        .owners
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    socket
                        .path
                        .as_ref()
                        .map(|v| format!("\"{}\"", json_escape(v)))
                        .unwrap_or_else(|| "null".into())
                );
                println!("{}", if index + 1 < values.len() { "," } else { "" });
            }
            print!("]}}")
        }
        other => print!("{{\"status\":\"{}\"}}", other.status()),
    }
    println!(",");
    print!("\"mounts\":");
    match &inventory.mounts {
        Field::Value(values) => {
            println!("{{\"status\":\"value\",\"value\":[");
            for (index, mount) in values.iter().enumerate() {
                print!(
                    "{{\"mount_point\":\"{}\",\"fs_type\":\"{}\",\"source\":\"{}\"}}",
                    json_escape(&mount.mount_point),
                    json_escape(&mount.fs_type),
                    json_escape(&mount.source)
                );
                println!("{}", if index + 1 < values.len() { "," } else { "" });
            }
            print!("]}}")
        }
        other => print!("{{\"status\":\"{}\"}}", other.status()),
    }
    println!("}}");
}
/// Prints a JSON array of escaped strings without a trailing newline (callers add punctuation).
fn json_string_vec(values: &[String]) {
    print!("[");
    for (index, value) in values.iter().enumerate() {
        print!(
            "\"{}\"{}",
            json_escape(value),
            if index + 1 < values.len() { "," } else { "" }
        );
    }
    print!("]")
}
fn json_field_vec(field: &Field<Vec<String>>) {
    match field {
        Field::Value(values) => {
            print!("{{\"status\":\"value\",\"value\":");
            json_string_vec(values);
            print!("}}")
        }
        other => print!("{{\"status\":\"{}\"}}", other.status()),
    }
}

/// Minimal hand-rolled parser. Returns Ok(None) after printing --help/--version, Err on bad input.
fn parse_args() -> Result<Option<Options>, String> {
    let mut options = Options {
        format: Format::Table,
        show_command: false,
        samples: 1,
        interval_ms: 1000,
        tui: false,
        gui: false,
        events: None,
        inventory: false,
        check: false,
        record: None,
        max_record_bytes: 10 * 1024 * 1024,
    };
    // skip(1) drops the program name.
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => options.format = Format::Json,
            "--show-command" => options.show_command = true,
            "--samples" => options.samples = positive(args.next(), "--samples")?,
            "--interval-ms" => options.interval_ms = positive(args.next(), "--interval-ms")? as u64,
            "--gui" => options.gui = true,
            // Boolean flags just set a field; value flags consume the next argument via `positive`.
            "--tui" => options.tui = true,
            "--events" => options.events = Some(positive(args.next(), "--events")?),
            "--inventory" => options.inventory = true,
            "--check" => options.check = true,
            "--record" => {
                options.record = Some(
                    args.next()
                        .ok_or_else(|| "--record requires a path".to_string())?,
                )
            }
            "--max-record-bytes" => {
                options.max_record_bytes = positive(args.next(), "--max-record-bytes")? as u64
            }
            "-h" | "--help" => {
                println!("{}", help());
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("whats-running {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            // Unknown options are errors rather than ignored, so typos don't silently change behavior.
            _ => return Err(format!("unknown option {arg:?}; try --help")),
        }
    }
    Ok(Some(options))
}

/// Parses a required strictly-positive integer option value, with an error message naming the option.
fn positive(value: Option<String>, option: &str) -> Result<usize, String> {
    let value = value
        .ok_or_else(|| format!("{option} requires a value"))?
        .parse::<usize>()
        .map_err(|_| format!("invalid {option} value"))?;
    if value == 0 {
        Err(format!("{option} must be positive"))
    } else {
        Ok(value)
    }
}

/// Usage text; keep in sync with the options handled in parse_args.
fn help() -> &'static str {
    "What's Running? — honest local background-activity visibility\n\nUSAGE:\n    whats-running [--check | --tui | --inventory | --events N | --json] [OPTIONS]\n\nOPTIONS:\n    --check                Print running processes and listening ports as plain text, then exit\n    --tui                  Open the interactive multi-domain terminal interface\n    --inventory            Inventory units, sockets, cgroups, mounts, and timers\n    --events N             Capture up to N kernel lifecycle events (may need elevation)\n    --record PATH          Privately record captured events; requires --events\n    --max-record-bytes N   Hard recording byte cap (default: 10485760)\n    --json                 Emit versioned JSON instead of a table\n    --show-command         Explicitly collect command lines (may reveal secrets)\n    --samples N            Take N snapshots; hard cap 120 (default: 1)\n    --interval-ms N        Milliseconds between snapshots (default: 1000)\n    -h, --help             Show this help\n    -V, --version          Show the version\n\nPRIVACY:\n    Command-line files are not opened unless --show-command is supplied. Environment\n    variables are never collected. Recordings are mode 0600, bounded, and command-free.\n\nCOMPLETENESS:\n    Coverage is relative to enabled collectors, namespaces, permissions, event loss,\n    and sampling windows. Optional event capture never elevates itself or invokes sudo."
}

/// Prints one snapshot as a table; `previous` (if any) enables CPU% and IO rate columns.
fn print_table(snapshot: &Snapshot, previous: Option<&Snapshot>, show_command: bool) {
    // Computed once per table, then looked up per row.
    let rates = rates(snapshot, previous);
    let io_rates = io_rates(snapshot, previous);
    let (started, exited) = changes(snapshot, previous);
    println!(
        "What's Running?  observer_pid={}  processes={}  errors={}  collected={} ms",
        snapshot.observer_pid,
        snapshot.processes.len(),
        snapshot.enumeration_errors,
        snapshot.duration_micros as f64 / 1000.0
    );
    // Honesty note shown to users: processes are read one at a time, so the picture is not instantaneous,
    // and containers/namespaces limit what is visible.
    println!("boundary: visible /proc PID namespace; snapshot is non-atomic");
    println!(
        "system: cpus={} memory={} / {} MiB swap={} / {} MiB observed +{} -{}",
        snapshot.cpu_count,
        used(snapshot.memory.total_kib, snapshot.memory.available_kib),
        mib(snapshot.memory.total_kib),
        used(
            snapshot.memory.swap_total_kib,
            snapshot.memory.swap_free_kib
        ),
        mib(snapshot.memory.swap_total_kib),
        started,
        exited
    );
    println!(
        "{:<7} {:<7} {:<12} {:<7} {:>10} {:>7} {:>9} {:>9} {:<24} {}",
        "PID",
        "PPID",
        "USER",
        "STATE",
        "RSS KiB",
        "CPU%",
        "READ/s",
        "WRITE/s",
        "NAME",
        if show_command {
            "COMMAND"
        } else {
            "VISIBILITY"
        }
    );

    for process in &snapshot.processes {
        let user = process
            .uid
            .copied()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into());
        let ppid = process
            .ppid
            .copied()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into());
        let rss = process
            .rss_kib
            .copied()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into());
        // Process names are attacker-controlled; sanitize() replaces control characters so a name
        // cannot inject terminal escape sequences.
        let name = sanitize(process.name.value().map_or("?", String::as_str));
        let tail = if show_command {
            field_display(&process.command)
        } else {
            process.visibility_summary()
        };
        println!(
            "{:<7} {:<7} {:<12} {:<7} {:>10} {:>7} {:>9} {:>9} {:<24} {}{}",
            process.pid,
            ppid,
            user,
            // `?` is the display convention for any value that could not be read.
            process.state.copied().unwrap_or('?'),
            rss,
            rates
                // Rates are keyed by (pid, start time), not pid alone, so a reused PID is never confused with the old process.
                .get(&(process.pid, process.start_ticks.copied()))
                .map(|v| format!("{v:.1}"))
                .unwrap_or_else(|| "?".into()),
            io_rates
                .get(&(process.pid, process.start_ticks.copied()))
                .and_then(|v| v.0)
                .map(bytes_rate)
                .unwrap_or_else(|| "?".into()),
            io_rates
                .get(&(process.pid, process.start_ticks.copied()))
                .and_then(|v| v.1)
                .map(bytes_rate)
                .unwrap_or_else(|| "?".into()),
            truncate(&name, 24),
            sanitize(&tail),
            if process.pid == snapshot.observer_pid {
                // Marks the row for this program's own process.
                "  [THIS TOOL]"
            } else {
                ""
            }
        );
    }
}

/// JSON counterpart of print_table; one object per sample inside the array opened in `run`.
fn print_json(snapshot: &Snapshot, previous: Option<&Snapshot>, show_command: bool) {
    // Rate maps are computed once per snapshot, then looked up per process.
    let rate_map = rates(snapshot, previous);
    let io_rate_map = io_rates(snapshot, previous);
    let (started, exited) = changes(snapshot, previous);
    println!("    {{");
    println!("  \"observer_pid\": {},", snapshot.observer_pid);
    println!("  \"started_unix_ms\": {},", snapshot.started_unix_ms);
    println!("  \"duration_micros\": {},", snapshot.duration_micros);
    println!("  \"observation_boundary\": \"visible /proc PID namespace; non-atomic snapshot\",");
    println!("  \"enumeration_errors\": {},", snapshot.enumeration_errors);
    println!("  \"command_lines_included\": {},", show_command);
    println!(
        "  \"system\": {{\"cpu_count\":{},\"memory_total_kib\":{},\"memory_available_kib\":{},\"swap_total_kib\":{},\"swap_free_kib\":{},\"observed_started\":{},\"observed_exited\":{}}},",
        snapshot.cpu_count,
        number(snapshot.memory.total_kib),
        number(snapshot.memory.available_kib),
        number(snapshot.memory.swap_total_kib),
        number(snapshot.memory.swap_free_kib),
        started,
        exited
    );
    println!("  \"processes\": [");
    for (index, process) in snapshot.processes.iter().enumerate() {
        print_process_json(
            process,
            show_command,
            process.pid == snapshot.observer_pid,
            rate_map
                .get(&(process.pid, process.start_ticks.copied()))
                .copied(),
            io_rate_map
                .get(&(process.pid, process.start_ticks.copied()))
                .copied(),
        );
        println!(
            "{}",
            if index + 1 == snapshot.processes.len() {
                ""
            } else {
                ","
            }
        );
    }
    println!("  ]");
    print!("    }}");
}

/// Emits one process as a JSON object. Each field is {status, value?} (see Field) so unreadable data is
/// explicit rather than zero or missing.
fn print_process_json(
    process: &Process,
    show_command: bool,
    observer: bool,
    cpu_percent: Option<f64>,
    io_rate: Option<(Option<f64>, Option<f64>)>,
) {
    println!("    {{");
    println!("      \"pid\": {},", process.pid);
    json_typed_number("ppid", &process.ppid, true);
    json_typed_number("uid", &process.uid, true);
    json_typed_string("state", &process.state, true);
    json_field("name", &process.name, true);
    json_field("executable", &process.executable, true);
    json_typed_number("start_ticks", &process.start_ticks, true);
    json_typed_number("cpu_user_ticks", &process.cpu_user_ticks, true);
    json_typed_number("cpu_system_ticks", &process.cpu_system_ticks, true);
    json_typed_number("rss_kib", &process.rss_kib, true);
    println!(
        "      \"cpu_percent\": {},",
        cpu_percent
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "null".into())
    );
    json_typed_number("read_bytes", &process.read_bytes, true);
    json_typed_number("write_bytes", &process.write_bytes, true);
    json_typed_number("thread_count", &process.thread_count, true);
    json_field("cgroup", &process.cgroup, true);
    json_field("systemd_unit", &process.systemd_unit, true);
    // threads and namespaces are nested structures with their own helpers; they print without a newline.
    print!("      \"threads\": ");
    print_threads_json(&process.threads);
    println!(",");
    print!("      \"namespaces\": ");
    print_namespaces_json(&process.namespaces);
    println!(",");
    println!(
        "      \"read_bytes_per_second\": {},",
        decimal(io_rate.and_then(|v| v.0))
    );
    println!(
        "      \"write_bytes_per_second\": {},",
        decimal(io_rate.and_then(|v| v.1))
    );
    println!(
        "      \"rate_status\": \"{}\",",
        if cpu_percent.is_some() {
            "value"
        } else {
            "warming_up_or_unavailable"
        }
    );
    if show_command {
        json_field("command", &process.command, true);
    } else {
        println!("      \"command\": {{\"status\":\"not_collected\"}},");
    }
    println!("      \"is_observer\": {observer}");
    print!("    }}");
}

fn print_threads_json(threads: &Field<Vec<procfs::Thread>>) {
    match threads {
        Field::Value(threads) => {
            println!("{{\"status\":\"value\",\"value\":[");
            for (index, thread) in threads.iter().enumerate() {
                print!(
                    "        {{\"tid\":{},\"name\":\"{}\",\"state\":\"{}\",\"start_ticks\":{},\"cpu_user_ticks\":{},\"cpu_system_ticks\":{}}}",
                    thread.tid,
                    json_escape(&thread.name),
                    thread.state,
                    thread.start_ticks,
                    thread.cpu_user_ticks,
                    thread.cpu_system_ticks
                );
                println!("{}", if index + 1 < threads.len() { "," } else { "" });
            }
            print!("      ]}}")
        }
        other => print!("{{\"status\":\"{}\"}}", other.status()),
    }
}
fn print_namespaces_json(namespaces: &Field<Vec<(String, String)>>) {
    match namespaces {
        Field::Value(values) => {
            print!("{{\"status\":\"value\",\"value\":{{");
            for (index, (name, value)) in values.iter().enumerate() {
                print!(
                    "\"{}\":\"{}\"{}",
                    json_escape(name),
                    json_escape(value),
                    if index + 1 < values.len() { "," } else { "" }
                );
            }
            print!("}}}}")
        }
        other => print!("{{\"status\":\"{}\"}}", other.status()),
    }
}

/// CPU% per process between two snapshots: delta(user+system ticks) / delta(total system ticks)
/// * cpu_count * 100. This is per-core percent, like `top`, so a busy multi-thread process can exceed 100.
/// Entries are omitted (never guessed) when counters are missing, went backwards, or the process is new.
fn rates(snapshot: &Snapshot, previous: Option<&Snapshot>) -> HashMap<(u32, Option<u64>), f64> {
    // Starts empty: every early `return out` below means "rates unavailable" and callers print `?`/null.
    let mut out = HashMap::new();
    let Some(previous) = previous else { return out };
    // checked_sub guards against counters going backwards; total == 0 would divide by zero.
    let Some(total) = snapshot
        .system_cpu_ticks
        .zip(previous.system_cpu_ticks)
        .and_then(|(a, b)| a.checked_sub(b))
    else {
        return out;
    };
    if total == 0 {
        return out;
    }
    // Index the previous snapshot by identity for O(1) matching of each current process.
    let old: HashMap<_, _> = previous
        .processes
        .iter()
        .map(|p| ((p.pid, p.start_ticks.copied()), p))
        .collect();
    for p in &snapshot.processes {
        let Some(before) = old.get(&(p.pid, p.start_ticks.copied())) else {
            continue;
        };
        let now = p
            .cpu_user_ticks
            .copied()
            .zip(p.cpu_system_ticks.copied())
            .and_then(|(a, b)| a.checked_add(b));
        let then = before
            .cpu_user_ticks
            .copied()
            .zip(before.cpu_system_ticks.copied())
            .and_then(|(a, b)| a.checked_add(b));
        if let Some(delta) = now.zip(then).and_then(|(a, b)| a.checked_sub(b)) {
            out.insert(
                (p.pid, p.start_ticks.copied()),
                // Share of total CPU time scaled by core count so 100.0 equals one fully used core.
                delta as f64 / total as f64 * snapshot.cpu_count.max(1) as f64 * 100.0,
            );
        }
    }
    out
}

/// Returns (started, exited): identities present only in the new / only in the old snapshot.
/// Identity includes start time, so PID reuse counts as one exit plus one start.
fn changes(snapshot: &Snapshot, previous: Option<&Snapshot>) -> (usize, usize) {
    let Some(previous) = previous else {
        return (0, 0);
    };
    let now: HashSet<_> = snapshot
        .processes
        .iter()
        .map(|p| (p.pid, p.start_ticks.copied()))
        .collect();
    let old: HashSet<_> = previous
        .processes
        .iter()
        .map(|p| (p.pid, p.start_ticks.copied()))
        .collect();
    (now.difference(&old).count(), old.difference(&now).count())
}

/// (read bytes/s, write bytes/s), each None if that counter was unavailable.
type IoRates = HashMap<(u32, Option<u64>), (Option<f64>, Option<f64>)>;
/// Rates use wall-clock time between snapshot starts (started_unix_ms), saturating so a clock step
/// backwards yields zero seconds and no rates instead of nonsense.
fn io_rates(snapshot: &Snapshot, previous: Option<&Snapshot>) -> IoRates {
    let mut out = HashMap::new();
    let Some(previous) = previous else { return out };
    let seconds = snapshot
        .started_unix_ms
        .saturating_sub(previous.started_unix_ms) as f64
        / 1000.0;
    if seconds <= 0.0 {
        return out;
    }
    let old: HashMap<_, _> = previous
        .processes
        .iter()
        .map(|p| ((p.pid, p.start_ticks.copied()), p))
        .collect();
    for p in &snapshot.processes {
        if let Some(before) = old.get(&(p.pid, p.start_ticks.copied())) {
            // Rate helper: (new - old) / seconds, None if either counter is missing or went backwards.
            let rate = |a: Option<u64>, b: Option<u64>| {
                a.zip(b)
                    .and_then(|(a, b)| a.checked_sub(b))
                    .map(|v| v as f64 / seconds)
            };
            out.insert(
                (p.pid, p.start_ticks.copied()),
                (
                    rate(p.read_bytes.copied(), before.read_bytes.copied()),
                    rate(p.write_bytes.copied(), before.write_bytes.copied()),
                ),
            );
        }
    }
    out
}
/// The helpers below format Option values for display: "?" in tables, "null" in JSON.
fn mib(v: Option<u64>) -> String {
    v.map(|v| format!("{:.1}", v as f64 / 1024.0))
        .unwrap_or_else(|| "?".into())
}
/// Used memory in MiB = total - available (KiB -> MiB).
fn used(t: Option<u64>, a: Option<u64>) -> String {
    t.zip(a)
        .and_then(|(t, a)| t.checked_sub(a))
        .map(|v| format!("{:.1}", v as f64 / 1024.0))
        .unwrap_or_else(|| "?".into())
}
fn number(v: Option<u64>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "null".into())
}
fn decimal(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.3}"))
        .unwrap_or_else(|| "null".into())
}
/// Compact rate with binary units: bytes, K (1024), M (1024^2).
fn bytes_rate(v: f64) -> String {
    if v >= 1_048_576.0 {
        format!("{:.1}M", v / 1_048_576.0)
    } else if v >= 1024.0 {
        format!("{:.1}K", v / 1024.0)
    } else {
        format!("{v:.0}")
    }
}
/// Display helpers: show a count (or the Field failure status as `<status>`) instead of failing.
fn count_field<T>(field: &Field<Vec<T>>) -> String {
    match field {
        Field::Value(v) => v.len().to_string(),
        other => format!("<{}>", other.status()),
    }
}
fn display_field<T: std::fmt::Display>(field: &Field<T>) -> String {
    match field {
        Field::Value(v) => v.to_string(),
        other => format!("<{}>", other.status()),
    }
}

/// The json_* helpers print one `"name": {"status":..., "value":...}` entry each. `name` values are
/// internal literals so they are not escaped; untrusted strings go through json_escape.
fn json_field(name: &str, field: &Field<String>, comma: bool) {
    match field {
        Field::Value(value) => println!(
            "      \"{}\": {{\"status\":\"value\",\"value\":\"{}\"}}{}",
            name,
            json_escape(value),
            comma_suffix(comma)
        ),
        other => println!(
            "      \"{}\": {{\"status\":\"{}\"}}{}",
            name,
            other.status(),
            comma_suffix(comma)
        ),
    }
}

fn json_typed_number<T: std::fmt::Display>(name: &str, field: &Field<T>, comma: bool) {
    match field {
        Field::Value(value) => println!(
            "      \"{name}\": {{\"status\":\"value\",\"value\":{value}}}{}",
            comma_suffix(comma)
        ),
        other => println!(
            "      \"{name}\": {{\"status\":\"{}\"}}{}",
            other.status(),
            comma_suffix(comma)
        ),
    }
}
fn json_typed_string<T: std::fmt::Display>(name: &str, field: &Field<T>, comma: bool) {
    match field {
        Field::Value(value) => println!(
            "      \"{name}\": {{\"status\":\"value\",\"value\":\"{}\"}}{}",
            json_escape(&value.to_string()),
            comma_suffix(comma)
        ),
        other => println!(
            "      \"{name}\": {{\"status\":\"{}\"}}{}",
            other.status(),
            comma_suffix(comma)
        ),
    }
}

/// Lets the same JSON field printers be used for the last field (no comma) and the others.
fn comma_suffix(comma: bool) -> &'static str {
    if comma { "," } else { "" }
}

/// Plain-text form of a string field for table output (callers sanitize before printing).
fn field_display(field: &Field<String>) -> String {
    match field {
        Field::Value(value) => value.clone(),
        other => format!("<{}>", other.status()),
    }
}

/// Replaces control characters (ESC, NUL, newlines...) with U+FFFD. Applied to all untrusted text
/// before it reaches a terminal, to prevent escape-sequence injection and layout breakage.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

/// Truncates by characters (not bytes, so UTF-8 is never split) and marks the cut with an ellipsis.
fn truncate(value: &str, max_chars: usize) -> String {
    let mut result: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars && max_chars > 0 {
        result.pop();
        result.push('…');
    }
    result
}

/// Escapes per RFC 8259: quote, backslash, and control characters (short forms for \n \r \t,
/// \u00XX for the rest).
fn json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_text_is_sanitized() {
        assert_eq!(sanitize("safe\u{1b}[31m"), "safe�[31m");
    }

    #[test]
    fn json_text_is_escaped() {
        assert_eq!(json_escape("a\"\\\n"), "a\\\"\\\\\\n");
    }
}
