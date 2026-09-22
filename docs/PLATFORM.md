# Accessor platform

Accessor is a local voice shell around **harnesses** (Codex, Claude Code, Antigravity, mock). It owns the microphone, wake/sleep, speech, the dashboard, and routing. Each harness owns tools, sandbox, and **its own conversation thread**.

## Audio overflow

`Audio buffer overflow; restart Accessor before issuing more commands` means the microphone callback filled a bounded queue faster than the DSP thread could drain it (CPU spike, a long burst, or a stall). That used to **kill capture**. Accessor now drops the overrun, resets the segmenter, and keeps listening. One occurrence is noisy, not fatal. Persistent repeats mean the machine cannot keep up — shorter turns, a quieter room, or fewer other CPU-heavy jobs.

Long speech is retained in overlapping 30-second chunks. Completed clips queue in order.

## Wake vs conversation STT

| Stage | Default | Optional |
| --- | --- | --- |
| Wake word (`29`, `hey 29`) | **Internal** local STT (`stt.engine`, default Canary) | Always local — never cloud. Speech → Local STT model opens a list (Canary, Parakeet TDT 0.6B INT8, Whisper Tiny/Base/Small Q5). Esc leaves the list with no download. Only a missing model you then pick asks to confirm size. |
| After the session is awake (GREEN) | Same local engine | **External** Cartesia **Ink-2** (`stt.conversation=cartesia`) |

`/tts provider cartesia` is **spoken output** (Sonic). `/stt provider cartesia` (aliases: `ink-2`, `external`) is **after-wake transcription**. `/stt engine parakeet` (or Speech → Local STT model) selects the on-device model. Wake spotting cannot use Ink-2.

Cartesia has two modes after GREEN. The default waits for the locally segmented utterance and a local word check, then uploads the clip; failures fall back to that local transcript. Optional `stt.streaming=true` uploads detected awake audio in ~100 ms chunks while the user speaks, before words/relevance are known. It is active only when `stt.conversation=cartesia`, and is exposed under Speech → Cartesia streaming. Local endpointing sends `finalize`; only final transcript deltas acknowledged by `flush_done` become a prompt. Interim/partial results never trigger actions. Local transcription runs alongside the upload for wake controls and fallback. Wake-addressed commands do not wait for cloud results. Streaming is cancelled when capture is suppressed, the session sleeps, or speaker playback starts. Full local clips remain available after errors or overload; no automatic cloud retry replays a partial transcript.

Both modes retain 320 ms pre-roll, use a one-second silence endpoint, and chunk long utterances at 30 seconds with overlap. Noise can be mistaken for voice, especially in streaming mode; there is no claim that a VAD proves human intent. Only local wake detection runs during sleep/output. A rejected awake transcript appears in Activity with its reason, but is excluded from agent history and persisted analytics.

**Compute saving** (`stt.lazy`, default off): the microphone and Earshot VAD always run, and the segmenter already keeps **320ms of audio before the spike**. The local STT session is the expensive part (Canary ~180MB; Parakeet INT8 ~640MB encoder). When this is on, weights load on the first ~160ms of voiced audio (usually while you are still talking) and drop ~45s after you go back to WHITE. First wake after unload can add a fraction of a second if the phrase is shorter than the load. ONNX Runtime itself stays in process; only the selected STT sessions are freed. Both Cartesia modes retain local transcription for controls and fallback.

## Harnesses

The **Main agent** owns the conversation and defaults to Codex / gpt-5.6-luna / low reasoning. Coding and difficult analysis run in isolated workers, even when they use the same harness. Worker results return to main without copying their full tool logs.

Settings → **Harnesses** lists Main agent first, then Coding agent and Compaction agent. Each opens three pages: **harness → model → reasoning**. Arrow keys and Enter work on every page; Esc goes back. All three values save together on the final page. Old `routine` configuration fields migrate to `main` without losing preferences.

**Plugins & connectors** is a preference passed to main, lower in the list. It defaults to **Use main agent**, following changes to main's harness, model and reasoning automatically. Simple connector work can run directly in main; a different plugin preference or complex task can use a worker. Selecting the same harness never creates a worker merely because a plugin is involved.

