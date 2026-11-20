# Armonia / HukukOS — Linux Dağıtımı Uygulanabilir Yol Haritası

> Hazırlayan: Claude · İlk sürüm: 2026-11-13 · Sürüm 2 (araç araştırması + GitHub referansları) ·
> **Sürüm 3 (2026-11-14): "DEVAM NOKTASI" bölümü eklendi — yeni oturum BURADAN başlar**

---

## ▶ DEVAM NOKTASI (2026-11-14 öğlen) — yeni oturumda Claude'a "yol haritasından devam et" deyin

### D.YÖN DEĞİŞİKLİĞİ (2026-11-19) — ÖNCE BUNU OKUYUN
Kullanıcı kararı: Armonia artık **Çoklu UYAP üyesi olsun olmasın her hukukçunun kurup kullanabileceği** bir sistem.
Açılışta Rust kodu (uyap_app) çalışmaz; önce işletim sisteminin ana çatısı (masaüstü, tarayıcı, e-imza, ofis,
kurulum), Çoklu UYAP en son Ayarlar'dan açılan katman olarak eklenir. Kiosk düzeni ve aşağıdaki D.0–D.4'ün
uyap_app'e ait maddeleri bu yüzden RAFTA. Güncel plan ve ilerleme: `docs/claude-notlar/planlar/armonia-herkes-icin.md`.
Kiosk dosyaları: `armonia-linux/cokluuyap-katmani/` (BENIOKU.md).

**Claude için ilk iş:** bu bölümü oku → kullanıcıya gerçek donanım denemesinin sonucunu sor (§D.3 soru listesi)
→ §D.0'daki yarım işi bitir → §D.4 iş listesinde sıradaki maddeye geç. Ayrıntılı gece günlüğü: `docs/ARMONIA_GECE_DURUM.md`.

### D.0b. İLK GERÇEK DONANIM DENEMESİ (2026-11-14 öğleden sonra, `armonia_seri.iso`, Rufus DD, i5-3470)
Kullanıcı bildirdi: Num Lock kapalı; UYAP oturumu kurulmadı, arayüz uzun süre bekledi; Günlük'te yalnız
"Paylaşım Aktif"; tarayıcıda avukat.uyap.gov.tr / cokluuyap.com / eimza.adalet.gov.tr "yasak"; düğmeler geç tepki.
Bulunan kök nedenler ve düzeltmeler (Claude):
- **Sertifika seçimi stdin'den okuyordu** (`uyap_giris_disaridan.rs` `select_certificate`): kartta >1 sertifika ve
  kayıtlı cert_id yoksa `read_line` tty1'de SONSUZA DEK bekliyor, relogin kilidi tutuluyor → her UYAP isteği askıda.
  Windows GUI'de stdin yok → anında ilk sertifika. Düzeltme: yalnız `main_cli` etkileşimli; diğer yerlerde süresi
  geçmemiş ilk sertifika. Ayrıca `oturum.sh` uygulamayı `</dev/null` ile başlatır.
