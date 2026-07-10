# Product requirements document

Status: Proposed

## Product outcome

A local Arch Linux user launches one tool and receives a complete, explainable view of all processes visible within the tool's observation boundary, together with system-pressure context and honest gaps.

## Personas

- **Concerned owner:** wants “what is making it hot?” without learning kernel vocabulary first.
- **Curious Arch user:** wants exact fields, provenance, process tree, and inspectable raw data.
- **Troubleshooter:** wants reproducible snapshots and bounded history.

## Core experiences

1. **Now:** complete snapshot, obvious top consumers, total system context.
2. **Why:** drill into ancestry, executable, command, owner, age, resource trend, package/cgroup context.
3. **What changed:** compare recent samples and expose process starts/exits detected between polls.
4. **Can I trust it?:** show observer PID, collection timestamp/duration, errors, privilege boundary, and overhead.

## Product requirements

- P1: First useful information appears within two seconds on a typical desktop.
- P2: Default view highlights sustained resource use, not only instantaneous spikes.
- P3: Complete-list mode must not silently filter system processes or the observer.
- P4: Friendly names never replace access to exact raw identifiers and paths.
- P5: Unknown does not become zero; unavailable does not become empty.
- P6: Sensitive command-line/environment data is not exported accidentally.
- P7: A snapshot can be exported locally in a versioned, documented schema.
- P8: The UI remains navigable with thousands of processes/threads.
- P9: Documentation explains every field and known limitation.
- P10: No network communication occurs in the MVP.

## Measures

- Enumeration coverage against an independent `/proc` fixture and controlled spawned processes.
- Detection rate for controlled short-lived processes at documented polling intervals.
- CPU and memory overhead across idle, normal, and stress scenarios.
- Time for test users to answer “what is using CPU?” and “where did this process come from?”
- Count of unexplained blanks: target zero; unavailable fields must have typed reasons.

