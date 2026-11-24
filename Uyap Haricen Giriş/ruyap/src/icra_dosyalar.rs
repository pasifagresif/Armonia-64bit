//! İcra Takip Süreçleri → "Dosya Listesi" sekmesi (20.11.2026, kullanıcı notu `Desktop\süreçler\dosya listesi sekmesi.md`).
//!
//! Her icra dosyası bir satırdır: daire, dosya no, alacaklılar, borçlular, e-imza sahibi avukatın bu dosyadaki rolü
//! ("benim rolüm"), takip türü / yolu / şekli, açılış ve kapanış günü. Her sütunda başlık süzgeci (yazı: içinde geçen;
//! `=`: listeden seçilen; `!`: listedekiler hariç) ve sıralama vardır. Sıralama Türkçe alfabeyle ve sayılar sayı
//! olarak yapılır ("İstanbul 2." "İstanbul 11."den önce; Ç C'den sonra).
//!
//! Kapanış günü ve sebebi UYAP'ın dosya durumu metninden okunur: kapalı dosyada "Kapalı (sebep - gg.aa.yyyy)".
//!
//! "Benim rolüm": büro ayarındaki avukat adı (`buro_ayar` → `eimza_avukat` = {"adlar": ["AD SOYAD", …]}) dosyanın
//! vekil listelerinde aranır; adın bulunduğu tarafın rolü avukatın rolüdür. Adın UYAP'tan otomatik alınması ayrı iş
//! (akşam canlı inceleme); ayar yoksa sütun boş kalır.
//!
//! Uçlar (`/__cu__/veri/…`, `icra_api::dagit`):
//!   GET  dosyalar             ?<süzgeçler>&sirala=&yon=artan|azalan&sayfa=&boyut= — satırlar + adetli seçenekler
//!   GET  dosyalar_kimlikleri  ?<süzgeçler>&mod= — süzgeçteki dosyaların kimlikleri; `mod` verilirse yalnız o güncellemenin
//!                             istek atacağı (eksiği olan) dosyalar (güncelleme kapsamı için)
//!   GET  dosyalar_istek       ?<süzgeçler>&mod=liste|takip|taraf|hepsi — başlatmadan önce kaç dosyaya, kaç istek gideceği

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use postgres::GenericClient;
use serde_json::{json, Value};

use crate::icra_api::{haric_listesi, metin_uyar, secenek_sinirli, AYRAC, SECENEK_SINIRI};
use crate::icra_tebligat as teb;
use crate::icra_veri as veri;

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

/// E-imza sahibi avukatın adlarının tutulduğu büro ayarı.
pub const AVUKAT_AYARI: &str = "eimza_avukat";

// ── Süzgeç ──────────────────────────────────────────────────────────────────────────────────────

/// Süzgeç sütunları. Tablo sütunlarına ek olarak: dosya yılı (Dosya No süzgecinin içinde) ve açılış / kapanış günlerinin
/// yılı ve ayı (üstteki "Yıl / ay" seçimi; 21.11.2026). Yıl ve ay kodludur: yıl "2025", ay "1"…"12".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sutun {
    Birim,
    DosyaNo,
    DosyaTur,
    Alacakli,
    Borclu,
    Rol,
    Tur,
    Yol,
    Sekil,
    Yil,
    AcilisYil,
    AcilisAy,
    KapanisYil,
    KapanisAy,
}

/// Ay adları (süzgeç etiketi).
pub const AYLAR: [&str; 12] = ["Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"];

impl Sutun {
    pub const HEPSI: [Sutun; 14] = [
        Sutun::Birim,
        Sutun::DosyaNo,
        Sutun::DosyaTur,
        Sutun::Alacakli,
        Sutun::Borclu,
        Sutun::Rol,
        Sutun::Tur,
        Sutun::Yol,
        Sutun::Sekil,
        Sutun::Yil,
        Sutun::AcilisYil,
        Sutun::AcilisAy,
        Sutun::KapanisYil,
        Sutun::KapanisAy,
    ];
    pub fn anahtar(self) -> &'static str {
        match self {
            Sutun::Birim => "birim",
            Sutun::DosyaNo => "dosya_no",
            Sutun::DosyaTur => "dosya_tur",
            Sutun::Alacakli => "alacakli",
            Sutun::Borclu => "borclu",
            Sutun::Rol => "rol",
            Sutun::Tur => "takip_turu",
            Sutun::Yol => "takip_yolu",
            Sutun::Sekil => "takip_sekli",
            Sutun::Yil => "yil",
            Sutun::AcilisYil => "acilis_yil",
            Sutun::AcilisAy => "acilis_ay",
            Sutun::KapanisYil => "kapanis_yil",
            Sutun::KapanisAy => "kapanis_ay",
        }
    }
    fn bul(anahtar: &str) -> Option<Sutun> {
        Sutun::HEPSI.into_iter().find(|s| s.anahtar() == anahtar)
    }
    /// Değer sabit bir koddur (yalnız listeden seçilir).
    fn kodlu(self) -> bool {
        matches!(self, Sutun::Rol | Sutun::Yil | Sutun::AcilisYil | Sutun::AcilisAy | Sutun::KapanisYil | Sutun::KapanisAy)
    }
    /// Yalnız kapalı dosyalarda anlamlı (kapanış günü).
    fn kapanis(self) -> bool {
        matches!(self, Sutun::KapanisYil | Sutun::KapanisAy)
    }
    fn sinirli(self) -> bool {
        matches!(self, Sutun::DosyaNo | Sutun::Alacakli | Sutun::Borclu)
    }
    fn etiket(self, deger: &str) -> String {
        match (self, deger) {
            (Sutun::Rol, d) => rol_etiketi(d).to_string(),
            (Sutun::Tur | Sutun::Yol | Sutun::Sekil, "") => "(okunmadı)".into(),
            (Sutun::AcilisYil | Sutun::AcilisAy | Sutun::KapanisYil | Sutun::KapanisAy, "") => "(tarihsiz)".into(),
            (Sutun::AcilisAy | Sutun::KapanisAy, d) => {
                d.parse::<usize>().ok().and_then(|n| AYLAR.get(n.wrapping_sub(1))).map(|a| a.to_string()).unwrap_or_else(|| d.to_string())
            }
            (_, "") => "(boş)".into(),
            (_, d) => d.to_string(),
        }
    }
}

/// Avukatın dosyadaki rolü (süzgeç kodu → ad).
pub fn rol_etiketi(kod: &str) -> &'static str {
    match kod {
        "alacakli" => "Alacaklı vekili",
        "borclu" => "Borçlu vekili",
        "diger_alacakli" => "Diğer haciz/rehin alacaklısı vekili",
        "ucuncu" => "3. kişi vekili",
        "diger" => "Diğer taraf vekili",
        _ => "(bilinmiyor)",
    }
}

#[derive(Debug, Default, Clone)]
pub struct Filtre {
    /// "acik" | "kapali" | "" (hepsi)
    pub durum: String,
    pub acilis: (Option<NaiveDate>, Option<NaiveDate>),
    /// Yalnız kapalı dosyalarda anlamlı.
    pub kapanis: (Option<NaiveDate>, Option<NaiveDate>),
    /// Dosya listesi (`icra_listeler`): yalnız bu listedeki dosyalar; None = hepsi.
    pub liste: Option<i64>,
    pub sutun: HashMap<&'static str, String>,
    /// Süzgeçlerin sırası ("Süzgeçler" penceresinde sürüklenerek değişir; 21.11.2026). Bir süzgecin seçenekleri yalnız
    /// kendinden ÖNCEKİ süzgeçlere uyan dosyalardan sayılır (iç içe süzgeç: üstteki alttakini kapsar). Sırada olmayan
    /// süzgeçler en üsttedir. Boşsa eski kural: her sütunun seçenekleri öteki bütün süzgeçlerle.
    pub sira: Vec<Sutun>,
    pub sirala: String,
    pub azalan: bool,
    pub sayfa: usize,
    pub boyut: usize,
}

