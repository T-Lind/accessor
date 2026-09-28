# Features

A tour of what Accessor does, with the trade-offs that matter. Each section links
to the deeper guide where one exists.

## Speech

Conversation recognition defaults to **local** Canary 180M Flash, and replies
default to fast local **Piper** speech, so the default path needs no key.

- **Local STT** decodes completed utterances after a configurable pause
  (`stt.endpoint-ms`, default 600 ms). Long speech is delivered in overlapping
  30-second chunks. Wake probes and dictation always stay local.
- **Cloud STT (Cartesia Ink-2)** is opt-in. When selected, detected awake speech
  is uploaded over a WebSocket while you talk, but only the **final** transcript
  is submitted — interim text never triggers actions. Wake detection and spoken
  unlocking remain local.
- **Local STT models**: Canary 180M Flash (default), Parakeet TDT 0.6B, and
  Whisper Tiny/Base/Small.
- **TTS providers**: Piper (default), System, Kokoro, Cartesia, or off.
- **Room-noise gate** removes steady ambient noise just before recognition,
  learned automatically or with `/noise calibrate`. An independent 70 Hz high-pass
  rumble filter is on by default (`stt.denoise`).

Privacy note: after an ordinary wake, opting-in cloud streaming uploads speech
before its words are known. While the password lock is active, cloud STT is
disabled and audio stays local.

See [Voice performance](VOICE_PERFORMANCE.md) for measured latency and the
[platform notes](PLATFORM.md) for the STT modes.

## Wake and conversation

Say **"twenty-nine"** (or "hey 29", "hi 29", "ok 29"), pause for the chime, then
speak. The conversation stays open for 120 seconds of idle time. While the agent
is thinking you can keep speaking; completed clips queue in order. During audible
speech, saying "29" interrupts and opens listening — the recognizer decodes only
short wake windows, and mixed speaker/user text is never submitted as a request.

The numeric word reduces accidental activations; it is not authentication. See the
[local access lock](SECURITY.md) for the real boundary.

## Harnesses and routing

Accessor drives Codex App Server, Claude Code, Antigravity (`agy`), opencode, and
Cursor (`cursor-agent`); mock mode needs no account. Routing picks the **main**,
**coding**, or **plugin** role, and a lightweight coordinator owns the main
conversation by default and delegates from its metaprompt. Coding and difficult
work runs in an isolated 15-minute worker that returns a bounded result. See
[Harnesses](harnesses.md).

Accessor reuses the agent's own connectors — Gmail, Google Calendar, Drive, Slack,
GitHub, and others — configured in the harness itself. It does not implement a
second OAuth store or connector SDK.

## Memory

Stable facts and preferences live in `memory.json`, shared by all harnesses. With
memory capture on (default), ordinary statements are captured off the speech hot
path; explicit memory commands and MCP tools always work. A bounded digest of the
highest-scoring in-scope facts is injected when a harness starts.

```sh
acc memory list
acc memory search "query"
acc memory save key "fact" --source "user statement"
acc memory forget key --revision N
```

Facts are ranked by importance, recency decay, and usage. Corrections require the
revision returned by search. Forget clears text and retains a key tombstone so old
agents cannot recreate the key.

## Notes, alarms, and scheduled tasks

Structured one-line JSON directives in a reply become local actions. Notes are
private Markdown files; alarms repeat until you say "29 stop"; scheduled tasks run
in isolated workers with a harness, model, and reasoning level.

```sh
acc organizer status
acc organizer note "Filter size is 20 by 25" --title workshop
acc organizer alarm --in-seconds 300 --label tea
acc organizer task "Summarize today's project status" --in-seconds 3600 \
  --every-seconds 86400 --harness codex --model gpt-5.6-sol
acc organizer edit ITEM_ID --paused true
acc organizer cancel ITEM_ID
```

Alarms and tasks are checked once per second only while an `acc` process is
running; this release installs no background service. Quiet hours defer due tasks
but not alarms, and skipped runs are reported.

## Watches and notifications

A **watch** is a recurring, gated survey that runs through your connectors. It may
emit only `notify` findings, each scored before it reaches you.

```
/watch every 30m [quiet] [threshold=0.7] [night=20:00-07:00] urgent email about the release
/watch list
/watch edit ID [every …] [quiet|speak] [threshold=…] [night=…]
/watch stop ID
```

Notifications are stored privately in `notifications.json` (capped at 100, newest
kept). While Accessor is idle the newest unread items appear in Activity, ding
once, and are spoken when requested. Duplicate findings from the same source are
suppressed, and a re-worded repeat is suppressed by significant-word overlap.

```sh
acc notifications list --unread
acc notifications read ID
acc notifications dismiss ID
```

## Incoming replies

This release provides a **local handoff** for an existing email automation. It
never polls Gmail or invokes an LLM just to check for mail.

```sh
acc events setup
acc --events
acc events emit --thread-id GMAIL_THREAD_ID --message-id GMAIL_MESSAGE_ID
```

Your trusted automation must detect an authenticated reply and run the emit
command under the same OS account and `ACC_HOME`. Only validated Gmail API IDs are
accepted; no raw email text or shell command is. One process consumes the queue at
a time, and duplicates are suppressed across restarts. Accessor opens no HTTP
listener and stores no Gmail credentials.

## Desktop control (computer use)

Off by default, because it drives the real machine. When enabled, an MCP
`computer` tool mirrors the standard computer-use actions (screenshot, click,
type, key, scroll, drag) **and** adds reliable semantic actions: `open_app`,
`list_windows`, `focus_window`, and an accessibility-tree `ui_snapshot` with
`ui_invoke`, `ui_set_value`, `ui_select`, and `ui_expand`.

The semantic backend is native per platform: Windows UI Automation, macOS
Accessibility via System Events, and Linux AT-SPI2 with `wmctrl` as a fallback.
Screenshots are downscaled to `computer.max-image-dimension` and click coordinates
are mapped back to native pixels. This is not a sandbox; enable it only when you
want an agent to act on this desktop.

## Local-only dictation

While awake, say **"start dictation"** (or `/dictate on`). Recognized speech is
then transcribed locally and appended to an encrypted journal; it is never sent
to an agent or cloud service. Unlike normal after-wake transcription, dictation
forces local STT. The journal is encrypted at rest with XChaCha20-Poly1305 and its
key lives in the OS credential store.

```sh
acc journal show
acc journal add "A private note"
acc journal path
```

## Permissions and scope

The default Codex sandbox is read-only. Structured command/file approvals and
empty-form MCP confirmations require a typed `/approve N` and expire after 60
seconds — no spoken transcript can approve. Authentication/device-verification
requests, forms requiring field values, and permission-profile grants are declined
rather than guessed.

The agent's sandbox and connector permissions remain the enforcement boundary.
Accessor is not a sandbox around arbitrary tools.

## Analytics and usage

`/analytics` compares this run, the past day/week, and lifetime, with provider
calls, estimated costs, accepted/ignored counts, and median/P95 timings for the
pipeline stages. Costs are built-in estimates, not invoices.

`/usage` reports subscription quota for Codex, Claude Code, and Antigravity, with
reset times and countdowns. Unknown values are reported as **unavailable**, not
zero, and stale samples are labeled. Only quota fields are stored.
