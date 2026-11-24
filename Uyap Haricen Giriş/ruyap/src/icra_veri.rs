//! İcra veri modeli — büro veritabanındaki tablolar üzerinde çalışan temel katman (Aşama 2).
//!
//! Şema `src/sema/0002_icra.sql` dosyasındadır ve örnek modelden (models/icra_models/models.py,
//! 0035 hâli) `araclar/sema_uret.py` ile üretilir; tablo ve alanlar örnekle birebir aynıdır.
//! Burada örnek modelin KENDİ kuralları vardır:
//!   • seçenek (choices) değerleri — veritabanında zorlanmaz, burada sabittir
//!   • ham tarih metinlerinden gerçek tarih türetme (örnekteki `HamTarihliModel` + `tarih.py`)
//!   • kapak künyesi kaydı (örnekteki `ingest.dosya_kunyesi_kaydet`)
//!   • işlem durumu, durum olayı, PTT hareketleri, kesinleşme kuralı seçimi
//! Tarama (UYAP/PTT çağrıları) Aşama 3'te, kesinleşme / takip aşaması hesabı Aşama 4'te gelir.
//!
//! Bütün işlevler eşzamanlıdır (`postgres` istemcisi); async koddan `spawn_blocking` içinde
//! çağrılmalıdır. Zaman damgaları: sunucunun saat dilimi Europe/Istanbul'dur; yerel (dilimsiz)
//! zamanlar `$n::timestamp` ile gönderilir, dilim çevirisini sunucu yapar.

use chrono::{NaiveDate, NaiveDateTime};
use postgres::GenericClient;
use serde_json::Value;
use std::collections::BTreeSet;

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

// ── Seçenekler ──────────────────────────────────────────────────────────────────────────────────

