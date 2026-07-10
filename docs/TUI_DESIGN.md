# Terminal interface design

Status: Accepted for M3  
Reference: Arch Linux + Hyprland/Wayland

The opt-in TUI uses the same collector and rates as table/JSON output. It is not a daemon and does not use Hyprland IPC.

| Key | Action |
|---|---|
| `q`, Ctrl-C | Exit |
| `j`, `k` | Move selection |
| `s` | Cycle CPU, memory, PID, name sort |
| `f` | Cycle all, current-user, problem filters |
| `t` | Toggle ancestry indentation |
| `d` | Toggle exact details |
| `v` | Cycle processes, services/cgroups, network, timers/mounts, events |
| `/` | Edit free-text search; Enter accepts, Escape clears |
| `r` | Refresh cached broad inventory |

The header always names active sort/filter/tree state. All processes are the default. Filters change presentation only. Selection uses `>` rather than color alone; controls and units are textual; no mouse or color capability is required. Rows are bounded by current terminal height and narrow terminals truncate names.

Tree mode uses parent-first depth-first ordering with cycle protection. ANSI plus Arch's `stty` avoids a framework dependency. Mouse handling remains unnecessary; every operation is keyboard-accessible and does not depend on color.

## Update — 2026-07-10: bounded legend bar

The key table above was previously rendered as one dense bracketed line sharing space with sort/filter/tree/search state, which made it easy to miss. The legend now renders as its own block—separator bar, one legend line in `v`/`/`/`t`/`s`/`f`/`r`/`j`/`k`/`d`/`q` order, separator bar—directly above the process list on every view, still with no color or mouse dependency. The row-budget reservation in `draw()` was widened from 4/8 lines to 6/10 lines (non-detail/detail) to keep the process list from being miscounted against the two added separator lines.
