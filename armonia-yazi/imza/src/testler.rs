//! İmza testleri: yazılım anahtarıyla (rcgen ECDSA P-256 öz imzalı sertifika) gerçek CAdES imza üretilir ve
//! doğrulanır; gerçek kart aynı `Imzalayici` arayüzünden geçer. Claude, 2026-11-23.

use std::io::{Cursor, Read, Write};

use base64::Engine as _;
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;

use crate::gunluk::{Gunluk, Kayit};
use crate::hizmet::{isle, Cagiran, Kart};
use crate::udf_imza::{udf_dogrula, udf_imzala, ImzaHatasi, Imzalayici};

struct YaziliAnahtar {
    sertifika: Vec<u8>,
    pkcs8: Vec<u8>,
    ham: bool,
}

fn anahtar(ad: &str, ham: bool) -> YaziliAnahtar {
    let k = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let mut p = rcgen::CertificateParams::new(vec![]).unwrap();
    p.distinguished_name.push(rcgen::DnType::CommonName, ad);
    p.distinguished_name.push(rcgen::DnType::OrganizationName, "Deneme SM");
    let c = p.self_signed(&k).unwrap();
    YaziliAnahtar { sertifika: c.der().to_vec(), pkcs8: k.serialize_der(), ham }
}

impl Imzalayici for YaziliAnahtar {
    fn sertifika(&mut self) -> Result<Vec<u8>, ImzaHatasi> {
        Ok(self.sertifika.clone())
    }
    fn imzala(&mut self, v: &[u8]) -> Result<(Vec<u8>, bool), ImzaHatasi> {
        // ham: kartların döndürdüğü r||s biçimi (DER'e çevrilmeli)
        let alg = if self.ham { &ECDSA_P256_SHA256_FIXED_SIGNING } else { &ECDSA_P256_SHA256_ASN1_SIGNING };
        let rng = SystemRandom::new();
        let k = EcdsaKeyPair::from_pkcs8(alg, &self.pkcs8, &rng).unwrap();
        Ok((k.sign(&rng, v).unwrap().as_ref().to_vec(), true))
    }
}

