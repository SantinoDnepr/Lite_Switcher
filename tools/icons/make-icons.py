"""Builds the icon files of the program from icons/source.png.

The source is a picture without an alpha channel: the "transparent" background is a grey and white checkerboard
that is part of the picture. The logo is cut out by colour (the logo is saturated, the checkerboard is grey). The script

* keeps only the solid body of the logo (yellow outline and dark blue letters),
* gives it a clean antialiased edge (the colours are premultiplied, so no dark or light fringe appears when the
  picture is made smaller),
* writes the PNG files of several sizes and one multi-size lite-switcher.ico. The big sizes (128 px and more) have
  the picture with a soft shadow. The small sizes (up to 64 px) are redrawn flat, without the shadow, with a thicker
  outline (the outline of the picture is too thin to stay a continuous line when it is shrunk) and the logo fills
  the square, so the letters stay readable.

usage: python tools/icons/make-icons.py      (needs Pillow: pip install pillow)
"""

import io
import struct
from collections import deque
from pathlib import Path

from PIL import Image, ImageChops, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
ICONS = ROOT / "icons"
MASTER = 1024
PNG_SIZES = [512, 256, 64, 32, 16]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
SMALL = 64  # up to this size the icon is redrawn flat with a thicker outline and has a thin margin
MARGIN_BIG, MARGIN_SMALL = 0.045, 0.02
# how much the outline of the flat small icons grows (a share of the width of the logo): the outside edge goes out
# and the letters go in by this value (the outline of the picture is about 4.5% of the width thick)
OUTLINE_GROWTH = {16: 0.042, 24: 0.024, 32: 0.019, 48: 0.004, 64: 0.0105}
NAVY, GOLD = (4, 45, 136), (252, 209, 0)
SHADOW = (0.012, 0.018, 0.013, 0.40)  # offset x, offset y, blur radius (shares of the side), opacity


def is_logo_colour(r: int, g: int, b: int) -> bool:
    """The logo is yellow and blue (saturated); the background checkerboard is white and grey."""
    return max(r, g, b) - min(r, g, b) > 70


