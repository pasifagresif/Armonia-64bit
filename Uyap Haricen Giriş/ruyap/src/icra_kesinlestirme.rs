//! İcra Takip Süreçleri → "Kesinleştirme" sekmesi (22.11.2026; kullanıcı notu `Desktop\süreçler\kesinleştirme sekmesi.md`).
//!
//! Her satır bir BORÇLUDUR (kesinleşme borçlu başınadır). Yalnız alacaklı vekili olduğumuz, "İcra Dosyası" türündeki
//! açık dosyalar listelenir: talimat, iflas, MTS (ve MTS'den gelen), ihtiyati haciz dosyaları kapsam dışıdır (kullanıcı
//! kararı: MTS kesinleştirmesi ayrı süreç; ihtiyati hacizde işlem Evrak Gönder'den yapılır). Rol, Dosya Listesi'ndeki
//! hesapla bulunur (`icra_dosyalar::benim_rolum`).
//!
//! Durum (grup) önceliği: UYAP'ta kesinleşti → itiraz / durduruldu → talep gönderildi (gönderildiği gün; ertesi gün hâlâ
//! kesinleşmemişse "kesinleşme görülmedi" uyarısı) → kesinleştirilebilir (programın hesabı: süre doldu) → süre işliyor
//! (kesinleşme günü belli) → tebliğ / mazbata bekleniyor.
//!
//! UYAP (canlıda 21.11.2026 doğrulandı, salt okuma):
//!   • `getTakibiKesinlesenBorcluListesi_brd.ajx {dosyaId}`: dosyada takibi kesinleşmiş borçlular (dosya başına 1 istek).
//!   • `dosya_icra_kesinlesme.ajx {dosyaId, kisiKurumId}`: `basarilimi` true = kesinleşmiş. Portal kesinleştirme talebini
//!     (talep kodu 31) bununla engeller; biz de her talepten hemen önce sorarız (daire kendiliğinden kesinleştirebilir).
//!   • `getPortalAvukatTalepList.ajx {dosyaId, islemTuru:"genel"}`: genel talepler; "Takibin Kesinleşmesi" talepKodu 31,
//!     masrafsız, açıklamasız. Yalnız alacaklı vekiline verilir (yoksa PRTL_GNL_1-1); kesinleşmiş dosyada listede yok.
//!   • Gönderim 121 talebiyle aynı (uyap_proxy.rs `talepEvrakiGonder`): `getIcraTalepEvrakHazirla.uyap` (UDF) →
//!     e-imza → `avukatIcraTalepEvrakiGonder.ajx` (çok parçalı; "Genel Talebi", talepGrupId 2).
//! UYAP'ta kesinleşme görülünce İş Listesi'ndeki kesinleştirme işleri (şablon h-kes-tarih / h-kes-gonder / h-kes-talep /
//! h-kesinlesme) otomatik tamamlanır.
//!
//! Uçlar (`/__cu__/veri/…`, `icra_api::dagit`):
//!   GET kesinlestirme            ?<süzgeçler>&sirala=&yon=&sayfa=&boyut= — satırlar + adetli seçenekler (gruplar dahil)
//!   GET kesinlestirme_kimlikleri ?<süzgeçler>&is=kesinlesme|talep — {dosyalar, borclular}: o işin istek atacakları
//!   GET kesinlestirme_istek      ?<süzgeçler>&is=… — başlatmadan önce kaç dosya / borçlu, kaç istek, ne kadar sürer

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use postgres::GenericClient;
use serde_json::{json, Value};

use crate::icra_api::{haric_listesi, metin_uyar, secenek_sinirli, AYRAC, SECENEK_SINIRI};
use crate::icra_dosyalar::{ad_anahtari, avukat_adlari, benim_rolum, tr_sira_anahtari, VekilliTaraf, AYLAR};
use crate::icra_guncelle::{
    ag_hatasi_mi, dosya_istegi_at, guncel_kimlik, kimlik_bayat_mi, kimlik_yenile, mola, sayfa_gecersiz, uyap_reddi, vt, Tarama,
    TaramaInner, Uyap, YazilanDosya, AG_ONEKI, BORCLU_LISTESI,
};
use crate::icra_tebligat as teb;
use crate::icra_veri::{KesinlesmeDurumu, TakipAsamasi};

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

// ── UYAP uçları ─────────────────────────────────────────────────────────────────────────────────

pub const KESINLESEN_LISTESI: &str = "getTakibiKesinlesenBorcluListesi_brd.ajx";
pub const KESINLESME_DENETIMI: &str = "dosya_icra_kesinlesme.ajx";
pub const TALEP_LISTESI: &str = "getPortalAvukatTalepList.ajx";
pub const TALEP_HAZIRLA: &str = "getIcraTalepEvrakHazirla.uyap";
pub const TALEP_GONDER: &str = "avukatIcraTalepEvrakiGonder.ajx";
/// UYAP genel talepler listesinde "Takibin Kesinleşmesi".
pub const TALEP_KODU: i64 = 31;
const GENEL_TALEP_EVRAK_TURU: &str = "ICR_AVUKAT_PORTAL_GENEL_TALEP";
const GENEL_TALEP_GRUBU: &str = "2";
/// UYAP kesinleşme görülünce tamamlanan İş Listesi şablonları.
pub const KESINLESTIRME_SABLONLARI: [&str; 4] = ["h-kes-tarih", "h-kes-gonder", "h-kes-talep", "h-kesinlesme"];

/// "Bu sorgulama, alacaklı vekili tarafından yapılabilmektedir" (PRTL_GNL_1-1): dosyaya özgü yetki reddi; oturum
/// düşmesi değildir, art arda ret sayımına girmez.
pub fn yetki_reddi_mi(neden: &str) -> bool {
    neden.contains("PRTL_GNL_1-1") || teb::tr_kucuk(neden).contains("alacakli vekili tarafindan")
}

// ── Borçlu eşleştirme (UYAP kaydı ↔ veritabanı) ───────────────────────────────────────────────────

/// Kimlik numarası taşıyan UYAP alanları (TCKN, vergi no, MERSİS).
const KIMLIK_ALANLARI: [&str; 5] = ["tarafTC", "tcKimlikNo", "vergiNo", "kurumVergiNo", "mersisNo"];

/// UYAP kaydındaki kimlik numaraları, iç içe nesneler dahil. Vekil / ortak / temsilci alt kayıtlarına girilmez (onların
/// TCKN'si borçlununki sanılmasın).
pub fn uyap_kimlikleri(v: &Value) -> HashSet<String> {
    fn topla(v: &Value, out: &mut HashSet<String>) {
        let Some(o) = v.as_object() else { return };
        for (k, x) in o {
            let kk = k.to_ascii_lowercase();
            if kk.contains("vekil") || kk.contains("ortak") || kk.contains("temsilci") || kk.contains("avukat") {
                continue;
            }
            if KIMLIK_ALANLARI.contains(&k.as_str()) {
                let s = match x {
                    Value::String(s) => s.trim().to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                };
                if s.len() >= 8 && s.chars().all(|c| c.is_ascii_digit()) && s.chars().any(|c| c != '0') {
                    out.insert(s);
                }
            } else if x.is_object() {
                topla(x, out);
            }
        }
    }
    let mut out = HashSet::new();
    topla(v, &mut out);
    out
}

/// UYAP kaydındaki taraf adı (kesinleşen listesi: `tarafAdi`; borçlu listesi: kişi adı soyadı ya da kurum adı).
pub fn uyap_adi(v: &Value) -> String {
    let m = |v: &Value, a: &str| teb::metin(v, a).trim().to_string();
    for yol in [&v["tarafAdi"], &v["dosyaKisiDVO"]["tarafAdi"]] {
        if let Some(s) = yol.as_str().filter(|s| !s.trim().is_empty()) {
            return s.trim().to_string();
        }
    }
    for k in ["kisiTumDVO", "kisiDVO"] {
        let ad = format!("{} {}", m(&v[k], "adi"), m(&v[k], "soyadi")).trim().to_string();
        if !ad.is_empty() {
            return ad;
        }
    }
    for k in ["kurumDVO", "kurumTumDVO"] {
        let ad = m(&v[k], "kurumAdi");
        if !ad.is_empty() {
            return ad;
        }
    }
    m(&v["kisiKurumBilgileriDVO"], "ortakAd")
}

/// UYAP kaydının kişi / kurum kimliği (talep isteklerinde `kisiKurumId`).
pub fn kisi_kurum_id(v: &Value) -> Option<i64> {
    for a in [&v["kisiKurumId"], &v["guncelKisiKurumId"], &v["kisiKurumBilgileriDVO"]["kisiKurumId"]] {
        if let Some(n) = a.as_i64().or_else(|| a.as_str().and_then(|s| s.trim().parse().ok())) {
            return Some(n);
        }
    }
    None
}

/// Veritabanındaki borçlu (eşleştirme için).
#[derive(Debug, Clone, Default)]
pub struct BizimBorclu {
    pub bag: i64,
    pub ad: String,
    pub kimlikler: Vec<String>,
}

/// UYAP kaydı bu borçlu mu: iki tarafta da kimlik varsa kimlikle (ad tutsa da kimlik farklıysa değil), yoksa adla
/// (Türkçe küçük harf, noktalama yok sayılarak tam eşitlik).
pub fn eslesir(b: &BizimBorclu, r: &Value) -> bool {
    let k = uyap_kimlikleri(r);
    if !b.kimlikler.is_empty() && !k.is_empty() {
        return b.kimlikler.iter().any(|x| k.contains(x));
    }
    let a = ad_anahtari(&b.ad);
    !a.is_empty() && a == ad_anahtari(&uyap_adi(r))
}

