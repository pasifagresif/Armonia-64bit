//! UDF alan değerlerini metne yerleştirme.
//!
//! UYAP'ın portal belgelerinde (ör. "Avukat Portal Genel Talebi") alanlar metinde yalnız ADIYLA durur
//! (`<field fieldName="dosyaNo" startOffset=".." length="7"/>` → CDATA'da "dosyaNo"); değerler `template`
//! altındaki ayrı `<data>` öğesindedir (`<data><dosyaNo>2026/38696</dosyaNo>..</data>`). UYAP'ın ekrandaki
//! görüntüleyicisi açarken bu ikisini birleştirir; birleştirmeyen her şey (UYAP Doküman Editörü, PDF çevirisi,
//! Armonia Yazı) alan adlarını gösterir.
//!
//! `doldur` her alanın metnini `<data>`'daki değeriyle değiştirir ve bütün öğelerin metin konumlarını
//! (`startOffset`/`length`, UTF-16 birimi) kaydırır. `<data>` öğesi aynen kalır (UYAP görüntüleyicisi yine aynı
//! değerleri koyar). İçerik değiştiği için `sign.sgn` (e-imza) bu kopyadan çıkarılır: imza asıl dosyada geçerlidir.
//! Claude, 2026-11-24.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use roxmltree::{Document, Node, ParsingOptions};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const AZAMI_DOSYA: u64 = 32 << 20;
const AZAMI_TOPLAM: u64 = 128 << 20;
const AZAMI_GIRDI_SAYISI: usize = 64;

/// Değer ataması: (eski başlangıç, eski bitiş, yeni metin) — UTF-16 birimi.
struct Degisim {
    bas: usize,
    son: usize,
    yeni: Vec<u16>,
}

impl Degisim {
    fn fark(&self) -> isize {
        self.yeni.len() as isize - (self.son - self.bas) as isize
    }
}

/// `<data>` altındaki değerler. Aynı ad birden çok kez geçebilir (gerçek belgede `tarih` önce dolu, sonda boş):
/// ilk DOLU değer alınır; hepsi boşsa boş.
fn veri_degerleri(kok: Node) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = HashMap::new();
    for data in kok.children().filter(|n| n.is_element() && n.tag_name().name() == "data") {
        for c in data.children().filter(Node::is_element) {
            let deger: String = c.descendants().filter(Node::is_text).filter_map(|t| t.text()).collect();
            let deger = deger.replace("\r\n", "\n").replace('\r', "\n");
            match m.get(c.tag_name().name()) {
                Some(eski) if !eski.trim().is_empty() => {}
                _ => {
                    m.insert(c.tag_name().name().to_string(), deger);
                }
            }
        }
    }
    m
}

fn sayi(n: Node, ad: &str) -> Option<usize> {
    n.attribute(ad).and_then(|d| d.trim().parse().ok())
}

