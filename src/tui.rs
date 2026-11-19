// ==============================================================================
// Program: tui.rs
// Programmer: Ethan-da-Tech-Wizard
// Last Modified: November 18, 2026
// Purpose: Interactive zero-dependency Terminal User Interface (TUI) for What's Running.
//          Configures raw terminal I/O via stty, renders ANSI color-coded views
//          (Processes, Services, Network, System, Events), provides real-time keyboard
//          navigation, search filtering, process tree rendering, and resource sorting.
// ==============================================================================

// No terminal library (curses/crossterm): raw mode is toggled with the system `stty` command and the
// screen is drawn with plain ANSI escape sequences. This keeps the binary dependency-free but means
// the TUI needs `stty` on PATH and an ANSI-capable terminal.
// File role: interactive front end started by `main.rs` for `--tui`. It repeatedly takes a procfs
// snapshot, optionally an activity inventory, and redraws a text screen. All numbers come from the same
// helpers as the CLI (main.rs), so the TUI and `--json` never disagree.
//
// Concurrency model: three threads. The main thread draws and handles keys; one helper thread reads
// stdin bytes into a channel; an optional second helper runs the blocking kernel event capture.
// Shared state is limited to the TERMINATE atomic flag set by signal handlers.
//
// Known limits: needs an interactive terminal and `stty`; only ASCII keys are handled; the screen is
// fully repainted on every refresh (simple, but it flickers on very slow terminals).
//
// Imports: activity::Inventory is the lazily computed services/network/system data.
use crate::activity::{self, Inventory};
use crate::procfs::{Process, Snapshot, collect_snapshot};
// Shared formatting/rate helpers live in main.rs so CLI and TUI agree on numbers.
use crate::{field_display, io_rates, mib, rates, sanitize, truncate, used};

// std pieces: mpsc connects the input and event threads to the loop, atomics carry the signal flag,
// Command/Stdio run `stty`, and IsTerminal refuses to start when output is piped.
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Read, Write};
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// Copy: these small tags are passed by value into draw() every frame and matched in several places.
#[derive(Clone, Copy)]
/// Process-list sort order; cycled with the `s` key (Cpu -> Memory -> Pid -> Name -> Cpu).
// The key `s` advances through these four orders. CPU is first because "what is busy right now" is the common question.
enum Sort {
    /// Highest CPU% first (from the delta between two snapshots, so it is blank on the first frame).
    Cpu,
    /// Highest resident set size first.
    Memory,
    /// Ascending PID: stable order, useful for finding a known process.
    Pid,
    /// Alphabetical by process name.
    Name,
}
#[derive(Clone, Copy)]
/// Which processes are listed; cycled with `f`.
// Filters narrow the list before sorting; the default (UserApps) matches what a desktop user means by "programs".
enum Filter {
    /// Everything readable under /proc, including kernel threads.
    All,
    /// Default: hides kernel threads and kernel worker names, so the list shows real programs.
    UserApps,
    /// Processes whose real UID matches the current user.
    Mine,
    /// Processes with at least one unreadable identity field (see Process::visibility_summary).
    Problems,
}
#[derive(Clone, Copy)]
/// Top-level screens; cycled with `v`. Only Services/Network/System need the (expensive) inventory.
// Each view is a separate screen; only the process view is drawn from the snapshot alone.
enum View {
    /// The sortable process table with an optional detail panel.
    Processes,
    /// systemd units/scopes and cgroup paths associated with running processes.
    Services,
    /// Open sockets (listening and connected) with the PIDs that own them, where readable.
    Network,
    /// Systemd timers and mounted filesystems.
    System,
    /// Kernel process start/exit events captured by `--tui --events N`.
    Events,
}
/// Set from the signal handler; polled by the main loop. Only an atomic store happens in the handler,
/// which is async-signal-safe, so cleanup (restoring the terminal) runs on the normal path.
// Relaxed ordering is enough: the flag is the only data shared between the handler and the loop.
static TERMINATE: AtomicBool = AtomicBool::new(false);

// Raw FFI instead of the libc crate: only `signal` is needed, and declaring it here avoids an entire dependency.
unsafe extern "C" {
    /// libc `signal(2)`, declared by hand to avoid a libc dependency.
    fn signal(number: i32, handler: extern "C" fn(i32)) -> usize;
}
/// Signal handler: just request shutdown.
// Must do nothing but an atomic store: other operations (allocation, printing) are not async-signal-safe.
extern "C" fn terminate(_: i32) {
    TERMINATE.store(true, Ordering::Relaxed);
}

/// RAII guard holding the original `stty -g` settings so the terminal is always restored on drop,
/// even on early return or error.
// The String is the exact output of `stty -g`; it is the only state needed to undo raw mode.
struct Terminal(String);
impl Terminal {
    /// Switches the terminal into raw, no-echo mode on the alternate screen and returns the guard.
    ///
    /// # Errors
    /// Fails when stdin or stdout is not a terminal (the TUI cannot work with pipes) or when `stty` fails.
    fn enter() -> io::Result<Self> {
        // Checked first so `whats-running --tui | less` fails with a clear message instead of garbage escapes.
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(io::Error::other("--tui requires an interactive terminal"));
        }
        // `stty -g` prints the current settings in a form that `stty` can re-apply later.
        let saved = stty_output(&["-g"])?;
        // Raw mode delivers each key immediately, with no line buffering, echo, or Ctrl-C signal.
        // -echo hides typed characters (the search box echoes them itself); `raw` also disables Ctrl-C/Ctrl-Z signals.
        stty(&["raw", "-echo"])?;
        // ESC[?1049h = switch to the alternate screen buffer (the user's scrollback is preserved);
        // ESC[?25l = hide the cursor.
        // Alternate screen: the TUI draws on a separate buffer so the user's shell history is intact after quitting.
        print!("\x1b[?1049h\x1b[?25l");
        io::stdout().flush()?;
        Ok(Self(saved))
    }
}

