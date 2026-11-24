//! İş Listesi uçları (plan L3, 16.11.2026): `/__cu__/veri/is/…`, büro veritabanından (is_veri.rs).
//!
//! Buluttaki İş Listesi'nin yerini alır; veri ofis bilgisayarında kalır. Her istek bir KİŞİ adına
//! çalışır (kimlik.rs: bu bilgisayar / tünel bileti / yerel ağ çerezi); kimliği bilinmeyen istek
//! reddedilir. Yetki kuralları veri katmanındadır (gizli iş yalnız sahibine, onayı yalnız yönetici /
//! onaylayıcı / işi başkasına açan). Başka siteden gelen yazma isteği reddedilir.
//! Yanıt: `{"ok": true, "veri": …}` ya da `{"ok": false, "error": "…"}`.
//!
//!   GET  ben                  kişi, sayaçlar, kişiler, etiketler, şablonlar, sabit listeler
//!   GET  liste                ?gorunum=bugun|onay|tum&ara=&durum=&tur=&sorumlu=&etiket=&dosya_no=&birim_adi=&ilk=&son=
//!   GET  gorev                ?id=  — ayrıntı (alt işler, notlar, kişiler, ertelemeler, geçmiş, yetki)
//!   GET  takvim               ?ilk=&son=&herkes=1
//!   GET  ozet · sablonlar · surecler · etiketler · kisiler · tekrarlar
//!   GET  evrak_sablon         ?ad=  — belge adına uyan şablonlar (İş ata penceresi)
//!   POST ac                   yeni iş (is_veri::YeniGorev alanları)
//!   POST guncelle             {id, …değişen alanlar}
//!   POST durum                {id, durum, sonuc, not} → {durum, oneri}
//!   POST oneriden_ac          {onceki, sablon_id, son_tarih, son_saat, sorumlu_id}
//!   POST ertele               {id, yeni_tarih, yeni_saat, gerekce} → {uygulandi}
//!   POST erteleme_karar       {id, kabul, notu}
//!   POST not                  {id, metin}
//!   POST kisi_ekle / kisi_cikar {id, kullanici, rol}
//!   POST sablon               şablon ekle / düzenle
//!   POST tekrar · tekrar_durdur {id, aktif}
//!   GET  hatirlaticilar       ?gorev=  — kişinin bekleyen hatırlatıcıları (plan L6)
//!   POST hatirlatici          {gorev_id, zaman: "YYYY-AA-GGTSS:DD", notu} → {id}  (kişi kendine kurar)
//!   POST hatirlatici_sil · hatirlatici_tamam {id} · hatirlatici_ertele {id, dakika}
//!   POST onay_hatirlat        {gorev, notu} → {kime: [ad]}  (onayda işin onaylayıcısına anında hatırlatma)
//!
//!
//! Belgeler (belge_veri.rs, 24.11.2026 — Armonia Yazı'nın büro deposu):
//!   GET  belge_liste          ?dosya=&gorev=&ara=&klasor=&tumu=0|1   GET belge ?id=   GET dosya_ara ?ara=
//!   GET  belge_agac           ?tumu=0|1  (yargı türü → birim → dosya klasörleri + sayılar)
//!   POST belge_adlandir {id, ad} → {ad}
//!   GET  belge_indir          ?id=&surum=  (ham bayt)
//!   POST belge_kaydet         ?id=&surum=&ad64=&bicim=&dosya=&gorev=&imzali=1&not64=&kart64=  (gövde = belge)
//!   POST belge_dosya {id, dosya_id} · belge_ise_ekle / belge_isten_cikar {belge, gorev}
//!   POST belge_onaya_sun {belge, gorev?, onaylayici?, not} → {gorev} · belge_gonderildi {id, tur, tur_adi, mesaj}
//!   POST belge_sil {id}
//!
//! `ozet` (sayfa dakikada bir yoklar): sayaçlar + `yeni`/`yeni_isler` (başkasının açtığı, bakılmamış işler)
//! + `hatirlatma`/`hatirlatma_sayi` (zamanı gelmiş hatırlatıcılar). Hepsi ofiste kalır; buluta gitmez.

use crate::belge_veri as bv;
use crate::is_veri::{self as iv, Kisi};
use chrono::NaiveDate;
use postgres::GenericClient;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const ON_EK: &str = "__cu__/veri/is/";

pub fn is_path(path: &str) -> bool {
    path.trim_start_matches('/').starts_with(ON_EK)
}

type Yanit = (u16, HashMap<String, String>, Vec<u8>);

