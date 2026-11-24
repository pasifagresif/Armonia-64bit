//! Çoklu UYAP açılırken hangi sürümün çalışacağı (cokluuyap_calistir.sh, kum havuzunun içinde).
//! Her adım başarısızsa imajdaki sürüm çalışır; hiçbir şey silinmez ya da değiştirilmez.

use crate::agac;
use crate::anahtar::Anahtarlar;
use crate::bilgi::{self, Baglam};
use crate::kur::{surum_dosyasi_oku, Imaj, Yerlesim};
use crate::surum::Surum;
use crate::{dosya, hata, Sonuc};
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Secim {
    Guncel(PathBuf, Surum),
    Imaj(String),
}

/// `son_bilgi`: denetleyicinin en son indirdiği (bilgi, imza); yalnız `geri_cekilen` için kullanılır.
pub fn etkin_sec(
    y: &Yerlesim,
    anahtarlar: &Anahtarlar,
    imaj: &Imaj,
    mimari: &str,
    simdi: i64,
    son_bilgi: Option<(&[u8], &[u8])>,
) -> Sonuc<Secim> {
    let Some(s) = surum_dosyasi_oku(&y.etkin())? else {
        return Ok(Secim::Imaj("güncelleme kurulmamış".into()));
    };
    if s <= imaj.surum {
        return Ok(Secim::Imaj(format!("imajdaki sürüm ({}) kurulu güncellemeden ({s}) yeni", imaj.surum)));
    }
    let d = y.surum_dizini(s);
    let json = dosya::oku_sinirli(&d.join(agac::BILGI_ADI), bilgi::BILGI_AZAMI)?;
    let imza = dosya::oku_sinirli(&d.join(agac::IMZA_ADI), crate::anahtar::IMZA_AZAMI)?;
    // Kurulu sürüm çalıştırılırken bilginin süresi denetlenmez (site kapalı olsa da çalışmaya devam etsin)
    let b = Baglam { mimari: mimari.into(), imaj_arayuzu: imaj.arayuz, imaj_tarihi: imaj.tarih, simdi, sure_denetimi: false };
    let v = bilgi::dogrula(&json, &imza, anahtarlar, &b)?;
    if v.surum != s {
        return Err(hata!("etkin dosyası ({s}) ile imzalı bilgi ({}) tutmuyor", v.surum));
    }
    if let Some((j, i)) = son_bilgi {
        if let Ok(son) = bilgi::dogrula(j, i, anahtarlar, &Baglam { sure_denetimi: false, ..b.clone() }) {
            if son.geri_cekilen.contains(&s) {
                return Ok(Secim::Imaj(format!("{s} yayıncı tarafından geri çekildi")));
            }
        }
    }
    agac::dogrula(&d, &v.paket.agac_ozeti)?;
    Ok(Secim::Guncel(d, s))
}
