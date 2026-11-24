//! Güncelleme denetimi ve kurulumu (2026-11-23). İmza ve sürüm dosyasının biçimi: `guncelleme_imza`.
//!
//! - Program açıldıktan 1 dk sonra, sonra 6 saatte bir cokluuyap.com'daki imzalı sürüm dosyasına bakar. İstekte
//!   kimlik / büro / müvekkil bilgisi yoktur; yalnız `User-Agent: CokluUyap/<sürüm>`.
//! - İmza tutmayan, kanalı farklı, sürümü kurulu olandan eski ya da aynı, paket adresi cokluuyap.com dışında olan
//!   dosya yok sayılır.
//! - Yeni sürüm sayfada (görev çubuğu, enjekte/gorevcubugu.js) bildirilir. Kurulum yalnız kullanıcı onayıyla
//!   (`kur`): paket indirilir, boyutu ve sha256'sı imzalı değerle karşılaştırılır, tutarsa kurulur. Toplu tarama
//!   sürerken kurulum başlamaz.
//! - Kurma adımı platforma göredir: Windows'ta MSI (msiexec; MSI çalışan programı kapatıp yenisini açar). Armonia
//!   Linux'ta paket biçimi belli olunca bağlanacak; şimdilik indirip doğrular, kurmaz.
//! - Geliştirme derlemesi (`CU_SURUM` verilmeden) denetim yapmaz. Deneme için `CU_GUNCELLEME_URL` verilir; o zaman
//!   paket adresi de o sunucuda olabilir. Yalnız geliştirme derlemesinde: `CU_GUNCELLEME_ACIK` (deneme açık
//!   anahtarı, gömülülere eklenir) ve `CU_GUNCELLEME_KURMA=0` (indirip doğrular, kurulumu çalıştırmaz).

use crate::guncelleme_imza as imza;
use crate::surum;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

pub const VARSAYILAN_URL: &str = "https://www.cokluuyap.com/guncelleme/test.json";
pub const IZINLI_KOK: &str = "https://www.cokluuyap.com/";
pub const KANAL: &str = "test";
const ARALIK: Duration = Duration::from_secs(6 * 3600);
const ILK_BEKLEME: Duration = Duration::from_secs(60);
const EN_BUYUK_SURUM_DOSYASI: usize = 64 * 1024;
const EN_BUYUK_PAKET: u64 = 600 * 1024 * 1024;

#[derive(Clone, Serialize, Debug, PartialEq)]
pub struct Yeni {
    pub surum: String,
    pub notlar: String,
    pub zorunlu: bool,
    pub tarih: String,
    pub boyut: u64,
    #[serde(skip)]
    pub url: String,
    #[serde(skip)]
    pub sha256: String,
}

#[derive(Clone, Serialize, Default)]
pub struct Durum {
    pub kurulu: String,
    pub platform: &'static str,
    /// Denetim açık mı (yayın derlemesi ya da deneme adresi verilmiş).
    pub etkin: bool,
    pub son_denetim: Option<String>,
    pub hata: Option<String>,
    pub yeni: Option<Yeni>,
    /// "" | "indiriliyor" | "kuruluyor" | "indirildi"
    pub asama: String,
    pub inen: u64,
    pub mesaj: Option<String>,
}

static DURUM: Lazy<Mutex<Durum>> = Lazy::new(|| {
    Mutex::new(Durum { kurulu: surum::SURUM.to_string(), platform: imza::platform(), etkin: kaynak().is_some(), ..Default::default() })
});

/// Sürüm dosyasının adresi; denetim kapalıysa None.
fn kaynak() -> Option<String> {
    if let Ok(u) = std::env::var("CU_GUNCELLEME_URL") {
        let u = u.trim().to_string();
        if !u.is_empty() {
            return Some(u);
        }
    }
    if surum::GELISTIRME {
        None
    } else {
        Some(VARSAYILAN_URL.to_string())
    }
}

