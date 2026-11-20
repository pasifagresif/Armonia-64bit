#!/bin/sh
# Root servislerinin (USB, surucu) masaustunde soru sormasi icin; ARMONIA KULLANICISI olarak calistirilir
# (setpriv). Kullanim: soru.sh "bildirim basligi" "bildirim metni" "istem" secenek1 [secenek2 ...]
# Secilen secenegi standart ciktiya yazar; 60 sn cevap yoksa bos. Ilk secenek Enter'la secilen olur: cagiran,
# zararsiz olani (Reddet / Indirme) ilk siraya koyar. Metinleri guvenli karakterlere indirmek cagiranin isidir.
# Claude, 2026-11-19.
set -u
[ $# -ge 4 ] || exit 2
[ -n "${XDG_RUNTIME_DIR:-}" ] || exit 1
W=""
for s in "$XDG_RUNTIME_DIR"/wayland-[0-9]*; do
  case "$s" in *.lock) continue ;; esac
  [ -S "$s" ] && { W=${s##*/}; break; }
done
[ -n "$W" ] || exit 1   # oturum yok (giris ekrani)
export WAYLAND_DISPLAY="$W" HOME=/home/armonia LANG=tr_TR.UTF-8 \
  DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
BASLIK=$1 METIN=$2 ISTEM=$3
shift 3
notify-send -a "Armonia" "$BASLIK" "$METIN" 2>/dev/null
# Bulanik eslesme kapali: yazi yazarken gelen soruda rastgele harfler + Enter "izin ver"i secmesin (denetim O2).
# Yazilan metin secenek degilse fuzzel onu dondurur; cagiran yalniz tam secenek metnini kabul eder.
ESLESME=""
fuzzel --help 2>&1 | grep -q -- '--match-mode' && ESLESME="--match-mode=exact"
printf '%s\n' "$@" | timeout 60 fuzzel --config /etc/armonia/fuzzel.ini --dmenu --lines $# --width 60 \
  $ESLESME --prompt "$ISTEM › "
