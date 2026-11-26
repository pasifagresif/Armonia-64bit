//! UDF -> PDF dönüştürücü (`uyap_core/udf_pdf.py` 1:1 Rust karşılığı)
//!
//! `content.xml` içindeki `<template format_id="1.8">` yapısını okuyup
//! Windows TTF yazı tipleriyle (veya yerleşik yedek metriklerle) A4/yatay PDF üretir.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use once_cell::sync::Lazy;
use regex::Regex;
use roxmltree::{Document, Node};
use zip::ZipArchive;

const _A4_W: f64 = 595.2756;
const _A4_H: f64 = 841.8898;
const _VARSAYILAN_MARJ: f64 = 42.525;
const _HUCRE_PAD_X: f64 = 2.0;
const _HUCRE_PAD_Y: f64 = 1.0;

fn win_fonts_dir() -> String {
    let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_string());
    Path::new(&win).join("Fonts").to_string_lossy().into_owned()
}

fn aile_ttf(aile: &str) -> Option<[&'static str; 4]> {
    match aile {
        "times new roman" => Some(["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"]),
        "arial" => Some(["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"]),
        "arial narrow" => Some(["arialn.ttf", "arialnb.ttf", "arialni.ttf", "arialnbi.ttf"]),
        "arial black" => Some(["ariblk.ttf", "ariblk.ttf", "ariblk.ttf", "ariblk.ttf"]),
        "calibri" => Some(["calibri.ttf", "calibrib.ttf", "calibrii.ttf", "calibriz.ttf"]),
        "cambria" => Some(["cambria.ttf", "cambriab.ttf", "cambriai.ttf", "cambriaz.ttf"]),
        "candara" => Some(["candara.ttf", "candarab.ttf", "candarai.ttf", "candaraz.ttf"]),
        "tahoma" => Some(["tahoma.ttf", "tahomabd.ttf", "tahoma.ttf", "tahomabd.ttf"]),
        "verdana" => Some(["verdana.ttf", "verdanab.ttf", "verdanai.ttf", "verdanaz.ttf"]),
        "trebuchet ms" => Some(["trebuc.ttf", "trebucbd.ttf", "trebucit.ttf", "trebucbi.ttf"]),
        "segoe ui" => Some(["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"]),
        "courier new" => Some(["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"]),
        "georgia" => Some(["georgia.ttf", "georgiab.ttf", "georgiai.ttf", "georgiaz.ttf"]),
        "bookman old style" => Some(["bookos.ttf", "bookosb.ttf", "bookosi.ttf", "bookosbi.ttf"]),
        "century gothic" => Some(["gothic.ttf", "gothicb.ttf", "gothici.ttf", "gothicbi.ttf"]),
        "century" => Some(["century.ttf", "century.ttf", "century.ttf", "century.ttf"]),
        "garamond" => Some(["gara.ttf", "garabd.ttf", "garait.ttf", "garabd.ttf"]),
        "monotype corsiva" => Some(["mtcorsva.ttf"; 4]),
        "segoe script" => Some(["segoesc.ttf", "segoescb.ttf", "segoesc.ttf", "segoescb.ttf"]),
        "brush script mt" => Some(["brushsci.ttf"; 4]),
        "vladimir script" => Some(["vladimir.ttf"; 4]),
        "rage italic" => Some(["rage.ttf"; 4]),
        "harlow solid italic" => Some(["harlowsi.ttf"; 4]),
        _ => None,
    }
}

fn aile_yedek(aile: &str) -> &'static [&'static str] {
    match aile {
        "dialog" | "sansserif" | ".applesystemuifont" => &["arial"],
        "dialoginput" | "monospaced" => &["courier new"],
        "serif" | "editor" | "source serif pro" | "albertus medium" => &["times new roman"],
        "coronetps" => &["monotype corsiva", "segoe script"],
        _ => &[],
    }
}

// ---------------------------------------------------------------- yardımcılar

fn _f(deger: Option<&str>, varsayilan: f64) -> f64 {
    deger
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(varsayilan)
}

fn _argb_rgb(argb: Option<&str>) -> Option<(f64, f64, f64)> {
    let mut v = argb?.trim().parse::<i64>().ok()?;
    if v < 0 {
        v += 1i64 << 32;
    }
    Some((
        ((v >> 16) & 255) as f64 / 255.0,
        ((v >> 8) & 255) as f64 / 255.0,
        (v & 255) as f64 / 255.0,
    ))
}

