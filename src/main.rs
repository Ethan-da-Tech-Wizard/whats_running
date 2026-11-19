// ==============================================================================
// Program: main.rs
// Programmer: Ethan-da-Tech-Wizard
// Last Modified: November 18, 2026
// Purpose: Main entry point and CLI option dispatcher for What's Running.
//          Parses command-line arguments, orchestrates execution modes including
//          table view, streaming JSON, interactive TUI, native Iced GUI,
//          quick check summaries, inventory reporting, and event recordings.
// ==============================================================================

// File role: this is the CLI front door. It owns argument parsing, the one-shot/sampling output
// formats (table and hand-written JSON) and the rate maths shared by both. Interactive modes
// (tui.rs, gui.rs) and the collectors (procfs.rs, activity.rs, events.rs) are separate modules.
//
// Design decisions:
//   - No serde and no clap: the JSON schema is small and fixed, so it is written by hand, which keeps
//     the dependency list short and lets every string pass through `json_escape`/`sanitize` explicitly.
//   - Every value that can fail to read is printed with its status (`?` in tables, `{"status":...}`
//     in JSON) rather than as zero, so the output never claims data it does not have.
//
// Testing: `cargo test --release` runs the two unit tests at the bottom plus those in procfs.rs;
// try it by hand with `cargo run --release -- --check` or `-- --json --samples 2`.
/// Module map: procfs = data collection from /proc; activity = sockets/mounts/cgroups/systemd inventory;
/// events = Netlink proc-connector capture; recording = JSONL writer; check = quick report;
/// tui/gui = interactive front ends. This file only parses arguments and formats CLI output.
// Declaration order is alphabetical (rustfmt sorts it); the dependency direction is main -> {tui, gui, check}
// -> {activity, events, recording} -> procfs, and procfs depends on nothing in this crate.
mod activity;
mod check;
mod events;
mod gui;

// procfs is private to the crate: other modules reach it as `crate::procfs`, and the public surface of
// the program is only the command line.
mod procfs;
mod recording;
mod tui;

// Field/Process/Snapshot are the shared data model; collect_snapshot is the only call that touches /proc.
use procfs::{Field, Process, Snapshot, collect_snapshot};
// VecDeque holds the bounded sample history; HashMap/HashSet key processes by (pid, start_ticks).
use std::collections::{HashMap, HashSet, VecDeque};

// env reads argv in parse_args; thread::sleep paces the sampling loop; ExitCode carries the process status out of main.
use std::env;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

// Copy: Format is a two-variant tag that is matched in several places, so passing it by value is simplest.
#[derive(Clone, Copy)]
// Selected by --json; there is no format flag for the interactive modes because they render themselves.
/// Output style for the one-shot/sampling CLI modes.
enum Format {
    /// Human-readable fixed-width table (default).
    Table,
    /// Machine-readable JSON, selected with --json.
    Json,
}

/// Parsed command-line options. Defaults are set in `parse_args`.
struct Options {
    /// Table or JSON; `--json` switches to JSON. Applies to the sampling mode and `--inventory`.
    format: Format,
    /// Opt-in (--show-command): command lines may contain secrets, so they are not collected by default.
    // Forwarded to collect_snapshot, tui::run and the table/JSON printers so every path honours the same privacy opt-in.
    show_command: bool,
    /// Number of snapshots to take; CPU/IO rates need at least two.
    // usize (not u32) because it is used directly as a loop bound and for history capacity; capped at 120 in `run`.
    samples: usize,
    /// Delay between samples, and the refresh period in TUI mode.
    // Milliseconds, not Duration, so the value maps one-to-one onto the --interval-ms flag and the TUI timer.
    interval_ms: u64,
    /// `--tui`: run the terminal UI (tui.rs) instead of printing and exiting.
    tui: bool,
    /// `--gui`: open the native window (gui.rs); checked first in `run`, so it wins over every other mode flag.
    gui: bool,
    /// Some(n): capture n process lifecycle events from the kernel instead of snapshotting.
    // None means "do not capture"; Some(n) both enables event mode and bounds it, so there is no separate boolean.
    events: Option<usize>,
    /// `--inventory`: print the unit/socket/cgroup/mount/timer inventory (activity.rs) instead of a process table.
    inventory: bool,
    /// `--check`: short plain-text report of processes and listening ports (check.rs), meant for quick human use.
    check: bool,
    /// Path to write captured events as JSONL (needs --events).
    record: Option<String>,
    /// Hard cap on recording size; default is 10 MiB.
    // u64 because recordings may in principle exceed 4 GiB on 32-bit usize targets; this is a byte budget, not an index.
    max_record_bytes: u64,
}

