# JSON schema contract

Status: Superseded by schema version 3  
Date: 2026-07-10

The JSON output is one object with `schema_version: 2` and a `samples` array. A one-shot run has one element; `--samples N` has N elements, with N limited to 120.

Each sample contains collection metadata, the observation-boundary statement, system memory/swap/CPU-count context, observed start/exit counts, and the complete enumerated process array. Every sample marks exactly one process as `is_observer` during normal operation.

Process identities are `(pid, start_ticks)`. CPU and I/O rates are `null` on the first sample or when inputs are unavailable. `rate_status` currently reads `value` or `warming_up_or_unavailable`. Command lines are represented as `not_collected` unless explicitly requested.

Compatibility rule: consumers must reject unknown major schema versions and ignore unknown object members within a known version. A semantic rename, unit change, or structural break increments `schema_version`; additive fields do not necessarily require one.

The project currently emits JSON manually to preserve the zero-dependency design. Escaping and whole-document parsing are tested. A machine-readable JSON Schema file may be added when it provides more value than maintenance cost.

## Schema version 3 — 2026-07-10

Schema 3 replaces numeric `null` fields with `{status, value?}` objects, preserving the same typed availability vocabulary used by strings. It also adds thread count, cgroup path, and inferred systemd unit/scope identity. Schema 2 remains documented above for history but is no longer emitted.
