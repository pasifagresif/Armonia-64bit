//! Görünüm: belgeyle aynı ağaç biçiminde, her paragrafın ve parçanın **çözülmüş** stili. Arayüz (editör, PDF)
//! çizimi buna göre yapar; kural tek yerde (`stil.rs`) kalır. Claude, 2026-11-23.

use crate::model::*;
use crate::stil::*;

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GBlok {
    Paragraf {
        stil: ParagrafStili,
        /// Liste öneki ("1.", "•"); liste değilse yok.
        onek: Option<String>,
        /// `Paragraf::parcalar` ile aynı sırada.
        parcalar: Vec<ParcaStili>,
    },
    Tablo {
        /// satır → hücre → bloklar
        satirlar: Vec<Vec<Vec<GBlok>>>,
        stil: TabloStili,
        /// `satirlar` ile aynı sırada: hücre oranları, en az yükseklik
        satir_bilgisi: Vec<SatirBilgisi>,
    },
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Gorunum {
    pub sayfa: SayfaBicimi,
    pub ust_bilgi: Option<Vec<GBlok>>,
    pub govde: Vec<GBlok>,
    pub alt_bilgi: Option<Vec<GBlok>>,
    pub ust_kenar: Option<KenarBilgisi>,
    pub alt_kenar: Option<KenarBilgisi>,
    /// Alt bilgi özelliklerinden çizilen sayfa numarası (udf_pdf.rs `_sayfa_no_metni`).
    pub sayfa_numarasi: Option<SayfaNumarasi>,
}

/// Üst/alt bilginin hangi sayfalarda çizileceği (udf_pdf.rs `aralikta`, `_gorunur`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KenarBilgisi {
    /// İlk sayfa (`startPage`, varsayılan 1).
    pub bas: i64,
    /// Son sayfa (`stopPage`); 0 ya da eksi: sona kadar.
    pub bit: i64,
    /// İçinde resim ya da boş olmayan metin var mı (yoksa hiç çizilmez, yer kaplamaz).
    pub gorunur: bool,
}

impl KenarBilgisi {
    pub fn sayfada(&self, n: i64) -> bool {
        self.gorunur && n >= self.bas && (self.bit <= 0 || n <= self.bit)
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SayfaNumarasi {
    /// İlk sayfanın numarası (`pageNumber-pageStartNumStr`, varsayılan 1).
    pub bas: i64,
    /// "3/12" biçimi (spec & 2048).
    pub toplam_goster: bool,
    pub ayirici: String,
    /// Önündeki metin (`pageNumber-foreStr`, ör. "Sayfa").
    pub on_ek: String,
    pub hiza: Hiza,
    pub stil: ParcaStili,
}

impl SayfaNumarasi {
    /// `pageNumber-spec` sonundaki sayı 0 ise ya da özellik yoksa sayfa numarası yok.
    pub fn coz(oz: &Ozellikler) -> Option<SayfaNumarasi> {
        let spec = oz.al("pageNumber-spec")?;
        let rakam: String = spec.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        let spec: u64 = rakam.parse().ok()?;
        if spec == 0 {
            return None;
        }
        let mut boyut = oz.sayi("pageNumber-fontSize").unwrap_or(10.0);
        if boyut <= 0.0 {
            boyut = 10.0;
        }
        Some(SayfaNumarasi {
            bas: oz.al("pageNumber-pageStartNumStr").and_then(|s| s.trim().parse().ok()).unwrap_or(1),
            toplam_goster: spec & 2048 != 0,
            ayirici: oz.al("pageNumber-seperator").unwrap_or("/").to_string(),
            on_ek: oz.al("pageNumber-foreStr").unwrap_or("").trim().to_string(),
            hiza: if spec & 64 != 0 {
                Hiza::Sag
            } else if spec & 32 != 0 {
                Hiza::Orta
            } else {
                Hiza::Sol
            },
            stil: ParcaStili {
                aile: oz.al("pageNumber-fontFace").unwrap_or("Arial").to_string(),
                boyut,
                kalin: oz.evet_mi("pageNumber-fontBold"),
                egik: oz.evet_mi("pageNumber-fontItalic"),
                alti_cizili: false,
                renk: oz.al("pageNumber-color").and_then(Renk::argb).unwrap_or(Renk::SIYAH),
            },
        })
    }

    /// n. sayfanın (1'den) numara metni.
    pub fn metin(&self, n: i64, toplam: i64) -> String {
        let mut m = (n + self.bas - 1).to_string();
        if self.toplam_goster {
            m.push_str(&self.ayirici);
            m.push_str(&(toplam + self.bas - 1).to_string());
        }
        if self.on_ek.is_empty() {
            m
        } else {
            format!("{} {m}", self.on_ek)
        }
    }
}

fn gorunur_mu(b: &[Blok]) -> bool {
    b.iter().any(|x| match x {
        Blok::Paragraf(p) => p.parcalar.iter().any(|c| match c {
            Parca::Resim { .. } => true,
            Parca::Metin { metin, .. } | Parca::Bosluk { metin, .. } | Parca::Alan { metin, .. } => !metin.trim().is_empty(),
            Parca::Sekme { .. } => false,
        }),
        Blok::Tablo(t) => t.satirlar.iter().any(|r| r.hucreler.iter().any(|h| gorunur_mu(&h.bloklar))),
    })
}

fn kenar_bilgisi(k: &Kenar) -> KenarBilgisi {
    KenarBilgisi {
        bas: k.oz.sayi("startPage").map(|x| x as i64).unwrap_or(1),
        bit: k.oz.sayi("stopPage").map(|x| x as i64).unwrap_or(0),
        gorunur: gorunur_mu(&k.bloklar),
    }
}

struct Hesap<'b> {
    cozucu: StilCozucu<'b>,
    sayac: ListeSayaci,
}

impl Hesap<'_> {
    fn bloklar(&mut self, b: &[Blok], renk: Renk) -> Vec<GBlok> {
        b.iter().map(|x| self.blok(x, renk)).collect()
    }

