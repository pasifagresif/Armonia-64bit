# Çoklu UYAP katmanı

19.11.2026 kullanıcı kararı: "İşletim sistemi başlar başlamaz Rust kodu çalışmasın. Önce işletim sisteminin ana
çatısını kuralım; Çoklu UYAP'ı sonra ekleriz."

Bu klasördeki dosyalar eski **kiosk** düzenine aittir (açılışta `uyap_app`) ve `iso_derle.sh` tarafından imaja
**kopyalanmaz**:

| Dosya | Eski yeri | Not |
|---|---|---|
| `etc/systemd/system/armonia-kiosk.service` | `config/includes.chroot/...` | tty1'de labwc + uyap_app |
| `etc/systemd/system/armonia-kiosk.service.d/10-sandbox.conf` | `sertlestirme/...` | uyap_app kum havuzu (systemd) |
| `etc/apparmor.d/opt.armonia.cokluuyap.uyap_app` | `sertlestirme/...` | AppArmor profili (complain) |
| `opt/armonia/bin/kiosk_oturum.sh` | `config/includes.chroot/opt/armonia/bin/oturum.sh` | uyap_app yeniden başlatma döngüsü |
| `hooks/normal/0200-cokluuyap-postgresql.hook.chroot` | `0100-armonia.hook.chroot` içindeki "Faz 2 PostgreSQL" bloğu | Debian kümesini sil + doğrula, servisleri maskele, her cihazda aynı **snakeoil anahtarını sil** (denetçi O2, O3, D1). 23.11'de geri kondu. |
| `package-lists/cokluuyap.list.chroot` | `config/package-lists/armonia.list.chroot` | `postgresql` |

Kasa (LUKS2 + PostgreSQL) katmanı da Çoklu UYAP içindi: `../kasa/` (imaja girmiyor).

## Geri bağlanırken (plan: `docs/claude-notlar/planlar/armonia-herkes-icin.md`)
- `uyap_app` açılışta değil, **Ayarlar'dan** açılır ("Çoklu UYAP kullanıyorum").
- Kiosk birimi yerine kullanıcı oturumunda ayrı, kum havuzlu bir birim; sonsuz yeniden başlatma yok.
- `10-sandbox.conf` ve AppArmor profili yeni birime uyarlanır; yol `/opt/armonia/cokluuyap/uyap_app` aynı kalabilir.
- Tarayıcı politikasına `127.0.0.1:8800`, nft `tarayici_yerel` kurallarına 8800 eklenir.
- `hooks/normal/*` → `config/hooks/normal/`, `package-lists/*` → `config/package-lists/` kopyalanır. PostgreSQL
  kancası `0100-armonia`'dan sonra çalışır; soket erişimi verilecek kullanıcı `COKLUUYAP_KULLANICI` ile seçilir.
  `armonia` masaüstü kullanıcısı kum havuzunda olmadığı için **postgres grubuna eklenmemeli** (bütün oturum sokete erişir).

## 23.11 — menüden açılan düzen (`menuden/`, imaja GİRER)

Kullanıcı kararı 23.11: "Açılışta otomatik başlamasın, kullanıcı menüden açsın. Linux (Rust) hâli çalışsın."
Kararlar: ISO'da veri RAM'de (kapanınca silinir; kalıcılık G2 kasasıyla), PostgreSQL şimdilik yok.

