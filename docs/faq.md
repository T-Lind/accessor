# FAQ

Answers to the questions that come up most. For step-by-step setup see
[Getting started](getting-started.md); for problems see
[Troubleshooting](troubleshooting.md).

## Is Accessor free, and does it need API keys?

The default path needs no key and no account. Speech recognition (Canary) and the
default voice (Piper) run locally, and the harnesses use their existing CLI logins.
Cartesia (cloud speech in both directions) is opt-in and needs a key; Codex uses
your existing login and Accessor never calls the OpenAI API directly.

## Does my audio leave my computer?

Not by default. Wake detection, conversation recognition, and the default voice
are local. If you select Cartesia for **after-wake STT**, detected awake speech is
uploaded while you speak (only the final transcript is submitted to the agent), and
wake detection and spoken unlocking stay local. While the password lock is active,
cloud STT is disabled. See [Features → Speech](features.md#speech).

## Which agents are supported?

Codex App Server, Claude Code, Antigravity (`agy`), opencode, and Cursor
(`cursor-agent`), plus a `mock` harness that needs no account. See
[Harnesses](harnesses.md).

## Do I need Python?

No, not for the default path. Canary, Piper, and the gateway run natively in Rust.
Python is only needed for the optional Kokoro voice worker and a few helper
scripts.

## What are the system requirements?

Windows x64 is the tested platform; Linux x64/ARM64, Apple Silicon macOS, and
Windows ARM64 use cross-platform libraries but need real-hardware testing. Linux
builds need the desktop dev packages; see
[Getting started → Platform prerequisites](getting-started.md#platform-prerequisites).
Intel macOS is source-only.

## How is this different from other local voice tools?

Accessor is a *gateway*, not another transcription app: it keeps the voice pipeline
local, manages wake/sleep, routing, memory, and playback, and drives one or more
existing CLI coding agents without replacing their tools, sandbox, or login. It
reuses the agent's own connectors rather than shipping a second OAuth store.

## Can it run as a background service or on a headless server?

No. This release does not install a service; alarms, tasks, and watches are
checked only while an `acc` process is running, and a powered-off or suspended
machine is not woken. It is a desktop audio application, not a server build.

## Does it work without a microphone?

Yes. `acc --text --agent mock` runs the full conversation plumbing with no
microphone and no account, which is the fastest way to test or demo. `--text`
never opens a microphone and is silent unless `--speak` is explicitly passed.

## How do I change the wake word?

`acc config set wake-code 29`, or say/type a settings change in the dashboard.
"Hey 29", "hi 29", and "ok 29" are accepted variants. The wake word reduces
accidental activations; it is not authentication — see the
[local access lock](SECURITY.md).

## Where is my data stored?

Settings and local state live in the directory shown by `acc config path`:
`config.json`/`device.json`, `memory.json`, `notes/`, `schedules.json`,
`notifications.json`, the optional master log, and the encrypted journal. Secrets
go in the OS credential store, never in `config.json`. The full table is in
[Troubleshooting → Logs and data locations](troubleshooting.md#logs-and-data-locations).

## Can I move my settings to another machine?

Yes. `acc config export FILE` writes the portable subset and `acc config import
FILE` merges it, keeping the target's device settings. Copying the whole folder
also works but carries `device.json`; delete it on the new machine or import
instead.

## What is the dictation journal?

An encrypted, local-only log of speech you explicitly dictate. It never goes to an
agent or cloud service and is encrypted at rest with XChaCha20-Poly1305, with its
key in the OS credential store. See
[Local-only dictation](features.md#local-only-dictation).

## Can an agent control my computer?

Only if you enable it. Desktop control is **off by default** and is deliberately
outside the agent-editable settings surface, so an agent cannot turn it on for
itself. When enabled, it can drive the real mouse, keyboard, and screen — it is not
a sandbox. See [Desktop control](features.md#desktop-control-computer-use).

## How fast is it?

It depends on the model, microphone, and CPU. Accessor reports median/P95 timings
in `/analytics` and ships repeatable benchmarks; see
[Voice performance](VOICE_PERFORMANCE.md). Local TTS uses completed WAV chunks,
while Cartesia streams PCM.

## How do I update Accessor?

Re-run the installer; `acc update` only updates the agent CLIs, not Accessor
itself. See [Releases](releases.md).

## Why do macOS and Windows warn about the app?

The binaries are unsigned (and macOS builds are unnotarized), so Gatekeeper and
SmartScreen warn once. The workarounds are in
[Getting started](getting-started.md). Code signing is planned but not shipped.

## How do I report a bug or suggest a feature?

Use the [issue templates](https://github.com/T-Lind/accessor/issues/new/choose).
For anything involving the access lock or a privacy boundary, read
[Local access lock](SECURITY.md) first and describe the impact privately rather
than in a public issue.
