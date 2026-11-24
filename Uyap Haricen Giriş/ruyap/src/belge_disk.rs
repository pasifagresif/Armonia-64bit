//! Büro belgelerinin diskteki klasör düzeni (24.11.2026; plan docs/claude-notlar/planlar/yazi-cokluuyap.md
//! "Büro Belgeleri sayfası + klasör düzeni").
//!
//! ```text
//! <kök>/Belgeler/<Yargı türü>/<Birim adı>/<yıl-sıra>/<belge adı>.udf          son sürüm
//! <kök>/Belgeler/<Yargı türü>/<Birim adı>/<yıl-sıra>/.surumler/<belge no>/<N>.udf   eski sürümler
//! <kök>/Belgeler/Dosyasız/<kullanıcı>/…                                         dosyaya bağlı olmayan
//! ```
//! Klasörler toptan açılmaz: belge kaydedildikçe oluşur, içindeki son belge gidince boş klasör silinir.
//! Kök `kok_dizin()`: Windows'ta %LOCALAPPDATA%\CokluUyap, Armonia Linux'ta LUKS2 kasasındaki
//! /var/lib/armonia/CokluUyap (kasa_bagla.sh o dizini zaten kasaya bağlar).
//!
//! Hangi dosyanın hangi belge olduğunu veritabanı söyler (`belge.yol`: kökten göreli, `/` ayraçlı). Yol yalnız burada,
//! temizlenmiş parçalardan üretilir; okunurken de denetlenir (mutlak yol, `..`, boş parça yok).

use std::path::{Path, PathBuf};

pub const SURUM_KLASORU: &str = ".surumler";
pub const DOSYASIZ: &str = "Dosyasız";

pub fn kok() -> PathBuf {
    crate::veritabani::kok_dizin().join("Belgeler")
}

/// Klasör ya da dosya adı parçası: Windows'ta yasak karakterler ve aygıt adları (CON, NUL…), baştaki/sondaki nokta ve
/// boşluk (gizli klasörlerle çakışmasın), uzunluk.
pub fn parca(s: &str, en_cok: usize) -> String {
    let t: String = s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.chars().take(en_cok).collect();
    let t = t.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if t.is_empty() {
        return "_".into();
    }
    const AYGIT: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
        "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let govde = t.split('.').next().unwrap_or("").trim().to_ascii_uppercase();
    if AYGIT.contains(&govde.as_str()) {
        format!("_{t}")
    } else {
        t
    }
}

/// Belgenin dosyasının künyesi (klasörü bundan çıkar).
#[derive(Clone, Debug, Default)]
pub struct Kunye {
    /// Yargı türünün adı ("İcra", "Hukuk"…); bilinmiyorsa "Diğer"
    pub yargi: String,
    pub birim: String,
    pub dosya_no: String,
    /// Talimat dosyası: aynı birimde aynı numaralı esas dosyasıyla karışmasın
    pub talimat: bool,
}

/// Belgenin klasörü (kökten göreli parçalar). Künye eksikse `Dosyasız/<kullanıcı>`.
pub fn klasor(k: Option<&Kunye>, kullanici: &str) -> Vec<String> {
    match k {
        Some(k) if !k.birim.trim().is_empty() && !k.dosya_no.trim().is_empty() => {
            let mut d = parca(&k.dosya_no.replace('/', "-"), 40);
            if k.talimat {
                d.push_str(" Talimat");
            }
            let y = if k.yargi.trim().is_empty() { "Diğer" } else { k.yargi.as_str() };
            vec![parca(y, 40), parca(&k.birim, 80), d]
        }
        _ => vec![DOSYASIZ.into(), parca(if kullanici.trim().is_empty() { "Ortak" } else { kullanici }, 60)],
    }
}

/// Veritabanındaki göreli yolu diskteki yola çevirir; bizim üretmediğimiz biçimde bir yol reddedilir.
pub fn coz(yol: &str) -> Result<PathBuf, String> {
    let bozuk = || "Belgenin disk yolu bozuk.".to_string();
    if yol.is_empty() || yol.starts_with('/') || yol.contains('\\') || yol.contains(':') || yol.contains('\0') {
        return Err(bozuk());
    }
    let mut p = kok();
    for s in yol.split('/') {
        if s.is_empty() || s == "." || s == ".." || s.trim() != s {
            return Err(bozuk());
        }
        p.push(s);
    }
    Ok(p)
}

