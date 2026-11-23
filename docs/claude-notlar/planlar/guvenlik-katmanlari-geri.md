# Güvenlik katmanlarını geri getirme + LUKS2 (23.11.2026)

Kullanıcı (23.11): "LUKS2 olsun. İşletim sisteminin güvenli olması için uğraştık o kadar. İlk versiyonda bir çok
güvenlik katmanı oluşturmuştuk, hepsi gitmiş, lütfen dikkat et."

Karşılaştırma: `59fb985` (kiosk, son sıkı sürüm) ↔ `2a6297e` (temel masaüstü + Pi). Durum: ✅ yerinde,
🔁 katmana taşındı, ⚠️ gevşedi, ❌ yok oldu.

## 1. Envanter

| # | Katman (59fb985) | 2a6297e'de | Not |
|---|---|---|---|
| 1 | Çekirdek parametreleri (lockdown=integrity, slab_nomerge, debugfs=off, ia32_emulation=0…) | ✅ | Pi'de ia32 çıkarılıyor (anlamsız) |
| 2 | sysctl (`90-armonia.conf`), modprobe kara liste | ✅ | |
| 3 | nftables: giriş kapalı, forward drop | ✅ | + yeni `oturum_yerel` zinciri |
| 4 | live-config purge, herkese-yazılabilir dosya denetimi, test bileşeni denetimi, DNS denetimi | ✅ | |
| 5 | Chromium politikası: oturum açma/eşitleme/parola yöneticisi/eklenti/misafir kapalı | ✅ | Silinen anahtar yok |
| 6 | Tarayıcı ayrı birimde, `/run/user` içinde yalnız 3 soket, D-Bus/systemd --user yok, `/dev/input` ve pcscd kapalı | ✅ | |
| 7 | `uyap_app` systemd kum havuzu (`10-sandbox.conf`), AppArmor profili, kiosk birimi | 🔁 | `cokluuyap-katmani/` (değişmeden) |
| 8 | PostgreSQL sertleştirmesi (küme silme, snakeoil, servis maskesi) | 🔁 | 23.11'de katmana geri kondu |
| 9 | `toram` | ✅ | 23.11'de geri eklendi |
| 10 | **Oturum kum havuzu**: kiosk = `nologin` sistem kullanıcısı, NoNewPrivileges, seccomp, boş yetki seti, RestrictNamespaces, ProtectSystem=strict | ⚠️ | Masaüstü oturumu hiçbir kısıtsız; `armonia` kabuğu bash |
| 11 | **LUKS2 kasa** (Plymouth parolası, TRIM kapalı, nodev/nosuid/noexec, fsck) | ❌ | `kasa/` var ama ISO'ya girmiyor; Pi'de disk şifresiz ve kalıcı |
| 12 | **Tarayıcı ağı**: yalnız 127.0.0.1:8800, `IPAddressDeny=any` | ⚠️ | İnternet 80/443 açık; site süzgeci yalnız Chromium politikası |
| 13 | **Tarayıcı DNS'i kapalı** (DNS tüneliyle sızdırma yok) | ⚠️ | Açık, her alan adı çözülür |
| 14 | **Tarayıcı → 5975 (e-imza) yasak** (E-Y1) | ⚠️ | Ağ katmanında bütün tarayıcıya açık |
| 15 | **USBGuard sıkı A** (bellek/yazıcı/tarayıcı/ff/ef ret, sonda `reject`) | ⚠️ | "Takınca sor"; Y1 (sanal klavye onaylatabilir), D8 açık. **Pi'de tamamen kapalı** |
| 16 | **AppArmor** (tek profil, complain) | ⚠️ | Temel imajda **sıfır profil** |
| 17 | Kiosk labwc: kısayol yok, kök menü yok | ⚠️ | Sınırlı kısayol; terminal yok. Thunar "Birlikte aç → özel komut" denetlenmeli |
| 18 | Yetkili yardımcı yoktu | yeni | `pkexec ayar-yardimci`, polkit `allow_active=yes` → **parola sormuyor** |
| 19 | Giriş: kiosk otomatik, veri kasada | ⚠️ | Otomatik giriş; Pi'de `/etc/armonia/deneme` → parola ve ekran kilidi yok |
| 20 | `olcum.sh` güvenlik doğrulamaları (kasa, AppArmor, PG, X0, IPC reddi) | ⚠️ | Kasa/AppArmor/IPC denetimleri gitti |

