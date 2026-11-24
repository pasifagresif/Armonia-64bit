//! İş Listesi — büro veritabanındaki yeni model (`sema/0003_is_listesi.sql`).
//!
//! Buluttaki `zs_*` İş Listesi'nin yerini alır. Bu katman yalnız veritabanıyla konuşur;
//! uçlar `is_api.rs`'te (L3), ekran `uyap_core/enjekte/isler.js`'te (L4).
//!
//! Temel kurallar:
//!   • Onay iş başına seçimlidir: `onay_gerekir` işaretli iş tamamlanmak istendiğinde `onayda`ya
//!     gider, onaylayınca `tamamlandi` olur; işaretsiz iş doğrudan `tamamlandi` olur.
//!   • İşi kapatırken (onaya sunarken de) sonuca göre sonraki iş ÖNERİSİ hemen döner (B-7); öneri
//!     kendiliğinden iş açmaz, ekran tarihini gösterip değiştirilebilir biçimde onaylatır (B-8).
//!   • Süre "gün sayısı" değil türüyle hesaplanır: takvim günü / iş günü / ayın günü / tarih sor.
//!     İş günü hesabı icra katmanıyla aynı (`icra_durum`, `resmi_tatil` tablosu).
//!   • Tekrar eden işin (rutin) en çok bir açık örneği olur; bitmeyen örnek gecikmiş kalır,
//!     elle ertesi güne aktarmak gerekmez (B-10).
//!   • Gizli iş yalnız sahibine (oluşturan ve sorumlu) görünür; diğer işler bütün büroya açıktır.

use crate::icra_durum::{sonraki_is_gunu, Tatiller};
use crate::icra_tebligat::tr_kucuk;
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};
use postgres::types::ToSql;
use postgres::GenericClient;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

fn hata(e: postgres::Error) -> String {
    // postgres::Error Display yalnız "db error" der; asıl ileti as_db_error()'da.
    match e.as_db_error() {
        Some(d) => format!("veritabanı: {}{}", d.message(), d.detail().map(|x| format!(" ({x})")).unwrap_or_default()),
        None => format!("veritabanı: {e}"),
    }
}

// ── Sabitler ────────────────────────────────────────────────────────────────────────────────────

pub const TURLER: &[(&str, &str)] =
    &[("is", "İş"), ("durusma", "Duruşma"), ("kesif", "Keşif"), ("haciz", "Haciz"), ("rutin", "Rutin")];
pub const DURUMLAR: &[(&str, &str)] = &[
    ("acik", "Açık"),
    ("devam", "Devam ediyor"),
    ("onayda", "Onay bekliyor"),
    ("tamamlandi", "Tamamlandı"),
    ("iptal", "İptal"),
];
pub const ACIK_DURUMLAR: &[&str] = &["acik", "devam", "onayda"];
pub const SONUCLAR: &[(&str, &str)] = &[("olumlu", "Olumlu"), ("olumsuz", "Olumsuz"), ("bekliyor", "Henüz belli değil")];
pub const SURE_TURLERI: &[(&str, &str)] = &[
    ("takvim_gunu", "takvim günü sonra"),
    ("is_gunu", "iş günü sonra"),
    ("ayin_gunu", "ayın şu günü"),
    ("tarih_sor", "tarih sorulsun"),
];
pub const TEKRAR_KURALLARI: &[(&str, &str)] = &[
    ("gunluk", "Her gün"),
    ("is_gunu", "Her iş günü"),
    ("haftalik", "Haftanın belirli günleri"),
    ("aylik", "Her ayın belirli günü"),
    ("ay_basi_is_gunu", "Her ayın ilk iş günü"),
];

/// Olay türleri — geçmiş ve bildirim aynı listeyi kullanır (eski sistemdeki uyuşmazlık bitti).
pub mod olay {
    pub const ACILDI: &str = "acildi";
    pub const GUNCELLENDI: &str = "guncellendi";
    pub const DURUM: &str = "durum";
    pub const SORUMLU: &str = "sorumlu";
    pub const KISI_EKLENDI: &str = "kisi_eklendi";
    pub const KISI_CIKTI: &str = "kisi_cikti";
    pub const NOT: &str = "not";
    pub const EK: &str = "ek";
    pub const ERTELEME_ISTENDI: &str = "erteleme_istendi";
    pub const ERTELENDI: &str = "ertelendi";
    pub const ERTELEME_REDDEDILDI: &str = "erteleme_reddedildi";
    pub const SONRAKI_ACILDI: &str = "sonraki_acildi";
    pub const TASINDI: &str = "tasindi";
    pub const HATIRLATILDI: &str = "hatirlatildi";
}

fn gecerli(liste: &[(&str, &str)], k: &str) -> bool {
    liste.iter().any(|(a, _)| *a == k)
}

pub fn etiket(liste: &[(&str, &str)], k: &str) -> String {
    liste.iter().find(|(a, _)| *a == k).map(|(_, b)| (*b).to_string()).unwrap_or_else(|| k.to_string())
}

// ── Süre ve tekrar hesabı (saf) ─────────────────────────────────────────────────────────────────

fn ay_uzunlugu(yil: i32, ay: u32) -> u32 {
    let (y2, a2) = if ay == 12 { (yil + 1, 1) } else { (yil, ay + 1) };
    NaiveDate::from_ymd_opt(y2, a2, 1).unwrap().pred_opt().unwrap().day()
}

fn ayin_gunu_tarihi(yil: i32, ay: u32, gun: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(yil, ay, gun.clamp(1, ay_uzunlugu(yil, ay))).unwrap()
}

fn sonraki_ay(t: NaiveDate) -> (i32, u32) {
    if t.month() == 12 {
        (t.year() + 1, 1)
    } else {
        (t.year(), t.month() + 1)
    }
}

/// `n` iş günü sonrası (n = 0: bugün iş günüyse bugün, değilse ilk iş günü).
pub fn is_gunu_ekle(baslangic: NaiveDate, n: u32, tatiller: &Tatiller) -> NaiveDate {
    let mut t = sonraki_is_gunu(baslangic, tatiller);
    for _ in 0..n {
        t = sonraki_is_gunu(t + Duration::days(1), tatiller);
    }
    t
}

/// Şablon süresinden son tarih önerisi. `tarih_sor` ve bilinmeyen tür → None (kullanıcı seçer).
pub fn sure_hesapla(bugun: NaiveDate, sure_turu: &str, deger: i32, tatiller: &Tatiller) -> Option<NaiveDate> {
    let d = deger.max(0);
    match sure_turu {
        "takvim_gunu" => Some(bugun + Duration::days(d as i64)),
        "is_gunu" => Some(is_gunu_ekle(bugun, d as u32, tatiller)),
        "ayin_gunu" => {
            let gun = d.clamp(1, 31) as u32;
            let bu_ay = ayin_gunu_tarihi(bugun.year(), bugun.month(), gun);
            let t = if bu_ay > bugun {
                bu_ay
            } else {
                let (y, a) = sonraki_ay(bugun);
                ayin_gunu_tarihi(y, a, gun)
            };
            Some(sonraki_is_gunu(t, tatiller))
        }
        _ => None,
    }
}

/// Süreyi insanın okuyacağı biçimde yazar ("3 iş günü sonra").
pub fn sure_metni(sure_turu: &str, deger: i32) -> String {
    match sure_turu {
        "takvim_gunu" if deger == 0 => "aynı gün".into(),
        "is_gunu" if deger == 0 => "ilk iş günü".into(),
        "takvim_gunu" => format!("{deger} gün sonra"),
        "is_gunu" => format!("{deger} iş günü sonra"),
        "ayin_gunu" => format!("ayın {deger}. günü"),
        _ => "tarih sorulur".into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TekrarKurali {
    pub kural: String,
    pub haftanin_gunleri: Vec<i16>,
    pub ayin_gunu: Option<i16>,
    pub baslangic: NaiveDate,
    pub bitis: Option<NaiveDate>,
}

/// `itibaren` (dahil) ve sonrasında kuralın ilk günü; bitiş geçtiyse ya da kural boşsa None.
pub fn tekrar_tarihi(k: &TekrarKurali, itibaren: NaiveDate, tatiller: &Tatiller) -> Option<NaiveDate> {
    let bas = itibaren.max(k.baslangic);
    let t = match k.kural.as_str() {
        "gunluk" => Some(bas),
        "is_gunu" => Some(sonraki_is_gunu(bas, tatiller)),
        "haftalik" => {
            if k.haftanin_gunleri.is_empty() {
                None
            } else {
                (0..7)
                    .map(|i| bas + Duration::days(i))
                    .find(|t| k.haftanin_gunleri.contains(&(t.weekday().number_from_monday() as i16)))
            }
        }
        "aylik" => k.ayin_gunu.map(|g| {
            let g = g.clamp(1, 31) as u32;
            let bu = ayin_gunu_tarihi(bas.year(), bas.month(), g);
            if bu >= bas {
                bu
            } else {
                let (y, a) = sonraki_ay(bas);
                ayin_gunu_tarihi(y, a, g)
            }
        }),
        "ay_basi_is_gunu" => {
            let bu = sonraki_is_gunu(NaiveDate::from_ymd_opt(bas.year(), bas.month(), 1).unwrap(), tatiller);
            if bu >= bas {
                Some(bu)
            } else {
                let (y, a) = sonraki_ay(bas);
                Some(sonraki_is_gunu(NaiveDate::from_ymd_opt(y, a, 1).unwrap(), tatiller))
            }
        }
        _ => None,
    }?;
    match k.bitis {
        Some(b) if t > b => None,
        _ => Some(t),
    }
}

/// Takvimde göstermek için aralıktaki bütün tekrar günleri (iş açmaz).
pub fn tekrar_gunleri(k: &TekrarKurali, ilk: NaiveDate, son: NaiveDate, tatiller: &Tatiller) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut t = ilk;
    while t <= son && out.len() < 400 {
        match tekrar_tarihi(k, t, tatiller) {
            Some(g) if g <= son => {
                out.push(g);
                t = g + Duration::days(1);
            }
            _ => break,
        }
    }
    out
}

// ── Arama (B-5) ─────────────────────────────────────────────────────────────────────────────────

/// Türkçe harfleri katlayıp küçülten, boşlukları sadeleştiren arama metni.
pub fn arama_katla(parcalar: &[&str]) -> String {
    let birlesik = parcalar.iter().filter(|s| !s.trim().is_empty()).cloned().collect::<Vec<_>>().join(" ");
    tr_kucuk(&birlesik).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Arama sözcükleri (katlanmış); LIKE için % _ \ kaçışlı.
pub fn arama_sozcukleri(sorgu: &str) -> Vec<String> {
    tr_kucuk(sorgu)
        .split_whitespace()
        .take(8)
        .map(|w| w.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
        .collect()
}

// ── Kişi ve yetki ───────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Kisi {
    pub id: i64,
    pub yonetici: bool,
}

/// Buluttaki hesaptan büro kullanıcısını ekler/günceller; kimliği döner.
pub fn kullanici_esitle<C: GenericClient>(c: &mut C, hesap_adi: &str, ad: &str, yonetici: bool, aktif: bool) -> Result<i64, String> {
    let hesap = hesap_adi.trim();
    if hesap.is_empty() {
        return Err("Hesap adı boş olamaz.".into());
    }
    let rol = if yonetici { "yonetici" } else { "uye" };
    // Bulut bazı hesaplarda görünen ad olarak ofis adını veriyor (zeynel@savas → "savas"): o zaman kullanıcı adı.
    let (kullanici_adi, ofis) = hesap.split_once('@').unwrap_or((hesap, ""));
    let ad = ad.trim();
    let ad = if ad.is_empty() || (!ofis.is_empty() && ad.to_lowercase() == ofis.to_lowercase()) {
        if kullanici_adi.is_empty() { hesap } else { kullanici_adi }
    } else {
        ad
    };
    c.query_one(
        "INSERT INTO kullanici (hesap_adi, ad, rol, aktif) VALUES ($1, $2, $3, $4)
         ON CONFLICT (hesap_adi) DO UPDATE SET ad = EXCLUDED.ad, rol = EXCLUDED.rol, aktif = EXCLUDED.aktif,
             guncelleme = now()
         RETURNING id",
        &[&hesap, &ad, &rol, &aktif],
    )
    .map(|r| r.get(0))
    .map_err(hata)
}

pub fn kisi_bul<C: GenericClient>(c: &mut C, hesap_adi: &str) -> Result<Option<Kisi>, String> {
    Ok(c
        .query_opt("SELECT id, rol FROM kullanici WHERE hesap_adi = $1 AND aktif", &[&hesap_adi.trim()])
        .map_err(hata)?
        .map(|r| Kisi { id: r.get(0), yonetici: r.get::<_, String>(1) == "yonetici" }))
}

pub fn kullanicilar<C: GenericClient>(c: &mut C) -> Result<Vec<Value>, String> {
    Ok(c
        .query("SELECT id, hesap_adi, ad, rol FROM kullanici WHERE aktif ORDER BY ad", &[])
        .map_err(hata)?
        .iter()
        .map(|r| json!({"id": r.get::<_, i64>(0), "hesap": r.get::<_, String>(1), "ad": r.get::<_, String>(2), "rol": r.get::<_, String>(3)}))
        .collect())
}

/// Bir işle ilgili kişinin rolleri.
#[derive(Clone, Debug, Default)]
pub struct Yetki {
    pub gorebilir: bool,
    pub calisabilir: bool, // durum değiştirir, not ekler, erteleme ister
    pub onaylayabilir: bool,
    pub yonetebilir: bool, // düzenler, iptal eder, kişi ekler
}

#[derive(Clone, Debug)]
pub struct GorevOzu {
    pub id: i64,
    pub ust_gorev_id: Option<i64>,
    pub durum: String,
    pub onay_gerekir: bool,
    pub gizli: bool,
    pub sorumlu_id: Option<i64>,
    pub olusturan_id: Option<i64>,
    pub son_tarih: NaiveDate,
    pub son_saat: Option<NaiveTime>,
    pub sablon_id: Option<i64>,
    pub tekrar_id: Option<i64>,
}

pub fn gorev_ozu<C: GenericClient>(c: &mut C, id: i64) -> Result<Option<GorevOzu>, String> {
    Ok(c
        .query_opt(
            "SELECT id, ust_gorev_id, durum, onay_gerekir, gizli, sorumlu_id, olusturan_id, son_tarih, son_saat, sablon_id, tekrar_id
             FROM gorev WHERE id = $1",
            &[&id],
        )
        .map_err(hata)?
        .map(|r| GorevOzu {
            id: r.get(0),
            ust_gorev_id: r.get(1),
            durum: r.get(2),
            onay_gerekir: r.get(3),
            gizli: r.get(4),
            sorumlu_id: r.get(5),
            olusturan_id: r.get(6),
            son_tarih: r.get(7),
            son_saat: r.get(8),
            sablon_id: r.get(9),
            tekrar_id: r.get(10),
        }))
}

/// Saf yetki kuralı. `kisi_rolleri`: kişinin bu işteki ek rolleri (gorev_kisi).
pub fn yetki_hesapla(g: &GorevOzu, k: &Kisi, kisi_rolleri: &[String]) -> Yetki {
    let sahip = g.olusturan_id == Some(k.id) || g.sorumlu_id == Some(k.id);
    if g.gizli {
        return Yetki { gorebilir: sahip, calisabilir: sahip, onaylayabilir: sahip, yonetebilir: sahip };
    }
    let ek_sorumlu = kisi_rolleri.iter().any(|r| r == "sorumlu");
    let onaylayici = kisi_rolleri.iter().any(|r| r == "onaylayici");
    let olusturan = g.olusturan_id == Some(k.id);
    // Onay: yönetici, atanmış onaylayıcı ya da işi başkasına açan kişi.
    let onaylayabilir = k.yonetici || onaylayici || (olusturan && g.sorumlu_id != Some(k.id));
    Yetki {
        gorebilir: true,
        calisabilir: k.yonetici || sahip || ek_sorumlu,
        onaylayabilir,
        yonetebilir: k.yonetici || olusturan,
    }
}

pub fn yetki<C: GenericClient>(c: &mut C, g: &GorevOzu, k: &Kisi) -> Result<Yetki, String> {
    let roller: Vec<String> = c
        .query("SELECT rol FROM gorev_kisi WHERE gorev_id = $1 AND kullanici_id = $2", &[&g.id, &k.id])
        .map_err(hata)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    Ok(yetki_hesapla(g, k, &roller))
}

/// İstenen durum geçişinin sonucu: gerçekte yazılacak durum. Saf kural.
///   "tamamla" isteği: onay gerekmiyorsa (ya da onaylayabilen kişi istiyorsa) tamamlandi, değilse onayda.
pub fn gecis_coz(g: &GorevOzu, istenen: &str, y: &Yetki) -> Result<&'static str, String> {
    let cur = g.durum.as_str();
    let red = |m: &str| -> Result<&'static str, String> { Err(m.to_string()) };
    if !y.gorebilir {
        return red("İş bulunamadı.");
    }
    match istenen {
        "acik" | "devam" => {
            let hedef = if istenen == "acik" { "acik" } else { "devam" };
            match cur {
                "acik" | "devam" if y.calisabilir || y.onaylayabilir => Ok(hedef),
                // Onaydan geri çevirme (onaylayan) ya da geri çekme (sorumlu)
                "onayda" if y.calisabilir || y.onaylayabilir => Ok(hedef),
                // Kapanmış işi yeniden açma
                "tamamlandi" | "iptal" if y.onaylayabilir || y.yonetebilir => Ok(hedef),
                "tamamlandi" | "iptal" => red("Bu işi yeniden açma yetkiniz yok."),
                _ => red("Bu işte durum değiştirme yetkiniz yok."),
            }
        }
        "tamamla" | "tamamlandi" | "onayda" => {
            match cur {
                "acik" | "devam" => {}
                "onayda" if istenen == "onayda" => return red("İş zaten onay bekliyor."),
                "onayda" if y.onaylayabilir => return Ok("tamamlandi"),
                "onayda" => return red("Bu işi onaylama yetkiniz yok."),
                _ => return red("İş kapanmış; önce yeniden açın."),
            }
            if !y.calisabilir && !y.onaylayabilir {
                return red("Bu işte durum değiştirme yetkiniz yok.");
            }
            if g.ust_gorev_id.is_some() || !g.onay_gerekir {
                Ok("tamamlandi")
            } else if istenen == "onayda" || !y.onaylayabilir {
                Ok("onayda")
            } else {
                // Onaylayabilen kişi onay gereken işi doğrudan kapatabilir.
                Ok("tamamlandi")
            }
        }
        "iptal" => {
            if !y.yonetebilir {
                return red("İşi yalnız açan kişi ya da yönetici iptal edebilir.");
            }
            if !ACIK_DURUMLAR.contains(&cur) {
                return red("İş zaten kapanmış.");
            }
            Ok("iptal")
        }
        _ => red("Bilinmeyen durum."),
    }
}

// ── İş açma / güncelleme ────────────────────────────────────────────────────────────────────────

/// Yeni iş. Boş bırakılan alanlar varsayılanını alır; dosya künye metinleri dosya satırından
/// doldurulur (verilmemişse).
#[derive(Clone, Debug, Default)]
pub struct YeniGorev {
    pub ust_gorev_id: Option<i64>,
    pub baslik: String,
    pub aciklama: String,
    pub tur: String,
    pub onay_gerekir: bool,
    pub oncelik: i16,
    pub gizli: bool,
    pub sorumlu_id: Option<i64>,
    pub son_tarih: Option<NaiveDate>,
    pub son_saat: Option<NaiveTime>,
    pub bitis_saati: Option<NaiveTime>,
    pub yer: String,
    pub dosya_id: Option<i64>,
    pub taraf_id: Option<i64>,
    pub evrak_id: Option<i64>,
    pub tebligat_id: Option<i64>,
    pub birim_adi: String,
    pub dosya_no: String,
    pub dosya_turu: String,
    pub taraf_adi: String,
    pub evrak_adi: String,
    pub barkod: String,
    pub uyap_karti: Option<Value>,
    pub sablon_id: Option<i64>,
    pub surec_asama_id: Option<i64>,
    pub onceki_gorev_id: Option<i64>,
    pub tekrar_id: Option<i64>,
    pub durum_olayi_id: Option<i64>,
    pub eski_id: Option<i64>,
    pub etiketler: Vec<i64>,
    pub onaylayicilar: Vec<i64>,
    pub ek_sorumlular: Vec<i64>,
}

impl YeniGorev {
    /// Ekran/uç gövdesinden (JSON). Tarih "YYYY-AA-GG", saat "SS:DD".
    pub fn jsondan(g: &Value) -> Result<Self, String> {
        let m = |k: &str| g.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let i = |k: &str| g.get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
        let b = |k: &str| g.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
        let liste = |k: &str| -> Vec<i64> {
            g.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default()
        };
        Ok(YeniGorev {
            ust_gorev_id: i("ust_gorev_id"),
            baslik: m("baslik"),
            aciklama: g.get("aciklama").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            tur: m("tur"),
            onay_gerekir: b("onay_gerekir"),
            oncelik: i("oncelik").unwrap_or(1) as i16,
            gizli: b("gizli"),
            sorumlu_id: i("sorumlu_id"),
            son_tarih: tarih_coz(&m("son_tarih"))?,
            son_saat: saat_coz(&m("son_saat"))?,
            bitis_saati: saat_coz(&m("bitis_saati"))?,
            yer: m("yer"),
            dosya_id: i("dosya_id"),
            taraf_id: i("taraf_id"),
            evrak_id: i("evrak_id"),
            tebligat_id: i("tebligat_id"),
            birim_adi: m("birim_adi"),
            dosya_no: m("dosya_no"),
            dosya_turu: m("dosya_turu"),
            taraf_adi: m("taraf_adi"),
            evrak_adi: m("evrak_adi"),
            barkod: m("barkod"),
            uyap_karti: g.get("uyap_karti").filter(|v| v.is_object()).cloned(),
            sablon_id: i("sablon_id"),
            surec_asama_id: i("surec_asama_id"),
            onceki_gorev_id: i("onceki_gorev_id"),
            tekrar_id: None,
            durum_olayi_id: None,
            eski_id: None,
            etiketler: liste("etiketler"),
            onaylayicilar: liste("onaylayicilar"),
            ek_sorumlular: liste("ek_sorumlular"),
        })
    }
}

pub fn tarih_coz(s: &str) -> Result<Option<NaiveDate>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%d.%m.%Y"))
        .map(Some)
        .map_err(|_| format!("Tarih anlaşılamadı: {s}"))
}

