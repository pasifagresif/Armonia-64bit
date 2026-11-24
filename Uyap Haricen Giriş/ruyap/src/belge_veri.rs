//! Büro belge deposu (2. aşama, 24.11.2026; plan docs/claude-notlar/planlar/yazi-cokluuyap.md).
//!
//! Armonia Yazı'da yazılan belge büro veritabanına kaydedilir: her kayıt yeni bir sürüm (`belge_surum`). Dosyalar
//! ofis diskinde yargı türü / birim / dosya klasörlerinde (`belge_disk.rs`, 24.11 akşam): son sürüm belgenin adıyla,
//! eskiler `.surumler` altında. Eski içerik adresli depodaki (`ekler/<sha256>`) belgeler açılışta bir kez taşınır
//! (`eski_belgeleri_yerlestir`). Belge bir dosyaya bağlıdır ve İş Listesi işlerine eklenir (`gorev_belge`); onay iş
//! üzerinden yürür (`onaya_sun`).
//!
//! Yetki (kullanıcı kararı 24.11): belgeyi oluşturan, bağlı işlerin sorumlusu / açanı / kişileri (sorumlu,
//! onaylayıcı, izleyici) ve yöneticiler görür ve düzenler. Göremeyene belge "bulunamadı" denir (varlığı sızmaz).
//! Aynı anda düzenleme: kaydederken istemci elindeki sürüm numarasını yollar; arada başkası kaydetmişse
//! `CAKISMA:` ile reddedilir (kimse yazdığını sessizce kaybetmez).

use chrono::NaiveDate;
use postgres::GenericClient;
use serde_json::{json, Value};

use crate::belge_disk::{self as bd, GeriAl};
use crate::is_veri::{self as iv, Kisi};

pub const EN_BUYUK: usize = 25 * 1024 * 1024;

fn hata(e: postgres::Error) -> String {
    match e.as_db_error() {
        Some(d) => format!("veritabanı: {}", d.message()),
        None => format!("veritabanı: {e}"),
    }
}

/// Kişinin belgeyi görüp görmediği: SQL koşulu. $1 = kişi kimliği, $2 = yönetici mi; `b` = belge tablosu.
const GOREBILIR: &str = "($2::boolean OR b.olusturan_id = $1::bigint OR EXISTS (
        SELECT 1 FROM gorev_belge gb JOIN gorev g ON g.id = gb.gorev_id
        WHERE gb.belge_id = b.id AND (g.sorumlu_id = $1::bigint OR g.olusturan_id = $1::bigint
              OR EXISTS (SELECT 1 FROM gorev_kisi gk WHERE gk.gorev_id = g.id AND gk.kullanici_id = $1::bigint))))";

fn ad_temizle(ad: &str) -> String {
    let t: String = ad
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_matches(|c| c == '.' || c == ' ');
    // Uzantı ad'da tutulmaz (biçim ayrı)
    let t = t.strip_suffix(".udf").or_else(|| t.strip_suffix(".UDF")).or_else(|| t.strip_suffix(".docx")).unwrap_or(t);
    let t: String = t.chars().take(200).collect();
    if t.trim().is_empty() { "Belge".into() } else { t.trim().to_string() }
}

fn sha_gecerli(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

fn sha256(veri: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(veri))
}

/// Eski içerik adresli depodan okur (klasörlere taşınmamış belge).
fn depodan_oku(sha: &str) -> Result<Vec<u8>, String> {
    if !sha_gecerli(sha) {
        return Err("Belge kaydı bozuk.".into());
    }
    std::fs::read(crate::is_aktar::ekler_dizini().join(sha)).map_err(|_| "Belgenin dosyası ofis bilgisayarında bulunamadı.".to_string())
}

/// Belgenin özü (yalnız görebilene).
pub struct BelgeOzu {
    pub id: i64,
    pub ad: String,
    pub bicim: String,
    pub dosya_id: Option<i64>,
    pub son_surum: i32,
}

pub fn belge_ozu<C: GenericClient>(c: &mut C, k: &Kisi, id: i64) -> Result<Option<BelgeOzu>, String> {
    let sql = format!("SELECT b.id, b.ad, b.bicim, b.dosya_id, b.son_surum FROM belge b WHERE b.id = $3 AND b.silindi IS NULL AND {GOREBILIR}");
    Ok(c.query_opt(&sql, &[&k.id, &k.yonetici, &id]).map_err(hata)?.map(|r| BelgeOzu {
        id: r.get(0),
        ad: r.get(1),
        bicim: r.get(2),
        dosya_id: r.get(3),
        son_surum: r.get(4),
    }))
}

/// UYAP kart verisinden (birimId + dosyaNo) büro veritabanındaki dosya.
pub fn karttan_dosya<C: GenericClient>(c: &mut C, kart: &Value) -> Result<Option<i64>, String> {
    let birim = kart["birimId"].as_str().map(str::to_string).or_else(|| kart["birimId"].as_i64().map(|n| n.to_string()));
    let no = kart["dosyaNo"].as_str().map(str::trim).unwrap_or("");
    let (Some(birim), false) = (birim, no.is_empty()) else { return Ok(None) };
    Ok(c
        .query_opt(
            "SELECT d.id FROM dosya d JOIN birim bi ON bi.id = d.birim_id WHERE bi.birim_id = $1 AND d.dosya_no = $2 ORDER BY d.id LIMIT 1",
            &[&birim, &no],
        )
        .map_err(hata)?
        .map(|r| r.get(0)))
}

fn dosya_kunyesi<C: GenericClient>(c: &mut C, dosya_id: i64) -> Result<Option<(String, String)>, String> {
    Ok(c
        .query_opt("SELECT bi.ad, d.dosya_no FROM dosya d JOIN birim bi ON bi.id = d.birim_id WHERE d.id = $1", &[&dosya_id])
        .map_err(hata)?
        .map(|r| (r.get(0), r.get(1))))
}

/// Dosyanın klasör künyesi: yargı türü adı, birim, numara, talimat mı.
fn dosya_klasor_kunyesi<C: GenericClient>(c: &mut C, dosya_id: i64) -> Result<Option<bd::Kunye>, String> {
    Ok(c
        .query_opt(
            "SELECT d.yargi_turu, bi.ad, d.dosya_no, d.tur_kod FROM dosya d JOIN birim bi ON bi.id = d.birim_id WHERE d.id = $1",
            &[&dosya_id],
        )
        .map_err(hata)?
        .map(|r| bd::Kunye {
            yargi: crate::icra_veri::YargiTuru::coz(r.get::<_, i16>(0)).map(|y| y.etiket()).unwrap_or("Diğer").to_string(),
            birim: r.get(1),
            dosya_no: r.get(2),
            talimat: r.get::<_, i32>(3) == 1,
        }))
}

/// Büroda kaydı olmayan UYAP dosyası: künye kart verisinden; yargı türü dosya türü kodundan.
fn kart_kunyesi(kart: &Value) -> bd::Kunye {
    let tur = kart["dosyaTurKod"].as_i64().or_else(|| kart["dosyaTurKod"].as_str().and_then(|s| s.trim().parse().ok()));
    let yargi = match tur {
        Some(35) => "İcra",
        Some(14) | Some(15) => "Hukuk",
        Some(3) => "Ceza",
        Some(16) => "Cbs",
        _ => "Diğer",
    };
    bd::Kunye {
        yargi: yargi.into(),
        birim: kart["birimAdi"].as_str().unwrap_or("").to_string(),
        dosya_no: kart["dosyaNo"].as_str().unwrap_or("").to_string(),
        talimat: tur == Some(1),
    }
}

