//! Büro veritabanı — ofis bilgisayarında gömülü PostgreSQL (Aşama 1, 15.11.2026).
//!
//! Kullanıcı kararı: veri büroda kalır, buluta hiçbir şey çıkmaz. PostgreSQL'i ofis bilgisayarındaki
//! (UYAP oturumunu paylaşan) Çoklu UYAP kendisi kurar, başlatır ve durdurur; ayrı kurulum yok.
//!
//! - İkili dosyalar: program klasöründeki `pgsql\` (kurulum paketi; `pgsql_hazirla` ile yalnız sunucu
//!   dosyaları alınır). Geliştirmede `COKLUUYAP_PGSQL` ya da `%LOCALAPPDATA%\UyapIcra\pgsql`.
//! - Veri: `%LOCALAPPDATA%\CokluUyap\pgdata`; ayar (port + DPAPI ile korunan parola):
//!   `%LOCALAPPDATA%\CokluUyap\veritabani.json`.
//! - Yalnız `127.0.0.1` dinlenir; PostgreSQL portu ağa AÇILMAZ. Diğer bilgisayarlar veriye ofis
//!   bağlantısı üzerinden Çoklu UYAP aracılığıyla ulaşır.
//! - Şema: `src/sema/NNNN_*.sql` dosyaları sırayla, `sema_surumu` tablosuyla bir kez uygulanır.
//! - Yedek: günde bir `pg_dump -Fc` → `%LOCALAPPDATA%\CokluUyap\yedek`, son 14 gün.
//!
//! DİKKAT: `postgres` crate'i eşzamanlı (blocking) istemcidir ve kendi içinde tokio çalıştırır;
//! async bağlamdan çağıranlar `tokio::task::spawn_blocking` kullanmalıdır.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub const VERITABANI: &str = "cokluuyap";
pub const KULLANICI: &str = "cokluuyap";
const VARSAYILAN_PORT: u16 = 54329;
const YEDEK_GUN: usize = 14;
const HAVUZ_EN_COK: usize = 8;

/// Uygulanacak şema dosyaları (sürüm, ad, SQL). Sürüm numarası değiştirilmez; yenisi eklenir.
pub static SEMALAR: &[(i32, &str, &str)] = &[
    (1, "baslangic", include_str!("sema/0001_baslangic.sql")),
    (2, "icra", include_str!("sema/0002_icra.sql")),
    (3, "is_listesi", include_str!("sema/0003_is_listesi.sql")),
    (4, "is_etiketleri", include_str!("sema/0004_is_etiketleri.sql")),
    (5, "hatirlatici_goruldu", include_str!("sema/0005_hatirlatici_goruldu.sql")),
    (6, "olay_turu", include_str!("sema/0006_olay_turu.sql")),
    (7, "tebligat_notu", include_str!("sema/0007_tebligat_notu.sql")),
    (8, "takip_turu", include_str!("sema/0008_takip_turu.sql")),
    (9, "kesinlesme_kurali", include_str!("sema/0009_kesinlesme_kurali.sql")),
    (10, "tebligat_grubu", include_str!("sema/0010_tebligat_grubu.sql")),
    (11, "elle_kesinlesme", include_str!("sema/0011_elle_kesinlesme.sql")),
    (12, "taraf_bilgisi_zamani", include_str!("sema/0012_taraf_bilgisi_zamani.sql")),
    (13, "kesinlestirme", include_str!("sema/0013_kesinlestirme.sql")),
    (14, "mazbata_bagi", include_str!("sema/0014_mazbata_bagi.sql")),
    (15, "mernis", include_str!("sema/0015_mernis.sql")),
    (16, "mazbata_teblig_tarihi", include_str!("sema/0016_mazbata_teblig_tarihi.sql")),
    (17, "dosya_listesi", include_str!("sema/0017_dosya_listesi.sql")),
    (18, "belge", include_str!("sema/0018_belge.sql")),
    (19, "belge_yol", include_str!("sema/0019_belge_yol.sql")),
    (20, "iban", include_str!("sema/0020_iban.sql")),
    (21, "mazbata_iade", include_str!("sema/0021_mazbata_iade.sql")),
];

pub type Gunluk = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
struct Calisan {
    bin: PathBuf,
    veri: PathBuf,
    port: u16,
    parola: String,
}

static CALISAN: Lazy<Mutex<Option<Calisan>>> = Lazy::new(|| Mutex::new(None));
static HAVUZ: Lazy<Mutex<Vec<postgres::Client>>> = Lazy::new(|| Mutex::new(Vec::new()));
/// baslat/durdur aynı anda iki kez çalışmasın.
static ISLEM: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// ── Yollar ──────────────────────────────────────────────────────────────────────────────────────