pub fn saat_coz(s: &str) -> Result<Option<NaiveTime>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S"))
        .map(Some)
        .map_err(|_| format!("Saat anlaşılamadı: {s}"))
}

pub(crate) fn olay_yaz<C: GenericClient>(c: &mut C, gorev: Option<i64>, kisi: Option<i64>, tur: &str, ayrinti: Value) -> Result<i64, String> {
    c.query_one(
        "INSERT INTO gorev_olay (gorev_id, kullanici_id, tur, ayrinti) VALUES ($1, $2, $3, $4) RETURNING id",
        &[&gorev, &kisi, &tur, &ayrinti],
    )
    .map(|r| r.get(0))
    .map_err(hata)
}

pub(crate) fn arama_metnini_yenile<C: GenericClient>(c: &mut C, id: i64) -> Result<(), String> {
    let r = c
        .query_one(
            "SELECT baslik, aciklama, birim_adi, dosya_no, taraf_adi, evrak_adi, barkod, yer FROM gorev WHERE id = $1",
            &[&id],
        )
        .map_err(hata)?;
    let p: Vec<String> = (0..8).map(|i| r.get::<_, String>(i)).collect();
    let metin = arama_katla(&p.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    c.execute("UPDATE gorev SET arama_metni = $2 WHERE id = $1", &[&id, &metin]).map_err(hata)?;
    Ok(())
}

pub fn resmi_tatiller<C: GenericClient>(c: &mut C) -> Result<Tatiller, String> {
    crate::icra_veri::resmi_tatiller(c)
}

/// İş açar; kimliğini döner. `acan` NULL olabilir (otomatik/taşıma).
pub fn gorev_ac<C: GenericClient>(c: &mut C, acan: Option<&Kisi>, y: &YeniGorev, bugun: NaiveDate) -> Result<i64, String> {
    let baslik = y.baslik.trim();
    if baslik.is_empty() {
        return Err("İşin başlığı boş olamaz.".into());
    }
    let tur = if y.tur.is_empty() { "is" } else { y.tur.as_str() };
    if !gecerli(TURLER, tur) {
        return Err(format!("Bilinmeyen iş türü: {tur}"));
    }
    let mut tx = c.transaction().map_err(hata)?;
    // Üst iş varsa alt iş onun dosya bağını ve gizliliğini paylaşır.
    let mut y = y.clone();
    if let Some(ust) = y.ust_gorev_id {
        let u = gorev_ozu(&mut tx, ust)?.ok_or("Üst iş bulunamadı.")?;
        if u.ust_gorev_id.is_some() {
            return Err("Alt işin altına iş açılamaz.".into());
        }
        if let Some(k) = acan {
            if !yetki(&mut tx, &u, k)?.calisabilir {
                return Err("Bu işe alt iş ekleme yetkiniz yok.".into());
            }
        }
        y.gizli = u.gizli;
        y.onay_gerekir = false;
        if y.son_tarih.is_none() {
            y.son_tarih = Some(u.son_tarih);
        }
        if y.sorumlu_id.is_none() {
            y.sorumlu_id = u.sorumlu_id;
        }
    }
    // Şablondan eksikleri tamamla (süre, onay, öncelik).
    if let Some(sid) = y.sablon_id {
        if let Some(r) = tx
            .query_opt("SELECT sure_turu, sure_deger, onay_gerekir, surec_asama_id FROM sablon WHERE id = $1", &[&sid])
            .map_err(hata)?
        {
            if y.son_tarih.is_none() {
                let tatil = resmi_tatiller(&mut tx)?;
                y.son_tarih = sure_hesapla(bugun, &r.get::<_, String>(0), r.get(1), &tatil);
            }
            y.onay_gerekir = y.onay_gerekir || r.get::<_, bool>(2);
            if y.surec_asama_id.is_none() {
                y.surec_asama_id = r.get(3);
            }
        }
    }
    let son_tarih = y.son_tarih.ok_or("Son tarih seçilmeli.")?;
    let sorumlu = y.sorumlu_id.or(acan.map(|k| k.id));
    // Dosya künye metinleri dosya satırından
    if let Some(did) = y.dosya_id {
        if let Some(r) = tx
            .query_opt("SELECT b.ad, d.dosya_no, d.tur FROM dosya d JOIN birim b ON b.id = d.birim_id WHERE d.id = $1", &[&did])
            .map_err(hata)?
        {
            if y.birim_adi.is_empty() {
                y.birim_adi = r.get(0);
            }
            if y.dosya_no.is_empty() {
                y.dosya_no = r.get(1);
            }
            if y.dosya_turu.is_empty() {
                y.dosya_turu = r.get(2);
            }
        } else {
            return Err("Bağlanan dosya veritabanında yok.".into());
        }
    }
    let oncelik = y.oncelik.clamp(0, 2);
    let acan_id = acan.map(|k| k.id);
    let p: &[&(dyn ToSql + Sync)] = &[
        &y.ust_gorev_id,
        &baslik,
        &y.aciklama,
        &tur,
        &y.onay_gerekir,
        &oncelik,
        &y.gizli,
        &sorumlu,
        &acan_id,
        &son_tarih,
        &y.son_saat,
        &y.bitis_saati,
        &y.yer,
        &y.dosya_id,
        &y.taraf_id,
        &y.evrak_id,
        &y.tebligat_id,
        &y.birim_adi,
        &y.dosya_no,
        &y.dosya_turu,
        &y.taraf_adi,
        &y.evrak_adi,
        &y.barkod,
        &y.uyap_karti,
        &y.sablon_id,
        &y.surec_asama_id,
        &y.onceki_gorev_id,
        &y.tekrar_id,
        &y.durum_olayi_id,
        &y.eski_id,
    ];
    let id: i64 = tx
        .query_one(
            "INSERT INTO gorev (ust_gorev_id, baslik, aciklama, tur, onay_gerekir, oncelik, gizli, sorumlu_id, olusturan_id,
                son_tarih, son_saat, bitis_saati, yer, dosya_id, taraf_id, evrak_id, tebligat_id, birim_adi, dosya_no,
                dosya_turu, taraf_adi, evrak_adi, barkod, uyap_karti, sablon_id, surec_asama_id, onceki_gorev_id,
                tekrar_id, durum_olayi_id, eski_id)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30)
             RETURNING id",
            p,
        )
        .map_err(hata)?
        .get(0);
    for e in &y.etiketler {
        tx.execute("INSERT INTO gorev_etiket (gorev_id, etiket_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[&id, e])
            .map_err(hata)?;
    }
    if let Some(sid) = y.sablon_id {
        tx.execute(
            "INSERT INTO gorev_etiket (gorev_id, etiket_id) SELECT $1, etiket_id FROM sablon_etiket WHERE sablon_id = $2 ON CONFLICT DO NOTHING",
            &[&id, &sid],
        )
        .map_err(hata)?;
    }
    for (liste, rol) in [(&y.onaylayicilar, "onaylayici"), (&y.ek_sorumlular, "sorumlu")] {
        for u in liste.iter() {
            tx.execute(
                "INSERT INTO gorev_kisi (gorev_id, kullanici_id, rol, ekleyen_id) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                &[&id, u, &rol, &acan_id],
            )
            .map_err(hata)?;
        }
    }
    arama_metnini_yenile(&mut tx, id)?;
    olay_yaz(&mut tx, Some(id), acan_id, olay::ACILDI, json!({"sorumlu": sorumlu, "son_tarih": son_tarih.to_string()}))?;
    if let Some(onceki) = y.onceki_gorev_id {
        olay_yaz(&mut tx, Some(onceki), acan_id, olay::SONRAKI_ACILDI, json!({"gorev": id}))?;
    }
    tx.commit().map_err(hata)?;
    Ok(id)
}

