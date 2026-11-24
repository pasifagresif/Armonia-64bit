//! Çoklu UYAP güncellemeleri (Ayarlar → Çoklu UYAP). Plan: docs/claude-notlar/planlar/cokluuyap-linux-guncelleme.md.
//! Claude, 2026-11-24.
//!
//! Ayarlar hiçbir şeyi kendisi indirmez ya da kurmaz ve root yetkisi kullanmaz:
//! - durum: `armonia-guncelle durum` (oturum kullanıcısıyla) imzalı bilgiyi YENİDEN doğrular; ağa çıkan
//!   denetleyicinin yazdığı durum dosyasına güvenilmez (denetçi 24.11 O6/D-5).
//! - işlemler: `systemctl start <birim>`; izin polkit kuralında (61-armonia-guncelle.rules): denetle ve imaja-dön
//!   parolasız, indir (+ ardından ağsız kurulum) kullanıcı parolasıyla.

use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const ARAC: &str = "/opt/armonia/guncelle/armonia-guncelle";
const DENETLE: &str = "armonia-guncelle-denetle.service";
const INDIR: &str = "armonia-guncelle-indir.service";
const KUR: &str = "armonia-guncelle-kur.service";
const IMAJA_DON: &str = "armonia-guncelle-imaja-don.service";
const COKLUUYAP: &str = "armonia-cokluuyap.service";
const SON_KURULUM: &str = "/var/lib/armonia-guncelle/durum/son_kurulum";

#[derive(Deserialize, Default, Clone, Debug)]
pub struct Durum {
    pub katman: bool,
    pub canli: bool,
    pub imaj: Option<String>,
    pub etkin: Option<String>,
    /// Kurulu güncelleme açılışta kullanılmayacak: nedeni
    #[serde(default)]
    pub etkin_red: Option<String>,
    pub imaja_don: bool,
    pub teklif: Option<Teklif>,
    pub teklif_hata: Option<String>,
    pub son_denetim: Option<String>,
    pub son_denetim_hata: Option<String>,
    pub son_kurulum: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Teklif {
    pub surum: String,
    pub notlar: String,
    pub yayin: String,
    #[allow(dead_code)]
    pub gecerlilik_sonu: String,
    pub kurulabilir: bool,
}

fn calistir(program: &str, argumanlar: &[&str]) -> Result<String, String> {
    let c = Command::new(program)
        .args(argumanlar)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{program} çalıştırılamadı: {e}"))?;
    if c.status.success() {
        Ok(String::from_utf8_lossy(&c.stdout).into_owned())
    } else {
        let hata = String::from_utf8_lossy(&c.stderr).trim().to_string();
        Err(if hata.is_empty() { format!("{program} başarısız ({})", c.status) } else { hata })
    }
}

pub fn durum_coz(json: &str) -> Result<Durum, String> {
    serde_json::from_str(json.trim()).map_err(|e| format!("güncelleme durumu okunamadı: {e}"))
}

/// Katman yoksa (ISO, anahtarsız derleme) boş durum: "yalnız imajla güncellenir".
pub fn durum() -> Result<Durum, String> {
    if !Path::new(ARAC).exists() {
        return Ok(Durum { canli: crate::sistem::canli(), ..Default::default() });
    }
    durum_coz(&calistir(ARAC, &["durum"])?)
}

fn systemctl_baslat(birim: &str) -> Result<(), String> {
    calistir("systemctl", &["start", birim]).map(|_| ())
}

fn birim_ozelligi(birim: &str, ozellik: &str) -> String {
    calistir("systemctl", &["show", "-p", ozellik, "--value", birim]).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn denetle() -> Result<(), String> {
    systemctl_baslat(DENETLE)
}

/// İndirir (parola sorulur) ve indiricinin ardından kendiliğinden başlayan ağsız kurulumun bitmesini bekler.
/// Kurulan sürümü döner. Başarı, kurucunun yazdığı `son_kurulum` zamanının değişmesiyle anlaşılır: başarıyla
/// biten birimi systemd bellekten atabilir ve InvocationID boş döner (denetçi 24.11 O-1). Başarısız birim
/// atılmaz; yeni InvocationID + failed hatadır.
pub fn indir_ve_kur() -> Result<String, String> {
    // Kurucunun sonuç kaydı (herkes okur; yalnız kurucu yazar). Döngüde tam durum (ağaç özeti) alınmaz.
    let son_kurulum = || std::fs::read_to_string(SON_KURULUM).ok();
    let onceki_kimlik = birim_ozelligi(KUR, "InvocationID");
    let onceki_kurulum = son_kurulum();
    systemctl_baslat(INDIR).map_err(|e| format!("indirilemedi: {e}"))?;
    let bas = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(700));
        if let Some(k) = son_kurulum().filter(|k| Some(k) != onceki_kurulum.as_ref()) {
            return Ok(k.lines().find_map(|l| l.strip_prefix("surum=")).unwrap_or_default().chars().filter(|c| c.is_ascii_digit() || *c == '.').take(20).collect());
        }
        let kimlik = birim_ozelligi(KUR, "InvocationID");
        if !kimlik.is_empty() && kimlik != onceki_kimlik && birim_ozelligi(KUR, "ActiveState") == "failed" {
            return Err(format!(
                "kurulum başarısız ({}). Ayrıntı: journalctl -u {KUR}. Çoklu UYAP önceki sürümüyle çalışmaya devam eder.",
                birim_ozelligi(KUR, "Result")
            ));
        }
        if bas.elapsed() > Duration::from_secs(15 * 60) {
            return Err(format!("kurulum 15 dakikada bitmedi. Ayrıntı: journalctl -u {KUR}"));
        }
    }
}

