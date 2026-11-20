#!/bin/bash
# Armonia Raspberry Pi DENEME imaji (arm64, Pi 4/5). Kullanici istegi 19.11: "ilk deneme icin raspberry pi icin
# derleme yapalim. Denemesi de kolay olur." Claude, 2026-11-19.
#
# Taban: Raspberry Pi OS Lite (trixie, arm64) resmi imaji -> uzerine ayni Armonia katmani (config/includes.chroot,
# sertlestirme/, paket listesi, 0100-armonia kancasi, kendi uygulamalarimiz). live-build KULLANILMAZ: Pi'nin
# acilisi (firmware bolumu, Pi cekirdegi) ISO'dan farkli. Cikti diske yazilip dogrudan acilan bir imajdir.
# Farklar: e-imza yok (paket yalniz amd64); /etc/armonia/deneme isareti -> canli sistem gibi (parola, kilit yok).
#
# Kullanim (Pi'de, aarch64):  sudo bash armonia-linux/pi_derle.sh
#   CIKTI=<dizin>   cikti ve onbellek (varsayilan: ~/armonia-pi, sudo cagiraninin evi)
#   BUYUT=<GB>      kok bolume eklenecek yer (varsayilan 5)
#   SIKISTIR=1      ciktiyi ayrica .img.xz yap
# Cikti: $CIKTI/armonia-pi.img  -> Raspberry Pi Imager ("Use custom") ya da dd ile USB bellege / SD karta.
set -euo pipefail
KAYNAK="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
[ "$(id -u)" = "0" ] || { echo "HATA: root ile calistirin (sudo)"; exit 1; }
[ "$(uname -m)" = "aarch64" ] || { echo "HATA: arm64 makinede calisir (chroot dogrudan, qemu yok)"; exit 1; }
EV=$( [ -n "${SUDO_USER:-}" ] && getent passwd "$SUDO_USER" | cut -d: -f6 || echo /root )
CIKTI="${CIKTI:-$EV/armonia-pi}"
BUYUT="${BUYUT:-5}"
TARIH=2026-10-06
TABAN="${TARIH}-raspios-trixie-arm64-lite.img.xz"
URL="https://downloads.raspberrypi.com/raspios_lite_arm64/images/raspios_lite_arm64-${TARIH}/${TABAN}"
IMAJ="$CIKTI/armonia-pi.img"
K="$CIKTI/kok"
mkdir -p "$CIKTI"
for p in curl xz parted losetup e2fsck resize2fs sha256sum; do command -v $p >/dev/null || { echo "HATA: $p yok"; exit 1; }; done

# 1. Taban imaj: indir + sagla (sha256 resmi sunucudan, HTTPS)
cd "$CIKTI"
if [ ! -f "$TABAN" ]; then
  echo "Taban imaj indiriliyor: $URL"
  curl -fL --retry 3 -o "$TABAN.indiriliyor" "$URL" && mv "$TABAN.indiriliyor" "$TABAN"
fi
curl -fsSL -o "$TABAN.sha256" "$URL.sha256"
sha256sum -c "$TABAN.sha256" || { echo "HATA: taban imaj saglamasi tutmadi"; rm -f "$TABAN"; exit 2; }

# 2. Calisma kopyasi, kok bolumu buyut
echo "Imaj aciliyor"
rm -f "$IMAJ"
xz -dc "$TABAN" > "$IMAJ"
truncate -s "+${BUYUT}G" "$IMAJ"
parted -s "$IMAJ" resizepart 2 100%
LOOP=$(losetup -fP --show "$IMAJ")

temizle() {
  set +e
  if [ -d "$K" ]; then
    rm -f "$K/usr/sbin/policy-rc.d"
    [ -f "$K/etc/resolv.conf.armonia-yedek" ] && mv -f "$K/etc/resolv.conf.armonia-yedek" "$K/etc/resolv.conf"
    # Bagli her seyi icten disa ayir
    findmnt -rn -o TARGET | grep -F "$K" | sort -r | while read -r m; do umount "$m" || umount -l "$m"; done
  fi
  [ -n "${LOOP:-}" ] && losetup -d "$LOOP"
}
trap temizle EXIT