fn yanit(durum: u16, govde: Value) -> Yanit {
    let mut h = HashMap::new();
    h.insert("Content-Type".to_string(), "application/json; charset=utf-8".to_string());
    h.insert("Cache-Control".to_string(), "no-store".to_string());
    (durum, h, serde_json::to_vec(&govde).unwrap_or_else(|_| b"{}".to_vec()))
}

fn hata(e: postgres::Error) -> String {
    match e.as_db_error() {
        Some(d) => format!("veritabanı: {}", d.message()),
        None => format!("veritabanı: {e}"),
    }
}

fn sayi(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}
fn tam(g: &Value, k: &str) -> Result<i64, String> {
    sayi(&g[k]).ok_or_else(|| format!("'{k}' eksik."))
}
fn metin(g: &Value, k: &str) -> String {
    g[k].as_str().unwrap_or("").trim().to_string()
}
fn tarih(s: &str, ad: &str) -> Result<Option<NaiveDate>, String> {
    iv::tarih_coz(s).map_err(|_| format!("{ad} anlaşılamadı."))
}
/// "2026-11-16T09:30", "2026-11-16 09:30" (saniyeli de olur) → yerel saat.
fn zaman_coz(s: &str) -> Result<chrono::NaiveDateTime, String> {
    let s = s.trim().replace('T', " ");
    ["%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S"]
        .iter()
        .find_map(|b| chrono::NaiveDateTime::parse_from_str(&s, b).ok())
        .ok_or_else(|| "Hatırlatma zamanı anlaşılamadı.".to_string())
}

/// Hesap adından büro kullanıcısı. Hiç görülmemiş hesap üye olarak eklenir (ad, buluttan eşitlenince
/// düzelir); pasifleştirilmiş hesap reddedilir.
pub fn kisi_getir<C: GenericClient>(c: &mut C, hesap: &str) -> Result<Kisi, String> {
    if let Some(k) = iv::kisi_bul(c, hesap)? {
        return Ok(k);
    }
    let ad = hesap.split('@').next().unwrap_or(hesap);
    c.execute(
        "INSERT INTO kullanici (hesap_adi, ad, rol) VALUES ($1, $2, 'uye') ON CONFLICT (hesap_adi) DO NOTHING",
        &[&hesap.trim(), &ad],
    )
    .map_err(hata)?;
    iv::kisi_bul(c, hesap)?.ok_or_else(|| "Bu hesap büroda pasif; İş Listesi'ni kullanamaz.".to_string())
}

fn sabitler() -> Value {
    let liste = |l: &[(&str, &str)]| l.iter().map(|(k, a)| json!({"kod": k, "ad": a})).collect::<Vec<_>>();
    json!({
        "turler": liste(iv::TURLER), "durumlar": liste(iv::DURUMLAR), "sonuclar": liste(iv::SONUCLAR),
        "sure_turleri": liste(iv::SURE_TURLERI), "tekrar_kurallari": liste(iv::TEKRAR_KURALLARI),
    })
}

