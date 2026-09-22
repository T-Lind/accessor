#!/usr/bin/env python3
"""Install the experimental sherpa-onnx wake-word test environment.

This is intentionally separate from Accessor's production wake path. It installs
an isolated Python runtime and the official small Chinese/English KWS model.
"""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.request
import venv


MODEL_NAME = "sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20"
MODEL_URL = (
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/"
    f"{MODEL_NAME}.tar.bz2"
)
MODEL_SHA256 = "68447f4fbc67e70eee3a93961f36e81e98f47aef73ce7e7ca00885c6cd3616a6"
SHERPA_VERSION = "1.13.8"
GET_PIP_URL = "https://bootstrap.pypa.io/get-pip.py"
GET_PIP_SHA256 = "fb24e693bab954209a063d90953621412ccad4a500905a726286e038f508ddf6"


def default_assets() -> Path:
    configured = os.environ.get("ACC_ASSETS")
    if configured:
        return Path(configured).expanduser()
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "Accessor" / "assets"
    if os.name == "nt":
        return Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local")) / "Accessor" / "assets"
    return Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / "accessor" / "assets"


def digest(path: Path) -> str:
    checksum = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(block)
    return checksum.hexdigest()


def download(url: str, archive: Path, expected: str, label: str) -> None:
    if archive.is_file() and digest(archive) == expected:
        print(f"Verified {archive.name}", flush=True)
        return
    archive.parent.mkdir(parents=True, exist_ok=True)
    partial = archive.with_suffix(archive.suffix + ".part")
    print(f"Downloading {label}...", flush=True)
    request = urllib.request.Request(url, headers={"User-Agent": "Accessor/0.1"})
    with urllib.request.urlopen(request, timeout=60) as response, partial.open("wb") as output:
        shutil.copyfileobj(response, output, length=1024 * 1024)
    if digest(partial) != expected:
        partial.unlink(missing_ok=True)
        raise RuntimeError(f"Checksum mismatch for {label}; nothing was installed")
    partial.replace(archive)


def extract_model(archive: Path, models: Path) -> Path:
    destination = models / MODEL_NAME
    required = destination / "encoder-epoch-13-avg-2-chunk-16-left-64.int8.onnx"
    if required.is_file():
        print(f"Model already installed at {destination}", flush=True)
        return destination
    models.mkdir(parents=True, exist_ok=True)
    partial = models / f".{MODEL_NAME}.installing"
    if partial.exists():
        shutil.rmtree(partial)
    partial.mkdir()
    with tarfile.open(archive, "r:bz2") as bundle:
        prefix = f"{MODEL_NAME}/"
        for member in bundle:
            if not member.isfile() or not member.name.startswith(prefix):
                continue
            relative = Path(member.name[len(prefix):])
            if not relative.parts or relative.is_absolute() or ".." in relative.parts:
                raise RuntimeError(f"Unsafe path in model archive: {member.name}")
            target = partial / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            source = bundle.extractfile(member)
            if source is None:
                raise RuntimeError(f"Could not read {member.name}")
            with source, target.open("wb") as output:
                shutil.copyfileobj(source, output)
    if not (partial / required.name).is_file():
        raise RuntimeError("Keyword model archive did not contain the expected encoder")
    partial.replace(destination)
    print(f"Installed model at {destination}", flush=True)
    return destination


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", type=Path, default=default_assets())
    parser.add_argument("--archive", type=Path, help="Use an already-downloaded model archive")
    args = parser.parse_args()
    assets = args.assets.expanduser().resolve()
    environment = assets / "runtime" / "sherpa-kws"
    python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    if not python.is_file():
        print(f"Creating isolated Python environment at {environment}", flush=True)
        try:
            venv.create(environment, with_pip=True)
        except subprocess.CalledProcessError:
            # Minimal Debian/Ubuntu installs omit ensurepip. The environment is
            # still usable, so bootstrap a pinned copy of pip without sudo.
            if not python.is_file():
                venv.create(environment, with_pip=False)
    pip_check = subprocess.run(
        [str(python), "-m", "pip", "--version"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if pip_check.returncode:
        bootstrap = assets / "downloads" / "get-pip.py"
        download(GET_PIP_URL, bootstrap, GET_PIP_SHA256, "PyPA pip bootstrap")
        subprocess.run([str(python), str(bootstrap)], check=True)
    subprocess.run(
        [str(python), "-m", "pip", "install", f"sherpa-onnx=={SHERPA_VERSION}", "numpy==2.4.3"],
        check=True,
    )
    archive = args.archive or assets / "downloads" / f"{MODEL_NAME}.tar.bz2"
    if args.archive:
        archive = archive.expanduser().resolve()
        if digest(archive) != MODEL_SHA256:
            raise SystemExit(f"Checksum mismatch for {archive}")
    else:
        download(MODEL_URL, archive, MODEL_SHA256, MODEL_NAME)
    model = extract_model(archive, assets / "models")
    print("\nSherpa wake-word experiment is ready.")
    print(f"Model: {model}")
    print("Live test: python3 scripts/test_wakeword.py")
    print("File test: python3 scripts/test_wakeword.py --wav tests/fixtures/voice-benchmark.wav")


if __name__ == "__main__":
    main()
