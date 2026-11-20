//! Armonia Hukuk Hesaplayıcı — egui arayüzü. Hesaplar `hukuk_hesap` kütüphanesinde; burası yalnız gösterir.
//! Claude, 2026-11-19 (hesap makinesi, H1 süre, H2 faiz, H3 vekalet ücreti). Harç sekmesi sonraki aşamada.
#![cfg_attr(windows, windows_subsystem = "windows")]

use chrono::{Datelike, Local, NaiveDate};
use egui::{Color32, RichText};
use hukuk_hesap::{faiz, ifade, kayit, para, sablon::SABLONLAR, sure, tarih, tatil, vekalet};

const KREM: Color32 = Color32::from_rgb(243, 238, 230);
const KART: Color32 = Color32::from_rgb(252, 250, 246);
const YESIL: Color32 = Color32::from_rgb(95, 122, 97);
const ACIK_YESIL: Color32 = Color32::from_rgb(124, 154, 126);
const KENAR: Color32 = Color32::from_rgb(226, 218, 203);
const YAZI: Color32 = Color32::from_rgb(47, 47, 47);
const UYARI: Color32 = Color32::from_rgb(160, 82, 45);

#[derive(PartialEq, Clone, Copy)]
enum FaizTuru {
    Yasal,
    Ticari,
    Sozlesmesel,
}

struct FaizKalemi {
    ad: String,
    tutar: String,
    tarih: String,
}

#[derive(PartialEq, Clone, Copy)]
enum VekaletIsi {
    Dava,
    Icra,
    Arabuluculuk,
}

#[derive(PartialEq, Clone, Copy)]
enum Sekme {
    Makine,
    Sure,
    Faiz,
    Vekalet,
    Harc,
}

struct Uygulama {
    sekme: Sekme,
    // Süre
    sablon: Option<usize>,
    tarih_metni: String,
    miktar: u32,
    birim: sure::Birim,
    tebligat: sure::Tebligat,
    adli_tatil: bool,
    ayar: sure::Ayarlar,
    // Tatiller
    takvim: tatil::Takvim,
    tatil_yili: i32,
    yeni_tatil_tarih: String,
    yeni_tatil_ad: String,
    yeni_tatil_yarim: bool,
    durum_mesaji: Option<(bool, String)>,
    // Vekalet ücreti
    tarifeler: Vec<vekalet::Tarife>,
    vek_isi: VekaletIsi,
    vek_kabul: String,
    vek_red: String,
    vek_mahkeme: usize,
    vek_maddi: bool,
    vek_on_inceleme: bool,
    vek_icra: String,
    vek_odeme: bool,
    vek_arb: String,
    vek_anlasma: bool,
    maktu_ara: String,
    // Hesap makinesi
    ifade: String,
    gecmis: Vec<(String, String)>,
    // Faiz
    faiz_kalemleri: Vec<FaizKalemi>,
    faiz_bitis: String,
    faiz_turu: FaizTuru,
    avans_kipi: faiz::AvansKipi,
    sozlesme_orani: String,
    faiz_verisi: faiz::Veri,
    kullanici_oranlari: Vec<faiz::TcmbOrani>,
    yeni_oran_tarih: String,
    yeni_oran_reeskont: String,
    yeni_oran_avans: String,
    oran_mesaji: Option<(bool, String)>,
}

impl Uygulama {
    fn yeni(cc: &eframe::CreationContext<'_>) -> Self {
        tema(&cc.egui_ctx);
        let bugun = Local::now().date_naive();
        let (kullanici, mesaj) = match kayit::tatilleri_oku() {
            Ok(v) => (v, None),
            Err(e) => (Vec::new(), Some((false, e))),
        };
        let (oranlar, oran_mesaji) = match kayit::oranlari_oku() {
            Ok(v) => (v, None),
            Err(e) => (Vec::new(), Some((false, e))),
        };
        let mut faiz_verisi = faiz::Veri::gomulu();
        faiz_verisi.kullanici_ekle(oranlar.clone());
        Uygulama {
            faiz_kalemleri: vec![FaizKalemi { ad: String::new(), tutar: String::new(), tarih: String::new() }],
            faiz_bitis: tarih::kisa(bugun),
            faiz_turu: FaizTuru::Yasal,
            avans_kipi: faiz::AvansKipi::Kanun,
            sozlesme_orani: String::new(),
            faiz_verisi,
            kullanici_oranlari: oranlar,
            yeni_oran_tarih: String::new(),
            yeni_oran_reeskont: String::new(),
            yeni_oran_avans: String::new(),
            oran_mesaji,
            tarifeler: vekalet::tarifeler(),
            vek_isi: VekaletIsi::Dava,
            vek_kabul: String::new(),
            vek_red: String::new(),
            vek_mahkeme: 1,
            vek_maddi: false,
            vek_on_inceleme: false,
            vek_icra: String::new(),
            vek_odeme: false,
            vek_arb: String::new(),
            vek_anlasma: true,
            maktu_ara: String::new(),
            ifade: String::new(),
            gecmis: Vec::new(),
            sekme: Sekme::Makine,
            sablon: None,
            tarih_metni: tarih::kisa(bugun),
            miktar: 2,
            birim: sure::Birim::Hafta,
            tebligat: sure::Tebligat::Fiziki,
            adli_tatil: true,
            ayar: sure::Ayarlar::default(),
            takvim: tatil::Takvim::yeni(kullanici),
            tatil_yili: bugun.year(),
            yeni_tatil_tarih: String::new(),
            yeni_tatil_ad: String::new(),
            yeni_tatil_yarim: false,
            durum_mesaji: mesaj,
        }
    }

