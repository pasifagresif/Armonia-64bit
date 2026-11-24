//! Stil çözücü testleri. Claude, 2026-11-23.

use crate::model::*;
use crate::stil::*;
use crate::testler::ornek_belge;

fn oz(v: &[(&str, &str)]) -> Ozellikler {
    Ozellikler(v.iter().map(|(a, d)| (a.to_string(), d.to_string())).collect())
}

fn metin(m: &str, o: &[(&str, &str)]) -> Parca {
    Parca::Metin { metin: m.into(), oz: oz(o) }
}

fn paragraf_al(b: &Blok) -> &Paragraf {
    match b {
        Blok::Paragraf(p) => p,
        _ => panic!("paragraf bekleniyordu"),
    }
}

#[test]
fn ornek_paragraf_stilleri() {
    let b = ornek_belge();
    let c = StilCozucu::yeni(&b);
    let p1 = c.paragraf(paragraf_al(&b.govde[0]));
    assert_eq!(p1.hiza, Hiza::Orta);
    assert_eq!(p1.liste, None);
    let p2 = c.paragraf(paragraf_al(&b.govde[1]));
    assert_eq!(p2.hiza, Hiza::IkiYana);
    assert_eq!((p2.sol, p2.ilk_satir, p2.satir_araligi), (10.0, 28.35, 0.5));
    assert_eq!(p2.sekmeler, vec![Sekme { konum: 70.0, hiza: 0 }]);
    let p4 = c.paragraf(paragraf_al(&b.govde[3]));
    assert_eq!(p4.liste, Some(Liste::Numara { liste_id: "1".into(), tur: NumaraTuru::Nokta }));
}

#[test]
fn ornek_parca_stilleri() {
    let b = ornek_belge();
    let c = StilCozucu::yeni(&b);
    let p1 = paragraf_al(&b.govde[0]);
    let s = c.parca(&p1.parcalar[0], p1, Renk::SIYAH);
    assert_eq!((s.aile.as_str(), s.boyut, s.kalin, s.egik), ("Times New Roman", 12.0, true, false));
    let p2 = paragraf_al(&b.govde[1]);
    let s = c.parca(&p2.parcalar[2], p2, Renk::SIYAH);
    assert!(s.egik && !s.kalin);
    assert_eq!(s.renk, Renk { r: 255, g: 0, b: 0 });
    // Renk verilmemiş parça: "default" stilinin (UYAP arayüz) rengi DEĞİL, varsayılan siyah
    let s = c.parca(&p2.parcalar[0], p2, Renk::SIYAH);
    assert_eq!(s.renk, Renk::SIYAH);
    // Üst bilgi: parçanın kendi boyutu 10; satır sonu parçası hvl-default'tan 12
    let ust = paragraf_al(&b.ust_bilgi.as_ref().unwrap().bloklar[0]);
    let mavi = Renk { r: 0, g: 0, b: 255 };
    assert_eq!(c.parca(&ust.parcalar[0], ust, mavi).boyut, 10.0);
    assert_eq!(c.parca(&ust.parcalar[1], ust, mavi).boyut, 12.0);
    assert_eq!(c.parca(&ust.parcalar[1], ust, mavi).renk, mavi);
}

#[test]
fn stil_zinciri_ve_dongu() {
    let b = Belge {
        stiller: vec![
            Stil { ad: "A".into(), oz: oz(&[("name", "A"), ("size", "14"), ("resolver", "B")]) },
            Stil { ad: "B".into(), oz: oz(&[("name", "B"), ("bold", "true"), ("foreground", "-16711936"), ("resolver", "A")]) },
            Stil { ad: "hvl-default".into(), oz: oz(&[("family", "Dialog"), ("size", "11")]) },
        ],
        ..Default::default()
    };
    let c = StilCozucu::yeni(&b);
    let p = Paragraf { oz: Ozellikler::default(), parcalar: vec![metin("x", &[("resolver", "A")]), metin("y", &[])] };
    let s = c.parca(&p.parcalar[0], &p, Renk::SIYAH);
    assert_eq!((s.boyut, s.kalin, s.renk), (14.0, true, Renk { r: 0, g: 255, b: 0 }));
    assert_eq!(s.aile, "Dialog");
    assert_eq!(s.cizim_ailesi(), "Arial");
    let s = c.parca(&p.parcalar[1], &p, Renk::SIYAH);
    assert_eq!((s.boyut, s.kalin), (11.0, false));
}

