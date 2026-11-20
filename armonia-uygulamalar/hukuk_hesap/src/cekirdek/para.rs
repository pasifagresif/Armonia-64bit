//! Para (kuruş cinsinden tamsayı) ve oran (yüzdenin on binde biri: %39,75 → 397_500) yazımı/çözümü.

/// Oran birimi: %1 = 10_000.
pub const ORAN_BIRIM: u64 = 10_000;

/// "125.000,50", "125000", "125000.5", "1.500 TL" → kuruş. Virgül varsa ondalık ayırıcıdır ve noktalar binlik
/// sayılır; virgül yoksa tek nokta + 1–2 hane ondalık sayılır, diğer noktalar binlik.
pub fn coz(s: &str) -> Option<i64> {
    let s: String = s
        .trim()
        .trim_end_matches("TL")
        .trim_end_matches('₺')
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if s.is_empty() {
        return None;
    }
    let (tam, ond) = if let Some((t, o)) = s.rsplit_once(',') {
        (t.replace('.', ""), o.to_string())
    } else if let Some((t, o)) = s.rsplit_once('.') {
        if (1..=2).contains(&o.len()) && !t.contains('.') {
            (t.to_string(), o.to_string())
        } else {
            (s.replace('.', ""), String::new())
        }
    } else {
        (s, String::new())
    };
    if tam.is_empty() || !tam.chars().all(|c| c.is_ascii_digit()) || ond.len() > 2 || !ond.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let t: i64 = tam.parse().ok()?;
    let k: i64 = match ond.len() {
        0 => 0,
        1 => ond.parse::<i64>().ok()? * 10,
        _ => ond.parse().ok()?,
    };
    t.checked_mul(100)?.checked_add(k)
}

/// 12345678 → "123.456,78 TL"
pub fn yaz(kurus: i64) -> String {
    let isaret = if kurus < 0 { "-" } else { "" };
    let k = kurus.unsigned_abs();
    let tam = (k / 100).to_string();
    let mut gruplu = String::new();
    for (i, c) in tam.chars().enumerate() {
        if i > 0 && (tam.len() - i) % 3 == 0 {
            gruplu.push('.');
        }
        gruplu.push(c);
    }
    format!("{isaret}{gruplu},{:02} TL", k % 100)
}

/// Yüzde (ör. 39.75) → oran birimi.
pub fn oran_birim(yuzde: f64) -> u64 {
    (yuzde * ORAN_BIRIM as f64).round() as u64
}

/// "39,75" / "39.75" / "%24" → oran birimi (en çok 4 ondalık).
pub fn oran_coz(s: &str) -> Option<u64> {
    let s = s.trim().trim_start_matches('%').trim().replace(',', ".");
    let (t, o) = s.split_once('.').unwrap_or((&s, ""));
    if t.is_empty() || o.len() > 4 || !t.chars().chain(o.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let tam: u64 = t.parse().ok()?;
    let ond: u64 = if o.is_empty() { 0 } else { format!("{o:0<4}").parse().ok()? };
    tam.checked_mul(ORAN_BIRIM)?.checked_add(ond)
}

/// 397_500 → "39,75"; 240_000 → "24"
pub fn oran_yaz(o: u64) -> String {
    let tam = o / ORAN_BIRIM;
    let ond = o % ORAN_BIRIM;
    if ond == 0 {
        return tam.to_string();
    }
    let s = format!("{ond:04}");
    format!("{tam},{}", s.trim_end_matches('0'))
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn para() {
        assert_eq!(coz("125.000,50"), Some(12_500_050));
        assert_eq!(coz("125000"), Some(12_500_000));
        assert_eq!(coz("1.500 TL"), Some(150_000));
        assert_eq!(coz("1.500.000"), Some(150_000_000));
        assert_eq!(coz("99.5"), Some(9_950));
        assert_eq!(coz("10,5"), Some(1_050));
        assert_eq!(coz("abc"), None);
        assert_eq!(coz("1,234"), None);
        assert_eq!(yaz(12_345_678), "123.456,78 TL");
        assert_eq!(yaz(5), "0,05 TL");
        assert_eq!(yaz(100_000), "1.000,00 TL");
    }

    #[test]
    fn oran() {
        assert_eq!(oran_coz("39,75"), Some(397_500));
        assert_eq!(oran_coz("%24"), Some(240_000));
        assert_eq!(oran_coz("x"), None);
        assert_eq!(oran_yaz(397_500), "39,75");
        assert_eq!(oran_yaz(240_000), "24");
        assert_eq!(oran_yaz(346_000), "34,6");
        assert_eq!(oran_birim(17.75), 177_500);
    }
}
