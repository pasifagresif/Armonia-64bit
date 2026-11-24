"""Güncelleme yayını hazırlar (2026-11-23). Windows'ta çalışır (MSI için WiX gerekir).

    python araclar/yayin_hazirla.py --notlar "Bu sürümde: …" [--zorunlu] [--surum 26.11.24.1]
                                    [--anahtar C:/Users/…/CokluUyap-Gizli/guncelleme-asil.json] [--pgsql <PostgreSQL kurulumu>]

Adımlar:
  1. Sürüm: verilmezse yıl.ay.gün.N (N: vendor_deploy'daki son sürüm bugününse +1, değilse 1).
  2. uyap_app bu sürümle derlenir (CU_SURUM).
  3. Paket klasörü dist/paket/CokluUyap: CokluUyap.exe + uyap_core/enjekte + webapp + pgsql (gömülü PostgreSQL;
     ilk seferde tam kurulumdan ayıklanıp dist/pgsql-hazir'da saklanır).
  4. MSI: dist/CokluUyap-<sürüm>.msi (msi_hazirla).
  5. MSI vendor_deploy/downloads/guncelleme/ altına kopyalanır (eski CokluUyap-*.msi silinir).
  6. Sürüm dosyası imzalanır (anahtar parolası sorulur): vendor_deploy/downloads/guncelleme/test.json.
Push / yayın YAPILMAZ; sonunda yapılacak git komutları yazılır.
"""
import argparse, datetime, glob, json, os, pathlib, shutil, subprocess, sys

RUYAP = pathlib.Path(__file__).resolve().parent.parent
VENDOR = RUYAP.parent / "vendor_deploy"
GNC = VENDOR / "downloads" / "guncelleme"
PAKET_KOK = "https://www.cokluuyap.com/guncelleme/paket/"


def calistir(komut, **kw):
    print(">", " ".join(str(k) for k in komut), flush=True)
    r = subprocess.run([str(k) for k in komut], **kw)
    if r.returncode != 0:
        sys.exit(f"HATA: komut başarısız ({r.returncode})")


def parcalar(s):
    try:
        p = [int(x) for x in s.split(".")]
    except ValueError:
        return None
    return p + [0] * (4 - len(p)) if 3 <= len(p) <= 4 else None


def son_surum():
    try:
        return json.loads((GNC / "test.json").read_text("utf-8")).get("surum")
    except (OSError, ValueError):
        return None


def yeni_surum():
    b = datetime.date.today()
    gun = f"{b.year % 100}.{b.month}.{b.day}"
    son = son_surum()
    n = 1
    if son and son.rsplit(".", 1)[0] == gun and len(son.split(".")) == 4:
        n = int(son.rsplit(".", 1)[1]) + 1
    return f"{gun}.{n}"


def hedef_dizini():
    return pathlib.Path(os.environ.get("CARGO_TARGET_DIR") or (RUYAP / "target"))


