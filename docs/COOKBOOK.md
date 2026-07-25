# cull cookbook

Real-world recipes, each verified against the live page it targets.
(Sites change; if a selector goes stale, `cull <sel> -t` interactively until it clicks.)

Everything below assumes `cull` is on your `PATH`. When the input argument is a
URL, cull fetches it and automatically resolves relative `href`/`src` against it.

---

## 1. Scrape a news front page into NDJSON

Hacker News titles + URLs, one JSON object per story:

```console
$ cull '.athing.submission' -j '{title: .titleline > a, url: .titleline > a @href}' https://news.ycombinator.com
{"title":"Back to Kagi","url":"https://blog.melashri.net/micro/back-to-kagi/"}
...
```

Add points as real numbers with the `| num` filter (strips everything but the number):

```console
$ cull 'td.subtext' -j '{points: .score | num}' https://news.ycombinator.com
{"points":344}
```

lobste.rs, including the tag list (`[sel]` collects *all* matches into an array):

```console
$ cull '.story' -j '{title: .u-url, url: .u-url @href, tags: [.tag]}' https://lobste.rs
{"title":"...","url":"https://...","tags":["security","vibecoding"]}
```

Want one big array instead of NDJSON? Add `--array`; add `-p` to pretty-print.

## 2. Tables → CSV or JSON

Any Wikipedia table straight to CSV (multi-row headers are merged,
`colspan`/`rowspan` are expanded):

```console
$ cull --table 'table.wikitable' https://en.wikipedia.org/wiki/List_of_tallest_buildings > buildings.csv
```

Same thing as one JSON object per row, keyed by header — ready for `jq`:

```console
$ cull --table 'table.wikitable' --json-rows https://en.wikipedia.org/wiki/List_of_tallest_buildings \
    | jq -r 'select((.Floors | tonumber? // 0) > 100) | .Name'
```

No selector? `cull --table page.html` extracts every table in the document,
blank-line separated.

## 3. Page → Markdown (feed a web page to an LLM)

Strip the chrome, keep the content:

```console
$ cull article --md https://example.com/post | llm "summarize this"
```

Headings, lists, code blocks, tables, blockquotes, links and images all survive
the trip. Links come out absolute because the base URL is known.

No convenient `<article>` to select? Strip the boilerplate instead with
`--remove` (`-r`), which deletes matching nodes before conversion:

```console
$ cull --md -r 'header, nav, footer, script, style' https://lobste.rs/about | head -3

# About

Lobsters is a computing-focused community centered around link aggregation...
```

## 4. Pull attributes

Every image URL on a page (already absolute, thanks to auto-base):

```console
$ cull img -a src 'https://en.wikipedia.org/wiki/Rust_(programming_language)'
https://en.wikipedia.org/static/images/icons/enwiki-25.svg
...
```

All external links:

```console
$ cull 'a[href^=http]' -a href page.html | sort -u
```

From stdin, resolve relative URLs yourself with `-b`:

```console
$ curl -s https://example.com/docs/ | cull a -a href -b https://example.com/docs/
```

## 5. Quick answers

First match only (`-1`), collapsed text (`-t`):

```console
$ cull h1 -t -1 'https://en.wikipedia.org/wiki/Rust_(programming_language)'
Rust (programming language)
```

Page title of anything:

```console
$ cull title -t -1 https://example.com
```

Grab a meta tag:

```console
$ cull 'meta[property="og:description"]' -a content -1 page.html
```

## 6. Monitoring / diffing in cron

Watch a page section for changes:

```console
$ cull '.release-notes' -t https://example.com/changelog > /tmp/new
$ cmp -s /tmp/new /tmp/old || { diff /tmp/old /tmp/new; cp /tmp/new /tmp/old; }
```

Because output is deterministic and plain, `diff`, `grep`, `awk`, and `jq`
compose the rest.

## 7. Slicing raw HTML

No output flag = the matched elements' outer HTML, one per line block. Useful
for chaining culls:

```console
$ cull '#main' page.html | cull table --table
```

`cull | head` is safe — SIGPIPE is handled like grep, no panic spew.

---

## 8. Working over many files

Globs behave like grep: output concatenates, `-c` and `-l` go per-file.

```console
$ cull 'img:not([alt])' -c site/**/*.html      # alt-text audit
site/index.html:0
site/blog/post1.html:4

$ cull 'a[href^="http:"]' -l site/**/*.html    # pages with insecure links
site/legacy.html

$ cull '.product' -j '{name: h2, price: .price}' snapshots/*.html \
    | jq -s 'sort_by(.price)'                  # merge a crawl into one dataset
```

Unreadable files are reported on stderr and skipped (final exit code 2).

---

## Template syntax refresher

```
{key: selector, key2: selector @attr, key3: [selector], key4: selector | num}
```

- `selector` → collapsed text of the first match (relative to the outer match)
- `@attr` → attribute value instead of text
- `[...]` → array of all matches
- `| num` → extract the first number (int or float) from the text
- quote a selector if it contains `,` or `}`: `{age: "a:last-child"}`
