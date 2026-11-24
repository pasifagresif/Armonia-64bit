#!/bin/bash
# Armonia güncelleme katmanını imaj ağacına koyar. Şimdilik yalnız pi_derle.sh çağırır (canlı ISO'da güncelleme
# kapalı, kullanıcı kararı 24.11). Claude, 2026-11-24.
#   guncelle_hazirla.sh <kök dizin> <x86_64|aarch64> <kanca hedef dizini>
# İkili armonia-guncelle/derle.sh çıktısından: ARMONIA_GUNCELLE_CIKTI, yoksa <sudo çağıranın evi>/armonia-cikti/guncelle.
# Açık anahtarlar: guncelle-katmani/anahtarlar (yayıncının 'etkin' ve 'yedek' açık anahtarı; gizlileri depoda YOK).
set -euo pipefail
HEDEF=$1 MIMARI=$2 KANCA=$3
KAYNAK="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
EV=$( [ -n "${SUDO_USER:-}" ] && getent passwd "$SUDO_USER" | cut -d: -f6 || echo "$HOME" )
D="${ARMONIA_GUNCELLE_CIKTI:-$EV/armonia-cikti/guncelle}"
ANAHTAR="$KAYNAK/guncelle-katmani/anahtarlar"
[ -s "$ANAHTAR" ] || { echo "HATA: $ANAHTAR yok (önce yayıncı anahtarları üretilmeli; GUNCELLE=0 ile katmansız)"; exit 9; }
elf_mimari() {  # ELF e_machine (ofset 18): 3e00 = x86_64, b700 = aarch64
  case "$(od -An -tx1 -j18 -N2 "$1" 2>/dev/null | tr -d ' \n')" in 3e00) echo x86_64 ;; b700) echo aarch64 ;; *) echo bilinmiyor ;; esac
}
hazir() { [ -f "$D/armonia-guncelle" ] && [ "$(elf_mimari "$D/armonia-guncelle")" = "$MIMARI" ]; }
if ! hazir && [ -n "${SUDO_USER:-}" ] && [ -z "${ARMONIA_GUNCELLE_CIKTI:-}" ] && [ -x "$EV/.cargo/bin/cargo" ]; then
  echo "armonia-guncelle derleniyor (kullanıcı $SUDO_USER)"
  sudo -u "$SUDO_USER" env HOME="$EV" PATH="$EV/.cargo/bin:/usr/local/bin:/usr/bin:/bin" bash "$KAYNAK/../armonia-guncelle/derle.sh"
fi
hazir || { echo "HATA: $D altında $MIMARI için armonia-guncelle yok. Önce: bash armonia-guncelle/derle.sh"; exit 9; }
rm -rf "$HEDEF/opt/armonia/guncelle"
install -d -m 0755 "$HEDEF/opt/armonia/guncelle"
install -m 0755 "$D/armonia-guncelle" "$HEDEF/opt/armonia/guncelle/"
[ -f "$D/SURUM" ] && install -m 0644 "$D/SURUM" "$HEDEF/opt/armonia/guncelle/SURUM"
cp -a "$KAYNAK/guncelle-katmani/includes.chroot/." "$HEDEF/"
install -m 0644 "$ANAHTAR" "$HEDEF/etc/armonia/guncelle/anahtarlar"
install -d "$KANCA"
install -m 0755 "$KAYNAK/guncelle-katmani/hooks/0230-armonia-guncelle.hook.chroot" "$KANCA/"
# Windows'tan gelmiş olabilecek CR satır sonları
find "$KAYNAK/guncelle-katmani/includes.chroot" -type f -printf '%P\n' | while IFS= read -r f; do
  sed -i 's/\r$//' "$HEDEF/$f"
done
sed -i 's/\r$//' "$KANCA/0230-armonia-guncelle.hook.chroot" "$HEDEF/etc/armonia/guncelle/anahtarlar"
echo "Armonia güncelleme katmanı kopyalandı: $(sed -n 's/^surum=//p' "$D/SURUM" 2>/dev/null) ($MIMARI)"
