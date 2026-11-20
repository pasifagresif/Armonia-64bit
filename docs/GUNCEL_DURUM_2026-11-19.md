# Güncel Durum — 19.11.2026

> Bu dosya, proje Raspberry Pi'ye taşınırken tüm notlardan derlendi. Ayrıntı için aşağıdaki kaynaklara bakın.
> Kullanıcıyla **Türkçe** konuşulur.

## Kaynaklar (önce bunları okuyun)

| Konu | Dosya |
|---|---|
| Windows Rust uygulaması (ruyap), icra veritabanı, İş Listesi — en güncel | `docs/18.11.2026 durum ve kalan işler.md` |
| Aynı konunun ayrıntılı günlüğü | `docs/16.11.2026 yapılacaklar.md` |
| Armonia Linux planı — "▶ DEVAM NOKTASI" bölümü | `docs/ARMONIA_LINUX_YOL_HARITASI.md` |
| Armonia ilk gerçek donanım denemesi | `docs/ARMONIA_GERCEK_DONANIM_SORUNLARI.md` |
| Armonia gece günlüğü (14.11) | `docs/ARMONIA_GECE_DURUM.md` |
| Gemini taslakları (AppArmor, USBGuard, Faz 3, tarayıcı) | `docs/armonia_taslaklar/` |
| Temel tasarım ilkesi + Rust iyileştirme listesi | `Uyap Haricen Giriş/Rust Program İyileştirme..md` |
| Claude'un kalıcı notları (hafıza) | `docs/claude-notlar/hafiza/` (dizin: `MEMORY.md`) |
| Onaylı plan dosyaları | `docs/claude-notlar/planlar/` |
| Proje ajanları/skill'leri (guvenlik-denetci, gemini, iso-derle) | `docs/claude-notlar/claude-yerel/` |

## Projenin parçaları

- **`Uyap Haricen Giriş/ruyap/`** — ana ürün. Rust ile yazılmış Çoklu UYAP (egui masaüstü + UYAP vekil sunucusu,
  port 8800). Arayüz eklentileri `ruyap/uyap_core/enjekte/` altında. **Asıl kopya burası.** Üst dizindeki
  `Uyap Haricen Giriş/uyap_core/enjekte` Python dönemine ait; artık düzenlenmiyor.
- **`Uyap Haricen Giriş/` (Python)** — eski sürüm, rafta. Canlı web sunucusu (`vendor_deploy`, Render) hâlâ Python.
- **`UyapMasa/`, `models/icra_models/`** — yalnız örnek model. Özellikler ikinci program olmadan ruyap'ta çalışmalı.
- **`armonia-linux/`** — sertleştirilmiş Debian canlı imajı. 19.11'den beri **temel hukukçu masaüstü** (Çoklu UYAP
  yok; `cokluuyap-katmani/` ve `kasa/` rafta). Live-build yapılandırması, sertleştirme, masaüstü, Plymouth teması.

## Armonia Linux — özet

- **19.11 yön değişikliği:** Armonia kiosk değil, her hukukçunun kurabileceği temel masaüstü olacak; Çoklu UYAP en
  son, Ayarlar'dan açılan katman. Plan + ilerleme: `docs/claude-notlar/planlar/armonia-herkes-icin.md`.
  Temel sistemin 1. turu (greetd, labwc masaüstü, dock, tarayıcı izin listesi, ofis) yazıldı; ISO derlenmedi.
  Aşağıdaki maddelerin uyap_app'e ait olanları rafta.

