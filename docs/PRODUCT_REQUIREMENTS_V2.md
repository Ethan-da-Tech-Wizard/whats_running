# Accepted product and system baseline v2

Status: Accepted  
Date: 2026-07-10  
Supersedes: the Proposed v1 charter/PRD/FRD/SRD where they conflict

## Product promise

Show all background activity visible through enabled Arch Linux interfaces, include the observer/collector, preserve changes, and state every known visibility boundary. Never claim omniscience, safety, maliciousness, intent, heat causality, or completeness outside the named boundary.

## Required domains

Processes, individual threads, kernel threads, ancestry, systemd units/scopes, cgroup v2, TCP/UDP/Unix sockets, timers, mounts, lifecycle events, field/source errors, collector loss, reconciliation state, and observer overhead.

## Operating modes

- Default unprivileged, read-only, local, network-independent, command-private, memory-only.
- Explicit optional elevation for richer kernel/system visibility; no setuid or silent `sudo`.
- Explicit optional bounded owner-private event recording; off by default and command-free unless separately authorized.

## Architecture

Kernel events are the lifecycle chronology. Periodic snapshots provide current state and reconcile missed/denied events. All sources normalize into typed `value`, `not_collected`, `permission_denied`, `vanished`, `unsupported`, `parse_error`, or `io_error` states. TUI, tables, inventory, and exports consume the same model.

## Safety and correctness

Treat kernel strings as hostile terminal input; pair PID/TID with start time; do not double-count guest CPU; use monotonic kernel time for event ordering; identify event loss; bound memory/disk/output; use restrictive recording permissions; collect no environment variables; read command lines only after explicit opt-in.

## Acceptance rule

A milestone closes only when every exit gate is directly tested or explicitly removed through a superseding accepted decision. Inference, deferral, and a documented bug do not satisfy a gate.

