#!/usr/bin/env python3
"""Download pinned Canary INT8 files and the official CPU ONNX Runtime.

`acc` now does this itself on first run (any OS). This script remains for
manual/offline use and for setup_tts.py, which imports `download`.
"""
import hashlib
import io
import platform
import shutil
import tarfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODEL = ROOT / "models" / "canary-180m-flash"
RUNTIME = ROOT / "runtime"
REVISION = "92c2231a4e2b2524277fea759be967d2e6edfc49"
BASE = f"https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/{REVISION}"
FILES = [
    ("encoder-model.int8.onnx", f"{BASE}/encoder-model.int8.onnx", "996d1c89e6cbc891a7c88bf410884c178ffa474f7b13084522ac74a5e144cc81"),
    ("decoder-model.int8.onnx", f"{BASE}/decoder-model.int8.onnx", "9dd9c447872088c912e916d73751f9621a54085d5bc46788454fe904db51a914"),
    ("vocab.txt", f"{BASE}/vocab.txt", "2dae6fc7815f9640645e0c765522b278ee0cef49b482d91f6913e334628d3e77"),
    ("nemo128.onnx", "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx", "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f"),
]
RUNTIMES = {
    ("Windows", "x86_64"): ("win-x64", "zip", "8e3e9c826375352e29cb2614fe44f3d7a4b0ff7b8028ad7a456af9d949a7e8b0"),
    ("Windows", "aarch64"): ("win-arm64", "zip", "dd8180d98e5a0ead7ead99029acc80b86a8b905b9aba4cc978e388039bb5823b"),
    ("Linux", "x86_64"): ("linux-x64", "tgz", "43725474ba5663642e17684717946693850e2005efbd724ac72da278fead25e6"),
    ("Linux", "aarch64"): ("linux-aarch64", "tgz", "6715b3d19965a2a6981e78ed4ba24f17a8c30d2d26420dbed10aac7ceca0085e"),
    ("Darwin", "aarch64"): ("osx-arm64", "tgz", "0af4fa503e8ea285245b47ee42d0a7461b8156a81270857da0c1d4ecf858abde"),
}


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def download(url, path, expected):
    if path.is_file() and digest(path) == expected:
        print(f"Verified {path.name}", flush=True)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    part = path.with_suffix(path.suffix + ".part")
    print(f"Downloading {path.name}...", flush=True)
    request = urllib.request.Request(url, headers={"User-Agent": "Accessor/0.1"})
    with urllib.request.urlopen(request, timeout=60) as response, part.open("wb") as out:
        shutil.copyfileobj(response, out, length=1024 * 1024)
    if digest(part) != expected:
        raise RuntimeError(f"Checksum mismatch for {path.name}; file was not installed")
    part.replace(path)


def install_runtime():
    machine = platform.machine().lower()
    machine = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    entry = RUNTIMES.get((platform.system(), machine))
    if not entry:
        raise RuntimeError("No bundled runtime for this platform. Supply a compatible ONNX Runtime 1.24+ using ORT_DYLIB_PATH (Intel macOS needs a separate build).")
    target, ext, checksum = entry
    name = f"onnxruntime-{target}-1.24.2.{ext}"
    archive = RUNTIME / name
    download(f"https://github.com/microsoft/onnxruntime/releases/download/v1.24.2/{name}", archive, checksum)
    # Copy only regular library/license files, flattening names. Never extract
    # arbitrary archive paths or symlinks into the user's filesystem.
    def save(name, data):
        leaf = Path(name).name
        if ("/lib/" in name and (".so" in leaf or ".dylib" in leaf or leaf.endswith(".dll"))) or leaf in ("LICENSE", "ThirdPartyNotices.txt"):
            if leaf.startswith("libonnxruntime.so."):
                leaf = "libonnxruntime.so"
            elif leaf.startswith("libonnxruntime.") and leaf.endswith(".dylib"):
                leaf = "libonnxruntime.dylib"
            dest = RUNTIME / leaf
            temp = dest.with_suffix(dest.suffix + ".part")
            with temp.open("wb") as out:
                shutil.copyfileobj(data, out)
            temp.replace(dest)
    if ext == "zip":
        with zipfile.ZipFile(archive) as z:
            for info in z.infolist():
                if not info.is_dir():
                    with z.open(info) as data:
                        save(info.filename, data)
    else:
        with tarfile.open(archive) as tar:
            for info in tar:
                if info.isfile():
                    with tar.extractfile(info) as data:
                        save(info.name, data)


if __name__ == "__main__":
    install_runtime()
    for name, url, sha in FILES:
        download(url, MODEL / name, sha)
    print("Speech ready. Run cargo run --release -- run --wake-code 29 --agent mock")
