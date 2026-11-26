//! Armonia Yazı masaüstü uygulaması (Tauri 2). Editör arayüzü (../arayuz) sistemin web motorunda açılır; bu kabuk
//! yalnız şunları verir: dosya açma/kaydetme pencereleri, komut satırından (çift tıkla) gelen dosya, imza hizmeti
//! köprüsü (/run/armonia-imza/imza.sock) ve imzalı belgenin doğrulanması. Arayüze dosya sistemi, kabuk ya da ağ izni
//! verilmez (capabilities/varsayilan.json: yalnız core:default). Claude, 2026-11-24.
//!
//! Arayüz dosya YOLU göndermez (denetçi 24.11 O2): kabuk, kullanıcının pencereden seçtiği ya da çift tıkla açtığı
//! dosyaları kendi listesinde tutar, arayüze yalnız bir kimlik verir. "Kaydet" yalnız bu listedeki bir dosyaya
//! doğrudan yazar; başka her durumda kaydetme penceresi açılır. Böylece ele geçirilmiş bir arayüz, kullanıcının
//! seçmediği bir dosyanın üzerine yazamaz.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(unix)]
mod tek_ornek;

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use base64::Engine as _;
use serde::Serialize;
use serde_json::{json, Value};

/// Açılabilecek en büyük dosya (çekirdeğin sınırıyla aynı)
const AZAMI: u64 = 64 << 20;

fn b64() -> &'static base64::engine::GeneralPurpose {
    &base64::engine::general_purpose::STANDARD
}

/// Kullanıcının seçtiği dosyalar: kimlik → yol (yalnız kabukta; arayüz yolu görmez/göndermez)
#[derive(Default)]
struct Dosyalar {
    sayac: AtomicU64,
    liste: Mutex<HashMap<u64, PathBuf>>,
}

impl Dosyalar {
    /// Aynı yol yeniden gelirse aynı kimlik döner: arayüz zaten açık belgeyi ikinci kez açmaz, sekmesine geçer.
    fn kaydet(&self, yol: PathBuf) -> u64 {
        let mut liste = self.liste.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, _)) = liste.iter().find(|(_, y)| **y == yol) {
            return *k;
        }
        let k = self.sayac.fetch_add(1, Ordering::Relaxed) + 1;
        liste.insert(k, yol);
        k
    }
    fn yol(&self, k: u64) -> Option<PathBuf> {
        self.liste.lock().unwrap_or_else(|e| e.into_inner()).get(&k).cloned()
    }
}

#[derive(Serialize, Clone)]
struct Dosya {
    ad: String,
    /// Kabuğun listesindeki kimlik ("Kaydet" bununla doğrudan yazar)
    kimlik: u64,
    /// base64
    bayt: String,
}

#[derive(Serialize)]
struct Kayit {
    ad: String,
    kimlik: u64,
    /// Yalnız bildirimde göstermek için
    yol: String,
}

fn ad_al(yol: &Path) -> String {
    yol.file_name().map(|a| a.to_string_lossy().to_string()).unwrap_or_default()
}

fn dosya_oku(yol: &Path, d: &Dosyalar) -> Result<Dosya, String> {
    let boy = std::fs::metadata(yol).map_err(|e| format!("dosya okunamadı: {e}"))?.len();
    if boy > AZAMI {
        return Err(format!("dosya çok büyük ({} MB üstü)", AZAMI >> 20));
    }
    let v = std::fs::read(yol).map_err(|e| format!("dosya okunamadı: {e}"))?;
    Ok(Dosya { ad: ad_al(yol), kimlik: d.kaydet(yol.to_path_buf()), bayt: b64().encode(v) })
}

