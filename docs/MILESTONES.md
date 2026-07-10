# Milestones

Status: Proposed

## M0 — Truth before code — Complete 2026-07-10

Exit: problem interviews planned, field/source matrix drafted, completeness language accepted, Rust spike decision recorded, scope v1 locked.

## M1 — Honest snapshot CLI — Complete 2026-07-10

Delivered: one-shot process snapshot; self appears; typed errors for volatile string fields; human table and versioned JSON; parser/security unit tests; no application network code or dependencies; Arch build/install/run docs. Aggregate system metrics originally mentioned in scope are deferred to M2 because they become materially useful alongside deltas. This explicit deferral supersedes the shorthand M1 exit wording; it does not silently claim delivery.

## M2 — Rates and bounded history — Complete 2026-07-10

Delivered: CPU and storage-I/O deltas, `(PID, start_ticks)` reuse protection, observed starts/exits, aggregate memory/swap context, a hard 120-sample bound, valid schema-v2 history JSON, a 2,000-process synthetic collector test, and reference-system measurements. See `docs/verification/M2-2026-07-10.md`.

## M3 — Usable terminal interface — Complete 2026-07-10

Delivered: complete-list default, CPU/memory/PID/name sorting, visible all/current-user/problem filters, ancestry indentation, selection/details, dynamic sizing, bounded visible rows, self marker, and terminal restoration on ordinary exit. See `docs/verification/M3-2026-07-10.md`.

## M4 — Heat/slow context

Exit: system pressure and optional thermal/hwmon readings, cgroup/systemd context, carefully worded explanations without false causality.

## M5 — Arch-ready release

Exit: PKGBUILD, reproducible build notes, security/privacy review, threat tests, full field reference, troubleshooting, signed release/checksums where feasible.

## M6 — Linux portability

Exit: tested on selected non-Arch distributions; platform assumptions isolated; compatibility matrix published.

## Later, separately scoped

GUI, Windows collector, and macOS collector each require their own research, requirements delta, threat review, and milestone plan.
