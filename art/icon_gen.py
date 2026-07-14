#!/usr/bin/env python3
"""Generate the fabric-language-rust icon: pixel Ferris on a woven background."""

CELL = 32  # 16 cells * 32 = 512px

# palette
WEAVE = {
    "light": "#d6cba9",
    "light_shade": "#ccc09c",
    "mid": "#c9bd97",
    "mid_shade": "#bfb28a",
}
CRAB = {
    "o": "#f74c00",   # body orange
    "d": "#b53600",   # dark orange / shading
    "h": "#ff7e38",   # highlight
    "W": "#ffffff",   # eye glint
    "B": "#26170f",   # eye
}

# 16x16 pixel map, '.' = weave background
ROWS = [
    "................",
    "................",
    "................",
    "....o.o..o.o....",
    "...oooooooooo...",
    "..ohhooooooooo..",
    "..oooWBooWBooo..",
    ".ooooBBooBBoooo.",
    ".oooooooooooooo.",
    "oo.oooooooooo.oo",
    "oo..dddddddd..oo",
    "....o.o..o.o....",
    "...o.o....o.o...",
    "................",
    "................",
    "................",
]

def weave_color(x, y):
    bx, by = x // 2, y // 2
    horizontal = (bx + by) % 2 == 0
    if horizontal:
        return WEAVE["light_shade"] if y % 2 == 1 else WEAVE["light"]
    return WEAVE["mid_shade"] if x % 2 == 1 else WEAVE["mid"]

rects = []
for y, row in enumerate(ROWS):
    for x, ch in enumerate(row):
        color = CRAB[ch] if ch in CRAB else weave_color(x, y)
        rects.append(
            f'<rect x="{x*CELL}" y="{y*CELL}" width="{CELL}" height="{CELL}" fill="{color}"/>'
        )

svg = (
    f'<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" '
    f'viewBox="0 0 512 512" shape-rendering="crispEdges">\n'
    + "\n".join(rects)
    + "\n</svg>\n"
)

import sys
out = sys.argv[1] if len(sys.argv) > 1 else "icon.svg"
with open(out, "w") as f:
    f.write(svg)
print(f"wrote {out}")
