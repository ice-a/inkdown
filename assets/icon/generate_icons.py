#!/usr/bin/env python3
"""Render Inkdown's app icon from one vector-like source into the formats
Windows, macOS and Linux need.

Usage:
    python generate_icons.py <output-dir>

Requires Pillow. The generated PNG/ICO/ICNS files are committed, so this
script only needs to run when the artwork changes.
"""

import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


S = 1024
SS = 2  # supersampling factor for smooth edges


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def vertical_gradient(size, top, bottom):
    image = Image.new("RGBA", (size, size))
    draw = ImageDraw.Draw(image)
    for y in range(size):
        draw.line([(0, y), (size, y)], fill=lerp(top, bottom, y / (size - 1)))
    return image


def rounded_mask(size, radius):
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, size - 1, size - 1), radius=radius, fill=255
    )
    return mask


def draw_icon(size):
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    # Dark ink slab, the constant silhouette at every size.
    radius = int(size * 0.225)
    background = vertical_gradient(size, (0x34, 0x2D, 0x24), (0x14, 0x12, 0x0F))
    canvas.paste(background, (0, 0), rounded_mask(size, radius))

    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle(
        (2, 2, size - 3, size - 3),
        radius=radius,
        outline=(0x5A, 0x4B, 0x39, 0x90),
        width=max(2, int(size * 0.006)),
    )

    # Warm paper sheet with a soft shadow.
    px = int(size * 0.235)
    py = int(size * 0.155)
    pw = int(size * 0.515)
    ph = int(size * 0.665)
    page_radius = int(size * 0.062)

    shadow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        (px + int(size * 0.012), py + int(size * 0.022), px + pw, py + ph),
        radius=page_radius,
        fill=(0, 0, 0, 150),
    )
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(size * 0.022)))

    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle(
        (px, py, px + pw, py + ph),
        radius=page_radius,
        fill=(0xFB, 0xF6, 0xEC, 255),
        outline=(0xE2, 0xD3, 0xBE, 255),
        width=max(2, int(size * 0.004)),
    )

    # A slim torn-paper / code rail along the left edge.
    rail_x = px + int(size * 0.072)
    draw.rounded_rectangle(
        (rail_x - int(size * 0.016), py + int(size * 0.09),
         rail_x + int(size * 0.016), py + ph - int(size * 0.09)),
        radius=int(size * 0.016),
        fill=(0xB4, 0x55, 0x2D, 235),
    )

    # Heading and body lines.
    left = rail_x + int(size * 0.055)
    right = px + pw - int(size * 0.085)
    heading_top = py + int(size * 0.135)
    draw.rounded_rectangle(
        (left, heading_top, left + int(size * 0.245), heading_top + int(size * 0.052)),
        radius=int(size * 0.026),
        fill=(0x2B, 0x26, 0x1F, 255),
    )

    line_tops = [0.255, 0.325, 0.395]
    widths = [0.375, 0.315, 0.235]
    for top, width in zip(line_tops, widths):
        y = py + int(size * top)
        draw.rounded_rectangle(
            (left, y, left + int(size * width), y + int(size * 0.024)),
            radius=int(size * 0.012),
            fill=(0xC9, 0xB8, 0xA2, 255),
        )

    # Code block.
    code_top = py + int(size * 0.49)
    code_bottom = code_top + int(size * 0.135)
    draw.rounded_rectangle(
        (left, code_top, right, code_bottom),
        radius=int(size * 0.028),
        fill=(0xEF, 0xE3, 0xD1, 255),
    )
    for index, width in enumerate((0.20, 0.30, 0.14)):
        y = code_top + int(size * (0.032 + index * 0.033))
        draw.rounded_rectangle(
            (left + int(size * 0.03), y,
             left + int(size * 0.03) + int(size * width), y + int(size * 0.013)),
            radius=int(size * 0.007),
            fill=(0xA9, 0x92, 0x78, 255),
        )

    # Seal-red stamp with a downward arrow: ink + markdown.
    seal_size = int(size * 0.285)
    seal = Image.new("RGBA", (seal_size * 2, seal_size * 2), (0, 0, 0, 0))
    seal_draw = ImageDraw.Draw(seal)
    seal_draw.rounded_rectangle(
        (seal_size // 2, seal_size // 2, seal_size + seal_size // 2, seal_size + seal_size // 2),
        radius=int(seal_size * 0.24),
        fill=(0xB4, 0x55, 0x2D, 255),
        outline=(0x8F, 0x3A, 0x1C, 255),
        width=max(3, int(size * 0.008)),
    )
    # White down arrow.
    cx = seal_size
    cy = seal_size + int(seal_size * 0.02)
    arm = int(seal_size * 0.19)
    stroke = max(6, int(seal_size * 0.075))
    seal_draw.line(
        [(cx, cy - int(seal_size * 0.19)), (cx, cy + int(seal_size * 0.12))],
        fill=(0xFF, 0xF9, 0xF1, 255), width=stroke,
    )
    seal_draw.line(
        [(cx - arm, cy + int(seal_size * 0.005)), (cx, cy + int(seal_size * 0.155)),
         (cx + arm, cy + int(seal_size * 0.005))],
        fill=(0xFF, 0xF9, 0xF1, 255), width=stroke, joint="curve",
    )
    seal = seal.rotate(-10, resample=Image.Resampling.BICUBIC, expand=True)

    seal_layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    seal_layer.alpha_composite(seal, (px + pw - int(size * 0.315), py + ph - int(size * 0.325)))
    canvas.alpha_composite(seal_layer)

    # A quiet highlight on the slab's top edge.
    highlight = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ImageDraw.Draw(highlight).rounded_rectangle(
        (3, 3, size - 4, size - 4),
        radius=radius,
        outline=(0xFF, 0xF4, 0xE2, 0x28),
        width=max(2, int(size * 0.004)),
    )
    canvas.alpha_composite(highlight)

    return canvas


def save_pngs(icon, out_dir):
    sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
    for size in sizes:
        resized = icon.resize((size, size), Image.Resampling.LANCZOS)
        resized.save(out_dir / f"inkdown-{size}.png", "PNG", optimize=True)


def save_ico(icon, out_dir):
    icon.save(
        out_dir / "inkdown.ico",
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )


def save_icns(icon, out_dir):
    # ICNS uses big-endian chunks; modern entries carry PNG payloads.
    entries = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic13", 256),
        (b"ic08", 256),
        (b"ic14", 512),
        (b"ic09", 512),
        (b"ic10", 1024),
    ]
    chunks = []
    for code, size in entries:
        buffer = __import__("io").BytesIO()
        icon.resize((size, size), Image.Resampling.LANCZOS).save(buffer, "PNG", optimize=True)
        data = buffer.getvalue()
        chunks.append(code + struct.pack(">I", len(data) + 8) + data)
    body = b"".join(chunks)
    (out_dir / "inkdown.icns").write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)


def main():
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(".")
    out_dir.mkdir(parents=True, exist_ok=True)
    icon = draw_icon(S * SS).resize((S, S), Image.Resampling.LANCZOS)
    save_pngs(icon, out_dir)
    save_ico(icon, out_dir)
    save_icns(icon, out_dir)
    print(f"icons written to {out_dir}")


if __name__ == "__main__":
    main()
