# Changelog

All notable changes to **cull** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0] — 2026-07-26

### Added
- `-H`/`--header 'Name: Value'` for built-in URL fetches, curl-style and
  repeatable. Overriding `User-Agent` suppresses the default `cull/VERSION` one.
- `--timeout SECS` bounds each fetch (default 30 s; `0` disables). Previously a
  stalled origin could hang forever.
- Offline integration tests that assert the exact request headers on the wire.
- Multi-arch container image on GHCR: `ghcr.io/rashida-thorne/cull`
  (amd64 + arm64, `FROM scratch`, ~2 MB; pushed automatically on release).

## [0.5.0] — 2026-07-25

### Added
- **Multiple input files**: `cull SEL a.html b.html …` — globs just work.
- `-l`/`--files-with-matches` (grep `-l` semantics).
- `-c`/`--count` prints `file:count` lines when given multiple inputs.
- `-j --array` merges matches from all inputs into a single JSON array.

### Changed
- `-1`/`--first` applies per input (like `grep -m1`).
- Unreadable inputs are reported on stderr and skipped; remaining inputs still
  run, with exit code 2 at the end (like grep).
- Auto `--base` resolves per input when inputs are URLs.

## [0.4.0] — 2026-07-25

### Added
- `-p`/`--pretty` now pretty-prints **HTML** output (2-space indent), not just
  JSON; combines with `--color`. Contents of `pre`, `textarea`, `script`, and
  `style` are never reformatted.
- `-c`/`--count` — print only the number of matches, `grep -c` style
  (respects `-r/--remove`; exit 1 when zero).

## [0.3.0] — 2026-07-25

### Added
- `--color auto|always|never` — ANSI-highlighted HTML output. `auto` (default)
  colorizes only on a TTY and honors [`NO_COLOR`](https://no-color.org).
  Highlighting walks the parsed tree, not a regex pass.
- [`docs/MIGRATING.md`](docs/MIGRATING.md): side-by-side pup → cull and
  htmlq → cull command tables.

## [0.2.0] — 2026-07-25

### Added
- `-r`/`--remove SELECTOR` — strip nodes matching a selector before selection,
  `--md`, or `--table` runs. Repeatable. Useful for dropping nav/footer/script
  boilerplate before converting a page to Markdown.

## [0.1.0] — 2026-07-24

Initial release.

### Added
- CSS selection with text (`-t`), attribute (`-a`), and HTML output modes.
- **Shaped JSON extraction** (`-j`): jq-style templates over CSS selectors,
  e.g. `cull '.post' -j '{title: h2, url: a @href, tags: [.tag]}'` → NDJSON.
- `--table` — HTML tables to CSV or NDJSON, with colspan/rowspan expansion and
  multi-row header merging.
- `--md` — HTML to Markdown (headings, lists, code, tables, blockquotes,
  links with `--base` resolution).
- Built-in URL fetch; `--base` auto-set when the input is a URL.
- Charset detection via `encoding_rs` (BOM > header > `<meta>` prescan >
  lossy UTF-8).
- `--completions bash|zsh|fish|elvish|powershell`, `--man`, SIGPIPE handling
  (`cull … | head` exits silently, like grep).
- Prebuilt static binaries for 5 targets; `curl | sh` installer; Homebrew tap.

[Unreleased]: https://github.com/rashida-thorne/cull/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/rashida-thorne/cull/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/rashida-thorne/cull/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/rashida-thorne/cull/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/rashida-thorne/cull/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/rashida-thorne/cull/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/rashida-thorne/cull/releases/tag/v0.1.0
