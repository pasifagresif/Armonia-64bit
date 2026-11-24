//! DOCX yazma testleri: yazılan DOCX geçerli XML mi, kendi okuyucumuzla geri okununca içerik ve biçim aynı mı.
//! Claude, 2026-11-23.

use crate::docx;
use crate::docx_testleri::ornek_docx;
use crate::docx_yaz;
use crate::model::*;
use crate::stil::{oranlari_coz, StilCozucu};
use crate::testler::ornek_belge;
use std::io::{Cursor, Read};

fn xml_parcalari_gecerli(bayt: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(Cursor::new(bayt)).unwrap();
    let mut adlar = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let ad = f.name().to_string();
        if ad.ends_with(".xml") || ad.ends_with(".rels") {
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            roxmltree::Document::parse(&s).unwrap_or_else(|e| panic!("{ad} geçersiz XML: {e}"));
        }
        adlar.push(ad);
    }
    adlar
}

fn paragraflar(b: &[Blok], v: &mut Vec<Paragraf>) {
    for x in b {
        match x {
            Blok::Paragraf(p) => v.push(p.clone()),
            Blok::Tablo(t) => t.satirlar.iter().for_each(|r| r.hucreler.iter().for_each(|h| paragraflar(&h.bloklar, v))),
        }
    }
}

/// İki belgenin etkin biçimleri (çözülmüş) aynı mı: metin, hiza, girintiler, liste, kalın/italik/boyut.
fn ayni_gorunur(a: &Belge, b: &Belge) {
    let (ca, cb) = (StilCozucu::yeni(a), StilCozucu::yeni(b));
    let (mut pa, mut pb) = (Vec::new(), Vec::new());
    paragraflar(&a.govde, &mut pa);
    paragraflar(&b.govde, &mut pb);
    assert_eq!(pa.len(), pb.len(), "paragraf sayısı");
    for (i, (x, y)) in pa.iter().zip(&pb).enumerate() {
        // Resim parçasının yer tutucu karakteri metin sayılmaz
        let metin = |p: &Paragraf| {
            p.parcalar.iter().filter(|c| !matches!(c, Parca::Resim { .. })).map(|c| c.metin()).collect::<String>().replace(['\t', '\n', '\r'], "")
        };
        assert_eq!(metin(x), metin(y), "paragraf {i} metni");
        let (sx, sy) = (ca.paragraf(x), cb.paragraf(y));
        assert_eq!(sx.hiza, sy.hiza, "paragraf {i} hiza");
        for (ad, u, v) in [("sol", sx.sol, sy.sol), ("ilk", sx.ilk_satir, sy.ilk_satir), ("üst", sx.ust_bosluk, sy.ust_bosluk), ("satır", sx.satir_araligi, sy.satir_araligi)] {
            assert!((u - v).abs() < 0.06, "paragraf {i} {ad}: {u} / {v}");
        }
        assert_eq!(sx.liste.is_some(), sy.liste.is_some(), "paragraf {i} liste");
        // İlk görünür metin: metin, boşluk ya da alan (UYAP alanları Word'e düz metin olarak gider)
        let ilk = |p: &Paragraf| p.parcalar.iter().find(|c| matches!(c, Parca::Metin { .. } | Parca::Bosluk { .. } | Parca::Alan { .. }) && !c.metin().trim().is_empty()).cloned();
        if let (Some(rx), Some(ry)) = (ilk(x), ilk(y)) {
            let (bx, by) = (ca.parca(&rx, x, crate::stil::Renk::SIYAH), cb.parca(&ry, y, crate::stil::Renk::SIYAH));
            assert_eq!((bx.kalin, bx.egik, bx.alti_cizili, bx.renk), (by.kalin, by.egik, by.alti_cizili, by.renk), "paragraf {i} yazı biçimi");
            assert!((bx.boyut - by.boyut).abs() < 0.26, "paragraf {i} boyut {} / {}", bx.boyut, by.boyut);
            assert_eq!(bx.cizim_ailesi(), by.cizim_ailesi(), "paragraf {i} yazı tipi");
        }
    }
}

