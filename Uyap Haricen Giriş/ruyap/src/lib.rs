//! UYAP Çekirdek ve Uygulama Paketi (`ruyap`)
//! ------------------------------------------
//! `C:\Users\WİN10\Desktop\UYAP\cokluuyap\Uyap Haricen Giriş` Python kod tabanının
//! 1:1 Rust karşılığı.
//!
//! `uyap_core/__init__.py` dışa aktarımları (`uyap_proxy`, `p2p_wire`, `office_agent`,
//! `jobs`, `run_office`, `run_client`, `job_manager`, `job_type`,
//! `registered_types`) ve tüm alt modüller burada tanımlanır.

#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(unused_imports)]

/// `rustls 0.23` için süreç düzeyinde varsayılan `ring` CryptoProvider kurar
/// (birden fazla TLS bağımlılığı `ring` ve `aws-lc-rs` özelliklerini birlikte açtığında paniği önler).
pub fn ensure_rustls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Windows MinGW varsayılan 1–2 MB worker stack'inin `webrtc`/`rustls`/`axum`
/// derin async future'larında taşmasını (`STATUS_STACK_OVERFLOW`) önlemek için
/// 8 MB worker stack boyutlu Tokio runtime oluşturur.
pub fn build_tokio_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()
}

pub mod runtime;
pub use runtime as _runtime;

// @crypto-udf modülleri
pub mod akis_pkcs11;
pub mod cades;
pub mod udf_converter;
pub mod belge_disk;
pub mod belge_veri;
pub mod udf_alan;
pub mod udf_pdf;
pub mod paylas_win;
pub mod pdf_birlestir;
pub mod py_cevir;
pub mod tiff_pdf;
pub mod udf_signer;
pub mod uyap_giris_disaridan;
pub mod yerel_giris;
pub mod yerel_kapi;
pub mod yerel_yardimci;

// @proxy modülleri
pub mod hata_kodlari;
pub mod sifreli_gunluk;
pub mod surum;
pub mod guncelleme_imza;
pub mod guncelleme;
pub mod yazi_sunucu;
#[cfg(windows)]
pub mod tepsi;
pub mod is_listesi;
pub mod is_otomatik;
pub mod icra_tebligatlar;
pub mod icra_dosyalar;
pub mod icra_kesinlestirme;
pub mod icra_listeler;
pub mod icra_mernis;
pub mod uyap_proxy;
// Büro veritabanı (ofis bilgisayarında gömülü PostgreSQL) — 2026-11-15
pub mod veritabani;
pub mod icra_veri;
pub mod icra_tebligat;
pub mod icra_kayit;
pub mod icra_tarama;
pub mod icra_durum;
pub mod icra_api;
pub mod is_veri;
pub mod icra_aktar;
pub mod is_aktar;
pub mod is_api;
pub mod is_isaret;
pub mod kimlik;

// @p2p-net modülleri
pub mod lan_tls;
pub mod office_agent;
pub mod relay_sifre;
pub mod p2p_wire;
pub mod signaling;
pub mod turn_dogrula;

// @jobs-domain modülleri
pub mod dosya_kaydet;
pub mod esas_no;
pub mod evrak_toplu;
pub mod gerekceli_karar;
pub mod icra_guncelle;
pub mod ipotek;
pub mod job_handlers;
pub mod jobs;
pub mod mts;
pub mod ptt_sorgu;
pub mod sira_eslestirme;
pub mod sira_kapisi;

// @gui-app modülleri
pub mod kurucu;
pub mod msi_hazirla;
pub mod paket_hazirla;
pub mod sira_kapisi_arayuz;
pub mod uyap_app;
pub mod uyap_theme;

// @vendor-zs modülleri
pub mod accounts;
pub mod vendor_server;
pub mod zs;
pub mod zs_bildirim;
pub mod zs_denetim;
pub mod zs_push;
pub mod zs_r2;
pub mod zs_rapor;
pub mod zs_store;
pub mod zs_tg_grup;

/// `uyap_core` ad alanı uyumluluk modülü (`use crate::uyap_core::*` veya `ruyap::uyap_core::*` için).
pub mod uyap_core {
    pub use crate::akis_pkcs11;
    pub use crate::cades;
    pub use crate::dosya_kaydet;
    pub use crate::esas_no;
    pub use crate::evrak_toplu;
    pub use crate::gerekceli_karar;
    pub use crate::ipotek;
    pub use crate::is_listesi;
    pub use crate::job_handlers;
    pub use crate::jobs;
    pub use crate::lan_tls;
    pub use crate::mts;
    pub use crate::office_agent;
    pub use crate::p2p_wire;
    pub use crate::ptt_sorgu;
    pub use crate::runtime;
    pub use crate::runtime as _runtime;
    pub use crate::sira_eslestirme;
    pub use crate::sira_kapisi;
    pub use crate::udf_converter;
    pub use crate::udf_pdf;
    pub use crate::udf_signer;
    pub use crate::uyap_giris_disaridan;
    pub use crate::uyap_proxy;
    pub use crate::yerel_giris;
    pub use crate::yerel_kapi;
}
