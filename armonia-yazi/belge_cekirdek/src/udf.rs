//! UDF (UYAP belge biçimi) okuma ve yazma.
//!
//! UDF bir ZIP'tir: `content.xml` (asıl belge), varsa `sign.sgn` (e-imza) ve başka dosyalar. `content.xml`:
//! ```text
//! <template format_id="1.8">
//!   <content><![CDATA[belgenin bütün metni]]></content>
//!   <properties><pageFormat leftMargin=".." paperOrientation="1" .. /></properties>
//!   <elements resolver="hvl-default">
//!     <header ..>..</header> <footer ..>..</footer>
//!     <paragraph Alignment="3" LeftIndent=".."><content startOffset="0" length="12" bold="true"/>..</paragraph>
//!     <table columnSpans=".."><row><cell><paragraph>..</paragraph></cell></row></table>
//!   </elements>
//!   <styles><style name="hvl-default" family="Times New Roman" size="12"/>..</styles>
//! </template>
//! ```
//! Parçalar metne `startOffset` + `length` ile işaret eder. Konumlar **UTF-16 birimi** olarak sayılır (UYAP
//! editörü Java; Java dizinleri UTF-16). Türkçe harflerde fark yok; BMP dışı karakterde (ör. emoji) önemli.
//!
//! Güvenilmeyen dosya: boyut ve dosya sayısı sınırlı (ZIP bombası), DTD/varlık kapalı (roxmltree varsayılanı).
//! Claude, 2026-11-23.

use std::io::{Cursor, Read, Write};

use roxmltree::{Document, Node, ParsingOptions};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::model::*;

/// Sınırlar (güvenilmeyen dosya).
pub const AZAMI_GIRDI: usize = 64 << 20;
pub const AZAMI_ACILMIS: u64 = 32 << 20;
pub const AZAMI_TOPLAM: u64 = 128 << 20;
pub const AZAMI_DOSYA_SAYISI: usize = 64;
pub const AZAMI_XML_DUGUM: u32 = 5_000_000;

const IMZA_DOSYASI: &str = "sign.sgn";
const ICERIK_DOSYASI: &str = "content.xml";

#[derive(Debug, PartialEq, Eq)]
pub enum UdfHatasi {
    CokBuyuk(String),
    Zip(String),
    IcerikYok,
    Xml(String),
    Bicim(String),
}

impl std::fmt::Display for UdfHatasi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UdfHatasi::CokBuyuk(m) => write!(f, "dosya çok büyük: {m}"),
            UdfHatasi::Zip(m) => write!(f, "UDF (ZIP) açılamadı: {m}"),
            UdfHatasi::IcerikYok => write!(f, "UDF içinde content.xml yok"),
            UdfHatasi::Xml(m) => write!(f, "content.xml okunamadı: {m}"),
            UdfHatasi::Bicim(m) => write!(f, "UDF biçimi tanınmadı: {m}"),
        }
    }
}

impl std::error::Error for UdfHatasi {}

/// Okuma sonucu: belge + kullanıcıya gösterilecek uyarılar.
#[derive(Debug)]
pub struct UdfOkuma {
    pub belge: Belge,
    /// Dosya e-imzalıydı. Belge değiştirilip kaydedilince imza geçersiz olur; yazarken imza eklenmez.
    pub imzali: bool,
    /// Tanınmayan / korunamayan öğeler ve bozuk konumlar. Boşsa belge kayıpsız okundu.
    pub uyarilar: Vec<String>,
}

/// Bayt dizisi UDF mi (ZIP ve içinde content.xml)?
pub fn udf_mi(bayt: &[u8]) -> bool {
    bayt.starts_with(b"PK")
        && ZipArchive::new(Cursor::new(bayt)).is_ok_and(|mut z| z.by_name(ICERIK_DOSYASI).is_ok())
}

fn sinirli_oku(r: &mut impl Read, ad: &str, toplam: &mut u64) -> Result<Vec<u8>, UdfHatasi> {
    let mut v = Vec::new();
    r.take(AZAMI_ACILMIS + 1).read_to_end(&mut v).map_err(|e| UdfHatasi::Zip(format!("{ad}: {e}")))?;
    if v.len() as u64 > AZAMI_ACILMIS {
        return Err(UdfHatasi::CokBuyuk(format!("{ad} açılınca {} MB'tan büyük", AZAMI_ACILMIS >> 20)));
    }
    *toplam += v.len() as u64;
    if *toplam > AZAMI_TOPLAM {
        return Err(UdfHatasi::CokBuyuk(format!("açılmış toplam {} MB'tan büyük", AZAMI_TOPLAM >> 20)));
    }
    Ok(v)
}

