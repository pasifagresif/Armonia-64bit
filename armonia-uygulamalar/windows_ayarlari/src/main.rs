//! Armonia Windows Programları: ağdaki Windows programlarını (İcramatik vb., BİRDEN FAZLA olabilir) listele,
//! ekle/düzenle/sil, çalıştır. Hiçbir program adı koda gömülü değildir — hepsi /etc/armonia/windows.apps.d/*.conf
//! okunur/yazılır. Yazma işlemi root gerektirdiği için `pkexec /opt/armonia/bin/pencere-yardimci` üzerinden
//! (ayar-yardimci ile aynı desen: dar, doğrulanmış alt komutlar). Claude, 2026-11-25.
use eframe::egui;
use std::fs;
use std::io::Write as _;
use std::process::{Command, Stdio};

const APPS_DIR: &str = "/etc/armonia/windows.apps.d";
const YARDIMCI: &str = "/opt/armonia/bin/pencere-yardimci";
const CALISTIR: &str = "/opt/armonia/bin/windows_calistir.sh";

#[derive(Clone, Default)]
struct Uygulama {
    kimlik: String,
    ad: String,
    paylasim: String,
    program: String,
    sha256: String,
}

fn deger_oku(icerik: &str, anahtar: &str) -> String {
    for satir in icerik.lines() {
        if let Some(gerisi) = satir.strip_prefix(&format!("{anahtar}=\"")) {
            if let Some(deger) = gerisi.strip_suffix('"') {
                return deger.to_string();
            }
        }
    }
    String::new()
}

fn uygulamalari_listele() -> Vec<Uygulama> {
    let mut liste = Vec::new();
    let Ok(girdiler) = fs::read_dir(APPS_DIR) else {
        return liste;
    };
    for girdi in girdiler.flatten() {
        let yol = girdi.path();
        let Some(ad_dosya) = yol.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(kimlik) = ad_dosya.strip_suffix(".conf") else {
            continue;
        }; // *.conf.ornek atlanir
        let Ok(icerik) = fs::read_to_string(&yol) else {
            continue;
        };
        liste.push(Uygulama {
            kimlik: kimlik.to_string(),
            ad: deger_oku(&icerik, "AD"),
            paylasim: deger_oku(&icerik, "PAYLASIM"),
            program: deger_oku(&icerik, "PROGRAM"),
            sha256: deger_oku(&icerik, "SHA256"),
        });
    }
    liste.sort_by(|a, b| a.ad.cmp(&b.ad));
    liste
}

/// `pkexec pencere-yardimci yaz <kimlik> -` çağırır; AD/PAYLASIM/PROGRAM/SHA256 stdin'den 4 satır.
fn kaydet(u: &Uygulama) -> Result<(), String> {
    let mut c = Command::new("pkexec")
        .args([YARDIMCI, "yaz", &u.kimlik, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("pkexec çalıştırılamadı: {e}"))?;
    if let Some(mut girdi) = c.stdin.take() {
        let _ = write!(
            girdi,
            "{}\n{}\n{}\n{}\n",
            u.ad, u.paylasim, u.program, u.sha256
        );
    }
    let cikti = c.wait_with_output().map_err(|e| e.to_string())?;
    if cikti.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&cikti.stderr).trim().to_string())
    }
}

fn sil(kimlik: &str) -> Result<(), String> {
    let cikti = Command::new("pkexec")
        .args([YARDIMCI, "sil", kimlik])
        .output()
        .map_err(|e| format!("pkexec çalıştırılamadı: {e}"))?;
    if cikti.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&cikti.stderr).trim().to_string())
    }
}

fn calistir(kimlik: &str) {
    let _ = Command::new(CALISTIR).arg(kimlik).spawn();
}