pub(crate) fn kimlik_listesi(tckn: Option<String>, vergi: Option<String>, mersis: Option<String>) -> Vec<String> {
    [tckn, vergi, mersis].into_iter().flatten().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn dosya_borclulari<C: GenericClient>(c: &mut C, dosya: i64) -> Result<Vec<BizimBorclu>, String> {
    let ad = crate::icra_api::ad_sql("t");
    Ok(c.query(
        &format!(
            "SELECT x.id, {ad}, t.tckn, NULLIF(t.vergi_no, ''), t.mersis_no FROM dosya_taraf x JOIN taraf t ON t.id = x.taraf_id
              WHERE x.dosya_id = $1 AND x.rol = 'borclu' AND x.uyapta_gorulmedi IS NULL ORDER BY x.sira, x.id"
        ),
        &[&dosya],
    )
    .map_err(hata)?
    .iter()
    .map(|s| BizimBorclu { bag: s.get(0), ad: s.get(1), kimlikler: kimlik_listesi(s.get(2), s.get(3), s.get(4)) })
    .collect())
}

// ── Veritabanına yazma ──────────────────────────────────────────────────────────────────────────

/// UYAP'ın kesinleşme bilgisini borçluya yazar, dosyanın durumlarını yeniden hesaplar (UYAP'ta kesinleşen borçlu
/// Kesinleşti olur) ve kesinleşmişse kesinleştirme işlerini kapatır. Döner: kapanan iş sayısı.
pub fn kesinlesme_isaretle<C: GenericClient>(c: &mut C, bag: i64, kesinlesti: bool, bugun: NaiveDate) -> Result<usize, String> {
    let kapanan = kesinlesme_yaz(c, bag, kesinlesti, bugun)?;
    if let Some(s) = c.query_opt("SELECT dosya_id FROM dosya_taraf WHERE id = $1", &[&bag]).map_err(hata)? {
        crate::icra_durum::durumlari_guncelle(c, Some(s.get(0)), bugun, &|_| {})?;
    }
    Ok(kapanan)
}

fn kesinlesme_yaz<C: GenericClient>(c: &mut C, bag: i64, kesinlesti: bool, bugun: NaiveDate) -> Result<usize, String> {
    c.execute(
        "UPDATE dosya_taraf SET uyap_kesinlesti = $2, uyap_kesinlesme_zamani = now(),
                uyap_kesinlesti_gunu = CASE WHEN $2 THEN COALESCE(uyap_kesinlesti_gunu, $3) ELSE NULL END
          WHERE id = $1",
        &[&bag, &kesinlesti, &bugun],
    )
    .map_err(hata)?;
    if !kesinlesti {
        return Ok(0);
    }
    isleri_kapat(c, bag)
}

/// Borçlunun açık kesinleştirme işlerini (İş Listesi) tamamlar: UYAP'ta takip kesinleşmiş görüldü.
fn isleri_kapat<C: GenericClient>(c: &mut C, bag: i64) -> Result<usize, String> {
    let sablonlar: Vec<&str> = KESINLESTIRME_SABLONLARI.to_vec();
    let not = "UYAP'ta takip kesinleşmiş görüldü (otomatik kapatıldı).";
    let kapanan: Vec<i64> = c
        .query(
            "UPDATE gorev SET durum = 'tamamlandi', tamamlanma = now(), guncelleme = now(), sonuc = 'olumlu', sonuc_notu = $3
              WHERE taraf_id = $1 AND durum IN ('acik', 'devam', 'onayda')
                AND sablon_id IN (SELECT id FROM sablon WHERE anahtar = ANY($2))
              RETURNING id",
            &[&bag, &sablonlar, &not],
        )
        .map_err(hata)?
        .iter()
        .map(|s| s.get(0))
        .collect();
    for id in &kapanan {
        crate::is_veri::olay_yaz(c, Some(*id), None, crate::is_veri::olay::DURUM, json!({"yeni": "tamamlandi", "sonuc": "olumlu", "not": not}))?;
    }
    Ok(kapanan.len())
}

/// Dosyanın UYAP kesinleşen borçlu listesini veritabanındaki borçlularla eşleyip yazar.
/// Döner: (kesinleşmiş borçlu, kapanan iş, UYAP'ta olup eşleşmeyen adlar).
pub fn uyap_kesinlesmesini_yaz<C: GenericClient>(
    c: &mut C,
    dosya: i64,
    liste: &[Value],
    bugun: NaiveDate,
) -> Result<(usize, usize, Vec<String>), String> {
    let borclular = dosya_borclulari(c, dosya)?;
    let kayitlar: Vec<&Value> = liste.iter().filter(|x| x.is_object()).collect();
    let (mut kesinlesen, mut kapanan) = (0, 0);
    let mut eslesen: HashSet<usize> = HashSet::new();
    for b in &borclular {
        let bulunan: Vec<usize> = kayitlar.iter().enumerate().filter(|(_, r)| eslesir(b, r)).map(|(i, _)| i).collect();
        let k = !bulunan.is_empty();
        eslesen.extend(bulunan);
        kapanan += kesinlesme_yaz(c, b.bag, k, bugun)?;
        kesinlesen += usize::from(k);
    }
    crate::icra_durum::durumlari_guncelle(c, Some(dosya), bugun, &|_| {})?;
    let eslesmeyen = kayitlar.iter().enumerate().filter(|(i, _)| !eslesen.contains(i)).map(|(_, r)| uyap_adi(r)).collect();
    Ok((kesinlesen, kapanan, eslesmeyen))
}

fn talep_yaz<C: GenericClient>(c: &mut C, bag: i64, sonuc: &str, mesaj: &str) -> Result<(), String> {
    c.execute(
        "INSERT INTO kesinlestirme_talebi (dosyataraf_id, sonuc, mesaj) VALUES ($1, $2, $3)",
        &[&bag, &sonuc, &crate::icra_veri::kirp(mesaj, 1000)],
    )
    .map_err(hata)?;
    Ok(())
}

fn bugun_gonderildi<C: GenericClient>(c: &mut C, bag: i64) -> Result<bool, String> {
    Ok(c.query_one(
        "SELECT EXISTS (SELECT 1 FROM kesinlestirme_talebi WHERE dosyataraf_id = $1 AND sonuc = 'gonderildi' AND zaman::date = CURRENT_DATE)",
        &[&bag],
    )
    .map_err(hata)?
    .get(0))
}

// ── UYAP işleri (tarama altyapısıyla: hız, mola, iptal, kimlik yenileme) ─────────────────────────────

pub(crate) fn neden(v: &Option<Value>) -> String {
    sayfa_gecersiz(v).unwrap_or_else(|| "beklenmeyen yanıt".into())
}

/// `Mod::Kesinlesme`: her dosyada UYAP'ın kesinleşen borçlu listesi (1 istek) → borçlulara yazılır.
pub(crate) async fn kesinlesme_oku<U: Uyap>(t: &Tarama, u: &U, dosyalar: Vec<YazilanDosya>) -> Result<(), String> {
    let dosyalar: Vec<YazilanDosya> = dosyalar.into_iter().filter(|x| !x.1.is_empty()).collect();
    t.asama("kesinlesme", dosyalar.len());
    let bugun = chrono::Local::now().date_naive();
    let (mut ardisik, mut ret) = (0usize, 0usize);
    for (sira, (dosya, uyap_id, etiket)) in dosyalar.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.ind.iptal_kontrol()?;
        let mut uyap_id = guncel_kimlik(t, u, dosya, &uyap_id).await?;
        t.mesaj_yaz(format!("Kesinleşme UYAP'tan okunuyor: {etiket} ({}/{})", sira + 1, t.inner.lock().unwrap().toplam));
        match dosya_istegi_at(t, u, KESINLESEN_LISTESI, dosya, &mut uyap_id, json!({})).await {
            Ok(Some(Value::Array(liste))) => {
                (ardisik, ret) = (0, 0);
                let (n, kapanan, eslesmeyen) = vt(move |c| uyap_kesinlesmesini_yaz(c, dosya, &liste, bugun)).await?;
                let mut g = t.inner.lock().unwrap();
                g.kes_dosya += 1;
                g.kes_kesinlesen += n;
                g.kes_kapanan_is += kapanan;
                for a in eslesmeyen {
                    g.uyarilar.push(format!("{etiket}: UYAP'ta kesinleşmiş görünen “{a}” veritabanındaki borçlularla eşleşmedi"));
                }
            }
            Ok(v) => {
                ardisik = 0;
                let n = neden(&v);
                t.uyari(format!("{etiket}: kesinleşme UYAP'tan okunamadı ({n})"));
                if !yetki_reddi_mi(&n) {
                    uyap_reddi(&n, &mut ret)?;
                }
            }
            Err(e) => ag_hatasi_mi(&e, t, &etiket, &mut ardisik)?,
        }
        t.ilerle();
    }
    Ok(())
}

/// Talep gönderilecek borçlu.
#[derive(Debug, Clone)]
pub struct TalepIsi {
    pub bag: i64,
    pub dosya: i64,
    pub dosya_no: String,
    pub borclu: BizimBorclu,
}

/// Seçilen borçlular (seçim sırasıyla); açık icra dosyasının borçlusu olmayanlar düşer.
fn talep_isleri<C: GenericClient>(c: &mut C, baglar: &[i64]) -> Result<Vec<TalepIsi>, String> {
    let ad = crate::icra_api::ad_sql("t");
    Ok(c.query(
        &format!(
            "SELECT x.id, x.dosya_id, d.dosya_no, {ad}, t.tckn, NULLIF(t.vergi_no, ''), t.mersis_no
               FROM unnest($1::bigint[]) WITH ORDINALITY AS s(id, n)
               JOIN dosya_taraf x ON x.id = s.id JOIN dosya d ON d.id = x.dosya_id JOIN taraf t ON t.id = x.taraf_id
              WHERE x.rol = 'borclu' AND x.uyapta_gorulmedi IS NULL AND d.yargi_turu = 2 AND d.durum_kod = 0
              ORDER BY s.n"
        ),
        &[&baglar],
    )
    .map_err(hata)?
    .iter()
    .map(|s| TalepIsi {
        bag: s.get(0),
        dosya: s.get(1),
        dosya_no: s.get(2),
        borclu: BizimBorclu { bag: s.get(0), ad: s.get(3), kimlikler: kimlik_listesi(s.get(4), s.get(5), s.get(6)) },
    })
    .collect())
}

/// Bir talebin sonucu.
#[derive(Debug, Clone, PartialEq)]
pub enum TalepSonucu {
    /// UYAP gönderimi kabul etti (UYAP'ın mesajı).
    Gonderildi(String),
    /// UYAP borçlunun zaten kesinleşmiş olduğunu bildirdi: gönderilmedi.
    Kesinlesmis,
    /// Genel talep listesinde "Takibin Kesinleşmesi" yok.
    TurYok(String),
    /// UYAP geçersiz yanıt verdi (art arda ret sayımına girer; yetki reddi hariç).
    Red(String),
    /// Bu borçluya özgü engel (eşleşmedi, masraflı …): kayda geçer, sayıma girmez.
    Engel(String),
    /// Gönderim isteği sırasında bağlantı koptu: talebin UYAP'a ulaşıp ulaşmadığı bilinmiyor.
    Belirsiz(String),
}

/// Dosya başına bir kez alınanlar: UYAP borçlu listesi ve "Takibin Kesinleşmesi" talep türü (Some(None): listede yok).
#[derive(Default)]
struct DosyaOnbellegi {
    borclular: Option<Vec<Value>>,
    tur: Option<Option<Value>>,
}

/// Portalın talep listesine eklediği biçim: tür + masraf + boş alan listesi (açıklamasız talep; main.js 21.11.2026).
pub fn talep_bilgileri(tur: &Value) -> String {
    let mut t = tur.clone();
    t["masraf"] = tur.get("talepMasrafi").cloned().unwrap_or(json!(0));
    t["fields"] = json!([{"id": "", "title": ""}]);
    t["id"] = json!(0);
    Value::Array(vec![t]).to_string()
}

fn talep_kodu(v: &Value) -> Option<i64> {
    let k = &v["talepKodu"];
    k.as_i64().or_else(|| k.as_str().and_then(|s| s.trim().parse().ok()))
}

fn masraf(v: &Value) -> f64 {
    let m = &v["talepMasrafi"];
    m.as_f64().or_else(|| m.as_str().and_then(|s| s.trim().replace(',', ".").parse().ok())).unwrap_or(0.0)
}

/// UYAP'ın hazırladığı talep evrakı gerçekten UDF mi (zip ya da şablon XML'i).
pub fn udf_mu(st: u16, b: &[u8]) -> bool {
    st < 400 && (b.starts_with(b"PK") || String::from_utf8_lossy(&b[..b.len().min(4000)]).contains("<template"))
}

pub(crate) fn kisalt(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(&b[..b.len().min(300)]).replace(['\n', '\r'], " ");
    s.chars().take(160).collect()
}

