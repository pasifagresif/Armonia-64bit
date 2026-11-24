//! Belge modeli. UDF'in (UYAP) yapısına göre kuruldu: editörün iç yapısı = UDF öğeleri. DOCX bu modele çevrilerek
//! açılır (3. aşama).
//!
//! Kayıpsızlık ilkesi: her öğenin özellikleri (`Ozellikler`) dosyadaki ham değerleriyle, sırası korunarak saklanır.
//! Editörün bilmediği bir özellik de yazarken aynen geri gider. Yalnız metin konumları (`startOffset`, `length`)
//! saklanmaz: yazarken metinden yeniden hesaplanır. Claude, 2026-11-23.

/// Bir öğenin özellikleri: (ad, ham değer), dosyadaki sırayla. Konum özellikleri (`startOffset`, `length`) hariç.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ozellikler(pub Vec<(String, String)>);

impl Ozellikler {
    pub fn al(&self, ad: &str) -> Option<&str> {
        self.0.iter().find(|(a, _)| a == ad).map(|(_, d)| d.as_str())
    }

    /// Değeri ayarlar (varsa yerinde değiştirir, yoksa sona ekler).
    pub fn ayarla(&mut self, ad: &str, deger: impl Into<String>) {
        let deger = deger.into();
        match self.0.iter_mut().find(|(a, _)| a == ad) {
            Some(o) => o.1 = deger,
            None => self.0.push((ad.to_string(), deger)),
        }
    }

    pub fn kaldir(&mut self, ad: &str) {
        self.0.retain(|(a, _)| a != ad);
    }

    pub fn evet_mi(&self, ad: &str) -> bool {
        self.al(ad) == Some("true")
    }

    pub fn sayi(&self, ad: &str) -> Option<f64> {
        self.al(ad).and_then(|d| d.trim().parse().ok())
    }
}

/// Paragraf içindeki parça. UDF etiketleri: `content`, `space`, `tab`, `field`, `image`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Parca {
    /// Metin (`content`). Satır sonu karakterleri dahil, dosyadaki gibi.
    Metin { metin: String, oz: Ozellikler },
    /// Boşluk (`space`).
    Bosluk { metin: String, oz: Ozellikler },
    /// Sekme (`tab`).
    Sekme { metin: String, oz: Ozellikler },
    /// Alan (`field`): ör. sayfa numarası, tarih.
    Alan { metin: String, oz: Ozellikler },
    /// Satır içi resim (`image`); resim verisi `imageData` özelliğinde (base64), metin konumu taşıyabilir.
    Resim { metin: String, oz: Ozellikler },
}

impl Parca {
    pub fn metin(&self) -> &str {
        match self {
            Parca::Metin { metin, .. }
            | Parca::Bosluk { metin, .. }
            | Parca::Sekme { metin, .. }
            | Parca::Alan { metin, .. }
            | Parca::Resim { metin, .. } => metin,
        }
    }

    pub fn oz(&self) -> &Ozellikler {
        match self {
            Parca::Metin { oz, .. }
            | Parca::Bosluk { oz, .. }
            | Parca::Sekme { oz, .. }
            | Parca::Alan { oz, .. }
            | Parca::Resim { oz, .. } => oz,
        }
    }

    pub(crate) fn etiket(&self) -> &'static str {
        match self {
            Parca::Metin { .. } => "content",
            Parca::Bosluk { .. } => "space",
            Parca::Sekme { .. } => "tab",
            Parca::Alan { .. } => "field",
            Parca::Resim { .. } => "image",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Paragraf {
    pub oz: Ozellikler,
    pub parcalar: Vec<Parca>,
}

impl Paragraf {
    /// Paragrafın düz metni (satır sonları çıkarılmış).
    pub fn duz_metin(&self) -> String {
        self.parcalar.iter().map(Parca::metin).collect::<String>().replace(['\r', '\n'], "")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Hucre {
    pub oz: Ozellikler,
    pub bloklar: Vec<Blok>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Satir {
    pub oz: Ozellikler,
    pub hucreler: Vec<Hucre>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tablo {
    pub oz: Ozellikler,
    pub satirlar: Vec<Satir>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Blok {
    Paragraf(Paragraf),
    Tablo(Tablo),
}

/// Üst ya da alt bilgi (`header` / `footer`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Kenar {
    pub oz: Ozellikler,
    pub bloklar: Vec<Blok>,
}

/// Adlandırılmış stil (`styles/style`). `resolver` özelliği üst stile işaret eder (zincir).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Stil {
    pub ad: String,
    pub oz: Ozellikler,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Belge {
    /// `template` kök öğesinin özellikleri (ör. `format_id`).
    pub kok: Ozellikler,
    /// `properties/pageFormat`: kâğıt, yön, kenar boşlukları.
    pub sayfa: Ozellikler,
    /// `elements` öğesinin özellikleri (ör. `resolver`).
    pub govde_oz: Ozellikler,
    pub ust_bilgi: Option<Kenar>,
    pub alt_bilgi: Option<Kenar>,
    pub govde: Vec<Blok>,
    pub stiller: Vec<Stil>,
    /// UDF ZIP'indeki content.xml dışındaki dosyalar (ad, bayt), sırayla. İmza (`sign.sgn`) okunurken ayrılır.
    pub ek_dosyalar: Vec<(String, Vec<u8>)>,
    /// `template` altındaki tanınmayan, metin konumu taşımayan öğeler (ör. UYAP web'in `<webID>`'si), ham XML
    /// olarak; yazarken stillerden sonra aynen geri konur.
    #[cfg_attr(feature = "serde", serde(default))]
    pub ek_ogeler: Vec<String>,
}

impl Belge {
    /// Belgenin düz metni: gövde paragrafları satır satır (tablolar hücre hücre).
    pub fn duz_metin(&self) -> String {
        fn yaz(b: &[Blok], s: &mut Vec<String>) {
            for x in b {
                match x {
                    Blok::Paragraf(p) => s.push(p.duz_metin()),
                    Blok::Tablo(t) => {
                        for r in &t.satirlar {
                            for h in &r.hucreler {
                                yaz(&h.bloklar, s);
                            }
                        }
                    }
                }
            }
        }
        let mut s = Vec::new();
        yaz(&self.govde, &mut s);
        s.join("\n")
    }
}
