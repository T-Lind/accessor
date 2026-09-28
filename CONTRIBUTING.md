# Contributing to Accessor

Thanks for helping. Accessor is a local voice gateway that sits between a
microphone, a speech pipeline, and a command-line coding agent. Two rules shape
almost every review:

1. **Keep the speech and UI hot path clear.** Wake detection, STT, and playback
   are latency-sensitive. New work (network, disk, model loads, subprocesses)
   should run off that path and report overload visibly.
2. **Keep speech local by default.** Cloud transcription and speech are opt-in,
   and credentials belong in the OS credential store, never in `config.json`.

## Development setup

Install Rust (stable), then the platform build dependencies:

- **Debian/Ubuntu**

  ```sh
  sudo apt-get install -y libasound2-dev libdbus-1-dev libwayland-dev \
    libxcb1-dev libxrandr-dev libxkbcommon-dev libpipewire-0.3-dev \
    libgbm-dev libdrm-dev pkg-config
  ```

- **Windows**: Rust MSVC toolchain plus the C++ build tools.
- **macOS**: Xcode command-line tools.

Build and run from the checkout:

```sh
cargo run --bin acc -- doctor
cargo run --bin acc -- -wakecode 29 speak
```

The first run downloads ONNX Runtime and the default Canary speech model into
Accessor's assets folder. `--agent mock` and `--text` need no account and no
microphone, which is the fastest way to exercise the conversation plumbing.

## Before you open a pull request

Run the full gate; it must pass:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked --bin acc
python tests/smoke.py
python tests/runtime.py
```

`python tests/memory_runtime.py` and `python tests/security_runtime.py` cover
the memory store and the access lock. Live microphone/noise, real-model WAV,
Cartesia, and Linux/macOS hardware checks are separate and are not required for
every change; say in the PR what still needs physical validation.

## Versioning

Bump `version` in `Cargo.toml` for any commit that changes runtime behavior:
**patch** for fixes, **minor** for new user-visible features, **major** for
breaking config/protocol/data changes. Run `cargo check` so `Cargo.lock` stays
in sync, and include both files in the same commit. Docs-only and test-only
changes do not need a bump.

## Style

- Match the surrounding code; this codebase is compact and idiomatic Rust.
- Prefer editing existing patterns over adding new abstractions.
- Keep agent-editable settings narrow. Anything that can move audio off-device,
  grant permissions, or execute on the host is deliberately user-only.
- Never write secrets, raw audio, or private transcripts to logs or the repo.

## Reporting security issues

See [`docs/SECURITY.md`](docs/SECURITY.md) for the threat model, the password
and journal boundaries, and how to reset a forgotten passphrase. Do not open a
public issue for a vulnerability that would let another local process bypass the
lock; describe the impact privately to the maintainer first.

## License

By contributing you agree your work is licensed under the repository's
[MIT License](LICENSE).
