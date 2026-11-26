//! Sıra Kapısı Eşleştirme & Yönetici İşlemleri (`uyap_core/sira_eslestirme.py` 1:1 portu)

use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use x509_parser::prelude::*;

use crate::sira_kapisi::{
    _lan_ipv4, bot_ekle, kapi_hazirla, kok_der, varsayilan_lan_ip, HOSTS_ETIKET, UYAP_HOST,
};

pub const ESLESTIRME_PORT: u16 = 4430;
pub const PENCERE_SN: u64 = 3600;
pub const KARAR_SN: u64 = 300;
pub const ILETI_EN_BUYUK: usize = 8192;
pub const KURAL_ADI: &str = "CokluUyap Sira Kapisi (443, 4430)";
pub const KOK_ADI: &str = "cokluuyap Sira Kapisi Koku";
pub const CIKIS_METNI: &str = "Sıra Kapısı durduruldu.";

#[derive(Debug, Clone)]
pub enum EslestirmeHatasi {
    YetkiReddedildi,
    Mesaj(String),
}

impl fmt::Display for EslestirmeHatasi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EslestirmeHatasi::YetkiReddedildi => write!(f, "Yönetici izni verilmedi."),
            EslestirmeHatasi::Mesaj(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for EslestirmeHatasi {}

impl From<String> for EslestirmeHatasi {
    fn from(s: String) -> Self {
        EslestirmeHatasi::Mesaj(s)
    }
}

#[derive(Debug, Clone)]
pub enum SunucuOlay {
    Uyari { mesaj: String },
    Istek { ip: String, ad: String, kod: String },
    Bitti { basarili: bool, mesaj: String },
}

pub fn cikis_metni(kod: i32) -> &'static str {
    match kod {
        0 => "İşlem tamamlandı.",
        1 => "İşlem başarısız oldu.",
        2 => "Geçersiz parametre.",
        _ => "Yönetici işlemi tamamlanamadı.",
    }
}

pub fn dogrulama_kodu(a: &[u8], b: &[u8]) -> String {
    let mut h = Sha256::new();
    if a <= b {
        h.update(a);
        h.update(b);
    } else {
        h.update(b);
        h.update(a);
    }
    let dig = h.finalize();
    let num = u32::from_be_bytes([dig[0], dig[1], dig[2], dig[3]]) % 1_000_000;
    format!("{:06}", num)
}

pub fn kok_dogrula(der: &[u8]) -> Result<(), String> {
    let (_, cert) =
        X509Certificate::from_der(der).map_err(|e| format!("Geçersiz sertifika: {:?}", e))?;
    let is_ca = cert
        .basic_constraints()
        .ok()
        .flatten()
        .map(|bc| bc.value.ca)
        .unwrap_or(false);
    if !is_ca {
        return Err("Sertifika CA değil.".to_string());
    }
    let nc_oid = x509_parser::oid_registry::OID_X509_EXT_NAME_CONSTRAINTS;
    let has_nc = cert
        .extensions()
        .iter()
        .any(|ext| ext.oid == nc_oid && ext.critical);
    if !has_nc {
        return Err("Sertifikada kritik NameConstraints uzantısı yok.".to_string());
    }
    Ok(())
}

