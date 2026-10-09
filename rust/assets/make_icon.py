# -*- coding: utf-8 -*-
"""
AfterburnerWebMonitor 图标生成器 —— 方案 A：仪表弧 + 火焰指针

设计要点：
  * 深色圆角方块背景（Windows 应用图标规范，圆角 22%）
  * 半圆仪表弧：暗色轨道 + 橙色渐变进度
  * 火焰形指针：从弧心沿 290 度方向生长，外焰/中焰/内焰三层渐变
  * 元素少、线条粗，保证 16x16 托盘尺寸下依然可辨

在 1024x1024 上绘制后缩放，等效 4 倍超采样抗锯齿。
"""
import math
import os
from PIL import Image, ImageDraw, ImageFilter

S = 1024                      # 绘制画布（最终缩到 256）
OUT_DIR = os.path.dirname(os.path.abspath(__file__))
ICO_PATH = os.path.join(OUT_DIR, "AfterburnerWebMonitor.ico")
PNG_PATH = os.path.join(OUT_DIR, "preview_256.png")

# ---------- 工具函数 ----------
def mix(c1, c2, t):
    t = max(0.0, min(1.0, t))
    return tuple(int(round(c1[i] + (c2[i] - c1[i]) * t)) for i in range(3))


def ri(seq):
    """把浮点坐标序列转成整数元组，避免某些 PIL 版本对 float 的挑剔"""
    return tuple(int(round(v)) for v in seq)


def rounded_mask(size, radius):
    m = Image.new("L", (size, size), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=255)
    return m


