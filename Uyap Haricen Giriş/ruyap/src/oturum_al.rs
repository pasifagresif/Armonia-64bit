//! "Var olan oturumu al" (26.11.2026): ofis sunucusu olmayan bilgisayar, ofisin açık UYAP oturumunu kullanır.
//!
//! 1. cokluuyap.com sinyal sunucusuna kullanıcının hesabıyla `home` rolünde kısa bir bağlantı: ofis çevrimiçiyse sunucu
//!    ofisin yerel ağ bilgisini (`local_ips`, `port`, `lan_token` = bilet + sertifika parmak izi) verir; bağlantı kapanır.
//! 2. Aynı ağdaysa: ofisin yerel ağ kapısına (8843, kendinden imzalı TLS) parmak izi SABİTLİ bağlanılır (sır
//!    gönderilmeden önce sertifika doğrulanır), kullanıcının kendi hesabıyla `/__uyap_lan__/giris` → kişiye özel
//!    `uyap_lan` çerezi (İş Listesi kişiyi tanır; olmazsa bilet ile Basic).
//! 3. Bu bilgisayarda 127.0.0.1:8800'de vekil açılır; tarayıcı ofis sunucusundaki "UYAP'ı aç" ile aynı tek kullanımlık
//!    kod + çerez akışıyla açılır. Vekil istekleri ofise iletir (`X-Forwarded-Host` ile sayfalardaki adresler
//!    127.0.0.1:8800'e göre yazılır).
//! 4. Aynı ağda değilse çağıran cokluuyap.com'u (tarayıcı tüneli) açar.
//!
//! Ofis sunucusunda değişiklik gerekmez; yalnız "Yerel ağdan doğrudan erişim" açık olmalı (varsayılan açık).
//! Linux ve Windows'ta aynı kod.
use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::lan_tls;
use crate::uyap_proxy;

pub const YEREL_PORT: u16 = 8800;

/// Sinyal sunucusunun verdiği ofis bilgisi.
#[derive(Debug, Clone, Default)]
pub struct OfisBilgisi {
    pub cevrimici: bool,
    pub ips: Vec<String>,
    pub port: u16,
    pub lan_token: String,
}

/// Arayüze dönen sonuç.
pub enum Sonuc {
    /// Yerel ağdan bağlandı; vekil 127.0.0.1:8800'de çalışıyor. `dur` gönderilince (ya da düşünce) kapanır.
    Yerel { ip: String, kisi: bool, dur: tokio::sync::oneshot::Sender<()> },
    /// Aynı ağda değil (ya da ofis yerel ağ kapısı kapalı): cokluuyap.com açılmalı. Gerekçe günlüğe.
    Web(String),
    Hata(String),
    /// Vekil beklenmedik biçimde durdu.
    Bitti(String),
}