macro_rules! metin_secenek {
    ($(#[$m:meta])* $ad:ident { $($v:ident = $kod:expr, $etiket:expr;)+ }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $ad { $($v),+ }
        impl $ad {
            pub const HEPSI: &'static [$ad] = &[$($ad::$v),+];
            pub const fn kod(self) -> &'static str { match self { $($ad::$v => $kod),+ } }
            pub const fn etiket(self) -> &'static str { match self { $($ad::$v => $etiket),+ } }
            pub fn coz(kod: &str) -> Option<Self> {
                Self::HEPSI.iter().copied().find(|s| s.kod() == kod)
            }
            /// Kodun görünen adı; bilinmeyen kod olduğu gibi döner.
            pub fn etiket_bul(kod: &str) -> String {
                Self::coz(kod).map(|s| s.etiket().to_string()).unwrap_or_else(|| kod.to_string())
            }
        }
    };
}

macro_rules! sayi_secenek {
    ($(#[$m:meta])* $ad:ident : $t:ty { $($v:ident = $kod:expr, $etiket:expr;)+ }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $ad { $($v),+ }
        impl $ad {
            pub const HEPSI: &'static [$ad] = &[$($ad::$v),+];
            pub const fn kod(self) -> $t { match self { $($ad::$v => $kod),+ } }
            pub const fn etiket(self) -> &'static str { match self { $($ad::$v => $etiket),+ } }
            pub fn coz(kod: $t) -> Option<Self> {
                Self::HEPSI.iter().copied().find(|s| s.kod() == kod)
            }
        }
    };
}

sayi_secenek! {
    /// UYAP'ın sabit yargı türü kodları (`birim.yargi_turu`, `dosya.yargi_turu`).
    YargiTuru: i16 {
        Ceza = 0, "Ceza";
        Hukuk = 1, "Hukuk";
        Icra = 2, "İcra";
        Cbs = 3, "Cbs";
        IdariYargi = 6, "İdari Yargı";
        SatisMemurlugu = 11, "Satış Memurluğu";
        Arabuluculuk = 25, "Arabuluculuk";
        TazminatKomisyonu = 26, "Tazminat Komisyonu Başkanlığı";
    }
}

sayi_secenek! {
    /// UYAP'ın `dosyaDurumKod` değerleri (yalnız canlı doğrulananlar; tam liste değil).
    DosyaDurum: i32 {
        Acik = 0, "Açık";
        Kapali = 1, "Kapalı";
        Yargitayda = 3, "Yargıtayda";
        KararaCikmis = 7, "Karara Çıkmış";
        YargitaydanDondu = 16, "Yargıtaydan Döndü";
        Istinafta = 29, "İstinafta";
        IstinaftanKismiDondu = 31, "İstinaftan Kısmi Döndü";
    }
}

sayi_secenek! {
    /// UYAP'ın `dosyaTurKod` değerleri (yalnız canlı doğrulananlar; tam liste değil).
    DosyaTur: i32 {
        Esas = 0, "Esas";
        Talimat = 1, "Talimat";
        CezaDava = 3, "Ceza Dava Dosyası";
        HukukDegisikIs = 14, "Hukuk Değişik İş Dosyası";
        HukukDava = 15, "Hukuk Dava Dosyası";
        CbsSorusturma = 16, "CBS Soruşturma Dosyası";
        IcraDosyasi = 35, "İcra Dosyası";
    }
}

metin_secenek! {
    /// Büronun kendi iş işareti (`dosya.islem_durumu`); UYAP'tan gelmez.
    IslemDurumu {
        Aktif = "aktif", "Aktif - işlem yapılabilir";
        Pasif = "pasif", "Pasif - işlem yapma";
        Beklemede = "beklemede", "Beklemede";
    }
}

metin_secenek! {
    TarafTur {
        Gercek = "gercek", "Gerçek Kişi";
        Tuzel = "tuzel", "Tüzel Kişi";
    }
}

metin_secenek! {
    /// `dosya_taraf.rol` — UYAP rol metninin küçük harfe / ASCII'ye indirgenmiş hâli.
    Rol {
        Alacakli = "alacakli", "Alacaklı";
        Borclu = "borclu", "Borçlu";
        Davaci = "davaci", "Davacı";
        Davali = "davali", "Davalı";
        Sanik = "sanik", "Sanık";
        Musteki = "musteki", "Müşteki";
        UcuncuSahis = "ucuncu sahis", "Üçüncü Şahıs";
        Hissedar = "hissedar", "Hissedar";
        TasfiyeMemuru = "tasfiye memuru", "Tasfiye Memuru";
        Basvurucu = "basvurucu", "Başvurucu";
        DigerTaraf = "diger taraf", "Diğer Taraf";
        Katilan = "katilan", "Katılan";
        Vasi = "vasi", "Vasi";
        HacizAlacaklisi = "haciz alacaklisi", "Haciz Alacaklısı";
    }
}

metin_secenek! {
    /// Borçlu başına kesinleşme durumu (`dosya_taraf.kesinlesme_durumu`).
    KesinlesmeDurumu {
        Kesinlesti = "kesinlesti", "Kesinleşti";
        KesinlesmeBekliyor = "kesinlesme_bekliyor", "Kesinleşme Bekliyor";
        Itiraz = "itiraz", "İtiraz";
        Konkordato = "konkordato", "Konkordato";
        Vefat = "vefat", "Vefat";
        Vasi = "vasi", "Vasi";
        TehiriIcra = "tehiri_icra", "Tehir-i İcra";
        DurdurulmusIcraMahItiraz = "durd_icra_mah_itiraz", "Durdurulmuş: İcra Mahkemesine İtiraz";
        DurdurulmusGenelMahDava = "durd_genel_mah_dava", "Durdurulmuş: Genel Mahkemeye Dava Açılması";
        DurdurulmusTehiriIcra = "durd_tehiri_icra", "Durdurulmuş: Tehir-i İcra Kararı";
        DurdurulmusIhtiyatiTedbir = "durd_ihtiyati_tedbir", "Durdurulmuş: İhtiyati Tedbir";
        DurdurulmusIflas = "durd_iflas", "Durdurulmuş: İflas";
        DurdurulmusBorclununOlumu = "durd_borclu_olumu", "Durdurulmuş: Borçlunun Ölümü";
        DurdurulmusDiger = "durd_diger", "Durdurulmuş: Diğer";
        DurdurulmusTanimsiz = "durd_tanimsiz", "Durdurulmuş: Tanımsız Neden";
    }
}

metin_secenek! {
    /// Borçlu başına tebliğ durumu (`dosya_taraf.tebligat_durumu`) — ödeme emri için.
    TebligatDurumu {
        TebligEdildi = "teblig_edildi", "Tebliğ Edildi";
        Iade = "iade", "İade";
        Dagitimda = "dagitimda", "Dağıtımda";
        MernisYok = "mernis_yok", "Mernis Yok";
        YenidenTebligTalepEdilecek = "yeniden_teblig_talep_edilecek", "Yeniden Tebliğ Talep Edilecek";
        YenidenTebligTalepEdildi = "yeniden_teblig_talep_edildi", "Yeniden Tebliğ Talep Edildi";
    }
}

metin_secenek! {
    /// Borçlu başına takip aşaması (`dosya_taraf.takip_asamasi`) — örnek modelde yok, bizim ekimiz.
    /// Sıra = süreçteki ilerleyiş. Hesabı Aşama 4'te (`icra_durum`).
    TakipAsamasi {
        TakipTalebi = "takip_talebi", "Takip Talebi";
        OdemeEmri = "odeme_emri", "Ödeme Emri";
        TebligatOlusturuluyor = "teb_olusturuluyor", "Ödeme Emri Tebligatı Oluşturuluyor";
        PttVeriliyor = "ptt_veriliyor", "Ödeme Emri PTT'ye Veriliyor";
        TebligeCikti = "teblige_cikti", "Ödeme Emri Tebliğe Çıktı";
        Iade = "iade", "İade";
        Teblig = "teblig", "Tebliğ";
        Itiraz = "itiraz", "İtiraz";
        Durduruldu = "durduruldu", "Durduruldu";
        Kesinlesti = "kesinlesti", "Kesinleşti";
    }
}

metin_secenek! {
    /// `dosya_tahsilat_harc.tur`
    TahsilatTur {
        Tahsilat = "tahsilat", "Tahsilat";
        Harc = "harc", "Harç";
    }
}

metin_secenek! {
    /// `durum_olayi.tur`
    OlayTur {
        Kesinlesme = "kesinlesme", "Kesinleşme";
        Tebligat = "tebligat", "Tebliğ";
        TarafGorulmedi = "taraf_gorulmedi", "Taraf UYAP'ta görülmedi";
        TarafGeriGeldi = "taraf_geri_geldi", "Taraf UYAP'ta yeniden görüldü";
        TarafKimlik = "taraf_kimlik", "Taraf kimliğiyle ayrıldı";
        DosyaDurumu = "dosya_durumu", "Dosya durumu (UYAP)";
        IslemDurumu = "islem_durumu", "İşlem durumu";
        // Örnek modelde yok; takip aşaması ekimizle birlikte.
        TakipAsamasi = "takip_asamasi", "Takip aşaması";
    }
}

metin_secenek! {
    /// `durum_olayi.kaynak`
    OlayKaynak {
        Hesap = "hesap", "Hesap";
        Uyap = "uyap", "UYAP";
        Elle = "elle", "Elle";
    }
}

// ── Tarih çözme (örnekteki tarih.py) ────────────────────────────────────────────────────────────

/// UYAP / PTT tarih metinlerini tek yerden çözer. Çözülemeyen değer `None` döner; tarih
/// uydurulmaz. Görülen biçimler: "28/09/2026 14:05", "28.09.2026 14:05:09", "20260727",
/// "Mar 13, 2026 11:44:45 AM", "2026-09-28T14:05:09", `{"date":{…},"time":{…}}`.
pub mod tarih {
    use chrono::{NaiveDate, NaiveDateTime};
    use regex::Regex;
    use serde_json::Value;
    use std::sync::OnceLock;

    fn re(hucre: &'static OnceLock<Regex>, desen: &str) -> &'static Regex {
        hucre.get_or_init(|| Regex::new(desen).expect("tarih deseni"))
    }

    fn kur(y: i64, a: i64, g: i64, s: i64, d: i64, sn: i64) -> Option<NaiveDateTime> {
        let u = |v: i64| u32::try_from(v).ok();
        NaiveDate::from_ymd_opt(i32::try_from(y).ok()?, u(a)?, u(g)?)?.and_hms_opt(u(s)?, u(d)?, u(sn)?)
    }

    fn sayi(m: Option<regex::Match>) -> i64 {
        m.and_then(|x| x.as_str().parse().ok()).unwrap_or(0)
    }

    /// Metni yerel (dilimsiz) zamana çevirir.
    pub fn metin_coz(deger: &str) -> Option<NaiveDateTime> {
        static GAY: OnceLock<Regex> = OnceLock::new();
        static ISO: OnceLock<Regex> = OnceLock::new();
        static SIKISIK: OnceLock<Regex> = OnceLock::new();
        static ING: OnceLock<Regex> = OnceLock::new();
        let m_str = deger.trim();
        if m_str.is_empty() || m_str == "0" {
            return None;
        }
        if let Some(m) =
            re(&GAY, r"^(\d{1,2})[./-](\d{1,2})[./-](\d{4})(?:[ T]+(\d{1,2}):(\d{2})(?::(\d{2}))?)?").captures(m_str)
        {
            return kur(sayi(m.get(3)), sayi(m.get(2)), sayi(m.get(1)), sayi(m.get(4)), sayi(m.get(5)), sayi(m.get(6)));
        }
        if let Some(m) = re(&ISO, r"^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2}))?)?").captures(m_str) {
            return kur(sayi(m.get(1)), sayi(m.get(2)), sayi(m.get(3)), sayi(m.get(4)), sayi(m.get(5)), sayi(m.get(6)));
        }
        if let Some(m) = re(&SIKISIK, r"^(\d{4})(\d{2})(\d{2})$").captures(m_str) {
            return kur(sayi(m.get(1)), sayi(m.get(2)), sayi(m.get(3)), 0, 0, 0);
        }
        if let Some(m) = re(
            &ING,
            r"^([A-Za-z]{3})[a-z]*\.?\s+(\d{1,2}),\s*(\d{4})(?:\s+(\d{1,2}):(\d{2})(?::(\d{2}))?\s*([AaPp][Mm])?)?",
        )
        .captures(m_str)
        {
            let ay = match m.get(1)?.as_str().to_ascii_lowercase().as_str() {
                "jan" => 1,
                "feb" => 2,
                "mar" => 3,
                "apr" => 4,
                "may" => 5,
                "jun" => 6,
                "jul" => 7,
                "aug" => 8,
                "sep" => 9,
                "oct" => 10,
                "nov" => 11,
                "dec" => 12,
                _ => return None,
            };
            let mut s = sayi(m.get(4));
            match m.get(7).map(|x| x.as_str().to_ascii_lowercase()).as_deref() {
                Some("pm") if s < 12 => s += 12,
                Some("am") if s == 12 => s = 0,
                _ => {}
            }
            return kur(sayi(m.get(3)), ay, sayi(m.get(2)), s, sayi(m.get(5)), sayi(m.get(6)));
        }
        None
    }

    fn tam(v: Option<&Value>) -> Option<i64> {
        match v? {
            Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// JSON değerini (metin ya da UYAP'ın `{"date":{…},"time":{…}}` nesnesi) çözer.
    pub fn coz(deger: &Value) -> Option<NaiveDateTime> {
        match deger {
            Value::String(s) => metin_coz(s),
            Value::Number(n) => metin_coz(&n.to_string()),
            Value::Object(o) => {
                let t = o.get("date")?.as_object()?;
                let s = o.get("time").and_then(|x| x.as_object());
                let saat = |k: &str| s.and_then(|s| tam(s.get(k))).unwrap_or(0);
                kur(tam(t.get("year"))?, tam(t.get("month"))?, tam(t.get("day"))?, saat("hour"), saat("minute"), saat("second"))
            }
            _ => None,
        }
    }

    /// Yalnız gün.
    pub fn gun_coz(deger: &str) -> Option<NaiveDate> {
        metin_coz(deger).map(|t| t.date())
    }
}

/// Ham tarih metni → türetilmiş gerçek tarih alanı (örnekteki `HamTarihliModel.TARIH_ALANLARI`):
/// (tablo, ham alan, türetilen alan, yalnız gün mü). Ham alan her yazıldığında
/// [`ham_tarihleri_turet`] çağrılır; sorgu, sıralama ve süre hesabı türetilen alanla yapılır.
pub const HAM_TARIH_ALANLARI: &[(&str, &str, &str, bool)] = &[
    ("idari_yargi_detay", "karar_tarihi", "karar_zamani", false),
    ("dosya_safahat", "safahat_tarihi", "safahat_zamani", false),
    ("dosya_tahsilat_harc", "tahsilat_tarihi", "tahsilat_zamani", false),
    ("dosya_reddiyat", "reddiyat_tarihi", "reddiyat_zamani", false),
    ("borclu_sorgu", "sorgu_tarihi", "sorgulama_zamani", false),
    ("tebligat_barkod", "evrak_tarihi", "evrak_gunu", true),
    ("tebligat_barkod", "son_islem_tarihi", "son_islem_zamani", false),
];

/// Bir satırın ham tarih alanlarından gerçek tarih alanlarını yeniden türetir (örnekte
/// `save()` içinde olan iş). Tablonun ham tarih alanı yoksa hata döner.
pub fn ham_tarihleri_turet<C: GenericClient>(c: &mut C, tablo: &str, id: i64) -> Result<(), String> {
    let alanlar: Vec<_> = HAM_TARIH_ALANLARI.iter().filter(|a| a.0 == tablo).collect();
    if alanlar.is_empty() {
        return Err(format!("{tablo}: ham tarih alanı tanımlı değil"));
    }
    for (t, ham, hedef, yalniz_gun) in alanlar {
        let Some(satir) =
            c.query_opt(&format!("SELECT \"{ham}\" FROM \"{t}\" WHERE id = $1"), &[&id]).map_err(hata)?
        else {
            return Ok(());
        };
        let metin: String = satir.get(0);
        let zaman = tarih::metin_coz(&metin);
        if *yalniz_gun {
            let gun = zaman.map(|z| z.date());
            c.execute(&format!("UPDATE \"{t}\" SET \"{hedef}\" = $2 WHERE id = $1"), &[&id, &gun]).map_err(hata)?;
        } else {
            c.execute(&format!("UPDATE \"{t}\" SET \"{hedef}\" = $2::timestamp WHERE id = $1"), &[&id, &zaman])
                .map_err(hata)?;
        }
    }
    Ok(())
}

// ── Yardımcılar ─────────────────────────────────────────────────────────────────────────────────

/// Metni en çok `n` karaktere kırpar (varchar sınırı aşılıp kayıt düşmesin).
pub fn kirp(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn j_metin(rec: &Value, anahtar: &str) -> String {
    match rec.get(anahtar) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn j_tam(rec: &Value, anahtar: &str) -> i64 {
    match rec.get(anahtar) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// "2025/237" → (2025, 237). Yenilenen dosya "2026/5683 - ( 2022/11833 )" → (2026, 5683): UYAP'ın verdiği
/// yeni numara öndedir, eski numara parantezdedir. Ayrıştırılamazsa (0, 0).
///
/// 21.11.2026: bu biçim eskiden (0, 0) oluyordu; dosya (daire, yıl, sıra, tür) ile tanındığı için bir dairedeki
/// bütün yenilenmiş dosyalar tek satırda üst üste yazılıyordu.
pub fn yil_sira(dosya_no: &str) -> (i32, i32) {
    let dosya_no = match dosya_no.split_once('(') {
        Some((on, _)) => on.trim_end().trim_end_matches('-'),
        None => dosya_no,
    };
    let Some((y, s)) = dosya_no.split_once('/') else { return (0, 0) };
    match (y.trim().parse::<i32>(), s.trim().parse::<i32>()) {
        (Ok(y), Ok(s)) if y >= 0 && s >= 0 => (y, s),
        _ => (0, 0),
    }
}

// ── Durum olayı ─────────────────────────────────────────────────────────────────────────────────

/// Durum geçmişine yazılacak olay: eski → yeni değer, günü ve kaynağı.
pub struct YeniOlay<'a> {
    pub dosya: i64,
    pub dosya_taraf: Option<i64>,
    pub tur: OlayTur,
    pub eski: &'a str,
    pub yeni: &'a str,
    /// Olayın günü (tebliğ / kesinleşme günü); kayıt zamanı ayrıca `zaman` alanındadır.
    pub olay_tarihi: Option<NaiveDate>,
    pub kaynak: OlayKaynak,
    pub aciklama: &'a str,
}

/// Tarafın o anki görünen adı (örnekteki `Taraf.__str__`).
pub fn taraf_adi<C: GenericClient>(c: &mut C, dosya_taraf: i64) -> Result<String, String> {
    let Some(s) = c
        .query_opt(
            "SELECT t.id, t.tur, t.ad, t.soyad, t.tckn, t.unvan, t.mersis_no, t.vergi_no
               FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE dt.id = $1",
            &[&dosya_taraf],
        )
        .map_err(hata)?
    else {
        return Ok(String::new());
    };
    let id: i64 = s.get(0);
    let tur: String = s.get(1);
    let (ad, soyad): (String, String) = (s.get(2), s.get(3));
    let tckn: Option<String> = s.get(4);
    let unvan: String = s.get(5);
    let mersis: Option<String> = s.get(6);
    let vergi: String = s.get(7);
    let dolu = |x: String| if x.trim().is_empty() { None } else { Some(x) };
    let yedek = format!("Taraf #{id}");
    Ok(if tur == TarafTur::Tuzel.kod() {
        dolu(unvan).or(mersis.and_then(dolu)).or(dolu(vergi)).unwrap_or(yedek)
    } else {
        dolu(format!("{ad} {soyad}").trim().to_string()).or(tckn.and_then(dolu)).unwrap_or(yedek)
    })
}

/// Durum geçmişine olay yazar; yeni kaydın kimliğini döner.
pub fn durum_olayi_yaz<C: GenericClient>(c: &mut C, o: &YeniOlay) -> Result<i64, String> {
    let ad = match o.dosya_taraf {
        Some(dt) => kirp(&taraf_adi(c, dt)?, 400),
        None => String::new(),
    };
    let s = c
        .query_one(
            "INSERT INTO durum_olayi (dosya_id, dosya_taraf_id, taraf_adi, tur, eski, yeni, olay_tarihi, kaynak, aciklama)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
            &[
                &o.dosya,
                &o.dosya_taraf,
                &ad,
                &o.tur.kod(),
                &kirp(o.eski, 255),
                &kirp(o.yeni, 255),
                &o.olay_tarihi,
                &o.kaynak.kod(),
                &o.aciklama,
            ],
        )
        .map_err(hata)?;
    Ok(s.get(0))
}

// ── Dosya: kapak künyesi ve işlem durumu ────────────────────────────────────────────────────────

const OTOMATIK_PASIF_NOTU: &str = "UYAP'ta dosya kapandı (otomatik)";

/// UYAP `search_phrase_detayli` kaydını (kapak künyesi) Birim + Dosya olarak kaydeder;
/// (dosya kimliği, yeni mi) döner. Örnekteki `ingest.dosya_kunyesi_kaydet` ile aynı kurallar:
///   • kalıcı anahtar (birim, yıl, sıra, tür kodu); UYAP `dosyaId` oturumluktur, yalnız saklanır
///   • `yargi_turu` aramanın yargı türüdür; verilmezse birimin bilinen türü, o da yoksa hata
///   • dosya UYAP'ta Açık → Kapalı'ya geçtiği anda işlem durumu Pasif olur (yalnız geçişte)
///   • UYAP durumu değiştiyse ve otomatik Pasif'e düşüşte durum geçmişine olay yazılır
pub fn dosya_kunyesi_kaydet<C: GenericClient>(
    c: &mut C,
    rec: &Value,
    yargi_turu: Option<i16>,
) -> Result<(i64, bool), String> {
    let birim_no = j_metin(rec, "birimId");
    let dosya_no = j_metin(rec, "dosyaNo");
    if birim_no.trim().is_empty() {
        return Err(format!("Birim numarası olmayan dosya kaydedilemez: {dosya_no}"));
    }
    let mut tx = c.transaction().map_err(hata)?;
    let b = tx
        .query_one(
            "INSERT INTO birim (birim_id, ad, turu1, turu2, turu3, yargi_turu) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (birim_id) DO UPDATE SET ad = EXCLUDED.ad, turu1 = EXCLUDED.turu1, turu2 = EXCLUDED.turu2,
                 turu3 = EXCLUDED.turu3, yargi_turu = COALESCE(EXCLUDED.yargi_turu, birim.yargi_turu),
                 guncelleme = now()
             RETURNING id, yargi_turu",
            &[
                &kirp(&birim_no, 32),
                &kirp(&j_metin(rec, "birimAdi"), 255),
                &kirp(&j_metin(rec, "birimTuru1"), 8),
                &kirp(&j_metin(rec, "birimTuru2"), 8),
                &kirp(&j_metin(rec, "birimTuru3"), 8),
                &yargi_turu,
            ],
        )
        .map_err(hata)?;
    let birim: i64 = b.get(0);
    let Some(yargi_turu) = yargi_turu.or(b.get::<_, Option<i16>>(1)) else {
        return Err(format!(
            "Yargı türü bilinmeden dosya kaydedilemez: {} {dosya_no}",
            j_metin(rec, "birimAdi")
        ));
    };
    let (yil, sira) = yil_sira(&dosya_no);
    let tur_kod = j_tam(rec, "dosyaTurKod") as i32;
    let durum_kod = j_tam(rec, "dosyaDurumKod") as i32;
    let durum = kirp(&j_metin(rec, "dosyaDurum"), 255);
    let uyap_id = kirp(&j_metin(rec, "dosyaId"), 255);
    let tur = kirp(&j_metin(rec, "dosyaTur"), 255);
    let acilis = rec.get("dosyaAcilisTarihi").and_then(tarih::coz);
    let dava_acilmis = rec.get("isDavaDosyasiAcilmisMi").and_then(Value::as_bool).unwrap_or(false);
    let dosya_no = kirp(&dosya_no, 255);

    let eski = tx
        .query_opt(
            "SELECT id, durum_kod, durum, islem_durumu FROM dosya
              WHERE birim_id = $1 AND yil = $2 AND sira_no = $3 AND tur_kod = $4 FOR UPDATE",
            &[&birim, &yil, &sira, &tur_kod],
        )
        .map_err(hata)?;
    let (id, yeni_mi, eski_durum_kod, eski_durum, islem) = match &eski {
        Some(s) => {
            let id: i64 = s.get(0);
            tx.execute(
                "UPDATE dosya SET dosya_id = $2, dosya_no = $3, durum_kod = $4, durum = $5, tur = $6,
                        acilis_tarihi = $7::timestamp, is_dava_dosyasi_acilmis = $8, yargi_turu = $9,
                        son_senkron = now()
                  WHERE id = $1",
                &[&id, &uyap_id, &dosya_no, &durum_kod, &durum, &tur, &acilis, &dava_acilmis, &yargi_turu],
            )
            .map_err(hata)?;
            (id, false, Some(s.get::<_, i32>(1)), s.get::<_, String>(2), s.get::<_, String>(3))
        }
        None => {
            let s = tx
                .query_one(
                    "INSERT INTO dosya (dosya_id, birim_id, yargi_turu, yil, sira_no, dosya_no, durum_kod, durum,
                                        tur_kod, tur, acilis_tarihi, is_dava_dosyasi_acilmis)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::timestamp, $12) RETURNING id",
                    &[
                        &uyap_id,
                        &birim,
                        &yargi_turu,
                        &yil,
                        &sira,
                        &dosya_no,
                        &durum_kod,
                        &durum,
                        &tur_kod,
                        &tur,
                        &acilis,
                        &dava_acilmis,
                    ],
                )
                .map_err(hata)?;
            (s.get(0), true, None, String::new(), IslemDurumu::Aktif.kod().to_string())
        }
    };
    let kapali = DosyaDurum::Kapali.kod();
    let otomatik_pasif = durum_kod == kapali && eski_durum_kod != Some(kapali) && islem != IslemDurumu::Pasif.kod();
    if otomatik_pasif {
        tx.execute(
            "UPDATE dosya SET islem_durumu = $2, islem_durumu_notu = $3, islem_durumu_tarihi = now(),
                    bekleme_bitis_tarihi = NULL WHERE id = $1",
            &[&id, &IslemDurumu::Pasif.kod(), &OTOMATIK_PASIF_NOTU],
        )
        .map_err(hata)?;
    }
    if !yeni_mi && eski_durum != durum {
        durum_olayi_yaz(
            &mut tx,
            &YeniOlay {
                dosya: id,
                dosya_taraf: None,
                tur: OlayTur::DosyaDurumu,
                eski: &eski_durum,
                yeni: &durum,
                olay_tarihi: None,
                kaynak: OlayKaynak::Uyap,
                aciklama: "",
            },
        )?;
    }
    if otomatik_pasif && !yeni_mi {
        durum_olayi_yaz(
            &mut tx,
            &YeniOlay {
                dosya: id,
                dosya_taraf: None,
                tur: OlayTur::IslemDurumu,
                eski: &IslemDurumu::etiket_bul(&islem),
                yeni: IslemDurumu::Pasif.etiket(),
                olay_tarihi: None,
                kaynak: OlayKaynak::Hesap,
                aciklama: OTOMATIK_PASIF_NOTU,
            },
        )?;
    }
    tx.commit().map_err(hata)?;
    Ok((id, yeni_mi))
}

/// Dosyanın işlem durumunu elle değiştirir: tarih şimdi olur, bekleme bitişi yalnız
/// Beklemede'de tutulur; durum değiştiyse geçmişe olay yazılır.
pub fn islem_durumu_ata<C: GenericClient>(
    c: &mut C,
    dosya: i64,
    durum: IslemDurumu,
    notu: &str,
    bekleme_bitis: Option<NaiveDate>,
) -> Result<(), String> {
    let mut tx = c.transaction().map_err(hata)?;
    let Some(s) = tx.query_opt("SELECT islem_durumu FROM dosya WHERE id = $1 FOR UPDATE", &[&dosya]).map_err(hata)?
    else {
        return Err(format!("dosya bulunamadı: {dosya}"));
    };
    let eski: String = s.get(0);
    let bitis = if durum == IslemDurumu::Beklemede { bekleme_bitis } else { None };
    tx.execute(
        "UPDATE dosya SET islem_durumu = $2, islem_durumu_notu = $3, islem_durumu_tarihi = now(),
                bekleme_bitis_tarihi = $4 WHERE id = $1",
        &[&dosya, &durum.kod(), &notu, &bitis],
    )
    .map_err(hata)?;
    if eski != durum.kod() {
        durum_olayi_yaz(
            &mut tx,
            &YeniOlay {
                dosya,
                dosya_taraf: None,
                tur: OlayTur::IslemDurumu,
                eski: &IslemDurumu::etiket_bul(&eski),
                yeni: durum.etiket(),
                olay_tarihi: None,
                kaynak: OlayKaynak::Elle,
                aciklama: notu,
            },
        )?;
    }
    tx.commit().map_err(hata)
}

// ── PTT hareketleri ─────────────────────────────────────────────────────────────────────────────

const YER_ANAHTARLARI: [&str; 5] = ["islem_yeri", "merkez", "sube", "ilce", "il"];

/// PTT ham yanıtının `hareketDongu` listesini `ptt_hareket` satırlarına yazar: o tebligatın eski
/// hareketleri silinip yeniden yazılır. Hatalı / atlanmış yanıtta (`_hata`) hiçbir şeye
/// dokunmaz. Döner: yazılan hareket sayısı.
pub fn ptt_hareketlerini_yaz<C: GenericClient>(c: &mut C, tebligat: i64, ham_yanit: &Value) -> Result<usize, String> {
    let ayristirilmis;
    let ham_yanit = match ham_yanit {
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(v) => {
                ayristirilmis = v;
                &ayristirilmis
            }
            Err(_) => return Ok(0),
        },
        v => v,
    };
    let Some(o) = ham_yanit.as_object() else { return Ok(0) };
    let hata_var = match o.get("_hata") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    };
    if hata_var {
        return Ok(0);
    }
    let hareketler: Vec<&Value> =
        o.get("hareketDongu").and_then(Value::as_array).map(|a| a.iter().filter(|h| h.is_object()).collect()).unwrap_or_default();
    let al = |h: &Value, k: &str| j_metin(h, k).trim().to_string();
    let mut tx = c.transaction().map_err(hata)?;
    tx.execute("DELETE FROM ptt_hareket WHERE tebligat_id = $1", &[&tebligat]).map_err(hata)?;
    for (i, h) in hareketler.iter().enumerate() {
        let zaman = tarih::metin_coz(format!("{} {}", al(h, "tarih"), al(h, "saat")).trim());
        let mut yerler: Vec<String> = Vec::new();
        for k in YER_ANAHTARLARI {
            let y = al(h, k);
            if !y.is_empty() && !yerler.contains(&y) {
                yerler.push(y);
            }
        }
        tx.execute(
            "INSERT INTO ptt_hareket (tebligat_id, sira, zaman, aciklama, yer, islem_detay, ham)
             VALUES ($1, $2, $3::timestamp, $4, $5, $6, $7)",
            &[
                &tebligat,
                &(i as i16),
                &zaman,
                &kirp(&j_metin(h, "aciklama"), 255),
                &kirp(&yerler.join(" / "), 255),
                &kirp(&j_metin(h, "islem_detay"), 255),
                h,
            ],
        )
        .map_err(hata)?;
    }
    tx.commit().map_err(hata)?;
    Ok(hareketler.len())
}

// ── Kesinleşme kuralları ve resmî tatiller ──────────────────────────────────────────────────────

/// Takip türüne göre kesinleşme süresi. Boş kod "hepsi" demektir; üç kodu da boş kural
/// varsayılandır. `kesinlesme_suresi_gun` boşsa o türde kesinleşme yoktur (hesaplanmaz).
/// Kesinleşme süresi itiraz ve ödeme sürelerinden uzun olanıdır (kullanıcı kuralı 20.11.2026);
/// veritabanında `itiraz_suresi_gun` sütunundadır (bkz. sema/0009).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KesinlesmeKurali {
    pub id: i64,
    pub takibin_turu: String,
    pub takibin_yolu: String,
    pub takibin_sekli: String,
    pub ad: String,
    pub kesinlesme_suresi_gun: Option<i16>,
    pub aktif: bool,
    pub aciklama: String,
    /// İtiraz / başvuru süresi; yalnız bilgi.
    pub itiraz_gun: Option<i16>,
    /// Elle işaretlenen itiraz kesinleşmeyi durdurur mu.
    pub itiraz_durdurur: bool,
    pub dayanak: String,
    /// Kullanıcı hazır değeri değiştirdi; açılışta varsayılan yazılmaz.
    pub elle_degisti: bool,
}

/// Programda sabit duran hazır kural (takip türü / yolu / şekli UYAP kodlarıyla).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VarsayilanKural {
    pub turu: &'static str,
    pub yolu: &'static str,
    pub sekli: &'static str,
    pub ad: &'static str,
    /// Kesinleşme süresi; `None`: bu türde kesinleşme yok.
    pub kesinlesme: Option<i16>,
    pub itiraz: Option<i16>,
    pub itiraz_durdurur: bool,
    pub dayanak: &'static str,
}

const fn vk(
    turu: &'static str,
    yolu: &'static str,
    sekli: &'static str,
    ad: &'static str,
    kesinlesme: Option<i16>,
    itiraz: Option<i16>,
    itiraz_durdurur: bool,
    dayanak: &'static str,
) -> VarsayilanKural {
    VarsayilanKural { turu, yolu, sekli, ad, kesinlesme, itiraz, itiraz_durdurur, dayanak }
}

/// Hazır kesinleşme kuralları (20.11.2026; İİK metni ve Yargıtay 12. HD kararlarına göre araştırıldı, kullanıcı
/// onayladı). Kesinleşme = itiraz ve ödeme sürelerinden uzun olanın bitişi, yani icra işleminin yapılabileceği gün.
/// İlamlı takipte icra mahkemesine başvuru (İİK 33) ve kambiyo haciz yolunda itiraz (İİK 169) takibi kendiliğinden
/// durdurmaz. Kodlar UYAP takip açılış listesindendir (`takip_turu_listesi`).
pub const VARSAYILAN_KURALLAR: &[VarsayilanKural] = &[
    vk("", "", "", "Varsayılan (tüm takipler)", Some(7), Some(7), true, ""),
    // İlamlı (0)
    vk("0", "0", "0", "İlamlı · Teslim / tahliye · Örnek 2", Some(7), Some(7), false, "İİK 24, 26, 33"),
    vk("0", "0", "1", "İlamlı · Çocuk teslimi · Örnek 3", None, None, false, "İİK 25 mülga (7343); 5395 s. K. 41/A"),
    vk("0", "1", "0", "İlamlı · Para, teminat, iş, irtifak · Örnek 4-5", Some(7), Some(7), false, "İİK 30–33, 37"),
    vk("0", "1", "1", "İlamlı · İpotek · Örnek 6", Some(30), Some(7), false, "İİK 149, 149/a"),
    vk("0", "2", "", "İlamlı · Diğer (ihtiyati haciz, tedbir, satış)", None, None, false, "İcra emri gönderilmez"),
    vk("0", "3", "0", "İlamlı · Taşınır rehni · Örnek 44", Some(7), Some(7), false, "İİK 150/h, 32, 33"),
    // İlamsız (1)
    vk("1", "0", "0", "İlamsız · Genel haciz · Örnek 7", Some(7), Some(7), true, "İİK 60, 62, 66"),
    vk("1", "1", "0", "İlamsız · İflas, adi · Örnek 11", Some(7), Some(7), true, "İİK 155, 156"),
    vk("1", "1", "1", "İlamsız · Kambiyo, iflas yolu · Örnek 12", Some(5), Some(5), true, "İİK 171–174"),
    vk("1", "2", "0", "İlamsız · İpotek · Örnek 9", Some(30), Some(7), true, "İİK 149/b, 150"),
    vk("1", "2", "1", "İlamsız · Taşınır rehni · Örnek 8", Some(15), Some(7), true, "İİK 146, 147"),
    vk("1", "3", "0", "İlamsız · Kambiyo, haciz yolu · Örnek 10", Some(10), Some(5), false, "İİK 168–170"),
    vk("1", "4", "0", "İlamsız · Tahliye emri · Örnek 14", Some(15), Some(7), true, "İİK 272–274"),
    vk("1", "4", "1", "İlamsız · Kira ödeme emri · Örnek 13", Some(30), Some(7), true, "İİK 269; TBK 315"),
    vk("1", "5", "", "İlamsız · Diğer (aciz belgesi, hapis hakkı vb.)", None, None, true, "Ödeme emri gönderilmez"),
];

/// Takip kodları için hazır kural (tam kod eşleşmesi).
pub fn varsayilan_kural(turu: &str, yolu: &str, sekli: &str) -> Option<&'static VarsayilanKural> {
    VARSAYILAN_KURALLAR.iter().find(|v| v.turu == turu && v.yolu == yolu && v.sekli == sekli)
}