    fn kaydet(&mut self) {
        self.durum_mesaji = Some(match kayit::tatilleri_yaz(self.takvim.kullanici_tatilleri()) {
            Ok(()) => (true, "Tatiller kaydedildi.".into()),
            Err(e) => (false, format!("Kaydedilemedi: {e}")),
        });
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
    v.hyperlink_color = YESIL;
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
    egui::Frame::none()
        .fill(KART)
        .stroke(egui::Stroke::new(1.0_f32, KENAR))
        .rounding(14.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            icerik(ui)
        });
}

impl eframe::App for Uygulama {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("ust").frame(egui::Frame::none().fill(KART).inner_margin(12.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Hukuk Hesaplayıcı").size(20.0).strong().color(YESIL));
                ui.add_space(24.0);
                for (s, ad, hazir) in [
                    (Sekme::Makine, "Hesap Makinesi", true),
                    (Sekme::Sure, "Süre", true),
                    (Sekme::Faiz, "Faiz", true),
                    (Sekme::Vekalet, "Vekalet Ücreti", true),
                    (Sekme::Harc, "Harç ve Masraf", false),
                ] {
                    let r = ui.add_enabled(hazir, egui::SelectableLabel::new(self.sekme == s, ad));
                    if r.clicked() {
                        self.sekme = s;
                    }
                    if !hazir {
                        r.on_disabled_hover_text("Sonraki aşamada");
                    }
                }
            });
        });
        egui::TopBottomPanel::bottom("alt").frame(egui::Frame::none().fill(KREM).inner_margin(8.0)).show(ctx, |ui| {
            ui.label(
                RichText::new("Hesaplar bilgilendirme amaçlıdır; güncel mevzuatı ve içtihadı kontrol edin.")
                    .size(12.0)
                    .color(Color32::from_rgb(110, 105, 95)),
            );
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.sekme {
                Sekme::Makine => self.makine_ekrani(ui),
                Sekme::Sure => self.sure_ekrani(ui),
                Sekme::Faiz => self.faiz_ekrani(ui),
                Sekme::Vekalet => self.vekalet_ekrani(ui),
                _ => {}
            });
        });
    }
}