impl Filtre {
    pub fn oku(q: &HashMap<String, String>) -> Filtre {
        let gun = |a: &str| q.get(a).and_then(|v| veri::tarih::gun_coz(v));
        let durum = q.get("durum").cloned().unwrap_or_default();
        let mut sutun = HashMap::new();
        for s in Sutun::HEPSI {
            // Kapanış yılı / ayı yalnız kapalı dosyalarda (açık dosyanın kapanış günü yok).
            if s.kapanis() && durum != "kapali" {
                continue;
            }
            if let Some(v) = q.get(s.anahtar()).filter(|v| !v.is_empty()) {
                sutun.insert(s.anahtar(), v.clone());
            }
        }
        let mut sira: Vec<Sutun> = Vec::new();
        for s in q.get("sira").map(String::as_str).unwrap_or("").split(',').filter_map(|a| Sutun::bul(a.trim())) {
            if sutun.contains_key(s.anahtar()) && !sira.contains(&s) {
                sira.push(s);
            }
        }
        Filtre {
            acilis: (gun("acilis_bas"), gun("acilis_son")),
            kapanis: (gun("kapanis_bas"), gun("kapanis_son")),
            durum,
            liste: crate::icra_listeler::liste_oku(q),
            sutun,
            sira,
            sirala: q.get("sirala").cloned().unwrap_or_default(),
            azalan: q.get("yon").is_some_and(|y| y == "azalan"),
            sayfa: q.get("sayfa").and_then(|v| v.parse().ok()).unwrap_or(1),
            boyut: q.get("boyut").and_then(|v| v.parse().ok()).unwrap_or(100),
        }
    }
    /// Sütun `i`'nin seçenekleri sayılırken sütun `j`'nin süzgeci uygulanır mı (sıra kuralı, bkz. `sira`).
    fn sayimda_uygula(&self, i: Sutun, j: Sutun) -> bool {
        if self.sira.is_empty() {
            return true;
        }
        match (self.sira.iter().position(|s| *s == i), self.sira.iter().position(|s| *s == j)) {
            (Some(pi), Some(pj)) => pj < pi,
            _ => true,
        }
    }
    fn deger(&self, s: Sutun) -> &str {
        self.sutun.get(s.anahtar()).map(String::as_str).unwrap_or("")
    }
}