pub fn hosts_yolu() -> PathBuf {
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    Path::new(&sysroot)
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

pub fn hosts_guncelle(kapi_ip: Option<&str>) -> Result<(), String> {
    let yol = hosts_yolu();
    let mevcut = fs::read_to_string(&yol).unwrap_or_default();
    let mut satirlar: Vec<String> = mevcut
        .lines()
        .filter(|s| !s.contains(HOSTS_ETIKET))
        .map(|s| s.to_string())
        .collect();
    if let Some(ip) = kapi_ip {
        let temiz = _lan_ipv4(ip, "Kapı IP'si")?;
        satirlar.push(format!("{} {} {}", temiz, UYAP_HOST, HOSTS_ETIKET));
    }
    let mut cikis = satirlar.join("\r\n");
    if !cikis.is_empty() && !cikis.ends_with("\r\n") {
        cikis.push_str("\r\n");
    }
    fs::write(&yol, cikis.as_bytes()).map_err(|e| format!("hosts yazılamadı: {}", e))
}

pub fn hosts_kapi_ip(yol_opt: Option<&Path>) -> Option<String> {
    let yol = yol_opt.map(PathBuf::from).unwrap_or_else(hosts_yolu);
    let mevcut = fs::read_to_string(yol).ok()?;
    for satir in mevcut.lines() {
        if satir.contains(HOSTS_ETIKET) {
            let parca = satir.split_whitespace().next()?;
            if parca.parse::<Ipv4Addr>().is_ok() {
                return Some(parca.to_string());
            }
        }
    }
    None
}

// ── Windows Sertifika Deposu (crypt32.dll) ─────────────────────────────────────────────────

#[cfg(windows)]
mod win_cert {
    use super::*;
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    type HCERTSTORE = *mut c_void;

    #[repr(C)]
    struct CERT_CONTEXT {
        dw_cert_encoding_type: u32,
        pb_cert_encoded: *const u8,
        cb_cert_encoded: u32,
        p_cert_info: *const c_void,
        h_cert_store: HCERTSTORE,
    }

    const X509_ASN_ENCODING: u32 = 0x0000_0001;
    const PKCS_7_ASN_ENCODING: u32 = 0x0001_0000;
    const CERT_STORE_PROV_SYSTEM_W: *const u8 = 10usize as *const u8;
    const CERT_SYSTEM_STORE_LOCAL_MACHINE: u32 = 0x0002_0000;
    const CERT_STORE_ADD_REPLACE_EXISTING: u32 = 3;

    #[link(name = "crypt32")]
    extern "system" {
        fn CertOpenStore(
            lpsz_store_provider: *const u8,
            dw_msg_and_cert_encoding_type: u32,
            h_crypt_prov: usize,
            dw_flags: u32,
            pv_para: *const c_void,
        ) -> HCERTSTORE;
        fn CertCloseStore(h_cert_store: HCERTSTORE, dw_flags: u32) -> i32;
        fn CertEnumCertificatesInStore(
            h_cert_store: HCERTSTORE,
            p_prev_cert_context: *const CERT_CONTEXT,
        ) -> *const CERT_CONTEXT;
        fn CertAddEncodedCertificateToStore(
            h_cert_store: HCERTSTORE,
            dw_cert_encoding_type: u32,
            pb_cert_encoded: *const u8,
            cb_cert_encoded: u32,
            dw_add_disposition: u32,
            pp_cert_context: *mut *const CERT_CONTEXT,
        ) -> i32;
        fn CertDeleteCertificateFromStore(p_cert_context: *const CERT_CONTEXT) -> i32;
        fn CertDuplicateCertificateContext(p_cert_context: *const CERT_CONTEXT) -> *const CERT_CONTEXT;
    }

    fn to_wide(s: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn open_root_store() -> Result<HCERTSTORE, String> {
        let root_w = to_wide("ROOT");
        let store = unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                0,
                0,
                CERT_SYSTEM_STORE_LOCAL_MACHINE,
                root_w.as_ptr() as *const c_void,
            )
        };
        if store.is_null() {
            return Err("LocalMachine\\ROOT sertifika deposu açılamadı (yönetici yetkisi gerekir).".to_string());
        }
        Ok(store)
    }

    fn is_our_cert(der: &[u8]) -> bool {
        if let Ok((_, cert)) = X509Certificate::from_der(der) {
            let subj = cert.subject().to_string();
            return subj.contains(KOK_ADI) || subj.contains("CokluUyap Sira Kapisi");
        }
        false
    }

    pub fn kok_kur_impl(der: &[u8]) -> Result<(), String> {
        let store = open_root_store()?;
        let ok = unsafe {
            CertAddEncodedCertificateToStore(
                store,
                X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                der.as_ptr(),
                der.len() as u32,
                CERT_STORE_ADD_REPLACE_EXISTING,
                null_mut(),
            )
        };
        unsafe {
            CertCloseStore(store, 0);
        }
        if ok == 0 {
            return Err("Kök sertifika depoya eklenemedi.".to_string());
        }
        Ok(())
    }

    pub fn kok_kaldir_impl() -> Result<(), String> {
        let store = open_root_store()?;
        let mut to_delete = Vec::new();
        let mut ctx = null();
        loop {
            ctx = unsafe { CertEnumCertificatesInStore(store, ctx) };
            if ctx.is_null() {
                break;
            }
            let der = unsafe {
                std::slice::from_raw_parts(
                    (*ctx).pb_cert_encoded,
                    (*ctx).cb_cert_encoded as usize,
                )
            };
            if is_our_cert(der) {
                let dup = unsafe { CertDuplicateCertificateContext(ctx) };
                if !dup.is_null() {
                    to_delete.push(dup);
                }
            }
        }
        for dup in to_delete {
            unsafe {
                CertDeleteCertificateFromStore(dup);
            }
        }
        unsafe {
            CertCloseStore(store, 0);
        }
        Ok(())
    }

    pub fn kok_kurulu_mu_impl() -> bool {
        let Ok(store) = open_root_store() else {
            return false;
        };
        let mut found = false;
        let mut ctx = null();
        loop {
            ctx = unsafe { CertEnumCertificatesInStore(store, ctx) };
            if ctx.is_null() {
                break;
            }
            let der = unsafe {
                std::slice::from_raw_parts(
                    (*ctx).pb_cert_encoded,
                    (*ctx).cb_cert_encoded as usize,
                )
            };
            if is_our_cert(der) {
                found = true;
                break;
            }
        }
        unsafe {
            CertCloseStore(store, 0);
        }
        found
    }
}