fn dagit<C: GenericClient>(
    c: &mut C,
    hesap: &str,
    yontem: &str,
    uc: &str,
    q: &HashMap<String, String>,
    g: &Value,
    bugun: NaiveDate,
) -> Result<Option<Value>, String> {
    let k = kisi_getir(c, hesap)?;
    let qs = |a: &str| q.get(a).map(|s| s.trim().to_string()).unwrap_or_default();
    let qi = |a: &str| q.get(a).and_then(|s| s.trim().parse::<i64>().ok());
    Ok(Some(match (yontem, uc) {
        ("GET", "ben") => {
            // Rutinlerin bugünkü örnekleri burada üretilir (sayfa açılışı); her kural en çok bir açık örnek.
            iv::tekrarlari_uret(c, bugun)?;
            let r = c
                .query_one("SELECT id, hesap_adi, ad, rol FROM kullanici WHERE id = $1", &[&k.id])
                .map_err(hata)?;
            json!({
                "ben": {"id": r.get::<_, i64>(0), "hesap": r.get::<_, String>(1), "ad": r.get::<_, String>(2),
                        "yonetici": r.get::<_, String>(3) == "yonetici"},
                "bugun": bugun.to_string(),
                "ozet": iv::ozet(c, &k, bugun)?,
                "kisiler": iv::kullanicilar(c)?,
                "etiketler": iv::etiketler(c, &k)?,
                "sablonlar": iv::sablonlar(c)?,
                "sabitler": sabitler(),
            })
        }
        ("GET", "ozet") => iv::ozet(c, &k, bugun)?,
        ("GET", "liste") => {
            let durumlar: Vec<String> =
                qs("durum").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let s = iv::Suzgec {
                gorunum: qs("gorunum"),
                sorumlu_id: qi("sorumlu"),
                durumlar,
                tur: qs("tur"),
                ara: qs("ara"),
                dosya_id: qi("dosya"),
                dosya_no: qs("dosya_no"),
                birim_adi: qs("birim_adi"),
                etiket_id: qi("etiket"),
                ilk: tarih(&qs("ilk"), "Başlangıç tarihi")?,
                son: tarih(&qs("son"), "Bitiş tarihi")?,
                sinir: qi("sinir").unwrap_or(300),
            };
            json!(iv::gorevler(c, &k, &s, bugun)?)
        }
        ("GET", "gorev") => {
            let id = qi("id").ok_or("'id' eksik.")?;
            iv::gorev_ayrinti(c, &k, id, bugun)?.ok_or("İş bulunamadı.")?
        }
        ("GET", "takvim") => {
            let ilk = tarih(&qs("ilk"), "Başlangıç tarihi")?.unwrap_or(bugun);
            let son = tarih(&qs("son"), "Bitiş tarihi")?.unwrap_or(ilk + chrono::Duration::days(41));
            if (son - ilk).num_days() > 120 {
                return Err("Takvim en çok 120 gün gösterir.".into());
            }
            iv::takvim(c, &k, ilk, son, qs("herkes") == "1", bugun)?
        }
        ("GET", "sablonlar") => json!(iv::sablonlar(c)?),
        ("GET", "surecler") => json!(iv::surecler(c)?),
        ("GET", "etiketler") => json!(iv::etiketler(c, &k)?),
        ("GET", "kisiler") => json!(iv::kullanicilar(c)?),
        ("GET", "tekrarlar") => json!(iv::tekrar_kurallari(c, &k)?),
        ("GET", "evrak_sablon") => json!(iv::evraga_uyan_sablonlar(c, &qs("ad"))?),
        ("GET", "hatirlaticilar") => json!(iv::hatirlaticilar(c, &k, qi("gorev"))?),
        ("POST", "hatirlatici") => {
            let simdi = chrono::Local::now().naive_local();
            json!({"id": iv::hatirlatici_kur(c, &k, sayi(&g["gorev_id"]), zaman_coz(&metin(g, "zaman"))?, &metin(g, "notu"), simdi)?})
        }
        ("POST", "onay_hatirlat") => json!({"kime": iv::onay_hatirlat(c, &k, tam(g, "gorev")?, &metin(g, "notu"))?}),
        ("POST", "hatirlatici_sil") => {
            iv::hatirlatici_sil(c, &k, tam(g, "id")?)?;
            Value::Null
        }
        ("POST", "hatirlatici_tamam") => {
            iv::hatirlatici_goruldu(c, &k, tam(g, "id")?)?;
            Value::Null
        }
        ("POST", "hatirlatici_ertele") => {
            iv::hatirlatici_ertele(c, &k, tam(g, "id")?, sayi(&g["dakika"]).unwrap_or(60))?;
            Value::Null
        }
        ("POST", "ac") => {
            let mut y = iv::YeniGorev::jsondan(g)?;
            // Dosya kartından açılan iş: UYAP birim kimliği + dosya no ile icra şemasındaki dosyaya bağlanır
            // (dosya veritabanında yoksa künye metinleriyle bağsız kalır).
            let birim_uyap = metin(g, "birim_uyap_id");
            if y.dosya_id.is_none() && !birim_uyap.is_empty() && !y.dosya_no.is_empty() {
                y.dosya_id = c
                    .query_opt(
                        "SELECT d.id FROM dosya d JOIN birim b ON b.id = d.birim_id WHERE b.birim_id = $1 AND d.dosya_no = $2 LIMIT 1",
                        &[&birim_uyap, &y.dosya_no],
                    )
                    .map_err(hata)?
                    .map(|r| r.get(0));
            }
            json!({"id": iv::gorev_ac(c, Some(&k), &y, bugun)?})
        }
        ("POST", "guncelle") => {
            iv::gorev_guncelle(c, &k, tam(g, "id")?, g)?;
            Value::Null
        }
        ("POST", "durum") => {
            let s = iv::durum_degistir(c, &k, tam(g, "id")?, &metin(g, "durum"), &metin(g, "sonuc"), &metin(g, "not"), bugun)?;
            json!({"durum": s.durum, "oneri": s.oneri.map(|o| o.json())})
        }
        ("POST", "oneriden_ac") => {
            let son_tarih = tarih(&metin(g, "son_tarih"), "Son tarih")?.ok_or("Son tarih seçilmeli.")?;
            let son_saat = iv::saat_coz(&metin(g, "son_saat"))?;
            let id = iv::oneriden_ac(c, &k, tam(g, "onceki")?, tam(g, "sablon_id")?, son_tarih, son_saat, sayi(&g["sorumlu_id"]), bugun)?;
            json!({"id": id})
        }
        ("POST", "ertele") => {
            let yeni = tarih(&metin(g, "yeni_tarih"), "Yeni tarih")?.ok_or("Yeni tarih seçilmeli.")?;
            let saat = iv::saat_coz(&metin(g, "yeni_saat"))?;
            json!({"uygulandi": iv::ertele(c, &k, tam(g, "id")?, yeni, saat, &metin(g, "gerekce"))?})
        }
        ("POST", "erteleme_karar") => {
            iv::erteleme_karar(c, &k, tam(g, "id")?, g["kabul"].as_bool().unwrap_or(false), &metin(g, "notu"))?;
            Value::Null
        }
        ("POST", "not") => json!({"id": iv::not_ekle(c, &k, tam(g, "id")?, &metin(g, "metin"))?}),
        ("POST", "ek_sil") => {
            iv::ek_sil(c, &k, tam(g, "id")?)?;
            Value::Null
        }
        ("POST", "kisi_ekle") => {
            iv::kisi_ekle(c, &k, tam(g, "id")?, tam(g, "kullanici")?, &metin(g, "rol"))?;
            Value::Null
        }
        ("POST", "kisi_cikar") => {
            iv::kisi_cikar(c, &k, tam(g, "id")?, tam(g, "kullanici")?, &metin(g, "rol"))?;
            Value::Null
        }
        ("POST", "sablon") => json!({"id": iv::sablon_kaydet(c, &k, g)?}),
        ("POST", "tekrar") => json!({"id": iv::tekrar_kurali_ekle(c, &k, g)?}),
        // Belgeler
        ("GET", "belge_liste") => {
            // UYAP kart verisi (kart64) verilmişse dosya ondan bulunur; büro veritabanında yoksa liste boş
            let kart = serde_json::from_str::<Value>(&b64_metin(q, "kart64")).ok().filter(|v| v.is_object());
            let dosya = match (qi("dosya"), kart) {
                (Some(d), _) => Some(d),
                (None, Some(kv)) => match bv::karttan_dosya(c, &kv)? {
                    Some(d) => Some(d),
                    None => return Ok(Some(json!([]))),
                },
                (None, None) => None,
            };
            // tumu=1 bütün görebildikleri, 0 "Belgelerim"; verilmezse eski davranış (yönetici aramıyorsa kendi belgeleri)
            let tumu = match qs("tumu").as_str() {
                "1" => true,
                "0" => false,
                _ => !k.yonetici || !qs("ara").trim().is_empty(),
            };
            json!(bv::liste(c, &k, dosya, qi("gorev"), &qs("ara"), &qs("klasor"), tumu)?)
        }
        ("GET", "belge_agac") => bv::agac(c, &k, qs("tumu") == "1")?,
        ("POST", "belge_adlandir") => json!({"ad": bv::yeniden_adlandir(c, &k, tam(g, "id")?, &metin(g, "ad"))?}),
        ("GET", "kart_dosya") => {
            let kart = serde_json::from_str::<Value>(&b64_metin(q, "kart64")).ok().filter(|v| v.is_object());
            json!({"dosya_id": match kart { Some(kv) => bv::karttan_dosya(c, &kv)?, None => None }})
        }
        ("GET", "belge") => bv::getir(c, &k, qi("id").ok_or("'id' eksik.")?)?,
        ("GET", "dosya_ara") => json!(bv::dosya_ara(c, &qs("ara"))?),
        ("POST", "belge_dosya") => {
            bv::dosya_bagla(c, &k, tam(g, "id")?, sayi(&g["dosya_id"]))?;
            json!({})
        }
        ("POST", "belge_ise_ekle") => {
            bv::ise_ekle(c, &k, tam(g, "belge")?, tam(g, "gorev")?)?;
            json!({})
        }
        ("POST", "belge_isten_cikar") => {
            bv::isten_cikar(c, &k, tam(g, "belge")?, tam(g, "gorev")?)?;
            json!({})
        }
        ("POST", "belge_onaya_sun") => {
            json!({"gorev": bv::onaya_sun(c, &k, tam(g, "belge")?, sayi(&g["gorev"]), sayi(&g["onaylayici"]), &metin(g, "not"), bugun)?})
        }
        ("POST", "belge_gonderildi") => {
            bv::gonderildi(c, &k, tam(g, "id")?, &metin(g, "tur"), &metin(g, "tur_adi"), &metin(g, "mesaj"))?;
            json!({})
        }
        ("POST", "belge_sil") => {
            bv::sil(c, &k, tam(g, "id")?)?;
            json!({})
        }
        ("POST", "tekrar_durdur") => {
            iv::tekrar_kurali_durdur(c, &k, tam(g, "id")?, g["aktif"].as_bool().unwrap_or(false))?;
            Value::Null
        }
        _ => return Ok(None),
    }))
}

