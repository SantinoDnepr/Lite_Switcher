"""Builds the icon files of the program from icons/source.jpg.

The source is cropped to a square (without the thin light edge of the picture), the corners are rounded and
the result is written as PNG files of several sizes and as one multi-size lite-switcher.ico.

usage: python tools/icons/make-icons.py      (needs Pillow: pip install pillow)
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
ICONS = ROOT / "icons"
EDGE = 8  # pixels of the picture border to cut off
RADIUS = 0.225  # corner radius, a share of the side
MASTER = 1024
PNG_SIZES = [512, 256, 64, 32, 16]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


def rounded(image: Image.Image) -> Image.Image:
    size = image.width
    scale = 4  # the mask is drawn bigger and shrunk: smooth edge
    mask = Image.new("L", (size * scale, size * scale), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, size * scale - 1, size * scale - 1), radius=int(size * scale * RADIUS), fill=255)
    mask = mask.resize((size, size), Image.LANCZOS)
    out = image.convert("RGBA")
    out.putalpha(mask)
    return out


def main() -> None:
    source = Image.open(ICONS / "source.jpg").convert("RGB")
    side = min(source.size) - 2 * EDGE
    left = (source.width - side) // 2
    top = (source.height - side) // 2
    square = source.crop((left, top, left + side, top + side)).resize((MASTER, MASTER), Image.LANCZOS)
    master = rounded(square)
    for size in PNG_SIZES:
        master.resize((size, size), Image.LANCZOS).save(ICONS / f"icon-{size}.png", optimize=True)
    master.save(ICONS / "lite-switcher.ico", sizes=[(s, s) for s in ICO_SIZES])
    print("icons:", ", ".join(sorted(p.name for p in ICONS.iterdir() if p.name != "source.jpg")))


if __name__ == "__main__":
    main()