/// Bir sütunun süzgeci satırın değer(ler)ine uyuyor mu (birden çok değerli sütunda seçilenlerden biri yeter;
/// "hariç" süzgecinde hariç tutulan bir değeri olan satır gizlenir). `icra_tebligatlar` ile aynı kural.
fn sutun_uyar(s: Sutun, f: &str, degerler: &[String]) -> bool {
    if f.is_empty() {
        return true;
    }
    let bos = [String::new()];
    let degerler = if degerler.is_empty() { &bos[..] } else { degerler };
    if s.kodlu() {
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

fn yazi_mi(s: Sutun, f: &str) -> bool {
    !s.kodlu() && !f.is_empty() && !f.starts_with('=') && !f.starts_with('!')
}

// ── Yardımcılar ─────────────────────────────────────────────────────────────────────────────────

/// Kapalı dosyanın durum metninden kapanış günü ve sebebi: "Kapalı (Haricen Tahsil - 12.03.2025)" →
/// (2025-03-12, "Haricen Tahsil"). Gün bulunamazsa (None, parantez içi metin).
pub fn kapanis_coz(durum: &str) -> (Option<NaiveDate>, String) {
    let ic = match (durum.find('('), durum.rfind(')')) {
        (Some(a), Some(b)) if b > a => &durum[a + 1..b],
        (Some(a), None) => &durum[a + 1..],
        _ => "",
    };
    let karakterler: Vec<(usize, char)> = durum.char_indices().collect();
    let mut gun = None;
    let mut bulunan: Option<(usize, usize)> = None;
    // Son "gg.aa.yyyy" (ya da / ile) kalıbı.
    for i in (0..karakterler.len()).rev() {
        if i + 10 > karakterler.len() {
            continue;
        }
        let p: String = karakterler[i..i + 10].iter().map(|x| x.1).collect();
        let b = p.as_bytes();
        let rakam = |j: usize| b[j].is_ascii_digit();
        if p.len() == 10 && rakam(0) && rakam(1) && rakam(3) && rakam(4) && (6..10).all(rakam) && (b[2] == b'.' || b[2] == b'/') && b[5] == b[2] {
            let bicim = if b[2] == b'.' { "%d.%m.%Y" } else { "%d/%m/%Y" };
            if let Ok(g) = NaiveDate::parse_from_str(&p, bicim) {
                gun = Some(g);
                let bas = karakterler[i].0;
                bulunan = Some((bas, bas + p.len()));
                break;
            }
        }
    }
    let mut sebep = ic.to_string();
    if let Some((bas, son)) = bulunan {
        let tarih = &durum[bas..son];
        sebep = sebep.replace(tarih, "");
    }
    let sebep = sebep.trim().trim_matches(|c: char| c == '-' || c == ':' || c == ',' || c.is_whitespace()).trim().to_string();
    (gun, sebep)
}

/// Adları karşılaştırmak için: Türkçe küçük harf; harf ve rakam dışı her şey (nokta, virgül, parantez, köşeli parantez)
/// boşluk; boşluklar tek.
pub(crate) fn ad_anahtari(ad: &str) -> String {
    teb::tr_kucuk(ad)
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Taraf rolü → avukatın rol kodu.
fn rol_kodu(rol: &str) -> &'static str {
    let r = teb::tr_kucuk(rol);
    if r == "alacakli" {
        "alacakli"
    } else if r == "borclu" {
        "borclu"
    } else if r.contains("haciz") || r.contains("rehin") || r.contains("ipotek") {
        "diger_alacakli"
    } else if r.contains("ucuncu") || r.starts_with('3') {
        "ucuncu"
    } else {
        "diger"
    }
}

/// Dosyadaki bir taraf: rolü, adı ve UYAP'ın vekil sütunundaki bütün vekil adları (birlikte).
#[derive(Debug, Clone, Default)]
pub struct VekilliTaraf {
    pub rol: String,
    pub ad: String,
    pub vekiller: String,
}

/// Avukatın rolü ve vekili olduğu taraf(lar)ın adları. Avukatın adı tarafın BÜTÜN vekil verisi içinde aranır (UYAP'ta
/// vekil sütununda e-imza avukatıyla birlikte dosyadaki bütün avukatlar yazılır; kullanıcı isteği 20.11.2026): ad, tam
/// kelimeler hâlinde ve sırasıyla geçmeli ("AYŞE KAYA" "Av. Ayşe Kaya, Mehmet Demir" içinde bulunur, "AYŞE KAYALI"da
/// bulunmaz). Avukat birden çok tarafın vekiliyse sıra: alacaklı, borçlu, diğer haciz/rehin alacaklısı, 3. kişi.
pub fn benim_rolum(taraflar: &[VekilliTaraf], avukatlar: &[String]) -> (&'static str, Vec<String>) {
    let adlar: Vec<String> = avukatlar.iter().map(|a| ad_anahtari(a)).filter(|a| !a.is_empty()).map(|a| format!(" {a} ")).collect();
    if adlar.is_empty() {
        return ("", Vec::new());
    }
    let eslesen: Vec<(&'static str, &str)> = taraflar
        .iter()
        .filter(|t| {
            let v = format!(" {} ", ad_anahtari(&t.vekiller));
            adlar.iter().any(|a| v.contains(a.as_str()))
        })
        .map(|t| (rol_kodu(&t.rol), t.ad.trim()))
        .collect();
    let sira = |r: &str| ["alacakli", "borclu", "diger_alacakli", "ucuncu", "diger"].iter().position(|x| *x == r).unwrap_or(9);
    let Some(rol) = eslesen.iter().map(|x| x.0).min_by_key(|r| sira(r)) else { return ("", Vec::new()) };
    let mut adlar_: Vec<String> = Vec::new();
    for (r, ad) in &eslesen {
        if *r == rol && !ad.is_empty() && !adlar_.iter().any(|x| x == ad) {
            adlar_.push(ad.to_string());
        }
    }
    (rol, adlar_)
}

/// Türkçe alfabetik sıralama anahtarı: harfler Türkçe alfabe sırasıyla, rakam dizileri sayı olarak (sayılar
/// harflerden önce), boşluk ve noktalama yok sayılır. "İstanbul 2. İcra" < "İstanbul 11. İcra"; "Cizre" < "Çorum".
pub fn tr_sira_anahtari(s: &str) -> Vec<(u8, u64)> {
    const ALFABE: &str = "abcçdefgğhıijklmnoöprsştuüvwxyz";
    let mut out = Vec::new();
    let mut sayi: Option<u64> = None;
    for ch in s.chars() {
        if let Some(r) = ch.to_digit(10) {
            sayi = Some(sayi.unwrap_or(0).saturating_mul(10).saturating_add(u64::from(r)));
            continue;
        }
        if let Some(n) = sayi.take() {
            out.push((0, n));
        }
        let k = match ch {
            'I' => 'ı',
            'İ' => 'i',
            c => c.to_lowercase().next().unwrap_or(c),
        };
        // Şapkalı harfler düz karşılığıyla.
        let k = match k {
            'â' => 'a',
            'î' => 'i',
            'û' => 'u',
            c => c,
        };
        if let Some(i) = ALFABE.chars().position(|a| a == k) {
            out.push((1, i as u64));
        } else if k.is_alphanumeric() {
            out.push((2, u64::from(k as u32)));
        }
    }
    if let Some(n) = sayi {
        out.push((0, n));
    }
    out
}

fn aralikta(g: Option<NaiveDate>, (bas, son): (Option<NaiveDate>, Option<NaiveDate>)) -> bool {
    (bas.is_none() && son.is_none()) || g.is_some_and(|g| bas.is_none_or(|b| g >= b) && son.is_none_or(|s| g <= s))
}

// ── Satırlar ────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Satir {
    pub dosya: i64,
    pub birim: String,
    pub dosya_no: String,
    yil: i32,
    sira_no: i32,
    pub dosya_tur: String,
    /// MTS'den icra dairesine geçmiş dosya (`icra_api::MTS_KAYNAKLI_SQL`): dosya türü "İcra Dosyası (MTS'den)".
    pub mts_kaynakli: bool,
    pub durum: String,
    pub acik: bool,
    pub acilis: Option<NaiveDate>,
    pub kapanis: Option<NaiveDate>,
    pub kapanis_sebebi: String,
    pub alacaklilar: Vec<String>,
    pub borclular: Vec<String>,
    pub rol: &'static str,
    /// Avukatın vekili olduğu taraf(lar)ın adları (rolüyle aynı türden).
    pub rol_taraflar: Vec<String>,
    pub takip_turu: String,
    pub takip_yolu: String,
    pub takip_sekli: String,
    /// Takip türü hiç okunmamış (talimat dosyası değil): "Takip türlerini güncelle" istek atar.
    pub takip_eksik: bool,
    /// Alacaklısı ya da borçlusu kayıtlı değil: "Tarafları güncelle" bütün tarafları ister (2 istek).
    pub taraf_tum: bool,
    /// Borçlusu kimliksiz (TCKN / vergi no yok): "Tarafları güncelle" borçlu listesini ister (1 istek).
    pub taraf_kimliksiz: bool,
    /// Taraf bilgileri (bütün vekiller) hiç okunmamış: "Tarafları güncelle" taraf bilgilerini ister (1 istek).
    pub vekil_okunmamis: bool,
}

impl Satir {
    /// Dosya türü süzgeç değeri ve görünen adı: UYAP'taki tür ("İcra Dosyası", "MTS", "Talimat Dosyası", "İflas
    /// Dosyası" …); MTS'den gelen icra dosyasında " (MTS'den)" eki (Süreçler'deki "Dosya türü" süzgeciyle aynı değer).
    pub fn dosya_turu(&self) -> String {
        let tur = self.dosya_tur.trim();
        if self.mts_kaynakli { format!("{tur}{}", crate::icra_api::MTS_KAYNAKLI_EKI) } else { tur.to_string() }
    }

    fn degerler(&self, s: Sutun) -> Vec<String> {
        let bir = |x: &str| vec![x.to_string()];
        let yil = |g: Option<NaiveDate>| vec![g.map(|g| g.year().to_string()).unwrap_or_default()];
        let ay = |g: Option<NaiveDate>| vec![g.map(|g| g.month().to_string()).unwrap_or_default()];
        match s {
            Sutun::Birim => bir(&self.birim),
            Sutun::DosyaNo => bir(&self.dosya_no),
            Sutun::DosyaTur => vec![self.dosya_turu()],
            Sutun::Alacakli => self.alacaklilar.clone(),
            Sutun::Borclu => self.borclular.clone(),
            Sutun::Rol => bir(self.rol),
            Sutun::Tur => bir(&self.takip_turu),
            Sutun::Yol => bir(&self.takip_yolu),
            Sutun::Sekil => bir(&self.takip_sekli),
            Sutun::Yil => vec![if self.yil > 0 { self.yil.to_string() } else { String::new() }],
            Sutun::AcilisYil => yil(self.acilis),
            Sutun::AcilisAy => ay(self.acilis),
            Sutun::KapanisYil => yil(self.kapanis),
            Sutun::KapanisAy => ay(self.kapanis),
        }
    }

    fn json(&self) -> Value {
        json!({
            "dosya": self.dosya, "birim": self.birim, "dosya_no": self.dosya_no, "dosya_tur": self.dosya_tur,
            "dosya_turu": self.dosya_turu(), "mts_kaynakli": self.mts_kaynakli, "yil": self.yil,
            "durum": self.durum, "acik": self.acik,
            "acilis_tarihi": self.acilis.map(|d| d.to_string()), "kapanis_tarihi": self.kapanis.map(|d| d.to_string()),
            "kapanis_sebebi": self.kapanis_sebebi, "alacaklilar": self.alacaklilar, "borclular": self.borclular,
            "rol": self.rol, "rol_etiket": if self.rol.is_empty() { "" } else { rol_etiketi(self.rol) }, "rol_taraflar": self.rol_taraflar,
            "takip_turu": self.takip_turu, "takip_yolu": self.takip_yolu, "takip_sekli": self.takip_sekli,
        })
    }
}

/// Büro ayarındaki avukat adları (yoksa boş).
pub fn avukat_adlari<C: GenericClient>(c: &mut C) -> Result<Vec<String>, String> {
    let s = c.query_opt("SELECT deger FROM buro_ayar WHERE anahtar = $1", &[&AVUKAT_AYARI]).map_err(hata)?;
    Ok(s.map(|s| s.get::<_, Value>(0))
        .and_then(|v| v.get("adlar").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|x| x.as_str().map(str::trim).filter(|x| !x.is_empty()).map(str::to_string))
        .collect())
}

/// E-imza sertifikasının sahibinin adı (UYAP'a giriş yapılan kart): "AYŞE KAYA" ya da "CN=AYŞE KAYA, SERIALNUMBER=…"
/// biçiminde gelebilir; yalnız ad soyad döner.
pub fn sertifika_adi(subject: &str) -> String {
    let s = subject.trim();
    let cn = s
        .split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("CN=").or_else(|| p.strip_prefix("cn=")))
        .unwrap_or(s);
    cn.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// UYAP'a e-imzayla giriş yapılınca kartın sahibini büro ayarındaki avukatlara ekler (yoksa); "Rolüm" bu adı vekil
/// listelerinde arar (kullanıcı kararı 20.11.2026: avukat adı otomatik). Büro veritabanı bu bilgisayarda yoksa
/// (uzak cihaz, komut satırı aracı) bir şey yapmaz; hata girişi bozmaz.
pub fn eimza_avukatini_kaydet(subject: &str) {
    let ad = sertifika_adi(subject);
    if ad.is_empty() || !crate::veritabani::hazir() {
        return;
    }
    let sonuc = crate::veritabani::baglanti().and_then(|mut b| avukat_ekle(&mut *b, &ad));
    if let Err(e) = sonuc {
        println!("[!] E-imza avukatı kaydedilemedi: {e}");
    }
}

/// Avukat adını büro ayarına ekler (aynı ad varsa eklemez). Döner: eklendi mi.
pub fn avukat_ekle<C: GenericClient>(c: &mut C, ad: &str) -> Result<bool, String> {
    let mut adlar = avukat_adlari(c)?;
    if adlar.iter().any(|a| ad_anahtari(a) == ad_anahtari(ad)) {
        return Ok(false);
    }
    adlar.push(ad.to_string());
    c.execute(
        "INSERT INTO buro_ayar (anahtar, deger) VALUES ($1, $2)
         ON CONFLICT (anahtar) DO UPDATE SET deger = EXCLUDED.deger, guncellendi = now()",
        &[&AVUKAT_AYARI, &json!({ "adlar": adlar })],
    )
    .map_err(hata)?;
    Ok(true)
}

/// Bütün icra dosyalarında avukatın rolü (dosya kimliği → [`benim_rolum`] kodu; bilinmiyorsa ""). Dosya Listesi ile
/// aynı hesap; Tebligatlar sekmesinin "Rolümüz" süzgeci kullanır (23.11.2026).
pub fn avukat_rolleri<C: GenericClient>(c: &mut C) -> Result<HashMap<i64, &'static str>, String> {
    let avukatlar = avukat_adlari(c)?;
    let taraf_ad = crate::icra_api::ad_sql("vt");
    let mut taraflar: HashMap<i64, Vec<VekilliTaraf>> = HashMap::new();
    if avukatlar.is_empty() {
        return Ok(HashMap::new());
    }
    for s in c
        .query(
            &format!(
                "SELECT x.dosya_id, x.rol || chr(9) || {taraf_ad} || chr(9) || string_agg(btrim(v.ad || ' ' || v.soyad), ', ' ORDER BY dv.id)
                   FROM dosya_taraf x JOIN dosya d ON d.id = x.dosya_id AND d.yargi_turu = 2 JOIN taraf vt ON vt.id = x.taraf_id
                   JOIN dosya_taraf_vekiller dv ON dv.dosyataraf_id = x.id JOIN vekil v ON v.id = dv.vekil_id
                  WHERE x.uyapta_gorulmedi IS NULL
                  GROUP BY x.dosya_id, x.id, x.rol, vt.id, vt.tur, vt.unvan, vt.mersis_no, vt.vergi_no, vt.ad, vt.soyad, vt.tckn"
            ),
            &[],
        )
        .map_err(hata)?
    {
        let ham: String = s.get(1);
        let mut p = ham.splitn(3, '\t');
        let (Some(rol), Some(ad), Some(vekiller)) = (p.next(), p.next(), p.next()) else { continue };
        taraflar.entry(s.get::<_, i64>(0)).or_default().push(VekilliTaraf {
            rol: rol.to_string(), ad: ad.to_string(), vekiller: vekiller.to_string(),
        });
    }
    Ok(taraflar.into_iter().map(|(d, t)| (d, benim_rolum(&t, &avukatlar).0)).collect())
}

/// Takip şekli: "Örnek 7" (örnek no okunduysa), yoksa UYAP'taki adı.
fn sekil_adi(ornek: &str, aciklama: &str) -> String {
    if !ornek.trim().is_empty() {
        format!("Örnek {}", ornek.trim())
    } else {
        aciklama.trim().to_string()
    }
}

pub fn satirlari_oku<C: GenericClient>(c: &mut C) -> Result<Vec<Satir>, String> {
    let avukatlar = avukat_adlari(c)?;
    let alacakli_ad = crate::icra_api::ad_sql("a");
    let taraf_ad = crate::icra_api::ad_sql("vt");
    let sql = format!(
        "SELECT d.id, b.ad, d.dosya_no, d.yil, d.sira_no, d.tur, d.durum, d.durum_kod, d.acilis_tarihi::date,
                ARRAY(SELECT {alacakli_ad} FROM dosya_taraf x JOIN taraf a ON a.id = x.taraf_id
                       WHERE x.dosya_id = d.id AND x.rol = 'alacakli' AND x.uyapta_gorulmedi IS NULL ORDER BY x.sira, x.id),
                ARRAY(SELECT {alacakli_ad} FROM dosya_taraf x JOIN taraf a ON a.id = x.taraf_id
                       WHERE x.dosya_id = d.id AND x.rol = 'borclu' AND x.uyapta_gorulmedi IS NULL ORDER BY x.sira, x.id),
                COALESCE(i.takibin_turu_aciklama, ''), COALESCE(i.takibin_yolu_aciklama, ''),
                COALESCE(i.takibin_sekli_aciklama, ''), COALESCE(i.ornek_no, ''),
                ARRAY(SELECT x.rol || chr(9) || {taraf_ad} || chr(9)
                             || string_agg(btrim(v.ad || ' ' || v.soyad), ', ' ORDER BY dv.id)
                        FROM dosya_taraf x JOIN taraf vt ON vt.id = x.taraf_id
                        JOIN dosya_taraf_vekiller dv ON dv.dosyataraf_id = x.id JOIN vekil v ON v.id = dv.vekil_id
                       WHERE x.dosya_id = d.id AND x.uyapta_gorulmedi IS NULL
                       GROUP BY x.id, x.rol, vt.id, vt.tur, vt.unvan, vt.mersis_no, vt.vergi_no, vt.ad, vt.soyad, vt.tckn),
                (i.ayrinti_zamani IS NULL AND d.tur_kod <> 1),
                NOT EXISTS (SELECT 1 FROM dosya_taraf x WHERE x.dosya_id = d.id AND x.rol = 'alacakli')
                  OR NOT EXISTS (SELECT 1 FROM dosya_taraf x WHERE x.dosya_id = d.id AND x.rol = 'borclu'),
                EXISTS (SELECT 1 FROM dosya_taraf x JOIN taraf tr ON tr.id = x.taraf_id
                         WHERE x.dosya_id = d.id AND x.rol = 'borclu' AND x.uyapta_gorulmedi IS NULL
                           AND tr.tckn IS NULL AND tr.mersis_no IS NULL AND tr.vergi_no = ''),
                d.taraf_bilgisi_zamani IS NULL,
                {mts}
           FROM dosya d JOIN birim b ON b.id = d.birim_id
           LEFT JOIN icra_takip_detay i ON i.dosya_id = d.id
          WHERE d.yargi_turu = 2",
        mts = crate::icra_api::MTS_KAYNAKLI_SQL,
    );
    let mut out = Vec::new();
    for s in c.query(&sql, &[]).map_err(hata)? {
        let durum: String = s.get(6);
        let acik = s.get::<_, i32>(7) == 0;
        let (kapanis, kapanis_sebebi) = if acik { (None, String::new()) } else { kapanis_coz(&durum) };
        let vekilli: Vec<VekilliTaraf> = s
            .get::<_, Vec<String>>(15)
            .into_iter()
            .filter_map(|x| {
                let mut p = x.splitn(3, '\t');
                Some(VekilliTaraf { rol: p.next()?.to_string(), ad: p.next()?.to_string(), vekiller: p.next()?.to_string() })
            })
            .collect();
        let (rol, rol_taraflar) = benim_rolum(&vekilli, &avukatlar);
        let (ornek, sekil): (String, String) = (s.get(14), s.get(13));
        out.push(Satir {
            dosya: s.get(0),
            birim: s.get(1),
            dosya_no: s.get(2),
            yil: s.get(3),
            sira_no: s.get(4),
            dosya_tur: s.get(5),
            mts_kaynakli: s.get(20),
            durum,
            acik,
            acilis: s.get(8),
            kapanis,
            kapanis_sebebi,
            alacaklilar: s.get(9),
            borclular: s.get(10),
            rol,
            rol_taraflar,
            takip_turu: s.get::<_, String>(11).trim().to_string(),
            takip_yolu: s.get::<_, String>(12).trim().to_string(),
            takip_sekli: sekil_adi(&ornek, &sekil),
            takip_eksik: s.get(16),
            taraf_tum: s.get(17),
            taraf_kimliksiz: s.get(18),
            vekil_okunmamis: s.get(19),
        });
    }
    Ok(out)
}

// ── Sıralama ────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Anahtar {
    Tarih(NaiveDate),
    Sayi(i64, i64),
    Metin(Vec<(u8, u64)>),
}

