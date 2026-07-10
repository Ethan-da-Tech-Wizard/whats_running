# ADR-0002: Rust, terminal-first, single-process architecture

Status: Accepted  
Date: 2026-07-10

## Context

The tool must observe thousands of volatile OS records with low overhead, expose itself, distribute easily on Arch, and leave room for native Windows/macOS collectors.

## Proposed decision

Use Rust for one executable containing collector, model/history engine, CLI/JSON output, and TUI. Do not add a daemon, database, network layer, GUI framework, or privilege helper to MVP.

## Alternatives

Go, Python, C/C++, Zig, and Electron/TypeScript are discussed in `docs/LANGUAGE_DECISION.md`.

## Consequences

Rust raises the learning curve but supports a compact, low-overhead, memory-safe binary. Direct procfs work creates correctness responsibility and requires comprehensive fixtures.

## Verification before acceptance

Prototype enumeration and parsing; benchmark at multiple process counts; validate terminal stack; audit proposed dependencies; document cross-compilation/native API constraints.

## Acceptance — 2026-07-10

The M1 implementation validates direct procfs enumeration, typed failures, table/JSON output, self-identification, tests, and a zero third-party dependency build. Interactive terminal dependencies remain deferred until M3 and require a separate decision.