/// Dosyasız belgelerin klasörü için kullanıcı adı (hesabın @'ten önceki kısmı).
fn kullanici_klasoru<C: GenericClient>(c: &mut C, kullanici: Option<i64>) -> Result<String, String> {
    let Some(id) = kullanici else { return Ok(String::new()) };
    let h: Option<String> = c.query_opt("SELECT hesap_adi FROM kullanici WHERE id = $1", &[&id]).map_err(hata)?.map(|r| r.get(0));
    Ok(h.map(|h| h.split('@').next().unwrap_or("").to_string()).unwrap_or_default())
}

/// Belgenin bulunması gereken klasör.
fn belge_klasoru<C: GenericClient>(c: &mut C, dosya_id: Option<i64>, kart: Option<&Value>, olusturan: Option<i64>) -> Result<Vec<String>, String> {
    let kunye = match (dosya_id, kart) {
        (Some(d), _) => dosya_klasor_kunyesi(c, d)?,
        (None, Some(k)) => Some(kart_kunyesi(k)),
        (None, None) => None,
    };
    let kullanici = kullanici_klasoru(c, olusturan)?;
    Ok(bd::klasor(kunye.as_ref(), &kullanici))
}

/// Yol başka bir belgede mi (büyük/küçük harf ayırmadan)?
fn yol_dolu<C: GenericClient>(c: &mut C, yol: &str, haric: i64) -> Result<bool, String> {
    Ok(c
        .query_one("SELECT EXISTS (SELECT 1 FROM belge WHERE lower(yol) = lower($1) AND id <> $2)", &[&yol, &haric])
        .map_err(hata)?
        .get(0))
}

fn parcalar(klasor: &str) -> Vec<String> {
    klasor.split('/').map(str::to_string).collect()
}

/// Belgenin disk yolu; eski içerik adresli depodaysa (yol yok) önce klasör düzenine yerleştirilir: sürümler
/// klasöre kopyalanır, `yol` yazılır. Eski depodaki kopyalar `eski_shalari_temizle` ile silinir.
fn yerlestir<C: GenericClient>(c: &mut C, id: i64, geri: &mut GeriAl) -> Result<String, String> {
    let r = c
        .query_one("SELECT yol, ad, bicim, dosya_id, uyap_karti, olusturan_id, son_surum FROM belge WHERE id = $1", &[&id])
        .map_err(hata)?;
    if let Some(y) = r.get::<_, Option<String>>(0) {
        return Ok(y);
    }
    let (ad, bicim, son): (String, String, i32) = (r.get(1), r.get(2), r.get(6));
    let kart: Option<Value> = r.get(4);
    let klasor = belge_klasoru(c, r.get(3), kart.as_ref(), r.get(5))?;
    let yol = bd::bos_yol(&klasor, &ad, &bicim, |y| yol_dolu(c, y, id))?;
    let surumler = c.query("SELECT surum, sha256 FROM belge_surum WHERE belge_id = $1 ORDER BY surum", &[&id]).map_err(hata)?;
    for s in &surumler {
        let n: i32 = s.get(0);
        let veri = depodan_oku(&s.get::<_, String>(1))?;
        let hedef = if n == son { bd::coz(&yol)? } else { bd::surum_dosyasi(&yol, id, n, &bicim)? };
        bd::yaz(&hedef, &veri)?;
        bd::gizle_surumler(&hedef);
        geri.yazildi(&hedef);
    }
    c.execute("UPDATE belge SET yol = $2 WHERE id = $1", &[&id, &yol]).map_err(hata)?;
    Ok(yol)
}

/// Eski depodaki (ekler/<sha256>) dosyalardan artık hiçbir şeyin kullanmadıklarını siler: iş ekleri ya da klasöre
/// taşınmamış belgeler kullanıyorsa kalır.
fn eski_shalari_temizle<C: GenericClient>(c: &mut C, shalar: &[String]) -> Result<(), String> {
    for sha in shalar {
        if !sha_gecerli(sha) {
            continue;
        }
        let kullanilan: bool = c
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM gorev_ek WHERE sha256 = $1)
                     OR EXISTS (SELECT 1 FROM belge_surum bs JOIN belge b ON b.id = bs.belge_id WHERE bs.sha256 = $1 AND b.yol IS NULL)",
                &[sha],
            )
            .map_err(hata)?
            .get(0);
        if !kullanilan {
            let _ = std::fs::remove_file(crate::is_aktar::ekler_dizini().join(sha));
        }
    }
    Ok(())
}

/// Açılışta bir kez: eski içerik adresli depodaki belgeleri klasör düzenine taşır. (taşınan sayısı, hatalar);
/// taşınamayan belge (dosyası kayıp) eski yerinde kalır, okunurken eski depodan denenir.
pub fn eski_belgeleri_yerlestir(c: &mut postgres::Client) -> Result<(usize, Vec<String>), String> {
    let idler: Vec<i64> = c.query("SELECT id FROM belge WHERE yol IS NULL ORDER BY id", &[]).map_err(hata)?.iter().map(|r| r.get(0)).collect();
    let (mut n, mut hatalar) = (0, Vec::new());
    for id in idler {
        let mut tx = c.transaction().map_err(hata)?;
        let mut geri = GeriAl::default();
        let shalar: Vec<String> =
            tx.query("SELECT sha256 FROM belge_surum WHERE belge_id = $1", &[&id]).map_err(hata)?.iter().map(|r| r.get(0)).collect();
        match yerlestir(&mut tx, id, &mut geri) {
            Ok(_) => {
                tx.commit().map_err(hata)?;
                geri.tamam();
                n += 1;
                let _ = eski_shalari_temizle(c, &shalar);
            }
            Err(e) => hatalar.push(format!("belge {id}: {e}")),
        }
    }
    Ok((n, hatalar))
}

/// Kayıt isteği.
#[derive(Clone, Debug, Default)]
pub struct Kayit {
    /// Var olan belge (yoksa yeni belge açılır)
    pub id: Option<i64>,
    /// İstemcinin elindeki sürüm (var olan belgede zorunlu; çakışma denetimi)
    pub surum: Option<i32>,
    pub ad: String,
    pub bicim: String,
    pub dosya_id: Option<i64>,
    /// UYAP kart verisi (dosyaId, dosyaNo, birimId, birimAdi…): gönderim için saklanır, dosya_id'yi bulmaya yarar
    pub uyap_karti: Option<Value>,
    pub imzali: bool,
    pub notu: String,
    /// Yeni belgeyi bu işe ekle
    pub gorev_id: Option<i64>,
}

