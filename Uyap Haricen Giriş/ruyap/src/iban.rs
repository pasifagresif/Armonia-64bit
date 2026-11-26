//! IBAN kayıtları: vekil IBAN'ları (büro genelinde, birden çok) ve dosya başına alacaklı IBAN'ı.
//! "Borçlu Tarafından Yapılan Ödemelerin Hesaba Aktarılması" talebi (evrak listesinde tahsilat makbuzuna sağ tık →
//! "Parayı iste") ve Ayarlar penceresi kullanır. Uçlar `/__cu__/veri/` altında (icra_api::dagit).
use postgres::Client;
use serde_json::{json, Value};

/// Boşlukları atar, büyük harfe çevirir ve Türk IBAN'ı olarak doğrular ("TR" + 24 hane, mod-97 kontrolü).
pub fn duzelt(ham: &str) -> Result<String, String> {
    let s: String = ham.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_uppercase();
    let s = if s.len() == 24 && s.chars().all(|c| c.is_ascii_digit()) { format!("TR{s}") } else { s };
    if !s.starts_with("TR") || s.len() != 26 || !s[2..].chars().all(|c| c.is_ascii_digit()) {
        return Err("IBAN \"TR\" ile başlayan 26 karakter olmalı (TR + 24 rakam).".into());
    }
    if !mod97_gecerli(&s) {
        return Err("IBAN kontrol hanesi tutmuyor; yazımı kontrol edin.".into());
    }
    Ok(s)
}

fn mod97_gecerli(iban: &str) -> bool {
    let duzen = format!("{}{}", &iban[4..], &iban[..4]);
    let mut kalan: u32 = 0;
    for c in duzen.chars() {
        let parca = if c.is_ascii_digit() {
            c.to_digit(10).map(|d| d.to_string())
        } else if c.is_ascii_uppercase() {
            Some((c as u32 - 'A' as u32 + 10).to_string())
        } else {
            None
        };
        let Some(p) = parca else { return false };
        for h in p.chars() {
            kalan = (kalan * 10 + h.to_digit(10).unwrap_or(0)) % 97;
        }
    }
    kalan == 1
}

