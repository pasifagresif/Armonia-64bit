//! Dosya sistemi işlemleri: listeleme, kopyala/taşı/sil/yeniden adlandır/yeni klasör.
//! Hiçbiri `unwrap()` kullanmaz; hatalar `Result<_, String>` ile arayüze (Türkçe) döner.
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Girdi {
    pub yol: PathBuf,
    pub ad: String,
    pub dizin: bool,
    pub boyut: u64,
}

/// Bir dizinin içeriğini listeler: önce dizinler, sonra dosyalar, ikisi de ada göre (Türkçe harf
/// sırasına değil basit baytlara göre; UTF-8 karşılaştırma yeterli, özel bir koleksiyon kuralı gerekmiyor).
pub fn listele(dizin: &Path) -> Result<Vec<Girdi>, String> {
    let okuma = fs::read_dir(dizin).map_err(|e| format!("okunamadı: {e}"))?;
    let mut liste = Vec::new();
    for girdi in okuma.flatten() {
        let yol = girdi.path();
        let Ok(bilgi) = girdi.metadata() else {
            continue;
        };
        let ad = girdi.file_name().to_string_lossy().into_owned();
        liste.push(Girdi {
            yol,
            ad,
            dizin: bilgi.is_dir(),
            boyut: bilgi.len(),
        });
    }
    liste.sort_by(|a, b| match (a.dizin, b.dizin) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.ad.to_lowercase().cmp(&b.ad.to_lowercase()),
    });
    Ok(liste)
}

pub fn yeni_klasor(ust: &Path, ad: &str) -> Result<(), String> {
    let ad = ad.trim();
    if ad.is_empty() || ad.contains('/') {
        return Err("geçersiz klasör adı".to_string());
    }
    fs::create_dir(ust.join(ad)).map_err(|e| format!("klasör oluşturulamadı: {e}"))
}

pub fn yeniden_adlandir(yol: &Path, yeni_ad: &str) -> Result<PathBuf, String> {
    let yeni_ad = yeni_ad.trim();
    if yeni_ad.is_empty() || yeni_ad.contains('/') {
        return Err("geçersiz ad".to_string());
    }
    let hedef = yol.parent().ok_or("üst dizin yok")?.join(yeni_ad);
    fs::rename(yol, &hedef).map_err(|e| format!("yeniden adlandırılamadı: {e}"))?;
    Ok(hedef)
}

/// Kalıcı silme: dahili kullanım içindir (bkz. `tasi` — bölümler arası taşımanın kopyala+sil yedeği).
/// Kullanıcıya doğrudan sunulan "Sil" `cop_kutusuna_tasi` kullanır, bu geri alınabilir kalır.
pub fn sil(yol: &Path) -> Result<(), String> {
    if yol.is_dir() {
        fs::remove_dir_all(yol).map_err(|e| format!("silinemedi: {e}"))
    } else {
        fs::remove_file(yol).map_err(|e| format!("silinemedi: {e}"))
    }
}

pub fn cop_kutusu_dizini() -> Result<PathBuf, String> {
    let taban = std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| {
        let ev = std::env::var("HOME").unwrap_or_else(|_| "/home/armonia".to_string());
        PathBuf::from(ev).join(".local/share")
    });
    let kok = taban.join("armonia-dosyalar/cop-kutusu");
    fs::create_dir_all(&kok).map_err(|e| format!("çöp kutusu oluşturulamadı: {e}"))?;
    Ok(kok)
}

/// Çöp kutusunda çakışmayan ad bulur ("Ad (2).uzanti" biçiminde).
fn benzersiz_cop_adi(kok: &Path, ad: &str) -> String {
    if !kok.join(ad).exists() {
        return ad.to_string();
    }
    let (govde, uzanti) = match ad.rsplit_once('.') {
        Some((g, u)) if !g.is_empty() => (g.to_string(), Some(u.to_string())),
        _ => (ad.to_string(), None),
    };
    for i in 2.. {
        let aday = match &uzanti {
            Some(u) => format!("{govde} ({i}).{u}"),
            None => format!("{govde} ({i})"),
        };
        if !kok.join(&aday).exists() {
            return aday;
        }
    }
    unreachable!()
}

/// Çöp kutusundaki bir öğenin özgün yolunu tutan yan dosya ("<ad>.konum").
pub const KONUM_UZANTISI: &str = ".konum";

fn konum_dosyasi(cop_ogesi: &Path) -> Option<PathBuf> {
    let ad = cop_ogesi.file_name()?.to_string_lossy().into_owned();
    Some(cop_ogesi.parent()?.join(format!("{ad}{KONUM_UZANTISI}")))
}

