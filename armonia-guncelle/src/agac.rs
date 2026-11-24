//! Kurulu sürüm ağacının bütünlüğü: MANIFEST'in özeti imzalı `agac_ozeti` ile, her dosya MANIFEST ile tutmalı;
//! fazladan dosya, sembolik bağ ya da başka tür girdi olmamalı. Hiçbir şey silmez, yalnız okur.
//! Çoklu UYAP kum havuzunun İÇİNDE (User=armonia) çalışır; root olarak çalıştırılması gerekmez (denetçi 24.11 O2).

use crate::{dosya, hata, ozet_hex, paket, Sonuc};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::Path;

/// Sürüm dizininde ağaç dışı tutulan (kurucunun yazdığı) dosyalar
pub const BILGI_ADI: &str = "guncelleme.json";
pub const IMZA_ADI: &str = "guncelleme.json.sig";

fn yuru(kok: &Path, dizin: &Path, onek: &str, sonuc: &mut BTreeSet<String>) -> Sonuc<()> {
    for g in fs::read_dir(dizin)? {
        let g = g?;
        let ad = g.file_name().into_string().map_err(|_| hata!("ağaçta UTF-8 olmayan ad"))?;
        let rel = if onek.is_empty() { ad.clone() } else { format!("{onek}/{ad}") };
        if onek.is_empty() && (ad == paket::MANIFEST || ad == BILGI_ADI || ad == IMZA_ADI) {
            continue;
        }
        let m = fs::symlink_metadata(g.path())?;
        if m.file_type().is_symlink() {
            return Err(hata!("ağaçta sembolik bağ: {rel}"));
        } else if m.is_dir() {
            yuru(kok, &g.path(), &rel, sonuc)?;
        } else if m.is_file() {
            if sonuc.len() >= paket::DOSYA_SAYI_AZAMI {
                return Err(hata!("ağaçta çok fazla dosya"));
            }
            sonuc.insert(rel);
        } else {
            return Err(hata!("ağaçta düz olmayan dosya: {rel}"));
        }
    }
    let _ = kok;
    Ok(())
}

pub fn dogrula(dizin: &Path, agac_ozeti: &str) -> Sonuc<()> {
    let m = dosya::oku_sinirli(&dizin.join(paket::MANIFEST), paket::MANIFEST_AZAMI)?;
    if ozet_hex(&m) != agac_ozeti {
        return Err(hata!("MANIFEST imzalı özetle tutmuyor"));
    }
    let beklenen = paket::manifest_coz(&m)?;
    let mut bulunan = BTreeSet::new();
    yuru(dizin, dizin, "", &mut bulunan)?;
    if !beklenen.keys().eq(bulunan.iter()) {
        return Err(hata!("ağaçtaki dosyalar MANIFEST ile tutmuyor"));
    }
    let mut tampon = vec![0u8; 64 * 1024];
    for (y, o) in &beklenen {
        let mut f = dosya::ac_bagsiz(&dizin.join(y))?;
        let mut h = Sha256::new();
        loop {
            let n = f.read(&mut tampon)?;
            if n == 0 {
                break;
            }
            h.update(&tampon[..n]);
        }
        if hex::encode(h.finalize()) != *o {
            return Err(hata!("dosya değişmiş: {y}"));
        }
    }
    Ok(())
}
