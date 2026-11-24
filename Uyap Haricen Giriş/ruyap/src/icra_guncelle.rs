//! İcra dosyalarını UYAP'tan güncelleme (Python `UyapMasa/modules/icra_core.py::IcraSorgu.ara` portu).
//!
//! Dört ayrı iş (kullanıcı isteği 18.11: "bir buton dosya listesini, bir buton taraf bilgilerini sorgulasın"; 19.11 tebligat):
//!   • `liste`: 1 → 2 → 4 → 5 → 6 (taraf istenmez; tarafı eksik dosya sayısı rapora yazılır)
//!   • `taraf`: 1 → 2 → 3 → 5 → 6 (UYAP dosya kimliği oturumluk olduğu için liste yine alınır; kapanan kontrolü yok)
//!   • `tebligat`: 1 → 2 → 3' → T → 5 → 6. 3': yalnız alacaklısı / borçlusu HİÇ olmayan dosyaların tarafları (tebligatın
//!     hangi borçluya ait olduğu borçlu listesinden bulunur; kimliksiz borçlu için istek atılmaz). T: her dosyanın evrak
//!     listesi → tebligat evrakları → gerekirse PDF (barkod, tür) → gerekirse PTT → kayıt (`icra_tarama` kararları:
//!     barkodu bilinen evrakın PDF'i yeniden indirilmez, sonucu kesin barkod PTT'ye sorulmaz, e-tebligat PTT'ye gitmez).
//!     `ptt: false` ile yalnız UYAP verisi alınır (kayıtlı PTT durumu korunur).
//!   • `ptt` (yalnız PTT): UYAP'a HİÇ gidilmez; kapsamdaki açık dosyaların sonuçlanmamış fiziki tebligatları PTT'ye sorulur → 5 → 6.
//!   PTT art arda yanıt vermezse o tur PTT bırakılır; UYAP kısmı sürer.
//! Kapsam: bütün açık dosyalar ya da tek daire; isteğe bağlı açılış tarihi aralığı (UYAP `dosyaAcilisTarihiStart/End`).
//! Tarih aralığı verilince kapanan dosya kontrolü yapılmaz (aralık dışı açık dosyalar "listede yok" sayılırdı).
//!
//! Adımlar (Python ile aynı sıra):
//!   1. Liste: `search_phrase_detayli` ile UYAP'taki AÇIK icra dosyaları, 500'lük sayfalar. Hata zarfı ya da
//!      tükenmeyen "eş zamanlı sorgulama" reddi "son sayfa" SANILMAZ: tarama eksik işaretlenir.
//!   2. Künye: her kayıt `icra_veri::dosya_kunyesi_kaydet` ile eklenir / güncellenir (kalıcı anahtar).
//!   3. Taraflar: alacaklısı ya da borçlusu olmayan dosyalarda borçlular `dosya_borclu_list` (TCKN'li) + diğer roller
//!      `dosya_taraf_bilgileri_brd`; yalnız borçlusu kimliksiz (TCKN / vergi no yok) dosyalarda yalnız borçlu listesi.
//!      Tarafı tam dosyaya istek atılmaz.
//!   4. Kapanan dosyalar: liste eksiksizse, veritabanında açık olup UYAP'ın açık listesinde olmayanlar Kapalı
//!      listede TEK TEK doğrulanır. Bulunamayana dokunulmaz. Aday sayısı beklenmeyecek kadar çoksa adım atlanır.
//!   5. Sayım raporu (UYAP toplamı ↔ alınan ↔ yazılan ↔ veritabanındaki açık dosya).
//!   6. Kesinleşme / tebliğ / takip aşaması yeniden hesaplanır (`icra_durum::durumlari_guncelle`; L7 işleri de).
//!
//! UYAP'A SAYGI: her istek `evrak_toplu::_uyap` kapısından (eş zamanlı red tekrarı, ağ geri çekilmesi, yeniden
//! giriş, doğrulamayı KULLANICI çözer, kullanıcının tıklamaları önce); sabit hız ve düzenli mola; Tüm Evrakı
//! İndir ve Gerekçeli Kararlar ile aynı sıra kilidi.
//!
//!   POST /__uyap_agent__/icra_guncelle/dosya      {"dosya": id, …} ya da {"uyap_dosya_id", "evraklar": [no]} (dosya
//!                                                  içinden "Dosyaya tebligat ekle": yalnız seçilen tebligat + PTT)
//!   POST /__uyap_agent__/icra_guncelle            {"mod": "liste"|"taraf"|"tebligat"|"ptt", "ptt"?: bool (tebligat; varsayılan
//!                                                  true), "birim_id"?, "birim_adi"?, "acilis_bas"?, "acilis_son"?}
//!                                                  → 201 {tarama} (süren varsa o döner)
//!   GET  /__uyap_agent__/icra_guncelle            → {tarama|null} (süren ya da son tarama)
//!   GET  /__uyap_agent__/icra_guncelle/<id>       → {tarama}
//!   POST /__uyap_agent__/icra_guncelle/<id>/iptal → {tarama}
//!   POST /__uyap_agent__/icra_guncelle/<id>/captcha {"token"|null} → {tarama}

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use chrono::NaiveDate;
use serde_json::{json, Value};

use crate::evrak_toplu::{self, Indirme, BITTI, CALISIYOR, CAPTCHA, CIKTI_TEK, HATA, IPTAL, KAPSAM_TUM, SIRADA};
use crate::icra_kayit as kayit;
use crate::icra_tarama::{self as tarama, Adim};
use crate::icra_tebligat as teb;
use crate::icra_veri::{self as veri, Rol, YargiTuru};

pub const ON_EK: &str = "/__uyap_agent__/icra_guncelle";
pub const ISTEK_ARASI_SN: f64 = 3.0;
pub const YAVAS_EN_COK_SN: f64 = 15.0;
pub const MOLA_HER_DOSYA: usize = 50;
pub const MOLA_SN: u64 = 60;
pub const SAYFA_BOYU: usize = 500;
pub const SAYFA_EN_COK: usize = 200;
pub const ARDISIK_AG_HATASI_SINIRI: usize = 3;
/// PTT bu kadar barkodda art arda (yeniden denemeye rağmen) yanıt vermezse o tur PTT bırakılır.
pub const PTT_HATA_SINIRI: usize = 3;
/// Başarısız PTT sorgusu bu kadar beklenip bir kez yeniden denenir.
pub const PTT_TEKRAR_SN: u64 = 10;
/// Bitmiş tarama bu kadar süre sonra kayıttan düşer (sonucu ekranda görülebilsin diye bir süre tutulur).
pub const SAKLAMA_SN: f64 = 6.0 * 3600.0;

pub const ARAMA: &str = "search_phrase_detayli.ajx";
pub const BORCLU_LISTESI: &str = "dosya_borclu_list.ajx";
pub const TARAF_BILGILERI: &str = crate::gerekceli_karar::TARAF_BILGILERI;
/// Dosyanın takip türü / yolu / şekli (adlarıyla) ve alacak toplamları.
pub const DOSYA_AYRINTI: &str = "dosyaAyrintiBilgileri_brd.ajx";
/// UYAP takip açılış ekranının seçenek listeleri: tür → yol (takipTuru) → şekil (takipTuru, takipYolu).
pub const TAKIP_TURU_LISTESI: &str = "icra_takip_turu.ajx";
pub const TAKIP_YOLU_LISTESI: &str = "icra_takip_yolu.ajx";
pub const TAKIP_SEKLI_LISTESI: &str = "icra_takip_sekli.ajx";
/// İcra Dairesi / İcra (Python `sgk_sorgu_gui.BIRIM_TURU2/3`).
pub const BIRIM_TURU2: &str = "1101";
pub const BIRIM_TURU3: &str = "2";

const IPTAL_HATASI: &str = "__IPTAL__";
pub(crate) const AG_ONEKI: &str = "AG_HATASI:";
const SON: &[&str] = &[BITTI, HATA, IPTAL];

