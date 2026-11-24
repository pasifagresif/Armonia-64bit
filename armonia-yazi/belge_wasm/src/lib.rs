//! Belge çekirdeğinin tarayıcı köprüsü (WebAssembly). Editör arayüzü belgeyi buradan açar ve kaydeder; Armonia'da
//! (Tauri) ve cokluuyap.com'da aynı köprü kullanılır. Belge hiçbir yere gönderilmez: bayt girer, bayt çıkar.
//! Claude, 2026-11-23.

use belge_cekirdek::gorunum::{gorunum, Gorunum};
use belge_cekirdek::{udf, Belge};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct Acilis {
    belge: Belge,
    gorunum: Gorunum,
    imzali: bool,
    uyarilar: Vec<String>,
}

fn js<T: Serialize>(v: &T) -> Result<JsValue, JsValue> {
    // Düz JS nesneleri (Map değil); demetler dizi olur: Ozellikler = [[ad, değer], ...]
    v.serialize(&serde_wasm_bindgen::Serializer::json_compatible()).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn belge_al(v: JsValue) -> Result<Belge, JsValue> {
    serde_wasm_bindgen::from_value(v).map_err(|e| JsValue::from_str(&format!("belge biçimi bozuk: {e}")))
}

/// Bayt dizisi UDF mi?
#[wasm_bindgen]
pub fn udf_mi(bayt: &[u8]) -> bool {
    udf::udf_mi(bayt)
}

/// UDF açar: `{ belge, gorunum, imzali, uyarilar }`.
#[wasm_bindgen]
pub fn udf_ac(bayt: &[u8]) -> Result<JsValue, JsValue> {
    let o = udf::oku(bayt).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let g = gorunum(&o.belge);
    js(&Acilis { belge: o.belge, gorunum: g, imzali: o.imzali, uyarilar: o.uyarilar })
}

/// DOCX açar: `{ belge, gorunum, imzali: false, uyarilar }` (belge UDF modelinde; UDF olarak kaydedilir).
#[wasm_bindgen]
pub fn docx_ac(bayt: &[u8]) -> Result<JsValue, JsValue> {
    let o = belge_cekirdek::docx::oku(bayt).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let g = gorunum(&o.belge);
    js(&Acilis { belge: o.belge, gorunum: g, imzali: false, uyarilar: o.uyarilar })
}

/// Belge türünü içerikten tanır ve açar (uzantıya güvenilmez): UDF ya da DOCX. Dönüşte `tur` alanı da var.
#[wasm_bindgen]
pub fn belge_ac(bayt: &[u8]) -> Result<JsValue, JsValue> {
    #[derive(Serialize)]
    struct Turlu {
        tur: &'static str,
        #[serde(flatten)]
        acilis: Acilis,
    }
    let (tur, acilis) = if udf::udf_mi(bayt) {
        let o = udf::oku(bayt).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let g = gorunum(&o.belge);
        ("udf", Acilis { belge: o.belge, gorunum: g, imzali: o.imzali, uyarilar: o.uyarilar })
    } else if belge_cekirdek::docx::docx_mi(bayt) {
        let o = belge_cekirdek::docx::oku(bayt).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let g = gorunum(&o.belge);
        ("docx", Acilis { belge: o.belge, gorunum: g, imzali: false, uyarilar: o.uyarilar })
    } else if bayt.starts_with(b"\xD0\xCF\x11\xE0") {
        return Err(JsValue::from_str("Eski Word biçimi (.doc) henüz desteklenmiyor; Word ya da LibreOffice ile .docx olarak kaydedip açın."));
    } else {
        return Err(JsValue::from_str("Tanınmayan dosya: UDF ya da DOCX değil."));
    };
    js(&Turlu { tur, acilis })
}

/// Belgenin görünümünü (çözülmüş stiller) yeniden hesaplar.
#[wasm_bindgen]
pub fn gorunum_hesapla(belge: JsValue) -> Result<JsValue, JsValue> {
    js(&gorunum(&belge_al(belge)?))
}

/// Belgeyi DOCX (Word) baytlarına yazar.
#[wasm_bindgen]
pub fn docx_kaydet(belge: JsValue) -> Result<Vec<u8>, JsValue> {
    belge_cekirdek::docx_yaz::yaz(&belge_al(belge)?).map_err(|e| JsValue::from_str(&e))
}

/// Belgeyi UDF baytlarına yazar.
#[wasm_bindgen]
pub fn udf_kaydet(belge: JsValue) -> Result<Vec<u8>, JsValue> {
    udf::yaz(&belge_al(belge)?).map_err(|e| JsValue::from_str(&e))
}
