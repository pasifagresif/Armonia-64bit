#!/bin/sh
# Ekran kilidi. Canli sistemde kullanicinin parolasi yok -> kilit acilamazdi; yalniz uyari. Claude, 2026-11-19.
if [ -d /run/live/medium ] || [ -e /etc/armonia/deneme ]; then
  notify-send -a Armonia "Ekran kilidi" "Deneme (canlı) sisteminde ekran kilidi yok. Diske kurduktan sonra çalışır."
  exit 0
fi
exec swaylock -f -c 2F2F2F
