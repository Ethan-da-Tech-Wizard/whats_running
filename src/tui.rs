use crate::activity::{self, Inventory};
use crate::procfs::{Process, Snapshot, collect_snapshot};
use crate::{field_display, io_rates, mib, rates, sanitize, truncate, used};
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Read, Write};
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy)]
enum Sort {
    Cpu,
    Memory,
    Pid,
    Name,
}
#[derive(Clone, Copy)]
enum Filter {
    All,
    UserApps,
    Mine,
    Problems,
}
#[derive(Clone, Copy)]
enum View {
    Processes,
    Services,
    Network,
    System,
    Events,
}
static TERMINATE: AtomicBool = AtomicBool::new(false);
unsafe extern "C" {
    fn signal(number: i32, handler: extern "C" fn(i32)) -> usize;
}
extern "C" fn terminate(_: i32) {
    TERMINATE.store(true, Ordering::Relaxed);
}

struct Terminal(String);
impl Terminal {
    fn enter() -> io::Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(io::Error::other("--tui requires an interactive terminal"));
        }
        let saved = stty_output(&["-g"])?;
        stty(&["raw", "-echo"])?;
        print!("\x1b[?1049h\x1b[?25l");
        io::stdout().flush()?;
        Ok(Self(saved))
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = stty(&[self.0.trim()]);
        print!("\x1b[?25h\x1b[?1049l");
        let _ = io::stdout().flush();
    }
}

