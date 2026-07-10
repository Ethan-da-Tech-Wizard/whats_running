# Background-activity reset implementation progress

Status: Append-only progress record  
Date: 2026-07-10

Implemented so far:

- accepted product reset, v2 milestones, and coverage matrix;
- command lines are no longer opened or retained unless explicitly enabled;
- `/proc/stat` totals exclude guest counters already included in user/nice;
- typed availability covers numeric and string process fields through schema v3;
- thread counts, cgroup paths, and inferred systemd unit/scope identity;
- isolated Linux proc-connector lifecycle collector with parser tests and explicit capability failure;
- inventory of sockets, cgroups, mounts, associated units, and user systemd timers with typed source status.

Fourteen tests and warnings-as-errors Clippy pass. The restricted reference namespace reported 4 processes, 4 threads, 1 unit, 81 cgroups, 45 mounts, no visible sockets, and unavailable user timer-manager access. Proc-connector subscription was denied with `EPERM`, proving the unprivileged boundary rather than event success.

M1–M3 v2 remain in progress/pending; this record does not close them.
