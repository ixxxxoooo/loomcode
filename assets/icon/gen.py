#!/usr/bin/env python3
"""生成 LoomCode 图标的 SVG 源文件:浅色 / 深色 App 图标 + 菜单栏托盘图标。

设计:3 根经线(深色)与 3 根主题橙色纬线织成一小块 3×3 平纹布——「织机」的意象,
寓意把会话、终端、改动编织在一起;线头伸出织纹区,一眼看出是「线」。托盘图标是同一图形的单色版。
渲染:`render.py`(AppKit 不支持 SVG 遮罩,所以「压在下面」靠背景色描边留缝实现)。

@author ixxxxoooo
"""
import pathlib

OUT = pathlib.Path(__file__).parent
N, BAND, GAP = 3, 112, 26          # 3×3 织纹:带宽与带间缝
SPAN = N * BAND + (N - 1) * GAP     # 织纹区边长
ORIGIN = 512 - SPAN / 2
TAIL = 66                           # 线头伸出织纹区的长度


def band_pos(i):
    """第 i 根带子的起点坐标(经线取 x,纬线取 y)。参数:i 序号。返回值:坐标。"""
    return ORIGIN + i * (BAND + GAP)


def weave(ink, thread, gap):
    """织纹图形:经线 ink 竖排、纬线 thread 横排,交叉处按棋盘交替压上 / 穿下。

    参数:ink 经线色;thread 纬线色;gap 背景色(切口用,须与背景一致)。
    返回值:SVG 片段。规则:(i + j) 为偶数的交叉格经线在上,否则纬线在上。
    """
    r = BAND / 2
    lo, hi = ORIGIN - TAIL, ORIGIN + SPAN + TAIL
    out = []
    for j in range(N):  # 纬线整条
        y = band_pos(j)
        out.append(f'<rect x="{lo}" y="{y}" width="{hi - lo}" height="{BAND}" rx="{r}" fill="{thread}"/>')
    for i in range(N):  # 经线整条
        x = band_pos(i)
        out.append(f'<rect x="{x}" y="{lo}" width="{BAND}" height="{hi - lo}" rx="{r}" fill="{ink}"/>')
    cut = 14
    for i in range(N):
        for j in range(N):
            x, y = band_pos(i), band_pos(j)
            if (i + j) % 2 == 1:  # 纬线压在经线上:重画这一格纬线,并用背景色切开上下方的经线
                out.append(f'<rect x="{x}" y="{y - cut}" width="{BAND}" height="{cut}" fill="{gap}"/>')
                out.append(f'<rect x="{x}" y="{y + BAND}" width="{BAND}" height="{cut}" fill="{gap}"/>')
                out.append(f'<rect x="{x - 1}" y="{y}" width="{BAND + 2}" height="{BAND}" fill="{thread}"/>')
            else:  # 经线压在纬线上:切开左右两侧的纬线
                out.append(f'<rect x="{x - cut}" y="{y}" width="{cut}" height="{BAND}" fill="{gap}"/>')
                out.append(f'<rect x="{x + BAND}" y="{y}" width="{cut}" height="{BAND}" fill="{gap}"/>')
                out.append(f'<rect x="{x}" y="{y - 1}" width="{BAND}" height="{BAND + 2}" fill="{ink}"/>')
    return "\n  ".join(out)


def app_icon(dark):
    """1024×1024 App 图标(macOS 图标网格:824 圆角方块居中)。参数:dark 是否深色。返回值:SVG。"""
    if dark:
        bg0, bg1, ink, t0, t1, rim_op, shade = "#2C2B27", "#151411", "#EDEAE2", "#FF9A5C", "#FF5A0F", "0.16", "0.5"
    else:
        bg0, bg1, ink, t0, t1, rim_op, shade = "#FFFEFB", "#EAE7DE", "#26251E", "#FF8A3D", "#EE4A00", "0.95", "0.2"
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<defs>
  <linearGradient id="bg" x1="0" y1="100" x2="0" y2="924" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="{bg0}"/><stop offset="1" stop-color="{bg1}"/></linearGradient>
  <linearGradient id="th" x1="250" y1="250" x2="774" y2="774" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="{t0}"/><stop offset="1" stop-color="{t1}"/></linearGradient>
  <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="{rim_op}"/><stop offset="0.5" stop-color="#fff" stop-opacity="0"/></linearGradient>
  <filter id="sh" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="#000" flood-opacity="{shade}"/></filter>
</defs>
<rect x="100" y="100" width="824" height="824" rx="186" fill="url(#bg)" filter="url(#sh)"/>
<rect x="102" y="102" width="820" height="820" rx="184" fill="none" stroke="url(#rim)" stroke-width="4"/>
  {weave(ink, "url(#th)", "url(#bg)")}
</svg>
'''


def tray_icon():
    """托盘图标源图(36×36,即 18pt@2x):白底黑线,`render.py --template` 转成黑色 + 透明的模板图。

    单色下经纬同色,靠切口区分上下:上层格两侧(沿下层带子方向)各切开 CUT 像素。返回值:SVG。
    """
    b, g, cut, tail = 6.5, 4, 1.8, 4.25
    org = tail
    pos = [org + i * (b + g) for i in range(N)]
    lo, hi = 0.5, 35.5
    out = [f'<rect x="{lo}" y="{y}" width="{hi - lo}" height="{b}" rx="{b / 2}" fill="#000"/>' for y in pos]
    out += [f'<rect x="{x}" y="{lo}" width="{b}" height="{hi - lo}" rx="{b / 2}" fill="#000"/>' for x in pos]
    for i, x in enumerate(pos):
        for j, y in enumerate(pos):
            if (i + j) % 2 == 1:  # 纬线在上:切开这一格上下方的经线
                out.append(f'<rect x="{x}" y="{y - cut}" width="{b}" height="{cut}" fill="#fff"/>')
                out.append(f'<rect x="{x}" y="{y + b}" width="{b}" height="{cut}" fill="#fff"/>')
            else:  # 经线在上:切开这一格左右的纬线
                out.append(f'<rect x="{x - cut}" y="{y}" width="{cut}" height="{b}" fill="#fff"/>')
                out.append(f'<rect x="{x + b}" y="{y}" width="{cut}" height="{b}" fill="#fff"/>')
    body = "\n  ".join(out)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="36" height="36" viewBox="0 0 36 36">
<rect width="36" height="36" fill="#fff"/>
  {body}
</svg>
'''


if __name__ == "__main__":
    (OUT / "app-light.svg").write_text(app_icon(False))
    (OUT / "app-dark.svg").write_text(app_icon(True))
    (OUT / "tray.svg").write_text(tray_icon())
