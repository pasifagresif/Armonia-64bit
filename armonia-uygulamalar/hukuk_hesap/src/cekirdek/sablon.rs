//! Hazır süre şablonları. Her biri dayanak maddesiyle; **hiçbiri kullanıcı tarafından doğrulanmadı**
//! (`dogrulandi: false` → arayüzde uyarı). Kullanıcı onayladıkça `true` yapılır. Claude, 2026-11-19.

use crate::cekirdek::sure::Birim;

#[derive(Clone, Copy, Debug)]
pub struct Sablon {
    pub ad: &'static str,
    pub miktar: u32,
    pub birim: Birim,
    pub adli_tatil: bool,
    pub dayanak: &'static str,
    pub dogrulandi: bool,
}

pub const SABLONLAR: &[Sablon] = &[
    Sablon { ad: "Cevap dilekçesi (yazılı yargılama)", miktar: 2, birim: Birim::Hafta, adli_tatil: true, dayanak: "HMK 127", dogrulandi: false },
    Sablon { ad: "Bilirkişi raporuna itiraz", miktar: 2, birim: Birim::Hafta, adli_tatil: true, dayanak: "HMK 281", dogrulandi: false },
    Sablon { ad: "İstinaf (hukuk)", miktar: 2, birim: Birim::Hafta, adli_tatil: true, dayanak: "HMK 345", dogrulandi: false },
    Sablon { ad: "Temyiz (hukuk)", miktar: 1, birim: Birim::Ay, adli_tatil: true, dayanak: "HMK 361", dogrulandi: false },
    Sablon { ad: "Ödeme emrine itiraz (genel haciz)", miktar: 7, birim: Birim::Gun, adli_tatil: false, dayanak: "İİK 62", dogrulandi: false },
    Sablon { ad: "Kambiyo takibinde borca itiraz", miktar: 5, birim: Birim::Gun, adli_tatil: false, dayanak: "İİK 168, 169", dogrulandi: false },
    Sablon { ad: "İcra memuru işlemine şikâyet", miktar: 7, birim: Birim::Gun, adli_tatil: false, dayanak: "İİK 16", dogrulandi: false },
    Sablon { ad: "İdari dava açma", miktar: 60, birim: Birim::Gun, adli_tatil: false, dayanak: "İYUK 7", dogrulandi: false },
];
