"""生成多尺寸 logo.ico。

只放一帧 256x256 PNG 的 ico，会让 Windows 在各处以缩放方式取图：16x16 的
Explorer/任务栏看起来就是一片白（白色半区与浅色背景融在一起）。这里给每个
尺寸单独出一帧，并给圆盘补一圈极细的描边，让白色半区在白底上也有边界。

用法：python tools/make_icon.py [源图] [输出]
"""

import os
import struct
import sys
import zlib

from PIL import Image, ImageDraw

# 需要出帧的尺寸：小尺寸靠系统直接取用，不再缩放
SIZES = [16, 24, 32, 48, 64, 128, 256]
# 16~128 用 BMP(DIB) 帧，兼容性最好；256 用 PNG 帧控制体积
PNG_FROM = 256
RING_RATIO = 0.008  # 描边宽度 = 尺寸 * 该比例，最小 1px


def premultiplied_resize(image, size):
    """按预乘 alpha 缩放，避免半透明边缘出现黑边"""
    premul = image.convert("RGBa")
    out = premul.resize(size, Image.LANCZOS)
    return out.convert("RGBA")


def disc_geometry(master):
    """量出圆盘的中心与半径（取中心行列上的 alpha 范围）"""
    alpha = master.getchannel("A")
    w, h = master.size
    row = [alpha.getpixel((x, h // 2)) for x in range(w)]
    col = [alpha.getpixel((w // 2, y)) for y in range(h)]
    xs = [i for i, a in enumerate(row) if a >= 128]
    ys = [i for i, a in enumerate(col) if a >= 128]
    cx = (xs[0] + xs[-1]) / 2.0
    cy = (ys[0] + ys[-1]) / 2.0
    r = ((xs[-1] - xs[0]) + (ys[-1] - ys[0])) / 4.0
    return (cx, cy, r)


def render(master, geometry, size, ring_color=(18, 18, 18, 255)):
    """以 4 倍或 8 倍超采样绘制指定尺寸的一帧"""
    factor = 8 if size <= 64 else 4
    big = size * factor
    art = premultiplied_resize(master, (big, big))

    cx, cy, r = geometry
    scale = big / master.size[0]
    ring = max(1, round(size * RING_RATIO)) * factor
    draw = ImageDraw.Draw(art)
    draw.ellipse(
        [
            cx * scale - r * scale,
            cy * scale - r * scale,
            cx * scale + r * scale,
            cy * scale + r * scale,
        ],
        outline=ring_color,
        width=ring,
    )
    return premultiplied_resize(art, (size, size))


def dib_frame(image):
    """32bpp BMP 帧：BITMAPINFOHEADER + 自下而上的 BGRA + AND 掩码"""
    w, h = image.size
    px = image.load()
    header = struct.pack(
        "<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, w * h * 4, 0, 0, 0, 0
    )
    pixels = bytearray()
    masks = bytearray()
    stride = ((w + 31) // 32) * 4
    for y in range(h - 1, -1, -1):
        mask = bytearray(stride)
        for x in range(w):
            r, g, b, a = px[x, y]
            pixels += bytes((b, g, r, a))
            if a < 128:
                mask[x // 8] |= 0x80 >> (x % 8)
        masks += mask
    # 像素区与 AND 掩码区必须各自连续：先整块像素，再整块掩码
    return header + bytes(pixels) + bytes(masks)


def png_frame(image):
    buf = []
    image.save(
        _Collect(buf),
        format="PNG",
        optimize=True,
    )
    return b"".join(buf)


class _Collect:
    """PIL 需要一个写文件对象；这里收进内存"""

    def __init__(self, sink):
        self.sink = sink

    def write(self, data):
        self.sink.append(data)

    def flush(self):
        pass


def build_ico(master, geometry, path):
    frames = []
    for size in SIZES:
        art = render(master, geometry, size)
        data = png_frame(art) if size >= PNG_FROM else dib_frame(art)
        frames.append((size, data))

    out = bytearray(struct.pack("<HHH", 0, 1, len(frames)))
    offset = 6 + 16 * len(frames)
    for size, data in frames:
        out += struct.pack(
            "<BBBBHHII",
            0 if size >= 256 else size,
            0 if size >= 256 else size,
            0,
            0,
            1,
            32,
            len(data),
            offset,
        )
        offset += len(data)
    for _, data in frames:
        out += data
    open(path, "wb").write(out)
    return len(out)


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    src = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, "logo.png")
    dst = sys.argv[2] if len(sys.argv) > 2 else os.path.join(root, "logo.ico")

    master = Image.open(src).convert("RGBA")
    geometry = disc_geometry(master)
    print(f"源图 {master.size} 圆盘 中心=({geometry[0]:.1f},{geometry[1]:.1f}) 半径={geometry[2]:.1f}")
    total = build_ico(master, geometry, dst)

    check = Image.open(dst)
    print(f"已写入 {dst}  {total} 字节  尺寸: {sorted(check.info.get('sizes', []))}")


if __name__ == "__main__":
    main()
