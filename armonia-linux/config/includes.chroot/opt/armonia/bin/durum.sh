#!/bin/sh
# Ust cubuk hap gostergeleri (waybar, JSON). Claude, 2026-11-19.
#   kip     : canli (deneme) sistemde "Deneme sistemi" hapi; kurulu sistemde gizli
#   klavye  : TR-Q / TR-F (/etc/armonia/ayarlar.conf, Ayarlar yazar)
case "$1" in
  kip)
    if [ -d /run/live/medium ] || [ -e /etc/armonia/deneme ]; then
      echo '{"text":"● Deneme sistemi (canlı)","class":"canli","tooltip":"USB bellekten çalışıyor; kapatınca değişiklikler silinir. Kalıcı kullanım için diske kurun."}'
    else echo '{"text":""}'; fi ;;
  klavye)
    v=$(sed -n 's/^KLAVYE=//p' /etc/armonia/ayarlar.conf 2>/dev/null)
    [ "$v" = "f" ] && echo '{"text":"TR-F","tooltip":"Klavye: Türkçe F"}' || echo '{"text":"TR-Q","tooltip":"Klavye: Türkçe Q"}' ;;
esac
