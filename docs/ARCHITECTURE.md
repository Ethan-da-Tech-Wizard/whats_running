# Architecture

Status: Proposed

## KISS architecture

```text
Linux /proc + /sys
       |
       v
 [Linux collector] --> [normalized snapshot] --> [delta/history engine]
                                                   |          |
                                                   v          v
                                              [CLI/JSON]   [TUI]
```

One executable, one process by default, no daemon, no database, no plugin system, no network service.

## Components

### Platform collector

Enumerates processes and system metrics. Linux implementation reads procfs/sysfs using narrow, testable parsers. A future Windows/macOS collector produces the same conceptual schema, with capability flags rather than fake equivalence.

### Normalized model

Contains a `Snapshot`, `SystemSample`, and many `ProcessSample` records. Every optional measurement carries a value or a typed unavailability reason. Process identity combines PID and start time.

### Delta/history engine

Computes rates and trends from counters, detects observed birth/death transitions, and retains a bounded ring buffer. It must cope with missing samples and PID reuse.

### Views

The snapshot CLI is the correctness/debugging surface. JSON is the audit and integration surface. The TUI is a consumer of the same model, not a second collector.

## Key design rules

- Separate collection from rendering.
- Preserve raw values alongside derived values where useful.
- Do not use a generic cross-platform process library until its omissions and semantics are audited.
- Avoid async/concurrency until benchmarks show collection needs it; parallel `/proc` reads can increase perturbation.
- Package provenance and temperature are optional enrichments, never allowed to block the core snapshot.
- The observer effect is measurable: include collection duration and the tool's own resource use.

## Candidate repository layout

```text
src/
  main.rs
  model/
  platform/linux/
  history/
  output/
  tui/
tests/fixtures/proc/
docs/
```

## Open architecture decisions

- Direct procfs parsing versus selectively using `procfs`/`sysinfo` crates.
- Whether “processes” and “threads” share one table or distinct modes.
- Thermal integration through `/sys/class/thermal`, hwmon, or an optional library.
- Snapshot schema and compatibility policy.
- Packaging: Arch PKGBUILD first; reproducible binary release later.