fn sirala_anahtari(s: &Satir, alan: &str) -> Option<Anahtar> {
    let metin = |x: &str| (!x.trim().is_empty()).then(|| Anahtar::Metin(tr_sira_anahtari(x.trim())));
    match alan {
        "birim" => metin(&s.birim),
        "dosya_no" => Some(Anahtar::Sayi(i64::from(s.yil), i64::from(s.sira_no))),
        "dosya_tur" => metin(&s.dosya_turu()),
        "alacakli" => metin(&s.alacaklilar.join(", ")),
        "borclu" => metin(&s.borclular.join(", ")),
        "rol" => (!s.rol.is_empty()).then(|| Anahtar::Metin(tr_sira_anahtari(rol_etiketi(s.rol)))),
        "takip_turu" => metin(&s.takip_turu),
        "takip_yolu" => metin(&s.takip_yolu),
        "takip_sekli" => metin(&s.takip_sekli),
        "acilis" => s.acilis.map(Anahtar::Tarih),
        "kapanis" => s.kapanis.map(Anahtar::Tarih),
        _ => None,
    }
}

const SIRALANAN: [&str; 11] =
    ["birim", "dosya_no", "dosya_tur", "alacakli", "borclu", "rol", "takip_turu", "takip_yolu", "takip_sekli", "acilis", "kapanis"];

