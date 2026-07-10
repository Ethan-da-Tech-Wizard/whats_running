# Scale and limits

Status: Proposed

## How many things can be running?

There is no single practical number. “Things” may mean processes, threads/tasks, services, kernel threads, containers, sockets, files, timers, or hardware/firmware work. The MVP primarily observes processes and optionally threads, plus aggregate system metrics.

Linux assigns numeric task IDs up to the system's configured PID maximum. A machine can therefore have a theoretical PID namespace range reaching into the millions, but memory, per-user limits, cgroups, kernel resources, workload design, and PID reuse make the sustainable number machine-specific. The configured ceiling is researchable locally through `/proc/sys/kernel/pid_max`; it is not the count currently running.

On an ordinary desktop, expect roughly hundreds of processes and potentially many more threads. Development workloads, browsers, containers, or servers may produce thousands. The product should therefore:

- design for thousands without assuming millions are cheap;
- render only visible rows;
- bound history by bytes as well as time;
- benchmark 100, 500, 2,000, 10,000, and an environment-specific stress ceiling;
- expose process count, thread count, collection time, skipped/failed reads, and sample overruns;
- degrade by reducing retained detail/history, never by silently hiding live entries in complete-list mode.

## Complexity model

Each snapshot is at least O(P × F), where P is observed processes/tasks and F is files/fields read. History is approximately O(P × S) records for P items across S samples unless compressed or stored as deltas. At 10,000 items sampled each second, naïvely retaining every string and field for minutes can consume enormous memory and generate its own I/O/CPU problem.

## Required scale tests

- Many stable processes.
- Many threads inside few processes.
- Rapid fork/exit churn.
- Huge or hostile command lines.
- PID reuse.
- Permission-denied fields.
- Slow or unusual procfs mounts/containers.
- TUI sorting/filtering while collection continues.

Concrete support limits must come from benchmarks, not bravado.