pub fn run(interval_ms: u64, show_command: bool, event_limit: Option<usize>) -> io::Result<()> {
    let _terminal = Terminal::enter()?;
    unsafe {
        signal(1, terminate);
        signal(2, terminate);
        signal(15, terminate);
    }
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for byte in io::stdin().lock().bytes().flatten() {
            if tx.send(byte).is_err() {
                break;
            }
        }
    });
    let (mut previous, mut selected, mut sort, mut filter, mut tree, mut details) =
        (None, 0, Sort::Cpu, Filter::UserApps, false, true);
    let mut view = View::Processes;
    let mut inventory_cache = None;
    let mut search = String::new();
    let mut editing = false;
    let (event_tx, event_rx) = mpsc::channel();
    if let Some(limit) = event_limit {
        thread::spawn(move || {
            let _ = event_tx.send(
                crate::events::capture(limit, Duration::from_secs(5)).map_err(|e| e.to_string()),
            );
        });
    }
    let mut event_result = None;
    loop {
        if TERMINATE.swap(false, Ordering::Relaxed) {
            break;
        }
        if let Ok(result) = event_rx.try_recv() {
            event_result = Some(result)
        }
        let snapshot = collect_snapshot(show_command)?;
        if !matches!(view, View::Processes | View::Events) && inventory_cache.is_none() {
            inventory_cache = Some(activity::collect(&snapshot));
        }
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
        previous = Some(snapshot);
        match rx.recv_timeout(Duration::from_millis(interval_ms.max(100))) {
            Ok(byte) if editing => match byte {
                b'\n' | b'\r' => editing = false,
                27 => {
                    editing = false;
                    search.clear()
                }
                127 => {
                    search.pop();
                }
                value if value.is_ascii_graphic() || value == b' ' => search.push(value as char),
                _ => {}
            },
            Ok(b'q' | 3) => break,
            Ok(b'j') => selected += 1,
            Ok(b'k') => selected = selected.saturating_sub(1),
            Ok(b's') => {
                sort = match sort {
                    Sort::Cpu => Sort::Memory,
                    Sort::Memory => Sort::Pid,
                    Sort::Pid => Sort::Name,
                    Sort::Name => Sort::Cpu,
                }
            }
            Ok(b'f') => {
                filter = match filter {
                    Filter::All => Filter::UserApps,
                    Filter::UserApps => Filter::Mine,
                    Filter::Mine => Filter::Problems,
                    Filter::Problems => Filter::All,
                };
                selected = 0
            }
            Ok(b't') => tree = !tree,
            Ok(b'd') => details = !details,
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
            Ok(b'r') => inventory_cache = None,
            Ok(b'/') => editing = true,
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "explicit immutable UI state keeps the KISS loop local"
)]
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
    let size = stty_output(&["size"]).unwrap_or_else(|_| "24 80".into());
    let mut size = size.split_whitespace();
    let rows: usize = size.next().and_then(|v| v.parse().ok()).unwrap_or(24);
    let columns: usize = size.next().and_then(|v| v.parse().ok()).unwrap_or(80);
    let cpu = rates(s, previous);
    let io_rates = io_rates(s, previous);
    let uid = current_uid();
    let kthreadd_children: std::collections::HashSet<u32> = s
        .processes
        .iter()
        .filter(|p| p.ppid.copied() == Some(2))
        .map(|p| p.pid)
        .collect();
    let mut list: Vec<&Process> = s
        .processes
        .iter()
        .filter(|p| match filter {
            Filter::All => true,
            Filter::UserApps => {
                // Exclude PID 2 (kthreadd) itself and all its direct children (kernel threads)
                // Also exclude PID 0 (idle) and any process whose name looks like a kernel worker
                let pid = p.pid;
                let ppid = p.ppid.copied();
                let name = p.name.value().map(|s| s.as_str()).unwrap_or("");
                pid != 2
                    && ppid != Some(2)
                    && !kthreadd_children.contains(&pid)
                    && !name.starts_with("kworker/")
                    && !name.starts_with("kthread")
                    && name != "kthreadd"
                    && !name.starts_with("ksoftirqd")
                    && !name.starts_with("migration/")
                    && !name.starts_with("idle_inject/")
                    && !name.starts_with("rcu_")
                    && !name.starts_with("rcub/")
                    && !name.starts_with("rcuc/")
                    && !name.starts_with("kswapd")
                    && !name.starts_with("kcompactd")
                    && !name.starts_with("khugepaged")
                    && !name.starts_with("kdevtmpfs")
                    && !name.starts_with("netns")
                    && !name.starts_with("kauditd")
                    && !name.starts_with("khungtaskd")
                    && !name.starts_with("oom_reaper")
                    && !name.starts_with("writeback")
                    && !name.starts_with("kblockd")
                    && !name.starts_with("blkcg_punt_bio")
                    && !name.starts_with("edac-poller")
                    && !name.starts_with("devfreq_wq")
                    && !name.starts_with("watchdogd")
                    && !name.starts_with("watchdog/")
                    && !name.starts_with("irq/")
                    && !name.starts_with("card")
                    && !name.starts_with("i915")
                    && !name.starts_with("nouveau")
                    && !name.starts_with("amdgpu")
                    && !name.starts_with("jbd2/")
                    && !name.starts_with("ext4-")
                    && !name.starts_with("xfs-")
                    && !name.starts_with("btrfs")
                    && !name.starts_with("cryptd")
                    && !name.starts_with("zswap-")
                    && !name.starts_with("dm-")
                    && !name.starts_with("md")
                    && !name.starts_with("hwrng")
                    && !name.starts_with("acpi_thermal_pm")
                    && !name.starts_with("ipv6_addrconf")
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
                    && !name.starts_with("bioset")
                    && !name.starts_with("ttm")
                    && !name.starts_with("drm_sched")
                    && !name.starts_with("drm-")
            }
            Filter::Mine => p.uid.copied() == uid,
            Filter::Problems => p.visibility_summary() != "complete",
        })
        .filter(|p| {
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
        list = tree_order(list);
    } else {
        match sort {
            Sort::Cpu => list.sort_by(|a, b| {
                cpu.get(&(b.pid, b.start_ticks.copied()))
                    .partial_cmp(&cpu.get(&(a.pid, a.start_ticks.copied())))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            Sort::Memory => {
                list.sort_by_key(|p| std::cmp::Reverse(p.rss_kib.copied().unwrap_or(0)))
            }
            Sort::Pid => list.sort_by_key(|p| p.pid),
            Sort::Name => list.sort_by_key(|p| p.name.value().cloned().unwrap_or_default()),
        }
    }
    let selected = selected.min(list.len().saturating_sub(1));
    let page = rows.saturating_sub(if details { 10 } else { 6 }).max(1);
    let start = selected.saturating_sub(page - 1);
    let parents: HashMap<_, _> = s
        .processes
        .iter()
        .filter_map(|p| p.ppid.copied().map(|v| (p.pid, v)))
        .collect();
    let known: HashSet<_> = s.processes.iter().map(|p| p.pid).collect();
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
        sort_name(sort),
        filter_name(filter),
        tree,
        search
    ));
    let bar = "-".repeat(columns.clamp(20, 100));
    out.push_str(&format!(
        "{bar}\nv views   / search   t tree   s sort   f filter   r refresh   j/k select   d details   q quit\n{bar}\n"
    ));
    if let Some(inventory) = inventory {
        match view {
            View::Services => {
                out.push_str(&format!(
                    "associated_units={} system_units={} user_units={} cgroups={}\n",
                    inventory.units.len(),
                    field_count(&inventory.system_units),
                    field_count(&inventory.user_units),
                    field_value(&inventory.cgroup_count)
                ));
                out.push_str("\nUNITS / SCOPES\n");
                for unit in inventory.units.iter().take(page / 2) {
                    out.push_str(&format!("  {}\n", sanitize(unit)));
                }
                out.push_str("\nCGROUPS\n");
                if let crate::procfs::Field::Value(cgroups) = &inventory.cgroups {
                    for cgroup in cgroups.iter().take(page / 2) {
                        out.push_str(&format!("  {}\n", sanitize(cgroup)));
                    }
                }
            }
            View::Network => {
                out.push_str(&format!(
                    "sockets={} (owners are visible PID evidence; [] means unknown/unowned)\n",
                    field_count(&inventory.sockets)
                ));
                out.push_str("\nSOCKETS\n");
                if let crate::procfs::Field::Value(sockets) = &inventory.sockets {
                    for socket in sockets
                        .iter()
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
                            "  {:<5} {:<28} owners={:?} path={}\n",
                            socket.protocol,
                            socket.local,
                            socket.owners,
                            socket.path.as_deref().unwrap_or("-")
                        ));
                    }
                }
            }
            View::System => {
                out.push_str(&format!(
                    "system_timers={} user_timers={} mounts={}\n",
                    field_count(&inventory.system_timers),
                    field_count(&inventory.user_timers),
                    field_count(&inventory.mounts)
                ));
                out.push_str("\nTIMERS\n");
                if let crate::procfs::Field::Value(timers) = &inventory.system_timers {
                    for timer in timers
                        .iter()
                        .chain(match &inventory.user_timers {
                            crate::procfs::Field::Value(v) => v.iter(),
                            _ => [].iter(),
                        })
                        .take(page / 2)
                    {
                        out.push_str(&format!("  {}\n", sanitize(timer)));
                    }
                }
                out.push_str("\nMOUNTS\n");
                if let crate::procfs::Field::Value(mounts) = &inventory.mounts {
                    for mount in mounts
                        .iter()
                        .filter(|mount| search.is_empty() || mount.mount_point.contains(search))
                        .take(page / 2)
                    {
                        out.push_str(&format!(
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
    } else if matches!(view, View::Events) {
        match event_result {
            None => out.push_str("\nEVENT COLLECTOR: not enabled (launch with --tui --events N)\n"),
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
                for event in capture.events.iter().rev().take(page) {
                    out.push_str(&format!("  {}\n", event));
                }
            }
        }
    } else {
        out.push_str("  PID    CPU%   RSS MiB   READ/s  WRITE/s NAME\n");
        for (index, p) in list.iter().enumerate().skip(start).take(page) {
            let ios = io_rates
                .get(&(p.pid, p.start_ticks.copied()))
                .copied()
                .unwrap_or((None, None));
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
                truncate(&name, columns.saturating_sub(52)),
                if p.pid == s.observer_pid {
                    " [THIS TOOL]"
                } else {
                    ""
                }
            ));
        }
        if details && let Some(p) = list.get(selected) {
            out.push_str(&format!("\nDETAIL (command may contain secrets)\nPID={} PPID={:?} UID={:?} state={:?} start={:?}\nexe={}\ncommand={}\nvisibility={}\n",p.pid,p.ppid,p.uid,p.state,p.start_ticks,sanitize(&field_display(&p.executable)),sanitize(&field_display(&p.command)),p.visibility_summary()));
        }
    }
    print!(
        "{}",
        out.lines()
            .take(rows)
            .map(|line| format!("{line}\x1b[K\n"))
            .collect::<String>()
    );
    io::stdout().flush()
}
fn field_count<T>(field: &crate::procfs::Field<Vec<T>>) -> String {
    match field {
        crate::procfs::Field::Value(values) => values.len().to_string(),
        other => format!("<{}>", other.status()),
    }
}
fn field_value<T: std::fmt::Display>(field: &crate::procfs::Field<T>) -> String {
    match field {
        crate::procfs::Field::Value(value) => value.to_string(),
        other => format!("<{}>", other.status()),
    }
}
fn depth(mut pid: u32, p: &HashMap<u32, u32>, known: &HashSet<u32>) -> usize {
    let (mut d, mut seen) = (0, HashSet::new());
    while let Some(parent) = p.get(&pid) {
        if *parent == 0 || !known.contains(parent) || !seen.insert(pid) {
            break;
        }
        d += 1;
        pid = *parent
    }
    d
}
fn tree_order(list: Vec<&Process>) -> Vec<&Process> {
    let ids: HashSet<_> = list.iter().map(|p| p.pid).collect();
    let mut children: HashMap<u32, Vec<&Process>> = HashMap::new();
    let mut roots = Vec::new();
    for process in list {
        if let Some(parent) = process.ppid.copied().filter(|parent| ids.contains(parent)) {
            children.entry(parent).or_default().push(process)
        } else {
            roots.push(process)
        }
    }
    roots.sort_by_key(|p| p.pid);
    for values in children.values_mut() {
        values.sort_by_key(|p| p.pid)
    }
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
    for root in roots {
        visit(root, &mut children, &mut seen, &mut out)
    }
    for values in children.into_values() {
        for p in values {
            visit(p, &mut HashMap::new(), &mut seen, &mut out)
        }
    }
    out
}
fn rate(v: f64) -> String {
    if v >= 1_048_576.0 {
        format!("{:.1}M", v / 1_048_576.0)
    } else if v >= 1024.0 {
        format!("{:.1}K", v / 1024.0)
    } else {
        format!("{v:.0}")
    }
}
fn sort_name(v: Sort) -> &'static str {
    match v {
        Sort::Cpu => "cpu",
        Sort::Memory => "memory",
        Sort::Pid => "pid",
        Sort::Name => "name",
    }
}
fn filter_name(v: Filter) -> &'static str {
    match v {
        Filter::All => "all",
        Filter::UserApps => "user-apps (no kernel threads)",
        Filter::Mine => "mine",
        Filter::Problems => "problems",
    }
}
fn current_uid() -> Option<u32> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|v| v.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
fn stty_output(args: &[&str]) -> io::Result<String> {
    let o = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .output()?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        Err(io::Error::other("stty failed"))
    }
}
fn stty(args: &[&str]) -> io::Result<()> {
    if Command::new("stty").args(args).status()?.success() {
        Ok(())
    } else {
        Err(io::Error::other("stty failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycles_are_bounded() {
        let p = HashMap::from([(1, 2), (2, 1)]);
        assert!(depth(1, &p, &HashSet::from([1, 2])) <= 2)
    }
    #[test]
    fn rates_are_readable() {
        assert_eq!(rate(2048.0), "2.0K")
    }
    #[test]
    fn termination_handler_sets_safe_exit_flag() {
        TERMINATE.store(false, Ordering::Relaxed);
        terminate(15);
        assert!(TERMINATE.swap(false, Ordering::Relaxed));
    }
}
