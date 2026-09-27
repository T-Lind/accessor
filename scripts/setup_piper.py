#!/usr/bin/env python3
"""Install Piper (fast local neural TTS) and one voice.

Downloads the pinned Piper release for this platform plus a voice from the
rhasspy/piper-voices repository. Nothing is installed outside ACC_ASSETS.
"""
import hashlib
import os
import platform
import shutil
import tarfile
import urllib.request
import zipfile
from pathlib import Path

ASSETS = Path(os.environ.get("ACC_ASSETS", Path(__file__).resolve().parent.parent))
PIPER = ASSETS / "piper"
VOICE = os.environ.get("PIPER_VOICE", "en_GB-alan-medium")
RELEASE = "2023.11.14-2"
# (system, machine) -> (release asset, sha256 or None)
RUNTIMES = {
    ("Linux", "x86_64"): (
        "piper_linux_x86_64.tar.gz",
        "a50cb45f355b7af1f6d758c1b360717877ba0a398cc8cbe6d2a7a3a26e225992",
    ),
    ("Linux", "aarch64"): ("piper_linux_aarch64.tar.gz", None),
    ("Darwin", "x86_64"): ("piper_macos_x64.tar.gz", None),
    ("Darwin", "arm64"): ("piper_macos_aarch64.tar.gz", None),
    ("Windows", "amd64"): ("piper_windows_amd64.zip", None),
}
# voice id -> path under the piper-voices repository
VOICES = {
    "en_GB-alan-medium": "en/en_GB/alan/medium",
    "en_GB-cori-high": "en/en_GB/cori/high",
    "en_US-lessac-medium": "en/en_US/lessac/medium",
    "en_US-amy-medium": "en/en_US/amy/medium",
}


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def fetch(url, path, expected=None):
    if path.is_file() and (expected is None or digest(path) == expected):
        print(f"Verified {path.name}", flush=True)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    part = path.with_suffix(path.suffix + ".part")
    print(f"Downloading {path.name}...", flush=True)
    request = urllib.request.Request(url, headers={"User-Agent": "Accessor/0.1"})
    with urllib.request.urlopen(request, timeout=120) as response, part.open("wb") as out:
        shutil.copyfileobj(response, out, length=1024 * 1024)
    if expected is not None and digest(part) != expected:
        part.unlink(missing_ok=True)
        raise RuntimeError(f"Checksum mismatch for {path.name}; file was not installed")
    part.replace(path)


def extract_piper(archive, destination):
    """Extract only the release's own `piper/` tree, never arbitrary paths."""
    destination.mkdir(parents=True, exist_ok=True)
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as bundle:
            for member in bundle.namelist():
                if member == "piper" or member.startswith("piper/"):
                    bundle.extract(member, destination)
    else:
        with tarfile.open(archive) as bundle:
            for member in bundle.getmembers():
                if member.name == "piper" or member.name.startswith("piper/"):
                    bundle.extract(member, destination, filter="data")


def main():
    machine = platform.machine().lower()
    machine = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    entry = RUNTIMES.get((platform.system(), machine))
    if not entry:
        raise SystemExit("No Piper build for this platform; choose Kokoro or Cartesia.")
    asset, expected = entry
    bin_dir = PIPER / "bin"
    executable = bin_dir / ("piper.exe" if platform.system() == "Windows" else "piper")
    if not executable.is_file():
        archive = PIPER / asset
        fetch(
            f"https://github.com/rhasspy/piper/releases/download/{RELEASE}/{asset}",
            archive,
            expected,
        )
        staging = PIPER / "_staging"
        shutil.rmtree(staging, ignore_errors=True)
        extract_piper(archive, staging)
        if bin_dir.exists():
            shutil.rmtree(bin_dir)
        shutil.move(str(staging / "piper"), str(bin_dir))
        shutil.rmtree(staging, ignore_errors=True)

    if VOICE not in VOICES:
        raise SystemExit(f"Unknown Piper voice {VOICE}")
    voice_dir = PIPER / "voices"
    base = f"https://huggingface.co/rhasspy/piper-voices/resolve/main/{VOICES[VOICE]}/{VOICE}"
    for suffix in (".onnx", ".onnx.json"):
        fetch(f"{base}{suffix}", voice_dir / f"{VOICE}{suffix}")
    print(f"Piper installed with voice {VOICE}.")


if __name__ == "__main__":
    main()