pub(crate) fn html_temizle(s: &str) -> String {
    let mut out = String::new();
    let mut ic = false;
    for ch in s.chars() {
        match ch {
            '<' => ic = true,
            '>' => {
                ic = false;
                out.push(' ');
            }
            c if !ic => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Gönderim isteğinin alanları (UYAP'ın evrak yükleme bileşeninin gönderdiği biçimde; bkz. uyap_proxy.rs 121 talebi).
fn gonderim_alanlari(uyap_id: &str, kkid: i64, talep_bilgileri: &str, imzali_ad: &str, imzali: Vec<u8>) -> Vec<(String, Option<String>, Vec<u8>)> {
    let simdi = chrono::Utc::now().timestamp_millis();
    let tur = json!({
        "tur": GENEL_TALEP_EVRAK_TURU, "label": "Genel Talebi", "mandatory": true, "max": 1, "ekEvrakMax": 2, "sablonTurKodu": "",
        "acceptTypeList": [".udf", ".UDF"],
        "ekEvrakAcceptTypeList": [".udf", ".pdf", ".jpg", ".jpeg", ".png", ".tif", ".tiff", ".UDF", ".PDF", ".JPG", ".JPEG", ".PNG", ".TIF", ".TIFF"]
    });
    let item = json!({
        "id": simdi, "evrakTuruOptionDVO": tur, "tur": GENEL_TALEP_EVRAK_TURU, "turAciklama": "Genel Talebi", "mandatory": true,
        "file": {}, "path": format!("C:/fakepath/{imzali_ad}"), "kullaniciEvrakAciklama": "", "parentId": -1,
        "isVekalet": false, "isVekaletBilgiFormu": false, "fileId": 1
    });
    let metin = |a: &str, v: String| (a.to_string(), None, v.into_bytes());
    vec![
        ("file_1".into(), Some(imzali_ad.to_string()), imzali),
        metin("items", Value::Array(vec![item]).to_string()),
        metin("dosyaId", uyap_id.to_string()),
        metin("kisiKurumId", kkid.to_string()),
        metin("tutar", "0".into()),
        metin("talepGrupId", GENEL_TALEP_GRUBU.into()),
        metin("talepBilgileri", talep_bilgileri.to_string()),
    ]
}

/// Tek borçluya kesinleştirme talebi. Dış hata: ağ (`AG_ONEKI`), iptal ya da e-imza hatası (tarama durur).
async fn tek_talep<U: Uyap>(
    t: &Tarama,
    u: &U,
    is_: &TalepIsi,
    uyap_id: &mut String,
    ob: &mut HashMap<i64, DosyaOnbellegi>,
) -> Result<TalepSonucu, String> {
    use TalepSonucu::*;
    let dosya = is_.dosya;
    // 1) Borçlunun UYAP kişi / kurum kimliği (dosyanın borçlu listesinden).
    if ob.get(&dosya).is_none_or(|o| o.borclular.is_none()) {
        match dosya_istegi_at(t, u, BORCLU_LISTESI, dosya, uyap_id, json!({})).await? {
            Some(Value::Array(a)) => ob.entry(dosya).or_default().borclular = Some(a),
            v => return Ok(Red(format!("UYAP borçlu listesi alınamadı ({})", neden(&v)))),
        }
    }
    let liste = ob.get(&dosya).and_then(|o| o.borclular.clone()).unwrap_or_default();
    let adaylar: Vec<&Value> = liste.iter().filter(|r| r.is_object() && eslesir(&is_.borclu, r)).collect();
    let kkid = match adaylar.as_slice() {
        [r] => match kisi_kurum_id(r) {
            Some(k) => k,
            None => return Ok(Engel("UYAP borçlu kaydında kişi / kurum kimliği yok".into())),
        },
        [] => return Ok(Engel("borçlu UYAP'ın borçlu listesinde eşleşmedi (ad ya da kimlik farklı)".into())),
        _ => return Ok(Engel("UYAP'ın borçlu listesinde bu borçluya birden çok kayıt uyuyor".into())),
    };
    // 2) Daire kendiliğinden kesinleştirmiş olabilir: portal da talebi bununla engeller.
    let v = dosya_istegi_at(t, u, KESINLESME_DENETIMI, dosya, uyap_id, json!({"kisiKurumId": kkid})).await?;
    match v.as_ref().and_then(|v| v.get("basarilimi")).and_then(Value::as_bool) {
        Some(true) => return Ok(Kesinlesmis),
        Some(false) => {}
        None => return Ok(Red(format!("kesinleşme denetlenemedi ({})", neden(&v)))),
    }
    // 3) "Takibin Kesinleşmesi" talep türü (dosya başına bir kez).
    if ob.get(&dosya).is_none_or(|o| o.tur.is_none()) {
        let v = dosya_istegi_at(t, u, TALEP_LISTESI, dosya, uyap_id, json!({"islemTuru": "genel"})).await?;
        let tur = match &v {
            Some(Value::Array(a)) => a.iter().find(|x| talep_kodu(x) == Some(TALEP_KODU)).cloned(),
            _ => return Ok(Red(format!("UYAP talep türleri alınamadı ({})", neden(&v)))),
        };
        ob.entry(dosya).or_default().tur = Some(tur);
    }
    let Some(tur) = ob.get(&dosya).and_then(|o| o.tur.clone()).flatten() else {
        return Ok(TurYok("UYAP'ın genel talep listesinde “Takibin Kesinleşmesi” yok (dosya durumu bu talebe izin vermiyor)".into()));
    };
    let m = masraf(&tur);
    if m > 0.0 {
        return Ok(Engel(format!("talep {m} TL masraflı görünüyor; ödeme bilgisi gerektiği için gönderilmedi")));
    }
    let bilgiler = talep_bilgileri(&tur);
    // 4) Talep evrakı (UDF). Kimlik bayatsa yenilenip bir kez tekrarlanır (okuma isteği).
    let govde = |id: &str| json!({"dosyaId": id, "filename": is_.dosya_no, "kisiKurumId": kkid, "talepBilgileri": bilgiler});
    let (mut st, mut udf) = u.post_ham(TALEP_HAZIRLA, govde(uyap_id)).await?;
    if !udf_mu(st, &udf) && kimlik_bayat_mi(&String::from_utf8_lossy(&udf)) {
        if let Some(yeni) = kimlik_yenile(t, u, dosya, uyap_id).await? {
            *uyap_id = yeni;
            (st, udf) = u.post_ham(TALEP_HAZIRLA, govde(uyap_id)).await?;
        }
    }
    if !udf_mu(st, &udf) {
        return Ok(Red(format!("UYAP talep evrakını hazırlamadı (HTTP {st}): {}", kisalt(&udf))));
    }
    // 5) E-imza: hata kart / PIN sorunudur, sonraki borçlularda da sürer → tarama durur.
    let ad = format!("{}_Kesinlestirme_Talebi", is_.dosya_no.replace(['/', '\\'], "_"));
    let imzali = u.imzala(udf, format!("{ad}.udf")).await.map_err(|e| format!("{e}. Kesinleştirme talepleri durduruldu."))?;
    // 6) Gönderim (yazma; yeniden denenmez).
    let imzali_ad = format!("{ad}_imzali.udf");
    match u.yukle(TALEP_GONDER, gonderim_alanlari(uyap_id, kkid, &bilgiler, &imzali_ad, imzali)).await {
        Ok(Some(v)) if teb::metin(&v, "type") == "success" => Ok(Gonderildi(html_temizle(&teb::metin(&v, "message")))),
        Ok(v) => {
            let m = v.as_ref().map(|v| html_temizle(&teb::metin(v, "message"))).filter(|m| !m.is_empty()).unwrap_or_else(|| neden(&v));
            Ok(Red(format!("UYAP gönderimi kabul etmedi: {m}")))
        }
        Err(e) if e.starts_with(AG_ONEKI) => Ok(Belirsiz(e.trim_start_matches(AG_ONEKI).to_string())),
        Err(e) => Err(e),
    }
}

/// `Mod::Talep`: seçili borçlulara kesinleştirme talebi. Her deneme `kesinlestirme_talebi`'ne yazılır.
pub(crate) async fn talepleri_gonder<U: Uyap>(t: &Tarama, u: &U, dosyalar: &[YazilanDosya]) -> Result<(), String> {
    let secilen = t.inner.lock().unwrap().kapsam.borclular.clone().unwrap_or_default();
    let uyap: HashMap<i64, (String, String)> = dosyalar.iter().map(|(d, k, e)| (*d, (k.clone(), e.clone()))).collect();
    let isler = vt(move |c| talep_isleri(c, &secilen)).await?;
    t.asama("talep", isler.len());
    let bugun = chrono::Local::now().date_naive();
    let mut ob: HashMap<i64, DosyaOnbellegi> = HashMap::new();
    let (mut ardisik, mut ret) = (0usize, 0usize);
    for (sira, is_) in isler.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.ind.iptal_kontrol()?;
        let bag = is_.bag;
        let Some((uid, dosya_etiket)) = uyap.get(&is_.dosya).cloned().filter(|x| !x.0.is_empty()) else {
            t.inner.lock().unwrap().talep_atlanan += 1;
            t.uyari(format!("{} – {}: dosya UYAP'ta bulunamadı; talep gönderilmedi", is_.dosya_no, is_.borclu.ad));
            t.ilerle();
            continue;
        };
        let etiket = format!("{dosya_etiket} – {}", is_.borclu.ad);
        if vt(move |c| bugun_gonderildi(c, bag)).await? {
            t.inner.lock().unwrap().talep_atlanan += 1;
            t.uyari(format!("{etiket}: bugün zaten kesinleştirme talebi gönderildi; yeniden gönderilmedi"));
            t.ilerle();
            continue;
        }
        let mut uid = guncel_kimlik(t, u, is_.dosya, &uid).await?;
        t.mesaj_yaz(format!("Kesinleştirme talebi: {etiket} ({}/{})", sira + 1, t.inner.lock().unwrap().toplam));
        let sonuc = match tek_talep(t, u, &is_, &mut uid, &mut ob).await {
            Ok(s) => s,
            Err(e) if e.starts_with(AG_ONEKI) => {
                vt(move |c| talep_yaz(c, bag, "hata", "UYAP'a bağlanılamadı; talep gönderilmedi")).await?;
                t.inner.lock().unwrap().talep_hatasi += 1;
                ag_hatasi_mi(&e, t, &etiket, &mut ardisik)?;
                t.ilerle();
                continue;
            }
            Err(e) => {
                // İptal ya da e-imza hatası: bu borçlu için kayıt düşülür, tarama durur.
                if !e.contains("__IPTAL__") {
                    let m = e.clone();
                    vt(move |c| talep_yaz(c, bag, "hata", &m)).await?;
                    t.inner.lock().unwrap().talep_hatasi += 1;
                }
                return Err(e);
            }
        };
        match sonuc {
            TalepSonucu::Gonderildi(m) => {
                (ardisik, ret) = (0, 0);
                let mesaj = if m.is_empty() { "UYAP talebi kabul etti".to_string() } else { m };
                vt(move |c| talep_yaz(c, bag, "gonderildi", &mesaj)).await?;
                t.inner.lock().unwrap().talep_gonderilen += 1;
            }
            TalepSonucu::Kesinlesmis => {
                (ardisik, ret) = (0, 0);
                let kapanan = vt(move |c| {
                    talep_yaz(c, bag, "kesinlesmis", "UYAP: borçlu hakkında takip zaten kesinleşmiş; talep gönderilmedi")?;
                    kesinlesme_isaretle(c, bag, true, bugun)
                })
                .await?;
                let mut g = t.inner.lock().unwrap();
                g.talep_kesinlesmis += 1;
                g.kes_kesinlesen += 1;
                g.kes_kapanan_is += kapanan;
            }
            TalepSonucu::TurYok(m) => {
                (ardisik, ret) = (0, 0);
                t.uyari(format!("{etiket}: {m}"));
                vt(move |c| talep_yaz(c, bag, "tur_yok", &m)).await?;
                t.inner.lock().unwrap().talep_atlanan += 1;
            }
            TalepSonucu::Engel(m) => {
                ardisik = 0;
                t.uyari(format!("{etiket}: {m}"));
                vt(move |c| talep_yaz(c, bag, "hata", &m)).await?;
                t.inner.lock().unwrap().talep_hatasi += 1;
            }
            TalepSonucu::Red(m) => {
                ardisik = 0;
                t.uyari(format!("{etiket}: {m}"));
                let kayit = m.clone();
                vt(move |c| talep_yaz(c, bag, "hata", &kayit)).await?;
                t.inner.lock().unwrap().talep_hatasi += 1;
                if !yetki_reddi_mi(&m) {
                    uyap_reddi(&m, &mut ret)?;
                }
            }
            TalepSonucu::Belirsiz(m) => {
                let kayit = format!("Gönderim sırasında bağlantı koptu ({m}); talebin UYAP'a ulaşıp ulaşmadığını dosyanın evraklarından kontrol edin.");
                t.uyari(format!("⚠️ {etiket}: {kayit}"));
                vt(move |c| talep_yaz(c, bag, "hata", &kayit)).await?;
                t.inner.lock().unwrap().talep_hatasi += 1;
                ag_hatasi_mi(&format!("{AG_ONEKI}{m}"), t, &etiket, &mut ardisik)?;
            }
        }
        t.ilerle();
    }
    Ok(())
}

/// Tarama raporu (Mod::Kesinlesme / Mod::Talep).
pub(crate) fn rapor_satirlari(g: &TaramaInner) -> Vec<String> {
    let mut out = Vec::new();
    if g.mod_ == crate::icra_guncelle::Mod::Talep {
        out.push(format!("✔ {} borçluya kesinleştirme talebi gönderildi.", g.talep_gonderilen));
        if g.talep_kesinlesmis > 0 {
            out.push(format!("✔ {} borçlu UYAP'ta zaten kesinleşmiş; talep gönderilmedi.", g.talep_kesinlesmis));
        }
        if g.talep_atlanan > 0 {
            out.push(format!("… {} borçlu atlandı (talep türü yok, bugün gönderilmiş ya da dosya bulunamadı; ayrıntı uyarılarda).", g.talep_atlanan));
        }
        if g.talep_hatasi > 0 {
            out.push(format!("⚠️ {} borçluda talep gönderilemedi (neden uyarılarda ve satırın Talep sütununda).", g.talep_hatasi));
        }
    } else {
        out.push(format!("✔ {} dosyanın kesinleşmesi UYAP'tan okundu: {} borçlu kesinleşmiş.", g.kes_dosya, g.kes_kesinlesen));
    }
    if g.kes_kapanan_is > 0 {
        out.push(format!("✔ İş Listesi'nde {} kesinleştirme işi otomatik tamamlandı.", g.kes_kapanan_is));
    }
    out
}

pub(crate) fn bitis_mesaji(g: &TaramaInner) -> String {
    if g.mod_ == crate::icra_guncelle::Mod::Talep {
        let mut m = format!("Tamamlandı: {} kesinleştirme talebi gönderildi", g.talep_gonderilen);
        if g.talep_kesinlesmis > 0 {
            m.push_str(&format!(", {} borçlu zaten kesinleşmiş", g.talep_kesinlesmis));
        }
        if g.talep_hatasi + g.talep_atlanan > 0 {
            m.push_str(&format!(" ⚠️ {} borçluya gönderilemedi (ayrıntı uyarılarda)", g.talep_hatasi + g.talep_atlanan));
        }
        m + "."
    } else {
        format!("Tamamlandı: {} dosyanın kesinleşmesi UYAP'tan okundu; {} borçlu UYAP'ta kesinleşmiş.", g.kes_dosya, g.kes_kesinlesen)
    }
}

// ── Liste ───────────────────────────────────────────────────────────────────────────────────────

/// Durum grupları (sırası üstteki düğmelerin sırası).
pub const GRUPLAR: [(&str, &str); 7] = [
    ("kesinlestirilebilir", "Kesinleştirilebilir"),
    ("talep_sonucsuz", "Talep gönderildi, kesinleşme görülmedi"),
    ("talep_gonderildi", "Talep gönderildi"),
    ("sure_isliyor", "Süre işliyor"),
    ("bekleniyor", "Tebliğ / mazbata bekleniyor"),
    ("uyap_kesinlesti", "UYAP'ta kesinleşti"),
    ("durdu", "İtiraz / durduruldu"),
];

pub fn grup_etiketi(kod: &str) -> &'static str {
    GRUPLAR.iter().find(|g| g.0 == kod).map(|g| g.1).unwrap_or("(bilinmiyor)")
}

fn uyap_etiketi(kod: &str) -> &'static str {
    match kod {
        "evet" => "UYAP'ta kesinleşti",
        "hayir" => "UYAP'ta kesinleşmedi",
        _ => "UYAP'tan sorulmadı",
    }
}

fn talep_etiketi(kod: &str) -> &'static str {
    match kod {
        "gonderildi" => "Gönderildi",
        "kesinlesmis" => "Zaten kesinleşmiş",
        "tur_yok" => "Talep türü yok",
        "hata" => "Gönderilemedi",
        _ => "Gönderilmedi",
    }
}

fn mazbata_etiketi(kod: &str) -> &'static str {
    match kod {
        "dondu" => "Mazbata döndü",
        "donmedi" => "Mazbata dönmedi",
        "etebligat" => "E-tebligat",
        _ => "Ödeme emri tebligatı yok",
    }
}