## 2. Plan

### G1 — Pi: LUKS2 kök bölüm (öncelik)
- İmaj şifresiz üretilir (içinde kullanıcı verisi yok). **İlk açılışta initramfs** parolayı iki kez sorar ve kök
  bölümü yerinde şifreler: `cryptsetup reencrypt --encrypt --type luks2 --reduce-device-size 32M`.
  → Her cihazın **kendi ana anahtarı** olur. (Derlemede şifreleyip sonra parola değiştirmek ana anahtarı
  değiştirmez; aynı imajdan yazılan her bellek aynı anahtarı taşırdı. Bu yüzden o yol seçilmedi.)
- Şifre: işlemcide AES varsa (Pi 5) `aes-xts-plain64`, yoksa (Pi 4) `xchacha12,aes-adiantum-plain64`.
  PBKDF argon2id. TRIM kapalı (kasa D1 kararı).
- Elektrik kesilirse: LUKS2 yeniden şifreleme kaldığı yerden devam eder (`cryptsetup repair` + yeniden başlatma).
- `crypttab` + `cryptsetup-initramfs`, `cmdline.txt` → `root=/dev/mapper/armonia-kok`.
- `/etc/armonia/deneme` kaldırılır → ekran kilidi ve parola Pi'de çalışır.
- Doğrulama: `olcum.sh`'a `cryptsetup status`, şifre, ana anahtarın cihaza özgü olduğu (iki bellek karşılaştırması).

### G2 — ISO: LUKS2 kasa geri (kalıcı veri)
- `kasa/` yeniden bağlanır: `PARTLABEL=armonia-kasa` LUKS2 bölümü, Plymouth parolası, `nodev,nosuid,noexec`.
- Kasada: Belgeler, İndirilenler, tarayıcı profili, Wi-Fi bağlantıları. Kasa yoksa her şey RAM'de (bugünkü gibi).
- Eksik olan **kasa oluşturma akışı** (eskiden de yoktu, yol haritası D.4-4): Ayarlar → "Şifreli kasa oluştur"
  (`ayar-yardimci` üzerinden; disk seçimi, iki kez parola, onay).
- Diske tam kurulum (A6, Calamares, LUKS2 tam disk) bu planın dışında, sonra.

### G3 — Giriş ve yetki
- Kullanıcı parolası ilk açılışta belirlenir (Pi: LUKS parolasıyla birlikte; ISO: kasa oluşturulurken).
- polkit `tr.armonia.ayar`: `allow_active=yes` → `auth_self_keep` (arka plandaki süreç sessizce root yardımcıyı
  çalıştıramaz).

### G4 — Oturum kum havuzu (10)
- `pkexec` setuid olduğu için oturuma NoNewPrivileges konamıyor. `ayar-yardimci` tarayıcı soketi gibi root'un
  açtığı bir sokete taşınır (istek doğrulanır), `pkexec` kaldırılır.
- labwc oturumu `setpriv --no-new-privs` + systemd kapsamı ile başlar; oturumdan setuid ile yetki yükseltme biter.
- Thunar "özel komutla aç" kapısı kapatılır / denetlenir.

### G5 — Tarayıcı ağı (12–14)
- Tarayıcı için **izin listeli DNS**: tarayıcı diliminin 53 trafiği yerel bir süzgeç çözücüye (dnsmasq) yönlenir;
  yalnız izin listesindeki alan adları çözülür. → DNS tüneli kapanır, URL izin listesi ağ katmanında da uygulanır.
