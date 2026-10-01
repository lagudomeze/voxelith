"""抓某个进程窗口的内容（PrintWindow，不受遮挡影响）。

## 为什么要写成文件而不是内联 here-string

我的截图脚本反复出问题（缩进、转义、路径推导）。写成文件、用 `read`/`edit` 维护，
比每次现写一段 PowerShell 可靠得多。

## 为什么不用 `ImageGrab`

`ImageGrab` 抓的是**屏幕最上层**的像素，不是"我要的那个窗口"。
我因此连续截到浏览器/IDE 好几次。`PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT)`
是**按句柄**抓，才是指哪打哪。

用法：
    python probe/glb/capture_window.py <进程名> <输出png>
"""

import ctypes
import ctypes.wintypes as w
import subprocess
import sys
from pathlib import Path

from PIL import Image

USER32 = ctypes.windll.user32
GDI32 = ctypes.windll.gdi32
USER32.SetProcessDPIAware()

# PW_RENDERFULLCONTENT = 2。不加这个拿不到 D3D 渲染出来的内容（会是全黑）。
PW_RENDERFULLCONTENT = 2


class BitmapInfoHeader(ctypes.Structure):
    _fields_ = [
        ("biSize", w.DWORD),
        ("biWidth", ctypes.c_long),
        ("biHeight", ctypes.c_long),
        ("biPlanes", w.WORD),
        ("biBitCount", w.WORD),
        ("biCompression", w.DWORD),
        ("biSizeImage", w.DWORD),
        ("biXPelsPerMeter", ctypes.c_long),
        ("biYPelsPerMeter", ctypes.c_long),
        ("biClrUsed", w.DWORD),
        ("biClrImportant", w.DWORD),
    ]


def pids_of(image_name):
    """按可执行文件名查 PID。"""
    out = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {image_name}", "/FO", "CSV", "/NH"],
        capture_output=True,
        text=True,
    ).stdout
    pids = set()
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) > 1 and parts[0].lower() == image_name.lower():
            pids.add(int(parts[1].replace(",", "")))
    return pids


def find_window(image_name):
    """找该进程的**可见顶层窗口**句柄。"""
    pids = pids_of(image_name)
    if not pids:
        return None
    found = []

    def callback(hwnd, _):
        pid = w.DWORD()
        USER32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in pids and USER32.IsWindowVisible(hwnd):
            length = USER32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(length + 1)
            USER32.GetWindowTextW(hwnd, buf, length + 1)
            if buf.value:  # 只要有标题就算
                found.append((hwnd, buf.value))
        return True

    USER32.EnumWindows(ctypes.WINFUNCTYPE(ctypes.c_bool, w.HWND, w.LPARAM)(callback), 0)
    return found[0] if found else None


def capture(image_name, out_path):
    hit = find_window(image_name)
    if not hit:
        return f"找不到 {image_name} 的窗口（进程在跑吗？）"
    hwnd, title = hit
    rect = w.RECT()
    USER32.GetWindowRect(hwnd, ctypes.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top
    hdc = USER32.GetWindowDC(hwnd)
    mem = GDI32.CreateCompatibleDC(hdc)
    bmp = GDI32.CreateCompatibleBitmap(hdc, width, height)
    GDI32.SelectObject(mem, bmp)
    USER32.PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT)
    info = BitmapInfoHeader()
    info.biSize = ctypes.sizeof(info)
    info.biWidth = width
    info.biHeight = -height
    info.biPlanes = 1
    info.biBitCount = 32
    info.biCompression = 0
    buf = ctypes.create_string_buffer(width * height * 4)
    GDI32.GetDIBits(mem, bmp, 0, height, buf, ctypes.byref(info), 0)
    Image.frombuffer("RGBA", (width, height), buf, "raw", "BGRA", 0, 1).convert("RGB").save(
        out_path
    )
    GDI32.DeleteObject(bmp)
    GDI32.DeleteDC(mem)
    USER32.ReleaseDC(hwnd, hdc)
    return f"已存 {out_path}（{width}x{height}，标题：{title}）"


def main():
    if len(sys.argv) != 3:
        return "用法：capture_window.py <进程名> <输出png>"
    print(capture(sys.argv[1], Path(sys.argv[2]).resolve()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