fn _oranlar(metin: Option<&str>) -> Vec<f64> {
    let Some(m) = metin else {
        return Vec::new();
    };
    let mut l = Vec::new();
    for x in m.split(',') {
        let t = x.trim();
        if t.is_empty() {
            continue;
        }
        match t.parse::<f64>() {
            Ok(v) => l.push(v),
            Err(_) => return Vec::new(),
        }
    }
    if !l.is_empty() && l.iter().sum::<f64>() > 0.0 {
        l
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------- TTF ayrıştırıcı

#[derive(Clone)]
struct ParsedTtf {
    key: String,
    raw: Option<Arc<Vec<u8>>>,
    base14_name: &'static str,
    ascender: f64,
    descender: f64,
    units_per_em: u16,
    bbox: [i16; 4],
    cmap: HashMap<u32, u16>,
    advances: Vec<u16>,
}

impl ParsedTtf {
    fn gid(&self, ch: char) -> u16 {
        *self.cmap.get(&(ch as u32)).unwrap_or(&0)
    }

    fn glyph_advance(&self, ch: char) -> f64 {
        if self.raw.is_none() || self.units_per_em == 0 {
            return if ch == ' ' { 0.25 } else { 0.5 };
        }
        let gid = self.gid(ch) as usize;
        let adv = if gid < self.advances.len() {
            self.advances[gid]
        } else {
            *self.advances.last().unwrap_or(&(self.units_per_em / 2))
        };
        (adv as f64) / (self.units_per_em as f64)
    }
}

fn be_u16(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

fn be_i16(b: &[u8], off: usize) -> Option<i16> {
    b.get(off..off + 2).map(|s| i16::from_be_bytes([s[0], s[1]]))
}

fn be_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn parse_ttf_bytes(key: String, data: Vec<u8>) -> Option<ParsedTtf> {
    if data.len() < 12 {
        return None;
    }
    let num_tables = be_u16(&data, 4)? as usize;
    let mut tables: HashMap<[u8; 4], (usize, usize)> = HashMap::new();
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        if rec + 16 > data.len() {
            break;
        }
        let tag: [u8; 4] = [data[rec], data[rec + 1], data[rec + 2], data[rec + 3]];
        let off = be_u32(&data, rec + 8)? as usize;
        let len = be_u32(&data, rec + 12)? as usize;
        if off + len <= data.len() {
            tables.insert(tag, (off, len));
        }
    }

    let (head_off, _) = *tables.get(b"head")?;
    let units_per_em = be_u16(&data, head_off + 18)?;
    if units_per_em == 0 {
        return None;
    }
    let bbox = [
        be_i16(&data, head_off + 36).unwrap_or(0),
        be_i16(&data, head_off + 38).unwrap_or(-200),
        be_i16(&data, head_off + 40).unwrap_or(1000),
        be_i16(&data, head_off + 42).unwrap_or(900),
    ];

    let (hhea_off, _) = *tables.get(b"hhea")?;
    let asc_raw = be_i16(&data, hhea_off + 4).unwrap_or(800);
    let desc_raw = be_i16(&data, hhea_off + 6).unwrap_or(-200);
    let num_hmetrics = be_u16(&data, hhea_off + 34).unwrap_or(1) as usize;

    let ascender = (asc_raw as f64) / (units_per_em as f64);
    let descender = (desc_raw as f64) / (units_per_em as f64);

    let (hmtx_off, hmtx_len) = *tables.get(b"hmtx")?;
    let mut advances = Vec::with_capacity(num_hmetrics);
    for i in 0..num_hmetrics {
        let p = hmtx_off + i * 4;
        if p + 2 <= hmtx_off + hmtx_len {
            advances.push(be_u16(&data, p).unwrap_or(units_per_em / 2));
        }
    }

    let mut cmap_map = HashMap::new();
    if let Some(&(cmap_off, cmap_len)) = tables.get(b"cmap") {
        let num_sub = be_u16(&data, cmap_off + 2).unwrap_or(0) as usize;
        let mut chosen_sub = None;
        for i in 0..num_sub {
            let rec = cmap_off + 4 + i * 8;
            if rec + 8 > cmap_off + cmap_len {
                break;
            }
            let pid = be_u16(&data, rec).unwrap_or(0);
            let eid = be_u16(&data, rec + 2).unwrap_or(0);
            let sub_off = cmap_off + be_u32(&data, rec + 4).unwrap_or(0) as usize;
            if (pid == 3 && eid == 10) || (pid == 3 && eid == 1) || pid == 0 {
                chosen_sub = Some(sub_off);
                if pid == 3 && eid == 1 {
                    break;
                }
            }
        }
        if let Some(sub) = chosen_sub {
            let fmt = be_u16(&data, sub).unwrap_or(0);
            if fmt == 4 {
                let seg_count = (be_u16(&data, sub + 6).unwrap_or(0) / 2) as usize;
                let end_off = sub + 14;
                let start_off = end_off + seg_count * 2 + 2;
                let delta_off = start_off + seg_count * 2;
                let range_off = delta_off + seg_count * 2;
                for s in 0..seg_count {
                    let end_c = be_u16(&data, end_off + s * 2).unwrap_or(0xFFFF);
                    let start_c = be_u16(&data, start_off + s * 2).unwrap_or(0xFFFF);
                    if start_c == 0xFFFF {
                        continue;
                    }
                    let delta = be_i16(&data, delta_off + s * 2).unwrap_or(0) as i32;
                    let ro = be_u16(&data, range_off + s * 2).unwrap_or(0) as usize;
                    for c in start_c..=end_c {
                        let gid = if ro == 0 {
                            ((c as i32 + delta) & 0xFFFF) as u16
                        } else {
                            let idx_addr = range_off + s * 2 + ro + ((c - start_c) as usize) * 2;
                            let g = be_u16(&data, idx_addr).unwrap_or(0);
                            if g == 0 {
                                0
                            } else {
                                ((g as i32 + delta) & 0xFFFF) as u16
                            }
                        };
                        if gid != 0 {
                            cmap_map.insert(c as u32, gid);
                        }
                    }
                }
            } else if fmt == 12 {
                let n_groups = be_u32(&data, sub + 12).unwrap_or(0) as usize;
                for g in 0..n_groups {
                    let rec = sub + 16 + g * 12;
                    if rec + 12 > data.len() {
                        break;
                    }
                    let sc = be_u32(&data, rec).unwrap_or(0);
                    let ec = be_u32(&data, rec + 4).unwrap_or(0);
                    let sg = be_u32(&data, rec + 8).unwrap_or(0);
                    for c in sc..=ec.min(sc + 0x10FFFF) {
                        cmap_map.insert(c, (sg + (c - sc)) as u16);
                    }
                }
            }
        }
    }

    Some(ParsedTtf {
        key,
        raw: Some(Arc::new(data)),
        base14_name: "Times-Roman",
        ascender,
        descender,
        units_per_em,
        bbox,
        cmap: cmap_map,
        advances,
    })
}

static FONT_CACHE: Lazy<Mutex<HashMap<String, Option<Arc<ParsedTtf>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

struct Fontlar {
    coz: HashMap<(String, bool, bool), Arc<ParsedTtf>>,
}

impl Fontlar {
    fn new() -> Self {
        Self {
            coz: HashMap::new(),
        }
    }

    fn yukle(&self, yol: &str) -> Option<Arc<ParsedTtf>> {
        let mut cache = FONT_CACHE.lock().unwrap();
        if let Some(entry) = cache.get(yol) {
            return entry.clone();
        }
        let parsed = std::fs::read(yol)
            .ok()
            .and_then(|b| parse_ttf_bytes(yol.to_string(), b))
            .map(Arc::new);
        cache.insert(yol.to_string(), parsed.clone());
        parsed
    }

    fn al(&mut self, aile: &str, kalin: bool, egik: bool) -> Arc<ParsedTtf> {
        let ad = aile.trim().to_lowercase();
        let anahtar = (ad.clone(), kalin, egik);
        if let Some(f) = self.coz.get(&anahtar) {
            return Arc::clone(f);
        }

        let idx = (if egik { 2 } else { 0 }) + (if kalin { 1 } else { 0 });
        let mut adaylar = vec![ad.clone()];
        for &y in aile_yedek(&ad) {
            adaylar.push(y.to_string());
        }
        adaylar.push("times new roman".to_string());

        let winfonts = win_fonts_dir();
        let mut found: Option<Arc<ParsedTtf>> = None;
        for a in &adaylar {
            let Some(dosyalar) = aile_ttf(a) else {
                continue;
            };
            for &d in &[dosyalar[idx], dosyalar[0]] {
                let yol = Path::new(&winfonts).join(d);
                if yol.is_file() {
                    if let Some(f) = self.yukle(&yol.to_string_lossy()) {
                        found = Some(f);
                        break;
                    }
                }
            }
            if found.is_some() {
                break;
            }
        }

        let font = found.unwrap_or_else(|| {
            let base14 = if ad.contains("cour") || ad.contains("mono") {
                match idx {
                    1 => "Courier-Bold",
                    2 => "Courier-Oblique",
                    3 => "Courier-BoldOblique",
                    _ => "Courier",
                }
            } else if ad.contains("arial")
                || ad.contains("dialog")
                || ad.contains("sans")
                || ad.contains("tahoma")
                || ad.contains("verdana")
                || ad.contains("calibri")
            {
                match idx {
                    1 => "Helvetica-Bold",
                    2 => "Helvetica-Oblique",
                    3 => "Helvetica-BoldOblique",
                    _ => "Helvetica",
                }
            } else {
                match idx {
                    1 => "Times-Bold",
                    2 => "Times-Italic",
                    3 => "Times-BoldItalic",
                    _ => "Times-Roman",
                }
            };
            Arc::new(ParsedTtf {
                key: format!("base14:{}", base14),
                raw: None,
                base14_name: base14,
                ascender: 0.8,
                descender: -0.2,
                units_per_em: 1000,
                bbox: [0, -200, 1000, 800],
                cmap: HashMap::new(),
                advances: Vec::new(),
            })
        });

        self.coz.insert(anahtar, Arc::clone(&font));
        font
    }
}

// ---------------------------------------------------------------- Stiller & Bağlam

struct Stiller {
    s: HashMap<String, HashMap<String, String>>,
}

impl Stiller {
    fn new(root: Node) -> Self {
        let mut s = HashMap::new();
        if let Some(styles_el) = root.children().find(|n| n.tag_name().name() == "styles") {
            for st in styles_el.children().filter(|n| n.tag_name().name() == "style") {
                if let Some(name) = st.attribute("name") {
                    let mut m = HashMap::new();
                    for a in st.attributes() {
                        m.insert(a.name().to_string(), a.value().to_string());
                    }
                    s.insert(name.to_string(), m);
                }
            }
        }
        Self { s }
    }

    fn zincir_owned(&self, ad: Option<&str>) -> Vec<HashMap<String, String>> {
        let mut out = Vec::new();
        let mut gorulen = std::collections::HashSet::new();
        let mut cur = ad.map(|s| s.to_string());
        while let Some(name) = cur {
            if name.is_empty() || gorulen.contains(&name) {
                break;
            }
            let Some(m) = self.s.get(&name) else {
                break;
            };
            gorulen.insert(name);
            cur = m.get("resolver").cloned();
            out.push(m.clone());
        }
        out
    }
}

fn ad_first_name(_: &[HashMap<String, String>]) -> Option<String> {
    None
}

fn node_attrs(n: Node) -> HashMap<String, String> {
    n.attributes()
        .map(|a| (a.name().to_string(), a.value().to_string()))
        .collect()
}

struct Baglam {
    chars: Vec<char>,
    stiller: Stiller,
    fontlar: Fontlar,
    varsayilan_renk: (f64, f64, f64),
    liste_sayac: HashMap<String, usize>,
}

impl Baglam {
    fn new(root: Node, full_text: &str) -> Self {
        Self {
            chars: full_text.chars().collect(),
            stiller: Stiller::new(root),
            fontlar: Fontlar::new(),
            varsayilan_renk: (0.0, 0.0, 0.0),
            liste_sayac: HashMap::new(),
        }
    }

    fn dilim(&self, el: Node) -> String {
        let Ok(s) = el.attribute("startOffset").unwrap_or("0").parse::<usize>() else {
            return String::new();
        };
        let Ok(n) = el.attribute("length").unwrap_or("0").parse::<usize>() else {
            return String::new();
        };
        if s >= self.chars.len() || n == 0 {
            return String::new();
        }
        let end = (s + n).min(self.chars.len());
        self.chars[s..end].iter().collect()
    }

    fn par_zincir(&self, par: Node) -> Vec<HashMap<String, String>> {
        let mut v = vec![node_attrs(par)];
        v.extend(self.stiller.zincir_owned(par.attribute("resolver")));
        v
    }

    fn par_attr(&self, par: Node, ad: &str, varsayilan: Option<&str>) -> Option<String> {
        for d in self.par_zincir(par) {
            if let Some(val) = d.get(ad) {
                return Some(val.clone());
            }
        }
        for c in par.children().filter(|n| n.is_element()) {
            if let Some(val) = c.attribute(ad) {
                return Some(val.to_string());
            }
            break;
        }
        varsayilan.map(|s| s.to_string())
    }

    fn stil(&mut self, run: Node, par: Node) -> Stil {
        let mut zincir = vec![node_attrs(run)];
        zincir.extend(self.stiller.zincir_owned(run.attribute("resolver")));
        zincir.extend(self.par_zincir(par));
        zincir.extend(self.stiller.zincir_owned(Some("hvl-default")));

        let bul = |ad: &str, vars_: &str| -> String {
            for d in &zincir {
                if let Some(v) = d.get(ad) {
                    if !v.is_empty() {
                        return v.clone();
                    }
                }
            }
            vars_.to_string()
        };

        let aile = bul("family", "Times New Roman");
        let mut boyut = _f(Some(&bul("size", "12")), 12.0);
        if boyut <= 0.0 {
            boyut = 12.0;
        }
        let kalin = bul("bold", "false") == "true";
        let egik = bul("italic", "false") == "true";
        let mut fg = run.attribute("foreground").map(|s| s.to_string());
        if fg.is_none() {
            let def_style = self.stiller.s.get("default");
            for d in zincir.iter().skip(1) {
                if let Some(v) = d.get("foreground") {
                    if !v.is_empty() && def_style != Some(d) {
                        fg = Some(v.clone());
                        break;
                    }
                }
            }
        }
        let renk = fg
            .as_deref()
            .and_then(|s| _argb_rgb(Some(s)))
            .unwrap_or(self.varsayilan_renk);

        Stil {
            font: self.fontlar.al(&aile, kalin, egik),
            boyut,
            renk,
            alti_cizili: bul("underline", "false") == "true",
        }
    }
}

#[derive(Clone)]
struct Stil {
    font: Arc<ParsedTtf>,
    boyut: f64,
    renk: (f64, f64, f64),
    alti_cizili: bool,
}

impl Stil {
    fn same_style(&self, other: &Stil) -> bool {
        Arc::ptr_eq(&self.font, &other.font)
            && (self.boyut - other.boyut).abs() < 1e-6
            && (self.renk.0 - other.renk.0).abs() < 1e-6
            && (self.renk.1 - other.renk.1).abs() < 1e-6
            && (self.renk.2 - other.renk.2).abs() < 1e-6
    }

    fn gen(&self, metin: &str) -> f64 {
        let toplam: f64 = metin.chars().map(|ch| self.font.glyph_advance(ch)).sum();
        toplam * self.boyut
    }

    fn ust(&self) -> f64 {
        self.font.ascender * self.boyut
    }

    fn alt(&self) -> f64 {
        -self.font.descender * self.boyut
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParcaTur {
    Soz,
    Bosluk,
    Sekme,
    Resim,
}

#[derive(Clone)]
struct Parca {
    tur: ParcaTur,
    metin: String,
    stil: Stil,
    gen: f64,
    resim: Option<Arc<Vec<u8>>>,
    yuk: f64,
}

// ------------------------------------------------------------------ bloklar

struct DrawCmd {
    kind: DrawKind,
}

enum DrawKind {
    Text {
        x: f64,
        y: f64,
        metin: String,
        stil: Stil,
    },
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        color: (f64, f64, f64),
        width: f64,
    },
    Image {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        data: Arc<Vec<u8>>,
    },
}

struct SayfaTuval {
    w: f64,
    h: f64,
    cmds: Vec<DrawKind>,
}

impl SayfaTuval {
    fn yaz(&mut self, x: f64, y: f64, metin: &str, stil: &Stil) {
        self.cmds.push(DrawKind::Text {
            x,
            y,
            metin: metin.to_string(),
            stil: stil.clone(),
        });
    }

    fn draw_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.cmds.push(DrawKind::Rect { x, y, w, h });
    }

    fn draw_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, color: (f64, f64, f64), width: f64) {
        self.cmds.push(DrawKind::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        });
    }

    fn insert_image(&mut self, x: f64, y: f64, w: f64, h: f64, data: Arc<Vec<u8>>) {
        self.cmds.push(DrawKind::Image { x, y, w, h, data });
    }
}

