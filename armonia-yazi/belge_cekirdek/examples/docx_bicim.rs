//! UDF → DOCX → geri: ilk farklı biçimli paragrafın parçalarını gösterir (metin değil, uzunluk). Claude, 2026-11-23.
use belge_cekirdek::stil::{Renk, StilCozucu};
use belge_cekirdek::{docx, docx_yaz, udf, Blok, Paragraf, Parca};
fn ps(b: &[Blok], v: &mut Vec<Paragraf>) {
    for x in b { match x { Blok::Paragraf(p) => v.push(p.clone()), Blok::Tablo(t) => t.satirlar.iter().for_each(|r| r.hucreler.iter().for_each(|h| ps(&h.bloklar, v))) } }
}
fn main() {
    for yol in std::env::args().skip(1) {
        let a = udf::oku(&std::fs::read(&yol).unwrap()).unwrap().belge;
        let b = docx::oku(&docx_yaz::yaz(&a).unwrap()).unwrap().belge;
        let (ca, cb) = (StilCozucu::yeni(&a), StilCozucu::yeni(&b));
        let (mut pa, mut pb) = (vec![], vec![]);
        ps(&a.govde, &mut pa); ps(&b.govde, &mut pb);
        for (i, (x, y)) in pa.iter().zip(&pb).enumerate() {
            let goster = |c: &StilCozucu, p: &Paragraf| p.parcalar.iter().map(|k| {
                let s = c.parca(k, p, Renk::SIYAH);
                let tur = match k { Parca::Metin{..}=>"M", Parca::Bosluk{..}=>"B", Parca::Sekme{..}=>"S", Parca::Alan{..}=>"A", Parca::Resim{..}=>"R" };
                format!("{tur}{}{}{}{}", k.metin().chars().count(), if s.kalin {"k"} else {""}, if s.egik {"e"} else {""}, if s.alti_cizili {"a"} else {""})
            }).collect::<Vec<_>>().join(" ");
            let (gx, gy) = (goster(&ca, x), goster(&cb, y));
            let ilk = |c: &StilCozucu, p: &Paragraf| p.parcalar.iter().find(|k| matches!(k, Parca::Metin{metin,..} if !metin.trim().is_empty())).map(|k| c.parca(k, p, Renk::SIYAH).alti_cizili);
            if ilk(&ca, x) != ilk(&cb, y) { println!("paragraf {i}\n  önce: {gx}\n  sonra: {gy}"); return; }
        }
    }
}
