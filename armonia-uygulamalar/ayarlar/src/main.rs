//! Armonia Ayarlar (plan A3). Claude, 2026-11-19.
//! Bölümler: Ağ (A3.2'de Wi-Fi), Klavye, Ekran ve kilit, Yazıcılar, USB aygıtları (A3.3), Çoklu UYAP (katman
//! kurulunca), Hakkında. Yetki gerektiren her değişiklik `pkexec ayar-yardimci` üzerinden (dar alt komutlar).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod sistem;

use egui::{Color32, RichText};
use std::sync::mpsc;

/// Uzun süren ağ işleri (tarama, bağlanma) ayrı iş parçacığında; arayüz donmasın.
enum IsSonucu {
    Tarama(Result<Vec<sistem::WifiAgi>, String>),
    Baglanti(Result<(), String>, String),
}

const KREM: Color32 = Color32::from_rgb(243, 238, 230);
const KART: Color32 = Color32::from_rgb(252, 250, 246);
const YESIL: Color32 = Color32::from_rgb(95, 122, 97);
const ACIK_YESIL: Color32 = Color32::from_rgb(124, 154, 126);
const KENAR: Color32 = Color32::from_rgb(226, 218, 203);
const YAZI: Color32 = Color32::from_rgb(47, 47, 47);
const SOLUK: Color32 = Color32::from_rgb(110, 105, 95);
const UYARI: Color32 = Color32::from_rgb(160, 82, 45);

#[derive(PartialEq, Clone, Copy)]
enum Bolum {
    Ag,
    Klavye,
    Ekran,
    Yazicilar,
    Usb,
    CokluUyap,
    Hakkinda,
}

const BOLUMLER: [(Bolum, &str); 7] = [
    (Bolum::Ag, "Ağ"),
    (Bolum::Klavye, "Klavye"),
    (Bolum::Ekran, "Ekran ve kilit"),
    (Bolum::Yazicilar, "Yazıcılar"),
    (Bolum::Usb, "USB aygıtları"),
    (Bolum::CokluUyap, "Çoklu UYAP"),
    (Bolum::Hakkinda, "Hakkında"),
];

struct Uygulama {
    bolum: Bolum,
    canli: bool,
    klavye: String,
    kilit_dk: u32,
    yazicilar: Result<Vec<sistem::Yazici>, String>,
    silinecek: Option<String>,
    bilgi: sistem::Bilgi,
    mesaj: Option<(bool, String)>,
    // Ağ
    ag: Result<sistem::AgDurumu, String>,
    wifi: Vec<sistem::WifiAgi>,
    kayitli: Vec<(String, String)>,
    secili_ag: Option<String>,
    parola: String,
    is_kanali: Option<mpsc::Receiver<IsSonucu>>,
    is_metni: String,
}

impl Uygulama {
    fn yeni(cc: &eframe::CreationContext<'_>) -> Self {
        tema(&cc.egui_ctx);
        let mut u = Uygulama {
            bolum: Bolum::Klavye,
            canli: sistem::canli(),
            klavye: "q".into(),
            kilit_dk: 10,
            yazicilar: Ok(Vec::new()),
            silinecek: None,
            bilgi: sistem::bilgi(),
            mesaj: None,
            ag: Ok(Default::default()),
            wifi: Vec::new(),
            kayitli: Vec::new(),
            secili_ag: None,
            parola: String::new(),
            is_kanali: None,
            is_metni: String::new(),
        };
        u.yenile();
        // `armonia-ayarlar yazicilar` gibi doğrudan bölümle açılabilsin (dock kutucuğu)
        if let Some(a) = std::env::args().nth(1) {
            u.bolum = match a.as_str() {
                "ag" => Bolum::Ag,
                "ekran" => Bolum::Ekran,
                "yazicilar" => Bolum::Yazicilar,
                "usb" => Bolum::Usb,
                "hakkinda" => Bolum::Hakkinda,
                _ => Bolum::Klavye,
            };
        }
        u
    }

    fn yenile(&mut self) {
        let a = sistem::ayarlar();
        self.klavye = a.get("KLAVYE").cloned().filter(|k| k == "f").unwrap_or_else(|| "q".into());
        self.kilit_dk = a.get("KILIT_DK").and_then(|d| d.parse().ok()).filter(|d| [5, 10, 15, 30].contains(d)).unwrap_or(10);
        self.yazicilar = sistem::yazicilar();
        self.ag = sistem::ag_durumu();
        self.kayitli = sistem::kayitli_wifi();
    }

