//! Resmî tatiller ve iş günü.
//!
//! - Kanunla sabit tatiller (2429 s. Ulusal Bayram ve Genel Tatiller Hakkında Kanun) koddan hesaplanır.
//! - Dini bayramlar her yıl değişir: veri çekme modülü yazılana dek (kullanıcı kararı 19.11) kullanıcı ekler.
//! - Cumartesi ve Pazar iş günü sayılmaz (ruyap `icra_durum.rs` ile aynı kabul).
//! - Yarım gün tatiller (28 Ekim öğleden sonra, bayram arifeleri) ayrı işaretlenir; sürenin son günü yarım güne
//!   denk gelirse ne olacağı ayardır (bkz. `sure::Ayarlar::yarim_gun_uzatir`).

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kaynak {
    /// 2429 sayılı Kanun (koddan)
    Kanun,
    /// Kullanıcının eklediği (dini bayram, arife vb.)
    Kullanici,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tatil {
    pub tarih: NaiveDate,
    pub ad: String,
    /// Yalnız öğleden sonra tatil (28 Ekim, arife)
    pub yarim: bool,
    pub kaynak: Kaynak,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GunDurumu {
    IsGunu,
    HaftaSonu,
    Tatil(String),
    YarimGun(String),
}

/// Kanunla sabit tatiller (2429 s.K. m.1–2). Yürürlük: 1 Mayıs 2009'dan (5892 s.K.), 15 Temmuz 2017'den
/// (6752 s.K., 2016 sonu yürürlük; ilk uygulama 2017).
pub fn kanuni_tatiller(yil: i32) -> Vec<Tatil> {
    let mut v = Vec::new();
    let mut ekle = |ay: u32, gun: u32, ad: &str, yarim: bool| {
        v.push(Tatil {
            tarih: NaiveDate::from_ymd_opt(yil, ay, gun).expect("geçerli tarih"),
            ad: ad.to_string(),
            yarim,
            kaynak: Kaynak::Kanun,
        })
    };
    ekle(1, 1, "Yılbaşı", false);
    ekle(4, 23, "Ulusal Egemenlik ve Çocuk Bayramı", false);
    if yil >= 2009 {
        ekle(5, 1, "Emek ve Dayanışma Günü", false);
    }
    ekle(5, 19, "Atatürk'ü Anma, Gençlik ve Spor Bayramı", false);
    if yil >= 2017 {
        ekle(7, 15, "Demokrasi ve Millî Birlik Günü", false);
    }
    ekle(8, 30, "Zafer Bayramı", false);
    ekle(10, 28, "Cumhuriyet Bayramı arifesi (öğleden sonra)", true);
    ekle(10, 29, "Cumhuriyet Bayramı", false);
    v
}

/// Tatil takvimi: kanuni tatiller + kullanıcı eklemeleri.
#[derive(Clone, Debug, Default)]
pub struct Takvim {
    kullanici: BTreeMap<NaiveDate, Tatil>,
}

impl Takvim {
    pub fn yeni(kullanici: impl IntoIterator<Item = Tatil>) -> Self {
        let mut k = Takvim::default();
        for t in kullanici {
            k.ekle(t);
        }
        k
    }

    pub fn ekle(&mut self, mut t: Tatil) {
        t.kaynak = Kaynak::Kullanici;
        self.kullanici.insert(t.tarih, t);
    }

    pub fn sil(&mut self, tarih: NaiveDate) {
        self.kullanici.remove(&tarih);
    }

    pub fn kullanici_tatilleri(&self) -> impl Iterator<Item = &Tatil> {
        self.kullanici.values()
    }

    /// O günün tatil kaydı (kullanıcı kaydı kanuni olanın önüne geçmez; tam gün yarım günden önce gelir).
    pub fn tatil(&self, t: NaiveDate) -> Option<Tatil> {
        let kanuni = kanuni_tatiller(t.year()).into_iter().find(|x| x.tarih == t);
        match (kanuni, self.kullanici.get(&t)) {
            (Some(k), Some(u)) if k.yarim && !u.yarim => Some(u.clone()),
            (Some(k), _) => Some(k),
            (None, u) => u.cloned(),
        }
    }

    pub fn yil_tatilleri(&self, yil: i32) -> Vec<Tatil> {
        let mut v: Vec<Tatil> = kanuni_tatiller(yil);
        for u in self.kullanici.values().filter(|u| u.tarih.year() == yil) {
            if let Some(k) = v.iter_mut().find(|k| k.tarih == u.tarih) {
                if k.yarim && !u.yarim {
                    *k = u.clone();
                }
            } else {
                v.push(u.clone());
            }
        }
        v.sort_by_key(|t| t.tarih);
        v
    }

    pub fn durum(&self, t: NaiveDate) -> GunDurumu {
        if let Some(x) = self.tatil(t) {
            if !x.yarim {
                return GunDurumu::Tatil(x.ad);
            }
            if matches!(t.weekday(), Weekday::Sat | Weekday::Sun) {
                return GunDurumu::HaftaSonu;
            }
            return GunDurumu::YarimGun(x.ad);
        }
        if matches!(t.weekday(), Weekday::Sat | Weekday::Sun) {
            GunDurumu::HaftaSonu
        } else {
            GunDurumu::IsGunu
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn t(y: i32, a: u32, g: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, a, g).unwrap()
    }

    #[test]
    fn kanuni() {
        let k = Takvim::default();
        assert_eq!(k.durum(t(2026, 10, 29)), GunDurumu::Tatil("Cumhuriyet Bayramı".into()));
        assert!(matches!(k.durum(t(2026, 10, 28)), GunDurumu::YarimGun(_)));
        assert_eq!(k.durum(t(2026, 11, 21)), GunDurumu::HaftaSonu); // Cumartesi
        assert_eq!(k.durum(t(2026, 11, 19)), GunDurumu::IsGunu);
        assert_eq!(k.durum(t(2008, 5, 1)), GunDurumu::IsGunu); // 1 Mayıs 2009'dan beri tatil
        assert_eq!(k.durum(t(2016, 7, 15)), GunDurumu::IsGunu);
        assert!(matches!(k.durum(t(2017, 7, 17)), GunDurumu::IsGunu));
        assert_eq!(kanuni_tatiller(2026).len(), 8);
    }

    #[test]
    fn kullanici_eklemesi() {
        let mut k = Takvim::default();
        k.ekle(Tatil { tarih: t(2026, 5, 27), ad: "Kurban Bayramı 1. gün".into(), yarim: false, kaynak: Kaynak::Kanun });
        assert_eq!(k.durum(t(2026, 5, 27)), GunDurumu::Tatil("Kurban Bayramı 1. gün".into()));
        // Kaynak her zaman kullanıcıya çevrilir
        assert_eq!(k.kullanici_tatilleri().next().unwrap().kaynak, Kaynak::Kullanici);
        // Kanuni tam gün tatil kullanıcı yarım günüyle ezilmez
        k.ekle(Tatil { tarih: t(2026, 1, 1), ad: "deneme".into(), yarim: true, kaynak: Kaynak::Kullanici });
        assert_eq!(k.durum(t(2026, 1, 1)), GunDurumu::Tatil("Yılbaşı".into()));
        assert_eq!(k.yil_tatilleri(2026).len(), 9);
        k.sil(t(2026, 5, 27));
        assert_eq!(k.durum(t(2026, 5, 27)), GunDurumu::IsGunu);
    }
}
