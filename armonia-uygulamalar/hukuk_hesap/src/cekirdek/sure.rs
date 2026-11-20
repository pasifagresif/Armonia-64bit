//! Süre hesabı (H1).
//!
//! Kurallar (plan: docs/claude-notlar/planlar/hukuk-hesaplayici.md; madde metinleri kullanıcıyla DOĞRULANACAK):
//! - HMK 92: gün olarak belirlenen sürede ilk gün (tebliğ günü) sayılmaz. Hafta, ay ve yıl süreleri başlangıç
//!   gününe karşılık gelen günde biter; o ayda karşılık gelen gün yoksa ayın son günü.
//! - HMK 93: son gün resmî tatile rastlarsa süre tatili izleyen ilk iş günü biter. Cumartesi/Pazar da iş günü
//!   sayılmaz (ruyap `icra_durum.rs` ile aynı kabul).
//! - HMK 104: adli tatile tabi işlerde sürenin bitimi adli tatile (20 Temmuz – 31 Ağustos, HMK 102) rastlarsa
//!   süre tatilin bittiği günden itibaren bir hafta uzamış sayılır.
//! - Tebligat K. 7/a: elektronik tebligat, muhatabın adresine ulaştığı günü izleyen 5. günün sonunda yapılmış
//!   sayılır.
//! - Yarım gün tatil (28 Ekim, arife): son güne denk gelirse süre uzar mı → ayar (açık soru, iş listesinde).

use crate::cekirdek::tarih::{self, ay_ekle};
use crate::cekirdek::tatil::{GunDurumu, Takvim};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Birim {
    Gun,
    Hafta,
    Ay,
    Yil,
}

