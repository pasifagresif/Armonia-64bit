//! dist/CokluUyap içeriğinden gerçek Windows Installer paketi (CokluUyapKur.msi) üretir.
//! =======================================================================================
//! Neden MSI? İmzasız tek-exe kurucuyu Chrome/SmartScreen "şüpheli" diye engelliyordu;
//! MSI standart kurulum biçimidir, ayrıca Program Ekle/Kaldır'da görünür ve düzgün
//! güncellenir/kaldırılır (bkz. ux md/masaustu_kurulum_ve_arayuz_sorunlari.md, madde 1).

use chrono::Datelike;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

pub const WIX_URL: &str =
    "https://github.com/wixtoolset/wix3/releases/download/wix3112rtm/wix311-binaries.zip";
pub const APP_ADI: &str = "Çoklu UYAP";
pub const EXE_ADI: &str = "CokluUyap.exe";
/// SABİT kalmalı: Windows bu GUID ile "aynı ürün" olduğunu anlar ve üstüne günceller.
pub const UPGRADE_CODE: &str = "{B6E1B3A4-6C3F-4D2A-9C1E-7A5D0C4E8F21}";
pub const ZAMAN_DAMGASI: &str = "http://timestamp.digicert.com";

pub fn here_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("dist").join("CokluUyap").is_dir() {
                return parent.to_path_buf();
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// MSI sürümü. Yayında `CU_SURUM` verilir (programın içindeki sürümle aynı olmalı: src/surum.rs); verilmezse
/// derleme günü (yıl.ay.gün). MSI yalnız ilk üç parçayı karşılaştırır; aynı günün ikinci derlemesi
/// AllowSameVersionUpgrades sayesinde yine üstüne kurulur.
pub fn surum() -> String {
    if let Ok(s) = std::env::var("CU_SURUM") {
        let s = s.trim();
        if crate::surum::parcalar(s).is_some() {
            return s.to_string();
        }
        eprintln!("CU_SURUM geçersiz ({s:?}); derleme günü kullanılacak.");
    }
    let t = chrono::Local::now().date_naive();
    format!("{}.{}.{}", t.year() % 100, t.month(), t.day())
}

pub fn wix_hazirla(wix_dir: &Path) {
    if wix_dir.join("candle.exe").exists() {
        return;
    }
    let _ = fs::create_dir_all(wix_dir);
    let zpath = wix_dir.join("wix311-binaries.zip");
    println!("WiX araçları indiriliyor (bir kereye mahsus, ~33 MB)…");
    let resp = reqwest::blocking::get(WIX_URL).unwrap_or_else(|e| {
        eprintln!("WiX indirilemedi: {e}");
        std::process::exit(1);
    });
    let bytes = resp.bytes().unwrap_or_else(|e| {
        eprintln!("WiX indirme verisi okunamadı: {e}");
        std::process::exit(1);
    });
    fs::write(&zpath, &bytes).unwrap_or_else(|e| {
        eprintln!("WiX zip yazılamadı: {e}");
        std::process::exit(1);
    });
    let file = File::open(&zpath).unwrap_or_else(|e| {
        eprintln!("WiX zip açılamadı: {e}");
        std::process::exit(1);
    });
    let mut archive = ZipArchive::new(file).unwrap_or_else(|e| {
        eprintln!("WiX zip geçersiz: {e}");
        std::process::exit(1);
    });
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let outpath = match entry.enclosed_name() {
            Some(p) => wix_dir.join(p),
            None => continue,
        };
        if entry.name().ends_with('/') || entry.is_dir() {
            let _ = fs::create_dir_all(&outpath);
        } else {
            if let Some(p) = outpath.parent() {
                let _ = fs::create_dir_all(p);
            }
            let mut outfile = File::create(&outpath).unwrap();
            let _ = io::copy(&mut entry, &mut outfile);
        }
    }
    let _ = fs::remove_file(&zpath);
}

