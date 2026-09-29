#!/bin/sh
# Install a prebuilt Accessor (acc) binary on Linux or Apple Silicon macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.sh | sh
#
# Overrides:
#   ACC_VERSION=0.34.4   install a specific release instead of the latest
#   ACC_BIN_DIR=DIR      install into DIR instead of ~/.local/bin
#
# This downloads only the small acc binary; the first run fetches ONNX Runtime,
# the speech model, and the Piper voice into Accessor's assets folder.
set -eu

REPO="T-Lind/accessor"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }

os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Linux/x86_64 | Linux/amd64) target="x86_64-unknown-linux-gnu" ;;
  Linux/aarch64 | Linux/arm64) target="aarch64-unknown-linux-gnu" ;;
  Darwin/arm64) target="aarch64-apple-darwin" ;;
  Darwin/x86_64)
    err "Intel macOS is not supported by the prebuilt binaries; build from source with 'cargo install --path . --bin acc'" ;;
  *) err "unsupported platform: $os/$arch" ;;
esac

if [ -n "${ACC_VERSION:-}" ]; then
  version="${ACC_VERSION#v}"
else
  api="https://api.github.com/repos/${REPO}/releases/latest"
  if command -v curl >/dev/null 2>&1; then
    tag="$(curl -fsSL "$api" | grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"\([^"]*\)"$/\1/')"
  elif command -v wget >/dev/null 2>&1; then
    tag="$(wget -qO- "$api" | grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"\([^"]*\)"$/\1/')"
  else
    err "need curl or wget"
  fi
  [ -n "$tag" ] || err "could not find the latest release (set ACC_VERSION to override)"
  version="${tag#v}"
fi

asset="acc-${version}-${target}.tar.gz"
base="https://github.com/${REPO}/releases/download/v${version}"
bin_dir="${ACC_BIN_DIR:-$HOME/.local/bin}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading ${asset} ..."
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "${base}/${asset}" -o "${tmp}/${asset}"
  curl -fsSL "${base}/SHA256SUMS" -o "${tmp}/SHA256SUMS"
else
  wget -qO "${tmp}/${asset}" "${base}/${asset}"
  wget -qO "${tmp}/SHA256SUMS" "${base}/SHA256SUMS"
fi

echo "Verifying checksum ..."
if command -v sha256sum >/dev/null 2>&1; then
  digest="$(sha256sum "${tmp}/${asset}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  digest="$(shasum -a 256 "${tmp}/${asset}" | awk '{print $1}')"
else
  err "need sha256sum or shasum to verify the download"
fi
expected="$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1 }' "${tmp}/SHA256SUMS" | head -1)"
[ -n "$expected" ] || err "no checksum for ${asset} in SHA256SUMS"
[ "$digest" = "$expected" ] || err "checksum mismatch for ${asset}"

tar -xzf "${tmp}/${asset}" -C "$tmp"
mkdir -p "$bin_dir"
install -m 0755 "${tmp}/acc" "${bin_dir}/acc" 2>/dev/null || {
  cp "${tmp}/acc" "${bin_dir}/acc"
  chmod 0755 "${bin_dir}/acc"
}

echo "Installed acc ${version} to ${bin_dir}/acc"
case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *) echo "Add it to your PATH:  export PATH=\"${bin_dir}:\$PATH\"" ;;
esac
echo "Next:  acc doctor   (first run downloads the speech files)"
