# ADR-0007: Typed schema v3 and proc-connector lifecycle

Status: Accepted  
Date: 2026-07-10

Use typed availability objects for process/thread fields in schema v3. Use the Linux proc connector for lifecycle chronology, track sequence continuity per CPU, and reconcile with snapshots before/after capture. Command collection is opt-in at the filesystem-read boundary.

See `docs/verification/M1-V2-2026-07-10.md`.

