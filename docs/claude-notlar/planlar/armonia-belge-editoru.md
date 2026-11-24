# Armonia Belge Editörü — plan (taslak, onay bekliyor)

Kullanıcı isteği (23.11.2026): "LibreOffice güzel ama kendi döküman editörümüz olsun. Hafif olsun; Word ve UDF
açabilsin; PDF yüklediğimizde UDF ya da DOCX olarak kaydedebilsin; PDF'leri biçimi şaşmadan açsın. İleride
cokluuyap.com içine de ekleyeceğiz." Daha önce (23.11): "docx ve türevlerini, udf'yi açacak, türlü formatları
birbirine kolayca ve kayıpsız çevirecek bir döküman editörü."

## 1. Hedef ve dürüst sınırlar

| Özellik | Hedef | Sınır |
|---|---|---|
| UDF aç / düzenle / kaydet | Tam (UYAP'ın kullandığı öğeler) | — |
| DOCX aç / düzenle / kaydet | Hukuk belgesinin kullandığı kısım | Word'ün tamamı değil (makro, izlenen değişiklik, SmartArt, form denetimleri, karmaşık çizim yok). Desteklenmeyen öğe açılışta **bildirilir**, sessizce düşürülmez. |
| UDF ↔ DOCX çevirme | Desteklenen biçimlendirme içinde kayıpsız | Gidiş-dönüş testleriyle ölçülür. |
| PDF görüntüleme | Birebir (pdf.js) | — |
| PDF → UDF / DOCX | En iyi tahmin + önizleme + uyarı | PDF'te paragraf yok, yalnız konumlu harfler var; tam kayıpsız **imkânsız**. Taranmış PDF için OCR (Tesseract, Türkçe). |
| UDF / DOCX → PDF | Birebir (`udf_pdf.rs` motoru) | — |
| `.doc` (eski Word) | Sonraki aşama (karar bekliyor) | İkili biçim; ayrı ayrıştırıcı gerekir. |

"Hukuk belgesinin kullandığı kısım" (ilk hedef): paragraf; yazı tipi, boyut, kalın/italik/altı çizili, renk;
hizalama (iki yana dahil); girinti ve satır/paragraf aralığı; sekme; madde ve numaralı liste; tablo (birleşik
hücre, kenarlık); resim (satır içi); üst/alt bilgi; sayfa numarası; sayfa sonu; sayfa boyutu, yönü ve kenar
boşlukları. UDF'in `content.xml` yapısı bunlarla örtüşüyor (`paragraph`, `content`, `tab`, `space`, `table/row/
cell`, `image`, `header/footer`, `field`, `pageFormat`, `LeftIndent`, `SpaceAbove`, `columnSpans` …).

## 2. Elimizde olanlar (ruyap, Rust)

- `udf_converter.rs` (742 satır): DOCX/PDF → UDF `content.xml`.
- `udf_pdf.rs` (2045 satır): UDF → PDF; UDF sayfa düzenini ve yazı tipi ölçülerini çözüyor.
- `udf_signer.rs`: UDF e-imza. `pdf_birlestir.rs`, `tiff_pdf.rs`.
Bunlar ortak çekirdeğe taşınır (ruyap'tan kopyalanmaz, tek kaynak olur; ruyap da çekirdeği kullanır).

## 3. Mimari

```
belge_cekirdek (Rust kütüphanesi)        ← tek kaynak: belge modeli + UDF/DOCX okuma-yazma + PDF üretimi
   ├─ yerel derleme  → Armonia masaüstü uygulaması, ruyap
   └─ WebAssembly    → tarayıcı (cokluuyap.com)
editör arayüzü (TypeScript, ProseMirror/TipTap)  ← tek kod, iki yerde
   ├─ Armonia: masaüstü kabuğu (karar: Tauri / Chromium uygulama penceresi), kum havuzunda, AĞSIZ
   └─ cokluuyap.com: web sayfası; çeviriler kullanıcının tarayıcısında (WASM), belge sunucuya gitmez
PDF: pdf.js (görüntüleme + metin katmanı), OCR: Tesseract (WASM ya da yerel)
```

- **Belge modeli UDF'e göre kurulur:** editörün iç yapısı = UDF öğeleri. DOCX bu modele çevrilerek açılır.
  Modelde karşılığı olmayan DOCX öğesi "korunmayacak" listesine girer ve kullanıcıya gösterilir.
- **Sayfalı görünüm:** ProseMirror sayfa kavramı taşımaz; A4 sayfalara bölme (sayfa sonu, üst/alt bilgi, sayfa
  numarası) ayrı bir katman olarak yazılır. **İşin en zor kısmı budur.** Satır kırılımı PDF çıktısıyla aynı olmalı:
  ekran ve `udf_pdf.rs` aynı yazı tipi ölçülerini kullanmalı.
- **Yazı tipleri:** UDF/Word belgeleri çoğunlukla Times New Roman / Arial kullanır. Linux'ta ölçüleri birebir aynı
  açık yazı tipleri (Liberation / Tinos, Arimo) imaja eklenir; yoksa satırlar kayar.
- **Lisanslar:** ProseMirror/TipTap (MIT), pdf.js (Apache-2.0), Tesseract (Apache-2.0), Liberation (OFL). Ticari
  kullanıma uygun.

## 4. Armonia'da çalışma ve güvenlik

- Ayrı sistem birimi, Çoklu UYAP ile aynı kum havuzu çizgisi: **ağ yok**, ev dizininde yalnız Belgeler ve
  İndirilenler, D-Bus / pcscd / klavye aygıtı yok. Menüden ve dock'tan ("Yazı" kutucuğu) açılır; `.udf` / `.docx` /
  `.pdf` dosyaları çift tıkla bununla açılır (mimeapps).
- Güvenilmeyen dosya ayrıştırma: zip bombası (açılmış boyut ve dosya sayısı sınırı), XML varlık genişletme kapalı,
  dev resim sınırı, PDF.js kendi kum havuzunda; Rust tarafında `unsafe` yok. Bulanık test (fuzzing) çekirdeğe
  eklenir.
- E-imza: `udf_signer.rs`; kart erişimi yalnız e-imza istemcisi (5975) üzerinden.
- Güvenlik denetçisi her aşamada.

## 5. Aşamalar

1. **Çekirdek + UDF:** `belge_cekirdek` (model, UDF oku/yaz, `udf_pdf` taşınması), gidiş-dönüş testleri.
2. **Editör (Armonia):** sayfalı görünüm, biçimlendirme araç çubuğu, UDF aç/kaydet, PDF'e aktar, kum havuzlu birim.
3. **DOCX:** oku/yaz, UDF ↔ DOCX gidiş-dönüş testleri, "korunmayan öğeler" bildirimi.
4. **PDF:** görüntüleme (pdf.js), metinli PDF → UDF/DOCX, sonra OCR.
5. **cokluuyap.com:** WASM derleme, web sayfası, e-imza ve şablonlar.

Her aşama sonunda: birim + gidiş-dönüş testleri, gerçek belgelerle karşılaştırma (PDF çıktısının görüntü farkı),
güvenlik denetimi.

## 6. Test seti (kullanıcıdan)

Gerçek, kişisel veri çıkarılmış örnekler: dilekçe, tensip, karar/gerekçeli karar, tablolu belge (hesap/masraf),
üst/alt bilgili ve sayfa numaralı belge, resimli belge, Word'de hazırlanmış bir dilekçe, taranmış ve metinli PDF.
Kişisel veri depoya girmez; test seti depo dışında tutulur ya da anonimleştirilir.

## 6b. E-imza (taslak, 23.11 — onay bekliyor)

İki yol, tek imza biçimi: UYAP'ın kabul ettiği `sign.sgn` (CAdES, ruyap `udf_signer.rs` + `cades.rs`; UYAP'ta
denenmiş). İmza her zaman **kaydedilecek son baytların** üzerine atılır; imzalı belge salt okunur açılır (düzenleme
imzayı geçersiz kılar), açılışta imza bilgisi (imzalayan, tarih, geçerlilik) gösterilir.

**A. Bilgisayara takılı kart (Armonia).** Armonia'da karta yalnız e-imza hizmeti erişebiliyor (pcscd polkit
kuralı: yalnız `eimza` kullanıcısı); editör ağsız kum havuzunda ve karta dokunamaz. Öneri: `eimza` kimliğiyle
çalışan küçük bir **imza hizmeti** (Rust; ruyap'taki PKCS#11 + CAdES kodu çekirdeğe taşınır), yerel soket:
editör UDF'i gönderir → hizmet **kendi güvenli penceresinde** belgenin özetini/ilk sayfasını, imzalayacak
sertifikayı gösterip PIN ister → imzalar → imzalı UDF döner. PIN editöre hiç girmez. Alternatif: Adalet E-imza
istemcisi (5975) belge imzalama sunuyorsa onu kullanmak (DOĞRULANMADI; şimdilik yalnız sertifika listesi ve
portal girişi biliniyor). Windows / cokluuyap.com: tarayıcı karta erişemez → yerel Çoklu UYAP uygulaması köprü.

**B. Uzaktaki Çoklu UYAP sunucusu (ofiste takılı kart).** ruyap'ta var olan akış: istemci belgeyi ofise gönderir,
ofis kartla başsız imzalar (ofis girişinde alınan PIN bellekte), imzalı UDF döner (UYAP sayfasındaki "E-imzala").
Editörde "Ofise imzalat": editör ağsız olduğu için belgeyi Çoklu UYAP uygulamasına (yerel, kum havuzlu birim)
verir, o mevcut kimlikli kanaldan (LAN TLS / uzaktan WebRTC) ofise iletir. Güvenlik gereği: ofis "gelen her şeyi
imzalayan" bir makine olmamalı — istek başına onay (ofiste ya da kart sahibinin telefonunda) ya da en azından
kimlik doğrulamalı istek + değiştirilemez imza günlüğü (kim, ne zaman, belge özeti, belge önizlemesi); PIN'in
bellekte süresiz tutulmaması.

**Kararlar (kullanıcı, 23.11):** önce A (bilgisayara takılı kart): "karta erişebilen küçük bir imza hizmeti
yazalım"; "imza günlüğü açalım". PIN sıklığı söylenmedi → her imzada soruluyor (güvenli varsayılan).

**Bulgu (23.11):** Adalet E-imza paketinde AKİS PKCS#11 sürücüsü YOK (Java PC/SC ile karta doğrudan konuşuyor; Pi'de
de bu yüzden çalışıyor). Ama istemci UDF'i kendisi imzalıyor: `POST /api/v1/docsignature/signUdf`
{certificateId, pin, content(base64)} → {content(imzalı UDF), imzaGecerliMi, signatureList}; ayrıca `/signdigest`,
`/verifySignUdf`; sertifikalar `GET /api/v1/signature/getCertificates` (jar sınıflarından okundu; gerçek kartla
DENENMEDİ). → İmza hizmetinin varsayılan arka ucu Adalet istemcisi; PKCS#11 yedek (sürücü kurulursa).

**Yapılan (23.11, `armonia-yazi/imza`):**
- `udf_imza.rs`: UDF imzalama (ruyap cades.rs CAdES-BES, UYAP profili) ve doğrulama (messageDigest, imzanın
  sertifika açık anahtarıyla doğrulanması — RSA / ECDSA P-256/P-384, imzalayan, imza zamanı, geçerlilik süresi).
  Zincir/iptal doğrulanmıyor (açıkça `zincir_dogrulandi: false`).
- `adalet.rs`: 5975 istemcisi (yalnız 127.0.0.1; parçalı yanıt; PIN hatası tanıma).
- `kart.rs`: PKCS#11 (ruyap akis_pkcs11.rs) yedek arka uç.
- `gunluk.rs`: zincirli günlük (her kayıt öncekinin özetini taşır; silme/değiştirme/sıra bozma yakalanır); PIN ve
  belge yazılmaz; günlüğe yazılamazsa imza verilmez.
- `hizmet.rs` + `bin/armonia-imza.rs`: Unix soket hizmeti (systemd soket etkinleştirmesi), isteyen sürecin kimliği
  çekirdekten (SO_PEERCRED), istekler sırayla, yanlış PIN'de 2 sn bekleme, açılışta günlük bozuksa başlamaz;
  arka ucun döndürdüğü belge doğrulanır ve içeriği gönderilenle aynı olmalı. `gunluk-dogrula` alt komutu.
- Testler 9/9 (yazılım ECDSA anahtarı + öz imzalı sertifika; kart biçimi r||s; değiştirilmiş belge; yanlış anahtar;
  bozuk girdi; günlük zinciri; hizmet; içeriği değiştiren arka uç; sahte Adalet istemcisi). clippy temiz.
- Linux'a özgü kısım (soket, SO_PEERCRED) bu makinede derlenemedi (ring için Linux C derleyicisi yok); evde.
- `paket/armonia-imza.{service,socket}` TASLAK (imaja bağlanmadı, güvenlik denetimi bekliyor).

**Kalan:** editörde "İmzala" (PIN penceresi, sertifika seçimi) — Tauri komutu üzerinden; imzalı belgeyi salt okunur
açma ve imza bilgisini gösterme; Armonia entegrasyonu (kullanıcı, birimler, oturumun 5975 erişiminin kapatılması
ki günlüğü atlayan imza olmasın); gerçek kartla deneme; ProtectProc=invisible yüzünden program yolu başka
kullanıcının sürecinden okunamayabilir (uid/pid yine yazılır).

## 7. Kararlar (kullanıcı, 23.11.2026)

- Plan onaylandı.
- Ad: **Armonia Yazı**.
- Armonia kabuğu: **Tauri** (WebKitGTK; çekirdek doğrudan Rust'tan çağrılır).
- `.doc`: ilk sürümde yok, sonraki aşama.
- Kodun yeri: depo kökünde `armonia-yazi/` (çekirdek: `armonia-yazi/belge_cekirdek`).
- ruyap'taki UDF kodu şimdilik **taşınmaz, port edilir**: ruyap'ta başka bir oturumda güncelleme işi sürüyor
  (çakışma olmasın). Çekirdek olgunlaşınca ruyap çekirdeğe bağlanır ve eski kopyalar silinir.

## 8. İlerleme
- [~] 1. aşama: `belge_cekirdek` — model, UDF oku/yaz, gidiş-dönüş testleri.
  - [x] 23.11: `armonia-yazi/belge_cekirdek` (model.rs, udf.rs, testler.rs). Model UDF öğeleri; özellikler ham ve
    sıralı saklanır (bilinmeyen özellik de geri yazılır), metin konumları yazarken UTF-16 ile yeniden hesaplanır.
    Okuma: ZIP bombası / dosya sayısı / boyut sınırı, DTD kapalı, bozuk konum ve tanınmayan öğe **uyarı** olarak
    döner. İmzalı UDF: `imzali=true`, imza geri yazılmaz. 15 test (gidiş-dönüş, düzenleme sonrası konumlar, `]]>`,
    emoji/UTF-16, `\r\n`, denetim karakteri, ZIP bombası, DTD). clippy temiz.
  - [x] 23.11: İlk gerçek UDF (menfi tespit cevap dilekçesi, 128 paragraf, üst bilgide resim, alt bilgide sayfa
    numarası): uyarısız okundu, gidiş-dönüş kayıpsız. Yazıcı UYAP biçimine uyduruldu (sıra üst bilgi → gövde → alt
    bilgi, konumlar öğe sonunda, öğeler arasında satır sonu yok, metin sonunda göstergesiz "\n", resim verisi
    satırlara bölünür): yeniden yazılan `content.xml` UYAP'ınkiyle **bayt bayt aynı**. Görülen yeni özellikler:
    `Hanging`, `SecListTypeLevel1`, `field@fieldName/fieldType/isList`; footer `pageNumber-*` (sayfa numarası alt
    bilgi özelliklerinden çiziliyor). Gerçek örnek testi: `cargo test -- --ignored gercek`.
  - [ ] **UYAP editörünün bizim düzenleyip yazdığımız UDF'i açması** (kullanıcı deneyecek).
  - [x] 23.11: Stil çözücü (`stil.rs`): parça (aile, boyut, kalın, eğik, altı çizili, renk) ve paragraf (hiza,
    girintiler, satır aralığı, boşluklar, sekme durakları, madde/numaralı liste) etkin biçimi; liste sayacı; sayfa
    biçimi (A4, yön, kenar boşlukları); Java mantıksal yazı tipi eşlemesi (Dialog → Arial …); ARGB renk okuma/yazma.
    Kurallar `udf_pdf.rs` ile birebir (ekran = PDF). 9 test (toplam 24).
    Not: `udf_pdf.rs` üst/alt bilgiyi çizerken varsayılan rengi kenarın `foreground`'una çekiyor ve gövdeye de
    taşıyabiliyor (yan etki); çekirdekte gövde varsayılanı siyah. Gerçek örneklerle karşılaştırılacak.
  - [ ] `udf_pdf.rs`'nin çekirdeğe taşınması (PDF çıktısı) — ruyap'taki güncelleme işi bitince.
  - [x] WebAssembly: çekirdek derleniyor; `belge_wasm` köprüsü (`udf_ac`, `udf_kaydet`, `gorunum_hesapla`).
- [~] 2. aşama: editör.
  - [x] 23.11: `arayuz/` (Vite + TypeScript + ProseMirror, ~760 KB). UDF aç (dosya seç / sürükle-bırak), A4 kâğıt
    genişliğinde gösterim (pt ölçüleri, kenar boşlukları, satır yüksekliği udf_pdf formülü), metin düzenleme,
    kalın/italik/altı çizili, hizalama, geri al/yinele, UDF kaydet. Üst/alt bilgi ve tablolar salt okunur (korunur).
    Kayıpsızlık: parça ham özellikleri işarette; yalnız değişen biçim açık özellik olur. CSP ile ağ kapalı.
    Node testi (gerçek belge, gerçek WASM): dokunulmamış belge bayt bayt aynı; kalın, paragraf bölme, ekleme,
    hizalama kaydedilip yeniden açılınca korunuyor (13/13).
  - [x] 23.11: Görsel deneme görünmez Edge ile (`?ornek=/@fs/...`, yalnız geliştirme sunucusu,
    ARMONIA_YAZI_ORNEKLER).
  - [x] 23.11: Sayfalara bölme (`sayfa.ts`): satırlar ölçülür, taşan satırın önüne boşluk (paragraf içinde de);
    kâğıtlar, her sayfada üst/alt bilgi ve sayfa numarası ayrı katmanda. Yazı alanı udf_pdf formülüyle (üst bilgi
    yüksekliği üst marjı aşarsa yazı aşağı iner). Sayfa numarası `pageNumber-*` (çekirdek `SayfaNumarasi`), alt
    bilgi boş olsa da çizilir. Gerçek belge 6 sayfa.
  - [x] 23.11: Sekmeler duraklara göre (`TabSet`, yoksa 72 pt; sol/orta/sağ hizalı duraklar), kullanıcı bildirimi:
    "iki nokta sonrası tab tanınmamış" (sabit genişlik çiziliyordu). Tab tuşu sekme ekler (UYAP gibi).
  - [x] 23.11: Yazı tipi, boyut, renk, satır aralığı seçicileri; girinti artır/azalt (35,4 pt); madde / numaralı
    liste (önekler çekirdekle yeniden hesaplanır). Paragraf alanları değişince UDF'e yazılır (LeftIndent, ...).
    Node testi 21/21.
  - Not: kullanıcının "Veli Sarı ismi kaymış" gözlemi deneme dosyasındaki bilerek yapılan bölmeden (isim, bölünen
    kısa paragrafın ikinci yarısındaydı ve ortalandı) — hata değil.
  - [x] 23.11: Yakınlaştırma (kullanıcı isteği: "A4 dışındaki boşluklar gereksiz"): Sayfa genişliği, Tam sayfa,
    %50–200, Ctrl+tekerlek; seçim hatırlanır (localStorage, hata korumalı). Sayfa CSS transform ile ölçeklenir,
    düzen ölçekten bağımsız (ölçümler ölçeğe bölünür). Satır başları `posAtCoords` yerine metinden (ikiye bölerek)
    bulunur — ölçekli görünümde posAtCoords yanılıyordu. Ölçüm salınımı sigortası (belge değişmeden en çok 4 tur).
  - [x] 23.11: Kompakt görünüm (pencere < 900 px; kullanıcı isteği: Çoklu UYAP yanında dilekçe yazarken belge
    görüntüleme): araç çubuğu tek satır, kaydırılabilir, simgeli (hizalama/liste SVG); kenarlar küçük; sayfa
    otomatik pencere genişliğine sığar (çıkınca kullanıcının seçimine döner).
  - [x] 23.11: Tablo düzenleme. Çekirdek: `TabloStili` (kenarlık hücre/tablo/yok, girinti, boşluk) ve
    `SatirBilgisi` (hücre oranları: satırın columnSpans'i, yoksa tablonunki, yoksa eşit; en az yükseklik) —
    udf_pdf.rs ile aynı. Editör: tablo/satir/hucre düğümleri (kutu + CSS grid; sayfa boşluğu satırlar arasına
    konabilsin), hücrelerde tam biçimlendirme. Komutlar: tablo ekle (2×2, 3×3) / sil, satır ekle (üst/alt) / sil,
    sütun ekle (sol/sağ; yeni sütun bulunulanın genişliğini paylaşır) / sil (genişlik komşuya), Tab/Shift+Tab hücre
    gezinme (son hücrede yeni satır), columnSpans tabloda ve satırda güncellenir. Sayfa bölme tabloyu satır
    aralarından böler. Biçimsiz metin ekranda hvl-default yazı tipiyle. Node testi 32/32 (tablolu belge gidiş-dönüş
    bayt bayt aynı).
  - [ ] Yeni tabloyu UYAP'ın açması (bizim yazdığımız tablo/satır/hücre özellikleri az: columnSpans, border).
  - [ ] Hücre birleştirme, hücre içinde iç içe tablo ekleme komutu, sütun genişliğini sürükleyerek değiştirme.
  - [ ] Üst/alt bilgi düzenleme.
- [~] 3. aşama: DOCX (kullanıcı 23.11: "benim için en önemlisi bu"; tablo/üst-alt bilgiden önce alındı).
  - [x] 23.11: DOCX okuma (`belge_cekirdek/src/docx.rs`): Word stilleri (docDefaults → paragraf stili zinciri →
    karakter stili → doğrudan), tema yazı tipleri (minor/major), numaralandırma (madde/numara, %1. / %1) / %1-,
    harf/Roma türleri, seviye girintisi), hizalama, girinti (asılı dahil), boşluklar, satır aralığı (auto/exact),
    sekme durakları (sol/orta/sağ/ondalık), tablolar (ızgara oranları, gridSpan → satır oranı, kenarlık doğrudan
    ya da tablo stilinden, satır yüksekliği, tablo girintisi), resimler (satır içi/kayan, EMU → pt), üst/alt bilgi
    (kendi ilişkileriyle), alt bilgide "Sayfa X / Y" → UDF sayfa numarası özellikleri, kâğıt/kenar boşlukları.
    Uyarılar: dipnot, yorum, izlenen değişiklik, metin kutusu, eski çizim/OLE, sayfa sonu, Shift+Enter (ayrı
    paragraf), dikey birleşik hücre, ilk sayfaya özel üst/alt bilgi, çok sütun, A4 dışı kâğıt, üst/alt simge vb.
    Tür içerikten tanınır (`belge_ac`; .doc için açıklayıcı hata). Editör: "Aç" DOCX alır, UDF olarak kaydeder.
    Testler: çekirdek 3 (örnek DOCX) + arayüz 4.
  - [x] 23.11: Gerçek UDF'lerden 10 tanesi (kullanıcının klasörü, yerelde): UYAP web'in `<webID>` öğesi ham
    korunuyor (`Belge::ek_ogeler`); 10/10 bayt bayt aynı.
  - [ ] Gerçek DOCX örnekleriyle deneme (kullanıcıdan).
  - [x] 23.11: DOCX yazma (`docx_yaz.rs`): her paragraf/parça çözülmüş etkin biçimiyle açık Word biçimi (stil.rs
    = editör = PDF), Java mantıksal yazı tipleri gerçek adlarla, sekme durakları, listeler (numbering.xml, tür ve
    önek), tablolar (birleşik satırların sınırlarından ortak ızgara + gridSpan, kenarlık türü, satır yüksekliği,
    hücre boşluğu), resimler (png/jpeg/gif), üst/alt bilgi, UDF sayfa numarası → PAGE/NUMPAGES alanı, sayfa ve
    kenar boşlukları, varsayılan yazı tipi hvl-default'tan. Paragraf içi satır sonu ↔ `<w:br/>` (okuyucu artık
    Shift+Enter'ı paragraf bölmeden "\n" olarak tutuyor). Word'ün "hücre/belge paragrafla biter" kuralı için
    eklenen boş `<w:p/>` okurken atılıyor. Belirlenimci çıktı.
    Testler: örnek UDF → DOCX → geri ve örnek DOCX → DOCX → geri aynı görünür (metin, hiza, girinti, satır aralığı,
    liste, kalın/italik/altı çizili/renk/boyut/yazı tipi, tablo yapısı, sayfa numarası); 10 gerçek UDF → DOCX →
    geri aynı görünür. Editör: Kaydet açılan biçimde, "Farklı kaydet" UDF / DOCX.
  - [ ] Word'ün bizim yazdığımız DOCX'i açması (kullanıcı deneyecek).
  - Bilinen: UYAP alanları (sayfa numarası dışı) Word'e düz metin gider.
  - [ ] Harf/Roma numaralı listelerin önekleri (stil.rs şimdilik hep rakam gösteriyor).
  - [x] 24.11: Tauri 2 kabuğu (`masaustu/`): dosya aç/kaydet pencereleri (Kaydet aynı biçimde doğrudan yola,
    atomik yazım), komut satırından dosya (çift tık), imza hizmeti köprüsü (`imza_durum`, `imzala` →
    /run/armonia-imza.sock), imzalı UDF'in yerelde doğrulanması (`imza_dogrula`). Arayüze yalnız `core:default`
    izni (dosya sistemi / kabuk / ağ izni yok); CSP'de yalnız Tauri iç iletişimi. Arayüz Tauri'yi algılar, yoksa
    tarayıcı yoluna döner. "İmzala": sertifika seçimi + PIN penceresi; imzalı UDF kaydedilir ve salt okunur açılır.
    İmzalı belge açılınca bildirim (imzalayan, zaman, geçerli / belge değişmiş / imza geçersiz; zincir denetlenmedi
    notu) + "Düzenlemeyi aç". Windows'ta (GNU araç zinciri) derlenip denendi: test anahtarıyla imzalı belge
    komut satırından açıldı, doğrulama bildirimi ve salt okunur görünüm doğru.
  - [x] 24.11: Armonia katmanı `armonia-linux/yazi-katmani/` + `yazi_hazirla.sh` (ISO ve Pi, YAZI=1 varsayılan;
    ikililer `armonia-yazi/derle.sh` çıktısından, ~/armonia-cikti/yazi). Editör: soketle açılan
    `armonia-yazi@.service` (her pencere bir örnek; PrivateNetwork, ev dizininde yalnız Belgeler + İndirilenler +
    veri klasörü, D-Bus / USB diskler / 5975 / systemd --user yok), `yazi_ac.sh` (oturum) + `yazi_calistir.sh` (kum
    havuzu içi) yolu iki kez denetler. İmza hizmeti: `armonia-imza.socket` (/run/armonia-imza/imza.sock, root:armonia
    0660) + `armonia-imza.service` (kullanıcı armonia-imza, yalnız localhost). nftables: oturum_yerel'de 5975 kapalı
    (`fib daddr type local tcp dport 5975 reject`); tarayıcı ve Çoklu UYAP 5975'e kendi zincirlerinden gitmeye devam
    eder, imza soketi dizinini görmez. UDF MIME türü (application/x-uyap-udf), mimeapps (.udf/.docx → Armonia Yazı),
    dock "Yazı" → Armonia Yazı (LibreOffice Writer menüde kalır).
  - [x] 24.11 güvenlik-denetci: DÜZELTMEYLE KULLANILABİLİR (kritik yok). Düzeltilenler: O1 (WebKitGTK 4.1'de web
    içeriği bubblewrap kum havuzu varsayılan KAPALI, Tauri açmıyor → birim ad alanı/mount izni vermiyor,
    @system-service; BİLİNEN EKSİK olarak birimde yazılı), O2 (arayüz yol göndermez; kabuk seçilen dosyaları kimlikle
    tutar, geçici dosya O_EXCL), O3 (günlük gücü/kalıcılığı birimde dürüstçe yazıldı), O4 (/run/dbus, /media gizli),
    O5 (günlükte çağıranın systemd birimi, ProtectProc=default), D1 (soket kendi dizininde), D2 (fib kuralı),
    D3 (isteğin tamamına 30 sn, artan PIN beklemesi, MemoryMax), D5, D6, D8, D9 (WebKit yalnız YAZI=1), D10.
    Açık: D4 (imzalanan içerik ekrandakine bağlı değil; günlükte özet var), D7 (pencere /tmp'ye kayda izin veriyor).
    İmza hizmetinin Linux kısmı Windows'ta Linux hedefiyle tür denetiminden ve clippy'den geçti; Tauri'nin Linux
    kısmı burada denetlenemedi (GTK gerekir).
  - [ ] Linux'ta ilk deneme (DOĞRULANMADI): derle.sh; WebKitGTK @system-service ile çalışıyor mu; dosya penceresi;
    `systemd-analyze verify/security`; `nsenter -t <pid> -m ls -A /home/armonia /run/dbus /media`; tarayıcı
    diliminde /run/armonia-imza görünmemeli; oturumdan `nc -vz 127.0.0.1 5975` reddedilmeli; gerçek kartla imza.
  - [ ] WebKit web içeriği kum havuzunu açmak (O1 (b)): WebContext sandbox + /proc bağlama uyumu.
  - [x] İmza zamanı Türkiye saatiyle gösteriliyor.
