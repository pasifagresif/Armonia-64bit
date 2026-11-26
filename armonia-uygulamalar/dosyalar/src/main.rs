//! Armonia Dosyalar: temel dosya yöneticisi (Thunar'ın yerine, kullanıcının temaya uygun arayüzü).
//! Sol panelde klasör ağacı (ÇokluUYAP'ın Yargı Türü/Birim/Dosya No hiyerarşisi dahil — gerçek iç içe
//! dizinler olduğu için ağaç bunu doğal olarak gösterir), sağda seçili klasörün içeriği. Claude, 2026-11-25.
//! 26.11 (kullanıcı isteği): sekmeler (tek pencerede birden çok klasör), dosya işlemleri yalnız sağ tuş
//! menüsünde (araç çubuğundaki Kopyala/Kes/Yapıştır/Yeniden Adlandır/Sil kaldırıldı), Çöp Kutusu görünümü.
mod ac;
mod agac;
mod islemler;

use eframe::egui;
use islemler::Girdi;
use std::path::{Path, PathBuf};

/// Tek pencerede açık klasörlerden biri.
struct Sekme {
    simdiki: PathBuf,
    girdiler: Vec<Girdi>,
    secili: Option<PathBuf>,
}

/// Liste çizilirken toplanıp döngüden sonra uygulanan eylem (çizim sırasında listeyi değiştirmemek için).
enum Eylem {
    Git(PathBuf),
    YeniSekme(PathBuf),
    GeriYukle(PathBuf),
}

struct Dosyalar {
    kisayollar: Vec<(String, PathBuf)>,
    ev: PathBuf,
    cop: Option<PathBuf>,
    sekmeler: Vec<Sekme>,
    etkin: usize,
    pano: Option<(PathBuf, bool)>, // (yol, kesildi mi)
    mesaj: String,
    yeni_klasor_acik: bool,
    yeni_klasor_adi: String,
    yeniden_adlandir: Option<(PathBuf, String)>,
    silme_onayi: Option<PathBuf>,
    kalici_silme_onayi: Option<PathBuf>,
    bosaltma_onayi: bool,
}

impl Dosyalar {
    fn yeni() -> Self {
        let ev = PathBuf::from(
            std::env::var("HOME").unwrap_or_else(|_| "/home/armonia".to_string()),
        );
        let cop = islemler::cop_kutusu_dizini().ok();
        // İlk bağımsız değişken: bir klasör (masaüstü simgeleri ve sağ tuş menüsü böyle çağırır) ya da
        // "--cop" (masaüstündeki Çöp Kutusu simgesi).
        let baslangic = match std::env::args_os().nth(1) {
            Some(a) if a == "--cop" => cop.clone(),
            Some(a) => Some(PathBuf::from(a)).filter(|y| y.is_dir()),
            None => None,
        }
        .unwrap_or_else(|| ev.clone());
        let mut uyg = Self {
            kisayollar: agac::kisayollar(),
            ev,
            cop,
            sekmeler: vec![Sekme {
                simdiki: baslangic,
                girdiler: Vec::new(),
                secili: None,
            }],
            etkin: 0,
            pano: None,
            mesaj: String::new(),
            yeni_klasor_acik: false,
            yeni_klasor_adi: String::new(),
            yeniden_adlandir: None,
            silme_onayi: None,
            kalici_silme_onayi: None,
            bosaltma_onayi: false,
        };
        uyg.yenile();
        uyg
    }

    fn sekme(&self) -> &Sekme {
        &self.sekmeler[self.etkin]
    }

    fn sekme_mut(&mut self) -> &mut Sekme {
        &mut self.sekmeler[self.etkin]
    }

    fn cop_mu(&self, yol: &Path) -> bool {
        self.cop.as_deref() == Some(yol)
    }

    fn sekme_adi(&self, yol: &Path) -> String {
        if self.cop_mu(yol) {
            return "🗑 Çöp Kutusu".to_string();
        }
        if yol == self.ev {
            return "🏠 Ev".to_string();
        }
        let ad = yol
            .file_name()
            .map(|a| a.to_string_lossy().into_owned())
            .unwrap_or_else(|| yol.to_string_lossy().into_owned());
        format!("📁 {ad}")
    }

