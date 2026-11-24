//! İmza günlüğü (kullanıcı kararı 23.11: "İmza günlüğü açalım").
//!
//! Her imza isteği (başarılı, PIN hatalı, hata) bir satır JSON olarak eklenir: zaman, isteyen süreç (uid, pid,
//! program), belge adı ve özeti, imzalı dosyanın özeti, sertifika (ad, seri), sonuç. **Belgenin kendisi ve PIN
//! yazılmaz.** Kayıtlar zincirlidir: her kayıt bir öncekinin özetini taşır ve kendi özeti bütün alanlarından
//! hesaplanır; aradan kayıt silinir, değiştirilir ya da sıra bozulursa `dogrula` kırılan yeri gösterir. (Zincir
//! sondan kesilmeye karşı tek başına koruma değildir; son özet ayrıca kaydedilebilir.)
//! Claude, 2026-11-23.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::udf_imza::onaltilik;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Kayit {
    pub sira: u64,
    /// UTC, ISO 8601
    pub zaman: String,
    /// "imzala" / "dogrula" ...
    pub islem: String,
    pub uid: Option<u32>,
    pub pid: Option<u32>,
    /// İsteyen sürecin systemd birimi (/proc/<pid>/cgroup; ör. armonia-yazi@….service)
    pub program: Option<String>,
    pub belge_adi: Option<String>,
    /// content.xml SHA-256
    pub belge_ozeti: Option<String>,
    /// İmzalı UDF'in SHA-256'sı
    pub imzali_ozet: Option<String>,
    pub imzalayan: Option<String>,
    pub sertifika_seri: Option<String>,
    /// "basarili" / "pin_hatali" / "kart_yok" / "hata: ..."
    pub sonuc: String,
    /// Bir önceki kaydın özeti (ilk kayıtta 64 sıfır)
    pub onceki: String,
    /// Bu kaydın özeti (yukarıdaki bütün alanlardan)
    pub ozet: String,
}

const SIFIR: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn kayit_ozeti(k: &Kayit) -> String {
    let mut c = k.clone();
    c.ozet = String::new();
    let j = serde_json::to_string(&c).unwrap_or_default();
    onaltilik(&Sha256::digest(j.as_bytes()))
}

pub struct Gunluk {
    yol: PathBuf,
}

impl Gunluk {
    pub fn yeni(yol: impl AsRef<Path>) -> Self {
        Gunluk { yol: yol.as_ref().to_path_buf() }
    }

    fn son(&self) -> Result<Option<Kayit>, String> {
        let Ok(f) = std::fs::File::open(&self.yol) else { return Ok(None) };
        let mut son = None;
        for satir in BufReader::new(f).lines() {
            let satir = satir.map_err(|e| e.to_string())?;
            if !satir.trim().is_empty() {
                son = Some(serde_json::from_str::<Kayit>(&satir).map_err(|e| format!("günlük bozuk: {e}"))?);
            }
        }
        Ok(son)
    }

    /// Kaydı zincire bağlayıp ekler (sıra, önceki, özet burada doldurulur); diske yazıldığından emin olunur.
    pub fn ekle(&self, mut k: Kayit) -> Result<Kayit, String> {
        let son = self.son()?;
        k.sira = son.as_ref().map_or(1, |s| s.sira + 1);
        k.onceki = son.map_or_else(|| SIFIR.to_string(), |s| s.ozet);
        k.ozet = kayit_ozeti(&k);
        let mut secenek = OpenOptions::new();
        secenek.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            secenek.mode(0o600);
        }
        let mut f = secenek.open(&self.yol).map_err(|e| format!("günlük açılamadı: {e}"))?;
        let mut satir = serde_json::to_string(&k).map_err(|e| e.to_string())?;
        satir.push('\n');
        f.write_all(satir.as_bytes()).map_err(|e| format!("günlüğe yazılamadı: {e}"))?;
        f.sync_all().map_err(|e| format!("günlük diske yazılamadı: {e}"))?;
        Ok(k)
    }

    /// Zinciri baştan sona denetler. Dönüş: kayıt sayısı ve son özet; bozuksa kırılan kaydın sırası ve nedeni.
    pub fn dogrula(&self) -> Result<(u64, String), String> {
        let Ok(f) = std::fs::File::open(&self.yol) else { return Ok((0, SIFIR.to_string())) };
        let mut onceki = SIFIR.to_string();
        let mut beklenen = 1u64;
        for (i, satir) in BufReader::new(f).lines().enumerate() {
            let satir = satir.map_err(|e| e.to_string())?;
            if satir.trim().is_empty() {
                continue;
            }
            let k: Kayit = serde_json::from_str(&satir).map_err(|e| format!("{}. satır okunamadı: {e}", i + 1))?;
            if k.sira != beklenen {
                return Err(format!("{}. satır: sıra {} (beklenen {beklenen}) — kayıt silinmiş ya da eklenmiş", i + 1, k.sira));
            }
            if k.onceki != onceki {
                return Err(format!("kayıt {}: önceki kaydın özeti tutmuyor — araya girilmiş ya da önceki kayıt değiştirilmiş", k.sira));
            }
            if kayit_ozeti(&k) != k.ozet {
                return Err(format!("kayıt {}: içerik değiştirilmiş", k.sira));
            }
            onceki = k.ozet.clone();
            beklenen += 1;
        }
        Ok((beklenen - 1, onceki))
    }
}
