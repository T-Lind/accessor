#!/usr/bin/env python3
"""Compare production wake detection and optional candidate results on a private corpus.

Manifest: [{"file":"room.wav","wake":false,"condition":"music"}, ...].
Files must be non-overlapping 16 kHz mono PCM16 wake windows, at most 8 s each.
No microphone, cloud call, or default-engine change is made. Missing local
models follow acc's ordinary first-run setup; prepare assets before offline use.
Candidate JSON uses the same `runs`/`results` shape as `acc stt wake --json`.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import wave


def summarize(rows, manifest):
    modes = sorted({mode for row in rows for mode in row["results"]})
    output = {}
    for mode in modes:
        positives = hits = false_accepts = 0
        negative_seconds = 0.0
        latencies = []
        conditions = {}
        for row, expected in zip(rows, manifest):
            result = row["results"][mode]
            # Probe matching is appropriate during playback; prefix matching is
            # used for idle wake. Neither detection is treated as authorization.
            detected = bool(result["probe" if expected.get("playback") else "wake"])
            if expected["wake"]:
                positives += 1
                hits += int(detected)
            else:
                negative_seconds += expected["seconds"]
                false_accepts += int(detected)
            group = conditions.setdefault(expected.get("condition", "unspecified"), {"trials": 0, "errors": 0})
            group["trials"] += 1
            group["errors"] += int(detected != expected["wake"])
            latencies.append(float(result["inference_ms"]))
        output[mode] = {"wake_hits": hits, "positive_trials": positives,
                        "wake_recall": hits / positives if positives else None,
                        "false_accepts": false_accepts, "negative_hours": negative_seconds / 3600,
                        "false_accepts_per_hour": false_accepts * 3600 / negative_seconds if negative_seconds else None,
                        "median_inference_ms": statistics.median(latencies),
                        "p95_inference_ms": sorted(latencies)[max(0, int(len(latencies) * .95 + .999) - 1)],
                        "conditions": conditions}
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--acc", default="acc")
    parser.add_argument("--engine", default="canary")
    parser.add_argument("--candidate", type=Path, help="Previously collected candidate JSON; never executes an arbitrary command")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text())
    if not isinstance(manifest, list) or not 1 <= len(manifest) <= 2000:
        parser.error("Provide 1–2000 labelled, non-overlapping windows")
    files = []
    hashes = set()
    for item in manifest:
        if type(item.get("wake")) is not bool:
            parser.error("Every window needs a boolean wake label")
        path = (args.manifest.parent / item["file"]).resolve()
        with wave.open(str(path)) as wav:
            if (wav.getframerate(), wav.getnchannels(), wav.getsampwidth()) != (16000, 1, 2):
                parser.error("Use 16 kHz mono PCM16 WAV files")
            item["seconds"] = wav.getnframes() / 16000
        if not 0 < item["seconds"] <= 8:
            parser.error("Wake windows must be 0–8 seconds")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest in hashes:
            parser.error("Duplicate audio would inflate corpus hours")
        hashes.add(digest)
        item["sha256"] = digest
        files.append(str(path))
    rows = []
    for offset in range(0, len(files), 50):
        result = subprocess.run([args.acc, "stt", "wake", *files[offset:offset+50], "--engine", args.engine,
                                 "--noise-gate", "compare", "--json"], capture_output=True, text=True, check=True, timeout=3600)
        rows.extend(json.loads(result.stdout)["runs"])
    if len(rows) != len(manifest):
        raise SystemExit("Production report is missing corpus windows")
    report = {"engine": args.engine, "production": summarize(rows, manifest), "corpus": manifest,
              "note": "Offline window detections, not a measured live false-wake rate. Label playback/music/fan/TV/distance separately. Zero errors on a short corpus is weak evidence. Measure live idle CPU and power separately."}
    if args.candidate:
        candidate = json.loads(args.candidate.read_text())
        candidate_rows = candidate["runs"]
        if len(candidate_rows) != len(manifest) or [Path(r["file"]).resolve() for r in candidate_rows] != list(map(Path, files)):
            raise SystemExit("Candidate report must cover the same files in the same order")
        report["candidate"] = summarize(candidate_rows, manifest)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k != "corpus"}, indent=2))


if __name__ == "__main__":
    main()
