# Contributing

AI-written code is allowed. AI slop is not. You must understand every line you submit and be able to explain it in review.

## Roles
- neonjava: tech lead, core (features, detector, integration), docs (slides, README). Reviews and merges every PR.
- megha: analysis and evaluation
- rakshitha: data and CLI
- ruthwin: server, web

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

## Code structure
Cargo workspace. One crate = one job. Dependencies only point down this list:

```
config      Config struct, Config::load(path) -> Result<Config>   (neonjava)
features    ResNet extractor: image -> Vec<f32>                   (neonjava)
data        folder -> images loader                               (rakshitha)
detector    score, good-vector store, verdict, predict            (neonjava)
analysis    threshold, severity, evaluate                         (megha)
cli         clap subcommands: predict, evaluate                   (rakshitha)
server      axum POST /predict                                    (ruthwin)
web/        plain HTML upload page                                (ruthwin)
scripts/    dataset download and split                            (rakshitha)
```

- Libraries (config, features, data, detector, analysis) return `Result<T, E>` with `thiserror` errors. Only `cli` and `server` use `anyhow` and print errors.
- No `println!` in libraries. Only `cli` and `server` print.
- Every tunable value is a key in `config.toml` and a field in `Config`. Add the key to `config.toml` in the same PR.
- A crate only edits its own folder. Need a change in someone else's crate? Ask in the issue.
- Tests live next to the code (`#[cfg(test)]`). Test with tiny synthetic data, never real dataset files.
- Never commit data, model weights, images, or build output.

## Using Antigravity (or any AI) without making slop
1. Open the issue. Give the AI only that issue plus `AGENTS.md`. Do not ask it to "build the project".
2. Tell it the exact file to edit and the exact function signature from the issue.
3. After it writes code, read every line. Delete anything you cannot explain: extra helpers, extra error types, long comments, unused imports, "just in case" code.
4. Run it yourself and paste the real output into the PR.
5. If it invents a candle function that does not compile, do not fight it. Check the candle repo examples (https://github.com/huggingface/candle/tree/main/candle-examples) and tell neonjava if stuck.
6. Common slop to reject: long doc comments, `unwrap()`, hardcoded `224` or `"data/..."`, a trait or builder where a function is enough, new dependencies nobody asked for, reformatted files, TODO stubs.
7. Smaller is better. If your diff passes ~300 lines, you did too much. Split it.

## Review
neonjava reviews every PR against this file. A PR is sent back if the author cannot explain a line, there is no proof output, or any checklist item is unticked.
