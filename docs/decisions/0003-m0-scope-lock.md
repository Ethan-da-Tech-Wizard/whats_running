# ADR-0003: Lock the Arch snapshot MVP and close M0

Status: Accepted  
Date: 2026-07-10

## Context

The initial corpus established a broad product direction. Implementation needs a small frozen first slice.

## Decision

M1 is one unprivileged Rust executable that enumerates visible procfs PIDs, identifies itself, exposes core identity/resource counters, represents volatile field failures, and emits a human table or privacy-conscious JSON. It has no daemon, network, database, process-control actions, GUI, or third-party dependency.

## Consequences

This completes the planning milestone sufficiently to build and verify the first slice. User interviews and competitor research remain valuable product research, but do not block a read-only factual collector. Aggregate system context moves into M2 alongside rates.

## Verification

See `docs/VERIFICATION.md` and the append-only M1 closure journal entry.

