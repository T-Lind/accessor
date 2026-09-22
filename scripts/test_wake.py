#!/usr/bin/env python3
"""Test production Canary wake detection with the noise gate, on WAV files.

Builds clean/quiet/noisy variants of each source clip and runs
``acc stt wake --noise-gate compare``, so you can compare the transcript and
whether the wake code matched with the gate off versus on.

Pass your own PCM WAV recording (say "twenty nine, do something"); mono or
stereo and any sample rate are accepted and converted to 16 kHz locally.

    python scripts/test_wake.py my_wake_recording.wav

With no file it uses tests/fixtures/voice-benchmark.wav, which does not contain
the wake code; that still shows the gate's effect on the transcript but cannot
show a wake match. Exit code 2 means no wake match was seen with the gate off.

The microphone is never opened and no audio leaves the machine.
"""
import argparse
import json
import os
from array import array
from pathlib import Path
import random
import shutil
import subprocess
import sys
import tempfile
import wave

ROOT = Path(__file__).resolve().parent.parent


def read_wav(path: Path):
    with wave.open(str(path)) as wav:
        channels = wav.getnchannels()
        width = wav.getsampwidth()
        rate = wav.getframerate()
        frames = wav.readframes(wav.getnframes())
    if width != 2:
        raise SystemExit(f"{path} must be 16-bit PCM; got {width * 8}-bit")
    data = array("h", frames)
    if sys.byteorder != "little":
        data.byteswap()
    if channels > 1:
        data = array(
            "h",
            [round(sum(data[i:i + channels]) / channels) for i in range(0, len(data) - channels + 1, channels)],
        )
    samples = [value / 32768.0 for value in data]
    if rate != 16000:
        samples = resample(samples, rate, 16000)
    return samples


def resample(samples, source_rate: int, target_rate: int = 16000):
    """Linear resample; quality is adequate for a wake-detection smoke test."""
    if source_rate == target_rate or not samples:
        return samples
    out_len = round(len(samples) * target_rate / source_rate)
    out = []
    for index in range(out_len):
        position = index * source_rate / target_rate
        left = int(position)
        right = min(left + 1, len(samples) - 1)
        fraction = position - left
        out.append(samples[left] * (1 - fraction) + samples[right] * fraction)
    return out


def write_wav(path: Path, samples) -> None:
    pcm = array("h", [round(max(-1.0, min(1.0, value)) * 32767) for value in samples])
    if sys.byteorder != "little":
        pcm.byteswap()
    with wave.open(str(path), "wb") as wav:
        wav.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        wav.writeframes(pcm.tobytes())


def rms(samples) -> float:
    return (sum(value * value for value in samples) / max(1, len(samples))) ** 0.5


def variants(source: Path, directory: Path, snr_db: float, quiet_gain: float):
    samples = read_wav(source)
    level = rms(samples) or 1e-4
    rng = random.Random(29)
    noise = [rng.gauss(0.0, level / (10 ** (snr_db / 20))) for _ in samples]
    clips = {
        "clean": samples,
        "quiet": [value * quiet_gain for value in samples],
        f"noise_{snr_db:g}db": [value + extra for value, extra in zip(samples, noise)],
    }
    paths = []
    for name, clip in clips.items():
        path = directory / f"{source.stem}-{name}.wav"
        write_wav(path, clip)
        paths.append(path)
    return paths


def find_binary(explicit):
    if explicit:
        return explicit
    for candidate in (ROOT / "target" / "release" / "acc.exe", ROOT / "target" / "release" / "acc"):
        if candidate.is_file():
            return candidate
    found = shutil.which("acc")
    if found:
        return Path(found)
    raise SystemExit("Build the release binary first: cargo build --release --bin acc")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("wav", type=Path, nargs="*", help="PCM WAV containing your wake phrase (any rate/channels)")
    parser.add_argument("--wake-code", default="29")
    parser.add_argument("--engine", default="canary")
    parser.add_argument("--floor-db", type=float, default=None, help="Noise floor in dBFS (default: saved setting)")
    parser.add_argument("--snr-db", type=float, default=10.0, help="Noise level for the degraded variant")
    parser.add_argument("--quiet-gain", type=float, default=0.1, help="Gain for the quiet variant")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--assets", type=Path, default=ROOT, help="Speech assets folder (models/ + runtime/)")
    parser.add_argument("--no-variants", action="store_true", help="Test the given files as-is")
    args = parser.parse_args()
    sources = args.wav or [ROOT / "tests" / "fixtures" / "voice-benchmark.wav"]
    for source in sources:
        if not source.is_file():
            raise SystemExit(f"Not found: {source}")
    binary = find_binary(args.binary)
    assets = args.assets.expanduser().resolve()
    with tempfile.TemporaryDirectory(prefix="acc-wake-") as temporary:
        temporary = Path(temporary)
        if args.no_variants:
            paths = list(sources)
        else:
            paths = []
            for source in sources:
                paths.extend(variants(source, temporary, args.snr_db, args.quiet_gain))
        home = temporary / "home"
        home.mkdir()
        command = [
            str(binary.expanduser().resolve()),
            "stt",
            "wake",
            *map(str, paths),
            "--wake-code",
            args.wake_code,
            "--engine",
            args.engine,
            "--noise-gate",
            "compare",
            "--json",
        ]
        if args.floor_db is not None:
            command += ["--floor-db", str(args.floor_db)]
        env = dict(os.environ, ACC_HOME=str(home), ACC_ASSETS=str(assets))
        print("Running: " + " ".join(command), flush=True)
        result = subprocess.run(command, env=env, capture_output=True, text=True, encoding="utf-8", timeout=1800)
        if result.returncode != 0:
            sys.stderr.write(result.stderr)
            raise SystemExit(f"acc stt wake failed ({result.returncode})")
        report = json.loads(result.stdout)
        detected_baseline = False
        print(f"\nengine={report['engine']} code={report['wake_code']!r} floor={report['floor_db']:.1f} dBFS")
        for row in report["runs"]:
            print(f"\n{Path(row['file']).name}  ({row['seconds']:.2f}s)")
            for label in ("off", "on"):
                entry = row["results"].get(label)
                if not entry:
                    continue
                mark = "WAKE" if entry["wake"] else ("probe" if entry["probe"] else "-")
                print(f"  gate {label:<3} [{mark:>5}] {entry['inference_ms']:6.0f} ms  {entry['text']!r}")
                if entry["command"]:
                    print(f"        command: {entry['command']!r}")
                if label == "off" and entry["wake"]:
                    detected_baseline = True
        if not detected_baseline:
            print("\nNo wake match with the gate off. Check the wake code and that the clip is clear.")
            raise SystemExit(2)
        print("\nBaseline wake detection worked with the gate off; compare the 'on' row above.")


if __name__ == "__main__":
    main()
