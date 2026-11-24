#!/bin/bash
# Masaustu (labwc) ortaya cikinca acilis olcumlerini /dev/ttyS0'a yazar. Claude, 2026-11-13 (14'te genisletildi;
# 19.11 temel masaustu: uyap_app/kasa/PostgreSQL bolumleri cikti, Coklu UYAP katmaniyla geri gelecek).
exec >/dev/ttyS0 2>&1
for i in $(seq 1 120); do pid=$(pgrep -x labwc | head -1); [ -n "$pid" ] && break; sleep 0.5; done
HZ=$(getconf CLK_TCK)
bas() { awk -v hz="$HZ" '{printf "%.2f sn", $22/hz}' "/proc/$1/stat" 2>/dev/null; }
# Acilisin bitmesini bekle (bu servis Type=exec, acilisi bekletmez)
timeout 60 systemctl is-system-running --wait >/dev/null 2>&1
sleep 3
echo "=====ARMONIA-OLCUM-BASI====="
echo "labwc baslangic (acilistan beri): $( [ -n "$pid" ] && bas "$pid" || echo YOK )"
echo "waybar: $(pgrep -c -x waybar)  swaybg: $(pgrep -c -x swaybg)  mako: $(pgrep -c -x mako)  uyap_app (0 olmali): $(pgrep -c -x uyap_app)"
# Cift pencere teshisi: kac uyap_app/oturum.sh/labwc var, kim baslatmis (ust surec zinciri)
# guvenlik-denetci D3: args DEGIL comm ile sec (chromium --app=...?k=<giris kodu> COM1'e dusmesin)
echo "--- surec agaci (greetd/labwc/waybar)"; ps -o pid,ppid,etimes,user,comm -C greetd,labwc,waybar
echo "--- systemd-analyze"; systemd-analyze 2>&1
echo "--- blame (ilk 20)"; systemd-analyze blame 2>&1 | head -20
echo "--- critical-chain"; systemd-analyze critical-chain greetd.service 2>&1 | head -25
echo "--- systemctl durum"; systemctl is-system-running; systemctl --failed --no-legend
echo "--- bellek"; free -m
echo "--- surec RSS (MB)"; ps -eo rss,comm --sort=-rss | head -12 | awk 'NR==1{print;next}{printf "%6.1f %s\n",$1/1024,$2}'
pgrep -x Xwayland >/dev/null && echo "Xwayland: CALISIYOR" || echo "Xwayland: yok"
echo "--- herkese-yazilabilir dosyalar (kok dosya sistemi)"
find / -xdev -type f -perm -0002 2>/dev/null | head -5; echo "toplam: $(find / -xdev -type f -perm -0002 2>/dev/null | wc -l)"
echo "--- izinler"; stat -c '%a %U %n' /opt/armonia/bin/*.sh /etc/greetd/config.toml /etc/armonia/labwc/rc.xml /etc/default/locale
echo "--- ag"; networkctl list --no-legend 2>&1 | head -5; resolvectl query deb.debian.org 2>&1 | head -2
echo "--- sertlestirme"
echo "cmdline: $(cat /proc/cmdline)"
echo "lockdown: $(cat /sys/kernel/security/lockdown 2>&1)"
echo "nftables: $(systemctl is-active nftables) / $(nft list chain inet filter input 2>&1 | grep -o 'policy drop' | head -1)"
echo "rp_filter eth0: $(sysctl -n net.ipv4.conf.eth0.rp_filter 2>&1)  io_uring_disabled: $(sysctl -n kernel.io_uring_disabled 2>&1)"
echo "live-config paketi: $(dpkg-query -W -f='${Status}' live-config 2>&1)"
loginctl show-session "$(loginctl list-sessions --no-legend | awk '/armonia/{print $1; exit}')" -p Seat -p VTNr -p Type 2>&1 | tr '\n' ' '; echo
echo "--- tarayici: $(/usr/bin/chromium --version 2>&1 | head -1)  du: $(du -sh /usr/lib/chromium 2>/dev/null | cut -f1)"
echo "soket: $(systemctl is-active armonia-tarayici.socket 2>&1)  $(stat -c '%a %U' /run/armonia-tarayici.sock 2>&1)  politika: $(stat -c '%a %U' /etc/chromium/policies/managed/armonia.json 2>&1)"
echo "reddedilmeli (dosya adresi): $(runuser -u armonia -- /usr/local/bin/xdg-open file:///etc/shadow 2>&1)"
echo "reddedilmeli (nobody -> soket): $(printf 'https://avukat.uyap.gov.tr/\n' | runuser -u nobody -- /usr/bin/nc.openbsd -N -U /run/armonia-tarayici.sock 2>&1 | head -1)"
if grep -q 'armonia.olcum_tarayici=1' /proc/cmdline; then
  # Olcum ISO'su: dock'taki UYAP Avukat dugmesinin yaptigini bir kez yap, kum havuzunu raporla
  # Cikti $(...) ile alinir: runuser'in stdout'u dogrudan ttyS0 iken seri cikti kesiliyordu (14 Kasim, 2 deneme)
  # timeout: baglanti acik kalirsa (fd 3 sizintisi, 14 Kasim 12:10) olcum burada askida kaliyordu; 124 = hata
  echo "xdg-open: $(timeout 10 runuser -u armonia -- /usr/local/bin/xdg-open "https://avukat.uyap.gov.tr/" </dev/null 2>&1; echo "cikis=$?")"
  for i in $(seq 1 40); do pgrep -u armonia -x chromium >/dev/null && break; sleep 0.5; done
  sleep 8
  echo "chromium surec sayisi: $(pgrep -u armonia -c chromium)  birim: $(systemctl list-units --no-legend 'armonia-tarayici@*' 2>&1 | head -2)"
  ana=$(pgrep -u armonia -o -x chromium)
  for p in $(pgrep -u armonia chromium); do
    tip=$(tr '\0' ' ' < /proc/$p/cmdline 2>/dev/null | grep -o -- '--type=[a-z-]*' | head -1)
    echo "  pid $p ${tip:-ana} userns:$( [ "$(readlink /proc/$p/ns/user)" != "$(readlink /proc/1/ns/user)" ] && echo AYRI || echo ana) pidns:$( [ "$(readlink /proc/$p/ns/pid)" != "$(readlink /proc/1/ns/pid)" ] && echo AYRI || echo ana) seccomp:$(awk '/^Seccomp:/{print $2}' /proc/$p/status 2>/dev/null)"
  done | head -12
  if [ -n "$ana" ]; then
    echo "tarayici /run/user (yalniz 3 soket): $(nsenter -t "$ana" -m ls -A /run/user/$(id -u armonia) 2>&1 | tr '\n' ' ')"
    echo "tarayici -> bus/postgresql/input (yok olmali): $(nsenter -t "$ana" -m sh -c "ls /run/user/$(id -u armonia)/bus /run/postgresql /dev/input 2>&1 | head -3" | tr '\n' ' ')"
    echo "tarayici gruplar: $(grep Groups /proc/$ana/status)"
  fi
  systemctl show 'armonia-tarayici@*' -p DevicePolicy -p IPAddressDeny 2>&1 | head -4 | tr '\n' ' '; echo
  echo "tarayici nft kural sayisi (ret): $(nft list chain inet filter tarayici_yerel 2>&1 | grep -c reject)"
  # nft kurali cgroup'a bakar: nsenter cgroup'u DEGISTIRMEZ (14 Kasim 13:10 "ACIK" yaniltici). Deneme tarayici
  # slice'inda gecici birimle: cgroup armonia.slice/armonia-tarayici.slice/run-*.service (seviye 2 eslesir)
  tdene() { timeout 15 systemd-run --quiet --wait --pipe --collect -p Slice=armonia-tarayici.slice -p User=armonia \
    /bin/bash -c "exec 3<>/dev/tcp/$1/$2 && echo ACIK" 2>&1 | tail -1; }
  echo "tarayici slice -> 127.0.0.1:5975 (ACIK olmali, e-imza): $(tdene 127.0.0.1 5975)"
  echo "tarayici slice -> 127.0.0.1:631 (reddedilmeli): $(tdene 127.0.0.1 631)"
  echo "tarayici slice -> 192.168.1.1:80 (reddedilmeli): $(tdene 192.168.1.1 80)"
  echo "tarayici slice -> avukat.uyap.gov.tr:443 (ACIK olmali): $(tdene avukat.uyap.gov.tr 443)"
  echo "oturum (armonia, dilimsiz) -> 127.0.0.1:5975 (24.11: reddedilmeli; imza yalniz armonia-imza ile): $(timeout 15 systemd-run --quiet --wait --pipe --collect -p User=armonia /bin/bash -c 'exec 3<>/dev/tcp/127.0.0.1/5975 && echo ACIK' 2>&1 | tail -1)"
  echo "nft sayaclari: $(nft list chain inet filter tarayici_yerel 2>&1 | grep -o 'packets [0-9]*' | tr '\n' ' ')"
  nft list ruleset 2>/dev/null | grep -n "tarayici_yerel" | head -3
  journalctl -b -u 'armonia-tarayici@*' --no-pager -o cat 2>&1 | grep -viE 'dbus|fontconfig' | tail -15
  sleep 2
fi
echo "--- e-imza (Adalet E-imza)"
for i in $(seq 1 30); do grep -q ':175F ' /proc/net/tcp /proc/net/tcp6 2>/dev/null && break; sleep 1; done
echo "servis: $(systemctl is-active armonia-eimza.service 2>&1)  pcscd.socket: $(systemctl is-active pcscd.socket 2>&1)  5975 dinleyen: $(awk '$4=="0A"' /proc/net/tcp /proc/net/tcp6 | grep -c ':175F ')"
# Java [::ffff:127.0.0.1]:5975 (tcp6) dinler; yalniz tcp okumak 0 gosteriyordu. Xvfb: soyut (@) X soketi OLMAMALI
echo "5975 adresi: $(ss -ltnH 'sport = :5975' 2>&1 | awk '{print $4}' | tr '\n' ' ')  Xvfb: $(pgrep -c -u eimza Xvfb)  Xvfb soyut X99 (0 olmali): $(ss -xlH | grep -c '@/tmp/.X11-unix/X99')  TCP 6000+ (0 olmali): $(ss -ltnH | grep -cE ':60[0-9][0-9] ')"
# guvenlik-denetci O-X1: labwc Depends xwayland -> masaustune X kapisi (X0) acik mi? Kim dinliyor, eimza baglanabiliyor mu
echo "X0 dinleyenler: $(ss -xlpH 2>/dev/null | grep -E 'X11-unix/X0( |$)' | awk '{print $5, $NF}' | tr '\n' ';')  Xwayland ikilisi: $(ls /usr/bin/Xwayland 2>/dev/null || echo yok)"
echo "soyut X0 soketi (varsa eimza/diger surecler masaustu X'ine ulasabilir): $(ss -xlH | grep -c '@/tmp/.X11-unix/X0')"
# O-X1 dogrulama (WLR_XWAYLAND=/bin/false): X11 kurulum istegi (LE, protokol 11.0, yetkisiz) gonder. Basarili X
# sunucusu ilk bayt 01 doner; bos yanit + Xwayland sureci yok = masaustune X kapisi KAPALI.
lw=$(pgrep -o -x labwc)
if [ -n "$lw" ]; then
  xy=$(printf 'l\000\013\000\000\000\000\000\000\000\000\000' | timeout 6 nsenter -t "$lw" -m -- runuser -u armonia -- \
    /usr/bin/nc.openbsd -U -w 3 /tmp/.X11-unix/X0 2>&1 | head -c 1 | od -An -tx1 | tr -d ' ')
  sleep 1
  echo "X0 baglanti denemesi: yanit ilk bayt='${xy:-bos}' (01 = X ACIK)  Xwayland sureci: $(pgrep -c -x Xwayland)  labwc hala calisiyor: $(pgrep -c -x labwc)"
fi
# D-X1: Xvfb/Java SysV paylasimli bellegi ortak IPC ad alaninda gorunmemeli (PrivateIPC=yes)
echo "eimza SysV shm (0 olmali): $(ipcs -m 2>/dev/null | awk -v u=eimza '$3==u' | wc -l)"
echo "surec: $(ps -o user=,rss=,args= -C java 2>/dev/null | cut -c1-80; pgrep -a -u eimza 2>&1 | cut -c1-100 | head -2)"
# Kart takili olmadan yanit (Hyper-V'de okuyucu yok): HTTP durum satiri gelmeli
if exec 3<>/dev/tcp/127.0.0.1/5975 2>/dev/null; then
  printf 'GET /api/v1/signature/getCertificates HTTP/1.0\r\nHost: localhost:5975\r\n\r\n' >&3
  # Yalniz HTTP durum satiri: govde sertifika (ad, TCKN) icerebilir -> seri porta yazilmaz (guvenlik-denetci E-K2)
  echo "getCertificates: $(timeout 15 head -n 1 <&3 | tr -d '\r')"; exec 3>&-
else echo "getCertificates: BAGLANAMADI"; fi
journalctl -b -u armonia-eimza --no-pager -o cat 2>&1 | grep -viE '^\s+at |DEBUG|pin|serial|tckn|seri|kimlik|subject|CN=' | tail -12
echo "--- ses (pipewire)"
echo "kullanici birimleri: $(systemctl --machine=armonia@ --user is-active pipewire.socket pipewire-pulse.socket wireplumber.service 2>&1 | tr '\n' ' ')"
echo "pulse soketi: $(ls -l /run/user/$(id -u armonia)/pulse/native 2>&1)  ses kartlari: $(cat /proc/asound/cards 2>&1 | head -2 | tr '\n' ' ')"
echo "--- usbguard"
echo "servis: $(systemctl is-active usbguard.service 2>&1)  kural sayisi: $(usbguard list-rules 2>&1 | wc -l)  aygitlar: $(usbguard list-devices 2>&1 | wc -l)"
usbguard list-rules 2>&1 | head -8
echo "kullanici IPC (reddedilmeli): $(runuser -u armonia -- usbguard list-devices 2>&1 | head -1)"
echo "izinler: $(stat -c '%a %U %n' /etc/usbguard /etc/usbguard/rules.conf /etc/usbguard/usbguard-daemon.conf 2>&1 | tr '\n' ' ')"
systemd-analyze security usbguard.service 2>&1 | tail -1
systemd-analyze security armonia-tarayici@x.service 2>&1 | tail -1
journalctl -b -u usbguard --no-pager -o cat 2>&1 | tail -5
echo "--- apparmor"
echo "etkin: $(cat /sys/module/apparmor/parameters/enabled 2>&1)  lsm: $(cat /sys/kernel/security/lsm 2>&1)"
[ -f /run/armonia-lynis.txt ] && { echo "--- lynis (canli sistem)"; head -60 /run/armonia-lynis.txt; }
echo "--- squashfs / toram"; grep -o 'toram' /proc/cmdline; df -h / /run/live/medium 2>/dev/null
echo "--- oturum gunlugu"; journalctl -b -u greetd --no-pager 2>&1 | tail -30
echo "--- hatalar"; journalctl -b -p err --no-pager 2>&1 | tail -20
echo "=====ARMONIA-OLCUM-SONU====="