/// Seçilen sütuna göre (boşlar yönden bağımsız sonda); eşitlikte daire (alfabetik) ve dosya no. Varsayılan: daire.
fn sirala(satirlar: &mut [Satir], alan: &str, azalan: bool) {
    let (alan, azalan) = if SIRALANAN.contains(&alan) { (alan, azalan) } else { ("birim", false) };
    let daire = |a: &Satir, b: &Satir| {
        tr_sira_anahtari(&a.birim)
            .cmp(&tr_sira_anahtari(&b.birim))
            .then((a.yil, a.sira_no).cmp(&(b.yil, b.sira_no)))
            .then(a.dosya.cmp(&b.dosya))
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

// ── Liste ───────────────────────────────────────────────────────────────────────────────────────

fn suz(satirlar: Vec<Satir>, f: &Filtre) -> (Vec<Satir>, HashMap<&'static str, HashMap<String, i64>>) {
    let mut sayim: HashMap<&'static str, HashMap<String, i64>> = HashMap::new();
    let mut gecen = Vec::new();
    for s in satirlar {
        if (f.durum == "acik" && !s.acik) || (f.durum == "kapali" && s.acik) {
            continue;
        }
        if !aralikta(s.acilis, f.acilis) {
            continue;
        }
        if f.durum == "kapali" && !aralikta(s.kapanis, f.kapanis) {
            continue;
        }
        let uyum: Vec<bool> = Sutun::HEPSI.iter().map(|c| sutun_uyar(*c, f.deger(*c), &s.degerler(*c))).collect();
        for (i, c) in Sutun::HEPSI.iter().enumerate() {
            let digerleri = uyum.iter().enumerate().all(|(j, u)| j == i || *u || !f.sayimda_uygula(*c, Sutun::HEPSI[j]));
            if digerleri && (!yazi_mi(*c, f.deger(*c)) || uyum[i]) {
                let say = sayim.entry(c.anahtar()).or_default();
                let tekil: HashSet<String> = s.degerler(*c).into_iter().collect();
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

pub fn dosya_listesi<C: GenericClient>(c: &mut C, f: &Filtre) -> Result<Value, String> {
    let avukat_bilinmiyor = avukat_adlari(c)?.is_empty();
    let hepsi = crate::icra_listeler::suz(satirlari_oku(c)?, c, f.liste, |s| s.dosya)?;
    let genel_toplam = hepsi.len();
    let (satirlar, mut sayim) = suz(hepsi, f);
    let toplam = satirlar.len();
    let boyut = f.boyut.clamp(1, 500);
    let sayfa = f.sayfa.max(1);
    let dilim: Vec<Value> = satirlar.iter().skip((sayfa - 1) * boyut).take(boyut).map(Satir::json).collect();
    let mut secenekler = serde_json::Map::new();
    for c in Sutun::HEPSI {
        let say = sayim.remove(c.anahtar()).unwrap_or_default();
        let liste = secenek_sinirli(say, &|d| c.etiket(d), if c.sinirli() { SECENEK_SINIRI } else { usize::MAX });
        secenekler.insert(c.anahtar().to_string(), Value::Array(liste));
    }
    Ok(json!({"toplam": toplam, "sayfa": sayfa, "boyut": boyut, "satirlar": dilim, "secenekler": secenekler,
              "secenek_siniri": SECENEK_SINIRI, "avukat_bilinmiyor": avukat_bilinmiyor, "genel_toplam": genel_toplam}))
}

/// Güncelleme türünün bu dosyaya istek atıp atmayacağı (`icra_guncelle` ile aynı kural). Bilinmeyen tür: hepsi.
/// "hepsi" (Tüm verileri güncelle) dosya listesinden gelen AÇIK dosyalara işler. "eksik": Tüm verileri güncelle, dosya
/// listesi yeniden ALINMADAN (kullanıcı "Liste yakın zamanda alındı, yeniden alınmasın" dedi; 21.11.2026) — aynı dosyalar.
fn ihtiyac(s: &Satir, mod_: &str) -> bool {
    match mod_ {
        "takip" => s.takip_eksik,
        "taraf" => s.taraf_tum || s.taraf_kimliksiz || s.vekil_okunmamis,
        "hepsi" | "eksik" => s.acik && (s.takip_eksik || s.taraf_tum || s.taraf_kimliksiz || s.vekil_okunmamis),
        _ => true,
    }
}

/// UYAP'taki bütün açık icra dosyalarının listesinin en son eksiksiz alındığı zamanı yazan büro ayarı.
pub const LISTE_ZAMANI_AYARI: &str = "icra_liste_zamani";

/// Dosya listesi eksiksiz alındı (`icra_guncelle`, daire / tarih / seçim kapsamı olmadan): zaman + alınan dosya sayısı.
pub fn liste_zamanini_yaz<C: GenericClient>(c: &mut C, alinan: usize) -> Result<(), String> {
    c.execute(
        "INSERT INTO buro_ayar (anahtar, deger)
         VALUES ($1, jsonb_build_object('zaman', to_char(now(), 'YYYY-MM-DD\"T\"HH24:MI:SS'), 'alinan', $2::bigint))
         ON CONFLICT (anahtar) DO UPDATE SET deger = EXCLUDED.deger, guncellendi = now()",
        &[&LISTE_ZAMANI_AYARI, &(alinan as i64)],
    )
    .map_err(hata)?;
    Ok(())
}

/// Dosya listesinin en son ne zaman alındığı: `{"zaman": "YYYY-MM-DDTHH:MM:SS", "alinan": n, "yaklasik": false}`.
/// Kayıt yoksa (bu sürümden önceki taramalar) en son künye yazımı (`dosya.son_senkron`) yaklaşık zaman olarak döner;
/// hiç icra dosyası yoksa null.
pub fn liste_zamani<C: GenericClient>(c: &mut C) -> Result<Value, String> {
    if let Some(s) = c.query_opt("SELECT deger FROM buro_ayar WHERE anahtar = $1", &[&LISTE_ZAMANI_AYARI]).map_err(hata)? {
        let v: Value = s.get(0);
        if v.get("zaman").and_then(Value::as_str).is_some() {
            return Ok(json!({"zaman": v["zaman"], "alinan": v.get("alinan").cloned().unwrap_or(Value::Null), "yaklasik": false}));
        }
    }
    let s = c
        .query_one("SELECT to_char(max(son_senkron), 'YYYY-MM-DD\"T\"HH24:MI:SS') FROM dosya WHERE yargi_turu = 2", &[])
        .map_err(hata)?;
    Ok(match s.get::<_, Option<String>>(0) {
        Some(z) => json!({"zaman": z, "alinan": Value::Null, "yaklasik": true}),
        None => Value::Null,
    })
}

/// Süzgeçteki dosyaların kimlikleri (listedeki sırayla); `mod` verilirse yalnız o güncellemenin istek atacakları.
pub fn dosya_kimlikleri<C: GenericClient>(c: &mut C, f: &Filtre, mod_: &str) -> Result<Vec<i64>, String> {
    let (satirlar, _) = suz(crate::icra_listeler::suz(satirlari_oku(c)?, c, f.liste, |s| s.dosya)?, f);
    Ok(satirlar.into_iter().filter(|s| ihtiyac(s, mod_)).map(|s| s.dosya).collect())
}

/// Başlatmadan önce tahmin: süzgeçteki dosya sayısı, istek atılacak dosya sayısı, en az istek sayısı ve süresi.
#[derive(Debug, Clone, PartialEq)]
pub struct Tahmin {
    pub dosya: usize,
    pub hedef: usize,
    pub takip: usize,
    pub taraf_tum: usize,
    pub taraf_kimliksiz: usize,
    pub vekil_okunmamis: usize,
    pub sayfa: usize,
    pub istek: usize,
    pub sure_sn: u64,
    /// Yeni / kapanan dosyalar için ek istek gidebilir (liste ve hepsi).
    pub en_az: bool,
}

/// `acik_toplam`: veritabanındaki açık icra dosyası sayısı (UYAP listesinin kaç sayfa süreceği buna göre).
pub fn tahmin_et(satirlar: &[Satir], mod_: &str, acik_toplam: usize) -> Result<Tahmin, String> {
    use crate::icra_guncelle::{ISTEK_ARASI_SN, MOLA_HER_DOSYA, MOLA_SN, SAYFA_BOYU};
    if !["liste", "takip", "taraf", "hepsi", "eksik"].contains(&mod_) {
        return Err("Güncelleme türü geçersiz.".into());
    }
    let sayfa = acik_toplam.div_ceil(SAYFA_BOYU).max(1);
    let hedef: Vec<&Satir> = if mod_ == "liste" { Vec::new() } else { satirlar.iter().filter(|s| ihtiyac(s, mod_)).collect() };
    let n = hedef.len();
    let takip = hedef.iter().filter(|s| s.takip_eksik).count();
    let taraf_tum = if mod_ == "takip" { 0 } else { hedef.iter().filter(|s| s.taraf_tum).count() };
    let taraf_kimliksiz = if mod_ == "takip" { 0 } else { hedef.iter().filter(|s| !s.taraf_tum && s.taraf_kimliksiz).count() };
    let vekil_okunmamis = if mod_ == "takip" { 0 } else { hedef.iter().filter(|s| !s.taraf_tum && s.vekil_okunmamis).count() };
    // Taraf istekleri (`icra_guncelle::taraflari_cek`): borçlu listesi (borçlu yok / kimliksiz) + taraf bilgileri
    // (taraf yok / vekiller okunmamış); ikisi birden 2 istek.
    let taraf_istegi: usize = if mod_ == "takip" {
        0
    } else {
        hedef.iter().map(|s| usize::from(s.taraf_tum || s.taraf_kimliksiz) + usize::from(s.taraf_tum || s.vekil_okunmamis)).sum()
    };
    let kapali = hedef.iter().filter(|s| !s.acik).count();
    // Seçili dosyaları UYAP'ta bulma: `icra_guncelle::secili_kayitlar` kararı (tek tek arama ya da listeyi bir kez alma).
    let bulma = if n == 0 { 0 } else if n > sayfa + kapali { sayfa + kapali } else { n + kapali };
    let istek = match mod_ {
        "liste" => sayfa,
        "takip" => bulma + takip,
        // Eksikler (liste yeniden alınmadan): dosyalar UYAP'ta bulunur (tek tek ya da liste bir kez), sonra taraf + takip.
        "taraf" | "eksik" => bulma + taraf_istegi + takip,
        _ => sayfa + taraf_istegi + takip,
    };
    let sure_sn = (istek as f64 * ISTEK_ARASI_SN) as u64 + (n / MOLA_HER_DOSYA) as u64 * MOLA_SN;
    Ok(Tahmin { dosya: satirlar.len(), hedef: n, takip, taraf_tum, taraf_kimliksiz, vekil_okunmamis, sayfa, istek, sure_sn, en_az: matches!(mod_, "liste" | "hepsi") })
}

pub fn istek_tahmini<C: GenericClient>(c: &mut C, f: &Filtre, mod_: &str) -> Result<Value, String> {
    let (satirlar, _) = suz(crate::icra_listeler::suz(satirlari_oku(c)?, c, f.liste, |s| s.dosya)?, f);
    let acik: i64 = c
        .query_one("SELECT count(*) FROM dosya WHERE yargi_turu = 2 AND durum_kod = 0", &[])
        .map_err(hata)?
        .get(0);
    let t = tahmin_et(&satirlar, mod_, acik.max(0) as usize)?;
    let liste_zamani = liste_zamani(c)?;
    Ok(json!({"mod": mod_, "dosya": t.dosya, "hedef": t.hedef, "takip": t.takip, "taraf_tum": t.taraf_tum,
              "taraf_kimliksiz": t.taraf_kimliksiz, "vekil_okunmamis": t.vekil_okunmamis, "sayfa": t.sayfa, "istek": t.istek, "sure_sn": t.sure_sn, "en_az": t.en_az,
              "acik_toplam": acik, "liste_zamani": liste_zamani}))
}

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use crate::icra_kayit as kayit;
    use crate::icra_veri::{dosya_kunyesi_kaydet, YargiTuru};
    let sessiz = |_: &str| {};
    let kunye = |no: &str, durum_kod: i64, durum: &str, birim: &str, bid: i64, acilis: &str| {
        json!({"dosyaId": "x", "dosyaNo": no, "dosyaDurumKod": durum_kod, "dosyaDurum": durum, "dosyaTurKod": 35,
               "dosyaTur": "İcra Dosyası", "birimAdi": birim, "birimId": bid, "dosyaAcilisTarihi": acilis})
    };
    let icra = Some(YargiTuru::Icra.kod());
    let (d1, _) = dosya_kunyesi_kaydet(c, &kunye("2026/11", 0, "Açık", "İstanbul 11. İcra Dairesi", 3011, "01.10.2026"), icra).unwrap();
    let (d2, _) = dosya_kunyesi_kaydet(c, &kunye("2026/5", 0, "Açık", "İstanbul 2. İcra Dairesi", 3002, "05.10.2026"), icra).unwrap();
    let (_d3, _) = dosya_kunyesi_kaydet(
        c,
        &kunye("2025/9", 1, "Kapalı (Haricen Tahsil - 12.03.2026)", "Çorum 1. İcra Dairesi", 3101, "02.01.2025"),
        icra,
    )
    .unwrap();
    kayit::dosya_taraf_kaydet(c, d1, &[json!({"adi": "X BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum", "vekil": "[AYŞE KAYA]"}),
                                       json!({"adi": "AHMET YILMAZ", "rol": "Borçlu", "kisiKurum": "Kişi"}),
                                       json!({"adi": "MEHMET YILMAZ", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    kayit::dosya_taraf_kaydet(c, d2, &[json!({"adi": "Y A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum"}),
                                       json!({"adi": "VELİ KARA", "rol": "Borçlu", "kisiKurum": "Kişi", "vekil": "[Ayşe  Kaya]"})], &sessiz).unwrap();
    let al = |c: &mut postgres::Client, q: &[(&str, &str)]| -> Value {
        let q: HashMap<String, String> = q.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        dosya_listesi(c, &Filtre::oku(&q)).unwrap()
    };

    // Avukat bilinmiyor: rol boş
    let l = al(c, &[("durum", "")]);
    assert_eq!(l["toplam"], 3);
    assert_eq!(l["avukat_bilinmiyor"], true);
    // Varsayılan sıra: daire alfabetik, sayılar sayı olarak (Ç C'den sonra, İ I'dan sonra; "2." "11."den önce)
    let birimler: Vec<&str> = l["satirlar"].as_array().unwrap().iter().map(|s| s["birim"].as_str().unwrap()).collect();
    assert_eq!(birimler, vec!["Çorum 1. İcra Dairesi", "İstanbul 2. İcra Dairesi", "İstanbul 11. İcra Dairesi"]);
    let kapali = &l["satirlar"][0];
    assert_eq!((&kapali["kapanis_tarihi"], &kapali["kapanis_sebebi"]), (&json!("2026-03-12"), &json!("Haricen Tahsil")));
    assert_eq!(l["satirlar"][2]["borclular"], json!(["AHMET YILMAZ", "MEHMET YILMAZ"]));

    // Avukat adı (girişteki e-imza kartından) ayarda: alacaklı vekili / borçlu vekili (büyük-küçük harf ve boşluk farkı önemsiz)
    assert!(avukat_ekle(c, &sertifika_adi("Ayşe Kaya")).unwrap());
    assert!(!avukat_ekle(c, "AYŞE  KAYA").unwrap(), "aynı ad ikinci kez eklenmez");
    let l = al(c, &[]);
    assert_eq!(l["avukat_bilinmiyor"], false);
    let rol = |l: &Value, d: i64| l["satirlar"].as_array().unwrap().iter().find(|s| s["dosya"] == d).unwrap()["rol"].clone();
    assert_eq!((rol(&l, d1), rol(&l, d2)), (json!("alacakli"), json!("borclu")));
    let satir = |l: &Value, d: i64| l["satirlar"].as_array().unwrap().iter().find(|s| s["dosya"] == d).unwrap().clone();
    assert_eq!((&satir(&l, d1)["rol_taraflar"], &satir(&l, d2)["rol_taraflar"]), (&json!(["X BANKASI A.Ş."]), &json!(["VELİ KARA"])));
    assert_eq!(al(c, &[("rol", "borclu")])["toplam"], 1);
    assert_eq!(al(c, &[("rol", "!borclu")])["toplam"], 2, "hariç: borçlu vekili olunan dosya dışındakiler");
    // Tebligatlar sekmesinin "Rolümüz" süzgeci aynı hesabı kullanır.
    let roller = avukat_rolleri(c).unwrap();
    assert_eq!((roller.get(&d1).copied(), roller.get(&d2).copied()), (Some("alacakli"), Some("borclu")));
    c.execute("INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, evrak_aciklama) VALUES ($1, 5, 'Ödeme Emri', 'Normal Tebligat[BORÇLU VELİ KARA]')",
              &[&d2]).unwrap();
    let q: HashMap<String, String> = [("rol".to_string(), "borclu".to_string())].into_iter().collect();
    let t = crate::icra_tebligatlar::tebligat_listesi(c, &crate::icra_tebligatlar::Filtre::oku(&q)).unwrap();
    assert_eq!((t["toplam"].as_i64(), t["satirlar"][0]["rol_etiket"].as_str()), (Some(1), Some("Borçlu vekili")));
    let q: HashMap<String, String> = [("rol".to_string(), "alacakli".to_string())].into_iter().collect();
    assert_eq!(crate::icra_tebligatlar::tebligat_listesi(c, &crate::icra_tebligatlar::Filtre::oku(&q)).unwrap()["toplam"].as_i64(), Some(0));
    c.execute("DELETE FROM tebligat_barkod WHERE dosya_id = $1", &[&d2]).unwrap();

    // Açık / kapalı, kapanış aralığı, süzgeçler, sıralama
    assert_eq!(al(c, &[("durum", "acik")])["toplam"], 2);
    assert_eq!(al(c, &[("durum", "kapali"), ("kapanis_bas", "2026-03-01"), ("kapanis_son", "2026-03-31")])["toplam"], 1);
    assert_eq!(al(c, &[("durum", "kapali"), ("kapanis_bas", "2026-04-01")])["toplam"], 0);
    assert_eq!(al(c, &[("acilis_bas", "2026-10-03")])["toplam"], 1);
    assert_eq!(al(c, &[("birim", "istanbul")])["toplam"], 2);
    assert_eq!(al(c, &[("birim", "!Çorum 1. İcra Dairesi")])["toplam"], 2);
    assert_eq!(al(c, &[("borclu", "=MEHMET YILMAZ")])["satirlar"][0]["dosya"], d1);
    assert_eq!(al(c, &[("sirala", "birim"), ("yon", "azalan")])["satirlar"][0]["birim"], "İstanbul 11. İcra Dairesi");
    assert_eq!(al(c, &[("sirala", "acilis"), ("yon", "azalan")])["satirlar"][0]["dosya"], d2);
    let s = al(c, &[("birim", "=İstanbul 2. İcra Dairesi")]);
    assert_eq!(s["secenekler"]["birim"].as_array().unwrap().len(), 3, "kendi süzgeci listeden seçildiyse diğer daireler kalır");
    let q: HashMap<String, String> = [("durum".to_string(), "acik".to_string())].into();
    assert_eq!(dosya_kimlikleri(c, &Filtre::oku(&q), "").unwrap(), vec![d2, d1]);
    assert_eq!(l["genel_toplam"], 3);
    // İstek tahmini: üç dosyanın da takip türü okunmamış; d3'ün (kapalı) tarafı hiç yok, d1/d2 borçluları kimliksiz
    let q: HashMap<String, String> = HashMap::new();
    assert_eq!(dosya_kimlikleri(c, &Filtre::oku(&q), "taraf").unwrap().len(), 3);
    let t = istek_tahmini(c, &Filtre::oku(&q), "taraf").unwrap();
    assert_eq!((&t["dosya"], &t["hedef"], &t["taraf_tum"], &t["taraf_kimliksiz"], &t["vekil_okunmamis"]),
               (&json!(3), &json!(3), &json!(1), &json!(2), &json!(2)));
    let t = istek_tahmini(c, &Filtre::oku(&q), "hepsi").unwrap();
    assert_eq!((&t["hedef"], &t["en_az"]), (&json!(2), &json!(true)), "tüm veriler yalnız açık dosyalara");
    assert!(istek_tahmini(c, &Filtre::oku(&q), "olmaz").is_err());
    // Eksikler (liste yeniden alınmadan) "hepsi" ile aynı dosyalar
    assert_eq!(dosya_kimlikleri(c, &Filtre::oku(&q), "eksik").unwrap(), dosya_kimlikleri(c, &Filtre::oku(&q), "hepsi").unwrap());
    // Dosya listesinin en son alındığı zaman: kayıt yoksa son künye yazımı (yaklaşık), yazılınca kesin
    let z = liste_zamani(c).unwrap();
    assert_eq!(z["yaklasik"], true, "{z}");
    assert_eq!(t["liste_zamani"]["yaklasik"], true);
    liste_zamanini_yaz(c, 1234).unwrap();
    let z = liste_zamani(c).unwrap();
    assert_eq!((&z["yaklasik"], &z["alinan"]), (&json!(false), &json!(1234)));
    assert_eq!(z["zaman"].as_str().unwrap().len(), 19, "{z}");
    // Dosya türü sütunu ve dosya yılı süzgeci
    let l = al(c, &[("yil", "2025")]);
    assert_eq!((&l["toplam"], &l["satirlar"][0]["dosya_turu"]), (&json!(1), &json!("İcra Dosyası")));
    assert_eq!(al(c, &[("yil", "2025\n2026")])["toplam"], 3);
    assert_eq!(al(c, &[("acilis_yil", "2026"), ("acilis_ay", "10")])["toplam"], 2);
    assert!(al(c, &[])["secenekler"]["dosya_tur"].as_array().unwrap().iter().any(|o| o["deger"] == "İcra Dosyası"));

    c.batch_execute(
        "DELETE FROM dosya; DELETE FROM taraf; DELETE FROM vekil; DELETE FROM birim;
         DELETE FROM buro_ayar WHERE anahtar IN ('eimza_avukat', 'icra_liste_zamani')",
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kapanis_durum_metninden() {
        let g = |y, a, d| NaiveDate::from_ymd_opt(y, a, d);
        assert_eq!(kapanis_coz("Kapalı (Haricen Tahsil - 12.03.2025)"), (g(2025, 3, 12), "Haricen Tahsil".into()));
        assert_eq!(kapanis_coz("Kapalı (İnfaz - 01/02/2024)"), (g(2024, 2, 1), "İnfaz".into()));
        assert_eq!(kapanis_coz("Kapalı (12.03.2025 - Takipten Vazgeçme)"), (g(2025, 3, 12), "Takipten Vazgeçme".into()));
        assert_eq!(kapanis_coz("Kapalı (Birleştirme)"), (None, "Birleştirme".into()));
        assert_eq!(kapanis_coz("Kapalı"), (None, String::new()));
        assert_eq!(kapanis_coz("Kapalı (Hatalı - 31.02.2025)").0, None, "geçersiz gün");
    }

    #[test]
    fn rol_eslestirme() {
        let t = |rol: &str, ad: &str, v: &str| VekilliTaraf { rol: rol.into(), ad: ad.into(), vekiller: v.into() };
        let av = vec!["AYŞE KAYA".to_string()];
        assert_eq!(benim_rolum(&[t("alacakli", "X BANKASI A.Ş.", "Ayşe Kaya")], &av), ("alacakli", vec!["X BANKASI A.Ş.".to_string()]));
        // Bütün vekil verisi içinde: başka avukatlarla birlikte, "Av." ön ekiyle, köşeli parantezle
        assert_eq!(benim_rolum(&[t("borclu", "VELİ KARA", "[MEHMET DEMİR, Av. AYSE  KAYA, ALİ ER]")], &av).0, "borclu");
        assert_eq!(benim_rolum(&[t("alacakli", "X", "AYŞE KAYALI, MEHMET AYŞE")], &av).0, "", "tam kelime: KAYALI eşleşmez");
        assert_eq!(benim_rolum(&[t("haciz alacaklisi", "Y A.Ş.", "AYŞE KAYA")], &av).0, "diger_alacakli");
        assert_eq!(benim_rolum(&[t("rehin alacaklisi", "Y A.Ş.", "AYŞE KAYA")], &av).0, "diger_alacakli");
        assert_eq!(benim_rolum(&[t("ucuncu sahis", "Z", "AYŞE KAYA")], &av).0, "ucuncu");
        // Birden çok taraf: alacaklı önce; aynı türden taraflar birlikte
        let r = benim_rolum(&[t("borclu", "B", "AYŞE KAYA"), t("alacakli", "A1", "AYŞE KAYA, ALİ ER"), t("alacakli", "A2", "AYŞE KAYA"),
                              t("alacakli", "A3", "ALİ ER")], &av);
        assert_eq!(r, ("alacakli", vec!["A1".to_string(), "A2".to_string()]));
        assert_eq!(benim_rolum(&[t("alacakli", "X", "ALİ VELİ")], &av).0, "");
        assert_eq!(benim_rolum(&[t("alacakli", "X", "AYŞE KAYA")], &[]).0, "", "avukat bilinmiyor");
    }

    #[test]
    fn sertifika_sahibi() {
        assert_eq!(sertifika_adi(" AYŞE  KAYA "), "AYŞE KAYA");
        assert_eq!(sertifika_adi("CN=AYŞE KAYA, SERIALNUMBER=12345678901, C=TR"), "AYŞE KAYA");
    }

    #[test]
    fn turkce_siralama() {
        let mut v = vec!["İstanbul 11. İcra Dairesi", "Çorum 1. İcra Dairesi", "İstanbul 2. İcra Dairesi", "Cizre 1. İcra Dairesi",
                         "Ankara 3. İcra Dairesi", "Iğdır 1. İcra Dairesi", "Üsküdar 3. İcra Dairesi", "Uşak 1. İcra Dairesi"];
        v.sort_by_key(|s| tr_sira_anahtari(s));
        assert_eq!(v, vec!["Ankara 3. İcra Dairesi", "Cizre 1. İcra Dairesi", "Çorum 1. İcra Dairesi", "Iğdır 1. İcra Dairesi",
                           "İstanbul 2. İcra Dairesi", "İstanbul 11. İcra Dairesi", "Uşak 1. İcra Dairesi", "Üsküdar 3. İcra Dairesi"]);
    }

    fn satir(acik: bool, takip: bool, tum: bool, kimliksiz: bool) -> Satir {
        Satir {
            dosya: 1, birim: String::new(), dosya_no: String::new(), yil: 2026, sira_no: 1, dosya_tur: String::new(), mts_kaynakli: false, durum: String::new(),
            acik, acilis: None, kapanis: None, kapanis_sebebi: String::new(), alacaklilar: vec![], borclular: vec![], rol: "",
            rol_taraflar: vec![], takip_turu: String::new(), takip_yolu: String::new(), takip_sekli: String::new(),
            takip_eksik: takip, taraf_tum: tum, taraf_kimliksiz: kimliksiz, vekil_okunmamis: false,
        }
    }

    #[test]
    fn istek_tahmini_hesabi() {
        let s = vec![satir(true, true, false, false), satir(true, false, true, true), satir(true, false, false, true), satir(true, false, false, false)];
        // Takip: 1 dosya → bulma 1 + okuma 1
        let t = tahmin_et(&s, "takip", 1200).unwrap();
        assert_eq!((t.dosya, t.hedef, t.istek, t.en_az), (4, 1, 2, false));
        // Taraf: 2 dosya (biri tam: 2 istek, biri kimliksiz: 1) + bulma 2
        let t = tahmin_et(&s, "taraf", 1200).unwrap();
        assert_eq!((t.hedef, t.taraf_tum, t.taraf_kimliksiz, t.istek), (2, 1, 1, 5));
        // Liste: 1200 açık dosya → 3 sayfa, en az
        let t = tahmin_et(&s, "liste", 1200).unwrap();
        assert_eq!((t.hedef, t.istek, t.sayfa, t.en_az), (0, 3, 3, true));
        // Hepsi: liste sayfaları + taraf + takip (bulma yok: dosyalar listeden gelir)
        let t = tahmin_et(&s, "hepsi", 1200).unwrap();
        assert_eq!((t.hedef, t.istek), (3, 3 + 2 + 1 + 1));
        // Çok dosyada tek tek aramak yerine liste bir kez alınır
        let cok: Vec<Satir> = (0..100).map(|_| satir(true, true, false, false)).collect();
        assert_eq!(tahmin_et(&cok, "takip", 600).unwrap().istek, 2 + 100);
        assert!(tahmin_et(&s, "olmaz", 1).is_err());
        // Vekiller okunmamış: kimliksiz + okunmamış 2 istek, yalnız okunmamış 1 istek
        let mut v = vec![satir(true, false, false, true), satir(true, false, false, false)];
        v[0].vekil_okunmamis = true;
        v[1].vekil_okunmamis = true;
        let t = tahmin_et(&v, "taraf", 1200).unwrap();
        assert_eq!((t.hedef, t.vekil_okunmamis, t.istek), (2, 2, 2 + 2 + 1));
        // Eksikler (liste yeniden alınmadan): yalnız açık ve eksiği olan dosyalar; liste sayfası yok, bulma var
        let mut e = vec![satir(true, true, false, false), satir(false, true, false, false), satir(true, false, false, false)];
        e[2].vekil_okunmamis = true;
        let t = tahmin_et(&e, "eksik", 1200).unwrap();
        assert_eq!((t.dosya, t.hedef, t.takip, t.istek, t.en_az), (3, 2, 1, 2 + 1 + 1, false));
    }

    #[test]
    fn yeni_sutunlar_ve_sira() {
        let mut a = satir(true, false, false, false);
        a.dosya_tur = "İcra Dosyası".into();
        a.mts_kaynakli = true;
        a.acilis = NaiveDate::from_ymd_opt(2025, 3, 14);
        a.birim = "Ankara 1. İcra Dairesi".into();
        assert_eq!(a.degerler(Sutun::DosyaTur), vec!["İcra Dosyası (MTS'den)".to_string()]);
        assert_eq!(a.degerler(Sutun::Yil), vec!["2026".to_string()]);
        assert_eq!((a.degerler(Sutun::AcilisYil), a.degerler(Sutun::AcilisAy)), (vec!["2025".to_string()], vec!["3".to_string()]));
        assert_eq!(a.degerler(Sutun::KapanisYil), vec![String::new()]);
        assert_eq!(Sutun::AcilisAy.etiket("3"), "Mart");
        assert_eq!(Sutun::KapanisAy.etiket(""), "(tarihsiz)");
        // Yıl / ay süzgeçleri kodlu: çoklu seçim (satır sonuyla) ve hariç
        assert!(sutun_uyar(Sutun::AcilisAy, "3\n4", &a.degerler(Sutun::AcilisAy)));
        assert!(!sutun_uyar(Sutun::AcilisYil, "2024", &a.degerler(Sutun::AcilisYil)));
        assert!(!sutun_uyar(Sutun::Yil, "!2026", &a.degerler(Sutun::Yil)));

        let mut b = a.clone();
        b.dosya = 2;
        b.birim = "İzmir 2. İcra Dairesi".into();
        b.mts_kaynakli = false;
        b.acilis = NaiveDate::from_ymd_opt(2024, 3, 1);
        let q = |x: &[(&str, &str)]| Filtre::oku(&x.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        // Sırasız (eski kural): dosya türü seçenekleri daire süzgeciyle daralır
        let f = q(&[("birim", "=Ankara 1. İcra Dairesi"), ("dosya_tur", "=İcra Dosyası")]);
        let (gecen, sayim) = suz(vec![a.clone(), b.clone()], &f);
        assert_eq!(gecen.len(), 0, "Ankara dosyası MTS'den, düz İcra Dosyası değil");
        assert_eq!(sayim["dosya_tur"].keys().collect::<Vec<_>>(), vec!["İcra Dosyası (MTS'den)"]);
        // Sıra: dosya türü önce, daire sonra → dosya türünün seçenekleri daireden bağımsız (üstteki alttakini kapsar)
        let f = q(&[("birim", "=Ankara 1. İcra Dairesi"), ("dosya_tur", "=İcra Dosyası"), ("sira", "dosya_tur,birim,olmayan")]);
        assert_eq!(f.sira, vec![Sutun::DosyaTur, Sutun::Birim]);
        let (_, sayim) = suz(vec![a.clone(), b.clone()], &f);
        assert_eq!(sayim["dosya_tur"].len(), 2);
        // Daire seçenekleri yalnız üstteki süzgece (İcra Dosyası) uyanlardan: İzmir
        assert_eq!(sayim["birim"].keys().collect::<Vec<_>>(), vec!["İzmir 2. İcra Dairesi"]);
        // Kapanış süzgeçleri yalnız "Kapalı"da okunur
        assert!(!q(&[("kapanis_yil", "2025")]).sutun.contains_key("kapanis_yil"));
        assert!(q(&[("durum", "kapali"), ("kapanis_yil", "2025")]).sutun.contains_key("kapanis_yil"));
        // Açılış ayı: yıldan bağımsız ("yalnız ay"): Mart → iki dosya
        assert_eq!(suz(vec![a.clone(), b.clone()], &q(&[("acilis_ay", "3")])).0.len(), 2);
        assert_eq!(suz(vec![a, b], &q(&[("acilis_ay", "3"), ("acilis_yil", "2024")])).0.len(), 1);
    }

    #[test]
    fn sutun_suzgeci() {
        let d = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(sutun_uyar(Sutun::Borclu, "mehmet", &d(&["AHMET YILMAZ", "MEHMET YILMAZ"])));
        assert!(!sutun_uyar(Sutun::Borclu, "!MEHMET YILMAZ", &d(&["AHMET YILMAZ", "MEHMET YILMAZ"])));
        assert!(sutun_uyar(Sutun::Rol, "alacakli\nborclu", &d(&["borclu"])));
        assert!(!sutun_uyar(Sutun::Rol, "!borclu", &d(&["borclu"])));
        assert!(sutun_uyar(Sutun::Rol, "!borclu", &d(&[""])));
        assert_eq!(sekil_adi("7", "ÖRNEK: 7 …"), "Örnek 7");
        assert_eq!(sekil_adi("", " Örnek 10 "), "Örnek 10");
    }
}
