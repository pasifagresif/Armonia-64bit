//! UYAP'tan gelen dosya verisinin büro veritabanına yazılması (Aşama 3, çevrimdışı yarı).
//!
//! Girdi her zaman elde hazır JSON'dur; burada UYAP'a istek atılmaz. Kurallar örnek koddan
//! (UyapMasa `dosya_core` / `icra_core` / `barkod_sorgu`) taşınmıştır:
//!   • taraf kimlikle tekildir; aynı dosyada ad tekildir; dolu alan boşla ezilmez
//!   • UYAP'ın taraf listesinde artık görünmeyen bağ silinmez, işaretlenir (geçmişi ona bağlı)
//!   • UYAP'ın hata zarfı (`errorCode`) veri sayılmaz, kayıtlı doğru verinin üzerine yazılmaz
//!   • PTT'ye ulaşılamadıysa kayıtlı PTT durumu korunur
//!
//! İşlevler eşzamanlıdır; async koddan `spawn_blocking` içinde çağrılır.

use crate::icra_tebligat::{self as teb, TarafBilgisi};
use crate::icra_veri::{self as veri, durum_olayi_yaz, kirp, OlayKaynak, OlayTur, Rol, YeniOlay};
use postgres::{GenericClient, Row};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

/// İlerleme / uyarı satırları için.
pub type Gunluk<'a> = &'a dyn Fn(&str);

// ── Taraf ───────────────────────────────────────────────────────────────────────────────────────

const TARAF_SUTUNLARI: &str = "t.id, t.tur, t.ad, t.soyad, t.unvan, t.tckn, t.mersis_no, t.vergi_no";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TarafKaydi {
    pub id: i64,
    pub tur: String,
    pub ad: String,
    pub soyad: String,
    pub unvan: String,
    pub tckn: Option<String>,
    pub mersis_no: Option<String>,
    pub vergi_no: String,
}

fn var(v: &Option<String>) -> bool {
    v.as_deref().is_some_and(|s| !s.is_empty())
}

impl TarafKaydi {
    fn satirdan(s: &Row, bas: usize) -> Self {
        TarafKaydi {
            id: s.get(bas),
            tur: s.get(bas + 1),
            ad: s.get(bas + 2),
            soyad: s.get(bas + 3),
            unvan: s.get(bas + 4),
            tckn: s.get(bas + 5),
            mersis_no: s.get(bas + 6),
            vergi_no: s.get(bas + 7),
        }
    }

    /// Ekranda ve geçmişte görünen ad.
    pub fn gorunen_ad(&self) -> String {
        let dolu = |x: &str| (!x.trim().is_empty()).then(|| x.to_string());
        let yedek = format!("Taraf #{}", self.id);
        if self.tur == "tuzel" {
            dolu(&self.unvan)
                .or_else(|| self.mersis_no.as_deref().and_then(dolu))
                .or_else(|| dolu(&self.vergi_no))
                .unwrap_or(yedek)
        } else {
            dolu(format!("{} {}", self.ad, self.soyad).trim())
                .or_else(|| self.tckn.as_deref().and_then(dolu))
                .unwrap_or(yedek)
        }
    }

    pub fn kimliksiz(&self) -> bool {
        !var(&self.tckn) && !var(&self.mersis_no) && self.vergi_no.is_empty()
    }

    fn anahtar(&self) -> String {
        teb::ad_anahtari_taraf(&self.tur, &self.ad, &self.soyad, &self.unvan)
    }
}

/// Gelen satırdaki geçerli kimlikler: gerçek kişide TCKN (11), tüzelde MERSİS (16) ve vergi no
/// (10 / 11). Maskeli ya da eksik değer kimlik sayılmaz.
struct Kimlik {
    tckn: Option<String>,
    mersis: Option<String>,
    vergi: Option<String>,
}

impl Kimlik {
    fn cikar(info: &TarafBilgisi) -> Self {
        if info.tuzel_mi() {
            Kimlik {
                tckn: None,
                mersis: teb::kimlik_temizle(&info.mersis_no, &[16]),
                vergi: teb::kimlik_temizle(&info.vergi_no, &[10, 11]),
            }
        } else {
            Kimlik { tckn: teb::kimlik_temizle(&info.tckn, &[11]), mersis: None, vergi: None }
        }
    }

    fn var(&self) -> bool {
        self.tckn.is_some() || self.mersis.is_some() || self.vergi.is_some()
    }

    fn celisir(&self, t: &TarafKaydi) -> bool {
        let farkli = |yeni: &Option<String>, eski: Option<&str>| match (yeni, eski) {
            (Some(y), Some(e)) => !e.is_empty() && e != y,
            _ => false,
        };
        farkli(&self.tckn, t.tckn.as_deref())
            || farkli(&self.mersis, t.mersis_no.as_deref())
            || farkli(&self.vergi, Some(t.vergi_no.as_str()))
    }
}

fn info_turu(info: &TarafBilgisi) -> &str {
    if info.tur.is_empty() { "gercek" } else { &info.tur }
}

/// Yalnız dolu gelen alanları yazar: dolu bir kimlik ya da ad, sonraki kimliksiz yanıtla silinmez.
fn taraf_guncelle<C: GenericClient>(c: &mut C, taraf: &mut TarafKaydi, info: &TarafBilgisi, k: &Kimlik) -> Result<(), String> {
    let mut yeni = taraf.clone();
    yeni.tur = info_turu(info).to_string();
    for (hedef, deger, sinir) in
        [(&mut yeni.ad, &info.ad, 120), (&mut yeni.soyad, &info.soyad, 120), (&mut yeni.unvan, &info.unvan, 400)]
    {
        if !deger.is_empty() {
            *hedef = kirp(deger, sinir);
        }
    }
    if let Some(t) = &k.tckn {
        yeni.tckn = Some(t.clone());
    }
    if let Some(m) = &k.mersis {
        yeni.mersis_no = Some(m.clone());
    }
    if let Some(v) = &k.vergi {
        yeni.vergi_no = v.clone();
    }
    if yeni != *taraf {
        c.execute(
            "UPDATE taraf SET tur = $2, ad = $3, soyad = $4, unvan = $5, tckn = $6, mersis_no = $7, vergi_no = $8,
                    guncelleme = now() WHERE id = $1",
            &[&yeni.id, &yeni.tur, &yeni.ad, &yeni.soyad, &yeni.unvan, &yeni.tckn, &yeni.mersis_no, &yeni.vergi_no],
        )
        .map_err(hata)?;
        *taraf = yeni;
    }
    Ok(())
}

