//! Armonia Yazı belge çekirdeği (plan: docs/claude-notlar/planlar/armonia-belge-editoru.md).
//!
//! Birinci aşama: belge modeli + UDF okuma/yazma. Sonraki aşamalar: DOCX, PDF üretimi (ruyap `udf_pdf.rs`
//! taşınacak), PDF'ten çeviri.
//!
//! Hem yerel (Armonia, Tauri) hem WebAssembly (cokluuyap.com) için derlenir; bu yüzden dosya sistemi, ağ ve iş
//! parçacığı kullanmaz: yalnız bayt alır, bayt verir. Claude, 2026-11-23.

pub mod model;
pub mod udf;

pub use model::*;

#[cfg(test)]
mod testler;