pub(crate) fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// UYAP'a (ve PTT'ye) istek atan katman. Gerçekte `evrak_toplu::_uyap` / `ptt_sorgu`; testte hazır yanıt veren sahte.
pub trait Uyap: Send + Sync {
    /// POST; dönen değer yanıtın JSON'u (JSON değilse None).
    fn post(&self, uc: &'static str, govde: Value) -> impl Future<Output = Result<Option<Value>, String>> + Send;
    /// GET ile ham içerik (evrak indirme): (HTTP durumu, bayt).
    fn indir(&self, uc: &'static str, params: HashMap<String, String>) -> impl Future<Output = Result<(u16, Vec<u8>), String>> + Send;
    /// PTT gönderi takibi: ham yanıt (PTT listesinin ilk elemanı).
    fn ptt(&self, barkod: String) -> impl Future<Output = Result<Value, String>> + Send;
    /// POST, ham yanıt (talep evrakı UDF'si gibi ikili içerik): (HTTP durumu, bayt).
    fn post_ham(&self, uc: &'static str, govde: Value) -> impl Future<Output = Result<(u16, Vec<u8>), String>> + Send;
    /// UDF'yi girişteki e-imza kartı ve PIN'iyle imzalar.
    fn imzala(&self, udf: Vec<u8>, ad: String) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
    /// Çok parçalı POST (talep gönderimi; yeniden denenmez): yanıtın JSON'u.
    fn yukle(&self, uc: &'static str, alanlar: Vec<(String, Option<String>, Vec<u8>)>) -> impl Future<Output = Result<Option<Value>, String>> + Send;
}

pub(crate) struct GercekUyap(pub(crate) Arc<Indirme>);

impl Uyap for GercekUyap {
    fn post(&self, uc: &'static str, govde: Value) -> impl Future<Output = Result<Option<Value>, String>> + Send {
        let ind = self.0.clone();
        async move { evrak_toplu::_uyap(&ind, "POST", uc, Some(&govde), None).await.map(|(_, _, _, v)| v) }
    }
    fn indir(&self, uc: &'static str, params: HashMap<String, String>) -> impl Future<Output = Result<(u16, Vec<u8>), String>> + Send {
        let ind = self.0.clone();
        async move { evrak_toplu::_uyap(&ind, "GET", uc, None, Some(&params)).await.map(|(st, _, b, _)| (st, b)) }
    }
    fn ptt(&self, barkod: String) -> impl Future<Output = Result<Value, String>> + Send {
        async move { crate::ptt_sorgu::toplu_sorgula(&barkod).await }
    }
    fn post_ham(&self, uc: &'static str, govde: Value) -> impl Future<Output = Result<(u16, Vec<u8>), String>> + Send {
        let ind = self.0.clone();
        async move { evrak_toplu::_uyap(&ind, "POST", uc, Some(&govde), None).await.map(|(st, _, b, _)| (st, b)) }
    }
    fn imzala(&self, udf: Vec<u8>, ad: String) -> impl Future<Output = Result<Vec<u8>, String>> + Send {
        async move {
            let gw = crate::uyap_proxy::get_gw().ok_or("UYAP oturumu hazır değil.")?;
            let pin = gw.login_args.pin.clone().unwrap_or_default();
            let sertifika = gw.cert_id.lock().clone();
            tokio::task::spawn_blocking(move || crate::udf_signer::sign_document(&udf, &ad, sertifika.as_deref(), &pin))
                .await
                .map_err(|e| format!("İmzalama tamamlanamadı: {e}"))?
                .map_err(|e| format!("E-imza: {}", e.0))
        }
    }
    fn yukle(&self, uc: &'static str, alanlar: Vec<(String, Option<String>, Vec<u8>)>) -> impl Future<Output = Result<Option<Value>, String>> + Send {
        let ind = self.0.clone();
        async move { evrak_toplu::_uyap_multipart(&ind, uc, alanlar).await.map(|(_, v)| v) }
    }
}

// ── Saf yardımcılar ─────────────────────────────────────────────────────────────────────────────

/// Sayfa yanıtı gerçek bir kayıt listesi değilse nedeni: boş / JSON değil, UYAP hata zarfı ya da tekrarlara
/// rağmen süren "eş zamanlı sorgulama" reddi. Bunlar eskiden "0/1 kayıtlık son sayfa" sanılıyordu.
pub fn sayfa_gecersiz(veri: &Option<Value>) -> Option<String> {
    let Some(v) = veri else {
        return Some("UYAP boş ya da JSON olmayan yanıt verdi".into());
    };
    if evrak_toplu::eszamanli_mi(v, 0) {
        return Some("UYAP 'eş zamanlı sorgulama' reddi tekrarlara rağmen sürdü".into());
    }
    if v.is_object() {
        let (aciklama, kod) = (teb::metin(v, "error"), teb::metin(v, "errorCode"));
        if !aciklama.is_empty() || !kod.is_empty() {
            let ek = if !aciklama.is_empty() && !kod.is_empty() { format!(" [{kod}]") } else { String::new() };
            return Some(format!("UYAP hatası: {}{ek}", if aciklama.is_empty() { kod.trim() } else { aciklama.trim() }));
        }
    }
    None
}

/// `[kayıtlar, toplam]` biçiminde UYAP'ın bildirdiği toplam; yoksa None (uydurulmaz).
pub fn uyap_toplami(veri: &Value) -> Option<i64> {
    let a = veri.as_array()?;
    if a.len() < 2 || !a[0].is_array() {
        return None;
    }
    match &a[1] {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Python `sayim_satiri`: tek kapsamın okunur sayım satırı + eksik mi.
pub fn sayim_satiri(etiket: &str, alinan: usize, uyap_toplam: Option<i64>, tam: bool) -> (String, bool) {
    if !tam {
        return (format!("⚠️ {etiket}: {alinan} dosya alındı — tarama YARIM kaldı (bir sayfa alınamadı), liste EKSİK olabilir."), true);
    }
    match uyap_toplam {
        None => (format!("… {etiket}: {alinan} dosya alındı (UYAP toplam sayı bildirmedi — karşılaştırılamadı)."), false),
        Some(t) if t == alinan as i64 => (format!("✔ {etiket}: UYAP {t} dosya bildirdi, {alinan} dosyanın tamamı alındı."), false),
        Some(t) => (format!("⚠️ {etiket}: UYAP {t} dosya bildirdi ama {alinan} alındı — {} fark var.", (t - alinan as i64).abs()), true),
    }
}

/// Kapanan dosya kontrolünün emniyet freni: aday sayısı görülen açık dosyalardan (en az 50) fazlaysa liste
/// şüphelidir (UYAP boş / eksik yanıt vermiş olabilir) — hiçbir dosya kapatılmaz.
pub fn fren_asildi(aday: usize, gorulen: usize) -> bool {
    aday > gorulen.max(50)
}

/// Güncellemenin türü: dosya listesi, taraf bilgileri, tebligat bilgileri ya da yalnız PTT (ayrı düğmeler).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mod {
    #[default]
    Liste,
    Taraf,
    Tebligat,
    Ptt,
    /// Dosya panelinden tek dosya: künye (+ dosya no / durum), istenirse taraflar ve tebligatlar (tümü ya da seçilenler).
    Dosya,
    /// Takip türü / yolu / şekli henüz okunmamış dosyaların ayrıntısı (`dosyaAyrintiBilgileri_brd`, dosya başına 1 istek).
    Takip,
    /// UYAP takip açılış ekranının tür → yol → şekil listesi (kesinleşme süreleri adlarla seçilsin diye; birkaç on istek).
    TakipListesi,
    /// Dosya Listesi sekmesi "Tüm verileri güncelle" (20.11.2026): dosya listesi (yeni / kapanan dahil) + eksik taraflar
    /// + okunmamış takip türleri. Taraf ve takip türü `Kapsam::hedefler` ile sınırlanabilir (yeni dosyalar hep dahil).
    Hepsi,
    /// Kesinleştirme sekmesi (22.11.2026): seçili dosyalarda takibi UYAP'ta kesinleşmiş borçlular (dosya başına 1 istek;
    /// `icra_kesinlestirme::kesinlesme_oku`).
    Kesinlesme,
    /// Kesinleştirme sekmesi: seçili borçlulara "Takibin Kesinleşmesi" genel talebi, e-imzalı (`Kapsam::borclular`;
    /// `icra_kesinlestirme::talepleri_gonder`).
    Talep,
    /// Seçili borçluların Mernis adresi + ilk ödeme emri zarfı adresiyle kıyas (`icra_mernis::mernis_sorgula`).
    Mernis,
    /// Seçili borçlulara yeniden tebliğ talebi (21/2 ya da normal; ÜCRETLİ; `icra_mernis::talepleri_gonder`).
    YenidenTeblig,
}

impl Mod {
    pub fn kod(self) -> &'static str {
        match self {
            Mod::Liste => "liste",
            Mod::Taraf => "taraf",
            Mod::Tebligat => "tebligat",
            Mod::Ptt => "ptt",
            Mod::Dosya => "dosya",
            Mod::Takip => "takip",
            Mod::TakipListesi => "takip_listesi",
            Mod::Hepsi => "hepsi",
            Mod::Kesinlesme => "kesinlesme",
            Mod::Talep => "kesinlestirme_talebi",
            Mod::Mernis => "mernis",
            Mod::YenidenTeblig => "yeniden_teblig",
        }
    }
    pub fn coz(s: &str) -> Option<Mod> {
        match s.trim() {
            "" | "liste" => Some(Mod::Liste),
            "taraf" => Some(Mod::Taraf),
            "tebligat" => Some(Mod::Tebligat),
            "ptt" => Some(Mod::Ptt),
            "takip" => Some(Mod::Takip),
            "takip_listesi" => Some(Mod::TakipListesi),
            "hepsi" => Some(Mod::Hepsi),
            "kesinlesme" => Some(Mod::Kesinlesme),
            "kesinlestirme_talebi" => Some(Mod::Talep),
            "mernis" => Some(Mod::Mernis),
            "yeniden_teblig" => Some(Mod::YenidenTeblig),
            _ => None,
        }
    }
}

/// Taramanın kapsamı: daire (yoksa bütün açık dosyalar) ve açılış tarihi aralığı.
#[derive(Debug, Clone, Default)]
pub struct Kapsam {
    pub birim_id: Option<String>,
    pub birim_adi: String,
    pub acilis_bas: Option<NaiveDate>,
    pub acilis_son: Option<NaiveDate>,
    /// Listede seçilen dosyalar (veritabanı kimlikleri). Varsa daire / tarih yerine yalnız bunlara işlem yapılır.
    pub dosyalar: Option<Vec<i64>>,
    /// Yalnız `Mod::Hepsi`: taraf ve takip türü istekleri bu dosyalarla (ve bu turda yeni gelenlerle) sınırlı.
    /// Yok = kapsamdaki bütün dosyalar.
    pub hedefler: Option<Vec<i64>>,
    /// `Mod::Talep` / `Mod::Mernis` / `Mod::YenidenTeblig`: işlem yapılacak borçlular (`dosya_taraf` kimlikleri; dosyaları
    /// `dosyalar`da).
    pub borclular: Option<Vec<i64>>,
    /// Yalnız `Mod::Mernis`: aynı kişinin başka dosyadaki yakın tarihli sonucu kullanılmaz, UYAP'a yeniden sorulur.
    pub yeniden: bool,
}

impl Kapsam {
    pub fn tarihli(&self) -> bool {
        self.acilis_bas.is_some() || self.acilis_son.is_some()
    }
    pub fn metin(&self) -> String {
        if let Some(d) = &self.dosyalar {
            return format!("Seçili {} dosya", d.len());
        }
        let mut m = if self.birim_id.is_some() {
            if self.birim_adi.is_empty() { format!("Birim {}", self.birim_id.as_deref().unwrap_or("")) } else { self.birim_adi.clone() }
        } else {
            "Bütün açık dosyalar".to_string()
        };
        if self.tarihli() {
            let g = |d: Option<NaiveDate>| d.map(|d| d.format("%d.%m.%Y").to_string()).unwrap_or_else(|| "…".into());
            m.push_str(&format!(" · açılış {}–{}", g(self.acilis_bas), g(self.acilis_son)));
        }
        m
    }
}

fn liste_govdesi(sayfa: usize, boy: usize, k: &Kapsam) -> Value {
    let mut g = json!({"dosyaDurumKod": 0, "pageSize": boy, "pageNumber": sayfa,
                       "birimTuru2": BIRIM_TURU2, "birimTuru3": BIRIM_TURU3});
    if let Some(b) = &k.birim_id {
        g["birimId"] = json!(b);
    }
    if let Some(d) = k.acilis_bas {
        g["dosyaAcilisTarihiStart"] = json!(d.format("%d.%m.%Y").to_string());
    }
    if let Some(d) = k.acilis_son {
        g["dosyaAcilisTarihiEnd"] = json!(d.format("%d.%m.%Y").to_string());
    }
    g
}

// ── Tarama durumu ───────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct TaramaInner {
    /// liste | kunye | taraf | kapanan | hesap | bitti
    pub asama: String,
    pub mod_: Mod,
    pub kapsam: Kapsam,
    /// Liste güncellemesinde tarafı eksik kalan dosya sayısı (taraf düğmesine yönlendirmek için).
    pub taraf_eksik: usize,
    pub i: usize,
    pub toplam: usize,
    pub uyap_toplam: Option<i64>,
    pub alinan: usize,
    pub yazilan: usize,
    pub yeni_dosya: usize,
    pub taraf_cekilen: usize,
    pub aday: usize,
    pub kapandi: usize,
    pub bulunamadi: usize,
    /// Liste eksiksiz alındı mı.
    pub tam: bool,
    pub guncellenen: usize,
    pub otomatik_is: usize,
    pub mola_bitis: Option<f64>,
    pub uyarilar: Vec<String>,
    pub rapor: Vec<String>,
    // ── Tebligat / yalnız PTT ──
    /// Tebligat güncellemesinde PTT de sorulsun mu (false: yalnız UYAP verisi).
    pub ptt_sorgusu: bool,
    /// PTT art arda yanıt vermediği için bu tur bırakıldı.
    pub ptt_kapandi: bool,
    /// Evrak listesi alınan dosya.
    pub teb_dosya: usize,
    pub pdf_indirilen: usize,
    /// PTT'ye sorulan barkod (yeniden deneme bir sayılır).
    pub ptt_sorulan: usize,
    pub ptt_hatasi: usize,
    pub teb_yazilan: usize,
    pub teb_yeni: usize,
    /// PTT / tebliğ durumu değişen kayıtlı tebligat.
    pub teb_degisen: usize,
    pub teb_atlanan: usize,
    /// Taraf adımından sonra hâlâ borçlusu olmayan dosyalar (etiket).
    pub borclusuz: Vec<String>,
    // ── Tek dosya (Mod::Dosya) ──
    pub dosya: Option<i64>,
    pub dosya_etiket: String,
    pub dosya_taraf: bool,
    pub dosya_tebligat: bool,
    /// Tebligattan sonra gerekiyorsa kesinleşme UYAP'tan okunur ve talep adayları çıkarılır (panel "Veritabanını güncelle").
    pub dosya_kesinlesme: bool,
    /// Tek dosya işinde UYAP'ta bulunan dosya kimliği (kesinleşme adımı için).
    pub dosya_uyap_kimligi: Option<String>,
    /// Kesinleştirme talebi için kullanıcıya sorulacak borçlular: {bag, borclu, grup_etiket}.
    pub talep_adaylari: Vec<Value>,
    /// Yalnız bu evrak numaralı tebligatlar (boş: dosyadaki bütün tebligatlar).
    pub evrak_secimi: Vec<i32>,
    /// Tek dosya işinde UYAP kimliği biliniyorsa (dosya içinden tebligat ekleme): arama / künye / takip türü atlanır.
    pub uyap_id: Option<String>,
    // ── Takip türü ──
    /// Takip türü (ayrıntı) okunan / okunamayan dosya.
    pub takip_okunan: usize,
    pub takip_okunamayan: usize,
    /// Dosya listesi güncellemesinde takip türü hâlâ okunmamış dosya (düğmeye yönlendirmek için).
    pub takip_eksik: usize,
    /// Takip türü listesi: alınan tür, yol, şekil sayısı.
    pub takip_listesi: (usize, usize, usize),
    // ── UYAP dosya kimlikleri (bkz. `KIMLIK_OMRU_SN`) ──
    /// Elimizdeki UYAP kimliklerinin alındığı (liste isteğinin başladığı) an; yoksa kimlikler listeden gelmedi (tek dosya).
    pub kimlik_zamani: Option<f64>,
    /// Künyesi yazılan dosyanın UYAP listesindeki anahtarı (yenilemede yeni kimlik bununla eşlenir).
    pub kimlik_anahtari: HashMap<i64, KimlikAnahtari>,
    /// Yenilemeyle gelen güncel UYAP kimlikleri (veritabanı kimliği → dosyaId).
    pub taze_kimlik: HashMap<i64, String>,
    /// Kimliklerin kaç kez yenilendiği.
    pub kimlik_tazeleme: usize,
    // ── Kesinleştirme (Mod::Kesinlesme / Mod::Talep) ──
    /// Kesinleşmesi UYAP'tan okunan dosya.
    pub kes_dosya: usize,
    /// UYAP'ta takibi kesinleşmiş görülen borçlu.
    pub kes_kesinlesen: usize,
    /// UYAP'ta kesinleşme görülünce otomatik kapatılan iş (İş Listesi).
    pub kes_kapanan_is: usize,
    pub talep_gonderilen: usize,
    /// UYAP borçlunun zaten kesinleşmiş olduğunu bildirdi (gönderilmedi).
    pub talep_kesinlesmis: usize,
    /// Talep türü UYAP'ın listesinde yok / bugün zaten gönderilmiş / başka nedenle atlanan.
    pub talep_atlanan: usize,
    pub talep_hatasi: usize,
    // ── Mernis / yeniden tebliğ (Mod::Mernis / Mod::YenidenTeblig) ──
    pub mer_sorulan: usize,
    pub mer_onbellek: usize,
    pub mer_uygun: usize,
    pub mer_farkli: usize,
    pub mer_elle: usize,
    pub mer_hata: usize,
    pub yt_gonderilen: usize,
    pub yt_atlanan: usize,
    pub yt_hatasi: usize,
    pub yt_tutar: f64,
}

/// `evrak_toplu::Indirme`'nin iptal / doğrulama / yavaşlatma davranışını aynen kullanır (durum, mesaj, captcha
/// orada); yalnız hız ve akış farklıdır.
pub struct Tarama {
    pub ind: Arc<Indirme>,
    pub inner: StdMutex<TaramaInner>,
    pub sayfa_boyu: usize,
    pub mola_her: usize,
    pub mola_sn: u64,
    pub ptt_tekrar_sn: u64,
    /// Bu kadar saniye dolunca UYAP kimlikleri bir sonraki dosyadan önce yenilenir.
    pub kimlik_omru_sn: f64,
}

impl Tarama {
    /// PTT'ye bu an gidilebilir mi (tür, kullanıcı seçimi, art arda hata).
    fn ptt_acik(&self) -> bool {
        let g = self.inner.lock().unwrap();
        !g.ptt_kapandi && (g.mod_ == Mod::Ptt || (matches!(g.mod_, Mod::Tebligat | Mod::Dosya) && g.ptt_sorgusu))
    }

    pub fn new(mod_: Mod, kapsam: Kapsam) -> Arc<Self> {
        let ind = Indirme::new(String::new(), "İcra güncelleme".into(), CIKTI_TEK.into(), KAPSAM_TUM.into(), None);
        {
            let mut g = ind.inner.lock().unwrap();
            g.istek_arasi_sn = ISTEK_ARASI_SN;
            g.yavas_en_cok_sn = YAVAS_EN_COK_SN;
            g.mesaj = "Sırada bekliyor…".into();
        }
        Arc::new(Self {
            ind,
            inner: StdMutex::new(TaramaInner {
                asama: match mod_ {
                    Mod::Ptt => "ptt",
                    Mod::Dosya => "dosya",
                    _ => "liste",
                }
                .into(),
                mod_,
                kapsam,
                tam: true,
                ptt_sorgusu: true,
                ..Default::default()
            }),
            sayfa_boyu: SAYFA_BOYU,
            mola_her: MOLA_HER_DOSYA,
            mola_sn: MOLA_SN,
            ptt_tekrar_sn: PTT_TEKRAR_SN,
            kimlik_omru_sn: KIMLIK_OMRU_SN,
        })
    }

    pub fn id(&self) -> String {
        self.ind.inner.lock().unwrap().id.clone()
    }

    pub fn durum(&self) -> String {
        self.ind.inner.lock().unwrap().durum.clone()
    }

    pub(crate) fn mesaj_yaz(&self, m: impl Into<String>) {
        self.ind.inner.lock().unwrap().mesaj = m.into();
    }

    fn durum_mesaj(&self, durum: &str, mesaj: impl Into<String>) {
        let mut g = self.ind.inner.lock().unwrap();
        g.durum = durum.to_string();
        g.mesaj = mesaj.into();
    }

    pub(crate) fn uyari(&self, u: impl Into<String>) {
        self.inner.lock().unwrap().uyarilar.push(u.into());
    }

    pub(crate) fn asama(&self, a: &str, toplam: usize) {
        let mut g = self.inner.lock().unwrap();
        g.asama = a.to_string();
        g.i = 0;
        g.toplam = toplam;
    }

    pub(crate) fn ilerle(&self) {
        self.inner.lock().unwrap().i += 1;
    }

    fn bitti_mi(&self) -> bool {
        SON.contains(&self.durum().as_str())
    }

    pub fn sozluk(&self) -> Value {
        let (id, durum, mesaj, captcha, baslangic, bitis) = {
            let g = self.ind.inner.lock().unwrap();
            (g.id.clone(), g.durum.clone(), g.mesaj.clone(), g.captcha.clone(), g.baslangic, g.bitis)
        };
        let aralik = self.ind.aralik();
        let g = self.inner.lock().unwrap();
        let kalan = g.toplam.saturating_sub(g.i);
        // taraf: dosya başına 2 istek; kapanan: 1 istek; tebligat: ~3 istek (evrak listesi, ara sıra PDF);
        // PTT: barkod başına ~10 sn (sayfa yükleme + bekleme). Mola süresi eklenir.
        let mola = |n: usize| (n / self.mola_her.max(1)) as f64 * self.mola_sn as f64;
        let kalan_sn = match (g.asama.as_str(), SON.contains(&durum.as_str())) {
            (_, true) => None,
            ("taraf", _) => Some((kalan as f64 * 2.0 * aralik + mola(kalan)) as i64),
            ("kapanan" | "arama", _) => Some((kalan as f64 * aralik + mola(kalan)) as i64),
            ("tebligat", _) => Some((kalan as f64 * 3.0 * aralik + mola(kalan)) as i64),
            ("takip" | "kesinlesme", _) => Some((kalan as f64 * aralik + mola(kalan)) as i64),
            // Talep: borçlu listesi, kesinleşme denetimi, talep türleri, evrak hazırlama, gönderim (+ imza).
            ("talep", _) => Some((kalan as f64 * 5.0 * aralik + mola(kalan)) as i64),
            ("ptt", _) => Some((kalan as f64 * 10.0) as i64),
            _ => None,
        };
        let uyarilar: Vec<&String> = g.uyarilar.iter().skip(g.uyarilar.len().saturating_sub(20)).collect();
        let mut v = json!({
            "id": id, "durum": durum, "mesaj": mesaj, "asama": g.asama, "mod": g.mod_.kod(), "kapsam": g.kapsam.metin(),
            "birim_id": g.kapsam.birim_id, "taraf_eksik": g.taraf_eksik,
            "i": g.i, "toplam": g.toplam, "kalan_sn": kalan_sn, "aralik_sn": aralik,
            "mola_kalan_sn": g.mola_bitis.map(|b| (b - now_secs()).max(0.0) as i64),
            "uyap_toplam": g.uyap_toplam, "alinan": g.alinan, "yazilan": g.yazilan, "yeni_dosya": g.yeni_dosya,
            "taraf_cekilen": g.taraf_cekilen, "aday": g.aday, "kapandi": g.kapandi, "bulunamadi": g.bulunamadi,
            "tam": g.tam, "guncellenen": g.guncellenen, "otomatik_is": g.otomatik_is,
            "uyari_sayisi": g.uyarilar.len(), "uyarilar": uyarilar, "rapor": g.rapor, "captcha": captcha,
            "baslangic": baslangic, "bitis": bitis,
        });
        // Tebligat / tek dosya alanları ayrı eklenir (tek `json!` makro açılım sınırını aşıyordu).
        let ek = json!({
            "ptt_sorgusu": g.ptt_sorgusu, "ptt_kapandi": g.ptt_kapandi, "teb_dosya": g.teb_dosya,
            "pdf_indirilen": g.pdf_indirilen, "ptt_sorulan": g.ptt_sorulan, "ptt_hatasi": g.ptt_hatasi,
            "teb_yazilan": g.teb_yazilan, "teb_yeni": g.teb_yeni, "teb_degisen": g.teb_degisen, "teb_atlanan": g.teb_atlanan,
            "borclusuz": g.borclusuz.len(), "dosya": g.dosya, "dosya_etiket": g.dosya_etiket,
            "takip_okunan": g.takip_okunan, "takip_okunamayan": g.takip_okunamayan, "takip_eksik": g.takip_eksik,
            "takip_listesi": [g.takip_listesi.0, g.takip_listesi.1, g.takip_listesi.2],
            "kimlik_tazeleme": g.kimlik_tazeleme,
            "talep_adaylari": g.talep_adaylari, "dosya_kesinlesme": g.dosya_kesinlesme,
            "kes_dosya": g.kes_dosya, "kes_kesinlesen": g.kes_kesinlesen, "kes_kapanan_is": g.kes_kapanan_is,
            "talep_gonderilen": g.talep_gonderilen, "talep_kesinlesmis": g.talep_kesinlesmis,
            "talep_atlanan": g.talep_atlanan, "talep_hatasi": g.talep_hatasi,
        });
        let mer = json!({
            "mer_sorulan": g.mer_sorulan, "mer_onbellek": g.mer_onbellek, "mer_uygun": g.mer_uygun, "mer_farkli": g.mer_farkli,
            "mer_elle": g.mer_elle, "mer_hata": g.mer_hata, "yt_gonderilen": g.yt_gonderilen, "yt_atlanan": g.yt_atlanan,
            "yt_hatasi": g.yt_hatasi, "yt_tutar": g.yt_tutar,
        });
        for ek in [ek, mer] {
            if let (Some(a), Value::Object(b)) = (v.as_object_mut(), ek) {
                a.extend(b);
            }
        }
        v
    }
}

// ── Veritabanı (eşzamanlı istemci; ayrı iş parçacığında) ───────────────────────────────────────

pub(crate) async fn vt<R: Send + 'static>(f: impl FnOnce(&mut postgres::Client) -> Result<R, String> + Send + 'static) -> Result<R, String> {
    tokio::task::spawn_blocking(move || {
        let mut c = crate::veritabani::baglanti()?;
        f(&mut c)
    })
    .await
    .map_err(|e| format!("İşlem tamamlanamadı: {e}"))?
}

/// Künye yazımından sonra her dosya için taraf ihtiyacı.
struct TarafIsi {
    dosya: i64,
    uyap_id: String,
    etiket: String,
    /// Borçlu listesi (TCKN / vergi no veren uç): borçlusu yok ya da kimliksiz.
    borclu_listesi: bool,
    /// Taraf bilgileri (alacaklı, diğer roller ve bütün VEKİLLER): alacaklı / borçlu yok ya da taraf bilgileri hiç
    /// okunmamış (`dosya.taraf_bilgisi_zamani` boş; "Rolüm" için borçlu vekilleri de gerekir, 20.11.2026).
    taraf_bilgisi: bool,
}

// ── Akış ────────────────────────────────────────────────────────────────────────────────────────

pub(crate) async fn mola(t: &Tarama) -> Result<(), String> {
    t.inner.lock().unwrap().mola_bitis = Some(now_secs() + t.mola_sn as f64);
    let onceki = t.ind.inner.lock().unwrap().mesaj.clone();
    t.mesaj_yaz(format!("Mola: UYAP'ı yormamak için {} sn bekleniyor…", t.mola_sn));
    let mut sonuc = Ok(());
    for _ in 0..t.mola_sn {
        if let Err(e) = t.ind.iptal_kontrol() {
            sonuc = Err(e);
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    t.inner.lock().unwrap().mola_bitis = None;
    t.mesaj_yaz(onceki);
    sonuc
}

/// Tek dosyalık isteklerde ağ hatası taramayı hemen durdurmaz; art arda sınırı aşarsa durdurur.
pub(crate) fn ag_hatasi_mi(e: &str, t: &Tarama, etiket: &str, ardisik: &mut usize) -> Result<(), String> {
    let Some(ag) = e.strip_prefix(AG_ONEKI) else {
        return Err(e.to_string());
    };
    *ardisik += 1;
    t.uyari(format!("{etiket}: bağlantı koptu ({ag})"));
    if *ardisik >= ARDISIK_AG_HATASI_SINIRI {
        return Err(format!("{AG_ONEKI}art arda {ardisik} dosyada bağlantı koptu"));
    }
    Ok(())
}

/// UYAP bu kadar dosyada art arda veri yerine boş / JSON olmayan / hata yanıtı verirse tarama durur. Oturum düşünce
/// UYAP her isteğe giriş sayfası döndürür; eskiden bu "alınamadı" sayılıp saatlerce sürüyor, sonunda "Tamamlandı"
/// deniyordu (21.11.2026: takip türü taramasında ~4900 dosya böyle boşa geçti).
pub const ARDISIK_RET_SINIRI: usize = 15;

/// Bir dosyada UYAP veri vermedi: art arda sınırı aşılırsa taramayı açık bir nedenle durdurur.
pub(crate) fn uyap_reddi(neden: &str, ardisik: &mut usize) -> Result<(), String> {
    *ardisik += 1;
    if *ardisik >= ARDISIK_RET_SINIRI {
        return Err(format!(
            "UYAP art arda {ardisik} dosyada veri vermedi (son yanıt: {neden}). UYAP oturumu düşmüş olabilir: UYAP'a yeniden \
             girip aynı düğmeyle devam edin."
        ));
    }
    Ok(())
}

/// Bitiş mesajına: takip türü okunamayan dosya varsa açıkça yazılır ("Tamamlandı" tek başına yanıltmasın).
fn okunamayan_eki(n: usize) -> String {
    if n == 0 { String::new() } else { format!(" ⚠️ {n} dosyanın takip türü UYAP'tan alınamadı (ayrıntı uyarılarda).") }
}

// ── UYAP dosya kimliklerinin yenilenmesi ────────────────────────────────────────────────────────

/// UYAP'ın listede verdiği dosya kimliği (`dosyaId`) yaklaşık bir saat geçerli; sonra her dosya isteğine
/// PRTL_GNL_10001-4 "Doğrulama hatası" döner (21.11.2026 gece taraması: saatte bir, ~63 dk'da). Kimlikler süre dolmadan
/// açık dosya listesi yeniden alınarak yenilenir; yine de bu hata gelirse hemen yenilenip istek bir kez tekrarlanır.
/// Eskiden tarama 15 ret sonra duruyor, dışarıdan yeniden başlatılması gerekiyordu.
pub const KIMLIK_OMRU_SN: f64 = 45.0 * 60.0;
/// Kimliğin bayatladığını bildiren UYAP hata kodu.
pub const BAYAT_KIMLIK_KODU: &str = "PRTL_GNL_10001-4";
/// İki yenileme arasında en az bu kadar saniye: yenileme de işe yaramadıysa (oturum düşmüş) her dosyada liste istenmez,
/// art arda ret sınırı taramayı durdurur.
const KIMLIK_YENILEME_ARASI_SN: f64 = 60.0;

/// Dosyanın UYAP listesindeki anahtarı: (daire kimliği, yıl, sıra, tür kodu). Yenilenen dosyada UYAP'ın verdiği
/// "2026/5683 - ( 2022/11833 )" numarası da yıl/sıra olarak okunur (`veri::yil_sira`).
pub type KimlikAnahtari = (String, i32, i32, i32);

fn kimlik_anahtari(r: &Value) -> KimlikAnahtari {
    let (yil, sira) = veri::yil_sira(&teb::metin(r, "dosyaNo"));
    (teb::metin(r, "birimId"), yil, sira, tur_kodu(r))
}

/// UYAP yanıtının nedeni "kimlik bayatladı" mı.
pub fn kimlik_bayat_mi(neden: &str) -> bool {
    neden.contains(BAYAT_KIMLIK_KODU)
}

/// Açık dosya listesini (taramanın daire / tarih kapsamıyla) sessizce yeniden alır, bilinen dosyaların yeni UYAP
/// kimliklerini `taze_kimlik`'e yazar. Sayaçlara ve rapora dokunmaz. Liste alınamazsa uyarı yazılır, tarama sürer
/// (oturum düştüyse sonraki istekler art arda ret sınırıyla durdurur).
async fn kimlikleri_tazele<U: Uyap>(t: &Tarama, u: &U) -> Result<(), String> {
    let (kapsam, ters) = {
        let mut g = t.inner.lock().unwrap();
        g.kimlik_zamani = Some(now_secs());
        let ters: HashMap<KimlikAnahtari, i64> = g.kimlik_anahtari.iter().map(|(d, a)| (a.clone(), *d)).collect();
        (g.kapsam.clone(), ters)
    };
    if ters.is_empty() {
        return Ok(());
    }
    let onceki = t.ind.inner.lock().unwrap().mesaj.clone();
    let mut yeni: HashMap<i64, String> = HashMap::new();
    for sayfa in 1..=SAYFA_EN_COK {
        t.ind.iptal_kontrol()?;
        t.mesaj_yaz(format!("UYAP dosya kimlikleri yenileniyor (liste sayfası {sayfa})…"));
        let veri = u.post(ARAMA, liste_govdesi(sayfa, t.sayfa_boyu, &kapsam)).await?;
        if let Some(neden) = sayfa_gecersiz(&veri) {
            t.uyari(format!("UYAP dosya kimlikleri yenilenemedi (liste sayfası {sayfa}: {neden})"));
            break;
        }
        let parca = crate::gerekceli_karar::kayitlar_listesi(&veri.unwrap_or(Value::Null));
        let n = parca.len();
        for r in parca {
            let did = teb::metin(&r, "dosyaId");
            if let Some(d) = ters.get(&kimlik_anahtari(&r)).filter(|_| !did.is_empty()) {
                yeni.insert(*d, did);
            }
        }
        if n < t.sayfa_boyu {
            break;
        }
    }
    t.mesaj_yaz(onceki);
    let mut g = t.inner.lock().unwrap();
    if !yeni.is_empty() {
        g.kimlik_tazeleme += 1;
    }
    g.taze_kimlik.extend(yeni);
    Ok(())
}

/// Dosyaya istek atmadan önce: kimliklerin süresi dolmak üzereyse yenilenir; dosyanın güncel kimliği döner.
pub(crate) async fn guncel_kimlik<U: Uyap>(t: &Tarama, u: &U, dosya: i64, eski: &str) -> Result<String, String> {
    let vakti = t.inner.lock().unwrap().kimlik_zamani.is_some_and(|z| now_secs() - z >= t.kimlik_omru_sn);
    if vakti {
        kimlikleri_tazele(t, u).await?;
    }
    Ok(t.inner.lock().unwrap().taze_kimlik.get(&dosya).cloned().unwrap_or_else(|| eski.to_string()))
}

/// UYAP "kimlik bayat" dedi: kimlikler yenilenir (son yenilemeden beri en az [`KIMLIK_YENILEME_ARASI_SN`] geçtiyse).
/// Dosya açık listede yoksa (seçimdeki kapalı dosya) numarasıyla aranır. Yeni ve farklı bir kimlik bulunursa döner;
/// bulunamazsa None (istek reddedilmiş sayılır).
pub(crate) async fn kimlik_yenile<U: Uyap>(t: &Tarama, u: &U, dosya: i64, eski: &str) -> Result<Option<String>, String> {
    let (zaman, anahtar) = {
        let g = t.inner.lock().unwrap();
        (g.kimlik_zamani, g.kimlik_anahtari.get(&dosya).cloned())
    };
    // Kimliği listeden gelmeyen iş (tek dosya): yenilenecek liste yok.
    let Some(z) = zaman else { return Ok(None) };
    if now_secs() - z >= KIMLIK_YENILEME_ARASI_SN {
        kimlikleri_tazele(t, u).await?;
    }
    let taze = t.inner.lock().unwrap().taze_kimlik.get(&dosya).cloned();
    match taze {
        Some(k) if k != eski => return Ok(Some(k)),
        Some(_) => return Ok(None),
        None => {}
    }
    let Some((birim_id, yil, sira_no, tur_kod)) = anahtar.filter(|a| !a.0.is_empty()) else { return Ok(None) };
    let a = ArananDosya { id: dosya, yil, sira_no, tur_kod, durum_kod: 1, birim_id, birim_ad: String::new() };
    let k = match dosya_ara(t, u, &a, 1).await? {
        Ok(Some(r)) => teb::metin(&r, "dosyaId"),
        _ => return Ok(None),
    };
    if k.is_empty() || k == eski {
        return Ok(None);
    }
    t.inner.lock().unwrap().taze_kimlik.insert(dosya, k.clone());
    Ok(Some(k))
}

/// Tek dosyalık istek (`{"dosyaId", …ek}`): UYAP "kimlik bayat" derse kimlik yenilenip AYNI istek bir kez yeni kimlikle
/// tekrarlanır; `uyap_id` güncel kimliğe çevrilir.
pub(crate) async fn dosya_istegi_at<U: Uyap>(
    t: &Tarama,
    u: &U,
    uc: &'static str,
    dosya: i64,
    uyap_id: &mut String,
    ek: Value,
) -> Result<Option<Value>, String> {
    let govde = |id: &str| {
        let mut g = if ek.is_object() { ek.clone() } else { json!({}) };
        g["dosyaId"] = json!(id);
        g
    };
    let v = u.post(uc, govde(uyap_id)).await?;
    if !sayfa_gecersiz(&v).is_some_and(|n| kimlik_bayat_mi(&n)) {
        return Ok(v);
    }
    match kimlik_yenile(t, u, dosya, uyap_id).await? {
        Some(yeni) => {
            *uyap_id = yeni;
            u.post(uc, govde(uyap_id)).await
        }
        None => Ok(v),
    }
}

/// 1. Liste: UYAP'taki açık icra dosyaları (dosyaId ile tekil).
async fn listele<U: Uyap>(t: &Tarama, u: &U) -> Result<Vec<Value>, String> {
    let kapsam = t.inner.lock().unwrap().kapsam.clone();
    t.asama("liste", 0);
    t.mesaj_yaz("UYAP'taki açık icra dosyaları listeleniyor…");
    let mut tum: Vec<Value> = Vec::new();
    let mut gorulen: HashSet<String> = HashSet::new();
    for sayfa in 1..=SAYFA_EN_COK {
        t.ind.iptal_kontrol()?;
        let veri = u.post(ARAMA, liste_govdesi(sayfa, t.sayfa_boyu, &kapsam)).await?;
        if let Some(neden) = sayfa_gecersiz(&veri) {
            t.inner.lock().unwrap().tam = false;
            t.uyari(format!("Liste sayfası {sayfa} alınamadı: {neden}. Liste eksik kaldı; kapanan dosya kontrolü yapılmayacak."));
            break;
        }
        let veri = veri.unwrap_or(Value::Null);
        if sayfa == 1 {
            t.inner.lock().unwrap().uyap_toplam = uyap_toplami(&veri);
        }
        let parca = crate::gerekceli_karar::kayitlar_listesi(&veri);
        let n = parca.len();
        for r in parca {
            let did = teb::metin(&r, "dosyaId");
            if !did.is_empty() && !gorulen.insert(did) {
                continue;
            }
            tum.push(r);
        }
        {
            let mut g = t.inner.lock().unwrap();
            g.alinan = tum.len();
            g.i = tum.len();
        }
        t.mesaj_yaz(format!("{} dosya alındı (sayfa {sayfa})…", tum.len()));
        if n < t.sayfa_boyu {
            break;
        }
        if sayfa == SAYFA_EN_COK {
            t.inner.lock().unwrap().tam = false;
            t.uyari(format!("Sayfa sınırına ({SAYFA_EN_COK}) ulaşıldı; bazı dosyalar alınmamış olabilir."));
        }
    }
    Ok(tum)
}

/// Künyesi yazılan dosya: (veritabanı kimliği, UYAP dosya kimliği, görünen etiket).
pub(crate) type YazilanDosya = (i64, String, String);

/// Künye adımının sonucu.
struct KunyeSonucu {
    /// Görülen (yazılan) dosya kimlikleri.
    gorulen: Vec<i64>,
    /// Taraf isteyecek dosyalar.
    isler: Vec<TarafIsi>,
    /// Yazılan dosyalar ve UYAP kimlikleri (tebligat / takip türü adımları için).
    dosyalar: Vec<YazilanDosya>,
    /// Bu turda veritabanına ilk kez eklenen dosyalar.
    yeniler: HashSet<i64>,
}

/// 2. Künye: her kayıt eklenir / güncellenir.
async fn kunyeleri_yaz(t: &Tarama, kayitlar: Vec<Value>) -> Result<KunyeSonucu, String> {
    t.asama("kunye", kayitlar.len());
    t.mesaj_yaz(format!("{} dosyanın künyesi veritabanına yazılıyor…", kayitlar.len()));
    // UYAP kimliği → listedeki anahtar (kimlik yenilemesi için; yazılan dosyalara aşağıda bağlanır).
    let anahtarlar: HashMap<String, KimlikAnahtari> =
        kayitlar.iter().map(|r| (teb::metin(r, "dosyaId"), kimlik_anahtari(r))).filter(|(k, _)| !k.is_empty()).collect();
    let mut gorulen: Vec<i64> = Vec::new();
    let mut eslesme: Vec<(i64, String, String)> = Vec::new();
    let mut yeniler: HashSet<i64> = HashSet::new();
    for parca in kayitlar.chunks(250) {
        t.ind.iptal_kontrol()?;
        let parca = parca.to_vec();
        let (yazilan, uyarilar, yeni) = vt(move |c| {
            let mut yazilan: Vec<(i64, String, String)> = Vec::new();
            let mut uyarilar = Vec::new();
            let mut yeni: Vec<i64> = Vec::new();
            for rec in &parca {
                let etiket = format!("{} {}", teb::metin(rec, "birimAdi"), teb::metin(rec, "dosyaNo"));
                match veri::dosya_kunyesi_kaydet(c, rec, Some(YargiTuru::Icra.kod())) {
                    Ok((id, y)) => {
                        if y {
                            yeni.push(id);
                        }
                        yazilan.push((id, teb::metin(rec, "dosyaId"), etiket));
                    }
                    Err(e) => uyarilar.push(format!("{etiket}: künye yazılamadı ({e})")),
                }
            }
            Ok((yazilan, uyarilar, yeni))
        })
        .await?;
        {
            let mut g = t.inner.lock().unwrap();
            g.i += parca_len(&yazilan, &uyarilar);
            g.yazilan += yazilan.len();
            g.yeni_dosya += yeni.len();
            g.uyarilar.extend(uyarilar);
        }
        yeniler.extend(yeni);
        gorulen.extend(yazilan.iter().map(|x| x.0));
        eslesme.extend(yazilan);
    }
    {
        let mut g = t.inner.lock().unwrap();
        for (id, uyap_id, _) in &eslesme {
            if let Some(a) = anahtarlar.get(uyap_id) {
                g.kimlik_anahtari.insert(*id, a.clone());
            }
        }
    }
    // Taraf ihtiyacı: alacaklısı ya da borçlusu olmayan (her icra dosyasında ikisi de vardır; yoksa kayıt eksik) →
    // tam; borçlusu kimliksiz → yalnız borçlu listesi.
    let idler = gorulen.clone();
    // (tam taraf yok, borçlu kimliksiz, taraf bilgileri hiç okunmamış)
    let eksik: HashMap<i64, (bool, bool, bool)> = vt(move |c| {
        Ok(c.query(
            "SELECT d.id,
                    NOT EXISTS (SELECT 1 FROM dosya_taraf x WHERE x.dosya_id = d.id AND x.rol = 'alacakli')
                    OR NOT EXISTS (SELECT 1 FROM dosya_taraf x WHERE x.dosya_id = d.id AND x.rol = 'borclu'),
                    EXISTS (SELECT 1 FROM dosya_taraf x JOIN taraf tr ON tr.id = x.taraf_id
                             WHERE x.dosya_id = d.id AND x.rol = 'borclu' AND x.uyapta_gorulmedi IS NULL
                               AND tr.tckn IS NULL AND tr.mersis_no IS NULL AND tr.vergi_no = ''),
                    d.taraf_bilgisi_zamani IS NULL
               FROM dosya d WHERE d.id = ANY($1)",
            &[&idler],
        )
        .map_err(|e| format!("veritabanı: {e}"))?
        .iter()
        .map(|s| (s.get::<_, i64>(0), (s.get::<_, bool>(1), s.get::<_, bool>(2), s.get::<_, bool>(3))))
        .collect())
    })
    .await?;
    let mod_ = t.inner.lock().unwrap().mod_;
    if mod_ == Mod::Liste {
        t.inner.lock().unwrap().taraf_eksik = eksik.values().filter(|(tum, kimliksiz, _)| *tum || *kimliksiz).count();
        return Ok(KunyeSonucu { gorulen, isler: Vec::new(), dosyalar: eslesme, yeniler });
    }
    // Tebligat güncellemesinde yalnız tarafı HİÇ olmayan dosyalar: borçlu kimliği tebligat eşleşmesi için
    // gerekmez (açık dosyaların neredeyse hepsinde borçlu kimliksiz; hepsine istek binlerce istek demek).
    let isler = eslesme
        .iter()
        .filter(|(_, uyap_id, _)| !uyap_id.is_empty())
        .filter_map(|(dosya, uyap_id, etiket)| {
            let (dosya, uyap_id, etiket) = (*dosya, uyap_id.clone(), etiket.clone());
            match (eksik.get(&dosya), mod_) {
                (Some((true, _, _)), _) => Some(TarafIsi { dosya, uyap_id, etiket, borclu_listesi: true, taraf_bilgisi: true }),
                (Some((false, kimliksiz, okunmamis)), Mod::Taraf | Mod::Hepsi) if *kimliksiz || *okunmamis => {
                    Some(TarafIsi { dosya, uyap_id, etiket, borclu_listesi: *kimliksiz, taraf_bilgisi: *okunmamis })
                }
                _ => None,
            }
        })
        .collect();
    Ok(KunyeSonucu { gorulen, isler, dosyalar: eslesme, yeniler })
}

fn parca_len(a: &[YazilanDosya], b: &[String]) -> usize {
    a.len() + b.len()
}

/// Dosyada UYAP'ta görülen (işaretsiz) borçlu var mı.
async fn borclu_var(dosya: i64) -> Result<bool, String> {
    vt(move |c| {
        Ok(c.query_one(
            "SELECT EXISTS (SELECT 1 FROM dosya_taraf WHERE dosya_id = $1 AND rol = 'borclu' AND uyapta_gorulmedi IS NULL)",
            &[&dosya],
        )
        .map_err(|e| format!("veritabanı: {e}"))?
        .get(0))
    })
    .await
}

/// 3. Taraflar: borçlular (TCKN'li) + borçlu dışındaki roller. Borçlu listesi ucu borçlu vermezse (boş liste
/// ya da red) borçlular taraf bilgileri ucundan TCKN'siz yazılır; yoksa dosya borçlusuz kalıyordu (19.11 bulgusu:
/// 46 açık dosya). İş sonunda hâlâ borçlusu olmayan dosyalar `borclusuz`'a yazılır; `taraf_cekilen` yalnız
/// borçlusu olan dosyaları sayar.
async fn taraflari_cek<U: Uyap>(t: &Tarama, u: &U, isler: Vec<TarafIsi>) -> Result<(), String> {
    t.asama("taraf", isler.len());
    if isler.is_empty() {
        return Ok(());
    }
    let borclu_disi: Vec<&'static str> = Rol::HEPSI.iter().map(|r| r.kod()).filter(|k| *k != Rol::Borclu.kod()).collect();
    let hepsi: Vec<&'static str> = Rol::HEPSI.iter().map(|r| r.kod()).collect();
    let (mut ardisik, mut ret) = (0usize, 0usize);
    for (sira, mut is_) in isler.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.ind.iptal_kontrol()?;
        is_.uyap_id = guncel_kimlik(t, u, is_.dosya, &is_.uyap_id).await?;
        t.mesaj_yaz(format!("Taraflar alınıyor: {} ({}/{})", is_.etiket, sira + 1, t.inner.lock().unwrap().toplam));
        // Bu dosyada UYAP'tan gerçek veri geldi mi / veri yerine geçersiz yanıt mı geldi (art arda ret sayımı).
        let (mut veri_geldi, mut reddedildi): (bool, Option<String>) = (false, None);
        // Borçlular: TCKN / vergi no veren uç.
        let mut borclu_notu: Option<String> = None;
        let borclular = if is_.borclu_listesi {
            dosya_istegi_at(t, u, BORCLU_LISTESI, is_.dosya, &mut is_.uyap_id, json!({})).await
        } else {
            Ok(Some(Value::Array(Vec::new())))
        };
        match borclular {
            _ if !is_.borclu_listesi => {}
            Ok(Some(Value::Array(liste))) if liste.iter().any(Value::is_object) => {
                ardisik = 0;
                veri_geldi = true;
                let (dosya, etiket) = (is_.dosya, is_.etiket.clone());
                let notlar = vt(move |c| {
                    let notlar = StdMutex::new(Vec::new());
                    kayit::borclu_kimliklerini_kaydet(c, dosya, &liste, &|m: &str| notlar.lock().unwrap().push(format!("{etiket}: {}", m.trim())))?;
                    Ok(notlar.into_inner().unwrap())
                })
                .await?;
                t.inner.lock().unwrap().uyarilar.extend(notlar);
            }
            Ok(Some(Value::Array(_))) => {
                ardisik = 0;
                veri_geldi = true;
                borclu_notu = Some("borçlu listesi boş döndü".into());
            }
            Ok(v) => {
                ardisik = 0;
                let neden = sayfa_gecersiz(&v).unwrap_or_else(|| "beklenmeyen yanıt".into());
                borclu_notu = Some(format!("borçlu listesi alınamadı: {neden}"));
                reddedildi = Some(neden);
            }
            Err(e) => {
                ag_hatasi_mi(&e, t, &is_.etiket, &mut ardisik)?;
                borclu_notu = Some("borçlu listesi alınamadı: bağlantı koptu".into());
            }
        }
        if is_.taraf_bilgisi {
            // Alacaklı ve diğer roller. Borçlu yukarıda (ya da önceden) yazıldıysa bu uçtaki borçlu satırlarından yalnız
            // VEKİLLER kayıtlı borçluya yazılır (bu uç TCKN vermez, borçlu çoğalmasın); borçlu yoksa buradan alınır.
            let borclu_de = !borclu_var(is_.dosya).await?;
            match dosya_istegi_at(t, u, TARAF_BILGILERI, is_.dosya, &mut is_.uyap_id, json!({})).await {
                Ok(Some(Value::Array(liste))) => {
                    ardisik = 0;
                    veri_geldi = true;
                    if !borclu_de {
                        let (dosya, ham) = (is_.dosya, liste.clone());
                        vt(move |c| kayit::borclu_vekillerini_yaz(c, dosya, &ham)).await?;
                    }
                    let dosya = is_.dosya;
                    vt(move |c| kayit::taraf_bilgisi_okundu(c, dosya)).await?;
                    let secilen: Vec<Value> = liste
                        .into_iter()
                        .filter(|x| x.is_object() && (borclu_de || teb::tr_kucuk(&teb::metin(x, "rol")) != Rol::Borclu.kod()))
                        .collect();
                    if !secilen.is_empty() {
                        let (dosya, etiket) = (is_.dosya, is_.etiket.clone());
                        let roller = if borclu_de { hepsi.clone() } else { borclu_disi.clone() };
                        let notlar = vt(move |c| {
                            let notlar = StdMutex::new(Vec::new());
                            kayit::dosya_taraf_kaydet_roller(c, dosya, &secilen, Some(&roller), &|m: &str| {
                                notlar.lock().unwrap().push(format!("{etiket}: {}", m.trim()))
                            })?;
                            Ok(notlar.into_inner().unwrap())
                        })
                        .await?;
                        t.inner.lock().unwrap().uyarilar.extend(notlar);
                    } else {
                        t.uyari(format!("{}: UYAP taraf listesi boş", is_.etiket));
                    }
                }
                Ok(v) => {
                    ardisik = 0;
                    let neden = sayfa_gecersiz(&v).unwrap_or_else(|| "beklenmeyen yanıt".into());
                    t.uyari(format!("{}: taraflar alınamadı ({neden})", is_.etiket));
                    reddedildi = Some(neden);
                }
                Err(e) => ag_hatasi_mi(&e, t, &is_.etiket, &mut ardisik)?,
            }
        }
        if veri_geldi {
            ret = 0;
        } else if let Some(neden) = &reddedildi {
            uyap_reddi(neden, &mut ret)?;
        }
        if borclu_var(is_.dosya).await? {
            t.inner.lock().unwrap().taraf_cekilen += 1;
        } else {
            let neden = borclu_notu.unwrap_or_else(|| "UYAP borçlu bildirmedi".into());
            let mut g = t.inner.lock().unwrap();
            g.borclusuz.push(is_.etiket.clone());
            g.uyarilar.push(format!("⚠️ {}: UYAP'tan borçlu alınamadı ({neden})", is_.etiket));
        }
        t.ilerle();
    }
    Ok(())
}

/// Takip türü hiç okunmamış dosyalar (verilenler arasından; `icra_takip_detay.ayrinti_zamani` boş). Talimat dosyaları
/// sorulmaz: kesinleşmeleri olmadığı için takip türüne gerek yok.
async fn takip_okunmamislar(dosyalar: Vec<YazilanDosya>) -> Result<Vec<YazilanDosya>, String> {
    if dosyalar.is_empty() {
        return Ok(dosyalar);
    }
    let idler: Vec<i64> = dosyalar.iter().map(|x| x.0).collect();
    let okunmus: HashSet<i64> = vt(move |c| {
        Ok(c.query(
            "SELECT dosya_id FROM icra_takip_detay WHERE dosya_id = ANY($1) AND ayrinti_zamani IS NOT NULL
             UNION SELECT id FROM dosya WHERE id = ANY($1) AND tur_kod = 1",
            &[&idler],
        )
            .map_err(|e| format!("veritabanı: {e}"))?
            .iter()
            .map(|s| s.get(0))
            .collect())
    })
    .await?;
    Ok(dosyalar.into_iter().filter(|x| !okunmus.contains(&x.0)).collect())
}

/// Kapsamdaki görülen dosyalardan takip türü hiç okunmamış olanların sayısı.
async fn takip_eksik_say(gorulen: Vec<i64>) -> Result<usize, String> {
    vt(move |c| {
        let n: i64 = c
            .query_one(
                "SELECT count(*) FROM unnest($1::bigint[]) AS g(id)
                  WHERE NOT EXISTS (SELECT 1 FROM icra_takip_detay i WHERE i.dosya_id = g.id AND i.ayrinti_zamani IS NOT NULL)
                    AND NOT EXISTS (SELECT 1 FROM dosya d WHERE d.id = g.id AND d.tur_kod = 1)",
                &[&gorulen],
            )
            .map_err(|e| format!("veritabanı: {e}"))?
            .get(0);
        Ok(n as usize)
    })
    .await
}

/// Takip türü: her dosya için `dosyaAyrintiBilgileri_brd` (1 istek) → `icra_takip_detay` (kodlar, adlar, Örnek no,
/// alacak toplamları). Takip türü değişmez; toplu taramalar yalnız okunmamış dosyaları verir.
async fn takip_ayrintilari<U: Uyap>(t: &Tarama, u: &U, dosyalar: Vec<YazilanDosya>) -> Result<(), String> {
    let dosyalar: Vec<YazilanDosya> = dosyalar.into_iter().filter(|x| !x.1.is_empty()).collect();
    if dosyalar.is_empty() {
        return Ok(());
    }
    t.asama("takip", dosyalar.len());
    let (mut ardisik, mut ret) = (0usize, 0usize);
    for (sira, (dosya, uyap_id, etiket)) in dosyalar.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.ind.iptal_kontrol()?;
        let mut uyap_id = guncel_kimlik(t, u, dosya, &uyap_id).await?;
        t.mesaj_yaz(format!("Takip türü alınıyor: {etiket} ({}/{})", sira + 1, t.inner.lock().unwrap().toplam));
        match dosya_istegi_at(t, u, DOSYA_AYRINTI, dosya, &mut uyap_id, json!({})).await {
            Ok(Some(v)) if v.as_object().is_some_and(|o| !o.is_empty()) && !kayit::hata_zarfi_mi(&v) => {
                (ardisik, ret) = (0, 0);
                let yazildi = vt(move |c| kayit::dosya_ayrinti_kaydet(c, dosya, &v)).await?;
                let mut g = t.inner.lock().unwrap();
                if yazildi.is_some() {
                    g.takip_okunan += 1;
                } else {
                    g.takip_okunamayan += 1;
                    g.uyarilar.push(format!("{etiket}: takip türü yazılamadı (icra dosyası değil)"));
                }
            }
            Ok(v) => {
                ardisik = 0;
                let neden = sayfa_gecersiz(&v).unwrap_or_else(|| "boş yanıt".into());
                {
                    let mut g = t.inner.lock().unwrap();
                    g.takip_okunamayan += 1;
                    g.uyarilar.push(format!("{etiket}: takip türü alınamadı ({neden})"));
                }
                uyap_reddi(&neden, &mut ret)?;
            }
            Err(e) => {
                ag_hatasi_mi(&e, t, &etiket, &mut ardisik)?;
                t.inner.lock().unwrap().takip_okunamayan += 1;
            }
        }
        t.ilerle();
    }
    Ok(())
}

/// Takip türü listesinin istek gövdesindeki kod: UYAP sayı bekler (açılış ekranı böyle gönderir).
fn kod_degeri(kod: &str) -> Value {
    kod.parse::<i64>().map(Value::from).unwrap_or_else(|_| json!(kod))
}

/// UYAP takip açılış ekranının tür → yol → şekil listesi → `takip_turu_listesi`. Bir türün / yolun alt listesi
/// alınamazsa uyarı yazılır, kalanlar sürer.
async fn takip_listesi_al<U: Uyap>(t: &Tarama, u: &U) -> Result<(), String> {
    t.asama("takip_listesi", 0);
    t.mesaj_yaz("UYAP takip türü listesi alınıyor…");
    let turler_ham = u.post(TAKIP_TURU_LISTESI, json!({})).await?;
    let turler = turler_ham.as_ref().map(kayit::takip_secenekleri).unwrap_or_default();
    if turler.is_empty() {
        return Err(format!("UYAP takip türü listesini vermedi ({}).", sayfa_gecersiz(&turler_ham).unwrap_or_else(|| "boş liste".into())));
    }
    let mut ogeler: Vec<kayit::TakipSecenegi> = Vec::new();
    let mut sayi = (turler.len(), 0usize, 0usize);
    for (tur, tur_adi) in &turler {
        ogeler.push(kayit::TakipSecenegi { kademe: 1, turu: tur.clone(), yolu: String::new(), sekli: String::new(), ad: tur_adi.clone() });
        t.ind.iptal_kontrol()?;
        t.mesaj_yaz(format!("Takip yolları alınıyor: {tur_adi}"));
        let yollar_ham = u.post(TAKIP_YOLU_LISTESI, json!({"takipTuru": kod_degeri(tur)})).await?;
        let yollar = yollar_ham.as_ref().map(kayit::takip_secenekleri).unwrap_or_default();
        if yollar.is_empty() {
            t.uyari(format!("{tur_adi}: takip yolu listesi alınamadı"));
            continue;
        }
        sayi.1 += yollar.len();
        for (yol, yol_adi) in &yollar {
            ogeler.push(kayit::TakipSecenegi { kademe: 2, turu: tur.clone(), yolu: yol.clone(), sekli: String::new(), ad: yol_adi.clone() });
            t.ind.iptal_kontrol()?;
            t.mesaj_yaz(format!("Takip şekilleri alınıyor: {tur_adi} · {yol_adi}"));
            let sekiller_ham = u.post(TAKIP_SEKLI_LISTESI, json!({"takipTuru": kod_degeri(tur), "takipYolu": kod_degeri(yol)})).await?;
            let sekiller = sekiller_ham.as_ref().map(kayit::takip_secenekleri).unwrap_or_default();
            if sekiller.is_empty() {
                t.uyari(format!("{tur_adi} · {yol_adi}: takip şekli listesi alınamadı"));
                continue;
            }
            sayi.2 += sekiller.len();
            ogeler.extend(sekiller.into_iter().map(|(sekli, ad)| kayit::TakipSecenegi { kademe: 3, turu: tur.clone(), yolu: yol.clone(), sekli, ad }));
        }
    }
    vt(move |c| kayit::takip_listesi_kaydet(c, &ogeler)).await?;
    t.inner.lock().unwrap().takip_listesi = sayi;
    Ok(())
}

/// 4. Kapanan dosyalar: açık listede olmayan ama veritabanında açık duranlar Kapalı listede tek tek aranır.
async fn kapananlari_uzlastir<U: Uyap>(t: &Tarama, u: &U, gorulen: Vec<i64>) -> Result<(), String> {
    let birim = t.inner.lock().unwrap().kapsam.birim_id.clone();
    let gorulen_sayisi = gorulen.len();
    let adaylar: Vec<(i64, i32, i32, i32, String, String)> = vt(move |c| {
        Ok(c.query(
            "SELECT d.id, d.yil, d.sira_no, d.tur_kod, b.birim_id, b.ad FROM dosya d JOIN birim b ON b.id = d.birim_id
              WHERE d.yargi_turu = 2 AND d.durum_kod = 0 AND NOT (d.id = ANY($1))
                AND ($2::text IS NULL OR b.birim_id = $2) ORDER BY d.yil DESC, d.sira_no DESC",
            &[&gorulen, &birim],
        )
        .map_err(|e| format!("veritabanı: {e}"))?
        .iter()
        .map(|s| (s.get(0), s.get(1), s.get(2), s.get(3), s.get(4), s.get(5)))
        .collect())
    })
    .await?;
    t.inner.lock().unwrap().aday = adaylar.len();
    if adaylar.is_empty() {
        return Ok(());
    }
    if fren_asildi(adaylar.len(), gorulen_sayisi) {
        t.uyari(format!(
            "{} açık dosya UYAP listesinde görünmedi — sayı beklenmeyecek kadar yüksek, kapanan dosya kontrolü bu tur ATLANDI.",
            adaylar.len()
        ));
        return Ok(());
    }
    t.asama("kapanan", adaylar.len());
    let mut ardisik = 0usize;
    for (sira, (id, yil, sira_no, tur_kod, birim_id, birim_ad)) in adaylar.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.ind.iptal_kontrol()?;
        let etiket = format!("{birim_ad} {yil}/{sira_no}");
        t.mesaj_yaz(format!("Kapanan dosya kontrolü: {etiket} ({}/{})", sira + 1, t.inner.lock().unwrap().toplam));
        let govde = json!({"dosyaDurumKod": 1, "pageSize": t.sayfa_boyu, "pageNumber": 1, "dosyaYil": yil, "dosyaSira": sira_no,
                           "birimId": birim_id, "birimTuru2": BIRIM_TURU2, "birimTuru3": BIRIM_TURU3});
        let v = match u.post(ARAMA, govde).await {
            Ok(v) => {
                ardisik = 0;
                v
            }
            Err(e) => {
                ag_hatasi_mi(&e, t, &etiket, &mut ardisik)?;
                t.ilerle();
                continue;
            }
        };
        if let Some(neden) = sayfa_gecersiz(&v) {
            t.uyari(format!("{etiket}: kapalı listede aranamadı ({neden}); dokunulmadı"));
            t.ilerle();
            continue;
        }
        // Numara yıl/sıra olarak karşılaştırılır: yenilenen dosyada UYAP "2026/5683 - ( 2022/11833 )" verir.
        let eslesen = crate::gerekceli_karar::kayitlar_listesi(&v.unwrap_or(Value::Null))
            .into_iter()
            .find(|r| (yil, sira_no) != (0, 0) && veri::yil_sira(&teb::metin(r, "dosyaNo")) == (yil, sira_no) && tur_kodu(r) == tur_kod);
        match eslesen {
            Some(rec) => {
                let durum = teb::metin(&rec, "dosyaDurum");
                vt(move |c| veri::dosya_kunyesi_kaydet(c, &rec, Some(YargiTuru::Icra.kod())).map(|_| ())).await?;
                t.inner.lock().unwrap().kapandi += 1;
                let _ = id;
                t.uyari(format!("✔ {etiket} kapalı olarak güncellendi ({}).", if durum.is_empty() { "Kapalı".into() } else { durum }));
            }
            None => t.inner.lock().unwrap().bulunamadi += 1,
        }
        t.ilerle();
    }
    Ok(())
}

fn tur_kodu(r: &Value) -> i32 {
    match r.get("dosyaTurKod") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) as i32,
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

// ── Tebligat (T) ve yalnız PTT ──────────────────────────────────────────────────────────────────

/// İptal edilebilir bekleme (saniye adımlarıyla).
async fn bekle(t: &Tarama, sn: u64) -> Result<(), String> {
    for _ in 0..sn {
        t.ind.iptal_kontrol()?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}

/// Dosyanın bütün evrakı (Tüm Evrakı İndir ile aynı uçlar). Dış hata: ağ / iptal (tarama kararı verir);
/// iç hata: UYAP reddi (dosya atlanır). Yarım liste kullanılmaz: mazbatası başka sayfada kalan tebligat
/// gereksiz yere PTT'ye sorulurdu.
pub(crate) async fn evrak_listesi<U: Uyap>(u: &U, uyap_id: &str) -> Result<Result<Vec<Value>, String>, String> {
    let toplam = u.post(evrak_toplu::EVRAK_LISTESI_TOPLAM, json!({"dosyaId": uyap_id, "pageNumber": 1})).await?;
    if let Some(v) = toplam.as_ref().filter(|v| v.is_object()) {
        if let Some(neden) = sayfa_gecersiz(&Some(v.clone())) {
            return Ok(Err(neden));
        }
    }
    let sayfa_sayisi = toplam
        .as_ref()
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(1)
        .clamp(1, 200);
    let mut tum = Vec::new();
    for sayfa in 1..=sayfa_sayisi {
        let v = u.post(evrak_toplu::EVRAK_LISTESI, json!({"dosyaId": uyap_id, "pageNumber": sayfa})).await?;
        if let Some(neden) = sayfa_gecersiz(&v) {
            return Ok(Err(neden));
        }
        let v = v.unwrap_or(Value::Null);
        if v.get("tumEvraklar").is_some() {
            tum.extend(teb::evraklari_duzle(&v));
        } else if let Some(a) = v.as_array() {
            tum.extend(a.iter().filter(|e| e.is_object()).cloned());
        }
    }
    Ok(Ok(evrak_toplu::_duzlestir(tum)))
}

/// Tebligat evrakının içeriği (PDF). Dış hata: ağ / iptal; iç hata: UYAP vermedi.
pub(crate) async fn evrak_pdf<U: Uyap>(u: &U, uyap_id: &str, evrak_id: &str) -> Result<Result<Vec<u8>, String>, String> {
    if evrak_id.is_empty() {
        return Ok(Err("evrak kimliği yok".into()));
    }
    u.post(evrak_toplu::DOC_VIEWER_HAZIRLIK, json!({})).await?;
    let params = HashMap::from([("evrakId".to_string(), evrak_id.to_string()), ("dosyaId".to_string(), uyap_id.to_string())]);
    let (st, bayt) = u.indir(evrak_toplu::EVRAK_ICERIK, params).await?;
    if st >= 400 {
        return Ok(Err(format!("HTTP {st}")));
    }
    if bayt.is_empty() {
        return Ok(Err("boş yanıt".into()));
    }
    Ok(Ok(bayt))
}

/// PTT sorgusu; başarısızsa bir kez yeniden denenir. Art arda [`PTT_HATA_SINIRI`] barkodda yanıt alınamazsa
/// PTT bu tur bırakılır. None: PTT kapalı ya da sorgu başarısız (kayıtlı PTT durumu korunur).
async fn ptt_sor<U: Uyap>(t: &Tarama, u: &U, barkod: &str, ardisik: &mut usize) -> Result<Option<Value>, String> {
    if !t.ptt_acik() {
        return Ok(None);
    }
    t.inner.lock().unwrap().ptt_sorulan += 1;
    let mut son = String::new();
    for deneme in 0..2 {
        if deneme > 0 {
            bekle(t, t.ptt_tekrar_sn).await?;
        }
        t.ind.iptal_kontrol()?;
        match u.ptt(barkod.to_string()).await {
            Ok(v) => {
                *ardisik = 0;
                return Ok(Some(v));
            }
            Err(e) => son = e,
        }
    }
    *ardisik += 1;
    let mut g = t.inner.lock().unwrap();
    g.ptt_hatasi += 1;
    g.uyarilar.push(format!("PTT {barkod}: sorgulanamadı ({son}); kayıtlı durum korundu"));
    if *ardisik >= PTT_HATA_SINIRI {
        g.ptt_kapandi = true;
        g.uyarilar.push(format!("⚠️ PTT art arda {ardisik} barkodda yanıt vermedi; bu tur PTT sorgusu bırakıldı."));
    }
    Ok(None)
}

/// Alınamayan tebligat zarfının nedenini evrak kaydına yazar (dosya panelinde gösterilir; zarf alınınca silinir).
async fn tebligat_notu(dosya: i64, no: &str, not: String) -> Result<(), String> {
    let Ok(n) = no.trim().parse::<i32>() else { return Ok(()) };
    vt(move |c| {
        c.execute(
            "UPDATE evrak SET tebligat_notu = $3 WHERE dosya_id = $1 AND birim_evrak_no = $2",
            &[&dosya, &n, &veri::kirp(&not, 300)],
        )
        .map(|_| ())
        .map_err(|e| format!("veritabanı: {e}"))
    })
    .await
}

/// Taslağın PTT alanları veritabanına yazılacak mı (`_hata` taşıyan işaretler yazılmaz).
fn ptt_yazilir(k: &kayit::TebligatKaydi) -> bool {
    k.ham_yanit.as_ref().is_none_or(|h| !teb::dolu(h.get("_hata")))
}

/// Tek dosyanın tebligat evrakları: `icra_tarama` adımları sırasıyla (başlat → PDF → PTT → kaydet).
async fn dosya_tebligatlari<U: Uyap>(
    t: &Tarama,
    u: &U,
    (dosya, uyap_id, etiket): &YazilanDosya,
    evraklar: Vec<Value>,
    secim: &[i32],
    ardisik: &mut usize,
    ptt_ardisik: &mut usize,
) -> Result<(), String> {
    let dosya = *dosya;
    // Bağlam (mazbatalar, talep evrakı) bütün listeden kurulur; seçim yalnız işlenecek tebligatları daraltır.
    let hedef: Vec<Value> = tarama::hedef_evraklar(&evraklar)
        .into_iter()
        .filter(|e| secim.is_empty() || kayit::birim_evrak_no(e).is_some_and(|n| secim.contains(&n)))
        .cloned()
        .collect();
    let mut ctx = vt(move |c| {
        kayit::evrak_listesi_kaydet(c, dosya, &evraklar)?;
        tarama::dosya_baglami(c, dosya, &evraklar)
    })
    .await?;
    for evrak in hedef {
        t.ind.iptal_kontrol()?;
        let ptt_acik = t.ptt_acik();
        let no = teb::metin(&evrak, "birimEvrakNo");
        let e = evrak.clone();
        let (c2, mut adim) = vt(move |c| {
            let a = tarama::evrak_baslat(c, &ctx, &e, ptt_acik)?;
            Ok((ctx, a))
        })
        .await?;
        ctx = c2;
        if matches!(adim, Adim::PdfGerek) {
            let pdf = match evrak_pdf(u, uyap_id, &teb::metin(&evrak, "evrakId")).await {
                Ok(Ok(p)) => {
                    *ardisik = 0;
                    p
                }
                Ok(Err(neden)) => {
                    *ardisik = 0;
                    t.uyari(format!("{etiket} evrak {no}: tebligat belgesi alınamadı ({neden})"));
                    t.inner.lock().unwrap().teb_atlanan += 1;
                    tebligat_notu(dosya, &no, format!("Tebligat belgesi UYAP'tan alınamadı ({neden})")).await?;
                    continue;
                }
                Err(e) => {
                    ag_hatasi_mi(&e, t, etiket, ardisik)?;
                    t.inner.lock().unwrap().teb_atlanan += 1;
                    tebligat_notu(dosya, &no, "Tebligat belgesi alınırken bağlantı koptu".into()).await?;
                    continue;
                }
            };
            t.inner.lock().unwrap().pdf_indirilen += 1;
            let e = evrak.clone();
            let (c2, a) = vt(move |c| {
                let a = tarama::pdf_geldi(c, &mut ctx, &e, &pdf, ptt_acik)?;
                Ok((ctx, a))
            })
            .await?;
            ctx = c2;
            adim = a;
        }
        let taslak = match adim {
            Adim::Atla(neden) => {
                // MTS'te aynı barkodlu ikinci evrak olağandır; uyarı değildir.
                if !neden.starts_with("MTS") {
                    t.uyari(format!("{etiket} evrak {no}: {neden}"));
                    t.inner.lock().unwrap().teb_atlanan += 1;
                    tebligat_notu(dosya, &no, neden).await?;
                }
                continue;
            }
            Adim::PdfGerek => continue,
            Adim::Kaydet(tk) => tk,
            Adim::PttGerek(mut tk) => match ptt_sor(t, u, &tk.kayit.barkod, ptt_ardisik).await? {
                Some(v) => tarama::ptt_geldi(&ctx, tk, v),
                None => {
                    tk.kayit.ham_yanit = Some(teb::ptt_atlandi());
                    tk
                }
            },
        };
        let (c2, (once, taslak)) = vt(move |c| {
            let once = kayit::bilinen_tebligat(c, dosya, taslak.birim_evrak_no)?;
            tarama::kaydet(c, &ctx, &taslak)?;
            // Zarf alındı: önceki "alınamadı" notu silinir.
            c.execute("UPDATE evrak SET tebligat_notu = '' WHERE dosya_id = $1 AND birim_evrak_no = $2 AND tebligat_notu <> ''",
                      &[&dosya, &taslak.birim_evrak_no])
                .map_err(|e| format!("veritabanı: {e}"))?;
            Ok((ctx, (once, taslak)))
        })
        .await?;
        ctx = c2;
        let mut g = t.inner.lock().unwrap();
        g.teb_yazilan += 1;
        match once {
            None => g.teb_yeni += 1,
            Some(o) if ptt_yazilir(&taslak.kayit) && o.ptt_durumu != taslak.kayit.ptt_durumu => g.teb_degisen += 1,
            Some(_) => {}
        }
    }
    Ok(())
}

/// T. Tebligatlar: künyesi yazılan her dosyanın evrak listesi ve tebligat evrakları.
async fn tebligatlari_cek<U: Uyap>(t: &Tarama, u: &U, dosyalar: Vec<YazilanDosya>) -> Result<(), String> {
    let dosyalar: Vec<YazilanDosya> = dosyalar.into_iter().filter(|(_, uyap_id, _)| !uyap_id.is_empty()).collect();
    t.asama("tebligat", dosyalar.len());
    let (mut ardisik, mut ptt_ardisik, mut ret) = (0usize, 0usize, 0usize);
    let toplam = dosyalar.len();
    for (sira, mut d) in dosyalar.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.ind.iptal_kontrol()?;
        d.1 = guncel_kimlik(t, u, d.0, &d.1).await?;
        t.mesaj_yaz(format!("Tebligatlar: {} ({}/{})", d.2, sira + 1, toplam));
        let mut liste = evrak_listesi(u, &d.1).await;
        if let Ok(Err(neden)) = &liste {
            if kimlik_bayat_mi(neden) {
                if let Some(yeni) = kimlik_yenile(t, u, d.0, &d.1).await? {
                    d.1 = yeni;
                    liste = evrak_listesi(u, &d.1).await;
                }
            }
        }
        match liste {
            Ok(Ok(evraklar)) => {
                (ardisik, ret) = (0, 0);
                t.inner.lock().unwrap().teb_dosya += 1;
                dosya_tebligatlari(t, u, &d, evraklar, &[], &mut ardisik, &mut ptt_ardisik).await?;
            }
            Ok(Err(neden)) => {
                ardisik = 0;
                t.uyari(format!("{}: evrak listesi alınamadı ({neden})", d.2));
                uyap_reddi(&neden, &mut ret)?;
            }
            Err(e) => ag_hatasi_mi(&e, t, &d.2, &mut ardisik)?,
        }
        t.ilerle();
    }
    Ok(())
}

/// Yalnız PTT: UYAP'a gidilmez. Kapsamdaki açık dosyaların sonucu kesinleşmemiş fiziki tebligatları sorulur
/// (e-tebligat, mazbatası dönmüş ya da açıklamasında tebliğ sonucu olanlar sorulmaz).
async fn yalniz_ptt<U: Uyap>(t: &Tarama, u: &U) -> Result<(), String> {
    let kapsam = t.inner.lock().unwrap().kapsam.clone();
    let (birim, bas, son, secili) = (kapsam.birim_id.clone(), kapsam.acilis_bas, kapsam.acilis_son, kapsam.dosyalar.clone());
    t.mesaj_yaz("Sonuçlanmamış tebligatlar veritabanından alınıyor…");
    let satirlar: Vec<(i64, i64, String, String, String, String, Option<i64>, String)> = vt(move |c| {
        Ok(c.query(
            "SELECT tb.id, tb.dosya_id, tb.barkod, tb.ptt_durumu, b.ad || ' ' || d.dosya_no, tb.evrak_aciklama, tb.borclu_id, tb.tebligat_turu
               FROM tebligat_barkod tb JOIN dosya d ON d.id = tb.dosya_id JOIN birim b ON b.id = d.birim_id
              WHERE d.yargi_turu = 2 AND d.durum_kod = 0 AND NOT tb.elektronik_tebligat
                AND (NOT tb.tebligat_mazbatasi_var OR tb.ptt_durumu = '')
                AND tb.barkod ~ '^[0-9]{13}$' AND ($1::text IS NULL OR b.birim_id = $1)
                AND ($2::date IS NULL OR d.acilis_tarihi::date >= $2) AND ($3::date IS NULL OR d.acilis_tarihi::date <= $3)
                AND ($4::bigint[] IS NULL OR d.id = ANY($4))
              -- Kesinleşme hesabına giren türler önce (tur yarıda kalsa da en önemlileri düzelsin), sonra yeni dosyalar.
              ORDER BY (tb.tebligat_turu IN ('Ödeme Emri', 'Tahliye Emri', '103 Davetiyesi', 'Kıymet Takdiri Raporu')) DESC,
                       d.yil DESC, d.sira_no DESC, tb.birim_evrak_no",
            &[&birim, &bas, &son, &secili],
        )
        .map_err(|e| format!("veritabanı: {e}"))?
        .iter()
        .map(|s| (s.get(0), s.get(1), s.get(2), s.get(3), s.get(4), s.get(5), s.get(6), s.get(7)))
        .collect())
    })
    .await?;
    // Açıklamada "Tebliğ Edildi" yazan (mazbata dönmüş) tebligat sorulmaz; ama kesinleşme hesabına giren türde PTT'den
    // tebliğ tarihi hiç alınmamışsa sorulur (tarih yoksa süre sayılamıyor; İzmir 10. İcra 2026/8645).
    let satirlar: Vec<_> = satirlar
        .into_iter()
        .filter(|(_, _, _, durum, _, aciklama, _, tur)| {
            !teb::ptt_sonuclandi(durum)
                && (!teb::teblig_sonucu_aciklamada_mi(aciklama)
                    || (durum.trim().is_empty() && crate::icra_durum::tebligat_kesinlesmesi(tur) != crate::icra_durum::TebligatKesinlesmesi::Yok))
        })
        .map(|(id, dosya, barkod, durum, etiket, aciklama, borclu, _)| (id, dosya, barkod, durum, etiket, aciklama, borclu))
        .collect();
    t.asama("ptt", satirlar.len());
    let mut ardisik = 0usize;
    for (sira, (id, dosya, barkod, durum, etiket, aciklama, borclu)) in satirlar.into_iter().enumerate() {
        t.ind.iptal_kontrol()?;
        if t.inner.lock().unwrap().ptt_kapandi {
            break;
        }
        t.mesaj_yaz(format!("PTT: {etiket} · {barkod} ({}/{})", sira + 1, t.inner.lock().unwrap().toplam));
        if let Some(v) = ptt_sor(t, u, &barkod, &mut ardisik).await? {
            let yeni = teb::ptt_durum_ve_tarih(&v).0;
            vt(move |c| {
                let borclu = match borclu {
                    Some(b) => Some(b),
                    None => teb::tebligat_borclusunu_bul(&aciklama, &kayit::dosya_borclulari(c, dosya)?, Some(&v)),
                };
                kayit::ptt_sonucu_kaydet(c, id, &v, borclu)
            })
            .await?;
            let mut g = t.inner.lock().unwrap();
            g.teb_yazilan += 1;
            if yeni != durum {
                g.teb_degisen += 1;
            }
        }
        t.ilerle();
    }
    Ok(())
}

/// UYAP'ta numarasıyla aranacak bir icra dosyası (veritabanından).
#[derive(Debug, Clone)]
struct ArananDosya {
    id: i64,
    yil: i32,
    sira_no: i32,
    tur_kod: i32,
    durum_kod: i32,
    birim_id: String,
    birim_ad: String,
}

impl ArananDosya {
    fn no(&self) -> String {
        format!("{}/{}", self.yil, self.sira_no)
    }
    fn etiket(&self) -> String {
        format!("{} {}", self.birim_ad, self.no())
    }
    /// UYAP kaydı bu dosya mı (daire, numara, tür).
    fn uyar(&self, r: &Value) -> bool {
        // Numara yıl/sıra olarak karşılaştırılır: yenilenen dosyada UYAP "2026/5683 - ( 2022/11833 )" verir.
        veri::yil_sira(&teb::metin(r, "dosyaNo")) == (self.yil, self.sira_no) && tur_kodu(r) == self.tur_kod && {
            let b = teb::metin(r, "birimId");
            b.is_empty() || b == self.birim_id
        }
    }
}

async fn aranan_dosyalar(idler: Vec<i64>) -> Result<Vec<ArananDosya>, String> {
    vt(move |c| {
        Ok(c.query(
            "SELECT d.id, d.yil, d.sira_no, d.tur_kod, d.durum_kod, b.birim_id, b.ad FROM dosya d JOIN birim b ON b.id = d.birim_id
              WHERE d.id = ANY($1) AND d.yargi_turu = 2 ORDER BY d.yil DESC, d.sira_no DESC",
            &[&idler],
        )
        .map_err(|e| format!("veritabanı: {e}"))?
        .iter()
        .map(|s| ArananDosya {
            id: s.get(0),
            yil: s.get(1),
            sira_no: s.get(2),
            tur_kod: s.get(3),
            durum_kod: s.get(4),
            birim_id: s.get(5),
            birim_ad: s.get(6),
        })
        .collect())
    })
    .await
}

/// Dosyayı UYAP'ta numarasıyla arar: önce `ilk_durum` listesinde (0 açık, 1 kapalı), bulunamazsa ötekinde.
/// Dış hata: ağ / iptal. İç hata: UYAP reddi. `Ok(Ok(None))`: iki listede de yok.
async fn dosya_ara<U: Uyap>(t: &Tarama, u: &U, a: &ArananDosya, ilk_durum: i64) -> Result<Result<Option<Value>, String>, String> {
    for durum in [ilk_durum, 1 - ilk_durum] {
        t.ind.iptal_kontrol()?;
        let govde = json!({"dosyaDurumKod": durum, "pageSize": 50, "pageNumber": 1, "dosyaYil": a.yil, "dosyaSira": a.sira_no,
                           "birimId": a.birim_id, "birimTuru2": BIRIM_TURU2, "birimTuru3": BIRIM_TURU3});
        let v = u.post(ARAMA, govde).await?;
        if let Some(neden) = sayfa_gecersiz(&v) {
            return Ok(Err(neden));
        }
        if let Some(r) = crate::gerekceli_karar::kayitlar_listesi(&v.unwrap_or(Value::Null)).into_iter().find(|r| a.uyar(r)) {
            return Ok(Ok(Some(r)));
        }
    }
    Ok(Ok(None))
}

/// Seçili dosyaların UYAP kayıtları, EN AZ istekle: dosya dosya aramak (dosya başına 1–2 istek) açık dosya listesini
/// almaktan (500'lük sayfalar + listede çıkmayanlar için birer arama) ucuzsa tek tek aranır; değilse liste alınıp
/// seçilenler ayıklanır. Bulunamayana dokunulmaz (`bulunamadi`).
///
/// Sıra: dosyalar `idler`deki sırayla aranır ve işlenir (Dosya Listesi'nin sıralaması ve süzgeci; kullanıcı isteği
/// 21.11.2026: "güncelleme listeleme sırasına göre sırayla yapılsın").
async fn secili_kayitlar<U: Uyap>(t: &Tarama, u: &U, idler: Vec<i64>) -> Result<Vec<Value>, String> {
    let istenen = idler.len();
    let mut sira: HashMap<i64, usize> = HashMap::new();
    for (i, id) in idler.iter().enumerate() {
        sira.entry(*id).or_insert(i);
    }
    let yeri = |id: i64| sira.get(&id).copied().unwrap_or(usize::MAX);
    let mut aranan = aranan_dosyalar(idler).await?;
    aranan.sort_by_key(|a| yeri(a.id));
    if aranan.len() < istenen {
        t.uyari(format!("{} seçili dosya icra dosyası değil ya da veritabanında yok; atlandı.", istenen - aranan.len()));
    }
    let acik_toplam: i64 = vt(|c| {
        Ok(c.query_one("SELECT count(*) FROM dosya WHERE yargi_turu = 2 AND durum_kod = 0", &[]).map_err(|e| format!("veritabanı: {e}"))?.get(0))
    })
    .await?;
    let kapali = aranan.iter().filter(|a| a.durum_kod != 0).count();
    let liste_maliyeti = (acik_toplam.max(0) as usize).div_ceil(t.sayfa_boyu.max(1)) + kapali;
    // (listedeki yeri, UYAP kaydı): sonda yere göre dizilir.
    let mut kayitlar: Vec<(usize, Value)> = Vec::new();
    // (aranacak dosya, önce bakılacak liste)
    let mut aranacak: Vec<(ArananDosya, i64)> = Vec::new();
    if aranan.len() > liste_maliyeti {
        let liste = listele(t, u).await?;
        for a in aranan {
            match liste.iter().find(|r| a.uyar(r)) {
                Some(r) => kayitlar.push((yeri(a.id), r.clone())),
                None => aranacak.push((a, 1)),
            }
        }
    } else {
        aranacak = aranan.into_iter().map(|a| {
            let ilk = if a.durum_kod == 0 { 0 } else { 1 };
            (a, ilk)
        }).collect();
    }
    t.asama("arama", aranacak.len());
    let mut ardisik = 0usize;
    for (sira, (a, ilk)) in aranacak.into_iter().enumerate() {
        if sira > 0 && sira % t.mola_her == 0 {
            mola(t).await?;
        }
        t.mesaj_yaz(format!("Seçili dosyalar UYAP'ta aranıyor: {} ({}/{})", a.etiket(), sira + 1, t.inner.lock().unwrap().toplam));
        match dosya_ara(t, u, &a, ilk).await {
            Ok(Ok(Some(r))) => {
                ardisik = 0;
                kayitlar.push((yeri(a.id), r));
            }
            Ok(Ok(None)) => {
                ardisik = 0;
                t.inner.lock().unwrap().bulunamadi += 1;
                t.uyari(format!("{}: UYAP'ta açık ve kapalı listede bulunamadı; dokunulmadı", a.etiket()));
            }
            Ok(Err(neden)) => {
                ardisik = 0;
                t.uyari(format!("{}: UYAP'ta aranamadı ({neden})", a.etiket()));
            }
            Err(e) => ag_hatasi_mi(&e, t, &a.etiket(), &mut ardisik)?,
        }
        t.ilerle();
    }
    {
        let mut g = t.inner.lock().unwrap();
        g.alinan = kayitlar.len();
        g.uyap_toplam = None;
    }
    kayitlar.sort_by_key(|x| x.0);
    Ok(kayitlar.into_iter().map(|x| x.1).collect())
}

/// Tek dosya: UYAP'ta numarasıyla bulunur (önce kayıtlı durumuna göre açık ya da kapalı listede, yoksa ötekinde),
/// künyesi yazılır (dosya no / durum güncellenir); istenirse tarafları ve tebligatları (tümü ya da seçilenler) alınır.
async fn tek_dosya<U: Uyap>(t: &Tarama, u: &U) -> Result<(), String> {
    let (dosya, taraf, tebligat, secim, bilinen) = {
        let g = t.inner.lock().unwrap();
        (g.dosya.ok_or("Dosya seçilmedi.")?, g.dosya_taraf, g.dosya_tebligat, g.evrak_secimi.clone(), g.uyap_id.clone())
    };
    let a = aranan_dosyalar(vec![dosya]).await?.into_iter().next().ok_or("Dosya veritabanında yok ya da icra dosyası değil.")?;
    let etiket = a.etiket();
    t.inner.lock().unwrap().dosya_etiket = etiket.clone();
    if let Some(uyap_id) = bilinen {
        // Dosya içinden "Dosyaya tebligat ekle": yalnız seçilen tebligat belgeleri (en az istek).
        return dosya_tebligat_adimi(t, u, dosya, uyap_id, etiket, &secim).await;
    }
    t.asama("dosya", 1);
    t.mesaj_yaz(format!("{etiket} UYAP'ta aranıyor…"));
    let rec = match dosya_ara(t, u, &a, if a.durum_kod == 0 { 0 } else { 1 }).await {
        Ok(Ok(Some(r))) => r,
        Ok(Ok(None)) => return Err(format!("{etiket} UYAP'ta bulunamadı (açık ve kapalı listelerde arandı).")),
        Ok(Err(neden)) => return Err(format!("{etiket}: UYAP'ta aranamadı ({neden})")),
        Err(e) => return Err(e.strip_prefix(AG_ONEKI).map(|x| format!("UYAP'a ulaşılamadı ({x})")).unwrap_or(e)),
    };
    let uyap_id = teb::metin(&rec, "dosyaId");
    let durum = teb::metin(&rec, "dosyaDurum");
    t.inner.lock().unwrap().dosya_uyap_kimligi = Some(uyap_id.clone()).filter(|x| !x.is_empty());
    vt(move |c| veri::dosya_kunyesi_kaydet(c, &rec, Some(YargiTuru::Icra.kod())).map(|_| ())).await?;
    {
        let mut g = t.inner.lock().unwrap();
        g.alinan = 1;
        g.yazilan = 1;
        g.rapor.push(format!("✔ Künye güncellendi: {etiket}{}", if durum.is_empty() { String::new() } else { format!(" ({durum})") }));
    }
    // Takip türü her elle güncellemede yeniden okunur (1 istek).
    takip_ayrintilari(t, u, vec![(dosya, uyap_id.clone(), etiket.clone())]).await?;
    if taraf {
        taraflari_cek(t, u, vec![TarafIsi { dosya, uyap_id: uyap_id.clone(), etiket: etiket.clone(), borclu_listesi: true, taraf_bilgisi: true }]).await?;
    }
    if tebligat {
        dosya_tebligat_adimi(t, u, dosya, uyap_id, etiket, &secim).await?;
    }
    Ok(())
}

/// Tek dosyanın tebligat adımı: evrak listesi, tebligat belgeleri (hepsi ya da `secim`), PTT.
async fn dosya_tebligat_adimi<U: Uyap>(t: &Tarama, u: &U, dosya: i64, uyap_id: String, etiket: String, secim: &[i32]) -> Result<(), String> {
    t.asama("tebligat", 1);
    t.mesaj_yaz(format!("{etiket}: tebligatlar alınıyor…"));
    let (mut ardisik, mut ptt_ardisik) = (0usize, 0usize);
    match evrak_listesi(u, &uyap_id).await {
        Ok(Ok(evraklar)) => {
            t.inner.lock().unwrap().teb_dosya = 1;
            let d: YazilanDosya = (dosya, uyap_id, etiket.clone());
            dosya_tebligatlari(t, u, &d, evraklar, secim, &mut ardisik, &mut ptt_ardisik).await?;
        }
        Ok(Err(neden)) => t.uyari(format!("{etiket}: evrak listesi alınamadı ({neden})")),
        Err(e) => return Err(e.strip_prefix(AG_ONEKI).map(|a| format!("UYAP'a ulaşılamadı ({a})")).unwrap_or(e)),
    }
    t.ilerle();
    Ok(())
}

/// Panel "Veritabanını güncelle"in son adımı (durum hesabından sonra): süresi dolmuş, mazbatası dönmüş ve UYAP'ta
/// kesinleşmesi görülmemiş borçlu varsa dosyanın kesinleşmesi UYAP'tan okunur (1 istek, salt okuma); hâlâ
/// kesinleştirilebilir kalan borçlular talep adayı olarak döner. Talep burada GÖNDERİLMEZ: kullanıcı onaylayınca
/// `Mod::Talep` ile gider.
async fn dosya_kesinlesme_adimi<U: Uyap>(t: &Tarama, u: &U) -> Result<(), String> {
    let (dosya, uyap_id, etiket) = {
        let g = t.inner.lock().unwrap();
        (g.dosya.ok_or("Dosya seçilmedi.")?, g.dosya_uyap_kimligi.clone(), g.dosya_etiket.clone())
    };
    let bugun = chrono::Local::now().date_naive();
    let satirlar = vt(move |c| crate::icra_kesinlestirme::dosya_satirlari(c, dosya, bugun)).await?;
    if satirlar.is_empty() {
        t.inner.lock().unwrap().rapor.push(
            "Kesinleşme: dosya Kesinleştirme kapsamında değil (yalnız alacaklı vekili olduğumuz açık İcra Dosyası türü dosyalar); UYAP'a sorulmadı.".into(),
        );
        return Ok(());
    }
    if !satirlar.iter().any(crate::icra_kesinlestirme::kesinlesme_sorulmali) {
        t.inner.lock().unwrap().rapor.push(
            "Kesinleşme: UYAP'a sorulmadı — kesinleşme süresi dolmuş ve mazbatası dönmüş, kesinleşmesi görülmemiş borçlu yok.".into(),
        );
        return Ok(());
    }
    let Some(uyap_id) = uyap_id else {
        t.uyari(format!("{etiket}: UYAP dosya kimliği yok; kesinleşme okunamadı"));
        return Ok(());
    };
    crate::icra_kesinlestirme::kesinlesme_oku(t, u, vec![(dosya, uyap_id, etiket)]).await?;
    let satirlar = vt(move |c| crate::icra_kesinlestirme::dosya_satirlari(c, dosya, bugun)).await?;
    let adaylar: Vec<Value> = satirlar
        .iter()
        .filter(|s| crate::icra_kesinlestirme::talep_adayi(s))
        .map(|s| json!({"bag": s.bag, "borclu": s.borclu, "grup": s.grup, "grup_etiket": crate::icra_kesinlestirme::grup_etiketi(s.grup)}))
        .collect();
    let mut g = t.inner.lock().unwrap();
    let satir = if g.kes_dosya == 0 {
        "⚠️ Kesinleşme UYAP'tan okunamadı (uyarılara bakın).".to_string()
    } else {
        format!("✔ Kesinleşme UYAP'tan okundu: {} borçlu UYAP'ta kesinleşmiş.", g.kes_kesinlesen)
    };
    g.rapor.push(satir);
    if !adaylar.is_empty() {
        g.rapor.push(format!("{} borçlu kesinleştirilebilir: talep göndermek için onayınız bekleniyor.", adaylar.len()));
    }
    g.talep_adaylari = adaylar;
    Ok(())
}

/// Taraf adımından sonra hâlâ borçlusu olmayan dosyalar (adları uyarılarda).
fn borclusuz_satiri(g: &TaramaInner) -> Option<String> {
    (!g.borclusuz.is_empty()).then(|| {
        let ornek: Vec<&str> = g.borclusuz.iter().take(5).map(String::as_str).collect();
        format!(
            "⚠️ {} dosyada UYAP'tan borçlu alınamadı (talimat dosyalarında borçlu olmayabilir): {}{}",
            g.borclusuz.len(),
            ornek.join(", "),
            if g.borclusuz.len() > 5 { " …" } else { "" }
        )
    })
}

/// Takip türü rapor satırı (okunan / okunamayan); bu turda hiç istenmediyse yok.
fn takip_satiri(g: &TaramaInner) -> Option<String> {
    (g.takip_okunan + g.takip_okunamayan > 0).then(|| {
        format!(
            "{} Takip türü: {} dosyada okundu{}.",
            if g.takip_okunamayan > 0 { "⚠️" } else { "✔" },
            g.takip_okunan,
            if g.takip_okunamayan > 0 { format!(", {} dosyada alınamadı (uyarılara bakın)", g.takip_okunamayan) } else { String::new() }
        )
    })
}

/// Tebligat / yalnız PTT rapor satırları.
fn tebligat_satirlari(g: &TaramaInner) -> Vec<String> {
    let mut s = Vec::new();
    match g.mod_ {
        Mod::Dosya if !g.dosya_tebligat => return s,
        Mod::Tebligat | Mod::Dosya => {
            s.push(format!(
                "Tebligat: {} dosyanın evrak listesi alındı · {} tebligat belgesi indirildi · {} barkod PTT'ye soruldu.",
                g.teb_dosya, g.pdf_indirilen, g.ptt_sorulan
            ));
            s.push(format!(
                "{} tebligat yazıldı: {} yeni, {} tebligatın durumu değişti{}.",
                g.teb_yazilan,
                g.teb_yeni,
                g.teb_degisen,
                if g.teb_atlanan > 0 { format!(" · atlanan evrak: {} (uyarılara bakın)", g.teb_atlanan) } else { String::new() }
            ));
            if !g.ptt_sorgusu {
                s.push("PTT sorgulanmadı (yalnız UYAP seçildi); kayıtlı PTT durumları korundu.".into());
            }
        }
        Mod::Ptt => s.push(format!(
            "Yalnız PTT ({}): {} sonuçlanmamış barkod, {} soruldu, {} tebligatın durumu değişti.",
            g.kapsam.metin(),
            g.toplam,
            g.ptt_sorulan,
            g.teb_degisen
        )),
        _ => return s,
    }
    if g.ptt_hatasi > 0 {
        s.push(format!("⚠️ {} barkod PTT'de sorgulanamadı (kayıtlı durumları korundu).", g.ptt_hatasi));
    }
    if g.ptt_kapandi {
        s.push("⚠️ PTT art arda yanıt vermediği için bu tur PTT sorgusu yarıda bırakıldı; \"Yalnız PTT\" ile sonra tamamlanabilir.".into());
    }
    s
}

/// 5. Sayım raporu.
async fn rapor_yaz(t: &Tarama, durduruldu: bool) -> Result<(), String> {
    let (alinan, uyap_toplam, tam, yazilan, kapsam, mod_, taraf_eksik) = {
        let g = t.inner.lock().unwrap();
        (g.alinan, g.uyap_toplam, g.tam, g.yazilan, g.kapsam.clone(), g.mod_, g.taraf_eksik)
    };
    if mod_ == Mod::TakipListesi {
        let g = &mut t.inner.lock().unwrap();
        let (tur, yol, sekil) = g.takip_listesi;
        g.rapor = vec![if durduruldu {
            "⚠️ Takip türü listesi alınamadı ya da yarıda kaldı; veritabanındaki liste değişmedi.".to_string()
        } else {
            format!("✔ UYAP takip türü listesi alındı: {tur} tür, {yol} yol, {sekil} şekil.")
        }];
        return Ok(());
    }
    if matches!(mod_, Mod::Mernis | Mod::YenidenTeblig) {
        let g = &mut t.inner.lock().unwrap();
        let mut satirlar = crate::icra_mernis::rapor_satirlari(g);
        if durduruldu {
            satirlar.push(format!("⚠️ Durduruldu: {}/{} işlendi.", g.i, g.toplam));
        }
        g.rapor = satirlar;
        return Ok(());
    }
    if matches!(mod_, Mod::Kesinlesme | Mod::Talep) {
        let g = &mut t.inner.lock().unwrap();
        let mut satirlar = crate::icra_kesinlestirme::rapor_satirlari(g);
        if durduruldu {
            satirlar.push(format!("⚠️ Durduruldu: {}/{} işlendi.", g.i, g.toplam));
        }
        g.rapor = satirlar;
        return Ok(());
    }
    if mod_ == Mod::Ptt {
        let g = &mut t.inner.lock().unwrap();
        let mut satirlar = tebligat_satirlari(g);
        if durduruldu {
            satirlar.push(format!("⚠️ Durduruldu: {}/{} barkod soruldu.", g.i, g.toplam));
        }
        g.rapor = satirlar;
        return Ok(());
    }
    if mod_ == Mod::Dosya {
        let g = &mut t.inner.lock().unwrap();
        let mut satirlar = std::mem::take(&mut g.rapor);
        if g.dosya_taraf && g.borclusuz.is_empty() && g.yazilan > 0 {
            satirlar.push("✔ Taraflar güncellendi.".into());
        }
        satirlar.extend(borclusuz_satiri(g));
        satirlar.extend(takip_satiri(g));
        // İş hiç başlamadan (ör. UYAP girişi yapılamadı) bittiyse "0 dosyanın evrak listesi alındı" gibi boş sayaçlar
        // yazılmaz (kullanıcı bulgusu 19.11.2026: hatanın altında yanıltıcı görünüyordu).
        if !(durduruldu && g.teb_dosya == 0) {
            satirlar.extend(tebligat_satirlari(g));
        }
        if durduruldu {
            satirlar.push("⚠️ Durduruldu.".into());
        }
        g.rapor = satirlar;
        return Ok(());
    }
    let secili = kapsam.dosyalar.as_ref().map(Vec::len);
    let mut satirlar = vec![match secili {
        Some(n) => {
            let bulunamadi = t.inner.lock().unwrap().bulunamadi;
            format!(
                "{} Seçili {n} dosya: {alinan} dosya UYAP'ta bulundu{}.",
                if alinan == n { "✔" } else { "⚠️" },
                if bulunamadi > 0 { format!(", {bulunamadi} dosya açık ve kapalı listede bulunamadı (dokunulmadı)") } else { String::new() }
            )
        }
        None => sayim_satiri(&format!("İcra ({})", kapsam.metin()), alinan, uyap_toplam, tam).0,
    }];
    if durduruldu {
        satirlar.push(format!("⚠️ Tarama durduruldu: {yazilan}/{alinan} dosya veritabanına yazıldı."));
    } else if yazilan < alinan {
        satirlar.push(format!("⚠️ {} dosya veritabanına yazılamadı (uyarılara bakın).", alinan - yazilan));
    } else {
        satirlar.push(format!("✔ Alınan {alinan} dosyanın tamamı veritabanına yazıldı."));
    }
    if tam && !durduruldu && secili.is_none() {
        let (birim, bas, son) = (kapsam.birim_id.clone(), kapsam.acilis_bas, kapsam.acilis_son);
        let db_acik: i64 = vt(move |c| {
            Ok(c.query_one(
                "SELECT count(*) FROM dosya d JOIN birim b ON b.id = d.birim_id
                  WHERE d.yargi_turu = 2 AND d.durum_kod = 0 AND ($1::text IS NULL OR b.birim_id = $1)
                    AND ($2::date IS NULL OR d.acilis_tarihi::date >= $2) AND ($3::date IS NULL OR d.acilis_tarihi::date <= $3)",
                &[&birim, &bas, &son],
            )
            .map_err(|e| format!("veritabanı: {e}"))?
            .get(0))
        })
        .await?;
        satirlar.push(if db_acik == alinan as i64 {
            format!("✔ Veritabanındaki açık icra dosyası sayısı ({db_acik}) UYAP ile birebir aynı.")
        } else {
            format!(
                "⚠️ Veritabanında {db_acik} açık icra dosyası var, UYAP'ta {alinan} — {} fark (UYAP'ta hiçbir listede bulunamayan ya da yazılamayan dosyalar).",
                (db_acik - alinan as i64).abs()
            )
        });
    }
    if mod_ == Mod::Liste && kapsam.tarihli() {
        satirlar.push("Tarih aralıklı güncellemede kapanan dosya kontrolü yapılmaz (bütün dosyalar ya da daire seçin).".into());
    }
    if mod_ == Mod::Liste && taraf_eksik > 0 {
        satirlar.push(format!("{taraf_eksik} dosyanın tarafları ya da borçlu kimlikleri eksik — \"Taraf bilgilerini güncelle\" ile alınabilir."));
    }
    let g = &mut t.inner.lock().unwrap();
    if mod_ == Mod::Liste && g.takip_eksik > 0 {
        satirlar.push(format!("{} dosyanın takip türü henüz okunmadı — \"Takip türü bilgisi\" ile alınabilir.", g.takip_eksik));
    }
    if g.yeni_dosya > 0 || g.kapandi > 0 || g.bulunamadi > 0 || g.taraf_cekilen > 0 {
        satirlar.push(format!(
            "Yeni dosya: {} · tarafları / borçlu kimlikleri alınan: {} · kapandı: {} · hiçbir listede bulunamayan (dokunulmadı): {}",
            g.yeni_dosya, g.taraf_cekilen, g.kapandi, g.bulunamadi
        ));
    }
    satirlar.extend(borclusuz_satiri(g));
    satirlar.extend(takip_satiri(g));
    satirlar.extend(tebligat_satirlari(g));
    g.rapor = satirlar;
    Ok(())
}

/// 6. Kesinleşme / tebliğ / takip aşaması hesabı (L7 otomatik işleri de buradan).
async fn hesapla(t: &Tarama) -> Result<(), String> {
    t.asama("hesap", 0);
    t.mesaj_yaz("Kesinleşme, tebliğ durumu ve takip aşamaları yeniden hesaplanıyor…");
    // Tek dosya güncellemesinde yalnız o dosya hesaplanır.
    let dosya = {
        let g = t.inner.lock().unwrap();
        if g.mod_ == Mod::Dosya { g.dosya } else { None }
    };
    let (s, notlar) = vt(move |c| {
        let notlar = StdMutex::new(Vec::new());
        let s = crate::icra_durum::durumlari_guncelle(c, dosya, chrono::Local::now().date_naive(), &|m: &str| {
            notlar.lock().unwrap().push(m.trim().to_string())
        })?;
        Ok((s, notlar.into_inner().unwrap()))
    })
    .await?;
    let mut g = t.inner.lock().unwrap();
    g.guncellenen = s.guncellenen;
    g.otomatik_is = s.otomatik_is;
    g.uyarilar.extend(notlar.into_iter().filter(|m| m.starts_with('⚠')));
    g.rapor.push(format!("Durum hesabı: {} borçlunun durumu değişti, {} otomatik iş açıldı.", s.guncellenen, s.otomatik_is));
    Ok(())
}

/// Bütün akış. Durdurulunca / koparsa da o ana kadarki sayım raporu yazılır.
pub async fn akis<U: Uyap>(t: &Tarama, u: &U) -> Result<(), String> {
    let tur = t.inner.lock().unwrap().mod_;
    if tur == Mod::TakipListesi {
        let sonuc = takip_listesi_al(t, u).await;
        rapor_yaz(t, sonuc.is_err()).await?;
        sonuc?;
        t.asama("bitti", 0);
        return Ok(());
    }
    if matches!(tur, Mod::Ptt | Mod::Dosya) {
        let sonuc = if tur == Mod::Ptt { yalniz_ptt(t, u).await } else { tek_dosya(t, u).await };
        rapor_yaz(t, sonuc.is_err()).await?;
        sonuc?;
        hesapla(t).await?;
        if tur == Mod::Dosya && t.inner.lock().unwrap().dosya_kesinlesme {
            dosya_kesinlesme_adimi(t, u).await?;
        }
        t.asama("bitti", 0);
        return Ok(());
    }
    let secili = t.inner.lock().unwrap().kapsam.dosyalar.clone();
    // Kimliklerin ömrü liste isteğinin BAŞINDAN sayılır (ilk sayfanın kimlikleri en eski olanlar).
    let liste_bas = now_secs();
    let kayitlar = match secili {
        Some(idler) => secili_kayitlar(t, u, idler).await?,
        None => listele(t, u).await?,
    };
    t.inner.lock().unwrap().kimlik_zamani = Some(liste_bas);
    let sonuc: Result<(), String> = async {
        let KunyeSonucu { gorulen, mut isler, dosyalar, yeniler } = kunyeleri_yaz(t, kayitlar).await?;
        // Seçimde kapanan dosyalar zaten tek tek kapalı listede bulunup künyeleriyle yazıldı.
        let (mod_, kapanan, hedefler, tam_liste) = {
            let g = t.inner.lock().unwrap();
            let tam_liste = g.tam && !g.kapsam.tarihli() && g.kapsam.dosyalar.is_none() && g.kapsam.birim_id.is_none();
            (
                g.mod_,
                matches!(g.mod_, Mod::Liste | Mod::Hepsi) && g.tam && !g.kapsam.tarihli() && g.kapsam.dosyalar.is_none(),
                g.kapsam.hedefler.clone().map(|h| h.into_iter().chain(yeniler.iter().copied()).collect::<HashSet<i64>>()),
                tam_liste,
            )
        };
        // Bütün açık dosyaların listesi eksiksiz alındı: "Dosya listesi en son …" bilgisi (Dosya Listesi onay kutusu).
        if matches!(mod_, Mod::Liste | Mod::Hepsi) && tam_liste {
            let n = gorulen.len();
            vt(move |c| crate::icra_dosyalar::liste_zamanini_yaz(c, n)).await?;
        }
        let hedefte = |d: i64| mod_ != Mod::Hepsi || hedefler.as_ref().is_none_or(|h| h.contains(&d));
        isler.retain(|x| hedefte(x.dosya));
        // Tüm veriler: taraf ve takip türü istekleri Dosya Listesi'ndeki sırayla (hedefler o sırayla gelir); bu turda
        // yeni gelen dosyalar sonda.
        let hedef_sirasi: HashMap<i64, usize> = {
            let g = t.inner.lock().unwrap();
            let mut m = HashMap::new();
            for (i, d) in g.kapsam.hedefler.iter().flatten().enumerate() {
                m.entry(*d).or_insert(i);
            }
            m
        };
        let yer = |d: i64| hedef_sirasi.get(&d).copied().unwrap_or(usize::MAX);
        if mod_ == Mod::Hepsi && !hedef_sirasi.is_empty() {
            isler.sort_by_key(|x| yer(x.dosya));
        }
        // Takip türü (dosya başına 1 istek, bir kez): dosya listesinde yalnız yeni dosyalar; taraf güncellemesinde
        // tarafı istenen ve yeni dosyalar; tebligat ve takip türü güncellemesinde kapsamdaki bütün dosyalar —
        // hepsinde yalnız takip türü hiç okunmamış olanlar.
        let takip_adaylari: Vec<YazilanDosya> = match mod_ {
            Mod::Liste => dosyalar.iter().filter(|x| yeniler.contains(&x.0)).cloned().collect(),
            Mod::Taraf => {
                let istenen: HashSet<i64> = isler.iter().map(|x| x.dosya).chain(yeniler.iter().copied()).collect();
                dosyalar.iter().filter(|x| istenen.contains(&x.0)).cloned().collect()
            }
            Mod::Hepsi => {
                let mut v: Vec<YazilanDosya> = dosyalar.iter().filter(|x| hedefte(x.0)).cloned().collect();
                if !hedef_sirasi.is_empty() {
                    v.sort_by_key(|x| yer(x.0));
                }
                v
            }
            // Kesinleştirme işleri takip türü sormaz (sekme zaten türü bilinen dosyaları gösterir).
            Mod::Kesinlesme | Mod::Talep | Mod::Mernis | Mod::YenidenTeblig => Vec::new(),
            _ => dosyalar.clone(),
        };
        if matches!(mod_, Mod::Taraf | Mod::Tebligat | Mod::Hepsi) {
            taraflari_cek(t, u, isler).await?;
        }
        takip_ayrintilari(t, u, takip_okunmamislar(takip_adaylari).await?).await?;
        if mod_ == Mod::Liste {
            let eksik = takip_eksik_say(gorulen.clone()).await?;
            t.inner.lock().unwrap().takip_eksik = eksik;
        }
        if mod_ == Mod::Kesinlesme {
            crate::icra_kesinlestirme::kesinlesme_oku(t, u, dosyalar.clone()).await?;
        }
        if mod_ == Mod::Talep {
            crate::icra_kesinlestirme::talepleri_gonder(t, u, &dosyalar).await?;
        }
        if mod_ == Mod::Mernis {
            crate::icra_mernis::mernis_sorgula(t, u, &dosyalar).await?;
        }
        if mod_ == Mod::YenidenTeblig {
            crate::icra_mernis::talepleri_gonder(t, u, &dosyalar).await?;
        }
        if mod_ == Mod::Tebligat {
            tebligatlari_cek(t, u, dosyalar).await?;
        }
        if kapanan {
            kapananlari_uzlastir(t, u, gorulen).await?;
        }
        Ok(())
    }
    .await;
    rapor_yaz(t, sonuc.is_err()).await?;
    sonuc?;
    hesapla(t).await?;
    t.asama("bitti", 0);
    Ok(())
}

// ── Çalıştırma ve uçlar ─────────────────────────────────────────────────────────────────────────

static KAYITLAR: Lazy<StdMutex<HashMap<String, Arc<Tarama>>>> = Lazy::new(|| StdMutex::new(HashMap::new()));

async fn calistir(t: Arc<Tarama>) {
    let mod_ = t.inner.lock().unwrap().mod_;
    let sonuc = {
        // Tek dosya kullanıcının elle tıklamasıdır: toplu işlerin sıra kilidini beklemez (UYAP'ın eş zamanlı
        // sorgu reddini `_uyap` tekrarlar). Toplu işler birbirini bekler.
        let _sira = if mod_ == Mod::Dosya { None } else { Some(evrak_toplu::sira_kilidi().lock().await) };
        match t.ind.iptal_kontrol() {
            Err(e) => Err(e),
            Ok(()) => {
                t.durum_mesaj(CALISIYOR, "Başladı…");
                akis(&t, &GercekUyap(t.ind.clone())).await
            }
        }
    };
    // Tur boyunca açık tutulan PTT tarayıcısı (açılmadıysa bir şey yapmaz). Tek dosya işi, süren toplu taramanın
    // tarayıcısını kapatmaz.
    let toplu_suruyor = || aktif().is_some_and(|a| !a.bitti_mi());
    if matches!(mod_, Mod::Tebligat | Mod::Ptt) || (mod_ == Mod::Dosya && !toplu_suruyor()) {
        crate::ptt_sorgu::toplu_kapat().await;
    }
    t.ind.inner.lock().unwrap().captcha = None;
    t.inner.lock().unwrap().mola_bitis = None;
    match sonuc {
        Ok(()) => {
            let (alinan, yeni, kapandi, cekilen, teb_yazilan, teb_yeni, teb_degisen, sorulan) = {
                let g = t.inner.lock().unwrap();
                (g.alinan, g.yeni_dosya, g.kapandi, g.taraf_cekilen, g.teb_yazilan, g.teb_yeni, g.teb_degisen, g.ptt_sorulan)
            };
            t.durum_mesaj(BITTI, match mod_ {
                Mod::Liste => format!("Tamamlandı: {alinan} açık dosya güncellendi ({yeni} yeni, {kapandi} kapandı)."),
                Mod::Taraf => format!("Tamamlandı: {cekilen} dosyanın tarafları / borçlu kimlikleri alındı ({alinan} dosya tarandı)."),
                Mod::Tebligat => format!(
                    "Tamamlandı: {alinan} dosya tarandı, {teb_yazilan} tebligat yazıldı ({teb_yeni} yeni, {teb_degisen} durumu değişti)."
                ),
                Mod::Ptt => format!("Tamamlandı: {sorulan} barkod PTT'ye soruldu, {teb_degisen} tebligatın durumu değişti."),
                Mod::Takip => {
                    let g = t.inner.lock().unwrap();
                    format!("Tamamlandı: {} dosyanın takip türü okundu ({alinan} dosya tarandı).{}", g.takip_okunan, okunamayan_eki(g.takip_okunamayan))
                }
                Mod::Hepsi => {
                    let g = t.inner.lock().unwrap();
                    if g.kapsam.dosyalar.is_some() {
                        // Dosya listesi yeniden alınmadan, yalnız süzgeçteki dosyaların eksikleri (21.11.2026).
                        format!(
                            "Tamamlandı: dosya listesi yeniden alınmadı; {alinan} dosya tarandı, {cekilen} dosyanın tarafları, {} dosyanın takip türü alındı.{}",
                            g.takip_okunan,
                            okunamayan_eki(g.takip_okunamayan)
                        )
                    } else {
                        format!(
                            "Tamamlandı: {alinan} açık dosya güncellendi ({yeni} yeni, {kapandi} kapandı); {cekilen} dosyanın tarafları, {} dosyanın takip türü alındı.{}",
                            g.takip_okunan,
                            okunamayan_eki(g.takip_okunamayan)
                        )
                    }
                }
                Mod::TakipListesi => {
                    let (tur, yol, sekil) = t.inner.lock().unwrap().takip_listesi;
                    format!("Tamamlandı: UYAP takip türü listesi alındı ({tur} tür, {yol} yol, {sekil} şekil).")
                }
                Mod::Kesinlesme | Mod::Talep => crate::icra_kesinlestirme::bitis_mesaji(&t.inner.lock().unwrap()),
                Mod::Mernis | Mod::YenidenTeblig => crate::icra_mernis::bitis_mesaji(&t.inner.lock().unwrap()),
                Mod::Dosya => {
                    let g = t.inner.lock().unwrap();
                    let mut m = format!("{} güncellendi", g.dosya_etiket);
                    if g.dosya_tebligat {
                        m.push_str(&format!(": {teb_yazilan} tebligat ({teb_yeni} yeni, {teb_degisen} durumu değişti)"));
                    }
                    if !g.borclusuz.is_empty() {
                        m.push_str(" — UYAP'tan borçlu alınamadı");
                    }
                    if !g.talep_adaylari.is_empty() {
                        m.push_str(&format!(" — {} borçlu kesinleştirilebilir", g.talep_adaylari.len()));
                    }
                    m + "."
                }
            });
        }
        Err(e) if e == IPTAL_HATASI => t.durum_mesaj(IPTAL, "Durduruldu. Yeniden başlatınca tamamlanan işler için istek atılmaz."),
        Err(e) => match e.strip_prefix(AG_ONEKI) {
            Some(ag) => t.durum_mesaj(HATA, format!("UYAP bağlantısı tekrar tekrar koptu ({ag}). Bir süre sonra yeniden deneyin.")),
            None => t.durum_mesaj(HATA, format!("Hata: {e}")),
        },
    }
    t.ind.inner.lock().unwrap().bitis = Some(now_secs());
}

fn temizle() {
    let simdi = now_secs();
    KAYITLAR.lock().unwrap().retain(|_, t| t.ind.inner.lock().unwrap().bitis.is_none_or(|b| simdi - b < SAKLAMA_SN));
}

/// Şu an süren bir toplu tarama var mı (güncelleme kurulumu bunu bekletir).
pub fn suruyor() -> bool {
    aktif().is_some_and(|t| !t.bitti_mi())
}

/// Süren tarama; yoksa en son başlayan.
pub fn aktif() -> Option<Arc<Tarama>> {
    temizle();
    let k = KAYITLAR.lock().unwrap();
    k.values().find(|t| !t.bitti_mi()).cloned().or_else(|| {
        k.values()
            .max_by(|a, b| a.ind.inner.lock().unwrap().baslangic.total_cmp(&b.ind.inner.lock().unwrap().baslangic))
            .cloned()
    })
}

pub fn baslat(mod_: Mod, kapsam: Kapsam, ptt_sorgusu: bool) -> Arc<Tarama> {
    if let Some(t) = aktif().filter(|t| !t.bitti_mi()) {
        return t;
    }
    let t = Tarama::new(mod_, kapsam);
    t.inner.lock().unwrap().ptt_sorgusu = ptt_sorgusu;
    KAYITLAR.lock().unwrap().insert(t.id(), t.clone());
    tokio::spawn(calistir(t.clone()));
    t
}

/// Tek dosya işleri ayrı tutulur (ana paneldeki "süren tarama" ile karışmaz); bitenler 10 dk sonra düşer.
static DOSYA_ISLERI: Lazy<StdMutex<HashMap<String, Arc<Tarama>>>> = Lazy::new(|| StdMutex::new(HashMap::new()));
const DOSYA_ISI_SAKLAMA_SN: f64 = 600.0;
/// Elle tek dosya isteği: az sayıda istek, bekletmeden (toplu taramalarda 3 sn).
const DOSYA_ISTEK_ARASI_SN: f64 = 1.0;

/// Tek dosya isteği.
#[derive(Debug, Clone, PartialEq)]
pub struct DosyaIstegi {
    pub dosya: i64,
    pub taraf: bool,
    pub tebligat: bool,
    pub evraklar: Vec<i32>,
    pub ptt: bool,
    /// Tebligattan sonra gerekiyorsa kesinleşme okunsun ve talep adayları çıkarılsın (panel "Veritabanını güncelle").
    pub kesinlesme: bool,
    /// UYAP dosya kimliği (dosya içindeki "Dosyaya tebligat ekle"): verilirse dosya UYAP'ta yeniden aranmaz, künye ve
    /// takip türü okunmaz; yalnız evrak listesi + seçilen tebligat belgeleri (+ PTT). `dosya` bu kimlikten bulunur.
    pub uyap_id: Option<String>,
    /// Dosya içinden gelen istekte kartın UYAP birim kimliği, dosya numarası ve tür kodu: veritabanındaki dosya önce
    /// bunlarla bulunur (UYAP dosya kimliği oturuma göre değişebilir; kayıtlı kimlik eski kalır).
    pub kunye: Option<(String, String, i32)>,
}

/// `{"dosya": id, "taraf"?: true, "tebligat"?: true, "evraklar"?: [evrak no], "ptt"?: true}` ya da
/// `{"uyap_dosya_id": "…", "evraklar": [evrak no], "ptt"?: true}` (dosya içinden tebligat ekleme; `dosya` 0 döner, çağıran
/// veritabanından bulur; taraf istenmez).
pub fn dosya_istegi(govde: &Value) -> Result<DosyaIstegi, String> {
    let uyap_id = Some(teb::metin(govde, "uyap_dosya_id")).filter(|x| !x.is_empty() && x.len() <= 255);
    let dosya = match &uyap_id {
        Some(_) => 0,
        None => govde.get("dosya").and_then(Value::as_i64).filter(|d| *d > 0).ok_or("Dosya kimliği geçersiz.")?,
    };
    let b = |a: &str| govde.get(a).and_then(Value::as_bool).unwrap_or(true);
    let evraklar: Vec<i32> = match govde.get("evraklar") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.len() <= 500 => {
            a.iter().map(|x| x.as_i64().and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0)).collect::<Option<_>>().ok_or("Evrak numarası geçersiz.")?
        }
        _ => return Err("Evrak listesi geçersiz.".into()),
    };
    if uyap_id.is_some() && evraklar.is_empty() {
        return Err("Eklenecek tebligat belgesi seçilmedi.".into());
    }
    let (taraf, tebligat) = (b("taraf") && uyap_id.is_none(), b("tebligat") || !evraklar.is_empty());
    let kunye = Some((teb::metin(govde, "birim_id"), teb::metin(govde, "dosya_no"), govde.get("dosya_tur_kod").and_then(Value::as_i64).unwrap_or(-1) as i32))
        .filter(|(b, n, _)| uyap_id.is_some() && !b.is_empty() && !n.is_empty());
    let kesinlesme = uyap_id.is_none() && govde.get("kesinlesme").and_then(Value::as_bool).unwrap_or(false);
    Ok(DosyaIstegi { dosya, taraf, tebligat, evraklar, ptt: b("ptt"), kesinlesme, uyap_id, kunye })
}

