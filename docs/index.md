# Accessor

Say "twenty-nine", hear a gentle chime, and talk to your agent. Accessor keeps
speech recognition local, manages the conversation and terminal display, and
reuses the agent's tools and account connections.

![Release](https://img.shields.io/github/v/release/T-Lind/accessor)
![License](https://img.shields.io/github/license/T-Lind/accessor)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue)
![Speech](https://img.shields.io/badge/speech-local%20by%20default-success)

[Get started](getting-started.md){:.btn} · [Architecture](architecture.md) ·
[Full README](https://github.com/T-Lind/accessor#readme)

## What it is

Accessor is a local voice shell around the command-line coding agents you already
use. It owns the microphone, wake/sleep, speech synthesis, the dashboard, routing,
and shared memory; each agent harness keeps its own tools, sandbox, and native
conversation thread.

- **Local by default.** Wake detection, conversation speech recognition, and the
  default voice run on this machine with no API key. Cloud transcription and
  speech (Cartesia) are opt-in.
- **Harness-agnostic.** Codex App Server, Claude Code, Antigravity, opencode, and
  Cursor, plus a mock mode that needs no account.
- **One conversation, many roles.** A lightweight main agent can delegate coding
  and connector work to isolated workers, then return a bounded result.
- **Durable context.** Shared memory, notes, alarms, scheduled tasks, watches, and
  notifications persist across harnesses and restarts.
- **Reviewable and reversible.** Passwords, permissions, desktop control, and
  event handling are explicit and off unless you turn them on.

## The voice pipeline

![Accessor 29 core voice pipeline](diagrams/29.png)

*Editable source: [`docs/diagrams/29.dot`](https://github.com/T-Lind/accessor/blob/main/docs/diagrams/29.dot);
regenerate with Graphviz `dot`. See [Architecture](architecture.md) for more
diagrams.*

## Install

Prebuilt binaries for Windows x64, Linux x64/ARM64, and Apple Silicon macOS are
on the [releases page](https://github.com/T-Lind/accessor/releases).

```sh
# Linux and Apple Silicon macOS
curl -fsSL https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.sh | sh
```

```powershell
# Windows x64 (PowerShell)
irm https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.ps1 | iex
```

Then start it:

```sh
acc doctor
acc -wakecode 29 speak
```

The first run downloads ONNX Runtime and the default Canary speech model (and the
pinned Piper engine plus a voice) into Accessor's assets folder. No Python is
needed for the default path. See [Getting started](getting-started.md) for
platform prerequisites and a first-run walkthrough.

## Explore the docs

| Guide | What's inside |
| --- | --- |
| [Getting started](getting-started.md) | Install, first wake, controls, and CLI basics |
| [Features](features.md) | Speech, routing, memory, organizer, notifications, computer use |
| [Architecture](architecture.md) | Pipeline, routing, memory, and security diagrams |
| [Configuration](configuration.md) | Portable vs device settings, key reference |
| [Harnesses](harnesses.md) | Supported agents, roles, connectors, and MCP |
| [Troubleshooting](troubleshooting.md) | Common install, audio, and platform problems |
| [Releases](releases.md) | Versioning, artifacts, and how updates work |
| [Local access lock](SECURITY.md) | Password boundary, spoken unlock, and recovery |
| [Platform notes](PLATFORM.md) | Overflow, Jev routing, and per-OS detail |
| [Voice performance](VOICE_PERFORMANCE.md) | Measured latency and deployment priorities |

## Requirements

Windows x64 is the tested development platform. Linux x64/ARM64, Apple Silicon
macOS, and Windows ARM64 use cross-platform libraries but need testing on actual
hardware. See [Getting started](getting-started.md#platform-prerequisites) for the
Linux build packages and per-OS notes.

## Project

- [Contributing guide](https://github.com/T-Lind/accessor/blob/main/CONTRIBUTING.md)
- [Third-party attribution](https://github.com/T-Lind/accessor/blob/main/THIRD_PARTY.md)
- [MIT License](https://github.com/T-Lind/accessor/blob/main/LICENSE)
