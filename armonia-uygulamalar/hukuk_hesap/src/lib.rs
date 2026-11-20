//! Armonia Hukuk Hesaplayıcı — arayüzsüz hesap çekirdeği.
//!
//! Her hesap sonucu, adım adım bir **döküm** ve dayanak maddeleriyle döner; arayüz yalnız gösterir.
//! Plan: `docs/claude-notlar/planlar/hukuk-hesaplayici.md`. Claude, 2026-11-19.

pub mod cekirdek;
pub use cekirdek::*;
