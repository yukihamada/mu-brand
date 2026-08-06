#!/usr/bin/env python3
"""声のかたち — 実音声「ありがとう。」の波形からアート3種を生成。
tee: 透過背景+白波形 (黒Tee DTG用) / poster: 黒地+金波形 / coin: 円形放射波形
"""
import subprocess, io, math, os
import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageFilter

OUT = os.path.dirname(os.path.abspath(__file__))
AUDIO_URL = "https://koe.live/api/post-audio/2c7e70e0856a35d3e48a3b3d969dd6bd6b8aff0543ff29406c00e4d5763caaff"
FONT = "/System/Library/Fonts/Hiragino Sans GB.ttc"

# ── 1. 実波形エンベロープ ────────────────────────────────
mp3 = os.path.join(OUT, "arigato.mp3")
subprocess.run(["curl", "-s", "-o", mp3, AUDIO_URL], check=True)
raw = subprocess.run(
    ["ffmpeg", "-v", "quiet", "-i", mp3, "-f", "s16le", "-ac", "1", "-ar", "16000", "-"],
    capture_output=True, check=True).stdout
sig = np.frombuffer(raw, dtype=np.int16).astype(np.float64)
# 無音の頭尻を落とす
nz = np.where(np.abs(sig) > 300)[0]
sig = sig[nz[0]:nz[-1]] if len(nz) else sig
def envelope(n_bars):
    chunks = np.array_split(np.abs(sig), n_bars)
    env = np.array([c.max() for c in chunks])
    env = env / env.max()
    return np.clip(env, 0.04, 1.0)  # 無音部も髪の毛1本残す

def rounded_bar(d, x0, y0, x1, y1, fill):
    r = (x1 - x0) / 2
    d.rounded_rectangle([x0, y0, x1, y1], radius=r, fill=fill)

# ── 2. Tee (透過・白) 3000x3000 ──────────────────────────
W = H = 3000
img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
n = 64
env = envelope(n)
bw = 22           # bar width
gap = (W * 0.86 - n * bw) / (n - 1)
x = W * 0.07
cy = H * 0.44
maxh = H * 0.30
for i, e in enumerate(env):
    h = maxh * e
    rounded_bar(d, x, cy - h / 2, x + bw, cy + h / 2, (255, 255, 255, 255))
    x += bw + gap
f_big = ImageFont.truetype(FONT, 150)
f_sm = ImageFont.truetype(FONT, 62)
t = "ありがとう"
tw = d.textlength(t, font=f_big)
d.text(((W - tw) / 2, H * 0.66), t, font=f_big, fill=(255, 255, 255, 255))
t2 = "こ え の か た ち  ·  2 0 2 6"
tw2 = d.textlength(t2, font=f_sm)
d.text(((W - tw2) / 2, H * 0.66 + 230), t2, font=f_sm, fill=(255, 255, 255, 170))
img.save(os.path.join(OUT, "knk_tee.png"))

# ── 3. Poster (黒地・金) 2700x3600 (18x24) ───────────────
W, H = 2700, 3600
img = Image.new("RGB", (W, H), (10, 10, 10))
d = ImageDraw.Draw(img)
GOLD = (230, 196, 73)
n = 72
env = envelope(n)
bw = 16
gap = (W * 0.78 - n * bw) / (n - 1)
x = W * 0.11
cy = H * 0.42
maxh = H * 0.26
# 淡いグロー層
glow = Image.new("RGB", (W, H), (10, 10, 10))
dg = ImageDraw.Draw(glow)
xg = x
for i, e in enumerate(env):
    h = maxh * e
    rounded_bar(dg, xg - 4, cy - h / 2 - 4, xg + bw + 4, cy + h / 2 + 4, (120, 100, 35))
    xg += bw + gap
glow = glow.filter(ImageFilter.GaussianBlur(26))
img = Image.blend(img, glow, 0.55)
d = ImageDraw.Draw(img)
for i, e in enumerate(env):
    h = maxh * e
    rounded_bar(d, x, cy - h / 2, x + bw, cy + h / 2, GOLD)
    x += bw + gap
f_eyebrow = ImageFont.truetype(FONT, 52)
f_big = ImageFont.truetype(FONT, 210)
f_sm = ImageFont.truetype(FONT, 56)
t0 = "K O E   N O   K A T A C H I"
d.text(((W - d.textlength(t0, font=f_eyebrow)) / 2, H * 0.115), t0, font=f_eyebrow, fill=GOLD)
t = "ありがとう"
d.text(((W - d.textlength(t, font=f_big)) / 2, H * 0.62), t, font=f_big, fill=(245, 245, 240))
t2 = "声は、いちばんあたたかい形見。"
d.text(((W - d.textlength(t2, font=f_sm)) / 2, H * 0.62 + 330), t2, font=f_sm, fill=(245, 245, 240, ))
t3 = "実声より · 2026.08.06 · wearmu.com/koe-no-katachi"
f_t3 = ImageFont.truetype(FONT, 40)
d.text(((W - d.textlength(t3, font=f_t3)) / 2, H * 0.90), t3, font=f_t3, fill=(140, 140, 135))
img.save(os.path.join(OUT, "knk_poster.png"))

# ── 4. Coin (円形放射) 2000x2000 ─────────────────────────
S = 2000
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
cx = cyc = S / 2
d.ellipse([60, 60, S - 60, S - 60], fill=(12, 12, 14, 255), outline=GOLD, width=14)
n = 120
env = envelope(n)
r0 = S * 0.26
for i, e in enumerate(env):
    a = 2 * math.pi * i / n - math.pi / 2
    r1 = r0 + (S * 0.155) * e
    x0, y0 = cx + r0 * math.cos(a), cyc + r0 * math.sin(a)
    x1, y1 = cx + r1 * math.cos(a), cyc + r1 * math.sin(a)
    d.line([x0, y0, x1, y1], fill=GOLD, width=13)
f_c = ImageFont.truetype(FONT, 108)
t = "こえ"
d.text((cx - d.textlength(t, font=f_c) / 2, cyc - 70), t, font=f_c, fill=(245, 245, 240))
img.save(os.path.join(OUT, "knk_coin.png"))
print("done:", [f for f in os.listdir(OUT) if f.startswith("knk_")])
print("audio_sec:", len(sig) / 16000)
