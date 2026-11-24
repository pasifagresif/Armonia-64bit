//! Güncelleme imzası (2026-11-23, kullanıcı isteği: "güncelleme iki taraflı anahtarla indirilip kurulsun").
//!
//! İşletim sisteminden bağımsızdır (Windows ve Armonia Linux'ta aynı): yalnız p256 / pbkdf2 / aes-gcm.
//!
//! **Sürüm dosyası** (cokluuyap.com/guncelleme/test.json):
//! ```json
//! { "bicim": 1, "surum": "26.11.23.2", "kanal": "test", "tarih": "2026-11-23T19:00:00+03:00",
//!   "zorunlu": false, "notlar": "…",
//!   "paketler": { "windows-x64": { "url": "https://www.cokluuyap.com/…msi", "boyut": 7340032, "sha256": "…" } },
//!   "imza": "<base64, 64 bayt ECDSA P-256 / SHA-256>" }
//! ```
//! İmza `imza` dışındaki BÜTÜN alanları kapsar: önek `cokluuyap-guncelleme-v1\n` + anahtarları sıralı, boşluksuz
//! JSON (`kanonik`). Paketin sha256'sı imzalı olduğundan paketin tek baytı değişse kurulmaz.
//!
//! **Anahtarlar:** gizli anahtar yalnız yayıncının bilgisayarında, parolayla şifreli dosyada durur (PBKDF2-SHA256
//! 600 000 tur → AES-256-GCM). Açık anahtarlar programa gömülür (`ACIK_ANAHTARLAR`: asıl + yedek); sürüm dosyası
//! bunlardan biriyle doğrulanırsa geçerlidir. Asıl anahtar kaybolur / çalınırsa yedekle imzalanmış bir sürüm yeni
//! anahtar listesini taşır.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use rand::RngCore;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const ONEK: &[u8] = b"cokluuyap-guncelleme-v1\n";
pub const BICIM: u64 = 1;
const ANAHTAR_TURU: &str = "cokluuyap-guncelleme-anahtar-v1";
const KDF_TUR: u32 = 600_000;

/// Programa gömülü açık anahtarlar (SEC1 sıkıştırılmış, base64). Anahtar üretilince buraya yazılır
/// (`guncelleme_arac anahtar-uret` çıktısı). Boşken hiçbir sürüm dosyası geçerli sayılmaz.
pub const ACIK_ANAHTARLAR: &[(&str, &str)] = &[
    // 2026-11-23, gizli anahtar: yayıncının bilgisayarında CokluUyap-Gizli/guncelleme-asil.json (parolalı).
    ("asil", "ArCaZP5g52gZ3mSIu+XhdADjXtuyczheibIuuVMNaR55"),
    // Yedek: çevrim dışı (USB) saklanır; asıl anahtar kaybolur ya da çalınırsa yeni anahtar listesini bununla imzala.
    ("yedek", "A31Oah3SQHK7Uzkowq6BmENxO44oY8N5HQN/0VFJFvjC"),
];

/// Anahtarları sıralı, boşluksuz JSON. serde_json'un nesne sırası derleme özelliğine bağlı olabildiğinden
/// sıralama burada elle yapılır; dizeler serde_json ile kaçışlanır.
pub fn kanonik(v: &Value) -> String {
    let mut s = String::new();
    yaz(v, &mut s);
    s
}

fn yaz(v: &Value, s: &mut String) {
    match v {
        Value::Object(m) => {
            let mut k: Vec<&String> = m.keys().collect();
            k.sort();
            s.push('{');
            for (i, a) in k.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&serde_json::to_string(a).unwrap_or_default());
                s.push(':');
                yaz(&m[*a], s);
            }
            s.push('}');
        }
        Value::Array(a) => {
            s.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                yaz(x, s);
            }
            s.push(']');
        }
        _ => s.push_str(&serde_json::to_string(v).unwrap_or_default()),
    }
}

/// İmzalanan baytlar: önek + `imza` alanı çıkarılmış kanonik JSON.
pub fn imzalanan(v: &Value) -> Result<Vec<u8>, String> {
    let mut m: Map<String, Value> = v.as_object().cloned().ok_or("Sürüm dosyası bir JSON nesnesi değil.")?;
    m.remove("imza");
    let mut b = ONEK.to_vec();
    b.extend_from_slice(kanonik(&Value::Object(m)).as_bytes());
    Ok(b)
}

