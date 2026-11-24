//! DOCX (Word) yazma: belge modelini (UDF yapısı) Word belgesine çevirir.
//!
//! Her paragraf ve parça, UDF stil zincirinden çözülmüş **etkin** biçimiyle (stil.rs, editör ve PDF ile aynı
//! kurallar) açık Word biçimi olarak yazılır; böylece Word'de görünen editördekiyle aynıdır. Tablolarda, birleşik
//! hücreli satırların sınırlarından ortak bir sütun ızgarası çıkarılır (gridSpan). Alt bilgideki UDF sayfa numarası
//! Word alanı olur (PAGE / NUMPAGES). Çıktı belirlenimcidir (aynı belge → aynı bayt). Claude, 2026-11-23.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Cursor, Write};

use base64::Engine as _;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::gorunum::SayfaNumarasi;
use crate::model::*;
use crate::stil::*;

const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;

fn kacis(m: &str) -> String {
    let mut s = String::with_capacity(m.len());
    for c in m.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\t' | '\n' | '\r' => s.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => s.push(c),
        }
    }
    s
}

fn twip(pt: f64) -> i64 {
    (pt * 20.0).round() as i64
}

/// Liste: UDF ListId + tür → Word numId
#[derive(Default)]
struct Listeler {
    /// (anahtar) → (numId, madde mi, lvlText, numFmt)
    harita: BTreeMap<String, (u32, bool, String, String)>,
}

impl Listeler {
    fn num_id(&mut self, p: &Paragraf, s: &ParagrafStili) -> Option<u32> {
        let liste = s.liste.as_ref()?;
        let (anahtar, madde, metin, bicim) = match liste {
            Liste::Madde { ok } => (format!("madde-{ok}"), true, if *ok { "»" } else { "•" }.to_string(), "bullet".to_string()),
            Liste::Numara { liste_id, tur } => {
                let udf_tur = p.oz.al("NumberType").unwrap_or("");
                let bicim = if udf_tur.contains("CHAR_SMALL") {
                    "lowerLetter"
                } else if udf_tur.contains("CHAR_BIG") {
                    "upperLetter"
                } else if udf_tur.contains("ROMAN_SMALL") {
                    "lowerRoman"
                } else if udf_tur.contains("ROMAN_BIG") {
                    "upperRoman"
                } else {
                    "decimal"
                };
                let son = match tur {
                    NumaraTuru::Nokta => ".",
                    NumaraTuru::Tire => "-",
                    NumaraTuru::Parantez => ")",
                };
                (format!("numara-{liste_id}-{bicim}{son}"), false, format!("%1{son}"), bicim.to_string())
            }
        };
        let yeni = self.harita.len() as u32 + 1;
        Some(self.harita.entry(anahtar).or_insert((yeni, madde, metin, bicim)).0)
    }

