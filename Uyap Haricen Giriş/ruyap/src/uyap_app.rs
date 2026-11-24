//! UYAP Ağ Geçidi - Birleşik Masaüstü Kontrol Ekranı (uyap_app.rs)
//! ----------------------------------------------------------------
//! `uyap_app.py` (1981 satır) dosyasının 1:1 Rust/egui karşılığı.

use base64::Engine;
use eframe::egui;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio_tungstenite::tungstenite::Message;

use crate::sira_kapisi_arayuz::{
    msg_ask_yes_no, msg_error, open_file_shell, BotKurulumPenceresi, SiraKapisiSekmesi,
};
use crate::uyap_theme::{configure_egui_fonts, round_rect, ButtonKind, C, Fonts, RoundButton};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const DEFAULT_SERVER_URL: &str = "wss://www.cokluuyap.com/ws";
pub const LOCAL_PORT: u16 = 8800;
const _PASSPHRASE_ENV: &str = "UYAP_CONFIG_SECRET";
const STARTUP_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_RUN_NAME: &str = "CokluUyap";
const GECIT_SCRIPT_PREFIX: &str = "/__gecit__";
const LOGIN_CARD_W: f32 = 412.0;
const LOGIN_CARD_MARGIN: f32 = 26.0;

static WARNED: once_cell::sync::Lazy<Mutex<HashSet<String>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashSet::new()));
static GECIT_PROCESS: once_cell::sync::Lazy<Mutex<Option<Child>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(None));

/// Arka plan bileşenlerinin (vekil, e-imza girişi, ofis ajanı) println! satırları Kontrol Ekranı günlüğüne
/// de düşsün (Armonia'da kullanıcı sistem günlüğünü göremez). Kanal UyapApp::new'de kurulur.
static UI_LOG_TX: once_cell::sync::Lazy<Mutex<Option<Sender<String>>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(None));

/// stdout ve stderr'i bir boruya yönlendirir; okuyucu iş parçacığı her satırı özgün stdout'a (journal) yazar,
/// "[" ile başlayan durum satırlarını da arayüz günlüğüne iletir. Yalnız Linux (Armonia). 2026-11-14.
#[cfg(target_os = "linux")]
fn stdout_arayuze_akit() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::FromRawFd;
    // SAFETY: yalnız yeni oluşturulan tanımlayıcılar ve 1/2 üzerinde pipe/dup/dup2/close; başarısızlıkta
    // hiçbir şey değiştirilmeden çıkılır. Her fd'nin sahipliği tek bir File'a devredilir.
    let (okur, orijinal) = unsafe {
        let mut fds = [0i32; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        let orijinal = libc::dup(1);
        if orijinal < 0 || libc::dup2(fds[1], 1) < 0 {
            if orijinal >= 0 {
                libc::close(orijinal);
            }
            libc::close(fds[0]);
            libc::close(fds[1]);
            return;
        }
        libc::dup2(fds[1], 2);
        libc::close(fds[1]);
        (std::fs::File::from_raw_fd(fds[0]), std::fs::File::from_raw_fd(orijinal))
    };
    std::thread::spawn(move || {
        let mut cikis = orijinal;
        for satir in BufReader::new(okur).split(b'\n') {
            let Ok(b) = satir else { break };
            let _ = cikis.write_all(&b);
            let _ = cikis.write_all(b"\n");
            let s = String::from_utf8_lossy(&b);
            let s = s.trim();
            crate::sifreli_gunluk::yaz(s);
            if s.starts_with('[') {
                if let Some(tx) = UI_LOG_TX.lock().unwrap().as_ref() {
                    let _ = tx.send(format!("{s}\n"));
                }
            }
        }
    });
}

/// Windows: GUI programında println! hiçbir yere gitmiyordu. stdout/stderr bir boruya yönlendirilir; okuyucu
/// iş parçacığı her satırı şifreli günlüğe yazar, "[" ile başlayan durum satırlarını arayüze iletir
/// (bağlantı durumu / hata kodu buradan çıkar). Rust'ın stdout'u her yazımda GetStdHandle'ı sorar. 2026-11-14.
#[cfg(windows)]
fn stdout_arayuze_akit() {
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    use windows_sys::Win32::System::Pipes::CreatePipe;
    let mut okur = std::ptr::null_mut();
    let mut yazar = std::ptr::null_mut();
    // SAFETY: CreatePipe iki yeni tanıtıcı döndürür; yazar uç süreç boyunca std tanıtıcısı olarak kalır,
    // okur ucun sahipliği tek bir File'a devredilir.
    let okur_dosya = unsafe {
        if CreatePipe(&mut okur, &mut yazar, std::ptr::null(), 0) == 0 {
            return;
        }
        SetStdHandle(STD_OUTPUT_HANDLE, yazar);
        SetStdHandle(STD_ERROR_HANDLE, yazar);
        std::fs::File::from_raw_handle(okur as _)
    };
    std::thread::spawn(move || {
        for satir in BufReader::new(okur_dosya).split(b'\n') {
            let Ok(b) = satir else { break };
            let s = String::from_utf8_lossy(&b);
            let s = s.trim();
            crate::sifreli_gunluk::yaz(s);
            if s.starts_with('[') {
                if let Some(tx) = UI_LOG_TX.lock().unwrap().as_ref() {
                    let _ = tx.send(format!("{s}\n"));
                }
            }
        }
    });
}

fn warn_once(key: &str, msg: &str) {
    let mut w = WARNED.lock().unwrap();
    if w.insert(key.to_string()) {
        println!("[GÜVENLİK] {msg}");
    }
}

pub fn workspace_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".to_string());
    let dir = PathBuf::from(base).join("CokluUyap");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn src_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            if p.join("uyap_gecit").join("web").join("manage.py").is_file() {
                return p.to_path_buf();
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    workspace_dir().join("uyap_app_config.json")
}

pub fn startup_shortcut_eski() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    PathBuf::from(appdata)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup")
        .join("CokluUyapOfis.lnk")
}

// ── Port temizleme ────────────────────────────────────────────────────────────────────────

pub fn free_port_if_busy<F: FnMut(&str)>(port: u16, mut log: F) {
    if TcpListener::bind(("0.0.0.0", port)).is_ok() {
        return;
    }

    if !cfg!(windows) {
        log(&format!(
            "[SİSTEM] {port} portu meşgul; lütfen önceki örneği kapatıp tekrar deneyin.\n"
        ));
        return;
    }

    let out = match Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .creation_flags_win()
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(e) => {
            log(&format!(
                "[SİSTEM] Port kontrolü yapılamadı ({e}); paylaşım yine de denenecek.\n"
            ));
            return;
        }
    };

    let suffix = format!(":{port}");
    let mut pids = HashSet::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5
            && parts[3].eq_ignore_ascii_case("LISTENING")
            && parts[1].ends_with(&suffix)
        {
            pids.insert(parts[4].to_string());
        }
    }

    if pids.is_empty() {
        log(&format!(
            "[SİSTEM] {port} portu meşgul; lütfen önceki örneği kapatıp tekrar deneyin.\n"
        ));
        return;
    }

    for pid in pids {
        let tl = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags_win()
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase())
            .unwrap_or_default();

        if tl.contains("python") || tl.contains("cokluuyap") {
            log(&format!(
                "[SİSTEM] {port} portunu tutan eski UYAP süreci (PID {pid}) kapatılıyor...\n"
            ));
            match Command::new("taskkill")
                .args(["/F", "/PID", &pid])
                .creation_flags_win()
                .output()
            {
                Ok(_) => {}
                Err(e) => log(&format!(
                    "[SİSTEM] Eski süreç kapatılamadı (PID {pid}): {e}\n"
                )),
            }
        } else {
            log(&format!(
                "[SİSTEM] UYARI: {port} portunu python olmayan bir uygulama (PID {pid}) tutuyor; dokunulmadı.\n"
            ));
        }
    }

    std::thread::sleep(Duration::from_millis(800));
}

trait CmdWinExt {
    fn creation_flags_win(&mut self) -> &mut Self;
}

impl CmdWinExt for Command {
    fn creation_flags_win(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            self.creation_flags(CREATE_NO_WINDOW);
        }
        self
    }
}

// ── DPAPI Şifreleme ───────────────────────────────────────────────────────────────────────

#[cfg(windows)]
fn dpapi_crypt(protect: bool, data: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    let mut blob_in = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut blob_out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe {
        if protect {
            CryptProtectData(
                &mut blob_in,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut blob_out,
            )
        } else {
            CryptUnprotectData(
                &mut blob_in,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut blob_out,
            )
        }
    };

    if ok == 0 {
        return Err(format!("DPAPI hatası: {}", std::io::Error::last_os_error()));
    }

    let out = unsafe {
        let slice = std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize);
        let vec = slice.to_vec();
        LocalFree(blob_out.pbData as *mut core::ffi::c_void);
        vec
    };
    Ok(out)
}

#[cfg(not(windows))]
fn dpapi_crypt(_protect: bool, _data: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI yalnızca Windows üzerinde kullanılabilir.".to_string())
}

pub fn encrypt_secret(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    if cfg!(windows) {
        match dpapi_crypt(true, plain.as_bytes()) {
            Ok(enc) => {
                return format!(
                    "dpapi:{}",
                    base64::engine::general_purpose::STANDARD.encode(&enc)
                );
            }
            Err(e) => {
                warn_once(
                    "dpapi_enc",
                    &format!("DPAPI şifreleme başarısız ({e}); passphrase fallback denenecek."),
                );
            }
        }
    }
    warn_once(
        "no_enc",
        &format!(
            "Sır güvenli şekilde şifrelenemedi -> DİSKE YAZILMADI (fail-closed). \
             Windows dışında saklamak için {_PASSPHRASE_ENV} ortam değişkenini ayarlayın."
        ),
    );
    String::new()
}

pub fn decrypt_secret(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    if let Some(rest) = token.strip_prefix("dpapi:") {
        if !cfg!(windows) {
            warn_once(
                "dpapi_other_os",
                "Kayıtlı sır Windows DPAPI ile şifrelenmiş; bu OS'te çözülemez (yeniden girmeniz gerekir).",
            );
            return String::new();
        }
        if let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(rest.as_bytes()) {
            if let Ok(plain) = dpapi_crypt(false, &raw) {
                if let Ok(s) = String::from_utf8(plain) {
                    return s;
                }
            }
        }
        return String::new();
    }
    if token.starts_with("pbk:") {
        if std::env::var(_PASSPHRASE_ENV).unwrap_or_default().is_empty() {
            warn_once(
                "pbk_dec",
                &format!(
                    "{_PASSPHRASE_ENV} ayarlı değil; kayıtlı sır çözülemiyor (yeniden girmeniz gerekebilir)."
                ),
            );
        }
        return String::new();
    }
    token.to_string()
}

// ── Config ────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub server_url: String,
    pub username: String,
    pub office_code: String,
    pub remember: bool,
    pub password_enc: String,
    pub cert_id: String,
    pub local_port: u16,
    pub auto_share: bool,
    pub lan_share: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_SERVER_URL.to_string(),
            username: String::new(),
            office_code: String::new(),
            remember: true,
            password_enc: String::new(),
            cert_id: String::new(),
            local_port: LOCAL_PORT,
            auto_share: true,
            lan_share: false,
        }
    }
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    let mut cfg = AppConfig::default();
    let mut had_pin_enc = false;

    if path.exists() {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(mut val) = serde_json::from_str::<Value>(&raw) {
                if let Some(obj) = val.as_object_mut() {
                    if obj.remove("pin_enc").is_some() {
                        had_pin_enc = true;
                    }
                    if let Ok(parsed) = serde_json::from_value::<AppConfig>(Value::Object(obj.clone())) {
                        cfg = parsed;
                    }
                }
            }
        }
    }

    if let Ok(env_url) = std::env::var("UYAP_SERVER_URL") {
        let u = env_url.trim().to_string();
        if !u.is_empty() {
            cfg.server_url = u;
        }
    }

    if had_pin_enc {
        save_config(&cfg);
    }

    cfg
}

fn restrict_perms(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    {
        if let Ok(kullanici) = std::env::var("USERNAME") {
            let k = kullanici.trim();
            if !k.is_empty() {
                let _ = Command::new("icacls")
                    .args([
                        &path.to_string_lossy().into_owned(),
                        "/inheritance:r",
                        "/grant:r",
                        &format!("{k}:(F)"),
                    ])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            }
        }
    }
}

pub fn save_config(cfg: &AppConfig) {
    let path = config_path();
    let tmp = PathBuf::from(format!("{}.tmp", path.to_string_lossy()));
    match serde_json::to_string_pretty(cfg) {
        Ok(json_str) => {
            if let Err(e) = fs::write(&tmp, json_str.as_bytes()) {
                println!("[GUI] Ayar kaydedilemedi: {e}");
                return;
            }
            restrict_perms(&tmp);
            if let Err(e) = fs::rename(&tmp, &path) {
                let _ = fs::copy(&tmp, &path);
                let _ = fs::remove_file(&tmp);
                let _ = e;
            }
            restrict_perms(&path);
        }
        Err(e) => println!("[GUI] Ayar kaydedilemedi: {e}"),
    }
}

// ── Windows Başlangıç kaydı ───────────────────────────────────────────────────────────────

pub fn sync_startup_shortcut<F: FnMut(&str)>(enable: bool, mut log: F) {
    if !cfg!(windows) {
        return;
    }
    let eski = startup_shortcut_eski();
    if eski.exists() {
        let _ = fs::remove_file(&eski);
    }

    let key_path = format!(r"HKCU\{STARTUP_RUN_KEY}");
    if !enable {
        let out = Command::new("reg")
            .args(["delete", &key_path, "/v", STARTUP_RUN_NAME, "/f"])
            .creation_flags_win()
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                log("[SİSTEM] Başlangıç kaydı kaldırıldı.\n");
            }
        }
        return;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(e) => {
            log(&format!("[SİSTEM] Başlangıç kaydı ayarlanamadı: {e}\n"));
            return;
        }
    };
    let val = format!("\"{exe}\"");
    match Command::new("reg")
        .args([
            "add",
            &key_path,
            "/v",
            STARTUP_RUN_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &val,
            "/f",
        ])
        .creation_flags_win()
        .output()
    {
        Ok(o) if o.status.success() => {
            log("[SİSTEM] Başlangıç kaydı yapıldı: bilgisayar açılınca paylaşım kendiliğinden başlar.\n");
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            log(&format!("[SİSTEM] Başlangıç kaydı ayarlanamadı: {err}\n"));
        }
        Err(e) => {
            log(&format!("[SİSTEM] Başlangıç kaydı ayarlanamadı: {e}\n"));
        }
    }
}

