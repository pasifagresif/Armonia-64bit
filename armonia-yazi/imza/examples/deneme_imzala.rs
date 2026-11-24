//! Geliştirme: UDF'i TEST anahtarıyla (öz imzalı, geçerliliği olmayan) imzalar; doğrulama ekranını denemek için.
//! cargo run -p imza --example deneme_imzala -- <girdi.udf> <çıktı.udf>. Claude, 2026-11-24.
use imza::udf_imza::{udf_imzala, ImzaHatasi, Imzalayici};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

struct Test(Vec<u8>, Vec<u8>);
impl Imzalayici for Test {
    fn sertifika(&mut self) -> Result<Vec<u8>, ImzaHatasi> { Ok(self.0.clone()) }
    fn imzala(&mut self, v: &[u8]) -> Result<(Vec<u8>, bool), ImzaHatasi> {
        let r = SystemRandom::new();
        let k = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &self.1, &r).unwrap();
        Ok((k.sign(&r, v).unwrap().as_ref().to_vec(), true))
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let mut p = rcgen::CertificateParams::new(vec![]).unwrap();
    p.distinguished_name.push(rcgen::DnType::CommonName, "DENEME Avukat (test anahtarı)");
    let c = p.self_signed(&k).unwrap();
    let s = udf_imzala(&std::fs::read(&a[1]).unwrap(), &mut Test(c.der().to_vec(), k.serialize_der()), None).unwrap();
    std::fs::write(&a[2], s.udf).unwrap();
    println!("imzalandı: {} ({})", a[2], s.bilgi.imzalayan);
}
