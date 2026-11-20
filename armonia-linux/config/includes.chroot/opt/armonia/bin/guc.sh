#!/bin/sh
# Ust cubuktaki guc dugmesi: Kilitle / Yeniden baslat / Kapat. Yetki: logind'in "etkin yerel oturum" kurali
# (polkit varsayilani); kullanicinin sudo yetkisi yok. Claude, 2026-11-19.
pkill -x fuzzel && exit 0
SECIM=$(printf 'Kilitle\nYeniden başlat\nKapat\n' | fuzzel --config /etc/armonia/fuzzel.ini --dmenu --lines 3 --prompt 'Güç  ')
case "$SECIM" in
  Kilitle) exec /opt/armonia/bin/kilitle.sh ;;
  "Yeniden başlat") exec systemctl reboot ;;
  Kapat) exec systemctl poweroff ;;
esac