/// Ofisin yerel ağ bilgisini sinyal sunucusundan alır (`home` rolü; kısa bağlantı).
pub async fn ofis_bilgisi(sinyal: &str, kullanici: &str, ofis_kodu: &str, parola: &str) -> Result<OfisBilgisi, String> {
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(25), tokio_tungstenite::connect_async(sinyal))
        .await
        .map_err(|_| "cokluuyap.com zaman aşımına uğradı; birkaç saniye sonra yeniden deneyin.".to_string())?
        .map_err(|e| format!("cokluuyap.com'a bağlanılamadı: {e}"))?;
    let katil = json!({"role": "home", "room": kullanici, "office_code": ofis_kodu, "password": parola});
    ws.send(Message::Text(katil.to_string())).await.map_err(|e| format!("cokluuyap.com'a bağlanılamadı: {e}"))?;
    let mut bilgi = OfisBilgisi { port: *lan_tls::LAN_TLS_PORT, ..Default::default() };
    let oku = |v: &Value, b: &mut OfisBilgisi| {
        if let Some(l) = v.get("local_ips").and_then(Value::as_array) {
            let ips: Vec<String> = l.iter().filter_map(|x| x.as_str().map(str::to_string)).filter(|s| !s.is_empty()).collect();
            if !ips.is_empty() {
                b.ips = ips;
            }
        }
        if let Some(p) = v.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0) {
            b.port = p;
        }
        if let Some(t) = v.get("lan_token").and_then(Value::as_str).filter(|t| !t.is_empty()) {
            b.lan_token = t.to_string();
        }
    };
    let bitis = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut katildi = false;
    loop {
        // Katıldıktan sonra ofis çevrimiçiyse "start" (yerel ağ bilgisiyle) hemen gelir; en çok 3 sn beklenir.
        let kalan = if katildi {
            Duration::from_secs(3).min(bitis.saturating_duration_since(tokio::time::Instant::now()))
        } else {
            bitis.saturating_duration_since(tokio::time::Instant::now())
        };
        let m = match tokio::time::timeout(kalan, ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => t,
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => return Err(format!("cokluuyap.com bağlantısı koptu: {e}")),
            Ok(None) | Err(_) => break,
        };
        let v: Value = serde_json::from_str(&m).unwrap_or(Value::Null);
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "error" => {
                let _ = ws.close(None).await;
                return Err(v.get("error").and_then(Value::as_str).unwrap_or("cokluuyap.com girişi reddetti.").to_string());
            }
            "joined" => {
                katildi = true;
                bilgi.cevrimici = v.get("peer_present").and_then(Value::as_bool).unwrap_or(false);
                oku(&v, &mut bilgi);
                if !bilgi.cevrimici || !bilgi.lan_token.is_empty() {
                    break;
                }
            }
            "start" => {
                bilgi.cevrimici = true;
                oku(&v, &mut bilgi);
                break;
            }
            _ => {}
        }
    }
    let _ = ws.close(None).await;
    if !katildi {
        return Err("cokluuyap.com yanıt vermedi.".into());
    }
    Ok(bilgi)
}

/// Ofisin yerel ağ kapısı: sabitli TLS istemcisi + kimlik.
pub struct LanHedef {
    pub taban: String,
    pub ip: String,
    istemci: reqwest::Client,
    bilet: String,
    hesap: String,
    parola: String,
    cerez: Mutex<Option<String>>,
}

impl LanHedef {
    /// Kullanıcının kendi hesabıyla kişiye özel `uyap_lan` çerezi (ofis bulutta doğrular; parola saklanmaz).
    async fn giris(&self) -> Option<String> {
        let r = self
            .istemci
            .post(format!("{}/__uyap_lan__/giris", self.taban))
            .form(&[("kullanici", self.hesap.as_str()), ("parola", self.parola.as_str()), ("hedef", "/giris")])
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .ok()?;
        for v in r.headers().get_all("set-cookie") {
            let s = v.to_str().unwrap_or("");
            if let Some(d) = s.split(';').next().and_then(|p| p.trim().strip_prefix(&format!("{}=", uyap_proxy::LAN_CEREZ))) {
                if !d.is_empty() {
                    return Some(d.to_string());
                }
            }
        }
        None
    }
}

/// Aynı ağda mı: ofisin bildirdiği adreslerde yerel ağ kapısı parmak iziyle doğrulanır, bilet denenir. Değilse `None`.
pub async fn lan_bul(b: &OfisBilgisi, hesap: &str, parola: &str) -> Result<LanHedef, String> {
    if b.lan_token.is_empty() || b.ips.is_empty() {
        return Err("ofis sunucusunda \"Yerel ağdan doğrudan erişim\" kapalı".into());
    }
    let (bilet, iz) = lan_tls::bilet_ayir(&b.lan_token);
    let iz = iz.ok_or("ofis sunucusu eski sürüm (yerel ağ kapısında TLS yok)")?;
    for ip in &b.ips {
        let Some(cfg) = lan_tls::sabitli_baglam(ip, b.port, &iz, 1.5).await else { continue };
        let istemci = match reqwest::Client::builder()
            .use_preconfigured_tls((*cfg).clone())
            .no_proxy()
            .no_gzip()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
        {
            Ok(c) => c,
            Err(_) => continue,
        };
        let taban = format!("https://{}:{}", ip, b.port);
        let ping = istemci
            .get(format!("{taban}/__ping__"))
            .basic_auth("", Some(&bilet))
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        if !matches!(&ping, Ok(r) if r.status().is_success()) {
            continue;
        }
        let h = LanHedef {
            taban,
            ip: ip.clone(),
            istemci,
            bilet: bilet.clone(),
            hesap: hesap.to_string(),
            parola: parola.to_string(),
            cerez: Mutex::new(None),
        };
        let c = h.giris().await;
        *h.cerez.lock() = c;
        return Ok(h);
    }
    Err("ofis sunucusunun yerel ağ adresine ulaşılamadı (aynı ağda değilsiniz ya da güvenlik duvarı 8843'ü kapatıyor)".into())
}

