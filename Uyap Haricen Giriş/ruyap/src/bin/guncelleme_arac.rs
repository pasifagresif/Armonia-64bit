//! Güncelleme yayın aracı (2026-11-23). Gizli anahtar yalnız yayıncının bilgisayarında kalır.
//!
//!   guncelleme_arac anahtar-uret <anahtar.json>
//!       Yeni anahtar çifti üretir, gizli anahtarı parolayla şifreleyip dosyaya yazar, açık anahtarı gösterir
//!       (src/guncelleme_imza.rs → ACIK_ANAHTARLAR'a eklenir). Asıl ve yedek için iki kez çalıştırılır.
//!   guncelleme_arac imzala --anahtar <anahtar.json> --surum 26.11.23.2 --notlar "…" [--zorunlu]
//!       --paket windows-x64=<https adres>=<yerel dosya> [--paket …] --cikti test.json
//!       Paketlerin boyutunu / sha256'sını hesaplar, sürüm dosyasını imzalar ve yazar.
//!   guncelleme_arac dogrula <test.json> [--acik <base64>]…
//!       İmzayı programa gömülü (ya da verilen) açık anahtarlarla doğrular.

use clap::{Parser, Subcommand};
use ruyap::guncelleme_imza as imza;
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Çoklu UYAP güncelleme yayın aracı")]
struct Arg {
    #[command(subcommand)]
    komut: Komut,
}

#[derive(Subcommand)]
enum Komut {
    /// Yeni anahtar çifti üret.
    AnahtarUret { cikti: PathBuf },
    /// Sürüm dosyasını üret ve imzala.
    Imzala {
        #[arg(long)]
        anahtar: PathBuf,
        #[arg(long)]
        surum: String,
        #[arg(long, default_value = "")]
        notlar: String,
        #[arg(long)]
        zorunlu: bool,
        /// platform=adres=yerel_dosya (ör. windows-x64=https://www.cokluuyap.com/indir/CokluUyap-26.11.23.2.msi=dist/CokluUyapKur.msi)
        #[arg(long, required = true)]
        paket: Vec<String>,
        #[arg(long)]
        cikti: PathBuf,
    },
    /// Sürüm dosyasının imzasını doğrula.
    Dogrula {
        dosya: PathBuf,
        #[arg(long)]
        acik: Vec<String>,
    },
}

fn main() {
    let a = Arg::parse();
    let sonuc = match a.komut {
        Komut::AnahtarUret { cikti } => anahtar_uret(cikti),
        Komut::Imzala { anahtar, surum, notlar, zorunlu, paket, cikti } => imzala(anahtar, surum, notlar, zorunlu, paket, cikti),
        Komut::Dogrula { dosya, acik } => dogrula(dosya, acik),
    };
    if let Err(e) = sonuc {
        eprintln!("HATA: {e}");
        std::process::exit(1);
    }
}

fn anahtar_uret(cikti: PathBuf) -> Result<(), String> {
    if cikti.exists() {
        return Err(format!("{} zaten var; üzerine yazılmaz.", cikti.display()));
    }
    println!("Gizli anahtarı koruyacak parolayı girin (en az 12 karakter; unutulursa anahtar açılamaz).");
    let p1 = parola_oku("Parola: ")?;
    let p2 = parola_oku("Parola (yeniden): ")?;
    if p1 != p2 {
        return Err("Parolalar aynı değil.".into());
    }
    let g = imza::yeni_anahtar();
    let icerik = imza::anahtar_dosyasi(&g, &p1)?;
    if let Some(d) = cikti.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&cikti, icerik).map_err(|e| format!("Yazılamadı: {e}"))?;
    println!("\nAnahtar dosyası: {}", cikti.display());
    println!("Açık anahtar (programa gömülecek):\n  {}", imza::acik_anahtar_b64(&g));
    println!("\nBu dosyanın bir yedeğini çevrim dışı (USB) saklayın. Dosya ve parola birlikte kimseye verilmemeli.");
    Ok(())
}

