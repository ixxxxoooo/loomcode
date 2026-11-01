#!/usr/bin/env python3
"""用 AppKit 把 SVG 渲染成 PNG。用法:render.py 输入.svg 输出.png 边长像素 [--template]

--template:把白底黑线转成「黑色 + 透明度」的菜单栏模板图。

@author ixxxxoooo
"""
import sys
import AppKit

src, dst, px = sys.argv[1], sys.argv[2], int(sys.argv[3])
template = "--template" in sys.argv
img = AppKit.NSImage.alloc().initWithContentsOfFile_(src)
if img is None:
    sys.exit(f"无法读取 {src}")
rep = AppKit.NSBitmapImageRep.alloc().initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel_(
    None, px, px, 8, 4, True, False, AppKit.NSDeviceRGBColorSpace, 0, 0)
rep.setSize_((px, px))
AppKit.NSGraphicsContext.saveGraphicsState()
ctx = AppKit.NSGraphicsContext.graphicsContextWithBitmapImageRep_(rep)
ctx.setImageInterpolation_(AppKit.NSImageInterpolationHigh)
AppKit.NSGraphicsContext.setCurrentContext_(ctx)
img.drawInRect_fromRect_operation_fraction_(((0, 0), (px, px)), AppKit.NSZeroRect, AppKit.NSCompositingOperationSourceOver, 1.0)
ctx.flushGraphics()
AppKit.NSGraphicsContext.restoreGraphicsState()
rep.representationUsingType_properties_(AppKit.NSBitmapImageFileTypePNG, None).writeToFile_atomically_(dst, True)

if template:
    from PIL import Image
    im = Image.open(dst).convert("L")
    out = Image.new("RGBA", im.size, (0, 0, 0, 0))
    out.putalpha(im.point(lambda v: 255 - v))
    out.save(dst)
