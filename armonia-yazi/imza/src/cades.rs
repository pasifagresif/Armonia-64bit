//! Kaynak: ruyap/src/cades.rs (UYAP sign.sgn profiliyle doğrulanmış). Armonia Yazı imza paketine kopyalandı, 23.11.2026;
//! ruyap çekirdeğe bağlanınca tek kopya kalacak.
//!
//! `cades.rs` — UYAP UDF imzası için saf-Rust CAdES-BES (detached CMS) oluşturucu.
//!
//! Gerçek UYAP `sign.sgn`'inden tersine mühendislikle doğrulanan profil:
//!   - CMS SignedData v1, DER, DETACHED (eContent yok)
//!   - digestAlgorithm: SHA-256
//!   - signedAttrs: contentType, signingTime, messageDigest, signingCertificateV2
//!   - signatureAlgorithm: RSA (sha256WithRSAEncryption) — karta göre ECDSA da olabilir
//!   - sertifikalar: yalnızca imzalayan

use chrono::{DateTime, Timelike, Utc};
use sha2::{Digest, Sha256};

// ----------------------------------------------------------------------------
// Minimal DER kodlayıcı
// ----------------------------------------------------------------------------

pub fn _der_len(n: usize) -> Vec<u8> {
    if n < 0x80 {
        return vec![n as u8];
    }
    let bytes = n.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len() - 1);
    let b = &bytes[start..];
    let mut out = Vec::with_capacity(1 + b.len());
    out.push(0x80 | (b.len() as u8));
    out.extend_from_slice(b);
    out
}

pub fn _tlv(tag: u8, val: &[u8]) -> Vec<u8> {
    let len = _der_len(val.len());
    let mut out = Vec::with_capacity(1 + len.len() + val.len());
    out.push(tag);
    out.extend_from_slice(&len);
    out.extend_from_slice(val);
    out
}

pub fn seq(items: &[&[u8]]) -> Vec<u8> {
    let total: usize = items.iter().map(|i| i.len()).sum();
    let mut joined = Vec::with_capacity(total);
    for item in items {
        joined.extend_from_slice(item);
    }
    _tlv(0x30, &joined)
}

pub fn set_of(items: &[&[u8]]) -> Vec<u8> {
    let total: usize = items.iter().map(|i| i.len()).sum();
    let mut joined = Vec::with_capacity(total);
    for item in items {
        joined.extend_from_slice(item);
    }
    _tlv(0x31, &joined)
}

pub fn octet(b: &[u8]) -> Vec<u8> {
    _tlv(0x04, b)
}

pub fn integer(n: u64) -> Vec<u8> {
    if n == 0 {
        return _tlv(0x02, &[0x00]);
    }
    let bytes = n.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len() - 1);
    integer_be(&bytes[start..])
}

pub fn integer_be(raw_be: &[u8]) -> Vec<u8> {
    let trimmed = match raw_be.iter().position(|&b| b != 0) {
        Some(idx) => &raw_be[idx..],
        None => return _tlv(0x02, &[0x00]),
    };
    if (trimmed[0] & 0x80) != 0 {
        let mut buf = Vec::with_capacity(1 + trimmed.len());
        buf.push(0x00);
        buf.extend_from_slice(trimmed);
        _tlv(0x02, &buf)
    } else {
        _tlv(0x02, trimmed)
    }
}

pub fn explicit(tag_no: u8, content: &[u8]) -> Vec<u8> {
    _tlv(0xA0 | tag_no, content)
}

pub fn utctime(dt: DateTime<Utc>) -> Vec<u8> {
    let s = dt.format("%y%m%d%H%M%SZ").to_string();
    _tlv(0x17, s.as_bytes())
}

pub fn oid(dotted: &str) -> Vec<u8> {
    let parts: Vec<u64> = dotted
        .split('.')
        .filter_map(|x| x.parse::<u64>().ok())
        .collect();
    let mut head = Vec::new();
    if parts.len() >= 2 {
        head.push((40 * parts[0] + parts[1]) as u8);
        for &p_orig in &parts[2..] {
            let mut p = p_orig;
            if p < 0x80 {
                head.push(p as u8);
            } else {
                let mut stack = vec![(p & 0x7f) as u8];
                p >>= 7;
                while p > 0 {
                    stack.push(((p & 0x7f) as u8) | 0x80);
                    p >>= 7;
                }
                stack.reverse();
                head.extend_from_slice(&stack);
            }
        }
    }
    _tlv(0x06, &head)
}

// OID sabitleri
pub const OID_CONTENT_TYPE_STR: &str = "1.2.840.113549.1.9.3";
pub const OID_MESSAGE_DIGEST_STR: &str = "1.2.840.113549.1.9.4";
pub const OID_SIGNING_TIME_STR: &str = "1.2.840.113549.1.9.5";
pub const OID_PKCS7_DATA_STR: &str = "1.2.840.113549.1.7.1";
pub const OID_PKCS7_SIGNED_STR: &str = "1.2.840.113549.1.7.2";
pub const OID_SIGNING_CERT_V2_STR: &str = "1.2.840.113549.1.9.16.2.47";
pub const OID_SHA256_STR: &str = "2.16.840.1.101.3.4.2.1";
pub const OID_RSA_ENC_STR: &str = "1.2.840.113549.1.1.1";
pub const OID_SHA256_RSA_STR: &str = "1.2.840.113549.1.1.11";
pub const OID_ECDSA_SHA256_STR: &str = "1.2.840.10045.4.3.2";

