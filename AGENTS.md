# Rules for AI coding tools
Read CONTRIBUTING.md first. Work on ONE issue only.

- Rust workspace, candle for ML, `image` crate, axum for the server.
- No hardcoded paths, sizes, thresholds, URLs. Read them from `Config` (config.toml) or CLI args.
- No `unwrap()`/`expect()` outside tests. Return `Result` with clear errors.
- No dead code, commented-out code, TODO stubs, placeholder functions.
- Do not invent APIs. If unsure a candle function exists, say so instead of guessing.
- Small functions, one job each. Short comments that explain WHY only.
- Keep the diff under ~300 lines. Touch only the files named in the issue.
- Do not add dependencies or change function signatures unless the issue says so.
- `cargo fmt` and `cargo clippy -- -D warnings` must pass.
