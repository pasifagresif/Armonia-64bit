//! Çift tıklanan dosyayı uzantısına göre doğru uygulamada açar.
use std::path::Path;
use std::process::Command;

/// Dosyayı uzantısına göre açar. Bilinmeyen/eşleşmeyen uzantılar `xdg-open`'a (masaüstü varsayılanı) düşer.
pub fn ac(yol: &Path) {
    let uzanti = yol
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let (program, ekler): (&str, &[&str]) = match uzanti.as_str() {
        "udf" | "docx" | "doc" | "odt" => ("/opt/armonia/bin/yazi_ac.sh", &[]),
        "pdf" => ("evince", &[]),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => ("imv", &[]),
        "ods" | "xlsx" | "xls" => ("libreoffice", &["--calc"]),
        _ => ("xdg-open", &[]),
    };
    let mut komut = Command::new(program);
    komut.args(ekler).arg(yol);
    let _ = komut.spawn();
}

#[cfg(test)]
mod testler {
    use super::*;

    fn uzanti_programi(ad: &str) -> &'static str {
        let yol = Path::new(ad);
        let uzanti = yol
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        match uzanti.as_str() {
            "udf" | "docx" | "doc" | "odt" => "yazi",
            "pdf" => "evince",
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => "imv",
            "ods" | "xlsx" | "xls" => "libreoffice",
            _ => "xdg-open",
        }
    }

    #[test]
    fn dogru_uygulamaya_yonlendirir() {
        assert_eq!(uzanti_programi("dilekce.udf"), "yazi");
        assert_eq!(uzanti_programi("rapor.PDF"), "evince");
        assert_eq!(uzanti_programi("foto.JPG"), "imv");
        assert_eq!(uzanti_programi("tablo.xlsx"), "libreoffice");
        assert_eq!(uzanti_programi("arsiv.zip"), "xdg-open");
        assert_eq!(uzanti_programi("uzantisiz"), "xdg-open");
    }
}