#[test]
fn bos_ve_bozuk_degerler() {
    let b = Belge::default();
    let c = StilCozucu::yeni(&b);
    let p = Paragraf {
        oz: oz(&[("LineSpacing", "9"), ("LeftIndent", "abc"), ("SpaceAbove", "-4")]),
        parcalar: vec![metin("x", &[("size", "0"), ("family", ""), ("foreground", "")])],
    };
    let s = c.parca(&p.parcalar[0], &p, Renk::SIYAH);
    assert_eq!((s.boyut, s.aile.as_str(), s.renk), (VARSAYILAN_BOYUT, VARSAYILAN_AILE, Renk::SIYAH));
    let ps = c.paragraf(&p);
    assert_eq!((ps.satir_araligi, ps.sol, ps.ust_bosluk), (3.0, 0.0, 0.0));
}

#[test]
fn paragraf_ozelligi_ilk_parcadan() {
    let b = Belge::default();
    let p = Paragraf { oz: Ozellikler::default(), parcalar: vec![metin("x", &[("Alignment", "2")])] };
    assert_eq!(StilCozucu::yeni(&b).paragraf(&p).hiza, Hiza::Sag);
}

#[test]
fn liste_onekleri() {
    let mut sayac = ListeSayaci::default();
    let num = |id: &str, tur| ParagrafStili {
        hiza: Hiza::Sol, sol: 0.0, sag: 0.0, ilk_satir: 0.0, satir_araligi: 0.0, ust_bosluk: 0.0, alt_bosluk: 0.0,
        sekmeler: vec![], liste: Some(Liste::Numara { liste_id: id.into(), tur }),
    };
    assert_eq!(sayac.onek(&num("1", NumaraTuru::Nokta)).as_deref(), Some("1."));
    assert_eq!(sayac.onek(&num("1", NumaraTuru::Nokta)).as_deref(), Some("2."));
    assert_eq!(sayac.onek(&num("2", NumaraTuru::Tire)).as_deref(), Some("1-"));
    assert_eq!(sayac.onek(&num("1", NumaraTuru::Parantez)).as_deref(), Some("3)"));
    let mut madde = num("x", NumaraTuru::Nokta);
    madde.liste = Some(Liste::Madde { ok: true });
    assert_eq!(sayac.onek(&madde).as_deref(), Some("»"));
    madde.liste = None;
    assert_eq!(sayac.onek(&madde), None);
}

#[test]
fn sekmeler() {
    let s = sekmeleri_coz("140:1:0, 70.5:0:0,,bozuk:x");
    assert_eq!(s, vec![Sekme { konum: 0.0, hiza: 0 }, Sekme { konum: 70.5, hiza: 0 }, Sekme { konum: 140.0, hiza: 1 }]);
    let mut ps = ornek_belge_p2();
    assert_eq!(ps.sonraki_sekme(10.0).konum, 70.0);
    assert_eq!(ps.sonraki_sekme(80.0).konum, 85.0);
    ps.sekmeler.clear();
    assert_eq!(ps.sonraki_sekme(80.0).konum, 144.0);
}

fn ornek_belge_p2() -> ParagrafStili {
    let b = ornek_belge();
    StilCozucu::yeni(&b).paragraf(paragraf_al(&b.govde[1]))
}

