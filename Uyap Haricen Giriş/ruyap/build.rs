//! Build script for `ruyap` (Çoklu UYAP).
//! Mirrors the version metadata and static asset checks from `cokluuyap_app.spec`
//! and `cokluuyap_kur.spec`.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../uyap_core/enjekte");
    println!("cargo:rerun-if-changed=../uyap_core/static_cache");
    println!("cargo:rerun-if-changed=../webapp");
    println!("cargo:rerun-if-changed=../UyapBaslat.bat");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let parent_dir = manifest_dir.parent().unwrap_or(&manifest_dir);

    println!(
        "cargo:rustc-env=UYAP_KAYNAK_KOK={}",
        parent_dir.display()
    );
    println!("cargo:rustc-env=UYAP_COMPANY_NAME=Çoklu UYAP");
    println!("cargo:rustc-env=UYAP_FILE_DESCRIPTION=Çoklu UYAP Ofis Uygulaması");
    println!("cargo:rustc-env=UYAP_INTERNAL_NAME=CokluUyap");
    println!("cargo:rustc-env=UYAP_ORIGINAL_FILENAME=CokluUyap.exe");
    println!("cargo:rustc-env=UYAP_PRODUCT_NAME=Çoklu UYAP");

    // Tek sürüm kaynağı (src/surum.rs): yayın derlemesinde CU_SURUM, yoksa Cargo.toml'daki sürüm.
    println!("cargo:rerun-if-env-changed=CU_SURUM");
    let surum = match env::var("CU_SURUM").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(s) => {
            let gecerli = s.split('.').count() >= 3 && s.split('.').count() <= 4
                && s.split('.').all(|p| !p.is_empty() && p.parse::<u16>().is_ok());
            if !gecerli {
                panic!("CU_SURUM geçersiz: {s:?} (beklenen: 26.11.23.2 gibi 3-4 sayı)");
            }
            println!("cargo:rustc-env=CU_SURUM_YAYIN=1");
            s
        }
        None => env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()),
    };
    println!("cargo:rustc-env=CU_SURUM={surum}");

    windows_kaynagi(&manifest_dir, &surum);
}

/// Exe dosya simgesi + sürüm bilgisi (Windows Gezgini'nde Armonia simgesi). Yalnız windows-gnu hedefinde,
/// MinGW `windres` ile; araç yoksa ya da hata verirse derleme durmaz, yalnız uyarı. Kaynaklar ASCII yollu
/// OUT_DIR'e kopyalanır: Türkçe karakterli proje yolu GNU araçlarını bozuyor.
fn windows_kaynagi(manifest_dir: &std::path::Path, surum: &str) {
    let simge = manifest_dir.join("assets").join("armonia.ico");
    println!("cargo:rerun-if-changed={}", simge.display());
    let hedef = env::var("TARGET").unwrap_or_default();
    if !hedef.contains("windows") || !hedef.contains("gnu") {
        return;
    }
    let Ok(out) = env::var("OUT_DIR").map(PathBuf::from) else { return };
    if std::fs::copy(&simge, out.join("armonia.ico")).is_err() {
        println!("cargo:warning=armonia.ico kopyalanamadı; exe simgesiz derlenecek");
        return;
    }
    let mut p: Vec<u16> = surum.split('.').filter_map(|x| x.parse().ok()).collect();
    p.resize(4, 0);
    let rc = format!(
        "#pragma code_page(65001)
         1 ICON \"armonia.ico\"
         1 VERSIONINFO
 FILEVERSION {a},{b},{c},{d}
 PRODUCTVERSION {a},{b},{c},{d}
 FILEOS 0x40004
 FILETYPE 0x1
         BEGIN
  BLOCK \"StringFileInfo\"
  BEGIN
   BLOCK \"041F04B0\"
   BEGIN
             VALUE \"CompanyName\", \"Çoklu UYAP\"
             VALUE \"FileDescription\", \"Çoklu UYAP Ofis Uygulaması\"
             VALUE \"FileVersion\", \"{v}\"
             VALUE \"InternalName\", \"CokluUyap\"
             VALUE \"OriginalFilename\", \"CokluUyap.exe\"
             VALUE \"ProductName\", \"Çoklu UYAP\"
             VALUE \"ProductVersion\", \"{v}\"
           END
  END
  BLOCK \"VarFileInfo\"
  BEGIN
   VALUE \"Translation\", 0x041F, 1200
  END
 END
",
        a = p[0], b = p[1], c = p[2], d = p[3], v = surum
    );
    if std::fs::write(out.join("armonia.rc"), rc).is_err() {
        return;
    }
    let windres = env::var("WINDRES").unwrap_or_else(|_| "windres".into());
    let ok = std::process::Command::new(&windres)
        .current_dir(&out)
        .args(["--codepage=65001", "-i", "armonia.rc", "-O", "coff", "-o", "armonia-res.o"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", out.join("armonia-res.o").display());
    } else {
        println!("cargo:warning=windres çalışmadı; exe simgesiz derlenecek");
    }
}
