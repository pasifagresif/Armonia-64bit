#!/bin/sh
# labwc icinde CokluUYAP'i calistirir; kapanirsa yeniden acar. Claude, 2026-11-13.
# DISPLAY kaldirilir: uygulama Wayland'e dogrudan baglanir, Xwayland hic baslamaz.
unset DISPLAY
cd /var/lib/armonia/cokluuyap || exit 1
# Tek ornek: ikinci oturum.sh hemen cikar (cift "Kontrol Ekrani" penceresi; ayni accounts.json/zs.sqlite3'e
# es zamanli yazma ve 8800 cakismasi). guvenlik-denetci D4. Kalici cozum: uyap_app icinde flock.
exec 9>"${XDG_RUNTIME_DIR:-/tmp}/armonia-oturum.lock"
flock -n 9 || exit 0
# stdin tty1 olmasin: uygulama icinde konsoldan okuma (eski sertifika secimi) sonsuza dek bekliyordu (14 Kasim)
while :; do
  /opt/armonia/cokluuyap/uyap_app </dev/null
  sleep 1
done
