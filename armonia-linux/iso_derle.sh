#!/bin/bash
# Armonia ISO'sunu live-build ile derler. 2026-11-19'dan beri temel masaustu imaji; kasa ve PostgreSQL GIRMEZ (kasa/ rafta).
# COKLUUYAP=1 (varsayilan, 23.11): Coklu UYAP (uyap_app, Rust Linux surumu) MENUDEN acilan, kum havuzlu katman olarak
# girer; acilista HICBIR sey baslatmaz (cokluuyap-katmani/menuden/, cokluuyap_hazirla.sh). COKLUUYAP=0: katmansiz.
# YAZI=1 (varsayilan, 24.11): Armonia Yazi (belge editoru, kum havuzlu) + imza hizmeti (yazi-katmani/, yazi_hazirla.sh;
# ikililer armonia-yazi/derle.sh ciktisindan). YAZI=0: katmansiz.
# Root ile:  wsl -d Debian -u root -- bash /mnt/c/.../armonia-linux/iso_derle.sh   (ya da: sudo bash .../iso_derle.sh)
#   TEST=1 ... iso_derle.sh  -> test katmani da eklenir (kasa bicimlendirme, lynis), cikti armonia_test_kasa.iso
# Cikti: ISO_DIZIN/armonia.iso  (varsayilan: /mnt/c/dev varsa orasi = C:\dev, yoksa /root/armonia-cikti)
# Yollar betigin kendi yerinden bulunur; ortam degiskeniyle degistirilebilir:
#   KAYNAK=<armonia-linux dizini>  ISO_DIZIN=<ISO hedef dizini>  IS=<lb calisma dizini>
# Imaj amd64'tur: x86_64 olmayan makinede (or. Raspberry Pi) durur; ZORLA=1 ile gecilir.
# Claude, 2026-11-13 (14'te sertlestirme + kasa + Plymouth; 19.11 yollar tasinabilir, temel masaustu).
set -euo pipefail
KAYNAK="${KAYNAK:-$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)}"
IS="${IS:-/root/armonia-iso}"
if [ -z "${ISO_DIZIN:-}" ]; then
  if [ -d /mnt/c/dev ]; then ISO_DIZIN=/mnt/c/dev; else ISO_DIZIN=/root/armonia-cikti; fi
fi
mkdir -p "$ISO_DIZIN"
TEST="${TEST:-0}"
# Windows katmani (windows/BENIOKU.md): Icramatik icin Debian Wine 32+64 bit, agsiz bubblewrap kum havuzunda.
# Kullanici karari 23.11: Icramatik 32 bit -> WINDOWS=1'de ia32_emulation=0 KALDIRILIR (bilincli gevseme).
# WINDOWS=0: Windows katmani yok, ia32_emulation=0 yerinde (tam sertlestirilmis ISO).
WINDOWS="${WINDOWS:-1}"
# Coklu UYAP katmani (menuden; uyap_app derle_uyap.sh ciktisindan, x86_64)
COKLUUYAP="${COKLUUYAP:-1}"
# Armonia Yazi katmani (belge editoru + imza hizmeti; armonia-yazi/derle.sh ciktisindan, x86_64)
YAZI="${YAZI:-1}"
# TEST=1 katmani (kasa bicimlendirme/dogrulama) kasa katmanina dayanir; temel masaustu imajinda kasa yok
[ "$TEST" = "1" ] && { echo "HATA: TEST=1 kasa katmani ister; temel imajda kullanilmaz (Coklu UYAP asamasinda geri gelecek)"; exit 1; }
[ "$(id -u)" = "0" ] || { echo "HATA: root ile calistirin (live-build root ister)"; exit 1; }
if [ "$(uname -m)" != "x86_64" ] && [ "${ZORLA:-0}" != "1" ]; then
  echo "HATA: bu makine $(uname -m); Armonia imaji amd64 (e-imza paketi amd64). ZORLA=1 ile gecilir."; exit 1
