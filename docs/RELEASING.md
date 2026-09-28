# Releasing Accessor

Accessor ships as a small prebuilt `acc` binary. The heavy assets (ONNX Runtime,
the STT model, the Piper engine and voice) are downloaded on first run into the
per-user assets folder, so release artifacts stay around 20 MB.

## 1. Prepare the change

Versioning follows [AGENTS.md](../AGENTS.md): patch for fixes, minor for new
user-visible features, major for breaking config/protocol/data changes. Bump
`version` in `Cargo.toml` and let `cargo check` update `Cargo.lock`, then run the
full gate:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked --bin acc
python tests/smoke.py
python tests/runtime.py
```

## 2. Tag and push

```sh
git add -A && git commit -m "Release 0.34.3"
git tag v0.34.3
git push origin main v0.34.3
```

The `Release` workflow (`.github/workflows/release.yml`) runs on any `v*` tag. It
builds and publishes:

| Runner | Rust target | Archive |
| --- | --- | --- |
| `ubuntu-24.04` | `x86_64-unknown-linux-gnu` | `acc-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `acc-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| `macos-14` | `aarch64-apple-darwin` | `acc-<version>-aarch64-apple-darwin.tar.gz` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `acc-<version>-x86_64-pc-windows-msvc.zip` |

Each archive contains `acc` (`acc.exe`), `LICENSE`, and `THIRD_PARTY.md`. The
workflow also publishes a combined `SHA256SUMS`. Both Linux builds use Ubuntu
24.04 because the PipeWire headers on 22.04 are too old for the desktop-capture
bindings, so the Linux binaries need **glibc 2.39+** (Ubuntu 24.04, Fedora 40+,
Debian 13+). Older distributions should build from source.

Use **Actions → Release → Run workflow** to build the matrix without publishing;
the release job only runs for tags.

## 3. Verify

- The release has all four archives plus `SHA256SUMS`.
- `curl -fsSL .../main/scripts/install.sh | sh` installs `acc` and `acc doctor`
  starts the first-run downloads.
- On each platform, run `acc tts test` and `acc doctor`, then a real wake.

## 4. Installers

- `scripts/install.sh` (Linux, Apple Silicon macOS): detects the platform,
  downloads the matching archive, verifies the SHA-256 against `SHA256SUMS`, and
  installs to `$ACC_BIN_DIR` (default `~/.local/bin`). `ACC_VERSION` pins a
  version.
- `scripts/install.ps1` (Windows x64): same flow into
  `%LOCALAPPDATA%\Programs\accessor` and adds it to the user `PATH`.

## Platform notes to pass on

- **Linux runtime libraries**: the binary links common desktop libraries
  (`libasound2`, `libdbus-1`, `libwayland-client`, `libxcb`, `libgbm`,
  `libsystemd`). Secret storage needs a running Secret Service such as GNOME
  Keyring. This is not a headless/server build.
- **macOS**: the binary is unsigned and unnotarized, so Gatekeeper quarantines
  it. The installer prints the workaround; document `xattr -d
  com.apple.quarantine` or right-click → Open. Desktop control needs
  Accessibility permission.
- **Windows**: the binary is unsigned, so SmartScreen warns on first run
  (More info → Run anyway).
- **Intel macOS** is intentionally not shipped: there is no ONNX Runtime
  download entry for `x86_64-apple-darwin`. Build from source and supply a
  compatible ONNX Runtime 1.24+ dylib.

## Not done yet

- **Code signing / notarization.** Add a Windows Authenticode certificate and an
  Apple Developer ID when targeting non-technical users, and wire the secrets into
  this workflow.
- **Package managers.** A Homebrew tap, Scoop bucket, and winget manifest would
  wrap the same release assets.
- **`cargo install accessor` / crates.io.** Blocked by the vendored Canary fork:
  `[patch.crates-io] transcribe-rs = { path = "vendor/transcribe-rs" }` is a path
  dependency, which crates.io forbids in published crates. Publishing needs the
  fork resolved first (publish it as its own crate and depend on it normally, or
  upstream the Canary-confidence change).
- **Self-update.** `acc update` only updates Codex/Claude/Antigravity; Accessor
  itself is updated by re-running the installer (or `brew upgrade`, once a tap
  exists).
