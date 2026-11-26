//! Büro antetleri: Ayarlar → "Antetlerim" ve Armonia Yazı "Antet" menüsü. Büro genelinde ortak; en çok biri
//! varsayılan (yeni belgeler onunla açılır). Resim, UYAP'ın üst bilgideki `<image imageData=..>` biçiminde
//! saklanır: PNG, base64; genişlik/yükseklik pt. Resmin boyutlandırılması (PNG'ye çevirme, yazı alanına sığdırma)
//! tarayıcıda yapılır; burada yalnız doğrulanır. Uçlar `/__cu__/veri/` altında (icra_api::dagit). 26.11.2026
use base64::Engine;
use postgres::Client;
use serde_json::{json, Value};

/// Çözülmüş PNG en çok bu kadar (UDF'e gömülür; UYAP antetleri ~20 KB).
const EN_COK_BAYT: usize = 3 * 1024 * 1024;
const EN_COK_ANTET: i64 = 50;

/// base64 PNG'yi doğrular, boşluksuz hâlini ve piksel boyutunu döndürür.
pub fn png_dogrula(b64: &str) -> Result<(String, u32, u32), String> {
    let temiz: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let temiz = temiz.strip_prefix("data:image/png;base64,").unwrap_or(&temiz).to_string();
    let bayt = base64::engine::general_purpose::STANDARD
        .decode(temiz.as_bytes())
        .map_err(|_| "Resim verisi bozuk (base64 değil).".to_string())?;
    if bayt.len() > EN_COK_BAYT {
        return Err("Resim çok büyük (en çok 3 MB).".into());
    }
    if bayt.len() < 24 || &bayt[..8] != b"\x89PNG\r\n\x1a\n" || &bayt[12..16] != b"IHDR" {
        return Err("Resim PNG olmalı.".into());
    }
    let w = u32::from_be_bytes([bayt[16], bayt[17], bayt[18], bayt[19]]);
    let h = u32::from_be_bytes([bayt[20], bayt[21], bayt[22], bayt[23]]);
    if w == 0 || h == 0 || w > 10_000 || h > 10_000 {
        return Err("Resim boyutu geçersiz.".into());
    }
    Ok((temiz, w, h))
}

fn olcu(x: f64, ad: &str) -> Result<f64, String> {
    if x.is_finite() && x > 0.0 && x <= 2000.0 {
        Ok((x * 100.0).round() / 100.0)
    } else {
        Err(format!("Antet {ad} geçersiz."))
    }
}

fn satir(r: &postgres::Row) -> Value {
    json!({"id": r.get::<_, i64>(0), "ad": r.get::<_, String>(1), "resim": r.get::<_, String>(2),
           "genislik": r.get::<_, f64>(3), "yukseklik": r.get::<_, f64>(4), "varsayilan": r.get::<_, bool>(5)})
}

const ALANLAR: &str = "id, ad, resim, genislik, yukseklik, varsayilan";

/// Antetler: varsayılan önce, sonra eklenme sırası.
pub fn liste(c: &mut Client) -> Result<Value, String> {
    let satirlar = c
        .query(&format!("SELECT {ALANLAR} FROM antet ORDER BY varsayilan DESC, id"), &[])
        .map_err(|e| format!("Antetler okunamadı: {e}"))?;
    Ok(Value::Array(satirlar.iter().map(satir).collect()))
}

/// Varsayılan antet; yoksa null.
pub fn varsayilan(c: &mut Client) -> Result<Value, String> {
    let r = c
        .query_opt(&format!("SELECT {ALANLAR} FROM antet WHERE varsayilan"), &[])
        .map_err(|e| format!("Varsayılan antet okunamadı: {e}"))?;
    Ok(r.as_ref().map(satir).unwrap_or(Value::Null))
}

