//! Kurulum (armonia-guncelle-kur.service; kullanıcı `armonia-guncelle`, ağsız).
//!
//! Yerleşim (kök: /var/lib/armonia-guncelle):
//!   gelen/                 indiricinin yazdığı paket (GÜVENİLMEZ; kurucu yalnız okur)
//!   cokluuyap/<sürüm>/     açılmış, doğrulanmış ağaç + guncelleme.json{,.sig}
//!   cokluuyap/etkin        tek satır: etkin sürüm (atomik yazılır; sembolik bağ değil)
//!   durum/en_yuksek        tek yönlü sayaç: bundan eski ya da eşit sürüm kurulmaz
//!   durum/iptal            yedek anahtarla imzalı iptal bildirimleri (JSON dizi)

use crate::anahtar::{Anahtarlar, IptalKaydi};
use crate::bilgi::{self, Baglam};
use crate::surum::Surum;
use crate::{agac, dosya, hata, zaman, Sonuc};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Yerlesim {
    pub kok: PathBuf,
}

impl Yerlesim {
    pub fn new(kok: impl Into<PathBuf>) -> Self {
        Yerlesim { kok: kok.into() }
    }
    pub fn surumler(&self) -> PathBuf {
        self.kok.join("cokluuyap")
    }
    pub fn etkin(&self) -> PathBuf {
        self.surumler().join("etkin")
    }
    pub fn surum_dizini(&self, s: Surum) -> PathBuf {
        self.surumler().join(s.to_string())
    }
    pub fn durum(&self) -> PathBuf {
        self.kok.join("durum")
    }
    pub fn en_yuksek(&self) -> PathBuf {
        self.durum().join("en_yuksek")
    }
    pub fn iptal(&self) -> PathBuf {
        self.durum().join("iptal")
    }
    pub fn gelen(&self) -> PathBuf {
        self.kok.join("gelen")
    }
}

/// İmajdaki Çoklu UYAP hakkında bilinenler (imajın parçası, root'a ait dosyalardan).
#[derive(Clone, Debug)]
pub struct Imaj {
    pub surum: Surum,
    pub tarih: i64,
    pub arayuz: u32,
}

impl Imaj {
    /// `surum_dosyasi`: /opt/armonia/cokluuyap/SURUM (surum=, derleme=); `ayar`: /etc/armonia/guncelle/ayar
    /// (imaj_arayuzu=).
    pub fn oku(surum_dosyasi: &Path, ayar: &Path) -> Sonuc<Imaj> {
        let s = String::from_utf8(dosya::oku_sinirli(surum_dosyasi, 4096)?).map_err(|_| hata!("SURUM UTF-8 değil"))?;
        let a = String::from_utf8(dosya::oku_sinirli(ayar, 4096)?).map_err(|_| hata!("ayar UTF-8 değil"))?;
        let al = |m: &str, k: &str| m.lines().find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()));
        let surum = al(&s, "surum=").and_then(|v| Surum::coz(&v)).ok_or_else(|| hata!("imaj SURUM: surum= yok/geçersiz"))?;
        let tarih = al(&s, "derleme=").and_then(|v| zaman::coz(&v)).ok_or_else(|| hata!("imaj SURUM: derleme= yok/geçersiz"))?;
        let arayuz = al(&a, "imaj_arayuzu=").and_then(|v| v.parse().ok()).ok_or_else(|| hata!("ayar: imaj_arayuzu= yok"))?;
        Ok(Imaj { surum, tarih, arayuz })
    }
}

pub fn surum_dosyasi_oku(yol: &Path) -> Sonuc<Option<Surum>> {
    match dosya::oku_varsa(yol, 32)? {
        None => Ok(None),
        Some(b) => {
            let m = std::str::from_utf8(&b).map_err(|_| hata!("{} bozuk", yol.display()))?;
            let s = m.strip_suffix('\n').and_then(Surum::coz).ok_or_else(|| hata!("{} bozuk", yol.display()))?;
            Ok(Some(s))
        }
    }
}

/// Aynı anda tek kurulum (indirici ile de Conflicts= var; bu ikinci kilit).
struct Kilit(#[allow(dead_code)] fs::File);

fn kilitle(y: &Yerlesim) -> Sonuc<Kilit> {
    let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(y.durum().join("kilit"))?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // SAFETY: geçerli dosya tanımlayıcısı; flock yalnız kilit alır
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(hata!("başka bir kurulum sürüyor"));
        }
    }
    Ok(Kilit(f))
}

