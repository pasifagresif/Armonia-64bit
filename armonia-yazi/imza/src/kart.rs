//! Kartla imzalama (PKCS#11, AKİS ve benzerleri). ruyap `udf_signer.rs` `kartla_imzala` mantığının imza paketine
//! uyarlanmışı: oturum aç → PIN ile giriş → imza sertifikası (anahtar kullanımı: dijital imza / inkâr edilemezlik)
//! → aynı CKA_ID'li özel anahtar → imza (RSA PKCS#1 v1.5 SHA-256 DigestInfo ya da ECDSA). Oturum düşünce kapanır.
//! Claude, 2026-11-23.

use sha2::{Digest, Sha256};

use crate::cades;
use crate::pkcs11::{self as p, AttrValue, Mechanism, PyKCS11Lib, Session, TemplateVal};
use crate::udf_imza::{ImzaHatasi, Imzalayici};

/// Sürücü adayları. `ARMONIA_PKCS11` ortam değişkeni önce gelir (Armonia birimi kesin yolu verir).
pub fn surucu_yolu() -> Option<String> {
    if let Ok(y) = std::env::var("ARMONIA_PKCS11") {
        if std::path::Path::new(&y).exists() {
            return Some(y);
        }
    }
    let adaylar: &[&str] = if cfg!(windows) {
        &["akisp11.dll", "akisp11_64.dll", "etpkcs11.dll"]
    } else {
        &[
            "/usr/lib/libakisp11.so",
            "/usr/lib/x86_64-linux-gnu/libakisp11.so",
            "/usr/lib/aarch64-linux-gnu/libakisp11.so",
            "/usr/local/lib/libakisp11.so",
            "/opt/akis/libakisp11.so",
            "/usr/lib/adalet-eimza-tray/lib/libakisp11.so",
        ]
    };
    adaylar
        .iter()
        .find(|a| cfg!(windows) || std::path::Path::new(a).exists())
        .map(|a| a.to_string())
}

/// Kart üzerindeki bir sertifikanın özeti (PIN'siz okunur).
#[derive(Debug, Clone, serde::Serialize)]
pub struct KartSertifikasi {
    pub ad: String,
    pub seri: String,
    pub imza_icin: bool,
}

fn kutuphane() -> Result<PyKCS11Lib, ImzaHatasi> {
    let yol = surucu_yolu().ok_or_else(|| ImzaHatasi::KartYok("E-imza kartı sürücüsü (PKCS#11) bulunamadı".into()))?;
    let mut l = PyKCS11Lib::new();
    l.load(&yol).map_err(|e| ImzaHatasi::KartYok(format!("kart sürücüsü yüklenemedi: {e}")))?;
    Ok(l)
}

fn oturum(l: &PyKCS11Lib) -> Result<Session, ImzaHatasi> {
    let slotlar = l.getSlotList(true).map_err(|e| ImzaHatasi::KartYok(format!("kart okuyucu listesi alınamadı: {e}")))?;
    let slot = *slotlar.first().ok_or_else(|| ImzaHatasi::KartYok("Kart takılı değil".into()))?;
    l.openSession(slot, p::CKF_SERIAL_SESSION).map_err(|e| ImzaHatasi::KartYok(format!("kart oturumu açılamadı: {e}")))
}

/// Karttaki sertifikalar (PIN gerekmez; sertifikalar açık nesnelerdir).
pub fn sertifikalar() -> Result<Vec<KartSertifikasi>, ImzaHatasi> {
    let l = kutuphane()?;
    let s = oturum(&l)?;
    let mut v = Vec::new();
    let nesneler = s
        .findObjects(&[(p::CKA_CLASS, TemplateVal::Ulong(p::CKO_CERTIFICATE))])
        .map_err(|e| ImzaHatasi::Diger(format!("sertifika araması başarısız: {e}")))?;
    for n in nesneler {
        let Ok(a) = s.getAttributeValue(n, &[p::CKA_VALUE]) else { continue };
        let der = a[0].as_bytes();
        let ad = x509_parser::parse_x509_certificate(der)
            .ok()
            .and_then(|(_, c)| c.subject().iter_common_name().next().and_then(|x| x.as_str().ok()).map(str::to_string))
            .unwrap_or_default();
        let (seri, imza_icin) = cades::parse_cert_summary(der).map(|o| (o.serial_decimal, o.key_usage_ok)).unwrap_or_default();
        v.push(KartSertifikasi { ad, seri, imza_icin });
    }
    s.closeSession();
    Ok(v)
}

/// PIN ile açılmış kart oturumu; `Imzalayici` olarak kullanılır. Düşünce oturum kapanır (PIN bellekte tutulmaz).
pub struct KartImzalayici {
    _lib: PyKCS11Lib,
    oturum: Session,
    sertifika: Vec<u8>,
    anahtar: p::CK_ULONG,
    ecdsa: bool,
}

