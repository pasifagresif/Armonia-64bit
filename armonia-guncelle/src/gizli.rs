//! Gizli anahtar dosyası: Ed25519 PKCS#8, paroladan scrypt ile türetilen anahtarla ChaCha20-Poly1305.
//! Biçim: "ARMGZL01" (8) | tuz (16) | nonce (12) | şifreli PKCS#8 + etiket. Yalnız yayıncı makinesinde kullanılır.

use crate::{hata, Sonuc};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use ring::rand::{SecureRandom, SystemRandom};

const SIHIR: &[u8; 8] = b"ARMGZL01";
// scrypt: N = 2^17, r = 8, p = 1 (~128 MB, yaklaşık 0,5 sn)
const LOG_N: u8 = 17;

fn turet(parola: &[u8], tuz: &[u8], log_n: u8) -> Sonuc<[u8; 32]> {
    let p = scrypt::Params::new(log_n, 8, 1, 32).map_err(|_| hata!("scrypt parametresi"))?;
    let mut k = [0u8; 32];
    scrypt::scrypt(parola, tuz, &p, &mut k).map_err(|_| hata!("scrypt"))?;
    Ok(k)
}

/// Yeni anahtar çifti üretir; (şifreli dosya baytları, PKCS#8) döner.
pub fn uret(parola: &[u8]) -> Sonuc<(Vec<u8>, Vec<u8>)> {
    let rng = SystemRandom::new();
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| hata!("anahtar üretilemedi"))?;
    let dosya = sifrele(parola, pkcs8.as_ref(), LOG_N)?;
    Ok((dosya, pkcs8.as_ref().to_vec()))
}

pub fn sifrele(parola: &[u8], pkcs8: &[u8], log_n: u8) -> Sonuc<Vec<u8>> {
    if parola.len() < 12 {
        return Err(hata!("parola en az 12 karakter olmalı"));
    }
    let rng = SystemRandom::new();
    let mut tuz = [0u8; 16];
    let mut nonce = [0u8; 12];
    rng.fill(&mut tuz).map_err(|_| hata!("rastgele"))?;
    rng.fill(&mut nonce).map_err(|_| hata!("rastgele"))?;
    let k = turet(parola, &tuz, log_n)?;
    let c = ChaCha20Poly1305::new(Key::from_slice(&k));
    let s = c.encrypt(Nonce::from_slice(&nonce), pkcs8).map_err(|_| hata!("şifreleme"))?;
    let mut v = Vec::with_capacity(8 + 1 + 16 + 12 + s.len());
    v.extend_from_slice(SIHIR);
    v.push(log_n);
    v.extend_from_slice(&tuz);
    v.extend_from_slice(&nonce);
    v.extend_from_slice(&s);
    Ok(v)
}

pub fn coz(parola: &[u8], dosya: &[u8]) -> Sonuc<Vec<u8>> {
    if dosya.len() < 8 + 1 + 16 + 12 + 16 || &dosya[..8] != SIHIR {
        return Err(hata!("gizli anahtar dosyası tanınmadı"));
    }
    let log_n = dosya[8];
    if !(14..=20).contains(&log_n) {
        return Err(hata!("gizli anahtar dosyası: scrypt parametresi beklenmedik"));
    }
    let (tuz, kalan) = dosya[9..].split_at(16);
    let (nonce, s) = kalan.split_at(12);
    let k = turet(parola, tuz, log_n)?;
    let c = ChaCha20Poly1305::new(Key::from_slice(&k));
    c.decrypt(Nonce::from_slice(nonce), s).map_err(|_| hata!("parola yanlış ya da dosya bozuk"))
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn gidis_donus() {
        let rng = SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        // testte hızlı parametre
        let d = sifrele(b"uzun-bir-parola-123", pkcs8.as_ref(), 14).unwrap();
        assert_eq!(coz(b"uzun-bir-parola-123", &d).unwrap(), pkcs8.as_ref());
        assert!(coz(b"yanlis-parola-12345", &d).is_err());
        let mut bozuk = d.clone();
        let son = bozuk.len() - 1;
        bozuk[son] ^= 1;
        assert!(coz(b"uzun-bir-parola-123", &bozuk).is_err());
        assert!(sifrele(b"kisa", pkcs8.as_ref(), 14).is_err());
    }
}