    fn is_baslat(&mut self, metin: &str, ctx: &egui::Context, is: impl FnOnce() -> IsSonucu + Send + 'static) {
        let (t, r) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = t.send(is());
            ctx.request_repaint();
        });
        self.is_kanali = Some(r);
        self.is_metni = metin.to_string();
    }

    fn is_bitti_mi(&mut self) {
        let Some(r) = &self.is_kanali else { return };
        let Ok(s) = r.try_recv() else { return };
        self.is_kanali = None;
        match s {
            IsSonucu::Tarama(Ok(l)) => self.wifi = l,
            IsSonucu::Tarama(Err(e)) => self.mesaj = Some((false, format!("Ağlar taranamadı: {e}"))),
            IsSonucu::Baglanti(r, ad) => {
                if r.is_ok() {
                    self.secili_ag = None;
                    self.parola.clear();
                }
                self.sonuc(r, &format!("{ad} ağına bağlanıldı."));
                if let Ok(l) = sistem::wifi_tara() {
                    self.wifi = l;
                }
            }
        }
    }

    fn sonuc(&mut self, r: Result<(), String>, iyi: &str) {
        self.mesaj = Some(match r {
            Ok(()) => (true, iyi.to_string()),
            Err(e) => (false, format!("Yapılamadı: {e}")),
        });
        self.yenile();
    }
}

fn tema(ctx: &egui::Context) {
    let mut v = egui::Visuals::light();
    v.panel_fill = KREM;
    v.window_fill = KART;
    v.extreme_bg_color = Color32::WHITE;
    v.override_text_color = Some(YAZI);
    v.selection.bg_fill = ACIK_YESIL;
    v.selection.stroke.color = Color32::WHITE;
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(236, 230, 219);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(230, 235, 227);
    v.widgets.active.weak_bg_fill = ACIK_YESIL;
    v.widgets.noninteractive.bg_stroke.color = KENAR;
    let r = egui::Rounding::same(8.0);
    v.widgets.inactive.rounding = r;
    v.widgets.hovered.rounding = r;
    v.widgets.active.rounding = r;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        use egui::{FontId, TextStyle};
        s.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(22.0));
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

fn kart(ui: &mut egui::Ui, icerik: impl FnOnce(&mut egui::Ui)) {
    // İç kenar boşluğu (2 × 16) ve çerçeve çizgisi dış genişliğe eklenir; taşmasın
    let w = (ui.available_width() - 34.0).max(100.0);
    egui::Frame::none()
        .fill(KART)
        .stroke(egui::Stroke::new(1.0_f32, KENAR))
        .rounding(14.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(w);
            icerik(ui)
        });
    ui.add_space(8.0);
}

fn aciklama(ui: &mut egui::Ui, m: &str) {
    ui.label(RichText::new(m).size(13.0).color(SOLUK));
}

impl eframe::App for Uygulama {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.is_bitti_mi();
        egui::SidePanel::left("bolumler")
            .resizable(false)
            .exact_width(200.0)
            .frame(egui::Frame::none().fill(KART).inner_margin(14.0))
            .show(ctx, |ui| {
                ui.label(RichText::new("Ayarlar").size(22.0).strong().color(YESIL));
                ui.add_space(10.0);
                for (b, ad) in BOLUMLER {
                    let r = ui.add_sized([170.0, 34.0], egui::SelectableLabel::new(self.bolum == b, RichText::new(ad).size(16.0)));
                    if r.clicked() && self.bolum != b {
                        self.bolum = b;
                        self.mesaj = None;
                        self.silinecek = None;
                        self.yenile();
                    }
                }
            });
        egui::CentralPanel::default().frame(egui::Frame::none().fill(KREM).inner_margin(20.0)).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Kaydırma çubuğuna yer kalsın (yoksa kartların sağ kenarı kırpılıyor)
                ui.set_max_width((ui.available_width() - 16.0).min(720.0));
                match self.bolum {
                    Bolum::Ag => self.ag(ui),
                    Bolum::Klavye => self.klavye_bolumu(ui),
                    Bolum::Ekran => self.ekran(ui),
                    Bolum::Yazicilar => self.yazici_bolumu(ui),
                    Bolum::Usb => self.usb(ui),
                    Bolum::CokluUyap => self.coklu_uyap(ui),
                    Bolum::Hakkinda => self.hakkinda(ui),
                }
                if let Some((iyi, m)) = &self.mesaj {
                    ui.label(RichText::new(m).color(if *iyi { YESIL } else { UYARI }));
                }
            });
        });
    }
}

