//! Dosya listeleri (23.11.2026, kullanıcı isteği; şema `0017_dosya_listesi.sql`).
//!
//! Dosya Listesi sekmesinde süzgece uyan (ya da seçili) dosyalar adlı bir listeye eklenir. Bütün dosya sekmelerinde
//! (Dosya Listesi, Kesinleştirme, Süreçler, Tebligatlar) `liste=<id>` süzgeci yalnız o listedeki dosyaları bırakır; sekmenin
//! kendi süzgeçleri bunun içinde çalışır. Toplu taramanın kapsamı sekmenin süzgecinden geldiği için liste dışındaki
//! dosyaya UYAP isteği gitmez. Liste sabittir (eklenen kalır); büro ortaktır.
//!
//! Uçlar (`/__cu__/veri/…`, `icra_api::dagit`):
//!   GET  listeler                       bütün listeler: {id, ad, aciklama, dosya_sayisi, acik_sayisi, guncelleme}
//!   POST liste        {id?, ad, aciklama?} — yeni liste (id yoksa) ya da ad / açıklama değişikliği
//!   POST liste_sil    {id}
//!   POST liste_ekle   {id, dosyalar: [..]} — {eklenen, toplam}
//!   POST liste_cikar  {id, dosyalar: [..]} — {cikan, toplam}

use std::collections::{HashMap, HashSet};

use postgres::GenericClient;
use serde_json::{json, Value};

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

/// Sorgudaki `liste` süzgeci (liste kimliği); yoksa / geçersizse None.
pub fn liste_oku(q: &HashMap<String, String>) -> Option<i64> {
    q.get("liste").and_then(|v| v.trim().parse().ok()).filter(|n: &i64| *n > 0)
}

/// Listedeki dosyalar. Liste yoksa hata (silinmiş listeyle süzülen ekran boş liste değil, uyarı görmeli).
pub fn liste_dosyalari<C: GenericClient>(c: &mut C, liste: i64) -> Result<HashSet<i64>, String> {
    if c.query_opt("SELECT 1 FROM dosya_listesi WHERE id = $1", &[&liste]).map_err(hata)?.is_none() {
        return Err("Seçili dosya listesi bulunamadı (silinmiş olabilir). Liste seçimini kaldırın.".into());
    }
    Ok(c.query("SELECT dosya_id FROM dosya_listesi_uyesi WHERE liste_id = $1", &[&liste])
        .map_err(hata)?
        .iter()
        .map(|s| s.get(0))
        .collect())
}

/// Satırları listeyle süzer (liste seçilmemişse dokunmaz). `dosya`: satırın dosya kimliği.
pub fn suz<C: GenericClient, T>(mut satirlar: Vec<T>, c: &mut C, liste: Option<i64>, dosya: impl Fn(&T) -> i64) -> Result<Vec<T>, String> {
    if let Some(l) = liste {
        let uyeler = liste_dosyalari(c, l)?;
        satirlar.retain(|s| uyeler.contains(&dosya(s)));
    }
    Ok(satirlar)
}

pub fn listeler<C: GenericClient>(c: &mut C) -> Result<Value, String> {
    Ok(Value::Array(
        c.query(
            "SELECT l.id, l.ad, l.aciklama, to_char(l.guncelleme, 'YYYY-MM-DD\"T\"HH24:MI'),
                    count(u.dosya_id), count(u.dosya_id) FILTER (WHERE d.durum_kod = 0)
               FROM dosya_listesi l LEFT JOIN dosya_listesi_uyesi u ON u.liste_id = l.id LEFT JOIN dosya d ON d.id = u.dosya_id
              GROUP BY l.id ORDER BY lower(l.ad)",
            &[],
        )
        .map_err(hata)?
        .iter()
        .map(|s| {
            json!({"id": s.get::<_, i64>(0), "ad": s.get::<_, String>(1), "aciklama": s.get::<_, String>(2),
                   "guncelleme": s.get::<_, String>(3), "dosya_sayisi": s.get::<_, i64>(4), "acik_sayisi": s.get::<_, i64>(5)})
        })
        .collect(),
    ))
}

