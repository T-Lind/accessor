# Voice evaluation experiments

These helpers evaluate candidates without changing live recognition or the
selected synthesis engine. They do not open a microphone or upload audio. The
wake helper calls `acc stt wake`, which can download missing local models through
its ordinary first-run setup; prepare assets before offline evaluation. Turn and
Piper helpers require separately supplied models/environments. Use recordings you deliberately collected; keep private audio outside Git.

## Wake detection

`acc config set wake-attention true` requires an attention word before the numeric
code, including during playback wake probes. Try it before replacing the local
Canary detector. It reduces opportunities for an ordinary spoken number to wake
Accessor; it is not authentication or a measured false-wake improvement.

Create non-overlapping 16 kHz mono PCM16 windows, at most eight seconds each:

```json
[
  {"file":"quiet-wake.wav","wake":true,"condition":"quiet-near"},
  {"file":"music-wake.wav","wake":true,"condition":"music","playback":true},
  {"file":"television-negative.wav","wake":false,"condition":"television"}
]
```

```sh
python3 scripts/evaluate_wake.py corpus/manifest.json --acc target/release/acc \
  --output /tmp/wake-report.json
```

The report includes wake recall, false detections per negative corpus hour,
condition errors, inference median/P95, and audio hashes. Duplicate files are
rejected to avoid inflating the exposure. `--candidate candidate.json` compares
previously collected candidate results on the same files/order using the
`acc stt wake --json` results shape. This is offline comparison, not a live shadow
detector. The helper reports inference timing, not end-to-end or acoustic latency.

Collect separate conditions for fan/TV/music, room distance, the user's accent,
near-miss numbers, assistant speech, and speech while assistant/music output is
playing. Short negative recordings cannot establish a room false-accept rate.
Measure idle CPU, power and long negative exposure separately on the target
machine. Keep the existing `scripts/test_room_wake.py` procedure for deliberate
room recordings and do not infer real-room accuracy from synthetic clean clips.

For candidate research, [sherpa-onnx KWS](https://github.com/k2-fsa/sherpa/blob/master/docs/source/onnx/kws/index.rst)
supports configurable keywords. [openWakeWord](https://github.com/dscripka/openWakeWord)
is another candidate; its code and pretrained-model licences differ. Neither is
enabled by this change.

## Semantic turn endpoints

Draft mode (“let me explain … send that”) is the implemented way to prevent
hesitations and corrections from submitting a partial request. The ordinary
silence cutoff remains configurable with `stt.endpoint-ms`.

`scripts/evaluate_turns.py` evaluates a separately provided
[Smart Turn v3 ONNX model](https://github.com/pipecat-ai/smart-turn) on labelled
utterances. Its preprocessing follows the upstream eight-second audio tail,
left padding and normalized Whisper features. Use a separate Python environment
with `numpy`, `onnxruntime`, and `transformers`; provide the model explicitly.

```json
[
  {"file":"hesitation.wav","complete":false},
  {"file":"finished-request.wav","complete":true}
]
```

```sh
python3 scripts/evaluate_turns.py corpus/turns.json --model /path/to/model.onnx \
  --threads 1 --output /tmp/turn-report.json
```

The report includes model hash, load time, feature-plus-inference time and counts
of premature/delayed endpoints. Include silence, background noise, corrections,
backchannels, accents and deliberately unfinished phrases. Verify preprocessing
against the exact model revision and benchmark on the old/slowest supported CPU
before integrating it into the capture loop. Upstream speed claims are not
Accessor measurements. No semantic endpoint model is loaded during normal use.

## Piper migration

The pinned legacy binary remains the default. The
[OHF-Voice successor](https://github.com/OHF-Voice/piper1-gpl) provides a chunked
Python API and [libpiper C API](https://github.com/OHF-Voice/piper1-gpl/blob/main/libpiper/include/piper.h).
Its engine is GPL-3.0-or-later; voice licences need separate review. The legacy
persistent-WAV protocol cannot simply be substituted with the successor's CLI,
which reports generated paths differently.

If you have installed the successor in a separate environment, measure it:

```sh
python3 scripts/evaluate_piper.py --python /path/to/piper-env/bin/python \
  --voice /path/to/voice.onnx --output /tmp/piper-successor.json
acc tts benchmark --provider piper --runs 5 "The task is running."
```

Use the same text and voice for comparison. The successor helper reports engine
version, voice hash, load time, first synthesized chunk, total synthesis time and
audio length while keeping the model warm between runs. It does not play audio.
First chunk availability is not audible latency. Cancellation, buffering,
resampling, AEC references and libpiper ABI compatibility still need validation
before a live-engine migration.
