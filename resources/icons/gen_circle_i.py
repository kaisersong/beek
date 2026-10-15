#!/usr/bin/env python3
"""生成菜单项用的「圆圈里加 i」图标：beek-circle-i@4x.png

用途：菜单栏「如何获取 API Key…」这一项左边的小图标。
要求是线描式（跟发音图标同一套语言），并且做成 **template 图**：
纯黑 + alpha，交给 AppKit 在深浅色菜单里自动反色。

为什么自己画：本机没有 rsvg/PIL，gpui/AppKit 也不吃 SVG 字符串；
标准库 zlib 直接把折线栅格化成位图最省事（8 倍超采样抗锯齿）。
64×64 对应绘制时的 16pt（4x），和菜单项文字同高。
"""
import struct
import zlib

SIZE = 64
SS = 8
STROKE = 0.085          # 线宽（相对画布）
COLOR = (0, 0, 0)       # template：只要 alpha 形状，颜色由系统染

CIRCLE = (0.5, 0.5, 0.365)   # 圆心 + 半径
DOT = (0.5, 0.295, 0.052)    # i 的点（实心圆）
STEM = (0.5, 0.435, 0.5, 0.72)  # i 的竖线


def arc(cx, cy, r, steps=240):
    import math
    pts = []
    for i in range(steps + 1):
        a = 2 * math.pi * i / steps
        pts.append((cx + r * math.cos(a), cy - r * math.sin(a)))
    return pts


def dist_to_segment(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    seg = vx * vx + vy * vy
    t = 0.0 if seg == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / seg))
    dx, dy = px - (ax + t * vx), py - (ay + t * vy)
    return (dx * dx + dy * dy) ** 0.5


def render():
    cx, cy, r = CIRCLE
    ring = arc(cx, cy, r)
    segs = [(a[0] * SIZE, a[1] * SIZE, b[0] * SIZE, b[1] * SIZE) for a, b in zip(ring, ring[1:])]
    segs.append(tuple(v * SIZE for v in STEM))
    half = STROKE * SIZE / 2.0
    dot_cx, dot_cy, dot_r = DOT[0] * SIZE, DOT[1] * SIZE, max(DOT[2] * SIZE, half)

    n = SIZE * SS
    hi = bytearray(n * n)
    for y in range(n):
        py = (y + 0.5) / SS
        row = y * n
        for x in range(n):
            px = (x + 0.5) / SS
            if dist_to_segment(px, py, dot_cx, dot_cy, dot_cx, dot_cy) <= dot_r:
                hi[row + x] = 1
                continue
            for ax, ay, bx, by in segs:
                if dist_to_segment(px, py, ax, ay, bx, by) <= half:
                    hi[row + x] = 1
                    break

    out = bytearray()
    for y in range(SIZE):
        for x in range(SIZE):
            hits = 0
            for dy in range(SS):
                row = (y * SS + dy) * n + x * SS
                hits += sum(hi[row:row + SS])
            a = round(255 * hits / (SS * SS))
            out += bytes((COLOR[0], COLOR[1], COLOR[2], a))
    return bytes(out)


def write_png(path, rgba):
    raw = b"".join(b"\x00" + rgba[y * SIZE * 4:(y + 1) * SIZE * 4] for y in range(SIZE))

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    write_png("beek-circle-i@4x.png", render())
    print(f"已生成 beek-circle-i@4x.png（{SIZE}x{SIZE}，线宽 {STROKE * SIZE:.1f}px，template）")