pub fn kok_dizin() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".to_string());
    let d = PathBuf::from(base).join("CokluUyap");
    let _ = fs::create_dir_all(&d);
    kisa_yol(&d)
}

/// PostgreSQL'in C araçları ASCII dışı yolda (ör. kullanıcı adı "WİN10") UTF-8 açılışında çöker
/// ("invalid byte sequence for UTF8"; UyapMasa db_baslat.py dersi). Windows'ta var olan yol 8.3 kısa
/// adına çevrilir; kısa ad üretilemezse yol olduğu gibi döner (bkz. `ascii_mi` denetimi).
pub fn kisa_yol(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
        let genis: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut tampon = vec![0u16; 1024];
        let n = unsafe { GetShortPathNameW(genis.as_ptr(), tampon.as_mut_ptr(), tampon.len() as u32) };
        if n > 0 && (n as usize) < tampon.len() {
            return PathBuf::from(std::ffi::OsString::from_wide(&tampon[..n as usize]));
        }
    }
    p.to_path_buf()
}

fn ascii_mi(p: &Path) -> bool {
    p.to_str().is_some_and(|s| s.is_ascii())
}

pub fn veri_dizini() -> PathBuf {
    kok_dizin().join("pgdata")
}

pub fn yedek_dizini() -> PathBuf {
    kok_dizin().join("yedek")
}

fn ayar_yolu() -> PathBuf {
    kok_dizin().join("veritabani.json")
}

fn exe_adi(ad: &str) -> String {
    if cfg!(windows) {
        format!("{ad}.exe")
    } else {
        ad.to_string()
    }
}

/// PostgreSQL `bin` klasörünü bulur (içinde `postgres` + `pg_ctl` + `initdb` olmalı).
pub fn bin_dizini() -> Option<PathBuf> {
    let mut adaylar: Vec<PathBuf> = Vec::new();
    if let Ok(e) = std::env::var("COKLUUYAP_PGSQL") {
        if !e.trim().is_empty() {
            adaylar.push(PathBuf::from(e.trim()).join("bin"));
        }
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        adaylar.push(dir.join("pgsql").join("bin"));
        adaylar.push(dir.join("_internal").join("pgsql").join("bin"));
    }
    adaylar.push(kok_dizin().join("pgsql").join("bin"));
    if let Ok(la) = std::env::var("LOCALAPPDATA") {
        // Geliştirme bilgisayarı: UyapMasa kurulumunun getirdiği PostgreSQL.
        adaylar.push(PathBuf::from(la).join("UyapIcra").join("pgsql").join("bin"));
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(rd) = fs::read_dir("/usr/lib/postgresql") {
            let mut s: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path().join("bin")).collect();
            s.sort();
            s.reverse();
            adaylar.extend(s);
        }
    }
    adaylar
        .into_iter()
        .find(|b| ["postgres", "pg_ctl", "initdb"].iter().all(|a| b.join(exe_adi(a)).is_file()))
        .map(|b| kisa_yol(&b))
}

// ── Ayar (port + parola) ────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct Ayar {
    port: u16,
    /// Windows: DPAPI ile korunmuş parolanın base64'ü; diğer sistemlerde düz (dosya 0600).
    parola: String,
}

fn ayar_oku() -> Option<Ayar> {
    let s = fs::read_to_string(ayar_yolu()).ok()?;
    serde_json::from_str(&s).ok()
}

