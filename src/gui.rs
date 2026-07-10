//! # gui — Iced native-window frontend for *What's Running?*
//!
//! This module implements the full desktop GUI using the [Iced] framework
//! (version 0.13).  It is the target of the `--gui` CLI flag and is
//! completely independent of the TUI layer in `tui.rs`.
//!
//! ## Architecture overview
//!
//! Iced follows the Elm / Model-View-Update (MVU) pattern:
//!
//! ```text
//!   ┌────────────────────────────────────────────────┐
//!   │  App (state)                                   │
//!   │    snapshot:  current /proc snapshot           │
//!   │    previous:  previous snapshot for delta      │
//!   │    groups:    aggregated process groups        │
//!   │    collapsed: set of group names hidden        │
//!   │    sort / filter / search / selected …        │
//!   └──────────┬─────────────────────────────────────┘
//!              │ view()         Message events
//!              ▼                     │
//!   ┌──────────────────────┐   ┌─────▼──────────────────┐
//!   │   Element tree       │   │   update()             │
//!   │   (Iced widgets)     │──▶│   mutates App state    │
//!   └──────────────────────┘   └────────────────────────┘
//!              ▲
//!   subscription() fires Message::Tick every 1 second → collect_snapshot()
//! ```
//!
//! ## Key design decisions
//!
//! * **Grouping** — processes are grouped by their short name so that e.g. all
//!   16 Chrome helper processes appear under one collapsible "Google Chrome (16)"
//!   row, exactly like Windows Task Manager.  The group header row shows the
//!   *sum* of CPU% and RSS across all members.
//!
//! * **Heat bars** — behind each CPU% and Memory value we draw a thin coloured
//!   bar proportional to the value.  This gives instant visual scannability
//!   without needing an extra sparkline widget.
//!
//! * **No blocking** — `collect_snapshot()` reads `/proc` synchronously but
//!   completes in ~1 ms on a modern machine; running it inside `update()` keeps
//!   the architecture simple and avoids channel plumbing for a sub-millisecond
//!   operation.

// ─── Standard imports ─────────────────────────────────────────────────────────
use std::collections::{HashMap, HashSet};
use std::time::Duration;

// ─── Iced imports ─────────────────────────────────────────────────────────────
use iced::keyboard::{self, key::Named};
use iced::widget::{button, column, container, horizontal_rule, row, scrollable, text, text_input};
use iced::widget::canvas;

use iced::{Color, Element, Font, Length, Subscription, Task, Theme};
use iced::{event, time};

// ─── Internal imports ─────────────────────────────────────────────────────────
use crate::procfs::{Process, Snapshot, collect_snapshot};
use crate::{io_rates, mib, rates, used};

// ══════════════════════════════════════════════════════════════════════════════
// COLOUR PALETTE
// ══════════════════════════════════════════════════════════════════════════════
//
// All colours are expressed as linear-sRGB floats [0.0, 1.0].
// The palette is intentionally dark with a magenta accent to match the
// existing terminal aesthetic of the project.

/// Deepest background — used for the main window and even-numbered rows.
const BG: Color = Color { r: 0.051, g: 0.059, b: 0.078, a: 1.0 }; // #0d0f14

/// Slightly lighter background — used for odd rows, header bar, toolbar,
/// detail panel.
const BG2: Color = Color { r: 0.075, g: 0.086, b: 0.110, a: 1.0 }; // #13161c

/// Column-header / button background.
const BG3: Color = Color { r: 0.110, g: 0.125, b: 0.160, a: 1.0 }; // #1c2028

/// Magenta accent — used for titles, active filter buttons, sort arrows.
const ACCENT: Color = Color { r: 0.78, g: 0.22, b: 0.55, a: 1.0 }; // #c73889

/// Primary foreground text colour.
const FG: Color = Color { r: 0.85, g: 0.87, b: 0.91, a: 1.0 }; // #d9dde8

/// Dimmed text — used for secondary labels and I/O rate cells.
const FG_DIM: Color = Color { r: 0.50, g: 0.52, b: 0.56, a: 1.0 }; // #80848f

/// Selection highlight — used for the currently selected row.
const SEL: Color = Color { r: 0.18, g: 0.20, b: 0.27, a: 1.0 }; // #2e3345

/// Group header row background — slightly brighter than BG2 to visually
/// separate app groups from their child rows.
const BG_GROUP: Color = Color { r: 0.095, g: 0.108, b: 0.140, a: 1.0 }; // #181b24

// Heat-bar colours (used for the thin indicator behind CPU% / Memory values)
const HEAT_LOW: Color = Color { r: 0.18, g: 0.58, b: 0.30, a: 0.55 }; // translucent green
const HEAT_MED: Color = Color { r: 0.80, g: 0.62, b: 0.10, a: 0.55 }; // translucent amber
const HEAT_HIGH: Color = Color { r: 0.80, g: 0.22, b: 0.15, a: 0.65 }; // translucent red

// Status-label colours
const STATUS_RUNNING: Color = Color { r: 0.30, g: 0.85, b: 0.45, a: 1.0 }; // green
const STATUS_SLEEPING: Color = Color { r: 0.50, g: 0.52, b: 0.56, a: 1.0 }; // grey (same as FG_DIM)
const STATUS_ZOMBIE: Color = Color { r: 0.90, g: 0.20, b: 0.20, a: 1.0 }; // red
const STATUS_OTHER: Color = Color { r: 1.00, g: 0.70, b: 0.00, a: 1.0 }; // amber

// Column pixel widths — kept as named constants so every function that lays out
// a row uses the same numbers and columns stay perfectly aligned.
const W_NAME: f32 = 280.0;
const W_STATUS: f32 = 90.0;
const W_CPU: f32 = 90.0;
const W_MEM: f32 = 100.0;
const W_READ: f32 = 90.0;
const W_WRITE: f32 = 90.0;

