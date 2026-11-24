//! UDF imzalama ve doğrulama.
//!
//! İmza UYAP'ın kabul ettiği biçimdedir: `sign.sgn` = `content.xml` baytları üzerine ayrık (detached) CAdES-BES
//! (cades.rs; ruyap'ta UYAP'ta denenmiş profil). İmzalayan bir `Imzalayici`dır: gerçekte kart (kart.rs), testte
//! yazılım anahtarı. Doğrulama: belge değişmiş mi (messageDigest = SHA-256(content.xml)), imza matematiksel olarak
//! geçerli mi (sertifikanın açık anahtarıyla), imzalayan kim. Sertifika zinciri ve iptal (Kamu SM kökü, SİL/OCSP)
//! **doğrulanmaz**; bunu ayrıca söyleriz. Claude, 2026-11-23.

use std::io::{Cursor, Read, Write};

use chrono::{DateTime, Timelike, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::cades;
use x509_parser::prelude::FromDer;

const AZAMI_GIRDI: usize = 64 << 20;
const AZAMI_ACILMIS: u64 = 32 << 20;
const AZAMI_DOSYA: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImzaHatasi {
    /// Kart takılı değil / okuyucu yok
    KartYok(String),
    /// PIN hatalı (kart kilitlenmeden önce kalan deneme kartta tutulur)
    PinHatali(String),
    /// Belge UDF değil ya da bozuk
    Belge(String),
    Diger(String),
}

impl std::fmt::Display for ImzaHatasi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImzaHatasi::KartYok(m) | ImzaHatasi::PinHatali(m) | ImzaHatasi::Belge(m) | ImzaHatasi::Diger(m) => f.write_str(m),
        }
    }
}

/// İmzayı atan taraf (kart ya da test anahtarı).
pub trait Imzalayici {
    /// İmzalayanın sertifikası (DER).
    fn sertifika(&mut self) -> Result<Vec<u8>, ImzaHatasi>;
    /// `signedAttrs` (DER SET) üzerine imza. Dönüş: (imza, ECDSA mı). ECDSA imzası DER ya da r||s olabilir.
    fn imzala(&mut self, imzalanacak: &[u8]) -> Result<(Vec<u8>, bool), ImzaHatasi>;
}

/// İmza ve imzalayan bilgisi (arayüzde gösterilir, günlüğe yazılır).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImzaBilgisi {
    /// Sertifikadaki ad (CN)
    pub imzalayan: String,
    /// Sertifikanın seri numarası (onluk)
    pub seri: String,
    /// Sertifikayı veren (CN)
    pub veren: String,
    pub gecerlilik_bas: String,
    pub gecerlilik_son: String,
    /// İmzadaki imza zamanı (signingTime, UTC)
    pub imza_zamani: Option<String>,
    /// content.xml imzadan sonra değişmemiş
    pub belge_degismemis: bool,
    /// İmza sertifikanın açık anahtarıyla doğrulandı
    pub imza_gecerli: bool,
    /// Sertifika zinciri / iptal durumu doğrulanmadı (her zaman false; Kamu SM kökü ve SİL ileride)
    pub zincir_dogrulandi: bool,
    /// Sertifika imza zamanında geçerlilik süresi içinde
    pub sure_icinde: bool,
}

/// UDF'in parçaları: content.xml ve öteki dosyalar (sign.sgn hariç), sırayla.
/// (content.xml, öteki dosyalar, sign.sgn)
type UdfParcalari = (Vec<u8>, Vec<(String, Vec<u8>)>, Option<Vec<u8>>);

