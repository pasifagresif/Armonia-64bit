//! DOCX (Word) okuma: belge modeline (UDF yapısı) çevirir.
//!
//! Word stilleri (docDefaults → paragraf stili zinciri → karakter stili → doğrudan biçim), tema yazı tipleri,
//! numaralandırma tanımları, tablolar (birleşik hücreler), resimler, üst/alt bilgi ve sayfa numarası çözülür ve
//! her paragrafa / parçaya **açık** UDF özelliği olarak yazılır (UDF'te Word stillerinin karşılığı yok).
//! Modelde karşılığı olmayan öğeler (dipnot, yorum, izlenen değişiklik, metin kutusu, üst simge...) sessizce
//! atılmaz: `uyarilar` listesinde kullanıcıya bildirilir.
//!
//! Güvenilmeyen dosya: UDF okuyucusuyla aynı boyut / dosya sayısı sınırları, DTD kapalı. Claude, 2026-11-23.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use base64::Engine as _;
use roxmltree::{Document, Node, ParsingOptions};
use zip::ZipArchive;

use crate::model::*;
use crate::stil::Renk;
use crate::udf::{AZAMI_ACILMIS, AZAMI_DOSYA_SAYISI, AZAMI_GIRDI, AZAMI_TOPLAM, AZAMI_XML_DUGUM};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// UYAP editörünün resim parçası için metinde tuttuğu yer tutucu karakter (gerçek UDF'ten).
const RESIM_YER_TUTUCU: &str = "\u{b8}";
/// A4 (twip)
const A4_W: f64 = 11906.0;
const A4_H: f64 = 16838.0;

#[derive(Debug)]
pub struct DocxOkuma {
    pub belge: Belge,
    /// Korunamayan / yaklaşık açılan öğeler. Boşsa belge desteklenen biçimlerle açıldı.
    pub uyarilar: Vec<String>,
}

/// Bayt dizisi DOCX mi (ZIP ve içinde word/document.xml)?
pub fn docx_mi(bayt: &[u8]) -> bool {
    bayt.starts_with(b"PK")
        && ZipArchive::new(Cursor::new(bayt)).is_ok_and(|mut z| z.by_name("word/document.xml").is_ok())
}

// ── Biçim özellikleri ───────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Default, Debug)]
struct RunOz {
    aile: Option<String>,
    tema_aile: Option<String>,
    boyut: Option<f64>,
    kalin: Option<bool>,
    egik: Option<bool>,
    alti: Option<bool>,
    renk: Option<Option<Renk>>,
    gizli: Option<bool>,
    desteksiz: bool,
}

impl RunOz {
    fn ust(&mut self, o: &RunOz) {
        if o.aile.is_some() {
            self.aile.clone_from(&o.aile);
            self.tema_aile = None;
        }
        if o.tema_aile.is_some() {
            self.tema_aile.clone_from(&o.tema_aile);
            self.aile = None;
        }
        macro_rules! al {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f.clone(); } )* };
        }
        al!(boyut, kalin, egik, alti, renk, gizli);
        self.desteksiz |= o.desteksiz;
    }
}

#[derive(Clone, Default, Debug)]
struct ParOz {
    hiza: Option<&'static str>,
    sol: Option<f64>,
    sag: Option<f64>,
    ilk: Option<f64>,
    ust_bosluk: Option<f64>,
    alt_bosluk: Option<f64>,
    satir: Option<(f64, String)>,
    sekmeler: Option<Vec<(f64, i32)>>,
    numara: Option<(String, u32)>,
}

impl ParOz {
    fn ust(&mut self, o: &ParOz) {
        macro_rules! al {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f.clone(); } )* };
        }
        al!(hiza, sol, sag, ilk, ust_bosluk, alt_bosluk, satir, sekmeler, numara);
    }
}

fn wa<'a>(n: Node<'a, '_>, ad: &str) -> Option<&'a str> {
    n.attribute((W, ad))
}

fn cocuk<'a, 'i>(n: Node<'a, 'i>, ad: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == ad && c.tag_name().namespace() == Some(W))
}

fn twip(d: Option<&str>) -> Option<f64> {
    d.and_then(|s| s.trim().parse::<f64>().ok()).filter(|x| x.is_finite())
}

/// `<w:b/>`, `<w:b w:val="0"/>`, `<w:b w:val="false"/>`
fn acik_mi(n: Node) -> bool {
    !matches!(wa(n, "val"), Some("0" | "false" | "off" | "none"))
}

fn run_oz_coz(rpr: Node, u: &mut Uyarici) -> RunOz {
    let mut o = RunOz::default();
    for c in rpr.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(W)) {
        match c.tag_name().name() {
            "rFonts" => {
                if let Some(a) = wa(c, "ascii").or_else(|| wa(c, "hAnsi")) {
                    o.aile = Some(a.to_string());
                } else if let Some(t) = wa(c, "asciiTheme").or_else(|| wa(c, "hAnsiTheme")) {
                    o.tema_aile = Some(t.to_string());
                }
            }
            "sz" => o.boyut = twip(wa(c, "val")).map(|x| x / 2.0),
            "b" => o.kalin = Some(acik_mi(c)),
            "i" => o.egik = Some(acik_mi(c)),
            "u" => o.alti = Some(acik_mi(c)),
            "color" => {
                o.renk = Some(match wa(c, "val") {
                    Some("auto") | None => None,
                    Some(h) => u32::from_str_radix(h, 16).ok().map(|v| Renk { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }),
                })
            }
            "vanish" => o.gizli = Some(acik_mi(c)),
            "strike" | "dstrike" | "caps" | "smallCaps" | "vertAlign" | "highlight" | "shd"
                if acik_mi(c) && (c.tag_name().name() != "vertAlign" || matches!(wa(c, "val"), Some("superscript" | "subscript"))) =>
            {
                o.desteksiz = true;
                u.uyar("Üstü çizili, üst/alt simge, büyük harf, vurgulama gibi bazı yazı biçimleri korunmadı");
            }
            _ => {}
        }
    }
    o
}