/// Gömülü açık anahtarlar; geliştirme derlemesinde deneme anahtarı da.
fn anahtarlar() -> Vec<(String, String)> {
    let mut k: Vec<(String, String)> = imza::ACIK_ANAHTARLAR.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    if surum::GELISTIRME {
        if let Ok(a) = std::env::var("CU_GUNCELLEME_ACIK") {
            if !a.trim().is_empty() {
                k.push(("deneme".into(), a.trim().to_string()));
            }
        }
    }
    k
}

fn deneme_kurma() -> bool {
    surum::GELISTIRME && std::env::var("CU_GUNCELLEME_KURMA").is_ok_and(|v| v.trim() == "0")
}

/// Paket adresinin başlaması gereken kök. Deneme adresinde o sunucunun kökü.
fn izinli_kok(kaynak: &str) -> String {
    if kaynak == VARSAYILAN_URL {
        return IZINLI_KOK.to_string();
    }
    match url::Url::parse(kaynak) {
        Ok(u) => format!("{}/", u.origin().ascii_serialization()),
        Err(_) => IZINLI_KOK.to_string(),
    }
}

/// Doğrulanmış sürüm dosyasını yorumlar. Yeni ve bu platforma uygun sürüm varsa döner.
pub fn yorumla(v: &Value, anahtarlar: &[(&str, &str)], kurulu: &str, platform: &str, kok: &str) -> Result<Option<Yeni>, String> {
    imza::dogrula_anahtarlarla(v, anahtarlar)?;
    if v.get("bicim").and_then(Value::as_u64) != Some(imza::BICIM) {
        return Err("Sürüm dosyasının biçimi tanınmıyor (programı elle güncelleyin).".into());
    }
    if v.get("kanal").and_then(Value::as_str) != Some(KANAL) {
        return Err("Sürüm dosyası başka bir kanala ait.".into());
    }
    let s = v.get("surum").and_then(Value::as_str).unwrap_or("");
    if surum::parcalar(s).is_none() {
        return Err("Sürüm dosyasındaki sürüm numarası geçersiz.".into());
    }
    if !surum::daha_yeni(s, kurulu) {
        return Ok(None);
    }
    let Some(p) = v.pointer(&format!("/paketler/{platform}")) else { return Ok(None) };
    let url = p.get("url").and_then(Value::as_str).unwrap_or("");
    let boyut = p.get("boyut").and_then(Value::as_u64).unwrap_or(0);
    let sha = p.get("sha256").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    if !url.starts_with(kok) || url.contains("..") || url.contains('@') {
        return Err("Paket adresi izinli sunucuda değil.".into());
    }
    if boyut == 0 || boyut > EN_BUYUK_PAKET {
        return Err("Paket boyutu geçersiz.".into());
    }
    if sha.len() != 64 || !sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Paket özeti geçersiz.".into());
    }
    Ok(Some(Yeni {
        surum: s.to_string(),
        notlar: v.get("notlar").and_then(Value::as_str).unwrap_or("").to_string(),
        zorunlu: v.get("zorunlu").and_then(Value::as_bool).unwrap_or(false),
        tarih: v.get("tarih").and_then(Value::as_str).unwrap_or("").to_string(),
        boyut,
        url: url.to_string(),
        sha256: sha,
    }))
}

fn istemci(zaman_asimi: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("CokluUyap/{}", surum::SURUM))
        .timeout(zaman_asimi)
        .build()
        .map_err(|e| format!("Bağlantı kurulamadı: {e}"))
}

