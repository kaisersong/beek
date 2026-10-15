#!/usr/bin/env python3
"""生成「线描式发音图标」素材：beek-speak-outline@4x.png

为什么自己画：发音按钮原来用 emoji 🔊，彩色且有渐变，和灰色的音标文字不搭。
需求是「线描式」，所以这里只描边不填充，颜色跟音标同色（#666666）。

为什么不用 SVG / 第三方库：gpui 0.2.2 的 img() 走的是位图（Image::from_bytes），
本机也没有 rsvg/PIL，所以直接用标准库把折线栅格化成位图。

画法：8 倍超采样 + 到线段的胶囊距离做覆盖率，再盒式下采样，得到抗锯齿的细线。
56×56 对应绘制时的 14pt（4x），够清晰。
"""
import math
import struct
import zlib

SIZE = 56          # 输出边长（px）
SS = 8             # 超采样倍数
STROKE = 0.075     # 线宽（相对画布）
COLOR = (0x66, 0x66, 0x66)

# 喇叭外轮廓（闭合折线，坐标按 0..1 归一化）
SPEAKER = [
    (0.13, 0.38), (0.29, 0.38), (0.51, 0.14),
    (0.51, 0.86), (0.29, 0.62), (0.13, 0.62), (0.13, 0.38),
]


def arc(cx, cy, r, deg0, deg1, steps=48):
    """一段圆弧采样成折线，角度按 y 向下的屏幕坐标算（数学角取负）。"""
    pts = []
    for i in range(steps + 1):
        a = math.radians(deg0 + (deg1 - deg0) * i / steps)
        pts.append((cx + r * math.cos(a), cy - r * math.sin(a)))
    return pts


# 两道声波弧：圆心在喇叭口右侧
WAVES = [arc(0.47, 0.50, 0.17, -52, 52), arc(0.47, 0.50, 0.30, -52, 52)]


def dist_to_segment(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    seg = vx * vx + vy * vy
    t = 0.0 if seg == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / seg))
    dx, dy = px - (ax + t * vx), py - (ay + t * vy)
    return math.hypot(dx, dy)


def polylines():
    out = []
    for path in [SPEAKER] + WAVES:
        for a, b in zip(path, path[1:]):
            out.append((a[0] * SIZE, a[1] * SIZE, b[0] * SIZE, b[1] * SIZE))
    return out


def render():
    segs = polylines()
    half = STROKE * SIZE / 2.0
    n = SIZE * SS
    hi = bytearray(n * n)
    for y in range(n):
        py = (y + 0.5) / SS
        row = y * n
        for x in range(n):
            px = (x + 0.5) / SS
            for ax, ay, bx, by in segs:
                if dist_to_segment(px, py, ax, ay, bx, by) <= half:
                    hi[row + x] = 1
                    break
    # 盒式下采样：覆盖率 → alpha
    px_out = bytearray()
    for y in range(SIZE):
        for x in range(SIZE):
            hits = 0
            for dy in range(SS):
                row = (y * SS + dy) * n + x * SS
                hits += sum(hi[row:row + SS])
            a = round(255 * hits / (SS * SS))
            px_out += bytes((COLOR[0], COLOR[1], COLOR[2], a))
    return bytes(px_out)


def write_png(path, rgba):
    raw = b"".join(
        b"\x00" + rgba[y * SIZE * 4:(y + 1) * SIZE * 4] for y in range(SIZE)
    )

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
    out = "beek-speak-outline@4x.png"
    write_png(out, render())
    print(f"已生成 {out}（{SIZE}x{SIZE}，线宽 {STROKE * SIZE:.1f}px）")
