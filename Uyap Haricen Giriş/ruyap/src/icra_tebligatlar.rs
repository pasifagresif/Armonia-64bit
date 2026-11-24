//! İcra Takip Süreçleri → "Tebligatlar" bölümü ve "Tür eşleştir" (19.11.2026, plan
//! `docs/claude-notlar/planlar/takip-turu-ve-tebligatlar-bolumu.md` C+D; Python `UyapMasa/modules/barkod_sorgu_ekrani.py`).
//!
//! Her tebligat (barkod) bir satırdır. Borçluya bağlanmayan zarflar da (banka, işveren, vekil, üçüncü kişi) muhatap
//! adı ve türüyle listededir. Her sütunda başlık süzgeci (yazı: içinde geçen; `=`: listeden seçilen; `!`: listedekiler
//! hariç) ve sıralama vardır. Sıralama DOSYALARIN sırasını belirler: bir dosyanın zarfları bölünmez, kendi içinde
//! eskiden yeniye kalır (Python ile aynı, kullanıcı isteği 24.09.2026).
//!
//! Uçlar (`/__cu__/veri/…`, `icra_api::dagit`):
//!   GET  tebligatlar                 ?<süzgeçler>&sirala=&yon=artan|azalan&sayfa=&boyut= — satırlar + adetli seçenekler
//!   GET  tebligat_dosya_kimlikleri   ?<süzgeçler> — süzgeçteki bütün tebligatların dosyaları (seçim için)
//!   GET  tebligatlar_excel           ?<süzgeçler>&sirala=&yon= — .xlsx (süzülmüş liste, sıralı, her tebligat bir satır)
//!   GET  tur_eslestirme              şablon dışı tebligat türü yazılışları (gruplu) + tür seçenekleri
//!   POST tur_eslestir                {eslesmeler: [{anahtar, tur}]} — kaydet, mevcut kayıtlara uygula, yeniden hesapla

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::NaiveDate;
use postgres::GenericClient;
use serde_json::{json, Value};

use crate::icra_api::{haric_listesi, metin_uyar, secenek_sinirli, KES_TALIMAT, SECENEK_SINIRI};
use crate::icra_durum as durum;
use crate::icra_tebligat::{self as teb, MuhatapTuru};
use crate::icra_veri::{self as veri, KesinlesmeDurumu};

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

// ── Süzgeç ──────────────────────────────────────────────────────────────────────────────────────

/// Başlık süzgeci olan sütunlar. `kodlu`: değer sabit bir koddur (yalnız listeden seçilir).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sutun {
    Birim,
    DosyaNo,
    DosyaTur,
    Takip,
    Alacakli,
    Borclu,
    Muhatap,
    Barkod,
    Tur,
    Teblig,
    Teslim,
    Mazbata,
    Yeniden,
    Elektronik,
    Kesinlesme,
    /// Avukatın dosyadaki rolü (Dosya Listesi ile aynı hesap): alacaklı / borçlu / 3. kişi vekili …
    Rol,
    /// Tebligat grubu (bir bakışta sayım).
    Grup,
    /// Haciz ihbarnamesi sırası: "1" / "2" / "3", bilinmiyorsa "?", haciz ihbarnamesi değilse "".
    Haciz,
}

impl Sutun {
    pub const HEPSI: [Sutun; 18] = [
        Sutun::Grup,
        Sutun::Haciz,
        Sutun::Birim,
        Sutun::DosyaNo,
        Sutun::DosyaTur,
        Sutun::Rol,
        Sutun::Takip,
        Sutun::Alacakli,
        Sutun::Borclu,
        Sutun::Muhatap,
        Sutun::Barkod,
        Sutun::Tur,
        Sutun::Teblig,
        Sutun::Teslim,
        Sutun::Mazbata,
        Sutun::Yeniden,
        Sutun::Elektronik,
        Sutun::Kesinlesme,
    ];
    pub fn anahtar(self) -> &'static str {
        match self {
            Sutun::Birim => "birim",
            Sutun::DosyaNo => "dosya_no",
            Sutun::DosyaTur => "dosya_tur",
            Sutun::Rol => "rol",
            Sutun::Takip => "takip",
            Sutun::Alacakli => "alacakli",
            Sutun::Borclu => "borclu",
            Sutun::Muhatap => "muhatap",
            Sutun::Barkod => "barkod",
            Sutun::Tur => "tur",
            Sutun::Teblig => "teblig",
            Sutun::Teslim => "teslim",
            Sutun::Mazbata => "mazbata",
            Sutun::Yeniden => "yeniden",
            Sutun::Elektronik => "elektronik",
            Sutun::Kesinlesme => "kesinlesme",
            Sutun::Grup => "grup",
            Sutun::Haciz => "haciz",
        }
    }
    fn kodlu(self) -> bool {
        matches!(
            self,
            Sutun::DosyaTur | Sutun::Rol | Sutun::Muhatap | Sutun::Mazbata | Sutun::Yeniden | Sutun::Elektronik | Sutun::Kesinlesme | Sutun::Grup | Sutun::Haciz
        )
    }
    /// Seçenek listesi uzun olabilir (en çok `SECENEK_SINIRI`).
    fn sinirli(self) -> bool {
        matches!(self, Sutun::Birim | Sutun::DosyaNo | Sutun::Alacakli | Sutun::Borclu | Sutun::Barkod | Sutun::Teslim | Sutun::Tur)
    }
    /// Seçeneğin ekranda görünen adı.
    fn etiket(self, deger: &str) -> String {
        match (self, deger) {
            (Sutun::Muhatap, d) => muhatap_etiketi(d).to_string(),
            (Sutun::Rol, d) => crate::icra_dosyalar::rol_etiketi(d).to_string(),
            (Sutun::Mazbata, "var") => "Döndü".into(),
            (Sutun::Mazbata, "etebligat") => "Gerekmez (e-tebligat)".into(),
            (Sutun::Mazbata, _) => "Dönmedi".into(),
            (Sutun::Yeniden, "evet") => "Talep edildi".into(),
            (Sutun::Yeniden, _) => "Talep yok".into(),
            (Sutun::Elektronik, "evet") => "E-tebligat".into(),
            (Sutun::Elektronik, _) => "Fiziki (PTT)".into(),
            (Sutun::Kesinlesme, d) => kesinlesme_etiketi(d),
            (Sutun::Haciz, "?") => "89 sırası bilinmiyor".into(),
            (Sutun::Haciz, "") => "(haciz ihbarnamesi değil)".into(),
            (Sutun::Haciz, d) => format!("89/{d}"),
            (Sutun::Tur, "") => "(türü yok)".into(),
            (Sutun::Teblig, "") => "Henüz sorgulanmadı".into(),
            (Sutun::Teslim, "") => "(teslim / iade bilgisi yok)".into(),
            (Sutun::Takip, "") => "(takip türü okunmadı)".into(),
            (Sutun::Borclu, "") => "(muhatap belirsiz)".into(),
            (_, "") => "(boş)".into(),
            (_, d) => d.to_string(),
        }
    }
}