// ── Yerel Django arayüzü (uyap_gecit/web) ─────────────────────────────────────────────────

pub fn start_gecit_server<F: FnMut(&str) + Clone>(mut log: F) {
    let mut guard = GECIT_PROCESS.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        if let Ok(None) = child.try_wait() {
            return;
        }
    }
    let gecit_dir = src_dir().join("uyap_gecit").join("web");
    let manage_py = gecit_dir.join("manage.py");
    if !manage_py.is_file() {
        log("[SİSTEM] Geçit kaynağı bulunamadı, 'Geçidi Aç' devre dışı kalacak.\n");
        return;
    }
    free_port_if_busy(8000, log.clone());
    match Command::new("python")
        .args([
            &manage_py.to_string_lossy().into_owned(),
            "runserver",
            "127.0.0.1:8000",
            "--noreload",
            "--insecure",
        ])
        .current_dir(&gecit_dir)
        .env("UYAP_GECIT_SCRIPT_NAME", GECIT_SCRIPT_PREFIX)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags_win()
        .spawn()
    {
        Ok(child) => {
            *guard = Some(child);
            log("[SİSTEM] Geçit yerel sunucusu başlatıldı (127.0.0.1:8000).\n");
        }
        Err(e) => {
            log(&format!("[SİSTEM] Geçit başlatılamadı: {e}\n"));
        }
    }
}

pub fn stop_gecit_server<F: FnMut(&str)>(mut log: F) {
    let mut guard = GECIT_PROCESS.lock().unwrap();
    if let Some(mut child) = guard.take() {
        if let Ok(None) = child.try_wait() {
            if let Err(e) = child.kill() {
                log(&format!("[SİSTEM] Geçit süreci durdurulurken hata: {e}\n"));
            }
        }
    }
}

pub fn detect_ips() -> Vec<String> {
    let mut ips = Vec::new();
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(SocketAddr::V4(addr)) = sock.local_addr() {
                ips.push(addr.ip().to_string());
            }
        }
    }
    ips
}

// ── Giriş Doğrulama ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LoginInfo {
    pub role: String,
    pub office_code: String,
    pub office_label: String,
}

pub type LoginResult = (bool, String, LoginInfo);

fn verify_credentials_thread(
    url: String,
    username: String,
    password: String,
    office_code: String,
    tx: Sender<LoginResult>,
) {
    let rt = match crate::build_tokio_runtime() {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send((false, format!("Çalışma zamanı hatası: {e}"), LoginInfo::default()));
            return;
        }
    };

    rt.block_on(async move {
        let connect_fut = tokio_tungstenite::connect_async(&url);
        let (mut ws, _) = match tokio::time::timeout(Duration::from_secs(30), connect_fut).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => {
                let _ = tx.send((false, format!("Bağlantı başarısız: {e}"), LoginInfo::default()));
                return;
            }
            Err(_) => {
                let _ = tx.send((
                    false,
                    "Sunucu zaman aşımına uğradı. Sunucu uykudaysa birkaç saniye bekleyip tekrar deneyin.".to_string(),
                    LoginInfo::default(),
                ));
                return;
            }
        };

        let payload = json!({
            "role": "probe",
            "room": username,
            "office_code": office_code,
            "password": password,
        })
        .to_string();

        if let Err(e) = ws.send(Message::Text(payload)).await {
            let _ = tx.send((false, format!("Bağlantı başarısız: {e}"), LoginInfo::default()));
            return;
        }

        let msg = match tokio::time::timeout(Duration::from_secs(20), ws.next()).await {
            Ok(Some(Ok(Message::Text(txt)))) => txt,
            Ok(Some(Ok(_))) => {
                let _ = tx.send((false, "Beklenmeyen sunucu yanıtı.".to_string(), LoginInfo::default()));
                return;
            }
            Ok(Some(Err(e))) => {
                let _ = tx.send((false, format!("Bağlantı başarısız: {e}"), LoginInfo::default()));
                return;
            }
            Ok(None) => {
                let _ = tx.send((false, "Sunucu bağlantıyı kapattı.".to_string(), LoginInfo::default()));
                return;
            }
            Err(_) => {
                let _ = tx.send((
                    false,
                    "Sunucu zaman aşımına uğradı. Sunucu uykudaysa birkaç saniye bekleyip tekrar deneyin.".to_string(),
                    LoginInfo::default(),
                ));
                return;
            }
        };

        let parsed: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send((false, format!("Bağlantı başarısız: {e}"), LoginInfo::default()));
                return;
            }
        };

        let mtype = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match mtype {
            "joined" => {
                let info = LoginInfo {
                    role: parsed.get("role").and_then(|v| v.as_str()).unwrap_or("member").to_string(),
                    office_code: parsed.get("office_code").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    office_label: parsed.get("office_label").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                };
                let _ = tx.send((true, "Başarılı".to_string(), info));
            }
            "error" => {
                let err = parsed.get("error").and_then(|v| v.as_str()).unwrap_or("Bilinmeyen hata.").to_string();
                let _ = tx.send((false, err, LoginInfo::default()));
            }
            other => {
                let _ = tx.send((
                    false,
                    format!("Beklenmeyen sunucu yanıtı: {other}"),
                    LoginInfo::default(),
                ));
            }
        }
    });
}

// ── Yardımcı UI veri yapıları ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenMode {
    Login,
    Dashboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavTab {
    Baglanti,
    SiraKapisi,
}


struct ForgotPasswordDialog {
    ident: String,
    status: String,
    status_ok: bool,
    rx: Option<Receiver<Result<Value, String>>>,
    close: bool,
}




/// Ofis kartındaki bağlantı durumu.
#[derive(Clone, PartialEq)]
enum BaglantiDurumu {
    Kapali,
    Baglaniyor,
    Kuruldu,
    Hata { kod: &'static str, aciklama: String },
}

// ── Ana Uygulama ──────────────────────────────────────────────────────────────────────────

pub struct UyapApp {
    cfg: AppConfig,
    screen: ScreenMode,
    nav: NavTab,

    // Log kuyruğu
    log_tx: Sender<String>,
    log_rx: Receiver<String>,
    /// Ofis kartındaki tek satırlık bağlantı durumu (günlük kutusunun yerine; 2026-11-14).
    durum: BaglantiDurumu,
    /// Windows tepsi simgesi (kur: main'deki oluşturma kapanışı).
    #[cfg(windows)]
    pub tepsi: Option<crate::tepsi::Tepsi>,
    /// Otomatik paylaşımla açılışta pencere tepside başlar (ilk karelerden sonra gizlenir).
    tepside_baslat: bool,
    kare: u64,
    /// Başlıktaki tema seçici açık mı.
    tema_menu_acik: bool,
    /// Temalı tepsi paneli açıksa sol üst konumu (nokta) ve odak alıp almadığı.
    tepsi_panel: Option<egui::Pos2>,
    tepsi_panel_odak: bool,

    // Giriş durumu
    login_var: String,
    pass_var: String,
    remember_var: bool,
    auto_share_var: bool,
    login_pin_var: String,
    login_status_text: String,
    login_status_color: egui::Color32,
    login_in_progress: bool,
    login_rx: Option<Receiver<LoginResult>>,
    login_screen_shown: bool,
    auto_login_deadline: Option<Instant>,
    login_height_last: f32,
    login_height_pending: f32,
    /// Kontrol ekranı büyütülmüşse önceki (konum, iç boyut); None = normal boyut.
    dash_onceki: Option<(egui::Pos2, egui::Vec2)>,
    /// false: girişten sonra sade panel (durum + Bağlantıyı paylaş / UYAP'ı aç). true: eski ayrıntılı Kontrol Ekranı.
    ayrintili: bool,
    kompakt_yukseklik: f32,
    /// Tepsi menüsünde "Tema" alt listesi açık mı
    tepsi_tema_acik: bool,

    // Kimlik
    username: String,
    password: String,
    office_code: String,
    role: String,
    office_label: String,
    server_url: String,

    // Paylaşım
    is_sharing: bool,
    pin_var: String,
    cert_var: String,
    lan_share_var: bool,
    auto_share_dash_var: bool,
    share_status_text: String,
    share_status_color: egui::Color32,
    share_stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    share_exit_rx: Option<Receiver<()>>,

    // Alt pencereler
    sira_sekmesi: SiraKapisiSekmesi,
    bot_kurulum: Option<BotKurulumPenceresi>,
    forgot_dialog: Option<ForgotPasswordDialog>,
}

impl UyapApp {
    pub fn new() -> Self {
        let cfg = load_config();
        let (log_tx, log_rx) = mpsc::channel();
        *UI_LOG_TX.lock().unwrap() = Some(log_tx.clone());
        std::thread::spawn(crate::paylas_win::udf_turunu_kaydet);
        let server_url = cfg.server_url.clone();

        let mut app = Self {
            cfg,
            screen: ScreenMode::Login,
            nav: NavTab::Baglanti,
            log_tx,
            log_rx,
            durum: BaglantiDurumu::Kapali,
            #[cfg(windows)]
            tepsi: None,
            tepside_baslat: false,
            kare: 0,
            tema_menu_acik: false,
            tepsi_panel: None,
            tepsi_panel_odak: false,

            login_var: String::new(),
            pass_var: String::new(),
            remember_var: true,
            auto_share_var: true,
            login_pin_var: String::new(),
            login_status_text: String::new(),
            login_status_color: C::INK_SOFT(),
            login_in_progress: false,
            login_rx: None,
            login_screen_shown: false,
            auto_login_deadline: None,
            login_height_last: 0.0,
            login_height_pending: 0.0,
            dash_onceki: None,
            ayrintili: false,
            kompakt_yukseklik: 0.0,
            tepsi_tema_acik: false,

            username: String::new(),
            password: String::new(),
            office_code: String::new(),
            role: "member".to_string(),
            office_label: String::new(),
            server_url,

            is_sharing: false,
            pin_var: String::new(),
            cert_var: String::new(),
            lan_share_var: false,
            auto_share_dash_var: true,
            share_status_text: "● Paylaşım durduruldu".to_string(),
            share_status_color: C::INK_FAINT(),
            share_stop_tx: None,
            share_exit_rx: None,

            sira_sekmesi: SiraKapisiSekmesi::default(),
            bot_kurulum: None,
            forgot_dialog: None,
        };

        app.show_login_screen(None);
        app
    }

    fn log(&self, msg: &str) {
        for satir in msg.lines() {
            crate::sifreli_gunluk::yaz(satir);
        }
        let _ = self.log_tx.send(msg.to_string());
    }

    fn poll_logs(&mut self) {
        let mut count = 0;
        while count < 200 {
            match self.log_rx.try_recv() {
                Ok(msg) => {
                    for satir in msg.lines() {
                        self.durum_guncelle(satir);
                    }
                    count += 1;
                }
                Err(_) => break,
            }
        }
    }

    /// Arka plan satırlarından bağlantı durumunu çıkarır (yalnız paylaşım açıkken).
    fn durum_guncelle(&mut self, satir: &str) {
        let s = satir.trim();
        if s.is_empty() || !self.is_sharing {
            return;
        }
        if s.starts_with("[+] Oturum hazır") || s.starts_with("[+] İlk giriş tamam") {
            self.durum = BaglantiDurumu::Kuruldu;
        } else if s.starts_with("[HATA]")
            || s.starts_with("[!] UYAP girişi başarısız")
            || s.starts_with("[!] İlk giriş başarısız")
            || s.starts_with("[!] UYAP girişi şimdilik yapılamadı")
            || s.starts_with("[!] Dinleme hatası")
        {
            let (kod, aciklama) = crate::hata_kodlari::cozumle(s);
            self.durum = BaglantiDurumu::Hata { kod, aciklama: aciklama.to_string() };
            #[cfg(windows)]
            if crate::tepsi::gizli_mi() {
                crate::tepsi::goster();
            }
        }
    }

    fn api_base(&self) -> String {
        let mut u = self.server_url.trim().to_string();
        if let Some(rest) = u.strip_prefix("wss://") {
            u = format!("https://{rest}");
        } else if let Some(rest) = u.strip_prefix("ws://") {
            u = format!("http://{rest}");
        }
        if let Some(rest) = u.strip_suffix("/ws") {
            u = rest.to_string();
        }
        u.trim_end_matches('/').to_string()
    }


