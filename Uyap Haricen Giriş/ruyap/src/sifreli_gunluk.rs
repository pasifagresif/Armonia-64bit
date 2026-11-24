//! Şifreli günlük (2026-11-14, kullanıcı isteği: "günlük dosyaya kaydediyorsa şifreli olsun; yalnız program
//! üzerinden paylaşılabilsin").
//!
//! Her satır `%LOCALAPPDATA%/CokluUyap/gunluk/gunluk-*.cul` dosyasına şifreli eklenir. Dosya başına rastgele
//! bir P-256 geçici anahtarı üretilir; AES-256-GCM anahtarı, geçici anahtar ile SATICI açık anahtarının ECDH
//! ortak sırrından türetilir. Gizli anahtar yalnız satıcıdadır (repoda / sunucuda değil) → kullanıcı dosyayı
//! okuyamaz; paylaşılan paket `araclar/gunluk_coz.py` ile çözülür. Biçim o dosyanın başında anlatılır.
//!
//! Paylaşım yalnız programdan: [`masaustune_kaydet`] (zip paketi) ve [`sunucuya_gonder`] (destek kaydı).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use once_cell::sync::Lazy;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Satıcı (Çoklu UYAP) günlük açık anahtarı — P-256, sıkıştırılmamış (65 bayt). Gizli eşi satıcıda.
const SATICI_ACIK_ANAHTAR: &str = "046077f10f3143f2f7de6518f1836b8eda7b5660fcc0ac9ab448d15225b0ec2dfa2684ab19388af83cadc35c18a7ed79927079fb690f117cea080f6603602a42e2";
const ETIKET: &[u8] = b"cokluuyap-gunluk-v1";
const DOSYA_SINIRI: u64 = 5 * 1024 * 1024;
const TUTULAN_DOSYA: usize = 20;

struct Yazici {
    dosya: File,
    aes: Aes256Gcm,
    boyut: u64,
    gun: String,
}

static YAZICI: Lazy<Mutex<Option<Yazici>>> = Lazy::new(|| Mutex::new(None));

pub fn klasor() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".to_string());
    PathBuf::from(base).join("CokluUyap").join("gunluk")
}

fn yeni_yazici() -> Option<Yazici> {
    let dir = klasor();
    fs::create_dir_all(&dir).ok()?;
    let simdi = chrono::Local::now();
    let gun = simdi.format("%Y%m%d").to_string();
    let yol = dir.join(format!("gunluk-{}.cul", simdi.format("%Y%m%d-%H%M%S")));
    let satici = p256::PublicKey::from_sec1_bytes(&hex::decode(SATICI_ACIK_ANAHTAR).ok()?).ok()?;
    let gecici = p256::ecdh::EphemeralSecret::random(&mut rand::rngs::OsRng);
    let ortak = gecici.diffie_hellman(&satici);
    let mut h = Sha256::new();
    h.update(ETIKET);
    h.update(ortak.raw_secret_bytes());
    let aes = Aes256Gcm::new_from_slice(&h.finalize()).ok()?;
    let mut dosya = OpenOptions::new().create_new(true).write(true).open(&yol).ok()?;
    let acik = gecici.public_key().to_encoded_point(false);
    dosya.write_all(b"CUL1").ok()?;
    dosya.write_all(acik.as_bytes()).ok()?;
    eskileri_temizle();
    Some(Yazici { dosya, aes, boyut: 4 + 65, gun })
}

fn eskileri_temizle() {
    let Ok(rd) = fs::read_dir(klasor()) else { return };
    let mut dosyalar: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "cul").unwrap_or(false))
        .collect();
    dosyalar.sort();
    while dosyalar.len() > TUTULAN_DOSYA {
        let _ = fs::remove_file(dosyalar.remove(0));
    }
}

/// Bir satırı şifreli günlüğe ekler (zaman damgasıyla). Hata olursa sessizce geçer: günlük yazılamaması
/// programı durdurmamalı.
pub fn yaz(satir: &str) {
    let satir = satir.trim_end();
    if satir.is_empty() {
        return;
    }
    let Ok(mut kilit) = YAZICI.lock() else { return };
    let bugun = chrono::Local::now().format("%Y%m%d").to_string();
    let yenile = match kilit.as_ref() {
        None => true,
        Some(y) => y.boyut > DOSYA_SINIRI || y.gun != bugun,
    };
    if yenile {
        *kilit = yeni_yazici();
    }
    let Some(y) = kilit.as_mut() else { return };
    let metin = format!("{} {}", chrono::Local::now().format("%H:%M:%S"), satir);
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let Ok(sifreli) = y.aes.encrypt(Nonce::from_slice(&nonce), metin.as_bytes()) else { return };
    let uzunluk = (12 + sifreli.len()) as u32;
    let mut kayit = Vec::with_capacity(4 + uzunluk as usize);
    kayit.extend_from_slice(&uzunluk.to_le_bytes());
    kayit.extend_from_slice(&nonce);
    kayit.extend_from_slice(&sifreli);
    if y.dosya.write_all(&kayit).is_ok() {
        let _ = y.dosya.flush();
        y.boyut += kayit.len() as u64;
    }
}