fn ayar_yaz(a: &Ayar) -> Result<(), String> {
    let _ = fs::create_dir_all(kok_dizin());
    let s = serde_json::to_string_pretty(a).map_err(|e| e.to_string())?;
    fs::write(ayar_yolu(), s).map_err(|e| format!("veritabanı ayarı yazılamadı: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(ayar_yolu(), fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn parola_koru(p: &str) -> Result<String, String> {
    use base64::Engine;
    #[cfg(windows)]
    {
        let k = dpapi::koru(p.as_bytes())?;
        Ok(base64::engine::general_purpose::STANDARD.encode(k))
    }
    #[cfg(not(windows))]
    {
        let _ = base64::engine::general_purpose::STANDARD;
        Ok(p.to_string())
    }
}

fn parola_ac(s: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        use base64::Engine;
        let ham = base64::engine::general_purpose::STANDARD.decode(s).map_err(|e| e.to_string())?;
        let acik = dpapi::ac(&ham)?;
        String::from_utf8(acik).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        Ok(s.to_string())
    }
}

fn yeni_parola() -> String {
    use rand::Rng;
    const HARF: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
    let mut r = rand::thread_rng();
    (0..32).map(|_| HARF[r.gen_range(0..HARF.len())] as char).collect()
}

fn port_bos_mu(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn bos_port(tercih: u16) -> u16 {
    (tercih..tercih.saturating_add(200)).find(|p| port_bos_mu(*p)).unwrap_or(tercih)
}

// ── Süreç çalıştırma ────────────────────────────────────────────────────────────────────────────

fn komut(bin: &Path, ad: &str) -> Command {
    let mut c = Command::new(bin.join(exe_adi(ad)));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.env("LC_MESSAGES", "C").env_remove("PGDATA").env_remove("PGPORT");
    c
}

fn calistir(mut c: Command, ne: &str) -> Result<Output, String> {
    let o = c.output().map_err(|e| format!("{ne} çalıştırılamadı: {e}"))?;
    if o.status.success() {
        Ok(o)
    } else {
        let hata = String::from_utf8_lossy(&o.stderr);
        let cikti = String::from_utf8_lossy(&o.stdout);
        Err(format!("{ne} başarısız ({}): {} {}", o.status, hata.trim(), cikti.trim()))
    }
}

/// Veri klasöründe çalışan bir sunucu varsa portunu döndürür (`postmaster.pid` 4. satır).
fn calisan_port(bin: &Path, veri: &Path) -> Option<u16> {
    let mut c = komut(bin, "pg_ctl");
    c.arg("status").arg("-D").arg(veri);
    let o = c.output().ok()?;
    if !o.status.success() {
        return None;
    }
    let pid = fs::read_to_string(veri.join("postmaster.pid")).ok()?;
    pid.lines().nth(3)?.trim().parse().ok()
}

// ── Başlat / durdur ─────────────────────────────────────────────────────────────────────────────

pub fn hazir() -> bool {
    CALISAN.lock().is_some()
}

/// Bağlantı dizesi (İş Listesi deposu `ZsStore::new(Some(dsn), …)` için). Sunucu hazır değilse None.
pub fn dsn() -> Option<String> {
    CALISAN.lock().as_ref().map(|c| dsn_yap(c, VERITABANI))
}

fn dsn_yap(c: &Calisan, db: &str) -> String {
    format!(
        "host=127.0.0.1 port={} user={} password={} dbname={} connect_timeout=10 application_name=CokluUyap",
        c.port, KULLANICI, c.parola, db
    )
}

/// Gömülü PostgreSQL'i hazırlar ve başlatır; zaten çalışıyorsa yalnız bağlanır. Tekrar çağrılabilir.
/// Eşzamanlıdır (initdb ilk seferde ~10-30 sn) — arka plan iş parçacığından çağırın.
pub fn baslat(log: Gunluk) -> Result<(), String> {
    let _kilit = ISLEM.lock();
    if hazir() {
        return Ok(());
    }
    let bin = bin_dizini().ok_or_else(|| {
        "PostgreSQL dosyaları bulunamadı (program klasöründe pgsql\\bin yok)".to_string()
    })?;
    let veri = veri_dizini();
    for (ne, p) in [("PostgreSQL klasörü", &bin), ("veri klasörü", &kok_dizin())] {
        if !ascii_mi(p) {
            return Err(format!(
                "{ne} yolunda Türkçe/ASCII dışı karakter var ve kısa ad üretilemedi: {}",
                p.display()
            ));
        }
    }

    // 1) İlk kurulum
    let mut ayar = ayar_oku().unwrap_or_default();
    let ilk = !veri.join("PG_VERSION").is_file();
    let parola = if ilk {
        log("[VERİTABANI] Büro veritabanı ilk kez kuruluyor…");
        if veri.exists() {
            // Yarım kalmış kurulum (PG_VERSION yok): boşsa temizle, doluysa dokunma.
            let bos = fs::read_dir(&veri).map(|mut r| r.next().is_none()).unwrap_or(true);
            if !bos {
                return Err(format!("{} dolu ama PostgreSQL veri klasörü değil; elle inceleyin", veri.display()));
            }
            let _ = fs::remove_dir(&veri);
        }
        let parola = yeni_parola();
        let pw_dosya = kok_dizin().join(format!(".pw-{}", std::process::id()));
        fs::write(&pw_dosya, &parola).map_err(|e| format!("geçici parola dosyası yazılamadı: {e}"))?;
        let mut c = komut(&bin, "initdb");
        c.arg("-D").arg(&veri)
            .arg("-U").arg(KULLANICI)
            .arg("--auth=scram-sha-256")
            .arg(format!("--pwfile={}", pw_dosya.display()))
            .arg("-E").arg("UTF8")
            .arg("--locale=C")
            .arg("--no-instructions");
        let sonuc = calistir(c, "initdb");
        let _ = fs::remove_file(&pw_dosya);
        if let Err(e) = sonuc {
            let _ = fs::remove_dir_all(&veri);
            return Err(e);
        }
        conf_yaz(&veri)?;
        ayar.parola = parola_koru(&parola)?;
        ayar.port = bos_port(VARSAYILAN_PORT);
        ayar_yaz(&ayar)?;
        parola
    } else {
        parola_ac(&ayar.parola).map_err(|e| format!("veritabanı parolası okunamadı: {e}"))?
    };

    // 2) Çalışıyor mu? (önceki oturum çökmüş olabilir) — değilse başlat.
    let port = match calisan_port(&bin, &veri) {
        Some(p) => {
            log(&format!("[VERİTABANI] Çalışan sunucu bulundu (port {p})."));
            p
        }
        None => {
            if ayar.port == 0 || !port_bos_mu(ayar.port) {
                ayar.port = bos_port(if ayar.port == 0 { VARSAYILAN_PORT } else { ayar.port });
                ayar_yaz(&ayar)?;
            }
            // DİKKAT: çıktı YAKALANMAZ. pg_ctl'in başlattığı postgres süreci stdout/stderr
            // borularını miras alır; output() boru kapanmasını beklerken sonsuza dek takılır
            // (15.11 testinde görüldü). Ayrıntı -l günlük dosyasındadır.
            let gunluk_dosyasi = kok_dizin().join("pgdata.log");
            let mut c = komut(&bin, "pg_ctl");
            c.arg("start").arg("-D").arg(&veri)
                .arg("-l").arg(&gunluk_dosyasi)
                .arg("-w").arg("-t").arg("90")
                .arg("-o").arg(format!("-p {} -c listen_addresses=127.0.0.1", ayar.port))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            let durum = c.status().map_err(|e| format!("pg_ctl çalıştırılamadı: {e}"))?;
            if !durum.success() {
                let son: String = fs::read_to_string(&gunluk_dosyasi)
                    .unwrap_or_default()
                    .lines()
                    .rev()
                    .take(5)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" | ");
                return Err(format!("PostgreSQL başlatılamadı ({durum}): {son}"));
            }
            log(&format!("[VERİTABANI] Sunucu başlatıldı (127.0.0.1:{}).", ayar.port));
            ayar.port
        }
    };
    let calisan = Calisan { bin, veri, port, parola };

    // 3) Veritabanı + şema
    veritabani_olustur(&calisan)?;
    let mut istemci = postgres::Client::connect(&dsn_yap(&calisan, VERITABANI), postgres::NoTls)
        .map_err(|e| format!("veritabanına bağlanılamadı: {e}"))?;
    let uygulanan = sema_uygula(&mut istemci)?;
    if uygulanan > 0 {
        log(&format!("[VERİTABANI] Şema güncellendi ({uygulanan} adım)."));
    }
    // Büro belgeleri: eski içerik adresli depodakiler yargı türü / birim / dosya klasörlerine (bir kez)
    match crate::belge_veri::eski_belgeleri_yerlestir(&mut istemci) {
        Ok((0, h)) if h.is_empty() => {}
        Ok((n, h)) => {
            log(&format!("[VERİTABANI] {n} belge klasörlere taşındı."));
            for e in h {
                log(&format!("[VERİTABANI] Belge taşınamadı: {e}"));
            }
        }
        Err(e) => log(&format!("[VERİTABANI] Belgeler klasörlere taşınamadı: {e}")),
    }
    HAVUZ.lock().push(istemci);
    *CALISAN.lock() = Some(calisan);
    log("[VERİTABANI] Büro veritabanı hazır.");

    // 4) Günlük yedek (arka planda; sunucu durunca biter)
    let log2 = log.clone();
    std::thread::spawn(move || yedek_dongusu(log2));
    Ok(())
}

fn conf_yaz(veri: &Path) -> Result<(), String> {
    // Port ve dinleme adresi pg_ctl -o ile de verilir; burada kalıcı ve güvenli varsayılanlar.
    let ek = "\n# ── Çoklu UYAP (değiştirmeyin) ──\n\
              listen_addresses = '127.0.0.1'\n\
              max_connections = 40\n\
              password_encryption = 'scram-sha-256'\n\
              timezone = 'Europe/Istanbul'\n\
              log_timezone = 'Europe/Istanbul'\n\
              lc_messages = 'C'\n";
    let yol = veri.join("postgresql.conf");
    let mut s = fs::read_to_string(&yol).map_err(|e| format!("postgresql.conf okunamadı: {e}"))?;
    s.push_str(ek);
    fs::write(&yol, s).map_err(|e| format!("postgresql.conf yazılamadı: {e}"))?;
    // Yalnız yerel bağlantı, parolayla.
    let hba = "# Çoklu UYAP: yalnız bu bilgisayar, parolayla\n\
               host all all 127.0.0.1/32 scram-sha-256\n\
               host all all ::1/128 scram-sha-256\n";
    fs::write(veri.join("pg_hba.conf"), hba).map_err(|e| format!("pg_hba.conf yazılamadı: {e}"))
}

fn veritabani_olustur(c: &Calisan) -> Result<(), String> {
    let mut sys = postgres::Client::connect(&dsn_yap(c, "postgres"), postgres::NoTls)
        .map_err(|e| format!("PostgreSQL'e bağlanılamadı: {e}"))?;
    let var = sys
        .query_opt("SELECT 1 FROM pg_database WHERE datname = $1", &[&VERITABANI])
        .map_err(|e| e.to_string())?
        .is_some();
    if !var {
        sys.batch_execute(&format!(
            "CREATE DATABASE {VERITABANI} TEMPLATE template0 ENCODING 'UTF8' LC_COLLATE 'C' LC_CTYPE 'C'"
        ))
        .map_err(|e| format!("veritabanı oluşturulamadı: {e}"))?;
    }
    Ok(())
}

/// Eksik şema adımlarını sırayla uygular; uygulanan adım sayısını döndürür.
pub fn sema_uygula(istemci: &mut postgres::Client) -> Result<usize, String> {
    istemci
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS sema_surumu (
                 surum integer PRIMARY KEY,
                 ad    varchar(100) NOT NULL,
                 zaman timestamptz NOT NULL DEFAULT now()
             )",
        )
        .map_err(|e| format!("sema_surumu oluşturulamadı: {e}"))?;
    let mut n = 0;
    for (surum, ad, sql) in SEMALAR {
        let mut tx = istemci.transaction().map_err(|e| e.to_string())?;
        // Aynı anda iki süreç şema uygulamasın.
        tx.batch_execute("SELECT pg_advisory_xact_lock(4242001)").map_err(|e| e.to_string())?;
        let var = tx
            .query_opt("SELECT 1 FROM sema_surumu WHERE surum = $1", &[surum])
            .map_err(|e| e.to_string())?
            .is_some();
        if var {
            continue;
        }
        tx.batch_execute(sql).map_err(|e| format!("şema {surum:04}_{ad} uygulanamadı: {e}"))?;
        tx.execute("INSERT INTO sema_surumu (surum, ad) VALUES ($1, $2)", &[surum, ad])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// Sunucuyu düzgün kapatır (program çıkışında). Sunucu bu süreçte başlatılmadıysa bir şey yapmaz.
pub fn durdur(log: &dyn Fn(&str)) {
    let _kilit = ISLEM.lock();
    let Some(c) = CALISAN.lock().take() else { return };
    HAVUZ.lock().clear();
    let mut k = komut(&c.bin, "pg_ctl");
    k.arg("stop").arg("-D").arg(&c.veri).arg("-m").arg("fast").arg("-w").arg("-t").arg("30");
    match calistir(k, "PostgreSQL durdurma") {
        Ok(_) => log("[VERİTABANI] Sunucu durduruldu."),
        Err(e) => log(&format!("[VERİTABANI] {e}")),
    }
}

// ── Bağlantı havuzu ─────────────────────────────────────────────────────────────────────────────

/// Havuzdan alınan bağlantı; bırakılınca havuza döner.
pub struct Baglanti(Option<postgres::Client>);

impl std::ops::Deref for Baglanti {
    type Target = postgres::Client;
    fn deref(&self) -> &postgres::Client {
        self.0.as_ref().expect("bağlantı")
    }
}

impl std::ops::DerefMut for Baglanti {
    fn deref_mut(&mut self) -> &mut postgres::Client {
        self.0.as_mut().expect("bağlantı")
    }
}

impl Drop for Baglanti {
    fn drop(&mut self) {
        if let Some(c) = self.0.take() {
            if !c.is_closed() && hazir() {
                let mut h = HAVUZ.lock();
                if h.len() < HAVUZ_EN_COK {
                    h.push(c);
                }
            }
        }
    }
}

/// Büro veritabanına bağlantı (eşzamanlı; async kodda `spawn_blocking` içinde kullanın).
pub fn baglanti() -> Result<Baglanti, String> {
    loop {
        let eski = HAVUZ.lock().pop();
        match eski {
            Some(c) if !c.is_closed() => return Ok(Baglanti(Some(c))),
            Some(_) => continue,
            None => break,
        }
    }
    let d = dsn().ok_or_else(|| "büro veritabanı hazır değil".to_string())?;
    let c = postgres::Client::connect(&d, postgres::NoTls).map_err(|e| format!("veritabanına bağlanılamadı: {e}"))?;
    Ok(Baglanti(Some(c)))
}

// ── Yedek ───────────────────────────────────────────────────────────────────────────────────────

fn yedek_dongusu(log: Gunluk) {
    loop {
        let Some(c) = CALISAN.lock().clone() else { return };
        if let Err(e) = gunluk_yedek(&c) {
            log(&format!("[VERİTABANI] Yedek alınamadı: {e}"));
        }
        // Saatte bir bak; gün değişince yeni yedek. Sunucu durunca döngü biter.
        for _ in 0..360 {
            std::thread::sleep(Duration::from_secs(10));
            if !hazir() {
                return;
            }
        }
    }
}

fn gunluk_yedek(c: &Calisan) -> Result<(), String> {
    let dizin = yedek_dizini();
    fs::create_dir_all(&dizin).map_err(|e| e.to_string())?;
    let bugun = chrono::Local::now().format("%Y%m%d").to_string();
    let hedef = dizin.join(format!("{VERITABANI}_{bugun}.dump"));
    if hedef.is_file() {
        return Ok(());
    }
    let gecici = dizin.join(format!("{VERITABANI}_{bugun}.dump.yarim"));
    let mut k = komut(&c.bin, "pg_dump");
    k.arg("-Fc").arg("-h").arg("127.0.0.1").arg("-p").arg(c.port.to_string())
        .arg("-U").arg(KULLANICI).arg("-f").arg(&gecici).arg(VERITABANI)
        .env("PGPASSWORD", &c.parola);
    if let Err(e) = calistir(k, "pg_dump") {
        let _ = fs::remove_file(&gecici);
        return Err(e);
    }
    fs::rename(&gecici, &hedef).map_err(|e| e.to_string())?;
    // Eskileri sil (son YEDEK_GUN dosya kalır).
    let mut dosyalar: Vec<PathBuf> = fs::read_dir(&dizin)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with(&format!("{VERITABANI}_")) && n.ends_with(".dump")
            })
        })
        .collect();
    dosyalar.sort();
    while dosyalar.len() > YEDEK_GUN {
        let _ = fs::remove_file(dosyalar.remove(0));
    }
    Ok(())
}