/// Yeni liste (`id` yok) ya da ad / açıklama değişikliği. Döner: liste kimliği.
pub fn liste_kaydet<C: GenericClient>(c: &mut C, id: Option<i64>, ad: &str, aciklama: &str) -> Result<i64, String> {
    let ad = ad.trim();
    if ad.is_empty() {
        return Err("Liste adı boş olamaz.".into());
    }
    if ad.chars().count() > 120 {
        return Err("Liste adı en çok 120 harf olabilir.".into());
    }
    let ayni = c
        .query_opt("SELECT id FROM dosya_listesi WHERE lower(ad) = lower($1) AND ($2::bigint IS NULL OR id <> $2)", &[&ad, &id])
        .map_err(hata)?;
    if ayni.is_some() {
        return Err(format!("\"{ad}\" adlı bir liste zaten var."));
    }
    let aciklama = crate::icra_veri::kirp(aciklama.trim(), 500);
    match id {
        None => Ok(c
            .query_one("INSERT INTO dosya_listesi (ad, aciklama) VALUES ($1, $2) RETURNING id", &[&ad, &aciklama])
            .map_err(hata)?
            .get(0)),
        Some(id) => {
            let n = c
                .execute("UPDATE dosya_listesi SET ad = $2, aciklama = $3, guncelleme = now() WHERE id = $1", &[&id, &ad, &aciklama])
                .map_err(hata)?;
            if n == 0 {
                return Err("Liste bulunamadı.".into());
            }
            Ok(id)
        }
    }
}

pub fn liste_sil<C: GenericClient>(c: &mut C, id: i64) -> Result<(), String> {
    if c.execute("DELETE FROM dosya_listesi WHERE id = $1", &[&id]).map_err(hata)? == 0 {
        return Err("Liste bulunamadı.".into());
    }
    Ok(())
}

fn toplam<C: GenericClient>(c: &mut C, id: i64) -> Result<i64, String> {
    Ok(c.query_one("SELECT count(*) FROM dosya_listesi_uyesi WHERE liste_id = $1", &[&id]).map_err(hata)?.get(0))
}

/// Dosyaları listeye ekler (zaten olanlar ve veritabanında olmayan kimlikler atlanır).
pub fn liste_ekle<C: GenericClient>(c: &mut C, id: i64, dosyalar: &[i64]) -> Result<Value, String> {
    liste_dosyalari(c, id)?;
    let eklenen = c
        .execute(
            "INSERT INTO dosya_listesi_uyesi (liste_id, dosya_id) SELECT $1, d.id FROM dosya d WHERE d.id = ANY($2)
             ON CONFLICT DO NOTHING",
            &[&id, &dosyalar],
        )
        .map_err(hata)?;
    c.execute("UPDATE dosya_listesi SET guncelleme = now() WHERE id = $1", &[&id]).map_err(hata)?;
    Ok(json!({"eklenen": eklenen, "toplam": toplam(c, id)?}))
}

pub fn liste_cikar<C: GenericClient>(c: &mut C, id: i64, dosyalar: &[i64]) -> Result<Value, String> {
    liste_dosyalari(c, id)?;
    let cikan = c
        .execute("DELETE FROM dosya_listesi_uyesi WHERE liste_id = $1 AND dosya_id = ANY($2)", &[&id, &dosyalar])
        .map_err(hata)?;
    c.execute("UPDATE dosya_listesi SET guncelleme = now() WHERE id = $1", &[&id]).map_err(hata)?;
    Ok(json!({"cikan": cikan, "toplam": toplam(c, id)?}))
}

