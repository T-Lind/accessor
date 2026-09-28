---
layout: default
title: Accessor
---

# Accessor

Say "twenty-nine", hear a gentle chime, and talk to your agent. Accessor keeps
speech recognition local, manages the conversation and terminal display, and
reuses the agent's tools and account connections.

The full project overview, quick start, and CLI reference live in the
[README](https://github.com/T-Lind/accessor#readme).

## Guides

- **[Platform notes](PLATFORM.html)** — history, Jev routing, Ink-2, overflow
  behavior, and per-OS details.
- **[Local access lock](SECURITY.html)** — the password boundary, spoken vs.
  keyboard unlock, auto-lock, journal encryption, and recovery.
- **[Voice performance](VOICE_PERFORMANCE.html)** — measured latency,
  benchmarks, and deployment priorities.

## Install

```sh
# Linux and Apple Silicon macOS
curl -fsSL https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.sh | sh
```

```powershell
# Windows x64 (PowerShell)
irm https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.ps1 | iex
```

Prebuilt binaries for Windows x64, Linux x64/ARM64, and Apple Silicon macOS are
on the [releases page](https://github.com/T-Lind/accessor/releases).

## Getting help

- [Search or open an issue](https://github.com/T-Lind/accessor/issues)
- [Read the security policy](SECURITY.html) before unattended use
- [Contributing guide](https://github.com/T-Lind/accessor/blob/main/CONTRIBUTING.md)
