#!/usr/bin/env python3
"""Generate the GitHub social preview card (1280x640) for cull."""
from PIL import Image, ImageDraw, ImageFont

W, H = 1280, 640
BG = (13, 17, 23)        # GitHub dark
PANEL = (22, 27, 34)
BORDER = (48, 54, 61)
FG = (230, 237, 243)
DIM = (139, 148, 158)
GREEN = (63, 185, 80)
BLUE = (88, 166, 255)
ORANGE = (255, 166, 87)
PURPLE = (188, 140, 255)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"

img = Image.new("RGB", (W, H), BG)
d = ImageDraw.Draw(img)

f_title = ImageFont.truetype(MONO_B, 88)
f_tag = ImageFont.truetype(MONO, 34)
f_code = ImageFont.truetype(MONO, 27)
f_small = ImageFont.truetype(MONO, 24)

# Title
d.text((80, 60), "cull", font=f_title, fill=GREEN)
tw = d.textlength("cull", font=f_title)
d.text((80 + tw + 30, 96), "— jq for HTML", font=f_tag, fill=FG)
d.text((80, 178), "CSS selectors in → JSON / CSV / Markdown out. One static binary.",
       font=f_small, fill=DIM)

# Terminal panel
px, py, pw, ph = 80, 240, W - 160, 320
d.rounded_rectangle([px, py, px + pw, py + ph], radius=14, fill=PANEL, outline=BORDER, width=2)
# traffic lights
for i, c in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
    d.ellipse([px + 22 + i * 34, py + 20, px + 40 + i * 34, py + 38], fill=c)

x, y, lh = px + 30, py + 64, 40


def line(parts, yy):
    xx = x
    for text, color, font in parts:
        d.text((xx, yy), text, font=font, fill=color)
        xx += d.textlength(text, font=font)


line([("$ ", DIM, f_code),
      ("cull", GREEN, f_code),
      (" https://news.ycombinator.com ", FG, f_code),
      ("'.athing'", ORANGE, f_code), (" \\", DIM, f_code)], y)
line([("    -j ", BLUE, f_code),
      ("'{title: .titleline a, url: .titleline a @href}'", ORANGE, f_code)], y + lh)
line([('{"title":"Show HN: ...","url":"https://..."}', PURPLE, f_code)], y + 2 * lh)
line([('{"title":"...","url":"https://..."}', PURPLE, f_code)], y + 3 * lh)

y2 = y + 4 * lh + 18
line([("$ ", DIM, f_code), ("cull", GREEN, f_code),
      (" page.html ", FG, f_code), ("--table", BLUE, f_code),
      ("        ", FG, f_code), ("# any <table> → CSV", DIM, f_code)], y2)
line([("$ ", DIM, f_code), ("cull", GREEN, f_code),
      (" https://any.site ", FG, f_code), ("--md", BLUE, f_code),
      ("   ", FG, f_code), ("# page → Markdown for LLMs", DIM, f_code)], y2 + lh)

# Footer
d.text((80, 600), "github.com/rashida-thorne/cull", font=f_small, fill=DIM)
foot = "cargo install cull"
fw = d.textlength(foot, font=f_small)
d.text((W - 80 - fw, 600), foot, font=f_small, fill=DIM)

img.save("/home/agent/workspace/repo/assets/social-preview.png", optimize=True)
print("wrote assets/social-preview.png")
