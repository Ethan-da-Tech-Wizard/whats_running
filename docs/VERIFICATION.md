# Verification record

Status: Historical verification record  
Date: 2026-07-10  
Environment: Arch Linux x86_64, kernel 7.1.3-arch1-1, rustc/cargo 1.96.0

## Commands executed

```text
cargo fmt
cargo test
cargo clippy -- -D warnings
cargo run --quiet --
cargo run --quiet -- --json
python -m json.tool /tmp/whats-running.json
```

## Results

- Five tests passed: stat names containing spaces/parentheses, NUL command parsing, status values, terminal-control sanitization, and JSON escaping.
- Clippy completed with warnings denied.
- Live table enumerated four visible processes with zero enumeration errors in the constrained validation environment.
- Observer PID 154 appeared as `whats-running` and was explicitly marked `[THIS TOOL]`.
- JSON parsed successfully and contained `command_lines_included: false` and exactly the observed self record with `is_observer: true`.
- Measured collection duration in that small environment was approximately 0.28 ms. This is evidence, not a general performance claim.

## M1 gaps retained honestly

- Numeric absence is currently `null`, not a typed reason.
- No CPU/I/O rates, trends, history, system aggregates, thermal data, tree, sorting, filtering, or interactive TUI yet.
- No large-scale benchmark or synthetic procfs integration fixture yet.
- No PKGBUILD or signed release yet; those belong to M5.
- Manual JSON serialization is tested for escaping but should gain broader property/fuzz coverage.