/// Yolun klasör kısmı (dosya adı hariç).
pub fn klasoru(yol: &str) -> &str {
    yol.rsplit_once('/').map(|(k, _)| k).unwrap_or("")
}

/// Belgenin eski sürümlerinin klasörü: `<belgenin klasörü>/.surumler/<belge no>`.
pub fn surum_klasoru(yol: &str, id: i64) -> Result<PathBuf, String> {
    Ok(coz(klasoru(yol))?.join(SURUM_KLASORU).join(id.to_string()))
}

pub fn surum_dosyasi(yol: &str, id: i64, surum: i32, bicim: &str) -> Result<PathBuf, String> {
    Ok(surum_klasoru(yol, id)?.join(format!("{surum}.{bicim}")))
}

/// Klasörde boş bir ad: "<ad>.<uzantı>", doluysa "<ad> (2).<uzantı>"… `dolu`: veritabanında başka bir belgenin yolu mu.
/// Diskteki dosyaya da bakılır (Windows'ta büyük/küçük harf ayırmaz).
pub fn bos_yol(klasor: &[String], ad: &str, bicim: &str, mut dolu: impl FnMut(&str) -> Result<bool, String>) -> Result<String, String> {
    let govde = parca(ad, 80);
    for i in 1..1000 {
        let dosya = if i == 1 { format!("{govde}.{bicim}") } else { format!("{govde} ({i}).{bicim}") };
        let yol = format!("{}/{dosya}", klasor.join("/"));
        if !dolu(&yol)? && !coz(&yol)?.exists() {
            return Ok(yol);
        }
    }
    Err("Bu klasörde aynı adda çok fazla belge var; başka bir ad verin.".into())
}

/// Dosyayı yarım kalmadan yazar (önce geçici dosya, sonra yerine taşıma).
pub fn yaz(hedef: &Path, veri: &[u8]) -> Result<(), String> {
    let klasor = hedef.parent().ok_or("Belge klasörü yok.")?;
    std::fs::create_dir_all(klasor).map_err(|e| format!("Belge klasörü açılamadı: {e}"))?;
    let ad = hedef.file_name().and_then(|s| s.to_str()).unwrap_or("belge");
    let gecici = klasor.join(format!(".{ad}.{}.yarim", std::process::id()));
    std::fs::write(&gecici, veri).map_err(|e| format!("Belge yazılamadı: {e}"))?;
    std::fs::rename(&gecici, hedef).map_err(|e| {
        let _ = std::fs::remove_file(&gecici);
        format!("Belge yazılamadı: {e}")
    })
}

/// Dosyayı ya da klasörü taşır (hedefin üst klasörü açılır). `.surumler` klasörü Windows'ta gizlenir.
pub fn tasi(kaynak: &Path, hedef: &Path) -> Result<(), String> {
    if let Some(k) = hedef.parent() {
        std::fs::create_dir_all(k).map_err(|e| format!("Belge klasörü açılamadı: {e}"))?;
        gizle_surumler(k);
    }
    std::fs::rename(kaynak, hedef).map_err(|e| format!("Belge taşınamadı: {e}"))
}

/// Yolda `.surumler` klasörü varsa Windows'ta gizli işaretlenir (Linux'ta adındaki nokta zaten gizler).
pub fn gizle_surumler(p: &Path) {
    let mut a = p;
    loop {
        if a.file_name().and_then(|s| s.to_str()) == Some(SURUM_KLASORU) {
            gizle(a);
            return;
        }
        match a.parent() {
            Some(u) => a = u,
            None => return,
        }
    }
}

#[cfg(windows)]
fn gizle(p: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, INVALID_FILE_ATTRIBUTES};
    let w: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: w NUL ile biten geçerli bir UTF-16 yol; çağrılar yalnız bu tamponu okur.
    unsafe {
        let a = GetFileAttributesW(w.as_ptr());
        if a != INVALID_FILE_ATTRIBUTES && a & FILE_ATTRIBUTE_HIDDEN == 0 {
            SetFileAttributesW(w.as_ptr(), a | FILE_ATTRIBUTE_HIDDEN);
        }
    }
}

#[cfg(not(windows))]
fn gizle(_: &Path) {}

