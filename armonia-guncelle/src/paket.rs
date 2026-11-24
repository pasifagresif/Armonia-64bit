//! Paket (`.tar.zst`): üretme (yayıncı) ve açma (cihaz). Açıcı tar'ın kendi açma işlevini KULLANMAZ: her girdi
//! burada denetlenir, dosyalar kendimiz yazarız (sahip, kip, zaman tar'dan alınmaz).
//!
//! İçerik beyaz listesi: `uyap_app`, `SURUM`, `webapp/**`, `uyap_core/enjekte/**` ve `MANIFEST`.
//! `MANIFEST`: her satır `<sha256> <yol>`, yola göre sıralı; özeti imzalı bilgideki `agac_ozeti`dir.

use crate::{hata, hex64_mi, ozet_hex, Sonuc};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const DOSYA_SAYI_AZAMI: usize = 5000;
pub const ACIK_AZAMI: u64 = 512 * 1024 * 1024;
pub const MANIFEST_AZAMI: u64 = 1024 * 1024;
pub const MANIFEST: &str = "MANIFEST";

pub fn yol_gecerli(y: &str) -> bool {
    if y.is_empty() || y.len() > 512 || y.starts_with('/') {
        return false;
    }
    if y.chars().any(|c| c == '\0' || c == '\\' || c.is_control()) {
        return false;
    }
    y.split('/').all(|c| !c.is_empty() && c != "." && c != ".." && c.len() <= 255)
}

pub fn izinli_dosya(y: &str) -> bool {
    yol_gecerli(y) && (y == "uyap_app" || y == "SURUM" || y.starts_with("webapp/") || y.starts_with("uyap_core/enjekte/"))
}

fn izinli_dizin(y: &str) -> bool {
    yol_gecerli(y)
        && (y == "webapp" || y == "uyap_core" || y == "uyap_core/enjekte" || y.starts_with("webapp/") || y.starts_with("uyap_core/enjekte/"))
}

pub fn manifest_yaz(dosyalar: &BTreeMap<String, String>) -> Vec<u8> {
    let mut v = Vec::new();
    for (y, o) in dosyalar {
        v.extend_from_slice(format!("{o} {y}\n").as_bytes());
    }
    v
}

pub fn manifest_coz(bayt: &[u8]) -> Sonuc<BTreeMap<String, String>> {
    let m = std::str::from_utf8(bayt).map_err(|_| hata!("MANIFEST UTF-8 değil"))?;
    let mut sonuc = BTreeMap::new();
    let mut onceki: Option<&str> = None;
    if !m.is_empty() && !m.ends_with('\n') {
        return Err(hata!("MANIFEST satır sonuyla bitmiyor"));
    }
    for satir in m.lines() {
        let (o, y) = satir.split_once(' ').ok_or_else(|| hata!("MANIFEST satırı bozuk"))?;
        if !hex64_mi(o) || !izinli_dosya(y) {
            return Err(hata!("MANIFEST satırı geçersiz: {y:?}"));
        }
        if onceki.is_some_and(|p| p >= y) {
            return Err(hata!("MANIFEST sıralı ve tekil değil"));
        }
        onceki = Some(y);
        sonuc.insert(y.to_string(), o.to_string());
    }
    if !sonuc.contains_key("uyap_app") {
        return Err(hata!("MANIFEST'te uyap_app yok"));
    }
    if sonuc.len() > DOSYA_SAYI_AZAMI {
        return Err(hata!("MANIFEST'te çok fazla dosya"));
    }
    Ok(sonuc)
}

/// ELF başlığı: 64 bit, küçük endian, beklenen makine.
pub fn elf_mimari_tutuyor(bas: &[u8], mimari: &str) -> bool {
    if bas.len() < 20 || &bas[..4] != b"\x7fELF" || bas[4] != 2 || bas[5] != 1 {
        return false;
    }
    let m = u16::from_le_bytes([bas[18], bas[19]]);
    matches!((mimari, m), ("x86_64", 0x3e) | ("aarch64", 0xb7))
}

struct Sinirli<R> {
    ic: R,
    kalan: u64,
}