/// Sürüm dosyasını imzalar (`imza` alanını yazar).
pub fn imzala(v: &mut Value, gizli: &SigningKey) -> Result<(), String> {
    let b = imzalanan(v)?;
    let imza: Signature = gizli.sign(&b);
    v.as_object_mut().ok_or("Sürüm dosyası bir JSON nesnesi değil.")?
        .insert("imza".into(), Value::String(B64.encode(imza.to_bytes())));
    Ok(())
}

/// İmzayı verilen açık anahtarlardan biriyle doğrular; doğrulayan anahtarın adını döner.
pub fn dogrula_anahtarlarla(v: &Value, anahtarlar: &[(&str, &str)]) -> Result<String, String> {
    if anahtarlar.is_empty() {
        return Err("Programda güncelleme anahtarı yok; güncelleme doğrulanamaz.".into());
    }
    let imza_b64 = v.get("imza").and_then(Value::as_str).ok_or("Sürüm dosyasında imza yok.")?;
    let imza_b = B64.decode(imza_b64).map_err(|_| "İmza çözülemedi.")?;
    let imza = Signature::from_slice(&imza_b).map_err(|_| "İmza biçimi geçersiz.")?;
    let b = imzalanan(v)?;
    for (ad, acik) in anahtarlar {
        let Ok(ab) = B64.decode(acik) else { continue };
        let Ok(vk) = VerifyingKey::from_sec1_bytes(&ab) else { continue };
        if vk.verify(&b, &imza).is_ok() {
            return Ok((*ad).to_string());
        }
    }
    Err("İmza geçersiz: sürüm dosyası bizim anahtarımızla imzalanmamış ya da değiştirilmiş.".into())
}

pub fn dogrula(v: &Value) -> Result<String, String> {
    dogrula_anahtarlarla(v, ACIK_ANAHTARLAR)
}

pub fn acik_anahtar_b64(gizli: &SigningKey) -> String {
    B64.encode(VerifyingKey::from(gizli).to_encoded_point(true).as_bytes())
}

pub fn yeni_anahtar() -> SigningKey {
    SigningKey::random(&mut rand::rngs::OsRng)
}

/// Gizli anahtarı parolayla şifreleyip anahtar dosyasının içeriğini (JSON) üretir.
pub fn anahtar_dosyasi(gizli: &SigningKey, parola: &str) -> Result<String, String> {
    anahtar_dosyasi_tur(gizli, parola, KDF_TUR)
}

fn anahtar_dosyasi_tur(gizli: &SigningKey, parola: &str, tur: u32) -> Result<String, String> {
    if parola.chars().count() < 12 {
        return Err("Parola en az 12 karakter olmalı.".into());
    }
    let mut tuz = [0u8; 16];
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut tuz);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let sifre = sifreleyici(parola, &tuz, tur);
    let sifreli = sifre
        .encrypt(Nonce::from_slice(&nonce), gizli.to_bytes().as_slice())
        .map_err(|_| "Anahtar şifrelenemedi.")?;
    let v = serde_json::json!({
        "tur": ANAHTAR_TURU, "kdf": "pbkdf2-sha256", "tekrar": tur,
        "tuz": B64.encode(tuz), "nonce": B64.encode(nonce), "sifreli": B64.encode(sifreli),
        "acik": acik_anahtar_b64(gizli),
    });
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// Anahtar dosyasını parolayla açar.
pub fn anahtar_ac(icerik: &str, parola: &str) -> Result<SigningKey, String> {
    let v: Value = serde_json::from_str(icerik).map_err(|_| "Anahtar dosyası okunamadı.")?;
    if v.get("tur").and_then(Value::as_str) != Some(ANAHTAR_TURU) {
        return Err("Bu bir güncelleme anahtarı dosyası değil.".into());
    }
    let al = |k: &str| -> Result<Vec<u8>, String> {
        B64.decode(v.get(k).and_then(Value::as_str).ok_or(format!("Anahtar dosyasında {k} yok."))?)
            .map_err(|_| format!("Anahtar dosyasında {k} bozuk."))
    };
    let tekrar = v.get("tekrar").and_then(Value::as_u64).ok_or("Anahtar dosyasında tekrar yok.")? as u32;
    let (tuz, nonce, sifreli) = (al("tuz")?, al("nonce")?, al("sifreli")?);
    if nonce.len() != 12 {
        return Err("Anahtar dosyası bozuk.".into());
    }
    let ham = sifreleyici(parola, &tuz, tekrar)
        .decrypt(Nonce::from_slice(&nonce), sifreli.as_slice())
        .map_err(|_| "Parola yanlış ya da anahtar dosyası bozuk.")?;
    let gizli = SigningKey::from_slice(&ham).map_err(|_| "Anahtar bozuk.")?;
    if v.get("acik").and_then(Value::as_str) != Some(acik_anahtar_b64(&gizli).as_str()) {
        return Err("Anahtar dosyasındaki açık anahtar gizli anahtarla uyuşmuyor.".into());
    }
    Ok(gizli)
}

