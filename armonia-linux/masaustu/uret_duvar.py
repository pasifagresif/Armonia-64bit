"""Armonia duvar kağıdı: krem zemin, bütün ekranı kaplayan eş yükselti (isoline) çizgileri. Font kullanmaz.
Tasarım: kullanıcının ana ekran görseli (Faz 5, "eş yükselti çizgili arka plan"). Claude, 2026-11-19.
Kullanım: python3 -I uret_duvar.py <çıktı.png>
"""
import math
import random
import sys
from PIL import Image, ImageDraw, ImageFilter

G, Y = 1920, 1080
OLCEK = 2
UST = (245, 241, 234)   # krem
ALT = (240, 234, 224)
CIZGI = (196, 182, 160)  # açık toprak


def yukselti(x, y):
    # Sabit tohumlu çok sayıda yumuşak tepe/çukur: deterministik, gürültü kütüphanesi gerekmez
    h = 0.0
    for cx, cy, r, a in TEPELER:
        h += a * math.exp(-(((x - cx) ** 2) + ((y - cy) ** 2) * 0.8) / (r * r))
    return h + 0.05 * math.sin(9 * x + 3 * y) + 0.04 * math.cos(5 * y - 7 * x)


_r = random.Random(1914)
TEPELER = [(_r.uniform(-0.1, 1.1), _r.uniform(-0.1, 1.1), _r.uniform(0.06, 0.2), _r.choice((-1, 1)) * _r.uniform(0.4, 1.0))
           for _ in range(34)]


def main(hedef):
    w, h = G * OLCEK // 4, Y * OLCEK // 4  # alan örnekleme ızgarası
    im = Image.new("RGB", (G * OLCEK, Y * OLCEK))
    d = ImageDraw.Draw(im)
    for yy in range(Y * OLCEK):
        t = yy / (Y * OLCEK)
        d.line([(0, yy), (G * OLCEK, yy)], fill=tuple(int(UST[i] + (ALT[i] - UST[i]) * t) for i in range(3)))
    alan = [[yukselti(i / w, j / h) for i in range(w + 1)] for j in range(h + 1)]
    katman = Image.new("RGBA", im.size, (0, 0, 0, 0))
    kd = ImageDraw.Draw(katman)
    sx, sy = G * OLCEK / w, Y * OLCEK / h
    seviyeler = [0.11 * k for k in range(-14, 15)]
    # Kare yürüyüşü (marching squares): her hücrede seviye kesişimlerini birleştir
    for j in range(h):
        for i in range(w):
            a, b, c, e = alan[j][i], alan[j][i + 1], alan[j + 1][i + 1], alan[j + 1][i]
            for s in seviyeler:
                noktalar = []
                for (v1, v2, p1, p2) in ((a, b, (i, j), (i + 1, j)), (b, c, (i + 1, j), (i + 1, j + 1)),
                                         (c, e, (i + 1, j + 1), (i, j + 1)), (e, a, (i, j + 1), (i, j))):
                    if (v1 < s) != (v2 < s):
                        t = (s - v1) / (v2 - v1)
                        noktalar.append(((p1[0] + t * (p2[0] - p1[0])) * sx, (p1[1] + t * (p2[1] - p1[1])) * sy))
                if len(noktalar) >= 2:
                    kd.line(noktalar[:2], fill=CIZGI + (150,), width=2 * OLCEK // 2)
                if len(noktalar) == 4:
                    kd.line(noktalar[2:], fill=CIZGI + (150,), width=2 * OLCEK // 2)
    im = Image.alpha_composite(im.convert("RGBA"), katman.filter(ImageFilter.GaussianBlur(0.6)))
    im.convert("RGB").resize((G, Y), Image.LANCZOS).save(hedef, optimize=True)


if __name__ == "__main__":
    main(sys.argv[1])