e2fsck -fy "${LOOP}p2" >/dev/null || true
resize2fs "${LOOP}p2" >/dev/null
mkdir -p "$K"
mount "${LOOP}p2" "$K"
mount "${LOOP}p1" "$K/boot/firmware"
mount --bind /dev "$K/dev"
mount -t devpts devpts "$K/dev/pts"
mount -t proc proc "$K/proc"
mount -t sysfs sysfs "$K/sys"
mount -t tmpfs tmpfs "$K/run"
mount -t tmpfs tmpfs "$K/tmp"
# chroot icinde servis baslatilmasin
printf '#!/bin/sh\nexit 101\n' > "$K/usr/sbin/policy-rc.d"; chmod 0755 "$K/usr/sbin/policy-rc.d"
# Paket indirme icin DNS (acilista armonia-resolv.conf tmpfiles kurali resolved'a baglar)
cp -a "$K/etc/resolv.conf" "$K/etc/resolv.conf.armonia-yedek" 2>/dev/null || true
rm -f "$K/etc/resolv.conf"; cp /etc/resolv.conf "$K/etc/resolv.conf"

# 3. Paketler (apt-recommends kapali; ISO ile ayni liste)
PAKETLER=$(grep -hv '^\s*#' "$KAYNAK"/config/package-lists/*.list.chroot | tr -s ' \t\r' '\n' | grep -v '^$' | sort -u | tr '\n' ' ')
echo "Paketler kuruluyor (uzun surer)"
chroot "$K" apt-get update -qq
chroot "$K" env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
  -o Dpkg::Options::=--force-confold $PAKETLER
# Pi OS'un ilk acilis sihirbazi (kullanici adi sorar) ve cloud-init: Armonia kendi kullanicisini acar
chroot "$K" env DEBIAN_FRONTEND=noninteractive apt-get purge -y -q userconf-pi 2>/dev/null || true
mkdir -p "$K/etc/cloud"; touch "$K/etc/cloud/cloud-init.disabled"

# 4. Armonia katmani (iso_derle.sh ile ayni kaynaklar)
cp -a "$KAYNAK/config/includes.chroot/." "$K/"
cp -a "$KAYNAK/sertlestirme/etc/." "$K/etc/"
mkdir -p "$K/usr/share/plymouth/themes/armonia"
cp "$KAYNAK"/plymouth/armonia/* "$K/usr/share/plymouth/themes/armonia/"
bash "$KAYNAK/uygulamalari_hazirla.sh" "$K" aarch64
# Windows'tan gelmis olabilecek CR satir sonlari
find "$K/opt/armonia" "$K/etc/armonia" "$K/etc/systemd/system" -type f \
  ! -path "$K/opt/armonia/uygulamalar/*" -exec sed -i 's/\r$//' {} +
chmod 0755 "$K"/opt/armonia/bin/*.sh "$K/opt/armonia/bin/ayar-yardimci" "$K/usr/local/bin/xdg-open"
touch "$K/etc/armonia/deneme"
cp "$KAYNAK/config/hooks/normal/0100-armonia.hook.chroot" "$K/tmp/kanca"
echo "Armonia kancasi calisiyor"
chroot "$K" env ARMONIA_PI=1 /bin/sh /tmp/kanca

# 5. Pi'ye ozgu: cekirdek parametreleri (AppArmor Pi cekirdeginde varsayilan LSM degil) ve sessiz acilis
CMD="$K/boot/firmware/cmdline.txt"
EK="$(tr -d '\r\n' < "$KAYNAK/sertlestirme/cekirdek_parametreleri.txt")"
EK="${EK/ia32_emulation=0/}"
grep -q 'lsm=' "$CMD" || sed -i "1 s|\$| lsm=landlock,lockdown,yama,integrity,apparmor,bpf quiet splash loglevel=3 $EK|" "$CMD"
sed -i 's/  */ /g' "$CMD"
echo "cmdline.txt: $(cat "$CMD")"
chroot "$K" apt-get clean
rm -f "$K/tmp/kanca"

# 6. Ayir ve bitir
temizle; trap - EXIT; LOOP=""
L2=$(losetup -fP --show "$IMAJ"); e2fsck -fy "${L2}p2" >/dev/null 2>&1 || true; losetup -d "$L2"
[ -n "${SUDO_USER:-}" ] && chown "$SUDO_USER": "$IMAJ"
ls -la "$IMAJ"
if [ "${SIKISTIR:-0}" = "1" ]; then xz -T0 -kf "$IMAJ" && ls -la "$IMAJ.xz"; fi
echo "Hazir: $IMAJ  (Raspberry Pi Imager -> 'Use custom' ya da dd ile USB bellek / SD kart)"
