"""Repeatable file-only voice benchmark. Does not record a mic or use an agent.

Provide a synthetic 16 kHz mono PCM16 WAV. Writes degraded copies + JSON beside
--output. TTS benchmarks are separate: acc tts benchmark --provider PROVIDER.
"""
import argparse
from array import array
import json
import math
import os
from pathlib import Path
import platform
import random
import subprocess
import tempfile
import wave


def variants(source, directory):
    with wave.open(str(source)) as wav:
        assert (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) == (1, 2, 16000)
        data = array("h", wav.readframes(wav.getnframes()))
    if __import__("sys").byteorder != "little":
        data.byteswap()
    samples = [v / 32768 for v in data]
    rms = math.sqrt(sum(x*x for x in samples) / len(samples))
    rng = random.Random(29)
    noise = [rng.gauss(0, rms / (10 ** (10 / 20))) for _ in samples]
    clips = {
        "clean": samples,
        "quiet": [v * 0.1 for v in samples],
        "noise_10db": [v + n for v, n in zip(samples, noise)],
        # A controlled stress case, not a claim to simulate a real room.
        "echo_120ms": [v + (0.4 * samples[i-1920] if i >= 1920 else 0) for i, v in enumerate(samples)],
    }
    paths = []
    for name, clip in clips.items():
        path = directory / (name + ".wav")
        pcm = array("h", [round(max(-1, min(1, v)) * 32767) for v in clip])
        if __import__("sys").byteorder != "little":
            pcm.byteswap()
        with wave.open(str(path), "wb") as wav:
            wav.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            wav.writeframes(pcm.tobytes())
        paths.append(path)
    return paths


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("wav", type=Path)
    parser.add_argument("--binary", type=Path, default=Path("target/release/acc.exe" if os.name == "nt" else "target/release/acc"))
    parser.add_argument("--assets", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--engine", default="canary")
    parser.add_argument("--runs", type=int, default=7)
    args = parser.parse_args()
    assert 1 <= args.runs <= 100
    args.output.parent.mkdir(parents=True, exist_ok=True)
    directory = args.output.parent / (args.output.stem + "-clips")
    directory.mkdir(exist_ok=True)
    paths = variants(args.wav, directory)
    reports = []
    for threads, spin in [(1, False), (2, False), (2, True), (4, False)]:
        with tempfile.TemporaryDirectory(prefix="acc-bench-") as temporary:
            home = Path(temporary)
            (home / "config.json").write_text(json.dumps({"stt": {"threads": threads, "spin": spin}}))
            env = dict(os.environ, ACC_HOME=str(home), ACC_ASSETS=str(args.assets.resolve()))
            result = subprocess.run([str(args.binary.resolve()), "stt", "benchmark", *map(str, paths),
                                     "--engine", args.engine, "--runs", str(args.runs)],
                                    env=env, capture_output=True, text=True, encoding="utf-8", timeout=900, check=True)
            report = json.loads(result.stdout)
            reports.append(report)
            print(f"{args.engine} threads={threads} spin={spin}: " + ", ".join(f"{Path(f['file']).stem} {f['median_ms']:.0f} ms" for f in report["files"]), flush=True)
    args.output.write_text(json.dumps({"platform": platform.platform(), "processor": platform.processor(),
                                       "logical_cpus": os.cpu_count(), "source": str(args.wav),
                                       "note": "Synthetic file tests. Thread limits do not emulate a 2012 CPU; not a far-field accuracy test.",
                                       "configurations": reports}, indent=2))
    print(args.output)


if __name__ == "__main__":
    main()