/// Thin wrapper: all logic lives in `run`, which returns errors as plain strings that are printed
/// to stderr here, giving a consistent `whats-running: <message>` format and exit status.
// ExitCode (not process::exit) so destructors run and stdout is flushed before the process ends.
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        // Errors are plain strings by design: every layer maps its own error type with `to_string()` at the call
        // site, so main never needs to know about io::Error vs. netlink errors.
        Err(message) => {
            eprintln!("whats-running: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Dispatches on the selected mode. Order matters: GUI, TUI, event capture, quick check, inventory,
/// then the default sampling loop. The first matching mode wins and returns.
// Control flow summary (first match wins, each branch returns):
//   --gui          -> native window                      (gui::run)
//   --tui          -> terminal UI                        (tui::run)
//   --events N     -> kernel lifecycle capture + report  (events::capture, optional recording)
//   --check        -> short plain-text report            (check::report)
//   --inventory    -> units/sockets/cgroups/mounts       (activity::collect)
//   otherwise      -> N timed snapshots as table or JSON (print_table / print_json)
//
// Returns Err(String) for bad flag combinations and collector failures; main prints it and exits 1.
fn run() -> Result<(), String> {
    // parse_args returns None when it already handled --help/--version.
    // Mode precedence is explicit (see doc comment): there is no "combined" mode, so an earlier flag silently
    // ignores later ones except where an incompatible pair is rejected below.
    let Some(options) = parse_args()? else {
        // A None from parse_args (help or version already printed) is a success exit, not an error.
        return Ok(());
    };

    // The GUI owns its own event loop and refresh timer, so the sampling options (--samples, --interval-ms, --json) have no effect on it.
    if options.gui {
        return gui::run().map_err(|e| e.to_string());
    }
    // Reject incompatible flag combinations before doing any work.
    // --record only makes sense for a finite capture: the TUI runs until the user quits, so the size cap and end of file would be undefined.
    if options.tui && options.record.is_some() {
        return Err("--record is for finite --events capture, not TUI mode".into());
    }
    if options.tui {
        // interval_ms doubles as the TUI refresh period; `events` lets the TUI show captured lifecycle events if requested.
        return tui::run(options.interval_ms, options.show_command, options.events)
            .map_err(|error| error.to_string());
    }

    // Recording stores kernel events; without --events there is nothing to record, and silently doing nothing would look like success.
    if options.record.is_some() && options.events.is_none() {
        return Err("--record currently requires --events N".into());
    }
    if let Some(limit) = options.events {
        // Event mode takes a snapshot before and after the capture window. Comparing them (`changes`)
        // reconciles what the kernel events reported with what actually appeared/disappeared, exposing
        // missed events. Commands are not collected (false).
        // First of two snapshots bracketing the capture window; commands are skipped because event mode never needs them.
        let before = collect_snapshot(false).map_err(|e| e.to_string())?;

        // Blocking call: opens a netlink socket and waits for up to `limit` events (needs privileges on most kernels).
        let capture =
            // 5 s is the receive timeout: capture ends early if no event arrives within that long.
            events::capture(limit, Duration::from_secs(5)).map_err(|error| error.to_string())?;

        // Taken after the capture ends so differences with `before` can be checked against the events received.
        let after = collect_snapshot(false).map_err(|e| e.to_string())?;
        // (started, exited) process counts seen between the two snapshots.
        // Reconciliation = processes that appeared/disappeared between the two snapshots. If it exceeds the number of
        // events captured, the kernel (or the receive buffer) dropped some, which `sequence_gaps` also reports.
        let reconciliation = changes(&after, Some(&before));
        if let Some(path) = options.record {
            // write_events caps the file at --max-record-bytes and reports how many events actually fit.
            // Counts are returned so the message tells the user whether the cap truncated the capture.
            let (count, bytes) = recording::write_events(
                std::path::Path::new(&path),
                &capture.events,
                options.max_record_bytes,
            )
            .map_err(|e| e.to_string())?;

            // `count` can be lower than --events N if the byte cap was hit; the user sees both numbers.
            println!("recorded {count} events ({bytes} bytes) to {path}");
        } else {
            println!(
                // First line of event output is a coverage record so a consumer knows how complete the stream is
                // before reading it. It is JSON Lines: one object per line, no enclosing array.
                "{{\"record_type\":\"coverage\",\"source\":\"linux-proc-connector\",\"events\":{},\"sequence_gaps\":{},\"reconciled_started\":{},\"reconciled_exited\":{},\"observer_pid\":{}}}",
                capture.events.len(),
                capture.sequence_gaps,
                reconciliation.0,
                reconciliation.1,
                after.observer_pid
            );

            // Each event is already a single-line JSON object (event.json), so it can be streamed straight to stdout.
            for event in capture.events {
                println!("{}", event.json());
            }
        }
        return Ok(());
    }

    // check mode never collects command lines (false): it is the privacy-safe default for a quick look.
    if options.check {
        let snapshot = collect_snapshot(false).map_err(|e| e.to_string())?;

        // Inventory collection runs external listings (systemd, /proc/net, mountinfo); each part fails independently.
        let inventory = activity::collect(&snapshot);

        // report() prints directly to stdout and has no return value, so there is nothing to map here.
        check::report(&snapshot, &inventory);
        return Ok(());
    }

    // Inventory is a separate collector (activity.rs) layered on a snapshot: it maps processes to systemd
    // units, sockets, cgroups and mounts, and reports which of those the current user was allowed to read.
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
            // Thread total is computed here (not stored) because only some rows may have a readable Threads: value.
            snapshot.processes.len(),
            snapshot
                .processes
                .iter()
                .filter_map(|p| p.thread_count.copied())
                .sum::<u64>(),
            // associated_units counts units found from the processes' cgroups; system/user_units count what systemctl lists.
            inventory.units.len(),
            count_field(&inventory.system_units),
            count_field(&inventory.user_units),
            count_field(&inventory.sockets),
            display_field(&inventory.cgroup_count),
            count_field(&inventory.mounts),
            count_field(&inventory.system_timers),
            count_field(&inventory.user_timers)
        );

        // Only a readable socket list is printed; when it is not readable the summary line above already shows `<status>`.
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

        // Same 50-entry cap as sockets; a typical desktop has hundreds of mounts, mostly kernel pseudo-filesystems.
        if let Field::Value(mounts) = inventory.mounts {
            for mount in mounts.iter().take(50) {
                println!(
                    "mount {:<12} {:<30} source={}",
                    mount.fs_type, mount.mount_point, mount.source
                );
            }
        }

        // cgroup lines are plain paths, so a single `cgroup <path>` line each is enough and needs no column formatting.
        if let Field::Value(cgroups) = inventory.cgroups {
            for cgroup in cgroups.iter().take(50) {
                println!("cgroup {cgroup}");
            }
        }
        return Ok(());
    }

    // 120 samples at the 1 s default is two minutes: enough to see a trend, small enough to bound output size.
    // Capped so output and the in-memory history stay bounded.
    // Checked after the other modes so the cap only matters for the sampling loop that actually uses it.
    if options.samples > 120 {
        return Err("--samples is capped at 120 to bound memory and output".into());
    }
    // Only the most recent snapshot is needed for rates (`history.back()`); the deque is kept
    // bounded in case more history is used later.
    // with_capacity(120) allocates once up front so pushing snapshots in the loop never reallocates.
    // schema_version 3 is the sampling JSON (bump when a field changes meaning). The opening brackets are printed
    // before the first sample so a reader sees valid streaming output even if a later sample fails.
    let mut history: VecDeque<Snapshot> = VecDeque::with_capacity(120);
    if matches!(options.format, Format::Json) {
        // Streaming JSON: the array is written incrementally, one sample at a time, so output appears
        // while sampling; commas are emitted between samples below.
        println!("{{\n  \"schema_version\": 3,\n  \"samples\": [");
    }

    // `index` is needed (not iterating the history) only to decide separators: commas, sleeps and blank lines.
    for index in 0..options.samples {
        // show_command flows into collection itself, so command lines are never even read unless opted in.
        let snapshot = collect_snapshot(options.show_command).map_err(|error| error.to_string())?;
        // None on the first sample, in which case rates print as unavailable (`?` / null).
        // Borrowed before push_back below: `previous` is the sample taken one interval ago, which is what the rate maths needs.
        let previous = history.back();
        match options.format {
            // Table output is for humans, so it gets blank lines between samples; JSON gets commas.
            Format::Table => print_table(&snapshot, previous, options.show_command),
            Format::Json => {
                print_json(&snapshot, previous, options.show_command);
                // Trailing comma only between samples, so the JSON array stays valid.
                println!("{}", if index + 1 < options.samples { "," } else { "" });
            }
        }

        // Push after printing so `previous` above never aliases the snapshot being printed.
        history.push_back(snapshot);

        // Unreachable while samples <= 120, but keeps the invariant local to the data structure if the cap is raised.
        if history.len() > 120 {
            history.pop_front();
        }
        if index + 1 < options.samples {
            // Sleep only between samples (never after the last) so a single --samples 1 run returns immediately.
            thread::sleep(Duration::from_millis(options.interval_ms));
            if matches!(options.format, Format::Table) {
                println!();
            }
        }
    }
    if matches!(options.format, Format::Json) {
        // Close the array and object opened before the loop; reached only if every sample succeeded.
        println!("  ]\n}}");
    }
    Ok(())
}
/// Hand-writes the inventory as JSON (no serde). Each section is either {status:"value",value:..}
/// or {status:"<reason>"} so consumers can tell "empty" from "not permitted".
// Schema: top-level keys are associated_units, system_units, user_units, system_timers, user_timers,
// cgroups, sockets, mounts. `schema_version` is 1 and independent of the sampling JSON's version.
//
// Printed with many small print!/println! calls instead of building a String, which keeps memory flat
// for inventories with thousands of sockets.
fn print_inventory_json(snapshot: &Snapshot, inventory: &activity::Inventory) {
    println!(
        "{{\"schema_version\":1,\"record_type\":\"background_inventory\",\"observer_pid\":{},\"boundary\":\"current namespaces and permissions\",",
        snapshot.observer_pid
    );

    // Each section prints its own key, then either a {status,value} wrapper (json_field_vec) or a custom array.
    print!("\"associated_units\":");
    json_string_vec(&inventory.units);
    println!(",");

    // Units and timers are Field<Vec<String>> because listing them needs systemctl, which may be missing or denied.
    print!("\"system_units\":");
    json_field_vec(&inventory.system_units);
    println!(",");

    // system/user variants are separate because `systemctl --system` and `--user` list different managers.
    print!("\"user_units\":");
    json_field_vec(&inventory.user_units);
    println!(",");
    print!("\"system_timers\":");
    json_field_vec(&inventory.system_timers);
    println!(",");
    print!("\"user_timers\":");
    json_field_vec(&inventory.user_timers);
    println!(",");

    // A flat list of cgroup paths; for the counts see `cgroup_count` in the text inventory.
    print!("\"cgroups\":");
    json_field_vec(&inventory.cgroups);
    println!(",");
    print!("\"sockets\":");
    match &inventory.sockets {
        Field::Value(values) => {
            // Opening of a status-wrapped array; the matching `]}}` is printed after the loop, with the last element comma-free.
            println!("{{\"status\":\"value\",\"value\":[");
            for (index, socket) in values.iter().enumerate() {
                print!(
                    // Sockets get a hand-written object because they mix strings, an optional inode (null) and a list of owner PIDs.
                    "{{\"protocol\":\"{}\",\"local\":\"{}\",\"remote\":\"{}\",\"state\":\"{}\",\"inode\":{},\"owners\":[{}],\"path\":{}}}",
                    socket.protocol,
                    json_escape(&socket.local),
                    json_escape(&socket.remote),
                    json_escape(&socket.state),
                    number(socket.inode),
                    socket
                        .owners
                        .iter()
                        // Owner PIDs come from matching the socket inode against /proc/<pid>/fd; empty means the owner was not readable.
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

        // Any failure becomes `{"status":"<reason>"}` with no value key, so consumers can tell "unreadable" from "empty list".
        other => print!("{{\"status\":\"{}\"}}", other.status()),
    }
    println!(",");

    // Same {status,value} wrapper as sockets, but each mount object has only three string fields, all escaped.
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

    // Closes the top-level object opened by the first println! of this function.
    println!("}}");
}
/// Prints a JSON array of escaped strings without a trailing newline (callers add punctuation).
fn json_string_vec(values: &[String]) {
    print!("[");

    // enumerate() is used only to omit the comma after the last element, which JSON forbids.
    for (index, value) in values.iter().enumerate() {
        print!(
            "\"{}\"{}",
            json_escape(value),
            if index + 1 < values.len() { "," } else { "" }
        );
    }

    // No newline: the caller decides what punctuation follows (comma, brace) so these helpers compose.
    print!("]")
}

/// Prints `{"status":"value","value":[...]}` for a readable list, or `{"status":"<reason>"}` otherwise.
// Used for plain string lists (units, timers, cgroups); sockets and mounts have their own object printers above.
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
// Walks argv once. Flags that take a value read the next argument through `positive`/`ok_or_else`, so a flag
// at the end of the line produces "requires a value" instead of a panic.
// Returns Ok(None) after printing --help or --version, Ok(Some(options)) to continue, Err on bad input.
// Validation that depends on several flags together (for example --record without --events) happens in
// `run`, because only there are all flags known.
fn parse_args() -> Result<Option<Options>, String> {
    // Defaults: one table sample, 1 s interval (matches `top`'s default feel), commands off, 10 MiB recording cap.
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
            // Simple flags set a field and continue the loop; there is no short-flag clustering (-abc) on purpose.
            "--json" => options.format = Format::Json,
            "--show-command" => options.show_command = true,
            // Value flags are validated as strictly positive so 0 (which would print nothing or loop forever) is impossible.
            "--samples" => options.samples = positive(args.next(), "--samples")?,
            "--interval-ms" => options.interval_ms = positive(args.next(), "--interval-ms")? as u64,
            "--gui" => options.gui = true,
            // Boolean flags just set a field; value flags consume the next argument via `positive`.
            "--tui" => options.tui = true,
            "--events" => options.events = Some(positive(args.next(), "--events")?),
            "--inventory" => options.inventory = true,
            "--check" => options.check = true,
            // Only the path is checked here; the file is created later by recording::write_events with restrictive permissions.
            "--record" => {
                options.record = Some(
                    args.next()
                        .ok_or_else(|| "--record requires a path".to_string())?,
                )
            }

            // `as u64` widens from usize; the value is only an upper bound for the recording file size.
            "--max-record-bytes" => {
                options.max_record_bytes = positive(args.next(), "--max-record-bytes")? as u64
            }

            // Help and version are handled while parsing so they work even if later arguments are invalid.
            "-h" | "--help" => {
                println!("{}", help());
                return Ok(None);
            }

            // The version comes from Cargo.toml at compile time, so it can never disagree with the package metadata.
            "-V" | "--version" => {
                println!("whats-running {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            // Unknown options are errors rather than ignored, so typos don't silently change behavior.
            _ => return Err(format!("unknown option {arg:?}; try --help")),
        }
    }

    // Some(options) = proceed to run; every mode flag is validated lazily in `run` where the combination is known.
    Ok(Some(options))
}

/// Parses a required strictly-positive integer option value, with an error message naming the option.
// Used for --samples, --interval-ms, --events and --max-record-bytes; all of them must be at least 1.
fn positive(value: Option<String>, option: &str) -> Result<usize, String> {
    // Three failure messages: missing, not a number (including negative, which fails usize parsing), and zero.
    let value = value
        .ok_or_else(|| format!("{option} requires a value"))?
        .parse::<usize>()
        .map_err(|_| format!("invalid {option} value"))?;

    // parse::<usize> accepts "0"; it is rejected here so 0 samples / 0 ms intervals cannot slip through.
    if value == 0 {
        Err(format!("{option} must be positive"))
    } else {
        Ok(value)
    }
}

/// Usage text; keep in sync with the options handled in parse_args.
// A single string literal (not a printed table) so --help output is identical everywhere and testable.
fn help() -> &'static str {
    "What's Running? — honest local background-activity visibility\n\nUSAGE:\n    whats-running [--check | --tui | --inventory | --events N | --json] [OPTIONS]\n\nOPTIONS:\n    --check                Print running processes and listening ports as plain text, then exit\n    --tui                  Open the interactive multi-domain terminal interface\n    --inventory            Inventory units, sockets, cgroups, mounts, and timers\n    --events N             Capture up to N kernel lifecycle events (may need elevation)\n    --record PATH          Privately record captured events; requires --events\n    --max-record-bytes N   Hard recording byte cap (default: 10485760)\n    --json                 Emit versioned JSON instead of a table\n    --show-command         Explicitly collect command lines (may reveal secrets)\n    --samples N            Take N snapshots; hard cap 120 (default: 1)\n    --interval-ms N        Milliseconds between snapshots (default: 1000)\n    -h, --help             Show this help\n    -V, --version          Show the version\n\nPRIVACY:\n    Command-line files are not opened unless --show-command is supplied. Environment\n    variables are never collected. Recordings are mode 0600, bounded, and command-free.\n\nCOMPLETENESS:\n    Coverage is relative to enabled collectors, namespaces, permissions, event loss,\n    and sampling windows. Optional event capture never elevates itself or invokes sudo."
}

/// Prints one snapshot as a table; `previous` (if any) enables CPU% and IO rate columns.
// Layout (fixed-width columns, left-aligned text, right-aligned numbers):
//   header line   observer pid, process count, scan errors and scan time
//   boundary line what the data can and cannot claim
//   system line   CPUs, memory and swap used / total in MiB, processes started/exited since last sample
//   column header
//   one row per process
//
// Every untrusted string is passed through `sanitize` before printing.
fn print_table(snapshot: &Snapshot, previous: Option<&Snapshot>, show_command: bool) {
    // Computed once per table, then looked up per row.
    // Both rate maps are built once up front (O(n)) rather than per row, which would make printing O(n^2).
    // Shadowing the function name with its result is intentional: after this line only the map is needed.
    let rates = rates(snapshot, previous);

    // io_rates needs wall-clock time between snapshots, so it is a separate map from the CPU rates (which use CPU ticks).
    let io_rates = io_rates(snapshot, previous);

    // Shown in the header as `observed +N -M`: processes that appeared or vanished since the previous sample.
    let (started, exited) = changes(snapshot, previous);
    println!(
        // Header line: errors counts /proc entries that could not even be listed; collected is the scan time in ms.
        "What's Running?  observer_pid={}  processes={}  errors={}  collected={} ms",
        snapshot.observer_pid,
        snapshot.processes.len(),
        snapshot.enumeration_errors,
        snapshot.duration_micros as f64 / 1000.0
    );
    // Honesty note shown to users: processes are read one at a time, so the picture is not instantaneous,
    // and containers/namespaces limit what is visible.
    // Plain statement of the observation limits, repeated in JSON as `observation_boundary`.
    println!("boundary: visible /proc PID namespace; snapshot is non-atomic");
    println!(
        // used() and mib() print `?` for unreadable meminfo values; swap shows 0 / 0 on machines without swap.
        "system: cpus={} memory={} / {} MiB swap={} / {} MiB observed +{} -{}",
        snapshot.cpu_count,
        used(snapshot.memory.total_kib, snapshot.memory.available_kib),
        mib(snapshot.memory.total_kib),
        used(
            snapshot.memory.swap_total_kib,
            snapshot.memory.swap_free_kib
        ),
        mib(snapshot.memory.swap_total_kib),
        // observed_started / observed_exited: the same churn counters as the table header, 0 on the first sample.
        started,
        exited
    );
    println!(
        "{:<7} {:<7} {:<12} {:<7} {:>10} {:>7} {:>9} {:>9} {:<24} {}",
        // Column header; the widths must match the row format string below or the table misaligns.
        // The last column switches between COMMAND and VISIBILITY because both would be too wide to show together.
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

    // One row per process in pid order (procfs sorts them), so rows stay put between samples.
    for process in &snapshot.processes {
        // Numbers are rendered via `copied()` so any unreadable field prints `?` instead of 0.
        let user = process
            .uid
            .copied()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into());

        // Same "?" convention for every numeric cell: an unreadable value must never look like a real 0.
        // `?` is also the display for a process whose stat file could not be read (exited or permission denied).
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
        // map_or("?", String::as_str): borrow the name if present, else the placeholder, without allocating.
        let name = sanitize(process.name.value().map_or("?", String::as_str));

        // The tail column is either the command line (opt-in) or the explanation of what could not be read.
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
            // Unit is KiB straight from VmRSS, matching the RSS KiB column header.
            rss,
            rates
                // Rates are keyed by (pid, start time), not pid alone, so a reused PID is never confused with the old process.
                // Key = (pid, start_ticks): PIDs get reused, so the start time disambiguates a new process from the one that held the pid before.
                .get(&(process.pid, process.start_ticks.copied()))
                .map(|v| format!("{v:.1}"))
                .unwrap_or_else(|| "?".into()),
            // I/O rates: the pair is (read, write); `v.0` / `v.1` pick the column. Each is "?" until two samples exist.
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
            // 24 characters matches the column width so long names never push the tail column out of line.
            truncate(&name, 24),
            // Sanitised again here because the tail can be a command line, which is user-controlled text.
            sanitize(&tail),
            // The tool lists itself like any other process; the marker prevents users from mistaking its own load for background activity.
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
// Schema (version 3, printed by `run`): observer info, timing, system totals, then one object per process.
// Each process field is {status, value?}; rates are top-level numbers or null while warming up.
// Emits one element of the "samples" array (the surrounding brackets and the commas are written by `run`).
// Streaming: nothing is buffered beyond one process object, so memory use does not grow with process count.
fn print_json(snapshot: &Snapshot, previous: Option<&Snapshot>, show_command: bool) {
    // Rate maps are computed once per snapshot, then looked up per process.
    let rate_map = rates(snapshot, previous);
    let io_rate_map = io_rates(snapshot, previous);
    let (started, exited) = changes(snapshot, previous);

    // Opening brace of the process object; field order is fixed so diffs between runs stay readable.
    println!("    {{");

    println!("  \"observer_pid\": {},", snapshot.observer_pid);

    // Timing fields let consumers align samples on a timeline; started_unix_ms is wall clock, duration_micros is monotonic.
    println!("  \"started_unix_ms\": {},", snapshot.started_unix_ms);
    println!("  \"duration_micros\": {},", snapshot.duration_micros);

    // Machine-readable copy of the boundary note, so consumers cannot mistake the data for an atomic snapshot.
    println!("  \"observation_boundary\": \"visible /proc PID namespace; non-atomic snapshot\",");

    // Non-zero means some /proc directories could not be listed at all, so the process count is a lower bound.
    println!("  \"enumeration_errors\": {},", snapshot.enumeration_errors);

    // States explicitly whether commands were collected, so a missing "command" value is never ambiguous.
    println!("  \"command_lines_included\": {},", show_command);
    println!(
        "  \"system\": {{\"cpu_count\":{},\"memory_total_kib\":{},\"memory_available_kib\":{},\"swap_total_kib\":{},\"swap_free_kib\":{},\"observed_started\":{},\"observed_exited\":{}}},",
        snapshot.cpu_count,
        // number() prints null when /proc/meminfo lacked the line, so JSON consumers can tell "unknown" from 0.
        number(snapshot.memory.total_kib),
        number(snapshot.memory.available_kib),
        number(snapshot.memory.swap_total_kib),
        number(snapshot.memory.swap_free_kib),
        started,
        exited
    );

    // Processes are streamed one at a time so a 1000-process sample does not need an in-memory string.
    println!("  \"processes\": [");
    for (index, process) in snapshot.processes.iter().enumerate() {
        // The fourth and fifth arguments are the rates for this process, looked up by identity; None for new processes.
        print_process_json(
            process,
            show_command,
            process.pid == snapshot.observer_pid,
            // Rate lookups use `.copied()` because the maps hold plain f64 values and the printers want Option<f64>.
            rate_map
                .get(&(process.pid, process.start_ticks.copied()))
                .copied(),
            io_rate_map
                .get(&(process.pid, process.start_ticks.copied()))
                .copied(),
        );
        println!(
            "{}",
            // Same comma-between-elements rule as in `run`, applied per process object.
            if index + 1 == snapshot.processes.len() {
                ""
            } else {
                ","
            }
        );
    }

    // Closes the processes array; the enclosing object is closed by the print! below.
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
    // Process objects are indented four spaces and their fields six, matching the sample object (two) one level up.
    println!("    {{");
    println!("      \"pid\": {},", process.pid);

    // Fields before `cpu_percent` come straight from the snapshot; `true` means "followed by another field" (comma).
    json_typed_number("ppid", &process.ppid, true);
    json_typed_number("uid", &process.uid, true);
    json_typed_string("state", &process.state, true);
    json_field("name", &process.name, true);
    json_field("executable", &process.executable, true);
    json_typed_number("start_ticks", &process.start_ticks, true);

    // Raw tick counts are exported next to the derived percentage so consumers can recompute rates with their own window.
    json_typed_number("cpu_user_ticks", &process.cpu_user_ticks, true);
    json_typed_number("cpu_system_ticks", &process.cpu_system_ticks, true);
    json_typed_number("rss_kib", &process.rss_kib, true);
    println!(
        // Three decimals: enough resolution for idle processes (0.003 %) without bloating the output.
        "      \"cpu_percent\": {},",
        cpu_percent
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "null".into())
    );

    // Cumulative counters since process start (unlike the per-second rates printed below).
    json_typed_number("read_bytes", &process.read_bytes, true);
    json_typed_number("write_bytes", &process.write_bytes, true);
    json_typed_number("thread_count", &process.thread_count, true);

    // cgroup and systemd_unit locate the process in the service tree; systemd_unit is derived text, not asked from systemd.
    json_field("cgroup", &process.cgroup, true);
    json_field("systemd_unit", &process.systemd_unit, true);
    // threads and namespaces are nested structures with their own helpers; they print without a newline.
    // Nested structures print inline and finish with `,` here because their helpers emit no trailing punctuation.
    print!("      \"threads\": ");
    print_threads_json(&process.threads);
    println!(",");
    print!("      \"namespaces\": ");
    print_namespaces_json(&process.namespaces);
    println!(",");
    println!(
        // decimal() prints null for a missing rate; combined with rate_status below, consumers can distinguish causes.
        "      \"read_bytes_per_second\": {},",
        decimal(io_rate.and_then(|v| v.0))
    );
    println!(
        "      \"write_bytes_per_second\": {},",
        decimal(io_rate.and_then(|v| v.1))
    );
    println!(
        // `warming_up_or_unavailable` covers both the first sample and processes that appeared between samples.
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
        // Explicit not_collected instead of omitting the key keeps the schema identical with and without --show-command.
        println!("      \"command\": {{\"status\":\"not_collected\"}},");
    }

    // Last field: no trailing comma, which is why it is printed by hand instead of through json_field.
    println!("      \"is_observer\": {observer}");
    print!("    }}");
}

/// Prints the thread list as `{"status":"value","value":[{tid,name,state,...}]}` or just the failure status.
fn print_threads_json(threads: &Field<Vec<procfs::Thread>>) {
    match threads {
        Field::Value(threads) => {
            println!("{{\"status\":\"value\",\"value\":[");
            for (index, thread) in threads.iter().enumerate() {
                print!(
                    // Thread objects are compact one-liners; a process with hundreds of threads would otherwise dominate the output size.
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

/// Prints namespaces as an object (`{"pid":"pid:[...]", ...}`); equal values across processes mean a shared namespace.
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

            // `}}}}` prints two literal closing braces: the inner object of namespaces and the {status,value} wrapper.
            print!("}}}}")
        }
        other => print!("{{\"status\":\"{}\"}}", other.status()),
    }
}

/// CPU% per process between two snapshots: delta(user+system ticks) / delta(total system ticks)
/// multiplied by cpu_count * 100. This is per-core percent, like `top`, so a busy multi-thread process can exceed 100.
///
/// Entries are omitted (never guessed) when counters are missing, went backwards, or the process is new.
// Worked example: 4 CPUs, system tick delta 400 over the window, process used 100 ticks.
// share = 100 / 400 = 0.25 of all CPU time, times 4 cores = 1.0 core, times 100 = 100.0 %.
// That matches what `top` shows for one busy thread, and a process spread over 2 cores would show 200.0.
//
// Why ticks instead of wall-clock time: the system total comes from /proc/stat (the same clock the
// process counters use), so no CLK_TCK constant or timer drift enters the calculation.
fn rates(snapshot: &Snapshot, previous: Option<&Snapshot>) -> HashMap<(u32, Option<u64>), f64> {
    // Starts empty: every early `return out` below means "rates unavailable" and callers print `?`/null.
    // Key (pid, start_ticks) -> percent. Missing keys mean "no rate", which callers render as `?` or null.
    let mut out = HashMap::new();

    // First sample: there is nothing to difference against, so return the empty map.
    let Some(previous) = previous else { return out };
    // checked_sub guards against counters going backwards; total == 0 would divide by zero.
    // `total` is the system-wide tick delta between samples (all CPUs together); it is the denominator for every process.
    let Some(total) = snapshot
        .system_cpu_ticks
        // Both totals must be readable; otherwise CPU% is unavailable for the whole table rather than per process.
        .zip(previous.system_cpu_ticks)
        .and_then(|(a, b)| a.checked_sub(b))
    else {
        return out;
    };

    // No ticks elapsed (two samples within one clock tick, i.e. under 10 ms): a ratio would be 0/0.
    if total == 0 {
        return out;
    }
    // Index the previous snapshot by identity for O(1) matching of each current process.
    // Built once per call: looking each current process up in `previous.processes` linearly would be O(n^2).
    let old: HashMap<_, _> = previous
        .processes
        .iter()
        .map(|p| ((p.pid, p.start_ticks.copied()), p))
        .collect();

    // Iterates the new snapshot, not the old one: processes that exited have no current counters to compare.
    for p in &snapshot.processes {
        // A process missing from the old snapshot is new; without a baseline it gets no rate (omitted rather than guessed).
        let Some(before) = old.get(&(p.pid, p.start_ticks.copied())) else {
            continue;
        };

        // `now`/`then` are user+system ticks of the same process at the two snapshots; the Option chain yields None if either is unreadable.
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

        // A negative delta (counter reset) yields None and the process simply gets no rate.
        if let Some(delta) = now.zip(then).and_then(|(a, b)| a.checked_sub(b)) {
            out.insert(
                (p.pid, p.start_ticks.copied()),
                // Share of total CPU time scaled by core count so 100.0 equals one fully used core.
                // Example: on 4 cores a process that used 1 core fully gets delta/total = 0.25, times 4 cores times 100 = 100.0 %.
                delta as f64 / total as f64 * snapshot.cpu_count.max(1) as f64 * 100.0,
            );
        }
    }
    out
}

/// Returns (started, exited): identities present only in the new / only in the old snapshot.
/// Identity includes start time, so PID reuse counts as one exit plus one start.
// Cheap churn detector used in the table header and the JSON system block; it does not say *which*
// processes changed, only how many. Detailed lifecycle events come from the event capture mode.
fn changes(snapshot: &Snapshot, previous: Option<&Snapshot>) -> (usize, usize) {
    let Some(previous) = previous else {
        return (0, 0);
    };

    // Sets of identities, compared in both directions: new-only = started, old-only = exited.
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

    // Set difference both ways; PID reuse shows up as one start plus one exit because start_ticks differ.
    (now.difference(&old).count(), old.difference(&now).count())
}

/// (read bytes/s, write bytes/s), each None if that counter was unavailable.
// Tuple order is (read, write). Kept as a type alias because the nested generic is otherwise hard to read.
type IoRates = HashMap<(u32, Option<u64>), (Option<f64>, Option<f64>)>;
/// Rates use wall-clock time between snapshot starts (started_unix_ms), saturating so a clock step
/// backwards yields zero seconds and no rates instead of nonsense.
// Uses cumulative counters from /proc/<pid>/io: rate = (bytes_now - bytes_before) / seconds.
// Counters need two samples, so the first sample (and every new process) has no rate.
// The numbers measure bytes that reached storage, not page-cache hits (see procfs.rs read_bytes).
fn io_rates(snapshot: &Snapshot, previous: Option<&Snapshot>) -> IoRates {
    let mut out = HashMap::new();
    let Some(previous) = previous else { return out };

    // Wall-clock seconds between snapshot starts. Wall time (not ticks) is right here because I/O counters are bytes, not CPU time.
    let seconds = snapshot
        .started_unix_ms
        .saturating_sub(previous.started_unix_ms) as f64
        / 1000.0;

    // Zero or negative time (clock stepped back, or two snapshots in the same millisecond) would divide by zero or invert rates.
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
            // Closure captures `seconds`; the cumulative byte counters only go up, so a smaller new value means the process was replaced.
            // Evaluated per process and per direction, so the closure avoids repeating the zip/checked_sub chain four times.
            let rate = |a: Option<u64>, b: Option<u64>| {
                a.zip(b)
                    .and_then(|(a, b)| a.checked_sub(b))
                    .map(|v| v as f64 / seconds)
            };

            // Both directions are stored together so a process needs only one map lookup per printed row.
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
// KiB -> MiB with one decimal place; `?` when the kernel line was missing.
fn mib(v: Option<u64>) -> String {
    // Divide by 1024 twice overall: kernel reports KiB, display is MiB.
    v.map(|v| format!("{:.1}", v as f64 / 1024.0))
        .unwrap_or_else(|| "?".into())
}
/// Used memory in MiB = total - available (KiB -> MiB).
// Example: total 8_000_000 KiB, available 2_000_000 KiB -> "5859.4" (MiB in use). Uses MemAvailable, not
// MemFree, so reclaimable cache is not counted as "used".
fn used(t: Option<u64>, a: Option<u64>) -> String {
    // zip yields None if either side is unknown; checked_sub guards against available > total from inconsistent reads.
    t.zip(a)
        .and_then(|(t, a)| t.checked_sub(a))
        .map(|v| format!("{:.1}", v as f64 / 1024.0))
        .unwrap_or_else(|| "?".into())
}

/// Formats an optional integer for JSON: the number, or the literal `null`.
fn number(v: Option<u64>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "null".into())
}

/// Formats an optional float for JSON with three decimals, or `null`.
fn decimal(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.3}"))
        .unwrap_or_else(|| "null".into())
}
/// Compact rate with binary units: bytes, K (1024), M (1024^2).
// Example outputs: 512 -> "512", 2048.0 -> "2.0K", 3_145_728.0 -> "3.0M". Always bytes per second.
fn bytes_rate(v: f64) -> String {
    // Thresholds are powers of 1024 (binary units), matching the K/M suffixes in the doc comment.
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
        // Only the count is shown in the text summary; use --json for the list contents.
        Field::Value(v) => v.len().to_string(),
        // Angle brackets mark a failure reason (for example `<permission_denied>`) so it cannot be mistaken for a number.
        other => format!("<{}>", other.status()),
    }
}

/// Like `count_field` but for a single displayable value (for example the cgroup count).
fn display_field<T: std::fmt::Display>(field: &Field<T>) -> String {
    match field {
        Field::Value(v) => v.to_string(),
        other => format!("<{}>", other.status()),
    }
}

/// The json_* helpers print one `"name": {"status":..., "value":...}` entry each. `name` values are
/// internal literals so they are not escaped; untrusted strings go through json_escape.
// Strings go through json_escape because process names, paths and command lines are attacker-controlled.
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

/// For numeric-like payloads (`ppid`, `rss_kib`...): the value is printed unquoted so JSON consumers see a number.
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

/// For single-character or display payloads such as the process state: the value is printed as a quoted, escaped string.
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
    // The only reason the JSON helpers take `comma`: the last field in an object must not have one.
    if comma { "," } else { "" }
}

/// Plain-text form of a string field for table output (callers sanitize before printing).
fn field_display(field: &Field<String>) -> String {
    match field {
        // Cloned because the caller wants an owned String; the field itself stays borrowed from the snapshot.
        Field::Value(value) => value.clone(),
        other => format!("<{}>", other.status()),
    }
}

/// Replaces control characters (ESC, NUL, newlines...) with U+FFFD. Applied to all untrusted text
/// before it reaches a terminal, to prevent escape-sequence injection and layout breakage.
// Security: process names and command lines are chosen by whoever runs the process. Without this, a
// process named with an escape sequence could rewrite the user's terminal when listed.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            // U+FFFD replacement rather than deletion keeps column widths stable and makes the tampering visible.
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

/// Truncates by characters (not bytes, so UTF-8 is never split) and marks the cut with an ellipsis.
// Layout helper only; never applied to JSON output, where truncation would lose data.
fn truncate(value: &str, max_chars: usize) -> String {
    // take() keeps max_chars characters; the last one is then replaced by an ellipsis only if text was actually cut.
    let mut result: String = value.chars().take(max_chars).collect();

    // Counted in characters, not bytes, so a multi-byte name is never cut inside a UTF-8 sequence.
    if value.chars().count() > max_chars && max_chars > 0 {
        result.pop();
        result.push('…');
    }
    result
}

/// Escapes per RFC 8259: quote, backslash, and control characters (short forms for \n \r \t,
/// \u00XX for the rest).
// Hand-written because there is no serde. Output is always valid inside a double-quoted JSON string.
fn json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            // Order of arms matters: the control-character arm must come after the named escapes so \n is written as \n, not \u000a.
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            // RFC 8259 requires every control character below U+0020 to be escaped; `{:04x}` gives the \u00XX form.
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32))
            }

            // Everything else, including non-ASCII, is valid unescaped JSON (RFC 8259 permits UTF-8 directly).
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
// Both tests target the two functions that stand between untrusted /proc text and the output.
mod tests {
    use super::*;

    #[test]
    /// ESC (U+001B) must be replaced so a process named with an ANSI sequence cannot recolour or clear the user's terminal.
    fn terminal_text_is_sanitized() {
        assert_eq!(sanitize("safe\u{1b}[31m"), "safe�[31m");
    }

    #[test]
    /// Quote, backslash and newline must be escaped or a hostile process name could break out of its JSON string.
    fn json_text_is_escaped() {
        assert_eq!(json_escape("a\"\\\n"), "a\\\"\\\\\\n");
    }
}
