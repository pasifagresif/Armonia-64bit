//! armonia-guncelle: Çoklu UYAP imzalı güncelleme aracı. Claude, 2026-11-24.
//!
//! Yayıncı (çevrimdışı makinede):
//!   anahtar-uret <gizli_dosya>                       yeni anahtar; açık anahtarı ve kimliği yazar
//!   imzala <gizli_dosya> <dosya> [--iptal]           <dosya>.sig yazar (parola stdin/terminalden)
//!   paketle <kaynak_dizin> <cikti.tar.zst>           belirlenimci paket; sha256, boyut, ağaç özeti yazar
//!   bilgi-olustur --surum S --gecerlilik-gun N --imaj-arayuzu N --notlar DOSYA
//!                 --paket MIMARI=YOL [--paket ...] [--geri-cekilen A,B]   guncelleme.json'u stdout'a
//!   iptal-olustur --eski-kimlik K --yeni-acik B64 --sonraki-acik B64      iptal bildirimini stdout'a
//!   dogrula --anahtarlar DOSYA <json> <sig> [--paket YOL --mimari M]      yüklemeden önce son denetim
//! Cihaz (sabit yollar; ortam değişkeniyle değiştirilemez; canlı sistemde kapalı):
//!   denetle      (armonia-guncelle-denetle.service; ağ)      indir   (armonia-guncelle-indir.service; ağ)
//!   kur-gelen    (armonia-guncelle-kur.service; ağsız)       imaja-don (bu açılış için imajdaki sürüm)
//!   buda         (açılışta; etkin dışındaki sürümleri siler)   oz-denetim (imaj kancası)
//!   etkin-sec    (cokluuyap_calistir.sh; kum havuzu içinde)  kur <json> <sig> <paket>   iptal-ekle <bildirim> <sig>
//!   durum        (Ayarlar; JSON, imzayı yeniden doğrular)    cubuk  (üst çubuk göstergesi; waybar JSON)

use armonia_guncelle::anahtar::{self, Anahtarlar};
use armonia_guncelle::bilgi::{self, Baglam, Bilgi, Paket};
use armonia_guncelle::kur::{self, Imaj, KurGirdisi, Yerlesim};
use armonia_guncelle::sec::{self, Secim};
use armonia_guncelle::surum::Surum;
use armonia_guncelle::{dosya, gizli, hata, ozet_hex, paket, zaman, Sonuc, BICIM, KANAL, URUN};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const KOK: &str = "/var/lib/armonia-guncelle";
const ANAHTARLAR: &str = "/etc/armonia/guncelle/anahtarlar";
const AYAR: &str = "/etc/armonia/guncelle/ayar";
const IMAJ_SURUM: &str = "/opt/armonia/cokluuyap/SURUM";
/// Denetleyicinin çalışma dizini (sahibi armonia-guncelle-indir; herkes okur)
const CALISMA: &str = "/run/armonia-guncelle";
const SON_BILGI: &str = "/run/armonia-guncelle/guncelleme.json";
const SON_IMZA: &str = "/run/armonia-guncelle/guncelleme.json.sig";
/// "Bu açılışta imajdaki sürüm" işareti (sahibi armonia-guncelle; ağa çıkan kullanıcı yazamaz)
const IMAJA_DON: &str = "/run/armonia-guncelle-don/imaja-don";
const BILGI_ADI: &str = "guncelleme.json";
const IMZA_ADI: &str = "guncelleme.json.sig";
const IPTAL_ADI: &str = "iptal.json";
const IPTAL_IMZA_ADI: &str = "iptal.json.sig";

/// Ayarlar ve üst çubuk için durum. Önerilen sürüm ve notları, denetleyicinin yazdığı imzalı bilgiden BURADA
/// yeniden doğrulanarak alınır; durum.json'a (ağa çıkan kullanıcının yazdığı) yalnız zaman/hata için bakılır
/// (denetçi 24.11 O6/D-5).
#[derive(serde::Serialize, Default)]
struct Durum {
    /// Güncelleme katmanı var mı (anahtar dosyası ve birimler)
    katman: bool,
    canli: bool,
    imaj: Option<String>,
    /// Kurulu (etkin) güncelleme; imajdakinden yeni değilse yok. Ayarlar için etkin-sec ile doğrulanır.
    etkin: Option<String>,
    /// Kurulu güncelleme var ama açılışta kullanılmayacak (doğrulanamadı, geri çekildi...): nedeni (denetçi 24.11 O-3)
    etkin_red: Option<String>,
    /// Bu açılışta imajdaki sürüme dönüldü mü
    imaja_don: bool,
    teklif: Option<Teklif>,
    teklif_hata: Option<String>,
    son_denetim: Option<String>,
    son_denetim_hata: Option<String>,
    son_kurulum: Option<String>,
}