// ══════════════════════════════════════════════════════════════════════════════
// DOMAIN TYPES
// ══════════════════════════════════════════════════════════════════════════════

/// Which column the process list is currently sorted by.
///
/// Clicking a column header that is already the sort column inverts the
/// direction; clicking a new column always defaults to descending (highest
/// value first) except for Name which defaults to ascending (A→Z).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortCol {
    Name,
    Status,
    Cpu,
    Memory,
    ReadRate,
    WriteRate,
}

/// Determines which processes are shown in the list.
///
/// `UserApps` is the default and the most useful mode — it hides the hundreds
/// of kernel worker threads that would otherwise drown out real user processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Show every process visible in /proc.
    All,
    /// Hide kernel threads (kworker/*, ksoftirqd, etc.) — the "Steam level"
    /// view the user asked for.
    UserApps,
    /// Show only processes owned by the same UID as this tool.
    Mine,
}

/// A single display row in the process list.
///
/// A `DisplayRow` is either the *header* for a group of identically-named
/// processes (e.g. "Google Chrome (16)") or an individual *child* process
/// belonging to such a group.  When the sort column is Name, top-level groups
/// are sorted among themselves; children are always sorted by CPU descending
/// within their group so the busiest child floats to the top.
#[derive(Debug)]
enum DisplayRow<'a> {
    /// Aggregate header for a group of processes sharing the same short name.
    Group {
        /// The canonical short name shared by all members (e.g. "chrome").
        name: &'a str,
        /// Number of processes in this group (shown in the count badge).
        count: usize,
        /// Sum of CPU% across all members.
        total_cpu: f64,
        /// Sum of RSS KiB across all members.
        total_rss_kib: u64,
        /// Whether this group is currently expanded in the UI.
        expanded: bool,
    },
    /// A single process, indented beneath its group header.
    Child {
        /// Reference to the underlying process data from the snapshot.
        process: &'a Process,
        /// Pre-computed CPU% for this tick (requires two snapshots).
        cpu: f64,
        /// Pre-computed read bytes/s (None on first tick).
        read_rate: Option<f64>,
        /// Pre-computed write bytes/s (None on first tick).
        write_rate: Option<f64>,
    },
}

// ══════════════════════════════════════════════════════════════════════════════
// MESSAGES
// ══════════════════════════════════════════════════════════════════════════════

/// All user-facing and system events the application can handle.
///
/// Iced's MVU loop calls `App::update(msg)` for each message, which may mutate
/// state.  The view is then re-derived from state.
#[derive(Debug, Clone)]
pub enum Message {
    /// Fired by the 1-second subscription timer.  Triggers a fresh `/proc`
    /// snapshot collection.  The `Instant` payload is unused but required by
    /// `time::every`'s mapped type.
    Tick(()),

    /// User clicked a column header → change or invert the sort.
    SortBy(SortCol),

    /// User clicked one of the All / Apps / Mine filter buttons.
    SetFilter(FilterMode),

    /// User typed into the search box.
    SearchChanged(String),

    /// User clicked a child-process row to select/deselect it.
    SelectRow(u32),

    /// User clicked a group header row to expand or collapse it.
    ToggleGroup(String),

    /// Raw Iced event — used to intercept keyboard presses for navigation.
    IcedEvent(iced::Event),
}

// ══════════════════════════════════════════════════════════════════════════════
// APPLICATION STATE
// ══════════════════════════════════════════════════════════════════════════════

/// Root application state.
///
/// All mutable UI and data state lives here.  Iced calls `view(&self)` to
/// derive the widget tree, and `update(&mut self, msg)` to apply changes.
pub struct App {
    /// The most recent snapshot collected from /proc.  `None` only on the very
    /// first frame before the subscription has fired once.
    snapshot: Option<Snapshot>,

    /// The snapshot collected on the *previous* tick.  Used to compute CPU%
    /// and I/O byte-rate deltas.  `None` until at least two ticks have fired.
    previous: Option<Snapshot>,

    /// Which column the visible list is sorted by.
    sort: SortCol,

    /// `true` = ascending (A→Z, low→high).  `false` = descending.
    sort_asc: bool,

    /// Which processes to include in the list.
    filter: FilterMode,

    /// Current contents of the search box (compared case-insensitively against
    /// process names).
    search: String,

    /// The PID of the currently highlighted child-process row, or `None` if
    /// nothing is selected.
    selected: Option<u32>,

    /// Set of group names whose child rows are currently *hidden*.
    ///
    /// A group that is NOT in this set is expanded (children visible).
    /// We default to all groups expanded, so this starts empty.
    collapsed: HashSet<String>,
}

impl App {
    /// Construct the initial application state and return the first `Task`.
    ///
    /// `Task::none()` means "do nothing extra on startup"; the subscription
    /// timer will fire within 1 second to populate the process list.
    pub fn new() -> (Self, Task<Message>) {
        (
            Self {
                snapshot: None,
                previous: None,
                sort: SortCol::Cpu,
                sort_asc: false,        // default: highest CPU first
                filter: FilterMode::UserApps, // start in the clean "Apps" view
                search: String::new(),
                selected: None,
                collapsed: HashSet::new(), // all groups start expanded
            },
            Task::none(),
        )
    }

    // ── Iced application interface ────────────────────────────────────────────

    /// Returns the string shown in the window title bar.
    ///
    /// Includes the live process count so the user can glance at the title bar
    /// without looking at the header row.
    pub fn title(&self) -> String {
        match &self.snapshot {
            Some(s) => format!("What's Running? — {} processes", s.processes.len()),
            None => "What's Running?".into(),
        }
    }