impl Uygulama {
    fn ag(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ağ");
        let durum = match &self.ag {
            Ok(d) => d.clone(),
            Err(e) => {
                ui.label(RichText::new(format!("Ağ yöneticisine ulaşılamadı: {e}")).color(UYARI));
                return;
            }
        };
        let mesgul = self.is_kanali.is_some();
        kart(ui, |ui| {
            ui.label(RichText::new("Kablolu bağlantı").strong());
            match &durum.kablolu {
                Some(_) => ui.label(RichText::new("Bağlı").color(YESIL)),
                None => ui.label(RichText::new("Kablo takılı değil").color(SOLUK)),
            };
        });
        if durum.kablosuz_var {
            kart(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Kablosuz (Wi-Fi)").strong());
                    let mut acik = durum.wifi_acik;
                    if ui.add_enabled(!mesgul, egui::Checkbox::new(&mut acik, "Açık")).changed() {
                        let r = sistem::wifi_radyo(acik);
                        self.wifi.clear();
                        self.sonuc(r, if acik { "Wi-Fi açıldı." } else { "Wi-Fi kapatıldı." });
                    }
                });
                if !durum.wifi_acik {
                    return;
                }
                match &durum.kablosuz {
                    Some(ad) => ui.label(RichText::new(format!("Bağlı: {ad}")).color(YESIL)),
                    None => ui.label(RichText::new("Bağlı değil").color(SOLUK)),
                };
                ui.horizontal(|ui| {
                    if ui.add_enabled(!mesgul, egui::Button::new("Ağları tara")).clicked() {
                        self.is_baslat("Ağlar taranıyor…", ui.ctx(), || IsSonucu::Tarama(sistem::wifi_tara()));
                    }
                    if mesgul {
                        ui.spinner();
                        ui.label(&self.is_metni);
                    }
                });
                let mut baglan: Option<(String, String, &'static str)> = None;
                for a in self.wifi.clone() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        let cubuk = match a.sinyal {
                            0..=25 => "▂",
                            26..=50 => "▂▄",
                            51..=75 => "▂▄▆",
                            _ => "▂▄▆█",
                        };
                        ui.label(RichText::new(cubuk).color(YESIL).monospace());
                        ui.label(RichText::new(&a.ssid).strong());
                        if a.guvenlik == "acik" {
                            ui.label(RichText::new("şifresiz").size(12.0).color(UYARI));
                        }
                        if a.bagli {
                            ui.label(RichText::new("bağlı").size(12.0).color(YESIL));
                        } else if self.secili_ag.as_deref() != Some(&a.ssid)
                            && ui.add_enabled(!mesgul, egui::Button::new("Bağlan").small()).clicked()
                        {
                            let kayit = self.kayitli.iter().find(|(ad, _)| *ad == a.ssid).map(|(_, u)| u.clone());
                            match (kayit, a.guvenlik) {
                                (Some(u), _) => {
                                    let ad = a.ssid.clone();
                                    self.is_baslat("Bağlanılıyor…", ui.ctx(), move || {
                                        IsSonucu::Baglanti(sistem::kayitliya_baglan(&u), ad)
                                    });
                                }
                                (None, "acik") => baglan = Some((a.ssid.clone(), String::new(), "acik")),
                                (None, _) => {
                                    self.secili_ag = Some(a.ssid.clone());
                                    self.parola.clear();
                                }
                            }
                        }
                    });
                    if self.secili_ag.as_deref() == Some(&a.ssid) {
                        ui.horizontal(|ui| {
                            ui.label("Parola");
                            let r = ui.add(egui::TextEdit::singleline(&mut self.parola).password(true).desired_width(220.0));
                            let gecerli = (8..=63).contains(&self.parola.len());
                            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.add_enabled(gecerli && !mesgul, egui::Button::new("Bağlan")).clicked() || (enter && gecerli) {
                                baglan = Some((a.ssid.clone(), self.parola.clone(), a.guvenlik));
                            }
                            if ui.button("Vazgeç").clicked() {
                                self.secili_ag = None;
                                self.parola.clear();
                            }
                        });
                    }
                }
                if let Some((ssid, parola, guv)) = baglan {
                    let ad = ssid.clone();
                    self.is_baslat("Bağlanılıyor…", ui.ctx(), move || {
                        IsSonucu::Baglanti(sistem::wifi_baglan(&ssid, &parola, guv), ad)
                    });
                }
            });
        }
        if !self.kayitli.is_empty() {
            kart(ui, |ui| {
                ui.label(RichText::new("Kayıtlı Wi-Fi ağları").strong());
                let mut unut = None;
                for (ad, uuid) in &self.kayitli {
                    ui.horizontal(|ui| {
                        ui.label(ad);
                        if ui.add_enabled(!mesgul, egui::Button::new("Unut").small()).clicked() {
                            unut = Some((ad.clone(), uuid.clone()));
                        }
                    });
                }
                if let Some((ad, uuid)) = unut {
                    let r = sistem::yardimci("wifi-unut", &uuid);
                    self.sonuc(r, &format!("{ad} unutuldu."));
                }
            });
        }
        kart(ui, |ui| {
            ui.label(RichText::new("Güvenlik").strong());
            aciklama(
                ui,
                "Tarayıcı yalnız adli sitelere açılır. Yerel ağdaki aygıtlara (ağ yazıcısı, dosya paylaşımı) erişim \
                 güvenlik nedeniyle kapalıdır. Kurumsal (802.1X) ve WEP ağlar desteklenmez.",
            );
        });
    }

    fn klavye_bolumu(&mut self, ui: &mut egui::Ui) {
        ui.heading("Klavye");
        kart(ui, |ui| {
            ui.label(RichText::new("Klavye düzeni").strong());
            let mut secim = self.klavye.clone();
            ui.radio_value(&mut secim, "q".to_string(), "Türkçe Q");
            ui.radio_value(&mut secim, "f".to_string(), "Türkçe F");
            aciklama(ui, "Değişiklik hemen uygulanır (masaüstü ve metin giriş ekranı).");
            if secim != self.klavye {
                let r = sistem::yardimci("klavye", &secim);
                if r.is_ok() {
                    sistem::labwc_yenile();
                    sistem::ust_cubuk_yenile();
                }
                let ad = if secim == "f" { "Türkçe F" } else { "Türkçe Q" };
                self.sonuc(r, &format!("Klavye düzeni: {ad}"));
            }
        });
    }

    fn ekran(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ekran ve kilit");
        kart(ui, |ui| {
            ui.label(RichText::new("Ekran kilidi").strong());
            if self.canli {
                aciklama(
                    ui,
                    "Deneme (canlı) sisteminde parola olmadığı için ekran kilitlenmez; 15 dakika boşta kalınca ekran \
                     kapanır. Diske kurulumdan sonra kilit süresi buradan seçilir.",
                );
                return;
            }
            aciklama(ui, "Bilgisayar bu süre boyunca kullanılmazsa ekran kilitlenir; açmak için parola gerekir.");
            let mut dk = self.kilit_dk;
            ui.horizontal(|ui| {
                for d in [5, 10, 15, 30] {
                    ui.radio_value(&mut dk, d, format!("{d} dakika"));
                }
            });
            if dk != self.kilit_dk {
                let r = sistem::yardimci("kilit", &dk.to_string());
                if r.is_ok() {
                    sistem::bosta_yenile();
                }
                self.sonuc(r, &format!("Ekran {dk} dakika sonra kilitlenecek."));
            }
        });
        kart(ui, |ui| {
            ui.label(RichText::new("Şimdi kilitle").strong());
            aciklama(ui, "Kısayol: Windows tuşu + L");
        });
    }

    fn yazici_bolumu(&mut self, ui: &mut egui::Ui) {
        ui.heading("Yazıcılar");
        let liste = match &self.yazicilar {
            Err(e) => {
                ui.label(RichText::new(e).color(UYARI));
                return;
            }
            Ok(l) => l.clone(),
        };
        if liste.is_empty() {
            kart(ui, |ui| {
                ui.label(RichText::new("Kurulu yazıcı yok").strong());
            });
        }
        let mut islem: Option<(&str, String)> = None;
        for y in &liste {
            kart(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(y.ad.replace('_', " ")).strong().size(16.0));
                    if y.varsayilan {
                        ui.label(RichText::new("varsayılan").size(12.0).color(Color32::WHITE).background_color(ACIK_YESIL));
                    }
                });
                aciklama(ui, &format!("{} · {}", sistem::baglanti_turu(&y.adres), y.durum));
                ui.horizontal(|ui| {
                    if ui.button("Deneme sayfası").clicked() {
                        islem = Some(("deneme", y.ad.clone()));
                    }
                    if !y.varsayilan && ui.button("Varsayılan yap").clicked() {
                        islem = Some(("varsayilan", y.ad.clone()));
                    }
                    if self.silinecek.as_deref() == Some(&y.ad) {
                        ui.label(RichText::new("Silinsin mi?").color(UYARI));
                        if ui.button("Evet, sil").clicked() {
                            islem = Some(("sil", y.ad.clone()));
                        }
                        if ui.button("Vazgeç").clicked() {
                            islem = Some(("vazgec", String::new()));
                        }
                    } else if ui.button("Sil").clicked() {
                        islem = Some(("sor", y.ad.clone()));
                    }
                });
            });
        }
        match islem {
            Some(("deneme", ad)) => self.sonuc(sistem::deneme_sayfasi(&ad), "Deneme sayfası gönderildi."),
            Some(("varsayilan", ad)) => self.sonuc(sistem::yardimci("yazici-varsayilan", &ad), "Varsayılan yazıcı değişti."),
            Some(("sil", ad)) => {
                self.silinecek = None;
                self.sonuc(sistem::yardimci("yazici-sil", &ad), "Yazıcı silindi.");
            }
            Some(("sor", ad)) => self.silinecek = Some(ad),
            Some(("vazgec", _)) => self.silinecek = None,
            _ => {}
        }
        kart(ui, |ui| {
            ui.label(RichText::new("Yazıcı eklemek").strong());
            aciklama(
                ui,
                "Yazıcıyı USB ile takın. Armonia izin isteyecek; izin verince sürücü gerektirmeyen yazıcılar kendiliğinden \
                 eklenir. Sürücü gerekiyorsa boyutu gösterilir ve onayınızla indirilir.",
            );
            if ui.button("Listeyi yenile").clicked() {
                self.yenile();
            }
        });
    }

    fn usb(&mut self, ui: &mut egui::Ui) {
        ui.heading("USB aygıtları");
        kart(ui, |ui| {
            ui.label(RichText::new("Takınca sor").strong());
            aciklama(
                ui,
                "Klavye, fare, e-imza okuyucu, kamera ve kulaklık doğrudan çalışır. USB bellek, yazıcı gibi aygıtlar \
                 takılınca izin sorulur; izin yalnız o takılış için geçerlidir. USB ağ aygıtları ve gizli klavye \
                 içeren aygıtlar her zaman reddedilir.",
            );
        });
        kart(ui, |ui| {
            ui.label(RichText::new("Her zaman izin verilen aygıtlar").strong());
            aciklama(ui, "Kalıcı izin listesi bir sonraki adımda eklenecek (diske kurulan sistemde).");
        });
    }

    fn coklu_uyap(&mut self, ui: &mut egui::Ui) {
        ui.heading("Çoklu UYAP");
        kart(ui, |ui| {
            ui.add_enabled(false, egui::Checkbox::new(&mut false, "Çoklu UYAP'ı aç"));
            aciklama(
                ui,
                "Çoklu UYAP, birden çok avukatın UYAP işlerini tek ekranda izleyen ek programdır. Bu sistemde henüz \
                 kurulu değil; kurulunca buradan açılıp kapatılacak.",
            );
        });
    }

    fn hakkinda(&mut self, ui: &mut egui::Ui) {
        ui.heading("Hakkında");
        let b = self.bilgi.clone();
        kart(ui, |ui| {
            egui::Grid::new("bilgi").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                for (k, v) in [("Sistem", &b.sistem), ("İşlemci", &b.islemci), ("Bellek", &b.bellek), ("Disk", &b.disk), ("Çekirdek", &b.cekirdek)] {
                    ui.label(RichText::new(k).color(SOLUK));
                    ui.label(v.as_str());
                    ui.end_row();
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let secenekler = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ayarlar")
            .with_app_id("armonia-ayarlar")
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([620.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native("Ayarlar", secenekler, Box::new(|cc| Ok(Box::new(Uygulama::yeni(cc)))))
}
