//! Kendi ekranlarımız için büro veritabanı uçları (Aşama 7, veri katmanı).
//!
//! `/__cu__/veri/…` yolları bütün büro kullanıcılarına açıktır (bu bilgisayar, ofis ağı ve tünel);
//! veritabanı ofis bilgisayarındadır. Yalnız büro veritabanını okur / yazar; UYAP'a istek atmaz. Ekranlar (borçlu listesi, dosya penceresi, geçmiş, ayarlar) bu
//! uçlardan beslenir. Yanıt biçimi: `{"ok": true, "veri": …}` ya da `{"ok": false, "error": "…"}`.
//!
//!   GET  ozet                 sayılar ve takip aşamalarına göre dağılım
//!   GET  borclular            ?q=&asama=&durum=acik|kapali&birim=&alacakli=&borclu=&yil=&dosya_tur=&acilis_bas=&acilis_son=
//!                             &tur=&teblig=&mazbata=&kesinlesme=&sayfa=1&boyut=50 — borçlu başına satır (borçlusuz dosya
//!                             tek satır) + borçlunun bütün tebligatları + süzgeç seçenekleri (adetli)
//!   GET  dosya                ?id= — künye, takip türü, taraflar, tebligatlar, uygulanan süre
//!   GET  gecmis               ?dosya= — durum olayları (yeniden eskiye)
//!   GET  ptt                  ?tebligat= — PTT hareketleri
//!   GET  kurallar             kesinleşme süreleri (adlarıyla) + takip türü ağacı (UYAP listesi + dosyalarda görülenler,
//!                             dosya sayılarıyla) + veritabanındaki takip türü dağılımı
//!   POST kural / kural_sil    süre ekle, düzenle, sil (ardından bütün borçlular yeniden hesaplanır)
//!   POST kural_varsayilan     hazır kuralı varsayılan değerlerine döndür (hazır kurallar silinmez)
//!   GET  tatiller · POST tatil / tatil_sil
//!   POST itiraz               {bag, edildi, tarih} — elle itiraz
//!   POST kesinlesme_elle      {bag, durum} — "", konkordato, vefat, vasi, tehiri_icra
//!   POST islem_durumu         {dosya, durum, notu, bekleme_bitis}
//!   POST hesapla              {dosya?} — kesinleşme / tebliğ / takip aşamasını yeniden hesapla

use crate::icra_durum as durum;
use crate::icra_tebligatlar as teb_liste;
use crate::icra_dosyalar as dosya_liste;
use crate::icra_kesinlestirme as kesin;
use crate::icra_tebligat::{self as teb, tr_kucuk};
use crate::icra_veri::{
    self as veri, durum_olayi_yaz, IslemDurumu, KesinlesmeDurumu, OlayKaynak, OlayTur, Rol, TakipAsamasi, YeniOlay,
};
use chrono::NaiveDate;
use postgres::GenericClient;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const ON_EK: &str = "__cu__/veri/";

pub fn is_path(path: &str) -> bool {
    path.trim_start_matches('/').starts_with(ON_EK)
}

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

fn gun(t: Option<NaiveDate>) -> Value {
    t.map(|t| json!(t.format("%Y-%m-%d").to_string())).unwrap_or(Value::Null)
}

fn gun_coz(v: &Value) -> Result<Option<NaiveDate>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) => veri::tarih::gun_coz(s).map(Some).ok_or_else(|| format!("Tarih anlaşılamadı: {s}")),
        _ => Err("Tarih metin olmalı.".into()),
    }
}

/// Tarafın görünen adı (SQL): tüzelde unvan, gerçekte ad soyad; yoksa kimlik.
pub(crate) fn ad_sql(t: &str) -> String {
    format!(
        "CASE WHEN {t}.tur = 'tuzel'
              THEN COALESCE(NULLIF(btrim({t}.unvan), ''), {t}.mersis_no, NULLIF({t}.vergi_no, ''), 'Taraf #' || {t}.id)
              ELSE COALESCE(NULLIF(btrim({t}.ad || ' ' || {t}.soyad), ''), {t}.tckn, 'Taraf #' || {t}.id) END"
    )
}

/// UYAP'ın dosya kartını açmak için gereken künye (web arayüzü dosyayı UYAP'ta güncel kimliğiyle yeniden
/// bulup kartı açar; saklanan oturumluk `dosyaId` kullanılmaz).
#[allow(clippy::too_many_arguments)]
fn uyap_karti(birim_id: &str, birim_adi: &str, dosya_no: &str, tur_kod: i32, tur: &str, turu3: &str, durum: &str, durum_kod: i32) -> Value {
    json!({"birimId": birim_id, "birimAdi": birim_adi, "dosyaNo": dosya_no, "dosyaTurKod": tur_kod,
           "dosyaTurKodAciklama": tur, "birimTuru3": turu3, "dosyaDurum": durum, "dosyaDurumKod": durum_kod})
}

// ── Okuma ───────────────────────────────────────────────────────────────────────────────────────

/// Sayılar ve icra borçlularının takip aşamalarına göre dağılımı (aşama sırasıyla).
pub fn ozet<C: GenericClient>(c: &mut C) -> Result<Value, String> {
    let s = c
        .query_one(
            "SELECT (SELECT count(*) FROM dosya), (SELECT count(*) FROM dosya WHERE yargi_turu = 2),
                    (SELECT count(*) FROM dosya WHERE yargi_turu = 2 AND durum_kod = 0),
                    (SELECT count(*) FROM tebligat_barkod),
                    (SELECT to_char(max(son_senkron), 'YYYY-MM-DD HH24:MI') FROM dosya)",
            &[],
        )
        .map_err(hata)?;
    let mut sayilar: HashMap<String, i64> = HashMap::new();
    for r in c
        .query(
            "SELECT dt.takip_asamasi, count(*) FROM dosya_taraf dt JOIN dosya d ON d.id = dt.dosya_id
              WHERE dt.rol = 'borclu' AND d.yargi_turu = 2 GROUP BY dt.takip_asamasi",
            &[],
        )
        .map_err(hata)?
    {
        sayilar.insert(r.get(0), r.get(1));
    }
    let mut asamalar: Vec<Value> = TakipAsamasi::HEPSI
        .iter()
        .map(|a| json!({"kod": a.kod(), "etiket": a.etiket(), "adet": sayilar.get(a.kod()).copied().unwrap_or(0)}))
        .collect();
    if let Some(n) = sayilar.get("").filter(|n| **n > 0) {
        asamalar.push(json!({"kod": "", "etiket": "Henüz hesaplanmadı", "adet": n}));
    }
    // İcra daireleri (UYAP'tan güncelleme kapsamı için UYAP birim kimliğiyle).
    let birimler: Vec<Value> = c
        .query(
            "SELECT b.ad, b.birim_id, count(*) FILTER (WHERE d.durum_kod = 0) FROM dosya d JOIN birim b ON b.id = d.birim_id
              WHERE d.yargi_turu = 2 GROUP BY b.ad, b.birim_id ORDER BY b.ad",
            &[],
        )
        .map_err(hata)?
        .iter()
        .map(|r| json!({"ad": r.get::<_, String>(0), "birim_id": r.get::<_, String>(1), "acik": r.get::<_, i64>(2)}))
        .collect();
    Ok(json!({
        "birimler": birimler,
        "dosya": s.get::<_, i64>(0), "icra_dosya": s.get::<_, i64>(1), "acik_icra_dosya": s.get::<_, i64>(2),
        "tebligat": s.get::<_, i64>(3), "son_guncelleme": s.get::<_, Option<String>>(4),
        "borclu": sayilar.values().sum::<i64>(), "asamalar": asamalar,
    }))
}

#[derive(Debug, Default, Clone)]
pub struct Filtre {
    /// Dosya no, birim, borçlu ya da alacaklı adında aranır (Türkçe-duyarsız).
    pub q: String,
    /// Takip aşaması kodu; boş = hepsi.
    pub asama: String,
    /// Dosya listesi (`icra_listeler`): yalnız bu listedeki dosyalar; None = hepsi.
    pub liste: Option<i64>,
    /// Dosyanın UYAP durumu: "acik" | "kapali" | "" (hepsi).
    pub durum: String,
    /// İcra dairesi. Düz metin: içinde geçen; `=` ile başlarsa tam eşleşme (listeden seçilen); `!` ile başlarsa
    /// listedekiler HARİÇ hepsi ("Tümünü seç" sonra bazılarını kaldır; `!` tek başına: hepsi).
    pub birim: String,
    /// Dosya numarası; aynı kural.
    pub dosya_no: String,
    /// Dosyanın alacaklılarından biri; aynı kural.
    pub alacakli: String,
    /// Borçlunun adı; aynı kural.
    pub borclu: String,
    /// Dosya yılı (tam değer); "" = hepsi.
    pub yil: String,
    /// Dosya türü (tam değer: İcra Dosyası, MTS, Talimat Dosyası …); "" = hepsi.
    pub dosya_tur: String,
    /// Açılış tarihi aralığı (iki uç dahil); boş uç sınırsız.
    pub acilis_bas: Option<NaiveDate>,
    pub acilis_son: Option<NaiveDate>,
    /// Tebligat türü (borçlunun tebligatlarından biri). Düz metin: içinde geçen; `=` ile başlarsa tam eşleşme.
    pub tur: String,
    /// Tebligatın PTT / tebliğ durumu; aynı kural. `=` tek başına: durumu boş (henüz sorgulanmamış).
    pub teblig: String,
    /// Mazbata dosyaya döndü mü: "var" | "yok" | "" (hepsi).
    pub mazbata: String,
    /// Kesinleşme durumu kodu; "-" = henüz hesaplanmamış; "" = hepsi.
    pub kesinlesme: String,
    pub sayfa: usize,
    pub boyut: usize,
}

/// Listedeki bir tebligatın süzgeçlerde kullanılan alanları.
struct ListeTebligati {
    tur: String,
    ptt: String,
    mazbata: bool,
    veri: Value,
    /// Sabit süreli kesinleşmesi olan türlerde (103, kıymet takdiri) hesap için tebligat bilgisi.
    sabit: Option<(i16, durum::OdemeEmri)>,
}

/// Kesinleşme süzgecinde talimat dosyalarının değeri (gerçek bir kesinleşme durumu değildir).
pub const KES_TALIMAT: &str = "talimat";

/// Listeden seçilen değerler satır sonuyla ayrılır (birden çok seçim: herhangi birine uyan).
pub const AYRAC: char = '\n';

/// "Hariç" süzgeci: `!` + satır sonuyla ayrılmış değerler → (hariç tutulanlar). `!` tek başına hiçbirini hariç
/// tutmaz (hepsi seçili); yalnız boş değeri hariç tutmak için `!\n`.
pub(crate) fn haric_listesi(suzgec: &str) -> Option<Vec<&str>> {
    suzgec.strip_prefix('!').map(|r| if r.is_empty() { Vec::new() } else { r.split(AYRAC).collect() })
}

/// Listeden seçim mi (tam değerler ya da hariç listesi); değilse düz yazı.
fn listeden_mi(suzgec: &str) -> bool {
    suzgec.starts_with('=') || suzgec.starts_with('!')
}

/// Başlık kutusundaki metin süzgeci: boş = hepsi; `=değer` tam eşleşme (listeden seçilen; birden çok değer
/// satır sonuyla ayrılır, herhangi biri yeter); `!değer` listedekiler hariç hepsi; düz metin içinde geçen.
pub(crate) fn metin_uyar(suzgec: &str, deger: &str) -> bool {
    let d = || tr_kucuk(deger.trim());
    if let Some(haric) = haric_listesi(suzgec) {
        let d = d();
        return !haric.iter().any(|h| tr_kucuk(h.trim()) == d);
    }
    match suzgec.strip_prefix('=') {
        Some(tamlar) => {
            let d = d();
            tamlar.split(AYRAC).any(|tam| tr_kucuk(tam.trim()) == d)
        }
        None => suzgec.trim().is_empty() || tr_kucuk(deger).contains(tr_kucuk(suzgec.trim()).as_str()),
    }
}

/// Kodlu (yalnız listeden seçilen) süzgeçlerin ortak kuralı: boş = hepsi; `!…` = listedekiler hariç; yoksa
/// seçilenlerden biri. `uyar`: bir seçimin değere uyup uymadığı.
fn kodlu_uyar(suzgec: &str, uyar: impl Fn(&str) -> bool) -> bool {
    if suzgec.is_empty() {
        return true;
    }
    match haric_listesi(suzgec) {
        Some(haric) => !haric.iter().any(|h| uyar(h)),
        None => suzgec.split(AYRAC).any(uyar),
    }
}

/// Kodlu (yalnız listeden seçilen) süzgeç: boş = hepsi, yoksa seçilen değerlerden biri.
fn kod_uyar(suzgec: &str, deger: &str) -> bool {
    kodlu_uyar(suzgec, |s| s == deger)
}

fn mazbata_uyar(suzgec: &str, var: bool) -> bool {
    kodlu_uyar(suzgec, |s| match s {
        "var" => var,
        "yok" => !var,
        _ => false,
    })
}

fn kesinlesme_uyar(suzgec: &str, kod: &str) -> bool {
    kodlu_uyar(suzgec, |s| match s {
        "-" => kod.is_empty(),
        k => k == kod,
    })
}

/// Seçenek listesi: değere göre adet, çoktan aza (eşitlikte ada göre).
fn secenek_listesi(sayim: HashMap<String, i64>, etiket: &dyn Fn(&str) -> String) -> Vec<Value> {
    secenek_sinirli(sayim, etiket, usize::MAX)
}

/// Uzun olabilecek listeler (borçlu, alacaklı, daire) için en çok `en_cok` seçenek; kalanına yazarak ulaşılır
/// (yazılan metin o kutunun kendi seçeneklerini de daraltır, bkz. [`borclu_listesi`]).
pub const SECENEK_SINIRI: usize = 300;

pub(crate) fn secenek_sinirli(sayim: HashMap<String, i64>, etiket: &dyn Fn(&str) -> String, en_cok: usize) -> Vec<Value> {
    let mut v: Vec<(String, i64)> = sayim.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().take(en_cok).map(|(d, n)| json!({"deger": d, "etiket": etiket(&d), "adet": n})).collect()
}

/// Bir satırın künye süzgeçleri (daire, alacaklı, borçlu, yıl, tür). Seçenek sayımında bir kutunun
/// kendi süzgeci dışarıda tutulur; ancak düz yazıysa (listeden seçilmemişse) uygulanır, böylece yazılan
/// metin o kutunun seçeneklerini daraltır ve sınırın ötesindeki adlar da bulunur.
#[derive(Clone, Copy, PartialEq)]
enum Kunye {
    Birim,
    DosyaNo,
    Alacakli,
    Borclu,
    Yil,
    Tur,
}

/// MTS'den icra dairesine geçmiş dosya: dosyada, dosyanın açılışından ÖNCE düzenlenmiş bir ödeme emri tebligatı var
/// (UYAP, MTS'nin ödeme emrini ve mazbatasını yeni esas dosyasına taşır; 7155 s. K. md. 8/2: yeni ödeme emri yok, süre
/// baştan başlamaz). Veritabanında 5.219 abonelik dosyasının 5.217'sinde görüldü (20.11.2026). Tebliğ imkânsızlığıyla
/// gelen dosyada da MTS ödeme emri vardır; yeni dairenin ödeme emri ayrıca gelir, hesap tebliğ edilen en yenisini alır.
pub const MTS_KAYNAKLI_SQL: &str = "(d.tur <> 'MTS' AND EXISTS (SELECT 1 FROM tebligat_barkod mtb
        WHERE mtb.dosya_id = d.id AND mtb.tebligat_turu = 'Ödeme Emri' AND mtb.evrak_gunu < d.acilis_tarihi::date))";
/// MTS (Merkezi Takip Sistemi) dosyası mı. UYAP tür kodu canlıda doğrulanmadı (kodda 294 ve 36 geçiyor); ad esas alınır.
pub fn mts_mi(_tur_kod: i32, tur: &str) -> bool {
    tur == "MTS" || tr_kucuk(tur).contains("merkezi takip")
}

/// "Dosya türü" süzgecinde MTS'den gelen dosyaların değeri: "İcra Dosyası (MTS'den)".
pub const MTS_KAYNAKLI_EKI: &str = " (MTS'den)";

/// İcra dosyaları borçlu başına satır: dosya, borçlu, alacaklı, takip aşaması, kesinleşme ve tebliğ
/// durumu, borçluya giden BÜTÜN tebligatlar (barkod, tür, PTT durumu, mazbata). Borçlu kaydı olmayan
/// dosya da tek satırla gelir (`bag` boş) — kapalı dosyaların çoğunda yalnız künye kayıtlıdır.
/// Sıra: aşama günü yeniden eskiye, sonra dosya numarası.
///
/// Künye süzgeçleri: daire, alacaklı, borçlu (yazı: içinde geçen; `=`: tam), yıl ve dosya türü (tam),
/// açılış tarihi aralığı, dosya durumu (açık / kapalı / hepsi).
///
/// Tebligat süzgeçleri (tür, tebliğ, mazbata) aynı tebligatta birlikte aranır: "Ödeme Emri + İADE" seçilince
/// iade dönen ödeme emri olan borçlular gelir. Satırdaki tebligatların hepsi gösterilir; süzgece uyanlar
/// `uyan: true` ile işaretlenir.
///
/// `secenekler`: her süzgeç için, DİĞER süzgeçler uygulanmışken kalan değerler ve satır adetleri — kullanıcı
/// boş sonuç verecek bir seçenek görmez.
pub fn borclu_listesi<C: GenericClient>(c: &mut C, f: &Filtre) -> Result<Value, String> {
    borclu_listesi_ic(c, f, None)
}

/// Süzgece uyan BÜTÜN dosyaların kimlikleri (sayfalama yok; listedeki sırayla, tekil). "Tümünü seç" ve
/// seçili dosyalara işlem için; süzgeç mantığı listeyle aynı yerdedir.
pub fn suzulen_dosyalar<C: GenericClient>(c: &mut C, f: &Filtre) -> Result<Vec<i64>, String> {
    let mut idler = Vec::new();
    borclu_listesi_ic(c, f, Some(&mut idler))?;
    Ok(idler)
}

