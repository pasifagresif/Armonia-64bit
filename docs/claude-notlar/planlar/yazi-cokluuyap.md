# Armonia Yazı — ÇokluUYAP içinde (plan ve durum)

Kullanıcı isteği (23.11.2026): editör ÇokluUYAP web arayüzünde **açılır pencerede** (tam sayfa değil); UYAP'ta açık
belge okunurken yanında yazılsın, **Ctrl+1…0 çoklu yapıştırma** editörde de çalışsın. Sonra: dosyadan **şablon
dilekçe** (ör. itiraz varsa arabuluculuk başvurusu, itirazın iptali, memur muamelesini şikâyet), kayıt **büro
veritabanına**, cokluuyap.com'dan bağlanan kullanıcı dosya taşımadan açsın / düzenlesin / kaydetsin, belge
**İş Listesi'ne** bağlansın.

Editör ayrı depoda geliştiriliyor (`C:\dev\Armonia-64bit\armonia-yazi`, uzak: pasifagresif/Armonia-64bit). Bu depoda
yalnız derlenmiş çıktısı tutulur: `ruyap/uyap_core/yazi` (`araclar/yazi_al.py`). Editörün son hâli beklenmez.

## 1. aşama — açılır pencere (23.11.2026, yapıldı)
- `src/yazi_sunucu.rs`: `/__cu__/yazi/…` (uyap_proxy + office_agent/tünel). Yol süzgeci, CSP `frame-ancestors 'self'`.
- `enjekte/yazi.js`: modal olmayan pencere (sağ yarı "yan yana" / serbest, taşınır, boyutlanır; telefonda tam ekran),
  sol menü "Armonia Yazı", görev çubuğu girdisi ('yazi'), kapatırken kaydedilmemiş değişiklik sorusu.
  Aynı köken köprüsü (editörde değişiklik gerekmez): tema değişkenleri, ilk ölçek "sayfa genişliği",
  Ctrl+1…0 (yazar gibi ekler) / Ctrl+Shift+1…0 (`window.__uyapCokluPano`), belge verme `__cuYaziDosyaAc(File)`.
- `paylas.js`: UYAP belge çubuğunda UDF için "Yazı'da aç".
- E-imza (24.11): editörün "İmzala" düğmesi yazi.js'te yakalanır → editörün Kaydet çıktısı (çerçevedeki indirme
  bağlantısı yakalanır) → `/__uyap_agent__/sign_udf` (ofis kartı, girişteki PIN) → imzalı UDF indirilir ve editörde
  salt okunur açılır. Kullanıcı kararı: YALNIZ ofis bilgisayarında (localhost); uzaktan bağlananda kapalı.
- Paketler: `yayin_hazirla.py` ve `derle_uyap.sh` `uyap_core/yazi`'yi pakete koyar.

## 1b. aşama — kaydetme, bağlı dosya, dosyaya gönder (24.11.2026, yapıldı; gerçek gönderim denenmedi)
- Başlıkta "Kaydet ▾": İmzalı UDF (varsa) / UDF / Word (editörün "Farklı kaydet" listesi) / PDF (`udf_pdf`).
  İmzalayınca indirme YOK (kullanıcı isteği); imzalı dosya yazi.js'te tutulur, başka belge açılınca ya da
  "Düzenlemeyi aç"ta bırakılır.
- UYAP dosya kartı başlığında "Editörde yaz" → editör o dosyaya bağlı (başlıkta "Dosya: …"); "Yazı'da aç" da bağlar.
- "Dosyaya gönder" (bağlı + imzalı): UYAP'ın Evrak Gönderme sekmesiyle aynı istekler —
  `dosya_gonderilecek_evrak_listesi_brd.ajx` (türler), `odeme_alinacak_evrak_listesi.ajx` (masraflı türler:
  gönderilmez), `evrakGonder_brd.ajx` (file_1 + items + dosyaId, isDusurme, evrakTipi, dosyaNo, birimId, birimAdi).
  Kart verisinde evrakTipi yok; UYAP da "undefined" yolluyor (canlı fiber 24.11). İki adımlı onay; yalnız localhost.

