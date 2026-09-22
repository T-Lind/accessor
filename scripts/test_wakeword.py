#!/usr/bin/env python3
"""Try the experimental sherpa-onnx detector on a Linux microphone or WAV file.

The default phrase is Accessor's current wake code, "twenty nine". Microphone
capture uses PipeWire's pw-record, so no extra audio Python package is needed.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import wave


MODEL_NAME = "sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20"
SAMPLE_RATE = 16000
BLOCK_SAMPLES = 1280


def default_assets() -> Path:
    configured = os.environ.get("ACC_ASSETS")
    if configured:
        return Path(configured).expanduser()
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "Accessor" / "assets"
    if os.name == "nt":
        return Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local")) / "Accessor" / "assets"
    return Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / "accessor" / "assets"


def use_installed_runtime(assets: Path) -> None:
    runtime = assets / "runtime" / "sherpa-kws"
    python = runtime / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    if python.is_file() and Path(sys.executable).resolve() != python.resolve():
        os.execv(str(python), [str(python), str(Path(__file__).resolve()), *sys.argv[1:]])


def read_wav(path: Path):
    import numpy as np

    with wave.open(str(path)) as audio:
        actual = (audio.getnchannels(), audio.getsampwidth(), audio.getframerate())
        if actual != (1, 2, SAMPLE_RATE):
            raise SystemExit(f"{path} must be 16 kHz, mono, 16-bit PCM; got {actual}")
        return np.frombuffer(audio.readframes(audio.getnframes()), dtype="<i2").astype(np.float32) / 32768.0


def make_spotter(model: Path, keyword_file: Path, args):
    import sherpa_onnx

    stem = f"epoch-13-avg-2-chunk-{args.chunk}-left-64"
    return sherpa_onnx.KeywordSpotter(
        tokens=str(model / "tokens.txt"),
        encoder=str(model / f"encoder-{stem}.int8.onnx"),
        decoder=str(model / f"decoder-{stem}.onnx"),
        joiner=str(model / f"joiner-{stem}.int8.onnx"),
        keywords_file=str(keyword_file),
        keywords_score=args.score,
        keywords_threshold=args.threshold,
        num_threads=args.threads,
    )


def feed(spotter, stream, samples, received: float) -> int:
    stream.accept_waveform(SAMPLE_RATE, samples)
    hits = 0
    while spotter.is_ready(stream):
        spotter.decode_stream(stream)
        result = spotter.get_result(stream)
        if result:
            hits += 1
            print(f"\aDETECTED {result} at {received:.2f}s", flush=True)
            spotter.reset_stream(stream)
    return hits


def test_wav(spotter, path: Path) -> int:
    import numpy as np

    samples = np.concatenate([read_wav(path), np.zeros(SAMPLE_RATE // 2, dtype=np.float32)])
    stream = spotter.create_stream()
    started = time.perf_counter()
    hits = 0
    for offset in range(0, len(samples), BLOCK_SAMPLES):
        block = samples[offset:offset + BLOCK_SAMPLES]
        hits += feed(spotter, stream, block, min(offset + len(block), len(samples)) / SAMPLE_RATE)
    elapsed = (time.perf_counter() - started) * 1000
    print(f"Finished {path}: {hits} detection(s), {elapsed:.1f} ms inference", flush=True)
    return hits


def test_microphone(spotter, target: str | None, seconds: float) -> int:
    import numpy as np

    recorder = shutil.which("pw-record")
    if recorder is None:
        raise SystemExit("pw-record was not found; install PipeWire tools or use --wav")
    command = [recorder, "--rate", str(SAMPLE_RATE), "--channels", "1", "--format", "s16"]
    if target:
        command.extend(["--target", target])
    command.append("-")
    print('Listening. Say "twenty nine"; press Ctrl+C to stop.', flush=True)
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    stream = spotter.create_stream()
    started = time.monotonic()
    hits = 0
    try:
        assert process.stdout is not None
        while seconds <= 0 or time.monotonic() - started < seconds:
            raw = process.stdout.read(BLOCK_SAMPLES * 2)
            if not raw:
                raise SystemExit("Microphone stream ended unexpectedly")
            samples = np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0
            hits += feed(spotter, stream, samples, time.monotonic() - started)
    except KeyboardInterrupt:
        print("\nStopped.", flush=True)
    finally:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
    print(f"Total detections: {hits}", flush=True)
    return hits


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", type=Path, default=default_assets())
    parser.add_argument("--model", type=Path)
    parser.add_argument("--wav", type=Path, help="Test a 16 kHz mono PCM WAV instead of the microphone")
    parser.add_argument("--device", help="PipeWire target node name or serial")
    parser.add_argument("--seconds", type=float, default=0, help="Stop mic test after N seconds (0 waits for Ctrl+C)")
    parser.add_argument("--chunk", type=int, choices=(8, 16), default=16)
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--score", type=float, default=1.0)
    parser.add_argument("--threshold", type=float, default=0.25)
    args = parser.parse_args()
    assets = args.assets.expanduser().resolve()
    use_installed_runtime(assets)
    try:
        import numpy  # noqa: F401
        import sherpa_onnx  # noqa: F401
    except ImportError as error:
        raise SystemExit("Run `python scripts/setup_wakeword.py` first") from error
    model = (args.model or assets / "models" / MODEL_NAME).expanduser().resolve()
    if not (model / "tokens.txt").is_file():
        raise SystemExit(f"Sherpa model not found at {model}; run scripts/setup_wakeword.py")
    # Pronunciation is verified against the model's en.phone lexicon.
    with tempfile.NamedTemporaryFile("w", suffix="-keywords.txt", delete=False) as keywords:
        keywords.write("T W EH1 N T IY0 N AY1 N @twenty_nine\n")
        keyword_path = Path(keywords.name)
    try:
        began = time.perf_counter()
        spotter = make_spotter(model, keyword_path, args)
        print(f"Loaded sherpa-onnx in {(time.perf_counter() - began) * 1000:.1f} ms", flush=True)
        hits = test_wav(spotter, args.wav.expanduser().resolve()) if args.wav else test_microphone(spotter, args.device, args.seconds)
    finally:
        keyword_path.unlink(missing_ok=True)
    raise SystemExit(0 if hits else 2)


if __name__ == "__main__":
    main()
