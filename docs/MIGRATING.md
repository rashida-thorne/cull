# Migrating from pup or htmlq

Every common pup/htmlq invocation has a direct cull equivalent — usually
shorter, and often you can drop the trailing `sed`/`awk`/`paste` entirely.

All examples assume `curl -s URL | ...` on the left; with cull you can also
just pass the URL as the input argument (relative links resolve automatically).

## From htmlq

| htmlq | cull |
|---|---|
| `htmlq 'a'` | `cull 'a'` |
| `htmlq 'a' --text` | `cull 'a' -t` |
| `htmlq 'a' --attribute href` | `cull 'a' -a href` |
| `htmlq 'a' -a href --base URL` | `cull 'a' -a href -b URL` (automatic when the input *is* a URL) |
| `htmlq --remove-nodes nav 'a'` | `cull 'a' -r nav` |
| `htmlq 'a' --filename page.html` | `cull 'a' page.html` |
| `htmlq --pretty '#main'` | `cull '#main'` (colored on a TTY; `--color always` to force) |

Things htmlq has no equivalent for:

```sh
# Shaped JSON — one object per match, keys in your order
cull '.athing.submission' -j '{title: .titleline > a, url: .titleline > a @href}' https://news.ycombinator.com

# Any <table> to CSV, or NDJSON keyed by header row
cull --table page.html
cull --table --json-rows page.html

# Page (minus chrome) to Markdown, e.g. to feed an LLM
cull --md -r 'nav, footer, script, style' https://example.com/post
```

Also: htmlq decodes everything as UTF-8 and produces mojibake on
Shift_JIS / windows-125x / KOI8-R pages; cull sniffs BOM, `Content-Type`
header, and `<meta charset>`.

## From pup

| pup | cull |
|---|---|
| `pup 'a'` | `cull 'a'` |
| `pup 'a text{}'` | `cull 'a' -t` |
| `pup 'a attr{href}'` | `cull 'a' -a href` |
| `pup 'a json{}'` | `cull 'a' -j '{...}'` — you name the fields you want |
| `pup -f page.html 'a'` | `cull 'a' page.html` |
| `pup --color 'div'` | `cull 'div'` (auto on TTY) |
| `pup 'div slice{0,1}'` | `cull 'div' -1` (or CSS `:nth-child` / `:nth-of-type`) |

Differences worth knowing:

- pup's `json{}` dumps the entire node structure; cull's `-j` templates
  extract exactly the fields you ask for, so there's no follow-up `jq` pass
  for the common cases.
- pup prints each text node on its own line in `text{}`; cull collapses
  whitespace per match (one line per match).
- cull exits `1` when nothing matched (grep-style), which makes shell
  conditionals work: `if cull '.error' -t page.html >/dev/null; then ...`

## Selector notes

cull uses [the `scraper` crate's](https://docs.rs/scraper) CSS selector
engine (html5ever + selectors — the same matching code as Firefox/Servo).
Standard CSS works: descendant/child/sibling combinators, attribute
selectors (`a[href^="https:"]`), `:not()`, `:nth-child()`, etc.
Non-standard extensions from pup (`slice{}`, `json{}`, text/attr pseudo
"display filters") are replaced by cull's flags shown above.

Something missing from these tables? [Open an issue](https://github.com/rashida-thorne/cull/issues)
— migration gaps are treated as bugs.