// ── Kurulum paketi: yalnız sunucu dosyaları ─────────────────────────────────────────────────────

/// Tam bir PostgreSQL kurulumundan (`kaynak`, içinde bin/lib/share) yalnız gömülü sunucu için
/// gerekenleri `hedef`e kopyalar (pgAdmin, StackBuilder, belgeler, başlık dosyaları, çeviriler,
/// .lib/.a dosyaları alınmaz). Kopyalanan toplam bayt sayısını döndürür.
/// Kullanım: `uyap_app --pgsql-hazirla <kaynak> <hedef>`; hedef paketin `pgsql\` klasörü olur.
pub fn pgsql_hazirla(kaynak: &Path, hedef: &Path) -> Result<u64, String> {
    const BIN_EXE: &[&str] = &["postgres", "pg_ctl", "initdb", "pg_dump", "pg_restore", "psql"];
    let mut toplam = 0u64;
    let kopyala = |a: &Path, b: &Path, toplam: &mut u64| -> Result<(), String> {
        if let Some(p) = b.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        *toplam += fs::copy(a, b).map_err(|e| format!("{}: {e}", a.display()))?;
        Ok(())
    };
    // bin: seçili exe'ler + dll'ler (pgAdmin'in wx* kitaplıkları ve test eklentisi hariç)
    let kb = kaynak.join("bin");
    for e in fs::read_dir(&kb).map_err(|e| format!("{}: {e}", kb.display()))?.flatten() {
        let ad = e.file_name().to_string_lossy().to_string();
        let kucuk = ad.to_lowercase();
        let al = (if let Some(govde) = kucuk.strip_suffix(".exe") {
            BIN_EXE.contains(&govde)
        } else {
            kucuk.ends_with(".dll")
                && !kucuk.starts_with("wx")
                && !kucuk.starts_with("testplug")
                && !kucuk.starts_with("libecpg")
                && !kucuk.starts_with("libpgtypes")
        }) || (!cfg!(windows) && BIN_EXE.contains(&kucuk.as_str()));
        if al {
            kopyala(&e.path(), &hedef.join("bin").join(&ad), &mut toplam)?;
        }
    }
    // lib: yalnız sunucu eklentileri (.dll / .so)
    let kl = kaynak.join("lib");
    for e in fs::read_dir(&kl).map_err(|e| format!("{}: {e}", kl.display()))?.flatten() {
        let ad = e.file_name().to_string_lossy().to_string();
        let k = ad.to_lowercase();
        if e.path().is_file() && (k.ends_with(".dll") || k.ends_with(".so")) {
            kopyala(&e.path(), &hedef.join("lib").join(&ad), &mut toplam)?;
        }
    }
    // share: hepsi, çeviriler (locale) ve belgeler hariç
    fn agac(a: &Path, b: &Path, toplam: &mut u64) -> Result<(), String> {
        for e in fs::read_dir(a).map_err(|e| format!("{}: {e}", a.display()))?.flatten() {
            let ad = e.file_name();
            let p = e.path();
            if p.is_dir() {
                let s = ad.to_string_lossy().to_lowercase();
                if s == "locale" || s == "doc" {
                    continue;
                }
                agac(&p, &b.join(&ad), toplam)?;
            } else {
                fs::create_dir_all(b).map_err(|e| e.to_string())?;
                *toplam += fs::copy(&p, b.join(&ad)).map_err(|e| format!("{}: {e}", p.display()))?;
            }
        }
        Ok(())
    }
    agac(&kaynak.join("share"), &hedef.join("share"), &mut toplam)?;
    for lisans in ["server_license.txt", "commandlinetools_3rd_party_licenses.txt"] {
        let p = kaynak.join(lisans);
        if p.is_file() {
            kopyala(&p, &hedef.join(lisans), &mut toplam)?;
        }
    }
    Ok(toplam)
}