impl VarsayilanKural {
    /// Kayıtlı kuralın değerleri bu hazır kuralla aynı mı (ad dahil).
    pub fn ayni_mi(&self, ad: &str, kesinlesme: Option<i16>, itiraz: Option<i16>, durdurur: bool, dayanak: &str) -> bool {
        self.ad == ad && self.kesinlesme == kesinlesme && self.itiraz == itiraz && self.itiraz_durdurur == durdurur && self.dayanak == dayanak
    }
}

/// Hazır kuralları yazar: olmayanı ekler, kullanıcının değiştirmediği (`elle_degisti` değil) ve değeri farklı olanı
/// günceller; elle değiştirilmiş kurala dokunmaz. Döner: eklenen + güncellenen satır sayısı.
pub fn varsayilan_kurallari_yaz<C: GenericClient>(c: &mut C) -> Result<usize, String> {
    let mut n = 0;
    for v in VARSAYILAN_KURALLAR {
        let mevcut = c
            .query_opt(
                "SELECT id, elle_degisti, ad, itiraz_suresi_gun, itiraz_gun, itiraz_durdurur, dayanak, aktif FROM kesinlesme_kurali
                  WHERE takibin_turu = $1 AND takibin_yolu = $2 AND takibin_sekli = $3",
                &[&v.turu, &v.yolu, &v.sekli],
            )
            .map_err(hata)?;
        match mevcut {
            None => {
                c.execute(
                    "INSERT INTO kesinlesme_kurali (takibin_turu, takibin_yolu, takibin_sekli, ad, itiraz_suresi_gun, itiraz_gun,
                                                    itiraz_durdurur, dayanak, aktif, elle_degisti)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true, false)",
                    &[&v.turu, &v.yolu, &v.sekli, &v.ad, &v.kesinlesme, &v.itiraz, &v.itiraz_durdurur, &v.dayanak],
                )
                .map_err(hata)?;
                n += 1;
            }
            Some(s) => {
                let (id, elle): (i64, bool) = (s.get(0), s.get(1));
                let ayni = v.ayni_mi(&s.get::<_, String>(2), s.get(3), s.get(4), s.get(5), &s.get::<_, String>(6)) && s.get::<_, bool>(7);
                if elle || ayni {
                    continue;
                }
                varsayilana_dondur(c, id, v)?;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// Kuralı hazır değerlerine döndürür (etkin, `elle_degisti` = false).
pub fn varsayilana_dondur<C: GenericClient>(c: &mut C, id: i64, v: &VarsayilanKural) -> Result<(), String> {
    c.execute(
        "UPDATE kesinlesme_kurali SET ad = $2, itiraz_suresi_gun = $3, itiraz_gun = $4, itiraz_durdurur = $5, dayanak = $6,
                aktif = true, elle_degisti = false, guncelleme = now() WHERE id = $1",
        &[&id, &v.ad, &v.kesinlesme, &v.itiraz, &v.itiraz_durdurur, &v.dayanak],
    )
    .map_err(hata)?;
    Ok(())
}

impl KesinlesmeKurali {
    pub fn ozgulluk(&self) -> usize {
        [&self.takibin_turu, &self.takibin_yolu, &self.takibin_sekli].iter().filter(|x| !x.is_empty()).count()
    }

    pub fn eslesir(&self, turu: &str, yolu: &str, sekli: &str) -> bool {
        [(&self.takibin_turu, turu), (&self.takibin_yolu, yolu), (&self.takibin_sekli, sekli)]
            .iter()
            .all(|(k, v)| k.is_empty() || k.as_str() == *v)
    }
}

/// Aktif kurallardan dosyanın takip koduna en özel eşleşeni döner; yoksa `None`. Takip türü
/// bilinmiyorsa (kodlar boş) yalnız varsayılan kural eşleşir. Eşitlikte listedeki ilki seçilir.
pub fn kesinlesme_kurali_sec<'a>(
    kurallar: &'a [KesinlesmeKurali],
    turu: &str,
    yolu: &str,
    sekli: &str,
) -> Option<&'a KesinlesmeKurali> {
    let mut secilen: Option<&KesinlesmeKurali> = None;
    for k in kurallar.iter().filter(|k| k.aktif && k.eslesir(turu, yolu, sekli)) {
        if secilen.map(|s| k.ozgulluk() > s.ozgulluk()).unwrap_or(true) {
            secilen = Some(k);
        }
    }
    secilen
}

