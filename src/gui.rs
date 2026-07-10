use crate::procfs::{Process, Snapshot, collect_snapshot};
use crate::{io_rates, mib, rates, used};
use iced::widget::{
    button, column, container, horizontal_rule, row, scrollable, text, text_input,
};
use iced::{Color, Element, Font, Length, Subscription, Task, Theme};
use iced::time;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

// ── Colour palette ────────────────────────────────────────────────────────────
const BG: Color = Color { r: 0.051, g: 0.059, b: 0.078, a: 1.0 }; // #0d0f14
const BG2: Color = Color { r: 0.075, g: 0.086, b: 0.11, a: 1.0 }; // #13161c
const BG3: Color = Color { r: 0.11,  g: 0.125, b: 0.16, a: 1.0 }; // #1c2028
const ACCENT: Color = Color { r: 0.78, g: 0.22, b: 0.55, a: 1.0 }; // magenta
const FG: Color = Color { r: 0.85, g: 0.87, b: 0.91, a: 1.0 };
const FG_DIM: Color = Color { r: 0.50, g: 0.52, b: 0.56, a: 1.0 };
const SEL: Color = Color { r: 0.18, g: 0.20, b: 0.27, a: 1.0 };

// ── Domain types ──────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortCol {
    Name,
    Cpu,
    Memory,
    ReadRate,
    WriteRate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    All,
    UserApps,
    Mine,
}

// ── Messages ─────────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum Message {
    Tick(iced::time::Instant),
    SortBy(SortCol),
    SetFilter(FilterMode),
    SearchChanged(String),
    SelectRow(u32),
    ToggleTree,
}