#[derive(serde::Serialize)]
struct Teklif {
    surum: String,
    notlar: String,
    yayin: String,
    gecerlilik_sonu: String,
    /// Kurulu/imaj sürümünden yeni mi
    kurulabilir: bool,
}

/// Gösterilecek metin: denetim (Cc) ve biçim/yön (Cf: U+200B-200F, U+202A-202E, U+2060-206F, U+FEFF) karakterleri
/// atılır; satır sonu yalnız izin verilirse kalır (denetçi 24.11 D-1).
fn temiz_satirli(s: &str, n: usize, satir: bool) -> String {
    s.chars()
        .filter(|&c| {
            let k = c as u32;
            let bicim = (0x200B..=0x200F).contains(&k) || (0x202A..=0x202E).contains(&k) || (0x2060..=0x206F).contains(&k) || k == 0xFEFF;
            !bicim && (!c.is_control() || (satir && c == '\n'))
        })
        .take(n)
        .collect()
}

fn temiz(s: &str, n: usize) -> String {
    temiz_satirli(s, n, false)
}

/// `agac`: kurulu güncellemeyi etkin-sec ile tam doğrula (Ayarlar; ağaç özeti alır). Üst çubuk her 30 dk çağırır,
/// orada gerekmez.
fn durum_topla(agac: bool) -> Durum {
    let mut d = Durum { katman: Path::new(ANAHTARLAR).exists(), canli: canli_mi(), ..Default::default() };
    let imaj = Imaj::oku(Path::new(IMAJ_SURUM), Path::new(AYAR)).ok();
    d.imaj = imaj.as_ref().map(|i| i.surum.to_string());
    if !d.katman || d.canli {
        return d;
    }
    d.imaja_don = !matches!(std::fs::symlink_metadata(IMAJA_DON), Err(ref e) if e.kind() == std::io::ErrorKind::NotFound);
    let y = Yerlesim::new(KOK);
    if let (Ok(Some(s)), Some(i)) = (kur::surum_dosyasi_oku(&y.etkin()), &imaj) {
        if s > i.surum {
            d.etkin = Some(s.to_string());
            if agac {
                let son_j = dosya::oku_varsa(Path::new(SON_BILGI), bilgi::BILGI_AZAMI).ok().flatten();
                let son_i = dosya::oku_varsa(Path::new(SON_IMZA), anahtar::IMZA_AZAMI).ok().flatten();
                let secim = Anahtarlar::yukle(Path::new(ANAHTARLAR), Some(&y.iptal()))
                    .and_then(|a| sec::etkin_sec(&y, &a, i, mimari(), zaman::simdi(), son_j.as_deref().zip(son_i.as_deref())));
                match secim {
                    Ok(Secim::Guncel(..)) => {}
                    Ok(Secim::Imaj(n)) => {
                        d.etkin = None;
                        d.etkin_red = Some(temiz(&format!("{s}: {n}"), 300));
                    }
                    Err(e) => {
                        d.etkin = None;
                        d.etkin_red = Some(temiz(&format!("{s} doğrulanamadı: {}", e.0), 300));
                    }
                }
            }
        }
    }
    if let Ok(Some(b)) = dosya::oku_varsa(&y.durum().join("son_kurulum"), 256) {
        d.son_kurulum = String::from_utf8_lossy(&b).lines().find_map(|l| l.strip_prefix("zaman=")).map(|z| temiz(z, 32));
    }
    if let Ok(Some(b)) = dosya::oku_varsa(&Path::new(CALISMA).join("durum.json"), 8192) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) {
            d.son_denetim = v["denetim"].as_str().map(|z| temiz(z, 32));
            if v["durum"] == "hata" {
                d.son_denetim_hata = v["hata"].as_str().map(|h| temiz(h, 300));
            }
        }
    }
    let c = Path::new(CALISMA);
    let teklif = (|| -> Sonuc<Option<Teklif>> {
        let Some(j) = dosya::oku_varsa(&c.join(BILGI_ADI), bilgi::BILGI_AZAMI)? else { return Ok(None) };
        let s = dosya::oku_sinirli(&c.join(IMZA_ADI), anahtar::IMZA_AZAMI)?;
        let ib = dosya::oku_varsa(&c.join(IPTAL_ADI), 4096)?;
        let ii = dosya::oku_varsa(&c.join(IPTAL_IMZA_ADI), anahtar::IMZA_AZAMI)?;
        let (_, a, imaj, taban) = cihaz_baglami(ib.as_deref().zip(ii.as_deref()))?;
        let b = Baglam { mimari: mimari().into(), imaj_arayuzu: imaj.arayuz, imaj_tarihi: imaj.tarih, simdi: zaman::simdi(), sure_denetimi: true };
        let v = bilgi::dogrula(&j, &s, &a, &b)?;
        Ok(Some(Teklif {
            surum: v.surum.to_string(),
            notlar: temiz_satirli(&v.bilgi.notlar, bilgi::NOT_AZAMI, true),
            yayin: v.bilgi.yayin.clone(),
            gecerlilik_sonu: v.bilgi.gecerlilik_sonu.clone(),
            kurulabilir: v.surum > taban,
        }))
    })();
    match teklif {
        Ok(t) => d.teklif = t,
        Err(e) => d.teklif_hata = Some(temiz(&e.0, 300)),
    }
    d
}

