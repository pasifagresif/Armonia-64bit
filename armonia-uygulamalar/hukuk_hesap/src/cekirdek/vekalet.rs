//! Vekalet ücreti (AAÜT) hesabı (H3). Claude, 2026-11-19. Kurallar ve tutarlar kullanıcıyla DOĞRULANACAK.
//!
//! Tarife maddeleri (AAÜT 2026–2027, RG 03.11.2026/33419):
//! - m.13/1: para ile ölçülebilen işlerde ücret Üçüncü Kısma (nispi dilimler) göre, maktu ücretin altında
//!   kalmamak üzere. m.13/2: hükmedilen ücret kabul ya da reddedilen miktarı geçemez.
//! - m.13/3: maddi tazminat davasının kısmen reddinde davalı vekili lehine ücret, davacı lehine ücreti geçemez.
//! - m.6: uyuşmazlık ön inceleme tutanağı imzalanmadan giderilirse ücretin yarısı.
//! - m.11/1: takip tutarı 78.125 TL'ye kadar icra takiplerinde icra dairesi maktu ücreti (takipte kesinleşen
//!   miktarı geçemez); üstünde nispi. m.11/4: ödeme süresinde ödenirse 3/4.
//! - m.16/2: arabuluculukta anlaşma → nispi ücretin 1/4 fazlası (62.500 TL'ye kadar: anlaşmazlık maktu
//!   ücretinin 1/4 fazlası), asıl alacağı geçemez; anlaşmazlık → maktu (asıl alacağı geçemez).

use crate::cekirdek::para;
use chrono::NaiveDate;
use serde::Deserialize;

