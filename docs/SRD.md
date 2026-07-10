# System requirements document

Status: Proposed

## Correctness

- SR-001: Use stable process identity `(pid, start_time)`; PID alone is insufficient due to reuse.
- SR-002: Derived CPU rates account for elapsed monotonic time and system clock ticks.
- SR-003: Parsers handle spaces, parentheses, NUL separators, large counters, unknown states, and kernel-version variation.
- SR-004: Snapshot consistency is best-effort and explicitly timestamped; `/proc` is not transactional.
- SR-005: Arithmetic is overflow-safe for long uptime and fast counters.

## Performance budgets (initial hypotheses to benchmark)

- SR-010: Default sampling interval: 1 second, configurable no lower than a guarded limit.
- SR-011: Observer median CPU below 1% of one core on the reference desktop at 500 processes.
- SR-012: Observer resident memory below 100 MiB for default history at 2,000 process records per sample, subject to prototype measurement.
- SR-013: Collection completes within 250 ms at 2,000 processes on reference hardware, or reports overrun.
- SR-014: Rendering cost is decoupled from collection and bounded to visible rows.

## Reliability and compatibility

- SR-020: One unreadable/malformed process cannot abort a snapshot.
- SR-021: Unsupported optional sensors degrade to explicit unavailable states.
- SR-022: Terminal restoration occurs after normal exit, error, and common signals.
- SR-023: Initial supported target is current 64-bit Arch Linux with procfs mounted; exact minimum kernel/toolchain is set after research.
- SR-024: Architecture keeps OS collection behind an interface, but cross-platform code is not required in MVP.

## Security/privacy

- SR-030: No outbound network calls in production MVP code.
- SR-031: No setuid binary and no automatic privilege escalation.
- SR-032: Treat `/proc` strings as untrusted bytes; sanitize control sequences before terminal rendering.
- SR-033: Export files use restrictive permissions by default.
- SR-034: Never collect process environments by default.

## Testability

- SR-040: Parsers accept fixture inputs without a live `/proc` dependency.
- SR-041: Golden fixtures cover kernel threads, zombies, vanished processes, permission errors, odd names, PID reuse, and counter rollover boundaries.
- SR-042: Integration tests create controlled CPU, memory, and I/O workloads.
- SR-043: Benchmarks publish hardware, kernel, process count, interval, and build mode.