pub fn imaja_don() -> Result<(), String> {
    systemctl_baslat(IMAJA_DON)
}

pub fn cokluuyap_calisiyor() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", COKLUUYAP])
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Çoklu UYAP'ı kapatıp yeniden açar (polkit: armonia bu birimi durdurup başlatabilir, 60-armonia-cokluuyap.rules).
pub fn cokluuyap_yeniden_baslat() -> Result<(), String> {
    calistir("systemctl", &["stop", COKLUUYAP])?;
    Command::new("/opt/armonia/bin/cokluuyap_ac.sh")
        .stdin(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Çoklu UYAP açılamadı: {e}"))
}

/// "2026-11-24T12:00:00Z" → "24.11.2026 15:00" (Türkiye saati, UTC+3; 2016'dan beri yaz saati yok).
pub fn yerel_zaman(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() != 20 || b[10] != b'T' || b[19] != b'Z' {
        return s.to_string();
    }
    let n = |a: usize, c: usize| s.get(a..a + c).and_then(|t| t.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi)) = (n(0, 4), n(5, 2), n(8, 2), n(11, 2), n(14, 2)) else {
        return s.to_string();
    };
    // Gün taşması: ay sonu ve yıl sonu
    let ay_gun = |y: i64, m: i64| match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    let (mut y, mut mo, mut d, mut h) = (y, mo, d, h + 3);
    if h >= 24 {
        h -= 24;
        d += 1;
        if d > ay_gun(y, mo) {
            d = 1;
            mo += 1;
            if mo > 12 {
                mo = 1;
                y += 1;
            }
        }
    }
    format!("{d:02}.{mo:02}.{y} {h:02}:{mi:02}")
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn durum_cozumleme() {
        let d = durum_coz(
            r#"{"katman":true,"canli":false,"imaj":"26.11.13","etkin":null,"imaja_don":false,
            "teklif":{"surum":"26.12.1","notlar":"Düzeltmeler","yayin":"2026-12-01T10:00:00Z",
            "gecerlilik_sonu":"2027-01-01T00:00:00Z","kurulabilir":true},"teklif_hata":null,
            "son_denetim":"2026-12-01T11:00:00Z","son_denetim_hata":null,"son_kurulum":null}"#,
        )
        .unwrap();
        assert!(d.katman && d.teklif.as_ref().unwrap().kurulabilir);
        assert!(durum_coz("bozuk").is_err());
    }

    #[test]
    fn zaman() {
        assert_eq!(yerel_zaman("2026-11-24T12:05:00Z"), "24.11.2026 15:05");
        assert_eq!(yerel_zaman("2026-12-31T22:30:00Z"), "01.01.2027 01:30");
        assert_eq!(yerel_zaman("2028-02-28T23:00:00Z"), "29.02.2028 02:00");
        assert_eq!(yerel_zaman("bozuk"), "bozuk");
    }
}
