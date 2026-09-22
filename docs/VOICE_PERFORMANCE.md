# Voice performance and deployment

Accessor now exposes repeatable local STT and TTS benchmarks. Results are file and
software timings, not promises for a particular room, microphone, network or CPU.
The development machine reports an Intel i7-1260P; the intended 2012 dual-core i7
must be measured separately. Limiting modern hardware to two threads does not
emulate the older CPU, memory bandwidth, instructions or thermal limits. An SSD
helps loading models, not their steady-state computation.

## Changes

- End-of-speech silence is configurable from 300–2000 ms, default **600 ms**,
  previously 1000 ms. This removes 400 ms of intentional waiting for continuous
  speech; set it back to 1000–1200 ms if thoughtful pauses split sentences. The
  playback capture hold follows the selected cutoff instead of a fixed 1200 ms.
- Local inference defaults to **two CPU threads, spinning off**. Thread count
  (1–8) and spinning are configurable and require restart. Spinning can shave
  latency while wasting idle CPU/battery; benchmark before enabling it.
- Cartesia TTS plays incoming raw PCM from its HTTP streaming endpoint by default.
  It no longer has to receive a whole sentence's audio before starting playback.
  The queue is bounded, has a 120 ms prebuffer, pauses without dropping samples
  during user capture, and feeds actual speaker samples to echo cancellation.
  Network starvation produces silence and rebuffering; incomplete audio errors
  are surfaced without automatically repeating already-spoken output. Set
  `tts.streaming false` for the previous completed-WAV, prefetched-sentence path.
  Streaming sentence requests are currently sequential, so longer replies can
  have gaps between chunks; the buffered option retains next-sentence prefetch.
- Cancellation owns and stops in-flight synthesis/prefetch jobs. Spoken-audio
  caching stays in memory and is invalidated by locking.

The full model still decodes wake speech locally. Cartesia after-wake streaming
overlaps upload with capture, but the current STT path also runs local recognition
for controls and fallback. It does **not** eliminate local compute. Relevance
filtering and the selected agent add further latency. `/analytics` separates the
measured stages; `/audio` reports wake decoder timing and capture diagnostics.

## Repeat on the Dell

Generate a non-private speech sample once, or copy the synthetic WAV used here:

```sh
acc tts test "Twenty nine. What is on my calendar tomorrow morning?" --provider cartesia --output sample.wav
acc stt benchmark sample.wav --runs 7 --engine canary
python scripts/benchmark_voice.py sample.wav --assets PATH_TO_ACCESSOR_ASSETS --output results/voice.json --runs 7
acc tts benchmark --provider system --runs 4
acc tts benchmark --provider kokoro --runs 4
acc tts benchmark --provider cartesia --runs 4
acc tts benchmark --provider cartesia --runs 3 --playback
```

Only the Cartesia commands use the service/key and incur provider usage. The file
benchmarks don't open the microphone or contact an agent. `--playback` explicitly
plays test audio; its playback-start flag is software timing, not a microphone
measurement of audible onset. The STT comparison script isolates its settings,
leaves the user's configuration unchanged, and compares one/two/four threads plus
spinning using clean, quiet, 10 dB synthetic noise and delayed-echo variants. Run
benchmarks serially with other heavy work stopped. Reports include load, first
decode, seven warm runs, median, nearest-rank P95, RTF, and exact recognized text.
An RTF below 1 means computation faster than clip duration; it says nothing by
itself about wake false positives or far-field accuracy.

The checked-in [synthetic WAV](../tests/fixtures/voice-benchmark.wav) can be used
without a Cartesia account. Raw results and fixture metadata are in
[benchmarks/2026-09-21](benchmarks/2026-09-21/RESULTS.md).

## Suggested starting setups

| Setup | Local recognition | After-wake STT | TTS | Tradeoff |
| --- | --- | --- | --- | --- |
| Responsive cloud-assisted | Canary warm, 2 threads, no spin | Cartesia streaming | Cartesia streaming | Lowest cloud result wait; awake speech uploads before relevance is known |
| Conservative cloud upload | Canary warm, 2 threads, no spin | Cartesia finished clip | Cartesia streaming | Local word check before upload, additional transcription round trip |
| Entirely local voice | Canary warm, 1–2 threads | Local | System | No speech-service key; agent/harness can still be cloud-backed |
| Local neural voice | Canary | Local | Kokoro warm | Better neural voice; competing CPU load, particularly on the old Dell |