    fn git(&mut self, yol: PathBuf) {
        let s = self.sekme_mut();
        s.simdiki = yol;
        s.secili = None;
        self.yenile();
    }

    fn yeni_sekme(&mut self, yol: PathBuf) {
        self.sekmeler.push(Sekme {
            simdiki: yol,
            girdiler: Vec::new(),
            secili: None,
        });
        self.etkin = self.sekmeler.len() - 1;
        self.yenile();
    }

    fn sekme_kapat(&mut self, sira: usize) {
        if self.sekmeler.len() < 2 || sira >= self.sekmeler.len() {
            return;
        }
        self.sekmeler.remove(sira);
        if self.etkin > sira || self.etkin >= self.sekmeler.len() {
            self.etkin = self.etkin.saturating_sub(1);
        }
        self.yenile();
    }

    fn sekme_sec(&mut self, sira: usize) {
        if sira < self.sekmeler.len() {
            self.etkin = sira;
            self.yenile();
        }
    }

    fn yapistir(&mut self) {
        if let Some((kaynak, kesildi)) = self.pano.take() {
            let hedef = self.sekme().simdiki.clone();
            let sonuc = if kesildi {
                islemler::tasi(&kaynak, &hedef)
            } else {
                islemler::kopyala(&kaynak, &hedef)
            };
            if let Err(e) = sonuc {
                self.mesaj = e;
            }
            self.yenile();
        }
    }

    /// Etkin sekmenin klasörünü yeniden okur (başka sekmede yapılan değişiklik sekmeye dönünce görünür).
    fn yenile(&mut self) {
        let yol = self.sekme().simdiki.clone();
        let cop = self.cop_mu(&yol);
        let sonuc = islemler::listele(&yol);
        match sonuc {
            Ok(mut l) => {
                if cop {
                    // Çöp kutusunda özgün konumu tutan yan dosyalar kullanıcıya gösterilmez
                    l.retain(|g| !g.ad.ends_with(islemler::KONUM_UZANTISI));
                }
                self.sekme_mut().girdiler = l;
            }
            Err(e) => {
                self.mesaj = format!("Klasör okunamadı: {e}");
                self.sekme_mut().girdiler.clear();
            }
        }
        self.kisayollar = agac::kisayollar();
    }

