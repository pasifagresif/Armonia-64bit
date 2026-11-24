#!/bin/sh
# armonia-cokluuyap.service icinde (kum havuzu, kullanici 'armonia') uyap_app'i calistirir. Claude, 2026-11-23.
set -u
R="/run/user/$(id -u)"
[ -S "$R/wayland-0" ] || { echo "cokluuyap: masaustu oturumu yok"; exit 0; }
export XDG_RUNTIME_DIR="$R" WAYLAND_DISPLAY=wayland-0
# Xwayland'e gitmesin; uygulama Wayland'e dogrudan baglanir
unset DISPLAY
cd "$UYAP_DATA_DIR" || exit 1
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$HOME/CokluUyap"
# Armonia temasi (Ayarlar -> Gorunum, /etc/armonia/ayarlar.conf TEMA=) Coklu UYAP'a da: uygulama temayi
# $HOME/CokluUyap/tema.txt'den okur (yalniz kendi listesindeki kimlik). Kum havuzunun icinde yazilir; Ayarlar
# havuzun klasorune yazmaz (denetci 23.11 O1). Bag varsa once kaldirilir.
TEMA=$(sed -n 's/^TEMA=\([a-z]*\)$/\1/p' /etc/armonia/ayarlar.conf 2>/dev/null | head -n1)
if [ -n "$TEMA" ]; then
  rm -f "$HOME/CokluUyap/tema.txt"
  printf '%s' "$TEMA" > "$HOME/CokluUyap/tema.txt"
fi
# Hangi surum (24.11, guncelleme katmani; yalniz Pi): armonia-guncelle etkin-sec, kurulu imzali guncellemeyi bu kum
# havuzunun ICINDE (root degil) dogrular (imza -> agac ozeti -> her dosya) ve dizinini yazar; bos, hata ya da
# beklenmeyen cikti -> imajdaki surum. Dizin bir kez cozulur ve yalniz o somut dizin calistirilir (denetci 24.11
# O2/O3). Katman yoksa (ISO) ikili de yoktur.
U=/opt/armonia/cokluuyap
G=/opt/armonia/guncelle/armonia-guncelle
if [ -x "$G" ]; then
  D=$("$G" etkin-sec) || D=""
  case "$D" in
    "") ;;
    /var/lib/armonia-guncelle/cokluuyap/[0-9]*.[0-9]*.[0-9]*)
      case "$D" in *[!0-9a-z/.-]*|*..*) echo "cokluuyap: etkin-sec beklenmeyen yol, imaj kullaniliyor" ;; *) U="$D" ;; esac ;;
    *) echo "cokluuyap: etkin-sec beklenmeyen yol, imaj kullaniliyor" ;;
  esac
fi
echo "cokluuyap: $U"
# stdin bos: uygulama konsoldan okumaya kalkarsa sonsuza dek beklemesin (14.11 sertifika secimi hatasi)
# Oturum kapaninca oturum.sh birimi durdurur (yeniden deneme de iptal olur; denetci 23.11 O2)
exec "$U/uyap_app" </dev/null