/// `baslangic`tan köke doğru boş klasörleri siler (içinde yalnız boş `.surumler` kalmışsa onu da). Kökün kendisi kalır.
pub fn bos_klasorleri_sil(baslangic: &Path) {
    let kok = kok();
    let mut a = baslangic.to_path_buf();
    while a.starts_with(&kok) && a != kok {
        let _ = std::fs::remove_dir(a.join(SURUM_KLASORU)); // yalnız boşsa silinir
        if std::fs::remove_dir(&a).is_err() {
            return;
        }
        match a.parent() {
            Some(u) => a = u.to_path_buf(),
            None => return,
        }
    }
}

/// Belgenin bütün dosyalarını (son sürüm + eski sürümler) siler, boşalan klasörleri toplar.
pub fn belgeyi_sil(yol: &str, id: i64) {
    if let Ok(p) = coz(yol) {
        let _ = std::fs::remove_file(&p);
    }
    if let Ok(s) = surum_klasoru(yol, id) {
        let _ = std::fs::remove_dir_all(&s);
    }
    if let Ok(k) = coz(klasoru(yol)) {
        bos_klasorleri_sil(&k);
    }
}

/// Disk işlemlerini veritabanı işlemiyle birlikte geri alır: veritabanı commit'lenmeden çıkılırsa (hata, `?`)
/// yazılan dosyalar silinir, taşınanlar yerine döner. Başarıda `tamam()`.
#[derive(Default)]
pub struct GeriAl {
    isler: Vec<GeriIs>,
    tamam: bool,
}

enum GeriIs {
    Sil(PathBuf),
    Tasi(PathBuf, PathBuf),
}

impl GeriAl {
    pub fn yazildi(&mut self, p: &Path) {
        self.isler.push(GeriIs::Sil(p.to_path_buf()));
    }
    /// `kaynak` → `hedef` taşındı (geri alınırken hedef → kaynak).
    pub fn tasindi(&mut self, kaynak: &Path, hedef: &Path) {
        self.isler.push(GeriIs::Tasi(hedef.to_path_buf(), kaynak.to_path_buf()));
    }
    pub fn tamam(mut self) {
        self.tamam = true;
    }
}

impl Drop for GeriAl {
    fn drop(&mut self) {
        if self.tamam {
            return;
        }
        for i in self.isler.drain(..).rev() {
            match i {
                GeriIs::Sil(p) => {
                    let _ = std::fs::remove_file(&p);
                    if let Some(k) = p.parent() {
                        bos_klasorleri_sil(k);
                    }
                }
                GeriIs::Tasi(a, b) => {
                    let _ = std::fs::rename(&a, &b);
                    if let Some(k) = a.parent() {
                        bos_klasorleri_sil(k);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parca_temizler() {
        assert_eq!(parca("İstanbul 5. İcra Dairesi", 80), "İstanbul 5. İcra Dairesi");
        assert_eq!(parca("a/b\\c:d*e?f\"g<h>i|j", 80), "a b c d e f g h i j");
        assert_eq!(parca("  ..gizli..  ", 80), "gizli");
        assert_eq!(parca("CON", 80), "_CON");
        assert_eq!(parca("nul.udf", 80), "_nul.udf");
        assert_eq!(parca("", 80), "_");
        assert_eq!(parca("abcdef", 3), "abc");
    }

    #[test]
    fn klasor_duzeni() {
        let k = Kunye { yargi: "İcra".into(), birim: "Ankara 3. İcra Dairesi".into(), dosya_no: "2026/38696".into(), talimat: false };
        assert_eq!(klasor(Some(&k), "zeynel"), ["İcra", "Ankara 3. İcra Dairesi", "2026-38696"]);
        let t = Kunye { talimat: true, ..k.clone() };
        assert_eq!(klasor(Some(&t), "zeynel")[2], "2026-38696 Talimat");
        assert_eq!(klasor(None, "zeynel"), ["Dosyasız", "zeynel"]);
        let eksik = Kunye { dosya_no: String::new(), ..k };
        assert_eq!(klasor(Some(&eksik), ""), ["Dosyasız", "Ortak"]);
    }

    #[test]
    fn yol_denetimi() {
        assert!(coz("İcra/Ankara/2026-1/Dilekçe.udf").is_ok());
        for kotu in ["", "/mutlak", "a/../b", "a//b", "a/./b", "C:/x", "a\\b", " a/b"] {
            assert!(coz(kotu).is_err(), "{kotu}");
        }
        assert_eq!(klasoru("a/b/c.udf"), "a/b");
    }
}