fn par_oz_coz(ppr: Node) -> ParOz {
    let mut o = ParOz::default();
    for c in ppr.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(W)) {
        match c.tag_name().name() {
            "jc" => {
                o.hiza = Some(match wa(c, "val") {
                    Some("center") => "1",
                    Some("right" | "end") => "2",
                    Some("both" | "distribute" | "justify") => "3",
                    _ => "0",
                })
            }
            "ind" => {
                o.sol = twip(wa(c, "left").or_else(|| wa(c, "start"))).map(|x| x / 20.0).or(o.sol);
                o.sag = twip(wa(c, "right").or_else(|| wa(c, "end"))).map(|x| x / 20.0).or(o.sag);
                if let Some(h) = twip(wa(c, "hanging")) {
                    o.ilk = Some(-h / 20.0);
                } else if let Some(f) = twip(wa(c, "firstLine")) {
                    o.ilk = Some(f / 20.0);
                }
            }
            "spacing" => {
                if let Some(x) = twip(wa(c, "before")) {
                    o.ust_bosluk = Some(x / 20.0);
                }
                if let Some(x) = twip(wa(c, "after")) {
                    o.alt_bosluk = Some(x / 20.0);
                }
                if let Some(x) = twip(wa(c, "line")) {
                    o.satir = Some((x, wa(c, "lineRule").unwrap_or("auto").to_string()));
                }
            }
            "tabs" => {
                let mut v: Vec<(f64, i32)> = c
                    .children()
                    .filter(|t| t.is_element() && t.tag_name().name() == "tab")
                    .filter(|t| wa(*t, "val") != Some("clear"))
                    .filter_map(|t| {
                        let pos = twip(wa(t, "pos"))? / 20.0;
                        // UDF TabSet hizası (udf_pdf.rs): 0 sol, 2 orta, 1 sağ, 4 ondalık
                        let h = match wa(t, "val") {
                            Some("center") => 2,
                            Some("right" | "end") => 1,
                            Some("decimal") => 4,
                            _ => 0,
                        };
                        Some((pos, h))
                    })
                    .collect();
                v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                o.sekmeler = Some(v);
            }
            "numPr" => {
                let id = cocuk(c, "numId").and_then(|n| wa(n, "val")).unwrap_or("0").to_string();
                let lvl = cocuk(c, "ilvl").and_then(|n| wa(n, "val")).and_then(|s| s.parse().ok()).unwrap_or(0);
                o.numara = Some((id, lvl));
            }
            _ => {}
        }
    }
    o
}

// ── Uyarılar ────────────────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Uyarici(Vec<String>);

impl Uyarici {
    fn uyar(&mut self, m: &str) {
        if !self.0.iter().any(|x| x == m) {
            self.0.push(m.to_string());
        }
    }
}

// ── Stiller, tema, numaralandırma ──────────────────────────────────────────────────────────────────

struct WStil {
    tur: String,
    temel: Option<String>,
    p: ParOz,
    r: RunOz,
    kenarlikli: Option<bool>,
}

#[derive(Default)]
struct Stiller {
    s: HashMap<String, WStil>,
    varsayilan_p: ParOz,
    varsayilan_r: RunOz,
    varsayilan_paragraf: Option<String>,
    tema_minor: Option<String>,
    tema_major: Option<String>,
}

impl Stiller {
    fn zincir(&self, id: Option<&str>) -> Vec<&WStil> {
        let mut v = Vec::new();
        let mut cur = id.map(str::to_string);
        let mut sayac = 0;
        while let Some(i) = cur {
            let Some(s) = self.s.get(&i) else { break };
            v.push(s);
            cur = s.temel.clone();
            sayac += 1;
            if sayac > 32 {
                break;
            }
        }
        v.reverse(); // en temelden en özele
        v
    }

    fn tema(&self, t: &str) -> Option<String> {
        if t.starts_with("major") {
            self.tema_major.clone()
        } else {
            self.tema_minor.clone()
        }
    }
}

struct NumSeviye {
    madde: bool,
    numara_turu: String,
    p: ParOz,
}

#[derive(Default)]
struct Numaralar {
    num: HashMap<String, String>,
    soyut: HashMap<String, HashMap<u32, NumSeviye>>,
}

impl Numaralar {
    fn seviye(&self, num_id: &str, lvl: u32) -> Option<&NumSeviye> {
        self.soyut.get(self.num.get(num_id)?)?.get(&lvl)
    }
}

fn xml_ac(veri: &[u8]) -> Option<String> {
    String::from_utf8(veri.to_vec()).ok()
}

fn ayristir(xml: &str) -> Result<Document<'_>, String> {
    Document::parse_with_options(xml, ParsingOptions { allow_dtd: false, nodes_limit: AZAMI_XML_DUGUM }).map_err(|e| e.to_string())
}

