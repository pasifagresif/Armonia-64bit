//! Armonia Ayarlar (plan A3). Claude, 2026-11-19.
//! Bölümler: Ağ (A3.2'de Wi-Fi), Klavye, Ekran ve kilit, Yazıcılar, USB aygıtları (A3.3), Çoklu UYAP (katman
//! kurulunca), Hakkında. Yetki gerektiren her değişiklik `pkexec ayar-yardimci` üzerinden (dar alt komutlar).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod guncelleme;
mod sistem;

use egui::{Color32, RichText};
use std::sync::mpsc;

/// Uzun süren ağ işleri (tarama, bağlanma) ayrı iş parçacığında; arayüz donmasın.
enum IsSonucu {
    Tarama(Result<Vec<sistem::WifiAgi>, String>),
    Baglanti(Result<(), String>, String),
    /// Çoklu UYAP güncellemesi: denetim ya da indir + kur (kurulan sürüm)
    Denetim(Result<(), String>),
    Kurulum(Result<String, String>),
    /// Çoklu UYAP'ı durdurup açma gibi kısa ama bekleten işler (arayüz donmasın; denetçi 24.11 D-3)
    Islem(Result<(), String>, String),
}

/// Hata/uyarı rengi temadan bağımsız (anlam taşır).
const UYARI: Color32 = Color32::from_rgb(160, 82, 45);

/// Pencerenin renkleri: seçili Armonia temasından (/etc/armonia/tema/paletler, 23.11).
#[derive(Clone, Copy)]
struct Renkler {
    arka: Color32,
    kart: Color32,
    baslik: Color32,
    vurgu: Color32,
    kenar: Color32,
    yazi: Color32,
    soluk: Color32,
    hap: Color32,
    vurgu_zemin: Color32,
    vurgu_uzeri: Color32,
    koyu: bool,
}

const fn hex(x: u32) -> Color32 {
    Color32::from_rgb((x >> 16) as u8, (x >> 8) as u8, x as u8)
}

fn karistir(a: Color32, b: Color32, t: f32) -> Color32 {
    let k = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(k(a.r(), b.r()), k(a.g(), b.g()), k(a.b(), b.b()))
}

impl Renkler {
    fn yeni(t: &sistem::Tema) -> Self {
        let [arka, zemin, yazi, kenar, hap, vurgu, vurgu_koyu, vurgu_zemin, _pasif, vurgu_uzeri] = t.renk.map(hex);
        let koyu = t.kimlik == "koyu";
        Renkler {
            arka,
            kart: zemin,
            // Koyu zeminde koyu vurgu okunmaz: başlık/olumlu yazı vurgunun açılmış tonu
            baslik: if koyu { karistir(vurgu, Color32::WHITE, 0.45) } else { vurgu_koyu },
            vurgu,
            kenar,
            yazi,
            soluk: karistir(yazi, arka, 0.42),
            hap,
            vurgu_zemin,
            vurgu_uzeri,
            koyu,
        }
    }
}

static RENK: std::sync::RwLock<Option<Renkler>> = std::sync::RwLock::new(None);

fn r() -> Renkler {
    RENK.read().ok().and_then(|g| *g).unwrap_or_else(|| Renkler::yeni(&sistem::armonia_temasi()))
}