fn borclu_listesi_ic<C: GenericClient>(c: &mut C, f: &Filtre, kimlikler: Option<&mut Vec<i64>>) -> Result<Value, String> {
    let durum_kosulu = match f.durum.as_str() {
        "acik" => "AND d.durum_kod = 0",
        "kapali" => "AND d.durum_kod <> 0",
        _ => "",
    };
    let sql = format!(
        "SELECT dt.id, d.id, b.ad, d.dosya_no, d.durum, d.durum_kod, d.islem_durumu, COALESCE({borclu}, ''),
                COALESCE(dt.takip_asamasi, ''), dt.takip_asamasi_tarihi, COALESCE(dt.kesinlesme_durumu, ''), dt.kesinlesme_tarihi,
                COALESCE(dt.tebligat_durumu, ''), dt.tebligat_durum_tarihi, COALESCE(dt.tebligat_sekli, ''),
                COALESCE(dt.uyapta_gorulmedi IS NOT NULL, false), COALESCE(dt.itiraz_edildi_mi, false),
                ARRAY(SELECT {alacakli} FROM dosya_taraf x JOIN taraf a ON a.id = x.taraf_id
                       WHERE x.dosya_id = d.id AND x.rol = 'alacakli' ORDER BY x.sira, x.id),
                b.birim_id, b.turu3, d.tur_kod, d.tur, d.acilis_tarihi::date, d.yil, {mts}
           FROM dosya d JOIN birim b ON b.id = d.birim_id
           LEFT JOIN dosya_taraf dt ON dt.dosya_id = d.id AND dt.rol = 'borclu'
           LEFT JOIN taraf t ON t.id = dt.taraf_id
          WHERE d.yargi_turu = 2 {durum_kosulu}
            AND ($1::text = '' OR dt.takip_asamasi = $1)
            AND ($2::bigint IS NULL OR d.id IN (SELECT u.dosya_id FROM dosya_listesi_uyesi u WHERE u.liste_id = $2))
          ORDER BY dt.takip_asamasi_tarihi DESC NULLS LAST, d.yil DESC, d.sira_no DESC, dt.sira, dt.id",
        borclu = ad_sql("t"),
        alacakli = ad_sql("a"),
        mts = MTS_KAYNAKLI_SQL,
    );
    if let Some(l) = f.liste {
        crate::icra_listeler::liste_dosyalari(c, l)?; // silinmiş liste: boş ekran değil, uyarı
    }
    let satir_kayitlari = c.query(&sql, &[&f.asama, &f.liste]).map_err(hata)?;

    // Borçluya bağlı tebligatlar (bankalara giden haciz ihbarnameleri gibi borçlusuz kayıtlar dosya panelinde kalır).
    let mut tebligatlar: HashMap<i64, Vec<ListeTebligati>> = HashMap::new();
    for s in c
        .query(
            "SELECT tb.borclu_id, tb.id, tb.tebligat_turu, tb.evrak_tarihi, tb.barkod, tb.elektronik_tebligat, tb.ptt_durumu,
                    tb.son_islem_tarihi, tb.teslim_sekli, tb.tebligat_mazbatasi_var, tb.tebligat_mazbatasi_aciklama,
                    tb.birim_evrak_no, tb.evrak_aciklama, tb.evrak_gunu, tb.son_islem_zamani::date
               FROM tebligat_barkod tb JOIN dosya d ON d.id = tb.dosya_id
              WHERE tb.borclu_id IS NOT NULL AND d.yargi_turu = 2
              ORDER BY tb.evrak_gunu DESC NULLS LAST, tb.birim_evrak_no DESC",
            &[],
        )
        .map_err(hata)?
    {
        let (tur, ptt, mazbata): (String, String, bool) = (s.get(2), s.get(6), s.get(9));
        let veri = json!({
            "id": s.get::<_, i64>(1), "tur": tur, "evrak_tarihi": s.get::<_, String>(3), "barkod": s.get::<_, String>(4),
            "elektronik": s.get::<_, bool>(5), "ptt_durumu": ptt, "son_islem_tarihi": s.get::<_, String>(7),
            "teslim_sekli": s.get::<_, String>(8), "mazbata_var": mazbata, "mazbata": s.get::<_, String>(10),
        });
        let sabit = match durum::tebligat_kesinlesmesi(&tur) {
            durum::TebligatKesinlesmesi::SabitGun(gun) => Some((gun, durum::OdemeEmri {
                birim_evrak_no: s.get(11),
                evrak_aciklama: s.get(12),
                evrak_tarihi: s.get(3),
                evrak_gunu: s.get(13),
                barkod: s.get(4),
                elektronik_tebligat: s.get(5),
                ptt_durumu: ptt.clone(),
                son_islem_tarihi: s.get(7),
                son_islem_gunu: s.get(14),
                teslim_sekli: s.get(8),
                tebligat_mazbatasi_var: mazbata,
                tebligat_mazbatasi_aciklama: s.get(10),
            })),
            _ => None,
        };
        tebligatlar.entry(s.get(0)).or_default().push(ListeTebligati { tur, ptt, mazbata, veri, sabit });
    }
    let tatiller = veri::resmi_tatiller(c)?;
    let bugun = chrono::Local::now().date_naive();

    let (tur_var, teblig_var, mazbata_var) = (!f.tur.trim().is_empty(), !f.teblig.trim().is_empty(), !f.mazbata.is_empty());
    let aranan: Vec<String> = tr_kucuk(&f.q).split_whitespace().map(str::to_string).collect();
    let bos: Vec<ListeTebligati> = Vec::new();
    let mut say_tur: HashMap<String, i64> = HashMap::new();
    let mut say_teblig: HashMap<String, i64> = HashMap::new();
    let mut say_mazbata: HashMap<String, i64> = HashMap::new();
    let mut say_kes: HashMap<String, i64> = HashMap::new();
    let mut say_birim: HashMap<String, i64> = HashMap::new();
    let mut say_dosya_no: HashMap<String, i64> = HashMap::new();
    let mut say_alacakli: HashMap<String, i64> = HashMap::new();
    let mut say_borclu: HashMap<String, i64> = HashMap::new();
    let mut say_yil: HashMap<String, i64> = HashMap::new();
    let mut say_dosya_tur: HashMap<String, i64> = HashMap::new();
    let ekle = |sayim: &mut HashMap<String, i64>, degerler: BTreeSet<String>| {
        for d in degerler {
            *sayim.entry(d).or_default() += 1;
        }
    };
    // Bir kutunun kendi seçenekleri sayılırken o kutunun süzgeci: listeden seçildiyse (`=` / kodlu) yok sayılır,
    // düz yazıysa uygulanır.
    let yazi = |s: &str| !s.is_empty() && !listeden_mi(s);
    let mut satirlar: Vec<(Option<i64>, Value)> = Vec::new();
    let mut dosyalar: BTreeSet<i64> = BTreeSet::new();
    for s in satir_kayitlari {
        let (birim, dosya_no, borclu): (String, String, String) = (s.get(2), s.get(3), s.get(7));
        let alacaklilar: Vec<String> = s.get(17);
        let alacakli = alacaklilar.join(", ");
        let (dosya_tur, acilis, yil): (String, Option<NaiveDate>, i32) = (s.get(21), s.get(22), s.get(23));
        // MTS'den icra dairesine geçen dosya, "Dosya türü" süzgecinde ayrı değerdir.
        let mts_kaynakli: bool = s.get(24);
        let tur_suzgec = if mts_kaynakli { format!("{dosya_tur}{MTS_KAYNAKLI_EKI}") } else { dosya_tur.clone() };
        let yil_metni = if yil > 0 { yil.to_string() } else { String::new() };
        if !aranan.is_empty() {
            let metin = tr_kucuk(&format!("{birim} {dosya_no} {borclu} {alacakli}"));
            if !aranan.iter().all(|k| metin.contains(k)) {
                continue;
            }
        }
        if f.acilis_bas.is_some_and(|b| acilis.is_none_or(|a| a < b)) || f.acilis_son.is_some_and(|son| acilis.is_none_or(|a| a > son)) {
            continue;
        }
        // Künye süzgeçleri; `haric` verilen kutu (kendi seçenekleri sayılırken) yalnız düz yazıysa uygulanır.
        let kunye_uyar = |haric: Option<Kunye>| {
            let uygula = |k: Kunye, deger: &str| haric != Some(k) || yazi(deger);
            // Alacaklı: "hariç" süzgecinde hariç tutulan bir alacaklısı olan dosya gizlenir; diğerlerinde alacaklılardan biri yeter.
            let alacakli_uyar = || match f.alacakli.starts_with('!') {
                true => alacaklilar.iter().all(|a| metin_uyar(&f.alacakli, a)),
                false => alacaklilar.iter().any(|a| metin_uyar(&f.alacakli, a)),
            };
            (!uygula(Kunye::Birim, &f.birim) || metin_uyar(&f.birim, &birim))
                && (!uygula(Kunye::DosyaNo, &f.dosya_no) || metin_uyar(&f.dosya_no, &dosya_no))
                && (!uygula(Kunye::Alacakli, &f.alacakli) || f.alacakli.is_empty() || alacakli_uyar())
                && (!uygula(Kunye::Borclu, &f.borclu) || metin_uyar(&f.borclu, &borclu))
                && (haric == Some(Kunye::Yil) || kod_uyar(&f.yil, &yil_metni))
                && (haric == Some(Kunye::Tur) || kod_uyar(&f.dosya_tur, &tur_suzgec))
        };
        let kunye_tamam = kunye_uyar(None);
        let bag: Option<i64> = s.get(0);
        let (kes, kes_t): (String, Option<NaiveDate>) = (s.get(10), s.get(11));
        let talimat = durum::talimat_mi(s.get(20), &dosya_tur);
        // Talimat dosyasında kesinleşme yok: süzgeçte "Henüz hesaplanmadı"ya karışmasın diye ayrı değer.
        let kes_suzgec = if talimat { KES_TALIMAT.to_string() } else { kes.clone() };
        let tebl = bag.and_then(|b| tebligatlar.get(&b)).unwrap_or(&bos);
        let kes_uyar = kesinlesme_uyar(&f.kesinlesme, &kes_suzgec);
        // Hangi süzgeçler hesaba katılsın: bir boyutun seçenekleri hesaplanırken o boyut dışarıda tutulur.
        let teb_uyar = |t: &ListeTebligati, tur: bool, teblig: bool, mazbata: bool| {
            (!tur || metin_uyar(&f.tur, &t.tur)) && (!teblig || metin_uyar(&f.teblig, &t.ptt)) && (!mazbata || mazbata_uyar(&f.mazbata, t.mazbata))
        };
        if kes_uyar && kunye_tamam {
            ekle(&mut say_tur, tebl.iter().filter(|t| teb_uyar(t, false, teblig_var, mazbata_var)).map(|t| t.tur.clone()).collect());
            ekle(&mut say_teblig, tebl.iter().filter(|t| teb_uyar(t, tur_var, false, mazbata_var)).map(|t| t.ptt.clone()).collect());
            ekle(
                &mut say_mazbata,
                tebl.iter()
                    .filter(|t| teb_uyar(t, tur_var, teblig_var, false))
                    .map(|t| if t.mazbata { "var" } else { "yok" }.to_string())
                    .collect(),
            );
        }
        let suzgec_var = tur_var || teblig_var || mazbata_var;
        let teb_tamam = !suzgec_var || tebl.iter().any(|t| teb_uyar(t, tur_var, teblig_var, mazbata_var));
        if teb_tamam && kunye_tamam {
            *say_kes.entry(if kes_suzgec.is_empty() { "-".to_string() } else { kes_suzgec.clone() }).or_default() += 1;
        }
        if kes_uyar && teb_tamam {
            if kunye_uyar(Some(Kunye::Birim)) {
                *say_birim.entry(birim.clone()).or_default() += 1;
            }
            if kunye_uyar(Some(Kunye::DosyaNo)) {
                *say_dosya_no.entry(dosya_no.clone()).or_default() += 1;
            }
            if kunye_uyar(Some(Kunye::Alacakli)) {
                let uyanlar: BTreeSet<String> =
                    alacaklilar.iter().filter(|a| !yazi(&f.alacakli) || metin_uyar(&f.alacakli, a)).cloned().collect();
                ekle(&mut say_alacakli, uyanlar);
            }
            if !borclu.is_empty() && kunye_uyar(Some(Kunye::Borclu)) {
                *say_borclu.entry(borclu.clone()).or_default() += 1;
            }
            if !yil_metni.is_empty() && kunye_uyar(Some(Kunye::Yil)) {
                *say_yil.entry(yil_metni.clone()).or_default() += 1;
            }
            if !dosya_tur.is_empty() && kunye_uyar(Some(Kunye::Tur)) {
                *say_dosya_tur.entry(tur_suzgec.clone()).or_default() += 1;
            }
        }
        if !(kes_uyar && teb_tamam && kunye_tamam) {
            continue;
        }
        let dosya: i64 = s.get(1);
        dosyalar.insert(dosya);
        let (asama, asama_t): (String, Option<NaiveDate>) = (s.get(8), s.get(9));
        let (tebl_d, tebl_t, tebl_s): (String, Option<NaiveDate>, String) = (s.get(12), s.get(13), s.get(14));
        satirlar.push((bag, json!({
            "bag": bag, "dosya": dosya, "birim": birim, "dosya_no": dosya_no, "dosya_tur": dosya_tur, "mts_kaynakli": mts_kaynakli,
            "dosya_durumu": s.get::<_, String>(4), "acik": s.get::<_, i32>(5) == 0, "acilis_tarihi": gun(acilis),
            "islem_durumu": s.get::<_, String>(6), "borclu": borclu, "alacakli": alacakli,
            "asama": asama, "asama_etiket": if bag.is_some() { TakipAsamasi::etiket_bul(&asama) } else { String::new() },
            "asama_tarihi": gun(asama_t),
            "kesinlesme": if bag.is_some() && !talimat { durum::kesinlesme_metni(&kes, kes_t) } else { String::new() },
            "kesinlesme_durumu": kes, "kesinlesme_tarihi": gun(kes_t), "talimat": talimat,
            "tebligat": if bag.is_some() { durum::tebligat_metni(&tebl_d, tebl_t, &tebl_s) } else { String::new() },
            "tebligat_durumu": tebl_d,
            "uyapta_gorulmedi": s.get::<_, bool>(15), "itiraz": s.get::<_, bool>(16),
            "uyap": uyap_karti(&s.get::<_, String>(18), &birim, &dosya_no, s.get(20), &dosya_tur, &s.get::<_, String>(19),
                               s.get::<_, String>(4).as_str(), s.get(5)),
        })));
    }
    if let Some(out) = kimlikler {
        let mut gorulen = std::collections::HashSet::new();
        out.extend(satirlar.iter().filter_map(|(_, s)| s["dosya"].as_i64()).filter(|d| gorulen.insert(*d)));
        return Ok(Value::Null);
    }
    let toplam = satirlar.len();
    let boyut = f.boyut.clamp(1, 500);
    let sayfa = f.sayfa.max(1);
    let suzgec_var = tur_var || teblig_var || mazbata_var;
    let dilim: Vec<Value> = satirlar
        .into_iter()
        .skip((sayfa - 1) * boyut)
        .take(boyut)
        .map(|(bag, mut satir)| {
            // Borçlunun bütün tebligatları; süzgeç varsa uyanlar işaretlenir. Kesinleşme tebligat başına gösterilir:
            // ödeme emrinde borçlunun hesabı, 103 / kıymet takdirinde sabit süre, diğer türlerde yok.
            let borclu_kes = satir["kesinlesme"].clone();
            let talimat = satir["talimat"].as_bool().unwrap_or(false);
            let liste: Vec<Value> = bag
                .and_then(|b| tebligatlar.get(&b))
                .unwrap_or(&bos)
                .iter()
                .map(|t| {
                    let mut v = t.veri.clone();
                    v["uyan"] = json!(
                        suzgec_var && metin_uyar(&f.tur, &t.tur) && metin_uyar(&f.teblig, &t.ptt) && mazbata_uyar(&f.mazbata, t.mazbata)
                    );
                    v["kesinlesme"] = match (durum::tebligat_kesinlesmesi(&t.tur), &t.sabit) {
                        _ if talimat => json!(""),
                        (durum::TebligatKesinlesmesi::Borclu, _) => borclu_kes.clone(),
                        (durum::TebligatKesinlesmesi::SabitGun(_), Some((gun, oe))) => {
                            let (k, kt) = durum::borclu_kesinlesme_hesapla(false, std::slice::from_ref(oe), &tatiller, bugun, None, Some(*gun));
                            json!(durum::kesinlesme_metni(&k, kt))
                        }
                        _ => json!(""),
                    };
                    v["kesinlesme_yok"] = json!(talimat || durum::tebligat_kesinlesmesi(&t.tur) == durum::TebligatKesinlesmesi::Yok);
                    if talimat {
                        v["kesinlesme_neden"] = json!(durum::TALIMAT_KESINLESME_YOK);
                    }
                    v
                })
                .collect();
            satir["tebligatlar"] = Value::Array(liste);
            satir
        })
        .collect();
    let ayni = |d: &str| d.to_string();
    let mut yillar = secenek_listesi(say_yil, &ayni);
    yillar.sort_by(|a, b| b["deger"].as_str().cmp(&a["deger"].as_str()));
    Ok(json!({
        "toplam": toplam, "dosya_sayisi": dosyalar.len(), "sayfa": sayfa, "boyut": boyut, "satirlar": dilim,
        "secenek_siniri": SECENEK_SINIRI,
        "secenekler": {
            "birim": secenek_sinirli(say_birim, &ayni, SECENEK_SINIRI),
            "dosya_no": secenek_sinirli(say_dosya_no, &ayni, SECENEK_SINIRI),
            "alacakli": secenek_sinirli(say_alacakli, &ayni, SECENEK_SINIRI),
            "borclu": secenek_sinirli(say_borclu, &ayni, SECENEK_SINIRI),
            "yil": yillar,
            "dosya_tur": secenek_listesi(say_dosya_tur, &ayni),
            "tur": secenek_listesi(say_tur, &|d| if d.is_empty() { "(türü yok)".to_string() } else { d.to_string() }),
            "teblig": secenek_listesi(say_teblig, &|d| if d.is_empty() { "Henüz sorgulanmadı".to_string() } else { d.to_string() }),
            "mazbata": secenek_listesi(say_mazbata, &|d| if d == "var" { "Döndü" } else { "Dönmedi" }.to_string()),
            "kesinlesme": secenek_listesi(say_kes, &|d| match d {
                "-" => "Henüz hesaplanmadı".to_string(),
                KES_TALIMAT => "Talimat dosyası (kesinleşme yok)".to_string(),
                _ => KesinlesmeDurumu::etiket_bul(d),
            }),
        },
    }))
}

