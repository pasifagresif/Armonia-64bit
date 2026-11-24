//! Zaman: yalnız `YYYY-AA-GGTSS:DD:ssZ` (UTC). Unix saniyesine çevrilir.

pub fn simdi() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn coz(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let n = |a: usize, c: usize| -> Option<i64> {
        let t = &s[a..a + c];
        if !t.bytes().all(|x| x.is_ascii_digit()) {
            return None;
        }
        t.parse().ok()
    };
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 2)?, n(8, 2)?, n(11, 2)?, n(14, 2)?, n(17, 2)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 || y < 1970 {
        return None;
    }
    let t = gunler(y, mo, d) * 86400 + h * 3600 + mi * 60 + se;
    // 31 Şubat gibi tarihler: geri yazınca aynı olmalı
    (yaz(t) == s).then_some(t)
}

pub fn yaz(t: i64) -> String {
    let g = t.div_euclid(86400);
    let s = t.rem_euclid(86400);
    let (y, m, d) = takvim(g);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, (s % 3600) / 60, s % 60)
}

// H. Hinnant, "chrono-Compatible Low-Level Date Algorithms"
fn gunler(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn takvim(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn gidis_donus() {
        assert_eq!(coz("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(coz("2026-11-24T12:00:00Z"), Some(1795521600));
        assert_eq!(yaz(1795521600), "2026-11-24T12:00:00Z");
        assert_eq!(coz("2028-02-29T00:00:00Z").map(yaz).as_deref(), Some("2028-02-29T00:00:00Z"));
        for k in ["2027-02-29T00:00:00Z", "2026-13-01T00:00:00Z", "2026-11-24 12:00:00Z", "2026-11-24T12:00:00+03:00",
                  "2026-11-24T24:00:00Z", "2026-11-24T12:00:00z", "+026-11-24T12:00:00Z"] {
            assert_eq!(coz(k), None, "{k}");
        }
    }
}
