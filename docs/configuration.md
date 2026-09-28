# Configuration

Settings live in the OS user configuration directory. `acc config path` prints it
and `acc config locations` lists the whole folder.

```sh
acc config show                  # print effective settings
acc config set wake-code 29      # change one setting
acc config set idle-seconds 120
acc config set microphone "Microphone device name"
acc config export portable.json  # portable preferences only
acc config import portable.json  # merge into this profile
```

## Two files: portable and device-local

Accessor splits settings so preferences can move between machines without dragging
device details along.

| File | Holds | Travels? |
| --- | --- | --- |
| `config.json` | Wake, harnesses, TTS, routing, volumes, prompts | Yes — `config export` writes only this subset |
| `device.json` | Microphone, `assets-dir` / `codex-bin`, CPU tuning (`stt.threads`, `stt.spin`, `stt.lazy`, `stt.engine`), calibrated `stt.noise_floor_db` | No — stays with the machine |

`acc config export FILE` writes the portable subset; `acc config import FILE`
merges it into the current profile and keeps the target's device settings.
Copying the whole folder also works but carries `device.json`; delete it on the
new machine or import instead.

`ACC_HOME` selects a different settings/event directory, and `ACC_ASSETS` selects
a model/runtime directory. Credentials are **never** stored in `config.json`;
they go in the OS credential store (Cartesia, Jev/TypeSafe, journal key).

## In the dashboard

`/settings` opens a category menu (Voice, Speech, Harnesses, Display, Metaprompt,
Tests, Security, Computer). Arrow keys move, Enter opens or toggles, Esc goes up a
level. Changes persist and apply immediately. Typed `/settings KEY VALUE` still
works for scripts. The status bar always shows the live **harness · model**.

Wake, timeout, speech, and voice changes take effect immediately. Microphone,
asset path, and Codex executable changes require a restart.

## Key settings

### Conversation

| Key | Default | Meaning |
| --- | --- | --- |
| `wake-code` | `29` | The spoken wake word |
| `idle-seconds` | `120` | Seconds of idle before sleep; `0` disables auto-sleep |
| `barge-in` | on | Allow follow-ups without repeating the wake word |
| `microphone` | system default | Input device name (`acc devices`) |
| `event-owner` | unset | Reply allowlist for incoming-event answers |

### Speech recognition

| Key | Default | Meaning |
| --- | --- | --- |
| `stt.conversation` | `local` | `local` or `cartesia` after wake |
| `stt.engine` | `canary` | `canary`, `parakeet`, `whisper-tiny`, `whisper-base`, `whisper-small` |
| `stt.endpoint-ms` | `600` | Silence before a clip is decoded |
| `stt.streaming` | on | Cartesia streaming upload (only with `stt.conversation=cartesia`) |
| `stt.threads` / `stt.spin` | two / off | CPU tuning for local decoding |
| `stt.noise-gate` | on | Room-noise gate |
| `stt.noise-floor-db` | learned | Calibrated ambient floor |
| `stt.denoise` | `highpass` | 70 Hz rumble filter |

### Speech output

| Key | Default | Meaning |
| --- | --- | --- |
| `tts.provider` | `piper` | `piper`, `kokoro`, `system`, `cartesia`, `off` |
| `tts.piper-voice` | `en_GB-alan-medium` | Piper voice name |
| `tts.local-voice` | `bm_lewis` | Kokoro voice |
| `tts.model` | `sonic-3` | Cartesia model |
| `tts.voice` | Classy British Man | Cartesia voice by name or id |
| `tts.speed` | `1.1` | `0.6`–`2.5` (Cartesia clamped to 1.5) |
| `tts.volume` | `1.0` | `0`–`1.5` |
| `tts.streaming` | on | Stream Cartesia PCM while synthesizing |
| `sounds.wake` / `.sleep` / `.think` / `.alarm` / `.ready` / `.notify` | varied | Per-cue volumes, `0`–`1.5` |

### Harnesses and routing

| Key | Default | Meaning |
| --- | --- | --- |
| `routing.main` | `codex` | Main conversation harness |
| `routing.main-model` | harness light model | Model for main |
| `routing.reasoning` | `default` | `default`, `low`, `medium`, `high` |
| `routing.coding` / `model` | — | Coding role harness and model |
| `agent` / `routing.plugin-model` | follows main | Plugin role |
| `routing.plugin-use-main` | on | Inherit main for the plugin slot |
| `routing.coordinator` | on | Lightweight coordinator owns the conversation |
| `routing.fast-mode` | off | Best-effort low-latency path |
| `routing.compaction-model` | — | Model used to compact history and infer memories |

### Memory, logging, and security

| Key | Default | Meaning |
| --- | --- | --- |
| `memory.capture` | on | Capture durable facts off the hot path |
| `logging.enabled` | off | Write the master log |
| `logging.level` | `info` | `info` or `debug` |
| `logging.max-mb` | `16` | Rotate at this size |
| `logging.path` | `logs/accessor.log` | Override the log location |
| `security.spoken-unlock` | on | Allow spoken unlock |
| `security.lock-seconds` | `3600` | Absolute auto-lock deadline, `1`–`86400` |
| `computer.enabled` | off | Expose the MCP computer tool |
| `computer.max-image-dimension` | `1280` | Longest screenshot edge |

The metaprompt, master-log settings, `memory.capture`, and security settings are
deliberately **not** agent-editable over MCP. After-wake STT provider/streaming
are also user-only, so a prompt-injected agent cannot move microphone audio to the
cloud.

## Ask the agent to change a setting

An agent can call the MCP `settings_read` / `settings_update` tools for supported
preferences:

```json
{"changes":{"tts.speed":1.2,"tts.volume":0.7,"sounds.think":0.3}}
```

The patch is validated before anything is saved; the live session receives a
receipt. Harness/model/reasoning changes affect the next turn and leave the
current caller running.