/// Belgeyi kaydeder: yeni belge ya da yeni sürüm. `{id, surum, yeni}` döner. Son sürümle aynı bayt ve aynı imza
/// durumu gelirse yeni sürüm açılmaz.
pub fn kaydet<C: GenericClient>(c: &mut C, k: &Kisi, r: &Kayit, veri: &[u8]) -> Result<Value, String> {
    if veri.is_empty() {
        return Err("Belge boş.".into());
    }
    if veri.len() > EN_BUYUK {
        return Err(format!("Belge çok büyük (en çok {} MB).", EN_BUYUK / 1024 / 1024));
    }
    let bicim = if r.bicim.is_empty() { "udf" } else { r.bicim.as_str() };
    if !["udf", "docx"].contains(&bicim) {
        return Err("Bilinmeyen belge biçimi.".into());
    }
    let notu: String = r.notu.trim().chars().take(300).collect();
    let mut tx = c.transaction().map_err(hata)?;
    // Disk işleri veritabanıyla birlikte: commit'ten önce çıkılırsa geri alınır
    let mut geri = GeriAl::default();
    let sha = sha256(veri);
    let boyut = veri.len() as i64;
    if let Some(id) = r.id {
        let oz = belge_ozu(&mut tx, k, id)?.ok_or("Belge bulunamadı.")?;
        let son: (i32, Option<String>, bool) = {
            let s = tx.query_one("SELECT son_surum FROM belge WHERE id = $1 FOR UPDATE", &[&id]).map_err(hata)?;
            let son: i32 = s.get(0);
            let ust = tx
                .query_opt(
                    "SELECT bs.sha256, bs.imzali, coalesce(nullif(ku.ad, ''), ku.hesap_adi, '') FROM belge_surum bs
                     LEFT JOIN kullanici ku ON ku.id = bs.kaydeden_id WHERE bs.belge_id = $1 AND bs.surum = $2",
                    &[&id, &son],
                )
                .map_err(hata)?;
            match ust {
                Some(u) => {
                    let beklenen = r.surum.ok_or("Kaydetmek için belgenin sürüm numarası gerekli.")?;
                    if beklenen != son {
                        let kim: String = u.get(2);
                        return Err(format!(
                            "CAKISMA: Bu belgeyi siz açtıktan sonra {} kaydetti (sürüm {son}). Kaydetmeden önce son hâlini açın; değişikliklerinizi kaybetmemek için önce \"Kaydet ▾\" ile bilgisayarınıza indirin.",
                            if kim.is_empty() { "başka biri".to_string() } else { kim }
                        ));
                    }
                    (son, Some(u.get(0)), u.get(1))
                }
                None => (son, None, false),
            }
        };
        if son.1.as_deref() == Some(sha.as_str()) && son.2 == r.imzali {
            tx.commit().map_err(hata)?;
            return Ok(json!({"id": id, "surum": son.0, "yeni": false}));
        }
        let yeni = son.0 + 1;
        let ad = if r.ad.trim().is_empty() { oz.ad.clone() } else { ad_temizle(&r.ad) };
        // Disk: son sürüm .surumler'e iner, yenisi belgenin adıyla yazılır (ad değiştiyse aynı klasörde yeni adla)
        let yol = yerlestir(&mut tx, id, &mut geri)?;
        let ana = bd::coz(&yol)?;
        if son.0 > 0 && ana.exists() {
            let eski = bd::surum_dosyasi(&yol, id, son.0, &oz.bicim)?;
            bd::tasi(&ana, &eski)?;
            geri.tasindi(&ana, &eski);
        }
        let yeni_yol = if ad != oz.ad {
            bd::bos_yol(&parcalar(bd::klasoru(&yol)), &ad, &oz.bicim, |y| yol_dolu(&mut tx, y, id))?
        } else {
            yol
        };
        let yeni_ana = bd::coz(&yeni_yol)?;
        bd::yaz(&yeni_ana, veri)?;
        geri.yazildi(&yeni_ana);
        tx.execute(
            "INSERT INTO belge_surum (belge_id, surum, sha256, boyut, imzali, kaydeden_id, notu) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[&id, &yeni, &sha, &boyut, &r.imzali, &k.id, &notu],
        )
        .map_err(hata)?;
        let (bi, no): (String, String) =
            tx.query_one("SELECT birim_adi, dosya_no FROM belge WHERE id = $1", &[&id]).map_err(hata).map(|r| (r.get(0), r.get(1)))?;
        tx.execute(
            "UPDATE belge SET son_surum = $2, imzali = $3, ad = $4, yol = $6, guncelleme = now(),
                    arama_metni = $5 WHERE id = $1",
            &[&id, &yeni, &r.imzali, &ad, &iv::arama_katla(&[&ad, &bi, &no]), &yeni_yol],
        )
        .map_err(hata)?;
        tx.commit().map_err(hata)?;
        geri.tamam();
        return Ok(json!({"id": id, "surum": yeni, "yeni": false}));
    }
    // Yeni belge
    let mut dosya_id = r.dosya_id;
    if dosya_id.is_none() {
        if let Some(kart) = &r.uyap_karti {
            dosya_id = karttan_dosya(&mut tx, kart)?;
        }
    }
    let (mut birim_adi, mut dosya_no) = (String::new(), String::new());
    if let Some(d) = dosya_id {
        let (b, n) = dosya_kunyesi(&mut tx, d)?.ok_or("Dosya bulunamadı.")?;
        birim_adi = b;
        dosya_no = n;
    } else if let Some(kart) = &r.uyap_karti {
        birim_adi = kart["birimAdi"].as_str().unwrap_or("").chars().take(255).collect();
        dosya_no = kart["dosyaNo"].as_str().unwrap_or("").chars().take(60).collect();
    }
    let ad = ad_temizle(&r.ad);
    let arama = iv::arama_katla(&[&ad, &birim_adi, &dosya_no]);
    // Klasör belge kaydedilince oluşur
    let kart = if dosya_id.is_none() { r.uyap_karti.as_ref() } else { None };
    let klasor = belge_klasoru(&mut tx, dosya_id, kart, Some(k.id))?;
    let yol = bd::bos_yol(&klasor, &ad, bicim, |y| yol_dolu(&mut tx, y, 0))?;
    let ana = bd::coz(&yol)?;
    bd::yaz(&ana, veri)?;
    geri.yazildi(&ana);
    let id: i64 = tx
        .query_one(
            "INSERT INTO belge (ad, bicim, dosya_id, birim_adi, dosya_no, uyap_karti, olusturan_id, son_surum, imzali, arama_metni, yol)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8, $9, $10) RETURNING id",
            &[&ad, &bicim, &dosya_id, &birim_adi, &dosya_no, &r.uyap_karti, &k.id, &r.imzali, &arama, &yol],
        )
        .map_err(hata)?
        .get(0);
    tx.execute(
        "INSERT INTO belge_surum (belge_id, surum, sha256, boyut, imzali, kaydeden_id, notu) VALUES ($1, 1, $2, $3, $4, $5, $6)",
        &[&id, &sha, &boyut, &r.imzali, &k.id, &notu],
    )
    .map_err(hata)?;
    if let Some(g) = r.gorev_id {
        ise_ekle_ic(&mut tx, k, id, g)?;
    }
    tx.commit().map_err(hata)?;
    geri.tamam();
    Ok(json!({"id": id, "surum": 1, "yeni": true}))
}

/// Belgenin baytları: istenen sürüm (yoksa son). (ad, biçim, sürüm, imzalı, bayt)
pub fn oku<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, surum: Option<i32>) -> Result<(String, String, i32, bool, Vec<u8>), String> {
    let oz = belge_ozu(c, k, id)?.ok_or("Belge bulunamadı.")?;
    let s = surum.unwrap_or(oz.son_surum);
    let r = c
        .query_opt("SELECT sha256, imzali FROM belge_surum WHERE belge_id = $1 AND surum = $2", &[&id, &s])
        .map_err(hata)?
        .ok_or("Bu sürüm bulunamadı.")?;
    let sha: String = r.get(0);
    let yol: Option<String> = c.query_one("SELECT yol FROM belge WHERE id = $1", &[&id]).map_err(hata)?.get(0);
    let veri = match yol {
        Some(y) => {
            let p = if s == oz.son_surum { bd::coz(&y)? } else { bd::surum_dosyasi(&y, id, s, &oz.bicim)? };
            match std::fs::read(&p) {
                Ok(v) => v,
                // Taşınırken eski depoda kalmış olabilir
                Err(_) => depodan_oku(&sha).map_err(|_| format!("Belgenin dosyası ofis bilgisayarında bulunamadı (Belgeler/{y}, sürüm {s})."))?,
            }
        }
        None => depodan_oku(&sha)?,
    };
    Ok((oz.ad, oz.bicim, s, r.get(1), veri))
}

fn belge_json(r: &postgres::Row) -> Value {
    json!({
        "id": r.get::<_, i64>("id"), "ad": r.get::<_, String>("ad"), "bicim": r.get::<_, String>("bicim"),
        "dosya_id": r.get::<_, Option<i64>>("dosya_id"), "birim_adi": r.get::<_, String>("birim_adi"),
        "dosya_no": r.get::<_, String>("dosya_no"), "son_surum": r.get::<_, i32>("son_surum"),
        "imzali": r.get::<_, bool>("imzali"), "gonderim": r.get::<_, Option<Value>>("gonderim"),
        "uyap_karti": r.get::<_, Option<Value>>("uyap_karti"),
        "olusturan": r.get::<_, String>("olusturan"), "guncelleme": r.get::<_, String>("guncelleme_m"),
        "is_sayisi": r.get::<_, i64>("is_sayisi"),
        "klasor": r.get::<_, Option<String>>("yol").map(|y| bd::klasoru(&y).to_string()),
    })
}