- 5975: UYAP imzası için gerekli; ağ katmanında açık kalır. Chromium yerel ağ erişimi kısıtları
  (`LocalNetworkAccessAllowedForUrls` + kısıtların zorunlu tutulması) gerçek denemede doğrulanır; başka sitenin 5975'e
  erişemediği `olcum.sh` ile ölçülür.

### G6 — USB (15)
- Pi'de USBGuard yeniden açılır: ilk açılışta kök disk olan belleğin kimliği/seri numarası için `allow` kuralı
  yazılır (sistemin kendi diskini düşürme sorunu böyle çözülür).
- Y1: soru oturum dışında (greetd/VT) ya da "çıkar–tak" ile onay. D8: açılışta takılı aygıtlar da sorulur.

### G7 — AppArmor (16)
- Temel imaja profiller: Debian `apparmor-profiles(-extra)` (evince, LibreOffice…) + kendi profillerimiz
  (armonia-ayarlar, hukuk_hesap, ayar-yardimci, tarayıcı betiği). Önce complain → ölçüm → enforce.
- Kanca artık "imajda olmayan programların profilini sil" yapar, hepsini değil.

### G8 — Doğrulama
- `olcum.sh` güvenlik bölümü: LUKS, USBGuard, AppArmor sayıları, polkit, DNS süzgeci, 5975, NoNewPrivileges.
- Her aşamadan sonra `guvenlik-denetci` denetimi.

### Kararlar (23.11, kullanıcı)
- Parola: **tek parola** (LUKS2 disk parolası = `armonia` kullanıcısı; ekran kilidi bununla). Kural 14.11 kararı:
  ≥10, yalnız ASCII, büyük + küçük harf + rakam + sembol (`kasa_parola_kurali.sh`).