impl Uygulama {
    fn sure_ekrani(&mut self, ui: &mut egui::Ui) {
        let genis = ui.available_width() > 820.0;
        let girdi_bolumu = |this: &mut Self, ui: &mut egui::Ui| {
            kart(ui, |ui| {
                ui.heading("Süre");
                egui::Grid::new("sure_girdi").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                    ui.label("Hazır süre");
                    let secili = this.sablon.map(|i| SABLONLAR[i].ad).unwrap_or("Serbest (elle)");
                    egui::ComboBox::from_id_salt("sablon").selected_text(secili).width(300.0).show_ui(ui, |ui| {
                        if ui.selectable_label(this.sablon.is_none(), "Serbest (elle)").clicked() {
                            this.sablon = None;
                        }
                        for (i, s) in SABLONLAR.iter().enumerate() {
                            let ad = format!("{} — {} {} ({})", s.ad, s.miktar, s.birim.ad(), s.dayanak);
                            if ui.selectable_label(this.sablon == Some(i), ad).clicked() {
                                this.sablon = Some(i);
                                this.miktar = s.miktar;
                                this.birim = s.birim;
                                this.adli_tatil = s.adli_tatil;
                            }
                        }
                    });
                    ui.end_row();

                    ui.label(match this.tebligat {
                        sure::Tebligat::Fiziki => "Tebliğ / öğrenme tarihi",
                        sure::Tebligat::Elektronik => "E-tebligatın ulaştığı tarih",
                    });
                    ui.horizontal(|ui| {
                        let gecerli = tarih::coz(&this.tarih_metni).is_some();
                        ui.add(
                            egui::TextEdit::singleline(&mut this.tarih_metni)
                                .hint_text("gg.aa.yyyy")
                                .desired_width(110.0)
                                .text_color(if gecerli { YAZI } else { UYARI }),
                        );
                        if ui.button("Bugün").clicked() {
                            this.tarih_metni = tarih::kisa(Local::now().date_naive());
                        }
                        if let Some(t) = tarih::coz(&this.tarih_metni) {
                            ui.label(RichText::new(tarih::gun_adi(t.weekday())).color(Color32::GRAY));
                        }
                    });
                    ui.end_row();

                    ui.label("Tebligat");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut this.tebligat, sure::Tebligat::Fiziki, "Fiziki / tefhim");
                        ui.radio_value(&mut this.tebligat, sure::Tebligat::Elektronik, "Elektronik (+5 gün)");
                    });
                    ui.end_row();

                    ui.label("Süre");
                    ui.horizontal(|ui| {
                        let eski = (this.miktar, this.birim, this.adli_tatil);
                        ui.add(egui::DragValue::new(&mut this.miktar).range(1..=3650));
                        egui::ComboBox::from_id_salt("birim").selected_text(this.birim.ad()).show_ui(ui, |ui| {
                            for b in [sure::Birim::Gun, sure::Birim::Hafta, sure::Birim::Ay, sure::Birim::Yil] {
                                ui.selectable_value(&mut this.birim, b, b.ad());
                            }
                        });
                        ui.checkbox(&mut this.adli_tatil, "Adli tatile tabi");
                        if let Some(i) = this.sablon {
                            let s = SABLONLAR[i];
                            if eski != (this.miktar, this.birim, this.adli_tatil)
                                && (this.miktar, this.birim, this.adli_tatil) != (s.miktar, s.birim, s.adli_tatil)
                            {
                                this.sablon = None; // elle değiştirildi
                            }
                        }
                    });
                    ui.end_row();

                    ui.label("Ayar");
                    ui.checkbox(&mut this.ayar.yarim_gun_uzatir, "Yarım gün tatil (arife, 28 Ekim) süreyi uzatır");
                    ui.end_row();
                });
                if let Some(i) = this.sablon {
                    if !SABLONLAR[i].dogrulandi {
                        ui.label(
                            RichText::new(format!(
                                "Bu hazır süre ({}) henüz doğrulanmadı; maddeyi kontrol edin.",
                                SABLONLAR[i].dayanak
                            ))
                            .color(UYARI)
                            .size(13.0),
                        );
                    }
                }
            });
        };
        let sonuc_bolumu = |this: &mut Self, ui: &mut egui::Ui| {
            kart(ui, |ui| {
                let Some(bas) = tarih::coz(&this.tarih_metni) else {
                    ui.label(RichText::new("Tarihi gg.aa.yyyy biçiminde girin.").color(UYARI));
                    return;
                };
                let g = sure::Girdi {
                    baslangic: bas,
                    miktar: this.miktar,
                    birim: this.birim,
                    tebligat: this.tebligat,
                    adli_tatil: this.adli_tatil,
                };
                match sure::hesapla(&g, &this.takvim, this.ayar) {
                    Err(e) => {
                        ui.label(RichText::new(e).color(UYARI));
                    }
                    Ok(s) => {
                        ui.label(RichText::new("Sürenin son günü").color(Color32::GRAY));
                        ui.label(RichText::new(tarih::uzun(s.son_gun)).size(26.0).strong().color(YESIL));
                        let kalan = (s.son_gun - Local::now().date_naive()).num_days();
                        ui.label(match kalan {
                            k if k > 0 => format!("Bugünden {k} gün sonra"),
                            0 => "Bugün".to_string(),
                            k => format!("{} gün önce geçti", -k),
                        });
                        ui.separator();
                        for (i, a) in s.adimlar.iter().enumerate() {
                            ui.label(format!("{}. {}", i + 1, a));
                        }
                        for u in &s.uyarilar {
                            ui.label(RichText::new(format!("⚠ {u}")).color(UYARI).size(13.0));
                        }
                        ui.add_space(4.0);
                        if ui.button("Dökümü kopyala").clicked() {
                            ui.ctx().copy_text(sure::metin(&g, &s));
                        }
                    }
                }
            });
        };
        if genis {
            ui.columns(2, |c| {
                girdi_bolumu(self, &mut c[0]);
                sonuc_bolumu(self, &mut c[1]);
            });
        } else {
            girdi_bolumu(self, ui);
            sonuc_bolumu(self, ui);
        }
        ui.add_space(8.0);
        self.tatil_bolumu(ui);
    }

    fn tatil_bolumu(&mut self, ui: &mut egui::Ui) {
        kart(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new("Resmî tatiller").strong()).default_open(false).show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Kanuni tatiller otomatik gelir. Dini bayramları ve arifeleri her yıl buradan ekleyin \
                         (otomatik güncelleme sonraki sürümde).",
                    )
                    .size(13.0),
                );
                ui.horizontal(|ui| {
                    if ui.button("◀").clicked() {
                        self.tatil_yili -= 1;
                    }
                    ui.label(RichText::new(self.tatil_yili.to_string()).strong());
                    if ui.button("▶").clicked() {
                        self.tatil_yili += 1;
                    }
                });
                let mut silinecek: Option<NaiveDate> = None;
                egui::Grid::new("tatiller").striped(true).num_columns(3).show(ui, |ui| {
                    for t in self.takvim.yil_tatilleri(self.tatil_yili) {
                        ui.label(tarih::uzun(t.tarih));
                        ui.label(if t.yarim { format!("{} (yarım gün)", t.ad) } else { t.ad.clone() });
                        if t.kaynak == tatil::Kaynak::Kullanici {
                            if ui.small_button("Sil").clicked() {
                                silinecek = Some(t.tarih);
                            }
                        } else {
                            ui.label(RichText::new("kanun").size(12.0).color(Color32::GRAY));
                        }
                        ui.end_row();
                    }
                });
                if let Some(t) = silinecek {
                    self.takvim.sil(t);
                    self.kaydet();
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.yeni_tatil_tarih).hint_text("gg.aa.yyyy").desired_width(110.0));
                    ui.add(egui::TextEdit::singleline(&mut self.yeni_tatil_ad).hint_text("Ad (ör. Kurban Bayramı 1. gün)").desired_width(260.0));
                    ui.checkbox(&mut self.yeni_tatil_yarim, "Yarım gün (arife)");
                    let t = tarih::coz(&self.yeni_tatil_tarih);
                    if ui.add_enabled(t.is_some() && !self.yeni_tatil_ad.trim().is_empty(), egui::Button::new("Ekle")).clicked() {
                        let t = t.unwrap();
                        self.takvim.ekle(tatil::Tatil {
                            tarih: t,
                            ad: self.yeni_tatil_ad.trim().to_string(),
                            yarim: self.yeni_tatil_yarim,
                            kaynak: tatil::Kaynak::Kullanici,
                        });
                        self.tatil_yili = t.year();
                        self.yeni_tatil_tarih.clear();
                        self.yeni_tatil_ad.clear();
                        self.yeni_tatil_yarim = false;
                        self.kaydet();
                    }
                });
                if let Some((iyi, m)) = &self.durum_mesaji {
                    ui.label(RichText::new(m).size(13.0).color(if *iyi { YESIL } else { UYARI }));
                }
            });
        });
    }
}

