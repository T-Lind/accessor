# Changelog

Notable changes to Accessor. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); releases are tagged on
GitHub, and full notes are generated there. This file starts at 0.34.3 — see the
[releases page](https://github.com/T-Lind/accessor/releases) and `git log` for
earlier history.

## [0.43.0] - 2026-10-14

### Added

- An always-on **wake-word spotter**. A 3M-parameter sherpa-onnx zipformer
  keyword model runs beside the full Canary wake path, so the wake code wakes
  and interrupts playback faster and more reliably, at a fraction of a Canary
  probe's CPU. Local Canary recognition is never removed: it still transcribes
  every request and remains the path for noisy rooms, and a spotter hit only
  opens listening or interrupts playback. On the dual-core machine used for
  testing the small model decoded 3.1 s of audio in ~0.16 s (versus ~1.5 s for
  Canary) and fired mid-utterance.
- Settings → Speech → **Wake engine** (`auto`, `kws`, `canary`) and **Wake
  threshold**. `auto` downloads the ~4 MB model on first run; `kws` uses it only
  if already present; `canary` keeps the previous behavior. The equivalent
  settings are `stt.wake-engine` and `stt.wake-threshold`.

### Changed

- The spotter is fed echo-cancelled capture and its hits are ignored while
  already awake and idle, so a wake-sounding number does not cancel an open
  conversation. Hits that match recent speaker text are rejected as echo.
- `/audio` reports keyword-spotter hits and the last matched phrase.

## [0.42.0] - 2026-10-13

### Added

- The supplied honey bear and mint scarf mascot, rendered natively in Rust at
  its original 40-column × 18-row size. All fifteen poses retain their local
  animation loops and interruptible blink, doze, wake, and reassurance transitions.
- A companion panel beside the conversation in terminals at least 80 × 24;
  terminals at least 44 × 33 stack the full bear above the conversation.
  Small terminals keep a compact expression, and settings use the full width.
- `/mascot` accepts the supplied pose names, including `greeting`, `curious`,
  `working`, `focused`, `success`, `surprised`, `error`, `sleepy`, `sleeping`,
  and `celebrating`.

### Changed

- Reply tone is classified by the existing local openJev model even when the
  keyword heuristic finds an initial expression. Classification is bounded to
  one background job; obsolete results cannot replace a newer reply, manual
  pose, or locked session. Uncertain local decisions stay local.
- Live alarms, locks, microphone problems, approvals, listening, speech, and
  task activity take precedence over reply tone; workers use the typing pose.
- Decision models use two CPU threads and stop spinning between requests;
  control-only replies and hidden acknowledgments skip tone inference.

### Fixed

- A queued local control acknowledgment waits if a newer user request has
  started, so background feedback cannot interrupt an edit or other follow-up.

## [0.41.0] - 2026-10-13

### Added

- A local **reranker** for memory/notes search and watch-finding gating: a 22M
  cross-encoder distilled from Jev's query→document scores (held-out Pearson
  0.70, AUC 0.93), ~23 MB int8, ~10 ms per pair. `memory_search` with
  `rerank=true` prefers it and watch findings are gated locally; Jev remains the
  fallback when the model is absent.
- **Model lifecycle**: decision models load lazily and are freed after ten
  minutes idle, so an idle Accessor holds no decision-model memory while active
  turns reuse a warm session.
- `acc decider install` fetches the reranker alongside the router/tone and gate
  models (~248 MB total).

## [0.40.1] - 2026-10-13

### Changed

- On first run, when local routing or the local relevance gate is selected and
  the model is missing, Accessor fetches it in the background (~224 MB,
  checksum-verified) and reports when it finishes, so a fresh setup needs no
  manual `acc decider install`. Routing and relevance use their fallbacks until
  it completes.

## [0.40.0] - 2026-10-13

### Added

- A **local relevance gate**: a 66M DistilBERT fine-tuned on Jev's own
  addressed/response labels now makes the input-relevance decision locally when
  `routing.input-gate = local`. On the held-out split it reproduces Jev's
  accept/reject rule on 98.3% of utterances (addressed AUC 0.994, response AUC
  1.000) at ~50 ms and ~67 MB int8. The filler filter still runs first, and the
  keyword/heuristic path is the fallback when the model is absent.
- `acc decider install` now fetches the gate alongside the router/tone model.

### Notes

- The gate was distilled from Jev (labels captured while TypeSafe credits were
  active), so the setup can run fully local.

## [0.39.1] - 2026-10-13

### Fixed

- `routing.auto-model` no longer overrides an explicit `routing.router` choice:
  it now only upgrades the `keywords` heuristic to a model router, so selecting
  local routing sticks even if a TypeSafe key is added later.
- Local routing and tone run on blocking threads, so a ~150 ms decision cannot
  stall the voice/UI loop, and routing defers to keyword matching when the model
  is unsure (helping follow-ups like "do the same for the other file").

### Changed

- `acc doctor` reports whether the local decision model is installed.

## [0.39.0] - 2026-10-13

### Added

- A local decision model (openJev, ModernBERT-151M, Apache-2.0) run through
  ONNX Runtime, replacing the hosted Jev service for the two decisions it does
  well: request routing (chat / coding / plugin) and the mascot's reply tone.
  `routing.router` gains `local` and now defaults to it; `routing.input-gate`
  defaults to the local filler filter. Both remain selectable in `/settings`.
- `acc decider status|install|remove` downloads the int8 weights (~150 MB,
  checksum-verified) on first use; routing falls back to keywords while the model
  is absent.
- The shipped harness guide and docs note the local decider.

### Notes

- The relevance gate stays on the local filler filter (or Jev): the 151M model is
  not reliable zero-shot for yes/no relevance, only for choice over short labels.