    fn office_reset_request_sync(api_base: String, identifier: String) -> Result<Value, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(35))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .post(format!("{api_base}/api/reset"))
            .json(&json!({ "identifier": identifier, "office_code": "" }))
            .send()
            .map_err(|e| format!("Sunucuya ulaşılamadı: {e}"))?;
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        let val: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = val
                .get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Sunucu hatası ({}).", status.as_u16()));
            return Err(msg);
        }
        Ok(val)
    }


    fn show_login_screen(&mut self, ctx: Option<&egui::Context>) {
        let first_show = !self.login_screen_shown;
        self.login_screen_shown = true;
        self.screen = ScreenMode::Login;
        self.login_height_last = 0.0;
        self.login_height_pending = 0.0;

        let mut saved_user = self.cfg.username.trim().to_string();
        let saved_office = self.cfg.office_code.trim().to_string();
        if !saved_user.is_empty() && !saved_user.contains('@') && !saved_office.is_empty() {
            saved_user = format!("{saved_user}@{saved_office}");
        }
        self.login_var = saved_user;

        let saved_pass = if self.cfg.remember && !self.cfg.password_enc.is_empty() {
            decrypt_secret(&self.cfg.password_enc)
        } else {
            String::new()
        };
        self.pass_var = saved_pass;
        self.remember_var = self.cfg.remember;
        self.auto_share_var = self.cfg.auto_share;
        self.login_pin_var.clear();
        self.login_status_text.clear();
        self.login_status_color = C::INK_SOFT();
        self.auto_login_deadline = None;

        if first_show
            && self.cfg.auto_share
            && self.cfg.remember
            && self.login_var.contains('@')
            && !self.pass_var.is_empty()
        {
            self.login_status_text =
                "Otomatik giriş 3 saniye içinde başlıyor… (durdurmak için bir alana tıklayın)"
                    .to_string();
            self.login_status_color = C::SAGE_DK();
            self.auto_login_deadline = Some(Instant::now() + Duration::from_secs(3));
            self.tepside_baslat = true;
        }

        if let Some(c) = ctx {
            c.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
            c.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(1.0, 1.0)));
            c.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(LOGIN_CARD_W, 560.0)));
        }
    }

    fn show_dashboard(&mut self, ctx: &egui::Context) {
        self.screen = ScreenMode::Dashboard;
        self.nav = NavTab::Baglanti;
        self.pin_var.clear();
        self.cert_var = self.cfg.cert_id.clone();
        self.lan_share_var = self.cfg.lan_share;
        self.auto_share_dash_var = self.cfg.auto_share;
        self.share_status_text = "● Paylaşım durduruldu".to_string();
        self.share_status_color = C::INK_FAINT();

        // Kontrol ekranı da giriş kartı gibi çerçevesiz, yuvarlak köşeli (kullanıcı isteği 2026-11-14):
        // başlık çubuğu, küçült/büyüt/kapat ve kenarlardan boyutlandırma bizde (ui_dashboard).
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        self.ayrintili = false;
        self.kompakt_boyuta_don(ctx);

        self.log("[SİSTEM] Bulut hesabı üzerinden giriş yapıldı.\n");
        self.sira_sekmesi.yenile();
    }

    fn parse_login_id(&self) -> Option<(String, String)> {
        let raw = self.login_var.trim();
        let (user, office) = raw.rsplit_once('@')?;
        let u = user.trim();
        let o = office.trim();
        if u.is_empty() || o.is_empty() {
            return None;
        }
        Some((u.to_string(), o.to_string()))
    }

    fn on_login_click(&mut self) {
        self.auto_login_deadline = None;
        let url = self.server_url.clone();
        let Some((user, office)) = self.parse_login_id() else {
            self.login_status_text =
                "Kullanıcı adını kullanıcıadı@ofisadı biçiminde girin (ör. ahmet@kemalburo)."
                    .to_string();
            self.login_status_color = C::CLAY();
            return;
        };
        let pw = self.pass_var.trim().to_string();
        if pw.is_empty() {
            self.login_status_text = "Lütfen tüm alanları doldurun.".to_string();
            self.login_status_color = C::CLAY();
            return;
        }

        self.login_in_progress = true;
        self.login_status_text = "Sunucuya bağlanılıyor, bilgiler doğrulanıyor...".to_string();
        self.login_status_color = C::SAGE_DK();

        let (tx, rx) = mpsc::channel();
        self.login_rx = Some(rx);
        std::thread::spawn(move || {
            verify_credentials_thread(url, user, pw, office, tx);
        });
    }

    fn cancel_auto_login(&mut self) {
        if self.auto_login_deadline.take().is_some() {
            self.login_status_text = "Otomatik giriş durduruldu.".to_string();
            self.login_status_color = C::INK_SOFT();
        }
    }

    fn save_login_config(&mut self) {
        self.cfg.server_url = self.server_url.clone();
        self.cfg.office_code = self.office_code.clone();
        self.cfg.username = self.username.clone();
        self.cfg.remember = self.remember_var;
        if self.remember_var {
            self.cfg.password_enc = encrypt_secret(&self.password);
        } else {
            self.cfg.password_enc.clear();
        }
        self.cfg.auto_share = self.auto_share_var;
        save_config(&self.cfg);
    }

    fn save_dashboard_config(&mut self) {
        self.cfg.cert_id = self.cert_var.trim().to_string();
        self.cfg.local_port = LOCAL_PORT;
        self.cfg.lan_share = self.lan_share_var;
        save_config(&self.cfg);
    }

    fn maybe_auto_share(&mut self) {
        if !self.cfg.auto_share || self.is_sharing {
            return;
        }
        if self.pin_var.trim().is_empty() {
            self.log(
                "[SİSTEM] Paylaşım için E-imza PIN'inizi girip 'Bağlantıyı Paylaş'a basın (PIN güvenlik gereği saklanmaz).\n",
            );
            // PIN saklanmadığından tepside başlayan program burada kullanıcıyı beklemeli: pencereyi öne getir.
            #[cfg(windows)]
            crate::tepsi::goster();
            return;
        }
        self.log("[SİSTEM] Ofis sunucusu kipi: paylaşım kendiliğinden başlatılıyor…\n");
        let log_tx = self.log_tx.clone();
        sync_startup_shortcut(true, |s| {
            let _ = log_tx.send(s.to_string());
        });
        self.start_sharing();
    }

    fn start_sharing(&mut self) {
        let pin = self.pin_var.trim().to_string();
        let cert_id = self.cert_var.trim().to_string();
        if pin.is_empty() {
            msg_error("Hata", "Lütfen E-imza PIN kodunu girin.");
            return;
        }

        self.is_sharing = true;
        self.save_dashboard_config();
        if self.cfg.auto_share {
            let log_tx = self.log_tx.clone();
            sync_startup_shortcut(true, |s| {
                let _ = log_tx.send(s.to_string());
            });
        }

        let log_tx = self.log_tx.clone();
        free_port_if_busy(LOCAL_PORT, |s| {
            let _ = log_tx.send(s.to_string());
        });

        std::env::set_var("UYAP_LAN_SHARE", if self.lan_share_var { "1" } else { "0" });
        self.log("[SİSTEM] UYAP oturumu kuruluyor; LAN + dış ağ paylaşımı başlatılıyor...\n");

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let (exit_tx, exit_rx) = mpsc::channel();
        self.share_stop_tx = Some(stop_tx);
        self.share_exit_rx = Some(exit_rx);

        let signaling = self.server_url.clone();
        let room = self.username.clone();
        let office_code = self.office_code.clone();
        let password = self.password.clone();
        let log_tx_bg = self.log_tx.clone();
        let lan_share = self.lan_share_var;

        std::thread::spawn(move || {
            let rt = match crate::build_tokio_runtime() {
                Ok(r) => r,
                Err(e) => {
                    let _ = log_tx_bg.send(format!("[HATA] Paylaşım durdu: {e}\n"));
                    let _ = exit_tx.send(());
                    return;
                }
            };
            rt.block_on(async move {
                let args = crate::office_agent::OfficeArgs {
                    signaling: Some(signaling),
                    room: Some(room.clone()),
                    office_code,
                    password: Some(password),
                    pin: Some(pin),
                    cert_id: if cert_id.is_empty() { None } else { Some(cert_id) },
                    no_verify: false,
                    interval: 5,
                    proxy_port: LOCAL_PORT,
                    host: "0.0.0.0".to_string(),
                    user: Some(room),
                    lan_token: None,
                };
                tokio::select! {
                    res = crate::office_agent::run_office(args) => {
                        if let Err(e) = res {
                            let _ = log_tx_bg.send(format!("[HATA] Paylaşım durdu: {e}\n"));
                        }
                    }
                    _ = stop_rx => {}
                }
                let _ = log_tx_bg.send("[SİSTEM] Paylaşım sonlandı.\n".to_string());
                let _ = exit_tx.send(());
            });
        });

        let log_tx2 = self.log_tx.clone();
        start_gecit_server(|s| {
            let _ = log_tx2.send(s.to_string());
        });

        // Büro veritabanı (gömülü PostgreSQL) — yalnız bu ofis bilgisayarında, arkada başlar.
        let log_tx_db = std::sync::Mutex::new(self.log_tx.clone());
        std::thread::spawn(move || {
            let gunluk: crate::veritabani::Gunluk = std::sync::Arc::new(move |s: &str| {
                if let Ok(tx) = log_tx_db.lock() {
                    let _ = tx.send(format!("{s}
"));
                }
            });
            if let Err(e) = crate::veritabani::baslat(gunluk.clone()) {
                gunluk(&format!("[VERİTABANI] Başlatılamadı: {e}"));
            } else {
                // Yeni kurulum: hazır şablon / süreç / etiket yoksa yüklenir (L7 olay bağları dahil).
                // Kullanıcının sildiği hazır şablon geri gelmesin diye yalnız hiç hazır şablon yokken.
                match crate::veritabani::baglanti().and_then(|mut c| crate::is_veri::hazir_verileri_ilk_kez(&mut *c)) {
                    Ok(n) if n > 0 => gunluk(&format!("[VERİTABANI] Hazır İş Listesi verileri yüklendi ({n} kayıt).")),
                    Ok(_) => {}
                    Err(e) => gunluk(&format!("[VERİTABANI] Hazır İş Listesi verileri yüklenemedi: {e}")),
                }
                // Hazır kesinleşme kuralları (takip türüne göre süreler); kullanıcının değiştirdiği kurala dokunulmaz.
                let bugun = chrono::Local::now().date_naive();
                let g2 = gunluk.clone();
                if let Err(e) = crate::veritabani::baglanti()
                    .and_then(|mut c| crate::icra_api::varsayilan_kurallari_uygula(&mut *c, bugun, &move |m: &str| g2(m)))
                {
                    gunluk(&format!("[VERİTABANI] Hazır kesinleşme kuralları yazılamadı: {e}"));
                }
                // Tebligat grupları: eski ham türler (memurun yazdığı zarf içeriği) sabit gruplara, muhatap alanlara.
                let g3 = gunluk.clone();
                if let Err(e) = crate::veritabani::baglanti()
                    .and_then(|mut c| crate::icra_api::tebligat_gruplarini_uygula(&mut *c, bugun, &move |m: &str| g3(m)))
                {
                    gunluk(&format!("[VERİTABANI] Tebligat grupları uygulanamadı: {e}"));
                }
            }
        });

        self.share_status_text = "● Paylaşım Aktif".to_string();
        self.durum = BaglantiDurumu::Baglaniyor;
        self.share_status_color = C::OK();

        let mut body = if lan_share {
            let port = *crate::lan_tls::LAN_TLS_PORT;
            let addr_lines: Vec<String> = detect_ips()
                .into_iter()
                .map(|ip| format!("https://{ip}:{port}/__uyap_lan__/giris"))
                .collect();
            format!(
                "Bu bilgisayardan giriş: 'Tarayıcıyı Aç' düğmesi.\n\
                 Yerel ağdaki diğer bilgisayarlar tarayıcıda şu adreslerden (parolayla) bağlanabilir:\n{}",
                addr_lines.join("  |  ")
            )
        } else {
            "Bu bilgisayardan giriş: 'Tarayıcıyı Aç' düğmesi \
             (yerel ağdan doğrudan erişim KAPALI; diğer bilgisayarlar dış ağ yoluyla bağlanır)."
                .to_string()
        };
        body.push_str(&format!(
            "\nDış ağ: '{}' kullanıcısıyla {} üzerinden paylaşılıyor (oda kimliği içeride otomatik döner).",
            self.username, self.server_url
        ));
        self.log(&format!("[SİSTEM] {body}\n"));
    }

    fn stop_sharing(&mut self, unexpected: bool) {
        self.is_sharing = false;
        let log_tx = self.log_tx.clone();
        stop_gecit_server(|s| {
            let _ = log_tx.send(s.to_string());
        });
        if let Some(tx) = self.share_stop_tx.take() {
            let _ = tx.send(());
        }
        self.share_exit_rx = None;
        self.share_status_text = "● Paylaşım Durduruldu".to_string();
        self.share_status_color = C::OFF();
        self.pin_var.clear();
        self.login_pin_var.clear();
        if unexpected {
            self.log("[SİSTEM] Paylaşım beklenmedik bir hata nedeniyle durduruldu.\n");
            if !matches!(self.durum, BaglantiDurumu::Hata { .. }) {
                let (kod, aciklama) = crate::hata_kodlari::cozumle("beklenmedik");
                self.durum = BaglantiDurumu::Hata { kod, aciklama: aciklama.to_string() };
            }
        } else {
            self.durum = BaglantiDurumu::Kapali;
        }
    }



    fn on_open_browser_click(&self) {
        uyap_tarayicida_ac(Some(self.log_tx.clone()));
    }

    #[allow(dead_code)]
    fn on_open_browser_click_eski(&self) {
        let log_tx = self.log_tx.clone();
        std::thread::spawn(move || {
            let url = crate::yerel_giris::tarayici_giris_url(LOCAL_PORT, "/giris", 3.0);
            // cmd /C start adresteki '&' işaretini komut ayıracı sayıp adresi kesiyordu ('hedef' is not
            // recognized…). Adres cmd'den geçmeden varsayılan tarayıcıyla açılır.
            let res = if cfg!(windows) {
                Command::new("rundll32")
                    .args(["url.dll,FileProtocolHandler", &url])
                    .creation_flags_win()
                    .spawn()
            } else {
                Command::new("xdg-open").arg(&url).spawn()
            };
            if let Err(e) = res {
                let _ = log_tx.send(format!("[HATA] Tarayıcı açılamadı: {e}\n"));
            }
        });
    }

    fn on_auto_share_toggle(&mut self) {
        self.cfg.auto_share = self.auto_share_dash_var;
        save_config(&self.cfg);
        self.auto_share_var = self.cfg.auto_share;
        let log_tx = self.log_tx.clone();
        sync_startup_shortcut(self.cfg.auto_share, |s| {
            let _ = log_tx.send(s.to_string());
        });
        let msg = if self.cfg.auto_share {
            "açık: girişte kendiliğinden başlar"
        } else {
            "kapalı"
        };
        self.log(&format!("[SİSTEM] Otomatik paylaşım {msg}.\n"));
    }




    fn on_logout_click(&mut self, ctx: &egui::Context) {
        if msg_ask_yes_no(
            "Çıkış",
            "Oturumu kapatmak ve çalışan tüm süreçleri durdurmak istediğinize emin misiniz?",
        ) {
            self.stop_sharing(false);
            std::thread::spawn(|| crate::veritabani::durdur(&|_| {}));
            self.username.clear();
            self.password.clear();
            crate::is_listesi::clear();
            #[cfg(windows)]
            crate::tepsi::kimlik_temizle();
            self.show_login_screen(Some(ctx));
        }
    }

    fn on_close(&mut self) {
        self.is_sharing = false;
        self.stop_sharing(false);
        crate::veritabani::durdur(&|_| {});
        std::process::exit(0);
    }

    // ── Arka plan sonuçlarını yoklama ─────────────────────────────────────────────────────

    fn poll_background(&mut self, ctx: &egui::Context) {
        if let Some(deadline) = self.auto_login_deadline {
            if Instant::now() >= deadline {
                self.auto_login_deadline = None;
                if !self.login_in_progress {
                    self.on_login_click();
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        if let Some(rx) = &self.login_rx {
            if let Ok((success, message, info)) = rx.try_recv() {
                self.login_rx = None;
                self.login_in_progress = false;
                if success {
                    if let Some((user, office)) = self.parse_login_id() {
                        self.username = user;
                        self.office_code = office;
                    }
                    self.password = self.pass_var.trim().to_string();
                    self.role = info.role;
                    self.office_label = info.office_label;
                    crate::is_listesi::configure(
                        &self.api_base(),
                        &self.office_code,
                        &self.username,
                        &self.password,
                    );
                    self.save_login_config();
                    #[cfg(windows)]
                    crate::tepsi::kimlik_ayarla(&self.api_base(), &self.username, &self.office_code, &self.password);
                    let login_pin = self.login_pin_var.trim().to_string();
                    self.show_dashboard(ctx);
                    if !login_pin.is_empty() {
                        self.pin_var = login_pin;
                    }
                    self.maybe_auto_share();
                } else {
                    self.login_status_text = format!("Giriş Başarısız: {message}");
                    self.login_status_color = C::CLAY();
                    // Tepside açılışta giriş düşerse kullanıcı görsün.
                    #[cfg(windows)]
                    crate::tepsi::goster();
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        if let Some(rx) = &self.share_exit_rx {
            if rx.try_recv().is_ok() {
                self.share_exit_rx = None;
                if self.is_sharing {
                    self.stop_sharing(true);
                }
            }
        }


    }

    // ── Widget yardımcıları ───────────────────────────────────────────────────────────────

    fn flat_entry(
        ui: &mut egui::Ui,
        text: &mut String,
        password: bool,
        enabled: bool,
        width: Option<f32>,
    ) -> egui::Response {
        let fill = if enabled { C::ENTRY() } else { C::BG() };
        let fg = if enabled { C::INK() } else { C::INK_FAINT() };
        let frame_resp = egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, C::LINE()))
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .show(ui, |ui| {
                let mut te = egui::TextEdit::singleline(text)
                    .frame(false)
                    .password(password)
                    .text_color(fg)
                    .font(Fonts::body());
                if let Some(w) = width {
                    te = te.desired_width(w);
                } else {
                    te = te.desired_width(f32::INFINITY);
                }
                ui.add_enabled(enabled, te)
            });
        frame_resp.inner
    }

    fn labeled_entry(
        ui: &mut egui::Ui,
        label: &str,
        text: &mut String,
        password: bool,
        enabled: bool,
    ) -> egui::Response {
        ui.label(egui::RichText::new(label).font(Fonts::small()).color(C::INK_SOFT()));
        ui.add_space(4.0);
        let resp = Self::flat_entry(ui, text, password, enabled, None);
        ui.add_space(13.0);
        resp
    }

    fn check(ui: &mut egui::Ui, checked: &mut bool, text: &str, enabled: bool) -> egui::Response {
        ui.add_enabled(
            enabled,
            egui::Checkbox::new(
                checked,
                egui::RichText::new(text).font(Fonts::small()).color(C::INK_SOFT()),
            ),
        )
    }

    fn section_head(ui: &mut egui::Ui, title: &str) {
        ui.add_space(14.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 1.0),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 0.0, C::LINE());
        ui.add_space(10.0);
        ui.label(egui::RichText::new(title).font(Fonts::card_d()).color(C::INK()));
        ui.add_space(8.0);
    }

    // ── Giriş Ekranı Çizimi ───────────────────────────────────────────────────────────────

    fn ui_login(&mut self, ctx: &egui::Context) {
        let mut trigger_login = false;
        let mut cancel_auto = false;

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let outer_rect = ui.max_rect();
                let card_rect = egui::Rect::from_min_max(
                    egui::pos2(outer_rect.min.x + 6.0, outer_rect.min.y + 6.0),
                    egui::pos2(outer_rect.max.x - 10.0, outer_rect.max.y - 10.0),
                );
                let shadow_rect = egui::Rect::from_min_max(
                    egui::pos2(outer_rect.min.x + 8.0, outer_rect.min.y + 8.0),
                    egui::pos2(outer_rect.max.x - 8.0, outer_rect.max.y - 8.0),
                );
                round_rect(ui.painter(), shadow_rect, 16.0, C::SHADOW(), None);
                round_rect(ui.painter(), card_rect, 16.0, C::CARD(), Some(C::CARD_EDGE()));

                // Pencereyi sürükle (çerçevesiz kart)
                let drag_resp = ui.interact(
                    outer_rect,
                    ui.id().with("login_drag"),
                    egui::Sense::drag(),
                );
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                let inner_rect = outer_rect.shrink(LOGIN_CARD_MARGIN);
                let mut child_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner_rect)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                );

                // Sağ üst kapatma düğmesi
                child_ui.horizontal(|ui| {
                    ui.horizontal(|ui| {
                        let (badge_rect, _) =
                            ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                        let logo = armonia_logo(ui.ctx());
                        ui.painter().image(
                            logo.id(),
                            badge_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                        ui.add_space(11.0);
                        ui.label(
                            egui::RichText::new("Çoklu UYAP")
                                .font(Fonts::card_t())
                                .color(C::INK()),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        // 28×28 tıklama alanı: küçük "×" etiketi kolayca ıskalanıyordu.
                        let (close_rect, close_resp) = ui.allocate_exact_size(
                            egui::vec2(28.0, 28.0),
                            egui::Sense::click(),
                        );
                        if close_resp.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            round_rect(ui.painter(), close_rect, 7.0, C::LINE(), None);
                        }
                        ui.painter().text(
                            close_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "×",
                            egui::FontId::proportional(20.0),
                            if close_resp.hovered() { C::INK() } else { C::INK_SOFT() },
                        );
                        if close_resp.clicked() {
                            self.on_close();
                        }
                    });
                });

                child_ui.add_space(12.0);
                child_ui.label(
                    egui::RichText::new("Hesabınızla giriş yapın")
                        .font(Fonts::body())
                        .color(C::INK_SOFT()),
                );
                child_ui.add_space(20.0);

                let enabled = !self.login_in_progress;

                let r1 = Self::labeled_entry(
                    &mut child_ui,
                    "Kullanıcı adı (kullanıcıadı@ofisadı)",
                    &mut self.login_var,
                    false,
                    enabled,
                );
                if r1.changed() || r1.clicked() {
                    cancel_auto = true;
                }

                let r2 = Self::labeled_entry(
                    &mut child_ui,
                    "Parola",
                    &mut self.pass_var,
                    true,
                    enabled,
                );
                if r2.changed() || r2.clicked() {
                    cancel_auto = true;
                }

                Self::check(&mut child_ui, &mut self.remember_var, "Bilgilerimi hatırla", enabled);
                child_ui.add_space(6.0);

                Self::check(
                    &mut child_ui,
                    &mut self.auto_share_var,
                    "Bu bilgisayar ofis sunucusu olsun\n(girişte paylaşım kendiliğinden başlar)",
                    enabled,
                );
                child_ui.add_space(6.0);

                if self.auto_share_var {
                    let r3 = Self::labeled_entry(
                        &mut child_ui,
                        "E-imza PIN kodu",
                        &mut self.login_pin_var,
                        true,
                        enabled,
                    );
                    if r3.changed() || r3.clicked() {
                        cancel_auto = true;
                    }
                }

                // Durum yazısı düğmenin ÜSTÜNDE: altta kalınca sabit boyutlu pencerede (Armonia/labwc yeniden
                // boyutlandırmayı uygulamıyor) "Bağlanılıyor"/"Giriş Başarısız" hiç görünmüyordu. 2026-11-14.
                if !self.login_status_text.is_empty() {
                    child_ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.login_status_text)
                                .font(Fonts::small())
                                .color(self.login_status_color),
                        )
                        .wrap(),
                    );
                    child_ui.add_space(6.0);
                }

                if RoundButton::new("Giriş Yap")
                    .kind(ButtonKind::Primary)
                    .font(Fonts::card_t())
                    .height(42.0)
                    .fill_width(true)
                    .enabled(enabled)
                    .show(&mut child_ui)
                    .clicked()
                {
                    trigger_login = true;
                }

                child_ui.add_space(14.0);
                child_ui.vertical_centered(|ui| {
                    let forgot_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new("Parolamı unuttum")
                                .font(Fonts::small())
                                .underline()
                                .color(C::SAGE_DK()),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if forgot_resp.hovered() {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if forgot_resp.clicked() {
                        self.forgot_dialog = Some(ForgotPasswordDialog {
                            ident: self.login_var.trim().to_string(),
                            status: String::new(),
                            status_ok: true,
                            rx: None,
                            close: false,
                        });
                    }

                    ui.add_space(6.0);
                    let bot_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new("Bot bilgisayarı kurulumu (Sıra Kapısı)")
                                .font(Fonts::small())
                                .underline()
                                .color(C::INK_SOFT()),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if bot_resp.hovered() {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if bot_resp.clicked() {
                        self.bot_kurulum = Some(BotKurulumPenceresi::new());
                    }

                });

                // Giriş sürerken açılış ekranı (kullanıcı isteği 2026-11-14: bekleme sırasında ekran hareketsiz
                // kalıyordu). Kartın üstüne en öne çizilir; alttaki alanlar zaten devre dışı.
                if self.login_in_progress {
                    giris_acilis_ekrani(ctx, card_rect, &self.login_status_text);
                }

                // Pencere boyunu yalnız içerik yüksekliği iki karede aynı kaldığında ve tam fiziksel
                // piksele yuvarlanmış olarak değiştir: her karede yeniden boyutlama (özellikle Windows
                // %125 ölçekte) imleç ile çizim arasında kaymaya yol açıyordu.
                let ppp = ctx.pixels_per_point().max(0.5);
                let used_h = ((child_ui.min_rect().height() + 2.0 * LOGIN_CARD_MARGIN) * ppp).ceil() / ppp;
                let stable = (used_h - self.login_height_pending).abs() < 0.5;
                self.login_height_pending = used_h;
                if stable && (used_h - self.login_height_last).abs() > 2.0 && used_h > 200.0 {
                    self.login_height_last = used_h;
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        LOGIN_CARD_W,
                        used_h,
                    )));
                }
            });

        if cancel_auto {
            self.cancel_auto_login();
        }
        if trigger_login {
            self.on_login_click();
        }
    }

    // ── Kontrol Ekranı Çizimi ─────────────────────────────────────────────────────────────

    fn ui_dashboard(&mut self, ctx: &egui::Context) {
        let mut do_logout = false;
        let buyuk = self.dash_onceki.is_some();
        let mut buyut_degistir = false;
        // Windows'un kendi büyütmesi (Win+↑, görev çubuğundan) çerçevesiz pencerede görev çubuğunu da örtüyor:
        // yakala, geri al ve çalışma alanına yerleştiren kendi büyütmemize yönlendir (kullanıcı bildirimi).
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            if !buyuk {
                buyut_degistir = true;
            }
        }
        // Büyütülünce kenar boşluğu ve köşe yuvarlaması yok; normalde giriş kartıyla aynı dil.
        let (pay, yaricap) = if buyuk { (0.0, 0.0) } else { (6.0, 16.0) };
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let dis = ui.max_rect();
                let kart = egui::Rect::from_min_max(
                    egui::pos2(dis.min.x + pay, dis.min.y + pay),
                    egui::pos2(dis.max.x - pay - if buyuk { 0.0 } else { 4.0 }, dis.max.y - pay - if buyuk { 0.0 } else { 4.0 }),
                );
                if !buyuk {
                    round_rect(ui.painter(), kart.translate(egui::vec2(2.0, 2.0)), yaricap, C::SHADOW(), None);
                }
                round_rect(ui.painter(), kart, yaricap, C::BG(), if buyuk { None } else { Some(C::CARD_EDGE()) });

                // ── Başlık çubuğu (sürükle, çift tık = büyüt, küçült / büyüt / kapat) ──
                let baslik = egui::Rect::from_min_max(kart.min, egui::pos2(kart.max.x, kart.min.y + 44.0));
                let surukle = ui.interact(baslik, ui.id().with("dash_baslik"), egui::Sense::click_and_drag());
                if surukle.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if surukle.double_clicked() {
                    buyut_degistir = true;
                }
                let logo = armonia_logo(ctx);
                let logo_r = egui::Rect::from_center_size(
                    egui::pos2(baslik.min.x + 18.0 + 13.0, baslik.center().y),
                    egui::vec2(26.0, 26.0),
                );
                ui.painter().image(
                    logo.id(),
                    logo_r,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                ui.painter().text(
                    egui::pos2(logo_r.max.x + 10.0, baslik.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Çoklu UYAP",
                    Fonts::card_d(),
                    C::INK(),
                );
                // Tema düğmesi (pencere düğmelerinin solunda): web ile ortak tema seçici.
                let tema_r = egui::Rect::from_min_size(
                    egui::pos2(baslik.max.x - 10.0 - 36.0 * 3.0 - 40.0, baslik.min.y + 7.0),
                    egui::vec2(34.0, 30.0),
                );
                let tema_resp = ui.interact(tema_r, ui.id().with("dash_tema"), egui::Sense::click());
                if tema_resp.hovered() || self.tema_menu_acik {
                    round_rect(ui.painter(), tema_r, 7.0, C::LINE(), None);
                }
                tema_noktasi(ui.painter(), tema_r.center(), 7.5, crate::uyap_theme::palet());
                if tema_resp.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if tema_resp.clicked() {
                    self.tema_menu_acik = !self.tema_menu_acik;
                }
                if self.tema_menu_acik {
                    let alan = egui::Area::new(egui::Id::new("dash_tema_menu"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(egui::pos2(tema_r.max.x - 210.0, tema_r.max.y + 6.0))
                        .show(ctx, |ui| {
                            egui::Frame::none()
                                .fill(C::CARD())
                                .stroke(egui::Stroke::new(1.0, C::CARD_EDGE()))
                                .rounding(egui::Rounding::same(12.0))
                                .shadow(egui::epaint::Shadow {
                                    offset: egui::vec2(0.0, 4.0),
                                    blur: 14.0,
                                    spread: 0.0,
                                    color: egui::Color32::from_black_alpha(40),
                                })
                                .inner_margin(egui::Margin::same(8.0))
                                .show(ui, |ui| {
                                    ui.set_width(194.0);
                                    if let Some(ad) = tema_listesi(ui) {
                                        tema_degistir(ctx, ad);
                                        self.tema_menu_acik = false;
                                    }
                                });
                        });
                    let disari = ctx.input(|i| i.pointer.any_click())
                        && !alan.response.contains_pointer()
                        && !tema_resp.contains_pointer();
                    if disari || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.tema_menu_acik = false;
                    }
                }

                // Sağ üst düğmeler: kapat, büyüt/geri, küçült (sağdan sola)
                let mut x = baslik.max.x - 10.0;
                for (simge, is) in [("×", 0u8), (if buyuk { "❐" } else { "□" }, 1u8), ("–", 2u8)] {
                    let r = egui::Rect::from_min_size(egui::pos2(x - 34.0, baslik.min.y + 7.0), egui::vec2(34.0, 30.0));
                    x -= 36.0;
                    let resp = ui.interact(r, ui.id().with(("dash_dugme", is)), egui::Sense::click());
                    if resp.hovered() {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        let zemin = if is == 0 { egui::Color32::from_rgb(0xE8, 0x5D, 0x4F) } else { C::LINE() };
                        round_rect(ui.painter(), r, 7.0, zemin, None);
                    }
                    let renk = if resp.hovered() && is == 0 { egui::Color32::WHITE } else { C::INK_SOFT() };
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, simge, egui::FontId::proportional(if is == 1 { 15.0 } else { 19.0 }), renk);
                    if resp.clicked() {
                        match is {
                            // Kontrol ekranında × pencereyi tepsiye gizler; program çalışmaya devam eder
                            // (çıkış: tepsi menüsü → Çıkış).
                            0 => {
                                // Bir dahaki açılışta sade panel gelsin
                                self.ayrintili = false;
                                self.dash_onceki = None;
                                self.kompakt_boyuta_don(ctx);
                                #[cfg(windows)]
                                crate::tepsi::gizle();
                                #[cfg(not(windows))]
                                self.on_close();
                            }
                            1 => buyut_degistir = true,
                            _ => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                        }
                    }
                }

                // ── Kenardan boyutlandırma (büyütülmemişken): sağ kenar, alt kenar, sağ alt köşe ──
                // Windows'un hazır boyutlandırması kalın kenar bayrağı istiyor; o bayrak çizimi kaydırdığı için
                // kaldırıldı (cercevesiz_pencere_stilini_duzelt). Pencerenin sol üstü sabit kaldığından yeni boyut
                // doğrudan fare konumundan hesaplanır.
                if !buyuk {
                    let k = 8.0;
                    let parcalar = [
                        (egui::Rect::from_min_max(egui::pos2(dis.max.x - k * 2.0, dis.max.y - k * 2.0), dis.max), true, true, egui::CursorIcon::ResizeNwSe),
                        (egui::Rect::from_min_max(egui::pos2(dis.max.x - k, dis.min.y + 44.0), egui::pos2(dis.max.x, dis.max.y - k * 2.0)), true, false, egui::CursorIcon::ResizeHorizontal),
                        (egui::Rect::from_min_max(egui::pos2(dis.min.x + k * 2.0, dis.max.y - k), egui::pos2(dis.max.x - k * 2.0, dis.max.y)), false, true, egui::CursorIcon::ResizeVertical),
                    ];
                    for (i, (r, gen, yuk, imlec)) in parcalar.into_iter().enumerate() {
                        let resp = ui.interact(r, ui.id().with(("dash_kenar", i)), egui::Sense::drag());
                        if resp.hovered() || resp.dragged() {
                            ctx.set_cursor_icon(imlec);
                        }
                        if resp.dragged() {
                            if let Some(fare) = ctx.input(|i| i.pointer.interact_pos()) {
                                let simdi = dis.size();
                                let yeni = egui::vec2(
                                    if gen { (fare.x + 4.0).max(760.0) } else { simdi.x },
                                    if yuk { (fare.y + 4.0).max(560.0) } else { simdi.y },
                                );
                                if (yeni - simdi).length() > 0.5 {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(yeni));
                                }
                            }
                        }
                    }
                }

                // Gövde: başlığın altı, köşe yuvarlamasından taşmasın diye alt köşelerde iç pay
                let govde_r = egui::Rect::from_min_max(
                    egui::pos2(kart.min.x + 1.0, baslik.max.y),
                    egui::pos2(kart.max.x - 1.0, kart.max.y - if buyuk { 0.0 } else { yaricap * 0.6 }),
                );
                let mut gui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(govde_r)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                );
                gui.set_clip_rect(govde_r);
                let ui = &mut gui;
                // Header
                egui::Frame::none()
                    .fill(C::BG())
                    .inner_margin(egui::Margin {
                        left: 28.0,
                        right: 28.0,
                        top: 20.0,
                        bottom: 16.0,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let kimlik = if self.office_code.is_empty() {
                                self.username.clone()
                            } else {
                                format!("{}@{}", self.username, self.office_code)
                            };
                            ui.label(
                                egui::RichText::new(format!("Kullanıcı: {kimlik}"))
                                    .font(Fonts::h1())
                                    .color(C::INK()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if RoundButton::new("Çıkış Yap")
                                        .kind(ButtonKind::Ghost)
                                        .font(Fonts::card_d())
                                        .height(32.0)
                                        .show(ui)
                                        .clicked()
                                    {
                                        do_logout = true;
                                    }
                                },
                            );
                        });
                    });

                let (hr_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(hr_rect, 0.0, C::LINE());

                // Gövde: sol menü + sağ içerik
                let avail = ui.available_rect_before_wrap();
                // Dar pencerede sol menü daralır; içerik alanı kaydırılabilir (kullanıcı bildirimi 2026-11-14).
                let side_w = if avail.width() < 820.0 { 140.0 } else { 190.0 };
                let side_rect = egui::Rect::from_min_max(
                    avail.min,
                    egui::pos2(avail.min.x + side_w, avail.max.y),
                );
                let line_rect = egui::Rect::from_min_max(
                    egui::pos2(side_rect.max.x, avail.min.y),
                    egui::pos2(side_rect.max.x + 1.0, avail.max.y),
                );
                let content_rect = egui::Rect::from_min_max(
                    egui::pos2(line_rect.max.x, avail.min.y),
                    avail.max,
                );

                ui.painter().rect_filled(side_rect, 0.0, C::SIDEBAR());
                ui.painter().rect_filled(line_rect, 0.0, C::LINE());

                let mut side_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(side_rect)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                );
                side_ui.add_space(14.0);
                for (tab, label) in [
                    (NavTab::Baglanti, "Bağlantı"),
                    (NavTab::SiraKapisi, "Sıra Kapısı"),
                ] {
                    let active = self.nav == tab;
                    let (item_rect, resp) = side_ui.allocate_exact_size(
                        egui::vec2(side_w, 40.0),
                        egui::Sense::click(),
                    );
                    if resp.hovered() {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let bg = if active {
                        C::SAGE_TINT()
                    } else if resp.hovered() {
                        C::LINE()
                    } else {
                        C::SIDEBAR()
                    };
                    let fg = if active { C::INK() } else { C::INK_SOFT() };
                    let font = if active { Fonts::nav_b() } else { Fonts::nav() };
                    side_ui.painter().rect_filled(item_rect, 0.0, bg);
                    side_ui.painter().text(
                        egui::pos2(item_rect.min.x + 20.0, item_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        font,
                        fg,
                    );
                    if resp.clicked() {
                        self.nav = tab;
                        if tab == NavTab::SiraKapisi {
                            self.sira_sekmesi.yenile();
                        }
                    }
                }

                let mut content_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                );
                match self.nav {
                    NavTab::Baglanti => self.ui_conn_tab(&mut content_ui),
                    NavTab::SiraKapisi => self.sira_sekmesi.show(&mut content_ui, ctx),
                }
            });

        if buyut_degistir {
            self.dash_buyut_degistir(ctx);
        }
        if do_logout {
            self.on_logout_click(ctx);
        }
    }

    /// Temalı tepsi paneli (Windows): sağ tıkta açılır; Aç, tema seçimi, günlük, Çıkış. Odağı kaybedince,
    /// Esc'de ya da bir öğe seçilince kapanır.
    fn tepsi_paneli(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        {
            let Some(konum) = self.tepsi_panel else { return };
            let h_once = self.tepsi_panel_h();
            let pin_goster = self.tepsi_pin_goster();
            let mut kapat = false;
            let mut eylem: Option<u8> = None;
            let mut secilen_tema: Option<&'static str> = None;
            const BASLIK: &str = "Çoklu UYAP Menü";
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("tepsi_panel"),
                egui::ViewportBuilder::default()
                    .with_title(BASLIK)
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_always_on_top()
                    .with_taskbar(false)
                    .with_resizable(false)
                    .with_position(konum)
                    .with_inner_size([TEPSI_PANEL_W, h_once]),
                |ctx, _| {
                    crate::uyap_app::cercevesiz_stil_duzelt(BASLIK);
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
                        .show(ctx, |ui| {
                            let r = ui.max_rect().shrink(2.0);
                            round_rect(ui.painter(), r, 14.0, C::CARD(), Some(C::CARD_EDGE()));
                            let mut cui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(r.shrink(10.0))
                                    .layout(egui::Layout::top_down(egui::Align::LEFT)),
                            );
                            cui.horizontal(|ui| {
                                let logo = armonia_logo(ui.ctx());
                                let (lr, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                                ui.painter().image(logo.id(), lr,
                                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                    egui::Color32::WHITE);
                                ui.label(egui::RichText::new("Çoklu UYAP").font(Fonts::card_d()).color(C::INK()));
                            });
                            cui.add_space(6.0);
                            // Sunucuda paylaşım kapalıysa (PIN saklanmaz) PIN burada girilir.
                            if pin_goster {
                                cui.label(egui::RichText::new("Paylaşım kapalı · E-imza PIN'i").font(Fonts::small()).color(C::INK_SOFT()));
                                cui.add_space(2.0);
                                let te = cui.add(
                                    egui::TextEdit::singleline(&mut self.pin_var)
                                        .password(true)
                                        .hint_text("PIN")
                                        .desired_width(f32::INFINITY),
                                );
                                let enter = te.lost_focus() && cui.input(|i| i.key_pressed(egui::Key::Enter));
                                cui.add_space(2.0);
                                if panel_ogesi(&mut cui, "Bağlantıyı paylaş") || enter { eylem = Some(7); }
                                ayrac(&mut cui);
                            }
                            if panel_ogesi(&mut cui, "UYAP'ı aç") { eylem = Some(5); }
                            if panel_ogesi(&mut cui, "Pencereyi aç") { eylem = Some(1); }
                            let tema_metni = if self.tepsi_tema_acik { "Tema  ▾" } else { "Tema  ▸" };
                            if panel_ogesi(&mut cui, tema_metni) { self.tepsi_tema_acik = !self.tepsi_tema_acik; }
                            if self.tepsi_tema_acik {
                                if let Some(ad) = tema_listesi(&mut cui) { secilen_tema = Some(ad); }
                            }
                            ayrac(&mut cui);
                            if panel_ogesi(&mut cui, "Günlüğü dosyaya kaydet") { eylem = Some(2); }
                            if panel_ogesi(&mut cui, "Günlüğü destek ekibine gönder") { eylem = Some(3); }
                            ayrac(&mut cui);
                            if panel_ogesi(&mut cui, "Çıkış") { eylem = Some(4); }
                        });
                    let odakli = ctx.input(|i| i.viewport().focused.unwrap_or(false));
                    if odakli {
                        self.tepsi_panel_odak = true;
                    } else if self.tepsi_panel_odak {
                        kapat = true;
                    } else {
                        crate::tepsi::one_al(BASLIK);
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.viewport().close_requested()) {
                        kapat = true;
                    }
                },
            );
            if let Some(ad) = secilen_tema {
                tema_degistir(ctx, ad);
            }
            // Yükseklik değiştiyse (tema listesi açıldı/kapandı) alt kenar sabit kalsın: yukarı kaydır.
            let h_sonra = self.tepsi_panel_h();
            if (h_sonra - h_once).abs() > 0.5 {
                let yeni = egui::pos2(konum.x, konum.y - (h_sonra - h_once));
                self.tepsi_panel = Some(yeni);
                let vid = egui::ViewportId::from_hash_of("tepsi_panel");
                ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::OuterPosition(yeni));
                ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::InnerSize(egui::vec2(TEPSI_PANEL_W, h_sonra)));
            }
            match eylem {
                Some(1) => {
                    crate::tepsi::goster();
                    kapat = true;
                }
                Some(2) => {
                    crate::tepsi::gunluk_kaydet();
                    kapat = true;
                }
                Some(3) => {
                    crate::tepsi::gunluk_gonder();
                    kapat = true;
                }
                Some(4) => crate::tepsi::cikis(),
                Some(5) => {
                    self.uyap_ac();
                    kapat = true;
                }
                Some(7) => {
                    self.start_sharing();
                    if self.is_sharing {
                        kapat = true;
                    }
                }
                _ => {}
            }
            if kapat {
                self.tepsi_panel = None;
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        #[cfg(not(windows))]
        let _ = ctx;
    }

    /// Büyüt / geri al. Çerçevesiz pencerede Windows'un kendi büyütmesi görev çubuğunu da örtüyordu; pencere
    /// çalışma alanına (görev çubuğu hariç) kendimiz yerleştiririz, geri alınca eski konum ve boyuta döner.
    fn dash_buyut_degistir(&mut self, ctx: &egui::Context) {
        if let Some((konum, boyut)) = self.dash_onceki.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(konum));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(boyut));
            return;
        }
        let Some(alan) = calisma_alani(ctx) else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            return;
        };
        let (konum, boyut) = ctx.input(|i| {
            let vp = i.viewport();
            (
                vp.outer_rect.map(|r| r.min).unwrap_or(egui::pos2(100.0, 100.0)),
                vp.inner_rect.map(|r| r.size()).unwrap_or(egui::vec2(900.0, 640.0)),
            )
        });
        self.dash_onceki = Some((konum, boyut));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(alan.min));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(alan.size()));
    }

    fn ui_conn_tab(&mut self, ui: &mut egui::Ui) {
        // Tek kart: Bağlantı Paylaş. "Bağlantı Al" ve işlem günlüğü kaldırıldı (kullanıcı isteği 2026-11-14);
        // günlük şifreli dosyaya yazılır, ekranda yalnız tek satır durum. Küçük pencerede kaydırılır.
        egui::ScrollArea::vertical()
            .id_salt("baglanti_kaydir")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let genislik = ui.available_width();
                let kart_w = (genislik - 28.0).clamp(280.0, 480.0);
                let sol_pay = ((genislik - kart_w) / 2.0).max(14.0);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(sol_pay);
                    ui.vertical(|ui| {
                        ui.set_width(kart_w);
                        egui::Frame::none()
                            .fill(C::CARD())
                            .stroke(egui::Stroke::new(1.0, C::CARD_EDGE()))
                            .rounding(egui::Rounding::same(14.0))
                            .inner_margin(egui::Margin::symmetric(22.0, 20.0))
                            .show(ui, |ui| {
                                ui.set_width(kart_w - 44.0);
                                ui.label(
                                    egui::RichText::new("Bağlantı Paylaş")
                                        .font(Fonts::card_t())
                                        .color(C::INK()),
                                );
                                ui.add_space(14.0);
                                let inputs_enabled = !self.is_sharing;
                                Self::labeled_entry(ui, "E-imza PIN kodu", &mut self.pin_var, true, inputs_enabled);
                                Self::labeled_entry(
                                    ui,
                                    "Sertifika ID (isteğe bağlı)",
                                    &mut self.cert_var,
                                    false,
                                    inputs_enabled,
                                );
                                Self::check(
                                    ui,
                                    &mut self.lan_share_var,
                                    "Yerel ağdan doğrudan erişim (aynı ofis ağındaki bilgisayarlar)",
                                    inputs_enabled,
                                );
                                ui.add_space(6.0);
                                if Self::check(
                                    ui,
                                    &mut self.auto_share_dash_var,
                                    "Girişte ve bilgisayar açılışında otomatik paylaş",
                                    true,
                                )
                                .changed()
                                {
                                    self.on_auto_share_toggle();
                                }
                                ui.add_space(10.0);
                                let (etiket, tur) = if self.is_sharing {
                                    ("Paylaşımı Durdur", ButtonKind::Stop)
                                } else {
                                    ("Bağlantıyı Paylaş", ButtonKind::Primary)
                                };
                                if RoundButton::new(etiket)
                                    .kind(tur)
                                    .font(Fonts::nav_b())
                                    .height(38.0)
                                    .fill_width(true)
                                    .show(ui)
                                    .clicked()
                                {
                                    if self.is_sharing {
                                        self.stop_sharing(false);
                                    } else {
                                        self.start_sharing();
                                    }
                                }
                                ui.add_space(6.0);
                                if RoundButton::new("Tarayıcıyı Aç")
                                    .kind(ButtonKind::Ghost)
                                    .font(Fonts::nav_b())
                                    .height(36.0)
                                    .fill_width(true)
                                    .enabled(self.is_sharing)
                                    .show(ui)
                                    .clicked()
                                {
                                    self.on_open_browser_click();
                                }
                                ui.add_space(12.0);
                                self.durum_satiri(ui);
                            });
                        ui.add_space(12.0);
                        // Kullanıcı ekleme/çıkarma web panelinde (kullanıcı isteği 2026-11-14).
                        let baglanti = ui.add(
                            egui::Label::new(
                                egui::RichText::new("Kullanıcıları web panelinde yönet  ↗")
                                    .font(Fonts::small())
                                    .underline()
                                    .color(C::SAGE_DK()),
                            )
                            .sense(egui::Sense::click()),
                        );
                        if baglanti.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if baglanti.clicked() {
                            let adres = format!("{}/ofis", self.api_base());
                            ui.ctx().open_url(egui::OpenUrl::new_tab(adres));
                        }
                        ui.add_space(14.0);
                    });
                });
            });
    }

    /// Tek satırlık bağlantı durumu: bağlanıyor (dönen yay) / kuruldu / hata kodu.
    /// Tepsi menüsünde PIN + "Bağlantıyı paylaş" gösterilsin mi (girişli sunucu, paylaşım kapalı).
    fn tepsi_pin_goster(&self) -> bool {
        self.screen == ScreenMode::Dashboard && self.sunucu_mu() && !self.is_sharing
    }

    fn tepsi_panel_h(&self) -> f32 {
        // PIN bölümü: etiket + kutu + düğme + ayraç
        tepsi_panel_yuksekligi(self.tepsi_tema_acik) + if self.tepsi_pin_goster() { 18.0 + 32.0 + 34.0 + 13.0 } else { 0.0 }
    }

    /// Ofis sunucusu mu? (girişte "Bu bilgisayar ofis sunucusu olsun" / otomatik paylaşım)
    fn sunucu_mu(&self) -> bool {
        self.cfg.auto_share || self.is_sharing
    }

    fn kompakt_boyuta_don(&mut self, ctx: &egui::Context) {
        self.kompakt_yukseklik = 0.0;
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(1.0, 1.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(LOGIN_CARD_W, 360.0)));
    }

    /// UYAP'ı aç: ofis sunucusunda yerel vekil (paylaşım açık olmalı); istemcide web sitesi (ofise tünel).
    fn uyap_ac(&mut self) {
        if self.screen == ScreenMode::Login {
            #[cfg(windows)]
            crate::tepsi::goster();
            return;
        }
        if self.sunucu_mu() {
            uyap_tarayicida_ac(Some(self.log_tx.clone()));
        } else {
            let adres = format!("{}/", self.api_base());
            let _ = if cfg!(windows) {
                Command::new("cmd").args(["/C", "start", "", &adres]).creation_flags_win().spawn()
            } else {
                Command::new("xdg-open").arg(&adres).spawn()
            };
        }
    }

    /// Bağlantı durumunu doğrudan UYAP oturumundan okur. Günlük satırı ayrıştırmak Windows'ta çalışmıyordu:
    /// arka plan println! çıktısı arayüze ulaşmıyor, "Bağlantı kuruluyor" hiç bitmiyordu (kullanıcı, 16.11).
    fn durum_yokla(&mut self) {
        if !self.is_sharing {
            return;
        }
        let Some(gw) = crate::uyap_proxy::get_gw() else { return };
        let kilit = gw.giris_hatasi.lock().clone();
        let gecici = crate::uyap_proxy::SON_GECICI_HATA.lock().clone();
        let yeni = if let Some(h) = kilit {
            let (kod, _) = crate::hata_kodlari::cozumle(&h);
            BaglantiDurumu::Hata { kod, aciklama: h }
        } else if gw.logged_in.load(std::sync::atomic::Ordering::SeqCst) {
            BaglantiDurumu::Kuruldu
        } else if let Some(h) = gecici {
            BaglantiDurumu::Hata { kod: "BEKLE", aciklama: format!("{h} — 20 sn içinde yeniden denenecek.") }
        } else {
            BaglantiDurumu::Baglaniyor
        };
        if yeni != self.durum {
            self.durum = yeni;
        }
    }

    /// Girişten sonraki sade panel (kullanıcı isteği 16.11): durum + rolüne göre düğmeler.
    /// Sunucu: PIN + "Bağlantıyı paylaş", paylaşımda "UYAP'ı aç" + "Paylaşımı durdur". İstemci: "UYAP'ı aç".
    fn ui_kompakt(&mut self, ctx: &egui::Context) {
        let mut cikis = false;
        let mut ayrintili = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let dis = ui.max_rect();
                let kart = egui::Rect::from_min_max(
                    egui::pos2(dis.min.x + 6.0, dis.min.y + 6.0),
                    egui::pos2(dis.max.x - 10.0, dis.max.y - 10.0),
                );
                round_rect(ui.painter(), kart.translate(egui::vec2(2.0, 2.0)), 16.0, C::SHADOW(), None);
                round_rect(ui.painter(), kart, 16.0, C::CARD(), Some(C::CARD_EDGE()));
                let surukle = ui.interact(dis, ui.id().with("kompakt_surukle"), egui::Sense::drag());
                if surukle.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                let mut c = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(dis.shrink(LOGIN_CARD_MARGIN))
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                );
                // Başlık: logo + ad + hesap/rol, sağda × (tepsiye gizler)
                c.horizontal(|ui| {
                    let (lr, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                    let logo = armonia_logo(ui.ctx());
                    ui.painter().image(
                        logo.id(),
                        lr,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    ui.add_space(11.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Çoklu UYAP").font(Fonts::card_t()).color(C::INK()));
                        let rol = if self.sunucu_mu() { "Ofis sunucusu" } else { "İstemci" };
                        ui.label(
                            egui::RichText::new(format!("{}@{} · {rol}", self.username, self.office_code))
                                .font(Fonts::small())
                                .color(C::INK_SOFT()),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
                        if resp.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            round_rect(ui.painter(), r, 7.0, C::LINE(), None);
                        }
                        ui.painter().text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            "×",
                            egui::FontId::proportional(20.0),
                            if resp.hovered() { C::INK() } else { C::INK_SOFT() },
                        );
                        if resp.clicked() {
                            #[cfg(windows)]
                            crate::tepsi::gizle();
                            #[cfg(not(windows))]
                            self.on_close();
                        }
                    });
                });
                c.add_space(16.0);
                self.buyuk_durum(&mut c);
                c.add_space(14.0);

                if self.sunucu_mu() {
                    if self.is_sharing {
                        if RoundButton::new("UYAP'ı aç")
                            .kind(ButtonKind::Primary)
                            .font(Fonts::nav_b())
                            .height(42.0)
                            .fill_width(true)
                            .show(&mut c)
                            .clicked()
                        {
                            self.uyap_ac();
                        }
                        c.add_space(8.0);
                        if RoundButton::new("Paylaşımı durdur")
                            .kind(ButtonKind::Ghost)
                            .font(Fonts::nav_b())
                            .height(36.0)
                            .fill_width(true)
                            .show(&mut c)
                            .clicked()
                        {
                            self.stop_sharing(false);
                        }
                    } else {
                        Self::labeled_entry(&mut c, "E-imza PIN kodu", &mut self.pin_var, true, true);
                        if RoundButton::new("Bağlantıyı paylaş")
                            .kind(ButtonKind::Primary)
                            .font(Fonts::nav_b())
                            .height(42.0)
                            .fill_width(true)
                            .show(&mut c)
                            .clicked()
                        {
                            self.start_sharing();
                        }
                    }
                } else if RoundButton::new("UYAP'ı aç")
                    .kind(ButtonKind::Primary)
                    .font(Fonts::nav_b())
                    .height(42.0)
                    .fill_width(true)
                    .show(&mut c)
                    .clicked()
                {
                    self.uyap_ac();
                }

                c.add_space(14.0);
                c.horizontal(|ui| {
                    let bag = |ui: &mut egui::Ui, metin: &str| -> bool {
                        let r = ui.add(
                            egui::Label::new(egui::RichText::new(metin).font(Fonts::small()).underline().color(C::INK_SOFT()))
                                .sense(egui::Sense::click()),
                        );
                        if r.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        r.clicked()
                    };
                    if bag(ui, "Ayrıntılı görünüm") {
                        ayrintili = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if bag(ui, "Oturumu kapat") {
                            cikis = true;
                        }
                    });
                });

                let h = (c.min_rect().height() + 2.0 * LOGIN_CARD_MARGIN + 4.0).ceil();
                if (h - self.kompakt_yukseklik).abs() > 2.0 && h > 150.0 {
                    self.kompakt_yukseklik = h;
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(LOGIN_CARD_W, h)));
                }
            });
        if ayrintili {
            self.ayrintili = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(760.0, 560.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(900.0, 640.0)));
        }
        if cikis {
            self.on_logout_click(ctx);
        }
    }

    /// Sade paneldeki büyük, renkli durum kutusu: kullanıcı bağlantının kurulup kurulmadığını hemen anlasın.
    /// Dönen işaret yalnız gerçekten giriş denenirken; kurulunca yeşil ✓, hata/beklemede açıklamasıyla.
    fn buyuk_durum(&self, ui: &mut egui::Ui) {
        // Durum renkleri temadan bağımsız: turuncu/pembe temada "kuruldu" uyarı gibi görünüyordu.
        const YESIL: egui::Color32 = egui::Color32::from_rgb(0x2E, 0x8B, 0x57);
        const SARI: egui::Color32 = egui::Color32::from_rgb(0xD4, 0xA0, 0x17);
        const KIRMIZI: egui::Color32 = egui::Color32::from_rgb(0xC6, 0x3B, 0x2F);
        let (renk, baslik, ayrinti, doner): (egui::Color32, String, String, bool) = if !self.sunucu_mu() {
            (YESIL, "Giriş yapıldı".into(), "UYAP'ı açınca ofis bağlantısı kurulur.".into(), false)
        } else {
            match &self.durum {
                BaglantiDurumu::Kapali => (
                    C::INK_FAINT(),
                    "Paylaşım kapalı".into(),
                    "E-imza PIN'ini girip \"Bağlantıyı paylaş\"a basın.".into(),
                    false,
                ),
                BaglantiDurumu::Baglaniyor => {
                    (SARI, "Bağlantı kuruluyor…".into(), "E-imza ile UYAP oturumu açılıyor.".into(), true)
                }
                BaglantiDurumu::Kuruldu => (
                    YESIL,
                    "Bağlantı kuruldu".into(),
                    "UYAP oturumu hazır; büro bu bilgisayar üzerinden bağlanabilir.".into(),
                    false,
                ),
                BaglantiDurumu::Hata { kod: "BEKLE", aciklama } => {
                    (SARI, "UYAP oturumu bekleniyor".into(), aciklama.clone(), false)
                }
                BaglantiDurumu::Hata { kod, aciklama } => {
                    (KIRMIZI, format!("Bağlantı kurulamadı ({kod})"), aciklama.clone(), false)
                }
            }
        };
        let genislik = ui.available_width();
        let ayr = ui.painter().layout(ayrinti, Fonts::small(), C::INK_SOFT(), genislik - 48.0);
        let yuk = 30.0 + ayr.size().y + 14.0;
        let (r, _) = ui.allocate_exact_size(egui::vec2(genislik, yuk), egui::Sense::hover());
        let zemin = crate::uyap_theme::karistir(renk, C::CARD(), 0.85);
        let kenar = crate::uyap_theme::karistir(renk, C::CARD(), 0.5);
        round_rect(ui.painter(), r, 10.0, zemin, Some(kenar));
        let m = egui::pos2(r.min.x + 20.0, r.min.y + 21.0);
        if doner {
            let t = ui.input(|i| i.time) as f32;
            ui.painter().circle_stroke(m, 7.0, egui::Stroke::new(2.0, kenar));
            let bas = t * std::f32::consts::TAU / 0.9;
            let noktalar: Vec<egui::Pos2> = (0..=16)
                .map(|k| {
                    let a = bas + std::f32::consts::PI * 1.2 * k as f32 / 16.0;
                    m + egui::vec2(a.cos(), a.sin()) * 7.0
                })
                .collect();
            ui.painter().add(egui::Shape::line(noktalar, egui::Stroke::new(2.0, renk)));
            ui.ctx().request_repaint();
        } else {
            ui.painter().circle_filled(m, 6.0, renk);
        }
        ui.painter().text(egui::pos2(r.min.x + 36.0, m.y), egui::Align2::LEFT_CENTER, baslik, Fonts::card_d(), C::INK());
        ui.painter().galley(egui::pos2(r.min.x + 36.0, r.min.y + 34.0), ayr, C::INK_SOFT());
    }


    fn durum_satiri(&self, ui: &mut egui::Ui) {
        match &self.durum {
            BaglantiDurumu::Kapali => {
                ui.label(egui::RichText::new("Paylaşım kapalı").font(Fonts::small()).color(C::INK_FAINT()));
            }
            BaglantiDurumu::Baglaniyor => {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    let t = ui.input(|i| i.time) as f32;
                    let m = r.center();
                    ui.painter().circle_stroke(m, 7.0, egui::Stroke::new(2.0, C::SAGE_TINT()));
                    let bas = t * std::f32::consts::TAU / 0.9;
                    let noktalar: Vec<egui::Pos2> = (0..=16)
                        .map(|k| {
                            let a = bas + std::f32::consts::PI * 1.2 * k as f32 / 16.0;
                            m + egui::vec2(a.cos(), a.sin()) * 7.0
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(noktalar, egui::Stroke::new(2.0, C::SAGE())));
                    ui.label(egui::RichText::new("Bağlantı kuruluyor…").font(Fonts::small()).color(C::INK_SOFT()));
                    ui.ctx().request_repaint();
                });
            }
            BaglantiDurumu::Kuruldu => {
                ui.label(egui::RichText::new("✓  Bağlantı kuruldu").font(Fonts::small()).color(C::SAGE_DK()));
            }
            BaglantiDurumu::Hata { kod, aciklama } => {
                ui.label(
                    egui::RichText::new(format!("Hata {kod} · {aciklama}"))
                        .font(Fonts::small())
                        .color(C::CLAY()),
                );
            }
        }
    }

    fn show_child_windows(&mut self, ctx: &egui::Context) {
        if let Some(mut bot) = self.bot_kurulum.take() {
            let mut kapandi = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("bot_kurulum_penceresi"),
                egui::ViewportBuilder::default()
                    .with_title("Bot bilgisayarı kurulumu")
                    .with_inner_size([480.0, 470.0])
                    .with_resizable(false),
                |ctx_child, _| {
                    if ctx_child.input(|i| i.viewport().close_requested()) {
                        bot.istemci_kapat();
                        kapandi = true;
                        return;
                    }
                    bot.show(ctx_child);
                    if bot.kapandi {
                        kapandi = true;
                    }
                },
            );
            if !kapandi {
                self.bot_kurulum = Some(bot);
            }
        }

        if let Some(mut dlg) = self.forgot_dialog.take() {
            let api_base = self.api_base();
            let mut close = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("forgot_password_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Parola Sıfırlama")
                    .with_inner_size([420.0, 260.0])
                    .with_resizable(false),
                |ctx_child, _| {
                    if ctx_child.input(|i| i.viewport().close_requested()) {
                        close = true;
                        return;
                    }
                    if let Some(rx) = &dlg.rx {
                        if let Ok(res) = rx.try_recv() {
                            dlg.rx = None;
                            match res {
                                Err(err) => {
                                    dlg.status = err;
                                    dlg.status_ok = false;
                                }
                                Ok(data) => {
                                    let test_mode = data
                                        .get("test_mode")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    let sent_to = data
                                        .get("sent_to")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let extra = if test_mode {
                                        " (TEST modu: bağlantı sunucu günlüğüne yazıldı)"
                                    } else {
                                        ""
                                    };
                                    dlg.status = format!(
                                        "Sıfırlama bağlantısı {sent_to} adresine gönderildi.{extra}"
                                    );
                                    dlg.status_ok = true;
                                }
                            }
                        } else {
                            ctx_child.request_repaint_after(Duration::from_millis(100));
                        }
                    }

                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::none()
                                .fill(C::CARD())
                                .inner_margin(egui::Margin::symmetric(16.0, 14.0)),
                        )
                        .show(ctx_child, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("Parola Sıfırlama")
                                        .font(Fonts::card_t())
                                        .color(C::SAGE_DK()),
                                );
                            });
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(
                                    "Kullanıcı adınızı (kullanıcıadı@ofisadı) ya da e-postanızı girin. \
                                     Sıfırlama bağlantısı, ofis ayarına göre size ya da büronuzun master \
                                     kullanıcısına e-posta ile gönderilir.",
                                )
                                .font(Fonts::body())
                                .color(C::INK()),
                            );
                            ui.add_space(10.0);
                            Self::flat_entry(ui, &mut dlg.ident, false, true, None);
                            ui.add_space(4.0);
                            if !dlg.status.is_empty() {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new(&dlg.status)
                                            .font(Fonts::small())
                                            .color(if dlg.status_ok { C::SAGE_DK() } else { C::CLAY() }),
                                    );
                                });
                            }
                            ui.add_space(12.0);
                            if RoundButton::new("Sıfırlama Bağlantısı Gönder")
                                .kind(ButtonKind::Primary)
                                .font(Fonts::card_d())
                                .height(34.0)
                                .fill_width(true)
                                .show(ui)
                                .clicked()
                            {
                                let ident = dlg.ident.trim().to_string();
                                if ident.is_empty() {
                                    dlg.status = "Kullanıcı adı veya e-posta girin.".to_string();
                                    dlg.status_ok = false;
                                } else {
                                    dlg.status = "Gönderiliyor…".to_string();
                                    dlg.status_ok = true;
                                    let (tx, rx) = mpsc::channel();
                                    dlg.rx = Some(rx);
                                    let base = api_base.clone();
                                    std::thread::spawn(move || {
                                        let _ = tx.send(Self::office_reset_request_sync(base, ident));
                                    });
                                }
                            }
                        });
                },
            );
            if !close && !dlg.close {
                self.forgot_dialog = Some(dlg);
            }
        }


    }
}

