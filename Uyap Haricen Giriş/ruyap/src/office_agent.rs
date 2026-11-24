//! UYAP Ofis Ajanı (paylaşan taraf) — `uyap_core/office_agent.py` birebir Rust portu.
//! ----------------------------------------------------------------------------------
//! Ofis bilgisayarında, e-imza kartının takılı olduğu makinede çalışır. TEK UYAP oturumunu
//! kurar/canlı tutar ve onu İKİ yoldan paylaşır:
//!   1. YEREL AĞ (LAN): uyap_proxy HTTPS/HTTP sunucusu
//!   2. DIŞ AĞ (WebRTC + WS-relay): signaling üzerinden teklif yanıtlar.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD as B64_STD, URL_SAFE_NO_PAD as B64_URL};
use base64::Engine as _;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use md5::{Digest as Md5Digest, Md5};
use once_cell::sync::Lazy;
use parking_lot::Mutex as SyncMutex;
use rand::RngCore;
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::{mpsc, Mutex as AsyncMutex, Notify};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use webrtc::api::APIBuilder;
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::is_listesi;
use crate::jobs;
use crate::lan_tls;
use crate::p2p_wire::{
    fetch_ice_servers, send_message, FrameChannel, RTCIceServerConfig, Reassembler, AGENT_PREFIX,
};
use crate::runtime::{self, interactive_write, is_write_method, uyap_istek_izi};
use crate::sira_kapisi;
use crate::uyap_proxy;

type HmacSha256 = Hmac<Sha256>;

pub const SHELL_TTL: f64 = 300.0;

pub static CACHE_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let p = Path::new("uyap_core").join("static_cache");
    if p.exists() {
        p
    } else {
        PathBuf::from("static_cache")
    }
});

static REFRESHING: Lazy<SyncMutex<HashSet<String>>> = Lazy::new(|| SyncMutex::new(HashSet::new()));
// İçeriksiz bildirim sinyali (is_isaret.rs): yalnız sunucu gerçek kimlik doğrulaması bildirdiğinde (joined +
// ticket_key) dolu; bağlantı kopunca boşalır.
static ISARET_KANALI: Lazy<SyncMutex<Option<mpsc::UnboundedSender<WsMessage>>>> = Lazy::new(|| SyncMutex::new(None));

pub fn isaret_bagli() -> bool {
    ISARET_KANALI.lock().is_some()
}

/// `{"type":"isaret","hesap":…,"tur":…}` — içerik yok; bulut hesabın bu ofise ait olduğunu denetler.
pub fn isaret_gonder(hesap: &str, tur: &str) -> bool {
    match ISARET_KANALI.lock().as_ref() {
        Some(tx) => tx.send(WsMessage::Text(json!({"type": "isaret", "hesap": hesap, "tur": tur}).to_string())).is_ok(),
        None => false,
    }
}
static ACTIVE_DOWNLOADS: Lazy<AsyncMutex<HashMap<String, Arc<Notify>>>> =
    Lazy::new(|| AsyncMutex::new(HashMap::new()));

#[derive(Parser, Debug, Clone)]
#[command(about = "UYAP ofis ajanı (LAN + WebRTC paylaşım).")]
pub struct OfficeArgs {
    /// Signaling sunucusu, ör. wss://senin-sunucun:9000/ws
    #[arg(long, env = "UYAP_SIGNALING")]
    pub signaling: Option<String>,
    /// Oda/lisans anahtarı (ofis ve istemci aynı olmalı)
    #[arg(long, env = "UYAP_ROOM")]
    pub room: Option<String>,
    /// Ofis kodu
    #[arg(long, default_value = "")]
    pub office_code: String,
    /// Paylaşım parolası (LAN Basic-Auth + signaling için)
    #[arg(long, env = "UYAP_PASSWORD")]
    pub password: Option<String>,
    /// E-imza PIN (ya da UYAP_PIN ortam değişkeni)
    #[arg(long, env = "UYAP_PIN")]
    pub pin: Option<String>,
    /// Sertifika ID (gözetimsiz yeniden giriş için)
    #[arg(long, env = "UYAP_CERT_ID")]
    pub cert_id: Option<String>,
    /// UYAP'a karşı SSL doğrulamasını atla
    #[arg(long)]
    pub no_verify: bool,
    /// Boşta oturum probe aralığı (sn)
    #[arg(long, default_value_t = 5)]
    pub interval: u64,
    /// Yerel LAN portu (varsayılan 8800)
    #[arg(long, default_value_t = 8800)]
    pub proxy_port: u16,
    /// LAN dinleme adresi (varsayılan 0.0.0.0)
    #[arg(long, default_value = "0.0.0.0")]
    pub host: String,
    /// LAN Basic-Auth kullanıcı adı (varsayılan: oda anahtarı)
    #[arg(long, env = "GW_USER")]
    pub user: Option<String>,
    #[arg(skip)]
    pub lan_token: Option<String>,
}

pub fn _build_forward_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, v) in headers {
        let lk = k.to_lowercase();
        if !uyap_proxy::DROP_REQUEST.contains(&lk.as_str()) {
            out.insert(k.clone(), v.clone());
        }
    }
    out.insert("Host".to_string(), uyap_proxy::UYAP_HOST.to_string());
    out
}

pub fn should_cache(method: &str, path: &str) -> bool {
    if !method.eq_ignore_ascii_case("GET") {
        return false;
    }
    if path.trim_start_matches('/').starts_with(AGENT_PREFIX) {
        return false;
    }
    let path_lower = path.to_lowercase();
    if path_lower.contains("/static/") || path_lower.contains("static/") {
        return true;
    }
    let exts = [
        ".js", ".css", ".woff2", ".woff", ".png", ".jpg", ".jpeg", ".svg", ".ico", ".gif",
    ];
    if exts.iter().any(|ext| path_lower.ends_with(ext)) {
        return true;
    }
    if path.is_empty() || matches!(path_lower.as_str(), "index.html" | "index.jsp" | "giris") {
        return true;
    }
    let last_segment = path_lower.split('/').last().unwrap_or("");
    !last_segment.is_empty() && !last_segment.contains('.')
}

