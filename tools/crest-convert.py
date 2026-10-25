#!/usr/bin/env python3
"""Converts the Mod 2026 club logos into crest pictures of the original size and palette.

Usage: tools/crest-convert.py [--data DIR] [--cache DIR] [--hires] [CREST ...]

Reads the logo URLs from docs/mod2026/crests/SOURCES.md, downloads each logo once into the
cache directory (default ~/.cache/bmp-crest-logos; the logos are never committed) and writes
docs/mod2026/crests/<crest>.png for the given crest numbers (default: every row).

The original crest pictures PIC/80.VGA..PIC/143.VGA of the data directory (default $BMP_DATA)
give the size and the colour indices of the original crests; PIC/1.PAL gives their colours.
Besides those a crest may use the mod's own crest colours, palette entries 64..127 (no original
picture uses an entry above 31; the mod writes them into its 1.PAL and fades them with 0..31).
The logo is trimmed to its visible part, laid on the crest background colour, scaled to fit
the crest with a box filter and every pixel is mapped to the allowed colour that keeps its hue
best (a CIE Lab distance that weighs hue and chroma more for saturated colours). The PNG is
indexed with a 128-entry palette: entries 0..31 of 1.PAL, black, then the crest colours.

The crest colours are read from the committed docs/mod2026/crests/80.png. With --new-colours
the tool computes them first: 64 k-means centres of the logo pixels that the original crest
colours show worst, in 6-bit DAC precision; then every crest has to be converted again.
Needs Pillow and NumPy.

With --hires the tool writes docs/mod2026/crests/hires/<crest>.png instead: the same logo at
twice the crest width and height, for the 2x display layer. Its pixels may use only the colour
indices the committed crests docs/mod2026/crests/<n>.png use, so the 2x crest never shows a
colour its crest lacks.
"""
import argparse
from collections import Counter
import io
import os
import re
import struct
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import numpy as np
from PIL import Image

REPO = Path(__file__).resolve().parent.parent
CRESTS = REPO / "docs" / "mod2026" / "crests"
FIRST_CREST, LAST_CREST = 80, 143
# Index 29 (light grey) is the background of the original crests.
BACKGROUND = 29
# Wikimedia renders an SVG as PNG at a fixed set of widths; 500 is one of them.
SVG_RENDER_WIDTH = 500
# The mod's own crest colours (palette entries 64..127).
MOD_FIRST, MOD_COUNT = 64, 64
PNG_ENTRIES = MOD_FIRST + MOD_COUNT
USER_AGENT = "bmp-remake-crest-convert/1.0 (crest conversion for a private game mod)"
SOURCE_ROW = re.compile(r"^\| (\d+) \| [^|]+ \| (https://\S+) \| \d{4}-\d\d-\d\d \|$")


def decode_rle(data):
    """The .VGA RLE stream (docs/re/vga.md)."""
    out, i = bytearray(), 0
    while True:
        head = data[i]
        i += 1
        if head & 0x80 == 0:
            out += data[i : i + head + 1]
            i += head + 1
        elif head & 0x7F == 0:
            return bytes(out)
        else:
            out += bytes([data[i]]) * ((head & 0x7F) + 1)
            i += 1


def read_vga(path):
    data = path.read_bytes()
    width, height, packed_len = struct.unpack_from("<HHH", data)
    return width, height, decode_rle(data[6 : 6 + packed_len])


def crest_format(data_dir):
    """Size, allowed indices and the 8-bit colours of palette entries 0..31."""
    size, used = None, set()
    for crest in range(FIRST_CREST, LAST_CREST + 1):
        width, height, pixels = read_vga(data_dir / "PIC" / f"{crest}.VGA")
        if size not in (None, (width, height)):
            sys.exit(f"PIC/{crest}.VGA is {width}x{height}, not {size[0]}x{size[1]}")
        size = (width, height)
        used.update(pixels)
    dac = (data_dir / "PIC" / "1.PAL").read_bytes()[: 32 * 3]
    palette = [(v << 2) | (v >> 4) for v in dac]
    return size, sorted(used), palette


def crest_indices(rows):
    """The colour indices the committed crests of `rows` use together."""
    used = set()
    for crest in rows:
        path = CRESTS / f"{crest}.png"
        if not path.exists():
            sys.exit(f"{path} is missing: make the crests before their 2x versions")
        used.update(Image.open(path).getdata())
    return sorted(used)


def sources():
    rows = {}
    for line in (CRESTS / "SOURCES.md").read_text(encoding="utf-8").splitlines():
        if not line.startswith("|") or line.startswith(("| Crest |", "|---")):
            continue
        match = SOURCE_ROW.match(line)
        if not match:
            sys.exit(f"bad SOURCES.md row: {line}")
        rows[int(match[1])] = match[2]
    return rows