// ── Vekil (127.0.0.1:8800) ────────────────────────────────────────────────────────────────

/// Tarayıcı isteği güvenli mi: Host yalnız 127.0.0.1 / localhost (DNS rebinding), çapraz site alt kaynak yok,
/// Origin varsa yerel.
fn tarayici_reddi(h: &HeaderMap, yontem: &Method) -> Option<&'static str> {
    let host = uyap_proxy::_hostname_of(h.get("host").and_then(|v| v.to_str().ok()).unwrap_or(""));
    if !matches!(host.as_str(), "127.0.0.1" | "localhost" | "") {
        return Some("Beklenmeyen Host başlığı (olası DNS rebinding).");
    }
    let baslik = |a: &str| h.get(a).and_then(|v| v.to_str().ok()).unwrap_or("").trim().to_ascii_lowercase();
    if baslik("sec-fetch-site") == "cross-site" {
        let gezinme = baslik("sec-fetch-mode") == "navigate"
            && matches!(baslik("sec-fetch-dest").as_str(), "document" | "")
            && (*yontem == Method::GET || *yontem == Method::HEAD);
        if !gezinme {
            return Some("Çapraz-site alt-kaynak isteği reddedildi.");
        }
    }
    if let Some(o) = h.get("origin").and_then(|v| v.to_str().ok()) {
        if !matches!(uyap_proxy::_hostname_of(o).as_str(), "127.0.0.1" | "localhost") {
            return Some("Yetkisiz Origin (çapraz-site istek reddedildi).");
        }
    }
    None
}

fn json_hata(durum: StatusCode, m: &str) -> Response {
    (durum, [("Content-Type", "application/json; charset=utf-8"), ("Cache-Control", "no-store")], json!({"detail": m}).to_string())
        .into_response()
}

async fn kod_ver(headers: HeaderMap) -> Response {
    if tarayici_reddi(&headers, &Method::POST).is_some()
        || !uyap_proxy::yerel_jeton_gecerli(headers.get("x-uyap-local-token").and_then(|v| v.to_str().ok()).unwrap_or(""))
    {
        return json_hata(StatusCode::FORBIDDEN, "Yerel yetki jetonu gerekli.");
    }
    (StatusCode::OK, [("Content-Type", "application/json"), ("Cache-Control", "no-store")], json!({"kod": uyap_proxy::_giris_kodu_uret()}).to_string())
        .into_response()
}

#[derive(serde::Deserialize)]
struct GirisSorgu {
    #[serde(default)]
    k: String,
    #[serde(default)]
    hedef: String,
}

async fn giris(axum::extract::Query(q): axum::extract::Query<GirisSorgu>, headers: HeaderMap) -> Response {
    if tarayici_reddi(&headers, &Method::GET).is_some() || !uyap_proxy::_giris_kodu_kullan(&q.k) {
        return giris_sayfasi();
    }
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", uyap_proxy::_guvenli_hedef(&q.hedef))
        .header("Cache-Control", "no-store")
        .header("Set-Cookie", format!("{}={}; HttpOnly; SameSite=Strict; Path=/", uyap_proxy::YEREL_CEREZ, *uyap_proxy::GW_BROWSER_SECRET))
        .body(Body::empty())
        .unwrap()
}

fn giris_sayfasi() -> Response {
    (StatusCode::FORBIDDEN, [("Content-Type", "text/html; charset=utf-8"), ("Cache-Control", "no-store")], uyap_proxy::yerel_giris_sayfasi())
        .into_response()
}

