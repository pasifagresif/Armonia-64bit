//! Basit hesap makinesi için ifade çözücü: + − × ÷, yüzde, parantez, Türkçe ondalık virgül.
//! Claude, 2026-11-19.
//!
//! - Sayılar "1.250,5" (Türkçe) ya da "1250.5" yazılabilir: virgül varsa ondalık ayırıcıdır ve noktalar binliktir.
//! - "%": "200 × 18%" = 36; "1000 + 10%" = 1100 (toplama/çıkarmada sol tarafın yüzdesi, hesap makinesi alışkanlığı).
//! - Bölme sıfıra → hata.

pub fn hesapla(s: &str) -> Result<f64, String> {
    let jetonlar = jetonla(s)?;
    if jetonlar.is_empty() {
        return Err("İfade boş".into());
    }
    let mut c = Cozucu { j: &jetonlar, i: 0 };
    let v = c.toplam()?;
    if c.i != jetonlar.len() {
        return Err("İfade eksik ya da hatalı".into());
    }
    if !v.is_finite() {
        return Err("Sonuç tanımsız".into());
    }
    Ok(v)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum J {
    Sayi(f64),
    Arti,
    Eksi,
    Carp,
    Bol,
    Yuzde,
    Ac,
    Kapa,
}

fn sayi_coz(m: &str) -> Option<f64> {
    // Virgül yoksa: noktadan sonraki her grup tam 3 haneyse noktalar binliktir ("12.500" = 12500), değilse ondalık.
    let binlik = |m: &str| {
        let mut p = m.split('.');
        let ilk = p.next().unwrap_or("");
        !ilk.is_empty() && ilk.len() <= 3 && m.contains('.') && p.all(|g| g.len() == 3)
    };
    let m = if m.contains(',') {
        m.replace('.', "").replace(',', ".")
    } else if binlik(m) || m.matches('.').count() > 1 {
        m.replace('.', "")
    } else {
        m.to_string()
    };
    m.parse().ok()
}

fn jetonla(s: &str) -> Result<Vec<J>, String> {
    let mut v = Vec::new();
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        match ch {
            ' ' | '\t' => {}
            '+' => v.push(J::Arti),
            '-' | '−' => v.push(J::Eksi),
            '*' | '×' | 'x' => v.push(J::Carp),
            '/' | '÷' | ':' => v.push(J::Bol),
            '%' => v.push(J::Yuzde),
            '(' => v.push(J::Ac),
            ')' => v.push(J::Kapa),
            d if d.is_ascii_digit() || d == ',' || d == '.' => {
                let bas = i;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == ',' || c[i] == '.') {
                    i += 1;
                }
                let m: String = c[bas..i].iter().collect();
                v.push(J::Sayi(sayi_coz(&m).ok_or(format!("Sayı anlaşılamadı: {m}"))?));
                continue;
            }
            d => return Err(format!("Tanınmayan karakter: {d}")),
        }
        i += 1;
    }
    Ok(v)
}

struct Cozucu<'a> {
    j: &'a [J],
    i: usize,
}

