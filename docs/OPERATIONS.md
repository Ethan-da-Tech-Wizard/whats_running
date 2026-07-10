# Operations guide

Status: Verified for M1 on Arch Linux, 2026-07-10

## Requirements

- Linux with procfs mounted at `/proc`.
- Rust/Cargo 1.96 or a compatible edition-2024 toolchain to build.
- No runtime library, daemon, root permission, or network connection.

## Commands

```bash
cargo run --release -- --check
cargo run --release
cargo run --release -- --json
cargo run --release -- --show-command
cargo run --release -- --samples 10 --interval-ms 1000
cargo run --release -- --tui
cargo run --release -- --inventory
cargo run --release -- --inventory --json
cargo run --release -- --events 50
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

`--show-command` can expose tokens, URLs, filenames, and other secrets supplied as arguments. It is off by default. Environment variables are never collected.

Multiple samples enable CPU/I/O rates and observed start/exit counts. Input is capped at 120 snapshots. JSON schema v2 stores all requested samples in one `samples` array, so history is valid JSON rather than concatenated documents.

## Quick non-interactive check

`--check` prints two plain-text sections and exits: every running process (name-sorted, so an unexpected leftover such as a game client that didn't fully quit stands out) and every listening TCP/UDP port decoded to a human `address:port` form alongside the owning process name and PID (`unknown owner` when `/proc/PID/fd` evidence isn't visible, e.g. another user's process). It reuses the same collector and socket-ownership evidence as `--tui`'s network view and `--inventory`; it adds no new privilege requirement. This is the fastest way to answer "is anything hidden still running" or "did one of my own programs leave a local port open" without learning the interactive keys below.

## Interactive terminal

`--tui` requires a terminal. Keys are immediate and the full legend is always rendered on-screen, bounded by separator bars directly above the process list, so it never has to be memorized:

| Key | Action |
|---|---|
| `v` | Cycle five domain views |
| `/` | Edit free-text search; Enter accepts, Escape clears |
| `t` | Toggle genuine process tree |
| `s` | Cycle sort |
| `f` | Cycle filters |
| `r` | Refresh the cached broad inventory |
| `j`, `k` | Move selection |
| `d` | Toggle details |
| `q`, Ctrl-C | Exit |

Details expose command arguments only when `--show-command` was explicitly supplied.

`--events N` uses the Arch kernel process connector and may require explicit elevation/capability. The program never invokes `sudo`. Add `--record PATH --max-record-bytes N` for a finite command-free event journal created with mode `0600`.

The Arch implementation uses GNU `stty` for raw mode while retaining zero Rust dependencies. Dimensions are queried each frame. After a force-kill, use `stty sane` if the terminal was not restored.

## Local install/uninstall

```bash
cargo build --release
install -Dm755 target/release/whats-running ~/.local/bin/whats-running
rm ~/.local/bin/whats-running
```

The removal command is documented for the human operator; the application never installs or removes itself.

## Reading output

The first line reports the observer PID, enumerated count, directory-enumeration errors, and collection time. `[THIS TOOL]` proves the current observer record was not intentionally filtered. `VISIBILITY` says `complete` only for the three string resources currently audited (name, command, executable), not for universal system visibility.

## Troubleshooting

- Fewer processes than expected: check PID namespaces, containers, and procfs `hidepid` options.
- `permission_denied`: rerunning as root may reveal more, but is not recommended as the default and cannot guarantee omniscience.
- `vanished`: normal during process churn; the process exited between enumeration and reading.
- Empty-looking kernel command: kernel threads commonly have empty `cmdline`; their stat name remains available.
- Broken pipe while piping into a short-lived consumer can produce Rust's stdout error behavior; direct terminal and complete-file output are the supported M1 paths.
