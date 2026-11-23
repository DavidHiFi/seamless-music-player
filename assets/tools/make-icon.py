"""Generate the Seamless Music Player icon set.

Same family as Etcher's icon: an inset rounded tile with a vertical blue
#89B4FA to mauve #CBA6F7 gradient and one bold Mocha base #1E1E2E mark.
The mark is a pair of beamed eighth notes, built only from ellipses and
straight-edged polygons so every size stays clean.

Produces assets/smp.ico (16, 20, 24, 32, 40, 48, 64, 256),
assets/icon-preview.png (512) and assets/icon.svg.

Run: python -I assets/tools/make-icon.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
BLUE = (0x89, 0xB4, 0xFA)
MAUVE = (0xCB, 0xA6, 0xF7)
MARK = (0x1E, 0x1E, 0x2E, 255)
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256]

INSET = 18      # tile inset in 512 units, as Etcher
RADIUS = 60     # tile corner radius in 512 units
DX = -6         # nudge to centre the mark optically

HEAD_RX, HEAD_RY, HEAD_TILT = 60, 43, -22
HEADS = [(178, 372), (356, 332)]
STEM_W = 36
BEAM_T = 66
# stem top points sit under the beam; the beam runs from the left stem to the right stem
BEAM_L = (204, 150)
BEAM_R = (414, 104)


def ellipse_pts(cx, cy, rx, ry, tilt_deg, steps=160):
    t = math.radians(tilt_deg)
    ct, st = math.cos(t), math.sin(t)
    pts = []
    for i in range(steps):
        a = 2 * math.pi * i / steps
        x, y = rx * math.cos(a), ry * math.sin(a)
        pts.append((cx + x * ct - y * st, cy + x * st + y * ct))
    return pts


def head_right_x(cx, cy):
    """x of the head's right-most point and its y, so the stem meets it flush."""
    pts = ellipse_pts(cx, cy, HEAD_RX, HEAD_RY, HEAD_TILT, 720)
    return max(pts, key=lambda p: p[0])


def mark_shapes():
    """Polygons of the mark in 512 units."""
    shapes = []
    stems = []
    for (cx, cy) in HEADS:
        shapes.append(ellipse_pts(cx + DX, cy, HEAD_RX, HEAD_RY, HEAD_TILT))
        rx, ry = head_right_x(cx + DX, cy)
        stems.append((rx - STEM_W, rx, ry))
    # the beam spans exactly from the left stem's left edge to the right stem's right edge
    lx, rx_ = stems[0][0], stems[1][1]
    slope = (BEAM_R[1] - BEAM_L[1]) / (BEAM_R[0] - BEAM_L[0])
    ly = BEAM_L[1] + slope * (lx - (BEAM_L[0] + DX))
    ry_ = BEAM_L[1] + slope * (rx_ - (BEAM_L[0] + DX))
    beam_top = lambda x: ly + slope * (x - lx)
    for x0, x1, foot in stems:
        shapes.append([(x0, beam_top(x0)), (x1, beam_top(x1)), (x1, foot), (x0, foot + 6)])
    shapes.append([(lx, ly), (rx_, ry_), (rx_, ry_ + BEAM_T), (lx, ly + BEAM_T)])
    return shapes


def make(size: int) -> Image.Image:
    ss = 8 if size <= 64 else 4
    big = size * ss
    u = big / 512
    tile = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    grad = Image.new("RGBA", (big, big))
    gp = grad.load()
    for y in range(big):
        t = min(max((y / u - INSET) / (512 - 2 * INSET), 0), 1)
        c = tuple(round(BLUE[i] + (MAUVE[i] - BLUE[i]) * t) for i in range(3)) + (255,)
        for x in range(big):
            gp[x, y] = c
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [INSET * u, INSET * u, (512 - INSET) * u - 1, (512 - INSET) * u - 1],
        radius=RADIUS * u, fill=255)
    tile.paste(grad, (0, 0), mask)
    d = ImageDraw.Draw(tile)
    for poly in mark_shapes():
        d.polygon([(x * u, y * u) for x, y in poly], fill=MARK)
    return tile.resize((size, size), Image.LANCZOS)


def write_svg():
    def path(poly):
        return "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in poly) + "Z"
    paths = "\n    ".join(f'<path d="{path(p)}"/>' for p in mark_shapes())
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <!-- Seamless Music Player: beamed notes on a blue to mauve tile (Etcher family). -->
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#89B4FA"/>
      <stop offset="1" stop-color="#CBA6F7"/>
    </linearGradient>
  </defs>
  <rect x="{INSET}" y="{INSET}" width="{512 - 2 * INSET}" height="{512 - 2 * INSET}" rx="{RADIUS}" fill="url(#tile)"/>
  <g fill="#1E1E2E">
    {paths}
  </g>
</svg>
"""
    (ROOT / "icon.svg").write_text(svg, encoding="utf-8")


def main():
    frames = [make(s) for s in ICO_SIZES]
    big = frames[-1]
    big.save(ROOT / "smp.ico", format="ICO", sizes=[(s, s) for s in ICO_SIZES],
             append_images=frames[:-1])
    make(512).save(ROOT / "icon-preview.png")
    write_svg()
    print("ok smp.ico", ICO_SIZES)
    print("ok icon-preview.png 512, icon.svg")


if __name__ == "__main__":
    main()
