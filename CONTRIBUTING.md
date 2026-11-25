# Contributing

AI-written code is allowed. AI slop is not. You must understand every line you submit and be able to explain it in review.

## Roles
- neonjava: tech lead, core (features, detector, integration). Reviews and merges every PR.
- megha: analysis and evaluation
- rakshitha: data and CLI
- ruthwin: server, web, slides

## Workflow
- `main` is protected: PR required, 1 approval from neonjava, CI must pass, no direct push, squash merge only. Branches are deleted after merge.
- Branch names: `feat/<crate>-<short-name>`, `fix/<short-name>`, `docs/<short-name>`.
- One issue = one branch = one PR.
- PR title: `<crate>: <what>`. Body: `Closes #<issue>` and how you tested it.
- PR proof: paste the command you ran and its output.
- Commits are authored as yourself. No Co-Authored-By or "Generated with" lines.

## Code rules
- No hardcoded paths, thresholds, sizes, URLs or magic numbers. Use `config.toml` or CLI args.
- No `unwrap()`/`expect()` outside tests. Return `Result` with clear errors.
- No dead code, commented-out code, TODO stubs or placeholder functions.
- No invented APIs. Code must compile and run. Check the candle docs/examples when unsure.
- Small functions, one job each. Comments explain WHY only, kept short.
- PR under ~300 lines. No unrelated changes or reformatting.
- `cargo fmt` and `cargo clippy -- -D warnings` must pass.
- Do not change shared function signatures without asking neonjava.
- Only code goes in the repo. `data/`, `models/` and build output stay out of git.

## CI/CD
You do not need Rust installed. GitHub Actions runs everything on every PR:
- `ci`: `cargo fmt --check`, clippy (`-D warnings`, plus unwrap/expect denied outside tests), build, test.
- The build uploads the release binaries as a PR artifact so neonjava can download and run them.
- Pushing a tag `v*` builds a release and publishes the binaries to a GitHub Release.