/// `/__cu__/veri/is/…` isteği. `hesap`: isteğin sahibi (kimlik.rs); yoksa reddedilir.
pub async fn handle(method: &str, path: &str, query: &str, body: &[u8], capraz_site: bool, hesap: Option<&str>) -> Yanit {
    let yontem = method.trim().to_uppercase();
    let uc = path.trim_start_matches('/').trim_start_matches(ON_EK).trim_matches('/').to_string();
    if yontem != "GET" && capraz_site {
        return yanit(403, json!({"ok": false, "error": "Bu istek başka bir siteden geldi; reddedildi."}));
    }
    let Some(hesap) = hesap.map(str::trim).filter(|h| !h.is_empty()).map(String::from) else {
        return yanit(
            401,
            json!({"ok": false, "kod": "KIMLIK",
                   "error": "Kim olduğunuz doğrulanamadı. Yerel ağdan bağlanıyorsanız kendi Çoklu UYAP hesabınızla yeniden giriş yapın."}),
        );
    };
    if !crate::veritabani::hazir() {
        return yanit(503, json!({"ok": false, "kod": "E-402", "error": "Büro veritabanı hazır değil."}));
    }
    let q: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    // Ekler: ham gövde (yükleme) ve ham yanıt (indirme); JSON değil.
    if (yontem == "POST" && uc == "ek_yukle") || (yontem == "GET" && uc == "ek_indir") {
        return ek_ucu(&yontem, &q, body.to_vec(), hesap).await;
    }
    if (yontem == "POST" && uc == "belge_kaydet") || (yontem == "GET" && uc == "belge_indir") {
        return belge_ucu(&yontem, &q, body.to_vec(), hesap).await;
    }
    let g: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => return yanit(400, json!({"ok": false, "error": "İstek gövdesi JSON değil."})),
        }
    };
    let sonuc = tokio::task::spawn_blocking(move || {
        let mut c = crate::veritabani::baglanti()?;
        let bugun = chrono::Local::now().date_naive();
        dagit(&mut *c, &hesap, &yontem, &uc, &q, &g, bugun)
    })
    .await;
    match sonuc {
        Ok(Ok(Some(veri))) => yanit(200, json!({"ok": true, "veri": veri})),
        Ok(Ok(None)) => yanit(404, json!({"ok": false, "error": "Böyle bir uç yok."})),
        Ok(Err(e)) => {
            let sunucu_hatasi = e.starts_with("veritabanı") || e.contains("büro veritabanı");
            yanit(if sunucu_hatasi { 500 } else { 400 }, json!({"ok": false, "error": e}))
        }
        Err(e) => yanit(500, json!({"ok": false, "error": format!("İşlem tamamlanamadı: {e}")})),
    }
}

