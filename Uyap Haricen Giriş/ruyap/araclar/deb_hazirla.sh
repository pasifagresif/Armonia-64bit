#!/bin/bash
# Linux 64 bit (amd64) .deb paketi (2026-11-23). Önce armonia-linux/derle_uyap.sh çalışır (CU_SURUM ile):
#   CU_SURUM=26.11.24.1 bash armonia-linux/derle_uyap.sh && CU_SURUM=26.11.24.1 bash "Uyap Haricen Giriş/ruyap/araclar/deb_hazirla.sh"
# Girdi: $ARMONIA_CIKTI/cokluuyap (uyap_app + uyap_core/enjekte + webapp). Çıktı: $ARMONIA_CIKTI/cokluuyap_<sürüm>_amd64.deb
# Kurulum: sudo apt install ./cokluuyap_<sürüm>_amd64.deb   (PostgreSQL'i de kurar; veritabanı kullanıcı klasöründe)
# Program /opt/cokluuyap'ta; kullanıcı verisi ~/.local/share/cokluuyap ve ~/CokluUyap (yazılabilir yerler).
set -euo pipefail

BETIK_DIZINI="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
RUYAP="$(cd "$BETIK_DIZINI/.." && pwd)"
CIKTI_KOK="${ARMONIA_CIKTI:-$HOME/armonia-cikti}"
GIRDI="$CIKTI_KOK/cokluuyap"
SURUM="${CU_SURUM:?CU_SURUM verin (ör. 26.11.24.1)}"
[ "$(uname -m)" = "x86_64" ] || { echo "HATA: amd64 paketi x86_64 makinede hazırlanır"; exit 1; }
[ -x "$GIRDI/uyap_app" ] || { echo "HATA: önce derle_uyap.sh ($GIRDI/uyap_app yok)"; exit 1; }

PAKET="$CIKTI_KOK/deb-kok"
rm -rf "$PAKET"
mkdir -p "$PAKET/DEBIAN" "$PAKET/opt/cokluuyap" "$PAKET/usr/bin" "$PAKET/usr/share/applications" \
         "$PAKET/usr/share/icons/hicolor/256x256/apps"
cp -a "$GIRDI/uyap_app" "$GIRDI/uyap_core" "$GIRDI/webapp" "$PAKET/opt/cokluuyap/"
chmod 0755 "$PAKET/opt/cokluuyap/uyap_app"
install -m 0644 "$RUYAP/assets/armonia-simge-256.png" "$PAKET/usr/share/icons/hicolor/256x256/apps/cokluuyap.png"

cat >"$PAKET/usr/bin/cokluuyap" <<'EOT'
#!/bin/sh
# Çoklu UYAP başlatıcı: kullanıcı verisi yazılabilir bir klasörde tutulur (/opt salt okunur).
export UYAP_DATA_DIR="${UYAP_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/cokluuyap}"
mkdir -p "$UYAP_DATA_DIR"
cd "$UYAP_DATA_DIR" || exit 1
exec /opt/cokluuyap/uyap_app "$@"
EOT
chmod 0755 "$PAKET/usr/bin/cokluuyap"

cat >"$PAKET/usr/share/applications/cokluuyap.desktop" <<'EOT'
[Desktop Entry]
Type=Application
Name=Çoklu UYAP
Comment=Büro paneli ve UYAP paylaşımı
Exec=cokluuyap
Icon=cokluuyap
Terminal=false
Categories=Office;
EOT

# Gereken en düşük glibc, ikilinin sembollerinden (Debian 13'te derlenince 2.39: Ubuntu 24.04+ / Debian 13+).
GLIBC=$(objdump -T "$PAKET/opt/cokluuyap/uyap_app" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)
BOYUT_KB=$(du -sk "$PAKET/opt" "$PAKET/usr" | awk '{s+=$1} END {print s}')
cat >"$PAKET/DEBIAN/control" <<EOT
Package: cokluuyap
Version: $SURUM
Architecture: amd64
Maintainer: Çoklu UYAP
Installed-Size: $BOYUT_KB
Depends: libc6 (>= $GLIBC), postgresql, libgl1, libxkbcommon0
Recommends: libxkbcommon-x11-0, libwayland-client0, libx11-6, libxcursor1, libxrandr2, libxi6, pcscd
Section: misc
Priority: optional
Homepage: https://www.cokluuyap.com
Description: Çoklu UYAP ofis programı (test sürümü)
 UYAP'ı büro içinde paylaştıran ofis programı ve büro paneli.
 Veritabanı kullanıcının klasöründe, gömülü PostgreSQL ile çalışır.
EOT

DEB="$CIKTI_KOK/cokluuyap_${SURUM}_amd64.deb"
dpkg-deb --root-owner-group -Zxz --build "$PAKET" "$DEB" >/dev/null
dpkg-deb --info "$DEB" | sed -n '1,12p'
ls -la "$DEB"
sha256sum "$DEB"
