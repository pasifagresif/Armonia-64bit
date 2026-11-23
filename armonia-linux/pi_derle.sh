#!/bin/bash
# Armonia Raspberry Pi DENEME imaji (arm64, Pi 4/5). Kullanici istegi 19.11: "ilk deneme icin raspberry pi icin
# derleme yapalim. Denemesi de kolay olur." Claude, 2026-11-19.
#
# Taban: Raspberry Pi OS Lite (trixie, arm64) resmi imaji -> uzerine ayni Armonia katmani (config/includes.chroot,
# sertlestirme/, paket listesi, 0100-armonia kancasi, kendi uygulamalarimiz). live-build KULLANILMAZ: Pi'nin
# acilisi (firmware bolumu, Pi cekirdegi) ISO'dan farkli. Cikti diske yazilip dogrudan acilan bir imajdir.
# Farklar: e-imza amd64 paketinin jar'i + arm64 OpenJDK 21 ile (AKIS PKCS#11 yerine APDU, 20.11); /etc/armonia/deneme isareti -> canli sistem gibi (parola, kilit yok).
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
# Adalet E-imza (resmi paket yalniz amd64): yalniz icindeki jar kullanilir, arm64 OpenJDK 21 ile calisir (20.11).
EIMZA_DEB=adalet-eimza-tray_1.0.17_amd64.deb
EIMZA_MD5=a1a4b3c3099df599c2d5193f2259c1aa
# Once depodaki parcalardan (eimza-parcalar/, 23.11.2026) birlestirilir; parca yoksa resmi sunucudan indirilir.
if [ ! -f "$EIMZA_DEB" ] && [ -f "$KAYNAK/eimza-parcalar/$EIMZA_DEB.sha256" ]; then
  cat "$KAYNAK/eimza-parcalar/$EIMZA_DEB".parca-* > "$EIMZA_DEB"
  sha256sum -c --quiet "$KAYNAK/eimza-parcalar/$EIMZA_DEB.sha256" \
    && echo "E-imza paketi parcalardan birlestirildi" || { echo "UYARI: e-imza parcalari bozuk; indirilecek"; rm -f "$EIMZA_DEB"; }
fi
if [ ! -f "$EIMZA_DEB" ]; then
  curl -fL --retry 3 -o "$EIMZA_DEB.indiriliyor" "https://cdn.uyap.gov.tr/framework/public/$EIMZA_DEB" \
    && mv "$EIMZA_DEB.indiriliyor" "$EIMZA_DEB"
fi
echo "$EIMZA_MD5  $EIMZA_DEB" | md5sum -c - || { echo "HATA: e-imza paketi saglamasi tutmadi"; rm -f "$EIMZA_DEB"; exit 2; }

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
# Dosya sistemi bolum sonundan 64 MB kisa: ilk acilista yerinde LUKS2 sifrelemesi (--reduce-device-size 32M)
# basliga yer ister (G1, 23.11). Bolum boyu 1K blok cinsinden; ilk acilis bolumu diske gore ayrica buyutur.
BOLUM_KB=$(( $(blockdev --getsize64 "${LOOP}p2") / 1024 ))
resize2fs "${LOOP}p2" "$(( BOLUM_KB - 65536 ))K" >/dev/null
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
  -o Dpkg::Options::=--force-confold $PAKETLER openjdk-21-jre cryptsetup-initramfs
# Pi OS'un ilk acilis sihirbazi (kullanici adi sorar) ve cloud-init: Armonia kendi kullanicisini acar
chroot "$K" env DEBIAN_FRONTEND=noninteractive apt-get purge -y -q userconf-pi 2>/dev/null || true
# Uzaktan erisim YOK (yol haritasi §5 karar 4: "SSH YOK"). Pi OS Lite openssh-server ve rpi-connect-lite getirir.
for p in openssh-server openssh-sftp-server rpi-connect-lite rpi-connect; do
  chroot "$K" dpkg -s "$p" >/dev/null 2>&1 && chroot "$K" env DEBIAN_FRONTEND=noninteractive apt-get purge -y -q "$p"