impl Uygulama {
    fn faiz_girdisi(&self) -> Result<faiz::Girdi, String> {
        let bitis = tarih::coz(&self.faiz_bitis).ok_or("Bitiş tarihini gg.aa.yyyy biçiminde girin.")?;
        let mut kalemler = Vec::new();
        for (i, k) in self.faiz_kalemleri.iter().enumerate() {
            let ad = if k.ad.trim().is_empty() { format!("{}. kalem", i + 1) } else { k.ad.trim().to_string() };
            let anapara = para::coz(&k.tutar).ok_or(format!("{ad}: tutarı girin (ör. 125.000,50)."))?;
            let baslangic = tarih::coz(&k.tarih).ok_or(format!("{ad}: başlangıç tarihini gg.aa.yyyy biçiminde girin."))?;
            kalemler.push(faiz::Kalem { ad, anapara, baslangic });
        }
        let tur = match self.faiz_turu {
            FaizTuru::Yasal => faiz::Tur::Yasal,
            FaizTuru::Ticari => faiz::Tur::Ticari(self.avans_kipi),
            FaizTuru::Sozlesmesel => faiz::Tur::Sozlesmesel(
                para::oran_coz(&self.sozlesme_orani).filter(|o| *o > 0).ok_or("Sözleşmedeki yıllık oranı girin (ör. 36 ya da 42,5).")?,
            ),
        };
        Ok(faiz::Girdi { kalemler, bitis, tur })
    }