    fn blok(&mut self, b: &Blok, renk: Renk) -> GBlok {
        match b {
            Blok::Paragraf(p) => {
                let stil = self.cozucu.paragraf(p);
                let onek = self.sayac.onek(&stil);
                let parcalar = p.parcalar.iter().map(|c| self.cozucu.parca(c, p, renk)).collect();
                GBlok::Paragraf { stil, onek, parcalar }
            }
            Blok::Tablo(t) => GBlok::Tablo {
                satirlar: t
                    .satirlar
                    .iter()
                    .map(|r| r.hucreler.iter().map(|h| self.bloklar(&h.bloklar, renk)).collect())
                    .collect(),
                stil: tablo_stili(t),
                satir_bilgisi: t.satirlar.iter().map(|r| satir_bilgisi(t, r)).collect(),
            },
        }
    }
}

/// Belgenin görünümü. Liste numaraları gövdede baştan sona; üst ve alt bilgide ayrı sayılır.
pub fn gorunum(belge: &Belge) -> Gorunum {
    let kenar = |k: &Option<Kenar>| {
        k.as_ref().map(|k| {
            let renk = k.oz.al("foreground").and_then(Renk::argb).unwrap_or(Renk::SIYAH);
            Hesap { cozucu: StilCozucu::yeni(belge), sayac: ListeSayaci::default() }.bloklar(&k.bloklar, renk)
        })
    };
    let mut govde = Hesap { cozucu: StilCozucu::yeni(belge), sayac: ListeSayaci::default() };
    Gorunum {
        sayfa: SayfaBicimi::coz(&belge.sayfa),
        ust_bilgi: kenar(&belge.ust_bilgi),
        govde: govde.bloklar(&belge.govde, Renk::SIYAH),
        alt_bilgi: kenar(&belge.alt_bilgi),
        ust_kenar: belge.ust_bilgi.as_ref().map(kenar_bilgisi),
        alt_kenar: belge.alt_bilgi.as_ref().map(kenar_bilgisi),
        sayfa_numarasi: belge.alt_bilgi.as_ref().and_then(|k| SayfaNumarasi::coz(&k.oz)),
    }
}