impl Cozucu<'_> {
    fn bak(&self) -> Option<J> {
        self.j.get(self.i).copied()
    }

    fn toplam(&mut self) -> Result<f64, String> {
        let mut v = self.carpim()?;
        while let Some(op @ (J::Arti | J::Eksi)) = self.bak() {
            self.i += 1;
            let mut r = self.carpim_yuzdesiz()?;
            if self.bak() == Some(J::Yuzde) {
                self.i += 1;
                r = v * r / 100.0;
            }
            v = if op == J::Arti { v + r } else { v - r };
        }
        Ok(v)
    }

    /// Toplamanın sağ tarafı: sondaki "%" toplam düzeyinde yorumlansın diye tek başına bırakılır.
    fn carpim_yuzdesiz(&mut self) -> Result<f64, String> {
        let mut v = self.tekli()?;
        loop {
            match self.bak() {
                Some(J::Yuzde) if matches!(self.j.get(self.i + 1), Some(J::Carp | J::Bol)) => {
                    self.i += 1;
                    v /= 100.0;
                }
                Some(op @ (J::Carp | J::Bol)) => {
                    self.i += 1;
                    let r = self.yuzdeli_tekli()?;
                    v = bol_carp(v, op, r)?;
                }
                _ => return Ok(v),
            }
        }
    }

    fn carpim(&mut self) -> Result<f64, String> {
        let mut v = self.yuzdeli_tekli()?;
        while let Some(op @ (J::Carp | J::Bol)) = self.bak() {
            self.i += 1;
            let r = self.yuzdeli_tekli()?;
            v = bol_carp(v, op, r)?;
        }
        Ok(v)
    }

    fn yuzdeli_tekli(&mut self) -> Result<f64, String> {
        let mut v = self.tekli()?;
        while self.bak() == Some(J::Yuzde) {
            self.i += 1;
            v /= 100.0;
        }
        Ok(v)
    }

    fn tekli(&mut self) -> Result<f64, String> {
        match self.bak() {
            Some(J::Eksi) => {
                self.i += 1;
                Ok(-self.tekli()?)
            }
            Some(J::Arti) => {
                self.i += 1;
                self.tekli()
            }
            Some(J::Sayi(x)) => {
                self.i += 1;
                Ok(x)
            }
            Some(J::Ac) => {
                self.i += 1;
                let v = self.toplam()?;
                if self.bak() != Some(J::Kapa) {
                    return Err("Parantez kapanmadı".into());
                }
                self.i += 1;
                Ok(v)
            }
            _ => Err("İfade eksik ya da hatalı".into()),
        }
    }
}

fn bol_carp(v: f64, op: J, r: f64) -> Result<f64, String> {
    if op == J::Carp {
        Ok(v * r)
    } else if r == 0.0 {
        Err("Sıfıra bölünemez".into())
    } else {
        Ok(v / r)
    }
}

/// Sonucu Türkçe yazar: binlik nokta, ondalık virgül, en çok 10 ondalık (sondaki sıfırlar atılır).
pub fn yaz(x: f64) -> String {
    let s = format!("{:.10}", x.abs());
    let (tam, ond) = s.split_once('.').unwrap();
    let ond = ond.trim_end_matches('0');
    let mut g = String::new();
    for (i, ch) in tam.chars().enumerate() {
        if i > 0 && (tam.len() - i) % 3 == 0 {
            g.push('.');
        }
        g.push(ch);
    }
    let isaret = if x < 0.0 && (tam != "0" || !ond.is_empty()) { "-" } else { "" };
    if ond.is_empty() {
        format!("{isaret}{g}")
    } else {
        format!("{isaret}{g},{ond}")
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn h(s: &str) -> String {
        yaz(hesapla(s).unwrap())
    }

    #[test]
    fn dort_islem() {
        assert_eq!(h("2+3*4"), "14");
        assert_eq!(h("(2+3)×4"), "20");
        assert_eq!(h("10 ÷ 4"), "2,5");
        assert_eq!(h("-5 + 2"), "-3");
        assert_eq!(h("1.250,50 + 0,5"), "1.251");
        assert_eq!(h("1250.5*2"), "2.501");
        assert_eq!(h("12.500 × 18%"), "2.250");
        assert_eq!(h("2.5 + 1"), "3,5");
        assert_eq!(h("1.000.000 + 1"), "1.000.001");
        assert_eq!(h("1.000.000 / 3"), "333.333,3333333333");
        assert_eq!(h("0,1+0,2"), "0,3");
    }

    #[test]
    fn yuzde() {
        assert_eq!(h("200 × 18%"), "36");
        assert_eq!(h("1000 + 10%"), "1.100");
        assert_eq!(h("1000 - 20%"), "800");
        assert_eq!(h("50%"), "0,5");
        assert_eq!(h("1000 + 10% × 2"), "1.000,2");
    }

    #[test]
    fn hatalar() {
        assert!(hesapla("5/0").unwrap_err().contains("Sıfır"));
        assert!(hesapla("(2+3").is_err());
        assert!(hesapla("2+").is_err());
        assert!(hesapla("").is_err());
        assert!(hesapla("2a").is_err());
    }
}