- Sıra: G1 → G8 hepsi. USB: "takınca sor" kalır, sertleştirilir (G6).
- Yol haritasından (ilk güvenlik belgesi) bu plana eklenenler: **SSH yok** (§5 karar 4 → Pi OS Lite'ın
  openssh-server/rpi-connect'i silinir), cihaza özgü machine-id/rastgele tohum, kurtarma anahtarı + LUKS başlık
  yedeği (açık karar), giden beyaz liste/kill-switch, bubblewrap, Faz 6 (dm-verity, Secure Boot, imzalı güncelleme).

### G1 durumu (23.11)
- Yazıldı: `pi/initramfs/{hooks,scripts/local-top}/armonia-ilk-sifreleme`, `pi/sifreleme_son.sh`,
  `pi/armonia-sifreleme-son.service`, `pi_derle.sh` (SSH/rpi-connect silme, cihaza özgü kimlik, ` resize` kapatma,
  TR klavye önbelleği, `systemd.run` üreteci kapalı, initramfs + firmware initramfs denetimi).
- guvenlik-denetci: **DÜZELTMEYLE KULLANILABİLİR** → K1 (yarıda kalınca parola/crypttab yok), O1 (trixie ` resize`,
  aynı PARTUUID'li ikinci disk), O2 (firmware initramfs denetimi), O3 (GB klavye önbelleği), O4 (dm_crypt modülü),
  O5 (süre mesajı), O6 (yanlış parola/kurtarma ayrımı), O7 kısa vadeli (systemd.run), D1 (tries=0), D9 düzeltildi.
- Açık: D3 (`:` içeren parola + chpasswd denemesi), D5 (tek parolayı birlikte değiştiren araç → G3; kurtarma anahtarı
  ve başlık yedeği kararı), D7 (Raspberry Pi Imager "özelleştirme" KULLANILMAZ), O7 asıl çözüm Faz 6.
- **Zorunlu deneme (kullanıcı, boş bellek):** ilk açılış; şifreleme sırasında ~1 dk sonra fiş çekme → devam;
  `findmnt /`, `cat /etc/crypttab`, `passwd -S armonia` (P), `cat /boot/firmware/cmdline.txt`,
  `lsinitramfs /boot/firmware/initramfs* | grep -c cryptroot/crypttab`; parolada `i ş " @ * ,` → ekran kilidi.

### G9 — Windows uygulamaları (Bottles) — kullanıcı isteği 23.11
- Yalnız ISO (x86; Pi'de ARM64 Flatpak yok). Amaç: ağdaki **İcramatik** (paylaşımlı klasörde). Kullanıcı:
  "Armonia ağdaki bu İcramatik'in olduğu klasöre erişsin ama ağa kapalı olsun."
- Taslak: paylaşım klasörü sistem (root) tarafından bağlanır (kimlik bilgisi root'a özel, kasada); nft yalnız o
  sunucunun 445'ine izin verir; Bottles (Flatpak/bubblewrap) ağsız (`--unshare=network`), ev dizinsiz, yalnız o
  klasör + "Windows Dosyaları". **İcramatik paylaşılan klasörde** (kullanıcı 23.11) → Windows tarafı tamamen ağsız.
- Kullanım (kullanıcı 23.11): masaüstünde kısayol; tıklanınca ağ konumundaki (192.x.x.x) klasörden açılır.
  Kullanıcı isterse Office uygulamaları da kurulabilir.

### G3/G6 durumu (23.11)
- G3: `lxpolkit` (paket + labwc autostart); Pi'de `tr.armonia.ayar` → `auth_self_keep`. ISO canlıda `yes` kalır
  (kullanıcı parolası yok; G2 kasa ile birlikte değişecek). Tek parolayı birlikte değiştiren araç (LUKS + kullanıcı)
  henüz yok → Ayarlar'da parola değiştirme de yok, eşzamansızlık yolu açılmadı.
- G6: `usb_sor.sh` fiziksel onay (izin ver → 2 dk içinde çıkar → 20 sn içinde tak, aynı tanımlayıcı özeti) → Y1 ve D8
  kapanır (ISO + Pi). Pi: `usb_kok_izin.sh` + `armonia-usb-kok.service` (usbguard'dan önce kök disk için dar kural),
  USBGuard Pi'de yeniden açık.

### 2. tur denetim (23.11) ve düzeltmeler
- G1 denemeye hazır. G1-a (bayrak kipinde çift parola / her açılışta e2fsck), G1-b (debug-shell maskeli), G3-c
  (lxpolkit yeniden başlatma), G6-a (**fiziksel onayda başka yuva şartı** + 2 sn alt sınır + yeni hub reddi;
  BadUSB kendini yazılımla çıkarıp takabiliyordu), G6-b (politika yarışı), G6-c (kök disk kuralı seçimi + yedek
  kural; usbguard hiç başlamazsa tüm USB serbest kalıyordu), G6-d düzeltildi.
- Açık/belgelenen: G3-a **tek parola + sahte parola penceresi** → oturumdaki kötü süreç disk parolasını alabilir
  (Y1 sınıfı; çözüm G4: parola sorusunu oturum dışına taşımak ya da ayrı disk parolası). G1-c/d/e doğrulama
  denemelerinde bakılacak. `init=` ile atlatma Faz 6'ya kadar açık.

### G9 durumu (23.11)
- Yazıldı: `armonia-linux/windows/` (BENIOKU.md), `iso_derle.sh` `WINDOWS=1` (varsayılan). Debian wine 32+64,
  bubblewrap (ağsız), cifs misafir bağlama (root), dock'ta "İcramatik". ISO'da `ia32_emulation=0` kaldırıldı
  (kullanıcı kararı; İcramatik 32 bit, trixie Wine eski WoW64). `WINDOWS=0` tam sertleştirilmiş ISO.
- Derlemeden önce `windows/includes.chroot/etc/armonia/windows.conf` (PAYLASIM, PROGRAM) doldurulmalı.

### 3. tur denetim (23.11) ve düzeltmeler
- KRİTİK A-1 (kök disk betiği `with-connect-type`'ı arayüz sanıyordu → usbguard hiç başlamaz, tüm USB açık) ve
  KRİTİK B-1 (Wine ana Wayland soketini görüyordu: sanal klavye, pano, ekran görüntüsü; EXE herkese yazılabilir
  paylaşımda) düzeltildi: kök diske her durumda tam kural; Wine iç içe `cage` kafesinde + isteğe bağlı SHA256.
- A-2 (içinde hub taşıyan BadUSB: ebeveyn ≥10 sn önce görülmüş olmalı), A-3, B-2 (wineserver -w), B-3 (userns
  kapalı, GPU yok), B-4 (username=guest), B-5 (RuntimeDirectory), B-6/B-7 düzeltildi/belgelendi.
- Bilinçli gevşemeler belgede: ia32 tüm süreçlere açık; masaüstüne yazılabilir LAN klasörü (oturum_yerel delindi).

### 4. tur denetim (23.11): KRİTİK yok
- O-1 düzeltildi: `/etc/armonia/windows.sha256` listesi → program klasöründeki tüm exe/dll/ocx/bpl… doğrulanır,
  listede olmayan çalıştırılabilir dosya varsa açılmaz (yalnız-EXE SHA256 zayıf seçenek olarak kaldı, uyarıyla).
  Kalan: doğrulama–yükleme arası yarış (etkisi cage + bwrap içinde).
- D-1 (sonradan takılan hub üzerinden onay reddi), D-2 (`generate-policy -p` + her zaman yuva süzgeci), D-4
  (`sec=none` ikinci deneme) düzeltildi. D-3 (cage'in kendisi kafessiz; ileride bwrap + security-context-v1),
  D-5 (cage tek pencere; İcramatik diyaloglarıyla denenecek) açık.

## Gelecek (bu planın dışında)
- **Armonia belge düzenleyicisi** (kullanıcı 23.11): docx ve türevleri + UDF açar, biçimler arasında kolay ve
  kayıpsız çevirir. Mevcut parçalar: `ruyap/src/udf_pdf.rs`, `udf_converter.rs`, `python/udf_pdf.py`. Ayrı plan yazılacak.

## 3. Kısıtlar
- Bu makinede (Windows) Pi ve ISO derlenip denenemiyor: derleme ve açılış denemesi kullanıcının Linux makinesinde.
  Her aşama betik + denetim + kullanıcının deneme çıktısıyla kapanır.
- G1 ve G2 veri kaybı riski taşır (yerinde şifreleme, disk biçimlendirme): önce boş bellek/diskle denenir.

## Çoklu UYAP menüden (23.11)
Kullanıcı kararı: açılışta başlamasın, menüden açılsın, Linux (Rust) sürümü; ISO'da veri RAM'de; PostgreSQL yok.
Ayrıntı: `armonia-linux/cokluuyap-katmani/BENIOKU.md` ("23.11 — menüden açılan düzen").
Denetim (23.11): K1 (ISO'da uyap_app 0644), K2 (WorkingDirectory yok → 200/CHDIR), K3 (SYN-ACK oturum_yerel'de
reddediliyordu → ct mark 0x8800) düzeltildi; O2 (çıkışta birim durmuyordu) ve D1–D3 düzeltildi. Açık: O1 (veri
klasörü aynı UID'de, ayrı kullanıcı kararı), D4 (internet kapıları), D5 (LAN özellikleri kapalı).
Sırada: imzalı güncelleme tasarımı (kullanıcı isteği 23.11: uygulamada uyarı, doğrulayıp indirme, sahte güncelleme
yüklenemesin; açık/gizli anahtar).
