//! Adalet E-imza istemcisi (127.0.0.1:5975) arka ucu.
//!
//! Armonia'da karta yalnız Adalet'in resmî istemcisi erişir (Java, PC/SC; ISO'da ve Pi'de çalışıyor). İstemci
//! UDF'i kendisi imzalar: `POST /api/v1/docsignature/signUdf` {certificateId, pin, content(base64 UDF)} →
//! {content(base64 imzalı UDF), imzaGecerliMi, signatureList} (uç noktalar ve alan adları istemcinin
//! eimza-tray-1.0.17.jar sınıflarından okundu, 23.11; yanıt zarfı `{metadata:{STATUS,MESSAGE}, data}` ruyap
//! uyap_giris_disaridan.rs'deki getCertificates / sign kullanımından). **Gerçek kartla denenmedi.**
//! Yalnız 127.0.0.1'e bağlanır; TLS yok (yerel). Claude, 2026-11-23.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;
use serde_json::{json, Value};

use crate::udf_imza::ImzaHatasi;

pub const VARSAYILAN_ADRES: &str = "127.0.0.1:5975";
const AZAMI_YANIT: usize = 128 << 20;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AdaletSertifika {
    /// İstemcinin sertifika kimliği (imzada kullanılır)
    pub kimlik: String,
    pub ad: String,
    pub veren: String,
    pub bas: String,
    pub son: String,
    pub okuyucu: String,
}

pub struct Adalet {
    pub adres: String,
}

impl Default for Adalet {
    fn default() -> Self {
        Adalet { adres: VARSAYILAN_ADRES.into() }
    }
}

/// Basit HTTP/1.1 istek (yalnız yerel istemci için): Content-Length ya da chunked yanıt.
fn http(adres: &str, yontem: &str, yol: &str, govde: Option<&[u8]>, sure: Duration) -> Result<(u16, Vec<u8>), String> {
    let mut s = TcpStream::connect(adres).map_err(|e| format!("e-imza istemcisine ({adres}) ulaşılamadı: {e}"))?;
    s.set_read_timeout(Some(sure)).ok();
    s.set_write_timeout(Some(sure)).ok();
    let govde = govde.unwrap_or(&[]);
    let baslik = format!(
        "{yontem} {yol} HTTP/1.1\r\nHost: localhost:5975\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        govde.len()
    );
    s.write_all(baslik.as_bytes()).and_then(|_| s.write_all(govde)).map_err(|e| format!("istek gönderilemedi: {e}"))?;
    let mut ham = Vec::new();
    s.take(AZAMI_YANIT as u64 + 1).read_to_end(&mut ham).map_err(|e| format!("yanıt okunamadı: {e}"))?;
    if ham.len() > AZAMI_YANIT {
        return Err("yanıt çok büyük".into());
    }
    let ayrac = ham.windows(4).position(|w| w == b"\r\n\r\n").ok_or("bozuk HTTP yanıtı")?;
    let basliklar = String::from_utf8_lossy(&ham[..ayrac]).to_string();
    let mut govde = ham[ayrac + 4..].to_vec();
    let durum: u16 = basliklar.split_whitespace().nth(1).and_then(|k| k.parse().ok()).ok_or("bozuk HTTP durum satırı")?;
    if basliklar.lines().any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.to_ascii_lowercase().contains("chunked")) {
        govde = parcali_coz(&govde)?;
    }
    Ok((durum, govde))
}

fn parcali_coz(mut d: &[u8]) -> Result<Vec<u8>, String> {
    let mut v = Vec::new();
    loop {
        let son = d.windows(2).position(|w| w == b"\r\n").ok_or("bozuk parçalı yanıt")?;
        let boy_metni = String::from_utf8_lossy(&d[..son]);
        let boy = usize::from_str_radix(boy_metni.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "bozuk parça boyu")?;
        d = &d[son + 2..];
        if boy == 0 {
            return Ok(v);
        }
        if d.len() < boy + 2 {
            return Err("kesik parçalı yanıt".into());
        }
        v.extend_from_slice(&d[..boy]);
        d = &d[boy + 2..];
    }
}