fn udf(icerik: &str, imzali: bool) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let s = SimpleFileOptions::default();
    z.start_file("content.xml", s).unwrap();
    z.write_all(icerik.as_bytes()).unwrap();
    z.start_file("documentproperties.xml", s).unwrap();
    z.write_all(b"<p/>").unwrap();
    if imzali {
        z.start_file("sign.sgn", s).unwrap();
        z.write_all(b"eski imza").unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn dosya(udf: &[u8], ad: &str) -> Option<Vec<u8>> {
    let mut z = zip::ZipArchive::new(Cursor::new(udf)).unwrap();
    let mut f = z.by_name(ad).ok()?;
    let mut v = Vec::new();
    f.read_to_end(&mut v).unwrap();
    Some(v)
}

const ICERIK: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?> \n\n<template format_id=\"1.8\" ><content><![CDATA[Dilekçe\n]]></content></template>\n";

#[test]
fn imzala_ve_dogrula() {
    let mut a = anahtar("Av. Deneme Kişi", false);
    let s = udf_imzala(&udf(ICERIK, true), &mut a, None).unwrap();
    assert!(s.bilgi.belge_degismemis && s.bilgi.imza_gecerli && s.bilgi.sure_icinde);
    assert_eq!(s.bilgi.imzalayan, "Av. Deneme Kişi");
    assert!(!s.bilgi.zincir_dogrulandi);
    assert!(s.bilgi.imza_zamani.as_deref().is_some_and(|z| z.ends_with("UTC")));
    // İçerik ve öteki dosyalar korunur, eski imza değişir
    assert_eq!(dosya(&s.udf, "content.xml").unwrap(), ICERIK.as_bytes());
    assert_eq!(dosya(&s.udf, "documentproperties.xml").unwrap(), b"<p/>");
    assert_ne!(dosya(&s.udf, "sign.sgn").unwrap(), b"eski imza");
    assert_eq!(udf_dogrula(&s.udf).unwrap().unwrap(), s.bilgi);
    assert!(udf_dogrula(&udf(ICERIK, false)).unwrap().is_none());
}

#[test]
fn kart_bicimi_ecdsa_imzasi() {
    // Kartlar ECDSA imzasını r||s döndürür; DER'e çevrilip doğrulanmalı
    let mut a = anahtar("Av. Ham", true);
    let s = udf_imzala(&udf(ICERIK, false), &mut a, None).unwrap();
    assert!(s.bilgi.imza_gecerli);
}

#[test]
fn degistirilen_belge_yakalanir() {
    let mut a = anahtar("Av. Deneme", false);
    let s = udf_imzala(&udf(ICERIK, false), &mut a, None).unwrap();
    // İmzalı UDF'te content.xml değiştirilir, imza aynı kalır
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    z.start_file("content.xml", o).unwrap();
    z.write_all(ICERIK.replace("Dilekçe", "Değişmiş").as_bytes()).unwrap();
    z.start_file("sign.sgn", o).unwrap();
    z.write_all(&dosya(&s.udf, "sign.sgn").unwrap()).unwrap();
    let sahte = z.finish().unwrap().into_inner();
    let b = udf_dogrula(&sahte).unwrap().unwrap();
    assert!(!b.belge_degismemis, "değişen belge yakalanmalı");
    assert!(b.imza_gecerli, "imzalı niteliklerin kendisi sağlam");
}

struct YanlisAnahtar(YaziliAnahtar, YaziliAnahtar);

impl Imzalayici for YanlisAnahtar {
    fn sertifika(&mut self) -> Result<Vec<u8>, ImzaHatasi> {
        self.0.sertifika()
    }
    fn imzala(&mut self, v: &[u8]) -> Result<(Vec<u8>, bool), ImzaHatasi> {
        self.1.imzala(v)
    }
}

#[test]
fn yanlis_anahtarla_imza_reddedilir() {
    let mut y = YanlisAnahtar(anahtar("A", false), anahtar("B", false));
    let e = udf_imzala(&udf(ICERIK, false), &mut y, None).err().unwrap();
    assert!(matches!(e, ImzaHatasi::Diger(m) if m.contains("doğrulanamadı")));
}

#[test]
fn bozuk_girdiler() {
    let mut a = anahtar("A", false);
    assert!(matches!(udf_imzala(b"udf degil", &mut a, None), Err(ImzaHatasi::Belge(_))));
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("baska.xml", SimpleFileOptions::default()).unwrap();
    let icsiz = z.finish().unwrap().into_inner();
    assert!(matches!(udf_imzala(&icsiz, &mut a, None), Err(ImzaHatasi::Belge(_))));
    // Bozuk sign.sgn: hata, çökme değil
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    z.start_file("content.xml", o).unwrap();
    z.write_all(b"<a/>").unwrap();
    z.start_file("sign.sgn", o).unwrap();
    z.write_all(&[0x30, 0x82, 0xFF, 0xFF, 0x01]).unwrap();
    assert!(udf_dogrula(&z.finish().unwrap().into_inner()).is_err());
}

fn gecici(ad: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("armonia-imza-test-{}-{ad}", std::process::id()));
    let _ = std::fs::remove_file(&d);
    d
}

#[test]
fn gunluk_zinciri() {
    let yol = gecici("zincir.jsonl");
    let g = Gunluk::yeni(&yol);
    for i in 0..3 {
        g.ekle(Kayit { zaman: format!("2026-11-23T10:0{i}:00Z"), islem: "imzala".into(), sonuc: "basarili".into(), ..Default::default() })
            .unwrap();
    }
    assert_eq!(g.dogrula().unwrap().0, 3);
    let metin = std::fs::read_to_string(&yol).unwrap();
    let satirlar: Vec<&str> = metin.lines().collect();
    // Ortadaki kaydın sonucu değiştirilir
    std::fs::write(&yol, format!("{}\n{}\n{}\n", satirlar[0], satirlar[1].replace("basarili", "pin_hatali"), satirlar[2])).unwrap();
    assert!(g.dogrula().unwrap_err().contains("kayıt 2"));
    // Bir kayıt silinir
    std::fs::write(&yol, format!("{}\n{}\n", satirlar[0], satirlar[2])).unwrap();
    assert!(g.dogrula().unwrap_err().contains("silinmiş"));
    let _ = std::fs::remove_file(&yol);
}