/// Önce geçici dosyaya yazılır, sonra yerine taşınır: yarıda kesilen kayıt eski dosyayı bozmasın.
/// Geçici dosya yeni oluşturulur (create_new = O_CREAT|O_EXCL): aynı adda dosya ya da sembolik bağ varsa izlenmez,
/// hata verir (denetçi 24.11 O2).
fn atomik_yaz(yol: &Path, v: &[u8]) -> Result<(), String> {
    static SIRA: AtomicU64 = AtomicU64::new(0);
    let ad = format!(".{}.{}-{}.armonia-yazi-gecici", ad_al(yol), std::process::id(), SIRA.fetch_add(1, Ordering::Relaxed));
    let gecici = yol.with_file_name(ad);
    let yaz = || -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&gecici)?;
        f.write_all(v)?;
        f.sync_all()
    };
    if let Err(e) = yaz() {
        let _ = std::fs::remove_file(&gecici);
        return Err(format!("kaydedilemedi: {e}"));
    }
    std::fs::rename(&gecici, yol).map_err(|e| {
        let _ = std::fs::remove_file(&gecici);
        format!("kaydedilemedi: {e}")
    })
}

/// Belgeler'de `onerilen` adının çakışmayan bir sürümü ("Ad (2).uzanti" vb.). Yeni belgenin ilk kaydı ve "Farklı
/// kaydet" buraya düşer: yerli kaydetme penceresi (tauri-plugin-dialog) D-Bus/xdg-desktop-portal ister, ama bu
/// birimde (armonia-yazi@.service) D-Bus kasıtlı olarak hiç yok (denetçi 24.11 O4) — pencere hep "Operation was
/// cancelled" ile çöküyordu. Masaüstü sandbox'ı yalnız Belgeler ve İndirilenler'i bağlıyor; burada yol seçtirecek
/// yerli bir bileşen yok, o yüzden sabit klasöre otomatik adla kaydedilir (kullanıcı dosyayı sonra Armonia
/// Dosyalar'da yeniden adlandırabilir/taşıyabilir). Claude, 2026-11-25.
fn carpismasiz_yol(onerilen: &str) -> PathBuf {
    let kok = Path::new("/home/armonia/Belgeler");
    let ad = Path::new(onerilen);
    let govde = ad.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Belge".into());
    let uzanti = ad.extension().map(|s| s.to_string_lossy().into_owned());
    let adlandir = |i: u32| match (&uzanti, i) {
        (Some(u), 0) => format!("{govde}.{u}"),
        (Some(u), _) => format!("{govde} ({i}).{u}"),
        (None, 0) => govde.clone(),
        (None, _) => format!("{govde} ({i})"),
    };
    (0..1000).map(adlandir).map(|a| kok.join(a)).find(|y| !y.exists()).unwrap_or_else(|| kok.join(onerilen))
}

/// Kaydeder. `kimlik` kabuğun listesindeyse ve uzantısı biçimle aynıysa ("Kaydet") doğrudan o dosyaya yazar; başka
/// her durumda Belgeler'de yeni bir ad üretilir (bkz. `carpismasiz_yol`) ve seçilen dosya listeye girer.
#[tauri::command]
async fn dosya_kaydet(
    d: tauri::State<'_, Dosyalar>,
    onerilen: String,
    bayt: String,
    bicim: String,
    kimlik: Option<u64>,
) -> Result<Option<Kayit>, String> {
    let v = b64().decode(bayt).map_err(|_| "belge bozuk".to_string())?;
    let uzanti = if bicim == "docx" { "docx" } else { "udf" };
    let bilinen = kimlik.and_then(|k| d.yol(k).map(|y| (k, y))).filter(|(_, y)| {
        y.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() == Some(uzanti)
    });
    let (k, hedef) = match bilinen {
        Some(b) => b,
        None => {
            let p = carpismasiz_yol(&onerilen);
            (d.kaydet(p.clone()), p)
        }
    };
    atomik_yaz(&hedef, &v)?;
    Ok(Some(Kayit { ad: ad_al(&hedef), kimlik: k, yol: hedef.display().to_string() }))
}

/// Komut satırından verilen dosya (çift tıkla açma: `armonia-yazi belge.udf`).
#[tauri::command]
fn baslangic_dosyasi(d: tauri::State<'_, Dosyalar>) -> Option<Dosya> {
    let a = std::env::args_os().nth(1)?;
    dosya_oku(Path::new(&a), &d).ok()
}