fn udf_parcala(udf: &[u8]) -> Result<UdfParcalari, ImzaHatasi> {
    if udf.len() > AZAMI_GIRDI {
        return Err(ImzaHatasi::Belge("belge çok büyük".into()));
    }
    let mut z = ZipArchive::new(Cursor::new(udf)).map_err(|e| ImzaHatasi::Belge(format!("UDF açılamadı: {e}")))?;
    if z.len() > AZAMI_DOSYA {
        return Err(ImzaHatasi::Belge("UDF içinde çok fazla dosya".into()));
    }
    let (mut icerik, mut oteki, mut imza) = (None, Vec::new(), None);
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| ImzaHatasi::Belge(e.to_string()))?;
        if f.is_dir() {
            continue;
        }
        let ad = f.name().to_string();
        let mut v = Vec::new();
        (&mut f).take(AZAMI_ACILMIS + 1).read_to_end(&mut v).map_err(|e| ImzaHatasi::Belge(e.to_string()))?;
        if v.len() as u64 > AZAMI_ACILMIS {
            return Err(ImzaHatasi::Belge(format!("{ad} çok büyük")));
        }
        match ad.as_str() {
            "content.xml" => icerik = Some(v),
            "sign.sgn" => imza = Some(v),
            _ => oteki.push((ad, v)),
        }
    }
    Ok((icerik.ok_or_else(|| ImzaHatasi::Belge("UDF içinde content.xml yok".into()))?, oteki, imza))
}

fn p1363_der(sig: &[u8]) -> Vec<u8> {
    let n = sig.len() / 2;
    cades::seq(&[&cades::integer_be(&sig[..n]), &cades::integer_be(&sig[n..])])
}

/// ECDSA imzası DER SEQUENCE değilse (kartlar r||s döndürür) DER'e çevrilir.
fn ecdsa_der(sig: Vec<u8>) -> Vec<u8> {
    let der_mi = cades::read_tlv(&sig).is_ok_and(|(t, tam, _, kalan)| t == 0x30 && tam.len() == sig.len() && kalan.is_empty());
    if der_mi || !sig.len().is_multiple_of(2) {
        sig
    } else {
        p1363_der(&sig)
    }
}

pub struct ImzaSonucu {
    /// İmzalı UDF (content.xml + öteki dosyalar + sign.sgn)
    pub udf: Vec<u8>,
    pub bilgi: ImzaBilgisi,
}

/// UDF'i imzalar. Var olan imza değiştirilir (UYAP tek imza tutar). `zaman` yalnız testte verilir.
pub fn udf_imzala(udf: &[u8], im: &mut dyn Imzalayici, zaman: Option<DateTime<Utc>>) -> Result<ImzaSonucu, ImzaHatasi> {
    let (icerik, oteki, _) = udf_parcala(udf)?;
    let sertifika = im.sertifika()?;
    let t = zaman.unwrap_or_else(|| Utc::now().with_nanosecond(0).unwrap_or_else(Utc::now));
    let nitelikler = cades::build_signed_attributes(&icerik, &sertifika, t).map_err(ImzaHatasi::Diger)?;
    let (imza, ecdsa) = im.imzala(&nitelikler)?;
    let imza = if ecdsa { ecdsa_der(imza) } else { imza };
    let sgn = cades::build_cms(&icerik, &sertifika, &nitelikler, &imza, ecdsa).map_err(ImzaHatasi::Diger)?;

    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    let sec = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut yaz = |ad: &str, v: &[u8]| -> Result<(), ImzaHatasi> {
        z.start_file(ad, sec).map_err(|e| ImzaHatasi::Diger(e.to_string()))?;
        z.write_all(v).map_err(|e| ImzaHatasi::Diger(e.to_string()))
    };
    yaz("content.xml", &icerik)?;
    for (ad, v) in &oteki {
        yaz(ad, v)?;
    }
    yaz("sign.sgn", &sgn)?;
    let cikti = z.finish().map_err(|e| ImzaHatasi::Diger(e.to_string()))?.into_inner();
    let bilgi = udf_dogrula(&cikti)?.ok_or_else(|| ImzaHatasi::Diger("imza yazılamadı".into()))?;
    if !(bilgi.belge_degismemis && bilgi.imza_gecerli) {
        return Err(ImzaHatasi::Diger("üretilen imza doğrulanamadı (kart yanlış anahtarla imzalamış olabilir)".into()));
    }
    Ok(ImzaSonucu { udf: cikti, bilgi })
}

// ── Doğrulama ───────────────────────────────────────────────────────────────────────────────────────

