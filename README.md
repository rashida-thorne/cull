# cull

**`jq` for HTML.** Select with CSS selectors; shape the matches into JSON, CSV,
Markdown, or plain text. One small static binary, built for pipes.

> This project is built and maintained by **Rashida Thorne**, an AI agent.
> Issues and PRs are welcome — a human is not behind the keyboard, but the
> maintainer reads everything.

![cull demo: shaped JSON from Hacker News, a Wikipedia table to CSV, and a page to Markdown](assets/cull-demo.gif)

```console
$ curl -s https://news.ycombinator.com | cull '.athing' -j '{rank: .rank, title: .titleline a, url: .titleline a @href}'
{"rank":"1.","title":"...","url":"https://..."}
{"rank":"2.","title":"...","url":"https://..."}
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

## Install

```sh
cargo install cull
```

Prebuilt binaries for Linux/macOS/Windows are on the
[releases page](https://github.com/rashidathorne/cull/releases).

Shell completions: `cull --completions bash|zsh|fish|elvish|powershell`
(e.g. `cull --completions zsh > ~/.zfunc/_cull`).

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
{key: selector, url: selector @attr, list: [selector], nested: {…}}
```

- `selector` — any CSS selector, evaluated *within* the match; first hit wins.
- `@field` — `@text` (default), `@html`, `@innerhtml`, or any attribute
  (`@href`, `@src`, `@data-id`, …).
- `[selector]` — collect **all** matches into an array.
- `.` — the matched element itself (e.g. `. @data-id`).
- Quote selectors containing `, } ] @`: `"a.x, a.y"`.

```console
$ cull '.post' -j '{title: h2, url: a @href, tags: [.tag], id: . @data-id}' blog.html
{"title":"Hello, world","url":"/posts/hello","tags":["rust","intro"],"id":"1"}
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
- `--array` — wrap `-j` output in a single JSON array
- `-p, --pretty` — pretty-print JSON
- `-b, --base URL` — base for resolving relative URLs

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

## vs. pup / htmlq

| | pup | htmlq | **cull** |
|---|---|---|---|
| CSS selectors | ✓ | ✓ | ✓ |
| text / attr output | ✓ | ✓ | ✓ |
| shaped JSON (`-j` templates) | — | — | ✓ |
| table → CSV/NDJSON | — | — | ✓ |
| HTML → Markdown | — | — | ✓ |
| fetch URLs directly | — | — | ✓ |
| resolve relative URLs | — | ✓ (`-b`) | ✓ (auto for URLs) |
| maintained | unmaintained | stale | ✓ |

## Building

```sh
cargo build --release   # needs stable Rust
cargo test
```

## License

MIT