// Ofise iletilmeyen istek başlıkları (bağlantıya özgü olanlar + yerel sırlar; kimlik vekil tarafından eklenir).
const ILETILMEZ: &[&str] = &[
    "host", "connection", "keep-alive", "proxy-connection", "proxy-authorization", "te", "trailer", "transfer-encoding",
    "upgrade", "content-length", "cookie", "authorization", "x-uyap-local-token", "accept-encoding", "x-forwarded-host",
    "x-forwarded-proto", "x-forwarded-for",
];
const YANIT_ILETILMEZ: &[&str] = &["connection", "keep-alive", "transfer-encoding", "content-length", "trailer", "upgrade"];

async fn ilet(State(h): State<Arc<LanHedef>>, ConnectInfo(adres): ConnectInfo<SocketAddr>, istek: Request) -> Response {
    let (parca, govde) = istek.into_parts();
    if !adres.ip().is_loopback() {
        return json_hata(StatusCode::FORBIDDEN, "Yalnız bu bilgisayardan.");
    }
    if let Some(r) = tarayici_reddi(&parca.headers, &parca.method) {
        return json_hata(StatusCode::FORBIDDEN, r);
    }
    let jeton = parca.headers.get("x-uyap-local-token").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !uyap_proxy::yerel_cerez_gecerli(&parca.headers) && !uyap_proxy::yerel_jeton_gecerli(jeton) {
        let html = parca.headers.get("accept").and_then(|v| v.to_str().ok()).unwrap_or("").contains("text/html");
        if (parca.method == Method::GET || parca.method == Method::HEAD) && html {
            return giris_sayfasi();
        }
        return json_hata(StatusCode::FORBIDDEN, "Yerel yetki gerekli: programdaki \"UYAP'ı aç\" ile açın.");
    }
    let govde = match axum::body::to_bytes(govde, 512 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return json_hata(StatusCode::PAYLOAD_TOO_LARGE, "İstek gövdesi okunamadı."),
    };
    let yol = parca.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let hedef_url = format!("{}{}", h.taban, yol);
    let yontem = reqwest::Method::from_bytes(parca.method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET);
    let ileri_host = parca.headers.get("host").and_then(|v| v.to_str().ok()).unwrap_or("127.0.0.1:8800").to_string();

    // Ofis çerezi düşmüşse (ofis programı yeniden başladı) bir kez yeniden giriş yapılıp istek tekrarlanır.
    for deneme in 0..2 {
        let mut b = h.istemci.request(yontem.clone(), &hedef_url);
        for (ad, deger) in parca.headers.iter() {
            if !ILETILMEZ.contains(&ad.as_str()) {
                b = b.header(ad.as_str(), deger.as_bytes());
            }
        }
        // Tarayıcının bu vekile ait olmayan çerezleri (ör. UYAP'ın kendi çerezleri) ofise gider; yerel çerez gitmez.
        let digerleri: Vec<String> = parca
            .headers
            .get_all("cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|s| s.split(';'))
            .map(str::trim)
            .filter(|p| !p.is_empty() && !p.starts_with(&format!("{}=", uyap_proxy::YEREL_CEREZ)) && !p.starts_with(&format!("{}=", uyap_proxy::LAN_CEREZ)))
            .map(str::to_string)
            .collect();
        let lan = h.cerez.lock().clone();
        let mut cerezler = digerleri;
        match &lan {
            Some(c) => cerezler.push(format!("{}={}", uyap_proxy::LAN_CEREZ, c)),
            None => b = b.basic_auth("", Some(&h.bilet)),
        }
        if !cerezler.is_empty() {
            b = b.header("Cookie", cerezler.join("; "));
        }
        b = b.header("X-Forwarded-Host", ileri_host.as_str()).header("X-Forwarded-Proto", "http").body(govde.clone());
        let yanit = match b.send().await {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    [("Content-Type", "text/html; charset=utf-8"), ("Cache-Control", "no-store")],
                    format!(
                        "<!doctype html><meta charset='utf-8'><body style='font-family:Segoe UI,Arial,sans-serif;max-width:560px;margin:60px auto'>\
                         <h2>Ofis bilgisayarına ulaşılamadı</h2><p>{}</p><p>Ofis sunucusunun açık olduğundan emin olup Çoklu UYAP'ta \
                         \"Var olan oturumu al\"a yeniden basın.</p></body>",
                        html_kacis(&e.to_string())
                    ),
                )
                    .into_response();
            }
        };
        if deneme == 0 && yanit.status() == reqwest::StatusCode::UNAUTHORIZED && lan.is_some() {
            if let Some(yeni) = h.giris().await {
                *h.cerez.lock() = Some(yeni);
                continue;
            }
            *h.cerez.lock() = None;   // kişiye özel giriş olmuyorsa bilet ile (kişi "lan" görünür)
            continue;
        }
        return yanit_cevir(&h, yanit);
    }
    json_hata(StatusCode::BAD_GATEWAY, "Ofis bilgisayarı isteği kabul etmedi.")
}

