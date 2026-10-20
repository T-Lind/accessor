#!/usr/bin/env python3
"""Opt-in CPU Smart Turn experiment on labelled WAVs; never changes live endpointing.

Install numpy, onnxruntime and transformers in a separate environment, then
provide an upstream Smart Turn v3 ONNX model and [{"file":"pause.wav",
"complete":false}, ...]. No model/audio is downloaded or uploaded by this script.
Uses the upstream WhisperFeatureExtractor(chunk_length=8) preprocessing.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import time
import wave


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("manifest", type=Path)
    p.add_argument("--model", type=Path, required=True)
    p.add_argument("--threads", type=int, default=1, choices=range(1, 9))
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    import numpy as np
    import onnxruntime as ort
    from transformers import WhisperFeatureExtractor
    options = ort.SessionOptions()
    options.intra_op_num_threads = args.threads
    options.inter_op_num_threads = 1
    options.add_session_config_entry("session.intra_op.allow_spinning", "0")
    options.add_session_config_entry("session.inter_op.allow_spinning", "0")
    started = time.monotonic()
    session = ort.InferenceSession(str(args.model), sess_options=options, providers=["CPUExecutionProvider"])
    load_ms = (time.monotonic() - started) * 1000
    extractor = WhisperFeatureExtractor(chunk_length=8)
    manifest = json.loads(args.manifest.read_text())
    if not isinstance(manifest, list) or not 1 <= len(manifest) <= 2000:
        p.error("Provide 1–2000 labelled utterances")
    rows = []
    for item in manifest:
        if type(item.get("complete")) is not bool:
            p.error("Every utterance needs a boolean complete label")
        with wave.open(str(args.manifest.parent / item["file"])) as wav:
            if (wav.getframerate(), wav.getnchannels(), wav.getsampwidth()) != (16000, 1, 2):
                p.error("Use 16 kHz mono PCM16 WAVs")
            if wav.getnframes() > 16000 * 120:
                p.error("Utterances must be at most two minutes")
            audio = np.frombuffer(wav.readframes(wav.getnframes()), dtype="<i2").astype(np.float32) / 32768
        audio = audio[-8*16000:]
        # Upstream pads on the left before feature extraction.
        audio = np.pad(audio, (max(0, 8*16000-len(audio)), 0))
        began = time.monotonic()
        features = extractor(audio, sampling_rate=16000, return_tensors="np", padding="max_length",
                             max_length=8*16000, truncation=True, do_normalize=True).input_features.astype(np.float32)
        probability = float(session.run(None, {"input_features": features})[0].reshape(-1)[0])
        rows.append({**item, "probability": probability, "predicted_complete": probability > .5,
                     "feature_and_inference_ms": (time.monotonic()-began)*1000})
    latencies = [r["feature_and_inference_ms"] for r in rows]
    report = {"model_sha256": hashlib.sha256(args.model.read_bytes()).hexdigest(), "threads": args.threads,
              "load_ms": load_ms, "median_ms": statistics.median(latencies), "rows": rows,
              "premature_endpoints": sum(r["predicted_complete"] and not r["complete"] for r in rows),
              "delayed_endpoints": sum(not r["predicted_complete"] and r["complete"] for r in rows),
              "note": "Offline experiment including feature extraction; not integrated into capture. Validate hesitation, correction, silence, noise, accents and total response latency before enabling."}
    args.output.write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps({k:v for k,v in report.items() if k!="rows"}, indent=2))


if __name__ == "__main__":
    main()
