#!/usr/bin/env python3
"""Measure Accessor's production local STT wake recall with real room recordings.

This Linux helper records short 16 kHz WAV files with PipeWire and transcribes
them in one local `acc stt benchmark` process. Nothing is sent to an agent,
Cartesia, Jev, or another network service. It can compare the physical input
with a PipeWire noise-suppressed virtual source via --denoised-device.

The result measures whether the production STT model retained the wake phrase.
It does not reproduce Accessor's rolling-window timing or establish security.
"""

import argparse
from collections import defaultdict
from datetime import datetime
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import time


def words(text: str) -> list[str]:
    return re.findall(r"[a-z0-9]+", text.lower())


def contains_phrase(text: str, phrase: str) -> bool:
    heard = words(text)
    wanted = words(phrase)
    return any(heard[index : index + len(wanted)] == wanted for index in range(len(heard)))


def record(path: Path, device: str | None, seconds: float) -> None:
    command = [
        "pw-record",
        "--rate",
        "16000",
        "--channels",
        "1",
        "--format",
        "s16",
    ]
    if device:
        command.extend(["--target", device])
    command.append(str(path))
    process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        time.sleep(0.35)
        print("  Speak now.\a", flush=True)
        time.sleep(seconds)
        process.send_signal(signal.SIGINT)
        _, stderr = process.communicate(timeout=3)
    except BaseException:
        process.kill()
        process.wait()
        raise
    if process.returncode not in (0, -signal.SIGINT) or not path.is_file():
        message = stderr.decode(errors="replace").strip() if stderr else "no WAV produced"
        raise SystemExit(f"pw-record failed: {message}")


def benchmark(acc: str, paths: list[Path], engine: str | None) -> dict:
    command = [acc, "stt", "benchmark", *map(str, paths), "--runs", "1"]
    if engine:
        command.extend(["--engine", engine])
    result = subprocess.run(command, check=False, capture_output=True, text=True)
    if result.returncode:
        raise SystemExit(f"Accessor transcription failed:\n{result.stderr or result.stdout}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SystemExit(f"Accessor returned an invalid benchmark report: {error}") from error


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phrase", default="twenty nine", help="Words expected in every positive trial")
    parser.add_argument("--distances", default="1,3,5", help="Comma-separated metre labels")
    parser.add_argument("--conditions", default="quiet,background", help="Comma-separated room conditions")
    parser.add_argument("--trials", type=int, default=3, help="Recordings per distance/condition/source")
    parser.add_argument("--seconds", type=float, default=3.5, help="Length of each recording")
    parser.add_argument("--device", help="Raw PipeWire target node name or serial")
    parser.add_argument("--denoised-device", help="Optional PipeWire noise-suppressed virtual source")
    parser.add_argument("--engine", help="Accessor local STT engine; defaults to the saved setting")
    parser.add_argument("--acc", default="acc", help="Accessor executable")
    parser.add_argument("--keep-audio", type=Path, help="Keep private room recordings in this directory")
    parser.add_argument("--output", type=Path, help="JSON report path")
    args = parser.parse_args()

    if args.trials < 1 or args.trials > 20:
        parser.error("--trials must be 1-20")
    if not 1 <= args.seconds <= 30:
        parser.error("--seconds must be 1-30")
    if not words(args.phrase):
        parser.error("--phrase must contain at least one word")
    if shutil.which("pw-record") is None:
        raise SystemExit("pw-record was not found; install PipeWire tools")
    acc = shutil.which(args.acc) if os.sep not in args.acc else args.acc
    if not acc:
        raise SystemExit(f"Accessor executable not found: {args.acc}")

    distances = [value.strip() for value in args.distances.split(",") if value.strip()]
    conditions = [value.strip() for value in args.conditions.split(",") if value.strip()]
    if not distances or not conditions:
        parser.error("distances and conditions cannot be empty")
    sources = [("raw", args.device)]
    if args.denoised_device:
        sources.append(("denoised", args.denoised_device))

    stamp = datetime.now().astimezone().strftime("%Y%m%d-%H%M%S")
    output = (args.output or Path(f"room-wake-{stamp}.json")).expanduser().resolve()
    if args.keep_audio:
        audio_dir = args.keep_audio.expanduser().resolve()
        audio_dir.mkdir(parents=True, exist_ok=True)
        temporary = None
    else:
        temporary = tempfile.TemporaryDirectory(prefix="accessor-room-wake-")
        audio_dir = Path(temporary.name)

    trials = []
    print(
        f'Recording {len(sources) * len(distances) * len(conditions) * args.trials} local trials. '
        f'Say "{args.phrase}" naturally once per recording.'
    )
    print("For 'background', turn on the ordinary fan/TV/music you want to test. Ctrl+C stops safely.")
    try:
        for source, device in sources:
            for condition in conditions:
                for distance in distances:
                    for number in range(1, args.trials + 1):
                        input(
                            f"\n{source} · {condition} · {distance} m · trial {number}/{args.trials}. "
                            "Move into position, then press Enter."
                        )
                        path = audio_dir / f"{source}-{condition}-{distance}m-{number}.wav"
                        record(path, device, args.seconds)
                        trials.append(
                            {
                                "source": source,
                                "device": device or "default",
                                "condition": condition,
                                "distance_metres": distance,
                                "trial": number,
                                "file": str(path),
                            }
                        )

        report = benchmark(str(acc), [Path(item["file"]) for item in trials], args.engine)
        rows = report.get("files", [])
        if len(rows) != len(trials):
            raise SystemExit("Accessor report did not contain every recording")
        totals = defaultdict(lambda: [0, 0])
        for trial, row in zip(trials, rows):
            text = row.get("first_text", "")
            detected = contains_phrase(text, args.phrase)
            trial.update(
                {
                    "transcript": text,
                    "wake_phrase_detected": detected,
                    "decode_ms": row.get("first_decode_ms"),
                }
            )
            key = (trial["source"], trial["condition"], trial["distance_metres"])
            totals[key][0] += int(detected)
            totals[key][1] += 1

        summary = []
        print("\nWake recall")
        for (source, condition, distance), (hits, count) in sorted(totals.items()):
            rate = hits / count
            summary.append(
                {
                    "source": source,
                    "condition": condition,
                    "distance_metres": distance,
                    "hits": hits,
                    "trials": count,
                    "recall": rate,
                }
            )
            print(f"  {source:9} {condition:12} {distance:>4} m: {hits}/{count} ({rate:.0%})")

        result = {
            "created_at": datetime.now().astimezone().isoformat(),
            "phrase": args.phrase,
            "engine": report.get("engine"),
            "local_only": True,
            "summary": summary,
            "trials": trials,
            "limitations": [
                "Full-clip production STT test; it does not reproduce rolling wake-window latency.",
                "Positive recall test only; use an hours-long negative corpus to estimate false accepts.",
                "Transcripts and retained WAV files may contain nearby speech.",
            ],
        }
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(f"Report: {output}")
        if temporary:
            print("Private WAV recordings were deleted; use --keep-audio DIR to retain them.")
    finally:
        if temporary:
            temporary.cleanup()


if __name__ == "__main__":
    main()