impl eframe::App for UyapApp {
    // Çerçevesiz yuvarlak kart: köşelerin dışı tam saydam kalsın (eframe varsayılanı yarı saydam koyu gri).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            // Kontrol ekranında Alt+F4 / kapat: tepsiye gizle; girişte kapat.
            #[cfg(windows)]
            if self.screen == ScreenMode::Dashboard && self.tepsi.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                crate::tepsi::gizle();
                return;
            }
            self.on_close();
            return;
        }
        self.kare += 1;
        // Web'de seçilen tema dosyaya yazılır; ~1 sn'de bir bakılır.
        if self.kare % 10 == 0 {
            if let Some(ad) = crate::uyap_theme::tema_dosyadan_oku() {
                if ad != crate::uyap_theme::etkin_tema() && crate::uyap_theme::tema_sec(&ad) {
                    crate::uyap_theme::gorunum_uygula(ctx);
                }
            }
        }
        #[cfg(windows)]
        if let Some((x, y)) = crate::tepsi::panel_istegi() {
            let ppp = ctx.pixels_per_point().max(0.5);
            self.tepsi_tema_acik = false;
            // Gerçek yükseklikle konumlanır (en uzun hâle göre hesaplamak menüyü çok yukarıda açıyordu);
            // tema listesi açılınca alt kenar sabit kalıp yukarı büyür (tepsi_paneli).
            let (w, h) = (TEPSI_PANEL_W, self.tepsi_panel_h());
            let mut sol_ust = egui::pos2(x / ppp - w + 12.0, y / ppp - h - 10.0);
            if let Some(alan) = calisma_alani(ctx) {
                sol_ust.x = sol_ust.x.clamp(alan.min.x + 4.0, (alan.max.x - w - 4.0).max(alan.min.x));
                sol_ust.y = sol_ust.y.clamp(alan.min.y + 4.0, (alan.max.y - h - 4.0).max(alan.min.y));
            }
            self.tepsi_panel = Some(sol_ust);
            self.tepsi_panel_odak = false;
        }
        self.tepsi_paneli(ctx);
        #[cfg(windows)]
        if self.tepside_baslat && self.kare >= 3 && self.tepsi.is_some() {
            self.tepside_baslat = false;
            crate::tepsi::gizle();
        }
        #[cfg(windows)]
        {
            let onceki_hata = matches!(self.durum, BaglantiDurumu::Hata { .. });
            let ipucu = match &self.durum {
                _ if self.screen == ScreenMode::Login => "Çoklu UYAP — giriş yapılmadı".to_string(),
                BaglantiDurumu::Kapali => "Çoklu UYAP — paylaşım kapalı".to_string(),
                BaglantiDurumu::Baglaniyor => "Çoklu UYAP — bağlantı kuruluyor…".to_string(),
                BaglantiDurumu::Kuruldu => "Çoklu UYAP — bağlantı kuruldu".to_string(),
                BaglantiDurumu::Hata { kod, aciklama } => format!("Çoklu UYAP — hata {kod}: {aciklama}"),
            };
            if let Some(t) = self.tepsi.as_mut() {
                t.ipucu(&ipucu);
            }
            let _ = onceki_hata;
        }

        cercevesiz_pencere_stilini_duzelt();

        self.poll_logs();
        self.poll_background(ctx);
        if self.kare % 5 == 0 {
            self.durum_yokla();
        }

        match self.screen {
            ScreenMode::Login => self.ui_login(ctx),
            ScreenMode::Dashboard if self.ayrintili => self.ui_dashboard(ctx),
            ScreenMode::Dashboard => self.ui_kompakt(ctx),
        }

        self.show_child_windows(ctx);
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