fn simdi() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// Bir kez denetler, durumu günceller.
pub fn denetle() {
    let Some(adres) = kaynak() else { return };
    let sonuc = (|| -> Result<Option<Yeni>, String> {
        let yanit = istemci(Duration::from_secs(20))?.get(&adres).send().map_err(|e| format!("Sürüm bilgisi alınamadı: {e}"))?;
        if !yanit.status().is_success() {
            return Err(format!("Sürüm bilgisi alınamadı (HTTP {}).", yanit.status().as_u16()));
        }
        let mut b = Vec::new();
        yanit.take(EN_BUYUK_SURUM_DOSYASI as u64 + 1).read_to_end(&mut b).map_err(|e| e.to_string())?;
        if b.len() > EN_BUYUK_SURUM_DOSYASI {
            return Err("Sürüm dosyası çok büyük.".into());
        }
        let v: Value = serde_json::from_slice(&b).map_err(|_| "Sürüm dosyası okunamadı.")?;
        let k = anahtarlar();
        let kr: Vec<(&str, &str)> = k.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        yorumla(&v, &kr, surum::SURUM, imza::platform(), &izinli_kok(&adres))
    })();
    let mut d = DURUM.lock();
    d.son_denetim = Some(simdi());
    match sonuc {
        Ok(y) => {
            d.hata = None;
            // İndirme sürerken bulunan bilgi değişmesin.
            if d.asama != "indiriliyor" && d.asama != "kuruluyor" {
                d.yeni = y;
            }
        }
        Err(e) => {
            eprintln!("[güncelleme] {e}");
            d.hata = Some(e);
        }
    }
}

/// Arka plan denetimini başlatır (program açılışında bir kez).
pub fn baslat() {
    if kaynak().is_none() {
        return;
    }
    let _ = std::thread::Builder::new().name("guncelleme".into()).spawn(|| {
        let deneme = std::env::var("CU_GUNCELLEME_URL").is_ok_and(|u| !u.trim().is_empty());
        std::thread::sleep(if deneme { Duration::from_secs(3) } else { ILK_BEKLEME });
        loop {
            denetle();
            std::thread::sleep(ARALIK);
        }
    });
}

/// Sayfanın istediği "şimdi denetle" (en fazla dakikada bir).
pub fn simdi_denetle() {
    let son = DURUM.lock().son_denetim.clone();
    if son.as_deref() == Some(simdi().as_str()) || kaynak().is_none() {
        return;
    }
    let _ = std::thread::Builder::new().name("guncelleme-denetim".into()).spawn(denetle);
}

pub fn durum_json() -> Value {
    serde_json::to_value(DURUM.lock().clone()).unwrap_or(Value::Null)
}

fn dizin() -> PathBuf {
    #[cfg(windows)]
    if let Ok(d) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(d).join("CokluUyap").join("guncelleme");
    }
    if let Ok(d) = std::env::var("XDG_CACHE_HOME") {
        if !d.trim().is_empty() {
            return PathBuf::from(d).join("cokluuyap").join("guncelleme");
        }
    }
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h).join(".cache").join("cokluuyap").join("guncelleme");
    }
    crate::accounts::data_dir().join("guncelleme")
}

/// Kullanıcı onayıyla indirir, doğrular ve kurar (arka planda). Hemen döner.
pub fn kur() -> Result<(), String> {
    let y = {
        let mut d = DURUM.lock();
        if d.asama == "indiriliyor" || d.asama == "kuruluyor" {
            return Err("Güncelleme zaten sürüyor.".into());
        }
        let y = d.yeni.clone().ok_or("Kurulacak yeni sürüm yok.")?;
        if crate::icra_guncelle::suruyor() {
            return Err("Toplu tarama sürüyor; bitince yeniden deneyin.".into());
        }
        d.asama = "indiriliyor".into();
        d.inen = 0;
        d.mesaj = None;
        d.hata = None;
        y
    };
    let _ = std::thread::Builder::new().name("guncelleme-kur".into()).spawn(move || {
        let sonuc = indir(&y).and_then(|yol| {
            if deneme_kurma() {
                DURUM.lock().asama = "indirildi".into();
                return Ok(format!("Deneme: paket indirildi ve doğrulandı ({}), kurulmadı.", yol.display()));
            }
            DURUM.lock().asama = "kuruluyor".into();
            kur_platform(&yol)
        });
        let mut d = DURUM.lock();
        match sonuc {
            Ok(m) => {
                d.mesaj = Some(m);
                if d.asama == "kuruluyor" && !cfg!(windows) {
                    d.asama = "indirildi".into();
                }
            }
            Err(e) => {
                eprintln!("[güncelleme] {e}");
                d.asama.clear();
                d.hata = Some(e);
            }
        }
    });
    Ok(())
}

