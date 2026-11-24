//! Armonia Yazı e-imza paketi (plan: docs/claude-notlar/planlar/armonia-belge-editoru.md, 6b).
//!
//! - `udf_imza`: UDF imzalama (UYAP sign.sgn, CAdES-BES) ve doğrulama
//! - `kart`: PKCS#11 kartla imzalama (AKİS vb.)
//! - `gunluk`: zincirli imza günlüğü
//! - `hizmet`: imza hizmetinin istek işleyicisi (soket: bin/armonia-imza.rs)
//!
//! Claude, 2026-11-23.

// ruyap'tan kopyalanan modüller kendi adlandırmalarıyla (PyKCS11 uyumlu adlar) kalır
#[allow(non_snake_case, non_camel_case_types, dead_code, clippy::all)]
pub mod cades;
#[allow(non_snake_case, non_camel_case_types, dead_code, clippy::all)]
pub mod pkcs11;

pub mod adalet;
pub mod gunluk;
pub mod hizmet;
pub mod kart;
pub mod udf_imza;

#[cfg(test)]
mod testler;
