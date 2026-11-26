#!/bin/sh
# Agdaki bir Windows programini ($1 = uygulama kimligi) Wine ile, AGSIZ bubblewrap kum havuzunda ve AYRI bir
# Wayland kafesinde (cage) acar. 'armonia' kullanicisi calistirir (dock / menu kisayolu, windows_hazirla.sh
# tarafindan ureteilen .desktop/dock girdisinden $1 ile cagrilir). Kullanici karari 23.11: "Armonia agdaki programin
# oldugu klasore erissin ama aga kapali olsun." Claude, 2026-11-23 (tek uygulama, "Icramatik"); 2026-11-25
# (coklu uygulama: hicbir program adi koda gomulu degil, hepsi $1 ile windows.apps.d/$1.conf'tan okunur).
#
# Iki katman (guvenlik-denetci 23.11 B-1):
#  1. cage (ic ice Wayland birlestiricisi): Wine ana masaustunun Wayland soketini HIC gormez. Sanal klavye/isaretci,
#     pano okuma (data-control) ve ekran goruntusu (screencopy) yalniz cage'in kendi icinde kalir: Wine baska
#     pencerelere tus gonderemez, panoyu ve UYAP ekranini okuyamaz.
#  2. bubblewrap: ag yok (yalniz lo), ev dizininin geri kalani / D-Bus / systemd --user / /dev/input / pcscd / GPU yok,
#     yeni kullanici ad alani acilamaz (--disable-userns), tum yetkiler dusurulur.
# Gorunenler: /usr (salt okunur), Wine oneki (uygulamaya ozel), ~/Belgeler/Windows, bu uygulamanin paylasim
# klasoru, cage'in soketi.
# Bilinen sinir: paylasim parolasiz ve LAN'daki herkes yazabilir -> EXE degistirilebilir. SHA256 ayari
# (windows.apps.d/<kimlik>.conf) doluysa EXE dogrulanir; bossa her acilista uyari verilir. Bedel: program ile
# masaustu arasinda kopyala-yapistir yok.
set -u
KIMLIK=${1:?kullanim: windows_calistir.sh <kimlik> [--ic]}
case "$KIMLIK" in [a-z0-9_-]*) ;; *) echo "gecersiz kimlik: $KIMLIK" >&2; exit 1 ;; esac
KONF="/etc/armonia/windows.apps.d/$KIMLIK.conf"
P="/srv/paylasim/$KIMLIK"
oku() { sed -n "s/^$1=\"\(.*\)\"\$/\1/p" "$KONF" | head -n1; }
PROGRAM=$(oku PROGRAM)
AD=$(oku AD); [ -n "$AD" ] || AD="Windows programı ($KIMLIK)"
SHA=$(oku SHA256 | tr -cd '0-9a-f')
bildir() { notify-send -a Armonia "$AD" "$1" 2>/dev/null; echo "$AD: $1" >&2; }