done
if chroot "$K" dpkg -s openssh-server >/dev/null 2>&1 || [ -e "$K/usr/sbin/sshd" ]; then
  echo "HATA: imajda sshd kaldi"; exit 1
fi
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
# 23.11 (G1): deneme isareti KALDIRILDI. Pi imaji kalici (diske yazar); kilit ve parola canli sistem gibi kapali
# kalamaz. Ilk acilista LUKS2 + tek parola (disk + 'armonia' kullanicisi) belirlenir.
rm -f "$K/etc/armonia/deneme"
# G1: ilk acilis sifrelemesi (initramfs) + son adim servisi
install -m 0755 "$KAYNAK/pi/initramfs/hooks/armonia-ilk-sifreleme" "$K/etc/initramfs-tools/hooks/armonia-ilk-sifreleme"
install -d -m 0755 "$K/etc/initramfs-tools/scripts/local-top"
install -m 0755 "$KAYNAK/pi/initramfs/scripts/local-top/armonia-ilk-sifreleme" \
  "$K/etc/initramfs-tools/scripts/local-top/armonia-ilk-sifreleme"
install -m 0755 "$KAYNAK/pi/sifreleme_son.sh" "$K/opt/armonia/bin/sifreleme_son.sh"
install -m 0644 "$KAYNAK/pi/armonia-sifreleme-son.service" "$K/etc/systemd/system/armonia-sifreleme-son.service"
sed -i 's/\r$//' "$K"/etc/initramfs-tools/hooks/armonia-ilk-sifreleme \
  "$K"/etc/initramfs-tools/scripts/local-top/armonia-ilk-sifreleme "$K"/opt/armonia/bin/sifreleme_son.sh \
  "$K"/etc/systemd/system/armonia-sifreleme-son.service
# Kok bolum henuz sifreli degilken de cryptsetup + askpass initramfs'e girsin
install -d -m 0755 "$K/etc/cryptsetup-initramfs"
grep -q '^CRYPTSETUP=y' "$K/etc/cryptsetup-initramfs/conf-hook" 2>/dev/null \
  || echo 'CRYPTSETUP=y' >> "$K/etc/cryptsetup-initramfs/conf-hook"
# Parola ekraninda TR klavye (konsol duzeni initramfs'e)
sed -i 's/^#\?KEYMAP=.*/KEYMAP=y/' "$K/etc/initramfs-tools/initramfs.conf"
grep -q '^KEYMAP=y' "$K/etc/initramfs-tools/initramfs.conf" || echo 'KEYMAP=y' >> "$K/etc/initramfs-tools/initramfs.conf"
# setupcon onbellegi taban imajdan (GB) kalmasin: /etc/default/keyboard (TR) cp -a ile eski tarihli gelebilir ve
# setupcon eski onbellegi kullanir -> parola initramfs'te GB, ekran kilidinde TR duzeniyle girilir (denetci O3)
rm -f "$K"/etc/console-setup/cached_*
touch "$K/etc/default/keyboard"
grep -q '^XKBLAYOUT="tr"' "$K/etc/default/keyboard" || { echo "HATA: /etc/default/keyboard TR degil"; exit 1; }
chroot "$K" setupcon --save-only
ls "$K"/etc/console-setup/cached_*.kmap.gz >/dev/null 2>&1 || { echo "HATA: TR konsol klavye onbellegi uretilemedi"; exit 1; }
# G3 (23.11): Pi'de kullanicinin parolasi var (tek parola) -> Ayarlar'in root yardimcisi parolasiz calismaz;
# oturumdaki baska bir surec (arka planda) sessizce ag/yazici/klavye ayarini degistiremez. Pencere: lxpolkit.
POL="$K/usr/share/polkit-1/actions/tr.armonia.ayar.policy"
sed -i 's|<allow_active>yes</allow_active>|<allow_active>auth_self_keep</allow_active>|' "$POL"
grep -q '<allow_active>auth_self_keep</allow_active>' "$POL" || { echo "HATA: polkit politikasi degistirilemedi"; exit 1; }
# Sifresiz FAT bolumdeki cmdline.txt'ye systemd.run= eklenerek disk acildiktan sonra root komut calistirilmasin
# (denetci O7, kisa vadeli; asil cozum Faz 6 imzali acilis)
install -d -m 0755 "$K/etc/systemd/system-generators"
ln -sf /dev/null "$K/etc/systemd/system-generators/systemd-run-generator"
# Her cihaz kendi kimligini uretsin: ayni imajdan yazilan bellekler ayni machine-id / rastgele tohum tasimasin
: > "$K/etc/machine-id"; rm -f "$K/var/lib/dbus/machine-id" "$K/var/lib/systemd/random-seed" "$K"/etc/ssh/ssh_host_*
# E-imza: jar + Pi'ye ozgu birim ek ayarlari (kanca eimza kullanicisini acar ve birimi etkinlestirir)
EJ=$(mktemp -d); dpkg-deb -x "$CIKTI/$EIMZA_DEB" "$EJ"
install -d -m 0755 "$K/opt/armonia/eimza"
install -m 0644 "$EJ"/usr/lib/adalet-eimza-tray/lib/app/eimza-tray-*.jar "$K/opt/armonia/eimza/eimza-tray.jar"
rm -rf "$EJ"
for d in armonia-eimza.service.d pcscd.service.d; do
  install -d -m 0755 "$K/etc/systemd/system/$d"; install -m 0644 "$KAYNAK/pi/$d/"*.conf "$K/etc/systemd/system/$d/"