pub fn algid(o: &[u8], params: Option<&[u8]>) -> Vec<u8> {
    match params {
        Some(p) => seq(&[o, p]),
        None => seq(&[o]),
    }
}

pub fn digest_algid() -> Vec<u8> {
    // SHA-256 digestAlgorithm: gerçek UYAP imzası parametreyi (NULL dahil) ATLIYOR.
    let o = oid(OID_SHA256_STR);
    seq(&[&o])
}

// ----------------------------------------------------------------------------
// Minimal X.509 DER ayrıştırıcı (issuer DER + serialNumber + KeyUsage)
// ----------------------------------------------------------------------------

pub fn read_tlv(data: &[u8]) -> Result<(u8, &[u8], &[u8], &[u8]), String> {
    if data.len() < 2 {
        return Err("Geçersiz DER: yetersiz bayt".to_string());
    }
    let tag = data[0];
    let len_byte = data[1];
    let (len, header_len) = if len_byte < 0x80 {
        (len_byte as usize, 2usize)
    } else {
        let num_bytes = (len_byte & 0x7f) as usize;
        if num_bytes == 0 || num_bytes > 4 || data.len() < 2 + num_bytes {
            return Err("Geçersiz DER uzunluğu".to_string());
        }
        let mut l: usize = 0;
        for &b in &data[2..2 + num_bytes] {
            l = (l << 8) | (b as usize);
        }
        (l, 2 + num_bytes)
    };
    let end = header_len.checked_add(len).ok_or("DER taşması")?;
    if data.len() < end {
        return Err("Geçersiz DER: kesik veri".to_string());
    }
    Ok((tag, &data[..end], &data[header_len..end], &data[end..]))
}