const LISTE_SECIM: &str = "SELECT b.id, b.ad, b.bicim, b.dosya_id, b.birim_adi, b.dosya_no, b.son_surum, b.imzali, b.gonderim, b.uyap_karti, b.yol,
        coalesce(nullif(ku.ad, ''), ku.hesap_adi, '') AS olusturan,
        to_char(b.guncelleme AT TIME ZONE 'Europe/Istanbul', 'DD.MM.YYYY HH24:MI') AS guncelleme_m,
        (SELECT count(*) FROM gorev_belge x WHERE x.belge_id = b.id) AS is_sayisi
     FROM belge b LEFT JOIN kullanici ku ON ku.id = b.olusturan_id";

/// "Belgelerim": kişinin oluşturduğu ya da işleri (sorumlu / kişi) yoluyla bağlı olduğu belgeler. $1 = kişi.
const BENIM: &str = "(b.olusturan_id = $1::bigint OR EXISTS (
        SELECT 1 FROM gorev_belge gb JOIN gorev g ON g.id = gb.gorev_id WHERE gb.belge_id = b.id
        AND (g.sorumlu_id = $1::bigint OR EXISTS (SELECT 1 FROM gorev_kisi gk WHERE gk.gorev_id = g.id AND gk.kullanici_id = $1::bigint))))";

/// LIKE deseninde \, % ve _ kaçırılır (klasör adında geçebilir; PostgreSQL'de varsayılan kaçış \).
fn like_kacir(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            o.push('\\');
        }
        o.push(ch);
    }
    o
}

/// Kişinin görebildiği belgeler, en yeni önce. Süzgeçler: dosya, iş, arama metni, klasör (alt klasörleriyle).
/// `tumu` değilse yalnız "Belgelerim" (dosya ya da iş süzgeci verilince bu kısıt kalkar).
pub fn liste<C: GenericClient>(c: &mut C, k: &Kisi, dosya_id: Option<i64>, gorev_id: Option<i64>, ara: &str, klasor: &str, tumu: bool) -> Result<Vec<Value>, String> {
    let ara = iv::arama_katla(&[ara]);
    let desen = format!("%{}%", ara.replace('%', "").replace('_', ""));
    let klasor = klasor.trim_matches('/').to_string();
    let klasor_deseni = format!("{}/%", like_kacir(&klasor));
    let sql = format!(
        "{LISTE_SECIM} WHERE b.silindi IS NULL AND {GOREBILIR}
           AND ($3::bigint IS NULL OR b.dosya_id = $3)
           AND ($4::bigint IS NULL OR EXISTS (SELECT 1 FROM gorev_belge y WHERE y.belge_id = b.id AND y.gorev_id = $4))
           AND ($5::varchar = '' OR b.arama_metni LIKE $6)
           AND ($8::varchar = '' OR b.yol LIKE $9)
           AND ($7::boolean OR $3::bigint IS NOT NULL OR $4::bigint IS NOT NULL OR {BENIM})
         ORDER BY b.guncelleme DESC LIMIT 200"
    );
    Ok(c
        .query(&sql, &[&k.id, &k.yonetici, &dosya_id, &gorev_id, &ara, &desen, &tumu, &klasor, &klasor_deseni])
        .map_err(hata)?
        .iter()
        .map(belge_json)
        .collect())
}

/// Klasör ağacı: yargı türü → birim → dosya, her düğümde (alt klasörleriyle) belge sayısı; yalnız kişinin
/// görebildiği belgeler (`tumu` değilse "Belgelerim"). Ağaç diskteki düzenin aynısı (belge yollarından).
pub fn agac<C: GenericClient>(c: &mut C, k: &Kisi, tumu: bool) -> Result<Value, String> {
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Dugum {
        sayi: i64,
        alt: BTreeMap<String, Dugum>,
    }
    // Sıra: dosya numaraları yeniden eskiye (yıl, sıra); öteki adlar Türkçe abece; "Dosyasız" en sonda
    fn sira_anahtari(ad: &str) -> (u8, i64, i64, String) {
        let sayisal = ad
            .split(' ')
            .next()
            .and_then(|n| n.split_once('-'))
            .and_then(|(y, s)| Some((y.parse::<i64>().ok()?, s.parse::<i64>().ok()?)));
        match sayisal {
            Some((y, s)) => (0, -y, -s, String::new()),
            None if ad == bd::DOSYASIZ => (2, 0, 0, String::new()),
            None => (1, 0, 0, iv::arama_katla(&[ad])),
        }
    }
    fn json_yap(d: &Dugum, ust: &str) -> Vec<Value> {
        let mut v: Vec<(&String, &Dugum)> = d.alt.iter().collect();
        v.sort_by_key(|(ad, _)| sira_anahtari(ad));
        v.into_iter()
            .map(|(ad, a)| {
                let klasor = if ust.is_empty() { ad.clone() } else { format!("{ust}/{ad}") };
                json!({"ad": ad, "klasor": klasor, "sayi": a.sayi, "alt": json_yap(a, &klasor)})
            })
            .collect()
    }
    let sql = format!("SELECT b.yol FROM belge b WHERE b.silindi IS NULL AND b.yol IS NOT NULL AND {GOREBILIR} AND ($3::boolean OR {BENIM})");
    let mut kok = Dugum::default();
    for r in c.query(&sql, &[&k.id, &k.yonetici, &tumu]).map_err(hata)? {
        let yol: String = r.get(0);
        kok.sayi += 1;
        let mut d = &mut kok;
        for p in bd::klasoru(&yol).split('/').filter(|p| !p.is_empty()) {
            d = d.alt.entry(p.to_string()).or_default();
            d.sayi += 1;
        }
    }
    Ok(json!({"sayi": kok.sayi, "alt": json_yap(&kok, "")}))
}

/// Belgenin ayrıntısı: özet + sürümler + bağlı işler.
pub fn getir<C: GenericClient>(c: &mut C, k: &Kisi, id: i64) -> Result<Value, String> {
    belge_ozu(c, k, id)?.ok_or("Belge bulunamadı.")?;
    let r = c.query_one(&format!("{LISTE_SECIM} WHERE b.id = $1"), &[&id]).map_err(hata)?;
    let mut v = belge_json(&r);
    let surumler: Vec<Value> = c
        .query(
            "SELECT bs.surum, bs.boyut, bs.imzali, bs.notu, coalesce(nullif(ku.ad, ''), ku.hesap_adi, ''),
                    to_char(bs.olusturulma AT TIME ZONE 'Europe/Istanbul', 'DD.MM.YYYY HH24:MI')
             FROM belge_surum bs LEFT JOIN kullanici ku ON ku.id = bs.kaydeden_id WHERE bs.belge_id = $1 ORDER BY bs.surum DESC",
            &[&id],
        )
        .map_err(hata)?
        .iter()
        .map(|s| json!({"surum": s.get::<_, i32>(0), "boyut": s.get::<_, i64>(1), "imzali": s.get::<_, bool>(2),
                        "notu": s.get::<_, String>(3), "kaydeden": s.get::<_, String>(4), "zaman": s.get::<_, String>(5)}))
        .collect();
    let isler: Vec<Value> = c
        .query(
            "SELECT g.id, g.baslik, g.durum, to_char(g.son_tarih, 'DD.MM.YYYY') FROM gorev_belge gb JOIN gorev g ON g.id = gb.gorev_id
             WHERE gb.belge_id = $1 ORDER BY g.son_tarih DESC",
            &[&id],
        )
        .map_err(hata)?
        .iter()
        .map(|g| json!({"id": g.get::<_, i64>(0), "baslik": g.get::<_, String>(1), "durum": g.get::<_, String>(2),
                        "durum_adi": iv::etiket(iv::DURUMLAR, &g.get::<_, String>(2)), "son_tarih": g.get::<_, String>(3)}))
        .collect();
    v["surumler"] = json!(surumler);
    v["isler"] = json!(isler);
    Ok(v)
}