/// Dosya penceresi: künye, takip türü ve uygulanan itiraz süresi, taraflar (borçlularda kesinleşme
/// / tebliğ / takip aşaması), tebligatlar. Dosya yoksa `None`.
pub fn dosya_ayrintisi<C: GenericClient>(c: &mut C, dosya: i64) -> Result<Option<Value>, String> {
    let Some(d) = c
        .query_opt(
            &format!(
            "SELECT b.ad, d.dosya_no, d.durum, d.durum_kod, d.tur, to_char(d.acilis_tarihi, 'YYYY-MM-DD'), d.yargi_turu,
                    d.islem_durumu, d.islem_durumu_notu, d.bekleme_bitis_tarihi, to_char(d.son_senkron, 'YYYY-MM-DD HH24:MI'),
                    i.takibin_turu, i.takibin_yolu, i.takibin_sekli, i.alacak_kalemi_toplam::text, i.vekalet_ucreti::text,
                    i.tahsil_harci::text, b.birim_id, b.turu3, d.tur_kod,
                    i.takibin_turu_aciklama, i.takibin_yolu_aciklama, i.takibin_sekli_aciklama, i.ornek_no,
                    to_char(i.ayrinti_zamani, 'YYYY-MM-DD HH24:MI'), {mts}
               FROM dosya d JOIN birim b ON b.id = d.birim_id LEFT JOIN icra_takip_detay i ON i.dosya_id = d.id
              WHERE d.id = $1",
            mts = MTS_KAYNAKLI_SQL
        ),
            &[&dosya],
        )
        .map_err(hata)?
    else {
        return Ok(None);
    };
    let kod = |i: usize| d.get::<_, Option<String>>(i).unwrap_or_default();
    let kurallar = veri::kesinlesme_kurallari(c)?;
    let talimat = durum::talimat_mi(d.get(19), &d.get::<_, String>(4));
    let kural = veri::kesinlesme_kurali_sec(&kurallar, &kod(11), &kod(12), &kod(13)).filter(|_| !talimat);
    let islem: String = d.get(7);
    let kunye = json!({
        "id": dosya, "birim": d.get::<_, String>(0), "dosya_no": d.get::<_, String>(1), "durum": d.get::<_, String>(2),
        "acik": d.get::<_, i32>(3) == 0, "tur": d.get::<_, String>(4), "acilis_tarihi": d.get::<_, Option<String>>(5),
        "yargi_turu": veri::YargiTuru::coz(d.get(6)).map(|y| y.etiket()),
        "islem_durumu": islem, "islem_durumu_etiket": IslemDurumu::etiket_bul(&islem),
        "islem_durumu_notu": d.get::<_, String>(8), "bekleme_bitis": gun(d.get(9)), "son_guncelleme": d.get::<_, String>(10),
        "talimat": talimat, "mts_kaynakli": d.get::<_, bool>(25),
        "mts_icraya_gecti": mts_mi(d.get(19), &d.get::<_, String>(4)) && d.get::<_, String>(2).contains("Borcun Tamamının Ödenmemesi"),
        "uyap": uyap_karti(&d.get::<_, String>(17), &d.get::<_, String>(0), &d.get::<_, String>(1), d.get(19),
                           &d.get::<_, String>(4), &d.get::<_, String>(18), &d.get::<_, String>(2), d.get(3)),
    });
    let adlar = TakipAdlari::oku(c)?;
    let ad = |i: usize, kademe: i16| {
        let kayitli = kod(i);
        if kayitli.is_empty() { adlar.ad(kademe, &kod(11), &kod(12), &kod(13)) } else { kayitli }
    };
    let sekli_adi = ad(22, 3);
    let takip = json!({
        "okundu": d.get::<_, Option<String>>(11).is_some(), "turu": kod(11), "yolu": kod(12), "sekli": kod(13),
        "turu_ad": ad(20, 1), "yolu_ad": ad(21, 2), "sekli_ad": sekli_adi,
        "ornek_no": if kod(23).is_empty() { crate::icra_kayit::ornek_no(&sekli_adi) } else { kod(23) },
        "okunma": d.get::<_, Option<String>>(24),
        "alacak_toplam": d.get::<_, Option<String>>(14), "vekalet_ucreti": d.get::<_, Option<String>>(15),
        "tahsil_harci": d.get::<_, Option<String>>(16),
        "kesinlesme_suresi_gun": kural.and_then(|k| k.kesinlesme_suresi_gun), "kural": kural.map(|k| k.ad.clone()),
        "itiraz_gun": kural.and_then(|k| k.itiraz_gun), "itiraz_durdurur": kural.map_or(true, |k| k.itiraz_durdurur),
        "dayanak": kural.map(|k| k.dayanak.clone()).unwrap_or_default(),
        "kesinlesme_yok": if talimat {
            json!(durum::TALIMAT_KESINLESME_YOK)
        } else if kural.is_some_and(|k| k.kesinlesme_suresi_gun.is_none()) {
            json!(TUR_KESINLESME_YOK)
        } else {
            Value::Null
        },
    });

    let sql = format!(
        "SELECT dt.id, dt.rol, {ad}, t.tur, t.tckn, t.mersis_no, t.vergi_no, dt.uyapta_gorulmedi IS NOT NULL,
                dt.takip_asamasi, dt.takip_asamasi_tarihi, dt.kesinlesme_durumu, dt.kesinlesme_tarihi,
                dt.tebligat_durumu, dt.tebligat_durum_tarihi, dt.tebligat_sekli, dt.itiraz_edildi_mi, dt.itiraz_tarihi,
                COALESCE((SELECT string_agg(btrim(v.ad || ' ' || v.soyad), ', ' ORDER BY v.id) FROM dosya_taraf_vekiller dv
                           JOIN vekil v ON v.id = dv.vekil_id WHERE dv.dosyataraf_id = dt.id), ''),
                dt.kesinlesme_elle_tarihi
           FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE dt.dosya_id = $1 ORDER BY dt.sira, dt.id",
        ad = ad_sql("t")
    );
    let mut taraflar = Vec::new();
    for s in c.query(&sql, &[&dosya]).map_err(hata)? {
        let rol: String = s.get(1);
        let (asama, asama_t): (String, Option<NaiveDate>) = (s.get(8), s.get(9));
        let (kes, kes_t): (String, Option<NaiveDate>) = (s.get(10), s.get(11));
        let (tebl, tebl_t, tebl_s): (String, Option<NaiveDate>, String) = (s.get(12), s.get(13), s.get(14));
        let kimlik = [s.get::<_, Option<String>>(4), s.get::<_, Option<String>>(5), Some(s.get::<_, String>(6))]
            .into_iter()
            .flatten()
            .find(|k| !k.is_empty());
        taraflar.push(json!({
            "bag": s.get::<_, i64>(0), "rol": rol, "rol_etiket": Rol::etiket_bul(&rol), "ad": s.get::<_, String>(2),
            "tuzel": s.get::<_, String>(3) == "tuzel", "kimlik": kimlik, "uyapta_gorulmedi": s.get::<_, bool>(7),
            "vekiller": s.get::<_, String>(17),
            "asama": asama, "asama_metni": durum::takip_asamasi_metni(&asama, asama_t),
            "kesinlesme_durumu": kes, "kesinlesme": durum::kesinlesme_metni(&kes, kes_t),
            "tebligat": durum::tebligat_metni(&tebl, tebl_t, &tebl_s), "teblig_kodu": tebl,
            "itiraz": s.get::<_, bool>(15), "itiraz_tarihi": gun(s.get(16)), "kesinlesme_elle_tarihi": gun(s.get(18)),
        }));
    }

    // Borçluların Mernis kıyası ve son yeniden tebliğ talebi (sağ tık menüsü ve kart).
    for s in c
        .query(
            "SELECT x.id, m.kategori, m.adres, m.teblig_adresi, m.aciklama, to_char(m.zaman, 'DD.MM.YYYY HH24:MI'),
                    y.sonuc, to_char(y.zaman, 'DD.MM.YYYY HH24:MI'), y.mesaj, y.tur
               FROM dosya_taraf x LEFT JOIN borclu_mernis m ON m.dosyataraf_id = x.id
               LEFT JOIN LATERAL (SELECT sonuc, zaman, mesaj, tur FROM yeniden_teblig_talebi yt WHERE yt.dosyataraf_id = x.id
                                   ORDER BY zaman DESC, id DESC LIMIT 1) y ON true
              WHERE x.dosya_id = $1 AND x.rol = 'borclu' AND (m.dosyataraf_id IS NOT NULL OR y.sonuc IS NOT NULL)",
            &[&dosya],
        )
        .map_err(hata)?
    {
        let bag: i64 = s.get(0);
        let Some(t) = taraflar.iter_mut().find(|t| t["bag"] == bag) else { continue };
        if let Some(k) = s.get::<_, Option<String>>(1) {
            let tur = crate::icra_mernis::talep_turu(&k);
            t["mernis"] = json!({"kategori": k, "etiket": crate::icra_mernis::kategori_etiketi(&k), "adres": s.get::<_, String>(2),
                                 "teblig_adresi": s.get::<_, String>(3), "aciklama": s.get::<_, String>(4), "zaman": s.get::<_, String>(5),
                                 "talep_turu": tur.map(|x| x.1), "tutar": tur.map(|x| x.2)});
        }
        if let Some(sonuc) = s.get::<_, Option<String>>(6) {
            t["yeniden_talep"] = json!({"sonuc": sonuc, "zaman": s.get::<_, Option<String>>(7), "mesaj": s.get::<_, Option<String>>(8),
                                        "tur": s.get::<_, Option<i16>>(9)});
        }
    }

    let sql = format!(
        "SELECT tb.id, tb.birim_evrak_no, tb.tebligat_turu, tb.evrak_aciklama, tb.evrak_tarihi, tb.barkod,
                tb.elektronik_tebligat, tb.ptt_durumu, tb.son_islem_tarihi, tb.teslim_sekli, tb.tebligat_mazbatasi_var,
                tb.tebligat_mazbatasi_aciklama, tb.yeniden_teblig_talep_edildi, tb.borclu_id, {ad},
                (SELECT count(*) FROM ptt_hareket h WHERE h.tebligat_id = tb.id), tb.zarf_icerigi, tb.haciz_sirasi
           FROM tebligat_barkod tb LEFT JOIN dosya_taraf dt ON dt.id = tb.borclu_id LEFT JOIN taraf t ON t.id = dt.taraf_id
          WHERE tb.dosya_id = $1 ORDER BY tb.evrak_gunu DESC NULLS LAST, tb.birim_evrak_no DESC",
        ad = ad_sql("t")
    );
    let mut tebligatlar = Vec::new();
    for s in c.query(&sql, &[&dosya]).map_err(hata)? {
        let aciklama: String = s.get(3);
        let borclu: Option<i64> = s.get(13);
        tebligatlar.push(json!({
            "id": s.get::<_, i64>(0), "evrak_no": s.get::<_, i32>(1), "tur": s.get::<_, String>(2),
            "muhatap": teb::tebligat_muhatabi(&aciklama).0, "aciklama": aciklama, "evrak_tarihi": s.get::<_, String>(4),
            "barkod": s.get::<_, String>(5), "elektronik": s.get::<_, bool>(6), "ptt_durumu": s.get::<_, String>(7),
            "son_islem_tarihi": s.get::<_, String>(8), "teslim_sekli": s.get::<_, String>(9),
            "mazbata_var": s.get::<_, bool>(10), "mazbata": s.get::<_, String>(11),
            "yeniden_teblig_talebi": s.get::<_, bool>(12), "borclu_bag": borclu,
            "borclu": borclu.map(|_| s.get::<_, String>(14)), "hareket_sayisi": s.get::<_, i64>(15),
            "grup": teb::tebligat_grubu(&s.get::<_, String>(2)), "zarf_icerigi": s.get::<_, String>(16),
            "haciz_sirasi": s.get::<_, Option<i16>>(17), "haciz_turu": teb::haciz_turu_mu(&s.get::<_, String>(2)),
        }));
    }
    // Evrak listesinde olup tebligat kaydı olmayan zarflar (alınamamış) ve nedeni.
    let alinamayan: Vec<Value> = c
        .query(
            "SELECT e.birim_evrak_no, e.evrak_turu, e.aciklama, to_char(e.evrak_tarihi, 'DD/MM/YYYY'), e.tebligat_notu FROM evrak e
              WHERE e.dosya_id = $1 AND e.evrak_turu IN ($2, $3)
                AND NOT EXISTS (SELECT 1 FROM tebligat_barkod tb WHERE tb.dosya_id = e.dosya_id AND tb.birim_evrak_no = e.birim_evrak_no)
              ORDER BY e.evrak_tarihi DESC NULLS LAST, e.birim_evrak_no DESC",
            &[&dosya, &teb::HEDEF_TUR, &teb::MTS_TEBLIGAT_TUR],
        )
        .map_err(hata)?
        .iter()
        .map(|s| {
            let aciklama: String = s.get(2);
            json!({"evrak_no": s.get::<_, i32>(0), "tur": s.get::<_, String>(1), "muhatap": teb::tebligat_muhatabi(&aciklama).0,
                   "aciklama": aciklama, "evrak_tarihi": s.get::<_, Option<String>>(3), "neden": s.get::<_, String>(4)})
        })
        .collect();
    // Elle bağlanabilen mazbata evrakları ve bağları (sağ tık menüsü).
    let elle: HashMap<i32, i32> = crate::icra_kayit::elle_mazbata_baglari(c, dosya)?.into_iter().map(|(t, m)| (m, t)).collect();
    let turler: Vec<String> = BAGLANABILIR_MAZBATALAR.iter().map(|t| t.to_string()).collect();
    let mazbatalar: Vec<Value> = c
        .query(
            "SELECT e.birim_evrak_no, e.evrak_turu, e.aciklama, to_char(e.evrak_tarihi, 'DD/MM/YYYY'), to_char(mb.teblig_tarihi, 'DD/MM/YYYY')
               FROM evrak e LEFT JOIN tebligat_mazbata_bagi mb ON mb.dosya_id = e.dosya_id AND mb.mazbata_evrak_no = e.birim_evrak_no
              WHERE e.dosya_id = $1 AND e.evrak_turu = ANY($2) ORDER BY e.evrak_tarihi DESC NULLS LAST, e.birim_evrak_no DESC",
            &[&dosya, &turler],
        )
        .map_err(hata)?
        .iter()
        .map(|s| {
            let no: i32 = s.get(0);
            json!({"evrak_no": no, "tur": s.get::<_, String>(1), "aciklama": s.get::<_, String>(2),
                   "evrak_tarihi": s.get::<_, Option<String>>(3), "elle_tebligat": elle.get(&no),
                   "teblig_tarihi": s.get::<_, Option<String>>(4)})
        })
        .collect();
    let elle_tebligatlar: std::collections::HashSet<i32> = elle.values().copied().collect();
    for t in tebligatlar.iter_mut() {
        let no = t["evrak_no"].as_i64().unwrap_or(0) as i32;
        t["mazbata_elle"] = json!(elle_tebligatlar.contains(&no));
    }
    Ok(Some(json!({"kunye": kunye, "takip": takip, "taraflar": taraflar, "tebligatlar": tebligatlar, "alinamayan": alinamayan,
                   "mazbatalar": mazbatalar})))
}

/// Dosyanın durum geçmişi, yeniden eskiye.
pub fn gecmis<C: GenericClient>(c: &mut C, dosya: i64) -> Result<Value, String> {
    let satirlar = c
        .query(
            "SELECT to_char(zaman, 'YYYY-MM-DD HH24:MI'), tur, taraf_adi, eski, yeni, olay_tarihi, kaynak, aciklama
               FROM durum_olayi WHERE dosya_id = $1 ORDER BY zaman DESC, id DESC",
            &[&dosya],
        )
        .map_err(hata)?;
    Ok(Value::Array(
        satirlar
            .iter()
            .map(|s| {
                let (tur, kaynak): (String, String) = (s.get(1), s.get(6));
                json!({
                    "zaman": s.get::<_, String>(0), "tur": tur, "tur_etiket": OlayTur::etiket_bul(&tur),
                    "taraf": s.get::<_, String>(2), "eski": s.get::<_, String>(3), "yeni": s.get::<_, String>(4),
                    "olay_tarihi": gun(s.get(5)), "kaynak": kaynak, "kaynak_etiket": OlayKaynak::etiket_bul(&kaynak),
                    "aciklama": s.get::<_, String>(7),
                })
            })
            .collect(),
    ))
}

/// Bir tebligatın PTT hareketleri, sırasıyla.
pub fn ptt_hareketleri<C: GenericClient>(c: &mut C, tebligat: i64) -> Result<Value, String> {
    let satirlar = c
        .query(
            "SELECT to_char(zaman, 'YYYY-MM-DD HH24:MI'), aciklama, yer, islem_detay FROM ptt_hareket
              WHERE tebligat_id = $1 ORDER BY sira",
            &[&tebligat],
        )
        .map_err(hata)?;
    Ok(Value::Array(
        satirlar
            .iter()
            .map(|s| {
                json!({"zaman": s.get::<_, Option<String>>(0), "aciklama": s.get::<_, String>(1),
                       "yer": s.get::<_, String>(2), "detay": s.get::<_, String>(3)})
            })
            .collect(),
    ))
}

// ── Kesinleşme süreleri ─────────────────────────────────────────────────────────────────────────

/// Kesinleşme süreleri ve veritabanındaki icra dosyalarının takip kodu dağılımı (kural girerken
/// hangi kodun kaç dosyada olduğu görülsün); takip türü hiç okunmamış dosya sayısı ayrıca.
pub fn kurallar<C: GenericClient>(c: &mut C) -> Result<Value, String> {
    let adlar = TakipAdlari::oku(c)?;
    let adli = |v: &mut Value, turu: &str, yolu: &str, sekli: &str| {
        let sekli_adi = if sekli.is_empty() { String::new() } else { adlar.ad(3, turu, yolu, sekli) };
        v["turu_ad"] = json!(if turu.is_empty() { String::new() } else { adlar.ad(1, turu, "", "") });
        v["yolu_ad"] = json!(if yolu.is_empty() { String::new() } else { adlar.ad(2, turu, yolu, "") });
        v["ornek_no"] = json!(crate::icra_kayit::ornek_no(&sekli_adi));
        v["sekli_ad"] = json!(sekli_adi);
    };
    let kurallar = veri::kesinlesme_kurallari(c)?;
    let dagilim: Vec<Value> = c
        .query(
            "SELECT i.takibin_turu, i.takibin_yolu, i.takibin_sekli, count(*),
                    (SELECT b.ad || ' ' || d2.dosya_no FROM dosya d2 JOIN birim b ON b.id = d2.birim_id WHERE d2.id = min(d.id))
               FROM icra_takip_detay i JOIN dosya d ON d.id = i.dosya_id WHERE d.yargi_turu = 2 AND d.tur_kod <> 1
              GROUP BY 1, 2, 3 ORDER BY 4 DESC",
            &[],
        )
        .map_err(hata)?
        .iter()
        .map(|s| {
            let (turu, yolu, sekli): (String, String, String) = (s.get(0), s.get(1), s.get(2));
            let mut v = json!({"turu": turu, "yolu": yolu, "sekli": sekli, "adet": s.get::<_, i64>(3), "ornek": s.get::<_, Option<String>>(4)});
            adli(&mut v, &turu, &yolu, &sekli);
            v
        })
        .collect();
    let liste_zamani: Option<String> = c
        .query_one("SELECT to_char(max(guncelleme), 'YYYY-MM-DD HH24:MI') FROM takip_turu_listesi", &[])
        .map_err(hata)?
        .get(0);
    let bilinmeyen: i64 = c
        .query_one(
            "SELECT count(*) FROM dosya d WHERE d.yargi_turu = 2 AND d.tur_kod <> 1
                AND NOT EXISTS (SELECT 1 FROM icra_takip_detay i WHERE i.dosya_id = d.id)",
            &[],
        )
        .map_err(hata)?
        .get(0);
    // Her kurala, ona düşen dosya sayısı (en özel eşleşme; takip türü okunmamış dosyalar varsayılana düşer).
    let mut adet: HashMap<i64, i64> = HashMap::new();
    for x in &dagilim {
        let kod = |a: &str| x[a].as_str().unwrap_or("").to_string();
        if let Some(k) = veri::kesinlesme_kurali_sec(&kurallar, &kod("turu"), &kod("yolu"), &kod("sekli")) {
            *adet.entry(k.id).or_default() += x["adet"].as_i64().unwrap_or(0);
        }
    }
    if let Some(k) = veri::kesinlesme_kurali_sec(&kurallar, "", "", "") {
        *adet.entry(k.id).or_default() += bilinmeyen;
    }
    let liste: Vec<Value> = kurallar
        .iter()
        .map(|k| {
            let hazir = veri::varsayilan_kural(&k.takibin_turu, &k.takibin_yolu, &k.takibin_sekli);
            let mut v = json!({"id": k.id, "takibin_turu": k.takibin_turu, "takibin_yolu": k.takibin_yolu, "takibin_sekli": k.takibin_sekli,
                   "ad": k.ad, "gun": k.kesinlesme_suresi_gun, "itiraz_gun": k.itiraz_gun, "itiraz_durdurur": k.itiraz_durdurur,
                   "dayanak": k.dayanak, "aktif": k.aktif, "aciklama": k.aciklama, "elle_degisti": k.elle_degisti,
                   "adet": adet.get(&k.id).copied().unwrap_or(0),
                   "varsayilan": hazir.map(|h| json!({"ad": h.ad, "gun": h.kesinlesme, "itiraz_gun": h.itiraz,
                                                       "itiraz_durdurur": h.itiraz_durdurur, "dayanak": h.dayanak}))});
            adli(&mut v, &k.takibin_turu, &k.takibin_yolu, &k.takibin_sekli);
            v
        })
        .collect();
    Ok(json!({"kurallar": liste, "dagilim": dagilim, "takip_turu_bilinmeyen": bilinmeyen, "agac": takip_agaci(c, &adlar)?,
              "liste_zamani": liste_zamani}))
}

/// Takip türü / yolu / şekli adları: UYAP takip açılış listesi (`takip_turu_listesi`), yoksa dosyaların ayrıntısında
/// görülen ad. Anahtar: (kademe, tür, yol, şekil) — kademe 1'de yol ve şekil, 2'de şekil boştur.
pub struct TakipAdlari(HashMap<(i16, String, String, String), String>);

impl TakipAdlari {
    pub fn oku<C: GenericClient>(c: &mut C) -> Result<Self, String> {
        let mut m: HashMap<(i16, String, String, String), String> = HashMap::new();
        // Önce dosyalarda görülen adlar, sonra UYAP listesi (liste üstüne yazar).
        for s in c
            .query(
                "SELECT takibin_turu, takibin_yolu, takibin_sekli, max(takibin_turu_aciklama), max(takibin_yolu_aciklama),
                        max(takibin_sekli_aciklama)
                   FROM icra_takip_detay WHERE takibin_turu <> '' GROUP BY 1, 2, 3",
                &[],
            )
            .map_err(hata)?
        {
            let (turu, yolu, sekli): (String, String, String) = (s.get(0), s.get(1), s.get(2));
            let (a1, a2, a3): (String, String, String) = (s.get(3), s.get(4), s.get(5));
            for (anahtar, ad) in [
                ((1, turu.clone(), String::new(), String::new()), a1),
                ((2, turu.clone(), yolu.clone(), String::new()), a2),
                ((3, turu.clone(), yolu.clone(), sekli.clone()), a3),
            ] {
                if !ad.is_empty() {
                    m.insert(anahtar, ad);
                }
            }
        }
        for s in c.query("SELECT kademe, takibin_turu, takibin_yolu, takibin_sekli, ad FROM takip_turu_listesi WHERE ad <> ''", &[]).map_err(hata)? {
            m.insert((s.get(0), s.get(1), s.get(2), s.get(3)), s.get(4));
        }
        Ok(Self(m))
    }

