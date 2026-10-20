#!/usr/bin/env python3
"""Measure an opt-in successor Piper environment without changing Accessor's engine.

Provide a Python executable where OHF-Voice piper-tts is already installed and
an existing voice ONNX. The child loads once and reports each synthesis chunk;
no microphone, playback, download or cloud call is made. Compare with
`acc tts benchmark --provider piper --runs 5 TEXT` for the pinned engine.
"""
import argparse
import json
from pathlib import Path
import subprocess

CHILD = r'''
import hashlib, importlib.metadata, json, sys, time
from piper import PiperVoice, SynthesisConfig
start=time.monotonic();voice=PiperVoice.load(sys.argv[1]);load=(time.monotonic()-start)*1000
rows=[]
for text in json.loads(sys.stdin.read()):
    start=time.monotonic();first=None;samples=0;chunks=0;rate=None
    for chunk in voice.synthesize(text,SynthesisConfig()):
        if first is None:first=(time.monotonic()-start)*1000
        samples+=len(chunk.audio_int16_bytes)//chunk.sample_width//chunk.sample_channels
        rate=chunk.sample_rate;chunks+=1
    rows.append(dict(first_chunk_ms=first,total_ms=(time.monotonic()-start)*1000,chunks=chunks,audio_seconds=samples/rate if rate else 0))
print(json.dumps(dict(version=importlib.metadata.version('piper-tts'),voice_sha256=hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest(),load_ms=load,runs=rows)))
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--python", required=True, help="Explicit opt-in environment Python executable")
    p.add_argument("--voice", type=Path, required=True)
    p.add_argument("--runs", type=int, default=5, choices=range(1, 21))
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("text", nargs="?", default="The task is running. I will let you know when it needs your input.")
    args = p.parse_args()
    if not args.voice.is_file() or len(args.text) > 6000:
        p.error("Provide an existing voice and at most 6000 characters of text")
    result = subprocess.run([args.python, "-c", CHILD, str(args.voice.resolve())],
                            input=json.dumps([args.text]*args.runs), text=True, capture_output=True, check=True, timeout=180)
    report = json.loads(result.stdout)
    report["note"] = "Successor Python API synthesis only. First chunk arrival is not audible latency. GPL-3.0-or-later applies to the successor engine; voice licences are separate. The pinned Accessor engine remains selected. Validate cancellation, AEC references, long replies and native libpiper ABI before migration."
    args.output.write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