fn kimlik_gecerli(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s != "_ornek"
        && s != "yonetici"
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

struct UygulamaAyarlari {
    liste: Vec<Uygulama>,
    secili: Option<usize>,
    duzenleme: Option<Uygulama>,
    yeni_kimlik: String,
    mesaj: String,
    silme_onayi: Option<usize>,
}

impl Default for UygulamaAyarlari {
    fn default() -> Self {
        Self {
            liste: uygulamalari_listele(),
            secili: None,
            duzenleme: None,
            yeni_kimlik: String::new(),
            mesaj: String::new(),
            silme_onayi: None,
        }
    }
}

impl eframe::App for UygulamaAyarlari {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Windows Programları");
            ui.label(
                "Ağdaki paylaşılan klasörden çalışan Windows programları (İcramatik vb.). \
                 Birden fazla program eklenebilir; her birinin kendi paylaşımı ve kum havuzu vardır.",
            );
            ui.separator();

            if self.liste.is_empty() {
                ui.label("Henüz tanımlı Windows programı yok.");
            }

            let mut silme_istegi: Option<usize> = None;
            for (i, u) in self.liste.clone().iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(if u.ad.is_empty() { &u.kimlik } else { &u.ad });
                    ui.label(format!("({})", u.kimlik));
                    if u.paylasim.is_empty() {
                        ui.colored_label(egui::Color32::ORANGE, "Ayarlanmamış");
                    }
                    if ui.button("Çalıştır").clicked() {
                        calistir(&u.kimlik);
                    }
                    if ui.button("Düzenle").clicked() {
                        self.secili = Some(i);
                        self.duzenleme = Some(u.clone());
                        self.mesaj.clear();
                    }
                    if ui.button("Sil").clicked() {
                        self.silme_onayi = Some(i);
                    }
                });
                if self.silme_onayi == Some(i) {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::RED, format!("\"{}\" silinsin mi?", u.kimlik));
                        if ui.button("Evet, sil").clicked() {
                            silme_istegi = Some(i);
                        }
                        if ui.button("Vazgeç").clicked() {
                            self.silme_onayi = None;
                        }
                    });
                }
            }
            if let Some(i) = silme_istegi {
                let kimlik = self.liste[i].kimlik.clone();
                match sil(&kimlik) {
                    Ok(()) => {
                        self.mesaj = format!("Silindi: {kimlik}");
                        self.liste = uygulamalari_listele();
                    }
                    Err(e) => self.mesaj = format!("Silinemedi: {e}"),
                }
                self.silme_onayi = None;
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Yeni kimlik (örn. icramatik):");
                ui.text_edit_singleline(&mut self.yeni_kimlik);
                if ui.button("Ekle").clicked() {
                    if kimlik_gecerli(&self.yeni_kimlik) {
                        self.duzenleme = Some(Uygulama { kimlik: self.yeni_kimlik.clone(), ..Default::default() });
                        self.secili = None;
                        self.mesaj.clear();
                    } else {
                        self.mesaj = "Geçersiz kimlik: yalnız a-z, 0-9, '-', '_' (en fazla 32 karakter)".into();
                    }
                }
            });

            if !self.mesaj.is_empty() {
                ui.separator();
                ui.label(&self.mesaj);
            }
        });

        if let Some(mut duz) = self.duzenleme.clone() {
            let mut acik = true;
            let mut kaydedildi = false;
            egui::Window::new(format!("Düzenle: {}", duz.kimlik))
                .open(&mut acik)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Dock'ta görünen ad:");
                        ui.text_edit_singleline(&mut duz.ad);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Ağ paylaşımı (//IP/klasör):");
                        ui.text_edit_singleline(&mut duz.paylasim);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Program (paylaşıma göre, örn. İcramatik.exe):");
                        ui.text_edit_singleline(&mut duz.program);
                    });
                    ui.horizontal(|ui| {
                        ui.label("SHA256 (isteğe bağlı bütünlük denetimi):");
                        ui.text_edit_singleline(&mut duz.sha256);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Kaydet").clicked() {
                            if duz.ad.trim().is_empty() {
                                self.mesaj = "Ad boş olamaz".into();
                            } else {
                                match kaydet(&duz) {
                                    Ok(()) => {
                                        self.mesaj = format!("Kaydedildi: {}", duz.kimlik);
                                        self.liste = uygulamalari_listele();
                                        kaydedildi = true;
                                    }
                                    Err(e) => self.mesaj = format!("Kaydedilemedi: {e}"),
                                }
                            }
                        }
                        if ui.button("Vazgeç").clicked() {
                            kaydedildi = true; // pencereyi kapatmak icin ayni bayrak kullanilir
                        }
                    });
                });
            if !acik || kaydedildi {
                self.duzenleme = None;
            } else {
                self.duzenleme = Some(duz);
            }
        }
    }
}

/// Seçili Armonia teması (ayarlar.conf TEMA=; yoksa "armonia"). dosyalar/src/main.rs'teki ile aynı mantık
/// (ayarlar crate'ine bağımlı olmadan): egui varsayılanı koyudur, çağırmazsak pencere Armonia temasıyla uyuşmaz.
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
        "Armonia Windows Programları",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            tema(&cc.egui_ctx);
            Ok(Box::new(UygulamaAyarlari::default()))
        }),
    )
}