/// İşin düzenlenebilir alanlarını günceller (yalnız gövdede bulunanlar).
pub fn gorev_guncelle<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, g: &Value) -> Result<(), String> {
    let mut tx = c.transaction().map_err(hata)?;
    let oz = gorev_ozu(&mut tx, id)?.ok_or("İş bulunamadı.")?;
    let yt = yetki(&mut tx, &oz, k)?;
    if !yt.gorebilir {
        return Err("İş bulunamadı.".into());
    }
    if !yt.yonetebilir && !yt.calisabilir {
        return Err("Bu işi düzenleme yetkiniz yok.".into());
    }
    let mut degisen = Map::new();
    let metin_alanlari = [("baslik", 300usize), ("aciklama", 100_000), ("yer", 300)];
    for (alan, uz) in metin_alanlari {
        if let Some(v) = g.get(alan).and_then(|v| v.as_str()) {
            let v: String = v.chars().take(uz).collect();
            if alan == "baslik" && v.trim().is_empty() {
                return Err("İşin başlığı boş olamaz.".into());
            }
            tx.execute(&format!("UPDATE gorev SET {alan} = $2 WHERE id = $1"), &[&id, &v.trim()]).map_err(hata)?;
            degisen.insert(alan.into(), json!(v.trim()));
        }
    }
    if let Some(v) = g.get("son_tarih").and_then(|v| v.as_str()) {
        // Tarihi doğrudan değiştirmek: yalnız yöneten (sorumlu "ertele" kullanır).
        if !yt.yonetebilir {
            return Err("Son tarihi değiştirmek için 'Ertele'yi kullanın.".into());
        }
        let t = tarih_coz(v)?.ok_or("Son tarih boş olamaz.")?;
        tx.execute("UPDATE gorev SET son_tarih = $2 WHERE id = $1", &[&id, &t]).map_err(hata)?;
        degisen.insert("son_tarih".into(), json!(t.to_string()));
    }
    if let Some(v) = g.get("son_saat").and_then(|v| v.as_str()) {
        let s = saat_coz(v)?;
        tx.execute("UPDATE gorev SET son_saat = $2 WHERE id = $1", &[&id, &s]).map_err(hata)?;
        degisen.insert("son_saat".into(), json!(v));
    }
    if let Some(v) = g.get("oncelik").and_then(|v| v.as_i64()) {
        let o = v.clamp(0, 2) as i16;
        tx.execute("UPDATE gorev SET oncelik = $2 WHERE id = $1", &[&id, &o]).map_err(hata)?;
        degisen.insert("oncelik".into(), json!(o));
    }
    for alan in ["onay_gerekir", "gizli"] {
        if let Some(v) = g.get(alan).and_then(|v| v.as_bool()) {
            if !yt.yonetebilir {
                return Err("Bu ayarı yalnız işi açan kişi ya da yönetici değiştirebilir.".into());
            }
            tx.execute(&format!("UPDATE gorev SET {alan} = $2 WHERE id = $1"), &[&id, &v]).map_err(hata)?;
            degisen.insert(alan.into(), json!(v));
        }
    }
    if let Some(v) = g.get("sorumlu_id") {
        if !yt.yonetebilir {
            return Err("Sorumluyu yalnız işi açan kişi ya da yönetici değiştirebilir.".into());
        }
        let s = v.as_i64();
        tx.execute("UPDATE gorev SET sorumlu_id = $2 WHERE id = $1", &[&id, &s]).map_err(hata)?;
        olay_yaz(&mut tx, Some(id), Some(k.id), olay::SORUMLU, json!({"eski": oz.sorumlu_id, "yeni": s}))?;
    }
    if let Some(arr) = g.get("etiketler").and_then(|v| v.as_array()) {
        tx.execute("DELETE FROM gorev_etiket WHERE gorev_id = $1", &[&id]).map_err(hata)?;
        for e in arr.iter().filter_map(|x| x.as_i64()) {
            tx.execute("INSERT INTO gorev_etiket (gorev_id, etiket_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[&id, &e])
                .map_err(hata)?;
        }
        degisen.insert("etiketler".into(), json!(arr));
    }
    tx.execute("UPDATE gorev SET guncelleme = now() WHERE id = $1", &[&id]).map_err(hata)?;
    arama_metnini_yenile(&mut tx, id)?;
    if !degisen.is_empty() {
        olay_yaz(&mut tx, Some(id), Some(k.id), olay::GUNCELLENDI, Value::Object(degisen))?;
    }
    tx.commit().map_err(hata)
}

// ── Durum, sonuç ve sonraki iş önerisi ─────────────────────────────────────────────────────────

/// Sonraki iş önerisi (ekran onaylatır; iş açmaz).
#[derive(Clone, Debug, PartialEq)]
pub struct Oneri {
    pub sablon_id: i64,
    pub baslik: String,
    pub sonuc_adi: String,
    pub son_tarih: Option<NaiveDate>,
    pub sure_metni: String,
    pub onay_gerekir: bool,
    pub sorumlu_id: Option<i64>,
}

impl Oneri {
    pub fn json(&self) -> Value {
        json!({
            "sablon_id": self.sablon_id, "baslik": self.baslik, "sonuc_adi": self.sonuc_adi,
            "son_tarih": self.son_tarih.map(|t| t.to_string()), "sure_metni": self.sure_metni,
            "onay_gerekir": self.onay_gerekir, "sorumlu_id": self.sorumlu_id,
        })
    }
}

/// İşin şablonuna ve sonuca göre sonraki iş önerisi.
pub fn sonraki_oneri<C: GenericClient>(c: &mut C, gorev: i64, sonuc: &str, bugun: NaiveDate) -> Result<Option<Oneri>, String> {
    let Some(oz) = gorev_ozu(c, gorev)? else { return Ok(None) };
    let Some(sid) = oz.sablon_id else { return Ok(None) };
    let Some(r) = c
        .query_opt(
            "SELECT coalesce(ss.sonraki_sablon_id, ss.sablon_id), ss.sonuc_adi,
                    coalesce(ss.sure_turu, h.sure_turu), coalesce(ss.sure_deger, h.sure_deger), h.baslik, h.onay_gerekir
             FROM sablon_sonraki ss JOIN sablon h ON h.id = coalesce(ss.sonraki_sablon_id, ss.sablon_id)
             WHERE ss.sablon_id = $1 AND ss.sonuc = $2 AND h.aktif",
            &[&sid, &sonuc],
        )
        .map_err(hata)?
    else {
        return Ok(None);
    };
    let tatil = resmi_tatiller(c)?;
    let sure_turu: String = r.get(2);
    let deger: i32 = r.get(3);
    Ok(Some(Oneri {
        sablon_id: r.get(0),
        baslik: r.get(4),
        sonuc_adi: r.get(1),
        son_tarih: sure_hesapla(bugun, &sure_turu, deger, &tatil),
        sure_metni: sure_metni(&sure_turu, deger),
        onay_gerekir: r.get(5),
        sorumlu_id: oz.sorumlu_id,
    }))
}

#[derive(Clone, Debug)]
pub struct DurumSonucu {
    pub durum: String,
    pub oneri: Option<Oneri>,
}

/// Durum değiştirir. `istenen`: acik / devam / tamamla / onayda / iptal.
/// Kapatırken (tamamla/onayda) sonuç verilmişse sonraki iş önerisi döner (B-7).
pub fn durum_degistir<C: GenericClient>(
    c: &mut C,
    k: &Kisi,
    id: i64,
    istenen: &str,
    sonuc: &str,
    not: &str,
    bugun: NaiveDate,
) -> Result<DurumSonucu, String> {
    if !sonuc.is_empty() && !gecerli(SONUCLAR, sonuc) {
        return Err("Bilinmeyen sonuç.".into());
    }
    let mut tx = c.transaction().map_err(hata)?;
    let oz = gorev_ozu(&mut tx, id)?.ok_or("İş bulunamadı.")?;
    let yt = yetki(&mut tx, &oz, k)?;
    if !yt.gorebilir {
        return Err("İş bulunamadı.".into());
    }
    let yeni = gecis_coz(&oz, istenen, &yt)?;
    let kapaniyor = matches!(yeni, "onayda" | "tamamlandi");
    if yeni == "tamamlandi" && oz.ust_gorev_id.is_none() {
        let acik_alt: i64 = tx
            .query_one(
                "SELECT count(*) FROM gorev WHERE ust_gorev_id = $1 AND durum IN ('acik','devam','onayda')",
                &[&id],
            )
            .map_err(hata)?
            .get(0);
        if acik_alt > 0 {
            return Err(format!("Önce {acik_alt} açık alt işi tamamlayın."));
        }
    }
    tx.execute(
        "UPDATE gorev SET durum = $2::varchar, guncelleme = now(),
             onaya_sunulma = CASE WHEN $2::varchar = 'onayda' THEN now() WHEN $2::varchar IN ('acik','devam') THEN NULL ELSE onaya_sunulma END,
             tamamlanma    = CASE WHEN $2::varchar = 'tamamlandi' THEN now() WHEN $2::varchar IN ('acik','devam','onayda') THEN NULL ELSE tamamlanma END,
             tamamlayan_id = CASE WHEN $2::varchar = 'tamamlandi' THEN $3::bigint WHEN $2::varchar IN ('acik','devam','onayda') THEN NULL ELSE tamamlayan_id END,
             sonuc         = CASE WHEN $4::varchar <> '' THEN $4::varchar WHEN $2::varchar IN ('acik','devam') THEN '' ELSE sonuc END,
             sonuc_notu    = CASE WHEN $4::varchar <> '' THEN $5::text ELSE sonuc_notu END
         WHERE id = $1",
        &[&id, &yeni, &k.id, &sonuc, &not],
    )
    .map_err(hata)?;
    olay_yaz(&mut tx, Some(id), Some(k.id), olay::DURUM, json!({"eski": oz.durum, "yeni": yeni, "sonuc": sonuc, "not": not}))?;
    if !not.trim().is_empty() {
        tx.execute("INSERT INTO gorev_not (gorev_id, yazan_id, metin) VALUES ($1, $2, $3)", &[&id, &k.id, &not.trim()])
            .map_err(hata)?;
    }
    // Onay beklenen erteleme isteği iş kapanınca anlamsızlaşır.
    if kapaniyor || yeni == "iptal" {
        tx.execute(
            "UPDATE gorev_erteleme SET durum = 'ret', karar_notu = 'İş kapandı', karar_zamani = now() WHERE gorev_id = $1 AND durum = 'bekliyor'",
            &[&id],
        )
        .map_err(hata)?;
    }
    // Öneri: onaya sunarken de hemen (B-7). Onaylanınca (onayda → tamamlandi) yeniden önerilmez.
    let oneri = if kapaniyor && !sonuc.is_empty() && oz.durum != "onayda" {
        sonraki_oneri(&mut tx, id, sonuc, bugun)?
    } else {
        None
    };
    tx.commit().map_err(hata)?;
    Ok(DurumSonucu { durum: yeni.to_string(), oneri })
}

/// Öneriyi (gerekirse değiştirilmiş tarih/sorumluyla) iş olarak açar; önceki işin dosya
/// bağlarını taşır.
pub fn oneriden_ac<C: GenericClient>(
    c: &mut C,
    k: &Kisi,
    onceki: i64,
    sablon_id: i64,
    son_tarih: NaiveDate,
    son_saat: Option<NaiveTime>,
    sorumlu_id: Option<i64>,
    bugun: NaiveDate,
) -> Result<i64, String> {
    let r = c
        .query_one(
            "SELECT g.dosya_id, g.taraf_id, g.evrak_id, g.tebligat_id, g.birim_adi, g.dosya_no, g.dosya_turu, g.taraf_adi,
                    g.barkod, g.uyap_karti, g.oncelik, g.gizli, g.sorumlu_id, s.baslik, s.aciklama, s.tur, s.onay_gerekir, s.taraf
             FROM gorev g, sablon s WHERE g.id = $1 AND s.id = $2",
            &[&onceki, &sablon_id],
        )
        .map_err(|e| format!("Önceki iş ya da şablon bulunamadı ({e})."))?;
    let oz = gorev_ozu(c, onceki)?.ok_or("Önceki iş bulunamadı.")?;
    if !yetki(c, &oz, k)?.gorebilir {
        return Err("Önceki iş bulunamadı.".into());
    }
    let taraf_adi: String = r.get(7);
    let sablon_baslik: String = r.get(13);
    let taraf_li: String = r.get(17);
    let baslik = if !taraf_li.is_empty() && !taraf_adi.is_empty() { format!("{taraf_adi} – {sablon_baslik}") } else { sablon_baslik };
    let y = YeniGorev {
        baslik,
        aciklama: r.get(14),
        tur: r.get(15),
        onay_gerekir: r.get(16),
        oncelik: r.get(10),
        gizli: r.get(11),
        sorumlu_id: sorumlu_id.or(r.get(12)),
        son_tarih: Some(son_tarih),
        son_saat,
        dosya_id: r.get(0),
        taraf_id: r.get(1),
        evrak_id: r.get(2),
        tebligat_id: r.get(3),
        birim_adi: r.get(4),
        dosya_no: r.get(5),
        dosya_turu: r.get(6),
        taraf_adi,
        barkod: r.get(8),
        uyap_karti: r.get(9),
        sablon_id: Some(sablon_id),
        onceki_gorev_id: Some(onceki),
        ..Default::default()
    };
    gorev_ac(c, Some(k), &y, bugun)
}

// ── Erteleme ("Ertesi güne aktar", B-15) ───────────────────────────────────────────────────────

/// Erteleme ister. Onay gerekmeyen işte (ya da onaylayabilen kişi isterse) hemen uygulanır.
/// Dönüş: true = uygulandı, false = onaya gitti.
pub fn ertele<C: GenericClient>(
    c: &mut C,
    k: &Kisi,
    id: i64,
    yeni_tarih: NaiveDate,
    yeni_saat: Option<NaiveTime>,
    gerekce: &str,
) -> Result<bool, String> {
    let mut tx = c.transaction().map_err(hata)?;
    let oz = gorev_ozu(&mut tx, id)?.ok_or("İş bulunamadı.")?;
    let yt = yetki(&mut tx, &oz, k)?;
    if !yt.gorebilir {
        return Err("İş bulunamadı.".into());
    }
    if !yt.calisabilir && !yt.onaylayabilir {
        return Err("Bu işi erteleme yetkiniz yok.".into());
    }
    if !ACIK_DURUMLAR.contains(&oz.durum.as_str()) {
        return Err("Kapanmış iş ertelenemez.".into());
    }
    let hemen = !oz.onay_gerekir || yt.onaylayabilir;
    let durum = if hemen { "kabul" } else { "bekliyor" };
    tx.execute(
        "UPDATE gorev_erteleme SET durum = 'ret', karar_notu = 'Yeni istek', karar_zamani = now() WHERE gorev_id = $1 AND durum = 'bekliyor'",
        &[&id],
    )
    .map_err(hata)?;
    tx.execute(
        "INSERT INTO gorev_erteleme (gorev_id, isteyen_id, eski_tarih, eski_saat, yeni_tarih, yeni_saat, gerekce, durum,
             karar_veren_id, karar_zamani)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, CASE WHEN $8::varchar = 'kabul' THEN $2::bigint END, CASE WHEN $8::varchar = 'kabul' THEN now() END)",
        &[&id, &k.id, &oz.son_tarih, &oz.son_saat, &yeni_tarih, &yeni_saat, &gerekce.trim(), &durum],
    )
    .map_err(hata)?;
    let ayr = json!({"eski": oz.son_tarih.to_string(), "yeni": yeni_tarih.to_string(), "gerekce": gerekce.trim()});
    if hemen {
        tx.execute("UPDATE gorev SET son_tarih = $2, son_saat = $3, guncelleme = now() WHERE id = $1", &[&id, &yeni_tarih, &yeni_saat])
            .map_err(hata)?;
        olay_yaz(&mut tx, Some(id), Some(k.id), olay::ERTELENDI, ayr)?;
    } else {
        olay_yaz(&mut tx, Some(id), Some(k.id), olay::ERTELEME_ISTENDI, ayr)?;
    }
    tx.commit().map_err(hata)?;
    Ok(hemen)
}

pub fn erteleme_karar<C: GenericClient>(c: &mut C, k: &Kisi, erteleme_id: i64, kabul: bool, notu: &str) -> Result<(), String> {
    let mut tx = c.transaction().map_err(hata)?;
    let r = tx
        .query_opt("SELECT gorev_id, yeni_tarih, yeni_saat, durum FROM gorev_erteleme WHERE id = $1", &[&erteleme_id])
        .map_err(hata)?
        .ok_or("Erteleme isteği bulunamadı.")?;
    let gid: i64 = r.get(0);
    if r.get::<_, String>(3) != "bekliyor" {
        return Err("Bu istek zaten karara bağlanmış.".into());
    }
    let oz = gorev_ozu(&mut tx, gid)?.ok_or("İş bulunamadı.")?;
    if !yetki(&mut tx, &oz, k)?.onaylayabilir {
        return Err("Erteleme kararı verme yetkiniz yok.".into());
    }
    let durum = if kabul { "kabul" } else { "ret" };
    tx.execute(
        "UPDATE gorev_erteleme SET durum = $2, karar_veren_id = $3, karar_notu = $4, karar_zamani = now() WHERE id = $1",
        &[&erteleme_id, &durum, &k.id, &notu.trim()],
    )
    .map_err(hata)?;
    let yeni_tarih: NaiveDate = r.get(1);
    let yeni_saat: Option<NaiveTime> = r.get(2);
    if kabul {
        tx.execute("UPDATE gorev SET son_tarih = $2, son_saat = $3, guncelleme = now() WHERE id = $1", &[&gid, &yeni_tarih, &yeni_saat])
            .map_err(hata)?;
    }
    olay_yaz(
        &mut tx,
        Some(gid),
        Some(k.id),
        if kabul { olay::ERTELENDI } else { olay::ERTELEME_REDDEDILDI },
        json!({"yeni": yeni_tarih.to_string(), "not": notu.trim()}),
    )?;
    tx.commit().map_err(hata)
}

// ── Not, kişi ───────────────────────────────────────────────────────────────────────────────────

// ── Ek dosyalar (ofis diskinde %LOCALAPPDATA%\CokluUyap\ekler\<sha256>; buluta gitmez) ─────────────

/// Tek ekin en büyük boyutu.
pub const EK_EN_BUYUK: usize = 25 * 1024 * 1024;

fn ek_adi_temizle(ad: &str) -> String {
    let t: String = ad
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.trim_matches(|c| c == '.' || c == ' ').chars().take(200).collect();
    if t.is_empty() { "belge".into() } else { t }
}

/// İşe ek yükler (işi görebilen herkes; not eklemekle aynı kural). Kimliği döner.
pub fn ek_ekle<C: GenericClient>(c: &mut C, k: &Kisi, gorev: i64, ad: &str, mime: &str, veri: &[u8]) -> Result<i64, String> {
    if veri.is_empty() {
        return Err("Dosya boş.".into());
    }
    if veri.len() > EK_EN_BUYUK {
        return Err(format!("Dosya çok büyük (en çok {} MB).", EK_EN_BUYUK / 1024 / 1024));
    }
    let oz = gorev_ozu(c, gorev)?.ok_or("İş bulunamadı.")?;
    if !yetki(c, &oz, k)?.gorebilir {
        return Err("İş bulunamadı.".into());
    }
    use sha2::{Digest, Sha256};
    let sha = hex::encode(Sha256::digest(veri));
    let dizin = crate::is_aktar::ekler_dizini();
    std::fs::create_dir_all(&dizin).map_err(|e| format!("Ekler klasörü açılamadı: {e}"))?;
    let yol = dizin.join(&sha);
    if !yol.exists() {
        let gecici = dizin.join(format!("{sha}.yarim"));
        std::fs::write(&gecici, veri).map_err(|e| format!("Dosya yazılamadı: {e}"))?;
        std::fs::rename(&gecici, &yol).map_err(|e| format!("Dosya yazılamadı: {e}"))?;
    }
    let ad = ek_adi_temizle(ad);
    let mime: String = mime.trim().chars().take(120).collect();
    let mut tx = c.transaction().map_err(hata)?;
    let eid: i64 = tx
        .query_one(
            "INSERT INTO gorev_ek (gorev_id, yukleyen_id, ad, mime, boyut, sha256) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
            &[&gorev, &k.id, &ad, &mime, &(veri.len() as i64), &sha],
        )
        .map_err(hata)?
        .get(0);
    olay_yaz(&mut tx, Some(gorev), Some(k.id), olay::EK, json!({"ek": eid, "ad": ad}))?;
    tx.commit().map_err(hata)?;
    Ok(eid)
}