#[derive(Clone)]
enum Blok {
    Bosluk {
        yuk: f64,
    },
    Satir {
        ogeler: Vec<(f64, Parca)>,
        ust: f64,
        yuk: f64,
    },
    Dikey {
        cocuklar: Vec<Blok>,
    },
    TabloSatiri {
        x: f64,
        hucreler: Vec<(f64, f64, Blok)>,
        min_yuk: f64,
        kenarlik: Option<&'static str>,
    },
}

impl Blok {
    fn yuk(&self) -> f64 {
        match self {
            Blok::Bosluk { yuk } => *yuk,
            Blok::Satir { yuk, .. } => *yuk,
            Blok::Dikey { cocuklar } => cocuklar.iter().map(|c| c.yuk()).sum(),
            Blok::TabloSatiri {
                hucreler, min_yuk, ..
            } => {
                let ic = hucreler
                    .iter()
                    .map(|(_, _, d)| d.yuk())
                    .fold(0.0f64, f64::max);
                min_yuk.max(ic + 2.0 * _HUCRE_PAD_Y)
            }
        }
    }

    fn bol(&self, kalan: f64, zorla: bool) -> (Option<Blok>, Option<Blok>) {
        match self {
            Blok::Bosluk { yuk } => {
                if *yuk <= kalan {
                    (Some(self.clone()), None)
                } else {
                    (None, None)
                }
            }
            Blok::Satir { yuk, .. } => {
                if *yuk <= kalan + 0.01 || zorla {
                    (Some(self.clone()), None)
                } else {
                    (None, Some(self.clone()))
                }
            }
            Blok::Dikey { cocuklar } => {
                let mut bas = Vec::new();
                let mut kullanilan = 0.0;
                for (i, c) in cocuklar.iter().enumerate() {
                    let cy = c.yuk();
                    if kullanilan + cy <= kalan + 0.01 {
                        bas.push(c.clone());
                        kullanilan += cy;
                        continue;
                    }
                    let (h, t) = c.bol(kalan - kullanilan, zorla && bas.is_empty());
                    if let Some(hb) = h {
                        bas.push(hb);
                    }
                    let mut kuyruk = Vec::new();
                    if let Some(tb) = t {
                        kuyruk.push(tb);
                    }
                    kuyruk.extend_from_slice(&cocuklar[i + 1..]);
                    if bas.is_empty() {
                        return (None, Some(self.clone()));
                    }
                    let k_opt = if kuyruk.is_empty() {
                        None
                    } else {
                        Some(Blok::Dikey { cocuklar: kuyruk })
                    };
                    return (Some(Blok::Dikey { cocuklar: bas }), k_opt);
                }
                (Some(self.clone()), None)
            }
            Blok::TabloSatiri {
                x,
                hucreler,
                kenarlik,
                ..
            } => {
                if self.yuk() <= kalan + 0.01 {
                    return (Some(self.clone()), None);
                }
                let ic = kalan - 2.0 * _HUCRE_PAD_Y;
                let mut baslar = Vec::with_capacity(hucreler.len());
                let mut kuyruklar = Vec::with_capacity(hucreler.len());
                let mut ilerledi = false;
                for (dx, w, d) in hucreler {
                    let (h, t) = if ic > 0.0 || zorla {
                        d.bol(ic, zorla)
                    } else {
                        (None, Some(d.clone()))
                    };
                    if h.is_some() {
                        ilerledi = true;
                    }
                    baslar.push((
                        *dx,
                        *w,
                        h.unwrap_or_else(|| Blok::Dikey {
                            cocuklar: Vec::new(),
                        }),
                    ));
                    kuyruklar.push((
                        *dx,
                        *w,
                        t.unwrap_or_else(|| Blok::Dikey {
                            cocuklar: Vec::new(),
                        }),
                    ));
                }
                if !ilerledi {
                    return (None, Some(self.clone()));
                }
                (
                    Some(Blok::TabloSatiri {
                        x: *x,
                        hucreler: baslar,
                        min_yuk: 0.0,
                        kenarlik: *kenarlik,
                    }),
                    Some(Blok::TabloSatiri {
                        x: *x,
                        hucreler: kuyruklar,
                        min_yuk: 0.0,
                        kenarlik: *kenarlik,
                    }),
                )
            }
        }
    }

