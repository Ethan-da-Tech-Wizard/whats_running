# Glossary

- **Process:** An OS execution/resource container identified at a moment by PID; PID can be reused.
- **Thread/task:** A schedulable execution unit; Linux procfs sometimes uses “task” where users expect thread.
- **PID namespace:** A visibility and numbering boundary for processes.
- **Snapshot:** Best-effort collection over a time window, not an atomic picture.
- **Sample:** A metric observation at a recorded time.
- **Rate:** Change in a counter divided by elapsed time.
- **RSS:** Resident set size; memory pages currently resident, with accounting caveats.
- **Virtual memory:** Address space mappings, not equivalent to physical RAM consumed.
- **I/O counter:** Kernel-accounted read/write quantity whose semantics must be stated.
- **Observer effect:** Resource use and timing distortion caused by the monitoring tool itself.
- **Completeness boundary:** The namespace, permissions, interfaces, and sampling interval defining what can be observed.
- **Typed unavailability:** A reason such as denied, vanished, unsupported, or parse error instead of a fabricated value.
- **Scope lock:** Accepted feature boundary for a milestone; later changes require a recorded proposal.
- **ADR:** Architecture Decision Record.