#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use crate::icra_veri::{dosya_kunyesi_kaydet, YargiTuru};
    let kunye = |no: &str| {
        json!({"dosyaId": "x", "dosyaNo": no, "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": 35, "birimAdi": "İzmir 3. İcra Dairesi", "birimId": 1003})
    };
    let (d1, _) = dosya_kunyesi_kaydet(c, &kunye("2026/301"), Some(YargiTuru::Icra.kod())).unwrap();
    let (d2, _) = dosya_kunyesi_kaydet(c, &kunye("2026/302"), Some(YargiTuru::Icra.kod())).unwrap();
    let (d3, _) = dosya_kunyesi_kaydet(c, &kunye("2026/303"), Some(YargiTuru::Icra.kod())).unwrap();

    assert!(liste_kaydet(c, None, "  ", "").unwrap_err().contains("boş"));
    let l = liste_kaydet(c, None, "Bankalar", "banka alacaklı dosyaları").unwrap();
    assert!(liste_kaydet(c, None, "bankalar", "").unwrap_err().contains("zaten var"), "ad büyük-küçük harf duyarsız tekil");
    assert_eq!(liste_ekle(c, l, &[d1, d2, 999_999_999]).unwrap()["eklenen"], 2, "olmayan dosya atlanır");
    assert_eq!(liste_ekle(c, l, &[d2, d3]).unwrap(), json!({"eklenen": 1, "toplam": 3}), "zaten olan atlanır");
    assert_eq!(liste_cikar(c, l, &[d3]).unwrap(), json!({"cikan": 1, "toplam": 2}));
    let q: HashMap<String, String> = [("liste".to_string(), l.to_string())].into_iter().collect();
    assert_eq!(liste_oku(&q), Some(l));
    let suzulen = suz(vec![d1, d2, d3], c, liste_oku(&q), |d| *d).unwrap();
    assert_eq!(suzulen, vec![d1, d2]);
    assert_eq!(suz(vec![d1, d3], c, None, |d| *d).unwrap(), vec![d1, d3], "liste seçilmemiş: dokunulmaz");

    // Her sekmenin süzgeci listeyi uygular.
    let dl = crate::icra_dosyalar::dosya_listesi(c, &crate::icra_dosyalar::Filtre::oku(&q)).unwrap();
    assert_eq!(dl["toplam"], 2);
    let mut q2 = q.clone();
    q2.insert("dosya_no".into(), "=2026/302".into());
    assert_eq!(crate::icra_dosyalar::dosya_listesi(c, &crate::icra_dosyalar::Filtre::oku(&q2)).unwrap()["toplam"], 1, "sekme süzgeci listenin içinde");
    let sur = crate::icra_api::suzulen_dosyalar(c, &crate::icra_api::filtre_oku(&q)).unwrap();
    assert_eq!(sur.iter().copied().collect::<HashSet<_>>(), [d1, d2].into_iter().collect());
    for (no, d) in [(10, d1), (11, d3)] {
        c.execute("INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, evrak_aciklama) VALUES ($1, $2, 'Banka', 'Elektronik Tebligat[X BANKASI]')",
                  &[&d, &no]).unwrap();
    }
    let t = crate::icra_tebligatlar::tebligat_listesi(c, &crate::icra_tebligatlar::Filtre::oku(&q)).unwrap();
    assert_eq!(t["toplam"], 1);
    assert_eq!(crate::icra_tebligatlar::tebligat_dosyalari(c, &crate::icra_tebligatlar::Filtre::oku(&q)).unwrap(), vec![d1]);

    let ls = listeler(c).unwrap();
    assert_eq!((ls[0]["ad"].as_str(), ls[0]["dosya_sayisi"].as_i64(), ls[0]["acik_sayisi"].as_i64()), (Some("Bankalar"), Some(2), Some(2)));
    liste_kaydet(c, Some(l), "Banka dosyaları", "").unwrap();
    liste_sil(c, l).unwrap();
    assert!(crate::icra_dosyalar::dosya_listesi(c, &crate::icra_dosyalar::Filtre::oku(&q)).unwrap_err().contains("bulunamadı"));
    c.batch_execute("DELETE FROM dosya; DELETE FROM birim").unwrap();
}