    fn ciz(&self, c: &mut SayfaTuval, x0: f64, y0: f64) {
        match self {
            Blok::Bosluk { .. } => {}
            Blok::Satir { ogeler, ust, .. } => {
                let taban = y0 + ust;
                let mut kosu: Option<(f64, String, Stil, f64)> = None;
                for (x, p) in ogeler {
                    let can_merge = if matches!(p.tur, ParcaTur::Soz | ParcaTur::Bosluk) {
                        if let Some((_, _, ref k_stil, k_end)) = kosu {
                            p.stil.same_style(k_stil)
                                && (*x - k_end).abs() < 0.05
                                && (p.tur == ParcaTur::Soz
                                    || p.gen <= p.stil.gen(&p.metin) + 0.05)
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if can_merge {
                        if let Some((_, ref mut k_txt, _, ref mut k_end)) = kosu {
                            k_txt.push_str(&p.metin);
                            *k_end = *x + p.gen;
                        }
                        continue;
                    }
                    if let Some((kx, k_txt, k_stil, _)) = kosu.take() {
                        if !k_txt.trim().is_empty() {
                            c.yaz(x0 + kx, taban, k_txt.trim_end_matches(' '), &k_stil);
                        }
                    }
                    if p.tur == ParcaTur::Soz {
                        kosu = Some((*x, p.metin.clone(), p.stil.clone(), *x + p.gen));
                    }
                }
                if let Some((kx, k_txt, k_stil, _)) = kosu.take() {
                    if !k_txt.trim().is_empty() {
                        c.yaz(x0 + kx, taban, k_txt.trim_end_matches(' '), &k_stil);
                    }
                }
                for (x, p) in ogeler {
                    if p.tur == ParcaTur::Resim {
                        if let Some(ref data) = p.resim {
                            c.insert_image(x0 + x, taban - p.yuk, p.gen, p.yuk, Arc::clone(data));
                        }
                    }
                    if matches!(p.tur, ParcaTur::Soz | ParcaTur::Bosluk) && p.stil.alti_cizili {
                        let yy = taban + p.stil.boyut * 0.12;
                        c.draw_line(
                            x0 + x,
                            yy,
                            x0 + x + p.gen,
                            yy,
                            p.stil.renk,
                            (p.stil.boyut * 0.05).max(0.5),
                        );
                    }
                }
            }
            Blok::Dikey { cocuklar } => {
                let mut y = y0;
                for b in cocuklar {
                    b.ciz(c, x0, y);
                    y += b.yuk();
                }
            }
            Blok::TabloSatiri {
                x,
                hucreler,
                kenarlik,
                ..
            } => {
                let h = self.yuk();
                for (dx, w, d) in hucreler {
                    if *kenarlik == Some("hucre") {
                        c.draw_rect(x0 + x + dx, y0, *w, h);
                    }
                    d.ciz(c, x0 + x + dx + _HUCRE_PAD_X, y0 + _HUCRE_PAD_Y);
                }
                if *kenarlik == Some("tablo") && !hucreler.is_empty() {
                    let sag = hucreler
                        .iter()
                        .map(|(dx, w, _)| dx + w)
                        .fold(0.0f64, f64::max);
                    c.draw_rect(x0 + x, y0, sag, h);
                }
            }
        }
    }
}

// --------------------------------------------------------- paragraf yerleşimi

fn _sekme_duraklari(tabset: Option<&str>) -> Vec<(f64, i32)> {
    let mut out = Vec::new();
    for parca in tabset.unwrap_or("").split(',') {
        let alanlar: Vec<&str> = parca.split(':').collect();
        if let Some(first) = alanlar.first() {
            if !first.trim().is_empty() {
                let poz = _f(Some(first), 0.0);
                let hiza = if alanlar.len() > 1 {
                    _f(Some(alanlar[1]), 0.0) as i32
                } else {
                    0
                };
                out.push((poz, hiza));
            }
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn _sonraki_durak(x_goreli: f64, duraklar: &[(f64, i32)]) -> (f64, i32) {
    if duraklar.is_empty() {
        // Açık duraklardaki gibi 0,01 pt pay: 71,99999 gibi yuvarlama artığı aynı durağa düşmesin
        return ((((x_goreli + 0.01) / 72.0).floor() + 1.0) * 72.0, 0);
    }
    for &(poz, hiza) in duraklar {
        if poz > x_goreli + 0.01 {
            return (poz, hiza);
        }
    }
    (x_goreli + 5.0, 0)
}

fn _liste_oneki(par: Node, ctx: &mut Baglam) -> Option<String> {
    if ctx.par_attr(par, "Bulleted", None).as_deref() == Some("true") {
        let bt = ctx.par_attr(par, "BulletType", None).unwrap_or_default();
        return Some(if bt.contains("ARROW") { "»" } else { "•" }.to_string());
    }
    if ctx.par_attr(par, "Numbered", None).as_deref() == Some("true") {
        let lid = ctx
            .par_attr(par, "ListId", None)
            .unwrap_or_else(|| "_".to_string());
        let n = {
            let e = ctx.liste_sayac.entry(lid).or_insert(0);
            *e += 1;
            *e
        };
        let tur = ctx.par_attr(par, "NumberType", None).unwrap_or_default();
        if tur.contains("TRE") {
            return Some(format!("{}-", n));
        } else if tur.contains("PARANTHESE") {
            return Some(format!("{})", n));
        } else {
            return Some(format!("{}.", n));
        }
    }
    None
}

fn image_dimensions(bytes: &[u8]) -> Option<(f64, f64)> {
    // PNG: 89 50 4E 47 ... IHDR at 16..24
    if bytes.len() >= 24 && bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as f64;
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as f64;
        if w > 0.0 && h > 0.0 {
            return Some((w, h));
        }
    }
    // JPEG SOF0/SOF2
    if bytes.len() > 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut i = 2usize;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xD9 || marker == 0xDA {
                break;
            }
            let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as f64;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as f64;
                if w > 0.0 && h > 0.0 {
                    return Some((w, h));
                }
            }
            if seg_len < 2 {
                break;
            }
            i += 2 + seg_len;
        }
    }
    None
}

fn _resim_parcasi(el: Node, stil: Stil, maks_gen: f64) -> Option<Parca> {
    let b64 = el.attribute("imageData").unwrap_or("");
    let veri = B64.decode(b64.trim()).ok()?;
    if veri.is_empty() {
        return None;
    }
    let mut w = _f(el.attribute("width"), 0.0);
    let mut h = _f(el.attribute("height"), 0.0);
    if w <= 0.0 || h <= 0.0 {
        let (pw, ph) = image_dimensions(&veri)?;
        w = pw * 0.75;
        h = ph * 0.75;
    }
    if w > maks_gen && maks_gen > 0.0 {
        h = h * maks_gen / w;
        w = maks_gen;
    }
    if h > 700.0 {
        w = w * 700.0 / h;
        h = 700.0;
    }
    Some(Parca {
        tur: ParcaTur::Resim,
        metin: String::new(),
        stil,
        gen: w,
        resim: Some(Arc::new(veri)),
        yuk: h,
    })
}

static TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\t| +|[^\t ]+").unwrap());

fn _paragraf(par: Node, genislik: f64, ctx: &mut Baglam) -> Vec<Blok> {
    let sol = _f(ctx.par_attr(par, "LeftIndent", None).as_deref(), 0.0);
    let sag = _f(ctx.par_attr(par, "RightIndent", None).as_deref(), 0.0);
    let ilk = _f(ctx.par_attr(par, "FirstLineIndent", None).as_deref(), 0.0);
    let hiza = ctx
        .par_attr(par, "Alignment", Some("0"))
        .unwrap_or_else(|| "0".to_string());
    let ls = _f(ctx.par_attr(par, "LineSpacing", None).as_deref(), 0.0).clamp(0.0, 3.0);
    let duraklar = _sekme_duraklari(ctx.par_attr(par, "TabSet", None).as_deref());
    let sinir = (sol + 10.0).max(genislik - sag);

    let mut parcalar: Vec<Parca> = Vec::new();
    let mut son_stil: Option<Stil> = None;

    for el in par.children().filter(|n| n.is_element()) {
        let tag = el.tag_name().name();
        if matches!(tag, "content" | "field" | "space" | "tab") {
            let stil = ctx.stil(el, par);
            son_stil = Some(stil.clone());
            let mut metin = ctx.dilim(el).replace(['\r', '\n'], "");
            if tag == "tab" && metin.is_empty() {
                metin = "\t".to_string();
            }
            for m in TOKEN_RE.find_iter(&metin) {
                let t = m.as_str();
                if t == "\t" {
                    parcalar.push(Parca {
                        tur: ParcaTur::Sekme,
                        metin: t.to_string(),
                        stil: stil.clone(),
                        gen: 0.0,
                        resim: None,
                        yuk: 0.0,
                    });
                } else if t.starts_with(' ') {
                    let g = stil.gen(t);
                    parcalar.push(Parca {
                        tur: ParcaTur::Bosluk,
                        metin: t.to_string(),
                        stil: stil.clone(),
                        gen: g,
                        resim: None,
                        yuk: 0.0,
                    });
                } else {
                    let g = stil.gen(t);
                    parcalar.push(Parca {
                        tur: ParcaTur::Soz,
                        metin: t.to_string(),
                        stil: stil.clone(),
                        gen: g,
                        resim: None,
                        yuk: 0.0,
                    });
                }
            }
        } else if tag == "image" {
            let stil = ctx.stil(el, par);
            if let Some(p) = _resim_parcasi(el, stil, sinir - sol) {
                parcalar.push(p);
            }
        }
    }

    let son_stil = son_stil.unwrap_or_else(|| ctx.stil(par, par));
    if let Some(onek) = _liste_oneki(par, ctx) {
        let ilk_stil = parcalar
            .iter()
            .find(|p| p.tur == ParcaTur::Soz)
            .map(|p| p.stil.clone())
            .unwrap_or_else(|| son_stil.clone());
        let p1 = Parca {
            tur: ParcaTur::Soz,
            gen: ilk_stil.gen(&onek),
            metin: onek,
            stil: ilk_stil.clone(),
            resim: None,
            yuk: 0.0,
        };
        let p2 = Parca {
            tur: ParcaTur::Bosluk,
            gen: ilk_stil.gen(" "),
            metin: " ".to_string(),
            stil: ilk_stil,
            resim: None,
            yuk: 0.0,
        };
        parcalar.insert(0, p2);
        parcalar.insert(0, p1);
    }

    let mut atomlar: Vec<(ParcaTur, Vec<Parca>)> = Vec::new();
    for p in parcalar {
        if p.tur == ParcaTur::Soz {
            if let Some((ParcaTur::Soz, ref mut grp)) = atomlar.last_mut() {
                grp.push(p);
                continue;
            }
        }
        atomlar.push((p.tur, vec![p]));
    }

    let mut satirlar: Vec<(Vec<(f64, Parca)>, isize, bool)> = Vec::new();
    let mut cur: Vec<(f64, Parca)> = Vec::new();
    let mut x = sol + ilk;
    let mut son_sekme: isize = -1;

    let satir_bitir = |cur: &mut Vec<(f64, Parca)>,
                       x: &mut f64,
                       son_sekme: &mut isize,
                       satirlar: &mut Vec<(Vec<(f64, Parca)>, isize, bool)>,
                       son: bool| {
        while cur
            .last()
            .is_some_and(|(_, p)| matches!(p.tur, ParcaTur::Bosluk | ParcaTur::Sekme))
        {
            cur.pop();
        }
        satirlar.push((std::mem::take(cur), *son_sekme, son));
        *x = sol;
        *son_sekme = -1;
    };

    let icerik_var = |cur: &[(f64, Parca)]| {
        cur.iter()
            .any(|(_, p)| matches!(p.tur, ParcaTur::Soz | ParcaTur::Resim | ParcaTur::Sekme))
    };

    for (i, (tur, grup)) in atomlar.iter().enumerate() {
        match tur {
            ParcaTur::Soz => {
                let w: f64 = grup.iter().map(|p| p.gen).sum();
                if x + w > sinir + 0.01 && icerik_var(&cur) {
                    satir_bitir(&mut cur, &mut x, &mut son_sekme, &mut satirlar, false);
                }
                if w > sinir - sol {
                    for p in grup {
                        for ch in p.metin.chars() {
                            let ch_s = ch.to_string();
                            let cw = p.stil.gen(&ch_s);
                            if x + cw > sinir + 0.01 && icerik_var(&cur) {
                                satir_bitir(&mut cur, &mut x, &mut son_sekme, &mut satirlar, false);
                            }
                            cur.push((
                                x,
                                Parca {
                                    tur: ParcaTur::Soz,
                                    metin: ch_s,
                                    stil: p.stil.clone(),
                                    gen: cw,
                                    resim: None,
                                    yuk: 0.0,
                                },
                            ));
                            x += cw;
                        }
                    }
                    continue;
                }
                for p in grup {
                    cur.push((x, p.clone()));
                    x += p.gen;
                }
            }
            ParcaTur::Bosluk => {
                if cur.is_empty() && !satirlar.is_empty() {
                    continue;
                }
                let p = &grup[0];
                cur.push((x, p.clone()));
                x += p.gen;
            }
            ParcaTur::Sekme => {
                let p = &grup[0];
                let (poz, th) = _sonraki_durak(x - sol, &duraklar);
                let mut yeni = sol + poz;
                if matches!(th, 1 | 2 | 4) {
                    let mut seg = 0.0;
                    for (t2, g2) in &atomlar[i + 1..] {
                        if *t2 == ParcaTur::Sekme {
                            break;
                        }
                        seg += g2.iter().map(|q| q.gen).sum::<f64>();
                    }
                    yeni = x.max(yeni - if th == 2 { seg / 2.0 } else { seg });
                }
                if yeni > sinir && icerik_var(&cur) {
                    satir_bitir(&mut cur, &mut x, &mut son_sekme, &mut satirlar, false);
                    continue;
                }
                cur.push((
                    x,
                    Parca {
                        tur: ParcaTur::Sekme,
                        metin: "\t".to_string(),
                        stil: p.stil.clone(),
                        gen: yeni - x,
                        resim: None,
                        yuk: 0.0,
                    },
                ));
                x = yeni;
                son_sekme = cur.len() as isize;
            }
            ParcaTur::Resim => {
                let p = &grup[0];
                if x + p.gen > sinir + 0.01 && icerik_var(&cur) {
                    satir_bitir(&mut cur, &mut x, &mut son_sekme, &mut satirlar, false);
                }
                cur.push((x, p.clone()));
                x += p.gen;
            }
        }
    }
    satir_bitir(&mut cur, &mut x, &mut son_sekme, &mut satirlar, true);

    let mut bloklar = vec![Blok::Bosluk {
        yuk: _f(ctx.par_attr(par, "SpaceAbove", None).as_deref(), 0.0).max(0.0),
    }];

    for (mut ogeler, son_sekme, son) in satirlar {
        let bit = ogeler
            .iter()
            .map(|(ox, p)| ox + p.gen)
            .fold(0.0f64, f64::max);
        let fazla = sinir - bit;
        if !ogeler.is_empty() && fazla > 0.0 {
            if hiza == "1" {
                for o in &mut ogeler {
                    o.0 += fazla / 2.0;
                }
            } else if hiza == "2" {
                for o in &mut ogeler {
                    o.0 += fazla;
                }
            } else if hiza == "3" && !son {
                let bas = son_sekme.max(0) as usize;
                let aralik_sayisi = (bas..ogeler.len())
                    .filter(|&j| ogeler[j].1.tur == ParcaTur::Bosluk)
                    .count();
                if aralik_sayisi > 0 {
                    let pay = fazla / (aralik_sayisi as f64);
                    let mut ek = 0.0;
                    for j in bas..ogeler.len() {
                        ogeler[j].0 += ek;
                        if ogeler[j].1.tur == ParcaTur::Bosluk {
                            ogeler[j].1.gen += pay;
                            ek += pay;
                        }
                    }
                }
            }
        }
        let mut metrik: Vec<Stil> = ogeler
            .iter()
            .filter(|(_, p)| p.tur != ParcaTur::Resim)
            .map(|(_, p)| p.stil.clone())
            .collect();
        if metrik.is_empty() {
            metrik.push(son_stil.clone());
        }
        let mut ust = metrik.iter().map(|s| s.ust()).fold(0.0f64, f64::max);
        let alt = metrik.iter().map(|s| s.alt()).fold(0.0f64, f64::max);
        let resim_ust = ogeler
            .iter()
            .filter(|(_, p)| p.tur == ParcaTur::Resim)
            .map(|(_, p)| p.yuk)
            .fold(0.0f64, f64::max);
        ust = ust.max(resim_ust);
        let max_boyut = metrik.iter().map(|s| s.boyut).fold(0.0f64, f64::max);
        let yuk = (ust + alt + 0.04 * max_boyut) * (1.0 + ls);
        bloklar.push(Blok::Satir { ogeler, ust, yuk });
    }

    bloklar.push(Blok::Bosluk {
        yuk: _f(ctx.par_attr(par, "SpaceBelow", None).as_deref(), 0.0).max(0.0),
    });
    bloklar
}

fn _tablo(tbl: Node, genislik: f64, ctx: &mut Baglam) -> Vec<Blok> {
    let sol = _f(tbl.attribute("LeftIndent"), 0.0);
    let tg = (genislik - sol - _f(tbl.attribute("RightIndent"), 0.0)).max(20.0);
    let oran_t = _oranlar(tbl.attribute("columnSpans"));
    let kenar = match tbl.attribute("border").unwrap_or("borderCell") {
        "borderCell" => Some("hucre"),
        "borderTable" => Some("tablo"),
        _ => None,
    };
    let mut bloklar = vec![Blok::Bosluk {
        yuk: _f(tbl.attribute("SpaceAbove"), 0.0).max(0.0),
    }];
    for row in tbl.children().filter(|n| n.tag_name().name() == "row") {
        let hucreler: Vec<Node> = row
            .children()
            .filter(|n| n.tag_name().name() == "cell")
            .collect();
        if hucreler.is_empty() {
            continue;
        }
        let mut oran = _oranlar(row.attribute("columnSpans"));
        if oran.len() != hucreler.len() {
            oran = if oran_t.len() == hucreler.len() {
                oran_t.clone()
            } else {
                vec![1.0; hucreler.len()]
            };
        }
        let top: f64 = oran.iter().sum();
        let mut dx = 0.0;
        let mut liste = Vec::with_capacity(hucreler.len());
        for (cell, o) in hucreler.into_iter().zip(oran.into_iter()) {
            let w = tg * o / top;
            let cocuk_elemanlar: Vec<Node> = cell.children().filter(|n| n.is_element()).collect();
            let ic = _kap(&cocuk_elemanlar, (w - 2.0 * _HUCRE_PAD_X).max(5.0), ctx);
            liste.push((dx, w, Blok::Dikey { cocuklar: ic }));
            dx += w;
        }
        bloklar.push(Blok::TabloSatiri {
            x: sol,
            hucreler: liste,
            min_yuk: _f(row.attribute("height"), 0.0),
            kenarlik: kenar,
        });
    }
    bloklar.push(Blok::Bosluk {
        yuk: _f(tbl.attribute("SpaceBelow"), 0.0).max(0.0),
    });
    bloklar
}

fn _kap(elemanlar: &[Node], genislik: f64, ctx: &mut Baglam) -> Vec<Blok> {
    let mut out = Vec::new();
    for &el in elemanlar {
        match el.tag_name().name() {
            "paragraph" => out.extend(_paragraf(el, genislik, ctx)),
            "table" => out.extend(_tablo(el, genislik, ctx)),
            _ => {}
        }
    }
    out
}

fn _gorunur(el: Node, ctx: &Baglam) -> bool {
    for c in el.descendants() {
        let t = c.tag_name().name();
        if t == "image" {
            return true;
        }
        if matches!(t, "content" | "field" | "space") && !ctx.dilim(c).trim().is_empty() {
            return true;
        }
    }
    false
}

static SPEC_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+)$").unwrap());

