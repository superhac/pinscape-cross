#!/usr/bin/env python3
"""Generate placeholder app icons (a silver pinball on a blue tile).

Replace with real artwork later via:  cargo tauri icon path/to/icon-1024.png
"""
import math, struct, sys, zlib
from pathlib import Path

def png(size: int) -> bytes:
    rows = []
    c = (size - 1) / 2
    r_tile = size * 0.22
    ball_r = size * 0.32
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            # rounded-square tile
            dx = max(abs(x - c) - (c - r_tile), 0)
            dy = max(abs(y - c) - (c - r_tile), 0)
            if math.hypot(dx, dy) > r_tile:
                row += b"\0\0\0\0"; continue
            d = math.hypot(x - c, y - c)
            if d <= ball_r:
                # shaded ball, highlight up-left
                hl = math.hypot(x - (c - ball_r * 0.35), y - (c - ball_r * 0.35)) / (ball_r * 1.4)
                v = int(250 - 150 * min(hl, 1))
                row += bytes([v, v, min(v + 8, 255), 255])
            else:
                row += bytes([37, 99, 235, 255])
        rows.append(bytes(row))
    raw = zlib.compress(b"".join(rows), 9)
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", raw) + chunk(b"IEND", b""))

def ico(images: list[tuple[int, bytes]]) -> bytes:
    out = struct.pack("<HHH", 0, 1, len(images))
    off = 6 + 16 * len(images)
    body = b""
    for size, data in images:
        out += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), off + len(body))
        body += data
    return out + body

d = Path(sys.argv[1] if len(sys.argv) > 1 else "src-tauri/icons")
d.mkdir(parents=True, exist_ok=True)
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
    (d / name).write_bytes(png(size))
(d / "icon.ico").write_bytes(ico([(s, png(s)) for s in (16, 32, 48, 256)]))
print("icons written to", d)