/// `POST ek_yukle?gorev=<id>&ad64=<base64url ad>&mime=<tür>` (gövde = dosya) ve `GET ek_indir?id=<ek>`.
/// Ad Base64 ile gelir: adres satırından geçen Türkçe harfler bozulabiliyordu (paylaşımda görüldü, 17.11.2026).
async fn ek_ucu(yontem: &str, q: &HashMap<String, String>, body: Vec<u8>, hesap: String) -> Yanit {
    use base64::Engine as _;
    let sayi_q = |k: &str| q.get(k).and_then(|s| s.trim().parse::<i64>().ok());
    let yukle = yontem == "POST";
    let id = if yukle { sayi_q("gorev") } else { sayi_q("id") };
    let Some(id) = id else { return yanit(400, json!({"ok": false, "error": "Kimlik eksik."})) };
    let ad = q
        .get("ad64")
        .and_then(|s| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim().trim_end_matches('=')).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();
    let mime = q.get("mime").cloned().unwrap_or_default();
    let sonuc = tokio::task::spawn_blocking(move || -> Result<Result<i64, (String, String, Vec<u8>)>, String> {
        let mut c = crate::veritabani::baglanti()?;
        let k = kisi_getir(&mut *c, &hesap)?;
        if yukle {
            iv::ek_ekle(&mut *c, &k, id, &ad, &mime, &body).map(Ok)
        } else {
            iv::ek_oku(&mut *c, &k, id).map(Err)
        }
    })
    .await;
    match sonuc {
        Ok(Ok(Ok(eid))) => yanit(200, json!({"ok": true, "veri": {"id": eid}})),
        Ok(Ok(Err((ad, mime, veri)))) => {
            let mut h = HashMap::new();
            let tur = if mime.is_empty() || mime.chars().any(|c| c.is_control()) { "application/octet-stream".to_string() } else { mime };
            h.insert("Content-Type".to_string(), tur);
            h.insert("Cache-Control".to_string(), "no-store".to_string());
            h.insert("X-Content-Type-Options".to_string(), "nosniff".to_string());
            let kodlu: String = url::form_urlencoded::byte_serialize(ad.as_bytes()).collect::<String>().replace('+', "%20");
            h.insert("Content-Disposition".to_string(), format!("attachment; filename=\"ek\"; filename*=UTF-8''{kodlu}"));
            (200, h, veri)
        }
        Ok(Err(e)) => {
            let sunucu = e.starts_with("veritabanı");
            yanit(if sunucu { 500 } else { 400 }, json!({"ok": false, "error": e}))
        }
        Err(e) => yanit(500, json!({"ok": false, "error": format!("İşlem tamamlanamadı: {e}")})),
    }
}