fn _sayfa_no_metni(el: Node, n: usize, toplam: usize) -> Option<(String, &'static str)> {
    let spec_str = el.attribute("pageNumber-spec").unwrap_or("");
    let spec: usize = SPEC_RE
        .captures(spec_str)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    if spec == 0 {
        return None;
    }
    let bas: usize = el
        .attribute("pageNumber-pageStartNumStr")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let n2 = n + bas - 1;
    let top2 = toplam + bas - 1;
    let mut metin = n2.to_string();
    if (spec & 2048) != 0 {
        let sep = el.attribute("pageNumber-seperator").unwrap_or("/");
        metin.push_str(sep);
        metin.push_str(&top2.to_string());
    }
    let on = el.attribute("pageNumber-foreStr").unwrap_or("").trim();
    if !on.is_empty() {
        metin = format!("{} {}", on, metin);
    }
    let hiza = if (spec & 64) != 0 {
        "2"
    } else if (spec & 32) != 0 {
        "1"
    } else {
        "0"
    };
    Some((metin, hiza))
}

// ---------------------------------------------------------------- PDF Üretici

fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_else(|_| data.to_vec())
}

pub(crate) fn decode_png_rgb(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.len() < 33 || !bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return None;
    }
    let mut pos = 8usize;
    let mut w = 0u32;
    let mut h = 0u32;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut idat = Vec::new();
    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
        let ctype = &bytes[pos + 4..pos + 8];
        if pos + 12 + len > bytes.len() {
            break;
        }
        let data = &bytes[pos + 8..pos + 8 + len];
        if ctype == b"IHDR" && len >= 13 {
            w = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            h = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            bit_depth = data[8];
            color_type = data[9];
        } else if ctype == b"IDAT" {
            idat.extend_from_slice(data);
        } else if ctype == b"IEND" {
            break;
        }
        pos += 12 + len;
    }
    if w == 0 || h == 0 || bit_depth != 8 || !matches!(color_type, 2 | 6) {
        return None;
    }
    let mut dec = ZlibDecoder::new(&idat[..]);
    let mut raw = Vec::new();
    dec.read_to_end(&mut raw).ok()?;
    let bpp: usize = if color_type == 6 { 4 } else { 3 };
    let stride = (w as usize) * bpp;
    let mut recon = vec![0u8; (h as usize) * stride];
    for row in 0..h as usize {
        let src_start = row * (stride + 1);
        if src_start + 1 + stride > raw.len() {
            return None;
        }
        let filter = raw[src_start];
        let src = &raw[src_start + 1..src_start + 1 + stride];
        let dst_start = row * stride;
        for col in 0..stride {
            let a = if col >= bpp { recon[dst_start + col - bpp] } else { 0 };
            let b = if row > 0 { recon[dst_start - stride + col] } else { 0 };
            let c = if row > 0 && col >= bpp {
                recon[dst_start - stride + col - bpp]
            } else {
                0
            };
            let x = src[col];
            recon[dst_start + col] = match filter {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => {
                    let p = a as i32 + b as i32 - c as i32;
                    let pa = (p - a as i32).abs();
                    let pb = (p - b as i32).abs();
                    let pc = (p - c as i32).abs();
                    let pr = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    x.wrapping_add(pr)
                }
                _ => x,
            };
        }
    }
    let mut rgb = Vec::with_capacity((w as usize) * (h as usize) * 3);
    if color_type == 6 {
        for px in recon.chunks_exact(4) {
            // Alpha compositing on white
            let a = px[3] as u16;
            let inv = 255 - a;
            rgb.push(((px[0] as u16 * a + 255 * inv) / 255) as u8);
            rgb.push(((px[1] as u16 * a + 255 * inv) / 255) as u8);
            rgb.push(((px[2] as u16 * a + 255 * inv) / 255) as u8);
        }
    } else {
        rgb = recon;
    }
    Some((w, h, rgb))
}

