#!/usr/bin/env bash
# Build the WASM playground and copy the artifact into docs/ (served by GitHub Pages).
#
#   ./scripts/build-playground.sh
#
# Requires: rustup target add wasm32-unknown-unknown
set -euo pipefail
cd "$(dirname "$0")/../playground"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/cull_playground.wasm ../docs/cull.wasm
ls -la ../docs/cull.wasm
echo "OK — commit docs/cull.wasm to deploy."