fn imzala(anahtar: PathBuf, surum: String, notlar: String, zorunlu: bool, paketler: Vec<String>, cikti: PathBuf) -> Result<(), String> {
    if ruyap::surum::parcalar(&surum).is_none() {
        return Err(format!("Sürüm geçersiz: {surum} (ör. 26.11.23.2)"));
    }
    let mut pk = serde_json::Map::new();
    for p in &paketler {
        let mut ps = p.splitn(3, '=');
        let (Some(platform), Some(adres), Some(yerel)) = (ps.next(), ps.next(), ps.next()) else {
            return Err(format!("--paket biçimi: platform=adres=yerel_dosya ({p})"));
        };
        if !adres.starts_with(ruyap::guncelleme::IZINLI_KOK) {
            eprintln!("UYARI: {adres} {} altında değil; programlar bu paketi reddeder.", ruyap::guncelleme::IZINLI_KOK);
        }
        let veri = std::fs::read(yerel).map_err(|e| format!("{yerel} okunamadı: {e}"))?;
        println!("{platform}: {} bayt, {}", veri.len(), yerel);
        pk.insert(platform.to_string(), serde_json::json!({ "url": adres, "boyut": veri.len(), "sha256": imza::sha256_hex(&veri) }));
    }
    let icerik = std::fs::read_to_string(&anahtar).map_err(|e| format!("Anahtar dosyası okunamadı: {e}"))?;
    let g = imza::anahtar_ac(&icerik, &parola_oku("Anahtar parolası: ")?)?;
    let mut v = serde_json::json!({
        "bicim": imza::BICIM, "surum": surum, "kanal": ruyap::guncelleme::KANAL,
        "tarih": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        "zorunlu": zorunlu, "notlar": notlar, "paketler": pk,
    });
    imza::imzala(&mut v, &g)?;
    let acik = imza::acik_anahtar_b64(&g);
    imza::dogrula_anahtarlarla(&v, &[("bu", acik.as_str())])?;
    if !imza::ACIK_ANAHTARLAR.iter().any(|(_, a)| *a == acik) {
        eprintln!("UYARI: bu anahtar programa gömülü değil; kurulu programlar bu dosyayı reddeder.");
    }
    std::fs::write(&cikti, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    println!("Sürüm dosyası yazıldı: {}", cikti.display());
    Ok(())
}

fn dogrula(dosya: PathBuf, acik: Vec<String>) -> Result<(), String> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&dosya).map_err(|e| e.to_string())?)
        .map_err(|_| "JSON okunamadı.")?;
    let ad = if acik.is_empty() {
        imza::dogrula(&v)?
    } else {
        let k: Vec<(&str, &str)> = acik.iter().map(|a| ("verilen", a.as_str())).collect();
        imza::dogrula_anahtarlarla(&v, &k)?
    };
    println!("İmza geçerli ({ad} anahtarı). Sürüm {}", v.get("surum").and_then(|x| x.as_str()).unwrap_or("?"));
    Ok(())
}

/// Parolayı ekranda göstermeden okur (Windows konsolu ve Unix uçbirimi).
fn parola_oku(istem: &str) -> Result<String, String> {
    print!("{istem}");
    let _ = std::io::stdout().flush();
    let _yanki = YankiKapali::kur();
    let mut s = String::new();
    std::io::stdin().lock().read_line(&mut s).map_err(|e| e.to_string())?;
    println!();
    Ok(s.trim_end_matches(['\r', '\n']).to_string())
}

struct YankiKapali {
    #[cfg(windows)]
    eski: Option<(windows_sys::Win32::Foundation::HANDLE, u32)>,
    #[cfg(target_os = "linux")]
    eski: Option<libc_termios::Termios>,
}

#[cfg(windows)]
impl YankiKapali {
    fn kur() -> Self {
        use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_ECHO_INPUT, STD_INPUT_HANDLE};
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            let mut kip = 0u32;
            if GetConsoleMode(h, &mut kip) != 0 && SetConsoleMode(h, kip & !ENABLE_ECHO_INPUT) != 0 {
                return YankiKapali { eski: Some((h, kip)) };
            }
        }
        YankiKapali { eski: None }
    }
}

#[cfg(windows)]
impl Drop for YankiKapali {
    fn drop(&mut self) {
        if let Some((h, kip)) = self.eski {
            unsafe { windows_sys::Win32::System::Console::SetConsoleMode(h, kip) };
        }
    }
}

#[cfg(target_os = "linux")]
mod libc_termios {
    pub type Termios = libc::termios;
}

#[cfg(target_os = "linux")]
impl YankiKapali {
    fn kur() -> Self {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) == 0 {
                let eski = t;
                t.c_lflag &= !libc::ECHO;
                if libc::tcsetattr(0, libc::TCSANOW, &t) == 0 {
                    return YankiKapali { eski: Some(eski) };
                }
            }
        }
        YankiKapali { eski: None }
    }
}

#[cfg(target_os = "linux")]
impl Drop for YankiKapali {
    fn drop(&mut self) {
        if let Some(t) = self.eski {
            unsafe { libc::tcsetattr(0, libc::TCSANOW, &t) };
        }
    }
}