/// XML'de yazılamayan denetim karakterlerini atar (CDATA'ya girecek).
fn temiz(m: &str) -> String {
    m.chars()
        .filter(|&c| matches!(c, '\t' | '\n' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'))
        .collect()
}

/// `content.xml` metnini doldurur. Doldurulacak alan yoksa `None`.
pub fn icerik_doldur(xml: &str) -> Result<Option<String>, String> {
    let secenek = ParsingOptions { allow_dtd: false, nodes_limit: 5_000_000 };
    let doc = Document::parse_with_options(xml, secenek).map_err(|e| format!("content.xml okunamadı: {e}"))?;
    let kok = doc.root_element();
    if kok.tag_name().name() != "template" {
        return Ok(None);
    }
    let degerler = veri_degerleri(kok);
    if degerler.is_empty() {
        return Ok(None);
    }
    let Some(icerik) = kok.children().find(|n| n.is_element() && n.tag_name().name() == "content") else {
        return Ok(None);
    };
    let metin: Vec<u16> = icerik.text().unwrap_or("").encode_utf16().collect();

    // Değiştirilecek alanlar
    let mut degisimler: Vec<Degisim> = Vec::new();
    for f in kok.descendants().filter(|n| n.is_element() && n.tag_name().name() == "field") {
        let (Some(ad), Some(bas), Some(uz)) = (f.attribute("fieldName"), sayi(f, "startOffset"), sayi(f, "length")) else {
            continue;
        };
        let Some(deger) = degerler.get(ad) else { continue };
        let son = bas.saturating_add(uz);
        if uz == 0 || son > metin.len() {
            continue;
        }
        // Metin alan adı değilse (ör. UYAP zaten doldurmuşsa) dokunma
        let simdiki = String::from_utf16_lossy(&metin[bas..son]);
        if simdiki != ad {
            continue;
        }
        degisimler.push(Degisim { bas, son, yeni: temiz(deger).encode_utf16().collect() });
    }
    if degisimler.is_empty() {
        return Ok(None);
    }
    degisimler.sort_by_key(|d| d.bas);
    // Çakışan aralık olmamalı; varsa güvenli tarafta kal
    if degisimler.windows(2).any(|w| w[1].bas < w[0].son) {
        return Err("alan konumları çakışıyor".into());
    }

    // Konum eşleme: bir noktadan önce BİTEN değişimlerin farkları toplanır.
    let kaydir = |p: usize| -> usize {
        let mut f: isize = 0;
        for d in &degisimler {
            if d.son <= p {
                f += d.fark();
            } else {
                break;
            }
        }
        (p as isize + f).max(0) as usize
    };

    // Yeni metin
    let mut yeni_metin: Vec<u16> = Vec::with_capacity(metin.len() + 256);
    let mut i = 0;
    for d in &degisimler {
        yeni_metin.extend_from_slice(&metin[i..d.bas]);
        yeni_metin.extend_from_slice(&d.yeni);
        i = d.son;
    }
    yeni_metin.extend_from_slice(&metin[i..]);
    let yeni_metin = String::from_utf16_lossy(&yeni_metin);

    // Kaynak metinde yapılacak bayt değişiklikleri: (aralık, yeni metin)
    let mut yamalar: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    yamalar.push((icerik.range(), format!("<content><![CDATA[{}]]></content>", yeni_metin.replace("]]>", "]]]]><![CDATA[>"))));
    for n in kok.descendants().filter(Node::is_element) {
        if n.range().start >= icerik.range().start && n.range().end <= icerik.range().end {
            continue;
        }
        let (Some(bas), Some(uz)) = (sayi(n, "startOffset"), sayi(n, "length")) else { continue };
        let son = bas.saturating_add(uz);
        let (yb, ys) = (kaydir(bas), kaydir(son));
        let yuz = ys.saturating_sub(yb);
        if yb != bas {
            if let Some(a) = n.attributes().find(|a| a.name() == "startOffset") {
                yamalar.push((a.range_value(), yb.to_string()));
            }
        }
        if yuz != uz {
            if let Some(a) = n.attributes().find(|a| a.name() == "length") {
                yamalar.push((a.range_value(), yuz.to_string()));
            }
        }
    }
    yamalar.sort_by_key(|(r, _)| r.start);
    let mut cikti = String::with_capacity(xml.len() + 1024);
    let mut i = 0;
    for (r, s) in yamalar {
        if r.start < i {
            return Err("iç hata: çakışan yama".into());
        }
        cikti.push_str(&xml[i..r.start]);
        cikti.push_str(&s);
        i = r.end;
    }
    cikti.push_str(&xml[i..]);
    Ok(Some(cikti))
}

/// UDF baytlarını doldurur. Doldurulacak alan yoksa `None` (asıl dosya olduğu gibi kullanılır).
/// Doldurulmuş kopyada `sign.sgn` yoktur (içerik değişti; imza asıl dosyada geçerli).
pub fn doldur(bayt: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if !bayt.starts_with(b"PK") {
        return Ok(None);
    }
    let mut z = ZipArchive::new(Cursor::new(bayt)).map_err(|e| format!("UDF açılamadı: {e}"))?;
    if z.len() > AZAMI_GIRDI_SAYISI {
        return Err("UDF içinde çok fazla dosya".into());
    }
    let mut toplam = 0u64;
    let mut dosyalar: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let ad = f.name().to_string();
        let mut v = Vec::new();
        f.by_ref().take(AZAMI_DOSYA + 1).read_to_end(&mut v).map_err(|e| e.to_string())?;
        toplam += v.len() as u64;
        if v.len() as u64 > AZAMI_DOSYA || toplam > AZAMI_TOPLAM {
            return Err("UDF çok büyük".into());
        }
        if dosyalar.iter().any(|(a, _)| *a == ad) {
            return Err(format!("UDF içinde iki kez {ad}"));
        }
        dosyalar.push((ad, v));
    }
    let Some(icerik) = dosyalar.iter().find(|(a, _)| a == "content.xml").map(|(_, v)| v) else {
        return Ok(None);
    };
    let xml = std::str::from_utf8(icerik).map_err(|_| "content.xml UTF-8 değil".to_string())?;
    let Some(yeni) = icerik_doldur(xml)? else { return Ok(None) };

    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let sec = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (ad, v) in &dosyalar {
        if ad == "sign.sgn" {
            continue;
        }
        w.start_file(ad.as_str(), sec).map_err(|e| e.to_string())?;
        w.write_all(if ad == "content.xml" { yeni.as_bytes() } else { v }).map_err(|e| e.to_string())?;
    }
    Ok(Some(w.finish().map_err(|e| e.to_string())?.into_inner()))
}

/// Doldurulabiliyorsa doldurulmuş kopyayı, değilse (ya da hata olursa) asıl baytları verir.
pub fn doldur_ya_da_asil(bayt: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    match doldur(bayt) {
        Ok(Some(v)) => std::borrow::Cow::Owned(v),
        Ok(None) => std::borrow::Cow::Borrowed(bayt),
        Err(e) => {
            println!("[udf] alan değerleri yerleştirilemedi: {e}");
            std::borrow::Cow::Borrowed(bayt)
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    const ORNEK: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" ?><template format_id="1.8"><content><![CDATA[Sayın dosyaNo için"#,
        "\ntalep\nŞ tarih\n",
        r#"]]></content><properties><pageFormat /></properties><elements resolver="hvl-default">"#,
        r#"<paragraph><content startOffset="0" length="6" /><field fieldName="dosyaNo" bold="true" startOffset="6" length="7" /><content startOffset="13" length="6" /></paragraph>"#,
        r#"<paragraph><field fieldName="talep" startOffset="19" length="5" /><content startOffset="24" length="1" /></paragraph>"#,
        r#"<paragraph><content startOffset="25" length="2" /><field fieldName="tarih" startOffset="27" length="5" /><content startOffset="32" length="1" /></paragraph>"#,
        r#"</elements><styles><style name="hvl-default" /></styles>"#,
        r#"<data><tarih>24/11/2026</tarih><dosyaNo>2026/38696</dosyaNo><talep>Hesap &amp; "kayıt"</talep><tarih></tarih></data></template>"#
    );

    fn metinler(xml: &str) -> Vec<(String, String)> {
        let d = Document::parse(xml).unwrap();
        let k = d.root_element();
        let m: Vec<u16> = k.children().find(|n| n.tag_name().name() == "content").unwrap().text().unwrap().encode_utf16().collect();
        k.descendants()
            .filter(|n| n.attribute("startOffset").is_some())
            .map(|n| {
                let b = sayi(n, "startOffset").unwrap();
                let u = sayi(n, "length").unwrap();
                (n.tag_name().name().to_string(), String::from_utf16_lossy(&m[b..b + u]))
            })
            .collect()
    }

    #[test]
    fn alanlar_doldurulur_konumlar_kayar() {
        let y = icerik_doldur(ORNEK).unwrap().unwrap();
        let m = metinler(&y);
        assert_eq!(
            m,
            vec![
                ("content".into(), "Sayın ".into()),
                ("field".into(), "2026/38696".into()),
                ("content".into(), " için\n".into()),
                ("field".into(), "Hesap & \"kayıt\"".into()),
                ("content".into(), "\n".into()),
                ("content".into(), "Ş ".into()),
                ("field".into(), "24/11/2026".into()),
                ("content".into(), "\n".into()),
            ]
        );
        assert!(y.contains("<data><tarih>24/11/2026</tarih>"), "data öğesi korunur");
        assert!(y.contains(r#"fieldName="dosyaNo" bold="true""#), "öznitelikler korunur");
    }

    #[test]
    fn ikinci_kez_doldurmak_degistirmez() {
        let y = icerik_doldur(ORNEK).unwrap().unwrap();
        assert_eq!(icerik_doldur(&y).unwrap(), None);
    }

    #[test]
    fn data_yoksa_dokunulmaz() {
        let x = ORNEK.split("<data>").next().unwrap().to_string() + "</template>";
        assert_eq!(icerik_doldur(&x).unwrap(), None);
    }

    #[test]
    fn imza_doldurulmus_kopyadan_cikar() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        let sec = SimpleFileOptions::default();
        w.start_file("content.xml", sec).unwrap();
        w.write_all(ORNEK.as_bytes()).unwrap();
        w.start_file("sign.sgn", sec).unwrap();
        w.write_all(b"imza").unwrap();
        let udf = w.finish().unwrap().into_inner();
        let y = doldur(&udf).unwrap().unwrap();
        let mut z = ZipArchive::new(Cursor::new(y)).unwrap();
        assert!(z.by_name("sign.sgn").is_err());
        let mut s = String::new();
        z.by_name("content.xml").unwrap().read_to_string(&mut s).unwrap();
        assert!(s.contains("2026/38696"));
    }

    /// Gerçek belgeyle: UDF_ORNEK=<dosya.udf> cargo test --lib udf_alan -- --ignored
    #[test]
    #[ignore]
    fn gercek_belge() {
        let yol = std::env::var("UDF_ORNEK").expect("UDF_ORNEK");
        let b = std::fs::read(yol).unwrap();
        let y = doldur(&b).unwrap().expect("doldurulacak alan yok");
        let mut z = ZipArchive::new(Cursor::new(&y)).unwrap();
        let mut s = String::new();
        z.by_name("content.xml").unwrap().read_to_string(&mut s).unwrap();
        for (t, m) in metinler(&s).iter().filter(|(t, _)| t == "field") {
            println!("{t}: {m:?}");
        }
        std::fs::write(std::env::temp_dir().join("udf_alan_doldurulmus.udf"), &y).unwrap();
        let pdf = crate::udf_pdf::udf_pdf_uret(&y).unwrap();
        std::fs::write(std::env::temp_dir().join("udf_alan_doldurulmus.pdf"), pdf).unwrap();
    }
}
