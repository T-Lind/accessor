#!/usr/bin/env python3
"""Install optional Kokoro in an isolated environment; verified model downloads."""
import os
from pathlib import Path
import subprocess
import sys
import venv
from setup_speech import download

ROOT = Path(__file__).resolve().parent.parent
ASSETS = Path(os.environ.get("ACC_ASSETS", ROOT))
ENV = ASSETS / "runtime" / "kokoro"
MODEL = ASSETS / "models" / "kokoro"
BASE = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.1"

def main():
    if not (3, 10) <= sys.version_info[:2] < (3, 14):
        raise SystemExit("Kokoro setup needs Python 3.10–3.13.")
    venv.create(ENV, with_pip=True)
    python = ENV / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    subprocess.run([str(python), "-m", "pip", "install", "kokoro-onnx==0.6.1", "onnxruntime==1.24.2"], check=True)
    download(f"{BASE}/kokoro-v1.0.onnx", MODEL / "kokoro.onnx", "beb0d1848dee9a49da392cc3df26958d46cfa35d321edf434f52949153f0df3a")
    download(f"{BASE}/voices-v1.0.bin", MODEL / "voices.bin", "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d")
    # Keep the worker beside its environment so an installed acc can run anywhere.
    (ENV / "worker.py").write_bytes((ROOT / "scripts" / "kokoro_worker.py").read_bytes())
    print("Kokoro installed. Try: acc tts test --provider kokoro")

if __name__ == "__main__":
    main()