def main():
    a = argparse.ArgumentParser(description="Çoklu UYAP güncelleme yayını hazırla")
    a.add_argument("--surum")
    a.add_argument("--notlar", required=True)
    a.add_argument("--zorunlu", action="store_true")
    a.add_argument("--anahtar", default=str(pathlib.Path.home() / "CokluUyap-Gizli" / "guncelleme-asil.json"))
    a.add_argument("--pgsql", default=os.path.join(os.environ.get("LOCALAPPDATA", ""), "UyapIcra", "pgsql"))
    a.add_argument("--imzasiz", action="store_true", help="MSI hazırla, sürüm dosyasını imzalama")
    g = a.parse_args()
    if os.name != "nt":
        sys.exit("MSI üretimi Windows'ta yapılır (WiX).")

    surum = g.surum or yeni_surum()
    if not parcalar(surum):
        sys.exit(f"Sürüm geçersiz: {surum}")
    son = son_surum()
    if son and parcalar(son) and parcalar(surum) <= parcalar(son):
        sys.exit(f"Sürüm {surum}, yayındaki {son}'dan yeni değil.")
    print(f"=== Sürüm {surum} (yayındaki: {son or 'yok'}) ===")
    ortam = dict(os.environ, CU_SURUM=surum)

    # 2) Derleme
    calistir(["cargo", "build", "--release", "--bin", "uyap_app", "--bin", "msi_hazirla", "--bin", "guncelleme_arac"],
             cwd=RUYAP, env=ortam)
    rel = hedef_dizini() / "release"
    exe, msi_arac, gnc_arac = rel / "uyap_app.exe", rel / "msi_hazirla.exe", rel / "guncelleme_arac.exe"

    # 3) Paket klasörü
    dist = RUYAP / "dist"
    paket = dist / "paket" / "CokluUyap"
    if paket.exists():
        shutil.rmtree(paket)
    (paket / "uyap_core").mkdir(parents=True)
    shutil.copy2(exe, paket / "CokluUyap.exe")
    shutil.copytree(RUYAP / "uyap_core" / "enjekte", paket / "uyap_core" / "enjekte",
                    ignore=shutil.ignore_patterns("__pycache__", "*.bak"))
    shutil.copytree(RUYAP / "webapp", paket / "webapp", ignore=shutil.ignore_patterns("__pycache__"))
    # Armonia Yazı editörü (derlenmiş; araclar/yazi_al.py ile alınır)
    if (RUYAP / "uyap_core" / "yazi" / "index.html").is_file():
        shutil.copytree(RUYAP / "uyap_core" / "yazi", paket / "uyap_core" / "yazi")
    else:
        print("UYARI: uyap_core/yazi yok; Armonia Yazı pakete girmiyor (araclar/yazi_al.py).")
    pg_hazir = dist / "pgsql-hazir"
    if not (pg_hazir / "bin").is_dir():
        if not os.path.isdir(os.path.join(g.pgsql, "bin")):
            sys.exit(f"PostgreSQL kurulumu bulunamadı: {g.pgsql} (--pgsql ile verin)")
        calistir([exe, "--pgsql-hazirla", g.pgsql, pg_hazir])
    shutil.copytree(pg_hazir, paket / "pgsql")

    # 4) MSI
    msi = dist / f"CokluUyap-{surum}.msi"
    calistir([msi_arac], cwd=RUYAP, env=dict(ortam, CU_MSI_KAYNAK=str(paket), CU_MSI_CIKTI=str(msi)))
    boyut = msi.stat().st_size
    print(f"MSI: {msi} ({boyut / 1e6:.1f} MB)")
    if boyut > 95 * 1024 * 1024:
        print("UYARI: MSI 95 MB'ı aşıyor; GitHub depo sınırı (100 MB) yüzünden vendor_deploy'a konamaz. "
              "Paketi GitHub Release'e koyup Render'da UYAP_GUNCELLEME_PAKET_KOK ayarlayın.")

    # 5) vendor_deploy'a kopyala
    GNC.mkdir(parents=True, exist_ok=True)
    for eski in glob.glob(str(GNC / "CokluUyap-*.msi")):
        os.remove(eski)
    shutil.copy2(msi, GNC / msi.name)

    # 6) İmzala
    if g.imzasiz:
        print("İmzalama atlandı (--imzasiz).")
        return
    calistir([gnc_arac, "imzala", "--anahtar", g.anahtar, "--surum", surum, "--notlar", g.notlar]
             + (["--zorunlu"] if g.zorunlu else [])
             + ["--paket", f"windows-x64={PAKET_KOK}{msi.name}={msi}", "--cikti", GNC / "test.json"])
    calistir([gnc_arac, "dogrula", GNC / "test.json"])
    print(f"""
=== Hazır: {surum} ===
Yayın için (onayınızla) vendor_deploy klasöründe:
  git add downloads/guncelleme
  git commit -m "Güncelleme {surum}"
  git push
Render ~1 dk içinde yayınlar; programlar 6 saat içinde (ya da sayfa açılınca) yeni sürümü görür.""")


if __name__ == "__main__":
    main()
