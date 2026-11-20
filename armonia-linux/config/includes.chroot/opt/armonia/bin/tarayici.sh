#!/bin/sh
# armonia-tarayici@.service: soketten tek satir URL okur, yeniden denetler, Chromium'u acar ya da acik
# pencereye iletir. Claude, 2026-11-14 (guvenlik-denetci O1, O2, O4, D2, D4 duzeltmeleri).
# 2026-11-19: kiosk -> masaustu. Normal pencere (sekme, adres cubugu, yer imleri); hangi sitelere gidilecegine
# /etc/chromium/policies/managed/armonia.json (URLAllowlist: adli/resmi siteler) karar verir.
# D2: satir sonu gondermeyen istemci ornegi askida tutmasin
URL=$(timeout 3 head -n 1) || exit 1
case "$URL" in
  https://*|http://*) ;;
  *) echo "tarayici: izin verilmeyen adres reddedildi" >&2; exit 1 ;;
esac
case "$URL" in *[[:space:]]*) echo "tarayici: gecersiz adres" >&2; exit 1 ;; esac
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
# labwc Wayland soketi: birim yalniz wayland-0 dosyasini baglar (20-calisma.conf, guvenlik-denetci EK-Y1)
WAYLAND_DISPLAY=wayland-0; [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] || WAYLAND_DISPLAY=
[ -n "${WAYLAND_DISPLAY:-}" ] || { echo "tarayici: Wayland oturumu yok" >&2; exit 1; }
# HOME ve XDG dizinleri yazilabilir profil dizinine: kiosk ev dizini bu birimde salt okunur (.pki/nssdb, onbellek)
P=/home/armonia/.local/share/armonia-tarayici
# O1: her soket ornegi kendi PrivateTmp'sinde; tekil-ornek soketi ortak dizinde olmali
export WAYLAND_DISPLAY HOME="$P" XDG_CONFIG_HOME="$P/.config" XDG_CACHE_HOME="$P/.cache" TMPDIR="$P/tmp"
mkdir -p "$TMPDIR" "$P/profil/Default"
# O4: Chromium kendi cercevesini (kucult dugmeli) cizmesin, labwc'nin yalniz-kapat basligi kullanilsin
[ -f "$P/profil/Default/Preferences" ] || printf '{"browser":{"custom_chrome_frame":false}}\n' > "$P/profil/Default/Preferences"
unset DISPLAY
# O2: baglanti soketi Chromium'a devredilmesin (xdg-open/nc hemen biter, baglanti yuvasi bosalir)
exec </dev/null
# systemd baglantiyi ayrica fd 3 olarak da verir (LISTEN_FDS): Chromium'a devredilmesin. (Baglantinin kendisini
# PID 1 birim bitene dek tutar; istemci tarafi xdg-open'da nc -w ile cikar.)
exec 3<&-
unset LISTEN_FDS LISTEN_PID LISTEN_FDNAMES
# --password-store=basic: gnome-keyring/kwallet aranmaz. --lang=tr (D4). Politika: /etc/chromium/policies/managed/
exec /usr/bin/chromium --ozone-platform=wayland --no-first-run --noerrdialogs --no-default-browser-check \
  --disable-translate --password-store=basic --start-maximized --lang=tr \
  --user-data-dir="$P/profil" "$URL"
