#!/bin/sh
# Agdaki Windows program klasorunu (/etc/armonia/windows.conf PAYLASIM) root olarak baglar; ag gelene kadar
# 15 sn arayla dener. armonia-paylasim.service calistirir. Claude, 2026-11-23.
# Kullanici karari 23.11: paylasim parolasiz (misafir). Kullanici surecleri baglama yapamaz (mount.cifs setuid degil).
set -u
KONF=/etc/armonia/windows.conf
HEDEF=/srv/paylasim/windows
oku() { sed -n "s/^$1=\"\(.*\)\"\$/\1/p" "$KONF" | head -n1; }
PAYLASIM=$(oku PAYLASIM)
[ -n "$PAYLASIM" ] || { echo "paylasim ayarlanmamis ($KONF)"; exit 0; }
# Yalniz //IPv4/ad bicimi; virgul (baglama secenegi enjeksiyonu) ve kontrol karakterleri reddedilir
case "$PAYLASIM" in
  //[0-9]*.[0-9]*.[0-9]*.[0-9]*/?*) ;;
  *) echo "gecersiz PAYLASIM: //IP/ad bicimi bekleniyor"; exit 1 ;;
esac
case "$PAYLASIM" in *,*|*'"'*) echo "gecersiz karakter"; exit 1 ;; esac
printf '%s' "$PAYLASIM" | LC_ALL=C grep -q '[^ -~]' && { echo "ASCII disi karakter"; exit 1; }

KUID=$(id -u armonia) KGID=$(id -g armonia)
install -d -m 0750 -o root -g armonia /srv/paylasim
install -d -m 0770 -o root -g armonia "$HEDEF"
# SMB1 yok (vers belirtilmez -> 2.1+ pazarlik); nosuid,nodev; dosyalar yalniz armonia'ya.
# noexec KONMAZ: Wine PE dosyalarini calistirilabilir olarak esler (mmap PROT_EXEC) ve noexec'te acilmaz.
# username=guest,password= : misafir kimligi acikca (yalniz 'guest' secenegi root adiyla gidebilir; denetci B-4).
SECENEK="username=guest,password=,uid=$KUID,gid=$KGID,forceuid,forcegid,file_mode=0660,dir_mode=0770,nosuid,nodev,noserverino"
# Hata dosyasi /tmp'de degil, servisin kendi calisma dizininde (denetci B-5: /tmp'de onceden olusturulan dosya engeller)
HATA="${RUNTIME_DIRECTORY:-/run/armonia-paylasim}/hata"
until mountpoint -q "$HEDEF"; do
  # Sunucuda Guest hesabi kapali ama anonim erisim aciksa ikinci deneme sec=none (denetci D-4)
  if mount -t cifs "$PAYLASIM" "$HEDEF" -o "$SECENEK" 2>"$HATA" \
     || mount -t cifs "$PAYLASIM" "$HEDEF" -o "sec=none,${SECENEK#username=guest,password=,}" 2>>"$HATA"; then
    echo "baglandi: $PAYLASIM -> $HEDEF ($(findmnt -n -o OPTIONS "$HEDEF" | grep -o 'vers=[^,]*'))"
    break
  fi
  echo "baglanamadi ($(head -c 200 "$HATA")); 15 sn sonra yeniden"
  sleep 15
done