def download_url(url):
    """The URL to fetch: Wikimedia's PNG rendering for an SVG, else the file itself."""
    parts = urllib.parse.urlsplit(url)
    if not parts.path.endswith(".svg"):
        return url
    project, file_path = re.match(r"^(/wikipedia/[^/]+)/(.+)$", parts.path).groups()
    name = file_path.rsplit("/", 1)[1]
    thumb = f"{project}/thumb/{file_path}/{SVG_RENDER_WIDTH}px-{name}.png"
    return urllib.parse.urlunsplit((parts.scheme, parts.netloc, thumb, "", ""))


def download(url):
    """Wikimedia answers 429 to quick successive requests: pause before each one and wait
    as long as it asks."""
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    for _ in range(5):
        time.sleep(1)
        try:
            with urllib.request.urlopen(request) as response:
                return response.read()
        except urllib.error.HTTPError as error:
            if error.code != 429:
                raise
            time.sleep(int(error.headers.get("Retry-After", "10")))
    sys.exit(f"{url}: still rate limited after 5 attempts")


def fetch(url, cache):
    url = download_url(url)
    parts = urllib.parse.urlsplit(url)
    path = cache / parts.netloc / urllib.parse.unquote(parts.path).lstrip("/")
    if not path.exists():
        data = download(url)
        try:
            Image.open(io.BytesIO(data)).verify()
        except Exception as error:
            sys.exit(f"{url}: not an image ({error})")
        path.parent.mkdir(parents=True, exist_ok=True)
        partial = path.with_name(path.name + ".part")
        partial.write_bytes(data)
        partial.replace(path)
    return Image.open(io.BytesIO(path.read_bytes())).convert("RGBA")