/// Ekin adı, türü ve içeriği (işi görebilene).
pub fn ek_oku<C: GenericClient>(c: &mut C, k: &Kisi, id: i64) -> Result<(String, String, Vec<u8>), String> {
    let r = c
        .query_opt("SELECT gorev_id, ad, mime, sha256 FROM gorev_ek WHERE id = $1", &[&id])
        .map_err(hata)?
        .ok_or("Ek bulunamadı.")?;
    let oz = gorev_ozu(c, r.get(0))?.ok_or("Ek bulunamadı.")?;
    if !yetki(c, &oz, k)?.gorebilir {
        return Err("Ek bulunamadı.".into());
    }
    let sha: String = r.get(3);
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Ek kaydı bozuk.".into());
    }
    let veri = std::fs::read(crate::is_aktar::ekler_dizini().join(&sha))
        .map_err(|_| "Ekin dosyası ofis bilgisayarında bulunamadı.".to_string())?;
    Ok((r.get(1), r.get(2), veri))
}

/// Eki siler: yükleyen ya da işi yönetebilen. Başka kayıt aynı dosyayı kullanmıyorsa disktekini de siler.
pub fn ek_sil<C: GenericClient>(c: &mut C, k: &Kisi, id: i64) -> Result<(), String> {
    let r = c
        .query_opt("SELECT gorev_id, yukleyen_id, ad, sha256 FROM gorev_ek WHERE id = $1", &[&id])
        .map_err(hata)?
        .ok_or("Ek bulunamadı.")?;
    let gorev: i64 = r.get(0);
    let oz = gorev_ozu(c, gorev)?.ok_or("Ek bulunamadı.")?;
    let yt = yetki(c, &oz, k)?;
    if !yt.gorebilir {
        return Err("Ek bulunamadı.".into());
    }
    if !(yt.yonetebilir || r.get::<_, Option<i64>>(1) == Some(k.id)) {
        return Err("Bu eki yalnız yükleyen ya da işi yöneten silebilir.".into());
    }
    let sha: String = r.get(3);
    let mut tx = c.transaction().map_err(hata)?;
    tx.execute("DELETE FROM gorev_ek WHERE id = $1", &[&id]).map_err(hata)?;
    olay_yaz(&mut tx, Some(gorev), Some(k.id), olay::EK, json!({"silinen": id, "ad": r.get::<_, String>(2)}))?;
    // Aynı dosyayı başka ek ya da bir belge sürümü (belge_veri.rs, aynı depo) kullanıyorsa diskten silinmez.
    let baska: bool = tx.query_one("SELECT EXISTS (SELECT 1 FROM gorev_ek WHERE sha256 = $1)", &[&sha]).map_err(hata)?.get(0)
        || crate::belge_veri::sha_kullaniliyor(&mut tx, &sha)?;
    tx.commit().map_err(hata)?;
    if !baska && sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        let _ = std::fs::remove_file(crate::is_aktar::ekler_dizini().join(&sha));
    }
    Ok(())
}

pub fn not_ekle<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, metin: &str) -> Result<i64, String> {
    let metin = metin.trim();
    if metin.is_empty() {
        return Err("Not boş olamaz.".into());
    }
    let oz = gorev_ozu(c, id)?.ok_or("İş bulunamadı.")?;
    let yt = yetki(c, &oz, k)?;
    if !yt.gorebilir {
        return Err("İş bulunamadı.".into());
    }
    let mut tx = c.transaction().map_err(hata)?;
    let nid: i64 = tx
        .query_one("INSERT INTO gorev_not (gorev_id, yazan_id, metin) VALUES ($1, $2, $3) RETURNING id", &[&id, &k.id, &metin])
        .map_err(hata)?
        .get(0);
    olay_yaz(&mut tx, Some(id), Some(k.id), olay::NOT, json!({"not": nid}))?;
    tx.commit().map_err(hata)?;
    Ok(nid)
}

pub fn kisi_ekle<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, kullanici: i64, rol: &str) -> Result<(), String> {
    if !["sorumlu", "onaylayici", "izleyici"].contains(&rol) {
        return Err("Bilinmeyen rol.".into());
    }
    let oz = gorev_ozu(c, id)?.ok_or("İş bulunamadı.")?;
    if !yetki(c, &oz, k)?.yonetebilir {
        return Err("Kişi eklemeyi yalnız işi açan kişi ya da yönetici yapabilir.".into());
    }
    let mut tx = c.transaction().map_err(hata)?;
    let n = tx
        .execute(
            "INSERT INTO gorev_kisi (gorev_id, kullanici_id, rol, ekleyen_id) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            &[&id, &kullanici, &rol, &k.id],
        )
        .map_err(hata)?;
    if n > 0 {
        olay_yaz(&mut tx, Some(id), Some(k.id), olay::KISI_EKLENDI, json!({"kullanici": kullanici, "rol": rol}))?;
    }
    tx.commit().map_err(hata)
}

pub fn kisi_cikar<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, kullanici: i64, rol: &str) -> Result<(), String> {
    let oz = gorev_ozu(c, id)?.ok_or("İş bulunamadı.")?;
    if !yetki(c, &oz, k)?.yonetebilir {
        return Err("Kişi çıkarmayı yalnız işi açan kişi ya da yönetici yapabilir.".into());
    }
    let mut tx = c.transaction().map_err(hata)?;
    let n = tx
        .execute("DELETE FROM gorev_kisi WHERE gorev_id = $1 AND kullanici_id = $2 AND rol = $3", &[&id, &kullanici, &rol])
        .map_err(hata)?;
    if n > 0 {
        olay_yaz(&mut tx, Some(id), Some(k.id), olay::KISI_CIKTI, json!({"kullanici": kullanici, "rol": rol}))?;
    }
    tx.commit().map_err(hata)
}

// ── Okuma: liste, ayrıntı ───────────────────────────────────────────────────────────────────────

const GOREV_SEC: &str = "SELECT g.id, g.ust_gorev_id, g.baslik, g.aciklama, g.tur, g.durum, g.onay_gerekir, g.oncelik, g.gizli,
        g.sorumlu_id, su.ad, g.olusturan_id, ol.ad, g.son_tarih, g.son_saat, g.bitis_saati, g.yer, g.sonuc, g.sonuc_notu,
        g.dosya_id, g.taraf_id, g.evrak_id, g.tebligat_id, g.birim_adi, g.dosya_no, g.dosya_turu, g.taraf_adi, g.evrak_adi,
        g.barkod, g.uyap_karti, g.sablon_id, g.surec_asama_id, sa.ad, g.onceki_gorev_id, g.tekrar_id,
        g.onaya_sunulma::timestamp, g.tamamlanma::timestamp, g.olusturulma::timestamp, g.guncelleme::timestamp,
        (SELECT count(*) FROM gorev a WHERE a.ust_gorev_id = g.id),
        (SELECT count(*) FROM gorev a WHERE a.ust_gorev_id = g.id AND a.durum IN ('tamamlandi','iptal')),
        (SELECT count(*) FROM gorev_not n WHERE n.gorev_id = g.id),
        (SELECT coalesce(array_agg(ge.etiket_id ORDER BY ge.etiket_id), '{}') FROM gorev_etiket ge WHERE ge.gorev_id = g.id),
        EXISTS (SELECT 1 FROM gorev_erteleme e WHERE e.gorev_id = g.id AND e.durum = 'bekliyor')
    FROM gorev g
    LEFT JOIN kullanici su ON su.id = g.sorumlu_id
    LEFT JOIN kullanici ol ON ol.id = g.olusturan_id
    LEFT JOIN surec_asama sa ON sa.id = g.surec_asama_id";

/// Görünürlük koşulu ($1 = kişi kimliği).
const GORUNUR: &str = "(NOT g.gizli OR g.olusturan_id = $1 OR g.sorumlu_id = $1)";

fn ts(v: Option<chrono::NaiveDateTime>) -> Value {
    v.map(|t| json!(t.format("%Y-%m-%d %H:%M").to_string())).unwrap_or(Value::Null)
}

fn gorev_json(r: &postgres::Row, bugun: NaiveDate) -> Value {
    let son_tarih: NaiveDate = r.get(13);
    let durum: String = r.get(5);
    let acik = ACIK_DURUMLAR.contains(&durum.as_str());
    // json! tek parça çok uzun olunca makro özyineleme sınırına takılıyor: iki parçada kurulur.
    let mut v = json!({
        "id": r.get::<_, i64>(0),
        "ust_gorev_id": r.get::<_, Option<i64>>(1),
        "baslik": r.get::<_, String>(2),
        "aciklama": r.get::<_, String>(3),
        "tur": r.get::<_, String>(4),
        "durum": durum,
        "durum_adi": etiket(DURUMLAR, &durum),
        "onay_gerekir": r.get::<_, bool>(6),
        "oncelik": r.get::<_, i16>(7),
        "gizli": r.get::<_, bool>(8),
        "sorumlu_id": r.get::<_, Option<i64>>(9),
        "sorumlu_adi": r.get::<_, Option<String>>(10),
        "olusturan_id": r.get::<_, Option<i64>>(11),
        "olusturan_adi": r.get::<_, Option<String>>(12),
        "son_tarih": son_tarih.to_string(),
        "son_saat": r.get::<_, Option<NaiveTime>>(14).map(|s| s.format("%H:%M").to_string()),
        "bitis_saati": r.get::<_, Option<NaiveTime>>(15).map(|s| s.format("%H:%M").to_string()),
        "yer": r.get::<_, String>(16),
        "sonuc": r.get::<_, String>(17),
        "sonuc_notu": r.get::<_, String>(18),
        "dosya_id": r.get::<_, Option<i64>>(19),
        "taraf_id": r.get::<_, Option<i64>>(20),
        "evrak_id": r.get::<_, Option<i64>>(21),
        "tebligat_id": r.get::<_, Option<i64>>(22),
        "birim_adi": r.get::<_, String>(23),
        "dosya_no": r.get::<_, String>(24),
        "dosya_turu": r.get::<_, String>(25),
        "taraf_adi": r.get::<_, String>(26),
        "evrak_adi": r.get::<_, String>(27),
        "barkod": r.get::<_, String>(28),
    });
    let ek = json!({
        "uyap_karti": r.get::<_, Option<Value>>(29),
        "sablon_id": r.get::<_, Option<i64>>(30),
        "surec_asama_id": r.get::<_, Option<i64>>(31),
        "surec_asama_adi": r.get::<_, Option<String>>(32),
        "onceki_gorev_id": r.get::<_, Option<i64>>(33),
        "tekrar_id": r.get::<_, Option<i64>>(34),
        "onaya_sunulma": ts(r.get(35)),
        "tamamlanma": ts(r.get(36)),
        "olusturulma": ts(r.get(37)),
        "guncelleme": ts(r.get(38)),
        "alt_sayisi": r.get::<_, i64>(39),
        "alt_biten": r.get::<_, i64>(40),
        "not_sayisi": r.get::<_, i64>(41),
        "etiketler": r.get::<_, Vec<i64>>(42),
        "erteleme_bekliyor": r.get::<_, bool>(43),
        "gecikti": acik && son_tarih < bugun,
        "gecikme_gun": if acik && son_tarih < bugun { (bugun - son_tarih).num_days() } else { 0 },
    });
    if let (Some(o), Value::Object(e)) = (v.as_object_mut(), ek) {
        o.extend(e);
    }
    v
}

/// Liste süzgeci. Boş alan süzmez.
#[derive(Clone, Debug, Default)]
pub struct Suzgec {
    pub gorunum: String, // "bugun" | "onay" | "tum" | "dosya" | "takvim"
    pub sorumlu_id: Option<i64>,
    pub durumlar: Vec<String>,
    pub tur: String,
    pub ara: String,
    pub dosya_id: Option<i64>,
    pub dosya_no: String,
    pub birim_adi: String,
    pub etiket_id: Option<i64>,
    pub ilk: Option<NaiveDate>,
    pub son: Option<NaiveDate>,
    pub sinir: i64,
}

/// İş listesi. "bugun": kişinin bugün ve öncesine düşen bütün açık işleri (yalnız dün değil, B-17);
/// "onay": kişinin onaylayabileceği onayda işler.
pub fn gorevler<C: GenericClient>(c: &mut C, k: &Kisi, s: &Suzgec, bugun: NaiveDate) -> Result<Vec<Value>, String> {
    let mut kosul = vec![GORUNUR.to_string(), "g.ust_gorev_id IS NULL".to_string()];
    let mut p: Vec<Box<dyn ToSql + Sync>> = vec![Box::new(k.id)];
    let ekle = |v: Box<dyn ToSql + Sync>, p: &mut Vec<Box<dyn ToSql + Sync>>| -> String {
        p.push(v);
        format!("${}", p.len())
    };
    match s.gorunum.as_str() {
        "bugun" => {
            let b = ekle(Box::new(bugun), &mut p);
            kosul.push(format!("g.durum IN ('acik','devam','onayda') AND g.son_tarih <= {b}"));
            kosul.push("(g.sorumlu_id = $1 OR EXISTS (SELECT 1 FROM gorev_kisi gk WHERE gk.gorev_id = g.id AND gk.kullanici_id = $1 AND gk.rol = 'sorumlu'))".into());
        }
        "onay" => {
            kosul.push("g.durum = 'onayda'".into());
            if !k.yonetici {
                kosul.push("(EXISTS (SELECT 1 FROM gorev_kisi gk WHERE gk.gorev_id = g.id AND gk.kullanici_id = $1 AND gk.rol = 'onaylayici')
                             OR (g.olusturan_id = $1 AND g.sorumlu_id IS DISTINCT FROM $1))".into());
            }
        }
        _ => {}
    }
    if let Some(u) = s.sorumlu_id {
        let x = ekle(Box::new(u), &mut p);
        kosul.push(format!("g.sorumlu_id = {x}"));
    }
    if !s.durumlar.is_empty() {
        let x = ekle(Box::new(s.durumlar.clone()), &mut p);
        kosul.push(format!("g.durum = ANY({x})"));
    }
    if !s.tur.is_empty() {
        let x = ekle(Box::new(s.tur.clone()), &mut p);
        kosul.push(format!("g.tur = {x}"));
    }
    for w in arama_sozcukleri(&s.ara) {
        let x = ekle(Box::new(format!("%{w}%")), &mut p);
        kosul.push(format!("g.arama_metni LIKE {x}"));
    }
    if let Some(d) = s.dosya_id {
        let x = ekle(Box::new(d), &mut p);
        kosul.push(format!("g.dosya_id = {x}"));
    }
    if !s.dosya_no.is_empty() {
        let x = ekle(Box::new(s.dosya_no.clone()), &mut p);
        kosul.push(format!("g.dosya_no = {x}"));
        if !s.birim_adi.is_empty() {
            let y = ekle(Box::new(s.birim_adi.clone()), &mut p);
            kosul.push(format!("g.birim_adi = {y}"));
        }
    }
    if let Some(e) = s.etiket_id {
        let x = ekle(Box::new(e), &mut p);
        kosul.push(format!("EXISTS (SELECT 1 FROM gorev_etiket ge WHERE ge.gorev_id = g.id AND ge.etiket_id = {x})"));
    }
    if let Some(t) = s.ilk {
        let x = ekle(Box::new(t), &mut p);
        kosul.push(format!("g.son_tarih >= {x}"));
    }
    if let Some(t) = s.son {
        let x = ekle(Box::new(t), &mut p);
        kosul.push(format!("g.son_tarih <= {x}"));
    }
    let sinir = if s.sinir <= 0 { 300 } else { s.sinir.min(1000) };
    let sql = format!(
        "{GOREV_SEC} WHERE {} ORDER BY g.son_tarih, g.son_saat NULLS LAST, g.oncelik DESC, g.id LIMIT {sinir}",
        kosul.join(" AND ")
    );
    let refs: Vec<&(dyn ToSql + Sync)> = p.iter().map(|b| b.as_ref()).collect();
    let mut liste: Vec<Value> = c.query(&sql, &refs).map_err(hata)?.iter().map(|r| gorev_json(r, bugun)).collect();
    hatirlatma_ekle(c, k, &mut liste)?;
    Ok(liste)
}

