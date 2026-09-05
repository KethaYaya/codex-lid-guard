"""Measure geometry updates in an owned preview, without moving the user's cursor.

This measures native frame delivery, not compositor/display presentation.
Usage: python scripts/measure-overlay-motion.py HELPER_EXE OUTPUT_JSON
"""
import ctypes
from ctypes import wintypes as w
import importlib.util
import json
from pathlib import Path
import statistics
import subprocess
import sys
import time

spec = importlib.util.spec_from_file_location("overlay_metrics", Path(__file__).with_name("measure-overlay.py"))
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)
user = metrics.user
user.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]


def observe(window, trigger):
    original = metrics.window_bounds(window)
    started = time.perf_counter()
    trigger()
    changes = []
    previous = original
    while time.perf_counter() - started < 0.4:
        bounds = metrics.window_bounds(window)
        if bounds != previous:
            changes.append((time.perf_counter() - started) * 1000)
            previous = bounds
        time.sleep(0.001)
    assert changes, "no animation frames observed"
    # Exclude the cubic easing's final rounded pixels, which intentionally stop changing.
    intervals = [b - a for a, b in zip(changes, changes[1:]) if b <= 195]
    assert intervals
    return dict(first_change_ms=round(changes[0], 3), changes=len(changes),
                interval_median_ms=round(statistics.median(intervals), 3),
                interval_p95_ms=round(sorted(intervals)[min(len(intervals)-1, int(len(intervals)*.95))], 3),
                final_change_ms=round(changes[-1], 3))


def main():
    helper = Path(sys.argv[1]).resolve(strict=True)
    output = Path(sys.argv[2]).resolve()
    process = subprocess.Popen([str(helper), "overlay-preview"], stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               creationflags=subprocess.CREATE_NO_WINDOW)
    handle = metrics.kernel.OpenProcess(0x0400 | 0x0010, False, process.pid)
    try:
        deadline = time.perf_counter() + 4
        windows = []
        while time.perf_counter() < deadline:
            windows = [user.FindWindowW(f"CodexLidGuardMessageOverlay.{process.pid}{suffix}", None)
                       for suffix in ["", ".1", ".2"]]
            if all(windows):
                break
            time.sleep(.02)
        assert all(windows)
        time.sleep(.6)
        # Identical background indicator workload in both old/new builds.
        for window in windows:
            rect = metrics.window_bounds(window)
            if rect[2] - rect[0] > 60:
                metrics.click(window, False)
        window = windows[1]
        before = metrics.snapshot(handle)
        expanded = []
        for _ in range(6):
            others = [metrics.window_bounds(other) for other in (windows[0], windows[2])]
            expanded.append(observe(window, lambda: user.PostMessageW(window, 0x8003, 0, 0)))
            assert others == [metrics.window_bounds(other) for other in (windows[0], windows[2])]
            metrics.click(window, False)
        after = metrics.snapshot(handle)
        result = dict(helper=str(helper), scope="native geometry delivery; six expansions of one of three tabs",
                      expanded=expanded, cpu_percent_one_core=round((after['cpu']-before['cpu']) /
                          (after['at']-before['at']) * 100, 3), before=before, after=after)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(json.dumps(result))
    finally:
        if process.poll() is None:
            process.terminate()  # Only this benchmark's own child.
            process.wait(timeout=5)
        metrics.kernel.CloseHandle(handle)


if __name__ == "__main__":
    main()
