# Files 1 of 1 — What’s Running?

[All pages](../FILE_GUIDE.md) · [Project guide](../PROJECT_GUIDE.md)

| File | Role and useful details |
| --- | --- |
| [.github/workflows/ci.yml](../../.github/workflows/ci.yml) | GitHub Actions workflow for ci; defines automation triggers, jobs, and build/check steps. |
| [.gitignore](../../.gitignore) | Defines local data, credentials, dependencies, and generated outputs that Git should leave untracked. |
| [Cargo.lock](../../Cargo.lock) | Dependency lockfile recording resolved package versions and integrity/source metadata; update through the project’s package manager. |
| [Cargo.toml](../../Cargo.toml) | Configuration or build metadata for Cargo; read alongside the application or tool that loads it. |
| [LICENSE-APACHE](../../LICENSE-APACHE) | Preserved license, copyright, or attribution terms; read the full text before reusing the covered component. |
| [LICENSE-MIT](../../LICENSE-MIT) | Preserved license, copyright, or attribution terms; read the full text before reusing the covered component. |
| [PROJECT_NOTES.md](../../PROJECT_NOTES.md) | Preserved detailed setup, usage, architecture, and development notes from the earlier README. |
| [README.md](../../README.md) | Project overview, current status, entry points, setup navigation, and documentation links. |
| [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) | Documentation: Architecture. Covers KISS architecture; Components; Platform collector. |
| [docs/ARCH_HYPRLAND_PROFILE.md](../../docs/ARCH_HYPRLAND_PROFILE.md) | Documentation: Arch Linux + Hyprland reference profile. Covers Design consequences; Suggested Hyprland binding after local installation. |
| [docs/COVERAGE_MATRIX.md](../../docs/COVERAGE_MATRIX.md) | Documentation: Observation coverage matrix. |
| [docs/DEPENDENCIES.csv](../../docs/DEPENDENCIES.csv) | Machine-readable inventory of declared and locked dependencies, versions, reported licenses, and evidence URLs. |
| [docs/DOCUMENTATION_POLICY.md](../../docs/DOCUMENTATION_POLICY.md) | Documentation: Documentation policy. Covers The append-only promise; Document classes; How to change an accepted idea. |
| [docs/FIELD_REFERENCE.md](../../docs/FIELD_REFERENCE.md) | Documentation: Linux field reference. Covers Typed field statuses. |
| [docs/FILE_GUIDE.md](../../docs/FILE_GUIDE.md) | Index of the complete tracked-file guide, with one entry for every file and links to the paginated descriptions. |
| [docs/FRD.md](../../docs/FRD.md) | Documentation: Functional requirements document. Covers Collection; Presentation; History and export. |
| [docs/GLOSSARY.md](../../docs/GLOSSARY.md) | Documentation: Glossary. |
| [docs/JSON_SCHEMA.md](../../docs/JSON_SCHEMA.md) | Documentation: JSON schema contract. Covers Schema version 3 — 2026-07-10. |
| [docs/LANGUAGE_DECISION.md](../../docs/LANGUAGE_DECISION.md) | Documentation: Language and tooling recommendation. Covers Recommendation: Rust, with a deliberately boring architecture; Alternatives; Proposed stack, pending spikes. |
| [docs/LICENSE_REVIEW.md](../../docs/LICENSE_REVIEW.md) | Original-code license, preserved component notices, dependency-license evidence, and unresolved distribution questions. |
| [docs/MILESTONES.md](../../docs/MILESTONES.md) | Documentation: Milestones. Covers M0 — Truth before code — Complete 2026-07-10; M1 — Honest snapshot CLI — Complete 2026-07-10; M2 — Rates and bounded history — Complete 2026-07-10. |
| [docs/MILESTONES_V2.md](../../docs/MILESTONES_V2.md) | Documentation: Milestones v2 — background activity. Covers M0 — Accepted baseline — Complete; M1 — Processes, threads, and lifecycle — Complete 2026-07-10; M2 — Background domains and bounded recording — Complete 2026-07-10. |
| [docs/OPERATIONS.md](../../docs/OPERATIONS.md) | Documentation: Operations guide. Covers Requirements; Commands; Quick non-interactive check. |
| [docs/PRD.md](../../docs/PRD.md) | Documentation: Product requirements document. Covers Product outcome; Personas; Core experiences. |
| [docs/PROBLEM_STATEMENT.md](../../docs/PROBLEM_STATEMENT.md) | Documentation: Problem statement. Covers Human problem; Product problem; Problem statement template to validate. |
| [docs/PRODUCT_REQUIREMENTS_V2.md](../../docs/PRODUCT_REQUIREMENTS_V2.md) | Documentation: Accepted product and system baseline v2. Covers Product promise; Required domains; Operating modes. |
| [docs/PROJECT_CHARTER.md](../../docs/PROJECT_CHARTER.md) | Documentation: Project charter. Covers Vision; User promise; Important honesty boundary. |
| [docs/PROJECT_GUIDE.md](../../docs/PROJECT_GUIDE.md) | Maintainer orientation: purpose, important entry points, source layout, commands, and project-specific boundaries. |
| [docs/README.md](../../docs/README.md) | Documentation: Documentation map. Covers Status vocabulary. |
| [docs/RESEARCH_PLAN.md](../../docs/RESEARCH_PLAN.md) | Documentation: Research and brainstorming plan. Covers Gates before scope lock; Linux questions; Product experiments. |
| [docs/SAFETY_AND_PRIVACY.md](../../docs/SAFETY_AND_PRIVACY.md) | Documentation: Safety, privacy, and trust. Covers Threat model; Required controls; Completeness statement. |
| [docs/SCALE_AND_LIMITS.md](../../docs/SCALE_AND_LIMITS.md) | Documentation: Scale and limits. Covers How many things can be running?; Complexity model; Required scale tests. |
| [docs/SCOPE.md](../../docs/SCOPE.md) | Documentation: Scope and scope lock. Covers MVP: Arch Linux, terminal-first; Scope-lock protocol; Future expansion sequence. |
| [docs/SRD.md](../../docs/SRD.md) | Documentation: System requirements document. Covers Correctness; Performance budgets (initial hypotheses to benchmark); Reliability and compatibility. |
| [docs/TUI_DESIGN.md](../../docs/TUI_DESIGN.md) | Documentation: Terminal interface design. Covers Update — 2026-07-10: bounded legend bar. |
| [docs/VERIFICATION.md](../../docs/VERIFICATION.md) | Documentation: Verification record. Covers Commands executed; Results; M1 gaps retained honestly. |
| [docs/decisions/0001-documentation-is-append-only.md](../../docs/decisions/0001-documentation-is-append-only.md) | Documentation: ADR-0001: Documentation is append-only historical evidence. Covers Context; Decision; Consequences. |
| [docs/decisions/0002-proposed-rust-terminal-first.md](../../docs/decisions/0002-proposed-rust-terminal-first.md) | Documentation: ADR-0002: Rust, terminal-first, single-process architecture. Covers Context; Proposed decision; Alternatives. |
| [docs/decisions/0003-m0-scope-lock.md](../../docs/decisions/0003-m0-scope-lock.md) | Documentation: ADR-0003: Lock the Arch snapshot MVP and close M0. Covers Context; Decision; Consequences. |
| [docs/decisions/0004-m2-history-schema-and-bounds.md](../../docs/decisions/0004-m2-history-schema-and-bounds.md) | Documentation: ADR-0004: Finite schema-v2 history with a 120-sample hard bound. Covers Context; Decision; Consequences. |
| [docs/decisions/0005-dependency-free-arch-tui.md](../../docs/decisions/0005-dependency-free-arch-tui.md) | Documentation: ADR-0005: Dependency-free Arch terminal interface. |
| [docs/decisions/0006-background-activity-product-reset.md](../../docs/decisions/0006-background-activity-product-reset.md) | Documentation: ADR-0006: Background-activity inspector product reset. Covers Decision; Milestone consequence. |
| [docs/decisions/0007-schema-v3-and-m1-lifecycle.md](../../docs/decisions/0007-schema-v3-and-m1-lifecycle.md) | Documentation: ADR-0007: Typed schema v3 and proc-connector lifecycle. |
| [docs/decisions/0008-background-domains-and-multiview-tui.md](../../docs/decisions/0008-background-domains-and-multiview-tui.md) | Documentation: ADR-0008: Background domains and multi-view TUI. |
| [docs/decisions/0009-arch-only-scope-lock.md](../../docs/decisions/0009-arch-only-scope-lock.md) | Documentation: ADR-0009: Lock target platform to Arch Linux; reject portability as scope. Covers Context; Decision; Alternatives considered. |
| [docs/decisions/README.md](../../docs/decisions/README.md) | Documentation: Architecture decision records. Covers Index. |
| [docs/file-guide/files-001.md](../../docs/file-guide/files-001.md) | One page of the tracked-file catalog; links each listed path to its role and source-derived details. |
| [docs/journal/2026-07-10-1300-initial-brainstorm.md](../../docs/journal/2026-07-10-1300-initial-brainstorm.md) | Documentation: Initial brainstorm. Covers Raw intent distilled; Early product personality; Brainstorm inventory (not commitments). |
| [docs/journal/2026-07-10-1400-m0-m1-closure.md](../../docs/journal/2026-07-10-1400-m0-m1-closure.md) | Documentation: M0 and M1 closure. Covers What changed; KISS decisions; Observations. |
| [docs/journal/2026-07-10-1500-m2-arch-hyprland-progress.md](../../docs/journal/2026-07-10-1500-m2-arch-hyprland-progress.md) | Documentation: M2 progress on the Arch + Hyprland reference system. Covers Delivered increment; Reference environment; Verification. |
| [docs/journal/2026-07-10-1600-m2-closure.md](../../docs/journal/2026-07-10-1600-m2-closure.md) | Documentation: M2 closure. |
| [docs/journal/2026-07-10-1700-m3-closure.md](../../docs/journal/2026-07-10-1700-m3-closure.md) | Documentation: M3 closure. |
| [docs/journal/2026-07-10-1800-m1-v2-closure.md](../../docs/journal/2026-07-10-1800-m1-v2-closure.md) | Documentation: M1 v2 closure. |
| [docs/journal/2026-07-10-1900-m2-m3-v2-closure.md](../../docs/journal/2026-07-10-1900-m2-m3-v2-closure.md) | Documentation: M2 and M3 v2 closure. |
| [docs/journal/2026-07-10-2000-tui-legend-visibility-and-status.md](../../docs/journal/2026-07-10-2000-tui-legend-visibility-and-status.md) | Documentation: TUI legend visibility, documentation alignment, and project status. Covers What changed this session; Verification; Where the project stands. |
| [docs/journal/2026-07-10-2100-quick-check-mode.md](../../docs/journal/2026-07-10-2100-quick-check-mode.md) | Documentation: Quick non-interactive check mode. Covers Why; What was added; Verification. |
| [docs/journal/2026-07-10-2200-ci-and-aur-packaging.md](../../docs/journal/2026-07-10-2200-ci-and-aur-packaging.md) | Documentation: CI pipeline and AUR packaging setup. Covers Why; What was set up; Still blocked on the project owner. |
| [docs/journal/README.md](../../docs/journal/README.md) | Documentation: Append-only project journal. Covers Index. |
| [docs/verification/M1-V2-2026-07-10.md](../../docs/verification/M1-V2-2026-07-10.md) | Documentation: M1 v2 verification and closure. Covers Delivered; Evidence. |
| [docs/verification/M2-2026-07-10.md](../../docs/verification/M2-2026-07-10.md) | Documentation: M2 verification and closure. Covers Delivered requirements; Verification commands and results; Budget disposition. |
| [docs/verification/M2-V2-2026-07-10.md](../../docs/verification/M2-V2-2026-07-10.md) | Documentation: M2 v2 verification and closure. Covers Delivered; Real-host evidence; Boundaries. |
| [docs/verification/M3-2026-07-10.md](../../docs/verification/M3-2026-07-10.md) | Documentation: M3 verification and closure. |
| [docs/verification/M3-V2-2026-07-10.md](../../docs/verification/M3-V2-2026-07-10.md) | Documentation: M3 v2 verification and closure. Covers Delivered; Verification; Hard boundary. |
| [docs/verification/RESET-PROGRESS-2026-07-10.md](../../docs/verification/RESET-PROGRESS-2026-07-10.md) | Documentation: Background-activity reset implementation progress. |
| [docs/whats-running-gui.png](../../docs/whats-running-gui.png) | Image or interface asset for whats running gui; 980 × 680 pixels. Source/asset rights remain separate from software-license terms. |
| [packaging/PKGBUILD](../../packaging/PKGBUILD) | Tracked PKGBUILD resource in packaging; 1,218 bytes. |
| [src/activity.rs](../../src/activity.rs) | Source module for activity; defines `SocketRecord`, `MountRecord`, `Inventory`, `collect`, `collect_sockets`, `read_bytes`, `failure` and 10 more symbols. |
| [src/check.rs](../../src/check.rs) | Source module for check; defines `report`, `print_listening_ports`, `decode_local`, `decode_v4`, `decode_v6`, `hex_bytes`, `decodes_ipv4_loopback_and_port` and 3 more symbols. |
| [src/events.rs](../../src/events.rs) | Source module for events; defines `socket`, `bind`, `send`, `recv`, `setsockopt`, `close`, `getpid` and 19 more symbols. |
| [src/gui.rs](../../src/gui.rs) | Source module for gui; defines `that`, `SortCol`, `FilterMode`, `UpdateSpeed`, `DisplayRow`, `Message`, `App` and 26 more symbols. |
| [src/main.rs](../../src/main.rs) | Source module for main; defines `Format`, `Options`, `main`, `run`, `print_inventory_json`, `json_string_vec`, `json_field_vec` and 28 more symbols. |
| [src/procfs.rs](../../src/procfs.rs) | Source module for procfs; defines `Field`, `status`, `value`, `copied`, `Process`, `Thread`, `visibility_summary` and 26 more symbols. |
| [src/recording.rs](../../src/recording.rs) | Source module for recording; defines `write_events`, `recording_is_bounded_and_private`. |
| [src/tui.rs](../../src/tui.rs) | Source module for tui; defines `Sort`, `Filter`, `View`, `signal`, `terminate`, `Terminal`, `enter` and 17 more symbols. |
