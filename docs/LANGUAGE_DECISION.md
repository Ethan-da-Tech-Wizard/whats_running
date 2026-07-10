# Language and tooling recommendation

Status: Proposed

## Recommendation: Rust, with a deliberately boring architecture

Rust is the best fit for the collector and terminal application:

- low and predictable runtime overhead for a tool investigating overhead;
- strong types for “value versus denied versus vanished versus unsupported”;
- memory safety while parsing volatile, untrusted OS data;
- good single-binary distribution on Arch;
- capable terminal ecosystem (`ratatui`/`crossterm`) and serialization (`serde`);
- conditional compilation and native API access for future Windows/macOS collectors.

The spunky choice would be to build a tiny observability spaceship. The correct choice is Rust wearing sensible shoes: one binary, direct data flow, few dependencies, no daemon, no Electron, no web server.

## Alternatives

- **Go:** excellent simplicity and cross-compilation; garbage collection and TUI/library tradeoffs are acceptable, but typed systems parsing and fine control favor Rust here. Strong second choice.
- **Python:** superb for research prototypes, weaker for a polished always-running low-overhead monitor and self-contained packaging. Useful for test workload generators only if needed.
- **C/C++:** maximum native control, substantially higher memory-safety and maintenance cost without a compelling MVP benefit.
- **TypeScript/Electron:** fast UI work but disproportionate baseline CPU/RAM for a trust-sensitive system monitor.
- **Zig:** attractive low-level model, but smaller/more volatile ecosystem raises delivery risk.

## Proposed stack, pending spikes

- Rust stable edition supported by current Arch toolchain.
- Direct standard-library file reads plus narrowly chosen crates.
- `clap` for CLI, `serde`/`serde_json` for schema, `ratatui` + `crossterm` for TUI.
- Property/fuzz testing for parsers; criterion-style benchmarks only if dependency cost is justified.
- Markdown docs, JSON Schema, and an Arch PKGBUILD.

Final dependency selection requires an ADR and audit; crate names here are candidates, not commitments.