struct DenemeKart;

impl Kart for DenemeKart {
    fn durum(&self) -> Value {
        json!({ "takili": true })
    }
    fn udf_imzala(&self, udf: &[u8], pin: &str, _s: Option<&str>) -> Result<Vec<u8>, ImzaHatasi> {
        if pin == "123456" {
            crate::hizmet::imzalayici_ile(udf, &mut anahtar("Av. Kart Sahibi", true))
        } else {
            Err(ImzaHatasi::PinHatali("PIN hatalı".into()))
        }
    }
}

/// Belgeyi değiştirip imzalayan (kötü / bozuk) arka uç: hizmet reddetmeli.
struct DegistirenKart;

impl Kart for DegistirenKart {
    fn durum(&self) -> Value {
        json!({})
    }
    fn udf_imzala(&self, _udf: &[u8], _pin: &str, _s: Option<&str>) -> Result<Vec<u8>, ImzaHatasi> {
        crate::hizmet::imzalayici_ile(&udf(&ICERIK.replace("Dilekçe", "Başka metin"), false), &mut anahtar("X", false))
    }
}

#[test]
fn icerigi_degistiren_arka_uc_reddedilir() {
    let yol = gecici("degistiren.jsonl");
    let g = Gunluk::yeni(&yol);
    let b64 = base64::engine::general_purpose::STANDARD.encode(udf(ICERIK, false));
    let y = isle(json!({ "islem": "imzala", "dosya_adi": "a.udf", "udf": b64, "pin": "1" }).to_string().as_bytes(), &Cagiran::default(), &DegistirenKart, &g);
    assert_eq!(y["tamam"], false);
    assert!(y["hata"].as_str().unwrap().contains("farklı"), "{y}");
    assert!(std::fs::read_to_string(&yol).unwrap().contains("hata:"));
    let _ = std::fs::remove_file(&yol);
}

/// Adalet istemcisi gibi davranan yerel sunucu: getCertificates (parçalı yanıt) ve signUdf.
fn sahte_adalet(istek_sayisi: usize) -> String {
    use std::io::{BufRead, BufReader};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let adres = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for s in l.incoming().take(istek_sayisi) {
            let mut s = s.unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut ilk = String::new();
            r.read_line(&mut ilk).unwrap();
            let mut uz = 0usize;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h == "\r\n" {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    uz = v.trim().parse().unwrap();
                }
            }
            let mut govde = vec![0u8; uz];
            r.read_exact(&mut govde).unwrap();
            let yanit = if ilk.contains("getCertificates") {
                let j = json!({"metadata":{"STATUS":"SUCCESS"},"data":[{"terminal":"ACS ACR38","ready":true,"certificates":[
                    {"certificateId":"c1","subject":"Av. Adalet Deneme","issuer":"Kamu SM","notBefore":"2025-01-01","notAfter":"2028-01-01"}]}]}).to_string();
                // Parçalı (chunked) yanıt: iki parça
                let (a, b) = j.split_at(10);
                format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n", a.len(), b.len())
            } else {
                let istek: Value = serde_json::from_slice(&govde).unwrap();
                let j = if istek["pin"] == "4321" && istek["certificateId"] == "c1" {
                    let gelen = base64::engine::general_purpose::STANDARD.decode(istek["content"].as_str().unwrap()).unwrap();
                    let imzali = crate::hizmet::imzalayici_ile(&gelen, &mut anahtar("Av. Adalet Deneme", true)).unwrap();
                    json!({"metadata":{"STATUS":"SUCCESS"},"data":{"content": base64::engine::general_purpose::STANDARD.encode(imzali),"imzaGecerliMi":true}})
                } else {
                    json!({"metadata":{"STATUS":"FAIL","MESSAGE":"PIN hatalı girildi"}})
                };
                let j = j.to_string();
                format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{j}", j.len())
            };
            s.write_all(yanit.as_bytes()).unwrap();
        }
    });
    adres
}