pub fn _is_shell(path: &str) -> bool {
    let p = path.to_lowercase();
    let last = p.split('/').last().unwrap_or("");
    path.is_empty()
        || matches!(p.as_str(), "index.html" | "index.jsp" | "giris")
        || (!last.is_empty() && !last.contains('.'))
}

pub fn _cache_key(path: &str, proxy_base: &str, query: &str) -> String {
    let mut key = format!("{path}?{query}::{proxy_base}");
    if _is_shell(path) {
        key.push_str("::iv=");
        key.push_str(&uyap_proxy::INJECT_VERSION);
    }
    let digest = Md5::digest(key.as_bytes());
    hex::encode(digest)
}

pub fn _cache_age(path: &str, proxy_base: &str, query: &str) -> Option<f64> {
    let h = _cache_key(path, proxy_base, query);
    let p = CACHE_DIR.join(h);
    let meta = std::fs::metadata(p).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    Some(age.as_secs_f64())
}

pub fn get_cached_response(
    path: &str,
    proxy_base: &str,
    query: &str,
) -> Option<(u16, HashMap<String, String>, Vec<u8>)> {
    if !CACHE_DIR.exists() {
        return None;
    }
    let h = _cache_key(path, proxy_base, query);
    let cache_path = CACHE_DIR.join(&h);
    let meta_path = CACHE_DIR.join(format!("{h}.meta"));
    if cache_path.exists() && meta_path.exists() {
        if let (Ok(meta_str), Ok(body)) = (
            std::fs::read_to_string(&meta_path),
            std::fs::read(&cache_path),
        ) {
            if let Ok(meta) = serde_json::from_str::<Value>(&meta_str) {
                let status = meta.get("status").and_then(|v| v.as_u64()).unwrap_or(200) as u16;
                let mut headers = HashMap::new();
                if let Some(obj) = meta.get("headers").and_then(|v| v.as_object()) {
                    for (k, v) in obj {
                        if let Some(s) = v.as_str() {
                            headers.insert(k.clone(), s.to_string());
                        }
                    }
                }
                return Some((status, headers, body));
            }
        }
    }
    None
}

pub fn save_response_to_cache(
    path: &str,
    proxy_base: &str,
    status: u16,
    headers: &HashMap<String, String>,
    body: &[u8],
    query: &str,
) {
    if status != 200 {
        return;
    }
    let _ = std::fs::create_dir_all(&*CACHE_DIR);
    let h = _cache_key(path, proxy_base, query);
    let cache_path = CACHE_DIR.join(&h);
    let meta_path = CACHE_DIR.join(format!("{h}.meta"));
    let meta_json = json!({"status": status, "headers": headers});
    if let Ok(s) = serde_json::to_string(&meta_json) {
        let _ = std::fs::write(&meta_path, s);
        let _ = std::fs::write(&cache_path, body);
    }
}

async fn _gw_forward(
    gw: &uyap_proxy::GatewaySession,
    method: &str,
    target: &str,
    body: &[u8],
    fwd_headers: &HashMap<String, String>,
) -> Result<(u16, reqwest::header::HeaderMap, Vec<u8>), String> {
    let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let mut req = gw.client.request(m, target);
    for (k, v) in fwd_headers {
        if let (Ok(hn), Ok(hv)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(v),
        ) {
            req = req.header(hn, hv);
        }
    }
    if !body.is_empty() {
        req = req.body(body.to_vec());
    }
    let r = req.send().await.map_err(|e| e.to_string())?;
    let status = r.status().as_u16();
    let headers = r.headers().clone();
    let bytes = r.bytes().await.map_err(|e| e.to_string())?.to_vec();
    Ok((status, headers, bytes))
}

async fn _fetch_uyap(
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    fwd_headers: &HashMap<String, String>,
    proxy_base: &str,
    store: bool,
) -> Result<(u16, HashMap<String, String>, Vec<u8>), String> {
    let gw = uyap_proxy::get_gw().ok_or_else(|| "UYAP oturumu hazır değil (gw=None)".to_string())?;
    let mut target = format!("{}/{}", uyap_proxy::UYAP_BASE, path);
    if !query.is_empty() {
        target.push('?');
        target.push_str(query);
    }

    let (status, resp_headers, resp_body) = {
        let _izi = uyap_istek_izi().await;
        let (st, hdrs, bdy) = _gw_forward(&gw, method, &target, body, fwd_headers).await?;
        if uyap_proxy::_looks_like_login_wall(st, &hdrs) {
            gw.relogin().await?;
            _gw_forward(&gw, method, &target, body, fwd_headers).await?
        } else {
            (st, hdrs, bdy)
        }
    };

    let orig_ctype = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let (ctype, govde) = uyap_proxy::_ek_zip_pdf_ac(path, &orig_ctype, &resp_body);
    let out_body = uyap_proxy::_rewrite_body(&ctype, &govde, proxy_base, true);
    let mut out_headers = uyap_proxy::_build_response_headers_from_map(&resp_headers, proxy_base);
    if ctype != orig_ctype {
        out_headers = uyap_proxy::_ek_pdf_basliklari(out_headers, &ctype);
    }

    if store && status == 200 {
        save_response_to_cache(path, proxy_base, status, &out_headers, &out_body, query);
    }
    Ok((status, out_headers, out_body))
}

async fn _background_refresh(
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
    fwd_headers: HashMap<String, String>,
    proxy_base: String,
) {
    {
        let mut set = REFRESHING.lock();
        if set.contains(&path) {
            return;
        }
        set.insert(path.clone());
    }
    let _ = _fetch_uyap(&method, &path, &query, &body, &fwd_headers, &proxy_base, true).await;
    REFRESHING.lock().remove(&path);
}

