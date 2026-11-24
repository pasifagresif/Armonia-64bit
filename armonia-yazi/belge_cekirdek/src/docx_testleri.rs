//! DOCX okuma testleri. Örnek DOCX, Word'ün ürettiği yapıya göre elle kuruldu (gerçek örnekler kullanıcıdan
//! gelecek). Claude, 2026-11-23.

use crate::docx;
use crate::model::*;
use crate::udf;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;

fn belge_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document {NS}><w:body>
<w:p><w:pPr><w:pStyle w:val="Baslik"/></w:pPr><w:r><w:t>T.C.</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="both"/><w:ind w:firstLine="709"/><w:spacing w:line="360" w:lineRule="auto"/><w:tabs><w:tab w:val="left" w:pos="2268"/></w:tabs></w:pPr>
  <w:r><w:rPr><w:b/></w:rPr><w:t>Davacı</w:t></w:r><w:r><w:tab/></w:r><w:r><w:rPr><w:i/><w:color w:val="FF0000"/></w:rPr><w:t xml:space="preserve">: Ali Veli</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Birinci madde</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>Madde imli</w:t></w:r></w:p>
<w:p><w:r><w:t>Üst satır</w:t><w:br/><w:t>Alt satır</w:t></w:r></w:p>
<w:p><w:hyperlink r:id="rId9"><w:r><w:t>bağlantı metni</w:t></w:r></w:hyperlink><w:ins><w:r><w:t> eklenen</w:t></w:r></w:ins><w:del><w:r><w:delText> silinen</w:delText></w:r></w:del><w:r><w:footnoteReference w:id="1"/></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="457200"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TabloIzgara"/></w:tblPr><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/></w:tblGrid>
  <w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>
  <w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Birleşik</w:t></w:r></w:p></w:tc></w:tr>
  <w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>Dikey</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr>
  <w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:sectPr><w:headerReference w:type="default" r:id="rId6"/><w:footerReference w:type="default" r:id="rId7"/>
  <w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1418" w:right="1134" w:bottom="1134" w:left="1701" w:header="709" w:footer="567"/></w:sectPr>
</w:body></w:document>"#
    )
}

const STILLER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi"/><w:sz w:val="22"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
<w:style w:type="paragraph" w:styleId="Baslik"><w:basedOn w:val="Normal"/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/><w:b/><w:sz w:val="28"/></w:rPr></w:style>
<w:style w:type="table" w:styleId="TabloIzgara"><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/><w:insideH w:val="single" w:sz="4"/></w:tblBorders></w:tblPr></w:style>
</w:styles>"#;

const TEMA: &str = r#"<?xml version="1.0"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme>
<a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;

const NUMARALAR: &str = r#"<?xml version="1.0"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="%1)"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#;

const ILISKI: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId5" Type="image" Target="media/image1.png"/><Relationship Id="rId6" Type="header" Target="header1.xml"/>
<Relationship Id="rId7" Type="footer" Target="footer1.xml"/><Relationship Id="rId9" Type="hyperlink" Target="https://example.com" TargetMode="External"/></Relationships>"#;

