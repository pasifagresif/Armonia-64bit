//! Armonia imza hizmeti. Editör (ve izinli uygulamalar) yerel Unix soketinden UDF gönderir; hizmet karta erişen
//! tek süreçtir (Armonia'da pcscd yalnız e-imza kimliklerine açık). PIN istekle gelir, yalnız kart girişinde
//! kullanılır, saklanmaz. Her imza isteği zincirli günlüğe yazılır.
//!
//! Çalıştırma: systemd soket etkinleştirmesi (LISTEN_FDS) ya da ARMONIA_IMZA_SOKET=<yol>.
//! Günlük: ARMONIA_IMZA_GUNLUK (varsayılan /var/lib/armonia-imza/gunluk.jsonl).
//! Yönetici: `armonia-imza gunluk-dogrula [yol]`. Claude, 2026-11-23.

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let gunluk_yolu = std::env::var("ARMONIA_IMZA_GUNLUK").unwrap_or_else(|_| "/var/lib/armonia-imza/gunluk.jsonl".into());
    if a.get(1).map(String::as_str) == Some("gunluk-dogrula") {
        let yol = a.get(2).cloned().unwrap_or(gunluk_yolu);
        match imza::gunluk::Gunluk::yeni(&yol).dogrula() {
            Ok((n, son)) => println!("günlük sağlam: {n} kayıt, son özet {son}"),
            Err(e) => {
                eprintln!("GÜNLÜK BOZUK: {e}");
                std::process::exit(2);
            }
        }
        return;
    }
    #[cfg(unix)]
    unix::calis(&gunluk_yolu);
    #[cfg(not(unix))]
    {
        eprintln!("armonia-imza hizmeti yalnız Linux'ta çalışır");
        std::process::exit(1);
    }
}

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::io::{AsRawFd, FromRawFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::time::{Duration, Instant};

    use imza::hizmet::{isle, Cagiran, Kart, AZAMI_ISTEK};
    use imza::udf_imza::{ImzaHatasi, Imzalayici};
    use serde_json::{json, Value};

    /// Arka uç: ARMONIA_IMZA_ARKA=adalet (varsayılan; Adalet E-imza istemcisi, 127.0.0.1:5975) ya da pkcs11
    /// (kart sürücüsü, ARMONIA_PKCS11). Armonia'da karta yalnız Adalet istemcisi erişir.
    enum GercekKart {
        Adalet(imza::adalet::Adalet),
        Pkcs11,
    }

    impl Kart for GercekKart {
        fn durum(&self) -> Value {
            let s = match self {
                GercekKart::Adalet(a) => a.sertifikalar().map(|v| json!(v)),
                GercekKart::Pkcs11 => imza::kart::sertifikalar().map(|v| json!(v)),
            };
            match s {
                Ok(v) => json!({ "takili": true, "sertifikalar": v }),
                Err(e) => json!({ "takili": false, "neden": e.to_string() }),
            }
        }
        fn udf_imzala(&self, udf: &[u8], pin: &str, sertifika: Option<&str>) -> Result<Vec<u8>, ImzaHatasi> {
            match self {
                GercekKart::Adalet(a) => a.udf_imzala(udf, pin, sertifika),
                GercekKart::Pkcs11 => {
                    let mut k: Box<dyn Imzalayici> = Box::new(imza::kart::KartImzalayici::ac(pin, sertifika)?);
                    imza::hizmet::imzalayici_ile(udf, k.as_mut())
                }
            }
        }
    }

    /// Soketin karşı ucundaki sürecin kimliği (çekirdekten, SO_PEERCRED).
    fn cagiran(s: &UnixStream) -> Cagiran {
        let mut c = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut uz = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut c as *mut _ as *mut libc::c_void, &mut uz) };
        if r != 0 {
            return Cagiran::default();
        }
        // Programın yolu (/proc/<pid>/exe) başka kullanıcının sürecinde okunamaz (ptrace izni); systemd birimi
        // (cgroup) okunabilir ve daha anlamlıdır: armonia-yazi@….service ya da session-*.scope (denetçi 24.11 O5)
        let program = std::fs::read_to_string(format!("/proc/{}/cgroup", c.pid))
            .ok()
            .and_then(|m| m.lines().find_map(|l| l.strip_prefix("0::").map(|y| y.trim().chars().take(200).collect::<String>())));
        Cagiran { uid: Some(c.uid), pid: Some(c.pid as u32), program }
    }

    /// İsteğin TAMAMI için süre sınırı (her okuma için ayrı değil): yavaş gönderen bir süreç hizmeti meşgul
    /// edemesin (denetçi 24.11 D3).
    struct Sureli<'a> {
        s: &'a UnixStream,
        son: Instant,
    }

    impl Read for Sureli<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let kalan = self.son.saturating_duration_since(Instant::now());
            if kalan.is_zero() {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "istek süresi doldu"));
            }
            self.s.set_read_timeout(Some(kalan))?;
            let mut r: &UnixStream = self.s;
            r.read(buf)
        }
    }

    fn dinleyici() -> UnixListener {
        // systemd soket etkinleştirmesi: dinleyen soket fd 3
        if std::env::var("LISTEN_FDS").ok().as_deref() == Some("1")
            && std::env::var("LISTEN_PID").ok().and_then(|p| p.parse::<u32>().ok()) == Some(std::process::id())
        {
            return unsafe { UnixListener::from_raw_fd(3) };
        }
        let yol = std::env::var("ARMONIA_IMZA_SOKET").unwrap_or_else(|_| {
            eprintln!("LISTEN_FDS ya da ARMONIA_IMZA_SOKET gerekli");
            std::process::exit(1)
        });
        let _ = std::fs::remove_file(&yol);
        UnixListener::bind(&yol).unwrap_or_else(|e| {
            eprintln!("soket açılamadı ({yol}): {e}");
            std::process::exit(1)
        })
    }

    pub fn calis(gunluk_yolu: &str) {
        let gunluk = imza::gunluk::Gunluk::yeni(gunluk_yolu);
        if let Err(e) = gunluk.dogrula() {
            // Bozuk günlüğe yeni kayıt eklenmez; yöneticinin bakması gerekir
            eprintln!("İMZA GÜNLÜĞÜ BOZUK, hizmet başlamıyor: {e}");
            std::process::exit(2);
        }
        let kart = match std::env::var("ARMONIA_IMZA_ARKA").as_deref() {
            Ok("pkcs11") => GercekKart::Pkcs11,
            _ => GercekKart::Adalet(imza::adalet::Adalet {
                adres: std::env::var("ARMONIA_EIMZA_ADRES").unwrap_or_else(|_| imza::adalet::VARSAYILAN_ADRES.into()),
            }),
        };
        // Art arda yanlış PIN sayısı (doğru PIN ya da başarılı imza sıfırlar)
        let mut yanlis: u32 = 0;
        // İstekler sırayla: kart aynı anda tek işlem yapar
        for baglanti in dinleyici().incoming() {
            let Ok(mut s) = baglanti else { continue };
            let _ = s.set_write_timeout(Some(Duration::from_secs(30)));
            let c = cagiran(&s);
            let mut satir = Vec::new();
            let okuyucu = Sureli { s: &s, son: Instant::now() + Duration::from_secs(30) };
            let okundu = BufReader::new(okuyucu.take(AZAMI_ISTEK as u64 + 1)).read_until(b'\n', &mut satir);
            let yanit = match okundu {
                Ok(_) if satir.len() > AZAMI_ISTEK => json!({ "tamam": false, "hata": "istek çok büyük" }),
                Ok(_) => isle(&satir, &c, &kart, &gunluk),
                Err(e) => json!({ "tamam": false, "hata": format!("istek okunamadı: {e}") }),
            };
            let pin_hatali = yanit.get("tur").and_then(Value::as_str) == Some("pin_hatali");
            if yanit.get("tamam").and_then(Value::as_bool) == Some(true) && yanit.get("udf").is_some() {
                yanlis = 0;
            }
            let mut cikti = yanit.to_string();
            cikti.push('\n');
            let _ = s.write_all(cikti.as_bytes());
            drop(s);
            if pin_hatali {
                // Kaba kuvvete karşı artan yavaşlatma: 2, 4, 8, 16, 30 sn (kart da birkaç yanlıştan sonra kilitlenir)
                yanlis = yanlis.saturating_add(1);
                std::thread::sleep(Duration::from_secs((2u64 << (yanlis - 1).min(4)).min(30)));
            }
        }
    }
}
