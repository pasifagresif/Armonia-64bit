//! Armonia dock bekçisi (kullanıcı isteği 26.11: "pencere açılınca dock aşağı insin, ekranı daraltmasın").
//!
//! Dock'un kendisi yine waybar'dır (dock.jsonc); bu program onu ÇOCUK SÜREÇ olarak yönetir:
//!   * açık (küçültülmemiş) pencere yoksa dock görünür,
//!   * pencere varsa dock gizlenir (waybar süreci durdurulur),
//!   * fare ekranın alt kenarına değince dock geri gelir, fare dock bölgesinden çıkınca yeniden gizlenir.
//! "Görünür" = waybar çocuğu çalışıyor; ayrı bir durum tutulmaz, bu yüzden tema değişimi (waybar yeniden
//! yüklenir) ya da waybar çökmesi durumu bozamaz.
//!
//! Wayland tarafı: pencere listesi `wlr-foreign-toplevel-management` ile; alt kenar için 2 piksellik saydam
//! bir katman yüzeyi ("kenar", overlay); dock açıkken farenin dock'tan ayrıldığını anlamak için pencerelerin
//! üstünde, dock'un altında tam ekran saydam bir yüzey ("gözcü", top katmanı — yalnız dock açıkken vardır).
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use wayland_client::{
    backend::ObjectId,
    delegate_noop, event_created_child,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

/// Alt kenardaki tetik şeridinin yüksekliği (piksel). İnce: pencerelerin en alt satırını kapatmasın.
const KENAR_YUKSEKLIGI: u32 = 2;
/// Dock bölgesi: ekranın altından bu kadar piksel içindeyken fare "dock'ta" sayılır (dock 86 + boşluk 8 + pay).
const DOCK_BOLGESI: f64 = 120.0;
/// Waybar üst üste çökerse yeniden başlatmalar arasındaki en kısa süre.
const YENIDEN_BASLATMA_ARASI: Duration = Duration::from_secs(2);
/// Olay yokken çocuk sürecin denetlenme aralığı (milisaniye).
const YOKLAMA_MS: i32 = 500;

/// 1x1 saydam tamponu yüzeyin tamamına yayan katman yüzeyi.
struct Yuzey {
    katman: LayerSurface,
    gorunum: WpViewport,
    yukseklik: u32,
}

struct Bekci {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    havuz: SlotPool,
    gorunumcu: WpViewporter,
    isaretci: Option<wl_pointer::WlPointer>,

    kenar: Option<Yuzey>,
    gozcu: Option<Yuzey>,
    /// Pencere -> küçültülmüş mü
    pencereler: HashMap<ObjectId, bool>,
    /// Fare alt kenardan dock'u çağırdı ve henüz dock bölgesinden çıkmadı
    cagrildi: bool,

    waybar: Vec<String>,
    cocuk: Option<Child>,
    son_baslatma: Option<Instant>,
    /// Çocuk kendiliğinden kapandı (çökme): hemen değil, kısa bir aradan sonra yeniden başlat
    cokme_sonrasi: bool,
}

impl Bekci {
    fn pencere_var(&self) -> bool {
        self.pencereler.values().any(|kucuk| !kucuk)
    }

    /// Durumdan dock'un görünürlüğünü ve gözcü yüzeyinin varlığını türetir.
    fn karar(&mut self, qh: &QueueHandle<Self>) {
        let pencere = self.pencere_var();
        if !pencere {
            self.cagrildi = false;
        }
        let gorunur = !pencere || self.cagrildi;
        if gorunur != self.cocuk.is_some() {
            // Günlük (stderr): autostart /dev/null'a yollar; elle çalıştırınca teşhis için görünür
            eprintln!(
                "armonia-dock: {} (pencere: {}, alt kenardan çağrıldı: {})",
                if gorunur { "göster" } else { "gizle" },
                self.pencereler.len(),
                self.cagrildi
            );
        }
        if gorunur {
            self.goster();
        } else {
            self.gizle();
        }
        if pencere && self.cagrildi {
            if self.gozcu.is_none() {
                self.gozcu = Some(self.yuzey_olustur(qh, Layer::Top, "armonia-dock-gozcu", Anchor::all(), 0));
            }
        } else {
            self.gozcu = None;
        }
    }

    fn yuzey_olustur(
        &self,
        qh: &QueueHandle<Self>,
        katman: Layer,
        ad: &str,
        capa: Anchor,
        yukseklik: u32,
    ) -> Yuzey {
        let yuzey = self.compositor.create_surface(qh);
        let gorunum = self.gorunumcu.get_viewport(&yuzey, qh, ());
        let k = self
            .layer_shell
            .create_layer_surface(qh, yuzey, katman, Some(ad), None);
        k.set_anchor(capa);
        k.set_size(0, yukseklik);
        k.set_exclusive_zone(-1); // diğer çubukların ayırdığı alanı yok say: gerçek ekran kenarına otur
        k.set_keyboard_interactivity(KeyboardInteractivity::None);
        k.commit();
        Yuzey {
            katman: k,
            gorunum,
            yukseklik: 0,
        }
    }

    fn goster(&mut self) {
        if self.cocuk.is_some() {
            return;
        }
        if let Some(son) = self.son_baslatma {
            if son.elapsed() < YENIDEN_BASLATMA_ARASI && self.cokme_sonrasi {
                return;
            }
        }
        let mut komut = Command::new(&self.waybar[0]);
        komut
            .args(&self.waybar[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: fork ile exec arasında yalnız async-signal-safe prctl çağrılır. Bekçi ölürse waybar da
        // ölsün (yetim dock kalmasın; autostart döngüsü bekçiyi yeniden başlatınca çift dock olmasın).
        unsafe {
            komut.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        match komut.spawn() {
            Ok(c) => self.cocuk = Some(c),
            Err(e) => eprintln!("armonia-dock: waybar başlatılamadı: {e}"),
        }
        self.son_baslatma = Some(Instant::now());
        self.cokme_sonrasi = false;
    }

    fn gizle(&mut self) {
        if let Some(mut c) = self.cocuk.take() {
            // SAFETY: kendi çocuğumuzun pid'i; henüz wait edilmedi, pid yeniden kullanılmış olamaz.
            unsafe {
                libc::kill(c.id() as libc::pid_t, libc::SIGTERM);
            }
            let _ = c.wait();
        }
    }

    /// Waybar kendiliğinden kapandıysa (çökme) fark eder; gerekiyorsa `karar` yeniden başlatır.
    fn cocugu_denetle(&mut self, qh: &QueueHandle<Self>) {
        if let Some(c) = self.cocuk.as_mut() {
            if matches!(c.try_wait(), Ok(Some(_)) | Err(_)) {
                self.cocuk = None;
                self.cokme_sonrasi = true;
            }
        }
        self.karar(qh);
    }

    fn ciz(&mut self, hangi: Hangi, genislik: u32, yukseklik: u32) {
        let Ok((tampon, tuval)) = self.havuz.create_buffer(1, 1, 4, wl_shm::Format::Argb8888) else {
            return;
        };
        tuval.fill(0);
        let hedef = match hangi {
            Hangi::Kenar => self.kenar.as_mut(),
            Hangi::Gozcu => self.gozcu.as_mut(),
        };
        let Some(y) = hedef else { return };
        y.yukseklik = yukseklik;
        let s = y.katman.wl_surface();
        if tampon.attach_to(s).is_err() {
            return;
        }
        y.gorunum.set_destination(genislik.max(1) as i32, yukseklik.max(1) as i32);
        s.damage_buffer(0, 0, 1, 1);
        y.katman.commit();
    }

    fn hangi(&self, yuzey: &wl_surface::WlSurface) -> Option<Hangi> {
        if self.kenar.as_ref().is_some_and(|y| y.katman.wl_surface() == yuzey) {
            Some(Hangi::Kenar)
        } else if self.gozcu.as_ref().is_some_and(|y| y.katman.wl_surface() == yuzey) {
            Some(Hangi::Gozcu)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Hangi {
    Kenar,
    Gozcu,
}

impl Drop for Yuzey {
    fn drop(&mut self) {
        self.gorunum.destroy();
    }
}

impl LayerShellHandler for Bekci {
    fn closed(&mut self, _: &Connection, qh: &QueueHandle<Self>, katman: &LayerSurface) {
        // Ekran çıkarıldı/değişti: yüzeyi bırak, kenar şeridini yeniden kur
        match self.hangi(katman.wl_surface()) {
            Some(Hangi::Kenar) => {
                self.kenar = Some(self.yuzey_olustur(
                    qh,
                    Layer::Overlay,
                    "armonia-dock-kenar",
                    Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                    KENAR_YUKSEKLIGI,
                ));
            }
            Some(Hangi::Gozcu) => {
                self.cagrildi = false;
                self.karar(qh);
            }
            None => {}
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        katman: &LayerSurface,
        yapi: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(h) = self.hangi(katman.wl_surface()) {
            self.ciz(h, yapi.new_size.0, yapi.new_size.1);
        }
    }
}

impl PointerHandler for Bekci {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        olaylar: &[PointerEvent],
    ) {
        for olay in olaylar {
            if !matches!(olay.kind, PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. }) {
                continue;
            }
            match self.hangi(&olay.surface) {
                Some(Hangi::Kenar) => {
                    if self.pencere_var() && !self.cagrildi {
                        self.cagrildi = true;
                        self.karar(qh);
                    }
                }
                Some(Hangi::Gozcu) => {
                    let yukseklik = self.gozcu.as_ref().map_or(0, |y| y.yukseklik) as f64;
                    // Yükseklik henüz bilinmiyorsa (ilk configure gelmedi) karar verme
                    if yukseklik > 0.0 && olay.position.1 < yukseklik - DOCK_BOLGESI {
                        self.cagrildi = false;
                        self.karar(qh);
                    }
                }
                None => {}
            }
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Bekci {
    fn event(
        durum: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        olay: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = olay {
            durum.pencereler.insert(toplevel.id(), false);
        }
    }

    event_created_child!(Bekci, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Bekci {
    fn event(
        durum: &mut Self,
        tutamac: &ZwlrForeignToplevelHandleV1,
        olay: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::{Event, State};
        match olay {
            Event::State { state } => {
                let kucuk = state
                    .chunks_exact(4)
                    .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                    .any(|d| d == State::Minimized as u32);
                durum.pencereler.insert(tutamac.id(), kucuk);
            }
            Event::Done => durum.karar(qh),
            Event::Closed => {
                durum.pencereler.remove(&tutamac.id());
                tutamac.destroy();
                durum.karar(qh);
            }
            _ => {}
        }
    }
}

delegate_noop!(Bekci: ignore WpViewporter);
delegate_noop!(Bekci: ignore WpViewport);

impl CompositorHandler for Bekci {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Bekci {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for Bekci {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, koltuk: wl_seat::WlSeat, yetenek: Capability) {
        if yetenek == Capability::Pointer && self.isaretci.is_none() {
            self.isaretci = self.seat_state.get_pointer(qh, &koltuk).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, yetenek: Capability) {
        if yetenek == Capability::Pointer {
            if let Some(i) = self.isaretci.take() {
                i.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl ShmHandler for Bekci {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Bekci {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Bekci);
delegate_output!(Bekci);
delegate_shm!(Bekci);
delegate_seat!(Bekci);
delegate_pointer!(Bekci);
delegate_layer!(Bekci);
delegate_registry!(Bekci);

fn calistir() -> Result<(), Box<dyn std::error::Error>> {
    // Bağımsız değişkenler: çalıştırılacak dock komutu. Verilmezse Armonia'nın waybar dock'u.
    let mut waybar: Vec<String> = std::env::args().skip(1).collect();
    if waybar.is_empty() {
        waybar = ["waybar", "-c", "/etc/armonia/waybar/dock.jsonc", "-s", "/etc/armonia/waybar/dock.css"]
            .map(String::from)
            .to_vec();
    }

    let baglanti = Connection::connect_to_env()?;
    let (globaller, mut kuyruk) = registry_queue_init::<Bekci>(&baglanti)?;
    let qh = kuyruk.handle();

    let compositor = CompositorState::bind(&globaller, &qh)?;
    let layer_shell = LayerShell::bind(&globaller, &qh)?;
    let shm = Shm::bind(&globaller, &qh)?;
    let havuz = SlotPool::new(64, &shm)?;
    let gorunumcu: WpViewporter = globaller.bind(&qh, 1..=1, ())?;
    let _pencere_yoneticisi: ZwlrForeignToplevelManagerV1 = globaller.bind(&qh, 1..=3, ())?;

    let mut bekci = Bekci {
        registry_state: RegistryState::new(&globaller),
        seat_state: SeatState::new(&globaller, &qh),
        output_state: OutputState::new(&globaller, &qh),
        compositor,
        layer_shell,
        shm,
        havuz,
        gorunumcu,
        isaretci: None,
        kenar: None,
        gozcu: None,
        pencereler: HashMap::new(),
        cagrildi: false,
        waybar,
        cocuk: None,
        son_baslatma: None,
        cokme_sonrasi: false,
    };
    bekci.kenar = Some(bekci.yuzey_olustur(
        &qh,
        Layer::Overlay,
        "armonia-dock-kenar",
        Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        KENAR_YUKSEKLIGI,
    ));
    // İlk tur: mevcut pencereler öğrenilsin, sonra ilk karar verilsin (boş masaüstünde dock hemen gelir)
    kuyruk.roundtrip(&mut bekci)?;
    kuyruk.roundtrip(&mut bekci)?;

    let sonuc = (|| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            bekci.cocugu_denetle(&qh);
            kuyruk.dispatch_pending(&mut bekci)?;
            kuyruk.flush()?;
            let Some(okuma) = kuyruk.prepare_read() else {
                continue;
            };
            let mut p = libc::pollfd {
                fd: okuma.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: geçerli tek elemanlı pollfd dizisi.
            let n = unsafe { libc::poll(&mut p, 1, YOKLAMA_MS) };
            if n > 0 {
                match okuma.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
    })();
    bekci.gizle();
    sonuc
}

fn main() {
    if let Err(e) = calistir() {
        eprintln!("armonia-dock: {e}");
        std::process::exit(1);
    }
}
