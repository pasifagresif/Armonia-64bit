#!/bin/sh
# Menu / dock "Coklu UYAP" kisayolu. Uygulamayi KENDISI calistirmaz: root'un tanimladigi kum havuzlu sistem birimini
# (armonia-cokluuyap.service) baslatir; izin polkit kuralinda (60-armonia-cokluuyap.rules: yalniz bu birim, yalniz
# start/stop, yalniz yerel etkin oturumdaki 'armonia'). Acilista hicbir sey bu betigi cagirmaz. Claude, 2026-11-23.
set -u
B=armonia-cokluuyap.service
bildir() { notify-send -a Armonia "Çoklu UYAP" "$1" 2>/dev/null; echo "Çoklu UYAP: $1" >&2; }
[ -x /opt/armonia/cokluuyap/uyap_app ] || { bildir "Bu sistemde Çoklu UYAP kurulu değil."; exit 1; }
if systemctl is-active --quiet "$B"; then
  # Pencere dock'a kucultulmusse geri getirilir: uyap_app SIGUSR1'i yakalar (ruyap/src/linux_gosterge.rs). Yalniz
  # ana surec (--kill-whom=main): yardimci alt surecler bu sinyalle olmesin. Izin: 60-armonia-cokluuyap.rules.
  systemctl kill --kill-whom=main -s SIGUSR1 "$B" 2>/dev/null
  bildir "Çoklu UYAP zaten açık, pencere öne getirildi."
  exit 0
fi
# Canli sistem (ISO, toram): veri RAM'de (kullanici karari 23.11)
grep -qw 'boot=live' /proc/cmdline && bildir "Canlı sistem: hesaplar ve veriler bilgisayar kapanınca silinir."
if ! HATA=$(systemctl start "$B" 2>&1); then
  case "$HATA" in
    *start-limit*|*"too quickly"*|*"too often"*) bildir "Çoklu UYAP üst üste kapandı. Beş dakika sonra yeniden deneyin." ;;
    *) bildir "Açılamadı: $(printf '%s' "$HATA" | head -c 200)" ;;
  esac
  exit 1
fi