/// Kullanıcının "Sil" dediği öğeyi kalıcı silmek yerine çöp kutusuna taşır (yanlış silme kalıcı
/// kayıp olmasın; kullanıcı isteği 25.11). Çöp kutusu: $XDG_DATA_HOME/armonia-dosyalar/cop-kutusu.
/// Her öğenin yanına özgün yolunu tutan bir ".konum" dosyası yazılır ("Geri Yükle" bunu okur).
pub fn cop_kutusuna_tasi(yol: &Path) -> Result<(), String> {
    cop_icine_tasi(&cop_kutusu_dizini()?, yol)
}

fn cop_icine_tasi(kok: &Path, yol: &Path) -> Result<(), String> {
    if yol.starts_with(kok) {
        return Err("bu öğe zaten çöp kutusunda".to_string());
    }
    let ad = yol
        .file_name()
        .ok_or("geçersiz ad")?
        .to_string_lossy()
        .into_owned();
    let hedef_ad = benzersiz_cop_adi(kok, &ad);
    let hedef = kok.join(&hedef_ad);
    if fs::rename(yol, &hedef).is_err() {
        kopyala_ozyinelemeli(yol, &hedef)?;
        sil(yol)?;
    }
    let simdi = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let bilgi = format!("{}\n{simdi}\n", yol.display());
    // Konum bilgisi yazilamazsa da tasima basarili sayilir (asil guvenlik: dosya kaybolmadi)
    let _ = fs::write(kok.join(format!("{hedef_ad}{KONUM_UZANTISI}")), bilgi);
    Ok(())
}

/// Çöp kutusundaki öğeyi silindiği yere geri koyar; geri konduğu yolu döndürür.
/// Aynı yerde aynı adda bir öğe varsa üzerine YAZMAZ, hata döner.
pub fn geri_yukle(cop_ogesi: &Path) -> Result<PathBuf, String> {
    let konum = konum_dosyasi(cop_ogesi).ok_or("geçersiz ad")?;
    let metin = fs::read_to_string(&konum)
        .map_err(|_| "özgün konum bilinmiyor; öğeyi Kes/Yapıştır ile taşıyın".to_string())?;
    let hedef = PathBuf::from(metin.lines().next().unwrap_or_default());
    if !hedef.is_absolute() {
        return Err("özgün konum kaydı bozuk".to_string());
    }
    if hedef.exists() {
        return Err(format!("\"{}\" zaten var; geri yüklenmedi", hedef.display()));
    }
    if let Some(ust) = hedef.parent() {
        fs::create_dir_all(ust).map_err(|e| format!("klasör oluşturulamadı: {e}"))?;
    }
    if fs::rename(cop_ogesi, &hedef).is_err() {
        kopyala_ozyinelemeli(cop_ogesi, &hedef)?;
        sil(cop_ogesi)?;
    }
    let _ = fs::remove_file(konum);
    Ok(hedef)
}

/// Çöp kutusundaki tek bir öğeyi geri dönüşsüz siler.
pub fn kalici_sil(cop_ogesi: &Path) -> Result<(), String> {
    sil(cop_ogesi)?;
    if let Some(konum) = konum_dosyasi(cop_ogesi) {
        let _ = fs::remove_file(konum);
    }
    Ok(())
}

/// Çöp kutusunun tamamını geri dönüşsüz siler.
pub fn cop_bosalt(kok: &Path) -> Result<(), String> {
    for girdi in listele(kok)? {
        sil(&girdi.yol)?;
    }
    Ok(())
}

/// Kopyalar (dosya ya da dizin, özyinelemeli). Hedef dizinin İÇİNE, kaynağın adıyla kopyalanır.
pub fn kopyala(kaynak: &Path, hedef_dizin: &Path) -> Result<(), String> {
    let ad = kaynak.file_name().ok_or("geçersiz kaynak adı")?;
    let hedef = hedef_dizin.join(ad);
    if hedef == *kaynak {
        return Err("kaynak ve hedef aynı".to_string());
    }
    kopyala_ozyinelemeli(kaynak, &hedef)
}

fn kopyala_ozyinelemeli(kaynak: &Path, hedef: &Path) -> Result<(), String> {
    if kaynak.is_dir() {
        fs::create_dir_all(hedef).map_err(|e| format!("klasör oluşturulamadı: {e}"))?;
        for girdi in listele(kaynak)? {
            kopyala_ozyinelemeli(&girdi.yol, &hedef.join(&girdi.ad))?;
        }
        Ok(())
    } else {
        fs::copy(kaynak, hedef)
            .map_err(|e| format!("kopyalanamadı: {e}"))
            .map(|_| ())
    }
}

/// Taşır: aynı disk/bölümde basit `rename`; başka bölümdeyse (ör. USB) kopyala+sil ile tamamlanır.
pub fn tasi(kaynak: &Path, hedef_dizin: &Path) -> Result<(), String> {
    let ad = kaynak.file_name().ok_or("geçersiz kaynak adı")?;
    let hedef = hedef_dizin.join(ad);
    if hedef == *kaynak {
        return Err("kaynak ve hedef aynı".to_string());
    }
    match fs::rename(kaynak, &hedef) {
        Ok(()) => Ok(()),
        Err(_) => {
            kopyala_ozyinelemeli(kaynak, &hedef)?;
            sil(kaynak)
        }
    }
}

