#!/bin/bash
# Armonia Yazi (belge editoru) ve imza hizmeti katmanini imaj agacina koyar. iso_derle.sh (amd64) ve pi_derle.sh
# (arm64) YAZI=1 (varsayilan) iken cagirir. Claude, 2026-11-24.
#   yazi_hazirla.sh <includes.chroot / kok dizini> <x86_64|aarch64> <kanca hedef dizini>
# Ikililer armonia-yazi/derle.sh ciktisindan alinir: ARMONIA_YAZI_CIKTI, yoksa <sudo cagiraninin evi>/armonia-cikti/yazi.
# Cikti yoksa ya da mimarisi tutmuyorsa ve cagiranin cargo'su varsa derle.sh onun kimligiyle calistirilir.
set -euo pipefail
HEDEF=$1 MIMARI=$2 KANCA=$3
KAYNAK="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
EV=$( [ -n "${SUDO_USER:-}" ] && getent passwd "$SUDO_USER" | cut -d: -f6 || echo "$HOME" )
D="${ARMONIA_YAZI_CIKTI:-$EV/armonia-cikti/yazi}"
elf_mimari() {  # ELF e_machine (ofset 18): 3e00 = x86_64, b700 = aarch64
  case "$(od -An -tx1 -j18 -N2 "$1" 2>/dev/null | tr -d ' \n')" in 3e00) echo x86_64 ;; b700) echo aarch64 ;; *) echo bilinmiyor ;; esac
}
hazir() { [ -f "$D/armonia-yazi" ] && [ -f "$D/armonia-imza" ] && [ "$(elf_mimari "$D/armonia-yazi")" = "$MIMARI" ] \
  && [ "$(elf_mimari "$D/armonia-imza")" = "$MIMARI" ]; }
# Kaynak ikiliden yeniyse de yeniden derlenir (25.11: kaynak degistigi halde imaja eski ikili girmisti)
eskimis() { [ -n "$(find "$KAYNAK/../armonia-yazi" \( -name target -o -name node_modules -o -name dist -o -name pkg \) -prune -o -type f -newer "$D/armonia-yazi" -print -quit 2>/dev/null)" ]; }
if { ! hazir || eskimis; } && [ -n "${SUDO_USER:-}" ] && [ -z "${ARMONIA_YAZI_CIKTI:-}" ] && [ -x "$EV/.cargo/bin/cargo" ]; then
  echo "Armonia Yazi derleniyor (armonia-yazi/derle.sh, kullanici $SUDO_USER)"
  sudo -u "$SUDO_USER" env HOME="$EV" PATH="$EV/.cargo/bin:/usr/local/bin:/usr/bin:/bin" bash "$KAYNAK/../armonia-yazi/derle.sh"
fi
hazir || { echo "HATA: $D altinda $MIMARI icin armonia-yazi/armonia-imza yok. Once normal kullaniciyla: bash armonia-yazi/derle.sh (YAZI=0 ile katmansiz derlenir)"; exit 8; }
rm -rf "$HEDEF/opt/armonia/yazi"
install -d -m 0755 "$HEDEF/opt/armonia/yazi"
install -m 0755 "$D/armonia-yazi" "$D/armonia-imza" "$HEDEF/opt/armonia/yazi/"
[ -f "$D/SURUM" ] && install -m 0644 "$D/SURUM" "$HEDEF/opt/armonia/yazi/SURUM"
cp -a "$KAYNAK/yazi-katmani/includes.chroot/." "$HEDEF/"
install -d "$KANCA"
install -m 0755 "$KAYNAK/yazi-katmani/hooks/0220-armonia-yazi.hook.chroot" "$KANCA/"
# Windows'tan gelmis olabilecek CR satir sonlari (yalniz katmanin metin dosyalari)
find "$KAYNAK/yazi-katmani/includes.chroot" -type f -printf '%P\n' | while IFS= read -r f; do
  sed -i 's/\r$//' "$HEDEF/$f"
done
sed -i 's/\r$//' "$KANCA/0220-armonia-yazi.hook.chroot"
chmod 0755 "$HEDEF/opt/armonia/bin/yazi_ac.sh" "$HEDEF/opt/armonia/bin/yazi_calistir.sh"
echo "Armonia Yazi katmani kopyalandi: $(sed -n 's/^surum=//p' "$D/SURUM" 2>/dev/null) ($MIMARI)"
