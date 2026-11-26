//! Yerel ağ (LAN) doğrudan erişimi için TLS — kendinden imzalı sertifika + parmak izi sabitleme.
//!
//! `uyap_core/lan_tls.py` dosyasının birebir (1:1) Rust karşılığı.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType, PKCS_ECDSA_P256_SHA256};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use x509_parser::prelude::*;

pub static LAN_TLS_PORT: Lazy<u16> = Lazy::new(|| {
    std::env::var("UYAP_LAN_TLS_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|&p| p > 0)
        .unwrap_or(8843)
});

const _AYIRAC: char = '.';
const _YENILEME_PAYI_SECS: i64 = 30 * 24 * 3600;

pub fn _dizin() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var("LOCALAPPDATA")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("USERPROFILE").ok().filter(|s| !s.is_empty()))
            .unwrap_or_else(|| ".".to_string());
        Path::new(&base).join("UyapIcra").join("lan_tls")
    } else {
        let base = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                Path::new(&home).join(".config").to_string_lossy().to_string()
            });
        Path::new(&base).join("uyapicra").join("lan_tls")
    }
}

pub fn parmak_izi(der: &[u8]) -> String {
    hex::encode(Sha256::digest(der))
}

pub fn _sertifika_uret(ips: &[String]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
        .map_err(|e| format!("EC P-256 anahtar üretilemedi: {e}"))?;

    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Coklu UYAP LAN");
    params.distinguished_name = dn;

    let mut sans = vec![
        SanType::DnsName("localhost".try_into().map_err(|e| format!("SAN DNS hatası: {e}"))?),
        SanType::IpAddress("127.0.0.1".parse::<IpAddr>().unwrap()),
    ];
    for ip in ips {
        if let Ok(parsed) = ip.parse::<IpAddr>() {
            sans.push(SanType::IpAddress(parsed));
        }
    }
    params.subject_alt_names = sans;

    let now = SystemTime::now();
    params.not_before = (now - Duration::from_secs(5 * 60)).into();
    params.not_after = (now + Duration::from_secs(3 * 365 * 24 * 3600)).into();

    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| format!("Sertifika imzalanamadı: {e}"))?;

    let cert_pem = cert.pem().into_bytes();
    let key_pem = key_pair.serialize_pem().into_bytes();
    Ok((cert_pem, key_pem))
}

pub fn _gecerli_mi(sert_pem: &[u8]) -> bool {
    let mut reader = std::io::BufReader::new(sert_pem);
    let Ok(certs) = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>() else {
        return false;
    };
    let Some(first) = certs.first() else {
        return false;
    };
    let Ok((_, parsed)) = X509Certificate::from_der(first.as_ref()) else {
        return false;
    };
    let bitis_ts = parsed.validity().not_after.timestamp();
    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (bitis_ts - now_ts) > _YENILEME_PAYI_SECS
}

/// (sertifika_yolu, anahtar_yolu, parmak_izi) döndürür; yoksa/süresi dolmak üzereyse üretir.
pub fn sertifika_hazirla(ips: &[String]) -> Result<(PathBuf, PathBuf, String), String> {
    let d = _dizin();
    std::fs::create_dir_all(&d).map_err(|e| format!("Klasör oluşturulamadı: {e}"))?;
    let sert_yol = d.join("lan_cert.pem");
    let anahtar_yol = d.join("lan_key.pem");

    let mut sert_pem = Vec::new();
    if sert_yol.exists() && anahtar_yol.exists() {
        if let Ok(bytes) = std::fs::read(&sert_yol) {
            sert_pem = bytes;
        }
    }

    if sert_pem.is_empty() || !_gecerli_mi(&sert_pem) {
        let (new_cert, new_key) = _sertifika_uret(ips)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&anahtar_yol)
                .map_err(|e| format!("Anahtar yazılamadı: {e}"))?;
            f.write_all(&new_key)
                .map_err(|e| format!("Anahtar yazılamadı: {e}"))?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&anahtar_yol, &new_key)
                .map_err(|e| format!("Anahtar yazılamadı: {e}"))?;
        }
        std::fs::write(&sert_yol, &new_cert)
            .map_err(|e| format!("Sertifika yazılamadı: {e}"))?;
        sert_pem = new_cert;
    }

    let mut reader = std::io::BufReader::new(&sert_pem[..]);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("PEM sertifika okunamadı: {e}"))?;
    let first = certs
        .first()
        .ok_or_else(|| "PEM dosyasında sertifika yok".to_string())?;
    let iz = parmak_izi(first.as_ref());
    Ok((sert_yol, anahtar_yol, iz))
}

// ── Windows güvenlik duvarı (26.11.2026) ─────────────────────────────────────────────────────
// Yerel ağ kapısı 0.0.0.0:8843'te dinler; Windows gelen bağlantıyı kuralsız engeller ("Var olan oturumu al" diğer
// bilgisayarlardan çalışmaz). MSI perUser (UAC'siz) olduğundan kuralı kurulum ekleyemez: ofis sunucusu yerel ağ
// erişimiyle paylaşımı başlatırken kural yoksa program kendini bir kez yönetici olarak çalıştırıp ekler
// (`--sira-yonetici lan-guvenlik-duvari`, sira_eslestirme::yonetici_main). Yalnız özel ağ profili. Ad ASCII:
// netsh adı konsol kod sayfasıyla geçirir.

pub const GUVENLIK_DUVARI_KURALI: &str = "Coklu UYAP yerel ag kapisi (TCP 8843)";