fn yanit_cevir(h: &LanHedef, yanit: reqwest::Response) -> Response {
    let mut r = Response::builder().status(yanit.status().as_u16());
    for (ad, deger) in yanit.headers().iter() {
        let a = ad.as_str();
        if YANIT_ILETILMEZ.contains(&a) {
            continue;
        }
        let Ok(s) = deger.to_str() else {
            r = r.header(a, deger.as_bytes());
            continue;
        };
        if a == "set-cookie" {
            if s.trim_start().starts_with(&format!("{}=", uyap_proxy::LAN_CEREZ)) {
                continue;   // ofis kapısının çerezi vekilde kalır, tarayıcıya verilmez
            }
            // Vekil düz HTTP: "Secure" bayrağı çerezi düşürür
            let temiz: Vec<&str> = s.split(';').filter(|p| !p.trim().eq_ignore_ascii_case("secure")).collect();
            if let Ok(v) = HeaderValue::from_str(&temiz.join(";")) {
                r = r.header(a, v);
            }
            continue;
        }
        if a == "location" {
            let yerel = s.strip_prefix(h.taban.as_str()).map(|x| if x.is_empty() { "/" } else { x }).unwrap_or(s);
            if let Ok(v) = HeaderValue::from_str(yerel) {
                r = r.header(a, v);
            }
            continue;
        }
        r = r.header(a, deger.as_bytes());
    }
    r.body(Body::from_stream(yanit.bytes_stream())).unwrap_or_else(|_| json_hata(StatusCode::BAD_GATEWAY, "Yanıt aktarılamadı."))
}

fn html_kacis(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Akışın tamamı ayrı bir iş parçacığında: ofis bilgisi → yerel ağ denemesi → vekil. Sonuç `tx` ile döner;
/// vekil açıksa iş parçacığı vekil kapanana dek yaşar (kapanınca `Sonuc::Bitti`).
pub fn baslat(sinyal: String, kullanici: String, ofis_kodu: String, parola: String, tx: Sender<Sonuc>, gunluk: Sender<String>) {
    std::thread::spawn(move || {
        let rt = match crate::build_tokio_runtime() {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Sonuc::Hata(format!("Çalışma zamanı hatası: {e}")));
                return;
            }
        };
        rt.block_on(async move {
            let bilgi = match ofis_bilgisi(&sinyal, &kullanici, &ofis_kodu, &parola).await {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx.send(Sonuc::Hata(e));
                    return;
                }
            };
            if !bilgi.cevrimici {
                let _ = tx.send(Sonuc::Hata("Ofis sunucusu şu an çevrimiçi değil: ofisteki Çoklu UYAP'ta paylaşım açık olmalı.".into()));
                return;
            }
            let hesap = crate::kimlik::hesap_adi(&kullanici, &ofis_kodu);
            let hedef = match lan_bul(&bilgi, &hesap, &parola).await {
                Ok(h) => h,
                Err(neden) => {
                    let _ = tx.send(Sonuc::Web(neden));
                    return;
                }
            };
            let (ip, kisi) = (hedef.ip.clone(), hedef.cerez.lock().is_some());
            let _ = gunluk.send(format!(
                "[+] Ofis oturumu yerel ağdan alındı: {ip}:{} → 127.0.0.1:{YEREL_PORT}{}",
                bilgi.port,
                if kisi { "" } else { " (kişiye özel giriş yapılamadı; İş Listesi ofis adına görünür)" }
            ));
            let (dur_tx, dur_rx) = tokio::sync::oneshot::channel();
            // Dinleyici açılmadan "hazır" denmesin: önce bağlanır, sonra bildirilir.
            let (hazir_tx, hazir_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
            let gorev = tokio::spawn(vekil_calistir(hedef, dur_rx, hazir_tx));
            match hazir_rx.await {
                Ok(Ok(())) => {
                    let _ = tx.send(Sonuc::Yerel { ip, kisi, dur: dur_tx });
                }
                Ok(Err(e)) => {
                    let _ = tx.send(Sonuc::Hata(e));
                    return;
                }
                Err(_) => {
                    let _ = tx.send(Sonuc::Hata("Yerel vekil başlatılamadı.".into()));
                    return;
                }
            }
            let son = match gorev.await {
                Ok(Ok(())) => "Ofis oturumu bırakıldı.".to_string(),
                Ok(Err(e)) => e,
                Err(e) => format!("Yerel vekil çöktü: {e}"),
            };
            let _ = tx.send(Sonuc::Bitti(son));
        });
    });
}

