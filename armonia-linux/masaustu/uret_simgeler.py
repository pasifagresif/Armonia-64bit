"""Armonia masaüstü simgeleri: ince çizgili (line) simgeler, font kullanmaz. Tasarım: kullanıcının ana ekran
görseli (2026-11-19). Claude, 2026-11-19.  Kullanım: python3 -I uret_simgeler.py <çıktı_dizini>
"""
import math
import os
import sys
from PIL import Image, ImageDraw

K = 8            # büyük çiz, küçült (yumuşak kenar)
N = 64           # çıktı boyutu (CSS'te 30-32 px gösterilir; HiDPI için 2x)
KOYU = (47, 47, 47, 255)
YESIL = (95, 122, 97, 255)


def tuval():
    im = Image.new("RGBA", (N * K, N * K), (0, 0, 0, 0))
    return im, ImageDraw.Draw(im)


def p(x, y):
    return (x * K * N / 24, y * K * N / 24)  # 24'lük ızgara koordinatı


def cizgi(d, noktalar, w=1.6, renk=KOYU):
    d.line([p(*a) for a in noktalar], fill=renk, width=int(w * K * N / 24), joint="curve")
    for a in (noktalar[0], noktalar[-1]):
        r = w * K * N / 48
        x, y = p(*a)
        d.ellipse((x - r, y - r, x + r, y + r), fill=renk)


def kutu(d, x0, y0, x1, y1, r=2, w=1.6, renk=KOYU):
    d.rounded_rectangle((*p(x0, y0), *p(x1, y1)), radius=r * K * N / 24, outline=renk, width=int(w * K * N / 24))


def kaydet(im, hedef, ad):
    im.resize((N, N), Image.LANCZOS).save(os.path.join(hedef, ad + ".png"))


def logo(hedef):
    # Geometrik "A": iki bacak + ara çizgi, yeşil, zeminsiz
    im, d = tuval()
    w = 3.2
    cizgi(d, [(4.5, 21), (12, 3), (19.5, 21)], w, YESIL)
    cizgi(d, [(8.2, 14.5), (15.8, 14.5)], w * 0.8, YESIL)
    kaydet(im, hedef, "logo")


def uyap(hedef):
    # Belge + konuşma balonu (UYAP & e-Duruşma)
    im, d = tuval()
    kutu(d, 3, 2.5, 15, 19)
    for y in (7, 10.5, 14):
        cizgi(d, [(6, y), (12, y)], 1.4)
    d.rounded_rectangle((*p(12, 12), *p(21.5, 20)), radius=2 * K * N / 24, fill=(0, 0, 0, 0))
    kutu(d, 12, 12, 21.5, 20, r=2.5)
    cizgi(d, [(14.5, 20), (13.5, 22.5), (17, 20)], 1.4)
    cizgi(d, [(14.5, 16), (19, 16)], 1.4)
    kaydet(im, hedef, "uyap")


def yazi(hedef):
    # Kare + kalem (yazı / editör)
    im, d = tuval()
    cizgi(d, [(11, 4), (5, 4), (4, 5), (4, 20), (5, 21), (19, 21), (20, 20), (20, 13)])
    cizgi(d, [(17.5, 3.5), (20.5, 6.5), (11.5, 15.5), (8, 16.5), (9, 13), (17.5, 3.5)])
    kaydet(im, hedef, "yazi")


def tablo(hedef):
    im, d = tuval()
    kutu(d, 3, 4, 21, 20)
    cizgi(d, [(3, 9), (21, 9)], 1.4)
    cizgi(d, [(3, 14.5), (21, 14.5)], 1.4)
    cizgi(d, [(9, 4), (9, 20)], 1.4)
    cizgi(d, [(15, 9), (15, 20)], 1.4)
    kaydet(im, hedef, "tablo")


def belgeler(hedef):
    im, d = tuval()
    cizgi(d, [(3, 7), (3, 19), (4, 20), (20, 20), (21, 19), (21, 9), (20, 8), (11.5, 8), (9.5, 5), (4, 5), (3, 6), (3, 7)])
    cizgi(d, [(3, 10.5), (21, 10.5)], 1.4)
    kaydet(im, hedef, "belgeler")


def edevlet(hedef):
    # Küre (resmî siteler)
    im, d = tuval()
    d.ellipse((*p(3, 3), *p(21, 21)), outline=KOYU, width=int(1.6 * K * N / 24))
    d.ellipse((*p(8, 3), *p(16, 21)), outline=KOYU, width=int(1.4 * K * N / 24))
    cizgi(d, [(3.5, 12), (20.5, 12)], 1.4)
    cizgi(d, [(5, 7.5), (19, 7.5)], 1.2)
    cizgi(d, [(5, 16.5), (19, 16.5)], 1.2)
    kaydet(im, hedef, "edevlet")


def wifi(hedef):
    im, d = tuval()
    for r in (9, 6, 3):
        d.arc((*p(12 - r, 18 - r), *p(12 + r, 18 + r)), 225, 315, fill=KOYU, width=int(2 * K * N / 24))
    d.ellipse((*p(10.8, 17), *p(13.2, 19.4)), fill=KOYU)
    kaydet(im, hedef, "wifi")


def kablo(hedef):
    im, d = tuval()
    kutu(d, 8, 4, 16, 11, r=1)
    cizgi(d, [(12, 11), (12, 15), (5, 15), (5, 19)], 1.6)
    cizgi(d, [(12, 15), (19, 15), (19, 19)], 1.6)
    cizgi(d, [(12, 15), (12, 19)], 1.6)
    kaydet(im, hedef, "kablo")


def pil(hedef):
    im, d = tuval()
    kutu(d, 2.5, 7, 19.5, 17, r=2)
    d.rectangle((*p(20, 10), *p(21.8, 14)), fill=KOYU)
    d.polygon([p(12, 8.5), p(8, 12.5), p(11, 12.5), p(10, 15.5), p(14, 11.5), p(11, 11.5)], fill=KOYU)
    kaydet(im, hedef, "pil")


def ses(hedef):
    im, d = tuval()
    d.polygon([p(3, 9), p(7, 9), p(12, 4.5), p(12, 19.5), p(7, 15), p(3, 15)], outline=KOYU, width=int(1.6 * K * N / 24))
    d.arc((*p(10, 8), *p(18, 16)), 300, 60, fill=KOYU, width=int(1.6 * K * N / 24))
    d.arc((*p(10, 4.5), *p(21.5, 19.5)), 300, 60, fill=KOYU, width=int(1.6 * K * N / 24))
    kaydet(im, hedef, "ses")


def guc(hedef):
    im, d = tuval()
    d.arc((*p(4, 5), *p(20, 21)), 300, 240, fill=KOYU, width=int(2 * K * N / 24))
    cizgi(d, [(12, 3), (12, 12)], 2)
    kaydet(im, hedef, "guc")


if __name__ == "__main__":
    hedef = sys.argv[1]
    os.makedirs(hedef, exist_ok=True)
    for f in (logo, uyap, yazi, tablo, belgeler, edevlet, wifi, kablo, pil, ses, guc):
        f(hedef)