#[test]
fn renk_donusumleri() {
    let k = Renk::argb("-65536").unwrap();
    assert_eq!(k, Renk { r: 255, g: 0, b: 0 });
    assert_eq!(k.argb_metni(), "-65536");
    assert_eq!(Renk::argb("-16777216"), Some(Renk::SIYAH));
    assert_eq!(Renk { r: 0x12, g: 0xab, b: 0x0f }.hex(), "#12ab0f");
    assert_eq!(Renk::argb("yesil"), None);
    assert_eq!(Renk::argb("99999999999"), None);
}

#[test]
fn sayfa_bicimi() {
    let b = ornek_belge();
    let s = SayfaBicimi::coz(&b.sayfa);
    assert!(!s.yatay);
    assert_eq!((s.sol, s.ust, s.ust_bilgi_payi), (70.86, 56.69, 20.0));
    assert!((s.yazi_genisligi() - (A4_GENISLIK - 2.0 * 70.86)).abs() < 1e-9);
    let y = SayfaBicimi::coz(&oz(&[("paperOrientation", "0")]));
    assert!(y.yatay && y.genislik > y.yukseklik);
    assert_eq!(y.sol, VARSAYILAN_MARJ);
}

#[test]
fn gorunum_agaci() {
    use crate::gorunum::{gorunum, GBlok};
    let b = ornek_belge();
    let g = gorunum(&b);
    assert_eq!(g.govde.len(), b.govde.len());
    assert!(g.ust_bilgi.is_some() && g.alt_bilgi.is_some());
    // Kenar varsayılan rengi kenarın foreground'u (-16777216 = siyah)
    let GBlok::Paragraf { parcalar, .. } = &g.alt_bilgi.as_ref().unwrap()[0] else { panic!() };
    assert_eq!(parcalar[0].renk, Renk::SIYAH);
    let GBlok::Tablo { satirlar, stil, satir_bilgisi } = &g.govde[2] else { panic!("tablo") };
    assert_eq!(stil.kenarlik, Kenarlik::Hucre);
    assert_eq!(satir_bilgisi[0].oranlar, vec![0.4, 0.6]);
    assert_eq!((satirlar.len(), satirlar[0].len()), (1, 2));
    let GBlok::Paragraf { onek, parcalar, .. } = &g.govde[3] else { panic!() };
    assert_eq!(onek.as_deref(), Some("1."));
    assert_eq!(parcalar.len(), 2);
}

#[test]
fn sayfa_numarasi_ve_kenar_araligi() {
    use crate::gorunum::{gorunum, SayfaNumarasi};
    // Gerçek UDF'teki alt bilgi: spec "BSP32_2088" = 2048 (toplam) + 32 (orta) + 8
    let o = oz(&[("pageNumber-spec", "BSP32_2088"), ("pageNumber-color", "-16777216"), ("pageNumber-fontFace", "Arial"),
        ("pageNumber-fontSize", "11"), ("pageNumber-foreStr", ""), ("pageNumber-pageStartNumStr", ""),
        ("pageNumber-seperator", "/"), ("pageNumber-fontBold", "false"), ("pageNumber-fontItalic", "false")]);
    let s = SayfaNumarasi::coz(&o).unwrap();
    assert_eq!((s.hiza, s.toplam_goster, s.stil.boyut, s.bas), (Hiza::Orta, true, 11.0, 1));
    assert_eq!(s.metin(2, 5), "2/5");
    let mut o2 = o.clone();
    o2.ayarla("pageNumber-foreStr", "Sayfa");
    o2.ayarla("pageNumber-spec", "X_64");
    o2.ayarla("pageNumber-pageStartNumStr", "3");
    let s2 = SayfaNumarasi::coz(&o2).unwrap();
    assert_eq!((s2.hiza, s2.metin(1, 9)), (Hiza::Sag, "Sayfa 3".to_string()));
    o2.ayarla("pageNumber-spec", "X_0");
    assert!(SayfaNumarasi::coz(&o2).is_none());

    let b = ornek_belge();
    let g = gorunum(&b);
    let u = g.ust_kenar.unwrap();
    assert!(u.gorunur && u.sayfada(1) && u.sayfada(99));
    assert!(g.sayfa_numarasi.is_none());
}