- **Bitti:** Faz 1 (Hyper-V'de 8–15 sn açılış), Faz 3 ilk katman (nftables, lockdown, sandbox), Faz 2 kasa
  (LUKS + kasada PostgreSQL + AppArmor complain; Hyper-V'de doğrulandı), Chromium ayrı birimde, USBGuard A, PipeWire,
  e-imza katmanı (`adalet-eimza-tray` .deb, 5975, Hyper-V'de getCertificates → 200).
- **İlk gerçek donanım denemesi (14.11):** UYAP girişi kurulmadı. Kök nedenler bulundu ve Rust'ta düzeltildi
  (sertifika seçimi stdin'i bekliyordu; geçici hatada giriş kapısı kilitleniyordu). Bu düzeltmelerle
  **uyap_app ve ISO yeniden derlenmedi, denenmedi.**
- **Sıradaki iş (yol haritası §D.4):**
  1. Linux'ta e-imza girişi: pcscd + AKİS PKCS#11, gerçek kartla deneme.
  2. Wi-Fi.
  3. UEFI açılış.
  4. Kasa ilk-kurulum akışı.
  5. Gerçek GPU.
  6. USBGuard saha ayarı.
  7. E-Duruşma için tarayıcıya ağ izni.
- **Açık kullanıcı kararları:** PostgreSQL zorunluluğu, kasa kurtarma anahtarı, otomatik kilit süresi,
  AppArmor enforce, rust-review (3 orta bulgu) ve cargo-audit (10 uyarı) bulguları.

## Windows Rust uygulaması (ruyap) — özet (18.11 akşamı)

- İş Listesi büro veritabanına taşındı (L1–L7: şema, kimlik, bildirim, otomatik işler). Bulut kopyası silindi.
- İcra veritabanı: liste ekranı, çoklu seçim, UYAP'tan güncelleme (`src/icra_guncelle.rs`), canlı liste.
- Testler: birim 65/65; uçtan uca ve Playwright testleri geçti.
- **Sıradaki işler:**
  1. Tebligat bilgileri (`barkod_sorgu.py` → Rust).
  2. Mernis + 21/2. Gönderim ücretli; her gönderim ayrı onay ister.
  3. Borçlu TCKN doldurma gece işi (onay bekliyor).
  4. Varsayılan birim ayarı web görünümüne taşınacak.

## İş listesi — Armonia ve Hukuk Hesaplayıcı (19.11)

Planlar: `docs/claude-notlar/planlar/armonia-herkes-icin.md`, `docs/claude-notlar/planlar/hukuk-hesaplayici.md`.

**Armonia temel sistem**
- [x] 1. tur: greetd + labwc masaüstü, üst çubuk + dock (tasarım görseline göre), tarayıcı izin listesi, ofis.
- [x] Güvenlik denetimi bulguları (oturum ağ sınırı, makro kilidi, dar izin listesi).
- [ ] ISO'yu Windows'ta (WSL) derleyip Hyper-V'de denemek.
- [x] A5a: USB "takınca sor" (USBGuard block + `armonia-usb-sor.service` → fuzzel sorusu; ağ aygıtı ve HID
  birleşikleri kalıcı ret; udisks2 + gvfs). Pi'de sahte olaylarla denendi; gerçek aygıtla ISO'da denenecek.
- [x] A5b: Yazıcı + sonradan takılan aygıta sürücü. **Kullanıcı kararı 19.11: sürücü paketleri imaja konmaz;**
  kurulumda tespit edilen donanımın sürücüsü seçilir, sonradan takılan için onay verilirse indirilir. İmajda yalnız
  sürücüsüz yazdırma (cups + ipp-usb, localhost, avahi maskeli) + isenkram. `armonia-surucu@.service` +
  `surucu.sh`. Gerçek yazıcıyla ISO'da denenecek. "Yazıcı ve Tarayıcı" kutucuğu / yazıcı ekleme → A3 Ayarlar.
- [x] A3 (ilk iki parça): Armonia Ayarlar (`armonia-uygulamalar/ayarlar/`, egui): Ağ (NetworkManager; Wi-Fi tara/
  bağlan/unut, parola stdin ile root yardımcısına), Klavye (TR-Q/F, labwc hemen), Ekran kilidi (5/10/15/30 dk,
  canlıda kapalı), Yazıcılar (liste, varsayılan, sil, deneme sayfası), USB (bilgi), Çoklu UYAP (yer tutucu),
  Hakkında. Yetki: `pkexec /opt/armonia/bin/ayar-yardimci` (polkit tr.armonia.ayar, yalnız etkin oturum).
  Ağ yönetimi systemd-networkd → **NetworkManager** (19.11).
- [ ] A3.3: Kalıcı USB izin listesi → kurulu sistemle (A6) birlikte.
- [ ] Açık: giriş sayfalı (captive portal) Wi-Fi'ler — tarayıcı izin listesi giriş sayfasını engeller; çözüm gerekli.
- [x] A7 denetimi (19.11, yeni root parçaları): KRİTİK yok. Düzeltildi: cevap borusu → root 0600 dosya (O3),
  soru yağmuru sınırı + oturumluk ret hafızası + TasksMax (O2), fuzzel tam eşleşme, USB ağ aygıtları NM'de
  yönetilmez (O1), yazıcı adı `-` ile başlayamaz (D1), Wi-Fi parolasında boşluk kaçışı (D2), load hatasında dosya
  silinir (D3), apt kilidi (D7), boş apt listesinde güncelleme (D6), seri no ile yazıcı URI'si (D5).
- [ ] AÇIK (denetim Y1, karar gerekli): aynı kullanıcı kimliğindeki kötü amaçlı süreç Wayland sanal klavyesiyle
  USB/sürücü sorusuna "izin ver" dedirtebilir → "takınca sor" ele geçirilmiş oturuma karşı sınır değil. Seçenekler:
  labwc'de sanal klavye/işaretçi protokolünü kapatmak, onayı fiziksel eyleme bağlamak (çıkar-tak), soruyu
  oturum dışı (greetd/VT) göstermek.
- [ ] Açık (denetim D8): açılışta takılı olan aygıtlar sorulmuyor (yalnız Insert olayı) → bekletilmiş kalıyor.
- [ ] A6'da (denetim D10): kurulu sisteme `/etc/armonia/deneme` geçmemeli; derlemede denetle.
- [ ] **Raspberry Pi deneme imajı** (`armonia-linux/pi_derle.sh`, taban RPi OS Lite trixie 2026-10-06; e-imza yok,
  `/etc/armonia/deneme` → canlı gibi). İlk derleme 19.11'de başlatıldı.
- [ ] A6: Calamares ile diske kurulum (LUKS2, otomatik güvenlik güncellemesi, UEFI).
  + kurulumda "Donanım sürücüleri" adımı: isenkram-lookup ile takılı donanım → kullanıcı işaretler → hedefe kurulur
  (depo bileşenine non-free-firmware eklenmeli; şu an imaj yalnız main).
- [ ] Açık denetçi maddeleri: tarayıcıya ayrı kullanıcı, E-Duruşma (WebRTC) kapıları, IPv6, indirme kısıtı.
- [ ] En son: Çoklu UYAP katmanı (Ayarlar'dan açılır; sağ üst durum kartı, UYAP Tüneli hapı).

**Hukuk Hesaplayıcı** (onaylandı 19.11; Armonia + Windows, egui)
> **Öncelik (kullanıcı, 19.11):** "Asıl önemli kısımdan sapıyoruz. İşletim sistemi yazıyoruz." Hesaplayıcının
> kalan işleri (H4, H5, doğrulamalar, H2/H3 ek adımları) **bekletilir**; önce Armonia temel sistemi.
- [x] H1: Süre hesabı + uygulama iskeleti (`armonia-uygulamalar/hukuk_hesap/`, 13 birim testi). Hazır süre
  şablonları ve kurallar kullanıcı doğrulaması bekliyor.
- [x] Hesap makinesi sekmesi (dört işlem, yüzde, parantez, Türkçe virgül/binlik; açılışta bu sekme gelir).
- [x] H2: Faiz (yasal, ticari/avans, sözleşmesel; dönemlere bölme; çok kalem). Oranlar `veri/faiz.json`
  (TCMB tablosu + 3095 metni, 19.11'de okundu; kullanıcı doğrulaması bekliyor). Kısmi ödeme mahsubu (TBK 100) ve
  döviz alacağı (3095 m.4/a) sonraki adım. Açık soru: avans oranı kanun metni mi, TCMB değişiklik tarihleri mi.
- [x] H3: Vekalet ücreti (AAÜT 2026–2027, RG 03.11.2026/33419; dava kabul/red, icra, arabuluculuk, maktu
  tablosu). Tutarlar taranmış RG ekinden okundu → kullanıcı doğrulaması bekliyor. Eski tarifeler (karar tarihine
  göre) ve istinaf/temyiz, manevi tazminat, seri dava ayrıntıları sonraki adım.
- [ ] H4: Harç ve masraf (492 s. Harçlar Kanunu 2026 tarifeleri). — BEKLİYOR
- [ ] H5: Armonia'ya (dock kutucuğu) ve Windows'a (exe) koyma. — BEKLİYOR
- [ ] Doğrulamalar: hazır süreler (H1), faiz oranları ve avans kuralı (H2), AAÜT tutarları ve kabuller (H3).
- [ ] H2 ek: kısmi ödeme mahsubu (TBK 100), döviz alacağı. H3 ek: eski tarifeler, istinaf/temyiz, manevi tazminat.
- [ ] **Sonra: veri çekme modülü** (faiz oranları, AAÜT, harç tarifeleri, dini bayram tarihleri). Kullanıcı kararı
  19.11: ayrı yazılacak. O zamana dek veri programa gömülü + kullanıcı elle ekler.
- [ ] Açık soru: süre son günü arifeye (yarım gün) denk gelirse uzar mı? (şimdilik ayar)

## Raspberry Pi'de çalışırken bilinmesi gerekenler

- **İşlemci farkı:** Pi ARM64, Armonia imajı ve e-imza paketi **amd64**. Pi'de kod ve yapılandırma düzenlenir.
  Şunlar Pi'de yapılamaz ya da pratik değildir:
  - ISO derleme (live-build) ve Hyper-V testi,
  - amd64 `uyap_app` derleme,
  - gerçek UYAP/e-imza girişi.
  Bunlar Windows makinede (WSL Debian 13 + Hyper-V) yapılır.
- `ruyap` Pi'de **ARM için** derlenip birim testleri çalıştırılabilir:
  `cargo test --lib` (Linux'ta Türkçe yol sorunu yok).
  Windows'a özgü parçalar (tepsi, `paylas_win.rs`) Linux'ta derlenmez; `cfg` korumaları vardır.
- **Derleme betikleri (19.11):** `derle_uyap.sh` ve `iso_derle.sh` yollarını kendi yerlerinden bulur. Yollar
  `REPO`, `ARMONIA_CIKTI`, `KAYNAK`, `UYGULAMA`, `ISO_DIZIN` ortam değişkenleriyle değiştirilebilir. WSL'deki
  varsayılanlar aynı kaldı. `iso_derle.sh` x86_64 olmayan makinede durur; `ZORLA=1` ile geçilir.
  Ayrıca imaja artık asıl arayüz kopyası (`ruyap/uyap_core/enjekte`, `ruyap/webapp`) giriyor. Eskiden Python
  kopyası giriyordu ve `icra.js`, `isler.js`, `paylas.js` eksikti.
- **Depoda olmayan dosya:** `armonia-linux/config/packages.chroot/adalet-eimza-tray_1.0.17_amd64.deb` (70 MB,
  `.gitignore`'da). ISO derlenecekse resmî kaynaktan yeniden indirilir:
  `https://cdn.uyap.gov.tr/framework/public/adalet-eimza-tray_1.0.17_amd64.deb`
  (md5 `a1a4b3c3099df599c2d5193f2259c1aa`).
- **Notlardaki Windows yolları** (`C:\dev\...`, `%LOCALAPPDATA%`, `C:\Users\WİN10\...`) Windows makineye aittir;
  Pi'de karşılıkları yoktur.
- **`agy` (Gemini CLI) Pi'de kurulu değil.** `gemini` ajanı ve `gemini-ajan` skill'i orada çalışmaz.
  `guvenlik-denetci` ajanı çalışır.

## Değişmez kurallar

- Buluta müvekkil verisi çıkmaz. UYAP'tan robotla veri çekilmez. PIN saklanmaz, Gemini'ye gönderilmez.
- Commit, push, exe kopyalama ve yayın: her biri ayrı kullanıcı onayı ister.
- UYAP'ın hiçbir tasarım öğesi kullanılmaz (bkz. hafıza: `uyap-tasarimindan-kopus.md`).
  İstisna: "UYAP" ve "Koyu" temaları seçenek olarak kalır.
- Büyük işlerde önce plan yapılır, kod yazmadan önce onay alınır.