    fn xml(&self) -> String {
        let mut s = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering {NS}>"#);
        for (num, madde, metin, bicim) in self.harita.values() {
            // Girinti paragrafın kendisinde (açık); seviye yalnız biçim ve öneki taşır
            let yazi = if *madde { r#"<w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/></w:rPr>"# } else { "" };
            let _ = write!(
                s,
                r#"<w:abstractNum w:abstractNumId="{num}"><w:multiLevelType w:val="singleLevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="{bicim}"/><w:lvlText w:val="{}"/><w:lvlJc w:val="left"/><w:suff w:val="space"/>{yazi}</w:lvl></w:abstractNum>"#,
                kacis(metin)
            );
        }
        for (num, ..) in self.harita.values() {
            let _ = write!(s, r#"<w:num w:numId="{num}"><w:abstractNumId w:val="{num}"/></w:num>"#);
        }
        s.push_str("</w:numbering>");
        s
    }
}

/// Bir XML parçası (document / header / footer) yazılırken: resimler ve ilişkileri.
struct Bolum<'a> {
    cozucu: &'a StilCozucu<'a>,
    listeler: &'a mut Listeler,
    resimler: &'a mut Vec<(String, Vec<u8>)>,
    iliski: Vec<(String, String)>,
    resim_no: &'a mut u32,
    yazi_genisligi: f64,
}

impl Bolum<'_> {
    fn rpr(&self, s: &ParcaStili) -> String {
        let aile = kacis(s.cizim_ailesi());
        let mut r = format!(r#"<w:rPr><w:rFonts w:ascii="{aile}" w:hAnsi="{aile}" w:cs="{aile}"/>"#);
        if s.kalin {
            r.push_str("<w:b/>");
        }
        if s.egik {
            r.push_str("<w:i/>");
        }
        if s.alti_cizili {
            r.push_str(r#"<w:u w:val="single"/>"#);
        }
        if s.renk != Renk::SIYAH {
            let _ = write!(r, r#"<w:color w:val="{:02X}{:02X}{:02X}"/>"#, s.renk.r, s.renk.g, s.renk.b);
        }
        let yarim = (s.boyut * 2.0).round().max(2.0) as i64;
        let _ = write!(r, r#"<w:sz w:val="{yarim}"/><w:szCs w:val="{yarim}"/></w:rPr>"#);
        r
    }

    /// Metin parçası: sekme karakteri `<w:tab/>`, paragraf içi satır sonu `<w:br/>` (Shift+Enter). Paragraf sonu
    /// "\n"'i çağıran tarafta çıkarılmış olmalı.
    fn metin_run(&self, m: &str, s: &ParcaStili, x: &mut String) {
        let rpr = self.rpr(s);
        for (j, satir) in m.replace("\r\n", "\n").replace('\r', "\n").split('\n').enumerate() {
            if j > 0 {
                let _ = write!(x, "<w:r>{rpr}<w:br/></w:r>");
            }
            for (i, parca) in satir.split('\t').enumerate() {
                if i > 0 {
                    let _ = write!(x, "<w:r>{rpr}<w:tab/></w:r>");
                }
                if !parca.is_empty() {
                    let _ = write!(x, r#"<w:r>{rpr}<w:t xml:space="preserve">{}</w:t></w:r>"#, kacis(parca));
                }
            }
        }
    }

    fn alan(&self, kod: &str, gosterilen: &str, s: &ParcaStili, x: &mut String) {
        let rpr = self.rpr(s);
        let _ = write!(
            x,
            r#"<w:r>{rpr}<w:fldChar w:fldCharType="begin"/></w:r><w:r>{rpr}<w:instrText xml:space="preserve"> {kod} </w:instrText></w:r><w:r>{rpr}<w:fldChar w:fldCharType="separate"/></w:r><w:r>{rpr}<w:t>{}</w:t></w:r><w:r>{rpr}<w:fldChar w:fldCharType="end"/></w:r>"#,
            kacis(gosterilen)
        );
    }

    fn resim(&mut self, oz: &Ozellikler, x: &mut String) {
        let veri: String = oz.al("imageData").unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')).collect();
        let Ok(bayt) = base64::engine::general_purpose::STANDARD.decode(veri) else { return };
        let uzanti = if bayt.starts_with(b"\xFF\xD8") {
            "jpeg"
        } else if bayt.starts_with(b"GIF8") {
            "gif"
        } else {
            "png"
        };
        *self.resim_no += 1;
        let no = *self.resim_no;
        let ad = format!("media/resim{no}.{uzanti}");
        let rid = format!("rIdR{no}");
        self.resimler.push((format!("word/{ad}"), bayt));
        self.iliski.push((rid.clone(), format!(r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="{ad}""#)));
        let gen = oz.sayi("width").filter(|g| *g > 0.0).unwrap_or(72.0).min(self.yazi_genisligi.max(10.0));
        let yuk = oz.sayi("height").filter(|g| *g > 0.0).unwrap_or(72.0);
        let (cx, cy) = ((gen * 12700.0).round() as i64, (yuk * 12700.0).round() as i64);
        let _ = write!(
            x,
            r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{no}" name="Resim {no}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{no}" name="resim{no}.{uzanti}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
        );
    }

    fn paragraf(&mut self, p: &Paragraf, renk: Renk, x: &mut String) {
        let s = self.cozucu.paragraf(p);
        x.push_str("<w:p><w:pPr>");
        if let Some(num) = self.listeler.num_id(p, &s) {
            let _ = write!(x, r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="{num}"/></w:numPr>"#);
        }
        if !s.sekmeler.is_empty() {
            x.push_str("<w:tabs>");
            for t in &s.sekmeler {
                let tur = match t.hiza {
                    2 => "center",
                    1 => "right",
                    4 => "decimal",
                    _ => "left",
                };
                let _ = write!(x, r#"<w:tab w:val="{tur}" w:pos="{}"/>"#, twip(t.konum.max(0.0)));
            }
            x.push_str("</w:tabs>");
        }
        let _ = write!(
            x,
            r#"<w:spacing w:before="{}" w:after="{}" w:line="{}" w:lineRule="auto"/>"#,
            twip(s.ust_bosluk),
            twip(s.alt_bosluk),
            ((1.0 + s.satir_araligi) * 240.0).round() as i64
        );
        let ilk = if s.ilk_satir < 0.0 {
            format!(r#" w:hanging="{}""#, twip(-s.ilk_satir))
        } else {
            format!(r#" w:firstLine="{}""#, twip(s.ilk_satir))
        };
        let _ = write!(x, r#"<w:ind w:left="{}" w:right="{}"{ilk}/>"#, twip(s.sol), twip(s.sag));
        let jc = match s.hiza {
            Hiza::Orta => "center",
            Hiza::Sag => "right",
            Hiza::IkiYana => "both",
            Hiza::Sol => "left",
        };
        let _ = write!(x, r#"<w:jc w:val="{jc}"/>"#);
        // Paragraf işaretinin biçimi: son parçanın (boş paragrafın yüksekliği buna göre)
        if let Some(son) = p.parcalar.last() {
            let st = self.cozucu.parca(son, p, renk);
            x.push_str(&self.rpr(&st));
        }
        x.push_str("</w:pPr>");
        // Paragraf sonu: son metin parçasının sonundaki "\n" (UYAP) yazılmaz; öteki "\n"ler satır sonudur
        let son_metin = p.parcalar.iter().rposition(|c| matches!(c, Parca::Metin { .. } | Parca::Bosluk { .. }));
        for (i, c) in p.parcalar.iter().enumerate() {
            let st = self.cozucu.parca(c, p, renk);
            match c {
                Parca::Metin { metin, .. } | Parca::Bosluk { metin, .. } => {
                    let m = if Some(i) == son_metin { metin.strip_suffix('\n').unwrap_or(metin) } else { metin.as_str() };
                    self.metin_run(m, &st, x)
                }
                Parca::Sekme { .. } => {
                    let _ = write!(x, "<w:r>{}<w:tab/></w:r>", self.rpr(&st));
                }
                Parca::Alan { metin, oz } => {
                    if oz.al("fieldName") == Some("pageNumber") {
                        self.alan("PAGE", metin.trim(), &st, x);
                    } else {
                        self.metin_run(metin, &st, x);
                    }
                }
                Parca::Resim { oz, .. } => self.resim(oz, x),
            }
        }
        x.push_str("</w:p>");
    }

    fn tablo(&mut self, t: &Tablo, renk: Renk, genislik: f64, x: &mut String) {
        let ts = tablo_stili(t);
        let tg = (genislik - ts.sol - ts.sag).max(20.0);
        // Ortak ızgara: her satırın hücre sınırları (0..1) birleştirilir
        let mut sinirlar: Vec<f64> = vec![0.0, 1.0];
        let bilgiler: Vec<SatirBilgisi> = t.satirlar.iter().map(|r| satir_bilgisi(t, r)).collect();
        for b in &bilgiler {
            let mut a = 0.0;
            for o in &b.oranlar {
                a += o;
                sinirlar.push(a);
            }
        }
        sinirlar.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sinirlar.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        let izgara: Vec<i64> = sinirlar.windows(2).map(|w| twip((w[1] - w[0]) * tg).max(1)).collect();
        let kenar = |tur: &str| format!(r#"<w:{tur} w:val="single" w:sz="4" w:space="0" w:color="000000"/>"#);
        let kenarlar = match ts.kenarlik {
            Kenarlik::Hucre => ["top", "left", "bottom", "right", "insideH", "insideV"].iter().map(|t| kenar(t)).collect::<String>(),
            Kenarlik::Tablo => ["top", "left", "bottom", "right"].iter().map(|t| kenar(t)).collect::<String>(),
            Kenarlik::Yok => ["top", "left", "bottom", "right", "insideH", "insideV"].iter().map(|t| format!(r#"<w:{t} w:val="nil"/>"#)).collect(),
        };
        let _ = write!(
            x,
            r#"<w:tbl><w:tblPr><w:tblW w:w="{}" w:type="dxa"/><w:tblInd w:w="{}" w:type="dxa"/><w:tblBorders>{kenarlar}</w:tblBorders><w:tblLayout w:type="fixed"/><w:tblCellMar><w:left w:w="{}" w:type="dxa"/><w:right w:w="{}" w:type="dxa"/></w:tblCellMar></w:tblPr><w:tblGrid>"#,
            twip(tg),
            twip(ts.sol),
            twip(HUCRE_BOSLUK_X),
            twip(HUCRE_BOSLUK_X)
        );
        for g in &izgara {
            let _ = write!(x, r#"<w:gridCol w:w="{g}"/>"#);
        }
        x.push_str("</w:tblGrid>");
        for (r, b) in t.satirlar.iter().zip(&bilgiler) {
            x.push_str("<w:tr>");
            if b.min_yukseklik > 0.0 {
                let _ = write!(x, r#"<w:trPr><w:trHeight w:val="{}" w:hRule="atLeast"/></w:trPr>"#, twip(b.min_yukseklik));
            }
            let mut bas = 0.0;
            for (h, o) in r.hucreler.iter().zip(&b.oranlar) {
                let son = bas + o;
                let i0 = sinirlar.iter().position(|s| (s - bas).abs() < 1e-4).unwrap_or(0);
                let i1 = sinirlar.iter().position(|s| (s - son).abs() < 1e-4).unwrap_or(i0 + 1);
                let kapla = i1.saturating_sub(i0).max(1);
                let _ = write!(x, r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/>"#, twip(o * tg));
                if kapla > 1 {
                    let _ = write!(x, r#"<w:gridSpan w:val="{kapla}"/>"#);
                }
                x.push_str("</w:tcPr>");
                let ic_gen = (o * tg - 2.0 * HUCRE_BOSLUK_X).max(5.0);
                self.bloklar(&h.bloklar, renk, ic_gen, x);
                // Word: hücre bir paragrafla bitmeli
                if !matches!(h.bloklar.last(), Some(Blok::Paragraf(_))) {
                    x.push_str("<w:p/>");
                }
                x.push_str("</w:tc>");
                bas = son;
            }
            x.push_str("</w:tr>");
        }
        x.push_str("</w:tbl>");
    }

    fn bloklar(&mut self, b: &[Blok], renk: Renk, genislik: f64, x: &mut String) {
        for blok in b {
            match blok {
                Blok::Paragraf(p) => self.paragraf(p, renk, x),
                Blok::Tablo(t) => self.tablo(t, renk, genislik, x),
            }
        }
    }

    fn iliski_xml(&self, ekler: &[(String, String)]) -> String {
        let mut s = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
        for (id, ozellik) in ekler.iter().chain(&self.iliski) {
            let _ = write!(s, r#"<Relationship Id="{id}" {ozellik}/>"#);
        }
        s.push_str("</Relationships>");
        s
    }
}

/// UDF sayfa numarasının Word alt bilgisindeki karşılığı: "<önek> PAGE[ / NUMPAGES]" alanları.
fn sayfa_numarasi_paragrafi(sn: &SayfaNumarasi, b: &Bolum) -> String {
    let jc = match sn.hiza {
        Hiza::Orta => "center",
        Hiza::Sag => "right",
        _ => "left",
    };
    let mut x = format!(r#"<w:p><w:pPr><w:jc w:val="{jc}"/></w:pPr>"#);
    if !sn.on_ek.is_empty() {
        b.metin_run(&format!("{} ", sn.on_ek), &sn.stil, &mut x);
    }
    b.alan("PAGE", "1", &sn.stil, &mut x);
    if sn.toplam_goster {
        b.metin_run(&sn.ayirici, &sn.stil, &mut x);
        b.alan("NUMPAGES", "1", &sn.stil, &mut x);
    }
    x.push_str("</w:p>");
    x
}

/// Belgeyi DOCX baytlarına yazar.
pub fn yaz(belge: &Belge) -> Result<Vec<u8>, String> {
    let cozucu = StilCozucu::yeni(belge);
    let sayfa = SayfaBicimi::coz(&belge.sayfa);
    let yazi_gen = sayfa.yazi_genisligi();
    let mut listeler = Listeler::default();
    let mut resimler: Vec<(String, Vec<u8>)> = Vec::new();
    let mut resim_no = 0u32;

    // Üst / alt bilgi
    let kenar_xml = |k: Option<&Kenar>, alt: bool, listeler: &mut Listeler, resimler: &mut Vec<(String, Vec<u8>)>, resim_no: &mut u32| -> Option<(String, String)> {
        let sn = if alt { belge.alt_bilgi.as_ref().and_then(|k| SayfaNumarasi::coz(&k.oz)) } else { None };
        let k = k?;
        let renk = k.oz.al("foreground").and_then(Renk::argb).unwrap_or(Renk::SIYAH);
        let mut b = Bolum { cozucu: &cozucu, listeler, resimler, iliski: Vec::new(), resim_no, yazi_genisligi: yazi_gen };
        let etiket = if alt { "ftr" } else { "hdr" };
        let mut x = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{etiket} {NS}>"#);
        b.bloklar(&k.bloklar, renk, yazi_gen, &mut x);
        if let Some(sn) = &sn {
            x.push_str(&sayfa_numarasi_paragrafi(sn, &b));
        }
        if !x.ends_with("</w:p>") {
            x.push_str("<w:p/>");
        }
        let _ = write!(x, "</w:{etiket}>");
        Some((x, b.iliski_xml(&[])))
    };
    let ust = kenar_xml(belge.ust_bilgi.as_ref(), false, &mut listeler, &mut resimler, &mut resim_no);
    let alt = kenar_xml(belge.alt_bilgi.as_ref(), true, &mut listeler, &mut resimler, &mut resim_no);

    // Gövde
    let mut b = Bolum { cozucu: &cozucu, listeler: &mut listeler, resimler: &mut resimler, iliski: Vec::new(), resim_no: &mut resim_no, yazi_genisligi: yazi_gen };
    let mut govde = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {NS}><w:body>"#);
    b.bloklar(&belge.govde, Renk::SIYAH, yazi_gen, &mut govde);
    if !matches!(belge.govde.last(), Some(Blok::Paragraf(_))) {
        govde.push_str("<w:p/>");
    }
    govde.push_str("<w:sectPr>");
    if ust.is_some() {
        govde.push_str(r#"<w:headerReference w:type="default" r:id="rIdUst"/>"#);
    }
    if alt.is_some() {
        govde.push_str(r#"<w:footerReference w:type="default" r:id="rIdAlt"/>"#);
    }
    let (w, h) = (twip(sayfa.genislik), twip(sayfa.yukseklik));
    let yon = if sayfa.yatay { r#" w:orient="landscape""# } else { "" };
    let _ = write!(
        govde,
        r#"<w:pgSz w:w="{w}" w:h="{h}"{yon}/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
        twip(sayfa.ust),
        twip(sayfa.sag),
        twip(sayfa.alt),
        twip(sayfa.sol),
        twip(sayfa.ust_bilgi_payi),
        twip(sayfa.alt_bilgi_payi)
    );
    let mut ana_ekler = vec![
        ("rIdStil".to_string(), r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml""#.to_string()),
        ("rIdAyar".to_string(), r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml""#.to_string()),
        ("rIdNum".to_string(), r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml""#.to_string()),
    ];
    if ust.is_some() {
        ana_ekler.push(("rIdUst".into(), r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml""#.into()));
    }
    if alt.is_some() {
        ana_ekler.push(("rIdAlt".into(), r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml""#.into()));
    }
    let belge_iliski = b.iliski_xml(&ana_ekler);
    drop(b);

    // Varsayılanlar UDF'in hvl-default'u (biçimsiz metin onu alır)
    let vs = belge.stiller.iter().find(|s| s.ad == "hvl-default").map(|s| &s.oz);
    let v_aile = kacis(vs.and_then(|o| o.al("family")).unwrap_or(VARSAYILAN_AILE));
    let v_boyut = (vs.and_then(|o| o.sayi("size")).filter(|x| *x > 0.0).unwrap_or(VARSAYILAN_BOYUT) * 2.0).round() as i64;
    let stiller = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {NS}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="{v_aile}" w:hAnsi="{v_aile}" w:cs="{v_aile}" w:eastAsia="{v_aile}"/><w:sz w:val="{v_boyut}"/><w:szCs w:val="{v_boyut}"/><w:lang w:val="tr-TR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style></w:styles>"#
    );
    let ayarlar = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings {NS}><w:defaultTabStop w:val="1440"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#);

    let mut turler = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>"#);
    if ust.is_some() {
        turler.push_str(r#"<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>"#);
    }
    if alt.is_some() {
        turler.push_str(r#"<Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>"#);
    }
    turler.push_str("</Types>");
    let kok_iliski = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    let sec = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut ekle = |ad: &str, v: &[u8]| -> Result<(), String> {
        z.start_file(ad, sec).map_err(|e| e.to_string())?;
        z.write_all(v).map_err(|e| e.to_string())
    };
    ekle("[Content_Types].xml", turler.as_bytes())?;
    ekle("_rels/.rels", kok_iliski.as_bytes())?;
    ekle("word/document.xml", govde.as_bytes())?;
    ekle("word/_rels/document.xml.rels", belge_iliski.as_bytes())?;
    ekle("word/styles.xml", stiller.as_bytes())?;
    ekle("word/settings.xml", ayarlar.as_bytes())?;
    ekle("word/numbering.xml", listeler.xml().as_bytes())?;
    if let Some((x, r)) = &ust {
        ekle("word/header1.xml", x.as_bytes())?;
        ekle("word/_rels/header1.xml.rels", r.as_bytes())?;
    }
    if let Some((x, r)) = &alt {
        ekle("word/footer1.xml", x.as_bytes())?;
        ekle("word/_rels/footer1.xml.rels", r.as_bytes())?;
    }
    for (ad, v) in &resimler {
        ekle(ad, v)?;
    }
    Ok(z.finish().map_err(|e| e.to_string())?.into_inner())
}
