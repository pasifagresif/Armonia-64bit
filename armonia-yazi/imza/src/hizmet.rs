//! İmza hizmetinin istek işleyicisi (taşıyıcıdan bağımsız; soket kısmı bin/armonia-imza.rs).
//!
//! İstek: tek satır JSON. Yanıt: tek satır JSON (`tamam`, hata ise `hata`).
//!   {"islem":"durum"}                                     → kart takılı mı, sertifikalar
//!   {"islem":"imzala","dosya_adi":"..","udf":"<b64>","pin":"..","seri":null} → {"udf":"<b64>","bilgi":{..}}
//!   {"islem":"dogrula","udf":"<b64>"}                     → {"bilgi":{..} | null}
//!   {"islem":"gunluk_dogrula"}                            → {"kayit":n,"son_ozet":".."}
//! Her imzala isteği (sonucu ne olursa olsun) günlüğe yazılır; PIN ve belgenin kendisi hiçbir yere yazılmaz.
//! Günlüğe yazılamazsa imza verilmez (kayıtsız imza olmasın). Claude, 2026-11-23.

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::gunluk::{Gunluk, Kayit};
use crate::udf_imza::{belge_ozeti, onaltilik, udf_dogrula, udf_imzala, ImzaHatasi, Imzalayici};

/// Bir istekte en çok bu kadar bayt (base64 dahil).
pub const AZAMI_ISTEK: usize = 96 << 20;

#[derive(Deserialize)]
#[serde(tag = "islem", rename_all = "snake_case")]
enum Istek {
    Durum,
    Imzala {
        dosya_adi: String,
        udf: String,
        pin: String,
        #[serde(alias = "sertifika")]
        seri: Option<String>,
    },
    Dogrula { udf: String },
    GunlukDogrula,
}

/// İsteği gönderen süreç (soketten çekirdeğin bildirdiği kimlik; istemcinin beyanı değil).
#[derive(Debug, Clone, Default)]
pub struct Cagiran {
    pub uid: Option<u32>,
    pub pid: Option<u32>,
    pub program: Option<String>,
}

/// Kart erişimi (gerçekte kart.rs; testte yazılım anahtarı).
/// Kart erişimi. İki arka uç: Adalet E-imza istemcisi (Armonia'da varsayılan; adalet.rs) ve PKCS#11 sürücüsü
/// (kart.rs). Dönen imzalı UDF hizmette ayrıca doğrulanır; arka uca körü körüne güvenilmez.
pub trait Kart {
    fn durum(&self) -> Value;
    /// UDF'i imzalar. `sertifika`: Adalet'te sertifika kimliği, PKCS#11'de seri numarası.
    fn udf_imzala(&self, udf: &[u8], pin: &str, sertifika: Option<&str>) -> Result<Vec<u8>, ImzaHatasi>;
}

/// `Imzalayici` açabilen bir arka ucu (PKCS#11, test anahtarı) `Kart` olarak kullanmak için.
pub fn imzalayici_ile(udf: &[u8], im: &mut dyn Imzalayici) -> Result<Vec<u8>, ImzaHatasi> {
    Ok(udf_imzala(udf, im, None)?.udf)
}

fn hata(m: impl std::fmt::Display) -> Value {
    json!({ "tamam": false, "hata": m.to_string() })
}

fn b64(s: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|_| "belge (base64) bozuk".to_string())
}

pub fn isle(istek: &[u8], cagiran: &Cagiran, kart: &dyn Kart, gunluk: &Gunluk) -> Value {
    if istek.len() > AZAMI_ISTEK {
        return hata("istek çok büyük");
    }
    let istek: Istek = match serde_json::from_slice(istek) {
        Ok(i) => i,
        Err(e) => return hata(format!("istek anlaşılamadı: {e}")),
    };
    match istek {
        Istek::Durum => json!({ "tamam": true, "kart": kart.durum() }),
        Istek::GunlukDogrula => match gunluk.dogrula() {
            Ok((n, son)) => json!({ "tamam": true, "kayit": n, "son_ozet": son }),
            Err(e) => hata(format!("günlük zinciri bozuk: {e}")),
        },
        Istek::Dogrula { udf } => {
            let udf = match b64(&udf) {
                Ok(v) => v,
                Err(e) => return hata(e),
            };
            match udf_dogrula(&udf) {
                Ok(b) => json!({ "tamam": true, "bilgi": b }),
                Err(e) => hata(e),
            }
        }
        Istek::Imzala { dosya_adi, udf, pin, seri } => {
            let mut kayit = Kayit {
                zaman: chrono::Utc::now().to_rfc3339(),
                islem: "imzala".into(),
                uid: cagiran.uid,
                pid: cagiran.pid,
                program: cagiran.program.clone(),
                belge_adi: Some(dosya_adi.chars().take(200).collect()),
                ..Default::default()
            };
            let sonuc = (|| -> Result<(Vec<u8>, crate::udf_imza::ImzaBilgisi), ImzaHatasi> {
                let udf = b64(&udf).map_err(ImzaHatasi::Belge)?;
                kayit.belge_ozeti = Some(belge_ozeti(&udf)?);
                let imzali = kart.udf_imzala(&udf, &pin, seri.as_deref())?;
                // Arka ucun döndürdüğünü doğrula: imza geçerli, belge imzadan sonra değişmemiş ve içerik bizim
                // gönderdiğimizle aynı (arka uç belgeyi değiştirip imzalamış olmasın)
                let bilgi = udf_dogrula(&imzali)?.ok_or_else(|| ImzaHatasi::Diger("imzalı belgede imza yok".into()))?;
                if !(bilgi.imza_gecerli && bilgi.belge_degismemis) {
                    return Err(ImzaHatasi::Diger("dönen imza doğrulanamadı".into()));
                }
                if belge_ozeti(&imzali)? != *kayit.belge_ozeti.as_ref().unwrap_or(&String::new()) {
                    return Err(ImzaHatasi::Diger("imzalanan belgenin içeriği gönderilenden farklı; imza reddedildi".into()));
                }
                Ok((imzali, bilgi))
            })();
            drop(pin);
            match &sonuc {
                Ok((imzali, bilgi)) => {
                    kayit.imzali_ozet = Some(onaltilik(&Sha256::digest(imzali)));
                    kayit.imzalayan = Some(bilgi.imzalayan.clone());
                    kayit.sertifika_seri = Some(bilgi.seri.clone());
                    kayit.sonuc = "basarili".into();
                }
                Err(ImzaHatasi::PinHatali(_)) => kayit.sonuc = "pin_hatali".into(),
                Err(ImzaHatasi::KartYok(_)) => kayit.sonuc = "kart_yok".into(),
                Err(e) => kayit.sonuc = format!("hata: {e}"),
            }
            // Kayıtsız imza olmasın: günlüğe yazılamazsa imzalı belge verilmez
            if let Err(e) = gunluk.ekle(kayit) {
                return hata(format!("imza günlüğüne yazılamadı, imza verilmedi: {e}"));
            }
            match sonuc {
                Ok((imzali, bilgi)) => json!({
                    "tamam": true,
                    "udf": base64::engine::general_purpose::STANDARD.encode(imzali),
                    "bilgi": bilgi,
                }),
                Err(e) => {
                    let tur = match e {
                        ImzaHatasi::PinHatali(_) => "pin_hatali",
                        ImzaHatasi::KartYok(_) => "kart_yok",
                        ImzaHatasi::Belge(_) => "belge",
                        ImzaHatasi::Diger(_) => "diger",
                    };
                    json!({ "tamam": false, "hata": e.to_string(), "tur": tur })
                }
            }
        }
    }
}
