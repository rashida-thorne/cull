# Changelog

All notable changes to **cull** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.11.2] — 2026-07-26

### Fixed
- No more panic on element-less documents: input that parses to a tree with
  no root element (e.g. only an `<?xml version="1.0"?>` declaration, or
  `--xml` with a comment-only body) used to abort with
  `html node missing` when run in whole-document modes (`--md`, `--table`,
  `--json-nodes`, no-selector). It now behaves like "no matches"
  (exit code 1). Found by the extended fuzz harness, which now also covers
  XML mode, nested `-j` templates, and `--json-nodes`.

## [0.11.1] — 2026-07-26

### Fixed
- `cull -I page.html` / `cull -I https://…` now treat the first positional
  as the *input* when it looks like a file or URL (same disambiguation as
  `--table`/`--md`). In 0.11.0 it was parsed as a selector, so launching
  interactive mode without an explicit selector needed a confusing
  workaround. `cull -I '.athing' page.html` still works, of course.

## [0.11.0] — 2026-07-26

### Added
- **`-I` / `--interactive`** — a live-preview TUI for finding the right
  selector. Type to edit the selector and watch matches update on every
  keystroke; **Tab** cycles the output shape (as-given → HTML → text →
  Markdown → JSON node tree); **↑/↓/PgUp/PgDn** scroll; **Enter** prints the
  current result to stdout and exits (exit code follows matches, grep-style);
  **Esc** quits without printing. The UI draws on stderr and reads keys from
  the terminal, so stdin can be a pipe and stdout stays clean:
  `curl -s https://example.com | cull -I | less`. Works with the other flags
  (`-j` templates, `--table`, `--has-text`, `-r`, `-b`, `-1`, …) — they shape
  the preview too. Invalid selectors show inline instead of erroring out.
  Ships as a default-on `interactive` cargo feature
  (`--no-default-features` builds skip the dependency).

## [0.10.0] — 2026-07-26

### Added
- **XML mode** — RSS/Atom feeds, sitemaps, SVG, OPML, and arbitrary XML now
  parse correctly. HTML parsers silently mangle XML (`<link>` is a void
  element in HTML, so every RSS item's URL is lost; `pubDate` is lowercased);
  cull now detects XML (an `<?xml…?>` declaration or a known root:
  `<rss>`, `<feed>`, `<urlset>`, `<sitemapindex>`, `<opml>`, `<svg>`) and
  parses it with a real XML parser. `--xml` forces XML parsing, `--html`
  forces HTML. In XML mode selectors are case-sensitive (`pubDate` matches
  `<pubDate>` only) and namespaced tags are selectable with an escaped
  colon (`media\:thumbnail @url`). All output modes work: `-t` puts each
  XML field on its own line, `-j` templates, `-p`, `--json-nodes`, `-i`, …
  A feed reader is now one line:
  `cull item -j '{title: title, url: link, date: pubDate}' https://lobste.rs/rss`
- Charset sniffing now also honors the `encoding` label in a leading
  `<?xml version="1.0" encoding="…"?>` declaration (after BOM and
  `Content-Type`, before `<meta charset>`).

### Fixed
- Detection is per-input, so `cull 'h2, item > title' page.html feed.xml`
  parses each file the right way. If auto-detected XML turns out to be
  malformed, cull warns on stderr and falls back to the forgiving HTML
  parser; explicit `--xml` makes malformed XML a hard error (exit 2).

## [0.9.0] — 2026-07-26

### Added
- **`--json-nodes`** — dump each match as a JSON node tree (pup's `json{}`,
  with a cleaner shape): `{"tag", "attrs": {…}, "text", "children": […]}`.
  Attributes live in their own object so they can't collide with
  `tag`/`text`; `text` is the collapsed subtree text; `children`
  interleaves child elements and text-node strings; URL attributes respect
  `-b/--base` (and auto-base for fetched URLs). Works with `--array` and
  `-p/--pretty`, NDJSON by default. Also the escape hatch for raw
  `<script>` payloads, e.g.
  `cull 'script[type="application/ld+json"]' --json-nodes | jq '.children[0] | fromjson'`.

## [0.8.0] — 2026-07-26

### Added
- **Nested objects in `-j` templates**: `sel {…}` evaluates the object with
  `sel`'s first match as the new context (null if nothing matches), and
  `[sel {…}]` emits **one object per match** — so
  `cull '.post' -j '{title: h2, comments: [.c {user: .u, text: .t}]}'`
  now does real hierarchical extraction. Nests arbitrarily deep.
- **New template filters** `| trim`, `| lower`, `| upper`, and filters now
  **chain**: `{href: a @href | trim | lower}`.

### Fixed
- Whitespace-collapsed text (`-j` values, `--table` cells, `--has-text`
  matching) is now layout-aware: `<br>` and block boundaries contribute a
  space (`A<br>B` no longer glues to `AB`), and `script`/`style`/`template`
  content no longer leaks into extracted values.

## [0.7.0] — 2026-07-26

All three features in this release answer long-open feature requests on
htmlq's tracker ([#75](https://github.com/mgdm/htmlq/issues/75),
[#55](https://github.com/mgdm/htmlq/issues/55),
[#74](https://github.com/mgdm/htmlq/issues/74)).

### Added
- `-i`/`--inner` prints inner HTML — children only, no outer tag. Composes
  with `-p` (pretty) and `--color`.
- `--has-text STRING` keeps only matches whose text content contains STRING.
  Repeatable (all strings must be present); runs before `-1`/`-c`/`-l`, so
  counts and file lists reflect the filter. Matching is against
  whitespace-collapsed text, so needles cross inline-tag boundaries.

### Changed
- `-t`/`--text` now lays text out the way a browser would (innerText-style):
  `<br>` and block-element boundaries become newlines, `<pre>`/`<textarea>`
  contents stay verbatim, `script`/`style`/`template` are skipped, and inline
  whitespace is still collapsed. Previously block boundaries were dropped
  entirely (`<div>x<p>y</p></div>` → `xy`). Single-line values in `-j`
  templates and `--table` cells are unchanged.

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

[Unreleased]: https://github.com/rashida-thorne/cull/compare/v0.11.2...HEAD
[0.11.2]: https://github.com/rashida-thorne/cull/compare/v0.11.1...v0.11.2
[0.11.1]: https://github.com/rashida-thorne/cull/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/rashida-thorne/cull/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/rashida-thorne/cull/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/rashida-thorne/cull/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/rashida-thorne/cull/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/rashida-thorne/cull/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/rashida-thorne/cull/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/rashida-thorne/cull/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/rashida-thorne/cull/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/rashida-thorne/cull/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/rashida-thorne/cull/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/rashida-thorne/cull/releases/tag/v0.1.0