pub fn be_bytes_to_decimal(bytes: &[u8]) -> String {
    if bytes.is_empty() || bytes.iter().all(|&b| b == 0) {
        return "0".to_string();
    }
    let mut digits: Vec<u8> = vec![0];
    for &b in bytes {
        let mut carry = b as u32;
        for d in digits.iter_mut() {
            let val = (*d as u32) * 256 + carry;
            *d = (val % 10) as u8;
            carry = val / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    digits.iter().rev().map(|d| (b'0' + d) as char).collect()
}

pub struct CertSummary {
    pub issuer_der: Vec<u8>,
    pub serial_be: Vec<u8>,
    pub serial_decimal: String,
    pub key_usage_ok: bool,
}

/// Sertifika DER'inden (issuer_der_bytes, serial_be_bytes) döndürür.
pub fn cert_issuer_and_serial(cert_der: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let info = parse_cert_summary(cert_der)?;
    Ok((info.issuer_der, info.serial_be))
}

pub fn parse_cert_summary(cert_der: &[u8]) -> Result<CertSummary, String> {
    let (tag, _, cert_body, _) = read_tlv(cert_der)?;
    if tag != 0x30 {
        return Err("Sertifika SEQUENCE değil".to_string());
    }
    let (tbs_tag, _, mut tbs, _) = read_tlv(cert_body)?;
    if tbs_tag != 0x30 {
        return Err("tbsCertificate SEQUENCE değil".to_string());
    }

    // [0] EXPLICIT Version OPTIONAL
    if !tbs.is_empty() && tbs[0] == 0xA0 {
        let (_, _, _, rest) = read_tlv(tbs)?;
        tbs = rest;
    }

    // serialNumber INTEGER
    let (ser_tag, _, ser_val, rest) = read_tlv(tbs)?;
    if ser_tag != 0x02 {
        return Err("serialNumber INTEGER değil".to_string());
    }
    let serial_be = ser_val.to_vec();
    let serial_decimal = be_bytes_to_decimal(&serial_be);
    tbs = rest;

    // signature AlgorithmIdentifier
    let (_, _, _, rest) = read_tlv(tbs)?;
    tbs = rest;

    // issuer Name (full TLV)
    let (iss_tag, iss_full, _, mut rest) = read_tlv(tbs)?;
    if iss_tag != 0x30 {
        return Err("issuer SEQUENCE değil".to_string());
    }
    let issuer_der = iss_full.to_vec();

    // KeyUsage extension kontrolü ([3] EXPLICIT Extensions içinde OID 2.5.29.15 = 55 1D 0F)
    let mut key_usage_ok = true;
    while !rest.is_empty() {
        let (t, _, val, next) = read_tlv(rest)?;
        rest = next;
        if t == 0xA3 {
            if let Ok((0x30, _, mut exts, _)) = read_tlv(val) {
                while !exts.is_empty() {
                    if let Ok((0x30, _, mut ext, next_ext)) = read_tlv(exts) {
                        exts = next_ext;
                        if let Ok((0x06, _, ext_oid, ext_rest)) = read_tlv(ext) {
                            if ext_oid == [0x55, 0x1D, 0x0F] {
                                // KeyUsage bulundu
                                ext = ext_rest;
                                if !ext.is_empty() && ext[0] == 0x01 {
                                    if let Ok((_, _, _, r2)) = read_tlv(ext) {
                                        ext = r2;
                                    }
                                }
                                if let Ok((0x04, _, oct, _)) = read_tlv(ext) {
                                    if let Ok((0x03, _, bs, _)) = read_tlv(oct) {
                                        if bs.len() >= 2 {
                                            // Bit 0 = digitalSignature (0x80), Bit 1 = contentCommitment/nonRepudiation (0x40)
                                            let flags = bs[1];
                                            key_usage_ok = (flags & 0x80) != 0 || (flags & 0x40) != 0;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    }

    Ok(CertSummary {
        issuer_der,
        serial_be,
        serial_decimal,
        key_usage_ok,
    })
}

// ----------------------------------------------------------------------------
// SignedAttributes
// ----------------------------------------------------------------------------

fn _attr(attr_oid: &[u8], value_der: &[u8]) -> Vec<u8> {
    let s = set_of(&[value_der]);
    seq(&[attr_oid, &s])
}

pub fn build_signed_attributes(
    content: &[u8],
    cert_der: &[u8],
    signing_time: DateTime<Utc>,
) -> Result<Vec<u8>, String> {
    let digest = Sha256::digest(content);
    let cert_hash = Sha256::digest(cert_der);
    let (issuer_der, serial_be) = cert_issuer_and_serial(cert_der)?;

    let general_name = explicit(4, &issuer_der);
    let general_names = seq(&[&general_name]);
    let serial_int = integer_be(&serial_be);
    let issuer_serial = seq(&[&general_names, &serial_int]);

    let cert_hash_oct = octet(&cert_hash);
    let ess_cert_id = seq(&[&cert_hash_oct, &issuer_serial]);
    let inner_seq = seq(&[&ess_cert_id]);
    let signing_cert_v2 = seq(&[&inner_seq]);

    let a1 = _attr(&oid(OID_CONTENT_TYPE_STR), &oid(OID_PKCS7_DATA_STR));
    let a2 = _attr(&oid(OID_SIGNING_TIME_STR), &utctime(signing_time));
    let a3 = _attr(&oid(OID_MESSAGE_DIGEST_STR), &octet(&digest));
    let a4 = _attr(&oid(OID_SIGNING_CERT_V2_STR), &signing_cert_v2);

    // EKLEME SIRASINI koru (UYAP davranışı)
    Ok(set_of(&[&a1, &a2, &a3, &a4]))
}

// ----------------------------------------------------------------------------
// CMS SignedData montajı
// ----------------------------------------------------------------------------

pub fn build_cms(
    _content: &[u8],
    cert_der: &[u8],
    signed_attrs_der_set: &[u8],
    signature: &[u8],
    use_ecdsa: bool,
) -> Result<Vec<u8>, String> {
    if signed_attrs_der_set.is_empty() {
        return Err("signed_attrs boş".to_string());
    }
    // SignerInfo içindeki signedAttrs [0] IMPLICIT olur: ilk baytı 0x31 -> 0xA0
    let mut implicit_attrs = Vec::with_capacity(signed_attrs_der_set.len());
    implicit_attrs.push(0xA0);
    implicit_attrs.extend_from_slice(&signed_attrs_der_set[1..]);

    let (sid_issuer_der, serial_be) = cert_issuer_and_serial(cert_der)?;
    let ser_int = integer_be(&serial_be);
    let sid = seq(&[&sid_issuer_der, &ser_int]);

    let sig_algid = if use_ecdsa {
        algid(&oid(OID_ECDSA_SHA256_STR), None)
    } else {
        algid(&oid(OID_SHA256_RSA_STR), Some(b"\x05\x00"))
    };

    let v1 = integer(1);
    let da = digest_algid();
    let sig_oct = octet(signature);
    let signer_info = seq(&[&v1, &sid, &da, &implicit_attrs, &sig_algid, &sig_oct]);

    let encap_content_info = seq(&[&oid(OID_PKCS7_DATA_STR)]);
    let digest_algs = set_of(&[&da]);
    let certs = explicit(0, cert_der);
    let signer_infos = set_of(&[&signer_info]);

    let signed_data = seq(&[&v1, &digest_algs, &encap_content_info, &certs, &signer_infos]);
    let signed_oid = oid(OID_PKCS7_SIGNED_STR);
    let exp_sd = explicit(0, &signed_data);
    Ok(seq(&[&signed_oid, &exp_sd]))
}

pub fn to_be_signed(
    content: &[u8],
    cert_der: &[u8],
    signing_time: Option<DateTime<Utc>>,
) -> Result<(Vec<u8>, Vec<u8>, DateTime<Utc>), String> {
    let st = signing_time.unwrap_or_else(|| {
        Utc::now().with_nanosecond(0).unwrap_or_else(Utc::now)
    });
    let sa = build_signed_attributes(content, cert_der, st)?;
    Ok((sa.clone(), sa, st))
}
