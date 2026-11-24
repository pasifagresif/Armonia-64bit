//! Çekirdek testleri. Örnek belge UYAP'ın ürettiği yapıya göre elle yazıldı (gerçek UDF örnekleri kullanıcıdan
//! gelecek; kişisel veri depoya girmez). Claude, 2026-11-23.

use crate::model::*;
use crate::udf::{self, UdfHatasi};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

// Metin (UTF-16 konumları):
//  0 "Üst bilgi" 9 "\n" | 10 "1" 11 "\n" | 12 "T.C." 16 "\n" | 17 "Davacı" 23 "\t" 24 "Ali ]]> Veli 😀"(15) 39 "\n"
// | 40 "A1" 42 "\n" 43 "B1" 45 "\n" | 46 "Madde" 51 "\n" | 52 "R" (resim yer tutucu) 53 "\n"
const ORNEK: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<template format_id="1.8" >
<content><![CDATA[Üst bilgi
1
T.C.
Davacı	Ali ]]]]><![CDATA[> Veli 😀
A1
B1
Madde
R
]]></content><properties><pageFormat mediaSizeName="1" leftMargin="70.86" rightMargin="70.86" topMargin="56.69" bottomMargin="56.69" paperOrientation="1" headerFOffset="20.0" footerFOffset="20.0" /></properties>
<elements resolver="hvl-default">
<header background="-1" foreground="-16777216"><paragraph Alignment="1"><content startOffset="0" length="9" size="10" /><content startOffset="9" length="1" /></paragraph></header>
<footer background="-1" foreground="-16777216"><paragraph Alignment="2"><field startOffset="10" length="1" fieldName="pageNumber" /><content startOffset="11" length="1" /></paragraph></footer>
<paragraph Alignment="1"><content startOffset="12" length="4" bold="true" family="Times New Roman" size="12" /><content startOffset="16" length="1" /></paragraph>
<paragraph Alignment="3" LeftIndent="10.0" FirstLineIndent="28.35" LineSpacing="0.5" TabSet="70.0:0:0"><content startOffset="17" length="6" bold="true" /><tab startOffset="23" length="1" /><content startOffset="24" length="15" italic="true" foreground="-65536" description="a &quot;tırnak&quot; &amp; satır&#10;sonu" /><content startOffset="39" length="1" /></paragraph>
<table columnSpans="200,300" border="borderCell"><row><cell><paragraph><content startOffset="40" length="2" /><content startOffset="42" length="1" /></paragraph></cell><cell><paragraph><content startOffset="43" length="2" /><content startOffset="45" length="1" /></paragraph></cell></row></table>
<paragraph Numbered="true" NumberType="NUMBER_TYPE_NUMBER_DOT" ListId="1" ListLevel="1"><content startOffset="46" length="5" /><content startOffset="51" length="1" /></paragraph>
<paragraph><image startOffset="52" length="1" imageData="iVBORw0KGgo=" width="10.0" height="12.0" /><content startOffset="53" length="1" /></paragraph>
</elements>
<styles><style name="default" description="Geçerli" family="Dialog" size="12" bold="false" italic="false" foreground="-13421773" /><style name="hvl-default" family="Times New Roman" size="12" description="Gövde" /></styles>
</template>
"#;