pub fn muhatap_etiketi(kod: &str) -> &'static str {
    match kod {
        "borclu" => "Borçlu",
        "vekil" => "Borçlu vekili",
        "temsilci" => "Borçlunun kanuni temsilcisi (vasi, veli, kayyum …)",
        "ucuncu" => "Üçüncü kişi (banka, işveren …)",
        _ => "Belirtilmemiş",
    }
}

/// Tebligatın kesinleşme kategorisi (süzgeç değeri).
pub const KES_YOK: &str = "yok";
pub const KES_BAGSIZ: &str = "bagsiz";
pub const KES_HESAPSIZ: &str = "-";

fn kesinlesme_etiketi(kod: &str) -> String {
    match kod {
        KES_HESAPSIZ => "Henüz hesaplanmadı".into(),
        KES_YOK => "Bu türde kesinleşme yok".into(),
        KES_BAGSIZ => "Borçluya ait değil (üçüncü kişi / bağlanmamış)".into(),
        KES_TALIMAT => "Talimat dosyası (kesinleşme yok)".into(),
        k => KesinlesmeDurumu::etiket_bul(k),
    }
}

/// Liste süzgeçleri.
#[derive(Debug, Default, Clone)]
pub struct Filtre {
    /// Dosya no, daire, borçlu / muhatap, alacaklı, barkod içinde aranır.
    pub q: String,
    /// Dosya listesi (`icra_listeler`): yalnız bu listedeki dosyaların tebligatları; None = hepsi.
    pub liste: Option<i64>,
    /// "acik" | "kapali" | "" (hepsi)
    pub durum: String,
    pub acilis: (Option<NaiveDate>, Option<NaiveDate>),
    pub evrak: (Option<NaiveDate>, Option<NaiveDate>),
    /// PTT son işlem (tebliğ / iade) günü.
    pub islem: (Option<NaiveDate>, Option<NaiveDate>),
    /// Sütun süzgeçleri (anahtar → değer).
    pub sutun: HashMap<&'static str, String>,
    pub sirala: String,
    pub azalan: bool,
    pub sayfa: usize,
    pub boyut: usize,
}

impl Filtre {
    pub fn oku(q: &HashMap<String, String>) -> Filtre {
        let gun = |a: &str| q.get(a).and_then(|v| veri::tarih::gun_coz(v));
        let mut sutun = HashMap::new();
        for s in Sutun::HEPSI {
            if let Some(v) = q.get(s.anahtar()).filter(|v| !v.is_empty()) {
                sutun.insert(s.anahtar(), v.clone());
            }
        }
        Filtre {
            q: q.get("q").cloned().unwrap_or_default(),
            durum: q.get("durum").cloned().unwrap_or_default(),
            acilis: (gun("acilis_bas"), gun("acilis_son")),
            evrak: (gun("evrak_bas"), gun("evrak_son")),
            islem: (gun("islem_bas"), gun("islem_son")),
            sutun,
            liste: crate::icra_listeler::liste_oku(q),
            sirala: q.get("sirala").cloned().unwrap_or_default(),
            azalan: q.get("yon").is_some_and(|y| y == "azalan"),
            sayfa: q.get("sayfa").and_then(|v| v.parse().ok()).unwrap_or(1),
            boyut: q.get("boyut").and_then(|v| v.parse().ok()).unwrap_or(100),
        }
    }
    fn deger(&self, s: Sutun) -> &str {
        self.sutun.get(s.anahtar()).map(String::as_str).unwrap_or("")
    }
}

/// Bir sütunun süzgeci satırın değer(ler)ine uyuyor mu. Birden çok değerli sütunda (alacaklılar) seçilenlerden biri
/// yeter; "hariç" süzgecinde hariç tutulan bir değeri olan satır gizlenir.
fn sutun_uyar(s: Sutun, f: &str, degerler: &[String]) -> bool {
    if f.is_empty() {
        return true;
    }
    let bos = [String::new()];
    let degerler = if degerler.is_empty() { &bos[..] } else { degerler };
    if s.kodlu() {
        return match haric_listesi(f) {
            Some(haric) => !degerler.iter().any(|d| haric.contains(&d.as_str())),
            None => degerler.iter().any(|d| f.split(crate::icra_api::AYRAC).any(|x| x == d)),
        };
    }
    if f.starts_with('!') {
        degerler.iter().all(|d| metin_uyar(f, d))
    } else {
        degerler.iter().any(|d| metin_uyar(f, d))
    }
}

/// Süzgeç düz yazıysa (listeden seçilmemişse) kendi seçeneklerini de daraltır.
fn yazi_mi(s: Sutun, f: &str) -> bool {
    !s.kodlu() && !f.is_empty() && !f.starts_with('=') && !f.starts_with('!')
}

// ── Satırlar ────────────────────────────────────────────────────────────────────────────────────

/// Listedeki bir tebligat.
#[derive(Debug, Clone)]
pub struct Satir {
    pub id: i64,
    pub dosya: i64,
    pub birim: String,
    pub dosya_no: String,
    yil: i32,
    sira_no: i32,
    pub dosya_tur: String,
    /// Avukatın dosyadaki rolü ([crate::icra_dosyalar::benim_rolum] kodu; bilinmiyorsa "").
    pub rol: &'static str,
    pub dosya_durumu: String,
    pub acik: bool,
    pub acilis: Option<NaiveDate>,
    pub takip: String,
    pub alacaklilar: Vec<String>,
    /// Borçlu adı ya da (borçluya bağlanmamış / üçüncü kişi zarfında) muhatap adı.
    pub borclu: String,
    pub muhatap: &'static str,
    pub barkod: String,
    pub tur: String,
    pub evrak_no: i32,
    pub evrak_tarihi: String,
    evrak_gunu: Option<NaiveDate>,
    pub ptt_durumu: String,
    pub son_islem_tarihi: String,
    islem_gunu: Option<NaiveDate>,
    pub teslim_sekli: String,
    pub mazbata_var: bool,
    pub mazbata: String,
    pub yeniden: bool,
    pub elektronik: bool,
    pub kesinlesme: String,
    pub kesinlesme_kod: String,
    pub kesinlesme_neden: String,
    pub talimat: bool,
    pub aciklama: String,
    pub grup: &'static str,
    pub zarf_icerigi: String,
    pub haciz_sirasi: Option<i16>,
    pub muhatap_adi: String,
}