    /// Adı; bilinmiyorsa boş.
    pub fn ad(&self, kademe: i16, turu: &str, yolu: &str, sekli: &str) -> String {
        let (y, s) = match kademe {
            1 => ("", ""),
            2 => (yolu, ""),
            _ => (yolu, sekli),
        };
        self.0.get(&(kademe, turu.to_string(), y.to_string(), s.to_string())).cloned().unwrap_or_default()
    }
}

/// Kodları sayıya göre sırala (sayı olmayanlar sonda, metin sırasıyla).
fn kod_sirasi(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    }
}

/// Kesinleşme süresi seçimi için takip türü ağacı: tür → yollar → şekiller, adları ve dosya sayılarıyla. UYAP
/// listesindeki bütün seçenekler ve dosyalarda görülen (listede olmayan) kodlar birlikte.
fn takip_agaci<C: GenericClient>(c: &mut C, adlar: &TakipAdlari) -> Result<Value, String> {
    // tür → yol → şekil → dosya sayısı
    let mut agac: BTreeMap<String, BTreeMap<String, BTreeMap<String, i64>>> = BTreeMap::new();
    for s in c.query("SELECT kademe, takibin_turu, takibin_yolu, takibin_sekli FROM takip_turu_listesi", &[]).map_err(hata)? {
        let (kademe, turu, yolu, sekli): (i16, String, String, String) = (s.get(0), s.get(1), s.get(2), s.get(3));
        let t = agac.entry(turu).or_default();
        if kademe >= 2 {
            let y = t.entry(yolu).or_default();
            if kademe == 3 {
                y.entry(sekli).or_default();
            }
        }
    }
    for s in c
        .query(
            "SELECT i.takibin_turu, i.takibin_yolu, i.takibin_sekli, count(*) FROM icra_takip_detay i JOIN dosya d ON d.id = i.dosya_id
              WHERE d.yargi_turu = 2 AND d.tur_kod <> 1 AND i.takibin_turu <> '' GROUP BY 1, 2, 3",
            &[],
        )
        .map_err(hata)?
    {
        let (turu, yolu, sekli, n): (String, String, String, i64) = (s.get(0), s.get(1), s.get(2), s.get(3));
        let y = agac.entry(turu).or_default().entry(yolu.clone()).or_default();
        if !yolu.is_empty() {
            *y.entry(sekli).or_default() += n;
        }
    }
    let sirali = |m: Vec<String>| {
        let mut m = m;
        m.sort_by(|a, b| kod_sirasi(a, b));
        m
    };
    let mut out = Vec::new();
    for turu in sirali(agac.keys().cloned().collect()) {
        let yollar_m = &agac[&turu];
        let mut yollar = Vec::new();
        let mut tur_adet = 0;
        for yolu in sirali(yollar_m.keys().filter(|y| !y.is_empty()).cloned().collect()) {
            let sekiller_m = &yollar_m[&yolu];
            let mut yol_adet = 0;
            let sekiller: Vec<Value> = sirali(sekiller_m.keys().filter(|x| !x.is_empty()).cloned().collect())
                .into_iter()
                .map(|sekli| {
                    let n = sekiller_m[&sekli];
                    yol_adet += n;
                    let ad = adlar.ad(3, &turu, &yolu, &sekli);
                    json!({"kod": sekli, "ad": ad, "ornek_no": crate::icra_kayit::ornek_no(&ad), "adet": n})
                })
                .collect();
            tur_adet += yol_adet;
            yollar.push(json!({"kod": yolu, "ad": adlar.ad(2, &turu, &yolu, ""), "adet": yol_adet, "sekiller": sekiller}));
        }
        out.push(json!({"kod": turu, "ad": adlar.ad(1, &turu, "", ""), "adet": tur_adet, "yollar": yollar}));
    }
    Ok(Value::Array(out))
}

/// Kesinleşme kuralının kullanıcıdan gelen alanları.
#[derive(Debug, Default, Clone)]
pub struct KuralGirdisi {
    pub id: Option<i64>,
    pub turu: String,
    pub yolu: String,
    pub sekli: String,
    pub ad: String,
    /// Kesinleşme süresi; boş: bu türde kesinleşme yok.
    pub gun: Option<i64>,
    /// İtiraz / başvuru süresi (bilgi).
    pub itiraz_gun: Option<i64>,
    pub itiraz_durdurur: bool,
    pub dayanak: String,
    pub aktif: bool,
    pub aciklama: String,
}

impl KuralGirdisi {
    fn oku(g: &Value) -> Self {
        let metin = |a: &str| teb::metin(g, a);
        let evet = |a: &str, yoksa: bool| g.get(a).and_then(Value::as_bool).unwrap_or(yoksa);
        Self {
            id: teb::tam(g, "id"),
            turu: metin("takibin_turu"),
            yolu: metin("takibin_yolu"),
            sekli: metin("takibin_sekli"),
            ad: metin("ad"),
            gun: teb::tam(g, "gun"),
            itiraz_gun: teb::tam(g, "itiraz_gun"),
            itiraz_durdurur: evet("itiraz_durdurur", true),
            dayanak: metin("dayanak"),
            aktif: evet("aktif", true),
            aciklama: metin("aciklama"),
        }
    }
}

/// Bu türde kesinleşme yok (kural süresi boş): dosya panelinde gösterilir.
pub const TUR_KESINLESME_YOK: &str = "Bu takip türünde kesinleşme yok";

fn gun_dogrula(gun: Option<i64>, ne: &str) -> Result<Option<i16>, String> {
    match gun {
        Some(g) if !(1..=365).contains(&g) => Err(format!("{ne} 1–365 gün arasında olmalı.")),
        Some(g) => Ok(Some(g as i16)),
        None => Ok(None),
    }
}

/// Süre ekler ya da günceller. `gun` boşsa o türde kesinleşme hesaplanmaz. Aynı takip kodu için
/// ikinci kural açılmaz. Değerler hazır kuraldan farklıysa `elle_degisti` işaretlenir (açılışta
/// varsayılan yazılmaz); hazır kuralı olmayan kod her zaman elle sayılır. Döner: kuralın kimliği.
pub fn kural_kaydet<C: GenericClient>(c: &mut C, k: &KuralGirdisi) -> Result<i64, String> {
    let (id, turu, yolu, sekli, aktif, aciklama) = (k.id, k.turu.as_str(), k.yolu.as_str(), k.sekli.as_str(), k.aktif, k.aciklama.as_str());
    let (turu, yolu, sekli) = (turu.trim(), yolu.trim(), sekli.trim());
    if [turu, yolu, sekli].iter().any(|k| k.chars().count() > 8) {
        return Err("Takip kodu en çok 8 karakter olabilir.".into());
    }
    // Kodlar yalnız üstündekiyle birlikte anlamlıdır (şekil "0" ilamsızda Örnek 7, ilamlıda Örnek 4-5).
    if (turu.is_empty() && !yolu.is_empty()) || (yolu.is_empty() && !sekli.is_empty()) {
        return Err("Takip yolu için önce takip türü, takip şekli için önce takip yolu seçilmeli.".into());
    }
    let gun = gun_dogrula(k.gun, "Kesinleşme süresi")?;
    let itiraz_gun = gun_dogrula(k.itiraz_gun, "İtiraz süresi")?;
    if let (Some(kes), Some(it)) = (gun, itiraz_gun) {
        if it > kes {
            return Err("Kesinleşme süresi itiraz süresinden kısa olamaz (itiraz ve ödeme sürelerinden uzun olanı yazılır).".into());
        }
    }
    let dayanak = veri::kirp(k.dayanak.trim(), 200);
    let cakisan = c
        .query_opt(
            "SELECT 1 FROM kesinlesme_kurali WHERE takibin_turu = $1 AND takibin_yolu = $2 AND takibin_sekli = $3
                AND ($4::bigint IS NULL OR id <> $4)",
            &[&turu, &yolu, &sekli, &id],
        )
        .map_err(hata)?;
    if cakisan.is_some() {
        return Err("Bu takip kodu için zaten bir süre var; onu düzenleyin.".into());
    }
    let ad = veri::kirp(k.ad.trim(), 160);
    let elle = !veri::varsayilan_kural(turu, yolu, sekli)
        .is_some_and(|h| aktif && h.ayni_mi(&ad, gun, itiraz_gun, k.itiraz_durdurur, &dayanak));
    match id {
        Some(id) => {
            let n = c
                .execute(
                    "UPDATE kesinlesme_kurali SET takibin_turu = $2, takibin_yolu = $3, takibin_sekli = $4, ad = $5,
                            itiraz_suresi_gun = $6, aktif = $7, aciklama = $8, itiraz_gun = $9, itiraz_durdurur = $10,
                            dayanak = $11, elle_degisti = $12, guncelleme = now() WHERE id = $1",
                    &[&id, &turu, &yolu, &sekli, &ad, &gun, &aktif, &aciklama, &itiraz_gun, &k.itiraz_durdurur, &dayanak, &elle],
                )
                .map_err(hata)?;
            if n == 0 { Err("Kayıt bulunamadı.".into()) } else { Ok(id) }
        }
        None => Ok(c
            .query_one(
                "INSERT INTO kesinlesme_kurali (takibin_turu, takibin_yolu, takibin_sekli, ad, itiraz_suresi_gun, aktif, aciklama,
                                                itiraz_gun, itiraz_durdurur, dayanak, elle_degisti)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id",
                &[&turu, &yolu, &sekli, &ad, &gun, &aktif, &aciklama, &itiraz_gun, &k.itiraz_durdurur, &dayanak, &elle],
            )
            .map_err(hata)?
            .get(0)),
    }
}

/// Kuralın takip kodları.
fn kural_kodlari<C: GenericClient>(c: &mut C, id: i64) -> Result<(String, String, String), String> {
    c.query_opt("SELECT takibin_turu, takibin_yolu, takibin_sekli FROM kesinlesme_kurali WHERE id = $1", &[&id])
        .map_err(hata)?
        .map(|s| (s.get(0), s.get(1), s.get(2)))
        .ok_or_else(|| "Kayıt bulunamadı.".to_string())
}

/// Hazır kurallar silinmez (açılışta geri gelirdi): süre boş bırakılır ya da varsayılana döndürülür.
pub fn kural_sil<C: GenericClient>(c: &mut C, id: i64) -> Result<(), String> {
    let (t, y, s) = kural_kodlari(c, id)?;
    if veri::varsayilan_kural(&t, &y, &s).is_some() {
        return Err("Bu hazır bir kural, silinemez. Kesinleşme olmasın istiyorsanız süreyi boş bırakın ya da varsayılana döndürün.".into());
    }
    c.execute("DELETE FROM kesinlesme_kurali WHERE id = $1", &[&id]).map_err(hata)?;
    Ok(())
}

/// Hazır kuralı programdaki varsayılan değerlerine döndürür.
pub fn kural_varsayilana<C: GenericClient>(c: &mut C, id: i64) -> Result<(), String> {
    let (t, y, s) = kural_kodlari(c, id)?;
    let hazir = veri::varsayilan_kural(&t, &y, &s).ok_or("Bu kuralın hazır bir varsayılanı yok.")?;
    veri::varsayilana_dondur(c, id, hazir)
}

/// Program açılışında: tebligatları sabit gruplara taşır (eski ham türler, muhatap); değişen varsa bütün borçlular
/// yeniden hesaplanır (tür kesinleşmeyi etkiler: ör. "4-5 örnek icra emri" artık ödeme emri). Döner: değişen kayıt.
pub fn tebligat_gruplarini_uygula<C: GenericClient>(c: &mut C, bugun: NaiveDate, log: &dyn Fn(&str)) -> Result<usize, String> {
    let n = {
        let mut tx = c.transaction().map_err(hata)?;
        let n = durum::tebligat_alanlarini_duzelt(&mut tx, None)?;
        tx.commit().map_err(hata)?;
        n
    };
    if n > 0 {
        log(&format!("[VERİTABANI] Tebligat türleri gruplandı ({n} kayıt); borçlular yeniden hesaplanıyor."));
        durum::durumlari_guncelle(c, None, bugun, log)?;
    }
    Ok(n)
}

/// Haciz ihbarnamesinin 89 sırasını kullanıcı girer (belgede yazmıyorsa; tahmin yok). `None`: bilinmiyor.
pub fn haciz_sirasi_ata<C: GenericClient>(c: &mut C, tebligat: i64, sira: Option<i64>) -> Result<(), String> {
    let sira = match sira {
        Some(s @ 1..=3) => Some(s as i16),
        Some(_) => return Err("89 sırası 1, 2 ya da 3 olmalı.".into()),
        None => None,
    };
    let n = c
        .execute("UPDATE tebligat_barkod SET haciz_sirasi = $2::smallint, haciz_sirasi_elle = ($2::smallint IS NOT NULL) WHERE id = $1", &[&tebligat, &sira])
        .map_err(hata)?;
    if n == 0 { Err("Tebligat bulunamadı.".into()) } else { Ok(()) }
}

/// Program açılışında: hazır kesinleşme kurallarını yazar (elle değiştirilene dokunmaz) ve bir şey değiştiyse
/// bütün borçluları yeniden hesaplar. Döner: değişen kural sayısı.
pub fn varsayilan_kurallari_uygula<C: GenericClient>(c: &mut C, bugun: NaiveDate, log: &dyn Fn(&str)) -> Result<usize, String> {
    let n = {
        let mut tx = c.transaction().map_err(hata)?;
        let n = veri::varsayilan_kurallari_yaz(&mut tx)?;
        tx.commit().map_err(hata)?;
        n
    };
    if n > 0 {
        log(&format!("[VERİTABANI] Hazır kesinleşme kuralları yazıldı ({n} kural); borçlular yeniden hesaplanıyor."));
        durum::durumlari_guncelle(c, None, bugun, log)?;
    }
    Ok(n)
}

// ── Resmî tatiller ──────────────────────────────────────────────────────────────────────────────

pub fn tatiller<C: GenericClient>(c: &mut C) -> Result<Value, String> {
    let satirlar = c.query("SELECT tarih, aciklama FROM resmi_tatil ORDER BY tarih", &[]).map_err(hata)?;
    Ok(Value::Array(satirlar.iter().map(|s| json!({"tarih": gun(s.get(0)), "aciklama": s.get::<_, String>(1)})).collect()))
}

pub fn tatil_kaydet<C: GenericClient>(c: &mut C, tarih: NaiveDate, aciklama: &str) -> Result<(), String> {
    c.execute(
        "INSERT INTO resmi_tatil (tarih, aciklama) VALUES ($1, $2) ON CONFLICT (tarih) DO UPDATE SET aciklama = EXCLUDED.aciklama",
        &[&tarih, &veri::kirp(aciklama.trim(), 120)],
    )
    .map_err(hata)?;
    Ok(())
}

pub fn tatil_sil<C: GenericClient>(c: &mut C, tarih: NaiveDate) -> Result<(), String> {
    c.execute("DELETE FROM resmi_tatil WHERE tarih = $1", &[&tarih]).map_err(hata)?;
    Ok(())
}

// ── E-tebliğ mazbatası (dosya içinden elle) ─────────────────────────────────────────────────────

/// Görüntüleyicide açık "Kapalı E-Tebliğ Mazbatası"nın yazı katmanından okunanlar. Evrak listesinde bu belgenin
/// açıklaması YOK (barkod yalnız belgenin içinde; canlı 20.11.2026, İzmir 7. İcra 2021/15056).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EMazbata {
    pub barkod: Option<String>,
    /// "UETS Hesap Sahibi: X" (yoksa "Üçüncü Şahıs X" / "Borçlu X" satırı).
    pub hesap_sahibi: String,
    /// İlk UETS olayının günü (tebligatın hesabına konulduğu gün = tebligat evrakının tarihi).
    pub ilk_olay: Option<NaiveDate>,
    /// Son olay: gün ve metni ("Tebligat, alıcının hesabına … okundu sayıldı.").
    pub son_olay: Option<(NaiveDate, String)>,
}

pub fn e_mazbata_coz(satirlar: &[String]) -> EMazbata {
    static TARIH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let tarih_re = TARIH.get_or_init(|| regex::Regex::new(r"^(\d{2})/(\d{2})/(\d{4})(\s+\d{2}:\d{2})?$").unwrap());
    let mut m = EMazbata::default();
    let satirlar: Vec<String> = satirlar.iter().map(|x| x.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|x| !x.is_empty() && x != ":").collect();
    let mut olaylar: Vec<(NaiveDate, String)> = Vec::new();
    let mut i = 0;
    while i < satirlar.len() {
        let t = &satirlar[i];
        if m.barkod.is_none() {
            if let Some(b) = t.strip_prefix('*').and_then(|x| x.strip_suffix('*')).map(|x| x.replace(' ', "")) {
                if b.len() == 13 && b.chars().all(|c| c.is_ascii_digit()) {
                    m.barkod = Some(b);
                }
            }
        }
        if let Some((_, ad)) = t.split_once("Hesap Sahibi:") {
            m.hesap_sahibi = ad.trim().to_string();
        } else if m.hesap_sahibi.is_empty() {
            for on in ["Üçüncü Şahıs ", "Borçlu ", "BORÇLU "] {
                if let Some(ad) = t.strip_prefix(on) {
                    m.hesap_sahibi = ad.trim().to_string();
                }
            }
        }
        if let Some(c) = tarih_re.captures(t) {
            if let Some(g) = NaiveDate::from_ymd_opt(c[3].parse().unwrap_or(0), c[2].parse().unwrap_or(0), c[1].parse().unwrap_or(0)) {
                // Olay metni: sonraki tarih satırına ya da başlığa kadar
                let mut metin = Vec::new();
                let mut j = i + 1;
                while j < satirlar.len() && !tarih_re.is_match(&satirlar[j]) && !satirlar[j].starts_with("E-TEBLİĞ") && metin.len() < 4 {
                    metin.push(satirlar[j].clone());
                    j += 1;
                }
                olaylar.push((g, metin.join(" ")));
                i = j;
                continue;
            }
        }
        i += 1;
    }
    m.ilk_olay = olaylar.first().map(|o| o.0);
    m.son_olay = olaylar.pop();
    m
}

/// Elle bağlanabilen (fiziki) mazbata evrak türleri.
pub const BAGLANABILIR_MAZBATALAR: [&str; 3] = [teb::TEBLIG_MAZBATASI_TUR, teb::MTS_TEBLIG_MAZBATASI_TUR, teb::KAPALI_TEBLIG_MAZBATASI_TUR];

/// Mazbatayı (evrak no) tebligata (evrak no) elle bağlar; ikisinin eski bağları düşer. `mazbata` None: tebligatın elle
/// bağı kaldırılır (otomatik eşleşmeye döner). Sonra dosyanın mazbata eşleşmesi ve durumları yeniden kurulur.
pub fn mazbata_bagla<C: GenericClient>(c: &mut C, dosya: i64, tebligat: i32, mazbata: Option<i32>, bugun: NaiveDate) -> Result<Value, String> {
    mazbata_bagla_tarihli(c, dosya, tebligat, mazbata, None, bugun)
}

