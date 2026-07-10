# Functional requirements document

Status: Proposed

## Collection

- FR-001: Enumerate numeric entries in the active `/proc` mount on every snapshot.
- FR-002: Read each process defensively; processes may exit or change between reads.
- FR-003: Record per-field states: value, permission denied, vanished, unsupported, parse error, or not collected.
- FR-004: Locate and positively identify the observer's own process in each successful snapshot.
- FR-005: Use deltas between timestamped samples for rates; never label lifetime counters as rates.
- FR-006: Capture collection start/end times and duration.
- FR-007: Preserve raw kernel identifiers needed to audit derived values.

## Presentation

- FR-010: Offer an unfiltered process list as a first-class mode.
- FR-011: Sort by CPU, resident memory, I/O rate, age, PID, name, and observation status.
- FR-012: Filter/search is visibly active and instantly removable.
- FR-013: Show a process tree with explicit orphan/cycle/race handling.
- FR-014: Show exact executable and command line when readable, subject to privacy mode.
- FR-015: Explain units, sampling windows, and unavailable values in-product.
- FR-016: Surface aggregate system activity that cannot be fully attributed to visible processes.

## History and export

- FR-020: Maintain a bounded in-memory ring buffer with configurable duration.
- FR-021: Detect observed starts/exits between snapshots without claiming unseen events were captured.
- FR-022: Export a single snapshot or bounded history to versioned JSON.
- FR-023: Redacted export is default; full command lines require an explicit option and warning.

## CLI/TUI

- FR-030: Non-interactive snapshot command supports human table and JSON.
- FR-031: Interactive terminal mode updates without losing selection unnecessarily.
- FR-032: `--help` documents privilege, privacy, overhead, and completeness boundaries.
- FR-033: Terminal resizing and non-UTF-8/fallback rendering fail gracefully.

## Acceptance examples

- Start 100 named test processes; every still-alive process visible in the same PID namespace appears, plus the observer.
- Repeatedly create millisecond-lived processes; UI reports that polling can miss them and measured capture depends on interval.
- Make selected `/proc/PID` fields unreadable in a fixture; output distinguishes denial from zero.
- Run under a non-root account; no crash or demand for elevation.
- Export in privacy mode; secrets placed in synthetic command lines are redacted or omitted as documented.

