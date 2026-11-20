#!/bin/sh
# Sonradan takilan aygit icin surucu (kullanici karari 19.11: "sonradan eklenen cihazlar icin onay verirse surucu
# indirilsin"; surucu paketleri imaja konmaz). armonia-surucu@<vid>-<pid>[-yazici].service icinde root calisir.
# 1. Yazici ise once surucusuz yol: ipp-usb (IPP-over-USB, yalniz localhost) -> CUPS "everywhere" kuyrugu.
# 2. Olmazsa / yazici degilse: isenkram-lookup aygitin modalias'lariyla paket onerir; izin listesindeki paketler
#    icin kullaniciya boyutuyla sorulur, onaylarsa Debian deposundan (imzali) kurulur.
# Canli sistemde kurulan paket yeniden baslatinca gider (kalici kurulum A6'dan sonra). Claude, 2026-11-19.
set -u
ORNEK=${1:-}
case "$ORNEK" in
  [0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f]|[0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f]-yazici) ;;
  *) echo "gecersiz ornek: $ORNEK"; exit 2 ;;
esac
VID=$(echo "$ORNEK" | cut -d- -f1); PID=$(echo "$ORNEK" | cut -d- -f2)
YAZICI=""; case "$ORNEK" in *-yazici) YAZICI=1 ;; esac
B=/opt/armonia/bin
gunluk() { logger -t armonia-surucu "$ORNEK: $*"; echo "$*"; }

# Aygitin sysfs dizini ve modalias'lari (ilk eslesen; ayni modelden iki aygit ayni surucuyu kullanir)
AYGIT=""
for d in /sys/bus/usb/devices/*; do
  [ -f "$d/idVendor" ] || continue
  [ "$(cat "$d/idVendor")" = "$VID" ] && [ "$(cat "$d/idProduct")" = "$PID" ] && { AYGIT=$d; break; }
done
[ -n "$AYGIT" ] || { gunluk "aygit bulunamadi (cikarilmis olabilir)"; exit 0; }
URETICI=$(tr -cd 'A-Za-z0-9 ._-' < "$AYGIT/manufacturer" 2>/dev/null | cut -c1-30)
MODEL=$(tr -cd 'A-Za-z0-9 ._-' < "$AYGIT/product" 2>/dev/null | cut -c1-40)
AD=$(echo "$URETICI $MODEL" | sed 's/^ *//; s/ *$//'); [ -n "$AD" ] || AD="$VID:$PID"
KUYRUK=$(echo "$AD" | tr -c 'A-Za-z0-9\n' '_' | cut -c1-40)

kuyruk_kur() {  # $1 URI, $2 model (-m)
  lpadmin -p "$KUYRUK" -E -v "$1" -m "$2" -D "$AD" || return 1
  lpstat -d 2>/dev/null | grep -q ':' || lpadmin -d "$KUYRUK"
  gunluk "yazici kuruldu: $1 ($2)"
  $B/bildir.sh "Yazıcı hazır" "$AD yazdırmaya hazır."
}

if [ -n "$YAZICI" ]; then
  if lpstat -v 2>/dev/null | grep -q "^device for $KUYRUK:"; then gunluk "kuyruk zaten var"; exit 0; fi
  # 1. Surucusuz: ipp-usb aygiti 60000'den baslayan bir yerel kapida sunar (birkac saniye surebilir)
  i=0
  while [ $i -lt 10 ]; do
    for p in $(seq 60000 60015); do
      U="ipp://localhost:$p/ipp/print"
      lpstat -v 2>/dev/null | grep -q " $U\$" && continue
      if ipptool -q -T 3 "$U" get-printer-attributes.test 2>/dev/null; then
        kuyruk_kur "$U" everywhere && exit 0
      fi
    done
    i=$((i + 1)); sleep 2
  done
  gunluk "surucusuz yol yok, surucu aranacak"
fi

# 2. Surucu paketi. isenkram-lookup apt listelerindeki Modaliases alanlarini kullanir: canli sistemde listeler
#    bossa once guncelle (denetim D6). apt ayni anda tek ornekte (denetim D7).
exec 8>/run/lock/armonia-surucu.lock; flock 8
if ! ls /var/lib/apt/lists/*_Packages >/dev/null 2>&1; then timeout 120 apt-get update -qq >/dev/null 2>&1 || true; fi
MODALIAS=$(cat "$AYGIT/modalias" "$AYGIT"/*:*/modalias 2>/dev/null | sort -u)
PAKETLER=$(isenkram-lookup $MODALIAS 2>/dev/null \
  | grep -E '^(printer-driver-[a-z0-9.+-]+|hplip|foo2zjs|splix|firmware-[a-z0-9.+-]+|sane-airscan|libsane1)$' | tr '\n' ' ')