/// [`mazbata_bagla`] + mazbatadaki tebliğ tarihi (`teblig_tarihi`): PTT'de tebliğ günü olmayan (eski, silinmiş barkod)
/// tebligatta kesinleşme bu günden hesaplanır. Bağ kaldırılırken yok sayılır.
pub fn mazbata_bagla_tarihli<C: GenericClient>(
    c: &mut C,
    dosya: i64,
    tebligat: i32,
    mazbata: Option<i32>,
    teblig_tarihi: Option<NaiveDate>,
    bugun: NaiveDate,
) -> Result<Value, String> {
    if teblig_tarihi.is_some_and(|t| t > bugun) {
        return Err("Tebliğ tarihi bugünden sonra olamaz.".into());
    }
    let fiziki: Option<bool> = c
        .query_opt("SELECT elektronik_tebligat FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = $2", &[&dosya, &tebligat])
        .map_err(hata)?
        .map(|s| !s.get::<_, bool>(0));
    match fiziki {
        None => return Err("Tebligat bu dosyada bulunamadı.".into()),
        Some(false) if mazbata.is_some() => return Err("E-tebligata fiziki mazbata bağlanmaz.".into()),
        _ => {}
    }
    let mut tx = c.transaction().map_err(hata)?;
    tx.execute("DELETE FROM tebligat_mazbata_bagi WHERE dosya_id = $1 AND tebligat_evrak_no = $2", &[&dosya, &tebligat]).map_err(hata)?;
    if let Some(mz) = mazbata {
        let turler: Vec<String> = BAGLANABILIR_MAZBATALAR.iter().map(|t| t.to_string()).collect();
        let var = tx
            .query_opt("SELECT 1 FROM evrak WHERE dosya_id = $1 AND birim_evrak_no = $2 AND evrak_turu = ANY($3)", &[&dosya, &mz, &turler])
            .map_err(hata)?
            .is_some();
        if !var {
            return Err("Mazbata evrakı bu dosyada bulunamadı.".into());
        }
        tx.execute("DELETE FROM tebligat_mazbata_bagi WHERE dosya_id = $1 AND mazbata_evrak_no = $2", &[&dosya, &mz]).map_err(hata)?;
        tx.execute(
            "INSERT INTO tebligat_mazbata_bagi (dosya_id, tebligat_evrak_no, mazbata_evrak_no, teblig_tarihi) VALUES ($1, $2, $3, $4)",
            &[&dosya, &tebligat, &mz, &teblig_tarihi],
        )
        .map_err(hata)?;
    }
    tx.commit().map_err(hata)?;
    let degisen = crate::icra_tarama::mazbatalari_yeniden_bagla(c, dosya, bugun)?;
    Ok(json!({ "degisen": degisen }))
}

/// UYAP dosya penceresinde (evrak ağacı / belge görüntüleyici) mazbataya sağ tık: mazbata evrakını (UYAP'ın evrak
/// nesnesi: birimEvrakNo, tur, aciklama, onaylandigiTarih) büro veritabanına yazar — yeni taranan mazbatayı tarama
/// henüz görmemiş olabilir — ve bağlanabilecek PTT tebligatlarını döner. Bağın kendisi [`mazbata_bagla`] ile kurulur.
pub fn mazbata_bag_secenekleri<C: GenericClient>(c: &mut C, dosya: i64, evrak: &Value) -> Result<Value, String> {
    let tur = teb::metin(evrak, "tur");
    if !BAGLANABILIR_MAZBATALAR.contains(&tur.as_str()) {
        return Err(format!("\"{tur}\" tebligata elle bağlanan bir mazbata türü değil."));
    }
    let no = crate::icra_kayit::birim_evrak_no(evrak).ok_or("Mazbatanın evrak numarası okunamadı.")?;
    // Yalnız yoksa eklenir: ağaçtaki evrak nesnesi açıklama taşımayabilir, taramanın yazdığı açıklama (barkod) ezilmez.
    let tarih = evrak.get("onaylandigiTarih").and_then(crate::icra_veri::tarih::coz);
    c.execute(
        "INSERT INTO evrak (dosya_id, birim_evrak_no, evrak_turu, aciklama, evrak_tarihi) VALUES ($1, $2, $3, $4, $5::timestamp)
         ON CONFLICT ON CONSTRAINT uq_dosya_evrak DO NOTHING",
        &[&dosya, &no, &tur, &crate::icra_veri::kirp(&teb::metin(evrak, "aciklama"), 500), &tarih],
    )
    .map_err(hata)?;
    let v = dosya_ayrintisi(c, dosya)?.ok_or("Dosya bulunamadı.")?;
    let mazbata = v["mazbatalar"].as_array().and_then(|m| m.iter().find(|m| m["evrak_no"].as_i64() == Some(no as i64))).cloned();
    let tebligatlar: Vec<Value> = v["tebligatlar"].as_array().map(|t| t.iter().filter(|t| t["elektronik"] != json!(true)).cloned().collect()).unwrap_or_default();
    Ok(json!({"dosya": dosya, "mazbata": mazbata.ok_or("Mazbata evrakı kaydedilemedi.")?, "tebligatlar": tebligatlar}))
}

/// Mazbatayı dosyanın bir tebligatına işler (UYAP'a istek yok). Eşleşme: (1) barkod; (2) barkod tutmazsa ilk UETS olayı
/// günü = tebligat evrak günü olan ve muhatabı hesap sahibiyle uyuşan TEK e-tebligat; (3) yoksa `secilen` (kullanıcı
/// adaylardan seçer; tahmin yok). Döner: işlendiyse tebligat bilgisi, değilse aday listesi.
pub fn e_mazbata_isle<C: GenericClient>(
    c: &mut C,
    dosya: i64,
    belge: &str,
    satirlar: &[String],
    secilen: Option<i64>,
    bugun: NaiveDate,
) -> Result<Value, String> {
    let m = e_mazbata_coz(satirlar);
    let adaylar: Vec<(i64, i32, String, String, String, Option<NaiveDate>, bool)> = c
        .query(
            "SELECT id, birim_evrak_no, tebligat_turu, evrak_aciklama, barkod, evrak_gunu, elektronik_tebligat
               FROM tebligat_barkod WHERE dosya_id = $1 ORDER BY evrak_gunu DESC NULLS LAST, birim_evrak_no DESC",
            &[&dosya],
        )
        .map_err(hata)?
        .iter()
        .map(|s| (s.get(0), s.get(1), s.get(2), s.get(3), s.get(4), s.get(5), s.get(6)))
        .collect();
    if adaylar.is_empty() {
        return Err("Dosyada kayıtlı tebligat yok. Önce \"Kapalı Tebligat\" belgesini dosya tebligatlarına kaydedin.".into());
    }
    let muhatap_uyar = |aciklama: &str| {
        let sahip = tr_kucuk(&m.hesap_sahibi);
        !sahip.is_empty() && tr_kucuk(&teb::tebligat_muhatabi(aciklama).0).contains(&sahip)
    };
    let (secim, eslesme) = if let Some(id) = secilen {
        (adaylar.iter().find(|a| a.0 == id), "secim")
    } else if let Some(a) = m.barkod.as_ref().and_then(|b| adaylar.iter().find(|a| &a.4 == b)) {
        (Some(a), "barkod")
    } else {
        let uyan: Vec<_> = adaylar.iter().filter(|a| a.6 && m.ilk_olay.is_some() && a.5 == m.ilk_olay && muhatap_uyar(&a.3)).collect();
        if uyan.len() == 1 { (Some(uyan[0]), "muhatap_tarih") } else { (None, "") }
    };
    let Some(t) = secim else {
        // Aday: e-tebligatlar (aynı gün olanlar önde)
        let mut liste: Vec<_> = adaylar.iter().filter(|a| a.6).collect();
        liste.sort_by_key(|a| (a.5 != m.ilk_olay, std::cmp::Reverse(a.5)));
        return Ok(json!({
            "secim_gerekli": true,
            "neden": match &m.barkod {
                Some(b) => format!("Mazbatadaki barkod ({b}) dosyadaki tebligatlarla uyuşmadı; muhatap ve tarihle de tek bir tebligat bulunamadı."),
                None => "Mazbatada barkod okunamadı.".to_string(),
            },
            "mazbata": {"barkod": m.barkod, "hesap_sahibi": m.hesap_sahibi, "ilk_olay": gun(m.ilk_olay)},
            "adaylar": liste.iter().take(20).map(|a| json!({"id": a.0, "evrak_no": a.1, "tur": a.2, "muhatap": teb::tebligat_muhatabi(&a.3).0,
                                                         "barkod": a.4, "evrak_tarihi": gun(a.5)})).collect::<Vec<_>>(),
        }));
    };
    // Açıklamada tebligatın KENDİ barkodu durur (hesap mazbatayı barkodla tanır); farklıysa mazbatanınki de yazılır.
    let son = m.son_olay.as_ref().map(|(g, x)| format!(" - {}: {x}", g.format("%d/%m/%Y"))).unwrap_or_default();
    let farkli = m.barkod.as_ref().filter(|b| **b != t.4).map(|b| format!(" (mazbatadaki barkod {b})")).unwrap_or_default();
    let aciklama = veri::kirp(&format!("{} - Barkod Numarası: {}{farkli}{son}", belge.trim(), t.4), 500);
    c.execute(
        "UPDATE tebligat_barkod SET tebligat_mazbatasi_var = true, tebligat_mazbatasi_aciklama = $2 WHERE id = $1",
        &[&t.0, &aciklama],
    )
    .map_err(hata)?;
    let s = durum::durumlari_guncelle(c, Some(dosya), bugun, &|_| {})?;
    Ok(json!({
        "islendi": true, "eslesme": eslesme, "guncellenen": s.guncellenen,
        "tebligat": {"id": t.0, "evrak_no": t.1, "tur": t.2, "muhatap": teb::tebligat_muhatabi(&t.3).0, "barkod": t.4,
                     "evrak_tarihi": gun(t.5)},
        "mazbata": {"barkod": m.barkod, "hesap_sahibi": m.hesap_sahibi, "son_olay": m.son_olay.as_ref().map(|(g, x)| json!({"tarih": gun(Some(*g)), "metin": x}))},
    }))
}

/// Kart künyesinden (UYAP birim kimliği + dosya no + tür kodu) veritabanındaki icra dosyası.
pub fn kunyeden_dosya<C: GenericClient>(c: &mut C, birim_id: &str, dosya_no: &str, tur_kod: i32) -> Result<i64, String> {
    c.query_opt(
        "SELECT d.id FROM dosya d JOIN birim b ON b.id = d.birim_id
          WHERE b.birim_id = $1 AND d.dosya_no = $2 AND d.yargi_turu = 2 ORDER BY (d.tur_kod = $3) DESC, d.id DESC LIMIT 1",
        &[&birim_id, &dosya_no.trim(), &tur_kod],
    )
    .map_err(hata)?
    .map(|s| s.get(0))
    .ok_or_else(|| format!("{} büro veritabanında yok. Önce İcra Süreçleri'nden dosya listesini güncelleyin.", dosya_no.trim()))
}

// ── Elle girilenler ─────────────────────────────────────────────────────────────────────────────

fn bagin_dosyasi<C: GenericClient>(c: &mut C, bag: i64) -> Result<i64, String> {
    c.query_opt("SELECT dosya_id FROM dosya_taraf WHERE id = $1 AND rol = 'borclu'", &[&bag])
        .map_err(hata)?
        .map(|s| s.get(0))
        .ok_or_else(|| "Borçlu kaydı bulunamadı.".to_string())
}

/// Elle itiraz işareti (UYAP'tan otomatik anlaşılamıyor). Ardından borçlunun durumu yeniden
/// hesaplanır; değişiklik geçmişe yazılır.
pub fn itiraz_ata<C: GenericClient>(c: &mut C, bag: i64, edildi: bool, tarih: Option<NaiveDate>, bugun: NaiveDate) -> Result<(), String> {
    let dosya = bagin_dosyasi(c, bag)?;
    let tarih = if edildi { tarih } else { None };
    c.execute("UPDATE dosya_taraf SET itiraz_edildi_mi = $2, itiraz_tarihi = $3 WHERE id = $1", &[&bag, &edildi, &tarih]).map_err(hata)?;
    durum::durumlari_guncelle(c, Some(dosya), bugun, &|_| {}).map(|_| ())
}

/// Elle girilen kesinleşme değeri: Konkordato, Vefat, Vasi, Tehir-i İcra (hesap bunları ezmez) ya da boş
/// (yeniden hesaba bırak).
pub fn kesinlesme_elle<C: GenericClient>(c: &mut C, bag: i64, yeni: &str, bugun: NaiveDate) -> Result<(), String> {
    let izinli = [KesinlesmeDurumu::Konkordato.kod(), KesinlesmeDurumu::Vefat.kod(), KesinlesmeDurumu::Vasi.kod(), KesinlesmeDurumu::TehiriIcra.kod(), ""];
    if !izinli.contains(&yeni) {
        return Err("Elle yalnız Konkordato, Vefat, Vasi ya da Tehir-i İcra girilebilir.".into());
    }
    let dosya = bagin_dosyasi(c, bag)?;
    let mut tx = c.transaction().map_err(hata)?;
    let s = tx.query_one("SELECT kesinlesme_durumu, kesinlesme_tarihi FROM dosya_taraf WHERE id = $1 FOR UPDATE", &[&bag]).map_err(hata)?;
    let (eski, eski_t): (String, Option<NaiveDate>) = (s.get(0), s.get(1));
    if !yeni.is_empty() {
        // Konkordato / vefat / vasi elle kesinleşme gününün yerini alır.
        tx.execute("UPDATE dosya_taraf SET kesinlesme_elle_tarihi = NULL WHERE id = $1", &[&bag]).map_err(hata)?;
    }
    if eski != yeni {
        tx.execute("UPDATE dosya_taraf SET kesinlesme_durumu = $2, kesinlesme_tarihi = NULL WHERE id = $1", &[&bag, &yeni]).map_err(hata)?;
        if !yeni.is_empty() {
            durum_olayi_yaz(
                &mut tx,
                &YeniOlay {
                    dosya,
                    dosya_taraf: Some(bag),
                    tur: OlayTur::Kesinlesme,
                    eski: &durum::kesinlesme_metni(&eski, eski_t),
                    yeni: &durum::kesinlesme_metni(yeni, None),
                    olay_tarihi: None,
                    kaynak: OlayKaynak::Elle,
                    aciklama: "",
                },
            )?;
        }
    }
    durum::durumlari_guncelle(&mut tx, Some(dosya), bugun, &|_| {})?;
    tx.commit().map_err(hata)
}

/// Elle kesinleşme günü (kullanıcının UYAP'ta gördüğü). Gün gelmişse Kesinleşti, gelmemişse o güne Kesinleşme
/// Bekliyor; hesap ezmez. `None`: hesaba bırak. Etkili itiraz ya da durdurma varsa gün saklanır ama onlar önce
/// gelir. Dönen: borçlunun kayıttan sonraki kesinleşme metni (arayüz gösterir).
pub fn kesinlesme_tarihi_elle<C: GenericClient>(c: &mut C, bag: i64, gun: Option<NaiveDate>, bugun: NaiveDate) -> Result<String, String> {
    if gun.is_some_and(|g| g > bugun + chrono::Duration::days(400)) {
        return Err("Kesinleşme günü bir yıldan ileri olamaz.".into());
    }
    let dosya = bagin_dosyasi(c, bag)?;
    let mut tx = c.transaction().map_err(hata)?;
    let s = tx
        .query_one("SELECT kesinlesme_durumu, kesinlesme_tarihi, kesinlesme_elle_tarihi FROM dosya_taraf WHERE id = $1 FOR UPDATE", &[&bag])
        .map_err(hata)?;
    let (eski, eski_t, eski_elle): (String, Option<NaiveDate>, Option<NaiveDate>) = (s.get(0), s.get(1), s.get(2));
    if eski_elle != gun {
        // Konkordato / vefat / vasi işaretini kaldır: kullanıcı artık gün giriyor.
        let elle = [KesinlesmeDurumu::Konkordato.kod(), KesinlesmeDurumu::Vefat.kod(), KesinlesmeDurumu::Vasi.kod(), KesinlesmeDurumu::TehiriIcra.kod()];
        let (yeni, yeni_t) = match gun {
            Some(g) => (durum::elle_kesinlesme(g, bugun).to_string(), Some(g)),
            None => (if elle.contains(&eski.as_str()) { String::new() } else { eski.clone() }, eski_t),
        };
        tx.execute(
            "UPDATE dosya_taraf SET kesinlesme_elle_tarihi = $2,
                    kesinlesme_durumu = CASE WHEN $2::date IS NULL THEN $3 ELSE kesinlesme_durumu END
              WHERE id = $1",
            &[&bag, &gun, &yeni],
        )
        .map_err(hata)?;
        if gun.is_some() {
            // Olay elle kaynakla yazılır; durumu da burada yazarız ki hesap aynı değişikliği ikinci kez kaydetmesin.
            tx.execute("UPDATE dosya_taraf SET kesinlesme_durumu = $2, kesinlesme_tarihi = $3 WHERE id = $1", &[&bag, &yeni, &yeni_t])
                .map_err(hata)?;
            if (&eski, eski_t) != (&yeni, yeni_t) {
                durum_olayi_yaz(
                    &mut tx,
                    &YeniOlay {
                        dosya,
                        dosya_taraf: Some(bag),
                        tur: OlayTur::Kesinlesme,
                        eski: &durum::kesinlesme_metni(&eski, eski_t),
                        yeni: &durum::kesinlesme_metni(&yeni, yeni_t),
                        olay_tarihi: yeni_t,
                        kaynak: OlayKaynak::Elle,
                        aciklama: "Kesinleşme günü elle girildi",
                    },
                )?;
            }
        }
    }
    durum::durumlari_guncelle(&mut tx, Some(dosya), bugun, &|_| {})?;
    let s = tx.query_one("SELECT kesinlesme_durumu, kesinlesme_tarihi FROM dosya_taraf WHERE id = $1", &[&bag]).map_err(hata)?;
    let metin = durum::kesinlesme_metni(&s.get::<_, String>(0), s.get(1));
    tx.commit().map_err(hata)?;
    Ok(metin)
}

// ── HTTP ────────────────────────────────────────────────────────────────────────────────────────

type Yanit = (u16, HashMap<String, String>, Vec<u8>);

fn yanit(durum: u16, govde: Value) -> Yanit {
    let mut h = HashMap::new();
    h.insert("Content-Type".to_string(), "application/json; charset=utf-8".to_string());
    h.insert("Cache-Control".to_string(), "no-store".to_string());
    (durum, h, serde_json::to_vec(&govde).unwrap_or_else(|_| b"{}".to_vec()))
}

fn tam(v: &Value, anahtar: &str) -> Result<i64, String> {
    teb::tam(v, anahtar).ok_or_else(|| format!("'{anahtar}' eksik."))
}

fn sorgu_tam(q: &HashMap<String, String>, anahtar: &str) -> Result<i64, String> {
    q.get(anahtar).and_then(|v| v.trim().parse().ok()).ok_or_else(|| format!("'{anahtar}' eksik."))
}

/// Liste süzgeçleri (sorgu dizesinden).
pub(crate) fn filtre_oku(q: &HashMap<String, String>) -> Filtre {
    Filtre {
        q: q.get("q").cloned().unwrap_or_default(),
        asama: q.get("asama").cloned().unwrap_or_default(),
        liste: crate::icra_listeler::liste_oku(q),
        // `acik=1` eski istemciler için (yalnız açık); yeni istemci `durum` gönderir.
        durum: q.get("durum").cloned().unwrap_or_else(|| if q.get("acik").is_some_and(|v| v == "1") { "acik".into() } else { String::new() }),
        birim: q.get("birim").cloned().unwrap_or_default(),
        dosya_no: q.get("dosya_no").cloned().unwrap_or_default(),
        alacakli: q.get("alacakli").cloned().unwrap_or_default(),
        borclu: q.get("borclu").cloned().unwrap_or_default(),
        yil: q.get("yil").cloned().unwrap_or_default(),
        dosya_tur: q.get("dosya_tur").cloned().unwrap_or_default(),
        acilis_bas: q.get("acilis_bas").and_then(|v| veri::tarih::gun_coz(v)),
        acilis_son: q.get("acilis_son").and_then(|v| veri::tarih::gun_coz(v)),
        tur: q.get("tur").cloned().unwrap_or_default(),
        teblig: q.get("teblig").cloned().unwrap_or_default(),
        mazbata: q.get("mazbata").cloned().unwrap_or_default(),
        kesinlesme: q.get("kesinlesme").cloned().unwrap_or_default(),
        sayfa: q.get("sayfa").and_then(|v| v.parse().ok()).unwrap_or(1),
        boyut: q.get("boyut").and_then(|v| v.parse().ok()).unwrap_or(50),
    }
}

/// Gövdedeki `dosyalar` (seçili dosya kimlikleri); yoksa None.
pub fn dosya_listesi(g: &Value) -> Result<Option<Vec<i64>>, String> {
    match g.get("dosyalar") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) if a.len() <= SECIM_EN_COK => a
            .iter()
            .map(|x| x.as_i64().filter(|d| *d > 0))
            .collect::<Option<Vec<i64>>>()
            .map(Some)
            .ok_or_else(|| "Dosya listesi geçersiz.".to_string()),
        Some(_) => Err(format!("Dosya listesi geçersiz (en çok {SECIM_EN_COK} dosya).")),
    }
}

