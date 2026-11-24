# Çoklu UYAP'ı Armonia'da imajı yeniden derlemeden güncelleme (taslak plan)

Claude, 2026-11-24. Durum: **TASLAK, onay bekliyor**. guvenlik-denetci 24.11: "DÜZELTMEYLE KULLANILABİLİR";
bulgular (K1–K3, O1–O7, D1–D5) aşağıya işlendi, özet en altta.

## Amaç
Çoklu UYAP'ın yeni sürümü çıktığında Armonia kullanıcısı yeni ISO beklemeden güncelleyebilsin; sahte, bozuk ya da
eski bir sürüm kurulamasın; kum havuzu (armonia-cokluuyap.service) hiçbir şekilde gevşemesin.

## Neden düz .deb değil
- `/opt/armonia/cokluuyap` imajın parçası, root'a ait, salt okunur; uygulama `ProtectSystem=strict` ile çalışır.
  Uygulamanın ya da oturumun buraya yazabilmesi, ele geçirilen bir sürecin Çoklu UYAP'ı değiştirebilmesi demektir.
- `dpkg -i` imza denetlemez; kullanıcıya "siteden .deb indir, parolanı yaz" alışkanlığı kazandırır.
- Canlı USB'de dpkg ile kurulan RAM'e gider, yeniden açılışta silinir.
- .deb bakımcı betikleri (postinst) root olarak çalışır: güncelleme yalnız dosya değil, kod çalıştırma yetkisi olur.

## Tehdit modeli (neye karşı)
| Tehdit | Önlem |
|---|---|
| Sahte güncelleme (site ele geçirildi, ağda araya girme, DNS) | Ed25519 imzası; gizli anahtar sunucuda **yok**, çevrimdışı |
| Eski, açığı bilinen sürüme düşürme (rollback) | İmzalı sürüm numarası, kurulu/imaj sürümünden büyük olmalı; yerel "önceki sürüme dön" YOK, yalnız imaja dönüş (K2) |
| Dondurma (eski ama geçerli bilgiyi sürekli sunma) | İmzalı bilgide `gecerlilik_sonu`; geçmişse "bilgi alınamıyor" uyarısı; saat `yayin`'dan ya da imaj derleme tarihinden gerideyse ret (O4) |
| Oturumdaki süreç Çoklu UYAP'ı öldürüp geri dönüşü tetikler | Çökmede yalnız o açılış için imaj sürümüne düşülür; sürüm yerelde "kötü" işaretlenmez, geri çekme yalnız imzalı `geri_cekilen` listesiyle (K2) |
| Gizli anahtar sızar | Yalnız yedek anahtarın imzalayabildiği kalıcı `iptal` bildirimi (O5) |
| Karıştırma (imzalı bilgi + başka paket) | Paketin sha256'sı ve boyutu imzalı bilginin içinde |
| Zararlı arşiv (../, sembolik bağ, aygıt dosyası, dev boyut) | Kendi açıcımız: yalnız düz dosya/dizin, yol beyaz listesi, boyut sınırı |
| Yeni sürüm daha fazla izin ister (yeni port, yol) | `imaj_arayuzu` alanı; imajınkinden büyükse kurulmaz → yeni imaj gerekir |
| İndirici ele geçirilir (TLS/HTTP kitaplığı açığı) | İndirici ayrı, yetkisiz, yazamaz; doğrulayıcının ağı yok |
| Uygulama kendini güncellemeye çalışır | Linux'ta uygulama yalnız uyarır; kurulum dizinine erişimi zaten salt okunur |

Korumadığı: iki anahtarın birden çalınması, Çoklu UYAP kaynağına sızılması (imzalayan, imzaladığına güvenir),
root yetkisi ele geçirilmiş bir sistem. Canlı USB'de (toram) tek yönlü sayaç her açılışta sıfırlanır: orada
koruma "imaj tabanı + `gecerlilik_sonu` penceresi" kadardır (O4); saat kimlik doğrulamasız NTP'den geliyorsa
pencere saati geri alan ağ saldırganına karşı zayıflar (timesyncd durumu DOĞRULANMADI).