/// Belgeyi bir dosyaya bağlar (ya da bağı kaldırır); dosyaları o dosyanın klasörüne taşınır (eski sürümleriyle).
pub fn dosya_bagla<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, dosya_id: Option<i64>) -> Result<(), String> {
    let oz = belge_ozu(c, k, id)?.ok_or("Belge bulunamadı.")?;
    let (b, n) = match dosya_id {
        Some(d) => dosya_kunyesi(c, d)?.ok_or("Dosya bulunamadı.")?,
        None => (String::new(), String::new()),
    };
    let mut tx = c.transaction().map_err(hata)?;
    let mut geri = GeriAl::default();
    tx.query_one("SELECT 1 FROM belge WHERE id = $1 FOR UPDATE", &[&id]).map_err(hata)?;
    let yol = yerlestir(&mut tx, id, &mut geri)?;
    let olusturan: Option<i64> = tx.query_one("SELECT olusturan_id FROM belge WHERE id = $1", &[&id]).map_err(hata)?.get(0);
    let klasor = belge_klasoru(&mut tx, dosya_id, None, olusturan)?;
    let yeni_yol = if klasor.join("/") == bd::klasoru(&yol) {
        yol.clone()
    } else {
        let y = bd::bos_yol(&klasor, &oz.ad, &oz.bicim, |y| yol_dolu(&mut tx, y, id))?;
        let (ana, yeni_ana) = (bd::coz(&yol)?, bd::coz(&y)?);
        if ana.exists() {
            bd::tasi(&ana, &yeni_ana)?;
            geri.tasindi(&ana, &yeni_ana);
        }
        let (sk, yeni_sk) = (bd::surum_klasoru(&yol, id)?, bd::surum_klasoru(&y, id)?);
        if sk.exists() {
            bd::tasi(&sk, &yeni_sk)?;
            geri.tasindi(&sk, &yeni_sk);
        }
        y
    };
    tx.execute(
        "UPDATE belge SET dosya_id = $2, birim_adi = $3, dosya_no = $4, uyap_karti = CASE WHEN $2::bigint IS NULL THEN NULL ELSE uyap_karti END,
                guncelleme = now(), arama_metni = $5, yol = $6 WHERE id = $1",
        &[&id, &dosya_id, &b, &n, &iv::arama_katla(&[&oz.ad, &b, &n]), &yeni_yol],
    )
    .map_err(hata)?;
    tx.commit().map_err(hata)?;
    geri.tamam();
    if yeni_yol != yol {
        if let Ok(k) = bd::coz(bd::klasoru(&yol)) {
            bd::bos_klasorleri_sil(&k);
        }
    }
    Ok(())
}

/// Belgenin adını değiştirir (diskte de, aynı klasörde; ad doluysa "(2)" eklenir). Belgeyi düzenleyebilen herkes.
/// Yeni adı döner.
pub fn yeniden_adlandir<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, ad: &str) -> Result<String, String> {
    let oz = belge_ozu(c, k, id)?.ok_or("Belge bulunamadı.")?;
    if ad.trim().is_empty() {
        return Err("Belgenin adı boş olamaz.".into());
    }
    let ad = ad_temizle(ad);
    if ad == oz.ad {
        return Ok(ad);
    }
    let mut tx = c.transaction().map_err(hata)?;
    let mut geri = GeriAl::default();
    tx.query_one("SELECT 1 FROM belge WHERE id = $1 FOR UPDATE", &[&id]).map_err(hata)?;
    let yol = yerlestir(&mut tx, id, &mut geri)?;
    let yeni_yol = bd::bos_yol(&parcalar(bd::klasoru(&yol)), &ad, &oz.bicim, |y| yol_dolu(&mut tx, y, id))?;
    let (ana, yeni_ana) = (bd::coz(&yol)?, bd::coz(&yeni_yol)?);
    if ana.exists() {
        bd::tasi(&ana, &yeni_ana)?;
        geri.tasindi(&ana, &yeni_ana);
    }
    let (bi, no): (String, String) =
        tx.query_one("SELECT birim_adi, dosya_no FROM belge WHERE id = $1", &[&id]).map_err(hata).map(|r| (r.get(0), r.get(1)))?;
    tx.execute(
        "UPDATE belge SET ad = $2, yol = $3, arama_metni = $4, guncelleme = now() WHERE id = $1",
        &[&id, &ad, &yeni_yol, &iv::arama_katla(&[&ad, &bi, &no])],
    )
    .map_err(hata)?;
    tx.commit().map_err(hata)?;
    geri.tamam();
    Ok(ad)
}

fn ise_ekle_ic<C: GenericClient>(c: &mut C, k: &Kisi, belge: i64, gorev: i64) -> Result<(), String> {
    let oz = iv::gorev_ozu(c, gorev)?.ok_or("İş bulunamadı.")?;
    if !iv::yetki(c, &oz, k)?.calisabilir {
        return Err("Bu işe belge ekleme yetkiniz yok.".into());
    }
    let n = c
        .execute(
            "INSERT INTO gorev_belge (gorev_id, belge_id, ekleyen_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            &[&gorev, &belge, &k.id],
        )
        .map_err(hata)?;
    if n > 0 {
        let ad: String = c.query_one("SELECT ad FROM belge WHERE id = $1", &[&belge]).map_err(hata)?.get(0);
        iv::olay_yaz(c, Some(gorev), Some(k.id), iv::olay::EK, json!({"belge": belge, "ad": ad}))?;
    }
    Ok(())
}

/// Belgeyi işe ekler (belgeyi görebilen ve işte çalışabilen).
pub fn ise_ekle<C: GenericClient>(c: &mut C, k: &Kisi, belge: i64, gorev: i64) -> Result<(), String> {
    belge_ozu(c, k, belge)?.ok_or("Belge bulunamadı.")?;
    ise_ekle_ic(c, k, belge, gorev)
}

pub fn isten_cikar<C: GenericClient>(c: &mut C, k: &Kisi, belge: i64, gorev: i64) -> Result<(), String> {
    belge_ozu(c, k, belge)?.ok_or("Belge bulunamadı.")?;
    let oz = iv::gorev_ozu(c, gorev)?.ok_or("İş bulunamadı.")?;
    if !iv::yetki(c, &oz, k)?.calisabilir {
        return Err("Bu işten belge çıkarma yetkiniz yok.".into());
    }
    c.execute("DELETE FROM gorev_belge WHERE gorev_id = $1 AND belge_id = $2", &[&gorev, &belge]).map_err(hata)?;
    Ok(())
}