const TEPSI_PANEL_W: f32 = 250.0;

fn tepsi_panel_yuksekligi(tema_acik: bool) -> f32 {
    // başlık + UYAP'ı aç / Pencereyi aç / Tema + (açıksa temalar) + ayraçlar + günlük (2) + çıkış + iç paylar
    let temalar = if tema_acik { crate::uyap_theme::TEMALAR.len() as f32 * 30.0 } else { 0.0 };
    40.0 + 34.0 * 3.0 + temalar + 18.0 + 34.0 * 3.0 + 26.0
}

/// Tema seçimini uygular, web ile ortak dosyaya yazar.
fn tema_degistir(ctx: &egui::Context, ad: &str) {
    crate::uyap_theme::tema_sec(ad);
    crate::uyap_theme::tema_dosyaya_yaz(ad);
    crate::uyap_theme::gorunum_uygula(ctx);
    ctx.request_repaint();
}

/// Temayı simgeleyen yarım yarım daire (vurgu + zemin).
fn tema_noktasi(boya: &egui::Painter, merkez: egui::Pos2, r: f32, p: &crate::uyap_theme::Palet) {
    boya.circle_filled(merkez, r, p.zemin);
    let yarim: Vec<egui::Pos2> = (0..=16)
        .map(|k| {
            let a = std::f32::consts::PI * 0.75 + std::f32::consts::PI * k as f32 / 16.0;
            merkez + egui::vec2(a.cos(), a.sin()) * r
        })
        .collect();
    boya.add(egui::Shape::convex_polygon(yarim, p.vurgu, egui::Stroke::NONE));
    boya.circle_stroke(merkez, r, egui::Stroke::new(1.0, p.kenar));
}

