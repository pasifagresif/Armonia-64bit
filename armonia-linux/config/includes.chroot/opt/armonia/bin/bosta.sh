#!/bin/sh
# Bostayken ekran kapatma / kilit. Claude, 2026-11-19.
# Canli sistemde kullanicinin parolasi yok -> kilit acilamaz; yalniz ekran kapatilir.
# Kurulu sistemde kilit suresi /etc/armonia/ayarlar.conf KILIT_DK (Ayarlar; 5/10/15/30, varsayilan 10).
if [ -d /run/live/medium ] || [ -e /etc/armonia/deneme ]; then
  exec swayidle -w timeout 900 '/opt/armonia/bin/ekran.sh kapat' resume '/opt/armonia/bin/ekran.sh ac'
fi
DK=$(sed -n 's/^KILIT_DK=\([0-9]*\)$/\1/p' /etc/armonia/ayarlar.conf 2>/dev/null)
case "$DK" in 5|10|15|30) ;; *) DK=10 ;; esac
SN=$((DK * 60))
exec swayidle -w \
  timeout "$SN" 'swaylock -f -c 2F2F2F' \
  timeout "$((SN + 60))" '/opt/armonia/bin/ekran.sh kapat' resume '/opt/armonia/bin/ekran.sh ac' \
  before-sleep 'swaylock -f -c 2F2F2F'
