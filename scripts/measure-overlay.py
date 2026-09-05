"""Measure only an owned overlay-preview process; never touch the live guardian.

Usage: python scripts/measure-overlay.py HELPER_EXE OUTPUT_JSON
"""
import ctypes
from ctypes import wintypes as w
import json
from pathlib import Path
import subprocess
import sys
import time

user = ctypes.WinDLL("user32", use_last_error=True)
kernel = ctypes.WinDLL("kernel32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)


class Memory(ctypes.Structure):
    _fields_ = [("cb", w.DWORD), ("PageFaultCount", w.DWORD)] + [
        (name, ctypes.c_size_t) for name in (
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
            "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage",
            "PagefileUsage", "PeakPagefileUsage", "PrivateUsage")]


user.FindWindowW.argtypes = [w.LPCWSTR, w.LPCWSTR]
user.FindWindowW.restype = w.HWND
user.GetClientRect.argtypes = [w.HWND, ctypes.POINTER(w.RECT)]
user.GetWindowRect.argtypes = [w.HWND, ctypes.POINTER(w.RECT)]
user.GetDpiForWindow.argtypes = [w.HWND]
user.SendMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
user.SendMessageW.restype = w.LPARAM
user.SetThreadDpiAwarenessContext.argtypes = [w.HANDLE]
user.GetGuiResources.argtypes = [w.HANDLE, w.DWORD]
kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
kernel.OpenProcess.restype = w.HANDLE
kernel.CloseHandle.argtypes = [w.HANDLE]
kernel.GetProcessTimes.argtypes = [w.HANDLE] + [ctypes.POINTER(w.FILETIME)] * 4
psapi.GetProcessMemoryInfo.argtypes = [w.HANDLE, ctypes.POINTER(Memory), w.DWORD]
user.SetThreadDpiAwarenessContext(w.HANDLE(-4))


def snapshot(handle):
    times = [w.FILETIME() for _ in range(4)]
    if not kernel.GetProcessTimes(handle, *(ctypes.byref(t) for t in times)):
        raise ctypes.WinError(ctypes.get_last_error())
    cpu = sum((t.dwHighDateTime << 32) + t.dwLowDateTime for t in times[2:]) / 10_000_000
    memory = Memory()
    memory.cb = ctypes.sizeof(memory)
    if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb):
        raise ctypes.WinError(ctypes.get_last_error())
    return dict(at=time.perf_counter(), cpu=cpu, private_bytes=memory.PrivateUsage,
                working_set=memory.WorkingSetSize, gdi=user.GetGuiResources(handle, 0),
                user=user.GetGuiResources(handle, 1))


def sample(handle, seconds):
    rows = [snapshot(handle)]
    deadline = time.perf_counter() + seconds
    while time.perf_counter() < deadline:
        time.sleep(0.1)
        rows.append(snapshot(handle))
    duration = rows[-1]["at"] - rows[0]["at"]
    return dict(seconds=round(duration, 3),
                cpu_percent_one_core=round((rows[-1]["cpu"] - rows[0]["cpu"]) / duration * 100, 3),
                private_bytes_start=rows[0]["private_bytes"], private_bytes_end=rows[-1]["private_bytes"],
                working_set_max=max(row["working_set"] for row in rows),
                gdi_min=min(row["gdi"] for row in rows), gdi_max=max(row["gdi"] for row in rows),
                user_min=min(row["user"] for row in rows), user_max=max(row["user"] for row in rows))


def click(window, collapsed):
    bounds = w.RECT()
    user.GetClientRect(window, ctypes.byref(bounds))
    dpi = max(96, user.GetDpiForWindow(window))
    x = bounds.right // 2
    y = bounds.bottom // 2 if collapsed else round(60 * dpi / 96)
    point = (y << 16) | x
    user.SendMessageW(window, 0x0201, 0, point)
    user.SendMessageW(window, 0x0202, 0, point)
    time.sleep(0.9)
    user.GetClientRect(window, ctypes.byref(bounds))
    expected_tab = round(28 * dpi / 96)
    assert (bounds.right <= expected_tab + 2) != collapsed, "preview did not toggle as expected"


def window_bounds(window):
    rect = w.RECT()
    assert user.GetWindowRect(window, ctypes.byref(rect))
    return (rect.left, rect.top, rect.right, rect.bottom)


def toggle_all(windows, collapsed):
    for window in windows:
        others = {other: window_bounds(other) for other in windows if other != window}
        click(window, collapsed)
        assert all(window_bounds(other) == bounds for other, bounds in others.items()), \
            "toggling one chat moved another chat's window"


def main():
    helper = Path(sys.argv[1]).resolve(strict=True)
    output = Path(sys.argv[2]).resolve()
    process = subprocess.Popen([str(helper), "overlay-preview"], stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               creationflags=subprocess.CREATE_NO_WINDOW)
    handle = kernel.OpenProcess(0x0400 | 0x0010, False, process.pid)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    started = time.perf_counter()
    try:
        window = None
        for _ in range(30):
            window = user.FindWindowW(f"CodexLidGuardMessageOverlay.{process.pid}", None)
            if window:
                break
            time.sleep(0.1)
        assert window, "owned preview window did not appear"
        windows = [window]
        for slot in [1, 2]:
            extra = user.FindWindowW(f"CodexLidGuardMessageOverlay.{process.pid}.{slot}", None)
            if extra:
                windows.append(extra)
        time.sleep(0.4)
        # Current previews arrive as tabs; expand them for the same phase baseline.
        for owned_window in windows:
            bounds = window_bounds(owned_window)
            if bounds[2] - bounds[0] <= round(28 * max(96, user.GetDpiForWindow(owned_window)) / 96) + 2:
                click(owned_window, True)
        results = {"helper": str(helper), "pid": process.pid, "panels": len(windows), "phases": {}}
        results["phases"]["expanded_busy"] = sample(handle, 3)
        toggle_all(windows, False)
        results["phases"]["collapsed_busy"] = sample(handle, 3)
        toggle_all(windows, True)
        time.sleep(max(0, 16 + (len(windows) - 1) * 3 - (time.perf_counter() - started)))
        results["phases"]["expanded_complete"] = sample(handle, 3)
        toggle_all(windows, False)
        results["phases"]["collapsed_complete"] = sample(handle, 2)
        toggle_all(windows, True)
        results["phases"]["reopened_complete"] = sample(handle, 1)
        process.wait(timeout=10)
        assert process.returncode == 0, "preview failed"
        results["duration_seconds"] = round(time.perf_counter() - started, 3)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(results, indent=2), encoding="utf-8")
        print(json.dumps(results))
    finally:
        if process.poll() is None:
            process.terminate()  # Only the exact child this test created.
            process.wait(timeout=5)
        kernel.CloseHandle(handle)


if __name__ == "__main__":
    main()