/// Dosya içinden gelen istekte veritabanındaki icra dosyası: önce birim + dosya numarası (+ tür kodu), bulunamazsa
/// kayıtlı UYAP dosya kimliği.
async fn uyap_kimliginden_dosya(uyap_id: String, kunye: Option<(String, String, i32)>) -> Result<i64, String> {
    vt(move |c| {
        let hata = |e: postgres::Error| format!("veritabanı: {e}");
        if let Some((birim, no, tur)) = &kunye {
            let s = c
                .query_opt(
                    "SELECT d.id FROM dosya d JOIN birim b ON b.id = d.birim_id
                      WHERE b.birim_id = $1 AND d.dosya_no = $2 AND d.yargi_turu = 2
                      ORDER BY (d.tur_kod = $3) DESC, d.id DESC LIMIT 1",
                    &[birim, &no.trim(), tur],
                )
                .map_err(hata)?;
            if let Some(s) = s {
                return Ok(s.get(0));
            }
        }
        c.query_opt("SELECT id FROM dosya WHERE dosya_id = $1 AND yargi_turu = 2 ORDER BY id DESC LIMIT 1", &[&uyap_id])
            .map_err(hata)?
            .map(|s| s.get(0))
            .ok_or_else(|| match &kunye {
                Some((_, no, _)) => format!("{no} büro veritabanında yok. Önce İcra Süreçleri'nden dosya listesini güncelleyin."),
                None => "Bu dosya büro veritabanında yok. Önce İcra Süreçleri'nden dosya listesini güncelleyin.".to_string(),
            })
    })
    .await
}

