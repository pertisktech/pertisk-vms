#!/usr/bin/env python3
"""Regenerate Anaconda pixmap branding under packaging/anaconda-branding/pixmaps/."""
from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "pixmaps"

# Charcoal — not AlmaLinux purple. Keep accent cool blue (readable, not violet).
BG = (18, 20, 24)  # #121418
BG_BOTTOM = (24, 28, 34)
ACCENT = (56, 152, 220)  # #3898dc
WHITE = (255, 255, 255)


def solid(size: tuple[int, int], color: tuple[int, int, int]) -> Image.Image:
    return Image.new("RGB", size, color)


def vertical_gradient(
    size: tuple[int, int], top: tuple[int, int, int], bottom: tuple[int, int, int]
) -> Image.Image:
    w, h = size
    im = Image.new("RGB", size)
    px = im.load()
    for y in range(h):
        t = y / max(h - 1, 1)
        c = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        for x in range(w):
            px[x, y] = c
    return im


def find_font(size: int) -> ImageFont.ImageFont:
    for path in (
        "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/google-noto/NotoSans-Bold.ttf",
        "/usr/share/fonts/gnu-free/FreeSansBold.ttf",
    ):
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    # sidebar-bg: fully opaque charcoal (Alma bg is partly transparent so CSS
    # purple shows through — we must not leave alpha holes).
    sidebar = vertical_gradient((200, 1200), BG, BG_BOTTOM).convert("RGBA")
    ImageDraw.Draw(sidebar).rectangle([0, 0, 5, 1200], fill=(*ACCENT, 255))
    sidebar.save(OUT / "sidebar-bg.png", optimize=True)

    top = solid((1920, 132), BG).convert("RGBA")
    ImageDraw.Draw(top).rectangle([0, 126, 1920, 132], fill=(*ACCENT, 255))
    top.save(OUT / "topbar-bg.png", optimize=True)

    # sidebar-logo: fill the canvas like AlmaLinux's wordmark so 70% scale stays readable.
    # Transparent background; near-white text (Alma uses ~RGB 223,229,219).
    logo = Image.new("RGBA", (2000, 386), (0, 0, 0, 0))
    ld = ImageDraw.Draw(logo)
    title = find_font(160)
    text = "Pertisk VM"
    ink = (236, 240, 245, 255)
    bbox = ld.textbbox((0, 0), text, font=title)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    x = (2000 - tw) // 2
    y = (386 - th) // 2
    ld.text((x, y), text, font=title, fill=ink)
    logo.save(OUT / "sidebar-logo.png", optimize=True)

    for path in sorted(OUT.iterdir()):
        im = Image.open(path)
        print(f"{path.name}: {im.size} {im.mode}")


if __name__ == "__main__":
    main()