pub fn ekle(c: &mut Client, ad: &str, resim: &str, genislik: f64, yukseklik: f64, varsayilan: bool) -> Result<Value, String> {
    let (resim, _, _) = png_dogrula(resim)?;
    let (genislik, yukseklik) = (olcu(genislik, "genişliği")?, olcu(yukseklik, "yüksekliği")?);
    let ad: String = ad.trim().chars().take(120).collect();
    let ad = if ad.is_empty() { "Antet".to_string() } else { ad };
    let mut tx = c.transaction().map_err(|e| e.to_string())?;
    let sayi: i64 = tx.query_one("SELECT count(*) FROM antet", &[]).map_err(|e| e.to_string())?.get(0);
    if sayi >= EN_COK_ANTET {
        return Err(format!("En çok {EN_COK_ANTET} antet kaydedilebilir; önce birini silin."));
    }
    if varsayilan {
        tx.execute("UPDATE antet SET varsayilan = false WHERE varsayilan", &[]).map_err(|e| e.to_string())?;
    }
    let r = tx
        .query_one(
            &format!("INSERT INTO antet (ad, resim, genislik, yukseklik, varsayilan) VALUES ($1, $2, $3, $4, $5) RETURNING {ALANLAR}"),
            &[&ad, &resim, &genislik, &yukseklik, &varsayilan],
        )
        .map_err(|e| format!("Antet kaydedilemedi: {e}"))?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(satir(&r))
}

pub fn sil(c: &mut Client, id: i64) -> Result<(), String> {
    c.execute("DELETE FROM antet WHERE id = $1", &[&id]).map_err(|e| e.to_string())?;
    Ok(())
}

/// `id` varsayılan olur; `None` varsayılanı kaldırır (yeni belgeler antetsiz açılır).
pub fn varsayilan_yap(c: &mut Client, id: Option<i64>) -> Result<(), String> {
    let mut tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE antet SET varsayilan = false WHERE varsayilan", &[]).map_err(|e| e.to_string())?;
    if let Some(id) = id {
        let n = tx.execute("UPDATE antet SET varsayilan = true WHERE id = $1", &[&id]).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Antet bulunamadı.".into());
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn ad_ata(c: &mut Client, id: i64, ad: &str) -> Result<(), String> {
    let ad: String = ad.trim().chars().take(120).collect();
    if ad.is_empty() {
        return Err("Antet adı boş olamaz.".into());
    }
    c.execute("UPDATE antet SET ad = $2 WHERE id = $1", &[&id, &ad]).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
const ORNEK_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR4nGNgYGD4z8DAwMAAAA0BAf8VxHoAAAAASUVORK5CYII=";

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut Client) {
    assert!(varsayilan(c).unwrap().is_null());
    let a = ekle(c, "Büro", ORNEK_PNG, 453.0, 59.0, false).unwrap();
    assert_eq!(a["varsayilan"], false);
    let b = ekle(c, "", ORNEK_PNG, 200.0, 40.0, true).unwrap();
    assert_eq!(b["ad"], "Antet");
    assert_eq!(varsayilan(c).unwrap()["id"], b["id"]);
    // Başkası varsayılan yapılınca önceki düşer; kaldırılınca hiç varsayılan kalmaz.
    varsayilan_yap(c, a["id"].as_i64()).unwrap();
    assert_eq!(varsayilan(c).unwrap()["id"], a["id"]);
    varsayilan_yap(c, None).unwrap();
    assert!(varsayilan(c).unwrap().is_null());
    ad_ata(c, a["id"].as_i64().unwrap(), "Tireli Savaş").unwrap();
    assert_eq!(liste(c).unwrap()[0]["ad"], "Tireli Savaş");
    assert!(ekle(c, "x", "aGVsbG8=", 10.0, 10.0, false).unwrap_err().contains("PNG"));
    assert!(ekle(c, "x", ORNEK_PNG, 0.0, 10.0, false).unwrap_err().contains("genişliği"));
    for x in [&a, &b] {
        sil(c, x["id"].as_i64().unwrap()).unwrap();
    }
    assert!(liste(c).unwrap().as_array().unwrap().is_empty());
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn png_dogrulama() {
        let (t, w, h) = png_dogrula(&format!("data:image/png;base64,{ORNEK_PNG}")).unwrap();
        assert_eq!((t.as_str(), w, h), (ORNEK_PNG, 2, 1));
        assert!(png_dogrula("!!!").is_err());
        assert!(png_dogrula("aGVsbG8gZMO8bnlh").unwrap_err().contains("PNG"));
    }
}
