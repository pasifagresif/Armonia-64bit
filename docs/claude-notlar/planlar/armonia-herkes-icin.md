# Plan: Armonia'yı her hukukçunun kurabileceği sisteme çevirmek (2026-11-19)

Durum: **ONAYLANDI (19.11), sıra değişti.** Kullanıcı: "İşletim sistemi başlar başlamaz Rust kodu çalışmasın.
Önce işletim sisteminin ana çatısını kuralım; Çoklu UYAP'ı sonra ekleriz."

**Yeni sıra:**
1. **Temel sistem (Rust yok):** A1 (giriş + oturum), A2 (masaüstü), A4 (tarayıcı, e-imza, ofis). `uyap_app`, kasa,
   PostgreSQL ve `uyap_app` AppArmor profili imaja girmez; dosyaları `cokluuyap-katmani/` ve `kasa/` altında rafta.
2. A5 (USB takınca sor), A3 (Ayarlar), A6 (Calamares kurulum), A7 (denetim).
3. **En son:** Çoklu UYAP katmanı — Ayarlar'dan açılan, kum havuzlu kullanıcı birimi + kasa/PostgreSQL.

## Amaç
Çoklu UYAP üyesi olsun ya da olmasın, her hukukçu Armonia'yı indirip kurabilsin ve kullanabilsin. Bugünkü imaj bir
kiosk: açılışta doğrudan `uyap_app` başlıyor (`oturum.sh` sonsuz döngü), giriş kurulamayınca ekran işe yaramıyor.

## Kullanıcı kararları (19.11)
| Konu | Karar |
|---|---|
| Dağıtım | Canlı USB + "Diske Kur" (Calamares) |
| Masaüstü | Faz 5 kararları geçerli: labwc, alt dock, "A" menü, eş yükselti arka plan, simge yok |
| Uygulamalar | Chromium, Adalet E-imza, LibreOffice + PDF |
| Çoklu UYAP | İmajda var; **Ayarlar'dan açılır** ("Çoklu UYAP kullanıyorum" → açılışta başlar). Varsayılan kapalı |
| Sertleştirme | Bugünkü kadar sıkı |
| Tarayıcı | **Yalnız adli siteler** (izin listesi); e-imza 5975'e izin |
| USB | **Takınca sor**: klavye/fare/kart okuyucu kendiliğinden kabul, gerisi için masaüstünde İzin ver / Reddet |

## Aşamalar

### A1 — Açılış ve oturum (kiosk → masaüstü)
- `armonia-kiosk.service` → `armonia-oturum` düzeni: giriş ekranı **greetd + gtkgreet** (Wayland, hafif). Canlı
  sistemde otomatik giriş ("Armonia Deneme" kullanıcısı); kurulu sistemde kullanıcı + parola.
- `oturum.sh`: `uyap_app` döngüsü kalkar; yerine masaüstü bileşenlerini başlatır. `uyap_app` yalnız ayar açıksa,
  ayrı bir `systemd --user` birimi (`armonia-cokluuyap.service`) olarak ve **çökünce sınırlı yeniden deneme** ile.
- Kiosk birimindeki kum havuzu ayarları (`10-sandbox.conf`) Çoklu UYAP birimine taşınır; masaüstü oturumu ayrı.
- AppArmor `uyap_app` profili aynı kalır (yol değişmiyor).

### A2 — Masaüstü (Çoklu UYAP'tan bağımsız)
- Arka plan: `swaybg` + üretilen eş yükselti görseli (duvar kağıdı seçimi hâlâ açık: adaçayı degrade / kendi görseliniz).
- Üst çubuk: `waybar` — saat/tarih, ağ, ses, pil, e-imza kartı durumu (pcscd), Çoklu UYAP durumu (açıksa).
- Alt dock: ikinci `waybar` — solda "A", ortada sabit programlar (Tarayıcı, UYAP Avukat, Belgelerim, Yazı/Ofis,
  Ayarlar), sağda açık pencereler (`wlr/taskbar`: tıkla öne getir, orta tık kapat).
- "A" menü: `fuzzel` (temalı, Türkçe uygulama adları).
- Dosya yöneticisi: `pcmanfm-qt` ya da `thunar` (karar uygulamada, boyut ölçülerek).
- labwc `rc.xml`: kısayollar sınırlı açılır (Alt+Tab, menü tuşu, ekran kilidi); terminal kısayolu YOK.
- Ekran kilidi: `swaylock` + `swayidle` (süre: yol haritası D.4-8 açık kararı → varsayılan 10 dk öneri).
- Widget'lar (arama, İş Listesi, takvim) **A2'ye girmez**; ayrı küçük program gerektirir → sonraki aşama.