/// Tema listesi: her satırda tema noktası + ad + seçiliyse ✓. Tıklanan temanın kimliğini döndürür.
fn tema_listesi(ui: &mut egui::Ui) -> Option<&'static str> {
    let mut secilen = None;
    let etkin = crate::uyap_theme::etkin_tema();
    for (kimlik, ad, p) in crate::uyap_theme::TEMALAR {
        let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
        if resp.hovered() {
            round_rect(ui.painter(), r, 8.0, C::SAGE_TINT(), None);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        tema_noktasi(ui.painter(), egui::pos2(r.min.x + 16.0, r.center().y), 8.0, p);
        ui.painter().text(egui::pos2(r.min.x + 34.0, r.center().y), egui::Align2::LEFT_CENTER, *ad, Fonts::small(), C::INK());
        if *kimlik == etkin {
            ui.painter().text(egui::pos2(r.max.x - 12.0, r.center().y), egui::Align2::RIGHT_CENTER, "✓", Fonts::small(), C::SAGE_DK());
        }
        if resp.clicked() {
            secilen = Some(*kimlik);
        }
    }
    secilen
}

/// UYAP'ı tarayıcıda açar (yerel vekil 127.0.0.1:8800, tek kullanımlık giriş bağlantısıyla). Vekil çalışmıyorsa
/// (paylaşım ya da bağlantı kapalı) kullanıcıya söyler. Tepsi menüsü ve Kontrol Ekranı ortak kullanır.
pub(crate) fn uyap_tarayicida_ac(log_tx: Option<Sender<String>>) {
    std::thread::spawn(move || {
        let adres: SocketAddr = ([127, 0, 0, 1], LOCAL_PORT).into();
        if std::net::TcpStream::connect_timeout(&adres, Duration::from_millis(700)).is_err() {
            msg_error(
                "Çoklu UYAP",
                "UYAP açılamadı: yerel UYAP bağlantısı çalışmıyor.

Önce giriş yapıp paylaşımı (ofis sunucusu)                  ya da bağlantıyı başlatın.",
            );
            return;
        }
        let url = crate::yerel_giris::tarayici_giris_url(LOCAL_PORT, "/giris", 3.0);
        let res = if cfg!(windows) {
            Command::new("cmd").args(["/C", "start", "", &url]).creation_flags_win().spawn()
        } else {
            Command::new("xdg-open").arg(&url).spawn()
        };
        if let Err(e) = res {
            if let Some(tx) = log_tx {
                let _ = tx.send(format!("[HATA] Tarayıcı açılamadı: {e}
"));
            }
        }
    });
}

fn panel_ogesi(ui: &mut egui::Ui, metin: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    if resp.hovered() {
        round_rect(ui.painter(), r, 8.0, C::SAGE_TINT(), None);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().text(egui::pos2(r.min.x + 10.0, r.center().y), egui::Align2::LEFT_CENTER, metin, Fonts::small(), C::INK());
    resp.clicked()
}

fn ayrac(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 9.0), egui::Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, C::LINE()));
}