fn b64_metin(q: &HashMap<String, String>, k: &str) -> String {
    use base64::Engine as _;
    q.get(k)
        .and_then(|s| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim().trim_end_matches('=')).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default()
}

/// Belge kaydı (ham gövde) ve indirme (ham yanıt). Ad, not ve UYAP kart verisi Base64 ile gelir (Türkçe harfler).
/// Kaydetme yanıtı JSON: `{id, surum, yeni}`; çakışmada 409 + `kod: "CAKISMA"`.
async fn belge_ucu(yontem: &str, q: &HashMap<String, String>, body: Vec<u8>, hesap: String) -> Yanit {
    let qi = |k: &str| q.get(k).and_then(|s| s.trim().parse::<i64>().ok());
    let kaydet = yontem == "POST";
    let kayit = bv::Kayit {
        id: qi("id"),
        surum: qi("surum").map(|n| n as i32),
        ad: b64_metin(q, "ad64"),
        bicim: q.get("bicim").cloned().unwrap_or_default(),
        dosya_id: qi("dosya"),
        uyap_karti: serde_json::from_str::<Value>(&b64_metin(q, "kart64")).ok().filter(|v| v.is_object()),
        imzali: q.get("imzali").map(|s| s == "1" || s == "true").unwrap_or(false),
        notu: b64_metin(q, "not64"),
        gorev_id: qi("gorev"),
    };
    let (id, surum) = (qi("id"), qi("surum").map(|n| n as i32));
    let sonuc = tokio::task::spawn_blocking(move || -> Result<Result<Value, (String, String, i32, bool, Vec<u8>)>, String> {
        let mut c = crate::veritabani::baglanti()?;
        let k = kisi_getir(&mut *c, &hesap)?;
        if kaydet {
            bv::kaydet(&mut *c, &k, &kayit, &body).map(Ok)
        } else {
            bv::oku(&mut *c, &k, id.ok_or("Kimlik eksik.")?, surum).map(Err)
        }
    })
    .await;
    match sonuc {
        Ok(Ok(Ok(v))) => yanit(200, json!({"ok": true, "veri": v})),
        Ok(Ok(Err((ad, bicim, surum, imzali, veri)))) => {
            let mut h = HashMap::new();
            h.insert("Content-Type".to_string(), "application/octet-stream".to_string());
            h.insert("Cache-Control".to_string(), "no-store".to_string());
            h.insert("X-Content-Type-Options".to_string(), "nosniff".to_string());
            h.insert("X-Belge-Surum".to_string(), surum.to_string());
            h.insert("X-Belge-Imzali".to_string(), if imzali { "1" } else { "0" }.to_string());
            let tam_ad = format!("{ad}.{bicim}");
            let kodlu: String = url::form_urlencoded::byte_serialize(tam_ad.as_bytes()).collect::<String>().replace('+', "%20");
            h.insert("Content-Disposition".to_string(), format!("attachment; filename=\"belge.{bicim}\"; filename*=UTF-8''{kodlu}"));
            (200, h, veri)
        }
        Ok(Err(e)) if e.starts_with("CAKISMA:") => {
            yanit(409, json!({"ok": false, "kod": "CAKISMA", "error": e.trim_start_matches("CAKISMA:").trim()}))
        }
        Ok(Err(e)) => {
            let sunucu = e.starts_with("veritabanı");
            yanit(if sunucu { 500 } else { 400 }, json!({"ok": false, "error": e}))
        }
        Err(e) => yanit(500, json!({"ok": false, "error": format!("İşlem tamamlanamadı: {e}")})),
    }
}