fn indir(y: &Yeni) -> Result<PathBuf, String> {
    let d = dizin();
    std::fs::create_dir_all(&d).map_err(|e| format!("Güncelleme klasörü oluşturulamadı: {e}"))?;
    // Eski indirmeler silinir.
    if let Ok(o) = std::fs::read_dir(&d) {
        for g in o.flatten() {
            let _ = std::fs::remove_file(g.path());
        }
    }
    let uzanti = if cfg!(windows) { "msi" } else { "paket" };
    let yol = d.join(format!("CokluUyap-{}.{uzanti}", y.surum));
    let gecici = d.join(format!("CokluUyap-{}.indiriliyor", y.surum));
    let sonuc = (|| -> Result<(), String> {
        let mut yanit = istemci(Duration::from_secs(30 * 60))?.get(&y.url).send().map_err(|e| format!("Paket indirilemedi: {e}"))?;
        if !yanit.status().is_success() {
            return Err(format!("Paket indirilemedi (HTTP {}).", yanit.status().as_u16()));
        }
        let mut f = std::fs::File::create(&gecici).map_err(|e| format!("Paket yazılamadı: {e}"))?;
        let mut h = Sha256::new();
        let mut toplam: u64 = 0;
        let mut tampon = vec![0u8; 256 * 1024];
        loop {
            let n = yanit.read(&mut tampon).map_err(|e| format!("Paket indirilemedi: {e}"))?;
            if n == 0 {
                break;
            }
            toplam += n as u64;
            if toplam > y.boyut {
                return Err("Paket beklenenden büyük; kurulmadı.".into());
            }
            h.update(&tampon[..n]);
            f.write_all(&tampon[..n]).map_err(|e| format!("Paket yazılamadı: {e}"))?;
            DURUM.lock().inen = toplam;
        }
        f.flush().map_err(|e| e.to_string())?;
        drop(f);
        if toplam != y.boyut {
            return Err("Paket eksik indi; kurulmadı.".into());
        }
        if hex::encode(h.finalize()) != y.sha256 {
            return Err("Paketin özeti imzalı değerle uyuşmuyor; kurulmadı.".into());
        }
        std::fs::rename(&gecici, &yol).map_err(|e| format!("Paket yerine konamadı: {e}"))
    })();
    if let Err(e) = sonuc {
        let _ = std::fs::remove_file(&gecici);
        return Err(e);
    }
    Ok(yol)
}

#[cfg(windows)]
fn kur_platform(yol: &std::path::Path) -> Result<String, String> {
    // MSI kullanıcıya kurulur (UAC yok). Program önce kendini düzgün kapatır (veritabanı ve geçit durur, kurulacak
    // dosyalar serbest kalır); MSI'ın "kapan" mesajına güvenilmez, çünkü pencere kapatılınca tepsiye gizlenir.
    // Bu yüzden kurulum, program kapandıktan birkaç saniye sonra ayrı bir süreçte başlar; MSI bitince yeni sürümü açar.
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let kisa = crate::veritabani::kisa_yol(yol);
    let p = kisa.to_string_lossy();
    if p.contains('"') || p.contains('&') || p.contains('%') {
        return Err("Paket yolu kurulum için uygun değil.".into());
    }
    std::process::Command::new("cmd")
        .args(["/d", "/c"])
        .raw_arg(format!("ping -n 5 127.0.0.1 >nul & msiexec /i \"{p}\" /passive /norestart"))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Kurulum başlatılamadı: {e}"))?;
    let _ = std::thread::Builder::new().name("guncelleme-cikis".into()).spawn(|| {
        std::thread::sleep(Duration::from_millis(1500));
        crate::tepsi::cikis();
    });
    Ok("Program kapanıyor; kurulum birkaç saniye içinde başlayacak ve yeni sürüm kendiliğinden açılacak.".into())
}