/// Ortak bağlam: anahtarlar (kalıcı iptal kaydıyla), imaj, en yüksek kurulu sürüm.
fn cihaz_baglami(ek_iptal: Option<(&[u8], &[u8])>) -> Sonuc<(Yerlesim, Anahtarlar, Imaj, Surum)> {
    let y = Yerlesim::new(KOK);
    let mut a = Anahtarlar::yukle(Path::new(ANAHTARLAR), Some(&y.iptal()))?;
    if let Some((b, i)) = ek_iptal {
        // Yeni indirilen iptal bildirimi bellekte uygulanır (kalıcı kaydı kurucu yazar); uymuyorsa yok sayılır
        use base64::Engine;
        let k = anahtar::IptalKaydi {
            bildirim: base64::engine::general_purpose::STANDARD.encode(b),
            imza: String::from_utf8_lossy(i).into_owned(),
        };
        let mut deneme = a.clone();
        match deneme.iptal_uygula(&k) {
            Ok(()) => {
                eprintln!("iptal bildirimi geçerli: yeni etkin anahtar {}", deneme.etkin.kimlik());
                a = deneme;
            }
            Err(e) => eprintln!("iptal bildirimi uygulanmadı: {e}"),
        }
    }
    let imaj = Imaj::oku(Path::new(IMAJ_SURUM), Path::new(AYAR))?;
    let en_yuksek = kur::surum_dosyasi_oku(&y.en_yuksek())?.unwrap_or(Surum::SIFIR);
    let taban = imaj.surum.max(en_yuksek);
    Ok((y, a, imaj, taban))
}

fn durum_yaz(alanlar: serde_json::Value) {
    let mut v = alanlar;
    v["denetim"] = zaman::yaz(zaman::simdi()).into();
    let _ = dosya::atomik_yaz(&Path::new(CALISMA).join("durum.json"), (v.to_string() + "\n").as_bytes(), 0o644);
}

fn denetle() -> Sonuc<()> {
    if canli_mi() {
        durum_yaz(serde_json::json!({ "durum": "canli" }));
        return Ok(());
    }
    let ajan = armonia_guncelle::ag::ajan();
    let iptal = match (armonia_guncelle::ag::getir(&ajan, IPTAL_ADI, 4096)?, armonia_guncelle::ag::getir(&ajan, IPTAL_IMZA_ADI, anahtar::IMZA_AZAMI)?) {
        (Some(b), Some(i)) => Some((b, i)),
        _ => None,
    };
    let (_, a, imaj, taban) = cihaz_baglami(iptal.as_ref().map(|(b, i)| (&b[..], &i[..])))?;
    let j = armonia_guncelle::ag::getir(&ajan, BILGI_ADI, bilgi::BILGI_AZAMI)?.ok_or_else(|| hata!("sunucuda güncelleme bilgisi yok"))?;
    let s = armonia_guncelle::ag::getir(&ajan, IMZA_ADI, anahtar::IMZA_AZAMI)?.ok_or_else(|| hata!("sunucuda imza yok"))?;
    let b = Baglam { mimari: mimari().into(), imaj_arayuzu: imaj.arayuz, imaj_tarihi: imaj.tarih, simdi: zaman::simdi(), sure_denetimi: true };
    let d = bilgi::dogrula(&j, &s, &a, &b)?;
    let c = Path::new(CALISMA);
    dosya::atomik_yaz(&c.join(BILGI_ADI), &j, 0o644)?;
    dosya::atomik_yaz(&c.join(IMZA_ADI), &s, 0o644)?;
    if let Some((ib, ii)) = &iptal {
        dosya::atomik_yaz(&c.join(IPTAL_ADI), ib, 0o644)?;
        dosya::atomik_yaz(&c.join(IPTAL_IMZA_ADI), ii, 0o644)?;
    }
    let var = d.surum > taban;
    durum_yaz(serde_json::json!({
        "durum": if var { "var" } else { "guncel" },
        "surum": d.surum.to_string(),
        "en_yuksek_kurulu": taban.to_string(),
        "notlar": d.bilgi.notlar,
    }));
    println!("{}", if var { format!("yeni sürüm var: {}", d.surum) } else { format!("güncel ({taban})") });
    Ok(())
}

