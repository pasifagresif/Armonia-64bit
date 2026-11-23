#!/bin/sh
# Armonia Pi: ilk acilis sifrelemesinin son adimi (armonia-sifreleme-son.service). Claude, 2026-11-23.
# initramfs'i crypttab ile yeniden uretir (cryptroot artik kok bolumu acar), cmdline.txt'yi sifreli koke cevirir ve
# ilk-sifreleme bayragini kaldirir. Bir adim basarisiz olursa bayrak KALIR: initramfs betigi kok bolumu acmaya devam
# eder, sonraki acilista bu servis yeniden dener (sistem acilmaz hale gelmez).
set -eu
CMD=/boot/firmware/cmdline.txt
HARITA=/dev/mapper/armonia-kok
kaydet() { echo "armonia-sifreleme-son: $1"; }

[ "$(findmnt -n -o SOURCE /)" = "$HARITA" ] || { kaydet "kok $HARITA degil; bekleniyor"; exit 1; }
grep -q '^armonia-kok ' /etc/crypttab || { kaydet "crypttab kaydi yok"; exit 1; }
mountpoint -q /boot/firmware || { kaydet "/boot/firmware bagli degil"; exit 1; }

# 1. initramfs (Pi: raspi-firmware kancasi /boot/firmware'e kopyalar)
update-initramfs -u -k all
# Uretilen initramfs'lerde cryptroot yapilandirmasi olmali; yoksa bayrak kaldirilmaz
for f in /boot/initrd.img-*; do
  lsinitramfs "$f" | grep -q 'cryptroot/crypttab' || { kaydet "$f icinde cryptroot yok"; exit 1; }
done
# Firmware'in GERCEKTEN yukledigi kopyalar (Pi 4: initramfs8, Pi 5: initramfs_2712) da guncel olmali (denetci O2)
grep -q '^auto_initramfs=1' /boot/firmware/config.txt || { kaydet "config.txt'de auto_initramfs=1 yok"; exit 1; }
sayi=0
for f in /boot/firmware/initramfs8 /boot/firmware/initramfs_2712 /boot/firmware/initramfs; do
  [ -e "$f" ] || continue
  lsinitramfs "$f" | grep -q 'cryptroot/crypttab' || { kaydet "$f icinde cryptroot yok"; exit 1; }
  sayi=$((sayi + 1))
done
[ "$sayi" -gt 0 ] || { kaydet "/boot/firmware altinda initramfs yok"; exit 1; }

# 2. cmdline.txt: root=sifreli kok, gecici parametreler cikar (once yedek, sonra yerine koy)
cp "$CMD" "$CMD.sifreleme-oncesi"
sed -e "s#\broot=[^ ]*#root=$HARITA#" -e 's# armonia.ilk_sifreleme=1##' -e 's# armonia.kok=[^ ]*##' -e 's# resize\b##g' \
  "$CMD" > "$CMD.yeni"
grep -q "root=$HARITA" "$CMD.yeni" || { kaydet "cmdline guncellenemedi"; rm -f "$CMD.yeni"; exit 1; }
mv -f "$CMD.yeni" "$CMD"
sync
kaydet "tamam: $(cat "$CMD")"
systemctl disable armonia-sifreleme-son.service
