# TUI legend visibility, documentation alignment, and project status

Date: 2026-07-10  
Status: Historical record

## What changed this session

The `--tui` keybinding legend was previously a single dense bracketed line sharing space with sort/filter/tree/search state (`[q quit j/k move s sort f filter t tree d detail v view r refresh / search]`), easy to overlook. It now renders as its own bounded block—separator bar, one legend line in `v`/`/`/`t`/`s`/`f`/`r`/`j`/`k`/`d`/`q` order, separator bar—directly above the process list on every view, unconditionally, with no color or mouse dependency. `draw()`'s row-budget reservation moved from 4/8 lines (non-detail/detail) to 6/10 to account for the two added separator lines without truncating the process list.

Documentation was aligned around the same key order and wording:

- `README.md` gained a "TUI controls" section listing all nine keys.
- `docs/OPERATIONS.md`'s prose key summary became an explicit table.
- `docs/TUI_DESIGN.md` (Accepted for M3) received an appended `## Update` section rather than a silent rewrite, per `DOCUMENTATION_POLICY.md`.

## Verification

- `cargo test --release`: 16/16 passed.
- `cargo clippy --release -- -D warnings`: clean.
- `cargo fmt --check`: clean.
- Live verification in a real pseudo-terminal (Python `pty.fork`, 90x30): the separator/legend/separator block rendered correctly above the process table, and a `SIGTERM` sent mid-run still produced the expected cursor-show plus alternate-screen-exit sequence before the process ended, confirming the row-budget change did not disturb signal-safe restoration.

## Where the project stands

M0–M3 (`docs/MILESTONES_V2.md`) are all Accepted/Complete with immutable verification records under `docs/verification/`. No mandatory gate is open. This session's work is UI-clarity and documentation-consistency polish on top of an already-closed baseline, not a new milestone.

## What comes next (Proposed, not accepted)

No M4 is defined in `docs/PRODUCT_REQUIREMENTS_V2.md` or `docs/MILESTONES_V2.md`. The following are candidate directions only, offered for the project owner to accept, reject, or reorder before any of them is treated as scope:

1. **CI automation.** `docs/DOCUMENTATION_POLICY.md` already flags "Add CI later" as an enforcement step; `fmt`/`clippy`/`test` currently only run locally and by hand.
2. **Packaging/distribution.** A PKGBUILD or tagged release binary would remove the `cargo build --release` + manual `install` step documented in `docs/OPERATIONS.md`.
3. **Portability beyond the Arch/systemd/cgroup-v2 profile.** `docs/ARCH_HYPRLAND_PROFILE.md` is the only validated target; non-systemd inits, cgroup v1, and other distros are unexplored.
4. **Deeper container/namespace visibility.** `docs/COVERAGE_MATRIX.md` and the M2/M3 verification records already name unattributed sockets and namespace isolation as explicit, accepted blind spots rather than defects—closing them further would be new scope, not a bug fix.
5. **A dedicated in-TUI help overlay** (e.g. a `?` key) if the always-visible legend bar proves insufficient on very narrow terminals in practice.

None of these are started. Picking one turns it into a new milestone with its own exit gates, per the M0–M3 acceptance rule.
