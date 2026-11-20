//! Türkçe tarih yazımı ve küçük tarih yardımcıları.

use chrono::{Datelike, Months, NaiveDate, Weekday};

pub const AYLAR: [&str; 12] =
    ["Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"];

pub fn gun_adi(g: Weekday) -> &'static str {
    match g {
        Weekday::Mon => "Pazartesi",
        Weekday::Tue => "Salı",
        Weekday::Wed => "Çarşamba",
        Weekday::Thu => "Perşembe",
        Weekday::Fri => "Cuma",
        Weekday::Sat => "Cumartesi",
        Weekday::Sun => "Pazar",
    }
}

/// "7 Eylül 2026 Pazartesi"
pub fn uzun(t: NaiveDate) -> String {
    format!("{} {} {} {}", t.day(), AYLAR[t.month0() as usize], t.year(), gun_adi(t.weekday()))
}

/// "07.09.2026"
pub fn kisa(t: NaiveDate) -> String {
    t.format("%d.%m.%Y").to_string()
}

/// "07.09.2026" ya da "7.9.2026" biçimini çözer.
pub fn coz(s: &str) -> Option<NaiveDate> {
    let mut p = s.trim().split(['.', '/', '-']);
    let g: u32 = p.next()?.trim().parse().ok()?;
    let a: u32 = p.next()?.trim().parse().ok()?;
    let y: i32 = p.next()?.trim().parse().ok()?;
    if p.next().is_some() {
        return None;
    }
    NaiveDate::from_ymd_opt(y, a, g)
}

/// Ay ekler; karşılık gelen gün yoksa ayın son günü (31 Ocak + 1 ay = 28/29 Şubat). HMK 92/2.
pub fn ay_ekle(t: NaiveDate, ay: u32) -> NaiveDate {
    t.checked_add_months(Months::new(ay)).expect("tarih aralık dışında")
}

#[cfg(test)]
mod testler {
    use super::*;

    fn t(y: i32, a: u32, g: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, a, g).unwrap()
    }

    #[test]
    fn yazim() {
        assert_eq!(uzun(t(2026, 9, 7)), "7 Eylül 2026 Pazartesi");
        assert_eq!(kisa(t(2026, 9, 7)), "07.09.2026");
    }

    #[test]
    fn cozum() {
        assert_eq!(coz("07.09.2026"), Some(t(2026, 9, 7)));
        assert_eq!(coz(" 7/9/2026 "), Some(t(2026, 9, 7)));
        assert_eq!(coz("31.02.2026"), None);
        assert_eq!(coz("7.9"), None);
    }

    #[test]
    fn ay_sonu() {
        assert_eq!(ay_ekle(t(2026, 1, 31), 1), t(2026, 2, 28));
        assert_eq!(ay_ekle(t(2028, 1, 31), 1), t(2028, 2, 29));
        assert_eq!(ay_ekle(t(2026, 3, 15), 1), t(2026, 4, 15));
    }
}
