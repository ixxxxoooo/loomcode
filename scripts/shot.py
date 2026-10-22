#!/usr/bin/env python3
"""按进程名截取 UCode 窗口(不抢焦点)。用法:scripts/shot.py [输出路径] [进程名]"""
import subprocess, sys
import Quartz

out = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ucode-shot.png"
name = sys.argv[2] if len(sys.argv) > 2 else "ucode"
wins = Quartz.CGWindowListCopyWindowInfo(Quartz.kCGWindowListOptionAll, Quartz.kCGNullWindowID)
cands = [w for w in wins if (w.get("kCGWindowOwnerName") or "").lower() == name and w.get("kCGWindowIsOnscreen")]
if not cands:
    sys.exit("没有找到窗口")
w = max(cands, key=lambda w: w["kCGWindowBounds"]["Width"] * w["kCGWindowBounds"]["Height"])
subprocess.run(["screencapture", "-x", "-o", f"-l{w['kCGWindowNumber']}", out], check=True)
print(out)
