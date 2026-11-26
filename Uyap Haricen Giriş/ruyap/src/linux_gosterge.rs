//! Linux'ta tepsi simgesi yok (kum havuzunda D-Bus kapalı): Kontrol Ekranı'nda × ve "–" pencereyi dock'a
//! küçültür (labwc destekliyor), program çalışmaya devam eder. Geri getirme: dock'taki pencere simgesi ya da
//! "Çoklu UYAP" kısayolu — kısayol "zaten açık" durumunda SIGUSR1 gönderir (cokluuyap_ac.sh, systemctl kill;
//! polkit kuralı "kill" fiiline izin verir). Sinyal işleyicisi yalnız bir bayrak kaldırır; ayrı bir iş
//! parçacığı bayrağı yoklayıp pencereyi geri açar (`Context` her iş parçacığından kullanılabilir).
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

static GOSTER_ISTEGI: AtomicBool = AtomicBool::new(false);
static BAGLAM: OnceLock<egui::Context> = OnceLock::new();

extern "C" fn sinyal_geldi(_: libc::c_int) {
    GOSTER_ISTEGI.store(true, Ordering::SeqCst);
}

/// SIGUSR1 işleyicisini ve yoklama iş parçacığını kurar; eframe penceresi oluşturulduğunda bir kez çağrılır.
pub fn kur(ctx: egui::Context) {
    let _ = BAGLAM.set(ctx);
    // SAFETY: geçerli sinyal numarası; işleyici yalnız atomik bir yazma yapar (sinyal-güvenli).
    unsafe {
        libc::signal(libc::SIGUSR1, sinyal_geldi as libc::sighandler_t);
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(150));
        if GOSTER_ISTEGI.swap(false, Ordering::SeqCst) {
            if let Some(ctx) = BAGLAM.get() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
            }
        }
    });
}