done
cp "$KAYNAK/config/hooks/normal/0100-armonia.hook.chroot" "$K/tmp/kanca"
echo "Armonia kancasi calisiyor"
chroot "$K" env ARMONIA_PI=1 /bin/sh /tmp/kanca
# G6 (23.11): USBGuard Pi'de yeniden ACIK. 20.11'de kapatilma nedeni (kok USB bellegi acilista bekletip sistemi
# dusurmesi) armonia-usb-kok.service ile cozuldu: usbguard'dan once kok disk icin dar izin kurali (ozet + kimlik +
# seri + yalniz yigin depolama arayuzu) yazilir. "Takinca sor" fiziksel onayla (cikar + tak) calisir.
install -m 0755 "$KAYNAK/pi/usb_kok_izin.sh" "$K/opt/armonia/bin/usb_kok_izin.sh"
install -m 0644 "$KAYNAK/pi/armonia-usb-kok.service" "$K/etc/systemd/system/armonia-usb-kok.service"
install -d -m 0755 "$K/etc/systemd/system/usbguard.service.d"
install -m 0644 "$KAYNAK/pi/usbguard.service.d/20-pi-kok.conf" "$K/etc/systemd/system/usbguard.service.d/20-pi-kok.conf"
sed -i 's/\r$//' "$K/opt/armonia/bin/usb_kok_izin.sh" "$K/etc/systemd/system/armonia-usb-kok.service" \
  "$K/etc/systemd/system/usbguard.service.d/20-pi-kok.conf"
chroot "$K" systemctl unmask usbguard.service 2>/dev/null || true
chroot "$K" systemctl enable armonia-usb-kok.service usbguard.service armonia-usb-sor.service
chroot "$K" systemctl is-enabled usbguard.service armonia-usb-kok.service armonia-usb-sor.service \
  || { echo "HATA: USBGuard birimleri etkin degil"; exit 1; }

# 5. Pi'ye ozgu: cekirdek parametreleri (AppArmor Pi cekirdeginde varsayilan LSM degil) ve sessiz acilis
CMD="$K/boot/firmware/cmdline.txt"
EK="$(tr -d '\r\n' < "$KAYNAK/sertlestirme/cekirdek_parametreleri.txt")"
EK="${EK/ia32_emulation=0/}"
grep -q 'lsm=' "$CMD" || sed -i "1 s|\$| lsm=landlock,lockdown,yama,integrity,apparmor,bpf quiet splash loglevel=3 $EK|" "$CMD"
# G1: Pi OS'un ilk acilis bolum/dosya sistemi buyutmesi kapatilir (sifreli bolumu bozardi); buyutmeyi
# armonia-ilk-sifreleme initramfs betigi sifrelemeden once yapar.
# Trixie'de mekanizma cmdline'daki ' resize' sozcugu (initramfs resize_early + set_partuuid) ve rpi-resize.service
# (denetci O1); eski surumlerde init=...firstboot. Hepsi kaldirilir.
sed -i -e 's#\binit=[^ ]*firstboot[^ ]*##' -e 's#\bsystemd\.run[^ ]*##g' -e 's# resize\b##g' "$CMD"
for b in rpi-resize.service rpi-resize-swap-file.service resize2fs_once.service; do
  [ -e "$K/lib/systemd/system/$b" ] || [ -e "$K/etc/systemd/system/$b" ] && chroot "$K" systemctl mask "$b"