/// `{metadata:{STATUS,MESSAGE}, data}` zarfını açar.
fn zarf(durum: u16, govde: &[u8]) -> Result<Value, String> {
    let j: Value = serde_json::from_slice(govde).map_err(|_| format!("e-imza istemcisi geçersiz yanıt verdi (HTTP {durum})"))?;
    let m = j.get("metadata").cloned().unwrap_or(Value::Null);
    if m.get("STATUS").and_then(Value::as_str) != Some("SUCCESS") {
        let mesaj = m.get("MESSAGE").and_then(Value::as_str).unwrap_or("bilinmeyen hata");
        return Err(mesaj.to_string());
    }
    Ok(j.get("data").cloned().unwrap_or(Value::Null))
}

fn pin_hatasi_mi(m: &str) -> bool {
    let k = m.to_lowercase();
    ["pin", "şifre", "sifre", "parola", "password", "incorrect", "hatalı", "kilit", "locked"].iter().any(|x| k.contains(x))
}

impl Adalet {
    pub fn sertifikalar(&self) -> Result<Vec<AdaletSertifika>, ImzaHatasi> {
        let (d, g) = http(&self.adres, "GET", "/api/v1/signature/getCertificates", None, Duration::from_secs(15)).map_err(ImzaHatasi::KartYok)?;
        let veri = zarf(d, &g).map_err(ImzaHatasi::KartYok)?;
        let mut v = Vec::new();
        for terminal in veri.as_array().cloned().unwrap_or_default() {
            let okuyucu = terminal.get("terminal").and_then(Value::as_str).unwrap_or("").to_string();
            for c in terminal.get("certificates").and_then(Value::as_array).cloned().unwrap_or_default() {
                let al = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                v.push(AdaletSertifika { kimlik: al("certificateId"), ad: al("subject"), veren: al("issuer"), bas: al("notBefore"), son: al("notAfter"), okuyucu: okuyucu.clone() });
            }
        }
        if v.is_empty() {
            return Err(ImzaHatasi::KartYok("Kart takılı değil ya da kartta sertifika yok".into()));
        }
        Ok(v)
    }

    /// UDF'i istemciye imzalatır. `kimlik` verilmezse tek sertifika varsa o kullanılır.
    pub fn udf_imzala(&self, udf: &[u8], pin: &str, kimlik: Option<&str>) -> Result<Vec<u8>, ImzaHatasi> {
        let kimlik = match kimlik {
            Some(k) => k.to_string(),
            None => {
                let s = self.sertifikalar()?;
                if s.len() > 1 {
                    return Err(ImzaHatasi::Diger("Kartta birden çok sertifika var; hangisiyle imzalanacağını seçin".into()));
                }
                s[0].kimlik.clone()
            }
        };
        let govde = json!({
            "certificateId": kimlik,
            "pin": pin,
            "content": base64::engine::general_purpose::STANDARD.encode(udf),
        })
        .to_string();
        let (d, g) = http(&self.adres, "POST", "/api/v1/docsignature/signUdf", Some(govde.as_bytes()), Duration::from_secs(120))
            .map_err(ImzaHatasi::KartYok)?;
        let veri = zarf(d, &g).map_err(|m| if pin_hatasi_mi(&m) { ImzaHatasi::PinHatali(m) } else { ImzaHatasi::Diger(format!("e-imza istemcisi: {m}")) })?;
        let icerik = veri.get("content").and_then(Value::as_str).ok_or_else(|| ImzaHatasi::Diger("e-imza istemcisi imzalı belge döndürmedi".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(icerik.trim())
            .map_err(|_| ImzaHatasi::Diger("e-imza istemcisinin döndürdüğü belge bozuk".into()))
    }
}