fn taraf_olustur<C: GenericClient>(c: &mut C, info: &TarafBilgisi, k: &Kimlik) -> Result<TarafKaydi, String> {
    let mut t = TarafKaydi {
        id: 0,
        tur: info_turu(info).to_string(),
        ad: kirp(&info.ad, 120),
        soyad: kirp(&info.soyad, 120),
        unvan: kirp(&info.unvan, 400),
        tckn: k.tckn.clone(),
        mersis_no: k.mersis.clone(),
        vergi_no: k.vergi.clone().unwrap_or_default(),
    };
    t.id = c
        .query_one(
            "INSERT INTO taraf (tur, ad, soyad, unvan, tckn, mersis_no, vergi_no) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
            &[&t.tur, &t.ad, &t.soyad, &t.unvan, &t.tckn, &t.mersis_no, &t.vergi_no],
        )
        .map_err(hata)?
        .get(0);
    Ok(t)
}

fn taraf_sorgula<C: GenericClient>(c: &mut C, kosul: &str, deger: &str, en_cok: i64) -> Result<Vec<TarafKaydi>, String> {
    let sql = format!("SELECT {TARAF_SUTUNLARI} FROM taraf t WHERE {kosul} ORDER BY t.id LIMIT $2");
    Ok(c.query(&sql, &[&deger, &en_cok]).map_err(hata)?.iter().map(|s| TarafKaydi::satirdan(s, 0)).collect())
}

/// Bu kimliğin (TCKN, sonra MERSİS, sonra vergi no) kayıtlı sahibi.
fn kimlik_sahibi<C: GenericClient>(c: &mut C, k: &Kimlik) -> Result<Option<TarafKaydi>, String> {
    for (kosul, deger) in [("t.tckn = $1", &k.tckn), ("t.mersis_no = $1", &k.mersis), ("t.vergi_no = $1", &k.vergi)] {
        if let Some(d) = deger {
            if let Some(t) = taraf_sorgula(c, kosul, d, 1)?.into_iter().next() {
                return Ok(Some(t));
            }
        }
    }
    Ok(None)
}

/// Tarafı bulur ya da oluşturur. Sıra: (1) kimlikle birebir kayıt; (2) kimliksiz gelen satır
/// için ad eşleşmesi — yalnız tek kayıt varsa, adaşlar varsa tek kimliksiz kayıt; (3) yeni kayıt.
pub fn taraf_kaydet<C: GenericClient>(c: &mut C, info: &TarafBilgisi) -> Result<TarafKaydi, String> {
    let k = Kimlik::cikar(info);
    if let Some(mut t) = kimlik_sahibi(c, &k)? {
        taraf_guncelle(c, &mut t, info, &k)?;
        return Ok(t);
    }
    let tur = info_turu(info);
    if !k.var() && !teb::ad_anahtari_taraf(tur, &info.ad, &info.soyad, &info.unvan).is_empty() {
        let (kosul, deger) = if tur == "tuzel" {
            ("t.tur = 'tuzel' AND t.unvan = $1", info.unvan.clone())
        } else {
            let tam = format!("{} {}", info.ad, info.soyad).split_whitespace().collect::<Vec<_>>().join(" ");
            ("t.tur = 'gercek' AND btrim(t.ad || ' ' || t.soyad) = $1", tam)
        };
        if !deger.is_empty() {
            let mut adaylar = taraf_sorgula(c, kosul, &deger, 2)?;
            if adaylar.len() > 1 {
                let kimliksiz = format!("{kosul} AND t.tckn IS NULL AND t.mersis_no IS NULL AND t.vergi_no = ''");
                adaylar = taraf_sorgula(c, &kimliksiz, &deger, 2)?;
            }
            if adaylar.len() == 1 {
                let mut t = adaylar.remove(0);
                taraf_guncelle(c, &mut t, info, &k)?;
                return Ok(t);
            }
        }
    }
    taraf_olustur(c, info, &k)
}

// ── Dosya ↔ taraf bağı ──────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Bag {
    pub id: i64,
    pub rol: String,
    pub sira: i32,
    /// UYAP'ın taraf listesinde artık görünmüyor (işaretli).
    pub gorulmedi: bool,
    pub taraf: TarafKaydi,
}

/// Dosyanın bütün taraf bağları, taraflarıyla.
pub fn dosya_baglari<C: GenericClient>(c: &mut C, dosya: i64) -> Result<Vec<Bag>, String> {
    let sql = format!(
        "SELECT dt.id, dt.rol, dt.sira, dt.uyapta_gorulmedi IS NOT NULL, {TARAF_SUTUNLARI}
           FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE dt.dosya_id = $1 ORDER BY dt.id"
    );
    Ok(c.query(&sql, &[&dosya])
        .map_err(hata)?
        .iter()
        .map(|s| Bag { id: s.get(0), rol: s.get(1), sira: s.get(2), gorulmedi: s.get(3), taraf: TarafKaydi::satirdan(s, 4) })
        .collect())
}

/// Dosyanın elle mazbata bağları: (tebligat evrak no, mazbata evrak no).
pub fn elle_mazbata_baglari<C: GenericClient>(c: &mut C, dosya: i64) -> Result<Vec<(i32, i32)>, String> {
    Ok(c.query("SELECT tebligat_evrak_no, mazbata_evrak_no FROM tebligat_mazbata_bagi WHERE dosya_id = $1 ORDER BY id", &[&dosya])
        .map_err(hata)?
        .iter()
        .map(|s| (s.get(0), s.get(1)))
        .collect())
}

/// Dosyanın borçlu bağları: (bağ kimliği, görünen ad) — tebligat → borçlu eşleşmesi için.
pub fn dosya_borclulari<C: GenericClient>(c: &mut C, dosya: i64) -> Result<Vec<(i64, String)>, String> {
    Ok(dosya_baglari(c, dosya)?
        .into_iter()
        .filter(|b| b.rol == Rol::Borclu.kod())
        .map(|b| (b.id, b.taraf.gorunen_ad()))
        .collect())
}

/// Borçlu bağları dosya dosya: dosya → [(bağ kimliği, görünen ad)]. `dosya` verilmezse hepsi.
pub fn borclu_haritasi<C: GenericClient>(c: &mut C, dosya: Option<i64>) -> Result<HashMap<i64, Vec<(i64, String)>>, String> {
    let sql = format!(
        "SELECT dt.dosya_id, dt.id, {TARAF_SUTUNLARI} FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id
          WHERE dt.rol = 'borclu' AND ($1::bigint IS NULL OR dt.dosya_id = $1) ORDER BY dt.id"
    );
    let mut harita: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for s in c.query(&sql, &[&dosya]).map_err(hata)? {
        harita.entry(s.get(0)).or_default().push((s.get(1), TarafKaydi::satirdan(&s, 2).gorunen_ad()));
    }
    Ok(harita)
}

