#!/bin/sh
# waybar e-imza gostergesi (JSON). Kart bilgisi OKUNMAZ (PIN/sertifika yok): yalniz istemci calisiyor mu
# ve 5975 dinleniyor mu. Claude, 2026-11-19.
if ! systemctl -q is-active armonia-eimza.service; then
  echo '{"text":"E-İmza: Kapalı","class":"kapali","tooltip":"Adalet E-imza istemcisi çalışmıyor"}'
elif grep -q ':1757 ' /proc/net/tcp /proc/net/tcp6 2>/dev/null; then
  # "Hazir" yaniltici: kart/token TAKILI MI diye BAKILMIYOR (ustteki yorum), yalniz istemci calisiyor.
  # Kullanici karti takmadan da bunu "imzaya hazir" sanip yanlis anlayabiliyordu (denetci 25.11).
  echo '{"text":"E-İmza: İstemci Açık","class":"hazir","tooltip":"Adalet E-imza istemcisi çalışıyor (127.0.0.1:5975) — kart/token takılı mı ayrıca kontrol edin"}'
else
  echo '{"text":"E-İmza: Açılıyor","class":"bekliyor","tooltip":"Adalet E-imza açılıyor"}'
fi