fn stilleri_oku(xml: Option<&str>, tema: Option<&str>, u: &mut Uyarici) -> Stiller {
    let mut st = Stiller::default();
    if let Some(t) = tema.and_then(|t| ayristir(t).ok()) {
        let latin = |ad: &str| {
            t.descendants()
                .find(|n| n.tag_name().name() == ad && n.tag_name().namespace() == Some(A))
                .and_then(|n| n.children().find(|c| c.tag_name().name() == "latin"))
                .and_then(|n| n.attribute("typeface"))
                .map(str::to_string)
        };
        st.tema_minor = latin("minorFont");
        st.tema_major = latin("majorFont");
    }
    let Some(xml) = xml else { return st };
    let Ok(d) = ayristir(xml) else { return st };
    let kok = d.root_element();
    if let Some(dd) = cocuk(kok, "docDefaults") {
        if let Some(r) = cocuk(dd, "rPrDefault").and_then(|n| cocuk(n, "rPr")) {
            st.varsayilan_r = run_oz_coz(r, u);
        }
        if let Some(p) = cocuk(dd, "pPrDefault").and_then(|n| cocuk(n, "pPr")) {
            st.varsayilan_p = par_oz_coz(p);
        }
    }
    for s in kok.children().filter(|c| c.is_element() && c.tag_name().name() == "style") {
        let Some(id) = wa(s, "styleId") else { continue };
        let tur = wa(s, "type").unwrap_or("paragraph").to_string();
        if tur == "paragraph" && wa(s, "default").is_some_and(|d| d == "1" || d == "true") {
            st.varsayilan_paragraf = Some(id.to_string());
        }
        let kenarlikli = cocuk(s, "tblPr").and_then(|t| cocuk(t, "tblBorders")).map(kenarlik_var);
        st.s.insert(
            id.to_string(),
            WStil {
                tur,
                temel: cocuk(s, "basedOn").and_then(|n| wa(n, "val")).map(str::to_string),
                p: cocuk(s, "pPr").map(par_oz_coz).unwrap_or_default(),
                r: cocuk(s, "rPr").map(|r| run_oz_coz(r, u)).unwrap_or_default(),
                kenarlikli,
            },
        );
    }
    st
}

fn numaralari_oku(xml: Option<&str>) -> Numaralar {
    let mut n = Numaralar::default();
    let Some(d) = xml.and_then(|x| ayristir(x).ok()) else { return n };
    let kok = d.root_element();
    for a in kok.children().filter(|c| c.is_element() && c.tag_name().name() == "abstractNum") {
        let Some(id) = wa(a, "abstractNumId") else { continue };
        let mut sev = HashMap::new();
        for l in a.children().filter(|c| c.is_element() && c.tag_name().name() == "lvl") {
            let ilvl: u32 = wa(l, "ilvl").and_then(|s| s.parse().ok()).unwrap_or(0);
            let fmt = cocuk(l, "numFmt").and_then(|x| wa(x, "val")).unwrap_or("decimal");
            let metin = cocuk(l, "lvlText").and_then(|x| wa(x, "val")).unwrap_or("%1.");
            let son = if metin.trim_end().ends_with(')') {
                "PARANTHESE"
            } else if metin.trim_end().ends_with('-') {
                "TRE"
            } else {
                "DOT"
            };
            let kok_tur = match fmt {
                "lowerLetter" => "CHAR_SMALL",
                "upperLetter" => "CHAR_BIG",
                "lowerRoman" => "ROMAN_SMALL",
                "upperRoman" => "ROMAN_BIG",
                _ => "NUMBER",
            };
            sev.insert(
                ilvl,
                NumSeviye {
                    madde: fmt == "bullet",
                    numara_turu: format!("NUMBER_TYPE_{kok_tur}_{son}"),
                    p: cocuk(l, "pPr").map(par_oz_coz).unwrap_or_default(),
                },
            );
        }
        n.soyut.insert(id.to_string(), sev);
    }
    for m in kok.children().filter(|c| c.is_element() && c.tag_name().name() == "num") {
        if let (Some(id), Some(a)) = (wa(m, "numId"), cocuk(m, "abstractNumId").and_then(|x| wa(x, "val"))) {
            n.num.insert(id.to_string(), a.to_string());
        }
    }
    n
}

fn kenarlik_var(b: Node) -> bool {
    b.children()
        .filter(|c| c.is_element())
        .any(|c| !matches!(wa(c, "val"), Some("nil" | "none") | None))
}

fn iliskiler(xml: Option<&str>) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Some(d) = xml.and_then(|x| ayristir(x).ok()) {
        for r in d.root_element().children().filter(|c| c.is_element()) {
            if let (Some(id), Some(t)) = (r.attribute("Id"), r.attribute("Target")) {
                m.insert(id.to_string(), t.to_string());
            }
        }
    }
    m
}

/// "media/image1.png" (word/ klasörüne göre) ya da "/word/media/..." → ZIP içi ad.
fn hedef_yolu(hedef: &str) -> String {
    if let Some(m) = hedef.strip_prefix('/') {
        m.to_string()
    } else {
        let mut parcalar: Vec<&str> = vec!["word"];
        for p in hedef.split('/') {
            match p {
                ".." => {
                    parcalar.pop();
                }
                "." | "" => {}
                x => parcalar.push(x),
            }
        }
        parcalar.join("/")
    }
}

// ── Okuyucu ─────────────────────────────────────────────────────────────────────────────────────────

struct Okuyucu<'z> {
    dosyalar: &'z HashMap<String, Vec<u8>>,
    stiller: Stiller,
    numaralar: Numaralar,
    u: Uyarici,
}

/// Bir paragraf okunurken biriken parçalar.
struct ParcaToplayici {
    parcalar: Vec<Parca>,
    son_oz: Option<Ozellikler>,
}

impl ParcaToplayici {
    fn metin(&mut self, m: &str, oz: Ozellikler) {
        if m.is_empty() {
            return;
        }
        // Aynı biçimli ardışık metin tek parça
        if let Some(Parca::Metin { metin, oz: o }) = self.parcalar.last_mut() {
            if *o == oz {
                metin.push_str(m);
                return;
            }
        }
        self.son_oz = Some(oz.clone());
        self.parcalar.push(Parca::Metin { metin: m.to_string(), oz });
    }
}

