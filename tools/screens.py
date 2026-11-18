"""生成 README 用的界面截图（深色 / 浅色）。

用法: python tools/screens.py [用于截图的图片，默认 logo.png]
输出: docs/screenshot-dark.png、docs/screenshot-light.png
"""

import importlib.util
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("probe", os.path.join(HERE, "ui_probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

DOCS = os.path.join(probe.ROOT, "docs")


def main():
    sample = sys.argv[1] if len(sys.argv) > 1 else os.path.join(probe.ROOT, "logo.png")
    os.makedirs(DOCS, exist_ok=True)
    # EXE 与输出图都放到 docs 下，避免污染工程根目录
    exe = os.path.join(probe.ROOT, "target", "release", "icon-tool.exe")
    if not os.path.exists(exe):
        exe = probe.EXE

    proc = subprocess.Popen([exe, sample], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
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
        time.sleep(1.5)
        probe.topmost(hwnd)

        dark = os.path.join(DOCS, "screenshot-dark.png")
        light = os.path.join(DOCS, "screenshot-light.png")
        probe.shot(hwnd, "screenshot-dark.png")
        os.replace(os.path.join(probe.ROOT, "screenshot-dark.png"), dark)
        probe.click(hwnd, "theme")
        # 切主题后合成表面要慢一帧，立刻截图会拍到旧配色
        time.sleep(0.8)
        probe.shot(hwnd, "screenshot-light.png")
        os.replace(os.path.join(probe.ROOT, "screenshot-light.png"), light)
        print("已生成:", dark, light)
        return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    sys.exit(main())
