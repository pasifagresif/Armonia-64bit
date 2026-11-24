//! Gerçek bir UDF'i inceler: uyarılar, yapı, kullanılan öğe/özellik adları, gidiş-dönüş kayıpsızlığı.
//! Belgenin METNİNİ yazdırmaz (kişisel veri). Kullanım:
//!   cargo run --example udf_incele -- <dosya.udf> [gidis-donus-cikti.udf]
//! Claude, 2026-11-23.

use belge_cekirdek::stil::{Renk, SayfaBicimi, StilCozucu};
use belge_cekirdek::{udf, Blok, Paragraf};
use std::collections::BTreeMap;

fn say(b: &[Blok], s: &mut BTreeMap<String, usize>, oz: &mut BTreeMap<String, usize>, par: &mut Vec<Paragraf>) {
    for x in b {
        match x {
            Blok::Paragraf(p) => {
                *s.entry("paragraph".into()).or_default() += 1;
                for (a, _) in &p.oz.0 {
                    *oz.entry(format!("paragraph@{a}")).or_default() += 1;
                }
                for c in &p.parcalar {
                    let e = match c {
                        belge_cekirdek::Parca::Metin { .. } => "content",
                        belge_cekirdek::Parca::Bosluk { .. } => "space",
                        belge_cekirdek::Parca::Sekme { .. } => "tab",
                        belge_cekirdek::Parca::Alan { .. } => "field",
                        belge_cekirdek::Parca::Resim { .. } => "image",
                    };
                    *s.entry(e.into()).or_default() += 1;
                    for (a, _) in &c.oz().0 {
                        *oz.entry(format!("{e}@{a}")).or_default() += 1;
                    }
                }
                par.push(p.clone());
            }
            Blok::Tablo(t) => {
                *s.entry("table".into()).or_default() += 1;
                for (a, _) in &t.oz.0 {
                    *oz.entry(format!("table@{a}")).or_default() += 1;
                }
                for r in &t.satirlar {
                    *s.entry("row".into()).or_default() += 1;
                    for h in &r.hucreler {
                        *s.entry("cell".into()).or_default() += 1;
                        for (a, _) in &h.oz.0 {
                            *oz.entry(format!("cell@{a}")).or_default() += 1;
                        }
                        say(&h.bloklar, s, oz, par);
                    }
                }
            }
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bayt = std::fs::read(&a[1]).expect("dosya okunamadı");
    println!("boyut: {} bayt, udf_mi: {}", bayt.len(), udf::udf_mi(&bayt));
    let o = udf::oku(&bayt).expect("okunamadı");
    println!("imzalı: {}  uyarılar: {:?}", o.imzali, o.uyarilar);
    let b = &o.belge;
    println!("kök: {:?}", b.kok.0);
    println!("sayfa: {:?}", b.sayfa.0);
    println!("sayfa biçimi: {:?}", SayfaBicimi::coz(&b.sayfa));
    println!("elements: {:?}", b.govde_oz.0);
    println!("ek dosyalar: {:?}", b.ek_dosyalar.iter().map(|(n, v)| (n.as_str(), v.len())).collect::<Vec<_>>());
    println!("stiller:");
    for s in &b.stiller {
        let o: Vec<String> = s.oz.0.iter().filter(|(k, _)| k != "FONT_ATTRIBUTE_KEY").map(|(k, v)| format!("{k}={v}")).collect();
        println!("  {} -> {}", s.ad, o.join(" "));
    }
    for (ad, k) in [("üst bilgi", &b.ust_bilgi), ("alt bilgi", &b.alt_bilgi)] {
        if let Some(k) = k {
            println!("{ad}: oz={:?} blok={}", k.oz.0.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(), k.bloklar.len());
        }
    }
    let (mut s, mut oz, mut par) = (BTreeMap::new(), BTreeMap::new(), Vec::new());
    say(&b.govde, &mut s, &mut oz, &mut par);
    println!("gövde blok: {}  öğeler: {:?}", b.govde.len(), s);
    println!("özellik adları (sayı):");
    for (k, v) in &oz {
        println!("  {k}: {v}");
    }
    // Değer dağılımı (metin değil): hiza, aile, boyut
    let c = StilCozucu::yeni(b);
    let mut hiza = BTreeMap::new();
    let mut yazi = BTreeMap::new();
    for p in &par {
        *hiza.entry(format!("{:?}", c.paragraf(p).hiza)).or_insert(0) += 1;
        for x in &p.parcalar {
            let st = c.parca(x, p, Renk::SIYAH);
            *yazi.entry(format!("{} {} k={} e={} a={} {}", st.aile, st.boyut, st.kalin, st.egik, st.alti_cizili, st.renk.hex())).or_insert(0) += 1;
        }
    }
    println!("hizalar: {hiza:?}");
    println!("etkin yazı stilleri:");
    for (k, v) in &yazi {
        println!("  {k}: {v}");
    }
    println!("metin uzunluğu (karakter): {}", b.duz_metin().chars().count());

    // Gidiş-dönüş
    let yeni = udf::yaz(b).expect("yazılamadı");
    let o2 = udf::oku(&yeni).expect("yeniden okunamadı");
    println!("gidiş-dönüş: model aynı = {}, yeni uyarılar = {:?}", o2.belge == *b, o2.uyarilar);
    if let Some(cikti) = a.get(2) {
        std::fs::write(cikti, &yeni).expect("yazılamadı");
        println!("gidiş-dönüş dosyası: {cikti}");
    }
}
