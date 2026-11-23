"""Generate the Seamless Music Player icon set from assets/icon.svg.

Produces assets/smp.ico (16, 20, 24, 32, 40, 48, 64, 256) and
assets/icon-preview.png (512). Pure Python with Pillow; the SVG paths are
redrawn directly so the small sizes stay crisp.

Run: python -I assets/tools/make-icon.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
BLUE = (0x89, 0xB4, 0xFA)
MAUVE = (0xCB, 0xA6, 0xF7)
CRUST = (0x11, 0x11, 0x1B)
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256]

TILE_R = 112 / 512  # corner radius as a fraction of the tile


def grad_at(x, y, size):
    """Vertical blue to mauve gradient, 0 at the top."""
    t = y / max(size - 1, 1)
    return tuple(round(BLUE[i] + (MAUVE[i] - BLUE[i]) * t) for i in range(3))


def rounded_tile_mask(size):
    """Anti-aliased rounded-square mask for the tile."""
    ss = max(4, 512 // size * 4)  # supersample
    big = size * ss
    m = Image.new("L", (big, big), 0)
    d = ImageDraw.Draw(m)
    r = round(TILE_R * big)
    d.rounded_rectangle([0, 0, big - 1, big - 1], radius=r, fill=255)
    return m.resize((size, size), Image.LANCZOS)


def arc_ring(d: ImageDraw.ImageDraw, cx, cy, r_out, r_in, a0, a1, u, steps=220):
    pts_out, pts_in = [], []
    for i in range(steps + 1):
        a = math.radians(a0 + (a1 - a0) * i / steps)
        pts_out.append((u * (cx + r_out * math.cos(a)), u * (cy + r_out * math.sin(a))))
        pts_in.append((u * (cx + r_in * math.cos(a)), u * (cy + r_in * math.sin(a))))
    d.polygon(pts_out + pts_in[::-1], fill=CRUST)


def draw_mark(d: ImageDraw.ImageDraw, u: float):
    """Draw the ring-and-note mark. u = one unit (tile/512).

    Coordinates follow assets/icon.svg: an annulus ring, a note head, a stem
    and a curved flag, all inside the ring with a clear gap.
    """

    def quad(p0, c, p1, steps):
        return [(u * (p0[0] + (c[0] - p0[0]) * t + ((p1[0] - c[0]) - (p0[0] - c[0])) * t * t),
                 u * (p0[1] + (c[1] - p0[1]) * t + ((p1[1] - c[1]) - (p0[1] - c[1])) * t * t))
                for t in (i / steps for i in range(steps + 1))]

    # ring: annulus 88..424 with a 46.4 unit wall
    arc_ring(d, 256, 256, 168, 121.6, 0, 360, u)
    # note head
    r_head = 40
    steps = 120
    pts = [(u * (225 + r_head * math.cos(2 * math.pi * i / steps)),
            u * (318 + r_head * math.sin(2 * math.pi * i / steps))) for i in range(steps)]
    d.polygon(pts, fill=CRUST)
    # stem
    d.polygon([(u * x, u * y) for x, y in [(258, 148), (294, 148), (294, 318), (258, 318)]], fill=CRUST)
    # flag: two outer curves, one inner curve, closed along the stem edge
    flag = (quad((294, 148), (332, 164), (334, 210), 24)
            + quad((334, 210), (324, 236), (296, 246), 16)
            + quad((296, 246), (298, 216), (294, 186), 16))
    d.polygon(flag, fill=CRUST)


def draw_mark_small(d: ImageDraw.ImageDraw, u: float):
    """Simplified mark for 16 to 24 px: note only, thicker head and stem.

    The ring reads as mud at those sizes, so the small frames keep the mark
    alone, slightly larger and centred.
    """
    head = 46
    steps = 60
    pts = [(u * (240 + head * math.cos(2 * math.pi * i / steps)),
            u * (330 + head * math.sin(2 * math.pi * i / steps))) for i in range(steps)]
    d.polygon(pts, fill=CRUST)
    stem = [(268, 120), (310, 120), (310, 334), (268, 334)]
    flag = [(310, 120), (378, 158), (310, 196)]
    d.polygon([(u * x, u * y) for x, y in stem], fill=CRUST)
    d.polygon([(u * x, u * y) for x, y in flag], fill=CRUST)


def make(size: int) -> Image.Image:
    im = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    u = size / 512
    # tile with the vertical gradient
    px = im.load()
    mask = rounded_tile_mask(size)
    for y in range(size):
        c = grad_at(0, y, size)
        for x in range(size):
            px[x, y] = (*c, 255)
    im.putalpha(mask)
    # mark, supersampled for crisp small sizes
    ss = 6 if size <= 24 else 4 if size <= 48 else 2
    mark = Image.new("RGBA", (size * ss, size * ss), (0, 0, 0, 0))
    dm = ImageDraw.Draw(mark)
    if size <= 24:
        draw_mark_small(dm, u * ss)
    else:
        draw_mark(dm, u * ss)
    if ss > 1:
        mark = mark.resize((size, size), Image.LANCZOS)
    im.alpha_composite(mark)
    return im


def main():
    frames = []
    for s in ICO_SIZES:
        im = make(s)
        frames.append(im)
    ico_path = ROOT / "smp.ico"
    big = max(frames, key=lambda f: f.size[0])
    big.save(ico_path, format="ICO", sizes=[(s, s) for s in ICO_SIZES],
             append_images=[f for f in frames if f is not big])
    preview = make(512)
    preview.save(ROOT / "icon-preview.png")
    print("ok smp.ico", ICO_SIZES)
    print("ok icon-preview.png 512")


if __name__ == "__main__":
    main()