    fn faiz_ekrani(&mut self, ui: &mut egui::Ui) {
        let genis = ui.available_width() > 820.0;
        let girdi_bolumu = |this: &mut Self, ui: &mut egui::Ui| {
            kart(ui, |ui| {
                ui.heading("Faiz");
                ui.label("Faiz türü");
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut this.faiz_turu, FaizTuru::Yasal, "Yasal (3095 m.1)");
                    ui.radio_value(&mut this.faiz_turu, FaizTuru::Ticari, "Ticari / avans (m.2/2)");
                    ui.radio_value(&mut this.faiz_turu, FaizTuru::Sozlesmesel, "Sözleşmesel");
                });
                match this.faiz_turu {
                    FaizTuru::Ticari => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Avans oranı değişimi:");
                            ui.radio_value(&mut this.avans_kipi, faiz::AvansKipi::Kanun, "Kanun metni (31 Aralık / 30 Haziran)")
                                .on_hover_text("3095 m.2/2: önceki yılın 31 Aralık oranı; 30 Haziran oranı 5 puan ya da daha çok farklıysa yılın ikinci yarısında o oran.");
                            ui.radio_value(&mut this.avans_kipi, faiz::AvansKipi::TcmbTarihleri, "TCMB değişiklik tarihleri")
                                .on_hover_text("TCMB'nin her oran değişikliği yürürlük gününden itibaren uygulanır (bazı resmî tablolar böyle).");
                        });
                    }
                    FaizTuru::Sozlesmesel => {
                        ui.horizontal(|ui| {
                            ui.label("Yıllık oran %");
                            ui.add(egui::TextEdit::singleline(&mut this.sozlesme_orani).hint_text("ör. 36").desired_width(70.0));
                        });
                    }
                    FaizTuru::Yasal => {}
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Faizin hesaplanacağı gün");
                    let gecerli = tarih::coz(&this.faiz_bitis).is_some();
                    ui.add(
                        egui::TextEdit::singleline(&mut this.faiz_bitis)
                            .hint_text("gg.aa.yyyy")
                            .desired_width(110.0)
                            .text_color(if gecerli { YAZI } else { UYARI }),
                    );
                    if ui.button("Bugün").clicked() {
                        this.faiz_bitis = tarih::kisa(Local::now().date_naive());
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Alacak kalemleri").strong());
                let mut silinecek = None;
                let tek = this.faiz_kalemleri.len() == 1;
                // Grid yerine sabit genişlik: egui Grid'i sütunları ilk karede başlıklara göre daraltıyor.
                let (w_ad, w_tutar, w_tarih) = (160.0, 130.0, 110.0);
                ui.horizontal(|ui| {
                    for (m, w) in [("Açıklama", w_ad), ("Tutar (TL)", w_tutar), ("Faiz başlangıcı", w_tarih)] {
                        ui.add_sized([w, 16.0], egui::Label::new(RichText::new(m).size(13.0).color(Color32::GRAY)));
                    }
                });
                for (i, k) in this.faiz_kalemleri.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized([w_ad, 24.0], egui::TextEdit::singleline(&mut k.ad).hint_text(format!("{}. kalem", i + 1)));
                        let tutar_iyi = k.tutar.is_empty() || para::coz(&k.tutar).is_some();
                        ui.add_sized(
                            [w_tutar, 24.0],
                            egui::TextEdit::singleline(&mut k.tutar).hint_text("125.000,00").text_color(if tutar_iyi { YAZI } else { UYARI }),
                        );
                        let tarih_iyi = k.tarih.is_empty() || tarih::coz(&k.tarih).is_some();
                        ui.add_sized(
                            [w_tarih, 24.0],
                            egui::TextEdit::singleline(&mut k.tarih).hint_text("gg.aa.yyyy").text_color(if tarih_iyi { YAZI } else { UYARI }),
                        );
                        if ui.add_enabled(!tek, egui::Button::new("Sil").small()).clicked() {
                            silinecek = Some(i);
                        }
                    });
                }
                if let Some(i) = silinecek {
                    this.faiz_kalemleri.remove(i);
                }
                if ui.button("+ Kalem ekle").clicked() {
                    let tarih = this.faiz_kalemleri.last().map(|k| k.tarih.clone()).unwrap_or_default();
                    this.faiz_kalemleri.push(FaizKalemi { ad: String::new(), tutar: String::new(), tarih });
                }
            });
        };
        let sonuc_bolumu = |this: &mut Self, ui: &mut egui::Ui| {
            kart(ui, |ui| {
                let g = match this.faiz_girdisi() {
                    Ok(g) => g,
                    Err(e) => {
                        ui.label(RichText::new(e).color(UYARI));
                        return;
                    }
                };
                match faiz::hesapla(&g, &this.faiz_verisi) {
                    Err(e) => {
                        ui.label(RichText::new(e).color(UYARI));
                    }
                    Ok(s) => {
                        ui.label(RichText::new("Toplam faiz").color(Color32::GRAY));
                        ui.label(RichText::new(para::yaz(s.toplam_faiz)).size(26.0).strong().color(YESIL));
                        ui.label(format!(
                            "Anapara {} + faiz = {}",
                            para::yaz(s.toplam_anapara),
                            para::yaz(s.toplam_anapara + s.toplam_faiz)
                        ));
                        ui.separator();
                        for (ki, k) in s.kalemler.iter().enumerate() {
                            ui.label(RichText::new(format!(
                                "{} — {} ({} tarihinden)",
                                k.kalem.ad,
                                para::yaz(k.kalem.anapara),
                                tarih::kisa(k.kalem.baslangic)
                            ))
                            .strong());
                            egui::Grid::new(("donem", ki)).striped(true).num_columns(4).show(ui, |ui| {
                                for d in &k.donemler {
                                    ui.label(format!("{} – {}", tarih::kisa(d.baslangic), tarih::kisa(d.bitis)));
                                    ui.label(format!("{} gün", d.gun));
                                    ui.label(format!("%{}", para::oran_yaz(d.oran))).on_hover_text(&d.aciklama);
                                    ui.label(para::yaz(d.faiz));
                                    ui.end_row();
                                }
                            });
                            if k.donemler.len() > 1 {
                                ui.label(format!("Kalem faizi: {}", para::yaz(k.faiz)));
                            }
                            ui.add_space(4.0);
                        }
                        ui.label(RichText::new("Basit faiz, 365 gün esası; başlangıç günü sayılır, son gün sayılmaz. Oranın dayanağı için orana gelin.").size(12.0).color(Color32::GRAY));
                        for u in &s.uyarilar {
                            ui.label(RichText::new(format!("⚠ {u}")).color(UYARI).size(13.0));
                        }
                        ui.add_space(4.0);
                        if ui.button("Dökümü kopyala").clicked() {
                            ui.ctx().copy_text(faiz::metin(&g, &s));
                        }
                    }
                }
            });
        };
        if genis {
            ui.columns(2, |c| {
                girdi_bolumu(self, &mut c[0]);
                sonuc_bolumu(self, &mut c[1]);
            });
        } else {
            girdi_bolumu(self, ui);
            sonuc_bolumu(self, ui);
        }
        ui.add_space(8.0);
        self.oran_bolumu(ui);
    }

    fn oranlari_kaydet(&mut self) {
        let mut v = faiz::Veri::gomulu();
        v.kullanici_ekle(self.kullanici_oranlari.clone());
        self.faiz_verisi = v;
        self.oran_mesaji = Some(match kayit::oranlari_yaz(&self.kullanici_oranlari) {
            Ok(()) => (true, "Oranlar kaydedildi.".into()),
            Err(e) => (false, format!("Kaydedilemedi: {e}")),
        });
    }

    fn oran_bolumu(&mut self, ui: &mut egui::Ui) {
        kart(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new("Oranlar").strong()).default_open(false).show(ui, |ui| {
                let (baslik, cizelge) = match self.faiz_turu {
                    FaizTuru::Ticari => ("Ticari temerrüt faizi", faiz::ticari_cizelge(&self.faiz_verisi, self.avans_kipi)),
                    _ => ("Yasal faiz", faiz::yasal_cizelge(&self.faiz_verisi)),
                };
                ui.label(RichText::new(format!("{baslik} — programın kullandığı oranlar")).strong());
                egui::Grid::new("cizelge").striped(true).num_columns(3).show(ui, |ui| {
                    let n = cizelge.dilimler.len();
                    // Son 12 dönem yeter; eskiler kaydırmayı uzatmasın
                    for (i, d) in cizelge.dilimler.iter().enumerate().skip(n.saturating_sub(12)) {
                        let bitis = cizelge.dilimler.get(i + 1).map(|x| x.baslangic).or(cizelge.son);
                        ui.label(match bitis {
                            Some(b) => format!("{} – {}", tarih::kisa(d.baslangic), tarih::kisa(b.pred_opt().unwrap_or(b))),
                            None => format!("{} –", tarih::kisa(d.baslangic)),
                        });
                        ui.label(RichText::new(format!("%{}", para::oran_yaz(d.oran))).strong());
                        ui.label(RichText::new(&d.aciklama).size(12.0));
                        ui.end_row();
                    }
                });
                if let Some(s) = cizelge.son {
                    ui.label(RichText::new(format!("{} ve sonrası için oran henüz bilinmiyor.", tarih::kisa(s))).size(13.0).color(UYARI));
                }
                ui.label(
                    RichText::new(format!(
                        "TCMB oranları: {} · Veri tarihi {}",
                        self.faiz_verisi.tcmb_kaynak,
                        tarih::kisa(self.faiz_verisi.veri_tarihi)
                    ))
                    .size(12.0)
                    .color(Color32::GRAY),
                );
                ui.separator();
                ui.label(
                    RichText::new(
                        "TCMB yeni reeskont / avans oranı açıkladığında buradan ekleyin (otomatik güncelleme sonraki \
                         sürümde). Eklediğiniz tarihe kadar başka değişiklik olmadığı kabul edilir.",
                    )
                    .size(13.0),
                );
                let mut silinecek = None;
                for (i, o) in self.kullanici_oranlari.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{} · reeskont %{} · avans %{}",
                            tarih::kisa(o.tarih),
                            para::oran_yaz(para::oran_birim(o.reeskont)),
                            para::oran_yaz(para::oran_birim(o.avans))
                        ));
                        if ui.small_button("Sil").clicked() {
                            silinecek = Some(i);
                        }
                    });
                }
                if let Some(i) = silinecek {
                    self.kullanici_oranlari.remove(i);
                    self.oranlari_kaydet();
                }
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.yeni_oran_tarih).hint_text("yürürlük gg.aa.yyyy").desired_width(140.0));
                    ui.add(egui::TextEdit::singleline(&mut self.yeni_oran_reeskont).hint_text("reeskont %").desired_width(90.0));
                    ui.add(egui::TextEdit::singleline(&mut self.yeni_oran_avans).hint_text("avans %").desired_width(90.0));
                    let t = tarih::coz(&self.yeni_oran_tarih);
                    let r = para::oran_coz(&self.yeni_oran_reeskont);
                    let a = para::oran_coz(&self.yeni_oran_avans);
                    if ui.add_enabled(t.is_some() && r.is_some() && a.is_some(), egui::Button::new("Ekle")).clicked() {
                        let birim = para::ORAN_BIRIM as f64;
                        let tarih = t.unwrap();
                        self.kullanici_oranlari.retain(|o| o.tarih != tarih);
                        self.kullanici_oranlari.push(faiz::TcmbOrani {
                            tarih,
                            reeskont: r.unwrap() as f64 / birim,
                            avans: a.unwrap() as f64 / birim,
                            kullanici: true,
                        });
                        self.kullanici_oranlari.sort_by_key(|o| o.tarih);
                        self.yeni_oran_tarih.clear();
                        self.yeni_oran_reeskont.clear();
                        self.yeni_oran_avans.clear();
                        self.oranlari_kaydet();
                    }
                });
                if let Some((iyi, m)) = &self.oran_mesaji {
                    ui.label(RichText::new(m).size(13.0).color(if *iyi { YESIL } else { UYARI }));
                }
            });
        });
    }
}