/// Gösterim için dörtlü gruplar: "TR12 3456 …".
pub fn gruplu(iban: &str) -> String {
    iban.chars().collect::<Vec<_>>().chunks(4).map(|p| p.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

fn satir(id: i64, iban: &str, etiket: &str, varsayilan: bool) -> Value {
    json!({"id": id, "iban": iban, "gruplu": gruplu(iban), "etiket": etiket, "varsayilan": varsayilan})
}

/// Vekil IBAN'ları: varsayılan önce, sonra eklenme sırası.
pub fn liste(c: &mut Client) -> Result<Value, String> {
    let satirlar = c
        .query("SELECT id, iban, etiket, varsayilan FROM avukat_iban ORDER BY varsayilan DESC, id", &[])
        .map_err(|e| format!("IBAN listesi okunamadı: {e}"))?;
    Ok(Value::Array(satirlar.iter().map(|r| satir(r.get(0), r.get::<_, String>(1).as_str(), r.get::<_, String>(2).as_str(), r.get(3))).collect()))
}

/// Vekil IBAN'ı ekler (zaten varsa etiketini günceller). İlk kayıt kendiliğinden varsayılan olur.
pub fn ekle(c: &mut Client, ham: &str, etiket: &str, varsayilan: bool) -> Result<Value, String> {
    let iban = duzelt(ham)?;
    let etiket: String = etiket.trim().chars().take(120).collect();
    let mut tx = c.transaction().map_err(|e| e.to_string())?;
    let bos: i64 = tx.query_one("SELECT count(*) FROM avukat_iban", &[]).map_err(|e| e.to_string())?.get(0);
    let varsayilan = varsayilan || bos == 0;
    if varsayilan {
        tx.execute("UPDATE avukat_iban SET varsayilan = false WHERE varsayilan", &[]).map_err(|e| e.to_string())?;
    }
    let r = tx
        .query_one(
            "INSERT INTO avukat_iban (iban, etiket, varsayilan) VALUES ($1, $2, $3)
             ON CONFLICT (iban) DO UPDATE SET etiket = CASE WHEN EXCLUDED.etiket <> '' THEN EXCLUDED.etiket ELSE avukat_iban.etiket END,
                                              varsayilan = avukat_iban.varsayilan OR EXCLUDED.varsayilan
             RETURNING id, iban, etiket, varsayilan",
            &[&iban, &etiket, &varsayilan],
        )
        .map_err(|e| format!("IBAN kaydedilemedi: {e}"))?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(satir(r.get(0), r.get::<_, String>(1).as_str(), r.get::<_, String>(2).as_str(), r.get(3)))
}

/// Siler; silinen varsayılansa en eski kayıt varsayılan olur.
pub fn sil(c: &mut Client, id: i64) -> Result<(), String> {
    let mut tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM avukat_iban WHERE id = $1", &[&id]).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE avukat_iban SET varsayilan = true
         WHERE id = (SELECT min(id) FROM avukat_iban) AND NOT EXISTS (SELECT 1 FROM avukat_iban WHERE varsayilan)",
        &[],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn varsayilan_yap(c: &mut Client, id: i64) -> Result<(), String> {
    let mut tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE avukat_iban SET varsayilan = false WHERE varsayilan", &[]).map_err(|e| e.to_string())?;
    let n = tx.execute("UPDATE avukat_iban SET varsayilan = true WHERE id = $1", &[&id]).map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("IBAN bulunamadı.".into());
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn etiket_ata(c: &mut Client, id: i64, etiket: &str) -> Result<(), String> {
    let etiket: String = etiket.trim().chars().take(120).collect();
    c.execute("UPDATE avukat_iban SET etiket = $2 WHERE id = $1", &[&id, &etiket]).map_err(|e| e.to_string())?;
    Ok(())
}

fn kunye(birim_id: &str, dosya_no: &str) -> Result<(String, String), String> {
    let (b, d) = (birim_id.trim().to_string(), dosya_no.trim().to_string());
    if b.is_empty() || d.is_empty() {
        return Err("Dosya künyesi (birim, dosya no) eksik.".into());
    }
    Ok((b, d))
}

/// Dosyada kayıtlı alacaklı IBAN'ı; yoksa null.
pub fn alacakli_oku(c: &mut Client, birim_id: &str, dosya_no: &str) -> Result<Value, String> {
    let (b, d) = kunye(birim_id, dosya_no)?;
    let r = c
        .query_opt("SELECT iban FROM dosya_alacakli_iban WHERE birim_id = $1 AND dosya_no = $2", &[&b, &d])
        .map_err(|e| format!("Alacaklı IBAN'ı okunamadı: {e}"))?;
    Ok(match r {
        Some(r) => {
            let iban: String = r.get(0);
            json!({"iban": iban, "gruplu": gruplu(&iban)})
        }
        None => Value::Null,
    })
}

pub fn alacakli_kaydet(c: &mut Client, birim_id: &str, dosya_no: &str, birim_adi: &str, ham: &str) -> Result<Value, String> {
    let (b, d) = kunye(birim_id, dosya_no)?;
    let iban = duzelt(ham)?;
    let birim_adi: String = birim_adi.trim().chars().take(255).collect();
    c.execute(
        "INSERT INTO dosya_alacakli_iban (birim_id, dosya_no, iban, birim_adi) VALUES ($1, $2, $3, $4)
         ON CONFLICT (birim_id, dosya_no) DO UPDATE SET iban = EXCLUDED.iban, guncelleme = now(),
             birim_adi = CASE WHEN EXCLUDED.birim_adi <> '' THEN EXCLUDED.birim_adi ELSE dosya_alacakli_iban.birim_adi END",
        &[&b, &d, &iban, &birim_adi],
    )
    .map_err(|e| format!("Alacaklı IBAN'ı kaydedilemedi: {e}"))?;
    Ok(json!({"iban": iban, "gruplu": gruplu(&iban)}))
}

/// Dosyalarda kayıtlı bütün alacaklı IBAN'ları (Ayarlar sayfası), son güncellenen önce.
pub fn alacakli_liste(c: &mut Client) -> Result<Value, String> {
    let satirlar = c
        .query(
            "SELECT birim_id, dosya_no, iban, birim_adi, to_char(guncelleme AT TIME ZONE 'Europe/Istanbul', 'DD.MM.YYYY HH24:MI')
             FROM dosya_alacakli_iban ORDER BY guncelleme DESC LIMIT 5000",
            &[],
        )
        .map_err(|e| format!("Alacaklı IBAN'ları okunamadı: {e}"))?;
    Ok(Value::Array(
        satirlar
            .iter()
            .map(|r| {
                let iban: String = r.get(2);
                json!({"birim_id": r.get::<_, String>(0), "dosya_no": r.get::<_, String>(1), "iban": iban,
                       "gruplu": gruplu(&iban), "birim_adi": r.get::<_, String>(3), "guncelleme": r.get::<_, Option<String>>(4)})
            })
            .collect(),
    ))
}

pub fn alacakli_sil(c: &mut Client, birim_id: &str, dosya_no: &str) -> Result<(), String> {
    let (b, d) = kunye(birim_id, dosya_no)?;
    c.execute("DELETE FROM dosya_alacakli_iban WHERE birim_id = $1 AND dosya_no = $2", &[&b, &d]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut Client) {
    const A: &str = "TR330006100519786457841326";
    // İlk kayıt kendiliğinden varsayılan; ikinci (boşluklu yazılmış) varsayılan istenince ilki düşer.
    let ilk = ekle(c, "tr33 0006 1005 1978 6457 8413 26", "Ziraat", false).unwrap();
    assert_eq!((ilk["iban"].as_str(), ilk["varsayilan"].as_bool()), (Some(A), Some(true)));
    assert!(ekle(c, "TR330006100519786457841327", "", false).unwrap_err().contains("kontrol hanesi"));
    // Aynı IBAN yeniden eklenirse çoğalmaz; boş etiket eskisini ezmez.
    ekle(c, A, "", false).unwrap();
    let l = liste(c).unwrap();
    assert_eq!(l.as_array().unwrap().len(), 1);
    assert_eq!(l[0]["etiket"], "Ziraat");
    let id = ilk["id"].as_i64().unwrap();
    etiket_ata(c, id, "Ziraat — büro").unwrap();
    assert_eq!(liste(c).unwrap()[0]["etiket"], "Ziraat — büro");
    sil(c, id).unwrap();
    assert!(liste(c).unwrap().as_array().unwrap().is_empty());
    // Alacaklı IBAN'ı dosya başına: yaz, oku, üzerine yaz, listele, sil.
    assert!(alacakli_oku(c, "1001", "2026/5").unwrap().is_null());
    alacakli_kaydet(c, "1001", "2026/5", "İzmir 1. İcra Dairesi", A).unwrap();
    assert_eq!(alacakli_oku(c, "1001", "2026/5").unwrap()["iban"], A);
    assert!(alacakli_kaydet(c, "", "2026/5", "", A).unwrap_err().contains("künye"));
    assert_eq!(alacakli_liste(c).unwrap().as_array().unwrap().len(), 1);
    alacakli_sil(c, "1001", "2026/5").unwrap();
    assert!(alacakli_oku(c, "1001", "2026/5").unwrap().is_null());
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn gecerli_iban() {
        // Örnek (yayımlanmış test IBAN'ı): TR33 0006 1005 1978 6457 8413 26
        assert_eq!(duzelt("tr33 0006 1005 1978 6457 8413 26").unwrap(), "TR330006100519786457841326");
        assert_eq!(duzelt("330006100519786457841326").unwrap(), "TR330006100519786457841326");
    }

    #[test]
    fn gecersiz_iban() {
        assert!(duzelt("TR330006100519786457841327").is_err()); // kontrol hanesi
        assert!(duzelt("TR3300061005").is_err()); // kısa
        assert!(duzelt("DE89370400440532013000").is_err()); // Türk değil
    }

    #[test]
    fn gruplama() {
        assert_eq!(gruplu("TR330006100519786457841326"), "TR33 0006 1005 1978 6457 8413 26");
    }
}