## Mimari

```
cokluuyap.com/indir/linux/
  guncelleme.json        (imzalı bilgi)          ← armonia-guncelle-denetle (zamanlayıcı, yetkisiz, yalnız 443)
  guncelleme.json.sig    (Ed25519)
  cokluuyap-<sürüm>-x86_64.tar.zst               ← armonia-guncelle-indir (kullanıcı isteyince, yetkisiz)
  cokluuyap-<sürüm>-aarch64.tar.zst
                                                   ↓ /var/lib/armonia-guncelle/gelen/ (yalnız indirici yazar)
                                                 armonia-guncelle-kur (ağsız; doğrular, açar, etkinleştirir)
                                                   ↓ /var/lib/armonia-guncelle/cokluuyap/<sürüm>/ (salt okunur)
                                                 armonia-cokluuyap.service → çalıştırırken etkin sürümü seçer
```

### 1. İmzalı güncelleme bilgisi (`guncelleme.json`)
```json
{ "bicim": 1, "urun": "cokluuyap-linux", "kanal": "kararli",
  "surum": "1.8.0", "yayin": "2026-12-01T10:00:00Z", "gecerlilik_sonu": "2027-01-15T00:00:00Z",
  "imaj_arayuzu": 1, "geri_cekilen": ["1.7.2"],
  "paketler": { "x86_64":  { "ad": "cokluuyap-1.8.0-x86_64.tar.zst",  "sha256": "…", "boyut": 41234567,
                             "agac_ozeti": "<paketteki MANIFEST'in sha256'sı>" },
                "aarch64": { "ad": "cokluuyap-1.8.0-aarch64.tar.zst", "sha256": "…", "boyut": 39876543,
                             "agac_ozeti": "…" } },
  "notlar": "Kısa sürüm notu (düz metin, en çok 2000 karakter)" }
```
- İmza dosyası: `{ "anahtar_kimligi": "<ilk 8 bayt sha256(açık anahtar)>", "imza": "<64 bayt b64>" }`.
- Zorunlu denetimler: `bicim == 1`, `urun == "cokluuyap-linux"`, `kanal`, mimari anahtarı, `yayin ≤ şimdi`,
  `yayin ≥ imaj derleme tarihi`, `şimdi < gecerlilik_sonu`. Etkin sürüm `geri_cekilen` içindeyse imaja dönülür.
- İmza, dosyanın **ham baytları** üzerinedir ve önüne alan ayırıcı eklenir: `"armonia-guncelleme-v1\n" || bayt`
  (aynı anahtarla imzalanan başka bir şey güncelleme bilgisi yerine geçemesin). Doğrulama `verify_strict`.
- Paket adresi bilgide **yok**: yalnız `ad` var, adres koddaki sabit kökle birleştirilir
  (`https://www.cokluuyap.com/indir/linux/`); `ad` için `^[a-z0-9.\-]+\.tar\.zst$`. Yönlendirme yalnız aynı ana
  makineye izinli.
- Sürüm karşılaştırması sayısal (semver, ön sürüm eki kabul edilmez).
- Boyut sınırları: bilgi 64 KB, imza 64 B tam, paket 256 MB.