impl<R: Read> Read for Sinirli<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.ic.read(buf)?;
        if n as u64 > self.kalan {
            return Err(std::io::Error::other("açılmış paket boyut sınırını aşıyor"));
        }
        self.kalan -= n as u64;
        Ok(n)
    }
}

fn yeni_dosya(yol: &Path, calistirilabilir: bool) -> Sonuc<fs::File> {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(if calistirilabilir { 0o755 } else { 0o644 }).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    let _ = calistirilabilir;
    o.open(yol).map_err(|e| hata!("{} yazılamadı: {e}", yol.display()))
}

fn dizin_ac(yol: &Path) -> Sonuc<()> {
    fs::create_dir_all(yol).map_err(|e| hata!("{} açılamadı: {e}", yol.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(yol, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// `.tar.zst` akışını `hedef` (henüz var olmayan) dizinine açar ve MANIFEST'i `agac_ozeti` ile doğrular.
/// Hata olursa `hedef` silinir.
pub fn ac(kaynak: impl Read, hedef: &Path, agac_ozeti: &str, mimari: &str) -> Sonuc<()> {
    fs::create_dir(hedef).map_err(|e| hata!("{} açılamadı: {e}", hedef.display()))?;
    let s = ac_ic(kaynak, hedef, agac_ozeti, mimari);
    if s.is_err() {
        let _ = fs::remove_dir_all(hedef);
    }
    s
}

fn ac_ic(kaynak: impl Read, hedef: &Path, agac_ozeti: &str, mimari: &str) -> Sonuc<()> {
    let dec = ruzstd::decoding::StreamingDecoder::new(kaynak).map_err(|e| hata!("zstd: {e}"))?;
    let mut ar = tar::Archive::new(Sinirli { ic: dec, kalan: ACIK_AZAMI });
    let mut gorulen = HashSet::new();
    let mut dosyalar: BTreeMap<String, String> = BTreeMap::new();
    let mut manifest: Option<Vec<u8>> = None;
    for g in ar.entries().map_err(|e| hata!("tar: {e}"))? {
        let mut g = g.map_err(|e| hata!("tar: {e}"))?;
        let tur = g.header().entry_type();
        // path_bytes: PAX ve GNU uzun adları dahil asıl ad
        let ham = g.path_bytes().into_owned();
        let ad = String::from_utf8(ham).map_err(|_| hata!("paket: UTF-8 olmayan ad"))?;
        if gorulen.len() >= DOSYA_SAYI_AZAMI {
            return Err(hata!("paket: çok fazla girdi"));
        }
        if tur.is_dir() {
            let y = ad.strip_suffix('/').unwrap_or(&ad);
            if !izinli_dizin(y) || !gorulen.insert(y.to_string()) {
                return Err(hata!("paket: izin verilmeyen ya da yinelenen dizin {y:?}"));
            }
            dizin_ac(&hedef.join(y))?;
        } else if tur.is_file() {
            if !gorulen.insert(ad.clone()) {
                return Err(hata!("paket: yinelenen girdi {ad:?}"));
            }
            if ad == MANIFEST {
                let mut v = Vec::new();
                (&mut g).take(MANIFEST_AZAMI + 1).read_to_end(&mut v).map_err(|e| hata!("paket: {e}"))?;
                if v.len() as u64 > MANIFEST_AZAMI {
                    return Err(hata!("paket: MANIFEST çok büyük"));
                }
                manifest = Some(v);
                continue;
            }
            if !izinli_dosya(&ad) {
                return Err(hata!("paket: izin verilmeyen yol {ad:?}"));
            }
            let yol = hedef.join(&ad);
            if let Some(ust) = yol.parent() {
                dizin_ac(ust)?;
            }
            let mut f = yeni_dosya(&yol, ad == "uyap_app")?;
            let mut h = Sha256::new();
            let mut tampon = vec![0u8; 64 * 1024];
            loop {
                let n = g.read(&mut tampon).map_err(|e| hata!("paket: {e}"))?;
                if n == 0 {
                    break;
                }
                h.update(&tampon[..n]);
                f.write_all(&tampon[..n])?;
            }
            f.sync_all()?;
            dosyalar.insert(ad, hex::encode(h.finalize()));
        } else {
            return Err(hata!("paket: izin verilmeyen girdi türü ({:?}) {ad:?}", tur));
        }
    }
    let manifest = manifest.ok_or_else(|| hata!("paket: MANIFEST yok"))?;
    if ozet_hex(&manifest) != agac_ozeti {
        return Err(hata!("paket: MANIFEST özeti imzalı bilgiyle tutmuyor"));
    }
    let beklenen = manifest_coz(&manifest)?;
    if beklenen != dosyalar {
        return Err(hata!("paket: dosyalar MANIFEST ile tutmuyor"));
    }
    let mut bas = [0u8; 20];
    crate::dosya::ac_bagsiz(&hedef.join("uyap_app"))?.read_exact(&mut bas).map_err(|_| hata!("uyap_app çok kısa"))?;
    if !elf_mimari_tutuyor(&bas, mimari) {
        return Err(hata!("uyap_app {mimari} için değil"));
    }
    fs::write(hedef.join(MANIFEST), &manifest)?;
    Ok(())
}

fn topla(kok: &Path, dizin: &Path, sonuc: &mut Vec<(String, PathBuf)>) -> Sonuc<()> {
    let mut girdiler: Vec<_> = fs::read_dir(dizin)?.collect::<Result<_, _>>()?;
    girdiler.sort_by_key(|g| g.file_name());
    for g in girdiler {
        let yol = g.path();
        let m = fs::symlink_metadata(&yol)?;
        let rel = yol
            .strip_prefix(kok)
            .map_err(|_| hata!("yol"))?
            .components()
            .map(|c| c.as_os_str().to_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| hata!("UTF-8 olmayan ad"))?
            .join("/");
        if m.is_dir() {
            topla(kok, &yol, sonuc)?;
        } else if m.is_file() {
            if !izinli_dosya(&rel) {
                return Err(hata!("pakete girmeyecek dosya: {rel} (yalnız uyap_app, SURUM, webapp/, uyap_core/enjekte/)"));
            }
            sonuc.push((rel, yol));
        } else {
            return Err(hata!("düz dosya değil: {rel}"));
        }
    }
    Ok(())
}

/// Yayıncı: `kaynak` dizininden (derle_uyap.sh çıktısı) belirlenimci paket üretir. (paket baytları, agac_ozeti)
pub fn paketle(kaynak: &Path) -> Sonuc<(Vec<u8>, String)> {
    let mut liste = Vec::new();
    topla(kaynak, kaynak, &mut liste)?;
    let mut ozetler = BTreeMap::new();
    let mut icerik = BTreeMap::new();
    for (rel, yol) in liste {
        let b = fs::read(&yol)?;
        ozetler.insert(rel.clone(), ozet_hex(&b));
        icerik.insert(rel, b);
    }
    if !ozetler.contains_key("uyap_app") {
        return Err(hata!("kaynakta uyap_app yok"));
    }
    let manifest = manifest_yaz(&ozetler);
    let agac_ozeti = ozet_hex(&manifest);
    let mut b = tar::Builder::new(Vec::new());
    let mut ekle = |ad: &str, veri: &[u8], kip: u32| -> Sonuc<()> {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Regular);
        h.set_size(veri.len() as u64);
        h.set_mode(kip);
        h.set_mtime(0);
        h.set_uid(0);
        h.set_gid(0);
        b.append_data(&mut h, ad, veri).map_err(|e| hata!("tar: {e}"))
    };
    ekle(MANIFEST, &manifest, 0o644)?;
    for (rel, veri) in &icerik {
        ekle(rel, veri, if rel == "uyap_app" { 0o755 } else { 0o644 })?;
    }
    let tar_bayt = b.into_inner().map_err(|e| hata!("tar: {e}"))?;
    let zst = ruzstd::encoding::compress_to_vec(&tar_bayt[..], ruzstd::encoding::CompressionLevel::Fastest);
    Ok((zst, agac_ozeti))
}
