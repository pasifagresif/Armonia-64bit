#!/bin/bash
# Armonia'nin kendi uygulamalarini (armonia-uygulamalar/: Ayarlar, Hukuk Hesaplayici) derler ve imajin
# includes.chroot agacina koyar. iso_derle.sh (amd64) ve pi_derle.sh (arm64) cagirir. Claude, 2026-11-19.
#   uygulamalari_hazirla.sh <includes.chroot dizini> <x86_64|aarch64>
# cargo bulunursa (PATH, /root/.cargo, $SUDO_USER'in ~/.cargo) once derler; bulunmazsa onceden derlenmis
# target/release ikililerini kullanir. Ikilinin mimarisi ELF basligindan denetlenir.
set -euo pipefail
HEDEF=$1 MIMARI=$2
KAYNAK="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
UYG="$KAYNAK/../armonia-uygulamalar"
[ -f "$UYG/Cargo.toml" ] || { echo "HATA: $UYG yok"; exit 6; }
CARGO=$(command -v cargo || true)
for c in /root/.cargo/bin/cargo ${SUDO_USER:+/home/$SUDO_USER/.cargo/bin/cargo}; do
  [ -n "$CARGO" ] || { [ -x "$c" ] && CARGO=$c; } || true
done
if [ -n "$CARGO" ]; then
  echo "Uygulamalar derleniyor ($CARGO)"
  if [ -n "${SUDO_USER:-}" ] && [ "$CARGO" = "/home/$SUDO_USER/.cargo/bin/cargo" ]; then
    # Kullanicinin rustup kurulumu: derleme kullanici kimligiyle (target/ root'a gecmesin)
    sudo -u "$SUDO_USER" env -C "$UYG" "$CARGO" build --release --workspace -q
  else
    (cd "$UYG" && "$CARGO" build --release --workspace -q)
  fi
else
  echo "UYARI: cargo yok; onceden derlenmis ikililer kullanilacak"
fi
elf_mimari() {  # ELF e_machine (ofset 18, little-endian): 3e00 = x86_64, b700 = aarch64
  case "$(od -An -tx1 -j18 -N2 "$1" | tr -d ' \n')" in 3e00) echo x86_64 ;; b700) echo aarch64 ;; *) echo bilinmiyor ;; esac
}
install -d -m 0755 "$HEDEF/opt/armonia/uygulamalar"
for b in armonia-ayarlar hukuk_hesap; do
  f="$UYG/target/release/$b"
  [ -f "$f" ] || { echo "HATA: $f yok (cargo build --release --workspace)"; exit 6; }
  m=$(elf_mimari "$f")
  [ "$m" = "$MIMARI" ] || { echo "HATA: $b mimarisi $m, beklenen $MIMARI"; exit 6; }
  install -m 0755 "$f" "$HEDEF/opt/armonia/uygulamalar/$b"
done
echo "Uygulamalar hazir: $(ls "$HEDEF/opt/armonia/uygulamalar" | tr '\n' ' ')"