/// İmza hizmetine tek satırlık istek (bin/armonia-imza.rs). Belge ve PIN yalnız bu yerel sokete gider.
fn imza_istek(istek: Value) -> Result<Value, String> {
    #[cfg(unix)]
    {
        use std::io::{BufRead, BufReader, Write};
        let yol = std::env::var("ARMONIA_IMZA_SOKET").unwrap_or_else(|_| "/run/armonia-imza/imza.sock".into());
        let mut s = std::os::unix::net::UnixStream::connect(&yol)
            .map_err(|e| format!("İmza hizmetine ulaşılamadı ({yol}): {e}. Armonia'da imza hizmeti çalışıyor mu?"))?;
        s.set_read_timeout(Some(std::time::Duration::from_secs(180))).ok();
        let mut satir = istek.to_string();
        satir.push('\n');
        s.write_all(satir.as_bytes()).map_err(|e| format!("imza isteği gönderilemedi: {e}"))?;
        drop(satir);
        let mut yanit = String::new();
        BufReader::new(s).read_line(&mut yanit).map_err(|e| format!("imza yanıtı okunamadı: {e}"))?;
        serde_json::from_str(&yanit).map_err(|_| "imza hizmeti geçersiz yanıt verdi".to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = istek;
        Err("İmza hizmeti yalnız Armonia'da (Linux) var.".into())
    }
}

#[tauri::command]
async fn imza_durum() -> Result<Value, String> {
    imza_istek(json!({ "islem": "durum" }))
}

#[tauri::command]
async fn imzala(dosya_adi: String, udf: String, pin: String, sertifika: Option<String>) -> Result<Value, String> {
    imza_istek(json!({ "islem": "imzala", "dosya_adi": dosya_adi, "udf": udf, "pin": pin, "seri": sertifika }))
}

/// İmzalı UDF'i yerelde doğrular (hizmete gerek yok): imzalayan, zaman, belge değişmiş mi, imza geçerli mi.
#[tauri::command]
fn imza_dogrula(udf: String) -> Result<Option<imza::udf_imza::ImzaBilgisi>, String> {
    let v = b64().decode(udf).map_err(|_| "belge bozuk".to_string())?;
    imza::udf_imza::udf_dogrula(&v).map_err(|e| e.to_string())
}

/// Başka bir örneğin devrettiği isteği bu pencerede açar: belge yeni sekme olur (arayüz: "dosya-ac"), yol yoksa
/// yeni boş belge ("yeni-belge"). Pencere küçültülmüşse geri getirilir.
#[cfg(unix)]
fn devralinani_ac(uyg: &tauri::AppHandle, belge: Option<PathBuf>) {
    use tauri::{Emitter, Manager};
    let sonuc = match belge {
        None => uyg.emit("yeni-belge", ()),
        Some(yol) => match tek_ornek::kabul_edilir(&yol).and_then(|()| dosya_oku(&yol, &uyg.state::<Dosyalar>())) {
            Ok(dosya) => uyg.emit("dosya-ac", dosya),
            Err(e) => uyg.emit("ac-hata", e),
        },
    };
    if let Err(e) = sonuc {
        eprintln!("armonia-yazi: devralınan istek arayüze iletilemedi: {e}");
    }
    if let Some(p) = uyg.get_webview_window("main") {
        let _ = p.unminimize();
        let _ = p.set_focus();
    }
}

fn main() {
    // Armonia Yazı zaten açıksa belge o pencereye sekme olarak devredilir; bu örnek pencere açmadan çıkar
    #[cfg(unix)]
    let dinleyici = {
        let belge = std::env::args_os().nth(1).map(PathBuf::from);
        match tek_ornek::baslat(belge.as_deref()) {
            tek_ornek::Baslangic::Devredildi => return,
            tek_ornek::Baslangic::Dinle(d) => Some(d),
            tek_ornek::Baslangic::Kapali => None,
        }
    };
    tauri::Builder::default()
        .manage(Dosyalar::default())
        .setup(move |_uyg| {
            #[cfg(unix)]
            if let Some(d) = dinleyici {
                let uyg = _uyg.handle().clone();
                std::thread::spawn(move || {
                    for akis in d.incoming().flatten() {
                        if let Some(belge) = tek_ornek::istek_oku(akis) {
                            devralinani_ac(&uyg, belge);
                        }
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![dosya_kaydet, baslangic_dosyasi, imza_durum, imzala, imza_dogrula])
        .run(tauri::generate_context!())
        .expect("Armonia Yazı başlatılamadı");
}