/// Büro üyelerini buluttaki İş Listesi hesaplarından (yalnız ad + rol) eşitler. Ofis programı paylaşımı
/// başlatınca çağrılır; yönetici değilse yalnız kendi hesabı eklenir.
pub async fn uyeleri_esitle() {
    let (ben, uyeler) = match crate::is_listesi::uye_listesi().await {
        Ok(x) => x,
        Err(e) => {
            println!("[İŞ LİSTESİ] Büro üyeleri eşitlenemedi: {e}");
            return;
        }
    };
    if let Some(h) = ben.as_ref().and_then(|b| b["account_username"].as_str()) {
        crate::kimlik::ofis_hesabi_ayarla(h);
    }
    let sonuc = tokio::task::spawn_blocking(move || -> Result<usize, String> {
        for _ in 0..30 {
            if crate::veritabani::hazir() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        let mut c = crate::veritabani::baglanti()?;
        let mut n = 0;
        for u in uyeler.iter().chain(ben.iter()) {
            let Some(h) = u["account_username"].as_str().filter(|h| !h.is_empty()) else { continue };
            iv::kullanici_esitle(&mut *c, h, u["display_name"].as_str().unwrap_or(""), u["role"].as_str() == Some("admin"),
                                 u["active"].as_bool().unwrap_or(true))?;
            n += 1;
        }
        Ok(n)
    })
    .await;
    match sonuc {
        Ok(Ok(n)) => println!("[İŞ LİSTESİ] {n} büro üyesi eşitlendi."),
        Ok(Err(e)) => println!("[İŞ LİSTESİ] Büro üyeleri yazılamadı: {e}"),
        Err(e) => println!("[İŞ LİSTESİ] Büro üyeleri yazılamadı: {e}"),
    }
}

// ── Test (gömülü PostgreSQL uçtan uca: veritabani::tests::uctan_uca) ────────────────────────────

#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    let bugun = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let bos = HashMap::new();
    let al = |c: &mut postgres::Client, h: &str, uc: &str, q: &[(&str, &str)]| -> Result<Value, String> {
        let q: HashMap<String, String> = q.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        dagit(c, h, "GET", uc, &q, &json!({}), bugun).map(|v| v.expect("uç"))
    };
    let gonder = |c: &mut postgres::Client, h: &str, uc: &str, g: Value| -> Result<Value, String> {
        dagit(c, h, "POST", uc, &HashMap::new(), &g, bugun).map(|v| v.expect("uç"))
    };
    iv::kullanici_esitle(c, "mudur@api", "Müdür", true, true).unwrap();
    // Görünen ad ofis adıysa kullanıcı adı yazılır
    let zid = iv::kullanici_esitle(c, "zeynel@api", "API", true, true).unwrap();
    let zad: String = c.query_one("SELECT ad FROM kullanici WHERE id = $1", &[&zid]).unwrap().get(0);
    assert_eq!(zad, "zeynel");
    // Görülmemiş hesap üye olarak eklenir
    let ben = al(c, "katip@api", "ben", &[]).unwrap();
    assert_eq!((ben["ben"]["ad"].as_str(), ben["ben"]["yonetici"].as_bool()), (Some("katip"), Some(false)));
    assert!(ben["sabitler"]["turler"].as_array().unwrap().len() >= 5);
    let katip = ben["ben"]["id"].as_i64().unwrap();
    // Müdür katibe onaylı iş açar
    let id = gonder(c, "mudur@api", "ac", json!({"baslik": "API deneme işi", "son_tarih": "2026-11-16", "sorumlu_id": katip,
                                                 "onay_gerekir": true})).unwrap()["id"].as_i64().unwrap();
    let liste = al(c, "katip@api", "liste", &[("gorunum", "bugun")]).unwrap();
    assert!(liste.as_array().unwrap().iter().any(|g| g["id"] == id), "katibin bugünkü işlerinde");
    // Katip tamamlayınca onaya gider; müdür onaylar
    assert_eq!(gonder(c, "katip@api", "durum", json!({"id": id, "durum": "tamamla"})).unwrap()["durum"], "onayda");
    assert!(gonder(c, "katip@api", "durum", json!({"id": id, "durum": "tamamla"})).is_err(), "kendi işini onaylayamaz");
    assert_eq!(al(c, "mudur@api", "ozet", &[]).unwrap()["onay"], 1);
    assert_eq!(gonder(c, "mudur@api", "durum", json!({"id": id, "durum": "tamamla"})).unwrap()["durum"], "tamamlandi");
    // Not, ayrıntı, gizli iş başkasına görünmez
    gonder(c, "katip@api", "not", json!({"id": id, "metin": "Bitti"})).unwrap();
    assert_eq!(al(c, "mudur@api", "gorev", &[("id", &id.to_string())]).unwrap()["notlar"][0]["metin"], "Bitti");
    let gizli = gonder(c, "katip@api", "ac", json!({"baslik": "Kişisel", "son_tarih": "2026-11-17", "gizli": true})).unwrap()["id"]
        .as_i64()
        .unwrap();
    assert!(al(c, "mudur@api", "gorev", &[("id", &gizli.to_string())]).is_err(), "gizli iş yöneticiye de görünmez");
    // Erteleme (onay gerekmeyen iş: doğrudan uygulanır)
    assert_eq!(gonder(c, "katip@api", "ertele", json!({"id": gizli, "yeni_tarih": "2026-11-18", "gerekce": "yarın"})).unwrap()["uygulandi"], true);
    assert!(dagit(c, "katip@api", "GET", "olmayan", &bos, &json!({}), bugun).unwrap().is_none());
    // Bildirim (L6): müdürün katibe açtığı iş katibin "yeni" sayacında; katip açıp bakınca düşer.
    let yeni_id = gonder(c, "mudur@api", "ac", json!({"baslik": "Yeni atanan iş", "son_tarih": "2026-11-20", "sorumlu_id": katip}))
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let oz = al(c, "katip@api", "ozet", &[]).unwrap();
    assert_eq!(oz["yeni"], 1, "{oz}");
    assert_eq!(oz["yeni_isler"][0]["id"], yeni_id);
    assert_eq!(al(c, "mudur@api", "ozet", &[]).unwrap()["yeni"], 0, "kendi açtığı iş kendine yeni sayılmaz");
    al(c, "katip@api", "gorev", &[("id", &yeni_id.to_string())]).unwrap();
    assert_eq!(al(c, "katip@api", "ozet", &[]).unwrap()["yeni"], 0, "bakılan iş yeni değil");
    // Hatırlatıcı: geçmişe kurulamaz; başkasının gizli işine kurulamaz; zamanı gelince özette görünür
    let ileri = (chrono::Local::now() + chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
    let ileri_t = format!("{ileri}T09:00");
    assert!(gonder(c, "katip@api", "hatirlatici", json!({"gorev_id": yeni_id, "zaman": "2020-01-01T09:00"})).is_err());
    assert!(gonder(c, "mudur@api", "hatirlatici", json!({"gorev_id": gizli, "zaman": ileri_t})).is_err());
    let h = gonder(c, "katip@api", "hatirlatici", json!({"gorev_id": yeni_id, "zaman": ileri_t, "notu": "ara"})).unwrap()["id"]
        .as_i64()
        .unwrap();
    let liste = al(c, "katip@api", "liste", &[("gorunum", "tum"), ("dosya_no", "")]).unwrap();
    let satir = liste.as_array().unwrap().iter().find(|g| g["id"] == yeni_id).unwrap().clone();
    assert_eq!(satir["hatirlatma"], format!("{ileri} 09:00"));
    assert_eq!(al(c, "katip@api", "gorev", &[("id", &yeni_id.to_string())]).unwrap()["hatirlaticilar"][0]["notu"], "ara");
    assert_eq!(al(c, "katip@api", "ozet", &[]).unwrap()["hatirlatma_sayi"], 0, "zamanı gelmedi");
    c.execute("UPDATE hatirlatici SET zaman = now() - interval '1 minute' WHERE id = $1", &[&h]).unwrap();
    let oz = al(c, "katip@api", "ozet", &[]).unwrap();
    assert_eq!((oz["hatirlatma_sayi"].as_i64(), oz["hatirlatma"][0]["baslik"].as_str()), (Some(1), Some("Yeni atanan iş")));
    assert!(gonder(c, "mudur@api", "hatirlatici_sil", json!({"id": h})).is_err(), "başkasının hatırlatıcısı silinemez");
    gonder(c, "katip@api", "hatirlatici_ertele", json!({"id": h, "dakika": 30})).unwrap();
    assert_eq!(al(c, "katip@api", "ozet", &[]).unwrap()["hatirlatma_sayi"], 0, "ertelendi");
    c.execute("UPDATE hatirlatici SET zaman = now() - interval '1 minute' WHERE id = $1", &[&h]).unwrap();
    gonder(c, "katip@api", "hatirlatici_tamam", json!({"id": h})).unwrap();
    assert_eq!(al(c, "katip@api", "ozet", &[]).unwrap()["hatirlatma_sayi"], 0, "görüldü");
    assert!(al(c, "katip@api", "hatirlaticilar", &[]).unwrap().as_array().unwrap().is_empty());
    // Pasif hesap reddedilir
    iv::kullanici_esitle(c, "eski@api", "Eski", false, false).unwrap();
    assert!(al(c, "eski@api", "ben", &[]).is_err());
    c.batch_execute("DELETE FROM gorev WHERE baslik IN ('API deneme işi', 'Kişisel', 'Yeni atanan iş')").unwrap();
}
