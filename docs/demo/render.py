"""Render events.json (from record.py) to a GIF. Usage: render.py events.json out.gif frames_dir"""
import json, re, subprocess, sys, os
from PIL import Image, ImageDraw, ImageFont
ev = json.load(open(sys.argv[1])); ev = [(t - ev[0][0], x) for t, x in ev]; out = sys.argv[2]; frames_dir = sys.argv[3]
os.makedirs(frames_dir, exist_ok=True)
FP = "/usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-%s.ttf"
if not os.path.exists(FP % "Regular"):
    FP = "/usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-%s.otf"
reg = ImageFont.truetype(FP % "Regular", 15); bold = ImageFont.truetype(FP % "Bold", 15)
CW = int(reg.getlength("M") + 0.5); LH = 21; COLS = 92; ROWS = 25
W = COLS * CW + 32; H = ROWS * LH + 24 + 36 + 30
BG = (24, 26, 33); FG = (214, 218, 228)
COL = {"34": (97, 160, 255), "92": (110, 220, 130), "32": (110, 220, 130), "91": (255, 120, 120), "93": (240, 200, 100)}
CMD = re.compile(r"\x1b\]cmd;(.*?)\x07")
HDR = 30   # pinned command line under the title bar
SGR = re.compile(r"\x1b\[([0-9;]*)m")

class Term:
    def __init__(s): s.lines = [[]]; s.st = (FG, False, False); s.cmd = ""
    def feed(s, text):
        text = text.replace("/home/susitsm/.cargo-remap-demo", "~/work")
        i = 0
        while i < len(text):
            m = CMD.match(text, i)
            if m:
                s.cmd = m.group(1); i = m.end(); continue
            m = SGR.match(text, i)
            if m:
                for code in (m.group(1) or "0").split(";"):
                    fg, b, d = s.st
                    if code == "0": s.st = (FG, False, False)
                    elif code == "1": s.st = (fg, True, d)
                    elif code == "2": s.st = ((120, 128, 150), b, True)
                    elif code in COL: s.st = (COL[code], b, d)
                i = m.end(); continue
            ch = text[i]; i += 1
            if ch == "\r": continue
            if ch == "\n": s.lines.append([]); continue
            if len(s.lines[-1]) >= COLS: s.lines.append([])
            s.lines[-1].append((ch, s.st))
    def draw(s, cursor):
        im = Image.new("RGB", (W, H), BG); d = ImageDraw.Draw(im)
        d.rectangle([0, 0, W, 36], fill=(38, 41, 52))
        for k, c in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]): d.ellipse([14 + k * 22, 12, 26 + k * 22, 24], fill=c)
        d.text((W // 2, 18), "cargo-remap: new worktree, warm build", font=reg, fill=(150, 158, 178), anchor="mm")
        d.rectangle([0, 36, W, 36 + HDR], fill=(32, 35, 46))
        d.line([0, 36 + HDR, W, 36 + HDR], fill=(60, 64, 80))
        if s.cmd:
            d.text((16, 36 + HDR // 2), s.cmd, font=bold, fill=(240, 200, 100), anchor="lm")
        vis = s.lines[-ROWS:]
        for r, line in enumerate(vis):
            for c, (ch, (fg, b, _)) in enumerate(line):
                d.text((16 + c * CW, 48 + HDR + r * LH), ch, font=bold if b else reg, fill=fg)
        if cursor:
            r = len(vis) - 1; c = len(vis[-1]); d.rectangle([16 + c * CW, 50 + HDR + r * LH, 16 + (c + 1) * CW - 2, 48 + HDR + (r + 1) * LH - 2], fill=(200, 205, 220))
        return im
FPS = 10; term = Term(); end = ev[-1][0] + 3.5; n = int(end * FPS) + 1; j = 0
for f in range(n):
    t = f / FPS
    while j < len(ev) and ev[j][0] <= t: term.feed(ev[j][1]); j += 1
    term.draw(cursor=(f // 5) % 2 == 0).save(f"{frames_dir}/f{f:05d}.png")
subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-framerate", str(FPS), "-i", f"{frames_dir}/f%05d.png",
  "-vf", "split[a][b];[a]palettegen=max_colors=64[p];[b][p]paletteuse=dither=none", "-loop", "0", out], check=True)
print(n, "frames", os.path.getsize(out) // 1024, "KiB")
