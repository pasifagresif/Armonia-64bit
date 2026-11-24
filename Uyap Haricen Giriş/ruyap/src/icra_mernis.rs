//! Mernis adresi sorgusu ve yeniden tebliğ talebi (23.11.2026). Python karşılıkları: `UyapMasa/modules/teblig_21_2_core.py`
//! (uygunluk) ve `uyap_core/teblig_212/gonder.py` (gönderim; 13.08.2026 gerçek bir gönderimin ağ kaydından çıkarıldı).
//!
//! Ödeme emri İADE dönen borçlu için (`dosya_taraf.tebligat_durumu = 'iade'`):
//!   1. Borçlunun Mernis adresi sorgulanır: `borclu_bilgileri_goruntule_mernis.ajx {dosyaId, kisiKurumId}` (kisiKurumId
//!      dosyanın borçlu listesinden; eşleşme kimlikle, yoksa tam adla — tahmin yok). Sonuç `borclu_mernis`e yazılır.
//!      Aynı kişi başka dosyada son [`ONBELLEK_GUN`] günde sorgulandıysa UYAP'a yeniden sorulmaz (kullanıcı bulgusu
//!      14.08.2026: aynı kişiye tekrar tekrar Mernis sorgusu istenmiyor); dosya panelinden "yeniden sorgula" bunu aşar.
//!   2. Borçluya İLK giden fiziki ödeme emri zarfının adresi PDF'ten okunur ("TC:<maskeli kimlik>" ile "Mühür ve
//!      İmza" arası). Okunan adres tebligat satırında saklanır (`t212_teblig_adresi`), PDF bir daha indirilmez.
//!   3. Karşılaştırma (alan alan, Türkçe harf / "Mah." "Sk." eki farkı yok sayılarak): aynıysa **21/2 şerhli** yeniden
//!      tebliğ uygundur; farklıysa tebligat Mernis adresine hiç çıkmamıştır → Mernis adresine **normal tebligat**.
//!
//! Yeniden tebliğ talebi (ÜCRETLİ; e-Barobirlik kartından düşer): "İcra/Ödeme Emrinin Tebliğe Çıkartılması" talebi,
//! adres türü Mernis. Her borçlu için göndermeden hemen önce dosyanın evraklarına bakılır: iade tebligatından sonra
//! "Avukat Portal Tebligat Talebi" evrakı varsa talep zaten gönderilmiştir (çift ücret olmasın) → gönderilmez.
//! `getIcraTalepEvrakHazirla.uyap` UDF şablonu döndürmezse imza / gönderim adımına geçilmez (ücret harcanmaz).

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::NaiveDate;
use postgres::GenericClient;
use regex::Regex;
use serde_json::{json, Value};

use crate::icra_guncelle::{
    ag_hatasi_mi, dosya_istegi_at, evrak_listesi, evrak_pdf, guncel_kimlik, kimlik_bayat_mi, kimlik_yenile, mola, uyap_reddi, vt,
    Tarama, TaramaInner, Uyap, YazilanDosya, AG_ONEKI, BORCLU_LISTESI,
};
use crate::icra_kesinlestirme::{
    eslesir, html_temizle, kimlik_listesi, kisalt, kisi_kurum_id, neden, udf_mu, yetki_reddi_mi, BizimBorclu,
    TALEP_GONDER, TALEP_HAZIRLA,
};
use crate::icra_tebligat as teb;

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

pub const MERNIS_SORGUSU: &str = "borclu_bilgileri_goruntule_mernis.ajx";
/// Zarf PDF'i indirildi ama adres okunamadı (`t212_teblig_adresi`); PDF bir daha indirilmez.
pub const ADRES_OKUNAMADI: &str = "-";
/// Aynı kişinin başka dosyadaki Mernis sonucu bu kadar gün geçerli sayılır.
pub const ONBELLEK_GUN: i64 = 30;
const TALEP_EVRAK_TURU: &str = "ICR_AVUKAT_PORTAL_TEBLIGAT_TALEP";
const TEBLIGAT_TALEP_GRUBU: &str = "1";
/// e-Barobirlik Kart (gerçek gönderimde kullanılan değer; bu alan olmadan UYAP başka ödeme yöntemine yönlendiriyor).
const ODEME_TIPI: &str = "7";

/// Talep türü: (UYAP `tebligatTuru`, açıklama, masraf TL). UYAP'ın 17.08.2026 tarihli tebligat türü listesinden.
pub fn talep_turu(kategori: &str) -> Option<(i64, &'static str, f64)> {
    match kategori {
        "uygun" => Some((2, "T.K.21/2 Şerhli", 265.0)),
        "farkli" => Some((1, "Normal Tebligat", 265.0)),
        _ => None,
    }
}

pub fn kategori_etiketi(kod: &str) -> &'static str {
    match kod {
        "uygun" => "21/2 şerhli tebliğe uygun",
        "farkli" => "Mernis'e çıkmamış: Mernis adresine normal tebligat",
        "elle" => "Elle inceleyin",
        _ => "Mernis sorgulanmadı",
    }
}

// ── Adres ───────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Adres {
    pub mahalle: String,
    pub sokak: String,
    pub dis_no: String,
    pub ic_no: String,
    pub ilce: String,
    pub il: String,
    pub ham: String,
}