/// Bütün kesinleşme kuralları (takip türü, yolu, şekli sırasıyla).
pub fn kesinlesme_kurallari<C: GenericClient>(c: &mut C) -> Result<Vec<KesinlesmeKurali>, String> {
    let satirlar = c
        .query(
            "SELECT id, takibin_turu, takibin_yolu, takibin_sekli, ad, itiraz_suresi_gun, aktif, aciklama,
                    itiraz_gun, itiraz_durdurur, dayanak, elle_degisti
               FROM kesinlesme_kurali ORDER BY takibin_turu, takibin_yolu, takibin_sekli",
            &[],
        )
        .map_err(hata)?;
    Ok(satirlar
        .iter()
        .map(|s| KesinlesmeKurali {
            id: s.get(0),
            takibin_turu: s.get(1),
            takibin_yolu: s.get(2),
            takibin_sekli: s.get(3),
            ad: s.get(4),
            kesinlesme_suresi_gun: s.get(5),
            aktif: s.get(6),
            aciklama: s.get(7),
            itiraz_gun: s.get(8),
            itiraz_durdurur: s.get(9),
            dayanak: s.get(10),
            elle_degisti: s.get(11),
        })
        .collect())
}

/// Resmî / dinî tatil günleri (kesinleşme süre hesabında son günün kayması için).
pub fn resmi_tatiller<C: GenericClient>(c: &mut C) -> Result<BTreeSet<NaiveDate>, String> {
    Ok(c.query("SELECT tarih FROM resmi_tatil", &[]).map_err(hata)?.iter().map(|s| s.get(0)).collect())
}