impl Satir {
    fn degerler(&self, s: Sutun) -> Vec<String> {
        let bir = |x: &str| vec![x.to_string()];
        let evet = |b: bool| bir(if b { "evet" } else { "hayir" });
        match s {
            Sutun::Birim => bir(&self.birim),
            Sutun::DosyaNo => bir(&self.dosya_no),
            Sutun::DosyaTur => bir(&self.dosya_tur),
            Sutun::Rol => bir(self.rol),
            Sutun::Takip => bir(&self.takip),
            Sutun::Alacakli => self.alacaklilar.clone(),
            Sutun::Borclu => bir(&self.borclu),
            Sutun::Muhatap => bir(self.muhatap),
            Sutun::Barkod => bir(&self.barkod),
            Sutun::Tur => bir(&self.tur),
            Sutun::Teblig => bir(&self.ptt_durumu),
            Sutun::Teslim => bir(&self.teslim_sekli),
            // E-tebligatta fiziki mazbata dönmez (UETS kaydı yeter): "Dönmedi" sayılmaz.
            Sutun::Mazbata => bir(if self.mazbata_var { "var" } else if self.elektronik { "etebligat" } else { "yok" }),
            Sutun::Yeniden => evet(self.yeniden),
            Sutun::Elektronik => evet(self.elektronik),
            Sutun::Kesinlesme => bir(&self.kesinlesme_kod),
            Sutun::Grup => bir(self.grup),
            Sutun::Haciz => bir(&self.haciz_kodu()),
        }
    }

    fn haciz_kodu(&self) -> String {
        if !teb::haciz_turu_mu(&self.tur) {
            return String::new();
        }
        self.haciz_sirasi.map(|s| s.to_string()).unwrap_or_else(|| "?".into())
    }

    fn json(&self) -> Value {
        json!({
            "id": self.id, "dosya": self.dosya, "birim": self.birim, "dosya_no": self.dosya_no, "dosya_tur": self.dosya_tur,
            "rol": self.rol, "rol_etiket": if self.rol.is_empty() { "" } else { crate::icra_dosyalar::rol_etiketi(self.rol) },
            "dosya_durumu": self.dosya_durumu, "acik": self.acik, "acilis_tarihi": self.acilis.map(|d| d.to_string()),
            "takip": self.takip, "alacakli": self.alacaklilar.join(", "), "borclu": self.borclu, "muhatap": self.muhatap,
            "muhatap_etiket": muhatap_etiketi(self.muhatap), "barkod": self.barkod, "tur": self.tur, "evrak_no": self.evrak_no,
            "evrak_tarihi": self.evrak_tarihi, "ptt_durumu": self.ptt_durumu, "son_islem_tarihi": self.son_islem_tarihi,
            "teslim_sekli": self.teslim_sekli, "mazbata_var": self.mazbata_var, "mazbata": self.mazbata, "yeniden": self.yeniden,
            "elektronik": self.elektronik, "kesinlesme": self.kesinlesme, "kesinlesme_kod": self.kesinlesme_kod,
            "kesinlesme_neden": self.kesinlesme_neden, "talimat": self.talimat,
            "grup": self.grup, "zarf_icerigi": self.zarf_icerigi, "haciz_sirasi": self.haciz_sirasi,
            "haciz_turu": teb::haciz_turu_mu(&self.tur), "muhatap_adi": self.muhatap_adi,
        })
    }

    /// Excel satırı (`EXCEL_BASLIKLAR` sırasıyla).
    fn excel(&self) -> Vec<String> {
        vec![
            self.birim.clone(),
            self.dosya_no.clone(),
            self.dosya_tur.clone(),
            self.dosya_durumu.clone(),
            self.acilis.map(|d| d.format("%d.%m.%Y").to_string()).unwrap_or_default(),
            self.takip.clone(),
            self.alacaklilar.join(", "),
            self.borclu.clone(),
            muhatap_etiketi(self.muhatap).to_string(),
            self.barkod.clone(),
            self.tur.clone(),
            self.evrak_tarihi.clone(),
            self.ptt_durumu.clone(),
            self.son_islem_tarihi.clone(),
            self.teslim_sekli.clone(),
            if self.mazbata_var { "Döndü".into() } else if self.elektronik { "Gerekmez (e-tebligat)".into() } else { "Dönmedi".into() },
            if self.yeniden { "Talep edildi".into() } else { String::new() },
            if self.elektronik { "E-tebligat".into() } else { "Fiziki".into() },
            if self.kesinlesme.is_empty() { self.kesinlesme_neden.clone() } else { self.kesinlesme.clone() },
            self.grup.to_string(),
            self.zarf_icerigi.clone(),
            match self.haciz_kodu().as_str() { "" => String::new(), "?" => "Bilinmiyor".into(), d => format!("89/{d}") },
        ]
    }
}

const EXCEL_BASLIKLAR: [&str; 22] = [
    "İcra Dairesi",
    "Dosya No",
    "Dosya Türü",
    "Durum",
    "Açılış",
    "Takip Şekli",
    "Alacaklı",
    "Borçlu / Muhatap",
    "Muhatap Türü",
    "Barkod",
    "Tebligat Türü",
    "Evrak Tarihi",
    "Tebliğ / Son Durum",
    "Tebliğ / İade Tarihi",
    "Teslim Şekli / İade Nedeni",
    "Mazbata",
    "Yeniden Tebliğ Talebi",
    "E-tebligat",
    "Kesinleşme",
    "Grup",
    "Zarf İçeriği",
    "89 Sırası",
];
const EXCEL_GENISLIK: [u32; 22] = [28, 12, 14, 10, 11, 10, 30, 30, 18, 16, 24, 12, 24, 16, 30, 10, 14, 11, 26, 22, 40, 10];

