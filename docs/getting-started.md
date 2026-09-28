# Getting started

Accessor is a single small binary, `acc`. The heavy pieces — ONNX Runtime, the
speech recognition model, and the Piper voice — download on first run into
Accessor's own assets folder, so the release artifacts stay around 20 MB.

## At a glance

1. Install `acc`.
2. Run `acc doctor` to check the machine and trigger first-run downloads.
3. Run `acc -wakecode 29 speak`.
4. Say **"twenty-nine"**, wait for the chime, then speak.

## Install

### Prebuilt binary (recommended)

**Linux and Apple Silicon macOS**

```sh
curl -fsSL https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.sh | sh
```

The installer detects the platform, downloads the matching archive, verifies its
SHA-256 against the release `SHA256SUMS`, and installs to `$ACC_BIN_DIR`
(default `~/.local/bin`). Pin a version with `ACC_VERSION=0.34.3`.

**Windows x64 (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.ps1 | iex
```

Installs into `%LOCALAPPDATA%\Programs\accessor` and adds it to your user `PATH`.
Pass `-Version 0.34.3` to pin a release when running a saved copy of the script.

### Build from source

Install Rust, then:

```sh
cargo install --path . --bin acc --locked
acc doctor
```

On Windows from a checkout you can also run `.\install-accessor.ps1`, and the
older launcher still works: `.\start-accessor.ps1 -WakeCode 29 -Speak`.

> **Unsigned binaries.** The macOS build is unnotarized, so Gatekeeper
> quarantines it on first launch — right-click → **Open**, or run
> `xattr -d com.apple.quarantine /path/to/acc`. The Windows build is unsigned,
> so SmartScreen warns once (More info → Run anyway).

## Platform prerequisites

| Platform | What you need |
| --- | --- |
| Windows x64 | Rust MSVC/C++ build tools (source builds only) |
| Linux x64/ARM64 | `libasound2-dev libdbus-1-dev libwayland-dev libxcb1-dev libxrandr-dev libxkbcommon-dev libpipewire-0.3-dev libgbm-dev libdrm-dev libegl1-mesa-dev libgles2-mesa-dev pkg-config` on Debian/Ubuntu |
| Apple Silicon macOS | Xcode command-line tools |
| Linux system voices | `espeak-ng` for the System TTS provider |
| Linux credential store | A running Secret Service such as GNOME Keyring |

Intel macOS is not shipped prebuilt: there is no ONNX Runtime download entry for
`x86_64-apple-darwin`. Build from source and supply a compatible ONNX Runtime
1.24+ dylib. 32-bit machines are not supported by the provided setup.

## First run

The first launch performs one-time downloads and installs the pinned Piper
engine plus `en_GB-alan-medium`:

- ONNX Runtime 1.24.2 for your OS/CPU
- Canary 180M Flash INT8 (the default speech recognizer)
- The Piper engine and default voice

`acc setup` and `acc doctor` re-check the speech downloads if any files are still
missing. Python is needed only for the optional Kokoro voice worker; Canary,
Piper, and the gateway run natively in Rust.

```console
$ acc doctor
$ acc devices          # list microphones
$ acc mic              # report ambient floor, speech level, SNR
```

## Your first conversation

```sh
acc -wakecode 29 speak
acc run --wake-code 29 --speak   # equivalent
```

Say **"twenty-nine"** or **"hey twenty-nine"**, pause for the chime, then speak.
"Twenty-nine, do this" in one utterance also works. The conversation stays open
for **120 seconds of idle time** after the last accepted request, reply, or
completed work; set `--idle-seconds 0` to disable automatic sleep.

The numeric wake word reduces accidental activations; it is **not**
authentication. For a real access boundary, see [Local access lock](SECURITY.md).

## The dashboard

Interactive terminals use a fixed dashboard with status, a bounded conversation
area, and a command field. Type `/` to open the command menu; filter by typing,
choose with the arrow keys, and press Enter or Tab. You can type ordinary messages
directly to the agent without a voice wake code.

Everything day to day is available in the screen:

| In-screen command | Behavior |
| --- | --- |
| `/settings` | Category menu: Voice, Speech, Harnesses, Display, Metaprompt, Tests, Security, Computer |
| `/setup` | Five-step setup wizard; Enter keeps defaults, Escape cancels |
| `/config`, `/config set KEY VALUE` | Inspect and save settings |
| `/tts`, `/tts provider piper`, `/tts voice en_GB-alan-medium` | Configure spoken replies (system, kokoro, piper, cartesia, off) |
| `/tts test`, `/tts voices [search]` | Audition or list/search voices |
| `/tts key` | Masked Cartesia key entry, stored in the OS credential store |
| `/stt test`, `/stt off` | Start/stop live transcription testing |
| `/noise` | Room-noise gate: `calibrate`, `reset`, `on`, `off` |
| `/prompt` | Show the metaprompt; `preset`, `extra TEXT`, `edit`, `reset` |
| `/memory` | Inspect shared facts; `review`, `infer`, `forget N` |
| `/watch every 30m …` | Create a recurring, gated survey; `list`, `edit ID`, `stop ID` |
| `/notifications` | Review raised notifications: `list`, `read ID`, `readall`, `dismiss ID` |
| `/organizer` | Notes, alarms, and scheduled tasks (aliases `/notes`, `/alarms`, `/tasks`) |
| `/logs` | Show the master log path and tail it |
| `/devices` | List microphones in the conversation area |
| `/connectors` | Check the agent's connected apps |
| `/agent` | Open Codex's native UI directly |
| `/update` | Check Codex / Claude Code / Antigravity and apply CLI updates |
| `/status`, `/help` | Inspect state or controls |
| `/quit`, Ctrl+C | Stop Accessor and its agent connection |

Page Up/Down scroll the conversation; Escape cancels a task. Use `--plain` for
ordinary terminal output; pipes automatically use plain mode.

## CLI quick reference

```sh
acc setup                       # first-run setup
acc doctor                      # diagnose install, speech, and harnesses
acc devices                     # list microphones
acc mic [--device NAME]         # measure ambient floor and SNR
acc config show                 # print effective settings
acc config set wake-code 29     # change a setting
acc config locations            # show the settings folder
acc config export portable.json # export portable preferences
acc config import portable.json # merge them into this profile
acc connectors status           # check connected apps
acc agent login                 # sign in to the selected harness
acc organizer status            # notes, alarms, scheduled tasks
acc notifications list --unread
acc memory list
acc journal show
acc update                      # update agent CLIs
acc mcp                         # serve MCP over stdio
```

Settings live in the OS user configuration directory shown by `acc config
path`. `ACC_HOME` selects a different settings/event directory; `ACC_ASSETS`
selects a model/runtime directory. Credentials are never stored in `config.json`.

## Choosing speech

New profiles default to **local Canary** after-wake recognition and fast local
**Piper** speech, so the default path needs no key.

```sh
acc tts setup
acc tts test
acc stt test
```

- **Piper** (default): fast local neural speech, pinned build plus one voice.
- **System**: Windows System.Speech, macOS `say`, Linux `espeak-ng`.
- **Kokoro**: local neural speech via an optional Python/ONNX worker.
- **Cartesia**: opt-in cloud speech, key in the OS credential store.
- **Off**: display replies without speech.

See [Features](features.md#speech) and
[Voice performance](VOICE_PERFORMANCE.md) for the trade-offs.

## Next steps

- [Features](features.md) — the full capability tour
- [Architecture](architecture.md) — how the pipeline fits together
- [Configuration](configuration.md) — every setting that matters
- [Troubleshooting](troubleshooting.md) — when something does not work

> **Before unattended use,** read [Local access lock](SECURITY.md). Accessor
> protects an Accessor interface, not an unlocked OS account.