#[cfg(not(windows))]
fn kur_platform(yol: &std::path::Path) -> Result<String, String> {
    Ok(format!("Paket indirildi ve doğrulandı ({}); bu sistemde kendiliğinden kurulum henüz yok.", yol.display()))
}

#[cfg(test)]
mod testler {
    use super::*;

    fn dosya(g: &p256::ecdsa::SigningKey, surum_: &str, url: &str) -> Value {
        let mut v = serde_json::json!({
            "bicim": 1, "surum": surum_, "kanal": "test", "tarih": "2026-11-23", "zorunlu": true, "notlar": "n",
            "paketler": { "windows-x64": { "url": url, "boyut": 10, "sha256": "ab".repeat(32) } }
        });
        imza::imzala(&mut v, g).unwrap();
        v
    }

    #[test]
    fn yorum() {
        let g = imza::yeni_anahtar();
        let a = imza::acik_anahtar_b64(&g);
        let k = [("asil", a.as_str())];
        let url = "https://www.cokluuyap.com/indir/CokluUyap-26.11.23.2.msi";
        let y = yorumla(&dosya(&g, "26.11.23.2", url), &k, "26.11.23.1", "windows-x64", IZINLI_KOK).unwrap().unwrap();
        assert_eq!(y.surum, "26.11.23.2");
        assert!(y.zorunlu);
        // Aynı / eski sürüm, başka platform: yeni yok.
        assert!(yorumla(&dosya(&g, "26.11.23.2", url), &k, "26.11.23.2", "windows-x64", IZINLI_KOK).unwrap().is_none());
        assert!(yorumla(&dosya(&g, "26.11.22", url), &k, "26.11.23.1", "windows-x64", IZINLI_KOK).unwrap().is_none());
        assert!(yorumla(&dosya(&g, "26.11.23.2", url), &k, "26.11.23.1", "linux-x64", IZINLI_KOK).unwrap().is_none());
        // Yabancı adres (imzalı olsa bile), benzer ad hilesi.
        assert!(yorumla(&dosya(&g, "26.11.23.2", "https://kotu.example/a.msi"), &k, "26.11.23.1", "windows-x64", IZINLI_KOK).is_err());
        assert!(yorumla(&dosya(&g, "26.11.23.2", "https://www.cokluuyap.com.kotu.example/a.msi"), &k, "26.11.23.1", "windows-x64", IZINLI_KOK).is_err());
        assert!(yorumla(&dosya(&g, "26.11.23.2", "https://www.cokluuyap.com@kotu.example/a.msi"), &k, "26.11.23.1", "windows-x64", IZINLI_KOK).is_err());
        // Başka anahtar.
        let b = imza::yeni_anahtar();
        assert!(yorumla(&dosya(&b, "26.11.23.2", url), &k, "26.11.23.1", "windows-x64", IZINLI_KOK).is_err());
        // Başka kanal.
        let mut v = serde_json::json!({ "bicim": 1, "surum": "26.11.24", "kanal": "kararli", "paketler": {} });
        imza::imzala(&mut v, &g).unwrap();
        assert!(yorumla(&v, &k, "26.11.23.1", "windows-x64", IZINLI_KOK).is_err());
    }

