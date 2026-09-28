# Releases

Accessor ships as a small prebuilt `acc` binary. The heavy assets — ONNX Runtime,
the speech model, the Piper engine and voice — download on first run, so release
artifacts stay around 20 MB.

## Versioning

While Accessor is pre-1.0 it follows semantic versioning:

- **patch** — fixes
- **minor** — new user-visible features
- **major** — breaking config, protocol, or data-format changes

The current version is shown by `acc --version` and on the
[releases page](https://github.com/T-Lind/accessor/releases).

## Artifacts

Each tagged release publishes these archives plus a combined `SHA256SUMS`:

| Runner | Rust target | Archive |
| --- | --- | --- |
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | `acc-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `acc-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| `macos-14` | `aarch64-apple-darwin` | `acc-<version>-aarch64-apple-darwin.tar.gz` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `acc-<version>-x86_64-pc-windows-msvc.zip` |

Each archive contains `acc` (`acc.exe`), `LICENSE`, and `THIRD_PARTY.md`. Linux
x64 is built on Ubuntu 22.04 to keep the glibc floor at 2.35; the ARM64 build uses
Ubuntu 24.04, so it needs glibc 2.39+. Intel macOS is intentionally not shipped.

Both installers download the archive and verify its SHA-256 against the release's
`SHA256SUMS` before installing.

## Installing and upgrading

```sh
curl -fsSL https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.sh | sh
```

```powershell
irm https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.ps1 | iex
```

Re-running the installer upgrades in place. `acc update` is a different command:
it checks and updates the **agent CLIs** (Codex, Claude Code, Antigravity), not
Accessor itself. There is no self-updater yet.

## Building from source

```sh
cargo install --path . --bin acc --locked
```

The crate is not published to crates.io: it depends on a vendored fork of
`transcribe-rs` (which exposes Canary per-token confidence) through a
`[patch.crates-io]` path dependency, which crates.io forbids. Publishing would
require resolving the fork first.

## Changelog and attribution

- [Changelog](https://github.com/T-Lind/accessor/blob/main/CHANGELOG.md)
- [All releases](https://github.com/T-Lind/accessor/releases)
- [Third-party attribution](https://github.com/T-Lind/accessor/blob/main/THIRD_PARTY.md)

Maintainers cutting a release should follow the
[release runbook](https://github.com/T-Lind/accessor/blob/main/docs/RELEASING.md).
