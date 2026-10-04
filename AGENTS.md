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

## Local command after building (required)

- After every successful local release build, ensure the `acc` command on the
  developer's `PATH` runs this checkout's `target/release/acc`, rather than an
  older installed copy. On Unix, link the local `acc` launcher to the absolute
  release-binary path; subsequent builds then update it automatically.
- Verify both `acc --version` and `target/release/acc --version` match the
  version in `Cargo.toml`. Check `command -v acc` if they differ, and fix the
  local launcher before reporting the build complete.
- Existing Accessor sessions need a restart to use a rebuilt binary.

## Conventions

- Match the surrounding code style; this codebase favors compact, idiomatic Rust.
- Prefer editing existing files and patterns over introducing new abstractions.
- Accessor may create worker tasks itself; spin them off appropriately so they
  do not block the speech or UI hot path.
- Agents may also start Codex, Antigravity, or another appropriate agent
  harness and monitor those full threads directly; they are not necessarily
  sub-agents.
- Settings live in `config.json`; credentials never do. Secrets go in the OS
  credential store (see `config::save_secret`).
- Wake detection, STT, and playback are latency-sensitive: keep new work off the
  speech/UI hot path and report overload visibly.