    /// Yerel bir HTTP sunucusuyla denetim → indirme → doğrulama (kurulum çalıştırılmaz). Kurcalanmış paket reddedilir.
    #[test]
    fn uctan_uca_yerel_sunucu() {
        use std::io::{BufRead, BufReader};
        use std::sync::{Arc, Mutex as StdMutex};
        let paket: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let sunulan = Arc::new(StdMutex::new(paket.clone()));
        let dosya = Arc::new(StdMutex::new(Vec::<u8>::new()));
        let dinleyici = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let kok = format!("http://{}/", dinleyici.local_addr().unwrap());
        let (s2, d2) = (sunulan.clone(), dosya.clone());
        std::thread::spawn(move || {
            for akim in dinleyici.incoming().flatten() {
                let mut okuyucu = BufReader::new(akim.try_clone().unwrap());
                let mut ilk = String::new();
                let _ = okuyucu.read_line(&mut ilk);
                loop {
                    let mut l = String::new();
                    if okuyucu.read_line(&mut l).unwrap_or(0) == 0 || l == "\r\n" {
                        break;
                    }
                }
                let govde = if ilk.contains("/test.json") { d2.lock().unwrap().clone() } else { s2.lock().unwrap().clone() };
                let mut a = akim;
                let _ = write!(a, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", govde.len());
                let _ = a.write_all(&govde);
            }
        });
        let g = imza::yeni_anahtar();
        let mut v = serde_json::json!({
            "bicim": 1, "surum": "99.1.1.1", "kanal": "test", "tarih": "2026-11-23", "zorunlu": false, "notlar": "deneme",
            "paketler": { imza::platform(): { "url": format!("{kok}paket.bin"), "boyut": paket.len(), "sha256": imza::sha256_hex(&paket) } }
        });
        imza::imzala(&mut v, &g).unwrap();
        *dosya.lock().unwrap() = serde_json::to_vec(&v).unwrap();
        let gecici = std::env::temp_dir().join(format!("cu-guncelleme-deneme-{}", std::process::id()));
        std::env::set_var("CU_GUNCELLEME_URL", format!("{kok}test.json"));
        std::env::set_var("CU_GUNCELLEME_ACIK", imza::acik_anahtar_b64(&g));
        std::env::set_var("CU_GUNCELLEME_KURMA", "0");
        std::env::set_var("LOCALAPPDATA", &gecici);
        std::env::set_var("XDG_CACHE_HOME", &gecici);

        let bekle = || {
            for _ in 0..200 {
                let a = DURUM.lock().asama.clone();
                if a != "indiriliyor" && a != "kuruluyor" {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("indirme bitmedi");
        };
        denetle();
        assert_eq!(DURUM.lock().yeni.as_ref().map(|y| y.surum.clone()).as_deref(), Some("99.1.1.1"), "hata: {:?}", DURUM.lock().hata);
        kur().unwrap();
        bekle();
        {
            let d = DURUM.lock();
            assert_eq!(d.asama, "indirildi", "hata: {:?}", d.hata);
            assert_eq!(d.inen, paket.len() as u64);
        }
        let inen = std::fs::read(dizin().join("CokluUyap-99.1.1.1.".to_string() + if cfg!(windows) { "msi" } else { "paket" })).unwrap();
        assert_eq!(inen, paket);

        // Kurcalanmış paket (aynı boyut, bir bayt farklı): reddedilir, dosya bırakılmaz.
        sunulan.lock().unwrap()[1000] ^= 1;
        DURUM.lock().asama.clear();
        kur().unwrap();
        bekle();
        {
            let d = DURUM.lock();
            assert!(d.hata.as_deref().unwrap_or("").contains("özeti"), "hata: {:?}", d.hata);
            assert!(d.asama.is_empty());
        }
        assert!(std::fs::read_dir(dizin()).unwrap().next().is_none(), "kurcalanmış paket diskte kaldı");

        // İmzası bozulmuş sürüm dosyası: yeni sürüm görünmez, hata yazılır.
        let mut bozuk = v.clone();
        bozuk["notlar"] = Value::String("değişti".into());
        *dosya.lock().unwrap() = serde_json::to_vec(&bozuk).unwrap();
        DURUM.lock().yeni = None;
        denetle();
        assert!(DURUM.lock().yeni.is_none());
        assert!(DURUM.lock().hata.as_deref().unwrap_or("").contains("İmza geçersiz"));
        let _ = std::fs::remove_dir_all(&gecici);
    }

    #[test]
    fn kok() {
        assert_eq!(izinli_kok(VARSAYILAN_URL), IZINLI_KOK);
        assert_eq!(izinli_kok("http://127.0.0.1:8899/test.json"), "http://127.0.0.1:8899/");
    }
}
