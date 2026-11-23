#!/bin/sh
# USB "takinca sor" (plan A5). armonia-usb-sor.service icinde `usbguard watch --exec` ile HER IPC olayinda
# root olarak calisir. Yalniz bekletilen (block) yeni aygit icin masaustunde soru acar; cevaba gore
# `usbguard allow-device` (yalniz bu takilis) ya da `reject-device`. Cevap yoksa (60 sn, ekran kilitli, oturum
# yok) aygit bekletilmis kalir. Claude, 2026-11-19.
#
# Guven siniri: kullanici surecleri USBGuard IPC'ye erisemez (IPCAllowedUsers=root). Soru penceresi kullanici
# kimligiyle acilir; cevap root'a ait 0600 dosyaya yazilir (boru degil: ayni kullanicinin sureci /proc/<pid>/fd
# uzerinden boruya yazabilir ya da boruyu acik tutup soruları kilitleyebilirdi; denetim O3, 19.11).
# BILINEN SINIR (denetim Y1, 19.11): ayni kullanici kimligiyle calisan kotu amacli bir surec Wayland sanal klavye
# protokoluyle fuzzel'e "izin ver" sectirebilir. Yani bu soru, kullanici oturumu ele gecirilmisse guvenlik siniri
# DEGILDI. 23.11: fiziksel onayla (cikar + tak) kapatildi, asagiya bakin.
# Aygitin bildirdigi ad saldirganin denetimindedir: yalniz guvenli karakterler gosterilir.
#
# 23.11 (G6, kullanici karari "sor ama sertlestir"): Y1 icin FIZIKSEL ONAY. Sorudaki "izin ver" tek basina izin
# VERMEZ; yalniz "onay bekliyor" kaydi acar. Izin icin aygit cikarilip BASKA BIR USB YUVASINA takilmalidir:
#   - "izin ver"den sonra en gec 2 dk icinde cikarilir;
#   - en erken 2 sn, en gec 20 sn sonra, cikarildigi yuvadan FARKLI bir yuvaya takilir;
#   - yeni yuvanin ebeveyni (hub) cikarma aninda zaten takili olan bir aygit olmalidir.
# Neden yuva sarti (guvenlik-denetci 23.11 G6-a): mikrodenetleyicili kotu amacli aygit (BadUSB) kendini yazilimla
# ayirip ayni yuvaya yeniden baglanabilir; yuva degistirmeyi ve icindeki yeni bir hub'i taklit edemez.
# Korudugu: oturumdaki kotu surec (sanal klavyeyle "izin ver") + zararsiz ya da kotu aygit. Korumadigi: fiziksel
# olarak orada olan ve aygiti kendi eliyle tasiyan saldirgan. Ayni ozetli (seri numarasiz klon) aygitlar onayi
# paylasabilir; bunun icin de fiziksel varlik gerekir. Acilista takili bekletilen aygit (D8): cikar-tak ile soru
# gelir, onaydan sonra baska yuvaya bir kez daha takilir.
set -u
[ "${USBGUARD_IPC_SIGNAL:-}" = "Device.PresenceChanged" ] || exit 0
R=/run/armonia-usb-sor
KURAL=${USBGUARD_DEVICE_RULE:-}
alan() { printf '%s' "$KURAL" | sed -n "s/.* $1 \"\([^\"]*\)\".*/\1/p" | head -n1; }
OZET=$(alan hash | tr -cd 'A-Za-z0-9' | cut -c1-64)
YUVA=$(alan via-port | tr -cd '0-9.-' | cut -c1-32)
EBEVEYN=$(alan parent-hash | tr -cd 'A-Za-z0-9+/=' | cut -c1-64)
ONAY_SN=20
ALT_SN=2
yas() { echo $(( $(date +%s) - $(head -n1 "$1" 2>/dev/null || echo 0) )); }

case "${USBGUARD_DEVICE_EVENT:-}" in
  Remove)
    if [ -n "$OZET" ] && [ -e "$R/onay-$OZET" ] && [ "$(yas "$R/onay-$OZET")" -le 120 ]; then
      rm -f "$R/onay-$OZET"
      # Kayit: zaman, cikarilan yuva, cikarma aninda takili aygitlarin ozetleri (yeni hub'i ayirt etmek icin)
      { date +%s; echo "$YUVA"; usbguard list-devices 2>/dev/null | sed -n 's/.* hash "\([^"]*\)".*/\1/p'; } > "$R/tak-$OZET"
      logger -t armonia-usb-sor "onay: aygit $YUVA yuvasindan cikarildi; ${ALT_SN}-${ONAY_SN} sn icinde BASKA yuvaya takilmali"
    fi
    exit 0 ;;
  Insert)
    # Ilk gorulme zamani (her hedef icin; acilista takili olanlarin kaydi olmaz = "eski"). Icinde hub tasiyan
    # BadUSB'yi ayirt etmek icin (denetci A-2): onu tasiyan hub aygitla ayni anda belirir.
    [ -n "$OZET" ] && [ ! -e "$R/gorulme-$OZET" ] && date +%s > "$R/gorulme-$OZET" ;;
  *) exit 0 ;;