/// Seçili dosyalara işlemde en çok dosya.
pub const SECIM_EN_COK: usize = 50_000;
/// Seçim bundan küçükse "Yeniden hesapla" yalnız seçili dosyaları tek tek hesaplar; büyükse hepsini tek geçişte
/// (yerel iş; seçili dosyalar için sonuç aynı).
const HESAP_TEK_TEK_EN_COK: usize = 500;

/// İsteği veritabanı bağlantısıyla işler. `Ok(None)` = böyle bir uç yok.
fn dagit(c: &mut postgres::Client, yontem: &str, uc: &str, q: &HashMap<String, String>, g: &Value) -> Result<Option<Value>, String> {
    let bugun = chrono::Local::now().date_naive();
    let metin = |a: &str| teb::metin(g, a);
    let evet = |a: &str| g.get(a).and_then(Value::as_bool).unwrap_or(false);
    Ok(Some(match (yontem, uc) {
        ("GET", "ozet") => ozet(c)?,
        ("GET", "listeler") => crate::icra_listeler::listeler(c)?,
        ("POST", "liste") => json!({"id": crate::icra_listeler::liste_kaydet(c, teb::tam(g, "id"), &metin("ad"), &metin("aciklama"))?}),
        ("POST", "liste_sil") => {
            crate::icra_listeler::liste_sil(c, tam(g, "id")?)?;
            Value::Null
        }
        ("POST", "liste_ekle") => crate::icra_listeler::liste_ekle(c, tam(g, "id")?, &dosya_listesi(g)?.ok_or("Dosya seçilmedi.")?)?,
        ("POST", "liste_cikar") => crate::icra_listeler::liste_cikar(c, tam(g, "id")?, &dosya_listesi(g)?.ok_or("Dosya seçilmedi.")?)?,
        ("GET", "borclular") => borclu_listesi(c, &filtre_oku(q))?,
        ("GET", "dosya_kimlikleri") => json!(suzulen_dosyalar(c, &filtre_oku(q))?),
        ("GET", "dosya") => dosya_ayrintisi(c, sorgu_tam(q, "id")?)?.ok_or("Dosya bulunamadı.")?,
        ("GET", "gecmis") => gecmis(c, sorgu_tam(q, "dosya")?)?,
        ("GET", "ptt") => ptt_hareketleri(c, sorgu_tam(q, "tebligat")?)?,
        ("GET", "kurallar") => kurallar(c)?,
        ("GET", "tebligatlar") => teb_liste::tebligat_listesi(c, &teb_liste::Filtre::oku(q))?,
        ("GET", "tebligat_dosya_kimlikleri") => json!(teb_liste::tebligat_dosyalari(c, &teb_liste::Filtre::oku(q))?),
        ("GET", "tur_eslestirme") => teb_liste::tur_eslestirme(c)?,
        ("GET", "dosyalar") => dosya_liste::dosya_listesi(c, &dosya_liste::Filtre::oku(q))?,
        ("GET", "dosyalar_kimlikleri") => {
            json!(dosya_liste::dosya_kimlikleri(c, &dosya_liste::Filtre::oku(q), q.get("mod").map(String::as_str).unwrap_or(""))?)
        }
        ("GET", "dosyalar_istek") => {
            dosya_liste::istek_tahmini(c, &dosya_liste::Filtre::oku(q), q.get("mod").map(String::as_str).unwrap_or(""))?
        }
        ("GET", "kesinlestirme") => kesin::kesinlestirme_listesi(c, &kesin::Filtre::oku(q), bugun)?,
        ("GET", "kesinlestirme_kimlikleri") => {
            let (dosyalar, borclular) = kesin::kimlikler(c, &kesin::Filtre::oku(q), q.get("is").map(String::as_str).unwrap_or(""), bugun)?;
            json!({"dosyalar": dosyalar, "borclular": borclular})
        }
        ("GET", "kesinlestirme_istek") => {
            kesin::istek_tahmini(c, &kesin::Filtre::oku(q), q.get("is").map(String::as_str).unwrap_or(""), bugun)?
        }
        ("POST", "tur_eslestir") => {
            let liste: Vec<(String, String)> = g
                .get("eslesmeler")
                .and_then(Value::as_array)
                .ok_or("Eşleştirme listesi eksik.")?
                .iter()
                .map(|x| (teb::metin(x, "anahtar"), teb::metin(x, "tur")))
                .collect();
            let (eslesme, degisen) = teb_liste::tur_eslestir(c, &liste)?;
            // Tür kesinleşmeyi değiştirir (ör. "Ödeme Emri"): bütün borçlular yeniden hesaplanır.
            let s = durum::durumlari_guncelle(c, None, bugun, &|_| {})?;
            json!({"eslesme": eslesme, "degisen": degisen, "guncellenen": s.guncellenen})
        }
        ("GET", "tatiller") => tatiller(c)?,
        ("POST", "kural") => {
            let id = kural_kaydet(c, &KuralGirdisi::oku(g))?;
            let s = durum::durumlari_guncelle(c, None, bugun, &|_| {})?;
            json!({"id": id, "guncellenen": s.guncellenen})
        }
        ("POST", "kural_varsayilan") => {
            kural_varsayilana(c, tam(g, "id")?)?;
            let s = durum::durumlari_guncelle(c, None, bugun, &|_| {})?;
            json!({"guncellenen": s.guncellenen})
        }
        ("POST", "kural_sil") => {
            kural_sil(c, tam(g, "id")?)?;
            let s = durum::durumlari_guncelle(c, None, bugun, &|_| {})?;
            json!({"guncellenen": s.guncellenen})
        }
        ("POST", "tatil") => {
            tatil_kaydet(c, gun_coz(&g["tarih"])?.ok_or("Tarih eksik.")?, &metin("aciklama"))?;
            let s = durum::durumlari_guncelle(c, None, bugun, &|_| {})?;
            json!({"guncellenen": s.guncellenen})
        }
        ("POST", "tatil_sil") => {
            tatil_sil(c, gun_coz(&g["tarih"])?.ok_or("Tarih eksik.")?)?;
            let s = durum::durumlari_guncelle(c, None, bugun, &|_| {})?;
            json!({"guncellenen": s.guncellenen})
        }
        ("POST", "haciz_sirasi") => {
            haciz_sirasi_ata(c, tam(g, "tebligat")?, g.get("sira").and_then(Value::as_i64))?;
            Value::Null
        }
        ("POST", "mazbata_isle") => {
            let dosya = kunyeden_dosya(c, &metin("birim_id"), &metin("dosya_no"), g.get("dosya_tur_kod").and_then(Value::as_i64).unwrap_or(-1) as i32)?;
            let satirlar: Vec<String> = g.get("metin").and_then(Value::as_array).ok_or("Belge yazısı eksik.")?
                .iter().take(2000).filter_map(|x| x.as_str().map(|x| x.chars().take(500).collect())).collect();
            e_mazbata_isle(c, dosya, &metin("belge"), &satirlar, teb::tam(g, "tebligat"), bugun)?
        }
        ("POST", "mazbata_bag_secenek") => {
            let dosya = kunyeden_dosya(c, &metin("birim_id"), &metin("dosya_no"), g.get("dosya_tur_kod").and_then(Value::as_i64).unwrap_or(-1) as i32)?;
            mazbata_bag_secenekleri(c, dosya, g.get("evrak").ok_or("Mazbata evrakı eksik.")?)?
        }
        ("POST", "mazbata_bagla") => {
            let no = |a: &str| g.get(a).and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0);
            mazbata_bagla_tarihli(c, tam(g, "dosya")?, no("tebligat").ok_or("Tebligat seçilmedi.")?, no("mazbata"),
                                  gun_coz(g.get("teblig_tarihi").unwrap_or(&Value::Null))?, bugun)?
        }
        ("POST", "itiraz") => {
            itiraz_ata(c, tam(g, "bag")?, evet("edildi"), gun_coz(&g["tarih"])?, bugun)?;
            Value::Null
        }
        ("POST", "kesinlesme_elle") => {
            kesinlesme_elle(c, tam(g, "bag")?, &metin("durum"), bugun)?;
            Value::Null
        }
        ("POST", "kesinlesme_tarihi") => json!({ "kesinlesme": kesinlesme_tarihi_elle(c, tam(g, "bag")?, gun_coz(&g["tarih"])?, bugun)? }),
        ("POST", "islem_durumu") => {
            let d = IslemDurumu::coz(&metin("durum")).ok_or("Geçersiz işlem durumu.")?;
            veri::islem_durumu_ata(c, tam(g, "dosya")?, d, &metin("notu"), gun_coz(&g["bekleme_bitis"])?)?;
            Value::Null
        }
        ("POST", "hesapla") => match dosya_listesi(g)? {
            Some(idler) if idler.len() <= HESAP_TEK_TEK_EN_COK => {
                let (mut guncellenen, mut olay) = (0, 0);
                for d in &idler {
                    let s = durum::durumlari_guncelle(c, Some(*d), bugun, &|_| {})?;
                    guncellenen += s.guncellenen;
                    olay += s.olay;
                }
                json!({"guncellenen": guncellenen, "olay": olay, "dosya_sayisi": idler.len()})
            }
            secim => {
                let s = durum::durumlari_guncelle(c, teb::tam(g, "dosya"), bugun, &|_| {})?;
                json!({"guncellenen": s.guncellenen, "olay": s.olay, "dosya_sayisi": secim.map(|x| x.len())})
            }
        },
        _ => return Ok(None),
    }))
}