/// Borçluların vekilleri: dosya → [(borçlu bağı, vekil adı)]. "Vekil …" muhataplı tebligatı borçluya bağlamak için.
pub fn borclu_vekil_haritasi<C: GenericClient>(c: &mut C, dosya: Option<i64>) -> Result<HashMap<i64, Vec<(i64, String)>>, String> {
    let mut harita: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for s in c
        .query(
            "SELECT dt.dosya_id, dt.id, btrim(v.ad || ' ' || v.soyad) FROM dosya_taraf dt
               JOIN dosya_taraf_vekiller dv ON dv.dosyataraf_id = dt.id JOIN vekil v ON v.id = dv.vekil_id
              WHERE dt.rol = 'borclu' AND ($1::bigint IS NULL OR dt.dosya_id = $1)",
            &[&dosya],
        )
        .map_err(hata)?
    {
        harita.entry(s.get(0)).or_default().push((s.get(1), s.get(2)));
    }
    Ok(harita)
}

/// Bir UYAP taraf satırını bu dosyanın bağına yazar ve bağ kimliğini döner. Önce dosyanın kendi
/// bağlarında aynı rol + aynı ad aranır (dosya içinde ad tekildir); bulunursa:
///   • gelen kimlik bağın tarafında yoksa ve o taraf yalnız bu dosyada kullanılıyorsa kimlik
///     o tarafa yazılır;
///   • taraf başka dosyalarla paylaşılıyorsa ya da kimliği çelişiyorsa bağ, kimliğin gerçek
///     sahibine (yoksa yeni tarafa) taşınır — bağ satırı aynı kalır; kesinleşme / tebliğ alanları
///     ve tebligat → borçlu bağlantısı korunur.
/// Bulunmazsa genel eşleştirme ([`taraf_kaydet`]) ve yeni bağ.
fn dosya_tarafi_yaz<C: GenericClient>(
    c: &mut C,
    dosya: i64,
    info: &TarafBilgisi,
    rol: &str,
    sira: i32,
    baglar: &mut Vec<Bag>,
    log: Gunluk,
) -> Result<i64, String> {
    let tur = info_turu(info);
    let k = Kimlik::cikar(info);
    let anahtar = teb::ad_anahtari_taraf(tur, &info.ad, &info.soyad, &info.unvan);
    let mut es: Vec<usize> = baglar
        .iter()
        .enumerate()
        .filter(|(_, b)| !anahtar.is_empty() && b.rol == rol && b.taraf.tur == tur && b.taraf.anahtar() == anahtar)
        .map(|(i, _)| i)
        .collect();
    if es.len() > 1 {
        let gorunen: Vec<usize> = es.iter().copied().filter(|i| !baglar[*i].gorulmedi).collect();
        if !gorunen.is_empty() {
            es = gorunen;
        }
    }
    if es.len() != 1 {
        let taraf = taraf_kaydet(c, info)?;
        let s = c
            .query_one(
                "INSERT INTO dosya_taraf (dosya_id, taraf_id, rol, sira) VALUES ($1, $2, $3, $4)
                 ON CONFLICT ON CONSTRAINT uq_dosya_taraf_rol DO UPDATE SET sira = EXCLUDED.sira
                 RETURNING id, uyapta_gorulmedi IS NOT NULL",
                &[&dosya, &taraf.id, &rol, &sira],
            )
            .map_err(hata)?;
        let id: i64 = s.get(0);
        match baglar.iter_mut().find(|b| b.id == id) {
            Some(b) => b.sira = sira,
            None => baglar.push(Bag { id, rol: rol.to_string(), sira, gorulmedi: s.get(1), taraf }),
        }
        return Ok(id);
    }

    let i = es[0];
    let mevcut = baglar[i].taraf.clone();
    let mut bag_id = baglar[i].id;
    let mut hedef = Some(mevcut.clone());
    if k.var() && (mevcut.kimliksiz() || k.celisir(&mevcut)) {
        hedef = match kimlik_sahibi(c, &k)? {
            Some(sahibi) => Some(sahibi),
            None => {
                let paylasiliyor: bool = c
                    .query_one(
                        "SELECT EXISTS (SELECT 1 FROM dosya_taraf WHERE taraf_id = $1 AND dosya_id <> $2)",
                        &[&mevcut.id, &dosya],
                    )
                    .map_err(hata)?
                    .get(0);
                // Yalnız bu dosyanın kimliksiz kaydıysa kimlik ona yazılır; değilse yeni taraf.
                (mevcut.kimliksiz() && !paylasiliyor).then(|| mevcut.clone())
            }
        };
    }
    let mut hedef = match hedef {
        Some(h) => h,
        None => taraf_olustur(c, info, &k)?,
    };
    taraf_guncelle(c, &mut hedef, info, &k)?;
    if hedef.id == mevcut.id {
        baglar[i].taraf = hedef;
    } else {
        let cakisan = c
            .query_opt(
                "SELECT id FROM dosya_taraf WHERE dosya_id = $1 AND taraf_id = $2 AND rol = $3 AND id <> $4 LIMIT 1",
                &[&dosya, &hedef.id, &rol, &bag_id],
            )
            .map_err(hata)?;
        match cakisan {
            Some(s) => bag_id = s.get(0), // doğru bağ zaten var
            None => {
                let neyle = if k.tckn.is_some() { "TCKN" } else { "vergi/MERSİS" };
                log(&format!(
                    "  … {} '{}' kaydı kimliğiyle ayrıldı ({neyle} eşleşmesi).",
                    Rol::etiket_bul(rol),
                    mevcut.gorunen_ad()
                ));
                c.execute("UPDATE dosya_taraf SET taraf_id = $2 WHERE id = $1", &[&bag_id, &hedef.id]).map_err(hata)?;
                let yeni = match &k.tckn {
                    Some(t) => format!("{} (TCKN {t})", hedef.gorunen_ad()),
                    None => format!("{} (vergi/MERSİS)", hedef.gorunen_ad()),
                };
                baglar[i].taraf = hedef;
                durum_olayi_yaz(
                    c,
                    &YeniOlay {
                        dosya,
                        dosya_taraf: Some(bag_id),
                        tur: OlayTur::TarafKimlik,
                        eski: &mevcut.gorunen_ad(),
                        yeni: &yeni,
                        olay_tarihi: None,
                        kaynak: OlayKaynak::Uyap,
                        aciklama: "",
                    },
                )?;
            }
        }
    }
    c.execute("UPDATE dosya_taraf SET sira = $2 WHERE id = $1 AND sira <> $2", &[&bag_id, &sira]).map_err(hata)?;
    if let Some(b) = baglar.iter_mut().find(|b| b.id == bag_id) {
        b.sira = sira;
    }
    Ok(bag_id)
}

