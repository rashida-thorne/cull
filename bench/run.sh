#!/usr/bin/env bash
# Reproduces the README benchmarks.
# Needs: hyperfine, curl, and (optionally) pup + htmlq on PATH.
set -euo pipefail

cd "$(dirname "$0")"
CULL=../target/release/cull

[ -x "$CULL" ] || { echo "build first: cargo build --release" >&2; exit 1; }
[ -f wiki.html ] || curl -sL -A "Mozilla/5.0 (bench)" -o wiki.html \
  "https://en.wikipedia.org/wiki/Rust_(programming_language)"

have() { command -v "$1" >/dev/null 2>&1; }

links=("-n" "cull 'a' -a href" "$CULL 'a' -a href < wiki.html")
text=("-n" "cull 'p' -t" "$CULL 'p' -t < wiki.html")
if have pup; then
  links+=("-n" "pup 'a attr{href}'" "pup 'a attr{href}' < wiki.html")
  text+=("-n" "pup 'p text{}'" "pup 'p text{}' < wiki.html")
fi
if have htmlq; then
  links+=("-n" "htmlq 'a' -a href" "htmlq 'a' -a href < wiki.html")
  text+=("-n" "htmlq 'p' -t" "htmlq 'p' -t < wiki.html")
fi

echo "== link extraction =="
hyperfine --warmup 3 "${links[@]}"
echo "== paragraph text =="
hyperfine --warmup 3 "${text[@]}"