/// Bütün icra tebligatları (süzgeçsiz), satır biçiminde. Tebligat başına kesinleşme listeyle aynı kuralla: ödeme emri
/// → borçlunun hesabı; 103 / kıymet takdiri → sabit süre; diğer türler → yok; talimat dosyası → yok; üçüncü kişiye
/// giden ya da borçluya bağlanmamış zarf → borçlunun kesinleşmesi değildir.
pub fn satirlari_oku<C: GenericClient>(c: &mut C) -> Result<Vec<Satir>, String> {
    let alacakli_ad = veri_ad_sql("a");
    let borclu_ad = veri_ad_sql("t");
    let sql = format!(
        "SELECT tb.id, d.id, b.ad, d.dosya_no, d.yil, d.sira_no, d.tur, d.tur_kod, d.durum, d.durum_kod, d.acilis_tarihi::date,
                COALESCE(i.ornek_no, ''), (i.dosya_id IS NOT NULL),
                ARRAY(SELECT {alacakli_ad} FROM dosya_taraf x JOIN taraf a ON a.id = x.taraf_id
                       WHERE x.dosya_id = d.id AND x.rol = 'alacakli' ORDER BY x.sira, x.id),
                tb.borclu_id, COALESCE({borclu_ad}, ''), tb.barkod, tb.tebligat_turu, tb.birim_evrak_no, tb.evrak_tarihi, tb.evrak_gunu,
                tb.ptt_durumu, tb.son_islem_tarihi, tb.son_islem_zamani::date, tb.teslim_sekli, tb.tebligat_mazbatasi_var,
                tb.tebligat_mazbatasi_aciklama, tb.yeniden_teblig_talep_edildi, tb.elektronik_tebligat, tb.evrak_aciklama,
                COALESCE(dt.kesinlesme_durumu, ''), dt.kesinlesme_tarihi, tb.zarf_icerigi, tb.haciz_sirasi
           FROM tebligat_barkod tb JOIN dosya d ON d.id = tb.dosya_id JOIN birim b ON b.id = d.birim_id
           LEFT JOIN icra_takip_detay i ON i.dosya_id = d.id
           LEFT JOIN dosya_taraf dt ON dt.id = tb.borclu_id
           LEFT JOIN taraf t ON t.id = dt.taraf_id
          WHERE d.yargi_turu = 2"
    );
    let tatiller = veri::resmi_tatiller(c)?;
    let roller = crate::icra_dosyalar::avukat_rolleri(c)?;
    let bugun = chrono::Local::now().date_naive();
    let mut out = Vec::new();
    for s in c.query(&sql, &[]).map_err(hata)? {
        let (dosya_tur, tur_kod): (String, i32) = (s.get(6), s.get(7));
        let talimat = durum::talimat_mi(tur_kod, &dosya_tur);
        let aciklama: String = s.get(29);
        let borclu_id: Option<i64> = s.get(14);
        let borclu_adi: String = s.get(15);
        let mt = teb::muhatap_turu(&aciklama);
        let muhatap: &'static str = match (mt, borclu_id) {
            (MuhatapTuru::Borclu, _) => "borclu",
            (MuhatapTuru::Vekil, _) => "vekil",
            (MuhatapTuru::Temsilci, _) => "temsilci",
            (MuhatapTuru::Ucuncu, _) => "ucuncu",
            (MuhatapTuru::Yok, Some(_)) => "borclu",
            (MuhatapTuru::Yok, None) => "yok",
        };
        let muhatap_adi = teb::tebligat_muhatabi(&aciklama).0;
        let gorunen = match muhatap {
            "ucuncu" => muhatap_adi,
            _ if !borclu_adi.trim().is_empty() => borclu_adi.trim().to_string(),
            _ => muhatap_adi,
        };
        let tur: String = s.get(17);
        let (ptt, mazbata_var): (String, bool) = (s.get(21), s.get(25));
        let (borclu_kes, borclu_kes_t): (String, Option<NaiveDate>) = (s.get(30), s.get(31));
        let tk = durum::tebligat_kesinlesmesi(&tur);
        let borcluya_ait = borclu_id.is_some() && muhatap != "ucuncu";
        let (kod, metin, neden): (String, String, String) = if talimat {
            (KES_TALIMAT.into(), String::new(), durum::TALIMAT_KESINLESME_YOK.into())
        } else if tk == durum::TebligatKesinlesmesi::Yok {
            (KES_YOK.into(), String::new(), "Bu tebligat türünde kesinleşme yok".into())
        } else if !borcluya_ait {
            (KES_BAGSIZ.into(), String::new(), "Borçluya giden zarf değil (üçüncü kişi ya da bağlanmamış)".into())
        } else {
            let (k, kt) = match tk {
                durum::TebligatKesinlesmesi::SabitGun(gun) => {
                    let oe = durum::OdemeEmri {
                        birim_evrak_no: s.get(18),
                        evrak_aciklama: aciklama.clone(),
                        evrak_tarihi: s.get(19),
                        evrak_gunu: s.get(20),
                        barkod: s.get(16),
                        elektronik_tebligat: s.get(28),
                        ptt_durumu: ptt.clone(),
                        son_islem_tarihi: s.get(22),
                        son_islem_gunu: s.get(23),
                        teslim_sekli: s.get(24),
                        tebligat_mazbatasi_var: mazbata_var,
                        tebligat_mazbatasi_aciklama: s.get(26),
                    };
                    durum::borclu_kesinlesme_hesapla(false, std::slice::from_ref(&oe), &tatiller, bugun, None, Some(gun))
                }
                _ => (borclu_kes, borclu_kes_t),
            };
            let metin = durum::kesinlesme_metni(&k, kt);
            (if k.is_empty() { KES_HESAPSIZ.into() } else { k }, metin, String::new())
        };
        let ornek: String = s.get(11);
        let takip = if talimat {
            "Talimat".to_string()
        } else if !ornek.is_empty() {
            format!("Örnek {ornek}")
        } else if s.get::<_, bool>(12) {
            "(şekil bilinmiyor)".to_string()
        } else {
            String::new()
        };
        let grup = teb::tebligat_grubu(&tur);
        out.push(Satir {
            id: s.get(0),
            dosya: s.get(1),
            birim: s.get(2),
            dosya_no: s.get(3),
            yil: s.get(4),
            sira_no: s.get(5),
            rol: roller.get(&s.get::<_, i64>(1)).copied().unwrap_or(""),
            dosya_tur,
            dosya_durumu: s.get(8),
            acik: s.get::<_, i32>(9) == 0,
            acilis: s.get(10),
            takip,
            alacaklilar: s.get(13),
            borclu: gorunen,
            muhatap,
            barkod: s.get(16),
            tur,
            evrak_no: s.get(18),
            evrak_tarihi: s.get(19),
            evrak_gunu: s.get(20),
            ptt_durumu: ptt,
            son_islem_tarihi: s.get(22),
            islem_gunu: s.get(23),
            teslim_sekli: s.get(24),
            mazbata_var,
            mazbata: s.get(26),
            yeniden: s.get(27),
            elektronik: s.get(28),
            kesinlesme: metin,
            kesinlesme_kod: kod,
            kesinlesme_neden: neden,
            talimat,
            muhatap_adi: teb::muhatap_adi(&aciklama),
            aciklama,
            grup,
            zarf_icerigi: s.get(32),
            haciz_sirasi: s.get(33),
        });
    }
    Ok(out)
}

/// Taraf adı SQL ifadesi (`icra_api::ad_sql` ile aynı).
fn veri_ad_sql(t: &str) -> String {
    crate::icra_api::ad_sql(t)
}

fn aralikta(g: Option<NaiveDate>, (bas, son): (Option<NaiveDate>, Option<NaiveDate>)) -> bool {
    (bas.is_none() && son.is_none()) || g.is_some_and(|g| bas.is_none_or(|b| g >= b) && son.is_none_or(|s| g <= s))
}

// ── Sıralama ────────────────────────────────────────────────────────────────────────────────────

/// Sıralama anahtarı: boş değerler yönden bağımsız sonda.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Anahtar {
    Tarih(NaiveDate),
    Sayi(i64, i64),
    Metin(String),
}