/// İşin bütün ayrıntısı: alt işler, notlar, kişiler, ekler, ertelemeler, geçmiş, yetki.
pub fn gorev_ayrinti<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, bugun: NaiveDate) -> Result<Option<Value>, String> {
    let Some(oz) = gorev_ozu(c, id)? else { return Ok(None) };
    let yt = yetki(c, &oz, k)?;
    if !yt.gorebilir {
        return Ok(None);
    }
    let r = c.query_one(&format!("{GOREV_SEC} WHERE g.id = $1"), &[&id]).map_err(hata)?;
    let mut v = gorev_json(&r, bugun);
    let altlar: Vec<Value> = c
        .query(&format!("{GOREV_SEC} WHERE g.ust_gorev_id = $1 ORDER BY g.sira, g.id"), &[&id])
        .map_err(hata)?
        .iter()
        .map(|r| gorev_json(r, bugun))
        .collect();
    let notlar: Vec<Value> = c
        .query(
            "SELECT n.id, n.metin, k.ad, n.olusturulma::timestamp FROM gorev_not n LEFT JOIN kullanici k ON k.id = n.yazan_id
             WHERE n.gorev_id = $1 ORDER BY n.id",
            &[&id],
        )
        .map_err(hata)?
        .iter()
        .map(|r| json!({"id": r.get::<_, i64>(0), "metin": r.get::<_, String>(1), "yazan": r.get::<_, Option<String>>(2), "zaman": ts(r.get(3))}))
        .collect();
    let kisiler: Vec<Value> = c
        .query(
            "SELECT gk.kullanici_id, k.ad, gk.rol FROM gorev_kisi gk JOIN kullanici k ON k.id = gk.kullanici_id WHERE gk.gorev_id = $1 ORDER BY gk.rol, k.ad",
            &[&id],
        )
        .map_err(hata)?
        .iter()
        .map(|r| json!({"id": r.get::<_, i64>(0), "ad": r.get::<_, String>(1), "rol": r.get::<_, String>(2)}))
        .collect();
    let ekler: Vec<Value> = c
        .query(
            "SELECT e.id, e.ad, e.mime, e.boyut, e.olusturulma::timestamp, e.yukleyen_id, k.ad FROM gorev_ek e
               LEFT JOIN kullanici k ON k.id = e.yukleyen_id WHERE e.gorev_id = $1 ORDER BY e.id",
            &[&id],
        )
        .map_err(hata)?
        .iter()
        .map(|r| json!({"id": r.get::<_, i64>(0), "ad": r.get::<_, String>(1), "mime": r.get::<_, String>(2), "boyut": r.get::<_, i64>(3),
                        "zaman": ts(r.get(4)), "yukleyen_id": r.get::<_, Option<i64>>(5), "yukleyen": r.get::<_, Option<String>>(6)}))
        .collect();
    let ertelemeler: Vec<Value> = c
        .query(
            "SELECT e.id, e.eski_tarih, e.yeni_tarih, e.yeni_saat, e.gerekce, e.durum, k.ad, e.olusturulma::timestamp, e.karar_notu
             FROM gorev_erteleme e LEFT JOIN kullanici k ON k.id = e.isteyen_id WHERE e.gorev_id = $1 ORDER BY e.id DESC",
            &[&id],
        )
        .map_err(hata)?
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<_, i64>(0), "eski_tarih": r.get::<_, NaiveDate>(1).to_string(), "yeni_tarih": r.get::<_, NaiveDate>(2).to_string(),
                "yeni_saat": r.get::<_, Option<NaiveTime>>(3).map(|s| s.format("%H:%M").to_string()),
                "gerekce": r.get::<_, String>(4), "durum": r.get::<_, String>(5), "isteyen": r.get::<_, Option<String>>(6),
                "zaman": ts(r.get(7)), "karar_notu": r.get::<_, String>(8),
            })
        })
        .collect();
    let gecmis: Vec<Value> = c
        .query(
            "SELECT o.tur, o.ayrinti, k.ad, o.olusturulma::timestamp FROM gorev_olay o LEFT JOIN kullanici k ON k.id = o.kullanici_id
             WHERE o.gorev_id = $1 ORDER BY o.id DESC LIMIT 200",
            &[&id],
        )
        .map_err(hata)?
        .iter()
        .map(|r| json!({"tur": r.get::<_, String>(0), "ayrinti": r.get::<_, Value>(1), "kisi": r.get::<_, Option<String>>(2), "zaman": ts(r.get(3))}))
        .collect();
    // Kapatma düğmeleri: şablonun sonuç dalları (adlarıyla).
    let sonuclar: Vec<Value> = match oz.sablon_id {
        Some(sid) => c
            .query(
                "SELECT ss.sonuc, ss.sonuc_adi, coalesce(h.baslik, '') FROM sablon_sonraki ss
                 LEFT JOIN sablon h ON h.id = ss.sonraki_sablon_id WHERE ss.sablon_id = $1",
                &[&sid],
            )
            .map_err(hata)?
            .iter()
            .map(|r| json!({"sonuc": r.get::<_, String>(0), "ad": r.get::<_, String>(1), "sonraki": r.get::<_, String>(2)}))
            .collect(),
        None => vec![],
    };
    let kontrol: Option<String> = match oz.sablon_id {
        Some(sid) => c.query_opt("SELECT kontrol FROM sablon WHERE id = $1", &[&sid]).map_err(hata)?.map(|r| r.get(0)),
        None => None,
    };
    v["altlar"] = json!(altlar);
    v["notlar"] = json!(notlar);
    v["kisiler"] = json!(kisiler);
    v["ekler"] = json!(ekler);
    v["belgeler"] = json!(crate::belge_veri::gorev_belgeleri(c, id)?);
    v["ertelemeler"] = json!(ertelemeler);
    v["gecmis"] = json!(gecmis);
    v["sonuclar"] = json!(sonuclar);
    v["kontrol"] = json!(kontrol.unwrap_or_default());
    v["yetki"] = json!({"calisabilir": yt.calisabilir, "onaylayabilir": yt.onaylayabilir, "yonetebilir": yt.yonetebilir});
    v["hatirlaticilar"] = json!(hatirlaticilar(c, k, Some(id))?);
    hatirlatma_ekle(c, k, std::slice::from_mut(&mut v))?;
    // Kişi işi gördü
    let son_olay: i64 = c
        .query_one("SELECT coalesce(max(id), 0) FROM gorev_olay WHERE gorev_id = $1", &[&id])
        .map_err(hata)?
        .get(0);
    c.execute(
        "INSERT INTO gorev_goruldu (kullanici_id, gorev_id, son_olay_id) VALUES ($1, $2, $3)
         ON CONFLICT (kullanici_id, gorev_id) DO UPDATE SET son_olay_id = EXCLUDED.son_olay_id",
        &[&k.id, &id, &son_olay],
    )
    .map_err(hata)?;
    Ok(Some(v))
}

/// Sayaçlar: bugün (gecikenler dahil), geciken, onay bekleyen, erteleme kararı bekleyen.
pub fn ozet<C: GenericClient>(c: &mut C, k: &Kisi, bugun: NaiveDate) -> Result<Value, String> {
    let r = c
        .query_one(
            "SELECT
               count(*) FILTER (WHERE g.son_tarih <= $2 AND (g.sorumlu_id = $1)),
               count(*) FILTER (WHERE g.son_tarih <  $2 AND (g.sorumlu_id = $1)),
               count(*) FILTER (WHERE g.durum = 'onayda' AND ($3 OR g.olusturan_id = $1 AND g.sorumlu_id IS DISTINCT FROM $1
                                OR EXISTS (SELECT 1 FROM gorev_kisi gk WHERE gk.gorev_id = g.id AND gk.kullanici_id = $1 AND gk.rol = 'onaylayici')))
             FROM gorev g WHERE g.ust_gorev_id IS NULL AND g.durum IN ('acik','devam','onayda')
               AND (NOT g.gizli OR g.olusturan_id = $1 OR g.sorumlu_id = $1)",
            &[&k.id, &bugun, &k.yonetici],
        )
        .map_err(hata)?;
    let erteleme: i64 = c
        .query_one(
            "SELECT count(*) FROM gorev_erteleme e JOIN gorev g ON g.id = e.gorev_id WHERE e.durum = 'bekliyor'
               AND ($2 OR g.olusturan_id = $1 AND g.sorumlu_id IS DISTINCT FROM $1
                    OR EXISTS (SELECT 1 FROM gorev_kisi gk WHERE gk.gorev_id = g.id AND gk.kullanici_id = $1 AND gk.rol = 'onaylayici'))",
            &[&k.id, &k.yonetici],
        )
        .map_err(hata)?
        .get(0);
    let (yeni, yeni_isler) = yeni_isler(c, k)?;
    let (hatirlatma_sayi, hatirlatma) = vadesi_gelen_hatirlaticilar(c, k)?;
    Ok(json!({"bugun": r.get::<_, i64>(0), "geciken": r.get::<_, i64>(1), "onay": r.get::<_, i64>(2), "erteleme": erteleme,
              "yeni": yeni, "yeni_isler": yeni_isler, "hatirlatma_sayi": hatirlatma_sayi, "hatirlatma": hatirlatma}))
}

/// Başkasının kişiye açtığı ya da kişiye devrettiği, kişinin henüz açıp bakmadığı işler (son 7 gün).
/// Sistemin açtığı işler (rutin örneği vb., olayda kişi yok) sayılmaz. `olay`: uyarının tekrarlanmaması
/// için sayfanın hatırladığı anahtar (aynı iş yeniden devredilirse yeni olay = yeni uyarı).
fn yeni_isler<C: GenericClient>(c: &mut C, k: &Kisi) -> Result<(i64, Vec<Value>), String> {
    let satirlar = c
        .query(
            "SELECT g.id, g.baslik, coalesce(u.ad, ''), o.id, count(*) OVER ()
             FROM gorev g
             JOIN LATERAL (SELECT oo.id, oo.kullanici_id, oo.olusturulma FROM gorev_olay oo
                           WHERE oo.gorev_id = g.id AND oo.tur IN ('acildi', 'sorumlu') ORDER BY oo.id DESC LIMIT 1) o ON true
             LEFT JOIN kullanici u ON u.id = o.kullanici_id
             LEFT JOIN gorev_goruldu gg ON gg.gorev_id = g.id AND gg.kullanici_id = $1
             WHERE g.ust_gorev_id IS NULL AND g.durum IN ('acik', 'devam') AND g.sorumlu_id = $1
               AND o.kullanici_id IS NOT NULL AND o.kullanici_id <> $1
               AND o.id > coalesce(gg.son_olay_id, 0) AND o.olusturulma > now() - interval '7 days'
             ORDER BY o.id DESC LIMIT 5",
            &[&k.id],
        )
        .map_err(hata)?;
    let toplam = satirlar.first().map(|r| r.get::<_, i64>(4)).unwrap_or(0);
    let liste = satirlar
        .iter()
        .map(|r| json!({"id": r.get::<_, i64>(0), "baslik": r.get::<_, String>(1), "atayan": r.get::<_, String>(2), "olay": r.get::<_, i64>(3)}))
        .collect();
    Ok((toplam, liste))
}

// ── Hatırlatıcı: kişi kendine kurar (plan L6). Sayfa uyarısı `goruldu`, telefon sinyali `gonderildi` ile susar. ──

const HATIRLATICI_SEC: &str = "SELECT h.id, h.gorev_id, h.zaman::timestamp, h.baslik, h.notu, coalesce(g.baslik, h.baslik)
    FROM hatirlatici h LEFT JOIN gorev g ON g.id = h.gorev_id";

fn hatirlatici_json(r: &postgres::Row) -> Value {
    json!({"id": r.get::<_, i64>(0), "gorev_id": r.get::<_, Option<i64>>(1), "zaman": ts(r.get(2)),
           "baslik": r.get::<_, String>(5), "notu": r.get::<_, String>(4)})
}

/// Zamanı gelmiş, kişinin henüz "Tamam" demediği hatırlatıcılar (kapanmış işlerinkiler hariç), en çok 5.
fn vadesi_gelen_hatirlaticilar<C: GenericClient>(c: &mut C, k: &Kisi) -> Result<(i64, Vec<Value>), String> {
    let kosul = "WHERE h.kullanici_id = $1 AND h.goruldu IS NULL AND h.zaman <= now()
                   AND (h.gorev_id IS NULL OR g.durum IN ('acik', 'devam', 'onayda'))";
    let toplam: i64 = c
        .query_one(&format!("SELECT count(*) FROM hatirlatici h LEFT JOIN gorev g ON g.id = h.gorev_id {kosul}"), &[&k.id])
        .map_err(hata)?
        .get(0);
    if toplam == 0 {
        return Ok((0, vec![]));
    }
    let liste = c
        .query(&format!("{HATIRLATICI_SEC} {kosul} ORDER BY h.zaman LIMIT 5"), &[&k.id])
        .map_err(hata)?
        .iter()
        .map(hatirlatici_json)
        .collect();
    Ok((toplam, liste))
}

/// Kişinin bekleyen (görmediği) hatırlatıcıları; `gorev` verilirse yalnız o işin.
pub fn hatirlaticilar<C: GenericClient>(c: &mut C, k: &Kisi, gorev: Option<i64>) -> Result<Vec<Value>, String> {
    Ok(c
        .query(
            &format!("{HATIRLATICI_SEC} WHERE h.kullanici_id = $1 AND h.goruldu IS NULL AND ($2::bigint IS NULL OR h.gorev_id = $2) ORDER BY h.zaman LIMIT 100"),
            &[&k.id, &gorev],
        )
        .map_err(hata)?
        .iter()
        .map(hatirlatici_json)
        .collect())
}

/// Kişi kendine hatırlatıcı kurar. İşe bağlıysa işi görebilmeli; zaman geçmişte olamaz.
pub fn hatirlatici_kur<C: GenericClient>(
    c: &mut C,
    k: &Kisi,
    gorev: Option<i64>,
    zaman: chrono::NaiveDateTime,
    notu: &str,
    simdi: chrono::NaiveDateTime,
) -> Result<i64, String> {
    if zaman < simdi - Duration::minutes(1) {
        return Err("Geçmiş bir zamana hatırlatıcı kurulamaz.".into());
    }
    if zaman > simdi + Duration::days(800) {
        return Err("Hatırlatma zamanı çok ileri.".into());
    }
    let notu: String = notu.trim().chars().take(500).collect();
    let baslik: String = match gorev {
        Some(id) => {
            let oz = gorev_ozu(c, id)?.ok_or("İş bulunamadı.")?;
            if !yetki(c, &oz, k)?.gorebilir {
                return Err("İş bulunamadı.".into());
            }
            let b: String = c.query_one("SELECT baslik FROM gorev WHERE id = $1", &[&id]).map_err(hata)?.get(0);
            b.chars().take(300).collect()
        }
        None if notu.is_empty() => return Err("Hatırlatma için bir not yazın.".into()),
        None => notu.chars().take(300).collect(),
    };
    let bekleyen: i64 = c
        .query_one("SELECT count(*) FROM hatirlatici WHERE kullanici_id = $1 AND goruldu IS NULL", &[&k.id])
        .map_err(hata)?
        .get(0);
    if bekleyen >= 500 {
        return Err("Bekleyen hatırlatıcınız çok fazla; eskilerini silin.".into());
    }
    c.query_one(
        "INSERT INTO hatirlatici (gorev_id, kullanici_id, olusturan_id, zaman, baslik, notu)
         VALUES ($1, $2, $2, $3::timestamp, $4, $5) RETURNING id",
        &[&gorev, &k.id, &zaman, &baslik, &notu],
    )
    .map(|r| r.get(0))
    .map_err(hata)
}

/// Onayda bekleyen iş için onaylayana hatırlatma: onaylayıcıların (yoksa işi başkasına açanın, o da yoksa
/// yöneticilerin) Çoklu UYAP'ında hemen uyarı çıkar (hatırlatıcı, zaman = şimdi; telefon sinyali de aynı yoldan).
/// Yalnız işte çalışabilen (sunan / sorumlu / yönetici) gönderir; aynı kişiye aynı iş için saatte bir.
pub fn onay_hatirlat<C: GenericClient>(c: &mut C, k: &Kisi, gorev: i64, notu: &str) -> Result<Vec<String>, String> {
    let oz = gorev_ozu(c, gorev)?.ok_or("İş bulunamadı.")?;
    let y = yetki(c, &oz, k)?;
    if !y.gorebilir {
        return Err("İş bulunamadı.".into());
    }
    if oz.durum != "onayda" {
        return Err("Bu iş onay beklemiyor.".into());
    }
    if !y.calisabilir {
        return Err("Hatırlatmayı işin sorumlusu ya da onaya sunan gönderebilir.".into());
    }
    let mut alicilar: Vec<i64> = c
        .query(
            "SELECT DISTINCT gk.kullanici_id FROM gorev_kisi gk JOIN kullanici u ON u.id = gk.kullanici_id
             WHERE gk.gorev_id = $1 AND gk.rol = 'onaylayici' AND u.aktif",
            &[&gorev],
        )
        .map_err(hata)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    if alicilar.is_empty() {
        if let Some(o) = oz.olusturan_id.filter(|o| oz.sorumlu_id != Some(*o)) {
            alicilar.push(o);
        }
    }
    if alicilar.is_empty() {
        alicilar = c
            .query("SELECT id FROM kullanici WHERE rol = 'yonetici' AND aktif", &[])
            .map_err(hata)?
            .iter()
            .map(|r| r.get(0))
            .collect();
    }
    alicilar.retain(|a| *a != k.id);
    if alicilar.is_empty() {
        return Err("Hatırlatılacak onaylayıcı yok (onaylayıcı siz misiniz?).".into());
    }
    let ben: String = c.query_one("SELECT ad FROM kullanici WHERE id = $1", &[&k.id]).map_err(hata)?.get(0);
    let baslik: String = c.query_one("SELECT baslik FROM gorev WHERE id = $1", &[&gorev]).map_err(hata)?.get(0);
    let ek: String = notu.trim().chars().take(400).collect();
    let metin = format!("{ben} onayınızı bekliyor.{}", if ek.is_empty() { String::new() } else { format!(" Not: {ek}") });
    let mut tx = c.transaction().map_err(hata)?;
    let mut adlar = Vec::new();
    for a in alicilar {
        let yakin: i64 = tx
            .query_one(
                "SELECT count(*) FROM hatirlatici WHERE gorev_id = $1 AND kullanici_id = $2 AND olusturan_id = $3
                 AND olusturulma > now() - interval '1 hour'",
                &[&gorev, &a, &k.id],
            )
            .map_err(hata)?
            .get(0);
        let ad: String = tx.query_one("SELECT ad FROM kullanici WHERE id = $1", &[&a]).map_err(hata)?.get(0);
        if yakin > 0 {
            continue;
        }
        tx.execute(
            "INSERT INTO hatirlatici (gorev_id, kullanici_id, olusturan_id, zaman, baslik, notu) VALUES ($1, $2, $3, now(), $4, $5)",
            &[&gorev, &a, &k.id, &baslik.chars().take(300).collect::<String>(), &metin],
        )
        .map_err(hata)?;
        adlar.push(ad);
    }
    if adlar.is_empty() {
        return Err("Son bir saat içinde zaten hatırlattınız.".into());
    }
    olay_yaz(&mut tx, Some(gorev), Some(k.id), olay::HATIRLATILDI, json!({"kime": adlar, "notu": ek}))?;
    tx.commit().map_err(hata)?;
    Ok(adlar)
}

