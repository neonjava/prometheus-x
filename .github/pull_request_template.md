Closes #

## What changed


## Proof it works
Command run and output pasted:

```
```

## Checklist
- [ ] One issue, one branch, one PR; title is `<crate>: <what>`; under ~300 lines
- [ ] No hardcoded paths, thresholds, sizes, URLs or magic numbers (config.toml or CLI args)
- [ ] No `unwrap()`/`expect()` outside tests; errors returned as `Result` with clear messages
- [ ] No dead code, commented-out code, TODO stubs or placeholder functions
- [ ] No invented APIs; code compiles and runs
- [ ] Small functions, short WHY-only comments
- [ ] No unrelated changes or reformatting
- [ ] `cargo fmt` and `cargo clippy -- -D warnings` pass
- [ ] No shared function signature changed without asking neonjava
- [ ] I understand every line and can explain it in review
