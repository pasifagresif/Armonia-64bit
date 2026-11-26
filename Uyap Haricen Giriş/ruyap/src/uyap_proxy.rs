//! UYAP Office-Side Gateway (Transparent Reverse Proxy)
//! (`uyap_core/uyap_proxy.py` 1:1 Rust portu)
//! ----------------------------------------------------
//! Runs on the OFFICE machine that holds the e-imza smart card.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{ConnectInfo, Query, Request as AxumRequest};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, options, post};
use axum::Router;
use base64::Engine as _;
use clap::Parser;
use md5::{Digest as _, Md5};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rand::RngCore;
use reqwest::cookie::Jar;
use reqwest::Client;
use serde::Deserialize;
use sha2::Sha256;
use tokio::sync::Mutex as AsyncMutex;
use zip::ZipArchive;

use crate::jobs;
use crate::runtime::{interactive_write, is_write_method, uyap_istek_izi};
use crate::uyap_giris_disaridan as uyap_login;
use crate::yerel_kapi;

pub const UYAP_BASE: &str = "https://avukat.uyap.gov.tr";
pub const UYAP_HOST: &str = "avukat.uyap.gov.tr";

// ── Yerel Django arayüzü (uyap_gecit/web, ayrı süreç, 127.0.0.1:8000) ─────────────────
pub const GECIT_PREFIX: &str = "__gecit__";
pub const GECIT_BASE: &str = "http://127.0.0.1:8000";

static GECIT_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .tcp_nodelay(true)
        .pool_max_idle_per_host(16)
        .build()
        .expect("gecit reqwest client build")
});

static GECIT_STATIC_ROOT: Lazy<PathBuf> = Lazy::new(|| {
    let mut base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(p) = base.parent() {
        base = p.to_path_buf();
    }
    base.join("uyap_gecit").join("web").join("gecit").join("static")
});

/// Kaldırılan eski İş Listesi yolları (`/__uyap_kisi__/is-listesi…`, `dosya-ac`, `ptt`).
fn _eski_is_listesi_yolu(path: &str) -> bool {
    let p = format!("/{}", path.trim_start_matches('/'));
    p == "/__uyap_kisi__/is-listesi" || p.starts_with("/__uyap_kisi__/is-listesi/")
        || p == "/__uyap_kisi__/dosya-ac" || p == "/__uyap_kisi__/ptt"
}

pub fn is_gecit_path(path: &str) -> bool {
    let p = path.trim_start_matches('/');
    p == GECIT_PREFIX || p.starts_with(&format!("{}/", GECIT_PREFIX))
}

fn guess_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html",
        Some("css") => "text/css",
        Some("js") | Some("mjs") => "application/javascript",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}

pub fn _serve_gecit_static(rel_path: &str) -> (u16, HashMap<String, String>, Vec<u8>) {
    let rel = Path::new(rel_path);
    if rel.is_absolute() {
        let mut h = HashMap::new();
        h.insert("Content-Type".into(), "text/plain; charset=utf-8".into());
        return (404, h, b"Not found".to_vec());
    }
    let mut clean = PathBuf::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(c) => clean.push(c),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                let mut h = HashMap::new();
                h.insert("Content-Type".into(), "text/plain; charset=utf-8".into());
                return (404, h, b"Not found".to_vec());
            }
        }
    }
    let fs_path = GECIT_STATIC_ROOT.join(&clean);
    if !fs_path.is_file() {
        let mut h = HashMap::new();
        h.insert("Content-Type".into(), "text/plain; charset=utf-8".into());
        return (404, h, b"Not found".to_vec());
    }
    match std::fs::read(&fs_path) {
        Ok(data) => {
            let mut h = HashMap::new();
            h.insert("Content-Type".into(), guess_mime_type(&fs_path).into());
            h.insert("Cache-Control".into(), "public, max-age=3600".into());
            (200, h, data)
        }
        Err(_) => {
            let mut h = HashMap::new();
            h.insert("Content-Type".into(), "text/plain; charset=utf-8".into());
            (404, h, b"Not found".to_vec())
        }
    }
}

pub async fn handle_gecit_proxy(
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    headers: &HashMap<String, String>,
) -> (u16, HashMap<String, String>, Vec<u8>) {
    let trimmed = path.trim_start_matches('/');
    let sub_path = trimmed
        .get(GECIT_PREFIX.len()..)
        .unwrap_or("")
        .trim_start_matches('/');
    if sub_path.starts_with("static/") && matches!(method.to_ascii_uppercase().as_str(), "GET" | "HEAD") {
        return _serve_gecit_static(&sub_path["static/".len()..]);
    }
    let mut target = format!("{}/{}", GECIT_BASE, sub_path);
    if !query.is_empty() {
        target.push('?');
        target.push_str(query);
    }
    let req_method = reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);
    let mut req = GECIT_CLIENT.request(req_method, &target).body(body.to_vec());
    for (k, v) in headers {
        let lk = k.to_ascii_lowercase();
        if !HOP_BY_HOP.contains(&lk.as_str()) && lk != "host" && lk != "content-length" {
            req = req.header(k.as_str(), v.as_str());
        }
    }
    req = req.header("Host", "127.0.0.1:8000");

    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let mut out_headers = HashMap::new();
            for (k, v) in resp.headers() {
                let lk = k.as_str().to_ascii_lowercase();
                if !HOP_BY_HOP.contains(&lk.as_str()) && lk != "content-length" {
                    if let Ok(vs) = v.to_str() {
                        out_headers.insert(k.as_str().to_string(), vs.to_string());
                    }
                }
            }
            let content = resp.bytes().await.unwrap_or_default().to_vec();
            (status, out_headers, content)
        }
        Err(e) => {
            let mut h = HashMap::new();
            h.insert("Content-Type".into(), "text/plain; charset=utf-8".into());
            (
                502,
                h,
                format!("Geçit arayüzüne ulaşılamadı: {}", e).into_bytes(),
            )
        }
    }
}

pub const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

pub const DROP_REQUEST: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
    "cookie",
    "authorization",
    "content-length",
    "accept-encoding",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-uyap-local-token",
];

pub const DROP_RESPONSE: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "content-encoding",
    "content-length",
    "set-cookie",
    "content-security-policy",
    "content-security-policy-report-only",
    "strict-transport-security",
    "x-frame-options",
];

pub const TEXT_TYPES: &[&str] = &[
    "text/",
    "application/javascript",
    "application/x-javascript",
    "application/json",
    "application/xml",
    "image/svg",
];

pub fn kesilen_uc_yaniti(path: &str) -> Option<(u16, HashMap<String, String>, Vec<u8>)> {
    let ad = path.trim_start_matches('/').split('?').next().unwrap_or("");
    if ad == "getDuyurularYayinda.ajx" {
        let mut h = HashMap::new();
        h.insert(
            "Content-Type".into(),
            "application/json; charset=utf-8".into(),
        );
        h.insert("Cache-Control".into(), "no-store".into());
        Some((200, h, b"[]".to_vec()))
    } else {
        None
    }
}

pub static GW: Lazy<Mutex<Option<Arc<GatewaySession>>>> = Lazy::new(|| Mutex::new(None));
pub static IDLE_INTERVAL: AtomicU64 = AtomicU64::new(5);
pub static GW_USER: Lazy<Mutex<String>> =
    Lazy::new(|| Mutex::new(std::env::var("GW_USER").unwrap_or_else(|_| "uyap".into())));
pub static GW_PASS: Lazy<Mutex<Option<String>>> =
    Lazy::new(|| Mutex::new(std::env::var("GW_PASS").ok()));
pub static GW_LOCAL_TOKEN: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
pub static GW_LAN_TICKET: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Son geçici UYAP giriş hatası (e-imza istemcisi hazır değil, kart takılı değil, UYAP'a ulaşılamadı).
/// Kontrol Ekranı durum satırı için; başarılı girişte temizlenir. 2026-11-16.
pub static SON_GECICI_HATA: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Paylaşım başlarken UYAP oturumunu hemen açar (ilk sayfa isteğini beklemez): kullanıcı "Bağlantı kuruldu"yu
/// ya da hatayı hemen görür. Geçici hatada 20 sn arayla yeniden dener; PIN hatası kapıyı kilitleyince durur.
/// Paylaşımın tokio çalışma zamanında koşar; paylaşım durunca görev de düşer.
pub async fn oturumu_hemen_ac() {
    *SON_GECICI_HATA.lock() = None;
    tokio::time::sleep(Duration::from_millis(500)).await;
    loop {
        let Some(gw) = get_gw() else { return };
        if gw.check_giris_kapisi().is_err() {
            return;
        }
        match gw.ensure_alive().await {
            Ok(()) => return,
            Err(_) if gw.check_giris_kapisi().is_err() => return,
            Err(_) => tokio::time::sleep(Duration::from_secs(20)).await,
        }
    }
}

pub fn get_gw() -> Option<Arc<GatewaySession>> {
    GW.lock().clone()
}

pub fn set_gw_lan_ticket(ticket: Option<String>) {
    *GW_LAN_TICKET.lock() = ticket;
}

#[derive(Debug, Clone)]
pub struct LoginArgs {
    pub pin: Option<String>,
    pub cert_id: Option<String>,
}

pub struct GatewaySession {
    pub login_args: LoginArgs,
    pub verify_ssl: bool,
    pub cert_id: Mutex<Option<String>>,
    pub jar: Arc<Jar>,
    pub client: Client,
    pub lock: AsyncMutex<()>,
    pub last_request_ts: Mutex<Instant>,
    pub logged_in: AtomicBool,
    pub giris_hatasi: Mutex<Option<String>>,
}

impl GatewaySession {
    pub fn new(login_args: LoginArgs, verify_ssl: bool) -> Self {
        let jar = Arc::new(Jar::default());
        let client = Client::builder()
            .danger_accept_invalid_certs(!verify_ssl)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(15))
            .tcp_nodelay(true)
            .pool_max_idle_per_host(32)
            .redirect(reqwest::redirect::Policy::none())
            .cookie_provider(jar.clone())
            .build()
            .expect("reqwest client build");
        Self {
            cert_id: Mutex::new(login_args.cert_id.clone()),
            login_args,
            verify_ssl,
            jar,
            client,
            lock: AsyncMutex::new(()),
            last_request_ts: Mutex::new(Instant::now()),
            logged_in: AtomicBool::new(false),
            giris_hatasi: Mutex::new(None),
        }
    }

    pub fn touch(&self) {
        *self.last_request_ts.lock() = Instant::now();
    }

    pub fn check_giris_kapisi(&self) -> Result<(), String> {
        if let Some(ref err) = *self.giris_hatasi.lock() {
            return Err(format!("UYAP girişi başarısız: {}", err));
        }
        Ok(())
    }

    pub async fn _probe(&self) -> bool {
        if self.check_giris_kapisi().is_err() {
            return false;
        }
        let url = format!("{}/get_avukat_id.ajx", UYAP_BASE);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({}))
            .header("Content-Type", "application/json")
            .header("Referer", format!("{}/", UYAP_BASE))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().as_u16() == 200 => {
                let txt = r.text().await.unwrap_or_default();
                let t = txt.trim();
                !t.is_empty() && t.chars().all(|c| c.is_ascii_digit())
            }
            _ => false,
        }
    }

    pub async fn _login_blocking(&self) -> Result<(), String> {
        let login_args = self.login_args.clone();
        let verify_ssl = self.verify_ssl;
        let cert_id = self.cert_id.lock().clone();

        let (cookies, used_cert_id) = tokio::task::spawn_blocking(move || {
            let args = uyap_login::LoginArgs {
                pin: login_args.pin.clone().unwrap_or_default(),
                cert_id: login_args.cert_id.clone(),
                ..Default::default()
            };
            let (session, used_id) =
                uyap_login::perform_login(&args, verify_ssl, cert_id.as_deref())?;
            let cookies = session.cookies.lock().unwrap().clone();
            Ok::<_, String>((cookies, used_id))
        })
        .await
        .map_err(|e| format!("Giris hatasi: {}", e))?
        .map_err(|e| format!("Giris hatasi: {}", e))?;

        *self.cert_id.lock() = Some(used_cert_id);
        let uyap_url = url::Url::parse(UYAP_BASE).unwrap();
        for c in cookies {
            let d = if c.domain.is_empty() {
                UYAP_HOST.to_string()
            } else {
                c.domain
            };
            let p = if c.path.is_empty() {
                "/".to_string()
            } else {
                c.path
            };
            let cookie_str = format!("{}={}; Domain={}; Path={}", c.name, c.value, d, p);
            self.jar.add_cookie_str(&cookie_str, &uyap_url);
        }
        self.logged_in.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub async fn ensure_alive(&self) -> Result<(), String> {
        if self.logged_in.load(Ordering::SeqCst) && self._probe().await {
            return Ok(());
        }
        self.relogin().await
    }

    pub async fn relogin(&self) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        if let Some(ref err) = *self.giris_hatasi.lock() {
            return Err(err.clone());
        }
        if self.logged_in.load(Ordering::SeqCst) && self._probe().await {
            return Ok(());
        }
        println!("[*] Oturum yok/koptu — e-imza ile yeniden giriş yapılıyor...");
        if let Err(e) = self._login_blocking().await {
            self.logged_in.store(false, Ordering::SeqCst);
            // Kapı yalnız PIN karta gittikten sonraki hatalarda kilitlenir (yanlış PIN tekrarı kartı bloke eder).
            // E-imza istemcisi hazır değil / kart takılı değil / UYAP'a ulaşılamadı gibi hatalar bir sonraki
            // istekte yeniden denenir (Armonia: istemci açılıştan ~20-30 sn sonra hazır olur). 2026-11-14.
            if e.contains(uyap_login::GECICI_HATA) {
                *SON_GECICI_HATA.lock() = Some(e.replace(uyap_login::GECICI_HATA, "").replace("Giris hatasi: ", ""));
                println!("[!] UYAP girişi şimdilik yapılamadı: {}", e.replace(uyap_login::GECICI_HATA, ""));
                return Err(e.replace(uyap_login::GECICI_HATA, ""));
            }
            let e = format!(
                "{e}. E-imza imzası alınamadı (PIN yanlış olabilir). Kart kilitlenmesin diye otomatik \
                 yeniden denenmeyecek: PIN'i düzeltip paylaşımı durdurup yeniden başlatın."
            );
            println!("[!] UYAP girişi başarısız: {e}");
            *self.giris_hatasi.lock() = Some(e.clone());
            return Err(e);
        }
        *SON_GECICI_HATA.lock() = None;
        println!("[+] Oturum hazır.");
        Ok(())
    }
}

pub fn _looks_like_login_wall(status: u16, headers: &reqwest::header::HeaderMap) -> bool {
    if status == 401 || status == 403 {
        return true;
    }
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        if let Some(loc) = headers.get("location").and_then(|v| v.to_str().ok()) {
            let low = loc.to_ascii_lowercase();
            if low.contains("/giris") || low.contains("login") {
                return true;
            }
        }
    }
    false
}

pub fn _build_forward_headers(headers: &HeaderMap) -> reqwest::header::HeaderMap {
    let mut out = reqwest::header::HeaderMap::new();
    for (k, v) in headers {
        let lk = k.as_str().to_ascii_lowercase();
        if !DROP_REQUEST.contains(&lk.as_str()) {
            if let (Ok(rn), Ok(rv)) = (
                reqwest::header::HeaderName::from_bytes(k.as_str().as_bytes()),
                reqwest::header::HeaderValue::from_bytes(v.as_bytes()),
            ) {
                out.append(rn, rv);
            }
        }
    }
    out.insert(
        reqwest::header::HOST,
        reqwest::header::HeaderValue::from_static(UYAP_HOST),
    );
    out
}

pub fn _build_forward_headers_map(headers: &HashMap<String, String>) -> reqwest::header::HeaderMap {
    let mut out = reqwest::header::HeaderMap::new();
    for (k, v) in headers {
        let lk = k.to_ascii_lowercase();
        if !DROP_REQUEST.contains(&lk.as_str()) {
            if let (Ok(rn), Ok(rv)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_bytes(v.as_bytes()),
            ) {
                out.insert(rn, rv);
            }
        }
    }
    out.insert(
        reqwest::header::HOST,
        reqwest::header::HeaderValue::from_static(UYAP_HOST),
    );
    out
}

pub fn _rewrite_body(content_type: &str, body: &[u8], proxy_base: &str, inject: bool) -> Vec<u8> {
    if content_type.is_empty() || !TEXT_TYPES.iter().any(|t| content_type.contains(t)) {
        return body.to_vec();
    }
    let Ok(mut text) = std::str::from_utf8(body).map(|s| s.to_string()) else {
        return body.to_vec();
    };
    let host_only = proxy_base
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(proxy_base);
    text = text.replace("https://avukat.uyap.gov.tr", proxy_base);
    text = text.replace("//avukat.uyap.gov.tr", &format!("//{}", host_only));

    if inject && content_type.to_ascii_lowercase().contains("text/html") {
        let script = format!(
            "<meta charset=\"utf-8\">{}{}{}",
            _tema_on_eki(),
            INLINE_INJECT_SCRIPT,
            _ek_betikler()
        );
        if let Some(pos) = text.find("<title>Uyap Avukat Bilgi Sistemi</title>") {
            let mut replaced = String::with_capacity(text.len());
            replaced.push_str(&text[..pos]);
            replaced.push_str("<title>Çoklu UYAP</title>");
            replaced.push_str(&text[pos + "<title>Uyap Avukat Bilgi Sistemi</title>".len()..]);
            text = replaced;
        }
        if let Some(pos) = text.find("<head>") {
            let mut replaced = String::with_capacity(text.len() + script.len());
            replaced.push_str(&text[..pos + "<head>".len()]);
            replaced.push_str(&script);
            replaced.push_str(&text[pos + "<head>".len()..]);
            text = replaced;
        } else if let Some(pos) = text.find("<body>") {
            let mut replaced = String::with_capacity(text.len() + script.len());
            replaced.push_str(&text[..pos + "<body>".len()]);
            replaced.push_str(&script);
            replaced.push_str(&text[pos + "<body>".len()..]);
            text = replaced;
        } else if _tam_html_belgesi_mi(&text) {
            text = format!("{}{}", script, text);
        }
        // Aksi halde gövde tam sayfa değil: UYAP'ın XHR ile aldığı "text/html" etiketli kısa yanıtlar (ör. evrak
        // görüntülenemediğinde dönen hata metni). UYAP bunları olduğu gibi uyarı penceresinde gösterir; başa stil/betik
        // eklersek pencerede armonia-renk.css'in ham metni görünüyordu (2026-11-26). Bu yanıtlara dokunulmaz.
    }

    text.into_bytes()
}

/// `<head>`/`<body>` etiketi bulunmayan gövdenin yine de tarayıcıda sayfa olarak açılacak bir HTML belgesi olup
/// olmadığı: yalnız `<!doctype` ya da `<html` ile başlıyorsa evet.
fn _tam_html_belgesi_mi(text: &str) -> bool {
    let bas: String = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .chars()
        .take(9)
        .collect::<String>()
        .to_ascii_lowercase();
    bas.starts_with("<!doctype") || bas.starts_with("<html")
}

const INLINE_INJECT_SCRIPT: &str = r#####"<script>
(function() {
    // 1. IndexedDB SWR Cache Setup
    const DB_NAME = 'UYAP_SWR_CACHE';
    const DB_VERSION = 1;
    const STORE_NAME = 'responses';

    function getDB() {
        return new Promise((resolve) => {
            const req = indexedDB.open(DB_NAME, DB_VERSION);
            req.onupgradeneeded = (e) => {
                const db = e.target.result;
                if (!db.objectStoreNames.contains(STORE_NAME)) {
                    db.createObjectStore(STORE_NAME);
                }
            };
            req.onsuccess = (e) => resolve(e.target.result);
            req.onerror = () => resolve(null);
        });
    }

    // Açık db tutamacı üzerinde ham oku/yaz (oturum-kapsamı kurulurken kullanılır;
    // bunlar cacheReady'yi BEKLEMEZ — yoksa init kilitlenir).
    function rawGet(db, key) {
        return new Promise((resolve) => {
            try {
                const tx = db.transaction(STORE_NAME, 'readonly');
                const req = tx.objectStore(STORE_NAME).get(key);
                req.onsuccess = () => resolve(req.result);
                req.onerror = () => resolve(null);
            } catch (e) { resolve(null); }
        });
    }
    function rawSet(db, key, value) {
        return new Promise((resolve) => {
            try {
                const tx = db.transaction(STORE_NAME, 'readwrite');
                const req = tx.objectStore(STORE_NAME).put(value, key);
                req.onsuccess = () => resolve(true);
                req.onerror = () => resolve(false);
            } catch (e) { resolve(false); }
        });
    }

    // GÜVENLİK (bulgu #5): Cache'i tarayıcı OTURUMUNA bağla. Hassas UYAP yanıtları artık
    // "sonsuza kadar kalıcı" değil; yalnızca aktif oturum süresince yaşar. sessionStorage
    // sekme kapanınca silinir → yeni sekme/tarayıcı/SONRAKİ KULLANICI farklı SID üretir →
    // miras kalan kalıcı IndexedDB ilk erişimde tamamen SİLİNİR (ortak/halka açık makinede
    // veri sızıntısını engeller). Aynı oturum içi navigasyon/yeniden yükleme hızı korunur.
    const SID_KEY = 'UYAP_SWR_SID';
    let SID = null;
    try { SID = sessionStorage.getItem(SID_KEY); } catch (e) {}
    if (!SID) {
        SID = (window.crypto && crypto.randomUUID) ? crypto.randomUUID()
              : (Date.now().toString(36) + Math.random().toString(36).slice(2));
        try { sessionStorage.setItem(SID_KEY, SID); } catch (e) {}
    }

    function purgeCache() {
        return new Promise((resolve) => {
            try {
                const r = indexedDB.deleteDatabase(DB_NAME);
                r.onsuccess = r.onerror = r.onblocked = () => resolve();
            } catch (e) { resolve(); }
        });
    }

    // Oturum mührünü doğrula: depodaki SID mührü mevcut oturumunkiyle uyuşmuyorsa cache
    // ÖNCEKİ bir oturuma aittir → tümünü sil, taze aç, yeni mührü bas. Tüm dbGet/dbSet bunu
    // bekler; böylece sızdıran eski veri hiçbir okumada görünmez.
    const cacheReady = (async () => {
        let db = await getDB();
        if (!db) return;
        let storedSid = null;
        try { storedSid = await rawGet(db, '__sid__'); } catch (e) {}
        if (storedSid !== SID) {
            try { db.close(); } catch (e) {}
            await purgeCache();
            db = await getDB();
            if (db) { try { await rawSet(db, '__sid__', SID); } catch (e) {} }
        }
    })();

    function dbGet(key) {
        return new Promise(async (resolve) => {
            await cacheReady;
            const db = await getDB();
            if (!db) { resolve(null); return; }
            resolve(await rawGet(db, key));
        });
    }

    function dbSet(key, value) {
        return new Promise(async (resolve) => {
            await cacheReady;
            const db = await getDB();
            if (!db) { resolve(false); return; }
            resolve(await rawSet(db, key, value));
        });
    }

    // 2. Premium Glassmorphic Toast Notification System
    function showToast(message, action) {
        let container = document.getElementById('uyap-toast-container');
        if (!container) {
            container = document.createElement('div');
            container.id = 'uyap-toast-container';
            container.style.cssText = `
                position: fixed;
                bottom: 24px;
                right: 24px;
                z-index: 2147483647;
                display: flex;
                flex-direction: column;
                gap: 12px;
                font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
                pointer-events: none;
            `;
            document.body.appendChild(container);
        }
        
        if (container.querySelector(`[data-msg="${message}"]`)) {
            return;
        }
        
        const toast = document.createElement('div');
        toast.setAttribute('data-msg', message);
        toast.style.cssText = `
            background: rgba(var(--cu-panel-rgb), 0.85);
            backdrop-filter: blur(20px);
            -webkit-backdrop-filter: blur(20px);
            color: #f3f4f6;
            padding: 14px 24px;
            border-radius: 16px;
            border: 1px solid rgba(255, 255, 255, 0.08);
            box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
            display: flex;
            align-items: center;
            gap: 16px;
            transform: translateY(20px);
            opacity: 0;
            transition: all 0.5s cubic-bezier(0.16, 1, 0.3, 1);
            pointer-events: auto;
        `;
        
        const textSpan = document.createElement('span');
        textSpan.textContent = message;
        textSpan.style.cssText = `
            font-size: 14px;
            font-weight: 500;
            letter-spacing: -0.01em;
        `;
        toast.appendChild(textSpan);
        
        if (action) {
            const btn = document.createElement('button');
            btn.textContent = 'Yenile';
            btn.style.cssText = `
                background: linear-gradient(135deg, var(--cu-vurgu), var(--cu-vurgu-orta));
                color: #ffffff;
                border: none;
                padding: 8px 16px;
                border-radius: 10px;
                cursor: pointer;
                font-size: 13px;
                font-weight: 600;
                box-shadow: 0 4px 12px rgba(var(--cu-vurgu-rgb), 0.3);
                transition: all 0.2s ease;
                white-space: nowrap;
            `;
            btn.onmouseover = () => {
                btn.style.transform = 'translateY(-1px)';
                btn.style.boxShadow = '0 6px 16px rgba(var(--cu-vurgu-rgb), 0.4)';
            };
            btn.onmouseout = () => {
                btn.style.transform = 'translateY(0)';
                btn.style.boxShadow = '0 4px 12px rgba(var(--cu-vurgu-rgb), 0.3)';
            };
            btn.onclick = () => {
                action();
                toast.remove();
            };
            toast.appendChild(btn);
        }
        
        container.appendChild(toast);
        
        requestAnimationFrame(() => {
            toast.style.transform = 'translateY(0)';
            toast.style.opacity = '1';
        });
        
        setTimeout(() => {
            toast.style.transform = 'translateY(20px)';
            toast.style.opacity = '0';
            setTimeout(() => { toast.remove(); }, 500);
        }, 8000);
    }

    // 3. Smart Comparison Logic
    function isIgnoredKey(key) {
        const ignored = [
            'sistemtarihi', 'sorgutarihi', 'sorgusaati', 'time', 'timestamp', 
            'date', 'token', 'sectoken', 'transactionid', 'nonce', 'uuid', 
            'sorgu_saati', 'sorgu_tarihi', 'sistem_tarihi', 'last_sync', 'sync_time'
        ];
        return ignored.includes(key.toLowerCase());
    }

    function isDeepEqual(a, b) {
        if (a === b) return true;
        if (a && b && typeof a === 'object' && typeof b === 'object') {
            if (a.constructor !== b.constructor) return false;
            if (Array.isArray(a)) {
                if (a.length !== b.length) return false;
                for (let i = 0; i < a.length; i++) {
                    if (!isDeepEqual(a[i], b[i])) return false;
                }
                return true;
            }
            const keysA = Object.keys(a).filter(key => !isIgnoredKey(key));
            const keysB = Object.keys(b).filter(key => !isIgnoredKey(key));
            if (keysA.length !== keysB.length) return false;
            for (let key of keysA) {
                if (!b.hasOwnProperty(key)) return false;
                if (!isDeepEqual(a[key], b[key])) return false;
            }
            return true;
        }
        return false;
    }

    function hasDataChanged(oldText, newText) {
        if (oldText === newText) return false;
        try {
            const objA = JSON.parse(oldText);
            const objB = JSON.parse(newText);
            return !isDeepEqual(objA, objB);
        } catch (e) {
            return oldText !== newText;
        }
    }

    // 4. Cache Filter and Blacklist
    function isCacheable(url, method) {
        if (!url || !method) return false;
        if (method.toUpperCase() !== 'POST' && method.toUpperCase() !== 'GET') return false;
        const url_lower = url.toLowerCase();
        
        // Must be a UYAP JSON/text API endpoint
        if (!url_lower.includes('.ajx') && !url_lower.includes('.uyap')) return false;

        // İkili belgeler (PDF/zip/UDF/TIFF) ASLA önbelleğe alınmaz: önbellek yanıtı resp.text()
        // ile saklayıp 'application/json' diye geri verdiği için ikinci açılışta belge bozuk
        // geliyordu (canlı, 2026-10-14: 70 KB zip → 128 KB bozuk metin).
        if (url_lower.includes('view_document') || url_lower.includes('_document_')) return false;

        // Blacklist for state-modifying actions, auth, live ws and balance checks
        const blacklist = [
            'kaydet', 'gonder', 'sil', 'ekle', 'guncelle', 'imza', 'odeme',
            'harc', 'tahsilat', 'baslat', 'onayla', 'reddet', 'iptal',
            'save', 'delete', 'update', 'add', 'sign', 'payment', 'confirm', 'cancel',
            'create', 'send', 'upload', 'download', 'cikis', 'logout',
            'sifre', 'pin', 'parola', 'odemeYap', 'kart', 'nats', 'bakiye', 'bakiyesi',
            'hazirla'   // getIcraTalepEvrakHazirla: her seferinde yeni talep evrakı (UDF) üretir
        ];
        for (let word of blacklist) {
            if (url_lower.includes(word)) return false;
        }
        return true;
    }

    // GÜVENLİK (bulgu #5): Oturum kapanışı (çıkış) işareti — görülürse hassas cache'i hemen
    // sil. Oturum-mührü zaten sonraki oturumda temizler; bu, aynı makinede oturum kapatıp
    // bırakan kullanıcı için ek koruma.
    function isLogout(url) {
        if (!url) return false;
        const u = url.toLowerCase();
        return u.includes('cikis') || u.includes('logout') || u.includes('oturumkapat');
    }

    // 5. Bypass loginState check in localStorage
    localStorage.setItem('loginState', 'true');

    // 6. XMLHttpRequest Interceptor
    const RealXHR = window.XMLHttpRequest;
    
    function FakeXHR() {
        const xhr = new RealXHR();
        const self = this;
        
        let isFake = false;
        let fakeResponse = null;
        let method = '';
        let url = '';
        let requestHeaders = [];
        const listeners = {};
        
        const props = [
            'readyState', 'status', 'statusText', 'responseText', 'responseXML',
            'response', 'responseURL', 'upload', 'timeout', 'withCredentials',
            'responseType'  // ikili belgeler (blob/arraybuffer) gerçek XHR'a iletilmeli
        ];
        
        props.forEach(prop => {
            Object.defineProperty(self, prop, {
                get: function() {
                    if (isFake && fakeResponse) {
                        if (prop === 'readyState') return 4;
                        if (prop === 'status') return 200;
                        if (prop === 'statusText') return 'OK';
                        if (prop === 'responseText' || prop === 'response') return fakeResponse.responseText;
                        if (prop === 'responseXML') return null;
                    }
                    return xhr[prop];
                },
                set: function(val) {
                    try {
                        xhr[prop] = val;
                    } catch (e) {}
                },
                configurable: true,
                enumerable: true
            });
        });
        
        const eventProps = [
            'onreadystatechange', 'onload', 'onloadstart', 'onloadend',
            'onprogress', 'onerror', 'onabort', 'ontimeout'
        ];
        
        const appHandlers = {};
        eventProps.forEach(evt => {
            Object.defineProperty(self, evt, {
                get: function() {
                    return appHandlers[evt] || null;
                },
                set: function(val) {
                    appHandlers[evt] = val;
                    if (!isFake) {
                        xhr[evt] = function() {
                            if (appHandlers[evt]) {
                                appHandlers[evt].apply(self, arguments);
                            }
                        };
                    }
                },
                configurable: true,
                enumerable: true
            });
        });
        
        self.addEventListener = function(type, listener, options) {
            if (!listeners[type]) listeners[type] = [];
            listeners[type].push({ listener, options });
            
            const wrappedListener = function(e) {
                if (isFake) return;
                return listener.apply(self, arguments);
            };
            
            xhr.addEventListener(type, wrappedListener, options);
        };
        
        self.removeEventListener = function(type, listener, options) {
            if (listeners[type]) {
                listeners[type] = listeners[type].filter(l => l.listener !== listener);
            }
            xhr.removeEventListener(type, listener, options);
        };
        
        self.open = function(m, u, async, user, password) {
            method = m;
            url = u;
            return xhr.open.apply(xhr, arguments);
        };
        
        self.setRequestHeader = function(header, value) {
            requestHeaders.push({ header, value });
            return xhr.setRequestHeader.apply(xhr, arguments);
        };
        
        self.getResponseHeader = function(header) {
            if (isFake) {
                if (header.toLowerCase() === 'content-type') return 'application/json';
                return null;
            }
            return xhr.getResponseHeader.apply(xhr, arguments);
        };
        
        self.getAllResponseHeaders = function() {
            if (isFake) {
                return 'content-type: application/json\\r\\n';
            }
            return xhr.getAllResponseHeaders.apply(xhr);
        };
        
        self.abort = function() {
            return xhr.abort.apply(xhr);
        };
        
        self.overrideMimeType = function(mime) {
            return xhr.overrideMimeType.apply(xhr, arguments);
        };
        
        self.send = async function(data) {
            if (isLogout(url)) { purgeCache(); }
            if (isCacheable(url, method)) {
                const cacheKey = `xhr::${url}::${data || ''}`;
                const cached = await dbGet(cacheKey);
                
                if (cached) {
                    isFake = true;
                    fakeResponse = cached;

                    setTimeout(() => {
                        triggerFakeEvents();
                    }, 1);
                    
                    // Perform background revalidation using a separate originalFetch call!
                    const revalidate = async () => {
                        try {
                            const headersObj = {};
                            requestHeaders.forEach(h => {
                                headersObj[h.header] = h.value;
                            });
                            
                            const response = await originalFetch(url, {
                                method: method,
                                headers: headersObj,
                                body: data
                            });
                            
                            if (response.status === 200) {
                                const newText = await response.text();
                                if (hasDataChanged(cached.responseText, newText)) {
                                    await dbSet(cacheKey, { responseText: newText });
                                }
                            }
                        } catch(e) {}
                    };
                    revalidate();
                    
                    // Do NOT call native xhr.send()! We return immediately.
                    return;
                } else {
                    xhr.addEventListener('load', async function() {
                        if (xhr.status === 200) {
                            await dbSet(cacheKey, { responseText: xhr.responseText });
                        }
                    });
                }
            }

            return xhr.send.apply(xhr, arguments);
        };
        
        function triggerFakeEvents() {
            if (appHandlers['onreadystatechange']) {
                try { appHandlers['onreadystatechange'].call(self); } catch(e) {}
            }
            if (appHandlers['onload']) {
                try { appHandlers['onload'].call(self); } catch(e) {}
            }
            if (listeners['load']) {
                listeners['load'].forEach(l => {
                    try { l.listener.call(self, new Event('load')); } catch(e) {}
                });
            }
            if (listeners['readystatechange']) {
                listeners['readystatechange'].forEach(l => {
                    try { l.listener.call(self, new Event('readystatechange')); } catch(e) {}
                });
            }
            if (appHandlers['onloadend']) {
                try { appHandlers['onloadend'].call(self); } catch(e) {}
            }
        }
    }
    
    FakeXHR.prototype = RealXHR.prototype;
    for (let key in RealXHR) {
        if (RealXHR.hasOwnProperty(key)) {
            FakeXHR[key] = RealXHR[key];
        }
    }
    FakeXHR.toString = function() { return 'function XMLHttpRequest() { [native code] }'; };
    window.XMLHttpRequest = FakeXHR;

    // 7. Fetch API Interceptor
    // window'a BAĞLA: native fetch, this !== window ile çağrılırsa Chrome "Illegal
    // invocation" fırlatır. Belge görüntüleyici fetch'i farklı bir bağlamda çağırdığı
    // için bind şart; bağlı fonksiyon apply/call'daki this'i yok sayar.
    const originalFetch = window.fetch.bind(window);
    window.fetch = async function(resource, init) {
        const url = typeof resource === 'string' ? resource : resource.url;
        const method = init && init.method ? init.method : 'GET';

        if (isLogout(url)) { purgeCache(); }
        if (isCacheable(url, method)) {
            const data = (init && init.body) ? init.body : '';
            const cacheKey = `fetch::${url}::${data}`;
            const cached = await dbGet(cacheKey);
            
            if (cached) {
                originalFetch(resource, init).then(async (resp) => {
                    if (resp.status === 200) {
                        const text = await resp.text();
                        const cachedVal = await dbGet(cacheKey);
                        if (cachedVal && hasDataChanged(cachedVal.text, text)) {
                            await dbSet(cacheKey, { text });
                        }
                    }
                }).catch(() => {});
                
                return new Response(cached.text, {
                    status: 200,
                    statusText: 'OK',
                    headers: new Headers({ 'content-type': 'application/json' })
                });
            } else {
                const resp = await originalFetch(resource, init);
                if (resp.status === 200) {
                    const clone = resp.clone();
                    clone.text().then(async (text) => {
                        await dbSet(cacheKey, { text });
                    });
                }
                return resp;
            }
        }
        return originalFetch.apply(this, arguments);
    };
})();

// ── UDF E-İMZA KÖPRÜSÜ (sürükle-bırak) ───────────────────────────────────────────────
// Sabit düğmesi yok: alan, sayfaya belge sürüklenince ya da Çoklu UYAP çekmecesinden (Araçlar → E-İmza)
// açılır. Kullanıcı bir .udf / .doc / .docx / .pdf dosyasını oraya sürükler ya da tıklayıp seçer. Dosya ofise gider; ofis gerekiyorsa UDF'ye çevirip
// e-imzayla imzalar (headless) ve imzalı .udf geri gelir. Sonra UYAP'ta bir dosya yükleme
// alanına tıklandığında imzalı UDF otomatik yerleştirilir (DataTransfer).
(function() {
    const SIGN_ENDPOINT = '/__uyap_agent__/sign_udf';
    const ACCEPT = ['.udf', '.doc', '.docx', '.pdf'];
    let staged = null;                  // {name, bytes(Uint8Array)} — imzalanmış, yüklemeye hazır
    let pending = null;                 // {name, bytes(Uint8Array)} — onay bekleyen (henüz imzalanmadı)
    let collapsed = true;               // varsayılan: alan gizli (sürükleme / çekmece açar)

    function whenBody(cb) {
        if (document.body) { cb(); return; }
        document.addEventListener('DOMContentLoaded', cb, { once: true });
    }

    // sayfa yenilense bile (SPA içi) imzalı belgeyi koru
    try {
        const s = sessionStorage.getItem('uyapStagedUdf');
        if (s) { const o = JSON.parse(s); staged = { name: o.name, bytes: Uint8Array.from(atob(o.b64), c => c.charCodeAt(0)) }; }
    } catch (e) {}

    function persistStaged() {
        try {
            if (!staged) { sessionStorage.removeItem('uyapStagedUdf'); return; }
            let bin = ''; staged.bytes.forEach(b => bin += String.fromCharCode(b));
            sessionStorage.setItem('uyapStagedUdf', JSON.stringify({ name: staged.name, b64: btoa(bin) }));
        } catch (e) {}
    }

    function setStatus(text, kind) {
        const el = document.getElementById('uyap-eimza-status');
        if (!el) return;
        el.textContent = text || '';
        el.style.color = kind === 'err' ? '#fca5a5' : (kind === 'ok' ? '#86efac' : '#cbd5e1');
    }

    function extOf(name) { const m = /\.[^.]+$/.exec((name || '').toLowerCase()); return m ? m[0] : ''; }

    // ── Açılır/kapanır görünüm ────────────────────────────────────────────────────────
    function render() {
        const widget = document.getElementById('uyap-eimza-widget');
        if (!widget) return;
        // Yalnız ONAY bekleyen belge varken açık kalmaya zorlanır. Sabit düğme yok (2026-11-11):
        // alan; belge sürüklenince, UYAP'tan belge indirilince ya da Çoklu UYAP çekmecesinden
        // (Araçlar → E-İmza) açılır. "İmzalı belge hazır" bilgisi çekmecede ve tutamaçta görünür.
        if (pending) collapsed = false;
        widget.style.display = collapsed ? 'none' : 'block';
    }
    function expand() { collapsed = false; render(); }
    function collapse() { if (pending) return; collapsed = true; render(); }
    window.__uyapEimza = {
        ac: function() { buildWidget(); expand(); },
        durum: function() { return { hazir: !!staged, ad: staged ? staged.name : '', bekleyen: !!pending }; },
    };
    // Pencereye belge sürüklenince alan kendiliğinden belirir ve "Buraya bırakın" hedefi olur.
    // Kapanma: sürükleme sürdükçe tarayıcı ~50 ms'de bir dragover gönderir; 600 ms gelmezse
    // sürükleme bitmiş ya da pencereden çıkılmıştır. (dragleave KULLANILMAZ: Chrome her öğe
    // sınırında dragleave atar ve relatedTarget çoğu zaman boştur → alan bırakmadan kapanıyordu,
    // kullanıcı bulgusu 2026-11-11.)
    // suruklemeVar: sayfada belge sürükleniyor; suruklemeActi: alanı bu sürükleme açtı.
    let suruklemeVar = false, suruklemeActi = false, suruklemeZamani = null;
    function belgeSurukleniyor(ev) {
        const t = ev.dataTransfer && ev.dataTransfer.types;
        return !!(t && Array.prototype.indexOf.call(t, 'Files') !== -1);
    }
    function hedefVurgu(acik) {
        const w = document.getElementById('uyap-eimza-widget');
        const ipucu = document.getElementById('uyap-eimza-birak');
        if (!w) return;
        w.style.width = acik ? '260px' : '210px';
        w.style.padding = acik ? '22px 14px' : '12px';
        w.style.borderColor = acik ? 'var(--cu-vurgu-parlak)' : 'rgba(255,255,255,0.18)';
        w.style.borderWidth = acik ? '2px' : '1px';
        if (ipucu) ipucu.style.display = acik ? 'block' : 'none';
    }
    function suruklemeBitti(birakildi) {
        clearTimeout(suruklemeZamani);
        if (!suruklemeVar) return;
        suruklemeVar = false;
        hedefVurgu(false);
        // Yalnız sürüklemenin açtığı alan kapanır; kullanıcı önceden açtıysa açık kalır.
        if (!birakildi && suruklemeActi && !pending) collapse();
        suruklemeActi = false;
    }
    window.addEventListener('dragover', function(ev) {
        if (!belgeSurukleniyor(ev)) return;
        if (!suruklemeVar) {
            suruklemeVar = true;
            buildWidget();
            suruklemeActi = collapsed;
            if (!pending && !staged) resetActions();   // önceki denemenin mesajı kalmasın
            expand();
            hedefVurgu(true);
        }
        clearTimeout(suruklemeZamani);
        suruklemeZamani = setTimeout(function() { suruklemeBitti(false); }, 600);
    }, true);
    window.addEventListener('drop', function(ev) {
        // Alanın üstüne bırakıldıysa açık kalır (onay ya da hata mesajı görünsün).
        const alanda = !!(ev.target && ev.target.closest && ev.target.closest('#uyap-eimza-widget'));
        suruklemeBitti(alanda);
    }, true);
    window.addEventListener('dragend', function() { suruklemeBitti(false); }, true);

    // ── Dosya seçilince: ÖNCE onay iste (otomatik imzalama yok) ────────────────────────
    function stageForConfirm(file) {
        if (!file) return;
        const ext = extOf(file.name);
        if (ACCEPT.indexOf(ext) === -1) {
            expand();
            setStatus('Desteklenmeyen tür: ' + (ext || '?') + ' (.udf/.doc/.docx/.pdf)', 'err');
            return;
        }
        pending = file;
        expand();
        setStatus('');
        const wrap = document.getElementById('uyap-eimza-actions');
        if (!wrap) return;
        wrap.innerHTML = '';
        const info = document.createElement('div');
        info.style.cssText = 'font-size:12px;color:#e5e7eb;margin-bottom:8px;word-break:break-all;';
        info.textContent = '📄 ' + file.name;
        const row = document.createElement('div');
        row.style.cssText = 'display:flex;gap:6px;';
        const ok = mkBtn('E-imzala', 'var(--cu-vurgu)', () => { const f = pending; pending = null; processFile(f); });
        const no = mkBtn('Vazgeç', 'rgba(255,255,255,0.10)', () => { pending = null; resetActions(); collapse(); });
        row.appendChild(ok); row.appendChild(no);
        wrap.appendChild(info); wrap.appendChild(row);
    }

    function mkBtn(label, bg, onclick) {
        const b = document.createElement('button');
        b.textContent = label;
        b.style.cssText = 'flex:1;background:' + bg + ';color:' + (/--cu-vurgu/.test(bg) ? 'var(--cu-vurgu-uzeri,#fff)' : '#fff') + ';border:1px solid rgba(255,255,255,0.15);padding:6px 10px;border-radius:8px;cursor:pointer;font-size:12px;font-weight:600;';
        b.onclick = (ev) => { ev.stopPropagation(); onclick(); };
        return b;
    }
    function resetActions() {
        const wrap = document.getElementById('uyap-eimza-actions');
        if (wrap) wrap.innerHTML = '';
        setStatus('');
    }

    // ── İmzalama akışı (onaylandıktan sonra) ──────────────────────────────────────────
    async function processFile(file) {
        if (!file) return;
        resetActions();
        setStatus('"' + file.name + '" ofise gönderiliyor, e-imzalanıyor…');
        try {
            const buf = new Uint8Array(await file.arrayBuffer());
            const url = SIGN_ENDPOINT + '?name=' + encodeURIComponent(file.name);
            const resp = await window.fetch(url, {
                method: 'POST', body: buf,
                headers: { 'Content-Type': 'application/octet-stream' }
            });
            if (!resp.ok) {
                let msg = 'İmzalama başarısız (' + resp.status + ').';
                try { const j = await resp.json(); if (j && j.error) msg = j.error; } catch (e) {}
                setStatus(msg, 'err');
                return;
            }
            const signed = new Uint8Array(await resp.arrayBuffer());
            const signedName = file.name.replace(/\.[^.]+$/i, '') + '_imzali.udf';
            staged = { name: signedName, bytes: signed };
            persistStaged();
            setStatus('Hazır ✓ "Dosya Ekle/Yükle" alanına tıklayınca otomatik eklenecek.', 'ok');
            showDownload(signed, signedName);
            // Sonucu kısa süre göster, sonra küçük düğmeye (✅) dön; indirme için
            // düğmeye tıklayıp yeniden açılabilir.
            setTimeout(() => { if (!pending) { collapsed = true; render(); } }, 3000);
        } catch (e) {
            setStatus('Hata: ' + e, 'err');
        }
    }

    function showDownload(signed, signedName) {
        const wrap = document.getElementById('uyap-eimza-actions');
        if (!wrap) return;
        wrap.innerHTML = '';
        const b = mkBtn('İmzalıyı indir', 'rgba(255,255,255,0.12)', () => {
            const u = URL.createObjectURL(new Blob([signed], { type: 'application/octet-stream' }));
            const a = document.createElement('a'); a.href = u; a.download = signedName;
            a.setAttribute('data-uyap-internal', '1');   // kendi indirmemiz — yakalama dışı
            document.body.appendChild(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(u), 4000);
        });
        wrap.appendChild(b);
    }

    // ── Sürükle-bırak alanı (görev çubuğunun üstünde, sağda) ─────────────────────────
    function buildWidget() {
        if (document.getElementById('uyap-eimza-root')) return;
        const w = document.createElement('div');
        // 2026-09-23: dış kap AYRI id taşır. Eskiden iç panelle aynı id'yi
        // ('uyap-eimza-widget') paylaşıyordu; render() getElementById ile DIŞ kabı
        // bulup gizliyor, mor 🖋️ düğmesi de onunla birlikte kayboluyordu.
        w.id = 'uyap-eimza-root';
        // Alttan 56 px: görev çubuğunun (44 px) hemen üstü; sağdan 44 px: sağ kenardaki "Loglamayı
        // Başlat" düğmesinin görünen ~28 px'ini örtmez.
        w.style.cssText = 'position:fixed;bottom:56px;right:44px;z-index:2147483647;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;';

        // Açık widget
        const widget = document.createElement('div');
        widget.id = 'uyap-eimza-widget';
        widget.style.cssText = 'display:none;width:210px;padding:12px;border-radius:14px;background:rgba(var(--cu-panel-rgb),0.92);backdrop-filter:blur(20px);-webkit-backdrop-filter:blur(20px);border:1px dashed rgba(255,255,255,0.18);box-shadow:0 10px 30px rgba(0,0,0,0.35);color:#f3f4f6;cursor:pointer;transition:border-color .2s,background .2s;';
        widget.innerHTML =
            '<div style="display:flex;align-items:center;justify-content:space-between;">' +
              '<div style="font-size:13px;font-weight:700;display:flex;align-items:center;gap:6px;"><span style="font-size:15px;">🖋️</span> E-İmza</div>' +
              '<span id="uyap-eimza-collapse" title="Küçült" style="font-size:16px;line-height:1;color:#9ca3af;cursor:pointer;padding:0 4px;">–</span>' +
            '</div>' +
            '<div id="uyap-eimza-birak" style="display:none;margin-top:10px;font-size:13px;font-weight:700;color:var(--cu-vurgu-acik);text-align:center;">Belgeyi buraya bırakın<br><span style="font-weight:400;font-size:11.5px;">bıraktıktan sonra imza için onay istenir</span></div>' +
            '<div style="font-size:11.5px;color:#cbd5e1;margin-top:6px;line-height:1.4;">Belgeyi <b>sürükle</b> ya da tıkla.<br>.udf · .doc · .docx · .pdf</div>' +
            '<div id="uyap-eimza-status" style="font-size:11.5px;margin-top:8px;min-height:14px;line-height:1.4;color:#cbd5e1;"></div>' +
            '<div id="uyap-eimza-actions" style="margin-top:8px;"></div>';

        const input = document.createElement('input');
        input.type = 'file';
        input.accept = ACCEPT.join(',');
        input.style.display = 'none';
        input.addEventListener('change', () => { if (input.files && input.files[0]) stageForConfirm(input.files[0]); input.value = ''; });
        widget.appendChild(input);

        // widget boşluğuna tıklayınca dosya seçici (buton/küçült/eylem alanına tıklayınca değil)
        widget.addEventListener('click', (ev) => {
            const t = ev.target;
            if (t && (t.id === 'uyap-eimza-collapse' || (t.closest && t.closest('#uyap-eimza-actions')))) return;
            input.click();
        });

        // sürükle-bırak widget üzerine
        ['dragenter', 'dragover'].forEach(t => widget.addEventListener(t, (ev) => {
            ev.preventDefault(); ev.stopPropagation();
            widget.style.borderColor = 'var(--cu-vurgu-orta)'; widget.style.background = 'rgba(var(--cu-vurgu-rgb),0.18)';
        }));
        ['dragleave', 'dragend'].forEach(t => widget.addEventListener(t, (ev) => {
            ev.preventDefault(); ev.stopPropagation();
            // Sayfa üzerinde sürükleme sürüyorsa "bırakma hedefi" çerçevesi kalır.
            widget.style.borderColor = suruklemeVar ? 'var(--cu-vurgu-parlak)' : 'rgba(255,255,255,0.18)';
            widget.style.background = 'rgba(var(--cu-panel-rgb),0.92)';
        }));
        widget.addEventListener('drop', (ev) => {
            ev.preventDefault(); ev.stopPropagation();
            widget.style.borderColor = 'rgba(255,255,255,0.18)'; widget.style.background = 'rgba(var(--cu-panel-rgb),0.92)';
            const f = ev.dataTransfer && ev.dataTransfer.files && ev.dataTransfer.files[0];
            if (f) stageForConfirm(f);
        });

        w.appendChild(widget);
        document.body.appendChild(w);

        const cb = document.getElementById('uyap-eimza-collapse');
        if (cb) cb.addEventListener('click', (ev) => { ev.stopPropagation(); collapse(); });

        if (staged) { setStatus('İmzalı belge hazır — yükleme alanına tıklayın.', 'ok'); showDownload(staged.bytes, staged.name); }
        render();
    }

    // ── Dosya yükleme alanını otomatik doldur ────────────────────────────────────────
    function fillFileInput(input) {
        if (!staged) return false;
        try {
            const dt = new DataTransfer();
            dt.items.add(new File([staged.bytes], staged.name, { type: 'application/octet-stream' }));
            input.files = dt.files;
            input.dispatchEvent(new Event('input', { bubbles: true }));
            input.dispatchEvent(new Event('change', { bubbles: true }));
            setStatus('İmzalı belge yükleme alanına eklendi ✓', 'ok');
            const actions = document.getElementById('uyap-eimza-actions'); if (actions) actions.innerHTML = '';
            staged = null; persistStaged();   // tek seferlik: tüketildi
            setTimeout(() => { collapsed = true; render(); }, 1500);  // tüketilince tekrar küçülebilir
            return true;
        } catch (e) { return false; }
    }
    document.addEventListener('click', function(ev) {
        const el = ev.target;
        // kendi gizli inputumuza dokunma
        if (el && el.tagName === 'INPUT' && el.type === 'file' && staged
            && !(el.closest && el.closest('#uyap-eimza-widget'))) {
            ev.preventDefault();
            ev.stopPropagation();
            fillFileInput(el);
        }
    }, true);

    // ── İndirilen belgeyi yakalayıp "imzalayayım mı?" diye sor ───────────────────────
    // UYAP'ta bir .udf/.doc/.docx/.pdf indirme bağlantısına tıklanınca: dosya normal
    // iner AMA biz de baytları çekip E-İmza alanının onay akışına (stageForConfirm) sokarız.
    let _nativeFetch = null;
    function nativeFetch() {
        if (_nativeFetch) return _nativeFetch;
        try {
            const f = document.createElement('iframe');
            f.style.display = 'none';
            (document.body || document.documentElement).appendChild(f);
            _nativeFetch = f.contentWindow.fetch.bind(window);   // cache katmanına uğramayan ham fetch
            // iframe'i bilerek DOM'da bırakırız ki fetch realm'i yaşasın
        } catch (e) { _nativeFetch = window.fetch.bind(window); }
        return _nativeFetch;
    }
    function nameFromUrl(href) {
        try { const u = new URL(href, location.href); return decodeURIComponent((u.pathname.split('/').pop()) || ''); }
        catch (e) { return ''; }
    }
    async function offerSignForDownload(href, name) {
        try {
            const resp = await nativeFetch()(href, { credentials: 'include' });
            if (!resp || !resp.ok) return;
            const u8 = new Uint8Array(await resp.arrayBuffer());
            if (!u8.length) return;
            stageForConfirm(new File([u8], name, { type: 'application/octet-stream' }));
        } catch (e) {}
    }
    document.addEventListener('click', function(ev) {
        const a = ev.target && ev.target.closest && ev.target.closest('a[href]');
        if (!a || a.getAttribute('data-uyap-internal')) return;
        const href = a.href;
        if (!href || href.toLowerCase().indexOf('javascript:') === 0) return;
        let name = a.getAttribute('download') || nameFromUrl(href);
        const ext = extOf(name) || extOf(href);
        if (ACCEPT.indexOf(ext) === -1) return;
        if (!name || extOf(name) === '') name = 'belge' + ext;
        // indirme normal devam etsin; biz ek olarak imza için soralım
        offerSignForDownload(href, name);
    }, true);

    whenBody(buildWidget);
})();

// ── DOSYA SORGULAMA: Yargı Türü/Birimi otomatik aç-seç ───────────────────────────────
// Bu ekran DevExtreme SelectBox kullanır. Dosya Sorgulama -> "yargiTur"/"yargiBirim",
// dava/takip açma -> "yargiTuru"/"yargiBirimi" (id'ler ekrana göre değişir).
// İstenen davranış:
//   1) Ekran açılınca Yargı Türü açılır (kullanıcı fazladan tık yapmasın).
//   2) Bir tür seçilince Yargı Birimi (asenkron yüklenir) açılır; TEK seçenek varsa
//      (ör. İcra) otomatik seçilir, birden çoksa kullanıcının seçmesi için açık kalır.
// Native <select> programatik açılamadığından DevExtreme'in DOM'una gerçekçi pointer/mouse
// olayları gönderiyoruz (openOnFieldClick etkin). Instance API'sine bağımlı değiliz.
(function() {
    function q(id) { return document.getElementById(id); }
    function rootOf(input) { return input && input.closest ? input.closest('.dx-dropdowneditor') : null; }
    function isDisabled(input) { const r = rootOf(input); return !!(r && r.classList.contains('dx-state-disabled')); }
    function isOpen(input) { const r = rootOf(input); return !!(r && r.classList.contains('dx-dropdowneditor-active')); }

    function dispatch(el, ctor, type, extra) {
        try { el.dispatchEvent(new ctor(type, Object.assign({ bubbles: true, cancelable: true, view: window }, extra || {}))); }
        catch (e) {}
    }
    function clickLike(el) {
        if (!el) return;
        dispatch(el, window.PointerEvent || MouseEvent, 'pointerdown', { pointerId: 1, pointerType: 'mouse', isPrimary: true });
        dispatch(el, MouseEvent, 'mousedown');
        dispatch(el, window.PointerEvent || MouseEvent, 'pointerup', { pointerId: 1, pointerType: 'mouse', isPrimary: true });
        dispatch(el, MouseEvent, 'mouseup');
        dispatch(el, MouseEvent, 'click');
    }
    function openSelect(input) {
        if (!input || isDisabled(input) || isOpen(input)) return;
        const r = rootOf(input);
        const btn = r && r.querySelector('.dx-dropdowneditor-button');
        clickLike(btn || input);
    }
    function visiblePopup() {
        const ws = document.querySelectorAll('.dx-selectbox-popup-wrapper');
        for (let i = 0; i < ws.length; i++) {
            const c = ws[i].querySelector('.dx-overlay-content');
            if (c && c.offsetParent !== null) return ws[i];
        }
        return null;
    }
    function selectItem(item) { clickLike(item); }

    // Ekrana göre id'ler değişir: Dosya Sorgulama -> "yargiTur"/"yargiBirim",
    // Dava/takip açma -> "yargiTuru"/"yargiBirimi". İlk var olan eşleşmeyi kullan.
    const TUR_IDS = ['yargiTur', 'yargiTuru'];
    const BIRIM_IDS = ['yargiBirim', 'yargiBirimi'];
    function firstPresent(ids) { for (let i = 0; i < ids.length; i++) { const e = q(ids[i]); if (e) return e; } return null; }
    function birimEl() { return firstPresent(BIRIM_IDS); }
    function ilEl() { return q('il'); }   // yalnız Ceza/Cbs (yargiTur=3) iken render edilir

    // Genel: bir SelectBox'ı aç; popup dolunca TEK seçenek varsa otomatik seç, çok seçenek
    // varsa açık bırak (kullanıcı seçsin). Boşsa (liste async yükleniyor / başka alana
    // bağlı) yüklenmeyi bekler, gerçekten boşsa sessizce bırakır — boş kutuyu zorlamayız.
    // getEl her turda yeniden çözülür: React re-render edip elemanı değiştirse de takip eder.
    function handleSelect(getEl, autoSelectSingle) {
        let tries = 0;
        const iv = setInterval(function() {
            tries++;
            const input = getEl();
            if (!input || tries > 55) { clearInterval(iv); return; }   // ~10 sn
            if (isDisabled(input)) return;            // hazır/etkin olana dek bekle
            if (!isOpen(input) && !visiblePopup()) { openSelect(input); return; }
            const wrap = visiblePopup();
            if (!wrap) return;                        // popup henüz çizilmedi
            const items = wrap.querySelectorAll('.dx-list-item');
            if (items.length === 1 && autoSelectSingle !== false) { clearInterval(iv); selectItem(items[0]); return; }
            if (items.length >= 1) { clearInterval(iv); return; }   // çok seçenek: kullanıcı seçsin
            // boş: liste henüz yüklenmemiş olabilir; uzun süre boşsa bırak (kapatma).
            const empty = wrap.querySelector('.dx-empty-message');
            if (empty && tries > 45) { clearInterval(iv); }
        }, 180);
    }

    // Bir SelectBox'ı açık olana / zaman aşımına dek tekrar tekrar açmayı dener.
    // (Tek seferlik tık, DevExtreme widget'ı henüz hazır değilse kaçabiliyor.)
    function openWithRetry(getEl, maxTries) {
        let tries = 0;
        const iv = setInterval(function() {
            tries++;
            const el = getEl();
            if (!el || tries > (maxTries || 40)) { clearInterval(iv); return; }
            if (isOpen(el) || visiblePopup()) { clearInterval(iv); return; }
            if (!isDisabled(el)) openSelect(el);
        }, 150);
    }

    // Tür seçilince: Ceza/Cbs (il alanı render edilmişse) BİRİM il'e bağlı olduğundan ÖNCE
    // il'i aç; aksi halde doğrudan birimi aç (tek seçenekse — ör. İcra — otomatik seç).
    // Re-render'ın oturması için kısa gecikme.
    function afterTurChange() {
        if (Date.now() - kisayolZamani < 3000) return;   // kısayol birimi kendisi veriyor
        setTimeout(function() {
            if (ilEl()) handleSelect(ilEl);          // il çok seçenekli → kullanıcı seçer
            else handleSelect(birimEl);              // birim: tek seçenekse otomatik
        }, 500);
    }
    function afterIlChange() {
        setTimeout(function() { handleSelect(birimEl); }, 400);  // il seçilince birim yüklenir
    }

    // ── Dosya Sorgulama: Yargı Birimi seçilince Mahkeme açılır, Mahkeme seçilince tek kutuya geçilir ──
    // Yalnız Dosya Sorgulama'da (id'ler "yargiBirim" + "mahkeme"; UYAP chunk 3145). Mahkeme listesi
    // aranabilir: kutuya odaklanılır, "izmir 15" yazınca süzülür.
    function mahkemeEl() { return q('yargiBirim') ? q('mahkeme') : null; }
    function mahkemeAc() {
        if (!mahkemeEl()) return;
        handleSelect(mahkemeEl);
        let n = 0;
        const iv = setInterval(function() {
            const m = mahkemeEl();
            if (!m || ++n > 30) { clearInterval(iv); return; }
            if (isOpen(m)) { clearInterval(iv); if (document.activeElement !== m) m.focus(); }
        }, 150);
    }
    function tekKutuyaOdaklan() {
        const g = document.querySelector('[data-tekno] input');
        if (g && !g.value) setTimeout(function() { g.focus(); }, 150);
    }
    function alanDegeri(e) { return e ? (e.value || '') : null; }

    // ── Kısayol çubuğu: İcra Dairesi / Asliye Hukuk / … / Ağır Ceza ──
    // Dosya Sorgulama formunun üstünde. Tıklanınca seçim fare taklidiyle değil, doğrudan formun kendi
    // durum işlevleriyle yapılır (formData.setters.setSelectedYargiTur / setSelectedYargiBirimi; UYAP
    // chunk 3145, 2026-11-10). Birim değerleri (tablo kodu) yargiBirimleriSorgula_brd.ajx'ten bir kez
    // alınıp saklanır; ekran açılınca önceden çekildiğinden tık anında beklenmez. Mahkeme
    // açılmaz, yalnız odaklanır (yazınca süzülerek açılır); seçilince imleç "2026/1234" kutusuna geçer.
    // Dikkat: tür değişince UYAP'ın tür kutusu clearData(false) çağırıp birimi siler; bu yüzden birim,
    // tür kutusu yeni değeri gösterdikten sonra verilir.
    const KISAYOLLAR = [
        ['İcra', [['İcra Dairesi', '2', 'icra daire']]],
        ['Hukuk', [['Asliye Hukuk', '1', 'asliye hukuk'], ['Asliye Ticaret', '1', 'asliye ticaret'],
                   ['Sulh Hukuk', '1', 'sulh hukuk'], ['Aile', '1', 'aile'], ['Tüketici', '1', 'tüketici'],
                   ['İş', '1', 'iş mahkeme'], ['İcra Hukuk', '1', 'icra hukuk']]],
        ['Ceza', [['Ağır Ceza', '0', 'ağır ceza'], ['Asliye Ceza', '0', 'asliye ceza']]],
    ];
    const TUR_ADI = { '0': 'Ceza', '1': 'Hukuk', '2': 'İcra' };
    const BIRIM_ANAHTAR = 'cokluuyapYargiBirimleri';
    const birimListeleri = {};
    let kisayolZamani = 0;
    function kucuk(x) { return String(x || '').replace(/I/g, 'ı').replace(/İ/g, 'i').toLowerCase().replace(/\s+/g, ' ').trim(); }
    function birimListesiSakli(tur) {
        if (birimListeleri[tur]) return birimListeleri[tur];
        try {
            const o = JSON.parse(localStorage.getItem(BIRIM_ANAHTAR) || '{}') || {};
            const k = o[tur];
            if (k && Array.isArray(k.l) && k.l.length && Date.now() - k.t < 30 * 86400000) return (birimListeleri[tur] = k.l);
        } catch (e) {}
        return null;
    }
    const birimIstekleri = {};
    function birimListesiGetir(tur) {
        const s = birimListesiSakli(tur);
        if (s) return Promise.resolve(s);
        if (birimIstekleri[tur]) return birimIstekleri[tur];
        return (birimIstekleri[tur] = window.fetch('/yargiBirimleriSorgula_brd.ajx', { method: 'POST',
            headers: { 'Content-Type': 'application/json', 'Accept': 'application/json, text/plain, */*' },
            body: JSON.stringify({ yargiTuru: tur }) }).then(function(r) { return r.json(); }).then(function(j) {
            const l = (Array.isArray(j) ? j : []).filter(function(b) { return b && b.tablo; })
                .map(function(b) { return { kod: String(b.kod || ''), tablo: String(b.tablo) }; });
            if (l.length) {
                birimListeleri[tur] = l;
                try {
                    const o = JSON.parse(localStorage.getItem(BIRIM_ANAHTAR) || '{}') || {};
                    o[tur] = { t: Date.now(), l: l };
                    localStorage.setItem(BIRIM_ANAHTAR, JSON.stringify(o));
                } catch (e) {}
            }
            return l;
        }).catch(function() { return []; }).then(function(l) { delete birimIstekleri[tur]; return l; }));
    }
    function birimBul(liste, desen) {
        const l = liste || [];
        const bas = l.filter(function(b) { return kucuk(b.kod).indexOf(desen) === 0; });
        const ic = bas.length ? bas : l.filter(function(b) { return kucuk(b.kod).indexOf(desen) >= 0; });
        if (!ic.length) return l.length === 1 && desen === 'icra daire' ? l[0] : null;
        return ic.slice().sort(function(a, b) { return a.kod.length - b.kod.length; })[0];   // en yalın ad
    }
    // DevExtreme'in kendi oluşturduğu <input>'ta React bağı yok: React'in çizdiği ilk ataya çıkılır.
    function reactLifi(e) {
        for (; e && e.nodeType === 1; e = e.parentElement) {
            const a = Object.keys(e).find(function(k) { return k.indexOf('__reactFiber') === 0; });
            if (a) return e[a];
        }
        return null;
    }
    function dxBileseni(e) {   // girdinin DevExtreme bileşeni (devextreme-react sınıfının instance'ı)
        for (let f = reactLifi(e), i = 0; f && i < 15; i++, f = f.return) {
            const sn = f.stateNode;
            if (!sn || sn.nodeType || !('instance' in sn)) continue;
            try {
                const w = typeof sn.instance === 'function' ? sn.instance() : sn.instance;
                if (w && typeof w.option === 'function') return w;
            } catch (x) {}
        }
        return null;
    }
    function sorguFormu() {
        for (let f = reactLifi(document.querySelector('label[for="yargiTur"]') || q('yargiTur')), i = 0; f && i < 60; i++, f = f.return) {
            const fd = f.memoizedProps && f.memoizedProps.formData;
            if (fd && fd.setters && typeof fd.setters.setSelectedYargiTur === 'function'
                && typeof fd.setters.setSelectedYargiBirimi === 'function') return fd;
        }
        return null;
    }
    function turGorundu(tur) {   // tür kutusu yeni değeri gösterene (ve UYAP'ın temizliği işlenene) dek
        return new Promise(function(coz) {
            let n = 0;
            (function bak() {
                const t = q('yargiTur');
                if (!t || t.value.trim() === TUR_ADI[tur] || ++n > 60) return setTimeout(coz, 0);
                requestAnimationFrame(bak);
            })();
        });
    }
    async function kisayolUygula(tur, desen) {
        const form = sorguFormu();
        if (!form) { kisayolTiklaYedek(tur, desen); return; }
        kisayolZamani = Date.now();
        const liste = birimListesiGetir(tur);              // saklıysa hemen döner
        const turDegisti = q('yargiTur').value.trim() !== TUR_ADI[tur];
        if (turDegisti) form.setters.setSelectedYargiTur(tur);
        const b = birimBul(await liste, desen);
        if (turDegisti) await turGorundu(tur);
        kisayolZamani = Date.now();
        if (!b) { handleSelect(function() { return q('yargiBirim'); }, false); return; }   // ad eşleşmedi: listeyi aç
        // Birim listesi UYAP'tan gelene dek kutu boş görünmesin: saklı liste kutuya hemen verilir
        // (UYAP'ın listesi gelince kendi listesiyle değiştirir; değerler aynı).
        const bw = dxBileseni(q('yargiBirim'));
        if (bw) {
            try {
                const ds = bw.option('dataSource');
                const var_ = Array.isArray(ds) && ds.some(function(x) { return x && String(x.tablo) === b.tablo; });
                if (!var_) bw.option('dataSource', await liste);
            } catch (e) {}
        }
        const f2 = sorguFormu() || form;
        if (!(f2.state && f2.state.selectedYargiBirimi === b.tablo)) f2.setters.setSelectedYargiBirimi(b.tablo);
        // Mahkeme kendiliğinden açılmaz; kutuya odaklanılır: yazınca liste süzülerek açılır.
        requestAnimationFrame(function() {
            const m = mahkemeEl();
            if (m && !isDisabled(m)) m.focus();
        });
    }
    // Form bulunamazsa (başka sürüm/ekran): fare taklidiyle tür, ardından birim adıyla seçilir.
    function listedenSec(getEl, eslesir, bitti) {
        let n = 0;
        const iv = setInterval(function() {
            const e = getEl();
            if (!e || ++n > 60) { clearInterval(iv); return; }
            if (isDisabled(e)) return;
            const w = isOpen(e) ? visiblePopup() : null;
            if (!w) { openSelect(e); return; }
            const it = Array.prototype.find.call(w.querySelectorAll('.dx-list-item'), function(x) { return eslesir(kucuk(x.textContent)); });
            if (it) { clearInterval(iv); selectItem(it); if (bitti) setTimeout(bitti, 500); }
        }, 120);
    }
    function kisayolTiklaYedek(tur, desen) {
        kisayolZamani = Date.now();
        const birim = function() {
            kisayolZamani = Date.now();
            listedenSec(birimEl, function(m) { return m.indexOf(desen) >= 0; });
        };
        const t = q('yargiTur');
        if (t && t.value.trim() === TUR_ADI[tur]) birim();
        else listedenSec(function() { return q('yargiTur'); }, function(m) { return m === kucuk(TUR_ADI[tur]); }, birim);
    }
    function kisayolCubuguKur() {
        const label = document.querySelector('label[for="yargiTur"]');
        const satir = label && label.closest('.row');
        if (!satir || !satir.parentElement) return;
        const onceki = satir.previousElementSibling;
        if (onceki && onceki.hasAttribute('data-kisayollar')) return;
        const cubuk = document.createElement('div');
        cubuk.setAttribute('data-kisayollar', '1');
        cubuk.style.cssText = 'display:flex;flex-wrap:wrap;align-items:center;gap:6px 14px;margin:0 0 12px;';
        KISAYOLLAR.forEach(function(g) {
            const grup = document.createElement('div');
            grup.style.cssText = 'display:flex;flex-wrap:wrap;align-items:center;gap:4px;';
            const ad = document.createElement('span');
            ad.textContent = g[0] + ':';
            ad.style.cssText = 'font-size:12px;font-weight:600;color:#6c757d;margin-right:2px;';
            grup.appendChild(ad);
            g[1].forEach(function(k) {
                const b = document.createElement('button');
                b.type = 'button';
                b.textContent = k[0];
                b.title = TUR_ADI[k[1]] + ' → ' + k[0] + ' (Mahkeme kutusuna geçilir; yazarak arayın)';
                b.style.cssText = 'padding:2px 10px;border-radius:12px;border:1px solid #0d6efd;background:#fff;color:#0d6efd;font-size:12px;line-height:18px;cursor:pointer;';
                b.addEventListener('mouseenter', function() { b.style.background = '#0d6efd'; b.style.color = '#fff'; });
                b.addEventListener('mouseleave', function() { b.style.background = '#fff'; b.style.color = '#0d6efd'; });
                b.addEventListener('mousedown', function(ev) { ev.preventDefault(); ev.stopPropagation(); });
                b.addEventListener('click', function(ev) { ev.preventDefault(); ev.stopPropagation(); kisayolUygula(k[1], k[2]); });
                grup.appendChild(b);
            });
            cubuk.appendChild(grup);
        });
        satir.parentElement.insertBefore(cubuk, satir);
        ['0', '1', '2'].forEach(function(t) { birimListesiGetir(t); });   // tık anında beklenmesin
    }

    let lastTur = null;
    let lastIl = null;
    let lastBirim = null, lastMahkeme = null;
    let opened = false;
    setInterval(function() {
        const t = firstPresent(TUR_IDS);
        if (!t) { lastTur = null; lastIl = null; lastBirim = null; lastMahkeme = null; opened = false; return; }  // ekran yok
        kisayolCubuguKur();
        if (!opened) {                               // Yargı Türü kutusu ilk göründü
            opened = true;
            lastTur = t.value || '';
            const il = ilEl(); lastIl = il ? (il.value || '') : null;
            lastBirim = alanDegeri(q('yargiBirim'));
            lastMahkeme = alanDegeri(mahkemeEl());
            // Ana sayfadan doldurularak gelindiyse tür kutusu açılmasın (değerleri dolduran kendisi verir).
            if (!t.value && Date.now() - kisayolZamani > 10000) openWithRetry(function() { return firstPresent(TUR_IDS); });
            return;
        }
        const cb = alanDegeri(q('yargiBirim'));      // birim seçildi → Mahkeme listesini aç
        if (cb !== lastBirim) {
            lastBirim = cb;
            // Kısayolla seçildiyse açma (kullanıcı istemiyor); elle seçildiyse aç.
            if (cb && mahkemeEl() && Date.now() - kisayolZamani > 10000) setTimeout(mahkemeAc, 400);
        }
        const cm = alanDegeri(mahkemeEl());           // mahkeme seçildi → "2026/1234" kutusuna geç
        if (cm !== lastMahkeme) {
            lastMahkeme = cm;
            if (cm) tekKutuyaOdaklan();
        }
        const ct = t.value || '';
        if (ct !== lastTur) {                         // tür değişti (kullanıcı seçti)
            lastTur = ct;
            const il = ilEl(); lastIl = il ? (il.value || '') : null;
            if (ct) afterTurChange();
            return;
        }
        const il = ilEl();                            // Ceza akışında il değişimini izle
        const ci = il ? (il.value || '') : null;
        if (ci !== lastIl) {
            lastIl = ci;
            if (ci) afterIlChange();
        }
    }, 400);

    // ── Dosya Yıl/No tek kutu: "2026/1234" ──
    // UYAP'ın "Dosya Yıl/ No" bileşeni (label[for=dosyaYilNo] + yıl SelectBox'ı + sıra NumberBox'ı) yılı
    // ve sırayı ayrı ister. Üstüne tek kutu eklenir: yazılan/yapıştırılan metindeki ilk "YYYY/NNN"
    // (ör. sağ tıkla kopyalanan "İzmir 15. İcra Dairesi 2026/1234") yıla ve sıraya yazılır; Enter'da
    // Sorgula'ya basılır. Değerler bileşenin kendi setParentDosyaNoBilgileri(anahtar, değer) işleviyle
    // verilir (UYAP chunk 3145/3632, 2026-11-10). O işlev ebeveynin o anki durumunu kopyalayıp üzerine
    // yazdığından (ke({...we, [k]: v})) yıl ve sıra arka arkaya verilemez: yıl yazılır, yeniden çizim
    // beklenir, sıra güncel çizimin işleviyle yazılır.
    function yilNoBilesenleri(label) {
        const a = Object.keys(label).find(function(k) { return k.indexOf('__reactFiber') === 0; });
        for (let f = a ? label[a] : null, i = 0; f && i < 12; i++, f = f.return) {
            const p = f.memoizedProps;
            if (p && typeof p.setParentDosyaNoBilgileri === 'function' && p.dosyaNoKey && p.dosyaYilNo)
                return f.alternate ? [f, f.alternate] : [f];
        }
        return [];
    }
    function yilNoBekle(label, kosul, sinir) {
        return new Promise(function(coz) {
            let n = 0;
            const iv = setInterval(function() {
                const f = yilNoBilesenleri(label).find(function(x) { return x.memoizedProps && kosul(x.memoizedProps); });
                if (f || ++n > (sinir || 30)) { clearInterval(iv); coz(f || null); }
            }, 40);
        });
    }
    async function yilNoYaz(label, yil, sira) {
        const ilk = yilNoBilesenleri(label)[0];
        if (!ilk) return false;
        const k = ilk.memoizedProps.dosyaNoKey;
        const deger = function(p, a) { return p.dosyaYilNo && p.dosyaYilNo[a] != null ? Number(p.dosyaYilNo[a]) : null; };
        ilk.memoizedProps.setParentDosyaNoBilgileri(k.yil, yil);
        const f = await yilNoBekle(label, function(p) { return deger(p, k.yil) === yil; });
        if (!f) return false;
        f.memoizedProps.setParentDosyaNoBilgileri(k.sira, sira);
        return !!(await yilNoBekle(label, function(p) { return deger(p, k.yil) === yil && deger(p, k.sira) === sira; }));
    }
    function sorgulaDugmesi(kutu) {
        for (let e = kutu.parentElement, i = 0; e && i < 12; i++, e = e.parentElement) {
            const b = Array.prototype.find.call(e.querySelectorAll('.dx-button'), function(x) {
                return x.textContent.trim() === 'Sorgula' && !x.classList.contains('dx-state-disabled');
            });
            if (b) return b;
        }
        return null;
    }
    // ── Sorgu tek dosya bulursa "Pencere Görünümü"ne (göz, #dosya-goruntule) basılır ──
    // Sorgula düğmesi "Sorgulanıyor..." olur, sonra geri döner; ardından sonuç tablosunun (DataGrid)
    // toplam kaydı 1 ise satırdaki göz düğmesine basılır. Eski sonuç tablosuna bakılmasın diye
    // "Sorgulanıyor..." görülmezse hiçbir şeye basılmaz.
    function sorguSuruyor() {
        return Array.prototype.some.call(document.querySelectorAll('.dx-button'), function(x) {
            return /^Sorgulan/.test(x.textContent.trim());
        });
    }
    function gorunur(e) { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && e.offsetParent !== null; }
    function sonucKaydi() {   // {sayi, dugme} | null
        const dugmeler = Array.prototype.filter.call(document.querySelectorAll('#dosya-goruntule'), gorunur);
        if (!dugmeler.length) return { sayi: 0, dugme: null };
        const izgara = dugmeler[0].closest('.dx-datagrid');
        const w = izgara && dxBileseni(izgara);
        let sayi = null;
        try { if (w && typeof w.totalCount === 'function') sayi = w.totalCount(); } catch (e) {}
        if (typeof sayi !== 'number' || sayi < 0) {   // yedek: sabit sütun kopyası olmayan satırlar
            sayi = izgara ? izgara.querySelectorAll('.dx-datagrid-rowsview .dx-datagrid-content:not(.dx-datagrid-content-fixed) .dx-data-row').length : dugmeler.length;
        }
        return { sayi: sayi, dugme: dugmeler[0] };
    }
    function tekSonucuAc() {
        const bas = Date.now();
        let basladi = false;
        const iv = setInterval(function() {
            const gecen = Date.now() - bas;
            if (gecen > 30000) { clearInterval(iv); return; }
            if (sorguSuruyor()) { basladi = true; return; }
            if (!basladi) {                               // sorgu başlamadıysa (uyarı çıktı vb.) eski tabloya bakma
                if (gecen > 2000) clearInterval(iv);
                return;
            }
            clearInterval(iv);
            setTimeout(function() {
                const s = sonucKaydi();
                if (s.sayi === 1 && s.dugme) clickLike(s.dugme);
            }, 300);
        }, 100);
    }
    function tekKutuKur() {
        document.querySelectorAll('[data-tekno]').forEach(function(k) {   // bileşeni kaldırılmış kutu
            const s = k.nextElementSibling;
            if (!s || s.getAttribute('for') !== 'dosyaYilNo') k.remove();
        });
        document.querySelectorAll('label[for="dosyaYilNo"]').forEach(function(label) {
            const onceki = label.previousElementSibling;
            if ((onceki && onceki.hasAttribute('data-tekno')) || !label.parentElement || !yilNoBilesenleri(label).length) return;
            const kutu = document.createElement('div');
            kutu.setAttribute('data-tekno', '1');
            kutu.style.cssText = 'margin-bottom:10px;';
            const girdi = document.createElement('input');
            girdi.type = 'text';
            girdi.autocomplete = 'off';
            girdi.placeholder = 'Dosya No: 2026/1234 — yapıştırın, Enter ile sorgulayın';
            girdi.setAttribute('aria-label', 'Dosya yıl/no (2026/1234)');
            girdi.style.cssText = 'width:100%;box-sizing:border-box;height:36px;padding:6px 10px;border:1px solid #ced4da;border-radius:6px;font-size:14px;outline:none;background:#fff;color:#212529;';
            const durum = document.createElement('div');
            durum.style.cssText = 'font-size:11px;margin-top:2px;color:#dc3545;';
            kutu.appendChild(girdi);
            kutu.appendChild(durum);
            let zaman = null, sira = 0;
            async function uygula(sorgula) {
                const benim = ++sira;
                const m = /(\d{4})\s*\/\s*(\d{1,8})(?!\d)/.exec(girdi.value);
                const yil = m ? Number(m[1]) : 0;
                if (!m || yil < 1900 || yil > new Date().getFullYear() + 1) {
                    durum.textContent = girdi.value.trim() && sorgula ? 'Biçim: 2026/1234' : '';
                    return;
                }
                const tamam = await yilNoYaz(label, yil, Number(m[2]));
                if (benim !== sira) return;                    // bu arada yeni yazım geldi
                durum.textContent = tamam ? '' : 'Yıl / No kutularına yazılamadı; elle girin.';
                if (!tamam || !sorgula) return;
                const b = sorgulaDugmesi(kutu);
                if (b) setTimeout(function() { clickLike(b); tekSonucuAc(); }, 80);
                else durum.textContent = 'Sorgula düğmesi bulunamadı; yargı türü ve birimini seçip Sorgula\'ya basın.';
            }
            girdi.addEventListener('input', function(ev) {
                // Klavyeyle yıl (4 hane) yazılınca "/" kendiliğinden eklenir; silerken ve yapıştırırken eklenmez.
                if (ev.inputType === 'insertText' && /^\d{4}$/.test(girdi.value)
                    && girdi.selectionStart === girdi.value.length) {
                    girdi.value += '/';
                    girdi.setSelectionRange(girdi.value.length, girdi.value.length);
                }
                clearTimeout(zaman);
                zaman = setTimeout(function() { uygula(false); }, 250);
            });
            girdi.addEventListener('keydown', function(ev) {
                if (ev.key !== 'Enter') return;
                ev.preventDefault();
                ev.stopPropagation();
                clearTimeout(zaman);
                uygula(true);
            });
            // Sağ tık: tarayıcı menüsü yerine iki madde — seçmeye gerek yok, kutunun tamamı kopyalanır;
            // yapıştır kutudakini silip panodakini koyar, yıl/no'yu doldurur ve Sorgula'ya basar.
            girdi.addEventListener('contextmenu', function(ev) {
                if (!window.__uyapSagMenuAc) return;
                ev.preventDefault();
                ev.stopPropagation();
                const bildir = window.__uyapBildirim || function() {};
                window.__uyapSagMenuAc(ev.clientX, ev.clientY, [[
                    ['Dosya numarasını kopyala', function() {
                        const m = girdi.value.trim();
                        if (!m) { bildir('Kutu boş'); return; }
                        window.__uyapPanoya(m);
                    }],
                    ['Yapıştır', async function() {
                        let m = '';
                        try { m = await navigator.clipboard.readText(); } catch (e) {
                            girdi.focus();
                            bildir('Pano okunamadı — Ctrl+V ile yapıştırın');
                            return;
                        }
                        girdi.value = String(m || '').replace(/\s+/g, ' ').trim();
                        girdi.focus();
                        girdi.setSelectionRange(girdi.value.length, girdi.value.length);
                        clearTimeout(zaman);
                        uygula(true);                      // yıl/no yazılır ve Sorgula'ya basılır
                    }],
                ]]);
            });
            girdi.addEventListener('focus', function() { girdi.style.borderColor = '#86b7fe'; });
            girdi.addEventListener('blur', function() { girdi.style.borderColor = '#ced4da'; });
            label.parentElement.insertBefore(kutu, label);
        });
    }
    setInterval(function() { if (document.body) tekKutuKur(); }, 700);

    // ── Ana sayfa konsolu (enjekte/anasayfa.js) için: formu doldurup UYAP'ın kendi sorgusunu çalıştır ──
    // p = {tur: '0'|'1'|'2', birim: tablo kodu, mahkeme: birimId|'' , kapali: bool, yil, sira}.
    // Çok sonuçlu aramada kullanılır: sonuç UYAP'ın kendi listesinde görünür. Değerler formun durum
    // işlevleriyle verilir (formData.setters, canlı 2026-11-11: setSelectedYargiTur/Birimi/Mahkeme,
    // setDosyaDurumu(true = açık)); sorgu formData.actions.onSearch ile başlatılır.
    function durumBekle(kosul, ms) {
        return new Promise(function(coz) {
            const bas = Date.now();
            (function bak() {
                const f = sorguFormu();
                if (f && kosul(f)) return coz(f);
                if (Date.now() - bas > (ms || 6000)) return coz(null);
                setTimeout(bak, 80);
            })();
        });
    }
    async function formuDoldurVeSorgula(p) {
        kisayolZamani = Date.now();
        let f = await durumBekle(function() { return true; }, 10000);
        if (!f) return 'Dosya Sorgulama formu açılamadı.';
        kisayolZamani = Date.now();
        if (f.state.selectedYargiTur !== p.tur) { f.setters.setSelectedYargiTur(p.tur); await turGorundu(p.tur); }
        f = await durumBekle(function(x) { return x.state.selectedYargiTur === p.tur; });
        if (!f) return 'Yargı türü seçilemedi.';
        const bw = dxBileseni(q('yargiBirim'));
        if (bw) {                                  // birim listesi henüz gelmediyse saklı listeyi ver
            try {
                const ds = bw.option('dataSource');
                if (!(Array.isArray(ds) && ds.some(function(x) { return x && String(x.tablo) === p.birim; }))) bw.option('dataSource', await birimListesiGetir(p.tur));
            } catch (e) {}
        }
        kisayolZamani = Date.now();
        f.setters.setSelectedYargiBirimi(p.birim);
        f = await durumBekle(function(x) { return x.state.selectedYargiBirimi === p.birim; });
        if (!f) return 'Yargı birimi seçilemedi.';
        if (f.state.dosyaDurumu !== !p.kapali) {
            f.setters.setDosyaDurumu(!p.kapali);
            f = (await durumBekle(function(x) { return x.state.dosyaDurumu === !p.kapali; })) || f;
        }
        if (p.mahkeme) {
            kisayolZamani = Date.now();
            f.setters.setSelectedMahkeme(p.mahkeme);
            f = (await durumBekle(function(x) { return String(x.state.selectedMahkeme) === String(p.mahkeme); })) || f;
        }
        const label = document.querySelector('label[for="dosyaYilNo"]');
        if (p.yil && label && !(await yilNoYaz(label, p.yil, p.sira))) return 'Dosya yıl/no yazılamadı.';
        const tek = document.querySelector('[data-tekno] input');
        if (tek && p.yil) tek.value = p.yil + '/' + p.sira;
        f = sorguFormu() || f;
        if (f.actions && typeof f.actions.onSearch === 'function') f.actions.onSearch();
        return '';
    }
    window.__uyapSorgu = { KISAYOLLAR: KISAYOLLAR, TUR_ADI: TUR_ADI, kucuk: kucuk, birimListesiGetir: birimListesiGetir,
                           birimBul: birimBul, formuDoldurVeSorgula: formuDoldurVeSorgula };
})();

// ── TÜM EVRAKI İNDİR ─────────────────────────────────────────────────────────────────
// Her dosya kartının (.dosya-sorgula-popup) başlığına UYAP'ın kendi düğme stiliyle "Tüm
// Evrakı İndir" eklenir. Tıklanınca dosya bilgisi O KARTIN React verisinden (props.data:
// dosyaId, birimAdi, dosyaNo…) okunur — tahmin yok. Ofis tüm evrakı sabit hızla, tek tek
// indirip PDF'e çevirir (bkz. uyap_core/evrak_toplu.py), ofiste klasöre kaydeder, PDF/ZIP'i
// buraya indirir. İlerleme sağ üstteki panelde (kart kapansa da) görünür. UYAP güvenlik
// doğrulaması isterse panel açılır ve doğrulamayı KULLANICI çözer — otomatik çözüm yok.
(function() {
    const EP = '/__uyap_agent__/tum_evrak';
    const BITMIS = ['bitti', 'hata', 'iptal'];
    const isler = {};          // indirme id -> {veri: sunucu durumu, ad, zipIndirildi}
    let tsYukleniyor = null;

    // ── Kart verisi: kart başlığındaki UYAP düğmesinin React fiber'ından yukarı yürü ──
    // Listeden açılan kartta veri başlıktaki düğmelerin props.data'sında; ana sayfa aramasından
    // açılan kartın başlığında bu düğmeler yok — veri içerikteki sekmelerin props'unda
    // (dosyabilgileri / dosyaBilgileri …) durur. İkisinde de {dosyaId, dosyaNo, …} nesnesi aranır.
    function fiberVerisi(el, derinlik, herProp) {
        const anahtar = Object.keys(el).find(function(a) { return a.indexOf('__reactFiber') === 0; });
        let f = anahtar ? el[anahtar] : null;
        for (let i = 0; f && i < derinlik; i++, f = f.return) {
            const p = f.memoizedProps;
            if (!p || typeof p !== 'object') continue;
            const adaylar = herProp ? Object.values(p) : [p.data];
            for (const d of adaylar) {
                if (d && typeof d === 'object' && !Array.isArray(d) && d.dosyaId && (!herProp || d.dosyaNo)) return d;
            }
        }
        return null;
    }
    function kartVerisi(kart) {
        for (const k of kart.querySelectorAll('.dx-popup-title [role=button], .dx-popup-title div')) {
            const d = fiberVerisi(k, 80, false);
            if (d) return d;
        }
        for (const k of kart.querySelectorAll('.custom-tab-header, .dx-tabpanel')) {
            const d = fiberVerisi(k, 12, true);
            if (d) return d;
        }
        return null;
    }
    // Kart başlığında düğmelerimizin dayanağı: "Kart Görünümü" (listeden açılan kart) yoksa UYAP'ın tam ekran
    // düğmesi. O düğmenin yazısı kart tam ekrandayken "Küçült"e döner (canlı, 2026-11-11): kart tam ekran
    // açıldığı için (kartlar.js) yalnız "Tam Ekran" yazısı aranınca dayanak bulunamıyor, "İşler" ve "Evrak
    // İndir" düğmeleri başlığa eklenemiyordu (görev çubuğundan geri açılan kartta hep). Yazının iki hâli ve
    // simgesi de kabul edilir; kendi pencere düğmelerimiz (kartlar.js: data-kucult, data-cu-tam) hariç.
    function baslikDayanagi(baslik) {
        const btn = Array.from(baslik.querySelectorAll('[role=button]'));
        return btn.find(function(b) { return /Kart Görünümü/.test(b.textContent); })
            || btn.find(function(b) {
                if (b.hasAttribute('data-kucult') || b.hasAttribute('data-cu-tam')) return false;
                return /Tam Ekran|Normal Ekran|Küçült/.test(b.textContent) || !!b.querySelector('.icon-maximize, .icon-minimize');
            }) || null;
    }
    window.__uyapBaslikDayanagi = baslikDayanagi;
    function dosyaAdi(d) {
        return [d.birimAdi, d.dosyaNo, d.dosyaTurKodAciklama].filter(Boolean).join(' ')
            .replace(/\s+/g, ' ').trim();
    }
    // İş Listesi bloğu (aşağıda) aynı kart verisini kullanır.
    window.__uyapKartVerisi = kartVerisi;
    window.__uyapDosyaAdi = dosyaAdi;

    // ── Kartlara düğme ekle (React kartı yeniden çizerse düğme de yeniden eklenir) ──
    // 2026-11-11: "Tüm Evrakı İndir" + "Seçili Belgeleri İndir (N)" TEK düğmede birleşti: "Evrak İndir"
    // (işaret varken "Evrak İndir (N)"). Kapsam (tümü / işaretliler) açılan formda seçilir.
    function kartDugmesi(baslik, ozellik, ikon, metin, ipucu) {
        const b = document.createElement('div');
        b.setAttribute(ozellik, '1');
        b.setAttribute('role', 'button');
        b.setAttribute('tabindex', '0');
        b.title = ipucu;
        b.className = 'dx-widget dx-button dx-button-mode-outlined dx-button-normal dx-button-has-text dx-button-has-icon adalet-button';
        b.style.marginRight = '6px';
        b.innerHTML = '<div class="dx-button-content"><span class="dx-icon fe ' + ikon + '"></span> <span data-metin>' + metin + '</span></div>';
        b.addEventListener('click', function(ev) {
            ev.stopPropagation();
            const kart = baslik.closest('.dosya-sorgula-popup');
            const d = kart && kartVerisi(kart);
            if (!d) { panelMesaj('Bu kartın dosya bilgisi okunamadı; kartı kapatıp yeniden açın.'); return; }
            secenekAc(kart, String(d.dosyaId), dosyaAdi(d), kartSecimi(kart).size ? 'secili' : 'tum');
        });
        return b;
    }
    function kartlariTara() {
        document.querySelectorAll('.dosya-sorgula-popup .dx-popup-title').forEach(function(baslik) {
            let tum = baslik.querySelector('[data-tumevrak]');
            if (!tum) {
                const dayanak = baslikDayanagi(baslik);
                const yer = dayanak ? dayanak.parentElement : null;
                if (!yer) return;
                tum = kartDugmesi(baslik, 'data-tumevrak', 'icon-download', 'Evrak İndir',
                                  'Bu dosyanın evrakını PDF olarak indir (tümü ya da Evrak sekmesinde işaretledikleriniz)');
                yer.insertBefore(tum, dayanak);
            }
            const kart = baslik.closest('.dosya-sorgula-popup');
            const n = kart ? kartSecimi(kart).size : 0;
            const metin = tum.querySelector('[data-metin]');
            const yeni = n ? 'Evrak İndir (' + n + ')' : 'Evrak İndir';
            if (metin && metin.textContent !== yeni) metin.textContent = yeni;
        });
    }

    // ── Kartın kendi evrak ağacında seçim ──
    // Ağacın verisi .evrak-treeview'in React fiber'ındaki props.items'tır; her düğümün id'si
    // ekrandaki satırın data-item-id'siyle aynı. Belge düğümlerinde UYAP'ın evrak kaydı (evrakId,
    // tur, birimEvrakNo…) durur; "(n ek)" düğümü ana belgedir, altındakiler ekleri. Seçim kart
    // başına evrakId ile tutulur: aynı evrak "Son 20" ve "Tüm Evrak" altında birlikte işaretlenir.
    function agacVerisi(tv) {
        const a = Object.keys(tv).find(function(k) { return k.indexOf('__reactFiber') === 0; });
        let f = a ? tv[a] : null;
        for (let i = 0; f && i < 20; i++, f = f.return) {
            const p = f.memoizedProps;
            const v = p && (p.items || p.dataSource);
            if (Array.isArray(v) && v.length && v[0] && typeof v[0].id === 'string') return v;
        }
        return null;
    }
    function dugumHaritasi(koklar) {
        const h = {};
        (function gez(dizi, ebeveyn) {
            dizi.forEach(function(x) { h[x.id] = { x: x, ebeveyn: ebeveyn }; if (x.items) gez(x.items, x); });
        })(koklar, null);
        return h;
    }
    function evrakOzeti(x, ebeveyn) {
        const e = x.evrak;
        const ana = ebeveyn && ebeveyn.evrak;
        if (x.isEkEvrak && ana) {
            return { evrakId: String(e.evrakId), tur: ((ana.tur || 'evrak') + ' (ek) ' + (x.text || '')).trim(),
                     birimEvrakNo: e.birimEvrakNo || ((ana.birimEvrakNo || '') + '-ek'), onaylandigiTarih: e.onaylandigiTarih || ana.onaylandigiTarih || '' };
        }
        return { evrakId: String(e.evrakId), tur: e.tur || x.text || 'evrak',
                 birimEvrakNo: e.birimEvrakNo || '', onaylandigiTarih: e.onaylandigiTarih || '' };
    }
    function altEvraklar(x, ebeveyn) {           // düğüm + altındaki tüm belgeler
        const sonuc = [];
        (function gez(d, eb) {
            if (d.evrak && d.evrak.evrakId) sonuc.push(evrakOzeti(d, eb));
            (d.items || []).forEach(function(c) { gez(c, d); });
        })(x, ebeveyn);
        return sonuc;
    }
    function kartSecimi(kart) {
        if (!kart.__tumEvrakSecim) kart.__tumEvrakSecim = new Set();
        return kart.__tumEvrakSecim;
    }
    // İndirme sırası: "Tüm Evrak" ağacının sırası (Son 20 sonra; tekrarlar atlanır).
    function secilenEvraklar(kart) {
        const tv = kart.querySelector('.evrak-treeview');
        const koklar = tv && agacVerisi(tv);
        const secim = kartSecimi(kart);
        if (!koklar || !secim.size) return [];
        const sirali = koklar.slice().sort(function(a, b) { return (a.id === '1_1' ? 0 : 1) - (b.id === '1_1' ? 0 : 1); });
        const sonuc = [], gorulen = new Set();
        sirali.forEach(function(k) {
            altEvraklar(k, null).forEach(function(e) {
                if (secim.has(e.evrakId) && !gorulen.has(e.evrakId)) { gorulen.add(e.evrakId); sonuc.push(e); }
            });
        });
        return sonuc;
    }
    function durdur(ev) { ev.stopPropagation(); }
    function agaclariTara() {
        document.querySelectorAll('.dosya-sorgula-popup .evrak-treeview').forEach(function(tv) {
            const kart = tv.closest('.dosya-sorgula-popup');
            const koklar = agacVerisi(tv);
            if (!kart || !koklar) return;
            const harita = dugumHaritasi(koklar);
            const secim = kartSecimi(kart);
            tv.querySelectorAll('.dx-treeview-node').forEach(function(node) {
                const d = harita[node.getAttribute('data-item-id')];
                const satir = node.querySelector(':scope > .dx-treeview-item');
                if (!d || !satir) return;
                let c = satir.querySelector(':scope > input[data-tumevrak-sec]');
                if (!c) {
                    c = document.createElement('input');
                    c.type = 'checkbox';
                    c.setAttribute('data-tumevrak-sec', '1');
                    c.title = 'İndirmek için işaretle';
                    c.style.cssText = 'margin:0 6px 0 0;vertical-align:middle;cursor:pointer;accent-color:var(--cu-vurgu);';
                    // UYAP satıra tıklanınca belgeyi açar / klasörü açıp kapatır: kutu tıklaması oraya gitmez.
                    ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'click', 'dblclick', 'touchstart', 'touchend', 'keydown']
                        .forEach(function(t) { c.addEventListener(t, durdur); });
                    c.addEventListener('change', function() {
                        const g = harita2(tv)[c.getAttribute('data-dugum')];
                        if (!g) return;
                        altEvraklar(g.x, g.ebeveyn).forEach(function(e) {
                            if (c.checked) secim.add(e.evrakId); else secim.delete(e.evrakId);
                        });
                        agaclariTara();
                        kartlariTara();               // "Seçili Belgeleri İndir (N)" hemen güncellensin
                        formSayisiniTazele(kart);
                    });
                    satir.insertBefore(c, satir.firstChild);
                }
                c.setAttribute('data-dugum', d.x.id);
                const evr = altEvraklar(d.x, d.ebeveyn);
                const n = evr.filter(function(e) { return secim.has(e.evrakId); }).length;
                c.disabled = !evr.length;
                c.checked = evr.length > 0 && n === evr.length;
                c.indeterminate = n > 0 && n < evr.length;
            });
        });
    }
    function harita2(tv) { const k = agacVerisi(tv); return k ? dugumHaritasi(k) : {}; }
    // İş Listesi bloğu belgeye iş bağlamak / işli belgeyi işaretlemek için aynı ağaç verisini kullanır.
    window.__uyapEvrakAgaci = { veri: agacVerisi, harita: dugumHaritasi, ozet: evrakOzeti, durdur: durdur };
    setInterval(function() { kartlariTara(); agaclariTara(); }, 700);

    // ── Sunucu ──
    async function api(yol, opts) {
        // Zaman sınırı: telefon ↔ bulut tünelinde yanıtsız kalan tek istek ilerlemeyi kalıcı donduruyordu (5/164).
        const ks = typeof AbortController === 'function' ? new AbortController() : null;
        const zs = ks ? setTimeout(function() { ks.abort(); }, 15000) : 0;
        let r;
        try { r = await window.fetch(EP + yol, Object.assign({ cache: 'no-store' }, ks ? { signal: ks.signal } : {}, opts || {})); }
        finally { clearTimeout(zs); }
        let j = null;
        try { j = await r.json(); } catch (e) {}
        if (!r.ok) throw new Error((j && j.error) || ('HTTP ' + r.status));
        return j;
    }
    function post(yol, govde) {
        return api(yol, { method: 'POST', headers: { 'Content-Type': 'application/json' },
                          body: JSON.stringify(govde || {}) });
    }
    async function baslat(dosyaId, ad, cikti, kapsam, evraklar) {
        panelAc();
        try {
            const j = await post('', { dosyaId: dosyaId, ad: ad, cikti: cikti, kapsam: kapsam,
                                       evraklar: kapsam === 'secili' ? evraklar : undefined });
            const v = j.indirme;
            isler[v.id] = isler[v.id] || { ad: ad || 'UYAP dosyası', zipIndirildi: false };
            isler[v.id].veri = v;
            render();
            izle(v.id);
        } catch (e) { panelMesaj('Başlatılamadı: ' + e.message); }
    }
    const izlenen = {};
    async function izle(id) {
        if (izlenen[id]) return;
        izlenen[id] = true;
        try {
            while (isler[id] && BITMIS.indexOf(isler[id].veri.durum) === -1) {
                await new Promise(function(r) { setTimeout(r, 1500); });
                if (!isler[id]) break;
                try { isler[id].veri = (await api('/' + id)).indirme; } catch (e) { continue; }
                render();
            }
            const is = isler[id];
            if (is && is.veri.dosya_hazir && !is.zipIndirildi) dosyaIndir(id);
        } finally { delete izlenen[id]; }
    }
    // Dosya fetch ile çekilip blob olarak indirilir. Düz <a href> indirmesini telefon tarayıcısı
    // (Android Chrome) Service Worker'ı atlayıp doğrudan cokluuyap.com'a sorar: tünele girmez,
    // sunucunun 404 metni ".txt" olarak iner. fetch SW'den geçer; blob indirmesi her yerde çalışır.
    function cdAdi(cd) {
        let m = /filename\*=UTF-8''([^;]+)/i.exec(cd || '');
        if (m) { try { return decodeURIComponent(m[1].trim()); } catch (e) {} }
        m = /filename="?([^";]+)"?/i.exec(cd || '');
        return m ? m[1].trim() : '';
    }
    async function dosyaIndir(id) {
        const is = isler[id];
        if (!is) return;
        is.zipIndirildi = true;
        try {
            const r = await fetch(EP + '/' + id + '/dosya', { credentials: 'same-origin' });
            if (!r.ok) throw new Error('HTTP ' + r.status);
            const ad = cdAdi(r.headers.get('content-disposition'))
                || (is.ad || 'UYAP evrak') + (is.veri && is.veri.cikti === 'tek' ? '.pdf' : '.zip');
            const u = URL.createObjectURL(await r.blob());
            const a = document.createElement('a');
            a.href = u;
            a.download = ad;
            a.setAttribute('data-uyap-internal', '1');   // E-İmza indirme yakalayıcısının dışında
            document.body.appendChild(a); a.click(); a.remove();
            setTimeout(function() { URL.revokeObjectURL(u); }, 60000);
        } catch (e) {
            is.zipIndirildi = false;
            panelMesaj('Dosya indirilemedi: ' + e.message + ' — "İndir" düğmesiyle tekrar deneyin.');
        }
    }

    // ── Panel (sağ üst; E-İmza sağ altta — çakışmaz) ──
    function el(tag, css, metin) {
        const e = document.createElement(tag);
        if (css) e.style.cssText = css;
        if (metin != null) e.textContent = metin;
        return e;
    }
    function dugme(metin, renk, fn) {
        const b = el('button', 'margin:6px 6px 0 0;padding:5px 10px;border:0;border-radius:7px;cursor:pointer;font-size:12px;font-weight:600;color:' + (/--cu-vurgu/.test(renk) ? 'var(--cu-vurgu-uzeri,#fff)' : '#fff') + ';background:' + renk + ';', metin);
        b.addEventListener('click', function(ev) { ev.stopPropagation(); fn(); });
        return b;
    }
    let panel, govde, mesajEl, form;
    function panelKur() {
        if (panel || !document.body) return;
        panel = el('div', 'position:fixed;top:72px;right:24px;z-index:2147483646;width:320px;max-height:70vh;overflow:auto;padding:12px;border-radius:12px;background:rgba(var(--cu-panel-rgb),0.95);border:1px solid rgba(255,255,255,0.15);box-shadow:0 10px 30px rgba(0,0,0,0.35);color:#f3f4f6;font:12px/1.45 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;display:none;');
        panel.id = 'uyap-tumevrak-panel';
        // UYAP'ın label/span/a renk kuralları paneldeki yazıyı koyu yapıyordu (koyu zeminde
        // okunmuyordu): panel içinde renk panelden miras alınır; satır içi renkler yine geçerli.
        const stil = document.createElement('style');
        stil.textContent =
            '#uyap-tumevrak-panel{color:#f3f4f6!important}' +
            '#uyap-tumevrak-panel div,#uyap-tumevrak-panel span,#uyap-tumevrak-panel label,' +
            '#uyap-tumevrak-panel a,#uyap-tumevrak-panel p{color:inherit;font-family:inherit;font-size:inherit;margin-bottom:0}' +
            '#uyap-tumevrak-panel input{accent-color:var(--cu-vurgu);margin:0}';
        document.head.appendChild(stil);
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;');
        ust.appendChild(el('div', 'font-size:13px;font-weight:700;', 'Tüm Evrakı İndir'));
        const kapat = el('span', 'font-size:16px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        kapat.title = 'Gizle (indirme sürer)';
        kapat.addEventListener('click', function() { panel.style.display = 'none'; });
        ust.appendChild(kapat);
        mesajEl = el('div', 'margin-top:6px;color:#fca5a5;');
        form = el('div', '');
        govde = el('div', '');
        panel.appendChild(ust); panel.appendChild(mesajEl); panel.appendChild(form); panel.appendChild(govde);
        document.body.appendChild(panel);
    }

    // ── Seçenek formu: Çıktı (tek PDF | tek PDF + parçalar) × Kapsam (tüm | seçtiğim) ──
    function tercih(ad, varsayilan) {
        try { return localStorage.getItem('uyapTumEvrak.' + ad) || varsayilan; } catch (e) { return varsayilan; }
    }
    function tercihYaz(ad, deger) { try { localStorage.setItem('uyapTumEvrak.' + ad, deger); } catch (e) {} }
    function radyoGrubu(baslik, ad, secenekler, secili) {
        const kutu = el('div', 'margin-top:8px;');
        kutu.appendChild(el('div', 'font-weight:600;color:#cbd5e1;margin-bottom:2px;', baslik));
        secenekler.forEach(function(s) {
            const etiket = el('label', 'display:flex;align-items:center;gap:6px;margin-top:3px;cursor:pointer;');
            const r = document.createElement('input');
            r.type = 'radio'; r.name = 'uyapTumEvrak_' + ad; r.value = s[0]; r.checked = s[0] === secili;
            etiket.appendChild(r); etiket.appendChild(document.createTextNode(s[1]));
            kutu.appendChild(etiket);
        });
        return kutu;
    }
    function secilenDeger(ad) {
        const r = form.querySelector('input[name="uyapTumEvrak_' + ad + '"]:checked');
        return r ? r.value : null;
    }
    // Form kapsamı (tümü / işaretliler) ve çıktıyı sorar. Kartta işaret varsa "işaretliler" seçili
    // gelir; işaret sayısı form açıkken canlı güncellenir.
    let formKarti = null, kapsamEl = null, kapsamRadyo = null;
    function formSayisiniTazele(kart) {
        if (!kapsamEl || kart !== formKarti) return;
        const n = kartSecimi(kart).size;
        kapsamEl.textContent = n ? 'İşaretli belgeler (' + n + ')' : 'İşaretli belgeler (Evrak sekmesinde belge işaretleyin)';
        if (kapsamRadyo) {
            kapsamRadyo.disabled = !n;
            if (!n && kapsamRadyo.checked) {
                const tum = form.querySelector('input[name="uyapTumEvrak_kapsam"][value="tum"]');
                if (tum) tum.checked = true;
            }
        }
    }
    function secenekAc(kart, dosyaId, ad, kapsam) {
        panelAc();
        form.innerHTML = '';
        const kutu = el('div', 'margin-top:8px;padding:8px;border-radius:8px;background:rgba(255,255,255,0.06);');
        kutu.appendChild(el('div', 'font-weight:600;word-break:break-word;', ad || 'UYAP dosyası'));
        const kapsamGrubu = radyoGrubu('Belgeler', 'kapsam', [['tum', 'Tüm belgeler'], ['secili', 'İşaretli belgeler']], kapsam);
        form.appendChild(kutu);
        kutu.appendChild(kapsamGrubu);
        kapsamRadyo = kapsamGrubu.querySelector('input[value="secili"]');
        kapsamEl = kapsamRadyo.nextSibling;               // etiket metni (sayı canlı güncellenir)
        formKarti = kart;
        formSayisiniTazele(kart);
        kutu.appendChild(radyoGrubu('Çıktı', 'cikti', [
            ['tek', 'Tek PDF'],
            ['ikisi', 'Tek PDF + ayrı ayrı PDF\'ler (ZIP)'],
        ], tercih('cikti', 'tek')));
        kutu.appendChild(dugme('İndir', 'var(--cu-vurgu)', function() {
            const cikti = secilenDeger('cikti');
            const kapsam = secilenDeger('kapsam') || 'tum';
            const evraklar = kapsam === 'secili' ? secilenEvraklar(kart) : null;
            if (kapsam === 'secili' && !evraklar.length) { panelMesaj('İşaretli belge yok.'); return; }
            tercihYaz('cikti', cikti);
            form.innerHTML = ''; formKarti = null;
            baslat(dosyaId, ad, cikti, kapsam, evraklar);
        }));
        kutu.appendChild(dugme('Vazgeç', 'rgba(255,255,255,0.15)', function() {
            form.innerHTML = ''; formKarti = null;
            if (!Object.keys(isler).length) panel.style.display = 'none';
        }));
    }

    function panelAc() { panelKur(); if (panel) { panel.style.display = 'block'; mesajEl.textContent = ''; } }
    function panelMesaj(m) { panelAc(); if (mesajEl) mesajEl.textContent = m; }

    const satirlar = {};   // id -> {kok, durum, cubukIc, eylem, captcha, anahtar}
    function render() {
        panelKur();
        if (!panel) return;
        Object.keys(isler).forEach(function(id) {
            const is = isler[id], v = is.veri;
            let s = satirlar[id];
            if (!s) {
                s = satirlar[id] = { kok: el('div', 'margin-top:10px;padding-top:8px;border-top:1px solid rgba(255,255,255,0.1);') };
                s.kok.appendChild(el('div', 'font-weight:600;word-break:break-word;', is.ad));
                s.durum = el('div', 'margin-top:4px;'); s.kok.appendChild(s.durum);
                const cubuk = el('div', 'margin-top:6px;height:6px;border-radius:3px;background:rgba(255,255,255,0.12);overflow:hidden;');
                s.cubukIc = el('div', 'height:100%;width:0;background:var(--cu-vurgu);transition:width .3s;');
                cubuk.appendChild(s.cubukIc); s.kok.appendChild(cubuk); s.cubuk = cubuk;
                s.captcha = el('div', ''); s.kok.appendChild(s.captcha);
                s.eylem = el('div', ''); s.kok.appendChild(s.eylem);
                govde.insertBefore(s.kok, govde.firstChild);
            }
            const d = v.durum;
            if (d === 'captcha') panel.style.display = 'block';    // doğrulama kullanıcıdan saklanmaz
            s.durum.textContent = v.mesaj || '';
            s.durum.style.color = d === 'hata' ? '#fca5a5' : d === 'captcha' ? '#fcd34d' : d === 'bitti' ? '#6ee7b7' : '#e5e7eb';
            s.cubuk.style.display = (d === 'calisiyor' || d === 'captcha') && v.toplam ? 'block' : 'none';
            s.cubukIc.style.width = (v.toplam ? Math.round(100 * v.i / v.toplam) : 0) + '%';

            const anahtar = d + '|' + (v.dosya_hazir ? 1 : 0) + '|' + (v.captcha ? v.captcha.n : '');
            if (anahtar === s.anahtar) return;
            s.anahtar = anahtar;
            s.eylem.innerHTML = '';
            if (d !== 'captcha') s.captcha.innerHTML = '';
            if (d === 'calisiyor' || d === 'sirada') {
                s.eylem.appendChild(dugme('Durdur', '#b91c1c', function() { iptal(id); }));
            } else if (d === 'captcha') {
                captchaGoster(id, s.captcha);
                s.eylem.appendChild(dugme('Tekrar dene', 'rgba(255,255,255,0.15)', function() { captchaCevap(id, null); }));
                s.eylem.appendChild(dugme('Durdur', '#b91c1c', function() { iptal(id); }));
            } else {
                if (v.uyarilar && v.uyarilar.length) {
                    s.eylem.appendChild(el('div', 'margin-top:6px;color:#fcd34d;max-height:80px;overflow:auto;',
                        v.uyarilar.length + ' uyarı: ' + v.uyarilar.join(' • ')));
                }
                if (v.dosya_hazir) s.eylem.appendChild(dugme(v.cikti === 'tek' ? 'PDF\'i indir' : 'ZIP\'i indir',
                    'var(--cu-vurgu)', function() { dosyaIndir(id); }));
                s.eylem.appendChild(dugme('Listeden kaldır', 'rgba(255,255,255,0.15)', function() {
                    s.kok.remove(); delete satirlar[id]; delete isler[id];
                    if (!Object.keys(isler).length) panel.style.display = 'none';
                }));
            }
        });
    }
    async function iptal(id) {
        try { isler[id].veri = (await post('/' + id + '/iptal')).indirme; } catch (e) {}
        render();
    }

    // ── Güvenlik doğrulaması (UYAP'ın kendi Turnstile site anahtarıyla, kullanıcı çözer) ──
    function tsYukle() {
        if (window.turnstile) return Promise.resolve(window.turnstile);
        if (tsYukleniyor) return tsYukleniyor;
        tsYukleniyor = new Promise(function(res, rej) {
            const sc = document.createElement('script');
            sc.src = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
            sc.async = true;
            sc.onload = function() { res(window.turnstile); };
            sc.onerror = function() { tsYukleniyor = null; rej(new Error('doğrulama betiği yüklenemedi')); };
            document.head.appendChild(sc);
        });
        return tsYukleniyor;
    }
    async function captchaGoster(id, kutu) {
        kutu.innerHTML = '';
        const c = isler[id] && isler[id].veri.captcha;
        if (!c) return;
        kutu.appendChild(el('div', 'margin-top:6px;color:#fcd34d;', c.mesaj || 'UYAP güvenlik doğrulaması istiyor.'));
        const yer = el('div', 'margin-top:6px;min-height:65px;');
        kutu.appendChild(yer);
        const hata = function(m) {
            kutu.appendChild(el('div', 'margin-top:6px;color:#fca5a5;', m +
                ' Birkaç dakika bekleyip "Tekrar dene"ye basabilir ya da indirmeyi durdurabilirsiniz.'));
        };
        try {
            const ts = await tsYukle();
            ts.render(yer, {
                sitekey: c.site_key,
                callback: function(jeton) { captchaCevap(id, jeton); },
                'error-callback': function(kod) { hata('Doğrulama bu adreste açılamadı (kod: ' + kod + ').'); return true; },
            });
        } catch (e) { hata('Doğrulama gösterilemedi (' + e.message + ').'); }
    }
    async function captchaCevap(id, jeton) {
        try { isler[id].veri = (await post('/' + id + '/captcha', { token: jeton })).indirme; } catch (e) {}
        render();
        izle(id);
    }
})();

// ── GEREKÇELİ KARARLAR (GEÇİCİ) ──────────────────────────────────────────────────────
// Sol altta "Gerekçeli Kararlar": ofis açık + kapalı tüm hukuk dosyalarını listeler, kullanıcı
// mahkeme türü / dosya işaretler, ofis her dosyada gerekçeli kararı arar ve UDF + PDF indirir
// (bkz. uyap_core/gerekceli_karar.py). UYAP'a istekler ofiste sabit, yavaş hızla ve sırayla gider;
// doğrulama istenirse kullanıcı çözer. Bitince ZIP buraya iner.
(function() {
    const EP = '/__uyap_agent__/gerekceli';
    const SON = ['bitti', 'hata', 'iptal'];
    let panel, govde, tarama = null, liste = null, izleniyor = false, zipIndi = {}, tsYukleniyor = null;

    function el(tag, css, metin) {
        const e = document.createElement(tag);
        if (css) e.style.cssText = css;
        if (metin != null) e.textContent = metin;
        return e;
    }
    function dugme(metin, renk, fn) {
        const b = el('button', 'margin:8px 6px 0 0;padding:5px 10px;border:0;border-radius:7px;cursor:pointer;font-size:12px;font-weight:600;color:' + (/--cu-vurgu/.test(renk) ? 'var(--cu-vurgu-uzeri,#fff)' : '#fff') + ';background:' + renk + ';', metin);
        b.type = 'button';
        b.addEventListener('click', function(ev) { ev.stopPropagation(); fn(b); });
        return b;
    }
    async function api(yol, opts) {
        const r = await fetch(EP + yol, Object.assign({ cache: 'no-store', credentials: 'same-origin' }, opts || {}));
        let j = null;
        try { j = await r.json(); } catch (e) {}
        if (!r.ok) throw new Error((j && j.error) || ('HTTP ' + r.status));
        return j;
    }
    function post(yol, govde) {
        return api(yol, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(govde || {}) });
    }
    function sure(sn) {
        if (sn == null) return '';
        if (sn < 90) return Math.max(1, Math.round(sn)) + ' sn';
        const dk = Math.round(sn / 60);
        return dk < 90 ? dk + ' dk' : (Math.floor(dk / 60) + ' sa ' + (dk % 60) + ' dk');
    }

    function panelKur() {
        if (panel || !document.body) return;
        // Sağ üstte, Çoklu UYAP çekmecesinin (Araçlar → Gerekçeli Kararlar) açıldığı tarafta.
        panel = el('div', 'position:fixed;right:40px;top:72px;z-index:2147483646;width:420px;max-width:calc(100vw - 32px);max-height:75vh;overflow:auto;padding:12px;border-radius:12px;background:rgba(var(--cu-panel-rgb),0.96);border:1px solid rgba(255,255,255,0.15);box-shadow:0 10px 30px rgba(0,0,0,0.35);color:#f3f4f6;font:12px/1.45 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;display:none;');
        panel.id = 'uyap-gerekceli-panel';
        const stil = document.createElement('style');
        stil.textContent =
            '#uyap-gerekceli-panel{color:#f3f4f6!important}' +
            '#uyap-gerekceli-panel div,#uyap-gerekceli-panel span,#uyap-gerekceli-panel label,' +
            '#uyap-gerekceli-panel p{color:inherit;font-family:inherit;font-size:inherit;margin-bottom:0}' +
            '#uyap-gerekceli-panel input{accent-color:var(--cu-vurgu);margin:0}';
        document.head.appendChild(stil);
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;');
        ust.appendChild(el('div', 'font-size:13px;font-weight:700;', 'Gerekçeli Kararlar'));
        const kapat = el('span', 'font-size:16px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        kapat.title = 'Gizle (tarama sürer)';
        kapat.addEventListener('click', function() { panel.style.display = 'none'; });
        ust.appendChild(kapat);
        govde = el('div', '');
        panel.appendChild(ust); panel.appendChild(govde);
        document.body.appendChild(panel);
    }
    async function ac() {
        panelKur();
        panel.style.display = 'block';
        try { tarama = (await api('')).tarama; } catch (e) { tarama = null; }
        ciz();
        izle();
    }

    const BEKLEYEN = ['secim', 'karar_secim'];
    function ciz() {
        if (!govde) return;
        govde.innerHTML = '';
        const t = tarama;
        if (!t || t.durum === 'iptal' && ['liste', 'secim', 'karar_secim'].indexOf(t.asama) !== -1) return girisCiz();
        if (t.durum === 'secim') return secimCiz();
        if (t.durum === 'karar_secim') return kararSecimCiz();
        if (t.durum === 'captcha') panel.style.display = 'block';   // doğrulama saklanmaz
        const renk = t.durum === 'hata' ? '#fca5a5' : t.durum === 'captcha' ? '#fcd34d' : t.durum === 'bitti' ? '#6ee7b7' : '#e5e7eb';
        govde.appendChild(el('div', 'margin-top:8px;color:' + renk + ';word-break:break-word;', t.mesaj || ''));
        if (t.asama === 'kontrol' || t.asama === 'indirme') {
            const cubuk = el('div', 'margin-top:8px;height:6px;border-radius:3px;background:rgba(255,255,255,0.12);overflow:hidden;');
            cubuk.appendChild(el('div', 'height:100%;background:var(--cu-vurgu);width:' + (t.toplam ? Math.round(100 * t.i / t.toplam) : 0) + '%;'));
            govde.appendChild(cubuk);
            let ozet = t.asama === 'kontrol'
                ? 'Kontrol: ' + t.i + '/' + t.toplam + ' dosya · ' + t.karar_dosyasi + ' dosyada ' + t.bulunan + ' karar bulundu'
                : 'İndirme: ' + t.i + '/' + t.toplam + ' karar (' + t.yeni + ' yeni)';
            if (t.asama === 'kontrol' && t.atlanan) ozet += ' · ' + t.atlanan + ' dosya yakın zamanda kontrol edilmişti';
            if (t.kalan_sn != null) ozet += ' · kalan ~' + sure(t.kalan_sn);
            if (t.mola_kalan_sn != null) ozet += ' · mola ' + t.mola_kalan_sn + ' sn';
            govde.appendChild(el('div', 'margin-top:4px;color:#cbd5e1;', ozet));
        }
        if (t.durum === 'captcha') captchaGoster(t);
        if (SON.indexOf(t.durum) === -1) {
            govde.appendChild(dugme('Durdur', '#b91c1c', async function() {
                try { tarama = (await post('/' + t.id + '/iptal')).tarama; } catch (e) {}
                ciz();
            }));
            return;
        }
        if (t.uyarilar && t.uyarilar.length) {
            govde.appendChild(el('div', 'margin-top:6px;color:#fcd34d;max-height:90px;overflow:auto;',
                t.uyarilar.length + ' uyarı: ' + t.uyarilar.join(' • ')));
        }
        if (t.dosya_hazir) govde.appendChild(dugme('ZIP\'i indir', 'var(--cu-vurgu)', function() { zipIndir(t.id); }));
        if (t.excel_hazir) govde.appendChild(dugme('Excel listesini indir', '#1d6f42', function() { zipIndir(t.id, 'excel'); }));
        govde.appendChild(dugme('Yeni tarama', 'rgba(255,255,255,0.15)', function() { tarama = null; ciz(); }));
    }

    function girisCiz() {
        govde.appendChild(el('div', 'margin-top:8px;color:#cbd5e1;',
            'Açık ve kapalı tüm hukuk dosyalarınız (tüm mahkeme türleri) UYAP\'taki durumlarıyla ' +
            '(Açık, Karara Çıkmış, İstinafta, Kapalı…) listelenir. İşaretlediğiniz dosyalarda önce yalnız ' +
            'gerekçeli karar olup olmadığına bakılır; sonra indirmek istediklerinizi seçersiniz (UDF + PDF). ' +
            'Kontrol edilen dosyaların tarafları ile birlikte Excel listesi de hazırlanır. UYAP\'ı yormamak için ' +
            'istekler arasında birkaç saniye beklenir ve belli aralıklarla mola verilir; uzun sürebilir. ' +
            'Bu sırada UYAP\'ı kullanmaya devam edebilirsiniz — sizin işlemleriniz önce gider.'));
        govde.appendChild(dugme('Hukuk dosyalarını listele', '#0f766e', async function(b) {
            b.disabled = true;
            try { tarama = (await post('')).tarama; liste = null; } catch (e) {
                govde.appendChild(el('div', 'margin-top:6px;color:#fca5a5;', 'Başlatılamadı: ' + e.message));
                b.disabled = false;
                return;
            }
            ciz(); izle();
        }));
    }

    // UYAP'ın dosya durumu etiketi: Açık (derdest) sarı, Kapalı yeşil, diğerleri (Karara Çıkmış,
    // İstinafta, Yargıtayda…) mavi — gerekçeli karar çoğunlukla bunlarda.
    function etiket(durum) {
        const renk = durum === 'Açık' ? 'background:rgba(245,158,11,0.18);color:#fcd34d;'
            : durum === 'Kapalı' ? 'background:rgba(16,185,129,0.18);color:#6ee7b7;'
            : 'background:rgba(59,130,246,0.22);color:#93c5fd;';
        return el('span', 'flex:none;padding:0 6px;border-radius:999px;font-size:10px;font-weight:700;white-space:nowrap;' + renk, durum || '?');
    }
    function sayim(dosyalar) {
        const s = {};
        dosyalar.forEach(function(d) { s[d.durum] = (s[d.durum] || 0) + 1; });
        return s;
    }
    function hepsiDugmesi(kutular, say) {
        const a = el('span', 'margin-left:8px;color:#93c5fd;cursor:pointer;font-weight:400;', 'tümünü kaldır');
        a.addEventListener('click', function() {
            const gorunen = kutular.filter(function(x) { return x.satir.style.display !== 'none'; });
            const isaretle = gorunen.some(function(x) { return !x.k.checked; });
            gorunen.forEach(function(x) { x.k.checked = isaretle; });
            say();
        });
        return a;
    }

    async function secimCiz() {
        const t = tarama;
        govde.appendChild(el('div', 'margin-top:8px;color:#6ee7b7;', t.mesaj || ''));
        const yer = el('div', 'margin-top:6px;', 'Liste yükleniyor…');
        govde.appendChild(yer);
        if (!liste || liste.id !== t.id) {
            try { liste = Object.assign({ id: t.id }, await api('/' + t.id + '/liste')); }
            catch (e) { yer.textContent = 'Liste alınamadı: ' + e.message; return; }
        }
        yer.textContent = '';
        // Yalnız "Açık" (derdest) dosyalar varsayılan olarak taramaya girmez; Karara Çıkmış,
        // İstinafta, Kapalı… girer. Durum süzgeci UYAP'tan gelen durumlara göre kurulur.
        const sayilar = sayim(liste.dosyalar);
        const adlar = Object.keys(sayilar).sort(function(a, b) {
            const sira = function(x) { return x === 'Açık' ? 2 : x === 'Kapalı' ? 1 : 0; };
            return sira(a) - sira(b) || sayilar[b] - sayilar[a];
        });
        const durumlar = {};
        adlar.forEach(function(ad) { durumlar[ad] = ad !== 'Açık' || adlar.length === 1; });
        const kutular = [];               // {k: checkbox, d: dosya, satir}
        const gruplar = [];               // {grup, uyeler}
        const sayac = el('div', 'margin-top:8px;font-weight:600;');
        function secili() { return kutular.filter(function(x) { return x.k.checked && durumlar[x.d.durum]; }); }
        function say() {
            const n = secili().length;
            // Dosya başına en az 3 istek (evrak listesi + taraflar); sabit aralık ve molalarla kabaca.
            const sn = n * 3 * (t.aralik_sn || 5) + Math.floor(n / 25) * 60;
            sayac.textContent = n + ' dosya kontrol edilecek' + (n ? ' · en az ~' + sure(sn) : '');
            return n;
        }
        function gorunurluk() {
            kutular.forEach(function(x) { x.satir.style.display = durumlar[x.d.durum] ? 'flex' : 'none'; });
            gruplar.forEach(function(g) {
                g.grup.style.display = g.uyeler.some(function(x) { return durumlar[x.d.durum]; }) ? '' : 'none';
            });
            say();
        }
        yer.appendChild(el('div', 'font-weight:600;color:#cbd5e1;', 'Dosya durumu'));
        const suz = el('div', 'margin-top:2px;display:flex;flex-wrap:wrap;gap:4px 12px;');
        adlar.forEach(function(ad) {
            const l = el('label', 'display:flex;align-items:center;gap:5px;cursor:pointer;');
            const c = document.createElement('input'); c.type = 'checkbox'; c.checked = durumlar[ad];
            c.addEventListener('change', function() { durumlar[ad] = c.checked; gorunurluk(); });
            l.appendChild(c); l.appendChild(etiket(ad));
            l.appendChild(document.createTextNode(String(sayilar[ad])));
            suz.appendChild(l);
        });
        yer.appendChild(suz);
        if (sayilar['Açık'] && adlar.length > 1) {
            yer.appendChild(el('div', 'margin-top:2px;color:#9ca3af;', '"Açık" (derdest) dosyalar varsayılan olarak kontrol edilmez; gerekirse işaretleyin.'));
        }
        const kap = el('div', 'margin-top:6px;max-height:45vh;overflow:auto;padding-right:4px;');
        liste.turler.forEach(function(tur) {
            const dosyalar = liste.dosyalar.filter(function(d) { return d.tur_kod === tur.kod; });
            if (!dosyalar.length) return;
            const grup = el('div', 'margin-top:6px;');
            const bas = el('label', 'display:flex;align-items:center;gap:6px;font-weight:600;cursor:pointer;');
            const tc = document.createElement('input'); tc.type = 'checkbox'; tc.checked = true;
            bas.appendChild(tc);
            bas.appendChild(document.createTextNode(tur.ad));
            const ts = sayim(dosyalar);
            bas.appendChild(el('span', 'color:#9ca3af;font-weight:400;', '· ' + Object.keys(ts).map(function(k) { return ts[k] + ' ' + k; }).join(', ')));
            const ac_ = el('span', 'margin-left:auto;color:#9ca3af;font-weight:400;', 'göster');
            bas.appendChild(ac_);
            const ic = el('div', 'display:none;margin-left:20px;');
            ac_.addEventListener('click', function(ev) {
                ev.preventDefault();
                ic.style.display = ic.style.display === 'none' ? 'block' : 'none';
                ac_.textContent = ic.style.display === 'none' ? 'göster' : 'gizle';
            });
            const uyeler = [];
            dosyalar.forEach(function(d) {
                const satir = el('label', 'display:flex;align-items:center;gap:6px;margin-top:2px;cursor:pointer;');
                const k = document.createElement('input'); k.type = 'checkbox'; k.checked = true;
                k.addEventListener('change', say);
                satir.appendChild(k);
                satir.appendChild(etiket(d.durum));
                satir.appendChild(document.createTextNode(d.birim + ' ' + d.no));
                ic.appendChild(satir);
                const x = { k: k, d: d, satir: satir };
                kutular.push(x); uyeler.push(x);
            });
            // Tür kutusu yalnız görünen (durumu seçili) dosyaları işaretler/kaldırır.
            tc.addEventListener('change', function() {
                uyeler.forEach(function(x) { if (durumlar[x.d.durum]) x.k.checked = tc.checked; });
                say();
            });
            grup.appendChild(bas); grup.appendChild(ic);
            kap.appendChild(grup);
            gruplar.push({ grup: grup, uyeler: uyeler });
        });
        yer.appendChild(kap);
        yer.appendChild(sayac);
        gorunurluk();
        yer.appendChild(dugme('Gerekçeli kararları kontrol et', 'var(--cu-vurgu)', async function(b) {
            const secilen = secili().map(function(x) { return x.d.id; });
            if (!secilen.length) { sayac.textContent = 'İşaretli dosya yok.'; return; }
            b.disabled = true;
            try { tarama = (await post('/' + t.id + '/baslat', { dosyalar: secilen })).tarama; }
            catch (e) { sayac.textContent = 'Başlatılamadı: ' + e.message; b.disabled = false; return; }
            ciz(); izle();
        }));
        yer.appendChild(dugme('Vazgeç', 'rgba(255,255,255,0.15)', async function() {
            try { await post('/' + t.id + '/iptal'); } catch (e) {}
            tarama = null; liste = null; ciz();
        }));
    }

    // Kontrol bitti: kararı bulunan dosyalar listelenir, kullanıcı indirileceklerini seçer.
    async function kararSecimCiz() {
        const t = tarama;
        govde.appendChild(el('div', 'margin-top:8px;color:#6ee7b7;word-break:break-word;', t.mesaj || ''));
        const yer = el('div', 'margin-top:6px;', 'Sonuçlar yükleniyor…');
        govde.appendChild(yer);
        let s;
        try { s = await api('/' + t.id + '/sonuclar'); }
        catch (e) { yer.textContent = 'Sonuçlar alınamadı: ' + e.message; return; }
        yer.textContent = '';
        const bilgi = [];
        if (s.karar_yok) bilgi.push(s.karar_yok + ' dosyada gerekçeli karar yok');
        if (s.kontrol_edilemedi) bilgi.push(s.kontrol_edilemedi + ' dosya kontrol edilemedi');
        if (bilgi.length) yer.appendChild(el('div', 'color:#9ca3af;', bilgi.join(' · ') + ' (Excel listesinde yer alır).'));
        const kutular = [];
        const sayac = el('div', 'margin-top:8px;font-weight:600;');
        function secili() { return kutular.filter(function(x) { return x.k.checked; }); }
        function say() {
            const sec = secili();
            const karar = sec.reduce(function(n, x) {
                return n + x.d.kararlar.filter(function(k) { return !k.inmis; }).length;
            }, 0);
            const sn = karar * 2 * (t.aralik_sn || 5) + Math.floor(karar / 25) * 60;
            sayac.textContent = sec.length + ' dosya seçili · ' + karar + ' karar indirilecek' + (karar ? ' · ~' + sure(sn) : '');
        }
        if (s.dosyalar.length) {
            const bas = el('div', 'margin-top:6px;font-weight:600;color:#cbd5e1;', 'Gerekçeli kararı olan dosyalar (' + s.dosyalar.length + ')');
            bas.appendChild(hepsiDugmesi(kutular, say));
            yer.appendChild(bas);
            const kap = el('div', 'margin-top:4px;max-height:45vh;overflow:auto;padding-right:4px;');
            s.dosyalar.forEach(function(d) {
                const satir = el('label', 'display:flex;align-items:center;gap:6px;margin-top:3px;cursor:pointer;');
                const k = document.createElement('input'); k.type = 'checkbox'; k.checked = true;
                k.addEventListener('change', say);
                satir.appendChild(k);
                satir.appendChild(etiket(d.durum));
                const inmis = d.kararlar.every(function(x) { return x.inmis; });
                const metin = el('span', 'word-break:break-word;', d.birim + ' ' + d.no + ' · ' + d.kararlar.length + ' karar');
                satir.appendChild(metin);
                if (inmis) satir.appendChild(el('span', 'color:#9ca3af;', '(ofiste zaten var)'));
                satir.title = d.tur + ' · ' + d.kararlar.map(function(x) { return x.ad; }).join(', ');
                kap.appendChild(satir);
                kutular.push({ k: k, d: d, satir: satir });
            });
            yer.appendChild(kap);
            yer.appendChild(sayac);
            say();
        } else {
            yer.appendChild(el('div', 'margin-top:6px;', 'Kontrol edilen dosyaların hiçbirinde gerekçeli karar bulunamadı.'));
        }
        async function gonder(b, ids) {
            b.disabled = true;
            try { tarama = (await post('/' + t.id + '/indir', { dosyalar: ids })).tarama; }
            catch (e) { sayac.textContent = 'Başlatılamadı: ' + e.message; b.disabled = false; return; }
            ciz(); izle();
        }
        if (s.dosyalar.length) {
            yer.appendChild(dugme('Seçilenleri indir', 'var(--cu-vurgu)', function(b) {
                const ids = secili().map(function(x) { return x.d.id; });
                if (!ids.length) { sayac.textContent = 'İşaretli dosya yok.'; return; }
                gonder(b, ids);
            }));
        }
        yer.appendChild(dugme('İndirmeden bitir (yalnız Excel)', '#1d6f42', function(b) { gonder(b, []); }));
    }

    async function izle() {
        if (izleniyor) return;
        izleniyor = true;
        try {
            while (tarama && BEKLEYEN.indexOf(tarama.durum) === -1 && SON.indexOf(tarama.durum) === -1) {
                await new Promise(function(r) { setTimeout(r, 2000); });
                const onceki = tarama;
                try { tarama = (await api('/' + onceki.id)).tarama; } catch (e) { continue; }
                // Doğrulama kutusu her yoklamada yeniden çizilmesin.
                if (!(onceki.durum === 'captcha' && tarama.durum === 'captcha')) ciz();
            }
            if (tarama && tarama.dosya_hazir && !zipIndi[tarama.id]) zipIndir(tarama.id);
        } finally { izleniyor = false; }
    }

    // fetch + blob: telefonda düz bağlantı Service Worker'ı atlayıp tünele girmiyor.
    async function zipIndir(id, ne) {
        if (!ne) zipIndi[id] = true;
        try {
            const r = await fetch(EP + '/' + id + '/' + (ne || 'dosya'), { credentials: 'same-origin' });
            if (!r.ok) throw new Error('HTTP ' + r.status);
            const cd = r.headers.get('content-disposition') || '';
            const m = /filename\*=UTF-8''([^;]+)/i.exec(cd);
            const ad = m ? decodeURIComponent(m[1]) : (ne === 'excel' ? 'Hukuk Dosyalari.xlsx' : 'Gerekceli Kararlar.zip');
            const u = URL.createObjectURL(await r.blob());
            const a = document.createElement('a');
            a.href = u; a.download = ad;
            a.setAttribute('data-uyap-internal', '1');
            document.body.appendChild(a); a.click(); a.remove();
            setTimeout(function() { URL.revokeObjectURL(u); }, 60000);
        } catch (e) {
            if (!ne) zipIndi[id] = false;
            if (govde) govde.appendChild(el('div', 'margin-top:6px;color:#fca5a5;', (ne === 'excel' ? 'Excel' : 'ZIP') + ' indirilemedi: ' + e.message));
        }
    }

    // ── Güvenlik doğrulaması: UYAP'ın kendi Turnstile site anahtarıyla, kullanıcı çözer ──
    function tsYukle() {
        if (window.turnstile) return Promise.resolve(window.turnstile);
        if (tsYukleniyor) return tsYukleniyor;
        tsYukleniyor = new Promise(function(res, rej) {
            const sc = document.createElement('script');
            sc.src = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
            sc.async = true;
            sc.onload = function() { res(window.turnstile); };
            sc.onerror = function() { tsYukleniyor = null; rej(new Error('doğrulama betiği yüklenemedi')); };
            document.head.appendChild(sc);
        });
        return tsYukleniyor;
    }
    async function captchaGoster(t) {
        const c = t.captcha;
        if (!c) return;
        const kutu = el('div', 'margin-top:6px;');
        kutu.appendChild(el('div', 'color:#fcd34d;', c.mesaj || 'UYAP güvenlik doğrulaması istiyor.'));
        const yer = el('div', 'margin-top:6px;min-height:65px;');
        kutu.appendChild(yer);
        govde.appendChild(kutu);
        const cevap = async function(jeton) {
            try { tarama = (await post('/' + t.id + '/captcha', { token: jeton })).tarama; } catch (e) {}
            ciz(); izle();
        };
        govde.appendChild(dugme('Tekrar dene', 'rgba(255,255,255,0.15)', function() { cevap(null); }));
        try {
            const ts = await tsYukle();
            ts.render(yer, {
                sitekey: c.site_key,
                callback: cevap,
                'error-callback': function(kod) {
                    kutu.appendChild(el('div', 'margin-top:6px;color:#fca5a5;', 'Doğrulama açılamadı (kod: ' + kod + '). Biraz bekleyip "Tekrar dene"ye basın.'));
                    return true;
                },
            });
        } catch (e) {
            kutu.appendChild(el('div', 'margin-top:6px;color:#fca5a5;', 'Doğrulama gösterilemedi (' + e.message + ').'));
        }
    }

    // 2026-11-11: sol alttaki sabit düğme kaldırıldı; panel Çoklu UYAP çekmecesinden (Araçlar) açılır.
    // Süren tarama varsa izlemeye devam edilir, durumu çekmecede görünür.
    window.__uyapGerekceli = {
        ac: ac,
        durum: function() {
            if (!tarama) return '';
            if (tarama.durum === 'bitti') return 'Son tarama bitti';
            if (SON.indexOf(tarama.durum) !== -1 || tarama.durum === 'secim') return '';
            return 'Tarama sürüyor';
        },
    };
    let ilkKontrol = false;
    setInterval(function() {
        if (!document.body) return;
        panelKur();
        if (!ilkKontrol) {
            ilkKontrol = true;
            api('').then(function(j) {
                tarama = j.tarama;
                if (tarama && SON.indexOf(tarama.durum) === -1 && tarama.durum !== 'secim') { ciz(); izle(); }
            }).catch(function() {});
        }
    }, 1000);
})();

// ── İŞ LİSTESİ ───────────────────────────────────────────────────────────────────────
// Dosya kartına "İş Ata": o dosyanın işleri + yeni iş formu (iş UYAP dosyasına bağlanır). Veri büro
// veritabanında (/__cu__/veri/is/, enjekte/isler.js); buluttaki eski İş Listesi'ne bağlantı 17.11.2026'da kaldırıldı.
(function() {
    // Dışarıdan (cokluuyap.com, tarayıcıdan) girişte UYAP, sitenin kabuğundaki #app çerçevesinde açılır ve
    // istekler (iş uçları dahil) ofise tünellenir.
    function kabukCercevesi() {
        try { return window.top !== window && window.parent === window.top && !!window.frameElement && window.frameElement.id === 'app'; }
        catch (e) { return false; }
    }
    const UZAK = kabukCercevesi();
    // Panel / sağ menü yalnız ana çerçevede kurulur (UYAP'ın iç çerçevelerinde ikincisi olmasın).
    function anaCerceve() { return window.top === window || UZAK; }
    const DURUM = { acik: 'Açık', devam: 'Devam ediyor', onayda: 'Onayda', onaylandi: 'Onaylandı', tamamlandi: 'Tamamlandı' };
    let ben = null;            // {me, people, office} — ilk açılışta alınır

    // 2026-11-16: İş Listesi büro veritabanına taşındı (enjekte/isler.js, /__cu__/veri/is/): dosyanın işleri
    // bu uçlardan okunur (kart rozeti, mor bant,
    // belge işaretleri). "İş ata" kartın üstündeki pencerede kalır (yeni uçlara kaydeder; "Geniş formda aç" ile
    // sayfaya geçilir), "İşleri göster" yan çekmecede açılır ("Geniş ekranda" ile sayfaya). Banttaki hızlı düğmeler
    // (onaya gönder / tamamla, ertesi güne aktar, onayla / reddet) yeni uçlara bağlıdır (yeni* pencereler);
    // yetkiyi sunucu denetler. Aktarma talebine karar yeni sayfadaki iş panelinde verilir.
    async function yeniIstek(uc) {
        const r = await window.fetch('/__cu__/veri/is/' + uc, { cache: 'no-store', credentials: 'same-origin' });
        let j = null;
        try { j = await r.json(); } catch (e) {}
        if (!j || !j.ok) throw new Error((j && j.error) || 'İş Listesi okunamadı.');
        return j.veri;
    }
    async function yeniGonder(uc, veri) {
        let r;
        try {
            r = await window.fetch('/__cu__/veri/is/' + uc, { method: 'POST', cache: 'no-store', credentials: 'same-origin',
                headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(veri) });
        } catch (e) { throw new Error('Ofis bilgisayarına ulaşılamadı.'); }
        let j = null;
        try { j = await r.json(); } catch (e) {}
        if (!j || !j.ok) throw new Error((j && j.error) || ('HTTP ' + r.status));
        return j.veri;
    }
    async function yeniBen() {
        const b = await yeniIstek('ben');
        return { me: { id: b.ben.id, display_name: b.ben.ad, role: b.ben.yonetici ? 'admin' : 'user', account_username: b.ben.hesap },
                 people: (b.kisiler || []).map(function(k) { return { id: k.id, display_name: k.ad, active: true }; }), office: null,
                 etiketler: b.etiketler || [] };
    }
    // Yeni iş satırı → bu bloğun beklediği eski biçim (yalnız okunan alanlar).
    function eskiBicim(g) {
        const k = g.uyap_karti || {};
        const p = String(g.son_tarih || '').split('-').map(Number);
        const due = p.length === 3 ? Math.floor(new Date(p[0], p[1] - 1, p[2], 17, 0).getTime() / 1000) : null;
        const hm = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2})/.exec(g.hatirlatma || '');
        const hatirlatma = hm ? Math.floor(new Date(+hm[1], hm[2] - 1, +hm[3], +hm[4], +hm[5]).getTime() / 1000) : null;
        return { _yeni: true, id: g.id, reminder_at: hatirlatma, title: g.baslik, status: g.durum, priority: g.oncelik, due_at: due,
                 evrak_id: k._evrak || null, evrak_adi: g.evrak_adi || null, taraf_adi: g.taraf_adi || null, barkod: g.barkod || null,
                 ext_pending: !!g.erteleme_bekliyor, assignee_id: g.sorumlu_id, assignee_name: g.sorumlu_adi,
                 parent_id: g.ust_gorev_id || null, is_private: !!g.gizli, created_by: g.olusturan_id, onay_gerekir: !!g.onay_gerekir,
                 etiketler: g.etiketler || [],
                 birim_id: k.birimId != null ? String(k.birimId) : '', birim_adi: g.birim_adi, dosya_no: g.dosya_no };
    }
    // "İş ata": yeni sayfanın iş formu, dosya bağıyla. İş açılınca (ya da vazgeçilince) kullanıcı karta döner.
    function yeniIsAta(d, evrakId, on) {
        on = on || {};
        const kart = window.__uyapKartAlanlari ? window.__uyapKartAlanlari(d) : Object.assign({}, d);
        if (!kart.dosyaTurKodAciklama && (d.dosyaTur || d.dosyaTurKodAciklama)) kart.dosyaTurKodAciklama = d.dosyaTur || d.dosyaTurKodAciklama;
        if (evrakId) kart._evrak = String(evrakId);
        const dosyaNo = String(d.dosyaNo || '').trim();
        window.__cuIsAta({
            baslik: on.baslik || '', aciklama: on.aciklama || '', sablon_adi: on.sablon || '',
            birim_adi: d.birimAdi || '', dosya_no: dosyaNo, evrak_adi: on.evrakAdi || '',
            donus_kart: window.__uyapDosyaAnahtari ? window.__uyapDosyaAnahtari(d) : null,
            dosya: { birim_adi: d.birimAdi || '', dosya_no: dosyaNo, dosya_turu: kart.dosyaTurKodAciklama || '',
                     uyap_karti: kart, barkod: on.barkod || '', birim_uyap_id: d.birimId != null ? String(d.birimId) : '' }
        });
    }

    // UYAP'ın dosyaId'si oturuma bağlı (her girişte değişir): iş ↔ dosya eşleşmesi birim + dosya no ile.
    function dosyaAnahtari(d) { return String(d.birimId || '') + '|' + String(d.dosyaNo || '').trim(); }
    async function dosyaninIsleri(d) {
        const q = new URLSearchParams({ gorunum: 'tum', durum: 'acik,devam,onayda', dosya_no: String(d.dosyaNo || '').trim(),
                                        birim_adi: d.birimAdi || '', sinir: '100' });
        return (await yeniIstek('liste?' + q.toString())).map(eskiBicim);
    }
    function el(tag, css, metin) {
        const e = document.createElement(tag);
        if (css) e.style.cssText = css;
        if (metin != null) e.textContent = metin;
        return e;
    }
    function dugme(metin, renk, fn) {
        const b = el('button', 'margin:10px 6px 0 0;padding:6px 12px;border:0;border-radius:7px;cursor:pointer;font-size:12px;font-weight:600;color:' + (/--cu-vurgu/.test(renk) ? 'var(--cu-vurgu-uzeri,#fff)' : '#fff') + ';background:' + renk + ';', metin);
        b.type = 'button';
        b.addEventListener('click', function(ev) { ev.stopPropagation(); fn(b); });
        return b;
    }
    function tarih(ts) {
        if (!ts) return '';
        const d = new Date(ts * 1000);
        return d.toLocaleDateString('tr-TR') + ' ' + d.toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit' });
    }
    // İş Listesi'ni yeni sekmede açar; işNo verilirse doğrudan o iş.
    function isAc(isNo) {
        if (typeof isNo === 'number' && window.__cuIsGoster) window.__cuIsGoster(isNo);
        else if (window.__cuIslerAc) window.__cuIslerAc();
    }
    function ac() { isAc(null); }

    // Kişinin kendisi + büro kişileri: bant ve paneldeki "Onaya gönder" için; 5 dk saklanır.
    let benZamani = 0;
    async function beniGetir(zorla) {
        if (zorla || !ben || Date.now() - benZamani > 300000) { ben = await yeniBen(); benZamani = Date.now(); }
        return ben;
    }

    // ── UYAP'tan çıkmadan işi bitirme: "Onaya gönder" (özel işte "Tamamla") ──
    // Sorumlu, işi açan ya da yönetici kapatabilir / aktarabilir (sunucu ayrıca denetler).
    function yeniCalisabilir(t) {
        if (!ben || t.parent_id || (t.status !== 'acik' && t.status !== 'devam')) return false;
        return t.assignee_id === ben.me.id || t.created_by === ben.me.id || ben.me.role === 'admin';
    }
    function kapanisDugmesi(t, sinif) {
        if (!yeniCalisabilir(t)) return null;
        const b = el('button', null, t.onay_gerekir ? 'Onaya gönder' : 'Tamamla');
        b.type = 'button';
        b.className = sinif;
        b.title = t.onay_gerekir ? 'İşi yaptım: onaylanmak üzere gönder' : 'İşi tamamla';
        b.addEventListener('click', function(ev) { ev.stopPropagation(); yeniKapanisPenceresi(t); });
        return b;
    }
    // "Ertesi güne aktar": son tarihi ileri alma (not zorunlu). Onaylı işte talep yöneticiye / onaylayana gider.
    function aktarDugmesi(t, sinif) {
        if (!yeniCalisabilir(t)) return null;
        if (t.ext_pending) {
            const s = el('span', 'font-size:11px;opacity:.85;', 'aktarım onayda');
            s.title = 'Ertesi güne aktarma / ek süre talebi yöneticinin kararını bekliyor';
            return s;
        }
        const b = el('button', null, 'Ertesi güne aktar');
        b.type = 'button';
        b.className = sinif;
        b.title = 'Son tarihi bir iş günü ileri almak için yöneticiden onay iste (not zorunlu)';
        b.addEventListener('click', function(ev) { ev.stopPropagation(); yeniAktarPenceresi(t); });
        return b;
    }
    // ── Yan panelden karar: yönetici / işi açan, onaya gönderilen işi onaylar ya da reddeder (ret gerekçesi
    // zorunlu, yetkiyi sunucu da denetler). Aktarma talebine karar İş Listesi'ndeki iş panelinde verilir.
    function onayYetkili(t) {
        return !!ben && !t.parent_id && ben.me.role === 'admin';
    }
    function kararDugmeleri(t, kabulSinif, retSinif) {
        const out = [];
        // Onaylayan: yönetici ya da işi açan (kendi işini onaylayamaz; sunucu onaylayıcı listesini de bilir).
        if (!ben || t.parent_id || t.assignee_id === ben.me.id || (ben.me.role !== 'admin' && t.created_by !== ben.me.id)) return out;
        const ekle = function(metin, baslik, sinif, fn) {
            const b = el('button', null, metin);
            b.type = 'button';
            b.className = sinif;
            b.title = baslik;
            b.addEventListener('click', function(ev) { ev.stopPropagation(); fn(); });
            out.push(b);
        };
        if (t.status === 'onayda') {
            ekle('Onayla', 'İşi onayla (isteğe bağlı not)', kabulSinif, function() { yeniKararPenceresi(t, true); });
            ekle('Reddet', 'Geri gönder: iş "devam"a döner (gerekçe zorunlu)', retSinif, function() { yeniKararPenceresi(t, false); });
        }
        if (t.ext_pending && window.__cuIsGoster) {
            ekle('Aktarma talebi', 'Ertesi güne aktarma talebini İş Listesi\'nde gör ve karar ver', kabulSinif, function() { window.__cuIsGoster(t.id); });
        }
        return out;
    }
    // Onaya gönderilen işin özeti (panel satırında).
    function kararOzeti(t) {
        return 'Onaya gönderildi' + (t.submit_note ? ': ' + t.submit_note : ' (not yok)');
    }
    function tarihGirdisi(d) {
        const p = function(x) { return String(x).padStart(2, '0'); };
        return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) + 'T' + p(d.getHours()) + ':' + p(d.getMinutes());
    }
    // Son tarih (geçmişse bugün) + 1 iş günü, 17:00; cumartesi / pazar atlanır.
    function ertesiIsGunu(dueTs) {
        const d = new Date(Math.max(dueTs ? dueTs * 1000 : 0, Date.now()));
        d.setDate(d.getDate() + 1);
        while (d.getDay() === 0 || d.getDay() === 6) d.setDate(d.getDate() + 1);
        d.setHours(17, 0, 0, 0);
        return d;
    }
    function pencereBasi(baslik, t) {
        kur();
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', baslik));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', kapat);
        ust.appendChild(x);
        kutu.appendChild(ust);
        kutu.appendChild(el('div', 'margin-top:4px;font-weight:600;word-break:break-word;', t.title));
        const yer = [t.birim_adi, t.dosya_no].filter(Boolean).join(' ');
        if (yer) kutu.appendChild(el('div', 'color:#cbd5e1;', yer + (t.evrak_adi ? ' · Belge: ' + t.evrak_adi : '')));
        mesajEl = el('div', 'margin-top:8px;color:#fca5a5;');
        kutu.appendChild(mesajEl);
    }
    function yeniBasari(metin, sure) {
        mesajEl.style.color = '#86efac';
        mesajEl.textContent = metin;
        isleriTazele();
        setTimeout(function() { kapat(); mesajEl.style.color = '#fca5a5'; }, sure || 1000);
    }
    function yeniHata(b, onek, e) {
        mesajEl.style.color = '#fca5a5';
        mesajEl.textContent = onek + e.message;
        if (b) b.disabled = false;
    }
    function gunMetni(d) {
        const p = function(x) { return String(x).padStart(2, '0'); };
        return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate());
    }
    // Yeni iş: onaya gönder / tamamla (kontrol sonucu isteğe bağlı) → şablon sonraki işi önerirse aynı pencerede açılır.
    function yeniKapanisPenceresi(t) {
        const gonder = t.onay_gerekir;
        pencereBasi(gonder ? 'Onaya gönder' : 'İşi tamamla', t);
        let sonuc = '';
        const sonucYeri = el('div', '');
        sonucYeri.appendChild(el('div', 'margin-top:8px;font-weight:600;', 'Kontrol sonucu (isteğe bağlı)'));
        SONUC.forEach(function(x) {
            const b = el('button', null, x[1]);
            b.type = 'button';
            b.className = 'ia-cip';
            b.addEventListener('click', function(ev) {
                ev.stopPropagation();
                sonuc = sonuc === x[0] ? '' : x[0];
                Array.prototype.forEach.call(sonucYeri.querySelectorAll('.ia-cip'), function(c) { c.classList.toggle('sec', c === b && !!sonuc); });
            });
            sonucYeri.appendChild(b);
        });
        kutu.appendChild(sonucYeri);
        const not = el('textarea'); not.rows = 3; not.maxLength = 1800;
        not.placeholder = 'Ne görüldü / ne yapıldı? (isteğe bağlı)';
        kutu.appendChild(alan('Not', not));
        const dugmeler = el('div', '');
        dugmeler.appendChild(dugme(gonder ? 'Onaya gönder' : 'Tamamla', 'var(--cu-vurgu)', async function(b) {
            b.disabled = true;
            mesajEl.textContent = '';
            try {
                const n = not.value.trim();
                // Sonuçsuz kapanışta not, işin notlarına yazılır (sonuç notu yalnız sonuçla saklanır).
                if (n && !sonuc) await yeniGonder('not', { id: t.id, metin: n });
                const v = await yeniGonder('durum', { id: t.id, durum: 'tamamla', sonuc: sonuc, not: sonuc ? n : '' });
                const bitti = v.durum === 'onayda' ? 'Onaya gönderildi.' : 'İş tamamlandı.';
                if (!v.oneri) { yeniBasari(bitti); return; }
                // Sonraki iş önerisi: tarih ve kişi değiştirilebilir.
                const o = v.oneri;
                isleriTazele();
                dugmeler.innerHTML = '';
                mesajEl.style.color = '#86efac';
                mesajEl.textContent = bitti;
                const yer = el('div', 'margin-top:8px;padding:8px;border-radius:8px;background:rgba(255,255,255,0.06);');
                yer.appendChild(el('div', 'font-weight:600;', 'Sonraki iş' + (o.sonuc_adi ? ' (' + o.sonuc_adi + ')' : '') + ': ' + o.baslik));
                if (o.sure_metni) yer.appendChild(el('div', 'font-size:11px;color:#9ca3af;', o.sure_metni));
                const gun = el('input'); gun.type = 'date'; gun.value = o.son_tarih || gunMetni(ertesiIsGunu(null));
                const kisi = el('select');
                ((ben && ben.people) || []).forEach(function(k) {
                    const op = el('option', null, k.display_name);
                    op.value = String(k.id);
                    if (k.id === (o.sorumlu_id || t.assignee_id)) op.selected = true;
                    kisi.appendChild(op);
                });
                yer.appendChild(alan('Son gün', gun));
                yer.appendChild(alan('Sorumlu', kisi));
                kutu.insertBefore(yer, dugmeler);
                dugmeler.appendChild(dugme('Sonraki işi aç', 'var(--cu-vurgu)', async function(b2) {
                    if (!gun.value) { mesajEl.textContent = 'Son günü seçin.'; gun.focus(); return; }
                    b2.disabled = true;
                    try {
                        await yeniGonder('oneriden_ac', { onceki: t.id, sablon_id: o.sablon_id, son_tarih: gun.value, sorumlu_id: Number(kisi.value) || null });
                        yeniBasari('Sonraki iş açıldı.');
                    } catch (e) { yeniHata(b2, 'Açılamadı: ', e); }
                }));
                dugmeler.appendChild(dugme('Gerek yok', 'rgba(255,255,255,0.15)', kapat));
            } catch (e) { yeniHata(b, 'Kaydedilemedi: ', e); }
        }));
        dugmeler.appendChild(dugme('Vazgeç', 'rgba(255,255,255,0.15)', kapat));
        kutu.appendChild(dugmeler);
        setTimeout(function() { not.focus(); }, 0);
    }
    // Yeni iş: ertesi güne aktar. Onay gerekmeyen işte (ya da onaylayan aktarınca) hemen uygulanır.
    function yeniAktarPenceresi(t) {
        pencereBasi('Ertesi güne aktar', t);
        kutu.appendChild(el('div', 'margin-top:6px;color:#cbd5e1;', 'Şu anki son gün: ' + (t.due_at ? new Date(t.due_at * 1000).toLocaleDateString('tr-TR') : 'yok')));
        const gun = el('input'); gun.type = 'date';
        gun.value = gunMetni(ertesiIsGunu(t.due_at));
        kutu.appendChild(alan('Yeni son gün', gun));
        const not = el('textarea'); not.rows = 3; not.maxLength = 2000;
        not.placeholder = 'Neden aktarılıyor? (zorunlu — ör. tebligat sonucu henüz düşmedi)';
        kutu.appendChild(alan('Not', not));
        if (t.onay_gerekir) kutu.appendChild(el('div', 'margin-top:6px;color:#9ca3af;font-size:11px;',
            'Onaylı işte talep yöneticiye / onaylayana gider; kabul edilince son gün değişir.'));
        kutu.appendChild(dugme(t.onay_gerekir ? 'Aktar / onaya gönder' : 'Aktar', 'var(--cu-vurgu)', async function(b) {
            if (!not.value.trim()) { mesajEl.textContent = 'Aktarma nedenini yazın.'; not.focus(); return; }
            if (!gun.value) { mesajEl.textContent = 'Yeni günü seçin.'; gun.focus(); return; }
            b.disabled = true;
            mesajEl.textContent = '';
            try {
                const v = await yeniGonder('ertele', { id: t.id, yeni_tarih: gun.value, gerekce: not.value });
                yeniBasari(v && v.uygulandi ? 'Son gün ' + new Date(gun.value + 'T12:00').toLocaleDateString('tr-TR') + ' oldu.' : 'Aktarma talebi onaya gönderildi.', 1100);
            } catch (e) { yeniHata(b, 'Gönderilemedi: ', e); }
        }));
        kutu.appendChild(dugme('Vazgeç', 'rgba(255,255,255,0.15)', kapat));
        setTimeout(function() { not.focus(); }, 0);
    }
    // Yeni iş: onaydaki işi onayla ya da geri gönder (ret gerekçesi işin notlarına yazılır).
    function yeniKararPenceresi(t, kabul) {
        pencereBasi(kabul ? 'İşi onayla' : 'İşi reddet', t);
        const not = el('textarea'); not.rows = 3; not.maxLength = 2000;
        not.placeholder = kabul ? 'Not (isteğe bağlı)' : 'Ne eksik? İş "devam"a döner (zorunlu)';
        kutu.appendChild(alan(kabul ? 'Not' : 'Ret gerekçesi', not));
        kutu.appendChild(dugme(kabul ? 'Onayla' : 'Reddet', kabul ? 'var(--cu-vurgu)' : '#dc2626', async function(b) {
            const n = not.value.trim();
            if (!kabul && !n) { mesajEl.textContent = 'Ret gerekçesini yazın.'; not.focus(); return; }
            b.disabled = true;
            mesajEl.textContent = '';
            try {
                if (n) await yeniGonder('not', { id: t.id, metin: (kabul ? 'Onay notu: ' : 'Ret gerekçesi: ') + n });
                await yeniGonder('durum', { id: t.id, durum: kabul ? 'tamamla' : 'devam', sonuc: '', not: '' });
                yeniBasari(kabul ? 'İş onaylandı.' : 'Reddedildi; iş "devam"a döndü.', 900);
            } catch (e) { yeniHata(b, 'Kaydedilemedi: ', e); }
        }));
        kutu.appendChild(dugme('Vazgeç', 'rgba(255,255,255,0.15)', kapat));
        setTimeout(function() { not.focus(); }, 0);
    }
    // ── Hatırlatıcı: kullanıcı kendine kurar (başkasına kurma ve liste İş Listesi'nde) ──
    function bitmis(t) { return t.status === 'onaylandi' || t.status === 'tamamlandi' || t.status === 'tamam' || t.status === 'iptal'; }
    function hatirlaticiDugmesi(t, sinif) {
        if (!ben || t.parent_id || bitmis(t)) return null;
        const b = el('button', null, t.reminder_at ? '🔔 ' + tarih(t.reminder_at) : '🔔 Hatırlatıcı');
        b.type = 'button';
        b.className = sinif;
        b.title = t.reminder_at ? 'Sıradaki hatırlatma: ' + tarih(t.reminder_at) + ' — yeni hatırlatıcı kur' : 'Bu iş için kendinize hatırlatıcı kurun';
        b.addEventListener('click', function(ev) { ev.stopPropagation(); hatirlaticiPenceresi(t); });
        return b;
    }
    // Hazır seçenekler (1 saat sonra, yarın 09:00, son tarihten önce …) + tarih/saat girdisi.
    function hatirlaticiSecimi(dueAt) {
        const girdi = el('input'); girdi.type = 'datetime-local';
        const yer = el('div', 'display:flex;flex-wrap:wrap;gap:4px;margin-top:4px;');
        const simdi = Date.now() / 1000;
        const saatte = function(gun, ss) { const d = new Date(); d.setDate(d.getDate() + gun); d.setHours(ss, 0, 0, 0); return d.getTime() / 1000; };
        const secenek = [['1 saat sonra', Math.ceil((simdi + 3600) / 300) * 300], ['Bu akşam 17:00', saatte(0, 17)], ['Yarın 09:00', saatte(1, 9)]];
        if (dueAt) secenek.push(['Son tarihten 1 gün önce', dueAt - 86400], ['Son tarihte', dueAt]);
        const cipler = [];
        secenek.filter(function(s) { return s[1] > simdi + 30; }).forEach(function(s) {
            const b = el('button', null, s[0]);
            b.type = 'button';
            b.className = 'ia-cip';
            b.addEventListener('click', function(ev) {
                ev.stopPropagation();
                girdi.value = tarihGirdisi(new Date(s[1] * 1000));
                cipler.forEach(function(c) { c.classList.toggle('sec', c === b); });
            });
            cipler.push(b);
            yer.appendChild(b);
        });
        girdi.addEventListener('input', function() { cipler.forEach(function(c) { c.classList.remove('sec'); }); });
        const kutu_ = el('div', '');
        kutu_.appendChild(yer);
        kutu_.appendChild(girdi);
        return { el: kutu_, girdi: girdi, temizle: function() { girdi.value = ''; cipler.forEach(function(c) { c.classList.remove('sec'); }); },
                 deger: function() { return girdi.value ? Math.floor(new Date(girdi.value).getTime() / 1000) : null; } };
    }
    async function hatirlaticiKur(taskId, ts, not) {
        if (ts < Date.now() / 1000 - 60) throw new Error('Geçmiş bir zamana hatırlatıcı kurulamaz.');
        await yeniGonder('hatirlatici', { gorev_id: taskId, zaman: tarihGirdisi(new Date(ts * 1000)), notu: not || '' });
    }
    function hatirlaticiPenceresi(t) {
        pencereBasi('Hatırlatıcı', t);
        if (t.reminder_at) kutu.appendChild(el('div', 'margin-top:6px;color:#cbd5e1;', 'Sıradaki hatırlatma: ' + tarih(t.reminder_at)));
        const secim = hatirlaticiSecimi(t.due_at);
        kutu.appendChild(alan('Ne zaman?', secim.el));
        const not = el('input'); not.maxLength = 500;
        not.placeholder = 'Not (isteğe bağlı) — ör. “Müvekkili ara”';
        kutu.appendChild(alan('Not', not));
        kutu.appendChild(el('div', 'margin-top:6px;color:#9ca3af;font-size:11px;',
            'Zamanı gelince Çoklu UYAP\'ta yalnız size uyarı çıkar; iş panelinden silebilirsiniz.'));
        kutu.appendChild(dugme('Kur', 'var(--cu-vurgu)', async function(b) {
            const ts = secim.deger();
            if (!ts) { mesajEl.textContent = 'Tarih ve saat seçin.'; secim.girdi.focus(); return; }
            b.disabled = true;
            mesajEl.textContent = '';
            try {
                await hatirlaticiKur(t.id, ts, not.value);
                mesajEl.style.color = '#86efac';
                mesajEl.textContent = 'Hatırlatıcı kuruldu: ' + tarih(ts);
                isleriTazele();
                setTimeout(function() { kapat(); mesajEl.style.color = '#fca5a5'; }, 1100);
            } catch (e) {
                mesajEl.style.color = '#fca5a5';
                mesajEl.textContent = 'Kurulamadı: ' + e.message;
                b.disabled = false;
            }
        }));
        kutu.appendChild(dugme('Vazgeç', 'rgba(255,255,255,0.15)', kapat));
    }

    // ── Kontrol sonucu → sonraki iş (model: "yapılacak iş.md" §3) ──
    const SONUC = [['olumlu', 'Olumlu'], ['olumsuz', 'Olumsuz'], ['bekliyor', 'Bekliyor']];
    // İş açılınca / durumu değişince: kart bandı, belge işaretleri ve panel yeniden çekilir.
    function isleriTazele() {
        Object.keys(dosyaIsleri).forEach(function(k) { delete dosyaIsleri[k]; });
        panelYenile();
    }

    // ── Pencere ──
    let ort, kutu, mesajEl;
    function kur() {
        if (ort || !document.body) return;
        const stil = document.createElement('style');
        stil.textContent =
            '#uyap-isata{color:#f3f4f6!important}' +
            '#uyap-isata div,#uyap-isata span,#uyap-isata label,#uyap-isata a,#uyap-isata p{color:inherit;font-family:inherit;font-size:inherit;margin-bottom:0}' +
            '#uyap-isata input,#uyap-isata textarea,#uyap-isata select{width:100%;box-sizing:border-box;margin-top:3px;padding:6px 8px;border-radius:7px;border:1px solid rgba(255,255,255,0.2);background:rgba(255,255,255,0.08);color:#f3f4f6;font:inherit}' +
            '#uyap-isata select option{color:#111827}' +
            // Evrak ağacı: bitmemiş işi olan belge mor; satır sonunda işe giden düğme ("+ İş" yalnız üstüne gelince).
            '.uyap-isli-belge{background:var(--cu-vurgu-zemin)!important;box-shadow:inset 3px 0 0 var(--cu-vurgu-yazi);border-radius:6px}' +
            '.uyap-isli-belge,.uyap-isli-belge .dx-treeview-item-content,.uyap-isli-belge span{color:var(--cu-vurgu-yazi)!important;font-weight:600}' +
            // Dosya Bilgileri / Evrak sekmesinde "bu dosyada yapılacak iş var" bandı.
            '.uyap-is-uyari{display:flex;align-items:center;gap:12px;margin:8px 8px 4px;padding:8px 12px;border-radius:8px;background:var(--cu-vurgu-zemin);border:1px solid var(--cu-vurgu-acik);box-shadow:inset 4px 0 0 var(--cu-vurgu);color:var(--cu-vurgu-yazi);font:13px/1.45 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif}' +
            '.uyap-is-uyari-is{display:inline-block;margin-right:10px;color:var(--cu-vurgu-koyu);text-decoration:underline;cursor:pointer}' +
            '.uyap-is-uyari-dugme{flex:none;padding:6px 14px;border:0;border-radius:7px;background:var(--cu-vurgu);color:var(--cu-vurgu-uzeri,#fff);font-weight:600;cursor:pointer}' +
            '.uyap-is-uyari-dugme:hover{background:var(--cu-vurgu-koyu)}' +
            // Belge satırındaki iş düğmesi (kullanıcı bulgusu 2026-11-11: UYAP metne order:2 verdiği için sona eklenen
            // düğme metnin SOLUNDA beliriyor, yazıyı itiyor ve dar sütuna sıkıştırıyordu). İşli belgede "İş →"
            // satırın en sonunda (order:3, indirme simgesinin sağı) kalıcı durur; işsiz belgedeki "+ İş" yalnız
            // fare gelince, indirme simgesinin solunda akış DIŞINDA belirir — metin hiç kaymaz.
            '.evrak-treeview .dx-treeview-item{position:relative}' +
            '.uyap-belge-is{order:3;flex:none;align-self:center;margin-left:4px;padding:1px 8px;border:0;border-radius:999px;background:var(--cu-vurgu);color:var(--cu-vurgu-uzeri,#fff);font:600 11px/18px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;cursor:pointer;white-space:nowrap;box-shadow:0 1px 4px rgba(0,0,0,0.15)}' +
            '.uyap-belge-is:hover{background:var(--cu-vurgu-koyu)}' +
            '.uyap-belge-is.bos{display:none;position:absolute;right:52px;top:50%;transform:translateY(-50%);z-index:2;margin:0;background:#fff;color:var(--cu-vurgu-yazi);border:1px dashed var(--cu-vurgu-kenar)}' +
            '.dx-treeview-item:hover > .uyap-belge-is.bos{display:inline-block}' +
            '.uyap-belge-is.bos:hover{background:var(--cu-vurgu-zemin)}' +
            '.uyap-is-uyari-satir{display:flex;align-items:center;flex-wrap:wrap;gap:4px 8px;margin-top:2px}' +
            '.uyap-is-uyari-kucuk{padding:1px 9px;border:1px solid var(--cu-vurgu-orta);border-radius:999px;background:#fff;color:var(--cu-vurgu-koyu);font:600 11px/18px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;cursor:pointer}' +
            '.uyap-is-uyari-kucuk:hover{background:var(--cu-vurgu-zemin)}' +
            // İş Ata penceresi: şablon / süre çipleri.
            '#uyap-isata .ia-cip{display:inline-block;margin:4px 4px 0 0;padding:3px 10px;border-radius:999px;border:1px solid rgba(255,255,255,0.22);background:transparent;color:#e5e7eb;cursor:pointer;font-family:inherit;font-size:12px;line-height:1.4}' +
            '#uyap-isata .ia-cip:hover{background:rgba(255,255,255,0.08)}' +
            '#uyap-isata .ia-cip.buro{border-color:rgba(167,139,250,0.65)}' +
            '#uyap-isata .ia-cip.sec{background:var(--cu-vurgu);border-color:var(--cu-vurgu);color:var(--cu-vurgu-uzeri,#fff)}' +
            '#uyap-isata .ia-not{margin-top:4px;font-size:11px;color:#9ca3af}' +
            '#uyap-isata .ia-link{padding:0;border:0;background:none;color:var(--cu-vurgu-acik);cursor:pointer;font-family:inherit;font-size:11px;text-decoration:underline}' +
            '#uyap-isata .ia-link:hover{color:var(--cu-vurgu-zemin)}' +
            // Sağ tık menüsü.
            '#uyap-sagmenu .uyap-sagmenu-madde{padding:6px 14px;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}' +
            '#uyap-sagmenu .uyap-sagmenu-madde:hover,#uyap-sagmenu .uyap-sagmenu-madde.sec{background:rgba(var(--cu-vurgu-rgb),0.4)}' +
            // UYAP ana sayfası "Bildirimler": bildirimin dosyasına iş.
            '.uyap-bildirim-is{margin-left:8px;padding:1px 9px;border:1px solid var(--cu-vurgu-kenar);border-radius:999px;background:var(--cu-vurgu-zemin);color:var(--cu-vurgu-yazi);font:600 11px/18px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;cursor:pointer;white-space:nowrap}' +
            '.uyap-bildirim-is:hover{background:var(--cu-vurgu-zemin)}';
        document.head.appendChild(stil);
        ort =el('div', 'position:fixed;inset:0;z-index:2147483646;background:rgba(0,0,0,0.45);display:none;align-items:flex-start;justify-content:center;padding-top:8vh;');
        ort.id = 'uyap-isata-ort';
        ort.addEventListener('click', function(ev) { if (ev.target === ort) kapat(); });
        kutu = el('div', 'width:min(520px,92vw);max-height:80vh;overflow:auto;padding:16px;border-radius:12px;background:rgba(var(--cu-panel-rgb),0.97);border:1px solid rgba(255,255,255,0.15);box-shadow:0 10px 30px rgba(0,0,0,0.45);font:12px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;');
        kutu.id = 'uyap-isata';
        ort.appendChild(kutu);
        document.body.appendChild(ort);
        document.addEventListener('keydown', function(ev) { if (ev.key === 'Escape' && ort.style.display !== 'none') kapat(); });
    }
    function kapat() { if (ort) ort.style.display = 'none'; }

    // kart: dosya kartı (evrak ağacından belge seçimi için); evrakId: belge satırından açıldıysa seçili gelir.
    // on: formun ön doldurması {sablon (adı), baslik, aciklama} — ör. bildirimden açılınca.
    async function pencereAc(d, kart, evrakId, on) {
        kur();
        if (!kutu) return;
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const ad = window.__uyapDosyaAdi ? window.__uyapDosyaAdi(d) : String(d.dosyaNo || d.dosyaId);
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', 'İş Ata'));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', kapat);
        ust.appendChild(x);
        kutu.appendChild(ust);
        kutu.appendChild(el('div', 'margin-top:2px;color:#cbd5e1;word-break:break-word;', ad || 'UYAP dosyası'));
        mesajEl = el('div', 'margin-top:8px;color:#fca5a5;');
        kutu.appendChild(mesajEl);
        const liste = el('div', 'margin-top:10px;');
        kutu.appendChild(liste);
        const formYeri = el('div', '');
        kutu.appendChild(formYeri);
        liste.appendChild(el('div', 'color:#9ca3af;', 'Yükleniyor…'));
        try {
            await beniGetir(true);   // her açılışta: büroya eklenen/silinen kişiler hemen yansısın
            await listeyiYukle(liste, d);
            formYeri.appendChild(formKur(d, liste, kart, evrakId, on || {}));
        } catch (e) {
            liste.innerHTML = '';
            mesajEl.textContent = e.message;
        }
    }

    function kisiAdi(id) {
        const p = ben && ben.people.find(function(x) { return x.id === id; });
        return p ? p.display_name : '—';
    }

    async function listeyiYukle(liste, d) {
        const isler = await dosyaninIsleri(d);
        liste.innerHTML = '';
        dosyaIsleri[dosyaAnahtari(d)] = { t: Date.now(), isler: isler };     // kart düğmesi / belge işaretleri
        liste.appendChild(el('div', 'font-weight:600;color:#cbd5e1;margin-bottom:4px;',
            isler.length ? 'Bu dosyanın işleri (' + isler.length + ')' : 'Bu dosyaya bağlı iş yok.'));
        isler.forEach(function(t) {
            const s = el('div', 'padding:6px 8px;margin-top:4px;border-radius:8px;background:rgba(255,255,255,0.06);cursor:pointer;');
            s.title = 'İş Listesi\'nde aç';
            s.addEventListener('click', function() { isAc(t.id); });
            s.appendChild(el('div', 'font-weight:600;word-break:break-word;', t.title));
            const kim = (t.assignees && t.assignees.length ? t.assignees : [t.assignee_id]).filter(Boolean).map(kisiAdi).join(', ');
            s.appendChild(el('div', 'color:#cbd5e1;', [DURUM[t.status] || t.status, kim, t.due_at ? 'Son: ' + tarih(t.due_at) : '']
                .filter(Boolean).join(' · ')));
            if (t.taraf_adi) s.appendChild(el('div', 'color:#cbd5e1;word-break:break-word;', 'Taraf: ' + t.taraf_adi));
            if (t.evrak_id) s.appendChild(el('div', 'color:var(--cu-vurgu-acik);word-break:break-word;', 'Belge: ' + (t.evrak_adi || t.evrak_id)));
            if (t.barkod) s.appendChild(el('div', 'color:#cbd5e1;', 'Barkod: ' + t.barkod));
            const etAd = etiketAdlari(t.etiketler);
            if (etAd) s.appendChild(el('div', 'color:var(--cu-vurgu-acik);', 'Etiket: ' + etAd));
            const pb = pttDugmesi(t, 'ia-cip');
            if (pb) s.appendChild(pb);
            const kb = kapanisDugmesi(t, 'ia-cip sec');
            if (kb) s.appendChild(kb);
            const ab = aktarDugmesi(t, 'ia-cip');
            if (ab) s.appendChild(ab);
            const hb = hatirlaticiDugmesi(t, 'ia-cip');
            if (hb) s.appendChild(hb);
            liste.appendChild(s);
        });
    }

    // ── İş şablonları: hazır (sunucu kodunda) + büronun eklediği; 5 dk saklanır ──
    let sablonlar = null, sablonZamani = 0;
    async function sablonlariGetir(zorla) {
        if (zorla || !sablonlar || Date.now() - sablonZamani > 300000) {
            const l = await yeniIstek('sablonlar');
            sablonlar = { hazir: [], buro: l.filter(function(x) { return x.aktif !== false; }).map(function(x) {
                return { id: x.id, title: x.baslik, description: x.aciklama || '', priority: x.oncelik, taraf: x.taraf || null,
                         action: x.eylem || '', kontrol: x.kontrol || '', onay_gerekir: !!x.onay_gerekir, sure_metni: x.sure_metni || '',
                         etiketler: x.etiketler || [], due_days: x.sure_turu === 'takvim_gunu' ? x.sure_deger : null };
            }) };
            sablonZamani = Date.now();
        }
        return sablonlar;
    }
    // Şablon yönetimi İş Listesi'nde (Şablonlar sekmesi); dönünce liste tazelensin diye saklanan silinir.
    function sablonlariYonet() {
        sablonZamani = 0;
        kapat();
        if (window.__cuIslerAc) window.__cuIslerAc();
    }
    // Hangi şablonu ne sık kullandığınız (bu bilgisayarda): pencerede en çok kullanılanlar önde gelir.
    const SAYAC_ANAHTARI = 'cokluuyapIsSablonSayac';
    function sablonSayaci() {
        try { return JSON.parse(localStorage.getItem(SAYAC_ANAHTARI) || '{}') || {}; } catch (e) { return {}; }
    }
    function sablonKullanildi(id) {
        try {
            const s = sablonSayaci();
            s[id] = (s[id] || 0) + 1;
            localStorage.setItem(SAYAC_ANAHTARI, JSON.stringify(s));
        } catch (e) {}
    }
    function kucukHarf(s) { return String(s || '').toLocaleLowerCase('tr-TR').replace(/\s+/g, ' ').trim(); }

    // ── Dosyanın tarafları (UYAP "Taraf Bilgileri" ucu): borçlular önce ──
    function rolSirasi(rol) { return /borçlu/i.test(rol) ? 0 : /alacakl/i.test(rol) ? 2 : 1; }
    async function dosyaTaraflari(d) {
        const j = await uyapPost('dosya_taraf_bilgileri_brd.ajx', { dosyaId: d.dosyaId });
        return (Array.isArray(j) ? j : []).filter(function(x) { return x && x.adi; })
            .map(function(x) { return { adi: String(x.adi).replace(/\s+/g, ' ').trim(), rol: String(x.rol || '').trim() }; })
            .sort(function(a, b) { return rolSirasi(a.rol) - rolSirasi(b.rol); });
    }
    // UYAP adları büyük harf: "HAMZA OĞUZ" → "Hamza Oğuz" (iş başlığında).
    function adDuzelt(s) {
        return kucukHarf(s).replace(/(^|[\s.\-\/(])(\S)/g, function(m, a, b) { return a + b.toLocaleUpperCase('tr-TR'); });
    }

    // ── Son tarih düğmeleri: bugünden n gün sonrası, saat 17:00 ──
    const SURELER = [[0, 'Bugün'], [1, 'Yarın'], [3, '3 gün'], [7, '1 hafta'], [15, '15 gün'], [30, '1 ay']];
    function gunSonra(n) {
        const t = new Date();
        t.setDate(t.getDate() + n);
        const p = function(x) { return String(x).padStart(2, '0'); };
        return t.getFullYear() + '-' + p(t.getMonth() + 1) + '-' + p(t.getDate()) + 'T17:00';
    }
    function kacGunSonra(v) {
        if (!v) return null;
        const a = new Date(); a.setHours(0, 0, 0, 0);
        const b = new Date(v); b.setHours(0, 0, 0, 0);
        return Math.max(0, Math.round((b - a) / 86400000));
    }

    // Belgenin kalıcı anahtarı: UYAP'ın evrakId'si de dosyaId gibi şifreli ve oturuma bağlı; işe
    // birim evrak no + tür + onay tarihi yazılır (ekte tür "(ek) <ad>" içerir). Sunucuda evrak_id alanı.
    function belgeAnahtari(o) {
        return [o.birimEvrakNo, o.tur, o.onaylandigiTarih].map(function(x) { return String(x == null ? '' : x).trim(); })
            .join('|').slice(0, 200);
    }
    // Kartın evrak ağacındaki belgeler (tekrarsız): [{evrakId: belge anahtarı, ad}]. Evrak sekmesi hiç açılmadıysa boş.
    function kartEvraklari(kart) {
        const A = window.__uyapEvrakAgaci;
        const tv = kart && A && kart.querySelector('.evrak-treeview');
        const koklar = tv && A.veri(tv);
        const sonuc = [], gorulen = new Set();
        (function gez(dizi, eb) {
            (dizi || []).forEach(function(x) {
                if (x.evrak && x.evrak.evrakId) {
                    const o = A.ozet(x, eb);
                    const a = belgeAnahtari(o);
                    if (!gorulen.has(a)) { gorulen.add(a); sonuc.push({ evrakId: a, ad: evrakAdi(o) }); }
                }
                gez(x.items, x);
            });
        })(koklar, null);
        return sonuc;
    }
    // Kartta görüntülenen belge (evrak ağacında seçili satır): {evrakId: belge anahtarı, ad, tur} | null.
    function acikBelge(kart) {
        const A = window.__uyapEvrakAgaci;
        const tv = kart && A && kart.querySelector('.evrak-treeview');
        const koklar = tv && A.veri(tv);
        const node = tv && tv.querySelector('.dx-treeview-node.dx-state-selected');
        const g = koklar && node ? A.harita(koklar)[node.getAttribute('data-item-id')] : null;
        if (!g || !g.x.evrak || !g.x.evrak.evrakId) return null;
        const o = A.ozet(g.x, g.ebeveyn);
        return { evrakId: belgeAnahtari(o), ad: evrakAdi(o), tur: String(o.tur || '') };
    }
    // Görüntülenen belgedeki PTT barkodu: Code-39 yazısıyla "*4007770084489*" (13 hane).
    function belgedekiBarkod(kart) {
        // İki görüntüleyici: rpv-core ve react-pdf (canlı 20.11.2026: Kapalı Tebligat PDF'i react-pdf'te, barkod
        // yazı katmanında "*5003049769049*" diye ayrı bir span).
        const l = kart ? kart.querySelectorAll('.rpv-core__text-layer-text, .react-pdf__Page__textContent span') : [];
        for (let i = 0; i < l.length; i++) {
            const m = /^\*\s*((?:\d\s*){13})\*$/.exec(l[i].textContent.trim());
            if (m) return m[1].replace(/\s/g, '');
        }
        return '';
    }
    // "Tebliğ durumu kontrol" gibi barkodla çalışan şablonlar (sunucuda action "ptt:barkod").
    const PTT_SABLONU = 'Tebliğ durumu kontrol';
    function pttSablonu(s) { return !!(s && s.action === 'ptt:barkod'); }
    function pttDugmesi(t, sinif) {
        if (!t.barkod) return null;
        const b = el('button', null, 'PTT sorgula');
        b.type = 'button';
        b.className = sinif;
        b.title = 'Barkod ' + t.barkod + ' için PTT gönderi takip sorgusu';
        b.addEventListener('click', function(ev) { ev.stopPropagation(); pttSorgula(t.barkod); });
        return b;
    }
    function evrakAdi(o) {
        return [o.tur, o.birimEvrakNo ? '(' + o.birimEvrakNo + ')' : '', o.onaylandigiTarih ? String(o.onaylandigiTarih).slice(0, 10) : '']
            .filter(Boolean).join(' ').replace(/\s+/g, ' ').trim().slice(0, 200);
    }

    function etiketAdlari(ids) {
        const l = (ben && ben.etiketler) || [];
        return (ids || []).map(function(id) { const t = l.find(function(x) { return x.id === id; }); return t ? t.ad : ''; }).filter(Boolean).join(', ');
    }
    // İş formunda etiket seçimi (yeni sistem): UYAP İşi, Adliye İşi … çip; diğerleri "+ diğer" ile. .deger() → [id]
    function etiketSecici() {
        const l = (ben && ben.etiketler) || [];
        const on_ = l.filter(function(t) { return /^is-/.test(t.anahtar || ''); }), diger = l.filter(function(t) { return !/^is-/.test(t.anahtar || ''); });
        const set = new Set();
        let digerAcik = false;
        const yer = el('div', '');
        function ciz() {
            yer.innerHTML = '';
            on_.concat(digerAcik ? diger : []).forEach(function(t) {
                const b = el('button', null, t.ad);
                b.type = 'button';
                b.className = 'ia-cip' + (set.has(t.id) ? ' sec' : '');
                b.addEventListener('click', function(ev) { ev.stopPropagation(); if (set.has(t.id)) set.delete(t.id); else set.add(t.id); ciz(); });
                yer.appendChild(b);
            });
            if (!digerAcik && diger.length) {
                const b = el('button', null, '+ diğer');
                b.type = 'button';
                b.className = 'ia-cip';
                b.addEventListener('click', function(ev) { ev.stopPropagation(); digerAcik = true; ciz(); });
                yer.appendChild(b);
            }
        }
        ciz();
        return { el: yer, deger: function() { return Array.from(set); },
                 ayarla: function(ids) { set.clear(); (ids || []).forEach(function(x) { set.add(Number(x)); });
                                         if ((ids || []).some(function(id) { return diger.some(function(t) { return t.id === Number(id); }); })) digerAcik = true; ciz(); } };
    }
    function alan(etiket, girdi) {
        const l = el('label', 'display:block;margin-top:8px;font-weight:600;color:#cbd5e1;', etiket);
        l.appendChild(girdi);
        return l;
    }

    function formKur(d, liste, kart, evrakId, on) {
        const f = el('div', 'margin-top:12px;padding-top:10px;border-top:1px solid rgba(255,255,255,0.12);');
        f.appendChild(el('div', 'font-weight:700;', 'Yeni iş'));
        // Şablonlar: tıklayınca iş adı, açıklama, süre, öncelik ve (gerekirse) taraf dolar.
        const sablonAra = el('input'); sablonAra.placeholder = 'Şablon ara… (Enter: ilkini seç)'; sablonAra.maxLength = 100;
        const sablonYeri = el('div', '');
        const sablonNot = el('div', '');
        sablonNot.className = 'ia-not';
        const sablonKutu = el('div', '');
        let sablonKaydet = null;                    // "Bu işi şablon olarak kaydet" (aşağıda kurulur)
        let sablonYapGoster = function() {};
        sablonKutu.appendChild(alan('Şablon', sablonAra));
        sablonKutu.appendChild(sablonYeri);
        sablonKutu.appendChild(sablonNot);
        f.appendChild(sablonKutu);
        // Taraf: iş dosyadaki bir kişiye yapılır (birden çok borçlu olabilir); başlığın başına adı gelir.
        const taraf = el('select');
        let taraflar = [];
        const tarafYok = el('option', null, '— Taraf seçilmedi —'); tarafYok.value = ''; taraf.appendChild(tarafYok);
        const baslik = el('input'); baslik.maxLength = 300; baslik.placeholder = 'Ne yapılacak? (ör. Tebligat kontrol)';
        const onizleme = el('div', 'margin-top:3px;color:var(--cu-vurgu-acik);word-break:break-word;');
        function tamBaslik() {
            const ad = taraf.value !== '' && taraflar[Number(taraf.value)] ? adDuzelt(taraflar[Number(taraf.value)].adi) : '';
            const is = baslik.value.trim();
            return ad && is ? ad + ' – ' + is : is;
        }
        function onizle() {
            const t = tamBaslik();
            onizleme.textContent = t && t !== baslik.value.trim() ? 'Kaydedilecek başlık: ' + t : '';
        }
        baslik.addEventListener('input', onizle);
        taraf.addEventListener('change', onizle);
        const aciklama = el('textarea'); aciklama.rows = 3; aciklama.maxLength = 5000;
        const kisi = el('select');
        const admin = ben.me.role === 'admin';
        ben.people.filter(function(p) { return p.active; }).forEach(function(p) {
            const o = el('option', null, p.display_name + (p.id === ben.me.id ? ' (ben)' : ''));
            o.value = String(p.id);
            if (p.id === ben.me.id) o.selected = true;
            kisi.appendChild(o);
        });
        // Üye yalnız kendine iş açar (sunucu da böyle uygular).
        if (!admin) { kisi.disabled = true; kisi.title = 'Başkasına iş yalnız yönetici atayabilir'; }
        // Son tarih: gün + İSTEĞE BAĞLI saat (istek B-9). datetime-local'de saat girilmezse değer boş
        // kalıyor, iş tarihsiz açılıyordu; şimdi saat boşsa 17:00. Dış yüz eski girdiyle aynı: value
        // "YYYY-AA-GGTSS:DD" okunur/yazılır, iç girdilerin 'input' olayı kapsayıcıya kabarır.
        const son = el('div', 'display:flex;gap:6px;');
        const sonGun = el('input'); sonGun.type = 'date'; sonGun.style.flex = '1 1 auto';
        const sonSaat = el('input'); sonSaat.type = 'time'; sonSaat.style.flex = '0 0 96px';
        sonSaat.title = 'İsteğe bağlı — boşsa 17:00';
        son.appendChild(sonGun); son.appendChild(sonSaat);
        Object.defineProperty(son, 'value', {
            get: function() { return sonGun.value ? sonGun.value + 'T' + (sonSaat.value || '17:00') : ''; },
            set: function(v) { v = v || ''; sonGun.value = v.slice(0, 10); sonSaat.value = v.length > 11 ? v.slice(11, 16) : ''; }
        });
        // Süre düğmeleri: son tarihi bugünden n gün sonrası 17:00 yapar; seçilen gün şablona da yazılır.
        let secilenGun = null;
        const sureYeri = el('div', '');
        const sureDugmeleri = SURELER.map(function(s) {
            const b = el('button', null, s[1]);
            b.type = 'button';
            b.className = 'ia-cip';
            b.addEventListener('click', function(ev) { ev.stopPropagation(); sureSec(s[0]); });
            sureYeri.appendChild(b);
            return b;
        });
        function sureSec(n) {
            secilenGun = n;
            son.value = n == null ? '' : gunSonra(n);
            sureDugmeleri.forEach(function(b, i) { b.classList.toggle('sec', SURELER[i][0] === n); });
        }
        son.addEventListener('input', function() {
            secilenGun = kacGunSonra(son.value);
            sureDugmeleri.forEach(function(b, i) { b.classList.toggle('sec', SURELER[i][0] === secilenGun); });
        });
        const oncelik = el('select');
        [['1', 'Normal'], ['2', 'Acil'], ['0', 'Düşük']].forEach(function(x) {
            const o = el('option', null, x[1]); o.value = x[0]; oncelik.appendChild(o);
        });
        const tarafAlani = alan('Taraf', taraf);
        tarafAlani.style.display = 'none';            // taraf listesi gelince görünür
        f.appendChild(tarafAlani);
        f.appendChild(alan('İş', baslik));
        f.appendChild(onizleme);
        f.appendChild(alan('Açıklama', aciklama));
        f.appendChild(alan('Atanacak kişi', kisi));
        const sonAlani = alan('Son tarih', son);
        sonAlani.appendChild(sureYeri);
        f.appendChild(sonAlani);
        f.appendChild(alan('Öncelik', oncelik));
        // Etiket ve "onay gerekir".
        const etiketSec = etiketSecici();
        const onayK = el('input'); onayK.type = 'checkbox'; onayK.style.cssText = 'width:auto;margin:0 6px 0 0;vertical-align:middle;';
        f.insertBefore(alan('Etiket', etiketSec.el), f.querySelector('textarea').parentNode);
        const onayEtiketi = el('label', 'display:flex;align-items:center;margin-top:8px;font-weight:600;color:#cbd5e1;cursor:pointer;');
        onayEtiketi.appendChild(onayK);
        onayEtiketi.appendChild(document.createTextNode('Onay gerekir (tamamlanınca onaya gider)'));
        f.appendChild(onayEtiketi);
        // Hatırlatıcı (isteğe bağlı): iş açılınca işi açana kurulur.
        const hatirlat = hatirlaticiSecimi(null);
        f.appendChild(alan('Bana hatırlat (isteğe bağlı)', hatirlat.el));

        if (on.baslik) baslik.value = on.baslik;
        if (on.aciklama) aciklama.value = on.aciklama;
        let sonSablonAciklamasi = '';
        let seciliSablon = null;
        let barkodGoster = function() {};           // barkod alanı (aşağıda kurulur)
        function sablonUygula(s) {
            seciliSablon = s;
            barkodGoster();
            baslik.value = s.title;
            // Elle yazılmış (ya da bildirimden gelen) açıklama korunur; boşsa / önceki şablonunsa değişir.
            if (!aciklama.value.trim() || aciklama.value === sonSablonAciklamasi) aciklama.value = s.description || '';
            else if (s.description && aciklama.value.indexOf(s.description) < 0) aciklama.value = s.description + '\n' + aciklama.value;
            sonSablonAciklamasi = s.description || '';
            if (s.due_days != null) sureSec(Number(s.due_days));
            oncelik.value = String(s.priority == null ? 1 : s.priority);
            onayK.checked = !!s.onay_gerekir;
            if ((s.etiketler || []).length && !etiketSec.deger().length) etiketSec.ayarla(s.etiketler);
            sablonNot.title = s.sure_metni ? 'Son tarih boşsa şablona göre: ' + s.sure_metni : '';
            if (s.taraf === 'borclu' && taraf.value === '') {
                const i = taraflar.findIndex(function(x) { return rolSirasi(x.rol) === 0; });
                if (i >= 0) taraf.value = String(i);
            }
            sablonlariCiz();
            onizle();
            baslik.focus();
        }
        // Şablon sayısı artınca pencere şişmesin: arama boşken en çok kullandıklarınızdan 6, yazınca
        // en iyi 8 eşleşme görünür. Tüm liste, düzenleme ve silme İş Listesi → Şablonlar'da.
        let gorunenler = [];
        function sablonlariCiz() {
            sablonYeri.innerHTML = '';
            sablonNot.innerHTML = '';
            if (!sablonlar) return;
            const q = kucukHarf(sablonAra.value.trim());
            const sayac = sablonSayaci();
            const uygun = sablonlar.hazir.concat(sablonlar.buro).filter(function(s) { return !q || kucukHarf(s.title).indexOf(q) >= 0; });
            // Önce adı aramayla başlayanlar, sonra çok kullanılanlar; eşitlikte sunucunun sırası.
            uygun.sort(function(a, b) {
                const ba = q && kucukHarf(a.title).indexOf(q) === 0 ? 0 : 1, bb = q && kucukHarf(b.title).indexOf(q) === 0 ? 0 : 1;
                return ba - bb || (sayac[b.id] || 0) - (sayac[a.id] || 0);
            });
            gorunenler = uygun.slice(0, q ? 8 : 6);
            const secili = seciliSablon && uygun.find(function(s) { return s.id === seciliSablon.id; });
            if (secili && gorunenler.indexOf(secili) < 0) gorunenler.unshift(secili);   // seçili şablon hep görünür
            gorunenler.forEach(function(s) {
                const buro = typeof s.id === 'number';
                const b = el('button', null, s.title);
                b.type = 'button';
                b.className = 'ia-cip' + (buro ? ' buro' : '') + (seciliSablon && seciliSablon.id === s.id ? ' sec' : '');
                b.title = [s.description, s.due_days != null ? s.due_days + ' gün' : '',
                           buro ? 'Büro şablonu' + (s.created_by_name ? ' · ' + s.created_by_name : '') : 'Hazır şablon']
                    .filter(Boolean).join('\n');
                b.addEventListener('click', function(ev) { ev.stopPropagation(); sablonUygula(s); });
                sablonYeri.appendChild(b);
            });
            const kalan = uygun.length - gorunenler.length;
            sablonNot.appendChild(document.createTextNode(
                !uygun.length ? 'Eşleşen şablon yok. ' : kalan > 0 ? '+' + kalan + ' şablon daha — aramaya yazın. ' : ''));
            const yonet = el('button', null, 'Şablonları yönet');
            yonet.type = 'button';
            yonet.className = 'ia-link';
            yonet.title = "İş Listesi'nde Şablonlar sekmesi: ekle, düzenle, sil";
            yonet.addEventListener('click', function(ev) { ev.stopPropagation(); sablonlariYonet(); });
            sablonNot.appendChild(yonet);
            sablonYapGoster();
        }
        sablonAra.addEventListener('input', sablonlariCiz);
        sablonAra.addEventListener('keydown', function(ev) {
            if (ev.key === 'Enter' && gorunenler.length) { ev.preventDefault(); sablonUygula(gorunenler[0]); }
            if (ev.key === 'Escape' && sablonAra.value) { ev.stopPropagation(); sablonAra.value = ''; sablonlariCiz(); }
        });
        sablonlariGetir().then(function() {
            sablonlariCiz();
            if (on.sablon) {
                const s = sablonlar.hazir.concat(sablonlar.buro).find(function(x) { return x.title === on.sablon; });
                if (s) sablonUygula(s);
            }
        }).catch(function() {                     // sunucuda şablon ucu yok (güncellenmemiş): bölüm gizlenir
            sablonKutu.style.display = 'none';
            if (sablonKaydet) sablonKaydet.style.display = 'none';
        });
        dosyaTaraflari(d).then(function(l) {
            taraflar = l;
            l.forEach(function(x, i) {
                const o = el('option', null, x.adi + (x.rol ? ' (' + x.rol + ')' : ''));
                o.value = String(i);
                taraf.appendChild(o);
            });
            if (!l.length) return;
            tarafAlani.style.display = '';
            // Varsayılan: ilk borçlu (işlerin çoğu borçluya yapılır); taraf seçmemek de mümkün.
            const i = l.findIndex(function(x) { return rolSirasi(x.rol) === 0; });
            if (i >= 0) taraf.value = String(i);
            onizle();
        }).catch(function() {});
        // Belge: iş dosyanın belirli bir evrakına bağlanabilir (evrak ağacında mor görünür).
        const evraklar = kartEvraklari(kart);
        const belge = el('select');
        const bos = el('option', null, '— Belge yok (dosyaya bağlı) —'); bos.value = ''; belge.appendChild(bos);
        evraklar.forEach(function(e) {
            const o = el('option', null, e.ad || e.evrakId);
            o.value = e.evrakId;
            if (evrakId && e.evrakId === String(evrakId)) o.selected = true;
            belge.appendChild(o);
        });
        if (evraklar.length) f.appendChild(alan('Belge', belge));
        // PTT barkodu: "Tebliğ durumu kontrol" gibi şablonda zorunlu; görüntülenen Kapalı Tebligat'tan
        // (ya da sağ tıkta seçilen barkoddan) hazır gelir, böylece her kontrolde belgeye dönülmez.
        const barkod = el('input'); barkod.maxLength = 30; barkod.inputMode = 'numeric';
        barkod.placeholder = '13 haneli barkod (ör. 4007770084489)';
        const acik = acikBelge(kart);
        const acikTebligat = !!(acik && /^kapalı tebligat/.test(kucukHarf(acik.tur)));
        barkod.value = on.barkod || (acikTebligat ? belgedekiBarkod(kart) : '');
        const barkodAlani = alan('PTT barkodu', barkod);
        barkodAlani.style.display = barkod.value ? '' : 'none';
        f.appendChild(barkodAlani);
        barkodGoster = function() {
            if (!pttSablonu(seciliSablon)) { if (!barkod.value) barkodAlani.style.display = 'none'; return; }
            barkodAlani.style.display = '';
            // Belge seçilmediyse görüntülenen Kapalı Tebligat'a bağla.
            if (!belge.value && acikTebligat
                && Array.prototype.some.call(belge.options, function(o) { return o.value === acik.evrakId; })) belge.value = acik.evrakId;
        };
        barkodGoster();
        f.appendChild(dugme('Kaydet', 'var(--cu-vurgu)', async function(b) {
            if (!baslik.value.trim()) { mesajEl.textContent = 'Ne yapılacağını yazın ya da bir şablon seçin.'; baslik.focus(); return; }
            const bk = barkod.value.replace(/\D/g, '');
            if (pttSablonu(seciliSablon) && bk.length !== 13) {
                mesajEl.textContent = 'Bu iş için 13 haneli PTT barkodunu girin (Kapalı Tebligat belgesindeki *…* numarası).';
                barkodAlani.style.display = ''; barkod.focus(); return;
            }
            b.disabled = true;
            mesajEl.textContent = '';
            const tarafSecili = taraf.value !== '' ? taraflar[Number(taraf.value)] : null;
            const hts = hatirlat.deger();
            if (hts && hts < Date.now() / 1000 - 60) {
                mesajEl.textContent = 'Hatırlatma zamanı geçmişte olamaz.'; hatirlat.girdi.focus(); b.disabled = false; return;
            }
            const tamBaslikDegeri = tamBaslik();
            {
                if (!sonGun.value && !(seciliSablon && baslik.value.trim() === seciliSablon.title)) {
                    mesajEl.textContent = 'Son tarihi seçin (ya da bir şablon seçin: süre şablondan hesaplanır).'; sonGun.focus(); b.disabled = false; return;
                }
                const kartV = window.__uyapKartAlanlari ? window.__uyapKartAlanlari(d) : kartAlanlari(d);
                if (!kartV.dosyaTurKodAciklama && (d.dosyaTur || d.dosyaTurKodAciklama)) kartV.dosyaTurKodAciklama = d.dosyaTur || d.dosyaTurKodAciklama;
                if (belge.value) kartV._evrak = belge.value;
                try {
                    const acilan = await yeniGonder('ac', {
                        baslik: tamBaslikDegeri, aciklama: aciklama.value,
                        sablon_id: seciliSablon && baslik.value.trim() === seciliSablon.title ? seciliSablon.id : null,
                        sorumlu_id: Number(kisi.value) || null, son_tarih: sonGun.value, son_saat: sonSaat.value,
                        oncelik: Number(oncelik.value), onay_gerekir: onayK.checked, etiketler: etiketSec.deger(),
                        birim_adi: d.birimAdi || '', dosya_no: String(d.dosyaNo || '').trim(), dosya_turu: kartV.dosyaTurKodAciklama || '',
                        taraf_adi: tarafSecili ? adDuzelt(tarafSecili.adi) : '',
                        evrak_adi: belge.value ? belge.options[belge.selectedIndex].textContent : '',
                        barkod: bk, uyap_karti: kartV, birim_uyap_id: d.birimId != null ? String(d.birimId) : '' });
                    if (seciliSablon) sablonKullanildi(seciliSablon.id);
                    // Hatırlatıcı ayrı istekte kurulur; kurulamazsa iş yine açılmış kalır.
                    let hHata = '';
                    if (hts && acilan && acilan.id) { try { await hatirlaticiKur(acilan.id, hts, ''); } catch (e) { hHata = e.message; } }
                    isleriTazele();
                    kutu.innerHTML = '';
                    kutu.appendChild(el('div', 'font-size:14px;font-weight:700;', 'İş Ata'));
                    kutu.appendChild(el('div', 'margin-top:10px;padding:10px 12px;border-radius:8px;background:rgba(255,255,255,0.06);font-weight:600;word-break:break-word;color:' + (hHata ? '#fca5a5' : '#86efac') + ';',
                        (onayK.checked ? 'İş açıldı; tamamlanınca onaya gidecek.' : 'İş açıldı.')
                        + (hHata ? ' Hatırlatıcı kurulamadı: ' + hHata : hts ? ' Hatırlatıcı kuruldu: ' + tarih(hts) : '')));
                    kutu.appendChild(el('div', 'margin-top:6px;color:#e5e7eb;word-break:break-word;', tamBaslikDegeri));
                    const tamam = dugme('Tamam', 'var(--cu-vurgu)', kapat);
                    kutu.appendChild(tamam);
                    kutu.appendChild(dugme('Bu dosyaya bir iş daha ata', 'rgba(255,255,255,0.15)', function() { pencereAc(d, kart, evrakId); }));
                    setTimeout(function() { tamam.focus(); }, 0);
                } catch (e) {
                    mesajEl.style.color = '#fca5a5';
                    mesajEl.textContent = 'İş açılamadı: ' + e.message;
                } finally { b.disabled = false; }
            }
        }));
        // Formdaki iş adı / açıklama / süre / öncelik büroya yeni şablon olur (taraf adı şablona girmez).
        // Büyük düğme değil, "İş" alanının altında küçük bağlantı: yalnız şablonu olmayan bir iş yazılınca görünür.
        sablonKaydet = el('button', null, '☆ Bu işi şablon olarak kaydet');
        sablonKaydet.type = 'button';
        sablonKaydet.className = 'ia-link';
        sablonKaydet.style.display = 'none';
        sablonKaydet.title = 'Büronun şablonlarına eklenir; düzenlemek için İş Listesi → Şablonlar';
        f.insertBefore(sablonKaydet, onizleme.nextSibling);
        sablonYapGoster = function() {          // şablon listesi her çizildiğinde de çağrılır
            const ad = kucukHarf(baslik.value);
            const var_ = sablonlar && sablonlar.hazir.concat(sablonlar.buro).some(function(s) { return kucukHarf(s.title) === ad; });
            sablonKaydet.style.display = ad && sablonlar && !var_ && sablonKutu.style.display !== 'none' ? '' : 'none';
        };
        baslik.addEventListener('input', sablonYapGoster);
        sablonYapGoster();
        sablonKaydet.addEventListener('click', async function(ev) {
            ev.stopPropagation();
            const b = sablonKaydet;
            const ad = baslik.value.trim();
            if (!ad) { mesajEl.textContent = 'Şablon için "İş" alanını doldurun.'; baslik.focus(); return; }
            b.disabled = true;
            const ts = taraf.value !== '' ? taraflar[Number(taraf.value)] : null;
            try {
                const g0 = secilenGun != null ? secilenGun : kacGunSonra(son.value);
                await yeniGonder('sablon', { baslik: ad, aciklama: aciklama.value, oncelik: Number(oncelik.value),
                    sure_turu: 'takvim_gunu', sure_deger: g0 == null ? 1 : g0, onay_gerekir: onayK.checked, etiketler: etiketSec.deger(),
                    taraf: ts ? (rolSirasi(ts.rol) === 0 ? 'borclu' : rolSirasi(ts.rol) === 2 ? 'alacakli' : 'herhangi') : '' });
                await sablonlariGetir(true);
                seciliSablon = sablonlar.buro.find(function(x) { return kucukHarf(x.title) === kucukHarf(ad); }) || null;
                sablonlariCiz();
                mesajEl.style.color = '#86efac';
                mesajEl.textContent = 'Şablon kaydedildi: ' + ad;
                setTimeout(function() { if (mesajEl) { mesajEl.textContent = ''; mesajEl.style.color = '#fca5a5'; } }, 3000);
            } catch (e) {
                mesajEl.style.color = '#fca5a5';
                mesajEl.textContent = 'Şablon kaydedilemedi: ' + e.message;
            } finally { b.disabled = false; }
        });
        f.appendChild(dugme('Geniş formda aç', 'rgba(255,255,255,0.15)', function() {
            const ad0 = baslik.value.trim();
            kapat();
            yeniIsAta(d, belge.value || evrakId, { baslik: tamBaslik(), aciklama: aciklama.value,
                sablon: seciliSablon && ad0 === seciliSablon.title ? seciliSablon.title : '', barkod: barkod.value.replace(/\D/g, '') });
        }));
        f.appendChild(dugme('İş Listesi\'nde aç', 'rgba(255,255,255,0.15)', ac));
        f.appendChild(dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
        setTimeout(function() { baslik.focus(); }, 0);
        return f;
    }

    // ── İş Listesi'nden "UYAP'ta aç": UYAP'ın kendi dosya kartını açma ──
    // UYAP ana sayfasındaki arama bileşeni bir dosya sonucu seçilince kartı şöyle açar:
    //   I({dosyaId, dosyaNo, birimId, birimAdi, dosyaTurKod, dosyaTurKodAciklama, dosyaDurum,
    //      dosyaDurumKod, birimTuru3}); w(!0)        (UYAP main.js, "dosya-sorgula-popup")
    // Bileşenin kancaları sırasıyla: …, [E,w] (kart görünür mü), [b,I] (kart verisi), [B,x],
    // S=useRef(#searchId), D=useRef(#mainSearchArea). Kancalar S ref'inden geriye sayılarak bulunur;
    // yapı tutmazsa hiçbir şey yapılmaz (UYAP güncellenmiş olabilir) ve kullanıcıya söylenir.
    const KART_ALANLARI = ['dosyaId', 'dosyaNo', 'birimId', 'birimAdi', 'dosyaTurKod', 'dosyaTurKodAciklama',
                           'dosyaDurum', 'dosyaDurumKod', 'birimTuru3'];
    function kartAlanlari(d) {
        const o = {};
        KART_ALANLARI.forEach(function(k) { if (d[k] !== undefined && d[k] !== null) o[k] = d[k]; });
        return o;
    }
    function uyapKartiAc(dosya) {
        const ara = document.getElementById('searchId');
        if (!ara) return false;
        const a = Object.keys(ara).find(function(k) { return k.indexOf('__reactFiber') === 0; });
        for (let f = a ? ara[a] : null, i = 0; f && i < 40; i++, f = f.return) {
            if (typeof f.type !== 'function') continue;
            const kancalar = [];
            for (let h = f.memoizedState; h && kancalar.length < 200; h = h.next) kancalar.push(h);
            const ri = kancalar.findIndex(function(h) {
                return !h.queue && h.memoizedState && typeof h.memoizedState === 'object' && h.memoizedState.current === ara;
            });
            if (ri < 3) continue;
            const gorunur = kancalar[ri - 3], veri = kancalar[ri - 2];
            if (!gorunur.queue || !veri.queue || typeof gorunur.memoizedState !== 'boolean'
                || typeof gorunur.queue.dispatch !== 'function' || typeof veri.queue.dispatch !== 'function') return false;
            veri.queue.dispatch(dosya);
            gorunur.queue.dispatch(true);
            return true;
        }
        return false;
    }
    // ── Saklanan kartı güncel kimlikle açma ──
    // UYAP'ın dosyaId'si şifreli ve oturuma bağlı: işte saklanan değer sonraki oturumda "Doğrulama
    // hatası" verir. Kart açılmadan önce dosya, UYAP'ın Dosya Sorgulama ucuyla (yıl/sıra + birimId)
    // yeniden bulunur. Uç birimTuru2 (yargı birimi kodu, ör. icra dairesi 1101) ister; kart verisinde
    // yok. Önce birimTuru3'ün ailesinden tahmin edilir (1199 → 1101), olmazsa yargı türlerinin birim
    // listelerinden aynı aileden kodlar denenir. Bulunan kod birim başına saklanır.
    const BT2_ANAHTAR = 'cokluuyapBirimTuru2';
    async function uyapPost(uc, govde) {
        const r = await window.fetch('/' + uc, { method: 'POST', cache: 'no-store',
            headers: { 'Content-Type': 'application/json', 'Accept': 'application/json, text/plain, */*' },
            body: JSON.stringify(govde) });
        return r.json();
    }
    function bt2Sakli() {
        try { return JSON.parse(localStorage.getItem(BT2_ANAHTAR) || '{}') || {}; } catch (e) { return {}; }
    }
    async function dosyayiBul(dosya, bt2) {
        const m = /(\d{4})\s*\/\s*(\d+)/.exec(String(dosya.dosyaNo || ''));
        if (!m || !dosya.birimId) return null;
        // Önbelleği (SWR) atlayan ham fetch: önbellek search_phrase_detayli yanıtını da saklıyor ve
        // önceki oturumun (artık geçersiz) dosyaId'sini dönebiliyordu (2026-11-11).
        const r = await hamFetch('/search_phrase_detayli.ajx', { method: 'POST',
            headers: { 'Content-Type': 'application/json', 'Accept': 'application/json, text/plain, */*' },
            body: JSON.stringify({
                dosyaDurumKod: typeof dosya.dosyaDurumKod === 'number' ? dosya.dosyaDurumKod : 0,
                pageSize: 50, pageNumber: 1, dosyaYil: Number(m[1]), dosyaSira: Number(m[2]),
                birimId: String(dosya.birimId), birimTuru2: bt2 }) });
        const j = await r.json();
        const kayitlar = Array.isArray(j) && Array.isArray(j[0]) ? j[0] : (Array.isArray(j) ? j : []);
        return kayitlar.find(function(k) {
            // Yenilenen dosyada UYAP numarayı "2026/5683 - ( 2022/11833 )" verir (yeni esas no önde, eskisi parantezde):
            // birebir karşılaştırma tutmuyor, dosya "bulunamadı" deniyordu (kullanıcı bulgusu 23.11.2026). Arama kayıttaki
            // ilk yıl/sıra ile (yeni esas no) yapılır; sonuçtaki numaraların herhangi biri tutarsa eşleşir. Böylece
            // veritabanında yalnız eski numara varsa da (yenilemeden sonra henüz taranmadı) parantezdeki eski no sayılır.
            if (!k || String(k.birimId) !== String(dosya.birimId)) return false;
            const nolar = String(k.dosyaNo || '').match(/\d{4}\s*\/\s*\d+/g) || [];
            return nolar.some(function(x) { const n = /(\d{4})\s*\/\s*(\d+)/.exec(x); return Number(n[1]) === Number(m[1]) && Number(n[2]) === Number(m[2]); });
        }) || null;
    }
    async function bt2Adaylari(dosya) {
        const b3 = String(dosya.birimTuru3 || '');
        const aile = b3.slice(0, 2);
        const ilk = [b3, aile ? aile + '01' : ''].filter(Boolean);
        const ek = [];
        for (const yt of ['0', '1', '2', '3', '4', '5', '6', '7', '8']) {
            try {
                const l = await uyapPost('yargiBirimleriSorgula_brd.ajx', { yargiTuru: yt });
                (Array.isArray(l) ? l : []).forEach(function(b) {
                    const t = String(b && b.tablo || '');
                    if (t && (!aile || t.slice(0, 2) === aile) && ilk.indexOf(t) < 0 && ek.indexOf(t) < 0) ek.push(t);
                });
            } catch (e) {}
        }
        return { ilk: ilk, ek: ek };
    }
    async function tazeKart(dosya) {
        const sakli = bt2Sakli();
        const dene = async function(bt2) {
            const k = await dosyayiBul(dosya, bt2);
            if (!k || !k.dosyaId) return null;
            sakli[String(dosya.birimId)] = k.birimTuru2 || bt2;
            try { localStorage.setItem(BT2_ANAHTAR, JSON.stringify(sakli)); } catch (e) {}
            return { dosyaId: k.dosyaId, dosyaNo: k.dosyaNo, birimId: k.birimId, birimAdi: k.birimAdi,
                     dosyaTurKod: k.dosyaTurKod, dosyaTurKodAciklama: k.dosyaTur || dosya.dosyaTurKodAciklama,
                     dosyaDurum: k.dosyaDurum, dosyaDurumKod: k.dosyaDurumKod, birimTuru3: k.birimTuru3 };
        };
        try {
            if (sakli[String(dosya.birimId)]) {
                const k = await dene(sakli[String(dosya.birimId)]);
                if (k) return k;
            }
            const a = await bt2Adaylari({ birimTuru3: dosya.birimTuru3 });
            for (const bt2 of a.ilk.concat(a.ek)) {
                const k = await dene(bt2);
                if (k) return k;
            }
        } catch (e) {}
        return null;       // bulunamadı: saklanan veriyle denenir (aynı oturumdaysa geçerlidir)
    }

    let acmaUyarisi = null, acmaSuruyor = false;
    async function bekleyenDosyaAc() {
        if (acmaSuruyor) return;
        let kayit = null;
        try { kayit = JSON.parse(sessionStorage.getItem('cokluuyapDosyaAc') || 'null'); } catch (e) {}
        if (!kayit) return;
        const sil = function() { try { sessionStorage.removeItem('cokluuyapDosyaAc'); } catch (e) {} };
        if (!kayit.dosya || !kayit.dosya.dosyaId || Date.now() - (kayit.t || 0) > 90000) { sil(); return; }
        if (!document.getElementById('searchId')) {
            // Ana sayfa henüz çizilmedi (ya da UYAP girişi bekleniyor) — sonraki turda tekrar.
            return;
        }
        // Kayda körü körüne güvenilmez: yalnız kart alanlarının düz değerleri.
        let dosya = {};
        KART_ALANLARI.forEach(function(k) {
            const v = kayit.dosya[k];
            if (typeof v === 'string' || typeof v === 'number') dosya[k] = typeof v === 'string' ? v.slice(0, 200) : v;
        });
        acmaSuruyor = true;
        try { dosya = (await tazeKart(dosya)) || dosya; } finally { acmaSuruyor = false; }
        if (uyapKartiAc(dosya)) { sil(); evrakSekmesiIstendi = Date.now(); return; }
        sil();
        if (!acmaUyarisi && document.body) {
            acmaUyarisi = el('div', 'position:fixed;left:16px;bottom:60px;z-index:2147483646;max-width:360px;padding:10px 12px;border-radius:10px;background:rgba(var(--cu-panel-rgb),0.95);color:#fca5a5;font:12px/1.45 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;',
                'Dosya kartı otomatik açılamadı (UYAP sayfası değişmiş olabilir). Dosya: ' +
                [kayit.dosya.birimAdi, kayit.dosya.dosyaNo].filter(Boolean).join(' ') + ' — üstteki aramadan açabilirsiniz.');
            acmaUyarisi.id = 'uyap-acma-uyarisi';
            acmaUyarisi.addEventListener('click', function() { acmaUyarisi.remove(); acmaUyarisi = null; });
            document.body.appendChild(acmaUyarisi);
        }
    }

    // ── Sağda açılır iş paneli ──
    // Sağ kenardaki "İşler" sekmesi paneli açar: kişinin işleri (yöneticide onay bekleyen / tümü),
    // UYAP dosyasına bağlı işte dosya kartını bu sayfada açma, işe tıklayınca İş Listesi'nde açma.
    // Sekmedeki sayı kişinin bitmemiş işleri; arada bir ve panel açılınca tazelenir.
    const BITTI = { onaylandi: 1, tamamlandi: 1 };
    const pnl = { kok: null, sekme: null, rozet: null, govde: null, cipler: null, filtre: 'benim', isler: [],
                  bittiGoster: false, acik: false, yukleniyor: false };

    function panelKur() {
        if (pnl.kok || !document.body || !anaCerceve()) return;    // çerçevelerde ikinci panel olmasın
        kur();
        const stil = document.createElement('style');
        stil.textContent =
            '#uyap-ispanel{position:fixed;top:0;right:0;bottom:0;width:min(380px,92vw);z-index:2147483644;display:flex;flex-direction:column;' +
            'background:rgba(var(--cu-panel-rgb),0.97);color:#f3f4f6;border-left:1px solid rgba(255,255,255,0.15);box-shadow:-10px 0 30px rgba(0,0,0,0.35);' +
            'font:12px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;transform:translateX(100%);transition:transform .2s ease;visibility:hidden}' +
            '#uyap-ispanel.acik{transform:none;visibility:visible}' +
            '#uyap-ispanel div,#uyap-ispanel span,#uyap-ispanel button{font-family:inherit;margin-bottom:0}' +
            '#uyap-ispanel .ip-cip{padding:4px 10px;border-radius:999px;border:1px solid rgba(255,255,255,0.2);background:transparent;color:#cbd5e1;cursor:pointer;font-size:12px}' +
            '#uyap-ispanel .ip-cip.sec{background:var(--cu-vurgu);border-color:var(--cu-vurgu);color:var(--cu-vurgu-uzeri,#fff)}' +
            '#uyap-ispanel .ip-is{padding:8px 10px;margin-top:6px;border-radius:9px;background:rgba(255,255,255,0.06);cursor:pointer;border-left:3px solid transparent}' +
            '#uyap-ispanel .ip-is:hover{background:rgba(255,255,255,0.11)}' +
            '#uyap-ispanel .ip-is.acil{border-left-color:#f87171}' +
            '#uyap-ispanel .ip-dosya{margin-top:5px;padding:3px 8px;border-radius:6px;border:1px solid rgba(var(--cu-vurgu-parlak-rgb),0.5);background:rgba(var(--cu-vurgu-rgb),0.18);color:var(--cu-vurgu-acik);cursor:pointer;font-size:11px;max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;text-align:left}' +
            '#uyap-ispanel .ip-dosya:hover{background:rgba(var(--cu-vurgu-rgb),0.35)}' +
            '#uyap-ispanel .ip-onay{flex:none;margin-top:5px;padding:3px 10px;border:0;border-radius:6px;background:var(--cu-vurgu);color:var(--cu-vurgu-uzeri,#fff);font-size:11px;font-weight:600;cursor:pointer}' +
            '#uyap-ispanel .ip-onay:hover{background:var(--cu-vurgu-koyu)}' +
            '#uyap-ispanel .ip-aktar{flex:none;margin-top:5px;padding:3px 10px;border:1px solid rgba(255,255,255,0.25);border-radius:6px;background:transparent;color:#e5e7eb;font-size:11px;font-weight:600;cursor:pointer}' +
            '#uyap-ispanel .ip-aktar:hover{background:rgba(255,255,255,0.08)}' +
            '#uyap-ispanel .ip-red{flex:none;margin-top:5px;padding:3px 10px;border:1px solid rgba(248,113,113,0.6);border-radius:6px;background:transparent;color:#fca5a5;font-size:11px;font-weight:600;cursor:pointer}' +
            '#uyap-ispanel .ip-red:hover{background:rgba(220,38,38,0.2)}' +
            '#uyap-ispanel .ip-link{background:none;border:0;padding:0;color:var(--cu-vurgu-acik);cursor:pointer;font-size:12px;text-decoration:underline}' +
            // Sağ kenar tutamacı: ekrandaki TEK sabit CokluUyap öğesi. İnce ve yarı saydam; üstüne
            // gelince belirginleşir. Dikey yeri sekmeYerlestir() seçer (UYAP'ın kendi düğmelerini örtmez).
            '#uyap-issekme{position:fixed;right:0;top:120px;z-index:2147483644;padding:9px 3px;border:0;border-radius:8px 0 0 8px;cursor:pointer;' +
            'background:var(--cu-vurgu);color:var(--cu-vurgu-uzeri,#fff);font:600 11px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;writing-mode:vertical-rl;' +
            'opacity:.72;box-shadow:-2px 2px 8px rgba(var(--cu-vurgu-rgb),0.25);display:flex;align-items:center;gap:5px;transition:right .2s ease,opacity .15s,padding .15s}' +
            '#uyap-ispanel.acik ~ #uyap-issekme{right:min(380px,92vw);opacity:1}' +
            '#uyap-issekme:hover{opacity:1;padding:9px 6px;background:var(--cu-vurgu-koyu)}' +
            '#uyap-issekme .ip-rozet{writing-mode:horizontal-tb;min-width:16px;padding:0 4px;border-radius:999px;background:#fff;color:var(--cu-vurgu-koyu);font-size:10px;line-height:16px;text-align:center}' +
            '#uyap-issekme .ip-nokta{writing-mode:horizontal-tb;width:7px;height:7px;border-radius:50%;background:#4ade80}' +
            // Çekmece sekmeleri: İşler | Araçlar.
            '#uyap-ispanel .ip-sekmeler{display:flex;gap:4px;padding:0 14px;border-bottom:1px solid rgba(255,255,255,0.1)}' +
            '#uyap-ispanel .ip-sekme{padding:7px 12px;border:0;border-bottom:2px solid transparent;background:none;color:#9ca3af;cursor:pointer;font-size:13px;font-weight:600}' +
            '#uyap-ispanel .ip-sekme.sec{color:#fff;border-bottom-color:var(--cu-vurgu-parlak)}' +
            '#uyap-ispanel .ip-arac{display:block;width:100%;text-align:left;margin-top:8px;padding:10px 12px;border:1px solid rgba(255,255,255,0.1);border-radius:9px;background:rgba(255,255,255,0.05);color:#f3f4f6;cursor:pointer}' +
            '#uyap-ispanel .ip-arac:hover{background:rgba(255,255,255,0.11)}' +
            '#uyap-ispanel .ip-arac-ad{font-size:13px;font-weight:600}' +
            '#uyap-ispanel .ip-arac-not{margin-top:2px;font-size:11px;color:#9ca3af}' +
            '#uyap-ispanel .ip-arac-not.ok{color:#86efac}';
        document.head.appendChild(stil);

        const kok = el('div', '');
        kok.id = 'uyap-ispanel';
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;padding:14px 14px 6px;');
        ust.appendChild(el('div', 'font-size:15px;font-weight:700;', 'Çoklu UYAP'));
        const sag = el('div', 'display:flex;align-items:center;gap:12px;');
        const yenile = el('button', null, 'Yenile'); yenile.className = 'ip-link'; yenile.type = 'button';
        yenile.addEventListener('click', function() { panelYenile(); });
        pnl.yenile = yenile;
        const x = el('span', 'font-size:20px;color:#9ca3af;cursor:pointer;padding:0 2px;line-height:1;', '×');
        x.title = 'Kapat (Esc)';
        x.addEventListener('click', panelKapat);
        sag.appendChild(yenile); sag.appendChild(x);
        ust.appendChild(sag);
        kok.appendChild(ust);
        // Sekmeler: İşler (kişinin işleri) | Araçlar (İş Listesi, Gerekçeli Kararlar, E-İmza …).
        const sekmeler = el('div', '');
        sekmeler.className = 'ip-sekmeler';
        pnl.sekmeDugmeleri = {};
        [['isler', 'İşler'], ['araclar', 'Araçlar']].forEach(function(s) {
            const b = el('button', null, s[1]);
            b.type = 'button';
            b.className = 'ip-sekme';
            b.addEventListener('click', function() { gorunumSec(s[0]); });
            pnl.sekmeDugmeleri[s[0]] = b;
            sekmeler.appendChild(b);
        });
        kok.appendChild(sekmeler);
        pnl.islerKutu = el('div', 'flex:1;display:flex;flex-direction:column;min-height:0;');
        pnl.cipler = el('div', 'display:flex;flex-wrap:wrap;gap:6px;padding:8px 14px 8px;border-bottom:1px solid rgba(255,255,255,0.1);');
        pnl.islerKutu.appendChild(pnl.cipler);
        pnl.govde = el('div', 'flex:1;overflow-y:auto;overflow-x:hidden;padding:4px 14px 14px;');
        pnl.islerKutu.appendChild(pnl.govde);
        kok.appendChild(pnl.islerKutu);
        pnl.araclarKutu = el('div', 'flex:1;overflow:auto;padding:6px 14px 14px;display:none;');
        kok.appendChild(pnl.araclarKutu);
        document.body.appendChild(kok);
        pnl.kok = kok;

        const sekme = el('button', null);
        sekme.id = 'uyap-issekme';
        sekme.type = 'button';
        sekme.title = 'Çoklu UYAP: işlerim ve araçlar';
        pnl.rozet = el('span', 'display:none;');
        pnl.rozet.className = 'ip-rozet';
        sekme.appendChild(pnl.rozet);
        sekme.appendChild(el('span', null, 'Çoklu UYAP'));
        pnl.nokta = el('span', 'display:none;');       // yeşil: imzalı belge yüklemeye hazır
        pnl.nokta.className = 'ip-nokta';
        sekme.appendChild(pnl.nokta);
        sekme.addEventListener('click', function() { pnl.acik ? panelKapat() : panelAc(); });
        document.body.appendChild(sekme);
        pnl.sekme = sekme;

        document.addEventListener('keydown', function(ev) {
            if (ev.key === 'Escape' && pnl.acik && !(ort && ort.style.display !== 'none')) panelKapat();
        });
        gorunumSec('isler');
        cipleriCiz();
        panelYenile();
        setInterval(function() { if (!document.hidden) panelYenile(); }, 180000);
        sekmeYerlestir();
        window.addEventListener('resize', function() { sekmeYerlestir(true); });
        setInterval(function() { if (!document.hidden) { sekmeYerlestir(); noktaGuncelle(); } }, 2000);
    }

    // ── Tutamacın yeri: UYAP'ın sağ kenardaki kendi düğmelerini (ör. "Loglamayı Başlat" ekranın
    // ortasında, erişilebilirlik sağ altta — canlı, 2026-11-11) ÖRTMEYEN ilk boşluk. Tutamacın
    // sütununda örnek noktalara elementsFromPoint ile bakılır; bizim olmayan, küçük, sabit konumlu
    // bir öğe (ya da onun içi) çıkarsa o yükseklik doludur. UYAP düzenini değiştirse de çakışmaz.
    function yabanciSabitOge(e) {
        for (; e && e !== document.body && e !== document.documentElement; e = e.parentElement) {
            if (e.id && e.id.indexOf('uyap-') === 0) return false;           // bizim öğemiz
            const cs = getComputedStyle(e);
            if (cs.position === 'fixed') {
                const r = e.getBoundingClientRect();
                return r.width < innerWidth * 0.5 && r.height < innerHeight * 0.5;   // tam ekran örtüler değil
            }
        }
        return false;
    }
    function yerBosMu(x1, x2, ust, yuk) {
        for (let y = ust - 6; y <= ust + yuk + 6; y += 8) {
            for (const x of [x1, x2]) {
                if (document.elementsFromPoint(x, y).some(yabanciSabitOge)) return false;
            }
        }
        return true;
    }
    let sekmeOlcu = '';
    function sekmeYerlestir(zorla) {
        const s = pnl.sekme;
        if (!s || pnl.acik) return;
        const r = s.getBoundingClientRect();
        const yuk = Math.round(r.height) || 110;
        const x1 = Math.round(r.right) - 3, x2 = Math.round(r.right) - 14;
        const olcu = innerWidth + 'x' + innerHeight;
        if (!zorla && olcu === sekmeOlcu && yerBosMu(x1, x2, Math.round(r.top), yuk)) return;
        sekmeOlcu = olcu;
        const enUst = 72, enAlt = innerHeight - yuk - 8;     // UYAP üst çubuğu ~60 px
        const tercih = Math.max(enUst, Math.min(enAlt, Math.round(innerHeight * 0.18)));
        const adaylar = [];
        for (let y = tercih; y <= enAlt; y += 10) adaylar.push(y);
        for (let y = tercih - 10; y >= enUst; y -= 10) adaylar.push(y);
        const bos = adaylar.find(function(y) { return yerBosMu(x1, x2, y, yuk); });
        s.style.top = (bos != null ? bos : tercih) + 'px';
    }
    function noktaGuncelle() {
        const e = window.__uyapEimza && window.__uyapEimza.durum();
        if (pnl.nokta) pnl.nokta.style.display = e && e.hazir ? '' : 'none';
        if (pnl.gorunum === 'araclar' && pnl.acik) araclariCiz();
    }

    // ── Çekmece görünümü: İşler | Araçlar ──
    function gorunumSec(g) {
        pnl.gorunum = g;
        Object.keys(pnl.sekmeDugmeleri).forEach(function(k) { pnl.sekmeDugmeleri[k].classList.toggle('sec', k === g); });
        pnl.islerKutu.style.display = g === 'isler' ? 'flex' : 'none';
        pnl.araclarKutu.style.display = g === 'araclar' ? 'block' : 'none';
        pnl.yenile.style.display = g === 'isler' ? '' : 'none';
        if (g === 'araclar') araclariCiz();
    }
    function araclariCiz() {
        const k = pnl.araclarKutu;
        k.innerHTML = '';
        const arac = function(ad, not, fn, notOk) {
            const b = el('button', null);
            b.type = 'button';
            b.className = 'ip-arac';
            b.appendChild(el('div', null, ad)).className = 'ip-arac-ad';
            if (not) b.appendChild(el('div', null, not)).className = 'ip-arac-not' + (notOk ? ' ok' : '');
            b.addEventListener('click', fn);
            k.appendChild(b);
        };
        arac('İş Listesi', 'Bugün, takvim, onay bekleyenler ve büronun tüm işleri',
             function() { panelKapat(); ac(); });
        const g = window.__uyapGerekceli;
        if (g) {
            const d = g.durum();
            arac('Gerekçeli Kararlar', d || 'Hukuk dosyalarında gerekçeli kararları bul ve indir',
                 function() { panelKapat(); g.ac(); }, !!d);
        }
        const e = window.__uyapEimza;
        if (e) {
            const d = e.durum();
            arac('E-İmza', d.hazir ? 'İmzalı belge hazır: ' + d.ad + ' — UYAP\'ta "Dosya Ekle"ye tıklayınca eklenir'
                                   : 'Belgeyi seçin ya da sayfaya sürükleyin (.udf · .doc · .docx · .pdf)',
                 function() { panelKapat(); e.ac(); }, d.hazir);
        }
        // Ana sayfadan kaldırılan indirme kutusunun bağlantıları (enjekte/anasayfa.js).
        if (window.__uyapIndirmeler) {
            k.appendChild(el('div', 'margin-top:14px;font-size:11px;font-weight:700;letter-spacing:.04em;text-transform:uppercase;color:var(--cu-vurgu-acik);',
                             'Programlar ve indirmeler'));
            const yer = el('div', 'display:flex;flex-wrap:wrap;gap:6px;margin-top:6px;');
            window.__uyapIndirmeler().forEach(function(x) {
                const b = el('button', null, x.ad);
                b.type = 'button';
                b.className = 'ip-cip';
                b.addEventListener('click', function() { panelKapat(); x.ac(); });
                yer.appendChild(b);
            });
            k.appendChild(yer);
        }
    }

    function cipleriCiz() {
        pnl.cipler.innerHTML = '';
        const sec = [['benim', 'Benim'], ['onay', 'Onay bekleyen']];
        if (ben && ben.me && ben.me.role === 'admin') sec.push(['tumu', 'Tümü']);
        sec.forEach(function(s) {
            const b = el('button', null, s[1]);
            b.type = 'button';
            b.className = 'ip-cip' + (!pnl.dosya && pnl.filtre === s[0] ? ' sec' : '');
            b.addEventListener('click', function() {
                if (!pnl.dosya && pnl.filtre === s[0]) return;
                pnl.dosya = null;
                pnl.filtre = s[0];
                cipleriCiz();
                panelYenile();
            });
            pnl.cipler.appendChild(b);
        });
        if (pnl.dosya) {
            // Kartın "Yapılacak iş var" düğmesinden: yalnız o dosyanın işleri; × ile genel listeye dönülür.
            const b = el('button', 'max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;',
                         'Bu dosya: ' + pnl.dosya.ad + '  ×');
            b.type = 'button';
            b.className = 'ip-cip sec';
            b.title = 'Dosya süzgecini kaldır';
            b.addEventListener('click', function() { pnl.dosya = null; cipleriCiz(); panelYenile(); });
            pnl.cipler.appendChild(b);
            const yeni = el('button', null, '+ Bu dosyaya iş ata');
            yeni.type = 'button';
            yeni.className = 'ip-cip';
            yeni.addEventListener('click', function() {
                const ds = pnl.dosya;
                if (!ds) return;
                panelKapat();
                pencereAc(ds.d, ds.kart && ds.kart.isConnected ? ds.kart : null);
            });
            pnl.cipler.appendChild(yeni);
            if (window.__cuIsDosya) {
                const genis = el('button', null, 'Geniş ekranda');
                genis.type = 'button';
                genis.className = 'ip-cip';
                genis.title = 'Bu dosyanın işlerini İş Listesi sayfasında aç';
                genis.addEventListener('click', function() {
                    const ds = pnl.dosya;
                    if (!ds) return;
                    panelKapat();
                    window.__cuIsDosya({ dosya_no: String(ds.d.dosyaNo || '').trim(), birim_adi: ds.d.birimAdi || '',
                                         donus_kart: window.__uyapDosyaAnahtari ? window.__uyapDosyaAnahtari(ds.d) : null });
                });
                pnl.cipler.appendChild(genis);
            }
        }
    }

    // Paneli bir dosyanın işleriyle açar (kart düğmesi / birden çok işli belge).
    function panelDosyaAc(d, kart) {
        panelKur();
        if (!pnl.kok) return;
        pnl.dosya = { id: dosyaAnahtari(d), d: d, kart: kart || null, ad: (window.__uyapDosyaAdi ? window.__uyapDosyaAdi(d) : '') || String(d.dosyaNo || d.dosyaId) };
        gorunumSec('isler');
        cipleriCiz();
        panelAc();
    }

    function panelAc() {
        panelKur();
        pnl.acik = true;
        pnl.kok.classList.add('acik');
        panelYenile();
    }
    function panelKapat() {
        pnl.acik = false;
        if (pnl.kok) pnl.kok.classList.remove('acik');
    }

    async function panelYenile() {
        if (!pnl.kok || pnl.yukleniyor) return;
        pnl.yukleniyor = true;
        const dosya = pnl.dosya;
        const anahtar = function() { return pnl.dosya ? 'd:' + pnl.dosya.id : pnl.filtre; };
        const filtre = anahtar();
        let tekrar = false;
        try {
            if (!ben) { ben = await yeniBen(); cipleriCiz(); }
            let isler;
            if (dosya) isler = await dosyaninIsleri(dosya.d);
            else {
                // Çekmecedeki "işlerim" (benim → sorumlusu olduğum açık işler)
                const q = filtre === 'onay' ? 'liste?gorunum=onay'
                    : 'liste?' + new URLSearchParams({ gorunum: 'tum', durum: 'acik,devam,onayda', sorumlu: filtre === 'benim' ? String(ben.me.id) : '' }).toString();
                isler = (await yeniIstek(q)).map(eskiBicim);
            }
            if (dosya) dosyaIsleri[dosya.id] = { t: Date.now(), isler: isler };
            if (filtre !== anahtar()) { tekrar = true; return; }   // bu arada süzgeç değişti → yeniden
            pnl.isler = isler;
            if (filtre === 'benim') {
                const ak = pnl.isler.filter(function(t) { return !BITTI[t.status]; });
                const simdi = Date.now() / 1000;
                rozet(ak.length, ak.some(function(t) { return t.due_at && t.due_at < simdi && t.status !== 'onayda'; }));
            }
            listeCiz();
        } catch (e) {
            rozet(0);
            if (pnl.acik) {
                pnl.govde.innerHTML = '';
                pnl.govde.appendChild(el('div', 'margin-top:12px;color:#fca5a5;', e.message));
            }
        } finally {
            pnl.yukleniyor = false;
            if (tekrar) panelYenile();
        }
    }

    // Gecikmiş işi varsa sayı kırmızı.
    function rozet(n, gecikmis) {
        pnl.rozet.textContent = n > 99 ? '99+' : String(n);
        pnl.rozet.style.display = n ? '' : 'none';
        pnl.rozet.style.background = gecikmis ? '#dc2626' : '';
        pnl.rozet.style.color = gecikmis ? '#fff' : '';
        pnl.sekme.title = gecikmis ? 'Çoklu UYAP — gecikmiş iş var' : 'Çoklu UYAP: işlerim ve araçlar';
    }

    // Sıra: Acil önce, sonra son tarihi yakın olan; tarihsizler sonda.
    function oncelikDegeri(t) { return t.priority == null ? 1 : t.priority; }   // 0 = Düşük (|| 1 onu yutardı)
    function sirala(a, b) {
        if (oncelikDegeri(b) !== oncelikDegeri(a)) return oncelikDegeri(b) - oncelikDegeri(a);
        return (a.due_at || Infinity) - (b.due_at || Infinity) || a.id - b.id;
    }

    function listeCiz() {
        const g = pnl.govde;
        g.innerHTML = '';
        const simdi = Date.now() / 1000;
        const aktif = pnl.isler.filter(function(t) { return !BITTI[t.status]; }).sort(sirala);
        const biten = pnl.isler.filter(function(t) { return BITTI[t.status]; });
        if (!aktif.length) {
            g.appendChild(el('div', 'margin-top:12px;color:#9ca3af;',
                pnl.dosya ? 'Bu dosyada bitmemiş iş yok.' : pnl.filtre === 'onay' ? 'Onay bekleyen iş yok.' : 'Bitmemiş iş yok.'));
        }
        // Bugün özeti: gecikmiş / bugün / sonra / onayda grupları (onay süzgecinde hepsi onayda: grupsuz).
        const gun = new Date(); gun.setHours(23, 59, 59, 999);
        const gunSonu = gun.getTime() / 1000;
        const grup = function(t) {
            if (t.status === 'onayda') return 'onayda';
            if (t.due_at && t.due_at < simdi) return 'gecikmis';
            if (t.due_at && t.due_at <= gunSonu) return 'bugun';
            return 'sonra';
        };
        const GRUPLAR = [['gecikmis', 'Gecikmiş', '#fca5a5'], ['bugun', 'Bugün', '#fdba74'],
                         ['sonra', 'Sonra', '#cbd5e1'], ['onayda', 'Onay bekliyor', 'var(--cu-vurgu-acik)']];
        const sayilar = {};
        aktif.forEach(function(t) { sayilar[grup(t)] = (sayilar[grup(t)] || 0) + 1; });
        const grupla = pnl.filtre !== 'onay' || pnl.dosya;
        if (grupla && aktif.length) {
            const ozet = el('div', 'display:flex;flex-wrap:wrap;gap:6px 12px;margin-top:8px;font-weight:600;');
            GRUPLAR.forEach(function(x) {
                if (sayilar[x[0]]) ozet.appendChild(el('span', 'color:' + x[2] + ';', x[1] + ': ' + sayilar[x[0]]));
            });
            g.appendChild(ozet);
            GRUPLAR.forEach(function(x) {
                const l = aktif.filter(function(t) { return grup(t) === x[0]; });
                if (!l.length) return;
                g.appendChild(el('div', 'margin-top:12px;font-size:11px;font-weight:700;letter-spacing:.04em;text-transform:uppercase;color:' + x[2] + ';',
                                 x[1] + ' (' + l.length + ')'));
                l.forEach(function(t) { g.appendChild(isSatiri(t, simdi)); });
            });
        } else {
            aktif.forEach(function(t) { g.appendChild(isSatiri(t, simdi)); });
        }
        (pnl.bittiGoster ? biten : []).forEach(function(t) { g.appendChild(isSatiri(t, simdi)); });
        if (biten.length) {
            const b = el('button', 'margin-top:10px;', pnl.bittiGoster ? 'Bitmiş işleri gizle' : 'Bitmiş ' + biten.length + ' işi göster');
            b.type = 'button';
            b.className = 'ip-link';
            b.addEventListener('click', function() { pnl.bittiGoster = !pnl.bittiGoster; listeCiz(); });
            g.appendChild(b);
        }
    }

    function isSatiri(t, simdi) {
        const s = el('div', BITTI[t.status] ? 'opacity:0.55;' : '');
        s.className = 'ip-is' + (t.priority === 2 ? ' acil' : '');
        s.title = 'İş Listesi\'nde aç';
        s.addEventListener('click', function() { isAc(t.id); });
        s.appendChild(el('div', 'font-weight:600;word-break:break-word;', (t.priority === 2 ? 'Acil · ' : '') + t.title));
        const gecikti = t.due_at && t.due_at < simdi && !BITTI[t.status] && t.status !== 'onayda';
        const kim = (t.assignees && t.assignees.length ? t.assignees : [t.assignee_id]).filter(Boolean).map(kisiAdi).join(', ');
        const alt = t.subtasks && t.subtasks.length
            ? t.subtasks.filter(function(x) { return BITTI[x.status]; }).length + '/' + t.subtasks.length + ' alt iş' : '';
        const meta = el('div', 'color:#cbd5e1;');
        meta.appendChild(document.createTextNode([DURUM[t.status] || t.status, kim, alt].filter(Boolean).join(' · ')));
        if (t.due_at) {
            meta.appendChild(document.createTextNode(' · '));
            meta.appendChild(el('span', gecikti ? 'color:#fca5a5;font-weight:600;' : '',
                (gecikti ? 'Gecikti: ' : 'Son: ') + tarih(t.due_at)));
        }
        s.appendChild(meta);
        if (t.taraf_adi) s.appendChild(el('div', 'color:#cbd5e1;word-break:break-word;', 'Taraf: ' + t.taraf_adi));
        if (t.evrak_id) s.appendChild(el('div', 'color:var(--cu-vurgu-acik);word-break:break-word;', 'Belge: ' + (t.evrak_adi || t.evrak_id)));
        if (t.barkod) s.appendChild(el('div', 'color:#cbd5e1;', 'Barkod: ' + t.barkod));
        const alt2 = el('div', 'display:flex;align-items:center;gap:6px;min-width:0;');
        const pb = pttDugmesi(t, 'ip-aktar');
        if (pb) alt2.appendChild(pb);
        if (t.dosya_id) {
            const d = el('button', null, [t.birim_adi, t.dosya_no].filter(Boolean).join(' ') || 'UYAP dosyası');
            d.type = 'button';
            d.className = 'ip-dosya';
            d.title = 'Dosya kartını aç' + (t.dosya_tur ? ' · ' + t.dosya_tur : '');
            d.addEventListener('click', function(ev) { ev.stopPropagation(); dosyaKartiAc(t); });
            alt2.appendChild(d);
        }
        const kb = kapanisDugmesi(t, 'ip-onay');
        if (kb) alt2.appendChild(kb);
        const ab = aktarDugmesi(t, 'ip-aktar');
        if (ab) alt2.appendChild(ab);
        const hb = hatirlaticiDugmesi(t, 'ip-aktar');
        if (hb) alt2.appendChild(hb);
        if (alt2.children.length) s.appendChild(alt2);
        if (onayYetkili(t)) {
            if (t.status === 'onayda') s.appendChild(el('div', 'margin-top:5px;color:var(--cu-vurgu-acik);word-break:break-word;', kararOzeti(t)));
            const kd = kararDugmeleri(t, 'ip-onay', 'ip-red');
            if (kd.length) {
                const yer = el('div', 'display:flex;flex-wrap:wrap;align-items:center;gap:6px;');
                kd.forEach(function(b) { yer.appendChild(b); });
                s.appendChild(yer);
            }
        }
        return s;
    }

    // Kartı bu sayfada UYAP'ın kendi arama bileşeniyle açar; ana sayfada değilsek kaydı bırakıp
    // ana sayfaya geçer (bekleyenDosyaAc orada açar — İş Listesi'nden "UYAP'ta aç" ile aynı yol).
    function isKarti(t) {
        let kart = null;
        try { kart = t.dosya_uyap ? JSON.parse(t.dosya_uyap) : null; } catch (e) {}
        if (!kart || !kart.dosyaId) {
            kart = { dosyaId: t.dosya_id, dosyaNo: t.dosya_no || '', birimId: t.birim_id || '',
                     birimAdi: t.birim_adi || '', dosyaTurKodAciklama: t.dosya_tur || '' };
        }
        return kart;
    }
    function dosyaKartiAc(t) {
        const kart = isKarti(t);
        // Her durumda bekleyenDosyaAc yolundan: kimlik önce UYAP'tan tazelenir (bkz. tazeKart).
        try { sessionStorage.setItem('cokluuyapDosyaAc', JSON.stringify({ t: Date.now(), dosya: kart })); } catch (e) {}
        if (document.getElementById('searchId')) { panelKapat(); bekleyenDosyaAc(); }
        else anaSayfayaGit();
    }
    // UYAP ana sayfası: dışarıdan girişte "/" sitenin kabuğudur (tünele girmez), UYAP'a /giris ile dönülür.
    function anaSayfayaGit() { location.assign(UZAK ? '/giris' : '/'); }

    // ── Dosya kartında işler: "Yapılacak iş var (N)" düğmesi + işli belgeler mor ──
    // Açık kartın dosyasının işleri köprüden alınır ve DOSYA_TAZE ms saklanır; iş açılınca / panel
    // o dosyayı gösterince tazelenir. Yalnız bitmemiş işler sayılır ve belgeyi işaretler.
    const DOSYA_TAZE = 60000;
    const dosyaIsleri = {};       // dosyaAnahtari -> {t, isler, yukleniyor}
    function dosyaIsleriniGetir(d) {
        const a = dosyaAnahtari(d);
        const k = dosyaIsleri[a];
        if (k && (k.yukleniyor || Date.now() - k.t < DOSYA_TAZE)) return k;
        const yeni = { t: k ? k.t : 0, isler: k ? k.isler : [], yukleniyor: true };
        dosyaIsleri[a] = yeni;
        // Kişi bilgisi bantta "Onaya gönder" düğmesi için (işe atanan mıyım?).
        Promise.all([dosyaninIsleri(d), beniGetir().catch(function() { return null; })])
            .then(function(r) { dosyaIsleri[a] = { t: Date.now(), isler: r[0] }; })
            .catch(function() { dosyaIsleri[a] = { t: Date.now(), isler: yeni.isler }; });  // köprü kapalı: sessiz
        return yeni;
    }
    function acikIsler(d) {
        const k = dosyaIsleri[dosyaAnahtari(d)];
        return k ? k.isler.filter(function(t) { return !BITTI[t.status]; }) : [];
    }

    function kartIsleriniIsle(kart, baslik, d) {
        dosyaIsleriniGetir(d);
        const isler = acikIsler(d);

        // Kart başlığındaki "İşler" düğmesinin rozeti: bitmemiş iş sayısı (yoksa gizli).
        const b = baslik.querySelector('[data-isata]');
        const rz = b && b.querySelector('[data-rozet]');
        if (rz) {
            const yeni = isler.length ? String(isler.length) : '';
            if (rz.textContent !== yeni) rz.textContent = yeni;
            rz.style.display = isler.length ? '' : 'none';
            b.title = isler.length ? 'Bu dosyanın ' + isler.length + ' bitmemiş işi — göster / yeni iş ata'
                                   : 'Bu dosyaya iş ata (İş Listesi)';
        }

        uyariBandi(kart, d, isler);
        belgeleriIsaretle(kart, d, isler);
        if (evrakSekmesiIstendi && Date.now() - evrakSekmesiIstendi < 20000 && sekmeSec(kart, 'Evrak')) evrakSekmesiIstendi = 0;
    }

    // Kartın sekmeleri (Dosya Bilgileri, Evrak, …): DevExtreme [role=tab]; metin iki kez yazılı olabilir.
    function sekmeAdi(t) { return t.textContent.trim().replace(/^(.+)\1$/, '$1'); }
    function seciliSekme(kart) {
        const t = kart.querySelector('[role=tab][aria-selected=true]');
        return t ? sekmeAdi(t) : '';
    }
    function sekmeSec(kart, ad) {
        const t = Array.from(kart.querySelectorAll('[role=tab]')).find(function(x) { return sekmeAdi(x) === ad; });
        if (!t) return false;
        if (t.getAttribute('aria-selected') !== 'true') t.click();
        return true;
    }
    // İş Listesi'nden (panel / web "UYAP'ta aç") açılan kart Evrak sekmesiyle gelsin.
    let evrakSekmesiIstendi = 0;

    // "Dosya Bilgileri" ve "Evrak" sekmelerinde, sekme başlıklarının hemen altında mor uyarı bandı.
    const UYARI_SEKMELERI = { 'Dosya Bilgileri': 1, 'Evrak': 1 };
    function uyariBandi(kart, d, isler) {
        const tp = kart.querySelector('.dx-tabpanel');
        const kap = tp && tp.querySelector(':scope > .dx-tabpanel-container');
        if (!kap) return;
        let band = tp.querySelector(':scope > [data-isuyari]');
        if (!band) {
            band = document.createElement('div');
            band.setAttribute('data-isuyari', '1');
            band.className = 'uyap-is-uyari';
            tp.insertBefore(band, kap);
        }
        const goster = isler.length > 0 && !!UYARI_SEKMELERI[seciliSekme(kart)];
        band.style.display = goster ? '' : 'none';
        if (!goster) return;
        const imza = (ben ? ben.me.id : '') + '#' + isler.map(function(t) { return t.id + ':' + t.status + ':' + t.title + ':' + (t.ext_pending ? 1 : 0); }).join('|');
        if (band.getAttribute('data-imza') === imza) return;
        band.setAttribute('data-imza', imza);
        band.innerHTML = '';
        const sol = el('div', 'flex:1;min-width:0;');
        sol.appendChild(el('div', 'font-weight:700;', 'Bu dosyada ' + isler.length + ' yapılacak iş var'));
        const liste = el('div', 'margin-top:2px;');
        isler.slice().sort(sirala).slice(0, 3).forEach(function(t) {
            const satir = el('div', null);
            satir.className = 'uyap-is-uyari-satir';
            const a = el('span', null, (t.priority === 2 ? 'Acil · ' : '') + t.title + (t.evrak_adi ? ' (belge: ' + t.evrak_adi + ')' : ''));
            a.className = 'uyap-is-uyari-is';
            a.title = 'İş Listesi\'nde aç';
            a.addEventListener('click', function(ev) { ev.stopPropagation(); isAc(t.id); });
            satir.appendChild(a);
            if (t.status === 'onayda') satir.appendChild(el('span', 'font-size:11px;opacity:.8;', 'onayda'));
            const pb = pttDugmesi(t, 'uyap-is-uyari-kucuk');
            if (pb) satir.appendChild(pb);
            const kb = kapanisDugmesi(t, 'uyap-is-uyari-kucuk');
            if (kb) satir.appendChild(kb);
            const ab = aktarDugmesi(t, 'uyap-is-uyari-kucuk');
            if (ab) satir.appendChild(ab);
            kararDugmeleri(t, 'uyap-is-uyari-kucuk', 'uyap-is-uyari-kucuk').forEach(function(b) { satir.appendChild(b); });
            liste.appendChild(satir);
        });
        if (isler.length > 3) liste.appendChild(el('div', 'opacity:.8;', '+' + (isler.length - 3) + ' iş daha'));
        sol.appendChild(liste);
        band.appendChild(sol);
        const b = el('button', null, isler.length === 1 ? 'İşi aç' : 'İşleri göster');
        b.type = 'button';
        b.className = 'uyap-is-uyari-dugme';
        b.addEventListener('click', function(ev) {
            ev.stopPropagation();
            if (isler.length === 1) isAc(isler[0].id); else panelDosyaAc(d);
        });
        band.appendChild(b);
    }

    // Evrak ağacı: bitmemiş işi olan belgenin yazısı mor + yanında işe giden düğme. İşi olmayan
    // belgede satırın üstüne gelince "+ İş" çıkar (o belgeye iş açar).
    function belgeleriIsaretle(kart, d, isler) {
        const A = window.__uyapEvrakAgaci;
        const tv = A && kart.querySelector('.evrak-treeview');
        const koklar = tv && A.veri(tv);
        if (!koklar) return;
        const harita = A.harita(koklar);
        const belgeIsleri = {};
        isler.forEach(function(t) { if (t.evrak_id) (belgeIsleri[String(t.evrak_id)] = belgeIsleri[String(t.evrak_id)] || []).push(t); });
        tv.querySelectorAll('.dx-treeview-node').forEach(function(node) {
            const g = harita[node.getAttribute('data-item-id')];
            const satir = node.querySelector(':scope > .dx-treeview-item');
            if (!g || !satir || !g.x.evrak || !g.x.evrak.evrakId) return;
            const evrakId = belgeAnahtari(A.ozet(g.x, g.ebeveyn));
            const ilgili = belgeIsleri[evrakId] || [];
            satir.classList.toggle('uyap-isli-belge', ilgili.length > 0);
            let git = satir.querySelector(':scope > [data-isgit]');
            if (!git) {
                git = document.createElement('button');
                git.type = 'button';
                git.setAttribute('data-isgit', '1');
                git.className = 'uyap-belge-is';
                ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'dblclick', 'touchstart', 'touchend', 'keydown']
                    .forEach(function(tur) { git.addEventListener(tur, A.durdur); });
                git.addEventListener('click', function(ev) {
                    ev.stopPropagation();
                    ev.preventDefault();
                    const isNolar = (git.getAttribute('data-isler') || '').split(',').filter(Boolean).map(Number);
                    const k = git.closest('.dosya-sorgula-popup');
                    const dd = k && window.__uyapKartVerisi(k);
                    if (isNolar.length === 1) isAc(isNolar[0]);
                    else if (isNolar.length > 1 && dd) panelDosyaAc(dd);
                    else if (dd) pencereAc(dd, k, git.getAttribute('data-evrak'));
                });
                satir.appendChild(git);
            }
            const isNolar = ilgili.map(function(t) { return t.id; }).join(',');
            if (git.getAttribute('data-isler') !== isNolar || git.getAttribute('data-evrak') !== evrakId) {
                git.setAttribute('data-isler', isNolar);
                git.setAttribute('data-evrak', evrakId);
                git.classList.toggle('bos', !ilgili.length);
                git.textContent = !ilgili.length ? '+ İş' : ilgili.length === 1 ? 'İş →' : ilgili.length + ' iş →';
                git.title = !ilgili.length ? 'Bu belgeye iş ekle'
                    : ilgili.map(function(t) { return t.title; }).join('\n');
            }
        });
    }

    // ── UYAP ana sayfası "Bildirimler": dosyaya ilişkin bildirimden tek tıkla iş ──
    // Bildirim yalnız birim adı + dosya no taşır ("İzmir 15. İcra Dairesi Biriminde Bulunan 2025/2025
    // İcra Dosyası Borç Tahsilatı Yapılmıştır."). Birim kimliği avukatın dosya birimleri listesinden
    // (avukat_dosya_birimler.ajx), güncel dosya kimliği Dosya Sorgulama'dan (tazeKart) bulunur.
    const BILDIRIM_RE = /^(.+?) Biriminde Bulunan (\d{4}\/\d+) /;
    let birimHaritasi = null;
    async function birimIdBul(ad) {
        if (!birimHaritasi) {
            const l = await uyapPost('avukat_dosya_birimler.ajx', {});
            const h = {};
            (Array.isArray(l) ? l : []).forEach(function(b) { if (b && b.birimAdi && b.birimId) h[kucukHarf(b.birimAdi)] = String(b.birimId); });
            if (Object.keys(h).length) birimHaritasi = h;
            else return null;
        }
        return birimHaritasi[kucukHarf(ad)] || null;
    }
    function bildirimDugmeleri() {
        document.querySelectorAll('.main-page-notifications li.notify-item:not([data-isbildirim])').forEach(function(li) {
            const metinEl = li.querySelector('.icon-list-text');
            const metin = metinEl ? (metinEl.getAttribute('aria-label') || metinEl.textContent || '').trim() : '';
            if (!metin) return;                       // henüz çizilmedi: sonraki turda
            li.setAttribute('data-isbildirim', '1');
            const m = BILDIRIM_RE.exec(metin);
            if (!m) return;
            const bas = li.querySelector('.icon-list-baslik');
            // Başlığın sonundaki " -" ayıracı düğmeyle yan yana "… - + İş" gibi okunuyordu (A-5): kaldırılır.
            if (bas) {
                const son = Array.from(bas.childNodes).reverse().find(function(n) { return n.nodeType === 3 && n.nodeValue.trim(); });
                if (son && /\s*-\s*$/.test(son.nodeValue)) son.nodeValue = son.nodeValue.replace(/\s*-\s*$/, '');
            }
            const b = el('button', null, 'İş ata');
            b.type = 'button';
            b.className = 'uyap-bildirim-is';
            b.title = 'Bu bildirimin dosyasına iş ata';
            b.addEventListener('click', function(ev) {
                ev.stopPropagation();
                ev.preventDefault();
                bildirimdenIs(m[1].trim(), m[2], bildirimBasligi(bas), metin);
            });
            (bas || li).appendChild(b);
        });
    }
    function bildirimBasligi(bas) {
        if (!bas) return '';
        return Array.from(bas.childNodes).filter(function(n) { return !(n.classList && n.classList.contains('uyap-bildirim-is')); })
            .map(function(n) { return n.textContent; }).join('').replace(/\s*-\s*$/, '').trim();
    }
    async function bildirimdenIs(birimAdi, dosyaNo, baslik, metin) {
        kur();
        kutu.replaceChildren(el('div', 'color:#cbd5e1;', 'Dosya bulunuyor: ' + birimAdi + ' ' + dosyaNo + '…'));
        ort.style.display = 'flex';
        try {
            const birimId = await birimIdBul(birimAdi);
            if (!birimId) throw new Error('Birim bulunamadı: ' + birimAdi);
            const d = await tazeKart({ birimId: birimId, dosyaNo: dosyaNo, birimAdi: birimAdi,
                                       birimTuru3: /icra dairesi/i.test(birimAdi) ? '1101' : undefined });
            if (!d) throw new Error('Dosya UYAP\'ta bulunamadı: ' + birimAdi + ' ' + dosyaNo);
            const sablon = /tahsilat/i.test(baslik) ? 'Tahsilat kontrol' : /reddiyat/i.test(baslik) ? 'Reddiyat kontrol' : '';
            pencereAc(d, null, null, { sablon: sablon, baslik: sablon ? '' : baslik, aciklama: metin });
        } catch (e) {
            kutu.replaceChildren(el('div', 'color:#fca5a5;', e.message), dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
        }
    }

    // ── Kart düğmesi + sabit "İş Listesi" düğmesi ──
    function kartlariTara() {
        bekleyenDosyaAc();
        if (document.body) { kur(); bildirimDugmeleri(); sagMenuKur(); }   // sağ tık / çoklu pano: kart olmayan sayfalarda da
        if (!window.__uyapKartVerisi) return;
        kur();
        document.querySelectorAll('.dosya-sorgula-popup .dx-popup-title').forEach(function(baslik) {
            if (!baslik.querySelector('[data-isata]')) isAtaDugmesi(baslik);
            const kart = baslik.closest('.dosya-sorgula-popup');
            const d = kart && baslik.querySelector('[data-isata]') && window.__uyapKartVerisi(kart);
            if (d) kartIsleriniIsle(kart, baslik, d);
        });
        // 2026-11-11: sol alttaki sabit "İş Listesi" düğmesi kaldırıldı; İş Listesi çekmeceden
        // ("Tam sayfa" ya da Araçlar → İş Listesi) açılır.
        panelKur();
        sagMenuKur();
    }
    // UYAP kartının kendi kapatma düğmesi (başlıktaki kırmızı "icon-x", canlı 2026-11-11). Yalnız kartın KENDİ
    // başlığında aranır: bazı sekmeler (ör. Taraf Bilgileri, Safahat) açılınca kartın içine gizli bir iç pencere
    // gelir; onun başlığındaki .dx-closebutton kartın düğmesi sanılıyor, tıklanınca hiçbir şey olmuyordu —
    // küçült (–) ve görev çubuğundan kapatma sessizce çalışmıyordu (canlı, 2026-11-11).
    function uyapKartiniKapat(kart) {
        const bas = kart.querySelector(':scope > .dx-overlay-content > .dx-popup-title') || kart.querySelector('.dx-popup-title');
        if (!bas) return false;
        const aday = Array.from(bas.querySelectorAll('[role=button]')).find(function(b) {
                return !b.textContent.trim() && b.querySelector('.icon-x, .dx-icon-close');
            }) || bas.querySelector('.dx-closebutton');
        if (aday) { aday.click(); return true; }
        return false;
    }
    function isAtaDugmesi(baslik) {
        const yer = baslik.querySelector('[data-tumevrak]')
            || (window.__uyapBaslikDayanagi && window.__uyapBaslikDayanagi(baslik));
        if (!yer || !yer.parentElement) return;
        const b = document.createElement('div');
        b.setAttribute('data-isata', '1');
        b.setAttribute('role', 'button');
        b.setAttribute('tabindex', '0');
        b.title = 'Bu dosyaya iş ata (İş Listesi)';
        b.className = 'dx-widget dx-button dx-button-mode-outlined dx-button-normal dx-button-has-text dx-button-has-icon adalet-button';
        b.style.marginRight = '6px';
        // 2026-11-11: "Yapılacak iş var (N)" + "İş Ata" tek düğme: "İşler" (+ bitmemiş iş rozeti).
        // Açık iş varsa çekmecede bu dosyanın işleri (oradan "Yeni iş"), yoksa doğrudan İş Ata.
        b.innerHTML = '<div class="dx-button-content"><span class="dx-icon fe icon-check-circle"></span> <span>İşler</span>' +
            '<span data-rozet style="display:none;margin-left:6px;min-width:18px;padding:0 6px;border-radius:999px;background:var(--cu-vurgu);color:var(--cu-vurgu-uzeri,#fff);font-size:11px;font-weight:700;line-height:18px;text-align:center;"></span></div>';
        b.addEventListener('click', function(ev) {
            ev.stopPropagation();
            const kart = baslik.closest('.dosya-sorgula-popup');
            const d = kart && window.__uyapKartVerisi(kart);
            if (!d) {
                kur();
                kutu.replaceChildren(el('div', 'color:#fca5a5;', 'Bu kartın dosya bilgisi okunamadı; kartı kapatıp yeniden açın.'),
                                     dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
                ort.style.display = 'flex';
                return;
            }
            if (acikIsler(d).length) panelDosyaAc(d, kart); else pencereAc(d, kart);
        });
        yer.parentElement.insertBefore(b, yer);
    }
    // ── Sağ tık menüsü ──
    // UYAP'ta tarayıcının uzun menüsü yerine sade CokluUyap menüsü — dosya türüne göre (DOSYA_TURLERI).
    // Menüsü tanımlı olmayan yerde, yazı alanlarında (yapıştır, yazım denetimi), kendi pencerelerimizde
    // ve Shift / Ctrl + sağ tıkta tarayıcının menüsü açılır. Kartta metin seçiliyse menünün başında
    // "Seçili metni kopyala" olur.
    // Kapatmak için: localStorage cokluuyapSagMenu = 'kapali'.
    let sagMenu = null, sagMenuSecili = -1;
    function sagMenuKapat() {
        if (sagMenu && sagMenu.style.display !== 'none') { sagMenu.style.display = 'none'; sagMenuSecili = -1; }
    }
    function sagMenuKapali() {
        try { return localStorage.getItem('cokluuyapSagMenu') === 'kapali'; } catch (e) { return false; }
    }
    function kisaBildirim(m) {
        const b = el('div', 'position:fixed;left:50%;bottom:40px;transform:translateX(-50%);z-index:2147483647;padding:6px 14px;border-radius:8px;background:rgba(var(--cu-panel-rgb),0.95);color:#e5e7eb;font:12px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;', m);
        b.className = 'uyap-kisa-bildirim';
        document.body.appendChild(b);
        setTimeout(function() { b.remove(); }, 1200);
    }
    let kendiKopyam = false;   // panoya()'nın execCommand yedeği: UYAP'ın copy engeline takılmasın
    let sonKopya = '';         // çoklu pano: seçim yokken Ctrl+Shift+N / sağ tık bunu kaydeder
    async function panoya(metin, sessiz) {
        sonKopya = String(metin || '').slice(0, 20000);
        try {
            await navigator.clipboard.writeText(metin);
        } catch (e) {
            const ta = el('textarea', 'position:fixed;left:-9999px;top:0;');
            ta.value = metin;
            document.body.appendChild(ta);
            ta.select();
            kendiKopyam = true;
            try { document.execCommand('copy'); } finally { kendiKopyam = false; ta.remove(); }
        }
        if (!sessiz) kisaBildirim('Kopyalandı');
    }
    // ── Belge görüntüleyicide seçili metin ──
    // UYAP'ın kartındaki belge görüntüleyicisi metin seçtiriyor ama kopyalatmıyor: Ctrl+C sonuçsuz,
    // sağ tıkta seçim de kayboluyordu. Dinleyiciler betik <head>'in başında kurulduğundan UYAP'ınkilerden
    // önce çalışır: kartta seçim varken Ctrl+C / copy olayı UYAP'a ulaşmadan biz kopyalarız; sağ tık
    // anındaki seçim mousedown'da saklanır (UYAP temizlese de menüde "Seçili metni kopyala" çıkar).
    let sagTikSecimi = '';
    function seciliMetin() {
        try { return String(window.getSelection ? window.getSelection() : '').trim(); } catch (e) { return ''; }
    }
    function secimKartta() {
        try {
            const s = window.getSelection && window.getSelection();
            const n = s && s.rangeCount ? s.anchorNode : null;
            const e = n && (n.nodeType === 1 ? n : n.parentElement);
            return !!(e && e.closest && e.closest('.dosya-sorgula-popup'));
        } catch (e) { return false; }
    }
    function yaziAlaninda(e) {
        return !!(e && e.closest && e.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"]'));
    }
    // UDF belgeleri UYAP'ın canvas'a çizen kendi editöründe gösteriliyor: seçim tarayıcının değil
    // editörün durumunda (zustand deposu; eylemi getSelectedText(), UYAP main.js 2026-11-10). Depoya
    // tıklanan yerin React ağacından ulaşılır: ata bileşenlerin props'u, kancaları ve context'i taranır.
    // Görüntüleyicinin deposu bileşene useStore prop'u / context değeri olarak geçer ve zustand'ın
    // create()'i bir İŞLEV döndürür (getState onun özelliği): işlevler de taranmalı. useMemo kancasının
    // değeri [depo, bağımlılıklar] dizisidir.
    function secimOkuyucu(o) {
        if (Array.isArray(o)) o = o[0];
        if (!o || (typeof o !== 'object' && typeof o !== 'function')) return null;
        try {
            if (typeof o.getSelectedText === 'function') return o.getSelectedText;
            if (typeof o.getState === 'function' && o.getState.length === 0) {
                const s = o.getState();
                if (s && typeof s.getSelectedText === 'function') return s.getSelectedText;
            }
        } catch (e) {}
        return null;
    }
    function belgeSecimi(h) {
        try {
            let e = h && h.nodeType === 1 ? h : (h && h.parentElement);
            if (!e || !e.closest || !e.closest('.dosya-sorgula-popup')) return '';
            let a = null;
            for (; e && e.nodeType === 1; e = e.parentElement) {
                a = Object.keys(e).find(function(k) { return k.indexOf('__reactFiber') === 0; });
                if (a) break;
            }
            for (let f = a ? e[a] : null, i = 0; f && i < 120; i++, f = f.return) {
                const adaylar = [];
                const p = f.memoizedProps;
                if (p && typeof p === 'object') for (const k in p) adaylar.push(p[k]);
                if (f.tag === 0 || f.tag === 11 || f.tag === 15) {   // işlev bileşeni: kancalar
                    for (let k = f.memoizedState, n = 0; k && n < 150; k = k.next, n++) {
                        const v = k.memoizedState;
                        adaylar.push(v);
                        if (v && typeof v === 'object' && 'current' in v) adaylar.push(v.current);
                    }
                }
                for (let c = f.dependencies && f.dependencies.firstContext, n = 0; c && n < 30; c = c.next, n++) adaylar.push(c.memoizedValue);
                for (let j = 0; j < adaylar.length; j++) {
                    const fn = secimOkuyucu(adaylar[j]);
                    if (!fn) continue;
                    const m = fn();
                    if (typeof m === 'string') return m.trim();
                }
            }
        } catch (e) {}
        return '';
    }
    let sonBelgeHedefi = null;   // Ctrl+C için: kartta en son tıklanan yer
    window.addEventListener('mousedown', function(ev) {
        sonBelgeHedefi = ev.target && ev.target.closest && ev.target.closest('.dosya-sorgula-popup') ? ev.target : null;
        if (ev.button === 2) sagTikSecimi = seciliMetin() || belgeSecimi(ev.target);
    }, true);
    window.addEventListener('keydown', function(ev) {
        if (!(ev.ctrlKey || ev.metaKey) || ev.altKey || ev.shiftKey) return;
        if (!(ev.code === 'KeyC' || String(ev.key).toLowerCase() === 'c' || ev.key === 'Insert')) return;
        const ae = document.activeElement;     // gerçek bir alanda metin seçiliyse dokunma (editörün gizli
        if (yaziAlaninda(ae) && (ae.selectionStart !== ae.selectionEnd || !sonBelgeHedefi)) return;   // girdisi seçimsiz)
        if (secimKartta()) {
            const m = seciliMetin();
            if (!m) return;
            ev.stopImmediatePropagation();     // UYAP'ın tuş engeli çalışmasın; tarayıcının kopyalaması sürer
            panoya(m, true);                   // yedek: tarayıcı kopyalaması yine de engellenirse
            return;
        }
        if (seciliMetin()) return;             // kart dışında seçim: tarayıcıya bırak
        const m = sonBelgeHedefi && sonBelgeHedefi.isConnected ? belgeSecimi(sonBelgeHedefi) : '';
        if (m) panoya(m);                      // canvas editörü: seçimi depodan alıp biz kopyalarız
    }, true);
    window.addEventListener('copy', function(ev) {
        if (kendiKopyam) { ev.stopImmediatePropagation(); return; }
        if (!secimKartta()) return;
        const m = seciliMetin();
        if (!m || !ev.clipboardData) return;
        sonKopya = m.slice(0, 20000);
        ev.stopImmediatePropagation();
        ev.clipboardData.setData('text/plain', m);
        ev.preventDefault();
    }, true);
    // ── Dosya türleri: her türün kendi sağ tık menüsü ──
    // Tür, kartın dosya türü açıklamasından (dosyaTurKodAciklama), yoksa birim adından bulunur; sıra
    // önemlidir (ilk eşleşen). menu(c) türün menüsünü verir: [[ [metin, fn], … ], …] (gruplar arasına
    // çizgi; fn'siz satır başlıktır). c = {d: kart verisi, kart}. menu'süz türde ortak kart menüsü (kartMenusu:
    // dosya bilgisi / dosya numarası kopyala) açılır; icra buna 121'i ekler.
    const DOSYA_TURLERI = [
        { anahtar: 'arabuluculuk', eslesir: /arabulucu/, menu: null },
        { anahtar: 'icra', eslesir: /icra dosya|icra dairesi|icra müdürlüğü|^icra/, menu: icraMenusu },
        { anahtar: 'ceza', eslesir: /ceza|soruşturma|savcılı|başsavcı/, menu: null },
        { anahtar: 'idare', eslesir: /idare|vergi mahkeme|danıştay/, menu: null },
        { anahtar: 'hukuk', eslesir: /hukuk|asliye|sulh|aile|iş mahkeme|tüketici|ticaret|kadastro/, menu: null },
    ];
    const DIGER_TUR = { anahtar: 'diger', menu: null };
    function dosyaTuru(d) {
        const metinler = [d.dosyaTurKodAciklama, d.birimAdi].map(function(x) { return kucukHarf(String(x || '')); });
        for (const m of metinler) {
            if (!m) continue;
            const t = DOSYA_TURLERI.find(function(x) { return x.eslesir.test(m); });
            if (t) return t;
        }
        return DIGER_TUR;
    }
    window.__uyapDosyaTuru = function(d) { return dosyaTuru(d).anahtar; };

    // "İzmir 15. İcra Dairesi 2025/123": dilekçe / yazışmada en çok yazılan bilgi.
    function dosyaBilgisi(d) {
        return [d.birimAdi, d.dosyaNo].filter(Boolean).join(' ').replace(/\s+/g, ' ').trim();
    }
    function kartMenusu(c) {
        const bilgi = dosyaBilgisi(c.d);
        if (!bilgi) return null;
        const no = String(c.d.dosyaNo || '').replace(/\s+/g, ' ').trim();
        const g = [[bilgi, null], ['Dosya bilgilerini kopyala', function() { panoya(bilgi); }]];
        if (no) g.push(['Dosya numarasını kopyala', function() { panoya(no); }]);
        return [g];
    }
    function icraMenusu(c) {
        const gruplar = kartMenusu(c);
        if (gruplar && c.d.dosyaId) gruplar.push([['121 Gönder', function() { talep121(c.d); }]]);
        return gruplar;
    }

    // ── İİK 121 talebi: sağ tık → (borçlu seçimi) → açıklama kutusu → hazırla, e-imzala, gönder ──
    // UYAP'ın Borçlu > Talep Gönder ekranının yaptığını tarayıcısız yapar (UYAP main.js'ten, 2026-10-20):
    //   getPortalAvukatTalepList.ajx {dosyaId, islemTuru:'genel'} → talep türleri (121'inki adından bulunur)
    //   talepBilgileri = [{...talepTuru, aciklama, masraf: talepMasrafi, fields:[aciklama], id:0}]
    //   getIcraTalepEvrakHazirla.uyap → UDF ("Talep Evrakı Oluştur") → /__uyap_agent__/sign_udf
    //   avukatIcraTalepEvrakiGonder.ajx (multipart; evrak türü ICR_AVUKAT_PORTAL_GENEL_TALEP, talepGrupId 2)
    // Masraflı talepte ödeme bilgisi gerekir; o durumda gönderilmez, UYAP ekranına yönlendirilir.
    const T121_GRUP_ID = '2';
    const T121_EVRAK_TUR = 'ICR_AVUKAT_PORTAL_GENEL_TALEP';
    let t121Konum = null, t121Mesgul = false;
    // Önbellek katmanına uğramayan ham fetch: borçlu / talep listesi ve talep evrakı hep taze gelmeli.
    let hamFetchFn = null;
    function hamFetch(url, init) {
        if (!hamFetchFn) {
            try {
                const f = document.createElement('iframe');
                f.style.display = 'none';
                document.documentElement.appendChild(f);
                hamFetchFn = f.contentWindow.fetch.bind(window);
            } catch (e) { hamFetchFn = window.fetch.bind(window); }
        }
        return hamFetchFn(url, Object.assign({ credentials: 'include', cache: 'no-store' }, init || {}));
    }
    // UYAP aynı oturumda süren başka bir sorgu varken dizi yerine {type:'error', message:'Eş zamanlı olarak birden fazla
    // sorgulama yapamazsınız!'} döner (canlı 26.11.2026: "Parayı iste" borçlu listesinde "borçlu bulunamadı" sanıldı).
    // O yanıtta 4 sn arayla 3 kez daha denenir.
    function uyapHataMesaji(j) {
        if (!j || Array.isArray(j) || typeof j !== 'object') return '';
        return String(j.message || j.errorMessage || j.mesaj || '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
    }
    async function uyapTaze(uc, govde) {
        for (let deneme = 1; ; deneme++) {
            const r = await hamFetch('/' + uc, { method: 'POST',
                headers: { 'Content-Type': 'application/json', 'Accept': 'application/json, text/plain, */*' },
                body: JSON.stringify(govde) });
            if (!r.ok) throw new Error('UYAP ' + uc + ' HTTP ' + r.status);
            const j = await r.json();
            if (!/eş zamanlı|es zamanli/i.test(uyapHataMesaji(j)) || deneme >= 4) return j;
            await new Promise(function(c) { setTimeout(c, 4000); });
        }
    }
    async function borcluListesi(d) {
        const j = await uyapTaze('dosya_borclu_list.ajx', { dosyaId: d.dosyaId });
        if (!Array.isArray(j)) throw new Error(uyapHataMesaji(j) || 'UYAP borçlu listesini beklenmeyen biçimde döndürdü.');
        return j.filter(function(b) { return b && b.kisiKurumId; }).map(function(b) {
            const k = b.kisiTumDVO || {}, kr = b.kurumDVO || b.kurumTumDVO || {};
            const ad = [k.adi, k.soyadi].filter(Boolean).join(' ').trim() || kr.kurumAdi || kr.unvan || k.unvan || 'Borçlu';
            return { kisiKurumId: b.kisiKurumId, ad: String(ad).replace(/\s+/g, ' ').trim() };
        });
    }
    function hata121(m) {
        kur();
        kutu.replaceChildren(el('div', 'font-size:14px;font-weight:700;', '121 Talebi'),
                             el('div', 'margin-top:8px;color:#fca5a5;word-break:break-word;', m),
                             dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
        ort.style.display = 'flex';
    }
    async function talep121(d) {
        const konum = t121Konum;
        let borclular;
        try { borclular = await borcluListesi(d); } catch (e) { hata121('Borçlular okunamadı: ' + e.message); return; }
        if (!borclular.length) { hata121('Bu dosyada borçlu bulunamadı.'); return; }
        if (borclular.length === 1) { kutu121(d, borclular[0]); return; }
        // Birden çok borçlu: aynı yerde ikinci menü — talep kimin için?
        const g = [['121 talebi — borçlu seçin', null]].concat(borclular.map(function(b) {
            return [b.ad, function() { kutu121(d, b); }];
        }));
        sagMenuGoster(konum ? konum.x : innerWidth / 2, konum ? konum.y : innerHeight / 3, [g]);
    }
    function kutu121(d, borclu) {
        kur();
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', 'İİK 121 — Yetki Verilmesi Talebi'));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', function() { if (!t121Mesgul) kapat(); });
        ust.appendChild(x);
        kutu.appendChild(ust);
        kutu.appendChild(el('div', 'margin-top:2px;color:#cbd5e1;word-break:break-word;', dosyaBilgisi(d)));
        kutu.appendChild(el('div', 'margin-top:2px;color:#cbd5e1;word-break:break-word;', 'Borçlu: ' + borclu.ad));
        const etiket = el('label', 'display:block;margin-top:10px;font-weight:600;', 'Açıklama');
        const ta = el('textarea', 'min-height:90px;resize:vertical;');
        ta.maxLength = 250;   // UYAP'ın açıklama alanı sınırı; metni UYAP kendi şablonuna yerleştirir
        ta.placeholder = 'Talep evrakına eklenecek açıklama';
        etiket.appendChild(ta);
        kutu.appendChild(etiket);
        const sayac = el('div', 'margin-top:2px;font-size:11px;color:#9ca3af;text-align:right;');
        const say = function() { sayac.textContent = ta.value.length + ' / 250'; };
        ta.addEventListener('input', say);
        say();
        kutu.appendChild(sayac);
        const durum = el('div', 'margin-top:8px;color:#cbd5e1;word-break:break-word;');
        kutu.appendChild(durum);
        const gonder = dugme('E-imzala ve Gönder', 'var(--cu-vurgu)', async function(b) {
            const metin = ta.value.replace(/\s+/g, ' ').trim();
            if (!metin) { durum.style.color = '#fca5a5'; durum.textContent = 'Açıklama boş olamaz.'; return; }
            if (t121Mesgul) return;
            t121Mesgul = true;
            b.disabled = vazgec.disabled = ta.disabled = true;
            b.style.opacity = '0.6';
            try {
                const sonuc = await gonder121(d, borclu, metin, function(m) { durum.style.color = '#cbd5e1'; durum.textContent = m; });
                durum.style.color = '#86efac';
                durum.textContent = '✓ Gönderildi. ' + (sonuc.message || '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
                b.remove();
                vazgec.textContent = 'Kapat';
                vazgec.disabled = false;
            } catch (e) {
                durum.style.color = '#fca5a5';
                durum.textContent = e.message;
                b.disabled = vazgec.disabled = ta.disabled = false;
                b.style.opacity = '';
            } finally {
                t121Mesgul = false;
            }
        });
        const vazgec = dugme('Vazgeç', 'rgba(255,255,255,0.15)', function() { if (!t121Mesgul) kapat(); });
        kutu.appendChild(gonder);
        kutu.appendChild(vazgec);
        setTimeout(function() { ta.focus(); }, 0);
    }
    async function gonder121(d, borclu, metin, ilerle) {
        ilerle('Talep türü UYAP\'tan alınıyor…');
        const turler = await uyapTaze('getPortalAvukatTalepList.ajx', { dosyaId: d.dosyaId, islemTuru: 'genel' });
        const tur = (Array.isArray(turler) ? turler : []).find(function(t) {
            return t && /121/.test(t.talepAdi || '') && /yetki/i.test(kucukHarf(t.talepAdi));
        });
        if (!tur) throw new Error('UYAP\'ın genel talep listesinde "İİK. 121. Maddeye Göre Yetki Verilmesi" bulunamadı; '
            + 'dosya durumu bu talebe izin vermiyor olabilir.');
        const masraf = Number(tur.talepMasrafi) || 0;
        if (masraf > 0) throw new Error('Bu talep ' + masraf + ' TL masraflı görünüyor; ödeme bilgisi gerektiği için '
            + 'buradan gönderilmedi. UYAP\'ın Talep Gönder ekranını kullanın.');
        const talepBilgileri = JSON.stringify([Object.assign({}, tur, {
            aciklama: metin, masraf: masraf, fields: [{ id: 'aciklama', title: 'Açıklama' }], id: 0 })]);
        return talepEvrakiGonder(d, borclu, talepBilgileri, '_121_Talebi', ilerle);
    }
    // Genel talep evrakı: hazırla ("Talep Evrakı Oluştur") → e-imzala → "Genel Talebi" olarak ekle ve gönder.
    async function talepEvrakiGonder(d, borclu, talepBilgileri, adEki, ilerle) {
        // UYAP aynı oturumda süren başka bir sorgu varken "Eş zamanlı olarak birden fazla sorgulama yapamazsınız" döner
        // (canlı 26.11.2026: 8 sn sonra ikinci deneme geçti). O yanıtta birkaç kez bekleyip yeniden denenir.
        const eszamanli = function(m) { return /eş zamanlı|es zamanli/i.test(String(m || '')); };
        const bekle = function(ms) { return new Promise(function(r) { setTimeout(r, ms); }); };
        let hr, udf, bas, zip;
        for (let deneme = 1; ; deneme++) {
            ilerle(deneme === 1 ? 'Talep evrakı hazırlanıyor…' : 'UYAP meşgul; talep evrakı yeniden hazırlanıyor (' + deneme + '. deneme)…');
            hr = await hamFetch('/getIcraTalepEvrakHazirla.uyap', { method: 'POST',
                headers: { 'Content-Type': 'application/json', 'Accept': 'application/json, text/plain, */*' },
                body: JSON.stringify({ dosyaId: d.dosyaId, filename: d.dosyaNo, kisiKurumId: borclu.kisiKurumId,
                                       talepBilgileri: talepBilgileri }) });
            udf = new Uint8Array(await hr.arrayBuffer());
            bas = new TextDecoder().decode(udf.slice(0, 4000));
            zip = udf[0] === 0x50 && udf[1] === 0x4b;
            if (zip || !eszamanli(bas) || deneme >= 4) break;
            await bekle(4000);
        }
        if (!hr.ok || !(zip || (bas.indexOf('<template') >= 0))) {
            throw new Error('UYAP talep evrakını hazırlamadı (HTTP ' + hr.status + '): ' + bas.slice(0, 160));
        }

        ilerle('E-imzalanıyor…');
        const ad = String(d.dosyaNo || 'dosya').replace(/\//g, '_') + adEki;
        const sr = await hamFetch('/__uyap_agent__/sign_udf?name=' + encodeURIComponent(ad + '.udf'), {
            method: 'POST', body: udf, headers: { 'Content-Type': 'application/octet-stream' } });
        if (!sr.ok) {
            let m = 'İmzalama başarısız (' + sr.status + ').';
            try { const j = await sr.json(); if (j && j.error) m = j.error; } catch (e) {}
            throw new Error(m);
        }
        const imzali = await sr.arrayBuffer();
        const imzaliAd = ad + '_imzali.udf';

        ilerle('Evrak UYAP\'a gönderiliyor…');
        // Evrak listesi UYAP'ın evrak yükleme bileşeninin gönderdiği biçimde (bkz. teblig_212/gonder.py).
        const item = {
            id: Date.now(),
            evrakTuruOptionDVO: { tur: T121_EVRAK_TUR, label: 'Genel Talebi', mandatory: true, max: 1, ekEvrakMax: 2,
                sablonTurKodu: '', acceptTypeList: ['.udf', '.UDF'],
                ekEvrakAcceptTypeList: ['.udf', '.pdf', '.jpg', '.jpeg', '.png', '.tif', '.tiff',
                                        '.UDF', '.PDF', '.JPG', '.JPEG', '.PNG', '.TIF', '.TIFF'] },
            tur: T121_EVRAK_TUR, turAciklama: 'Genel Talebi', mandatory: true, file: {},
            path: 'C:/fakepath/' + imzaliAd, kullaniciEvrakAciklama: '',
            parentId: -1, isVekalet: false, isVekaletBilgiFormu: false, fileId: 1,
        };
        const fd = new FormData();
        fd.append('file_1', new File([imzali], imzaliAd, { type: 'application/octet-stream' }));
        fd.append('items', JSON.stringify([item]));
        fd.append('dosyaId', d.dosyaId);
        fd.append('kisiKurumId', String(borclu.kisiKurumId));
        fd.append('tutar', '0');
        fd.append('talepGrupId', T121_GRUP_ID);
        fd.append('talepBilgileri', talepBilgileri);
        let gr, sonuc = null;
        for (let deneme = 1; ; deneme++) {
            gr = await hamFetch('/avukatIcraTalepEvrakiGonder.ajx', { method: 'POST', body: fd });
            sonuc = null;
            try { sonuc = await gr.json(); } catch (e) {}
            // Yalnız "eş zamanlı sorgulama" reddinde yeniden gönderilir: UYAP o istekte hiçbir şey işlememiştir.
            if ((sonuc && sonuc.type === 'success') || !eszamanli(sonuc && sonuc.message) || deneme >= 4) break;
            ilerle('UYAP meşgul; yeniden gönderiliyor (' + (deneme + 1) + '. deneme)…');
            await bekle(4000);
        }
        if (!sonuc || sonuc.type !== 'success') {
            throw new Error('UYAP gönderimi kabul etmedi: '
                + String((sonuc && sonuc.message) || ('HTTP ' + gr.status)).replace(/<[^>]+>/g, ' ').trim());
        }
        return sonuc;
    }

    // ── PTT barkodu: "Kapalı Tebligat" belgesinde seçili 13 haneli barkod ──
    // Belgede barkod Code-39 yazısıyla "*4007770084489*" diye durur; seçim yıldızlı / boşluklu gelebilir.
    // Belgenin adı görüntüleyici başlığından (.documentViewerHeader h6), yoksa evrak ağacında seçili
    // satırdan okunur ("Kapalı Tebligat 19/10/2026").
    function acikBelgeAdi(h) {
        for (let a = h; a && a !== document.body; a = a.parentElement) {
            const bas = a.querySelector && a.querySelector('.documentViewerHeader h6');
            if (bas) return bas.textContent.trim();
        }
        const kart = h.closest('.dosya-sorgula-popup');
        const s = kart && kart.querySelector('.dx-treeview-node.dx-state-selected');
        return s ? s.textContent.trim() : '';
    }
    // Evrak ağacında seçili belgenin birim evrak numarası (ek belgede ana belgenin numarası); yoksa null.
    function acikBelgeNo(kart) {
        const A = window.__uyapEvrakAgaci;
        const tv = kart && A && kart.querySelector('.evrak-treeview');
        const koklar = tv && A.veri(tv);
        const node = tv && tv.querySelector('.dx-treeview-node.dx-state-selected');
        const g = koklar && node ? A.harita(koklar)[node.getAttribute('data-item-id')] : null;
        if (!g || !g.x.evrak) return null;
        const no = parseInt(String(A.ozet(g.x, g.ebeveyn).birimEvrakNo || ''), 10);
        return no > 0 ? no : null;
    }
    // Evrak ağacında seçili belgenin onay tarihi ("16/09/2026"); yoksa ''.
    function acikBelgeTarihi(kart) {
        const A = window.__uyapEvrakAgaci;
        const tv = kart && A && kart.querySelector('.evrak-treeview');
        const koklar = tv && A.veri(tv);
        const node = tv && tv.querySelector('.dx-treeview-node.dx-state-selected');
        const g = koklar && node ? A.harita(koklar)[node.getAttribute('data-item-id')] : null;
        return g && g.x.evrak ? String(A.ozet(g.x, g.ebeveyn).onaylandigiTarih || '').slice(0, 10) : '';
    }
    function barkodMenusu(h, secili) {
        const m = /^\*?\s*((?:\d\s*){13})\*?$/.exec(secili);
        if (!m || !/^kapalı tebligat/.test(kucukHarf(acikBelgeAdi(h)))) return null;
        const barkod = m[1].replace(/\s/g, '');
        const g = [['PTT barkodu · ' + barkod, null], ['Kopyala', function() { panoya(barkod); }],
                   ['PTT barkod sorgula', function() { pttSorgula(barkod); }]];
        const kart = h.closest('.dosya-sorgula-popup');
        const d = kart && window.__uyapKartVerisi ? window.__uyapKartVerisi(kart) : null;
        // Gözle görülen tebligatı tek tıkla büro veritabanına yaz (İcra Süreçleri'ne girmeden): bu belge + PTT durumu.
        const evrakNo = kart ? acikBelgeNo(kart) : null;
        if (d && d.dosyaId && evrakNo) g.push(['Dosya tebligatlarına kaydet', function() { tebligatEkle(d, evrakNo, barkod); }]);
        if (d) g.push([PTT_SABLONU + ' işi ata', function() {
            const b = acikBelge(kart);
            pencereAc(d, kart, b ? b.evrakId : null, { sablon: PTT_SABLONU, barkod: barkod });
        }]);
        return [g];
    }
    // Belge görüntüleyicide açık "Kapalı Tebligat (tarih)" belgesinin üstünde sağ tık (seçim gerekmez): tebligatı büro
    // veritabanına kaydet (belge dosya olarak saklanmaz; tür, borçlu, barkod, PTT durumu yazılır) ve belgedeki barkodu
    // PTT'de sorgula (görüntüleyicide barkoda tıklamayla aynı işlev). Evrak ağacında değil, yalnız görüntüleyicide.
    // Görüntüleyicinin kökü: .documentViewerHeader'ı DOĞRUDAN çocuk olarak taşıyan kap (canlı 20.11.2026: yalnız
    // [.documentViewerHeader, .documentViewerBody]). Başlığı içeren daha üst kaplar dosya penceresinin çoğudur; onlar
    // sayılırsa pencerenin her yerinde bu menü çıkıp dosya menüsünü örtüyordu.
    function goruntuleyiciKoku(h, kart) {
        for (let a = h; a && a !== kart; a = a.parentElement) {
            const c = a.children;
            for (let i = 0; c && i < c.length; i++) if (c[i].classList && c[i].classList.contains('documentViewerHeader')) return a;
        }
        return null;
    }
    function tebligatBelgesiMenusu(h) {
        const kart = h.closest('.dosya-sorgula-popup');
        if (!kart || h.closest('.evrak-treeview')) return null;
        const kok = goruntuleyiciKoku(h, kart);
        const bas = kok && kok.querySelector(':scope > .documentViewerHeader h6');
        const ad = (bas ? bas.textContent : '').trim() || acikBelgeAdi(h);
        const kucuk = kucukHarf(ad);
        const mazbataMi = /^kapalı (e-)?tebliğ mazbatası/.test(kucuk);
        if (!kok || !(mazbataMi || /^kapalı tebligat/.test(kucuk))) return null;
        const barkod = belgedekiBarkod(kok);   // kartın gizli sekmelerindeki başka görüntüleyiciler karışmasın
        const d = window.__uyapKartVerisi ? window.__uyapKartVerisi(kart) : null;
        const evrakNo = acikBelgeNo(kart);
        const g = [[ad + (barkod ? ' · ' + barkod : ''), null]];
        if (mazbataMi) {
            // E-tebliğ mazbatası: evrak listesinde açıklaması yok, barkod yalnız belgede. Ekrandaki yazıdan okunur, UYAP'a gidilmez.
            if (d && d.birimId && d.dosyaNo) g.push(['Mazbatayı tebligata işle', function() { mazbataIsle(d, kok, ad, acikBelgeTarihi(kart), null); }]);
            if (barkod) g.push(['Barkod sorgula', function() { pttSorgula(barkod); }]);
            return g.length > 1 ? [g] : null;
        }
        if (d && d.dosyaId && evrakNo) g.push(['Dosya tebligatlarına kaydet', function() { tebligatEkle(d, evrakNo, barkod); }]);
        if (barkod) g.push(['Barkod sorgula', function() { pttSorgula(barkod); }]);
        return g.length > 1 ? [g] : null;
    }
    // ── Mazbata ↔ tebligat elle bağı (fiziki mazbata çoğu zaman barkodsuz fotoğraf olarak taranır) ──
    // Evrak ağacında mazbata satırına ya da görüntüleyicide açık mazbataya sağ tık → "Tebligata bağla…". Tür UYAP'ın
    // evrak verisinden okunur (yazıdan değil); sunucudaki BAGLANABILIR_MAZBATALAR ile aynı liste.
    const BAGLANABILIR_MAZBATALAR = ['Tebliğ Mazbatası', 'PTT MTS Tebliğ Mazbatası', 'Kapalı Tebliğ Mazbatası'];
    function mazbataDugumu(h) {
        const kart = h.closest('.dosya-sorgula-popup');
        const A = window.__uyapEvrakAgaci;
        const tv = kart && A && kart.querySelector('.evrak-treeview');
        if (!tv) return null;
        let node = null;
        if (h.closest('.evrak-treeview')) node = h.closest('.dx-treeview-node');
        else if (goruntuleyiciKoku(h, kart)) node = tv.querySelector('.dx-treeview-node.dx-state-selected');
        const koklar = node && A.veri(tv);
        const g = koklar ? A.harita(koklar)[node.getAttribute('data-item-id')] : null;
        const e = g && !g.x.isEkEvrak ? g.x.evrak : null;
        if (!e || BAGLANABILIR_MAZBATALAR.indexOf(String(e.tur || '').trim()) < 0 || !(parseInt(e.birimEvrakNo, 10) > 0)) return null;
        return { kart: kart, evrak: e };
    }
    function mazbataBagMenusu(h) {
        const m = mazbataDugumu(h);
        const d = m && window.__uyapKartVerisi ? window.__uyapKartVerisi(m.kart) : null;
        if (!d || !d.birimId || !d.dosyaNo) return null;
        const tarih = String(m.evrak.onaylandigiTarih || '').slice(0, 10);
        return [[['Tebligata bağla… · ' + String(m.evrak.tur).trim() + (tarih ? ' ' + tarih : ''), function() { mazbataBagPenceresi(d, m.evrak); }]]];
    }
    // ── "Parayı iste": tahsilat makbuzuna sağ tık → Borçlu Tarafından Yapılan Ödemelerin Hesaba Aktarılması (26.11.2026) ──
    // UYAP'ın Borçlu > Talep Gönder > Genel Talepler ekranında canlı yakalandı: talepKodu 14, grupKodu 2,
    // className AvukatTalepGenelOdemeAktarDVO; talep alanları hesapKimeAit (V/A), hesapKimeAitText, ibanNo ("TR…").
    // Hazırla → e-imzala → gönder akışı 121 talebiyle ortak (talepEvrakiGonder). IBAN'lar büro veritabanında
    // (iban.rs): vekil IBAN'ları Ayarlar'dan, alacaklı IBAN'ı dosya başına.
    function tahsilatDugumu(h) {
        const kart = h.closest('.dosya-sorgula-popup');
        const A = window.__uyapEvrakAgaci;
        const tv = kart && A && kart.querySelector('.evrak-treeview');
        if (!tv) return null;
        let node = null;
        if (h.closest('.evrak-treeview')) node = h.closest('.dx-treeview-node');
        else if (goruntuleyiciKoku(h, kart)) node = tv.querySelector('.dx-treeview-node.dx-state-selected');
        const koklar = node && A.veri(tv);
        const g = koklar ? A.harita(koklar)[node.getAttribute('data-item-id')] : null;
        const e = g ? g.x.evrak : null;
        if (!e || !/tahsilat makbuzu/.test(kucukHarf(String(e.tur || '') + ' ' + String(e.aciklama || '')))) return null;
        return { kart: kart, evrak: e };
    }
    function paraIsteMenusu(h) {
        const m = tahsilatDugumu(h);
        const d = m && window.__uyapKartVerisi ? window.__uyapKartVerisi(m.kart) : null;
        if (!d || !d.dosyaId) return null;
        const tarih = String(m.evrak.onaylandigiTarih || '').slice(0, 10);
        return [[['Parayı iste · Tahsilat makbuzu' + (tarih ? ' ' + tarih : ''), null],
                 ['Kayıtlı IBAN\'ıma…', function() { paraVekil(d); }],
                 ['Alacaklı IBAN\'ına…', function() { paraAlacakli(d); }]]];
    }
    async function veriGet(uc) {
        const r = await window.fetch('/__cu__/veri/' + uc, { cache: 'no-store', credentials: 'same-origin' });
        let j = null;
        try { j = await r.json(); } catch (e) {}
        if (!j) throw new Error('Ofis bilgisayarından beklenmeyen yanıt (HTTP ' + r.status + '). Program güncel olmayabilir.');
        if (j.ok === false || j.error) throw new Error(j.error || 'İşlem yapılamadı.');
        return j.veri !== undefined ? j.veri : j;
    }
    function ibanGruplu(s) { return String(s || '').replace(/(.{4})/g, '$1 ').trim(); }
    // iban.rs::duzelt ile aynı kural: "TR" + 24 rakam, mod-97.
    function ibanDuzelt(ham) {
        let s = String(ham || '').replace(/[\s-]/g, '').toUpperCase();
        if (/^\d{24}$/.test(s)) s = 'TR' + s;
        if (!/^TR\d{24}$/.test(s)) return null;
        const d = s.slice(4) + '2927' + s.slice(2, 4);   // T=29, R=27
        let k = 0;
        for (let i = 0; i < d.length; i++) k = (k * 10 + (d.charCodeAt(i) - 48)) % 97;
        return k === 1 ? s : null;
    }
    function paraMenu(baslik, maddeler) {
        const konum = t121Konum;
        sagMenuGoster(konum ? konum.x : innerWidth / 2, konum ? konum.y : innerHeight / 3, [[[baslik, null]].concat(maddeler)], true);
    }
    function paraHata(m) {
        kur();
        kutu.replaceChildren(el('div', 'font-size:14px;font-weight:700;', 'Parayı iste'),
                             el('div', 'margin-top:8px;color:#fca5a5;word-break:break-word;', m),
                             dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
        ort.style.display = 'flex';
    }
    async function paraVekil(d) {
        let l;
        try { l = await veriGet('ibanlar'); } catch (e) { paraHata('IBAN\'lar okunamadı: ' + e.message); return; }
        l = Array.isArray(l) ? l : [];
        if (!l.length) { ibanGir(d, 'V'); return; }
        paraMenu('Kayıtlı IBAN\'ıma', l.map(function(x) {
            return [(x.etiket ? x.etiket + ' · ' : '') + x.gruplu + (x.varsayilan ? ' (varsayılan)' : ''), function() { paraBorclu(d, 'V', x.iban); }];
        }).concat([['Yeni IBAN gireceğim…', function() { ibanGir(d, 'V'); }]]));
    }
    async function paraAlacakli(d) {
        let k;
        try { k = await veriGet('alacakli_iban?birim_id=' + encodeURIComponent(d.birimId || '') + '&dosya_no=' + encodeURIComponent(d.dosyaNo || '')); }
        catch (e) { paraHata('Alacaklı IBAN\'ı okunamadı: ' + e.message); return; }
        if (!k || !k.iban) { ibanGir(d, 'A'); return; }
        paraMenu('Alacaklı IBAN\'ına', [['Dosyada kayıtlı IBAN · ' + k.gruplu, function() { paraBorclu(d, 'A', k.iban); }],
                                         ['Başka IBAN…', function() { ibanGir(d, 'A'); }]]);
    }
    // Yeni IBAN girişi. Vekil: "IBAN'larıma kaydedilsin mi?"; alacaklı: "bu dosyanın alacaklı IBAN'ı olarak kaydedilsin mi?"
    function ibanGir(d, kime) {
        kur();
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', kime === 'V' ? 'Parayı iste — yeni IBAN (vekil)' : 'Parayı iste — alacaklı IBAN\'ı'));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', kapat);
        ust.appendChild(x);
        kutu.appendChild(ust);
        kutu.appendChild(el('div', 'margin-top:2px;color:#cbd5e1;word-break:break-word;', dosyaBilgisi(d)));
        const etiket = el('label', 'display:block;margin-top:10px;font-weight:600;', 'IBAN');
        const giris = el('input', 'font-family:Consolas,monospace;letter-spacing:0.5px;');
        giris.placeholder = 'TR00 0000 0000 0000 0000 0000 00';
        giris.autocomplete = 'off';
        giris.spellcheck = false;
        etiket.appendChild(giris);
        kutu.appendChild(etiket);
        let adGiris = null;
        if (kime === 'V') {
            const a = el('label', 'display:block;margin-top:8px;font-weight:600;', 'Etiket (isteğe bağlı)');
            adGiris = el('input');
            adGiris.placeholder = 'ör. Ziraat — büro hesabı';
            adGiris.maxLength = 120;
            a.appendChild(adGiris);
            kutu.appendChild(a);
        }
        const kaydetSatir = el('label', 'display:flex;align-items:center;gap:6px;margin-top:10px;cursor:pointer;');
        const kaydet = el('input', 'width:auto;margin:0;');
        kaydet.type = 'checkbox';
        kaydet.checked = true;
        kaydetSatir.appendChild(kaydet);
        kaydetSatir.appendChild(el('span', '', kime === 'V' ? 'Bu IBAN\'ı IBAN\'larıma kaydet' : 'Bu dosyanın alacaklı IBAN\'ı olarak kaydet'));
        kutu.appendChild(kaydetSatir);
        const durum = el('div', 'margin-top:8px;color:#fca5a5;word-break:break-word;');
        kutu.appendChild(durum);
        const ileri = async function() {
            const iban = ibanDuzelt(giris.value);
            if (!iban) { durum.textContent = 'IBAN geçersiz: "TR" + 24 rakam olmalı ve kontrol hanesi tutmalı.'; giris.focus(); return; }
            if (kaydet.checked) {
                try {
                    if (kime === 'V') await veriPost('iban_ekle', { iban: iban, etiket: adGiris ? adGiris.value : '' });
                    else await veriPost('alacakli_iban', { birim_id: String(d.birimId || ''), dosya_no: String(d.dosyaNo || ''),
                                                           birim_adi: String(d.birimAdi || ''), iban: iban });
                } catch (e) { durum.textContent = 'IBAN kaydedilemedi: ' + e.message; return; }
            }
            paraBorclu(d, kime, iban);
        };
        giris.addEventListener('keydown', function(ev) { if (ev.key === 'Enter') { ev.preventDefault(); ileri(); } });
        kutu.appendChild(dugme('Devam', 'var(--cu-vurgu)', ileri));
        kutu.appendChild(dugme('Vazgeç', 'rgba(255,255,255,0.15)', kapat));
        setTimeout(function() { giris.focus(); }, 0);
    }
    // Talep bir borçlu üzerinden gönderilir (UYAP ekranındaki "Borçlu" seçimi); birden çok borçluda sorulur.
    async function paraBorclu(d, kime, iban) {
        let borclular;
        try { borclular = await borcluListesi(d); } catch (e) { paraHata('Borçlular okunamadı: ' + e.message); return; }
        if (!borclular.length) { paraHata('Bu dosyada borçlu bulunamadı.'); return; }
        if (borclular.length === 1) { paraOnay(d, kime, iban, borclular[0]); return; }
        paraMenu('Talep hangi borçlu için?', borclular.map(function(b) { return [b.ad, function() { paraOnay(d, kime, iban, b); }]; }));
    }
    function paraOnay(d, kime, iban, borclu) {
        kur();
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', 'Ödemelerin Hesaba Aktarılması Talebi'));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', function() { if (!t121Mesgul) kapat(); });
        ust.appendChild(x);
        kutu.appendChild(ust);
        const satir = function(a, b) {
            const s = el('div', 'margin-top:4px;display:flex;gap:8px;');
            s.appendChild(el('span', 'min-width:110px;color:#9ca3af;', a));
            s.appendChild(el('span', 'word-break:break-word;font-weight:600;', b));
            kutu.appendChild(s);
        };
        satir('Dosya', dosyaBilgisi(d));
        satir('Borçlu', borclu.ad);
        satir('Hesap kime ait', kime === 'V' ? 'Vekil' : 'Alacaklı');
        satir('IBAN', ibanGruplu(iban));
        const durum = el('div', 'margin-top:10px;color:#cbd5e1;word-break:break-word;');
        kutu.appendChild(durum);
        const gonder = dugme('E-imzala ve Gönder', 'var(--cu-vurgu)', async function(b) {
            if (t121Mesgul) return;
            t121Mesgul = true;
            b.disabled = vazgec.disabled = true;
            b.style.opacity = '0.6';
            try {
                const sonuc = await gonderPara(d, borclu, kime, iban, function(m) { durum.style.color = '#cbd5e1'; durum.textContent = m; });
                durum.style.color = '#86efac';
                durum.textContent = '✓ Gönderildi. ' + (sonuc.message || '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
                b.remove();
                vazgec.textContent = 'Kapat';
                vazgec.disabled = false;
            } catch (e) {
                durum.style.color = '#fca5a5';
                durum.textContent = e.message;
                b.disabled = vazgec.disabled = false;
                b.style.opacity = '';
            } finally {
                t121Mesgul = false;
            }
        });
        const vazgec = dugme('Vazgeç', 'rgba(255,255,255,0.15)', function() { if (!t121Mesgul) kapat(); });
        kutu.appendChild(gonder);
        kutu.appendChild(vazgec);
    }
    async function gonderPara(d, borclu, kime, iban, ilerle) {
        ilerle('Talep türü UYAP\'tan alınıyor…');
        const turler = await uyapTaze('getPortalAvukatTalepList.ajx', { dosyaId: d.dosyaId, islemTuru: 'genel' });
        const tur = (Array.isArray(turler) ? turler : []).find(function(t) {
            return t && (Number(t.talepKodu) === 14 || /hesaba aktar/.test(kucukHarf(t.talepAdi)));
        });
        if (!tur) throw new Error('UYAP\'ın genel talep listesinde "Borçlu Tarafından Yapılan Ödemelerin Hesaba Aktarılması" '
            + 'bulunamadı; dosya durumu bu talebe izin vermiyor olabilir.');
        const masraf = Number(tur.talepMasrafi) || 0;
        if (masraf > 0) throw new Error('Bu talep ' + masraf + ' TL masraflı görünüyor; ödeme bilgisi gerektiği için '
            + 'buradan gönderilmedi. UYAP\'ın Talep Gönder ekranını kullanın.');
        const talepBilgileri = JSON.stringify([Object.assign({}, tur, {
            hesapKimeAit: kime, hesapKimeAitText: kime === 'V' ? 'Vekil' : 'Alacaklı', ibanNo: iban, masraf: masraf,
            fields: [{ id: 'hesapKimeAitText', title: 'Hesap Kime Ait' }, { id: 'ibanNo', title: 'IBAN No' }], id: 0 })]);
        return talepEvrakiGonder(d, borclu, talepBilgileri, '_Genel_Talebi', ilerle);
    }
    async function veriPost(uc, govde) {
        const r = await window.fetch('/__cu__/veri/' + uc, { method: 'POST', cache: 'no-store', credentials: 'same-origin',
            headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(govde) });
        let j = null;
        try { j = await r.json(); } catch (e) {}
        if (!j) throw new Error('Ofis bilgisayarından beklenmeyen yanıt (HTTP ' + r.status + '). Program güncel olmayabilir.');
        if (j.ok === false || j.error || j.detail) throw new Error(j.error || j.detail || 'İşlem yapılamadı.');
        return j.veri !== undefined ? j.veri : j;
    }
    async function mazbataBagPenceresi(d, evrak) {
        kur();
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const tur = String(evrak.tur || 'Mazbata').trim();
        const tarih = String(evrak.onaylandigiTarih || '').slice(0, 10);
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', 'Mazbatayı tebligata bağla'));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', kapat);
        ust.appendChild(x);
        kutu.appendChild(ust);
        kutu.appendChild(el('div', 'margin-top:2px;color:#9ca3af;', [tur, tarih, 'evrak ' + evrak.birimEvrakNo].filter(Boolean).join(' · ')));
        const govde = el('div', 'margin-top:10px;color:#cbd5e1;', 'Tebligatlar alınıyor…');
        kutu.appendChild(govde);
        const hata = function(e) { govde.replaceChildren(el('div', 'color:#fca5a5;word-break:break-word;', e.message), dugme('Kapat', 'rgba(255,255,255,0.15)', kapat)); };
        const etiket = function(t) {
            const ptt = !t.ptt_durumu ? 'PTT sorgulanmadı'
                : (mzTeslim(t) ? 'Teslim edildi' : t.ptt_durumu) + (t.son_islem_tarihi ? ' ' + String(t.son_islem_tarihi).slice(0, 10) : '');
            return [t.evrak_tarihi ? String(t.evrak_tarihi).slice(0, 10) : '', t.tur || 'Tebligat', t.muhatap, t.barkod || 'barkodsuz', ptt,
                    t.mazbata_var ? 'mazbata döndü' : 'mazbata dönmedi'].filter(Boolean).join(' · ');
        };
        let j;
        try {
            j = await veriPost('mazbata_bag_secenek', { birim_id: String(d.birimId || ''), dosya_no: String(d.dosyaNo || ''),
                dosya_tur_kod: d.dosyaTurKod != null ? Number(d.dosyaTurKod) : null,
                evrak: { birimEvrakNo: parseInt(evrak.birimEvrakNo, 10), tur: tur, aciklama: String(evrak.aciklama || ''),
                         onaylandigiTarih: String(evrak.onaylandigiTarih || '') } });
        } catch (e) { hata(e); return; }
        const m = j.mazbata || {};
        const mzNo = parseInt(evrak.birimEvrakNo, 10);
        const bagla = async function(tebligat, mazbata, tebligTarihi, sonra, iade) {
            govde.replaceChildren(el('div', '', 'Kaydediliyor…'));
            try { await veriPost('mazbata_bagla', { dosya: j.dosya, tebligat: tebligat, mazbata: mazbata, teblig_tarihi: tebligTarihi || null, iade: !!iade }); }
            catch (e) { hata(e); return; }
            govde.replaceChildren(el('div', 'color:#86efac;font-weight:600;', sonra),
                el('div', 'margin-top:4px;', 'Tebliğ durumu ve kesinleşme yeniden hesaplandı.'), dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
        };
        // Tebliğ günü PTT'de yoksa (eski barkod PTT'den silinir) mazbatanın sonucu sorulur: tebliğ edildiyse mazbatadaki
        // tarih (boş bırakılabilir); iade döndüyse tarih istenmez (26.11.2026, kullanıcı bulgusu: iade dönen tebligatın
        // tebliğ tarihi olmaz). İade işaretli bağda tebligat İade sayılır, kesinleşme başlamaz.
        const tarihleBagla = function(t) {
            if (mzTebligGunuVar(t)) { bagla(t.evrak_no, mzNo, null, 'Mazbata bağlandı: ' + etiket(t)); return; }
            const giris = el('input', 'width:auto;margin:0 0 0 8px;');
            giris.type = 'date';
            const ust = mzIsoGun(evrak.onaylandigiTarih) || new Date().toISOString().slice(0, 10);
            giris.max = ust;
            if (mzIsoGun(t.evrak_tarihi)) giris.min = mzIsoGun(t.evrak_tarihi);
            if (m.elle_tebligat === t.evrak_no && m.teblig_tarihi && !m.iade) giris.value = mzIsoGun(m.teblig_tarihi);
            const satir = el('label', 'display:flex;align-items:center;margin-top:6px;', 'Tebliğ tarihi');
            satir.appendChild(giris);
            const iadeDugme = dugme('İade döndü (tebliğ edilemedi)', 'rgba(234,88,12,0.75)', function() {
                bagla(t.evrak_no, mzNo, null, 'Mazbata bağlandı: ' + etiket(t) + ' · iade (tebliğ edilemedi)', true);
            });
            iadeDugme.title = 'Tebligat İade sayılır; kesinleşme başlamaz, "ödeme emri iade dönen" işlemlerinde görünür.';
            govde.replaceChildren(el('div', 'font-weight:600;', etiket(t)),
                el('div', 'margin-top:6px;color:#fde68a;', 'Bu tebligatın tebliğ günü PTT\'de yok (eski barkodlar PTT\'nin sisteminden silinir). '
                    + 'Mazbatanın sonucu nedir?'),
                el('div', 'margin-top:10px;font-weight:600;', 'Tebliğ edildi'),
                el('div', 'margin-top:2px;color:#9ca3af;', 'Mazbatada yazan tebliğ tarihini girerseniz kesinleşme bu günden hesaplanır.'), satir,
                dugme('Tebliğ edildi olarak bağla', 'var(--cu-vurgu)', function() {
                    if (!giris.value) { giris.focus(); return; }
                    bagla(t.evrak_no, mzNo, giris.value, 'Mazbata bağlandı: ' + etiket(t) + ' · tebliğ ' + giris.value.split('-').reverse().join('.'));
                }),
                dugme('Tarihsiz bağla', 'rgba(255,255,255,0.12)', function() { bagla(t.evrak_no, mzNo, null, 'Mazbata bağlandı: ' + etiket(t)); }),
                el('div', 'margin-top:12px;font-weight:600;', 'İade döndü'),
                el('div', 'margin-top:2px;color:#9ca3af;', 'Tebliğ tarihi gerekmez.'),
                iadeDugme,
                el('div', 'margin-top:8px;'),
                dugme('Geri', 'rgba(255,255,255,0.15)', ciz));
        };
        const tebs = j.tebligatlar || [];
        const ciz = function() {
            govde.innerHTML = '';
            const bagli = m.elle_tebligat ? tebs.find(function(t) { return t.evrak_no === m.elle_tebligat; }) : null;
            govde.appendChild(el('div', '', m.elle_tebligat ? 'Şu an elle bağlı: ' + (bagli ? etiket(bagli) : 'evrak ' + m.elle_tebligat)
                + (m.iade ? ' · iade döndü (tebliğ edilemedi)' + (m.teblig_tarihi ? ' ' + m.teblig_tarihi : '')
                          : (m.teblig_tarihi ? ' · mazbatadaki tebliğ tarihi ' + m.teblig_tarihi : ''))
                : 'Elle bağ yok (barkod ve tarihle otomatik eşleşme geçerli).'));
            const a = mzAdaylari({ evrak_tarihi: evrak.onaylandigiTarih, elle_tebligat: m.elle_tebligat }, tebs);
            if (!a.liste.length) {
                govde.appendChild(el('div', 'margin-top:6px;color:#fde68a;', tebs.length ? 'Bu mazbatadan önce giden PTT tebligatı yok.'
                    : 'Dosyada PTT ile giden tebligat kaydı yok. Önce İcra Süreçleri\'nden dosyanın tebligatlarını güncelleyin.'));
            } else {
                govde.appendChild(el('div', 'margin-top:8px;font-weight:600;', 'Bu mazbata hangi tebligatın?'));
                a.liste.forEach(function(t) {
                    const secili = t.evrak_no === m.elle_tebligat;
                    const b = dugme((t === a.onerilen ? '★ Önerilen: ' : '') + etiket(t) + (secili ? '  ✓' : ''),
                        secili || t === a.onerilen ? 'var(--cu-vurgu)' : 'rgba(255,255,255,0.12)', function() { tarihleBagla(t); });
                    b.style.display = 'block';
                    b.style.textAlign = 'left';
                    b.style.margin = '6px 0 0';
                    if (t.mazbata_var && t.mazbata && !secili) b.title = 'Bu tebligatın şu anki mazbatası: ' + t.mazbata;
                    govde.appendChild(b);
                });
                if (a.elenen) govde.appendChild(el('div', 'margin-top:6px;color:#9ca3af;', a.elenen + ' tebligat mazbatadan sonra gittiği için listelenmedi.'));
            }
            if (bagli && !mzTebligGunuVar(bagli)) govde.appendChild(dugme(m.iade ? 'Mazbata sonucunu değiştir' : m.teblig_tarihi ? 'Tebliğ tarihini değiştir' : 'Mazbatanın sonucunu / tebliğ tarihini gir',
                'rgba(255,255,255,0.12)', function() { tarihleBagla(bagli); }));
            if (m.elle_tebligat) govde.appendChild(dugme('Elle bağı kaldır', 'rgba(239,68,68,0.55)',
                function() { bagla(m.elle_tebligat, null, null, 'Elle bağ kaldırıldı; otomatik eşleşmeye dönüldü.'); }));
            govde.appendChild(dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
        };
        ciz();
    }
    // Aynı kurallar icra.js'te (mazbataAdaylari, tebligGunuVar): mazbata kendi tebligatından önce dönemez; önerilen,
    // teslim edilmiş ve mazbatası dönmemiş, teslim günü mazbataya en yakın tebligat.
    function mzGun(s) {
        let m = /^(\d{2})[\/.](\d{2})[\/.](\d{4})/.exec(String(s || '').trim());
        if (m) return +(m[3] + m[2] + m[1]);
        m = /^(\d{4})-(\d{2})-(\d{2})/.exec(String(s || '').trim());
        return m ? +(m[1] + m[2] + m[3]) : 0;
    }
    function mzIsoGun(s) { const n = mzGun(s); return n ? String(n).replace(/^(\d{4})(\d{2})(\d{2})$/, '$1-$2-$3') : ''; }
    function mzTeslim(t) { return String(t.ptt_durumu || '').toLocaleLowerCase('tr-TR').indexOf('teslim edildi') >= 0; }
    function mzTebligGunuVar(t) { return mzTeslim(t) && !!mzGun(t.son_islem_tarihi); }
    function mzAdaylari(m, fiziki) {
        const mg = mzGun(m.evrak_tarihi);
        const uygun = fiziki.filter(function(t) { const tg = mzGun(t.evrak_tarihi); return !mg || !tg || tg <= mg; });
        const serbest = function(t) { return !t.mazbata_var || t.evrak_no === m.elle_tebligat; };
        const puan = function(t) {
            const sg = mzGun(t.son_islem_tarihi);
            if (mzTeslim(t) && serbest(t) && sg && (!mg || sg <= mg)) return 3e8 + sg;
            if (serbest(t)) return 1e8 + mzGun(t.evrak_tarihi);
            return mzGun(t.evrak_tarihi);
        };
        const liste = uygun.slice().sort(function(a, b) { return puan(b) - puan(a); });
        return { liste: liste, onerilen: liste.length && puan(liste[0]) >= 1e8 ? liste[0] : null, elenen: fiziki.length - uygun.length };
    }
    // "Mazbatayı tebligata işle": görüntülenen mazbatanın yazısı sunucuya gider (barkod, hesap sahibi, UETS olayları);
    // barkod → muhatap + gün → kullanıcı seçimi. UYAP'a istek yok.
    async function mazbataIsle(d, kok, ad, tarih, tebligatId) {
        kur();
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', 'Mazbatayı tebligata işle'));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', kapat);
        ust.appendChild(x);
        kutu.appendChild(ust);
        const govde = el('div', 'margin-top:10px;color:#cbd5e1;', 'İşleniyor…');
        kutu.appendChild(govde);
        const satirlar = [...kok.querySelectorAll('.react-pdf__Page__textContent span, .rpv-core__text-layer-text')].map(function(s) { return s.textContent; });
        const satir = function(a, deger, renk) {
            if (!deger) return;
            const s = el('div', 'display:flex;gap:8px;margin-top:3px;');
            s.appendChild(el('div', 'flex:none;width:110px;color:#9ca3af;', a));
            s.appendChild(el('div', 'color:' + (renk || '#f3f4f6') + ';font-weight:600;word-break:break-word;', deger));
            govde.appendChild(s);
        };
        let j = null;
        try {
            const r = await window.fetch('/__cu__/veri/mazbata_isle', { method: 'POST', cache: 'no-store', credentials: 'same-origin',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ birim_id: String(d.birimId || ''), dosya_no: String(d.dosyaNo || ''),
                                       dosya_tur_kod: d.dosyaTurKod != null ? Number(d.dosyaTurKod) : null,
                                       belge: ad + (tarih ? ' ' + tarih : ''), metin: satirlar, tebligat: tebligatId }) });
            try { j = await r.json(); } catch (e) {}
            if (!j) throw new Error('Ofis bilgisayarından beklenmeyen yanıt (HTTP ' + r.status + '). Program güncel olmayabilir.');
            if (j.ok === false || j.error || j.detail) throw new Error(j.error || j.detail || 'İşlem yapılamadı.');
            j = j.veri || j;
        } catch (e) {
            govde.replaceChildren(el('div', 'color:#fca5a5;word-break:break-word;', e.message), dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
            return;
        }
        govde.innerHTML = '';
        if (j.secim_gerekli) {
            govde.appendChild(el('div', 'color:#fde68a;font-weight:600;', j.neden || 'Tebligat belirlenemedi.'));
            govde.appendChild(el('div', 'margin-top:6px;', 'Bu mazbata hangi tebligatın? (' + ((j.mazbata && j.mazbata.hesap_sahibi) || 'muhatap okunamadı') +
                (j.mazbata && j.mazbata.ilk_olay ? ', UETS ' + j.mazbata.ilk_olay.split('-').reverse().join('.') : '') + ')'));
            (j.adaylar || []).forEach(function(a) {
                govde.appendChild(dugme([a.evrak_tarihi ? a.evrak_tarihi.split('-').reverse().join('.') : '', a.tur, a.muhatap, a.barkod].filter(Boolean).join(' · '),
                    'rgba(255,255,255,0.12)', function() { mazbataIsle(d, kok, ad, tarih, a.id); }));
            });
            if (!(j.adaylar || []).length) govde.appendChild(el('div', 'margin-top:6px;', 'Dosyada e-tebligat kaydı yok. Önce "Kapalı Tebligat" belgesini dosya tebligatlarına kaydedin.'));
            govde.appendChild(dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
            return;
        }
        const t = j.tebligat || {};
        govde.appendChild(el('div', 'color:#86efac;font-weight:600;', 'Mazbata tebligata işlendi.'));
        if (j.eslesme === 'muhatap_tarih') govde.appendChild(el('div', 'margin-top:4px;color:#fde68a;', 'Barkod uyuşmadı; muhatap ve tarihle eşleştirildi. Lütfen kontrol edin.'));
        satir('Tebligat', [t.evrak_tarihi ? t.evrak_tarihi.split('-').reverse().join('.') : '', t.tur].filter(Boolean).join(' · '));
        satir('Muhatap', t.muhatap);
        satir('Barkod', t.barkod + (j.mazbata && j.mazbata.barkod && j.mazbata.barkod !== t.barkod ? ' (mazbatada ' + j.mazbata.barkod + ')' : ''));
        if (j.mazbata && j.mazbata.son_olay) satir('Son UETS olayı', j.mazbata.son_olay.tarih.split('-').reverse().join('.') + ' · ' + j.mazbata.son_olay.metin);
        govde.appendChild(dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
    }
    // "Dosya tebligatlarına kaydet": ofisteki tek dosya işi (yalnız bu tebligat belgesi + PTT; dosya UYAP'ta yeniden aranmaz).
    async function tebligatEkle(d, evrakNo, barkod) {
        kur();
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', 'Dosya tebligatlarına kaydet' + (barkod ? ' · ' + barkod : '')));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', kapat);
        ust.appendChild(x);
        kutu.appendChild(ust);
        const govde = el('div', 'margin-top:10px;color:#cbd5e1;', 'Tebligat belgesi alınıyor ve PTT\'ye soruluyor…');
        kutu.appendChild(govde);
        const hata = function(m) {
            govde.replaceChildren(el('div', 'color:#fca5a5;word-break:break-word;', m),
                                  dugme('Yeniden dene', 'var(--cu-vurgu)', function() { tebligatEkle(d, evrakNo, barkod); }),
                                  dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
        };
        const istek = async function(yol, govdeVeri) {
            const r = await window.fetch('/__uyap_agent__/icra_guncelle/dosya' + yol, govdeVeri === undefined
                ? { cache: 'no-store', credentials: 'same-origin' }
                : { method: 'POST', cache: 'no-store', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(govdeVeri) });
            let j = null;
            try { j = await r.json(); } catch (e) {}
            if (!j) throw new Error('Ofis bilgisayarından beklenmeyen yanıt (HTTP ' + r.status + '). Program güncel olmayabilir.');
            if (j.error) throw new Error(j.error);
            return j.tarama;
        };
        let t;
        try {
            t = await istek('', { uyap_dosya_id: String(d.dosyaId), evraklar: [evrakNo], ptt: true,
                                  birim_id: String(d.birimId || ''), dosya_no: String(d.dosyaNo || ''),
                                  dosya_tur_kod: d.dosyaTurKod != null ? Number(d.dosyaTurKod) : null });
        }
        catch (e) { hata(e.message); return; }
        for (let i = 0; i < 300 && t && ['bitti', 'hata', 'iptal'].indexOf(t.durum) < 0; i++) {
            govde.textContent = t.durum === 'captcha' ? 'UYAP güvenlik doğrulaması istiyor: İcra Süreçleri panelinden doğrulayın.'
                : (t.mesaj || 'İşleniyor…');
            await new Promise(function(r) { setTimeout(r, 1500); });
            try { t = await istek('/' + encodeURIComponent(t.id)); } catch (e) { hata(e.message); return; }
        }
        if (!t || t.durum !== 'bitti') { hata((t && t.mesaj) || 'İşlem tamamlanamadı.'); return; }
        govde.innerHTML = '';
        const satir = function(ad, deger, renk) {
            if (!deger) return;
            const s = el('div', 'display:flex;gap:8px;margin-top:3px;');
            s.appendChild(el('div', 'flex:none;width:110px;color:#9ca3af;', ad));
            s.appendChild(el('div', 'color:' + (renk || '#f3f4f6') + ';font-weight:600;word-break:break-word;', deger));
            govde.appendChild(s);
        };
        let kayit = null, neden = '', kes = '';
        try {
            const r = await window.fetch('/__cu__/veri/dosya?id=' + encodeURIComponent(t.dosya), { cache: 'no-store', credentials: 'same-origin' });
            const j = await r.json();
            const v = j && (j.veri || j);
            kayit = ((v && v.tebligatlar) || []).find(function(x) { return x.evrak_no === evrakNo; }) || null;
            const a = ((v && v.alinamayan) || []).find(function(x) { return x.evrak_no === evrakNo; });
            neden = a ? (a.neden || a.tebligat_notu || '') : '';
            const b = kayit && kayit.borclu_bag ? ((v && v.taraflar) || []).find(function(x) { return x.bag === kayit.borclu_bag; }) : null;
            kes = b ? b.kesinlesme : '';
        } catch (e) {}
        if (kayit) {
            govde.appendChild(el('div', 'color:#86efac;font-weight:600;', 'Tebligat dosyanın tebligatlarına kaydedildi.'));
            satir('Grup', kayit.grup);
            satir('Tebligat türü', kayit.tur || 'Belirsiz', kayit.tur && kayit.tur !== 'Belirsiz' ? null : '#fde68a');
            if (kayit.zarf_icerigi && kayit.zarf_icerigi !== kayit.tur) satir('Zarf içeriği', kayit.zarf_icerigi);
            if (kayit.haciz_turu) {
                if (kayit.haciz_sirasi) satir('89 sırası', '89/' + kayit.haciz_sirasi);
                else {
                    // Belgede 89/1-2-3 yazmıyor: tahmin yok, kullanıcıya sorulur.
                    const s89 = el('div', 'display:flex;gap:8px;margin-top:6px;align-items:center;flex-wrap:wrap;');
                    s89.appendChild(el('div', 'flex:none;width:110px;color:#fde68a;', '89 sırası?'));
                    [1, 2, 3].forEach(function(n) {
                        s89.appendChild(dugme('89/' + n, 'rgba(255,255,255,0.12)', async function() {
                            try {
                                const r = await window.fetch('/__cu__/veri/haciz_sirasi', { method: 'POST', cache: 'no-store', credentials: 'same-origin',
                                    headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ tebligat: kayit.id, sira: n }) });
                                const j = await r.json();
                                if (j && (j.ok === false || j.error)) throw new Error(j.error || 'Kaydedilemedi.');
                                s89.replaceChildren(el('div', 'flex:none;width:110px;color:#9ca3af;', '89 sırası'), el('div', 'color:#86efac;font-weight:600;', '89/' + n + ' kaydedildi'));
                            } catch (e) { kisaBildirim(e.message); }
                        }));
                    });
                    govde.appendChild(s89);
                }
            }
            // Muhatap üçüncü kişi olabilir (89/1 haciz ihbarnamesi): borçluya bağlı olsa da muhatap ayrıca gösterilir.
            satir('Muhatap', kayit.muhatap);
            if (kayit.borclu && kucukHarf(kayit.borclu) !== kucukHarf(kayit.muhatap || '')) satir('Dosyadaki borçlu', kayit.borclu);
            satir('Barkod', kayit.barkod);
            satir('PTT durumu', kayit.ptt_durumu || (kayit.elektronik ? 'E-tebligat' : 'Sorgulanamadı'));
            satir('Mazbata', kayit.mazbata_var ? 'Dosyaya döndü'
                : kayit.elektronik ? 'Henüz işlenmedi ("Kapalı E-Tebliğ Mazbatası" gelince sağ tık › Mazbatayı tebligata işle)' : 'Henüz dönmedi');
            // Kesinleşme yalnız ödeme / tahliye emrinde (borçlunun) ve 103 / kıymet takdirinde (sabit süre) vardır.
            const kesTur = /^(ödeme emri|tahliye emri|103 davetiyesi|kıymet takdiri raporu)$/.test(kucukHarf(kayit.tur || ''));
            satir(kesTur ? 'Borçlunun kesinleşmesi' : 'Kesinleşme', kesTur ? kes : 'Bu tebligat türünde kesinleşme yok');
            if (!kayit.tur || kayit.tur === 'Belirsiz') govde.appendChild(el('div', 'margin-top:6px;color:#fde68a;',
                'Tür belgeden anlaşılamadı. İcra Süreçleri › Tebligatlar › "Tür eşleştir" ile türünü seçebilirsiniz.'));
        } else {
            govde.appendChild(el('div', 'color:#fde68a;font-weight:600;', 'Tebligat kaydedilemedi.'));
            if (neden) satir('Neden', neden);
        }
        (t.rapor || []).forEach(function(r) { govde.appendChild(el('div', 'margin-top:3px;color:#9ca3af;font-size:11px;', r)); });
        (t.uyarilar || []).slice(0, 5).forEach(function(u) {
            govde.appendChild(el('div', 'margin-top:3px;color:#fde68a;word-break:break-word;', typeof u === 'string' ? u : (u.metin || JSON.stringify(u))));
        });
        govde.appendChild(dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
    }
    async function pttSorgula(barkod) {
        kur();
        kutu.innerHTML = '';
        ort.style.display = 'flex';
        const ust = el('div', 'display:flex;align-items:center;justify-content:space-between;gap:8px;');
        ust.appendChild(el('div', 'font-size:14px;font-weight:700;', 'PTT Gönderi Takip · ' + barkod));
        const x = el('span', 'font-size:18px;color:#9ca3af;cursor:pointer;padding:0 4px;', '×');
        x.addEventListener('click', kapat);
        ust.appendChild(x);
        kutu.appendChild(ust);
        const govde = el('div', 'margin-top:10px;color:#cbd5e1;', 'PTT sorgulanıyor… (ptt.gov.tr, 10–20 sn sürebilir)');
        kutu.appendChild(govde);
        let j = null;
        try {
            const r = await window.fetch('/__uyap_agent__/ptt_sorgu?barkod=' + encodeURIComponent(barkod), { cache: 'no-store' });
            try { j = await r.json(); } catch (e) {}
            if (!j) throw new Error('PTT sorgusu yanıt vermedi (HTTP ' + r.status + ').');
            if (!j.ok) throw new Error(j.error || 'PTT sorgusu başarısız.');
        } catch (e) {
            govde.replaceChildren(el('div', 'color:#fca5a5;word-break:break-word;', e.message),
                                  dugme('Yeniden dene', 'var(--cu-vurgu)', function() { pttSorgula(barkod); }),
                                  dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
            return;
        }
        govde.innerHTML = '';
        const satir = function(ad, deger, renk) {
            if (!deger) return;
            const s = el('div', 'display:flex;gap:8px;margin-top:3px;');
            s.appendChild(el('div', 'flex:none;width:110px;color:#9ca3af;', ad));
            s.appendChild(el('div', 'color:' + (renk || '#f3f4f6') + ';font-weight:600;word-break:break-word;', deger));
            govde.appendChild(s);
        };
        const d = kucukHarf(j.durum);
        satir('Durum', j.durum || '—', /teslim edildi/.test(d) ? '#86efac' : /iade/.test(d) ? '#fca5a5' : '#fde68a');
        satir(/teslim edildi/.test(d) ? 'Teslim tarihi' : 'Son işlem', j.tarih);
        satir(/iade/.test(d) ? 'İade nedeni' : 'Teslim şekli', j.teslim_sekli);
        if (j.kabul) { satir('Kabul tarihi', j.kabul.tarih); satir('Gönderici', j.kabul.gonderici); satir('Alıcı', j.kabul.alici); }
        if (j.hareketler && j.hareketler.length) {
            govde.appendChild(el('div', 'margin-top:10px;font-weight:600;color:#cbd5e1;', 'Hareketler'));
            j.hareketler.slice().reverse().forEach(function(hr) {   // en yeni üstte
                const s = el('div', 'margin-top:4px;padding:5px 8px;border-radius:7px;background:rgba(255,255,255,0.06);');
                s.appendChild(el('div', 'color:#f3f4f6;font-weight:600;', hr.aciklama || '—'));
                s.appendChild(el('div', 'color:#9ca3af;font-size:11px;', [hr.tarih, hr.yer].filter(Boolean).join(' · ')));
                if (hr.detay) s.appendChild(el('div', 'color:#cbd5e1;font-size:11px;', hr.detay));
                govde.appendChild(s);
            });
        }
        govde.appendChild(dugme('Barkodu kopyala', 'rgba(255,255,255,0.15)', function() { panoya(barkod); }));
        govde.appendChild(dugme('Kapat', 'rgba(255,255,255,0.15)', kapat));
    }

    // Sağ tıklanan yerin menüsü; null → tarayıcı menüsü.
    function sagMenuMaddeleri(h) {
        const kart = h.closest('.dosya-sorgula-popup');
        const d = kart && window.__uyapKartVerisi ? window.__uyapKartVerisi(kart) : null;
        const tur = d ? dosyaTuru(d) : null;
        const gruplar = tur ? (tur.menu || kartMenusu)({ d: d, kart: kart }) : null;
        return gruplar && gruplar.length ? gruplar : null;
    }
    function sagMenuGoster(x, y, gruplar, altYaziYok) {
        if (!sagMenu) {
            sagMenu = el('div', 'position:fixed;left:0;top:0;z-index:2147483646;min-width:200px;max-width:340px;padding:4px 0;border-radius:9px;background:rgba(var(--cu-panel-rgb),0.97);border:1px solid rgba(255,255,255,0.15);box-shadow:0 10px 30px rgba(0,0,0,0.45);color:#e5e7eb;font:12px/1.4 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;display:none;');
            sagMenu.id = 'uyap-sagmenu';
            sagMenu.addEventListener('contextmenu', function(ev) { ev.preventDefault(); ev.stopPropagation(); });
            document.body.appendChild(sagMenu);
        }
        sagMenu.innerHTML = '';
        gruplar.forEach(function(g, i) {
            if (i) sagMenu.appendChild(el('div', 'height:1px;margin:4px 0;background:rgba(255,255,255,0.1);'));
            g.forEach(function(m) {
                if (m && m.sira) {         // düğme sırası (çoklu pano gözleri): [etiket, fn|null, ipucu, dolu]
                    const r = el('div', 'display:flex;gap:3px;padding:3px 12px 6px;');
                    m.sira.forEach(function(b) {
                        const x = el('button', 'flex:1;min-width:22px;padding:3px 0;border-radius:5px;cursor:pointer;font-family:inherit;font-size:11.5px;font-weight:700;color:inherit;border:1px solid rgba(255,255,255,' +
                            (b[3] ? '0.35' : '0.12') + ');background:' + (b[3] ? 'rgba(var(--cu-vurgu-rgb),0.35)' : 'transparent') + ';', b[0]);
                        x.type = 'button';
                        x.title = b[2] || '';
                        if (!b[1]) { x.disabled = true; x.style.opacity = '0.35'; x.style.cursor = 'default'; }
                        x.addEventListener('mousedown', function(ev) { ev.preventDefault(); ev.stopPropagation(); });
                        x.addEventListener('click', function(ev) { ev.stopPropagation(); sagMenuKapat(); b[1](); });
                        r.appendChild(x);
                    });
                    sagMenu.appendChild(r);
                    return;
                }
                if (!m[1]) {               // grup başlığı (ör. "İcra dosyası · …"): tıklanmaz
                    sagMenu.appendChild(el('div', 'padding:4px 14px 2px;color:var(--cu-vurgu-acik);font-size:11px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;', m[0]));
                    return;
                }
                const s = el('div', null, m[0]);
                s.className = 'uyap-sagmenu-madde';
                s.title = m[0];
                s.addEventListener('mousedown', function(ev) { ev.preventDefault(); ev.stopPropagation(); });
                s.addEventListener('click', function(ev) { ev.stopPropagation(); sagMenuKapat(); m[1](); });
                sagMenu.appendChild(s);
            });
        });
        if (!altYaziYok) sagMenu.appendChild(el('div', 'padding:5px 14px 2px;color:#6b7280;font-size:10.5px;', 'Tarayıcı menüsü: Shift + sağ tık')).className = 'uyap-sagmenu-not';
        sagMenu.style.display = 'block';
        const r = sagMenu.getBoundingClientRect();
        sagMenu.style.left = Math.max(4, Math.min(x, innerWidth - r.width - 4)) + 'px';
        sagMenu.style.top = Math.max(4, Math.min(y, innerHeight - r.height - 4)) + 'px';
    }
    function sagMenuTus(ev) {
        if (!sagMenu || sagMenu.style.display === 'none') return;
        const l = sagMenu.querySelectorAll('.uyap-sagmenu-madde');
        if (ev.key === 'Escape') {
            sagMenuKapat();
        } else if ((ev.key === 'ArrowDown' || ev.key === 'ArrowUp') && l.length) {
            const n = l.length, asagi = ev.key === 'ArrowDown';
            sagMenuSecili = sagMenuSecili < 0 ? (asagi ? 0 : n - 1) : (sagMenuSecili + (asagi ? 1 : n - 1)) % n;
            l.forEach(function(m, i) { m.classList.toggle('sec', i === sagMenuSecili); });
        } else if (ev.key === 'Enter' && sagMenuSecili >= 0) {
            l[sagMenuSecili].click();
        } else {
            sagMenuKapat();
            return;
        }
        ev.preventDefault();
        ev.stopPropagation();
    }
    // Başka bölümler (ör. Dosya Sorgulama'daki "2026/1234" kutusu) aynı menüyü ve panoyu kullanır.
    window.__uyapSagMenuAc = function(x, y, gruplar) { kur(); sagMenuGoster(x, y, gruplar, true); };
    window.__uyapSagMenuEkDestek = true;   // sağ tık menüsü __uyapSagMenuEkleri satırlarını da gösterir
    window.__uyapPanoya = panoya;
    window.__uyapBildirim = kisaBildirim;

    // ── Çoklu pano (16.11.2026): 10 göz (1–9, 0), arayüzün her yerinde ──
    // Ctrl+Shift+N: seçili metni (seçim yoksa son kopyalananı) N. göze kaydeder. Yazı alanında Ctrl+N: N. gözü
    // yapıştırır (Ctrl+V olağan panodur, dokunulmaz). Sağ tıkta "kaydet" ve "yapıştır" düğme sıraları.
    // 23.11.2026: Chrome / Edge / Firefox Ctrl+1…9'u sekme geçişine ayırır, sayfa engelleyemez → Alt+N de yapıştırır
    // (Alt+Shift+N kaydeder). Alt ile yalnız üst sıradaki rakamlar: sayısal tuş takımı Alt kodlarına (Alt+0199 = Ç)
    // kalır. AltGr (Ctrl+Alt) hiç yakalanmaz: Türkçe klavyede # $ { [ ] } yazar.
    // Gözler yalnız bu tarayıcıda kalır (localStorage); hiçbir yere gönderilmez.
    const PANO_ANAHTAR = 'cuCokluPano', PANO_SIRA = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'];
    function panoOku() {
        try {
            const v = JSON.parse(localStorage.getItem(PANO_ANAHTAR) || '[]');
            if (Array.isArray(v)) return PANO_SIRA.map(function(_, i) { return typeof v[i] === 'string' ? v[i] : ''; });
        } catch (e) {}
        return PANO_SIRA.map(function() { return ''; });
    }
    function panoYaz(l) { try { localStorage.setItem(PANO_ANAHTAR, JSON.stringify(l)); } catch (e) {} }
    function panoOnizleme(m) { m = String(m).replace(/\s+/g, ' ').trim(); return m.length > 60 ? m.slice(0, 57) + '…' : m; }
    function panoKaydet(i, metin) {
        metin = String(metin || '').slice(0, 20000);
        if (!metin.trim()) { kisaBildirim('Kaydedilecek metin yok: önce bir yeri seçin ya da kopyalayın.'); return; }
        const l = panoOku();
        l[i] = metin;
        panoYaz(l);
        kisaBildirim('Pano ' + PANO_SIRA[i] + ': ' + panoOnizleme(metin) + ' — yapıştırmak için Alt+' + PANO_SIRA[i]);
    }
    // Yazılabilir alan: metin kutusu, textarea, contenteditable (select / onay kutusu / sayı kutusu değil).
    function yaziAlani(e) {
        const a = e && e.closest ? e.closest('input, textarea, [contenteditable=""], [contenteditable="true"]') : null;
        if (!a || a.disabled || a.readOnly) return null;
        if (a.tagName === 'INPUT' && !/^(text|search|tel|url|email|password)?$/i.test(a.getAttribute('type') || '')) return null;
        return a;
    }
    function alanSecimi(a) {
        try {
            if (a && a.value != null && a.selectionStart != null && a.selectionStart !== a.selectionEnd) return a.value.slice(a.selectionStart, a.selectionEnd);
        } catch (e) {}
        return '';
    }
    // execCommand('insertText'): imleç yerine yazar, geri al (Ctrl+Z) çalışır, UYAP'ın / React'in input olayları tetiklenir.
    function alanaYaz(a, metin) {
        a.focus();
        let oldu = false;
        try { oldu = document.execCommand('insertText', false, metin); } catch (e) {}
        if (!oldu && a.value != null && a.setRangeText) {
            const bas = a.selectionStart != null ? a.selectionStart : a.value.length;
            a.setRangeText(metin, bas, a.selectionEnd != null ? a.selectionEnd : bas, 'end');
            a.dispatchEvent(new Event('input', { bubbles: true }));
        }
    }
    function panoYapistir(i, a) {
        const m = panoOku()[i];
        if (!m) { kisaBildirim('Pano ' + PANO_SIRA[i] + ' boş (kaydetmek için metni seçip Alt+Shift+' + PANO_SIRA[i] + ').'); return; }
        alanaYaz(a, m);
    }
    // Armonia Yazı penceresi (enjekte/yazi.js) aynı gözleri editör çerçevesinde kullanır.
    window.__uyapCokluPano = { oku: function() { return panoOku(); }, kaydet: function(i, m) { panoKaydet(i, m); } };
    function panoGruplari(alan, kaydedilecek, kaydetBasligi) {
        const l = panoOku(), g = [];
        if (kaydedilecek) {
            g.push([kaydetBasligi || 'Çoklu panoya kaydet · Alt+Shift+1…0', null]);
            g.push({ sira: PANO_SIRA.map(function(n, i) {
                return [n, function() { panoKaydet(i, kaydedilecek); }, n + ': ' + (l[i] ? panoOnizleme(l[i]) + ' (üzerine yazılır)' : 'boş'), !!l[i]];
            }) });
        }
        if (alan && l.some(Boolean)) {
            g.push(['Çoklu panodan yapıştır · Alt+1…0', null]);
            g.push({ sira: PANO_SIRA.map(function(n, i) {
                return [n, l[i] ? function() { panoYapistir(i, alan); } : null, n + ': ' + (l[i] ? panoOnizleme(l[i]) : 'boş'), !!l[i]];
            }) });
        }
        if (g.length && l.some(Boolean)) g.push(['Çoklu panoyu temizle', function() { panoYaz([]); kisaBildirim('Çoklu pano temizlendi.'); }]);
        return g.length ? [g] : [];
    }
    // Ctrl+N / Alt+N (bkz. yukarı). Armonia Yazı penceresi aynı kuralı çerçevede uygular (panoTusu).
    function panoKisayolu(ev) {
        const ctrl = (ev.ctrlKey || ev.metaKey) && !ev.altKey;
        const alt = ev.altKey && !ev.ctrlKey && !ev.metaKey;
        if (!ctrl && !alt) return null;
        const m = (alt ? /^Digit([0-9])$/ : /^(?:Digit|Numpad)([0-9])$/).exec(ev.code || '');
        return m ? m[1] : null;
    }
    window.__uyapPanoKisayolu = panoKisayolu;
    window.addEventListener('keydown', function(ev) {
        const n = panoKisayolu(ev);
        if (!n) return;
        const i = PANO_SIRA.indexOf(n);
        const ae = document.activeElement;
        if (ev.shiftKey) {
            const belge = sonBelgeHedefi && sonBelgeHedefi.isConnected ? belgeSecimi(sonBelgeHedefi) : '';
            ev.preventDefault();
            ev.stopImmediatePropagation();
            panoKaydet(i, alanSecimi(ae) || seciliMetin() || belge || sonKopya);
            return;
        }
        const a = yaziAlani(ae);
        if (!a) return;                        // yazı alanı dışında: tarayıcıya bırak
        ev.preventDefault();
        ev.stopImmediatePropagation();
        panoYapistir(i, a);
    }, true);
    document.addEventListener('copy', function() {   // tarayıcının kendi kopyalaması (UYAP'ın engellemediği yerler)
        const m = alanSecimi(document.activeElement) || seciliMetin();
        if (m) sonKopya = m.slice(0, 20000);
    }, true);

    function sagMenuKur() {
        if (!anaCerceve() || window.__uyapSagMenu || !document.body) return;
        window.__uyapSagMenu = true;
        document.addEventListener('contextmenu', function(ev) {
            sagMenuKapat();
            const h = ev.target && ev.target.closest ? ev.target : null;
            if (ev.shiftKey || ev.ctrlKey || !h || sagMenuKapali()) return;
            const alan = yaziAlani(h);
            const secili = (alan ? alanSecimi(alan) : '') || seciliMetin() || sagTikSecimi || belgeSecimi(h);
            sagTikSecimi = '';
            const goster = function(gruplar) {
                ev.preventDefault();
                kur();
                t121Konum = { x: ev.clientX, y: ev.clientY };   // borçlu seçim menüsü aynı yerde açılsın
                sagMenuGoster(ev.clientX, ev.clientY, gruplar);
            };
            const kopyala = secili ? [[['Seçili metni kopyala', function() { panoya(secili); }]]] : [];
            let tebG = null;
            try { tebG = tebligatBelgesiMenusu(h); } catch (e) {}
            if (alan && tebG) {
                let dosyaG = null;
                try { dosyaG = sagMenuMaddeleri(h); } catch (e) {}
                goster(kopyala.concat(tebG, dosyaG || [], panoGruplari(alan, secili)));
                return;
            }
            if (alan) {
                // Yazı alanı: seçim ya da dolu çoklu pano varsa bizim menü (Kes / Yapıştır için Shift + sağ tık).
                const g = panoGruplari(alan, secili);
                if (g.length) goster(kopyala.concat(g));
                return;
            }
            if (h.closest('select, #uyap-ispanel') || (ort && ort.contains(h))) return;
            // Seçim varken en üstte "Seçili metni kopyala", altında PTT barkodu ya da dosya türünün menüsü, en altta
            // çoklu pano. Kartta seçimsiz sağ tıkta pano sırası son kopyalananı kaydeder.
            let kartG = null;
            if (secili) { try { kartG = barkodMenusu(h, secili); } catch (e) {} }
            if (!kartG && tebG) {
                let dosyaG = null;
                try { dosyaG = sagMenuMaddeleri(h); } catch (e) {}
                kartG = tebG.concat(dosyaG || []);
            }
            if (!kartG) { try { kartG = sagMenuMaddeleri(h); } catch (e) {} }   // beklenmeyen sayfa yapısı: menüsüz
            // Mazbata (evrak ağacındaki satır ya da görüntüleyicide açık belge): "Tebligata bağla…" en üstte.
            let mzG = null;
            try { mzG = mazbataBagMenusu(h); } catch (e) {}
            if (mzG) kartG = mzG.concat(kartG || []);
            // Tahsilat makbuzu (evrak ağacındaki satır ya da görüntüleyicide açık belge): "Parayı iste" en üstte.
            let paraG = null;
            try { paraG = paraIsteMenusu(h); } catch (e) {}
            if (paraG) kartG = paraG.concat(kartG || []);
            // Ek betiklerin satırları (ör. paylas.js: belge üzerinde "Belgeyi paylaş")
            let ekG = [];
            (window.__uyapSagMenuEkleri || []).forEach(function(f) {
                try { const x = f(h); if (x && x.length) ekG = ekG.concat(x); } catch (e) {}
            });
            const pano = secili ? panoGruplari(null, secili)
                : (kartG || ekG.length) && sonKopya ? panoGruplari(null, sonKopya, 'Son kopyalananı kaydet: ' + panoOnizleme(sonKopya)) : [];
            const g = kopyala.concat(kartG || [], ekG, pano);
            if (g.length) goster(g);
        }, true);
        document.addEventListener('mousedown', function(ev) { if (sagMenu && !sagMenu.contains(ev.target)) sagMenuKapat(); }, true);
        window.addEventListener('keydown', sagMenuTus, true);
        window.addEventListener('blur', sagMenuKapat);
        window.addEventListener('resize', sagMenuKapat);
        document.addEventListener('scroll', sagMenuKapat, true);
    }

    // ── Görev çubuğu (enjekte/gorevcubugu.js) ve kart görev çubuğu (enjekte/kartlar.js) için kancalar ──
    window.__uyapAnaCerceve = anaCerceve;
    window.__uyapTazeKart = tazeKart;
    window.__uyapKartAlanlari = kartAlanlari;
    window.__uyapDosyaAnahtari = dosyaAnahtari;
    window.__uyapKartiAcDogrudan = uyapKartiAc;        // yalnız ana sayfada (#searchId) çalışır
    window.__uyapSeciliSekme = seciliSekme;
    window.__uyapSekmeSec = sekmeSec;
    window.__uyapUyapKartiniKapat = uyapKartiniKapat;

    setInterval(kartlariTara, 700);
})();
</script>"#####;


const _EK_BETIK_DOSYALARI: &[&str] = &[
    "tema.js",
    "mobil.js",
    "gorevcubugu.js",
    "kartlar.js",
    "anasayfa.js",
    "icra.js",
    "isler.js",
    "belgeler.js",
    "paylas.js",
    "guncelleme.js",
    "yazi.js",
    "ayarlar.js",
];
// armonia-renk.css: UYAP stilindeki lacivert/gri renkleri tema değişkenlerimize bağlayan, araclar/renk_uret.py ile
// üretilen katman (2026-11-14). tema.css'ten önce gelir: elle ince ayarlar eşit özgüllükte kazanır.
const _STIL_DOSYALARI: &[&str] = &["armonia-renk.css", "tema.css", "mobil.css"];

static _EK_BETIK_ONBELLEK: Lazy<Vec<String>> = Lazy::new(_yukle_ek_betik_metinleri);
static _TEMA_CSS_ONBELLEK: Lazy<String> = Lazy::new(_yukle_tema_css);

fn _enjekte_klasoru() -> PathBuf {
    let mut adaylar: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            adaylar.push(dir.join("uyap_core").join("enjekte"));
            adaylar.push(dir.join("enjekte"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        adaylar.push(cwd.join("uyap_core").join("enjekte"));
        adaylar.push(cwd.join("enjekte"));
    }
    adaylar.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("uyap_core")
            .join("enjekte"),
    );
    if let Some(kok) = option_env!("UYAP_KAYNAK_KOK") {
        adaylar.push(PathBuf::from(kok).join("uyap_core").join("enjekte"));
    }
    for p in &adaylar {
        if p.is_dir() {
            return p.clone();
        }
    }
    adaylar
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("uyap_core/enjekte"))
}

fn _yukle_tema_css() -> String {
    let klasor = _enjekte_klasoru();
    let mut parcalar = Vec::new();
    for ad in _STIL_DOSYALARI {
        match std::fs::read_to_string(klasor.join(ad)) {
            Ok(s) => parcalar.push(s.replace("</style", "")),
            Err(e) => println!("[!] {} okunamadı: {}", ad, e),
        }
    }
    parcalar.join("\n")
}

pub fn _tema_css() -> &'static str {
    &_TEMA_CSS_ONBELLEK
}

pub fn _tema_on_eki() -> String {
    format!(
        "<style>{}</style><script>(function(){{try{{var h=document.documentElement,\
t=localStorage.getItem('cokluuyapTema');\
t=localStorage.getItem('theme')==='dark'?'koyu':(t&&/^[a-z0-9]+$/.test(t)?t:'armonia');\
h.setAttribute('data-cu-tema',t);if(t!=='koyu'&&t!=='uyap')h.setAttribute('data-cu-deri','');}}catch(e){{}}}})();</script>\
<style>html.cu-ortu #root{{visibility:hidden!important}}</style>\
<script>(function(){{try{{var a=window.top===window;try{{a=a||(window.frameElement&&window.frameElement.id==='app')}}catch(e){{}}\
if(!a)return;var h=document.documentElement;h.classList.add('cu-ortu');\
setTimeout(function(){{h.classList.remove('cu-ortu')}},2500);}}catch(e){{}}}})();</script>",
        _tema_css()
    )
}

fn _yukle_ek_betik_metinleri() -> Vec<String> {
    let klasor = _enjekte_klasoru();
    let mut metinler = Vec::new();
    for ad in _EK_BETIK_DOSYALARI {
        let metin = match std::fs::read_to_string(klasor.join(ad)) {
            Ok(s) => s,
            Err(e) => {
                println!("[!] Enjekte betik okunamadı ({}): {}", ad, e);
                continue;
            }
        };
        if metin.to_ascii_lowercase().contains("</script") {
            println!("[!] Enjekte betik '</script' içeriyor, atlandı: {}", ad);
            continue;
        }
        metinler.push(metin);
    }
    metinler
}

pub fn _ek_betik_metinleri() -> &'static [String] {
    &_EK_BETIK_ONBELLEK
}

pub fn _ek_betikler() -> String {
    let mut out = String::new();
    for m in _ek_betik_metinleri() {
        out.push_str("<script>\n");
        out.push_str(m);
        out.push_str("\n</script>");
    }
    out
}

pub fn _compute_inject_version() -> String {
    let mut hasher = Md5::new();
    hasher.update(INLINE_INJECT_SCRIPT.as_bytes());
    for m in _ek_betik_metinleri() {
        hasher.update(m.as_bytes());
    }
    hasher.update(_tema_css().as_bytes());
    let digest = hex::encode(hasher.finalize());
    digest.chars().take(8).collect()
}

pub static INJECT_VERSION: Lazy<String> = Lazy::new(_compute_inject_version);

pub fn _build_response_headers_from_map(
    headers: &reqwest::header::HeaderMap,
    proxy_base: &str,
) -> HashMap<String, String> {
    let ctype = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let html = ctype.contains("text/html");
    let mut out = HashMap::new();
    for (k, v) in headers.iter() {
        let lk = k.as_str().to_ascii_lowercase();
        if DROP_RESPONSE.contains(&lk.as_str()) {
            continue;
        }
        if html
            && matches!(
                lk.as_str(),
                "last-modified" | "etag" | "cache-control" | "expires"
            )
        {
            continue;
        }
        let mut val = String::from_utf8_lossy(v.as_bytes()).to_string();
        if lk == "location" {
            val = val.replace(UYAP_BASE, proxy_base);
        }
        out.insert(k.as_str().to_string(), val);
    }
    if html {
        out.insert("Cache-Control".to_string(), "no-cache".to_string());
    }
    out
}

pub fn _build_response_headers(
    resp: &reqwest::Response,
    proxy_base: &str,
) -> HashMap<String, String> {
    _build_response_headers_from_map(resp.headers(), proxy_base)
}

const _EK_ZIP_ACILMIS_SINIR: u64 = 50 * 1024 * 1024;

pub fn _ek_zip_pdf_ac(path: &str, content_type: &str, body: &[u8]) -> (String, Vec<u8>) {
    let clean_path = path.split('?').next().unwrap_or("").trim_end_matches('/');
    if !clean_path.ends_with("view_document_brd.uyap") {
        return (content_type.to_string(), body.to_vec());
    }
    if !content_type.to_ascii_lowercase().contains("zip") || body.len() < 2 || &body[..2] != b"PK" {
        return (content_type.to_string(), body.to_vec());
    }

    let reader = Cursor::new(body);
    let mut archive = match ZipArchive::new(reader) {
        Ok(z) => z,
        Err(_) => return (content_type.to_string(), body.to_vec()),
    };

    let mut toplam_boyut: u64 = 0;
    let mut aday_indeksler: Vec<usize> = Vec::new();
    for idx in 0..archive.len() {
        if let Ok(entry) = archive.by_index(idx) {
            if !entry.is_dir() && entry.name().to_ascii_lowercase().ends_with(".pdf") {
                toplam_boyut = toplam_boyut.saturating_add(entry.size());
                aday_indeksler.push(idx);
            }
        }
    }
    if toplam_boyut > _EK_ZIP_ACILMIS_SINIR {
        return (content_type.to_string(), body.to_vec());
    }

    let mut pdfler: Vec<Vec<u8>> = Vec::new();
    let mut gorulen: HashSet<[u8; 32]> = HashSet::new();
    for idx in aday_indeksler {
        if let Ok(mut entry) = archive.by_index(idx) {
            let mut veri = Vec::with_capacity(entry.size() as usize);
            if entry.read_to_end(&mut veri).is_err() {
                return (content_type.to_string(), body.to_vec());
            }
            if veri.len() >= 4 && &veri[..4] == b"%PDF" {
                use sha2::Digest as _;
                let ozet: [u8; 32] = Sha256::digest(&veri).into();
                if gorulen.insert(ozet) {
                    pdfler.push(veri);
                }
            }
        }
    }

    if pdfler.is_empty() {
        return (content_type.to_string(), body.to_vec());
    }
    if pdfler.len() == 1 {
        return (
            "application/pdf".to_string(),
            pdfler.into_iter().next().unwrap(),
        );
    }
    // Birden fazla PDF birleştirme: harici PDF birleştirici yoksa Python'daki `except Exception` gibi
    // orijinal zip gövdesi korunur.
    (content_type.to_string(), body.to_vec())
}

pub fn _ek_pdf_basliklari(
    headers: HashMap<String, String>,
    content_type: &str,
) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = headers
        .into_iter()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("content-type"))
        .collect();
    out.insert("content-type".to_string(), content_type.to_string());
    out
}

// --------------------------------------------------------------------------------------
// Auth & Local / LAN security
// --------------------------------------------------------------------------------------
pub const _LOCAL_HOSTS: &[&str] = &["127.0.0.1", "::1", "localhost"];

static _ALLOWED_HOSTNAMES: Lazy<HashSet<String>> = Lazy::new(|| {
    std::env::var("UYAP_ALLOWED_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect()
});

pub fn _hostname_of(value: &str) -> String {
    let mut v = value.trim();
    if v.is_empty() {
        return String::new();
    }
    if let Some((_, after)) = v.split_once("://") {
        v = after;
    }
    if let Some((before, _)) = v.split_once('/') {
        v = before;
    }
    if let Some(rest) = v.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("").to_string();
    }
    if v.matches(':').count() == 1 {
        if let Some((h, _)) = v.split_once(':') {
            v = h;
        }
    }
    v.to_string()
}

pub fn _is_ip_or_local(hostname: &str) -> bool {
    let h = hostname.trim().to_ascii_lowercase();
    if h.is_empty() || h == "localhost" {
        return true;
    }
    if _ALLOWED_HOSTNAMES.contains(&h) {
        return true;
    }
    h.parse::<IpAddr>().is_ok()
}

pub fn _local_token_file() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("USERPROFILE").ok())
            .unwrap_or_else(|| ".".to_string());
        PathBuf::from(base).join("UyapIcra").join("gw_local_token")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                format!("{}/.config", home)
            });
        PathBuf::from(base).join("uyapicra").join("gw_local_token")
    }
}

fn _windows_acl_sinirla(path: &Path) {
    #[cfg(windows)]
    {
        let kullanici = std::env::var("USERNAME").unwrap_or_default();
        let kullanici = kullanici.trim();
        if kullanici.is_empty() {
            return;
        }
        let grant = format!("{}:(F)", kullanici);
        let _ = std::process::Command::new("icacls")
            .args([
                path.to_string_lossy().as_ref(),
                "/inheritance:r",
                "/grant:r",
                &grant,
            ])
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

fn token_urlsafe(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

pub fn _init_local_token() -> String {
    let env_tok = std::env::var("UYAP_LOCAL_TOKEN")
        .unwrap_or_default()
        .trim()
        .to_string();
    let token = if env_tok.is_empty() {
        token_urlsafe(32)
    } else {
        env_tok
    };
    *GW_LOCAL_TOKEN.lock() = Some(token.clone());
    std::env::set_var("UYAP_LOCAL_TOKEN", &token);
    let path = _local_token_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&path, token.as_bytes()) {
        Ok(_) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            _windows_acl_sinirla(&path);
        }
        Err(e) => {
            println!(
                "[!] Yerel jeton dosyası yazılamadı ({}); aynı-süreç ortam değişkeni kullanılacak.",
                e
            );
        }
    }
    token
}

pub static GW_BROWSER_SECRET: Lazy<String> = Lazy::new(|| token_urlsafe(32));
pub const YEREL_CEREZ: &str = "uyap_yerel";
const _GIRIS_KOD_OMRU_SN: Duration = Duration::from_secs(60);
static _GIRIS_KODLARI: Lazy<Mutex<HashMap<String, Instant>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn _giris_kodu_uret() -> String {
    let simdi = Instant::now();
    let kod = token_urlsafe(24);
    let mut map = _GIRIS_KODLARI.lock();
    map.retain(|_, son| *son >= simdi);
    map.insert(kod.clone(), simdi + _GIRIS_KOD_OMRU_SN);
    kod
}

pub fn _giris_kodu_kullan(kod: &str) -> bool {
    if kod.is_empty() {
        return false;
    }
    let son = _GIRIS_KODLARI.lock().remove(kod);
    matches!(son, Some(t) if t >= Instant::now())
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq as _;
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

fn extract_cookie(headers: &HeaderMap, name: &str) -> String {
    for val in headers.get_all("cookie").iter() {
        if let Ok(s) = val.to_str() {
            for part in s.split(';') {
                let part = part.trim();
                if let Some((k, v)) = part.split_once('=') {
                    if k.trim() == name {
                        let mut v = v.trim();
                        if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
                            v = &v[1..v.len() - 1];
                        }
                        return v.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

fn _yerel_tarayici_cerezi_gecerli(headers: &HeaderMap) -> bool {
    let deger = extract_cookie(headers, YEREL_CEREZ);
    !deger.is_empty() && constant_time_eq(&deger, &GW_BROWSER_SECRET)
}

pub fn _guvenli_hedef(hedef: &str) -> String {
    let h = hedef.trim();
    if !h.starts_with('/') || h.starts_with("//") || h.starts_with("/\\") {
        return "/giris".to_string();
    }
    h.to_string()
}

pub static GW_LAN_SECRET: Lazy<String> = Lazy::new(|| token_urlsafe(32));
pub const LAN_CEREZ: &str = "uyap_lan";

fn _lan_cerezi_gecerli(headers: &HeaderMap) -> bool {
    let deger = extract_cookie(headers, LAN_CEREZ);
    !deger.is_empty()
        && (constant_time_eq(&deger, &GW_LAN_SECRET) || crate::kimlik::lan_cerezi_coz(&deger, &GW_LAN_SECRET).is_some())
}

/// İsteğin sahibi (İş Listesi; kimlik.rs). require_auth geçtikten sonra çağrılır: bu bilgisayardan gelen
/// istek programa girişli hesaptır; yerel ağdan gelen yalnız kişiye özel çerezle tanınır (ortak ofis
/// parolasıyla girilen eski çerez kişiyi söylemez).
fn istek_hesabi(headers: &HeaderMap, client_host: &str) -> Option<String> {
    if _LOCAL_HOSTS.contains(&client_host) {
        return crate::kimlik::ofis_hesabi();
    }
    crate::kimlik::lan_cerezi_coz(&extract_cookie(headers, LAN_CEREZ), &GW_LAN_SECRET)
}

fn html_escape(s: &str, quote: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quote => out.push_str("&quot;"),
            '\'' if quote => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn _lan_giris_sayfasi(hedef: &str, hata: &str) -> String {
    let hata_html = if hata.is_empty() {
        String::new()
    } else {
        format!(
            "<p style='background:#F7E8DE;border:1px solid #C18A66;color:#9C6B47;padding:10px;\
border-radius:8px'>{}</p>",
            html_escape(hata, false)
        )
    };
    format!(
        "<!doctype html><html lang='tr'><meta charset='utf-8'>\
<meta name='viewport' content='width=device-width,initial-scale=1'><title>UYAP — Ofis Girişi</title>\
<body style='font-family:Segoe UI,Arial,sans-serif;background:#F1EEE8;color:#43423D;margin:0;padding:24px'>\
<div style='max-width:420px;margin:48px auto;background:#FCFBF8;border:1px solid #E8E2D7;\
border-radius:16px;padding:24px 26px;box-shadow:0 14px 40px rgba(96,86,66,.14)'>\
<h2 style='margin-top:0'>Ofis UYAP Girişi</h2>\
<p style='font-size:13px;color:#8C867B'>Bu bilgisayar ofisin UYAP oturumunu yerel ağda paylaşıyor. \
Programdan (<b>Bağlantıyı Al → Tarayıcıyı Aç</b>) bağlanıyorsanız bu ekranı görmezsiniz. \
Doğrudan bağlanmak için <b>kendi Çoklu UYAP kullanıcı adınızı ve parolanızı</b> yazın (İş Listesi işlerinizi sizin adınıza gösterir).</p>\
{}\
<form method='post' action='/__uyap_lan__/giris' autocomplete='off'>\
<input type='hidden' name='hedef' value='{}'>\
<label style='font-size:12px;color:#8C867B'>Kullanıcı adı</label>\
<input name='kullanici' required autofocus style='width:100%;box-sizing:border-box;padding:10px;\
border:1px solid #E3DDD2;border-radius:9px;margin:4px 0 12px'>\
<label style='font-size:12px;color:#8C867B'>Parola</label>\
<input name='parola' type='password' required style='width:100%;box-sizing:border-box;padding:10px;\
border:1px solid #E3DDD2;border-radius:9px;margin:4px 0 16px'>\
<button style='width:100%;background:#7C9A7E;color:#fff;border:0;padding:12px;border-radius:11px;\
font-weight:600;cursor:pointer'>Giriş</button></form>\
<p style='font-size:11px;color:#B6AFA2;margin-bottom:0'>Not: yerel ağ bağlantısı şifrelenmemiş HTTP'dir; \
yalnız güvendiğiniz ofis ağında kullanın.</p></div></body></html>",
        hata_html,
        html_escape(hedef, true)
    )
}

const _YEREL_GIRIS_SAYFASI: &str = "<!doctype html><html lang='tr'><meta charset='utf-8'><title>UYAP — programdan açın</title>\
<body style='font-family:Segoe UI,Arial,sans-serif;max-width:560px;margin:60px auto;line-height:1.5'>\
<h2>UYAP'ı programdan açın</h2>\
<p>Güvenlik nedeniyle bu adres artık doğrudan (yer imi ya da elle yazarak) açılamıyor.</p>\
<p>UyapMasa'daki <b>UYAP Bağlantısı</b> ekranında <b>“Tarayıcıda Aç”</b> düğmesine basın. \
Program yeniden başlatıldıysa açık sekmeleri de bu düğmeyle yeniden açmanız gerekir.</p>\
</body></html>";

const _AUTH_PENCERE_SN: Duration = Duration::from_secs(15 * 60);
const _AUTH_AZAMI_HATA: usize = 10;
static _AUTH_HATALARI: Lazy<Mutex<HashMap<String, Vec<Instant>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn _auth_engelli_mi(ip: &str) -> bool {
    let simdi = Instant::now();
    let mut map = _AUTH_HATALARI.lock();
    if let Some(v) = map.get_mut(ip) {
        v.retain(|t| simdi.saturating_duration_since(*t) < _AUTH_PENCERE_SN);
        let len = v.len();
        if len == 0 {
            map.remove(ip);
            return false;
        }
        return len >= _AUTH_AZAMI_HATA;
    }
    false
}

fn _auth_hata_kaydet(ip: &str) {
    let mut map = _AUTH_HATALARI.lock();
    map.entry(ip.to_string()).or_default().push(Instant::now());
}

fn parse_basic_auth(headers: &HeaderMap) -> Option<(String, String)> {
    let auth = headers.get("authorization")?.to_str().ok()?.trim();
    let (scheme, rest) = auth.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(rest.trim())
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

enum AuthRejection {
    YerelGirisGerekli,
    LanGirisGerekli(String),
    Http(StatusCode, &'static str, Option<(&'static str, &'static str)>),
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        match self {
            AuthRejection::YerelGirisGerekli => Response::builder()
                .status(StatusCode::FORBIDDEN)
                .header("Content-Type", "text/html; charset=utf-8")
                .header("Cache-Control", "no-store")
                .body(Body::from(_YEREL_GIRIS_SAYFASI.as_bytes()))
                .unwrap(),
            AuthRejection::LanGirisGerekli(hedef) => {
                _lan_sayfa(&_lan_giris_sayfasi(&_guvenli_hedef(&hedef), ""), StatusCode::UNAUTHORIZED)
            }
            AuthRejection::Http(status, detail, extra_header) => {
                let mut builder = Response::builder()
                    .status(status)
                    .header("Content-Type", "application/json; charset=utf-8");
                if let Some((hk, hv)) = extra_header {
                    builder = builder.header(hk, hv);
                }
                let body = serde_json::json!({ "detail": detail }).to_string();
                builder.body(Body::from(body)).unwrap()
            }
        }
    }
}

fn require_auth(
    headers: &HeaderMap,
    method: &Method,
    path_and_query: &str,
    client_host: &str,
) -> Result<String, AuthRejection> {
    let host_hdr = headers.get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
    let host_hn = _hostname_of(host_hdr);
    if !host_hn.is_empty() && !_is_ip_or_local(&host_hn) {
        return Err(AuthRejection::Http(
            StatusCode::FORBIDDEN,
            "Beklenmeyen Host başlığı (olası DNS rebinding).",
            None,
        ));
    }

    let sfs = headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if sfs == "cross-site" {
        let mode = headers
            .get("sec-fetch-mode")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let dest = headers
            .get("sec-fetch-dest")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let is_safe_nav = mode == "navigate"
            && (dest == "document" || dest.is_empty())
            && (*method == Method::GET || *method == Method::HEAD);
        if !is_safe_nav {
            return Err(AuthRejection::Http(
                StatusCode::FORBIDDEN,
                "Çapraz-site alt-kaynak isteği reddedildi.",
                None,
            ));
        }
    }

    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if origin.eq_ignore_ascii_case("null") || !_is_ip_or_local(&_hostname_of(origin)) {
            return Err(AuthRejection::Http(
                StatusCode::FORBIDDEN,
                "Yetkisiz Origin (çapraz-site istek reddedildi).",
                None,
            ));
        }
    }

    let gw_user = GW_USER.lock().clone();
    let gw_pass = GW_PASS.lock().clone();
    let gw_local_token = GW_LOCAL_TOKEN.lock().clone();
    let gw_lan_ticket = GW_LAN_TICKET.lock().clone();

    if _LOCAL_HOSTS.contains(&client_host) {
        let presented = headers
            .get("x-uyap-local-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !presented.is_empty() {
            if let Some(ref tok) = gw_local_token {
                if constant_time_eq(presented, tok) {
                    return Ok(gw_user);
                }
            }
        }
        if _yerel_tarayici_cerezi_gecerli(headers) {
            return Ok(gw_user);
        }
        let accept = headers.get("accept").and_then(|v| v.to_str().ok()).unwrap_or("");
        if (*method == Method::GET || *method == Method::HEAD) && accept.contains("text/html") {
            return Err(AuthRejection::YerelGirisGerekli);
        }
        return Err(AuthRejection::Http(
            StatusCode::FORBIDDEN,
            "Yerel yetki gerekli: program jetonu ya da 'Tarayıcıda Aç' ile verilen çerez.",
            None,
        ));
    }

    if _lan_cerezi_gecerli(headers) {
        return Ok(gw_user);
    }

    if _auth_engelli_mi(client_host) {
        return Err(AuthRejection::Http(
            StatusCode::TOO_MANY_REQUESTS,
            "Çok fazla hatalı giriş denemesi; 15 dakika sonra yeniden deneyin.",
            None,
        ));
    }

    let creds = parse_basic_auth(headers);
    if let (Some((ref cu, ref cp)), Some(ref ticket)) = (&creds, &gw_lan_ticket) {
        if constant_time_eq(cp, ticket) {
            return Ok(if cu.is_empty() { "lan".to_string() } else { cu.clone() });
        }
    }

    let expected_pass = match gw_pass {
        Some(ref p) if !p.is_empty() => p.clone(),
        _ => {
            return Err(AuthRejection::Http(
                StatusCode::SERVICE_UNAVAILABLE,
                "Sunucu parolası (GW_PASS ortam değişkeni) ayarlanmamış.",
                None,
            ));
        }
    };

    let (cu, cp) = match creds {
        Some(c) => c,
        None => {
            let accept = headers.get("accept").and_then(|v| v.to_str().ok()).unwrap_or("");
            if (*method == Method::GET || *method == Method::HEAD) && accept.contains("text/html") {
                return Err(AuthRejection::LanGirisGerekli(path_and_query.to_string()));
            }
            return Err(AuthRejection::Http(
                StatusCode::UNAUTHORIZED,
                "Kimlik bilgisi gerekli.",
                Some(("WWW-Authenticate", "Basic")),
            ));
        }
    };

    let ok_user = constant_time_eq(&cu, &gw_user);
    let ok_pass = constant_time_eq(&cp, &expected_pass);
    if !(ok_user && ok_pass) {
        _auth_hata_kaydet(client_host);
        return Err(AuthRejection::Http(
            StatusCode::UNAUTHORIZED,
            "Yetkisiz erişim.",
            Some(("WWW-Authenticate", "Basic")),
        ));
    }
    Ok(cu)
}

// --------------------------------------------------------------------------------------
// Keepalive loop & Axum app
// --------------------------------------------------------------------------------------
pub async fn _keepalive_loop(session: Arc<GatewaySession>, idle_interval: u64) {
    loop {
        tokio::time::sleep(Duration::from_secs(idle_interval)).await;
        let elapsed = session.last_request_ts.lock().elapsed();
        if elapsed < Duration::from_secs(idle_interval) {
            continue;
        }
        let res = {
            let _guard = crate::runtime::batch_write().await;
            session.ensure_alive().await
        };
        if let Err(e) = res {
            println!("[!] Keepalive hatası: {}", e);
            if session.giris_hatasi.lock().is_some() {
                println!("[!] Otomatik giriş denemeleri durduruldu; yeniden denemek için paylaşımı yeniden başlatın.");
                return;
            }
        }
    }
}

#[derive(Clone, Default)]
struct AppRuntimeState {
    is_lan_tls: bool,
    tls_peer_map: Arc<Mutex<HashMap<u16, IpAddr>>>,
}

fn resolve_client_host(
    state: &AppRuntimeState,
    connect_info: Option<&axum::Extension<ConnectInfo<SocketAddr>>>,
) -> String {
    match connect_info {
        Some(axum::Extension(ConnectInfo(addr))) => {
            if state.is_lan_tls {
                if let Some(real_ip) = state.tls_peer_map.lock().get(&addr.port()).copied() {
                    return real_ip.to_string();
                }
                return "lan-tls".to_string();
            }
            addr.ip().to_string()
        }
        None => String::new(),
    }
}

fn resolve_scheme(state: &AppRuntimeState, headers: &HeaderMap) -> &'static str {
    if state.is_lan_tls {
        return "https";
    }
    if let Some(xfp) = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) {
        if xfp.trim().eq_ignore_ascii_case("https") {
            return "https";
        }
    }
    "http"
}

fn tuple_to_axum_response(status: u16, headers: HashMap<String, String>, body: Vec<u8>) -> Response {
    let mut builder = Response::builder().status(status);
    for (k, v) in headers {
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_bytes(v.as_bytes()),
        ) {
            builder = builder.header(hn, hv);
        }
    }
    builder.body(Body::from(body)).unwrap()
}

async fn lan_ping(
    axum::extract::State(state): axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    req: AxumRequest,
) -> Result<Response, AuthRejection> {
    let client_host = resolve_client_host(&state, connect_info.as_ref());
    let pq = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let _user = require_auth(req.headers(), req.method(), pq, &client_host)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Body::from("ok"))
        .unwrap())
}

static _AGENT_PING_ORIGINS: Lazy<HashSet<String>> = Lazy::new(|| {
    let mut set: HashSet<String> = [
        "https://www.cokluuyap.com",
        "https://cokluuyap.com",
        "http://127.0.0.1:8080",
        "http://localhost:8080",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Ok(extra) = std::env::var("UYAP_AGENT_PING_ORIGINS") {
        for o in extra.split(',') {
            let clean = o.trim().trim_end_matches('/').to_string();
            if !clean.is_empty() {
                set.insert(clean);
            }
        }
    }
    set
});

/// cokluuyap.com gibi yerel programa istek atmasına izin verilen köken mi (ping ve uzak yardımcı aynı listeyi kullanır).
pub fn agent_koken_izinli(origin: &str) -> bool {
    _AGENT_PING_ORIGINS.contains(origin.trim_end_matches('/'))
}

pub(crate) fn _agent_ping_headers(headers: &HeaderMap) -> Vec<(&'static str, String)> {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let mut h = vec![("Cache-Control", "no-store".to_string())];
    if _AGENT_PING_ORIGINS.contains(&origin) {
        h.push(("Access-Control-Allow-Origin", origin));
        h.push(("Vary", "Origin".to_string()));
        h.push(("Access-Control-Allow-Methods", "GET, OPTIONS".to_string()));
        h.push(("Access-Control-Allow-Private-Network", "true".to_string()));
        h.push(("Access-Control-Max-Age", "600".to_string()));
    }
    h
}

pub(crate) async fn agent_ping_preflight(headers: HeaderMap) -> Response {
    let mut builder = Response::builder().status(StatusCode::NO_CONTENT);
    for (k, v) in _agent_ping_headers(&headers) {
        builder = builder.header(k, v);
    }
    builder.body(Body::empty()).unwrap()
}

pub(crate) async fn agent_ping(headers: HeaderMap) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json");
    for (k, v) in _agent_ping_headers(&headers) {
        builder = builder.header(k, v);
    }
    builder
        .body(Body::from(r#"{"app":"cokluuyap-ofis","sharing":true}"#))
        .unwrap()
}

fn _yerel_istemci_mi(client_host: &str, headers: &HeaderMap) -> bool {
    let host_hn = _hostname_of(
        headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    );
    _LOCAL_HOSTS.contains(&client_host) && (host_hn.is_empty() || _is_ip_or_local(&host_hn))
}

async fn yerel_giris_kodu(
    axum::extract::State(state): axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
) -> Response {
    let client_host = resolve_client_host(&state, connect_info.as_ref());
    let presented = headers
        .get("x-uyap-local-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected = GW_LOCAL_TOKEN.lock().clone();
    let ok = _yerel_istemci_mi(&client_host, &headers)
        && !presented.is_empty()
        && expected
            .as_deref()
            .map(|tok| constant_time_eq(presented, tok))
            .unwrap_or(false);
    if !ok {
        return AuthRejection::Http(
            StatusCode::FORBIDDEN,
            "Yerel yetki jetonu gerekli.",
            None,
        )
        .into_response();
    }
    let body = serde_json::json!({ "kod": _giris_kodu_uret() }).to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .body(Body::from(body))
        .unwrap()
}

/// Sayfadaki "Paylaş" düğmesi için: gövdedeki UDF baytlarını bu bilgisayarda PDF'e çevirip döner.
/// Hiçbir yere gönderilmez, saklanmaz; yalnız oturumu açık sayfa çağırabilir.
async fn udf_pdf_cevir(
    st: axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    req: AxumRequest,
) -> Response {
    udf_isle(st, connect_info, req, false).await
}

/// "UDF İndir" için: portal belgesindeki alan adlarının yerine değerleri yerleştirilmiş kopya (udf_alan.rs).
/// Doldurulacak alan yoksa 204 (sayfa asıl dosyayı kullanır).
async fn udf_doldur_cevir(
    st: axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    req: AxumRequest,
) -> Response {
    udf_isle(st, connect_info, req, true).await
}

async fn udf_isle(
    axum::extract::State(state): axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    req: AxumRequest,
    doldur: bool,
) -> Response {
    let client_host = resolve_client_host(&state, connect_info.as_ref());
    let pq = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    if let Err(rej) = require_auth(req.headers(), &Method::POST, &pq, &client_host) {
        return rej.into_response();
    }
    let hata = |kod: StatusCode, m: &str| {
        Response::builder()
            .status(kod)
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("Cache-Control", "no-store")
            .body(Body::from(m.to_string()))
            .unwrap()
    };
    let govde = match axum::body::to_bytes(req.into_body(), 40 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return hata(StatusCode::PAYLOAD_TOO_LARGE, "Belge çok büyük."),
    };
    if !crate::udf_pdf::udf_zip_mi(&govde) {
        return hata(StatusCode::UNSUPPORTED_MEDIA_TYPE, "UDF değil.");
    }
    if doldur {
        return match tokio::task::spawn_blocking(move || crate::udf_alan::doldur(&govde)).await {
            Ok(Ok(Some(udf))) => Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/octet-stream")
                .header("Cache-Control", "no-store")
                .body(Body::from(udf))
                .unwrap(),
            Ok(Ok(None)) => Response::builder().status(StatusCode::NO_CONTENT).body(Body::empty()).unwrap(),
            Ok(Err(e)) => hata(StatusCode::UNPROCESSABLE_ENTITY, &e),
            Err(_) => hata(StatusCode::INTERNAL_SERVER_ERROR, "İşlem yarıda kaldı."),
        };
    }
    match tokio::task::spawn_blocking(move || crate::udf_pdf::udf_pdf_uret(&govde)).await {
        Ok(Ok(pdf)) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/pdf")
            .header("Cache-Control", "no-store")
            .body(Body::from(pdf))
            .unwrap(),
        Ok(Err(e)) => hata(StatusCode::UNPROCESSABLE_ENTITY, &e),
        Err(_) => hata(StatusCode::INTERNAL_SERVER_ERROR, "Çevirme yarıda kaldı."),
    }
}

#[derive(Deserialize)]
struct PaylasQuery {
    #[serde(default)]
    ad: String,
    #[serde(default)]
    aciklama: String,
    /// "pano": pencere açmadan belgeyi dosya olarak panoya koy (tek tık paylaşım, 17.11.2026).
    #[serde(default)]
    kip: String,
}

/// Sayfadaki "Paylaş": tarayıcının paylaştırmadığı türler (UDF …) için belgeyi geçici klasöre yazıp
/// Windows'un paylaşım penceresini açar. Yalnız bu bilgisayardaki tarayıcı çağırabilir (pencere burada açılır).
async fn yerel_paylas(
    axum::extract::State(state): axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Query(q): Query<PaylasQuery>,
    req: AxumRequest,
) -> Response {
    let client_host = resolve_client_host(&state, connect_info.as_ref());
    let pq = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    if let Err(rej) = require_auth(req.headers(), &Method::POST, &pq, &client_host) {
        return rej.into_response();
    }
    let yanit = |kod: StatusCode, m: &str| {
        Response::builder()
            .status(kod)
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("Cache-Control", "no-store")
            .body(Body::from(m.to_string()))
            .unwrap()
    };
    if !_yerel_istemci_mi(&client_host, req.headers()) {
        return yanit(StatusCode::FORBIDDEN, "Paylaşım penceresi yalnız bu bilgisayarda açılabilir.");
    }
    // Ad / açıklama Base64 (UTF-8) başlıkta: adres satırından geçen Türkçe harfler bozuluyordu
    // (canlı 17.11.2026: "Reddiyat Beyanı.pdf" → "Reddiyat Beyan\u{FFFD}.pdf"). Yoksa eski sorgu parametresi.
    let q = {
        let h = req.headers();
        let b64 = |ad: &str| -> Option<String> {
            let v = h.get(ad)?.to_str().ok()?;
            let b = base64::engine::general_purpose::STANDARD.decode(v.trim()).ok()?;
            String::from_utf8(b).ok().filter(|s| !s.trim().is_empty())
        };
        PaylasQuery {
            ad: b64("x-cu-ad").unwrap_or(q.ad),
            aciklama: b64("x-cu-aciklama").unwrap_or(q.aciklama),
            kip: q.kip,
        }
    };
    let govde = match axum::body::to_bytes(req.into_body(), 60 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return yanit(StatusCode::PAYLOAD_TOO_LARGE, "Belge çok büyük."),
    };
    if govde.is_empty() {
        return yanit(StatusCode::BAD_REQUEST, "Belge boş.");
    }
    let ad = crate::paylas_win::guvenli_ad(&q.ad);
    if q.kip == "pano" {
        let sonuc = tokio::task::spawn_blocking(move || crate::paylas_win::panoya_kopyala(&ad, &govde)).await;
        return match sonuc {
            Ok(Ok(_)) => yanit(StatusCode::OK, "pano"),
            Ok(Err(e)) => yanit(StatusCode::INTERNAL_SERVER_ERROR, &e),
            Err(_) => yanit(StatusCode::INTERNAL_SERVER_ERROR, "Kopyalama yarıda kaldı."),
        };
    }
    // Windows paylaşım penceresi yolu kaldırıldı (17.11.2026): yalnız panoya kopyalama.
    yanit(StatusCode::BAD_REQUEST, "Desteklenmeyen paylaşım kipi.")
}

#[derive(Deserialize)]
struct YerelGirisQuery {
    #[serde(default)]
    k: String,
    #[serde(default = "default_hedef")]
    hedef: String,
}

fn default_hedef() -> String {
    "/giris".to_string()
}

async fn yerel_giris(
    axum::extract::State(state): axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Query(q): Query<YerelGirisQuery>,
    headers: HeaderMap,
) -> Response {
    let client_host = resolve_client_host(&state, connect_info.as_ref());
    if !_yerel_istemci_mi(&client_host, &headers) || !_giris_kodu_kullan(&q.k) {
        return AuthRejection::YerelGirisGerekli.into_response();
    }
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Strict; Path=/",
        YEREL_CEREZ, *GW_BROWSER_SECRET
    );
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", _guvenli_hedef(&q.hedef))
        .header("Cache-Control", "no-store")
        .header("Set-Cookie", cookie)
        .body(Body::empty())
        .unwrap()
}

fn _lan_sayfa(icerik: &str, status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-store")
        .header("X-Frame-Options", "DENY")
        .header(
            "Content-Security-Policy",
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'",
        )
        .body(Body::from(icerik.as_bytes().to_vec()))
        .unwrap()
}

fn _lan_istegi_reddi(headers: &HeaderMap) -> Option<&'static str> {
    let host_hn = _hostname_of(
        headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    );
    if !host_hn.is_empty() && !_is_ip_or_local(&host_hn) {
        return Some("Beklenmeyen Host başlığı.");
    }
    let sfs = headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if sfs == "cross-site" {
        return Some("Çapraz-site istek reddedildi.");
    }
    None
}

#[derive(Deserialize)]
struct LanHedefQuery {
    #[serde(default = "default_hedef")]
    hedef: String,
}

async fn lan_giris_formu(Query(q): Query<LanHedefQuery>, headers: HeaderMap) -> Response {
    if let Some(red) = _lan_istegi_reddi(&headers) {
        return AuthRejection::Http(StatusCode::FORBIDDEN, red, None).into_response();
    }
    _lan_sayfa(&_lan_giris_sayfasi(&_guvenli_hedef(&q.hedef), ""), StatusCode::OK)
}

async fn lan_giris(
    axum::extract::State(state): axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    req: AxumRequest,
) -> Response {
    let client_host = resolve_client_host(&state, connect_info.as_ref());
    if let Some(red) = _lan_istegi_reddi(req.headers()) {
        return AuthRejection::Http(StatusCode::FORBIDDEN, red, None).into_response();
    }
    let scheme = resolve_scheme(&state, req.headers());
    let body_bytes = match axum::body::to_bytes(req.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return _lan_sayfa(
                &_lan_giris_sayfasi("/giris", "Kullanıcı adı veya parola hatalı."),
                StatusCode::UNAUTHORIZED,
            )
        }
    };
    let mut kullanici = String::new();
    let mut parola = String::new();
    let mut hedef = "/giris".to_string();
    for (k, v) in url::form_urlencoded::parse(&body_bytes) {
        match k.as_ref() {
            "kullanici" => kullanici = v.trim().to_string(),
            "parola" => parola = v.into_owned(),
            "hedef" => hedef = _guvenli_hedef(&v),
            _ => {}
        }
    }
    if _auth_engelli_mi(&client_host) {
        return _lan_sayfa(
            &_lan_giris_sayfasi(
                &hedef,
                "Çok fazla hatalı deneme; 15 dakika sonra yeniden deneyin.",
            ),
            StatusCode::TOO_MANY_REQUESTS,
        );
    }
    let gw_user = GW_USER.lock().clone();
    let gw_pass = GW_PASS.lock().clone();
    let ofis_girisi = match gw_pass {
        Some(ref p) if !p.is_empty() => {
            constant_time_eq(&kullanici, &gw_user) && constant_time_eq(&parola, p)
        }
        _ => false,
    };
    // Kişi: ofis programının hesabıyla girildiyse o hesap; değilse büro üyesinin kendi hesabı bulutta
    // doğrulanır (yalnız aynı büro; parola saklanmaz).
    let hesap: Option<String> = if ofis_girisi {
        crate::kimlik::ofis_hesabi()
    } else if kullanici.is_empty() || parola.is_empty() {
        None
    } else {
        match crate::is_listesi::hesap_dogrula(&kullanici, &parola).await {
            Ok(h) => {
                let buro = |x: &str| x.rsplit_once('@').map(|(_, b)| b.to_lowercase()).unwrap_or_default();
                match crate::kimlik::ofis_hesabi() {
                    Some(o) if buro(&o) == buro(&h) => Some(h),
                    _ => None,
                }
            }
            Err(_) => None,
        }
    };
    if !ofis_girisi && hesap.is_none() {
        _auth_hata_kaydet(&client_host);
        return _lan_sayfa(
            &_lan_giris_sayfasi(&hedef, "Kullanıcı adı veya parola hatalı."),
            StatusCode::UNAUTHORIZED,
        );
    }
    let secure_attr = if scheme == "https" { "; Secure" } else { "" };
    let deger = match &hesap {
        Some(h) => crate::kimlik::lan_cerezi(h, &GW_LAN_SECRET),
        None => GW_LAN_SECRET.to_string(),
    };
    let cookie = format!("{}={}; HttpOnly; SameSite=Strict; Path=/{}", LAN_CEREZ, deger, secure_attr);
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", hedef)
        .header("Cache-Control", "no-store")
        .header("Set-Cookie", cookie)
        .body(Body::empty())
        .unwrap()
}

async fn _forward_once(
    gw: &GatewaySession,
    req_method: &reqwest::Method,
    target: &str,
    fwd_headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> Result<reqwest::Response, reqwest::Error> {
    gw.client
        .request(req_method.clone(), target)
        .headers(fwd_headers.clone())
        .body(body.to_vec())
        .send()
        .await
}

async fn _forward_with_relogin(
    gw: &GatewaySession,
    req_method: &reqwest::Method,
    target: &str,
    fwd_headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> Result<(u16, reqwest::header::HeaderMap, Vec<u8>), reqwest::Error> {
    let _izi = uyap_istek_izi().await;
    let resp = _forward_once(gw, req_method, target, fwd_headers, body).await?;
    let status = resp.status().as_u16();
    let resp_headers = resp.headers().clone();
    let resp_bytes = resp.bytes().await?.to_vec();
    if _looks_like_login_wall(status, &resp_headers) {
        let _ = gw.relogin().await;
        let resp2 = _forward_once(gw, req_method, target, fwd_headers, body).await?;
        let s2 = resp2.status().as_u16();
        let h2 = resp2.headers().clone();
        let b2 = resp2.bytes().await?.to_vec();
        Ok((s2, h2, b2))
    } else {
        Ok((status, resp_headers, resp_bytes))
    }
}

async fn proxy_handler(
    axum::extract::State(state): axum::extract::State<AppRuntimeState>,
    connect_info: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    req: AxumRequest,
) -> Response {
    let client_host = resolve_client_host(&state, connect_info.as_ref());
    let method = req.method().clone();
    let uri = req.uri().clone();
    let full_path = uri.path().trim_start_matches('/').to_string();
    let query = uri.query().unwrap_or("").to_string();
    let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");

    let user = match require_auth(req.headers(), &method, pq, &client_host) {
        Ok(u) => u,
        Err(rej) => return rej.into_response(),
    };
    let hesap = istek_hesabi(req.headers(), &client_host);

    let req_headers_axum = req.headers().clone();
    let scheme = resolve_scheme(&state, &req_headers_axum);
    let body = match axum::body::to_bytes(req.into_body(), 100 * 1024 * 1024).await {
        Ok(b) => b.to_vec(),
        Err(e) => {
            return AuthRejection::Http(
                StatusCode::BAD_REQUEST,
                Box::leak(format!("Gövde okunamadı: {}", e).into_boxed_str()),
                None,
            )
            .into_response();
        }
    };

    // Ortak tema (2026-11-14): masaüstü program ile bu bilgisayardaki web ekranı aynı temayı kullanır.
    // Seçim %LOCALAPPDATA%/CokluUyap/tema.txt'de; yalnız bu bilgisayardan (tünel/LAN üyeleri değil).
    if full_path == "__cu__/tema" {
        if !_yerel_istemci_mi(&client_host, &req_headers_axum) {
            return Response::builder().status(StatusCode::NOT_FOUND).body(Body::from("Bulunamadi.")).unwrap();
        }
        if method == axum::http::Method::POST {
            let capraz = req_headers_axum
                .get("sec-fetch-site")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.eq_ignore_ascii_case("cross-site"))
                .unwrap_or(false);
            let ad = String::from_utf8_lossy(&body).trim().to_string();
            if capraz || !crate::uyap_theme::TEMALAR.iter().any(|t| t.0 == ad) {
                return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::from("")).unwrap();
            }
            crate::uyap_theme::tema_dosyaya_yaz(&ad);
        }
        let ad = crate::uyap_theme::tema_dosyadan_oku().unwrap_or_default();
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("Cache-Control", "no-store")
            .body(Body::from(ad))
            .unwrap();
    }

    // Güncelleme (2026-11-23, src/guncelleme.rs): durum ve onaylı kurulum. Yalnız bu bilgisayardan; kurulum POST'u
    // başka siteden gelemez.
    if full_path == "__cu__/guncelleme" || full_path == "__cu__/guncelleme/kur" {
        if !_yerel_istemci_mi(&client_host, &req_headers_axum) {
            return Response::builder().status(StatusCode::NOT_FOUND).body(Body::from("Bulunamadi.")).unwrap();
        }
        let json = |kod: StatusCode, v: serde_json::Value| {
            Response::builder()
                .status(kod)
                .header("Content-Type", "application/json; charset=utf-8")
                .header("Cache-Control", "no-store")
                .body(Body::from(v.to_string()))
                .unwrap()
        };
        if full_path == "__cu__/guncelleme/kur" {
            let capraz = req_headers_axum
                .get("sec-fetch-site")
                .and_then(|v| v.to_str().ok())
                .map(|v| !v.eq_ignore_ascii_case("same-origin"))
                .unwrap_or(false);
            if method != axum::http::Method::POST || capraz {
                return json(StatusCode::BAD_REQUEST, serde_json::json!({ "hata": "Geçersiz istek." }));
            }
            return match crate::guncelleme::kur() {
                Ok(()) => json(StatusCode::OK, crate::guncelleme::durum_json()),
                Err(e) => json(StatusCode::CONFLICT, serde_json::json!({ "hata": e })),
            };
        }
        if query.split('&').any(|p| p == "yenile=1") {
            crate::guncelleme::simdi_denetle();
        }
        return json(StatusCode::OK, crate::guncelleme::durum_json());
    }

    // Armonia Yazı editörü (2026-11-23, src/yazi_sunucu.rs): açılır penceredeki çerçeve buradan yüklenir.
    if crate::yazi_sunucu::is_path(&full_path) {
        let (status, out_headers, out_body) = crate::yazi_sunucu::yanit(method.as_str(), &full_path);
        return tuple_to_axum_response(status, out_headers, out_body);
    }

    // İş Listesi (büro veritabanı, kişiye göre): icra_api'den ÖNCE (aynı ön ek).
    if crate::is_api::is_path(&full_path) {
        let capraz = req_headers_axum
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("cross-site"))
            .unwrap_or(false);
        let (status, out_headers, out_body) =
            crate::is_api::handle(method.as_str(), &full_path, &query, &body, capraz, hesap.as_deref()).await;
        return tuple_to_axum_response(status, out_headers, out_body);
    }

    // Büro veritabanı uçları (İcra Takip Süreçleri sayfası): bütün büro kullanıcılarına açık (kullanıcı kararı
    // 15.11.2026; kimlik yukarıda require_auth ile doğrulandı). UYAP'a gitmez. Tünelden gelenler office_agent'ta.
    if crate::icra_api::is_path(&full_path) {
        let capraz = req_headers_axum
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("cross-site"))
            .unwrap_or(false);
        let (status, out_headers, out_body) =
            crate::icra_api::handle(method.as_str(), &full_path, &query, &body, capraz).await;
        return tuple_to_axum_response(status, out_headers, out_body);
    }

    // Buluttaki eski İş Listesi köprüsü ve sayfaları 17.11.2026'da kaldırıldı: bu yollar UYAP'a iletilmez.
    if _eski_is_listesi_yolu(&full_path) {
        return Response::builder()
            .status(StatusCode::GONE)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(Body::from("İş Listesi artık Çoklu UYAP'ta sol menüde."))
            .unwrap();
    }

    if is_gecit_path(&full_path) {
        let headers_dict: HashMap<String, String> = req_headers_axum
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    String::from_utf8_lossy(v.as_bytes()).to_string(),
                )
            })
            .collect();
        let (status, out_headers, out_body) =
            handle_gecit_proxy(method.as_str(), &full_path, &query, &body, &headers_dict).await;
        return tuple_to_axum_response(status, out_headers, out_body);
    }

    if jobs::is_control_path(&full_path) {
        let kaynak = if _LOCAL_HOSTS.contains(&client_host.as_str()) {
            "yerel".to_string()
        } else {
            format!("LAN:{}", user)
        };
        let (status, out_headers, out_body) =
            jobs::handle_control(method.as_str(), &full_path, &query, &body, Some(&kaynak)).await;
        return tuple_to_axum_response(status, out_headers, out_body);
    }

    if let Some((status, out_headers, out_body)) = kesilen_uc_yaniti(&full_path) {
        return tuple_to_axum_response(status, out_headers, out_body);
    }

    let gw = match get_gw() {
        Some(g) => g,
        None => {
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("GatewaySession başlatılmamış."))
                .unwrap();
        }
    };
    gw.touch();

    let target = if query.is_empty() {
        format!("{}/{}", UYAP_BASE, full_path)
    } else {
        format!("{}/{}?{}", UYAP_BASE, full_path, query)
    };

    let fwd_headers = _build_forward_headers(&req_headers_axum);

    let proxy_base = if let Some(xf_host) = req_headers_axum
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
    {
        let xf_proto = req_headers_axum
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");
        format!("{}://{}", xf_proto, xf_host)
            .trim_end_matches('/')
            .to_string()
    } else {
        let host = req_headers_axum
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("127.0.0.1:8800");
        format!("{}://{}", scheme, host)
            .trim_end_matches('/')
            .to_string()
    };

    let req_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    // Sayfa gezinmesi (adres çubuğu, yenileme): oturum yoksa önce giriş denenir; olmazsa UYAP'a hiç
    // gidilmez. UYAP kökü ("/") oturumsuzken de 200 dönüp boş bir uygulama iskeleti çiziyordu.
    let sayfa_gezinmesi = method == axum::http::Method::GET
        && req_headers_axum
            .get("sec-fetch-dest")
            .and_then(|v| v.to_str().ok())
            .map(|d| d == "document")
            .unwrap_or_else(|| {
                req_headers_axum
                    .get("accept")
                    .and_then(|v| v.to_str().ok())
                    .map(|a| a.contains("text/html"))
                    .unwrap_or(false)
            });
    if sayfa_gezinmesi && !gw.logged_in.load(Ordering::SeqCst) {
        if let Err(e) = gw.ensure_alive().await {
            return _oturum_yok_sayfasi(Some(&e));
        }
    }

    let res = if is_write_method(method.as_str()) {
        let _guard = interactive_write().await;
        _forward_with_relogin(&gw, &req_method, &target, &fwd_headers, &body).await
    } else {
        _forward_with_relogin(&gw, &req_method, &target, &fwd_headers, &body).await
    };

    let (status, resp_headers, resp_bytes) = match res {
        Ok(v) => v,
        Err(e) => {
            let detail = serde_json::json!({ "detail": format!("UYAP'a ulaşılamadı: {}", e) }).to_string();
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .header("Content-Type", "application/json; charset=utf-8")
                .body(Body::from(detail))
                .unwrap();
        }
    };

    // Oturum yokken UYAP'ın kendi giriş sayfası tarayıcıya gitmesin (fikri mülkiyet; kullanıcı isteği
    // 2026-11-14): yeniden giriş de başarısızsa sayfa gezinmesine kendi "oturum açık değil" sayfamız döner.
    let giris_yolu = full_path == "giris" || full_path.starts_with("giris/") || full_path.starts_with("giris?");
    if sayfa_gezinmesi
        && (_looks_like_login_wall(status, &resp_headers)
            || (giris_yolu && !gw.logged_in.load(Ordering::SeqCst)))
    {
        let neden = gw.giris_hatasi.lock().clone();
        return _oturum_yok_sayfasi(neden.as_deref());
    }

    let orig_ctype = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let (ctype, govde) = _ek_zip_pdf_ac(&full_path, &orig_ctype, &resp_bytes);
    let out_body = _rewrite_body(&ctype, &govde, &proxy_base, true);
    let mut out_headers = _build_response_headers_from_map(&resp_headers, &proxy_base);
    if ctype != orig_ctype {
        out_headers = _ek_pdf_basliklari(out_headers, &ctype);
    } else if !ctype.is_empty() && !out_headers.keys().any(|k| k.eq_ignore_ascii_case("content-type"))
    {
        out_headers.insert("content-type".to_string(), ctype);
    }

    tuple_to_axum_response(status, out_headers, out_body)
}

/// UYAP oturumu açık değilken UYAP giriş sayfası yerine gösterilen Çoklu UYAP sayfası.
fn _oturum_yok_sayfasi(neden: Option<&str>) -> Response {
    bilgi_sayfasi(
        "UYAP oturumu açık değil",
        "Ofis bilgisayarındaki Çoklu UYAP programında e-imza PIN'inizi girip <b>Bağlantıyı Paylaş</b>'a basın. Oturum açılınca bu sayfayı yenileyin.",
        neden,
    )
}

/// Çoklu UYAP markalı bilgi sayfası (UYAP'ın kendi sayfaları yerine; fikri mülkiyet, 2026-11-14).
/// `metin_html` güvenilir sabit HTML'dir; `neden` kaçışlanır.
pub(crate) fn bilgi_sayfasi(baslik: &str, metin_html: &str, neden: Option<&str>) -> Response {
    static LOGO: Lazy<String> = Lazy::new(|| {
        base64::engine::general_purpose::STANDARD
            .encode(include_bytes!("../assets/armonia-simge-64.png"))
    });
    let neden_html = match neden {
        Some(n) if !n.trim().is_empty() => format!(
            "<p class='neden'><b>Ayrıntı:</b> {}</p>",
            html_escape(n, false)
        ),
        _ => String::new(),
    };
    let html = format!(
        "<!doctype html><html lang='tr'><head><meta charset='utf-8'>\
<meta name='viewport' content='width=device-width,initial-scale=1'><meta name='referrer' content='no-referrer'>\
<title>Çoklu UYAP — {baslik}</title><link rel='icon' href='data:image/png;base64,{logo}'>\
<style>\
:root{{color-scheme:light dark;--zemin:#f4f1ea;--kart:#fffdf8;--yazi:#43423d;--soluk:#7a776e;--vurgu:#7d9a7d;--kenar:#e3ddd2}}\
@media (prefers-color-scheme:dark){{:root{{--zemin:#1c2420;--kart:#243029;--yazi:#ecebe4;--soluk:#a8b0a8;--kenar:#33413a}}}}\
body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:var(--zemin);\
color:var(--yazi);font:15px/1.5 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Arial,sans-serif}}\
main{{box-sizing:border-box;width:min(440px,calc(100vw - 32px));padding:28px;border-radius:16px;background:var(--kart);\
border:1px solid var(--kenar);box-shadow:0 10px 30px rgba(0,0,0,.08)}}\
.ust{{display:flex;align-items:center;gap:12px;margin-bottom:18px}}.ust img{{width:40px;height:40px;border-radius:10px}}\
.ust b{{font-size:18px}}h1{{font-size:17px;margin:0 0 8px}}p{{margin:0 0 10px;color:var(--soluk)}}\
.neden{{padding:10px 12px;border-radius:10px;background:rgba(125,154,125,.12);color:var(--yazi);font-size:13px;word-break:break-word}}\
button{{margin-top:8px;padding:9px 18px;border:0;border-radius:9px;background:var(--vurgu);color:#fff;font:inherit;font-weight:600;cursor:pointer}}\
</style></head><body><main>\
<div class='ust'><img src='data:image/png;base64,{logo}' alt=''><b>Çoklu UYAP</b></div>\
<h1>{baslik}</h1><p>{metin}</p>{neden}\
<button type='button' onclick='location.replace(\"/\")'>Yeniden dene</button>\
</main></body></html>",
        logo = &*LOGO,
        baslik = html_escape(baslik, false),
        metin = metin_html,
        neden = neden_html,
    );
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-store")
        .body(Body::from(html))
        .unwrap()
}

pub fn build_app(state: AppRuntimeState) -> Router {
    Router::new()
        .route("/__ping__", get(lan_ping))
        .route("/__agent__/ping", options(agent_ping_preflight).get(agent_ping))
        .route("/__uyap_yerel__/kod", post(yerel_giris_kodu))
        .route("/__uyap_yerel__/giris", get(yerel_giris))
        .route("/__uyap_yerel__/udf_pdf", post(udf_pdf_cevir))
        .route("/__uyap_yerel__/udf_doldur", post(udf_doldur_cevir))
        .route("/__uyap_yerel__/paylas", post(yerel_paylas))
        .route("/__uyap_lan__/giris", get(lan_giris_formu).post(lan_giris))
        .route("/", any(proxy_handler))
        .route("/*full_path", any(proxy_handler))
        .with_state(state)
}

// --------------------------------------------------------------------------------------
// Configuration, TLS & Server Lifecycle
// --------------------------------------------------------------------------------------
pub fn resolve_verify_ssl(no_verify: bool) -> bool {
    if !no_verify {
        return true;
    }
    let allowed = matches!(
        std::env::var("UYAP_ALLOW_INSECURE_TLS")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    );
    let bar = "!".repeat(72);
    if !allowed {
        println!("\n{}", bar);
        println!("[GUVENLIK] --no-verify YOK SAYILDI: UYAP TLS dogrulamasi ACIK tutuldu.");
        println!("  Yargi sistemi trafiginde sertifika dogrulamasini kapatmak MITM'e acik birakir.");
        println!("  Gercekten gerekiyorsa (yalnizca gelistirme) UYAP_ALLOW_INSECURE_TLS=1 ayarlayin.");
        println!("{}\n", bar);
        return true;
    }
    println!("\n{}", bar);
    println!("[GUVENLIK][UYARI] UYAP TLS sertifika dogrulamasi DEVRE DISI (--no-verify).");
    println!("  Baglanti MITM'e ACIK. Yalnizca guvenilir gelistirme aginda kullanin!");
    println!("{}\n", bar);
    false
}

pub fn configure(
    _host: &str,
    _port: u16,
    pin: Option<&str>,
    cert_id: Option<&str>,
    interval: u64,
    no_verify: bool,
    user: &str,
    password: Option<&str>,
) -> Arc<GatewaySession> {
    IDLE_INTERVAL.store(interval, Ordering::Relaxed);
    *GW_USER.lock() = user.to_string();
    *GW_PASS.lock() = password.map(|s| s.to_string());
    _init_local_token();
    let login_args = LoginArgs {
        pin: pin.map(|s| s.to_string()),
        cert_id: cert_id.map(|s| s.to_string()),
    };
    let session = Arc::new(GatewaySession::new(
        login_args,
        resolve_verify_ssl(no_verify),
    ));
    *GW.lock() = Some(session.clone());
    session
}

pub fn _lan_share_enabled() -> bool {
    matches!(
        std::env::var("UYAP_LAN_SHARE")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

pub fn _resolve_bind_host(requested: &str) -> String {
    let req = requested.trim();
    let wildcard = matches!(req, "" | "0.0.0.0" | "::" | "[::]" | "*");
    if wildcard && !_lan_share_enabled() {
        return "127.0.0.1".to_string();
    }
    if wildcard {
        return if req.is_empty() {
            "0.0.0.0".to_string()
        } else {
            req.to_string()
        };
    }
    req.to_string()
}

pub struct ServerHandle {
    pub host: String,
    pub port: u16,
    pub ssl_cert: Option<PathBuf>,
    pub ssl_key: Option<PathBuf>,
    pub lifespan: String,
    pub should_exit: Arc<AtomicBool>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
}

impl ServerHandle {
    pub fn set_should_exit(&self) {
        self.should_exit.store(true, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(true);
    }

    pub async fn run(&self) -> Result<(), String> {
        let mut keepalive_task = None;
        if self.lifespan != "off" {
            println!("[*] UYAP ağ geçidi başlatılıyor, ilk giriş deneniyor...");
            if let Some(session) = get_gw() {
                match session.ensure_alive().await {
                    Ok(_) => println!("[+] İlk giriş tamam, ağ geçidi hazır."),
                    Err(e) => println!(
                        "[!] İlk giriş başarısız ({}). Otomatik yeniden deneme yapılmayacak.",
                        e
                    ),
                }
                let interval = IDLE_INTERVAL.load(Ordering::Relaxed);
                keepalive_task = Some(tokio::spawn(_keepalive_loop(session, interval)));
            }
        }

        let bind_addr = format!("{}:{}", self.host, self.port);
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let result = if let (Some(ref cert_path), Some(ref key_path)) = (&self.ssl_cert, &self.ssl_key)
        {
            self.run_tls(&bind_addr, cert_path, key_path, &mut shutdown_rx)
                .await
        } else {
            let listener = tokio::net::TcpListener::bind(&bind_addr)
                .await
                .map_err(|e| format!("Dinleme hatası ({}): {}", bind_addr, e))?;
            let app = build_app(AppRuntimeState::default());
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                while !*shutdown_rx.borrow() {
                    if shutdown_rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
            .map_err(|e| format!("Sunucu hatası: {}", e))
        };

        if let Some(t) = keepalive_task {
            t.abort();
        }
        result
    }

    async fn run_tls(
        &self,
        bind_addr: &str,
        cert_path: &Path,
        key_path: &Path,
        shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), String> {
        let cert_file = std::fs::File::open(cert_path)
            .map_err(|e| format!("TLS sertifikası açılamadı ({}): {}", cert_path.display(), e))?;
        let mut cert_reader = std::io::BufReader::new(cert_file);
        let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
            rustls_pemfile::certs(&mut cert_reader)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("TLS sertifikası okunamadı: {}", e))?;

        let key_file = std::fs::File::open(key_path)
            .map_err(|e| format!("TLS anahtarı açılamadı ({}): {}", key_path.display(), e))?;
        let mut key_reader = std::io::BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| format!("TLS anahtarı okunamadı: {}", e))?
            .ok_or_else(|| "TLS özel anahtarı bulunamadı".to_string())?;

        let tls_cfg = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| format!("TLS yapılandırma hatası: {}", e))?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_cfg));

        let tls_peer_map = Arc::new(Mutex::new(HashMap::new()));
        let internal_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("İç TLS köprü soketi açılamadı: {}", e))?;
        let internal_addr = internal_listener
            .local_addr()
            .map_err(|e| format!("İç TLS köprü adresi alınamadı: {}", e))?;

        let state = AppRuntimeState {
            is_lan_tls: true,
            tls_peer_map: tls_peer_map.clone(),
        };
        let app = build_app(state);
        let mut internal_shutdown = shutdown_rx.clone();
        let internal_srv = tokio::spawn(async move {
            let _ = axum::serve(
                internal_listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                while !*internal_shutdown.borrow() {
                    if internal_shutdown.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;
        });

        let public_listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|e| format!("HTTPS dinleme hatası ({}): {}", bind_addr, e))?;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                accept_res = public_listener.accept() => {
                    let (stream, peer_addr) = match accept_res {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let _ = stream.set_nodelay(true);
                    let acceptor = acceptor.clone();
                    let peer_map = tls_peer_map.clone();
                    tokio::spawn(async move {
                        if let Ok(mut tls_stream) = acceptor.accept(stream).await {
                            if let Ok(mut backend) = tokio::net::TcpStream::connect(internal_addr).await {
                                let _ = backend.set_nodelay(true);
                                let local_port = backend.local_addr().map(|a| a.port()).unwrap_or(0);
                                if local_port != 0 {
                                    peer_map.lock().insert(local_port, peer_addr.ip());
                                }
                                let _ = tokio::io::copy_bidirectional(&mut tls_stream, &mut backend).await;
                                if local_port != 0 {
                                    peer_map.lock().remove(&local_port);
                                }
                            }
                        }
                    });
                }
            }
        }

        let _ = internal_srv.await;
        Ok(())
    }
}

pub fn build_server(
    host: &str,
    port: u16,
    _in_thread: bool,
    ssl_cert: Option<PathBuf>,
    ssl_key: Option<PathBuf>,
    lifespan: &str,
) -> ServerHandle {
    let resolved_host = _resolve_bind_host(host);
    if ssl_cert.is_some() {
        println!(
            "[i] Yerel ağ kapısı HTTPS olarak açık ({}:{}).",
            resolved_host, port
        );
    } else if matches!(resolved_host.as_str(), "127.0.0.1" | "::1" | "localhost")
        && _lan_share_enabled()
    {
        println!(
            "[i] Düz HTTP yalnızca yerel arayüzde ({}:{}); yerel ağ HTTPS kapısından girer.",
            resolved_host, port
        );
    } else if matches!(resolved_host.as_str(), "127.0.0.1" | "::1" | "localhost") {
        println!(
            "[i] Proxy yalnızca yerel arayüzde ({}:{}) dinliyor; LAN-direct kapalı \
(açmak için UYAP_LAN_SHARE=1). Dış-ağ paylaşımı bundan etkilenmez.",
            resolved_host, port
        );
    } else {
        println!(
            "[!] Proxy TÜM AĞA açık dinliyor ({}:{}); aynı LAN'daki cihazlar \
Basic-Auth (GW_PASS) ile erişebilir. İnternete port-forward ETMEYİN.",
            resolved_host, port
        );
    }
    let (shutdown_tx, _) = tokio::sync::watch::channel(false);
    ServerHandle {
        host: resolved_host,
        port,
        ssl_cert,
        ssl_key,
        lifespan: lifespan.to_string(),
        should_exit: Arc::new(AtomicBool::new(false)),
        shutdown_tx,
    }
}

#[derive(Parser, Debug)]
#[command(about = "UYAP office-side gateway (reverse proxy).")]
struct CliArgs {
    #[arg(
        long,
        default_value = "0.0.0.0",
        help = "Bind address. Reachable only over your private VPN/LAN; never port-forward to the internet."
    )]
    host: String,
    #[arg(long, default_value_t = 8800, help = "Listen port (default: 8800)")]
    port: u16,
    #[arg(
        long,
        env = "UYAP_PIN",
        help = "E-imza PIN (ya da UYAP_PIN ortam değişkeni). Kaynağa YAZILMAZ."
    )]
    pin: Option<String>,
    #[arg(long, help = "Certificate ID for fully unattended (re)login")]
    cert_id: Option<String>,
    #[arg(long, help = "Bypass SSL verification toward UYAP")]
    no_verify: bool,
    #[arg(long, default_value_t = 5, help = "Idle session probe interval in seconds")]
    interval: u64,
    #[arg(
        long,
        env = "GW_USER",
        default_value = "uyap",
        help = "Basic-Auth username for home clients"
    )]
    user: String,
    #[arg(
        long,
        env = "GW_PASS",
        help = "Basic-Auth password for home clients"
    )]
    password: Option<String>,
}

pub fn main() {
    let args = CliArgs::parse();
    if args.pin.as_deref().unwrap_or("").is_empty() {
        println!(
            "[!] PIN verilmedi. --pin ile ya da UYAP_PIN ortam değişkeniyle sağlayın \
(e-imza/giriş için gerekli). Güvenlik: PIN kaynağa GÖMÜLMEZ."
        );
        std::process::exit(2);
    }
    configure(
        &args.host,
        args.port,
        args.pin.as_deref(),
        args.cert_id.as_deref(),
        args.interval,
        args.no_verify,
        &args.user,
        args.password.as_deref(),
    );
    if GW_PASS.lock().as_deref().unwrap_or("").is_empty() {
        println!("[!] UYARI: parola ayarlı değil (--password ya da GW_PASS) — sunucu istekleri reddedecek.");
    }
    println!(
        "[*] Ağ geçidi http://{}:{} adresinde dinleyecek (sadece VPN/LAN).",
        args.host, args.port
    );
    println!("[*] Evden bağlanmak için tarayıcıyı bu adrese yönlendir.");
    let rt = crate::build_tokio_runtime().expect("tokio runtime");
    rt.block_on(async {
        let server = build_server(&args.host, args.port, false, None, None, "auto");
        if let Err(e) = server.run().await {
            eprintln!("[!] {}", e);
        }
    });
}