fn zip_yap(dosyalar: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let sec = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (ad, veri) in dosyalar {
        z.start_file(*ad, sec).unwrap();
        z.write_all(veri).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// Örnek belge, okunmuş hâliyle (stil testleri de kullanır).
pub(crate) fn ornek_belge() -> Belge {
    udf::oku(&ornek_udf()).unwrap().belge
}

fn ornek_udf() -> Vec<u8> {
    zip_yap(&[("content.xml", ORNEK.as_bytes()), ("documentproperties.xml", b"<properties/>")])
}

fn paragraf(b: &Blok) -> &Paragraf {
    match b {
        Blok::Paragraf(p) => p,
        _ => panic!("paragraf bekleniyordu"),
    }
}

#[test]
fn ornek_okunur() {
    let o = udf::oku(&ornek_udf()).unwrap();
    assert!(o.uyarilar.is_empty(), "uyarı: {:?}", o.uyarilar);
    assert!(!o.imzali);
    let b = &o.belge;
    assert_eq!(b.kok.al("format_id"), Some("1.8"));
    assert_eq!(b.sayfa.sayi("leftMargin"), Some(70.86));
    assert_eq!(b.govde_oz.al("resolver"), Some("hvl-default"));
    assert_eq!(paragraf(&b.ust_bilgi.as_ref().unwrap().bloklar[0]).duz_metin(), "Üst bilgi");
    let alt = paragraf(&b.alt_bilgi.as_ref().unwrap().bloklar[0]);
    assert!(matches!(&alt.parcalar[0], Parca::Alan { metin, oz } if metin == "1" && oz.al("fieldName") == Some("pageNumber")));

    assert_eq!(b.govde.len(), 5);
    let p1 = paragraf(&b.govde[0]);
    assert!(p1.parcalar[0].oz().evet_mi("bold"));
    assert_eq!(p1.duz_metin(), "T.C.");
    // Konum özellikleri modele girmez
    assert!(p1.parcalar[0].oz().al("startOffset").is_none());

    let p2 = paragraf(&b.govde[1]);
    assert_eq!(p2.oz.al("Alignment"), Some("3"));
    assert!(matches!(&p2.parcalar[1], Parca::Sekme { metin, .. } if metin == "\t"));
    // UTF-16 konumu: emoji 2 birim; "]]>" CDATA bölünmesinden sağ çıkar
    assert_eq!(p2.parcalar[2].metin(), "Ali ]]> Veli 😀");
    assert_eq!(p2.parcalar[2].oz().al("description"), Some("a \"tırnak\" & satır\nsonu"));

    let Blok::Tablo(t) = &b.govde[2] else { panic!("tablo") };
    assert_eq!(t.oz.al("columnSpans"), Some("200,300"));
    assert_eq!(paragraf(&t.satirlar[0].hucreler[1].bloklar[0]).duz_metin(), "B1");

    assert!(paragraf(&b.govde[3]).oz.evet_mi("Numbered"));
    assert!(matches!(&paragraf(&b.govde[4]).parcalar[0], Parca::Resim { oz, .. } if oz.al("imageData") == Some("iVBORw0KGgo=")));

    assert_eq!(b.stiller.len(), 2);
    assert_eq!(b.stiller[1].ad, "hvl-default");
    assert_eq!(b.ek_dosyalar.len(), 1);
    assert_eq!(b.duz_metin(), "T.C.\nDavacı\tAli ]]> Veli 😀\nA1\nB1\nMadde\nR");
}

#[test]
fn gidis_donus_kayipsiz() {
    let o1 = udf::oku(&ornek_udf()).unwrap();
    let bayt = udf::yaz(&o1.belge).unwrap();
    assert!(udf::udf_mi(&bayt));
    let o2 = udf::oku(&bayt).unwrap();
    assert!(o2.uyarilar.is_empty(), "uyarı: {:?}", o2.uyarilar);
    assert_eq!(o1.belge, o2.belge);
    // İkinci tur bayt bayt aynı XML
    assert_eq!(udf::icerik_yaz(&o1.belge), udf::icerik_yaz(&o2.belge));
}

#[test]
fn yazilan_konumlar_sirali_ve_utf16() {
    let o = udf::oku(&ornek_udf()).unwrap();
    let xml = udf::icerik_yaz(&o.belge);
    // UYAP sırası: üst bilgi (0..10) → gövde → alt bilgi en sonda; konum özellikleri öğenin sonunda
    assert!(xml.contains(r#"<content size="10" startOffset="0" length="9" />"#));
    // emoji içeren parça 15 UTF-16 birimi; alt bilgi artık arada olmadığı için 22'de başlar
    assert!(xml.contains(r#"sonu" startOffset="22" length="15" />"#));
    assert!(xml.contains("Ali ]]]]><![CDATA[> Veli"));
    assert!(xml.contains("&#10;sonu"));
    let govde_sonu = xml.rfind("</paragraph><footer").expect("alt bilgi gövdeden sonra");
    assert!(xml[govde_sonu..].contains(r#"fieldName="pageNumber" startOffset="52" length="1" />"#));
    // UYAP biçimi: bildirim satırı, boşluklu açılış etiketleri, metnin sonunda göstergesiz "\n"
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" ?> \n\n<template format_id=\"1.8\" >\n"));
    assert!(xml.contains("<elements resolver=\"hvl-default\" >\n<header"));
    assert!(xml.contains("\n\n]]></content>"));
}

#[test]
fn resim_verisi_satirlara_bolunur() {
    let mut b = ornek_belge();
    if let Blok::Paragraf(p) = &mut b.govde[4] {
        if let Parca::Resim { oz, .. } = &mut p.parcalar[0] {
            // XML okuyucusu ham satır sonunu boşluğa çevirmiş hâli
            oz.ayarla("imageData", "AAAA BBBB\"<x>");
        }
    }
    let xml = udf::icerik_yaz(&b);
    assert!(xml.contains("imageData=\"AAAA\nBBBBx\""), "base64 dışı karakterler atılır, boşluk satır sonu olur");
    let o = udf::icerik_oku(&xml).unwrap();
    assert!(o.uyarilar.is_empty());
}

/// Gerçek UDF'ler (kişisel veri: depoya girmez). Klasör ARMONIA_YAZI_ORNEKLER ile verilir:
///   ARMONIA_YAZI_ORNEKLER=/yol cargo test -- --ignored gercek
/// Her dosya uyarısız okunmalı ve yeniden yazılan content.xml UYAP'ınkiyle bayt bayt aynı olmalı.
#[test]
#[ignore]
fn gercek_ornekler_bayt_bayt() {
    use std::io::Read;
    let Ok(klasor) = std::env::var("ARMONIA_YAZI_ORNEKLER") else {
        panic!("ARMONIA_YAZI_ORNEKLER verilmedi");
    };
    let mut sayi = 0;
    let mut hatalar = Vec::new();
    for g in std::fs::read_dir(&klasor).unwrap().flatten() {
        let yol = g.path();
        if yol.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("udf")) != Some(true) {
            continue;
        }
        sayi += 1;
        let ad = yol.file_name().unwrap().to_string_lossy().to_string();
        let bayt = std::fs::read(&yol).unwrap();
        let o = match udf::oku(&bayt) {
            Ok(o) => o,
            Err(e) => {
                hatalar.push(format!("{ad}: okunamadı: {e}"));
                continue;
            }
        };
        if !o.uyarilar.is_empty() {
            hatalar.push(format!("{ad}: uyarılar {:?}", o.uyarilar));
        }
        let mut asil = String::new();
        zip::ZipArchive::new(Cursor::new(&bayt)).unwrap().by_name("content.xml").unwrap().read_to_string(&mut asil).unwrap();
        let yeni = udf::icerik_yaz(&o.belge);
        if yeni != asil {
            let i = yeni.bytes().zip(asil.bytes()).position(|(a, b)| a != b).unwrap_or(yeni.len().min(asil.len()));
            hatalar.push(format!("{ad}: content.xml farklı (ilk fark bayt {i}, uzunluk {} / {})", yeni.len(), asil.len()));
        }
    }
    assert!(sayi > 0, "klasörde .udf yok");
    assert!(hatalar.is_empty(), "{sayi} dosyadan sorunlu olanlar:\n{}", hatalar.join("\n"));
}

#[test]
fn duzenleme_sonrasi_konumlar_yeniden_hesaplanir() {
    let mut b = udf::oku(&ornek_udf()).unwrap().belge;
    if let Blok::Paragraf(p) = &mut b.govde[0] {
        p.parcalar[0] = Parca::Metin { metin: "TÜRKİYE CUMHURİYETİ".into(), oz: p.parcalar[0].oz().clone() };
    }
    let o = udf::oku(&udf::yaz(&b).unwrap()).unwrap();
    assert!(o.uyarilar.is_empty());
    assert_eq!(paragraf(&o.belge.govde[0]).duz_metin(), "TÜRKİYE CUMHURİYETİ");
    assert_eq!(paragraf(&o.belge.govde[1]).parcalar[2].metin(), "Ali ]]> Veli 😀");
}

#[test]
fn satir_sonu_normallesir_konumlar_kaymaz() {
    let mut b = udf::oku(&ornek_udf()).unwrap().belge;
    if let Blok::Paragraf(p) = &mut b.govde[0] {
        p.parcalar[0] = Parca::Metin { metin: "a\r\nb\rc".into(), oz: Ozellikler::default() };
    }
    let o = udf::oku(&udf::yaz(&b).unwrap()).unwrap();
    assert_eq!(paragraf(&o.belge.govde[0]).parcalar[0].metin(), "a\nb\nc");
    assert_eq!(paragraf(&o.belge.govde[1]).parcalar[0].metin(), "Davacı");
}

#[test]
fn denetim_karakteri_degistirilir() {
    let mut b = udf::oku(&ornek_udf()).unwrap().belge;
    if let Blok::Paragraf(p) = &mut b.govde[0] {
        p.parcalar[0] = Parca::Metin { metin: "x\u{1}y".into(), oz: Ozellikler::default() };
    }
    let o = udf::oku(&udf::yaz(&b).unwrap()).unwrap();
    assert_eq!(paragraf(&o.belge.govde[0]).parcalar[0].metin(), "x\u{FFFD}y");
}

#[test]
fn imza_ayrilir_ve_geri_yazilmaz() {
    let bayt = zip_yap(&[("content.xml", ORNEK.as_bytes()), ("sign.sgn", b"imza")]);
    let o = udf::oku(&bayt).unwrap();
    assert!(o.imzali);
    assert!(o.belge.ek_dosyalar.is_empty());
    let yeni = udf::yaz(&o.belge).unwrap();
    assert!(!udf::oku(&yeni).unwrap().imzali);
}

#[test]
fn taninmayan_oge_uyarilir() {
    let xml = ORNEK.replace("<table ", "<grafik x=\"1\"/><table ");
    let o = udf::icerik_oku(&xml).unwrap();
    assert!(o.uyarilar.iter().any(|u| u.contains("<grafik>")), "{:?}", o.uyarilar);
    assert_eq!(o.belge.govde.len(), 5);
}

#[test]
fn bozuk_konum_uyarilir() {
    let xml = ORNEK.replace(r#"startOffset="46" length="5""#, r#"startOffset="46" length="5000""#);
    let o = udf::icerik_oku(&xml).unwrap();
    assert!(o.uyarilar.iter().any(|u| u.contains("kırpıldı")));
}

#[test]
fn dtd_ve_varlik_reddedilir() {
    let xml = r#"<?xml version="1.0"?><!DOCTYPE t [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;">]><template><content>&b;</content></template>"#;
    assert!(matches!(udf::icerik_oku(xml), Err(UdfHatasi::Xml(_))));
}

#[test]
fn kok_template_olmali() {
    assert!(matches!(udf::icerik_oku("<html/>"), Err(UdfHatasi::Bicim(_))));
}

#[test]
fn zip_bombasi_reddedilir() {
    let buyuk = vec![b' '; (udf::AZAMI_ACILMIS + 1) as usize];
    let bayt = zip_yap(&[("content.xml", &buyuk)]);
    assert!(bayt.len() < 1 << 20, "sıkıştırılmış hâli küçük olmalı");
    assert!(matches!(udf::oku(&bayt), Err(UdfHatasi::CokBuyuk(_))));
}

#[test]
fn cok_dosya_reddedilir() {
    let adlar: Vec<String> = (0..=udf::AZAMI_DOSYA_SAYISI).map(|i| format!("d{i}")).collect();
    let mut d: Vec<(&str, &[u8])> = adlar.iter().map(|a| (a.as_str(), &b"x"[..])).collect();
    d.push(("content.xml", ORNEK.as_bytes()));
    assert!(matches!(udf::oku(&zip_yap(&d)), Err(UdfHatasi::CokBuyuk(_))));
}

#[test]
fn icerik_olmayan_zip() {
    let bayt = zip_yap(&[("baska.xml", b"<a/>")]);
    assert!(!udf::udf_mi(&bayt));
    assert_eq!(udf::oku(&bayt).unwrap_err(), UdfHatasi::IcerikYok);
    assert!(matches!(udf::oku(b"bu bir zip degil"), Err(UdfHatasi::Zip(_))));
}

#[test]
fn ozellik_islemleri() {
    let mut oz = Ozellikler::default();
    oz.ayarla("bold", "true");
    oz.ayarla("size", "12");
    oz.ayarla("bold", "false");
    assert_eq!(oz.0, vec![("bold".into(), "false".into()), ("size".into(), "12".into())]);
    assert!(!oz.evet_mi("bold"));
    assert_eq!(oz.sayi("size"), Some(12.0));
    oz.kaldir("bold");
    assert_eq!(oz.al("bold"), None);
}