### A3 — Ayarlar
- Küçük "Armonia Ayarlar" penceresi: Çoklu UYAP aç/kapat, klavye (TR-Q/TR-F), ağ (Wi-Fi), ekran kilidi süresi,
  USB izinli aygıtlar listesi. Ayrıcalık gerektirenler polkit kuralıyla tek tek açılan küçük yardımcılar üzerinden
  (genel `sudo` yok).
- Teknoloji: GTK4 + Rust (ruyap ile aynı dil) ya da egui. Uygulamada karar; öneri egui (mevcut tema kodu).

### A4 — Uygulamalar ve tarayıcı
- Chromium: mevcut `armonia-tarayici@` kum havuzu korunur. Değişenler:
  - `xdg-open` yalnız 8800 değil, izin listesindeki adresleri kabul eder; dock'tan doğrudan da açılır.
  - Politika `URLAllowlist`: `*.uyap.gov.tr`, `*.adalet.gov.tr`, `turkiye.gov.tr` + `*.turkiye.gov.tr`,
    `resmigazete.gov.tr`, `mevzuat.gov.tr`, `*.barobirlik.org.tr`, `*.yargitay.gov.tr`, `*.danistay.gov.tr`,
    `*.anayasa.gov.tr`, `127.0.0.1:8800`. Liste tek dosyada; eksik site çıkınca eklenir. (Kesin alan adları
    gerçek denemede doğrulanacak — OAuth/CDN yönlendirmeleri.)
  - Birim `IPAddressDeny=any` kalkar: dışarı 80/443 açılır, **yerel ağ ve 8800 dışı yerel kapılar kapalı**,
    5975 (e-imza) açılır. DNS için resolved soketi açılır. nftables `tarayici_yerel` buna göre güncellenir.
  - Yer imleri: UYAP Avukat Portalı, e-Devlet, Resmî Gazete, Mevzuat.
- Adalet E-imza: birim aynı; yalnız Çoklu UYAP'a değil tarayıcıya da hizmet eder. Durum waybar'da.
- LibreOffice (writer, calc, `-gtk3`, `-l10n-tr`) + PDF: `evince` ya da `zathura`.
  Yazdırma: `cups` (+ `printer-driver-*` seçilmiş sürücüler) — USB yazıcı "takınca sor" ile gelir.