/// UDF baytlarını okur.
pub fn oku(bayt: &[u8]) -> Result<UdfOkuma, UdfHatasi> {
    if bayt.len() > AZAMI_GIRDI {
        return Err(UdfHatasi::CokBuyuk(format!("{} MB'tan büyük", AZAMI_GIRDI >> 20)));
    }
    let mut z = ZipArchive::new(Cursor::new(bayt)).map_err(|e| UdfHatasi::Zip(e.to_string()))?;
    if z.len() > AZAMI_DOSYA_SAYISI {
        return Err(UdfHatasi::CokBuyuk(format!("ZIP içinde {} dosya", z.len())));
    }
    let mut toplam = 0u64;
    let mut icerik: Option<Vec<u8>> = None;
    let mut ek_dosyalar = Vec::new();
    let mut imzali = false;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| UdfHatasi::Zip(e.to_string()))?;
        if f.is_dir() {
            continue;
        }
        let ad = f.name().to_string();
        let veri = sinirli_oku(&mut f, &ad, &mut toplam)?;
        if ad == ICERIK_DOSYASI {
            icerik = Some(veri);
        } else if ad == IMZA_DOSYASI {
            imzali = true;
        } else {
            ek_dosyalar.push((ad, veri));
        }
    }
    let icerik = icerik.ok_or(UdfHatasi::IcerikYok)?;
    let xml = String::from_utf8(icerik).map_err(|_| UdfHatasi::Xml("UTF-8 değil".into()))?;
    let mut okuma = icerik_oku(&xml)?;
    okuma.belge.ek_dosyalar = ek_dosyalar;
    okuma.imzali = imzali;
    Ok(okuma)
}

/// Yalnız `content.xml` metnini okur.
pub fn icerik_oku(xml: &str) -> Result<UdfOkuma, UdfHatasi> {
    let secenek = ParsingOptions { allow_dtd: false, nodes_limit: AZAMI_XML_DUGUM };
    let doc = Document::parse_with_options(xml, secenek).map_err(|e| UdfHatasi::Xml(e.to_string()))?;
    let kok = doc.root_element();
    if kok.tag_name().name() != "template" {
        return Err(UdfHatasi::Bicim(format!("kök öğe <{}> (beklenen <template>)", kok.tag_name().name())));
    }
    let metin: Vec<u16> = cocuk(kok, "content").and_then(|n| n.text()).unwrap_or("").encode_utf16().collect();
    let mut o = Okuyucu { metin, uyarilar: Vec::new() };
    let mut belge = Belge { kok: ozellikler(kok), ..Default::default() };

    for c in kok.children().filter(Node::is_element) {
        match c.tag_name().name() {
            "content" => {}
            "properties" => {
                for p in c.children().filter(Node::is_element) {
                    if p.tag_name().name() == "pageFormat" {
                        belge.sayfa = ozellikler(p);
                    } else {
                        o.uyar(format!("sayfa özelliklerinde tanınmayan öğe <{}> korunmadı", p.tag_name().name()));
                    }
                }
            }
            "elements" => {
                belge.govde_oz = ozellikler(c);
                for e in c.children().filter(Node::is_element) {
                    match e.tag_name().name() {
                        "header" => belge.ust_bilgi = Some(Kenar { oz: ozellikler(e), bloklar: o.bloklar(e) }),
                        "footer" => belge.alt_bilgi = Some(Kenar { oz: ozellikler(e), bloklar: o.bloklar(e) }),
                        _ => {
                            if let Some(b) = o.blok(e) {
                                belge.govde.push(b);
                            }
                        }
                    }
                }
            }
            "styles" => {
                for s in c.children().filter(Node::is_element) {
                    if s.tag_name().name() == "style" {
                        belge.stiller.push(Stil { ad: s.attribute("name").unwrap_or("").to_string(), oz: ozellikler(s) });
                    }
                }
            }
            ad => o.uyar(format!("tanınmayan öğe <{ad}> korunmadı")),
        }
    }
    Ok(UdfOkuma { belge, imzali: false, uyarilar: o.uyarilar })
}

fn cocuk<'a, 'i>(n: Node<'a, 'i>, ad: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == ad)
}

/// Konum dışındaki özellikler, dosyadaki sırayla.
fn ozellikler(n: Node) -> Ozellikler {
    Ozellikler(
        n.attributes()
            .filter(|a| a.name() != "startOffset" && a.name() != "length")
            .map(|a| (a.name().to_string(), a.value().to_string()))
            .collect(),
    )
}

