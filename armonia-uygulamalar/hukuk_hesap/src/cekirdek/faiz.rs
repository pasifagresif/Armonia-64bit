//! Faiz hesabı (H2). Claude, 2026-11-19. Kurallar ve oranlar kullanıcıyla DOĞRULANACAK.
//!
//! - 3095 m.1 (7589 s.K. ile değişik, yürürlük 31.07.2026): yasal faiz = TCMB'nin önceki yıl 31 Aralık günü
//!   uyguladığı reeskont oranının %80'i; 30 Haziran oranı 31 Aralık oranından 5 puan veya daha çok farklıysa
//!   yılın ikinci yarısında 30 Haziran oranının %80'i. 31.07.2026 öncesi: sabit oranlar (`veri/faiz.json`).
//! - 3095 m.2/2: ticari işlerde temerrüt faizi, TCMB'nin önceki yıl 31 Aralık kısa vadeli avans oranı (yasal
//!   faizden yüksekse); 30 Haziran avans oranı 5 puan veya daha çok farklıysa ikinci yarıda o oran.
//!   Uygulamada TCMB değişiklik tarihlerinden itibaren uygulayan tablolar da var (ör. UAB tablosu) → seçenek.
//! - 3095 m.3: mürekkep faiz yok → basit faiz. Gün esası 365 (artık yılda da).
//! - Gün sayısı: başlangıç günü sayılır, bitiş günü sayılmaz (bitiş − başlangıç).
//! - Her kalem, her oran dönemi ayrı hesaplanır ve kuruşa yuvarlanır (yarım kuruş yukarı).

use crate::cekirdek::para::{self, ORAN_BIRIM};
use crate::cekirdek::tarih;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