const OID_MESAJ_OZETI: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x04];
const OID_IMZA_ZAMANI: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x05];

struct CmsParcalari<'a> {
    sertifika: &'a [u8],
    /// signedAttrs, [0] IMPLICIT etiketiyle (A0); doğrulamada SET (31) olarak imzalanır
    nitelikler: &'a [u8],
    imza: &'a [u8],
}

/// (etiket, tam TLV, değer, kalan)
type Tlv<'a> = (u8, &'a [u8], &'a [u8], &'a [u8]);

fn tlv(d: &[u8]) -> Result<Tlv<'_>, ImzaHatasi> {
    cades::read_tlv(d).map_err(|e| ImzaHatasi::Belge(format!("imza (sign.sgn) okunamadı: {e}")))
}

fn cms_ayristir(sgn: &[u8]) -> Result<CmsParcalari<'_>, ImzaHatasi> {
    // ContentInfo SEQ { OID signedData, [0] { SignedData SEQ { ver, digestAlgs, encap, [0] certs, [1]? crls, signerInfos } } }
    let (_, _, ci, _) = tlv(sgn)?;
    let (_, _, _, ci) = tlv(ci)?; // OID
    let (_, _, sd_kap, _) = tlv(ci)?; // [0]
    let (_, _, mut sd, _) = tlv(sd_kap)?; // SignedData
    for _ in 0..3 {
        sd = tlv(sd)?.3; // version, digestAlgorithms, encapContentInfo
    }
    let mut sertifika: &[u8] = &[];
    let mut signer_infos: &[u8] = &[];
    while !sd.is_empty() {
        let (t, _, deger, kalan) = tlv(sd)?;
        match t {
            0xA0 => sertifika = tlv(deger)?.1, // ilk sertifika (tam TLV)
            0x31 => signer_infos = deger,
            _ => {}
        }
        sd = kalan;
    }
    let (_, _, si, _) = tlv(signer_infos)?; // ilk SignerInfo
    let mut si = si;
    for _ in 0..3 {
        si = tlv(si)?.3; // version, sid, digestAlgorithm
    }
    let (t, nitelikler, _, kalan) = tlv(si)?;
    if t != 0xA0 {
        return Err(ImzaHatasi::Belge("imzada imzalı nitelikler yok".into()));
    }
    let kalan = tlv(kalan)?.3; // signatureAlgorithm
    let (_, _, imza, _) = tlv(kalan)?;
    if sertifika.is_empty() {
        return Err(ImzaHatasi::Belge("imzada sertifika yok".into()));
    }
    Ok(CmsParcalari { sertifika, nitelikler, imza })
}

/// İmzalı nitelikler içinde OID'si verilen niteliğin ilk değeri (tam TLV).
fn nitelik<'a>(nitelikler_ic: &'a [u8], aranan: &[u8]) -> Option<&'a [u8]> {
    let mut d = nitelikler_ic;
    while !d.is_empty() {
        let (_, _, a, kalan) = cades::read_tlv(d).ok()?;
        let (_, _, o, deger) = cades::read_tlv(a).ok()?;
        if o == aranan {
            let (_, _, set_ic, _) = cades::read_tlv(deger).ok()?;
            return cades::read_tlv(set_ic).ok().map(|x| x.1);
        }
        d = kalan;
    }
    None
}

fn imza_zamani(tam: &[u8]) -> Option<String> {
    let (t, _, v, _) = cades::read_tlv(tam).ok()?;
    let s = std::str::from_utf8(v).ok()?;
    // UTCTime YYMMDDHHMMSSZ ya da GeneralizedTime YYYYMMDDHHMMSSZ
    let (yil, gerisi) = if t == 0x17 && s.len() >= 12 {
        let yy: i32 = s[..2].parse().ok()?;
        (if yy < 50 { 2000 + yy } else { 1900 + yy }, &s[2..])
    } else if t == 0x18 && s.len() >= 14 {
        (s[..4].parse().ok()?, &s[4..])
    } else {
        return None;
    };
    Some(format!("{yil:04}-{}-{} {}:{}:{} UTC", &gerisi[0..2], &gerisi[2..4], &gerisi[4..6], &gerisi[6..8], &gerisi[8..10]))
}