/// Tek dosya işini başlatır; aynı dosya için süren iş varsa o döner.
pub fn dosya_baslat(i: DosyaIstegi) -> Arc<Tarama> {
    let simdi = now_secs();
    let mut k = DOSYA_ISLERI.lock().unwrap();
    k.retain(|_, t| t.ind.inner.lock().unwrap().bitis.is_none_or(|b| simdi - b < DOSYA_ISI_SAKLAMA_SN));
    if let Some(t) = k.values().find(|t| !t.bitti_mi() && t.inner.lock().unwrap().dosya == Some(i.dosya)) {
        return t.clone();
    }
    let t = Tarama::new(Mod::Dosya, Kapsam::default());
    t.ind.inner.lock().unwrap().istek_arasi_sn = DOSYA_ISTEK_ARASI_SN;
    {
        let mut g = t.inner.lock().unwrap();
        g.dosya = Some(i.dosya);
        g.dosya_taraf = i.taraf;
        g.dosya_tebligat = i.tebligat;
        g.evrak_secimi = i.evraklar;
        g.ptt_sorgusu = i.ptt;
        g.dosya_kesinlesme = i.kesinlesme;
        g.uyap_id = i.uyap_id;
    }
    k.insert(t.id(), t.clone());
    tokio::spawn(calistir(t.clone()));
    t
}