esac
[ "${USBGUARD_DEVICE_TARGET:-}" = "block" ] || exit 0
ID=${USBGUARD_DEVICE_ID:-}
case "$ID" in ''|*[!0-9]*) exit 0 ;; esac
# Fiziksel onay tamamlandi mi?
if [ -n "$OZET" ] && [ -e "$R/tak-$OZET" ]; then
  T="$R/tak-$OZET"; Y=$(yas "$T"); ESKI_YUVA=$(sed -n 2p "$T")
  red=""
  [ "$Y" -ge "$ALT_SN" ] || red="cok hizli takildi (${Y} sn; yazilimla ayrilip baglanma suphesi)"
  [ "$Y" -le "$ONAY_SN" ] || red="sure gecti (${Y} sn)"
  [ -n "$YUVA" ] && [ "$YUVA" != "$ESKI_YUVA" ] || red="ayni yuvaya takildi ($YUVA); baska bir yuvaya takin"
  [ -n "$EBEVEYN" ] && sed -n '3,$p' "$T" | grep -qxF "$EBEVEYN" || red="yeni yuvanin ebeveyni onceden takili degil (aygit icinde hub?)"
  # Ebeveyn yalniz acilistan beri takili bir hub (ya da kok hub) olabilir: sonradan takilan hub'in "gorulme" kaydi
  # vardir ve reddedilir (denetci A-2 / D-1: icinde hub tasiyan BadUSB alt aygiti geciktirerek sure denetimini asabilir)
  EB_DOSYA="$R/gorulme-$(printf '%s' "$EBEVEYN" | tr -cd 'A-Za-z0-9' | cut -c1-64)"
  [ ! -e "$EB_DOSYA" ] || red="sonradan takilan bir hub uzerinde; onay icin aygiti dogrudan bilgisayarin yuvasina takin"
  rm -f "$T"
  if [ -n "$red" ]; then
    logger -t armonia-usb-sor "fiziksel onay REDDEDILDI: $ID: $red"
    KID=$(id -u armonia 2>/dev/null) && timeout 5 setpriv --reuid=armonia --regid=armonia --init-groups --reset-env \
      env XDG_RUNTIME_DIR="/run/user/$KID" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$KID/bus" \
      notify-send -a Armonia "USB izni verilmedi" "$red" 2>/dev/null
    exit 0
  fi
  # Politika kararindan once allow gonderilmesin (denetci G6-b): aygit bekletilenlerde gorunene dek bekle
  i=0; while ! usbguard list-devices -b 2>/dev/null | grep -q "^$ID: " && [ $i -lt 30 ]; do sleep 0.1; i=$((i + 1)); done
  if usbguard allow-device "$ID"; then
    logger -t armonia-usb-sor "izin (fiziksel onayla, yuva $ESKI_YUVA -> $YUVA): $ID"
    VP=$(printf '%s' "$KURAL" | sed -n 's/^[a-z]* id \([0-9a-f]\{4\}:[0-9a-f]\{4\}\) .*/\1/p')
    YZ=""; printf '%s' "$KURAL" | grep -q 'with-interface.*07:' && YZ=1
    case "$VP" in
      [0-9a-f][0-9a-f][0-9a-f][0-9a-f]:[0-9a-f][0-9a-f][0-9a-f][0-9a-f])
        systemctl --no-block start "armonia-surucu@$(echo "$VP" | tr : -)${YZ:+-yazici}.service" ;;
    esac
  fi
  exit 0
fi

AD=$(printf '%s' "$KURAL" | sed -n 's/.* name "\([^"]*\)".*/\1/p' | tr -cd 'A-Za-z0-9 ._-' | cut -c1-40)
[ -n "$AD" ] || AD="adsız aygıt"
SINIFLAR=$(printf '%s' "$KURAL" | sed -n 's/.*with-interface \(.*\)$/\1/p' \
  | grep -o '[0-9a-f][0-9a-f]:[0-9a-f*][0-9a-f*]:[0-9a-f*][0-9a-f*]' | cut -c1-2 | sort -u)