    fn sekme_cubugu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("sekme_cubugu").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut secilecek = None;
                let mut kapatilacak = None;
                let tek = self.sekmeler.len() < 2;
                for sira in 0..self.sekmeler.len() {
                    let ad = self.sekme_adi(&self.sekmeler[sira].simdiki);
                    let tepki = ui
                        .selectable_label(self.etkin == sira, ad)
                        .on_hover_text(self.sekmeler[sira].simdiki.to_string_lossy());
                    if tepki.clicked() {
                        secilecek = Some(sira);
                    }
                    if tepki.middle_clicked() {
                        kapatilacak = Some(sira);
                    }
                    if !tek && ui.small_button("×").on_hover_text("Sekmeyi kapat").clicked() {
                        kapatilacak = Some(sira);
                    }
                    ui.separator();
                }
                if ui.button("➕").on_hover_text("Yeni sekme").clicked() {
                    let yol = self.sekme().simdiki.clone();
                    self.yeni_sekme(yol);
                }
                if let Some(s) = secilecek {
                    self.sekme_sec(s);
                }
                if let Some(s) = kapatilacak {
                    self.sekme_kapat(s);
                }
            });
        });
    }

    fn yan_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("agac_paneli")
            .min_width(200.0)
            .show(ctx, |ui| {
                ui.heading("Kısayollar");
                let simdiki = self.sekme().simdiki.clone();
                for (ad, yol) in self.kisayollar.clone() {
                    let tepki = ui.selectable_label(simdiki == yol, &ad);
                    if tepki.clicked() {
                        self.git(yol.clone());
                    }
                    if tepki.middle_clicked() {
                        self.yeni_sekme(yol.clone());
                    }
                    tepki.context_menu(|ui| {
                        if ui.button("Yeni Sekmede Aç").clicked() {
                            self.yeni_sekme(yol.clone());
                            ui.close_menu();
                        }
                    });
                }
                ui.separator();
                ui.heading("Klasörler");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut gezinilecek = None;
                    let mut secili = simdiki.clone();
                    let ev = self.ev.clone();
                    agac::dugum(ui, &ev, "🏠 Ev", &mut secili, &mut gezinilecek);
                    if let Some(yeni) = gezinilecek {
                        self.git(yeni);
                    }
                });
            });
    }

    fn arac_cubugu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("arac_cubugu").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let simdiki = self.sekme().simdiki.clone();
                let ust = simdiki.parent().map(Path::to_path_buf);
                if ui
                    .add_enabled(ust.is_some(), egui::Button::new("⬆ Yukarı"))
                    .clicked()
                {
                    if let Some(u) = ust {
                        self.git(u);
                    }
                }
                if self.cop_mu(&simdiki) {
                    let dolu = !self.sekme().girdiler.is_empty();
                    if ui
                        .add_enabled(dolu, egui::Button::new("Çöp Kutusunu Boşalt"))
                        .clicked()
                    {
                        self.bosaltma_onayi = true;
                    }
                } else if ui.button("Yeni Klasör").clicked() {
                    self.yeni_klasor_acik = true;
                    self.yeni_klasor_adi.clear();
                }
                ui.separator();
                ui.label(simdiki.to_string_lossy());
            });
        });
    }

    fn liste(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.mesaj.is_empty() {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, &self.mesaj);
                    if ui.small_button("×").clicked() {
                        self.mesaj.clear();
                    }
                });
                ui.separator();
            }
            let copte = self.cop_mu(&self.sekme().simdiki);
            if copte && self.sekme().girdiler.is_empty() {
                ui.weak("Çöp kutusu boş.");
            }
            let mut eylem = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for girdi in self.sekme().girdiler.clone() {
                    let etiket = if girdi.dizin {
                        format!("📁 {}", girdi.ad)
                    } else {
                        format!("📄 {}  ({})", girdi.ad, islemler::boyut_metni(girdi.boyut))
                    };
                    let secili_mi = self.sekme().secili.as_deref() == Some(girdi.yol.as_path());
                    let tepki = ui.selectable_label(secili_mi, etiket);
                    if tepki.clicked() {
                        self.sekme_mut().secili = Some(girdi.yol.clone());
                    }
                    if tepki.double_clicked() {
                        if girdi.dizin {
                            eylem = Some(Eylem::Git(girdi.yol.clone()));
                        } else {
                            ac::ac(&girdi.yol);
                        }
                    }
                    if girdi.dizin && tepki.middle_clicked() {
                        eylem = Some(Eylem::YeniSekme(girdi.yol.clone()));
                    }
                    tepki.context_menu(|ui| {
                        self.sekme_mut().secili = Some(girdi.yol.clone());
                        if copte {
                            if ui.button("Geri Yükle").clicked() {
                                eylem = Some(Eylem::GeriYukle(girdi.yol.clone()));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Kalıcı Olarak Sil").clicked() {
                                self.kalici_silme_onayi = Some(girdi.yol.clone());
                                ui.close_menu();
                            }
                            return;
                        }
                        if ui.button("Aç").clicked() {
                            if girdi.dizin {
                                eylem = Some(Eylem::Git(girdi.yol.clone()));
                            } else {
                                ac::ac(&girdi.yol);
                            }
                            ui.close_menu();
                        }
                        if girdi.dizin && ui.button("Yeni Sekmede Aç").clicked() {
                            eylem = Some(Eylem::YeniSekme(girdi.yol.clone()));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Kopyala").clicked() {
                            self.pano = Some((girdi.yol.clone(), false));
                            ui.close_menu();
                        }
                        if ui.button("Kes").clicked() {
                            self.pano = Some((girdi.yol.clone(), true));
                            ui.close_menu();
                        }
                        if ui.button("Yeniden Adlandır").clicked() {
                            self.yeniden_adlandir = Some((girdi.yol.clone(), girdi.ad.clone()));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Sil").clicked() {
                            self.silme_onayi = Some(girdi.yol.clone());
                            ui.close_menu();
                        }
                    });
                }
            });
            match eylem {
                Some(Eylem::Git(y)) => self.git(y),
                Some(Eylem::YeniSekme(y)) => self.yeni_sekme(y),
                Some(Eylem::GeriYukle(y)) => {
                    match islemler::geri_yukle(&y) {
                        Ok(h) => self.mesaj = format!("Geri yüklendi: {}", h.display()),
                        Err(e) => self.mesaj = e,
                    }
                    self.yenile();
                }
                None => {}
            }
            // Listenin altındaki boş alan: sağ tuşla klasörün kendi menüsü
            let bos = ui.allocate_response(ui.available_size(), egui::Sense::click());
            if bos.clicked() {
                self.sekme_mut().secili = None;
            }
            bos.context_menu(|ui| {
                if copte {
                    let dolu = !self.sekme().girdiler.is_empty();
                    if ui
                        .add_enabled(dolu, egui::Button::new("Çöp Kutusunu Boşalt"))
                        .clicked()
                    {
                        self.bosaltma_onayi = true;
                        ui.close_menu();
                    }
                } else {
                    if ui.button("Yeni Klasör").clicked() {
                        self.yeni_klasor_acik = true;
                        self.yeni_klasor_adi.clear();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.pano.is_some(), egui::Button::new("Yapıştır"))
                        .clicked()
                    {
                        self.yapistir();
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("Yenile").clicked() {
                    self.yenile();
                    ui.close_menu();
                }
            });
        });
    }

    fn pencereler(&mut self, ctx: &egui::Context) {
        if self.yeni_klasor_acik {
            let mut acik = true;
            let mut olustur = false;
            let mut vazgec = false;
            egui::Window::new("Yeni Klasör")
                .open(&mut acik)
                .show(ctx, |ui| {
                    ui.text_edit_singleline(&mut self.yeni_klasor_adi);
                    ui.horizontal(|ui| {
                        olustur = ui.button("Oluştur").clicked();
                        vazgec = ui.button("Vazgeç").clicked();
                    });
                });
            if olustur && !self.yeni_klasor_adi.trim().is_empty() {
                let ust = self.sekme().simdiki.clone();
                if let Err(e) = islemler::yeni_klasor(&ust, &self.yeni_klasor_adi) {
                    self.mesaj = e;
                }
                self.yenile();
            }
            if olustur || vazgec || !acik {
                self.yeni_klasor_acik = false;
            }
        }

        if let Some((yol, mut ad)) = self.yeniden_adlandir.clone() {
            let mut acik = true;
            let mut kaydet = false;
            let mut vazgec = false;
            egui::Window::new("Yeniden Adlandır")
                .open(&mut acik)
                .show(ctx, |ui| {
                    ui.text_edit_singleline(&mut ad);
                    ui.horizontal(|ui| {
                        kaydet = ui.button("Kaydet").clicked();
                        vazgec = ui.button("Vazgeç").clicked();
                    });
                });
            if kaydet {
                if let Err(e) = islemler::yeniden_adlandir(&yol, &ad) {
                    self.mesaj = e;
                }
                self.yenile();
                self.yeniden_adlandir = None;
            } else if vazgec || !acik {
                self.yeniden_adlandir = None;
            } else {
                self.yeniden_adlandir = Some((yol, ad));
            }
        }

        if let Some(yol) = self.silme_onayi.clone() {
            let ad = dosya_adi(&yol);
            egui::Window::new("Sil").show(ctx, |ui| {
                ui.label(format!("\"{ad}\" çöp kutusuna taşınsın mı?"));
                ui.horizontal(|ui| {
                    if ui.button("Evet, sil").clicked() {
                        if let Err(e) = islemler::cop_kutusuna_tasi(&yol) {
                            self.mesaj = e;
                        }
                        self.silme_onayi = None;
                        self.sekme_mut().secili = None;
                        self.yenile();
                    }
                    if ui.button("Vazgeç").clicked() {
                        self.silme_onayi = None;
                    }
                });
            });
        }

        if let Some(yol) = self.kalici_silme_onayi.clone() {
            let ad = dosya_adi(&yol);
            egui::Window::new("Kalıcı Olarak Sil").show(ctx, |ui| {
                ui.label(format!("\"{ad}\" kalıcı olarak silinsin mi? Bu işlem geri alınamaz."));
                ui.horizontal(|ui| {
                    if ui.button("Evet, kalıcı sil").clicked() {
                        if let Err(e) = islemler::kalici_sil(&yol) {
                            self.mesaj = e;
                        }
                        self.kalici_silme_onayi = None;
                        self.sekme_mut().secili = None;
                        self.yenile();
                    }
                    if ui.button("Vazgeç").clicked() {
                        self.kalici_silme_onayi = None;
                    }
                });
            });
        }

        if self.bosaltma_onayi {
            egui::Window::new("Çöp Kutusunu Boşalt").show(ctx, |ui| {
                ui.label("Çöp kutusundaki her şey kalıcı olarak silinsin mi? Bu işlem geri alınamaz.");
                ui.horizontal(|ui| {
                    if ui.button("Evet, boşalt").clicked() {
                        if let Some(cop) = self.cop.clone() {
                            if let Err(e) = islemler::cop_bosalt(&cop) {
                                self.mesaj = e;
                            }
                        }
                        self.bosaltma_onayi = false;
                        self.yenile();
                    }
                    if ui.button("Vazgeç").clicked() {
                        self.bosaltma_onayi = false;
                    }
                });
            });
        }
    }
}