/// Başlatma gövdesi → (tür, kapsam, PTT de sorulsun mu). Tarihler GG.AA.YYYY ya da YYYY-AA-GG.
pub fn baslatma_istegi(govde: &Value) -> Result<(Mod, Kapsam, bool), String> {
    let mod_ = Mod::coz(&teb::metin(govde, "mod")).ok_or("Güncelleme türü geçersiz.")?;
    let birim_id = teb::metin(govde, "birim_id").trim().to_string();
    if !birim_id.is_empty() && (birim_id.len() > 32 || !birim_id.chars().all(|c| c.is_ascii_digit())) {
        return Err("Birim kimliği geçersiz.".into());
    }
    let tarih = |a: &str| -> Result<Option<NaiveDate>, String> {
        let s = teb::metin(govde, a);
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        veri::tarih::gun_coz(s).map(Some).ok_or_else(|| format!("Tarih anlaşılamadı: {s}"))
    };
    let (bas, son) = (tarih("acilis_bas")?, tarih("acilis_son")?);
    if let (Some(b), Some(s)) = (bas, son) {
        if b > s {
            return Err("Açılış tarihi aralığı ters: başlangıç bitişten sonra.".into());
        }
    }
    let ptt = govde.get("ptt").and_then(Value::as_bool).unwrap_or(true);
    let dosyalar = crate::icra_api::dosya_listesi(govde)?;
    if dosyalar.as_ref().is_some_and(|d| d.is_empty()) {
        return Err("Seçili dosya yok.".into());
    }
    let secili = dosyalar.is_some();
    let hedefler = if mod_ == Mod::Hepsi {
        crate::icra_api::dosya_listesi(&json!({"dosyalar": govde.get("hedefler").cloned().unwrap_or(Value::Null)}))?
    } else {
        None
    };
    // Kesinleştirme işleri yalnız seçimle çalışır (sekmenin süzgecinden gelir); talepte borçlular da gerekir.
    if matches!(mod_, Mod::Kesinlesme | Mod::Talep | Mod::Mernis | Mod::YenidenTeblig) && !secili {
        return Err("Kesinleştirme işleri için dosya seçimi gerekir.".into());
    }
    let borclular = if matches!(mod_, Mod::Talep | Mod::Mernis | Mod::YenidenTeblig) {
        let b = crate::icra_api::dosya_listesi(&json!({"dosyalar": govde.get("borclular").cloned().unwrap_or(Value::Null)}))?;
        if b.as_ref().is_none_or(|b| b.is_empty()) {
            return Err("Talep gönderilecek borçlu seçilmedi.".into());
        }
        b
    } else {
        None
    };
    Ok((
        mod_,
        Kapsam {
            // Seçim varsa daire / tarih yok sayılır (seçim zaten süzgeçten gelir).
            birim_id: (!birim_id.is_empty() && !secili).then_some(birim_id),
            birim_adi: if secili { String::new() } else { veri::kirp(teb::metin(govde, "birim_adi").trim(), 120) },
            acilis_bas: bas.filter(|_| !secili),
            acilis_son: son.filter(|_| !secili),
            dosyalar,
            hedefler,
            borclular,
            yeniden: mod_ == Mod::Mernis && govde.get("yeniden").and_then(Value::as_bool).unwrap_or(false),
        },
        ptt,
    ))
}

