# Development measurements — 2026-09-21

Machine: Windows, Intel Core i7-1260P (Windows registry). This is **not the 2012
dual-core Dell**. No CPU-affinity trick or thread limit is represented as an
emulation of that laptop. Results varied substantially between passes, so these
small samples are diagnostic, not a controlled performance certification.

The [3.1115-second fixture](../../../tests/fixtures/voice-benchmark.wav) is synthetic
Cartesia sonic-3 speech: “Twenty nine. What is on my calendar tomorrow morning?”
No room microphone was recorded. See [fixture metadata and SHA-256](fixture.json).
Stress variants use 0.1× amplitude, Gaussian noise at 10 dB SNR, or a 120 ms/0.4×
delayed copy. These are deliberately limited tests, not a room/accent corpus.

## Local transcription

Each configuration loads Canary INT8 once, records a first decode per file, then
times seven further decodes per file. The table is warm **median inference time
for the clean fixture**, excluding endpoint delay, capture, cloud, relevance and
agent generation.

| Threads / spinning | Initial pass | Serial repeat |
| --- | ---: | ---: |
| 1 / off | 425 ms | 678 ms |
| 2 / off | 301 ms | 320 ms |
| 2 / on | 288 ms | 1125 ms |
| 4 / off | 346 ms | 732 ms |

Raw reports: [initial pass](voice-benchmark.json), [serial repeat](voice-benchmark-final.json).
The initial pass's last configurations may overlap other test work. The repeat
was launched after the build completed and ran its own benchmarks serially, but
host-wide scheduling/load was not controlled. Do not infer that spinning always
helps, or that four threads beat two. Two threads with spinning disabled is the
conservative default; use the supplied script on the Dell.

All configurations retained the wake words in the clean, quiet, noise and echo
fixtures. The noisy transcript split “tomorrow” into “to morrow”; these are not
perfect-accuracy results. The configurable 600 ms endpoint saves 400 ms of
intentional silence wait compared with the previous 1000 ms setting, regardless
of CPU speed, but can split sentences when a speaker pauses. Restore 1000 ms if
that tradeoff is poor for the user.

## Synthesis and output

These use short synthetic phrases, not an agent conversation. Four uncached
synthesis runs each; first run includes worker/connection startup.

| Provider | First full synthesis | Subsequent full synthesis |
| --- | ---: | ---: |
| Windows system voice | 745 ms | 342–411 ms |
| Cartesia sonic-3 | 928 ms | 816–966 ms |
| Local Kokoro, initial | 8.21 s | 2.34–3.55 s |
| Local Kokoro, serial repeat | 13.39 s | 2.76–3.90 s |

Kokoro generated about 2.69 seconds of audio, so it was sometimes slower than real
time even here. It is not the latency-first default for the dual-core Dell. System
voices depend on the target OS and its installed voice; these Windows numbers
do not transfer to Linux/macOS.

Cartesia's first audio bytes arrived in **311–362 ms on warm requests**, whereas
the completed chunk arrived in 816–966 ms. This motivated streaming playback.
The live playback comparison used the same phrase (“This is a short streaming
playback test.”), three runs per mode, and cleared the audio cache every run:

| Mode | First playback-start flag | Two warm playback-start flags |
| --- | ---: | ---: |
| New streaming path | 1109 ms | **432, 460 ms** |
| Completed-WAV path | 979 ms | **842, 720 ms** |

The warm means differ by about **335 ms**. The cold streaming run was slower, so
there is no unconditional speedup claim. This measures Accessor's software
playback flag, not acoustic onset; the buffered flag is set before device setup,
while the streaming flag is set by the callback after prebuffering. Network and
generated audio durations also vary. All six runs completed through the actual
speaker backend. No live microphone barge-in or across-room assessment was made.

Raw reports: [system](tts-system.json), [Cartesia](tts-cartesia.json),
[Kokoro initial](tts-kokoro.json), [Kokoro repeat](tts-kokoro-final.json),
[streaming playback](tts-stream-playback.json), [buffered playback](tts-buffered-playback.json).

## Experimental dedicated keyword detector

Tested sherpa-onnx 1.12.40 and the official
`sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20` quantized model, default threshold
0.25/score 1.0, pronunciation tokens for “twenty nine.” It is **not integrated as
Accessor's production wake detector**.

| Model chunk / threads | Clean clip median computation | Detection by audio-arrival position |
| --- | ---: | ---: |
| 8 / 1 | 170 ms | 1.28 s |
| 8 / 2 | 139 ms | 1.28 s |
| 16 / 1 | 105 ms | 1.44 s |
| 16 / 2 | 97 ms | 1.44 s |

The benchmark feeds 80 ms blocks plus a 500 ms trailing pad, so computation covers
3.6115 seconds of audio; “audio-arrival position” is not the wall-clock inference
time or latency measured from the end of the wake phrase. Cold model loading was
about 1.0–1.2 seconds. The encoder is about 4.4 MiB, compared with Canary's roughly
203 MiB combined quantized encoder/decoder on disk.

The detector found the phrase in clean, quiet and delayed-echo clips, but **missed
the 10 dB noisy clip in every tested configuration**. Canary retained the wake
words there. Lower thresholds/boosting might recover misses at the cost of false
alarms; no negative corpus was tested. This is promising for CPU/memory but not
enough evidence to replace the current wake path. Raw [keyword results](keyword-benchmark.json)
and the repeatable research script are retained for follow-up room testing.

## What remains unmeasured

The Dell itself; actual across-room wake/password accuracy; false activations per
hour; accents and multiple speakers; acoustic echo during live interruption;
overnight battery/CPU/RAM; device disconnection and suspend/resume; and full
speech-to-agent-to-audible-answer P50/P95. No claim about these is inferred from
the synthetic clip benchmarks.