/// Bir açılışta diskte birikebilecek en çok sürüm sayısı (etkin + bu açılışta kurulanlar)
pub const KURULU_AZAMI: usize = 3;

/// İndirici ile kurucunun aynı anda çalışmaması için `gelen/.kilit` üzerinde flock (Conflicts= iki yönlüdür ve
/// süren kurulumu öldürürdü; denetçi 24.11 D-3). Kurucu dosyayı salt okunur açar (flock salt okunur tanımlayıcıda
/// da çalışır); dosya yoksa (indirici hiç çalışmamış) kilit gerekmez.
pub fn gelen_kilidi(y: &Yerlesim, olustur: bool) -> Sonuc<Option<fs::File>> {
    let yol = y.gelen().join(".kilit");
    let f = if olustur {
        fs::OpenOptions::new().create(true).truncate(false).write(true).open(&yol)?
    } else {
        match fs::File::open(&yol) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // SAFETY: geçerli dosya tanımlayıcısı; flock yalnız kilit alır
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(hata!("indirme ya da kurulum sürüyor"));
        }
    }
    Ok(Some(f))
}

pub struct KurGirdisi<'a> {
    pub bilgi: &'a [u8],
    pub imza: &'a [u8],
    pub paket: &'a Path,
    /// unix: paket dosyasının sahibi olması gereken UID (indirici)
    pub sahip_uid: Option<u32>,
}