fn json_resp(status: u16, val: Value) -> (u16, HashMap<String, String>, Vec<u8>) {
    let mut h = HashMap::new();
    h.insert("content-type".to_string(), "application/json; charset=utf-8".to_string());
    h.insert("cache-control".to_string(), "no-store".to_string());
    (status, h, serde_json::to_vec(&val).unwrap_or_default())
}

pub async fn handle_http(method: &str, path: &str, _qp: &HashMap<String, String>, body: &[u8]) -> (u16, HashMap<String, String>, Vec<u8>) {
    let rest = path.trim_start_matches('/').strip_prefix(ON_EK.trim_start_matches('/')).unwrap_or("").trim_matches('/');
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let mut govde = Value::Null;
    if method == "POST" && !body.is_empty() {
        match serde_json::from_slice::<Value>(body) {
            Ok(v) if v.is_object() => govde = v,
            Ok(_) => {}
            Err(_) => return json_resp(400, json!({"error": "Geçersiz JSON gövdesi."})),
        }
    }
    if parts.is_empty() {
        return match method {
            "GET" => json_resp(200, json!({"tarama": aktif().map(|t| t.sozluk())})),
            "POST" => match baslatma_istegi(&govde) {
                Ok((mod_, kapsam, ptt)) => json_resp(201, json!({"tarama": baslat(mod_, kapsam, ptt).sozluk()})),
                Err(e) => json_resp(400, json!({"error": e})),
            },
            _ => json_resp(405, json!({"error": "Yöntem desteklenmiyor."})),
        };
    }
    // Tek dosya: POST …/dosya {dosya, …} başlatır; …/dosya/<id>[/iptal|/captcha] toplu taramayla aynı.
    let (harita, parts): (&Lazy<StdMutex<HashMap<String, Arc<Tarama>>>>, &[&str]) =
        if parts[0] == "dosya" { (&DOSYA_ISLERI, &parts[1..]) } else { (&KAYITLAR, &parts[..]) };
    if parts.is_empty() {
        return match method {
            "POST" => match dosya_istegi(&govde) {
                Ok(mut i) => {
                    if let Some(uid) = i.uyap_id.clone() {
                        match uyap_kimliginden_dosya(uid, i.kunye.clone()).await {
                            Ok(d) => i.dosya = d,
                            Err(e) => return json_resp(404, json!({"error": e})),
                        }
                    }
                    json_resp(201, json!({"tarama": dosya_baslat(i).sozluk()}))
                }
                Err(e) => json_resp(400, json!({"error": e})),
            },
            _ => json_resp(405, json!({"error": "Yöntem desteklenmiyor."})),
        };
    }
    let Some(t) = harita.lock().unwrap().get(parts[0]).cloned() else {
        return json_resp(404, json!({"error": "Tarama bulunamadı (süresi dolmuş olabilir)."}));
    };
    match (parts.get(1).copied().unwrap_or(""), method) {
        ("", "GET") => json_resp(200, json!({"tarama": t.sozluk()})),
        ("iptal", "POST") => {
            t.ind.iptal_et();
            if t.durum() == SIRADA {
                t.ind.inner.lock().unwrap().bitis = Some(now_secs());
            }
            json_resp(200, json!({"tarama": t.sozluk()}))
        }
        ("captcha", "POST") => {
            let jeton = govde.get("token").or_else(|| govde.get("jeton")).and_then(|j| j.as_str()).filter(|j| j.len() < 8192).unwrap_or("");
            if t.durum() == CAPTCHA {
                t.ind.captcha_coz(jeton);
            }
            json_resp(200, json!({"tarama": t.sozluk()}))
        }
        _ => json_resp(405, json!({"error": "Yöntem/eylem desteklenmiyor."})),
    }
}

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

/// Hazır yanıt veren sahte UYAP: her çağrı kaydedilir, yanıt verilen işlevden gelir.
#[cfg(test)]
type SahteIndir = Box<dyn Fn(&HashMap<String, String>) -> Result<(u16, Vec<u8>), String> + Send + Sync>;
#[cfg(test)]
type SahtePtt = Box<dyn Fn(&str) -> Result<Value, String> + Send + Sync>;

/// Çağrılar `cagrilar`'a yazılır: POST'lar uç adıyla, indirmeler "GET <uç>" ({evrakId}), PTT "PTT" ({barkod}).
#[cfg(test)]
pub(crate) struct SahteUyap {
    pub cagrilar: StdMutex<Vec<(String, Value)>>,
    pub yanit: Box<dyn Fn(&str, &Value) -> Result<Option<Value>, String> + Send + Sync>,
    pub indir_yanit: SahteIndir,
    pub ptt_yanit: SahtePtt,
}

#[cfg(test)]
impl SahteUyap {
    pub(crate) fn yeni(yanit: Box<dyn Fn(&str, &Value) -> Result<Option<Value>, String> + Send + Sync>) -> Self {
        Self {
            cagrilar: StdMutex::new(Vec::new()),
            yanit,
            // Belge yerine oturum sayfası (UYAP'ın belgeyi vermediği durum): zarf "alınamadı" olur.
            indir_yanit: Box::new(|_| Ok((200, b"<html>oturum</html>".to_vec()))),
            ptt_yanit: Box::new(|_| Err("beklenmeyen PTT sorgusu".into())),
        }
    }
    pub(crate) fn say(&self, uc: &str) -> usize {
        self.cagrilar.lock().unwrap().iter().filter(|(u, _)| u == uc).count()
    }
}

#[cfg(test)]
impl Uyap for SahteUyap {
    fn post(&self, uc: &'static str, govde: Value) -> impl Future<Output = Result<Option<Value>, String>> + Send {
        let y = (self.yanit)(uc, &govde);
        self.cagrilar.lock().unwrap().push((uc.to_string(), govde));
        async move { y }
    }
    fn indir(&self, uc: &'static str, params: HashMap<String, String>) -> impl Future<Output = Result<(u16, Vec<u8>), String>> + Send {
        let y = (self.indir_yanit)(&params);
        self.cagrilar.lock().unwrap().push((format!("GET {uc}"), json!({"evrakId": params.get("evrakId")})));
        async move { y }
    }
    fn ptt(&self, barkod: String) -> impl Future<Output = Result<Value, String>> + Send {
        let y = (self.ptt_yanit)(&barkod);
        self.cagrilar.lock().unwrap().push(("PTT".into(), json!({"barkod": barkod})));
        async move { y }
    }
    /// Ham yanıt `yanit`tan: metin değer olduğu gibi bayt, öteki JSON metni.
    fn post_ham(&self, uc: &'static str, govde: Value) -> impl Future<Output = Result<(u16, Vec<u8>), String>> + Send {
        let y = (self.yanit)(uc, &govde).map(|v| match v {
            Some(Value::String(s)) => (200, s.into_bytes()),
            Some(v) => (200, v.to_string().into_bytes()),
            None => (200, Vec::new()),
        });
        self.cagrilar.lock().unwrap().push((uc.to_string(), govde));
        async move { y }
    }
    /// "İMZALI:" + içerik; çağrı "IMZA" ({ad}) olarak kaydedilir.
    fn imzala(&self, udf: Vec<u8>, ad: String) -> impl Future<Output = Result<Vec<u8>, String>> + Send {
        self.cagrilar.lock().unwrap().push(("IMZA".into(), json!({"ad": ad})));
        let mut out = b"IMZALI:".to_vec();
        out.extend(udf);
        async move { Ok(out) }
    }
    /// Alanlar JSON nesnesine çevrilip `yanit`a verilir (dosya alanı {"dosya_adi", "boyut"}).
    fn yukle(&self, uc: &'static str, alanlar: Vec<(String, Option<String>, Vec<u8>)>) -> impl Future<Output = Result<Option<Value>, String>> + Send {
        let mut g = serde_json::Map::new();
        for (ad, dosya, icerik) in alanlar {
            let v = match dosya {
                Some(d) => json!({"dosya_adi": d, "icerik": String::from_utf8_lossy(&icerik)}),
                None => json!(String::from_utf8_lossy(&icerik)),
            };
            g.insert(ad, v);
        }
        let govde = Value::Object(g);
        let y = (self.yanit)(uc, &govde);
        self.cagrilar.lock().unwrap().push((uc.to_string(), govde));
        async move { y }
    }
}

