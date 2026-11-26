#!/bin/sh
# Menü / dock / dosya yöneticisi (çift tık) "Armonia Yazı" kısayolu. Uygulamayı BURADA başlatmaz: belgenin yolunu
# denetleyip root'un açtığı Unix soketine (0600 armonia) yazar; kum havuzlu armonia-yazi@.service örneği yolu
# YENİDEN denetleyip pencereyi açar. Argümansız: yeni belge. Claude, 2026-11-24.
#   yazi_ac.sh [belge.udf|belge.docx]
set -u
bildir() { notify-send -a Armonia "Armonia Yazı" "$1" 2>/dev/null; echo "Armonia Yazı: $1" >&2; }
[ -S /run/armonia-yazi.sock ] || { bildir "Bu sistemde Armonia Yazı kurulu değil."; exit 1; }
YOL=""
if [ $# -ge 1 ] && [ -n "$1" ]; then
  # file:// adresi gelirse (bazı dosya yöneticileri) yola çevrilir; yüzde kodlu adlar desteklenmez
  case "$1" in file://*) set -- "${1#file://}" ;; esac
  YOL=$(realpath -e -- "$1" 2>/dev/null) || { bildir "Dosya bulunamadı."; exit 1; }
  [ -f "$YOL" ] || { bildir "Yalnız belge dosyaları açılır."; exit 1; }
  case "$YOL" in
    *"
"*) bildir "Dosya adı geçersiz."; exit 1 ;;
  esac
  case "$YOL" in
    /home/armonia/Belgeler/*|/home/armonia/İndirilenler/*) ;;
    *) bildir "Armonia Yazı yalnız Belgeler ve İndirilenler klasörlerindeki belgeleri açar. Belgeyi önce oraya kopyalayın."; exit 1 ;;
  esac
  case "$YOL" in
    *.[uU][dD][fF]|*.[dD][oO][cC][xX]) ;;
    *) bildir "Yalnız UDF ve Word (DOCX) belgeleri açılır."; exit 1 ;;
  esac
fi
# -w 2: Accept=yes'te systemd bağlantıyı pencere kapanana dek açık tutar; nc 2 sn boşta kalınca çıkar
printf '%s\n' "$YOL" | /usr/bin/nc.openbsd -N -w 2 -U /run/armonia-yazi.sock
DURUM=$?
# Armonia Yazı zaten açıksa belge o pencereye SEKME olarak devredilir (26.11) ama Wayland'da uygulama kendi
# penceresini öne alamaz: pencereyi buradan (masaüstü oturumundan) öne getiririz. Pencere yeni açılıyorsa zaten
# öndedir; henüz yoksa komut hiçbir şey yapmaz.
[ -x /opt/armonia/uygulamalar/armonia-one-getir ] && /opt/armonia/uygulamalar/armonia-one-getir "Armonia Yazı" >/dev/null 2>&1
exit "$DURUM"