// ── Windows DPAPI (kullanıcıya bağlı koruma) ────────────────────────────────────────────────────

#[cfg(windows)]
mod dpapi {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob_al(b: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let v = unsafe { std::slice::from_raw_parts(b.pbData, b.cbData as usize) }.to_vec();
        unsafe {
            LocalFree(b.pbData as _);
        }
        v
    }

    pub fn koru(veri: &[u8]) -> Result<Vec<u8>, String> {
        let giris = CRYPT_INTEGER_BLOB { cbData: veri.len() as u32, pbData: veri.as_ptr() as *mut u8 };
        let mut cikis = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let ok = unsafe {
            CryptProtectData(&giris, null(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut cikis)
        };
        if ok == 0 {
            return Err("CryptProtectData başarısız".into());
        }
        Ok(blob_al(&cikis))
    }

    pub fn ac(veri: &[u8]) -> Result<Vec<u8>, String> {
        let giris = CRYPT_INTEGER_BLOB { cbData: veri.len() as u32, pbData: veri.as_ptr() as *mut u8 };
        let mut cikis = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let ok = unsafe {
            CryptUnprotectData(&giris, null_mut(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut cikis)
        };
        if ok == 0 {
            return Err("CryptUnprotectData başarısız (başka kullanıcı / bilgisayar?)".into());
        }
        Ok(blob_al(&cikis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sema_surumleri_artan_ve_benzersiz() {
        let mut onceki = 0;
        for (s, ad, sql) in SEMALAR {
            assert!(*s > onceki, "sürümler artan olmalı: {s}");
            assert!(!ad.is_empty() && !sql.trim().is_empty());
            onceki = *s;
        }
    }

    #[test]
    fn parola_bicimi() {
        let p = yeni_parola();
        assert_eq!(p.len(), 32);
        assert!(p.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(p, yeni_parola());
    }

    /// Gerçek PostgreSQL ile uçtan uca (geçici LOCALAPPDATA). Çalıştırma:
    /// `COKLUUYAP_PGSQL=<pgsql klasörü> cargo test --lib veritabani::tests::uctan_uca -- --ignored`
    #[test]
    #[ignore]
    fn uctan_uca() {
        // Bilerek ASCII dışı ad: kısa yol çevirisi de sınanır.
        let kok = std::env::temp_dir().join(format!("cu-db-İ-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&kok);
        fs::create_dir_all(&kok).unwrap();
        std::env::set_var("LOCALAPPDATA", &kok);
        let satirlar = Arc::new(Mutex::new(Vec::<String>::new()));
        let s2 = satirlar.clone();
        let log: Gunluk = Arc::new(move |m: &str| {
            println!("{m}");
            s2.lock().push(m.to_string());
        });

        baslat(log.clone()).expect("ilk başlatma");
        assert!(hazir());
        assert!(baslat(log.clone()).is_ok(), "ikinci çağrı sorunsuz");
        {
            let mut b = baglanti().unwrap();
            let n: i64 = b.query_one("SELECT count(*) FROM sema_surumu", &[]).unwrap().get(0);
            assert_eq!(n as usize, SEMALAR.len());
            let la: String = b.query_one("SHOW listen_addresses", &[]).unwrap().get(0);
            assert_eq!(la, "127.0.0.1");
            let tz: String = b.query_one("SHOW timezone", &[]).unwrap().get(0);
            assert_eq!(tz, "Europe/Istanbul");
            b.execute(
                "INSERT INTO buro_ayar (anahtar, deger) VALUES ('deneme', '{\"a\": 1}')",
                &[],
            )
            .unwrap();
            assert_eq!(sema_uygula(&mut b).unwrap(), 0, "şema yeniden uygulanmaz");
            crate::icra_veri::veritabani_senaryosu(&mut b);
            crate::icra_tarama::veritabani_senaryosu(&mut b);
            crate::icra_durum::veritabani_senaryosu(&mut b);
            crate::icra_api::veritabani_senaryosu(&mut b);
            crate::icra_tebligatlar::veritabani_senaryosu(&mut b);
            crate::icra_dosyalar::veritabani_senaryosu(&mut b);
            crate::icra_guncelle::veritabani_senaryosu(&mut b);
            crate::is_veri::veritabani_senaryosu(&mut b);
            crate::is_otomatik::veritabani_senaryosu(&mut b);
            crate::icra_kesinlestirme::veritabani_senaryosu(&mut b);
            crate::icra_listeler::veritabani_senaryosu(&mut b);
            crate::is_veri::ek_senaryosu(&mut b);
            crate::belge_veri::veritabani_senaryosu(&mut b);
            crate::is_aktar::veritabani_senaryosu(&mut b);
            crate::is_api::veritabani_senaryosu(&mut b);
            crate::is_isaret::veritabani_senaryosu(&mut b);
            crate::iban::veritabani_senaryosu(&mut b);
        }
        // Parola dosyada açık metin olarak durmaz
        let ayar = fs::read_to_string(ayar_yolu()).unwrap();
        let c = CALISAN.lock().clone().unwrap();
        assert!(!ayar.contains(&c.parola) || !cfg!(windows));
        // Yedek
        for _ in 0..60 {
            // Yarım (.yarim) dosya sayılmaz: döküm bitip adı değişene kadar beklenir.
            let bitti = fs::read_dir(yedek_dizini())
                .map(|r| r.flatten().any(|e| e.path().extension().is_some_and(|u| u == "dump")))
                .unwrap_or(false);
            if bitti {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let yedekler: Vec<_> = fs::read_dir(yedek_dizini()).unwrap().flatten().collect();
        assert_eq!(yedekler.len(), 1, "günlük yedek alınmalı");
        assert!(yedekler[0].path().extension().is_some_and(|u| u == "dump"), "yedek tamamlanmış olmalı");

        durdur(&|m| println!("{m}"));
        assert!(!hazir());
        assert!(calisan_port(&c.bin, &c.veri).is_none(), "sunucu durmuş olmalı");

        // Yeniden açılış: aynı veri, aynı parola
        baslat(log.clone()).expect("yeniden başlatma");
        {
            let mut b = baglanti().unwrap();
            let n: i64 = b.query_one("SELECT count(*) FROM buro_ayar WHERE anahtar='deneme'", &[]).unwrap().get(0);
            assert_eq!(n, 1);
        }
        // Çökmüş oturum taklidi: durum bellekten silinir, sunucu çalışır kalır → yeniden bağlanır
        *CALISAN.lock() = None;
        HAVUZ.lock().clear();
        baslat(log.clone()).expect("çalışan sunucuya bağlanma");
        assert!(satirlar.lock().iter().any(|s| s.contains("Çalışan sunucu bulundu")));
        durdur(&|m| println!("{m}"));
        let _ = fs::remove_dir_all(&kok);
    }

    #[cfg(windows)]
    #[test]
    fn parola_koruma_geri_doner() {
        let k = parola_koru("deneme-123").unwrap();
        assert_ne!(k, "deneme-123");
        assert_eq!(parola_ac(&k).unwrap(), "deneme-123");
    }
}