const GOMULU: &str = include_str!("../../veri/aaut.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Mahkeme {
    pub ad: String,
    /// TL
    pub tutar: i64,
    pub yer: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Tarife {
    pub ad: String,
    pub yururluk: NaiveDate,
    pub kaynak: String,
    pub dogrulandi: bool,
    /// (dilim tutarı TL, yüzde)
    pub nispi: Vec<(i64, f64)>,
    /// Son dilimden yukarısı için yüzde
    pub ust_oran: f64,
    pub icra_maktu_esigi: i64,
    pub arabuluculuk_maktu_esigi: i64,
    pub icra_dairesi_maktu: i64,
    pub arabuluculuk_anlasmazlik: i64,
    pub mahkemeler: Vec<Mahkeme>,
    /// (bölüm, iş, tutar TL)
    pub maktu: Vec<(String, String, i64)>,
}

#[derive(Deserialize)]
struct Dosya {
    tarifeler: Vec<Tarife>,
}

pub fn tarifeler() -> Vec<Tarife> {
    let mut v = serde_json::from_str::<Dosya>(GOMULU).expect("veri/aaut.json bozuk").tarifeler;
    v.sort_by_key(|t| t.yururluk);
    v
}

/// Verilen tarihte yürürlükte olan tarife (karar / takip / işlem tarihi).
pub fn tarife_gunu(tarifeler: &[Tarife], t: NaiveDate) -> Option<&Tarife> {
    tarifeler.iter().rev().find(|x| x.yururluk <= t)
}

#[derive(Clone, Debug, PartialEq)]
pub struct DilimSatiri {
    /// Dilime giren tutar (kuruş)
    pub tutar: i64,
    pub yuzde: f64,
    pub ucret: i64,
}

/// Üçüncü Kısım: dilim dilim nispi ücret (kuruş). Yuvarlama: her dilim kuruşa (yarım yukarı).
pub fn nispi(t: &Tarife, deger: i64) -> (i64, Vec<DilimSatiri>) {
    let mut kalan = deger.max(0);
    let mut satirlar = Vec::new();
    let oran_kurus = |tutar: i64, yuzde: f64| -> i64 {
        let o = para::oran_birim(yuzde) as i128; // %1 = 10_000
        ((tutar as i128 * o * 2 + 1_000_000) / 2_000_000) as i64
    };
    for &(dilim, yuzde) in &t.nispi {
        if kalan == 0 {
            break;
        }
        let giren = kalan.min(dilim * 100);
        satirlar.push(DilimSatiri { tutar: giren, yuzde, ucret: oran_kurus(giren, yuzde) });
        kalan -= giren;
    }
    if kalan > 0 {
        satirlar.push(DilimSatiri { tutar: kalan, yuzde: t.ust_oran, ucret: oran_kurus(kalan, t.ust_oran) });
    }
    (satirlar.iter().map(|s| s.ucret).sum(), satirlar)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Is {
    /// Para ile ölçülebilen dava: kabul ve red edilen miktarlar ayrı (kuruş). `mahkeme`: tarifedeki sıra.
    Dava { kabul: i64, red: i64, mahkeme: usize, maddi_tazminat: bool, on_inceleme_oncesi: bool },
    /// İcra takibi (kuruş)
    Icra { tutar: i64, odeme_suresinde: bool },
    /// Arabuluculuk (kuruş)
    Arabuluculuk { tutar: i64, anlasma: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kalem {
    /// "Davacı vekili lehine" gibi
    pub baslik: String,
    pub esas: i64,
    pub dilimler: Vec<DilimSatiri>,
    pub adimlar: Vec<String>,
    pub ucret: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sonuc {
    pub kalemler: Vec<Kalem>,
    pub uyarilar: Vec<String>,
}

fn tl(x: i64) -> i64 {
    x * 100
}

/// Davada bir taraf lehine ücret: nispi, maktudan az değil, miktarı geçmez.
fn dava_kalemi(t: &Tarife, baslik: &str, miktar: i64, maktu: &Mahkeme, yari: bool) -> Kalem {
    let (n, dilimler) = nispi(t, miktar);
    let mut adimlar = vec![format!("Nispi ücret (Üçüncü Kısım): {}", para::yaz(n))];
    let mut u = n;
    if u < tl(maktu.tutar) {
        u = tl(maktu.tutar);
        adimlar.push(format!("Maktu ücretin altında kalamaz (m.13/1; {} {}): {}", maktu.ad, maktu.yer, para::yaz(u)));
    }
    if u > miktar {
        u = miktar;
        adimlar.push(format!("Kabul / ret edilen miktarı geçemez (m.13/2): {}", para::yaz(u)));
    }
    if yari {
        u = (u + 1) / 2;
        adimlar.push(format!("Ön inceleme tutanağından önce sona erdi, yarısı (m.6): {}", para::yaz(u)));
    }
    Kalem { baslik: baslik.into(), esas: miktar, dilimler, adimlar, ucret: u }
}

pub fn hesapla(t: &Tarife, is: &Is) -> Result<Sonuc, String> {
    let mut uyarilar = Vec::new();
    if !t.dogrulandi {
        uyarilar.push(format!("{} tutarları henüz kullanıcı tarafından doğrulanmadı ({}).", t.ad, t.kaynak));
    }
    let kalemler = match *is {
        Is::Dava { kabul, red, mahkeme, maddi_tazminat, on_inceleme_oncesi } => {
            if kabul < 0 || red < 0 || kabul + red == 0 {
                return Err("Kabul ve/veya ret edilen miktarı girin.".into());
            }
            let m = t.mahkemeler.get(mahkeme).ok_or("Mahkeme seçin.")?;
            let mut v = Vec::new();
            if kabul > 0 {
                v.push(dava_kalemi(t, "Davacı vekili lehine (kabul edilen kısım)", kabul, m, on_inceleme_oncesi));
            }
            if red > 0 {
                let mut k = dava_kalemi(t, "Davalı vekili lehine (reddedilen kısım)", red, m, on_inceleme_oncesi);
                if maddi_tazminat && kabul > 0 {
                    let ust = v[0].ucret;
                    if k.ucret > ust {
                        k.ucret = ust;
                        k.adimlar.push(format!("Maddi tazminatın kısmen reddi: davacı lehine ücreti geçemez (m.13/3): {}", para::yaz(ust)));
                    }
                }
                if kabul == 0 && maddi_tazminat {
                    uyarilar.push("Tazminat davasının tamamen reddinde ücret maktu belirlenir (m.13/4); maktu tutar için tabloya bakın.".into());
                }
                v.push(k);
            }
            uyarilar.push("Mahkeme karşı tarafa yükletilecek ücreti tarifenin altında ve üç katından fazla belirleyemez (m.3/1).".into());
            v
        }
        Is::Icra { tutar, odeme_suresinde } => {
            if tutar <= 0 {
                return Err("Takip tutarını girin.".into());
            }
            let mut adimlar = Vec::new();
            let (mut u, dilimler) = if tutar <= tl(t.icra_maktu_esigi) {
                let u = tl(t.icra_dairesi_maktu).min(tutar);
                adimlar.push(format!(
                    "Takip tutarı {} ve altında: icra dairesi maktu ücreti {} (m.11/1){}",
                    para::yaz(tl(t.icra_maktu_esigi)),
                    para::yaz(tl(t.icra_dairesi_maktu)),
                    if u < tl(t.icra_dairesi_maktu) { "; takip tutarını geçemez" } else { "" }
                ));
                (u, Vec::new())
            } else {
                let (n, d) = nispi(t, tutar);
                adimlar.push(format!("Nispi ücret (Üçüncü Kısım): {}", para::yaz(n)));
                let mut u = n;
                if u < tl(t.icra_dairesi_maktu) {
                    u = tl(t.icra_dairesi_maktu);
                    adimlar.push(format!("Maktu ücretin altında kalamaz (m.13/1): {}", para::yaz(u)));
                }
                (u, d)
            };
            if odeme_suresinde {
                u = (u * 3 + 2) / 4;
                adimlar.push(format!("Borçlu ödeme süresinde ödedi, dörtte üçü (m.11/4): {}", para::yaz(u)));
            }
            vec![Kalem { baslik: "Alacaklı vekili lehine".into(), esas: tutar, dilimler, adimlar, ucret: u }]
        }
        Is::Arabuluculuk { tutar, anlasma } => {
            if tutar <= 0 {
                return Err("Uyuşmazlık tutarını girin.".into());
            }
            let mut adimlar = Vec::new();
            let mut dilimler = Vec::new();
            let mut u = if !anlasma {
                adimlar.push(format!("Anlaşmazlıkla sonuçlandı: maktu ücret {} (m.16/2-c)", para::yaz(tl(t.arabuluculuk_anlasmazlik))));
                tl(t.arabuluculuk_anlasmazlik)
            } else if tutar <= tl(t.arabuluculuk_maktu_esigi) {
                let u = tl(t.arabuluculuk_anlasmazlik) * 5 / 4;
                adimlar.push(format!(
                    "{} ve altında: maktu ücretin dörtte bir fazlası {} (m.16/2-a)",
                    para::yaz(tl(t.arabuluculuk_maktu_esigi)),
                    para::yaz(u)
                ));
                u
            } else {
                let (n, d) = nispi(t, tutar);
                dilimler = d;
                let u = (n * 5 + 2) / 4;
                adimlar.push(format!("Nispi ücret {} + dörtte biri = {} (m.16/2-a)", para::yaz(n), para::yaz(u)));
                u
            };
            if u > tutar {
                u = tutar;
                adimlar.push(format!("Asıl alacağı geçemez (m.16/2): {}", para::yaz(u)));
            }
            uyarilar.push("Arabuluculuk ücreti akdi ücretin asgari değeridir (m.16/1).".into());
            vec![Kalem { baslik: "Taraf vekili".into(), esas: tutar, dilimler, adimlar, ucret: u }]
        }
    };
    Ok(Sonuc { kalemler, uyarilar })
}

/// Dilekçeye yapıştırılacak düz metin döküm.
pub fn metin(t: &Tarife, s: &Sonuc) -> String {
    let mut m = format!("VEKALET ÜCRETİ — {} ({})\n\n", t.ad, t.kaynak);
    for k in &s.kalemler {
        m.push_str(&format!("{} — esas: {}\n", k.baslik, para::yaz(k.esas)));
        for d in &k.dilimler {
            m.push_str(&format!("  {:>18} × %{:<5} = {:>16}\n", para::yaz(d.tutar), para::oran_yaz(para::oran_birim(d.yuzde)), para::yaz(d.ucret)));
        }
        for a in &k.adimlar {
            m.push_str(&format!("  {a}\n"));
        }
        m.push_str(&format!("  Ücret: {}\n\n", para::yaz(k.ucret)));
    }
    for u in &s.uyarilar {
        m.push_str(&format!("Not: {u}\n"));
    }
    m
}

#[cfg(test)]
mod testler {
    use super::*;

    fn t2026() -> Tarife {
        let v = tarifeler();
        tarife_gunu(&v, NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()).unwrap().clone()
    }

    fn asliye(t: &Tarife) -> usize {
        t.mahkemeler.iter().position(|m| m.ad.starts_with("Asliye")).unwrap()
    }

    #[test]
    fn tarife_okunur() {
        let t = t2026();
        assert_eq!(t.nispi.iter().map(|d| d.0).sum::<i64>(), 24_800_000);
        assert!(tarife_gunu(&tarifeler(), NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()).is_none());
    }

    #[test]
    fn nispi_dilimler() {
        let t = t2026();
        // 1.000.000: 800.000 × %16 = 128.000 + 200.000 × %15 = 30.000
        assert_eq!(nispi(&t, tl(1_000_000)).0, tl(158_000));
        // 30.000.000: tüm dilimler + 5.200.000 × %1
        let tum: i64 = 128_000 + 120_000 + 224_000 + 208_000 + 264_000 + 256_000 + 200_000 + 144_000 + 112_000;
        assert_eq!(nispi(&t, tl(30_000_000)).0, tl(tum + 52_000));
        assert_eq!(nispi(&t, 0).0, 0);
    }

    #[test]
    fn dava() {
        let t = t2026();
        let m = asliye(&t);
        // 100.000 kabul: nispi 16.000 < maktu 62.000 → 62.000, ama 100.000'i geçmez → 62.000
        let s = hesapla(&t, &Is::Dava { kabul: tl(100_000), red: 0, mahkeme: m, maddi_tazminat: false, on_inceleme_oncesi: false }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(62_000));
        // 40.000 kabul: maktu 62.000 > 40.000 → 40.000 (m.13/2)
        let s = hesapla(&t, &Is::Dava { kabul: tl(40_000), red: 0, mahkeme: m, maddi_tazminat: false, on_inceleme_oncesi: false }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(40_000));
        // Kısmi: 2.000.000 kabul, 3.000.000 red, maddi tazminat → davalı ücreti davacınınkini geçemez
        let s = hesapla(&t, &Is::Dava { kabul: tl(2_000_000), red: tl(3_000_000), mahkeme: m, maddi_tazminat: true, on_inceleme_oncesi: false }).unwrap();
        let davaci = 128_000 + 120_000 + 400_000 * 14 / 100;
        assert_eq!(s.kalemler[0].ucret, tl(davaci));
        assert_eq!(s.kalemler[1].ucret, tl(davaci));
        // Ön inceleme öncesi: yarısı
        let s = hesapla(&t, &Is::Dava { kabul: tl(1_000_000), red: 0, mahkeme: m, maddi_tazminat: false, on_inceleme_oncesi: true }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(79_000));
    }

    #[test]
    fn icra() {
        let t = t2026();
        let s = hesapla(&t, &Is::Icra { tutar: tl(50_000), odeme_suresinde: false }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(12_500));
        let s = hesapla(&t, &Is::Icra { tutar: tl(10_000), odeme_suresinde: false }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(10_000));
        // 78.125 üstü: nispi 80.000 × %16 = 12.800
        let s = hesapla(&t, &Is::Icra { tutar: tl(80_000), odeme_suresinde: false }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(12_800));
        let s = hesapla(&t, &Is::Icra { tutar: tl(1_000_000), odeme_suresinde: true }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(118_500));
    }

    #[test]
    fn arabuluculuk() {
        let t = t2026();
        let s = hesapla(&t, &Is::Arabuluculuk { tutar: tl(50_000), anlasma: true }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(12_500));
        let s = hesapla(&t, &Is::Arabuluculuk { tutar: tl(1_000_000), anlasma: true }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(197_500));
        let s = hesapla(&t, &Is::Arabuluculuk { tutar: tl(8_000), anlasma: false }).unwrap();
        assert_eq!(s.kalemler[0].ucret, tl(8_000));
    }
}