fn re(h: &'static OnceLock<Regex>, d: &str) -> &'static Regex {
    h.get_or_init(|| Regex::new(d).expect("regex"))
}

/// Adres bloğunu ("Ataşehir Mah. 8213 Sk. No:19 İç Kapı No:3  Çiğli/ İzmir") alanlarına ayırır; desen uymazsa None.
pub fn adres_parcala(blok: &str) -> Option<Adres> {
    static R: OnceLock<Regex> = OnceLock::new();
    let blok = blok.split_whitespace().collect::<Vec<_>>().join(" ");
    let m = re(&R, r"^(?P<mahalle>.+?)\s+(?i:Mah\.?|Mahallesi)\s+(?P<sokak>.+?)\s+No\s*:\s*(?P<dis>\S+)(?:\s+İç Kapı No\s*:\s*(?P<ic>\S+))?\s+(?P<ilce>[^/]+?)\s*/\s*(?P<il>.+)$")
        .captures(&blok)?;
    let g = |a: &str| m.name(a).map(|x| x.as_str().trim().to_string()).unwrap_or_default();
    Some(Adres { mahalle: g("mahalle"), sokak: g("sokak"), dis_no: g("dis"), ic_no: g("ic"), ilce: g("ilce"), il: g("il"), ham: blok.clone() })
}

/// Fiziki tebligat zarfının (PDF düz metni) adres bloğu: "TC:<maskeli kimlik>" ile "Mühür ve İmza" / "BU ZARFTA" /
/// "BURADAN KATLAYINIZ" arası (canlı doğrulandı 13.08.2026, 2026/98353).
pub fn zarf_adres_blogu(govde: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let m = re(&R, r"(?s)TC:[\d*]{4,15}\s*(.+?)(?:Mühür ve İmza|BU ZARFTA|BURADAN KATLAYINIZ)").captures(govde)?;
    let s = m.get(1)?.as_str().split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

/// Mernis yanıtının adresi: `sorguSonucDVO.mernisAdres.adresler[0]` altındaki adres nesnesi (il / ilçe merkezi, belde,
/// köy …; alan adları canlı doğrulandı: mahalle, csbm, disKapiNo, icKapiNo, ilce, il).
pub fn mernis_adresi(v: &Value) -> Option<Adres> {
    let ilk = v.pointer("/sorguSonucDVO/mernisAdres/adresler/0")?;
    let adr = if ilk.get("il").is_some() {
        ilk
    } else {
        ilk.as_object()?.values().find(|x| x.get("il").is_some() || x.get("ilce").is_some())?
    };
    let m = |a: &str| teb::metin(adr, a).trim().to_string();
    let mahalle = Some(m("mahalle")).filter(|s| !s.is_empty()).unwrap_or_else(|| m("koy"));
    let a = Adres { mahalle, sokak: m("csbm"), dis_no: m("disKapiNo"), ic_no: m("icKapiNo"), ilce: m("ilce"), il: m("il"), ham: String::new() };
    if a.il.is_empty() && a.ilce.is_empty() && a.mahalle.is_empty() {
        return None;
    }
    let ham = format!("{} {} No:{} İç Kapı No:{} {}/ {}", a.mahalle, a.sokak, a.dis_no, a.ic_no, a.ilce, a.il);
    Some(Adres { ham: ham.split_whitespace().collect::<Vec<_>>().join(" "), ..a })
}

/// Karşılaştırma için: Türkçe harf duyarsız, "mahallesi / mah. / sokak / sk. / cadde / cd. / bulvar / blv. / meydan" ekleri
/// ve noktalama yok sayılır.
pub fn parca_normal(s: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let k = teb::tr_kucuk(s);
    let k = re(&R, r"\b(mahallesi|mah|sokagi|sokak|sk|caddesi|cadde|cd|bulvari|bulvar|blv|meydani|meydan|mey)\b\.?").replace_all(&k, " ");
    k.replace(['.', ','], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn adresler_ayni(a: &Adres, b: &Adres) -> bool {
    [(&a.mahalle, &b.mahalle), (&a.sokak, &b.sokak), (&a.dis_no, &b.dis_no), (&a.ic_no, &b.ic_no), (&a.ilce, &b.ilce), (&a.il, &b.il)]
        .iter()
        .all(|(x, y)| parca_normal(x) == parca_normal(y))
}

/// Kategori ve açıklama.
pub fn kiyasla(mernis: Option<&Adres>, zarf: Option<&Adres>, zarf_ham: &str) -> (&'static str, String) {
    match (mernis, zarf) {
        (None, _) => ("elle", "Mernis adresi alınamadı".into()),
        (Some(_), None) if zarf_ham.is_empty() => ("elle", "Ödeme emri zarfının adresi okunamadı".into()),
        (Some(_), None) => ("elle", "Ödeme emri zarfındaki adres ayrıştırılamadı".into()),
        (Some(m), Some(z)) if adresler_ayni(m, z) => ("uygun", "Ödeme emri Mernis adresine çıkmış, iade dönmüş".into()),
        (Some(_), Some(_)) => ("farkli", "Ödeme emri Mernis adresinden farklı bir adrese çıkmış".into()),
    }
}

// ── Veritabanı ──────────────────────────────────────────────────────────────────────────────────

/// Mernis işi yapılacak borçlu.
#[derive(Debug, Clone)]
pub struct MernisIsi {
    pub bag: i64,
    pub dosya: i64,
    pub dosya_no: String,
    pub taraf: i64,
    pub borclu: BizimBorclu,
    /// İlk giden fiziki ödeme emri: (tebligat id, birim evrak no, kayıtlı zarf adresi).
    pub zarf: Option<(i64, i32, String)>,
    /// Gönderim için: kayıtlı Mernis kategorisi ve iade tebligatının günü.
    pub kategori: String,
    pub iade_gunu: Option<NaiveDate>,
}

pub fn isleri_oku<C: GenericClient>(c: &mut C, baglar: &[i64]) -> Result<Vec<MernisIsi>, String> {
    if baglar.is_empty() {
        return Ok(Vec::new());
    }
    let ad = crate::icra_api::ad_sql("t");
    let mut out: HashMap<i64, MernisIsi> = HashMap::new();
    for s in c
        .query(
            &format!(
                "SELECT x.id, x.dosya_id, d.dosya_no, t.id, {ad}, t.tckn, NULLIF(t.vergi_no, ''), t.mersis_no,
                        z.id, z.birim_evrak_no, z.t212_teblig_adresi, COALESCE(m.kategori, ''),
                        (SELECT max(i.evrak_gunu) FROM tebligat_barkod i WHERE i.borclu_id = x.id AND NOT i.elektronik_tebligat
                           AND i.tebligat_turu IN ('Ödeme Emri', 'Tahliye Emri'))
                   FROM dosya_taraf x JOIN dosya d ON d.id = x.dosya_id JOIN taraf t ON t.id = x.taraf_id
                   LEFT JOIN borclu_mernis m ON m.dosyataraf_id = x.id
                   LEFT JOIN LATERAL (SELECT tb.id, tb.birim_evrak_no, tb.t212_teblig_adresi FROM tebligat_barkod tb
                                       WHERE tb.borclu_id = x.id AND NOT tb.elektronik_tebligat
                                         AND tb.tebligat_turu IN ('Ödeme Emri', 'Tahliye Emri')
                                       ORDER BY tb.evrak_gunu ASC NULLS LAST, tb.birim_evrak_no ASC LIMIT 1) z ON true
                  WHERE x.id = ANY($1) AND x.rol = 'borclu' AND d.durum_kod = 0"
            ),
            &[&baglar],
        )
        .map_err(hata)?
    {
        let bag: i64 = s.get(0);
        out.insert(
            bag,
            MernisIsi {
                bag,
                dosya: s.get(1),
                dosya_no: s.get(2),
                taraf: s.get(3),
                borclu: BizimBorclu { bag, ad: s.get(4), kimlikler: kimlik_listesi(s.get(5), s.get(6), s.get(7)) },
                zarf: s.get::<_, Option<i64>>(8).map(|id| (id, s.get(9), s.get(10))),
                kategori: s.get(11),
                iade_gunu: s.get(12),
            },
        );
    }
    // Seçim sırası korunur.
    Ok(baglar.iter().filter_map(|b| out.remove(b)).collect())
}

/// Aynı kişinin başka dosyadaki yakın tarihli Mernis sonucu: (adres, ham yanıt).
fn onbellek<C: GenericClient>(c: &mut C, taraf: i64, bag: i64) -> Result<Option<(Adres, Value)>, String> {
    Ok(c.query_opt(
        "SELECT m.mahalle, m.sokak, m.dis_no, m.ic_no, m.ilce, m.il, m.adres, m.ham_yanit FROM borclu_mernis m
           JOIN dosya_taraf x ON x.id = m.dosyataraf_id
          WHERE x.taraf_id = $1 AND m.dosyataraf_id <> $2 AND m.adres <> '' AND m.zaman > now() - make_interval(days => $3)
          ORDER BY m.zaman DESC LIMIT 1",
        &[&taraf, &bag, &(ONBELLEK_GUN as i32)],
    )
    .map_err(hata)?
    .map(|s| {
        (
            Adres { mahalle: s.get(0), sokak: s.get(1), dis_no: s.get(2), ic_no: s.get(3), ilce: s.get(4), il: s.get(5), ham: s.get(6) },
            s.get::<_, Option<Value>>(7).unwrap_or(Value::Null),
        )
    }))
}

/// Sorgu sonucunu borçluya yazar (önceki sonuç ezilir); okunan zarf adresi tebligat satırında saklanır.
#[allow(clippy::too_many_arguments)]
pub fn mernis_yaz<C: GenericClient>(
    c: &mut C,
    bag: i64,
    adres: Option<&Adres>,
    ham: &Value,
    zarf: Option<(i64, &str)>,
    kategori: &str,
    aciklama: &str,
) -> Result<(), String> {
    let a = adres.cloned().unwrap_or_default();
    let ham = (!ham.is_null()).then_some(ham);
    let (teb_id, zarf_ham) = (zarf.map(|z| z.0), zarf.map(|z| z.1.to_string()).unwrap_or_default());
    c.execute(
        "INSERT INTO borclu_mernis (dosyataraf_id, adres, mahalle, sokak, dis_no, ic_no, ilce, il, ham_yanit, zaman, tebligat_id, teblig_adresi, kategori, aciklama)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now(), $10, $11, $12, $13)
         ON CONFLICT (dosyataraf_id) DO UPDATE SET adres = EXCLUDED.adres, mahalle = EXCLUDED.mahalle, sokak = EXCLUDED.sokak,
             dis_no = EXCLUDED.dis_no, ic_no = EXCLUDED.ic_no, ilce = EXCLUDED.ilce, il = EXCLUDED.il, ham_yanit = EXCLUDED.ham_yanit,
             zaman = now(), tebligat_id = EXCLUDED.tebligat_id, teblig_adresi = EXCLUDED.teblig_adresi, kategori = EXCLUDED.kategori,
             aciklama = EXCLUDED.aciklama",
        &[&bag, &a.ham, &a.mahalle, &a.sokak, &a.dis_no, &a.ic_no, &a.ilce, &a.il, &ham, &teb_id, &zarf_ham, &kategori, &aciklama],
    )
    .map_err(hata)?;
    if let Some(id) = teb_id.filter(|_| !zarf_ham.is_empty()) {
        c.execute(
            "UPDATE tebligat_barkod SET t212_teblig_adresi = $2, t212_mernis_adresi = $3, t212_kategori = $4, t212_tarandi_zamani = now() WHERE id = $1",
            &[&id, &zarf_ham, &a.ham, &kategori],
        )
        .map_err(hata)?;
    }
    Ok(())
}

fn talep_yaz<C: GenericClient>(c: &mut C, bag: i64, tur: i64, tutar: f64, sonuc: &str, mesaj: &str) -> Result<(), String> {
    c.execute(
        "INSERT INTO yeniden_teblig_talebi (dosyataraf_id, tur, tutar, sonuc, mesaj) VALUES ($1, $2, $3::float8::numeric, $4, $5)",
        &[&bag, &(tur as i16), &tutar, &sonuc, &mesaj],
    )
    .map_err(hata)?;
    if sonuc == "gonderildi" {
        c.execute(
            "UPDATE tebligat_barkod SET yeniden_teblig_talep_edildi = true WHERE dosya_id = (SELECT dosya_id FROM dosya_taraf WHERE id = $1)",
            &[&bag],
        )
        .map_err(hata)?;
    }
    Ok(())
}

/// Bu borçluya daha önce gönderilmiş (ya da dosyada var görülen) yeniden tebliğ talebi.
fn onceki_talep<C: GenericClient>(c: &mut C, bag: i64) -> Result<Option<String>, String> {
    Ok(c.query_opt(
        "SELECT to_char(zaman, 'DD.MM.YYYY HH24:MI') FROM yeniden_teblig_talebi WHERE dosyataraf_id = $1 AND sonuc IN ('gonderildi', 'zaten')
          ORDER BY zaman DESC LIMIT 1",
        &[&bag],
    )
    .map_err(hata)?
    .map(|s| s.get(0)))
}

fn dosya_borclu_sayisi<C: GenericClient>(c: &mut C, dosya: i64) -> Result<i64, String> {
    Ok(c.query_one("SELECT count(*) FROM dosya_taraf WHERE dosya_id = $1 AND rol = 'borclu' AND uyapta_gorulmedi IS NULL", &[&dosya])
        .map_err(hata)?
        .get(0))
}

// ── UYAP: ortak ─────────────────────────────────────────────────────────────────────────────────

/// Dosya başına bir kez alınanlar.
#[derive(Default)]
struct DosyaOnbellegi {
    borclular: Option<Vec<Value>>,
    evraklar: Option<Vec<Value>>,
}

enum Kimlik {
    Tamam(i64),
    Engel(String),
    Red(String),
}

async fn kisi_kurum<U: Uyap>(t: &Tarama, u: &U, is_: &MernisIsi, uyap_id: &mut String, ob: &mut HashMap<i64, DosyaOnbellegi>) -> Result<Kimlik, String> {
    if ob.get(&is_.dosya).is_none_or(|o| o.borclular.is_none()) {
        match dosya_istegi_at(t, u, BORCLU_LISTESI, is_.dosya, uyap_id, json!({})).await? {
            Some(Value::Array(a)) => ob.entry(is_.dosya).or_default().borclular = Some(a),
            v => return Ok(Kimlik::Red(format!("UYAP borçlu listesi alınamadı ({})", neden(&v)))),
        }
    }
    let liste = ob.get(&is_.dosya).and_then(|o| o.borclular.clone()).unwrap_or_default();
    let adaylar: Vec<&Value> = liste.iter().filter(|r| r.is_object() && eslesir(&is_.borclu, r)).collect();
    Ok(match adaylar.as_slice() {
        [r] => match kisi_kurum_id(r) {
            Some(k) => Kimlik::Tamam(k),
            None => Kimlik::Engel("UYAP borçlu kaydında kişi / kurum kimliği yok".into()),
        },
        [] => Kimlik::Engel("borçlu UYAP'ın borçlu listesinde eşleşmedi (ad ya da kimlik farklı)".into()),
        _ => Kimlik::Engel("UYAP'ın borçlu listesinde bu borçluya birden çok kayıt uyuyor".into()),
    })
}

async fn evraklar<U: Uyap>(u: &U, dosya: i64, uyap_id: &str, ob: &mut HashMap<i64, DosyaOnbellegi>, taze: bool) -> Result<Result<Vec<Value>, String>, String> {
    if taze || ob.get(&dosya).is_none_or(|o| o.evraklar.is_none()) {
        match evrak_listesi(u, uyap_id).await? {
            Ok(v) => ob.entry(dosya).or_default().evraklar = Some(v),
            Err(n) => return Ok(Err(n)),
        }
    }
    Ok(Ok(ob.get(&dosya).and_then(|o| o.evraklar.clone()).unwrap_or_default()))
}

fn uyap_haritasi(dosyalar: &[YazilanDosya]) -> HashMap<i64, (String, String)> {
    dosyalar.iter().map(|(d, k, e)| (*d, (k.clone(), e.clone()))).collect()
}

// ── Mod::Mernis ─────────────────────────────────────────────────────────────────────────────────

/// Seçili borçluların Mernis adresi + ilk ödeme emri zarfı adresiyle kıyas. `yeniden`: başka dosyadaki sonuç
/// kullanılmaz, UYAP'a sorulur.
pub(crate) async fn mernis_sorgula<U: Uyap>(t: &Tarama, u: &U, dosyalar: &[YazilanDosya]) -> Result<(), String> {
    let (secilen, yeniden) = {
        let g = t.inner.lock().unwrap();
        (g.kapsam.borclular.clone().unwrap_or_default(), g.kapsam.yeniden)
    };
    let uyap = uyap_haritasi(dosyalar);
    let isler = vt(move |c| isleri_oku(c, &secilen)).await?;
    t.asama("mernis", isler.len());
    let mut ob: HashMap<i64, DosyaOnbellegi> = HashMap::new();
    let (mut ardisik, mut ret) = (0usize, 0usize);
    for (sira, is_) in isler.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.ind.iptal_kontrol()?;
        let bag = is_.bag;
        let Some((uid, dosya_etiket)) = uyap.get(&is_.dosya).cloned().filter(|x| !x.0.is_empty()) else {
            t.uyari(format!("{} – {}: dosya UYAP'ta bulunamadı; Mernis sorgulanmadı", is_.dosya_no, is_.borclu.ad));
            t.inner.lock().unwrap().mer_hata += 1;
            t.ilerle();
            continue;
        };
        let etiket = format!("{dosya_etiket} – {}", is_.borclu.ad);
        t.mesaj_yaz(format!("Mernis adresi: {etiket} ({}/{})", sira + 1, t.inner.lock().unwrap().toplam));
        let mut uid = guncel_kimlik(t, u, is_.dosya, &uid).await?;
        let sonuc = tek_mernis(t, u, &is_, &mut uid, &mut ob, yeniden).await;
        match sonuc {
            Ok(Ok(kategori)) => {
                (ardisik, ret) = (0, 0);
                let mut g = t.inner.lock().unwrap();
                match kategori {
                    "uygun" => g.mer_uygun += 1,
                    "farkli" => g.mer_farkli += 1,
                    _ => g.mer_elle += 1,
                }
            }
            Ok(Err(m)) => {
                ardisik = 0;
                t.uyari(format!("{etiket}: {m}"));
                t.inner.lock().unwrap().mer_hata += 1;
                if !yetki_reddi_mi(&m) && !m.starts_with("borçlu ") && !m.contains("kişi / kurum") {
                    uyap_reddi(&m, &mut ret)?;
                }
            }
            Err(e) if e.starts_with(AG_ONEKI) => {
                t.inner.lock().unwrap().mer_hata += 1;
                ag_hatasi_mi(&e, t, &etiket, &mut ardisik)?;
            }
            Err(e) => return Err(e),
        }
        let _ = bag;
        t.ilerle();
    }
    Ok(())
}

/// Tek borçlu. İç hata: bu borçluya özgü (kayda geçmez, uyarı olur).
async fn tek_mernis<U: Uyap>(
    t: &Tarama,
    u: &U,
    is_: &MernisIsi,
    uyap_id: &mut String,
    ob: &mut HashMap<i64, DosyaOnbellegi>,
    yeniden: bool,
) -> Result<Result<&'static str, String>, String> {
    let (bag, taraf) = (is_.bag, is_.taraf);
    // Kesin sonuca varmış borçlu (uygun / farklı) toplu sorguda yeniden sorulmaz; panelden "yeniden sorgula" sorar.
    if !yeniden && matches!(is_.kategori.as_str(), "uygun" | "farkli") {
        t.inner.lock().unwrap().mer_onbellek += 1;
        return Ok(Ok(if is_.kategori == "uygun" { "uygun" } else { "farkli" }));
    }
    // 1) Mernis adresi: yakın tarihli başka dosyadaki sonuç, yoksa UYAP.
    let eski = if yeniden { None } else { vt(move |c| onbellek(c, taraf, bag)).await? };
    let (adres, ham) = match eski {
        Some((a, h)) => {
            t.inner.lock().unwrap().mer_onbellek += 1;
            (Some(a), h)
        }
        None => {
            let kkid = match kisi_kurum(t, u, is_, uyap_id, ob).await? {
                Kimlik::Tamam(k) => k,
                Kimlik::Engel(m) | Kimlik::Red(m) => return Ok(Err(m)),
            };
            t.inner.lock().unwrap().mer_sorulan += 1;
            let v = dosya_istegi_at(t, u, MERNIS_SORGUSU, is_.dosya, uyap_id, json!({"kisiKurumId": kkid})).await?;
            match v {
                Some(v) if v.get("sorguSonucDVO").is_some() => (mernis_adresi(&v), v),
                v => return Ok(Err(format!("Mernis sorgusu yanıt vermedi ({})", neden(&v)))),
            }
        }
    };
    // 2) İlk ödeme emri zarfının adresi (kayıtlıysa PDF indirilmez).
    let mut zarf_ham = is_.zarf.as_ref().map(|z| z.2.clone()).unwrap_or_default();
    if let Some((_, evrak_no, _)) = is_.zarf.as_ref().filter(|_| zarf_ham.is_empty()) {
        match evraklar(u, is_.dosya, uyap_id, ob, false).await? {
            Ok(liste) => {
                let no = evrak_no.to_string();
                let evrak_id = liste.iter().find(|e| teb::metin(e, "birimEvrakNo") == no).map(|e| teb::metin(e, "evrakId"));
                match evrak_id {
                    Some(id) => match evrak_pdf(u, uyap_id, &id).await? {
                        Ok(pdf) => {
                            t.inner.lock().unwrap().pdf_indirilen += 1;
                            zarf_ham = zarf_adres_blogu(&teb::pdf_govde_metni(&pdf)).unwrap_or_else(|| ADRES_OKUNAMADI.to_string());
                        }
                        Err(n) => t.uyari(format!("{} – {}: ödeme emri zarfı indirilemedi ({n})", is_.dosya_no, is_.borclu.ad)),
                    },
                    None => t.uyari(format!("{} – {}: ödeme emri zarfı (evrak {evrak_no}) UYAP'ın evrak listesinde yok", is_.dosya_no, is_.borclu.ad)),
                }
            }
            Err(n) => t.uyari(format!("{} – {}: evrak listesi alınamadı ({n})", is_.dosya_no, is_.borclu.ad)),
        }
    }
    let okunan = if zarf_ham == ADRES_OKUNAMADI { "" } else { zarf_ham.as_str() };
    let zarf = adres_parcala(okunan);
    let (kategori, mut aciklama) = kiyasla(adres.as_ref(), zarf.as_ref(), okunan);
    if is_.zarf.is_none() {
        aciklama = "Borçluya giden fiziki ödeme emri kaydı yok".into();
    }
    let teb_id = is_.zarf.as_ref().map(|z| z.0);
    vt(move |c| mernis_yaz(c, bag, adres.as_ref(), &ham, teb_id.map(|id| (id, zarf_ham.as_str())), kategori, &aciklama)).await?;
    Ok(Ok(kategori))
}

// ── Mod::YenidenTeblig ──────────────────────────────────────────────────────────────────────────

/// Python `_talep_bilgileri_json` ile birebir (dosyaya özgü alan yok; UYAP dosyaId / kisiKurumId'den doldurur).
pub fn talep_bilgileri(tur: i64, aciklama: &str, masraf: f64) -> String {
    json!([{
        "grupKodu": 1, "talepKodu": 8,
        "talepAdi": "İcra/Ödeme Emrinin Tebliğe Çıkartılması",
        "talepKisaAdi": "İcra/Ödeme Emrinin Tebliğe Çıkartılması",
        "talepMasrafi": 0, "className": "AvukatTalepTebligatOdemeEmriDVO",
        "postaMasrafId": 0, "dosyaDurum": "A",
        "tebligatTuruAciklama": aciklama, "tebligatTuru": tur,
        "adresTuru": 1, "adresTuruAciklama": "Mernis Adresi", "adres": "VAR",
        "masraf": masraf as i64,
        "fields": [{"id": "tebligatTuruAciklama", "title": "Tebligat Türü"},
                   {"id": "adresTuruAciklama", "title": "Adres Türü"},
                   {"id": "adres", "title": "Adres"}],
        "id": 0,
    }])
    .to_string()
}

fn gonderim_alanlari(uyap_id: &str, kkid: i64, bilgiler: &str, masraf: f64, imzali_ad: &str, imzali: Vec<u8>) -> Vec<(String, Option<String>, Vec<u8>)> {
    let item = json!({
        "id": chrono::Utc::now().timestamp_millis(),
        "evrakTuruOptionDVO": {
            "tur": TALEP_EVRAK_TURU, "label": "Tebligat Talebi", "mandatory": true, "max": 1, "ekEvrakMax": 2, "sablonTurKodu": "",
            "acceptTypeList": [".udf", ".UDF"],
            "ekEvrakAcceptTypeList": [".udf", ".pdf", ".jpg", ".jpeg", ".png", ".tif", ".tiff", ".UDF", ".PDF", ".JPG", ".JPEG", ".PNG", ".TIF", ".TIFF"]
        },
        "tur": TALEP_EVRAK_TURU, "turAciklama": "Tebligat Talebi", "mandatory": true, "file": {},
        "path": format!("C:/fakepath/{imzali_ad}"), "kullaniciEvrakAciklama": "", "parentId": -1,
        "isVekalet": false, "isVekaletBilgiFormu": false, "fileId": 1
    });
    let metin = |a: &str, v: String| (a.to_string(), None, v.into_bytes());
    vec![
        ("file_1".into(), Some(imzali_ad.to_string()), imzali),
        metin("items", Value::Array(vec![item]).to_string()),
        metin("dosyaId", uyap_id.to_string()),
        metin("kisiKurumId", kkid.to_string()),
        metin("tutar", format!("{}", masraf as i64)),
        metin("talepGrupId", TEBLIGAT_TALEP_GRUBU.into()),
        metin("talepBilgileri", bilgiler.to_string()),
        metin("vakifbankHesapBilgileri", "null".into()),
        metin("vakifbankOdemeIstekBilgileri", "null".into()),
        metin("smsSifre", "null".into()),
        metin("odemeTipi", ODEME_TIPI.into()),
    ]
}

enum TalepSonucu {
    Gonderildi(String),
    Zaten(String),
    Atla(String),
    Engel(String),
    Red(String),
    Belirsiz(String),
}

/// Evrak listesinde iade tebligatından sonra (ya da gün bilinmiyorsa herhangi) "Avukat Portal Tebligat Talebi" var mı.
pub fn talep_evraki_var(evraklar: &[Value], sonra: Option<NaiveDate>) -> bool {
    evraklar.iter().any(|e| {
        teb::metin(e, "tur") == teb::TALEP_EVRAK_TUR
            && match (sonra, teb::teblig_tarihini_coz(&teb::metin(e, "onaylandigiTarih").replace('.', "/"))) {
                (Some(s), Some(g)) => g >= s,
                _ => true,
            }
    })
}

async fn tek_teblig<U: Uyap>(
    t: &Tarama,
    u: &U,
    is_: &MernisIsi,
    uyap_id: &mut String,
    ob: &mut HashMap<i64, DosyaOnbellegi>,
    tur: (i64, &'static str, f64),
) -> Result<TalepSonucu, String> {
    use TalepSonucu::*;
    let kkid = match kisi_kurum(t, u, is_, uyap_id, ob).await? {
        Kimlik::Tamam(k) => k,
        Kimlik::Engel(m) => return Ok(Engel(m)),
        Kimlik::Red(m) => return Ok(Red(m)),
    };
    // Çift ücret olmasın: göndermeden hemen önce TAZE evrak listesi.
    let liste = match evraklar(u, is_.dosya, uyap_id, ob, true).await? {
        Ok(l) => l,
        Err(n) => return Ok(Red(format!("evrak listesi alınamadı ({n}); talep gönderilmedi"))),
    };
    if talep_evraki_var(&liste, is_.iade_gunu) {
        let dosya = is_.dosya;
        let n = vt(move |c| dosya_borclu_sayisi(c, dosya)).await?;
        return Ok(if n <= 1 {
            Zaten("Dosyada iade sonrası “Avukat Portal Tebligat Talebi” evrakı var; talep zaten gönderilmiş".into())
        } else {
            Atla("Dosyada iade sonrası tebligat talebi evrakı var; hangi borçlu için olduğu ayırt edilemiyor (elle inceleyin)".into())
        });
    }
    let (kod, aciklama, masraf) = tur;
    let bilgiler = talep_bilgileri(kod, aciklama, masraf);
    let govde = |id: &str| json!({"dosyaId": id, "filename": is_.dosya_no, "kisiKurumId": kkid, "talepBilgileri": bilgiler});
    let (mut st, mut udf) = u.post_ham(TALEP_HAZIRLA, govde(uyap_id)).await?;
    if !udf_mu(st, &udf) && kimlik_bayat_mi(&String::from_utf8_lossy(&udf)) {
        if let Some(yeni) = kimlik_yenile(t, u, is_.dosya, uyap_id).await? {
            *uyap_id = yeni;
            (st, udf) = u.post_ham(TALEP_HAZIRLA, govde(uyap_id)).await?;
        }
    }
    if !udf_mu(st, &udf) {
        return Ok(Red(format!("UYAP talep evrakını hazırlamadı (HTTP {st}): {}; ücret harcanmadı", kisalt(&udf))));
    }
    let ad = format!("{}_Tebligat_Talebi", is_.dosya_no.replace(['/', '\\'], "_"));
    let imzali = u.imzala(udf, format!("{ad}.udf")).await.map_err(|e| format!("{e}. Yeniden tebliğ talepleri durduruldu."))?;
    let imzali_ad = format!("{ad}_imzali.udf");
    match u.yukle(TALEP_GONDER, gonderim_alanlari(uyap_id, kkid, &bilgiler, masraf, &imzali_ad, imzali)).await {
        Ok(Some(v)) if teb::metin(&v, "type") == "success" => Ok(Gonderildi(html_temizle(&teb::metin(&v, "message")))),
        Ok(v) => {
            let m = v.as_ref().map(|v| html_temizle(&teb::metin(v, "message"))).filter(|m| !m.is_empty()).unwrap_or_else(|| neden(&v));
            Ok(Red(format!("UYAP gönderimi kabul etmedi: {m}")))
        }
        Err(e) if e.starts_with(AG_ONEKI) => Ok(Belirsiz(e.trim_start_matches(AG_ONEKI).to_string())),
        Err(e) => Err(e),
    }
}

/// Seçili borçlulara yeniden tebliğ talebi (Mernis kategorisi uygun → 21/2, farklı → normal). ÜCRETLİ.
pub(crate) async fn talepleri_gonder<U: Uyap>(t: &Tarama, u: &U, dosyalar: &[YazilanDosya]) -> Result<(), String> {
    let secilen = t.inner.lock().unwrap().kapsam.borclular.clone().unwrap_or_default();
    let uyap = uyap_haritasi(dosyalar);
    let isler = vt(move |c| isleri_oku(c, &secilen)).await?;
    t.asama("yeniden_teblig", isler.len());
    let mut ob: HashMap<i64, DosyaOnbellegi> = HashMap::new();
    let (mut ardisik, mut ret) = (0usize, 0usize);
    for (sira, is_) in isler.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.ind.iptal_kontrol()?;
        let bag = is_.bag;
        let etiket0 = format!("{} – {}", is_.dosya_no, is_.borclu.ad);
        let Some(tur) = talep_turu(&is_.kategori) else {
            t.uyari(format!("{etiket0}: Mernis kıyası uygun değil ({}); talep gönderilmedi", kategori_etiketi(&is_.kategori)));
            t.inner.lock().unwrap().yt_atlanan += 1;
            t.ilerle();
            continue;
        };
        if let Some(z) = vt(move |c| onceki_talep(c, bag)).await? {
            t.uyari(format!("{etiket0}: {z} tarihinde yeniden tebliğ talebi var; yeniden gönderilmedi"));
            t.inner.lock().unwrap().yt_atlanan += 1;
            t.ilerle();
            continue;
        }
        let Some((uid, dosya_etiket)) = uyap.get(&is_.dosya).cloned().filter(|x| !x.0.is_empty()) else {
            t.uyari(format!("{etiket0}: dosya UYAP'ta bulunamadı; talep gönderilmedi"));
            t.inner.lock().unwrap().yt_atlanan += 1;
            t.ilerle();
            continue;
        };
        let etiket = format!("{dosya_etiket} – {}", is_.borclu.ad);
        let mut uid = guncel_kimlik(t, u, is_.dosya, &uid).await?;
        t.mesaj_yaz(format!("Yeniden tebliğ talebi ({}): {etiket} ({}/{})", tur.1, sira + 1, t.inner.lock().unwrap().toplam));
        let (kod, _, masraf) = tur;
        let sonuc = match tek_teblig(t, u, &is_, &mut uid, &mut ob, tur).await {
            Ok(s) => s,
            Err(e) if e.starts_with(AG_ONEKI) => {
                vt(move |c| talep_yaz(c, bag, kod, 0.0, "hata", "UYAP'a bağlanılamadı; talep gönderilmedi")).await?;
                t.inner.lock().unwrap().yt_hatasi += 1;
                ag_hatasi_mi(&e, t, &etiket, &mut ardisik)?;
                t.ilerle();
                continue;
            }
            Err(e) => {
                if !e.contains("__IPTAL__") {
                    let m = e.clone();
                    vt(move |c| talep_yaz(c, bag, kod, 0.0, "hata", &m)).await?;
                    t.inner.lock().unwrap().yt_hatasi += 1;
                }
                return Err(e);
            }
        };
        match sonuc {
            TalepSonucu::Gonderildi(m) => {
                (ardisik, ret) = (0, 0);
                let mesaj = if m.is_empty() { "UYAP talebi kabul etti".to_string() } else { m };
                vt(move |c| talep_yaz(c, bag, kod, masraf, "gonderildi", &mesaj)).await?;
                let mut g = t.inner.lock().unwrap();
                g.yt_gonderilen += 1;
                g.yt_tutar += masraf;
            }
            TalepSonucu::Zaten(m) => {
                (ardisik, ret) = (0, 0);
                t.uyari(format!("{etiket}: {m}"));
                vt(move |c| talep_yaz(c, bag, kod, 0.0, "zaten", &m)).await?;
                t.inner.lock().unwrap().yt_atlanan += 1;
            }
            TalepSonucu::Atla(m) => {
                (ardisik, ret) = (0, 0);
                t.uyari(format!("{etiket}: {m}"));
                t.inner.lock().unwrap().yt_atlanan += 1;
            }
            TalepSonucu::Engel(m) => {
                ardisik = 0;
                t.uyari(format!("{etiket}: {m}"));
                vt(move |c| talep_yaz(c, bag, kod, 0.0, "hata", &m)).await?;
                t.inner.lock().unwrap().yt_hatasi += 1;
            }
            TalepSonucu::Red(m) => {
                ardisik = 0;
                t.uyari(format!("{etiket}: {m}"));
                let kayit = m.clone();
                vt(move |c| talep_yaz(c, bag, kod, 0.0, "hata", &kayit)).await?;
                t.inner.lock().unwrap().yt_hatasi += 1;
                if !yetki_reddi_mi(&m) {
                    uyap_reddi(&m, &mut ret)?;
                }
            }
            TalepSonucu::Belirsiz(m) => {
                let kayit = format!("Gönderim sırasında bağlantı koptu ({m}); talebin UYAP'a ulaşıp ulaşmadığını dosyanın evraklarından kontrol edin.");
                t.uyari(format!("⚠️ {etiket}: {kayit}"));
                vt(move |c| talep_yaz(c, bag, kod, 0.0, "hata", &kayit)).await?;
                t.inner.lock().unwrap().yt_hatasi += 1;
                ag_hatasi_mi(&format!("{AG_ONEKI}{m}"), t, &etiket, &mut ardisik)?;
            }
        }
        t.ilerle();
    }
    Ok(())
}

// ── Rapor ───────────────────────────────────────────────────────────────────────────────────────

pub(crate) fn rapor_satirlari(g: &TaramaInner) -> Vec<String> {
    let mut out = Vec::new();
    if g.mod_ == crate::icra_guncelle::Mod::Mernis {
        out.push(format!(
            "✔ Mernis: {} borçlu UYAP'a soruldu{}.",
            g.mer_sorulan,
            if g.mer_onbellek > 0 { format!(", {} borçlunun adresi aynı kişinin başka dosyadaki yakın tarihli sorgusundan alındı", g.mer_onbellek) } else { String::new() }
        ));
        out.push(format!(
            "Kıyas: {} borçlu 21/2 şerhli tebliğe uygun, {} borçluda tebligat Mernis adresine çıkmamış (normal tebligat), {} borçlu elle incelenmeli.",
            g.mer_uygun, g.mer_farkli, g.mer_elle
        ));
        if g.mer_hata > 0 {
            out.push(format!("⚠️ {} borçluda Mernis sorgulanamadı (neden uyarılarda).", g.mer_hata));
        }
    } else {
        out.push(format!("✔ {} borçluya yeniden tebliğ talebi gönderildi (toplam ≈ {} TL).", g.yt_gonderilen, g.yt_tutar as i64));
        if g.yt_atlanan > 0 {
            out.push(format!("… {} borçlu atlandı (talep zaten var, Mernis kıyası uygun değil ya da dosya bulunamadı; ayrıntı uyarılarda).", g.yt_atlanan));
        }
        if g.yt_hatasi > 0 {
            out.push(format!("⚠️ {} borçluda talep gönderilemedi (neden uyarılarda).", g.yt_hatasi));
        }
    }
    out
}

pub(crate) fn bitis_mesaji(g: &TaramaInner) -> String {
    if g.mod_ == crate::icra_guncelle::Mod::Mernis {
        format!(
            "Tamamlandı: {} borçlunun Mernis adresi alındı — {} uygun (21/2), {} farklı adres (normal tebligat), {} elle incelenmeli.",
            g.mer_uygun + g.mer_farkli + g.mer_elle,
            g.mer_uygun,
            g.mer_farkli,
            g.mer_elle
        ) + &if g.mer_hata > 0 { format!(" ⚠️ {} borçluda sorgulanamadı.", g.mer_hata) } else { String::new() }
    } else {
        let mut m = format!("Tamamlandı: {} yeniden tebliğ talebi gönderildi (≈ {} TL)", g.yt_gonderilen, g.yt_tutar as i64);
        if g.yt_hatasi + g.yt_atlanan > 0 {
            m.push_str(&format!(" ⚠️ {} borçluya gönderilmedi (ayrıntı uyarılarda)", g.yt_hatasi + g.yt_atlanan));
        }
        m + "."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zarf_ve_mernis_adresi() {
        let govde = "BORÇLU AHMET YILMAZ TC:1234*****90 Ataşehir Mah. 8213 Sk. No:19 İç Kapı No:3  Çiğli/ İzmir Mühür ve İmza";
        let blok = zarf_adres_blogu(govde).unwrap();
        let z = adres_parcala(&blok).unwrap();
        assert_eq!((z.mahalle.as_str(), z.sokak.as_str(), z.dis_no.as_str(), z.ic_no.as_str(), z.ilce.as_str(), z.il.as_str()),
                   ("Ataşehir", "8213 Sk.", "19", "3", "Çiğli", "İzmir"));
        let v = json!({"sorguSonucDVO": {"mernisAdres": {"adresler": [{"ilIlceMerkeziAdresi":
            {"mahalle": "ATAŞEHİR MAH.", "csbm": "8213 SOKAĞI", "disKapiNo": "19", "icKapiNo": "3", "ilce": "ÇİĞLİ", "il": "İZMİR"}}]}}});
        let m = mernis_adresi(&v).unwrap();
        assert!(m.ham.contains("ATAŞEHİR MAH. 8213 SOKAĞI No:19"), "{}", m.ham);
        assert!(adresler_ayni(&m, &z), "ek / harf farkı yok sayılır: {m:?} {z:?}");
        assert_eq!(kiyasla(Some(&m), Some(&z), &blok).0, "uygun");
        let mut baska = z.clone();
        baska.dis_no = "21".into();
        assert_eq!(kiyasla(Some(&m), Some(&baska), &blok).0, "farkli");
        assert_eq!(kiyasla(Some(&m), None, "").0, "elle");
        assert_eq!(kiyasla(None, Some(&z), &blok).0, "elle");
        // İç kapı numarası olmayan zarf
        let z2 = adres_parcala("Kazımdirik Mahallesi 372/7 Sokak No:5 Bornova/ İzmir").unwrap();
        assert_eq!((z2.sokak.as_str(), z2.dis_no.as_str(), z2.ic_no.as_str(), z2.ilce.as_str()), ("372/7 Sokak", "5", "", "Bornova"));
        assert!(adres_parcala("adres yok").is_none());
        assert!(mernis_adresi(&json!({"sorguSonucDVO": {}})).is_none());
    }

    #[test]
    fn talep_evraki_ve_bilgileri() {
        let g = NaiveDate::from_ymd_opt(2026, 10, 1);
        let e = |tarih: &str| json!({"tur": teb::TALEP_EVRAK_TUR, "onaylandigiTarih": tarih});
        assert!(!talep_evraki_var(&[e("15/09/2026 10:00")], g), "iadeden önceki talep sayılmaz");
        assert!(talep_evraki_var(&[e("02/10/2026 10:00")], g));
        assert!(talep_evraki_var(&[e("")], g), "gün bilinmiyorsa var sayılır");
        let b: Value = serde_json::from_str(&talep_bilgileri(2, "T.K.21/2 Şerhli", 265.0)).unwrap();
        assert_eq!((b[0]["tebligatTuru"].clone(), b[0]["adresTuruAciklama"].clone(), b[0]["masraf"].clone()), (json!(2), json!("Mernis Adresi"), json!(265)));
        assert_eq!(talep_turu("farkli").unwrap().0, 1);
        assert!(talep_turu("elle").is_none());
    }
}