struct Okuyucu {
    metin: Vec<u16>,
    uyarilar: Vec<String>,
}

impl Okuyucu {
    fn uyar(&mut self, m: String) {
        if !self.uyarilar.contains(&m) {
            self.uyarilar.push(m);
        }
    }

    fn dilim(&mut self, n: Node) -> String {
        let bas = n.attribute("startOffset").and_then(|d| d.trim().parse::<usize>().ok());
        let uz = n.attribute("length").and_then(|d| d.trim().parse::<usize>().ok());
        let (Some(bas), Some(uz)) = (bas, uz) else {
            if n.attribute("startOffset").is_some() || n.attribute("length").is_some() {
                self.uyar("bozuk metin konumu (startOffset/length) atlandı".into());
            }
            return String::new();
        };
        let son = bas.saturating_add(uz);
        if son > self.metin.len() {
            self.uyar("metnin dışını gösteren konum kırpıldı".into());
        }
        let son = son.min(self.metin.len());
        if bas >= son {
            return String::new();
        }
        String::from_utf16_lossy(&self.metin[bas..son])
    }

    fn bloklar(&mut self, n: Node) -> Vec<Blok> {
        n.children().filter(Node::is_element).filter_map(|e| self.blok(e)).collect()
    }

    fn blok(&mut self, e: Node) -> Option<Blok> {
        match e.tag_name().name() {
            "paragraph" => Some(Blok::Paragraf(self.paragraf(e))),
            "table" => Some(Blok::Tablo(self.tablo(e))),
            ad => {
                self.uyar(format!("tanınmayan belge öğesi <{ad}> korunmadı"));
                None
            }
        }
    }

    fn paragraf(&mut self, e: Node) -> Paragraf {
        let mut p = Paragraf { oz: ozellikler(e), parcalar: Vec::new() };
        for c in e.children().filter(Node::is_element) {
            let metin = self.dilim(c);
            let oz = ozellikler(c);
            let parca = match c.tag_name().name() {
                "content" => Parca::Metin { metin, oz },
                "space" => Parca::Bosluk { metin, oz },
                "tab" => Parca::Sekme { metin, oz },
                "field" => Parca::Alan { metin, oz },
                "image" => Parca::Resim { metin, oz },
                ad => {
                    self.uyar(format!("paragrafta tanınmayan öğe <{ad}> korunmadı"));
                    continue;
                }
            };
            p.parcalar.push(parca);
        }
        p
    }

    fn tablo(&mut self, e: Node) -> Tablo {
        let mut t = Tablo { oz: ozellikler(e), satirlar: Vec::new() };
        for r in e.children().filter(Node::is_element) {
            if r.tag_name().name() != "row" {
                self.uyar(format!("tabloda tanınmayan öğe <{}> korunmadı", r.tag_name().name()));
                continue;
            }
            let mut satir = Satir { oz: ozellikler(r), hucreler: Vec::new() };
            for h in r.children().filter(Node::is_element) {
                if h.tag_name().name() != "cell" {
                    self.uyar(format!("tablo satırında tanınmayan öğe <{}> korunmadı", h.tag_name().name()));
                    continue;
                }
                satir.hucreler.push(Hucre { oz: ozellikler(h), bloklar: self.bloklar(h) });
            }
            t.satirlar.push(satir);
        }
        t
    }
}

// ── Yazma ────────────────────────────────────────────────────────────────────────────────────────────

/// Belgeyi UDF baytlarına yazar. İmza eklenmez (değişen belgenin imzası geçersizdir; imza ayrı adım).
pub fn yaz(belge: &Belge) -> Result<Vec<u8>, String> {
    let xml = icerik_yaz(belge);
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    let sec = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    z.start_file(ICERIK_DOSYASI, sec).map_err(|e| e.to_string())?;
    z.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
    for (ad, veri) in &belge.ek_dosyalar {
        if ad == ICERIK_DOSYASI || ad == IMZA_DOSYASI {
            continue;
        }
        z.start_file(ad.as_str(), sec).map_err(|e| e.to_string())?;
        z.write_all(veri).map_err(|e| e.to_string())?;
    }
    Ok(z.finish().map_err(|e| e.to_string())?.into_inner())
}

