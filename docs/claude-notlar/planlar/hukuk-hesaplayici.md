# Plan: Hukuk Hesaplayıcı (Armonia'nın ilk kendi uygulaması) — 2026-11-19

Durum: **ONAYLANDI (19.11).** Kullanıcı: "Veri çekmeye ilişkin modülü sonra yazacağız."
→ H1–H4'te veri katmanı yalnız **okuma** arayüzü olarak yazılır (gömülü dosya + kullanıcı eklemesi). Oranları,
tarifeleri ve dini bayram tarihlerini kaynaklardan toplayan / güncelleyen **veri çekme modülü ayrı iş** (iş
listesinde). Kanunla sabit tatiller (2429 s.K.: 1 Ocak, 23 Nisan, 1 Mayıs, 19 Mayıs, 15 Temmuz, 30 Ağustos,
28 Ekim yarım, 29 Ekim) koddan hesaplanır; dini bayramlar o zamana dek kullanıcı tarafından eklenir.

## Kullanıcı kararları (19.11)
| Konu | Karar |
|---|---|
| İlk sürümün hesapları | Süre, faiz, vekalet ücreti (AAÜT), harç ve masraf |
| Oran / tarife verisi | Programa gömülü (kaynak + yürürlük tarihi yazılı), sistem güncellemesiyle yenilenir; kullanıcı yeni oranı kendisi de ekleyebilir. İnternet gerekmez |
| Platform | Armonia (Linux) + Windows; aynı kod (egui) |

Neden: Çoklu UYAP'tan bağımsız, her hukukçunun işine yarayan ve hazır Debian karşılığı olmayan bir uygulama.

