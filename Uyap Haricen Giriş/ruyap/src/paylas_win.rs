//! Paylaş = dosyayı panoya kopyala (kullanıcı isteği 17.11.2026: "pencere açmadan, tek tıkla").
//!
//! Sayfa belgeyi `/__uyap_yerel__/paylas?kip=pano` ucuna verir; program dosyayı bu bilgisayarda geçici
//! klasöre yazar ve dosya olarak panoya koyar. Belge hiçbir sunucuya gitmez. Windows paylaşım penceresini
//! açan eski yol 17.11.2026'da kaldırıldı (kullanılmıyordu).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Dosya adını Windows için güvenli yapar (uzantı korunur).
pub fn guvenli_ad(ad: &str) -> String {
    let t: String = ad
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_matches(|c| c == '.' || c == ' ').to_string();
    let t: String = t.chars().take(120).collect();
    if t.is_empty() { "belge".to_string() } else { t }
}

/// Tek tıkla paylaşım (kullanıcı isteği 17.11.2026: "pencere açmadan, tek tıkla"): belge geçici klasöre
/// yazılır ve DOSYA OLARAK panoya konur (Gezgin'de "Kopyala" ile aynı: CF_HDROP + Preferred DropEffect=kopya).
/// Kullanıcı WhatsApp, Outlook, Telegram, klasör … nereye isterse Ctrl+V ile yapıştırır. Hiç pencere açılmaz.
/// Dosya panodan geç yapıştırılabileceği için ayrı klasörde 24 saat tutulur.
pub fn pano_klasoru() -> PathBuf {
    std::env::temp_dir().join("CokluUyap").join("pano")
}

pub fn panoya_kopyala(ad: &str, veri: &[u8]) -> Result<PathBuf, String> {
    let k = pano_klasoru();
    if let Ok(rd) = std::fs::read_dir(&k) {
        for e in rd.flatten() {
            let eski = e.metadata().and_then(|m| m.modified()).ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .map(|d| d > Duration::from_secs(24 * 3600)).unwrap_or(false);
            if eski { let _ = std::fs::remove_dir_all(e.path()); }
        }
    }
    let alt = k.join(format!(
        "{}",
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&alt).map_err(|e| format!("Geçici klasör açılamadı: {e}"))?;
    let yol = alt.join(guvenli_ad(ad));
    std::fs::write(&yol, veri).map_err(|e| format!("Geçici dosya yazılamadı: {e}"))?;
    dosyayi_panoya_koy(&yol)?;
    Ok(yol)
}

/// `.udf` uzantısına dosya türü (`Content Type`) yazar (yalnız bu kullanıcı, HKCU; kullanıcı isteği 23.11.2026).
/// UYAP Editör kaydında tür yok; UDF içeride ZIP olduğundan WhatsApp panodan yapıştırılan UDF'yi ZIP sanıp adına
/// ".zip" ekliyordu ("x.udf.zip"). Var olan değer (başka bir program yazmışsa) ve dosya ilişkilendirmesi (hangi
/// programla açıldığı) değiştirilmez. Açılışta arka planda çalışır; hata sessizce yutulur.
pub fn udf_turunu_kaydet() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const PENCERESIZ: u32 = 0x0800_0000; // CREATE_NO_WINDOW
        const ANAHTAR: &str = r"HKCU\Software\Classes\.udf";
        let var = Command::new("reg")
            .args(["query", ANAHTAR, "/v", "Content Type"])
            .creation_flags(PENCERESIZ)
            .output()
            .is_ok_and(|o| o.status.success());
        if var {
            return;
        }
        let tamam = Command::new("reg")
            .args(["add", ANAHTAR, "/v", "Content Type", "/t", "REG_SZ", "/d", "application/x-udf", "/f"])
            .creation_flags(PENCERESIZ)
            .output()
            .is_ok_and(|o| o.status.success());
        println!("[SİSTEM] .udf dosya türü kaydı {}.", if tamam { "yazıldı (WhatsApp \".udf.zip\" düzeltmesi)" } else { "yazılamadı" });
    }
}

#[cfg(windows)]
fn dosyayi_panoya_koy(yol: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::w;
    use windows::Win32::Foundation::{BOOL, HANDLE, HWND, POINT};
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
    use windows::Win32::UI::Shell::DROPFILES;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE};
    const CF_HDROP: u32 = 15;

    // Çift sıfırla biten UTF-16 yol listesi (tek dosya).
    let mut liste: Vec<u16> = yol.as_os_str().encode_wide().collect();
    liste.push(0);
    liste.push(0);
    unsafe {
        let bas = std::mem::size_of::<DROPFILES>();
        let hdrop = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bas + liste.len() * 2).map_err(|e| e.to_string())?;
        let p = GlobalLock(hdrop) as *mut u8;
        if p.is_null() { return Err("Bellek ayrılamadı.".into()); }
        std::ptr::write(p as *mut DROPFILES, DROPFILES { pFiles: bas as u32, pt: POINT::default(), fNC: BOOL(0), fWide: BOOL(1) });
        std::ptr::copy_nonoverlapping(liste.as_ptr() as *const u8, p.add(bas), liste.len() * 2);
        let _ = GlobalUnlock(hdrop);

        let hefekt = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, 4).map_err(|e| e.to_string())?;
        let pe = GlobalLock(hefekt) as *mut u32;
        if !pe.is_null() { *pe = 1; } // DROPEFFECT_COPY: Gezgin'e yapıştırınca taşımasın, kopyalasın
        let _ = GlobalUnlock(hefekt);
        let fmt_efekt = RegisterClipboardFormatW(w!("Preferred DropEffect"));

        // Panonun sahibi olacak görünmez ileti penceresi (ekranda hiçbir şey açılmaz).
        let sahip = CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, HWND_MESSAGE, None, None, None)
            .unwrap_or(HWND::default());
        // Pano başka programda açıksa kısa aralıklarla yeniden dene.
        let mut acildi = false;
        for _ in 0..20 {
            if OpenClipboard(sahip).is_ok() { acildi = true; break; }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !acildi {
            if !sahip.is_invalid() { let _ = DestroyWindow(sahip); }
            return Err("Pano başka bir programda meşgul; bir kez daha deneyin.".into());
        }
        let _ = EmptyClipboard();
        let r = SetClipboardData(CF_HDROP, HANDLE(hdrop.0));
        if fmt_efekt != 0 { let _ = SetClipboardData(fmt_efekt, HANDLE(hefekt.0)); }
        let _ = CloseClipboard();
        if !sahip.is_invalid() { let _ = DestroyWindow(sahip); }
        r.map(|_| ()).map_err(|e| format!("Belge panoya konamadı: {e}"))
    }
}

#[cfg(not(windows))]
fn dosyayi_panoya_koy(_yol: &Path) -> Result<(), String> {
    Err("Bu işletim sisteminde dosya panoya konamıyor.".into())
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    #[ignore]
    fn pano_dene() {
        // Panoya "Taşınmaz deneme.udf" koyar (kullanıcının panosunu ezer; `--include-ignored` toplu koşuda çalışmasın):
        // PANO_DENE=1 cargo test --release --lib paylas_win::tests::pano_dene -- --ignored
        if std::env::var_os("PANO_DENE").is_none() {
            return;
        }
        let yol = super::panoya_kopyala("Taşınmaz deneme.udf", b"deneme").unwrap();
        eprintln!("[pano] {:?}", yol);
    }
}