## 2. aşama — belge deposu + İş Listesi + onay (24.11.2026 planı, onay bekliyor)
Kullanıcı isteği: "Kaydet" yalnız bilgisayara değil büro veritabanına; dosyasıyla bağlı (yoksa büro veritabanındaki
dosyalardan seçilir); dosyaya ve İş Listesi işlerine belge olarak eklensin, onaya sunulsun. Kararlar (24.11):
dosya seçimi BÜRO VERİTABANI; onay İŞ LİSTESİ ÜZERİNDEN; yetki OLUŞTURAN + bağlı işlerin kişileri (+ yöneticiler).
- B1 veri: `0018_belge.sql` — `belge` (ad, dosya_id → dosya, künye metinleri, olusturan, son_surum, durum),
  `belge_surum` (her kayıt; sha256, boyut, imzali, kaydeden, zaman), `gorev_belge` (iş ↔ belge). Baytlar ofis
  diskinde içerik adresli (ekler deposu). `belge_veri.rs` (saf yetki kuralı + sorgular), `belge_api.rs`
  `/__cu__/veri/belge/…` (liste, getir, indir, kaydet [iyimser kilit: sürüm no], dosya_ara, ise_ekle/cikar,
  onaya_sun). kimlik.rs ile kişi; tünel ve yerel ağ açık. Uçtan uca postgres testi.
  **B1 YAPILDI (24.11):** `src/sema/0018_belge.sql`, `src/belge_veri.rs` (+ senaryo testi, uçtan uca geçti),
  uçlar `is_api.rs` içinde `/__cu__/veri/is/belge_*` + `dosya_ara` (kimlik, tünel, LAN İş Listesi'yle aynı).
  `is_veri::gorev_ayrinti` → `belgeler`; `ek_sil` belge sürümünün dosyasını silmez. Canlı programda denenmedi.
- B2 editör: başlıkta "Kaydet" = veritabanı (ilk kayıtta dosya yoksa arama penceresi); "Kaydet ▾" indirme
  seçenekleri kalır; "Aç ▾ → Büro belgeleri"; başlıkta "Kayıtlı · sürüm N"; çakışmada uyarı; imzalı sürüm ve UYAP'a
  gönderim belgeye işlenir.
  **B2 YAPILDI (24.11, sahte sunucuyla Playwright; canlı denenmedi):** yazi.js — başlıkta "Kaydet" (+ editörün
  Kaydet'i ve Ctrl+S) büroya; ilk kayıtta ad + dosya (bağlı dosya / büro dosya seçici `dosya_ara`); "▾" menü: Büro
  belgelerini aç (bağlı dosya / Belgelerim + arama), ayrı belge olarak kaydet, indirmeler; başlıkta "Büroda · sürüm N";
  409 CAKISMA penceresi; imzalanınca imzalı sürüm büroya; gönderim `belge_gonderildi`. Büro dosyasına bağlı (kartsız)
  belgede "Dosyaya gönder" kapalı (UYAP dosyaId yok). `window.__cuYaziBelgeAc(id)` (B3 ekranları için).
- B3 ekranlar: icra.js dosya panelinde "Belgeler"; isler.js iş ayrıntısında "Belgeler" (aç / ekle / yeni yaz);
  "Onaya sun" (iş yoksa "Belge onayı: <ad>" işi açılır, onaylayıcı seçilir); onaylayan belgeyi açar,
  "Onayla" / "Düzeltme iste (not)" (mevcut durum akışı: onayda → tamamlandi / acik).
  **B3 YAZILDI (24.11, canlı deneme bekliyor):** isler.js iş ayrıntısında "Belgeler" (aç / işten çıkar / yeni belge
  yaz [ilk kayıtta işe eklenir] / kayıtlı belge ekle); icra.js dosya panelinde "Belgeler" sekmesi (liste, aç, yeni
  belge yaz); yazi.js ▾ menüde "Onaya sun…" (yeni onay işi + onaylayıcı ya da var olan iş) ve "İşe ekle…";
  `window.__cuYaziYeni({buroDosyaId|kart, birimAdi, dosyaNo, gorevId})`. Onay/geri çevirme mevcut iş akışı.
  **B3 canlı denendi (24.11):** icra Belgeler sekmesi + Aç (dosyaya bağlı açılır), editör "İşe ekle…" (iş 320),
  iş ayrıntısında Belgeler (1) / ✕ işten çıkar / kayıtlı belge ekle listesi / yeni belge yaz — hepsi çalıştı.
  Onaya sun canlı denenmedi (gerçek iş açar, kişiye bildirim gider). Tuzak: Claude eklentisi yalnız kendi sekme
  grubunu görür; çerez "Tarayıcıda Aç"la gelir, kullanıcı çalışan sekmeyi eklentinin grubuna sürüklemeli.
- B4 uzaktan: cokluuyap.com tüneliyle aç / düzenle / kaydet (imza ve gönderim yalnız ofiste); canlı deneme.
  **B4 incelemesi (24.11):** ek kod gerekmedi — office_agent tüneli `/__cu__/yazi/…`, `/__cu__/veri/is/…`
  (belge_kaydet / belge_indir ham uçları dahil, kimlik tünel biletinden) ve udf_pdf / udf_doldur'u zaten
  karşılıyor; sw.js yanıt başlıklarını (X-Belge-Surum / X-Belge-Imzali) olduğu gibi geçiriyor; editör varlıkları
  karma adlı (önbellek güvenli). Değişiklik: uzaktan bağlananda "Dosyaya gönder" düğmesi gizli (imza zaten
  uyarıyla kapalı). Not: sw.js STATIC_EXT `.wasm` içermiyor → 718 KB editör wasm'ı her açılışta tünelden gelir
  (sunucu yayını gerektirir, kullanıcı onayıyla).
  **B4 canlı denendi (24.11, cokluuyap.com, P2P):** büro belgesi uzaktan 1 sn'de açıldı (dosya bağıyla), düzenlendi,
  Ctrl+S → ofiste sürüm 3 (kaydeden utku); "Dosyaya gönder" gizli; İmzala → "yalnız ofis" uyarısı. HATA bulundu:
  office_agent'ta `path` baştaki "/" atılmış olduğu hâlde `"/__uyap_yerel__/udf_pdf"` / `udf_doldur` ile
  karşılaştırılıyordu → uzaktan PDF (ve telefon "Paylaş" PDF'i, UDF İndir alan doldurma) 404 dönüyordu. Düzeltildi,
  derlendi, kuruldu (15:16 exe); uzaktan udf_pdf → 200 application/pdf, udf_doldur → 204 (alansız belge). **B4 TAMAM.**

- Onay hatırlatması (24.11, kullanıcı isteği): iş ayrıntısında onayda iş için (onaylayamayan sorumlu / sunan)
  "Hatırlatma gönder" → `POST is/onay_hatirlat {gorev, notu}` → `is_veri::onay_hatirlat`: alıcı onaylayıcılar (yoksa
  işi açan, o da yoksa yöneticiler), `hatirlatici` satırı zaman=şimdi (sayfa uyarısı + telefon sinyali mevcut yoldan),
  aynı kişiye saatte bir, geçmişe `hatirlatildi` olayı. Uçtan uca test geçti; canlı denenmedi.
  Canlıda bulundu: sunan YÖNETİCİ ise `onaylayabilir` olduğundan düğme çıkmıyordu (utku → iş 327 savas, 328 sevval).
  Düzeltme: başkası onaylayıcıysa ve ben onaylayıcı değilsem, yöneticide de "Hatırlatma gönder" (+ Onayla kalır).
- Belge silme (24.11, kullanıcı: "her taslak şişme yapacak"): `belge_veri::sil` artık KALICI — belge + sürümler +
  iş bağları (CASCADE), başka yerde kullanılmayan dosyalar diskten; bağlı işlere olay. Oluşturan ya da yönetici;
  UYAP'a gönderilmişi yalnız yönetici. Arayüz: editör ▾ "Belgeyi sil…" (onay penceresi), icra Belgeler "Sil" (iki tık).
- Editörün "Aç"ı (24.11): doğrudan dosya seçici yerine menü — "Büro belgelerini aç…" / "Bilgisayarımdan aç…"
  (seçici üst belgede açılır: çerçevedeki gizli input'u üstten kod açamaz; dosya `dosyaVer` ile editöre). Playwright ile denendi.
- Kişi adları: görünen ad hesabı içermiyorsa listede hesap adı eklenir (zeynel@savas görünen adı "savas" geliyordu).

## Büro Belgeleri sayfası + klasör düzeni (24.11.2026; K1–K3 yapıldı, canlı deneme bekliyor)
Durum (24.11 gece): `src/belge_disk.rs` (yol üretimi/denetimi, ad temizliği + aygıt adları, "(2)", atomik yazma,
`GeriAl` = veritabanı commit'lenmezse disk işlerini geri sarar, boş klasör toplama, Windows'ta `.surumler` gizli),
şema 0019 `belge.yol` (+ lower(yol) tekil indeks), `belge_veri.rs`: kaydet/oku/dosya_bagla (taşır)/yeniden_adlandir/
sil diskle birlikte, `agac`, liste `klasor` + `tumu`; açılışta `eski_belgeleri_yerlestir` (ekler'den kopyalar, sonra
eski kopyayı siler). Uçlar: `belge_agac`, `belge_adlandir`, `belge_liste ?klasor=&tumu=`. Sayfa `enjekte/belgeler.js`
(sol menü İş Listesi'nin altında, görev çubuğu 📁, `cu-belge-degisti` olayıyla tazelenir; yazi.js kayıt/silmede
yayar). Testler: uçtan uca postgres+disk (klasör, sürüm, "(2)", ağaç, süzgeç, adlandırma, taşıma, boş klasör silme,
eski depo taşıma) geçti; Playwright (geniş + telefon, sahte sunucu) geçti. Linux: kök zaten kasada, değişiklik
gerekmedi (Linux tarafına bu oturumdan dokunulmaz). Dosya klasörü adı "2026-38696" (diskte "/" olmaz), arayüz
"2026/38696" gösterir. Büyük/küçük harf: PostgreSQL C yerelinde lower() yalnız ASCII → "İ"/"i" farkı ayrı ad sayılır
(NTFS de öyle); çakışma denetimi ikisinde tutarlı.
Kullanıcı isteği: İş Listesi / İcra Takip Süreçleri gibi bir "Büro Belgeleri" sayfası; belgeler yargı türü → birim
(mahkeme/daire) → dosya klasörlerinde. Klasörler toptan açılmaz, belge kaydedildikçe oluşur. Linux'ta LUKS2 kasası da
bu düzende. İleride: sunucu UYAP dosyalarındaki belgeleri indirir; dosya penceresinde tıklanan belge büroda varsa
veritabanından gelir (DİKKAT: "UYAP'tan robotla veri çekme yok" ilkesiyle çelişir → o aşamada açık karar gerekir).
- K1 disk düzeni: `<kök>/Belgeler/<Yargı türü>/<Birim adı>/<yıl-sıra>/<belge adı>.udf` (son sürüm) +
  `…/<yıl-sıra>/.surumler/<belge adı>/<N>.udf`; dosyasız: `Belgeler/Dosyasız/<kullanıcı>/`. Ad temizliği (Windows'ta
  yasak karakterler, uzunluk), çakışmada "(2)". `belge.yol` sütunu (0019). Dosya bağı / ad değişince taşınır, silinince
  gider. Mevcut içerik adresli (ekler) sürümler bir kez taşınır. Kök: `kok_dizin()` → Linux'ta zaten kasadaki
  `/var/lib/armonia/CokluUyap` (kasa_bagla.sh'a `Belgeler` 0700 kiosk eklenir).
- K2 uçlar: `belge_agac` (yargı türü/birim/dosya + sayılar, yalnız görebildikleri), `belge_liste` klasör süzgeci,
  `belge_tasi` (dosya değiştir), ad değiştir.
- K3 sayfa `enjekte/belgeler.js`: sol menü + görev çubuğu + sayfa katmanı (İş Listesi kalıbı); solda klasör ağacı,
  sağda liste (ad, sürüm, tarih, oluşturan, e-imza, işler, gönderim), arama, Belgelerim / Tümü; eylemler Aç, İndir,
  Taşı, Yeniden adlandır, Sil; telefonda tek sütun. Uzaktan (tünel) aynı.
- K4 test: uçtan uca postgres + disk (taşıma, çakışma, silme), Playwright, canlı; Linux derlemesi.

## 3–5. aşamalar (bekliyor)
Şablonlar (`{{alan}}` yer tutucular, öneri kuralları; metinleri büro verir), icra dosya kartında "Dilekçe hazırla",
İş Listesi bağlantısı (`gorev_ek` / bağ tablosu), uzaktan kullanım + güvenlik denetimi.

Açık kararlar: editör kaynağının yeri (ayrı depo / birleşme), belge yetkisi (bütün büro / oluşturan + atananlar),
şablonları kim düzenler.

## Editör oturumuna iletilecekler
1. **Köprü (2. aşama için):** gömülü çerçevede `window.ArmoniaYazi = { ac(bayt: Uint8Array, ad: string),
   al(): Uint8Array /* güncel UDF */, degisti(): boolean, onKaydet?: (bayt, ad) => void }`. `onKaydet` verilmişse
   "Kaydet" (Ctrl+S) indirmek yerine bunu çağırsın. (Şimdilik ÇokluUYAP `#durum`'daki "•" ve `document.title`'ı okuyor.)
2. **Kusur:** paragraf sonunda Enter'dan sonra yazılan metin biçimi (yazı tipi/boyut) kaybediyor; yeni paragraf
   varsayılan stile (DejaVu 14px) düşüyor. Yapıştırılan düz metin de çevre biçimini almıyor.
3. **Kusur:** `body.kompakt` araç çubuğunda "Kaydet" yazısı geri al düğmesinin üstüne taşıyor.
5. **Kusur (24.11):** portal belgelerinde alanlar metinde ADIYLA durur, değerler `template/data`'da; editör
   `<data>`'yı okumuyor → alan adları görünüyor. ÇokluUYAP'ta çözüm `ruyap/src/udf_alan.rs` (`icerik_doldur`):
   çekirdeğe alınabilir. Tuzak: aynı ad `<data>`'da iki kez geçebilir (ilk DOLU değer alınmalı).
6. **İmza incelemesi (24.11):** doğrulama zincir/Kamu SM denetlemiyor ama "geçerli" diyor; `dosya_kaydet` `yol`
   ile pencere açmadan yazıyor; `udf_imza.rs` imza zamanını bayt konumuyla bölüyor (çok baytlı karakterde çöker);
   seçilen sertifika yoksa ilk sertifikaya düşüyor; PKCS#11 oturumu hata yolunda kapanmıyor.
4. Bilgi: ölçek tercihi (`armonia-yazi-olcek`) ve ÇokluUYAP aynı kökende; ÇokluUYAP ilk açılışta `genislik` yazar.
   Tema için `:root` değişkenleri (`--zemin --cubuk --kenar --yazi --vurgu --vurgu-koyu --vurgu-zemin`) satır içi
   ezilir; adları değişirse ÇokluUYAP'a haber verilmeli.
7. **Düzeltildi (24.11 akşam):** Enter yeni paragrafı varsayılan özelliklerle açıyordu → sekme durakları (TabSet),
   hiza, girinti gidiyor, Tab 72 pt'ye düşüyor, UDF'e `Alignment="0"` yazılıyordu. `main.ts` `paragrafBol`
   (splitBlockAs + ensureMarks) editör deposuna uygulandı (commit'lenmedi), ruyap'a `yazi_al.py` ile alındı.
8. **Ortak sekme hizası (24.11 akşam):** boş belgeye varsayılan durak KONMADI: yerel 754 UDF'te paragraf başı Tab
   (UYAP'ta 72 pt girinti) 442 belgede var, tek 113,4 pt durak onları kaydırırdı. Bunun yerine araç çubuğunda "⇥:"
   (`komutlar.ts` `sekmeleriHizala`): imlecin Tab'lı satır bloğu ya da seçim → en uzun etiket + 6 pt, tam sayı;
   paragrafa UYAP cetvelinin yazdığı biçimde `TabSet="N.0:0:0"`; etiketten sonraki fazla art arda sekmeler silinir;
   paragraf başı sekmesi olan satırlara dokunulmaz. Playwright: hizalama, kaydet, yeniden aç aynı.