fn header_ci<'a>(headers: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Tünelden gelen "UDF İndir" isteği: alan değerleri yerleştirilmiş kopya (uyap_proxy `udf_doldur_cevir` ile aynı).
async fn udf_doldur_tunel(govde: Vec<u8>) -> (u16, HashMap<String, String>, Vec<u8>) {
    let metin = |kod: u16, m: &str| {
        let h = HashMap::from([("content-type".to_string(), "text/plain; charset=utf-8".to_string())]);
        (kod, h, m.as_bytes().to_vec())
    };
    if govde.len() > 40 * 1024 * 1024 {
        return metin(413, "Belge çok büyük.");
    }
    if !crate::udf_pdf::udf_zip_mi(&govde) {
        return metin(415, "UDF değil.");
    }
    match tokio::task::spawn_blocking(move || crate::udf_alan::doldur(&govde)).await {
        Ok(Ok(Some(udf))) => (
            200,
            HashMap::from([
                ("content-type".to_string(), "application/octet-stream".to_string()),
                ("cache-control".to_string(), "no-store".to_string()),
            ]),
            udf,
        ),
        Ok(Ok(None)) => (204, HashMap::new(), Vec::new()),
        Ok(Err(e)) => metin(422, &e),
        Err(_) => metin(500, "İşlem yarıda kaldı."),
    }
}

/// Tünelden gelen UDF → PDF isteği (ofis vekilindeki `/__uyap_yerel__/udf_pdf` ile aynı iş).
async fn udf_pdf_tunel(govde: Vec<u8>) -> (u16, HashMap<String, String>, Vec<u8>) {
    let metin = |kod: u16, m: &str| {
        let h = HashMap::from([("content-type".to_string(), "text/plain; charset=utf-8".to_string())]);
        (kod, h, m.as_bytes().to_vec())
    };
    if govde.len() > 40 * 1024 * 1024 {
        return metin(413, "Belge çok büyük.");
    }
    if !crate::udf_pdf::udf_zip_mi(&govde) {
        return metin(415, "UDF değil.");
    }
    match tokio::task::spawn_blocking(move || crate::udf_pdf::udf_pdf_uret(&govde)).await {
        Ok(Ok(pdf)) => (
            200,
            HashMap::from([
                ("content-type".to_string(), "application/pdf".to_string()),
                ("cache-control".to_string(), "no-store".to_string()),
            ]),
            pdf,
        ),
        Ok(Err(e)) => metin(422, &e),
        Err(_) => metin(500, "Çevirme yarıda kaldı."),
    }
}

pub async fn handle_uyap_request(
    meta: &Value,
    body: &[u8],
    kaynak: &str,
    hesap: Option<String>,
) -> (u16, HashMap<String, String>, Vec<u8>) {
    let path = meta
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_start_matches('/')
        .to_string();
    let method = meta
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .to_string();
    let query = meta
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let proxy_base = meta
        .get("proxy_base")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut req_headers = HashMap::new();
    if let Some(obj) = meta.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                req_headers.insert(k.clone(), s.to_string());
            }
        }
    }

    // Büro veritabanı uçları (İcra Takip Süreçleri sayfası): ofis bilgisayarındaki veritabanından cevaplanır,
    // UYAP'a iletilmez. Bütün büro kullanıcılarına açık; başka siteden gelen yazma isteği reddedilir.
    // İş Listesi (büro veritabanı): isteğin sahibi tünel biletinden (kimlik.rs). icra_api'den ÖNCE: aynı ön ek.
    // Armonia Yazı editörünün dosyaları (src/yazi_sunucu.rs): uzaktaki kullanıcı da aynı editörü açar.
    if crate::yazi_sunucu::is_path(&path) {
        return crate::yazi_sunucu::yanit(&method, &path);
    }
    if crate::is_api::is_path(&path) {
        let capraz = header_ci(&req_headers, "sec-fetch-site").is_some_and(|v| v.eq_ignore_ascii_case("cross-site"));
        return crate::is_api::handle(&method, &path, &query, body, capraz, hesap.as_deref()).await;
    }
    if crate::icra_api::is_path(&path) {
        let capraz = header_ci(&req_headers, "sec-fetch-site").is_some_and(|v| v.eq_ignore_ascii_case("cross-site"));
        return crate::icra_api::handle(&method, &path, &query, body, capraz).await;
    }

    if uyap_proxy::is_gecit_path(&path) {
        return uyap_proxy::handle_gecit_proxy(&method, &path, &query, &body, &req_headers).await;
    }

    if jobs::is_control_path(&path) {
        return jobs::handle_control(&method, &path, &query, &body, Some(kaynak)).await;
    }

    // DİKKAT: `path` baştaki "/" atılmış hâlde (yukarıda trim_start_matches).
    // Uzak cihazdaki "Paylaş": UDF'yi PDF'e çevir (telefonun paylaş ekranı UDF almıyor). Yalnız çeviri; UYAP'a gitmez.
    if path == "__uyap_yerel__/udf_pdf" && method.eq_ignore_ascii_case("POST") {
        return udf_pdf_tunel(body.to_vec()).await;
    }
    if path == "__uyap_yerel__/udf_doldur" && method.eq_ignore_ascii_case("POST") {
        return udf_doldur_tunel(body.to_vec()).await;
    }

    if let Some(kesik) = uyap_proxy::kesilen_uc_yaniti(&path) {
        return kesik;
    }

    if let Some(gw) = uyap_proxy::get_gw() {
        gw.touch();
    }

    let fwd_headers = _build_forward_headers(&req_headers);
    let cacheable = should_cache(&method, &path);
    let mut notify_guard: Option<Arc<Notify>> = None;

    if cacheable {
        loop {
            let existing = {
                let map = ACTIVE_DOWNLOADS.lock().await;
                map.get(&path).cloned()
            };
            match existing {
                Some(n) => n.notified().await,
                None => break,
            }
        }

        if let Some((status, headers, cached_body)) = get_cached_response(&path, &proxy_base, &query)
        {
            let if_none_match = header_ci(&req_headers, "if-none-match");
            let if_modified_since = header_ci(&req_headers, "if-modified-since");
            let cached_etag = header_ci(&headers, "etag");
            let cached_last_modified = header_ci(&headers, "last-modified");

            let mut use_304 = false;
            if let (Some(inm), Some(ce)) = (if_none_match, cached_etag) {
                if inm.trim() == ce.trim() {
                    use_304 = true;
                }
            } else if let (Some(ims), Some(clm)) = (if_modified_since, cached_last_modified) {
                if ims.trim() == clm.trim() {
                    use_304 = true;
                }
            }

            if use_304 {
                return (304, headers, Vec::new());
            }

            if _is_shell(&path) {
                if let Some(age) = _cache_age(&path, &proxy_base, &query) {
                    if age > SHELL_TTL {
                        let m = method.clone();
                        let p = path.clone();
                        let q = query.clone();
                        let b = body.to_vec();
                        let fh = fwd_headers.clone();
                        let pb = proxy_base.clone();
                        tokio::spawn(async move {
                            _background_refresh(m, p, q, b, fh, pb).await;
                        });
                    }
                }
            }
            return (status, headers, cached_body);
        }

        let n = Arc::new(Notify::new());
        ACTIVE_DOWNLOADS.lock().await.insert(path.clone(), n.clone());
        notify_guard = Some(n);
    }

    let result = if is_write_method(&method) {
        let _w = interactive_write().await;
        Box::pin(_fetch_uyap(&method, &path, &query, body, &fwd_headers, &proxy_base, cacheable)).await
    } else {
        Box::pin(_fetch_uyap(&method, &path, &query, body, &fwd_headers, &proxy_base, cacheable)).await
    };

    if let Some(n) = notify_guard {
        ACTIVE_DOWNLOADS.lock().await.remove(&path);
        n.notify_waiters();
    }

    match result {
        Ok(res) => res,
        Err(e) => {
            let mut h = HashMap::new();
            h.insert(
                "Content-Type".to_string(),
                "text/plain; charset=utf-8".to_string(),
            );
            (502, h, format!("UYAP'a ulaşılamadı: {e}").into_bytes())
        }
    }
}