- Derleme: `derle_uyap.sh` (normal kullanıcı; x86_64 → ISO, Pi'de aarch64 → Pi). `iso_derle.sh` / `pi_derle.sh`
  `COKLUUYAP=1` (varsayılan) ile `../cokluuyap_hazirla.sh`'yi çağırır: çıktı yoksa derler, ELF mimarisini denetler,
  `/opt/armonia/cokluuyap` (root, salt okunur) + `menuden/includes.chroot` + `0210` kancası. `COKLUUYAP=0`: katmansız.
- Başlatma: menü/dock "Çoklu UYAP" → `cokluuyap_ac.sh` → `systemctl start armonia-cokluuyap.service`
  (polkit `60-armonia-cokluuyap.rules`: yalnız bu birim, yalnız start/stop, yalnız yerel etkin oturumdaki `armonia`).
  Birimde `[Install]` yok; kanca ve `iso_derle.sh` açılışta başlatan bir şey olmadığını denetler. Çökünce 5 dakikada
  en fazla 3 deneme.
- Kum havuzu (systemd, tarayıcı birimiyle aynı çizgi): ev dizininde yalnız `~/.local/share/armonia-cokluuyap` (veri)
  ve `~/Belgeler/UYAP` (evrak); `/run/user` içinde yalnız Wayland soketi (D-Bus / systemd --user yok); pcscd, klavye
  aygıtları, PostgreSQL soketi yok; yetenek yok, yeni ad alanı yok.
- Ağ: kendi dilimi (`armonia-cokluuyap.slice`) ve `cokluuyap_yerel` nft zinciri: yerelde yalnız DNS, 5975 (e-imza),
  8800 (kendi vekili); yerel ağ kapalı; internet açık. Tarayıcı 8800'e gidebilir (`30-cokluuyap.conf`, politikada
  `http://127.0.0.1:8800`). **Masaüstü oturumunun geri kalanı 8800'e gidemez** (vekilde UYAP oturumu var).
  Giriş zinciri kapalı: `0.0.0.0:8800` dinlemesi LAN'dan erişilemez (ofis paylaşımı bu düzende kapalı).
- AppArmor: `menuden/includes.chroot/etc/apparmor.d/` (complain; enforce G7'de).
- Vekilin yanıtları: SYN-ACK yarı kurulmuş sokettir, `socket cgroupv2` ile eşleşmez. Bu yüzden girişte
  `cokluuyap_giris` zinciri, dinleyicisi Çoklu UYAP diliminde olan yeni 8800 bağlantısını `ct mark 0x8800` ile
  işaretler; çıkışta yalnız bu işaretli bağlantının yanıtları geçer. Oturumda açılan sahte bir 8800 dinleyicisi
  işaret alamaz (denetçi 23.11 K3). DOĞRULANMADI, denemede bakılacak.
- Oturum kapanınca `oturum.sh` birimi durdurur (denetçi O2).

### Bilinen sınırlar (23.11 denetimi)
- **O1:** "Oturum 8800'e gidemez" yalnız ağ yolu için geçerli. Veri klasörü (`accounts.json`, `zs.sqlite3`,
  `static_cache/`, LAN TLS anahtarı) aynı kullanıcının (`armonia`) kum havuzsuz oturumundan okunup yazılabilir. Tam
  çözüm: `uyap_app` ayrı bir kullanıcıyla çalışır (veri `/var/lib`'de 0700, Wayland soketine ACL). Kullanıcı kararı
  bekliyor.
- **D4:** İnternette bütün kapılar açık (tarayıcı birimi 80/443 ile sınırlı). WebRTC/STUN/TURN ihtiyacı netleşince
  daraltılacak.
- **D5:** LAN kapalı olduğu için ofis paylaşımı, LAN TLS (8843 ve iç köprüsü), 8801 yardımcısı ("Paylaş") ve UDP 4430
  keşfi bu düzende çalışmaz.

### Deneme komutları
- `systemctl status armonia-cokluuyap; journalctl -u armonia-cokluuyap -b | tail -30`
- Tarayıcı dilimi 8800'e gidebilmeli, oturum gidememeli:
  `systemd-run --wait --pipe -p User=armonia -p Slice=armonia-tarayici.slice curl -sI http://127.0.0.1:8800/`
  (yanıt gelmeli); aynı komut `-p Slice=...` olmadan reddedilmeli.
- `nft list chain inet filter cokluuyap_giris; nft list chain inet filter cokluuyap_yerel`
- Uygulama açıkken 8800 bağlantıları işaretli olmalı: `conntrack -L -p tcp --dport 8800` (`mark=34816`; araç yoksa
  `grep 'dport=8800' /proc/net/nf_conntrack`).
- Sahte vekil reddedilmeli: uygulama kapalıyken oturumdan `nc -l 127.0.0.1 8800`, sonra yukarıdaki tarayıcı dilimi
  `curl` komutu bağlantı kuramamalı.
- Çıkış yap → `systemctl is-active armonia-cokluuyap` "inactive" olmalı.
- Açılışta çalışmıyor olmalı: yeniden başlat → `systemctl is-active armonia-cokluuyap` "inactive".
- Bu klasörün kökündeki `etc/`, `opt/`, `hooks/`, `package-lists/` eski kiosk düzenine ve PostgreSQL'e aittir; imaja
  girmez.