### 2. Paket (`.tar.zst`)
İçerik yalnız: `uyap_app`, `webapp/**`, `uyap_core/enjekte/**`, `SURUM`, `MANIFEST` (her dosyanın yolu + sha256;
özeti imzalı bilgide `agac_ozeti`). `pgsql/` ya da başka çalıştırılabilir dizin listeye **girmez**
(`veritabani.rs:129` ikilinin yanındaki `pgsql/bin`'i çalıştırır; D2). Sürüm `SURUM`'dan değil imzalı bilgiden
okunur. Açıcı (Rust, `tar` + `zstd` crate'leri, kendi denetimiyle):
- Yalnız düz dosya ve dizin; sembolik/sabit bağ, aygıt, FIFO → ret. Mutlak yol, `/` ile başlayan ad, `..`, boş
  bileşen, NUL → ret; PAX/GNU uzun adlar da aynı denetimden geçer; yinelenen girdi → ret.
- Açılan her dosya MANIFEST'te olmalı, MANIFEST'teki her dosya açılmalı, özetler tutmalı.
- Açmadan önce `statvfs` ile boş alan (canlıda RAM) denetlenir (D3).
- İlk bileşen beyaz listede değilse ret. Dosya sayısı ≤ 5000, açılmış toplam ≤ 512 MB (zip bombası).
- İzinler bizden: dizin 0755, dosya 0644, yalnız `uyap_app` 0755; sahip `armonia-guncelle`. setuid/setgid yok.
- `uyap_app` ELF başlığı beklenen mimariyle (cokluuyap_hazirla.sh'deki `elf_mimari` mantığı) tutmalı.

### 3. Armonia tarafı: üç küçük birim, tek ikili (`/opt/armonia/guncelle/armonia-guncelle`, Rust)
Root gerekmez. Ayrı sistem kullanıcıları:
- **`armonia-guncelle-denetle.service` + `.timer`** (günde bir, `RandomizedDelaySec=2h`, oturum açılışından
  10 dk sonra bir kez): `DynamicUser=yes`, yalnız bilgi + imzayı indirir ve doğrular, sonucu
  `/run/armonia-guncelle/durum.json`'a yazar (yalnız: sürüm, notlar, durum). Paket indirmez. Dizin tmpfiles.d
  ile kurulur (oneshot çıkınca RuntimeDirectory silinirdi; O6). Durum dosyasına güvenilmez: Ayarlar "Güncelle"den
  önce imzalı bilgiyi kendisi doğrular, notları oradan gösterir.
- **`armonia-guncelle-indir.service`**: kullanıcı "Güncelle" deyince. Kullanıcı `armonia-guncelle-indir`;
  yalnız `/var/lib/armonia-guncelle/gelen/` yazılabilir. İndirirken sha256 ve boyut sınırı akış halinde
  denetlenir; aşılırsa keser ve siler.
- **`armonia-guncelle-kur.service`**: indirici başarıyla bitince. Kullanıcı `armonia-guncelle`;
  `PrivateNetwork=yes` + `IPAddressDeny=any`; `Conflicts=armonia-guncelle-indir.service` ve flock (O1).
  `gelen/` içindeki paketi **bir kez** açar (`O_RDONLY|O_NOFOLLOW`, `fstat`: düz dosya, sahibi indirici UID,
  `nlink==1`), okurken sha256'yı hesaplayıp baytları **kendi** `.gecici/` dizinine kopyalar; imza, özet ve
  açma yalnız bu kopya üzerinde. `imaj_arayuzu`, mimari, sürüm büyüklüğü denetlenir; `cokluuyap/.<sürüm>.gecici/`
  içine açar, `fsync`, `rename` ile `cokluuyap/<sürüm>/`, imzalı bilgiyi yanına koyar, `etkin` bağını atomik
  değiştirir. En çok iki sürüm tutulur; **etkin ve o an çalışan sürüm silinmez** (O3).
  `/var/lib/armonia-guncelle/en_yuksek` (tek yönlü sayaç) ve kalıcı `iptal` kaydı burada.
- Dizinler StateDirectory ile değil tmpfiles.d ile, açık sahiplikle (O1):
  `d /var/lib/armonia-guncelle 0755 armonia-guncelle armonia-guncelle`,
  `d /var/lib/armonia-guncelle/gelen 0750 armonia-guncelle-indir armonia-guncelle`; kurucuda `ReadWritePaths`
  yalnız `cokluuyap/`, `en_yuksek`, `iptal`.
- Ortak sertleştirme (yazi-katmani ile aynı çizgi; D4): `ProtectSystem=strict`, `ProtectHome=yes`,
  `NoNewPrivileges`, `CapabilityBoundingSet=`, `PrivateDevices`, `PrivateIPC`, `PrivateTmp`, `ProtectKernel*`,
  `ProtectProc=invisible`, `SystemCallFilter=@system-service`, `RestrictAddressFamilies` (kur: `AF_UNIX`),
  `MemoryMax`, `MemoryDenyWriteExecute`.
- Ağ (K3): `output` zincirinin politikası accept ve `oturum_yerel` yalnız `skuid armonia` için; yeni birimler
  oraya düşmez. Bu yüzden:
  - `nftables.conf`'a boş `chain guncelle_yerel {}` ve `output` içinde `jump cokluuyap_yerel`'den sonra
    `jump guncelle_yerel`. Kurallar `/opt/armonia/nft/guncelle_yerel.nft`'de, `socket cgroupv2 level 2
    "armonia.slice/armonia-guncelle.slice"` ile: 127.0.0.53:53 kabul → `fib daddr type local reject` → özel
    ağlar ret. Birimlerde `ExecStartPre=+/usr/sbin/nft -f …` (`-` öneki yok), `PartOf=nftables.service`.
  - Ayrıca birimde `IPAddressAllow=127.0.0.53/32` + `IPAddressDeny=localhost link-local multicast 10.0.0.0/8
    172.16.0.0/12 192.168.0.0/16 100.64.0.0/10 fc00::/7` (slice'a bağlı kalmasın).
  - nft ana makine adına göre süzemez: indirici her adresin 443'üne çıkabilir. Ana makine kısıtı yalnız kodda
    (sabit kök, rustls, yönlendirme yalnız aynı ana makineye; D1). Güven kaynağı TLS değil imzadır.

### 4. Çoklu UYAP'ın etkin sürümü seçmesi
- `cokluuyap_calistir.sh` bağı **bir kez** çözer (`D=$(readlink -e …/etkin)`), sonra yalnız o somut dizinle
  çalışır (O3): `armonia-guncelle dogrula-agac "$D"` → başarılıysa `exec "$D/uyap_app"`, değilse
  `exec /opt/armonia/cokluuyap/uyap_app`. Sürüm, `$D` yanındaki imzalı `guncelleme.json`'dan okunur; imaj
  sürümünden büyük değilse imaj kullanılır.
- Bütünlük denetimi **root olarak değil**, kum havuzunun içinde `User=armonia` ile, salt okunur ve `O_NOFOLLOW`
  ile yapılır (O2): imza → `agac_ozeti` → MANIFEST → her dosyanın özeti. Başarısızsa hiçbir şey silinmez, yalnız
  imaj sürümüne düşülür ve günlüğe yazılır. Ağacı değiştiren biri MANIFEST'i de değiştirse imzalı `agac_ozeti`
  tutmaz. Maliyet: açılışta ~50 MB özet.
- AppArmor (K1): profil yalnız `/opt/armonia/cokluuyap/uyap_app`'e bağlıydı; yeni yoldan çalışan ikili profilsiz
  kalırdı. Bağlanma yolu
  `profile uyap_app /{opt/armonia/cokluuyap,var/lib/armonia-guncelle/cokluuyap/[0-9]*}/uyap_app`, ek kurallar
  `/var/lib/armonia-guncelle/cokluuyap/[0-9]*/uyap_app mr,` ve `/var/lib/armonia-guncelle/cokluuyap/[0-9]*/** r,`.
  Kanca `apparmor_parser -Q` ile dener; `olcum.sh` güncellenmiş sürümde de etiketin `unconfined` olmadığını denetler.
- Kum havuzu değişmez: `/var/lib/armonia-guncelle` `ProtectSystem=strict` altında salt okunur görünür (UID ayrımı
  sayesinde `armonia` yazamaz); `/var/lib/armonia` ile çakışmaz. `gelen/` Çoklu UYAP'a `InaccessiblePaths` ile kapalı.
- Çökme (K2): `Restart=on-failure` + `StartLimitBurst=3/300s` değişmez. Güncellenmiş sürüm sınırı aşıp `failed`
  olursa (`OnFailure=` → `armonia-guncelle-imaja-don.service`) **yalnız o açılış için** imaj sürümü seçilir
  (`/run/armonia-guncelle/imaja-don` işareti). Sürüm yerelde "kötü" işaretlenmez, önceki güncellemeye dönülmez:
  oturumdaki bir süreç `kill -SEGV` ile Çoklu UYAP'ı eski açıklı sürüme indiremesin. Sürümü geri çekmek yalnız
  imzalı `geri_cekilen` listesiyle olur. (OnFailure'un StartLimit ile davranışı Linux'ta denenecek; DOĞRULANMADI.)

### 5. Kullanıcı arayüzü
- Waybar'da küçük gösterge (durum.json'dan; ağ yok, yalnız dosya okur): "Çoklu UYAP güncellemesi var".
- Tıklayınca Ayarlar → **Güncellemeler** bölümü: sürüm, notlar (düz metin), "Güncelle", "İmajdaki sürüme dön"
  (önceki güncellemeye dönüş yok; K2).
- polkit (O7): systemd yalnız `org.freedesktop.systemd1.manage-units` sorar; kural 60-armonia-cokluuyap.rules
  biçiminde: `unit == "armonia-guncelle-indir.service"`, `verb == "start"`, yerel + etkin oturum →
  `polkit.Result.AUTH_SELF_KEEP`. Kurulumdan sonra Çoklu UYAP'ı yeniden başlatma: mevcut kural zaten
  `armonia`'ya stop/start veriyor; Ayarlar bunu kullanır, kurucuya ek yetki verilmez.
  **Sınır:** ISO canlıda kullanıcı parolası henüz yok (G3 bekliyor); orada `AUTH_SELF_KEEP` parolasız geçebilir
  ya da hiç çalışmayabilir (DOĞRULANMADI). G3 gelene kadar canlı ISO'da düğme kapalı tutulabilir (açık soru 3).
- Çoklu UYAP açıksa: "Kurulum bitince Çoklu UYAP yeniden başlatılacak; süren işler durur" onayı.
- Uygulama içi uyarı (Windows için öbür pencerede yapılan özellik): Linux sürümünde uygulama **indirmez/kurmaz**,
  yalnız "Yeni sürüm var — Armonia Ayarlar → Güncellemeler'den kurun" der. Ortam değişkeni
  `ARMONIA_GUNCELLEME=sistem` birimden verilir; uygulama bunu görünce kendi güncelleyicisini kapatır.

### 6. Canlı USB ve kalıcılık
- Diske kurulu / Pi (LUKS2 kök): `/var/lib/armonia-guncelle` kalıcı, güncelleme kalır.
- Canlı ISO (toram, kasa yok — G2 rafta): güncelleme o oturumluk (RAM). Ayarlar bunu açıkça söyler:
  "USB'den çalışıyorsunuz: güncelleme kapanınca silinir; kalıcı için yeni ISO". G2 kasası gelince dizin kasaya
  taşınır (ayrı iş).
- İmaj sürümü her zaman taban: kasadaki sürüm imajdakinden eskiyse kullanılmaz.

### 7. Anahtar yönetimi ve yayın akışı
- Ed25519 anahtar çifti, **Linux için ayrı** (Windows güncelleme anahtarıyla aynı olmasın: biri sızarsa diğeri
  etkilenmez). Gizli anahtar parolayla şifreli (scrypt/age), çevrimdışı USB'de; sunucuya ve depoya girmez.
- İmajda iki açık anahtar: `etkin` ve `yedek` (gizlileri ayrı yerlerde, ayrı parolalarla).
  - Güncelleme bilgisini **yalnız `etkin`** imzalayabilir (1-of-1; yedek güncelleme imzalayamaz, saldırı yüzeyi
    büyümez).
  - **`yedek` yalnız iptal bildirimi** imzalar: `{"islem":"iptal","anahtar_kimligi":"<etkin>","yeni_etkin":
    "<açık anahtar>"}`. Cihaz bunu görünce eski anahtarı kalıcı reddeder (`iptal` kaydı; Pi'de diskte) ve yeni
    etkin anahtarı kabul eder. Böylece kurulu Pi'ler yeniden yazılmadan anahtar değiştirilebilir (O5).
  - İki gizli anahtar birden sızarsa tek çare yeni imaj.
- Alan ayırıcı (`armonia-guncelleme-v1\n`, iptal için `armonia-iptal-v1\n`) imzalama aracı ve doğrulayıcıda
  aynı sabitten gelir; test önek olmadan alınmış imzayı reddetmeli.
- Yayın betiği `armonia-linux/guncelleme_yayinla.sh` (depoya): her iki mimarideki `derle_uyap.sh` çıktısından
  paket üretir, `guncelleme.json` yazar; imzalama ayrı komut (`armonia-guncelle imzala`, çevrimdışı makinede).
  Yükleme elle (cokluuyap.com/indir/linux/).
- `armonia-guncelle dogrula <json> <sig> [paket]`: yüklemeden önce son denetim.

## Değişecek / eklenecek dosyalar
- Yeni: `armonia-guncelle/` (Rust crate: ed25519-dalek, sha2, tar, zstd, ureq+rustls, serde_json; testli).
- Yeni katman: `armonia-linux/guncelle-katmani/` (birimler, timer, slice, tmpfiles.d, nft, polkit kuralı,
  kanca 0230).
- Değişen: `sertlestirme/etc/nftables.conf` (boş `guncelle_yerel` zinciri + jump), AppArmor profili
  `opt.armonia.cokluuyap.uyap_app` (iki katmandaki kopya), `cokluuyap_calistir.sh` (tek çözümle sürüm seçimi ve
  kum havuzu içinde doğrulama), `armonia-cokluuyap.service` (OnFailure → imaja dön,
  `InaccessiblePaths=/var/lib/armonia-guncelle/gelen`, `ARMONIA_GUNCELLEME=sistem`),
  `iso_derle.sh` / `pi_derle.sh` (GUNCELLE=1, son denetimler), Ayarlar (Güncellemeler bölümü), waybar göstergesi,
  `olcum.sh` (indirici 5975/8800/yerel ağa gidemez; kur ağsız).
- Çoklu UYAP (ruyap): `ARMONIA_GUNCELLEME=sistem` iken uygulama içi güncelleyiciyi devre dışı bırakma.
- Hiçbir mevcut güvenlik katmanı kaldırılmaz ya da gevşetilmez.

## Aşamalar
1. ✅ (24.11) `armonia-guncelle/` crate: bilgi biçimi, imza doğrulama, açıcı, testler (sahte imza, eski sürüm, `..`,
   sembolik bağ, boyut, yanlış mimari, süresi geçmiş bilgi). Windows'tan çapraz `cargo check/clippy`.
   - Modüller: `anahtar` (ring Ed25519, alan ayırıcı, iptal zinciri), `gizli` (scrypt N=2^17 + ChaCha20-Poly1305),
     `bilgi`, `paket` (kendi tar denetimi, ruzstd, MANIFEST), `agac`, `kur`, `sec`, `surum`, `zaman`, `dosya`
     (O_NOFOLLOW açma, atomik yazma).
   - Komutlar: yayıncı `anahtar-uret / imzala [--iptal] / paketle / bilgi-olustur / iptal-olustur / dogrula`;
     cihaz `kur / etkin-sec / iptal-ekle` (sabit yollar, canlıda kapalı).
   - Testler: 3 birim + 12 uçtan uca (sahte/yedek/öneksiz imza, alan denetimleri, saat geri, imajdan eski
     yayın, geri düşürme, paket kurcalama, yanlış mimari ELF, 8 zararlı arşiv türü, kurulu ağaç kurcalama
     (MANIFEST yeniden yazılsa da), sembolik bağ (unix), geri çekme + sahte geri çekme, budama, iptal zinciri).
     clippy temiz (Windows ve x86_64-linux hedefi).
   - Not: doğrulama ring ED25519 ile (RFC 8032, S < L denetimi); dalek'in `verify_strict`'i yerine. Anahtarı biz
     ürettiğimiz için küçük dereceli anahtar sorunu bizi etkilemez.
   - Ağ (aşama 2'de eklendi): `ag.rs` ureq 3 + rustls (ring) + webpki-roots; yalnız HTTPS, yönlendirme yok,
     vekil yok, yalnız 200; gerçek siteye bağlantı denendi (`tests/ag_canli.rs`, elle).
2. ✅ (24.11) Katman `armonia-linux/guncelle-katmani/` (YALNIZ Pi; `pi_derle.sh` GUNCELLE, anahtar dosyası yoksa
   açık uyarıyla 0; `iso_derle.sh` canlı ISO'da katman varsa durur): denetle/indir/kur/imaja-don/buda birimleri,
   zamanlayıcı, slice, tmpfiles (gelen/ 2750 setgid), `guncelle_yerel.nft` + nftables.conf zinciri, polkit
   61-armonia-guncelle.rules, drop-in `armonia-cokluuyap.service.d/50-guncelle.conf`, kanca 0230,
   `guncelle_hazirla.sh`, `armonia-guncelle/derle.sh`.
3. ✅ (24.11) `cokluuyap_calistir.sh` etkin-sec ile seçim; AppArmor profili güncelleme yoluna bağlandı
   (yalnız `cokluuyap-katmani/menuden/` kopyası; `cokluuyap-katmani/etc/apparmor.d/` eski kiosk kopyasıdır,
   hiçbir betik kullanmıyor).
   - Denetçi 2. tur (uygulama, 24.11): "DÜZELTMEYLE KULLANILABİLİR"; düzeltilenler:
     K-1 gelen/ setgid 2750 (kurucu indiricinin dosyalarını okuyamıyordu) · O-1 yedek anahtar tek başına yetmesin:
     anahtar dosyasında `sonraki <sha256>` ön taahhüdü, iptal bildirimi yalnız taahhüt edilen anahtarı etkinleştirir
     ve yeni taahhüt taşır · O-2 imaja-don `reset-failed` (başlatma sınırından sonra hemen açılsın) · O-3 kanca
     `oz-denetim` (canlı denetiminden bağımsız, çıkış kodu denetlenir) · O-4 kurulum sürüm silmez; budama açılışta
     (`armonia-guncelle-buda.service`), bir açılışta en çok 3 sürüm · D-1 yalnız 200 · D-2 /proc/cmdline okunamazsa
     canlı say; işaret okunamazsa imaja dön · D-3 Conflicts= yerine gelen/.kilit flock · D-5 durum.json alanı
     `en_yuksek_kurulu` · D-6 GNU uzun ad, Continuous, seyrek, dosya/dizin çakışması testleri.
   - Kalan (kabul edilmiş / sonraki aşama): D-4 imaja-don parolasız (K2 kapsamında; Ayarlar görünür kılacak),
     D-5/O6 Ayarlar imzayı kendisi doğrulayacak (aşama 4), D-7 açılışta ağaç özeti süresi Pi'de ölçülecek,
     `+` önekli nft ExecStartPre cihazda denenecek.
   - Anahtar dosyası biçimi (`guncelle-katmani/anahtarlar`): `etkin <b64>`, `yedek <b64>`, `sonraki <sha256 hex>`.
     Üç anahtar da `anahtar-uret` ile ayrı parolalarla, çevrimdışı üretilir; `sonraki`nin gizlisi yedekten ayrı
     saklanır.
2. Katman + birimler + nft + polkit; denetçi.
3. Çoklu UYAP birimi ve çalıştırma betiği değişikliği; denetçi.
4. Ayarlar/waybar arayüzü.
5. Yayın betiği ve anahtar üretimi (kullanıcıyla, çevrimdışı).
6. Linux'ta uçtan uca deneme: yerel deneme sunucusuyla (127.0.0.1 değil, ayrı makine/VM) sahte imza, eski
  sürüm, bozuk paket, çöken sürüm → geri dönüş.

## Kullanıcı kararları (24.11)
1. **Anahtar ayrı:** Linux güncelleme anahtarı Windows'unkinden bağımsız. Biçim de bu plandaki kendi biçimimiz.
2. **Yer:** dosyalar `https://www.cokluuyap.com/indir/linux/` altında; insanlara görünen tek indirme sayfası
   (`/indir`) olarak kalır, Linux paketleri o sayfadan bağlanır. Denetleyici yalnız `/indir/linux/` altındaki
   sabit adları okur.
3. **Canlıda güncelleme kapalı:** toram/canlı ISO'da denetle/indir/kur birimleri çalışmaz
   (`ConditionKernelCommandLine=!boot=live`; ayrıca ikili `/proc/cmdline`'da `boot=live` görürse reddeder), Ayarlar
   "USB'den çalışırken güncelleme yok; yeni ISO yazın" der. Bu, O4 (canlıda sayaç sıfırlanır) ve O7 (canlıda
   parola yok) sınırlarını da ortadan kaldırır. Güncelleme yalnız Pi ve diske kurulu sistemde.

## Uygulama notu
- `etkin` sembolik bağ değil, atomik yazılan tek satırlık metin dosyası (`1.8.0`); okuyan sürüm biçimini
  denetler. Sembolik bağ yarışları (O3) ve izleme sorunları hiç doğmaz.
- Seçim tek komutta: `armonia-guncelle etkin-sec` (etkin dosyası → imzalı bilgi → iptal/geri çekme → imaj
  tabanı → ağaç doğrulaması) dizini yazar ya da boş döner; `cokluuyap_calistir.sh` yalnız sonucu kullanır.

## Denetçi raporu (guvenlik-denetci, 24.11) ve işlenişi
Hüküm: **DÜZELTMEYLE KULLANILABİLİR**. Hepsi plana işlendi; kod yazıldıkça her aşama yeniden denetlenecek.
| Bulgu | Konu | Plandaki karşılığı |
|---|---|---|
| K1 | Yeni yoldaki ikili AppArmor profilinin dışında | §4 AppArmor bağlanma yolu + olcum.sh |
| K2 | `kill -SEGV` ile geri dönüş = rollback/dondurma | §4 yalnız imaja, yalnız o açılış; `geri_cekilen`; §5 önceki sürüme dönüş kaldırıldı |
| K3 | Yeni birimler 5975/8800'e açık (oturum_yerel yalnız skuid armonia) | §3 `guncelle_yerel` zinciri + birimde IPAddressDeny |
| O1 | gelen/ TOCTOU, iç içe StateDirectory sahipliği | §3 tek açış + kendi kopyası, Conflicts/flock, tmpfiles.d |
| O2 | Root güvenilmeyen ağaçta dolaşıyor; OZET imzasız | §2 MANIFEST + imzalı `agac_ozeti`; §4 doğrulama kum havuzunda |
| O3 | `etkin` bağı yarışı, çalışan sürümün silinmesi | §4 tek çözüm; §3 etkin/çalışan silinmez |
| O4 | Canlıda rollback/dondurma koruması abartılı; saat | Tehdit modeli sınırı; `yayin`/imaj tarihi tabanı |
| O5 | Anahtar iptali yok | §7 yedek anahtarla iptal bildirimi; zorunlu alan denetimleri |
| O6 | RuntimeDirectory silinir; durum dosyasına güven | §3 tmpfiles.d; Ayarlar kendisi doğrular |
| O7 | polkit eylem kimliği yanlış; canlıda parola yok | §5 manage-units kuralı; canlı ISO sınırı |
| D1–D5 | Ana makine kısıtı, açıcı, RAM, tutarlılık, abartılı ifade | §2, §3, ifadeler düzeltildi |
