//! Stil çözücü: bir paragrafın ve bir parçanın **etkin** biçimini (yazı tipi, boyut, kalın, renk, hizalama,
//! girinti, sekmeler, liste) UDF stil zincirinden hesaplar. Editör ekranı ve PDF çıktısı aynı sonucu kullanır.
//!
//! Kurallar ruyap `udf_pdf.rs` (UYAP çıktısıyla karşılaştırılarak yazılmış UDF→PDF motoru) ile birebir aynıdır;
//! ekranda görünen ile PDF'te çıkan farklı olmasın:
//! - Parça özelliği arama sırası: parçanın kendisi → parçanın `resolver` zinciri → paragraf → paragrafın
//!   `resolver` zinciri → `hvl-default` zinciri. Boş değer yok sayılır.
//! - Renk (`foreground`): parçanın kendi rengi; yoksa zincirde (parça hariç) ilk dolu renk, ama `default` adlı
//!   stilin rengi yok sayılır (UYAP'ın arayüz rengi); yoksa belgenin/kenarın varsayılan rengi (siyah).
//! - Paragraf özelliği: paragraf → paragrafın `resolver` zinciri → ilk alt öğenin aynı adlı özelliği.
//!
//! Claude, 2026-11-23.

use std::collections::{HashMap, HashSet};

use crate::model::*;