impl Okuyucu<'_> {
    /// Parçanın stil kaynaklı biçimi: docDefaults → paragraf stili zinciri → karakter stili zinciri.
    /// (Doğrudan biçim çağıran tarafta eklenir.)
    fn etkin_run(&self, p_stil: Option<&str>, r: Node) -> RunOz {
        let mut o = self.stiller.varsayilan_r.clone();
        for s in self.stiller.zincir(p_stil) {
            o.ust(&s.r);
        }
        if let Some(rs) = cocuk(r, "rPr").and_then(|rpr| cocuk(rpr, "rStyle")).and_then(|n| wa(n, "val")) {
            for s in self.stiller.zincir(Some(rs)) {
                if s.tur == "character" || s.tur == "paragraph" {
                    o.ust(&s.r);
                }
            }
        }
        o
    }

    fn run_ozellikleri(&mut self, o: &RunOz) -> Ozellikler {
        let mut oz = Ozellikler::default();
        let aile = o
            .aile
            .clone()
            .or_else(|| o.tema_aile.as_deref().and_then(|t| self.stiller.tema(t)))
            .unwrap_or_else(|| "Times New Roman".to_string());
        oz.ayarla("family", aile);
        let boyut = o.boyut.unwrap_or(10.0);
        oz.ayarla("size", if boyut.fract() == 0.0 { format!("{}", boyut as i64) } else { format!("{boyut}") });
        if o.kalin == Some(true) {
            oz.ayarla("bold", "true");
        }
        if o.egik == Some(true) {
            oz.ayarla("italic", "true");
        }
        if o.alti == Some(true) {
            oz.ayarla("underline", "true");
        }
        if let Some(Some(r)) = o.renk {
            if r != Renk::SIYAH {
                oz.ayarla("foreground", r.argb_metni());
            }
        }
        oz
    }

    fn paragraf_oz(&self, p: Node) -> (ParOz, Option<String>) {
        let ppr = cocuk(p, "pPr");
        let p_stil = ppr
            .and_then(|x| cocuk(x, "pStyle"))
            .and_then(|n| wa(n, "val"))
            .map(str::to_string)
            .or_else(|| self.stiller.varsayilan_paragraf.clone());
        let mut o = self.stiller.varsayilan_p.clone();
        for s in self.stiller.zincir(p_stil.as_deref()) {
            o.ust(&s.p);
        }
        let dogrudan = ppr.map(par_oz_coz).unwrap_or_default();
        // Numaralandırma seviyesinin girintisi stilden sonra, doğrudan biçimden önce
        let numara = dogrudan.numara.clone().or_else(|| o.numara.clone());
        if let Some((id, lvl)) = &numara {
            if let Some(sv) = self.numaralar.seviye(id, *lvl) {
                o.ust(&sv.p);
            }
        }
        o.ust(&dogrudan);
        o.numara = numara;
        (o, p_stil)
    }

    fn paragraf_ozellikleri(&self, o: &ParOz, boyut: f64) -> Ozellikler {
        let mut oz = Ozellikler::default();
        oz.ayarla("Alignment", o.hiza.unwrap_or("0"));
        let mut yaz = |ad: &str, d: Option<f64>| {
            if let Some(x) = d.filter(|x| x.abs() > 1e-9) {
                oz.ayarla(ad, format!("{}", (x * 100.0).round() / 100.0));
            }
        };
        yaz("LeftIndent", o.sol);
        yaz("RightIndent", o.sag);
        yaz("FirstLineIndent", o.ilk);
        yaz("SpaceAbove", o.ust_bosluk);
        yaz("SpaceBelow", o.alt_bosluk);
        if let Some((deger, kural)) = &o.satir {
            // UDF LineSpacing: tek satıra eklenen oran (0–3). Word "auto": 240 = tek satır.
            let ls = if kural == "auto" { deger / 240.0 - 1.0 } else { (deger / 20.0) / (boyut * 1.16) - 1.0 };
            yaz("LineSpacing", Some(ls.clamp(0.0, 3.0)));
        }
        if let Some(s) = o.sekmeler.as_ref().filter(|s| !s.is_empty()) {
            oz.ayarla("TabSet", s.iter().map(|(p, h)| format!("{p}:{h}:0")).collect::<Vec<_>>().join(","));
        }
        if let Some((id, lvl)) = &o.numara {
            if id != "0" {
                if let Some(sv) = self.numaralar.seviye(id, *lvl) {
                    if sv.madde {
                        oz.ayarla("Bulleted", "true");
                        oz.ayarla("BulletType", "BULLET_TYPE_ELLIPSE");
                    } else {
                        oz.ayarla("Numbered", "true");
                        oz.ayarla("NumberType", sv.numara_turu.as_str());
                    }
                    oz.ayarla("ListId", id.as_str());
                    oz.ayarla("ListLevel", (lvl + 1).to_string());
                }
            }
        }
        oz
    }

    /// Paragraf: satır sonu (w:br) paragrafı böler; bu yüzden birden çok UDF paragrafı dönebilir.
    fn paragraf(&mut self, p: Node, iliski: &HashMap<String, String>, kenar: bool) -> Vec<Paragraf> {
        let (po, p_stil) = self.paragraf_oz(p);
        if cocuk(p, "pPr").and_then(|x| cocuk(x, "sectPr")).is_some() {
            self.u.uyar("Belgede birden çok bölüm var; sayfa düzeni olarak son bölümünki kullanıldı");
        }
        let mut cikti: Vec<Paragraf> = Vec::new();
        let mut t = ParcaToplayici { parcalar: Vec::new(), son_oz: None };
        let mut alan_kodu = false;
        self.icerik(p, iliski, p_stil.as_deref(), &mut t, &mut cikti, &po, &mut alan_kodu, kenar);
        self.paragrafi_bitir(&mut t, &mut cikti, &po, p_stil.as_deref(), p);
        cikti
    }

    #[allow(clippy::too_many_arguments)]
    fn icerik(&mut self, n: Node, iliski: &HashMap<String, String>, p_stil: Option<&str>, t: &mut ParcaToplayici,
              cikti: &mut Vec<Paragraf>, po: &ParOz, alan_kodu: &mut bool, kenar: bool) {
        for c in n.children().filter(|c| c.is_element()) {
            let ns = c.tag_name().namespace();
            let ad = c.tag_name().name();
            if ns == Some(MC) && ad == "AlternateContent" {
                if let Some(ch) = c.children().find(|x| x.tag_name().name() == "Choice") {
                    self.icerik(ch, iliski, p_stil, t, cikti, po, alan_kodu, kenar);
                }
                continue;
            }
            if ns != Some(W) {
                continue;
            }
            match ad {
                "r" => self.run(c, iliski, p_stil, t, cikti, po, alan_kodu, kenar),
                "hyperlink" | "smartTag" | "ins" | "customXml" | "fldSimple" => {
                    if ad == "ins" {
                        self.u.uyar("İzlenen değişiklikler: eklenenler kabul edilmiş, silinenler alınmamış olarak açıldı");
                    }
                    if ad == "fldSimple" && kenar && wa(c, "instr").is_some_and(|i| i.contains("PAGE")) {
                        t.parcalar.push(Parca::Alan { metin: "1".into(), oz: Ozellikler(vec![("fieldName".into(), "pageNumber".into())]) });
                        continue;
                    }
                    self.icerik(c, iliski, p_stil, t, cikti, po, alan_kodu, kenar);
                }
                "sdt" => {
                    if let Some(ic) = cocuk(c, "sdtContent") {
                        self.icerik(ic, iliski, p_stil, t, cikti, po, alan_kodu, kenar);
                    }
                }
                "del" => self.u.uyar("İzlenen değişiklikler: eklenenler kabul edilmiş, silinenler alınmamış olarak açıldı"),
                "commentRangeStart" | "commentRangeEnd" => self.u.uyar("Yorumlar korunmadı"),
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run(&mut self, r: Node, iliski: &HashMap<String, String>, p_stil: Option<&str>, t: &mut ParcaToplayici,
           _cikti: &mut [Paragraf], _po: &ParOz, alan_kodu: &mut bool, kenar: bool) {
        let mut ro = self.etkin_run(p_stil, r);
        if let Some(rpr) = cocuk(r, "rPr") {
            let d = run_oz_coz(rpr, &mut self.u);
            ro.ust(&d);
        }
        if ro.gizli == Some(true) {
            return;
        }
        let oz = self.run_ozellikleri(&ro);
        for c in r.children().filter(|c| c.is_element()) {
            let ns = c.tag_name().namespace();
            let ad = c.tag_name().name();
            if ns == Some(MC) && ad == "AlternateContent" {
                if let Some(ch) = c.children().find(|x| x.tag_name().name() == "Choice") {
                    for d in ch.children().filter(|x| x.is_element()) {
                        if d.tag_name().name() == "drawing" {
                            self.resim(d, iliski, t, &oz);
                        }
                    }
                }
                continue;
            }
            if ns != Some(W) {
                continue;
            }
            match ad {
                "t" if !*alan_kodu => t.metin(c.text().unwrap_or(""), oz.clone()),
                "tab" if !*alan_kodu => {
                    t.son_oz = Some(oz.clone());
                    t.parcalar.push(Parca::Sekme { metin: "\t".into(), oz: oz.clone() });
                }
                "br" | "cr" => {
                    if wa(c, "type") == Some("page") {
                        self.u.uyar("Sayfa sonları korunmadı (metin akışı devam ediyor)");
                    } else {
                        // Satır sonu (Shift+Enter): paragrafın içinde "\n" (UYAP da paragraf içi satır sonunu
                        // böyle tutuyor; gerçek UDF'te görüldü). Paragraf sonu ayrıca eklenir.
                        t.metin("\n", oz.clone());
                    }
                }
                "noBreakHyphen" => t.metin("-", oz.clone()),
                "sym" => {
                    if let Some(ch) = wa(c, "char").and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32) {
                        t.metin(&ch.to_string(), oz.clone());
                    }
                }
                "fldChar" => match wa(c, "fldCharType") {
                    Some("begin") => *alan_kodu = true,
                    Some("separate") | Some("end") => *alan_kodu = false,
                    _ => {}
                },
                "instrText" => {
                    if kenar && c.text().is_some_and(|x| x.contains("PAGE") && !x.contains("NUMPAGES")) {
                        t.parcalar.push(Parca::Alan { metin: "1".into(), oz: Ozellikler(vec![("fieldName".into(), "pageNumber".into())]) });
                    }
                }
                "drawing" => self.resim(c, iliski, t, &oz),
                "pict" | "object" => self.u.uyar("Eski biçim çizimler / gömülü nesneler korunmadı"),
                "footnoteReference" | "endnoteReference" => self.u.uyar("Dipnotlar ve son notlar korunmadı"),
                "commentReference" => self.u.uyar("Yorumlar korunmadı"),
                _ => {}
            }
        }
    }

    fn resim(&mut self, d: Node, iliski: &HashMap<String, String>, t: &mut ParcaToplayici, oz: &Ozellikler) {
        if d.descendants().any(|x| x.tag_name().name() == "txbx" || x.tag_name().name() == "txbxContent") {
            self.u.uyar("Metin kutuları korunmadı");
            return;
        }
        if d.children().any(|x| x.tag_name().name() == "anchor") {
            self.u.uyar("Kayan (metin çevresine yerleşen) resimler satır içi resim olarak açıldı");
        }
        let kap = d.children().find(|x| x.is_element() && x.tag_name().namespace() == Some(WP));
        let Some(kap) = kap else { return };
        let olcu = kap.children().find(|x| x.tag_name().name() == "extent");
        let emu = |ad: &str| olcu.and_then(|e| e.attribute(ad)).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let (g, y) = (emu("cx") / 12700.0, emu("cy") / 12700.0);
        let Some(id) = kap.descendants().find(|x| x.tag_name().name() == "blip").and_then(|b| b.attribute((R, "embed"))) else {
            self.u.uyar("Bağlantılı (dosyaya gömülmemiş) resimler korunmadı");
            return;
        };
        let Some(veri) = iliski.get(id).and_then(|h| self.dosyalar.get(&hedef_yolu(h))) else { return };
        let b64 = base64::engine::general_purpose::STANDARD.encode(veri);
        let mut roz = Ozellikler::default();
        roz.ayarla("width", format!("{}", (g * 100.0).round() / 100.0));
        roz.ayarla("height", format!("{}", (y * 100.0).round() / 100.0));
        if let Some(f) = oz.al("family") {
            roz.ayarla("family", f);
        }
        roz.ayarla("imageData", b64);
        t.parcalar.push(Parca::Resim { metin: RESIM_YER_TUTUCU.into(), oz: roz });
    }

    fn paragrafi_bitir(&mut self, t: &mut ParcaToplayici, cikti: &mut Vec<Paragraf>, po: &ParOz, p_stil: Option<&str>, kaynak: Node) {
        // Paragraf sonu "\n": son metnin biçimiyle (UYAP gibi ayrı parça); boş paragrafta paragraf işaretinin biçimi
        let oz = t.son_oz.clone().unwrap_or_else(|| {
            let mut ro = self.etkin_run(p_stil, kaynak);
            if let Some(r) = cocuk(kaynak, "pPr").and_then(|x| cocuk(x, "rPr")) {
                let d = run_oz_coz(r, &mut self.u);
                ro.ust(&d);
            }
            self.run_ozellikleri(&ro)
        });
        let boyut = oz.sayi("size").unwrap_or(12.0);
        let mut parcalar = std::mem::take(&mut t.parcalar);
        parcalar.push(Parca::Metin { metin: "\n".into(), oz });
        cikti.push(Paragraf { oz: self.paragraf_ozellikleri(po, boyut), parcalar });
        t.son_oz = None;
    }

    fn tablo(&mut self, tbl: Node, iliski: &HashMap<String, String>, kenar: bool) -> Tablo {
        let tpr = cocuk(tbl, "tblPr");
        let izgara: Vec<f64> = cocuk(tbl, "tblGrid")
            .map(|g| g.children().filter(|c| c.tag_name().name() == "gridCol").map(|c| twip(wa(c, "w")).unwrap_or(0.0)).collect())
            .unwrap_or_default();
        let mut oz = Ozellikler::default();
        if !izgara.is_empty() && izgara.iter().sum::<f64>() > 0.0 {
            oz.ayarla("columnSpans", izgara.iter().map(|w| format!("{}", (w / 20.0 * 100.0).round() / 100.0)).collect::<Vec<_>>().join(","));
        }
        // Kenarlık: doğrudan tblBorders, yoksa tablo stilininki; hiçbiri yoksa kenarlıksız
        let dogrudan = tpr.and_then(|t| cocuk(t, "tblBorders")).map(kenarlik_var);
        let stilden = tpr
            .and_then(|t| cocuk(t, "tblStyle"))
            .and_then(|n| wa(n, "val"))
            .and_then(|id| self.stiller.zincir(Some(id)).iter().rev().find_map(|s| s.kenarlikli));
        oz.ayarla("border", if dogrudan.or(stilden).unwrap_or(false) { "borderCell" } else { "borderNone" });
        if let Some(i) = tpr.and_then(|t| cocuk(t, "tblInd")).and_then(|n| twip(wa(n, "w"))) {
            if i.abs() > 1e-9 {
                oz.ayarla("LeftIndent", format!("{}", i / 20.0));
            }
        }
        let mut satirlar = Vec::new();
        for tr in tbl.children().filter(|c| c.is_element() && c.tag_name().name() == "tr") {
            let mut roz = Ozellikler::default();
            if let Some(h) = cocuk(tr, "trPr").and_then(|p| cocuk(p, "trHeight")).and_then(|n| twip(wa(n, "val"))) {
                roz.ayarla("height", format!("{}", h / 20.0));
            }
            let mut hucreler = Vec::new();
            let mut genislikler = Vec::new();
            let mut sutun = 0usize;
            let mut birlesik = false;
            for tc in tr.children().filter(|c| c.is_element() && c.tag_name().name() == "tc") {
                let tcpr = cocuk(tc, "tcPr");
                let kapla: usize = tcpr.and_then(|p| cocuk(p, "gridSpan")).and_then(|n| wa(n, "val")).and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
                let w: f64 = (sutun..sutun + kapla).map(|i| izgara.get(i).copied().unwrap_or(0.0)).sum();
                genislikler.push(w);
                birlesik |= kapla > 1;
                sutun += kapla;
                let devam = tcpr.and_then(|p| cocuk(p, "vMerge")).is_some_and(|v| wa(v, "val").is_none_or(|x| x == "continue"));
                let bloklar = if devam {
                    self.u.uyar("Dikey birleştirilmiş hücreler ayrı hücreler olarak açıldı");
                    vec![Blok::Paragraf(Paragraf { oz: Ozellikler(vec![("Alignment".into(), "0".into())]), parcalar: vec![Parca::Metin { metin: "\n".into(), oz: Ozellikler::default() }] })]
                } else {
                    self.bloklar(tc, iliski, kenar)
                };
                hucreler.push(Hucre { oz: Ozellikler::default(), bloklar });
            }
            if (birlesik || hucreler.len() != izgara.len()) && genislikler.iter().sum::<f64>() > 0.0 {
                roz.ayarla("columnSpans", genislikler.iter().map(|w| format!("{}", (w / 20.0 * 100.0).round() / 100.0)).collect::<Vec<_>>().join(","));
            }
            satirlar.push(Satir { oz: roz, hucreler });
        }
        Tablo { oz, satirlar }
    }

    fn bloklar(&mut self, n: Node, iliski: &HashMap<String, String>, kenar: bool) -> Vec<Blok> {
        let mut v = Vec::new();
        for c in n.children().filter(|c| c.is_element()) {
            if c.tag_name().namespace() != Some(W) {
                continue;
            }
            match c.tag_name().name() {
                // Tablodan hemen sonraki tamamen boş <w:p/>: Word'ün "hücre / belge paragrafla biter" kuralı için
                // yazıcının (docx_yaz) eklediği paragraf; belgeye ait değil (Word'ün kendi paragrafı pPr/rPr taşır)
                "p" if c.first_child().is_none() && matches!(v.last(), Some(Blok::Tablo(_)))
                    && c.next_sibling_element().is_none_or(|s| s.tag_name().name() == "sectPr") => {}
                "p" => v.extend(self.paragraf(c, iliski, kenar).into_iter().map(Blok::Paragraf)),
                "tbl" => v.push(Blok::Tablo(self.tablo(c, iliski, kenar))),
                "sdt" => {
                    if let Some(ic) = cocuk(c, "sdtContent") {
                        v.extend(self.bloklar(ic, iliski, kenar));
                    }
                }
                "customXml" => v.extend(self.bloklar(c, iliski, kenar)),
                _ => {}
            }
        }
        v
    }

    /// Üst/alt bilgi dosyası. Alt bilgideki sayfa numarası alanı UDF sayfa numarası özelliklerine çevrilir.
    fn kenar(&mut self, r_id: &str, iliski: &HashMap<String, String>, alt: bool) -> Option<Kenar> {
        let yol = hedef_yolu(iliski.get(r_id)?);
        let xml = xml_ac(self.dosyalar.get(&yol)?)?;
        let d = ayristir(&xml).ok()?;
        let ad = yol.rsplit('/').next().unwrap_or("");
        let rel = iliskiler(self.dosyalar.get(&format!("word/_rels/{ad}.rels")).and_then(|v| xml_ac(v)).as_deref());
        let mut bloklar = self.bloklar(d.root_element(), &rel, true);
        let mut oz = Ozellikler::default();
        // Sayfa numarası: alanı içeren paragraf UDF'te alt bilgi özellikleriyle çizilir (udf_pdf `_sayfa_no_metni`)
        let mut sil = None;
        for (i, b) in bloklar.iter().enumerate() {
            let Blok::Paragraf(p) = b else { continue };
            let Some(k) = p.parcalar.iter().position(|c| matches!(c, Parca::Alan { oz, .. } if oz.al("fieldName") == Some("pageNumber"))) else { continue };
            if !alt {
                self.u.uyar("Üst bilgideki sayfa numarası düz metin olarak açıldı");
                break;
            }
            let on: String = p.parcalar[..k].iter().map(|c| c.metin()).collect::<String>().trim().to_string();
            let sonra: String = p.parcalar[k + 1..].iter().map(|c| c.metin()).collect::<String>();
            let toplam = !sonra.trim().is_empty() && sonra.chars().any(|c| c.is_ascii_digit() || c == '/');
            let hiza_bit = match p.oz.al("Alignment") {
                Some("1") => 32,
                Some("2") => 64,
                _ => 0,
            };
            let spec = 8 + hiza_bit + if toplam { 2048 } else { 0 };
            let yazi = p.parcalar.get(k + 1).or(p.parcalar.get(k)).map(|c| c.oz().clone()).unwrap_or_default();
            oz.ayarla("pageNumber-spec", format!("BSP32_{spec}"));
            oz.ayarla("pageNumber-color", "-16777216");
            oz.ayarla("pageNumber-fontFace", yazi.al("family").unwrap_or("Arial"));
            oz.ayarla("pageNumber-fontSize", yazi.al("size").unwrap_or("11"));
            oz.ayarla("pageNumber-foreStr", on);
            oz.ayarla("pageNumber-pageStartNumStr", "");
            oz.ayarla("pageNumber-seperator", "/");
            oz.ayarla("pageNumber-fontBold", if yazi.evet_mi("bold") { "true" } else { "false" });
            oz.ayarla("pageNumber-fontItalic", if yazi.evet_mi("italic") { "true" } else { "false" });
            sil = Some(i);
            break;
        }
        if let Some(i) = sil {
            bloklar.remove(i);
        }
        Some(Kenar { oz, bloklar })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DocxHatasi {
    CokBuyuk(String),
    Zip(String),
    BelgeYok,
    Xml(String),
}

impl std::fmt::Display for DocxHatasi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocxHatasi::CokBuyuk(m) => write!(f, "dosya çok büyük: {m}"),
            DocxHatasi::Zip(m) => write!(f, "DOCX (ZIP) açılamadı: {m}"),
            DocxHatasi::BelgeYok => write!(f, "DOCX içinde word/document.xml yok"),
            DocxHatasi::Xml(m) => write!(f, "word/document.xml okunamadı: {m}"),
        }
    }
}