#[test]
fn adalet_istemcisiyle_imza() {
    let a = crate::adalet::Adalet { adres: sahte_adalet(4) };
    let s = a.sertifikalar().unwrap();
    assert_eq!((s.len(), s[0].kimlik.as_str(), s[0].ad.as_str(), s[0].okuyucu.as_str()), (1, "c1", "Av. Adalet Deneme", "ACS ACR38"));
    let belge = udf(ICERIK, false);
    assert!(matches!(a.udf_imzala(&belge, "0000", Some("c1")), Err(ImzaHatasi::PinHatali(_))));
    // Kimlik verilmeyince tek sertifika seçilir (getCertificates + signUdf)
    let imzali = a.udf_imzala(&belge, "4321", None).unwrap();
    let b = udf_dogrula(&imzali).unwrap().unwrap();
    assert!(b.imza_gecerli && b.belge_degismemis);
    assert_eq!(b.imzalayan, "Av. Adalet Deneme");
    // İstemci kapalı: anlaşılır hata
    let kapali = crate::adalet::Adalet { adres: "127.0.0.1:1".into() };
    assert!(matches!(kapali.sertifikalar(), Err(ImzaHatasi::KartYok(_))));
}

#[test]
fn hizmet_imzalar_ve_gunluge_yazar() {
    let yol = gecici("hizmet.jsonl");
    let g = Gunluk::yeni(&yol);
    let c = Cagiran { uid: Some(1000), pid: Some(42), program: Some("/opt/armonia/yazi/armonia-yazi".into()) };
    let b64 = base64::engine::general_purpose::STANDARD.encode(udf(ICERIK, false));
    let istek = |pin: &str| json!({ "islem": "imzala", "dosya_adi": "dilekce.udf", "udf": b64, "pin": pin }).to_string();

    let y = isle(istek("000000").as_bytes(), &c, &DenemeKart, &g);
    assert_eq!((y["tamam"].as_bool(), y["tur"].as_str()), (Some(false), Some("pin_hatali")));
    assert!(y.get("udf").is_none());

    let y = isle(istek("123456").as_bytes(), &c, &DenemeKart, &g);
    assert_eq!(y["tamam"], true, "{y}");
    assert_eq!(y["bilgi"]["imzalayan"], "Av. Kart Sahibi");
    let imzali = base64::engine::general_purpose::STANDARD.decode(y["udf"].as_str().unwrap()).unwrap();
    assert!(udf_dogrula(&imzali).unwrap().unwrap().imza_gecerli);

    // Günlük: iki kayıt; PIN ve belge metni yazılmamış
    let metin = std::fs::read_to_string(&yol).unwrap();
    assert!(!metin.contains("123456") && !metin.contains("Dilekçe"));
    let kayitlar: Vec<Kayit> = metin.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(kayitlar.len(), 2);
    assert_eq!((kayitlar[0].sonuc.as_str(), kayitlar[1].sonuc.as_str()), ("pin_hatali", "basarili"));
    assert_eq!(kayitlar[1].uid, Some(1000));
    assert_eq!(kayitlar[1].program.as_deref(), Some("/opt/armonia/yazi/armonia-yazi"));
    assert_eq!(kayitlar[1].imzalayan.as_deref(), Some("Av. Kart Sahibi"));
    assert_eq!(kayitlar[0].belge_ozeti, kayitlar[1].belge_ozeti);
    let y = isle(br#"{"islem":"gunluk_dogrula"}"#, &c, &DenemeKart, &g);
    assert_eq!(y["kayit"], 2);

    // Günlüğe yazılamazsa imza verilmez
    let kotu = Gunluk::yeni(std::env::temp_dir().join("olmayan-klasor-armonia").join("g.jsonl"));
    let y = isle(istek("123456").as_bytes(), &c, &DenemeKart, &kotu);
    assert_eq!(y["tamam"], false);
    assert!(y.get("udf").is_none());

    assert_eq!(isle(b"{bozuk", &c, &DenemeKart, &g)["tamam"], false);
    let _ = std::fs::remove_file(&yol);
}
