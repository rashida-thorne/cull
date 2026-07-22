#!/usr/bin/env python3
"""Build an asciinema v2 cast for the cull demo from pre-captured outputs."""
import json, random

random.seed(7)
W, H = 120, 30
events = []
t = 0.6

def out(s):
    events.append([round(t, 4), "o", s])

def type_line(s, cps=(0.028, 0.075)):
    global t
    for ch in s:
        out(ch)
        t += random.uniform(*cps)
    t += 0.35
    out("\r\n")

PROMPT = "\u001b[1;32m❯\u001b[0m "
DIM = "\u001b[2m"
RST = "\u001b[0m"

def comment(txt):
    global t
    out(PROMPT)
    t += 0.5
    type_line(f"{DIM}# {txt}{RST}", cps=(0.012, 0.03))
    t += 0.15

def cmd(command, outfile, net_pause=0.9, hold=2.6, wrap=W):
    global t
    out(PROMPT)
    t += 0.55
    type_line(command)
    t += net_pause
    with open(outfile) as f:
        for line in f.read().rstrip("\n").split("\n"):
            out(line + "\r\n")
            t += 0.03
    t += hold

comment("shape any page into JSON: CSS selectors on the left, structure on the right")
cmd("cull '.athing' -j '{title: .titleline a, url: .titleline a @href}' https://news.ycombinator.com | head -3",
    "/tmp/d1.out", net_pause=1.1)

comment("HTML tables -> CSV in one flag (colspan/rowspan and multi-row headers handled)")
cmd("cull 'table.sortable' --table https://en.wikipedia.org/wiki/List_of_tallest_buildings | head -3",
    "/tmp/d2.out", net_pause=1.2)

comment("or any page -> Markdown, ready to pipe into an LLM")
cmd("cull body --md https://example.com", "/tmp/d3.out", net_pause=0.8, hold=3.5)

out(PROMPT)
t += 2.0

header = {"version": 2, "width": W, "height": H,
          "title": "cull — jq for HTML",
          "env": {"SHELL": "/bin/bash", "TERM": "xterm-256color"}}
with open("/tmp/cull-demo.cast", "w") as f:
    f.write(json.dumps(header) + "\n")
    for e in events:
        f.write(json.dumps(e) + "\n")
print(f"cast written, {len(events)} events, duration {t:.1f}s")