fn indir() -> Sonuc<()> {
    if canli_mi() {
        return Err(hata!("USB'den (canlı) çalışırken güncelleme kapalı"));
    }
    let c = Path::new(CALISMA);
    let j = dosya::oku_sinirli(&c.join(BILGI_ADI), bilgi::BILGI_AZAMI).map_err(|_| hata!("önce güncelleme denetlenmeli"))?;
    let s = dosya::oku_sinirli(&c.join(IMZA_ADI), anahtar::IMZA_AZAMI)?;
    let ib = dosya::oku_varsa(&c.join(IPTAL_ADI), 4096)?;
    let ii = dosya::oku_varsa(&c.join(IPTAL_IMZA_ADI), anahtar::IMZA_AZAMI)?;
    let iptal = ib.as_deref().zip(ii.as_deref());
    let (y, a, imaj, taban) = cihaz_baglami(iptal)?;
    let b = Baglam { mimari: mimari().into(), imaj_arayuzu: imaj.arayuz, imaj_tarihi: imaj.tarih, simdi: zaman::simdi(), sure_denetimi: true };
    let d = bilgi::dogrula(&j, &s, &a, &b)?;
    if d.surum <= taban {
        return Err(hata!("kurulu sürüm ({taban}) zaten güncel"));
    }
    let g = y.gelen();
    let _kilit = kur::gelen_kilidi(&y, true)?;
    let gecici = g.join(".paket.yeni");
    let _ = std::fs::remove_file(&gecici);
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o640);
        }
        let mut f = o.open(&gecici)?;
        let ajan = armonia_guncelle::ag::ajan();
        if let Err(e) = armonia_guncelle::ag::paket_indir(&ajan, &d.paket.ad, &mut f, d.paket.boyut, &d.paket.sha256) {
            drop(f);
            let _ = std::fs::remove_file(&gecici);
            return Err(e);
        }
        f.sync_all()?;
    }
    std::fs::rename(&gecici, g.join("paket.tar.zst"))?;
    // Paketle birlikte, onu tanımlayan imzalı bilgi (kurucu yalnız bu klasörden okur)
    dosya::atomik_yaz(&g.join(BILGI_ADI), &j, 0o640)?;
    dosya::atomik_yaz(&g.join(IMZA_ADI), &s, 0o640)?;
    for ad in [IPTAL_ADI, IPTAL_IMZA_ADI] {
        match dosya::oku_varsa(&c.join(ad), 4096)? {
            Some(v) => dosya::atomik_yaz(&g.join(ad), &v, 0o640)?,
            None => {
                let _ = std::fs::remove_file(g.join(ad));
            }
        }
    }
    println!("indirildi: {} ({} bayt)", d.paket.ad, d.paket.boyut);
    Ok(())
}

fn kur_gelen() -> Sonuc<()> {
    if canli_mi() {
        return Err(hata!("USB'den (canlı) çalışırken güncelleme kapalı"));
    }
    let y = Yerlesim::new(KOK);
    let g = y.gelen();
    let _kilit = kur::gelen_kilidi(&y, false)?;
    // Yeni iptal bildirimi varsa kalıcı kayda (yedek anahtarla doğrulanır; zaten uygulanmışsa reddedilir)
    if let (Some(b), Some(i)) = (dosya::oku_varsa(&g.join(IPTAL_ADI), 4096)?, dosya::oku_varsa(&g.join(IPTAL_IMZA_ADI), anahtar::IMZA_AZAMI)?) {
        match kur::iptal_ekle(&y, Path::new(ANAHTARLAR), &b, &i) {
            Ok(k) => eprintln!("iptal kaydedildi; etkin anahtar {k}"),
            Err(e) => eprintln!("iptal bildirimi kaydedilmedi: {e}"),
        }
    }
    let a = Anahtarlar::yukle(Path::new(ANAHTARLAR), Some(&y.iptal()))?;
    let imaj = Imaj::oku(Path::new(IMAJ_SURUM), Path::new(AYAR))?;
    let jb = dosya::oku_sinirli(&g.join(BILGI_ADI), bilgi::BILGI_AZAMI)?;
    let sb = dosya::oku_sinirli(&g.join(IMZA_ADI), anahtar::IMZA_AZAMI)?;
    let v = kur::kur(&y, &a, &imaj, mimari(), zaman::simdi(), KurGirdisi {
        bilgi: &jb,
        imza: &sb,
        paket: &g.join("paket.tar.zst"),
        sahip_uid: gelen_sahibi(&y),
    })?;
    let _ = dosya::atomik_yaz(&y.durum().join("son_kurulum"), format!("surum={v}\nzaman={}\n", zaman::yaz(zaman::simdi())).as_bytes(), 0o644);
    println!("kuruldu: {v} (Çoklu UYAP yeniden açılınca etkin olur)");
    Ok(())
}

