//! Armonia Yazı editörünün derlenmiş arayüzünü sunar: `/__cu__/yazi/…` (2026-11-23).
//!
//! Dosyalar `uyap_core/yazi/` altında (araclar/yazi_al.py ile editör deposundan alınır). Editör ÇokluUYAP
//! sayfasının içindeki açılır pencerede (enjekte/yazi.js) aynı adresten çerçeve olarak açılır: aynı köken olduğu için
//! çoklu pano (Ctrl+1…0) ve tema köprüsü editörün kodunu değiştirmeden çalışır. Tünelden gelen kullanıcıya da
//! (office_agent) aynı dosyalar verilir. Editör ağa çıkmaz (CSP); belge yalnız bu bilgisayarda kalır.

use std::collections::HashMap;
use std::path::PathBuf;

const ON_EK: &str = "__cu__/yazi";

pub fn is_path(path: &str) -> bool {
    let p = path.trim_start_matches('/');
    p == ON_EK || p.starts_with("__cu__/yazi/")
}

/// Editör klasörü: exe'nin yanında, çalışma klasöründe ya da kaynak ağacında `uyap_core/yazi`.
pub fn klasor() -> PathBuf {
    let mut adaylar: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        adaylar.push(d.join("uyap_core").join("yazi"));
    }
    if let Ok(c) = std::env::current_dir() {
        adaylar.push(c.join("uyap_core").join("yazi"));
    }
    adaylar.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("uyap_core").join("yazi"));
    adaylar.iter().find(|p| p.join("index.html").is_file()).cloned().unwrap_or_else(|| adaylar.remove(0))
}

fn tur(ad: &str) -> &'static str {
    match ad.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "",
    }
}

/// Güvenli göreli yol: yalnız harf, rakam, `.`, `_`, `-` içeren parçalar; `..` ve gizli dosya yok.
fn yol_coz(path: &str) -> Option<String> {
    let p = path.split(['?', '#']).next().unwrap_or("").trim_start_matches('/');
    let geri = p.strip_prefix(ON_EK)?.trim_start_matches('/');
    if geri.is_empty() {
        return Some("index.html".into());
    }
    let parcalar: Vec<&str> = geri.split('/').collect();
    if parcalar.len() > 3 {
        return None;
    }
    for c in &parcalar {
        if c.is_empty() || c.starts_with('.') || !c.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
            return None;
        }
    }
    let son = parcalar.last()?;
    if tur(son).is_empty() {
        return None;
    }
    Some(parcalar.join("/"))
}

pub fn yanit(method: &str, path: &str) -> (u16, HashMap<String, String>, Vec<u8>) {
    let mut h = HashMap::new();
    h.insert("X-Content-Type-Options".to_string(), "nosniff".to_string());
    h.insert("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
    if !method.eq_ignore_ascii_case("GET") && !method.eq_ignore_ascii_case("HEAD") {
        return (405, h, b"Yalniz GET.".to_vec());
    }
    // `/__cu__/yazi` → sonda / olmadan göreli yollar kırılmasın diye yönlendir.
    if path.split(['?', '#']).next().unwrap_or("").trim_start_matches('/') == ON_EK {
        h.insert("Location".to_string(), "/__cu__/yazi/".to_string());
        return (308, h, Vec::new());
    }
    let Some(goreli) = yol_coz(path) else { return (404, h, b"Bulunamadi.".to_vec()) };
    let dosya = klasor().join(&goreli);
    let Ok(veri) = std::fs::read(&dosya) else {
        return (404, h, "Armonia Yazı bu kurulumda yok (uyap_core/yazi).".as_bytes().to_vec());
    };
    h.insert("Content-Type".to_string(), tur(&goreli).to_string());
    if goreli == "index.html" {
        h.insert("Cache-Control".to_string(), "no-cache".to_string());
        // Sayfadaki CSP'ye ek: yalnız ÇokluUYAP sayfasının içinde çerçevelenebilir.
        h.insert(
            "Content-Security-Policy".to_string(),
            "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: blob:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; \
             frame-ancestors 'self'"
                .to_string(),
        );
    } else {
        // Vite dosya adlarında içerik özeti var: değişen dosya yeni ad alır.
        h.insert("Cache-Control".to_string(), "public, max-age=31536000, immutable".to_string());
    }
    let govde = if method.eq_ignore_ascii_case("HEAD") { Vec::new() } else { veri };
    (200, h, govde)
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn yollar() {
        assert!(is_path("__cu__/yazi/"));
        assert!(is_path("/__cu__/yazi/assets/a.js"));
        assert!(is_path("__cu__/yazi"));
        assert!(!is_path("__cu__/yazilar"));
        assert_eq!(yol_coz("__cu__/yazi/").as_deref(), Some("index.html"));
        assert_eq!(yol_coz("/__cu__/yazi/assets/index-nfF_Bhv9.js?x=1").as_deref(), Some("assets/index-nfF_Bhv9.js"));
        assert_eq!(yol_coz("__cu__/yazi/assets/belge_wasm_bg-DO5Z819E.wasm").as_deref(), Some("assets/belge_wasm_bg-DO5Z819E.wasm"));
        for kotu in ["__cu__/yazi/../accounts.json", "__cu__/yazi/assets/..%2Fx.js", "__cu__/yazi/.gizli.js",
                     "__cu__/yazi/KAYNAK.txt", "__cu__/yazi/a//b.js", "__cu__/yazi/a\\b.js", "__cu__/yazi/a/b/c/d.js"] {
            assert!(yol_coz(kotu).is_none(), "{kotu} kabul edildi");
        }
        assert_eq!(yanit("POST", "__cu__/yazi/").0, 405);
        assert_eq!(yanit("GET", "__cu__/yazi").0, 308);
    }
}
