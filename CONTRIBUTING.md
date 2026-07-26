# Contributing to cull

Thanks for your interest! Bug reports, recipes for the cookbook, and PRs are
all welcome.

## Building

```sh
cargo build --release          # binary at target/release/cull
```

No non-Rust dependencies. MSRV is whatever stable was ~6 months ago; if a
newer feature sneaks in, CI will catch it — feel free to flag it.

## Before you open a PR

```sh
cargo test                     # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

CI runs exactly these three, so a green local run means a green PR.

## Fuzz smoke test

`scripts/fuzz-smoke.py` throws ~1,000 randomized inputs at the release
binary — tag soup, raw random bytes, malformed templates and selectors, and
pathological documents (5,000-deep nesting, 1 MB attributes, colspan/rowspan
bombs). It fails on any panic or timeout:

```sh
cargo build --release
python3 scripts/fuzz-smoke.py   # expect: "failures: 0"
```

If you touch the template parser (`-j`), the table extractor (`--table`), or
input decoding, please run it and mention the result in your PR.

## What makes a good bug report

A minimal HTML snippet + the exact command + expected vs. actual output.
`cull` should never panic on any input — a panic is always a bug, however
absurd the input.

## Cookbook recipes

`docs/COOKBOOK.md` recipes are all verified against live pages. If you add
one, note the date you verified it.

## Cutting a release (maintainer checklist)

1. Update `CHANGELOG.md`: move Unreleased items under the new version heading,
   add the compare link at the bottom.
2. Bump `version` in `Cargo.toml`; `cargo build` to refresh `Cargo.lock`.
3. Tag `vX.Y.Z` and push — CI builds the 5 targets, uploads release assets,
   and pushes the GHCR image automatically.
4. `cargo publish`.
5. Bump the Homebrew tap and Scoop manifest (version + sha256s), the winget
   manifest, and `softwareVersion` in `docs/index.html` JSON-LD.
6. Verify: `curl | sh` installer, `cargo binstall cull`, `brew install`,
   and the aarch64 binary under qemu.

## WASM playground

The website's [playground](https://rashida-thorne.github.io/cull/playground.html)
runs cull's core compiled to WebAssembly (`playground/` crate, hand-rolled ABI,
no wasm-bindgen). The built artifact `docs/cull.wasm` is committed. If you change
anything that affects output (selection, --md, --table, -j templates, serializers),
rebuild it:

```sh
rustup target add wasm32-unknown-unknown
./scripts/build-playground.sh   # rebuilds and copies docs/cull.wasm
```

CI compiles the playground (and the no-default-features lib) so it can't rot,
but does not diff the committed wasm.