#[cfg(windows)]
fn netsh(args: &[&str]) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("netsh").args(args).creation_flags(0x0800_0000).output()   // CREATE_NO_WINDOW
}

/// Kural var mı (Windows dışında her zaman true: bu güvenlik duvarı yok).
pub fn guvenlik_duvari_var() -> bool {
    #[cfg(windows)]
    {
        netsh(&["advfirewall", "firewall", "show", "rule", &format!("name={GUVENLIK_DUVARI_KURALI}")])
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Kuralı (yeniden) ekler. Yönetici yetkisi gerekir.
pub fn guvenlik_duvari_kur() -> Result<(), String> {
    #[cfg(windows)]
    {
        let ad = format!("name={GUVENLIK_DUVARI_KURALI}");
        let _ = netsh(&["advfirewall", "firewall", "delete", "rule", &ad]);
        let port = format!("localport={}", *LAN_TLS_PORT);
        let o = netsh(&["advfirewall", "firewall", "add", "rule", &ad, "dir=in", "action=allow", "protocol=TCP", &port, "profile=private"])
            .map_err(|e| format!("netsh çalıştırılamadı: {e}"))?;
        if !o.status.success() {
            return Err(format!("Güvenlik duvarı kuralı eklenemedi: {}", String::from_utf8_lossy(&o.stdout)));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

// ── Bilet + parmak izi ────────────────────────────────────────────────────────────────

pub fn bilet_birlestir(bilet: &str, izi: &str) -> String {
    format!("{bilet}{_AYIRAC}{izi}")
}

/// (bilet, parmak_izi | None). Parmak izi yoksa ofis eski sürümdür (düz HTTP LAN).
pub fn bilet_ayir(lan_token: &str) -> (String, Option<String>) {
    let tok = lan_token.trim();
    if let Some((bilet, izi)) = tok.rsplit_once(_AYIRAC) {
        if izi.len() == 64 && izi.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
            return (bilet.to_string(), Some(izi.to_string()));
        }
    }
    (tok.to_string(), None)
}

// ── İstemci tarafı: sertifikayı çek, parmak iziyle doğrula, sabitlenmiş bağlam üret ──────

#[derive(Debug)]
struct ProbeAnyVerifier;

impl ServerCertVerifier for ProbeAnyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        CryptoProvider::get_default()
            .map(|p| p.signature_verification_algorithms.supported_schemes())
            .unwrap_or_else(|| {
                vec![
                    SignatureScheme::ECDSA_NISTP256_SHA256,
                    SignatureScheme::ECDSA_NISTP384_SHA384,
                    SignatureScheme::RSA_PSS_SHA256,
                    SignatureScheme::RSA_PKCS1_SHA256,
                ]
            })
    }
}

#[derive(Debug)]
struct PinnedCertVerifier {
    pinned_der: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if end_entity.as_ref() == self.pinned_der.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General(
                "LAN TLS sertifikası sabitlenmiş parmak iziyle eşleşmiyor".to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn ensure_rustls_provider() -> Arc<CryptoProvider> {
    if let Some(p) = CryptoProvider::get_default() {
        return p.clone();
    }
    let provider = rustls::crypto::ring::default_provider();
    let _ = provider.install_default();
    CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::ring::default_provider()))
}

/// Ofisin sunduğu sertifikanın parmak izi beklenenle aynıysa YALNIZ o sertifikaya güvenen
/// bir `rustls::ClientConfig` döndürür; değilse / ulaşılamazsa `None`. Bu aşamada hiçbir sır gönderilmez.
pub async fn sabitli_baglam(
    ip: &str,
    port: u16,
    beklenen_izi: &str,
    timeout_secs: f64,
) -> Option<Arc<ClientConfig>> {
    let provider = ensure_rustls_provider();
    let probe_cfg = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .ok()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(ProbeAnyVerifier))
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(probe_cfg));
    let addr: SocketAddr = format!("{ip}:{port}").parse().ok()?;
    let timeout = Duration::from_secs_f64(timeout_secs.max(0.1));

    let tcp = tokio::time::timeout(timeout, TcpStream::connect(addr))
        .await
        .ok()?
        .ok()?;

    let server_name = ServerName::try_from(ip.to_string())
        .unwrap_or_else(|_| ServerName::try_from("localhost".to_string()).unwrap());

    let tls_stream = tokio::time::timeout(timeout, connector.connect(server_name, tcp))
        .await
        .ok()?
        .ok()?;

    let (_, conn) = tls_stream.get_ref();
    let der = conn.peer_certificates()?.first()?.as_ref().to_vec();
    drop(tls_stream);

    if der.is_empty() || parmak_izi(&der) != beklenen_izi {
        return None;
    }

    let pinned_verifier = PinnedCertVerifier {
        pinned_der: der,
        provider: provider.clone(),
    };
    let pinned_cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .ok()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(pinned_verifier))
        .with_no_client_auth();

    Some(Arc::new(pinned_cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bilet_ayir() {
        let fp = "a".repeat(64);
        let combined = bilet_birlestir("secret_ticket", &fp);
        let (b, iz) = bilet_ayir(&combined);
        assert_eq!(b, "secret_ticket");
        assert_eq!(iz.as_deref(), Some(fp.as_str()));

        let (b2, iz2) = bilet_ayir("old_ticket_only");
        assert_eq!(b2, "old_ticket_only");
        assert_eq!(iz2, None);
    }

    #[test]
    fn test_sertifika_uret_ve_gecerli_mi() {
        let (cert_pem, _key_pem) = _sertifika_uret(&["192.168.1.10".to_string()]).unwrap();
        assert!(_gecerli_mi(&cert_pem));
    }
}