/// Bütün .cul dosyalarını ve kısa bir bilgi notunu tek zip paketine koyar.
pub fn paketle() -> Result<Vec<u8>, String> {
    let mut dosyalar: Vec<PathBuf> = fs::read_dir(klasor())
        .map_err(|e| format!("Günlük klasörü okunamadı: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "cul").unwrap_or(false))
        .collect();
    dosyalar.sort();
    if dosyalar.is_empty() {
        return Err("Henüz günlük kaydı yok.".to_string());
    }
    let mut tampon = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut tampon);
        let secenek = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let bilgi = format!(
            "Çoklu UYAP günlük paketi\nSürüm: {}\nOluşturulma: {}\nİşletim sistemi: {}\n",
            crate::surum::SURUM,
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            std::env::consts::OS
        );
        z.start_file("bilgi.txt", secenek).map_err(|e| e.to_string())?;
        z.write_all(bilgi.as_bytes()).map_err(|e| e.to_string())?;
        for p in &dosyalar {
            let ad = p.file_name().and_then(|x| x.to_str()).unwrap_or("gunluk.cul");
            let veri = fs::read(p).map_err(|e| format!("{ad} okunamadı: {e}"))?;
            z.start_file(ad, secenek).map_err(|e| e.to_string())?;
            z.write_all(&veri).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(tampon.into_inner())
}

fn masaustu() -> PathBuf {
    let ev = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_else(|_| ".".into());
    for aday in ["Desktop", "OneDrive\\Desktop", "OneDrive\\Masaüstü", "Masaüstü"] {
        let p = PathBuf::from(&ev).join(aday);
        if p.is_dir() {
            return p;
        }
    }
    PathBuf::from(ev)
}

/// Paketi Masaüstüne `CokluUyap-gunluk-<tarih>.zip` olarak kaydeder; yolunu döndürür.
pub fn masaustune_kaydet() -> Result<PathBuf, String> {
    let veri = paketle()?;
    let yol = masaustu().join(format!(
        "CokluUyap-gunluk-{}.zip",
        chrono::Local::now().format("%Y%m%d-%H%M")
    ));
    fs::write(&yol, veri).map_err(|e| format!("Kaydedilemedi: {e}"))?;
    Ok(yol)
}

/// Paketi Çoklu UYAP sunucusuna destek kaydı olarak gönderir; destek numarasını döndürür.
/// `api_base`: https://www.cokluuyap.com gibi. Kimlik: kullanıcının kendi hesabı.
pub fn sunucuya_gonder(api_base: &str, kullanici: &str, ofis: &str, parola: &str) -> Result<String, String> {
    use base64::Engine as _;
    let veri = paketle()?;
    if veri.len() > 20 * 1024 * 1024 {
        return Err("Günlük paketi çok büyük (20 MB üstü); 'Dosyaya kaydet' kullanın.".to_string());
    }
    let govde = serde_json::json!({
        "username": kullanici,
        "office_code": ofis,
        "password": parola,
        "surum": crate::surum::SURUM,
        "paket": base64::engine::general_purpose::STANDARD.encode(&veri),
    });
    let istemci = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())?;
    let yanit = istemci
        .post(format!("{}/destek/gunluk", api_base.trim_end_matches('/')))
        .json(&govde)
        .send()
        .map_err(|e| format!("Sunucuya ulaşılamadı: {e}"))?;
    let durum = yanit.status();
    let j: serde_json::Value = yanit.json().unwrap_or_default();
    if !durum.is_success() {
        return Err(j.get("error").and_then(|v| v.as_str()).unwrap_or("Gönderilemedi.").to_string());
    }
    Ok(j.get("destek_no").and_then(|v| v.as_str()).unwrap_or("?").to_string())
}

#[cfg(test)]
mod tests {
    /// Geçici klasöre birkaç satır yazar ve paketi diske bırakır; çözümü araclar/gunluk_coz.py ile
    /// (gizli anahtarla) elle doğrulanır: CU_GUNLUK_TEST_KLASOR ortam değişkeniyle çalıştırın.
    #[test]
    fn yaz_ve_paketle() {
        let Ok(kok) = std::env::var("CU_GUNLUK_TEST_KLASOR") else { return };
        std::env::set_var("LOCALAPPDATA", &kok);
        super::yaz("[+] Oturum hazır.");
        super::yaz("[!] UYAP girişi başarısız: CKR_PIN_INCORRECT — Türkçe karakter: çğıöşü");
        let paket = super::paketle().expect("paket");
        std::fs::write(std::path::Path::new(&kok).join("paket.zip"), paket).unwrap();
        let ham = std::fs::read_dir(super::klasor()).unwrap().next().unwrap().unwrap().path();
        let icerik = std::fs::read(&ham).unwrap();
        assert_eq!(&icerik[..4], b"CUL1");
        assert!(!String::from_utf8_lossy(&icerik).contains("Oturum"), "düz metin sızdı");
    }
}