impl Birim {
    pub fn ad(self) -> &'static str {
        match self {
            Birim::Gun => "gün",
            Birim::Hafta => "hafta",
            Birim::Ay => "ay",
            Birim::Yil => "yıl",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tebligat {
    /// Fiziki tebliğ ya da öğrenme/tefhim: süre bu günden başlar
    Fiziki,
    /// Elektronik tebligat: girilen tarih adrese ulaşma günüdür
    Elektronik,
}

/// E-tebligatın yapılmış sayıldığı gün: ulaşma + 5 (Tebligat K. 7/a). ruyap `E_TEBLIG_OKUMA_SURESI_GUN` ile aynı.
pub const E_TEBLIGAT_GUN: i64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ayarlar {
    /// Son gün yarım gün tatile (arife, 28 Ekim) denk gelirse süre ertesi iş gününe uzasın mı.
    /// Varsayılan: uzamaz (daha temkinli); sonuçta ayrıca uyarı verilir.
    pub yarim_gun_uzatir: bool,
}

impl Default for Ayarlar {
    fn default() -> Self {
        Ayarlar { yarim_gun_uzatir: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Girdi {
    pub baslangic: NaiveDate,
    pub miktar: u32,
    pub birim: Birim,
    pub tebligat: Tebligat,
    pub adli_tatil: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sonuc {
    /// Sürenin işlemeye başladığı tebliğ günü (e-tebligatta ulaşma + 5)
    pub teblig_gunu: NaiveDate,
    /// Tatil kaydırmalarından önceki son gün
    pub ham_son_gun: NaiveDate,
    pub son_gun: NaiveDate,
    /// Adım adım döküm (dilekçeye kopyalanabilir)
    pub adimlar: Vec<String>,
    pub uyarilar: Vec<String>,
}

fn adli_tatilde(t: NaiveDate) -> bool {
    let bas = NaiveDate::from_ymd_opt(t.year(), 7, 20).unwrap();
    let son = NaiveDate::from_ymd_opt(t.year(), 8, 31).unwrap();
    t >= bas && t <= son
}

pub fn hesapla(g: &Girdi, takvim: &Takvim, ayar: Ayarlar) -> Result<Sonuc, String> {
    if g.miktar == 0 {
        return Err("Süre en az 1 olmalı.".into());
    }
    let mut adimlar = Vec::new();
    let mut uyarilar = Vec::new();

    let teblig_gunu = match g.tebligat {
        Tebligat::Fiziki => {
            adimlar.push(format!("Tebliğ / öğrenme günü: {}.", tarih::uzun(g.baslangic)));
            g.baslangic
        }
        Tebligat::Elektronik => {
            let t = g.baslangic + Duration::days(E_TEBLIGAT_GUN);
            adimlar.push(format!(
                "Elektronik tebligat {} günü adrese ulaştı; ulaştığı günü izleyen 5. günün sonunda, {} günü tebliğ \
                 edilmiş sayılır (Tebligat K. 7/a).",
                tarih::uzun(g.baslangic),
                tarih::uzun(t)
            ));
            t
        }
    };

    let ham = match g.birim {
        Birim::Gun => teblig_gunu + Duration::days(g.miktar as i64),
        Birim::Hafta => teblig_gunu + Duration::days(7 * g.miktar as i64),
        Birim::Ay => ay_ekle(teblig_gunu, g.miktar),
        Birim::Yil => ay_ekle(teblig_gunu, 12 * g.miktar),
    };
    adimlar.push(match g.birim {
        Birim::Gun => format!(
            "{} günlük süre: tebliğ günü sayılmaz, {} gün sonra {} (HMK 92).",
            g.miktar,
            g.miktar,
            tarih::uzun(ham)
        ),
        b => format!(
            "{} {}lık süre: başlangıç gününe karşılık gelen gün {} (HMK 92).",
            g.miktar,
            b.ad(),
            tarih::uzun(ham)
        ),
    });

    let mut son = ham;
    if g.adli_tatil && adli_tatilde(son) {
        let uzamis = NaiveDate::from_ymd_opt(son.year(), 8, 31).unwrap() + Duration::days(7);
        adimlar.push(format!(
            "Son gün adli tatile (20 Temmuz – 31 Ağustos) rastlıyor; süre tatilin bittiği günden itibaren bir hafta \
             uzar: {} (HMK 104).",
            tarih::uzun(uzamis)
        ));
        son = uzamis;
    } else if !g.adli_tatil && adli_tatilde(son) {
        uyarilar.push("Son gün adli tatile denk geliyor ama bu iş adli tatile tabi değil olarak seçildi; süre uzamadı.".into());
    }

    // Hafta sonu / resmî tatil kaydırması (HMK 93)
    for _ in 0..60 {
        match takvim.durum(son) {
            GunDurumu::IsGunu => break,
            GunDurumu::HaftaSonu => {
                let yeni = son + Duration::days(1);
                adimlar.push(format!("{} hafta sonu; süre ertesi güne geçer.", tarih::uzun(son)));
                son = yeni;
            }
            GunDurumu::Tatil(ad) => {
                adimlar.push(format!("{} resmî tatil ({ad}); süre ertesi güne geçer (HMK 93).", tarih::uzun(son)));
                son += Duration::days(1);
            }
            GunDurumu::YarimGun(ad) => {
                if ayar.yarim_gun_uzatir {
                    adimlar.push(format!(
                        "{} yarım gün tatil ({ad}); ayar gereği süre ilk tam iş gününe geçer.",
                        tarih::uzun(son)
                    ));
                    son += Duration::days(1);
                } else {
                    uyarilar.push(format!(
                        "Son gün yarım gün tatil ({ad}): öğleden sonra mahkemeler kapalı. Bu hesap süreyi uzatmadı; \
                         işlemi öğleden önce yapın (ayar: \"yarım gün süreyi uzatır\")."
                    ));
                    break;
                }
            }
        }
    }
    if son != ham || adimlar.len() > 2 {
        adimlar.push(format!("Sürenin son günü: {}.", tarih::uzun(son)));
    } else {
        adimlar.push(format!("{} iş günü; sürenin son günü budur.", tarih::uzun(son)));
    }
    uyarilar.push(
        "Süre son günün çalışma saati bitiminde sona erer (HMK 92). UYAP üzerinden elektronik işlemlerde son gün \
         24.00'e kadar kabul edilir (DOĞRULANMADI)."
            .into(),
    );
    Ok(Sonuc { teblig_gunu, ham_son_gun: ham, son_gun: son, adimlar, uyarilar })
}

/// Dökümü dilekçeye yapıştırılacak düz metin olarak verir.
pub fn metin(g: &Girdi, s: &Sonuc) -> String {
    let mut m = format!(
        "SÜRE HESABI\nSüre: {} {}{}\n",
        g.miktar,
        g.birim.ad(),
        if g.adli_tatil { " (adli tatile tabi)" } else { "" }
    );
    for (i, a) in s.adimlar.iter().enumerate() {
        m += &format!("{}. {}\n", i + 1, a);
    }
    m += &format!("SONUÇ: Sürenin son günü {}.\n", tarih::uzun(s.son_gun));
    m
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::cekirdek::tatil::{Kaynak, Tatil};

    fn t(y: i32, a: u32, g: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, a, g).unwrap()
    }

    fn girdi(b: NaiveDate, n: u32, birim: Birim) -> Girdi {
        Girdi { baslangic: b, miktar: n, birim, tebligat: Tebligat::Fiziki, adli_tatil: false }
    }

    #[test]
    fn gun_suresi_ilk_gun_sayilmaz() {
        // 19.11.2026 Perşembe + 7 gün = 26.11.2026 Perşembe
        let s = hesapla(&girdi(t(2026, 11, 19), 7, Birim::Gun), &Takvim::default(), Ayarlar::default()).unwrap();
        assert_eq!(s.son_gun, t(2026, 11, 26));
    }

    #[test]
    fn hafta_sonuna_rastlarsa_pazartesi() {
        // 14.11.2026 Cumartesi tebliğ + 7 gün = 21.11 Cumartesi -> 23.11 Pazartesi
        let s = hesapla(&girdi(t(2026, 11, 14), 7, Birim::Gun), &Takvim::default(), Ayarlar::default()).unwrap();
        assert_eq!(s.ham_son_gun, t(2026, 11, 21));
        assert_eq!(s.son_gun, t(2026, 11, 23));
    }

    #[test]
    fn hafta_ve_ay() {
        let k = Takvim::default();
        // 2 hafta: 05.11.2026 Perşembe -> 19.11.2026 Perşembe
        assert_eq!(hesapla(&girdi(t(2026, 11, 5), 2, Birim::Hafta), &k, Ayarlar::default()).unwrap().son_gun, t(2026, 11, 19));
        // 1 ay: 31.01.2026 -> 28.02.2026 Cumartesi -> 02.03.2026 Pazartesi
        let s = hesapla(&girdi(t(2026, 1, 31), 1, Birim::Ay), &k, Ayarlar::default()).unwrap();
        assert_eq!(s.ham_son_gun, t(2026, 2, 28));
        assert_eq!(s.son_gun, t(2026, 3, 2));
    }

    #[test]
    fn resmi_tatil_zinciri() {
        // 22.10.2026 Perşembe + 7 = 29.10 Perşembe (Cumhuriyet Bayramı) -> 30.10 Cuma
        let s = hesapla(&girdi(t(2026, 10, 22), 7, Birim::Gun), &Takvim::default(), Ayarlar::default()).unwrap();
        assert_eq!(s.son_gun, t(2026, 10, 30));
        // Kullanıcının eklediği bayram + hafta sonu zinciri
        let mut k = Takvim::default();
        for g in 26..=29 {
            k.ekle(Tatil { tarih: t(2026, 5, g), ad: "Kurban Bayramı".into(), yarim: false, kaynak: Kaynak::Kullanici });
        }
        // 19.05.2026 Salı (19 Mayıs tatil ama başlangıç) + 7 = 26.05 Salı (bayram) ... 29.05 Cuma bayram, 30-31 hafta sonu
        let s = hesapla(&girdi(t(2026, 5, 19), 7, Birim::Gun), &k, Ayarlar::default()).unwrap();
        assert_eq!(s.son_gun, t(2026, 6, 1));
    }

    #[test]
    fn yarim_gun_ayari() {
        // 21.10.2026 Çarşamba + 7 = 28.10 Çarşamba (öğleden sonra tatil)
        let g = girdi(t(2026, 10, 21), 7, Birim::Gun);
        let k = Takvim::default();
        let s = hesapla(&g, &k, Ayarlar { yarim_gun_uzatir: false }).unwrap();
        assert_eq!(s.son_gun, t(2026, 10, 28));
        assert!(s.uyarilar.iter().any(|u| u.contains("yarım gün")));
        // Uzatırsa: 29.10 tatil -> 30.10 Cuma
        let s = hesapla(&g, &k, Ayarlar { yarim_gun_uzatir: true }).unwrap();
        assert_eq!(s.son_gun, t(2026, 10, 30));
    }

    #[test]
    fn adli_tatil_uzamasi() {
        // 2 hafta, 01.08.2026 tebliğ -> 15.08 (adli tatilde) -> 31.08 + 7 = 07.09.2026 Pazartesi
        let mut g = girdi(t(2026, 8, 1), 2, Birim::Hafta);
        g.adli_tatil = true;
        let s = hesapla(&g, &Takvim::default(), Ayarlar::default()).unwrap();
        assert_eq!(s.son_gun, t(2026, 9, 7));
        // Tabi değilse: 15.08 Cumartesi -> 17.08 Pazartesi
        g.adli_tatil = false;
        let s = hesapla(&g, &Takvim::default(), Ayarlar::default()).unwrap();
        assert_eq!(s.son_gun, t(2026, 8, 17));
        // Uzamış gün hafta sonuna denk gelirse kayar: 2027'de 07.09 Salı (kaymaz); 2025'te 07.09 Pazar -> 08.09
        let mut g = girdi(t(2025, 8, 1), 2, Birim::Hafta);
        g.adli_tatil = true;
        assert_eq!(hesapla(&g, &Takvim::default(), Ayarlar::default()).unwrap().son_gun, t(2025, 9, 8));
    }

    #[test]
    fn e_tebligat_bes_gun() {
        // 02.11.2026 Pazartesi ulaştı -> 07.11 tebliğ sayılır -> + 7 = 14.11 Cumartesi -> 16.11 Pazartesi
        let mut g = girdi(t(2026, 11, 2), 7, Birim::Gun);
        g.tebligat = Tebligat::Elektronik;
        let s = hesapla(&g, &Takvim::default(), Ayarlar::default()).unwrap();
        assert_eq!(s.teblig_gunu, t(2026, 11, 7));
        assert_eq!(s.son_gun, t(2026, 11, 16));
        assert!(metin(&g, &s).contains("SONUÇ: Sürenin son günü 16 Kasım 2026 Pazartesi."));
    }

    #[test]
    fn sifir_sure_hata() {
        assert!(hesapla(&girdi(t(2026, 1, 1), 0, Birim::Gun), &Takvim::default(), Ayarlar::default()).is_err());
    }
}
