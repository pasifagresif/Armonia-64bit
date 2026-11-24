//! Açık anahtarlar, imza doğrulama ve iptal zinciri.
//!
//! İmzalanan ileti = alan ayırıcı önek || dosyanın ham baytları. Önek, aynı anahtarla imzalanmış başka bir şeyin
//! (ör. iptal bildirimi) güncelleme bilgisi yerine geçmesini önler. Doğrulama ring ED25519 (RFC 8032; S < L).

use crate::{dosya, hata, Sonuc};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ring::signature::{Ed25519KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const ONEK_GUNCELLEME: &[u8] = b"armonia-guncelleme-v1\n";
pub const ONEK_IPTAL: &[u8] = b"armonia-iptal-v1\n";
/// Bir imza dosyasının azami boyutu (JSON: kimlik + 64 bayt imzanın base64'ü)
pub const IMZA_AZAMI: u64 = 512;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AcikAnahtar(pub [u8; 32]);

impl AcikAnahtar {
    pub fn b64_coz(s: &str) -> Sonuc<AcikAnahtar> {
        let v = B64.decode(s.trim()).map_err(|_| hata!("açık anahtar base64 değil"))?;
        let a: [u8; 32] = v.try_into().map_err(|_| hata!("açık anahtar 32 bayt değil"))?;
        Ok(AcikAnahtar(a))
    }
    pub fn b64(&self) -> String {
        B64.encode(self.0)
    }
    /// sha256(açık anahtar)'ın ilk 8 baytı, onaltılık
    pub fn kimlik(&self) -> String {
        crate::ozet_hex(&self.0)[..16].to_string()
    }
    pub fn dogrula(&self, onek: &[u8], veri: &[u8], imza: &[u8]) -> bool {
        let mut ileti = Vec::with_capacity(onek.len() + veri.len());
        ileti.extend_from_slice(onek);
        ileti.extend_from_slice(veri);
        UnparsedPublicKey::new(&ED25519, &self.0).verify(&ileti, imza).is_ok()
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ImzaDosyasi {
    pub anahtar_kimligi: String,
    pub imza: String,
}

impl ImzaDosyasi {
    pub fn coz(bayt: &[u8]) -> Sonuc<(String, Vec<u8>)> {
        if bayt.len() as u64 > IMZA_AZAMI {
            return Err(hata!("imza dosyası çok büyük"));
        }
        let d: ImzaDosyasi = serde_json::from_slice(bayt).map_err(|e| hata!("imza dosyası bozuk: {e}"))?;
        let i = B64.decode(&d.imza).map_err(|_| hata!("imza base64 değil"))?;
        if i.len() != 64 {
            return Err(hata!("imza 64 bayt değil"));
        }
        Ok((d.anahtar_kimligi, i))
    }
}

/// İmzalar (yalnız yayıncı makinesinde; gizli anahtar `gizli` modülüyle çözülür).
pub fn imzala(pkcs8: &[u8], onek: &[u8], veri: &[u8]) -> Sonuc<(AcikAnahtar, String)> {
    let k = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|_| hata!("gizli anahtar geçersiz"))?;
    use ring::signature::KeyPair;
    let acik = AcikAnahtar(k.public_key().as_ref().try_into().map_err(|_| hata!("açık anahtar"))?);
    let mut ileti = onek.to_vec();
    ileti.extend_from_slice(veri);
    let imza = k.sign(&ileti);
    let d = ImzaDosyasi { anahtar_kimligi: acik.kimlik(), imza: B64.encode(imza.as_ref()) };
    Ok((acik, serde_json::to_string(&d).expect("json") + "\n"))
}

/// İmajdaki anahtar dosyası (`etkin <b64>` ve `yedek <b64>` satırları) + iptal zinciri sonucu.
#[derive(Clone, Debug)]
pub struct Anahtarlar {
    pub etkin: AcikAnahtar,
    pub yedek: AcikAnahtar,
    /// İptal edilmiş eski etkin anahtarların kimlikleri
    pub iptal_edilen: Vec<String>,
    /// Ön taahhüt: bir sonraki etkin anahtarın sha256(açık anahtar) özeti. İptal bildirimi YALNIZ bu özetle tutan
    /// anahtarı etkin yapabilir; yedek anahtar tek başına keyfi bir anahtarı etkinleştiremez (denetçi 24.11 O-1).
    pub sonraki: String,
}

impl Anahtarlar {
    pub fn metinden(metin: &str) -> Sonuc<Anahtarlar> {
        let (mut etkin, mut yedek, mut sonraki) = (None, None, None);
        for satir in metin.lines() {
            let s = satir.trim();
            if s.is_empty() || s.starts_with('#') {
                continue;
            }
            match s.split_once(' ') {
                Some(("etkin", b)) if etkin.is_none() => etkin = Some(AcikAnahtar::b64_coz(b)?),
                Some(("yedek", b)) if yedek.is_none() => yedek = Some(AcikAnahtar::b64_coz(b)?),
                Some(("sonraki", o)) if sonraki.is_none() && crate::hex64_mi(o.trim()) => sonraki = Some(o.trim().to_string()),
                _ => return Err(hata!("anahtar dosyasında beklenmeyen satır")),
            }
        }
        let (etkin, yedek) = (etkin.ok_or_else(|| hata!("etkin anahtar yok"))?, yedek.ok_or_else(|| hata!("yedek anahtar yok"))?);
        let sonraki = sonraki.ok_or_else(|| hata!("sonraki anahtar taahhüdü yok"))?;
        if etkin == yedek {
            return Err(hata!("etkin ve yedek anahtar aynı olamaz"));
        }
        if sonraki == crate::ozet_hex(&etkin.0) || sonraki == crate::ozet_hex(&yedek.0) {
            return Err(hata!("sonraki anahtar etkin ya da yedekle aynı olamaz"));
        }
        Ok(Anahtarlar { etkin, yedek, iptal_edilen: Vec::new(), sonraki })
    }

    /// İmaj anahtar dosyasını ve (varsa) iptal kaydını yükler. İptal kaydındaki her bildirim yeniden doğrulanır.
    pub fn yukle(anahtar_dosyasi: &Path, iptal_dosyasi: Option<&Path>) -> Sonuc<Anahtarlar> {
        let m = dosya::oku_sinirli(anahtar_dosyasi, 4096)?;
        let mut a = Anahtarlar::metinden(std::str::from_utf8(&m).map_err(|_| hata!("anahtar dosyası UTF-8 değil"))?)?;
        if let Some(y) = iptal_dosyasi {
            if let Some(k) = dosya::oku_varsa(y, 64 * 1024)? {
                let kayit: Vec<IptalKaydi> = serde_json::from_slice(&k).map_err(|e| hata!("iptal kaydı bozuk: {e}"))?;
                for i in &kayit {
                    a.iptal_uygula(i)?;
                }
            }
        }
        Ok(a)
    }

    /// Yedek anahtarla imzalı iptal bildirimini doğrular ve uygular: etkin anahtar kalıcı reddedilir.
    pub fn iptal_uygula(&mut self, k: &IptalKaydi) -> Sonuc<()> {
        let bildirim = B64.decode(&k.bildirim).map_err(|_| hata!("iptal bildirimi base64 değil"))?;
        let (kimlik, imza) = ImzaDosyasi::coz(k.imza.as_bytes())?;
        if kimlik != self.yedek.kimlik() || !self.yedek.dogrula(ONEK_IPTAL, &bildirim, &imza) {
            return Err(hata!("iptal bildiriminin imzası geçersiz (yalnız yedek anahtar imzalayabilir)"));
        }
        let b: IptalBildirimi = serde_json::from_slice(&bildirim).map_err(|e| hata!("iptal bildirimi bozuk: {e}"))?;
        if b.islem != "iptal" || b.urun != crate::URUN {
            return Err(hata!("iptal bildirimi bu ürün için değil"));
        }
        if b.anahtar_kimligi != self.etkin.kimlik() {
            return Err(hata!("iptal bildirimi şu anki etkin anahtarı hedeflemiyor"));
        }
        let yeni = AcikAnahtar::b64_coz(&b.yeni_etkin)?;
        if yeni == self.yedek || yeni == self.etkin || self.iptal_edilen.contains(&yeni.kimlik()) {
            return Err(hata!("iptal bildirimindeki yeni anahtar kabul edilemez"));
        }
        if crate::ozet_hex(&yeni.0) != self.sonraki {
            return Err(hata!("iptal bildirimindeki yeni anahtar önceden taahhüt edilmemiş"));
        }
        if !crate::hex64_mi(&b.sonraki) || b.sonraki == crate::ozet_hex(&yeni.0) || b.sonraki == crate::ozet_hex(&self.yedek.0) {
            return Err(hata!("iptal bildirimindeki yeni taahhüt geçersiz"));
        }
        self.iptal_edilen.push(self.etkin.kimlik());
        self.etkin = yeni;
        self.sonraki = b.sonraki;
        Ok(())
    }

    /// Güncelleme bilgisinin imzasını etkin anahtarla doğrular.
    pub fn bilgi_imzasi_gecerli(&self, veri: &[u8], imza_dosyasi: &[u8]) -> Sonuc<()> {
        let (kimlik, imza) = ImzaDosyasi::coz(imza_dosyasi)?;
        if self.iptal_edilen.contains(&kimlik) {
            return Err(hata!("imzalayan anahtar iptal edilmiş ({kimlik})"));
        }
        if kimlik != self.etkin.kimlik() {
            return Err(hata!("bilinmeyen imza anahtarı ({kimlik})"));
        }
        if !self.etkin.dogrula(ONEK_GUNCELLEME, veri, &imza) {
            return Err(hata!("imza geçersiz"));
        }
        Ok(())
    }
}

/// Yedek anahtarın imzaladığı bildirim (JSON).
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct IptalBildirimi {
    pub islem: String,
    pub urun: String,
    pub anahtar_kimligi: String,
    pub yeni_etkin: String,
    /// Bir sonraki iptal için yeni ön taahhüt (sha256(açık anahtar), onaltılık)
    pub sonraki: String,
}

/// Kalıcı iptal kaydındaki bir girdi: bildirimin ham baytları (base64) + imza dosyası metni.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct IptalKaydi {
    pub bildirim: String,
    pub imza: String,
}
