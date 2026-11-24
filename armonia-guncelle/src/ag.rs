//! İndirme (armonia-guncelle-denetle / -indir; kullanıcı `armonia-guncelle-indir`). Yalnız sabit kökten
//! (`KOK_ADRES`), yalnız HTTPS, yönlendirme YOK, ortamdaki vekil yok sayılır. TLS güven kaynağı değildir; içerik
//! her zaman imzayla doğrulanır. Kök sertifikalar: webpki-roots (Mozilla, ikiliye gömülü).

use crate::{hata, Sonuc, KOK_ADRES};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::time::Duration;

pub fn ajan() -> ureq::Agent {
    let c = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .proxy(None)
        .http_status_as_error(true)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(1800)))
        .user_agent("armonia-guncelle/1")
        .build();
    ureq::Agent::new_with_config(c)
}

pub fn ad_gecerli(ad: &str) -> bool {
    !ad.is_empty() && ad.len() <= 100 && !ad.starts_with('.') && ad.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-' || b == b'_')
}

fn ac(ajan: &ureq::Agent, ad: &str) -> Sonuc<Option<ureq::http::Response<ureq::Body>>> {
    if !ad_gecerli(ad) {
        return Err(hata!("geçersiz dosya adı {ad:?}"));
    }
    match ajan.get(&format!("{KOK_ADRES}{ad}")).call() {
        // max_redirects(0) iken 3xx hata değil yanıt olarak döner; yalnız 200 kabul (denetçi 24.11 D-1)
        Ok(r) if r.status().as_u16() == 200 => Ok(Some(r)),
        Ok(r) => Err(hata!("{ad}: beklenmeyen yanıt {}", r.status())),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(hata!("{ad} indirilemedi: {e}")),
    }
}

/// Küçük dosya (bilgi, imza); yoksa (404) None.
pub fn getir(ajan: &ureq::Agent, ad: &str, azami: u64) -> Sonuc<Option<Vec<u8>>> {
    let Some(r) = ac(ajan, ad)? else { return Ok(None) };
    let mut v = Vec::new();
    r.into_body().into_reader().take(azami + 1).read_to_end(&mut v).map_err(|e| hata!("{ad}: {e}"))?;
    if v.len() as u64 > azami {
        return Err(hata!("{ad} beklenenden büyük"));
    }
    Ok(Some(v))
}

/// Paketi akış halinde yazar; boyut ve sha256 imzalı bilgiyle tutmalı (aşılırsa hemen keser).
pub fn paket_indir(ajan: &ureq::Agent, ad: &str, hedef: &mut impl Write, boyut: u64, sha256: &str) -> Sonuc<()> {
    let r = ac(ajan, ad)?.ok_or_else(|| hata!("{ad} sunucuda yok"))?;
    let mut ok = r.into_body().into_reader().take(boyut + 1);
    let mut h = Sha256::new();
    let mut tampon = vec![0u8; 64 * 1024];
    let mut toplam = 0u64;
    loop {
        let n = ok.read(&mut tampon).map_err(|e| hata!("{ad}: {e}"))?;
        if n == 0 {
            break;
        }
        toplam += n as u64;
        if toplam > boyut {
            return Err(hata!("{ad} imzalı boyuttan büyük"));
        }
        h.update(&tampon[..n]);
        hedef.write_all(&tampon[..n])?;
    }
    if toplam != boyut || hex::encode(h.finalize()) != sha256 {
        return Err(hata!("{ad} imzalı özetle tutmuyor"));
    }
    Ok(())
}