pub fn _wire_datachannel(channel: Arc<RTCDataChannel>, cid: String) {
    let reasm = Arc::new(SyncMutex::new(Reassembler::new()));
    let ch_for_msg = channel.clone();
    let cid_for_msg = cid.clone();

    channel.on_message(Box::new(move |msg| {
        let reasm = reasm.clone();
        let ch = ch_for_msg.clone();
        let cid = cid_for_msg.clone();
        Box::pin(async move {
            let decoded = match reasm.lock().feed(&msg.data) {
                Ok(Some(m)) => m,
                Ok(None) => return,
                Err(e) => {
                    println!("[!] Çerçeve çözme hatası: {e}");
                    return;
                }
            };

            if decoded.kind == "ping" {
                let _ = send_message(&ch, &decoded.id, "pong", Some(json!({})), b"").await;
            } else if decoded.kind == "req" {
                let ch_reply = ch.clone();
                let cid_short: String = cid.chars().take(8).collect();
                let hesap = crate::kimlik::tunel_hesabi(&cid);
                tokio::spawn(async move {
                    let meta = decoded.meta.unwrap_or_else(|| json!({}));
                    let (status, headers, body) =
                        Box::pin(handle_uyap_request(&meta, &decoded.body, &format!("tünel:{cid_short}"), hesap))
                            .await;
                    let res_meta = json!({"status": status, "headers": headers});
                    if let Err(e) =
                        Box::pin(send_message(&ch_reply, &decoded.id, "res", Some(res_meta), &body)).await
                    {
                        println!("[!] Istek cevabi tünelden gonderilemedi: {e}");
                    }
                });
            }
        })
    }));

    println!("[+] DataChannel açıldı; istekler UYAP'a iletilecek.");
}

/// Geçiş dönemi (20.11.2026): cokluuyap.com'daki tarayıcı kodu güncellenene kadar ŞİFRESİZ aktarma (eski istemcinin
/// `pub`suz relay_hello'su) da kabul edilir, günlüğe uyarı yazılır. Site güncellenince `false` yapılır: o zaman yalnız
/// uçtan uca şifreli aktarma (`relay_sifre`) çalışır.
pub const SIFRESIZ_AKTARMA_KABUL: bool = true;

pub struct WSRelaySender {
    pub cid: Option<String>,
    buffered_amount: Arc<AtomicUsize>,
    tx: mpsc::UnboundedSender<Option<Vec<u8>>>,
    closed: AtomicBool,
}

impl WSRelaySender {
    /// `sifre`: uçtan uca şifreli aktarmada ofis→istemci yönü (yoksa eski, şifresiz base64 aktarma).
    pub fn new(ws_tx: mpsc::UnboundedSender<WsMessage>, cid: Option<String>, sifre: Option<crate::relay_sifre::Yon>) -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Option<Vec<u8>>>();
        let mut sifre = sifre;
        let buffered_amount = Arc::new(AtomicUsize::new(0));
        let ba_clone = buffered_amount.clone();
        let cid_clone = cid.clone();