#[cfg(not(windows))]
mod win_cert {
    pub fn kok_kur_impl(_der: &[u8]) -> Result<(), String> {
        Err("Yalnız Windows'ta destekleniyor.".to_string())
    }
    pub fn kok_kaldir_impl() -> Result<(), String> {
        Ok(())
    }
    pub fn kok_kurulu_mu_impl() -> bool {
        false
    }
}

pub fn kok_kur(der: &[u8]) -> Result<(), String> {
    kok_dogrula(der)?;
    win_cert::kok_kur_impl(der)
}

pub fn kok_kaldir() -> Result<(), String> {
    win_cert::kok_kaldir_impl()
}

pub fn kok_kurulu_mu() -> bool {
    win_cert::kok_kurulu_mu_impl()
}

pub fn bot_kur(kapi_ip: &str, kok_der_bytes: &[u8]) -> Result<(), String> {
    kok_kur(kok_der_bytes)?;
    hosts_guncelle(Some(kapi_ip))
}

pub fn bot_kaldir() -> Result<(), String> {
    let _ = hosts_guncelle(None);
    kok_kaldir()
}

pub fn guvenlik_duvari_var() -> bool {
    let out = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "show",
            "rule",
            &format!("name={}", KURAL_ADI),
        ])
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

pub fn guvenlik_duvari_kur(portlar: &[u16]) -> Result<(), String> {
    let port_str: String = if portlar.is_empty() {
        "443,4430".to_string()
    } else {
        portlar
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let _ = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={}", KURAL_ADI),
        ])
        .output();
    let out = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={}", KURAL_ADI),
            "dir=in",
            "action=allow",
            "protocol=TCP",
            &format!("localport={}", port_str),
            "profile=private,domain",
        ])
        .output()
        .map_err(|e| format!("netsh çalıştırılamadı: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "Güvenlik duvarı kuralı eklenemedi: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub fn yonetici_olarak_calistir(args: &[&str]) -> Result<i32, EslestirmeHatasi> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    fn to_w(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let exe = std::env::current_exe().map_err(|e| EslestirmeHatasi::Mesaj(e.to_string()))?;
    let exe_w = to_w(&exe.to_string_lossy());
    let verb_w = to_w("runas");
    let mut full_args = vec!["--sira-yonetici".to_string()];
    for a in args {
        full_args.push((*a).to_string());
    }
    let params = full_args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{}\"", a)
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let params_w = to_w(&params);

    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = verb_w.as_ptr();
    sei.lpFile = exe_w.as_ptr();
    sei.lpParameters = params_w.as_ptr();
    sei.nShow = SW_HIDE;

    let ok = unsafe { ShellExecuteExW(&mut sei) };
    if ok == 0 {
        return Err(EslestirmeHatasi::YetkiReddedildi);
    }
    let mut code: u32 = 0;
    if !sei.hProcess.is_null() {
        unsafe {
            WaitForSingleObject(sei.hProcess, 30_000);
            GetExitCodeProcess(sei.hProcess, &mut code);
            CloseHandle(sei.hProcess);
        }
    }
    Ok(code as i32)
}

#[cfg(not(windows))]
pub fn yonetici_olarak_calistir(_args: &[&str]) -> Result<i32, EslestirmeHatasi> {
    Err(EslestirmeHatasi::Mesaj(
        "Yalnız Windows'ta destekleniyor.".to_string(),
    ))
}

pub fn yonetici_main(args: &[String]) -> i32 {
    if args.is_empty() {
        return 2;
    }
    let res = match args[0].as_str() {
        "guvenlik-duvari" | "guvenlik_duvari" => guvenlik_duvari_kur(&[443, ESLESTIRME_PORT]),
        "bot-kur" | "bot_kur" => {
            if args.len() < 3 {
                Err("bot-kur <kapi_ip> <kok_der_b64>".to_string())
            } else {
                match base64::engine::general_purpose::STANDARD.decode(args[2].as_bytes()) {
                    Ok(der) => bot_kur(&args[1], &der),
                    Err(e) => Err(e.to_string()),
                }
            }
        }
        "bot-kaldir" | "bot_kaldir" => bot_kaldir(),
        // Ofis sunucusunun yerel ağ kapısı (8843; "Var olan oturumu al"), bkz. lan_tls::guvenlik_duvari_kur
        "lan-guvenlik-duvari" => crate::lan_tls::guvenlik_duvari_kur(),
        other => Err(format!("Bilinmeyen yönetici komutu: {}", other)),
    };
    match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

pub fn kapi_sina(kapi_ip: &str, _kok_der: Option<&[u8]>, timeout_sn: f64) -> String {
    let Ok(addr) = format!("{}:443", kapi_ip).parse::<SocketAddr>() else {
        return "Geçersiz kapı IP adresi.".to_string();
    };
    let dur = Duration::from_secs_f64(timeout_sn.max(1.0));
    match TcpStream::connect_timeout(&addr, dur) {
        Ok(_) => format!("Sıra kapısı ({}) erişilebilir.", kapi_ip),
        Err(e) => format!(
            "Sıra kapısına ({}:443) şu an ulaşılamadı ({}). Paylaşım açık olduğunda tekrar deneyin.",
            kapi_ip, e
        ),
    }
}

pub fn ofisleri_bul(timeout_secs: f64) -> Vec<(String, String)> {
    let mut bulunan = Vec::new();
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return bulunan;
    };
    let _ = sock.set_broadcast(true);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(250)));
    let _ = sock.send_to(b"UYAP_KAPI_ARA", ("255.255.255.255", ESLESTIRME_PORT));
    let bitis = Instant::now() + Duration::from_secs_f64(timeout_secs.max(0.2));
    let mut buf = [0u8; 2048];
    while Instant::now() < bitis {
        if let Ok((n, addr)) = sock.recv_from(&mut buf) {
            if let Ok(v) = serde_json::from_slice::<Value>(&buf[..n]) {
                let ip = v
                    .get("ip")
                    .and_then(|x| x.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| addr.ip().to_string());
                let ad = v
                    .get("ad")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if !bulunan.iter().any(|(existing_ip, _)| existing_ip == &ip) {
                    bulunan.push((ip, ad));
                }
            }
        }
    }
    bulunan
}