/// UYAP'ın taraf listesinde artık görünmeyen bağ silinmez; `uyapta_gorulmedi` ile işaretlenir ve
/// geçmişe yazılır. Yeniden görünen bağın işareti kalkar. Yalnız tam listesi alınan roller
/// karşılaştırılır (`roller` boşsa hepsi). Döner: bu çağrıda yeni işaretlenenler ("Rol: Ad").
pub fn kaybolan_taraflari_isaretle<C: GenericClient>(
    c: &mut C,
    dosya: i64,
    gorulen: &HashSet<i64>,
    roller: Option<&[&str]>,
    log: Gunluk,
) -> Result<Vec<String>, String> {
    let idler: Vec<i64> = gorulen.iter().copied().collect();
    let roller: Option<Vec<String>> = roller.map(|r| r.iter().map(|x| x.to_string()).collect());
    let geri: Vec<i64> = c
        .query(
            "SELECT id FROM dosya_taraf WHERE dosya_id = $1 AND id = ANY($2) AND uyapta_gorulmedi IS NOT NULL
                AND ($3::text[] IS NULL OR rol = ANY($3)) ORDER BY id",
            &[&dosya, &idler, &roller],
        )
        .map_err(hata)?
        .iter()
        .map(|s| s.get(0))
        .collect();
    for id in geri {
        c.execute("UPDATE dosya_taraf SET uyapta_gorulmedi = NULL WHERE id = $1", &[&id]).map_err(hata)?;
        durum_olayi_yaz(
            c,
            &YeniOlay {
                dosya,
                dosya_taraf: Some(id),
                tur: OlayTur::TarafGeriGeldi,
                eski: "UYAP'ta görülmedi",
                yeni: "UYAP'ta var",
                olay_tarihi: None,
                kaynak: OlayKaynak::Uyap,
                aciklama: "",
            },
        )?;
    }
    let sql = format!(
        "SELECT dt.id, dt.rol, {TARAF_SUTUNLARI} FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id
          WHERE dt.dosya_id = $1 AND NOT (dt.id = ANY($2)) AND dt.uyapta_gorulmedi IS NULL
            AND ($3::text[] IS NULL OR dt.rol = ANY($3)) ORDER BY dt.id"
    );
    let yeni: Vec<(i64, String, TarafKaydi)> = c
        .query(&sql, &[&dosya, &idler, &roller])
        .map_err(hata)?
        .iter()
        .map(|s| (s.get(0), s.get(1), TarafKaydi::satirdan(s, 2)))
        .collect();
    let mut adlar = Vec::new();
    for (id, rol, taraf) in &yeni {
        let etiket = Rol::etiket_bul(rol);
        c.execute("UPDATE dosya_taraf SET uyapta_gorulmedi = now() WHERE id = $1", &[id]).map_err(hata)?;
        durum_olayi_yaz(
            c,
            &YeniOlay {
                dosya,
                dosya_taraf: Some(*id),
                tur: OlayTur::TarafGorulmedi,
                eski: "UYAP'ta var",
                yeni: "UYAP'ta görülmedi",
                olay_tarihi: None,
                kaynak: OlayKaynak::Uyap,
                aciklama: &format!("{etiket} UYAP'ın taraf listesinde yok."),
            },
        )?;
        adlar.push(format!("{etiket}: {}", taraf.gorunen_ad()));
    }
    if !adlar.is_empty() {
        log(&format!(
            "⚠️ UYAP'ta artık görünmeyen taraf (kayıt silinmedi, işaretlendi): {}. UYAP'ta kontrol edin.",
            adlar.join("; ")
        ));
    }
    Ok(adlar)
}

/// Vekil alanındaki adları (yalnız ad + soyad; bu uç başka kimlik vermez) bulur ya da oluşturur.
fn vekilleri_kaydet<C: GenericClient>(c: &mut C, vekil_ham: &str) -> Result<Vec<i64>, String> {
    let mut idler = Vec::new();
    for (ad, soyad) in teb::vekil_adlari(vekil_ham) {
        let (ad, soyad) = (kirp(&ad, 120), kirp(&soyad, 120));
        let id: i64 = match c
            .query_opt("SELECT id FROM vekil WHERE ad = $1 AND soyad = $2 ORDER BY id LIMIT 1", &[&ad, &soyad])
            .map_err(hata)?
        {
            Some(s) => s.get(0),
            None => c.query_one("INSERT INTO vekil (ad, soyad) VALUES ($1, $2) RETURNING id", &[&ad, &soyad]).map_err(hata)?.get(0),
        };
        if !idler.contains(&id) {
            idler.push(id);
        }
    }
    Ok(idler)
}

/// `dosya_taraf_bilgileri_brd` yanıtını (adi, rol, kisiKurum, vekil) bağlara yazar. Liste tam
/// sayılır: listede olmayan bağlar işaretlenir. Döner: kaydedilen taraf sayısı.
pub fn dosya_taraf_kaydet<C: GenericClient>(c: &mut C, dosya: i64, ham_liste: &[Value], log: Gunluk) -> Result<usize, String> {
    dosya_taraf_kaydet_roller(c, dosya, ham_liste, None, log)
}

/// [`dosya_taraf_kaydet`]; `roller` verilirse liste yalnız o roller için tam sayılır (listede olmayan
/// bağ yalnız bu rollerde işaretlenir). Borçlu dışı roller yazılırken borçlular işaretlenmesin diye
/// (Python `haric_roller=["borclu"]`).
pub fn dosya_taraf_kaydet_roller<C: GenericClient>(
    c: &mut C,
    dosya: i64,
    ham_liste: &[Value],
    roller: Option<&[&str]>,
    log: Gunluk,
) -> Result<usize, String> {
    if ham_liste.is_empty() {
        return Ok(0);
    }
    let mut tx = c.transaction().map_err(hata)?;
    let mut baglar = dosya_baglari(&mut tx, dosya)?;
    let mut gorulen = HashSet::new();
    for (sira, item) in ham_liste.iter().enumerate() {
        if !item.is_object() {
            continue;
        }
        let info = teb::taraf_bilgisi_ayristir(item);
        if info.bos_mu() {
            log(&format!("  ⚠️ Adı olmayan taraf kaydı atlandı (rol: {}).", teb::metin(item, "rol")));
            continue;
        }
        let rol = teb::tr_kucuk(&teb::metin(item, "rol"));
        let rol = if rol.is_empty() { "diger".to_string() } else { kirp(&rol, 255) };
        let bag = dosya_tarafi_yaz(&mut tx, dosya, &info, &rol, sira as i32, &mut baglar, log)?;
        let vekiller = vekilleri_kaydet(&mut tx, &teb::metin(item, "vekil"))?;
        tx.execute("DELETE FROM dosya_taraf_vekiller WHERE dosyataraf_id = $1 AND NOT (vekil_id = ANY($2))", &[&bag, &vekiller])
            .map_err(hata)?;
        tx.execute(
            "INSERT INTO dosya_taraf_vekiller (dosyataraf_id, vekil_id) SELECT $1, v FROM unnest($2::bigint[]) AS v
             ON CONFLICT DO NOTHING",
            &[&bag, &vekiller],
        )
        .map_err(hata)?;
        gorulen.insert(bag);
    }
    if !gorulen.is_empty() {
        kaybolan_taraflari_isaretle(&mut tx, dosya, &gorulen, roller, log)?;
    }
    tx.commit().map_err(hata)?;
    Ok(gorulen.len())
}

