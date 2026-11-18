"""用 shell 的 IShellItemImageFactory 取文件图标——这就是资源管理器里画出来的那张图。

比 DrawIconEx 更接近真实观感：DrawIconEx 处理带 alpha 的 HICON 时并不可靠，
之前用它判断「exe 图标空白」会误判。
"""

import ctypes
import os
import sys
from ctypes import wintypes

ole32 = ctypes.WinDLL("ole32")
gdi32 = ctypes.WinDLL("gdi32")
user32 = ctypes.WinDLL("user32")
shell32 = ctypes.WinDLL("shell32")

SIIGBF_ICONONLY = 0x00000004
SIIGBF_BIGGERSIZEOK = 0x00000001
DIB_RGB_COLORS = 0


class GUID(ctypes.Structure):
    _fields_ = [
        ("Data1", ctypes.c_ulong),
        ("Data2", ctypes.c_ushort),
        ("Data3", ctypes.c_ushort),
        ("Data4", ctypes.c_ubyte * 8),
    ]

    def __init__(self, text):
        super().__init__()
        ole32.CLSIDFromString(ctypes.c_wchar_p(text), ctypes.byref(self))


IID_IShellItemImageFactory = "{bcc18b79-ba16-442f-80c4-8a59c30c463b}"


class SIZE(ctypes.Structure):
    _fields_ = [("cx", ctypes.c_long), ("cy", ctypes.c_long)]


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", wintypes.LONG),
        ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]


def get_image(path, size):
    p = ctypes.c_void_p()
    iid = GUID(IID_IShellItemImageFactory)
    hr = shell32.SHCreateItemFromParsingName(
        ctypes.c_wchar_p(os.path.abspath(path)), None, ctypes.byref(iid), ctypes.byref(p)
    )
    if hr != 0 or not p:
        raise OSError(f"SHCreateItemFromParsingName 失败 hr=0x{hr & 0xFFFFFFFF:08x}")
    vtbl = ctypes.cast(p, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p))).contents
    get_image_fn = ctypes.WINFUNCTYPE(
        ctypes.c_long, ctypes.c_void_p, SIZE, ctypes.c_int, ctypes.POINTER(wintypes.HBITMAP)
    )(vtbl[3])
    hbmp = wintypes.HBITMAP()
    hr = get_image_fn(p, SIZE(size, size), SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK, ctypes.byref(hbmp))
    release = ctypes.WINFUNCTYPE(ctypes.c_ulong, ctypes.c_void_p)(vtbl[2])
    release(p)
    if hr != 0 or not hbmp:
        raise OSError(f"GetImage 失败 hr=0x{hr & 0xFFFFFFFF:08x}")

    hdc = gdi32.CreateCompatibleDC(None)
    bmi = BITMAPINFO()
    bmi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bmi.bmiHeader.biWidth = size
    bmi.bmiHeader.biHeight = -size
    bmi.bmiHeader.biPlanes = 1
    bmi.bmiHeader.biBitCount = 32
    bmi.bmiHeader.biCompression = 0
    buf = ctypes.create_string_buffer(size * size * 4)
    got = gdi32.GetDIBits(hdc, hbmp, 0, size, buf, ctypes.byref(bmi), DIB_RGB_COLORS)
    gdi32.DeleteDC(hdc)
    gdi32.DeleteObject(hbmp)
    if got != size:
        raise OSError(f"GetDIBits 只取到 {got} 行")
    return buf.raw


_CRC_TABLE = None


def _crc(data):
    global _CRC_TABLE
    if _CRC_TABLE is None:
        _CRC_TABLE = []
        for n in range(256):
            c = n
            for _ in range(8):
                c = (0xEDB88320 ^ (c >> 1)) if c & 1 else (c >> 1)
            _CRC_TABLE.append(c)
    crc = 0xFFFFFFFF
    for b in data:
        crc = _CRC_TABLE[(crc ^ b) & 0xFF] ^ (crc >> 8)
    return crc ^ 0xFFFFFFFF


def write_png(path, buf, size):
    """把 32bpp RGBA 原始缓冲写成 PNG（自带 zlib，不依赖额外库）"""
    import struct
    import zlib

    raw = bytearray()
    for y in range(size):
        raw += b"\x00"
        row = buf[y * size * 4 : (y + 1) * size * 4]
        for x in range(size):
            b, g, r, a = row[x * 4 : x * 4 + 4]
            raw += bytes((r, g, b, a))

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", _crc(body))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    open(path, "wb").write(png)


def main(path):
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    for size in (16, 32, 48):
        raw = get_image(path, size)
        # HBITMAP 是预乘 alpha，转回直通 alpha 再写 PNG
        out = bytearray(raw)
        opaque = 0
        for i in range(0, len(out), 4):
            a = out[i + 3]
            if a == 0:
                out[i] = out[i + 1] = out[i + 2] = 0
            elif a < 255:
                for c in range(3):
                    out[i + c] = min(255, out[i + c] * 255 // a)
            else:
                opaque += 1
        name = f"shell-icon-{size}.png"
        write_png(name, bytes(out), size)
        print(f"{size}x{size}: 完全不透明像素 {opaque}/{size * size} -> {name}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "target/release/icon-tool.exe")