/// `/__cu__/veri/…` isteği. `capraz_site`: tarayıcı isteğin başka bir siteden geldiğini bildirdi
/// (yazma istekleri o zaman reddedilir).
pub async fn handle(method: &str, path: &str, query: &str, body: &[u8], capraz_site: bool) -> Yanit {
    let yontem = method.trim().to_uppercase();
    let uc = path.trim_start_matches('/').trim_start_matches(ON_EK).trim_matches('/').to_string();
    if yontem != "GET" && capraz_site {
        return yanit(403, json!({"ok": false, "error": "Bu istek başka bir siteden geldi; reddedildi."}));
    }
    if !crate::veritabani::hazir() {
        return yanit(503, json!({"ok": false, "kod": "E-402", "error": "Büro veritabanı hazır değil."}));
    }
    let q: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    // Excel: ikili yanıt (JSON zarfı yok); dosya istemcinin tarayıcısında indirilir.
    if yontem == "GET" && uc == "tebligatlar_excel" {
        let sonuc = tokio::task::spawn_blocking(move || {
            let mut c = crate::veritabani::baglanti()?;
            let c: &mut postgres::Client = &mut c;
            teb_liste::tebligat_excel(c, &teb_liste::Filtre::oku(&q))
        })
        .await;
        return match sonuc {
            Ok(Ok(bayt)) => {
                let mut h = HashMap::new();
                h.insert("Content-Type".to_string(), "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string());
                let ad = format!("Tebligatlar_{}.xlsx", chrono::Local::now().format("%Y%m%d_%H%M"));
                h.insert("Content-Disposition".to_string(), format!("attachment; filename=\"{ad}\""));
                h.insert("Cache-Control".to_string(), "no-store".to_string());
                (200, h, bayt)
            }
            Ok(Err(e)) => yanit(500, json!({"ok": false, "error": e})),
            Err(e) => yanit(500, json!({"ok": false, "error": format!("İşlem tamamlanamadı: {e}")})),
        };
    }
    let g: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => return yanit(400, json!({"ok": false, "error": "İstek gövdesi JSON değil."})),
        }
    };
    let sonuc = tokio::task::spawn_blocking(move || {
        let mut c = crate::veritabani::baglanti()?;
        dagit(&mut c, &yontem, &uc, &q, &g)
    })
    .await;
    match sonuc {
        Ok(Ok(Some(veri))) => yanit(200, json!({"ok": true, "veri": veri})),
        Ok(Ok(None)) => yanit(404, json!({"ok": false, "error": "Böyle bir uç yok."})),
        Ok(Err(e)) => {
            let sunucu_hatasi = e.starts_with("veritabanı") || e.contains("büro veritabanı");
            yanit(if sunucu_hatasi { 500 } else { 400 }, json!({"ok": false, "error": e}))
        }
        Err(e) => yanit(500, json!({"ok": false, "error": format!("İşlem tamamlanamadı: {e}")})),
    }
}

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use crate::icra_kayit as kayit;
    use crate::icra_veri::{dosya_kunyesi_kaydet, ham_tarihleri_turet, YargiTuru};
    let g = |y, a, d| NaiveDate::from_ymd_opt(y, a, d).unwrap();
    let sessiz = |_: &str| {};
    let bos = HashMap::new();
    let al = |c: &mut postgres::Client, uc: &str, q: &[(&str, &str)]| -> Value {
        let q: HashMap<String, String> = q.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        dagit(c, "GET", uc, &q, &json!({})).unwrap().unwrap()
    };
    let gonder = |c: &mut postgres::Client, uc: &str, govde: Value| dagit(c, "POST", uc, &HashMap::new(), &govde);

    let kunye = |no: &str| json!({"dosyaId": "x", "dosyaNo": no, "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": 35,
                                   "dosyaTur": "İcra Dosyası", "birimAdi": "İzmir 1. İcra Dairesi", "birimId": 1001});
    let (d1, _) = dosya_kunyesi_kaydet(c, &kunye("2026/5"), Some(YargiTuru::Icra.kod())).unwrap();
    let (d2, _) = dosya_kunyesi_kaydet(c, &kunye("2026/6"), None).unwrap();
    kayit::dosya_taraf_kaydet(
        c,
        d1,
        &[json!({"adi": "X BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum", "vekil": "[AYŞE KAYA]"}),
          json!({"adi": "AHMET YILMAZ", "rol": "Borçlu", "kisiKurum": "Kişi"}),
          json!({"adi": "IŞIL ÇİÇEK", "rol": "Borçlu", "kisiKurum": "Kişi"})],
        &sessiz,
    )
    .unwrap();
    kayit::dosya_taraf_kaydet(c, d2, &[json!({"adi": "MEHMET DEMİR", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    kayit::dosya_ayrinti_kaydet(c, d1, &json!({"takibinTuru": "0", "takibinYolu": "1", "takibinSekli": "7", "alacakKalemToplamTutar": 3762.9,
        "takibinTuruAciklama": "İlamlı Takip", "takibinYoluAciklama": "İlamların İcrası",
        "takibinSekliAciklama": " ÖRNEK: 4-5 Para Borcuna Dair İcra Emri - Eski No: 53 "})).unwrap();
    let teb_id: i64 = c
        .query_one(
            "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, evrak_tarihi, evrak_aciklama, barkod, ptt_durumu,
                                          son_islem_tarihi, teslim_sekli)
             VALUES ($1, 10, 'Ödeme Emri', '01/10/2026', 'Normal Tebligat[BORÇLU AHMET YILMAZ]', '4007663523293', 'TESLİM EDİLDİ',
                     '05/10/2026 14:30', 'Muhataba bizzat teslim') RETURNING id",
            &[&d1],
        )
        .unwrap()
        .get(0);
    ham_tarihleri_turet(c, "tebligat_barkod", teb_id).unwrap();
    veri::ptt_hareketlerini_yaz(c, teb_id, &json!({"hareketDongu": [{"tarih": "05/10/2026", "saat": "14:30", "aciklama": "TESLİM EDİLDİ", "il": "İZMİR"}]})).unwrap();

    // Süreler: doğrulama, çakışma, dağılım
    assert!(gonder(c, "kural", json!({"gun": 0})).is_err());
    assert!(gonder(c, "kural", json!({"gun": 400})).is_err());
    let v = gonder(c, "kural", json!({"ad": "Varsayılan", "gun": 7})).unwrap().unwrap();
    let varsayilan = v["id"].as_i64().unwrap();
    assert_eq!(v["guncellenen"], 3, "kural girilince bütün borçlular hesaplanır");
    assert!(gonder(c, "kural", json!({"ad": "çift", "gun": 5})).unwrap_err().contains("zaten"));
    let ozel = gonder(c, "kural", json!({"takibin_turu": "0", "takibin_yolu": " 1 ", "ad": "İlamsız", "gun": "10"})).unwrap().unwrap()["id"].as_i64().unwrap();
    assert!(gonder(c, "kural", json!({"id": varsayilan, "takibin_turu": "0", "takibin_yolu": "1", "gun": 7})).is_err(), "düzenlerken de çakışma denetlenir");
    gonder(c, "kural", json!({"id": varsayilan, "ad": "Varsayılan (7 gün)", "gun": 7})).unwrap();
    let k = al(c, "kurallar", &[]);
    assert_eq!(k["kurallar"].as_array().unwrap().len(), 2);
    assert_eq!(k["kurallar"][0]["ad"], "Varsayılan (7 gün)");
    assert_eq!(
        k["dagilim"][0],
        json!({"turu": "0", "yolu": "1", "sekli": "7", "adet": 1, "ornek": "İzmir 1. İcra Dairesi 2026/5", "turu_ad": "İlamlı Takip",
               "yolu_ad": "İlamların İcrası", "sekli_ad": "ÖRNEK: 4-5 Para Borcuna Dair İcra Emri - Eski No: 53", "ornek_no": "4-5"})
    );
    // Kural adları: tür + yol seçilmiş, şekil "hepsi"
    assert_eq!((&k["kurallar"][1]["turu_ad"], &k["kurallar"][1]["yolu_ad"], &k["kurallar"][1]["sekli_ad"]), (&json!("İlamlı Takip"), &json!("İlamların İcrası"), &json!("")));
    // Ağaç: dosyalarda görülen tür → yol → şekil, dosya sayısıyla (UYAP listesi henüz alınmadı)
    assert_eq!(k["agac"][0]["kod"], "0");
    assert_eq!((&k["agac"][0]["ad"], &k["agac"][0]["adet"]), (&json!("İlamlı Takip"), &json!(1)));
    assert_eq!(k["agac"][0]["yollar"][0]["sekiller"][0], json!({"kod": "7", "ad": "ÖRNEK: 4-5 Para Borcuna Dair İcra Emri - Eski No: 53", "ornek_no": "4-5", "adet": 1}));
    assert_eq!(k["liste_zamani"], Value::Null);
    // Kodlar sırayla seçilir: türsüz yol ya da yolsuz şekil olmaz
    assert!(gonder(c, "kural", json!({"takibin_yolu": "1", "gun": 7})).unwrap_err().contains("önce takip türü"));
    assert!(gonder(c, "kural", json!({"takibin_turu": "0", "takibin_sekli": "7", "gun": 7})).unwrap_err().contains("önce takip yolu"));
    assert_eq!(k["takip_turu_bilinmeyen"], 1);
    // Dosya sayısı: kurala düşen dosyalar (takip türü okunmamış olan varsayılana düşer)
    assert_eq!((&k["kurallar"][0]["adet"], &k["kurallar"][1]["adet"]), (&json!(1), &json!(1)));
    assert_eq!((&k["kurallar"][0]["varsayilan"]["gun"], &k["kurallar"][1]["varsayilan"]), (&json!(7), &Value::Null));
    assert_eq!((&k["kurallar"][0]["elle_degisti"], &k["kurallar"][1]["elle_degisti"]), (&json!(true), &json!(true)), "ad hazırdan farklı");
    // Hazır kural: itiraz süresi kesinleşmeden uzun olamaz; silinemez; varsayılana döner ve elle işareti kalkar
    assert!(gonder(c, "kural", json!({"id": varsayilan, "gun": 5, "itiraz_gun": 7})).unwrap_err().contains("kısa olamaz"));
    assert!(gonder(c, "kural_sil", json!({"id": varsayilan})).unwrap_err().contains("silinemez"));
    gonder(c, "kural_varsayilan", json!({"id": varsayilan})).unwrap();
    let k0 = &al(c, "kurallar", &[])["kurallar"][0];
    assert_eq!((&k0["ad"], &k0["gun"], &k0["itiraz_gun"], &k0["elle_degisti"]), (&json!("Varsayılan (tüm takipler)"), &json!(7), &json!(7), &json!(false)));
    // Hazır değerlerle aynı kaydedilirse elle sayılmaz; bir alan değişince sayılır
    let hazir = json!({"id": varsayilan, "ad": "Varsayılan (tüm takipler)", "gun": 7, "itiraz_gun": 7, "itiraz_durdurur": true, "dayanak": ""});
    gonder(c, "kural", hazir.clone()).unwrap();
    assert_eq!(al(c, "kurallar", &[])["kurallar"][0]["elle_degisti"], false);
    let mut degisik = hazir.clone();
    degisik["itiraz_durdurur"] = json!(false);
    gonder(c, "kural", degisik).unwrap();
    assert_eq!(al(c, "kurallar", &[])["kurallar"][0]["itiraz_durdurur"], false);
    assert_eq!(al(c, "kurallar", &[])["kurallar"][0]["elle_degisti"], true);
    gonder(c, "kural", hazir).unwrap();
    assert!(gonder(c, "kural_varsayilan", json!({"id": ozel})).unwrap_err().contains("hazır bir varsayılanı yok"));

    // Dosya penceresi: en özel kural uygulanır
    let d = al(c, "dosya", &[("id", &d1.to_string())]);
    assert_eq!(d["kunye"]["dosya_no"], "2026/5");
    assert_eq!((&d["takip"]["kesinlesme_suresi_gun"], &d["takip"]["kural"], &d["takip"]["alacak_toplam"]), (&json!(10), &json!("İlamsız"), &json!("3762.90")));
    assert_eq!((&d["takip"]["turu_ad"], &d["takip"]["yolu_ad"], &d["takip"]["ornek_no"]), (&json!("İlamlı Takip"), &json!("İlamların İcrası"), &json!("4-5")));
    assert!(d["takip"]["okunma"].is_string());
    let taraflar = d["taraflar"].as_array().unwrap();
    assert_eq!(taraflar.len(), 3);
    assert_eq!((&taraflar[0]["rol_etiket"], &taraflar[0]["vekiller"], &taraflar[0]["tuzel"]), (&json!("Alacaklı"), &json!("AYŞE KAYA"), &json!(true)));
    assert_eq!(taraflar[1]["ad"], "AHMET YILMAZ");
    assert_eq!(d["tebligatlar"][0]["muhatap"], "BORÇLU AHMET YILMAZ");
    assert_eq!((&d["tebligatlar"][0]["borclu"], &d["tebligatlar"][0]["hareket_sayisi"]), (&json!("AHMET YILMAZ"), &json!(1)));
    assert_eq!(al(c, "ptt", &[("tebligat", &teb_id.to_string())])[0]["yer"], "İZMİR");
    assert!(dagit(c, "GET", "dosya", &bos, &json!({})).is_err(), "id eksik");
    assert!(dagit(c, "GET", "olmayan", &bos, &json!({})).unwrap().is_none());

    // Hesap: 10 günlük süreyle 05.10 tebliğ → 15.10 son gün, 16.10 kesinleşme
    durum::durumlari_guncelle(c, None, g(2026, 10, 6), &sessiz).unwrap();
    let ahmet = al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1].clone();
    assert_eq!(ahmet["kesinlesme"], "Kesinleşme Bekliyor (16.10.2026)");
    assert_eq!(ahmet["tebligat"], "Tebliğ Edildi (05.10.2026) — Muhataba bizzat teslim");
    assert_eq!(ahmet["asama_metni"], "Tebliğ (05.10.2026)");
    // Tatil eklenince gün kayar (16.10 Cuma tatil → 19.10 Pazartesi)
    gonder(c, "tatil", json!({"tarih": "16.10.2026", "aciklama": "Deneme tatili"})).unwrap();
    assert_eq!(al(c, "tatiller", &[]), json!([{"tarih": "2026-10-16", "aciklama": "Deneme tatili"}]));
    assert!(gonder(c, "tatil", json!({"tarih": "olmaz"})).is_err());
    let ahmet_bag = ahmet["bag"].as_i64().unwrap();
    assert_eq!(al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1]["kesinlesme"], "Kesinleşme Bekliyor (19.10.2026)");
    gonder(c, "tatil_sil", json!({"tarih": "2026-10-16"})).unwrap();
    assert_eq!(al(c, "tatiller", &[]), json!([]));

    // Özet ve liste
    let o = al(c, "ozet", &[]);
    assert_eq!((&o["icra_dosya"], &o["borclu"], &o["tebligat"]), (&json!(2), &json!(3), &json!(1)));
    assert_eq!(o["birimler"], json!([{"ad": "İzmir 1. İcra Dairesi", "birim_id": "1001", "acik": 2}]));
    let adet = |o: &Value, kod: &str| o["asamalar"].as_array().unwrap().iter().find(|a| a["kod"] == kod).map(|a| a["adet"].clone());
    assert_eq!((adet(&o, "teblig"), adet(&o, "takip_talebi")), (Some(json!(1)), Some(json!(2))));
    let l = al(c, "borclular", &[]);
    assert_eq!(l["toplam"], 3);
    assert_eq!((&l["satirlar"][0]["borclu"], &l["satirlar"][0]["alacakli"]), (&json!("AHMET YILMAZ"), &json!("X BANKASI A.Ş.")), "aşama günü olan önce");
    assert_eq!(al(c, "borclular", &[("asama", "teblig")])["toplam"], 1);
    assert_eq!(al(c, "borclular", &[("q", "isil cicek")])["satirlar"][0]["borclu"], "IŞIL ÇİÇEK", "Türkçe-duyarsız arama");
    assert_eq!(al(c, "borclular", &[("q", "2026/6")])["satirlar"][0]["borclu"], "MEHMET DEMİR");
    assert_eq!(al(c, "borclular", &[("q", "x bankası ahmet")])["toplam"], 1);
    assert_eq!(al(c, "borclular", &[("q", "yok böyle biri")])["toplam"], 0);
    // Borçlunun tebligatları ve başlık süzgeçleri
    let ahmet_satir = l["satirlar"][0].clone();
    assert_eq!(ahmet_satir["tebligatlar"].as_array().unwrap().len(), 1, "borçlunun bütün tebligatları satırda");
    assert_eq!((&ahmet_satir["tebligatlar"][0]["barkod"], &ahmet_satir["tebligatlar"][0]["uyan"]), (&json!("4007663523293"), &json!(false)));
    assert_eq!(ahmet_satir["tebligatlar"][0]["kesinlesme"], ahmet_satir["kesinlesme"], "ödeme emri satırı borçlunun kesinleşmesini taşır");
    assert_eq!(ahmet_satir["tebligatlar"][0]["kesinlesme_yok"], json!(false));
    assert_eq!(l["satirlar"][1]["tebligatlar"], json!([]));
    assert_eq!(l["secenekler"]["tur"], json!([{"deger": "Ödeme Emri", "etiket": "Ödeme Emri", "adet": 1}]));
    assert_eq!(l["secenekler"]["teblig"][0]["deger"], "TESLİM EDİLDİ");
    assert_eq!(l["secenekler"]["mazbata"], json!([{"deger": "yok", "etiket": "Dönmedi", "adet": 1}]));
    let kes = l["secenekler"]["kesinlesme"].as_array().unwrap();
    assert_eq!(kes[0], json!({"deger": "-", "etiket": "Henüz hesaplanmadı", "adet": 2}));
    assert_eq!(kes[1], json!({"deger": "kesinlesme_bekliyor", "etiket": "Kesinleşme Bekliyor", "adet": 1}));
    let t = al(c, "borclular", &[("tur", "odeme")]);
    assert_eq!((&t["toplam"], &t["satirlar"][0]["tebligatlar"][0]["uyan"]), (&json!(1), &json!(true)), "yazarak: içinde geçen, Türkçe-duyarsız");
    assert_eq!(al(c, "borclular", &[("tur", "=Ödeme Emri"), ("teblig", "=TESLİM EDİLDİ")])["toplam"], 1);
    assert_eq!(al(c, "borclular", &[("tur", "=Ödeme")])["toplam"], 0, "listeden seçilen: tam eşleşme");
    assert_eq!(al(c, "borclular", &[("tur", "odeme"), ("teblig", "iade")])["toplam"], 0, "aynı tebligatta birlikte aranır");
    assert_eq!(al(c, "borclular", &[("mazbata", "yok")])["toplam"], 1);
    let m = al(c, "borclular", &[("mazbata", "var")]);
    assert_eq!(m["toplam"], 0);
    assert_eq!(m["secenekler"]["mazbata"][0]["deger"], "yok", "bir süzgecin seçenekleri kendi seçiminden etkilenmez");
    assert_eq!(m["secenekler"]["tur"], json!([]), "diğer süzgeçlerin seçenekleri daralır");
    assert_eq!(al(c, "borclular", &[("kesinlesme", "kesinlesme_bekliyor")])["satirlar"][0]["borclu"], "AHMET YILMAZ");
    let k = al(c, "borclular", &[("kesinlesme", "-")]);
    assert_eq!((&k["toplam"], &k["secenekler"]["tur"]), (&json!(2), &json!([])));
    assert_eq!(al(c, "borclular", &[("kesinlesme", "-"), ("tur", "odeme")])["toplam"], 0);
    let sayfa = al(c, "borclular", &[("boyut", "2"), ("sayfa", "2")]);
    assert_eq!((sayfa["toplam"].clone(), sayfa["satirlar"].as_array().unwrap().len()), (json!(3), 1));
    c.execute("UPDATE dosya SET durum_kod = 1 WHERE id = $1", &[&d2]).unwrap();
    assert_eq!(al(c, "borclular", &[("acik", "1")])["toplam"], 2);

    // Künye süzgeçleri: daire, alacaklı, borçlu, yıl, dosya türü, açılış tarihi, durum; borçlusuz dosya
    let (d3, _) = dosya_kunyesi_kaydet(
        c,
        &json!({"dosyaId": "y", "dosyaNo": "2025/77", "dosyaDurumKod": 1, "dosyaDurum": "Kapalı", "dosyaTurKod": 36, "dosyaTur": "MTS",
                "birimAdi": "Ankara 3. İcra Dairesi", "birimId": 2002, "dosyaAcilisTarihi": "15.03.2025"}),
        Some(YargiTuru::Icra.kod()),
    )
    .unwrap();
    kayit::dosya_taraf_kaydet(c, d3, &[json!({"adi": "Y FAKTORİNG A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum"})], &sessiz).unwrap();
    assert_eq!(al(c, "borclular", &[("durum", "acik")])["toplam"], 2);
    let k = al(c, "borclular", &[("durum", "kapali")]);
    assert_eq!((&k["toplam"], &k["dosya_sayisi"]), (&json!(2), &json!(2)), "kapalı: MEHMET'in dosyası + borçlusuz dosya");
    let h = al(c, "borclular", &[]);
    assert_eq!((&h["toplam"], &h["dosya_sayisi"]), (&json!(4), &json!(3)));
    let bos_satir = h["satirlar"].as_array().unwrap().iter().find(|s| s["dosya"] == d3).unwrap().clone();
    assert_eq!((&bos_satir["bag"], &bos_satir["borclu"], &bos_satir["alacakli"]), (&Value::Null, &json!(""), &json!("Y FAKTORİNG A.Ş.")));
    assert_eq!((&bos_satir["acilis_tarihi"], &bos_satir["dosya_tur"], &bos_satir["tebligatlar"]), (&json!("2025-03-15"), &json!("MTS"), &json!([])));
    assert_eq!(al(c, "borclular", &[("asama", "teblig")])["toplam"], 1, "aşama seçilince borçlusuz dosya gelmez");
    // Daire: yazı = içinde geçen, Türkçe-duyarsız; `=` tam
    assert_eq!(al(c, "borclular", &[("birim", "izmir")])["toplam"], 3);
    assert_eq!(al(c, "borclular", &[("birim", "=Ankara 3. İcra Dairesi")])["toplam"], 1);
    assert_eq!(al(c, "borclular", &[("birim", "=Ankara")])["toplam"], 0);
    // Daire "Tümünü seç" sonra bazılarını kaldır: `!` = listedekiler hariç; `!` tek başına hepsi
    assert_eq!(al(c, "borclular", &[("birim", "!")])["toplam"], 4);
    assert_eq!(al(c, "borclular", &[("birim", "!Ankara 3. İcra Dairesi")])["toplam"], 3);
    assert_eq!(al(c, "borclular", &[("birim", "!Ankara 3. İcra Dairesi\nİzmir 1. İcra Dairesi")])["toplam"], 0);
    let s = al(c, "borclular", &[("birim", "!Ankara 3. İcra Dairesi")]);
    assert_eq!(s["secenekler"]["birim"].as_array().unwrap().len(), 2, "hariç tutulan daire seçeneklerde kalır (geri eklenebilsin)");
    assert_eq!(al(c, "borclular", &[("dosya_tur", "!MTS")])["toplam"], 3, "kodlu süzgeçte de hariç");
    assert_eq!(al(c, "borclular", &[("alacakli", "!Y FAKTORİNG A.Ş.")])["dosya_sayisi"], 2);
    // Dosya no: ayrı sütun ve süzgeç
    assert_eq!(al(c, "borclular", &[("dosya_no", "=2025/77")])["satirlar"][0]["dosya"], d3);
    assert_eq!(al(c, "borclular", &[("dosya_no", "2026/")])["dosya_sayisi"], 2);
    assert_eq!(al(c, "borclular", &[])["secenekler"]["dosya_no"].as_array().unwrap().len(), 3);
    // Alacaklı: dosyanın alacaklılarından biri
    assert_eq!(al(c, "borclular", &[("alacakli", "x bankasi")])["toplam"], 2, "X BANKASI'nın dosyasındaki iki borçlu");
    assert_eq!(al(c, "borclular", &[("alacakli", "=Y FAKTORİNG A.Ş.")])["dosya_sayisi"], 1);
    // Borçlu
    assert_eq!(al(c, "borclular", &[("borclu", "isil")])["satirlar"][0]["borclu"], "IŞIL ÇİÇEK");
    assert_eq!(al(c, "borclular", &[("borclu", "=AHMET YILMAZ")])["toplam"], 1);
    // Yıl ve tür (kodlu: tam değer)
    assert_eq!(al(c, "borclular", &[("yil", "2025")])["toplam"], 1);
    assert_eq!(al(c, "borclular", &[("yil", "2026"), ("dosya_tur", "İcra Dosyası")])["toplam"], 3);
    assert_eq!(al(c, "borclular", &[("dosya_tur", "MTS")])["satirlar"][0]["dosya"], d3);
    // MTS'den gelen dosya: açılıştan önce düzenlenmiş ödeme emri tebligatı → "İcra Dosyası (MTS'den)" ayrı süzgeç değeri
    let (dm, _) = dosya_kunyesi_kaydet(
        c,
        &json!({"dosyaId": "m", "dosyaNo": "2026/900", "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": 35, "dosyaTur": "İcra Dosyası",
                "birimAdi": "İzmir Abonelik Sözleşmeleri İcra Dairesi", "birimId": 3003, "dosyaAcilisTarihi": "02.10.2026"}),
        Some(YargiTuru::Icra.kod()),
    )
    .unwrap();
    c.execute("INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, evrak_tarihi, evrak_gunu) VALUES ($1, 1, 'Ödeme Emri', '03/09/2026', '2026-09-03')", &[&dm]).unwrap();
    let m = al(c, "borclular", &[("dosya_tur", "İcra Dosyası (MTS'den)")]);
    assert_eq!((&m["toplam"], &m["satirlar"][0]["dosya"], &m["satirlar"][0]["mts_kaynakli"]), (&json!(1), &json!(dm), &json!(true)));
    assert_eq!(al(c, "borclular", &[("dosya_tur", "İcra Dosyası"), ("dosya_no", "=2026/900")])["toplam"], 0, "ayrı değer");
    assert!(al(c, "borclular", &[])["secenekler"]["dosya_tur"].as_array().unwrap().iter().any(|x| x["deger"] == "İcra Dosyası (MTS'den)"));
    assert_eq!(al(c, "dosya", &[("id", &dm.to_string())])["kunye"]["mts_kaynakli"], true);
    assert_eq!(al(c, "dosya", &[("id", &d3.to_string())])["kunye"]["mts_kaynakli"], false, "MTS dosyasının kendisi değil");
    c.execute("DELETE FROM dosya WHERE id = $1", &[&dm]).unwrap();
    // Açılış tarihi aralığı (açılışı bilinmeyen dosya aralık verilince dışarıda kalır)
    assert_eq!(al(c, "borclular", &[("acilis_bas", "2025-03-01"), ("acilis_son", "15.03.2025")])["toplam"], 1);
    assert_eq!(al(c, "borclular", &[("acilis_bas", "2025-03-16")])["toplam"], 0);
    // Seçenekler: diğer süzgeçlere göre daralır; listeden seçilen kendi kutusunu daraltmaz, yazı daraltır
    let s = al(c, "borclular", &[("birim", "=Ankara 3. İcra Dairesi")]);
    assert_eq!(s["secenekler"]["birim"].as_array().unwrap().len(), 2, "seçilen daire diğer daireleri gizlemez");
    assert_eq!(s["secenekler"]["alacakli"], json!([{"deger": "Y FAKTORİNG A.Ş.", "etiket": "Y FAKTORİNG A.Ş.", "adet": 1}]));
    assert_eq!(s["secenekler"]["borclu"], json!([]));
    assert_eq!(s["secenekler"]["yil"], json!([{"deger": "2025", "etiket": "2025", "adet": 1}]));
    let s = al(c, "borclular", &[("birim", "ankara")]);
    assert_eq!(s["secenekler"]["birim"].as_array().unwrap().len(), 1, "yazılan metin kendi seçeneklerini daraltır");
    let s = al(c, "borclular", &[]);
    assert_eq!(s["secenekler"]["yil"][0]["deger"], "2026", "yıllar yeniden eskiye");
    assert_eq!(s["secenekler"]["borclu"].as_array().unwrap().len(), 3);
    assert_eq!(s["secenekler"]["dosya_tur"][0], json!({"deger": "İcra Dosyası", "etiket": "İcra Dosyası", "adet": 3}));
    // Birden çok seçim (satır sonuyla): herhangi birine uyan
    assert_eq!(al(c, "borclular", &[("birim", "=Ankara 3. İcra Dairesi\nİzmir 1. İcra Dairesi")])["toplam"], 4);
    assert_eq!(al(c, "borclular", &[("borclu", "=AHMET YILMAZ\nIŞIL ÇİÇEK")])["toplam"], 2);
    assert_eq!(al(c, "borclular", &[("dosya_tur", "MTS\nİcra Dosyası"), ("yil", "2025\n2026")])["toplam"], 4);
    assert_eq!(al(c, "borclular", &[("yil", "2025\n2024")])["toplam"], 1);
    let s = al(c, "borclular", &[("alacakli", "=Y FAKTORİNG A.Ş.\nX BANKASI A.Ş.")]);
    assert_eq!((&s["toplam"], &s["dosya_sayisi"]), (&json!(3), &json!(2)));
    assert_eq!(s["secenekler"]["alacakli"].as_array().unwrap().len(), 2, "seçilenler kendi listesini daraltmaz");
    assert_eq!(al(c, "borclular", &[("kesinlesme", "-\nkesinlesme_bekliyor")])["toplam"], al(c, "borclular", &[])["toplam"]);
    assert_eq!(al(c, "borclular", &[("mazbata", "var\nyok")])["toplam"], 1, "mazbata süzgeci tebligatı olanlar");

    // Elle itiraz ve elle kesinleşme değeri; geçmiş
    gonder(c, "itiraz", json!({"bag": ahmet_bag, "edildi": true, "tarih": "2026-10-08"})).unwrap();
    let a = al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1].clone();
    assert_eq!((&a["kesinlesme"], &a["asama_metni"], &a["itiraz_tarihi"]), (&json!("İtiraz"), &json!("İtiraz (08.10.2026)"), &json!("2026-10-08")));
    gonder(c, "itiraz", json!({"bag": ahmet_bag, "edildi": false})).unwrap();
    assert!(gonder(c, "kesinlesme_elle", json!({"bag": ahmet_bag, "durum": "kesinlesti"})).is_err());
    gonder(c, "kesinlesme_elle", json!({"bag": ahmet_bag, "durum": "konkordato"})).unwrap();
    assert_eq!(al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1]["kesinlesme"], "Konkordato");
    gonder(c, "hesapla", json!({})).unwrap();
    assert_eq!(al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1]["kesinlesme"], "Konkordato", "hesap elle girileni ezmez");
    // Tehir-i icra: elle girilir, hesap ezmez, takip aşaması "Durduruldu"
    gonder(c, "kesinlesme_elle", json!({"bag": ahmet_bag, "durum": "tehiri_icra"})).unwrap();
    gonder(c, "hesapla", json!({})).unwrap();
    let a = al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1].clone();
    assert_eq!(a["kesinlesme"], "Tehir-i İcra", "hesap elle girilen tehir-i icrayı ezmez");
    assert!(a["asama_metni"].as_str().unwrap().starts_with("Durduruldu"), "{}", a["asama_metni"]);
    gonder(c, "kesinlesme_elle", json!({"bag": ahmet_bag, "durum": "konkordato"})).unwrap();
    // Seçili dosyalar: yalnız onlar hesaplanır; süzgeçteki bütün dosya kimlikleri tek istekle
    assert_eq!(gonder(c, "hesapla", json!({"dosyalar": [d1]})).unwrap().unwrap()["dosya_sayisi"], 1);
    assert!(gonder(c, "hesapla", json!({"dosyalar": ["x"]})).is_err());
    let idler = al(c, "dosya_kimlikleri", &[]);
    let toplam = al(c, "borclular", &[("boyut", "500")])["dosya_sayisi"].clone();
    assert_eq!(json!(idler.as_array().unwrap().len()), toplam, "kimlik listesi listedeki dosya sayısıyla aynı");
    assert!(idler.as_array().unwrap().contains(&json!(d1)));
    gonder(c, "kesinlesme_elle", json!({"bag": ahmet_bag, "durum": ""})).unwrap();
    assert!(al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1]["kesinlesme"].as_str().unwrap().starts_with("Kesinleşme Bekliyor"));
    // Elle kesinleşme günü: gün geldiyse Kesinleşti; hesap ezmez; konkordato yerine geçer; boş gönderilince hesaba döner
    gonder(c, "kesinlesme_elle", json!({"bag": ahmet_bag, "durum": "konkordato"})).unwrap();
    let r = gonder(c, "kesinlesme_tarihi", json!({"bag": ahmet_bag, "tarih": "2026-10-27"})).unwrap().unwrap();
    assert_eq!(r["kesinlesme"], "Kesinleşti (27.10.2026)");
    gonder(c, "hesapla", json!({})).unwrap();
    let a = al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1].clone();
    assert_eq!((&a["kesinlesme"], &a["kesinlesme_elle_tarihi"], &a["asama_metni"]),
               (&json!("Kesinleşti (27.10.2026)"), &json!("2026-10-27"), &json!("Kesinleşti (27.10.2026)")), "hesap elle günü ezmez");
    assert!(gonder(c, "kesinlesme_tarihi", json!({"bag": ahmet_bag, "tarih": "2099-01-01"})).is_err());
    assert_eq!(gonder(c, "kesinlesme_tarihi", json!({"bag": ahmet_bag, "tarih": "2027-03-01"})).unwrap().unwrap()["kesinlesme"],
               "Kesinleşme Bekliyor (01.03.2027)", "günü gelmemiş");
    gonder(c, "kesinlesme_tarihi", json!({"bag": ahmet_bag, "tarih": null})).unwrap();
    let a = al(c, "dosya", &[("id", &d1.to_string())])["taraflar"][1].clone();
    assert!(a["kesinlesme"].as_str().unwrap().starts_with("Kesinleşme Bekliyor") && a["kesinlesme_elle_tarihi"].is_null(), "{a}");
    assert!(gonder(c, "itiraz", json!({"bag": 999999, "edildi": true})).is_err());
    gonder(c, "islem_durumu", json!({"dosya": d1, "durum": "beklemede", "notu": "müvekkil bekleniyor", "bekleme_bitis": "01/12/2026"})).unwrap();
    assert!(gonder(c, "islem_durumu", json!({"dosya": d1, "durum": "olmaz"})).is_err());
    let d = al(c, "dosya", &[("id", &d1.to_string())]);
    assert_eq!((&d["kunye"]["islem_durumu_etiket"], &d["kunye"]["bekleme_bitis"]), (&json!("Beklemede"), &json!("2026-12-01")));
    let gecmis = al(c, "gecmis", &[("dosya", &d1.to_string())]);
    let gecmis = gecmis.as_array().unwrap();
    assert_eq!(gecmis[0]["tur_etiket"], "İşlem durumu");
    assert!(gecmis.iter().any(|o| o["kaynak"] == "elle" && o["yeni"] == "Konkordato" && o["taraf"] == "AHMET YILMAZ"));
    assert!(gecmis.iter().any(|o| o["tur"] == "kesinlesme" && o["yeni"] == "İtiraz" && o["kaynak_etiket"] == "Hesap"));

    // Süre silinince yeniden hesaplanır
    gonder(c, "kural_sil", json!({"id": ozel})).unwrap();
    assert_eq!(al(c, "dosya", &[("id", &d1.to_string())])["takip"]["kesinlesme_suresi_gun"], 7);

    // Talimat dosyası: kesinleşme süzgeçte ayrı değer, tebligatlarda "kesinleşme yok", panelde süre yok
    let bilinmeyen_once = al(c, "kurallar", &[])["takip_turu_bilinmeyen"].clone();
    let (dtl, _) = dosya_kunyesi_kaydet(
        c,
        &json!({"dosyaId": "t", "dosyaNo": "2026/900", "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": 1, "dosyaTur": "Talimat Dosyası",
                "birimAdi": "İzmir 1. İcra Dairesi", "birimId": 1001}),
        Some(YargiTuru::Icra.kod()),
    )
    .unwrap();
    kayit::dosya_taraf_kaydet(c, dtl, &[json!({"adi": "VELİ KARA", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    let tbag: i64 = c.query_one("SELECT id FROM dosya_taraf WHERE dosya_id = $1", &[&dtl]).unwrap().get(0);
    c.execute(
        "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, borclu_id, evrak_aciklama, ptt_durumu)
         VALUES ($1, 2, 'Kıymet Takdiri Raporu', $2, 'Normal Tebligat[BORÇLU VELİ KARA]', 'TESLİM EDİLDİ')",
        &[&dtl, &tbag],
    )
    .unwrap();
    let k = al(c, "borclular", &[("kesinlesme", "talimat")]);
    assert_eq!((&k["toplam"], &k["satirlar"][0]["dosya"], &k["satirlar"][0]["talimat"]), (&json!(1), &json!(dtl), &json!(true)));
    let t0 = &k["satirlar"][0]["tebligatlar"][0];
    assert_eq!((&t0["kesinlesme"], &t0["kesinlesme_yok"], &t0["kesinlesme_neden"]), (&json!(""), &json!(true), &json!("Talimat dosyasında kesinleşme yok")));
    let secenek = al(c, "borclular", &[])["secenekler"]["kesinlesme"].clone();
    assert!(secenek.as_array().unwrap().iter().any(|o| o["deger"] == "talimat" && o["etiket"] == "Talimat dosyası (kesinleşme yok)"), "{secenek}");
    assert_eq!(al(c, "borclular", &[("kesinlesme", "-"), ("dosya_no", "=2026/900")])["toplam"], 0, "talimat 'henüz hesaplanmadı' sayılmaz");
    let p = al(c, "dosya", &[("id", &dtl.to_string())]);
    assert_eq!((&p["kunye"]["talimat"], &p["takip"]["kesinlesme_suresi_gun"], &p["takip"]["kesinlesme_yok"]), (&json!(true), &Value::Null, &json!("Talimat dosyasında kesinleşme yok")));
    assert_eq!(al(c, "kurallar", &[])["takip_turu_bilinmeyen"], bilinmeyen_once, "talimatın takip türü bilinmeyen sayılmaz");

    // E-tebliğ mazbatası (dosya içinden, UYAP'sız): barkodla; barkod tutmazsa muhatap + gün; o da yoksa seçim
    let (dz, _) = dosya_kunyesi_kaydet(
        c,
        &json!({"dosyaId": "z", "dosyaNo": "2026/77", "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": 35, "dosyaTur": "İcra Dosyası",
                "birimAdi": "İzmir 7. İcra Dairesi", "birimId": "1004963", "dosyaAcilisTarihi": "01.01.2026"}),
        Some(YargiTuru::Icra.kod()),
    )
    .unwrap();
    for (no, barkod, aciklama, gun) in [
        (10, "5003049769049", "Elektronik Tebligat[ÜÇÜNCÜ ŞAHIS ALİ VELİ]", "2026-08-27"),
        (11, "5003049760000", "Elektronik Tebligat[ÜÇÜNCÜ ŞAHIS AYŞE KARA]", "2026-08-27"),
    ] {
        c.execute(
            "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, evrak_aciklama, barkod, evrak_gunu, elektronik_tebligat)
             VALUES ($1, $2, 'Haciz İhbarnamesi', $3, $4, $5::text::date, true)",
            &[&dz, &no, &aciklama, &barkod, &gun],
        )
        .unwrap();
    }
    let mazbata = |c: &mut postgres::Client, barkod: &str, secilen: Option<i64>| {
        let mut g = json!({"birim_id": "1004963", "dosya_no": "2026/77", "dosya_tur_kod": 35, "belge": "Kapalı E-Tebliğ Mazbatası 16/09/2026",
                           "metin": tests_ortak::e_mazbata_ornegi(barkod)});
        if let Some(t) = secilen { g["tebligat"] = json!(t); }
        gonder(c, "mazbata_isle", g).unwrap().unwrap()
    };
    let r = mazbata(c, "5003049769049", None);
    assert_eq!((&r["islendi"], &r["eslesme"], &r["tebligat"]["evrak_no"]), (&json!(true), &json!("barkod"), &json!(10)));
    let (var, ac): (bool, String) = c.query_one("SELECT tebligat_mazbatasi_var, tebligat_mazbatasi_aciklama FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = 10", &[&dz]).map(|s| (s.get(0), s.get(1))).unwrap();
    assert!(var && ac.starts_with("Kapalı E-Tebliğ Mazbatası 16/09/2026 - Barkod Numarası: 5003049769049") && ac.contains("okundu sayıldı"), "{ac}");
    assert!(teb::kendi_mazbatasi_dondu_mu("5003049769049", var, &ac), "hesap mazbatayı barkodla tanır");
    // Barkod tutmuyor: aynı gün + muhatap (ALİ VELİ) → evrak 10; açıklamada tebligatın kendi barkodu
    c.execute("UPDATE tebligat_barkod SET tebligat_mazbatasi_var = false, tebligat_mazbatasi_aciklama = '' WHERE dosya_id = $1", &[&dz]).unwrap();
    let r = mazbata(c, "9999999999999", None);
    assert_eq!((&r["eslesme"], &r["tebligat"]["evrak_no"]), (&json!("muhatap_tarih"), &json!(10)));
    let ac: String = c.query_one("SELECT tebligat_mazbatasi_aciklama FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = 10", &[&dz]).unwrap().get(0);
    assert!(ac.contains("Barkod Numarası: 5003049769049 (mazbatadaki barkod 9999999999999)"), "{ac}");
    // Muhatap da uymuyor: tahmin yok, adaylar döner; kullanıcı seçince işlenir
    c.execute("UPDATE tebligat_barkod SET evrak_aciklama = 'Elektronik Tebligat[ÜÇÜNCÜ ŞAHIS BAŞKA BİRİ]' WHERE dosya_id = $1 AND birim_evrak_no = 10", &[&dz]).unwrap();
    let r = mazbata(c, "9999999999999", None);
    assert_eq!((&r["secim_gerekli"], r["adaylar"].as_array().unwrap().len()), (&json!(true), 2));
    let t11: i64 = c.query_one("SELECT id FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = 11", &[&dz]).unwrap().get(0);
    let r = mazbata(c, "9999999999999", Some(t11));
    assert_eq!((&r["eslesme"], &r["tebligat"]["evrak_no"]), (&json!("secim"), &json!(11)));
    assert!(gonder(c, "mazbata_isle", json!({"birim_id": "1004963", "dosya_no": "2026/999", "metin": []})).unwrap_err().contains("2026/999"));

    // Tebligat grupları: eski ham tür → sabit tür + zarf içeriği; muhatap alanlara; tanınmayan ham kalır
    for (no, tur, aciklama) in [
        (20, "TAŞINMAZSATIŞ İLANI", "Normal Tebligat[BORÇLU AHMET YILMAZ]"),
        (21, "4-5 ÖRNEK İCRA EMRİ VE İLAM SURETİ", "Normal Tebligat[Vekil HÜSEYİN KAYA]"),
        (22, "Vekillikten Çekilme Dilekçesi", "Normal Tebligat[ALACAKLI X]"),
        (23, "Haciz İhbarnamesi", "Elektronik Tebligat[ÜÇÜNCÜ ŞAHIS SALİH GÜNDÜZ]"),
    ] {
        c.execute("INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, evrak_aciklama) VALUES ($1, $2, $3, $4)", &[&dz, &no, &tur, &aciklama]).unwrap();
    }
    let n = tebligat_gruplarini_uygula(c, chrono::Local::now().date_naive(), &|_| {}).unwrap();
    assert!(n >= 3, "{n}");
    let oku = |c: &mut postgres::Client, no: i32| -> (String, String, String, String) {
        c.query_one("SELECT tebligat_turu, zarf_icerigi, muhatap_adi, muhatap_turu FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = $2", &[&dz, &no])
            .map(|s| (s.get(0), s.get(1), s.get(2), s.get(3))).unwrap()
    };
    assert_eq!(oku(c, 20), ("Satış İlanı".into(), "TAŞINMAZSATIŞ İLANI".into(), "AHMET YILMAZ".into(), "borclu".into()));
    assert_eq!(oku(c, 21).0, "Ödeme Emri", "ilamlı icra emri ödeme emri grubunda");
    assert_eq!(oku(c, 22).0, "Vekillikten Çekilme Dilekçesi", "tanınmayan ham tür kalır (Tür eşleştir)");
    assert_eq!((oku(c, 23).2.as_str(), oku(c, 23).3.as_str()), ("SALİH GÜNDÜZ", "ucuncu"));
    assert_eq!(tebligat_gruplarini_uygula(c, chrono::Local::now().date_naive(), &|_| {}).unwrap(), 0, "ikinci kez değişiklik yok");
    // Kullanıcının seçtiği tür (kategori) ham sayılmaz, ezilmez
    c.execute("INSERT INTO tebligat_tur_kategorisi (ad) VALUES ('Vekillikten Çekilme') ON CONFLICT DO NOTHING", &[]).unwrap();
    c.execute("UPDATE tebligat_barkod SET tebligat_turu = 'Vekillikten Çekilme' WHERE dosya_id = $1 AND birim_evrak_no = 22", &[&dz]).unwrap();
    tebligat_gruplarini_uygula(c, chrono::Local::now().date_naive(), &|_| {}).unwrap();
    assert_eq!(oku(c, 22).0, "Vekillikten Çekilme");
    // 89 sırası: belgede yoksa "?"; kullanıcı girer; listede Grup ve 89 süzgeçleri
    let t23: i64 = c.query_one("SELECT id FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = 23", &[&dz]).unwrap().get(0);
    let l = al(c, "tebligatlar", &[("dosya_no", "=2026/77"), ("haciz", "?")]);
    assert!(l["satirlar"].as_array().unwrap().iter().any(|x| x["id"] == t23), "{l}");
    assert!(gonder(c, "haciz_sirasi", json!({"tebligat": t23, "sira": 4})).is_err());
    gonder(c, "haciz_sirasi", json!({"tebligat": t23, "sira": 2})).unwrap();
    let l = al(c, "tebligatlar", &[("dosya_no", "=2026/77"), ("haciz", "2")]);
    assert_eq!((&l["toplam"], &l["satirlar"][0]["haciz_sirasi"], &l["satirlar"][0]["grup"]), (&json!(1), &json!(2), &json!("Haciz İhbarnamesi (89)")));
    assert_eq!(al(c, "tebligatlar", &[("dosya_no", "=2026/77"), ("grup", "Satış İlanı")])["toplam"], 1);
    let elle: bool = c.query_one("SELECT haciz_sirasi_elle FROM tebligat_barkod WHERE id = $1", &[&t23]).unwrap().get(0);
    assert!(elle);
    c.execute("DELETE FROM dosya WHERE id = $1", &[&dz]).unwrap();

    c.batch_execute("DELETE FROM dosya; DELETE FROM taraf; DELETE FROM vekil; DELETE FROM birim; DELETE FROM kesinlesme_kurali").unwrap();
}

/// Senaryo ve birim testlerinin ortak örnekleri.
#[cfg(test)]
mod tests_ortak {
    // Canlıda görülen "Kapalı E-Tebliğ Mazbatası" yazı katmanı (20.11.2026; kişi ve hesap bilgileri değiştirildi)
    pub fn e_mazbata_ornegi(barkod: &str) -> Vec<String> {
        [
            "7. İcra Dairesi", "Dosya No:", " ", "2026/77 İcra", "No:", "Taahhütlü", "Adı ve Soyadı:",
            "[11111-22222-33333] UETS Hesap Sahibi: ALİ VELİ", "89/1. Haciz ihbarnamesi vardır.", "İZMİR", "T.C.",
            "Üçüncü Şahıs ALİ VELİ", "Mühür ve İmza", "Vekili", " ", ":", "BU ZARFTA İcra Daireleri Haciz Tebliğ Yazısı VARDIR.",
            "Örnek No: 25", "Tarih", " ", "Açıklama", "27/08/2026 16:29", " ", "Muhatap hesabına teslim edilmek üzere UETS",
            "tarafından teslim alındı.", "27/08/2026 16:29", " ", "Tebligat alıcı için ayrılmış tebligat alanına (hesabına)",
            "başarılı bir şekilde konuldu.", "27/08/2026 19:56", " ", "Tebligat, alıcısı tarafından açıldı.", "01/09/2026",
            "Tebligat, alıcının hesabına iletilmesine müteakip", "mevzuat gereği belirlenen süre sonunda otomatik", "olarak okundu sayıldı.",
            "E-TEBLİĞ MAZBATASI", "Karar No:",
        ]
        .iter()
        .map(|x| x.to_string())
        .chain([format!("*{barkod}*"), "Yukarıda Belirtilen Tebliğ Bilgileri … Sorgu Sonuçlarıdır.".to_string()])
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::tests_ortak::e_mazbata_ornegi;

    #[test]
    fn e_mazbata_cozumleme() {
        let g = |y, a, d| NaiveDate::from_ymd_opt(y, a, d).unwrap();
        let m = e_mazbata_coz(&e_mazbata_ornegi("5003049769049"));
        assert_eq!(m.barkod.as_deref(), Some("5003049769049"));
        assert_eq!(m.hesap_sahibi, "ALİ VELİ");
        assert_eq!(m.ilk_olay, Some(g(2026, 8, 27)));
        let (son, metin) = m.son_olay.unwrap();
        assert_eq!(son, g(2026, 9, 1));
        assert!(metin.contains("okundu sayıldı"), "{metin}");
        assert_eq!(e_mazbata_coz(&["yalnız yazı".to_string()]), EMazbata::default());
    }

    use super::*;

    #[test]
    fn yol_ve_tarih() {
        assert!(is_path("__cu__/veri/ozet") && is_path("/__cu__/veri/borclular"));
        assert!(!is_path("__cu__/tema") && !is_path("main/avukat/x.ajx"));
        assert_eq!(gun_coz(&json!("2026-10-16")).unwrap(), NaiveDate::from_ymd_opt(2026, 10, 16));
        assert_eq!(gun_coz(&json!("16.10.2026")).unwrap(), NaiveDate::from_ymd_opt(2026, 10, 16));
        assert_eq!(gun_coz(&json!("")).unwrap(), None);
        assert_eq!(gun_coz(&Value::Null).unwrap(), None);
        assert!(gun_coz(&json!("olmaz")).is_err() && gun_coz(&json!(5)).is_err());
        assert_eq!(gun(NaiveDate::from_ymd_opt(2026, 1, 5)), json!("2026-01-05"));
    }
}