/// Süzgeç sütunları: (anahtar, kodlu = yalnız listeden seçilir, sınırlı seçenek).
const SUTUNLAR: [(&str, bool, bool); 15] = [
    ("birim", false, false),
    ("dosya_no", false, true),
    ("alacakli", false, true),
    ("borclu", false, true),
    ("takip_yolu", false, false),
    ("asama", true, false),
    ("mazbata", true, false),
    ("grup", true, false),
    ("uyap", true, false),
    ("talep", true, false),
    ("yeniden", true, false),
    ("yil", true, false),
    ("acilis_yil", true, false),
    ("acilis_ay", true, false),
    ("kesinlesme_ay", true, false),
];

fn sutun_kodlu(k: &str) -> bool {
    SUTUNLAR.iter().any(|s| s.0 == k && s.1)
}

#[derive(Debug, Clone)]
pub struct Talep {
    pub sonuc: String,
    pub zaman: String,
    pub gun: Option<NaiveDate>,
    pub mesaj: String,
}

#[derive(Debug, Clone)]
pub struct Satir {
    pub bag: i64,
    pub dosya: i64,
    pub birim: String,
    pub dosya_no: String,
    yil: i32,
    sira_no: i32,
    pub acilis: Option<NaiveDate>,
    pub alacaklilar: Vec<String>,
    pub borclu: String,
    pub takip_turu: String,
    pub takip_yolu: String,
    pub asama: String,
    pub asama_tarihi: Option<NaiveDate>,
    pub teblig: String,
    pub teblig_tarihi: Option<NaiveDate>,
    pub kesinlesme: String,
    pub kesinlesme_tarihi: Option<NaiveDate>,
    pub elle_tarih: Option<NaiveDate>,
    pub uyap: Option<bool>,
    pub uyap_zamani: Option<String>,
    pub uyap_gunu: Option<NaiveDate>,
    pub talep: Option<Talep>,
    /// Ödeme emri tebligatının mazbata durumu: dondu / donmedi / etebligat / "" (tebligat yok).
    pub mazbata: &'static str,
    pub barkod: String,
    pub grup: &'static str,
    pub kalan_gun: Option<i64>,
    /// Mernis kıyası (`borclu_mernis`): uygun / farkli / elle / "" (sorgulanmadı).
    pub mernis_kategori: String,
    pub mernis_adresi: String,
    pub teblig_adresi: String,
    pub mernis_aciklama: String,
    pub mernis_zamani: Option<String>,
    /// Son yeniden tebliğ talebi (`yeniden_teblig_talebi`).
    pub yt: Option<Talep>,
    /// "Yeniden Tebliğ" sütununun kodu (bkz. [`yeniden_kodu`]).
    pub yeniden: &'static str,
}

/// "Yeniden Tebliğ" sütunu: son talep varsa onun sonucu, yoksa Mernis kıyası, yoksa ödeme emri iade ise "sorgulanmadı".
pub fn yeniden_kodu(s: &Satir) -> &'static str {
    if let Some(t) = &s.yt {
        match t.sonuc.as_str() {
            "gonderildi" => return "gonderildi",
            "zaten" => return "zaten",
            _ => {}
        }
    }
    match s.mernis_kategori.as_str() {
        "uygun" => "uygun",
        "farkli" => "farkli",
        "elle" => "elle",
        _ if s.yt.is_some() => "hata",
        _ if s.teblig == "iade" => "sorgulanmadi",
        _ => "",
    }
}

pub fn yeniden_etiketi(kod: &str) -> &'static str {
    match kod {
        "gonderildi" => "Talep gönderildi",
        "zaten" => "Dosyada talep var",
        "hata" => "Talep gönderilemedi",
        "uygun" => "21/2 şerhli tebliğe uygun",
        "farkli" => "Mernis'e çıkmamış (normal tebligat)",
        "elle" => "Elle inceleyin",
        "sorgulanmadi" => "İade: Mernis sorgulanmadı",
        _ => "—",
    }
}

/// Satırın durum grubu (bkz. modül notu).
pub fn grup_bul(s: &Satir, bugun: NaiveDate) -> &'static str {
    if s.uyap == Some(true) {
        return "uyap_kesinlesti";
    }
    let k = s.kesinlesme.as_str();
    if k == KesinlesmeDurumu::Itiraz.kod() || k.starts_with("durd_") || matches!(k, "konkordato" | "vefat" | "vasi" | "tehiri_icra") {
        return "durdu";
    }
    if let Some(t) = s.talep.as_ref().filter(|t| t.sonuc == "gonderildi") {
        return if t.gun.is_some_and(|g| g < bugun) { "talep_sonucsuz" } else { "talep_gonderildi" };
    }
    if k == KesinlesmeDurumu::Kesinlesti.kod() {
        return "kesinlestirilebilir";
    }
    if k == KesinlesmeDurumu::KesinlesmeBekliyor.kod() && s.kesinlesme_tarihi.is_some_and(|t| t > bugun) {
        return "sure_isliyor";
    }
    "bekleniyor"
}

impl Satir {
    fn degerler(&self, k: &str) -> Vec<String> {
        let bir = |x: &str| vec![x.to_string()];
        let yil = |g: Option<NaiveDate>| vec![g.map(|g| g.year().to_string()).unwrap_or_default()];
        let ay = |g: Option<NaiveDate>| vec![g.map(|g| g.month().to_string()).unwrap_or_default()];
        match k {
            "birim" => bir(&self.birim),
            "dosya_no" => bir(&self.dosya_no),
            "alacakli" => self.alacaklilar.clone(),
            "borclu" => bir(&self.borclu),
            "takip_yolu" => bir(&self.takip_yolu),
            "asama" => bir(&self.asama),
            "mazbata" => bir(self.mazbata),
            "grup" => bir(self.grup),
            "uyap" => bir(match self.uyap {
                Some(true) => "evet",
                Some(false) => "hayir",
                None => "",
            }),
            "talep" => bir(self.talep.as_ref().map(|t| t.sonuc.as_str()).unwrap_or("")),
            "yeniden" => bir(self.yeniden),
            "yil" => vec![if self.yil > 0 { self.yil.to_string() } else { String::new() }],
            "acilis_yil" => yil(self.acilis),
            "acilis_ay" => ay(self.acilis),
            "kesinlesme_ay" => ay(self.kesinlesme_tarihi),
            _ => Vec::new(),
        }
    }

    fn json(&self) -> Value {
        json!({
            "bag": self.bag, "dosya": self.dosya, "birim": self.birim, "dosya_no": self.dosya_no,
            "acilis_tarihi": self.acilis.map(|d| d.to_string()), "alacaklilar": self.alacaklilar, "borclu": self.borclu,
            "takip_turu": self.takip_turu, "takip_yolu": self.takip_yolu,
            "asama": self.asama, "asama_etiket": if self.asama.is_empty() { String::new() } else { TakipAsamasi::etiket_bul(&self.asama) },
            "asama_tarihi": self.asama_tarihi.map(|d| d.to_string()),
            "teblig": self.teblig, "teblig_tarihi": self.teblig_tarihi.map(|d| d.to_string()),
            "kesinlesme": self.kesinlesme,
            "kesinlesme_etiket": if self.kesinlesme.is_empty() { String::new() } else { KesinlesmeDurumu::etiket_bul(&self.kesinlesme) },
            "kesinlesme_tarihi": self.kesinlesme_tarihi.map(|d| d.to_string()), "elle_tarih": self.elle_tarih.map(|d| d.to_string()),
            "uyap": self.uyap, "uyap_zamani": self.uyap_zamani, "uyap_gunu": self.uyap_gunu.map(|d| d.to_string()),
            "talep": self.talep.as_ref().map(|t| json!({"sonuc": t.sonuc, "etiket": talep_etiketi(&t.sonuc), "zaman": t.zaman, "mesaj": t.mesaj})),
            "mazbata": self.mazbata, "mazbata_etiket": mazbata_etiketi(self.mazbata), "barkod": self.barkod,
            "grup": self.grup, "grup_etiket": grup_etiketi(self.grup), "kalan_gun": self.kalan_gun,
            "yeniden": self.yeniden, "yeniden_etiket": yeniden_etiketi(self.yeniden),
            "mernis": (!self.mernis_kategori.is_empty()).then(|| json!({
                "kategori": self.mernis_kategori, "etiket": crate::icra_mernis::kategori_etiketi(&self.mernis_kategori),
                "adres": self.mernis_adresi, "teblig_adresi": self.teblig_adresi, "aciklama": self.mernis_aciklama, "zaman": self.mernis_zamani})),
            "yeniden_talep": self.yt.as_ref().map(|t| json!({"sonuc": t.sonuc, "zaman": t.zaman, "mesaj": t.mesaj})),
        })
    }
}

fn etiket(k: &str, d: &str) -> String {
    match (k, d) {
        ("asama", "") => "(hesaplanmadı)".into(),
        ("asama", d) => TakipAsamasi::etiket_bul(d),
        ("mazbata", d) => mazbata_etiketi(d).into(),
        ("grup", d) => grup_etiketi(d).into(),
        ("uyap", d) => uyap_etiketi(d).into(),
        ("talep", d) => talep_etiketi(d).into(),
        ("yeniden", d) => yeniden_etiketi(d).into(),
        ("acilis_yil" | "acilis_ay" | "kesinlesme_ay", "") => "(tarihsiz)".into(),
        ("acilis_ay" | "kesinlesme_ay", d) => d.parse::<usize>().ok().and_then(|n| AYLAR.get(n.wrapping_sub(1))).map(|a| a.to_string()).unwrap_or_else(|| d.into()),
        ("takip_yolu", "") => "(okunmadı)".into(),
        (_, "") => "(boş)".into(),
        (_, d) => d.to_string(),
    }
}

/// Sekmenin kapsamındaki satırlar ve kapsam dışı kalanların sayıları.
pub struct Kapsam {
    pub satirlar: Vec<Satir>,
    /// Kapsamdaki türden dosyalarda rolü bilinmeyen (taraf bilgileri / avukat adı eksik) borçlu sayısı.
    pub rol_bilinmiyor: usize,
    pub avukat_bilinmiyor: bool,
}

/// Rolü alacaklı vekili olan, kapsamdaki türden açık dosyaların borçluları.
/// Tek dosyanın kesinleştirme satırları (borçlu başına; dosya sekmenin kapsamında değilse boş).
pub fn dosya_satirlari<C: GenericClient>(c: &mut C, dosya: i64, bugun: NaiveDate) -> Result<Vec<Satir>, String> {
    Ok(satirlari_oku(c, bugun)?.satirlar.into_iter().filter(|s| s.dosya == dosya).collect())
}

/// Dosya panelindeki "Veritabanını güncelle": UYAP'tan kesinleşme okunmalı mı (süresi dolmuş, mazbatası dönmüş ve
/// UYAP'ta kesinleşmesi görülmemiş borçlu var mı)?
pub fn kesinlesme_sorulmali(s: &Satir) -> bool {
    matches!(s.grup, "kesinlestirilebilir" | "talep_sonucsuz" | "talep_gonderildi")
}

/// Talep için kullanıcıya sorulacak borçlular (Kesinleştirme sekmesindeki "talep" işiyle aynı ölçüt).
pub fn talep_adayi(s: &Satir) -> bool {
    ise_uygun(s, "talep")
}