impl std::error::Error for DocxHatasi {}

/// DOCX baytlarını belge modeline çevirir.
pub fn oku(bayt: &[u8]) -> Result<DocxOkuma, DocxHatasi> {
    if bayt.len() > AZAMI_GIRDI {
        return Err(DocxHatasi::CokBuyuk(format!("{} MB'tan büyük", AZAMI_GIRDI >> 20)));
    }
    let mut z = ZipArchive::new(Cursor::new(bayt)).map_err(|e| DocxHatasi::Zip(e.to_string()))?;
    if z.len() > AZAMI_DOSYA_SAYISI * 8 {
        return Err(DocxHatasi::CokBuyuk(format!("ZIP içinde {} dosya", z.len())));
    }
    let mut dosyalar = HashMap::new();
    let mut toplam = 0u64;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| DocxHatasi::Zip(e.to_string()))?;
        if f.is_dir() {
            continue;
        }
        let ad = f.name().to_string();
        // Yalnız gerekenler: XML'ler, ilişkiler, resimler
        if !(ad.starts_with("word/") || ad == "[Content_Types].xml") {
            continue;
        }
        let mut v = Vec::new();
        (&mut f).take(AZAMI_ACILMIS + 1).read_to_end(&mut v).map_err(|e| DocxHatasi::Zip(e.to_string()))?;
        if v.len() as u64 > AZAMI_ACILMIS {
            return Err(DocxHatasi::CokBuyuk(format!("{ad} açılınca {} MB'tan büyük", AZAMI_ACILMIS >> 20)));
        }
        toplam += v.len() as u64;
        if toplam > AZAMI_TOPLAM {
            return Err(DocxHatasi::CokBuyuk(format!("açılmış toplam {} MB'tan büyük", AZAMI_TOPLAM >> 20)));
        }
        dosyalar.insert(ad, v);
    }
    let belge_xml = dosyalar.get("word/document.xml").and_then(|v| xml_ac(v)).ok_or(DocxHatasi::BelgeYok)?;
    let d = ayristir(&belge_xml).map_err(DocxHatasi::Xml)?;
    let metin = |ad: &str| dosyalar.get(ad).and_then(|v| xml_ac(v));
    let mut u = Uyarici::default();
    let stiller = stilleri_oku(metin("word/styles.xml").as_deref(), metin("word/theme/theme1.xml").as_deref(), &mut u);
    let numaralar = numaralari_oku(metin("word/numbering.xml").as_deref());
    let iliski = iliskiler(metin("word/_rels/document.xml.rels").as_deref());
    if dosyalar.contains_key("word/footnotes.xml") && belge_xml.contains("footnoteReference") {
        u.uyar("Dipnotlar ve son notlar korunmadı");
    }
    let mut o = Okuyucu { dosyalar: &dosyalar, stiller, numaralar, u };
    let govde_d = cocuk(d.root_element(), "body").ok_or_else(|| DocxHatasi::Xml("w:body yok".into()))?;
    let govde = o.bloklar(govde_d, &iliski, false);

    // Sayfa düzeni: gövdenin son sectPr'i
    let sect = cocuk(govde_d, "sectPr");
    let mut sayfa = Ozellikler::default();
    sayfa.ayarla("mediaSizeName", "1");
    let boyut = sect.and_then(|s| cocuk(s, "pgSz"));
    let (sw, sh) = (twip(boyut.and_then(|b| wa(b, "w"))).unwrap_or(A4_W), twip(boyut.and_then(|b| wa(b, "h"))).unwrap_or(A4_H));
    let yatay = sw > sh || boyut.and_then(|b| wa(b, "orient")) == Some("landscape");
    let (kisa, uzun) = if sw < sh { (sw, sh) } else { (sh, sw) };
    if (kisa - A4_W).abs() > 200.0 || (uzun - A4_H).abs() > 200.0 {
        o.u.uyar("Kâğıt boyutu A4 değil; A4 olarak açıldı (UDF yalnız A4 destekler)");
    }
    let marj = sect.and_then(|s| cocuk(s, "pgMar"));
    let m = |ad: &str, v: f64| twip(marj.and_then(|x| wa(x, ad))).unwrap_or(v) / 20.0;
    sayfa.ayarla("leftMargin", format!("{}", m("left", 1418.0)));
    sayfa.ayarla("rightMargin", format!("{}", m("right", 1418.0)));
    sayfa.ayarla("topMargin", format!("{}", m("top", 1418.0)));
    sayfa.ayarla("bottomMargin", format!("{}", m("bottom", 1418.0)));
    sayfa.ayarla("paperOrientation", if yatay { "0" } else { "1" });
    sayfa.ayarla("headerFOffset", format!("{}", m("header", 709.0)));
    sayfa.ayarla("footerFOffset", format!("{}", m("footer", 709.0)));
    if sect.and_then(|s| cocuk(s, "cols")).and_then(|c| wa(c, "num")).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n > 1) {
        o.u.uyar("Çok sütunlu sayfa düzeni tek sütun olarak açıldı");
    }

    let referans = |tur: &str| {
        sect.and_then(|s| {
            s.children()
                .filter(|c| c.tag_name().name() == tur)
                .find(|c| wa(*c, "type").unwrap_or("default") == "default")
                .and_then(|c| c.attribute((R, "id")))
                .map(str::to_string)
        })
    };
    if sect.is_some_and(|s| cocuk(s, "titlePg").is_some()) {
        o.u.uyar("İlk sayfaya özel üst/alt bilgi korunmadı; bütün sayfalarda aynısı kullanıldı");
    }
    let ust_bilgi = referans("headerReference").and_then(|id| o.kenar(&id, &iliski, false));
    let alt_bilgi = referans("footerReference").and_then(|id| o.kenar(&id, &iliski, true));

    let ozellik = |v: &[(&str, &str)]| Ozellikler(v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect());
    let belge = Belge {
        kok: ozellik(&[("format_id", "1.8")]),
        sayfa,
        govde_oz: ozellik(&[("resolver", "hvl-default")]),
        ust_bilgi,
        alt_bilgi,
        govde,
        stiller: vec![
            Stil {
                ad: "default".into(),
                oz: ozellik(&[("name", "default"), ("description", "Geçerli"), ("family", "Dialog"), ("size", "12"),
                    ("bold", "false"), ("italic", "false"), ("foreground", "-13421773"),
                    ("FONT_ATTRIBUTE_KEY", "javax.swing.plaf.FontUIResource[family=Dialog,name=Dialog,style=plain,size=12]")]),
            },
            Stil { ad: "hvl-default".into(), oz: ozellik(&[("name", "hvl-default"), ("family", "Times New Roman"), ("size", "12"), ("description", "Gövde")]) },
        ],
        ek_dosyalar: Vec::new(),
        ek_ogeler: Vec::new(),
    };
    Ok(DocxOkuma { belge, uyarilar: o.u.0 })
}