/// `dosya_taraf_bilgileri_brd` yanıtındaki BORÇLU satırlarının vekillerini, dosyada zaten kayıtlı (kimlikli) borçlulara
/// yazar (20.11.2026). Taraf açılmaz, borçlunun kimliği değişmez: bu uç kimlik vermez, borçlu `dosya_borclu_list`ten
/// gelir. Eşleşme adla (Türkçe küçük harf, boşluk farkı önemsiz). Döner: vekilleri yazılan borçlu sayısı.
pub fn borclu_vekillerini_yaz<C: GenericClient>(c: &mut C, dosya: i64, taraf_ham: &[Value]) -> Result<usize, String> {
    let anahtar = |s: &str| teb::tr_kucuk(s).split_whitespace().collect::<Vec<_>>().join(" ");
    let ad = crate::icra_api::ad_sql("t");
    let mut tx = c.transaction().map_err(hata)?;
    let borclular: Vec<(i64, String)> = tx
        .query(
            &format!(
                "SELECT dt.id, {ad} FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id
                  WHERE dt.dosya_id = $1 AND dt.rol = 'borclu' AND dt.uyapta_gorulmedi IS NULL"
            ),
            &[&dosya],
        )
        .map_err(hata)?
        .iter()
        .map(|r| (r.get(0), anahtar(&r.get::<_, String>(1))))
        .collect();
    let mut yazilan = 0;
    for item in taraf_ham.iter().filter(|x| x.is_object() && teb::tr_kucuk(&teb::metin(x, "rol")) == Rol::Borclu.kod()) {
        let aranan = anahtar(&teb::metin(item, "adi"));
        let Some((bag, _)) = borclular.iter().find(|(_, a)| !aranan.is_empty() && *a == aranan) else { continue };
        let vekiller = vekilleri_kaydet(&mut tx, &teb::metin(item, "vekil"))?;
        tx.execute("DELETE FROM dosya_taraf_vekiller WHERE dosyataraf_id = $1 AND NOT (vekil_id = ANY($2))", &[bag, &vekiller])
            .map_err(hata)?;
        tx.execute(
            "INSERT INTO dosya_taraf_vekiller (dosyataraf_id, vekil_id) SELECT $1, v FROM unnest($2::bigint[]) AS v ON CONFLICT DO NOTHING",
            &[bag, &vekiller],
        )
        .map_err(hata)?;
        yazilan += 1;
    }
    tx.commit().map_err(hata)?;
    Ok(yazilan)
}

/// Dosyanın taraf bilgileri (vekiller dahil) UYAP'tan okundu: "Tarafları güncelle" bu dosyaya yeniden gitmez.
pub fn taraf_bilgisi_okundu<C: GenericClient>(c: &mut C, dosya: i64) -> Result<(), String> {
    c.execute("UPDATE dosya SET taraf_bilgisi_zamani = now() WHERE id = $1", &[&dosya]).map_err(hata)?;
    Ok(())
}

/// `dosya_borclu_list` yanıtını borçlu bağlarına yazar: aynı dosyadaki adı tutan kimliksiz
/// borçluya TCKN / vergi no / MERSİS eklenir (yeni taraf açılmaz). Borçlu listesi tam sayılır.
pub fn borclu_kimliklerini_kaydet<C: GenericClient>(c: &mut C, dosya: i64, borclu_ham: &[Value], log: Gunluk) -> Result<usize, String> {
    if borclu_ham.is_empty() {
        return Ok(0);
    }
    let mut tx = c.transaction().map_err(hata)?;
    let mut baglar = dosya_baglari(&mut tx, dosya)?;
    let mut gorulen = HashSet::new();
    for (sira, item) in borclu_ham.iter().enumerate() {
        if !item.is_object() {
            continue;
        }
        let info = teb::borclu_ayristir(item);
        if info.bos_mu() {
            continue;
        }
        gorulen.insert(dosya_tarafi_yaz(&mut tx, dosya, &info, Rol::Borclu.kod(), sira as i32, &mut baglar, log)?);
    }
    if !gorulen.is_empty() {
        kaybolan_taraflari_isaretle(&mut tx, dosya, &gorulen, Some(&[Rol::Borclu.kod()]), log)?;
    }
    tx.commit().map_err(hata)?;
    Ok(gorulen.len())
}

/// Dosyada kimliği (TCKN / MERSİS / vergi no) olmayan borçlu var mı? (Varsa borçlu listesi
/// ayrıca istenir; yoksa o istek hiç atılmaz.)
pub fn borclu_kimligi_eksik_mi<C: GenericClient>(c: &mut C, dosya: i64) -> Result<bool, String> {
    Ok(c.query_one(
        "SELECT EXISTS (SELECT 1 FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id
                         WHERE dt.dosya_id = $1 AND dt.rol = 'borclu'
                           AND t.tckn IS NULL AND t.mersis_no IS NULL AND t.vergi_no = '')",
        &[&dosya],
    )
    .map_err(hata)?
    .get(0))
}

// ── Dosya ayrıntısı ─────────────────────────────────────────────────────────────────────────────

/// UYAP'ın genel hata zarfı (`{"errorCode": …, "error": …}`). Veri değildir; yazılırsa kayıtlı
/// doğru değerleri boşla ezer.
pub fn hata_zarfi_mi(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.contains_key("errorCode"))
}

/// Tutarı veritabanına gönderilecek sayı metnine çevirir; çözülemeyen değer 0'dır.
fn tutar(v: Option<&Value>) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let s = match v {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.trim().to_string(),
        _ => String::new(),
    };
    let gecerli = R.get_or_init(|| Regex::new(r"^-?\d+(\.\d+)?([eE][-+]?\d+)?$").expect("desen")).is_match(&s);
    if gecerli { s } else { "0".to_string() }
}

