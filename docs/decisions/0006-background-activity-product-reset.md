# ADR-0006: Background-activity inspector product reset

Status: Accepted  
Date: 2026-07-10  
Supersedes: heat/slow explanation language and the original M0–M3 completion interpretation

## Decision

The product exists to show kernel-visible background activity, changes over time, the observer itself, and known visibility gaps. It does not explain heat, diagnose malware, score safety, or claim causal intent.

“Background activity” includes processes, threads, kernel threads, systemd units, cgroups, sockets/listeners, timers, mounts, lifecycle events, and observation failures. Linux events are primary for lifecycle; snapshots reconcile current state.

Default operation is unprivileged and privacy-preserving. A richer collector may be explicitly elevated, but the project will never install setuid or silently invoke `sudo`. Persistent recording is explicit, bounded, private, and off by default.

## Milestone consequence

The original M0–M3 closure records remain historical. Their status is superseded pending the acceptance gates in `docs/MILESTONES_V2.md`.