// ── Eşleştirme Sunucusu & İstemcisi ────────────────────────────────────────────────────────

pub struct EslestirmeSunucusu {
    aktif: Arc<AtomicBool>,
    olaylar: Arc<StdMutex<VecDeque<SunucuOlay>>>,
    karar: Arc<(StdMutex<Option<bool>>, Condvar)>,
    baslangic: StdMutex<Option<Instant>>,
}

impl EslestirmeSunucusu {
    pub fn new() -> Self {
        Self {
            aktif: Arc::new(AtomicBool::new(false)),
            olaylar: Arc::new(StdMutex::new(VecDeque::new())),
            karar: Arc::new((StdMutex::new(None), Condvar::new())),
            baslangic: StdMutex::new(None),
        }
    }

    pub fn baslat(&self) -> Result<(), EslestirmeHatasi> {
        kapi_hazirla(1.0, "sirala").map_err(EslestirmeHatasi::Mesaj)?;
        let lan_ip = varsayilan_lan_ip();
        let bind_ip = if lan_ip.is_empty() {
            "0.0.0.0".to_string()
        } else {
            lan_ip
        };
        let listener = TcpListener::bind((bind_ip.as_str(), ESLESTIRME_PORT)).map_err(|e| {
            EslestirmeHatasi::Mesaj(format!(
                "Eşleştirme portu {} açılamadı: {}",
                ESLESTIRME_PORT, e
            ))
        })?;
        let _ = listener.set_nonblocking(true);

        self.aktif.store(true, Ordering::SeqCst);
        *self.baslangic.lock().unwrap() = Some(Instant::now());
        if let Ok(mut g) = self.karar.0.lock() {
            *g = None;
        }

        let aktif_c = self.aktif.clone();
        let olaylar_c = self.olaylar.clone();
        let karar_c = self.karar.clone();

        std::thread::Builder::new()
            .name("sira-eslestirme-srv".to_string())
            .spawn(move || {
                let basla = Instant::now();
                while aktif_c.load(Ordering::SeqCst)
                    && basla.elapsed() < Duration::from_secs(PENCERE_SN)
                {
                    match listener.accept() {
                        Ok((mut stream, addr)) => {
                            let peer_ip = addr.ip().to_string();
                            let Ok(cloned) = stream.try_clone() else {
                                continue;
                            };
                            let mut reader = BufReader::new(cloned);
                            let mut line = String::new();
                            if reader.read_line(&mut line).is_ok() && line.len() <= ILETI_EN_BUYUK
                            {
                                if let Ok(req) = serde_json::from_str::<Value>(line.trim()) {
                                    if req.get("tur").and_then(|v| v.as_str()) == Some("eslestir") {
                                        let ad = req
                                            .get("ad")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let Some(der) = kok_der() else {
                                            olaylar_c.lock().unwrap().push_back(
                                                SunucuOlay::Uyari {
                                                    mesaj: "Kök sertifika bulunamadı.".to_string(),
                                                },
                                            );
                                            continue;
                                        };
                                        let kod = dogrulama_kodu(&der, peer_ip.as_bytes());
                                        {
                                            let mut g = karar_c.0.lock().unwrap();
                                            *g = None;
                                        }
                                        olaylar_c.lock().unwrap().push_back(SunucuOlay::Istek {
                                            ip: peer_ip.clone(),
                                            ad: ad.clone(),
                                            kod: kod.clone(),
                                        });

                                        // Bot'un kararını oku (opsiyonel) ve ofis kararını bekle
                                        let onaylandi = {
                                            let (lock, cvar) = &*karar_c;
                                            let mut guard = lock.lock().unwrap();
                                            let deadline =
                                                Instant::now() + Duration::from_secs(KARAR_SN);
                                            while guard.is_none()
                                                && aktif_c.load(Ordering::SeqCst)
                                            {
                                                let rem = deadline
                                                    .saturating_duration_since(Instant::now());
                                                if rem.is_zero() {
                                                    break;
                                                }
                                                let (g, _) = cvar.wait_timeout(guard, rem).unwrap();
                                                guard = g;
                                            }
                                            guard.unwrap_or(false)
                                        };

                                        if onaylandi {
                                            let _ = bot_ekle(&peer_ip, Some(&ad));
                                            let resp = json!({
                                                "ok": true,
                                                "kod": kod,
                                                "kok_b64": base64::engine::general_purpose::STANDARD.encode(&der),
                                            });
                                            let _ = writeln!(stream, "{}", resp);
                                            olaylar_c.lock().unwrap().push_back(
                                                SunucuOlay::Bitti {
                                                    basarili: true,
                                                    mesaj: format!("{} eklendi.", peer_ip),
                                                },
                                            );
                                        } else {
                                            let resp = json!({"ok": false});
                                            let _ = writeln!(stream, "{}", resp);
                                            olaylar_c.lock().unwrap().push_back(
                                                SunucuOlay::Bitti {
                                                    basarili: false,
                                                    mesaj: "Eşleştirme reddedildi.".to_string(),
                                                },
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        Err(_) => break,
                    }
                }
                aktif_c.store(false, Ordering::SeqCst);
            })
            .map_err(|e| EslestirmeHatasi::Mesaj(e.to_string()))?;

        Ok(())
    }

    pub fn durdur(&self) {
        self.aktif.store(false, Ordering::SeqCst);
        let (lock, cvar) = &*self.karar;
        if let Ok(mut g) = lock.lock() {
            if g.is_none() {
                *g = Some(false);
            }
        }
        cvar.notify_all();
    }

    pub fn karar_ver(&self, evet: bool) {
        let (lock, cvar) = &*self.karar;
        if let Ok(mut g) = lock.lock() {
            *g = Some(evet);
        }
        cvar.notify_all();
    }

    pub fn durdu_mu(&self) -> bool {
        !self.aktif.load(Ordering::SeqCst)
    }

    pub fn kalan_sn(&self) -> u64 {
        let elapsed = self
            .baslangic
            .lock()
            .ok()
            .and_then(|g| g.map(|t| t.elapsed().as_secs()))
            .unwrap_or(0);
        PENCERE_SN.saturating_sub(elapsed)
    }

    pub fn olay_al(&self) -> Option<SunucuOlay> {
        self.olaylar.lock().ok()?.pop_front()
    }
}

pub struct EslestirmeIstemcisi {
    pub kapi_ip: String,
    pub cer: Option<Vec<u8>>,
    stream: Option<TcpStream>,
}

impl EslestirmeIstemcisi {
    pub fn new(kapi_ip: &str) -> Result<Self, EslestirmeHatasi> {
        let temiz = _lan_ipv4(kapi_ip, "Kapı IP'si").map_err(EslestirmeHatasi::Mesaj)?;
        Ok(Self {
            kapi_ip: temiz,
            cer: None,
            stream: None,
        })
    }

    pub fn baglan(&mut self) -> Result<String, EslestirmeHatasi> {
        let addr: SocketAddr = format!("{}:{}", self.kapi_ip, ESLESTIRME_PORT)
            .parse()
            .map_err(|_| EslestirmeHatasi::Mesaj("Geçersiz adres".to_string()))?;
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .map_err(|e| EslestirmeHatasi::Mesaj(format!("Ofise bağlanılamadı: {}", e)))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(KARAR_SN + 10)));
        let pc_ad = std::env::var("COMPUTERNAME").unwrap_or_default();
        let req = json!({"tur": "eslestir", "ad": pc_ad});
        writeln!(stream, "{}", req).map_err(|e| EslestirmeHatasi::Mesaj(e.to_string()))?;

        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|e| EslestirmeHatasi::Mesaj(e.to_string()))?,
        );
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| EslestirmeHatasi::Mesaj(e.to_string()))?;
        let resp: Value = serde_json::from_str(line.trim())
            .map_err(|e| EslestirmeHatasi::Mesaj(format!("Yanıt hatası: {}", e)))?;
        if !resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Err(EslestirmeHatasi::Mesaj(
                "Eşleştirme ofis tarafından reddedildi.".to_string(),
            ));
        }
        let kod = resp
            .get("kod")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let b64 = resp
            .get("kok_b64")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EslestirmeHatasi::Mesaj("Kök sertifika alınamadı.".to_string()))?;
        let der = base64::engine::general_purpose::STANDARD
            .decode(b64.as_bytes())
            .map_err(|e| EslestirmeHatasi::Mesaj(format!("Sertifika çözümleme hatası: {}", e)))?;
        kok_dogrula(&der).map_err(EslestirmeHatasi::Mesaj)?;
        self.cer = Some(der);
        self.stream = Some(stream);
        Ok(kod)
    }

    pub fn karar_bildir_ve_bekle(&mut self, evet: bool) -> Result<bool, EslestirmeHatasi> {
        if !evet {
            self.kapat();
            return Ok(false);
        }
        Ok(self.cer.is_some())
    }

    pub fn kapat(&mut self) {
        self.stream.take();
    }
}