fn renkleri_sec(t: &sistem::Tema) {
    if let Ok(mut g) = RENK.write() {
        *g = Some(Renkler::yeni(t));
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Bolum {
    Ag,
    Klavye,
    Ekran,
    Yazicilar,
    Usb,
    CokluUyap,
    Gorunum,
    Hakkinda,
}

const BOLUMLER: [(Bolum, &str); 8] = [
    (Bolum::Ag, "Ağ"),
    (Bolum::Klavye, "Klavye"),
    (Bolum::Ekran, "Ekran ve kilit"),
    (Bolum::Yazicilar, "Yazıcılar"),
    (Bolum::Usb, "USB aygıtları"),
    (Bolum::CokluUyap, "Çoklu UYAP"),
    (Bolum::Gorunum, "Görünüm"),
    (Bolum::Hakkinda, "Hakkında"),
];

struct Uygulama {
    bolum: Bolum,
    temalar: Vec<sistem::Tema>,
    tema: String,
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
    // Çoklu UYAP güncellemeleri
    guncelleme: Result<guncelleme::Durum, String>,
    /// Kurulum bitti, Çoklu UYAP açıksa yeniden başlatma önerilir
    yeniden_baslat_oner: bool,
    imaja_don_sor: bool,
}

impl Uygulama {
    fn yeni(cc: &eframe::CreationContext<'_>) -> Self {
        let temalar = sistem::temalar();
        let secili = sistem::ayarlar().get("TEMA").cloned().unwrap_or_default();
        let t = temalar.iter().find(|t| t.kimlik == secili).unwrap_or(&temalar[0]).clone();
        renkleri_sec(&t);
        tema(&cc.egui_ctx);
        let mut u = Uygulama {
            bolum: Bolum::Klavye,
            temalar,
            tema: t.kimlik,
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
            guncelleme: Ok(Default::default()),
            yeniden_baslat_oner: false,
            imaja_don_sor: false,
        };
        u.yenile();
        // `armonia-ayarlar yazicilar` gibi doğrudan bölümle açılabilsin (dock kutucuğu)
        if let Some(a) = std::env::args().nth(1) {
            u.bolum = match a.as_str() {
                "ag" => Bolum::Ag,
                "ekran" => Bolum::Ekran,
                "yazicilar" => Bolum::Yazicilar,
                "usb" => Bolum::Usb,
                // Üst çubuktaki "Güncelleme" göstergesi buraya açar
                "cokluuyap" => Bolum::CokluUyap,
                "gorunum" => Bolum::Gorunum,
                "hakkinda" => Bolum::Hakkinda,
                _ => Bolum::Klavye,
            };
            u.yenile();
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
        // Kurulu güncellemenin ağacı da doğrulanır (özet alır): yalnız bu bölüm açıkken
        if self.bolum == Bolum::CokluUyap {
            self.guncelleme = guncelleme::durum();
        }
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
            IsSonucu::Denetim(r) => {
                self.sonuc(r, "Güncelleme bilgisi denetlendi.");
                sistem::ust_cubuk_yenile();
            }
            IsSonucu::Islem(r, iyi) => self.sonuc(r, &iyi),
            IsSonucu::Kurulum(r) => {
                let iyi = match &r {
                    Ok(s) if !s.is_empty() => format!("Çoklu UYAP {s} kuruldu."),
                    _ => "Güncelleme kuruldu.".to_string(),
                };
                self.yeniden_baslat_oner = r.is_ok() && guncelleme::cokluuyap_calisiyor();
                self.sonuc(r.map(|_| ()), &iyi);
                sistem::ust_cubuk_yenile();
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
    let c = r();
    let mut v = if c.koyu { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = c.arka;
    v.window_fill = c.kart;
    v.extreme_bg_color = if c.koyu { c.arka } else { Color32::WHITE };
    v.override_text_color = Some(c.yazi);
    v.selection.bg_fill = c.vurgu;
    v.selection.stroke.color = c.vurgu_uzeri;
    v.widgets.inactive.weak_bg_fill = c.hap;
    v.widgets.hovered.weak_bg_fill = c.vurgu_zemin;
    v.widgets.active.weak_bg_fill = c.vurgu;
    v.widgets.noninteractive.bg_stroke.color = c.kenar;
    let yuv = egui::Rounding::same(8.0);
    v.widgets.inactive.rounding = yuv;
    v.widgets.hovered.rounding = yuv;
    v.widgets.active.rounding = yuv;
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
        .fill(r().kart)
        .stroke(egui::Stroke::new(1.0_f32, r().kenar))
        .rounding(14.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(w);
            icerik(ui)
        });
    ui.add_space(8.0);
}

fn aciklama(ui: &mut egui::Ui, m: &str) {
    ui.label(RichText::new(m).size(13.0).color(r().soluk));
}

impl eframe::App for Uygulama {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.is_bitti_mi();
        egui::SidePanel::left("bolumler")
            .resizable(false)
            .exact_width(200.0)
            .frame(egui::Frame::none().fill(r().kart).inner_margin(14.0))
            .show(ctx, |ui| {
                ui.label(RichText::new("Ayarlar").size(22.0).strong().color(r().baslik));
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
        egui::CentralPanel::default().frame(egui::Frame::none().fill(r().arka).inner_margin(20.0)).show(ctx, |ui| {
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
                    Bolum::Gorunum => self.gorunum(ui),
                    Bolum::Hakkinda => self.hakkinda(ui),
                }
                if let Some((iyi, m)) = &self.mesaj {
                    ui.label(RichText::new(m).color(if *iyi { r().baslik } else { UYARI }));
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
                Some(_) => ui.label(RichText::new("Bağlı").color(r().baslik)),
                None => ui.label(RichText::new("Kablo takılı değil").color(r().soluk)),
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
                    Some(ad) => ui.label(RichText::new(format!("Bağlı: {ad}")).color(r().baslik)),
                    None => ui.label(RichText::new("Bağlı değil").color(r().soluk)),
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
                        ui.label(RichText::new(cubuk).color(r().baslik).monospace());
                        ui.label(RichText::new(&a.ssid).strong());
                        if a.guvenlik == "acik" {
                            ui.label(RichText::new("şifresiz").size(12.0).color(UYARI));
                        }
                        if a.bagli {
                            ui.label(RichText::new("bağlı").size(12.0).color(r().baslik));
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
                        ui.label(RichText::new("varsayılan").size(12.0).color(r().vurgu_uzeri).background_color(r().vurgu));
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
        let kurulu = std::path::Path::new("/opt/armonia/cokluuyap/uyap_app").exists();
        kart(ui, |ui| {
            if kurulu {
                ui.label(RichText::new("Kurulu").strong().color(r().baslik));
                aciklama(
                    ui,
                    "Çoklu UYAP, birden çok avukatın UYAP işlerini tek ekranda izleyen ek programdır. Açılışta \
                     kendiliğinden başlamaz; menüden ya da alttaki \"Çoklu UYAP\" kutucuğundan açılır. Kendi kum \
                     havuzunda çalışır: yalnız kendi veri klasörünü ve Belgeler/UYAP klasörünü görür, yerel ağa kapalıdır.",
                );
                if ui.button("Çoklu UYAP'ı aç").clicked() {
                    let _ = std::process::Command::new("/opt/armonia/bin/cokluuyap_ac.sh").spawn();
                }
            } else {
                aciklama(
                    ui,
                    "Çoklu UYAP, birden çok avukatın UYAP işlerini tek ekranda izleyen ek programdır. Bu sistemde \
                     kurulu değil.",
                );
            }
        });
        if kurulu {
            self.guncellemeler(ui);
        }
    }

    fn guncellemeler(&mut self, ui: &mut egui::Ui) {
        let d = match &self.guncelleme {
            Ok(d) => d.clone(),
            Err(e) => {
                kart(ui, |ui| {
                    ui.label(RichText::new("Güncellemeler").strong());
                    ui.label(RichText::new(e).color(UYARI));
                });
                return;
            }
        };
        let mesgul = self.is_kanali.is_some();
        let mut eylem: Option<&str> = None;
        kart(ui, |ui| {
            ui.label(RichText::new("Güncellemeler").strong());
            if d.canli {
                aciklama(
                    ui,
                    "USB'den (canlı) çalışırken Çoklu UYAP güncellenmez; her açılışta imajdaki sürüm açılır. Yeni sürüm \
                     için güncel Armonia ISO'sunu belleğe yazın.",
                );
                return;
            }
            if !d.katman {
                aciklama(ui, "Bu sistemde Çoklu UYAP yalnız Armonia imajıyla birlikte güncellenir.");
                return;
            }
            egui::Grid::new("guncelleme").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                let satir = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).color(r().soluk));
                    ui.label(v);
                    ui.end_row();
                };
                satir(ui, "İmajdaki sürüm", d.imaj.clone().unwrap_or_else(|| "bilinmiyor".into()));
                satir(ui, "Kurulu güncelleme", d.etkin.clone().unwrap_or_else(|| "yok (imajdaki sürüm)".into()));
                if let Some(z) = &d.son_kurulum {
                    satir(ui, "Son kurulum", guncelleme::yerel_zaman(z));
                }
                // Zamanı ağa çıkan denetleyici yazar; imzalı değildir (denetçi 24.11 O-2)
                satir(
                    ui,
                    "Son denetim",
                    d.son_denetim.as_deref().map(|z| format!("{} (denetleyicinin bildirdiği)", guncelleme::yerel_zaman(z))).unwrap_or_else(|| "henüz yok".into()),
                );
            });
            if let Some(n) = &d.etkin_red {
                ui.label(
                    RichText::new(format!("Kurulu güncelleme kullanılmıyor, Çoklu UYAP imajdaki sürümle açılır: {n}"))
                        .size(13.0)
                        .color(UYARI),
                );
            }
            if d.imaja_don {
                ui.label(
                    RichText::new(
                        "Bu açılışta imajdaki sürüm çalışıyor (güncellenmiş sürüm açılamadı ya da siz istediniz). \
                         Bilgisayar yeniden başlayınca kurulu güncelleme kullanılır.",
                    )
                    .color(UYARI),
                );
            }
            if let Some(h) = &d.son_denetim_hata {
                ui.label(RichText::new(format!("Son denetim başarısız: {h}")).size(13.0).color(UYARI));
            }
            if let Some(h) = &d.teklif_hata {
                ui.label(RichText::new(format!("Güncelleme bilgisi doğrulanamadı: {h}")).size(13.0).color(UYARI));
            }
            ui.add_space(4.0);
            match &d.teklif {
                Some(t) if t.kurulabilir => {
                    ui.label(
                        RichText::new(format!("Yeni sürüm: {} ({})", t.surum, guncelleme::yerel_zaman(&t.yayin)))
                            .strong()
                            .color(r().baslik),
                    );
                    if !t.notlar.is_empty() {
                        aciklama(ui, &t.notlar);
                    }
                    aciklama(
                        ui,
                        "Paket cokluuyap.com'dan indirilir; imzası ve her dosyası doğrulanmadan kurulmaz. Parolanız \
                         sorulur. Kurulum bitince Çoklu UYAP yeniden açıldığında yeni sürüm çalışır.",
                    );
                    if ui.add_enabled(!mesgul, egui::Button::new(format!("{} sürümünü indir ve kur", t.surum))).clicked() {
                        eylem = Some("kur");
                    }
                }
                Some(t) => {
                    // "Güncel" yalnız son imzalı bilgiye göredir; yayın tarihi gösterilir ki eski bilgi fark edilsin
                    ui.label(
                        RichText::new(format!(
                            "Çoklu UYAP güncel (son imzalı yayın bilgisi: {} sürümü, {}).",
                            t.surum,
                            guncelleme::yerel_zaman(&t.yayin)
                        ))
                        .color(r().baslik),
                    );
                }
                None => aciklama(
                    ui,
                    "Güncelleme bilgisi henüz alınmadı. Açılıştan yaklaşık 10 dakika sonra ve günde bir kendiliğinden denetlenir.",
                ),
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!mesgul, egui::Button::new("Şimdi denetle")).clicked() {
                    eylem = Some("denetle");
                }
                if mesgul {
                    ui.spinner();
                    ui.label(&self.is_metni);
                }
            });
            if self.yeniden_baslat_oner {
                ui.separator();
                ui.label("Çoklu UYAP açık. Yeni sürüm, Çoklu UYAP yeniden açılınca çalışır; süren işler durur.");
                ui.horizontal(|ui| {
                    if ui.button("Çoklu UYAP'ı yeniden başlat").clicked() {
                        eylem = Some("yeniden");
                    }
                    if ui.button("Sonra").clicked() {
                        eylem = Some("sonra");
                    }
                });
            }
        });
        if d.katman && !d.canli && d.etkin.is_some() && !d.imaja_don {
            kart(ui, |ui| {
                ui.label(RichText::new("Sorun olursa").strong());
                aciklama(
                    ui,
                    "Güncellenmiş Çoklu UYAP düzgün çalışmazsa bu açılış boyunca imajdaki sürümle açabilirsiniz. Bu kalıcı \
                     değildir; yeniden başlatınca kurulu güncelleme kullanılır. İmajdaki sürüm daha eski olduğundan, \
                     yalnız gerektiğinde kullanın.",
                );
                if self.imaja_don_sor {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Çoklu UYAP kapatılıp imajdaki sürümle açılsın mı?").color(UYARI));
                        if ui.button("Evet").clicked() {
                            eylem = Some("imaja-don");
                        }
                        if ui.button("Vazgeç").clicked() {
                            eylem = Some("imaja-vazgec");
                        }
                    });
                } else if ui.add_enabled(!mesgul, egui::Button::new("Bu açılışta imajdaki sürümle aç")).clicked() {
                    eylem = Some("imaja-sor");
                }
            });
        }
        match eylem {
            Some("denetle") => self.is_baslat("Denetleniyor…", ui.ctx(), || IsSonucu::Denetim(guncelleme::denetle())),
            Some("kur") => {
                self.yeniden_baslat_oner = false;
                self.is_baslat("İndiriliyor ve doğrulanıyor…", ui.ctx(), || IsSonucu::Kurulum(guncelleme::indir_ve_kur()));
            }
            Some("yeniden") => {
                self.yeniden_baslat_oner = false;
                self.is_baslat("Çoklu UYAP yeniden başlatılıyor…", ui.ctx(), || {
                    IsSonucu::Islem(guncelleme::cokluuyap_yeniden_baslat(), "Çoklu UYAP yeni sürümle açılıyor.".into())
                });
            }
            Some("sonra") => self.yeniden_baslat_oner = false,
            Some("imaja-sor") => self.imaja_don_sor = true,
            Some("imaja-vazgec") => self.imaja_don_sor = false,
            Some("imaja-don") => {
                self.imaja_don_sor = false;
                self.is_baslat("İmajdaki sürüme geçiliyor…", ui.ctx(), || {
                    let r = guncelleme::imaja_don().and_then(|_| {
                        if guncelleme::cokluuyap_calisiyor() {
                            guncelleme::cokluuyap_yeniden_baslat()
                        } else {
                            Ok(())
                        }
                    });
                    IsSonucu::Islem(r, "Bu açılışta Çoklu UYAP imajdaki sürümle açılacak.".into())
                });
            }
            _ => {}
        }
    }

    fn gorunum(&mut self, ui: &mut egui::Ui) {
        ui.heading("Görünüm");
        let mut secilen: Option<sistem::Tema> = None;
        kart(ui, |ui| {
            ui.label(RichText::new("Tema").strong());
            aciklama(ui, "Üst çubuk, dock, pencere çerçeveleri, menü, bildirimler ve bu pencere seçilen temaya geçer. Çoklu UYAP da aynı temayla açılır.");
            ui.add_space(4.0);
            for t in &self.temalar {
                let (alan, cevap) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(360.0), 34.0), egui::Sense::click());
                let c = r();
                if cevap.hovered() || t.kimlik == self.tema {
                    ui.painter().rect_filled(alan, 8.0, if t.kimlik == self.tema { c.vurgu_zemin } else { c.hap });
                }
                // Örnek: tema zemini üstünde vurgu rengi (Çoklu UYAP'taki renkli nokta gibi)
                let m = egui::pos2(alan.min.x + 20.0, alan.center().y);
                ui.painter().circle_filled(m, 10.0, hex(t.renk[1]));
                ui.painter().circle_stroke(m, 10.0, egui::Stroke::new(1.0_f32, hex(t.renk[3])));
                ui.painter().circle_filled(m, 5.5, hex(t.renk[5]));
                ui.painter().text(egui::pos2(alan.min.x + 40.0, alan.center().y), egui::Align2::LEFT_CENTER, &t.ad,
                    egui::FontId::proportional(15.0), c.yazi);
                if t.kimlik == self.tema {
                    ui.painter().text(egui::pos2(alan.max.x - 12.0, alan.center().y), egui::Align2::RIGHT_CENTER, "✓",
                        egui::FontId::proportional(15.0), c.baslik);
                }
                if cevap.clicked() && t.kimlik != self.tema {
                    secilen = Some(t.clone());
                }
            }
        });
        if let Some(t) = secilen {
            let sonuc = sistem::tema_uygula(&t.kimlik);
            if sonuc.is_ok() {
                sistem::tema_yenile();
                renkleri_sec(&t);
                tema(ui.ctx());
                self.tema = t.kimlik.clone();
            }
            self.sonuc(sonuc, &format!("Tema: {}", t.ad));
        }
        if self.canli {
            kart(ui, |ui| aciklama(ui, "Deneme (canlı) sisteminde seçim bilgisayar kapanınca Armonia'ya döner."));
        }
    }

    fn hakkinda(&mut self, ui: &mut egui::Ui) {
        ui.heading("Hakkında");
        let b = self.bilgi.clone();
        kart(ui, |ui| {
            egui::Grid::new("bilgi").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                for (k, v) in [("Sistem", &b.sistem), ("İşlemci", &b.islemci), ("Bellek", &b.bellek), ("Disk", &b.disk), ("Çekirdek", &b.cekirdek)] {
                    ui.label(RichText::new(k).color(r().soluk));
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