## [0.38.0] - 2026-10-12

### Added

- The `ty` harness: a local, single-file Ollama agent. Select it as Main or
  Coding in `/settings` → Harnesses, or pin a conversation with
  `session.harness=ty`. Accessor drives it one-shot with NDJSON and resumes its
  saved session by id. It runs without MCP, reaching shared memory through the
  `acc memory` CLI, and its models are the local Ollama aliases (`lfm`, `2b`,
  `0.8b`, `4b`). Read-only sessions use ty's `control` mode: no workspace writes,
  but Accessor's own `acc` commands (notes, memory, status, notifications, quota)
  still run so spoken side effects work; writable sessions approve automatically
  only when auto-review is selected.
- `acc guide`: a short inventory of the local commands an agent with shell access
  can use (memory, notes, alarms/schedules, notifications, quota, settings). The
  no-MCP harnesses (ty, opencode, Cursor) are pointed at it, so spoken requests
  like "take down a note" reach `acc organizer note`.

## [0.36.0] - 2026-10-12

### Added

- A companion mascot in the top-right of the main screen with distinct faces for
  idle, listening, thinking, speaking, happy, sad, confused, alarmed and more.
  Its expression follows the live phase, the agent can steer it with a
  `[[mood:NAME]]` marker in a reply (stripped before display and speech), and
  with a Jev key replies are classified for tone off the hot path. `/mascot
  [NAME]` shows or forces the mood.
- `/mascot` command.

### Changed

- Main-screen layout refresh: the status panel shares its row with the mascot
  (hidden on small terminals), a `❯` prompt glyph, bold entry labels, and a
  highlighted command palette.
- Saying the wake code and a request in one breath during speech
  ("29 stop the music") now both interrupts and runs the request, instead of
  only opening listening.

## [0.35.0] - 2026-10-01

### Added

- `/mic` health and `/mic reconnect`, automatic microphone recovery, and live
  microphone switching with model/device initialization off the UI loop.
- `/repeat` (also spoken “repeat that”), `/copy`, and a compact `/jobs` view.
- A quieter startup and session-only input history with draft restoration, a
  50-input bound, and clearing on lock; masked credential/passphrase entry is
  excluded.

### Fixed

- Recoverable transcription, cue, and buffer failures no longer exit the app.
- Quick follow-ups during local-control feedback cannot be sent to a transport
  with cancellation still queued; receipt commentary no longer duplicates the
  local result in Activity. Interrupted user output is still preserved.
- Capture teardown cancels pending deliveries and joins capture workers before
  reconnecting, preventing overlapping pipelines and stuck speech state.
- Periodic organizer claims skip a busy file lock; schedule and notification
  polls and notification receipts run off the UI loop. Claims invalidated by a
  lock are interrupted rather than replayed after unlock.
- The master log uses a bounded background writer, reports dropped writes, and
  flushes before tailing or shutdown; rotation replaces the prior archive on
  Windows too.

### Changed

- Background compaction/memory jobs honor `--codex-bin`; mock mode retains
  context locally without a memory/compaction model. `/memory infer`
  runs as a cancellable background job.
- New profiles use Codex for compaction. Existing provider choices are retained;
  provider failures visibly fall back to a bounded local excerpt.
- Notifications announce up to five per batch with one ding per batch.

## [0.34.5] - 2026-09-29

### Changed

- Dependency upgrades: `base64` 0.23 and `tokio-tungstenite` 0.30, plus current
  GitHub Actions major versions.

### Fixed

- Cap organizer alarms and scheduled tasks at 500, so an agent cannot grow the
  store without limit.
- Drain an oversized MCP frame before resyncing, so its tail is not parsed as a
  new request.

### Security

- Compare the loopback session token in constant time.
- Mask CLI API-key entry instead of echoing it to the terminal.

### Added

- Declared the minimum supported Rust version (1.88, enforced by clippy) and
  add crate metadata (authors, readme, keywords, categories).

## [0.34.4] - 2026-09-28

### Changed

- Interrupting with the wake code ("29") while the agent is thinking or speaking
  now preserves what it had already produced — commentary, tool activity, and any
  partial reply — in the conversation history, so the next request keeps that
  context even when the harness session restarts. Harness adapters now surface
  partial output when a turn is cancelled.

## [0.34.3] - 2026-09-28

### Fixed

- The local recognizer thread no longer exits after a single transcription or
  model-load error, which could previously leave speech permanently silent
  until restart.
- The encrypted dictation journal fails closed when its key cannot be read but
  an encrypted file already exists, instead of silently generating a new key and
  orphaning existing entries.
- `limits` cooldown formatting no longer risks an unsigned underflow, and the
  startup memory-extraction timer no longer risks a panic on hosts that have
  been up for less than 15 minutes.
- The Codex adapter tolerates non-JSON stdout lines instead of aborting the
  session.
- Desktop `open_app` launches no longer inherit the MCP JSON-RPC pipe, leave
  zombies, or route resolved executables through `cmd` re-parsing.
- `save_private` now only tightens permissions on Accessor-owned directories.

### Security

- `tts.piper-voice` is validated before it is used in a filesystem path.
- The master log and its directory are created owner-only on Unix.
- Agent-editable settings no longer include after-wake STT provider/streaming,
  so a prompt-injected agent cannot move microphone audio to the cloud; change
  these in the dashboard or with `acc config set`.

### Changed

- Release publishing is gated to tag pushes, so a manual `workflow_dispatch`
  run cannot attach assets to a release.
- CI runs the declared runtime, memory, and security regression suites.
- The Windows installer normalizes a `v`-prefixed `-Version`.

### Documentation

- Added issue and pull-request templates, `CONTRIBUTING.md`, Dependabot
  configuration, and a GitHub Pages site for the guides under `docs/`.
