#!/usr/bin/env python3
"""按进程名截取 LoomCode 窗口(不抢焦点)。用法:scripts/shot.py [输出路径] [进程名] [进程号]

给了进程号时只截该进程的窗口,避免截到同时运行的开发版。"""
import subprocess, sys
import Quartz

out = sys.argv[1] if len(sys.argv) > 1 else "/tmp/loom-shot.png"
name = sys.argv[2] if len(sys.argv) > 2 else "loomcode"
pid = int(sys.argv[3]) if len(sys.argv) > 3 else None
wins = Quartz.CGWindowListCopyWindowInfo(Quartz.kCGWindowListOptionAll, Quartz.kCGNullWindowID)
cands = [w for w in wins if (w.get("kCGWindowOwnerName") or "").lower() == name and w.get("kCGWindowIsOnscreen") and (pid is None or w.get("kCGWindowOwnerPID") == pid)]
if not cands:
    sys.exit("没有找到窗口")
w = max(cands, key=lambda w: w["kCGWindowBounds"]["Width"] * w["kCGWindowBounds"]["Height"])
subprocess.run(["screencapture", "-x", "-o", f"-l{w['kCGWindowNumber']}", out], check=True)
print(out)
