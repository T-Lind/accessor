# Piper synthesis — 2026-09-27

First measurement of the **Piper** provider, which is now the default local voice
on new profiles. Earlier TTS numbers (system, Cartesia, Kokoro) are in
[../2026-09-21/RESULTS.md](../2026-09-21/RESULTS.md).

Machine: Intel Core i7-3520M (Ivy Bridge, 2 cores / 4 threads), Ubuntu 24.04,
release build, persistent Piper worker, no playback. This is a 2012 dual-core
laptop CPU — the same class as the intended Dell target, but **not** the Dell
itself and not a controlled benchmark. See [machine.json](machine.json) and the
raw [tts-piper.json](tts-piper.json).

```sh
acc tts benchmark --provider piper --runs 5
```

Text: “I'm ready to help. What would you like to do?” (~2.9 s of audio per run).
Runs are uncached; 5 runs, no playback.

| Run | Audio (s) | Synthesis (ms) | RTF |
| --- | ---: | ---: | ---: |
| 1 (cold, worker start) | 2.86 | 970.0 | 0.339 |
| 2 (warm) | 2.85 | 391.1 | 0.137 |
| 3 (warm) | 2.97 | 427.8 | 0.144 |
| 4 (warm) | 2.99 | 410.1 | 0.137 |
| 5 (warm) | 2.88 | 442.9 | 0.154 |

- Warm median synthesis: **418.9 ms**; nearest-rank warm P95: **442.9 ms**.
- Warm median RTF: **0.141** (well under real time).
- Cold first run: **970.0 ms**, dominated by worker startup.

These are software synthesis timings, not audible onset, and no microphone or
playback path was exercised. Piper is local and keyless, so unlike Cartesia the
numbers do not depend on network latency. The 2026-09-21 Kokoro run on the
i7-1260P was several times slower; Piper here is comfortably faster than real
time even on a 2012 dual-core.

## Still unmeasured

Piper on the exact Dell, the i7-1260P development machine, macOS/ARM, and the
speech-to-agent-to-audible-answer path with playback and echo cancellation.
Repeat the command above on the target machine and replace this file with the
target numbers rather than extrapolating from this one.