/// Onaya sunar (İş Listesi üzerinden). `gorev` verilmişse belge o işe eklenir ve iş onaya gider; verilmemişse
/// "Belge onayı: <ad>" işi açılır (sorumlusu sunan, onay gerekir, belgenin dosyasına bağlı), `onaylayici`
/// onaylayıcı olarak eklenir ve iş onaya gider. İşin kimliğini döner.
pub fn onaya_sun<C: GenericClient>(c: &mut C, k: &Kisi, belge: i64, gorev: Option<i64>, onaylayici: Option<i64>, notu: &str, bugun: NaiveDate) -> Result<i64, String> {
    let oz = belge_ozu(c, k, belge)?.ok_or("Belge bulunamadı.")?;
    let mut tx = c.transaction().map_err(hata)?;
    let gid = match gorev {
        Some(g) => {
            ise_ekle_ic(&mut tx, k, belge, g)?;
            g
        }
        None => {
            let onaylayici = onaylayici.ok_or("Onaylayacak kişiyi seçin.")?;
            if onaylayici == k.id {
                return Err("Belgeyi kendinize onaya sunamazsınız; başka bir kişi seçin.".into());
            }
            let (birim_adi, dosya_no): (String, String) = tx
                .query_one("SELECT birim_adi, dosya_no FROM belge WHERE id = $1", &[&belge])
                .map_err(hata)
                .map(|r| (r.get(0), r.get(1)))?;
            let y = iv::YeniGorev {
                baslik: format!("Belge onayı: {}", oz.ad),
                aciklama: notu.trim().to_string(),
                onay_gerekir: true,
                sorumlu_id: Some(k.id),
                son_tarih: Some(bugun),
                dosya_id: oz.dosya_id,
                birim_adi,
                dosya_no,
                ..Default::default()
            };
            let g = iv::gorev_ac(&mut tx, Some(k), &y, bugun)?;
            iv::kisi_ekle(&mut tx, k, g, onaylayici, "onaylayici")?;
            ise_ekle_ic(&mut tx, k, belge, g)?;
            g
        }
    };
    if gorev.is_some() && !notu.trim().is_empty() {
        iv::not_ekle(&mut tx, k, gid, notu)?;
    }
    // Onay gerektirmeyen işte de belge onayı istendi: işaretlenir
    tx.execute("UPDATE gorev SET onay_gerekir = true WHERE id = $1 AND NOT onay_gerekir", &[&gid]).map_err(hata)?;
    iv::durum_degistir(&mut tx, k, gid, "onayda", "", "", bugun)?;
    tx.commit().map_err(hata)?;
    Ok(gid)
}

/// UYAP'a gönderim belgeye işlenir (son gönderim).
pub fn gonderildi<C: GenericClient>(c: &mut C, k: &Kisi, id: i64, tur: &str, tur_adi: &str, mesaj: &str) -> Result<(), String> {
    belge_ozu(c, k, id)?.ok_or("Belge bulunamadı.")?;
    let kisi: String = c
        .query_one("SELECT coalesce(nullif(ad, ''), hesap_adi) FROM kullanici WHERE id = $1", &[&k.id])
        .map_err(hata)?
        .get(0);
    let zaman: String = c
        .query_one("SELECT to_char(now() AT TIME ZONE 'Europe/Istanbul', 'DD.MM.YYYY HH24:MI')", &[])
        .map_err(hata)?
        .get(0);
    let kes = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    let g = json!({"zaman": zaman, "tur": kes(tur, 80), "tur_adi": kes(tur_adi, 200), "mesaj": kes(mesaj, 500), "kisi": kisi});
    c.execute("UPDATE belge SET gonderim = $2, guncelleme = now() WHERE id = $1", &[&id, &g]).map_err(hata)?;
    Ok(())
}

/// Belgeyi KALICI siler: bütün sürümleri, iş bağları ve (başka yerde kullanılmıyorsa) diskteki dosyaları.
/// Taslaklar birikip şişirmesin diye işaretleme değil gerçek silme. Yalnız oluşturan ya da yönetici;
/// UYAP'a gönderilmiş belgeyi yalnız yönetici siler (gönderim kaydı belgeyle gider).
pub fn sil<C: GenericClient>(c: &mut C, k: &Kisi, id: i64) -> Result<(), String> {
    belge_ozu(c, k, id)?.ok_or("Belge bulunamadı.")?;
    let r = c.query_one("SELECT olusturan_id, gonderim IS NOT NULL, ad, yol FROM belge WHERE id = $1", &[&id]).map_err(hata)?;
    let (olusturan, gonderildi, ad, yol): (Option<i64>, bool, String, Option<String>) = (r.get(0), r.get(1), r.get(2), r.get(3));
    if !(k.yonetici || olusturan == Some(k.id)) {
        return Err("Belgeyi yalnız oluşturan ya da yönetici silebilir.".into());
    }
    if gonderildi && !k.yonetici {
        return Err("UYAP'a gönderilmiş belgeyi yalnız yönetici silebilir.".into());
    }
    let mut tx = c.transaction().map_err(hata)?;
    let shalar: Vec<String> = tx
        .query("SELECT DISTINCT sha256 FROM belge_surum WHERE belge_id = $1", &[&id])
        .map_err(hata)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let isler: Vec<i64> = tx
        .query("SELECT gorev_id FROM gorev_belge WHERE belge_id = $1", &[&id])
        .map_err(hata)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    tx.execute("DELETE FROM belge WHERE id = $1", &[&id]).map_err(hata)?; // sürümler ve iş bağları CASCADE
    for g in isler {
        iv::olay_yaz(&mut tx, Some(g), Some(k.id), iv::olay::EK, json!({"belge_silindi": id, "ad": ad}))?;
    }
    tx.commit().map_err(hata)?;
    // Disk: klasördeki dosyalar ve boşalan klasörler; eski depoda kalmış kopyalar
    if let Some(y) = yol {
        bd::belgeyi_sil(&y, id);
    }
    eski_shalari_temizle(c, &shalar)?;
    Ok(())
}

/// Büro veritabanında dosya arama (dosya no, birim adı, taraf adı). En çok 30.
pub fn dosya_ara<C: GenericClient>(c: &mut C, ara: &str) -> Result<Vec<Value>, String> {
    let ara = ara.trim();
    if ara.chars().count() < 2 {
        return Ok(vec![]);
    }
    let desen = format!("%{}%", ara.replace('%', "").replace('_', "").to_lowercase());
    let rows = c
        .query(
            "SELECT d.id, bi.ad, d.dosya_no, d.tur, bi.birim_id,
                    coalesce((SELECT string_agg(CASE WHEN t.unvan <> '' THEN t.unvan ELSE trim(t.ad || ' ' || t.soyad) END, ', ' ORDER BY dt.sira)
                              FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE dt.dosya_id = d.id AND dt.rol = 'borclu'), '')
             FROM dosya d JOIN birim bi ON bi.id = d.birim_id
             WHERE lower(d.dosya_no) LIKE $1 OR lower(bi.ad || ' ' || d.dosya_no) LIKE $1
                OR EXISTS (SELECT 1 FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE dt.dosya_id = d.id
                           AND lower(t.ad || ' ' || t.soyad || ' ' || t.unvan) LIKE $1)
             ORDER BY d.yil DESC, d.sira_no DESC LIMIT 30",
            &[&desen],
        )
        .map_err(hata)?;
    Ok(rows
        .iter()
        .map(|r| {
            let borclu: String = r.get(5);
            json!({"id": r.get::<_, i64>(0), "birim_adi": r.get::<_, String>(1), "dosya_no": r.get::<_, String>(2),
                   "tur": r.get::<_, String>(3), "birim_id": r.get::<_, String>(4),
                   "borclu": borclu.chars().take(160).collect::<String>()})
        })
        .collect())
}

/// İş ayrıntısı için: işin belgeleri (işi görebilene; belge yetkisi iş yoluyla gelir).
pub fn gorev_belgeleri<C: GenericClient>(c: &mut C, gorev: i64) -> Result<Vec<Value>, String> {
    Ok(c
        .query(&format!("{LISTE_SECIM} JOIN gorev_belge gb ON gb.belge_id = b.id WHERE gb.gorev_id = $1 AND b.silindi IS NULL ORDER BY gb.olusturulma"), &[&gorev])
        .map_err(hata)?
        .iter()
        .map(belge_json)
        .collect())
}

/// Bir sha256'yı başka kayıt (iş eki ya da belge sürümü) kullanıyor mu? (is_veri::ek_sil diskten silmeden önce)
pub fn sha_kullaniliyor<C: GenericClient>(c: &mut C, sha: &str) -> Result<bool, String> {
    Ok(c
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM belge_surum bs JOIN belge b ON b.id = bs.belge_id WHERE bs.sha256 = $1 AND b.yol IS NULL)",
            &[&sha],
        )
        .map_err(hata)?
        .get(0))
}

