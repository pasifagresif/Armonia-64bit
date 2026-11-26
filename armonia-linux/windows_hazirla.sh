#!/bin/bash
# Armonia Windows katmanini (Wine + bubblewrap, coklu uygulama) imaj agacina koyar. iso_derle.sh WINDOWS=1
# (varsayilan) iken cagirir. Claude, 2026-11-25 (tek uygulamadan -"Icramatik"- genel coklu uygulamaya gecis;
# bkz. docs/claude-notlar/hafiza/dosya-yoneticisi-ve-windows-genellestirme-plani.md).
#   windows_hazirla.sh <includes.chroot / kok dizini> <kanca hedef dizini>
# Hangi exe'lerin kullanilacagina HICBIR sabit varsayim yok: derlemeden once windows/includes.chroot/etc/armonia/
# windows.apps.d/ altina, _ornek.conf.ornek'ten kopyalanmis, ".conf" ile biten bir dosya her uygulama icin konur.
# Bu betik her tanim icin bir .desktop ve bir dock kutucugu (custom/windows-<kimlik>) ÜRETİR; sabit "Icramatik"
# adi/simgesi KODA GOMULU DEGILDIR.
set -euo pipefail
HEDEF=$1 KANCA=$2
KAYNAK="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"

cp "$KAYNAK"/windows/hooks/*.hook.chroot "$KANCA"/
cp -a "$KAYNAK/windows/includes.chroot/." "$HEDEF/"

APPS_D="$HEDEF/etc/armonia/windows.apps.d"
# dock.jsonc/dock.css.sablon katman disinda, temel imajda (config/includes.chroot/etc/armonia/...); HEDEF o
# dizinin kendisi oldugu icin ayni yoldan erisilir (iso_derle.sh ikisini de "config/includes.chroot" uzerine kopyalar).
DOCK_J="$HEDEF/etc/armonia/waybar/dock.jsonc"
DOCK_CSS="$HEDEF/etc/armonia/tema/dock.css.sablon"
MASAUSTU="$HEDEF/usr/local/share/applications"
install -d -m 0755 "$MASAUSTU"

SAYI=0
SELEKTORLER=""
for f in "$APPS_D"/*.conf; do
  [ -f "$f" ] || continue
  KIMLIK=$(basename "$f" .conf)
  case "$KIMLIK" in [a-z0-9_-]*) ;; *) echo "HATA: gecersiz windows.apps.d dosya adi: $(basename "$f") (yalniz a-z0-9_- )"; exit 1 ;; esac
  case "$KIMLIK" in _ornek|yonetici) echo "HATA: '$KIMLIK' ayrilmis bir kimlik (armonia-windows-yonetici.desktop ile cakisir)"; exit 1 ;; esac
  AD=$(sed -n 's/^AD="\(.*\)"$/\1/p' "$f" | head -n1)
  [ -n "$AD" ] || AD="$KIMLIK"
  PAYLASIM=$(sed -n 's/^PAYLASIM="\(.*\)"$/\1/p' "$f" | head -n1)
  [ -n "$PAYLASIM" ] || echo "UYARI: windows.apps.d/$KIMLIK.conf'ta PAYLASIM bos; kisayolu 'Ayarlanmamis' diyecek"

  # .desktop (sabit "Icramatik" yerine her tanimdan uretilir)
  cat > "$MASAUSTU/armonia-windows-$KIMLIK.desktop" <<EOT
[Desktop Entry]
Type=Application
Name=$AD
Comment=Ağdaki $AD (Windows programı, ağsız kum havuzunda)
Exec=/opt/armonia/bin/windows_calistir.sh $KIMLIK
Icon=applications-office
Categories=Office;
Terminal=false
EOT

  # Dock kutucugu: her uygulama kendi modulu (custom/windows-<kimlik>), dizinin SONUNA (", "wlr/taskbar"]" ile
  # BIRLIKTE eslenir ki ayrica "wlr/taskbar": {...} modul TANIMI (farkli bir JSON anahtari) yanlislikla
  # degismesin - ikisi de ayni altdizgiyi ("wlr/taskbar") icerir.
  MODUL="custom/windows-$KIMLIK"
  ADJSON=$(printf '%s' "$AD" | sed 's/\\/\\\\/g; s/"/\\"/g')
  sed -i -e "s|, \"wlr/taskbar\"\]|, \"$MODUL\", \"wlr/taskbar\"]|" \
    -e "/^  \"modules-center\": \[/a\\  \"$MODUL\": { \"format\": \"$ADJSON\\\\n \", \"justify\": \"center\", \"tooltip-format\": \"Ağdaki $ADJSON (ağsız kum havuzunda)\", \"on-click\": \"/opt/armonia/bin/windows_calistir.sh $KIMLIK\" },\n" "$DOCK_J"
  grep -q ", \"$MODUL\", \"wlr/taskbar\"\]" "$DOCK_J" && grep -q "^  \"$MODUL\":" "$DOCK_J" \
    || { echo "HATA: dock'a $MODUL eklenemedi"; exit 1; }

  SELEKTORLER="$SELEKTORLER #custom-windows-$KIMLIK"
  SAYI=$((SAYI + 1))
done

if [ -n "$SELEKTORLER" ]; then
  # Simgesiz kutucuk stili (Coklu UYAP ile ayni): isaretli (ARMONIA-WINDOWS-...) bir blok olarak EKLENIR; ayni
  # isaretler pencere-yardimci'nin CANLI sistemde (derlemeden sonra, uygulama eklenince/silinince) bu blogu
  # bulup YENIDEN URETEBILMESI icin kullanilir (iki betik ayni bicimi paylasir, bkz. pencere-yardimci).
  TEMEL=$(echo $SELEKTORLER | sed 's/ /, /g')
  HOVER=$(echo $SELEKTORLER | sed 's/\([^ ]*\)/\1:hover/g; s/ /, /g')
  {
    echo ""
    echo "/* ARMONIA-WINDOWS-BASLA (windows_hazirla.sh/pencere-yardimci uretti; elle duzenlemeyin) */"
    echo "$TEMEL {"
    echo "  color: #@YAZI@; min-width: 84px; padding: 12px 6px; border-radius: 12px;"
    echo "}"
    echo "$HOVER { background-color: #@VURGU_ZEMIN@; }"
    echo "/* ARMONIA-WINDOWS-BITIS */"
  } >> "$DOCK_CSS"
fi

echo "Windows katmani kopyalandi: $SAYI uygulama tanimli"
if [ "$SAYI" -eq 0 ]; then
  echo "UYARI: windows.apps.d altinda hic uygulama tanimi yok (ornek: _ornek.conf.ornek'i <kimlik>.conf olarak kopyalayip doldurun)"
fi