fn dosya_adi(yol: &Path) -> String {
    yol.file_name()
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl eframe::App for Dosyalar {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sekme_cubugu(ctx);
        self.yan_panel(ctx);
        self.arac_cubugu(ctx);
        self.liste(ctx);
        self.pencereler(ctx);
    }
}

/// Seçili Armonia teması (ayarlar.conf TEMA=; yoksa "armonia"). Diğer uygulamalarla aynı dosyalar
/// (/etc/armonia/ayarlar.conf, /etc/armonia/tema/paletler) ama kendi başına, ayarlar crate'ine bağımlı değil.
fn secili_tema_kimlik() -> String {
    std::fs::read_to_string("/etc/armonia/ayarlar.conf")
        .ok()
        .and_then(|m| {
            m.lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .filter_map(|l| l.split_once('='))
                .find(|(k, _)| *k == "TEMA")
                .map(|(_, v)| v.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "armonia".to_string())
}

/// egui varsayılanı koyu temadır; bunu çağırmadan pencere Armonia'nın seçtiği temayla uyuşmuyordu (25.11).
fn tema(ctx: &egui::Context) {
    let kimlik = secili_tema_kimlik();
    let koyu = kimlik == "koyu";
    let hex = |s: &str| -> egui::Color32 {
        let v = u32::from_str_radix(s, 16).unwrap_or(0xFFFFFF);
        egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    };
    let satir = std::fs::read_to_string("/etc/armonia/tema/paletler")
        .unwrap_or_default()
        .lines()
        .find(|l| l.split('|').next() == Some(kimlik.as_str()))
        .map(|l| l.split('|').map(str::to_string).collect::<Vec<_>>());
    // Alanlar: kimlik|Ad|ARKA|ZEMIN|YAZI|KENAR|HAP|VURGU|VURGU_KOYU|VURGU_ZEMIN|PASIF|VURGU_UZERI
    let (arka, zemin, yazi) = match satir {
        Some(a) if a.len() == 12 => (hex(&a[2]), hex(&a[3]), hex(&a[4])),
        _ if koyu => (
            egui::Color32::from_rgb(0x1A, 0x1A, 0x1A),
            egui::Color32::from_rgb(0x26, 0x26, 0x26),
            egui::Color32::from_rgb(0xE8, 0xE8, 0xE8),
        ),
        _ => (
            egui::Color32::from_rgb(0xF3, 0xEE, 0xE6),
            egui::Color32::from_rgb(0xFC, 0xFA, 0xF6),
            egui::Color32::from_rgb(0x2F, 0x2F, 0x2F),
        ),
    };
    let mut v = if koyu {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = arka;
    v.window_fill = zemin;
    v.extreme_bg_color = if koyu { arka } else { egui::Color32::WHITE };
    v.override_text_color = Some(yazi);
    ctx.set_visuals(v);
}

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "Armonia Dosyalar",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            tema(&cc.egui_ctx);
            Ok(Box::new(Dosyalar::yeni()))
        }),
    )
}
