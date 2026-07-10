# ADR-0008: Background domains and multi-view TUI

Status: Accepted  
Date: 2026-07-10

Inventory systemd, cgroup v2, procfs sockets, mounts, namespaces, and timers as typed sources. Attribute sockets only through visible inode/file-descriptor evidence. Use explicit bounded private event recording. Present processes, services/cgroups, network, timers/mounts, and events as separate TUI views with exact exports.

See `docs/verification/M2-V2-2026-07-10.md` and `M3-V2-2026-07-10.md`.