fn mimari() -> &'static str {
    std::env::consts::ARCH
}

/// Canlı (USB, toram) sistemde güncelleme kapalı (kullanıcı kararı 24.11).
fn canli_mi() -> bool {
    // Okunamazsa canlı sayılır (güncelleme kapalı kalır; denetçi 24.11 D-2)
    std::fs::read_to_string("/proc/cmdline").map(|c| c.split_whitespace().any(|t| t == "boot=live")).unwrap_or(true)
}

fn parola_oku(soru: &str) -> Sonuc<String> {
    rpassword::prompt_password(soru).map_err(|e| hata!("parola okunamadı: {e}"))
}

fn secenek(arg: &[String], ad: &str) -> Option<String> {
    arg.iter().position(|a| a == ad).and_then(|i| arg.get(i + 1).cloned())
}

fn secenekler(arg: &[String], ad: &str) -> Vec<String> {
    arg.windows(2).filter(|w| w[0] == ad).map(|w| w[1].clone()).collect()
}

fn konumsal(arg: &[String]) -> Vec<String> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < arg.len() {
        if arg[i].starts_with("--") {
            i += if arg[i] == "--iptal" { 1 } else { 2 };
        } else {
            v.push(arg[i].clone());
            i += 1;
        }
    }
    v
}

