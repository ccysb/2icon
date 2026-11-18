"""界面交互探查：置顶窗口 + 物理像素截屏 + 物理坐标点击。

用法:
  python tools/ui_probe.py status      # 仅截界面
  python tools/ui_probe.py light       # 切浅色主题
  python tools/ui_probe.py png         # 切 PNG 并保存
  python tools/ui_probe.py webp        # 切 WEBP 并保存
  python tools/ui_probe.py copy        # 复制到剪贴板
  python tools/ui_probe.py filter      # 切换质量过滤
  python tools/ui_probe.py select      # 选中第 2 个图标
  python tools/ui_probe.py custom      # 自定义尺寸 96x96 并应用
  python tools/ui_probe.py about       # 打开关于对话框

坐标是客户区内的物理像素（本机 150% 缩放，客户区 1500x1050）。
窗口会被临时设为置顶以免被其他窗口遮挡，进程退出后自然恢复。
"""

import ctypes
import os
import shutil
import subprocess
import sys
import tempfile
import time
from ctypes import wintypes

from PIL import ImageGrab

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "target", "debug", "icon-tool.exe")
TITLE = "图像/文件图标提取工具"
MAIN_CLASS = "ZsuiMainWindow"
CF_DIB = 8

user32 = ctypes.windll.user32
user32.SetProcessDPIAware()

# 客户区物理坐标，由 probe_status 的物理截图量出
PT = {
    "filter_box": (956, 158),
    "preset_16": (955, 250),
    "preset_32": (1085, 250),
    "preset_48": (1215, 250),
    "preset_64": (1344, 250),
    "preset_96": (955, 303),
    "preset_128": (1085, 303),
    "preset_256": (1215, 303),
    "preset_512": (1344, 303),
    "custom_w": (1133, 350),
    "custom_h": (1235, 350),
    "apply": (1321, 350),
    "reset": (1402, 350),
    "fmt_ico": (955, 725),
    "fmt_png": (1125, 725),
    "fmt_webp": (1295, 725),
    "slider_mid": (1219, 800),
    "save": (1073, 850),
    "copy": (1351, 850),
    "theme": (1054, 1012),
    "about": (1125, 1012),
    "github": (1260, 1012),
    "tile2": (563, 254),
    "tile3": (710, 254),
}


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", ctypes.c_long),
        ("top", ctypes.c_long),
        ("right", ctypes.c_long),
        ("bottom", ctypes.c_long),
    ]


def find_window():
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def cb(hwnd, _l):
        n = user32.GetWindowTextLengthW(hwnd)
        if n <= 0 or not user32.IsWindowVisible(hwnd):
            return True
        buf = ctypes.create_unicode_buffer(n + 1)
        user32.GetWindowTextW(hwnd, buf, n + 1)
        if buf.value != TITLE:
            return True
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, cls, 256)
        if cls.value != MAIN_CLASS:
            return True
        found.append(hwnd)
        return False

    user32.EnumWindows(cb, 0)
    return found[0] if found else None


def client_rect(hwnd):
    r = RECT()
    user32.GetClientRect(hwnd, ctypes.byref(r))
    w, h = r.right - r.left, r.bottom - r.top
    org = wintypes.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(org))
    return org.x, org.y, w, h


def topmost(hwnd):
    """置顶，避免被其他窗口遮挡导致截屏/点击失效。"""
    user32.SetWindowPos(hwnd, wintypes.HWND(-1), 0, 0, 0, 0, 0x0002 | 0x0001 | 0x0010)
    time.sleep(0.45)


def shot(hwnd, name):
    x, y, w, h = client_rect(hwnd)
    img = ImageGrab.grab(bbox=(x, y, x + w, y + h))
    path = os.path.join(ROOT, name)
    img.save(path)
    print(f"截图 {path} {img.size}")
    return img


def click(hwnd, key_or_xy):
    px, py = PT[key_or_xy] if isinstance(key_or_xy, str) else key_or_xy
    x0, y0, w, h = client_rect(hwnd)
    x, y = x0 + px, y0 + py
    user32.SetCursorPos(x, y)
    time.sleep(0.12)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.06)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.8)
    print(f"点击 {key_or_xy} 物理({px},{py}) -> 屏幕({x},{y})")


def type_text(text):
    for ch in text:
        scan = user32.VkKeyScanW(ord(ch))
        vk = scan & 0xFF
        shift = (scan >> 8) & 1
        if shift:
            user32.keybd_event(0x10, 0, 0, 0)
        user32.keybd_event(vk, 0, 0, 0)
        user32.keybd_event(vk, 0, 2, 0)
        if shift:
            user32.keybd_event(0x10, 0, 2, 0)
        time.sleep(0.05)


def dismiss_dialog():
    user32.keybd_event(0x1B, 0, 0, 0)
    user32.keybd_event(0x1B, 0, 2, 0)
    time.sleep(0.6)


def clear_clipboard():
    """先清空，避免把上一轮测试残留的 CF_DIB 当成这次的结果。"""
    if user32.OpenClipboard(None):
        user32.EmptyClipboard()
        user32.CloseClipboard()


def clipboard_has_dib():
    if not user32.OpenClipboard(None):
        return False
    try:
        return bool(user32.IsClipboardFormatAvailable(CF_DIB))
    finally:
        user32.CloseClipboard()


def run_action(hwnd, action, stem):
    if action == "status":
        shot(hwnd, "probe_status.png")
    elif action == "light":
        click(hwnd, "theme")
        shot(hwnd, "probe_light.png")
    elif action in ("png", "webp"):
        out = f"{stem}.{action}"
        if os.path.exists(out):
            os.remove(out)
        click(hwnd, f"fmt_{action}")
        click(hwnd, "save")
        shot(hwnd, f"probe_{action}.png")
        dismiss_dialog()
        print(f"{action.upper()} 存在:", os.path.exists(out), os.path.getsize(out) if os.path.exists(out) else 0)
    elif action == "copy":
        clear_clipboard()
        click(hwnd, "copy")
        shot(hwnd, "probe_copy.png")
        print("剪贴板含 CF_DIB:", clipboard_has_dib())
        dismiss_dialog()
    elif action == "filter":
        click(hwnd, "filter_box")
        shot(hwnd, "probe_filter.png")
    elif action == "select":
        click(hwnd, "tile2")
        shot(hwnd, "probe_select.png")
    elif action == "custom":
        click(hwnd, "custom_w")
        type_text("96")
        click(hwnd, "custom_h")
        type_text("96")
        click(hwnd, "apply")
        shot(hwnd, "probe_custom.png")
    elif action == "about":
        click(hwnd, "about")
        shot(hwnd, "probe_about.png")
        dismiss_dialog()
    else:
        print("未知动作", action)
        return 1
    return 0


def main():
    action = sys.argv[1] if len(sys.argv) > 1 else "status"
    sample = os.path.join(tempfile.gettempdir(), "icontool-test.exe")
    if not os.path.exists(sample):
        shutil.copy(r"C:\Windows\System32\notepad.exe", sample)
    stem = os.path.splitext(sample)[0]

    proc = subprocess.Popen([EXE, sample], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        hwnd = None
        for _ in range(60):
            time.sleep(0.25)
            hwnd = find_window()
            if hwnd:
                break
        if not hwnd:
            print("窗口未出现")
            return 1
        time.sleep(1.2)
        topmost(hwnd)
        return run_action(hwnd, action, stem)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    sys.exit(main())
