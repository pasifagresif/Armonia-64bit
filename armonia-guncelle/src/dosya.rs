//! Dosya yardımcıları: sembolik bağ izlemeden açma, sınırlı okuma, atomik yazma.

use crate::{hata, Sonuc};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

/// Sembolik bağ İZLEMEDEN okumak için açar; düz dosya değilse reddeder.
pub fn ac_bagsiz(yol: &Path) -> Sonuc<File> {
    #[cfg(unix)]
    let f = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(yol)
            .map_err(|e| hata!("{} açılamadı: {e}", yol.display()))?
    };
    #[cfg(not(unix))]
    let f = {
        let m = std::fs::symlink_metadata(yol).map_err(|e| hata!("{} açılamadı: {e}", yol.display()))?;
        if m.file_type().is_symlink() {
            return Err(hata!("{} sembolik bağ", yol.display()));
        }
        OpenOptions::new().read(true).open(yol).map_err(|e| hata!("{} açılamadı: {e}", yol.display()))?
    };
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(hata!("{} düz dosya değil", yol.display()));
    }
    Ok(f)
}

/// En çok `azami` bayt okur; daha büyükse reddeder.
pub fn oku_sinirli(yol: &Path, azami: u64) -> Sonuc<Vec<u8>> {
    let f = ac_bagsiz(yol)?;
    let mut v = Vec::new();
    f.take(azami + 1).read_to_end(&mut v)?;
    if v.len() as u64 > azami {
        return Err(hata!("{} çok büyük (sınır {azami} bayt)", yol.display()));
    }
    Ok(v)
}

/// Dosya varsa sınırlı okur; yoksa None.
pub fn oku_varsa(yol: &Path, azami: u64) -> Sonuc<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(yol) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(hata!("{}: {e}", yol.display())),
        Ok(_) => oku_sinirli(yol, azami).map(Some),
    }
}

/// Aynı dizinde geçici dosyaya yazar, fsync, rename; ardından dizini fsync eder.
pub fn atomik_yaz(yol: &Path, veri: &[u8], kip: u32) -> Sonuc<()> {
    let dizin = yol.parent().ok_or_else(|| hata!("{} üst dizini yok", yol.display()))?;
    let ad = yol.file_name().and_then(|a| a.to_str()).ok_or_else(|| hata!("geçersiz ad"))?;
    let gecici = dizin.join(format!(".{ad}.{}.yeni", std::process::id()));
    let _ = std::fs::remove_file(&gecici);
    {
        let mut o = OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(kip).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        #[cfg(not(unix))]
        let _ = kip;
        let mut f = o.open(&gecici)?;
        f.write_all(veri)?;
        f.sync_all()?;
    }
    std::fs::rename(&gecici, yol)?;
    dizin_esitle(dizin);
    Ok(())
}

/// Dizin girdilerini diske yazar (unix; başka yerde yok sayılır).
pub fn dizin_esitle(dizin: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dizin) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dizin;
}