/// UYAP'ın takip türü / yolu / şekli adı: baş-son boşluk atılır, iç boşluklar teke iner
/// (" ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49 " → "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49").
pub fn takip_adi(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Takip şekli adındaki örnek numarası ("ÖRNEK: 7 …" → "7", "ÖRNEK: 4-5 …" → "4-5"); yoksa boş.
pub fn ornek_no(sekli_adi: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = R.get_or_init(|| Regex::new(r"(?i)\b(?:örnek|ornek|ÖRNEK)\s*(?:no)?\s*[:.]?\s*(\d+(?:\s*-\s*\d+)?)").expect("desen"));
    r.captures(sekli_adi).map(|m| m[1].chars().filter(|c| !c.is_whitespace()).collect()).unwrap_or_default()
}

/// UYAP takip açılış ekranının bir seçenek listesi (`icra_takip_turu` / `_yolu` / `_sekli`): `[{value, name}]` →
/// (kod, ad). Liste değilse ya da öğede kod yoksa atlanır.
pub fn takip_secenekleri(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let kod = teb::metin(x, "value").trim().to_string();
                    (!kod.is_empty() && kod.chars().count() <= 8).then(|| (kod, takip_adi(&teb::metin(x, "name"))))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Takip türü listesinin bir öğesi: kademe 1 (tür), 2 (yol), 3 (şekil).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakipSecenegi {
    pub kademe: i16,
    pub turu: String,
    pub yolu: String,
    pub sekli: String,
    pub ad: String,
}

/// UYAP'tan alınan takip türü ağacını `takip_turu_listesi`'ne yazar (eklenir / adı güncellenir; listede artık
/// olmayan eski öğe silinmez: eski dosyalar o kodu taşıyor olabilir). Döner: yazılan öğe sayısı.
pub fn takip_listesi_kaydet<C: GenericClient>(c: &mut C, ogeler: &[TakipSecenegi]) -> Result<usize, String> {
    let mut tx = c.transaction().map_err(hata)?;
    for o in ogeler {
        let ad = kirp(&takip_adi(&o.ad), 255);
        tx.execute(
            "INSERT INTO takip_turu_listesi (kademe, takibin_turu, takibin_yolu, takibin_sekli, ad, ornek_no)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT ON CONSTRAINT uq_takip_turu_listesi DO UPDATE SET ad = EXCLUDED.ad, ornek_no = EXCLUDED.ornek_no,
                 guncelleme = now()",
            &[&o.kademe, &o.turu, &o.yolu, &o.sekli, &ad, &if o.kademe == 3 { ornek_no(&ad) } else { String::new() }],
        )
        .map_err(hata)?;
    }
    tx.commit().map_err(hata)?;
    Ok(ogeler.len())
}

/// `dosyaAyrintiBilgileri_brd` yanıtını dosyanın yargı türüne göre `icra_takip_detay` ya da
/// `hukuk_dava_detay` tablosuna yazar. Döner: "icra" / "hukuk"; yazılmadıysa `None` (boş yanıt,
/// hata zarfı ya da ayrıntı tablosu olmayan yargı türü).
pub fn dosya_ayrinti_kaydet<C: GenericClient>(c: &mut C, dosya: i64, ham: &Value) -> Result<Option<&'static str>, String> {
    if !ham.as_object().is_some_and(|o| !o.is_empty()) || hata_zarfi_mi(ham) {
        return Ok(None);
    }
    let Some(s) = c.query_opt("SELECT yargi_turu FROM dosya WHERE id = $1", &[&dosya]).map_err(hata)? else {
        return Err(format!("dosya bulunamadı: {dosya}"));
    };
    let yargi_turu: i16 = s.get(0);
    if yargi_turu == veri::YargiTuru::Icra.kod() {
        let kod = |a: &str| kirp(teb::metin(ham, a).trim(), 8);
        let sekli_adi = takip_adi(&teb::metin(ham, "takibinSekliAciklama"));
        c.execute(
            "INSERT INTO icra_takip_detay (dosya_id, takibin_turu, takibin_sekli, takibin_yolu,
                                           takibin_turu_aciklama, takibin_sekli_aciklama, takibin_yolu_aciklama, ornek_no,
                                           alacak_kalemi_toplam, vekalet_ucreti, tahsil_harci, ayrinti_zamani)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::text::numeric, $10::text::numeric, $11::text::numeric, now())
             ON CONFLICT (dosya_id) DO UPDATE SET takibin_turu = EXCLUDED.takibin_turu,
                 takibin_sekli = EXCLUDED.takibin_sekli, takibin_yolu = EXCLUDED.takibin_yolu,
                 takibin_turu_aciklama = EXCLUDED.takibin_turu_aciklama, takibin_sekli_aciklama = EXCLUDED.takibin_sekli_aciklama,
                 takibin_yolu_aciklama = EXCLUDED.takibin_yolu_aciklama, ornek_no = EXCLUDED.ornek_no,
                 alacak_kalemi_toplam = EXCLUDED.alacak_kalemi_toplam, vekalet_ucreti = EXCLUDED.vekalet_ucreti,
                 tahsil_harci = EXCLUDED.tahsil_harci, ayrinti_zamani = now(), guncelleme = now()",
            &[
                &dosya,
                &kod("takibinTuru"),
                &kod("takibinSekli"),
                &kod("takibinYolu"),
                &kirp(&takip_adi(&teb::metin(ham, "takibinTuruAciklama")), 120),
                &kirp(&sekli_adi, 255),
                &kirp(&takip_adi(&teb::metin(ham, "takibinYoluAciklama")), 120),
                &ornek_no(&sekli_adi),
                &tutar(ham.get("alacakKalemToplamTutar")),
                &tutar(ham.get("vekaletUcreti")),
                &tutar(ham.get("tahsilHarci")),
            ],
        )
        .map_err(hata)?;
        return Ok(Some("icra"));
    }
    if yargi_turu == veri::YargiTuru::Hukuk.kod() {
        let durusma = ham.get("durusmaTarihi").and_then(veri::tarih::coz);
        c.execute(
            "INSERT INTO hukuk_dava_detay (dosya_id, dava_acilis_turu, dava_turleri, ilgili_dava_listesi, durusma_tarihi)
             VALUES ($1, $2, $3, $4, $5::timestamp)
             ON CONFLICT (dosya_id) DO UPDATE SET dava_acilis_turu = EXCLUDED.dava_acilis_turu,
                 dava_turleri = EXCLUDED.dava_turleri, ilgili_dava_listesi = EXCLUDED.ilgili_dava_listesi,
                 durusma_tarihi = EXCLUDED.durusma_tarihi, guncelleme = now()",
            &[
                &dosya,
                &kirp(&teb::metin(ham, "davaAcilisTuru"), 120),
                &kirp(&teb::metin(ham, "davaTurleriStr"), 500),
                &kirp(&teb::metin(ham, "ilgiliDavaListesiStr"), 500),
                &durusma,
            ],
        )
        .map_err(hata)?;
        return Ok(Some("hukuk"));
    }
    Ok(None)
}

// ── Evrak listesi ───────────────────────────────────────────────────────────────────────────────

/// Evrak numarası (pozitif tam sayı) — yoksa `None`.
pub fn birim_evrak_no(evrak: &Value) -> Option<i32> {
    teb::tam(evrak, "birimEvrakNo").and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0)
}