pub fn calistir(komut: &[&str]) -> String {
    if komut.is_empty() {
        std::process::exit(1);
    }
    let out = Command::new(komut[0])
        .args(&komut[1..])
        .output()
        .unwrap_or_else(|e| {
            let base = Path::new(komut[0])
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| komut[0].to_string());
            eprintln!("Komut başarısız ({base}, {e})");
            std::process::exit(1);
        });
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        let _ = io::stderr().write_all(format!("{stdout}{stderr}").as_bytes());
        let base = Path::new(komut[0])
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| komut[0].to_string());
        let code = out.status.code().unwrap_or(1);
        eprintln!("Komut başarısız ({base}, kod {code})");
        std::process::exit(1);
    }
    stdout
}

pub fn signtool_bul() -> String {
    let yol = std::env::var("UYAP_SIGNTOOL").unwrap_or_default().trim().to_string();
    if !yol.is_empty() {
        return yol;
    }
    let pfx86 = std::env::var("ProgramFiles(x86)")
        .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string());
    let bin_dir = Path::new(&pfx86).join("Windows Kits").join("10").join("bin");
    let mut adaylar: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&bin_dir) {
        for entry in entries.flatten() {
            let st = entry.path().join("x64").join("signtool.exe");
            if st.is_file() {
                adaylar.push(st);
            }
        }
    }
    adaylar.sort();
    adaylar
        .last()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn imza_argumanlari() -> Option<Vec<String>> {
    let pfx = std::env::var("UYAP_IMZA_PFX").unwrap_or_default().trim().to_string();
    let sha1 = std::env::var("UYAP_IMZA_SHA1").unwrap_or_default().trim().to_string();
    let dlib = std::env::var("UYAP_IMZA_DLIB").unwrap_or_default().trim().to_string();
    if !pfx.is_empty() {
        let mut args = vec!["/f".to_string(), pfx];
        let parola = std::env::var("UYAP_IMZA_PAROLA").unwrap_or_default();
        if !parola.is_empty() {
            args.push("/p".to_string());
            args.push(parola);
        }
        return Some(args);
    }
    if !sha1.is_empty() {
        return Some(vec!["/sha1".to_string(), sha1]);
    }
    if !dlib.is_empty() {
        let dmdf = std::env::var("UYAP_IMZA_DMDF").unwrap_or_default().trim().to_string();
        return Some(vec!["/dlib".to_string(), dlib, "/dmdf".to_string(), dmdf]);
    }
    None
}

pub fn imzala(dosya: &Path) -> bool {
    let kimlik = match imza_argumanlari() {
        Some(k) => k,
        None => return false,
    };
    let st = signtool_bul();
    if st.is_empty() || !Path::new(&st).exists() {
        eprintln!(
            "İmza ayarı var ama signtool.exe bulunamadı (UYAP_SIGNTOOL verin ya da Windows SDK kurun)."
        );
        std::process::exit(1);
    }
    let dosya_str = dosya.to_string_lossy().into_owned();
    let mut sign_args: Vec<String> = vec![
        st.clone(),
        "sign".to_string(),
        "/fd".to_string(),
        "sha256".to_string(),
        "/tr".to_string(),
        ZAMAN_DAMGASI.to_string(),
        "/td".to_string(),
        "sha256".to_string(),
        "/d".to_string(),
        APP_ADI.to_string(),
    ];
    sign_args.extend(kimlik);
    sign_args.push(dosya_str.clone());
    let sign_refs: Vec<&str> = sign_args.iter().map(|s| s.as_str()).collect();
    calistir(&sign_refs);
    calistir(&[&st, "verify", "/pa", &dosya_str]);
    let base = dosya
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or(dosya_str);
    println!("  imzalandı: {base}");
    true
}