pub fn satirlari_oku<C: GenericClient>(c: &mut C, bugun: NaiveDate) -> Result<Kapsam, String> {
    let avukatlar = avukat_adlari(c)?;
    let taraf_ad = crate::icra_api::ad_sql("vt");
    let mts = crate::icra_api::MTS_KAYNAKLI_SQL;
    // Kapsamdaki dosyalar ve avukatın rolü (Dosya Listesi ile aynı hesap).
    let uygun = format!(
        "d.yargi_turu = 2 AND d.durum_kod = 0 AND d.tur_kod = 35 AND NOT {mts}
         AND NOT EXISTS (SELECT 1 FROM icra_takip_detay ih WHERE ih.dosya_id = d.id
                          AND lower(ih.takibin_turu_aciklama || ' ' || ih.takibin_yolu_aciklama || ' ' || ih.takibin_sekli_aciklama) ~ 'ihtiyat[iı]')"
    );
    let mut rol: HashMap<i64, &'static str> = HashMap::new();
    for s in c
        .query(
            &format!(
                "SELECT d.id, ARRAY(SELECT x.rol || chr(9) || {taraf_ad} || chr(9) || string_agg(btrim(v.ad || ' ' || v.soyad), ', ' ORDER BY dv.id)
                                     FROM dosya_taraf x JOIN taraf vt ON vt.id = x.taraf_id
                                     JOIN dosya_taraf_vekiller dv ON dv.dosyataraf_id = x.id JOIN vekil v ON v.id = dv.vekil_id
                                    WHERE x.dosya_id = d.id AND x.uyapta_gorulmedi IS NULL
                                    GROUP BY x.id, x.rol, vt.id, vt.tur, vt.unvan, vt.mersis_no, vt.vergi_no, vt.ad, vt.soyad, vt.tckn)
                   FROM dosya d WHERE {uygun}"
            ),
            &[],
        )
        .map_err(hata)?
    {
        let vekilli: Vec<VekilliTaraf> = s
            .get::<_, Vec<String>>(1)
            .into_iter()
            .filter_map(|x| {
                let mut p = x.splitn(3, '\t');
                Some(VekilliTaraf { rol: p.next()?.to_string(), ad: p.next()?.to_string(), vekiller: p.next()?.to_string() })
            })
            .collect();
        rol.insert(s.get(0), benim_rolum(&vekilli, &avukatlar).0);
    }
    let alacakli_ad = crate::icra_api::ad_sql("a");
    let borclu_ad = crate::icra_api::ad_sql("t");
    let sql = format!(
        "SELECT x.id, d.id, b.ad, d.dosya_no, d.yil, d.sira_no, d.acilis_tarihi::date, {borclu_ad},
                ARRAY(SELECT {alacakli_ad} FROM dosya_taraf y JOIN taraf a ON a.id = y.taraf_id
                       WHERE y.dosya_id = d.id AND y.rol = 'alacakli' AND y.uyapta_gorulmedi IS NULL ORDER BY y.sira, y.id),
                COALESCE(i.takibin_turu_aciklama, ''), COALESCE(i.takibin_yolu_aciklama, ''),
                x.takip_asamasi, x.takip_asamasi_tarihi, x.tebligat_durumu, x.tebligat_durum_tarihi,
                x.kesinlesme_durumu, x.kesinlesme_tarihi, x.kesinlesme_elle_tarihi,
                x.uyap_kesinlesti, to_char(x.uyap_kesinlesme_zamani, 'YYYY-MM-DD\"T\"HH24:MI'), x.uyap_kesinlesti_gunu,
                k.sonuc, to_char(k.zaman, 'YYYY-MM-DD\"T\"HH24:MI'), k.zaman::date, k.mesaj,
                oe.elektronik_tebligat, oe.tebligat_mazbatasi_var, oe.barkod,
                m.kategori, m.adres, m.teblig_adresi, m.aciklama, to_char(m.zaman, 'YYYY-MM-DD\"T\"HH24:MI'),
                y.sonuc, to_char(y.zaman, 'YYYY-MM-DD\"T\"HH24:MI'), y.zaman::date, y.mesaj
           FROM dosya_taraf x JOIN dosya d ON d.id = x.dosya_id JOIN birim b ON b.id = d.birim_id JOIN taraf t ON t.id = x.taraf_id
           LEFT JOIN icra_takip_detay i ON i.dosya_id = d.id
           LEFT JOIN LATERAL (SELECT kt.sonuc, kt.zaman, kt.mesaj FROM kesinlestirme_talebi kt
                               WHERE kt.dosyataraf_id = x.id ORDER BY kt.zaman DESC, kt.id DESC LIMIT 1) k ON true
           LEFT JOIN LATERAL (SELECT tb.elektronik_tebligat, tb.tebligat_mazbatasi_var, tb.barkod FROM tebligat_barkod tb
                               WHERE tb.borclu_id = x.id AND tb.tebligat_turu IN ('Ödeme Emri', 'Tahliye Emri')
                               ORDER BY tb.evrak_gunu DESC NULLS LAST, tb.birim_evrak_no DESC LIMIT 1) oe ON true
           LEFT JOIN borclu_mernis m ON m.dosyataraf_id = x.id
           LEFT JOIN LATERAL (SELECT yt.sonuc, yt.zaman, yt.mesaj FROM yeniden_teblig_talebi yt
                               WHERE yt.dosyataraf_id = x.id ORDER BY yt.zaman DESC, yt.id DESC LIMIT 1) y ON true
          WHERE {uygun} AND x.rol = 'borclu' AND x.uyapta_gorulmedi IS NULL"
    );
    let mut out = Kapsam { satirlar: Vec::new(), rol_bilinmiyor: 0, avukat_bilinmiyor: avukatlar.is_empty() };
    for s in c.query(&sql, &[]).map_err(hata)? {
        let dosya: i64 = s.get(1);
        match rol.get(&dosya).copied().unwrap_or("") {
            "alacakli" => {}
            "" => {
                out.rol_bilinmiyor += 1;
                continue;
            }
            _ => continue,
        }
        let talep = s.get::<_, Option<String>>(21).map(|sonuc| Talep {
            sonuc,
            zaman: s.get::<_, Option<String>>(22).unwrap_or_default(),
            gun: s.get(23),
            mesaj: s.get::<_, Option<String>>(24).unwrap_or_default(),
        });
        let (eteb, mazbata, barkod): (Option<bool>, Option<bool>, Option<String>) = (s.get(25), s.get(26), s.get(27));
        let mazbata = match (eteb, mazbata) {
            (None, _) => "",
            (Some(true), _) => "etebligat",
            (_, Some(true)) => "dondu",
            _ => "donmedi",
        };
        let mut satir = Satir {
            bag: s.get(0),
            dosya,
            birim: s.get(2),
            dosya_no: s.get(3),
            yil: s.get(4),
            sira_no: s.get(5),
            acilis: s.get(6),
            borclu: s.get(7),
            alacaklilar: s.get(8),
            takip_turu: s.get(9),
            takip_yolu: s.get(10),
            asama: s.get(11),
            asama_tarihi: s.get(12),
            teblig: s.get(13),
            teblig_tarihi: s.get(14),
            kesinlesme: s.get(15),
            kesinlesme_tarihi: s.get(16),
            elle_tarih: s.get(17),
            uyap: s.get(18),
            uyap_zamani: s.get(19),
            uyap_gunu: s.get(20),
            talep,
            mazbata,
            barkod: barkod.unwrap_or_default(),
            grup: "",
            kalan_gun: None,
            mernis_kategori: s.get::<_, Option<String>>(28).unwrap_or_default(),
            mernis_adresi: s.get::<_, Option<String>>(29).unwrap_or_default(),
            teblig_adresi: s.get::<_, Option<String>>(30).unwrap_or_default(),
            mernis_aciklama: s.get::<_, Option<String>>(31).unwrap_or_default(),
            mernis_zamani: s.get(32),
            yt: s.get::<_, Option<String>>(33).map(|sonuc| Talep {
                sonuc,
                zaman: s.get::<_, Option<String>>(34).unwrap_or_default(),
                gun: s.get(35),
                mesaj: s.get::<_, Option<String>>(36).unwrap_or_default(),
            }),
            yeniden: "",
        };
        satir.grup = grup_bul(&satir, bugun);
        satir.yeniden = yeniden_kodu(&satir);
        satir.kalan_gun = (satir.grup == "sure_isliyor").then(|| satir.kesinlesme_tarihi.map(|t| (t - bugun).num_days())).flatten();
        out.satirlar.push(satir);
    }
    Ok(out)
}

// ── Süzgeç ──────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct Filtre {
    pub acilis: (Option<NaiveDate>, Option<NaiveDate>),
    /// Dosya listesi (`icra_listeler`): yalnız bu listedeki dosyaların borçluları; None = hepsi.
    pub liste: Option<i64>,
    pub sutun: HashMap<&'static str, String>,
    /// Süzgeç sırası (Dosya Listesi ile aynı kural: bir süzgecin seçenekleri yalnız kendinden önceki süzgeçlere uyan
    /// satırlardan sayılır).
    pub sira: Vec<&'static str>,
    pub sirala: String,
    pub azalan: bool,
    pub sayfa: usize,
    pub boyut: usize,
}

impl Filtre {
    pub fn oku(q: &HashMap<String, String>) -> Filtre {
        let gun = |a: &str| q.get(a).and_then(|v| crate::icra_veri::tarih::gun_coz(v));
        let mut sutun = HashMap::new();
        for (k, _, _) in SUTUNLAR {
            if let Some(v) = q.get(k).filter(|v| !v.is_empty()) {
                sutun.insert(k, v.clone());
            }
        }
        let mut sira: Vec<&'static str> = Vec::new();
        for a in q.get("sira").map(String::as_str).unwrap_or("").split(',') {
            if let Some((k, _, _)) = SUTUNLAR.iter().find(|s| s.0 == a.trim()) {
                if sutun.contains_key(k) && !sira.contains(k) {
                    sira.push(k);
                }
            }
        }
        Filtre {
            acilis: (gun("acilis_bas"), gun("acilis_son")),
            liste: crate::icra_listeler::liste_oku(q),
            sutun,
            sira,
            sirala: q.get("sirala").cloned().unwrap_or_default(),
            azalan: q.get("yon").is_some_and(|y| y == "azalan"),
            sayfa: q.get("sayfa").and_then(|v| v.parse().ok()).unwrap_or(1),
            boyut: q.get("boyut").and_then(|v| v.parse().ok()).unwrap_or(100),
        }
    }
    fn deger(&self, k: &str) -> &str {
        self.sutun.get(k).map(String::as_str).unwrap_or("")
    }
    fn sayimda_uygula(&self, i: &str, j: &str) -> bool {
        if self.sira.is_empty() {
            return true;
        }
        match (self.sira.iter().position(|s| *s == i), self.sira.iter().position(|s| *s == j)) {
            (Some(pi), Some(pj)) => pj < pi,
            _ => true,
        }
    }
}

fn sutun_uyar(k: &str, f: &str, degerler: &[String]) -> bool {
    if f.is_empty() {
        return true;
    }
    let bos = [String::new()];
    let degerler = if degerler.is_empty() { &bos[..] } else { degerler };
    if sutun_kodlu(k) {
        return match haric_listesi(f) {
            Some(haric) => !degerler.iter().any(|d| haric.contains(&d.as_str())),
            None => degerler.iter().any(|d| f.split(AYRAC).any(|x| x == d)),
        };
    }
    if f.starts_with('!') {
        degerler.iter().all(|d| metin_uyar(f, d))
    } else {
        degerler.iter().any(|d| metin_uyar(f, d))
    }
}

fn yazi_mi(k: &str, f: &str) -> bool {
    !sutun_kodlu(k) && !f.is_empty() && !f.starts_with('=') && !f.starts_with('!')
}

