//! Programın sürümü: tek kaynak (2026-11-23).
//!
//! Yayın derlemesinde sürüm `CU_SURUM` ortam değişkeniyle verilir (ör. `26.11.23.2`: yıl.ay.gün.o günün kaçıncı
//! derlemesi). `build.rs` aynı değeri exe'nin sürüm bilgisine, `msi_hazirla` MSI'ın sürümüne yazar; güncelleme
//! denetimi de bu değerle karşılaştırır. `CU_SURUM` verilmeden yapılan derleme "geliştirme derlemesi"dir: sürümü
//! Cargo.toml'dakidir ve güncelleme denetimi (test sunucusu verilmedikçe) çalışmaz; yerel derlemeler kendini
//! yayındaki sürümle ezmeye kalkmasın.

/// Bu derlemenin sürümü.
pub const SURUM: &str = env!("CU_SURUM");

/// `CU_SURUM` verilmeden derlendiyse true.
pub const GELISTIRME: bool = option_env!("CU_SURUM_YAYIN").is_none();

/// "26.11.23.2" → [26, 11, 23, 2]. Eksik parçalar 0; sayı olmayan parça varsa None.
pub fn parcalar(s: &str) -> Option<[u32; 4]> {
    let mut p = [0u32; 4];
    let mut n = 0;
    for parca in s.trim().split('.') {
        if n == 4 || parca.is_empty() {
            return None;
        }
        p[n] = parca.parse().ok()?;
        n += 1;
    }
    if n < 3 {
        return None;
    }
    Some(p)
}

/// `aday`, `kurulu`dan yeni mi? Çözülemeyen sürüm hiçbir zaman yeni sayılmaz.
pub fn daha_yeni(aday: &str, kurulu: &str) -> bool {
    match (parcalar(aday), parcalar(kurulu)) {
        (Some(a), Some(k)) => a > k,
        _ => false,
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn karsilastirma() {
        assert!(daha_yeni("26.11.23.2", "26.11.23.1"));
        assert!(daha_yeni("26.11.24", "26.11.23.9"));
        assert!(daha_yeni("26.12.1", "26.11.30.3"));
        assert!(daha_yeni("27.1.1", "26.12.31.9"));
        assert!(!daha_yeni("26.11.23.1", "26.11.23.1"));
        assert!(!daha_yeni("26.11.22.9", "26.11.23"));
        assert!(!daha_yeni("bozuk", "26.11.23"));
        assert!(!daha_yeni("26.11", "26.1.1"));
        assert!(!daha_yeni("26.11.23.1.5", "26.1.1"));
        assert!(parcalar(SURUM).is_some(), "derleme sürümü çözülemiyor: {SURUM}");
    }
}