        tokio::spawn(async move {
            while let Some(maybe_frame) = rx.recv().await {
                let Some(frame) = maybe_frame else {
                    break;
                };
                let len = frame.len();
                // Kuyruk sırası = sayaç sırası: çerçeveler tek görevde, geldikleri sırayla şifrelenip gönderilir.
                let mut payload = match sifre.as_mut() {
                    Some(y) => json!({"type": "relay", "e": B64_STD.encode(y.sifrele(&frame))}),
                    None => json!({"type": "relay", "b64": B64_STD.encode(&frame)}),
                };
                if let Some(ref c) = cid_clone {
                    payload["cid"] = Value::String(c.clone());
                }
                if let Ok(txt) = serde_json::to_string(&payload) {
                    if let Err(e) = ws_tx.send(WsMessage::Text(txt)) {
                        println!("[!] relay gönderim hatası: {e}");
                    }
                }
                ba_clone.fetch_sub(len, Ordering::SeqCst);
            }
        });

        Arc::new(Self {
            cid,
            buffered_amount,
            tx,
            closed: AtomicBool::new(false),
        })
    }

    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            let _ = self.tx.send(None);
        }
    }
}

impl FrameChannel for Arc<WSRelaySender> {
    fn buffered_amount(&self) -> Pin<Box<dyn Future<Output = usize> + Send + '_>> {
        let val = self.buffered_amount.load(Ordering::SeqCst);
        Box::pin(async move { val })
    }

    fn send_frame(&self, frame: Vec<u8>) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        self.buffered_amount.fetch_add(frame.len(), Ordering::SeqCst);
        let res = self
            .tx
            .send(Some(frame))
            .map_err(|e| format!("WSRelaySender kuyruk hatası: {e}"));
        Box::pin(async move { res })
    }
}

pub struct MultiAnswerer {
    ice_servers: Vec<RTCIceServerConfig>,
    pcs: Arc<AsyncMutex<HashMap<String, Arc<RTCPeerConnection>>>>,
}

impl MultiAnswerer {
    pub fn new(ice_servers: Vec<RTCIceServerConfig>) -> Self {
        Self {
            ice_servers,
            pcs: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    pub async fn close_one(&self, cid: &str) {
        let pc = self.pcs.lock().await.remove(cid);
        if let Some(p) = pc {
            let _ = p.close().await;
        }
    }

    pub async fn close_all(&self) {
        let keys: Vec<String> = self.pcs.lock().await.keys().cloned().collect();
        for k in keys {
            self.close_one(&k).await;
        }
    }

    pub async fn on_offer(
        &self,
        ws_tx: &mpsc::UnboundedSender<WsMessage>,
        cid: &str,
        sdp: &str,
        _sdptype: &str,
    ) -> Result<(), String> {
        self.close_one(cid).await;

        let api = APIBuilder::new().build();
        let cfg = RTCConfiguration {
            ice_servers: self.ice_servers.iter().map(|s| s.to_webrtc()).collect(),
            ..Default::default()
        };
        let pc = Arc::new(
            api.new_peer_connection(cfg)
                .await
                .map_err(|e| format!("RTCPeerConnection kurulamadı: {e}"))?,
        );
        self.pcs.lock().await.insert(cid.to_string(), pc.clone());

        let cid_dc = cid.to_string();
        pc.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
            let c = cid_dc.clone();
            Box::pin(async move {
                _wire_datachannel(channel, c);
            })
        }));

        let cid_state = cid.to_string();
        let pcs_weak = Arc::downgrade(&self.pcs);
        let pc_weak = Arc::downgrade(&pc);
        pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let c = cid_state.clone();
            let pcs_ref = pcs_weak.clone();
            let pc_ref = pc_weak.clone();
            Box::pin(async move {
                let short: String = c.chars().take(6).collect();
                println!("[*] P2P[{short}] durumu: {state}");
                if matches!(
                    state,
                    RTCPeerConnectionState::Failed
                        | RTCPeerConnectionState::Closed
                        | RTCPeerConnectionState::Disconnected
                ) {
                    if let (Some(pcs), Some(cur_pc)) = (pcs_ref.upgrade(), pc_ref.upgrade()) {
                        let mut guard = pcs.lock().await;
                        if let Some(existing) = guard.get(&c) {
                            if Arc::ptr_eq(existing, &cur_pc) {
                                guard.remove(&c);
                            }
                        }
                    }
                }
            })
        }));

        let offer = RTCSessionDescription::offer(sdp.to_string())
            .map_err(|e| format!("Offer SDP hatası: {e}"))?;
        pc.set_remote_description(offer)
            .await
            .map_err(|e| format!("setRemoteDescription hatası: {e}"))?;

        let answer = pc
            .create_answer(None)
            .await
            .map_err(|e| format!("createAnswer hatası: {e}"))?;
        let mut gather_complete = pc.gathering_complete_promise().await;
        pc.set_local_description(answer)
            .await
            .map_err(|e| format!("setLocalDescription hatası: {e}"))?;
        let _ = tokio::time::timeout(Duration::from_secs(15), gather_complete.recv()).await;

        let local_desc = pc
            .local_description()
            .await
            .ok_or_else(|| "localDescription yok".to_string())?;

        let msg = json!({
            "type": "answer",
            "cid": cid,
            "sdp": local_desc.sdp,
            "sdptype": "answer",
        });
        ws_tx
            .send(WsMessage::Text(msg.to_string()))
            .map_err(|e| format!("Answer gönderilemedi: {e}"))?;

        let short: String = cid.chars().take(6).collect();
        println!("[*] [{short}] yanıt (answer) gönderildi; P2P bağlantı kuruluyor…");
        Ok(())
    }
}

pub fn detect_local_ips() -> Vec<String> {
    let mut ips = Vec::new();
    if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
        if s.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = s.local_addr() {
                if let IpAddr::V4(v4) = addr.ip() {
                    ips.push(v4.to_string());
                }
            }
        }
    }
    ips
}