impl Uygulama {
    fn esittir(&mut self) {
        if let Ok(v) = ifade::hesapla(&self.ifade) {
            let sonuc = ifade::yaz(v);
            self.gecmis.insert(0, (self.ifade.trim().to_string(), sonuc.clone()));
            self.gecmis.truncate(12);
            self.ifade = sonuc;
        }
    }

    fn makine_ekrani(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.set_max_width(420.0);
            kart(ui, |ui| {
                let anlik = ifade::hesapla(&self.ifade);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.ifade)
                        .id_salt("ifade")
                        .font(egui::FontId::proportional(24.0))
                        .hint_text("0")
                        .horizontal_align(egui::Align::Max)
                        .desired_width(f32::INFINITY),
                );
                if !r.has_focus() && !ui.ctx().memory(|m| m.focused().is_some()) {
                    r.request_focus();
                }
                let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                if enter {
                    self.esittir();
                    r.request_focus();
                }
                if esc {
                    self.ifade.clear();
                }
                let gen = ui.available_width();
                ui.allocate_ui_with_layout(egui::vec2(gen, 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let m = match (&anlik, self.ifade.trim().is_empty()) {
                        (_, true) => RichText::new(" "),
                        (Ok(v), false) => RichText::new(format!("= {}", ifade::yaz(*v))).color(YESIL).size(18.0),
                        (Err(e), false) => RichText::new(e).color(Color32::GRAY).size(13.0),
                    };
                    ui.label(m);
                });
                ui.add_space(4.0);
                let tuslar = [
                    ["C", "(", ")", "÷"],
                    ["7", "8", "9", "×"],
                    ["4", "5", "6", "−"],
                    ["1", "2", "3", "+"],
                    ["%", "0", ",", "="],
                ];
                let w = (ui.available_width() - 3.0 * 8.0) / 4.0;
                for satir in tuslar {
                    ui.horizontal(|ui| {
                        for t in satir {
                            let islem = matches!(t, "÷" | "×" | "−" | "+" | "=");
                            let yazi = RichText::new(t).size(20.0);
                            let yazi = if t == "=" { yazi.color(Color32::WHITE) } else { yazi };
                            let mut b = egui::Button::new(yazi).rounding(10.0);
                            if t == "=" {
                                b = b.fill(YESIL);
                            } else if islem {
                                b = b.fill(Color32::from_rgb(230, 235, 227));
                            }
                            let r = ui.add_sized([w, 48.0], b);
                            if r.clicked() {
                                match t {
                                    "C" => self.ifade.clear(),
                                    "=" => self.esittir(),
                                    _ => self.ifade.push_str(t),
                                }
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Son karakteri sil").clicked() {
                        self.ifade.pop();
                    }
                    if let Ok(v) = anlik {
                        if ui.button("Sonucu kopyala").clicked() {
                            ui.ctx().copy_text(ifade::yaz(v));
                        }
                    }
                });
                ui.label(
                    RichText::new("Klavyeyle de yazılabilir: * / + - ( ) %, ondalık için virgül. Enter = sonuç, Esc = temizle.")
                        .size(12.0)
                        .color(Color32::GRAY),
                );
            });
            if !self.gecmis.is_empty() {
                ui.add_space(8.0);
                kart(ui, |ui| {
                    ui.label(RichText::new("Geçmiş").strong());
                    let mut geri = None;
                    for (i, (a, b)) in self.gecmis.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(a).color(Color32::GRAY));
                            ui.label(format!("= {b}"));
                            if ui.small_button("Kullan").clicked() {
                                geri = Some(i);
                            }
                        });
                    }
                    if let Some(i) = geri {
                        self.ifade = self.gecmis[i].1.clone();
                    }
                });
            }
        });
    }
}