pub fn urun_wxs(ad: &str, exe: &str, surum_str: &str, upgrade: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi"
     xmlns:util="http://schemas.microsoft.com/wix/UtilExtension">
  <Product Id="*" Name="{ad}" Language="1055" Codepage="1254" Version="{surum_str}"
           Manufacturer="{ad}" UpgradeCode="{upgrade}">
    <!-- perUser + limited: %LOCALAPPDATA% altına kurar, UAC penceresi ÇIKMAZ. -->
    <Package InstallerVersion="500" Compressed="yes" InstallScope="perUser"
             InstallPrivileges="limited" Description="{ad} Kurulumu" />
    <MajorUpgrade AllowSameVersionUpgrades="yes"
                  DowngradeErrorMessage="Daha yeni bir sürüm zaten kurulu." />
    <MediaTemplate EmbedCab="yes" CompressionLevel="high" />
    <Property Id="MSIFASTINSTALL" Value="7" />
    <Property Id="ARPNOMODIFY" Value="1" />

    <Directory Id="TARGETDIR" Name="SourceDir">
      <Directory Id="LocalAppDataFolder">
        <Directory Id="INSTALLDIR" Name="CokluUyap" />
      </Directory>
      <Directory Id="DesktopFolder" />
      <Directory Id="ProgramMenuFolder" />
    </Directory>

    <!-- Kısayollar: perUser kurulumda keypath HKCU kayıt değeri olmak zorunda. -->
    <DirectoryRef Id="DesktopFolder">
      <Component Id="MasaustuKisayol" Guid="{{2F8A1D07-93B4-4E5C-A1D2-0B6C3E7F9A44}}">
        <Shortcut Id="MasaustuLnk" Name="{ad}" Target="[INSTALLDIR]{exe}"
                  WorkingDirectory="INSTALLDIR" />
        <RegistryValue Root="HKCU" Key="Software\CokluUyap" Name="masaustu_kisayol"
                       Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>
    <DirectoryRef Id="ProgramMenuFolder">
      <Component Id="BaslatKisayol" Guid="{{5C0D2E19-74A6-4B8F-B3E4-1D7F5A2C8B66}}">
        <Shortcut Id="BaslatLnk" Name="{ad}" Target="[INSTALLDIR]{exe}"
                  WorkingDirectory="INSTALLDIR" />
        <RegistryValue Root="HKCU" Key="Software\CokluUyap" Name="baslat_kisayol"
                       Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>

    <!-- Güncellemede çalışan örneği nazikçe kapat (eski kurucudaki taskkill'in dengi). -->
    <util:CloseApplication Id="CalisaniKapat" Target="{exe}" CloseMessage="yes"
                           RebootPrompt="no" />

    <!-- Kurulum bitince uygulamayı başlat (eski kurucu davranışı korunur).
         WixShellExecTarget çalışma anında biçimlenir; CNDL1077 uyarısı kasıtlı/zararsız. -->
    <Property Id="WixShellExecTarget" Value="[INSTALLDIR]{exe}" />
    <CustomAction Id="UygulamayiBaslat" BinaryKey="WixCA" DllEntry="WixShellExec"
                  Impersonate="yes" Return="ignore" />
    <InstallExecuteSequence>
      <Custom Action="UygulamayiBaslat" After="InstallFinalize">NOT Installed OR REINSTALL OR UPGRADINGPRODUCTCODE</Custom>
    </InstallExecuteSequence>

    <Feature Id="Ana" Title="{ad}" Level="1">
      <ComponentGroupRef Id="AppFiles" />
      <ComponentRef Id="MasaustuKisayol" />
      <ComponentRef Id="BaslatKisayol" />
    </Feature>
  </Product>
</Wix>
"#
    )
}

