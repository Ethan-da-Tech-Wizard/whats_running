# Scope and scope lock

Status: Proposed baseline v0.1

## MVP: Arch Linux, terminal-first

In scope:

- Periodically enumerate the process view available through Linux `/proc`.
- Include the tool's own PID and resource usage.
- Show PID, PPID, process/thread distinction, user, state, name, command line with privacy controls, executable path, start time, CPU time/rate, RSS, virtual memory, read/write accounting when available, and observation errors.
- Show process ancestry and grouped views.
- Show system context: load, CPU totals, memory/swap pressure, uptime, and thermal sensor readings when exposed.
- Sort/filter/search without altering the underlying complete snapshot.
- Provide JSON output for auditing/tests and a readable terminal UI.
- Make polling interval and history retention explicit.
- Operate read-only and useful without root.

Out of scope for MVP:

- GUI, Windows, macOS, remote monitoring, mobile clients, cloud sync, accounts, telemetry.
- Killing/suspending/renicing processes.
- Network packet inspection or per-process bandwidth attribution.
- Malware classification, signatures, “safe/unsafe” badges.
- eBPF as a required dependency.
- Long-term database or forensic evidence guarantees.
- Perfect attribution of temperature or power to an individual process.

## Scope-lock protocol

MVP scope locks when research gates R1–R6 in `RESEARCH_PLAN.md` are resolved and an accepted ADR names the baseline. After lock, new capabilities go to a dated backlog record unless required for an acceptance criterion, correctness, safety, or supported Arch operation.

Every proposed scope change must state:

- user outcome;
- new complexity and privilege needs;
- overhead/privacy impact;
- test strategy;
- what moves out, if anything;
- target milestone.

## Future expansion sequence

1. Linux portability beyond Arch.
2. Optional desktop GUI consuming the same snapshot schema.
3. Windows collector using native process/system APIs.
4. macOS collector using supported native APIs.
5. Optional advanced event collectors, only after the polling core is trustworthy.