fn cn(ad: &x509_parser::x509::X509Name) -> String {
    ad.iter_common_name().next().and_then(|c| c.as_str().ok()).unwrap_or("").to_string()
}

/// İmzalı UDF'i doğrular. İmza yoksa `Ok(None)`.
pub fn udf_dogrula(udf: &[u8]) -> Result<Option<ImzaBilgisi>, ImzaHatasi> {
    let (icerik, _, imza) = udf_parcala(udf)?;
    let Some(sgn) = imza else { return Ok(None) };
    let p = cms_ayristir(&sgn)?;
    let (_, sert) = x509_parser::parse_x509_certificate(p.sertifika).map_err(|e| ImzaHatasi::Belge(format!("imzadaki sertifika okunamadı: {e}")))?;
    let nitelik_ic = tlv(p.nitelikler)?.2;
    let ozet = nitelik(nitelik_ic, OID_MESAJ_OZETI).and_then(|t| cades::read_tlv(t).ok()).map(|x| x.2.to_vec());
    let belge_degismemis = ozet.as_deref() == Some(Sha256::digest(&icerik).as_slice());
    // İmzalanan veri: nitelikler SET etiketiyle (31)
    let mut imzalanan = p.nitelikler.to_vec();
    imzalanan[0] = 0x31;
    let anahtar = sert.public_key();
    let anahtar_baytlari = anahtar.subject_public_key.data.as_ref();
    let alg = anahtar.algorithm.algorithm.to_id_string();
    let imza_gecerli = match alg.as_str() {
        "1.2.840.113549.1.1.1" => ring::signature::UnparsedPublicKey::new(&ring::signature::RSA_PKCS1_2048_8192_SHA256, anahtar_baytlari)
            .verify(&imzalanan, p.imza)
            .is_ok(),
        "1.2.840.10045.2.1" => {
            let egri = anahtar.algorithm.parameters.as_ref().and_then(|x| x.as_oid().ok()).map(|o| o.to_id_string()).unwrap_or_default();
            let a: &dyn ring::signature::VerificationAlgorithm = match egri.as_str() {
                "1.3.132.0.34" => &ring::signature::ECDSA_P384_SHA256_ASN1,
                _ => &ring::signature::ECDSA_P256_SHA256_ASN1,
            };
            ring::signature::UnparsedPublicKey::new(a, anahtar_baytlari).verify(&imzalanan, p.imza).is_ok()
        }
        _ => false,
    };
    let zaman = nitelik(nitelik_ic, OID_IMZA_ZAMANI).and_then(imza_zamani);
    let gecerlilik = sert.validity();
    let sure_icinde = nitelik(nitelik_ic, OID_IMZA_ZAMANI)
        .and_then(|t| x509_parser::time::ASN1Time::from_der(t).ok().map(|x| x.1))
        .map(|t| gecerlilik.not_before <= t && t <= gecerlilik.not_after)
        .unwrap_or(false);
    Ok(Some(ImzaBilgisi {
        imzalayan: cn(sert.subject()),
        seri: cades::be_bytes_to_decimal(sert.raw_serial()),
        veren: cn(sert.issuer()),
        gecerlilik_bas: gecerlilik.not_before.to_string(),
        gecerlilik_son: gecerlilik.not_after.to_string(),
        imza_zamani: zaman,
        belge_degismemis,
        imza_gecerli,
        zincir_dogrulandi: false,
        sure_icinde,
    }))
}

/// content.xml'in SHA-256 özeti (onaltılık): günlükte belgeyi tanımlar (belgenin kendisi günlüğe yazılmaz).
pub fn belge_ozeti(udf: &[u8]) -> Result<String, ImzaHatasi> {
    let (icerik, _, _) = udf_parcala(udf)?;
    Ok(onaltilik(&Sha256::digest(&icerik)))
}

pub fn onaltilik(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
