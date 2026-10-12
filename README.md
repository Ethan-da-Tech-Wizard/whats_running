# What’s Running?

A local Linux process inspector with command-line, terminal, and graphical views of background activity and listening ports.

**Status:** Linux system utility  
**Built with:** Rust, Linux procfs, iced

[Project guide](docs/PROJECT_GUIDE.md) · [Every file explained](docs/FILE_GUIDE.md) · [Licenses and dependencies](docs/LICENSE_REVIEW.md)

## Start here

- [PROJECT_NOTES.md](PROJECT_NOTES.md) — Preserved detailed setup, usage, architecture, and development notes from the earlier README.
- [Cargo.toml](Cargo.toml) — Configuration or build metadata for Cargo; read alongside the application or tool that loads it.
- [src](src) — Application source and platform integration

The following commands are taken from this project’s source/configuration. Install its prerequisites first.

```sh
cargo run --release -- --check
```

[Detailed setup, usage, and development notes](PROJECT_NOTES.md) preserve the previous project documentation.

## Repository layout

| Path | Role |
| --- | --- |
| [.github/](.github/) | GitHub workflow and contributor configuration (1 tracked entries). |
| [docs/](docs/) | Project documentation, decisions, and reference material (57 tracked entries). |
| [packaging/](packaging/) | Project-specific packaging components (1 tracked entries). |
| [src/](src/) | Application source and platform integration (8 tracked entries). |

## Scope and current limits

Results reflect kernel-visible state and current permissions. Runtime snapshots can contain sensitive process arguments; keep personal captures out of public commits.

## License

Original project code retains **MIT OR Apache-2.0** terms in [LICENSE-MIT](LICENSE-MIT). Third-party libraries, media, data, models, and tools retain their own terms. See the [license review](docs/LICENSE_REVIEW.md) and [dependency inventory](docs/DEPENDENCIES.csv).