const GOMULU: &str = include_str!("../../veri/faiz.json");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TcmbOrani {
    pub tarih: NaiveDate,
    pub reeskont: f64,
    pub avans: f64,
    /// Kullanıcının eklediği satır (gömülü veride yok)
    #[serde(default)]
    pub kullanici: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct YasalDonem {
    pub baslangic: NaiveDate,
    pub oran: f64,
    pub dayanak: String,
    pub kaynak: String,
    pub dogrulandi: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Veri {
    pub veri_tarihi: NaiveDate,
    pub yasal: Vec<YasalDonem>,
    pub yasal_kural_baslangic: NaiveDate,
    pub yasal_kural_aciklama: String,
    pub tcmb_kaynak: String,
    pub tcmb: Vec<TcmbOrani>,
}

impl Veri {
    pub fn gomulu() -> Veri {
        let mut v: Veri = serde_json::from_str(GOMULU).expect("veri/faiz.json bozuk");
        v.tcmb.sort_by_key(|t| t.tarih);
        v
    }

    /// Kullanıcı satırlarını ekler (aynı tarihli gömülü satırın yerine geçer). Veri tarihi en son kullanıcı
    /// satırına kadar ilerler: kullanıcı o güne kadar başka değişiklik olmadığını bildirmiş sayılır.
    pub fn kullanici_ekle(&mut self, satirlar: impl IntoIterator<Item = TcmbOrani>) {
        for mut s in satirlar {
            s.kullanici = true;
            self.tcmb.retain(|t| t.tarih != s.tarih);
            self.veri_tarihi = self.veri_tarihi.max(s.tarih);
            self.tcmb.push(s);
        }
        self.tcmb.sort_by_key(|t| t.tarih);
    }

    /// O gün geçerli TCMB satırı. Gün veri tarihinden sonraysa bilinmiyor.
    fn tcmb_gunu(&self, t: NaiveDate) -> Option<&TcmbOrani> {
        if t > self.veri_tarihi {
            return None;
        }
        self.tcmb.iter().rev().find(|x| x.tarih <= t)
    }
}

/// Bir oran döneminin başlangıcı; sonraki dilimin başına (ya da çizelgenin `son`una) kadar geçerli.
#[derive(Clone, Debug, PartialEq)]
pub struct Dilim {
    pub baslangic: NaiveDate,
    pub oran: u64,
    pub aciklama: String,
    pub dogrulandi: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cizelge {
    pub dilimler: Vec<Dilim>,
    /// Bu günden itibaren oran bilinmiyor (None: açık uçlu)
    pub son: Option<NaiveDate>,
}

impl Cizelge {
    /// [a, b) aralığını oran dilimlerine böler. Bilinmeyen bölüm varsa hata.
    fn bol(&self, a: NaiveDate, b: NaiveDate) -> Result<Vec<(NaiveDate, NaiveDate, &Dilim)>, String> {
        let ilk = self.dilimler.first().ok_or("Oran verisi yok")?;
        if a < ilk.baslangic {
            return Err(format!("{} öncesi için oran verisi yok", tarih::kisa(ilk.baslangic)));
        }
        if let Some(s) = self.son {
            if b > s {
                return Err(format!(
                    "{} tarihinden itibaren oran bilinmiyor (TCMB'nin o tarihteki oranı programda yok). \
                     Yeni oranı \"Oranlar\" bölümünden ekleyebilirsiniz.",
                    tarih::kisa(s)
                ));
            }
        }
        let mut v = Vec::new();
        for (i, d) in self.dilimler.iter().enumerate() {
            let bitis = self.dilimler.get(i + 1).map(|x| x.baslangic).or(self.son);
            let s = a.max(d.baslangic);
            let e = bitis.map_or(b, |x| b.min(x));
            if s < e {
                v.push((s, e, d));
            }
        }
        Ok(v)
    }

    /// Ardışık aynı oranlı dilimleri birleştirir.
    fn sadelestir(mut self) -> Self {
        let mut v: Vec<Dilim> = Vec::new();
        for d in self.dilimler.drain(..) {
            if v.last().is_some_and(|s| s.oran == d.oran) {
                continue;
            }
            v.push(d);
        }
        self.dilimler = v;
        self
    }
}

fn yuzde(x: f64) -> String {
    para::oran_yaz(para::oran_birim(x))
}

/// 3095 m.1 / m.2 ortak kuralı: önceki yıl 31 Aralık oranı; 30 Haziran oranı ≥ 5 puan farklıysa ikinci yarıda o.
/// `oran` TCMB satırından ilgili oranı seçer, `carpan` (pay, payda) sonucu ölçekler (yasal faizde 8/10).
fn yari_yil_kurali(
    veri: &Veri,
    bas: NaiveDate,
    oran: impl Fn(&TcmbOrani) -> f64,
    carpan: (u64, u64),
    ad: &str,
    madde: &str,
) -> Cizelge {
    let mut dilimler = Vec::new();
    let mut yil = bas.year();
    loop {
        let aralik31 = NaiveDate::from_ymd_opt(yil - 1, 12, 31).unwrap();
        let haz30 = NaiveDate::from_ymd_opt(yil, 6, 30).unwrap();
        for yari in 0..2 {
            let yari_bas = NaiveDate::from_ymd_opt(yil, if yari == 0 { 1 } else { 7 }, 1).unwrap();
            let yari_son = if yari == 0 { NaiveDate::from_ymd_opt(yil, 7, 1).unwrap() } else { NaiveDate::from_ymd_opt(yil + 1, 1, 1).unwrap() };
            if yari_son <= bas {
                continue;
            }
            let dilim_bas = yari_bas.max(bas);
            let Some(r31) = veri.tcmb_gunu(aralik31) else {
                return Cizelge { dilimler, son: Some(dilim_bas) };
            };
            let o31 = para::oran_birim(oran(r31));
            let (secilen, gerekce) = if yari == 0 {
                (o31, format!("{} reeskont/avans oranı", tarih::kisa(aralik31)))
            } else {
                let Some(r30) = veri.tcmb_gunu(haz30) else {
                    return Cizelge { dilimler, son: Some(dilim_bas) };
                };
                let o30 = para::oran_birim(oran(r30));
                if o30.abs_diff(o31) >= 5 * ORAN_BIRIM {
                    (o30, format!("30.06.{yil} oranı, 31.12 oranından 5 puan veya daha çok farklı"))
                } else {
                    (o31, format!("{} oranı (30.06 farkı 5 puandan az)", tarih::kisa(aralik31)))
                }
            };
            let sonuc = secilen * carpan.0 / carpan.1;
            let aciklama = if carpan.0 == carpan.1 {
                format!("{ad} %{} ({madde}; {gerekce})", para::oran_yaz(sonuc))
            } else {
                format!("{ad} %{} ({madde}; %{} × %80, {gerekce})", para::oran_yaz(sonuc), para::oran_yaz(secilen))
            };
            dilimler.push(Dilim { baslangic: dilim_bas, oran: sonuc, aciklama, dogrulandi: false });
        }
        yil += 1;
    }
}

/// Yasal (kanuni) faiz çizelgesi.
pub fn yasal_cizelge(veri: &Veri) -> Cizelge {
    let mut dilimler: Vec<Dilim> = veri
        .yasal
        .iter()
        .filter(|y| y.baslangic < veri.yasal_kural_baslangic)
        .map(|y| Dilim {
            baslangic: y.baslangic,
            oran: para::oran_birim(y.oran),
            aciklama: format!("Yasal faiz %{} ({}; {})", yuzde(y.oran), y.dayanak, y.kaynak),
            dogrulandi: y.dogrulandi,
        })
        .collect();
    let kural = yari_yil_kurali(veri, veri.yasal_kural_baslangic, |t| t.reeskont, (8, 10), "Yasal faiz", "3095 m.1, 7589 s.K.");
    dilimler.extend(kural.dilimler);
    Cizelge { dilimler, son: kural.son }.sadelestir()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvansKipi {
    /// 3095 m.2/2 metni: 31 Aralık oranı, 30 Haziran'da 5 puan kuralı
    Kanun,
    /// TCMB'nin her oran değişikliği yürürlük gününden itibaren
    TcmbTarihleri,
}

impl AvansKipi {
    pub fn ad(self) -> &'static str {
        match self {
            AvansKipi::Kanun => "3095 m.2/2 (31 Aralık / 30 Haziran)",
            AvansKipi::TcmbTarihleri => "TCMB değişiklik tarihleri",
        }
    }
}

/// TCMB avans oranı çizelgesi (yasal faizle karşılaştırılmadan).
pub fn avans_cizelge(veri: &Veri, kip: AvansKipi) -> Cizelge {
    match kip {
        AvansKipi::Kanun => {
            // İlk tam yıl: önceki 31 Aralık'ı veride olan yıl
            let ilk = veri.tcmb.first().map(|t| t.tarih.year() + 1).unwrap_or(2006);
            let bas = NaiveDate::from_ymd_opt(ilk, 1, 1).unwrap();
            yari_yil_kurali(veri, bas, |t| t.avans, (1, 1), "Avans faizi", "3095 m.2/2").sadelestir()
        }
        AvansKipi::TcmbTarihleri => Cizelge {
            dilimler: veri
                .tcmb
                .iter()
                .map(|t| Dilim {
                    baslangic: t.tarih,
                    oran: para::oran_birim(t.avans),
                    aciklama: format!(
                        "Avans faizi %{} (TCMB, {} tarihinden itibaren{})",
                        yuzde(t.avans),
                        tarih::kisa(t.tarih),
                        if t.kullanici { "; kullanıcı ekledi" } else { "" }
                    ),
                    dogrulandi: false,
                })
                .collect(),
            son: None,
        },
    }
}

/// Ticari temerrüt faizi: avans oranı, yasal faizden düşükse yasal faiz (3095 m.2/2 "fazla ise").
pub fn ticari_cizelge(veri: &Veri, kip: AvansKipi) -> Cizelge {
    let a = avans_cizelge(veri, kip);
    let y = yasal_cizelge(veri);
    let mut noktalar: Vec<NaiveDate> = a.dilimler.iter().chain(y.dilimler.iter()).map(|d| d.baslangic).collect();
    noktalar.sort();
    noktalar.dedup();
    let bul = |c: &Cizelge, t: NaiveDate| -> Option<Dilim> {
        if c.son.is_some_and(|s| t >= s) {
            return None;
        }
        c.dilimler.iter().rev().find(|d| d.baslangic <= t).cloned()
    };
    let bas = a.dilimler.first().map(|d| d.baslangic).unwrap_or(NaiveDate::MIN);
    let son = match (a.son, y.son) {
        (Some(p), Some(q)) => Some(p.min(q)),
        (p, q) => p.or(q),
    };
    let mut dilimler = Vec::new();
    for t in noktalar.into_iter().filter(|t| *t >= bas && son.map_or(true, |s| *t < s)) {
        let (Some(da), Some(dy)) = (bul(&a, t), bul(&y, t)) else { continue };
        dilimler.push(if da.oran >= dy.oran {
            Dilim { baslangic: t, ..da }
        } else {
            Dilim {
                baslangic: t,
                oran: dy.oran,
                aciklama: format!("{} — avans oranı (%{}) daha düşük olduğundan", dy.aciklama, para::oran_yaz(da.oran)),
                dogrulandi: dy.dogrulandi,
            }
        });
    }
    let mut c = Cizelge { dilimler, son };
    // Aynı oranlı ardışık dilimler (açıklaması farklı olsa da) birleşir; ilk açıklama kalır.
    c = c.sadelestir();
    c
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tur {
    Yasal,
    Ticari(AvansKipi),
    /// Sözleşmede kararlaştırılan yıllık oran (oran birimi)
    Sozlesmesel(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kalem {
    pub ad: String,
    /// Kuruş
    pub anapara: i64,
    /// Faizin işlemeye başladığı gün (temerrüt / dava / takip tarihi)
    pub baslangic: NaiveDate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Girdi {
    pub kalemler: Vec<Kalem>,
    /// Faizin hesaplandığı son gün (bu gün sayılmaz)
    pub bitis: NaiveDate,
    pub tur: Tur,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DonemSatiri {
    pub baslangic: NaiveDate,
    pub bitis: NaiveDate,
    pub gun: i64,
    pub oran: u64,
    pub aciklama: String,
    pub faiz: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KalemSonucu {
    pub kalem: Kalem,
    pub donemler: Vec<DonemSatiri>,
    pub faiz: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sonuc {
    pub kalemler: Vec<KalemSonucu>,
    pub toplam_anapara: i64,
    pub toplam_faiz: i64,
    pub uyarilar: Vec<String>,
}

/// anapara × oran × gün / 365, kuruşa yuvarlanır (yarım yukarı).
pub fn donem_faizi(anapara: i64, oran: u64, gun: i64) -> i64 {
    let pay = anapara as i128 * oran as i128 * gun as i128;
    let payda = 365i128 * 100 * ORAN_BIRIM as i128;
    ((pay * 2 + payda) / (payda * 2)) as i64
}

pub fn hesapla(g: &Girdi, veri: &Veri) -> Result<Sonuc, String> {
    if g.kalemler.is_empty() {
        return Err("En az bir alacak kalemi girin.".into());
    }
    let cizelge = match g.tur {
        Tur::Yasal => yasal_cizelge(veri),
        Tur::Ticari(k) => ticari_cizelge(veri, k),
        Tur::Sozlesmesel(o) => Cizelge {
            dilimler: vec![Dilim {
                baslangic: NaiveDate::MIN,
                oran: o,
                aciklama: format!("Sözleşmesel faiz %{}", para::oran_yaz(o)),
                dogrulandi: true,
            }],
            son: None,
        },
    };
    let mut uyarilar = Vec::new();
    let mut kalemler = Vec::new();
    for (i, k) in g.kalemler.iter().enumerate() {
        let ad = if k.ad.trim().is_empty() { format!("{}. kalem", i + 1) } else { k.ad.clone() };
        if k.anapara <= 0 {
            return Err(format!("{ad}: tutar sıfırdan büyük olmalı."));
        }
        if k.baslangic >= g.bitis {
            return Err(format!("{ad}: başlangıç tarihi bitiş tarihinden önce olmalı."));
        }
        let parcalar = cizelge.bol(k.baslangic, g.bitis).map_err(|e| format!("{ad}: {e}"))?;
        let mut donemler = Vec::new();
        for (s, e, d) in parcalar {
            let gun = (e - s).num_days();
            if !d.dogrulandi && !uyarilar.iter().any(|u: &String| u.contains("doğrulanmadı")) {
                uyarilar.push("Programdaki oranlar henüz kullanıcı tarafından doğrulanmadı; resmî kaynakla karşılaştırın.".into());
            }
            donemler.push(DonemSatiri { baslangic: s, bitis: e, gun, oran: d.oran, aciklama: d.aciklama.clone(), faiz: donem_faizi(k.anapara, d.oran, gun) });
        }
        let faiz = donemler.iter().map(|d| d.faiz).sum();
        kalemler.push(KalemSonucu { kalem: Kalem { ad, ..k.clone() }, donemler, faiz });
    }
    if matches!(g.tur, Tur::Ticari(AvansKipi::TcmbTarihleri)) && g.bitis > veri.veri_tarihi {
        uyarilar.push(format!(
            "{} sonrası için TCMB oranının değişmediği varsayıldı.",
            tarih::kisa(veri.veri_tarihi)
        ));
    }
    if matches!(g.tur, Tur::Ticari(_)) {
        uyarilar.push("Ticari temerrüt faizi yalnız ticari işlerde istenebilir (3095 m.2/2).".into());
    }
    Ok(Sonuc {
        toplam_anapara: kalemler.iter().map(|k| k.kalem.anapara).sum(),
        toplam_faiz: kalemler.iter().map(|k| k.faiz).sum(),
        kalemler,
        uyarilar,
    })
}

/// Dilekçeye yapıştırılacak düz metin döküm.
pub fn metin(g: &Girdi, s: &Sonuc) -> String {
    let mut m = String::new();
    let tur = match g.tur {
        Tur::Yasal => "Yasal faiz (3095 m.1)".to_string(),
        Tur::Ticari(k) => format!("Ticari temerrüt (avans) faizi (3095 m.2/2; {})", k.ad()),
        Tur::Sozlesmesel(o) => format!("Sözleşmesel faiz, yıllık %{}", para::oran_yaz(o)),
    };
    m.push_str(&format!("FAİZ HESABI — {tur}\nBitiş: {} · Basit faiz, 365 gün esası\n\n", tarih::kisa(g.bitis)));
    for k in &s.kalemler {
        m.push_str(&format!("{} — {} ({} tarihinden)\n", k.kalem.ad, para::yaz(k.kalem.anapara), tarih::kisa(k.kalem.baslangic)));
        for d in &k.donemler {
            m.push_str(&format!(
                "  {} – {}  {:>5} gün  %{:<6} {:>18}   {}\n",
                tarih::kisa(d.baslangic),
                tarih::kisa(d.bitis),
                d.gun,
                para::oran_yaz(d.oran),
                para::yaz(d.faiz),
                d.aciklama
            ));
        }
        m.push_str(&format!("  Kalem faizi: {}\n\n", para::yaz(k.faiz)));
    }
    m.push_str(&format!(
        "Toplam anapara: {}\nToplam faiz:    {}\nGenel toplam:   {}\n",
        para::yaz(s.toplam_anapara),
        para::yaz(s.toplam_faiz),
        para::yaz(s.toplam_anapara + s.toplam_faiz)
    ));
    for u in &s.uyarilar {
        m.push_str(&format!("Not: {u}\n"));
    }
    m
}

#[cfg(test)]
mod testler {
    use super::*;

    fn t(y: i32, a: u32, g: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, a, g).unwrap()
    }

    fn oran_gunu(c: &Cizelge, g: NaiveDate) -> u64 {
        c.dilimler.iter().rev().find(|d| d.baslangic <= g).unwrap().oran
    }

    #[test]
    fn gomulu_veri_okunur() {
        let v = Veri::gomulu();
        assert!(v.tcmb.len() > 30);
        assert_eq!(v.yasal_kural_baslangic, t(2026, 7, 31));
    }

    #[test]
    fn yasal_oranlar() {
        let c = yasal_cizelge(&Veri::gomulu());
        assert_eq!(oran_gunu(&c, t(2010, 3, 1)), 90_000);
        assert_eq!(oran_gunu(&c, t(2024, 5, 31)), 90_000);
        assert_eq!(oran_gunu(&c, t(2024, 6, 1)), 240_000);
        assert_eq!(oran_gunu(&c, t(2026, 7, 30)), 240_000);
        // 31.12.2025 reeskont %38,75 × %80 = %31 (UAB tablosuyla aynı)
        assert_eq!(oran_gunu(&c, t(2026, 7, 31)), 310_000);
        assert_eq!(oran_gunu(&c, t(2026, 12, 31)), 310_000);
        // 2027 için 31.12.2026 oranı henüz bilinmiyor
        assert_eq!(c.son, Some(t(2027, 1, 1)));
    }

    #[test]
    fn avans_kanun_kurali() {
        let c = avans_cizelge(&Veri::gomulu(), AvansKipi::Kanun);
        // 2025: 31.12.2024 → %49,25; 30.06.2025 %44,25 (fark tam 5 puan) → ikinci yarı %44,25
        assert_eq!(oran_gunu(&c, t(2025, 3, 10)), 492_500);
        assert_eq!(oran_gunu(&c, t(2025, 7, 1)), 442_500);
        // 2024: 31.12.2023 %44,25; 30.06.2024 %51,75 (7,5 puan) → ikinci yarı %51,75
        assert_eq!(oran_gunu(&c, t(2024, 6, 30)), 442_500);
        assert_eq!(oran_gunu(&c, t(2024, 7, 1)), 517_500);
        // 2026: 31.12.2025 %39,75; değişiklik yok
        assert_eq!(oran_gunu(&c, t(2026, 9, 1)), 397_500);
        // 2023: 31.12.2022 %10,75; 30.06.2023 %16,75 (6 puan) → ikinci yarı %16,75 (Eylül artışları yansımaz)
        assert_eq!(oran_gunu(&c, t(2023, 10, 1)), 167_500);
    }

    #[test]
    fn avans_tcmb_tarihleri() {
        let c = avans_cizelge(&Veri::gomulu(), AvansKipi::TcmbTarihleri);
        assert_eq!(oran_gunu(&c, t(2025, 3, 7)), 492_500);
        assert_eq!(oran_gunu(&c, t(2025, 3, 8)), 442_500);
        assert_eq!(oran_gunu(&c, t(2025, 9, 17)), 422_500);
    }

    #[test]
    fn ticari_yasaldan_az_olamaz() {
        // 2013 ikinci yarı: 30.06.2013 avans %11, 31.12.2012 %13,75 (2,75 puan) → %13,75 sürer; yasal %9
        let c = ticari_cizelge(&Veri::gomulu(), AvansKipi::Kanun);
        assert_eq!(oran_gunu(&c, t(2013, 9, 1)), 137_500);
        // Yapay veri: avans yasal faizin altına inerse yasal uygulanır
        let mut v = Veri::gomulu();
        v.kullanici_ekle([TcmbOrani { tarih: t(2026, 12, 31), reeskont: 10.0, avans: 11.0, kullanici: true }]);
        let c = ticari_cizelge(&v, AvansKipi::Kanun);
        // 2027: avans %11, yasal 10×0,8 = %8 → %11
        assert_eq!(oran_gunu(&c, t(2027, 2, 1)), 110_000);
        let c = ticari_cizelge(&v, AvansKipi::TcmbTarihleri);
        // TCMB kipinde 31.12.2026'dan itibaren avans %11 ama yasal hâlâ %31 → %31
        assert_eq!(oran_gunu(&c, t(2026, 12, 31)), 310_000);
    }

    #[test]
    fn hesap_donemlere_bolunur() {
        let v = Veri::gomulu();
        let g = Girdi {
            kalemler: vec![Kalem { ad: "Kira".into(), anapara: 10_000_000, baslangic: t(2024, 1, 1) }],
            bitis: t(2025, 1, 1),
            tur: Tur::Yasal,
        };
        let s = hesapla(&g, &v).unwrap();
        let d = &s.kalemler[0].donemler;
        assert_eq!(d.len(), 2);
        // 01.01–01.06.2024: 152 gün × %9; 01.06.2024–01.01.2025: 214 gün × %24
        assert_eq!((d[0].gun, d[1].gun), (152, 214));
        // 100.000 × 0,09 × 152/365 = 3.747,945… → 3.747,95
        assert_eq!(d[0].faiz, 374_795);
        // 100.000 × 0,24 × 214/365 = 14.071,232… → 14.071,23
        assert_eq!(d[1].faiz, 1_407_123);
        assert_eq!(s.toplam_faiz, 1_781_918);
    }

    #[test]
    fn hesap_sinirlar() {
        let v = Veri::gomulu();
        let k = |b| Kalem { ad: String::new(), anapara: 100, baslangic: b };
        let hata = |g: Girdi| hesapla(&g, &v).unwrap_err();
        assert!(hata(Girdi { kalemler: vec![k(t(2026, 1, 1))], bitis: t(2026, 1, 1), tur: Tur::Yasal }).contains("önce"));
        assert!(hata(Girdi { kalemler: vec![k(t(2026, 1, 1))], bitis: t(2027, 3, 1), tur: Tur::Yasal }).contains("bilinmiyor"));
        assert!(hata(Girdi { kalemler: vec![k(t(2000, 1, 1))], bitis: t(2026, 3, 1), tur: Tur::Yasal }).contains("öncesi"));
        // Sözleşmesel: sınır yok; %36 × 365 gün
        let s = hesapla(&Girdi { kalemler: vec![Kalem { ad: "x".into(), anapara: 100_000, baslangic: t(2025, 1, 1) }], bitis: t(2026, 1, 1), tur: Tur::Sozlesmesel(360_000) }, &v).unwrap();
        assert_eq!(s.toplam_faiz, 36_000);
        assert_eq!(donem_faizi(1, 240_000, 365), 0); // 0,24 kuruş → 0
        assert_eq!(donem_faizi(3, 500_000, 365), 2); // 1,5 kuruş → 2 (yarım yukarı)
    }
}
