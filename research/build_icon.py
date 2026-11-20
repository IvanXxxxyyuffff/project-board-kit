# -*- coding: utf-8 -*-
"""生成「项目看板」应用图标 v2：深色圆角底 + 三条甘特进度条（P0/P1/P2 语义色）。
输出 <ROOT>/research/项目看板.ico（多尺寸）与 项目看板_256.png。
"""
import os
from PIL import Image, ImageDraw

S = 1024
SS = 4
W = S * SS

BG_TOP = (30, 35, 51)
BG_BOT = (14, 17, 25)
P0 = (255, 107, 97)
P1 = (255, 194, 71)
P2 = (90, 169, 255)

img = Image.new("RGBA", (W, W), (0, 0, 0, 0))

# 背景渐变 + 圆角
grad = Image.new("RGB", (1, W))
for y in range(W):
    t = y / (W - 1)
    grad.putpixel((0, y), tuple(int(BG_TOP[i] + (BG_BOT[i] - BG_TOP[i]) * t) for i in range(3)))
grad = grad.resize((W, W)).convert("RGBA")
mask = Image.new("L", (W, W), 0)
ImageDraw.Draw(mask).rounded_rectangle([0, 0, W - 1, W - 1], radius=int(W * 0.225), fill=255)
img.paste(grad, (0, 0), mask)

# 轨道层（半透明白，单独合成，避免 alpha 叠加出错）
track = Image.new("RGBA", (W, W), (0, 0, 0, 0))
td = ImageDraw.Draw(track)

L, R = 0.175, 0.825          # 轨道左右边界
H = 0.105                     # 条高
rows = [
    (0.315, 0.50, P0),        # (中心y, 进度比例, 颜色)
    (0.500, 0.78, P1),
    (0.685, 0.38, P2),
]
rad = int(H * W / 2)
for cy, p, col in rows:
    y0, y1 = (cy - H / 2) * W, (cy + H / 2) * W
    td.rounded_rectangle([L * W, y0, R * W, y1], radius=rad, fill=(255, 255, 255, 30))
img.alpha_composite(track)

d = ImageDraw.Draw(img)
for cy, p, col in rows:
    y0, y1 = (cy - H / 2) * W, (cy + H / 2) * W
    x1 = L * W + (R - L) * W * p
    d.rounded_rectangle([L * W, y0, x1, y1], radius=rad, fill=col + (255,))

# 今日竖线：贯穿三行，细、半透明白，落在进度终点右侧位置
x = 0.735 * W
line = Image.new("RGBA", (W, W), (0, 0, 0, 0))
ImageDraw.Draw(line).rounded_rectangle(
    [x - W * 0.0075, 0.235 * W, x + W * 0.0075, 0.765 * W],
    radius=int(W * 0.0075), fill=(255, 255, 255, 90))
img.alpha_composite(line)

out256 = img.resize((S, S), Image.LANCZOS)
base = r"<ROOT>/research"
out256.save(os.path.join(base, "项目看板_256.png"))
sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
out256.save(os.path.join(base, "项目看板.ico"), format="ICO", sizes=sizes)
print("ICON OK", os.path.getsize(os.path.join(base, "项目看板.ico")), "bytes")
