# Accessor — contributor and agent rules

## Versioning (required)

- Bump `version` in `Cargo.toml` for every commit that changes runtime behavior.
- While pre-1.0, use semantic versioning: **patch** for fixes, **minor** for new
  user-visible features, **major** for breaking config, protocol, or data-format
  changes.
- Run `cargo check` after the bump so `Cargo.lock` stays in sync, and include
  both files in the same commit.
- Docs-only or test-only commits do not need a bump. Never bump twice for one
  logical change.
- One exception: a commit that only adds or edits `AGENTS.md` itself does not
  need a version bump.

## Build, lint, and test (must pass before committing)

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked --bin acc
python tests/smoke.py
python tests/runtime.py
```

The offline suites above are the gate. Live microphone/noise, real-model WAV,
Cartesia, and Linux/macOS hardware checks are separate and are not required for
every commit.

## Conventions

- Match the surrounding code style; this codebase favors compact, idiomatic Rust.
- Prefer editing existing files and patterns over introducing new abstractions.
- Settings live in `config.json`; credentials never do. Secrets go in the OS
  credential store (see `config::save_secret`).
- Wake detection, STT, and playback are latency-sensitive: keep new work off the
  speech/UI hot path and report overload visibly.