For the Dell, start with System or Cartesia TTS and Canary kept warm. Kokoro's
cold start and CPU demand can dominate response time. The current Whisper adapter
spawns `whisper-cli` and reloads the model per utterance; a tiny model does not
automatically mean a faster always-on experience. Lazy STT frees RAM when asleep
but introduces wake-time loading. Parakeet's larger model needs measurements before
choosing it for a dual-core machine. None of these settings requires an OpenAI API
key; agent login remains the harness's own responsibility.

```sh
acc config set stt.endpoint-ms 600
acc config set stt.threads 2
acc config set stt.spin false
acc config set stt.lazy false
acc config set tts.streaming true
```

## Dedicated wake spotting

The research script `scripts/benchmark_keywords.py` tests the official sherpa-onnx
3M Chinese/English keyword model with “twenty nine,” two chunk sizes and one/two
threads. It is an experiment, not wired into production. The model accepts new
keywords through a pronunciation list, avoiding a newly trained classifier for
every numeric code. Official documentation lists a 4.4 MiB quantized encoder and
160/320 ms model chunk latency. [Model and customization documentation](https://k2-fsa.github.io/sherpa/onnx/kws/pretrained_models/index.html).

On Linux, install the isolated experiment and its checksum-pinned model, then
try the live microphone detector:

```sh
python3 scripts/setup_wakeword.py
python3 scripts/test_wakeword.py
```

Use `--seconds 30` for a bounded microphone run, `--device NAME` to select a
PipeWire input, or `--wav tests/fixtures/voice-benchmark.wav` for a repeatable
file check. A successful detection prints `DETECTED twenty_nine`. Exit status 2
means the test completed without a detection. This tool does not change saved
settings or replace Accessor's production Canary wake path.

To test the production local STT path across a real room, run:

```sh
python3 scripts/test_room_wake.py
```

The guided test records three trials at 1, 3 and 5 metres in quiet and ordinary
background noise, transcribes every clip locally in one model process, prints
wake recall, and deletes the WAV files afterward. The JSON report retains the
recognized text. Use `--keep-audio DIR` only when you deliberately want to keep
the room recordings. If PipeWire exposes an RNNoise or other noise-suppressed
virtual source, compare it with the physical/default source in the same run:

```sh
python3 scripts/test_room_wake.py --device RAW_NODE --denoised-device FILTERED_NODE
```

List candidate source names with `acc devices` or `wpctl status`. This is a
positive recall test of full-clip recognition, not the rolling wake-window
timing or an estimate of false activations per hour.

openWakeWord is another credible candidate: it supports ONNX on Windows and
custom wake models, but “twenty nine” needs a trained and validated model. Review
model licensing separately from its Apache-licensed code before distribution.
[Project documentation](https://github.com/dscripka/openWakeWord).

Neither candidate makes “29” an identity or password. A short common number can
occur on TV or in ordinary conversation. Offer “hey twenty-nine” as a more
distinct activation option and test similar numbers, accents and recordings of
the assistant itself. The current password lock remains independent of wake
technology.

## Product work worth prioritizing next

1. **Microphone calibration and a room acceptance suite.** Test 1/3/5 metres,
   facing/sideways/away, fan/TV/music, different voices, and TTS/alarm playback.
   Measure wake recall, false accepts per hour, interruptions, password success
   and median/P95 response. Synthetic noise tests cannot substitute for this.
   Consider a microphone near the user or a tested microphone array only after
   comparing the built-in mic; gain alone also amplifies noise.
2. **A separately validated small wake engine.** Keep full STT asleep until a
   confirmed keyword; preserve local password/quick-command recognition. Require
   an hours-long negative corpus, near-number confusions, and room tests before
   making it default. Avoid advertising noisy-room reliability from one WAV.
3. **Visible privacy state and recovery.** Make locked/local-listening/cloud-active
   states obvious; retain hardware mic-off as the reliable capture switch. Add
   microphone unplug/reconnect recovery and a guided setup/level check.
4. **Sensitive-action policy.** Voice passwords are replayable. A separate typed
   confirmation for sending/deleting mail, purchases and account changes is more
   useful than pretending wake codes identify household members.
5. **Packaging and soak tests.** Signed installer/update rollback, suspend/resume,
   network-loss recovery, overnight CPU/RAM/power and bounded logging checks.
   Keep one user profile for now; accounts, permissions, voice identity and shared
   speakers need a deliberate design before mapping “29” and “30” to people.

Cartesia protocol references: [streaming TTS bytes](https://docs.cartesia.ai/api-reference/tts/bytes),
[manual streaming STT](https://docs.cartesia.ai/api-reference/stt/websocket).