- **Giriş kapısı geçici hatada da kilitleniyordu** (`uyap_proxy.rs` `relogin`): e-imza istemcisi açılışta hazır
  değilken (Java ~20-30 sn) ilk deneme düşerse paylaşım yeniden başlatılana dek hiç denenmiyordu. Düzeltme: PIN karta
  gitmeden önceki hatalar (`GECICI_HATA` öneki: UYAP'a ulaşılamadı / 5975 yok / sertifika yok) kapıyı kilitlemez;
  imza adımı ve sonrası (yanlış PIN riski) kilitler.
- **Durum mesajları görünmüyordu:** vekil/giriş `println!` → yalnız journal. Düzeltme: Linux'ta stdout+stderr boruya
  alınıp "[" ile başlayan satırlar Kontrol Ekranı Günlüğü'ne de akar (`stdout_arayuze_akit`); günlük 64 KB ile sınırlı.
- **Giriş kartındaki durum yazısı** düğmenin altında, pencere dışında kalıyordu → düğmenin üstüne alındı.
- **Yavaş arayüz:** `WLR_RENDERER=pixman` gerçek donanımda da zorlanıyordu → kaldırıldı (wlroots render düğümü
  varsa GPU, yoksa pixman seçer).
- **Num Lock:** Hyper-V'de `-C` ile çalışıyor (tuş takımı "1" yazdı, 14:40). Gerçek USB klavyede yalnız LED sönük
  kalıyor olabilir — kullanıcıdan tuş takımıyla rakam yazılıp yazılmadığı soruldu.
- **"Yasak" siteler tasarım gereği:** tarayıcı politikası yalnız `127.0.0.1:8800` (UYAP vekil üzerinden açılır).
  UYAP oturumu kurulamayınca sayfalar gerçek alan adlarına yönleniyor ve engelleniyordu; kök neden yukarıdaki giriş.
- Hyper-V USB kart okuyucuyu konuğa geçiremiyor: e-imza girişi yalnız gerçek donanımda doğrulanabilir.

### D.0a. ÖLÇÜM GEÇTİ (2026-11-14 13:22, `C:\dev\armonia_tarayici_olcum.iso`, sha256 `8f09cc1f…40e9955594`)
Hyper-V 2 GB: açılış 15,4 sn, `running`, tek uyap_app, DNS çalışıyor. Bu turda düzeltilip doğrulananlar:
- **E-imza çalışıyor:** servis active, 5975 yalnız `[::ffff:127.0.0.1]`, getCertificates → 200, Xvfb soyut soket/TCP/SysV shm 0.
  İki kök neden: (1) Xvfb root değilken `/tmp/.X11-unix`'i oluşturmuyor → `ExecStartPre=mkdir -m 1777` (PrivateTmp içinde);
  (2) `~@privileged` X sunucusunun `System()` çocuğundaki `setuid(getuid())`'yi engelleyip xkbcomp'u düşürüyordu
  ("Failed to activate virtual core keyboard") → `SystemCallFilter=@setuid` geri açıldı (WSL'de systemd-run ile grup grup doğrulandı).
  `xvfb-run -e /dev/stderr` journal'a yazıyor (denetçinin K-1 ENXIO uyarısı Hyper-V'de çürütüldü).
- **Tarayıcı:** birim açılıyor; `xdg-open` artık asılı kalmıyor (`nc -w 2`; Accept=yes'te PID 1 bağlantıyı birim bitene dek tutar),
  `tarayici.sh` fd 3/LISTEN_* kapatıyor. **nft kuralı doğrulandı:** tarayıcı slice'ından 5975 ve 53 → reddedildi, system.slice'tan
  5975 → açık. (Eski olcum satırı `nsenter` ile cgroup değiştirmediği için yanıltıcıydı; artık `systemd-run -p Slice=` ile.)
- **O-X1 kapandı:** labwc'ye `WLR_XWAYLAND=/bin/false` (armonia-kiosk.service); X0 soketi dinliyor ama X11 kurulum isteğine
  yanıt yok, Xwayland süreci oluşmuyor, labwc çalışıyor.
- `iso_derle.sh` kopyası `dd oflag=direct` (WSL 2,9 GB'da cp → "Cannot allocate memory").
- Kullanıcı VM'i `Armonia`: açılış belleği 3 GB→2 GB (3 GB'ta "yeterli bellek yok / Video Monitor" hatası).
**Açık:** Num Lock ve tek pencere gözle doğrulanmadı; O-X2 (gerçek kartla e-imza); denetçi D-1 (`setuid setgid`'e daraltma,
isteğe bağlı), D-3 (`pin` süzgeci "mapping"i de yutar); üretim ISO'su (`C:\dev\armonia.iso`) henüz derlenmedi.

### D.0. YARIM KALAN İŞ (2026-11-14 ~10:00, bilgisayar deneme için kapatıldı)
- **Kullanıcının denediği ISO:** `C:\dev\armonia.iso` (742 MB, 09:47, sha256 `e0ffab03…5c6d5f8d`). İçinde: Chromium (ayrı birim),
  USBGuard A, PipeWire, Türkçe tarayıcı. **E-imza YOK.** Hyper-V: 15,4 sn açılış, running, giriş ekranı geldi.
  Hyper-V'de USBGuard USB veri yolu olmadığı için başlamadı (`loadFiles: /sys/bus/usb/devices`) → drop-in'e
  `ConditionPathIsDirectory=/sys/bus/usb/devices` eklendi (sonraki ISO'da).
- **E-imza katmanı YAZILDI, denetimi YARIM:** UYAP resmî Linux istemcisi `adalet-eimza-tray_1.0.17_amd64.deb`
  (kaynak `https://cdn.uyap.gov.tr/framework/public/adalet-eimza-tray_1.0.17_amd64.deb`, md5 `a1a4b3c3099df599c2d5193f2259c1aa`,
  sha256 `97f58f39…31e8fb68`; asıl `C:\dev\eimza_paket\`). İnceleme: Spring Boot + gömülü Java 21, `127.0.0.1:5975`
  (ÇokluUYAP'ın beklediği adres), TÜBİTAK MA3/AKİS + PC/SC, X11 tepsi, sistem bilgisi toplayan sınıflar, `bedesten.adalet.gov.tr`.
  Dosyalar: `config/packages.chroot/adalet-eimza-tray_1.0.17_amd64.deb`, `config/includes.chroot/etc/systemd/system/armonia-eimza.service`
  (User=eimza, headless JAVA_TOOL_OPTIONS, sandbox), `config/includes.chroot/etc/polkit-1/rules.d/50-armonia-eimza.rules`
  (pcscd yalnız 'eimza'), hook'ta e-imza bloğu, paket listesine `pcscd libccid`, kiosk `Wants=armonia-eimza.service`,
  tarayıcıda `-/run/pcscd` kapalı, `olcum.sh` e-imza bölümü (5975 + getCertificates).
  **Denetim TAMAM (10:05), bulgular uygulandı:** E-K1 polkit açık ret (yalnız 'eimza') + kiosk `InaccessiblePaths=-/run/pcscd`;
  E-K2 `LogFilterPatterns=~(?i)pin`, olcum.sh yalnız HTTP durum satırı (sertifika/TCKN seri porta yazılmaz);
  E-Y1 tarayıcı `Slice=armonia-tarayici.slice` + `ExecStartPre=+nft -f /opt/armonia/nft/tarayici_yerel.nft`
  (nftables `chain tarayici_yerel`, 5975/8843 reject); E-Y2 `libpcsclite1` + `-Dsun.security.smartcardio.library=...`;
  E-O4 hook denetimi. **Açık kalanlar:** E-O1 AKİS dışı kartlar için PKCS#11 sürücüsü (gerçek kartla bak),
  E-O2 sistem bilgisi uçları (`/api/v1/client`, `/info`) — proxy/enjekte'nin çağırmadığını doğrula, E-O3 eimza giden trafik
  izleme→beyaz liste, olcum servisinin üretimde kapatılması (denetçi önerisi).
  **Sıradaki:** `OLCUM_TARAYICI=1 iso_derle.sh` → Hyper-V (5975 dinliyor mu, headless açılıyor mu, nft kuralı, tarayıcı→5975 ret)
  → üretim `iso_derle.sh` → kullanıcıya gerçek donanımda kartla giriş (bilerek yanlış PIN ile günlük denetimi DEĞİL — kart kilitlenebilir;
  günlükte `pin=` araması yeterli).
- **2026-11-14 öğleden sonra (Hyper-V kullanıcı denemesi):** ÇokluUYAP girişi "Temporary failure in name resolution"
  → imajda `/etc/resolv.conf` 0 bayt (live-build chroot_resolv). Düzeltme: `etc/tmpfiles.d/armonia-resolv.conf`
  (`L+` → resolved stub) + `iso_derle.sh` varlık denetimi. Ayrıca: labwc `<numlock>on</numlock>`; çift
  "Kontrol Ekranı" penceresi (iki uyap_app) → `oturum.sh` flock tek örnek + `olcum.sh` süreç ağacı (kaynak DOĞRULANMADI).
  **guvenlik-denetci: DÜZELTMEYLE KULLANILABİLİR**, uygulananlar: O1 tarayıcı birimi DNS kapalı (nft 53 reject +
  `InaccessiblePaths=-/run/systemd/resolve`), D1 `resolved.conf.d/armonia.conf` (LLMNR/mDNS kapalı, FallbackDNS boş),
  D3 olcum.sh `ps -C` (args değil), D4 flock. **Açık:** D2 iso_derle.sh içerik denetimi (derleme bitince),
  O2 `armonia-olcum.service` üretimde koşulsuz açık → `ConditionKernelCommandLine=armonia.olcum=1` (test betikleri
  bu parametreyi eklemeli; karar bekliyor), DoT/DNSSEC kararı, Rust tarafında tek örnek kilidi.
  O2 kullanıcı onayıyla uygulandı (`ConditionKernelCommandLine=armonia.olcum=1`; D2 içerik denetimi de uygulandı).
- **Hyper-V ölçümü 11:40 (`armonia_tarayici_olcum.iso`, sha256 `c99ede5c…7f800761`):** açılış 16,1 sn, running, tek
  uyap_app (ppid oturum.sh, tek oturum.sh) → çift pencere bu açılışta YOK; DNS çalışıyor (deb.debian.org çözüldü);
  herkese-yazılabilir 0. **İki hata:** (1) tarayıcı birimi başlamadı: nft `socket cgroupv2 level 1
  "armonia-tarayici.slice"` → "cgroupv2 path fails" (systemd'de `-` hiyerarşi: yol `armonia.slice/armonia-tarayici.slice`,
  seviye 2) → düzeltildi, sonraki ISO'da doğrulanacak. (2) **e-imza çöküyor, 5975 dinlemiyor:** derleme chroot'unda
  kum havuzu olmadan da aynı: `AyarlarUI` (Swing JFrame) Spring bean olarak açılışta kuruluyor →
  `java.awt.HeadlessException`. Program ekransız ÇALIŞAMIYOR. Seçenekler: (a) e-imza birimine özel Xvfb (görünmez X,
  PrivateTmp içinde soket), (b) Rust'ta pcscd + `libakisp11.so` ile doğrudan PKCS#11 (yol haritası madde 1). Karar bekliyor.

### D.1. Şu an ne var (doğrulanmış)
| Bileşen | Durum | Nerede |
|---|---|---|
| Önyüklenebilir canlı imaj (Debian 13, toram, labwc Wayland kiosk, ÇokluUYAP Rust) | ✅ Hyper-V'de 12,9 sn açılış | `armonia-linux/config/` |
| Faz 2 kasa (LUKS2, Plymouth TR parola ekranı, parola kuralı ≥10/4 sınıf/ASCII) | ✅ Hyper-V uçtan uca | `armonia-linux/kasa/`, `plymouth/` |
| PostgreSQL (kasada, yalnız Unix soketi) | ✅ Hyper-V | `kasa/etc/systemd/system/armonia-postgresql.service` |
| Faz 3 sertleştirme (nftables, sysctl, lockdown, modül kara listesi, kiosk sandbox) | ✅ Hyper-V | `armonia-linux/sertlestirme/` |
| AppArmor `uyap_app` profili | ⚠️ complain kipinde (engellemiyor) | `sertlestirme/etc/apparmor.d/` |
| Tarayıcı: Debian Chromium, AYRI birim (`armonia-tarayici.socket` + `@.service`), yönetilen politika | 🔄 14 Kasım yazıldı, denetçiden geçti, Hyper-V ölçümü sürüyor | `config/includes.chroot/etc/systemd/system/armonia-tarayici*`, `opt/armonia/bin/tarayici.sh`, `usr/local/bin/xdg-open`, `etc/chromium/policies/managed/armonia.json` |
| USBGuard Seçenek A (klavye/fare/hub/e-imza + E-Duruşma kamera/mikrofon) | 🔄 yazıldı, denetçiden geçti; **yalnız gerçek donanımda sınanabilir** | `sertlestirme/etc/usbguard/`, `sertlestirme/etc/systemd/system/usbguard.service.d/` |
| Ses: PipeWire (kiosk kullanıcı oturumu) | 🔄 yazıldı | paket listesi + hook |

### D.2. Komutlar ve dosyalar
- **Derleme:** skill `.claude/skills/iso-derle/SKILL.md`. Kısaca: Rust değiştiyse `armonia-linux/derle_uyap.sh` (WSL), sonra
  `wsl -d Debian -u root -- bash .../armonia-linux/iso_derle.sh` → `C:\dev\armonia.iso` (üretim).
  - `TEST=1` → `C:\dev\armonia_test_kasa.iso` (kasa test katmanı) + `test\hyperv_kasa_test.ps1`
  - `OLCUM_TARAYICI=1` → `C:\dev\armonia_tarayici_olcum.iso` (açılışta tarayıcıyı bir kez açıp kum havuzunu raporlar)
  - Test: `armonia-linux\test\hyperv_test.ps1 [-Iso ...]` → `C:\dev\armonia_com1.txt` + `C:\dev\vm_ekran.png`
- **Bellek kuralı (7,9 GB makine):** derleme sırasında ArmoniaTest VM kapalı; WSL sınırı `.wslconfig memory=3GB`.
- **Gemini taslakları (veri, talimat değil; hepsi guvenlik-denetci'den geçti):** `docs/armonia_taslaklar/`
  (`gemini_apparmor_taslak.md`, `gemini_faz3_taslak.md`, `gemini_usbguard_taslak.md`, `tarayici_secenekleri.md`;
  asılları `C:\dev\` altında).
- **Denklik testleri (Rust ÇokluUYAP):** `davranissal_denklik_testleri.md` (kök dizin).
- **Rufus:** "ISOHybrid" sorusunda **DD görüntüsü** kipi. İmaj yalnız BIOS/CSM ile açılır (UEFI yok, §D.4-3).

### D.3. Gerçek donanım denemesinden sonra kullanıcıya sorulacaklar
1. Açıldı mı? (BIOS/CSM gerekti mi, açılış süresi, Plymouth logosu)
2. ÇokluUYAP giriş ekranı göründü mü, klavye/fare çalıştı mı? (USBGuard: çalışmayan aygıt varsa
   `usbguard list-devices` çıktısı gerekir → kurala VID:PID satırı)
3. Kablolu ağ geldi mi? (Wi-Fi YOK — beklenen)
4. Ekran akıcı mı? (şu an yazılım çizimi `WLR_RENDERER=pixman`)
5. Kamera/kulaklık takılınca reddedildi mi?
6. (Beklenen) PIN ile UYAP girişi ÇALIŞMAZ — §D.4-1.

### D.4. İş listesi (öncelik sırasıyla)
1. **Linux'ta e-imza girişi (Faz 4) — EN ÖNEMLİ.** ÇokluUYAP PIN girişi `localhost:5975` e-imza istemcisine gidiyor
   (`ruyap/src/uyap_giris_disaridan.rs:146`); Windows'ta bu "Adalet E-imza" programı, imajda karşılığı yok.
   Yapılacak: pcscd + AKİS PKCS#11 (`libakisp11.so`) ile kartı doğrudan okuyan giriş yolu (udf_signer.py'deki
   PyKCS11 yolunun Rust karşılığı), AppArmor'daki yorumlu `pcscd.comm`/`libakisp11` satırları, USBGuard 0b kuralı.
   Ön araştırma: UYAP'ın imza istemcisinin giriş için imzaladığı veri biçimi (`sign_payload`).
2. **Wi-Fi:** `firmware-iwlwifi/realtek/atheros` (non-free-firmware) + `iwd` veya NetworkManager'sız basit arayüz; nftables etkisi.
3. **UEFI açılış:** live-build `--bootloaders grub-efi` (+ Secure Boot shim değerlendirmesi).
4. **Kurulum/kasa bölümü oluşturma:** gerçek diskte `PARTLABEL=armonia-kasa` LUKS2 bölümünü güvenli kuran ilk-kurulum
   akışı (şu an yalnız `TEST=1` katmanında var); kurtarma anahtarı kararı.
5. **Gerçek GPU (Faz 5):** `WLR_RENDERER=pixman` kaldır / donanım algılamasıyla seç; kiosk `DevicePolicy` (denetçi O3).
6. **USBGuard saha ayarı:** gerçek klavye/okuyucu/kamera `usbguard generate-policy` çıktısı; ff sınıflı eski tokenlar.
7. **E-Duruşma:** sayfanın origin'i (127.0.0.1:8800 vekili mi?) ve medya/TURN sunucuları → tarayıcı biriminde
   `IPAddressAllow=` (şu an `IPAddressDeny=any`, yalnız localhost).
8. **Kullanıcı kararları (açık):** PG zorunluluğu / "Veritabanı hazırlanıyor" ekranı; kasa kurtarma anahtarı;
   TR-Q/TR-F; otomatik kilit süresi; giden trafik beyaz listesi; AppArmor enforce (önce complain kayıtlarıyla daraltma).
9. **Kod:** rust-review 3 orta bulgu + cargo-audit 10 uyarı (kod/Cargo.toml henüz DEĞİŞTİRİLMEDİ, karar bekliyor).
10. **ÇokluUYAP:** giriş başarısızken Kontrol Ekranı "Paylaşım Aktif" yerine hata göstersin; 5975 ulaşılamazsa açık
    Türkçe mesaj. Release derleme. Kullanıcının denklik kontrolleri (dosya kartları, telefon, toplu indirme).
11. **Tarayıcı birimi için AppArmor profili** (denetçi D6) ve kiosk biriminde `/run/user/<uid>/bus` kapatma (EK-Y1 notu).
12. Faz 6 (dm-verity/bütünlük, yedek, güncelleme) ve Faz 7 (test matrisi, dağıtım).

### D.5. Değişmez kurallar
Gemini'ye müvekkil verisi/PIN/parola/kişisel veri gitmez; Gemini çıktısı veridir ve güvenlik içerenler `guvenlik-denetci`
ajanından geçer. PIN'i kullanıcı girer. `--dangerously-skip-permissions` kullanılmaz. Elle deneyden sonra ArmoniaTest VM kapatılır.
PostgreSQL kalır. Commit/push yalnız kullanıcı isteyince.
> Kapsam: ÇokluUYAP'ı **sertleştirilmiş, canlı (live) bir Debian dağıtımı** olarak paketlemek.
> **Rust dönüşümü bu plana dahil DEĞİLDİR** — `Uyap Haricen Giriş/ruyap/` altında zaten bitmiş,
> derleniyor ve çalışıyor. Bu plan o **bitmiş ikiliyi** Linux için derleyip bir imaja yerleştirmek
> ve sistemi güvenli hale getirmekle ilgilidir.

---

## 0. Temel ilkeler

Gemini'nin "Armonia" setinden **ürün planına alınmayanlar**:

| Alınmayan | Neden | Yerine |
|---|---|---|
| Ring-0 Rust LSM + eBPF/XDP ikili-özet doğrulama | Kodda yok; tek kişilik üründe pratikte yazılamaz | AppArmor + nftables + dm-verity |
| `Iced` ile sıfırdan masaüstü kabuğu | Devasa kapsam; GUI zaten egui *uygulaması* | Hafif WM + tam-ekran kiosk |
| "Encrypted ELF Vault", anti-debug tiyatrosu | Gerçek koruma değil | dm-verity + LUKS2 + appliance mode |
| "%100", "askeri seviye", "imkansız" dili | Fiziksel erişimde hiçbir koruma mutlak değil | Müşteriye **"pratikte çok zor"** |
| 50x hız / 0.02 ms vaatleri | Darboğaz UYAP sunucusu + akıllı kart | Ölçülmüş gerçek rakamlar |

**Korunan gerçek değer:** Debian minbase + toram + LUKS2 + AppArmor + nftables beyaz liste +
systemd hardening + USBGuard + pcscd/Polkit e-imza tekeli + bubblewrap + `lockdown`.

**Taban sürüm: Debian 13 "trixie"** (Kasım 2026 itibarıyla güncel kararlı sürüm; aşağıdaki
referans projelerin çoğu da 13'ü hedefliyor). Debian 12 eski-kararlı olarak yedek seçenek.

---

## 1. ARAÇ YIĞINI — doğrulanmış (2026-11-13 araştırması + bu makinede test)

### 1.1. Bu makinenin durumu (ölçüldü)
| Bileşen | Durum |
|---|---|
| Donanım sanallaştırma (BIOS) | ✅ Açık |
| WSL2 dağıtımı | ✅ **Debian 13 (trixie), WSL2, systemd açık** — kullanıcı `utku` (sudo). live-build, mmdebstrap 1.5.7, xorriso, GRUB, gcc 14.2, Rust 1.100.0 kurulu (2026-11-13) |
| VirtualBox / Docker | ❌ Yok |
| Hyper-V | ? (Win10 Pro'da mevcut; yönetici yetkisiyle kontrol edilmeli) |
| Node / npx, uv / uvx | ✅ Var (MCP sunucuları için yeterli) |
| **Antigravity CLI `agy` 1.2.8** | ✅ Kurulu, oturum açık, **etkileşimsiz `agy -p` test edildi: çalışıyor** |
| Claude Code MCP sunucusu | ✅ Context7 (global) |
| Claude eklentileri | ✅ Trail of Bits: insecure-defaults, sharp-edges, rust-review, second-opinion |
| RAM | 7,9 GB — VM + derleme aynı anda sıkışık; diğer programlar kapatılmalı |

### 1.2. Derleme ve test ortamı (ZORUNLU İLK KAPI)
`live-build`/`mmdebstrap` yalnızca Linux'ta çalışır. Bu makine için en iyi kombinasyon:

- **Derleme: WSL2 + Debian** (`wsl --install -d Debian`, yönetici yetkisi gerekir).
  - ISO derleme (`live-build`) ve **Rust ikilisinin Linux için derlenmesi** burada yapılır.
  - Bonus: Linux dosya sisteminde Türkçe `İ` yol sorunu hiç yok.
  - Windows 10'da WSL2 içinde iç içe KVM yok → QEMU testi yavaş (emülasyon). Bu yüzden:
- **Test: Hyper-V (Win10 Pro yerleşik) veya VirtualBox** — üretilen ISO Windows tarafında
  donanım hızında açılıp test edilir.
- **Faz 7:** gerçek bir boş PC/USB ile donanım testi.

### 1.3. Gemini'yi agent olarak kullanmak — DOĞRULANDI
Araştırmada iki yol bulundu; **doğrudan yol test edildi ve çalışıyor**:

| Yol | Durum | Karar |
|---|---|---|
| **Doğrudan `agy -p` (Bash)** | ✅ Test edildi: `status: SUCCESS`, 2,5 sn, JSON çıktı, `conversation_id` ile devam | **ANA YOL** — sıfır bağımlılık, üçüncü taraf kod yok |
| MCP köprüsü (`mcp-server-google-antigravity`, `antigravity-claude-mcp` vb.) | Topluluk paketleri; asenkron iş kuyruğu sunar | Gerekirse sonra; üçüncü taraf npm kodu çalıştırır, önce incelenmeli |
| Gemini CLI / PAL MCP (eski zen-mcp) | Gemini CLI kişisel planlarda **2026-06-18'de emekliye ayrıldı** | ❌ Bu kurulum için geçersiz |

**Antigravity'de mevcut modeller (`agy models`):** Gemini 4 Argon (low/medium/high),
Gemini 4 Hydrogen (low→xhigh), Gemini 3.8 Flash (low/medium/high), Claude Opus 5.5,
Claude Sonnet 5.5, GPT-OSS 120B.

**Kullanım kuralları** (proje skill'i `.claude/skills/gemini-ajan/` içinde):
- Gemini **taslak** üretir, **dosyayı Claude yazar** (Gemini metni JSON yanıtta döner).
- Analiz görevleri `--mode plan` ile (düzenleme yok). `--dangerously-skip-permissions` **asla**.
- Her çağrıda ~12.500 token sabit yük ölçüldü → küçük işleri **tek çağrıda topla**.
- Gemini'ye müvekkil verisi, PIN, kişisel veri **gönderilmez**; yalnız proje/config içeriği.
- **Her güvenlik çıktısı `guvenlik-denetci` alt-ajanından geçer** (mutlak-güvenlik dili ayıklanır).

### 1.4. MCP sunucuları
| MCP | Ne işe yarar | Kurulum | Öncelik |
|---|---|---|---|
| **Context7** (Upstash, resmi) | live-build, AppArmor, nftables, systemd, cryptsetup güncel dokümanı → uydurma config önler | `npx ctx7 setup --claude` (OAuth/API anahtarı, kullanıcı yapar) | 🟢 Yüksek |
| Antigravity köprüsü | Asenkron Gemini işleri | `claude mcp add ...` | 🟡 Gerekirse |
| Filesystem/shell MCP | — | — | ❌ Gereksiz (Claude Code araçları yeterli) |

> Context7 aynı zamanda `agy mcp add` ile Gemini'ye de eklenebilir → iki model de aynı güncel dokümanı görür.

> **Ölçülen sınır (2026-11-13):** Context7 **Debian live-build'i kapsamıyor** (arama alakasız sonuç döndü).
> Debian sistem araçları (live-build, live-boot, cryptsetup, AppArmor araçları) için doğruluk kaynağı:
> **WSL'de kurulu paketin kendi kaynağı ve kılavuzu** (ör. `/usr/lib/live/build/config`,
> `/usr/share/live/build/functions/`). Context7 Rust kütüphaneleri ve genel yazılım dokümanı için kullanılır.

### 1.5. Skill ve eklentiler (GitHub araştırması)
| Kaynak | Ne verir | Bu projede ne zaman | Not |
|---|---|---|---|
| **Proje skill'i `gemini-ajan`** (yazıldı) | Gemini'ye `agy` ile doğru görev verme | Her faz | `.claude/skills/gemini-ajan/SKILL.md` |
| **`trailofbits/skills`** | ✅ Kurulu: **insecure-defaults** (güvensiz varsayılanlar), **sharp-edges** (tehlikeli yapılandırma), **rust-review** (`uyap_proxy.rs` incelemesi), **second-opinion** (`agy` ile Gemini ikinci görüşü) | Faz 1 sonu (rust-review), Faz 3 ve Faz 6 | 47 eklentiden yalnız bunlar seçildi. **`zeroize-audit` bilerek kurulmadı:** her oturumda sürümü sabitlenmemiş Serena'yı GitHub'dan indirip çalıştırıyor (tedarik zinciri riski) ve C/C++ odaklı. Kurulum: `claude plugin install <ad>@trailofbits` (`/plugin` uzaktan erişimde çalışmıyor) |
| **`trailofbits/skills-curated`** | Trail of Bits'in **incelediği** eklentiler | Üçüncü taraf skill almadan önce | Bu repo, yayınlanmış bazı skill'lerde **arka kapı ve kötü niyetli hook** bulunduğunu belgeliyor → yalnız incelenmiş kaynak |
| **`obra/superpowers`** (MIT) | Plan → uygula → doğrula disiplini, alt-ajan güdümlü geliştirme | Opsiyonel; uzun fazlarda | Ağır bir iş akışı getirir; önce Faz 1'de denenmeli |
| **Proje skill'i `iso-derle`** (yazılacak) | "ISO derle + test et" tek komut | Faz 1 oturduktan sonra | |

### 1.6. Claude alt-ajanları
| Alt-ajan | Görev | Durum |
|---|---|---|
| **`guvenlik-denetci`** | AppArmor/nftables/systemd/sysctl çıktılarını doğruluk, sözdizimi, aşırı-izin ve **abartılı güvenlik iddiası** açısından denetler | ✅ `.claude/agents/guvenlik-denetci.md` yazıldı |
| **`gemini`** | Görevi `agy -p` ile Gemini'ye yaptırır, özet döner; arka planda/paralel çalışır. Aracı model Haiku (Claude kotası az harcanır) | ✅ `.claude/agents/gemini.md` — ilk deneme başarılı (toram sorusu, 17,5 sn) |
| `sharp-edges-analyzer`, rust-review ajanları | Eklentilerle gelen denetim ajanları | ✅ Kurulu |
| `general-purpose` (yerleşik) | Bağımsız config taslaklarını paralel üretme | Faz 3 |
| `Explore` (yerleşik) | Kod tabanında hızlı tarama (port, yol, env değişkeni bulma) | Her faz |

---

## 2. GITHUB REFERANS PROJELERİ (tekerleği yeniden icat etmemek için)

Hiçbir proje bizim ihtiyacımızın tamamını (sertleştirilmiş + toram + LUKS2 + kiosk) karşılamıyor;
parçaları birleştireceğiz. Lisanslar ve bakım durumu **kullanmadan önce** kontrol edilecek.

| Proje | Neyi ondan alırız | Faz |
|---|---|---|
| **Debian `live-build` / `live-boot`** (resmi) | ISO iskeleti, `toram` boot parametresi | 1 |
| **`wesbluemarine/debianmaker`** | Tamamen RAM'de çalışan (toram) imaj üretimi | 1 |
| **Botmonster live-build kiosk rehberi** | Tek uygulamalı kiosk imajı + toram, USB'yi boot sonrası çıkarma | 1, 5 |
| **`convolutionary/phiOS`** | LUKS2 (aes-xts 512-bit + argon2id) sertleştirilmiş Debian 13; ISO'yu Debian imza anahtarıyla doğrulayan yayın akışı | 2, 6 |
| **`linuxtuts-website/vaevictis-os`** | Sertleştirilmiş Debian 13: betikli build, LUKS2, şifreli kurulum | 2, 3 |
| **`msw/CISS.debian.live.builder`** | LUKS2 + dm-integrity içinde şifreli canlı kök, deterministik build | 6 |
| **`Kicksecure/security-misc`** | Hazır çekirdek/sysctl sertleştirme **Debian paketi** (doğrudan kurulabilir) | 3 |
| **`konstruktoid/ansible-role-hardening`** (Apache-2.0) | Debian 12/13 sertleştirme rolü — kontrol listesi olarak | 3 |
| **`ansible-lockdown/DEBIAN12-CIS` + `-Audit`** | CIS uyum denetimi (GOSS ile otomatik test) | 3, 7 |
| **`ovh/debian-cis`** | CIS tabanlı modüler kabuk betikleri (Debian 11–13) | 3 |

**Yaklaşım:** Faz 3'te kuralları elle yazmak yerine `security-misc` paketini temel alıp, CIS denetim
setiyle (`DEBIAN12-CIS-Audit` / `lynis`) ölçeceğiz. Kendi yazdığımız tek şey ÇokluUYAP'a özel kurallar
(egress beyaz liste, 9. madde kill-switch, Akis tekeli) olacak.

---

## 3. YOL HARİTASI — fazlar

Her faz: **çıktı + test kapısı + kim yapar**. Test kapısı geçilmeden sonraki faza geçilmez.

### Faz 0 — Ortam ve kararlar (1–2 gün)
- [x] WSL2 + Debian 13 kurulur. *(2026-11-13)*
- [x] Hyper-V açıldı; kullanıcı Hyper-V Yöneticileri grubuna eklendi. *(2026-11-13)*
- [x] Context7 MCP eklenir. *(2026-11-13)*
- [x] `trailofbits/skills` eklenir, 4 eklenti kurulur. *(2026-11-13)*
- [x] §5'teki kararlar verildi. *(2026-11-13)*
- [x] Test ISO'su derlendi: `C:\dev\armonia_test.iso` (334 MB, 9 dk; betik `C:\dev\test_iso.sh`;
      SHA-256 `aa2baf1f…d1b3d`). *(2026-11-13)*
- **Test kapısı: ✅ GEÇİLDİ (2026-11-13 23:20).** `lb --version` çalışıyor; test ISO'su Hyper-V'de (Gen 1, 2 GB,
  `C:\dev\hyperv_test.ps1`) açılış menüsü → çekirdek 6.12.116 → systemd → `tty1` konsoluna ~80 sn'de ulaştı.
  - **Otomatik test döngüsü kuruldu (kullanıcıya gerek yok):** VM yönetimi izinsiz (Hyper-V Yöneticileri);
    ekran görüntüsü WMI ile (`C:\dev\vm_ekran.ps1`, 640x480 çalışıyor, 800x600+ "geçersiz durum" veriyor);
    tuş gönderme `Msvm_Keyboard.TypeKey`; açılış bekleme Hyper-V kalp atışıyla.
  - **Öğrenilen 1:** `--apt-recommends false` → `user-setup` ve `sudo` gelmiyor → live-config `user`
    hesabını oluşturamıyor (`Authentication failure`). Armonia'da genel `user`/`sudo` zaten istenmiyor;
    Faz 1'de kiosk kullanıcısı açıkça tanımlanacak.
  - **Öğrenilen 2:** `toram` parametresi ana menü girişinde mevcut; RAM'den çalışma Faz 1'de doğrulanacak
    (açılıştan sonra sanal DVD çıkarılıp sistemin çalıştığı görülecek).
  - Gen 2 (UEFI) VM testi Faz 6'ya (Secure Boot ile birlikte) bırakıldı.
- **Kim:** Kullanıcı (yönetici kurulumları, OAuth, kararlar) · Claude (kurulum betikleri).

### Faz 1 — Minimal önyüklenebilir sistem + ÇokluUYAP açılıyor (en kritik faz)
- [x] **Duman testi:** `ruyap/` WSL içinde Linux için `--release` derlendi — 13 dk 40 sn, 0 hata,
      `uyap_app` 41,9 MB. Ölçülen korumalar: PIE ✅, NX ✅, tam RELRO ✅; sembol temizliği ❌.
      Dinamik bağımlılık yalnız libc/libm/libgcc_s; grafik kütüphaneleri çalışma anında yükleniyor.
      *(Not: Faz 0 test kapısı geçilmeden, bekleme süresini değerlendirmek için öne alındı.)*
- [ ] **İmaj derlemesi** (`C:\dev\ruyap_derle.sh` güncellenecek):
  - yalnız imajda çalışacak programlar (`uyap_app`, gerekirse `office_agent` — koddan doğrulanacak);
    Windows araçları (`msi_hazirla`, `kurucu`, `paket_hazirla`) ve bulut parçaları
    (`vendor_server`, `signaling_server`) imaja **girmez**
  - ince LTO + sembol temizliği — ayar betikte (`CARGO_PROFILE_RELEASE_*`), paylaşılan `Cargo.toml`'a dokunulmaz
  - `--locked` (bağımlılık sürümleri kilitli)
  - `cargo audit` ile bilinen açık taraması (RustSec)
  - kaynak kopyası her derlemede birebir eşitlenir, ikiliye kaynak commit numarası kaydedilir
  - **Statik musl derlemesi YAPILMAZ:** Akis sürücüsü (`libakisp11.so`) çalışma anında
    `libloading` ile yükleniyor (`src/akis_pkcs11.rs`), glibc ile dinamik bağlanma zorunlu.
- [ ] `rust-review` eklentisiyle `uyap_proxy.rs` güvenlik incelemesi.
- [ ] `live-build` ile Debian 13 minimal imaj + `toram`.
- [ ] Linux ikilisi + `enjekte/` + `webapp/` imaja konur; açılışta otomatik başlar.
- [ ] **Açılış hızı** (Faz 0 test ISO'su ~80 sn sürdü, ayarsız genel Debian live idi; kabul edilemez):
  - önce `systemd-analyze` / `systemd-analyze blame` / `critical-chain` ile **ölçülür**, sonra kısılır
  - açılış menüsü beklemesi 0 (isolinux/grub timeout), squashfs `zstd` sıkıştırma
  - gereksiz servisler maskelenir (`*-wait-online`, `apt-daily*`, `man-db`, `e2scrub*`, `getty` fazlası)
  - initramfs küçültülür (`MODULES=dep`/hedef donanım); ağ ÇokluUYAP'ı bekletmez (arka planda gelir)
  - `toram` kopyalama süresi ayrıca ölçülür; nihai sistem SSD'den açılacağı için kopyalama ürün yolunda olmayacak
- Koddan doğrulananlar (2026-11-13): imaja **yalnız `uyap_app`** girer — `office_agent` ve `home_client`
  ayrı süreç değil, `uyap_app`'in içinde modül. Vekil (8800) giriş + e-imza PIN'inden sonra başlar,
  açılışta yalnız giriş kartı (egui) görünür. `enjekte/` ve `webapp/` imaja ana kaynaktan
  (`Uyap Haricen Giriş/…`) alınır; `ruyap/` içindeki kopyalar eski. `static_cache` (188 MB) bir çalışma
  önbelleği, imaja girmez. Linux'ta `encrypt_secret` (DPAPI) yok → "beni hatırla" parolası saklanamaz;
  Faz 2'de LUKS kasası / çekirdek anahtarlığı ile çözülecek. Linux'ta Segoe UI yok → egui varsayılan
  fontu; Faz 5'te Linux font yolu eklenecek.
- Dosyalar: `armonia-linux/derle_uyap.sh` (WSL, utku), `armonia-linux/iso_derle.sh` (WSL, root),
  `armonia-linux/config/` (live-build katmanı), `C:\dev\hyperv_armonia.ps1` + `C:\dev\com_oku.ps1`
  (Hyper-V + COM1 borusundan ölçüm okuma).
- [x] **İlk ölçüm (2026-11-13 23:56, Hyper-V Gen1, 2 GB, toram):** ISO 445 MB (squashfs zstd 389 MB),
      `lb build` 6 dk, `uyap_app` 30,5 MB (LTO+strip; önce 41,9). Çekirdek başlangıcından itibaren
      **labwc 8,5 sn, uyap_app 8,9 sn** (toram kopyası dahil); makineyi açmaktan kiosk'a ~10 sn.
      Bellek: toplam 798 MB kullanılan (475 MB'ı RAM'deki squashfs), uyap_app 132 MB, labwc 24 MB.
      Ekranda giriş kartı doğru çiziliyor (Türkçe karakterler tamam).
  - Açık: Xwayland (62 MB) gereksiz başlıyor → kapatılacak · `armonia-olcum` oneshot olduğu için
    `systemd-analyze` "bitmedi" diyor → ölçüm servisi graphical.target'ı bekletmeyecek şekilde düzeltilecek ·
    `/etc/default/locale` imajda yok uyarısı · toram tmpfs %96 dolu (498 MB sınır) ·
    `/mnt/c`'den gelen dosyalar 0777 girmişti → betikte düzeltildi, sonraki derlemede doğrulanacak ·
    Hyper-V COM borusu adında `_` "Parametre hatalı" verdi → `\\.\pipe\armoniacom`.
- **Test kapısı:** Hyper-V'de ISO açılır, ÇokluUYAP giriş kartı ekrana gelir (e-imzasız; labwc + uyap_app ayakta).
  **Hedef:** açılış menüsünden ÇokluUYAP ekranına ≤ 20 sn (Hyper-V); gerçek SSD donanımda ≤ 15 sn
  (BIOS/POST ve kasa parolasını yazma süresi hariç).
- **Kim:** Claude (live-build config, servis birimleri) · Gemini (paket listesi taslağı, debianmaker/kiosk rehberi özeti).
- ✅ **FAZ 1 KAPANDI (2026-11-14 00:39):** Hyper-V'de `Startup finished 6,25 + 1,85 = 8,1 sn`, giriş kartı ekranda,
  `systemctl` running, başarısız birim yok. rust-review + cargo-audit yapıldı (bulgular ve kararlar:
  `docs/ARMONIA_GECE_DURUM.md` A2/A3). Test: `iso-derle` skill'i + `armonia-linux/test/hyperv_test.ps1`.

### Faz 2 — Kalıcılık ve veritabanı (Sunucu modu)
- [~] **Kasa katmanı yazıldı** (`armonia-linux/kasa/`, Plymouth teması `armonia-linux/plymouth/`), güvenlik denetiminin
      K1–K3, O1–O5, D1/D3/D5 düzeltmeleri uygulandı. **Hyper-V'de uçtan uca doğrulandı (2026-11-14 03:39):** Plymouth
      parola ekranı → LUKS2 açılıyor → `/srv/kasa` + CokluUyap dizinleri kasada → kiosk; lynis 71 / 0 uyarı.
      Yolda bulunan hata: wall ajanı Plymouth parola ajanını durduruyordu (maskelendi). Açık: kurtarma anahtarı +
      başlık yedeği, gerçek kurulum sihirbazı (RAM'e göre argon2 belleği), Türkçe karakterli parola, yanlış parola
      ipucunun ekranda testi. Ayrıntı: `docs/ARMONIA_GECE_DURUM.md` C2.
- [ ] 2. bölüm: **LUKS2 + Argon2id** (phiOS ayarları referans), `--perf-no_read/write_workqueue`.
- [x] **PostgreSQL (2026-11-14, Hyper-V'de doğrulandı):** `kasa/.../armonia-postgresql.service` — veri kasada, yalnız
      Unix soketi (TCP yok), peer kimlik doğrulama, yalnız kasada `armonia/rol-sunucu` işareti varsa; kiosk'a
      DATABASE_URL ortam dosyasıyla; Sunucu rolünde PG hazır değilse kiosk açılmaz (veri bölünmesi önlemi);
      `systemd-analyze security` 0,4 SAFE; guvenlik-denetci düzeltmeleri uygulandı. Açık: uygulama kodunda sessiz
      SQLite'a düşme (kullanıcı kararı), pg_dump yedeği (Faz 6), rol işaretini yazacak kurulum sihirbazı.
- [ ] Kasa kilidini açma ekranı — **KARAR (2026-11-13, kullanıcı): A) Açılışta, Plymouth ile.**
  - Parola, sistem yüklenmeden erken açılışta sorulur (Linux'un standart yolu).
  - Plymouth "script" teması `armonia_acilis_v2` görseline yaklaştırılır: A logosu, "Armonia" başlığı,
    parola kutusu, tema arka plan rengi.
  - Bu yolun sınırları (kullanıcıya bildirildi): tıklanabilir düğme yok (Enter ile açılır); açılışta tema
    değiştirilemez (tek sabit Armonia teması); klavye düzeni (TR-Q/TR-F) cihaz kurulumunda sabitlenir;
    Akis durumu bu ekranda gösterilmez.
  - (Değerlendirilip alınmayan B seçeneği: ÇokluUYAP içinde temalı kasa ekranı + dar yetkili açma servisi.)
  - **Parola kuralı — KARAR (2026-11-14, kullanıcı):** Türkçe karakter yok (yalnız ASCII); en az bir büyük harf,
    bir küçük harf, bir rakam ve bir sembol zorunlu; **en az 10 karakter**. Denetim: `armonia-linux/kasa/opt/armonia/bin/kasa_parola_kurali.sh`
    (stdin'den okur; kurulum sihirbazı ve test biçimlendirmesi kullanır).
  - Görseldeki düzeltmeler geçerli: "AES-512" yazılmaz (512 bit anahtar = XTS kipinde AES-256);
    "Akis E-İmza Hazır" gösterilmez; "EROFS" yazılmaz.
- **Test kapısı:** Bölüm açılıyor, PostgreSQL kalkıyor, kapanışta kilitleniyor; İstemci modunda PG başlamıyor.
- **Kim:** Claude · Gemini (PostgreSQL ayar taslağı).

### Faz 3 — Sertleştirme
- [x] **İlk katman canlı ISO'da (2026-11-14):** `armonia-linux/sertlestirme/` — sysctl, modül kara listesi, nftables
      (giriş DROP, UDP 4430 yalnız özel ağ), kiosk sandbox (`systemd-analyze security` 1,9 OK), `lockdown=integrity`,
      `ia32_emulation=0`, açılış menüsü kilitli (`noescape`), live-config kaldırıldı. Gemini taslağı → guvenlik-denetci
      (K1 ProtectHome kiosk'u kırıyordu, K2 nftables yüklenmiyordu — düzeltildi) → Hyper-V'de doğrulandı.
      Kalan: giden beyaz liste/kill-switch, AppArmor, USBGuard, pcscd/Polkit, lynis canlı sistem puanı.
- [ ] `Kicksecure/security-misc` temel; üstüne ÇokluUYAP'a özel kurallar.
- [ ] **nftables:** varsayılan DROP + giden beyaz liste + **9. madde kill-switch**.
- [~] **AppArmor** profilleri; Akis `libakisp11.so` yalnız ÇokluUYAP ikilisine. **2026-11-14:** `uyap_app` profili
      (`sertlestirme/etc/apparmor.d/`) imajda yükleniyor, **complain** kipinde; enforce gerçek kullanım kayıtlarından sonra.
- [ ] **systemd hardening**, **USBGuard**, **pcscd/Polkit tekeli**, **bubblewrap**, `lockdown`, appliance mode.
- **Test kapısı:** nmap → 0 açık port; izinsiz hosta bağlantı reddi; `hukukcu` e-imzayı göremiyor;
  izinsiz USB reddi; `lynis` + CIS denetim skoru raporlanır.
- **Kim:** Gemini (çok sayıda profil/unit taslağı, paralel) → **`guvenlik-denetci`** denetler →
  Claude son halini yazar · `trailofbits/skills` ile ek inceleme.

### Faz 4 — Donanım ve dış entegrasyon
- [ ] Akis sürücüsü (`opensc pcscd libpcsclite1` + TÜBİTAK `.so`), CUPS+SANE (`cups.socket`),
      Ungoogled-Chromium (UYAP Portal, e-Duruşma), **UYAP Editörü sunucuda da** (Java + AppCDS, bubblewrap içinde — §5 karar 5).
- **Test kapısı:** Gerçek Akis kartıyla PIN + imza; yazıcı çıktısı; e-Duruşma kamerası.
- **Kim:** Kullanıcı (gerçek donanım) · Claude (entegrasyon).

### Faz 5 — Kullanıcı deneyimi (hafif kiosk)
- [ ] `labwc` (Wayland) + ÇokluUYAP tam ekran autostart; Plymouth; TR-Q/TR-F.
- [ ] Mevcut tema dili (`#F1EEE8` / `#7C9A7E`); Akis/UYAP/istemci/ağ göstergeleri.
- **Sıfırdan masaüstü kabuğu yazılmaz.**
- **Masaüstü tasarımı — kullanıcı kararları (2026-11-13)** (Gemini'nin `armonia_masaustu_onizleme` görseli esas):
  - Beğenilen ve alınacaklar: **"A" logosu**, **alttaki dock**, **eş yükselti çizgili arka plan**.
  - Masaüstünde **simge yok**; yerine **widget'lar**: arama çubuğu, İş Listesi, takvim (+ canlı durum).
    - Veri kaynakları mevcut: İş Listesi modülü (`is_listesi`, kesinleşme/teslim tarihleri),
      "Yaklaşan Duruşmalarım" ajandası (`anasayfa.js`).
    - Arama: esas no → dosyayı açar (mevcut sorgu kısayolu); isim → İş Listesi ve indirilmiş veride arar.
  - **Dock, ÇokluUYAP'ın görev çubuğunun YERİNE geçer.** Solda "A" (Başlat: UYAP menüsü + İş Listesi),
    ortada sabit programlar (UYAP & e-Duruşma, UYAP Editörü, Belgelerim, Yazıcı), sağda **açık dosyalar
    yatay sıralı** (görev çubuğundaki gibi; tıkla → öne getir, × → kapat, taşarsa kaydır).
  - Uygulama:
    - Masaüstü ve dock, `uyap_app`'in sunduğu yerel sayfalar (mevcut 9 tema geçerli).
    - Dock ↔ ÇokluUYAP aynı kaynak (`127.0.0.1:8800`) → BroadcastChannel ile senkron.
    - `gorevcubugu.js` kartları yönetmeye devam eder; **yalnız Armonia kipinde** kendi çubuğunu gizleyip
      listeyi dock'a yayınlar. **Windows/tarayıcı davranışı değişmez.**
    - labwc 0.8.3 (kılavuzdan doğrulandı): `<margin bottom="…">` dock alanını ayırır (büyütülen pencereler
      girmez); dock penceresi için `serverDecoration=no`, `fixedPosition=yes`, `skipTaskbar=yes`.
    - Pencereyi öne getirme: `wlrctl` (Debian 13'te var); üst çubuk: waybar 0.12 (temaya uyarlanır).
  - **Pencere yöneticisi labwc'de kalır** (Hyprland değerlendirildi, alınmadı: yalnız backports'ta,
    efektler zayıf donanımda ağır, döşeme düzeni kullanıcılara yabancı).
- **Test kapısı:** Boot → parola → doğrudan tam ekran ÇokluUYAP.
- **Kim:** Claude + Gemini (autostart/plymouth taslakları).

### Faz 6 — Bütünlük, yedek, güncelleme
- [ ] dm-verity + Secure Boot (CISS/phiOS referans), age şifreli yedek, Ed25519 imzalı güncelleme,
      (opsiyonel) TPM2 + Akis ile LUKS2 bağlama.
- **Test kapısı:** 1 bit değişince boot reddi; yedek başka makinede geri yükleniyor.
- **Kim:** Claude · Kullanıcı (Secure Boot anahtar kaydı) · `trailofbits/skills` inceleme.

### Faz 7 — Test matrisi + dağıtım
- [ ] 4 varyasyon: (USB/Disk) × (Sunucu/İstemci); gerçek donanım; CIS denetimi (GOSS).
- [ ] USB yazma aracı + kurulum ve müşteri kılavuzu.
- **Test kapısı:** Gerçek kartla uçtan uca bir icra dosyası işlemi.
- **Kim:** Kullanıcı (donanım + UYAP) · Claude (test listesi, kılavuz).

---

## 4. İş bölümü özeti

| İş | Kim / hangi araç |
|---|---|
| Güvenlik kararları, kill-switch, Akis tekeli, boot zinciri, dm-verity, son dosyalar | **Claude Code** |
| Çok sayıda config **taslağı**, uzun doküman özetleri, referans repo incelemesi | **Gemini (`agy -p`, `gemini-ajan` skill'i)** |
| Güvenlik çıktılarının denetimi | **`guvenlik-denetci` alt-ajanı** + `trailofbits/skills` |
| Güncel doküman doğrulama | **Context7 MCP** |
| Bağımsız taslakların paralel üretimi | **Claude alt-ajanları** veya paralel `agy -p` çağrıları |
| ISO derleme / Linux ikilisi | **WSL2 Debian** |
| ISO açılış testi | **Hyper-V / VirtualBox** |
| Gerçek kart, yazıcı, donanım, UYAP uçtan uca | **Kullanıcı** |

---

## 5. Kararlar — VERİLDİ (2026-11-13, kullanıcı onayı)

| # | Konu | Karar | Sonucu |
|---|---|---|---|
| 1 | Kalıcı bölüm | **Sunucunun dahili SSD'si** (LUKS2) | Faz 2: veri bölümü dahili diskte; USB'li taşınabilir varyant sonraya |
| 2 | İstemci bağlantısı | **Sabit IP gerekmez** | İstemciler sunucuyu mevcut cokluuyap.com eşleştirmesiyle (P2P) buluyor |
| 3 | Akis token | **Sunucuda** | Mevcut "Paylaş" düzeni; pcscd/AppArmor tekeli (Faz 3) sunucuda kurulur |
| 4 | Yönetim erişimi | **Yerel ekran-klavye + ÇokluUYAP paneli; SSH YOK** | Kapalı kutu cihaz; nftables'ta hiçbir yönetim portu açılmaz |
| 5 | UYAP Editörü | **Sunucuda da** (istemcilere ek olarak) | Faz 4: Java + UYAP Editörü imaja girer; bubblewrap içinde çalışır, AppCDS ile hızlandırılır. İmaj büyür — ağ ve e-imza ihtiyacı Faz 4'te koddan/denemeyle doğrulanacak |
| 6 | Test sanallaştırması | **Hyper-V** | ✅ Açıldı; kullanıcı "Hyper-V Yöneticileri" grubuna eklendi (test VM'leri izinsiz yönetilebilir) |

---

## 6. İlk somut adımlar

1. **(Kullanıcı, yönetici PowerShell)** `wsl --install -d Debian` → yeniden başlat → Debian kullanıcı adı/parola.
2. **(Kullanıcı)** `npx ctx7 setup --claude` → Context7 OAuth.
3. **(Claude)** WSL'de `live-build mmdebstrap` + Rust araç zinciri kurulumu, `ruyap` Linux derlemesi.
4. **(Birlikte)** §5 kararları → Faz 1: ilk açılan minimal ISO.

---

### Kaynaklar (araştırma)
- Antigravity MCP köprüsü: https://github.com/TurkerYakup/mcp-server-google-antigravity
- Gemini CLI emeklilik notu: https://github.com/jamubc/gemini-mcp-tool
- Antigravity MCP belgesi: https://antigravity.google/docs/mcp/
- Zen/PAL MCP: https://claudelog.com/claude-code-mcps/zen-mcp-server
- Context7: https://claudelog.com/addons/context7-mcp
- Trail of Bits skills: https://github.com/trailofbits/skills · https://github.com/trailofbits/skills-curated
- Superpowers: https://claudemarketplaces.com/plugins/obra-superpowers/superpowers
- phiOS: https://github.com/convolutionary/phiOS
- VaeVictis OS: https://github.com/linuxtuts-website/vaevictis-os
- debianmaker: https://github.com/wesbluemarine/debianmaker
- Kiosk rehberi: https://botmonster.com/self-hosting/build-custom-linux-iso-live-build-cubic/
- CISS live builder: https://git.coresecret.dev/msw/CISS.debian.live.builder
- Kicksecure security-misc: https://github.com/Kicksecure/security-misc
- ansible-role-hardening: https://www.github.com/konstruktoid/ansible-role-hardening
- ansible-lockdown: https://github.com/orgs/ansible-lockdown/repositories
- OVH debian-cis (ayna): https://gitea.soloconlinux.org.es/luisgulo/debian-cis/src/tag/latest/MANUAL.md
