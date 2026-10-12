# What’s Running?

A local Linux process inspector with command-line, terminal, and graphical views of background activity and listening ports.

**Status:** Linux system utility  
**Built with:** Rust, Linux procfs, iced

## Reading order

1. Read the root README for project purpose and setup navigation.
2. Follow the entry points below to find the implementation.
3. Use the file guide to locate individual modules, tests, assets, and configuration.
4. Check component licenses before redistributing dependencies or assets.

## Important paths

- [PROJECT_NOTES.md](../PROJECT_NOTES.md) — Preserved detailed setup, usage, architecture, and development notes from the earlier README.
- [Cargo.toml](../Cargo.toml) — Configuration or build metadata for Cargo; read alongside the application or tool that loads it.
- [src](../src) — Application source and platform integration

## Structure

| Path | Role |
| --- | --- |
| [.github/](../.github/) | GitHub workflow and contributor configuration (1 tracked entries). |
| [docs/](../docs/) | Project documentation, decisions, and reference material (57 tracked entries). |
| [packaging/](../packaging/) | Project-specific packaging components (1 tracked entries). |
| [src/](../src/) | Application source and platform integration (8 tracked entries). |

## Useful commands

Run from the repository root after following the documented prerequisites.

```sh
cargo run --release -- --check
```

## Scope

Results reflect kernel-visible state and current permissions. Runtime snapshots can contain sensitive process arguments; keep personal captures out of public commits.

## Maintaining the guide

The file catalog is a source-inspection snapshot, not proof that every runtime path works. Update it when moving files or changing module responsibilities. The 2026-10-12 documentation review did not run every application, deploy services, or validate all historical performance claims.