TUR=""
for s in $SINIFLAR; do
  case "$s" in
    08) t="bellek/disk" ;;
    07) t="yazıcı" ;;
    06) t="tarayıcı/fotoğraf makinesi" ;;
    ff) t="üreticiye özgü" ;;
    ef) t="çok işlevli" ;;
    *) t="sınıf $s" ;;
  esac
  TUR="${TUR:+$TUR + }$t"
done
[ -n "$TUR" ] || TUR="bilinmeyen tür"
VIDPID=$(printf '%s' "$KURAL" | sed -n 's/^[a-z]* id \([0-9a-f]\{4\}:[0-9a-f]\{4\}\) .*/\1/p')
YAZICI=""
case " $(echo $SINIFLAR) " in *" 07 "*) YAZICI=1 ;; esac

# Bu oturumda reddedilen model yeniden sorulmaz; cevapsiz kalan 2 dk sorulmaz (kendini yeniden tanitip soru
# yagdiran aygita karsi; denetim O2).
ANAHTAR=$(printf '%s' "${VIDPID:-yok}" | tr -cd '0-9a-f')
if [ -e "$R/ret-$ANAHTAR" ]; then
  usbguard reject-device "$ID" >/dev/null 2>&1; logger -t armonia-usb-sor "onceden reddedildi: $ID ($AD)"; exit 0
fi
if [ -e "$R/sessiz-$ANAHTAR" ] && [ -n "$(find "$R/sessiz-$ANAHTAR" -mmin -2 2>/dev/null)" ]; then
  logger -t armonia-usb-sor "yakinda cevapsiz kaldi, sorulmadi: $ID ($AD)"; exit 0
fi
# Kuyruk siniri: en fazla 3 soru bekler
BEKLEYEN=$(find "$R" -maxdepth 1 -name 'bekliyor-*' 2>/dev/null | wc -l)
[ "$BEKLEYEN" -lt 3 ] || { logger -t armonia-usb-sor "kuyruk dolu, sorulmadi: $ID ($AD)"; exit 0; }
: > "$R/bekliyor-$ID"

# Soru arka planda; watch beklemesin. Sorular sirayla (kilit).
(
  trap 'rm -f "$R/bekliyor-$ID" "$R/cevap-$ID"' EXIT
  flock 9
  usbguard list-devices -b 2>/dev/null | grep -q "^$ID: " || exit 0
  KULLANICI_ID=$(id -u armonia) || exit 0
  C="$R/cevap-$ID"; rm -f "$C"
  # Cevap root'a ait 0600 dosyaya (UMask=0077); kilit fd'si alt surece gecmez; sure asiminda hepsi oldurulur
  timeout -k 5 75 setpriv --reuid=armonia --regid=armonia --init-groups --reset-env \
    env XDG_RUNTIME_DIR="/run/user/$KULLANICI_ID" \
    /opt/armonia/bin/soru.sh "USB aygıtı takıldı" \
    "$TUR — $AD. İzin için seçin, sonra aygıtı çıkarıp BAŞKA bir USB yuvasına takın (2-${ONAY_SN} sn)." \
    "USB ($TUR): $AD" "Reddet" "Bu kez izin ver" >"$C" 2>/dev/null 9>&-
  SECIM=$(head -n1 "$C" 2>/dev/null)
  case "$SECIM" in
    "Bu kez izin ver")
      # Izin HENUZ verilmez (Y1): fiziksel onay (cikar + tak) beklenir; izin ustteki Insert dalinda verilir,
      # surucu/yazici kurulumu da orada baslar. Ozet yoksa onay kurulamaz -> izin yok.
      [ -n "$OZET" ] || { logger -t armonia-usb-sor "ozet yok, fiziksel onay kurulamadi: $ID"; exit 0; }
      date +%s > "$R/onay-$OZET"
      logger -t armonia-usb-sor "onay bekliyor (cikar-tak): $ID ($TUR, $AD)"
      timeout 5 setpriv --reuid=armonia --regid=armonia --init-groups --reset-env \
        env XDG_RUNTIME_DIR="/run/user/$KULLANICI_ID" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$KULLANICI_ID/bus" \
        notify-send -a Armonia "Onay: aygıtı başka yuvaya takın" \
        "$AD: şimdi çıkarın ve 2-${ONAY_SN} saniye içinde BAŞKA bir USB yuvasına takın." 2>/dev/null 9>&- ;;
    "Reddet")
      : > "$R/ret-$ANAHTAR"
      usbguard reject-device "$ID" && logger -t armonia-usb-sor "ret: $ID ($TUR, $AD)" ;;
    *)
      : > "$R/sessiz-$ANAHTAR"
      logger -t armonia-usb-sor "cevapsiz, bekletiliyor: $ID ($TUR, $AD)" ;;
  esac
) 9>"$R/kilit" >/dev/null 2>&1 &
exit 0