fn calistir(arg: Vec<String>) -> Sonuc<()> {
    let komut = arg.first().cloned().unwrap_or_default();
    let r = &arg[1.min(arg.len())..];
    let k = konumsal(r);
    match komut.as_str() {
        "anahtar-uret" => {
            let cikti = k.first().ok_or_else(|| hata!("kullanım: anahtar-uret <gizli_dosya>"))?;
            if Path::new(cikti).exists() {
                return Err(hata!("{cikti} zaten var"));
            }
            let p1 = parola_oku("Parola (en az 12 karakter): ")?;
            let p2 = parola_oku("Parola (tekrar): ")?;
            if p1 != p2 {
                return Err(hata!("parolalar aynı değil"));
            }
            let (d, pkcs8) = gizli::uret(p1.as_bytes())?;
            dosya::atomik_yaz(Path::new(cikti), &d, 0o600)?;
            let (acik, _) = anahtar::imzala(&pkcs8, b"", b"")?;
            println!("acik={}\nkimlik={}\nozet={}", acik.b64(), acik.kimlik(), ozet_hex(&acik.0));
            Ok(())
        }
        "imzala" => {
            let [g, f] = k.as_slice() else { return Err(hata!("kullanım: imzala <gizli_dosya> <dosya> [--iptal]")) };
            let onek = if r.iter().any(|a| a == "--iptal") { anahtar::ONEK_IPTAL } else { anahtar::ONEK_GUNCELLEME };
            let pkcs8 = gizli::coz(parola_oku("Parola: ")?.as_bytes(), &std::fs::read(g)?)?;
            let veri = std::fs::read(f)?;
            let (acik, imza) = anahtar::imzala(&pkcs8, onek, &veri)?;
            dosya::atomik_yaz(&PathBuf::from(format!("{f}.sig")), imza.as_bytes(), 0o644)?;
            eprintln!("{f}.sig yazıldı (anahtar {})", acik.kimlik());
            Ok(())
        }
        "paketle" => {
            let [kaynak, cikti] = k.as_slice() else { return Err(hata!("kullanım: paketle <kaynak_dizin> <cikti.tar.zst>")) };
            let (zst, agac_ozeti) = paket::paketle(Path::new(kaynak))?;
            dosya::atomik_yaz(Path::new(cikti), &zst, 0o644)?;
            println!("sha256={}\nboyut={}\nagac_ozeti={agac_ozeti}", ozet_hex(&zst), zst.len());
            Ok(())
        }
        "bilgi-olustur" => {
            let s = secenek(r, "--surum").and_then(|s| Surum::coz(&s)).ok_or_else(|| hata!("--surum A.B.C gerekli"))?;
            let gun: i64 = secenek(r, "--gecerlilik-gun").and_then(|g| g.parse().ok()).filter(|g| (1..=60).contains(g))
                // Kısa tutulur: ele geçirilmiş denetleyici eski (geçerli) bilgiyi en çok bu kadar sunabilir (denetçi 24.11 O-2)
                .ok_or_else(|| hata!("--gecerlilik-gun 1..60 gerekli (önerilen 30)"))?;
            let arayuz: u32 = secenek(r, "--imaj-arayuzu").and_then(|g| g.parse().ok()).ok_or_else(|| hata!("--imaj-arayuzu gerekli"))?;
            let notlar = std::fs::read_to_string(secenek(r, "--notlar").ok_or_else(|| hata!("--notlar DOSYA gerekli"))?)?;
            let geri: Vec<String> = secenek(r, "--geri-cekilen").map(|g| g.split(',').map(str::to_string).collect()).unwrap_or_default();
            let mut paketler = BTreeMap::new();
            for p in secenekler(r, "--paket") {
                let (m, yol) = p.split_once('=').ok_or_else(|| hata!("--paket MIMARI=YOL"))?;
                if !bilgi::mimari_gecerli(m) {
                    return Err(hata!("bilinmeyen mimari {m}"));
                }
                let zst = std::fs::read(yol)?;
                // ağaç özeti: paketi geçici olarak açıp MANIFEST'ten
                let agac_ozeti = agac_ozeti_bul(&zst)?;
                paketler.insert(m.to_string(), Paket { ad: bilgi::paket_adi(s, m), sha256: ozet_hex(&zst), boyut: zst.len() as u64, agac_ozeti });
            }
            if paketler.is_empty() {
                return Err(hata!("en az bir --paket gerekli"));
            }
            let simdi = zaman::simdi();
            let b = Bilgi {
                bicim: BICIM, urun: URUN.into(), kanal: KANAL.into(), surum: s.to_string(),
                yayin: zaman::yaz(simdi), gecerlilik_sonu: zaman::yaz(simdi + gun * 86400), imaj_arayuzu: arayuz,
                geri_cekilen: geri, paketler, notlar: notlar.trim_end().to_string(),
            };
            println!("{}", serde_json::to_string_pretty(&b).expect("json"));
            Ok(())
        }
        "iptal-olustur" => {
            let eski = secenek(r, "--eski-kimlik").ok_or_else(|| hata!("--eski-kimlik gerekli"))?;
            let yeni = anahtar::AcikAnahtar::b64_coz(&secenek(r, "--yeni-acik").ok_or_else(|| hata!("--yeni-acik gerekli"))?)?;
            let sonraki = anahtar::AcikAnahtar::b64_coz(
                &secenek(r, "--sonraki-acik").ok_or_else(|| hata!("--sonraki-acik gerekli (bir sonraki iptal için taahhüt)"))?,
            )?;
            let b = anahtar::IptalBildirimi {
                islem: "iptal".into(),
                urun: URUN.into(),
                anahtar_kimligi: eski,
                yeni_etkin: yeni.b64(),
                sonraki: ozet_hex(&sonraki.0),
            };
            println!("{}", serde_json::to_string_pretty(&b).expect("json"));
            Ok(())
        }
        "dogrula" => {
            let [j, s] = k.as_slice() else { return Err(hata!("kullanım: dogrula --anahtarlar DOSYA <json> <sig> [--paket YOL --mimari M]")) };
            let a = Anahtarlar::yukle(Path::new(&secenek(r, "--anahtarlar").ok_or_else(|| hata!("--anahtarlar gerekli"))?), None)?;
            let (jb, sb) = (std::fs::read(j)?, std::fs::read(s)?);
            let m = secenek(r, "--mimari").unwrap_or_else(|| "x86_64".into());
            let b = Baglam { mimari: m.clone(), imaj_arayuzu: u32::MAX, imaj_tarihi: 0, simdi: zaman::simdi(), sure_denetimi: true };
            let d = bilgi::dogrula(&jb, &sb, &a, &b)?;
            println!("imza ve bilgi geçerli: {} ({}), geçerlilik sonu {}", d.surum, m, d.bilgi.gecerlilik_sonu);
            if let Some(p) = secenek(r, "--paket") {
                let zst = std::fs::read(&p)?;
                if ozet_hex(&zst) != d.paket.sha256 || zst.len() as u64 != d.paket.boyut {
                    return Err(hata!("paket bilgiyle tutmuyor"));
                }
                let t = tempdir_yolu()?;
                let s = paket::ac(&zst[..], &t, &d.paket.agac_ozeti, &m);
                let _ = std::fs::remove_dir_all(&t);
                s?;
                println!("paket geçerli: {p}");
            }
            Ok(())
        }
        "kur" => {
            if canli_mi() {
                return Err(hata!("USB'den (canlı) çalışırken güncelleme kapalı"));
            }
            let [j, s, p] = k.as_slice() else { return Err(hata!("kullanım: kur <json> <sig> <paket>")) };
            let y = Yerlesim::new(KOK);
            let a = Anahtarlar::yukle(Path::new(ANAHTARLAR), Some(&y.iptal()))?;
            let imaj = Imaj::oku(Path::new(IMAJ_SURUM), Path::new(AYAR))?;
            let jb = dosya::oku_sinirli(Path::new(j), bilgi::BILGI_AZAMI)?;
            let sb = dosya::oku_sinirli(Path::new(s), anahtar::IMZA_AZAMI)?;
            let sahip = gelen_sahibi(&y);
            let v = kur::kur(&y, &a, &imaj, mimari(), zaman::simdi(), KurGirdisi { bilgi: &jb, imza: &sb, paket: Path::new(p), sahip_uid: sahip })?;
            println!("kuruldu: {v}");
            Ok(())
        }
        "denetle" => denetle().inspect_err(|e| durum_yaz(serde_json::json!({ "durum": "hata", "hata": e.0 }))),
        "indir" => indir(),
        "kur-gelen" => kur_gelen(),
        "durum" => {
            // Ayarlar → Çoklu UYAP → Güncellemeler (oturum kullanıcısıyla; yalnız okur ve doğrular)
            println!("{}", serde_json::to_string(&durum_topla(true)).expect("json"));
            Ok(())
        }
        "cubuk" => {
            // Üst çubuk göstergesi (waybar custom, return-type json): yalnız imzası doğrulanmış, kurulabilir sürüm
            // varsa görünür; tıklayınca Ayarlar açılır (orada yeniden doğrulanır)
            let d = durum_topla(false);
            let v = match d.teklif.as_ref().filter(|t| t.kurulabilir) {
                Some(t) => serde_json::json!({
                    "text": "Güncelleme",
                    "tooltip": format!("Çoklu UYAP {} hazır. Kurmak için tıklayın (Ayarlar).", t.surum),
                    "class": "var",
                }),
                None => serde_json::json!({ "text": "", "class": "yok" }),
            };
            println!("{v}");
            Ok(())
        }
        "oz-denetim" => {
            // İmaj kancası (0230) için: canlı denetiminden BAĞIMSIZ olarak anahtar dosyası, imaj SURUM'u, ayar ve
            // seçim mantığı gerçekten çalışıyor mu (chroot'ta /proc olmayabilir; denetçi 24.11 O-3)
            let (y, a, imaj, taban) = cihaz_baglami(None)?;
            match sec::etkin_sec(&y, &a, &imaj, mimari(), zaman::simdi(), None)? {
                Secim::Imaj(_) => {}
                Secim::Guncel(d, _) => return Err(hata!("imajda kurulu güncelleme olmamalı: {}", d.display())),
            }
            println!("tamam: etkin {} / yedek {}, imaj {} (taban {taban}), arayüz {}", a.etkin.kimlik(), a.yedek.kimlik(), imaj.surum, imaj.arayuz);
            Ok(())
        }
        "buda" => {
            // Açılışta (armonia-guncelle-buda.service), Çoklu UYAP çalışmazken: etkin dışındaki sürümler silinir
            let y = Yerlesim::new(KOK);
            for s in kur::buda(&y)? {
                eprintln!("eski sürüm silindi: {s}");
            }
            Ok(())
        }
        "imaja-don" => {
            // OnFailure= ya da Ayarlar: bu açılış boyunca imajdaki sürüm (kalıcı değil; denetçi 24.11 K2)
            dosya::atomik_yaz(Path::new(IMAJA_DON), b"1\n", 0o644)?;
            eprintln!("bu açılışta Çoklu UYAP imajdaki sürümle açılacak");
            Ok(())
        }
        "etkin-sec" => {
            // Çıktı: çalıştırılacak dizin ya da boş (imaj). Gerekçe stderr'e (günlük).
            if canli_mi() {
                return Ok(());
            }
            // İşaret var ya da durumu okunamıyor (izin vb.) → imaj (denetçi 24.11 D-2)
            if !matches!(std::fs::symlink_metadata(IMAJA_DON), Err(ref e) if e.kind() == std::io::ErrorKind::NotFound) {
                eprintln!("Çoklu UYAP imajdaki sürüm: bu açılış için imaja dönüldü");
                return Ok(());
            }
            let y = Yerlesim::new(KOK);
            let a = Anahtarlar::yukle(Path::new(ANAHTARLAR), Some(&y.iptal()))?;
            let imaj = Imaj::oku(Path::new(IMAJ_SURUM), Path::new(AYAR))?;
            let son_j = dosya::oku_varsa(Path::new(SON_BILGI), bilgi::BILGI_AZAMI).ok().flatten();
            let son_i = dosya::oku_varsa(Path::new(SON_IMZA), anahtar::IMZA_AZAMI).ok().flatten();
            let son = son_j.as_deref().zip(son_i.as_deref());
            match sec::etkin_sec(&y, &a, &imaj, mimari(), zaman::simdi(), son)? {
                Secim::Guncel(d, s) => {
                    eprintln!("Çoklu UYAP {s} (güncelleme, doğrulandı)");
                    println!("{}", d.display());
                }
                Secim::Imaj(neden) => eprintln!("Çoklu UYAP imajdaki sürüm: {neden}"),
            }
            Ok(())
        }
        "iptal-ekle" => {
            if canli_mi() {
                return Err(hata!("USB'den (canlı) çalışırken güncelleme kapalı"));
            }
            let [bf, sf] = k.as_slice() else { return Err(hata!("kullanım: iptal-ekle <bildirim> <sig>")) };
            let y = Yerlesim::new(KOK);
            let bb = dosya::oku_sinirli(Path::new(bf), 4096)?;
            let sb = dosya::oku_sinirli(Path::new(sf), anahtar::IMZA_AZAMI)?;
            let yeni = kur::iptal_ekle(&y, Path::new(ANAHTARLAR), &bb, &sb)?;
            println!("iptal uygulandı; yeni etkin anahtar {yeni}");
            Ok(())
        }
        _ => Err(hata!(
            "komutlar: anahtar-uret, imzala, paketle, bilgi-olustur, iptal-olustur, dogrula, denetle, indir, kur-gelen, \
             kur, buda, etkin-sec, imaja-don, iptal-ekle, oz-denetim, durum, cubuk"
        )),
    }
}