/// Varsayılanlar (udf_pdf.rs ile aynı).
pub const VARSAYILAN_AILE: &str = "Times New Roman";
pub const VARSAYILAN_BOYUT: f64 = 12.0;
/// Kenar boşluğu verilmemişse (pt).
pub const VARSAYILAN_MARJ: f64 = 42.525;
pub const A4_GENISLIK: f64 = 595.2756;
pub const A4_YUKSEKLIK: f64 = 841.8898;
/// Sekme durağı tanımlı değilse her 72 pt'de (1 inç) bir durak.
pub const VARSAYILAN_SEKME: f64 = 72.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Renk {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Renk {
    pub const SIYAH: Renk = Renk { r: 0, g: 0, b: 0 };

    /// UDF rengi: işaretli 32 bit ARGB tamsayı (Java `Color.getRGB()`), ör. "-65536" = kırmızı.
    pub fn argb(deger: &str) -> Option<Renk> {
        let mut v = deger.trim().parse::<i64>().ok()?;
        if v < 0 {
            v += 1i64 << 32;
        }
        if !(0..=u32::MAX as i64).contains(&v) {
            return None;
        }
        Some(Renk { r: ((v >> 16) & 255) as u8, g: ((v >> 8) & 255) as u8, b: (v & 255) as u8 })
    }

    /// UDF'e yazılacak biçim (opak).
    pub fn argb_metni(self) -> String {
        let v = 0xFF00_0000u32 | (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32;
        (v as i32).to_string()
    }

    /// CSS / arayüz için "#rrggbb".
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParcaStili {
    /// Belgede yazan aile (ör. "Times New Roman", "Dialog").
    pub aile: String,
    pub boyut: f64,
    pub kalin: bool,
    pub egik: bool,
    pub alti_cizili: bool,
    pub renk: Renk,
}

impl ParcaStili {
    /// Çizimde kullanılacak aile: Java'nın mantıksal yazı tipleri gerçek ailelere eşlenir (udf_pdf.rs
    /// `aile_yedek`). Linux'ta bu aileler ölçüleri aynı Liberation yazı tipleriyle karşılanır.
    pub fn cizim_ailesi(&self) -> &str {
        match self.aile.trim().to_lowercase().as_str() {
            "dialog" | "sansserif" | ".applesystemuifont" => "Arial",
            "dialoginput" | "monospaced" => "Courier New",
            "serif" | "editor" | "source serif pro" | "albertus medium" => "Times New Roman",
            "coronetps" => "Monotype Corsiva",
            _ => self.aile.trim(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Hiza {
    Sol,
    Orta,
    Sag,
    IkiYana,
}

impl Hiza {
    /// UDF `Alignment`: 0 sol, 1 orta, 2 sağ, 3 iki yana.
    pub fn udf(deger: &str) -> Hiza {
        match deger.trim() {
            "1" => Hiza::Orta,
            "2" => Hiza::Sag,
            "3" => Hiza::IkiYana,
            _ => Hiza::Sol,
        }
    }

    pub fn udf_metni(self) -> &'static str {
        match self {
            Hiza::Sol => "0",
            Hiza::Orta => "1",
            Hiza::Sag => "2",
            Hiza::IkiYana => "3",
        }
    }
}

/// Sekme durağı: konum (pt, paragrafın sol girintisine göre) ve hizalama kodu (UDF `TabSet` "konum:hiza:..").
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sekme {
    pub konum: f64,
    pub hiza: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Liste {
    /// Madde imi; `ok`: ok biçimli ("»"), değilse "•".
    Madde { ok: bool },
    Numara { liste_id: String, tur: NumaraTuru },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum NumaraTuru {
    /// "1."
    Nokta,
    /// "1-"
    Tire,
    /// "1)"
    Parantez,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParagrafStili {
    pub hiza: Hiza,
    pub sol: f64,
    pub sag: f64,
    pub ilk_satir: f64,
    /// UDF `LineSpacing` (0–3): satır yüksekliğine eklenen oran.
    pub satir_araligi: f64,
    pub ust_bosluk: f64,
    pub alt_bosluk: f64,
    pub sekmeler: Vec<Sekme>,
    pub liste: Option<Liste>,
}

impl ParagrafStili {
    /// Verilen konumdan (sol girintiye göre, pt) sonraki sekme durağı (udf_pdf.rs `_sonraki_durak`).
    pub fn sonraki_sekme(&self, x: f64) -> Sekme {
        if self.sekmeler.is_empty() {
            return Sekme { konum: ((x / VARSAYILAN_SEKME).floor() + 1.0) * VARSAYILAN_SEKME, hiza: 0 };
        }
        self.sekmeler
            .iter()
            .copied()
            .find(|s| s.konum > x + 0.01)
            .unwrap_or(Sekme { konum: x + 5.0, hiza: 0 })
    }
}

/// UDF `TabSet` ("70.0:0:0,140.0:1:0"): konuma göre sıralı duraklar.
pub fn sekmeleri_coz(tabset: &str) -> Vec<Sekme> {
    let mut v: Vec<Sekme> = tabset
        .split(',')
        .filter_map(|p| {
            let mut a = p.split(':');
            let konum = a.next()?.trim();
            if konum.is_empty() {
                return None;
            }
            let konum = konum.parse::<f64>().unwrap_or(0.0);
            let hiza = a.next().and_then(|h| h.trim().parse::<f64>().ok()).unwrap_or(0.0) as i32;
            Some(Sekme { konum, hiza })
        })
        .collect();
    v.sort_by(|a, b| a.konum.partial_cmp(&b.konum).unwrap_or(std::cmp::Ordering::Equal));
    v
}

fn sayi(d: Option<&str>, varsayilan: f64) -> f64 {
    d.and_then(|s| s.trim().parse::<f64>().ok()).filter(|x| x.is_finite()).unwrap_or(varsayilan)
}

/// Belgenin stillerinden etkin biçim hesaplar.
pub struct StilCozucu<'b> {
    stiller: HashMap<&'b str, &'b Ozellikler>,
}

impl<'b> StilCozucu<'b> {
    pub fn yeni(belge: &'b Belge) -> Self {
        StilCozucu { stiller: belge.stiller.iter().map(|s| (s.ad.as_str(), &s.oz)).collect() }
    }

    /// `ad` stilinden başlayıp `resolver` ile yukarı çıkan zincir (döngü korumalı).
    fn zincir(&self, ad: Option<&str>) -> Vec<(&'b str, &'b Ozellikler)> {
        let mut v = Vec::new();
        let mut gorulen = HashSet::new();
        let mut cur = ad;
        while let Some(a) = cur {
            if a.is_empty() || !gorulen.insert(a) {
                break;
            }
            let Some((ad, oz)) = self.stiller.get_key_value(a) else { break };
            v.push((*ad, *oz));
            cur = oz.al("resolver");
        }
        v
    }

    fn paragraf_zinciri<'a>(&self, p: &'a Paragraf) -> Vec<(&'a str, &'a Ozellikler)>
    where
        'b: 'a,
    {
        let mut v: Vec<(&'a str, &'a Ozellikler)> = vec![("", &p.oz)];
        v.extend(self.zincir(p.oz.al("resolver")));
        v
    }

    /// Paragraf özelliği (udf_pdf.rs `par_attr`).
    fn paragraf_ozelligi<'a>(&self, p: &'a Paragraf, ad: &str) -> Option<&'a str>
    where
        'b: 'a,
    {
        for (_, oz) in self.paragraf_zinciri(p) {
            if let Some(d) = oz.al(ad) {
                return Some(d);
            }
        }
        p.parcalar.first().and_then(|c| c.oz().al(ad))
    }

    pub fn paragraf(&self, p: &Paragraf) -> ParagrafStili {
        let oz = |ad: &str| self.paragraf_ozelligi(p, ad);
        let liste = if oz("Bulleted") == Some("true") {
            Some(Liste::Madde { ok: oz("BulletType").is_some_and(|t| t.contains("ARROW")) })
        } else if oz("Numbered") == Some("true") {
            let t = oz("NumberType").unwrap_or("");
            Some(Liste::Numara {
                liste_id: oz("ListId").unwrap_or("_").to_string(),
                tur: if t.contains("TRE") {
                    NumaraTuru::Tire
                } else if t.contains("PARANTHESE") {
                    NumaraTuru::Parantez
                } else {
                    NumaraTuru::Nokta
                },
            })
        } else {
            None
        };
        ParagrafStili {
            hiza: Hiza::udf(oz("Alignment").unwrap_or("0")),
            sol: sayi(oz("LeftIndent"), 0.0),
            sag: sayi(oz("RightIndent"), 0.0),
            ilk_satir: sayi(oz("FirstLineIndent"), 0.0),
            satir_araligi: sayi(oz("LineSpacing"), 0.0).clamp(0.0, 3.0),
            ust_bosluk: sayi(oz("SpaceAbove"), 0.0).max(0.0),
            alt_bosluk: sayi(oz("SpaceBelow"), 0.0).max(0.0),
            sekmeler: sekmeleri_coz(oz("TabSet").unwrap_or("")),
            liste,
        }
    }

    /// Parçanın etkin stili. `varsayilan_renk`: gövdede siyah; üst/alt bilgide kenarın `foreground` rengi.
    pub fn parca(&self, c: &Parca, p: &Paragraf, varsayilan_renk: Renk) -> ParcaStili {
        let mut zincir: Vec<(&str, &Ozellikler)> = vec![("", c.oz())];
        zincir.extend(self.zincir(c.oz().al("resolver")));
        zincir.extend(self.paragraf_zinciri(p));
        zincir.extend(self.zincir(Some("hvl-default")));

        let bul = |ad: &str| zincir.iter().find_map(|(_, oz)| oz.al(ad).filter(|d| !d.is_empty()));
        let mut boyut = sayi(bul("size"), VARSAYILAN_BOYUT);
        if boyut <= 0.0 {
            boyut = VARSAYILAN_BOYUT;
        }
        let renk = c
            .oz()
            .al("foreground")
            .or_else(|| {
                zincir.iter().skip(1).filter(|(ad, _)| *ad != "default").find_map(|(_, oz)| oz.al("foreground").filter(|d| !d.is_empty()))
            })
            .and_then(Renk::argb)
            .unwrap_or(varsayilan_renk);
        ParcaStili {
            aile: bul("family").unwrap_or(VARSAYILAN_AILE).to_string(),
            boyut,
            kalin: bul("bold") == Some("true"),
            egik: bul("italic") == Some("true"),
            alti_cizili: bul("underline") == Some("true"),
            renk,
        }
    }
}

/// Numaralı liste sayaçları: belge baştan sona dolaşılırken her `ListId` için ayrı sayılır (udf_pdf.rs
/// `_liste_oneki`). Madde imi için sayaç ilerlemez.
#[derive(Default)]
pub struct ListeSayaci(HashMap<String, usize>);

impl ListeSayaci {
    /// Paragrafın başına konacak önek ("•", "»", "3.", "3-", "3)"); liste değilse `None`.
    pub fn onek(&mut self, s: &ParagrafStili) -> Option<String> {
        match s.liste.as_ref()? {
            Liste::Madde { ok } => Some(if *ok { "»" } else { "•" }.to_string()),
            Liste::Numara { liste_id, tur } => {
                let n = self.0.entry(liste_id.clone()).or_insert(0);
                *n += 1;
                Some(match tur {
                    NumaraTuru::Nokta => format!("{n}."),
                    NumaraTuru::Tire => format!("{n}-"),
                    NumaraTuru::Parantez => format!("{n})"),
                })
            }
        }
    }
}

/// Sayfa biçimi (UDF `pageFormat`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SayfaBicimi {
    pub genislik: f64,
    pub yukseklik: f64,
    pub sol: f64,
    pub sag: f64,
    pub ust: f64,
    pub alt: f64,
    pub ust_bilgi_payi: f64,
    pub alt_bilgi_payi: f64,
    pub yatay: bool,
}

impl SayfaBicimi {
    /// udf_pdf.rs ile aynı: `paperOrientation="0"` yatay; öteki her değer dikey A4.
    pub fn coz(sayfa: &Ozellikler) -> SayfaBicimi {
        let yatay = sayfa.al("paperOrientation") == Some("0");
        let (genislik, yukseklik) = if yatay { (A4_YUKSEKLIK, A4_GENISLIK) } else { (A4_GENISLIK, A4_YUKSEKLIK) };
        SayfaBicimi {
            genislik,
            yukseklik,
            sol: sayi(sayfa.al("leftMargin"), VARSAYILAN_MARJ),
            sag: sayi(sayfa.al("rightMargin"), VARSAYILAN_MARJ),
            ust: sayi(sayfa.al("topMargin"), VARSAYILAN_MARJ),
            alt: sayi(sayfa.al("bottomMargin"), VARSAYILAN_MARJ),
            ust_bilgi_payi: sayi(sayfa.al("headerFOffset"), 15.0),
            alt_bilgi_payi: sayi(sayfa.al("footerFOffset"), 15.0),
            yatay,
        }
    }

    /// Yazı alanı genişliği (udf_pdf.rs: en az 50 pt).
    pub fn yazi_genisligi(&self) -> f64 {
        (self.genislik - self.sol - self.sag).max(50.0)
    }
}

// ── Tablo (udf_pdf.rs `_tablo`, `_oranlar`) ────────────────────────────────────────────────────────

/// Hücre iç boşluğu (pt): yatay, dikey.
pub const HUCRE_BOSLUK_X: f64 = 2.0;
pub const HUCRE_BOSLUK_Y: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Kenarlik {
    /// Her hücrenin çevresi (`borderCell`, varsayılan)
    Hucre,
    /// Yalnız tablonun dış çevresi (`borderTable`)
    Tablo,
    /// Kenarlık yok (ör. `borderNone`)
    Yok,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TabloStili {
    pub sol: f64,
    pub sag: f64,
    pub ust_bosluk: f64,
    pub alt_bosluk: f64,
    pub kenarlik: Kenarlik,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SatirBilgisi {
    /// Hücre genişlikleri, toplamı 1 olan oranlar (hücre sayısı kadar).
    pub oranlar: Vec<f64>,
    /// En az yükseklik (pt, `height`).
    pub min_yukseklik: f64,
}

/// "200,300" → [200, 300]; bozuk değer ya da toplam 0 → boş.
pub fn oranlari_coz(metin: Option<&str>) -> Vec<f64> {
    let Some(m) = metin else { return Vec::new() };
    let mut l = Vec::new();
    for x in m.split(',') {
        let t = x.trim();
        if t.is_empty() {
            continue;
        }
        match t.parse::<f64>() {
            Ok(v) if v.is_finite() => l.push(v),
            _ => return Vec::new(),
        }
    }
    if !l.is_empty() && l.iter().sum::<f64>() > 0.0 {
        l
    } else {
        Vec::new()
    }
}

pub fn tablo_stili(t: &Tablo) -> TabloStili {
    TabloStili {
        sol: sayi(t.oz.al("LeftIndent"), 0.0),
        sag: sayi(t.oz.al("RightIndent"), 0.0),
        ust_bosluk: sayi(t.oz.al("SpaceAbove"), 0.0).max(0.0),
        alt_bosluk: sayi(t.oz.al("SpaceBelow"), 0.0).max(0.0),
        kenarlik: match t.oz.al("border").unwrap_or("borderCell") {
            "borderCell" => Kenarlik::Hucre,
            "borderTable" => Kenarlik::Tablo,
            _ => Kenarlik::Yok,
        },
    }
}

/// Satırın hücre oranları: satırın `columnSpans`'i hücre sayısını tutuyorsa o, yoksa tablonunki, o da yoksa eşit.
pub fn satir_bilgisi(t: &Tablo, r: &Satir) -> SatirBilgisi {
    let n = r.hucreler.len();
    let mut oran = oranlari_coz(r.oz.al("columnSpans"));
    if oran.len() != n {
        let tablo = oranlari_coz(t.oz.al("columnSpans"));
        oran = if tablo.len() == n { tablo } else { vec![1.0; n] };
    }
    let top: f64 = oran.iter().sum();
    SatirBilgisi {
        oranlar: if top > 0.0 { oran.iter().map(|x| x / top).collect() } else { oran },
        min_yukseklik: sayi(r.oz.al("height"), 0.0).max(0.0),
    }
}
