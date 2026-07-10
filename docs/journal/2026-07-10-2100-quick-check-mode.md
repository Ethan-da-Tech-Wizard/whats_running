# Quick non-interactive check mode

Date: 2026-07-10  
Status: Historical record

## Why

The project owner said, in effect: "I don't really know what I want — I just want something simple to open and tell me what's currently running, so I can see if something's hidden, like Steam not fully quitting, or one of my own programs leaving a localhost port open." The existing surfaces (`--tui`'s five-view interactive inspector, `--inventory`, full `--json`) all assume the user wants to explore. This request was narrower: a single answer, no keys to learn, no dashboard.

Rather than guess at a heuristic ("is Steam running?" hardcoded, or a "leftover process" classifier), the report stays consistent with the project's evidence-only stance: it shows the full, honest process list (name-sorted, so scanning for something unexpected is fast) and every listening TCP/UDP socket with its decoded address and owning process, sourced from the same collector and `/proc/PID/fd` ownership evidence `--tui`'s network view and `--inventory` already use. No new privilege, no new collector, no invented judgment about what counts as "hidden."

## What was added

- `src/check.rs`: a new module with `report()`, plus an IPv4/IPv6 `/proc/net/{tcp,udp}` hex-address decoder (`0100007F:0016` → `127.0.0.1:22`) so ports are readable instead of raw kernel hex. Four unit tests cover IPv4 loopback, IPv4 wildcard, IPv6 loopback, and malformed input.
- `--check` CLI flag in `src/main.rs`, wired ahead of `--inventory` in the dispatch order, documented in `--help`.
- Listening-port detection: TCP sockets in state `0A` (LISTEN); UDP sockets are all treated as listening since UDP has no equivalent connection-state field. Unix sockets are intentionally excluded from this report — they're not reachable the way a TCP/UDP port is, and `--inventory`/`--tui` already cover them for anyone who needs that detail.
- Ownership resolution reuses `activity::collect`'s existing `/proc/PID/fd` inode-to-PID mapping; sockets whose owner isn't visible (permission or namespace boundary) are printed as `unknown owner`, never guessed.

## Verification

- `cargo test --release`: 20/20 passed (16 prior + 4 new `check::tests`).
- `cargo clippy --release -- -D warnings` and `cargo fmt --check`: clean.
- Live run on the real Arch/Hyprland host: correctly listed all running processes including multiple real `steam`/`steamwebhelper` processes with their actual open ports (27036, 27060, 34865, 40723, 57343), confirming the report answers the exact "is Steam still holding something open" question it was built for.
- Live port-attribution check: started a real Python `socket.bind(('127.0.0.1', 18765))` listener in a separate process and confirmed `--check` reported `tcp 127.0.0.1:18765 python3 (pid <correct-pid>)`, proving the ownership mapping and hex-address decoding are both correct, not just plausible-looking.

## Scope note

This is additive to the M0–M3 baseline (`docs/MILESTONES_V2.md`), not a new milestone — it introduces no new collector, privilege, or data domain, only a third, non-interactive way to read data the tool already gathers. `README.md` and `docs/OPERATIONS.md` were updated in the same change to document it as the recommended starting point for someone who isn't sure which mode they want yet.
