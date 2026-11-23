#!/bin/sh
# Armonia Pi: kok dosya sisteminin bulundugu USB bellege USBGuard izni (G6, 23.11). Claude, 2026-11-23.
# Sorun (20.11): PresentDevicePolicy=apply-policy acilista takili kok diski (sinif 08) bekletiyor, sistem kendi
# diskini kaybediyordu; bu yuzden Pi'de USBGuard kapatilmisti. Cozum: usbguard'dan ONCE, kok diskin kendisi icin
# dar bir izin kurali uretilir. Kural aygitin tanimlayici ozetini (hash), kimligini, seri numarasini ve arayuzlerini
# icerir; ayni modelden baska bir bellek ya da gizli klavye eklenmis bir kopya bu kurala uymaz.
# Her acilista calisir (bellek degisirse kural yenilenir); kok USB'de degilse (SD kart) hicbir sey yapmaz.
set -eu
KURALLAR=/etc/usbguard/rules.conf
ETIKET='# armonia-kok-disk (otomatik, usb_kok_izin.sh)'
kaydet() { echo "armonia-usb-kok: $1"; }

KAYNAK=$(findmnt -n -o SOURCE /)
DISK=$(lsblk -snpo NAME,TYPE "$KAYNAK" | awk '$2=="disk"{print $1; exit}')
[ -n "$DISK" ] || { kaydet "kok diski bulunamadi ($KAYNAK)"; exit 1; }
# sysfs'te diskten yukari cik: idVendor olan ilk dizin USB aygitidir
d=$(readlink -f "/sys/class/block/$(basename "$DISK")")
while [ "$d" != "/" ] && [ ! -f "$d/idVendor" ]; do d=$(dirname "$d"); done
[ -f "$d/idVendor" ] || { kaydet "kok ($DISK) USB'de degil; kural gerekmez"; exit 0; }
VP="$(cat "$d/idVendor"):$(cat "$d/idProduct")"
SERI=$(cat "$d/serial" 2>/dev/null || true)
PORT=$(basename "$d")

# usbguard generate-policy servis olmadan sysfs'ten okur. Kok aygitin satiri: kimlik + seri (seri yoksa yuva) ile
# secilir; ayni modelden ikinci bir bellegin satiri secilmesin (denetci G6-c).
# -p: butun satirlara yuva eklenir ve HER ZAMAN yuvayla suzulur; ayni kimlik/seriyi taklit eden ikinci bir aygitin
# satiri secilemez (denetci D-2). Kural her acilista yeniden uretildigi icin yuvaya baglanmasi sorun degil.
SATIR=$(usbguard generate-policy -p 2>/dev/null | grep -F " id $VP " | grep -F "via-port \"$PORT\"" \
  | { if [ -n "$SERI" ]; then grep -F "serial \"$SERI\""; else cat; fi; } | head -n1) || true
# Arayuzler yalniz sinif kalibiyla ayiklanir: kuralda with-interface'ten sonra with-connect-type / label gelebilir
# (denetci 23.11 A-1: onlari arayuz saymak usbguard'i hic baslatmiyor, tum USB'yi aciyordu).
IF=$(printf '%s' "$SATIR" | sed -n 's/.*with-interface //p' | grep -oE '[0-9a-f*]{2}:[0-9a-f*]{2}:[0-9a-f*]{2}' | tr '\n' ' ') || true
case "$SATIR" in allow\ *) ;; *) SATIR="" ;; esac
printf '%s' "$SATIR" | grep -q ' hash "' || SATIR=""
if [ -n "$SATIR" ]; then
  # Kok disk, sistemin AÇILDIGI aygittir (isletim sisteminin kendisi); ona izin vermemek sistemi dusurur ya da
  # usbguard'i hic baslatmayip tum USB'yi acar. Bu yuzden tam kurala (ozet + kimlik + arayuzler) her durumda izin
  # verilir; yigin depolama disi arayuz varsa yalniz uyari yazilir.
  for a in $IF; do
    case "$a" in 08:*) ;; *) kaydet "UYARI: kok aygit yigin depolama disi arayuz da bildiriyor ($IF); yalniz bu tam kurala izin"; break ;; esac
  done
else
  # generate-policy satir vermedi: usbguard'in yine de baslayabilmesi icin sysfs'ten dar yedek kural (yuva + kimlik +
  # YALNIZ yigin depolama). Yoksa Requires= yuzunden usbguard hic baslamaz ve TUM USB serbest kalirdi (G6-c).
  SATIR="allow id $VP via-port \"$PORT\" with-interface match-all { 08:*:* }"
  kaydet "UYARI: generate-policy kok aygit satirini vermedi; yedek kural: $SATIR"
fi
[ -n "$SERI" ] || kaydet "UYARI: bellegin seri numarasi yok; kural USB yuvasina bagli (bellegi ayni yuvaya takin)"

# Eski otomatik kural (etiket + bir sonraki satir) kaldirilip yenisi en uste (ilk eslesen kazanir) yazilir
YENI=$(mktemp /etc/usbguard/.rules.XXXXXX)
{ printf '%s\n%s\n' "$ETIKET" "$SATIR"; sed "/^# armonia-kok-disk /{N;d}" "$KURALLAR"; } > "$YENI"
grep -q '^# Armonia kurallari sonu$' "$YENI" || { rm -f "$YENI"; kaydet "kural dosyasi beklenen bicimde degil"; exit 1; }
chmod 0600 "$YENI"; chown root:root "$YENI"
mv -f "$YENI" "$KURALLAR"
kaydet "kok disk izni: $VP${SERI:+ seri=$SERI}"
