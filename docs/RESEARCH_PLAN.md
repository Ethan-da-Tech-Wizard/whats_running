# Research and brainstorming plan

Status: Proposed

## Gates before scope lock

- **R1 — User language:** Observe target users and identify their first five questions.
- **R2 — Linux truth model:** Map every desired field to `/proc` or `/sys`, kernel documentation, units, permission behavior, and race conditions.
- **R3 — Competitor audit:** Compare `ps`, `top`, `htop`, `btop`, `atop`, `systemd-cgtop`, GNOME System Monitor, and KDE System Monitor without cloning feature bloat.
- **R4 — Prototype benchmark:** Measure direct procfs parsing at 100, 500, 2,000, and stress-scale PIDs/threads.
- **R5 — Privacy review:** Decide command-line display/export defaults and redaction semantics.
- **R6 — Schema spike:** Produce and test a versioned snapshot JSON schema.

## Linux questions

- What exactly is visible across PID/user/cgroup namespaces and `hidepid` mount modes?
- Which process fields are stable enough to support, and how do kernel threads differ?
- How accurately can CPU percentages be normalized across cores and sampling jitter?
- Which I/O counters exclude cached activity or require permission?
- How should cgroup v2 and systemd unit ownership be explained?
- Which Arch packaging database queries are safe and affordable for provenance?
- What thermal/hwmon sensors are commonly available, and why process-to-temperature attribution is not causal proof?
- How do suspend/resume and clock changes affect rates?

## Product experiments

- Table-first versus tree-first default.
- “Hot now” versus sustained/trending ranking.
- A visible “observer overhead” row/card.
- Plain-language explanation panel backed by exact fields.
- Snapshot diff after a fan/heat event.
- Progressive disclosure for expert fields.

## Future-platform research, intentionally deferred

- Windows: Toolhelp/NT APIs, ETW, performance counters, protected processes, services/jobs.
- macOS: `libproc`, Mach APIs, Endpoint Security entitlements, sandbox/privacy restrictions.
- Define conceptual parity and capability reporting; do not promise identical metrics.

## Research record format

Each experiment gets a new dated file in `journal/` containing question, hypothesis, method, environment, raw evidence location, result, limitations, and decision impact. Results are appended/superseded, not rewritten.