/// Türkçe küçük harf (İ → i, I → ı); arama için.
fn kucuk(s: &str) -> String {
    s.replace('İ', "i").replace('I', "ı").to_lowercase()
}

fn para_kutusu(ui: &mut egui::Ui, metin: &mut String) {
    let iyi = metin.is_empty() || para::coz(metin).is_some();
    ui.add(
        egui::TextEdit::singleline(metin)
            .hint_text("0,00")
            .desired_width(150.0)
            .text_color(if iyi { YAZI } else { UYARI }),
    );
}

impl Uygulama {
    fn vekalet_isi(&self) -> Result<vekalet::Is, String> {
        let bos_sifir = |m: &str| if m.trim().is_empty() { Some(0) } else { para::coz(m) };
        Ok(match self.vek_isi {
            VekaletIsi::Dava => vekalet::Is::Dava {
                kabul: bos_sifir(&self.vek_kabul).ok_or("Kabul edilen miktar anlaşılamadı.")?,
                red: bos_sifir(&self.vek_red).ok_or("Reddedilen miktar anlaşılamadı.")?,
                mahkeme: self.vek_mahkeme,
                maddi_tazminat: self.vek_maddi,
                on_inceleme_oncesi: self.vek_on_inceleme,
            },
            VekaletIsi::Icra => vekalet::Is::Icra {
                tutar: para::coz(&self.vek_icra).ok_or("Takip tutarını girin.")?,
                odeme_suresinde: self.vek_odeme,
            },
            VekaletIsi::Arabuluculuk => vekalet::Is::Arabuluculuk {
                tutar: para::coz(&self.vek_arb).ok_or("Uyuşmazlık tutarını girin.")?,
                anlasma: self.vek_anlasma,
            },
        })
    }

