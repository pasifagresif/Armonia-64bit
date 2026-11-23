#!/bin/bash
# Coklu UYAP katmanini (menuden acilan duzen, 23.11) imaj agacina koyar. iso_derle.sh (amd64) ve pi_derle.sh (arm64)
# COKLUUYAP=1 iken cagirir. Claude, 2026-11-23.
#   cokluuyap_hazirla.sh <includes.chroot / kok dizini> <x86_64|aarch64> <kanca hedef dizini>
# uyap_app derle_uyap.sh ciktisindan alinir: COKLUUYAP_DIZIN, yoksa <sudo cagiraninin evi>/armonia-cikti/cokluuyap.
# Cikti yoksa ya da mimarisi tutmuyorsa ve cagiranin cargo'su varsa derle_uyap.sh onun kimligiyle calistirilir.
set -euo pipefail
HEDEF=$1 MIMARI=$2 KANCA=$3
KAYNAK="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
EV=$( [ -n "${SUDO_USER:-}" ] && getent passwd "$SUDO_USER" | cut -d: -f6 || echo "$HOME" )
D="${COKLUUYAP_DIZIN:-$EV/armonia-cikti/cokluuyap}"
elf_mimari() {  # ELF e_machine (ofset 18): 3e00 = x86_64, b700 = aarch64
  case "$(od -An -tx1 -j18 -N2 "$1" 2>/dev/null | tr -d ' \n')" in 3e00) echo x86_64 ;; b700) echo aarch64 ;; *) echo bilinmiyor ;; esac
}
if [ ! -f "$D/uyap_app" ] || [ "$(elf_mimari "$D/uyap_app")" != "$MIMARI" ]; then
  if [ -n "${SUDO_USER:-}" ] && [ -z "${COKLUUYAP_DIZIN:-}" ] && [ -x "$EV/.cargo/bin/cargo" ]; then
    echo "Coklu UYAP derleniyor (derle_uyap.sh, kullanici $SUDO_USER)"
    sudo -u "$SUDO_USER" env HOME="$EV" bash "$KAYNAK/derle_uyap.sh"
  fi
fi
[ -f "$D/uyap_app" ] || { echo "HATA: $D/uyap_app yok. Once normal kullaniciyla: bash armonia-linux/derle_uyap.sh (COKLUUYAP=0 ile katmansiz derlenir)"; exit 7; }
m=$(elf_mimari "$D/uyap_app")
[ "$m" = "$MIMARI" ] || { echo "HATA: uyap_app mimarisi $m, beklenen $MIMARI (derle_uyap.sh'yi hedef mimaride calistirin)"; exit 7; }
[ -d "$D/webapp" ] && [ -d "$D/uyap_core/enjekte" ] || { echo "HATA: $D altinda webapp/ veya uyap_core/enjekte yok"; exit 7; }
rm -rf "$HEDEF/opt/armonia/cokluuyap"
install -d -m 0755 "$HEDEF/opt/armonia/cokluuyap/uyap_core"
install -m 0755 "$D/uyap_app" "$HEDEF/opt/armonia/cokluuyap/uyap_app"
cp -r "$D/webapp" "$HEDEF/opt/armonia/cokluuyap/webapp"
cp -r "$D/uyap_core/enjekte" "$HEDEF/opt/armonia/cokluuyap/uyap_core/enjekte"
[ -f "$D/SURUM" ] && install -m 0644 "$D/SURUM" "$HEDEF/opt/armonia/cokluuyap/SURUM"
cp -a "$KAYNAK/cokluuyap-katmani/menuden/includes.chroot/." "$HEDEF/"
install -d "$KANCA"
install -m 0755 "$KAYNAK/cokluuyap-katmani/menuden/hooks/0210-cokluuyap-menuden.hook.chroot" "$KANCA/"
# Windows'tan gelmis olabilecek CR satir sonlari (yalniz katmanin metin dosyalari)
find "$KAYNAK/cokluuyap-katmani/menuden/includes.chroot" -type f -printf '%P\n' | while IFS= read -r f; do
  sed -i 's/\r$//' "$HEDEF/$f"
done
sed -i 's/\r$//' "$KANCA/0210-cokluuyap-menuden.hook.chroot"
echo "Coklu UYAP katmani kopyalandi: $(sed -n 's/^surum=//p' "$D/SURUM" 2>/dev/null) ($m)"