pub fn main() {
    let here = here_dir();
    // 2026-11-23: Rust paketi (araclar/yayin_hazirla.py) kaynak klasörü ve çıktı dosyasını ortam değişkeniyle verir;
    // verilmezse eski Python düzeni (dist/CokluUyap → dist/CokluUyapKur.msi).
    let ortam = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).map(PathBuf::from);
    let src = ortam("CU_MSI_KAYNAK").unwrap_or_else(|| here.join("dist").join("CokluUyap"));
    let msi = ortam("CU_MSI_CIKTI").unwrap_or_else(|| here.join("dist").join("CokluUyapKur.msi"));
    let build = msi.parent().map(|p| p.join("build-msi")).unwrap_or_else(|| here.join("build"));
    let wix = ortam("CU_WIX").unwrap_or_else(|| here.join("araclar").join("wix311"));

    if !src.is_dir() {
        eprintln!("{} yok — önce paketi hazırlayın (araclar/yayin_hazirla.py).", src.display());
        std::process::exit(1);
    }
    let exe_path = src.join(EXE_ADI);
    if !exe_path.exists() {
        eprintln!("dist/CokluUyap içinde {EXE_ADI} yok — derleme eksik.");
        std::process::exit(1);
    }

    wix_hazirla(&wix);
    let _ = fs::create_dir_all(&build);
    let v = surum();

    // 0) Uygulama exe'sini MSI'ya girmeden imzala (heat/light dosyayı bu hâliyle alır).
    let imzali = imzala(&exe_path);
    if !imzali {
        println!(
            "UYARI: kod imzalama ayarı yok — exe ve MSI İMZASIZ üretiliyor; Defender/\
SmartScreen yanlış pozitif verebilir (bkz. dosya başındaki UYAP_IMZA_*)."
        );
    }

    // 1) Dosya ağacını hasat et (heat): INSTALLDIR köküne, otomatik-kararlı GUID'lerle.
    let dosyalar_wxs = build.join("msi_dosyalar.wxs");
    println!("[1/3] Dosyalar taranıyor ({}) …", src.display());
    let heat_exe = wix.join("heat.exe").to_string_lossy().into_owned();
    let src_str = src.to_string_lossy().into_owned();
    let dosyalar_wxs_str = dosyalar_wxs.to_string_lossy().into_owned();
    calistir(&[
        &heat_exe,
        "dir",
        &src_str,
        "-cg",
        "AppFiles",
        "-dr",
        "INSTALLDIR",
        "-srd",
        "-sfrag",
        "-ag",
        "-sreg",
        "-scom",
        "-var",
        "var.SourceDir",
        "-nologo",
        "-out",
        &dosyalar_wxs_str,
    ]);

    let urun_wxs_path = build.join("msi_urun.wxs");
    let xml = urun_wxs(APP_ADI, EXE_ADI, &v, UPGRADE_CODE);
    fs::write(&urun_wxs_path, xml.as_bytes()).unwrap_or_else(|e| {
        eprintln!("msi_urun.wxs yazılamadı: {e}");
        std::process::exit(1);
    });

    // 2) Derle (candle).
    println!("[2/3] Derleniyor (sürüm {v}) …");
    let candle_exe = wix.join("candle.exe").to_string_lossy().into_owned();
    let d_source_dir = format!("-dSourceDir={src_str}");
    let build_out = format!("{}{}", build.display(), std::path::MAIN_SEPARATOR);
    let urun_wxs_str = urun_wxs_path.to_string_lossy().into_owned();
    calistir(&[
        &candle_exe,
        "-nologo",
        "-ext",
        "WixUtilExtension",
        &d_source_dir,
        "-out",
        &build_out,
        &urun_wxs_str,
        &dosyalar_wxs_str,
    ]);

    // 3) Bağla (light). -sval: ICE doğrulaması atlanır — perUser dosya bileşenleri
    // ICE38'e takılır (bilinen ve kasıtlı durum), 1700+ dosyada doğrulama çok yavaş.
    println!("[3/3] MSI paketleniyor (sıkıştırma birkaç dakika sürebilir) …");
    let light_exe = wix.join("light.exe").to_string_lossy().into_owned();
    let msi_str = msi.to_string_lossy().into_owned();
    let urun_wixobj = build.join("msi_urun.wixobj").to_string_lossy().into_owned();
    let dosyalar_wixobj = build
        .join("msi_dosyalar.wixobj")
        .to_string_lossy()
        .into_owned();
    calistir(&[
        &light_exe,
        "-nologo",
        "-sval",
        "-ext",
        "WixUtilExtension",
        "-out",
        &msi_str,
        &urun_wixobj,
        &dosyalar_wixobj,
    ]);

    if imzali {
        imzala(&msi);
    }

    let size_mb = fs::metadata(&msi).map(|m| m.len() as f64 / 1e6).unwrap_or(0.0);
    println!(
        "MSI hazır: {} ({:.1} MB, sürüm {})",
        msi.display(),
        size_mb,
        v
    );
    // Güncelleme yayınında (CU_MSI_CIKTI) dağıtımı araclar/yayin_hazirla.py anlatır.
    if std::env::var("CU_MSI_CIKTI").is_err() {
        println!(
            "Dağıtım: MSI'yı downloads/CokluUyapKur.msi olarak kopyalayıp \
vendor_deploy'a commit + PUSH edin."
        );
    }
}
