"""Generate the HEYU 鹤羽 icon set.

Run from the repo root:   python assets\\make_icon.py

Design
------
A squircle tile in deep ink-indigo fading to teal carries a single white
crane flight feather (鹤羽) sweeping from lower-left to upper-right, with a
small vermilion disc (丹顶, the red-crowned crane's crown / a seal-like sun)
floating in the open upper-left space.

Every size is rendered from vector geometry (sampled Bezier curves, polygons,
ellipses) at a large supersampled resolution; each layer mask is reduced to
the target size on its own and composited at the final resolution.  Small
sizes (16-24 px) use simplified, bolder geometry and a light alpha-sharpening
curve so the glyph stays crisp instead of turning into a blur.

Outputs (written next to this script):
  heyu.ico            16,20,24,32,40,48,64,128,256 (each size rendered separately)
  heyu.png            512x512
  tray_light.ico      dark glyph for light taskbars  (16..48)
  tray_dark.ico       white glyph for dark taskbars  (16..48)
  tray_light_32.png / tray_dark_32.png
  icon_preview.png    contact sheet (1:1 and 4x nearest-neighbour, light & dark)
"""
from __future__ import annotations

import math
import os

from PIL import Image, ImageChops, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))

# --------------------------------------------------------------------------
# palette
INK_TOP = (27, 36, 66)       # deep ink indigo (top-left)
INK_BOT = (14, 82, 86)       # deep teal (bottom-right)
FEATHER = (250, 248, 242)    # warm paper white
VERMILION = (228, 64, 42)    # 丹顶 red
GLYPH_DARK = (30, 36, 46)    # tray glyph for light taskbars
GLYPH_LIGHT = (255, 255, 255)

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
TRAY_SIZES = [16, 20, 24, 32, 40, 48]


# --------------------------------------------------------------------------
# geometry helpers (unit coordinates, y down)
def bezier(p, t):
    p0, p1, p2, p3 = p
    u = 1 - t
    a, b, c, d = u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t
    return (a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
            a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1])


def bezier_dir(p, t):
    p0, p1, p2, p3 = p
    u = 1 - t
    x = 3 * u * u * (p1[0] - p0[0]) + 6 * u * t * (p2[0] - p1[0]) + 3 * t * t * (p3[0] - p2[0])
    y = 3 * u * u * (p1[1] - p0[1]) + 6 * u * t * (p2[1] - p1[1]) + 3 * t * t * (p3[1] - p2[1])
    n = math.hypot(x, y) or 1.0
    return x / n, y / n


def squircle(cx, cy, r, n=5.0, steps=720):
    pts = []
    for i in range(steps):
        a = 2 * math.pi * i / steps
        c, s = math.cos(a), math.sin(a)
        pts.append((cx + r * math.copysign(abs(c) ** (2 / n), c),
                    cy + r * math.copysign(abs(s) ** (2 / n), s)))
    return pts


