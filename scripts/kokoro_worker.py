"""Private stdio worker. No sockets, network requests, or transcript files.

Input: one JSON line. Output: JSON header followed by exactly `bytes` WAV bytes.
Model stays loaded until the parent stops the process. Two CPU threads, no spin.
"""
import io
import json
from pathlib import Path
import sys
import wave

def main():
    import numpy as np
    import onnxruntime as ort
    from kokoro_onnx import Kokoro
    root = Path(sys.argv[1])
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    options.inter_op_num_threads = 1
    options.add_session_config_entry("session.intra_op.allow_spinning", "0")
    options.add_session_config_entry("session.inter_op.allow_spinning", "0")
    session = ort.InferenceSession(str(root / "kokoro.onnx"), options, providers=["CPUExecutionProvider"])
    model = Kokoro.from_session(session, str(root / "voices.bin"))
    for line in sys.stdin:
        try:
            request = json.loads(line)
            if request.get("action") == "voices":
                header = {"ok": True, "bytes": 0, "voices": sorted(model.voices.keys())}
                data = b""
            else:
                text = request["text"]
                if not isinstance(text, str) or len(text) > 6000:
                    raise ValueError("Text must be at most 6000 characters")
                voice = request.get("voice", "af_heart")
                speed = float(request.get("speed", 1.0))
                speed = min(1.5, max(0.6, speed))
                samples, rate = model.create(text, voice=voice, speed=speed, lang="en-gb" if voice.startswith("b") else "en-us")
                buffer = io.BytesIO()
                with wave.open(buffer, "wb") as wav:
                    wav.setnchannels(1)
                    wav.setsampwidth(2)
                    wav.setframerate(rate)
                    wav.writeframes((np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes())
                data = buffer.getvalue()
                header = {"ok": True, "bytes": len(data)}
        except Exception as error:
            header = {"ok": False, "bytes": 0, "error": str(error)}
            data = b""
        sys.stdout.buffer.write(json.dumps(header).encode() + b"\n" + data)
        sys.stdout.buffer.flush()

if __name__ == "__main__":
    main()
