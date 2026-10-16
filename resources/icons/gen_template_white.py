#!/usr/bin/env python3
"""由 template 图生成 README 用的「深色菜单栏」版本（纯白同形状）

背景：BeeKMenuBarTemplate@2x.png、beek-circle-i@4x.png 都是 template 图 ——
纯黑 + alpha，颜色由 AppKit 按菜单栏/菜单明暗自动染。README 里直接贴黑图，
在 GitHub 深色主题下就是一片黑（看不见）。这个脚本把同一张图的形状留着、
颜色换成白色，配合 README 里的 <picture>（按 prefers-color-scheme 切换）。

本机没有 PIL（见 gen_circle_i.py 的说明），所以这里只用标准库：
手工解 PNG（RGBA8、非隔行）→ 把 RGB 改白 → 手工写回 PNG。
"""
import struct
import zlib

ROOT = "resources/icons/"
PAIRS = [
    (ROOT + "BeeKMenuBarTemplate@2x.png", ROOT + "BeeKMenuBarTemplate-white@2x.png"),
    (ROOT + "beek-circle-i@4x.png", ROOT + "beek-circle-i-white@4x.png"),
]


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "不是 PNG"
    i, idat, wh = 8, b"", None
    while i < len(data):
        ln = struct.unpack(">I", data[i : i + 4])[0]
        typ = data[i + 4 : i + 8]
        body = data[i + 8 : i + 8 + ln]
        if typ == b"IHDR":
            w, h, bd, ct, comp, filt, inter = struct.unpack(">IIBBBBB", body)
            assert (bd, ct, inter) == (8, 6, 0), f"只支持 RGBA8 非隔行，实际 {bd}/{ct}/{inter}"
            wh = (w, h)
        elif typ == b"IDAT":
            idat += body
        i += 12 + ln
    w, h = wh
    raw = zlib.decompress(idat)
    bpp, stride = 4, w * 4
    out, prev = [], bytearray(stride)
    pos = 0
    for _ in range(h):
        f = raw[pos]
        line = bytearray(raw[pos + 1 : pos + 1 + stride])
        pos += 1 + stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1:
                line[x] = (line[x] + a) & 0xFF
            elif f == 2:
                line[x] = (line[x] + b) & 0xFF
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 0xFF
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 0xFF
        out.append(bytes(line))
        prev = line
    return w, h, out


def write_png(path, w, h, rows):
    def chunk(typ, body):
        return (
            struct.pack(">I", len(body))
            + typ
            + body
            + struct.pack(">I", zlib.crc32(typ + body) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + r for r in rows)
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    open(path, "wb").write(png)


for src, out in PAIRS:
    w, h, rows = read_png(src)
    white = []
    for r in rows:
        line = bytearray(r)
        for x in range(0, len(line), 4):
            line[x] = line[x + 1] = line[x + 2] = 255  # 形状保留，颜色换白
        white.append(bytes(line))
    write_png(out, w, h, white)
    print(f"已生成 {out}（{w}x{h}）")
