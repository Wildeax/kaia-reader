#!/usr/bin/env python3
"""Generate the bundled Kaia sticker PNGs with the Python standard library.

Usage: python3 tools/kaia/make_stickers.py

Writes star.png, heart.png, check.png and question.png to resources/kaia/stickers/.
Each is 128 x 128, RGBA, a black or dark gray shape on a transparent background.
The shapes are drawn procedurally, so no third-party art enters the product.
Edges use 4 x 4 supersampling for anti-aliasing. Running the script again
overwrites the files with identical bytes.
"""

import math
import os
import struct
import zlib

SIZE = 128
SUPERSAMPLE = 4
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(ROOT, "resources", "kaia", "stickers")


def png_bytes(width, height, rows):
    """Encodes RGBA rows (list of bytearray, 4 bytes per pixel) as a PNG file."""
    raw = bytearray()
    for row in rows:
        raw.append(0)  # filter type None
        raw.extend(row)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def render(inside, gray):
    """Rasterizes a shape. `inside(x, y)` takes float pixel coordinates and returns bool."""
    rows = []
    step = 1.0 / SUPERSAMPLE
    total = SUPERSAMPLE * SUPERSAMPLE
    for py in range(SIZE):
        row = bytearray()
        for px in range(SIZE):
            hits = 0
            for sy in range(SUPERSAMPLE):
                for sx in range(SUPERSAMPLE):
                    x = px + (sx + 0.5) * step
                    y = py + (sy + 0.5) * step
                    if inside(x, y):
                        hits += 1
            alpha = (hits * 255 + total // 2) // total
            row.extend((gray, gray, gray, alpha))
        rows.append(row)
    return rows


def point_in_polygon(x, y, points):
    inside = False
    j = len(points) - 1
    for i in range(len(points)):
        xi, yi = points[i]
        xj, yj = points[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            inside = not inside
        j = i
    return inside


def distance_to_segment(x, y, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    length_sq = dx * dx + dy * dy
    t = 0.0 if length_sq == 0 else max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / length_sq))
    cx, cy = ax + t * dx, ay + t * dy
    return math.hypot(x - cx, y - cy)


def star_shape():
    points = []
    cx, cy = 64.0, 66.0
    for i in range(10):
        radius = 56.0 if i % 2 == 0 else 23.0
        angle = -math.pi / 2 + i * math.pi / 5
        points.append((cx + radius * math.cos(angle), cy + radius * math.sin(angle)))
    return lambda x, y: point_in_polygon(x, y, points)


def heart_shape():
    def inside(x, y):
        # Implicit heart curve, scaled to about 50 px per unit, y pointing up.
        u = (x - 64.0) / 50.0
        v = (64.0 - y) / 50.0 - 0.1
        return (u * u + v * v - 1.0) ** 3 - u * u * v * v * v <= 0.0

    return inside


def check_shape():
    width = 14.0
    segments = [((30.0, 66.0), (54.0, 90.0)), ((54.0, 90.0), (98.0, 40.0))]

    def inside(x, y):
        return any(distance_to_segment(x, y, ax, ay, bx, by) <= width / 2 for (ax, ay), (bx, by) in segments)

    return inside


def question_shape():
    stroke = 12.0
    cx, cy, radius = 64.0, 48.0, 26.0
    segments = [
        ((88.0, 48.0), (66.0, 70.0)),  # hook down to the stem
        ((64.0, 72.0), (64.0, 86.0)),  # stem
    ]

    def inside(x, y):
        dist = math.hypot(x - cx, y - cy)
        if y <= cy and abs(dist - radius) <= stroke / 2:
            return True  # upper arc
        if math.hypot(x - 64.0, y - 104.0) <= 8.0:
            return True  # dot
        return any(distance_to_segment(x, y, ax, ay, bx, by) <= stroke / 2 for (ax, ay), (bx, by) in segments)

    return inside


STICKERS = [
    ("star.png", star_shape(), 0x00),
    ("heart.png", heart_shape(), 0x3A),
    ("check.png", check_shape(), 0x1A),
    ("question.png", question_shape(), 0x5A),
]


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    for name, inside, gray in STICKERS:
        data = png_bytes(SIZE, SIZE, render(inside, gray))
        path = os.path.join(OUT_DIR, name)
        with open(path, "wb") as f:
            f.write(data)
        print(path, len(data), "bytes")


if __name__ == "__main__":
    main()
