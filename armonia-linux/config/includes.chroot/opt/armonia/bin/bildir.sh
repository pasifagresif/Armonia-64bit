#!/bin/sh
# Root servislerinden masaustune bildirim (armonia kullanicisi olarak, setpriv ile). bildir.sh baslik metin
# Claude, 2026-11-19.
U=$(id -u armonia) || exit 0
exec setpriv --reuid=armonia --regid=armonia --init-groups --reset-env \
  env DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$U/bus" \
  notify-send -a "Armonia" "$1" "$2"