    /// The MVU *update* function.  Called once per message; mutates `self` and
    /// returns a follow-up `Task` (almost always `Task::none()`).
    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            // ── Timer tick: collect a fresh snapshot ──────────────────────────
            Message::Tick(_) => {
                // Rotate: current → previous, then collect a new current.
                // We keep the previous snapshot alive so the next `view()` call
                // can compute per-process deltas (CPU%, I/O rates).
                let prev = self.snapshot.take();
                if let Ok(snap) = collect_snapshot(false) {
                    // Only overwrite `previous` if we successfully got a new
                    // snapshot; this avoids losing the last good baseline on a
                    // transient /proc error.
                    if let Some(p) = prev {
                        self.previous = Some(p);
                    }
                    self.snapshot = Some(snap);
                }
            }

            // ── Sort column clicked ───────────────────────────────────────────
            Message::SortBy(col) => {
                if self.sort == col {
                    // Same column: flip direction.
                    self.sort_asc = !self.sort_asc;
                } else {
                    // New column: set direction to the sensible default for that
                    // column (Name → A-Z ascending; everything else → descending).
                    self.sort = col;
                    self.sort_asc = col == SortCol::Name || col == SortCol::Status;
                }
            }

            // ── Filter button ─────────────────────────────────────────────────
            Message::SetFilter(f) => {
                self.filter = f;
                // Clear selection when filter changes because the selected PID
                // might not be visible in the new filter mode.
                self.selected = None;
            }

            // ── Search box input ──────────────────────────────────────────────
            Message::SearchChanged(s) => {
                self.search = s;
                self.selected = None; // reset selection when the list changes
            }

            // ── Row click: select or deselect a child process ─────────────────
            Message::SelectRow(pid) => {
                // Toggle: clicking the already-selected row deselects it.
                self.selected = if self.selected == Some(pid) { None } else { Some(pid) };
            }

            // ── Group header click: expand or collapse ────────────────────────
            Message::ToggleGroup(name) => {
                // We use a set of *collapsed* names so the default (empty set)
                // means "all expanded".
                if self.collapsed.contains(&name) {
                    self.collapsed.remove(&name);
                } else {
                    self.collapsed.insert(name);
                }
            }



