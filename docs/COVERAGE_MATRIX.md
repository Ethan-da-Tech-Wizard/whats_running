# Observation coverage matrix

Status: Accepted baseline; append verified results as collectors land

| Domain | Unprivileged source | Optional elevated source | Event coverage | Known gaps |
|---|---|---|---|---|
| Processes | `/proc` | same with fewer denied fields | proc connector | namespaces, short lives, event loss |
| Threads | `/proc/PID/task` | same | fork/exit where exposed | races and namespace limits |
| systemd units | systemd manager query/cgroups | system manager query | manager change signals later | absent/non-systemd sessions |
| cgroups | cgroup v2 filesystem | same with fewer denied files | reconciliation initially | delegated/hidden hierarchy |
| TCP/UDP | `/proc/net`, inode ownership mapping | broader PID field access | reconciliation initially | network namespaces, races |
| Unix sockets | `/proc/net/unix` | broader PID field access | reconciliation initially | unnamed sockets, namespaces |
| Timers | systemd timer inventory | system manager inventory | reconciliation initially | application-internal/kernel timers |
| Mounts | `/proc/self/mountinfo` | namespace-specific inspection | reconciliation initially | other mount namespaces |
| Kernel work | kernel threads and aggregate counters | tracing is separately scoped | partial | IRQ/firmware/device work is not a process |

Coverage is always relative to the observer’s namespaces, permissions, enabled collectors, event-loss state, and sampling window.