**Input relevance** defaults to Jev when an existing TypeSafe key is available, with a local fallback otherwise. Jev receives the utterance and bounded recent conversation, and assesses whether it is addressed to the assistant and warrants a response/action. Explicit wake addressing establishes the first condition but does not force a response: a contextual dismissal such as “never mind” normally needs none. “Never mind that, stop the alarm” still requests an action. Rejected speech is not added to conversation history. The classifier runs asynchronously with a two-second timeout; outages fall back to local filtering. The local filter removes obvious filler/noise, not semantic intent. No classifier is called for silence or a bare wake.

A wake code interrupts output and active work, then opens listening. Intent is interpreted afterward. “Stop the alarm” reaches the main agent intact; the main agent uses `stop_alarm` and receives a receipt. Sleeping and typed Escape or `/cancel` remain immediate local controls. A bare wake provides at least eight seconds to begin speaking, stays silent, and holds automatic result delivery until a request or idle sleep. The usual conversation timeout remains 120 seconds.

The older `routing.router` / `routing.auto-model` settings still support legacy routing with the coordinator disabled; they are no longer part of the main settings flow.

Settings → Voice: **think warble**, **wake chime**, and **sleep chime** volumes (0 silent, 1 default). The warble follows unfinished work: it pauses during speech and resumes after progress speech, remaining active through tool calls until completion. Cancellation and pending approvals silence it. Status chrome — every box outline on the page — is **green** while the conversation is open, **light blue** while the agent works, and **purple** while TTS is speaking. Activity scrolls with the **mouse wheel** as well as PgUp/PgDn.

### Caching, compaction, and usage limits

A warm process preserves a native conversation and avoids rebuilding it for every utterance. Provider prompt caching is separate: compatible requests can reuse computation for an unchanged input prefix. Keeping the same main harness/model and stable metaprompt helps; Accessor does not inject a changing timestamp into its system instructions. It cannot guarantee cache hits, expose a shared KV cache, or move cached state between vendors. A separate worker using the same provider/model may qualify for provider caching without sharing the main conversation, depending on the provider's routing and cache rules. Model changes, different tool definitions, changed prefixes, compaction, and expiration can reduce reuse. The CLIs own cache controls and billing; Accessor does not claim savings without usage telemetry. See [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching).

Compaction has two layers. Each CLI manages its native context; `/compact` (or the automatic threshold) separately summarizes Accessor-owned history through the selected Codex, Claude Code, Antigravity or Gateway provider, using its configured model and reasoning (Gateway effort support depends on the model). Explicit Local trim is available without inference. CLI compaction runs in an isolated process and cannot change main's model. It is asynchronous, preserves turns arriving while it runs, and leaves history intact on failure. Cancellation stops its process. Brief harness handoffs use a bounded local excerpt so switching does not block voice controls. Notes, schedules and run receipts persist independently.

`/limits` reports observed provider failures and local backoff. Rate errors defer new work on that harness for five minutes; quota/usage exhaustion defers it for an hour. These are conservative **local retry delays**, not verified reset times or remaining subscription quotas. The errors and backoff persist in `limits.json`; scheduled work stays pending until eligible. Failed or uncertain actions are never automatically replayed. No paid fallback, credit purchase, or account switch occurs. Harness token/cost estimates in `/analytics` remain estimates.

Codex receives per-turn reasoning. Claude Code and Antigravity receive `--effort` and `--model`. The main default effort is low; workers select low/medium/high explicitly. Claude/Antigravity cancellation terminates the current CLI process; the next main turn reconnects with an Accessor handoff. Worker cancellation and the 15-minute deadline stop the isolated worker and leave a receipt for scheduled work. Inspect incomplete external actions before retrying.

## Approvals

Codex default is **`approvalsReviewer: auto_review`** with `approvalPolicy: on-request`. The Guardian/auto-review classifier handles sandbox escalations. Typed `/approve N` remains the human override. Claude uses its permission mode; Antigravity uses sandbox mode with plan/accept-edits according to workspace access. Accessor does not pass Antigravity’s skip-all-permissions flag. Worker approvals use typed `/worker-approve N` or `/worker-deny N`; voice cannot approve them.