            // ── Raw event: intercept arrow / j-k keys ─────────────────────────
            Message::IcedEvent(iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                ..
            })) => match key {
                keyboard::Key::Named(Named::ArrowUp) => self.move_selection(-1),
                keyboard::Key::Named(Named::ArrowDown) => self.move_selection(1),
                keyboard::Key::Character(c) if c == "k" => self.move_selection(-1),
                keyboard::Key::Character(c) if c == "j" => self.move_selection(1),
                _ => {}
            },

            // Swallow any other raw event (mouse moves etc.) — we only
            // subscribed to receive keyboard events.
            Message::IcedEvent(_) => {}
        }
        Task::none()
    }

    /// Returns all active Iced subscriptions.
    ///
    /// We subscribe to two things:
    /// 1. A 1-second timer that drives the data refresh loop.
    /// 2. All keyboard events so we can implement arrow-key navigation.
    pub fn subscription(&self) -> Subscription<Message> {
        // `Subscription::batch` merges multiple subscriptions into one stream.
        Subscription::batch([
            // Tick every second.  We use `map(|_| …)` rather than keeping the
            // `Instant` because we don't need it (the snapshot collection is
            // wall-clock independent).
            time::every(Duration::from_secs(1)).map(|_| Message::Tick(())),

            // Listen for ALL Iced events and forward them so `update()` can
            // inspect keyboard presses for navigation.
            event::listen().map(Message::IcedEvent),
        ])
    }

    /// Returns the Iced `Theme` used for all widgets.
    ///
    /// We build a custom theme from the project's colour palette so that
    /// widgets like `text_input`, `scrollable`, and `horizontal_rule` all use
    /// the dark background by default rather than Iced's built-in light theme.
    pub fn theme(&self) -> Theme {
        Theme::custom(
            "WRDark".into(),
            iced::theme::Palette {
                background: BG,
                text: FG,
                primary: ACCENT,
                success: Color { r: 0.30, g: 0.80, b: 0.40, a: 1.0 },
                danger: Color { r: 0.90, g: 0.20, b: 0.20, a: 1.0 },
            },
        )
    }

    // ── View ─────────────────────────────────────────────────────────────────

    /// The MVU *view* function.  Called after every state mutation.
    ///
    /// Returns the complete widget tree for this frame.  This is a pure
    /// function of `&self` — it never mutates state.
    pub fn view(&self) -> Element<'_, Message> {
        // Show a placeholder while the first snapshot is in flight.
        let snap = match &self.snapshot {
            Some(s) => s,
            None => return loading_view(),
        };

        // Pre-compute per-process CPU% and I/O-rate maps.
        // These are O(n) HashMaps keyed by (pid, start_ticks) so that we never
        // confuse a new process that happens to reuse an old PID.
        let cpu_map = rates(snap, self.previous.as_ref());
        let io_map = io_rates(snap, self.previous.as_ref());

        // Determine the current user's UID for the "Mine" filter.
        let uid = current_uid();

        // Build the set of PIDs whose parent is kthreadd (PID 2).
        // We use this in the UserApps filter to catch any kernel thread that
        // doesn't match a known name pattern.
        let kthreadd_kids: HashSet<u32> = snap
            .processes
            .iter()
            .filter(|p| p.ppid.copied() == Some(2))
            .map(|p| p.pid)
            .collect();

        // Apply filter + search → list of visible processes.
        let search_lc = self.search.to_lowercase();
        let filtered: Vec<&Process> = snap
            .processes
            .iter()
            .filter(|p| self.passes_filter(p, &kthreadd_kids, uid))
            .filter(|p| {
                search_lc.is_empty()
                    || p.name
                        .value()
                        .is_some_and(|n| n.to_lowercase().contains(&search_lc))
            })
            .collect();

        // Group filtered processes by short name, then build DisplayRows.
        let display_rows = self.build_display_rows(&filtered, &cpu_map, &io_map);

        // ── Assemble the full widget tree ─────────────────────────────────────
        let ui = column![
            self.build_header(snap),
            horizontal_rule(1),
            self.build_toolbar(),
            horizontal_rule(1),
            self.build_col_headers(),
            horizontal_rule(1),
            self.build_process_list(&display_rows),
            horizontal_rule(1),
            self.build_detail(snap, &cpu_map, &io_map),
        ]
        .spacing(0);

        container(ui)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(BG)),
                ..Default::default()
            })
            .into()
    }

    // ══════════════════════════════════════════════════════════════════════════
    // DATA HELPERS
    // ══════════════════════════════════════════════════════════════════════════

    /// Groups the filtered process list by name and builds the ordered sequence
    /// of `DisplayRow`s that the list renderer will iterate over.
    ///
    /// The ordering rules are:
    /// * Groups are sorted by the *active sort column* using the group's
    ///   aggregate value (sum CPU, sum RSS, group name, etc.).
    /// * Within an expanded group, child processes are always sorted by CPU%
    ///   descending so the busiest child appears first.
    fn build_display_rows<'a>(
        &self,
        filtered: &[&'a Process],
        cpu_map: &HashMap<(u32, Option<u64>), f64>,
        io_map: &HashMap<(u32, Option<u64>), (Option<f64>, Option<f64>)>,
    ) -> Vec<DisplayRow<'a>> {
        // ── Step 1: partition into groups (by short name) ─────────────────────
        // We use an insertion-ordered Vec<(name, members)> to preserve a
        // stable base order before sorting.
        let mut group_map: HashMap<&str, Vec<&Process>> = HashMap::new();
        let mut group_order: Vec<&str> = Vec::new();

        for &p in filtered {
            let name = p.name.value().map(|s| s.as_str()).unwrap_or("?");
            if !group_map.contains_key(name) {
                group_order.push(name);
            }
            group_map.entry(name).or_default().push(p);
        }

        // ── Step 2: compute per-group aggregate values ────────────────────────
        // (total_cpu, total_rss, min_status_priority) for sorting.
        struct GroupAgg {
            total_cpu: f64,
            total_rss: u64,
        }
        let agg: HashMap<&str, GroupAgg> = group_map
            .iter()
            .map(|(&name, members)| {
                let total_cpu = members
                    .iter()
                    .map(|p| cpu_map.get(&(p.pid, p.start_ticks.copied())).copied().unwrap_or(0.0))
                    .sum();
                let total_rss = members.iter().map(|p| p.rss_kib.copied().unwrap_or(0)).sum();
                (name, GroupAgg { total_cpu, total_rss })
            })
            .collect();

        // ── Step 3: sort groups by the active column ──────────────────────────
        group_order.sort_by(|&a, &b| {
            let ga = &agg[a];
            let gb = &agg[b];
            match self.sort {
                SortCol::Name | SortCol::Status => {
                    // Name and Status both sort groups alphabetically by name.
                    if self.sort_asc { a.cmp(b) } else { b.cmp(a) }
                }
                SortCol::Cpu => {
                    if self.sort_asc {
                        ga.total_cpu.partial_cmp(&gb.total_cpu)
                    } else {
                        gb.total_cpu.partial_cmp(&ga.total_cpu)
                    }
                    .unwrap_or(std::cmp::Ordering::Equal)
                }
                SortCol::Memory => {
                    if self.sort_asc { ga.total_rss.cmp(&gb.total_rss) } else { gb.total_rss.cmp(&ga.total_rss) }
                }
                SortCol::ReadRate => {
                    let ra: f64 = group_map[a].iter().filter_map(|p| io_map.get(&(p.pid, p.start_ticks.copied())).and_then(|v| v.0)).sum();
                    let rb: f64 = group_map[b].iter().filter_map(|p| io_map.get(&(p.pid, p.start_ticks.copied())).and_then(|v| v.0)).sum();
                    if self.sort_asc { ra.partial_cmp(&rb) } else { rb.partial_cmp(&ra) }
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                SortCol::WriteRate => {
                    let wa: f64 = group_map[a].iter().filter_map(|p| io_map.get(&(p.pid, p.start_ticks.copied())).and_then(|v| v.1)).sum();
                    let wb: f64 = group_map[b].iter().filter_map(|p| io_map.get(&(p.pid, p.start_ticks.copied())).and_then(|v| v.1)).sum();
                    if self.sort_asc { wa.partial_cmp(&wb) } else { wb.partial_cmp(&wa) }
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        });

        // ── Step 4: flatten into DisplayRows ──────────────────────────────────
        let mut rows: Vec<DisplayRow<'a>> = Vec::with_capacity(filtered.len() + group_order.len());

        for name in group_order {
            let members = &group_map[name];
            let g = &agg[name];
            let expanded = !self.collapsed.contains(name);

            rows.push(DisplayRow::Group {
                name,
                count: members.len(),
                total_cpu: g.total_cpu,
                total_rss_kib: g.total_rss,
                expanded,
            });

            if expanded {
                // Sort children by CPU% descending (busiest first) within the
                // group, regardless of the outer sort column.
                let mut sorted_members = members.clone();
                sorted_members.sort_by(|a, b| {
                    let ca = cpu_map.get(&(a.pid, a.start_ticks.copied())).copied().unwrap_or(0.0);
                    let cb = cpu_map.get(&(b.pid, b.start_ticks.copied())).copied().unwrap_or(0.0);
                    cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
                });

                for &p in &sorted_members {
                    let cpu = cpu_map.get(&(p.pid, p.start_ticks.copied())).copied().unwrap_or(0.0);
                    let (read_rate, write_rate) = io_map
                        .get(&(p.pid, p.start_ticks.copied()))
                        .map(|&(r, w)| (r, w))
                        .unwrap_or((None, None));
                    rows.push(DisplayRow::Child { process: p, cpu, read_rate, write_rate });
                }
            }
        }

        rows
    }

    /// Move the selection cursor up (`delta = -1`) or down (`delta = 1`) by
    /// finding all selectable child PIDs in display order and shifting the
    /// current index.
    fn move_selection(&mut self, delta: i64) {
        // We need the snapshot to build the display rows; if we don't have one
        // yet, keyboard nav is a no-op.
        let snap = match &self.snapshot {
            Some(s) => s,
            None => return,
        };
        let cpu_map = rates(snap, self.previous.as_ref());
        let io_map = io_rates(snap, self.previous.as_ref());
        let uid = current_uid();
        let kthreadd_kids: HashSet<u32> = snap.processes.iter()
            .filter(|p| p.ppid.copied() == Some(2)).map(|p| p.pid).collect();
        let search_lc = self.search.to_lowercase();
        let filtered: Vec<&Process> = snap.processes.iter()
            .filter(|p| self.passes_filter(p, &kthreadd_kids, uid))
            .filter(|p| search_lc.is_empty() || p.name.value().is_some_and(|n| n.to_lowercase().contains(&search_lc)))
            .collect();

        let rows = self.build_display_rows(&filtered, &cpu_map, &io_map);

        // Collect just the PIDs of visible child rows (group headers are not
        // selectable via keyboard).
        let pids: Vec<u32> = rows.iter().filter_map(|r| match r {
            DisplayRow::Child { process, .. } => Some(process.pid),
            _ => None,
        }).collect();

        if pids.is_empty() { return; }

        let current_idx = self.selected
            .and_then(|pid| pids.iter().position(|&p| p == pid))
            .unwrap_or(0);

        let new_idx = (current_idx as i64 + delta)
            .clamp(0, pids.len() as i64 - 1) as usize;

        self.selected = Some(pids[new_idx]);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // WIDGET BUILDERS
    // ══════════════════════════════════════════════════════════════════════════

    /// Renders the top header bar: app title + live system stats.
    fn build_header(&self, snap: &Snapshot) -> Element<'_, Message> {
        let title = text("⚡  What's Running?")
            .size(18)
            .font(Font::MONOSPACE)
            .color(ACCENT);

        let stats = format!(
            "RAM  {} / {} MiB     Swap  {} / {} MiB     {} processes",
            used(snap.memory.total_kib, snap.memory.available_kib),
            mib(snap.memory.total_kib),
            used(snap.memory.swap_total_kib, snap.memory.swap_free_kib),
            mib(snap.memory.swap_total_kib),
            snap.processes.len(),
        );

        container(
            row![title, text(stats).size(13).color(FG_DIM).font(Font::MONOSPACE)]
                .spacing(28)
                .align_y(iced::Alignment::Center),
        )
        .padding([10, 16])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(BG2)),
            ..Default::default()
        })
        .into()
    }

    /// Renders the toolbar: filter buttons, tree-toggle, search box.
    fn build_toolbar(&self) -> Element<'_, Message> {
        // Helper closure that creates a styled toggle button.
        // Using `&'static str` for `label` avoids lifetime issues with the
        // returned `Button<'_, Message>` which is invariant over its lifetime.
        let filter_btn = |label: &'static str, mode: FilterMode| {
            let active = self.filter == mode;
            button(
                text(label)
                    .size(12)
                    .font(Font::MONOSPACE)
                    .color(if active { BG } else { FG }),
            )
            .padding([5, 14])
            .style(move |_, _| button::Style {
                background: Some(iced::Background::Color(if active { ACCENT } else { BG3 })),
                border: iced::Border { radius: 5.0.into(), ..Default::default() },
                text_color: if active { BG } else { FG },
                ..Default::default()
            })
            .on_press(Message::SetFilter(mode))
        };

        let search = text_input("  search…", &self.search)
            .on_input(Message::SearchChanged)
            .padding([5, 10])
            .size(13)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(240.0));

        container(
            row![
                filter_btn("All", FilterMode::All),
                filter_btn("Apps", FilterMode::UserApps),
                filter_btn("Mine", FilterMode::Mine),
                iced::widget::horizontal_space(),
                search,
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding([8, 16])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(BG2)),
            ..Default::default()
        })
        .into()
    }

    /// Renders the sticky column-header row.
    ///
    /// Each header is a button so clicking it fires `Message::SortBy`.
    /// The active sort column shows an up/down arrow in the accent colour.
    fn build_col_headers(&self) -> Element<'_, Message> {
        // Inner helper: make one header cell.
        let hdr = |label: &str, col: SortCol, w: f32| {
            let active = self.sort == col;
            let arrow = if active { if self.sort_asc { " ▲" } else { " ▼" } } else { "" };
            button(
                text(format!("{label}{arrow}"))
                    .size(12)
                    .font(Font::MONOSPACE)
                    .color(if active { ACCENT } else { FG_DIM }),
            )
            .padding([7, 8])
            .width(Length::Fixed(w))
            .style(|_, _| button::Style {
                background: Some(iced::Background::Color(BG3)),
                border: iced::Border::default(),
                text_color: FG_DIM,
                ..Default::default()
            })
            .on_press(Message::SortBy(col))
        };

        container(
            row![
                hdr("Name", SortCol::Name, W_NAME),
                hdr("Status", SortCol::Status, W_STATUS),
                hdr("CPU %", SortCol::Cpu, W_CPU),
                hdr("Memory", SortCol::Memory, W_MEM),
                hdr("Read/s", SortCol::ReadRate, W_READ),
                hdr("Write/s", SortCol::WriteRate, W_WRITE),
            ]
            .spacing(1),
        )
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(BG3)),
            ..Default::default()
        })
        .into()
    }

    /// Renders the scrollable process list from pre-built `DisplayRow`s.
    fn build_process_list<'a>(&self, rows: &[DisplayRow<'a>]) -> Element<'a, Message> {
        let selected = self.selected;

        let widgets: Vec<Element<'_, Message>> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| match row {
                // ── Group header row ──────────────────────────────────────────
                DisplayRow::Group { name, count, total_cpu, total_rss_kib, expanded } => {
                    let chevron = if *expanded { "▼ " } else { "▶ " };
                    // Show "(N)" count badge only when the group has >1 member.
                    let badge = if *count > 1 { format!("  ({})", count) } else { String::new() };
                    let label = format!("{chevron}{name}{badge}");

                    let rss_mb = *total_rss_kib as f64 / 1024.0;
                    let status_str = "—"; // groups don't have a single status

                    let name_cell = text(label)
                        .size(13)
                        .font(Font::MONOSPACE)
                        .color(FG)
                        .width(Length::Fixed(W_NAME));

                    let status_cell = text(status_str)
                        .size(12)
                        .font(Font::MONOSPACE)
                        .color(FG_DIM)
                        .width(Length::Fixed(W_STATUS));

                    let cpu_cell = heat_cell(*total_cpu, 100.0, W_CPU, format!("{:.1}%", total_cpu));
                    let mem_cell = heat_cell(rss_mb, 8192.0, W_MEM, fmt_mem(rss_mb));

                    let read_cell = text("—")
                        .size(12).font(Font::MONOSPACE).color(FG_DIM)
                        .width(Length::Fixed(W_READ));
                    let write_cell = text("—")
                        .size(12).font(Font::MONOSPACE).color(FG_DIM)
                        .width(Length::Fixed(W_WRITE));

                    let inner = row![name_cell, status_cell, cpu_cell, mem_cell, read_cell, write_cell]
                        .spacing(1)
                        .align_y(iced::Alignment::Center);

                    // Group headers are buttons that toggle expand/collapse.
                    let owned_name = name.to_string();
                    button(inner)
                        .padding([5, 8])
                        .width(Length::Fill)
                        .style(move |_, _| button::Style {
                            background: Some(iced::Background::Color(BG_GROUP)),
                            border: iced::Border::default(),
                            text_color: FG,
                            ..Default::default()
                        })
                        .on_press(Message::ToggleGroup(owned_name))
                        .into()
                }

                // ── Child process row ─────────────────────────────────────────
                DisplayRow::Child { process: p, cpu, read_rate, write_rate } => {
                    let rss_mb = p.rss_kib.copied().unwrap_or(0) as f64 / 1024.0;
                    let is_sel = selected == Some(p.pid);

                    // Alternate row shading for readability.
                    let row_bg = if is_sel {
                        SEL
                    } else if i % 2 == 0 {
                        BG
                    } else {
                        BG2
                    };

                    // Indent child rows by 16px to visually nest them under the
                    // group header chevron.
                    let name_str = p.name.value().map(|s| s.as_str()).unwrap_or("?");
                    let name_label = format!("    {name_str}"); // 4-space indent
                    let name_cell = text(name_label)
                        .size(13)
                        .font(Font::MONOSPACE)
                        .color(if is_sel { FG } else { FG })
                        .width(Length::Fixed(W_NAME));

                    // State char → human-readable label + colour.
                    let (status_str, status_color) = process_status(p.state.copied());
                    let status_cell = text(status_str)
                        .size(11)
                        .font(Font::MONOSPACE)
                        .color(status_color)
                        .width(Length::Fixed(W_STATUS));

                    // CPU% with heat bar behind it.
                    let cpu_cell = heat_cell(*cpu, 100.0, W_CPU, format!("{:.1}%", cpu));

                    // Memory with heat bar (reference = 8 GiB for 100%).
                    let mem_cell = heat_cell(rss_mb, 8192.0, W_MEM, fmt_mem(rss_mb));

                    let read_cell = text(fmt_rate(*read_rate))
                        .size(12).font(Font::MONOSPACE).color(FG_DIM)
                        .width(Length::Fixed(W_READ));
                    let write_cell = text(fmt_rate(*write_rate))
                        .size(12).font(Font::MONOSPACE).color(FG_DIM)
                        .width(Length::Fixed(W_WRITE));

                    let pid = p.pid;
                    let inner = row![name_cell, status_cell, cpu_cell, mem_cell, read_cell, write_cell]
                        .spacing(1)
                        .align_y(iced::Alignment::Center);

                    button(inner)
                        .padding([4, 8])
                        .width(Length::Fill)
                        .style(move |_, _| button::Style {
                            background: Some(iced::Background::Color(row_bg)),
                            border: iced::Border::default(),
                            text_color: FG,
                            ..Default::default()
                        })
                        .on_press(Message::SelectRow(pid))
                        .into()
                }
            })
            .collect();

        scrollable(column(widgets).spacing(0).width(Length::Fill))
            .height(Length::Fill)
            .into()
    }

    /// Renders the fixed detail panel at the bottom of the window.
    ///
    /// Shows extended information about the currently selected process, or a
    /// placeholder hint when nothing is selected.
    fn build_detail(
        &self,
        snap: &Snapshot,
        cpu_map: &HashMap<(u32, Option<u64>), f64>,
        io_map: &HashMap<(u32, Option<u64>), (Option<f64>, Option<f64>)>,
    ) -> Element<'_, Message> {
        let content: Element<'_, Message> = if let Some(pid) = self.selected {
            if let Some(p) = snap.processes.iter().find(|p| p.pid == pid) {
                let exe = p.executable.value().map(|s| s.as_str()).unwrap_or("?");
                let uid_str = p.uid.copied().map(|v| v.to_string()).unwrap_or_else(|| "?".into());
                let ppid_str = p.ppid.copied().map(|v| v.to_string()).unwrap_or_else(|| "?".into());
                let rss_mb = p.rss_kib.copied().unwrap_or(0) as f64 / 1024.0;
                let (status_str, _) = process_status(p.state.copied());
                let cpu = cpu_map.get(&(p.pid, p.start_ticks.copied())).copied().unwrap_or(0.0);
                let unit = p.systemd_unit.value().map(|s| s.as_str()).unwrap_or("—");
                let cgroup = p.cgroup.value().map(|s| s.as_str()).unwrap_or("—");
                let (read_r, write_r) = io_map
                    .get(&(p.pid, p.start_ticks.copied()))
                    .map(|&(r, w)| (r, w))
                    .unwrap_or((None, None));

                column![
                    // First line: PID, state, CPU, memory summary
                    text(format!(
                        "PID {pid}  ·  PPID {ppid_str}  ·  UID {uid_str}  ·  {status_str}  ·  \
                         CPU {cpu:.1}%  ·  RSS {rss_mb:.1} MB  ·  \
                         Read {}  ·  Write {}",
                        fmt_rate(read_r),
                        fmt_rate(write_r),
                    ))
                    .size(12)
                    .font(Font::MONOSPACE)
                    .color(FG_DIM),
                    // Second line: executable path
                    text(format!("exe    {exe}"))
                        .size(12)
                        .font(Font::MONOSPACE)
                        .color(FG),
                    // Third line: systemd unit + cgroup
                    text(format!("unit   {unit}  ·  cgroup  {cgroup}"))
                        .size(11)
                        .font(Font::MONOSPACE)
                        .color(FG_DIM),
                ]
                .spacing(3)
                .into()
            } else {
                // The process exited between the click and this render.
                text("(process exited)").size(12).color(FG_DIM).into()
            }
        } else {
            text("↑ Click a process row to see details  ·  j/k or ↑/↓ to navigate")
                .size(12)
                .font(Font::MONOSPACE)
                .color(FG_DIM)
                .into()
        };

        container(content)
            .padding([10, 16])
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(BG2)),
                ..Default::default()
            })
            .into()
    }

    // ══════════════════════════════════════════════════════════════════════════
    // FILTER HELPERS
    // ══════════════════════════════════════════════════════════════════════════

    /// Returns `true` if process `p` should be visible given the current
    /// `self.filter` setting.
    ///
    /// `kthreadd_kids` is the pre-computed set of PIDs whose parent is PID 2
    /// (the kernel thread daemon).  Passing it in avoids recomputing it per
    /// process.
    fn passes_filter(&self, p: &Process, kthreadd_kids: &HashSet<u32>, uid: Option<u32>) -> bool {
        match self.filter {
            FilterMode::All => true,
            FilterMode::Mine => p.uid.copied() == uid,
            FilterMode::UserApps => {
                let pid = p.pid;
                let ppid = p.ppid.copied();
                let name = p.name.value().map(|s| s.as_str()).unwrap_or("");
                // Keep a process only if:
                //   · It is not PID 2 (kthreadd) itself
                //   · Its parent is not kthreadd
                //   · It is not in the kthreadd_kids set (catches renaming)
                //   · Its name does not match any known kernel thread pattern
                pid != 2
                    && ppid != Some(2)
                    && !kthreadd_kids.contains(&pid)
                    && !is_kernel_thread_name(name)
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// FREE HELPER FUNCTIONS
// ══════════════════════════════════════════════════════════════════════════════

/// Returns the widget shown while the first snapshot is being collected.
fn loading_view() -> Element<'static, Message> {
    container(
        column![
            text("⚡  What's Running?")
                .size(22)
                .font(Font::MONOSPACE)
                .color(ACCENT),
            text("Collecting process data…")
                .size(14)
                .font(Font::MONOSPACE)
                .color(FG_DIM),
        ]
        .spacing(8)
        .align_x(iced::Alignment::Center),
    )
    .center(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(BG)),
        ..Default::default()
    })
    .into()
}