/// Paketi doğrulayıp kurar ve etkinleştirir. Kurulan sürümü döner.
pub fn kur(y: &Yerlesim, anahtarlar: &Anahtarlar, imaj: &Imaj, mimari: &str, simdi: i64, g: KurGirdisi) -> Sonuc<Surum> {
    fs::create_dir_all(y.surumler())?;
    fs::create_dir_all(y.durum())?;
    let _kilit = kilitle(y)?;
    let en_yuksek = surum_dosyasi_oku(&y.en_yuksek())?.unwrap_or(Surum::SIFIR);
    let taban = imaj.surum.max(en_yuksek);
    let b = Baglam { mimari: mimari.to_string(), imaj_arayuzu: imaj.arayuz, imaj_tarihi: imaj.tarih, simdi, sure_denetimi: true };
    let d = bilgi::dogrula(g.bilgi, g.imza, anahtarlar, &b)?;
    if d.surum <= taban {
        return Err(hata!("{} kurulamaz: kurulu/en yüksek sürüm {taban}", d.surum));
    }
    // Budama yalnız açılışta: bir açılışta en çok KURULU_AZAMI sürüm birikir (disk sınırı)
    if kurulu_surumler(y)?.len() >= KURULU_AZAMI {
        return Err(hata!("bu açılışta yeterince güncelleme kuruldu; önce bilgisayarı yeniden başlatın"));
    }

    // Paket BİR KEZ açılır; doğrulama ve açma yalnız kurucunun kendi kopyasında (denetçi 24.11 O1)
    let mut kaynak = dosya::ac_bagsiz(g.paket)?;
    let m = kaynak.metadata()?;
    if m.len() != d.paket.boyut {
        return Err(hata!("paket boyutu tutmuyor ({} ≠ {})", m.len(), d.paket.boyut));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() != 1 {
            return Err(hata!("pakete birden çok bağ var"));
        }
        if let Some(u) = g.sahip_uid {
            if m.uid() != u {
                return Err(hata!("paketin sahibi beklenen indirici değil"));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = g.sahip_uid;

    let gecici = y.surumler().join(".gecici");
    if fs::symlink_metadata(&gecici).is_ok() {
        fs::remove_dir_all(&gecici)?;
    }
    fs::create_dir(&gecici)?;
    let sonuc = (|| -> Sonuc<Surum> {
        let kopya = gecici.join("paket.tar.zst");
        {
            let mut k = fs::OpenOptions::new().write(true).create_new(true).open(&kopya)?;
            let mut h = Sha256::new();
            let mut tampon = vec![0u8; 64 * 1024];
            let mut toplam = 0u64;
            loop {
                let n = kaynak.read(&mut tampon)?;
                if n == 0 {
                    break;
                }
                toplam += n as u64;
                if toplam > d.paket.boyut {
                    return Err(hata!("paket okunurken büyüdü"));
                }
                h.update(&tampon[..n]);
                k.write_all(&tampon[..n])?;
            }
            k.sync_all()?;
            if toplam != d.paket.boyut || hex::encode(h.finalize()) != d.paket.sha256 {
                return Err(hata!("paket özeti imzalı bilgiyle tutmuyor"));
            }
        }
        let agac_d = gecici.join("agac");
        crate::paket::ac(fs::File::open(&kopya)?, &agac_d, &d.paket.agac_ozeti, mimari)?;
        fs::write(agac_d.join(agac::BILGI_ADI), g.bilgi)?;
        fs::write(agac_d.join(agac::IMZA_ADI), g.imza)?;
        dosya::dizin_esitle(&agac_d);
        let hedef = y.surum_dizini(d.surum);
        if fs::symlink_metadata(&hedef).is_ok() {
            fs::remove_dir_all(&hedef)?;
        }
        fs::rename(&agac_d, &hedef)?;
        dosya::dizin_esitle(&y.surumler());
        Ok(d.surum)
    })();
    let _ = fs::remove_dir_all(&gecici);
    let yeni = sonuc?;

    dosya::atomik_yaz(&y.etkin(), format!("{yeni}\n").as_bytes(), 0o644)?;
    dosya::atomik_yaz(&y.en_yuksek(), format!("{yeni}\n").as_bytes(), 0o644)?;
    // Kurulum hiçbir sürümü SİLMEZ: o an çalışan sürüm (hangisi olduğu kurucuya görünmez) yerinde kalsın.
    // Eskiler açılışta, Çoklu UYAP çalışmazken budanır (buda(); denetçi 24.11 O-4).
    Ok(yeni)
}

/// Kurulu sürüm dizinleri (ad sürüm biçimindeyse)
pub fn kurulu_surumler(y: &Yerlesim) -> Sonuc<Vec<Surum>> {
    let mut v = Vec::new();
    match fs::read_dir(y.surumler()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(v),
        Err(e) => return Err(e.into()),
        Ok(d) => {
            for g in d.flatten() {
                if let Some(s) = g.file_name().to_str().and_then(Surum::coz) {
                    v.push(s);
                }
            }
        }
    }
    v.sort();
    Ok(v)
}

/// Açılışta (armonia-guncelle-buda.service; Çoklu UYAP henüz çalışmıyor): etkin dışındaki sürümleri ve yarım kalmış
/// geçici dizini siler. Silinenleri döner.
pub fn buda(y: &Yerlesim) -> Sonuc<Vec<Surum>> {
    if !y.surumler().exists() {
        return Ok(Vec::new());
    }
    let _kilit = kilitle(y)?;
    let etkin = surum_dosyasi_oku(&y.etkin()).ok().flatten();
    let mut silinen = Vec::new();
    for s in kurulu_surumler(y)? {
        if Some(s) != etkin {
            fs::remove_dir_all(y.surum_dizini(s))?;
            silinen.push(s);
        }
    }
    let gecici = y.surumler().join(".gecici");
    if fs::symlink_metadata(&gecici).is_ok() {
        fs::remove_dir_all(&gecici)?;
    }
    Ok(silinen)
}

/// Yedek anahtarla imzalı iptal bildirimini doğrulayıp kalıcı kayda ekler.
pub fn iptal_ekle(y: &Yerlesim, anahtar_dosyasi: &Path, bildirim: &[u8], imza: &[u8]) -> Sonuc<String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    fs::create_dir_all(y.durum())?;
    let _kilit = kilitle(y)?;
    let mut a = Anahtarlar::yukle(anahtar_dosyasi, Some(&y.iptal()))?;
    let k = IptalKaydi { bildirim: B64.encode(bildirim), imza: String::from_utf8(imza.to_vec()).map_err(|_| hata!("imza UTF-8 değil"))? };
    a.iptal_uygula(&k)?;
    let mut kayit: Vec<IptalKaydi> = match dosya::oku_varsa(&y.iptal(), 64 * 1024)? {
        Some(b) => serde_json::from_slice(&b).map_err(|e| hata!("iptal kaydı bozuk: {e}"))?,
        None => Vec::new(),
    };
    kayit.push(k);
    dosya::atomik_yaz(&y.iptal(), serde_json::to_string_pretty(&kayit).expect("json").as_bytes(), 0o644)?;
    Ok(a.etkin.kimlik())
}