## Settings

Interactive dashboard: **↑/↓** move, **Enter** opens a category or toggles, **Esc** goes up/closes. The selected row shows a cyan description of what that setting does. Harness pickers list which CLIs are actually on PATH. Categories: Voice, Speech, Harnesses, Display, Tests. Typed `/settings KEY VALUE` still works for scripts.

## What you need installed

1. Rebuild: `.\install-accessor.ps1` or `cargo build --release --bin acc`
2. `acc` or `acc doctor` — downloads ONNX Runtime + the selected local STT model if missing
3. `acc tts setup` — system / Kokoro / Cartesia
4. At least one harness login: Codex (`acc agent`), optional `claude`, optional `agy`
5. Optional: Cartesia key (TTS and Ink-2), TypeSafe or AI Gateway key (Jev / compaction)
6. `acc -wakecode 29 speak`

## Wake-once conversation

Wake opens an idle follow-up window (120 seconds by default). With barge-ins enabled, clean continuations while thinking are captured normally; only audible speech/alarm interruption requires the wake code. Capture uses a one-second silence endpoint, overlapping 30-second long-speech chunks, and a FIFO of completed clips. Playback waits for capture, transcription and relevance processing, including after synthesis. Bare wake does not flush the following captured phrase. A rolling 2.4-second local recognition window checks every ~600 ms without waiting for utterance completion; inference time is additional. Say the wake code, pause, then give the request. Rolling probe text is never submitted as a prompt. `/audio` exposes check/hit/echo-rejection counts and decoding time. Sleep closes active listening and keeps the local wake detector; separate mute/unmute state is removed. AEC uses speech, thinking warble, alarm and chime playback reference audio, with residual text filtering and epoch checks. This reduces echo loops but does not identify the human speaker. Physical hardware testing is still needed.

Live MCP controls use a session-bound loopback capability and acknowledge only after the session applies sleep or stop_alarm. Detected Antigravity registration is checked on startup; Codex/Claude receive per-session MCP settings. Barge-in decoding skips long completed speaker clips, and stale wake mentions in old replies no longer veto interruption throughout subsequent speech. `/audio` exposes capture, VAD, windows, inference errors and the latest transient probe recognition.


## Quotas and MCP preferences

`/usage` shows subscription bars and resets for all three harnesses; `acc usage --json` and MCP `usage_status` expose the same fields. Codex uses its account limits protocol, Claude its experimental `get_usage` control request, and Antigravity its standalone tab-separated `/usage` report. Queries are concurrent, bounded, and send no model turn. Missing data is unavailable; expired or older-than-five-minute samples are visibly stale. Status-line JSON can be ingested with `acc usage --ingest claude|antigravity`. `/analytics` remains approximate costs and latency; `/limits` remains observed-error retry delays. See README for provider caveats and schema sources.

MCP `settings_read` lists agent-editable keys; `settings_update` validates an atomic `changes` patch. Main's live bridge applies supported preferences and returns an actual receipt. Harness changes take effect on the next turn; TTS changes on subsequent playback. Standalone/worker connections explicitly save for next launch. Settings tools do not expose credentials, approval policy, filesystem/executable paths, or arbitrary metaprompts. Plugin settings choose harness/model/reasoning and whether to follow main; they do not authorize accounts.

MCP `notes_search` and `note_read` expose the user's private Markdown notes as bounded, read-only context, and both MCP initialization plus the harness metaprompt advertise that capability. `harness_health` reports executable discovery and configured roles without treating discovery as proof of login. Organizer status includes device-local time and IANA timezone. Calendar schedules use local date/time and optionally `every_days`; they follow device-timezone changes and DST, while `delay_seconds`/`every_seconds` retain elapsed-time semantics.

`tts.volume` and `sounds.alarm` now have independent controls, alongside `sounds.think`, `sounds.wake`, `sounds.sleep` and `tts.speed`. Accepted transcripts have only a `You` entry; rejected awake input includes the ignored words and reason. Cartesia STT is the only current live streaming audio path. All current TTS choices buffer each sentence chunk and prefetch the next; provider streaming capability is not the same as Accessor streaming playback.