/// Converts a Linux process state character to a human-readable label and a
/// display colour.
///
/// | Char | Meaning           | Colour |
/// |------|-------------------|--------|
/// | R    | Running / Runnable | green |
/// | S    | Interruptible sleep | grey |
/// | D    | Uninterruptible sleep (I/O wait) | amber |
/// | I    | Idle kernel thread | grey |
/// | T    | Stopped (SIGSTOP) | amber |
/// | Z    | Zombie            | red |
/// | X    | Dead              | red |
/// | _    | Unknown           | grey |
fn process_status(state: Option<char>) -> (&'static str, Color) {
    match state {
        Some('R') => ("Running",  STATUS_RUNNING),
        Some('S') => ("Sleeping", STATUS_SLEEPING),
        Some('D') => ("Disk wait",STATUS_OTHER),
        Some('I') => ("Idle",     STATUS_SLEEPING),
        Some('T') => ("Stopped",  STATUS_OTHER),
        Some('Z') => ("Zombie",   STATUS_ZOMBIE),
        Some('X') => ("Dead",     STATUS_ZOMBIE),
        _ =>         ("—",        STATUS_SLEEPING),
    }
}

/// Builds a "heat cell" widget: a text label with a semi-transparent coloured
/// bar rendered behind it, proportional to `value / max`.
///
/// This gives a quick visual heat-map glance across the entire list without
/// needing a separate sparkline or progress bar widget.
///
/// # Arguments
/// * `value` – the raw numeric value (e.g. CPU% or RSS MB)
/// * `max`   – the value that represents 100% of the bar width
/// * `width` – the fixed pixel width of the cell
/// * `label` – the formatted text to overlay on top of the bar
fn heat_cell<'a>(value: f64, max: f64, width: f32, label: String) -> Element<'a, Message> {
    // Clamp fraction to [0, 1] to avoid bar overflow on abnormal readings.
    let frac = (value / max).clamp(0.0, 1.0) as f32;

    // Pick bar colour based on how hot the value is (same thresholds as
    // `cpu_color` but using the translucent heat constants).
    let bar_color = if frac >= 0.5 {
        HEAT_HIGH
    } else if frac >= 0.15 {
        HEAT_MED
    } else {
        HEAT_LOW
    };

    // We use a Canvas to draw the heat bar rectangle behind the text.
    // The canvas is fixed-size and layered using a `container` → `stack`
    // approach: canvas goes first (background), text goes on top.
    let bar = canvas(HeatBar { frac, color: bar_color })
        .width(Length::Fixed(width))
        .height(Length::Fixed(22.0));

    // Overlay the text label centered in the same space.
    let label_widget = text(label)
        .size(12)
        .font(Font::MONOSPACE)
        .color(FG)
        .width(Length::Fixed(width));

    // Use iced's built-in `stack` to layer bar behind label.
    iced::widget::stack![bar, container(label_widget).padding([3, 8]).center_y(Length::Fixed(22.0))]
        .width(Length::Fixed(width))
        .into()
}