/// Dosyanın evrak listesini (tür, açıklama, tarih) `evrak` tablosuna yazar; içerik indirilmez.
/// Takip aşamasının "Ödeme Emri" adımı bu listeden okunur. Döner: yazılan evrak sayısı.
pub fn evrak_listesi_kaydet<C: GenericClient>(c: &mut C, dosya: i64, evraklar: &[Value]) -> Result<usize, String> {
    let mut tx = c.transaction().map_err(hata)?;
    let mut n = 0;
    for e in evraklar {
        let Some(no) = birim_evrak_no(e) else { continue };
        let tarih = e.get("onaylandigiTarih").and_then(veri::tarih::coz);
        tx.execute(
            "INSERT INTO evrak (dosya_id, birim_evrak_no, evrak_turu, aciklama, evrak_tarihi)
             VALUES ($1, $2, $3, $4, $5::timestamp)
             ON CONFLICT ON CONSTRAINT uq_dosya_evrak DO UPDATE SET evrak_turu = EXCLUDED.evrak_turu,
                 aciklama = EXCLUDED.aciklama, evrak_tarihi = COALESCE(EXCLUDED.evrak_tarihi, evrak.evrak_tarihi),
                 son_erisim_zamani = now()",
            &[&dosya, &no, &kirp(&teb::metin(e, "tur"), 64), &kirp(&teb::metin(e, "aciklama"), 500), &tarih],
        )
        .map_err(hata)?;
        n += 1;
    }
    tx.commit().map_err(hata)?;
    Ok(n)
}

// ── Tebligat ────────────────────────────────────────────────────────────────────────────────────

/// Bir tebligat evrakı için yazılacak sonuç.
#[derive(Clone, Debug, Default)]
pub struct TebligatKaydi {
    pub evrak_aciklama: String,
    pub evrak_tarihi: String,
    pub barkod: String,
    pub elektronik_tebligat: bool,
    pub tebligat_turu: String,
    pub ptt_durumu: String,
    pub son_islem_tarihi: String,
    /// PTT ham yanıtı. `_hata` taşıyorsa (PTT'ye ulaşılamadı ya da bilerek sorulmadı) kayıtlı
    /// PTT durumu, tarihi ve ham yanıtı korunur.
    pub ham_yanit: Option<Value>,
    pub tebligat_mazbatasi_var: bool,
    pub tebligat_mazbatasi_aciklama: String,
    pub kapali_tebligat_mazbatasi_var: bool,
    /// Tebligatın ait olduğu borçlu bağı. Boşsa kayıtlı bağlantıya dokunulmaz.
    pub borclu: Option<i64>,
    pub yeniden_teblig_talep_edildi: bool,
    /// Belgedeki "BU ZARFTA … VARDIR" içeriği (memurun yazdığı hâliyle). Boşsa kayıtlıya dokunulmaz.
    pub zarf_icerigi: String,
    /// Belgede yazan haciz ihbarnamesi sırası (89/1-2-3). Boşsa ya da kullanıcı elle girdiyse kayıtlıya dokunulmaz.
    pub haciz_sirasi: Option<i16>,
    /// Fiziki zarfın adres bloğu (PDF'ten; Mernis kıyası için). Boşsa kayıtlıya dokunulmaz.
    pub teblig_adresi: String,
}

/// Daha önce kaydedilmiş tebligat (barkod bir kez atanır, değişmez).
#[derive(Clone, Debug)]
pub struct BilinenTebligat {
    pub id: i64,
    pub barkod: String,
    pub tebligat_turu: String,
    pub elektronik_tebligat: bool,
    pub ptt_durumu: String,
    pub son_islem_tarihi: String,
    pub evrak_tarihi: String,
    /// Kayıtlı zarf adresi (`t212_teblig_adresi`); boşsa fiziki ödeme emri zarfının PDF'i bir kez daha indirilir.
    pub teblig_adresi: String,
}

pub fn bilinen_tebligat<C: GenericClient>(c: &mut C, dosya: i64, birim_evrak_no: i32) -> Result<Option<BilinenTebligat>, String> {
    Ok(c.query_opt(
        "SELECT id, barkod, tebligat_turu, elektronik_tebligat, ptt_durumu, son_islem_tarihi, evrak_tarihi, t212_teblig_adresi
           FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = $2",
        &[&dosya, &birim_evrak_no],
    )
    .map_err(hata)?
    .map(|s| BilinenTebligat {
        id: s.get(0),
        barkod: s.get(1),
        tebligat_turu: s.get(2),
        elektronik_tebligat: s.get(3),
        ptt_durumu: s.get(4),
        son_islem_tarihi: s.get(5),
        evrak_tarihi: s.get(6),
        teblig_adresi: s.get(7),
    }))
}

/// Tebligat sonucunu (dosya, evrak no) anahtarıyla yazar; ham tarihlerden gerçek tarihleri türetir
/// ve PTT hareketlerini ayrı tabloya yazar. Döner: kaydın kimliği.
pub fn tebligat_kaydet<C: GenericClient>(c: &mut C, dosya: i64, birim_evrak_no: i32, k: &TebligatKaydi) -> Result<i64, String> {
    let sozluk = k.ham_yanit.as_ref().filter(|h| h.is_object());
    let ptt_hatali = sozluk.is_some_and(|h| teb::dolu(h.get("_hata")));
    let mut tx = c.transaction().map_err(hata)?;
    let id: i64 = tx
        .query_one(
            "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no) VALUES ($1, $2)
             ON CONFLICT ON CONSTRAINT uq_dosya_tebligat_barkod DO UPDATE SET sorgu_zamani = now() RETURNING id",
            &[&dosya, &birim_evrak_no],
        )
        .map_err(hata)?
        .get(0);
    // E-tebliğ mazbatası evrak listesinde açıklamasız gelir; elle işlenmişse ("Kapalı E-Tebliğ Mazbatası …") listede
    // görünmüyor diye silinmez.
    tx.execute(
        "UPDATE tebligat_barkod SET evrak_aciklama = $2, evrak_tarihi = $3, barkod = $4, elektronik_tebligat = $5,
                tebligat_turu = $6,
                tebligat_mazbatasi_var = $7 OR (tebligat_mazbatasi_var AND tebligat_mazbatasi_aciklama LIKE 'Kapalı E-Tebliğ Mazbatası%'),
                tebligat_mazbatasi_aciklama = CASE WHEN NOT $7 AND tebligat_mazbatasi_var
                                                        AND tebligat_mazbatasi_aciklama LIKE 'Kapalı E-Tebliğ Mazbatası%'
                                                   THEN tebligat_mazbatasi_aciklama ELSE $8 END,
                kapali_tebligat_mazbatasi_var = $9, borclu_id = COALESCE($10, borclu_id),
                yeniden_teblig_talep_edildi = $11, sorgu_zamani = now(),
                zarf_icerigi = CASE WHEN $12 = '' THEN zarf_icerigi ELSE $12 END,
                haciz_sirasi = CASE WHEN haciz_sirasi_elle THEN haciz_sirasi ELSE COALESCE($13, haciz_sirasi) END,
                muhatap_adi = $14, muhatap_turu = $15,
                t212_teblig_adresi = CASE WHEN $16 = '' THEN t212_teblig_adresi ELSE $16 END
          WHERE id = $1",
        &[
            &id,
            &kirp(&k.evrak_aciklama, 500),
            &kirp(&k.evrak_tarihi, 40),
            &kirp(&k.barkod, 32),
            &k.elektronik_tebligat,
            &kirp(&k.tebligat_turu, 120),
            &k.tebligat_mazbatasi_var,
            &kirp(&k.tebligat_mazbatasi_aciklama, 500),
            &k.kapali_tebligat_mazbatasi_var,
            &k.borclu,
            &k.yeniden_teblig_talep_edildi,
            &kirp(&k.zarf_icerigi, 500),
            &k.haciz_sirasi,
            &kirp(&teb::muhatap_adi(&k.evrak_aciklama), 255),
            &teb::muhatap_turu(&k.evrak_aciklama).kod(),
            &k.teblig_adresi,
        ],
    )
    .map_err(hata)?;
    if !ptt_hatali {
        ptt_alanlarini_yaz(&mut tx, id, &k.ptt_durumu, &k.son_islem_tarihi, sozluk)?;
    }
    veri::ham_tarihleri_turet(&mut tx, "tebligat_barkod", id)?;
    tx.commit().map_err(hata)?;
    Ok(id)
}