fn sirala_anahtari(s: &Satir, alan: &str) -> Option<Anahtar> {
    let metin = |x: &str| (!x.trim().is_empty()).then(|| Anahtar::Metin(teb::tr_kucuk(x.trim())));
    match alan {
        "birim" => metin(&s.birim),
        "dosya_no" => Some(Anahtar::Sayi(i64::from(s.yil), i64::from(s.sira_no))),
        "dosya_tur" => metin(&s.dosya_tur),
        "rol" => (!s.rol.is_empty()).then(|| metin(crate::icra_dosyalar::rol_etiketi(s.rol))).flatten(),
        "durum" => metin(&s.dosya_durumu),
        "acilis" => s.acilis.map(Anahtar::Tarih),
        "takip" => metin(&s.takip),
        "alacakli" => metin(&s.alacaklilar.join(", ")),
        "borclu" => metin(&s.borclu),
        "muhatap" => metin(muhatap_etiketi(s.muhatap)),
        "barkod" => metin(&s.barkod),
        "tur" => metin(&s.tur),
        "evrak" => s.evrak_gunu.map(Anahtar::Tarih),
        "teblig" => metin(&s.ptt_durumu),
        "islem" => s.islem_gunu.map(Anahtar::Tarih),
        "teslim" => metin(&s.teslim_sekli),
        "mazbata" => Some(Anahtar::Sayi(if s.mazbata_var { 2 } else { i64::from(s.elektronik) }, 0)),
        "yeniden" => Some(Anahtar::Sayi(i64::from(s.yeniden), 0)),
        "elektronik" => Some(Anahtar::Sayi(i64::from(s.elektronik), 0)),
        "kesinlesme" => metin(if s.kesinlesme.is_empty() { &s.kesinlesme_neden } else { &s.kesinlesme }),
        "grup" => Some(Anahtar::Sayi(teb::GRUP_SIRASI.iter().position(|g| *g == s.grup).unwrap_or(99) as i64, 0)),
        "haciz" => metin(&s.haciz_kodu()),
        _ => None,
    }
}

/// Dosya içi sıra: evrak günü, sonra evrak no (eskiden yeniye).
fn dosya_ici(a: &Satir, b: &Satir) -> Ordering {
    a.evrak_gunu.cmp(&b.evrak_gunu).then(a.evrak_no.cmp(&b.evrak_no))
}

