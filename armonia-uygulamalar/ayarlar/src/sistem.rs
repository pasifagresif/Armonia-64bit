//! Sistemle konuşan kısım: ayar dosyası, root yardımcısı (pkexec), CUPS, sistem bilgisi.
//! Arayüzden ayrı; ayrıştırıcılar birim testli. Claude, 2026-11-19.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

pub const AYAR_DOSYASI: &str = "/etc/armonia/ayarlar.conf";
pub const YARDIMCI: &str = "/opt/armonia/bin/ayar-yardimci";

/// Canlı (USB'den deneme) sistem mi? `/etc/armonia/deneme`: deneme imajı (ör. Raspberry Pi derlemesi), parolasız.
pub fn canli() -> bool {
    Path::new("/run/live/medium").is_dir() || Path::new("/etc/armonia/deneme").exists()
}

/// "ANAHTAR=deger" satırları (yorum ve bozuk satırlar atlanır).
pub fn ayar_coz(metin: &str) -> BTreeMap<String, String> {
    metin
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()))
        .map(|(k, v)| (k.to_string(), v.trim().to_string()))
        .collect()
}

pub fn ayarlar() -> BTreeMap<String, String> {
    std::fs::read_to_string(AYAR_DOSYASI).map(|m| ayar_coz(&m)).unwrap_or_default()
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

/// Root yardımcısı: pkexec /opt/armonia/bin/ayar-yardimci <komut> <deger>
pub fn yardimci(komut: &str, deger: &str) -> Result<(), String> {
    calistir("pkexec", &[YARDIMCI, komut, deger]).map(|_| ())
}

/// labwc ayarlarını (klavye düzeni dahil) yeniden okur.
pub fn labwc_yenile() {
    let _ = calistir("labwc", &["--reconfigure"]);
}

/// Üst çubuktaki klavye hapını yeniler (waybar `signal: 8`).
pub fn ust_cubuk_yenile() {
    let _ = calistir("pkill", &["-RTMIN+8", "-x", "waybar"]);
}

/// Boşta / kilit zamanlayıcısını yeni süreyle yeniden başlatır.
pub fn bosta_yenile() {
    let _ = calistir("pkill", &["-x", "swayidle"]);
    let _ = Command::new("setsid")
        .args(["-f", "/opt/armonia/bin/bosta.sh"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Yazici {
    pub ad: String,
    pub adres: String,
    pub durum: String,
    pub varsayilan: bool,
}

/// `lpstat -v`, `lpstat -p`, `lpstat -d` çıktılarından yazıcı listesi (LANG=C ile çalıştırılır).
pub fn yazicilari_coz(v: &str, p: &str, d: &str) -> Vec<Yazici> {
    let varsayilan = d.lines().find_map(|l| l.strip_prefix("system default destination: ")).map(str::trim);
    v.lines()
        .filter_map(|l| l.strip_prefix("device for "))
        .filter_map(|l| l.split_once(": "))
        .map(|(ad, adres)| {
            let durum = p
                .lines()
                .find(|l| l.starts_with(&format!("printer {ad} ")))
                .map(|l| {
                    if l.contains("disabled") {
                        "Kapalı"
                    } else if l.contains("now printing") {
                        "Yazdırıyor"
                    } else {
                        "Hazır"
                    }
                })
                .unwrap_or("Bilinmiyor")
                .to_string();
            Yazici { ad: ad.to_string(), adres: adres.trim().to_string(), durum, varsayilan: varsayilan == Some(ad) }
        })
        .collect()
}

pub fn yazicilar() -> Result<Vec<Yazici>, String> {
    let lp = |a: &str| {
        Command::new("lpstat")
            .arg(a)
            .env("LANG", "C")
            .stdin(Stdio::null())
            .output()
            .map(|c| String::from_utf8_lossy(&c.stdout).into_owned())
            .map_err(|e| format!("Yazdırma sistemi (CUPS) bulunamadı: {e}"))
    };
    Ok(yazicilari_coz(&lp("-v")?, &lp("-p")?, &lp("-d")?))
}

pub fn deneme_sayfasi(ad: &str) -> Result<(), String> {
    calistir("lp", &["-d", ad, "-t", "Armonia deneme sayfası", "/usr/share/cups/data/testprint"]).map(|_| ())
}

/// Adres metnini kullanıcıya anlaşılır yazar.
pub fn baglanti_turu(adres: &str) -> &'static str {
    if adres.starts_with("ipp://localhost") {
        "USB (sürücüsüz)"
    } else if adres.starts_with("usb:") || adres.starts_with("hp:") {
        "USB (sürücülü)"
    } else {
        "Diğer"
    }
}

#[derive(Clone, Debug, Default)]
pub struct Bilgi {
    pub sistem: String,
    pub cekirdek: String,
    pub bellek: String,
    pub disk: String,
    pub islemci: String,
}

/// /proc/meminfo "MemTotal:  8000000 kB" → "7,6 GB"
pub fn bellek_coz(meminfo: &str) -> Option<String> {
    let kb: f64 = meminfo.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.split_whitespace().next()?.parse().ok()?;
    Some(format!("{:.1} GB", kb / 1024.0 / 1024.0).replace('.', ","))
}

pub fn bilgi() -> Bilgi {
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let ad = ayar_coz(&os.replace('"', "")).get("PRETTY_NAME").cloned().unwrap_or_else(|| "bilinmiyor".into());
    let islemci = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("model name") || l.starts_with("Model"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_else(|| std::env::consts::ARCH.to_string());
    let disk = calistir("df", &["-h", "--output=size,avail", "/"])
        .ok()
        .and_then(|m| m.lines().nth(1).map(|l| l.split_whitespace().collect::<Vec<_>>().join(" / boş ")))
        .unwrap_or_default();
    Bilgi {
        sistem: format!("Armonia ({ad}){}", if canli() { " — canlı (deneme)" } else { "" }),
        cekirdek: calistir("uname", &["-r"]).unwrap_or_default().trim().to_string(),
        bellek: std::fs::read_to_string("/proc/meminfo").ok().and_then(|m| bellek_coz(&m)).unwrap_or_default(),
        disk: if disk.is_empty() { String::new() } else { format!("{disk}") },
        islemci,
    }
}

/// nmcli -t çıktısında bir satırı alanlara böler (`\:` ve `\\` kaçışları).
pub fn nmcli_alanlar(satir: &str) -> Vec<String> {
    let mut v = vec![String::new()];
    let mut k = satir.chars();
    while let Some(c) = k.next() {
        match c {
            '\\' => {
                if let Some(n) = k.next() {
                    v.last_mut().unwrap().push(n);
                }
            }
            ':' => v.push(String::new()),
            c => v.last_mut().unwrap().push(c),
        }
    }
    v
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiAgi {
    pub ssid: String,
    pub sinyal: u8,
    /// "psk", "sae" ya da "acik"
    pub guvenlik: &'static str,
    pub bagli: bool,
}

/// `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY dev wifi list` → aynı adlı ağlardan en güçlüsü, güçlüden zayıfa.
pub fn wifi_coz(m: &str) -> Vec<WifiAgi> {
    let mut v: Vec<WifiAgi> = Vec::new();
    for l in m.lines() {
        let a = nmcli_alanlar(l);
        if a.len() < 4 || a[1].is_empty() {
            continue; // gizli ağ
        }
        let g = &a[3];
        let guvenlik = if g.is_empty() || g == "--" {
            "acik"
        } else if g.contains("WPA3") && !g.contains("WPA2") {
            "sae"
        } else if g.contains("WPA") {
            "psk"
        } else {
            continue; // WEP / kurumsal (802.1X): desteklenmiyor
        };
        let ag = WifiAgi { ssid: a[1].clone(), sinyal: a[2].parse().unwrap_or(0), guvenlik, bagli: a[0] == "*" };
        match v.iter_mut().find(|x| x.ssid == ag.ssid) {
            Some(x) => {
                x.bagli |= ag.bagli;
                x.sinyal = x.sinyal.max(ag.sinyal);
            }
            None => v.push(ag),
        }
    }
    v.sort_by(|a, b| b.bagli.cmp(&a.bagli).then(b.sinyal.cmp(&a.sinyal)));
    v
}

pub fn wifi_tara() -> Result<Vec<WifiAgi>, String> {
    let m = calistir("nmcli", &["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "device", "wifi", "list", "--rescan", "yes"])?;
    Ok(wifi_coz(&m))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgDurumu {
    pub kablolu: Option<String>,
    pub kablosuz_var: bool,
    pub kablosuz: Option<String>,
    pub wifi_acik: bool,
}

/// `nmcli -t -f TYPE,STATE,CONNECTION device`
pub fn ag_durumu_coz(m: &str, radyo: &str) -> AgDurumu {
    let mut d = AgDurumu { wifi_acik: radyo.trim() == "enabled", ..Default::default() };
    for l in m.lines() {
        let a = nmcli_alanlar(l);
        if a.len() < 3 {
            continue;
        }
        let bagli = a[1] == "connected";
        match a[0].as_str() {
            "ethernet" if bagli => d.kablolu = Some(a[2].clone()),
            "wifi" => {
                d.kablosuz_var = true;
                if bagli {
                    d.kablosuz = Some(a[2].clone());
                }
            }
            _ => {}
        }
    }
    d
}

pub fn ag_durumu() -> Result<AgDurumu, String> {
    let m = calistir("nmcli", &["-t", "-f", "TYPE,STATE,CONNECTION", "device"])?;
    let r = calistir("nmcli", &["radio", "wifi"]).unwrap_or_default();
    Ok(ag_durumu_coz(&m, &r))
}

pub fn wifi_radyo(acik: bool) -> Result<(), String> {
    calistir("nmcli", &["radio", "wifi", if acik { "on" } else { "off" }]).map(|_| ())
}

/// Kayıtlı Wi-Fi ağları: (ad, uuid)
pub fn kayitli_wifi() -> Vec<(String, String)> {
    calistir("nmcli", &["-t", "-f", "NAME,UUID,TYPE", "connection", "show"])
        .map(|m| {
            m.lines()
                .map(nmcli_alanlar)
                .filter(|a| a.len() >= 3 && a[2] == "802-11-wireless")
                .map(|a| (a[0].clone(), a[1].clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn kayitliya_baglan(uuid: &str) -> Result<(), String> {
    calistir("nmcli", &["-w", "30", "connection", "up", "uuid", uuid]).map(|_| ())
}

/// Parola komut satırına değil yardımcının standart girişine yazılır.
pub fn wifi_baglan(ssid: &str, parola: &str, guvenlik: &str) -> Result<(), String> {
    use std::io::Write;
    if ssid.contains('\n') || parola.contains('\n') {
        return Err("Geçersiz karakter".into());
    }
    let mut c = Command::new("pkexec")
        .args([YARDIMCI, "wifi-baglan", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("pkexec çalıştırılamadı: {e}"))?;
    if let Some(mut g) = c.stdin.take() {
        let _ = write!(g, "{ssid}\n{parola}\n{guvenlik}\n");
    }
    let c = c.wait_with_output().map_err(|e| e.to_string())?;
    if c.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&c.stderr).trim().to_string())
    }
}

// ── Tema (23.11) ─────────────────────────────────────────────────────────────────────────────────────
pub const PALET_DOSYASI: &str = "/etc/armonia/tema/paletler";
pub const TEMA_YARDIMCI: &str = "/opt/armonia/bin/tema-yardimci";

/// Bir Armonia teması. Renk sırası paletler dosyasındaki gibi:
/// ARKA, ZEMIN, YAZI, KENAR, HAP, VURGU, VURGU_KOYU, VURGU_ZEMIN, PASIF, VURGU_UZERI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tema {
    pub kimlik: String,
    pub ad: String,
    pub renk: [u32; 10],
}

/// Varsayılan (dosya okunamazsa): 19.11 masaüstünün Armonia tonları.
pub fn armonia_temasi() -> Tema {
    Tema {
        kimlik: "armonia".into(),
        ad: "Armonia".into(),
        renk: [0xF3EEE6, 0xFCFAF6, 0x2F2F2F, 0xE2DACB, 0xECE6DB, 0x7C9A7E, 0x5F7A61, 0xE6EBE3, 0xD8CFC0, 0xFAF8F4],
    }
}

/// "kimlik|Ad|ARKA|...|VURGU_UZERI" satırları; yorum ve bozuk satırlar atlanır.
pub fn palet_coz(metin: &str) -> Vec<Tema> {
    metin
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let a: Vec<&str> = l.trim().split('|').collect();
            if a.len() != 12 || a[0].is_empty() || !a[0].chars().all(|c| c.is_ascii_lowercase()) {
                return None;
            }
            let mut renk = [0u32; 10];
            for (i, h) in a[2..].iter().enumerate() {
                if h.len() != 6 {
                    return None;
                }
                renk[i] = u32::from_str_radix(h, 16).ok()?;
            }
            Some(Tema { kimlik: a[0].to_string(), ad: a[1].to_string(), renk })
        })
        .collect()
}

pub fn temalar() -> Vec<Tema> {
    let l = std::fs::read_to_string(PALET_DOSYASI).map(|m| palet_coz(&m)).unwrap_or_default();
    if l.is_empty() { vec![armonia_temasi()] } else { l }
}

/// Root tema yardımcısı (polkit tr.armonia.tema: parolasız, yalnız izin listesindeki tema adı).
pub fn tema_uygula(kimlik: &str) -> Result<(), String> {
    calistir("pkexec", &[TEMA_YARDIMCI, kimlik]).map(|_| ())
}

/// Yeni renk dosyalarını oturuma okutur: waybar (SIGUSR2 = yeniden yükle), labwc (çerçeve), mako (bildirim).
/// fuzzel her açılışta okur.
pub fn tema_yenile() {
    let _ = calistir("pkill", &["-USR2", "-x", "waybar"]);
    let _ = calistir("labwc", &["--reconfigure"]);
    let _ = calistir("makoctl", &["reload"]);
}

// Çoklu UYAP temayı başlarken kendisi alır (cokluuyap_calistir.sh, ayarlar.conf TEMA=). Ayarlar kum havuzunun
// veri klasörüne YAZMAZ: oradaki bir sembolik bağ havuz dışındaki dosyayı ezdirebilirdi (denetçi 23.11 O1).

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn palet() {
        let m = "# yorum\nkimlik|Ad|ARKA\narmonia|Armonia|F3EEE6|FCFAF6|2F2F2F|E2DACB|ECE6DB|7C9A7E|5F7A61|E6EBE3|D8CFC0|FAF8F4\n\
                 gunbatimi|Gün Batımı|FBF4EE|FFFBF7|4A3020|EDD9C7|F5E8DC|C2602F|A34E24|FBEBDF|EDD9C7|FFFFFF\n\
                 Bozuk|X|1|2|3|4|5|6|7|8|9|0\nkisa|K|FFFFFF\nkotu|K|GGGGGG|FFFBF7|4A3020|EDD9C7|F5E8DC|C2602F|A34E24|FBEBDF|EDD9C7|FFFFFF\n";
        let t = palet_coz(m);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], armonia_temasi());
        assert_eq!(t[1].ad, "Gün Batımı");
        assert_eq!(t[1].renk[5], 0xC2602F);
    }

    /// Depodaki paletler dosyası: 9 tema, ilki Armonia (varsayılanla aynı), Çoklu UYAP kimlikleriyle uyumlu.
    #[test]
    fn depodaki_paletler() {
        let t = palet_coz(include_str!("../../../armonia-linux/config/includes.chroot/etc/armonia/tema/paletler"));
        let k: Vec<&str> = t.iter().map(|x| x.kimlik.as_str()).collect();
        assert_eq!(k, ["armonia", "gul", "okyanus", "lavanta", "gunbatimi", "gunes", "grafit", "uyap", "koyu"]);
        assert_eq!(t[0], armonia_temasi());
    }

    #[test]
    fn ayar() {
        let a = ayar_coz("# yorum\nKLAVYE=f\nKILIT_DK=15\nbozuk satir\nkucuk=x\n");
        assert_eq!(a.get("KLAVYE").map(String::as_str), Some("f"));
        assert_eq!(a.get("KILIT_DK").map(String::as_str), Some("15"));
        assert!(!a.contains_key("kucuk"));
    }

    #[test]
    fn yazici() {
        let v = "device for HP_LaserJet: ipp://localhost:60000/ipp/print\ndevice for Epson: usb://EPSON/L3150\n";
        let p = "printer HP_LaserJet is idle.  enabled since Thu 19 Nov 2026\nprinter Epson disabled since ...\n";
        let d = "system default destination: HP_LaserJet\n";
        let y = yazicilari_coz(v, p, d);
        assert_eq!(y.len(), 2);
        assert_eq!(y[0], Yazici { ad: "HP_LaserJet".into(), adres: "ipp://localhost:60000/ipp/print".into(), durum: "Hazır".into(), varsayilan: true });
        assert_eq!(y[1].durum, "Kapalı");
        assert!(!y[1].varsayilan);
        assert_eq!(baglanti_turu(&y[0].adres), "USB (sürücüsüz)");
        assert!(yazicilari_coz("", "", "no system default destination").is_empty());
    }

    #[test]
    fn nmcli() {
        assert_eq!(nmcli_alanlar(r"*:Ev\:Agi:70:WPA2"), vec!["*", "Ev:Agi", "70", "WPA2"]);
        let l = wifi_coz("*:Ofis:80:WPA2\n :Ofis:60:WPA2\n :Kafe:90:\n :Yeni:50:WPA3\n :Eski:40:WEP\n ::30:WPA2\n");
        assert_eq!(l.len(), 3); // Ofis (tek), Kafe, Yeni; WEP ve gizli ag atlanir
        assert_eq!(l[0], WifiAgi { ssid: "Ofis".into(), sinyal: 80, guvenlik: "psk", bagli: true });
        assert_eq!(l[1].ssid, "Kafe");
        assert_eq!(l[1].guvenlik, "acik");
        assert_eq!(l.iter().find(|a| a.ssid == "Yeni").unwrap().guvenlik, "sae");
        let d = ag_durumu_coz("ethernet:connected:Kablolu baglanti 1\nwifi:disconnected:\nloopback:connected (externally):lo\n", "enabled\n");
        assert_eq!(d.kablolu.as_deref(), Some("Kablolu baglanti 1"));
        assert!(d.kablosuz_var && d.wifi_acik && d.kablosuz.is_none());
    }

    #[test]
    fn bellek() {
        assert_eq!(bellek_coz("MemTotal:        8122408 kB\nMemFree: 1 kB").as_deref(), Some("7,7 GB"));
        assert_eq!(bellek_coz("yok"), None);
    }
}