impl KartImzalayici {
    /// `seri`: birden çok sertifika varsa hangisi (onluk seri no). PIN hatalıysa `PinHatali`.
    pub fn ac(pin: &str, seri: Option<&str>) -> Result<Self, ImzaHatasi> {
        let l = kutuphane()?;
        let s = oturum(&l)?;
        if let Err(e) = s.login_user(pin) {
            s.closeSession();
            // CKR_PIN_INCORRECT 0xA0, CKR_PIN_LOCKED 0xA4
            let m = e.to_string();
            return Err(if m.contains("0xa4") || m.contains("0xA4") || m.to_lowercase().contains("locked") {
                ImzaHatasi::PinHatali("Kart kilitlenmiş (PIN çok kez yanlış girilmiş). PUK ile açılması gerekir.".into())
            } else {
                ImzaHatasi::PinHatali("PIN hatalı. Dikkat: üst üste yanlış girişte kart kilitlenir.".into())
            });
        }
        let sertifikalar = s
            .findObjects(&[(p::CKA_CLASS, TemplateVal::Ulong(p::CKO_CERTIFICATE))])
            .map_err(|e| ImzaHatasi::Diger(format!("sertifika araması başarısız: {e}")))?;
        let mut secilen: Option<(Vec<u8>, Vec<u8>)> = None;
        let mut ilk: Option<(Vec<u8>, Vec<u8>)> = None;
        for c in sertifikalar {
            let a = s.getAttributeValue(c, &[p::CKA_VALUE, p::CKA_ID]).map_err(|e| ImzaHatasi::Diger(format!("sertifika okunamadı: {e}")))?;
            let (der, id) = (a[0].as_bytes().to_vec(), a[1].as_bytes().to_vec());
            let ozet = cades::parse_cert_summary(&der).ok();
            ilk.get_or_insert_with(|| (der.clone(), id.clone()));
            if let Some(sr) = seri {
                if ozet.as_ref().map(|o| o.serial_decimal.as_str()) != Some(sr) {
                    continue;
                }
            }
            if ozet.as_ref().is_none_or(|o| o.key_usage_ok) {
                secilen = Some((der, id));
                break;
            }
        }
        let (sertifika, id) = secilen.or(ilk).ok_or_else(|| ImzaHatasi::Diger("Kartta sertifika yok".into()))?;
        let mut anahtarlar = s
            .findObjects(&[(p::CKA_CLASS, TemplateVal::Ulong(p::CKO_PRIVATE_KEY)), (p::CKA_ID, TemplateVal::Bytes(id))])
            .unwrap_or_default();
        if anahtarlar.is_empty() {
            anahtarlar = s.findObjects(&[(p::CKA_CLASS, TemplateVal::Ulong(p::CKO_PRIVATE_KEY))]).unwrap_or_default();
        }
        let anahtar = *anahtarlar.first().ok_or_else(|| ImzaHatasi::Diger("Kartta özel anahtar yok".into()))?;
        let tur = match s.getAttributeValue(anahtar, &[p::CKA_KEY_TYPE]).ok().and_then(|v| v.into_iter().next()) {
            Some(AttrValue::Ulong(Some(t))) => t,
            _ => p::CKK_RSA,
        };
        Ok(KartImzalayici { _lib: l, oturum: s, sertifika, anahtar, ecdsa: tur == p::CKK_EC })
    }
}

impl Imzalayici for KartImzalayici {
    fn sertifika(&mut self) -> Result<Vec<u8>, ImzaHatasi> {
        Ok(self.sertifika.clone())
    }

    fn imzala(&mut self, imzalanacak: &[u8]) -> Result<(Vec<u8>, bool), ImzaHatasi> {
        let h = Sha256::digest(imzalanacak);
        let (veri, mek) = if self.ecdsa {
            (h.to_vec(), Mechanism::new(p::CKM_ECDSA, None))
        } else {
            // DigestInfo(SHA-256) + özet: CKM_RSA_PKCS bunu PKCS#1 v1.5 dolgusuyla imzalar
            let mut v = vec![0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20];
            v.extend_from_slice(&h);
            (v, Mechanism::new(p::CKM_RSA_PKCS, None))
        };
        let imza = self.oturum.sign(self.anahtar, &veri, &mek).map_err(|e| ImzaHatasi::Diger(format!("imzalama başarısız: {e}")))?;
        Ok((imza, self.ecdsa))
    }
}

impl Drop for KartImzalayici {
    fn drop(&mut self) {
        self.oturum.logout();
        self.oturum.closeSession();
    }
}
