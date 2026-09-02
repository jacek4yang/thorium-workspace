"""Generate the Thorium Workspace application icon (src-tauri/icons/icon.ico).

Development-time tool only; not part of the build. Produces a simple
branded mark: dark slate rounded square with a stylized "Th" monogram.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUTPUT = Path(__file__).resolve().parents[1] / "src-tauri" / "icons" / "icon.ico"

BACKGROUND = (32, 36, 46, 255)
ACCENT = (94, 158, 255, 255)
GLYPH = (235, 240, 248, 255)

FONT_CANDIDATES = [
    r"C:\Windows\Fonts\segoeuib.ttf",
    r"C:\Windows\Fonts\arialbd.ttf",
    r"C:\Windows\Fonts\arial.ttf",
]


def load_font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for candidate in FONT_CANDIDATES:
        if Path(candidate).exists():
            return ImageFont.truetype(candidate, size)
    return ImageFont.load_default()


def draw_icon(size: int) -> Image.Image:
    scale = max(1, size // 32)
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    radius = max(2, size // 5)
    draw.rounded_rectangle(
        [0, 0, size - 1, size - 1],
        radius=radius,
        fill=BACKGROUND,
    )

    # Accent underline bar.
    bar_height = max(2 * scale, 2)
    bar_margin = size // 4
    draw.rounded_rectangle(
        [bar_margin, size - bar_margin - bar_height, size - bar_margin, size - bar_margin],
        radius=bar_height // 2,
        fill=ACCENT,
    )

    font = load_font(int(size * 0.52))
    text = "Th"
    bbox = draw.textbbox((0, 0), text, font=font)
    text_width = bbox[2] - bbox[0]
    text_height = bbox[3] - bbox[1]
    x = (size - text_width) / 2 - bbox[0]
    y = (size - text_height) / 2 - bbox[1] - size // 14
    draw.text((x, y), text, font=font, fill=GLYPH)
    return img


def main() -> None:
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    base = draw_icon(256)
    base.save(OUTPUT, sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    print(f"wrote {OUTPUT}")


if __name__ == "__main__":
    main()