# ---------- 主绘制 ----------
def build():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    # ===== 1. 背景：竖向渐变 =====
    bg = Image.new("RGB", (S, S))
    bd = ImageDraw.Draw(bg)
    top, bot = (54, 64, 88), (17, 19, 28)
    for y in range(S):
        bd.line([(0, y), (S, y)], fill=mix(top, bot, y / (S - 1)))

    # 左上柔光，避免背景死板
    sheen = Image.new("L", (S, S), 0)
    ImageDraw.Draw(sheen).ellipse([-420, -560, 720, 380], fill=190)
    sheen = sheen.filter(ImageFilter.GaussianBlur(150))
    bg = Image.composite(Image.new("RGB", (S, S), (78, 92, 122)), bg, sheen.point(lambda v: int(v * 0.30)))

    img.paste(bg, (0, 0), rounded_mask(S, 228))

    # ===== 2. 仪表弧参数 =====
    cx, cy = 512.0, 654.0
    R = 330.0            # 弧半径（中心线）
    W = 58.0             # 弧线宽

    # ===== 3. 火焰几何（先算出来，用于背景辉光） =====
    ang = math.radians(272.0)                 # 火焰竖直向上，略偏右
    d = (math.cos(ang), math.sin(ang))        # 中心线方向（图像坐标，y 向下）
    p = (-d[1], d[0])                         # 垂直方向

    L = 272.0                                 # 指针（火焰）长度
    WMAX = 74.0                               # 火焰最大半宽
    BEND = 52.0                               # 尖端弯曲量（大 -> 火苗有风感）

    def flame_width(t, wmax):
        """火苗宽度包络：根部略窄、中下部最宽、向上平滑收细至尖端归零"""
        t = max(0.0, min(1.0, t))
        pts = [(0.00, 0.55), (0.12, 0.82), (0.25, 1.00), (0.42, 0.87),
               (0.60, 0.64), (0.76, 0.41), (0.89, 0.21), (1.00, 0.00)]
        for i in range(len(pts) - 1):
            t0, v0 = pts[i]
            t1, v1 = pts[i + 1]
            if t <= t1:
                k = 0.0 if t1 == t0 else (t - t0) / (t1 - t0)
                k = k * k * (3 - 2 * k)          # smoothstep，过渡更柔顺
                return wmax * (v0 + (v1 - v0) * k)
        return 0.0

    def flame_polygon(wmax, length, start_off, bend, steps=120):
        """生成细长火苗轮廓：根部宽润、上部收细、尖端微弯"""
        left, right = [], []
        for i in range(steps + 1):
            t = i / steps
            w = flame_width(t, wmax)
            bx = bend * (t ** 2.2)
            X = cx + d[0] * (start_off + length * t) - p[0] * bx
            Y = cy + d[1] * (start_off + length * t) - p[1] * bx
            left.append((X + p[0] * w, Y + p[1] * w))
            right.append((X - p[0] * w, Y - p[1] * w))
        return left + right[::-1], flame_width(0.0, wmax)

    poly_outer, w_root = flame_polygon(WMAX, L, 0.0, BEND)
    poly_mid, _ = flame_polygon(WMAX * 0.60, L * 0.72, 26.0, BEND * 0.72)
    poly_core, _ = flame_polygon(WMAX * 0.30, L * 0.46, 52.0, BEND * 0.52)

    # ===== 4. 背景辉光（火焰照亮背景） =====
    glow_mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(glow_mask).polygon([ri(pt) for pt in poly_outer], fill=255)
    glow_mask = glow_mask.filter(ImageFilter.GaussianBlur(78))
    img.paste(Image.new("RGB", (S, S), (255, 96, 18)), (0, 0),
              glow_mask.point(lambda v: int(v * 0.32)))

    layer = ImageDraw.Draw(img)

    # ===== 5. 仪表弧 =====
    box = [cx - R, cy - R, cx + R, cy + R]
    TRACK = (76, 88, 118)
    a_start, a_end = 172.0, 368.0

    layer.arc(box, a_start, a_end, fill=TRACK, width=int(W))
    for a in (a_start, a_end):                       # 补圆头
        ex = cx + R * math.cos(math.radians(a))
        ey = cy + R * math.sin(math.radians(a))
        layer.ellipse(ri([ex - W / 2, ey - W / 2, ex + W / 2, ey + W / 2]), fill=TRACK)

    # 进度弧：172 -> 300，正好停在指针尖端外侧
    p_start, p_end = 172.0, 300.0
    c_from, c_to = (214, 58, 0), (255, 122, 24)
    seg = 96
    for i in range(seg):
        sa = p_start + (p_end - p_start) * i / seg
        ea = p_start + (p_end - p_start) * (i + 1) / seg + 0.8
        layer.arc(box, sa, ea, fill=mix(c_from, c_to, i / (seg - 1)), width=int(W))
    for a, c in ((p_start, c_from), (p_end, c_to)):
        ex = cx + R * math.cos(math.radians(a))
        ey = cy + R * math.sin(math.radians(a))
        layer.ellipse(ri([ex - W / 2, ey - W / 2, ex + W / 2, ey + W / 2]), fill=c)

    # ===== 6. 火焰（渐变填充：沿中心线方向分层上色） =====
    def paint_flame(polygon, w_root_this, root_col, tip_col, start_off):
        mask = Image.new("L", (S, S), 0)
        md = ImageDraw.Draw(mask)
        md.polygon([ri(pt) for pt in polygon], fill=255)
        # 底部圆头，让根部不那么"被切平"
        b0 = (cx + d[0] * start_off, cy + d[1] * start_off)
        md.ellipse(ri([b0[0] - w_root_this, b0[1] - w_root_this,
                       b0[0] + w_root_this, b0[1] + w_root_this]), fill=255)

        grad = Image.new("RGB", (S, S), root_col)
        gd = ImageDraw.Draw(grad)
        steps = 130
        span_from = start_off - 0.35 * L
        span_to = start_off + 1.15 * L
        lw = int((span_to - span_from) / steps) + 2
        for i in range(steps + 1):
            t = i / steps
            proj = span_from + (span_to - span_from) * t
            col = mix(root_col, tip_col, t)
            px = cx + d[0] * proj
            py = cy + d[1] * proj
            gd.line([ri([px - p[0] * 1000, py - p[1] * 1000]),
                     ri([px + p[0] * 1000, py + p[1] * 1000])], fill=col, width=lw)
        img.paste(grad, (0, 0), mask)

    paint_flame(poly_outer, w_root, (255, 64, 0), (255, 150, 30), 0.0)
    paint_flame(poly_mid, w_root * 0.60, (255, 142, 16), (255, 206, 78), 26.0)
    paint_flame(poly_core, w_root * 0.30, (255, 226, 130), (255, 250, 210), 52.0)

    # ===== 7. 输出 =====
    final = img.resize((256, 256), Image.LANCZOS)
    final.save(PNG_PATH)

    sizes = [(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (24, 24), (16, 16)]
    final.save(ICO_PATH, format="ICO", sizes=sizes)
    print("已生成:", PNG_PATH)
    print("已生成:", ICO_PATH)
    for s in sizes:
        print("  内嵌尺寸:", s[0], "x", s[1])


if __name__ == "__main__":
    build()