#[test]
fn udf_belgesi_docx_olarak_yazilir() {
    let b = ornek_belge();
    let bayt = docx_yaz::yaz(&b).unwrap();
    let adlar = xml_parcalari_gecerli(&bayt);
    for gerekli in ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/styles.xml", "word/numbering.xml", "word/header1.xml", "word/footer1.xml", "word/media/resim1.png"] {
        assert!(adlar.iter().any(|a| a == gerekli), "{gerekli} yok: {adlar:?}");
    }
    let geri = docx::oku(&bayt).unwrap();
    // Örnekteki tek uyarı kaynağı olabilecek şey yok: uyarısız okunmalı
    assert!(geri.uyarilar.is_empty(), "{:?}", geri.uyarilar);
    ayni_gorunur(&b, &geri.belge);
    // Tablo: 1 satır 2 hücre, oranlar 200:300 korunur
    let Blok::Tablo(t) = geri.belge.govde.iter().find(|x| matches!(x, Blok::Tablo(_))).unwrap() else { unreachable!() };
    let o = oranlari_coz(t.oz.al("columnSpans"));
    assert_eq!(o.len(), 2);
    assert!((o[0] / (o[0] + o[1]) - 0.4).abs() < 0.01, "{o:?}");
    assert_eq!(t.oz.al("border"), Some("borderCell"));
    // Üst bilgi metni
    assert_eq!(geri.belge.ust_bilgi.as_ref().unwrap().bloklar.iter().filter_map(|x| if let Blok::Paragraf(p) = x { Some(p.duz_metin()) } else { None }).collect::<Vec<_>>(), vec!["Üst bilgi"]);
    // Belirlenimci
    assert_eq!(bayt, docx_yaz::yaz(&b).unwrap());
}

#[test]
fn docx_gidis_donus() {
    let ilk = docx::oku(&ornek_docx()).unwrap().belge;
    let bayt = docx_yaz::yaz(&ilk).unwrap();
    xml_parcalari_gecerli(&bayt);
    let ikinci = docx::oku(&bayt).unwrap();
    assert!(ikinci.uyarilar.is_empty(), "{:?}", ikinci.uyarilar);
    ayni_gorunur(&ilk, &ikinci.belge);
    // Sayfa numarası "Sayfa X / Y" → Word alanı → yeniden UDF sayfa numarası
    let a1 = ilk.alt_bilgi.as_ref().unwrap();
    let a2 = ikinci.belge.alt_bilgi.as_ref().unwrap();
    assert_eq!(a2.oz.al("pageNumber-spec"), a1.oz.al("pageNumber-spec"));
    assert_eq!(a2.oz.al("pageNumber-foreStr"), Some("Sayfa"));
    // Birleşik hücreli satır ve kenar boşlukları
    let tablo = |b: &Belge| b.govde.iter().find_map(|x| if let Blok::Tablo(t) = x { Some(t.clone()) } else { None }).unwrap();
    assert_eq!(tablo(&ikinci.belge).satirlar[1].hucreler.len(), 1);
    assert_eq!(ikinci.belge.sayfa.sayi("leftMargin"), ilk.sayfa.sayi("leftMargin"));
    // Numaralı liste türü (parantezli) korunur
    let tur = |b: &Belge| b.govde.iter().find_map(|x| if let Blok::Paragraf(p) = x { p.oz.al("NumberType").map(str::to_string) } else { None });
    assert_eq!(tur(&ikinci.belge), tur(&ilk));
}

/// Gerçek UDF'ler → DOCX → geri: ARMONIA_YAZI_ORNEKLER=<klasör> cargo test -- --ignored udf_docx
#[test]
#[ignore]
fn gercek_udf_docx() {
    let klasor = std::env::var("ARMONIA_YAZI_ORNEKLER").expect("ARMONIA_YAZI_ORNEKLER verilmedi");
    let mut sayi = 0;
    for g in std::fs::read_dir(klasor).unwrap().flatten() {
        let yol = g.path();
        if yol.extension().and_then(|e| e.to_str()) != Some("udf") {
            continue;
        }
        let b = crate::udf::oku(&std::fs::read(&yol).unwrap()).unwrap().belge;
        let bayt = docx_yaz::yaz(&b).unwrap();
        xml_parcalari_gecerli(&bayt);
        let geri = docx::oku(&bayt).unwrap();
        assert!(geri.uyarilar.is_empty(), "{}: {:?}", yol.display(), geri.uyarilar);
        ayni_gorunur(&b, &geri.belge);
        sayi += 1;
    }
    assert!(sayi > 0);
}
