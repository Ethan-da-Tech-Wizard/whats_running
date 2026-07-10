# M2 progress on the Arch + Hyprland reference system

Date: 2026-07-10  
Status: Historical record

## Delivered increment

The collector now reads aggregate CPU ticks, logical CPU count, memory/swap totals, and per-process storage byte counters. Finite `--samples N --interval-ms N` collection retains at most 120 snapshots, calculates per-process CPU percentages using stable `(PID, start_ticks)` identity, and reports observed starts/exits.

## Reference environment

The host identifies as Arch Linux, Hyprland, and Wayland with 16 logical CPUs and approximately 15.2 GiB RAM. Hyprland does not alter procfs semantics, so compositor IPC remains outside the KISS collector.

## Verification

Six tests pass and clippy passes with warnings denied. A two-sample 50 ms run kept the observer visible, showed aggregate memory/swap, warmed CPU rates from unknown to measured, and retained zero third-party dependencies.

## Honest remaining M2 work

I/O rates are not yet derived, history cannot yet be exported as one schema-defined JSON document, numeric unavailable reasons remain untyped, and scale budgets are not benchmarked. Therefore M2 remains in progress.

