mod activity;
mod check;
mod events;
mod gui;
mod procfs;
mod recording;
mod tui;

use procfs::{Field, Process, Snapshot, collect_snapshot};
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy)]
enum Format {
    Table,
    Json,
}

struct Options {
    format: Format,
    show_command: bool,
    samples: usize,
    interval_ms: u64,
    tui: bool,
    gui: bool,
    events: Option<usize>,
    inventory: bool,
    check: bool,
    record: Option<String>,
    max_record_bytes: u64,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("whats-running: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let Some(options) = parse_args()? else {
        return Ok(());
    };
    if options.gui {
        return gui::run().map_err(|e| e.to_string());
    }
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
        let before = collect_snapshot(false).map_err(|e| e.to_string())?;
        let capture =
            events::capture(limit, Duration::from_secs(5)).map_err(|error| error.to_string())?;
        let after = collect_snapshot(false).map_err(|e| e.to_string())?;
        let reconciliation = changes(&after, Some(&before));
        if let Some(path) = options.record {
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
    if options.samples > 120 {
        return Err("--samples is capped at 120 to bound memory and output".into());
    }
    let mut history: VecDeque<Snapshot> = VecDeque::with_capacity(120);
    if matches!(options.format, Format::Json) {
        println!("{{\n  \"schema_version\": 3,\n  \"samples\": [");
    }
    for index in 0..options.samples {
        let snapshot = collect_snapshot(options.show_command).map_err(|error| error.to_string())?;
        let previous = history.back();
        match options.format {
            Format::Table => print_table(&snapshot, previous, options.show_command),
            Format::Json => {
                print_json(&snapshot, previous, options.show_command);
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
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => options.format = Format::Json,
            "--show-command" => options.show_command = true,
            "--samples" => options.samples = positive(args.next(), "--samples")?,
            "--interval-ms" => options.interval_ms = positive(args.next(), "--interval-ms")? as u64,
            "--gui" => options.gui = true,
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
            _ => return Err(format!("unknown option {arg:?}; try --help")),
        }
    }
    Ok(Some(options))
}

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

fn help() -> &'static str {
    "What's Running? — honest local background-activity visibility\n\nUSAGE:\n    whats-running [--check | --tui | --inventory | --events N | --json] [OPTIONS]\n\nOPTIONS:\n    --check                Print running processes and listening ports as plain text, then exit\n    --tui                  Open the interactive multi-domain terminal interface\n    --inventory            Inventory units, sockets, cgroups, mounts, and timers\n    --events N             Capture up to N kernel lifecycle events (may need elevation)\n    --record PATH          Privately record captured events; requires --events\n    --max-record-bytes N   Hard recording byte cap (default: 10485760)\n    --json                 Emit versioned JSON instead of a table\n    --show-command         Explicitly collect command lines (may reveal secrets)\n    --samples N            Take N snapshots; hard cap 120 (default: 1)\n    --interval-ms N        Milliseconds between snapshots (default: 1000)\n    -h, --help             Show this help\n    -V, --version          Show the version\n\nPRIVACY:\n    Command-line files are not opened unless --show-command is supplied. Environment\n    variables are never collected. Recordings are mode 0600, bounded, and command-free.\n\nCOMPLETENESS:\n    Coverage is relative to enabled collectors, namespaces, permissions, event loss,\n    and sampling windows. Optional event capture never elevates itself or invokes sudo."
}

fn print_table(snapshot: &Snapshot, previous: Option<&Snapshot>, show_command: bool) {
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
            process.state.copied().unwrap_or('?'),
            rss,
            rates
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
                "  [THIS TOOL]"
            } else {
                ""
            }
        );
    }
}

fn print_json(snapshot: &Snapshot, previous: Option<&Snapshot>, show_command: bool) {
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

fn rates(snapshot: &Snapshot, previous: Option<&Snapshot>) -> HashMap<(u32, Option<u64>), f64> {
    let mut out = HashMap::new();
    let Some(previous) = previous else { return out };
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
                delta as f64 / total as f64 * snapshot.cpu_count.max(1) as f64 * 100.0,
            );
        }
    }
    out
}

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

type IoRates = HashMap<(u32, Option<u64>), (Option<f64>, Option<f64>)>;
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
fn mib(v: Option<u64>) -> String {
    v.map(|v| format!("{:.1}", v as f64 / 1024.0))
        .unwrap_or_else(|| "?".into())
}
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
fn bytes_rate(v: f64) -> String {
    if v >= 1_048_576.0 {
        format!("{:.1}M", v / 1_048_576.0)
    } else if v >= 1024.0 {
        format!("{:.1}K", v / 1024.0)
    } else {
        format!("{v:.0}")
    }
}
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

fn comma_suffix(comma: bool) -> &'static str {
    if comma { "," } else { "" }
}

fn field_display(field: &Field<String>) -> String {
    match field {
        Field::Value(value) => value.clone(),
        other => format!("<{}>", other.status()),
    }
}

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

fn truncate(value: &str, max_chars: usize) -> String {
    let mut result: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars && max_chars > 0 {
        result.pop();
        result.push('…');
    }
    result
}

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
