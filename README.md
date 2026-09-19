# Accessor

Say “twenty-nine”, hear a gentle chime, and talk to your agent. Accessor keeps speech recognition local, manages the conversation and terminal display, and reuses the agent’s tools and account connections.

The stack is Rust/Tokio, Clap, Ratatui, CPAL, Earshot voice detection, and Canary 180M Flash INT8 via [transcribe-rs](https://github.com/cjpais/transcribe-rs), the transcription library behind [Handy](https://github.com/cjpais/Handy). Codex App Server, Claude Code, and Antigravity (`agy`) are supported harnesses; mock mode needs no account. Platform notes (history, Jev routing, Ink-2, overflow) live in [docs/PLATFORM.md](docs/PLATFORM.md).

## Quick start

On Windows, from this checkout:

```powershell
.\install-accessor.ps1
acc -wakecode 29 speak
```

The existing launcher also works: `.\start-accessor.ps1 -WakeCode 29 -Speak`.

On Linux/macOS, install Rust and run:

```sh
cargo install --path . --bin acc --locked
acc
```

The first run downloads ONNX Runtime and the default Canary speech model into Accessor’s assets folder (`~/.config/accessor/assets` on Linux, `~/Library/Application Support/Accessor/assets` on macOS). No Python and no `assets-dir` pointing at the git checkout. `acc setup` and `acc doctor` do the same download if files are still missing.

Python is needed only for the optional Kokoro worker. Canary and the gateway run in the native Rust process. Codex uses its existing login; Accessor does not call the OpenAI API directly. Windows automatically prefers the desktop-bundled Codex executable when available. Override with `acc config set codex-bin PATH` or `--codex-bin PATH`.

## A CLI for everyday use

```sh
acc setup
acc doctor
acc devices
acc config show
acc config set wake-code 29
acc config set idle-seconds 120
acc config set microphone "Microphone device name"
acc connectors status
acc connectors setup
acc agent login
acc update
acc config locations
```

`acc` starts listening using saved settings. `acc -wakecode 29 speak` and `acc run --wake-code 29 --speak` are equivalent. Settings live in the OS user configuration directory shown by `acc config path` and `acc config locations`. `ACC_HOME` selects a different settings/event directory; `ACC_ASSETS` selects a model/runtime directory. Credentials are never stored in config.json.

Interactive terminals use a fixed dashboard with status, a bounded conversation area, and a command field. Status changes update in place. Type `/` to open the command menu, filter by typing, use arrows to choose, and press Enter or Tab. You can type ordinary messages directly to the agent without a voice wake code (except in `--text` transcript simulation mode).

Everything needed day to day is available inside the screen:

| In-screen command | Behavior |
| --- | --- |
| `/settings` | Category menu: Voice, Speech, Harnesses, Display, Tests. ↑/↓, Enter, Esc |
| `/setup` | Four-step setup wizard; Enter keeps defaults, Escape cancels |
| `/config`, `/config set KEY VALUE` | Inspect and save settings |
| `/tts`, `/tts provider kokoro`, `/tts voice af_heart` | Configure spoken replies |
| `/tts test`, `/tts voices` | Audition or list voices |
| `/tts key` | Masked Cartesia key entry; saved in the OS credential store |
| `/stt test`, `/stt off` | Start/stop live transcription testing |
| `/devices` | List microphones in the conversation area |
| `/connectors` | Check the agent's connected apps |
| `/connectors setup`, `/agent` | Open Codex's native UI for account/plugin setup; exit it to return |
| `/update` | Check Codex / Claude Code / Antigravity and apply CLI updates |
| `/config locations` | Show the settings folder to copy to another machine |
| `/events` | Inspect the local event queue |

Wake, timeout, speech, and voice changes take effect immediately. Microphone, asset path, and Codex executable changes require restart. If speech/model initialization fails, the dashboard remains open for typed messages and setup. Page Up/Down scroll the conversation; Escape cancels a task. Use `--plain` for ordinary terminal output; pipes automatically use plain mode. Plain status lines print only when status changes. The dashboard is not saved as a transcript log.

## Settings and spoken switching

Open `/settings` for a category menu (Voice, Speech, Harnesses, Display, Tests). **↑/↓** moves, **Enter** opens or toggles, **Esc** goes up one level. Typed `/settings KEY VALUE` still works for scripts. Changes persist and apply immediately. The status bar always shows the live **harness · model** and a short identity code (`X S` Codex Sol, `C F` Claude Fable, `A F` Antigravity Flash).

**Harnesses:** **Plugin** is the CLI with Gmail/Calendar/Docs connectors. **Coding** is repos and tests. **Everyday** is general chat. Each role has its own **model list for that CLI** (Everyday = Antigravity shows Gemini, not the Codex catalog). Router `keywords` or `jev` (TypeSafe; `/jev key` or `TYPESAFE_API_KEY`) picks among those three per turn. `off` always uses everyday. Paste API keys with Ctrl+Shift+V / Shift+Insert, or `printf '%s' "$KEY" | acc jev key`. Enable **Jev auto-select** to use Jev whenever a TypeSafe key is present.

Model discovery reads the selected Codex installation's account catalog without running an LLM. `/settings refresh` refreshes it; the last successful list is cached for offline display.

Once activated, say "switch to Codex" or "switch agent to Codex". Say "switch model to Astra" using a name from your available list. A running harness can emit `ACCESSOR_SWITCH harness=codex` (or JSON `accessor_switch`) to pin this conversation to another CLI without rewriting the plugin/coding/everyday slots. These local controls obey the same wake-code and barge-in settings as ordinary speech.

Direct equivalents: `/settings barge-in true`, `/settings addressed true` (require the wake code on **every** request, including barge-ins), `/settings addressed false` (continuous conversation after one wake), `/settings idle-seconds 120`, `/settings speak-progress false`, `/settings chat activity`, `/settings tts.speed 1.1`, `/settings model MODEL_ID`. The numbered model picker and spoken commands use the discovered list; an exact ID can also be set manually for a newly available model. The backend makes the final availability check.

## Conversation behavior

Say “twenty-nine” or “hey twenty-nine”, pause for the chime, then speak, or say “twenty-nine, do this” in one utterance. Follow-up speech does not need the code unless **Wake mode** is set to require it every time (`/settings addressed true`). The default idle timeout is 120 seconds and starts after the agent finishes working and speaking. Detected speech also refreshes it. Use `--idle-seconds 0` to disable automatic sleep. When the conversation sleeps, a descending chime plays so you can hear that the wake code is required again.

| Control | Result |
| --- | --- |
| “29 stop”, “stop” while awake, or `/stop` | Cancel the task, stop playback, return to sleep |
| “go to sleep”, “go back to sleep”, “disconnect”, `/sleep`, `/disconnect` | Close voice access locally (the agent cannot sleep the microphone by talking) |
| “cancel the task”, `/cancel`, Escape | Cancel work/playback; keep the conversation open |
| `/mute`, `/unmute` | Discard/resume microphone input |
| `/approve N`, `/deny N` | Answer one pending permission request by typing |
| `/status`, `/help` | Inspect state or controls |
| `/quit`, Ctrl+C | Stop Accessor and its agent connection |

The numeric code prevents some accidental activations; it is not authentication. “Hey 29”, “hi 29”, and “ok 29” are accepted as well as “29”. Audio capture remains open during `/mute`, but samples are discarded. Quitting releases the device. Barge-ins are enabled by default: the microphone stays live during synthesis and playback. A recognized follow-up stops queued speech and interrupts the current agent turn; the replacement request starts after cancellation completes. In **code-every-request** mode (`addressed`), barge-in also needs the wake code, so ambient speech cannot pause playback. Local WebRTC AEC3 receives the actual speaker audio to reduce echo, with a residual transcript filter as a second check. In continuous-conversation mode, sustained speech pauses playback while Canary recognizes the utterance. Recognized requests then interrupt the agent; residual self-speech resumes playback, and unconfirmed pauses expire after two seconds without new speech activity. Echo performance depends on the microphone, speakers, room, and device buffering; headphones provide the clearest separation. In `/settings`, disable barge-ins to suppress the microphone during speech and reject spoken follow-ups while an agent is busy. While the agent is working and not speaking, a quiet warble plays. Sleep plays a descending chime. Settings/credential entry still suppress capture. Typed cancellation remains available.

The activity pane shows user lines, formatted agent replies (bold, links), and live tool calls. Commentary/progress text is spoken when enabled but hidden from the pane unless `/settings chat transcript`. `/settings chat off` keeps only tools and system notices.

Canary runs on completed speech segments, after about 640 ms of silence. Utterances longer than 15 seconds are discarded. Split long requests into shorter turns. The ASR model has a shared two-thread CPU pool with spinning disabled, bounded audio buffers, and stale-audio rejection.

## Test transcription separately

```sh
acc stt test
acc stt test --file sample.wav
acc transcribe sample.wav
acc --text --agent mock
```

The live STT test explicitly displays **all** recognized speech, without an agent or wake gate. File testing accepts 16 kHz mono PCM WAV and reports model load/inference timing. `--text` never opens a microphone and is silent unless `--speak` is explicitly passed.

## Choose a voice

```sh
acc tts setup
acc tts test
```

- **System**: Windows System.Speech, macOS `say`, Linux `espeak-ng`. No model download; quality depends on installed voices.
- **Kokoro**: local neural speech, with an optional persistent Python/ONNX worker. No API key or network requests during synthesis. The model loads on the first request and stays loaded for later replies; CPU inference uses two threads with spinning disabled.
- **Cartesia**: cloud speech with a key saved in the OS credential store. Only reply text is sent to Cartesia; microphone transcription remains local. `CARTESIA_API_KEY` can supply the credential instead. API version 2026-08-14 is pinned; the default model is sonic-3.
- **Off**: display replies without speech.

Install local neural speech:

```sh
python scripts/setup_tts.py
acc tts test --provider kokoro
acc tts voices --provider kokoro
acc config set tts.provider kokoro
acc config set tts.local-voice af_heart
```

The helper creates an isolated environment under runtime/kokoro and verifies the approximately 325 MB model plus 28 MB voice file by SHA-256. It supports Python 3.10–3.13 where upstream wheels are available. Initial loading can take several seconds. Performance on an old laptop needs measurement; no real-time guarantee is implied. [Piper](https://github.com/OHF-Voice/piper1-gpl) is a possible lighter future adapter, not bundled here.

For Cartesia, choose it in `acc tts setup`; the password prompt hides the key. `acc tts voices --provider cartesia` lists up to 100 voices. `acc tts forget-key` deletes the saved key. Linux credential storage needs a running Secret Service such as GNOME Keyring; there is no plaintext fallback.

Export a sample without playing it:

```sh
acc tts test "Hello, I'm Accessor." --provider kokoro --output sample.wav
```

The output must be a new file. Markdown, link destinations, and code blocks are removed from spoken text. Completed agent commentary is spoken as soon as it arrives, before the final response. Progress and final responses share an ordered speech queue. Disable progress speech in `/settings` if preferred. Tool output and private reasoning are not read aloud. `/tts speed 1.1` (range 0.6–1.5) applies to system voices, Kokoro, and Cartesia.

## Reuse agent connectors

Accessor does not implement a second Gmail client, OAuth store, or connector SDK. Configure Gmail, Google Calendar, Drive, Slack, GitHub, and other tools in the agent itself:

```sh
acc connectors setup
acc connectors list
acc connectors status
acc agent mcp list
acc agent mcp add --help
acc agent mcp login SERVER_NAME
```

`connectors setup` opens Codex; use `/plugins` to install/connect integrations. `connectors list` delegates to its plugin listing. `connectors status` asks its App Server for accessible apps on the first catalog page; this reports account availability, not a completed tool invocation. Restart Accessor after changing integrations. [Official plugin documentation](https://learn.chatgpt.com/docs/plugins) and [MCP configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

Accessor uses the same local Codex configuration and login. Plugins limited to the desktop UI may not work in a standalone App Server. Custom MCP setup remains provider-specific. A Gmail tool connection permits on-demand email tasks; it does not by itself deliver incoming-email notifications.

## Incoming replies through an event trigger

This release provides the **local handoff** for an existing email automation. It never polls Gmail or invokes an LLM just to check for mail. Configure the reply address and explicitly enable event handling:

```sh
acc events setup
acc --events
```

Your trusted automation must detect an authenticated reply and run this command on the laptop under the same OS account and ACC_HOME:

```sh
acc events emit --thread-id GMAIL_THREAD_ID --message-id GMAIL_MESSAGE_ID
```

Use Gmail API IDs, not an email address or the RFC Message-ID header. Both values are validated; no raw email text or shell command is accepted. Do not paste untrusted message content into a shell command. A remote automation needs an authenticated way to invoke the local command. Accessor opens no HTTP listener and stores no Gmail credentials.

Notifications persist as metadata in the user configuration directory. One process can consume the queue at a time. While idle, it checks this local queue once per second (no provider requests). An event starts an agent turn even while voice access is asleep; it does not open the voice conversation or read the response aloud. It asks the agent to use its Gmail connector, verify the configured owner and reply context, and respond in that thread. It instructs the agent to limit email-originated work to email replies and seek local authorization for machine actions. These instructions are not an independent security sandbox; the external trigger must authenticate the source, and the agent/connector must enforce its own permissions.

Duplicate message IDs are suppressed across restarts. A receipt is written before dispatch, so crashes and uncertain sends are not blindly retried. Receipts report agent-turn completion, not proof of email delivery. Inspect Gmail and receipts before retrying an uncertain action. `acc events status` shows the queue location/counts. `/mute` mutes microphone input; it does not disable the explicitly enabled event consumer. Quit Accessor to stop both.

**The upstream email automation is not installed or configured by this repository.** Connect that source to the local handoff before expecting replies to wake Accessor. No live email has been sent as part of development/testing.

## Agent permissions and scope

The default Codex sandbox is read-only. `--workspace-write --workspace PATH` permits workspace edits under Codex’s policy. Structured command/file approvals and empty-form MCP confirmations require a typed `/approve N` and expire after 60 seconds. No spoken transcript can approve. Authentication/device-verification requests, forms requiring field values, unknown protocol requests, and permission-profile grants are declined rather than guessed; complete those in the agent’s native interface.

The agent’s sandbox and connector permissions remain the enforcement boundaries. Read-only filesystem access does not imply read-only Gmail tools or prevent reading private files. Accessor is not a sandbox around arbitrary tools. Use appropriate OS-account and agent permissions for the device.

One agent connection owns the conversation and can use its own tools/delegation. Accessor handles the microphone, wake/sleep, display, speech, approvals, and external event handoff. A second supervisory LLM is unnecessary for this layer. Ambient audio and transcripts are held in bounded memory and not saved by Accessor; activated requests are passed to the agent, whose logging/retention rules are separate. There are no automatic retries of agent actions after a connection failure.

## Platform and development status

Windows x64 is the tested development platform. Linux x64/ARM64, Apple Silicon macOS, and Windows ARM64 use cross-platform libraries and have runtime download entries, but require testing on actual hardware. Intel macOS needs a separately supplied compatible ONNX Runtime 1.24+ build; 32-bit machines are not supported by the provided setup. Kokoro wheel availability is a separate platform constraint.

Linux compilation needs ALSA headers, pkg-config, and D-Bus development headers (`libasound2-dev libdbus-1-dev pkg-config` on Debian/Ubuntu). Windows needs Rust MSVC/C++ build tools; macOS needs Xcode command-line tools. System TTS on Linux needs espeak-ng. A physical light, startup/service packaging, and additional agent adapters remain future work. Echo cancellation and spoken interruption are implemented; physical laptop testing is still needed.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked --bin acc
python tests/smoke.py
```

The offline tests cover wake boundaries, interruption and replacement of a busy turn, disabling barge-ins, speech queuing for early progress, model discovery/selection and spoken switching, live wake-policy changes, synthetic echo attenuation, stop/sleep, active work delaying timeout, typed permissions, denied unsupported requests, saved settings, and event deduplication. Real-model WAV tests are separate from microphone/noise and long-running power measurements. Cartesia needs a user-provided key for a live test. See [THIRD_PARTY.md](THIRD_PARTY.md) for attribution.

### Measured on this Windows machine

- 23 Rust tests and 18 offline process tests passed; formatting and Clippy checks passed.
- The full-screen dashboard was exercised in a Windows terminal: activation, reply, stop, and terminal restoration worked.
- Canary: the 2.95-second synthetic WAV transcribed correctly in 0.24 seconds after a 0.98-second load.
- Kokoro fp32: a 3.47-second sample took about 5.1 seconds including initial load, then approximately 1.8 seconds with the model loaded (two CPU threads). Other simultaneous work increased these times considerably.
- Codex reported Gmail, Calendar, Drive, GitHub, and other apps as accessible. This was metadata inspection; no live email send/receive test was performed.

Live microphone/noise behavior, acoustic quality on the target laptop, Cartesia requests, an external email trigger, and Linux/macOS hardware remain unverified.
