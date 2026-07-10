# What's Running?

An honest, local-first answer to a deceptively simple question:

> What is my computer doing in the background right now, and what changed?

The project begins on Arch Linux. It will inventory kernel-visible background activity—including itself—capture lifecycle changes, and expose visibility limitations without pretending that incomplete data is complete.

The first milestone is now usable on Arch Linux: a dependency-free Rust snapshot CLI that reads `/proc`, lists every PID it enumerates, marks itself, records visibility failures, and emits a human table or versioned JSON. The documentation corpus is indexed in [docs/README.md](docs/README.md).

## Quick check

Not sure what you want yet, just want a fast answer to "is anything left running that shouldn't be, and did an app leave a port open"? Run:

```bash
cargo run --release -- --check
```

This prints a plain-text list of every running process (sorted by name, so a leftover like `steam` is easy to spot) followed by every listening TCP/UDP port with the process that owns it (e.g. a dev server you forgot was still bound to `localhost:3000`), then exits. No keys to press, nothing interactive.

## Try it

```bash
cargo run --release -- --check
cargo run --release
cargo run --release -- --json
cargo run --release -- --show-command  # warning: arguments can contain secrets
cargo run --release -- --samples 10 --interval-ms 1000
cargo run --release -- --tui
cargo run --release -- --inventory
cargo run --release -- --inventory --json
# Optional kernel lifecycle capture; run with explicit suitable privilege if denied:
cargo run --release -- --events 50
```

Build and install locally:

```bash
cargo build --release
install -Dm755 target/release/whats-running ~/.local/bin/whats-running
```

The program is read-only, needs no root privileges, makes no network requests, and has no third-party Rust dependencies.

## TUI controls

`--tui` opens an interactive inspector. Its keybinding legend is always shown on-screen between two separator bars, directly above the process list:

- `v` — cycle views (processes, services/cgroups, network, timers/mounts, events)
- `/` — edit free-text search (Enter accepts, Escape clears)
- `t` — toggle genuine parent-first process tree
- `s` — cycle sort (CPU, memory, PID, name)
- `f` — cycle filter (all, current-user, problems)
- `r` — refresh the cached broader inventory
- `j` / `k` — move selection
- `d` — toggle exact process details
- `q` — quit (Ctrl-C also works; SIGTERM/SIGHUP restore the terminal safely)

## Non-negotiable product principles

1. **No intentional hiding.** The observer appears in its own output.
2. **Truth includes uncertainty.** Permission failures, races, kernel threads, vanished processes, and unsupported metrics are explicitly marked.
3. **Local first.** No telemetry, accounts, cloud backend, or network dependency in the initial product.
4. **Read-only by default.** Observation is the core job; process control is outside the first release.
5. **KISS.** Prefer a small, inspectable system over a magical dashboard.
6. **Documentation is historical evidence.** Accepted records are not silently rewritten or deleted; changes are appended or superseded.

## Current status

The accepted background-activity M0–M3 milestones are complete: typed process/thread lifecycle, broader system inventory, bounded recording, exact export, and a multi-view terminal inspector. No mandatory gate is open. Historical prototype closures remain preserved in the documentation journal.

A non-interactive `--check` report was added on top of that baseline for the common case of "just tell me what's running and what ports are open, no dashboard required." See [docs/journal/2026-07-10-2100-quick-check-mode.md](docs/journal/2026-07-10-2100-quick-check-mode.md).

No M4 is defined yet. Portability beyond Arch/systemd/cgroup-v2 was proposed and then explicitly rejected — this project targets Arch Linux only, in order to ship on the AUR (see [ADR-0009](docs/decisions/0009-arch-only-scope-lock.md)). Active next steps are CI automation and AUR packaging; see [docs/journal/2026-07-10-2200-ci-and-aur-packaging.md](docs/journal/2026-07-10-2200-ci-and-aur-packaging.md). Deeper container visibility and an in-TUI help overlay remain proposed, not accepted.
