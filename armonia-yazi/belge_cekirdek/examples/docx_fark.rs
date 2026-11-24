//! UDF → DOCX → geri: paragraf / tablo yapısı farkının yerini bulur (metin yazdırılmaz). Claude, 2026-11-23.
use belge_cekirdek::{docx, docx_yaz, udf, Blok};

fn ozet(b: &[Blok], v: &mut Vec<String>, derinlik: usize) {
    for x in b {
        match x {
            Blok::Paragraf(_) => v.push(format!("{}P", " ".repeat(derinlik))),
            Blok::Tablo(t) => {
                v.push(format!("{}T{}", " ".repeat(derinlik), t.satirlar.len()));
                for (ri, r) in t.satirlar.iter().enumerate() {
                    for (hi, h) in r.hucreler.iter().enumerate() {
                        v.push(format!("{}hucre {ri}/{hi} blok:{}", " ".repeat(derinlik + 1), h.bloklar.len()));
                        ozet(&h.bloklar, v, derinlik + 2);
                    }
                }
            }
        }
    }
}

fn main() {
    for (n, yol) in std::env::args().skip(1).enumerate() {
        let b = udf::oku(&std::fs::read(&yol).unwrap()).unwrap().belge;
        let g = docx::oku(&docx_yaz::yaz(&b).unwrap()).unwrap().belge;
        let (mut a, mut c) = (Vec::new(), Vec::new());
        ozet(&b.govde, &mut a, 0);
        ozet(&g.govde, &mut c, 0);
        if a != c {
            println!("== dosya {n}: {} / {} satır", a.len(), c.len());
            let i = a.iter().zip(&c).position(|(x, y)| x != y).unwrap_or(a.len().min(c.len()));
            println!("  önce: {:?}", &a[i.saturating_sub(2)..(i + 4).min(a.len())]);
            println!("  sonra: {:?}", &c[i.saturating_sub(2)..(i + 4).min(c.len())]);
        }
    }
}