fn aralikta(g: Option<NaiveDate>, (bas, son): (Option<NaiveDate>, Option<NaiveDate>)) -> bool {
    (bas.is_none() && son.is_none()) || g.is_some_and(|g| bas.is_none_or(|b| g >= b) && son.is_none_or(|s| g <= s))
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Anahtar {
    Metin(Vec<(u8, u64)>),
    Sayi(i64, i64),
    Tarih(NaiveDate),
}

fn sirala_anahtari(s: &Satir, alan: &str) -> Option<Anahtar> {
    let metin = |x: &str| (!x.trim().is_empty()).then(|| Anahtar::Metin(tr_sira_anahtari(x.trim())));
    let sira = |liste: &[&str], x: &str| liste.iter().position(|a| *a == x).map(|i| Anahtar::Sayi(i as i64, 0));
    match alan {
        "birim" => metin(&s.birim),
        "dosya_no" => Some(Anahtar::Sayi(i64::from(s.yil), i64::from(s.sira_no))),
        "alacakli" => metin(&s.alacaklilar.join(", ")),
        "borclu" => metin(&s.borclu),
        "takip_yolu" => metin(&s.takip_yolu),
        "asama" => TakipAsamasi::HEPSI.iter().position(|a| a.kod() == s.asama).map(|i| Anahtar::Sayi(i as i64, 0)),
        "mazbata" => sira(&["dondu", "etebligat", "donmedi", ""], s.mazbata),
        "grup" => sira(&GRUPLAR.map(|g| g.0), s.grup),
        "kesinlesme" => s.kesinlesme_tarihi.map(Anahtar::Tarih),
        "uyap" => s.uyap_gunu.map(Anahtar::Tarih).or(s.uyap.map(|u| Anahtar::Sayi(i64::from(!u), 0))),
        "talep" => s.talep.as_ref().map(|t| Anahtar::Metin(tr_sira_anahtari(&t.zaman))),
        "yeniden" => sira(&["uygun", "farkli", "hata", "sorgulanmadi", "elle", "gonderildi", "zaten"], s.yeniden),
        "acilis" => s.acilis.map(Anahtar::Tarih),
        _ => None,
    }
}

const SIRALANAN: [&str; 13] =
    ["birim", "dosya_no", "alacakli", "borclu", "takip_yolu", "asama", "mazbata", "grup", "kesinlesme", "uyap", "talep", "yeniden", "acilis"];

/// Seçilen sütuna göre (boşlar sonda); eşitlikte daire, dosya no, borçlu. Varsayılan: kesinleşme günü (en eski önce).
fn sirala(satirlar: &mut [Satir], alan: &str, azalan: bool) {
    let (alan, azalan) = if SIRALANAN.contains(&alan) { (alan, azalan) } else { ("kesinlesme", false) };
    let daire = |a: &Satir, b: &Satir| {
        tr_sira_anahtari(&a.birim)
            .cmp(&tr_sira_anahtari(&b.birim))
            .then((a.yil, a.sira_no).cmp(&(b.yil, b.sira_no)))
            .then(a.bag.cmp(&b.bag))
    };
    satirlar.sort_by(|a, b| {
        match (sirala_anahtari(a, alan), sirala_anahtari(b, alan)) {
            (Some(x), Some(y)) => {
                let o = x.cmp(&y);
                if azalan { o.reverse() } else { o }
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| daire(a, b))
    });
}

type Sayim = HashMap<&'static str, HashMap<String, i64>>;

fn suz(satirlar: Vec<Satir>, f: &Filtre) -> (Vec<Satir>, Sayim) {
    let mut sayim: Sayim = HashMap::new();
    let mut gecen = Vec::new();
    for s in satirlar {
        if !aralikta(s.acilis, f.acilis) {
            continue;
        }
        let uyum: Vec<bool> = SUTUNLAR.iter().map(|c| sutun_uyar(c.0, f.deger(c.0), &s.degerler(c.0))).collect();
        for (i, c) in SUTUNLAR.iter().enumerate() {
            let digerleri = uyum.iter().enumerate().all(|(j, u)| j == i || *u || !f.sayimda_uygula(c.0, SUTUNLAR[j].0));
            if digerleri && (!yazi_mi(c.0, f.deger(c.0)) || uyum[i]) {
                let say = sayim.entry(c.0).or_default();
                let tekil: HashSet<String> = s.degerler(c.0).into_iter().collect();
                for d in tekil {
                    *say.entry(d).or_default() += 1;
                }
            }
        }
        if uyum.iter().all(|x| *x) {
            gecen.push(s);
        }
    }
    sirala(&mut gecen, &f.sirala, f.azalan);
    (gecen, sayim)
}

pub fn kesinlestirme_listesi<C: GenericClient>(c: &mut C, f: &Filtre, bugun: NaiveDate) -> Result<Value, String> {
    let mut k = satirlari_oku(c, bugun)?;
    k.satirlar = crate::icra_listeler::suz(k.satirlar, c, f.liste, |s| s.dosya)?;
    let genel_toplam = k.satirlar.len();
    let (satirlar, mut sayim) = suz(k.satirlar, f);
    let toplam = satirlar.len();
    let boyut = f.boyut.clamp(1, 500);
    let sayfa = f.sayfa.max(1);
    let dilim: Vec<Value> = satirlar.iter().skip((sayfa - 1) * boyut).take(boyut).map(Satir::json).collect();
    let mut secenekler = serde_json::Map::new();
    for (a, _, sinirli) in SUTUNLAR {
        let say = sayim.remove(a).unwrap_or_default();
        let liste = secenek_sinirli(say, &|d| etiket(a, d), if sinirli { SECENEK_SINIRI } else { usize::MAX });
        secenekler.insert(a.to_string(), Value::Array(liste));
    }
    let gruplar: Vec<Value> = GRUPLAR.iter().map(|(k, e)| json!({"kod": k, "etiket": e})).collect();
    Ok(json!({"toplam": toplam, "sayfa": sayfa, "boyut": boyut, "satirlar": dilim, "secenekler": secenekler,
              "secenek_siniri": SECENEK_SINIRI, "genel_toplam": genel_toplam, "gruplar": gruplar,
              "rol_bilinmiyor": k.rol_bilinmiyor, "avukat_bilinmiyor": k.avukat_bilinmiyor, "bugun": bugun.to_string()}))
}

/// Bir işin istek atacağı satırlar: talep → kesinleştirilebilir ya da talebi sonuçsuz kalan borçlular; kesinlesme →
/// UYAP'ta henüz kesinleşmemiş ve itiraz / durdurma olmayan borçlular.
fn ise_uygun(s: &Satir, is_: &str) -> bool {
    match is_ {
        "talep" => matches!(s.grup, "kesinlestirilebilir" | "talep_sonucsuz"),
        "kesinlesme" => !matches!(s.grup, "uyap_kesinlesti" | "durdu"),
        // Ödeme emri iade dönmüş, takip durmamış, yeniden tebliğ talebi gönderilmemiş borçlular.
        "mernis" => {
            s.teblig == "iade"
                && !matches!(s.grup, "uyap_kesinlesti" | "durdu")
                && !matches!(s.yeniden, "gonderildi" | "zaten")
                && !matches!(s.mernis_kategori.as_str(), "uygun" | "farkli")
        }
        "teblig" => {
            s.teblig == "iade"
                && !matches!(s.grup, "uyap_kesinlesti" | "durdu")
                && crate::icra_mernis::talep_turu(&s.mernis_kategori).is_some()
                && !matches!(s.yeniden, "gonderildi" | "zaten")
        }
        _ => false,
    }
}

/// Süzgeçteki satırlardan işe uygun olanların dosyaları (listedeki sırayla, tekil) ve borçluları.
pub fn kimlikler<C: GenericClient>(c: &mut C, f: &Filtre, is_: &str, bugun: NaiveDate) -> Result<(Vec<i64>, Vec<i64>), String> {
    if !["talep", "kesinlesme", "mernis", "teblig"].contains(&is_) {
        return Err("İş türü geçersiz.".into());
    }
    let (satirlar, _) = suz(crate::icra_listeler::suz(satirlari_oku(c, bugun)?.satirlar, c, f.liste, |s| s.dosya)?, f);
    let mut dosyalar: Vec<i64> = Vec::new();
    let mut gorulen: HashSet<i64> = HashSet::new();
    let mut borclular = Vec::new();
    for s in satirlar.iter().filter(|s| ise_uygun(s, is_)) {
        if gorulen.insert(s.dosya) {
            dosyalar.push(s.dosya);
        }
        borclular.push(s.bag);
    }
    Ok((dosyalar, borclular))
}

/// Başlatmadan önce tahmin: kaç dosya / borçlu, en az istek ve süre.
pub fn istek_tahmini<C: GenericClient>(c: &mut C, f: &Filtre, is_: &str, bugun: NaiveDate) -> Result<Value, String> {
    use crate::icra_guncelle::{ISTEK_ARASI_SN, MOLA_HER_DOSYA, MOLA_SN, SAYFA_BOYU};
    let (dosyalar, borclular) = kimlikler(c, f, is_, bugun)?;
    let (satir, _) = suz(crate::icra_listeler::suz(satirlari_oku(c, bugun)?.satirlar, c, f.liste, |s| s.dosya)?, f);
    let acik: i64 = c.query_one("SELECT count(*) FROM dosya WHERE yargi_turu = 2 AND durum_kod = 0", &[]).map_err(hata)?.get(0);
    let sayfa = (acik.max(0) as usize).div_ceil(SAYFA_BOYU).max(1);
    let (n, m) = (dosyalar.len(), borclular.len());
    // Dosyaları UYAP'ta bulma (`icra_guncelle::secili_kayitlar`): tek tek ya da listeyi bir kez almak, hangisi azsa.
    let bulma = if n == 0 { 0 } else { n.min(sayfa) };
    let (istek, birim) = match is_ {
        "talep" => (bulma + 2 * n + 3 * m, m),
        // Dosya başına borçlu + evrak listesi; borçlu başına Mernis + ödeme emri zarfı.
        "mernis" => (bulma + 2 * n + 2 * m, m),
        // Dosya başına borçlu listesi; borçlu başına taze evrak listesi + hazırla + imza + gönder.
        "teblig" => (bulma + n + 4 * m, m),
        _ => (bulma + n, n),
    };
    let tutar: f64 = if is_ == "teblig" {
        let secilen: HashSet<i64> = borclular.iter().copied().collect();
        satir.iter().filter(|s| secilen.contains(&s.bag)).filter_map(|s| crate::icra_mernis::talep_turu(&s.mernis_kategori)).map(|t| t.2).sum()
    } else {
        0.0
    };
    let (uygun_n, farkli_n) = if is_ == "teblig" {
        let secilen: HashSet<i64> = borclular.iter().copied().collect();
        let x = |k: &str| satir.iter().filter(|s| secilen.contains(&s.bag) && s.mernis_kategori == k).count();
        (x("uygun"), x("farkli"))
    } else {
        (0, 0)
    };
    let sure_sn = (istek as f64 * ISTEK_ARASI_SN) as u64 + (birim / MOLA_HER_DOSYA) as u64 * MOLA_SN;
    Ok(json!({"is": is_, "satir": satir.len(), "dosya": n, "borclu": m, "istek": istek, "sure_sn": sure_sn,
              "tutar": tutar, "uygun": uygun_n, "farkli": farkli_n}))
}

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

/// Gerçek veritabanı + sahte UYAP: liste, talep gönderimi (biri gönderilir, biri UYAP'ta zaten kesinleşmiş), aynı gün
/// ikinci gönderimin engellenmesi, UYAP'tan kesinleşme okuma ve iş kapatma. `veritabani::tests::uctan_uca`'dan çağrılır
/// (hazır iş şablonları yüklendikten sonra).
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use crate::icra_guncelle::{akis, Kapsam, Mod, SahteUyap, ARAMA};
    use crate::icra_veri::{dosya_kunyesi_kaydet, YargiTuru};
    use std::sync::Arc;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let bugun = chrono::Local::now().date_naive();
    let sessiz = |_: &str| {};
    const BIRIM: &str = "Kesin 1. İcra Dairesi";
    let birim = BIRIM;
    let kunye = json!({"dosyaId": "eski-k", "dosyaNo": "2026/7001", "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": 35,
                       "dosyaTur": "İcra Dosyası", "birimAdi": birim, "birimId": 7007, "dosyaAcilisTarihi": "01.09.2026"});
    let (d, _) = dosya_kunyesi_kaydet(c, &kunye, Some(YargiTuru::Icra.kod())).unwrap();
    crate::icra_dosyalar::avukat_ekle(c, "UTKU AVUKAT").unwrap();
    crate::icra_kayit::dosya_taraf_kaydet(c, d, &[
        json!({"adi": "K FAKTORİNG A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum", "vekil": "[UTKU AVUKAT]"}),
        json!({"adi": "VİVEN GRUP İNŞAAT LTD. ŞTİ.", "rol": "Borçlu", "kisiKurum": "Kurum"}),
        json!({"adi": "AYŞE KAYA", "rol": "Borçlu", "kisiKurum": "Kişi"}),
    ], &sessiz).unwrap();
    let bag = |c: &mut postgres::Client, ad: &str| -> i64 {
        c.query_one("SELECT x.id FROM dosya_taraf x JOIN taraf t ON t.id = x.taraf_id WHERE x.dosya_id = $1 AND x.rol = 'borclu'
                      AND (t.unvan = $2 OR btrim(t.ad || ' ' || t.soyad) = $2)", &[&d, &ad]).unwrap().get(0)
    };
    let (b1, b2) = (bag(c, "VİVEN GRUP İNŞAAT LTD. ŞTİ."), bag(c, "AYŞE KAYA"));
    c.execute("UPDATE taraf SET vergi_no = '6120406157' WHERE id = (SELECT taraf_id FROM dosya_taraf WHERE id = $1)", &[&b1]).unwrap();
    c.execute("UPDATE dosya_taraf SET kesinlesme_durumu = 'kesinlesti', kesinlesme_tarihi = $2 WHERE id = $1", &[&b1, &(bugun - chrono::Duration::days(3))]).unwrap();
    c.execute("UPDATE dosya_taraf SET kesinlesme_durumu = 'kesinlesme_bekliyor' WHERE id = $1", &[&b2]).unwrap();
    // AYŞE KAYA için açık "Kesinleştirme talebi gönder" işi: UYAP'ta kesinleşmiş görülünce kapanmalı.
    c.execute("INSERT INTO sablon (anahtar, baslik) VALUES ('h-kes-gonder', 'Kesinleştirme talebi gönder') ON CONFLICT (anahtar) DO NOTHING", &[]).unwrap();
    let is_: i64 = c
        .query_one("INSERT INTO gorev (baslik, son_tarih, dosya_id, taraf_id, sablon_id)
                    VALUES ('AYŞE KAYA – Kesinleştirme talebi gönder', $1, $2, $3, (SELECT id FROM sablon WHERE anahtar = 'h-kes-gonder')) RETURNING id",
                   &[&bugun, &d, &b2]).unwrap().get(0);

    // 1) Liste: yalnız bu dairenin iki borçlusu; VİVEN kesinleştirilebilir, AYŞE bekliyor.
    let q = |ek: &[(&str, &str)]| -> HashMap<String, String> {
        let mut m: HashMap<String, String> = ek.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        m.insert("birim".into(), format!("={birim}"));
        m
    };
    let l = kesinlestirme_listesi(c, &Filtre::oku(&q(&[])), bugun).unwrap();
    assert_eq!(l["toplam"], 2, "{l}");
    let grup = |l: &Value, b: i64| l["satirlar"].as_array().unwrap().iter().find(|s| s["bag"] == b).map(|s| s["grup"].clone()).unwrap();
    assert_eq!((grup(&l, b1), grup(&l, b2)), (json!("kesinlestirilebilir"), json!("bekleniyor")));
    assert_eq!(kimlikler(c, &Filtre::oku(&q(&[])), "talep", bugun).unwrap(), (vec![d], vec![b1]));
    assert_eq!(kimlikler(c, &Filtre::oku(&q(&[("grup", "kesinlestirilebilir")])), "kesinlesme", bugun).unwrap(), (vec![d], vec![b1]));

    // 2) Talep: VİVEN'e gönderilir; AYŞE KAYA UYAP'ta zaten kesinleşmiş (gönderilmez, iş kapanır).
    let yanit = || -> Box<dyn Fn(&str, &Value) -> Result<Option<Value>, String> + Send + Sync> {
        Box::new(|uc: &str, g: &Value| -> Result<Option<Value>, String> {
            Ok(Some(match uc {
                ARAMA if g["dosyaSira"] == 7001 && g["dosyaDurumKod"] == 0 => json!([[{"dosyaId": "u-k", "dosyaNo": "2026/7001", "dosyaDurumKod": 0,
                    "dosyaDurum": "Açık", "dosyaTurKod": 35, "dosyaTur": "İcra Dosyası", "birimAdi": BIRIM, "birimId": 7007}], 1]),
                ARAMA => json!([[], 0]),
                BORCLU_LISTESI => json!([{"kisiKurumId": 501, "turu": 1, "kurumDVO": {"kurumAdi": "VİVEN GRUP İNŞAAT LİMİTED ŞİRKETİ", "vergiNo": "6120406157"}},
                                         {"kisiKurumId": 502, "turu": 0, "kisiTumDVO": {"adi": "AYŞE", "soyadi": "KAYA", "tcKimlikNo": "12345678901"}}]),
                KESINLESME_DENETIMI => json!({"islemSonucu": "", "basarilimi": g["kisiKurumId"] == 502}),
                TALEP_LISTESI => json!([{"grupKodu": 2, "talepKodu": 13, "talepAdi": "Dosya Hesabının Yapılması", "talepMasrafi": 0},
                                        {"grupKodu": 2, "talepKodu": 31, "talepAdi": "Takibin Kesinleşmesi", "talepMasrafi": 0}]),
                TALEP_HAZIRLA => json!("PK\u{3}\u{4}udf"),
                TALEP_GONDER => {
                    assert_eq!((g["kisiKurumId"].clone(), g["dosyaId"].clone(), g["talepGrupId"].clone()), (json!("501"), json!("u-k"), json!("2")));
                    assert!(g["file_1"]["icerik"].as_str().unwrap().starts_with("IMZALI:PK"), "imzalı UDF gönderilir: {g}");
                    json!({"type": "success", "message": "<b>Evrak</b> gönderildi."})
                }
                KESINLESEN_LISTESI => json!([{"tarafAdi": "VİVEN GRUP İNŞAAT LTD. ŞTİ.", "dosyaKisiDVO": {"tarafTC": "6120406157"}, "guncelKisiKurumId": 501},
                                             {"tarafAdi": "AYŞE KAYA", "dosyaKisiDVO": {"tarafTC": "12345678901"}, "guncelKisiKurumId": 502}]),
                _ => return Err(format!("beklenmeyen istek: {uc} {g}")),
            }))
        })
    };
    let calistir = |mod_: Mod, borclular: Option<Vec<i64>>| {
        let mut t = Arc::try_unwrap(crate::icra_guncelle::Tarama::new(mod_, Kapsam { dosyalar: Some(vec![d]), borclular, ..Default::default() })).ok().unwrap();
        t.mola_her = 1000;
        let u = SahteUyap::yeni(yanit());
        let r = rt.block_on(akis(&t, &u));
        (t, u, r)
    };
    let (t, u, r) = calistir(Mod::Talep, Some(vec![b1, b2]));
    r.unwrap();
    {
        let g = t.inner.lock().unwrap();
        assert_eq!((g.talep_gonderilen, g.talep_kesinlesmis, g.talep_hatasi, g.kes_kapanan_is), (1, 1, 0, 1), "{:?}", g.uyarilar);
        assert!(bitis_mesaji(&g).contains("1 kesinleştirme talebi gönderildi"));
    }
    assert_eq!((u.say("IMZA"), u.say(TALEP_GONDER), u.say(BORCLU_LISTESI), u.say(TALEP_LISTESI)), (1, 1, 1, 1), "dosya başına bir kez");
    let son = |c: &mut postgres::Client, b: i64| -> (String, String) {
        let s = c.query_one("SELECT sonuc, mesaj FROM kesinlestirme_talebi WHERE dosyataraf_id = $1 ORDER BY id DESC LIMIT 1", &[&b]).unwrap();
        (s.get(0), s.get(1))
    };
    assert_eq!(son(c, b1), ("gonderildi".into(), "Evrak gönderildi.".into()));
    assert_eq!(son(c, b2).0, "kesinlesmis");
    let durum: String = c.query_one("SELECT durum FROM gorev WHERE id = $1", &[&is_]).unwrap().get(0);
    assert_eq!(durum, "tamamlandi", "UYAP'ta kesinleşince kesinleştirme işi kapanır");
    let l = kesinlestirme_listesi(c, &Filtre::oku(&q(&[])), bugun).unwrap();
    assert_eq!((grup(&l, b1), grup(&l, b2)), (json!("talep_gonderildi"), json!("uyap_kesinlesti")));
    assert!(kimlikler(c, &Filtre::oku(&q(&[])), "talep", bugun).unwrap().1.is_empty(), "gönderilen ve kesinleşen yeniden önerilmez");

    // 3) Aynı gün ikinci gönderim engellenir (UYAP'a talep gitmez).
    let (t, u, r) = calistir(Mod::Talep, Some(vec![b1]));
    r.unwrap();
    assert_eq!((t.inner.lock().unwrap().talep_atlanan, u.say(TALEP_GONDER), u.say("IMZA")), (1, 0, 0));

    // 4) UYAP'tan kesinleşme okuma: iki borçlu da kesinleşmiş.
    let (t, u, r) = calistir(Mod::Kesinlesme, None);
    r.unwrap();
    let (kd, kk) = {
        let g = t.inner.lock().unwrap();
        (g.kes_dosya, g.kes_kesinlesen)
    };
    assert_eq!((kd, kk, u.say(KESINLESEN_LISTESI)), (1, 2, 1));
    let l = kesinlestirme_listesi(c, &Filtre::oku(&q(&[("grup", "uyap_kesinlesti")])), bugun).unwrap();
    assert_eq!(l["toplam"], 2);
    assert_eq!(l["satirlar"][0]["uyap_gunu"], json!(bugun.to_string()));
    // Talep sonucu süzgeci ve istek tahmini.
    let t = istek_tahmini(c, &Filtre::oku(&q(&[])), "kesinlesme", bugun).unwrap();
    assert_eq!((t["dosya"].clone(), t["borclu"].clone()), (json!(0), json!(0)), "UYAP'ta kesinleşenler yeniden sorulmaz");

    // 5) Dosya paneli "Veritabanını güncelle" (Mod::Dosya + kesinleşme adımı). VİVEN yeniden kesinleştirilebilir
    //    (elle kesinleşme günü durum hesabından sonra da kalır), talebi yok, UYAP'ta hiç sorulmamış.
    c.execute("DELETE FROM kesinlestirme_talebi WHERE dosyataraf_id = $1", &[&b1]).unwrap();
    c.execute("UPDATE dosya_taraf SET uyap_kesinlesti = NULL, uyap_kesinlesti_gunu = NULL, kesinlesme_elle_tarihi = $2 WHERE id = $1",
              &[&b1, &(bugun - chrono::Duration::days(3))]).unwrap();
    let panel = |kesinlesen: Value| {
        let mut t = Arc::try_unwrap(crate::icra_guncelle::Tarama::new(Mod::Dosya, Kapsam::default())).ok().unwrap();
        {
            let mut g = t.inner.lock().unwrap();
            g.dosya = Some(d);
            g.dosya_kesinlesme = true;
        }
        t.mola_her = 1000;
        let ic = yanit();
        let u = SahteUyap::yeni(Box::new(move |uc: &str, g: &Value| match uc {
            KESINLESEN_LISTESI => Ok(Some(kesinlesen.clone())),
            crate::icra_guncelle::DOSYA_AYRINTI => Ok(Some(json!({}))),
            _ => ic(uc, g),
        }));
        let r = rt.block_on(akis(&t, &u));
        (t, u, r)
    };
    // 5a) UYAP'ta kesinleşme yok → VİVEN talep adayı; talep gönderilmez (yalnız onayla).
    let (t, u, r) = panel(json!([]));
    r.unwrap();
    {
        let g = t.inner.lock().unwrap();
        assert_eq!(g.talep_adaylari.iter().map(|a| a["bag"].as_i64().unwrap()).collect::<Vec<_>>(), vec![b1], "{:?} {:?}", g.rapor, g.uyarilar);
        assert!(g.rapor.iter().any(|x| x.contains("onayınız bekleniyor")), "{:?}", g.rapor);
    }
    assert_eq!((u.say(KESINLESEN_LISTESI), u.say(TALEP_GONDER), u.say("IMZA")), (1, 0, 0));
    // 5b) UYAP'ta kesinleşmiş görünür → aday yok.
    let (t, u, r) = panel(json!([{"tarafAdi": "VİVEN GRUP İNŞAAT LTD. ŞTİ.", "dosyaKisiDVO": {"tarafTC": "6120406157"}, "guncelKisiKurumId": 501}]));
    r.unwrap();
    assert!(t.inner.lock().unwrap().talep_adaylari.is_empty());
    assert_eq!(u.say(KESINLESEN_LISTESI), 1);
    // 5c) Kesinleştirilebilir borçlu kalmadı → UYAP'a sorulmaz.
    let (t, u, r) = panel(json!([]));
    r.unwrap();
    assert_eq!(u.say(KESINLESEN_LISTESI), 0);
    assert!(t.inner.lock().unwrap().rapor.iter().any(|x| x.contains("UYAP'a sorulmadı")));

    // 6) Mernis + yeniden tebliğ (AYŞE KAYA: ödeme emri iade). Zarf adresi Mernis adresiyle aynı → 21/2 uygun.
    c.execute("UPDATE dosya_taraf SET uyap_kesinlesti = NULL, uyap_kesinlesti_gunu = NULL, tebligat_durumu = 'iade' WHERE id = $1", &[&b2]).unwrap();
    let oe: i64 = c
        .query_one("INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, borclu_id, evrak_tarihi, evrak_gunu, barkod, tebligat_turu, ptt_durumu)
                    VALUES ($1, 40, $2, '01/10/2026', '2026-10-01', '40011', 'Ödeme Emri', 'İADE') RETURNING id", &[&d, &b2]).unwrap().get(0);
    c.execute("UPDATE tebligat_barkod SET son_islem_tarihi = '06/10/2026 10:00', son_islem_zamani = '2026-10-06 10:00+03', teslim_sekli = 'ADRESTE TANINMIYOR', evrak_aciklama = 'Normal Tebligat[BORÇLU AYŞE KAYA]' WHERE id = $1", &[&oe]).unwrap();
    crate::icra_durum::durumlari_guncelle(c, Some(d), bugun, &|_: &str| {}).unwrap();
    let td: String = c.query_one("SELECT tebligat_durumu FROM dosya_taraf WHERE id = $1", &[&b2]).unwrap().get(0);
    assert_eq!(td, "iade", "durum hesabı iade ödeme emrini görür");
    let mernis_yaniti = json!({"sorguSonucDVO": {"mernisAdres": {"adresler": [{"ilIlceMerkeziAdresi":
        {"mahalle": "BOSTANLI MAH.", "csbm": "1720 SK.", "disKapiNo": "5", "icKapiNo": "", "ilce": "KARŞIYAKA", "il": "İZMİR"}}]}}});
    let zarf = teb::ornek_pdf(b"(BORCLU AYSE KAYA TC:1234*****90 ) Tj (Bostanli Mah. 1720 Sk. No:5 Karsiyaka/ Izmir ) Tj (BU ZARFTA ODEME EMRI VARDIR) Tj");
    let mernis_isi = |mod_: Mod, talep_evraki: bool, yeniden: bool| {
        let mut t = Arc::try_unwrap(crate::icra_guncelle::Tarama::new(mod_, Kapsam { dosyalar: Some(vec![d]), borclular: Some(vec![b2]), yeniden, ..Default::default() })).ok().unwrap();
        t.mola_her = 1000;
        let ic = yanit();
        let my = mernis_yaniti.clone();
        let mut u = SahteUyap::yeni(Box::new(move |uc: &str, g: &Value| match uc {
            crate::icra_mernis::MERNIS_SORGUSU => {
                assert_eq!(g["kisiKurumId"], 502, "{g}");
                Ok(Some(my.clone()))
            }
            crate::evrak_toplu::EVRAK_LISTESI_TOPLAM => Ok(Some(json!(1))),
            crate::evrak_toplu::EVRAK_LISTESI => {
                let mut l = vec![json!({"evrakId": "e40", "birimEvrakNo": 40, "tur": "Kapalı Tebligat", "aciklama": "Normal Tebligat[AYŞE KAYA]", "onaylandigiTarih": "01/10/2026"})];
                if talep_evraki {
                    l.push(json!({"evrakId": "e50", "birimEvrakNo": 50, "tur": teb::TALEP_EVRAK_TUR, "aciklama": "", "onaylandigiTarih": "20/10/2026"}));
                }
                Ok(Some(json!({"tumEvraklar": {"Gelen": l}})))
            }
            crate::evrak_toplu::DOC_VIEWER_HAZIRLIK => Ok(Some(json!({}))),
            TALEP_HAZIRLA => {
                let b: Value = serde_json::from_str(g["talepBilgileri"].as_str().unwrap()).unwrap();
                assert_eq!((b[0]["tebligatTuru"].clone(), b[0]["adresTuru"].clone()), (json!(2), json!(1)), "21/2, Mernis adresi");
                Ok(Some(json!("<template><content/><data>talep</data></template>")))
            }
            TALEP_GONDER => {
                assert_eq!((g["odemeTipi"].clone(), g["talepGrupId"].clone(), g["tutar"].clone(), g["kisiKurumId"].clone()),
                           (json!("7"), json!("1"), json!("265"), json!("502")), "{g}");
                assert!(g["items"].as_str().unwrap().contains("ICR_AVUKAT_PORTAL_TEBLIGAT_TALEP"));
                Ok(Some(json!({"type": "success", "message": "Talep gönderildi."})))
            }
            _ => ic(uc, g),
        }));
        let z = zarf.clone();
        u.indir_yanit = Box::new(move |_| Ok((200, z.clone())));
        let r = rt.block_on(akis(&t, &u));
        (t, u, r)
    };
    let l = kesinlestirme_listesi(c, &Filtre::oku(&q(&[])), bugun).unwrap();
    assert_eq!(kimlikler(c, &Filtre::oku(&q(&[])), "mernis", bugun).unwrap(), (vec![d], vec![b2]), "{l}");
    assert!(kimlikler(c, &Filtre::oku(&q(&[])), "teblig", bugun).unwrap().1.is_empty(), "Mernis sorulmadan talep yok");
    // 6a) Mernis sorgusu: adres alınır, zarf PDF'i okunur, kıyas "uygun".
    let (t, u, r) = mernis_isi(Mod::Mernis, false, false);
    r.unwrap();
    {
        let g = t.inner.lock().unwrap();
        assert_eq!((g.mer_sorulan, g.mer_uygun, g.mer_hata), (1, 1, 0), "{:?}", g.uyarilar);
    }
    assert_eq!((u.say(crate::icra_mernis::MERNIS_SORGUSU), u.say("GET view_document_brd.uyap"), u.say(TALEP_GONDER)), (1, 1, 0));
    let (kat, adres, zarf_adr): (String, String, String) = {
        let s = c.query_one("SELECT kategori, adres, teblig_adresi FROM borclu_mernis WHERE dosyataraf_id = $1", &[&b2]).unwrap();
        (s.get(0), s.get(1), s.get(2))
    };
    assert_eq!(kat, "uygun");
    assert!(adres.starts_with("BOSTANLI MAH. 1720 SK. No:5") && zarf_adr.starts_with("Bostanli Mah. 1720 Sk."), "{adres} / {zarf_adr}");
    let t212: String = c.query_one("SELECT t212_teblig_adresi FROM tebligat_barkod WHERE id = $1", &[&oe]).unwrap().get(0);
    assert_eq!(t212, zarf_adr, "zarf adresi tebligatta saklanır");
    let l = kesinlestirme_listesi(c, &Filtre::oku(&q(&[("yeniden", "uygun")])), bugun).unwrap();
    assert_eq!((l["toplam"].clone(), l["satirlar"][0]["mernis"]["kategori"].clone()), (json!(1), json!("uygun")), "{l}");
    // 6b) Yeniden sorgu (toplu): kesin sonuçlu borçlu UYAP'a sorulmaz; panelden "yeniden" sorulur.
    assert!(kimlikler(c, &Filtre::oku(&q(&[])), "mernis", bugun).unwrap().1.is_empty());
    let (t, u, r) = mernis_isi(Mod::Mernis, false, false);
    r.unwrap();
    assert_eq!((u.say(crate::icra_mernis::MERNIS_SORGUSU), t.inner.lock().unwrap().mer_onbellek), (0, 1));
    let (_, u, r) = mernis_isi(Mod::Mernis, false, true);
    r.unwrap();
    assert_eq!((u.say(crate::icra_mernis::MERNIS_SORGUSU), u.say("GET view_document_brd.uyap")), (1, 0), "zarf adresi kayıtlı: PDF yeniden indirilmez");
    // 6c) Yeniden tebliğ talebi: tahmin 265 TL; gönderilir; ikinci kez gönderilmez.
    assert_eq!(kimlikler(c, &Filtre::oku(&q(&[])), "teblig", bugun).unwrap(), (vec![d], vec![b2]));
    let tah = istek_tahmini(c, &Filtre::oku(&q(&[])), "teblig", bugun).unwrap();
    assert_eq!((tah["borclu"].clone(), tah["tutar"].clone(), tah["uygun"].clone()), (json!(1), json!(265.0), json!(1)), "{tah}");
    let (t, u, r) = mernis_isi(Mod::YenidenTeblig, false, false);
    r.unwrap();
    {
        let g = t.inner.lock().unwrap();
        assert_eq!((g.yt_gonderilen, g.yt_tutar as i64, g.yt_hatasi), (1, 265, 0), "{:?}", g.uyarilar);
    }
    assert_eq!((u.say("IMZA"), u.say(TALEP_GONDER)), (1, 1));
    let (sonuc, tur): (String, i16) = {
        let s = c.query_one("SELECT sonuc, tur FROM yeniden_teblig_talebi WHERE dosyataraf_id = $1 ORDER BY id DESC LIMIT 1", &[&b2]).unwrap();
        (s.get(0), s.get(1))
    };
    assert_eq!((sonuc.as_str(), tur), ("gonderildi", 2));
    assert!(c.query_one("SELECT yeniden_teblig_talep_edildi FROM tebligat_barkod WHERE id = $1", &[&oe]).unwrap().get::<_, bool>(0));
    assert!(kimlikler(c, &Filtre::oku(&q(&[])), "teblig", bugun).unwrap().1.is_empty(), "gönderilen yeniden önerilmez");
    let (t, u, r) = mernis_isi(Mod::YenidenTeblig, false, false);
    r.unwrap();
    assert_eq!((u.say(TALEP_GONDER), t.inner.lock().unwrap().yt_atlanan), (0, 1), "aynı borçluya ikinci talep gitmez");
    // 6d) Dosyada iade sonrası tebligat talebi evrakı varsa (iki borçlulu dosya): gönderilmez.
    c.execute("DELETE FROM yeniden_teblig_talebi WHERE dosyataraf_id = $1", &[&b2]).unwrap();
    let (t, u, r) = mernis_isi(Mod::YenidenTeblig, true, false);
    r.unwrap();
    assert_eq!((u.say(TALEP_HAZIRLA), u.say(TALEP_GONDER), t.inner.lock().unwrap().yt_atlanan), (0, 0, 1));
    assert!(t.inner.lock().unwrap().uyarilar.iter().any(|x| x.contains("ayırt edilemiyor")));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn satir(kes: &str, tarih: Option<NaiveDate>) -> Satir {
        Satir {
            bag: 1, dosya: 1, birim: "İzmir 1. İcra Dairesi".into(), dosya_no: "2026/1".into(), yil: 2026, sira_no: 1, acilis: None,
            alacaklilar: vec![], borclu: "AHMET YILMAZ".into(), takip_turu: String::new(), takip_yolu: String::new(), asama: String::new(),
            asama_tarihi: None, teblig: String::new(), teblig_tarihi: None, kesinlesme: kes.into(), kesinlesme_tarihi: tarih,
            elle_tarih: None, uyap: None, uyap_zamani: None, uyap_gunu: None, talep: None, mazbata: "", barkod: String::new(),
            grup: "", kalan_gun: None, mernis_kategori: String::new(), mernis_adresi: String::new(), teblig_adresi: String::new(),
            mernis_aciklama: String::new(), mernis_zamani: None, yt: None, yeniden: "",
        }
    }

    #[test]
    fn grup_onceligi() {
        let bugun = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let dun = bugun.pred_opt().unwrap();
        let yarin = bugun.succ_opt().unwrap();
        assert_eq!(grup_bul(&satir("kesinlesti", Some(dun)), bugun), "kesinlestirilebilir");
        assert_eq!(grup_bul(&satir("kesinlesme_bekliyor", Some(yarin)), bugun), "sure_isliyor");
        assert_eq!(grup_bul(&satir("kesinlesme_bekliyor", None), bugun), "bekleniyor", "mazbata dönmedi / tebliğ günü yok");
        assert_eq!(grup_bul(&satir("", None), bugun), "bekleniyor");
        assert_eq!(grup_bul(&satir("itiraz", None), bugun), "durdu");
        assert_eq!(grup_bul(&satir("durd_iflas", None), bugun), "durdu");
        let mut s = satir("kesinlesti", Some(dun));
        s.talep = Some(Talep { sonuc: "gonderildi".into(), zaman: String::new(), gun: Some(bugun), mesaj: String::new() });
        assert_eq!(grup_bul(&s, bugun), "talep_gonderildi", "gönderildiği gün");
        s.talep.as_mut().unwrap().gun = Some(dun);
        assert_eq!(grup_bul(&s, bugun), "talep_sonucsuz", "ertesi gün hâlâ kesinleşme görülmedi");
        s.talep.as_mut().unwrap().sonuc = "hata".into();
        assert_eq!(grup_bul(&s, bugun), "kesinlestirilebilir", "gönderilemeyen talep yeniden gönderilebilir");
        // UYAP her şeyden önce gelir (daire kendiliğinden kesinleştirmiş olabilir; hesap "bekliyor" olsa da).
        let mut s = satir("kesinlesme_bekliyor", None);
        s.uyap = Some(true);
        assert_eq!(grup_bul(&s, bugun), "uyap_kesinlesti");
        s.uyap = Some(false);
        assert_eq!(grup_bul(&s, bugun), "bekleniyor");
    }

    #[test]
    fn uyap_borclu_eslesmesi() {
        // getTakibiKesinlesenBorcluListesi kaydı (21.11.2026 canlı yanıttan kısaltıldı): vekilin TCKN'si alınmamalı.
        let kesinlesen = json!({"kisiKurumBilgileriDVO": {"kisiKurumId": 169774006, "kurumDVO": {"kurumAdi": "VİVEN GRUP İNŞAAT LTD. ŞTİ.", "vergiNo": "6120406157", "mersisNo": "0612040615700013"}},
            "dosyaKisiDVO": {"tarafAdi": "VİVEN GRUP İNŞAAT LTD. ŞTİ.", "tarafTC": "6120406157", "guncelKisiKurumId": 169774006},
            "vekilArrayList": [{"avukatDVO": {"TC_KIMLIK_NO": "24041357220"}, "kisiTumBilgileriDVO": {"tcKimlikNo": "24041357220"}}],
            "guncelKisiKurumId": 169774006, "tarafAdi": "VİVEN GRUP İNŞAAT LTD. ŞTİ."});
        let k = uyap_kimlikleri(&kesinlesen);
        assert!(k.contains("6120406157") && k.contains("0612040615700013") && !k.contains("24041357220"), "{k:?}");
        assert_eq!(kisi_kurum_id(&kesinlesen), Some(169774006));
        let kimlikli = BizimBorclu { bag: 1, ad: "Başka Ad".into(), kimlikler: vec!["6120406157".into()] };
        assert!(eslesir(&kimlikli, &kesinlesen), "kimlik tutuyorsa ad farkı önemsiz");
        let yanlis = BizimBorclu { bag: 2, ad: "VİVEN GRUP İNŞAAT LTD. ŞTİ.".into(), kimlikler: vec!["1111111111".into()] };
        assert!(!eslesir(&yanlis, &kesinlesen), "iki tarafta kimlik var ve farklı: ad tutsa da değil");
        let kimliksiz = BizimBorclu { bag: 3, ad: "Viven Grup İnşaat Ltd Şti".into(), kimlikler: vec![] };
        assert!(eslesir(&kimliksiz, &kesinlesen), "kimliksiz borçlu adla eşleşir (noktalama / harf büyüklüğü yok sayılır)");
        // dosya_borclu_list kaydı (kişi).
        let kisi = json!({"kisiKurumId": 255110024, "turu": 0, "kisiTumDVO": {"adi": "AYŞE", "soyadi": "KAYA", "tcKimlikNo": "12345678901"}});
        assert_eq!(uyap_adi(&kisi), "AYŞE KAYA");
        assert!(eslesir(&BizimBorclu { bag: 4, ad: "Ayşe Kaya".into(), kimlikler: vec![] }, &kisi));
        assert!(!eslesir(&BizimBorclu { bag: 5, ad: "Ayşe Kayalı".into(), kimlikler: vec![] }, &kisi));
    }

    #[test]
    fn talep_govdesi_ve_udf() {
        let tur = json!({"grupKodu": 2, "talepKodu": 31, "talepAdi": "Takibin Kesinleşmesi", "talepMasrafi": 0,
                         "className": "AvukatTalepGenelTakibiKesinlestirDVO", "postaMasrafId": 0, "dosyaDurum": "A"});
        let b: Value = serde_json::from_str(&talep_bilgileri(&tur)).unwrap();
        assert_eq!((b[0]["talepKodu"].clone(), b[0]["masraf"].clone(), b[0]["fields"].clone(), b[0]["id"].clone()),
                   (json!(31), json!(0), json!([{"id": "", "title": ""}]), json!(0)));
        assert_eq!(talep_kodu(&tur), Some(31));
        assert_eq!(masraf(&json!({"talepMasrafi": "12,5"})), 12.5);
        assert!(udf_mu(200, b"PK\x03\x04...") && udf_mu(200, b"<?xml?><template>") && !udf_mu(200, b"{\"error\":\"x\"}") && !udf_mu(500, b"PK"));
        assert!(yetki_reddi_mi("UYAP hatası: Bu sorgulama, alacaklı vekili tarafından yapılabilmektedir. [PRTL_GNL_1-1]"));
        assert!(!yetki_reddi_mi("UYAP hatası: Doğrulama hatası. [PRTL_GNL_10001-4]"));
        assert_eq!(html_temizle("<b>Talep</b> gönderildi.<br/>Evrak no: 5"), "Talep gönderildi. Evrak no: 5");
        let alanlar = gonderim_alanlari("u-1", 7, "[]", "2026_1_imzali.udf", b"IMZALI".to_vec());
        let adlar: Vec<&str> = alanlar.iter().map(|a| a.0.as_str()).collect();
        assert_eq!(adlar, ["file_1", "items", "dosyaId", "kisiKurumId", "tutar", "talepGrupId", "talepBilgileri"]);
        assert_eq!(alanlar[0].1.as_deref(), Some("2026_1_imzali.udf"));
        let items: Value = serde_json::from_slice(&alanlar[1].2).unwrap();
        assert_eq!((items[0]["tur"].clone(), items[0]["path"].clone()), (json!("ICR_AVUKAT_PORTAL_GENEL_TALEP"), json!("C:/fakepath/2026_1_imzali.udf")));
    }
}