#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    let bugun = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
    let amir = iv::kisi_bul(c, "amir").unwrap().expect("amir");
    let uye = iv::kisi_bul(c, "uye").unwrap().expect("uye");
    assert!(amir.yonetici && !uye.yonetici, "senaryo: amir yönetici, uye değil");
    // Dosya (büro veritabanı)
    let birim: i64 = c
        .query_one("INSERT INTO birim (birim_id, ad) VALUES ('9990001', 'Deneme 8. İcra Dairesi') ON CONFLICT (birim_id) DO UPDATE SET ad = excluded.ad RETURNING id", &[])
        .unwrap()
        .get(0);
    let dosya: i64 = c
        .query_one(
            "INSERT INTO dosya (birim_id, yargi_turu, yil, sira_no, dosya_no, tur_kod, tur) VALUES ($1, 1, 2026, 99901, '2026/99901', 35, 'İcra Dosyası') RETURNING id",
            &[&birim],
        )
        .unwrap()
        .get(0);
    // Yeni belge (üye): karttan dosya bulunur
    let kart = json!({"dosyaId": "X=", "dosyaNo": "2026/99901", "birimId": "9990001", "birimAdi": "Deneme 8. İcra Dairesi"});
    let r = kaydet(c, &uye, &Kayit { ad: "İtiraz/İptal.udf".into(), uyap_karti: Some(kart), ..Default::default() }, b"surum-1").unwrap();
    let id = r["id"].as_i64().unwrap();
    assert_eq!((r["surum"].as_i64(), r["yeni"].as_bool()), (Some(1), Some(true)));
    let g = getir(c, &uye, id).unwrap();
    assert_eq!(g["ad"], "İtiraz İptal", "ad temizlenir, uzantı atılır");
    assert_eq!(g["dosya_id"].as_i64(), Some(dosya), "karttan büro dosyası bulunur");
    assert_eq!(g["dosya_no"], "2026/99901");
    // Disk: yargı türü / birim / dosya klasörü, belge kaydedilince oluşur
    let klasor = "Hukuk/Deneme 8. İcra Dairesi/2026-99901";
    assert_eq!(g["klasor"], klasor);
    let dk = bd::kok().join("Hukuk").join("Deneme 8. İcra Dairesi").join("2026-99901");
    assert_eq!(std::fs::read(dk.join("İtiraz İptal.udf")).unwrap(), b"surum-1");
    // Yetki: başka üye göremez (yalnız oluşturan + yönetici); yönetici görür
    let baska = iv::kullanici_esitle(c, "baska@buro", "Başka", false, true).unwrap();
    let baska = Kisi { id: baska, yonetici: false };
    assert!(getir(c, &baska, id).is_err(), "ilgisiz kişi göremez");
    assert!(oku(c, &baska, id, None).is_err());
    assert!(liste(c, &baska, Some(dosya), None, "", "", true).unwrap().is_empty(), "listede de görünmez");
    assert_eq!(liste(c, &amir, Some(dosya), None, "", "", false).unwrap().len(), 1, "yönetici görür");
    assert_eq!(liste(c, &uye, None, None, "", "", false).unwrap().iter().filter(|b| b["id"].as_i64() == Some(id)).count(), 1, "benim belgelerim");
    assert_eq!(liste(c, &uye, None, None, "itiraz", "", true).unwrap().len(), 1, "arama");
    // Sürüm ve çakışma
    let r2 = kaydet(c, &uye, &Kayit { id: Some(id), surum: Some(1), ..Default::default() }, b"surum-2").unwrap();
    assert_eq!(r2["surum"].as_i64(), Some(2));
    let ayni = kaydet(c, &uye, &Kayit { id: Some(id), surum: Some(2), ..Default::default() }, b"surum-2").unwrap();
    assert_eq!(ayni["surum"].as_i64(), Some(2), "aynı içerik yeni sürüm açmaz");
    let e = kaydet(c, &amir, &Kayit { id: Some(id), surum: Some(1), ..Default::default() }, b"eski-uzerine").unwrap_err();
    assert!(e.starts_with("CAKISMA:"), "{e}");
    assert!(kaydet(c, &uye, &Kayit { id: Some(id), ..Default::default() }, b"x").is_err(), "sürüm no zorunlu");
    let (_, _, s, imz, v) = oku(c, &uye, id, None).unwrap();
    assert_eq!((s, imz, v.as_slice()), (2, false, &b"surum-2"[..]));
    assert_eq!(oku(c, &uye, id, Some(1)).unwrap().4, b"surum-1", "eski sürüm okunur");
    assert_eq!(std::fs::read(dk.join("İtiraz İptal.udf")).unwrap(), b"surum-2", "son sürüm belgenin adıyla");
    assert_eq!(std::fs::read(dk.join(".surumler").join(id.to_string()).join("1.udf")).unwrap(), b"surum-1", "eski sürüm .surumler'de");
    // Aynı klasörde aynı ad: "(2)"; ağaç ve klasör süzgeci; yeniden adlandırma; silinince klasör kalır (içinde belge var)
    let id2 = kaydet(c, &uye, &Kayit { ad: "İtiraz İPTAL".into(), dosya_id: Some(dosya), ..Default::default() }, b"ikinci").unwrap()["id"].as_i64().unwrap();
    assert_eq!(std::fs::read(dk.join("İtiraz İPTAL (2).udf")).unwrap(), b"ikinci", "büyük/küçük harf farkı aynı ad sayılır");
    let a = agac(c, &uye, false).unwrap();
    assert_eq!((a["sayi"].as_i64(), a["alt"][0]["ad"].as_str()), (Some(2), Some("Hukuk")));
    assert_eq!(a["alt"][0]["alt"][0]["alt"][0]["klasor"], klasor);
    assert_eq!(a["alt"][0]["alt"][0]["alt"][0]["sayi"].as_i64(), Some(2));
    assert!(agac(c, &baska, true).unwrap()["alt"].as_array().unwrap().is_empty(), "göremediği belge ağaçta da yok");
    assert_eq!(liste(c, &uye, None, None, "", "Hukuk", false).unwrap().len(), 2, "klasör süzgeci alt klasörleri de kapsar");
    assert!(liste(c, &uye, None, None, "", "Hukuk/Deneme 8", false).unwrap().is_empty(), "klasör adı tam eşleşir");
    assert_eq!(yeniden_adlandir(c, &uye, id2, "Cevap: Dilekçe").unwrap(), "Cevap Dilekçe");
    assert!(dk.join("Cevap Dilekçe.udf").exists() && !dk.join("İtiraz İPTAL (2).udf").exists());
    assert!(yeniden_adlandir(c, &baska, id2, "x").is_err(), "göremeyen adlandıramaz");
    sil(c, &uye, id2).unwrap();
    assert!(!dk.join("Cevap Dilekçe.udf").exists() && dk.join("İtiraz İptal.udf").exists());
    // İmzalı sürüm
    let r3 = kaydet(c, &uye, &Kayit { id: Some(id), surum: Some(2), imzali: true, notu: "e-imza".into(), ..Default::default() }, b"imzali").unwrap();
    assert_eq!(r3["surum"].as_i64(), Some(3));
    assert_eq!(getir(c, &uye, id).unwrap()["imzali"], true);
    // İşe ekleme → işin kişisi görür
    let y = iv::YeniGorev { baslik: "Dilekçe hazırla".into(), son_tarih: Some(bugun), sorumlu_id: Some(baska.id), ..Default::default() };
    let gid = iv::gorev_ac(c, Some(&amir), &y, bugun).unwrap();
    assert!(ise_ekle(c, &uye, id, gid).is_err(), "işte yetkisi olmayan belge ekleyemez");
    ise_ekle(c, &amir, id, gid).unwrap();
    assert!(getir(c, &baska, id).is_ok(), "bağlı işin sorumlusu görür");
    assert_eq!(gorev_belgeleri(c, gid).unwrap().len(), 1);
    assert_eq!(liste(c, &baska, None, Some(gid), "", "", false).unwrap().len(), 1);
    isten_cikar(c, &baska, id, gid).unwrap();
    assert!(getir(c, &baska, id).is_err(), "işten çıkınca göremez");
    // Onaya sun: yeni "Belge onayı" işi, onaylayıcı amir
    assert!(onaya_sun(c, &uye, id, None, Some(uye.id), "", bugun).is_err(), "kendine onaya sunamaz");
    let og = onaya_sun(c, &uye, id, None, Some(amir.id), "Kontrol eder misiniz?", bugun).unwrap();
    let oz = iv::gorev_ozu(c, og).unwrap().unwrap();
    assert_eq!((oz.durum.as_str(), oz.onay_gerekir, oz.sorumlu_id), ("onayda", true, Some(uye.id)));
    let (baslik, d_id): (String, Option<i64>) = c.query_one("SELECT baslik, dosya_id FROM gorev WHERE id = $1", &[&og]).map(|r| (r.get(0), r.get(1))).unwrap();
    assert_eq!((baslik.as_str(), d_id), ("Belge onayı: İtiraz İptal", Some(dosya)));
    // Hatırlatma: sunan → onaylayıcıya anında hatırlatıcı; saatte bir; onaylayan kendine gönderemez
    assert_eq!(iv::onay_hatirlat(c, &uye, og, "Acil").unwrap(), vec!["Av. Amir".to_string()]);
    assert!(iv::onay_hatirlat(c, &uye, og, "").is_err(), "saatte bir");
    assert!(iv::onay_hatirlat(c, &amir, og, "").is_err(), "onaylayıcı kendine hatırlatamaz");
    let (hb, hn): (String, String) = c
        .query_one("SELECT baslik, notu FROM hatirlatici WHERE gorev_id = $1 AND kullanici_id = $2", &[&og, &amir.id])
        .map(|r| (r.get(0), r.get(1)))
        .unwrap();
    assert!(hb.starts_with("Belge onayı") && hn.contains("Acil"), "{hb} / {hn}");
    assert_eq!(iv::hatirlaticilar(c, &amir, Some(og)).unwrap().len(), 1, "onaylayan bekleyen hatırlatıcıyı görür");
    // Onaylayan onaylar (mevcut durum akışı)
    iv::durum_degistir(c, &amir, og, "tamamla", "", "", bugun).unwrap();
    assert!(iv::onay_hatirlat(c, &uye, og, "").is_err(), "onayda olmayan iş");
    assert_eq!(iv::gorev_ozu(c, og).unwrap().unwrap().durum, "tamamlandi");
    // Var olan işe onaya sun
    let gid2 = iv::gorev_ac(c, Some(&uye), &iv::YeniGorev { baslik: "İtiraz dilekçesi".into(), son_tarih: Some(bugun), sorumlu_id: Some(uye.id), ..Default::default() }, bugun).unwrap();
    iv::kisi_ekle(c, &uye, gid2, amir.id, "onaylayici").unwrap();
    assert_eq!(onaya_sun(c, &uye, id, Some(gid2), None, "Bakar mısınız", bugun).unwrap(), gid2);
    assert_eq!(iv::gorev_ozu(c, gid2).unwrap().unwrap().durum, "onayda");
    // Düzeltme iste: onaylayan geri çevirir
    iv::durum_degistir(c, &amir, gid2, "acik", "", "Tarih eksik", bugun).unwrap();
    assert_eq!(iv::gorev_ozu(c, gid2).unwrap().unwrap().durum, "acik");
    // Gönderim, dosya arama, dosya bağı
    gonderildi(c, &uye, id, "ICR_ADR_TLB", "Adres Araştırması Talebi", "Evrak gönderildi").unwrap();
    assert_eq!(getir(c, &uye, id).unwrap()["gonderim"]["tur_adi"], "Adres Araştırması Talebi");
    assert!(dosya_ara(c, "99901").unwrap().iter().any(|d| d["id"].as_i64() == Some(dosya)));
    dosya_bagla(c, &uye, id, None).unwrap();
    let g = getir(c, &uye, id).unwrap();
    assert!(g["dosya_id"].is_null());
    // Dosyadan ayrılınca Dosyasız/<kullanıcı> klasörüne taşınır, eski sürümleriyle; boşalan klasörler silinir
    assert_eq!(g["klasor"], "Dosyasız/uye");
    let dz = bd::kok().join("Dosyasız").join("uye");
    assert_eq!(std::fs::read(dz.join("İtiraz İptal.udf")).unwrap(), b"imzali");
    assert!(dz.join(".surumler").join(id.to_string()).join("2.udf").exists());
    assert!(!bd::kok().join("Hukuk").exists(), "boşalan yargı türü klasörü silinir");
    assert_eq!(oku(c, &uye, id, Some(1)).unwrap().4, b"surum-1", "taşınan eski sürüm okunur");
    // Klasördeki belge eski içerik adresli depoyu kullanmaz (iş eki silinirken engel olmaz)
    use sha2::{Digest, Sha256};
    let sha = hex::encode(Sha256::digest(b"surum-1"));
    assert!(!sha_kullaniliyor(c, &sha).unwrap());
    // Eski depodaki belge (yol yok) açılışta klasöre taşınır, eski kopya silinir
    let eski_sha = hex::encode(Sha256::digest(b"eski-belge"));
    let ekler = crate::is_aktar::ekler_dizini();
    std::fs::create_dir_all(&ekler).unwrap();
    std::fs::write(ekler.join(&eski_sha), b"eski-belge").unwrap();
    let eid: i64 = c
        .query_one("INSERT INTO belge (ad, dosya_id, olusturan_id, son_surum) VALUES ('Eski Dilekçe', $1, $2, 1) RETURNING id", &[&dosya, &uye.id])
        .unwrap()
        .get(0);
    c.execute("INSERT INTO belge_surum (belge_id, surum, sha256, boyut) VALUES ($1, 1, $2, 10)", &[&eid, &eski_sha]).unwrap();
    assert!(sha_kullaniliyor(c, &eski_sha).unwrap(), "taşınmamış belge eski depoyu kullanır");
    let (n, h) = eski_belgeleri_yerlestir(c).unwrap();
    assert_eq!((n, h.len()), (1, 0));
    assert_eq!(std::fs::read(dk.join("Eski Dilekçe.udf")).unwrap(), b"eski-belge");
    assert!(!ekler.join(&eski_sha).exists(), "eski depodaki kopya silinir");
    assert_eq!(oku(c, &uye, eid, None).unwrap().4, b"eski-belge");
    sil(c, &uye, eid).unwrap();
    assert!(!bd::kok().join("Hukuk").exists());
    // Silme
    assert!(sil(c, &baska, id).is_err());
    assert!(sil(c, &uye, id).is_err(), "UYAP'a gönderilmiş belgeyi yalnız yönetici siler");
    sil(c, &amir, id).unwrap();
    assert!(getir(c, &uye, id).is_err(), "silinen belge görünmez");
    let kalan: i64 = c.query_one("SELECT (SELECT count(*) FROM belge WHERE id = $1) + (SELECT count(*) FROM belge_surum WHERE belge_id = $1)", &[&id]).unwrap().get(0);
    assert_eq!(kalan, 0, "kalıcı silme: belge ve sürümleri gider");
    assert!(!dz.exists(), "silinen belgenin dosyaları ve boşalan klasörü gider");
    c.batch_execute(&format!(
        "DELETE FROM belge WHERE id = {id}; DELETE FROM gorev WHERE id IN ({gid}, {og}, {gid2}); DELETE FROM dosya WHERE id = {dosya};
         DELETE FROM kullanici WHERE hesap_adi = 'baska@buro';"
    ))
    .unwrap();
}
