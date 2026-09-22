# Accessor

Say “twenty-nine”, hear a gentle chime, and talk to your agent. Accessor keeps speech recognition local, manages the conversation and terminal display, and reuses the agent’s tools and account connections.

The stack is Rust/Tokio, Clap, Ratatui, CPAL, Earshot voice detection, and Canary 180M Flash INT8 via [transcribe-rs](https://github.com/cjpais/transcribe-rs), the transcription library behind [Handy](https://github.com/cjpais/Handy). Codex App Server, Claude Code, and Antigravity (`agy`) are supported harnesses; mock mode needs no account. Platform notes (history, Jev routing, Ink-2, overflow) live in [docs/PLATFORM.md](docs/PLATFORM.md).

See [local password locking](docs/SECURITY.md) and [voice performance, benchmarks, and deployment priorities](docs/VOICE_PERFORMANCE.md) for the latest security and latency work.

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

## A CLI for main use

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
| `/connectors setup` | Open the configured plugin harness for native plugin/MCP setup; exit it to return |
| `/agent` | Open Codex's native UI directly; exit it to return |
| `/update` | Check Codex / Claude Code / Antigravity and apply CLI updates |
| `/config locations` | Show the settings folder to copy to another machine |
| `/events` | Inspect the local event queue |

Wake, timeout, speech, and voice changes take effect immediately. Microphone, asset path, and Codex executable changes require restart. If speech/model initialization fails, the dashboard remains open for typed messages and setup. Page Up/Down scroll the conversation; Escape cancels a task. Use `--plain` for ordinary terminal output; pipes automatically use plain mode. Plain status lines print only when status changes. The dashboard is not saved as a transcript log.

## Settings and spoken switching

Open `/settings` for a category menu (Voice, Speech, Harnesses, Display, Tests). **↑/↓** moves, **Enter** opens or toggles, **Esc** goes up one level. Typed `/settings KEY VALUE` still works for scripts. Changes persist and apply immediately. The status bar always shows the live **harness · model** and a short identity code (`X S` Codex Sol, `C F` Claude Fable, `A F` Antigravity Flash).

**Harnesses:** **Main** is the persistent main conversation, with a lightweight model by default (Codex Luna, Claude Haiku, Antigravity Flash). **Coding** and **Plugin** select separate worker roles and independent models. The metaprompt tells the main agent to delegate coding, difficult analysis, and plugin work, even when all roles use the same CLI. A worker has a fresh process, its own model and low/medium/high reasoning, a 15-minute deadline, and a bounded result returned to the main conversation. Only one worker runs at a time. Accessor worker controls cannot recursively delegate. Existing native tools still enforce their own policies.

Settings → Harnesses → **Lightweight main conversation** is on by default (`routing.coordinator=true`). The agent selects delegation from the metaprompt; this is a routing policy, not a sandbox that prevents the main harness using its own tools. Turn it off to restore keyword/Jev routing. Jev remains an optional legacy classifier; the coordinator does not call it. Plugin delegation uses `agent` and `routing.plugin-model`, independently of the main model, even for the same harness.

Model discovery reads the selected Codex installation's account catalog without running an LLM. `/settings refresh` refreshes it; the last successful list is cached for offline display.

Once activated, say "switch to Codex" or "switch agent to Codex". Say "switch model to Astra" using a name from your available list. A running harness can emit `ACCESSOR_SWITCH harness=codex` (or JSON `accessor_switch`) to pin this conversation to another CLI without rewriting the plugin/coding/main slots. These local controls obey the same wake-code and barge-in settings as ordinary speech.

Direct equivalents: `/settings barge-in true`, `/settings idle-seconds 120`, `/settings speak-progress false`, `/settings chat activity`, `/settings tts.speed 1.1`, `/settings model MODEL_ID`. The numbered model picker and spoken commands use the discovered list; an exact ID can also be set manually for a newly available model. The backend makes the final availability check.

## Conversation behavior

Say “twenty-nine” or “hey twenty-nine”, pause for the chime, then speak, or say “twenty-nine, do this” in one utterance. The conversation stays open for **120 seconds of idle time** after the last accepted request or completed work/speech. Follow-ups while idle or thinking do not need the wake code when barge-ins are enabled. Detected speech holds the window open while it is captured and processed. Use `--idle-seconds 0` to disable automatic sleep. The assistant can also apply a structured sleep control when asked.

| Control | Result |
| --- | --- |
| “29 stop”, “stop” while awake, or `/stop` | Cancel the task, stop playback, return to sleep |
| “go to sleep”, “go back to sleep”, “disconnect”, `/sleep`, `/disconnect` | Close active listening; the local wake detector remains available |
| “cancel the task”, `/cancel`, Escape | Cancel main/worker work and playback; an open conversation remains open |
| `/sleep`, “29 go to sleep” | Close active listening; the local wake detector stays on |
| `/audio` | Show rolling wake checks, accepted hits, echo rejections and decoder time |
| `/approve N`, `/deny N` | Answer one pending permission request by typing |
| `/status`, `/help` | Inspect state or controls |
| `/quit`, Ctrl+C | Stop Accessor and its agent connection |

The numeric code reduces accidental activations; it is not authentication. “Hey 29”, “hi 29”, and “ok 29” are accepted. The voice states are awake and asleep; an independent password lock can block both voice and typed access. Sleep ends active listening and leaves the local wake detector on. Separate mute/unmute mode has been removed; use the hardware/OS mic switch or quit for microphone privacy. Asking the agent to mute or be quiet means sleep, and it must invoke the control before claiming success.

While the agent is thinking, you can continue speaking normally. Accessor keeps completed clips in order, waits for the configured silence cutoff (600 ms by default) to end a phrase, and delivers long speech in overlapping 30-second chunks. Recognition and relevance checks run in order without replacing earlier speech. New replies wait for capture and processing, including when speech synthesis has already started. A wake chime no longer resets capture and flushes the next phrase. Queues are bounded; overload is reported visibly.

During audible speech or an alarm, **say “29”, pause, then say your request**. During audible output, the recognizer decodes only short wake windows, bypassing completed long speaker clips. Rolling local wake checks inspect up to 2.4 seconds of audio at roughly 600 ms intervals, without waiting for the room to become silent. Actual recognition latency depends on the local model and computer; `/audio` reports it, alongside capture/VAD/window counts, decoder errors, capture-paused state and the latest transient wake-window text. That diagnostic text is not added to agent history or saved to disk. A rolling detection only interrupts and opens listening: mixed speaker/user text is never submitted as a request. The listening window stays silent and gives at least eight seconds to begin speaking. Raw voice activity alone does not interrupt. Escape remains immediate typed cancellation.

WebRTC AEC3 receives actual TTS playback, thinking warble, alarm and chime samples to estimate and remove acoustic echo. Residual transcript checks reject recognizable self-speech in wake probes, including wake words in recent output. Clean follow-ups are not rejected merely for repeating a word from the reply. These checks reduce loops but are not speaker identification; when the agent itself is saying the wake code, interruption may be suppressed. Headphones provide the clearest separation. Physical room/device validation is still needed. Settings and credential entry suppress capture. Disabling barge-ins suppresses the mic during speech. The thinking warble pauses for speech and approvals, resumes during unfinished work, and stops on cancellation.

The activity pane shows user lines, formatted agent replies (bold, links), and live tool calls. Commentary/progress text is spoken when enabled but hidden from the pane unless `/settings chat transcript`. `/settings chat off` keeps only tools and system notices.

Each harness keeps its own native conversation alive. When routing moves to another harness and later returns, Accessor supplies the user-and-agent turns that harness missed. Together, its native history plus that synchronized delta represent the full Accessor conversation without resending every turn repeatedly.

Local models decode completed speech segments after a configurable pause (`stt.endpoint-ms`, default 600 ms); long speech is delivered in overlapping 30-second chunks. Local recognition defaults to two CPU threads with spinning disabled; `stt.threads` and `stt.spin` tune the runtime after restart. Whisper uses the selected thread count with a low-beam decoder. Completed clips queue in order; explicit cancellation or sleep invalidates old capture. Optional Cartesia streaming overlaps upload with capture and local recognition.

## Test transcription separately

```sh
acc stt test
acc stt test --file sample.wav
acc transcribe sample.wav
acc --text --agent mock
```

The live STT test explicitly displays **all** recognized speech, without an agent or wake gate. Local STT automatically raises quiet, valid utterances into the model’s useful range. Severely clipped input cannot be reconstructed, so Accessor reports a one-time microphone-level warning with a PipeWire adjustment when appropriate. File testing accepts 16 kHz mono PCM WAV and reports model load/inference timing. `--text` never opens a microphone and is silent unless `--speak` is explicitly passed.

## Choose a voice

```sh
acc tts setup
acc tts test
```

- **System**: Windows System.Speech, macOS `say`, Linux `espeak-ng`. No model download; quality depends on installed voices.
- **Kokoro**: local neural speech, with an optional persistent Python/ONNX worker. No API key or network requests during synthesis. The model loads on the first request and stays loaded for later replies; CPU inference uses two threads with spinning disabled.
- **Cartesia**: cloud speech with a key saved in the OS credential store. This output setting sends reply text to Cartesia. Microphone audio is sent only if after-wake STT is separately set to Cartesia. `CARTESIA_API_KEY` can supply the credential instead. API version 2026-08-14 is pinned; the default model is sonic-3.
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

The output must be a new file. Markdown, link destinations, and code blocks are removed from spoken text. Completed agent commentary is spoken as soon as it arrives, before the final response. Cartesia plays incoming PCM packets by default (`tts.streaming=true`); local voices and the buffered Cartesia fallback synthesize completed chunks. Progress and final responses share an ordered speech queue. Disable progress speech in `/settings` if preferred. Tool output and private reasoning are not read aloud. `/tts speed 1.1` (range 0.6–1.5) applies to system voices, Kokoro, and Cartesia.

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

`connectors setup`, `connectors list`, and `connectors status` use the configured plugin harness. For Codex, setup opens its native UI and status asks App Server for accessible apps on the first catalog page; Accessor also refreshes the installed-app runtime before accepting a live Codex request. For Antigravity, the commands use its native plugin catalog and MCP registry, both of which are inherited by Accessor sessions. Status reports configuration/runtime availability, not a completed external tool invocation. Restart Accessor after changing integrations. [Official plugin documentation](https://learn.chatgpt.com/docs/plugins) and [MCP configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

Accessor uses the same local Codex configuration and login. Plugins limited to the desktop UI may not work in a standalone App Server. Custom MCP setup remains provider-specific. A Gmail tool connection permits on-demand email tasks; it does not by itself deliver incoming-email notifications.

## Notes, alarms, sleep, and scheduled tasks

The voice metaprompt tells every supported harness about Accessor's structured local controls. You can say things such as “make a note that the filter size is 20 by 25,” “set an alarm for five minutes,” “run this every morning using Codex and Sol,” or “go to sleep.” The agent emits a strict one-line JSON directive; Accessor removes it from the reply, validates it, performs the local action, and reports what was saved. A sleep directive closes voice access after the reply and waits for the wake code again.

Notes are private Markdown files under `notes/` in the directory shown by `acc config locations`. Alarms and scheduled tasks persist in `schedules.json`. An alarm repeats a two-beep cue until you say “29 stop” or type `/stop`. Alarms and tasks are checked once per second only while an `acc` process is running; this release does not install a background service or wake a powered-off/suspended computer.

Scheduled tasks require instructions, a first run time, a harness, an explicit model (or supported alias), and a reasoning level (default low). They can be listed, edited, paused/resumed, and deleted by any of the three main harnesses through validated local controls. The agent receives the actual local result, including task IDs or errors. Editing preserves omitted fields; changing the harness also requires a model. Repetition is an elapsed interval of at least 60 seconds, not a timezone-aware calendar recurrence.

Due tasks run in isolated workers. Known usage-limit backoff leaves them pending. A missed recurring task runs once and advances to its next future interval. Claims and completion/interruption receipts persist; a claimed task with an unknown outcome is not automatically replayed after a crash. The last 100 receipts are retained; status shows the newest ten. This is Accessor's local scheduler, independent of vendor scheduling products.

The same store has a direct CLI for inspection and scripting:

```sh
acc organizer status
acc organizer note "Filter size is 20 by 25" --title workshop
acc organizer alarm --in-seconds 300 --label tea
acc organizer task "Summarize today's project status" --in-seconds 3600 --every-seconds 86400 --harness codex --model gpt-5.6-sol
acc organizer edit ITEM_ID --in-seconds 7200 --prompt "Updated instructions" --harness claude --model sonnet --reasoning high
acc organizer edit ITEM_ID --paused true
acc organizer edit ITEM_ID --every-seconds 0
acc organizer cancel ITEM_ID
```

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

Duplicate message IDs are suppressed across restarts. A receipt is written before dispatch, so crashes and uncertain sends are not blindly retried. Receipts report agent-turn completion, not proof of email delivery. Inspect Gmail and receipts before retrying an uncertain action. `acc events status` shows the queue location/counts. `/sleep` closes active listening; it does not disable the explicitly enabled event consumer. Quit Accessor to stop both.

**The upstream email automation is not installed or configured by this repository.** Connect that source to the local handoff before expecting replies to wake Accessor. No live email has been sent as part of development/testing.

## Agent permissions and scope

The default Codex sandbox is read-only. `--workspace-write --workspace PATH` permits workspace edits under Codex’s policy. Structured command/file approvals and empty-form MCP confirmations require a typed `/approve N` and expire after 60 seconds. No spoken transcript can approve. Authentication/device-verification requests, forms requiring field values, unknown protocol requests, and permission-profile grants are declined rather than guessed; complete those in the agent’s native interface.

The agent’s sandbox and connector permissions remain the enforcement boundaries. Read-only filesystem access does not imply read-only Gmail tools or prevent reading private files. Accessor is not a sandbox around arbitrary tools. Use appropriate OS-account and agent permissions for the device.

One lightweight main agent owns the conversation; isolated workers return their results to it. Accessor handles the microphone, wake/sleep, display, speech, approvals, and external event handoff. A second supervisory LLM is unnecessary for this layer. Ambient audio and transcripts are held in bounded memory and not saved by Accessor; activated requests are passed to the agent, whose logging/retention rules are separate. There are no automatic retries of agent actions after a connection failure.

## Platform and development status

Windows x64 is the tested development platform. Linux x64/ARM64, Apple Silicon macOS, and Windows ARM64 use cross-platform libraries and have runtime download entries, but require testing on actual hardware. Intel macOS needs a separately supplied compatible ONNX Runtime 1.24+ build; 32-bit machines are not supported by the provided setup. Kokoro wheel availability is a separate platform constraint.

Linux compilation needs ALSA headers, pkg-config, and D-Bus development headers (`libasound2-dev libdbus-1-dev pkg-config` on Debian/Ubuntu). Windows needs Rust MSVC/C++ build tools; macOS needs Xcode command-line tools. System TTS on Linux needs espeak-ng. A physical light, startup/service packaging, and additional agent adapters remain future work. Echo cancellation and spoken interruption are implemented; physical laptop testing is still needed.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked --bin acc
python tests/smoke.py
python tests/runtime.py
```

The offline tests cover wake boundaries, interruption and replacement of a busy turn, disabling barge-ins, speech queuing for early progress, model discovery/selection and spoken switching, live wake-policy changes, synthetic echo attenuation, stop/sleep, active work delaying timeout, typed permissions, denied unsupported requests, saved settings, and event deduplication. Real-model WAV tests are separate from microphone/noise and long-running power measurements. Cartesia needs a user-provided key for a live test. See [THIRD_PARTY.md](THIRD_PARTY.md) for attribution.

### Measured on this Windows machine

- Historical baseline: 23 Rust tests and 18 offline process tests passed. Current regression suites also cover the coordinator, schedule editing/receipts, usage backoff, and Claude/Antigravity protocol fixtures.
- The full-screen dashboard was exercised in a Windows terminal: activation, reply, stop, and terminal restoration worked.
- Canary: the 2.95-second synthetic WAV transcribed correctly in 0.24 seconds after a 0.98-second load.
- Kokoro fp32: a 3.47-second sample took about 5.1 seconds including initial load, then approximately 1.8 seconds with the model loaded (two CPU threads). Other simultaneous work increased these times considerably.
- Codex reported Gmail, Calendar, Drive, GitHub, and other apps as accessible. This was metadata inspection; no live email send/receive test was performed.

Live microphone/noise behavior, acoustic quality on the target laptop, Cartesia requests, an external email trigger, and Linux/macOS hardware remain unverified.

### Agent settings and interruption

Settings → Harnesses now groups each agent into **harness → model → reasoning** pages. **Main agent** is first; Plugins & connectors defaults to following main and is only a connector preference. Coding and difficult work use isolated workers. The console highlights the selected setting, keeps it visible while navigating, and puts Main harness on its own status line.

Wake interruption stops output/work and waits silently for your request. Ordinary intent such as “stop the alarm” goes to main, which can invoke `stop_alarm`. Optional Jev relevance filtering considers conversation context; “never mind” normally needs no reply and is not a hardcoded stop action. Skipped speech stays out of history. Without a TypeSafe key the local fallback filters only obvious filler/noise. Cloud transcription and compaction run asynchronously so controls remain responsive.

`/compact` still works from the command line input, and now honors the selected CLI/model/reasoning. It summarizes Accessor's handoff history; each CLI still owns its native compaction. Failed compaction preserves the existing history.


### Shared memory and MCP

Accessor stores stable facts and preferences in `memory.json` alongside its settings, shared by all harnesses. Agents may save user-supported stable information automatically. Raw room speech, secrets, temporary guesses and permissions must not be saved as memories. Global scope is for cross-project preferences; project scope is tied to a canonical workspace directory. Repository instructions remain in native harness files. Memory is fallible context, never authority over current instructions.

Use `/memory` in the console to inspect shared facts. `acc memory list`, `acc memory search "query"`, `acc memory save key "fact" --source "user statement"`, and `acc memory forget key --revision N` inspect and maintain the store from any shell. Add `--scope global` for a personal preference. Corrections require the revision returned by search; new keys use revision 0. File locks and atomic replacement protect concurrent writers. Forget clears text/source and retains a key tombstone, which blocks old agents from recreating that key. It does not erase copies in provider history, backups, native memories, or previously compacted context. Do not mirror Accessor facts into those stores.

`acc mcp` serves memory, organizer, live session/settings controls, and subscription usage over stdio. Codex and Claude launched by Accessor receive a session-specific MCP connection; existing connectors remain available. Startup detects available harnesses and checks Antigravity registration automatically; `acc mcp-install` is also available for manual repair. Connections open when each harness starts. Registration preserves unrelated entries and refuses name collisions. Accessor passes the active workspace to Antigravity's server. Native harness tool permissions still apply. Main and worker prompts include the same memory policy; compaction prompts do not instruct memory retrieval or saving.

Memory search first filters global/current-project entries and ranks keyword matches locally. `acc memory search "query" --rerank` or MCP `rerank:true` optionally sends the query and at most 20 candidates to Jev using the configured TypeSafe credential. It has a three-second timeout and falls back to local ranking. Reranking orders candidates; it cannot grant permissions or change scope. It is not embedding-based semantic retrieval, so a keyword-free paraphrase can still miss a fact.

`session_control` MCP applies sleep and ringing-alarm stop through an authenticated loopback bridge to the launching Accessor session, and exposes live status. `organizer_control` also accepts sleep/stop_alarm. A receipt is returned after the UI applies the action. Session capabilities are supplied to main sessions, not isolated workers or compaction jobs; standalone memory MCP connections report that no live session is attached. Native tool permissions still apply. The old reply directives remain compatibility fallbacks. A successful MCP action must not also be emitted as a duplicate directive. Shared memory persists independently of CLI compaction and Accessor's handoff summaries.

## Speech decisions, streaming, and latency

An ignored awake input now appears in Activity as `Ignored (reason): words`, including Jev's addressed/actionable scores when available. Accepted input appears only once as `You`; there is no extra `Heard` line. The status line distinguishes hearing speech, local/cloud transcription, and relevance checking. Rejected words never enter agent history, shared memory, or analytics. Sleeping ambient speech stays hidden; `/stt-test` explicitly shows everything.

Cartesia streaming is optional and off by default. Select Cartesia for **After wake STT**, then enable **Speech → Cartesia streaming**, or run:

```sh
acc config set stt.conversation cartesia
acc config set stt.streaming true
```

Streaming uploads detected awake speech in roughly 100 ms PCM packets while you talk, including pre-roll. It starts before local words or Jev relevance are known, so subsequently ignored speech can reach Cartesia. Sleep and speaker playback use local wake detection. Accessor still owns the configurable silence endpoint: it finalizes the WebSocket and submits only the complete final transcript. Interim text never triggers actions. The complete local clip/transcript is retained as fallback; overload, disconnects, or missing finals cannot submit a partial cloud request. Wake-addressed local controls bypass waiting for the cloud result. Turning streaming off restores the finished-clip local word check before upload. See [Cartesia's manual streaming protocol](https://docs.cartesia.ai/api-reference/stt/websocket).

`/analytics` compares this run, the past day/week, and lifetime; lists provider calls and estimated costs; and shows accepted/ignored input counts, fallback/cache counts, and median/P95 timings for recognition queues, local/cloud recognition, relevance checks, and synthesis. Historical STT counts may include duplicates from older versions; new transcriptions are counted once per actual local/cloud pass. Analytics writes are batched outside the speech/UI path, and HTTP connections are reused. Costs are built-in estimates, not live quota balances or invoices, and may omit cancelled/failed calls. No speedup is promised for a particular microphone, model, or network; the measured stages show where time is going.


### Subscription usage

`/usage` opens a report with consumed-quota bars, percentage used/remaining, absolute reset times and countdowns for **Codex, Claude Code and Antigravity**. It queries all three concurrently, with a 15-second deadline per provider; it sends no model prompt. `acc usage --json` returns the same normalized observations for scripts. `acc usage --cached` avoids network queries. MCP `usage_status` returns cached readings by default; pass `{"refresh":true}` to refresh.

- **Codex:** App Server `account/rateLimits/read`, preferring `rateLimitsByLimitId` over the legacy bucket. Live account limit updates are also captured. Each primary/secondary window is labeled by its actual duration.
- **Claude Code:** the installed CLI supports a structured `get_usage` control request. This is an **experimental** interface and may change. Accessor requests initialization and usage only, without a model turn or persisted conversation. A non-subscription session can report plan limits unavailable. Unsupported versions or failed queries preserve the prior sample with a visible stale warning. As a fallback, pipe the native status-line JSON into `acc usage --ingest claude`; `rate_limits` may appear only after the first API response. Accessor does not replace your status-line configuration.
- **Antigravity:** standalone `agy -p /usage`, whose tab-separated report includes model groups, remaining percentages and reset timestamps. It is not sent as a prompt inside an active stream-JSON conversation. Status-line `quota` JSON can also be captured with `acc usage --ingest antigravity`.

Unknown values are **unavailable**, not zero. Observations older than five minutes, past their reset, or retained after a failed refresh are labeled **STALE**. A past reset never silently refills a bar. Only quota fields are stored in `quota-codex.json`, `quota-claude.json`, and `quota-antigravity.json`; raw status-line payloads, transcripts, paths and credentials are not saved. These are observations of the account signed into each CLI and may include use outside Accessor. They remain distinct from `/analytics` cost estimates and `/limits` retry delays.

Interfaces: [Codex App Server](https://learn.chatgpt.com/docs/app-server), [Claude status-line fields](https://code.claude.com/docs/en/statusline), [published Claude SDK experimental usage types](https://app.unpkg.com/@anthropic-ai/claude-agent-sdk@0.3.193/files/sdk.d.ts), [Antigravity headless commands](https://antigravity.google/docs/cli/headless/), [Antigravity status line](https://antigravity.google/docs/cli/statusline/).

### Settings through MCP

An agent can call `settings_read`, then `settings_update` with a `changes` object. For example:

```json
{"changes":{"tts.speed":1.2,"tts.volume":0.7,"sounds.think":0.3,"sounds.alarm":0.8}}
```

Supported preferences include speech provider/voice/model/speed/volume, spoken replies/progress, wake/sleep/think/alarm volumes, barge-in, idle timeout, display mode, after-wake STT/streaming/relevance, and main/coding/plugin harness/model/reasoning. Speed accepts 0.6–1.5; volumes accept 0–1.5 (0 is silent, 1 is normal). Speech gain is applied at playback, before echo-reference submission, so cached voices use the selected volume too.

The agent treats requests such as “speak more quietly” as settings changes: it reads the current settings, updates `tts.volume` through MCP, and waits for the receipt. The next spoken reply uses the new volume. Cartesia defaults to **Classy British Man** (`95856005-0332-41b0-935f-352e296aa0df`); existing profiles that still have the former built-in Skylar default migrate automatically, while explicitly selected voices are preserved.

Main uses `routing.main`, `routing.main-model`, `routing.reasoning`; coding uses `routing.coding`, `model`, `routing.coding-reasoning`; plugin preferences use `agent`, `routing.plugin-model`, `routing.plugin-reasoning`, and `routing.plugin-use-main`. Set the last one to true to inherit main. Change harness and model together, or set a model to `default`; available harnesses are included in `settings_read`. Reasoning values are default/low/medium/high. This selects a connector preference; it does not install plugins or connect/authorize accounts.

A patch is validated before any field is saved. Main's attached connection receives a receipt after applying the patch. Voice changes affect subsequent playback; harness/model/reasoning changes affect the next turn and leave the current caller running. A standalone or worker MCP connection saves preferences for the next launch, explicitly reporting that running sessions are unchanged. Credentials, executable paths, arbitrary prompts and approval policy are outside this tool's scope. No live connection failure silently falls back to an offline save.

Agents should discover and use Accessor's MCP memory tools rather than shelling out to `acc memory` from a read-only sandbox. Codex's server catalog is checked after connection; failures are surfaced in Activity. The CLI memory command is still available for direct human use. A denied shell write does not establish that MCP memory is broken.

### Which speech paths stream today?

| Provider offered by Accessor | Current Accessor behavior |
| --- | --- |
| Cartesia Ink-2 STT | Optional live PCM WebSocket upload while awake; only finalized transcripts go to the agent. |
| Canary / Parakeet TDT / Whisper STT | Local decoding of completed utterance chunks, plus rolling local wake checks. No incremental transcript stream. |
| Cartesia TTS | Incoming PCM playback through the HTTP streaming endpoint, with bounded buffering, echo references and cancellation. `tts.streaming=false` restores completed-WAV sentence prefetch. |
| Kokoro TTS | Persistent local worker, sentence-sized synthesis and prefetch; completed WAV chunks before playback. |
| System TTS | OS synthesis into a completed WAV chunk before playback. |

Cartesia STT and TTS both support streaming in Accessor. Input streaming remains opt-in; TTS streaming is enabled by default for Cartesia. Local TTS still uses completed WAV chunks. See [Cartesia streaming bytes](https://docs.cartesia.ai/api-reference/tts/bytes).


### Password and privacy controls

Use `/password` or `acc password set` to choose a passphrase, then say “29 unlock” followed by the passphrase in the same utterance. Spoken unlocking is local and enabled by default. “29 lock” or `/lock` revokes access and stops work/playback. `/unlock` provides masked keyboard entry in the dashboard. Settings → Security includes the default one-hour absolute auto-lock deadline and a keyboard-only option. Sleep and lock are separate. Read [the security boundary and recovery instructions](docs/SECURITY.md) before unattended use.

Short spoken replies are now cached only in bounded memory, cleared on locking. `acc tts clear-cache` removes WAVs from the previous disk cache. Benchmark generated speech with `acc stt benchmark sample.wav`, `acc tts benchmark --provider cartesia`, or the repeatable scripts described in [the performance guide](docs/VOICE_PERFORMANCE.md).