/// Vekili 127.0.0.1:8800'de çalıştırır: dinleyici açılınca `hazir` bildirilir, `dur` gelince kapanır.
async fn vekil_calistir(
    h: LanHedef,
    dur: tokio::sync::oneshot::Receiver<()>,
    hazir: tokio::sync::oneshot::Sender<Result<(), String>>,
) -> Result<(), String> {
    let dinleyici = match tokio::net::TcpListener::bind(("127.0.0.1", YEREL_PORT)).await {
        Ok(d) => d,
        Err(e) => {
            let m = format!("127.0.0.1:{YEREL_PORT} kullanımda ({e}); bu bilgisayarda başka bir Çoklu UYAP açık olabilir.");
            let _ = hazir.send(Err(m.clone()));
            return Err(m);
        }
    };
    uyap_proxy::_init_local_token();
    let _ = hazir.send(Ok(()));
    let uygulama = Router::new()
        .route(crate::yerel_giris::KOD_YOLU, post(kod_ver))
        .route(crate::yerel_giris::GIRIS_YOLU, get(giris))
        .fallback(ilet)
        .with_state(Arc::new(h));
    axum::serve(dinleyici, uygulama.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async move {
            let _ = dur.await;
        })
        .await
        .map_err(|e| format!("Yerel vekil durdu: {e}"))
}

#[cfg(test)]
mod testler {
    use super::*;

    fn basliklar(ciftler: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (a, d) in ciftler {
            h.insert(axum::http::HeaderName::from_bytes(a.as_bytes()).unwrap(), HeaderValue::from_str(d).unwrap());
        }
        h
    }

    #[test]
    fn tarayici_korumasi() {
        assert!(tarayici_reddi(&basliklar(&[("host", "127.0.0.1:8800")]), &Method::GET).is_none());
        assert!(tarayici_reddi(&basliklar(&[("host", "kotu.example:8800")]), &Method::GET).is_some(), "DNS rebinding");
        assert!(tarayici_reddi(&basliklar(&[("host", "127.0.0.1:8800"), ("origin", "https://kotu.example")]), &Method::POST).is_some());
        assert!(tarayici_reddi(&basliklar(&[("host", "127.0.0.1:8800"), ("origin", "http://127.0.0.1:8800")]), &Method::POST).is_none());
        let capraz = [("host", "127.0.0.1:8800"), ("sec-fetch-site", "cross-site"), ("sec-fetch-mode", "cors"), ("sec-fetch-dest", "empty")];
        assert!(tarayici_reddi(&basliklar(&capraz), &Method::GET).is_some(), "çapraz site alt kaynak");
        let gezinme = [("host", "127.0.0.1:8800"), ("sec-fetch-site", "cross-site"), ("sec-fetch-mode", "navigate"), ("sec-fetch-dest", "document")];
        assert!(tarayici_reddi(&basliklar(&gezinme), &Method::GET).is_none(), "adres çubuğundan gezinme serbest");
    }
}