if [ -z "$PAKETLER" ] && [ -n "$YAZICI" ]; then
  # isenkram bilmiyorsa ureticiye gore yedek (DOGRULANACAK: gercek yazicilarla)
  case "$VID" in
    03f0) PAKETLER="hplip" ;;
    04b8) PAKETLER="printer-driver-escpr" ;;
    04f9) PAKETLER="printer-driver-brlaser" ;;
    04e8) PAKETLER="printer-driver-splix" ;;
    *) PAKETLER="printer-driver-gutenprint" ;;
  esac
fi
PAKETLER=$(echo $PAKETLER)
[ -n "$PAKETLER" ] || { gunluk "surucu gerekmiyor"; exit 0; }
# Zaten kuruluysa sorma
EKSIK=""
for p in $PAKETLER; do dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q "install ok installed" || EKSIK="$EKSIK $p"; done
EKSIK=$(echo $EKSIK)
[ -n "$EKSIK" ] || { gunluk "surucu kurulu: $PAKETLER"; [ -n "$YAZICI" ] || exit 0; }

if [ -n "$EKSIK" ]; then
  if ! timeout 120 apt-get update -qq >/dev/null 2>&1; then
    $B/bildir.sh "Sürücü indirilemedi" "$AD için sürücü gerekiyor ama internete bağlanılamadı."
    gunluk "apt-get update basarisiz"; exit 0
  fi
  BOYUT=$(apt-get install --print-uris -qq --no-install-recommends $EKSIK 2>/dev/null \
    | awk '{t += $3} END {printf "%.0f", t / 1048576}')
  U=$(id -u armonia) || exit 0
  # Cevap root'a ait 0600 dosyaya (boru degil; bkz. usb_sor.sh guven siniri notu)
  C=$(mktemp); trap 'rm -f "$C"' EXIT
  timeout -k 5 75 setpriv --reuid=armonia --regid=armonia --init-groups --reset-env \
    env XDG_RUNTIME_DIR="/run/user/$U" $B/soru.sh "Sürücü gerekiyor" \
    "$AD için $EKSIK paketi (yaklaşık $BOYUT MB) indirilecek." "Sürücü ($BOYUT MB): $AD" \
    "İndirme" "İndir ve kur" >"$C" 2>/dev/null 8>&-
  SECIM=$(head -n1 "$C")
  [ "$SECIM" = "İndir ve kur" ] || { gunluk "kullanici indirmedi: $EKSIK"; exit 0; }
  $B/bildir.sh "Sürücü indiriliyor" "$EKSIK ($BOYUT MB)"
  if ! DEBIAN_FRONTEND=noninteractive timeout 900 apt-get install -y -qq --no-install-recommends $EKSIK; then
    $B/bildir.sh "Sürücü kurulamadı" "$AD — ayrıntı sistem günlüğünde."
    gunluk "apt-get install basarisiz: $EKSIK"; exit 1
  fi
  gunluk "kuruldu: $EKSIK"
fi

if [ -n "$YAZICI" ]; then
  sleep 2
  # Iki yazici takiliysa dogru URI: once bu aygitin seri numarasini iceren (denetim D5)
  SERI=$(tr -cd 'A-Za-z0-9' < "$AYGIT/serial" 2>/dev/null)
  URILER=$(lpinfo -v 2>/dev/null | awk '$1 == "direct" && $2 ~ /^(usb|hp):/ {print $2}')
  URI=""; [ -n "$SERI" ] && URI=$(echo "$URILER" | grep -F "serial=$SERI" | head -1)
  [ -n "$URI" ] || URI=$(echo "$URILER" | head -1)
  PPD=$(lpinfo -m --make-and-model "$MODEL" 2>/dev/null | head -1 | cut -d' ' -f1)
  if [ -n "$URI" ] && [ -n "$PPD" ] && kuyruk_kur "$URI" "$PPD"; then exit 0; fi
  $B/bildir.sh "Sürücü kuruldu" "$AD otomatik eklenemedi; Ayarlar'dan yazıcı ekleyin."
  gunluk "kuyruk kurulamadi: uri=[$URI] ppd=[$PPD]"
else
  $B/bildir.sh "Sürücü kuruldu" "$AD için sürücü kuruldu. Çalışmazsa aygıtı çıkarıp yeniden takın."
fi