/// Yerel zamanı okumak için: `SELECT sutun::timestamp` → `NaiveDateTime`.
pub type YerelZaman = NaiveDateTime;

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

/// Şemadaki icra tabloları (0002).
pub const TABLOLAR: &[&str] = &[
    "birim",
    "yargi_birimi",
    "senkron_kapsami",
    "taraf",
    "vekil",
    "dosya",
    "dosya_taraf",
    "dosya_taraf_vekiller",
    "icra_takip_detay",
    "hukuk_dava_detay",
    "idari_yargi_detay",
    "dosya_safahat",
    "dosya_tahsilat_harc",
    "dosya_reddiyat",
    "icra_dosya_hesabi",
    "borclu_sorgu",
    "tebligat_barkod",
    "evrak",
    "kesinlesme_kurali",
    "resmi_tatil",
    "ptt_hareket",
    "durum_olayi",
    "tebligat_tur_eslesmesi",
    "tebligat_tur_kategorisi",
];

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use serde_json::json;
    let say = |c: &mut postgres::Client, sql: &str| -> i64 { c.query_one(sql, &[]).unwrap().get(0) };

    // Şema: bütün tablolar var, ekler yerinde
    for t in TABLOLAR {
        let n: i64 = c
            .query_one("SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name = $1", &[t])
            .unwrap()
            .get(0);
        assert_eq!(n, 1, "tablo eksik: {t}");
    }
    for (t, ham, hedef, _) in HAM_TARIH_ALANLARI {
        let n: i64 = c
            .query_one(
                "SELECT count(*) FROM information_schema.columns WHERE table_name = $1 AND column_name IN ($2, $3)",
                &[t, ham, hedef],
            )
            .unwrap()
            .get(0);
        assert_eq!(n, 2, "ham tarih alanı eksik: {t}.{ham}/{hedef}");
    }
    assert_eq!(
        say(c, "SELECT count(*) FROM information_schema.columns WHERE table_name = 'dosya_taraf' AND column_name LIKE 'takip_asamasi%'"),
        2
    );

    // Kapak künyesi: yeni kayıt
    let mut rec = json!({
        "dosyaId": "oturum-1", "dosyaNo": "2025/237", "dosyaDurumKod": 0, "dosyaDurum": "Açık",
        "dosyaTurKod": 35, "dosyaTur": "İcra Dosyası",
        "dosyaAcilisTarihi": {"date": {"year": 2025, "month": 1, "day": 11}, "time": {"hour": 17, "minute": 3, "second": 9, "nano": 0}},
        "birimAdi": "İzmir 1. İcra Dairesi", "birimId": 1000428, "birimTuru1": "11", "birimTuru2": "1101", "birimTuru3": "1199"
    });
    assert!(dosya_kunyesi_kaydet(c, &rec, None).is_err(), "yargı türü bilinmeden kaydedilmez");
    assert_eq!(say(c, "SELECT count(*) FROM birim"), 0, "hata hiçbir şey bırakmaz");
    let (id, yeni) = dosya_kunyesi_kaydet(c, &rec, Some(YargiTuru::Icra.kod())).unwrap();
    assert!(yeni);
    let s = c.query_one("SELECT yil, sira_no, acilis_tarihi::timestamp, islem_durumu, yargi_turu FROM dosya WHERE id = $1", &[&id]).unwrap();
    assert_eq!((s.get::<_, i32>(0), s.get::<_, i32>(1)), (2025, 237));
    assert_eq!(s.get::<_, NaiveDateTime>(2), NaiveDate::from_ymd_opt(2025, 1, 11).unwrap().and_hms_opt(17, 3, 9).unwrap());
    assert_eq!(s.get::<_, String>(3), "aktif");
    assert_eq!(s.get::<_, i16>(4), 2);

    // Aynı dosya yeni oturum kimliğiyle: güncellenir, çoğalmaz; yargı türü birimden gelir
    rec["dosyaId"] = json!("oturum-2");
    rec["dosyaDurum"] = json!("Açık (Durdurulmuş : Takibe İtiraz)");
    let (id2, yeni2) = dosya_kunyesi_kaydet(c, &rec, None).unwrap();
    assert_eq!((id2, yeni2), (id, false));
    assert_eq!(say(c, "SELECT count(*) FROM dosya"), 1);
    let o = c.query_one("SELECT tur, eski, yeni, kaynak FROM durum_olayi ORDER BY id DESC LIMIT 1", &[]).unwrap();
    assert_eq!(o.get::<_, String>(0), "dosya_durumu");
    assert_eq!((o.get::<_, String>(1).as_str(), o.get::<_, String>(3).as_str()), ("Açık", "uyap"));
    // Değişiklik yoksa olay yazılmaz
    dosya_kunyesi_kaydet(c, &rec, None).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi"), 1);

    // Açık → Kapalı: otomatik Pasif + iki olay
    rec["dosyaDurumKod"] = json!(1);
    rec["dosyaDurum"] = json!("Kapalı");
    dosya_kunyesi_kaydet(c, &rec, None).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM dosya WHERE islem_durumu = 'pasif' AND islem_durumu_tarihi IS NOT NULL"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi"), 3);
    let o = c.query_one("SELECT eski, yeni, kaynak FROM durum_olayi WHERE tur = 'islem_durumu'", &[]).unwrap();
    assert_eq!(o.get::<_, String>(0), "Aktif - işlem yapılabilir");
    assert_eq!(o.get::<_, String>(1), "Pasif - işlem yapma");
    assert_eq!(o.get::<_, String>(2), "hesap");
    // Elle Aktif'e alınan kapalı dosya sonraki senkronda yeniden ezilmez
    islem_durumu_ata(c, id, IslemDurumu::Beklemede, "müvekkil bekleniyor", NaiveDate::from_ymd_opt(2026, 12, 1)).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM dosya WHERE bekleme_bitis_tarihi = '2026-12-01'"), 1);
    islem_durumu_ata(c, id, IslemDurumu::Aktif, "", NaiveDate::from_ymd_opt(2026, 12, 1)).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM dosya WHERE bekleme_bitis_tarihi IS NULL AND islem_durumu = 'aktif'"), 1);
    dosya_kunyesi_kaydet(c, &rec, None).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM dosya WHERE islem_durumu = 'aktif'"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi WHERE kaynak = 'elle'"), 2);

    // Taraf, bağ, tebligat, ham tarih türetme
    let taraf: i64 = c
        .query_one("INSERT INTO taraf (tur, ad, soyad) VALUES ('gercek', 'AHMET', 'YILMAZ') RETURNING id", &[])
        .unwrap()
        .get(0);
    let bag: i64 = c
        .query_one("INSERT INTO dosya_taraf (dosya_id, taraf_id, rol, sira) VALUES ($1, $2, 'borclu', 1) RETURNING id", &[&id, &taraf])
        .unwrap()
        .get(0);
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf WHERE takip_asamasi = '' AND kesinlesme_durumu = ''"), 1);
    let teb: i64 = c
        .query_one(
            "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, borclu_id, evrak_tarihi, son_islem_tarihi, barkod)
             VALUES ($1, 10, $2, '01/10/2026', '05/10/2026 14:30', '4007663523293') RETURNING id",
            &[&id, &bag],
        )
        .unwrap()
        .get(0);
    ham_tarihleri_turet(c, "tebligat_barkod", teb).unwrap();
    let s = c.query_one("SELECT evrak_gunu, son_islem_zamani::timestamp FROM tebligat_barkod WHERE id = $1", &[&teb]).unwrap();
    assert_eq!(s.get::<_, Option<NaiveDate>>(0), NaiveDate::from_ymd_opt(2026, 10, 1));
    assert_eq!(s.get::<_, Option<NaiveDateTime>>(1), NaiveDate::from_ymd_opt(2026, 10, 5).unwrap().and_hms_opt(14, 30, 0));
    c.execute("UPDATE tebligat_barkod SET son_islem_tarihi = 'bozuk' WHERE id = $1", &[&teb]).unwrap();
    ham_tarihleri_turet(c, "tebligat_barkod", teb).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE son_islem_zamani IS NULL AND evrak_gunu IS NOT NULL"), 1);
    assert!(ham_tarihleri_turet(c, "dosya", id).is_err());

    // PTT hareketleri
    let ham = json!({"hareketDongu": [
        {"tarih": "02/10/2026", "saat": "09:15", "aciklama": "KABUL EDİLDİ", "il": "İZMİR", "ilce": "KONAK", "merkez": "KONAK"},
        {"tarih": "20261005", "saat": "", "aciklama": "TESLİM EDİLDİ", "islem_detay": "BİZZAT", "il": "İZMİR"},
        "bozuk"
    ]});
    assert_eq!(ptt_hareketlerini_yaz(c, teb, &ham).unwrap(), 2);
    assert_eq!(ptt_hareketlerini_yaz(c, teb, &json!(ham.to_string())).unwrap(), 2, "metin olarak da okunur");
    assert_eq!(ptt_hareketlerini_yaz(c, teb, &json!({"_hata": "zaman aşımı"})).unwrap(), 0);
    assert_eq!(say(c, "SELECT count(*) FROM ptt_hareket"), 2, "hatalı yanıt eski hareketleri silmez");
    let s = c.query_one("SELECT zaman::timestamp, yer FROM ptt_hareket WHERE sira = 0", &[]).unwrap();
    assert_eq!(s.get::<_, Option<NaiveDateTime>>(0), NaiveDate::from_ymd_opt(2026, 10, 2).unwrap().and_hms_opt(9, 15, 0));
    assert_eq!(s.get::<_, String>(1), "KONAK / İZMİR");

    // Durum olayı: taraf adı o anki adıyla saklanır
    durum_olayi_yaz(
        c,
        &YeniOlay {
            dosya: id,
            dosya_taraf: Some(bag),
            tur: OlayTur::Tebligat,
            eski: "",
            yeni: TebligatDurumu::TebligEdildi.etiket(),
            olay_tarihi: NaiveDate::from_ymd_opt(2026, 10, 5),
            kaynak: OlayKaynak::Hesap,
            aciklama: "",
        },
    )
    .unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi WHERE taraf_adi = 'AHMET YILMAZ' AND olay_tarihi = '2026-10-05'"), 1);

    // Kurallar ve tatiller
    c.batch_execute(
        "INSERT INTO kesinlesme_kurali (takibin_turu, takibin_yolu, itiraz_suresi_gun, ad) VALUES
            ('', '', 7, 'Varsayılan'), ('1', '', NULL, 'İlamlı'), ('0', '5', 5, 'Kambiyo');
         INSERT INTO resmi_tatil (tarih, aciklama) VALUES ('2026-10-29', 'Cumhuriyet Bayramı');",
    )
    .unwrap();
    let kurallar = kesinlesme_kurallari(c).unwrap();
    assert_eq!(kurallar.len(), 3);
    assert_eq!(kesinlesme_kurali_sec(&kurallar, "0", "5", "9").unwrap().kesinlesme_suresi_gun, Some(5));
    assert_eq!(kesinlesme_kurali_sec(&kurallar, "1", "2", "").unwrap().kesinlesme_suresi_gun, None);
    assert!(kurallar.iter().all(|k| k.itiraz_durdurur && !k.elle_degisti), "yeni alanların varsayılanı");
    assert_eq!(kesinlesme_kurali_sec(&kurallar, "", "", "").unwrap().ad, "Varsayılan");
    assert!(resmi_tatiller(c).unwrap().contains(&NaiveDate::from_ymd_opt(2026, 10, 29).unwrap()));

    // Silme davranışı: taraf bağlıyken silinemez; bağ silinince tebligat kalır (borçlu boşalır),
    // dosya silinince altındaki her şey gider
    assert!(c.execute("DELETE FROM taraf WHERE id = $1", &[&taraf]).is_err(), "bağlı taraf korunur");
    c.execute("DELETE FROM dosya_taraf WHERE id = $1", &[&bag]).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE borclu_id IS NULL"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi WHERE dosya_taraf_id IS NULL AND taraf_adi = 'AHMET YILMAZ'"), 1);
    c.execute("DELETE FROM dosya WHERE id = $1", &[&id]).unwrap();
    for t in ["tebligat_barkod", "ptt_hareket", "durum_olayi", "dosya_taraf"] {
        assert_eq!(say(c, &format!("SELECT count(*) FROM {t}")), 0, "{t} dosyayla birlikte silinmeli");
    }
    assert_eq!(say(c, "SELECT count(*) FROM taraf"), 1);

    // Hazır kurallar: hepsi eklenir; ikinci çağrı bir şey yapmaz; elle değişene dokunulmaz; elle değişmemiş ama
    // farklı olan varsayılana döner; kodu varsayılandaki kural (ör. eski "Varsayılan") güncellenir.
    c.batch_execute("DELETE FROM kesinlesme_kurali; INSERT INTO kesinlesme_kurali (itiraz_suresi_gun, ad) VALUES (7, 'Varsayılan')").unwrap();
    assert_eq!(varsayilan_kurallari_yaz(c).unwrap(), VARSAYILAN_KURALLAR.len());
    assert_eq!(varsayilan_kurallari_yaz(c).unwrap(), 0);
    assert_eq!(say(c, "SELECT count(*) FROM kesinlesme_kurali"), VARSAYILAN_KURALLAR.len() as i64);
    let kurallar = kesinlesme_kurallari(c).unwrap();
    let kambiyo = kesinlesme_kurali_sec(&kurallar, "1", "3", "0").unwrap();
    assert_eq!((kambiyo.kesinlesme_suresi_gun, kambiyo.itiraz_gun, kambiyo.itiraz_durdurur), (Some(10), Some(5), false));
    assert_eq!(kesinlesme_kurali_sec(&kurallar, "1", "4", "1").unwrap().kesinlesme_suresi_gun, Some(30), "kira");
    assert_eq!(kesinlesme_kurali_sec(&kurallar, "0", "2", "7").unwrap().kesinlesme_suresi_gun, None, "ilamlı diğer: kesinleşme yok");
    assert_eq!(kesinlesme_kurali_sec(&kurallar, "", "", "").unwrap().ad, "Varsayılan (tüm takipler)");
    c.batch_execute(
        "UPDATE kesinlesme_kurali SET itiraz_suresi_gun = 8, elle_degisti = true WHERE takibin_turu = '1' AND takibin_yolu = '0';
         UPDATE kesinlesme_kurali SET itiraz_suresi_gun = 9 WHERE takibin_turu = '1' AND takibin_yolu = '3';
         UPDATE kesinlesme_kurali SET aktif = false WHERE takibin_turu = '0' AND takibin_yolu = '3';",
    )
    .unwrap();
    assert_eq!(varsayilan_kurallari_yaz(c).unwrap(), 2);
    let kurallar = kesinlesme_kurallari(c).unwrap();
    assert_eq!(kesinlesme_kurali_sec(&kurallar, "1", "0", "0").unwrap().kesinlesme_suresi_gun, Some(8), "elle değişen korunur");
    assert_eq!(kesinlesme_kurali_sec(&kurallar, "1", "3", "0").unwrap().kesinlesme_suresi_gun, Some(10));
    assert!(kesinlesme_kurali_sec(&kurallar, "0", "3", "0").unwrap().aktif);
    c.batch_execute("DELETE FROM taraf; DELETE FROM birim; DELETE FROM kesinlesme_kurali; DELETE FROM resmi_tatil").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn z(y: i32, a: u32, g: u32, s: u32, d: u32, sn: u32) -> Option<NaiveDateTime> {
        NaiveDate::from_ymd_opt(y, a, g).unwrap().and_hms_opt(s, d, sn)
    }

    #[test]
    fn tarih_bicimleri() {
        assert_eq!(tarih::metin_coz("28/09/2026"), z(2026, 9, 28, 0, 0, 0));
        assert_eq!(tarih::metin_coz("28/09/2026 14:05"), z(2026, 9, 28, 14, 5, 0));
        assert_eq!(tarih::metin_coz(" 28.09.2026 14:05:09 "), z(2026, 9, 28, 14, 5, 9));
        assert_eq!(tarih::metin_coz("5-1-2026"), z(2026, 1, 5, 0, 0, 0));
        assert_eq!(tarih::metin_coz("20260727"), z(2026, 7, 27, 0, 0, 0));
        assert_eq!(tarih::metin_coz("2026-09-28"), z(2026, 9, 28, 0, 0, 0));
        assert_eq!(tarih::metin_coz("2026-09-28T14:05:09"), z(2026, 9, 28, 14, 5, 9));
        assert_eq!(tarih::metin_coz("Mar 13, 2026 11:44:45 AM"), z(2026, 3, 13, 11, 44, 45));
        assert_eq!(tarih::metin_coz("Apr 9, 2026 11:58:19 PM"), z(2026, 4, 9, 23, 58, 19));
        assert_eq!(tarih::metin_coz("Mar 18, 2026 12:27:44 AM"), z(2026, 3, 18, 0, 27, 44));
        assert_eq!(tarih::metin_coz("Mar 18, 2026 12:27:44 PM"), z(2026, 3, 18, 12, 27, 44));
        assert_eq!(tarih::metin_coz("September 5, 2026"), z(2026, 9, 5, 0, 0, 0));
        for bozuk in ["", "0", "bozuk", "31/02/2026", "2026-13-01", "Xyz 1, 2026", "202607271", "28/09/26"] {
            assert_eq!(tarih::metin_coz(bozuk), None, "{bozuk}");
        }
        assert_eq!(tarih::gun_coz("05/10/2026 14:30"), NaiveDate::from_ymd_opt(2026, 10, 5));
    }

    #[test]
    fn tarih_json() {
        let v = json!({"date": {"year": 2025, "month": 9, "day": 11}, "time": {"hour": 17, "minute": 3, "second": 9, "nano": 0}});
        assert_eq!(tarih::coz(&v), z(2025, 9, 11, 17, 3, 9));
        assert_eq!(tarih::coz(&json!({"date": {"year": 2025, "month": 9, "day": 11}})), z(2025, 9, 11, 0, 0, 0));
        assert_eq!(tarih::coz(&json!({"date": {"year": 2025, "month": 9}})), None);
        assert_eq!(tarih::coz(&json!({"time": {"hour": 1}})), None);
        assert_eq!(tarih::coz(&json!("28/09/2026")), z(2026, 9, 28, 0, 0, 0));
        assert_eq!(tarih::coz(&json!(20260727)), z(2026, 7, 27, 0, 0, 0));
        assert_eq!(tarih::coz(&Value::Null), None);
    }

    #[test]
    fn dosya_no_ayristirma() {
        assert_eq!(yil_sira("2025/237"), (2025, 237));
        assert_eq!(yil_sira("2025/237/1"), (0, 0));
        assert_eq!(yil_sira("2025"), (0, 0));
        assert_eq!(yil_sira(""), (0, 0));
        assert_eq!(yil_sira("-1/5"), (0, 0));
        // Yenilenen dosya: yeni numara öndedir
        assert_eq!(yil_sira("2026/5683 - ( 2022/11833 )"), (2026, 5683));
        assert_eq!(yil_sira("2022/8742 - ( 2014/10733 )"), (2022, 8742));
        assert_eq!(yil_sira("2022/8742-(2014/10733)"), (2022, 8742));
        assert_eq!(yil_sira("( 2014/10733 )"), (0, 0));
    }

    #[test]
    fn secenek_kodlari_benzersiz_ve_sutuna_sigar() {
        fn denetle(ad: &str, kodlar: Vec<&str>, sinir: usize) {
            let tekil: BTreeSet<_> = kodlar.iter().collect();
            assert_eq!(tekil.len(), kodlar.len(), "{ad}: yinelenen kod");
            for k in kodlar {
                assert!(k.chars().count() <= sinir, "{ad}: '{k}' {sinir} karakteri aşıyor");
            }
        }
        denetle("islem_durumu", IslemDurumu::HEPSI.iter().map(|s| s.kod()).collect(), 12);
        denetle("taraf.tur", TarafTur::HEPSI.iter().map(|s| s.kod()).collect(), 8);
        denetle("rol", Rol::HEPSI.iter().map(|s| s.kod()).collect(), 255);
        denetle("kesinlesme_durumu", KesinlesmeDurumu::HEPSI.iter().map(|s| s.kod()).collect(), 30);
        denetle("tebligat_durumu", TebligatDurumu::HEPSI.iter().map(|s| s.kod()).collect(), 30);
        denetle("takip_asamasi", TakipAsamasi::HEPSI.iter().map(|s| s.kod()).collect(), 20);
        denetle("tahsilat.tur", TahsilatTur::HEPSI.iter().map(|s| s.kod()).collect(), 10);
        denetle("olay.tur", OlayTur::HEPSI.iter().map(|s| s.kod()).collect(), 20);
        denetle("olay.kaynak", OlayKaynak::HEPSI.iter().map(|s| s.kod()).collect(), 8);
        assert_eq!(YargiTuru::coz(2), Some(YargiTuru::Icra));
        assert_eq!(DosyaDurum::coz(1), Some(DosyaDurum::Kapali));
        assert_eq!(DosyaTur::coz(35).map(|t| t.etiket()), Some("İcra Dosyası"));
        assert_eq!(IslemDurumu::etiket_bul("pasif"), "Pasif - işlem yapma");
        assert_eq!(IslemDurumu::etiket_bul("bilinmeyen"), "bilinmeyen");
        assert_eq!(Rol::coz("ucuncu sahis"), Some(Rol::UcuncuSahis));
    }

    #[test]
    fn sema_tablolari_ve_ham_tarih_alanlari_semada() {
        let sql = include_str!("sema/0002_icra.sql");
        for t in TABLOLAR {
            assert!(sql.contains(&format!("CREATE TABLE \"{t}\" (")), "şemada yok: {t}");
        }
        assert_eq!(sql.matches("CREATE TABLE ").count(), TABLOLAR.len());
        for (t, ham, hedef, _) in HAM_TARIH_ALANLARI {
            assert!(TABLOLAR.contains(t));
            assert!(sql.contains(&format!("\"{ham}\" varchar(40)")), "{t}.{ham}");
            assert!(sql.contains(&format!("\"{hedef}\" ")), "{t}.{hedef}");
        }
        assert!(!sql.contains("icra_models_"));
    }

    #[test]
    fn varsayilan_kurallar_tutarli() {
        // Kodlar tekrar etmez; kesinleşme itiraz süresinden kısa olamaz (kesinleşme = uzun olanın bitişi);
        // kademe kuralı: yolsuz şekil, türsüz yol yok.
        let mut gorulen = std::collections::HashSet::new();
        for v in VARSAYILAN_KURALLAR {
            assert!(gorulen.insert((v.turu, v.yolu, v.sekli)), "tekrar: {}", v.ad);
            if let (Some(k), Some(i)) = (v.kesinlesme, v.itiraz) {
                assert!(k >= i, "{}", v.ad);
            }
            assert!(!(v.turu.is_empty() && !v.yolu.is_empty()) && !(v.yolu.is_empty() && !v.sekli.is_empty()), "{}", v.ad);
            assert!(v.ad.chars().count() <= 160 && v.dayanak.chars().count() <= 200);
        }
        assert_eq!(varsayilan_kural("1", "3", "0").map(|v| (v.kesinlesme, v.itiraz_durdurur)), Some((Some(10), false)));
        assert_eq!(varsayilan_kural("1", "4", "1").and_then(|v| v.kesinlesme), Some(30));
        assert!(varsayilan_kural("", "", "").is_some());
        assert!(varsayilan_kural("9", "", "").is_none());
    }

    #[test]
    fn kural_secimi() {
        let k = |id, t: &str, y: &str, s: &str, gun, aktif| KesinlesmeKurali {
            id,
            takibin_turu: t.into(),
            takibin_yolu: y.into(),
            takibin_sekli: s.into(),
            ad: String::new(),
            kesinlesme_suresi_gun: gun,
            aktif,
            aciklama: String::new(),
            itiraz_gun: None,
            itiraz_durdurur: true,
            dayanak: String::new(),
            elle_degisti: false,
        };
        let kurallar = vec![
            k(1, "", "", "", Some(7), true),
            k(2, "0", "", "", Some(7), true),
            k(3, "0", "5", "", Some(5), true),
            k(4, "0", "5", "2", Some(3), false),
            k(5, "1", "", "", None, true),
        ];
        let sec = |t, y, s| kesinlesme_kurali_sec(&kurallar, t, y, s).map(|k| k.id);
        assert_eq!(sec("0", "5", "2"), Some(3), "pasif kural seçilmez");
        assert_eq!(sec("0", "1", "2"), Some(2));
        assert_eq!(sec("1", "1", "1"), Some(5));
        assert_eq!(sec("9", "", ""), Some(1));
        assert_eq!(sec("", "", ""), Some(1), "takip türü bilinmeyende yalnız varsayılan");
        assert_eq!(kesinlesme_kurali_sec(&kurallar[1..], "9", "", ""), None);
        assert_eq!(kurallar[2].ozgulluk(), 2);
    }
}