fn sifreleyici(parola: &str, tuz: &[u8], tekrar: u32) -> Aes256Gcm {
    let mut k = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(parola.as_bytes(), tuz, tekrar, &mut k);
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&k))
}

pub fn sha256_hex(veri: &[u8]) -> String {
    hex::encode(Sha256::digest(veri))
}

/// Bu derlemenin paket adı (sürüm dosyasındaki `paketler` anahtarı).
pub fn platform() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        "windows-x64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "linux-arm64"
    } else {
        "bilinmiyor"
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn ornek() -> Value {
        serde_json::json!({
            "bicim": 1, "surum": "26.11.23.2", "kanal": "test", "tarih": "2026-11-23T19:00:00+03:00",
            "zorunlu": false, "notlar": "Deneme: ğüşiöç",
            "paketler": { "windows-x64": { "url": "https://www.cokluuyap.com/indir/a.msi", "boyut": 3, "sha256": sha256_hex(b"abc") } }
        })
    }

    #[test]
    fn imza_dogru_ve_degisiklik_reddedilir() {
        let g = yeni_anahtar();
        let acik = acik_anahtar_b64(&g);
        let anahtarlar = [("asil", acik.as_str())];
        let mut v = ornek();
        imzala(&mut v, &g).unwrap();
        assert_eq!(dogrula_anahtarlarla(&v, &anahtarlar).unwrap(), "asil");

        // Alan sırası imzayı etkilemez (kanonik).
        let yeniden: Value = serde_json::from_str(&serde_json::to_string_pretty(&v).unwrap()).unwrap();
        assert!(dogrula_anahtarlarla(&yeniden, &anahtarlar).is_ok());

        for (yol, deger) in [("/surum", "26.11.23.3"), ("/notlar", "x"), ("/paketler/windows-x64/sha256", "00"),
                             ("/paketler/windows-x64/url", "https://kotu.example/a.msi")] {
            let mut d = v.clone();
            *d.pointer_mut(yol).unwrap() = Value::String(deger.into());
            assert!(dogrula_anahtarlarla(&d, &anahtarlar).is_err(), "{yol} değişikliği kabul edildi");
        }
        let mut d = v.clone();
        d["zorunlu"] = Value::Bool(true);
        assert!(dogrula_anahtarlarla(&d, &anahtarlar).is_err());
        let mut d = v.clone();
        d["ek"] = Value::from(1);
        assert!(dogrula_anahtarlarla(&d, &anahtarlar).is_err());

        // Başka anahtar, boş liste, imzasız dosya.
        let baska = acik_anahtar_b64(&yeni_anahtar());
        assert!(dogrula_anahtarlarla(&v, &[("baska", baska.as_str())]).is_err());
        assert!(dogrula_anahtarlarla(&v, &[]).is_err());
        assert!(dogrula_anahtarlarla(&ornek(), &anahtarlar).is_err());
        // Yedek anahtarla imzalanan da geçer.
        let mut y = ornek();
        let yg = yeni_anahtar();
        imzala(&mut y, &yg).unwrap();
        let ya = acik_anahtar_b64(&yg);
        assert_eq!(dogrula_anahtarlarla(&y, &[("asil", acik.as_str()), ("yedek", ya.as_str())]).unwrap(), "yedek");
    }

    #[test]
    fn anahtar_dosyasi_parolayla_acilir() {
        let g = yeni_anahtar();
        // Testte az tur (gerçek dosya 600 000 tur).
        let d = anahtar_dosyasi_tur(&g, "uzun bir deneme parolası", 1000).unwrap();
        assert!(!d.contains(&hex::encode(g.to_bytes())));
        let a = anahtar_ac(&d, "uzun bir deneme parolası").unwrap();
        assert_eq!(a.to_bytes(), g.to_bytes());
        assert!(anahtar_ac(&d, "yanlış parola yanlış").is_err());
        assert!(anahtar_dosyasi_tur(&g, "kisa", 1000).is_err());
    }

    #[test]
    fn kanonik_sirali() {
        let v: Value = serde_json::from_str(r#"{"b":1,"a":{"d":[1,"x"],"c":"ş\"q"}}"#).unwrap();
        assert_eq!(kanonik(&v), r#"{"a":{"c":"ş\"q","d":[1,"x"]},"b":1}"#);
    }
}
