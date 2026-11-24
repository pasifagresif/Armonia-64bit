//! İmzalı güncelleme bilgisi (`guncelleme.json`) ve doğrulaması.

use crate::anahtar::Anahtarlar;
use crate::surum::Surum;
use crate::{hata, hex64_mi, zaman, Sonuc, BICIM, KANAL, URUN};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BILGI_AZAMI: u64 = 64 * 1024;
pub const PAKET_AZAMI: u64 = 256 * 1024 * 1024;
pub const NOT_AZAMI: usize = 2000;
/// Saat kayması payı: yayın zamanı bu kadar ileride olabilir
pub const SAAT_PAYI: i64 = 300;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Bilgi {
    pub bicim: u32,
    pub urun: String,
    pub kanal: String,
    pub surum: String,
    pub yayin: String,
    pub gecerlilik_sonu: String,
    pub imaj_arayuzu: u32,
    #[serde(default)]
    pub geri_cekilen: Vec<String>,
    pub paketler: BTreeMap<String, Paket>,
    pub notlar: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Paket {
    pub ad: String,
    pub sha256: String,
    pub boyut: u64,
    pub agac_ozeti: String,
}

/// Doğrulamanın bağlamı: bu cihaz ve imaj hakkında bilinenler.
#[derive(Clone, Debug)]
pub struct Baglam {
    pub mimari: String,
    /// İmajın desteklediği en yüksek arayüz düzeyi (kum havuzu izinleri)
    pub imaj_arayuzu: u32,
    /// İmajdaki Çoklu UYAP'ın derleme zamanı: daha eski yayın kabul edilmez
    pub imaj_tarihi: i64,
    pub simdi: i64,
    /// `gecerlilik_sonu` denetlensin mi (yeni sürüm önerirken/kurarken evet; kurulu sürümü çalıştırırken hayır)
    pub sure_denetimi: bool,
}

#[derive(Clone, Debug)]
pub struct Dogrulanmis {
    pub bilgi: Bilgi,
    pub surum: Surum,
    pub paket: Paket,
    pub geri_cekilen: Vec<Surum>,
}

pub fn paket_adi(surum: Surum, mimari: &str) -> String {
    format!("cokluuyap-{surum}-{mimari}.tar.zst")
}

pub fn mimari_gecerli(m: &str) -> bool {
    matches!(m, "x86_64" | "aarch64")
}

/// İmzayı doğrular, ardından bütün alanları denetler. Sürümün tabandan büyük olması çağıranın işidir.
pub fn dogrula(json: &[u8], imza: &[u8], anahtarlar: &Anahtarlar, b: &Baglam) -> Sonuc<Dogrulanmis> {
    if json.len() as u64 > BILGI_AZAMI {
        return Err(hata!("güncelleme bilgisi çok büyük"));
    }
    // Önce imza: imzasız içerik ayrıştırılmaz bile
    anahtarlar.bilgi_imzasi_gecerli(json, imza)?;
    let bilgi: Bilgi = serde_json::from_slice(json).map_err(|e| hata!("güncelleme bilgisi bozuk: {e}"))?;
    if bilgi.bicim != BICIM {
        return Err(hata!("bilinmeyen bilgi biçimi {}", bilgi.bicim));
    }
    if bilgi.urun != URUN {
        return Err(hata!("bilgi bu ürün için değil ({})", bilgi.urun));
    }
    if bilgi.kanal != KANAL {
        return Err(hata!("bilinmeyen kanal ({})", bilgi.kanal));
    }
    let surum = Surum::coz(&bilgi.surum).ok_or_else(|| hata!("sürüm geçersiz"))?;
    let mut geri_cekilen = Vec::new();
    for g in &bilgi.geri_cekilen {
        geri_cekilen.push(Surum::coz(g).ok_or_else(|| hata!("geri çekilen sürüm geçersiz"))?);
    }
    if geri_cekilen.len() > 256 {
        return Err(hata!("geri çekilen listesi çok uzun"));
    }
    if geri_cekilen.contains(&surum) {
        return Err(hata!("sürüm kendi geri çekilen listesinde"));
    }
    let yayin = zaman::coz(&bilgi.yayin).ok_or_else(|| hata!("yayın zamanı geçersiz"))?;
    let son = zaman::coz(&bilgi.gecerlilik_sonu).ok_or_else(|| hata!("geçerlilik sonu geçersiz"))?;
    if son <= yayin {
        return Err(hata!("geçerlilik sonu yayından önce"));
    }
    if yayin < b.imaj_tarihi {
        return Err(hata!("yayın, imajdaki sürümden eski"));
    }
    if b.sure_denetimi {
        if b.simdi + SAAT_PAYI < yayin {
            return Err(hata!("sistem saati yayın zamanının gerisinde (saat yanlış ya da geri alınmış)"));
        }
        if b.simdi >= son {
            return Err(hata!("güncelleme bilgisinin süresi geçmiş ({})", bilgi.gecerlilik_sonu));
        }
        if b.simdi < b.imaj_tarihi {
            return Err(hata!("sistem saati imajın derleme zamanının gerisinde"));
        }
    }
    if bilgi.imaj_arayuzu == 0 || bilgi.imaj_arayuzu > b.imaj_arayuzu {
        return Err(hata!(
            "bu sürüm daha yeni bir Armonia imajı gerektiriyor (arayüz {} > {})",
            bilgi.imaj_arayuzu,
            b.imaj_arayuzu
        ));
    }
    if bilgi.notlar.chars().count() > NOT_AZAMI || bilgi.notlar.chars().any(|c| c.is_control() && c != '\n') {
        return Err(hata!("sürüm notları geçersiz"));
    }
    for (m, p) in &bilgi.paketler {
        if !mimari_gecerli(m) {
            return Err(hata!("bilinmeyen mimari {m}"));
        }
        if p.ad != paket_adi(surum, m) || !hex64_mi(&p.sha256) || !hex64_mi(&p.agac_ozeti) || p.boyut == 0 || p.boyut > PAKET_AZAMI {
            return Err(hata!("{m} paket kaydı geçersiz"));
        }
    }
    let paket = bilgi
        .paketler
        .get(&b.mimari)
        .cloned()
        .ok_or_else(|| hata!("bu sürümde {} paketi yok", b.mimari))?;
    Ok(Dogrulanmis { bilgi, surum, paket, geri_cekilen })
}