## Yapı
- Yeni Rust paketi: `armonia-uygulamalar/hukuk_hesap/` (depo kökünde). İki parça:
  - **`cekirdek` (kütüphane):** arayüzsüz, saf hesap. Para kuruş cinsinden tamsayı (yuvarlama kuralları açık),
    tarih `chrono`. Her hesap sonuç + **döküm** (adım adım, dayanak madde) döner. Birim testleri burada.
  - **`uygulama` (egui 0.29, ruyap ile aynı sürüm):** dört sekme — Süre · Faiz · Vekalet Ücreti · Harç.
    Armonia renkleri (krem / #5F7A61). "Kopyala" (dilekçeye yapıştırılacak düz metin döküm).
- ruyap'a dokunulmaz. `icra_durum.rs`'teki iş günü / tatil mantığı çekirdeğe **taşınarak değil kopyalanarak**
  alınır; ileride ruyap bu kütüphaneyi kullanabilir (ayrı karar).
- Veri: `veri/*.toml` (yasal faiz, avans faizi, AAÜT yılları, harç tarifesi yılları, resmî tatiller). Programa
  `include_str!` ile gömülür. Kullanıcı eklemeleri ayrı dosyada (Linux `~/.local/share/armonia/hukuk_hesap/`,
  Windows `%APPDATA%\Armonia\hukuk_hesap\`); gömülü veriyi ezmez, "kullanıcı ekledi" diye işaretlenir.
- Her ekranda: kullanılan tarifenin adı, yürürlük tarihi ve Resmî Gazete kaynağı + "Hesap bilgilendirme
  amaçlıdır; güncel mevzuatı kontrol edin."

## Hesaplar

### H1 — Süre hesabı
- Girdi: başlangıç (tebliğ/öğrenme) tarihi, süre (gün / hafta / ay / yıl), tebligat türü (fiziki / e-tebligat),
  süre türü (adli tatil uygulanır mı).
- Kurallar (HMK 92–93, 104; Tebligat K. 7/a + Elektronik Tebligat Yönetmeliği; İİK süre hükümleri):
  ilk gün sayılmaz; hafta/ay/yıl karşılık gelen güne denk; son gün hafta sonu/resmî tatilse ilk iş günü;
  adli tatile (20 Temmuz–31 Ağustos) denk gelen süre tatilin bitiminden itibaren 1 hafta uzar (uygulanan
  davalarda); e-tebligatta tebliğ, ulaştığı günü izleyen 5. günün sonunda.
- Hazır şablonlar (her biri dayanak maddesiyle; **değerler gerçek mevzuattan tek tek doğrulanacak**):
  cevap dilekçesi, istinaf, temyiz, icra ödeme emrine itiraz, icra şikâyeti, karar düzeltme yok vb.
- Resmî tatiller: yıl yıl gömülü liste (dini bayramlar dahil). **Açık karar:** arife (yarım gün) son güne denk
  gelirse süre uzar mı — uygulama/içtihat kullanıcıyla netleştirilecek; şimdilik ayar.

### H2 — Faiz hesabı
- Türler: yasal faiz (3095 s.K. m.1), ticari / avans faizi (3095 m.2/2, TCMB avans oranı), sözleşmesel
  (kullanıcı oranı). Basit faiz, gün esası (365).
- Başlangıç–bitiş arası oran değişimlerine göre **dönemlere bölünür**; her dönemin gün, oran, faiz tutarı
  dökümü. Birden fazla alacak kalemi (farklı başlangıç tarihleri).
- Döviz alacağı (3095 m.4/a) ve kısmi ödemelerin mahsubu (TBK 100) — H2'nin ikinci adımı.

### H3 — Vekalet ücreti (AAÜT)
- Yıl seçimi (karar/takip tarihine göre). Nispi ücret dilimleri (Üçüncü Kısım) + mahkeme türüne göre maktu
  alt sınır (nispi ücret maktudan az olamaz kuralı), icra takipleri, tahliye vb. özel hükümler.
- Dava değeri girilir; döküm dilim dilim gösterilir.

### H4 — Harç ve masraf
- 492 s. Harçlar Kanunu tarifeleri (yıllık genel tebliğ): başvurma, peşin (1/4) ve karar-ilam harcı, maktu
  harçlar, vekalet harcı ve baro pulu; icra: peşin harç, tahsil harcı oranları, cezaevi harcı.
- Dava açılışı ve icra takibi için "toplam ödenecek" özeti.

### H5 — Armonia ve Windows'a koyma
- Armonia: `/opt/armonia/hukuk_hesap/` + `.desktop` + dock kutucuğu ("Hesap"); ağ erişimi gerekmez.
- Windows: tek exe (MSI'ye eklenmesi ayrı karar).
- Pi'de ARM için derlenip denenir; amd64 derlemesi WSL'de.

## Veri doğruluğu (en önemli risk)
- Oran ve tarifeleri **ben uydurmam.** Her değer Resmî Gazete / TCMB / TBB kaynağından, sayı + tarih +
  bağlantıyla `veri/` dosyasına yazılır. Bulamadığım ya da emin olmadığım değer "DOĞRULANMADI" işaretlenir ve
  programda uyarıyla gösterilir.
- Her aşama sonunda size bir **doğrulama tablosu** sunarım (değer, kaynak); yayından önce sizin onayınız.
- Birim testleri: elle hesaplanmış örnekler + sınır durumlar (dönem sınırı, artık yıl, tatil zinciri, adli
  tatil başı/sonu).

## Sıra
H1 (süre + uygulama iskeleti) → H2 (faiz) → H3 (AAÜT) → H4 (harç) → H5 (paketleme). Her aşama sonunda Pi'de
ekran görüntüsü + doğrulama tablosu. Commit yalnız istenince.

## İlerleme
### 19.11 — H1 yazıldı
- `armonia-uygulamalar/hukuk_hesap/`: kütüphane (`cekirdek/`: tarih, tatil, sure, sablon, kayit) + egui uygulaması
  (`src/bin/hukuk_hesap.rs`). `cargo test --no-default-features` → 13/13. Pi'de (ARM) derlendi, ekransız labwc'de
  açılıp ekran görüntüsü alındı.
- Kanuni tatiller koddan (2429 s.K.; 1 Mayıs 2009+, 15 Temmuz 2017+); dini bayramlar kullanıcı ekler
  (`~/.local/share/armonia/hukuk_hesap/tatiller.json`).
- Tarih girişi metin (gg.aa.yyyy) — egui_extras takvim seçicisi İngilizce olduğundan alınmadı.
- Doğrulama bekleyenler: 8 hazır süre (`sablon.rs`, hepsi `dogrulandi: false`), HMK 104 yorumu, e-tebligat +5,
  UYAP'ta son gün 24.00, Cumartesi'nin iş günü sayılmaması, yarım gün varsayılanı (uzatmaz).

### 19.11 — H2 yazıldı
- `cekirdek/faiz.rs`, `cekirdek/para.rs`, gömülü veri `veri/faiz.json`; kullanıcı TCMB satırları
  `tcmb_oranlari.json`. Testler 22/22.
- Kaynaklar: 3095 güncel metni (mevzuat.gov.tr; m.1 7589 s.K. ile değişik, yürürlük 31.07.2026: önceki yıl
  31 Aralık reeskont oranının %80'i, 30 Haziran'da 5 puan kuralı), TCMB reeskont/avans tablosu, UAB kanuni ve
  temerrüt faizi tablosu (09.09.2026). Yasal faiz: %12 (01.05.2005), %9 (01.01.2006, BKK 2005/9831), %24
  (01.06.2024, CK 8485), %31 (31.07.2026, kural). Ticari: avans, yasaldan düşükse yasal.
- **Açık soru:** 3095 m.2/2 metni avans oranını 31 Aralık / 30 Haziran kuralıyla bağlıyor; UAB tablosu TCMB
  değişiklik tarihlerinden uyguluyor (2025: %49,25 → 08.03 %44,25 → 17.09 %42,25). İkisi de seçenek; varsayılan
  kanun metni.
- Kabuller (doğrulanacak): basit faiz, 365 gün (artık yılda da), başlangıç günü sayılır / son gün sayılmaz, her
  dönem kuruşa yuvarlanır.

### 19.11 — Hesap makinesi + H3 yazıldı
- Kullanıcı aslında yalnız basit hesap makinesi istemişti ("bu da iyi olmuş, devam edelim"). "Hesap Makinesi"
  sekmesi eklendi ve açılışta gelir (`cekirdek/ifade.rs`: dört işlem, yüzde, parantez, "12.500" binlik okunur).
- H3: `cekirdek/vekalet.rs` + `veri/aaut.json` (AAÜT 2026–2027, RG 03.11.2026/33419; ekler taranmış görüntü,
  sayfalar görsel okunarak aktarıldı; nispi dilim toplamı 24.800.000 TL tutuyor). Kurallar: m.13/1–3, m.6, m.11/1
  ve 11/4 (78.125 TL eşiği), m.16/2 (62.500 TL eşiği). Testler 30/30.
- Kabuller (doğrulanacak): iş/ticaret/aile mahkemeleri "Asliye" maktu ücretiyle; m.6 yarı ücreti maktu sınırı ve
  miktar sınırından sonra uygulanıyor; icra nispi ücreti de icra dairesi maktusundan az olamaz.