fn build_pdf_bytes(sayfalar: &[SayfaTuval]) -> Vec<u8> {
    let mut objs: Vec<Vec<u8>> = Vec::new();
    let add_obj = |objs: &mut Vec<Vec<u8>>, data: Vec<u8>| -> usize {
        objs.push(data);
        objs.len()
    };

    // Reserve obj 1 (Catalog) & obj 2 (Pages)
    let catalog_id = add_obj(&mut objs, Vec::new());
    let pages_id = add_obj(&mut objs, Vec::new());

    // Collect unique fonts used across pages
    let mut font_map: HashMap<String, (usize, String, Arc<ParsedTtf>, BTreeMap<u16, char>)> =
        HashMap::new();
    let mut font_counter = 0usize;

    for s in sayfalar {
        for cmd in &s.cmds {
            if let DrawKind::Text { metin, stil, .. } = cmd {
                let entry = font_map.entry(stil.font.key.clone()).or_insert_with(|| {
                    font_counter += 1;
                    (
                        font_counter,
                        format!("F{}", font_counter),
                        Arc::clone(&stil.font),
                        BTreeMap::new(),
                    )
                });
                for ch in metin.chars() {
                    let gid = entry.2.gid(ch);
                    entry.3.entry(gid).or_insert(ch);
                }
            }
        }
    }

    // Emit PDF font objects
    let mut font_obj_ids: HashMap<String, (String, usize)> = HashMap::new();
    for (key, (_, fname, ttf, used_gids)) in &font_map {
        if let Some(ref raw_ttf) = ttf.raw {
            let compressed_ttf = zlib_compress(raw_ttf);
            let mut ff_dict = format!(
                "<< /Length {} /Length1 {} /Filter /FlateDecode >>\nstream\n",
                compressed_ttf.len(),
                raw_ttf.len()
            )
            .into_bytes();
            ff_dict.extend_from_slice(&compressed_ttf);
            ff_dict.extend_from_slice(b"\nendstream");
            let ff_id = add_obj(&mut objs, ff_dict);

            let scale = 1000.0 / (ttf.units_per_em as f64);
            let asc = (ttf.ascender * 1000.0).round() as i32;
            let desc = (ttf.descender * 1000.0).round() as i32;
            let bbox_str = format!(
                "[{} {} {} {}]",
                (ttf.bbox[0] as f64 * scale).round() as i32,
                (ttf.bbox[1] as f64 * scale).round() as i32,
                (ttf.bbox[2] as f64 * scale).round() as i32,
                (ttf.bbox[3] as f64 * scale).round() as i32
            );
            let ps_name = format!("RUYAP+{}", fname);
            let fd_id = add_obj(
                &mut objs,
                format!(
                    "<< /Type /FontDescriptor /FontName /{} /Flags 32 /FontBBox {} /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {} 0 R >>",
                    ps_name, bbox_str, asc, desc, asc, ff_id
                )
                .into_bytes(),
            );

            let mut w_str = String::from("[");
            for &gid in used_gids.keys() {
                let adv = ttf
                    .advances
                    .get(gid as usize)
                    .copied()
                    .unwrap_or(ttf.units_per_em / 2);
                let w = ((adv as f64) * scale).round() as i32;
                w_str.push_str(&format!("{} [{}] ", gid, w));
            }
            w_str.push(']');

            let cid_id = add_obj(
                &mut objs,
                format!(
                    "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {} 0 R /W {} /CIDToGIDMap /Identity >>",
                    ps_name, fd_id, w_str
                )
                .into_bytes(),
            );

            // ToUnicode CMap
            let mut cmap_txt = String::from(
                "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
            );
            let entries: Vec<(u16, char)> = used_gids.iter().map(|(&g, &c)| (g, c)).collect();
            for chunk in entries.chunks(100) {
                cmap_txt.push_str(&format!("{} beginbfchar\n", chunk.len()));
                for &(g, c) in chunk {
                    cmap_txt.push_str(&format!("<{:04X}> <{:04X}>\n", g, c as u32));
                }
                cmap_txt.push_str("endbfchar\n");
            }
            cmap_txt.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend");
            let cmap_comp = zlib_compress(cmap_txt.as_bytes());
            let mut tu_bytes = format!(
                "<< /Length {} /Filter /FlateDecode >>\nstream\n",
                cmap_comp.len()
            )
            .into_bytes();
            tu_bytes.extend_from_slice(&cmap_comp);
            tu_bytes.extend_from_slice(b"\nendstream");
            let tu_id = add_obj(&mut objs, tu_bytes);

            let t0_id = add_obj(
                &mut objs,
                format!(
                    "<< /Type /Font /Subtype /Type0 /BaseFont /{} /Encoding /Identity-H /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>",
                    ps_name, cid_id, tu_id
                )
                .into_bytes(),
            );
            font_obj_ids.insert(key.clone(), (fname.clone(), t0_id));
        } else {
            let f_id = add_obj(
                &mut objs,
                format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                    ttf.base14_name
                )
                .into_bytes(),
            );
            font_obj_ids.insert(key.clone(), (fname.clone(), f_id));
        }
    }

    let mut page_ids = Vec::with_capacity(sayfalar.len());
    let mut img_counter = 0usize;

    for s in sayfalar {
        let mut xobjects = Vec::new();
        let mut stream = String::new();
        let mut used_fonts_on_page: HashMap<String, usize> = HashMap::new();

        for cmd in &s.cmds {
            match cmd {
                DrawKind::Text { x, y, metin, stil } => {
                    if let Some((fname, fobj_id)) = font_obj_ids.get(&stil.font.key) {
                        used_fonts_on_page.insert(fname.clone(), *fobj_id);
                        let py = s.h - y;
                        stream.push_str(&format!(
                            "BT /{fname} {sz:.2} Tf {r:.3} {g:.3} {b:.3} rg 1 0 0 1 {x:.2} {py:.2} Tm ",
                            fname = fname,
                            sz = stil.boyut,
                            r = stil.renk.0,
                            g = stil.renk.1,
                            b = stil.renk.2,
                            x = x,
                            py = py
                        ));
                        if stil.font.raw.is_some() {
                            stream.push('<');
                            for ch in metin.chars() {
                                let gid = stil.font.gid(ch);
                                stream.push_str(&format!("{:04X}", gid));
                            }
                            stream.push_str("> Tj ET\n");
                        } else {
                            stream.push('(');
                            for ch in metin.chars() {
                                match ch {
                                    '\\' => stream.push_str("\\\\"),
                                    '(' => stream.push_str("\\("),
                                    ')' => stream.push_str("\\)"),
                                    c if (c as u32) < 128 => stream.push(c),
                                    _ => stream.push('?'),
                                }
                            }
                            stream.push_str(") Tj ET\n");
                        }
                    }
                }
                DrawKind::Rect { x, y, w, h } => {
                    let py = s.h - (y + h);
                    stream.push_str(&format!(
                        "q 0 0 0 RG 0.5 w {:.2} {:.2} {:.2} {:.2} re S Q\n",
                        x, py, w, h
                    ));
                }
                DrawKind::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                    width,
                } => {
                    let py1 = s.h - y1;
                    let py2 = s.h - y2;
                    stream.push_str(&format!(
                        "q {:.3} {:.3} {:.3} RG {:.2} w {:.2} {:.2} m {:.2} {:.2} l S Q\n",
                        color.0, color.1, color.2, width, x1, py1, x2, py2
                    ));
                }
                DrawKind::Image { x, y, w, h, data } => {
                    if data.len() > 4 && data[0] == 0xFF && data[1] == 0xD8 {
                        if let Some((iw, ih)) = image_dimensions(data) {
                            img_counter += 1;
                            let im_name = format!("Im{}", img_counter);
                            let mut im_obj = format!(
                                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
                                iw as u32, ih as u32, data.len()
                            ).into_bytes();
                            im_obj.extend_from_slice(data);
                            im_obj.extend_from_slice(b"\nendstream");
                            let im_id = add_obj(&mut objs, im_obj);
                            xobjects.push((im_name.clone(), im_id));
                            let py = s.h - (y + h);
                            stream.push_str(&format!(
                                "q {:.2} 0 0 {:.2} {:.2} {:.2} cm /{} Do Q\n",
                                w, h, x, py, im_name
                            ));
                        }
                    } else if let Some((iw, ih, rgb)) = decode_png_rgb(data) {
                        img_counter += 1;
                        let im_name = format!("Im{}", img_counter);
                        let comp = zlib_compress(&rgb);
                        let mut im_obj = format!(
                            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                            iw, ih, comp.len()
                        ).into_bytes();
                        im_obj.extend_from_slice(&comp);
                        im_obj.extend_from_slice(b"\nendstream");
                        let im_id = add_obj(&mut objs, im_obj);
                        xobjects.push((im_name.clone(), im_id));
                        let py = s.h - (y + h);
                        stream.push_str(&format!(
                            "q {:.2} 0 0 {:.2} {:.2} {:.2} cm /{} Do Q\n",
                            w, h, x, py, im_name
                        ));
                    }
                }
            }
        }

        let comp_stream = zlib_compress(stream.as_bytes());
        let mut content_obj = format!(
            "<< /Length {} /Filter /FlateDecode >>\nstream\n",
            comp_stream.len()
        )
        .into_bytes();
        content_obj.extend_from_slice(&comp_stream);
        content_obj.extend_from_slice(b"\nendstream");
        let content_id = add_obj(&mut objs, content_obj);

        let mut font_res = String::from("<< ");
        for (fname, fid) in &used_fonts_on_page {
            font_res.push_str(&format!("/{} {} 0 R ", fname, fid));
        }
        font_res.push_str(">>");

        let mut xobj_res = String::from("<< ");
        for (iname, iid) in &xobjects {
            xobj_res.push_str(&format!("/{} {} 0 R ", iname, iid));
        }
        xobj_res.push_str(">>");

        let page_obj = format!(
            "<< /Type /Page /Parent {} 0 R /MediaBox [0 0 {:.4} {:.4}] /Resources << /Font {} /XObject {} >> /Contents {} 0 R >>",
            pages_id, s.w, s.h, font_res, xobj_res, content_id
        );
        let p_id = add_obj(&mut objs, page_obj.into_bytes());
        page_ids.push(p_id);
    }

    let kids: Vec<String> = page_ids.iter().map(|id| format!("{} 0 R", id)).collect();
    objs[pages_id - 1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        page_ids.len()
    )
    .into_bytes();
    objs[catalog_id - 1] = format!("<< /Type /Catalog /Pages {} 0 R >>", pages_id).into_bytes();

    let mut out = Vec::from(&b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n"[..]);
    let mut offsets = Vec::with_capacity(objs.len() + 1);
    offsets.push(0usize);
    for (idx, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", idx + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_pos = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objs.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for &off in &offsets[1..] {
        out.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root {} 0 R >>\nstartxref\n{}\n%%EOF\n",
            objs.len() + 1,
            catalog_id,
            xref_pos
        )
        .as_bytes(),
    );
    out
}

