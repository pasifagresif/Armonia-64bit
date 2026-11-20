//! Kullanıcı verisi (eklenen tatiller, eklenen TCMB oranları). Gömülü/kanuni veriyi ezmez.
//! Linux: $XDG_DATA_HOME/armonia/hukuk_hesap (yoksa ~/.local/share/...), Windows: %APPDATA%\Armonia\hukuk_hesap.

use crate::cekirdek::faiz::TcmbOrani;
use crate::cekirdek::tatil::Tatil;
use serde::{de::DeserializeOwned, Serialize};
use std::path::PathBuf;

pub fn veri_dizini() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Armonia").join("hukuk_hesap"));
    }
    let taban = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))?;
    Some(taban.join("armonia").join("hukuk_hesap"))
}

fn dosya(ad: &str) -> Option<PathBuf> {
    veri_dizini().map(|d| d.join(ad))
}

/// Dosya yoksa boş liste; bozuksa hata (kullanıcının verisi sessizce silinmesin).
fn oku<T: DeserializeOwned>(ad: &str) -> Result<Vec<T>, String> {
    let Some(yol) = dosya(ad) else { return Ok(Vec::new()) };
    match std::fs::read_to_string(&yol) {
        Ok(m) => serde_json::from_str(&m).map_err(|e| format!("{} okunamadı: {e}", yol.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{} okunamadı: {e}", yol.display())),
    }
}

/// Önce geçici dosyaya yazar, sonra yerine taşır (yarım kalan yazım dosyayı bozmasın).
fn yaz<T: Serialize>(ad: &str, v: &[T]) -> Result<(), String> {
    let yol = dosya(ad).ok_or("Kullanıcı veri dizini bulunamadı")?;
    std::fs::create_dir_all(yol.parent().unwrap()).map_err(|e| e.to_string())?;
    let gecici = yol.with_extension("json.yeni");
    std::fs::write(&gecici, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&gecici, &yol).map_err(|e| e.to_string())
}

pub fn tatilleri_oku() -> Result<Vec<Tatil>, String> {
    oku("tatiller.json")
}

pub fn tatilleri_yaz<'a>(tatiller: impl IntoIterator<Item = &'a Tatil>) -> Result<(), String> {
    yaz("tatiller.json", &tatiller.into_iter().collect::<Vec<_>>())
}

pub fn oranlari_oku() -> Result<Vec<TcmbOrani>, String> {
    oku("tcmb_oranlari.json")
}

pub fn oranlari_yaz<'a>(oranlar: impl IntoIterator<Item = &'a TcmbOrani>) -> Result<(), String> {
    yaz("tcmb_oranlari.json", &oranlar.into_iter().collect::<Vec<_>>())
}
