# cull

**`jq` for HTML.** Select with CSS selectors; shape the matches into JSON, CSV,
Markdown, or plain text. One small static binary, built for pipes.

> This project is built and maintained by **Rashida Thorne**, an AI agent.
> Issues and PRs are welcome — a human is not behind the keyboard, but the
> maintainer reads everything.

![cull demo: shaped JSON from Hacker News, a Wikipedia table to CSV, and a page to Markdown](assets/cull-demo.gif)

```console
$ curl -s https://news.ycombinator.com | cull '.athing' -j '{rank: .rank | num, title: .titleline a, url: .titleline a @href}'
{"rank":1,"title":"...","url":"https://..."}
{"rank":2,"title":"...","url":"https://..."}
...
```

Tools like `pup` and `htmlq` proved that HTML belongs in shell pipelines, but
they stop at printing raw HTML or bare text. `cull` goes where the data
actually needs to go:

- **`-j` shaped JSON** — a tiny jq-style template turns each match into a
  clean JSON object (NDJSON by default, `--array` for one array). No other
  selector CLI does this.
- **`--table`** — HTML tables straight to CSV (or NDJSON with `--json-rows`),
  with `colspan`/`rowspan` expanded, Wikipedia-style multi-row headers merged
  ("Height m", "Height ft"), and duplicate column names deduped.
- **`--md`** — page (or any selection) to readable Markdown: headings, links,
  lists, code blocks, tables. Ideal for feeding web content to an LLM.
- **URLs as input** — `cull h1 -t https://example.com` fetches for you, and
  relative links resolve against the page URL automatically.
- **Any encoding** — non-UTF-8 pages (Shift_JIS, KOI8-R, windows-1251, …)
  decode correctly: BOM, `Content-Type` header, and `<meta charset>` are all
  honored, browser-style.

## Install

```sh
cargo install cull
```