def lab(rgb):
    """CIE L*a*b* (D65) of an 8-bit sRGB colour."""

    def linear(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (linear(c) for c in rgb)
    x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047
    y = 0.2126 * r + 0.7152 * g + 0.0722 * b
    z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883

    def f(t):
        return t ** (1 / 3) if t > 0.008856 else 7.787 * t + 16 / 116

    return (116 * f(y) - 16, 500 * (f(x) - f(y)), 200 * (f(y) - f(z)))


def distance(source, target):
    """Lab distance from a logo colour to a palette colour; for a saturated logo colour a
    loss of chroma or a hue shift counts up to four and seven times, so a green or violet
    logo does not turn into the nearest grey-blue."""
    (l1, a1, b1), (l2, a2, b2) = source, target
    c1, c2 = (a1 * a1 + b1 * b1) ** 0.5, (a2 * a2 + b2 * b2) ** 0.5
    dc = c1 - c2
    dh2 = max(0.0, (a1 - a2) ** 2 + (b1 - b2) ** 2 - dc * dc)
    w = min(1.0, c1 / 30)
    return (l1 - l2) ** 2 + (1 + 3 * w) * dc * dc + (1 + 6 * w) * dh2


def nearest(rgb, allowed, palette):
    """The allowed index whose colour keeps `rgb` best ([`distance`])."""
    source = lab(rgb)
    return min(allowed, key=lambda i: distance(source, lab(palette[3 * i : 3 * i + 3])))


def to_8bit(v):
    """A 6-bit DAC value as the 8-bit value the crest PNGs use."""
    return (v << 2) | (v >> 4)


def png_palette(palette, colours):
    """The 128-entry crest PNG palette: 1.PAL 0..31, black, the crest colours."""
    out = list(palette[: 32 * 3]) + [0] * (3 * (MOD_FIRST - 32))
    for rgb in colours:
        out += rgb
    return out


def committed_colours():
    path = CRESTS / f"{FIRST_CREST}.png"
    if not path.exists():
        sys.exit(f"{path} is missing: run with --new-colours")
    flat = Image.open(path).getpalette()[3 * MOD_FIRST : 3 * PNG_ENTRIES]
    if len(flat) != 3 * MOD_COUNT:
        sys.exit(f"{path} has no crest colours: run with --new-colours")
    return [tuple(flat[i : i + 3]) for i in range(0, len(flat), 3)]


def new_colours(logos, size, allowed, palette):
    """64 crest colours: k-means (fixed start, 30 rounds) in Lab over the logo pixels at 2x
    crest size whose best original crest colour is worse than distance 150, quantised to the
    6-bit DAC."""
    originals = [lab(palette[3 * i : 3 * i + 3]) for i in allowed]
    counts = Counter()
    for logo in logos:
        counts.update(place(logo, size, palette).getdata())
    samples, weights = [], []
    for rgb, count in counts.items():
        source = lab(rgb)
        if min(distance(source, o) for o in originals) > 150:
            samples.append(source)
            weights.append(count)
    points, weights = np.array(samples), np.array(weights, dtype=float)
    order = np.argsort(points[:, 0] * 1e6 + points[:, 1] * 1e3 + points[:, 2], kind="stable")
    centres = points[order[np.linspace(0, len(order) - 1, MOD_COUNT).astype(int)]].copy()
    for _ in range(30):
        labels = ((points[:, None, :] - centres[None, :, :]) ** 2).sum(axis=2).argmin(axis=1)
        for k in range(MOD_COUNT):
            members = labels == k
            if members.any():
                centres[k] = np.average(points[members], axis=0, weights=weights[members])
    colours = []
    for centre in centres:
        rgb = lab_to_rgb(centre)
        colours.append(tuple(to_8bit(round(c * 63 / 255)) for c in rgb))
    return sorted(set(colours)) + [(0, 0, 0)] * (MOD_COUNT - len(set(colours)))


def lab_to_rgb(value):
    l, a, b = value
    fy = (l + 16) / 116
    fx, fz = fy + a / 500, fy - b / 200

    def inverse(t):
        return t**3 if t**3 > 0.008856 else (t - 16 / 116) / 7.787

    x, y, z = inverse(fx) * 0.95047, inverse(fy), inverse(fz) * 1.08883
    r = 3.2406 * x - 1.5372 * y - 0.4986 * z
    g = -0.9689 * x + 1.8758 * y + 0.0415 * z
    bl = 0.0557 * x - 0.2040 * y + 1.0570 * z

    def gamma(c):
        c = max(0.0, min(1.0, c))
        return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055

    return tuple(round(255 * gamma(c)) for c in (r, g, bl))


def place(logo, size, palette):
    """The logo trimmed, on the crest background and scaled into a `size` canvas."""
    logo = logo.crop(logo.getchannel("A").getbbox())
    background = tuple(palette[3 * BACKGROUND : 3 * BACKGROUND + 3])
    flat = Image.new("RGBA", logo.size, background + (255,))
    flat.alpha_composite(logo)
    scale = min(size[0] / logo.width, size[1] / logo.height)
    fitted = (max(1, round(logo.width * scale)), max(1, round(logo.height * scale)))
    small = flat.convert("RGB").resize(fitted, Image.BOX)
    canvas = Image.new("RGB", size, background)
    canvas.paste(small, ((size[0] - fitted[0]) // 2, (size[1] - fitted[1]) // 2))
    return canvas


def convert(logo, size, allowed, palette):
    canvas = place(logo, size, palette)
    chosen = {}
    indices = bytes(
        chosen.setdefault(rgb, nearest(rgb, allowed, palette)) for rgb in canvas.getdata()
    )
    crest = Image.frombytes("P", size, indices)
    crest.putpalette(palette)
    return crest


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--data", type=Path, default=os.environ.get("BMP_DATA"))
    parser.add_argument("--cache", type=Path, default=Path.home() / ".cache" / "bmp-crest-logos")
    parser.add_argument("--hires", action="store_true", help="write the 2x crests")
    parser.add_argument(
        "--new-colours", action="store_true", help="compute the crest colours first"
    )
    parser.add_argument("crests", type=int, nargs="*")
    args = parser.parse_args()
    if args.data is None:
        sys.exit("no data directory: pass --data or set BMP_DATA")
    size, allowed, palette = crest_format(args.data)
    rows = sources()
    if args.new_colours:
        logos = [fetch(rows[crest], args.cache) for crest in sorted(rows)]
        colours = new_colours(logos, (2 * size[0], 2 * size[1]), allowed, palette)
    else:
        colours = committed_colours()
    palette = png_palette(palette, colours)
    allowed = allowed + list(range(MOD_FIRST, PNG_ENTRIES))
    out = CRESTS
    if args.hires:
        size = (2 * size[0], 2 * size[1])
        allowed = crest_indices(sorted(rows))
        out = CRESTS / "hires"
        out.mkdir(exist_ok=True)
    for crest in args.crests or sorted(rows):
        if crest not in rows:
            sys.exit(f"crest {crest} has no row in SOURCES.md")
        logo = fetch(rows[crest], args.cache)
        if logo.getchannel("A").getbbox() is None:
            sys.exit(f"crest {crest}: logo {rows[crest]} is fully transparent")
        image = convert(logo, size, allowed, palette)
        image.save(out / f"{crest}.png", optimize=True)
        print(f"{crest}.png")


if __name__ == "__main__":
    main()
