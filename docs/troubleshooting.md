# Troubleshooting

Start with the built-in diagnostics. They are safe, local, and do not send audio
anywhere.

```sh
acc doctor          # install, speech files, harnesses
acc devices         # microphones
acc mic             # ambient floor, speech level, peak, SNR
acc --plain ...     # ordinary stdout when piping or scripting
```

Keep new issues off the critical path while you investigate: `--text` runs the
conversation plumbing with no microphone, and `--agent mock` needs no account.

## Install and build

**Linux build fails on `wayland-sys` / `alsa-sys` / `pipewire-sys`.**
Install the desktop build dependencies:

```sh
sudo apt-get install -y libasound2-dev libdbus-1-dev libwayland-dev \
  libxcb1-dev libxrandr-dev libxkbcommon-dev libpipewire-0.3-dev \
  libgbm-dev libdrm-dev libegl1-mesa-dev libgles2-mesa-dev pkg-config
```

**Credentials will not save on Linux.** Accessor uses the Secret Service; run a
keyring such as GNOME Keyring. There is no plaintext fallback.

**System TTS is silent on Linux.** Install `espeak-ng`, or pick Piper/Kokoro in
`/tts`.

**macOS says the app is damaged / cannot be opened.** The binary is unsigned and
unnotarized. Right-click → **Open**, or
`xattr -d com.apple.quarantine /path/to/acc`.

**Windows SmartScreen warns.** The binary is unsigned; choose More info → Run
anyway. Windows ARM is not supported by the prebuilt binaries — build from source.

**Intel macOS.** There is no bundled ONNX Runtime for `x86_64-apple-darwin`.
Build from source and set `ORT_DYLIB_PATH` to a compatible 1.24+ dylib.

## First-run downloads

**A download fails or is incomplete.** `acc doctor` and `acc setup` re-check the
speech downloads. Files are written to a `.part` file and only accepted after
their SHA-256 matches, so a bad download is discarded and retried.

**Whisper CLI setup fails.** Some platforms have no pinned `whisper-cli` build;
Parakeet remains a local upgrade. On Linux, the CLI needs its shared libraries in
`runtime/` (the setup verifies this).

**Docker/CI has no GPU or audio.** The default path is CPU-only; a headless
environment has no microphone, so use `--text --agent mock` for tests.

## Microphone and audio

**No microphone found.** Run `acc devices` and set one with `acc config set
microphone "NAME"`, then restart.

**`Audio buffer overflow; restart Accessor before issuing more commands`.**
The microphone callback filled a bounded queue faster than the DSP thread could
drain it (CPU spike, long burst, or a stall). Accessor drops the overrun, resets
the segmenter, and keeps listening; one occurrence is noisy, not fatal. Persistent
repeats mean the machine cannot keep up — shorter turns, a quieter room, or fewer
other CPU-heavy jobs.

**The agent hears itself / wake loops.** Echo cancellation needs clean output.
Prefer headphones, keep speaker volume moderate, and avoid playing audio through a
different device than the one Accessor uses. Residual checks reject recognizable
self-speech in wake probes but are not speaker identification.

**Speech is quiet or clipped.** Run `acc mic` to check level and SNR, and
`/noise calibrate` while staying quiet for three seconds. Severely clipped input
cannot be reconstructed.

**Wake is unreliable.** Try `/audio` for rolling wake checks and decoder timing,
then `acc stt wake recording.wav` to test the production matcher on a real
recording. Cloud STT and streaming are disabled while the lock is active.

## Speech models and voices

**Piper is missing.** The pinned build and default voice download on first run.
Re-run `acc setup` or `python scripts/setup_piper.py`, then `acc tts test
--provider piper`.

**Kokoro setup fails.** The helper supports Python 3.10–3.13 where upstream wheels
exist; model loading can take several seconds.

**Cartesia key is rejected.** Save it with `/tts key` (masked) or
`acc tts setup`; it lives in the OS credential store. `acc tts forget-key`
removes it. Linux needs a running Secret Service.

## Harnesses and updates

**`acc update` shows nothing to update.** It only updates the agent CLIs (Codex,
Claude Code, Antigravity); Accessor itself is updated by re-running the installer.

**A harness cannot be found.** Check `acc agent login`, set an explicit path with
`acc config set codex-bin PATH` (or `--codex-bin`), and confirm the CLI is on
`PATH`. `harness_health` reports executable discovery and cached model counts.

**Connectors do not appear.** Re-run `acc connectors refresh` and restart
Accessor. Management commands for opencode/Cursor are interactive and are read
from their own configuration instead.

## The access lock

**Locked out.** The lock is an Accessor interface boundary, not OS security. The
OS owner can reset it by stopping Accessor and removing `password.json` from the
settings folder. Failed attempts get increasing cooldowns that persist across
restarts. See [Local access lock](SECURITY.md).

**Journal will not open.** The journal key lives in the OS credential store (or
`JOURNAL_KEY`). If the store is locked, Accessor refuses to generate a new key
rather than orphan existing entries — unlock the credential store and retry.

## Logs and data locations

| What | Where |
| --- | --- |
| Settings | `acc config path` |
| Assets (models, runtime, voices) | `acc config locations` / `ACC_ASSETS` |
| Master log (optional) | `logs/accessor.log`, or `logging.path` |
| Shared memory | `memory.json` beside settings |
| Notes / schedules / notifications | `notes/`, `schedules.json`, `notifications.json` |
| Encrypted journal | `journal/journal.enc` |

Enable the master log under Settings → Display. Logs never contain credentials or
raw audio. `/logs` prints the path and the last lines.
