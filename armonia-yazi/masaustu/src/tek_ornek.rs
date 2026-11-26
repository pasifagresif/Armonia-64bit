//! Tek pencere (kullanıcı isteği 26.11: belgeler aynı pencerede sekme olarak açılsın).
//!
//! Armonia'da her açma isteği ayrı bir kum havuzlu hizmet örneği başlatır (armonia-yazi@.service). İlk örnek
//! veri klasöründe (bütün örneklere bağlı: BindPaths) bir Unix soketi dinler; sonraki örnekler açılmadan önce
//! belgenin yolunu bu sokete tek satır olarak yazar ve çıkar. İlk örnek belgeyi KENDİ kum havuzunda yeniden
//! denetleyip okur ve arayüze yeni sekme olarak verir (main.rs `devralinani_ac`).
//!
//! Soket yolu `ARMONIA_YAZI_TEK_SOKET` ortam değişkeninden gelir (yazi_calistir.sh); yoksa özellik kapalıdır
//! (geliştirme, Armonia dışı kullanım: her çalıştırma kendi penceresini açar).
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ORTAM: &str = "ARMONIA_YAZI_TEK_SOKET";
const ONAY: &str = "tamam";
const SURE: Duration = Duration::from_secs(3);
/// Bir istek satırının en büyük boyu (PATH_MAX + pay)
const AZAMI_SATIR: u64 = 8192;

pub enum Baslangic {
    /// Çalışan pencereye devredildi: bu örnek pencere açmadan çıkmalı
    Devredildi,
    /// Bu örnek ilk: pencereyi açar ve gelen istekleri dinler
    Dinle(UnixListener),
    /// Özellik kapalı ya da soket kullanılamıyor: eski davranış (kendi penceresi)
    Kapali,
}

/// `belge`: komut satırından gelen dosya (yoksa "yeni belge" isteği devredilir).
pub fn baslat(belge: Option<&Path>) -> Baslangic {
    let Some(soket) = std::env::var_os(ORTAM).map(PathBuf::from) else {
        return Baslangic::Kapali;
    };
    if let Ok(akis) = UnixStream::connect(&soket) {
        return if devret(akis, belge) { Baslangic::Devredildi } else { Baslangic::Kapali };
    }
    // Dinleyen yok: önceki örnekten kalmış soket dosyası olabilir
    let _ = std::fs::remove_file(&soket);
    match UnixListener::bind(&soket) {
        Ok(d) => Baslangic::Dinle(d),
        Err(e) => {
            eprintln!("armonia-yazi: tek pencere soketi açılamadı ({}): {e}", soket.display());
            Baslangic::Kapali
        }
    }
}

fn devret(mut akis: UnixStream, belge: Option<&Path>) -> bool {
    let _ = akis.set_read_timeout(Some(SURE));
    let _ = akis.set_write_timeout(Some(SURE));
    let satir = match belge {
        None => String::new(),
        Some(y) => match std::fs::canonicalize(y).ok().and_then(|t| t.to_str().map(str::to_owned)) {
            Some(t) if !t.contains('\n') => t,
            _ => return false,
        },
    };
    if akis.write_all(format!("{satir}\n").as_bytes()).is_err() {
        return false;
    }
    let mut yanit = String::new();
    // Onay gelmezse (asılı kalmış örnek) belge kaybolmasın: bu örnek kendi penceresini açar
    BufReader::new(akis).read_line(&mut yanit).is_ok() && yanit.trim() == ONAY
}

/// Bir bağlantıdan isteği okur ve onaylar. `Some(None)`: yeni belge; `Some(Some(yol))`: belge.
pub fn istek_oku(akis: UnixStream) -> Option<Option<PathBuf>> {
    let _ = akis.set_read_timeout(Some(SURE));
    let _ = akis.set_write_timeout(Some(SURE));
    let mut satir = String::new();
    BufReader::new((&akis).take(AZAMI_SATIR)).read_line(&mut satir).ok()?;
    if !satir.ends_with('\n') {
        return None; // yarım ya da aşırı uzun istek
    }
    (&akis).write_all(format!("{ONAY}\n").as_bytes()).ok()?;
    let satir = satir.trim_end_matches('\n');
    Some(if satir.is_empty() { None } else { Some(PathBuf::from(satir)) })
}

/// Devralınan yol açılabilir mi: tam yol, gerçek dosya, UDF ya da DOCX. (Hangi klasörlerin görülebildiğini kum
/// havuzu belirler; gönderen tarafta yazi_ac.sh / yazi_calistir.sh da aynı denetimi yapar.)
pub fn kabul_edilir(yol: &Path) -> Result<(), String> {
    if !yol.is_absolute() {
        return Err("dosya yolu geçersiz".into());
    }
    let uzanti = yol.extension().and_then(|u| u.to_str()).map(str::to_lowercase).unwrap_or_default();
    if uzanti != "udf" && uzanti != "docx" {
        return Err("yalnız UDF ve Word (DOCX) belgeleri açılır".into());
    }
    if !yol.is_file() {
        return Err("dosya bulunamadı".into());
    }
    Ok(())
}

#[cfg(test)]
mod testler {
    use super::*;

    fn gecici(ad: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("armonia-yazi-tek-{}-{ad}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn yalniz_var_olan_udf_ve_docx_kabul_edilir() {
        let d = gecici("kabul");
        let udf = d.join("dilekce.UDF");
        std::fs::write(&udf, b"x").unwrap();
        std::fs::write(d.join("not.txt"), b"x").unwrap();
        assert!(kabul_edilir(&udf).is_ok());
        assert!(kabul_edilir(&d.join("not.txt")).is_err());
        assert!(kabul_edilir(&d.join("yok.udf")).is_err());
        assert!(kabul_edilir(Path::new("goreli/dilekce.udf")).is_err());
        assert!(kabul_edilir(&d).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn belge_ve_yeni_belge_istegi_devredilir_ve_onaylanir() {
        let d = gecici("devir");
        let soket = d.join("tek.sock");
        let belge = d.join("a.udf");
        std::fs::write(&belge, b"x").unwrap();
        let dinleyici = UnixListener::bind(&soket).unwrap();
        let alici = std::thread::spawn(move || {
            let ilk = istek_oku(dinleyici.accept().unwrap().0);
            let ikinci = istek_oku(dinleyici.accept().unwrap().0);
            (ilk, ikinci)
        });
        assert!(devret(UnixStream::connect(&soket).unwrap(), Some(&belge)));
        assert!(devret(UnixStream::connect(&soket).unwrap(), None));
        let (ilk, ikinci) = alici.join().unwrap();
        assert_eq!(ilk, Some(Some(std::fs::canonicalize(&belge).unwrap())));
        assert_eq!(ikinci, Some(None));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn onay_gelmezse_devredilmis_sayilmaz() {
        let d = gecici("onaysiz");
        let soket = d.join("tek.sock");
        let dinleyici = UnixListener::bind(&soket).unwrap();
        let alici = std::thread::spawn(move || drop(dinleyici.accept().unwrap().0));
        assert!(!devret(UnixStream::connect(&soket).unwrap(), None));
        alici.join().unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }
}