/// Birincil ekranın görev çubuğu hariç çalışma alanı (egui noktası cinsinden).
#[cfg(windows)]
fn calisma_alani(ctx: &egui::Context) -> Option<egui::Rect> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: SPI_GETWORKAREA geçerli bir RECT işaretçisine yazar; r yığında ve yeterli boyutta.
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut _, 0) };
    if ok == 0 || r.right <= r.left || r.bottom <= r.top {
        return None;
    }
    let ppp = ctx.pixels_per_point().max(0.5);
    Some(egui::Rect::from_min_max(
        egui::pos2(r.left as f32 / ppp, r.top as f32 / ppp),
        egui::pos2(r.right as f32 / ppp, r.bottom as f32 / ppp),
    ))
}

#[cfg(not(windows))]
fn calisma_alani(_ctx: &egui::Context) -> Option<egui::Rect> {
    None
}

/// Giriş beklenirken kartın üstünü kaplayan animasyonlu açılış ekranı: nefes alan Armonia logosu, çevresinde
/// dönen adaçayı yayı ve durum yazısı. Yalnız bizim paletimiz (C::*) kullanılır.
fn giris_acilis_ekrani(ctx: &egui::Context, kart: egui::Rect, durum: &str) {
    let t = ctx.input(|i| i.time) as f32;
    let boya = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("giris_acilis")));
    // Kartı örten yarı saydam yüzey (köşe yuvarlaması kartla aynı)
    boya.rect_filled(kart.shrink(1.0), 16.0, C::CARD().gamma_multiply(0.97));
    let merkez = egui::pos2(kart.center().x, kart.center().y - 30.0);

    // Dönen yay: 0,95 sn'de bir tur, uzunluğu nefesle birlikte 0,9π–1,5π arasında değişir
    let nefes = (t * 2.2).sin() * 0.5 + 0.5;
    let yaricap = 52.0;
    let bas = t * std::f32::consts::TAU / 0.95;
    let uzunluk = std::f32::consts::PI * (0.9 + 0.6 * nefes);
    boya.circle_stroke(merkez, yaricap, egui::Stroke::new(4.0, C::SAGE_TINT()));
    let noktalar: Vec<egui::Pos2> = (0..=48)
        .map(|k| {
            let a = bas + uzunluk * k as f32 / 48.0;
            merkez + egui::vec2(a.cos(), a.sin()) * yaricap
        })
        .collect();
    boya.add(egui::Shape::line(noktalar, egui::Stroke::new(4.0, C::SAGE())));

    // Nefes alan logo
    let boyut = 58.0 + 6.0 * nefes;
    let logo = armonia_isaret(ctx);
    boya.image(
        logo.id(),
        egui::Rect::from_center_size(merkez, egui::vec2(boyut * 0.82, boyut * 0.82)),
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    // Başlık + durum + üç nokta
    let y = merkez.y + yaricap + 30.0;
    boya.text(egui::pos2(merkez.x, y), egui::Align2::CENTER_CENTER, "Çoklu UYAP", Fonts::card_t(), C::INK());
    let yazi = if durum.trim().is_empty() { "Giriş yapılıyor" } else { durum.trim_end_matches(['.', '…']) };
    let noktalar = ".".repeat(1 + ((t * 2.5) as usize % 3));
    let galley = boya.layout(
        format!("{yazi}{noktalar}"),
        Fonts::small(),
        C::INK_SOFT(),
        kart.width() - 60.0,
    );
    let gr = egui::Rect::from_center_size(egui::pos2(merkez.x, y + 30.0), galley.size());
    boya.galley(gr.min, galley, C::INK_SOFT());
    ctx.request_repaint();
}

