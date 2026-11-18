"""检查 exe 的 PE 资源段里有没有图标资源（RT_GROUP_ICON / RT_ICON）。"""

import struct
import sys


def u16(b, o):
    return struct.unpack_from("<H", b, o)[0]


def u32(b, o):
    return struct.unpack_from("<I", b, o)[0]


def walk(data, base, off, depth, path, found):
    named, ids = u16(data, base + off + 12), u16(data, base + off + 14)
    for i in range(named + ids):
        ent = base + off + 16 + i * 8
        name_raw = u32(data, ent)
        sub = u32(data, ent + 4)
        ident = name_raw if name_raw & 0x80000000 == 0 else -1
        label = ident if ident >= 0 else "<named>"
        if sub & 0x80000000:
            walk(data, base, sub & 0x7FFFFFFF, depth + 1, path + [label], found)
        else:
            entry = base + (sub & 0x7FFFFFFF)
            rva, size = u32(data, entry), u32(data, entry + 4)
            found.append((path + [label], rva, size))


def main(path):
    # CI 的 Python 默认用 cp1252 写 stdout，中文会直接抛 UnicodeEncodeError
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    data = open(path, "rb").read()
    pe = u32(data, 0x3C)
    assert data[pe : pe + 4] == b"PE\0\0", "不是 PE 文件"
    nsect = u16(data, pe + 6)
    opt_size = u16(data, pe + 20)
    opt = pe + 24
    magic = u16(data, opt)
    dd = opt + (96 if magic == 0x10B else 112)
    res_rva, res_size = u32(data, dd + 2 * 8), u32(data, dd + 2 * 8 + 4)
    print(f"资源表 RVA=0x{res_rva:x} 大小={res_size}")
    if res_rva == 0:
        print("没有资源表 -> exe 无图标")
        return 1
    # RVA -> 文件偏移：按节表换算
    sect = opt + opt_size
    off = None
    for i in range(nsect):
        s = sect + i * 40
        va, raw_size, raw_ptr = u32(data, s + 12), u32(data, s + 16), u32(data, s + 20)
        if va <= res_rva < va + max(raw_size, 1):
            off = res_rva - va + raw_ptr
            break
    if off is None:
        print("资源表不在任何节内")
        return 1
    found = []
    walk(data, off, 0, 0, [], found)
    groups = [f for f in found if f[0][0] == 14]
    icons = [f for f in found if f[0][0] == 3]
    print(f"RT_ICON(3) 条目 {len(icons)} 个，RT_GROUP_ICON(14) 条目 {len(groups)} 个")
    for key, rva, size in groups:
        print(f"  组 {key} -> {size} 字节 @RVA 0x{rva:x}")
        grp = next((f for f in found if f[1] == rva), None)
        if grp:
            goff = None
            for i in range(nsect):
                s = sect + i * 40
                va, raw_size, raw_ptr = u32(data, s + 12), u32(data, s + 16), u32(data, s + 20)
                if va <= rva < va + max(raw_size, 1):
                    goff = rva - va + raw_ptr
                    break
            if goff is not None:
                blob = data[goff : goff + size]
                n = u16(blob, 4)
                sizes = []
                for i in range(n):
                    e = 6 + i * 14
                    w = 256 if blob[e] == 0 else blob[e]
                    h = 256 if blob[e + 1] == 0 else blob[e + 1]
                    sizes.append(f"{w}x{h}")
                print("    帧: " + ", ".join(sizes))
    return 0 if groups else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