/// Satırları sıralar: seçilen sütun dosyaların sırasını belirler (dosya ilk göründüğü yerde), dosyanın zarfları
/// bölünmez ve kendi içinde eskiden yeniye kalır. Sütun seçilmemişse en yeni tebligatı olan dosya önde.
fn sirala(satirlar: &mut Vec<Satir>, alan: &str, azalan: bool) {
    let (alan, azalan) = if sirala_anahtari_gecerli(alan) { (alan, azalan) } else { ("evrak", true) };
    satirlar.sort_by(|a, b| match (sirala_anahtari(a, alan), sirala_anahtari(b, alan)) {
        (Some(x), Some(y)) => {
            let o = x.cmp(&y);
            if azalan { o.reverse() } else { o }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
    let mut sira: HashMap<i64, usize> = HashMap::new();
    for s in satirlar.iter() {
        let n = sira.len();
        sira.entry(s.dosya).or_insert(n);
    }
    satirlar.sort_by(|a, b| sira[&a.dosya].cmp(&sira[&b.dosya]).then_with(|| dosya_ici(a, b)));
}

fn sirala_anahtari_gecerli(alan: &str) -> bool {
    [
        "birim", "dosya_no", "dosya_tur", "rol", "durum", "acilis", "takip", "alacakli", "borclu", "muhatap", "barkod", "tur", "evrak", "teblig",
        "islem", "teslim", "mazbata", "yeniden", "elektronik", "kesinlesme", "grup", "haciz",
    ]
    .contains(&alan)
}

// ── Liste ───────────────────────────────────────────────────────────────────────────────────────

/// Süzgeçten geçen satırlar (sıralı) ve sütun seçenekleri (adetli; bir sütunun seçenekleri DİĞER süzgeçler uygulanmış
/// hâlde, kendi süzgeci düz yazıysa o da uygulanmış hâlde sayılır).
fn suz(satirlar: Vec<Satir>, f: &Filtre) -> (Vec<Satir>, HashMap<&'static str, HashMap<String, i64>>) {
    let aranan: Vec<String> = teb::tr_kucuk(&f.q).split_whitespace().map(str::to_string).collect();
    let mut sayim: HashMap<&'static str, HashMap<String, i64>> = HashMap::new();
    let mut gecen = Vec::new();
    for s in satirlar {
        if (f.durum == "acik" && !s.acik) || (f.durum == "kapali" && s.acik) {
            continue;
        }
        if !aralikta(s.acilis, f.acilis) || !aralikta(s.evrak_gunu, f.evrak) || !aralikta(s.islem_gunu, f.islem) {
            continue;
        }
        if !aranan.is_empty() {
            let m = teb::tr_kucuk(&format!("{} {} {} {} {}", s.birim, s.dosya_no, s.borclu, s.alacaklilar.join(" "), s.barkod));
            if !aranan.iter().all(|k| m.contains(k.as_str())) {
                continue;
            }
        }
        // Her sütunun süzgecine uyuyor mu (bir kez hesaplanır).
        let uyum: Vec<bool> = Sutun::HEPSI.iter().map(|c| sutun_uyar(*c, f.deger(*c), &s.degerler(*c))).collect();
        let hepsi = uyum.iter().all(|x| *x);
        for (i, c) in Sutun::HEPSI.iter().enumerate() {
            let digerleri = uyum.iter().enumerate().all(|(j, u)| j == i || *u);
            if digerleri && (!yazi_mi(*c, f.deger(*c)) || uyum[i]) {
                let say = sayim.entry(c.anahtar()).or_default();
                let tekil: HashSet<String> = s.degerler(*c).into_iter().collect();
                for d in tekil {
                    *say.entry(d).or_default() += 1;
                }
            }
        }
        if hepsi {
            gecen.push(s);
        }
    }
    sirala(&mut gecen, &f.sirala, f.azalan);
    (gecen, sayim)
}

pub fn tebligat_listesi<C: GenericClient>(c: &mut C, f: &Filtre) -> Result<Value, String> {
    let (satirlar, mut sayim) = suz(crate::icra_listeler::suz(satirlari_oku(c)?, c, f.liste, |s| s.dosya)?, f);
    let toplam = satirlar.len();
    let dosya_sayisi = satirlar.iter().map(|s| s.dosya).collect::<HashSet<_>>().len();
    let boyut = f.boyut.clamp(1, 500);
    let sayfa = f.sayfa.max(1);
    let dilim: Vec<Value> = satirlar.iter().skip((sayfa - 1) * boyut).take(boyut).map(Satir::json).collect();
    let mut secenekler = serde_json::Map::new();
    for c in Sutun::HEPSI {
        let say = sayim.remove(c.anahtar()).unwrap_or_default();
        let liste = secenek_sinirli(say, &|d| c.etiket(d), if c.sinirli() { SECENEK_SINIRI } else { usize::MAX });
        secenekler.insert(c.anahtar().to_string(), Value::Array(liste));
    }
    Ok(json!({"toplam": toplam, "dosya_sayisi": dosya_sayisi, "sayfa": sayfa, "boyut": boyut, "satirlar": dilim,
              "secenekler": secenekler, "secenek_siniri": SECENEK_SINIRI}))
}

/// Süzgeçteki tebligatların dosyaları (listedeki sırayla, tekil).
pub fn tebligat_dosyalari<C: GenericClient>(c: &mut C, f: &Filtre) -> Result<Vec<i64>, String> {
    let (satirlar, _) = suz(crate::icra_listeler::suz(satirlari_oku(c)?, c, f.liste, |s| s.dosya)?, f);
    let mut gorulen = HashSet::new();
    Ok(satirlar.into_iter().map(|s| s.dosya).filter(|d| gorulen.insert(*d)).collect())
}

/// Süzülmüş, sıralı liste .xlsx olarak (her tebligat bir satır).
pub fn tebligat_excel<C: GenericClient>(c: &mut C, f: &Filtre) -> Result<Vec<u8>, String> {
    let (satirlar, _) = suz(crate::icra_listeler::suz(satirlari_oku(c)?, c, f.liste, |s| s.dosya)?, f);
    let satirlar: Vec<Vec<String>> = satirlar.iter().map(Satir::excel).collect();
    crate::gerekceli_karar::xlsx_bayt(&EXCEL_BASLIKLAR, &satirlar, &EXCEL_GENISLIK)
}

// ── Tür eşleştir ────────────────────────────────────────────────────────────────────────────────

/// Şablon dışı tebligat türü yazılışları, eşleştirme anahtarına göre gruplu (Python `tur_eslestirme`):
/// sabit türlerin dışındaki zarf içerikleri; "Belirsiz" / boş türlerde kaydın kendi mazbatasındaki belge adı.
/// Döner: {gruplar: [{anahtar, ornek, adet, cesit, kaynak, ornek_dosya, yazilislar}], turler: [..]}.
pub fn tur_eslestirme<C: GenericClient>(c: &mut C) -> Result<Value, String> {
    struct Grup {
        yazilislar: BTreeMap<String, i64>,
        adet: i64,
        kaynak: &'static str,
        ornek_dosya: String,
    }
    let mut gruplar: BTreeMap<String, Grup> = BTreeMap::new();
    // Bir kez eşleştirilen yazılış bir daha sorulmaz (Python ile aynı).
    let eslesmis: HashSet<String> = crate::icra_kayit::tur_eslesmeleri(c)?.into_keys().collect();
    for s in c
        .query(
            "SELECT tb.tebligat_turu, tb.tebligat_mazbatasi_aciklama, b.ad || ' ' || d.dosya_no
               FROM tebligat_barkod tb JOIN dosya d ON d.id = tb.dosya_id JOIN birim b ON b.id = d.birim_id
              WHERE d.yargi_turu = 2",
            &[],
        )
        .map_err(hata)?
    {
        let (tur, mazbata, dosya): (String, String, String) = (s.get(0), s.get(1), s.get(2));
        let Some((ham, kaynak)) = ham_tur(&tur, &mazbata) else { continue };
        let anahtar = teb::tur_eslesme_anahtari(&ham);
        if anahtar.is_empty() || eslesmis.contains(&anahtar) {
            continue;
        }
        let g = gruplar.entry(anahtar).or_insert_with(|| Grup { yazilislar: BTreeMap::new(), adet: 0, kaynak, ornek_dosya: dosya });
        *g.yazilislar.entry(ham).or_default() += 1;
        g.adet += 1;
    }
    let mut liste: Vec<Value> = gruplar
        .into_iter()
        .map(|(anahtar, g)| {
            let ornek = g.yazilislar.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|x| x.0.clone()).unwrap_or_default();
            let yazilislar: Vec<Value> = g.yazilislar.iter().map(|(y, n)| json!({"yazilis": y, "adet": n})).collect();
            json!({"anahtar": anahtar, "ornek": ornek, "adet": g.adet, "cesit": g.yazilislar.len(),
                   "kaynak": if g.kaynak == "mazbata" { "Mazbata" } else { "Zarf" }, "ornek_dosya": g.ornek_dosya,
                   "yazilislar": yazilislar})
        })
        .collect();
    liste.sort_by(|a, b| b["adet"].as_i64().cmp(&a["adet"].as_i64()).then_with(|| a["ornek"].as_str().cmp(&b["ornek"].as_str())));
    Ok(json!({"gruplar": liste, "turler": tur_secenekleri(c)?}))
}

/// Eşleştirilecek ham yazılış ve kaynağı. Sabit türler ve eşleştirilmiş (artık sabit ya da kategori olan) türler
/// sorulmaz; "Belirsiz" / boşta mazbatadaki belge adı alınır, o da yoksa sorulmaz.
fn ham_tur(tur: &str, mazbata: &str) -> Option<(String, &'static str)> {
    let tur = tur.trim();
    if teb::TEMEL_TURLER.contains(&tur) {
        return None;
    }
    if tur.is_empty() || tur == teb::BELIRSIZ {
        let ad = teb::mazbata_belge_adi(mazbata);
        return (!ad.is_empty()).then_some((ad, "mazbata"));
    }
    Some((tur.to_string(), "zarf"))
}

/// Tür seçenekleri: sabit türler + kullanıcının eklediği kategoriler + eşleştirmelerde kullanılan türler.
fn tur_secenekleri<C: GenericClient>(c: &mut C) -> Result<Vec<String>, String> {
    let mut turler: Vec<String> = teb::TEMEL_TURLER.iter().map(|x| x.to_string()).collect();
    for s in c
        .query("SELECT ad FROM tebligat_tur_kategorisi UNION SELECT tur FROM tebligat_tur_eslesmesi ORDER BY 1", &[])
        .map_err(hata)?
    {
        let ad: String = s.get(0);
        if !ad.trim().is_empty() && !turler.contains(&ad) {
            turler.push(ad);
        }
    }
    Ok(turler)
}

/// Eşleştirmeleri kaydeder ve mevcut kayıtlara uygular: grubun bütün yazılışları seçilen türe çevrilir. Yeni tür adı
/// kategori olarak eklenir. Döner: (eşleştirme sayısı, türü değişen tebligat sayısı).
pub fn tur_eslestir<C: GenericClient>(c: &mut C, eslesmeler: &[(String, String)]) -> Result<(usize, usize), String> {
    let mut harita: HashMap<String, String> = HashMap::new();
    for (anahtar, tur) in eslesmeler {
        let (anahtar, tur) = (anahtar.trim(), veri::kirp(tur.trim(), 120));
        if anahtar.is_empty() || tur.is_empty() {
            return Err("Her grup için bir tür seçilmeli.".into());
        }
        harita.insert(anahtar.to_string(), tur);
    }
    if harita.is_empty() {
        return Ok((0, 0));
    }
    let mut tx = c.transaction().map_err(hata)?;
    for (anahtar, tur) in &harita {
        tx.execute(
            "INSERT INTO tebligat_tur_eslesmesi (anahtar, tur) VALUES ($1, $2)
             ON CONFLICT (anahtar) DO UPDATE SET tur = EXCLUDED.tur, guncelleme = now()",
            &[anahtar, tur],
        )
        .map_err(hata)?;
        if !teb::TEMEL_TURLER.contains(&tur.as_str()) {
            tx.execute("INSERT INTO tebligat_tur_kategorisi (ad) VALUES ($1) ON CONFLICT (ad) DO NOTHING", &[tur]).map_err(hata)?;
        }
    }
    let mut degisen = 0usize;
    let kayitlar = tx
        .query(
            "SELECT tb.id, tb.tebligat_turu, tb.tebligat_mazbatasi_aciklama FROM tebligat_barkod tb JOIN dosya d ON d.id = tb.dosya_id
              WHERE d.yargi_turu = 2",
            &[],
        )
        .map_err(hata)?;
    for s in kayitlar {
        let (id, tur, mazbata): (i64, String, String) = (s.get(0), s.get(1), s.get(2));
        let Some((ham, _)) = ham_tur(&tur, &mazbata) else { continue };
        if let Some(yeni) = harita.get(&teb::tur_eslesme_anahtari(&ham)) {
            if *yeni != tur {
                tx.execute("UPDATE tebligat_barkod SET tebligat_turu = $2 WHERE id = $1", &[&id, yeni]).map_err(hata)?;
                degisen += 1;
            }
        }
    }
    tx.commit().map_err(hata)?;
    Ok((harita.len(), degisen))
}

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use crate::icra_kayit as kayit;
    use crate::icra_veri::{dosya_kunyesi_kaydet, ham_tarihleri_turet, YargiTuru};
    let sessiz = |_: &str| {};
    let kunye = |no: &str, tur_kod: i64, tur: &str, birim: &str, bid: i64| {
        json!({"dosyaId": "x", "dosyaNo": no, "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": tur_kod, "dosyaTur": tur,
               "birimAdi": birim, "birimId": bid, "dosyaAcilisTarihi": "01.10.2026"})
    };
    let (d1, _) = dosya_kunyesi_kaydet(c, &kunye("2026/5", 35, "İcra Dosyası", "İzmir 1. İcra Dairesi", 1001), Some(YargiTuru::Icra.kod())).unwrap();
    let (d2, _) = dosya_kunyesi_kaydet(c, &kunye("2026/40", 35, "İcra Dosyası", "Ankara 3. İcra Dairesi", 2002), Some(YargiTuru::Icra.kod())).unwrap();
    let (d3, _) = dosya_kunyesi_kaydet(c, &kunye("2026/7", 1, "Talimat Dosyası", "İzmir 1. İcra Dairesi", 1001), Some(YargiTuru::Icra.kod())).unwrap();
    kayit::dosya_taraf_kaydet(c, d1, &[json!({"adi": "X BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum"}),
                                       json!({"adi": "AHMET YILMAZ", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    kayit::dosya_taraf_kaydet(c, d2, &[json!({"adi": "AYŞE KAYA", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    kayit::dosya_taraf_kaydet(c, d3, &[json!({"adi": "VELİ KARA", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    let bag = |c: &mut postgres::Client, d: i64| -> i64 { c.query_one("SELECT id FROM dosya_taraf WHERE dosya_id = $1 AND rol = 'borclu'", &[&d]).unwrap().get(0) };
    let (b1, b2, b3) = (bag(c, d1), bag(c, d2), bag(c, d3));
    let teb_ekle = |c: &mut postgres::Client, d: i64, no: i32, tur: &str, borclu: Option<i64>, aciklama: &str, ptt: &str, tarih: &str, mazbata: &str| {
        let id: i64 = c
            .query_one(
                "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, borclu_id, evrak_aciklama, barkod, ptt_durumu,
                                              evrak_tarihi, son_islem_tarihi, teslim_sekli, tebligat_mazbatasi_aciklama)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8, $9, $10) RETURNING id",
                &[&d, &no, &tur, &borclu, &aciklama, &format!("40076635{no:05}"), &ptt, &tarih,
                  &if ptt == "İADE" { "Adres yetersiz" } else { "" }, &mazbata],
            )
            .unwrap()
            .get(0);
        ham_tarihleri_turet(c, "tebligat_barkod", id).unwrap();
    };
    teb_ekle(c, d1, 10, "Ödeme Emri", Some(b1), "Normal Tebligat[BORÇLU AHMET YILMAZ]", "İADE", "05/10/2026", "");
    teb_ekle(c, d1, 12, "Banka (Haciz İhbarnamesi)", None, "Elektronik Tebligat[ARACIKISIKURUM DENİZBANK A.Ş.]", "", "08/10/2026", "");
    teb_ekle(c, d1, 11, "Tensip zaptı", Some(b1), "Normal Tebligat[BORÇLU AHMET YILMAZ]", "TESLİM EDİLDİ", "06/10/2026", "");
    teb_ekle(c, d2, 3, "Ödeme Emri", Some(b2), "Normal Tebligat[BORÇLU AYŞE KAYA]", "TESLİM EDİLDİ", "01/10/2026", "");
    teb_ekle(c, d2, 4, "Belirsiz", Some(b2), "Normal Tebligat[BORÇLU AYŞE KAYA]", "", "02/10/2026",
             "Satış İlanı - [AYŞE KAYA] - Barkod Numarası: 4007663500004 - Tebliğ Edildi");
    teb_ekle(c, d3, 1, "Kıymet Takdiri Raporu", Some(b3), "Normal Tebligat[BORÇLU VELİ KARA]", "TESLİM EDİLDİ", "03/10/2026", "");
    teb_ekle(c, d2, 5, "TENSİP ZAPTI", Some(b2), "Normal Tebligat[BORÇLU AYŞE KAYA]", "", "09/10/2026", "");
    let al = |c: &mut postgres::Client, q: &[(&str, &str)]| -> Value {
        let q: HashMap<String, String> = q.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        tebligat_listesi(c, &Filtre::oku(&q)).unwrap()
    };

    // Bütün tebligatlar: borçluya bağlanmamış banka zarfı da listede, muhatap adı ve türüyle
    let l = al(c, &[]);
    assert_eq!((&l["toplam"], &l["dosya_sayisi"]), (&json!(7), &json!(3)));
    let banka = l["satirlar"].as_array().unwrap().iter().find(|s| s["tur"] == "Banka (Haciz İhbarnamesi)").unwrap().clone();
    assert_eq!((&banka["borclu"], &banka["muhatap"], &banka["kesinlesme_kod"]), (&json!("ARACIKISIKURUM DENİZBANK A.Ş."), &json!("ucuncu"), &json!("yok")));
    // Varsayılan sıra: en yeni tebligatı olan dosya önde (d2: 09/10), dosyanın zarfları eskiden yeniye ve bölünmeden
    let sira: Vec<(i64, i64)> = l["satirlar"].as_array().unwrap().iter().map(|s| (s["dosya"].as_i64().unwrap(), s["evrak_no"].as_i64().unwrap())).collect();
    assert_eq!(sira, vec![(d2, 3), (d2, 4), (d2, 5), (d1, 10), (d1, 11), (d1, 12), (d3, 1)]);
    // Sütuna göre sıralama dosyaların sırasını belirler: daire artan → Ankara önce, sonra İzmir dosyaları
    let s = al(c, &[("sirala", "birim")]);
    assert_eq!(s["satirlar"][0]["birim"], "Ankara 3. İcra Dairesi");
    let s = al(c, &[("sirala", "dosya_no"), ("yon", "azalan")]);
    assert_eq!(s["satirlar"][0]["dosya_no"], "2026/40", "dosya no sayıya göre (40 > 7 > 5)");
    // Süzgeçler: teslim şekli / iade nedeni, muhatap türü, kesinleşme (talimat ayrı), tarih aralığı, hariç
    assert_eq!(al(c, &[("teslim", "adres")])["toplam"], 1);
    assert_eq!(al(c, &[("muhatap", "ucuncu")])["satirlar"][0]["tur"], "Banka (Haciz İhbarnamesi)");
    let t = al(c, &[("kesinlesme", "talimat")]);
    assert_eq!((&t["toplam"], &t["satirlar"][0]["takip"], &t["satirlar"][0]["kesinlesme_neden"]), (&json!(1), &json!("Talimat"), &json!("Talimat dosyasında kesinleşme yok")));
    assert_eq!(al(c, &[("evrak_bas", "2026-10-06"), ("evrak_son", "08.10.2026")])["toplam"], 2);
    assert_eq!(al(c, &[("birim", "!İzmir 1. İcra Dairesi")])["dosya_sayisi"], 1);
    assert_eq!(al(c, &[("tur", "=Ödeme Emri"), ("teblig", "=İADE")])["toplam"], 1);
    // Seçenekler: kendi süzgeci listeden seçildiyse diğer değerler kalır
    let s = al(c, &[("muhatap", "ucuncu")]);
    assert_eq!(s["secenekler"]["muhatap"].as_array().unwrap().len(), 2, "{}", s["secenekler"]["muhatap"]);
    assert_eq!(s["secenekler"]["birim"], json!([{"deger": "İzmir 1. İcra Dairesi", "etiket": "İzmir 1. İcra Dairesi", "adet": 1}]));
    // Seçim ve Excel
    let q: HashMap<String, String> = [("tur".to_string(), "=Ödeme Emri".to_string())].into();
    assert_eq!(tebligat_dosyalari(c, &Filtre::oku(&q)).unwrap(), vec![d1, d2], "listedeki sırayla (d1'in ödeme emri daha yeni)");
    let x = tebligat_excel(c, &Filtre::oku(&HashMap::new())).unwrap();
    assert!(x.starts_with(b"PK"), "xlsx bir zip");

    // Tür eşleştir: "Tensip zaptı" ve "TENSİP ZAPTI" tek grup; "Belirsiz" kayıt mazbatadaki belge adıyla
    let e = tur_eslestirme(c).unwrap();
    let gruplar = e["gruplar"].as_array().unwrap();
    assert_eq!(gruplar.len(), 2, "{gruplar:?}");
    assert_eq!((&gruplar[0]["adet"], &gruplar[0]["cesit"], &gruplar[0]["kaynak"]), (&json!(2), &json!(2), &json!("Zarf")));
    assert_eq!((&gruplar[1]["ornek"], &gruplar[1]["kaynak"]), (&json!("Satış İlanı"), &json!("Mazbata")));
    assert!(e["turler"].as_array().unwrap().contains(&json!("Ödeme Emri")));
    let anahtar = gruplar[0]["anahtar"].as_str().unwrap().to_string();
    assert!(tur_eslestir(c, &[(anahtar.clone(), " ".into())]).is_err());
    assert_eq!(tur_eslestir(c, &[(anahtar, "Tensip Zaptı".into()), (gruplar[1]["anahtar"].as_str().unwrap().into(), "Satış İlanı".into())]).unwrap(), (2, 3));
    assert_eq!(al(c, &[("tur", "=Tensip Zaptı")])["toplam"], 2);
    let e = tur_eslestirme(c).unwrap();
    assert!(e["turler"].as_array().unwrap().contains(&json!("Tensip Zaptı")), "yeni tür seçeneklere eklenir");
    assert_eq!(e["gruplar"], json!([]), "eşleştirilen yazılışlar bir daha sorulmaz");
    // Eşleştirilen yazılış sonraki taramada da çevrilir
    let es = crate::icra_kayit::tur_eslesmeleri(c).unwrap();
    assert_eq!(teb::tur_eslestir("tensip zaptı", &es).as_deref(), Some("Tensip Zaptı"));

    c.batch_execute("DELETE FROM dosya; DELETE FROM taraf; DELETE FROM birim; DELETE FROM tebligat_tur_eslesmesi; DELETE FROM tebligat_tur_kategorisi").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sutun_suzgeci() {
        let d = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(sutun_uyar(Sutun::Alacakli, "x bank", &d(&["X BANKASI A.Ş.", "Y"])));
        assert!(sutun_uyar(Sutun::Alacakli, "=Y", &d(&["X BANKASI A.Ş.", "Y"])));
        assert!(!sutun_uyar(Sutun::Alacakli, "!Y", &d(&["X BANKASI A.Ş.", "Y"])), "hariç tutulan bir alacaklısı olan gizlenir");
        assert!(sutun_uyar(Sutun::Alacakli, "!Z", &d(&[])));
        assert!(sutun_uyar(Sutun::Mazbata, "var", &d(&["var"])) && !sutun_uyar(Sutun::Mazbata, "!var", &d(&["var"])));
        assert!(yazi_mi(Sutun::Tur, "odeme") && !yazi_mi(Sutun::Tur, "=Ödeme Emri") && !yazi_mi(Sutun::Mazbata, "var"));
    }

    #[test]
    fn ham_tur_secimi() {
        assert_eq!(ham_tur("Ödeme Emri", ""), None);
        assert_eq!(ham_tur("Tensip zaptı", ""), Some(("Tensip zaptı".into(), "zarf")));
        assert_eq!(ham_tur("Belirsiz", "Satış İlanı - [VELİ] - Barkod Numarası: 1"), Some(("Satış İlanı".into(), "mazbata")));
        assert_eq!(ham_tur("", ""), None);
    }
}