fn ust() -> String {
    format!(r#"<?xml version="1.0"?><w:hdr {NS}><w:p><w:r><w:t>Büro Antet</w:t></w:r></w:p></w:hdr>"#)
}

fn alt() -> String {
    format!(
        r#"<?xml version="1.0"?><w:ftr {NS}><w:p><w:pPr><w:jc w:val="center"/></w:pPr>
<w:r><w:t xml:space="preserve">Sayfa </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>
<w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>
<w:r><w:t xml:space="preserve"> / </w:t></w:r><w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>3</w:t></w:r></w:fldSimple></w:p></w:ftr>"#
    )
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0";

pub(crate) fn ornek_docx() -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let sec = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let dosyalar: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", b"<Types/>".to_vec()),
        ("word/document.xml", belge_xml().into_bytes()),
        ("word/styles.xml", STILLER.as_bytes().to_vec()),
        ("word/theme/theme1.xml", TEMA.as_bytes().to_vec()),
        ("word/numbering.xml", NUMARALAR.as_bytes().to_vec()),
        ("word/_rels/document.xml.rels", ILISKI.as_bytes().to_vec()),
        ("word/header1.xml", ust().into_bytes()),
        ("word/footer1.xml", alt().into_bytes()),
        ("word/footnotes.xml", b"<w:footnotes/>".to_vec()),
        ("word/media/image1.png", PNG.to_vec()),
    ];
    for (ad, v) in dosyalar {
        z.start_file(ad, sec).unwrap();
        z.write_all(&v).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn p(b: &Blok) -> &Paragraf {
    match b {
        Blok::Paragraf(p) => p,
        _ => panic!("paragraf bekleniyordu"),
    }
}

#[test]
fn docx_okunur() {
    let bayt = ornek_docx();
    assert!(docx::docx_mi(&bayt));
    assert!(!udf::udf_mi(&bayt));
    let o = docx::oku(&bayt).unwrap();
    let u = o.uyarilar.join(" | ");
    for beklenen in ["Dipnotlar", "İzlenen değişiklikler", "Dikey birleştirilmiş"] {
        assert!(u.contains(beklenen), "uyarı yok: {beklenen} / {u}");
    }
    let b = &o.belge;
    // Başlık: stil zinciri (Normal → Baslik): Arial 14 kalın, ortalı
    let p1 = p(&b.govde[0]);
    assert_eq!(p1.oz.al("Alignment"), Some("1"));
    let r = p1.parcalar[0].oz();
    assert_eq!((r.al("family"), r.al("size"), r.al("bold")), (Some("Arial"), Some("14"), Some("true")));
    assert_eq!(p1.parcalar.last().unwrap().metin(), "\n");
    // Doğrudan biçim + tema yazı tipi (minorHAnsi → Calibri), docDefaults 11 pt
    let p2 = p(&b.govde[1]);
    assert_eq!(p2.oz.al("Alignment"), Some("3"));
    assert_eq!(p2.oz.sayi("FirstLineIndent"), Some(35.45));
    assert_eq!(p2.oz.sayi("LineSpacing"), Some(0.5));
    assert_eq!(p2.oz.al("TabSet"), Some("113.4:0:0"));
    assert_eq!((p2.parcalar[0].metin(), p2.parcalar[0].oz().al("family"), p2.parcalar[0].oz().al("size")), ("Davacı", Some("Calibri"), Some("11")));
    assert!(p2.parcalar[0].oz().evet_mi("bold"));
    assert!(matches!(p2.parcalar[1], Parca::Sekme { .. }));
    assert!(p2.parcalar[2].oz().evet_mi("italic"));
    assert_eq!(p2.parcalar[2].oz().al("foreground"), Some("-65536"));
    // Numaralı liste: "%1)" → PARANTHESE, seviye girintisi (720 / asılı 360)
    let p3 = p(&b.govde[2]);
    assert_eq!((p3.oz.al("Numbered"), p3.oz.al("NumberType"), p3.oz.al("ListId")), (Some("true"), Some("NUMBER_TYPE_NUMBER_PARANTHESE"), Some("1")));
    assert_eq!((p3.oz.sayi("LeftIndent"), p3.oz.sayi("FirstLineIndent")), (Some(36.0), Some(-18.0)));
    assert_eq!(p(&b.govde[3]).oz.al("Bulleted"), Some("true"));
    // Satır içi satır sonu (Shift+Enter) → aynı paragrafta satır sonu karakteri
    assert_eq!(p(&b.govde[4]).parcalar[0].metin(), "Üst satır\nAlt satır");
    // Bağlantı metni + kabul edilmiş ekleme; silinen yok
    assert_eq!(p(&b.govde[5]).duz_metin(), "bağlantı metni eklenen");
    // Resim: 914400 EMU = 72 pt
    let Parca::Resim { oz, metin } = &p(&b.govde[6]).parcalar[0] else { panic!("resim") };
    assert_eq!((oz.sayi("width"), oz.sayi("height")), (Some(72.0), Some(36.0)));
    assert!(oz.al("imageData").unwrap().starts_with("iVBORw0KGgo"));
    assert_eq!(metin, "\u{b8}");
    // Tablo: ızgara 2000/3000 twip → 100/150 pt, stilden kenarlık, birleşik satırın kendi oranı
    let Blok::Tablo(t) = &b.govde[7] else { panic!("tablo") };
    assert_eq!((t.oz.al("columnSpans"), t.oz.al("border")), (Some("100,150"), Some("borderCell")));
    assert_eq!(t.satirlar[1].hucreler.len(), 1);
    assert_eq!(t.satirlar[1].oz.al("columnSpans"), Some("250"));
    assert_eq!(p(&t.satirlar[1].hucreler[0].bloklar[0]).duz_metin(), "Birleşik");
    assert_eq!(t.satirlar[3].hucreler.len(), 2);
    // Sayfa: kenar boşlukları twip/20
    assert_eq!((b.sayfa.sayi("leftMargin"), b.sayfa.sayi("topMargin"), b.sayfa.al("paperOrientation")), (Some(85.05), Some(70.9), Some("1")));
    assert_eq!(b.sayfa.sayi("footerFOffset"), Some(28.35));
    // Üst bilgi metni; alt bilgide "Sayfa X / Y" → UDF sayfa numarası (8 + orta 32 + toplam 2048)
    assert_eq!(p(&b.ust_bilgi.as_ref().unwrap().bloklar[0]).duz_metin(), "Büro Antet");
    let alt = b.alt_bilgi.as_ref().unwrap();
    assert_eq!(alt.oz.al("pageNumber-spec"), Some("BSP32_2088"));
    assert_eq!(alt.oz.al("pageNumber-foreStr"), Some("Sayfa"));
    assert!(alt.bloklar.is_empty(), "sayfa numarası paragrafı alt bilgi özelliklerine taşındı");
}

#[test]
fn docx_udf_olarak_kaydedilir() {
    let o = docx::oku(&ornek_docx()).unwrap();
    let udf_bayt = udf::yaz(&o.belge).unwrap();
    let u = udf::oku(&udf_bayt).unwrap();
    assert!(u.uyarilar.is_empty(), "{:?}", u.uyarilar);
    assert_eq!(u.belge, o.belge, "DOCX'ten gelen belge UDF gidiş-dönüşünde kayıpsız");
    let g = crate::gorunum::gorunum(&u.belge);
    assert_eq!(g.sayfa_numarasi.unwrap().metin(2, 5), "Sayfa 2/5");
}

#[test]
fn bozuk_docx() {
    assert!(matches!(docx::oku(b"zip degil"), Err(docx::DocxHatasi::Zip(_))));
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("word/baska.xml", SimpleFileOptions::default()).unwrap();
    z.write_all(b"<a/>").unwrap();
    let bayt = z.finish().unwrap().into_inner();
    assert!(!docx::docx_mi(&bayt));
    assert_eq!(docx::oku(&bayt).unwrap_err(), docx::DocxHatasi::BelgeYok);
}

/// Örnek DOCX'i dosyaya yazar (arayüz testi için): ARMONIA_YAZI_DOCX_CIKTI=<yol> cargo test -- --ignored docx_yaz
#[test]
#[ignore]
fn ornek_docx_dosyaya_yaz() {
    let yol = std::env::var("ARMONIA_YAZI_DOCX_CIKTI").expect("ARMONIA_YAZI_DOCX_CIKTI verilmedi");
    std::fs::write(yol, ornek_docx()).unwrap();
}
