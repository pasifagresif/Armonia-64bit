//! Sürüm: yalnız `A.B.C` (sayısal, başta sıfır yok, ön sürüm eki yok). Karşılaştırma sayısal.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Surum(pub u32, pub u32, pub u32);

impl Surum {
    pub const SIFIR: Surum = Surum(0, 0, 0);

    pub fn coz(s: &str) -> Option<Surum> {
        let p: Vec<&str> = s.split('.').collect();
        if p.len() != 3 {
            return None;
        }
        let mut v = [0u32; 3];
        for (i, x) in p.iter().enumerate() {
            if x.is_empty() || x.len() > 6 || !x.bytes().all(|b| b.is_ascii_digit()) || (x.len() > 1 && x.starts_with('0')) {
                return None;
            }
            v[i] = x.parse().ok()?;
        }
        Some(Surum(v[0], v[1], v[2]))
    }
}

impl fmt::Display for Surum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn cozumleme() {
        assert_eq!(Surum::coz("1.8.0"), Some(Surum(1, 8, 0)));
        assert_eq!(Surum::coz("10.0.12"), Some(Surum(10, 0, 12)));
        for k in ["1.8", "1.8.0.1", "01.2.3", "1.2.3-rc1", "1..3", "a.b.c", " 1.2.3", "1.2.3\n", "+1.2.3", ""] {
            assert_eq!(Surum::coz(k), None, "{k:?}");
        }
        assert!(Surum(1, 10, 0) > Surum(1, 9, 9));
        assert_eq!(Surum(2, 0, 1).to_string(), "2.0.1");
    }
}
