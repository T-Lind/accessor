"""Experimental sherpa-onnx keyword benchmark; no mic, training or cloud.

Run setup_wakeword.py to install sherpa-onnx and the official
sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20 model, then pass its folder. This
is research tooling, not the production wake path.
"""
import argparse
import json
from pathlib import Path
import statistics
import time
import wave

import numpy as np
import sherpa_onnx


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("model", type=Path)
    parser.add_argument("files", type=Path, nargs="+")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=7)
    args = parser.parse_args()
    # Verified against this model's en.phone lexicon; numeric 29 is spoken as words.
    keywords = args.output.parent / "benchmark-keywords.txt"
    keywords.write_text("T W EH1 N T IY0 N AY1 N @twenty_nine\n")
    reports = []
    for chunk in (8, 16):
        for threads in (1, 2):
            stem = f"epoch-13-avg-2-chunk-{chunk}-left-64"
            started = time.perf_counter()
            kws = sherpa_onnx.KeywordSpotter(tokens=str(args.model / "tokens.txt"),
                encoder=str(args.model / f"encoder-{stem}.int8.onnx"),
                decoder=str(args.model / f"decoder-{stem}.onnx"),
                joiner=str(args.model / f"joiner-{stem}.int8.onnx"),
                keywords_file=str(keywords), num_threads=threads)
            load_ms = (time.perf_counter() - started) * 1000
            rows = []
            for path in args.files:
                with wave.open(str(path)) as wav:
                    assert (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) == (1, 2, 16000)
                    samples = np.frombuffer(wav.readframes(wav.getnframes()), dtype="<i2").astype(np.float32) / 32768
                samples = np.concatenate([samples, np.zeros(8000, dtype=np.float32)])
                timings, detections = [], []
                for _ in range(args.runs):
                    stream = kws.create_stream()
                    hits = []
                    began = time.perf_counter()
                    for i in range(0, len(samples), 1280):
                        stream.accept_waveform(16000, samples[i:i+1280])
                        while kws.is_ready(stream):
                            kws.decode_stream(stream)
                            result = kws.get_result(stream)
                            if result:
                                hits.append({"keyword": result, "audio_received_seconds": min(i+1280, len(samples))/16000})
                                kws.reset_stream(stream)
                    timings.append((time.perf_counter()-began)*1000)
                    detections.append(hits)
                ms = statistics.median(timings)
                rows.append({"file": str(path), "median_ms": ms, "runs_ms": timings,
                             "rtf": ms/(len(samples)/16), "detections": detections})
            reports.append({"chunk": chunk, "threads": threads, "load_ms": load_ms, "files": rows})
            print(f"chunk={chunk} threads={threads}: " + ", ".join(f"{Path(r['file']).stem} {r['median_ms']:.1f} ms hits={len(r['detections'][0])}" for r in rows), flush=True)
    args.output.write_text(json.dumps({"model": str(args.model), "configurations": reports,
        "note": "Offline CPU compute time, not live detection latency. Audio arrival position includes 80ms feeder quantization. Synthetic clips cannot establish room false-accept or miss rates."}, indent=2))


if __name__ == "__main__":
    main()