// Drop (not an explicit call) is what guarantees the terminal is restored on every exit path, including `?` errors and panics that unwind.
impl Drop for Terminal {
    fn drop(&mut self) {
        // Errors are ignored: drop must not panic, and there is nothing more to do if restoring fails.
        // Re-applying the `stty -g` string restores every flag exactly, not just the two that were changed.
        let _ = stty(&[self.0.trim()]);
        // Show the cursor again and leave the alternate screen.
        print!("\x1b[?25h\x1b[?1049l");
        let _ = io::stdout().flush();
    }
}

/// Main TUI loop: collect a snapshot, draw it, then wait for a key or for the refresh interval to elapse.
// Parameters: `interval_ms` refresh period (clamped to at least 100 ms), `show_command` whether to read
// command lines (privacy opt-in, as in the CLI), `event_limit` Some(n) to start background event capture.
//
// Errors: terminal setup failures and /proc listing failures are returned; keyboard problems end the loop quietly.
pub fn run(interval_ms: u64, show_command: bool, event_limit: Option<usize>) -> io::Result<()> {
    // Bound to a named variable (not `_`) so the guard lives until `run` returns and restores the terminal then.
    let _terminal = Terminal::enter()?;
    unsafe {
        // Signals 1, 2, 15 = SIGHUP, SIGINT, SIGTERM (Linux numbering). Raw mode disables Ctrl-C's SIGINT,
        // so these chiefly cover external kills / terminal closure.
        // Only these three signals are handled; SIGKILL cannot be caught, in which case the terminal stays raw (`reset` fixes it).
        signal(1, terminate);
        signal(2, terminate);
        signal(15, terminate);
    }
    // Keyboard input is read on a helper thread and sent over a channel, so the main loop can use
    // recv_timeout as both "wait for key" and "refresh timer".
    let (tx, rx) = mpsc::channel();

    // Input thread: ends when stdin closes (the loop then sees Disconnected) or when the receiver is dropped (send fails).
    thread::spawn(move || {
        // Byte-wise, not line-wise: in raw mode every keypress arrives immediately; multi-byte sequences (arrows) arrive as
        // several bytes and are simply ignored by the key table below.
        for byte in io::stdin().lock().bytes().flatten() {
            // A send error means the main loop has exited, so the thread stops instead of waiting for input forever.
            if tx.send(byte).is_err() {
                break;
            }
        }
    });

    // `selected` is a row index into the filtered, sorted list; it is not a PID, so it can point at a different process after a re-sort.
    let (mut previous, mut selected, mut sort, mut filter, mut tree, mut details) =
        // Initial UI state: previous snapshot, selected row, sort, filter, tree mode off, details panel on.
        (None, 0, Sort::Cpu, Filter::UserApps, false, true);

    // Start on the cheapest view so the first frame appears immediately.
    let mut view = View::Processes;
    // The inventory (systemctl calls, /proc/net walk, fd scans) is slow, so it is computed lazily and
    // cached until the user presses `r`.
    let mut inventory_cache = None;

    // `editing` decides whether keystrokes extend the search string or act as commands.
    let mut search = String::new();
    let mut editing = false;
    // With --events N, the blocking kernel event capture runs on its own thread and its result is
    // picked up non-blockingly by the loop below.
    // Channel of one message: the finished capture result (or its error text).
    let (event_tx, event_rx) = mpsc::channel();

    // Only started when --events was given, so the default TUI never opens a netlink socket (which needs privileges).
    if let Some(limit) = event_limit {
        thread::spawn(move || {
            let _ = event_tx.send(
                // Same 5 s receive timeout as the CLI event mode; the thread then ends and reports once.
                crate::events::capture(limit, Duration::from_secs(5)).map_err(|e| e.to_string()),
            );
        });
    }

    // Stays None until the capture thread reports; the Events view shows "not enabled" or the result accordingly.
    let mut event_result = None;

    // One iteration = collect, draw, then wait. The wait doubles as the refresh timer, so the screen updates at
    // least once per interval even when no keys arrive.
    loop {
        // swap(false) both reads and clears the flag.
        // Checked at the top of each iteration, so shutdown happens within one refresh interval of the signal.
        if TERMINATE.swap(false, Ordering::Relaxed) {
            break;
        }
        // try_recv never blocks; the Events view shows results once the capture finishes.
        // At most one message ever arrives; once stored it is kept, so later frames keep showing the capture.
        if let Ok(result) = event_rx.try_recv() {
            event_result = Some(result)
        }
        // A fresh full snapshot every refresh; the previous one is kept only to compute rates.
        // `?` aborts the whole TUI on a /proc listing failure; per-process failures never reach here.
        let snapshot = collect_snapshot(show_command)?;

        // Computed on first entry into an inventory view and reused for later frames, because it costs far more than a snapshot.
        if !matches!(view, View::Processes | View::Events) && inventory_cache.is_none() {
            inventory_cache = Some(activity::collect(&snapshot));
        }

        // Argument order mirrors the `draw` signature. The inventory argument is hidden for the process and event views so they ignore stale data.
        draw(
            &snapshot,
            previous.as_ref(),
            selected,
            sort,
            filter,
            tree,
            details,
            view,
            inventory_cache
                .as_ref()
                .filter(|_| !matches!(view, View::Processes | View::Events)),
            &search,
            event_result.as_ref(),
        )?;

        // Moves (not clones) the snapshot: it was borrowed by draw above and is needed once more as the baseline for the next frame's rates.
        previous = Some(snapshot);
        // Floor of 100 ms prevents a busy loop when a tiny --interval-ms is given.
        // The only blocking call in the loop: returns early on a keypress, otherwise after the refresh interval.
        match rx.recv_timeout(Duration::from_millis(interval_ms.max(100))) {
            // Search-edit mode takes priority over single-key commands: Enter accepts, Esc (27) cancels and
            // clears, Backspace (127) deletes, printable ASCII appends.
            // Edit mode first: while typing a search, letters like `q` or `s` must be text, not commands.
            Ok(byte) if editing => match byte {
                // Both CR and LF are accepted because terminals in raw mode send CR for Enter.
                b'\n' | b'\r' => editing = false,
                27 => {
                    editing = false;
                    search.clear()
                }
                127 => {
                    search.pop();
                }

                // Printable ASCII only; control bytes and the lead bytes of UTF-8/escape sequences are dropped.
                value if value.is_ascii_graphic() || value == b' ' => search.push(value as char),
                _ => {}
            },
            // `q` or byte 3 (Ctrl-C, delivered as a plain byte in raw mode) quits.
            // Breaking (rather than returning) lets the Terminal guard drop in the normal way.
            Ok(b'q' | 3) => break,
            // `selected` may run past the end; draw() clamps it to the list length.
            // Vim-style movement; no arrow keys because they arrive as three-byte escape sequences that this reader does not decode.
            Ok(b'j') => selected += 1,
            Ok(b'k') => selected = selected.saturating_sub(1),
            // Sort and filter cycle in a fixed ring so one key reaches every state; there is no reverse key.
            Ok(b's') => {
                sort = match sort {
                    Sort::Cpu => Sort::Memory,
                    Sort::Memory => Sort::Pid,
                    Sort::Pid => Sort::Name,
                    Sort::Name => Sort::Cpu,
                }
            }

            // Selection resets to the top because the old index may not exist in the new filtered list.
            Ok(b'f') => {
                filter = match filter {
                    Filter::All => Filter::UserApps,
                    Filter::UserApps => Filter::Mine,
                    Filter::Mine => Filter::Problems,
                    Filter::Problems => Filter::All,
                };
                selected = 0
            }

            // Tree mode toggles a parent/child layout; `d` toggles the detail panel at the bottom (which reduces the visible rows).
            Ok(b't') => tree = !tree,
            // Hiding the detail panel gives the list four more rows on small terminals.
            Ok(b'd') => details = !details,
            // Views are cycled the same way; selection resets because each view has its own list.
            Ok(b'v') => {
                view = match view {
                    View::Processes => View::Services,
                    View::Services => View::Network,
                    View::Network => View::System,
                    View::System => View::Events,
                    View::Events => View::Processes,
                };
                selected = 0;
            }
            // Dropping the cache forces the next iteration to re-collect the inventory.
            // `r` is the only way to refresh the expensive inventory; process data refreshes automatically every interval.
            Ok(b'r') => inventory_cache = None,
            // Entering search mode does not clear the existing text, so the user can refine the previous search.
            Ok(b'/') => editing = true,
            // Unknown keys and plain timeouts simply fall through to the next refresh.
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            // The input thread ended (stdin closed): nothing more can be read, so exit.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

// `expect` (not `allow`) so the compiler warns if this lint exemption ever becomes unnecessary.
// Why `expect`: see the comment above. The long parameter list is intentional (see the doc comment below).
#[expect(
    clippy::too_many_arguments,
    reason = "explicit immutable UI state keeps the KISS loop local"
)]
/// Renders one full frame to stdout. State is passed in explicitly rather than held in a struct, which
/// is why there are so many parameters. Builds the whole screen in a String and prints it once to
/// reduce flicker.
// Drawing is a pure function of its arguments plus the terminal size. Nothing is stored between frames, which
// is why a refresh is just "call draw again". Steps in order: measure the terminal, compute CPU/IO rates,
// filter, sort, scroll window, header, then the body for the current view, and finally print once.
// Return value: Ok after the frame is flushed. The only errors are stdout write failures (for example a closed terminal), which end the TUI.
fn draw(
    s: &Snapshot,
    previous: Option<&Snapshot>,
    selected: usize,
    sort: Sort,
    filter: Filter,
    tree: bool,
    details: bool,
    view: View,
    inventory: Option<&Inventory>,
    search: &str,
    event_result: Option<&Result<crate::events::Capture, String>>,
) -> io::Result<()> {
    // `stty size` prints "rows cols"; fall back to 24x80 if it fails or parses badly.
    // Asked on every frame so resizing the terminal takes effect at the next refresh without handling SIGWINCH.
    let size = stty_output(&["size"]).unwrap_or_else(|_| "24 80".into());
    let mut size = size.split_whitespace();

    // stty prints "rows cols": the first token is height, the second width.
    let rows: usize = size.next().and_then(|v| v.parse().ok()).unwrap_or(24);
    let columns: usize = size.next().and_then(|v| v.parse().ok()).unwrap_or(80);

    // Rate maps come from main.rs; both are empty on the first frame, which renders as `?`.
    let cpu = rates(s, previous);
    let io_rates = io_rates(s, previous);

    // Needed only by the `Mine` filter, but cheap, so it is read on every frame instead of being cached.
    let uid = current_uid();
    // Kernel threads are children of PID 2 (kthreadd); collect them so they can be hidden.
    // Kernel threads all descend from PID 2, so this set is the main (cheap and exact) way to hide them.
    let kthreadd_children: std::collections::HashSet<u32> = s
        .processes
        .iter()
        .filter(|p| p.ppid.copied() == Some(2))
        .map(|p| p.pid)
        .collect();

    // Borrowed references into the snapshot, not copies: filtering and sorting never clone a Process.
    let mut list: Vec<&Process> = s
        .processes
        .iter()
        // First filter: category (all / user apps / mine / problems). A second filter below applies the search text.
        .filter(|p| match filter {
            Filter::All => true,
            Filter::UserApps => {
                // Exclude PID 2 (kthreadd) itself and all its direct children (kernel threads)
                // Also exclude PID 0 (idle) and any process whose name looks like a kernel worker
                // (The long list of name prefixes below catches kernel worker threads that may be reparented or
                // not visible as kthreadd children, e.g. inside containers. It is a heuristic and may hide
                // user processes whose names happen to start with one of these strings, e.g. "card" or "md".)
                // Local copies keep the long boolean chain below readable.
                let pid = p.pid;
                let ppid = p.ppid.copied();
                let name = p.name.value().map(|s| s.as_str()).unwrap_or("");

                // The first four conditions exclude PID 2 (kthreadd) and its children by identity; the name list after them covers stragglers.
                pid != 2
                    && ppid != Some(2)
                    && !kthreadd_children.contains(&pid)
                    // Name-prefix blocklist for kernel workers that are not children of kthreadd (for example inside containers).
                    // Grouped roughly as: scheduler/RCU/memory management, block and filesystem workers, networking/NFS, then GPU drivers.
                    // Trade-off: a user program whose name begins with one of these strings is hidden too; use the `all` filter to see it.
                    && !name.starts_with("kworker/")
                    && !name.starts_with("kthread")
                    && name != "kthreadd"
                    && !name.starts_with("ksoftirqd")
                    && !name.starts_with("migration/")
                    && !name.starts_with("idle_inject/")
                    // RCU and scheduler helpers: per-CPU threads such as rcu_gp, migration/N and ksoftirqd/N.
                    && !name.starts_with("rcu_")
                    && !name.starts_with("rcub/")
                    && !name.starts_with("rcuc/")
                    // Memory management: swap, compaction, huge pages and OOM handling.
                    && !name.starts_with("kswapd")
                    && !name.starts_with("kcompactd")
                    && !name.starts_with("khugepaged")
                    && !name.starts_with("kdevtmpfs")
                    && !name.starts_with("netns")
                    && !name.starts_with("kauditd")
                    && !name.starts_with("khungtaskd")
                    && !name.starts_with("oom_reaper")
                    // Block layer and I/O writeback workers.
                    && !name.starts_with("writeback")
                    && !name.starts_with("kblockd")
                    && !name.starts_with("blkcg_punt_bio")
                    && !name.starts_with("edac-poller")
                    && !name.starts_with("devfreq_wq")
                    && !name.starts_with("watchdogd")
                    && !name.starts_with("watchdog/")
                    && !name.starts_with("irq/")
                    // GPU and graphics kernel threads (`card0-crtc0`, i915, nouveau, amdgpu); "card" is broad on purpose.
                    && !name.starts_with("card")
                    && !name.starts_with("i915")
                    && !name.starts_with("nouveau")
                    && !name.starts_with("amdgpu")
                    // Filesystem and storage kernel threads.
                    && !name.starts_with("jbd2/")
                    && !name.starts_with("ext4-")
                    && !name.starts_with("xfs-")
                    && !name.starts_with("btrfs")
                    && !name.starts_with("cryptd")
                    // Compressed swap, device-mapper and software RAID workers.
                    && !name.starts_with("zswap-")
                    && !name.starts_with("dm-")
                    && !name.starts_with("md")
                    // Hardware, power and firmware helpers.
                    && !name.starts_with("hwrng")
                    && !name.starts_with("acpi_thermal_pm")
                    && !name.starts_with("ipv6_addrconf")
                    // Networking and remote-filesystem kernel threads.
                    && !name.starts_with("nfsiod")
                    && !name.starts_with("scsi_")
                    && !name.starts_with("usb-storage")
                    && !name.starts_with("cfg80211")
                    && !name.starts_with("rpciod")
                    && !name.starts_with("xprtiod")
                    && !name.starts_with("pool_workqueue")
                    && !name.starts_with("charger_manager")
                    && !name.starts_with("nvme-wq")
                    && !name.starts_with("nvme-reset-wq")
                    && !name.starts_with("nvme-delete-wq")
                    && !name.starts_with("mld")
                    && !name.starts_with("inet_frag_wq")
                    // Misc: block I/O sets, TTM (GPU memory manager) and DRM scheduler threads.
                    && !name.starts_with("bioset")
                    && !name.starts_with("ttm")
                    && !name.starts_with("drm_sched")
                    && !name.starts_with("drm-")
            }
            // If the UID could not be read, `uid` is None and only processes with unknown UID match.
            // Comparing Option<u32> with Option<u32> is deliberate: an unreadable UID on both sides would match, but `uid` is always readable in practice.
            Filter::Mine => p.uid.copied() == uid,
            // Shows only rows with an unreadable identity field; this is where permission and hidepid issues become visible.
            Filter::Problems => p.visibility_summary() != "complete",
        })
        // Case-insensitive substring search over both the name and the executable path.
        .filter(|p| {
            // Search is applied on top of the filter above.
            // Empty search matches everything, so the filter is a no-op until the user types.
            search.is_empty()
                || p.name
                    .value()
                    .is_some_and(|name| name.to_lowercase().contains(&search.to_lowercase()))
                || p.executable
                    .value()
                    .is_some_and(|value| value.to_lowercase().contains(&search.to_lowercase()))
        })
        .collect();
    if tree {
        // Tree mode ignores the sort setting: parents first, children indented beneath them.
        // tree_order returns a new Vec in depth-first order; `depth` later computes the indent per row.
        list = tree_order(list);
    } else {
        match sort {
            // Descending CPU%: operands are swapped. partial_cmp can fail on NaN; treat that as equal.
            // Missing rates (None) sort last because `None < Some(_)` and the comparison is reversed.
            Sort::Cpu => list.sort_by(|a, b| {
                // Key is (pid, start_ticks) for the same PID-reuse reason as in main.rs; swapping a and b gives descending order.
                cpu.get(&(b.pid, b.start_ticks.copied()))
                    .partial_cmp(&cpu.get(&(a.pid, a.start_ticks.copied())))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            // Reverse() turns the ascending key into descending; processes without RSS count as 0 and sort last.
            Sort::Memory => {
                list.sort_by_key(|p| std::cmp::Reverse(p.rss_kib.copied().unwrap_or(0)))
            }

            // Ascending numeric order; ties cannot occur because PIDs are unique.
            Sort::Pid => list.sort_by_key(|p| p.pid),
            // Clones the name for the sort key; acceptable for a few thousand rows and avoids borrowing problems with the closure.
            Sort::Name => list.sort_by_key(|p| p.name.value().cloned().unwrap_or_default()),
        }
    }
    // Clamp the cursor after filtering may have shrunk the list.
    // Re-clamped after filtering because the same index may now be past the end; `saturating_sub` handles an empty list.
    let selected = selected.min(list.len().saturating_sub(1));
    // Rows available for list entries after the header lines (and the detail panel if shown).
    // Fixed overhead: header (2 lines) + divider/help (3) + column header (1) = 6; the detail panel needs 4 more.
    let page = rows.saturating_sub(if details { 10 } else { 6 }).max(1);
    // Scroll window: keep the selected row visible as the last visible row when moving down.
    // Scrolls only when the cursor reaches the bottom row, so the window does not move while moving inside it.
    let start = selected.saturating_sub(page - 1);

    // pid -> ppid for every process (not only the filtered ones), so tree depth can walk through hidden ancestors.
    let parents: HashMap<_, _> = s
        .processes
        .iter()
        .filter_map(|p| p.ppid.copied().map(|v| (p.pid, v)))
        .collect();

    // Pids present in the snapshot: depth() stops at an ancestor that no longer exists.
    let known: HashSet<_> = s.processes.iter().map(|p| p.pid).collect();

    // The frame is assembled in memory and printed once; `\x1b[H\x1b[2J` homes the cursor and clears the screen first.
    let mut out = format!(
        "\x1b[H\x1b[2JWhat's Running?  Arch + Hyprland  view={} self={}  processes={}  collection={:.2}ms\n",
        match view {
            View::Processes => "processes",
            View::Services => "services/cgroups",
            View::Network => "network",
            View::System => "timers/mounts",
            View::Events => "events",
        },
        s.observer_pid,
        s.processes.len(),
        s.duration_micros as f64 / 1000.0
    );
    out.push_str(&format!(
        "memory {} / {} MiB  swap {} / {} MiB  sort={} filter={} tree={} search={:?}\n",
        used(s.memory.total_kib, s.memory.available_kib),
        mib(s.memory.total_kib),
        used(s.memory.swap_total_kib, s.memory.swap_free_kib),
        mib(s.memory.swap_total_kib),
        // Second header line: memory and swap use the same helpers as the CLI table, followed by the active sort/filter/search state.
        sort_name(sort),
        filter_name(filter),
        tree,
        search
    ));
    // Divider width follows the terminal width, limited to 20..=100 columns.
    // Clamped so the divider is neither tiny on narrow terminals nor absurdly long on wide ones.
    let bar = "-".repeat(columns.clamp(20, 100));
    out.push_str(&format!(
        "{bar}\nv views   / search   t tree   s sort   f filter   r refresh   j/k select   d details   q quit\n{bar}\n"
    ));
    // Inventory-backed views (Services, Network, System) are drawn here; the Events view and the
    // process list are the `else` branches.
    // Services/Network/System share one branch because they all draw from the same cached inventory.
    if let Some(inventory) = inventory {
        match view {
            // Counts first, then units, then cgroups; each list is cut to half a page so both fit on one screen.
            View::Services => {
                out.push_str(&format!(
                    // Summary counts first so the user sees totals even when the lists below are cut to fit the screen.
                    "associated_units={} system_units={} user_units={} cgroups={}\n",
                    inventory.units.len(),
                    field_count(&inventory.system_units),
                    field_count(&inventory.user_units),
                    field_value(&inventory.cgroup_count)
                ));

                // \n before each heading leaves a blank line between sections.
                out.push_str("\nUNITS / SCOPES\n");

                // Every untrusted name passes through sanitize() before reaching the terminal, as in the CLI.
                for unit in inventory.units.iter().take(page / 2) {
                    out.push_str(&format!("  {}\n", sanitize(unit)));
                }

                // Only a readable cgroup list is shown; an unreadable one is already visible as `<status>` in the counts line above.
                out.push_str("\nCGROUPS\n");
                if let crate::procfs::Field::Value(cgroups) = &inventory.cgroups {
                    for cgroup in cgroups.iter().take(page / 2) {
                        out.push_str(&format!("  {}\n", sanitize(cgroup)));
                    }
                }
            }

            // Sockets are filtered by the search text too, matching local address or unix path.
            View::Network => {
                out.push_str(&format!(
                    // The caveat is part of the label: an empty owners list means "not visible to us", not "no owner".
                    "sockets={} (owners are visible PID evidence; [] means unknown/unowned)\n",
                    field_count(&inventory.sockets)
                ));

                // Sockets are listed only when the table was readable (non-root users usually can read /proc/net but not other users' fds).
                out.push_str("\nSOCKETS\n");
                if let crate::procfs::Field::Value(sockets) = &inventory.sockets {
                    for socket in sockets
                        .iter()
                        // Search matches the local address or the unix socket path; remote addresses are not searched.
                        .filter(|socket| {
                            search.is_empty()
                                || socket.local.contains(search)
                                || socket
                                    .path
                                    .as_deref()
                                    .is_some_and(|path| path.contains(search))
                        })
                        .take(page)
                    {
                        out.push_str(&format!(
                            // Columns: protocol, local address (28 wide), owning PIDs, unix path if any.
                            "  {:<5} {:<28} owners={:?} path={}\n",
                            socket.protocol,
                            socket.local,
                            socket.owners,
                            socket.path.as_deref().unwrap_or("-")
                        ));
                    }
                }
            }

            // Timers (system and user together) and mounts share the screen, half a page each.
            View::System => {
                out.push_str(&format!(
                    // System and user timers are separate manager scopes; both are shown because many desktop jobs are user timers.
                    "system_timers={} user_timers={} mounts={}\n",
                    field_count(&inventory.system_timers),
                    field_count(&inventory.user_timers),
                    field_count(&inventory.mounts)
                ));
                out.push_str("\nTIMERS\n");
                if let crate::procfs::Field::Value(timers) = &inventory.system_timers {
                    for timer in timers
                        .iter()
                        // chain() appends user timers after system timers so one list (and one page budget) covers both.
                        .chain(match &inventory.user_timers {
                            crate::procfs::Field::Value(v) => v.iter(),
                            // Treat an unreadable user-timer list as empty so system timers still show.
                            // An unreadable user-timer list must not hide the system timers, hence the empty iterator.
                            _ => [].iter(),
                        })
                        .take(page / 2)
                    {
                        out.push_str(&format!("  {}\n", sanitize(timer)));
                    }
                }

                // Mount entries are shown as `type mountpoint <- source`, with the user-controllable strings sanitized.
                out.push_str("\nMOUNTS\n");
                if let crate::procfs::Field::Value(mounts) = &inventory.mounts {
                    for mount in mounts
                        .iter()
                        .filter(|mount| search.is_empty() || mount.mount_point.contains(search))
                        .take(page / 2)
                    {
                        out.push_str(&format!(
                            // Format: filesystem type, mount point, source device; reads "type mountpoint <- device".
                            "  {:<12} {} <- {}\n",
                            mount.fs_type,
                            sanitize(&mount.mount_point),
                            sanitize(&mount.source)
                        ));
                    }
                }
            }
            _ => {}
        }

    // Events view: three states (never started, failed, finished) rendered distinctly so the user knows why it is empty.
    } else if matches!(view, View::Events) {
        match event_result {
            // None = the user did not pass --events, so there is nothing to wait for.
            None => out.push_str("\nEVENT COLLECTOR: not enabled (launch with --tui --events N)\n"),
            // Capture failures (usually missing privileges for the netlink proc connector) are shown instead of hidden.
            Some(Err(error)) => out.push_str(&format!(
                "\nEVENT COLLECTOR: unavailable: {}\n",
                sanitize(error)
            )),
            Some(Ok(capture)) => {
                out.push_str(&format!(
                    "\nEVENT COLLECTOR: linux-proc-connector events={} sequence_gaps={}\n",
                    capture.events.len(),
                    capture.sequence_gaps
                ));

                // Newest first, because the interesting events are the most recent ones.
                for event in capture.events.iter().rev().take(page) {
                    out.push_str(&format!("  {}\n", event));
                }
            }
        }
    } else {
        // Column header for the process list; the widths here must match the row format below (52 characters before NAME).
        out.push_str("  PID    CPU%   RSS MiB   READ/s  WRITE/s NAME\n");

        // Only the visible window is formatted, so draw cost does not grow with the number of processes.
        for (index, p) in list.iter().enumerate().skip(start).take(page) {
            // Both I/O rates default to unknown so a new process does not break the row format.
            let ios = io_rates
                .get(&(p.pid, p.start_ticks.copied()))
                .copied()
                .unwrap_or((None, None));
            // Indentation depth is capped at 8 levels so deep trees don't push names off screen.
            // Indentation cost is only computed in tree mode; depth() walks the parent chain per row.
            let depth = if tree {
                depth(p.pid, &parents, &known).min(8)
            } else {
                0
            };
            let name = format!(
                "{}{}",
                "  ".repeat(depth),
                sanitize(p.name.value().map_or("?", String::as_str))
            );
            out.push_str(&format!(
                "{} {:<6} {:>6} {:>9.1} {:>7} {:>8} {}{}\n",
                if index == selected { '>' } else { ' ' },
                p.pid,
                cpu.get(&(p.pid, p.start_ticks.copied()))
                    .map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "?".into()),
                p.rss_kib.copied().unwrap_or(0) as f64 / 1024.0,
                ios.0.map(rate).unwrap_or_else(|| "?".into()),
                ios.1.map(rate).unwrap_or_else(|| "?".into()),
                // 52 is the width of the fixed columns before the name.
                // The fixed columns before the name occupy 43 characters; reserving 52 leaves a margin so a long name never wraps onto the next line.
                truncate(&name, columns.saturating_sub(52)),
                if p.pid == s.observer_pid {
                    // Same self-marker as the CLI table.
                    " [THIS TOOL]"
                } else {
                    ""
                }
            ));
        }

        // Detail panel for the highlighted row only; command lines can contain secrets, hence the warning in its title.
        if details && let Some(p) = list.get(selected) {
            out.push_str(&format!("\nDETAIL (command may contain secrets)\nPID={} PPID={:?} UID={:?} state={:?} start={:?}\nexe={}\ncommand={}\nvisibility={}\n",p.pid,p.ppid,p.uid,p.state,p.start_ticks,sanitize(&field_display(&p.executable)),sanitize(&field_display(&p.command)),p.visibility_summary()));
        }
    }
    print!(
        "{}",
        out.lines()
            // Truncating to `rows` lines prevents the terminal from scrolling; `\x1b[K` clears the rest of each line to remove leftovers from the previous frame.
            .take(rows)
            .map(|line| format!("{line}\x1b[K\n"))
            .collect::<String>()
    );
    // Flush so the frame appears immediately (stdout is line-buffered).
    io::stdout().flush()
}

/// Display helper for list-valued inventory fields: the item count, or `<status>` when unreadable.
fn field_count<T>(field: &crate::procfs::Field<Vec<T>>) -> String {
    match field {
        crate::procfs::Field::Value(values) => values.len().to_string(),
        other => format!("<{}>", other.status()),
    }
}

/// Display helper for scalar inventory fields: the value, or `<status>` when unreadable.
fn field_value<T: std::fmt::Display>(field: &crate::procfs::Field<T>) -> String {
    match field {
        crate::procfs::Field::Value(value) => value.to_string(),
        other => format!("<{}>", other.status()),
    }
}
/// Distance from `pid` to its topmost visible ancestor. `seen` breaks parent cycles, which can
/// appear in a non-atomic snapshot when PIDs are reused; stops at PID 0 or at unknown parents.
// Used only for indentation in tree mode. Cycles are possible because the snapshot is not atomic and PIDs can be recycled.
fn depth(mut pid: u32, p: &HashMap<u32, u32>, known: &HashSet<u32>) -> usize {
    // `d` counts hops to the top; `seen` records visited pids to break cycles.
    let (mut d, mut seen) = (0, HashSet::new());
    while let Some(parent) = p.get(&pid) {
        // Stop at PID 0, at unknown parents (filtered out or exited), and when a pid repeats (cycle).
        if *parent == 0 || !known.contains(parent) || !seen.insert(pid) {
            break;
        }
        d += 1;
        pid = *parent
    }
    d
}
/// Depth-first ordering for tree view. A process whose parent is not in the (filtered) list becomes a root,
/// so filtering never hides entries.
// Returns the same processes reordered; nothing is added or dropped. Complexity O(n log n) from the sorts.
fn tree_order(list: Vec<&Process>) -> Vec<&Process> {
    // Membership test for "is this parent in the visible list?"; a hidden parent turns its child into a root.
    let ids: HashSet<_> = list.iter().map(|p| p.pid).collect();

    // parent pid -> visible children, consumed by `visit`.
    let mut children: HashMap<u32, Vec<&Process>> = HashMap::new();
    let mut roots = Vec::new();
    for process in list {
        if let Some(parent) = process.ppid.copied().filter(|parent| ids.contains(parent)) {
            children.entry(parent).or_default().push(process)
        } else {
            roots.push(process)
        }
    }

    // Sorted by PID so the tree layout is stable from frame to frame.
    roots.sort_by_key(|p| p.pid);

    // Children are sorted too so siblings appear in PID order.
    for values in children.values_mut() {
        values.sort_by_key(|p| p.pid)
    }
    /// `children.remove` consumes each child list, so every process is emitted at most once;
    /// `seen` additionally guards against cycles.
    fn visit<'a>(
        p: &'a Process,
        c: &mut HashMap<u32, Vec<&'a Process>>,
        seen: &mut HashSet<u32>,
        out: &mut Vec<&'a Process>,
    ) {
        if !seen.insert(p.pid) {
            return;
        }
        out.push(p);
        if let Some(values) = c.remove(&p.pid) {
            for child in values {
                visit(child, c, seen, out)
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    // Visit each root in turn; `visit` recurses into children first, which gives the depth-first layout.
    for root in roots {
        visit(root, &mut children, &mut seen, &mut out)
    }
    // Anything left over was unreachable from the roots (a parent cycle); append it so it is not lost.
    for values in children.into_values() {
        for p in values {
            visit(p, &mut HashMap::new(), &mut seen, &mut out)
        }
    }
    out
}
/// Same formatting as main::bytes_rate (duplicated here as a small private helper).
// Private copy of main.rs `bytes_rate` (K/M suffixes); kept local so this module has no dependency on that function's visibility.
fn rate(v: f64) -> String {
    // Binary thresholds (1024^2 and 1024), the same as main::bytes_rate.
    if v >= 1_048_576.0 {
        format!("{:.1}M", v / 1_048_576.0)
    } else if v >= 1024.0 {
        format!("{:.1}K", v / 1024.0)
    } else {
        format!("{v:.0}")
    }
}

// Text shown in the header line for the current sort order.
fn sort_name(v: Sort) -> &'static str {
    match v {
        Sort::Cpu => "cpu",
        Sort::Memory => "memory",
        Sort::Pid => "pid",
        Sort::Name => "name",
    }
}

// Text shown in the header line for the current filter.
fn filter_name(v: Filter) -> &'static str {
    match v {
        Filter::All => "all",
        Filter::UserApps => "user-apps (no kernel threads)",
        Filter::Mine => "mine",
        Filter::Problems => "problems",
    }
}
/// Real UID: first number on the `Uid:` line of /proc/self/status (avoids a getuid libc binding).
// Reads /proc/self/status so no libc `getuid` binding is needed; None if the file is unreadable.
fn current_uid() -> Option<u32> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        // The `Uid:` line is "Uid: real effective saved fs"; nth(1) below takes the first number after the key.
        .find(|v| v.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
/// stdin must be inherited so stty acts on the controlling terminal rather than a pipe.
// Returns the captured stdout (for example the `stty -g` settings string or the "rows cols" line).
fn stty_output(args: &[&str]) -> io::Result<String> {
    let o = Command::new("stty")
        .args(args)
        // Required: `stty size` and `stty -g` query the terminal attached to stdin, which would be a pipe otherwise.
        .stdin(Stdio::inherit())
        .output()?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        // The message is generic on purpose; stty already printed its own diagnostic on stderr.
        Err(io::Error::other("stty failed"))
    }
}

/// Runs `stty` with the given arguments, discarding its output; used to enter and leave raw mode.
///
/// # Errors
/// Returns an error when `stty` cannot be started or exits non-zero.
fn stty(args: &[&str]) -> io::Result<()> {
    if Command::new("stty").args(args).status()?.success() {
        Ok(())
    } else {
        Err(io::Error::other("stty failed"))
    }
}

#[cfg(test)]
// Tests cover only the pure helpers and the signal flag; drawing and raw mode need a real terminal and are checked by hand.
mod tests {
    use super::*;
    #[test]
    /// 1 <-> 2 parent loop must terminate with a small depth instead of spinning forever.
    fn cycles_are_bounded() {
        // Two processes that name each other as parent: depth() must stop after at most two hops.
        let p = HashMap::from([(1, 2), (2, 1)]);
        assert!(depth(1, &p, &HashSet::from([1, 2])) <= 2)
    }
    #[test]
    /// Pins the unit formatting: 2048 bytes/s is shown as `2.0K`.
    fn rates_are_readable() {
        assert_eq!(rate(2048.0), "2.0K")
    }
    #[test]
    /// The signal handler must set the flag that the main loop polls; this is how SIGTERM leads to a clean terminal restore.
    fn termination_handler_sets_safe_exit_flag() {
        TERMINATE.store(false, Ordering::Relaxed);

        // Calling the handler directly (as a function) simulates SIGTERM without delivering a real signal to the test process.
        terminate(15);
        assert!(TERMINATE.swap(false, Ordering::Relaxed));
    }
}