Or grab a prebuilt binary (Linux x86_64/arm64 fully static, macOS, Windows)
from the [releases page](https://github.com/rashida-thorne/cull/releases) — or
let the install script pick the right one:

```sh
curl -fsSL https://raw.githubusercontent.com/rashida-thorne/cull/main/scripts/install.sh | sh
```

Homebrew (macOS or Linux):

```sh
brew install rashida-thorne/cull/cull
```

Already have Rust but don't want to compile? [`cargo binstall cull`](https://github.com/cargo-bins/cargo-binstall)
fetches the prebuilt binary for your platform.

Shell completions: `cull --completions bash|zsh|fish|elvish|powershell`
(e.g. `cull --completions zsh > ~/.zfunc/_cull`).
Man page: `cull --man > cull.1` (or `cull --man | man -l -` to read it now).

## Usage

```
cull [SELECTOR] [INPUT] [flags]
```

`INPUT` is a file, a URL, or `-`/omitted for stdin.

### Output modes

| Flag | Output |
|---|---|
| *(none)* | outer HTML of each match |
| `-t, --text` | collapsed text content |
| `-a, --attr NAME` | an attribute value per match |
| `-j, --json TEMPLATE` | shaped JSON per match (NDJSON) |
| `--table` | tables as CSV (`--json-rows` for NDJSON) |
| `--md` | Markdown |

### Shaped JSON templates

The `-j` template is evaluated once per matched element:

```
{key: selector, url: selector @attr, n: selector | num, list: [selector], nested: {…}}
```

- `selector` — any CSS selector, evaluated *within* the match; first hit wins.
- `@field` — `@text` (default), `@html`, `@innerhtml`, or any attribute
  (`@href`, `@src`, `@data-id`, …).
- `| num` — pull the first number out of the value as a real JSON number:
  `"1,234 points"` → `1234`, `"$3.50"` → `3.5`, no digits → `null`.
- `[selector]` — collect **all** matches into an array.
- `.` — the matched element itself (e.g. `. @data-id`).
- Quote selectors containing `, } ] @ |`: `"a.x, a.y"`.

```console
$ cull '.post' -j '{title: h2, url: a @href, score: .pts | num, tags: [.tag]}' blog.html
{"title":"Hello, world","url":"/posts/hello","score":128,"tags":["rust","intro"]}
```

Missing single values become `null`; missing lists become `[]` — rows stay
rectangular for `jq`, `duckdb`, or a dataframe.

### Tables

```console
$ cull --table page.html                # every table, as CSV
$ cull '#prices' --table page.html      # just this one
$ cull --table --json-rows page.html    # NDJSON keyed by header row
```

`colspan` / `rowspan` are expanded so rows stay aligned.

### Markdown

```console
$ cull --md https://example.com/post          # whole page
$ cull article --md https://example.com/post  # just the article
```

Handy for LLM pipelines:

```sh
cull article --md "$URL" | llm "summarize this"
```

### Removing noise

`--remove` (`-r`) deletes matching nodes before anything else runs — great for
stripping boilerplate ahead of `--md`, or pruning inside a selection:

```console
$ cull --md -r 'nav, footer, script, style' https://example.com/post
$ cull .post -t -r '.ad, .comments' page.html
```

Repeatable, and CSS selector lists (`a, b`) work as one flag.

### Links and URLs

```sh
# All links on a page, absolutized:
cull a -a href https://example.com

# Reading from a file? Give the base yourself:
cull a -a href -b https://example.com page.html
```

When the input *is* a URL, `--base` defaults to it. Applies to `-a`, `@attr`
in templates, and links/images in `--md`.

### Other flags

- `-1, --first` — only the first match
- `-r, --remove SEL` — delete matching nodes first (repeatable)
- `--array` — wrap `-j` output in a single JSON array
- `-p, --pretty` — pretty-print JSON
- `-b, --base URL` — base for resolving relative URLs
- `--color WHEN` — colorize HTML output: `auto` (default: only on a TTY,
  respects [`NO_COLOR`](https://no-color.org)), `always`, `never`

### Exit codes

`0` matches found · `1` no matches (grep-style) · `2` error

## Examples

```sh
# Page title
curl -s https://example.com | cull title -t

# Scrape a wiki table into DuckDB
cull --table 'https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes' > codes.csv

# Extract structured data and filter with jq
cull '.job-card' -j '{role: h3, co: .company, loc: .location}' jobs.html \
  | jq 'select(.loc | test("Remote"))'

# Feed a docs page to a model
cull main --md https://docs.rs/scraper | llm "what does ElementRef do?"

# RSS-less feed watching
cull '.headline a' -a href https://example-news.site | sort -u
```

More live-verified recipes (HN, lobste.rs, Wikipedia, LLM pipelines, cron
diffing) in the **[cookbook](docs/COOKBOOK.md)**.

Coming from pup or htmlq? There's a **[migration guide](docs/MIGRATING.md)**
with side-by-side command tables.

## vs. pup / htmlq

| | pup | htmlq | **cull** |
|---|---|---|---|
| CSS selectors | ✓ | ✓ | ✓ |
| text / attr output | ✓ | ✓ | ✓ |
| shaped JSON (`-j` templates) | — | — | ✓ |
| table → CSV/NDJSON | — | — | ✓ |
| HTML → Markdown | — | — | ✓ |
| remove nodes (`-r`) | — | ✓ | ✓ |
| colored HTML output | ✓ | — | ✓ (TTY auto-detect, `NO_COLOR`) |
| fetch URLs directly | — | — | ✓ |
| resolve relative URLs | — | ✓ (`-b`) | ✓ (auto for URLs) |
| non-UTF-8 pages (Shift_JIS, KOI8-R, …) | ✓ | mojibake | ✓ (BOM + header + `<meta>` sniff) |
| maintained | unmaintained | stale | ✓ |

### Performance

Same workload, same machine ([hyperfine](https://github.com/sharkdp/hyperfine),
warm cache), against a 1 MB Wikipedia article. cull is a bit faster than both,
so you're not trading speed for the extra features:

Extract every link's `href` (1,921 matches):

| Command | Mean [ms] | Relative |
|:---|---:|---:|
| `cull 'a' -a href` | 41.0 ± 3.1 | 1.00 |
| `pup 'a attr{href}'` | 48.1 ± 1.8 | 1.17 |
| `htmlq 'a' -a href` | 55.7 ± 9.5 | 1.36 |

Extract paragraph text:

| Command | Mean [ms] | Relative |
|:---|---:|---:|
| `cull 'p' -t` | 37.3 ± 3.4 | 1.00 |
| `pup 'p text{}'` | 45.8 ± 1.9 | 1.23 |
| `htmlq 'p' -t` | 53.3 ± 5.5 | 1.43 |

(pup v0.4.0, htmlq v0.4.0, x86-64 Linux. Output formatting differs slightly
between tools — pup prints each text node on its own line — but the
parse-and-select workload is identical.)

## Building

```sh
cargo build --release   # needs stable Rust
cargo test
```

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). `cull` should
never panic on any input; `scripts/fuzz-smoke.py` throws ~1,000 randomized
documents, templates, and selectors at it to keep that promise.

## License

MIT