// --------------------------------------------------------------------- API

/// `ham_bayt` bir UDF (zip + content.xml) mi? — gerçek ZIP imzasına bakar.
pub fn udf_zip_mi(ham_bayt: &[u8]) -> bool {
    if !ham_bayt.starts_with(b"PK") {
        return false;
    }
    let Ok(mut z) = ZipArchive::new(Cursor::new(ham_bayt)) else {
        return false;
    };
    let has = (0..z.len()).any(|i| z.by_index(i).is_ok_and(|f| f.name() == "content.xml"));
    has
}

/// UDF baytlarından PDF üretir, PDF baytlarını döner.
pub fn udf_pdf_uret(ham_bayt: &[u8]) -> Result<Vec<u8>, String> {
    // Portal belgelerinde alanlar metinde adıyla durur, değerler <data>'da: önce yerleştir (udf_alan.rs)
    let dolu = crate::udf_alan::doldur_ya_da_asil(ham_bayt);
    let ham_bayt: &[u8] = &dolu;
    let xml_str = {
        let mut z = ZipArchive::new(Cursor::new(ham_bayt))
            .map_err(|e| format!("UDF ZIP açılamadı: {}", e))?;
        let mut f = z
            .by_name("content.xml")
            .map_err(|e| format!("content.xml bulunamadı: {}", e))?;
        let mut s = String::new();
        f.read_to_string(&mut s)
            .map_err(|e| format!("content.xml okunamadı: {}", e))?;
        s
    };

    let doc = Document::parse(&xml_str).map_err(|e| format!("XML hatası: {}", e))?;
    let root = doc.root_element();

    let full_text = root
        .children()
        .find(|n| n.tag_name().name() == "content")
        .and_then(|n| n.text())
        .unwrap_or("");
    let mut ctx = Baglam::new(root, full_text);

    let pf = root
        .children()
        .find(|n| n.tag_name().name() == "properties")
        .and_then(|p| p.children().find(|n| n.tag_name().name() == "pageFormat"));
    let left_m = _f(pf.and_then(|n| n.attribute("leftMargin")), _VARSAYILAN_MARJ);
    let right_m = _f(pf.and_then(|n| n.attribute("rightMargin")), _VARSAYILAN_MARJ);
    let top_m = _f(pf.and_then(|n| n.attribute("topMargin")), _VARSAYILAN_MARJ);
    let bottom_m = _f(pf.and_then(|n| n.attribute("bottomMargin")), _VARSAYILAN_MARJ);
    let hdr_off = _f(pf.and_then(|n| n.attribute("headerFOffset")), 15.0);
    let ftr_off = _f(pf.and_then(|n| n.attribute("footerFOffset")), 15.0);
    let (sw, sh) = if pf.and_then(|n| n.attribute("paperOrientation")) == Some("0") {
        (_A4_H, _A4_W)
    } else {
        (_A4_W, _A4_H)
    };
    let usable_w = (sw - left_m - right_m).max(50.0);

    let elemanlar: Vec<Node> = root
        .children()
        .find(|n| n.tag_name().name() == "elements")
        .map(|el| el.children().filter(|n| n.is_element()).collect())
        .unwrap_or_default();

    let header_el = elemanlar
        .iter()
        .copied()
        .find(|n| n.tag_name().name() == "header");
    let footer_el = elemanlar
        .iter()
        .copied()
        .find(|n| n.tag_name().name() == "footer");

    let mut kenar_blok = |el_opt: Option<Node>| -> Option<Blok> {
        let el = el_opt?;
        if !_gorunur(el, &ctx) {
            return None;
        }
        ctx.varsayilan_renk = _argb_rgb(el.attribute("foreground")).unwrap_or((0.0, 0.0, 0.0));
        let children: Vec<Node> = el.children().filter(|n| n.is_element()).collect();
        let blok = Blok::Dikey {
            cocuklar: _kap(&children, usable_w, &mut ctx),
        };
        ctx.varsayilan_renk = (0.0, 0.0, 0.0);
        Some(blok)
    };

    let hdr = kenar_blok(header_el);
    let ftr = kenar_blok(footer_el);

    let aralikta = |el_opt: Option<Node>, n: usize| -> bool {
        let Some(el) = el_opt else {
            return false;
        };
        let bas = _f(el.attribute("startPage"), 1.0) as isize;
        let bit = _f(el.attribute("stopPage"), 0.0) as isize;
        let ni = n as isize;
        ni >= bas && (bit <= 0 || ni <= bit)
    };

    let govde_elemanlar: Vec<Node> = elemanlar
        .into_iter()
        .filter(|n| !matches!(n.tag_name().name(), "header" | "footer"))
        .collect();
    let govde = Blok::Dikey {
        cocuklar: _kap(&govde_elemanlar, usable_w, &mut ctx),
    };

    let mut sayfalar: Vec<SayfaTuval> = Vec::new();
    let mut kalan: Option<Blok> = Some(govde);

    loop {
        let n = sayfalar.len() + 1;
        let mut ust = top_m;
        if let Some(ref h) = hdr {
            if aralikta(header_el, n) {
                ust = ust.max(hdr_off + h.yuk());
            }
        }
        let mut alt = sh - bottom_m;
        if let Some(ref f) = ftr {
            if aralikta(footer_el, n) {
                alt = alt.min(sh - ftr_off - f.yuk());
            }
        }
        let kullanilabilir = (alt - ust).max(20.0);
        let mut bas = None;
        if let Some(ref k) = kalan {
            let (b, t) = k.bol(kullanilabilir, false);
            if b.is_none() {
                let (b2, t2) = k.bol(kullanilabilir, true);
                bas = b2;
                kalan = t2;
            } else {
                bas = b;
                kalan = t;
            }
        }
        let mut tuval = SayfaTuval {
            w: sw,
            h: sh,
            cmds: Vec::new(),
        };
        if let Some(b) = bas {
            b.ciz(&mut tuval, left_m, ust);
        }
        sayfalar.push(tuval);
        if kalan.is_none() || n >= 3000 {
            break;
        }
    }

    let toplam = sayfalar.len();
    for (idx, tuval) in sayfalar.iter_mut().enumerate() {
        let n = idx + 1;
        if let Some(ref h) = hdr {
            if aralikta(header_el, n) {
                h.ciz(tuval, left_m, hdr_off);
            }
        }
        if let Some(ref f) = ftr {
            if aralikta(footer_el, n) {
                f.ciz(tuval, left_m, sh - ftr_off - f.yuk());
            }
        }
        if let Some(fe) = footer_el {
            if aralikta(footer_el, n) {
                if let Some((metin, hiza)) = _sayfa_no_metni(fe, n, toplam) {
                    let font = ctx.fontlar.al(
                        fe.attribute("pageNumber-fontFace").unwrap_or("Arial"),
                        fe.attribute("pageNumber-fontBold") == Some("true"),
                        fe.attribute("pageNumber-fontItalic") == Some("true"),
                    );
                    let mut sz = _f(fe.attribute("pageNumber-fontSize"), 10.0);
                    if sz <= 0.0 {
                        sz = 10.0;
                    }
                    let color =
                        _argb_rgb(fe.attribute("pageNumber-color")).unwrap_or((0.0, 0.0, 0.0));
                    let stil = Stil {
                        font,
                        boyut: sz,
                        renk: color,
                        alti_cizili: false,
                    };
                    let w = stil.gen(&metin);
                    let x = left_m
                        + match hiza {
                            "1" => (usable_w - w) / 2.0,
                            "2" => usable_w - w,
                            _ => 0.0,
                        };
                    tuval.yaz(x, sh - ftr_off, &metin, &stil);
                }
            }
        }
    }

    Ok(build_pdf_bytes(&sayfalar))
}