def smoothstep(e0, e1, x):
    t = min(1.0, max(0.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


# --------------------------------------------------------------------------
# the feather
#   detail 0 = tiny (16-24): bold vane, one notch, no shaft line
#   detail 1 = small (32-48): shaft line, two notches
#   detail 2 = full (64+):   fine shaft, three swept notches
SPEC = {
    0: dict(wl=0.158, wr=0.100, t0=0.18, quill=0.072, shaft=0.0,
            notches=[(0.50, 0.66, +1, 0.034)]),
    1: dict(wl=0.142, wr=0.102, t0=0.19, quill=0.046, shaft=0.036, shaft_px=1.9,
            shaft_start=0.07, shaft_end=0.80,
            notches=[(0.51, 0.645, +1, 0.024)]),
    2: dict(wl=0.132, wr=0.078, t0=0.20, quill=0.030, shaft=0.019,
            notches=[(0.47, 0.585, +1, 0.014), (0.66, 0.765, +1, 0.011),
                     (0.41, 0.535, -1, 0.014)]),
}


class Feather:
    def __init__(self, detail, scale=1.0, offset=(0.0, 0.0)):
        self.spec = SPEC[detail]
        self.detail = detail
        self.s = scale
        ox, oy = offset

        def T(p):
            return (0.5 + (p[0] - 0.5) * scale + ox, 0.5 + (p[1] - 0.5) * scale + oy)

        # shaft: base lower-left -> tip upper-right, bowing gently (tip curls up)
        self.P = [T((0.250, 0.845)), T((0.410, 0.640)),
                  T((0.630, 0.505)), T((0.800, 0.160))]
        self.t0 = self.spec["t0"]

    def pt(self, t):
        return bezier(self.P, t)

    def nrm(self, t):
        dx, dy = bezier_dir(self.P, t)
        return dy, -dx  # points to the upper-left side of the shaft

    @staticmethod
    def profile(u):
        """Vane half-width, u in [0,1] from vane base to tip (max ~1)."""
        if u <= 0 or u >= 1:
            return 0.0
        rise = math.sin(min(u / 0.30, 1.0) * math.pi / 2) ** 0.85
        fall = (1 - u) ** 0.55
        tip = math.sin(min(1.0, (1 - u) / 0.16) * math.pi / 2) ** 0.8
        return rise * fall * tip * 1.12

    def edge(self, t, side, extra=0.0):
        u = (t - self.t0) / (1 - self.t0)
        w = (self.spec["wl"] if side > 0 else self.spec["wr"]) * self.profile(u)
        w = (w + extra) * self.s
        x, y = self.pt(t)
        nx, ny = self.nrm(t)
        return x + nx * w * side, y + ny * w * side

    def vane(self, n=500):
        ts = [self.t0 + (1 - self.t0) * i / n for i in range(n + 1)]
        return [self.edge(t, +1) for t in ts] + [self.edge(t, -1) for t in reversed(ts)]

    def notch(self, t_in, t_out, side, gap):
        """Swept wedge split: starts as a hairline near the shaft at t_in and
        opens toward the vane edge at t_out (barbs point toward the tip)."""
        nx, ny = self.nrm(t_in)
        x, y = self.pt(t_in)
        a = (x + nx * 0.016 * self.s * side, y + ny * 0.016 * self.s * side)
        pts_l, pts_r = [], []
        for i in range(41):
            k = i / 40
            # bend the split slightly (concave toward the base), like a barb
            tl = t_in + (t_out - gap - t_in) * k
            tr = t_in + (t_out + gap - t_in) * k
            el = self.edge(t_out - gap, side, 0.04)
            er = self.edge(t_out + gap, side, 0.04)
            bend = math.sin(k * math.pi) * 0.012 * self.s
            mx, my = self.nrm((tl + tr) / 2)
            dx, dy = bezier_dir(self.P, (tl + tr) / 2)
            pts_l.append((a[0] + (el[0] - a[0]) * k - dx * bend,
                          a[1] + (el[1] - a[1]) * k - dy * bend))
            pts_r.append((a[0] + (er[0] - a[0]) * k - dx * bend,
                          a[1] + (er[1] - a[1]) * k - dy * bend))
        return pts_l + pts_r[::-1]

    def shaft_line(self, n=300):
        """Tapered shaft groove inside the vane."""
        sw = self.spec["shaft"] * self.s
        t_a = self.t0 + self.spec.get("shaft_start", 0.005)
        t_b = self.spec.get("shaft_end", 0.90)
        left, right = [], []
        for i in range(n + 1):
            k = i / n
            t = t_a + (t_b - t_a) * k
            w = sw * (1 - k) ** 0.9 * 0.5 * smoothstep(0.0, 0.04, k) + 1e-4
            x, y = self.pt(t)
            nx, ny = self.nrm(t)
            left.append((x + nx * w, y + ny * w))
            right.append((x - nx * w, y - ny * w))
        return left + right[::-1]

    def quill(self, n=160):
        """Bare calamus below the vane, tapering to a rounded base."""
        qw = self.spec["quill"] * self.s
        t_b = self.t0 + 0.05
        left, right = [], []
        for i in range(n + 1):
            k = i / n
            t = t_b * k
            w = qw * 0.5 * (0.55 + 0.45 * k)
            x, y = self.pt(t)
            nx, ny = self.nrm(t)
            left.append((x + nx * w, y + ny * w))
            right.append((x - nx * w, y - ny * w))
        bx, by = self.pt(0.0)
        r = qw * 0.5 * 0.55
        return left + right[::-1], (bx, by, r)


def feather_mask(S, detail, scale=1.0, offset=(0.0, 0.0), size=None):
    f = Feather(detail, scale, offset)
    if size and "shaft_px" in f.spec:
        # hint the groove to a fixed pixel width so it reads as a continuous line
        f.spec = dict(f.spec, shaft=f.spec["shaft_px"] / size / scale)
    P = lambda pts: [(x * S, y * S) for x, y in pts]
    m = Image.new("L", (S, S), 0)
    ImageDraw.Draw(m).polygon(P(f.vane()), fill=255)

    cut = Image.new("L", (S, S), 0)
    dc = ImageDraw.Draw(cut)
    for spec in f.spec["notches"]:
        dc.polygon(P(f.notch(*spec)), fill=255)
    if f.spec["shaft"] > 0:
        dc.polygon(P(f.shaft_line()), fill=255)
    m = ImageChops.subtract(m, cut)

    q, (bx, by, r) = f.quill()
    d = ImageDraw.Draw(m)
    d.polygon(P(q), fill=255)
    d.ellipse([(bx - r) * S, (by - r) * S, (bx + r) * S, (by + r) * S], fill=255)
    return m


DOT = {0: (0.300, 0.300, 0.092), 1: (0.305, 0.305, 0.070), 2: (0.310, 0.310, 0.058)}


def dot_mask(S, detail, scale=1.0, offset=(0.0, 0.0)):
    cx, cy, r = DOT[detail]
    cx = 0.5 + (cx - 0.5) * scale + offset[0]
    cy = 0.5 + (cy - 0.5) * scale + offset[1]
    r *= scale
    m = Image.new("L", (S, S), 0)
    ImageDraw.Draw(m).ellipse([(cx - r) * S, (cy - r) * S, (cx + r) * S, (cy + r) * S], fill=255)
    return m


# --------------------------------------------------------------------------
# rendering
def detail_for(size):
    return 0 if size <= 24 else (1 if size <= 48 else 2)


def reduce_mask(m, size):
    """Downsample a supersampled mask; sharpen edges a little at tiny sizes."""
    if size <= 24:
        r = m.resize((size, size), Image.BOX)
        # gentle contrast on coverage: crisper edges, still anti-aliased
        return r.point(lambda v: int(255 * smoothstep(0.12, 0.88, v / 255)))
    if size <= 48:
        r = m.resize((size, size), Image.BOX)
        return r.point(lambda v: int(255 * smoothstep(0.06, 0.94, v / 255)))
    return m.resize((size, size), Image.LANCZOS)


def gradient(size):
    g = Image.new("RGB", (size, size))
    px = g.load()
    for y in range(size):
        for x in range(size):
            fx, fy = (x + 0.5) / size, (y + 0.5) / size
            t = smoothstep(0.0, 1.0, fx * 0.35 + fy * 0.65)
            c = [INK_TOP[i] + (INK_BOT[i] - INK_TOP[i]) * t for i in range(3)]
            l = max(0.0, 1 - math.hypot(fx - 0.30, fy - 0.20) / 0.80) ** 2 * 16
            px[x, y] = tuple(int(min(255, v + l)) for v in c)
    return g


def solid(size, color, mask):
    im = Image.new("RGBA", (size, size), color + (0,))
    im.putalpha(mask)
    return im


def render_app(size):
    detail = detail_for(size)
    ss = max(8, 2048 // size)
    S = size * ss

    margin = 0.0 if size <= 24 else (0.03 if size <= 48 else 0.055)
    r = 0.5 - margin
    tile_hi = Image.new("L", (S, S), 0)
    ImageDraw.Draw(tile_hi).polygon(
        [(x * S, y * S) for x, y in squircle(0.5, 0.5, r, n=4.4 if size <= 24 else 5.0)], fill=255)
    tile = reduce_mask(tile_hi, size) if size > 24 else tile_hi.resize((size, size), Image.BOX)

    img = gradient(size).convert("RGBA")
    img.putalpha(tile)

    if size >= 64:  # faint inner rim light
        inner = Image.new("L", (S, S), 0)
        ImageDraw.Draw(inner).polygon(
            [(x * S, y * S) for x, y in squircle(0.5, 0.5, r - 0.007, n=5.0)], fill=255)
        rim = reduce_mask(ImageChops.subtract(tile_hi, inner), size)
        img = Image.alpha_composite(img, solid(size, (255, 255, 255), rim.point(lambda v: v * 30 // 255)))

    scale = (1.0 - 2 * margin) * {0: 1.06, 1: 1.04, 2: 0.97}[detail]
    fm = reduce_mask(ImageChops.multiply(feather_mask(S, detail, scale, size=size), tile_hi), size)
    img = Image.alpha_composite(img, solid(size, FEATHER, fm))
    dm = reduce_mask(dot_mask(S, detail, scale), size)
    img = Image.alpha_composite(img, solid(size, VERMILION, dm))
    return img


def render_tray(size, color):
    detail = detail_for(size)
    ss = max(8, 2048 // size)
    S = size * ss
    scale = {0: 1.20, 1: 1.16, 2: 1.12}[detail]
    off = (0.0, 0.01)
    img = solid(size, color, reduce_mask(feather_mask(S, detail, scale, off, size=size), size))
    dm = reduce_mask(dot_mask(S, detail, scale, off), size)
    return Image.alpha_composite(img, solid(size, VERMILION, dm))


# --------------------------------------------------------------------------
def save_ico(path, images):
    images = sorted(images, key=lambda im: im.size[0])
    images[-1].save(path, format="ICO", sizes=[im.size for im in images],
                    append_images=images[:-1])


def contact_sheet(app, tray_l, tray_d, path):
    pad, zoom = 24, 4
    small = [im for im in app if im.size[0] <= 64]
    w_1x = sum(im.size[0] for im in app) + pad * (len(app) + 1)
    w_4x = sum(im.size[0] * zoom for im in small) + pad * (len(small) + 1)
    w_tr = sum(im.size[0] * (zoom + 1) for im in tray_l) + pad * (2 * len(tray_l) + 1)
    W = max(w_1x, w_4x, w_tr)
    H_band = (256 + pad) + (64 * zoom + pad) + (48 * zoom + pad) + pad
    out = Image.new("RGBA", (W, 2 * H_band))
    for band, (bgc, tray) in enumerate([((243, 243, 239), tray_l), ((30, 31, 36), tray_d)]):
        y0 = band * H_band
        out.paste(bgc + (255,), (0, y0, W, y0 + H_band))
        # row 1: everything at 1:1, bottom-aligned
        x, base = pad, y0 + pad + 256
        for im in app:
            out.alpha_composite(im, (x, base - im.size[1]))
            x += im.size[0] + pad
        # row 2: <=64 px at 4x nearest-neighbour
        x, base = pad, base + pad + 64 * zoom
        for im in small:
            z = im.resize((im.size[0] * zoom,) * 2, Image.NEAREST)
            out.alpha_composite(z, (x, base - z.size[1]))
            x += z.size[0] + pad
        # row 3: tray glyphs at 4x then 1:1
        x, base = pad, base + pad + 48 * zoom
        for im in tray:
            z = im.resize((im.size[0] * zoom,) * 2, Image.NEAREST)
            out.alpha_composite(z, (x, base - z.size[1]))
            x += z.size[0] + pad
        for im in tray:
            out.alpha_composite(im, (x, base - im.size[1]))
            x += im.size[0] + pad
    out.convert("RGB").save(path)


def main():
    app = [render_app(s) for s in ICO_SIZES]
    save_ico(os.path.join(HERE, "heyu.ico"), app)
    render_app(512).save(os.path.join(HERE, "heyu.png"))

    tray_l = [render_tray(s, GLYPH_DARK) for s in TRAY_SIZES]
    tray_d = [render_tray(s, GLYPH_LIGHT) for s in TRAY_SIZES]
    save_ico(os.path.join(HERE, "tray_light.ico"), tray_l)
    save_ico(os.path.join(HERE, "tray_dark.ico"), tray_d)
    tray_l[TRAY_SIZES.index(32)].save(os.path.join(HERE, "tray_light_32.png"))
    tray_d[TRAY_SIZES.index(32)].save(os.path.join(HERE, "tray_dark_32.png"))

    contact_sheet(app, tray_l, tray_d, os.path.join(HERE, "icon_preview.png"))

    for name in ["heyu.ico", "tray_light.ico", "tray_dark.ico"]:
        with Image.open(os.path.join(HERE, name)) as im:
            print(name, sorted(im.info.get("sizes", [])))


if __name__ == "__main__":
    main()
