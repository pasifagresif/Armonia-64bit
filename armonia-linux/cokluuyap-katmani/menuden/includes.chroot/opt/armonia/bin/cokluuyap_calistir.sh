#!/bin/sh
# armonia-cokluuyap.service icinde (kum havuzu, kullanici 'armonia') uyap_app'i calistirir. Claude, 2026-11-23.
set -u
R="/run/user/$(id -u)"
[ -S "$R/wayland-0" ] || { echo "cokluuyap: masaustu oturumu yok"; exit 0; }
export XDG_RUNTIME_DIR="$R" WAYLAND_DISPLAY=wayland-0
# Xwayland'e gitmesin; uygulama Wayland'e dogrudan baglanir
unset DISPLAY
cd "$UYAP_DATA_DIR" || exit 1
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME"
# stdin bos: uygulama konsoldan okumaya kalkarsa sonsuza dek beklemesin (14.11 sertifika secimi hatasi)
# Oturum kapaninca oturum.sh birimi durdurur (yeniden deneme de iptal olur; denetci 23.11 O2)
exec /opt/armonia/cokluuyap/uyap_app </dev/null
