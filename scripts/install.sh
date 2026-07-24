#!/bin/sh
# cull installer — downloads the latest prebuilt binary from GitHub releases.
#
#   curl -fsSL https://raw.githubusercontent.com/rashida-thorne/cull/main/scripts/install.sh | sh
#
# Options (env vars):
#   CULL_INSTALL_DIR  install directory (default: ~/.local/bin, or /usr/local/bin if run as root)
#   CULL_VERSION      version tag to install, e.g. v0.1.0 (default: latest)
set -eu

REPO="rashida-thorne/cull"

err() { printf 'install.sh: %s\n' "$1" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || err "curl is required"
command -v tar  >/dev/null 2>&1 || err "tar is required"

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux)
    case "$arch" in
      x86_64|amd64)  target="x86_64-unknown-linux-musl" ;;
      aarch64|arm64) target="aarch64-unknown-linux-musl" ;;
      *) err "unsupported architecture: $arch (build from source: cargo install cull)" ;;
    esac ;;
  Darwin)
    case "$arch" in
      x86_64)        target="x86_64-apple-darwin" ;;
      arm64)         target="aarch64-apple-darwin" ;;
      *) err "unsupported architecture: $arch" ;;
    esac ;;
  *) err "unsupported OS: $os (Windows: grab the .zip from https://github.com/$REPO/releases)" ;;
esac

if [ "${CULL_VERSION:-}" ]; then
  version=$CULL_VERSION
else
  version=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
    sed -n 's/^ *"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
  [ "$version" ] || err "could not determine the latest release tag"
fi

name="cull-$version-$target"
url="https://github.com/$REPO/releases/download/$version/$name.tar.gz"

if [ "${CULL_INSTALL_DIR:-}" ]; then
  dir=$CULL_INSTALL_DIR
elif [ "$(id -u)" = 0 ]; then
  dir=/usr/local/bin
else
  dir=$HOME/.local/bin
fi
mkdir -p "$dir"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

printf 'Downloading %s ...\n' "$url"
curl -fsSL "$url" -o "$tmp/$name.tar.gz" || err "download failed (no build for $target in $version?)"
tar xzf "$tmp/$name.tar.gz" -C "$tmp"
install -m 755 "$tmp/$name/cull" "$dir/cull"

printf 'Installed cull %s to %s/cull\n' "$version" "$dir"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) printf 'NOTE: %s is not on your PATH.\n' "$dir" ;;
esac
"$dir/cull" --version || true
