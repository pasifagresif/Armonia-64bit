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
# DEGILDIR; yalniz "habersiz takilan aygit"a karsi korur. Kalici cozum acik is (bkz. GUNCEL_DURUM).
# Aygitin bildirdigi ad saldirganin denetimindedir: yalniz guvenli karakterler gosterilir.
set -u
[ "${USBGUARD_IPC_SIGNAL:-}" = "Device.PresenceChanged" ] || exit 0
[ "${USBGUARD_DEVICE_EVENT:-}" = "Insert" ] || exit 0
[ "${USBGUARD_DEVICE_TARGET:-}" = "block" ] || exit 0
ID=${USBGUARD_DEVICE_ID:-}
case "$ID" in ''|*[!0-9]*) exit 0 ;; esac
KURAL=${USBGUARD_DEVICE_RULE:-}

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

R=/run/armonia-usb-sor
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
    /opt/armonia/bin/soru.sh "USB aygıtı takıldı" "$TUR — $AD. İzin vermezseniz kullanılamaz." \
    "USB ($TUR): $AD" "Reddet" "Bu kez izin ver" >"$C" 2>/dev/null 9>&-
  SECIM=$(head -n1 "$C" 2>/dev/null)
  case "$SECIM" in
    "Bu kez izin ver")
      usbguard allow-device "$ID" && logger -t armonia-usb-sor "izin: $ID ($TUR, $AD)" || exit 0
      # Surucu / yazici kurulumu ayri birimde (ag ve apt yetkisi bu servise verilmez)
      case "$VIDPID" in
        [0-9a-f][0-9a-f][0-9a-f][0-9a-f]:[0-9a-f][0-9a-f][0-9a-f][0-9a-f])
          systemctl --no-block start "armonia-surucu@$(echo "$VIDPID" | tr : -)${YAZICI:+-yazici}.service" ;;
      esac ;;
    "Reddet")
      : > "$R/ret-$ANAHTAR"
      usbguard reject-device "$ID" && logger -t armonia-usb-sor "ret: $ID ($TUR, $AD)" ;;
    *)
      : > "$R/sessiz-$ANAHTAR"
      logger -t armonia-usb-sor "cevapsiz, bekletiliyor: $ID ($TUR, $AD)" ;;
  esac
) 9>"$R/kilit" >/dev/null 2>&1 &
exit 0
