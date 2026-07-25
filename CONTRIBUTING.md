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