def large_parts(mask: Image.Image, min_share: float = 0.02) -> Image.Image:
    """The connected parts of a mask that are not specks (at least `min_share` of the mask area)."""
    step = 4
    small = mask.resize((mask.width // step, mask.height // step), Image.NEAREST)
    w, h = small.size
    px = small.load()
    seen = [[False] * w for _ in range(h)]
    parts, total = [], 0
    for y in range(h):
        for x in range(w):
            if px[x, y] and not seen[y][x]:
                part, queue = [], deque([(x, y)])
                seen[y][x] = True
                while queue:
                    cx, cy = queue.popleft()
                    part.append((cx, cy))
                    for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                        if 0 <= nx < w and 0 <= ny < h and px[nx, ny] and not seen[ny][nx]:
                            seen[ny][nx] = True
                            queue.append((nx, ny))
                parts.append(part)
                total += len(part)
    keep = Image.new("L", small.size, 0)
    kp = keep.load()
    for part in parts:
        if len(part) >= total * min_share:
            for x, y in part:
                kp[x, y] = 255
    return keep.resize(mask.size, Image.NEAREST)


def clean_logo(source: Image.Image) -> Image.Image:
    """The solid body of the logo as an RGBA picture with premultiplied-safe edges."""
    rgba = source.convert("RGBA")
    width, height = rgba.size
    px = rgba.load()
    mask = Image.new("L", rgba.size, 0)
    mp = mask.load()
    for y in range(height):
        for x in range(width):
            r, g, b, a = px[x, y]
            if a > 160 and is_logo_colour(r, g, b):
                mp[x, y] = 255
    mask = mask.filter(ImageFilter.MinFilter(7)).filter(ImageFilter.MaxFilter(7))  # drops thin specks
    mask = large_parts(mask)
    # fill the holes of the body (small gaps between the colours) and move the edge a little inside, where
    # the colours are clean, then soften it: this is the antialiasing of the new edge
    mask = mask.filter(ImageFilter.MaxFilter(5)).filter(ImageFilter.MinFilter(9)).filter(ImageFilter.GaussianBlur(1.6))
    # the colours are the picture itself: the edge was moved inside the body, so every pixel with a partial
    # alpha has a clean body colour (no spreading of colours, it mixes the channels and makes pale fringes)
    result = rgba.copy()
    result.putalpha(mask)
    return result


def grow(mask: Image.Image, pixels: float) -> Image.Image:
    """The mask grown (positive) or shrunk (negative) by `pixels`, with round corners (blur and threshold)."""
    if pixels == 0:
        return mask
    d = abs(pixels)
    blurred = mask.filter(ImageFilter.GaussianBlur(d))
    # at the distance d from a straight edge the blurred mask has 16% of its height
    level = 40 if pixels > 0 else 215
    return blurred.point(lambda v: 255 if v > level else 0).filter(ImageFilter.GaussianBlur(1.0))


def flat_logo(logo: Image.Image, growth: int) -> Image.Image:
    """The logo redrawn with two flat colours and a thicker yellow outline (for the small sizes)."""
    alpha = logo.getchannel("A")
    px = logo.load()
    letters = Image.new("L", logo.size, 0)
    lp = letters.load()
    for y in range(logo.height):
        for x in range(logo.width):
            r, g, b, a = px[x, y]
            if a > 128 and r < 60 and g < 90 and b > 90:
                lp[x, y] = 255
    width = alpha.point(lambda v: 255 if v > 128 else 0).getbbox()
    pixels = round(growth * (width[2] - width[0]))
    body = grow(alpha.point(lambda v: 255 if v > 128 else 0), pixels)
    letters = grow(letters, -pixels)
    out = Image.new("RGBA", logo.size, GOLD + (0,))
    out.paste(Image.new("RGBA", logo.size, NAVY + (255,)), (0, 0), letters)
    out.putalpha(body)
    # inside the body the letters are navy, the rest is gold
    colour = Image.composite(Image.new("RGB", logo.size, NAVY), Image.new("RGB", logo.size, GOLD), letters)
    colour.putalpha(body)
    return colour


def premultiplied_resize(image: Image.Image, size: int) -> Image.Image:
    return image.convert("RGBa").resize((size, size), Image.LANCZOS).convert("RGBA")


def square(logo: Image.Image, margin: float, shadow: bool) -> Image.Image:
    """The logo centred on a transparent square canvas of MASTER pixels, optionally with a soft shadow."""
    box = logo.getchannel("A").point(lambda v: 255 if v > 100 else 0).getbbox()
    logo = logo.crop(box)
    side = max(logo.size)
    inner = int(MASTER * (1 - 2 * margin))
    if shadow:
        inner = int(inner * 0.95)  # room for the shadow
    scale = inner / side
    logo = logo.convert("RGBa").resize((round(logo.width * scale), round(logo.height * scale)), Image.LANCZOS).convert("RGBA")
    canvas = Image.new("RGBA", (MASTER, MASTER), (0, 0, 0, 0))
    left, top = (MASTER - logo.width) // 2, (MASTER - logo.height) // 2
    if shadow:
        dx, dy, blur, opacity = SHADOW
        layer = Image.new("RGBA", (MASTER, MASTER), (0, 0, 0, 0))
        layer.paste(logo, (left, top))
        alpha = layer.getchannel("A").point(lambda v: int(v * opacity))
        alpha = ImageChops.offset(alpha, int(MASTER * dx), int(MASTER * dy)).filter(ImageFilter.GaussianBlur(MASTER * blur))
        shade = Image.new("RGBA", (MASTER, MASTER), (0, 0, 0, 0))
        shade.putalpha(alpha)
        canvas = Image.alpha_composite(shade, layer)
    else:
        canvas.paste(logo, (left, top))
    return canvas


def png_bytes(image: Image.Image) -> bytes:
    out = io.BytesIO()
    image.save(out, "PNG", optimize=True)
    return out.getvalue()


def write_ico(path: Path, images: dict[int, Image.Image]) -> None:
    """An .ico with one PNG-compressed picture for every size (every size can have its own picture)."""
    blobs = [(size, png_bytes(images[size])) for size in sorted(images)]
    header = struct.pack("<HHH", 0, 1, len(blobs))
    offset = 6 + 16 * len(blobs)
    entries, data = b"", b""
    for size, blob in blobs:
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    path.write_bytes(header + entries + data)


def main() -> None:
    logo = clean_logo(Image.open(ICONS / "source.png"))
    big = square(logo, MARGIN_BIG, shadow=True)
    small = {size: square(flat_logo(logo, growth), MARGIN_SMALL, shadow=False) for size, growth in OUTLINE_GROWTH.items()}
    pick = lambda size: small[size] if size <= SMALL else big
    for size in PNG_SIZES:
        premultiplied_resize(pick(size), size).save(ICONS / f"icon-{size}.png", optimize=True)
    write_ico(ICONS / "lite-switcher.ico", {size: premultiplied_resize(pick(size), size) for size in ICO_SIZES})
    print("icons:", ", ".join(sorted(p.name for p in ICONS.iterdir() if p.name != "source.png")))


if __name__ == "__main__":
    main()