/// A custom Iced `Program` (canvas geometry) for the heat bar.
///
/// Draws a filled rectangle from x=0 to x=`frac * width` in `color`,
/// then leaves the remainder transparent.
struct HeatBar {
    /// Fill fraction [0.0, 1.0].
    frac: f32,
    /// Bar fill colour (semi-transparent).
    color: Color,
}

impl<Message> canvas::Program<Message> for HeatBar {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // Fill the left portion of the frame with the heat colour.
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size {
                width: bounds.width * self.frac,
                height: bounds.height,
            },
            self.color,
        );
        vec![frame.into_geometry()]
    }
}

/// Returns `true` if the given process short name belongs to a kernel thread.
///
/// We match both prefix patterns (e.g. `"kworker/"`) and exact names
/// (e.g. `"kthreadd"`).  This list is comprehensive for a modern Linux 6.x
/// kernel running on x86-64 with common storage and GPU drivers.
fn is_kernel_thread_name(name: &str) -> bool {
    // These are *prefix* patterns — a process whose name starts with any of
    // these strings is considered a kernel thread.
    const PREFIXES: &[&str] = &[
        "kworker/", "kthread", "ksoftirqd", "migration/", "idle_inject/",
        "rcu_", "rcub/", "rcuc/", "kswapd", "kcompactd", "khugepaged",
        "kdevtmpfs", "netns", "kauditd", "khungtaskd", "oom_reaper",
        "writeback", "kblockd", "blkcg_punt_bio", "edac-poller", "devfreq_wq",
        "watchdogd", "watchdog/", "irq/", "i915", "nouveau", "amdgpu",
        "jbd2/", "ext4-", "xfs-", "btrfs", "cryptd", "zswap-", "dm-",
        "hwrng", "acpi_thermal_pm", "ipv6_addrconf", "nfsiod", "scsi_",
        "usb-storage", "cfg80211", "rpciod", "xprtiod", "pool_workqueue",
        "nvme-wq", "nvme-reset-wq", "nvme-delete-wq", "mld", "inet_frag_wq",
        "bioset", "ttm", "drm_sched", "drm-", "card", "md",
    ];
    // These are *exact* names — used for short names that would incorrectly
    // match as prefixes of legitimate user-space processes.
    const EXACT: &[&str] = &["kthreadd", "md"];

    name.is_empty()
        || PREFIXES.iter().any(|p| name.starts_with(p))
        || EXACT.iter().any(|e| name == *e)
}