/// Çerçevesiz giriş kartında tıklama kayması düzeltmesi (Windows, 2026-11-14).
///
/// winit `with_decorations(false)` pencerede WS_CAPTION + WS_THICKFRAME bayraklarını bırakıp başlık
/// çubuğunu yalnız WM_NCCALCSIZE ile gizliyor. eframe'in OpenGL yüzeyi bu görünmez başlık/kenar kadar
/// (%100 ölçekte 31 px) uzun kalıyor ve egui içeriği yüzeyin altına çiziyor: çizim 31 px aşağıda, tıklama
/// koordinatları ise doğru yerde → PIN kutusuna tıklanınca alttaki "Giriş Yap" basılıyordu (kullanıcı
/// bildirimi). Ekran görüntüsüyle ölçüldü: iki bayrak kalkınca logo y=62 → y=31 (olması gereken) yerine
/// oturuyor ve tıklama doğru kutuya gidiyor. Giriş ekranında her karede denetlenir (winit
/// `Decorations(false)` komutunda bayrakları geri ekler); kontrol ekranında winit `Decorations(true)` ile
/// normal çerçeveyi kendisi kurar. Sürükleme `StartDrag` (HTCAPTION) ile bayraksız da çalışır.
fn cercevesiz_pencere_stilini_duzelt() {
    cercevesiz_stil_duzelt("Çoklu UYAP - Kontrol Ekranı");
}

/// Başlığı verilen kendi çerçevesiz penceremize aynı düzeltmeyi uygular (tepsi paneli de çerçevesiz).
#[cfg(windows)]
pub fn cercevesiz_stil_duzelt(pencere_basligi: &str) {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongW, GetWindowThreadProcessId, SetWindowLongW, SetWindowPos, GWL_STYLE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION, WS_THICKFRAME,
    };
    let baslik: Vec<u16> = pencere_basligi.encode_utf16().chain(Some(0)).collect();
    // SAFETY: yalnız kendi sürecimize ait, başlığı bilinen pencerenin stil bayrakları okunur/değiştirilir;
    // tüm işaretçiler bu fonksiyonun yığınındaki geçerli tamponlardır.
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), baslik.as_ptr());
        if hwnd.is_null() {
            return;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != GetCurrentProcessId() {
            return;
        }
        let stil = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let kaldir = WS_CAPTION | WS_THICKFRAME;
        if stil & kaldir == 0 {
            return;
        }
        SetWindowLongW(hwnd, GWL_STYLE, (stil & !kaldir) as i32);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

#[cfg(not(windows))]
pub fn cercevesiz_stil_duzelt(_pencere_basligi: &str) {}

/// Armonia "A" işareti (kartsız, saydam), 96×96 ham RGBA — kaynak `assets/armonia-isaret-buyuk.png`.
fn armonia_isaret(ctx: &egui::Context) -> egui::TextureHandle {
    const BOYUT: usize = 96;
    static RGBA: &[u8] = include_bytes!("../assets/armonia-isaret-96.rgba");
    let id = egui::Id::new("armonia_isaret_doku");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let img = egui::ColorImage::from_rgba_unmultiplied([BOYUT, BOYUT], RGBA);
    let t = ctx.load_texture("armonia_isaret", img, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Armonia logosu (kartlı simge), 68×68 ham RGBA — kaynak `assets/armonia-simge-256.png`.
/// Doku bir kez yüklenir ve egui belleğinde saklanır.
fn armonia_logo(ctx: &egui::Context) -> egui::TextureHandle {
    const BOYUT: usize = 68;
    static RGBA: &[u8] = include_bytes!("../assets/armonia-simge-68.rgba");
    let id = egui::Id::new("armonia_logo_doku");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let img = egui::ColorImage::from_rgba_unmultiplied([BOYUT, BOYUT], RGBA);
    let t = ctx.load_texture("armonia_logo", img, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

pub fn sira_kapisi_komutu(argv: &[String]) {
    let dir = crate::sira_kapisi::veri_klasoru();
    let _ = fs::create_dir_all(&dir);
    let yol = dir.join("son_komut.txt");
    let cikti = crate::sira_kapisi::komut_calistir_metin(argv);
    let mut bytes = vec![0xEF, 0xBB, 0xBF]; // utf-8-sig
    let content = format!("CokluUyap --sira-kapisi {}\n\n{}", argv.join(" "), cikti);
    bytes.extend_from_slice(content.as_bytes());
    let _ = fs::write(&yol, bytes);
    open_file_shell(&yol);
}

pub fn main() {
    crate::ensure_rustls_provider();
    let args: Vec<String> = std::env::args().collect();
    #[cfg(any(target_os = "linux", windows))]
    if args.len() == 1 {
        stdout_arayuze_akit();
    }
    if args.len() > 1 && args[1] == "--sira-yonetici" {
        let code = crate::sira_eslestirme::yonetici_main(&args[2..]);
        std::process::exit(code);
    }
    if args.len() > 1 && args[1] == "--sira-kapisi" {
        sira_kapisi_komutu(&args[2..]);
        std::process::exit(0);
    }
    // Eski UyapMasa veritabanından icra verisini büro veritabanına aktar (--deneme: yaz, say, geri al).
    if args.len() > 1 && args[1] == "--uyapmasa-aktar" {
        std::process::exit(crate::icra_aktar::komut(&args[2..]));
    }
    // Buluttaki İş Listesi'ni büro veritabanına taşı (--deneme: yaz, say, geri al; --dosya: arşivden).
    if args.len() > 1 && args[1] == "--is-listesi-aktar" {
        std::process::exit(crate::is_aktar::komut(&args[2..]));
    }
    if args.len() > 1 && args[1] == "--icra-durum-guncelle" {
        std::process::exit(crate::icra_aktar::durum_komut(&args[2..]));
    }
    // Kurulum paketi: tam PostgreSQL kurulumundan yalnız gömülü sunucu dosyalarını al.
    if args.len() > 3 && args[1] == "--pgsql-hazirla" {
        match crate::veritabani::pgsql_hazirla(Path::new(&args[2]), Path::new(&args[3])) {
            Ok(n) => {
                println!("pgsql hazır: {:.1} MB", n as f64 / 1_048_576.0);
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("pgsql hazırlanamadı: {e}");
                std::process::exit(1);
            }
        }
    }

    // Light yalnız UYAP'ı paylaşır: toplu iş başlatmayı kapat
    std::env::set_var("UYAP_TOPLU_IS", "0");

    // Uzak bilgisayarda cokluuyap.com'dan açılan UYAP'ın "Paylaş"ı belgeyi buraya verir (panoya, Ctrl+V).
    crate::yerel_yardimci::baslat();
    // İmzalı güncelleme denetimi (src/guncelleme.rs): yayın derlemesinde 6 saatte bir; kurulum kullanıcı onayıyla.
    crate::guncelleme::baslat();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Çoklu UYAP - Kontrol Ekranı")
            .with_decorations(false)
            .with_transparent(true)
            .with_icon(std::sync::Arc::new(egui::IconData {
                rgba: include_bytes!("../assets/armonia-simge-68.rgba").to_vec(),
                width: 68,
                height: 68,
            }))
            .with_inner_size([LOGIN_CARD_W, 560.0]),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Çoklu UYAP - Kontrol Ekranı",
        options,
        Box::new(|cc| {
            configure_egui_fonts(&cc.egui_ctx);
            // Ortak tema (web ile aynı dosya); yoksa Armonia.
            crate::uyap_theme::tema_sec(&crate::uyap_theme::tema_dosyadan_oku().unwrap_or_else(|| "armonia".into()));
            crate::uyap_theme::gorunum_uygula(&cc.egui_ctx);
            #[allow(unused_mut)]
            let mut app = UyapApp::new();
            #[cfg(windows)]
            {
                app.tepsi = crate::tepsi::kur(cc.egui_ctx.clone());
            }
            Ok(Box::new(app))
        }),
    );
}