/// `content.xml` metnini üretir. Metin konumları baştan hesaplanır (UTF-16 birimi).
pub fn icerik_yaz(belge: &Belge) -> String {
    let mut y = Yazici { metin: String::new(), konum: 0, xml: String::new() };
    if let Some(k) = &belge.ust_bilgi {
        y.kenar("header", k);
    }
    if let Some(k) = &belge.alt_bilgi {
        y.kenar("footer", k);
    }
    for b in &belge.govde {
        y.blok(b);
    }
    let mut s = String::with_capacity(y.xml.len() + y.metin.len() + 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n");
    s.push_str(&ac("template", &belge.kok));
    s.push('\n');
    s.push_str("<content><![CDATA[");
    s.push_str(&cdata(&y.metin));
    s.push_str("]]></content>");
    s.push_str("<properties>");
    s.push_str(&bos("pageFormat", &belge.sayfa));
    s.push_str("</properties>\n");
    s.push_str(&ac("elements", &belge.govde_oz));
    s.push_str(&y.xml);
    s.push_str("</elements>\n<styles>");
    for st in &belge.stiller {
        let mut oz = st.oz.clone();
        if oz.al("name").is_none() {
            oz.0.insert(0, ("name".into(), st.ad.clone()));
        }
        s.push_str(&bos("style", &oz));
    }
    s.push_str("</styles>\n</template>\n");
    s
}

struct Yazici {
    metin: String,
    /// `metin`in UTF-16 uzunluğu
    konum: usize,
    xml: String,
}

impl Yazici {
    fn kenar(&mut self, etiket: &str, k: &Kenar) {
        self.xml.push_str(&ac(etiket, &k.oz));
        for b in &k.bloklar {
            self.blok(b);
        }
        self.xml.push_str(&format!("</{etiket}>"));
    }

    fn blok(&mut self, b: &Blok) {
        match b {
            Blok::Paragraf(p) => {
                self.xml.push_str(&ac("paragraph", &p.oz));
                for c in &p.parcalar {
                    self.parca(c);
                }
                self.xml.push_str("</paragraph>\n");
            }
            Blok::Tablo(t) => {
                self.xml.push_str(&ac("table", &t.oz));
                for r in &t.satirlar {
                    self.xml.push_str(&ac("row", &r.oz));
                    for h in &r.hucreler {
                        self.xml.push_str(&ac("cell", &h.oz));
                        for b in &h.bloklar {
                            self.blok(b);
                        }
                        self.xml.push_str("</cell>");
                    }
                    self.xml.push_str("</row>");
                }
                self.xml.push_str("</table>\n");
            }
        }
    }

    fn parca(&mut self, c: &Parca) {
        let m = gecerli_xml_metni(c.metin());
        let uz = m.encode_utf16().count();
        let mut oz = Ozellikler(vec![("startOffset".into(), self.konum.to_string()), ("length".into(), uz.to_string())]);
        oz.0.extend(c.oz().0.iter().cloned());
        self.xml.push_str(&bos(c.etiket(), &oz));
        self.metin.push_str(&m);
        self.konum += uz;
    }
}

fn oz_metni(oz: &Ozellikler) -> String {
    oz.0.iter().map(|(a, d)| format!(" {a}=\"{}\"", kacis(d))).collect()
}

fn ac(etiket: &str, oz: &Ozellikler) -> String {
    format!("<{etiket}{}>", oz_metni(oz))
}

fn bos(etiket: &str, oz: &Ozellikler) -> String {
    format!("<{etiket}{} />", oz_metni(oz))
}

/// Özellik değeri kaçışı (satır sonu ve sekme de: XML özellik normalleştirmesi onları boşluğa çevirirdi).
fn kacis(d: &str) -> String {
    let mut s = String::with_capacity(d.len());
    for ch in d.chars() {
        match ch {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\n' => s.push_str("&#10;"),
            '\r' => s.push_str("&#13;"),
            '\t' => s.push_str("&#9;"),
            c if gecerli_xml_karakteri(c) => s.push(c),
            _ => s.push('\u{FFFD}'),
        }
    }
    s
}

fn gecerli_xml_karakteri(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// Metin XML'e yazılabilir hâle getirilir:
/// - XML ayrıştırıcıları `\r\n` ve tek `\r`'yi `\n`'ye çevirir (CDATA içinde de). Önceden çevrilir; yoksa okuyan
///   taraf metni kısa görür ve sonraki bütün konumlar kayardı.
/// - XML'de bulunamayacak denetim karakterleri U+FFFD olur (UTF-16 uzunluğu değişmez).
fn gecerli_xml_metni(m: &str) -> String {
    m.replace("\r\n", "\n")
        .chars()
        .map(|c| match c {
            '\r' => '\n',
            c if gecerli_xml_karakteri(c) => c,
            _ => '\u{FFFD}',
        })
        .collect()
}

/// CDATA içinde "]]>" bölünür.
fn cdata(m: &str) -> String {
    m.replace("]]>", "]]]]><![CDATA[>")
}