pub fn boyut_metni(bayt: u64) -> String {
    const B: f64 = 1024.0;
    let b = bayt as f64;
    if b < B {
        format!("{bayt} B")
    } else if b < B * B {
        format!("{:.1} KB", b / B)
    } else if b < B * B * B {
        format!("{:.1} MB", b / (B * B))
    } else {
        format!("{:.1} GB", b / (B * B * B))
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn boyut_bicimi() {
        assert_eq!(boyut_metni(500), "500 B");
        assert_eq!(boyut_metni(2048), "2.0 KB");
        assert_eq!(boyut_metni(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn listele_once_dizinler_sonra_dosyalar() {
        let gecici =
            std::env::temp_dir().join(format!("armonia-dosyalar-test-{}", std::process::id()));
        fs::create_dir_all(gecici.join("b_dizin")).unwrap();
        fs::write(gecici.join("a_dosya.txt"), "x").unwrap();
        let liste = listele(&gecici).unwrap();
        assert!(liste[0].dizin);
        assert_eq!(liste[0].ad, "b_dizin");
        assert!(!liste[1].dizin);
        fs::remove_dir_all(&gecici).unwrap();
    }

    #[test]
    fn kopyala_ve_tasi() {
        let gecici =
            std::env::temp_dir().join(format!("armonia-dosyalar-test2-{}", std::process::id()));
        let kaynak_d = gecici.join("kaynak");
        let hedef_d = gecici.join("hedef");
        fs::create_dir_all(&kaynak_d).unwrap();
        fs::create_dir_all(&hedef_d).unwrap();
        let dosya = kaynak_d.join("deneme.txt");
        fs::write(&dosya, "icerik").unwrap();

        kopyala(&dosya, &hedef_d).unwrap();
        assert!(hedef_d.join("deneme.txt").exists());
        assert!(dosya.exists(), "kopyalamada kaynak silinmemeli");

        tasi(&hedef_d.join("deneme.txt"), &kaynak_d).unwrap();
        assert!(kaynak_d.join("deneme.txt").exists());
        assert!(
            !hedef_d.join("deneme.txt").exists(),
            "taşımada kaynak kalmamalı"
        );

        fs::remove_dir_all(&gecici).unwrap();
    }

    #[test]
    fn cop_kutusu_tasi_geri_yukle_ve_bosalt() {
        let gecici =
            std::env::temp_dir().join(format!("armonia-dosyalar-test3-{}", std::process::id()));
        let cop = gecici.join("cop");
        let belgeler = gecici.join("belgeler");
        fs::create_dir_all(&cop).unwrap();
        fs::create_dir_all(&belgeler).unwrap();
        let dosya = belgeler.join("dilekce.txt");
        fs::write(&dosya, "bir").unwrap();

        cop_icine_tasi(&cop, &dosya).unwrap();
        assert!(!dosya.exists(), "silinen dosya yerinde kalmamalı");
        assert!(cop.join("dilekce.txt").exists());
        assert!(cop.join("dilekce.txt.konum").exists());

        // Aynı adla ikinci silme çakışmaz
        fs::write(&dosya, "iki").unwrap();
        cop_icine_tasi(&cop, &dosya).unwrap();
        assert!(cop.join("dilekce (2).txt").exists());

        let geri = geri_yukle(&cop.join("dilekce.txt")).unwrap();
        assert_eq!(geri, dosya);
        assert_eq!(fs::read_to_string(&dosya).unwrap(), "bir");
        assert!(!cop.join("dilekce.txt.konum").exists());

        // Yerinde aynı adda dosya varken geri yükleme üzerine yazmaz
        assert!(geri_yukle(&cop.join("dilekce (2).txt")).is_err());
        assert_eq!(fs::read_to_string(&dosya).unwrap(), "bir");

        kalici_sil(&cop.join("dilekce (2).txt")).unwrap();
        assert!(!cop.join("dilekce (2).txt.konum").exists());

        fs::write(belgeler.join("x.txt"), "x").unwrap();
        cop_icine_tasi(&cop, &belgeler.join("x.txt")).unwrap();
        cop_bosalt(&cop).unwrap();
        assert!(listele(&cop).unwrap().is_empty());
        fs::remove_dir_all(&gecici).unwrap();
    }

    #[test]
    fn gecersiz_ad_reddedilir() {
        let gecici = std::env::temp_dir();
        assert!(yeni_klasor(&gecici, "a/b").is_err());
        assert!(yeni_klasor(&gecici, "").is_err());
    }
}