- UDF açıcı / UYAP Editörü: kapsam dışı (Faz 4'te ayrı karar).

### A5 — USB "takınca sor"
- USBGuard kuralları: HID (klavye/fare, tek arayüzlü), CCID (kart okuyucu) kabul; **HID+depolama birleşik aygıt
  ret** (klavye taklidi saldırısı). Diğerleri için `ImplicitPolicyTarget=block` (reject değil).
- Küçük bildirim yardımcısı: USBGuard IPC'yi dinler, masaüstünde "İzin ver (bu kez / her zaman) / Reddet" sorar.
  IPC erişimi yalnız bu yardımcıya (`IPCAllowedUsers` + ayrı grup); `usbguard-dbus` maskeli kalır.

### A6 — Kurulum (Calamares) ve kalıcılık
- Canlı imajda "Diske Kur" simgesi. Calamares: Türkçe, LUKS2 tam disk şifreleme zorunlu, kullanıcı oluşturma.
- Kurulu sistemde: `toram` yok; kasa (LUKS bölüm) yerine tam disk şifreleme; PostgreSQL yalnız Çoklu UYAP açıksa.
- `unattended-upgrades` (yalnız Debian güvenlik deposu) kurulu sistemde açık; canlıda kapalı.
- `live-config` purge kararı korunur; kurulu sistem kullanıcısının `sudo` yetkisi yok (yönetim: Ayarlar + polkit).
- Canlı imajda `toram` 19.11'de kaldırılmıştı (imaj ~1,5–2 GB); **23.11 kullanıcı kararıyla geri eklendi** (açılıştan sonra USB çıkarılabilir, medya değiştirilemez). Bedel: RAM'de imaj boyu kadar yer tutar; 4 GB RAM'li makinede sıkışabilir.
- UEFI: `grub-efi` + BIOS birlikte (yol haritası D.4-3).

### A7 — Sertleştirme denetimi ve belgeler
- Her aşamanın çıktısı `guvenlik-denetci` ajanından geçer.
- `olcum.sh` testleri yeni düzene göre güncellenir (X0 soketi, tarayıcı izin listesi, USB).
- Yol haritası, güncel durum ve hafıza notları güncellenir.

## Pi'de yapılabilecekler / yapılamayacaklar
- Pi'de: bütün yapılandırma dosyaları, betikler, labwc/waybar ayarları, USB yardımcısı ve Ayarlar programı
  (ARM'da derlenip denenebilir; waybar/labwc Pi'de de çalışır, görünüm burada önizlenebilir).
- Windows (WSL + Hyper-V) gerekir: ISO derleme, Calamares kurulumu, e-imza, gerçek donanım denemesi.

## Sıra ve teslim
A1 + A2 (bir arada, ilk görünür sonuç) → A4 → A5 → A3 → A6 → A7. Her aşama sonunda özet + kullanıcı denemesi.
Commit yalnız kullanıcı isteyince.

## Açık kalan kararlar
- Duvar kağıdı: (a) adaçayı degrade, (b) kendi görseliniz.
- Ekran kilidi süresi (öneri 10 dk).
- Tarayıcı izin listesinin son hali (gerçek denemede genişler).

## İlerleme

### 19.11 — Temel sistem 1. tur (A1 + A2 + A4'ün yapılandırması) YAZILDI, ISO DERLENMEDİ
- Kiosk dosyaları `armonia-linux/cokluuyap-katmani/` altına taşındı (bkz. BENIOKU.md). `iso_derle.sh` artık
  `derle_uyap.sh` çıktısı istemiyor, kasa katmanını kopyalamıyor, `toram` yok; imajda Çoklu UYAP bileşeni
  bulunursa derleme hata verir. `TEST=1` (kasa testleri) şimdilik kapalı.
- Giriş: greetd (`/etc/greetd/config.toml`), canlıda `armonia` kullanıcısına otomatik giriş; oturum
  `/opt/armonia/bin/oturum.sh` → `labwc -C /etc/armonia/labwc`.
- Masaüstü (`/etc/armonia/`): swaybg + eş yükselti duvar kağıdı (`armonia-linux/masaustu/uret_duvar.py`), üst
  waybar (saat, e-imza durumu, ağ, ses, pil, Güç menüsü), alt dock (A menüsü, UYAP Avukat, e-Devlet, Belgelerim,
  Yazı, Tablo, açık pencereler), fuzzel menü, mako bildirim, swayidle (canlıda yalnız ekran kapatma, kurulu
  sistemde 10 dk kilit). Kısayollar: Alt+Tab, Alt+F4, Windows tuşu (menü), Win+L, ses tuşları. Terminal/Çıkış yok.
- Tarayıcı: mevcut kum havuzu birimi korunarak normal pencere (sekme, adres çubuğu, yer imleri). Politika: `gov.tr`
  (tüm resmî siteler), TBB + İstanbul/Ankara/İzmir baroları, reCAPTCHA. nft: DNS yalnız 127.0.0.53, e-imza 5975
  açık, diğer yerel kapılar ve LAN kapalı, yalnız 80/443. Ev dizininden yalnız Belgeler (salt okunur) ve
  İndirilenler görünür.
- Uygulamalar: Thunar, Evince, LibreOffice Writer/Calc (Türkçe), Liberation yazı tipleri, Türkçe yerel ayar.
- **Pi'de doğrulanan:** waybar/fuzzel/swaybg/mako ekransız labwc'de çalıştırıldı, ekran görüntüsü alındı (bookworm
  sürümleriyle). fuzzel 1.8'de olmayan anahtarlar (anchor, colors.prompt) ayardan çıkarıldı.
- **Doğrulanmayan:** ISO derlemesi (amd64, Windows/WSL), greetd otomatik girişi, trixie paket adları/sürümleri
  (greetd, waybar 0.12, fuzzel), tarayıcıdan Avukat Portalı + e-imza (LocalNetworkAccess izni), izin listesinin
  yeterliliği (UYAP'ın dış kaynakları), yazıcı (CUPS yok; A5 ile).

### 19.11 — 2. tur: tasarım görseline uyum + güvenlik denetimi
- Kullanıcının ana ekran görseline göre: krem zemin + bütün ekranı kaplayan eş yükselti çizgileri, üstte "A Armonia"
  (tıklayınca menü) + "Deneme sistemi (canlı)" hapı + ortada saat — tarih + sağda TR-Q / E-İmza hapları ve ağ/ses/
  pil/güç simgeleri; altta ortada yüzen dock, simge + iki satır yazılı kutucuklar (UYAP & e-Duruşma, e-Devlet,
  Yazı, Tablo, Belgelerim) + açık pencereler. Simgeler `armonia-linux/masaustu/uret_simgeler.py` ile üretildi.
- Görseldeki Çoklu UYAP öğeleri (Sunucu modu hapı, UYAP Tüneli, sağ üst "Çoklu UYAP Canlı Durum" kartı, Çoklu UYAP
  ve UYAP Editör kutucukları) Çoklu UYAP katmanına; "Yazıcı ve Tarayıcı" A5'e kaldı.
- guvenlik-denetci (DÜZELTMEYLE KULLANILABİLİR) uygulananlar: oturum kullanıcısına nft `oturum_yerel` (LAN ve
  yerel kapılar kapalı; DNS + 5975 açık), Thunar özel eylemler eklentisi silindi, LibreOffice makroları kapalı ve
  kilitli (`armonia.xcd`), izin listesi `gov.tr` yerine açık adli site listesi (yalnız https),
  LocalNetworkAccess yalnız avukat.uyap.gov.tr, tarayıcı birimi nftables'a bağlı (PartOf), BindPaths `-` kaldırıldı,
  greetd `source_profile=false`.
- Açık kalan denetçi maddeleri: tarayıcı için ayrı kullanıcı / Wayland security-context (Y2), E-Duruşma WebRTC
  kapıları, IPv6 global LAN adresleri, indirme kısıtı (DownloadRestrictions), Calamares'te initial_session kaldırma.
- waybar 0.9.17'de (Pi) `justify` çalışmıyor; trixie 0.12'de denenecek.

### 19.11 — A5a: USB "takınca sor"
- `rules.conf`: ağ aygıtları (02/0a/e0) ve kurallara uymayan HID birleşikleri kalıcı ret; bellek, yazıcı, tarayıcı
  vb. için kural yok → `ImplicitPolicyTarget=block` (bekletilir). Son satırdaki `reject` kaldırıldı; kanca denetimi
  `# Armonia kurallari sonu` satırına bakıyor.
- `armonia-usb-sor.service` (root, yalnız CAP_SETUID/SETGID): `usbguard watch --exec usb_sor.sh`. Bekletilen yeni
  aygıtta `usb_soru.sh` armonia kullanıcısı olarak (setpriv) fuzzel ile sorar: "Reddet" (ilk satır) / "Bu kez izin
  ver" → `allow-device` (yalnız o takılış) ya da `reject-device`. 60 sn cevapsız / kilit ekranı / oturum yok →
  bekletilir. Aygıt adı yalnız güvenli karakterlerle gösterilir (sahte olayla denendi: `$(reboot)` → metin).
- USBGuard IPC hâlâ yalnız root; kullanıcı süreçleri izin veremez. Kalan risk: aynı kullanıcının süreci Wayland
  sanal klavye protokolüyle soruya cevap basabilir (labwc virtual-keyboard) → güvenlik denetiminde bakılacak.
- "Her zaman izin ver" yok (canlıda anlamsız; kalıcı liste A3 Ayarlar'a).

### 19.11 — A5b: Sürücü politikası ve yazıcı
- **Kullanıcı kararı:** "Kullanıcı işletim sistemini yüklerken tespit edilen donanımın sürücüsünü seçsin. Sonradan
  eklenen cihazlar için ise onay verirse sürücü indirilsin. Sürücü paketleri çok yer kaplıyor ve bilgisayarı
  hantallaştırıyor." → İmaja sürücü paketi (gutenprint, hplip, firmware-*) konmaz.
- İmajda: cups-daemon/client/filters, cups-ipp-utils (ipptool), ipp-usb (`dns-sd = disable`, `interface =
  loopback`; bağımlılık avahi-daemon maskeli), isenkram-cli (python3 getirir; Calamares da python3 ister).
- Akış: USB izni verilince `usb_sor.sh` → `armonia-surucu@<vid>-<pid>[-yazici].service` → `surucu.sh`:
  yazıcıysa önce sürücüsüz (localhost:60000–60015'te ipptool yoklaması → `lpadmin -m everywhere`); olmazsa
  `isenkram-lookup <modalias>` (yazıcıda üretici yedeği: HP→hplip, Epson→escpr, Brother→brlaser,
  Samsung→splix, diğer→gutenprint; DOĞRULANACAK) → izin listesi (printer-driver-*, hplip, foo2zjs, splix,
  firmware-*, sane) → boyutuyla sorulur ("İndirme" ilk satır) → `apt-get install --no-install-recommends`.
  Canlıda kurulan paket yeniden başlatınca gider.
- Ortak yardımcılar: `soru.sh` (fuzzel sorusu, kullanıcı olarak), `bildir.sh` (bildirim).
- A6'ya eklendi: kurulumda "Donanım sürücüleri" adımı (isenkram-lookup → işaretlenen paketler hedefe), depo
  bileşenlerine non-free-firmware.
- Denenecek (ISO/gerçek donanım): ipp-usb kapı aralığı, ipptool yoklaması, isenkram'ın yazıcı kapsamı, tarayıcının
  (chromium kum havuzu) /run/cups/cups.sock erişimi.
