"""Armonia Yazı editörünün derlenmiş hâlini (arayuz/dist) ruyap'a alır: uyap_core/yazi/ (2026-11-23).

    python araclar/yazi_al.py [--kaynak C:/dev/Armonia-64bit/armonia-yazi/arayuz/dist]

Editör ayrı bir depoda geliştiriliyor (armonia-yazi); burada yalnız derlenmiş çıktısı tutulur ve program onu
/__cu__/yazi/ adresinden sunar (enjekte/yazi.js açılır penceresi). Kaynak depoya yazılmaz.
Önce kaynakta: cd armonia-yazi && ./wasm_derle.sh && cd arayuz && npm run build
"""
import argparse, datetime, os, pathlib, shutil, subprocess, sys

RUYAP = pathlib.Path(__file__).resolve().parent.parent
HEDEF = RUYAP / "uyap_core" / "yazi"
VARSAYILAN = os.environ.get("ARMONIA_YAZI_DIST", r"C:\dev\Armonia-64bit\armonia-yazi\arayuz\dist")
IZINLI = {".html", ".js", ".css", ".wasm", ".png", ".svg", ".woff2", ".ttf"}


def main():
    a = argparse.ArgumentParser()
    a.add_argument("--kaynak", default=VARSAYILAN)
    k = pathlib.Path(a.parse_args().kaynak)
    if not (k / "index.html").is_file():
        sys.exit(f"Derlenmiş editör bulunamadı: {k} (önce npm run build)")
    dosyalar = [p for p in k.rglob("*") if p.is_file()]
    yabanci = [p for p in dosyalar if p.suffix.lower() not in IZINLI]
    if yabanci:
        sys.exit("Beklenmeyen dosya türü: " + ", ".join(str(p.relative_to(k)) for p in yabanci))
    if HEDEF.exists():
        shutil.rmtree(HEDEF)
    shutil.copytree(k, HEDEF)
    try:
        kok = k.parents[2]
        commit = subprocess.run(["git", "-C", str(kok), "log", "-1", "--format=%h %ci"], capture_output=True, text=True).stdout.strip()
        kirli = subprocess.run(["git", "-C", str(kok), "status", "--porcelain", "--", "armonia-yazi"], capture_output=True, text=True).stdout.count("\n")
    except Exception:
        commit, kirli = "?", "?"
    (HEDEF / "KAYNAK.txt").write_text(
        f"Armonia Yazı derlenmiş arayüzü — buraya araclar/yazi_al.py ile alındı, elle düzenlenmez.\n"
        f"kaynak: {k}\nkaynak commit: {commit} (armonia-yazi'de commit'lenmemiş değişiklik: {kirli})\n"
        f"alınma: {datetime.datetime.now():%Y-%m-%d %H:%M}\n", encoding="utf-8")
    toplam = sum(p.stat().st_size for p in HEDEF.rglob("*") if p.is_file())
    print(f"Alındı: {HEDEF} ({len(dosyalar)} dosya, {toplam / 1024:.0f} KB)")


if __name__ == "__main__":
    main()