pub fn hatirlatici_sil<C: GenericClient>(c: &mut C, k: &Kisi, id: i64) -> Result<(), String> {
    match c.execute("DELETE FROM hatirlatici WHERE id = $1 AND kullanici_id = $2", &[&id, &k.id]).map_err(hata)? {
        0 => Err("Hatırlatıcı bulunamadı.".into()),
        _ => Ok(()),
    }
}

/// Sayfadaki uyarıda "Tamam".
pub fn hatirlatici_goruldu<C: GenericClient>(c: &mut C, k: &Kisi, id: i64) -> Result<(), String> {
    c.execute("UPDATE hatirlatici SET goruldu = now() WHERE id = $1 AND kullanici_id = $2 AND goruldu IS NULL", &[&id, &k.id])
        .map_err(hata)?;
    Ok(())
}

/// Uyarıda "Sonra hatırlat": `dakika` sonra yeniden (5 dk – 7 gün).
pub fn hatirlatici_ertele<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, dakika: i64) -> Result<(), String> {
    let dakika = dakika.clamp(5, 7 * 24 * 60) as i32;
    match c
        .execute(
            "UPDATE hatirlatici SET zaman = now() + make_interval(mins => $3), goruldu = NULL, gonderildi = NULL
             WHERE id = $1 AND kullanici_id = $2",
            &[&id, &k.id, &dakika],
        )
        .map_err(hata)?
    {
        0 => Err("Hatırlatıcı bulunamadı.".into()),
        _ => Ok(()),
    }
}

/// İş satırlarına kişinin sıradaki hatırlatma zamanını ekler (`hatirlatma`: "YYYY-AA-GG SS:DD" ya da null).
pub fn hatirlatma_ekle<C: GenericClient>(c: &mut C, k: &Kisi, isler: &mut [Value]) -> Result<(), String> {
    let idler: Vec<i64> = isler.iter().filter_map(|g| g["id"].as_i64()).collect();
    if idler.is_empty() {
        return Ok(());
    }
    let m: HashMap<i64, Value> = c
        .query(
            "SELECT gorev_id, min(zaman)::timestamp FROM hatirlatici
             WHERE kullanici_id = $1 AND goruldu IS NULL AND gorev_id = ANY($2) GROUP BY gorev_id",
            &[&k.id, &idler],
        )
        .map_err(hata)?
        .iter()
        .map(|r| (r.get::<_, i64>(0), ts(r.get(1))))
        .collect();
    for g in isler.iter_mut() {
        let v = g["id"].as_i64().and_then(|id| m.get(&id).cloned()).unwrap_or(Value::Null);
        g["hatirlatma"] = v;
    }
    Ok(())
}

// ── Tekrar eden işler ───────────────────────────────────────────────────────────────────────────

fn kural_oku(r: &postgres::Row, o: usize) -> TekrarKurali {
    TekrarKurali {
        kural: r.get(o),
        haftanin_gunleri: r.get(o + 1),
        ayin_gunu: r.get(o + 2),
        baslangic: r.get(o + 3),
        bitis: r.get(o + 4),
    }
}

pub fn tekrar_kurali_ekle<C: GenericClient>(c: &mut C, k: &Kisi, g: &Value) -> Result<i64, String> {
    let m = |x: &str| g.get(x).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let baslik = m("baslik");
    if baslik.is_empty() {
        return Err("Rutinin başlığı boş olamaz.".into());
    }
    let kural = m("kural");
    if !gecerli(TEKRAR_KURALLARI, &kural) {
        return Err("Bilinmeyen tekrar kuralı.".into());
    }
    let gunler: Vec<i16> = g
        .get("haftanin_gunleri")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_i64()).filter(|n| (1..=7).contains(n)).map(|n| n as i16).collect())
        .unwrap_or_default();
    if kural == "haftalik" && gunler.is_empty() {
        return Err("Haftanın en az bir günü seçilmeli.".into());
    }
    let ayin_gunu = g.get("ayin_gunu").and_then(|v| v.as_i64()).map(|n| n.clamp(1, 31) as i16);
    if kural == "aylik" && ayin_gunu.is_none() {
        return Err("Ayın günü seçilmeli.".into());
    }
    let sorumlu = g.get("sorumlu_id").and_then(|v| v.as_i64()).unwrap_or(k.id);
    if sorumlu != k.id && !k.yonetici {
        return Err("Başkasına rutin yalnız yönetici tanımlayabilir.".into());
    }
    let baslangic = tarih_coz(&m("baslangic"))?;
    let bitis = tarih_coz(&m("bitis"))?;
    let saat = saat_coz(&m("saat"))?;
    let gizli = g.get("gizli").and_then(|v| v.as_bool()).unwrap_or(false);
    let sablon = g.get("sablon_id").and_then(|v| v.as_i64());
    let dosya = g.get("dosya_id").and_then(|v| v.as_i64());
    c.query_one(
        "INSERT INTO tekrar_kurali (baslik, aciklama, sablon_id, sorumlu_id, olusturan_id, kural, haftanin_gunleri, ayin_gunu,
             saat, baslangic, bitis, gizli, dosya_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, coalesce($10, current_date), $11, $12, $13) RETURNING id",
        &[&baslik, &m("aciklama"), &sablon, &sorumlu, &k.id, &kural, &gunler, &ayin_gunu, &saat, &baslangic, &bitis, &gizli, &dosya],
    )
    .map(|r| r.get(0))
    .map_err(hata)
}

pub fn tekrar_kurallari<C: GenericClient>(c: &mut C, k: &Kisi) -> Result<Vec<Value>, String> {
    Ok(c
        .query(
            "SELECT t.id, t.baslik, t.aciklama, t.kural, t.haftanin_gunleri, t.ayin_gunu, t.baslangic, t.bitis, t.saat,
                    t.sorumlu_id, k.ad, t.aktif, t.gizli
             FROM tekrar_kurali t JOIN kullanici k ON k.id = t.sorumlu_id
             WHERE NOT t.gizli OR t.sorumlu_id = $1 OR t.olusturan_id = $1 ORDER BY t.aktif DESC, t.baslik",
            &[&k.id],
        )
        .map_err(hata)?
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<_, i64>(0), "baslik": r.get::<_, String>(1), "aciklama": r.get::<_, String>(2),
                "kural": r.get::<_, String>(3), "kural_adi": etiket(TEKRAR_KURALLARI, &r.get::<_, String>(3)),
                "haftanin_gunleri": r.get::<_, Vec<i16>>(4), "ayin_gunu": r.get::<_, Option<i16>>(5),
                "baslangic": r.get::<_, NaiveDate>(6).to_string(), "bitis": r.get::<_, Option<NaiveDate>>(7).map(|t| t.to_string()),
                "saat": r.get::<_, Option<NaiveTime>>(8).map(|s| s.format("%H:%M").to_string()),
                "sorumlu_id": r.get::<_, i64>(9), "sorumlu_adi": r.get::<_, String>(10),
                "aktif": r.get::<_, bool>(11), "gizli": r.get::<_, bool>(12),
            })
        })
        .collect())
}

pub fn tekrar_kurali_durdur<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, aktif: bool) -> Result<(), String> {
    let n = c
        .execute(
            "UPDATE tekrar_kurali SET aktif = $2 WHERE id = $1 AND ($3 OR sorumlu_id = $4 OR olusturan_id = $4)",
            &[&id, &aktif, &k.yonetici, &k.id],
        )
        .map_err(hata)?;
    if n == 0 {
        return Err("Rutin bulunamadı ya da yetkiniz yok.".into());
    }
    Ok(())
}

