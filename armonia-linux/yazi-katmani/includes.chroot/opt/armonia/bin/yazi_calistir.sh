#!/bin/sh
# armonia-yazi@.service içinde (kum havuzu, kullanıcı 'armonia', ağsız): soketten tek satır okur (boş = yeni belge,
# yoksa belge yolu), yolu YENİDEN denetler ve Armonia Yazı'yı açar. Claude, 2026-11-24.
set -u
# Satır sonu göndermeyen istemci örneği askıda tutmasın
SATIR=$(timeout 3 head -n 1) || exit 1
YOL=""
if [ -n "$SATIR" ]; then
  # Kum havuzunda yalnız Belgeler ve İndirilenler bağlı: dışarı işaret eden bağ burada çözülemez ve reddedilir
  YOL=$(realpath -e -- "$SATIR" 2>/dev/null) || { echo "yazi: dosya yok ya da kum havuzu dışında" >&2; exit 1; }
  [ -f "$YOL" ] || { echo "yazi: dosya değil" >&2; exit 1; }
  case "$YOL" in
    /home/armonia/Belgeler/*|/home/armonia/İndirilenler/*) ;;
    *) echo "yazi: izin verilmeyen yol reddedildi" >&2; exit 1 ;;
  esac
  case "$YOL" in
    *.[uU][dD][fF]|*.[dD][oO][cC][xX]) ;;
    *) echo "yazi: yalnız udf/docx" >&2; exit 1 ;;
  esac
fi
R="/run/user/$(id -u)"
[ -S "$R/wayland-0" ] || { echo "yazi: masaüstü oturumu yok" >&2; exit 1; }
D=/home/armonia/.local/share/armonia-yazi
# HOME gerçek ev dizini kalır (dosya penceresi Belgeler/İndirilenler'i göstersin; ev dizininin kendisi salt okunur
# boş tmpfs). Yazılan her şey (WebKit önbelleği, GTK son kullanılanlar) veri klasörüne.
export XDG_RUNTIME_DIR="$R" WAYLAND_DISPLAY=wayland-0 GDK_BACKEND=wayland HOME=/home/armonia \
  XDG_CONFIG_HOME="$D/config" XDG_CACHE_HOME="$D/cache" XDG_DATA_HOME="$D/data" TMPDIR="$D/tmp"
unset DISPLAY
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$TMPDIR"
printf 'XDG_DOCUMENTS_DIR="$HOME/Belgeler"\nXDG_DOWNLOAD_DIR="$HOME/İndirilenler"\n' > "$XDG_CONFIG_HOME/user-dirs.dirs"
# Bağlantı soketi uygulamaya devredilmesin
exec </dev/null 3<&-
unset LISTEN_FDS LISTEN_PID LISTEN_FDNAMES
if [ -n "$YOL" ]; then
  exec /opt/armonia/yazi/armonia-yazi "$YOL"
fi
exec /opt/armonia/yazi/armonia-yazi
