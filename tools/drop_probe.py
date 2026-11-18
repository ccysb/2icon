"""模拟资源管理器拖放：在目标进程里构造 DROPFILES 并投递 WM_DROPFILES。

用法: python tools/drop_probe.py [要拖入的文件]
用于验证宿主增强里的拖放处理链路是否真的接上了。
"""

import ctypes
import importlib.util
import os
import struct
import subprocess
import sys
import time
from ctypes import wintypes

spec = importlib.util.spec_from_file_location(
    "probe", os.path.join(os.path.dirname(os.path.abspath(__file__)), "ui_probe.py")
)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

k32 = ctypes.windll.kernel32
k32.OpenProcess.restype = wintypes.HANDLE
k32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
k32.VirtualAllocEx.restype = ctypes.c_void_p
k32.VirtualAllocEx.argtypes = [
    wintypes.HANDLE,
    ctypes.c_void_p,
    ctypes.c_size_t,
    wintypes.DWORD,
    wintypes.DWORD,
]
k32.WriteProcessMemory.argtypes = [
    wintypes.HANDLE,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_size_t,
    ctypes.POINTER(ctypes.c_size_t),
]
k32.WriteProcessMemory.restype = wintypes.BOOL

PROCESS_ALL = 0x1F0FFF
MEM_COMMIT_RESERVE = 0x3000
PAGE_RW = 0x04
WM_DROPFILES = 0x0233


def send_drop(hwnd, pid, path):
    """在 pid 的地址空间里写一份单文件 DROPFILES，然后同步投递消息。"""
    paths = (path.replace("/", "\\") + "\0\0").encode("utf-16-le")
    header = struct.pack("<IiiII", 20, 0, 0, 0, 1)  # pFiles/pt/fNC/fWide
    data = header + paths

    handle = k32.OpenProcess(PROCESS_ALL, False, pid)
    addr = k32.VirtualAllocEx(handle, None, len(data), MEM_COMMIT_RESERVE, PAGE_RW)
    written = ctypes.c_size_t(0)
    ok = k32.WriteProcessMemory(
        handle, ctypes.c_void_p(addr), data, len(data), ctypes.byref(written)
    )
    if not ok:
        raise OSError("写入目标进程失败")
    print(f"已写入 {written.value} 字节到目标进程地址 {hex(addr)}")

    user32 = probe.user32
    user32.SendMessageW.restype = ctypes.c_ssize_t
    user32.SendMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
    res = user32.SendMessageW(
        wintypes.HWND(hwnd), WM_DROPFILES, wintypes.WPARAM(addr), wintypes.LPARAM(0)
    )
    print(f"WM_DROPFILES 处理返回 {res}")
    return res


def main():
    target = sys.argv[1] if len(sys.argv) > 1 else os.path.join(probe.ROOT, "logo.png")
    if not os.path.exists(target):
        print("找不到待拖入文件:", target)
        return 1

    proc = subprocess.Popen([probe.EXE], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        hwnd = None
        for _ in range(60):
            time.sleep(0.25)
            hwnd = probe.find_window()
            if hwnd:
                break
        if not hwnd:
            print("窗口未出现")
            return 1
        time.sleep(1.2)
        probe.topmost(hwnd)
        probe.shot(hwnd, "drop_before.png")
        send_drop(hwnd, proc.pid, target)
        time.sleep(1.5)
        print("进程存活:", proc.poll() is None)
        probe.shot(hwnd, "drop_after.png")
        return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    sys.exit(main())