fn ptt_alanlarini_yaz(tx: &mut postgres::Transaction, id: i64, durum: &str, tarih: &str, sozluk: Option<&Value>) -> Result<(), String> {
    tx.execute(
        "UPDATE tebligat_barkod SET ptt_durumu = $2, son_islem_tarihi = $3, ham_yanit = $4 WHERE id = $1",
        &[&id, &kirp(durum, 255), &kirp(tarih, 40), &sozluk],
    )
    .map_err(hata)?;
    if let Some(h) = sozluk {
        tx.execute("UPDATE tebligat_barkod SET teslim_sekli = $2 WHERE id = $1", &[&id, &kirp(&teb::ptt_teslim_sekli(h), 150)])
            .map_err(hata)?;
        veri::ptt_hareketlerini_yaz(tx, id, h)?;
    }
    Ok(())
}

/// "Yalnız PTT" güncellemesi: kayıtlı tebligatın yalnız PTT alanlarını yazar (UYAP alanlarına
/// dokunmaz). Borçlu bağı boşsa PTT alıcısından bulunanla doldurulur. `_hata` taşıyan yanıt yazılmaz.
pub fn ptt_sonucu_kaydet<C: GenericClient>(c: &mut C, id: i64, ham: &Value, borclu: Option<i64>) -> Result<(), String> {
    if !ham.is_object() || teb::dolu(ham.get("_hata")) {
        return Ok(());
    }
    let (durum, tarih) = teb::ptt_durum_ve_tarih(ham);
    let mut tx = c.transaction().map_err(hata)?;
    ptt_alanlarini_yaz(&mut tx, id, &durum, &tarih, Some(ham))?;
    tx.execute(
        "UPDATE tebligat_barkod SET borclu_id = COALESCE(borclu_id, $2), sorgu_zamani = now() WHERE id = $1",
        &[&id, &borclu],
    )
    .map_err(hata)?;
    veri::ham_tarihleri_turet(&mut tx, "tebligat_barkod", id)?;
    tx.commit().map_err(hata)?;
    Ok(())
}

// ── Tür eşleştirmeleri ve MTS mükerrerleri ──────────────────────────────────────────────────────

/// Kullanıcının "tür eşleştir" kararları: anahtar → tür (büro geneli).
pub fn tur_eslesmeleri<C: GenericClient>(c: &mut C) -> Result<HashMap<String, String>, String> {
    Ok(c.query("SELECT anahtar, tur FROM tebligat_tur_eslesmesi", &[]).map_err(hata)?.iter().map(|s| (s.get(0), s.get(1))).collect())
}

/// Kullanılabilir türler: temel türler + kullanıcının ekledikleri.
pub fn tur_kategorileri<C: GenericClient>(c: &mut C) -> Result<Vec<String>, String> {
    let mut tum: Vec<String> = teb::TEMEL_TURLER.iter().map(|t| t.to_string()).collect();
    for s in c.query("SELECT ad FROM tebligat_tur_kategorisi ORDER BY ad", &[]).map_err(hata)? {
        let ad: String = s.get(0);
        if !tum.contains(&ad) {
            tum.push(ad);
        }
    }
    Ok(tum)
}

const MTS_MUKERRER_ANAHTARI: &str = "mts_mukerrer";

/// MTS dosyalarında aynı barkodu taşıyan ikinci evraklar ("dosya:evrak no"). Bunların PDF'i
/// sonraki turlarda yeniden indirilmez.
pub fn mts_mukerrerler<C: GenericClient>(c: &mut C) -> Result<HashSet<String>, String> {
    let s = c.query_opt("SELECT deger FROM buro_ayar WHERE anahtar = $1", &[&MTS_MUKERRER_ANAHTARI]).map_err(hata)?;
    Ok(s.map(|s| s.get::<_, Value>(0)).and_then(|v| v.as_object().map(|o| o.keys().cloned().collect())).unwrap_or_default())
}

pub fn mts_mukerrer_ekle<C: GenericClient>(c: &mut C, anahtar: &str) -> Result<(), String> {
    c.execute(
        "INSERT INTO buro_ayar (anahtar, deger) VALUES ($1, jsonb_build_object($2::text, true))
         ON CONFLICT (anahtar) DO UPDATE SET deger = buro_ayar.deger || EXCLUDED.deger, guncellendi = now()",
        &[&MTS_MUKERRER_ANAHTARI, &anahtar],
    )
    .map_err(hata)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn takip_adi_ve_ornek_no() {
        let ad = takip_adi(" ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49 ");
        assert_eq!(ad, "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49");
        assert_eq!(ornek_no(&ad), "7");
        assert_eq!(ornek_no("ÖRNEK: 4-5 Para Borcuna Veya Teminat Verilmesine … - Eski No: 53"), "4-5");
        assert_eq!(ornek_no("Örnek No: 10 Kambiyo Senetlerine Mahsus Haciz Yoluyla Takiplerde Ödeme Emri"), "10");
        assert_eq!(ornek_no("Genel Haciz Yoluyla Takip"), "");
    }

    #[test]
    fn takip_secenekleri_ayristirma() {
        let v = json!([{"value": 1, "name": " İlamsız Takip "}, {"value": "0", "name": "İlamlı Takip"}, {"name": "kodsuz"}, {"value": "123456789", "name": "uzun"}]);
        assert_eq!(takip_secenekleri(&v), vec![("1".to_string(), "İlamsız Takip".to_string()), ("0".to_string(), "İlamlı Takip".to_string())]);
        assert!(takip_secenekleri(&json!({"errorCode": "X"})).is_empty());
    }
}