fn _ticket_ok(room_ticket_key: Option<&[u8]>, msg: &Value) -> bool {
    let Some(key) = room_ticket_key else {
        return true;
    };
    let Some(cid) = msg.get("cid").and_then(|v| v.as_str()) else {
        return false;
    };
    let Some(tkt) = msg.get("ticket").and_then(|v| v.as_str()) else {
        return false;
    };
    let exp = match msg.get("ticket_exp") {
        Some(v) => match v.as_i64() {
            Some(n) => n,
            None => v.as_str().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
        },
        None => return false,
    };
    if cid.is_empty() || tkt.is_empty() || exp == 0 {
        return false;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if exp < now {
        return false;
    }
    let Ok(mut mac) = HmacSha256::new_from_slice(key) else {
        return false;
    };
    mac.update(format!("{cid}:{exp}").as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    // Sabit zamanlı karşılaştırma
    if tkt.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in tkt.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub async fn run_signaling(args: OfficeArgs, ice_servers: Vec<RTCIceServerConfig>) {
    let session = Arc::new(MultiAnswerer::new(ice_servers));
    let mut backoff = 1u64;
    let sig_url = args.signaling.clone().unwrap_or_default();

    loop {
        let ws_cfg = WebSocketConfig {
            max_message_size: Some(4 * 1024 * 1024),
            max_frame_size: Some(4 * 1024 * 1024),
            ..Default::default()
        };

        let conn_res = tokio_tungstenite::connect_async_with_config(&sig_url, Some(ws_cfg), false).await;
        match conn_res {
            Ok((ws_stream, _)) => {
                let (mut ws_sink, mut ws_source) = ws_stream.split();
                let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsMessage>();

                let writer_task = tokio::spawn(async move {
                    while let Some(m) = ws_rx.recv().await {
                        if ws_sink.send(m).await.is_err() {
                            break;
                        }
                    }
                });

                let lan_token = args.lan_token.clone().unwrap_or_default();
                let port = if !lan_token.is_empty() {
                    *lan_tls::LAN_TLS_PORT
                } else {
                    args.proxy_port
                };
                let join_msg = json!({
                    "role": "office",
                    "room": args.room.clone().unwrap_or_default(),
                    "office_code": args.office_code,
                    "password": args.password.clone().unwrap_or_default(),
                    "local_ips": if !lan_token.is_empty() { detect_local_ips() } else { vec![] },
                    "port": port,
                    "lan_token": lan_token,
                });

                if let Err(e) = ws_tx.send(WsMessage::Text(join_msg.to_string())) {
                    println!("[!] Signaling join gönderilemedi: {e}");
                    writer_task.abort();
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = std::cmp::min(backoff * 2, 30);
                    continue;
                }

                println!("[+] Signaling'e bağlandı: {sig_url} (oda hazır, istemci bekleniyor).");
                backoff = 1;

                let mut room_ticket_key: Option<Vec<u8>> = None;
                let mut relay_senders: HashMap<String, Arc<WSRelaySender>> = HashMap::new();
                let mut relay_reasms: HashMap<String, Reassembler> = HashMap::new();
                // Uçtan uca şifreli aktarmada istemci→ofis yönünün çözücüsü (cid başına). Burada olan cid'den şifresiz
                // çerçeve kabul edilmez (düşürme saldırısı).
                let mut relay_cozucu: HashMap<String, crate::relay_sifre::Yon> = HashMap::new();

                let mut ping_interval = tokio::time::interval(Duration::from_secs(20));
                ping_interval.tick().await;

                let mut fatal_error = false;

                loop {
                    tokio::select! {
                        _ = ping_interval.tick() => {
                            let _ = ws_tx.send(WsMessage::Ping(vec![]));
                        }
                        item = ws_source.next() => {
                            let raw = match item {
                                Some(Ok(WsMessage::Text(t))) => t,
                                Some(Ok(WsMessage::Pong(_))) => continue,
                                Some(Ok(WsMessage::Close(_))) | None => break,
                                Some(Ok(_)) => continue,
                                Some(Err(e)) => {
                                    println!("[!] Signaling bağlantısı koptu ({e}); {backoff}s sonra yeniden denenecek.");
                                    break;
                                }
                            };

                            let msg: Value = match serde_json::from_str(&raw) {
                                Ok(v) => v,
                                Err(e) => {
                                    println!("[!] Signaling mesaj hatası ({e}); bağlantı korunuyor.");
                                    continue;
                                }
                            };

                            let mtype = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            let cid = msg.get("cid").and_then(|v| v.as_str()).map(|s| s.to_string());
                            // İstemcinin hesabı (imzalı; doğrulanamazsa yok sayılır) → bağlantı kimliğine eşlenir.
                            if matches!(mtype, "offer" | "relay_hello" | "relay") {
                                if let (Some(c), Some(h)) = (cid.as_deref(), crate::kimlik::hesap_bileti(room_ticket_key.as_deref(), &msg)) {
                                    crate::kimlik::tunel_ayarla(c, &h);
                                }
                            }

                            if mtype == "joined" {
                                room_ticket_key = msg
                                    .get("ticket_key")
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| hex::decode(s).ok());
                                if room_ticket_key.is_some() {
                                    println!("[*] Bulgu #6 savunma-derinliği: bilet doğrulaması AKTİF (signaling gerçek kimlik doğrulaması bildirdi).");
                                    *ISARET_KANALI.lock() = Some(ws_tx.clone());
                                }
                            } else if matches!(mtype, "offer" | "relay_hello" | "relay")
                                && !_ticket_ok(room_ticket_key.as_deref(), &msg)
                            {
                                let short: String = cid.as_deref().unwrap_or("").chars().take(6).collect();
                                println!("[!] Bilet doğrulaması BAŞARISIZ [{short}] — istek reddedildi (signaling'in oda-kabul kararına güvenilmiyor).");
                            } else if mtype == "offer" {
                                if let (Some(c), Some(sdp)) = (cid.as_deref(), msg.get("sdp").and_then(|v| v.as_str())) {
                                    let sdptype = msg.get("sdptype").and_then(|v| v.as_str()).unwrap_or("offer");
                                    if let Err(e) = session.on_offer(&ws_tx, c, sdp, sdptype).await {
                                        println!("[!] Offer işleme hatası: {e}");
                                    }
                                }
                            } else if mtype == "relay_hello" {
                                if let Some(c) = cid {
                                    if let Some(s) = relay_senders.remove(&c) {
                                        s.close();
                                    }
                                    relay_cozucu.remove(&c);
                                    let short: String = c.chars().take(6).collect();
                                    // İstemcinin tek kullanımlık açık anahtarı varsa uçtan uca şifreli aktarma.
                                    let el_sikis = msg
                                        .get("pub")
                                        .and_then(|v| v.as_str())
                                        .map(|p| B64_STD.decode(p).map_err(|_| "açık anahtar base64 değil".to_string())
                                            .and_then(|b| crate::relay_sifre::ofis_el_sikis(&b)));
                                    match el_sikis {
                                        Some(Ok((ofis_pub, gonder, al))) => {
                                            relay_reasms.insert(c.clone(), Reassembler::new());
                                            relay_cozucu.insert(c.clone(), al);
                                            relay_senders.insert(c.clone(), WSRelaySender::new(ws_tx.clone(), Some(c.clone()), Some(gonder)));
                                            let _ = ws_tx.send(WsMessage::Text(
                                                json!({"type": "relay_ready", "cid": c, "pub": B64_STD.encode(ofis_pub), "v": 1}).to_string(),
                                            ));
                                            println!("[+] WS-relay modu [{short}]: tünel kuruldu (uçtan uca şifreli).");
                                        }
                                        Some(Err(e)) => println!("[!] WS-relay [{short}] el sıkışması reddedildi: {e}"),
                                        None if SIFRESIZ_AKTARMA_KABUL => {
                                            relay_reasms.insert(c.clone(), Reassembler::new());
                                            relay_senders.insert(c.clone(), WSRelaySender::new(ws_tx.clone(), Some(c.clone()), None));
                                            let _ = ws_tx.send(WsMessage::Text(json!({"type": "relay_ready", "cid": c}).to_string()));
                                            println!("[!] WS-relay modu [{short}]: ŞİFRESİZ tünel (eski tarayıcı kodu; geçiş süresince kabul edildi).");
                                        }
                                        None => println!("[!] WS-relay [{short}]: şifresiz aktarma isteği reddedildi (tarayıcı kodu eski)."),
                                    }
                                }
                            } else if mtype == "relay" {
                                if let Some(c) = cid {
                                    // Şifreli tünelde yalnız "e" (şifreli) çerçeve; şifresiz tünelde (geçiş) yalnız "b64".
                                    let cerceve: Option<Vec<u8>> = match relay_cozucu.get_mut(&c) {
                                        Some(al) => match msg.get("e").and_then(|v| v.as_str()).map(|e| B64_STD.decode(e)) {
                                            Some(Ok(sifreli)) => match al.coz(&sifreli) {
                                                Ok(acik) => Some(acik),
                                                Err(e) => {
                                                    // Değiştirilmiş / tekrar / sırası bozuk: tünel kapatılır, istemci yeniden el sıkışır.
                                                    let short: String = c.chars().take(6).collect();
                                                    println!("[!] WS-relay [{short}]: {e}; tünel kapatıldı.");
                                                    relay_cozucu.remove(&c);
                                                    relay_reasms.remove(&c);
                                                    if let Some(s) = relay_senders.remove(&c) {
                                                        s.close();
                                                    }
                                                    None
                                                }
                                            },
                                            _ => {
                                                println!("[!] WS-relay: şifreli tünelde şifresiz çerçeve reddedildi.");
                                                None
                                            }
                                        },
                                        None if SIFRESIZ_AKTARMA_KABUL && relay_senders.contains_key(&c) => {
                                            msg.get("b64").and_then(|v| v.as_str()).and_then(|b| B64_STD.decode(b).ok())
                                        }
                                        None => None,
                                    };
                                    let (Some(bytes), Some(sender)) = (cerceve, relay_senders.get(&c).cloned()) else { continue };
                                    let reasm = relay_reasms.entry(c.clone()).or_insert_with(Reassembler::new);
                                    {
                                        {
                                            match reasm.feed(&bytes) {
                                                Ok(Some(rmsg)) if rmsg.kind == "ping" => {
                                                    let s_clone = sender.clone();
                                                    tokio::spawn(async move {
                                                        let _ = send_message(&s_clone, &rmsg.id, "pong", Some(json!({})), b"").await;
                                                    });
                                                }
                                                Ok(Some(rmsg)) if rmsg.kind == "req" => {
                                                    let s_clone = sender.clone();
                                                    tokio::spawn(async move {
                                                        let cid_short: String = s_clone.cid.as_deref().unwrap_or("").chars().take(8).collect();
                                                        let meta = rmsg.meta.unwrap_or_else(|| json!({}));
                                                        let hesap = s_clone.cid.as_deref().and_then(crate::kimlik::tunel_hesabi);
                                                        let (status, headers, body) = handle_uyap_request(&meta, &rmsg.body, &format!("relay:{cid_short}"), hesap).await;
                                                        let res_meta = json!({"status": status, "headers": headers});
                                                        if let Err(e) = send_message(&s_clone, &rmsg.id, "res", Some(res_meta), &body).await {
                                                            println!("[!] Relay cevabi gonderilemedi: {e}");
                                                        }
                                                    });
                                                }
                                                Ok(_) => {}
                                                Err(e) => println!("[!] Relay çerçeve çözme hatası: {e}"),
                                            }
                                        }
                                    }
                                }
                            } else if mtype == "peer_left" {
                                if let Some(c) = cid {
                                    let short: String = c.chars().take(6).collect();
                                    println!("[*] İstemci ayrıldı [{short}]; bağlantısı kapatılıyor.");
                                    session.close_one(&c).await;
                                    crate::kimlik::tunel_sil(&c);
                                    if let Some(s) = relay_senders.remove(&c) {
                                        s.close();
                                    }
                                    relay_reasms.remove(&c);
                                    relay_cozucu.remove(&c);
                                } else {
                                    session.close_all().await;
                                    crate::kimlik::tunel_temizle();
                                    for (_, s) in relay_senders.drain() {
                                        s.close();
                                    }
                                    relay_reasms.clear();
                                    relay_cozucu.clear();
                                }
                            } else if mtype == "error" {
                                let err_msg = msg.get("error").and_then(|v| v.as_str()).unwrap_or("");
                                println!("[!] Signaling reddetti: {err_msg}");
                                fatal_error = true;
                                break;
                            }
                        }
                    }
                }

                for (_, s) in relay_senders.drain() {
                    s.close();
                }
                *ISARET_KANALI.lock() = None;
                writer_task.abort();

                if fatal_error {
                    return;
                }
            }
            Err(e) => {
                println!("[!] Signaling bağlantısı koptu ({e}); {backoff}s sonra yeniden denenecek.");
            }
        }

        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = std::cmp::min(backoff * 2, 30);
    }
}

pub async fn run_office(mut args: OfficeArgs) -> Result<(), String> {
    let user = args
        .user
        .clone()
        .or_else(|| args.room.clone())
        .unwrap_or_default();
    let sig_url = args.signaling.clone().unwrap_or_default();

    is_listesi::baglantidan_configure(
        &sig_url,
        &args.office_code,
        &user,
        args.password.as_deref().unwrap_or(""),
    );

    runtime::dongu_icin_sifirla();

    uyap_proxy::configure(
        &args.host,
        args.proxy_port,
        args.pin.as_deref(),
        args.cert_id.as_deref(),
        args.interval,
        args.no_verify,
        &user,
        args.password.as_deref(),
    );
    // İş Listesi: bu bilgisayardaki isteklerin sahibi (paylaşımı başlatan hesap) + büro üyeleri (ad, rol).
    crate::kimlik::ofis_hesabi_ayarla(&crate::kimlik::hesap_adi(&user, &args.office_code));
    tokio::spawn(crate::is_api::uyeleri_esitle());
    // Telegram / telefon için içeriksiz bildirim sinyali (plan L6).
    crate::is_isaret::baslat();
    // UYAP oturumunu ilk istekte değil hemen aç (durum satırı "kuruldu"yu ya da hatayı göstersin).
    tokio::spawn(uyap_proxy::oturumu_hemen_ac());

    let bind_host = uyap_proxy::_resolve_bind_host(&args.host);
    let mut lan_token: Option<String> = None;
    let mut lan_tls_handle = None;

    if !matches!(bind_host.as_str(), "127.0.0.1" | "::1" | "localhost") {
        let mut rnd = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut rnd);
        let bilet = B64_URL.encode(rnd);
        let (sert, anahtar, izi) = lan_tls::sertifika_hazirla(&detect_local_ips())?;
        lan_token = Some(lan_tls::bilet_birlestir(&bilet, &izi));
        uyap_proxy::set_gw_lan_ticket(Some(bilet));
        let lan_srv = uyap_proxy::build_server(
            &bind_host,
            *lan_tls::LAN_TLS_PORT,
            false,
            Some(sert),
            Some(anahtar),
            "off",
        );
        lan_tls_handle = Some(tokio::spawn(async move { lan_srv.run().await }));
        println!(
            "[i] Yerel ağ HTTPS kapısı: https://<bu-bilgisayar-ip>:{}/__uyap_lan__/giris",
            *lan_tls::LAN_TLS_PORT
        );
        println!("[i] Sertifika parmak izi (SHA-256, tarayıcı uyarısında karşılaştırın): {izi}");
    } else {
        uyap_proxy::set_gw_lan_ticket(None);
    }
    args.lan_token = lan_token;

    let ice_servers = if !sig_url.is_empty() && (sig_url.contains("127.0.0.1") || sig_url.contains("localhost")) {
        Vec::new()
    } else {
        fetch_ice_servers(&sig_url, 15.0).await
    };

    let http_host = if lan_tls_handle.is_some() {
        "127.0.0.1".to_string()
    } else {
        args.host.clone()
    };
    let http_srv = uyap_proxy::build_server(&http_host, args.proxy_port, true, None, None, "auto");
    let http_handle = tokio::spawn(async move { http_srv.run().await });

    let sig_task = tokio::spawn(run_signaling(args, ice_servers));
    let kapi_task = tokio::spawn(async {
        sira_kapisi::denetci().await;
    });

    tokio::select! {
        res = http_handle => {
            if let Err(e) = res {
                println!("[!] HTTP sunucusu durdu: {e}");
            }
        }
        res = async {
            if let Some(h) = lan_tls_handle {
                h.await
            } else {
                std::future::pending().await
            }
        } => {
            if let Err(e) = res {
                println!("[!] LAN TLS sunucusu durdu: {e}");
            }
        }
        _ = sig_task => {}
        _ = kapi_task => {}
    }

    Ok(())
}

pub use run_office as amain;

pub fn main() {
    let args = OfficeArgs::parse();
    if args.signaling.is_none() || args.room.is_none() {
        println!("[!] --signaling ve --room zorunlu (ya da UYAP_SIGNALING / UYAP_ROOM).");
        std::process::exit(2);
    }
    if args.pin.is_none() {
        println!("[!] UYARI: PIN verilmedi (--pin / UYAP_PIN). Giriş başarısız olabilir.");
    }

    let rt = crate::build_tokio_runtime().expect("Tokio runtime başlatılamadı");
    rt.block_on(async move {
        tokio::select! {
            res = run_office(args) => {
                if let Err(e) = res {
                    eprintln!("[!] Ofis ajanı hatası: {e}");
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\n[!] Ofis ajanı kapatıldı.");
            }
        }
    });
}