/// Bugüne düşen tekrar örneklerini açar. Açık örneği olan kural atlanır (bitmeyen iş birikmez);
/// kaçırılan günler geriye dönük açılmaz. Açılan iş sayısını döner.
pub fn tekrarlari_uret<C: GenericClient>(c: &mut C, bugun: NaiveDate) -> Result<usize, String> {
    let kurallar = c
        .query(
            "SELECT t.id, t.baslik, t.aciklama, t.sablon_id, t.sorumlu_id, t.olusturan_id, t.saat, t.gizli, t.dosya_id,
                    (SELECT max(g.son_tarih) FROM gorev g WHERE g.tekrar_id = t.id),
                    t.kural, t.haftanin_gunleri, t.ayin_gunu, t.baslangic, t.bitis
             FROM tekrar_kurali t
             WHERE t.aktif AND t.baslangic <= $1 AND (t.bitis IS NULL OR t.bitis >= $1)
               AND NOT EXISTS (SELECT 1 FROM gorev g WHERE g.tekrar_id = t.id AND g.durum IN ('acik','devam','onayda'))",
            &[&bugun],
        )
        .map_err(hata)?;
    if kurallar.is_empty() {
        return Ok(0);
    }
    let tatil = resmi_tatiller(c)?;
    let mut n = 0;
    for r in &kurallar {
        let kural = kural_oku(r, 10);
        let son: Option<NaiveDate> = r.get(9);
        let itibaren = son.map(|s| s + Duration::days(1)).unwrap_or(kural.baslangic).max(bugun);
        let Some(gun) = tekrar_tarihi(&kural, itibaren, &tatil) else { continue };
        if gun > bugun {
            continue;
        }
        let y = YeniGorev {
            baslik: r.get(1),
            aciklama: r.get(2),
            tur: "rutin".into(),
            sablon_id: r.get(3),
            sorumlu_id: Some(r.get(4)),
            son_tarih: Some(gun),
            son_saat: r.get(6),
            gizli: r.get(7),
            dosya_id: r.get(8),
            tekrar_id: Some(r.get(0)),
            ..Default::default()
        };
        let olusturan: Option<i64> = r.get(5);
        let acan = olusturan.map(|id| Kisi { id, yonetici: false });
        match gorev_ac(c, acan.as_ref(), &y, bugun) {
            Ok(_) => n += 1,
            // Aynı anda başka bir istek açtıysa benzersiz indeks yakalar; sorun değil.
            Err(e) if e.contains("uq_gorev_tekrar_acik") => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Takvim: aralıktaki işler + henüz açılmamış rutin günleri (sanal, `sanal: true`).
pub fn takvim<C: GenericClient>(c: &mut C, k: &Kisi, ilk: NaiveDate, son: NaiveDate, herkes: bool, bugun: NaiveDate) -> Result<Value, String> {
    let s = Suzgec {
        gorunum: "takvim".into(),
        sorumlu_id: if herkes { None } else { Some(k.id) },
        ilk: Some(ilk),
        son: Some(son),
        sinir: 1000,
        ..Default::default()
    };
    let isler = gorevler(c, k, &s, bugun)?;
    let tatil = resmi_tatiller(c)?;
    let kurallar = c
        .query(
            "SELECT t.id, t.baslik, t.saat, t.sorumlu_id, k.ad,
                    (SELECT max(g.son_tarih) FROM gorev g WHERE g.tekrar_id = t.id),
                    t.kural, t.haftanin_gunleri, t.ayin_gunu, t.baslangic, t.bitis
             FROM tekrar_kurali t JOIN kullanici k ON k.id = t.sorumlu_id
             WHERE t.aktif AND (NOT t.gizli OR t.sorumlu_id = $1 OR t.olusturan_id = $1) AND ($2 OR t.sorumlu_id = $1)",
            &[&k.id, &herkes],
        )
        .map_err(hata)?;
    let mut rutinler = Vec::new();
    for r in &kurallar {
        let kural = kural_oku(r, 6);
        let son_acilan: Option<NaiveDate> = r.get(5);
        let bas = son_acilan.map(|t| t + Duration::days(1)).unwrap_or(ilk).max(ilk).max(bugun);
        for g in tekrar_gunleri(&kural, bas, son, &tatil) {
            rutinler.push(json!({
                "sanal": true, "tekrar_id": r.get::<_, i64>(0), "baslik": r.get::<_, String>(1),
                "son_tarih": g.to_string(), "son_saat": r.get::<_, Option<NaiveTime>>(2).map(|s| s.format("%H:%M").to_string()),
                "sorumlu_id": r.get::<_, i64>(3), "sorumlu_adi": r.get::<_, String>(4), "tur": "rutin",
            }));
        }
    }
    let tatiller: Vec<String> = tatil.range(ilk..=son).map(|t| t.to_string()).collect();
    Ok(json!({"isler": isler, "rutinler": rutinler, "tatiller": tatiller}))
}

// ── Şablon, süreç, etiket ───────────────────────────────────────────────────────────────────────

pub fn sablonlar<C: GenericClient>(c: &mut C) -> Result<Vec<Value>, String> {
    let sonrakiler = c
        .query(
            "SELECT ss.sablon_id, ss.sonuc, ss.sonraki_sablon_id, ss.sonuc_adi, ss.sure_turu, ss.sure_deger FROM sablon_sonraki ss",
            &[],
        )
        .map_err(hata)?;
    let mut harita: HashMap<i64, Map<String, Value>> = HashMap::new();
    for r in &sonrakiler {
        harita.entry(r.get(0)).or_default().insert(
            r.get::<_, String>(1),
            json!({"sonraki_sablon_id": r.get::<_, Option<i64>>(2), "ad": r.get::<_, String>(3),
                   "sure_turu": r.get::<_, Option<String>>(4), "sure_deger": r.get::<_, Option<i32>>(5)}),
        );
    }
    Ok(c
        .query(
            "SELECT s.id, s.anahtar, s.baslik, s.aciklama, s.kontrol, s.tur, s.onay_gerekir, s.oncelik, s.sure_turu, s.sure_deger,
                    s.taraf, s.eylem, s.evrak_deseni, s.olay_turu, s.surec_asama_id, s.hazir, s.aktif,
                    (SELECT coalesce(array_agg(etiket_id), '{}') FROM sablon_etiket se WHERE se.sablon_id = s.id)
             FROM sablon s ORDER BY s.aktif DESC, s.baslik",
            &[],
        )
        .map_err(hata)?
        .iter()
        .map(|r| {
            let id: i64 = r.get(0);
            json!({
                "id": id, "anahtar": r.get::<_, Option<String>>(1), "baslik": r.get::<_, String>(2), "aciklama": r.get::<_, String>(3),
                "kontrol": r.get::<_, String>(4), "tur": r.get::<_, String>(5), "onay_gerekir": r.get::<_, bool>(6),
                "oncelik": r.get::<_, i16>(7), "sure_turu": r.get::<_, String>(8), "sure_deger": r.get::<_, i32>(9),
                "sure_metni": sure_metni(&r.get::<_, String>(8), r.get(9)),
                "taraf": r.get::<_, String>(10), "eylem": r.get::<_, String>(11), "evrak_deseni": r.get::<_, String>(12),
                "olay_turu": r.get::<_, String>(13), "surec_asama_id": r.get::<_, Option<i64>>(14),
                "hazir": r.get::<_, bool>(15), "aktif": r.get::<_, bool>(16), "etiketler": r.get::<_, Vec<i64>>(17),
                "sonraki": harita.remove(&id).map(Value::Object).unwrap_or_else(|| json!({})),
            })
        })
        .collect())
}

/// Şablon ekler (id yoksa) ya da günceller. Sonraki dallar `sonraki: {olumlu: {...}, ...}`.
pub fn sablon_kaydet<C: GenericClient>(c: &mut C, k: &Kisi, g: &Value) -> Result<i64, String> {
    let m = |x: &str| g.get(x).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let baslik = m("baslik");
    if baslik.is_empty() {
        return Err("Şablonun başlığı boş olamaz.".into());
    }
    let sure_turu = if m("sure_turu").is_empty() { "is_gunu".to_string() } else { m("sure_turu") };
    if !gecerli(SURE_TURLERI, &sure_turu) {
        return Err("Bilinmeyen süre türü.".into());
    }
    let tur = if m("tur").is_empty() { "is".to_string() } else { m("tur") };
    if !gecerli(TURLER, &tur) {
        return Err("Bilinmeyen iş türü.".into());
    }
    let sure_deger = g.get("sure_deger").and_then(|v| v.as_i64()).unwrap_or(1).clamp(0, 3650) as i32;
    let oncelik = g.get("oncelik").and_then(|v| v.as_i64()).unwrap_or(1).clamp(0, 2) as i16;
    let onay = g.get("onay_gerekir").and_then(|v| v.as_bool()).unwrap_or(false);
    let aktif = g.get("aktif").and_then(|v| v.as_bool()).unwrap_or(true);
    let asama = g.get("surec_asama_id").and_then(|v| v.as_i64());
    let mut tx = c.transaction().map_err(hata)?;
    let id: i64 = match g.get("id").and_then(|v| v.as_i64()) {
        Some(id) => {
            let n = tx
                .execute(
                    "UPDATE sablon SET baslik=$2, aciklama=$3, kontrol=$4, tur=$5, onay_gerekir=$6, oncelik=$7, sure_turu=$8,
                        sure_deger=$9, taraf=$10, eylem=$11, evrak_deseni=$12, olay_turu=$13, surec_asama_id=$14, aktif=$15,
                        guncelleme=now() WHERE id=$1",
                    &[&id, &baslik, &m("aciklama"), &m("kontrol"), &tur, &onay, &oncelik, &sure_turu, &sure_deger, &m("taraf"),
                      &m("eylem"), &m("evrak_deseni"), &m("olay_turu"), &asama, &aktif],
                )
                .map_err(hata)?;
            if n == 0 {
                return Err("Şablon bulunamadı.".into());
            }
            id
        }
        None => tx
            .query_one(
                "INSERT INTO sablon (baslik, aciklama, kontrol, tur, onay_gerekir, oncelik, sure_turu, sure_deger, taraf, eylem,
                    evrak_deseni, olay_turu, surec_asama_id, aktif, olusturan_id)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING id",
                &[&baslik, &m("aciklama"), &m("kontrol"), &tur, &onay, &oncelik, &sure_turu, &sure_deger, &m("taraf"),
                  &m("eylem"), &m("evrak_deseni"), &m("olay_turu"), &asama, &aktif, &k.id],
            )
            .map_err(hata)?
            .get(0),
    };
    if let Some(arr) = g.get("etiketler").and_then(|v| v.as_array()) {
        tx.execute("DELETE FROM sablon_etiket WHERE sablon_id = $1", &[&id]).map_err(hata)?;
        for e in arr.iter().filter_map(|x| x.as_i64()) {
            tx.execute("INSERT INTO sablon_etiket VALUES ($1, $2) ON CONFLICT DO NOTHING", &[&id, &e]).map_err(hata)?;
        }
    }
    if let Some(sn) = g.get("sonraki").and_then(|v| v.as_object()) {
        for (sonuc, _) in SONUCLAR {
            match sn.get(*sonuc).filter(|v| v.is_object()) {
                None => {
                    tx.execute("DELETE FROM sablon_sonraki WHERE sablon_id = $1 AND sonuc = $2", &[&id, sonuc]).map_err(hata)?;
                }
                Some(d) => {
                    let st = d.get("sure_turu").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
                    if let Some(s) = &st {
                        if !gecerli(SURE_TURLERI, s) {
                            return Err("Bilinmeyen süre türü.".into());
                        }
                    }
                    let sd = d.get("sure_deger").and_then(|v| v.as_i64()).map(|n| n.clamp(0, 3650) as i32);
                    let sonraki = d.get("sonraki_sablon_id").and_then(|v| v.as_i64());
                    let ad = d.get("ad").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                    tx.execute(
                        "INSERT INTO sablon_sonraki (sablon_id, sonuc, sonraki_sablon_id, sonuc_adi, sure_turu, sure_deger)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT (sablon_id, sonuc) DO UPDATE SET sonraki_sablon_id = EXCLUDED.sonraki_sablon_id,
                             sonuc_adi = EXCLUDED.sonuc_adi, sure_turu = EXCLUDED.sure_turu, sure_deger = EXCLUDED.sure_deger",
                        &[&id, sonuc, &sonraki, &ad, &st, &sd],
                    )
                    .map_err(hata)?;
                }
            }
        }
    }
    tx.commit().map_err(hata)?;
    Ok(id)
}

/// Belge adına uyan şablonlar (B-6): evrak_deseni belge adında geçiyorsa (Türkçe katlanmış,
/// '|' ile birden çok desen) öne çıkar.
pub fn evraga_uyan_sablonlar<C: GenericClient>(c: &mut C, evrak_adi: &str) -> Result<Vec<i64>, String> {
    let ad = tr_kucuk(evrak_adi);
    if ad.trim().is_empty() {
        return Ok(vec![]);
    }
    Ok(c
        .query("SELECT id, evrak_deseni FROM sablon WHERE aktif AND evrak_deseni <> '' ORDER BY id", &[])
        .map_err(hata)?
        .iter()
        .filter(|r| {
            r.get::<_, String>(1)
                .split('|')
                .map(|d| tr_kucuk(d.trim()))
                .any(|d| !d.is_empty() && ad.contains(&d))
        })
        .map(|r| r.get(0))
        .collect())
}

pub fn surecler<C: GenericClient>(c: &mut C) -> Result<Vec<Value>, String> {
    let asamalar = c
        .query("SELECT id, surec_id, anahtar, ad, sira, olay_turu FROM surec_asama ORDER BY surec_id, sira", &[])
        .map_err(hata)?;
    let mut harita: HashMap<i64, Vec<Value>> = HashMap::new();
    for r in &asamalar {
        harita.entry(r.get(1)).or_default().push(json!({
            "id": r.get::<_, i64>(0), "anahtar": r.get::<_, String>(2), "ad": r.get::<_, String>(3),
            "sira": r.get::<_, i32>(4), "olay_turu": r.get::<_, String>(5),
        }));
    }
    Ok(c
        .query("SELECT id, anahtar, ad, aciklama, dosya_turu, hazir, aktif FROM surec ORDER BY ad", &[])
        .map_err(hata)?
        .iter()
        .map(|r| {
            let id: i64 = r.get(0);
            json!({"id": id, "anahtar": r.get::<_, String>(1), "ad": r.get::<_, String>(2), "aciklama": r.get::<_, String>(3),
                   "dosya_turu": r.get::<_, String>(4), "hazir": r.get::<_, bool>(5), "aktif": r.get::<_, bool>(6),
                   "asamalar": harita.remove(&id).unwrap_or_default()})
        })
        .collect())
}

pub fn etiketler<C: GenericClient>(c: &mut C, k: &Kisi) -> Result<Vec<Value>, String> {
    Ok(c
        .query(
            "SELECT id, anahtar, ad, renk, ust_id, sahip_id FROM etiket WHERE sahip_id IS NULL OR sahip_id = $1 ORDER BY ad",
            &[&k.id],
        )
        .map_err(hata)?
        .iter()
        .map(|r| {
            json!({"id": r.get::<_, i64>(0), "anahtar": r.get::<_, Option<String>>(1), "ad": r.get::<_, String>(2),
                   "renk": r.get::<_, String>(3), "ust_id": r.get::<_, Option<i64>>(4), "kisisel": r.get::<_, Option<i64>>(5).is_some()})
        })
        .collect())
}

// ── Hazır veriler (eski sistemdeki kodlanmış süreç/şablon/etiketler) ──────────────────────────

/// Eski sistemin kodlanmış süreç, şablon ve etiketlerini (zs_store::hazir_*) tablolara yazar.
/// Yalnız eksikleri ekler; kullanıcının sonradan yaptığı değişikliklere dokunmaz. Eklenen kayıt sayısı.
/// Açılışta: büroda hiç hazır şablon yoksa (yeni kurulum) hazır verileri yükler; varsa dokunmaz.
pub fn hazir_verileri_ilk_kez<C: GenericClient>(c: &mut C) -> Result<usize, String> {
    let var: bool = c.query_one("SELECT EXISTS (SELECT 1 FROM sablon WHERE hazir)", &[]).map_err(hata)?.get(0);
    if var {
        return Ok(0);
    }
    hazir_verileri_yukle(c)
}

pub fn hazir_verileri_yukle<C: GenericClient>(c: &mut C) -> Result<usize, String> {
    use crate::zs_store as zs;
    let mut tx = c.transaction().map_err(hata)?;
    let mut n = 0usize;
    // Etiketler (aşama etiketleri hariç: aşamalar artık surec_asama tablosunda)
    let ham = zs::hazir_etiketler();
    for e in ham.iter().filter(|e| e.get("asama").is_none()) {
        let anahtar = e["key"].as_str().unwrap_or("");
        n += tx
            .execute(
                "INSERT INTO etiket (anahtar, ad, renk) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                &[&anahtar, &e["name"].as_str().unwrap_or(anahtar), &e["color"].as_str().unwrap_or("")],
            )
            .map_err(hata)? as usize;
    }
    for e in ham.iter().filter(|e| e.get("asama").is_none()) {
        if let Some(ust) = e["parent"].as_str() {
            tx.execute(
                "UPDATE etiket SET ust_id = (SELECT id FROM etiket WHERE anahtar = $2) WHERE anahtar = $1 AND ust_id IS NULL",
                &[&e["key"].as_str().unwrap_or(""), &ust],
            )
            .map_err(hata)?;
        }
    }
    // Süreçler ve aşamalar
    for s in zs::hazir_surecler() {
        let anahtar = s["key"].as_str().unwrap_or("");
        let sid: i64 = tx
            .query_one(
                "INSERT INTO surec (anahtar, ad, dosya_turu, hazir) VALUES ($1, $2, $3, true)
                 ON CONFLICT (anahtar) DO UPDATE SET anahtar = EXCLUDED.anahtar RETURNING id, (xmax = 0)",
                &[&anahtar, &s["title"].as_str().unwrap_or(anahtar), &s["tur"].as_str().unwrap_or("")],
            )
            .map(|r| {
                if r.get::<_, bool>(1) {
                    n += 1;
                }
                r.get(0)
            })
            .map_err(hata)?;
        for (i, a) in s["asamalar"].as_array().cloned().unwrap_or_default().iter().enumerate() {
            n += tx
                .execute(
                    "INSERT INTO surec_asama (surec_id, anahtar, ad, sira, olay_turu) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                    &[&sid, &a["key"].as_str().unwrap_or(""), &a["title"].as_str().unwrap_or(""), &(i as i32 + 1),
                      &a["otomatik"].as_str().unwrap_or("")],
                )
                .map_err(hata)? as usize;
        }
    }
    // Şablonlar: önce satırlar, sonra sonraki-iş dalları (başlıkla eşlenir).
    let sablonlar = zs::hazir_sablonlar_exact();
    let mut yeni_eklenen: Vec<String> = Vec::new();
    for s in &sablonlar {
        let anahtar = s["id"].as_str().unwrap_or("");
        let asama = s["asama"].as_str().unwrap_or("");
        let asama_id: Option<i64> = match asama.split_once('/') {
            Some((su, a)) => tx
                .query_opt(
                    "SELECT sa.id FROM surec_asama sa JOIN surec s ON s.id = sa.surec_id WHERE s.anahtar = $1 AND sa.anahtar = $2",
                    &[&su, &a],
                )
                .map_err(hata)?
                .map(|r| r.get(0)),
            None => None,
        };
        let eski: Vec<String> =
            s["eski"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let eklendi = tx
            .execute(
                "INSERT INTO sablon (anahtar, baslik, aciklama, kontrol, tur, oncelik, sure_turu, sure_deger, taraf, eylem,
                     surec_asama_id, eski_adlar, hazir, olay_turu)
                 VALUES ($1, $2, $3, $4, 'is', $5, 'is_gunu', $6, $7, $8, $9, $10, true, $11) ON CONFLICT (anahtar) DO NOTHING",
                &[
                    &anahtar,
                    &s["title"].as_str().unwrap_or(anahtar),
                    &s["description"].as_str().unwrap_or(""),
                    &s["kontrol"].as_str().unwrap_or(""),
                    &(s["priority"].as_i64().unwrap_or(1).clamp(0, 2) as i16),
                    &(s["due_days"].as_i64().unwrap_or(1).max(0) as i32),
                    &s["taraf"].as_str().unwrap_or(""),
                    &s["action"].as_str().unwrap_or(""),
                    &asama_id,
                    &eski,
                    &crate::is_otomatik::hazir_olay_turu(anahtar),
                ],
            )
            .map_err(hata)?;
        if eklendi > 0 {
            n += 1;
            yeni_eklenen.push(anahtar.to_string());
            for et in s["etiketler"].as_array().cloned().unwrap_or_default() {
                tx.execute(
                    "INSERT INTO sablon_etiket SELECT s.id, e.id FROM sablon s, etiket e WHERE s.anahtar = $1 AND e.anahtar = $2 ON CONFLICT DO NOTHING",
                    &[&anahtar, &et.as_str().unwrap_or("")],
                )
                .map_err(hata)?;
            }
        }
    }
    for s in sablonlar.iter().filter(|s| yeni_eklenen.iter().any(|a| Some(a.as_str()) == s["id"].as_str())) {
        let anahtar = s["id"].as_str().unwrap_or("");
        for (sonuc, _) in SONUCLAR {
            let d = &s["sonuclar"][*sonuc];
            if !d.is_object() {
                continue;
            }
            let hedef = d["baslik"].as_str().unwrap_or("").trim();
            let sonraki: Option<i64> = if hedef.is_empty() {
                None
            } else {
                tx.query_opt("SELECT id FROM sablon WHERE baslik = $1 OR $1 = ANY(eski_adlar) ORDER BY hazir DESC, id LIMIT 1", &[&hedef])
                    .map_err(hata)?
                    .map(|r| r.get(0))
            };
            if !hedef.is_empty() && sonraki.is_none() {
                // Hedef şablon tanımlı değil ("Ödeme planı takibi" gibi): dal atlanır, kullanıcı tanımlar.
                continue;
            }
            let gun = d["gun"].as_i64().map(|g| g.max(0) as i32);
            tx.execute(
                "INSERT INTO sablon_sonraki (sablon_id, sonuc, sonraki_sablon_id, sonuc_adi, sure_turu, sure_deger)
                 SELECT id, $2, $3, $4, CASE WHEN $5::int IS NULL THEN NULL ELSE 'is_gunu' END, $5 FROM sablon WHERE anahtar = $1
                 ON CONFLICT DO NOTHING",
                &[&anahtar, sonuc, &sonraki, &d["ad"].as_str().unwrap_or(""), &gun],
            )
            .map_err(hata)?;
        }
    }
    tx.commit().map_err(hata)?;
    Ok(n)
}

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn t(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn is_gunu_ve_sure() {
        let mut tatil = Tatiller::new();
        tatil.insert(t(2026, 10, 29)); // Perşembe, Cumhuriyet Bayramı
        let cuma = t(2026, 10, 23);
        assert_eq!(sure_hesapla(cuma, "takvim_gunu", 3, &tatil), Some(t(2026, 10, 26)));
        assert_eq!(sure_hesapla(cuma, "is_gunu", 1, &tatil), Some(t(2026, 10, 26)), "hafta sonu atlanır");
        assert_eq!(sure_hesapla(t(2026, 10, 28), "is_gunu", 1, &tatil), Some(t(2026, 10, 30)), "tatil atlanır");
        assert_eq!(sure_hesapla(t(2026, 10, 24), "is_gunu", 0, &tatil), Some(t(2026, 10, 26)), "0 = ilk iş günü");
        assert_eq!(sure_hesapla(cuma, "tarih_sor", 3, &tatil), None);
        // Ayın günü: geçtiyse sonraki ay; ay kısaysa son gün; iş gününe kayar
        assert_eq!(sure_hesapla(t(2026, 1, 10), "ayin_gunu", 15, &tatil), Some(t(2026, 1, 15)));
        assert_eq!(sure_hesapla(t(2026, 1, 15), "ayin_gunu", 15, &tatil), Some(t(2026, 2, 16)), "15 Şubat Pazar → 16");
        assert_eq!(sure_hesapla(t(2026, 1, 31), "ayin_gunu", 31, &tatil), Some(t(2026, 3, 2)), "28 Şubat Ctesi → 2 Mart");
        assert_eq!(sure_metni("is_gunu", 3), "3 iş günü sonra");
    }

    #[test]
    fn tekrar_kurallari_hesabi() {
        let tatil: Tatiller = [t(2026, 6, 1)].into_iter().collect(); // Pazartesi tatil (deneme)
        let k = |kural: &str| TekrarKurali {
            kural: kural.into(),
            haftanin_gunleri: vec![],
            ayin_gunu: None,
            baslangic: t(2026, 1, 1),
            bitis: None,
        };
        let cumartesi = t(2026, 5, 30);
        assert_eq!(tekrar_tarihi(&k("gunluk"), cumartesi, &tatil), Some(cumartesi));
        assert_eq!(tekrar_tarihi(&k("is_gunu"), cumartesi, &tatil), Some(t(2026, 6, 2)), "hafta sonu + tatil atlanır");
        let mut h = k("haftalik");
        h.haftanin_gunleri = vec![3, 5]; // Çarşamba, Cuma
        assert_eq!(tekrar_tarihi(&h, cumartesi, &tatil), Some(t(2026, 6, 3)));
        assert_eq!(tekrar_tarihi(&k("haftalik"), cumartesi, &tatil), None, "gün seçilmemiş");
        let mut a = k("aylik");
        a.ayin_gunu = Some(31);
        assert_eq!(tekrar_tarihi(&a, t(2026, 2, 1), &tatil), Some(t(2026, 2, 28)), "kısa ayda son gün");
        assert_eq!(tekrar_tarihi(&a, t(2026, 3, 31), &tatil), Some(t(2026, 3, 31)));
        assert_eq!(tekrar_tarihi(&k("ay_basi_is_gunu"), t(2026, 5, 15), &tatil), Some(t(2026, 6, 2)), "1 Haziran tatil");
        assert_eq!(tekrar_tarihi(&k("ay_basi_is_gunu"), t(2026, 2, 1), &tatil), Some(t(2026, 2, 2)), "1 Şubat Pazar");
        let mut b = k("gunluk");
        b.bitis = Some(t(2026, 5, 29));
        assert_eq!(tekrar_tarihi(&b, cumartesi, &tatil), None, "bitiş geçti");
        b.bitis = None;
        b.baslangic = t(2026, 7, 1);
        assert_eq!(tekrar_tarihi(&b, cumartesi, &tatil), Some(t(2026, 7, 1)), "başlangıçtan önce açılmaz");
        assert_eq!(tekrar_gunleri(&h, t(2026, 6, 1), t(2026, 6, 14), &tatil).len(), 4);
    }

    #[test]
    fn arama_katlama() {
        assert_eq!(arama_katla(&["İZMİR  1. İcra", "", "Şahin ÇİFTÇİ"]), "izmir 1. icra sahin ciftci");
        assert_eq!(arama_sozcukleri("Çiftçi 50%_x"), vec!["ciftci".to_string(), "50\\%\\_x".to_string()]);
    }

    fn oz(durum: &str, onay: bool, sorumlu: i64, olusturan: i64) -> GorevOzu {
        GorevOzu {
            id: 1,
            ust_gorev_id: None,
            durum: durum.into(),
            onay_gerekir: onay,
            gizli: false,
            sorumlu_id: Some(sorumlu),
            olusturan_id: Some(olusturan),
            son_tarih: t(2026, 1, 1),
            son_saat: None,
            sablon_id: None,
            tekrar_id: None,
        }
    }

    #[test]
    fn onay_secimli_gecisler() {
        let uye = Kisi { id: 2, yonetici: false };
        let amir = Kisi { id: 1, yonetici: true };
        let baska = Kisi { id: 3, yonetici: false };
        // Onay gerekmeyen iş: sorumlu doğrudan tamamlar
        let g = oz("acik", false, 2, 1);
        let y = yetki_hesapla(&g, &uye, &[]);
        assert_eq!(gecis_coz(&g, "tamamla", &y), Ok("tamamlandi"));
        // Onay gereken iş: sorumlu onaya sunar, yönetici onaylar
        let g = oz("devam", true, 2, 1);
        assert_eq!(gecis_coz(&g, "tamamla", &yetki_hesapla(&g, &uye, &[])), Ok("onayda"));
        let g = oz("onayda", true, 2, 1);
        assert!(gecis_coz(&g, "tamamla", &yetki_hesapla(&g, &uye, &[])).is_err(), "sorumlu kendi onaylayamaz");
        assert_eq!(gecis_coz(&g, "tamamla", &yetki_hesapla(&g, &amir, &[])), Ok("tamamlandi"));
        assert_eq!(gecis_coz(&g, "devam", &yetki_hesapla(&g, &amir, &[])), Ok("devam"), "geri çevirme");
        // Atanmış onaylayıcı da onaylar
        assert_eq!(gecis_coz(&g, "tamamla", &yetki_hesapla(&g, &baska, &["onaylayici".into()])), Ok("tamamlandi"));
        // İlgisiz üye dokunamaz
        let g = oz("acik", false, 2, 1);
        assert!(gecis_coz(&g, "tamamla", &yetki_hesapla(&g, &baska, &[])).is_err());
        // Yönetici onay gereken işi doğrudan kapatabilir; açıkça "onayda" isterse onaya gider
        let g = oz("acik", true, 1, 1);
        assert_eq!(gecis_coz(&g, "tamamla", &yetki_hesapla(&g, &amir, &[])), Ok("tamamlandi"));
        assert_eq!(gecis_coz(&g, "onayda", &yetki_hesapla(&g, &amir, &[])), Ok("onayda"));
        // Kendine iş açan üye (onay gerekir): kendisi onaylayamaz, onaya gider
        let g = oz("acik", true, 2, 2);
        assert_eq!(gecis_coz(&g, "tamamla", &yetki_hesapla(&g, &uye, &[])), Ok("onayda"));
        // İptal: yalnız açan/yönetici
        let g = oz("acik", false, 2, 1);
        assert!(gecis_coz(&g, "iptal", &yetki_hesapla(&g, &uye, &[])).is_err());
        assert_eq!(gecis_coz(&g, "iptal", &yetki_hesapla(&g, &amir, &[])), Ok("iptal"));
        // Kapanmış iş: yalnız onaylayabilen yeniden açar
        let g = oz("tamamlandi", false, 2, 1);
        assert!(gecis_coz(&g, "devam", &yetki_hesapla(&g, &uye, &[])).is_err());
        assert_eq!(gecis_coz(&g, "devam", &yetki_hesapla(&g, &amir, &[])), Ok("devam"));
        // Gizli iş başkasına görünmez
        let mut g = oz("acik", false, 2, 2);
        g.gizli = true;
        assert!(!yetki_hesapla(&g, &amir, &[]).gorebilir);
        assert!(yetki_hesapla(&g, &uye, &[]).calisabilir);
    }
}

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    let bugun = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap(); // Cuma
    // Hazır veriler: iki kez yüklemek tekrar kayıt üretmez
    let n1 = hazir_verileri_yukle(c).unwrap();
    assert!(n1 > 20, "hazır veriler yüklenmeli ({n1})");
    assert_eq!(hazir_verileri_yukle(c).unwrap(), 0, "ikinci yükleme bir şey eklemez");
    let say = |c: &mut postgres::Client, sql: &str| -> i64 { c.query_one(sql, &[]).unwrap().get(0) };
    assert_eq!(say(c, "SELECT count(*) FROM surec_asama"), 13);
    assert!(say(c, "SELECT count(*) FROM sablon_sonraki WHERE sonraki_sablon_id IS NOT NULL") > 10);

    let _amir_id = kullanici_esitle(c, "amir", "Av. Amir", true, true).unwrap();
    let uye_id = kullanici_esitle(c, "uye", "Üye Kişi", false, true).unwrap();
    assert_eq!(kullanici_esitle(c, "uye", "Üye Kişi 2", false, true).unwrap(), uye_id, "eşitleme günceller");
    let amir = kisi_bul(c, "amir").unwrap().unwrap();
    let uye = kisi_bul(c, "uye").unwrap().unwrap();
    assert!(amir.yonetici && !uye.yonetici);

    // Dosyaya bağlı, şablonlu, onay gereken iş: süre şablondan (iş günü)
    let sablon: i64 = c.query_one("SELECT id FROM sablon WHERE anahtar = 'h-odeme-emri'", &[]).unwrap().get(0);
    // Önceki senaryolar dosya tablosunu temizliyor: kendi künyesini açar.
    let rec = json!({
        "dosyaId": "is-senaryo-1", "dosyaNo": "2025/999", "dosyaDurumKod": 0, "dosyaDurum": "Açık",
        "dosyaTurKod": 35, "dosyaTur": "İcra Dosyası",
        "birimAdi": "İzmir 2. İcra Dairesi", "birimId": 1000429, "birimTuru1": "11", "birimTuru2": "1101", "birimTuru3": "1199"
    });
    let (dosya, _) = crate::icra_veri::dosya_kunyesi_kaydet(c, &rec, Some(crate::icra_veri::YargiTuru::Icra.kod())).unwrap();
    let y = YeniGorev {
        baslik: "Tebligat oluşturuldu kontrol".into(),
        sorumlu_id: Some(uye_id),
        sablon_id: Some(sablon),
        dosya_id: Some(dosya),
        taraf_adi: "ŞAHİN ÇİFTÇİ".into(),
        onay_gerekir: true,
        ..Default::default()
    };
    let g1 = gorev_ac(c, Some(&amir), &y, bugun).unwrap();
    let a = gorev_ayrinti(c, &uye, g1, bugun).unwrap().unwrap();
    assert_eq!(a["son_tarih"], "2026-11-18", "3 iş günü: Pzt-Sal-Çar");
    assert!(!a["dosya_no"].as_str().unwrap().is_empty(), "künye dosyadan gelir");
    assert!(!a["sonuclar"].as_array().unwrap().is_empty());
    // Arama: Türkçe katlanmış
    let s = Suzgec { ara: "sahin ciftci".into(), ..Default::default() };
    assert_eq!(gorevler(c, &uye, &s, bugun).unwrap().len(), 1);
    // Bugün görünümü: tarihi gelmemiş iş yok
    let bg = Suzgec { gorunum: "bugun".into(), ..Default::default() };
    assert_eq!(gorevler(c, &uye, &bg, bugun).unwrap().len(), 0);
    assert_eq!(gorevler(c, &uye, &bg, NaiveDate::from_ymd_opt(2026, 11, 25).unwrap()).unwrap().len(), 1, "gecikenler görünür");

    // Erteleme: onay gereken işte üyenin isteği onaya gider, yönetici kabul eder
    let yeni = NaiveDate::from_ymd_opt(2026, 11, 19).unwrap();
    assert!(!ertele(c, &uye, g1, yeni, None, "Kalem kapalı").unwrap());
    assert_eq!(ozet(c, &amir, bugun).unwrap()["erteleme"], 1);
    let eid: i64 = c.query_one("SELECT id FROM gorev_erteleme WHERE gorev_id = $1 AND durum = 'bekliyor'", &[&g1]).unwrap().get(0);
    assert!(erteleme_karar(c, &uye, eid, true, "").is_err(), "üye kendi ertelemesine karar veremez");
    erteleme_karar(c, &amir, eid, true, "Tamam").unwrap();
    assert_eq!(gorev_ayrinti(c, &uye, g1, bugun).unwrap().unwrap()["son_tarih"], "2026-11-19");

    // Onaya sunma: öneri hemen gelir (B-7)
    let d = durum_degistir(c, &uye, g1, "tamamla", "olumlu", "Oluşturulmuş", bugun).unwrap();
    assert_eq!(d.durum, "onayda");
    let o = d.oneri.expect("öneri");
    assert_eq!(o.baslik, "PTT'ye verildi kontrol");
    assert_eq!(o.son_tarih, Some(NaiveDate::from_ymd_opt(2026, 11, 17).unwrap()), "2 iş günü");
    // Onaylanınca yeniden öneri yok
    let d2 = durum_degistir(c, &amir, g1, "tamamla", "olumlu", "", bugun).unwrap();
    assert_eq!(d2.durum, "tamamlandi");
    assert!(d2.oneri.is_none());
    // Öneriden iş: dosya bağı ve taraf adı taşınır
    let g2 = oneriden_ac(c, &uye, g1, o.sablon_id, o.son_tarih.unwrap(), None, None, bugun).unwrap();
    let a2 = gorev_ayrinti(c, &uye, g2, bugun).unwrap().unwrap();
    assert_eq!(a2["onceki_gorev_id"], g1);
    assert_eq!(a2["dosya_id"], dosya);
    assert!(a2["baslik"].as_str().unwrap().starts_with("ŞAHİN ÇİFTÇİ – "));

    // Alt iş varken tamamlanamaz
    let alt = gorev_ac(c, Some(&uye), &YeniGorev { baslik: "Alt".into(), ust_gorev_id: Some(g2), ..Default::default() }, bugun).unwrap();
    assert!(durum_degistir(c, &uye, g2, "tamamla", "", "", bugun).is_err());
    durum_degistir(c, &uye, alt, "tamamla", "", "", bugun).unwrap();
    assert_eq!(durum_degistir(c, &uye, g2, "tamamla", "", "", bugun).unwrap().durum, "tamamlandi");

    // Gizli iş: yönetici bile görmez
    let gz = gorev_ac(
        c,
        Some(&uye),
        &YeniGorev { baslik: "Özel".into(), gizli: true, son_tarih: Some(bugun), ..Default::default() },
        bugun,
    )
    .unwrap();
    assert!(gorev_ayrinti(c, &amir, gz, bugun).unwrap().is_none());
    assert!(gorevler(c, &amir, &Suzgec::default(), bugun).unwrap().iter().all(|g| g["id"] != gz));

    // Tekrar: her iş günü; tek açık örnek, bitmeyen birikmez
    let kid = tekrar_kurali_ekle(c, &uye, &json!({"baslik": "UYAP tebligat kontrolü", "kural": "is_gunu", "saat": "09:30", "baslangic": bugun.to_string()})).unwrap();
    assert_eq!(tekrarlari_uret(c, bugun).unwrap(), 1);
    assert_eq!(tekrarlari_uret(c, bugun).unwrap(), 0, "aynı gün ikinci örnek yok");
    let sonraki_pzt = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    assert_eq!(tekrarlari_uret(c, sonraki_pzt).unwrap(), 0, "bitmeyen örnek varken yenisi açılmaz");
    let ornek: i64 = c.query_one("SELECT id FROM gorev WHERE tekrar_id = $1", &[&kid]).unwrap().get(0);
    durum_degistir(c, &uye, ornek, "tamamla", "", "", sonraki_pzt).unwrap();
    assert_eq!(tekrarlari_uret(c, sonraki_pzt).unwrap(), 1, "tamamlanınca bugünkü örnek açılır");
    let tk = takvim(c, &uye, sonraki_pzt, NaiveDate::from_ymd_opt(2026, 11, 22).unwrap(), false, sonraki_pzt).unwrap();
    assert_eq!(tk["rutinler"].as_array().unwrap().len(), 4, "Sal–Cuma sanal rutin günleri");

    // Şablon kaydı + belge adı eşleşmesi (B-6)
    let sid = sablon_kaydet(
        c,
        &amir,
        &json!({"baslik": "Tebligat oluşturuldu mu kontrol et", "sure_turu": "takvim_gunu", "sure_deger": 2,
                "evrak_deseni": "avukat portal tebligat talebi", "sonraki": {"olumlu": {"ad": "Oluşturuldu", "sonraki_sablon_id": sablon}}}),
    )
    .unwrap();
    assert_eq!(evraga_uyan_sablonlar(c, "AVUKAT PORTAL TEBLİGAT TALEBİ (12.11.2026)").unwrap(), vec![sid]);
    assert!(sablonlar(c).unwrap().iter().any(|s| s["id"] == sid && s["sonraki"]["olumlu"]["sonraki_sablon_id"] == sablon));

    // Geçmiş tek olay listesiyle yazıldı
    let turler: Vec<String> = c.query("SELECT DISTINCT tur FROM gorev_olay", &[]).unwrap().iter().map(|r| r.get(0)).collect();
    for t in &turler {
        assert!(
            [olay::ACILDI, olay::DURUM, olay::ERTELEME_ISTENDI, olay::ERTELENDI, olay::NOT, olay::SONRAKI_ACILDI, olay::GUNCELLENDI]
                .contains(&t.as_str()),
            "bilinmeyen olay türü: {t}"
        );
    }
}

