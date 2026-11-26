//! Başlığı verilen pencereyi öne getirir (küçültülmüşse geri açar): `armonia-one-getir "Armonia Yazı"`.
//!
//! Neden var: Wayland'da (labwc) bir uygulama arkadayken kendi penceresini öne alamaz. Armonia Yazı açıkken
//! başka bir belgeye çift tıklanınca belge çalışan pencereye sekme olarak devredilir; pencere arkada kalırsa
//! kullanıcı hiçbir şey olmadığını sanar. Masaüstü tarafındaki başlatıcı (yazi_ac.sh) bu programı çağırır;
//! istek dock'un da kullandığı `wlr-foreign-toplevel-management` ile yapılır (görev çubuğu tıklamasıyla aynı).
//! Çıkış kodu: 0 = pencere bulundu ve öne alındı, 1 = böyle pencere yok, 2 = hata.
use std::collections::HashMap;
use wayland_client::{
    backend::ObjectId,
    delegate_noop, event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat::WlSeat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

#[derive(Default)]
struct Durum {
    /// Pencere -> (tutamaç, başlık)
    pencereler: HashMap<ObjectId, (ZwlrForeignToplevelHandleV1, String)>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Durum {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Durum {
    fn event(
        durum: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        olay: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = olay {
            durum.pencereler.insert(toplevel.id(), (toplevel, String::new()));
        }
    }

    event_created_child!(Durum, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Durum {
    fn event(
        durum: &mut Self,
        tutamac: &ZwlrForeignToplevelHandleV1,
        olay: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        match olay {
            Event::Title { title } => {
                if let Some(p) = durum.pencereler.get_mut(&tutamac.id()) {
                    p.1 = title;
                }
            }
            Event::Closed => {
                durum.pencereler.remove(&tutamac.id());
            }
            _ => {}
        }
    }
}

delegate_noop!(Durum: ignore WlSeat);

/// Aranan başlıkla eşleşir mi: tam eşleşme ya da "belge — Armonia Yazı" biçiminde sonda.
fn baslik_uyar(baslik: &str, aranan: &str) -> bool {
    baslik == aranan || baslik.ends_with(&format!(" — {aranan}"))
}

fn calistir(aranan: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let baglanti = Connection::connect_to_env()?;
    let (globaller, mut kuyruk) = registry_queue_init::<Durum>(&baglanti)?;
    let qh = kuyruk.handle();
    let koltuk: WlSeat = globaller.bind(&qh, 1..=1, ())?;
    let _yonetici: ZwlrForeignToplevelManagerV1 = globaller.bind(&qh, 1..=3, ())?;
    let mut durum = Durum::default();
    // İlk tur pencereleri, ikincisi başlıklarını getirir
    kuyruk.roundtrip(&mut durum)?;
    kuyruk.roundtrip(&mut durum)?;
    let mut bulundu = false;
    for (tutamac, baslik) in durum.pencereler.values() {
        if baslik_uyar(baslik, aranan) {
            tutamac.unset_minimized();
            tutamac.activate(&koltuk);
            bulundu = true;
        }
    }
    kuyruk.roundtrip(&mut durum)?;
    Ok(bulundu)
}

fn main() {
    let Some(aranan) = std::env::args().nth(1).filter(|a| !a.is_empty()) else {
        eprintln!("kullanım: armonia-one-getir <pencere başlığı>");
        std::process::exit(2);
    };
    match calistir(&aranan) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("armonia-one-getir: {e}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod testler {
    use super::baslik_uyar;

    #[test]
    fn baslik_tam_ya_da_belge_adli_bicimde_eslesir() {
        assert!(baslik_uyar("Armonia Yazı", "Armonia Yazı"));
        assert!(baslik_uyar("dilekce.udf — Armonia Yazı", "Armonia Yazı"));
        assert!(!baslik_uyar("Armonia Yazı hakkında - Chromium", "Armonia Yazı"));
        assert!(!baslik_uyar("Armonia Dosyalar", "Armonia Yazı"));
    }
}
