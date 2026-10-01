# Changelog

Notable changes to Accessor. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); releases are tagged on
GitHub, and full notes are generated there. This file starts at 0.34.3 — see the
[releases page](https://github.com/T-Lind/accessor/releases) and `git log` for
earlier history.

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