fi
[ -d "$KAYNAK/config" ] || { echo "HATA: armonia-linux kaynagi yok: $KAYNAK"; exit 1; }
# E-imza paketi (70 MB) depoda eimza-parcalar/ altinda 10 MB'lik parcalar halinde durur (tek dosya GitHub'a
# yuklenemedi, 23.11.2026). Paket config/packages.chroot/ altinda yoksa parcalar birlestirilip sha256 denetlenir.
if ! ls "$KAYNAK"/config/packages.chroot/adalet-eimza-tray_*_amd64.deb >/dev/null 2>&1; then
  for sha in "$KAYNAK"/eimza-parcalar/*.deb.sha256; do
    [ -f "$sha" ] || continue
    deb=$(basename "$sha" .sha256)
    mkdir -p "$KAYNAK/config/packages.chroot"
    cat "$KAYNAK/eimza-parcalar/$deb".parca-* > "$KAYNAK/config/packages.chroot/$deb"
    (cd "$KAYNAK/config/packages.chroot" && sha256sum -c --quiet "$sha") \
      || { rm -f "$KAYNAK/config/packages.chroot/$deb"; echo "HATA: e-imza parcalari bozuk ($deb)"; exit 1; }
    echo "E-imza paketi parcalardan birlestirildi: $deb"
  done
fi
ls "$KAYNAK"/config/packages.chroot/adalet-eimza-tray_*_amd64.deb >/dev/null 2>&1 \
  || echo "UYARI: e-imza paketi yok: config/packages.chroot/ altina indirin, bkz. docs/GUNCEL_DURUM_2026-11-19.md"
mkdir -p "$IS"; cd "$IS"
# Paket onbellegi Faz 0 test derlemesinden devralinir (indirme suresi kisalir)
[ -d cache ] || { [ -d /root/iso-test/cache ] && cp -a /root/iso-test/cache . || true; }
lb clean >/dev/null 2>&1 || true
rm -rf config
EK_PARAM="$(tr -d '\r\n' < "$KAYNAK/sertlestirme/cekirdek_parametreleri.txt")"
if [ "$WINDOWS" = "1" ]; then
  EK_PARAM="$(echo "${EK_PARAM/ia32_emulation=0/}" | tr -s ' ')"
  echo "UYARI: WINDOWS=1 -> ia32_emulation=0 kaldirildi (32 bit Wine icin; kullanici karari 23.11)"
fi
# non-free-firmware (23.11): Wi-Fi / ekran karti yazilimlari + mikrokod; imajdaki apt kaynaklarina da girer
# (surucu.sh sonradan takilan aygit icin firmware-* kurabilsin). --firmware-chroot false: live-build butun
# firmware paketlerini KOYMASIN, yalniz 0040-armonia-donanim-yazilimi kancasindaki liste girsin.
ACILIS="boot=live quiet splash loglevel=3 toram $EK_PARAM$( [ "$TEST" = "1" ] && echo ' armonia.test_kasa=1')$( [ "${OLCUM_TARAYICI:-0}" = "1" ] && echo ' armonia.olcum_tarayici=1')$( { [ "$TEST" = "1" ] || [ "${OLCUM_TARAYICI:-0}" = "1" ] || [ "${OLCUM:-0}" = "1" ]; } && echo ' armonia.olcum=1')"
# UEFI (23.11): BIOS'ta syslinux, UEFI'de GRUB. --uefi-secure-boot auto: Debian'in imzali shim + GRUB'u varsa
# Secure Boot acik makinede de acilir (imzali Debian cekirdegi). Derleme sonunda denetlenir.
lb config --distribution trixie --archive-areas "main non-free-firmware" --debian-installer none \
  --firmware-chroot false --firmware-binary false \
  --binary-images iso-hybrid --bootloaders "syslinux,grub-efi" --uefi-secure-boot auto \
  --apt-recommends false --memtest none \
  --iso-volume ARMONIA --chroot-squashfs-compression-type zstd \
  --bootappend-live "$ACILIS" >/dev/null
cp -a "$KAYNAK/config/." config/
# GRUB (UEFI) syslinux'taki gibi: menu yok, bekleme yok, cekirdek satiri ACILISTA KLAVYEYLE duzenlenemez.
# live-build'in varsayilan GRUB menusu 'e' ile cekirdek satirini degistirmeye (init=/bin/sh -> parolasiz root) ve
# 'c' ile GRUB komut satirina izin verir. superusers tanimli ama parolasi YOK -> kimse yetkilendirilemez; duzenleme
# ve komut satiri kapali, yalniz --unrestricted girdi acilir. Sablonun diger dosyalari (install_*.cfg, tema)
# live-build'den kopyalanir (binary_grub_cfg onlari bekler).
# SINIR (denetci 23.11 ORTA-1): USB'ye YAZABILEN biri grub.cfg'yi, initrd'yi ve syslinux ayarini Secure Boot acikken
# de degistirebilir (Secure Boot yalniz shim/GRUB/cekirdek ikililerini dogrular). Cozum Faz 6: kendi anahtarla
# imzali UKI (cekirdek + initrd + cekirdek satiri tek imza) ya da imzali satirda dm-verity kok ozeti.
G=config/bootloaders/grub-pc
rm -rf "$G"; cp -a /usr/share/live/build/bootloaders/grub-pc "$G"
cat > "$G/grub.cfg" <<EOT
# Armonia (UEFI): menu ve duzenleme yok. iso_derle.sh uretti, $(date -u +%Y-%m-%d).
set timeout=0
set timeout_style=hidden
set superusers="armonia-yonetim"
menuentry "Armonia" --unrestricted {
	linux /live/vmlinuz $ACILIS
	initrd /live/initrd.img
}
EOT
if [ "$WINDOWS" = "1" ]; then
  cp "$KAYNAK"/windows/hooks/*.hook.chroot config/hooks/normal/
  cp -a "$KAYNAK/windows/includes.chroot/." config/includes.chroot/
  grep -q '^PAYLASIM="//' "$KAYNAK/windows/includes.chroot/etc/armonia/windows.conf" \
    || echo "UYARI: windows.conf'ta PAYLASIM bos; Icramatik kisayolu 'Ayarlanmamis' diyecek"
  # Dock'a Icramatik kutucugu (yalniz bu derlemenin kopyasinda)
  D=config/includes.chroot/etc/armonia/waybar/dock.jsonc
  sed -i -e 's|"custom/belgeler", "wlr/taskbar"|"custom/belgeler", "custom/icramatik", "wlr/taskbar"|' \
    -e '/^  "custom\/belgeler":/a\  "custom/icramatik": { "format": "İcramatik\\n ", "justify": "center", "tooltip-format": "Ağdaki İcramatik (ağsız kum havuzu)", "on-click": "/opt/armonia/bin/windows_calistir.sh" },' "$D"
  grep -q '"custom/icramatik", "wlr/taskbar"' "$D" && grep -q '^  "custom/icramatik":' "$D" \
    || { echo "HATA: dock'a Icramatik eklenemedi"; exit 1; }
fi
# Faz 3 sertlestirme katmani (guvenlik-denetci denetiminden gecti, 2026-11-14)
cp -a "$KAYNAK/sertlestirme/etc/." config/includes.chroot/etc/
# Armonia'nin kendi uygulamalari (Ayarlar, Hukuk Hesaplayici): derlenip /opt/armonia/uygulamalar'a
bash "$KAYNAK/uygulamalari_hazirla.sh" config/includes.chroot x86_64
if [ "$COKLUUYAP" = "1" ]; then
  bash "$KAYNAK/cokluuyap_hazirla.sh" config/includes.chroot x86_64 config/hooks/normal
fi
if [ "$YAZI" = "1" ]; then
  bash "$KAYNAK/yazi_hazirla.sh" config/includes.chroot x86_64 config/hooks/normal
  # WebKitGTK yalniz bu katmanla girer (denetci 24.11 D9)
  cp "$KAYNAK/yazi-katmani/package-lists/yazi.list.chroot" config/package-lists/
fi
# Plymouth temasi. (Faz 2 kasa katmani -- LUKS2 kasa + PostgreSQL -- Coklu UYAP icindi; bu imaja girmez.)
mkdir -p config/includes.chroot/usr/share/plymouth/themes/armonia
cp "$KAYNAK"/plymouth/armonia/* config/includes.chroot/usr/share/plymouth/themes/armonia/
if [ "$TEST" = "1" ]; then
  echo "TEST katmani ekleniyor (YALNIZ test ISO'su)"
  cp -a "$KAYNAK/test/config-test/includes.chroot/." config/includes.chroot/
  cp "$KAYNAK"/test/config-test/hooks/normal/* config/hooks/normal/
  cp "$KAYNAK/test/config-test/test.list.chroot" config/package-lists/
fi
# SERI=1 -> uretim imaji + yalniz gunlugun COM1'e akmasi (uygulama hatasi teshisi; armonia_seri.iso). 2026-11-14.
if [ "${SERI:-0}" = "1" ]; then
  mkdir -p config/includes.chroot/etc/systemd/journald.conf.d
  cp "$KAYNAK/test/config-test/includes.chroot/etc/systemd/journald.conf.d/90-test-seri.conf" \
     config/includes.chroot/etc/systemd/journald.conf.d/
fi
# Tarayici (chromium) artik uretim katmaninda: config/ (armonia-tarayici.socket/@.service, xdg-open, politika).
# OLCUM_TARAYICI=1 -> olcum.sh acilistan sonra tarayiciyi bir kez acip kum havuzunu raporlar (armonia_tarayici_olcum.iso)
find config -type f \( -name '*.sh' -o -name '*.hook.*' -o -name '*.service' -o -name '*.conf' -o -name '*.cfg' \
  -o -name '*.in' -o -name '*.list.*' -o -name '*.script' -o -name '*.plymouth' -o -name 'crypttab' \
  -o -name '*.socket' -o -name '*.json' -o -name 'xdg-open' -o -name '*.xml' -o -name '*.rules' -o -name '*.nft' -o -name '*.slice' \
  -o -name '*.jsonc' -o -name '*.css' -o -name '*.ini' -o -name '*.toml' -o -name '*.desktop' -o -name 'autostart' \
  -o -name 'themerc-override' -o -name 'mimeapps.list' -o -name '*.xcd' -o -name '*.sha256' \
  -o -name '*.policy' -o -name 'environment' -o -name 'ayar-yardimci' -o -name 'tema-yardimci' \
  -o -name '*.sablon' -o -name 'paletler' \) \
  -exec sed -i 's/\r$//' {} +
# /mnt/c'den gelen dosyalar 0777 olur: imaja herkese-yazilabilir dosya girmesin
# (u=rwX 0777 dosyada x'i korur → systemd "marked executable" uyarisi; once hepsi 0644, sonra yalniz betikler 0755)
find config/includes.chroot config/hooks config/bootloaders -type d -exec chmod 0755 {} +
find config/includes.chroot config/hooks config/bootloaders -type f -exec chmod 0644 {} +
chmod 0755 config/hooks/normal/*.hook.chroot config/includes.chroot/opt/armonia/bin/*.sh
chmod 0755 config/includes.chroot/opt/armonia/bin/ayar-yardimci config/includes.chroot/opt/armonia/bin/tema-yardimci \
  config/includes.chroot/opt/armonia/uygulamalar/*
[ -f config/includes.chroot/usr/local/bin/xdg-open ] && chmod 0755 config/includes.chroot/usr/local/bin/xdg-open
# Coklu UYAP ikilisi (yukaridaki genel 0644 x bitini siler; kanca -x sinamasi icin, denetci 23.11 K1)
[ -f config/includes.chroot/opt/armonia/cokluuyap/uyap_app ] && chmod 0755 config/includes.chroot/opt/armonia/cokluuyap/uyap_app
# Armonia Yazi ikilileri (ayni neden)
for f in armonia-yazi armonia-imza; do
  if [ -f "config/includes.chroot/opt/armonia/yazi/$f" ]; then chmod 0755 "config/includes.chroot/opt/armonia/yazi/$f"; fi
done
BAS=$(date +%s)
set +e; lb build >"$IS/build.log" 2>&1; RC=$?; set -e
echo "lb build cikis: $RC  sure: $(( $(date +%s) - BAS )) sn"
grep -nE "^E:|^P: Begin|error|UYARI" "$IS/build.log" | grep -viE "liberror|error-perl|errors=" | tail -15
[ $RC -eq 0 ] || exit $RC
# UEFI: EFI ikilisi, cekirdek/initrd yollari ve kilitli GRUB menusu ISO'da olmali (23.11)
for f in binary/live/vmlinuz binary/live/initrd.img binary/boot/grub/grub.cfg; do
  [ -e "$f" ] || { echo "HATA: UEFI acilisi icin $f yok"; exit 9; }
done
# Kilit: superusers var, hicbir password satiri yok, tek girdi ve o --unrestricted (denetci 23.11 DUSUK-1)
GC=binary/boot/grub/grub.cfg
grep -q '^set superusers=' "$GC" && grep -q -- '--unrestricted' "$GC" && ! grep -qE '^[[:space:]]*password' "$GC" \
  && [ "$(grep -cE '^[[:space:]]*menuentry' "$GC")" = "1" ] || { echo "HATA: GRUB menusu kilitli degil"; exit 9; }
# Ara yapilandirmalar yalniz bizim grub.cfg'ye zincirlenmeli (kilidi atlatan ikinci menu olmasin)
for f in binary/boot/grub/loopback.cfg binary/boot/grub/x86_64-efi/grub.cfg; do
  [ -f "$f" ] || continue
  grep -qE '^[[:space:]]*menuentry' "$f" && { echo "HATA: $f kendi menusunu tasiyor"; exit 9; }
done
[ -n "$(find binary -ipath 'binary/efi/boot/bootx64.efi')" ] || { echo "HATA: UEFI acilisi icin EFI/boot/bootx64.efi yok"; exit 9; }
# USB'den UEFI acilisinda firmware cogu zaman ISO9660'taki EFI/ dizinini degil, gomulu FAT imajini (efi.img,
# El Torito EFI girdisi) kullanir (denetci 23.11 ORTA-3)
[ -f binary/boot/grub/efi.img ] || { echo "HATA: UEFI icin binary/boot/grub/efi.img yok"; exit 9; }
if command -v mdir >/dev/null; then
  EFI_IC="$(mdir -i binary/boot/grub/efi.img ::EFI/boot 2>/dev/null || true)"
  grep -qi 'bootx64' <<<"$EFI_IC" || { echo "HATA: efi.img icinde EFI/boot/bootx64.efi yok"; exit 9; }
else
  echo "UYARI: mtools yok; efi.img icerigi denetlenmedi (apt install mtools)"
fi
# Secure Boot: live-build imzali shim + GRUB'u kurabildiyse bunu gunluge yazar. Yalniz grubx64.efi'nin varligi
# yetmez: imzasiz gcdx64 de ayni adla gelebilir (denetci 23.11 ORTA-2).
if grep -q 'UEFI Secure Boot support enabled' "$IS/build.log"; then
  echo "UEFI: imzali shim + GRUB (Secure Boot acik makinede de acilmali)"
else
  echo "UYARI: imzali Secure Boot destegi yok; Secure Boot acik makinede acilmaz (BIOS'ta Secure Boot kapatilmali)"
fi
LISTE="$(unsquashfs -l binary/live/filesystem.squashfs 2>/dev/null || true)"
# Uretim ISO'sunda test bileseni olmamali (guvenlik-denetci O4). LISTE uzerinden: unsquashfs | grep -q boru
# hatti pipefail altinda SIGPIPE ile yanlis negatif veriyordu (denetci 23.11 ORTA-4)
if [ "$TEST" != "1" ] && grep -qE 'kasa_test_kur|armonia-kasa-test|lynis_test|/usr/sbin/lynis|90-test-seri|kasa_test_dogrula|tus_kaydi|/usr/bin/evtest' <<<"$LISTE"; then
  echo "HATA: uretim ISO'sunda test bileseni bulundu"; exit 3
fi
# Windows katmani: WINDOWS=1'de kum havuzu + 32 bit Wine olmali; WINDOWS=0'da Wine hic olmamali
if [ "$WINDOWS" = "1" ]; then
  for g in 'usr/bin/bwrap$' 'usr/bin/wine$' 'i386-windows' 'opt/armonia/bin/windows_calistir.sh$'; do
    grep -qE "$g" <<<"$LISTE" || { echo "HATA: Windows katmaninda $g yok"; exit 6; }
  done
else
  grep -qE 'usr/bin/wine$|i386-linux-gnu' <<<"$LISTE" && { echo "HATA: WINDOWS=0 ama imajda Wine/i386 var"; exit 6; }
fi
# Donanim yazilimlari (23.11): Wi-Fi, AMD ekran karti, Intel mikrokodu imajda olmali
for g in 'lib/firmware/(intel/)?iwlwifi' 'lib/firmware/rtw' 'lib/firmware/amdgpu/' 'lib/firmware/intel-ucode/'; do
  grep -qE "$g" <<<"$LISTE" || { echo "HATA: donanim yazilimi eksik: $g"; exit 8; }
done
echo "Donanim yazilimi dosyasi: $(grep -c 'lib/firmware/' <<<"$LISTE")"
# Acilista Rust kodu calismaz (kullanici kararlari 19.11 ve 23.11): eski kiosk bileseni hic olmamali, Coklu UYAP
# birimi hicbir .wants'ta olmamali. COKLUUYAP=1'de uygulama menuden acilmak uzere imajda olmali, 0'da hic olmamali.
grep -qE 'armonia-kiosk\.service|kiosk_oturum|\.wants/armonia-cokluuyap' <<<"$LISTE" \
  && { echo "HATA: imajda acilista baslayan Coklu UYAP bileseni bulundu"; exit 5; }
if [ "$COKLUUYAP" = "1" ]; then
  for g in 'opt/armonia/cokluuyap/uyap_app$' 'armonia-cokluuyap\.service$' 'cokluuyap_ac\.sh$' '60-armonia-cokluuyap\.rules$'; do
    grep -qE "$g" <<<"$LISTE" || { echo "HATA: Coklu UYAP katmaninda $g yok"; exit 5; }
  done
else
  grep -qE 'opt/armonia/cokluuyap' <<<"$LISTE" && { echo "HATA: COKLUUYAP=0 ama imajda Coklu UYAP var"; exit 5; }
fi
# Armonia Yazi (24.11): uygulama, imza hizmeti, soketler ve kanca ciktisi
if [ "$YAZI" = "1" ]; then
  for g in 'opt/armonia/yazi/armonia-yazi$' 'opt/armonia/yazi/armonia-imza$' 'armonia-yazi@\.service$' 'armonia-imza\.service$' \
           'sockets\.target\.wants/armonia-imza\.socket$' 'sockets\.target\.wants/armonia-yazi\.socket$' 'armonia-yazi@\.service\.d/20-calisma\.conf$' \
           'armonia-tarayici@\.service\.d/40-imza-soketi\.conf$' 'armonia-cokluuyap\.service\.d/40-imza-soketi\.conf$' \
           'usr/lib/x86_64-linux-gnu/libwebkit2gtk-4\.1\.so\.0$'; do
    grep -qE "$g" <<<"$LISTE" || { echo "HATA: Armonia Yazi katmaninda $g yok"; exit 5; }
  done
else
  grep -qE 'opt/armonia/yazi|libwebkit2gtk-4\.1' <<<"$LISTE" && { echo "HATA: YAZI=0 ama imajda Armonia Yazi/WebKitGTK var"; exit 5; }
fi
# DNS: live-build resolv.conf'u bos birakir; acilista tmpfiles ile resolved stub'ina baglanmali.
# Icerik denetlenir (guvenlik-denetci D2). Boru + grep -q KULLANMA: pipefail altinda unsquashfs SIGPIPE alir
# ve dosya varken de hata verir (2026-11-14 ilk deneme).
RESOLV_SATIR="$(unsquashfs -cat binary/live/filesystem.squashfs etc/tmpfiles.d/armonia-resolv.conf 2>/dev/null || true)"
grep -q '^L+ /etc/resolv.conf .*stub-resolv.conf' <<<"$RESOLV_SATIR" \
  || { echo "HATA: armonia-resolv.conf (DNS) imajda yok veya bozuk"; exit 4; }
if [ "$TEST" = "1" ]; then HEDEF="$ISO_DIZIN/armonia_test_kasa.iso"; else HEDEF="$ISO_DIZIN/armonia.iso"; fi
[ "${OLCUM:-0}" = "1" ] && HEDEF="$ISO_DIZIN/armonia_olcum.iso"
[ "${OLCUM_TARAYICI:-0}" = "1" ] && HEDEF="$ISO_DIZIN/armonia_tarayici_olcum.iso"
[ "${SERI:-0}" = "1" ] && HEDEF="$ISO_DIZIN/armonia_seri.iso"
# WSL bellegi ~2,9 GB: /mnt/c'ye (9p) cp sayfa onbellegini sisirip "Cannot allocate memory" verdi (14 Kasim 12:08)
sync; echo 3 > /proc/sys/vm/drop_caches || true
rm -f "$HEDEF"
dd if=live-image-amd64.hybrid.iso of="$HEDEF" bs=4M oflag=direct status=none
ls -la "$HEDEF"; sha256sum "$HEDEF"
echo "--- squashfs:"; ls -la binary/live/filesystem.squashfs