/// Ekler: yükle → ayrıntıda görünür → indir (aynı bayt) → yetkisiz silme reddi → sil (disktekiyle).
/// `veritabani::tests::uctan_uca` içinden, `veritabani_senaryosu`ndan sonra çağrılır.
#[cfg(test)]
pub(crate) fn ek_senaryosu(c: &mut postgres::Client) {
    let bugun = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let amir = kisi_bul(c, "amir").unwrap().expect("amir");
    let uye = kisi_bul(c, "uye").unwrap().expect("uye");
    let y = YeniGorev { baslik: "Ek denemesi".into(), son_tarih: Some(bugun), sorumlu_id: Some(amir.id), ..Default::default() };
    let gid = gorev_ac(c, Some(&amir), &y, bugun).unwrap();
    assert!(ek_ekle(c, &amir, gid, "x", "", b"").is_err(), "boş dosya");
    let veri = "Taşınmaz satış ilanı".as_bytes();
    let eid = ek_ekle(c, &amir, gid, "Satış/İlanı:1.pdf", "application/pdf", veri).unwrap();
    let a = gorev_ayrinti(c, &amir, gid, bugun).unwrap().unwrap();
    assert_eq!(a["ekler"][0]["ad"], "Satış İlanı 1.pdf", "ad temizlenir");
    assert_eq!(a["ekler"][0]["yukleyen_id"], amir.id);
    let (ad, mime, oku) = ek_oku(c, &uye, eid).unwrap();
    assert_eq!((ad.as_str(), mime.as_str(), oku.as_slice()), ("Satış İlanı 1.pdf", "application/pdf", veri));
    assert!(ek_sil(c, &uye, eid).is_err(), "yükleyen ya da yönetici değil");
    ek_sil(c, &amir, eid).unwrap();
    assert!(ek_oku(c, &amir, eid).is_err());
    use sha2::{Digest, Sha256};
    assert!(!crate::is_aktar::ekler_dizini().join(hex::encode(Sha256::digest(veri))).exists(), "dosya da silinir");
    c.execute("DELETE FROM gorev WHERE id = $1", &[&gid]).unwrap();
}