[ -f "$KONF" ] || { bildir "Tanımsız uygulama: $KIMLIK"; exit 1; }
[ -n "$PROGRAM" ] || { bildir "Ayarlanmamış (yönetici: /etc/armonia/windows.apps.d/$KIMLIK.conf)."; exit 1; }
PROGRAM=$(printf '%s' "$PROGRAM" | tr '\\' '/')
case "$PROGRAM" in /*|*../*|..*) bildir "Geçersiz program yolu."; exit 1 ;; esac
mountpoint -q "$P" || { bildir "Ağ klasörüne ulaşılamadı. Ağ bağlantısını kontrol edip birkaç saniye sonra tekrar deneyin."; exit 1; }
EXE="$P/$PROGRAM"
[ -f "$EXE" ] || { bildir "Program ağ klasöründe bulunamadı: $PROGRAM"; exit 1; }
DIZIN=$(dirname "$EXE")

# ---- Dis katman: butunluk denetimi + cage ----
if [ "${2:-}" != "--ic" ]; then
  R=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
  W=${WAYLAND_DISPLAY:-wayland-0}
  [ -S "$R/$W" ] || { bildir "Masaüstü oturumu bulunamadı."; exit 1; }
  # Butunluk (denetci 23.11 O-1): Wine DLL'leri once programin kendi klasorunden yukler -> yalniz EXE'yi dogrulamak
  # yetmez. /etc/armonia/windows.apps.d/<kimlik>.sha256 varsa program klasorundeki TUM calistirilabilir dosyalar
  # (exe, dll, ocx, bpl, cpl, drv, sys, ax) listedeki ozetlerle tutmali ve listede olmayan calistirilabilir dosya
  # bulunmamalidir.
  LISTE="/etc/armonia/windows.apps.d/$KIMLIK.sha256"
  if [ -s "$LISTE" ]; then
    ( cd "$DIZIN" && sha256sum -c --quiet --strict "$LISTE" >/dev/null 2>&1 ) \
      || { bildir "GÜVENLİK: program dosyaları değişmiş (özet listesi tutmuyor). Açılmadı; yöneticiye bildirin."; exit 1; }
    YABANCI=$(cd "$DIZIN" && find . -maxdepth 3 -type f \
      \( -iname '*.exe' -o -iname '*.dll' -o -iname '*.ocx' -o -iname '*.bpl' -o -iname '*.cpl' -o -iname '*.drv' \
         -o -iname '*.sys' -o -iname '*.ax' \) | sed 's|^\./||' | while IFS= read -r f; do
        awk -v f="$f" '{ p = $0; sub(/^[0-9a-fA-F]+ [ *]/, "", p); if (p == f) bulundu = 1 } END { exit !bulundu }' "$LISTE" \
          || echo "$f"; done | head -n3)
    [ -z "$YABANCI" ] || { bildir "GÜVENLİK: listede olmayan program dosyası: $YABANCI. Açılmadı."; exit 1; }
  elif [ -n "$SHA" ]; then
    [ "$(sha256sum "$EXE" | cut -d' ' -f1)" = "$SHA" ] \
      || { bildir "GÜVENLİK: $PROGRAM değişmiş (SHA256 tutmuyor). Açılmadı; yöneticiye bildirin."; exit 1; }
    bildir "Uyarı: yalnız EXE doğrulandı; klasördeki DLL'ler doğrulanmıyor ($KIMLIK.sha256 listesi önerilir)."
  else
    bildir "Uyarı: program bütünlüğü doğrulanmıyor (ağ klasörü parolasız, herkes değiştirebilir)."
  fi
  # Kalan sinir: dogrulama ile Wine'in dosyayi agdan yeniden okumasi arasinda degistirilebilir (yaris); etkisi
  # cage + bubblewrap ile kum havuzunda sinirli kalir.
  C=$(mktemp -d "$R/armonia-cage.XXXXXX") || exit 1
  chmod 0700 "$C"
  trap 'rm -rf "$C"' EXIT INT TERM
  # cage ana masaustune istemci olarak baglanir (mutlak WAYLAND_DISPLAY), kendi soketini $C icinde acar
  env XDG_RUNTIME_DIR="$C" WAYLAND_DISPLAY="$R/$W" XKB_DEFAULT_LAYOUT=tr \
    cage -- "$0" "$KIMLIK" --ic
  exit $?
fi

# ---- Ic katman (cage'in cocugu): bubblewrap + Wine; yalniz cage'in soketi ----
CR=${XDG_RUNTIME_DIR:?}
CW=${WAYLAND_DISPLAY:?}
[ -S "$CR/$CW" ] || exit 1
ONEK="$HOME/.local/share/armonia-wine/$KIMLIK"
WDOS="$HOME/Belgeler/Windows"
mkdir -p "$ONEK" "$WDOS"

kum() {
  bwrap --unshare-all --unshare-user --disable-userns --die-with-parent --new-session --cap-drop ALL \
    --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/sbin /sbin \
    --symlink usr/lib /lib --symlink usr/lib64 /lib64 \
    --ro-bind /etc/ld.so.cache /etc/ld.so.cache --ro-bind /etc/fonts /etc/fonts \
    --ro-bind /etc/passwd /etc/passwd --ro-bind /etc/group /etc/group \
    --ro-bind-try /etc/localtime /etc/localtime --ro-bind-try /etc/alternatives /etc/alternatives \
    --ro-bind-try /etc/wine /etc/wine \
    --proc /proc --dev /dev --tmpfs /tmp \
    --dir "$CR" --ro-bind "$CR/$CW" "$CR/$CW" \
    --bind "$ONEK" "$ONEK" --bind "$WDOS" "$WDOS" --bind "$P" "$P" \
    --clearenv --setenv HOME "$HOME" --setenv USER armonia --setenv LANG tr_TR.UTF-8 \
    --setenv PATH /usr/bin --setenv XDG_RUNTIME_DIR "$CR" --setenv WAYLAND_DISPLAY "$CW" \
    --setenv WINEPREFIX "$ONEK" --setenv WINEDEBUG -all \
    --setenv WINEDLLOVERRIDES "mscoree,mshtml=;winemenubuilder.exe=d" \
    --chdir "$DIZIN" \
    "$@"
}

if [ ! -f "$ONEK/system.reg" ]; then
  # Hazirlik, Wayland surucusu secimi ve wineserver'in kayit defterini yazip kapanmasi ayni kum havuzunda (B-2)
  kum sh -c 'wineboot -i && wine reg add "HKCU\\Software\\Wine\\Drivers" /v Graphics /d wayland /f && wineserver -w' \
    || { bildir "Windows ortamı hazırlanamadı."; exit 1; }
fi
# Surucu harfleri: I: paylasim, W: ~/Belgeler/Windows. Z: kaldirilir (kolaylik; asil sinir bubblewrap'tir, B-6).
rm -f "$ONEK/dosdevices/z:"
ln -sfn "$P" "$ONEK/dosdevices/i:"
ln -sfn "$WDOS" "$ONEK/dosdevices/w:"
exec kum sh -c 'wine "$1"; wineserver -w' sh "$EXE"