#[cfg(unix)]
fn gelen_sahibi(y: &Yerlesim) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(y.gelen()).ok().map(|m| m.uid())
}
#[cfg(not(unix))]
fn gelen_sahibi(_: &Yerlesim) -> Option<u32> {
    None
}

fn tempdir_yolu() -> Sonuc<PathBuf> {
    let t = std::env::temp_dir().join(format!("armonia-guncelle-{}-{}", std::process::id(), zaman::simdi()));
    if t.exists() {
        return Err(hata!("{} zaten var", t.display()));
    }
    Ok(t)
}

fn agac_ozeti_bul(zst: &[u8]) -> Sonuc<String> {
    let dec = ruzstd_ac(zst)?;
    let mut ar = tar::Archive::new(dec);
    for g in ar.entries().map_err(|e| hata!("tar: {e}"))? {
        let mut g = g.map_err(|e| hata!("tar: {e}"))?;
        if g.path_bytes().as_ref() == paket::MANIFEST.as_bytes() {
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut g, &mut v)?;
            return Ok(ozet_hex(&v));
        }
    }
    Err(hata!("pakette MANIFEST yok"))
}

fn ruzstd_ac(zst: &[u8]) -> Sonuc<impl std::io::Read + '_> {
    ruzstd::decoding::StreamingDecoder::new(zst).map_err(|e| hata!("zstd: {e}"))
}

fn main() -> ExitCode {
    let arg: Vec<String> = std::env::args().skip(1).collect();
    match calistir(arg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("armonia-guncelle: HATA: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod testler {
    use super::temiz_satirli;

    #[test]
    fn gosterim_suzgeci() {
        let kotu = "a\u{202E}b\u{2066}c\u{200B}d\u{FEFF}e\u{1b}[31mf\ng";
        assert_eq!(temiz_satirli(kotu, 100, false), "abcde[31mfg");
        assert_eq!(temiz_satirli(kotu, 100, true), "abcde[31mf\ng");
        assert_eq!(temiz_satirli("çğıöşü İ", 3, false), "çğı");
    }
}