done
ls "$K"/usr/share/initramfs-tools/scripts/*/ 2>/dev/null | grep -i resize && echo "UYARI: initramfs'te buyutme betigi var (yukarida); denetleyin"
# Kok bolum (sifrelemeden once): mevcut root= degeri armonia.kok olarak saklanir
KOK=$(grep -o '\broot=[^ ]*' "$CMD" | head -1 | cut -d= -f2-)
[ -n "$KOK" ] || { echo "HATA: cmdline.txt'de root= yok"; exit 1; }
grep -q 'armonia.ilk_sifreleme=1' "$CMD" || sed -i "1 s|\$| armonia.ilk_sifreleme=1 armonia.kok=$KOK|" "$CMD"
sed -i 's/  */ /g' "$CMD"
grep -qwE 'firstboot|resize' "$CMD" && { echo "HATA: cmdline'da ilk acilis buyutmesi kaldi"; exit 1; }
echo "cmdline.txt: $(cat "$CMD")"
# Firmware initramfs'i yuklesin
CFG="$K/boot/firmware/config.txt"
grep -q '^auto_initramfs=1' "$CFG" || echo 'auto_initramfs=1' >> "$CFG"
chroot "$K" systemctl enable armonia-sifreleme-son.service
# initramfs yeniden uretilir ve icerigi denetlenir (raspi-firmware kancasi /boot/firmware'e kopyalar)
chroot "$K" update-initramfs -u -k all
# Hem /boot/initrd.img-* hem firmware'in yukledigi /boot/firmware/initramfs* denetlenir (denetci O2, O4, D9)
FW=0
for f in "$K"/boot/initrd.img-* "$K"/boot/firmware/initramfs8 "$K"/boot/firmware/initramfs_2712; do
  [ -e "$f" ] || continue
  case "$f" in "$K"/boot/firmware/*) FW=$((FW + 1)) ;; esac
  ic=$(chroot "$K" lsinitramfs "${f#"$K"}")
  for g in 'scripts/local-top/armonia-ilk-sifreleme$' 'sbin/cryptsetup$' 'lib/cryptsetup/askpass$' 'sbin/sfdisk$' \
           'sbin/resize2fs$' 'sbin/partx$' 'bin/loadkeys$' 'etc/console-setup/' 'dm-crypt\.ko' 'adiantum\.ko'; do
    grep -qE "$g" <<<"$ic" || { echo "HATA: ${f#"$K"} icinde $g yok"; exit 1; }
  done
done
[ "$FW" -gt 0 ] || { echo "HATA: /boot/firmware altinda initramfs8/initramfs_2712 yok"; exit 1; }
echo "initramfs denetimi tamam"
chroot "$K" apt-get clean
rm -f "$K/tmp/kanca"

# 6. Ayir ve bitir
temizle; trap - EXIT; LOOP=""
L2=$(losetup -fP --show "$IMAJ"); e2fsck -fy "${L2}p2" >/dev/null 2>&1 || true; losetup -d "$L2"
[ -n "${SUDO_USER:-}" ] && chown "$SUDO_USER": "$IMAJ"
ls -la "$IMAJ"
if [ "${SIKISTIR:-0}" = "1" ]; then xz -T0 -kf "$IMAJ" && ls -la "$IMAJ.xz"; fi
echo "Hazir: $IMAJ  (Raspberry Pi Imager -> 'Use custom' ya da dd ile USB bellek / SD kart)"
