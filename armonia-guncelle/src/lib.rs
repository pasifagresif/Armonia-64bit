//! Armonia: Çoklu UYAP'ı imzalı paketle güncelleme çekirdeği.
//! Plan ve tehdit modeli: docs/claude-notlar/planlar/cokluuyap-linux-guncelleme.md. Claude, 2026-11-24.
//!
//! Güven kaynağı Ed25519 imzasıdır (TLS değil). Gizli anahtarlar çevrimdışıdır; imajda iki açık anahtar ve bir ön
//! taahhüt vardır: `etkin` güncelleme bilgisini imzalar; `yedek` YALNIZ etkin anahtarı iptal eden bildirimi imzalar
//! ve yeni etkin anahtar ancak önceden taahhüt edilmiş (`sonraki` özeti) anahtar olabilir. Yedeğin tek başına
//! sızması güncelleme imzalatmaya yetmez; yedek + sonraki anahtarın birlikte sızması yeter.

pub mod ag;
pub mod agac;
pub mod anahtar;
pub mod bilgi;
pub mod dosya;
pub mod gizli;
pub mod kur;
pub mod paket;
pub mod sec;
pub mod surum;
pub mod zaman;

use std::fmt;

#[derive(Debug)]
pub struct Hata(pub String);

impl fmt::Display for Hata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Hata {}
impl From<std::io::Error> for Hata {
    fn from(e: std::io::Error) -> Self {
        Hata(format!("G/Ç: {e}"))
    }
}

pub type Sonuc<T> = Result<T, Hata>;

#[macro_export]
macro_rules! hata {
    ($($t:tt)*) => { $crate::Hata(format!($($t)*)) };
}

/// Ürün adı: başka ürün için imzalanmış bilgi kabul edilmez.
pub const URUN: &str = "cokluuyap-linux";
pub const KANAL: &str = "kararli";
pub const BICIM: u32 = 1;
/// Paketlerin indirildiği sabit kök. Bilgi dosyasında adres YOK, yalnız dosya adı var.
pub const KOK_ADRES: &str = "https://www.cokluuyap.com/indir/linux/";

/// sha256 → küçük harf onaltılık
pub fn ozet_hex(veri: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(veri))
}

pub fn hex64_mi(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