/// Gerçek veritabanı üzerinde sahte UYAP ile senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use crate::icra_veri::dosya_kunyesi_kaydet;
    let sessiz = |_: &str| {};
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let kunye = |no: &str, durum_kod: i64, did: &str| {
        json!({"dosyaId": did, "dosyaNo": no, "dosyaDurumKod": durum_kod, "dosyaDurum": if durum_kod == 0 { "Açık" } else { "Kapalı" },
               "dosyaTurKod": 35, "dosyaTur": "İcra Dosyası", "birimAdi": "İzmir 9. İcra Dairesi", "birimId": 9009,
               "dosyaAcilisTarihi": "01.11.2026"})
    };
    // Var olan: A (alacaklı + kimliksiz borçlu, listede), B (listede yok, kapalı listede var), C (hiçbir yerde yok)
    let (a, _) = dosya_kunyesi_kaydet(c, &kunye("2026/10", 0, "eski-a"), Some(YargiTuru::Icra.kod())).unwrap();
    kayit::dosya_taraf_kaydet(c, a, &[json!({"adi": "X BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum"}),
                                      json!({"adi": "ALİ VELİ", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    let (b, _) = dosya_kunyesi_kaydet(c, &kunye("2026/11", 0, "eski-b"), Some(YargiTuru::Icra.kod())).unwrap();
    kayit::dosya_taraf_kaydet(c, b, &[json!({"adi": "X BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum"})], &sessiz).unwrap();
    let (cc, _) = dosya_kunyesi_kaydet(c, &kunye("2026/12", 0, "eski-c"), Some(YargiTuru::Icra.kod())).unwrap();
    let durum_kod = |c: &mut postgres::Client, id: i64| -> i32 { c.query_one("SELECT durum_kod FROM dosya WHERE id = $1", &[&id]).unwrap().get(0) };

    // UYAP: açık liste 2'lik sayfalar → [A, D] + toplam 3, sonra [E]. D yeni (taraflar gelir), E yeni (taraf ucu hata verir).
    let yanit = move |eszamanli_ikinci_sayfa: bool| -> Box<dyn Fn(&str, &Value) -> Result<Option<Value>, String> + Send + Sync> {
        Box::new(move |uc: &str, g: &Value| -> Result<Option<Value>, String> {
            let k = |no: &str, durum: i64, did: &str| {
                json!({"dosyaId": did, "dosyaNo": no, "dosyaDurumKod": durum, "dosyaDurum": if durum == 0 { "Açık" } else { "Kapalı" },
                       "dosyaTurKod": 35, "dosyaTur": "İcra Dosyası", "birimAdi": "İzmir 9. İcra Dairesi", "birimId": 9009})
            };
            Ok(Some(match (uc, g["dosyaDurumKod"].as_i64(), g["pageNumber"].as_i64(), g["dosyaSira"].as_i64()) {
                (ARAMA, Some(0), Some(1), _) => json!([[k("2026/10", 0, "u-a"), k("2026/20", 0, "u-d")], 3]),
                (ARAMA, Some(0), Some(2), _) if eszamanli_ikinci_sayfa => {
                    json!({"errorCode": "PRTL_GNL_10001", "error": "Eş zamanlı olarak birden fazla sorgulama yapamazsınız! "})
                }
                (ARAMA, Some(0), Some(2), _) => json!([[k("2026/21", 0, "u-e")], 3]),
                (ARAMA, Some(1), _, Some(11)) => json!([[k("2026/11", 1, "u-b")], 1]),
                (ARAMA, Some(1), _, Some(30)) => json!([[k("2026/30", 1, "u-g")], 1]),
                // G: borçlu listesi BOŞ döner; borçlu taraf bilgilerinden alınmalı
                (BORCLU_LISTESI, ..) if g["dosyaId"] == "u-g" => json!([]),
                (TARAF_BILGILERI, ..) if g["dosyaId"] == "u-g" => json!([
                    {"adi": "Z BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum"},
                    {"adi": "VELİ CAN", "rol": "Borçlu", "kisiKurum": "Kişi"}
                ]),
                (TARAF_BILGILERI, ..) if g["dosyaId"] == "u-a" => json!([
                    {"adi": "X BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum", "vekil": "[MEHMET DEMİR]"},
                    {"adi": "ALİ VELİ", "rol": "Borçlu", "kisiKurum": "Kişi", "vekil": "[UTKU AVUKAT, AYŞE YILMAZ]"}
                ]),
                (ARAMA, Some(1), _, _) => json!([[], 0]),
                (BORCLU_LISTESI, ..) if g["dosyaId"] == "u-d" => {
                    json!([{"adi": "AYŞE", "soyadi": "KARA", "tcKimlikNo": "12345678901", "kisiKurum": "Kişi"}])
                }
                (BORCLU_LISTESI, ..) if g["dosyaId"] == "u-a" => {
                    json!([{"adi": "ALİ", "soyadi": "VELİ", "tcKimlikNo": "10987654321", "kisiKurum": "Kişi"}])
                }
                (TARAF_BILGILERI, ..) if g["dosyaId"] == "u-d" => json!([
                    {"adi": "Y FAKTORİNG A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum", "vekil": "[UTKU AVUKAT]"},
                    {"adi": "AYŞE KARA", "rol": "Borçlu", "kisiKurum": "Kişi"}
                ]),
                (BORCLU_LISTESI | TARAF_BILGILERI, ..) if g["dosyaId"] == "u-e" => json!({"errorCode": "PRTL_GNL_500", "error": "Sunucu hatası"}),
                // Tebligat: A'da iki tebligat (fiziki + e-tebligat) ve bir ödeme emri; D'de evrak yok; E'nin listesi alınamaz.
                (evrak_toplu::EVRAK_LISTESI_TOPLAM, ..) if g["dosyaId"] == "u-e" => json!({"errorCode": "PRTL_GNL_500", "error": "Sunucu hatası"}),
                (evrak_toplu::EVRAK_LISTESI_TOPLAM, ..) => json!(1),
                (evrak_toplu::EVRAK_LISTESI, ..) if g["dosyaId"] == "u-a" => json!({"tumEvraklar": {"grup": [
                    {"evrakId": "t1", "birimEvrakNo": 5, "tur": "Kapalı Tebligat", "aciklama": "Normal Tebligat[BORÇLU ALİ VELİ]", "onaylandigiTarih": "01/11/2026"},
                    {"evrakId": "t2", "birimEvrakNo": 6, "tur": "Kapalı Tebligat", "aciklama": "Elektronik Tebligat[BORÇLU ALİ VELİ]", "onaylandigiTarih": "02/11/2026"},
                    // t3: UYAP belge yerine oturum sayfası verir → alınamaz, nedeni saklanır
                    {"evrakId": "t3", "birimEvrakNo": 7, "tur": "Kapalı Tebligat", "aciklama": "Normal Tebligat[BORÇLU ALİ VELİ]", "onaylandigiTarih": "03/11/2026"},
                    {"evrakId": "o1", "birimEvrakNo": 4, "tur": "Ödeme Emri", "aciklama": "", "onaylandigiTarih": "01/11/2026"}
                ]}}),
                (evrak_toplu::EVRAK_LISTESI, ..) => json!({"tumEvraklar": {}}),
                (evrak_toplu::DOC_VIEWER_HAZIRLIK, ..) => json!({}),
                // Takip türü: E'nin ayrıntısı alınamaz; diğerleri İlamsız / Genel haciz / Örnek 7
                (DOSYA_AYRINTI, ..) if g["dosyaId"] == "u-e" => json!({"errorCode": "PRTL_GNL_500", "error": "Sunucu hatası"}),
                (DOSYA_AYRINTI, ..) => json!({"takibinTuru": "1", "takibinTuruAciklama": "İlamsız Takip", "takibinYolu": "0",
                                              "takibinYoluAciklama": "Genel Haciz Yoluyla Takip", "takibinSekli": "0",
                                              "takibinSekliAciklama": " ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49 ",
                                              "alacakKalemToplamTutar": 1500}),
                (TAKIP_TURU_LISTESI, ..) => json!([{"value": 0, "name": "İlamlı Takip"}, {"value": 1, "name": "İlamsız Takip"}]),
                (TAKIP_YOLU_LISTESI, ..) if g["takipTuru"] == 1 => json!([{"value": 0, "name": "Genel Haciz Yoluyla Takip"}]),
                (TAKIP_YOLU_LISTESI, ..) => json!({"errorCode": "PRTL_GNL_500", "error": "Sunucu hatası"}),
                (TAKIP_SEKLI_LISTESI, ..) if g["takipTuru"] == 1 && g["takipYolu"] == 0 => json!([
                    {"value": 0, "name": " ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49 "},
                    {"value": 1, "name": "ÖRNEK: 10 Kambiyo Senetlerine Mahsus Haciz Yoluyla Takiplerde Ödeme Emri"}
                ]),
                _ => return Err(format!("beklenmeyen istek: {uc} {g}")),
            }))
        })
    };
    let calistir = |mod_: Mod, eszamanli: bool, bas: Option<NaiveDate>| {
        let kapsam = Kapsam { birim_id: Some("9009".into()), birim_adi: "İzmir 9. İcra Dairesi".into(), acilis_bas: bas, acilis_son: None, dosyalar: None, hedefler: None, borclular: None, yeniden: false };
        let t = Tarama::new(mod_, kapsam);
        let mut t = Arc::try_unwrap(t).ok().unwrap();
        t.sayfa_boyu = 2;
        t.mola_her = 1000;
        let u = SahteUyap::yeni(yanit(eszamanli));
        let r = rt.block_on(akis(&t, &u));
        (t, u, r)
    };
    let (barkod_1, barkod_2) = ("4007663523293", "5002864776768");
    let teb_calistir = |mod_: Mod, ptt: SahtePtt| {
        let kapsam = Kapsam { birim_id: Some("9009".into()), birim_adi: "İzmir 9. İcra Dairesi".into(), acilis_bas: None, acilis_son: None, dosyalar: None, hedefler: None, borclular: None, yeniden: false };
        let mut t = Arc::try_unwrap(Tarama::new(mod_, kapsam)).ok().unwrap();
        t.sayfa_boyu = 2;
        t.mola_her = 1000;
        t.ptt_tekrar_sn = 0;
        let mut u = SahteUyap::yeni(yanit(false));
        u.indir_yanit = Box::new(move |p: &HashMap<String, String>| {
            Ok((200, match p.get("evrakId").map(String::as_str) {
                Some("t1") => teb::ornek_pdf(format!("(*{barkod_1}*) Tj (BU ZARFTA \\326deme \\335cra Emri VARDIR) Tj").as_bytes()),
                Some("t2") => teb::ornek_pdf(format!("(*{barkod_2}*) Tj (BU ZARFTA \\326deme \\335cra Emri VARDIR) Tj").as_bytes()),
                _ => b"<html>oturum</html>".to_vec(),
            }))
        });
        u.ptt_yanit = ptt;
        let r = rt.block_on(akis(&t, &u));
        (t, u, r)
    };
    let ptt_durumu = |durum: &'static str, tarih: &'static str| -> SahtePtt {
        Box::new(move |_: &str| Ok(json!({"errorState": true, "sondurum": {"son_durum_aciklama": durum, "teslim_tarihi": tarih, "son_islem_tarihi": tarih}})))
    };
    let taraf_istegi = |u: &SahteUyap| u.cagrilar.lock().unwrap().iter().filter(|(uc, _)| uc != ARAMA && uc != DOSYA_AYRINTI).count();
    let takip = |c: &mut postgres::Client, id: i64| -> Option<(String, String, String, String, bool)> {
        c.query_opt(
            "SELECT takibin_turu_aciklama, takibin_yolu_aciklama, takibin_sekli_aciklama, ornek_no, ayrinti_zamani IS NOT NULL
               FROM icra_takip_detay WHERE dosya_id = $1",
            &[&id],
        )
        .unwrap()
        .map(|s| (s.get(0), s.get(1), s.get(2), s.get(3), s.get(4)))
    };
    let kapanan_istegi = |u: &SahteUyap| u.cagrilar.lock().unwrap().iter().any(|(uc, g)| uc == ARAMA && g["dosyaDurumKod"] == 1);

    // 1) Dosya listesi, ikinci sayfa reddedilir: liste eksik → kapanan kontrolü YOK; taraf hiç istenmez
    let (t, u, r) = calistir(Mod::Liste, true, None);
    r.unwrap();
    let g = t.inner.lock().unwrap();
    assert!(!g.tam);
    assert_eq!((g.alinan, g.uyap_toplam, g.yeni_dosya), (2, Some(3), 1));
    assert!(g.rapor[0].contains("YARIM"), "{:?}", g.rapor);
    assert!(g.uyarilar.iter().any(|x| x.contains("eş zamanlı")), "{:?}", g.uyarilar);
    assert_eq!(g.taraf_eksik, 2, "A (kimliksiz borçlu) + D (tarafsız)");
    assert!(g.rapor.iter().any(|x| x.contains("Taraf bilgilerini güncelle")), "{:?}", g.rapor);
    drop(g);
    assert!(!kapanan_istegi(&u), "eksik listede kapanan kontrolü yapılmaz");
    assert_eq!(taraf_istegi(&u), 0, "dosya listesi düğmesi taraf istemez");
    // Takip türü: dosya listesinde yalnız YENİ dosya (D) için istenir; A okunmamış kalır ve raporda sayılır
    let ayrinti_istenen: Vec<String> = u.cagrilar.lock().unwrap().iter().filter(|(uc, _)| uc == DOSYA_AYRINTI)
        .map(|(_, g)| g["dosyaId"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(ayrinti_istenen, vec!["u-d".to_string()]);
    let g = t.inner.lock().unwrap();
    assert_eq!((g.takip_okunan, g.takip_okunamayan, g.takip_eksik), (1, 0, 1));
    assert!(g.rapor.iter().any(|x| x.contains("1 dosyanın takip türü henüz okunmadı")), "{:?}", g.rapor);
    assert!(g.rapor.iter().any(|x| x.starts_with("✔ Takip türü: 1 dosyada okundu")), "{:?}", g.rapor);
    drop(g);
    let d_yeni: i64 = c.query_one("SELECT id FROM dosya WHERE dosya_no = '2026/20'", &[]).unwrap().get(0);
    assert_eq!(
        takip(c, d_yeni),
        Some(("İlamsız Takip".into(), "Genel Haciz Yoluyla Takip".into(), "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49".into(), "7".into(), true))
    );
    assert_eq!(takip(c, a), None);
    assert_eq!((durum_kod(c, b), durum_kod(c, cc)), (0, 0));

    // 2) Dosya listesi, tarih aralıklı: UYAP'a aralık gider, kapanan kontrolü yapılmaz
    let (t, u, r) = calistir(Mod::Liste, false, NaiveDate::from_ymd_opt(2026, 11, 1));
    r.unwrap();
    assert!(t.inner.lock().unwrap().tam);
    assert_eq!(u.cagrilar.lock().unwrap()[0].1["dosyaAcilisTarihiStart"], "01.11.2026");
    assert!(!kapanan_istegi(&u), "tarih aralığında kapanan kontrolü yapılmaz");
    assert!(t.inner.lock().unwrap().rapor.iter().any(|x| x.contains("kapanan dosya kontrolü yapılmaz")));
    assert_eq!(durum_kod(c, b), 0);

    // 3) Dosya listesi, tam: B kapandı + Pasif, C'ye dokunulmadı
    let (t, u, r) = calistir(Mod::Liste, false, None);
    r.unwrap();
    let g = t.inner.lock().unwrap();
    assert_eq!((g.alinan, g.yeni_dosya, g.kapandi, g.bulunamadi, g.aday), (3, 0, 1, 1, 2));
    assert!(g.rapor[0].starts_with('✔'), "{:?}", g.rapor);
    assert!(g.rapor.iter().any(|x| x.contains("Veritabanında 4 açık")), "{:?}", g.rapor);
    assert!(g.rapor.iter().any(|x| x.starts_with("Durum hesabı")), "{:?}", g.rapor);
    assert_eq!(g.asama, "bitti");
    drop(g);
    assert_eq!(taraf_istegi(&u), 0);
    assert_eq!((durum_kod(c, b), durum_kod(c, cc)), (1, 0));
    let islem: String = c.query_one("SELECT islem_durumu FROM dosya WHERE id = $1", &[&b]).unwrap().get(0);
    assert_eq!(islem, "pasif");

    // 4) Taraf bilgileri: D (tarafsız) iki uç; A (kimliksiz borçlu, taraf bilgileri hiç okunmamış) iki uç — borçlunun
    //    vekilleri kayıtlı borçluya yazılır, borçlu çoğalmaz; E hata → uyarı; kapanan YOK
    let (t, u, r) = calistir(Mod::Taraf, false, None);
    r.unwrap();
    let g = t.inner.lock().unwrap();
    assert_eq!(g.taraf_cekilen, 2, "{:?}", g.uyarilar);
    assert!(g.uyarilar.iter().any(|x| x.contains("2026/21") && x.contains("taraflar alınamadı")), "{:?}", g.uyarilar);
    drop(g);
    assert!(!kapanan_istegi(&u));
    let cagri: Vec<(String, String)> = u.cagrilar.lock().unwrap().iter().filter(|(uc, _)| uc != ARAMA)
        .map(|(uc, g)| (uc.clone(), g["dosyaId"].as_str().unwrap_or("").to_string())).collect();
    assert!(cagri.contains(&(BORCLU_LISTESI.into(), "u-a".into())) && cagri.contains(&(TARAF_BILGILERI.into(), "u-a".into())), "{cagri:?}");
    assert!(cagri.contains(&(TARAF_BILGILERI.into(), "u-d".into())), "{cagri:?}");
    let d: i64 = c.query_one("SELECT id FROM dosya WHERE dosya_no = '2026/20'", &[]).unwrap().get(0);
    let taraflar = |c: &mut postgres::Client, id: i64| -> Vec<(String, Option<String>, bool)> {
        c.query(
            "SELECT dt.rol, t.tckn, dt.uyapta_gorulmedi IS NOT NULL FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id
              WHERE dt.dosya_id = $1 ORDER BY dt.rol",
            &[&id],
        )
        .unwrap()
        .iter()
        .map(|s| (s.get(0), s.get(1), s.get(2)))
        .collect()
    };
    assert_eq!(taraflar(c, d), vec![("alacakli".into(), None, false), ("borclu".into(), Some("12345678901".into()), false)]);
    assert_eq!(taraflar(c, a), vec![("alacakli".into(), None, false), ("borclu".into(), Some("10987654321".into()), false)], "kimlik eklendi, borçlu çoğalmadı");
    let borclu_vekilleri: Vec<String> = c
        .query("SELECT v.ad || ' ' || v.soyad FROM dosya_taraf dt JOIN dosya_taraf_vekiller dv ON dv.dosyataraf_id = dt.id
                  JOIN vekil v ON v.id = dv.vekil_id WHERE dt.dosya_id = $1 AND dt.rol = 'borclu' ORDER BY 1", &[&a])
        .unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(borclu_vekilleri, vec!["AYŞE YILMAZ".to_string(), "UTKU AVUKAT".to_string()], "borçlunun vekilleri yazıldı");
    let okundu: bool = c.query_one("SELECT taraf_bilgisi_zamani IS NOT NULL FROM dosya WHERE id = $1", &[&a]).unwrap().get(0);
    assert!(okundu);

    // 5) Taraf bilgileri yeniden: tarafı tam dosyalara (A, D) istek atılmaz
    let (_, u, r) = calistir(Mod::Taraf, false, None);
    r.unwrap();
    assert!(!u.cagrilar.lock().unwrap().iter().any(|(uc, g)| uc != ARAMA && (g["dosyaId"] == "u-a" || g["dosyaId"] == "u-d")));

    // 5b) Tüm veriler: dosya listesi alınır, tarafı tam dosyalara (A, D) taraf isteği gitmez, rapor hepsini anlatır
    let (t, u, r) = calistir(Mod::Hepsi, false, None);
    r.unwrap();
    assert_eq!(t.inner.lock().unwrap().asama, "bitti");
    assert!(!u.cagrilar.lock().unwrap().iter().any(|(uc, g)| (uc == TARAF_BILGILERI || uc == BORCLU_LISTESI)
        && (g["dosyaId"] == "u-a" || g["dosyaId"] == "u-d")));

    // 6) Tebligat (UYAP + PTT): A'da 2 PDF, 1 PTT (e-tebligat PTT'ye gitmez); D evraksız; E'nin listesi alınamaz
    let tebligat = |c: &mut postgres::Client, no: i32| -> (String, String, String, bool, Option<i64>) {
        let s = c
            .query_one(
                "SELECT tb.barkod, tb.tebligat_turu, tb.ptt_durumu, tb.elektronik_tebligat, tb.borclu_id FROM tebligat_barkod tb
                  WHERE tb.dosya_id = $1 AND tb.birim_evrak_no = $2",
                &[&a, &no],
            )
            .unwrap();
        (s.get(0), s.get(1), s.get(2), s.get(3), s.get(4))
    };
    let (t, u, r) = teb_calistir(Mod::Tebligat, ptt_durumu("DAĞITIMDA", "20261103"));
    r.unwrap();
    let g = t.inner.lock().unwrap();
    assert_eq!((g.teb_dosya, g.pdf_indirilen, g.ptt_sorulan, g.teb_yazilan, g.teb_yeni, g.teb_degisen, g.teb_atlanan), (2, 3, 1, 2, 2, 0, 1), "{:?}", g.uyarilar);
    assert!(g.uyarilar.iter().any(|x| x.contains("2026/21") && x.contains("evrak listesi alınamadı")), "{:?}", g.uyarilar);
    assert!(g.rapor.iter().any(|x| x.starts_with("Tebligat: 2 dosyanın")), "{:?}", g.rapor);
    assert_eq!(g.asama, "bitti");
    drop(g);
    assert!(!kapanan_istegi(&u), "tebligat güncellemesinde kapanan kontrolü yok");
    assert_eq!(u.say(TARAF_BILGILERI), 1, "yalnız tarafı hiç olmayan E; kimliksiz borçlu için istek yok");
    // Tebligat güncellemesi takip türü okunmamış dosyaları da okur (A; D zaten okunmuştu, E alınamaz)
    assert!(takip(c, a).is_some_and(|x| x.4), "A'nın takip türü okundu");
    assert!(t.inner.lock().unwrap().uyarilar.iter().any(|x| x.contains("2026/21") && x.contains("takip türü alınamadı")));
    let (bk, tur, durum, elektronik, borclu) = tebligat(c, 5);
    assert_eq!((bk.as_str(), tur.as_str(), durum.as_str(), elektronik), (barkod_1, "Ödeme Emri", "DAĞITIMDA", false));
    assert!(borclu.is_some(), "tebligat ALİ VELİ'ye bağlandı");
    let (bk, _, durum, elektronik, _) = tebligat(c, 6);
    assert_eq!((bk.as_str(), elektronik), (barkod_2, true));
    assert!(durum.starts_with("E-Tebligat"), "{durum}");
    let evrak_sayisi: i64 = c.query_one("SELECT count(*) FROM evrak WHERE dosya_id = $1", &[&a]).unwrap().get(0);
    assert_eq!(evrak_sayisi, 4, "evrak listesi de yazıldı");
    // Alınamayan zarf (t3): kayıt yok, nedeni evrakta; dosya panelinde "alınamayan" olarak görünür
    let ayr = crate::icra_api::dosya_ayrintisi(c, a).unwrap().unwrap();
    let alinamayan = ayr["alinamayan"].as_array().unwrap().clone();
    assert_eq!(alinamayan.len(), 1, "{alinamayan:?}");
    assert_eq!(alinamayan[0]["evrak_no"], 7);
    assert!(alinamayan[0]["neden"].as_str().unwrap().contains("PDF değil"), "{alinamayan:?}");

    // 7) Yalnız PTT: UYAP'a hiç gidilmez; yalnız fiziki tebligat sorulur, durum değişir
    let (t, u, r) = teb_calistir(Mod::Ptt, ptt_durumu("TESLİM EDİLDİ", "20261105"));
    r.unwrap();
    assert_eq!(u.cagrilar.lock().unwrap().iter().filter(|(uc, _)| uc != "PTT").count(), 0, "UYAP'a istek yok");
    assert_eq!(u.say("PTT"), 1);
    let g = t.inner.lock().unwrap();
    assert_eq!((g.ptt_sorulan, g.teb_degisen), (1, 1));
    assert!(g.rapor.iter().any(|x| x.starts_with("Yalnız PTT") && x.contains("1 sonuçlanmamış barkod")), "{:?}", g.rapor);
    drop(g);
    assert_eq!(tebligat(c, 5).2, "TESLİM EDİLDİ");

    // 8) Tebligat yeniden: barkodu bilinen evrakın PDF'i indirilmez, sonucu kesin barkod PTT'ye sorulmaz
    let (t, u, r) = teb_calistir(Mod::Tebligat, Box::new(|_: &str| Err("sorulmamalıydı".into())));
    r.unwrap();
    // Yalnız alınamamış zarf (t3) yeniden denenir; barkodu bilinenlerin belgesi indirilmez
    let indirilen: Vec<String> = u.cagrilar.lock().unwrap().iter().filter(|(uc, _)| uc.starts_with("GET "))
        .map(|(_, g)| g["evrakId"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!((indirilen, u.say("PTT")), (vec!["t3".to_string()], 0));
    let g = t.inner.lock().unwrap();
    assert_eq!((g.teb_yazilan, g.teb_yeni, g.teb_degisen, g.ptt_hatasi), (2, 0, 0, 0), "{:?}", g.uyarilar);
    drop(g);
    assert_eq!(tebligat(c, 5).2, "TESLİM EDİLDİ", "kesin sonuç korunur");

    // 9) PTT yanıt vermez: bir kez yeniden denenir, kayıtlı durum korunur, uyarı yazılır
    c.execute("UPDATE tebligat_barkod SET ptt_durumu = 'DAĞITIMDA' WHERE dosya_id = $1 AND birim_evrak_no = 5", &[&a]).unwrap();
    let (t, u, r) = teb_calistir(Mod::Ptt, Box::new(|_: &str| Err("PTT yanıt vermedi".into())));
    r.unwrap();
    assert_eq!(u.say("PTT"), 2, "bir yeniden deneme");
    let g = t.inner.lock().unwrap();
    assert_eq!((g.ptt_hatasi, g.ptt_kapandi), (1, false));
    assert!(g.rapor.iter().any(|x| x.contains("sorgulanamadı")), "{:?}", g.rapor);
    drop(g);
    assert_eq!(tebligat(c, 5).2, "DAĞITIMDA");

    // 9b) Mazbata dönmüş ("… - Tebliğ Edildi") ama PTT tarihi yok: ödeme emrinde sorulur, kesinleşmeye girmeyen türde sorulmaz
    let (eski_tur, eski_ac): (String, String) = c
        .query_one("SELECT tebligat_turu, evrak_aciklama FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = 5", &[&a])
        .map(|s| (s.get(0), s.get(1)))
        .unwrap();
    c.execute(
        "UPDATE tebligat_barkod SET tebligat_turu = 'Ödeme Emri', ptt_durumu = '', evrak_aciklama = evrak_aciklama || ' - Tebliğ Edildi'
          WHERE dosya_id = $1 AND birim_evrak_no = 5",
        &[&a],
    )
    .unwrap();
    let (_, u, r) = teb_calistir(Mod::Ptt, ptt_durumu("TESLİM EDİLDİ", "20261019"));
    r.unwrap();
    assert_eq!(u.say("PTT"), 1, "tarihi olmayan ödeme emri sorulur");
    assert_eq!(tebligat(c, 5).2, "TESLİM EDİLDİ");
    c.execute("UPDATE tebligat_barkod SET tebligat_turu = 'Satış İlanı', ptt_durumu = '' WHERE dosya_id = $1 AND birim_evrak_no = 5", &[&a]).unwrap();
    let (_, u, r) = teb_calistir(Mod::Ptt, ptt_durumu("TESLİM EDİLDİ", "20261019"));
    r.unwrap();
    assert_eq!(u.say("PTT"), 0, "mazbatası dönmüş satış ilanı sorulmaz");
    c.execute("UPDATE tebligat_barkod SET tebligat_turu = $2, evrak_aciklama = $3, ptt_durumu = 'DAĞITIMDA' WHERE dosya_id = $1 AND birim_evrak_no = 5", &[&a, &eski_tur, &eski_ac]).unwrap();

    // 10) Tebligat, yalnız UYAP: PTT'ye gidilmez, kayıtlı PTT durumu korunur
    let kapsam = Kapsam { birim_id: Some("9009".into()), birim_adi: String::new(), acilis_bas: None, acilis_son: None, dosyalar: None, hedefler: None, borclular: None, yeniden: false };
    let mut t = Arc::try_unwrap(Tarama::new(Mod::Tebligat, kapsam)).ok().unwrap();
    t.sayfa_boyu = 2;
    t.inner.lock().unwrap().ptt_sorgusu = false;
    let mut u = SahteUyap::yeni(yanit(false));
    u.ptt_yanit = Box::new(|_: &str| Err("sorulmamalıydı".into()));
    rt.block_on(akis(&t, &u)).unwrap();
    assert_eq!(u.say("PTT"), 0);
    assert!(t.inner.lock().unwrap().rapor.iter().any(|x| x.contains("yalnız UYAP")));
    assert_eq!(tebligat(c, 5).2, "DAĞITIMDA");

    // 11) Tek dosya (A), yalnız seçilen tebligat (5): künye + taraflar + yalnız o tebligatın PTT'si; sıra kilidi beklenmez
    let tek = |i: DosyaIstegi, ptt: SahtePtt| {
        let t = Tarama::new(Mod::Dosya, Kapsam::default());
        let t = Arc::try_unwrap(t).ok().unwrap();
        {
            let mut g = t.inner.lock().unwrap();
            g.dosya = Some(i.dosya);
            g.dosya_taraf = i.taraf;
            g.dosya_tebligat = i.tebligat;
            g.evrak_secimi = i.evraklar;
            g.ptt_sorgusu = i.ptt;
            g.uyap_id = i.uyap_id;
        }
        let mut t = t;
        t.ptt_tekrar_sn = 0;
        let mut u = SahteUyap::yeni(yanit(false));
        u.ptt_yanit = ptt;
        let r = rt.block_on(akis(&t, &u));
        (t, u, r)
    };
    let (t, u, r) = tek(DosyaIstegi { dosya: a, taraf: true, tebligat: true, evraklar: vec![5], ptt: true, kesinlesme: false, uyap_id: None, kunye: None }, ptt_durumu("TESLİM EDİLDİ", "20261110"));
    r.unwrap();
    assert_eq!((u.say(BORCLU_LISTESI), u.say(TARAF_BILGILERI), u.say("PTT")), (1, 1, 1), "{:?}", u.cagrilar.lock().unwrap());
    assert_eq!(u.say(DOSYA_AYRINTI), 1, "tek dosyada takip türü her seferinde yeniden okunur");
    let g = t.inner.lock().unwrap();
    assert_eq!((g.teb_yazilan, g.teb_degisen, g.taraf_cekilen), (1, 1, 1), "{:?}", g.uyarilar);
    assert!(g.rapor.iter().any(|x| x.starts_with("✔ Künye güncellendi: İzmir 9. İcra Dairesi 2026/10")), "{:?}", g.rapor);
    assert!(g.rapor.iter().any(|x| x.starts_with("Durum hesabı")), "{:?}", g.rapor);
    drop(g);
    assert_eq!(tebligat(c, 5).2, "TESLİM EDİLDİ");
    assert_eq!(tebligat(c, 6).2.starts_with("E-Tebligat"), true, "seçilmeyen tebligata dokunulmadı");
    // 11a) Dosya içinden gelen istekte dosya birim + numarayla bulunur (UYAP kimliği oturuma göre değişir)
    assert_eq!(rt.block_on(uyap_kimliginden_dosya("bu-oturumun-kimligi".into(), Some(("9009".into(), "2026/10".into(), 35)))), Ok(a));
    let kayitli: String = c.query_one("SELECT dosya_id FROM dosya WHERE id = $1", &[&a]).unwrap().get(0);
    assert_eq!(rt.block_on(uyap_kimliginden_dosya(kayitli, None)), Ok(a), "künye yoksa kayıtlı kimlik");
    assert!(rt.block_on(uyap_kimliginden_dosya("yok".into(), Some(("9009".into(), "2026/999".into(), 35)))).unwrap_err().contains("2026/999"));
    // 11b) Dosya içinden "Dosyaya tebligat ekle": UYAP kimliği bilinir; arama, künye ve takip türü atlanır
    let uid: String = c.query_one("SELECT dosya_id FROM dosya WHERE id = $1", &[&a]).unwrap().get(0);
    c.execute("UPDATE tebligat_barkod SET ptt_durumu = '' WHERE dosya_id = $1 AND birim_evrak_no = 5", &[&a]).unwrap();
    let (t, u, r) = tek(DosyaIstegi { dosya: a, taraf: false, tebligat: true, evraklar: vec![5], ptt: true, kesinlesme: false, uyap_id: Some(uid), kunye: None },
                        ptt_durumu("TESLİM EDİLDİ", "20261110"));
    r.unwrap();
    assert_eq!((u.say(ARAMA), u.say(DOSYA_AYRINTI), u.say(BORCLU_LISTESI), u.say("PTT")), (0, 0, 0, 1), "{:?}", u.cagrilar.lock().unwrap());
    assert_eq!(t.inner.lock().unwrap().teb_yazilan, 1);
    assert_eq!(tebligat(c, 5).2, "TESLİM EDİLDİ");

    // 12) Borçlu listesi boş dönen dosya (G, kapalı listede): borçlu taraf bilgilerinden yazılır
    let (gd, _) = dosya_kunyesi_kaydet(c, &kunye("2026/30", 0, "eski-g"), Some(YargiTuru::Icra.kod())).unwrap();
    kayit::dosya_taraf_kaydet(c, gd, &[json!({"adi": "Z BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum"})], &sessiz).unwrap();
    let (t, u, r) = tek(DosyaIstegi { dosya: gd, taraf: true, tebligat: false, evraklar: vec![], ptt: true, kesinlesme: false, uyap_id: None, kunye: None }, Box::new(|_: &str| Err("yok".into())));
    r.unwrap();
    assert!(u.cagrilar.lock().unwrap().iter().any(|(uc, g)| uc == ARAMA && g["dosyaDurumKod"] == 1), "açıkta bulunamayınca kapalıda aranır");
    let g = t.inner.lock().unwrap();
    assert!(g.borclusuz.is_empty() && g.taraf_cekilen == 1, "{:?}", g.uyarilar);
    drop(g);
    let borclu_adi: String = c
        .query_one("SELECT t.ad || ' ' || t.soyad FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE dt.dosya_id = $1 AND dt.rol = 'borclu'", &[&gd])
        .unwrap()
        .get(0);
    assert_eq!(borclu_adi.trim(), "VELİ CAN");
    assert_eq!(durum_kod(c, gd), 1, "künyeden kapalı durumu yazıldı");

    // 13) Taraf güncellemesinde borçlu alınamayan dosya (E) raporda adıyla görünür
    let (t, _, r) = calistir(Mod::Taraf, false, None);
    r.unwrap();
    let g = t.inner.lock().unwrap();
    assert_eq!(g.borclusuz, vec!["İzmir 9. İcra Dairesi 2026/21".to_string()]);
    assert!(g.rapor.iter().any(|x| x.contains("1 dosyada UYAP'tan borçlu alınamadı") && x.contains("2026/21")), "{:?}", g.rapor);
    drop(g);

    // 14) Seçili dosyalar (az): yalnız A — açık liste ALINMAZ, A numarasıyla aranır; yalnız A'nın evrak listesi istenir
    let secimle = |mod_: Mod, idler: Vec<i64>| {
        let kapsam = Kapsam { dosyalar: Some(idler), ..Default::default() };
        let mut t = Arc::try_unwrap(Tarama::new(mod_, kapsam)).ok().unwrap();
        t.sayfa_boyu = 2;
        t.mola_her = 1000;
        t.ptt_tekrar_sn = 0;
        t.inner.lock().unwrap().ptt_sorgusu = false;
        let u = SahteUyap::yeni(yanit(false));
        let r = rt.block_on(akis(&t, &u));
        (t, u, r)
    };
    let (t, u, r) = secimle(Mod::Tebligat, vec![a]);
    r.unwrap();
    let liste_istegi = |u: &SahteUyap| u.cagrilar.lock().unwrap().iter().filter(|(uc, g)| uc == ARAMA && g.get("dosyaSira").is_none()).count();
    assert_eq!(liste_istegi(&u), 0, "az seçimde açık dosya listesi istenmez");
    let evrak_istekleri: Vec<String> = u.cagrilar.lock().unwrap().iter().filter(|(uc, _)| uc == evrak_toplu::EVRAK_LISTESI)
        .map(|(_, g)| g["dosyaId"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(evrak_istekleri, vec!["u-a".to_string()]);
    let g = t.inner.lock().unwrap();
    assert!(g.rapor[0].contains("Seçili 1 dosya: 1 dosya UYAP'ta bulundu"), "{:?}", g.rapor);
    drop(g);

    // 15) Seçili dosyalar (çok): A, D, E, C — açık liste bir kez alınır, C hiçbir listede yok → bulunamadı; kapanan turu yok
    let (t, u, r) = secimle(Mod::Liste, vec![a, d, cc, c.query_one("SELECT id FROM dosya WHERE dosya_no = '2026/21'", &[]).unwrap().get(0)]);
    r.unwrap();
    assert!(liste_istegi(&u) >= 1, "çok seçimde açık liste alınır");
    let g = t.inner.lock().unwrap();
    assert_eq!((g.alinan, g.bulunamadi), (3, 1), "{:?}", g.uyarilar);
    assert!(g.rapor[0].contains("Seçili 4 dosya: 3 dosya UYAP'ta bulundu, 1 dosya açık ve kapalı listede bulunamadı"), "{:?}", g.rapor);
    assert!(!g.rapor.iter().any(|x| x.contains("Veritabanındaki açık icra")), "seçimde genel sayım karşılaştırması yapılmaz");
    drop(g);
    assert_eq!(durum_kod(c, cc), 0, "bulunamayana dokunulmaz");

    // 16) Takip türü listesi: tür → yol → şekil; İlamlı'nın yol listesi alınamaz (uyarı), kalanı yazılır
    let mut t = Arc::try_unwrap(Tarama::new(Mod::TakipListesi, Kapsam::default())).ok().unwrap();
    t.mola_her = 1000;
    let u = SahteUyap::yeni(yanit(false));
    rt.block_on(akis(&t, &u)).unwrap();
    assert_eq!(u.cagrilar.lock().unwrap().iter().find(|(uc, _)| uc == TAKIP_SEKLI_LISTESI).unwrap().1, json!({"takipTuru": 1, "takipYolu": 0}));
    let g = t.inner.lock().unwrap();
    assert_eq!(g.takip_listesi, (2, 1, 2));
    assert!(g.uyarilar.iter().any(|x| x.contains("İlamlı Takip: takip yolu listesi alınamadı")), "{:?}", g.uyarilar);
    assert_eq!(g.rapor, vec!["✔ UYAP takip türü listesi alındı: 2 tür, 1 yol, 2 şekil.".to_string()]);
    drop(g);
    let liste: Vec<(i16, String, String, String, String, String)> = c
        .query("SELECT kademe, takibin_turu, takibin_yolu, takibin_sekli, ad, ornek_no FROM takip_turu_listesi ORDER BY kademe, takibin_turu, takibin_sekli", &[])
        .unwrap()
        .iter()
        .map(|s| (s.get(0), s.get(1), s.get(2), s.get(3), s.get(4), s.get(5)))
        .collect();
    assert_eq!(liste.len(), 5, "{liste:?}");
    assert_eq!(liste[3], (3, "1".into(), "0".into(), "0".into(), "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49".into(), "7".into()));
    assert_eq!(liste[4].5, "10");

    c.batch_execute("DELETE FROM dosya; DELETE FROM taraf; DELETE FROM vekil; DELETE FROM birim; DELETE FROM takip_turu_listesi").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sayfa_ve_toplam() {
        assert!(sayfa_gecersiz(&None).is_some());
        assert!(sayfa_gecersiz(&Some(json!({"errorCode": "X", "error": "olmaz"}))).unwrap().contains("olmaz"));
        assert!(sayfa_gecersiz(&Some(json!({"error": "Eş zamanlı olarak birden fazla sorgulama yapamazsınız!"}))).unwrap().contains("eş zamanlı"));
        assert!(sayfa_gecersiz(&Some(json!([[{"dosyaId": "1"}], 1]))).is_none());
        assert!(sayfa_gecersiz(&Some(json!([]))).is_none(), "boş liste geçerli son sayfadır");
        assert_eq!(uyap_toplami(&json!([[{"a": 1}], 1234])), Some(1234));
        assert_eq!(uyap_toplami(&json!([[], "56"])), Some(56));
        assert_eq!(uyap_toplami(&json!([{"a": 1}, {"b": 2}])), None);
        assert_eq!(uyap_toplami(&json!([[], true])), None);
    }

    /// 21.11.2026: oturum düşünce UYAP her dosyada giriş sayfası (JSON değil) verir; tarama saatlerce "alınamadı"
    /// sayıp "Tamamlandı" dememeli, art arda sınırda durmalı.
    #[test]
    fn uyap_art_arda_veri_vermezse_durur() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let t = Arc::try_unwrap(Tarama::new(Mod::Takip, Kapsam::default())).ok().unwrap();
        let u = SahteUyap::yeni(Box::new(|_, _| Ok(None)));
        let dosyalar: Vec<YazilanDosya> = (1..=40).map(|i| (i, format!("u-{i}"), format!("2026/{i}"))).collect();
        let e = rt.block_on(takip_ayrintilari(&t, &u, dosyalar)).unwrap_err();
        assert!(e.contains(&format!("art arda {ARDISIK_RET_SINIRI} dosyada veri vermedi")) && e.contains("oturumu"), "{e}");
        assert_eq!(u.say(DOSYA_AYRINTI), ARDISIK_RET_SINIRI, "sınırda durmalı, 40 dosyanın hepsini sormamalı");
        assert_eq!(t.inner.lock().unwrap().takip_okunamayan, ARDISIK_RET_SINIRI);
        // Arada tek tük hata (sınırın altında) taramayı durdurmaz.
        let mut n = 0;
        for _ in 0..ARDISIK_RET_SINIRI - 1 {
            assert!(uyap_reddi("x", &mut n).is_ok());
        }
        assert!(uyap_reddi("x", &mut n).is_err());
        assert_eq!(okunamayan_eki(0), "");
        assert!(okunamayan_eki(4886).contains("4886 dosyanın takip türü UYAP'tan alınamadı"));
    }

    /// UYAP kimliği bayatlayınca (PRTL_GNL_10001-4) liste sessizce yeniden alınır, istek yeni kimlikle tekrarlanır;
    /// süre dolunca istekten önce yenilenir; dakikada birden sık yenilenmez; açık listede olmayan dosya numarasıyla aranır.
    #[test]
    fn bayat_kimlik_yenilenir() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut t = Arc::try_unwrap(Tarama::new(Mod::Hepsi, Kapsam::default())).ok().unwrap();
        // Liste her istendiğinde kimlikler değişir (UYAP gibi): "k<n>-<sıra>". 7 numaralı dosya kapalı (açık listede yok).
        let tur = std::sync::atomic::AtomicUsize::new(0);
        let u = SahteUyap::yeni(Box::new(move |uc, g| {
            let id = g["dosyaId"].as_str().unwrap_or("").to_string();
            Ok(Some(match uc {
                ARAMA if g["dosyaDurumKod"] == 0 && g.get("dosyaSira").is_none() => {
                    let n = tur.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    json!([[{"dosyaId": format!("k{n}-1"), "dosyaNo": "2026/1", "birimId": "B1", "dosyaTurKod": 35},
                            {"dosyaId": format!("k{n}-2"), "dosyaNo": "2026/2 - ( 2022/9 )", "birimId": "B1", "dosyaTurKod": 35}], 2])
                }
                ARAMA if g["dosyaDurumKod"] == 1 => json!([[{"dosyaId": "kapali-7", "dosyaNo": "2025/7", "birimId": "B2", "dosyaTurKod": 35}], 1]),
                ARAMA => json!([[], 0]),
                // Eski kimlikler bayat; listeden gelenler geçerli.
                _ if id.starts_with("eski") => json!({"errorCode": BAYAT_KIMLIK_KODU, "error": "Doğrulama hatası.Lütfen tekrar deneyiniz. "}),
                _ => json!({"tamam": id}),
            }))
        }));
        {
            let mut g = t.inner.lock().unwrap();
            g.kimlik_anahtari.insert(1, ("B1".into(), 2026, 1, 35));
            g.kimlik_anahtari.insert(2, ("B1".into(), 2026, 2, 35));
            g.kimlik_anahtari.insert(7, ("B2".into(), 2025, 7, 35));
            g.kimlik_zamani = Some(now_secs() - 120.0);
        }
        // 1) Bayat kimlik: liste bir kez alınır, istek yeni kimlikle tekrarlanır.
        let mut id = "eski-1".to_string();
        let v = rt.block_on(dosya_istegi_at(&t, &u, DOSYA_AYRINTI, 1, &mut id, json!({}))).unwrap();
        assert_eq!((v, id.as_str()), (Some(json!({"tamam": "k1-1"})), "k1-1"));
        assert_eq!(u.say(ARAMA), 1);
        assert_eq!(t.inner.lock().unwrap().kimlik_tazeleme, 1);
        // 2) Aynı dakika içinde ikinci bayat yanıt: yenileme yeniden yapılmaz, ama listede gelmiş yeni kimlik kullanılır.
        let mut id2 = "eski-2".to_string();
        let v = rt.block_on(dosya_istegi_at(&t, &u, DOSYA_AYRINTI, 2, &mut id2, json!({}))).unwrap();
        assert_eq!((v, id2.as_str()), (Some(json!({"tamam": "k1-2"})), "k1-2"), "yenilenmiş dosya numarası da eşleşmeli");
        assert_eq!(u.say(ARAMA), 1);
        // 3) Açık listede olmayan dosya: numarasıyla (kapalı listede) aranır.
        let mut id7 = "eski-7".to_string();
        let v = rt.block_on(dosya_istegi_at(&t, &u, DOSYA_AYRINTI, 7, &mut id7, json!({}))).unwrap();
        assert_eq!((v, id7.as_str()), (Some(json!({"tamam": "kapali-7"})), "kapali-7"));
        // 4) Yenileme de işe yaramazsa (kimlik aynı kalır) bayat yanıt olduğu gibi döner: art arda ret sınırı karar verir.
        let mut ayni = "k1-1".to_string();
        t.inner.lock().unwrap().taze_kimlik.insert(1, "k1-1".into());
        let u2 = SahteUyap::yeni(Box::new(|_, _| Ok(Some(json!({"errorCode": BAYAT_KIMLIK_KODU, "error": "Doğrulama hatası."})))));
        let v = rt.block_on(dosya_istegi_at(&t, &u2, DOSYA_AYRINTI, 1, &mut ayni, json!({}))).unwrap();
        assert!(sayfa_gecersiz(&v).is_some_and(|n| kimlik_bayat_mi(&n)));
        assert_eq!(u2.say(DOSYA_AYRINTI), 1, "aynı kimlikle tekrar denenmez");
        // 5) Süre dolunca istekten ÖNCE yenilenir; dolmadıysa liste istenmez.
        t.kimlik_omru_sn = 3600.0;
        t.inner.lock().unwrap().kimlik_zamani = Some(now_secs());
        let once = u.say(ARAMA);
        assert_eq!(rt.block_on(guncel_kimlik(&t, &u, 2, "x")).unwrap(), "k1-2");
        assert_eq!(u.say(ARAMA), once);
        t.kimlik_omru_sn = 0.0;
        assert_eq!(rt.block_on(guncel_kimlik(&t, &u, 2, "x")).unwrap(), "k2-2");
        assert_eq!(u.say(ARAMA), once + 1);
        // 6) Kimliği listeden gelmeyen iş (tek dosya): yenileme yok.
        t.inner.lock().unwrap().kimlik_zamani = None;
        assert_eq!(rt.block_on(kimlik_yenile(&t, &u, 1, "eski")).unwrap(), None);
    }

    #[test]
    fn sayim_ve_fren() {
        assert!(sayim_satiri("İcra", 5, Some(5), true).0.starts_with('✔'));
        let (m, eksik) = sayim_satiri("İcra", 4, Some(5), true);
        assert!(eksik && m.contains("1 fark"));
        assert!(sayim_satiri("İcra", 4, None, false).1);
        assert!(!sayim_satiri("İcra", 4, None, true).1);
        assert!(!fren_asildi(50, 0) && fren_asildi(51, 0));
        assert!(!fren_asildi(300, 6000) && fren_asildi(6001, 6000));
    }

    #[test]
    fn liste_govdesi_ve_kapsam() {
        let k = Kapsam { birim_id: Some("9009".into()), birim_adi: "İzmir 9. İcra Dairesi".into(),
                         acilis_bas: NaiveDate::from_ymd_opt(2026, 11, 1), acilis_son: NaiveDate::from_ymd_opt(2026, 11, 30), dosyalar: None, hedefler: None, borclular: None, yeniden: false };
        let g = liste_govdesi(3, 500, &k);
        assert_eq!((g["pageNumber"].clone(), g["birimId"].clone(), g["birimTuru3"].clone()), (json!(3), json!("9009"), json!("2")));
        assert_eq!((g["dosyaAcilisTarihiStart"].clone(), g["dosyaAcilisTarihiEnd"].clone()), (json!("01.11.2026"), json!("30.11.2026")));
        assert_eq!(k.metin(), "İzmir 9. İcra Dairesi · açılış 01.11.2026–30.11.2026");
        let bos = Kapsam::default();
        let g = liste_govdesi(1, 500, &bos);
        assert!(g.get("birimId").is_none() && g.get("dosyaAcilisTarihiStart").is_none());
        assert_eq!(bos.metin(), "Bütün açık dosyalar");
    }

    #[test]
    fn dosya_istegi_govdesi() {
        assert_eq!(
            dosya_istegi(&json!({"dosya": 42})),
            Ok(DosyaIstegi { dosya: 42, taraf: true, tebligat: true, evraklar: vec![], ptt: true, kesinlesme: false, uyap_id: None, kunye: None })
        );
        let i = dosya_istegi(&json!({"dosya": 42, "taraf": false, "tebligat": false, "evraklar": [5, 7], "ptt": false})).unwrap();
        assert_eq!((i.taraf, i.tebligat, i.evraklar, i.ptt), (false, true, vec![5, 7], false), "seçim varsa tebligat istenmiş sayılır");
        assert!(dosya_istegi(&json!({})).is_err());
        assert!(dosya_istegi(&json!({"dosya": -1})).is_err());
        assert!(dosya_istegi(&json!({"dosya": 1, "evraklar": ["a"]})).is_err());
        assert!(dosya_istegi(&json!({"dosya": 1, "evraklar": [0]})).is_err());
        // Dosya içinden tebligat ekleme: UYAP kimliğiyle, taraf istenmez, evrak seçimi şart
        let i = dosya_istegi(&json!({"uyap_dosya_id": "abc==", "evraklar": [12]})).unwrap();
        assert_eq!((i.dosya, i.taraf, i.tebligat, i.evraklar, i.uyap_id.as_deref()), (0, false, true, vec![12], Some("abc==")));
        assert!(dosya_istegi(&json!({"uyap_dosya_id": "abc=="})).unwrap_err().contains("seçilmedi"));
        let i = dosya_istegi(&json!({"uyap_dosya_id": "abc==", "evraklar": [3], "birim_id": "1001", "dosya_no": "2026/5", "dosya_tur_kod": 35})).unwrap();
        assert_eq!(i.kunye, Some(("1001".into(), "2026/5".into(), 35)));
    }

    #[test]
    fn baslatma_govdesi() {
        let (m, k, ptt) = baslatma_istegi(&json!({})).unwrap();
        assert_eq!((m, k.birim_id.clone(), k.tarihli(), ptt), (Mod::Liste, None, false, true));
        assert_eq!(baslatma_istegi(&json!({"mod": "tebligat", "ptt": false})).map(|(m, _, p)| (m, p)), Ok((Mod::Tebligat, false)));
        assert_eq!(baslatma_istegi(&json!({"mod": "ptt"})).map(|(m, _, p)| (m, p)), Ok((Mod::Ptt, true)));
        let (m, k, _) = baslatma_istegi(&json!({"mod": "taraf", "birim_id": "2002", "birim_adi": "Ankara 3. İcra Dairesi",
                                              "acilis_bas": "2026-11-01", "acilis_son": "30.11.2026"})).unwrap();
        assert_eq!((m, k.birim_id.as_deref(), k.acilis_bas, k.acilis_son), (Mod::Taraf, Some("2002"),
                   NaiveDate::from_ymd_opt(2026, 11, 1), NaiveDate::from_ymd_opt(2026, 11, 30)));
        assert!(baslatma_istegi(&json!({"mod": "olmaz"})).is_err());
        assert!(baslatma_istegi(&json!({"birim_id": "12a"})).is_err());
        assert!(baslatma_istegi(&json!({"acilis_bas": "2026-12-01", "acilis_son": "2026-11-01"})).unwrap_err().contains("ters"));
        assert!(baslatma_istegi(&json!({"acilis_bas": "olmaz"})).is_err());
        // Tüm veriler: hedefler yalnız bu türde okunur
        let (m, k, _) = baslatma_istegi(&json!({"mod": "hepsi", "hedefler": [3, 5]})).unwrap();
        assert_eq!((m, k.hedefler.clone(), k.dosyalar.clone()), (Mod::Hepsi, Some(vec![3, 5]), None));
        assert_eq!(baslatma_istegi(&json!({"mod": "taraf", "hedefler": [3]})).unwrap().1.hedefler, None);
        assert!(baslatma_istegi(&json!({"mod": "hepsi", "hedefler": ["x"]})).is_err());
    }
}