// ── Application state ─────────────────────────────────────────────────────────
pub struct App {
    snapshot: Option<Snapshot>,
    previous: Option<Snapshot>,
    sort: SortCol,
    sort_asc: bool,
    filter: FilterMode,
    search: String,
    selected: Option<u32>,
    tree: bool,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        (
            Self {
                snapshot: None,
                previous: None,
                sort: SortCol::Cpu,
                sort_asc: false,
                filter: FilterMode::UserApps,
                search: String::new(),
                selected: None,
                tree: false,
            },
            Task::none(),
        )
    }

    pub fn title(&self) -> String {
        "What's Running?".into()
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Tick(_) => {
                let prev = self.snapshot.take();
                if let Ok(snap) = collect_snapshot(false) {
                    if let Some(p) = prev {
                        self.previous = Some(p);
                    }
                    self.snapshot = Some(snap);
                }
            }
            Message::SortBy(col) => {
                if self.sort == col {
                    self.sort_asc = !self.sort_asc;
                } else {
                    self.sort = col;
                    self.sort_asc = col == SortCol::Name;
                }
            }
            Message::SetFilter(f) => self.filter = f,
            Message::SearchChanged(s) => self.search = s,
            Message::SelectRow(pid) => {
                self.selected = if self.selected == Some(pid) { None } else { Some(pid) };
            }
            Message::ToggleTree => self.tree = !self.tree,
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(1)).map(Message::Tick)
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            "WRDark".into(),
            iced::theme::Palette {
                background: BG,
                text: FG,
                primary: ACCENT,
                success: Color { r: 0.3, g: 0.8, b: 0.4, a: 1.0 },
                danger: Color { r: 0.9, g: 0.2, b: 0.2, a: 1.0 },
            },
        )
    }

    pub fn view(&self) -> Element<Message> {
        let snap = match &self.snapshot {
            Some(s) => s,
            None => return loading_view(),
        };
        let cpu_map = rates(snap, self.previous.as_ref());
        let io_map = io_rates(snap, self.previous.as_ref());
        let uid = current_uid();

        // ── Filter ────────────────────────────────────────────────────────────
        let kthreadd_kids: HashSet<u32> = snap
            .processes
            .iter()
            .filter(|p| p.ppid.copied() == Some(2))
            .map(|p| p.pid)
            .collect();

        let search_lc = self.search.to_lowercase();
        let mut list: Vec<&Process> = snap
            .processes
            .iter()
            .filter(|p| self.passes_filter(p, &kthreadd_kids, uid))
            .filter(|p| {
                search_lc.is_empty()
                    || p.name.value().is_some_and(|n| n.to_lowercase().contains(&search_lc))
            })
            .collect();

        // ── Sort ──────────────────────────────────────────────────────────────
        match self.sort {
            SortCol::Cpu => list.sort_by(|a, b| {
                let va = cpu_map.get(&(a.pid, a.start_ticks.copied())).copied().unwrap_or(0.0);
                let vb = cpu_map.get(&(b.pid, b.start_ticks.copied())).copied().unwrap_or(0.0);
                if self.sort_asc { va.partial_cmp(&vb) } else { vb.partial_cmp(&va) }
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortCol::Memory => list.sort_by(|a, b| {
                let va = a.rss_kib.copied().unwrap_or(0);
                let vb = b.rss_kib.copied().unwrap_or(0);
                if self.sort_asc { va.cmp(&vb) } else { vb.cmp(&va) }
            }),
            SortCol::Name => list.sort_by(|a, b| {
                let na = a.name.value().map(|s| s.as_str()).unwrap_or("");
                let nb = b.name.value().map(|s| s.as_str()).unwrap_or("");
                if self.sort_asc { na.cmp(nb) } else { nb.cmp(na) }
            }),
            SortCol::ReadRate => list.sort_by(|a, b| {
                let va = io_map.get(&(a.pid, a.start_ticks.copied())).and_then(|v| v.0).unwrap_or(0.0);
                let vb = io_map.get(&(b.pid, b.start_ticks.copied())).and_then(|v| v.0).unwrap_or(0.0);
                if self.sort_asc { va.partial_cmp(&vb) } else { vb.partial_cmp(&va) }
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortCol::WriteRate => list.sort_by(|a, b| {
                let va = io_map.get(&(a.pid, a.start_ticks.copied())).and_then(|v| v.1).unwrap_or(0.0);
                let vb = io_map.get(&(b.pid, b.start_ticks.copied())).and_then(|v| v.1).unwrap_or(0.0);
                if self.sort_asc { va.partial_cmp(&vb) } else { vb.partial_cmp(&va) }
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        }

        // ── Build UI ──────────────────────────────────────────────────────────
        let header_bar = self.build_header(snap);
        let toolbar = self.build_toolbar();
        let col_headers = self.build_col_headers();
        let process_rows = self.build_rows(&list, &cpu_map, &io_map);
        let detail = self.build_detail(snap);

        let content = column![
            header_bar,
            horizontal_rule(1),
            toolbar,
            horizontal_rule(1),
            col_headers,
            horizontal_rule(1),
            process_rows,
            horizontal_rule(1),
            detail,
        ]
        .spacing(0);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(BG)),
                ..Default::default()
            })
            .into()
    }

    // ── Header bar ────────────────────────────────────────────────────────────
    fn build_header<'a>(&self, snap: &Snapshot) -> Element<'a, Message> {
        let title = text("⚡ What's Running?")
            .size(18)
            .font(Font::MONOSPACE)
            .color(ACCENT);

        let mem_str = format!(
            "RAM  {} / {} MiB     Swap  {} / {} MiB     Processes  {}",
            used(snap.memory.total_kib, snap.memory.available_kib),
            mib(snap.memory.total_kib),
            used(snap.memory.swap_total_kib, snap.memory.swap_free_kib),
            mib(snap.memory.swap_total_kib),
            snap.processes.len(),
        );
        let mem_label = text(mem_str).size(13).color(FG_DIM).font(Font::MONOSPACE);

        container(
            row![title, mem_label]
                .spacing(24)
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

    // ── Toolbar ───────────────────────────────────────────────────────────────
    fn build_toolbar(&self) -> Element<Message> {
        let filter_btn = |label: &'static str, mode: FilterMode| {
            let active = self.filter == mode;
            button(text(label).size(12).font(Font::MONOSPACE).color(if active { BG } else { FG }))
                .padding([4, 12])
                .style(move |_, _| button::Style {
                    background: Some(iced::Background::Color(if active { ACCENT } else { BG3 })),
                    border: iced::Border { radius: 4.0.into(), ..Default::default() },
                    text_color: if active { BG } else { FG },
                    ..Default::default()
                })
                .on_press(Message::SetFilter(mode))
        };

        let tree_active = self.tree;
        let tree_btn =
            button(text(if tree_active { "Tree ✓" } else { "Tree" }).size(12).font(Font::MONOSPACE))
                .padding([4, 12])
                .style(move |_, _| button::Style {
                    background: Some(iced::Background::Color(if tree_active { ACCENT } else { BG3 })),
                    border: iced::Border { radius: 4.0.into(), ..Default::default() },
                    text_color: if tree_active { BG } else { FG },
                    ..Default::default()
                })
                .on_press(Message::ToggleTree);

        let search_box = text_input("🔍  search processes…", &self.search)
            .on_input(Message::SearchChanged)
            .padding([4, 10])
            .size(13)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(260.0));

        container(
            row![
                filter_btn("All", FilterMode::All),
                filter_btn("Apps", FilterMode::UserApps),
                filter_btn("Mine", FilterMode::Mine),
                tree_btn,
                iced::widget::horizontal_space(),
                search_box,
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

    // ── Column headers ────────────────────────────────────────────────────────
    fn build_col_headers(&self) -> Element<Message> {
        let hdr = |label: &str, col: SortCol, w: f32| {
            let active = self.sort == col;
            let arrow = if active { if self.sort_asc { " ▲" } else { " ▼" } } else { "" };
            let lbl = format!("{label}{arrow}");
            button(
                text(lbl)
                    .size(12)
                    .font(Font::MONOSPACE)
                    .color(if active { ACCENT } else { FG_DIM }),
            )
            .padding([6, 8])
            .width(Length::Fixed(w))
            .style(|_, _| button::Style {
                background: Some(iced::Background::Color(BG3)),
                border: iced::Border::default(),
                text_color: FG,
                ..Default::default()
            })
            .on_press(Message::SortBy(col))
        };

        container(
            row![
                hdr("Name", SortCol::Name, 300.0),
                hdr("CPU %", SortCol::Cpu, 80.0),
                hdr("Memory", SortCol::Memory, 100.0),
                hdr("Read/s", SortCol::ReadRate, 90.0),
                hdr("Write/s", SortCol::WriteRate, 90.0),
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

    // ── Process rows ─────────────────────────────────────────────────────────
    fn build_rows<'a>(
        &self,
        list: &[&'a Process],
        cpu_map: &HashMap<(u32, Option<u64>), f64>,
        io_map: &HashMap<(u32, Option<u64>), (Option<f64>, Option<f64>)>,
    ) -> Element<'a, Message> {
        let selected = self.selected;

        let rows: Vec<Element<Message>> = list
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let cpu = cpu_map.get(&(p.pid, p.start_ticks.copied())).copied().unwrap_or(0.0);
                let rss_mb = p.rss_kib.copied().unwrap_or(0) as f64 / 1024.0;
                let (read_r, write_r) = io_map
                    .get(&(p.pid, p.start_ticks.copied()))
                    .copied()
                    .unwrap_or((None, None));

                let name_str = p.name.value().map(|s| s.as_str()).unwrap_or("?");
                let is_sel = selected == Some(p.pid);
                let row_bg = if is_sel {
                    SEL
                } else if i % 2 == 0 {
                    BG
                } else {
                    BG2
                };

                let cpu_color = cpu_color(cpu);
                let name_cell = text(format!("  {name_str}"))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(FG)
                    .width(Length::Fixed(300.0));
                let cpu_cell = text(format!("{cpu:.1}%"))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(cpu_color)
                    .width(Length::Fixed(80.0));
                let mem_cell = text(format!("{rss_mb:.1} MB"))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(FG)
                    .width(Length::Fixed(100.0));
                let read_cell = text(fmt_rate(read_r))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(FG_DIM)
                    .width(Length::Fixed(90.0));
                let write_cell = text(fmt_rate(write_r))
                    .size(13)
                    .font(Font::MONOSPACE)
                    .color(FG_DIM)
                    .width(Length::Fixed(90.0));

                let pid = p.pid;
                let inner_row = row![name_cell, cpu_cell, mem_cell, read_cell, write_cell]
                    .spacing(1)
                    .align_y(iced::Alignment::Center);

                button(inner_row)
                    .padding([3, 4])
                    .width(Length::Fill)
                    .style(move |_, _| button::Style {
                        background: Some(iced::Background::Color(row_bg)),
                        border: iced::Border::default(),
                        text_color: FG,
                        ..Default::default()
                    })
                    .on_press(Message::SelectRow(pid))
                    .into()
            })
            .collect();

        scrollable(
            column(rows).spacing(0).width(Length::Fill),
        )
        .height(Length::Fill)
        .into()
    }

    // ── Detail panel ─────────────────────────────────────────────────────────
    fn build_detail<'a>(&self, snap: &'a Snapshot) -> Element<'a, Message> {
        let content: Element<Message> = if let Some(pid) = self.selected {
            if let Some(p) = snap.processes.iter().find(|p| p.pid == pid) {
                let exe = p.executable.value().map(|s| s.as_str()).unwrap_or("?");
                let uid = p.uid.copied().map(|v| v.to_string()).unwrap_or_else(|| "?".into());
                let state = p.state.copied().unwrap_or('?');
                let ppid = p.ppid.copied().map(|v| v.to_string()).unwrap_or_else(|| "?".into());
                let rss_mb = p.rss_kib.copied().unwrap_or(0) as f64 / 1024.0;
                let unit = p
                    .systemd_unit
                    .value()
                    .map(|s| s.as_str())
                    .unwrap_or("-");

                column![
                    text(format!(
                        "PID {}  ·  PPID {}  ·  UID {}  ·  State {}  ·  RSS {:.1} MB  ·  Unit {}",
                        p.pid, ppid, uid, state, rss_mb, unit
                    ))
                    .size(12)
                    .font(Font::MONOSPACE)
                    .color(FG_DIM),
                    text(format!("exe: {exe}"))
                        .size(12)
                        .font(Font::MONOSPACE)
                        .color(FG),
                ]
                .spacing(4)
                .into()
            } else {
                text("(process no longer running)").size(12).color(FG_DIM).into()
            }
        } else {
            text("Click a process row to see details.")
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

    // ── Filter helper ─────────────────────────────────────────────────────────
    fn passes_filter(&self, p: &Process, kthreadd_kids: &HashSet<u32>, uid: Option<u32>) -> bool {
        match self.filter {
            FilterMode::All => true,
            FilterMode::Mine => p.uid.copied() == uid,
            FilterMode::UserApps => {
                let pid = p.pid;
                let ppid = p.ppid.copied();
                let name = p.name.value().map(|s| s.as_str()).unwrap_or("");
                pid != 2
                    && ppid != Some(2)
                    && !kthreadd_kids.contains(&pid)
                    && !is_kernel_thread_name(name)
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────
fn loading_view() -> Element<'static, Message> {
    container(
        text("Collecting process data…")
            .size(18)
            .font(Font::MONOSPACE)
            .color(ACCENT),
    )
    .center(Length::Fill)
    .style(|_| container::Style {
        background: Some(iced::Background::Color(BG)),
        ..Default::default()
    })
    .into()
}

fn cpu_color(pct: f64) -> Color {
    if pct >= 50.0 {
        Color { r: 0.9, g: 0.2, b: 0.2, a: 1.0 } // red
    } else if pct >= 20.0 {
        Color { r: 1.0, g: 0.55, b: 0.0, a: 1.0 } // orange
    } else if pct >= 5.0 {
        Color { r: 1.0, g: 0.85, b: 0.0, a: 1.0 } // yellow
    } else {
        Color { r: 0.35, g: 0.85, b: 0.45, a: 1.0 } // green
    }
}

fn fmt_rate(v: Option<f64>) -> String {
    match v {
        None => "?".into(),
        Some(v) if v >= 1_048_576.0 => format!("{:.1} MB/s", v / 1_048_576.0),
        Some(v) if v >= 1024.0 => format!("{:.1} KB/s", v / 1024.0),
        Some(v) if v < 1.0 => "0".into(),
        Some(v) => format!("{v:.0} B/s"),
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

fn is_kernel_thread_name(name: &str) -> bool {
    let prefixes = [
        "kworker/", "kthread", "ksoftirqd", "migration/", "idle_inject/",
        "rcu_", "rcub/", "rcuc/", "kswapd", "kcompactd", "khugepaged",
        "kdevtmpfs", "netns", "kauditd", "khungtaskd", "oom_reaper",
        "writeback", "kblockd", "blkcg_punt_bio", "edac-poller", "devfreq_wq",
        "watchdogd", "watchdog/", "irq/", "i915", "nouveau", "amdgpu",
        "jbd2/", "ext4-", "xfs-", "btrfs", "cryptd", "zswap-", "dm-",
        "hwrng", "acpi_thermal_pm", "ipv6_addrconf", "nfsiod", "scsi_",
        "usb-storage", "cfg80211", "rpciod", "xprtiod", "pool_workqueue",
        "nvme-wq", "nvme-reset-wq", "nvme-delete-wq", "mld", "inet_frag_wq",
        "bioset", "ttm", "drm_sched", "drm-", "card",
    ];
    let exact = ["kthreadd", "md"];
    name.is_empty()
        || prefixes.iter().any(|p| name.starts_with(p))
        || exact.iter().any(|e| name == *e)
}

// ── Entry point ───────────────────────────────────────────────────────────────
pub fn run() -> iced::Result {
    iced::application("What's Running?", App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .window_size((900.0, 640.0))
        .run_with(App::new)
}