/// Reads the real UID of the current process from `/proc/self/status`.
///
/// Returns `None` on any parse failure (e.g. running inside a container that
/// doesn't expose /proc/self).
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

/// Formats a memory value in MiB or GiB with appropriate precision.
///
/// * < 1024 MB  → `"123.4 MB"`
/// * ≥ 1024 MB  → `"1.23 GB"`
fn fmt_mem(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{:.1} MB", mb)
    }
}

/// Formats a byte rate value with an appropriate unit suffix.
///
/// * `None`           → `"?"`
/// * < 1 B/s          → `"0"`
/// * < 1024 B/s       → `"N B/s"`
/// * < 1 MiB/s        → `"N.N KB/s"`
/// * ≥ 1 MiB/s        → `"N.N MB/s"`
fn fmt_rate(v: Option<f64>) -> String {
    match v {
        None => "?".into(),
        Some(v) if v >= 1_048_576.0 => format!("{:.1} MB/s", v / 1_048_576.0),
        Some(v) if v >= 1024.0 => format!("{:.1} KB/s", v / 1024.0),
        Some(v) if v < 1.0 => "0".into(),
        Some(v) => format!("{v:.0} B/s"),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// ENTRY POINT
// ══════════════════════════════════════════════════════════════════════════════

/// Initialises and runs the Iced window.
///
/// Called from `main.rs` when `--gui` is passed.  Returns `iced::Result` which
/// is either `Ok(())` on clean exit or an Iced platform error.
pub fn run() -> iced::Result {
    iced::application(App::title, App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .window_size((980.0, 680.0))
        .run_with(App::new)
}
