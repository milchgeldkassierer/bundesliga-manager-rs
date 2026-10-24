#!/usr/bin/env python3
"""Converts the Mod 2026 club logos into crest pictures of the original size and palette.

Usage: tools/crest-convert.py [--data DIR] [--cache DIR] [CREST ...]

Reads the logo URLs from docs/mod2026/crests/SOURCES.md, downloads each logo once into the
cache directory (default ~/.cache/bmp-crest-logos; the logos are never committed) and writes
docs/mod2026/crests/<crest>.png for the given crest numbers (default: every row).

The original crest pictures PIC/80.VGA..PIC/143.VGA of the data directory (default $BMP_DATA)
give the size and the colour indices a crest may use; PIC/1.PAL gives their colours. The logo
is trimmed to its visible part, laid on the crest background colour, scaled to fit the crest
with a box filter and every pixel is mapped to the nearest allowed colour. The PNG is indexed
with the palette entries 0..31 of 1.PAL. Needs Pillow.
"""
import argparse
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

from PIL import Image

REPO = Path(__file__).resolve().parent.parent
CRESTS = REPO / "docs" / "mod2026" / "crests"
FIRST_CREST, LAST_CREST = 80, 143
# Index 29 (light grey) is the background of the original crests.
BACKGROUND = 29
# Wikimedia renders an SVG as PNG at a fixed set of widths; 500 is one of them.
SVG_RENDER_WIDTH = 500
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


def nearest(rgb, allowed, palette):
    """The allowed index closest to `rgb` (weighted "redmean" RGB distance)."""

    def distance(index):
        r, g, b = palette[3 * index : 3 * index + 3]
        mean = (rgb[0] + r) / 2
        dr, dg, db = rgb[0] - r, rgb[1] - g, rgb[2] - b
        return (2 + mean / 256) * dr * dr + 4 * dg * dg + (3 - mean / 256) * db * db

    return min(allowed, key=distance)


def convert(logo, size, allowed, palette):
    logo = logo.crop(logo.getchannel("A").getbbox())
    background = tuple(palette[3 * BACKGROUND : 3 * BACKGROUND + 3])
    flat = Image.new("RGBA", logo.size, background + (255,))
    flat.alpha_composite(logo)
    scale = min(size[0] / logo.width, size[1] / logo.height)
    fitted = (max(1, round(logo.width * scale)), max(1, round(logo.height * scale)))
    small = flat.convert("RGB").resize(fitted, Image.BOX)
    canvas = Image.new("RGB", size, background)
    canvas.paste(small, ((size[0] - fitted[0]) // 2, (size[1] - fitted[1]) // 2))
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
    parser.add_argument("crests", type=int, nargs="*")
    args = parser.parse_args()
    if args.data is None:
        sys.exit("no data directory: pass --data or set BMP_DATA")
    size, allowed, palette = crest_format(args.data)
    rows = sources()
    for crest in args.crests or sorted(rows):
        if crest not in rows:
            sys.exit(f"crest {crest} has no row in SOURCES.md")
        logo = fetch(rows[crest], args.cache)
        if logo.getchannel("A").getbbox() is None:
            sys.exit(f"crest {crest}: logo {rows[crest]} is fully transparent")
        image = convert(logo, size, allowed, palette)
        image.save(CRESTS / f"{crest}.png", optimize=True)
        print(f"{crest}.png")


if __name__ == "__main__":
    main()