    fn vekalet_ekrani(&mut self, ui: &mut egui::Ui) {
        let Some(tarife) = self.tarifeler.last().cloned() else {
            ui.label(RichText::new("Tarife verisi yok.").color(UYARI));
            return;
        };
        let genis = ui.available_width() > 820.0;
        let girdi_bolumu = |this: &mut Self, ui: &mut egui::Ui| {
            kart(ui, |ui| {
                ui.heading("Vekalet Ücreti");
                ui.label(RichText::new(format!("{} · yürürlük {} · {}", tarife.ad, tarih::kisa(tarife.yururluk), tarife.kaynak)).size(12.0).color(Color32::GRAY));
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut this.vek_isi, VekaletIsi::Dava, "Dava (karşı vekalet)");
                    ui.radio_value(&mut this.vek_isi, VekaletIsi::Icra, "İcra takibi");
                    ui.radio_value(&mut this.vek_isi, VekaletIsi::Arabuluculuk, "Arabuluculuk");
                });
                ui.add_space(4.0);
                egui::Grid::new("vek_girdi").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| match this.vek_isi {
                    VekaletIsi::Dava => {
                        ui.label("Mahkeme");
                        let secili = tarife.mahkemeler.get(this.vek_mahkeme).map(|m| m.ad.clone()).unwrap_or_default();
                        egui::ComboBox::from_id_salt("mahkeme").selected_text(secili).width(300.0).show_ui(ui, |ui| {
                            for (i, m) in tarife.mahkemeler.iter().enumerate() {
                                ui.selectable_value(&mut this.vek_mahkeme, i, format!("{} — maktu {}", m.ad, para::yaz(m.tutar * 100)));
                            }
                        });
                        ui.end_row();
                        ui.label("Kabul edilen (TL)");
                        para_kutusu(ui, &mut this.vek_kabul);
                        ui.end_row();
                        ui.label("Reddedilen (TL)");
                        para_kutusu(ui, &mut this.vek_red);
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut this.vek_maddi, "Maddi tazminat davası (m.13/3)");
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut this.vek_on_inceleme, "Ön inceleme tutanağından önce sona erdi (m.6)");
                        ui.end_row();
                    }
                    VekaletIsi::Icra => {
                        ui.label("Takip tutarı (TL)");
                        para_kutusu(ui, &mut this.vek_icra);
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut this.vek_odeme, "Borçlu ödeme süresinde ödedi (m.11/4)");
                        ui.end_row();
                    }
                    VekaletIsi::Arabuluculuk => {
                        ui.label("Uyuşmazlık tutarı (TL)");
                        para_kutusu(ui, &mut this.vek_arb);
                        ui.end_row();
                        ui.label("Sonuç");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut this.vek_anlasma, true, "Anlaşma");
                            ui.radio_value(&mut this.vek_anlasma, false, "Anlaşmazlık");
                        });
                        ui.end_row();
                    }
                });
            });
        };
        let sonuc_bolumu = |this: &mut Self, ui: &mut egui::Ui| {
            kart(ui, |ui| {
                let sonuc = this.vekalet_isi().and_then(|is| vekalet::hesapla(&tarife, &is));
                match sonuc {
                    Err(e) => {
                        ui.label(RichText::new(e).color(UYARI));
                    }
                    Ok(s) => {
                        for k in &s.kalemler {
                            ui.label(RichText::new(&k.baslik).color(Color32::GRAY));
                            ui.label(RichText::new(para::yaz(k.ucret)).size(26.0).strong().color(YESIL));
                            if !k.dilimler.is_empty() {
                                egui::CollapsingHeader::new(format!("Dilimler ({})", para::yaz(k.esas))).id_salt(&k.baslik).show(ui, |ui| {
                                    egui::Grid::new(("dilim", &k.baslik)).striped(true).num_columns(3).show(ui, |ui| {
                                        for d in &k.dilimler {
                                            ui.label(para::yaz(d.tutar));
                                            ui.label(format!("%{}", para::oran_yaz(para::oran_birim(d.yuzde))));
                                            ui.label(para::yaz(d.ucret));
                                            ui.end_row();
                                        }
                                    });
                                });
                            }
                            for a in &k.adimlar {
                                ui.label(RichText::new(format!("• {a}")).size(14.0));
                            }
                            ui.separator();
                        }
                        for u in &s.uyarilar {
                            ui.label(RichText::new(format!("⚠ {u}")).color(UYARI).size(13.0));
                        }
                        ui.add_space(4.0);
                        if ui.button("Dökümü kopyala").clicked() {
                            ui.ctx().copy_text(vekalet::metin(&tarife, &s));
                        }
                    }
                }
            });
        };
        if genis {
            ui.columns(2, |c| {
                girdi_bolumu(self, &mut c[0]);
                sonuc_bolumu(self, &mut c[1]);
            });
        } else {
            girdi_bolumu(self, ui);
            sonuc_bolumu(self, ui);
        }
        ui.add_space(8.0);
        kart(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new("Maktu ücretler tablosu").strong()).default_open(false).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Ara");
                    ui.add(egui::TextEdit::singleline(&mut self.maktu_ara).hint_text("ör. icra, danışma, ağır ceza").desired_width(260.0));
                });
                let ara = kucuk(&self.maktu_ara);
                egui::Grid::new("maktu").striped(true).num_columns(3).show(ui, |ui| {
                    for (bolum, ad, tutar) in &tarife.maktu {
                        if !ara.is_empty() && !kucuk(ad).contains(&ara) && !kucuk(bolum).contains(&ara) {
                            continue;
                        }
                        ui.label(RichText::new(bolum).size(12.0).color(Color32::GRAY));
                        ui.label(ad);
                        ui.label(RichText::new(para::yaz(tutar * 100)).strong());
                        ui.end_row();
                    }
                });
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let secenekler = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hukuk Hesaplayıcı")
            .with_app_id("armonia-hukuk-hesap")
            .with_inner_size([1040.0, 720.0])
            .with_min_inner_size([560.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native("Hukuk Hesaplayıcı", secenekler, Box::new(|cc| Ok(Box::new(Uygulama::yeni(cc)))))
}
