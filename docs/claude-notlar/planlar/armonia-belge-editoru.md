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
  - [ ] Gerçek UDF örnekleriyle deneme (kullanıcıdan) ve **UYAP editörünün bizim yazdığımız UDF'i açması**
    (en önemli doğrulama; bizim yazıcı üst/alt bilgiyi başa koyuyor, konumları baştan sayıyor).
  - [ ] Stil zinciri çözücü (paragraf/parça → etkin yazı tipi, boyut, kalın…; `udf_pdf.rs` mantığı).
  - [ ] `udf_pdf.rs`'nin çekirdeğe taşınması (PDF çıktısı) — ruyap'taki güncelleme işi bitince.
  - [ ] WebAssembly derleme denemesi (`rustup target add wasm32-unknown-unknown`).
