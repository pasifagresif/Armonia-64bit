//! Sol paneldeki klasör ağacı (yalnız dizinler). ÇokluUYAP'ın belge klasörleri (Yargı Türü/Birim/Dosya No)
//! zaten gerçek iç içe dizinler olduğu için burada özel bir mantık yok — düz bir ağaç, UYAP Kasası'nı da
//! diğer klasörler gibi gösterir.
use crate::islemler::listele;
use egui::{CollapsingHeader, Ui};
use std::path::{Path, PathBuf};

pub fn dugum(
    ui: &mut Ui,
    yol: &Path,
    ad: &str,
    secili: &mut PathBuf,
    gezinilecek: &mut Option<PathBuf>,
) {
    let id = ui.make_persistent_id(yol);
    let vurgulu = secili == yol;
    let baslik = if vurgulu {
        format!("▸ {ad}")
    } else {
        ad.to_string()
    };
    let acilim = CollapsingHeader::new(baslik).id_salt(id).show(ui, |ui| {
        let Ok(girdiler) = listele(yol) else { return };
        for girdi in girdiler.iter().filter(|g| g.dizin) {
            dugum(ui, &girdi.yol, &girdi.ad, secili, gezinilecek);
        }
    });
    if acilim.header_response.clicked() {
        *gezinilecek = Some(yol.to_path_buf());
    }
}

/// Sabit kısayollar (Ev, Belgeler, İndirilenler, UYAP Kasası varsa, bağlı USB'ler).
/// Her etikette bir simge (emoji) öne eklenir — listedeki girdilerin diğer dosya/klasör satırlarıyla
/// (📁/📄, bkz. main.rs) aynı tarzda ayırt edilmesi kullanıcı isteği (25.11).
pub fn kisayollar() -> Vec<(String, PathBuf)> {
    let ev = std::env::var("HOME").unwrap_or_else(|_| "/home/armonia".to_string());
    let ev = PathBuf::from(ev);
    let mut liste = vec![("🏠 Ev".to_string(), ev.clone())];
    for (ad, alt, simge) in [("Belgeler", "Belgeler", "📁"), ("İndirilenler", "İndirilenler", "📁")] {
        let yol = ev.join(alt);
        if yol.is_dir() {
            liste.push((format!("{simge} {ad}"), yol));
        }
    }
    let uyap = ev.join("Belgeler").join("UYAP");
    if uyap.is_dir() {
        liste.push(("⚖ UYAP Kasası".to_string(), uyap));
    }
    if let Ok(cop) = crate::islemler::cop_kutusu_dizini() {
        liste.push(("🗑 Çöp Kutusu".to_string(), cop));
    }
    for kok in ["/media", "/run/media"] {
        let Ok(ust) = std::fs::read_dir(kok) else {
            continue;
        };
        for girdi in ust.flatten() {
            if !girdi.path().is_dir() {
                continue;
            }
            // /run/media/<kullanici>/<aygit> iki seviyeli; /media/<aygit> tek seviyeli
            if kok == "/run/media" {
                let Ok(alt) = std::fs::read_dir(girdi.path()) else {
                    continue;
                };
                for a in alt.flatten() {
                    if a.path().is_dir() {
                        liste.push((format!("💾 {}", a.file_name().to_string_lossy()), a.path()));
                    }
                }
            } else {
                liste.push((
                    format!("💾 {}", girdi.file_name().to_string_lossy()),
                    girdi.path(),
                ));
            }
        }
    }
    liste
}
