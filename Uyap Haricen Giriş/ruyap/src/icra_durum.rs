//! Borçlu başına kesinleşme, tebliğ durumu ve takip aşaması hesabı (Aşama 4).
//!
//! Yalnız büro veritabanındaki kayıtlardan hesaplar; UYAP'a ya da PTT'ye istek atmaz. Kurallar
//! örnek koddan (UyapMasa `kesinlesme_core` / `dosya_core`) taşınmıştır:
//!   • Kesinleşme: tebliğ günü sayılmaz; kesinleşme süresi (takip türüne göre `kesinlesme_kurali`;
//!     itiraz ve ödeme sürelerinden uzun olanı) kadar gün sonra son gün gelir; son gün hafta sonu / tatile denk gelirse ilk iş gününe
//!     kayar; kesinleşme son günden sonraki ilk iş günüdür. Aradaki tatiller süreyi durdurmaz.
//!   • Kesinleşti için bir ödeme emrinde üçü birden gerekir: tebliğ gerçekleşmiş, mazbatası
//!     dosyaya dönmüş ve süre dolmuş. Ödeme emri gönderilmiş borçlu o güne dek Kesinleşme
//!     Bekliyor'dur. Elle itiraz (yalnız itirazın takibi durdurduğu türlerde; ilamlı ve kambiyo haciz
//!     yolunda durdurmaz) ve dosyanın "Durdurulmuş : X" durumu (her türde) bunlardan önce gelir.
//!   • E-tebligatta tebliğ tarihi evrak tarihidir; süre 5 gün sonra başlar.
//!   • Tebliğ durumu borçlu başına ve ödeme emrine göredir.
//!   • Takip aşaması bunlardan türetilir: Takip Talebi → Ödeme Emri → Tebligat Oluşturuluyor →
//!     PTT'ye Veriliyor → Tebliğe Çıktı → Tebliğ / İade / İtiraz / Durduruldu → Kesinleşti.
//! Her değişiklik durum geçmişine (`durum_olayi`) eski → yeni olarak yazılır; değişiklik yoksa
//! yazılmaz. İş Listesi'ne otomatik iş açma (Aşama 6) bu olaylardan doğacak.

use crate::icra_kayit as kayit;
use crate::icra_tebligat::{self as teb, tr_kucuk, E_TEBLIG_OKUMA_SURESI_GUN};
use crate::is_otomatik as otomatik;
use crate::icra_veri::{
    self as veri, durum_olayi_yaz, KesinlesmeDurumu as K, OlayKaynak, OlayTur, TakipAsamasi as A, TebligatDurumu as T, YeniOlay,
};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use postgres::GenericClient;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

fn hata(e: postgres::Error) -> String {
    format!("veritabanı: {e}")
}

// ── Süre hesabı ─────────────────────────────────────────────────────────────────────────────────

pub type Tatiller = BTreeSet<NaiveDate>;

/// Hafta sonu ve resmî tatil dışındaki gün.
pub fn is_gunu_mu(tarih: NaiveDate, tatiller: &Tatiller) -> bool {
    !matches!(tarih.weekday(), Weekday::Sat | Weekday::Sun) && !tatiller.contains(&tarih)
}

/// Tarih iş günüyse kendisi, değilse sonraki ilk iş günü.
pub fn sonraki_is_gunu(mut tarih: NaiveDate, tatiller: &Tatiller) -> NaiveDate {
    while !is_gunu_mu(tarih, tatiller) {
        tarih += Duration::days(1);
    }
    tarih
}

/// Tarihten sonraki (kendisi hariç) ilk iş günü.
pub fn ilk_is_gunu_sonrasi(tarih: NaiveDate, tatiller: &Tatiller) -> NaiveDate {
    sonraki_is_gunu(tarih + Duration::days(1), tatiller)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KesinlesmeTarihleri {
    /// İtiraz süresinin son günü (iş günü olacak şekilde kaydırılmış).
    pub itiraz_son_gunu: NaiveDate,
    /// Takibin kesinleştirilebileceği gün: son günden sonraki ilk iş günü.
    pub kesinlesebilir_tarihi: NaiveDate,
}

pub fn kesinlesme_hesapla(teblig_tarihi: NaiveDate, sure_gun: i64, tatiller: &Tatiller) -> KesinlesmeTarihleri {
    let itiraz_son_gunu = sonraki_is_gunu(teblig_tarihi + Duration::days(sure_gun), tatiller);
    KesinlesmeTarihleri { itiraz_son_gunu, kesinlesebilir_tarihi: ilk_is_gunu_sonrasi(itiraz_son_gunu, tatiller) }
}

// ── Ödeme emri tebligatı ────────────────────────────────────────────────────────────────────────

/// Elle bağlanan mazbatada tebliğ tarihi girilmiş fiziki tebligatın hesaptaki PTT durumu ("teslim edildi" içerir).
pub const MAZBATADAN_TESLIM: &str = "TESLİM EDİLDİ (mazbatadaki tarih)";
/// Elle bağlanan mazbata "tebliğ edilemedi" döndü (kullanıcı işaretledi): PTT durumunun yerine geçer, "iade" içerir.
pub const MAZBATADAN_IADE: &str = "İADE (mazbata tebliğ edilemedi, elle)";

/// Borçluya bağlı bir ödeme emri tebligatı (`tebligat_barkod` satırının hesapta kullanılan alanları).
#[derive(Clone, Debug, Default)]
pub struct OdemeEmri {
    pub birim_evrak_no: i32,
    pub evrak_aciklama: String,
    pub evrak_tarihi: String,
    pub evrak_gunu: Option<NaiveDate>,
    pub barkod: String,
    pub elektronik_tebligat: bool,
    pub ptt_durumu: String,
    pub son_islem_tarihi: String,
    /// `son_islem_zamani` alanının günü.
    pub son_islem_gunu: Option<NaiveDate>,
    pub teslim_sekli: String,
    pub tebligat_mazbatasi_var: bool,
    pub tebligat_mazbatasi_aciklama: String,
}

/// UYAP mazbata dönünce evrak açıklamasının sonuna eklediği sonuç.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MazbataSonucu {
    Yok,
    Edildi,
    Edilemedi,
}

pub fn mazbata_sonucu(aciklama: &str) -> MazbataSonucu {
    let son = tr_kucuk(aciklama.rsplit(']').next().unwrap_or(""));
    if son.contains("teblig edilemedi") {
        MazbataSonucu::Edilemedi
    } else if son.contains("teblig edildi") {
        MazbataSonucu::Edildi
    } else {
        MazbataSonucu::Yok
    }
}

impl OdemeEmri {
    /// Son işlem (tebliğ / iade) günü: tarih alanından, yoksa ham metinden.
    pub fn son_islem(&self) -> Option<NaiveDate> {
        self.son_islem_gunu.or_else(|| teb::teblig_tarihini_coz(&self.son_islem_tarihi))
    }

    pub fn evrak(&self) -> Option<NaiveDate> {
        self.evrak_gunu.or_else(|| teb::teblig_tarihini_coz(&self.evrak_tarihi))
    }

    /// Sürenin sayıldığı tebliğ günü: fizikide tebliğ tarihi, e-tebligatta tebliğ tarihi + 5 gün.
    pub fn sure_baslangici(&self) -> Option<NaiveDate> {
        let t = self.son_islem()?;
        Some(if self.elektronik_tebligat { t + Duration::days(E_TEBLIG_OKUMA_SURESI_GUN) } else { t })
    }

    /// Gerçekten teslim edilmiş / okunmuş sayılmış mı? Dağıtımda ya da iade gibi durumlarda süre
    /// başlamaz. Fizikide PTT durumunda "teslim edildi" aranır.
    pub fn gerceklesti_mi(&self, bugun: NaiveDate) -> bool {
        if self.elektronik_tebligat {
            return self.sure_baslangici().is_some_and(|t| t <= bugun);
        }
        tr_kucuk(&self.ptt_durumu).contains("teslim edildi")
    }

    /// Bu tebligatın kendi mazbatası dosyaya dönmüş mü? E-tebligatta ayrı dönüş beklenmez.
    /// Fizikide yalnız bu barkoda ait kanıt sayılır (başka barkodun mazbatası sayılmaz).
    pub fn mazbatasi_dondu_mu(&self) -> bool {
        self.elektronik_tebligat
            || mazbata_sonucu(&self.evrak_aciklama) == MazbataSonucu::Edildi
            || teb::kendi_mazbatasi_dondu_mu(&self.barkod, self.tebligat_mazbatasi_var, &self.tebligat_mazbatasi_aciklama)
    }

    fn teslim(&self) -> String {
        if self.elektronik_tebligat { "E-tebligat".to_string() } else { self.teslim_sekli.clone() }
    }
}

// ── Borçlu başına hesaplar ──────────────────────────────────────────────────────────────────────

/// Dosya durum metninden kesinleşme kodu: "Açık (Durdurulmuş : X)" → X'in kodu; bilinmeyen X →
/// "durd_tanimsiz"; durdurulmamışsa `None`.
pub fn durdurma_kesinlesme_kodu(durum: &str) -> Option<&'static str> {
    let d = tr_kucuk(durum).split_whitespace().collect::<Vec<_>>().join(" ");
    let (_, sonrasi) = d.split_once("durdurulmus")?;
    let neden = sonrasi.trim_start().strip_prefix(':')?.trim().trim_end_matches(')').trim();
    Some(match neden {
        "takibe itiraz" => K::Itiraz.kod(),
        "icra mahkemesine itiraz" => K::DurdurulmusIcraMahItiraz.kod(),
        "genel mahkemeye dava acilmasi" => K::DurdurulmusGenelMahDava.kod(),
        "tehir-i icra karari" => K::DurdurulmusTehiriIcra.kod(),
        "ihtiyati tedbir" => K::DurdurulmusIhtiyatiTedbir.kod(),
        "iflas" => K::DurdurulmusIflas.kod(),
        "borclunun olumu" => K::DurdurulmusBorclununOlumu.kod(),
        "diger" => K::DurdurulmusDiger.kod(),
        _ => K::DurdurulmusTanimsiz.kod(),
    })
}

/// 103 davetiyesi ve kıymet takdiri raporunda kesinleşme süresi (her takip türünde aynı).
pub const SABIT_KESINLESME_GUN: i16 = 7;

/// Tebligat türüne göre kesinleşme (kullanıcı kuralı 19.11.2026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TebligatKesinlesmesi {
    /// Ödeme emri (ilamlı icra emri de bu türe düşer): takip türünün süresiyle borçlu hesabı.
    Borclu,
    /// 103 davetiyesi, kıymet takdiri raporu: takip türünden bağımsız sabit süre.
    SabitGun(i16),
    /// Maaş / banka haczi, haciz ihbarnamesi, bilirkişi raporu vb.: kesinleşme yok.
    Yok,
}

/// UYAP dosya türü kodu: Talimat Dosyası (İcra Dosyası 35, MTS 294).
pub const TALIMAT_TUR_KODU: i32 = 1;

/// Talimat dosyası mı. Talimat dosyasında kesinleşme olmaz (kesinleşme esas dosyada izlenir); kesinleşme
/// hesaplanmaz, süzgeçte ayrı görünür, "kesinleşme kontrolü" işi açılmaz, takip türü sorulmaz (kullanıcı kuralı
/// 19.11.2026; Python'da da "Dosya Türü" Esas / Talimat ayrı tutuluyordu).
pub fn talimat_mi(tur_kod: i32, tur: &str) -> bool {
    tur_kod == TALIMAT_TUR_KODU || tr_kucuk(tur).contains("talimat")
}

/// Talimat dosyasındaki tebligatın ve borçlunun kesinleşme yerine gösterilen açıklama.
pub const TALIMAT_KESINLESME_YOK: &str = "Talimat dosyasında kesinleşme yok";

/// Borçlunun kesinleşmesini başlatan tebligat türleri: ödeme / icra emri ve kira süresi bitiminde tahliye emri
/// (Örnek 14; 20.11.2026).
pub const KESINLESME_TEBLIGATLARI: [&str; 2] = ["Ödeme Emri", "Tahliye Emri"];

pub fn tebligat_kesinlesmesi(tebligat_turu: &str) -> TebligatKesinlesmesi {
    match tebligat_turu {
        "Ödeme Emri" | "Tahliye Emri" => TebligatKesinlesmesi::Borclu,
        "103 Davetiyesi" | "Kıymet Takdiri Raporu" => TebligatKesinlesmesi::SabitGun(SABIT_KESINLESME_GUN),
        _ => TebligatKesinlesmesi::Yok,
    }
}

/// Borçlunun kesinleşme durumu ve günü. Borçluya bağlı ödeme emri yoksa ya da takip türü için
/// süre tanımlı değilse boş döner. `itiraz_edildi_mi`, çağıran tarafta takip türünün kuralına göre
/// süzülmüş olmalı: itirazın takibi durdurmadığı türde `false` geçilir (bkz. [`itiraz_etkili_mi`]).
pub fn borclu_kesinlesme_hesapla(
    itiraz_edildi_mi: bool,
    odeme_emirleri: &[OdemeEmri],
    tatiller: &Tatiller,
    bugun: NaiveDate,
    durdurma: Option<&str>,
    sure_gun: Option<i16>,
) -> (String, Option<NaiveDate>) {
    if itiraz_edildi_mi {
        return (K::Itiraz.kod().into(), None);
    }
    if let Some(d) = durdurma {
        return (d.into(), None);
    }
    let Some(sure) = sure_gun.filter(|_| !odeme_emirleri.is_empty()) else { return (String::new(), None) };
    let (mut mazbatali, mut teblig): (Option<NaiveDate>, Option<NaiveDate>) = (None, None);
    for k in odeme_emirleri {
        if !k.gerceklesti_mi(bugun) || mazbata_sonucu(&k.evrak_aciklama) == MazbataSonucu::Edilemedi {
            continue;
        }
        let Some(t) = k.sure_baslangici() else { continue };
        teblig = teblig.max(Some(t));
        if k.mazbatasi_dondu_mu() {
            mazbatali = mazbatali.max(Some(t));
        }
    }
    if let Some(t) = mazbatali {
        let gun = kesinlesme_hesapla(t, i64::from(sure), tatiller).kesinlesebilir_tarihi;
        if bugun >= gun {
            return (K::Kesinlesti.kod().into(), Some(gun));
        }
    }
    (K::KesinlesmeBekliyor.kod().into(), teblig.map(|t| kesinlesme_hesapla(t, i64::from(sure), tatiller).kesinlesebilir_tarihi))
}

/// Elle girilen kesinleşme gününün durumu: gün gelmişse Kesinleşti, gelmemişse Kesinleşme Bekliyor.
pub fn elle_kesinlesme(gun: NaiveDate, bugun: NaiveDate) -> &'static str {
    if bugun >= gun { K::Kesinlesti.kod() } else { K::KesinlesmeBekliyor.kod() }
}

/// Elle işaretlenen itiraz kesinleşmeyi durdurur mu: kural "itiraz durdurmaz" diyorsa hayır (ilamlıda icra
/// mahkemesine başvuru İİK 33, kambiyo haciz yolunda İİK 169). Kural yoksa eskisi gibi durdurur.
pub fn itiraz_etkili_mi(itiraz_edildi_mi: bool, kural: Option<&veri::KesinlesmeKurali>) -> bool {
    itiraz_edildi_mi && kural.map_or(true, |k| k.itiraz_durdurur)
}

/// Borçlunun ödeme emri tebligatlarından (tebliğ durumu, günü, teslim şekli / iade nedeni):
///   • biri tebliğ edilmişse Tebliğ Edildi, en yeni tebliğ günü;
///   • yoksa mazbatası "Tebliğ Edildi" dönmüş olan varsa Tebliğ Edildi (gün yok);
///   • yoksa evrak tarihi en yeni olan ve bilgi taşıyan tebligata göre: PTT iade ya da mazbata
///     "Tebliğ Edilemedi" → İade; başka bir PTT hareketi ya da günü gelmemiş e-tebligat → Dağıtımda;
///   • hiç bilgi yoksa boş.
pub fn borclu_tebligat_hesapla(odeme_emirleri: &[OdemeEmri], bugun: NaiveDate) -> (String, Option<NaiveDate>, String) {
    let mut en_yeni: Option<(NaiveDate, &OdemeEmri)> = None;
    for k in odeme_emirleri {
        if !k.gerceklesti_mi(bugun) {
            continue;
        }
        if let Some(t) = k.son_islem() {
            if en_yeni.map_or(true, |(e, _)| t > e) {
                en_yeni = Some((t, k));
            }
        }
    }
    if let Some((t, k)) = en_yeni {
        return (T::TebligEdildi.kod().into(), Some(t), k.teslim());
    }
    if odeme_emirleri.iter().any(|k| mazbata_sonucu(&k.evrak_aciklama) == MazbataSonucu::Edildi) {
        return (T::TebligEdildi.kod().into(), None, String::new());
    }
    let mut sirali: Vec<&OdemeEmri> = odeme_emirleri.iter().collect();
    sirali.sort_by_key(|k| std::cmp::Reverse(k.evrak()));
    for k in sirali {
        let ptt = tr_kucuk(&k.ptt_durumu);
        if ptt.contains("iade") || mazbata_sonucu(&k.evrak_aciklama) == MazbataSonucu::Edilemedi {
            return (T::Iade.kod().into(), k.son_islem(), k.teslim());
        }
        if k.elektronik_tebligat || (!ptt.is_empty() && !ptt.contains("kayit yok")) {
            return (T::Dagitimda.kod().into(), None, String::new());
        }
    }
    (String::new(), None, String::new())
}

/// Borçlunun bu turda hesaplanmış durumu (takip aşamasının girdisi).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BorcluDurumu {
    pub kesinlesme_durumu: String,
    pub kesinlesme_tarihi: Option<NaiveDate>,
    pub tebligat_durumu: String,
    pub tebligat_durum_tarihi: Option<NaiveDate>,
    pub tebligat_sekli: String,
    pub itiraz_tarihi: Option<NaiveDate>,
}

/// Borçlunun takip aşaması ve günü. Öncelik: Kesinleşti / İtiraz / Durdurulmuş (kesinleşme
/// durumundan) → Tebliğ / İade (tebliğ durumundan) → en yeni ödeme emri tebligatının hâli:
/// e-tebligat ya da kabul ötesi hareket → Tebliğe Çıktı; barkod var, PTT'de kayıt yok ya da yalnız
/// kabul → PTT'ye Veriliyor; barkod yok → Tebligat Oluşturuluyor → tebligat yok ama dosyada ödeme
/// emri evrakı var (`odeme_emri_evraki`: evrak günü) → Ödeme Emri → o da yok → Takip Talebi.
pub fn takip_asamasi_hesapla(
    b: &BorcluDurumu,
    odeme_emirleri: &[OdemeEmri],
    odeme_emri_evraki: Option<Option<NaiveDate>>,
) -> (A, Option<NaiveDate>) {
    let kes = b.kesinlesme_durumu.as_str();
    if kes == K::Kesinlesti.kod() {
        return (A::Kesinlesti, b.kesinlesme_tarihi);
    }
    if kes == K::Itiraz.kod() {
        return (A::Itiraz, b.itiraz_tarihi);
    }
    if kes.starts_with("durd_") || kes == K::TehiriIcra.kod() {
        return (A::Durduruldu, None);
    }
    if b.tebligat_durumu == T::TebligEdildi.kod() {
        return (A::Teblig, b.tebligat_durum_tarihi);
    }
    if b.tebligat_durumu == T::Iade.kod() {
        return (A::Iade, b.tebligat_durum_tarihi);
    }
    let Some(k) = odeme_emirleri.iter().max_by_key(|k| (k.evrak(), k.birim_evrak_no)) else {
        return match odeme_emri_evraki {
            Some(gun) => (A::OdemeEmri, gun),
            None => (A::TakipTalebi, None),
        };
    };
    if k.elektronik_tebligat {
        return (A::TebligeCikti, k.evrak());
    }
    if k.barkod.trim().is_empty() {
        return (A::TebligatOlusturuluyor, k.evrak());
    }
    let ptt = tr_kucuk(&k.ptt_durumu);
    if ptt.is_empty() {
        return (A::PttVeriliyor, k.evrak());
    }
    if ptt.contains("kayit yok") || ptt.contains("kabul") {
        return (A::PttVeriliyor, k.son_islem());
    }
    (A::TebligeCikti, k.son_islem())
}

// ── Görünen metinler ────────────────────────────────────────────────────────────────────────────

fn tarihli(etiket: String, tarih: Option<NaiveDate>) -> String {
    match tarih {
        Some(t) => format!("{etiket} ({})", t.format("%d.%m.%Y")),
        None => etiket,
    }
}

/// "Kesinleşme Bekliyor (14.10.2026)"
pub fn kesinlesme_metni(durum: &str, tarih: Option<NaiveDate>) -> String {
    if durum.is_empty() { String::new() } else { tarihli(K::etiket_bul(durum), tarih) }
}

/// "Tebliğ Edildi (12.09.2026) — Muhtara teslim (21. md.)"
pub fn tebligat_metni(durum: &str, tarih: Option<NaiveDate>, sekli: &str) -> String {
    if durum.is_empty() {
        return String::new();
    }
    let m = tarihli(T::etiket_bul(durum), tarih);
    if sekli.is_empty() { m } else { format!("{m} — {sekli}") }
}

/// "Tebliğ (05.10.2026)"
pub fn takip_asamasi_metni(asama: &str, tarih: Option<NaiveDate>) -> String {
    if asama.is_empty() { String::new() } else { tarihli(A::etiket_bul(asama), tarih) }
}

// ── Veritabanı ──────────────────────────────────────────────────────────────────────────────────

/// Tebligatların borçlusunu muhatap kuralına göre yeniden hesaplar ve farklıysa düzeltir; yanlış
/// bağlanmış kayıt (ör. tek borçlulu dosyada bankaya giden tebligat) da çözülür. Döner: değişen
/// kayıt sayısı.
pub fn tebligat_borclularini_duzelt<C: GenericClient>(c: &mut C, dosya: Option<i64>) -> Result<usize, String> {
    let borclular = kayit::borclu_haritasi(c, dosya)?;
    let vekiller = kayit::borclu_vekil_haritasi(c, dosya)?;
    let satirlar = c
        .query(
            "SELECT id, dosya_id, evrak_aciklama, ham_yanit, borclu_id FROM tebligat_barkod
              WHERE $1::bigint IS NULL OR dosya_id = $1 ORDER BY id",
            &[&dosya],
        )
        .map_err(hata)?;
    let bos = Vec::new();
    let mut degisen = 0;
    for s in satirlar {
        let (id, dosya_id): (i64, i64) = (s.get(0), s.get(1));
        let aciklama: String = s.get(2);
        let ham: Option<Value> = s.get(3);
        let eski: Option<i64> = s.get(4);
        let yeni = teb::tebligat_borclusunu_coz(
            &aciklama,
            borclular.get(&dosya_id).unwrap_or(&bos),
            vekiller.get(&dosya_id).unwrap_or(&bos),
            ham.as_ref(),
        );
        if yeni != eski {
            c.execute("UPDATE tebligat_barkod SET borclu_id = $2 WHERE id = $1", &[&id, &yeni]).map_err(hata)?;
            degisen += 1;
        }
    }
    Ok(degisen)
}

/// Tebligatların türünü sabit gruba, eski ham türü `zarf_icerigi`ne taşır ve muhatabı (ad, tür) yazar. Ham tür: sabit
/// türlerde ve kullanıcının eklediği türlerde olmayan, memurun yazdığı içerik. Önce kullanıcının eşleştirmesi, sonra
/// yazılış kuralları ([`teb::icerik_turu`]); ikisi de tanımazsa ham tür kalır (Tür eşleştir'de sorulur). UYAP'a gitmez.
/// Döner: değişen kayıt sayısı.
pub fn tebligat_alanlarini_duzelt<C: GenericClient>(c: &mut C, dosya: Option<i64>) -> Result<usize, String> {
    let bilinen: std::collections::HashSet<String> = kayit::tur_kategorileri(c)?.into_iter().collect();
    let eslesmeler = kayit::tur_eslesmeleri(c)?;
    let satirlar = c
        .query(
            "SELECT id, tebligat_turu, zarf_icerigi, evrak_aciklama, muhatap_adi, muhatap_turu FROM tebligat_barkod
              WHERE $1::bigint IS NULL OR dosya_id = $1",
            &[&dosya],
        )
        .map_err(hata)?;
    let mut degisen = 0;
    for s in satirlar {
        let id: i64 = s.get(0);
        let (eski_tur, eski_zarf, aciklama, eski_ad, eski_mt): (String, String, String, String, String) =
            (s.get(1), s.get(2), s.get(3), s.get(4), s.get(5));
        let (mut tur, mut zarf) = (eski_tur.clone(), eski_zarf.clone());
        if !tur.is_empty() && tur != teb::BELIRSIZ && !bilinen.contains(&tur) {
            if zarf.is_empty() {
                zarf = tur.clone();
            }
            if let Some(t) = teb::tur_eslestir(&tur, &eslesmeler).or_else(|| teb::icerik_turu(&tur).map(String::from)) {
                tur = t;
            }
        }
        let ad = veri::kirp(&teb::muhatap_adi(&aciklama), 255);
        let mt = teb::muhatap_turu(&aciklama).kod().to_string();
        if (&tur, &zarf, &ad, &mt) != (&eski_tur, &eski_zarf, &eski_ad, &eski_mt) {
            c.execute(
                "UPDATE tebligat_barkod SET tebligat_turu = $2, zarf_icerigi = $3, muhatap_adi = $4, muhatap_turu = $5 WHERE id = $1",
                &[&id, &veri::kirp(&tur, 120), &veri::kirp(&zarf, 500), &ad, &mt],
            )
            .map_err(hata)?;
            degisen += 1;
        }
    }
    Ok(degisen)
}

/// Dosyalardaki ödeme emri evrakının günü (evrak listesinden): dosya → en yeni gün. Evrak türü
/// adının UYAP'taki tam yazılışı canlı doğrulanmadı; "Ödeme Emri" / "Ödeme İcra Emri" kabul edilir.
fn odeme_emri_evraklari<C: GenericClient>(c: &mut C, dosya: Option<i64>) -> Result<HashMap<i64, Option<NaiveDate>>, String> {
    Ok(c.query(
        "SELECT dosya_id, max(evrak_tarihi)::date FROM evrak
          WHERE ($1::bigint IS NULL OR dosya_id = $1) AND evrak_turu ~ '^\\s*[öÖ]deme\\s+([iİ]cra\\s+)?[eE]mr[iİ]\\s*$'
          GROUP BY dosya_id",
        &[&dosya],
    )
    .map_err(hata)?
    .iter()
    .map(|s| (s.get(0), s.get(1)))
    .collect())
}

/// Hesap sonucu.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sonuc {
    /// Değeri değişen borçlu satırı sayısı.
    pub guncellenen: usize,
    /// Durum geçmişine yazılan olay sayısı.
    pub olay: usize,
    /// Durum geçişinden şablonla açılan iş sayısı (L7).
    pub otomatik_is: usize,
}

/// Borçluların kesinleşme, tebliğ durumu ve takip aşamasını kayıtlı tebligatlardan hesaplayıp
/// yazar. `dosya` verilirse yalnız o dosyanın borçluları; verilmezse tebligatı, durdurması,
/// itirazı olan ya da takip aşaması henüz yazılmamış bütün icra borçluları. Elle girilen
/// Konkordato / Vefat / Vasi / Tehir-i İcra değerleri ezilmez. `bugun` sürenin dolup dolmadığını belirler.
pub fn durumlari_guncelle<C: GenericClient>(c: &mut C, dosya: Option<i64>, bugun: NaiveDate, log: kayit::Gunluk) -> Result<Sonuc, String> {
    let mut tx = c.transaction().map_err(hata)?;
    let duzeltilen = tebligat_borclularini_duzelt(&mut tx, dosya)?;
    if duzeltilen > 0 {
        log(&format!("… tebligat → borçlu eşlemesi güncellendi: {duzeltilen} tebligat kaydı."));
    }
    let gruplanan = tebligat_alanlarini_duzelt(&mut tx, dosya)?;
    if gruplanan > 0 {
        log(&format!("… tebligat türü / muhatap güncellendi: {gruplanan} tebligat kaydı."));
    }
    let tatiller = veri::resmi_tatiller(&mut tx)?;
    let kurallar = veri::kesinlesme_kurallari(&mut tx)?;
    let evraklar = odeme_emri_evraklari(&mut tx, dosya)?;

    let vekiller = kayit::borclu_vekil_haritasi(&mut tx, dosya)?;
    let mut odeme_emirleri: HashMap<i64, Vec<OdemeEmri>> = HashMap::new();
    for s in tx
        .query(
            "SELECT tb.borclu_id, tb.birim_evrak_no, tb.evrak_aciklama, tb.evrak_tarihi, tb.evrak_gunu, tb.barkod,
                    tb.elektronik_tebligat, tb.ptt_durumu, tb.son_islem_tarihi, tb.son_islem_zamani::date, tb.teslim_sekli,
                    tb.tebligat_mazbatasi_var, tb.tebligat_mazbatasi_aciklama, tb.dosya_id, mb.teblig_tarihi, COALESCE(mb.iade, false)
               FROM tebligat_barkod tb
               LEFT JOIN tebligat_mazbata_bagi mb ON mb.dosya_id = tb.dosya_id AND mb.tebligat_evrak_no = tb.birim_evrak_no
              WHERE tb.tebligat_turu = ANY($2) AND ($1::bigint IS NULL OR tb.dosya_id = $1)
              ORDER BY tb.id",
            &[&dosya, &KESINLESME_TEBLIGATLARI.to_vec()],
        )
        .map_err(hata)?
    {
        let aciklama: String = s.get(2);
        // Üçüncü kişiye giden zarf (borçlu tek diye borçluya bağlanmış olabilir) borçlunun tebliği ve kesinleşmesi değildir.
        if teb::muhatap_turu(&aciklama) == teb::MuhatapTuru::Ucuncu {
            continue;
        }
        // Borçluya bağlı değilse: vekile giden zarf vekilin temsil ettiği bütün borçluların tebliğidir (vekil birden çok
        // borçlunun vekiliyken zarf tek borçluya bağlanamıyor, borçlular "tebliğ bekleniyor"da kalıyordu).
        let sahipler: Vec<i64> = match s.get::<_, Option<i64>>(0) {
            Some(b) => vec![b],
            None => teb::vekilin_borclulari(&aciklama, vekiller.get(&s.get::<_, i64>(13)).map(Vec::as_slice).unwrap_or(&[])),
        };
        if sahipler.is_empty() {
            continue;
        }
        let mut oe = OdemeEmri {
            birim_evrak_no: s.get(1),
            evrak_aciklama: aciklama,
            evrak_tarihi: s.get(3),
            evrak_gunu: s.get(4),
            barkod: s.get(5),
            elektronik_tebligat: s.get(6),
            ptt_durumu: s.get(7),
            son_islem_tarihi: s.get(8),
            son_islem_gunu: s.get(9),
            teslim_sekli: s.get(10),
            tebligat_mazbatasi_var: s.get(11),
            tebligat_mazbatasi_aciklama: s.get(12),
        };
        // Elle bağlanan mazbata iade döndü (kullanıcı işaretledi): tebligat İade; PTT'nin "teslim edildi"si ve günü geçersiz,
        // kesinleşme başlamaz. Girilmişse tarih iade günüdür (iade dönen tebligatın tebliğ tarihi olmaz).
        if s.get::<_, bool>(15) && !oe.elektronik_tebligat {
            let t = s.get::<_, Option<NaiveDate>>(14);
            oe.ptt_durumu = MAZBATADAN_IADE.into();
            oe.son_islem_gunu = t;
            oe.son_islem_tarihi = t.map(|t| t.format("%d/%m/%Y").to_string()).unwrap_or_default();
            oe.teslim_sekli = "Mazbata: tebliğ edilemedi (elle)".into();
        }
        // Elle bağlanan mazbatadaki tebliğ günü (kullanıcı girdi): PTT'nin bilgisi yerine geçer (eski barkod PTT'de silinmiş).
        else if let Some(t) = s.get::<_, Option<NaiveDate>>(14).filter(|_| !oe.elektronik_tebligat) {
            oe.ptt_durumu = MAZBATADAN_TESLIM.into();
            oe.son_islem_gunu = Some(t);
            oe.son_islem_tarihi = t.format("%d/%m/%Y").to_string();
            if oe.teslim_sekli.is_empty() {
                oe.teslim_sekli = "Mazbatadaki tebliğ tarihi (elle)".into();
            }
        }
        for b in sahipler {
            odeme_emirleri.entry(b).or_default().push(oe.clone());
        }
    }

    let borclular = tx
        .query(
            "SELECT dt.id, dt.dosya_id, dt.itiraz_edildi_mi, dt.itiraz_tarihi, dt.kesinlesme_durumu, dt.kesinlesme_tarihi,
                    dt.tebligat_durumu, dt.tebligat_durum_tarihi, dt.tebligat_sekli, dt.takip_asamasi,
                    dt.takip_asamasi_tarihi, d.durum, d.yargi_turu,
                    COALESCE(i.takibin_turu, ''), COALESCE(i.takibin_yolu, ''), COALESCE(i.takibin_sekli, ''), d.tur_kod, d.tur,
                    dt.kesinlesme_elle_tarihi, dt.uyap_kesinlesti, dt.uyap_kesinlesti_gunu
               FROM dosya_taraf dt JOIN dosya d ON d.id = dt.dosya_id
               LEFT JOIN icra_takip_detay i ON i.dosya_id = d.id
              WHERE dt.rol = 'borclu'
                AND (dt.dosya_id = $1
                     OR ($1::bigint IS NULL
                         AND (EXISTS (SELECT 1 FROM tebligat_barkod tb WHERE tb.dosya_id = d.id)
                              OR d.durum LIKE '%Durdurulmuş%'
                              OR dt.kesinlesme_durumu = 'itiraz' OR dt.kesinlesme_durumu LIKE 'durd\\_%'
                              OR dt.kesinlesme_elle_tarihi IS NOT NULL OR dt.uyap_kesinlesti
                              OR (d.yargi_turu = 2 AND dt.takip_asamasi = ''))))
              ORDER BY dt.id",
            &[&dosya],
        )
        .map_err(hata)?;

    let icra = veri::YargiTuru::Icra.kod();
    let elle = [K::Konkordato.kod(), K::Vefat.kod(), K::Vasi.kod(), K::TehiriIcra.kod()];
    let bos: Vec<OdemeEmri> = Vec::new();
    let (mut kuralsiz, mut tanimsiz) = (BTreeSet::new(), BTreeSet::new());
    let mut sonuc = Sonuc::default();
    for s in borclular {
        let (id, dosya_id): (i64, i64) = (s.get(0), s.get(1));
        let itiraz: bool = s.get(2);
        let eski = BorcluDurumu {
            itiraz_tarihi: s.get(3),
            kesinlesme_durumu: s.get(4),
            kesinlesme_tarihi: s.get(5),
            tebligat_durumu: s.get(6),
            tebligat_durum_tarihi: s.get(7),
            tebligat_sekli: s.get(8),
        };
        let (eski_asama, eski_asama_tarihi): (String, Option<NaiveDate>) = (s.get(9), s.get(10));
        let durum: String = s.get(11);
        let icra_mi = s.get::<_, i16>(12) == icra;
        let (turu, yolu, sekli): (String, String, String) = (s.get(13), s.get(14), s.get(15));
        let talimat = icra_mi && talimat_mi(s.get(16), &s.get::<_, String>(17));
        let oe = odeme_emirleri.get(&id).unwrap_or(&bos);

        let durdurma = if icra_mi { durdurma_kesinlesme_kodu(&durum) } else { None };
        if durdurma == Some(K::DurdurulmusTanimsiz.kod()) {
            tanimsiz.insert(durum.clone());
        }
        let kural = veri::kesinlesme_kurali_sec(&kurallar, &turu, &yolu, &sekli);
        if kural.is_none() && !talimat {
            let ya = |x: &str| if x.is_empty() { "?".to_string() } else { x.to_string() };
            kuralsiz.insert(format!("{}/{}/{}", ya(&turu), ya(&yolu), ya(&sekli)));
        }
        // Kural bu türde kesinleşme yok diyor (ör. Örnek 3, "Diğer"): talimat gibi kesinleşme kontrol işi açılmaz.
        let kesinlesme_yok = talimat || kural.is_some_and(|k| k.kesinlesme_suresi_gun.is_none());
        let itiraz_etkili = itiraz_etkili_mi(itiraz, kural);
        let mut yeni = eski.clone();
        (yeni.kesinlesme_durumu, yeni.kesinlesme_tarihi) =
            borclu_kesinlesme_hesapla(itiraz_etkili, oe, &tatiller, bugun, durdurma, kural.and_then(|k| k.kesinlesme_suresi_gun));
        if talimat {
            // Talimat dosyasında kesinleşme yok (elle girilen konkordato / vefat / vasi korunur).
            (yeni.kesinlesme_durumu, yeni.kesinlesme_tarihi) = (String::new(), None);
        }
        // Kullanıcının girdiği kesinleşme günü hesabı ezer (etkili itiraz ve durdurma hariç).
        if let Some(t) = s.get::<_, Option<NaiveDate>>(18).filter(|_| !talimat && !itiraz_etkili && durdurma.is_none()) {
            (yeni.kesinlesme_durumu, yeni.kesinlesme_tarihi) = (elle_kesinlesme(t, bugun).into(), Some(t));
        }
        // UYAP'ta takip kesinleşmiş görüldüyse (Kesinleştirme sekmesindeki sorgu / talep öncesi denetim) Kesinleşti:
        // programın hesabı ve itiraz işareti ezilir (kullanıcı kuralı 23.11.2026; eski PTT barkodu sorgulanamadığı için
        // tebliğ günü bulunamayan borçlu "bekliyor"da kalıyordu). Gün: hesaplanan kesinleşme günü UYAP'ta görüldüğü
        // günden sonra değilse o, değilse ilk görüldüğü gün.
        if s.get::<_, Option<bool>>(19) == Some(true) && !talimat && durdurma.is_none() {
            let gorulen: Option<NaiveDate> = s.get(20);
            let gun = yeni.kesinlesme_tarihi.filter(|t| gorulen.map_or(true, |g| *t <= g)).or(gorulen);
            (yeni.kesinlesme_durumu, yeni.kesinlesme_tarihi) = (K::Kesinlesti.kod().into(), gun);
        }
        if elle.contains(&eski.kesinlesme_durumu.as_str()) && (!itiraz_etkili || talimat) {
            (yeni.kesinlesme_durumu, yeni.kesinlesme_tarihi) = (eski.kesinlesme_durumu.clone(), eski.kesinlesme_tarihi);
        }
        (yeni.tebligat_durumu, yeni.tebligat_durum_tarihi, yeni.tebligat_sekli) = borclu_tebligat_hesapla(oe, bugun);
        let (asama, asama_tarihi) = if icra_mi {
            let (a, t) = takip_asamasi_hesapla(&yeni, oe, evraklar.get(&dosya_id).copied());
            (a.kod().to_string(), t)
        } else {
            (String::new(), None)
        };

        // Olaylar: (tür, eski metin, yeni metin, olay günü)
        let mut olaylar: Vec<(OlayTur, String, String, Option<NaiveDate>)> = Vec::new();
        if (&yeni.kesinlesme_durumu, yeni.kesinlesme_tarihi) != (&eski.kesinlesme_durumu, eski.kesinlesme_tarihi) {
            olaylar.push((
                OlayTur::Kesinlesme,
                kesinlesme_metni(&eski.kesinlesme_durumu, eski.kesinlesme_tarihi),
                kesinlesme_metni(&yeni.kesinlesme_durumu, yeni.kesinlesme_tarihi),
                yeni.kesinlesme_tarihi,
            ));
        }
        if (&yeni.tebligat_durumu, yeni.tebligat_durum_tarihi, &yeni.tebligat_sekli)
            != (&eski.tebligat_durumu, eski.tebligat_durum_tarihi, &eski.tebligat_sekli)
        {
            olaylar.push((
                OlayTur::Tebligat,
                tebligat_metni(&eski.tebligat_durumu, eski.tebligat_durum_tarihi, &eski.tebligat_sekli),
                tebligat_metni(&yeni.tebligat_durumu, yeni.tebligat_durum_tarihi, &yeni.tebligat_sekli),
                yeni.tebligat_durum_tarihi,
            ));
        }
        let asama_degisti = (&asama, asama_tarihi) != (&eski_asama, eski_asama_tarihi);
        if asama_degisti && !eski_asama.is_empty() && !asama.is_empty() && asama != eski_asama {
            // Geçmişe yalnız aşama değişince yazılır (gün değişimi değil; ilk hesap da yazılmaz).
            // Tebliğ / iade / kesinleşme / itiraz / durdurma bu turdaki kesinleşme ya da tebliğ
            // olayıyla zaten görünüyorsa tekrar edilmez.
            let sonuc_asamasi = [A::Teblig, A::Iade, A::Kesinlesti, A::Itiraz, A::Durduruldu].iter().any(|a| a.kod() == asama);
            if !(sonuc_asamasi && !olaylar.is_empty()) {
                olaylar.push((
                    OlayTur::TakipAsamasi,
                    takip_asamasi_metni(&eski_asama, eski_asama_tarihi),
                    takip_asamasi_metni(&asama, asama_tarihi),
                    asama_tarihi,
                ));
            }
        }
        if yeni == eski && !asama_degisti {
            continue;
        }
        tx.execute(
            "UPDATE dosya_taraf SET kesinlesme_durumu = $2, kesinlesme_tarihi = $3, tebligat_durumu = $4,
                    tebligat_durum_tarihi = $5, tebligat_sekli = $6, takip_asamasi = $7, takip_asamasi_tarihi = $8
              WHERE id = $1",
            &[
                &id,
                &yeni.kesinlesme_durumu,
                &yeni.kesinlesme_tarihi,
                &yeni.tebligat_durumu,
                &yeni.tebligat_durum_tarihi,
                &veri::kirp(&yeni.tebligat_sekli, 150),
                &asama,
                &asama_tarihi,
            ],
        )
        .map_err(hata)?;
        sonuc.guncellenen += 1;
        let mut olay_kimlikleri: Vec<(OlayTur, i64)> = Vec::new();
        for (tur, eski_metin, yeni_metin, tarih) in &olaylar {
            let oid = durum_olayi_yaz(
                &mut tx,
                &YeniOlay {
                    dosya: dosya_id,
                    dosya_taraf: Some(id),
                    tur: *tur,
                    eski: eski_metin,
                    yeni: yeni_metin,
                    olay_tarihi: *tarih,
                    kaynak: OlayKaynak::Hesap,
                    aciklama: "",
                },
            )?;
            olay_kimlikleri.push((*tur, oid));
            sonuc.olay += 1;
        }
        // L7: tebliğ / iade / kesinleşme geçişinde şablondan otomatik iş (bkz. is_otomatik).
        let olay_bilgisi = |t: OlayTur| {
            let kimlik = olay_kimlikleri.iter().find(|(x, _)| *x == t).map(|(_, i)| *i);
            let metin = olaylar.iter().find(|o| o.0 == t).map(|o| o.2.clone()).unwrap_or_default();
            (kimlik, metin)
        };
        let mut gecisler: Vec<(&str, OlayTur, Option<NaiveDate>, Option<NaiveDate>)> = Vec::new();
        if let Some(o) = otomatik::tebligat_olayi(&eski.tebligat_durumu, &yeni.tebligat_durumu) {
            let son_gun = if o == "tebligat:teblig_edildi" { yeni.kesinlesme_tarihi } else { None };
            // Talimatta ve kesinleşmesi olmayan takip türünde "kesinleşme tarihi kontrol" işi anlamsız.
            if !(kesinlesme_yok && o == "tebligat:teblig_edildi") {
                gecisler.push((o, OlayTur::Tebligat, yeni.tebligat_durum_tarihi, son_gun));
            }
        }
        if let Some(o) = otomatik::kesinlesme_olayi(&eski.kesinlesme_durumu, &yeni.kesinlesme_durumu) {
            gecisler.push((o, OlayTur::Kesinlesme, yeni.kesinlesme_tarihi, None));
        }
        for (olay, tur, olay_tarihi, son_gun) in gecisler {
            let (kimlik, metin) = olay_bilgisi(tur);
            let g = otomatik::Gecis {
                dosya_id,
                taraf_id: id,
                durum_olayi_id: kimlik,
                olay,
                olay_tarihi,
                son_gun,
                olay_metni: &metin,
            };
            // Ayrı savepoint: iş açılamazsa icra hesabı yine kaydedilir.
            let mut sp = tx.transaction().map_err(hata)?;
            match otomatik::olaydan_is_ac(&mut sp, &g, bugun) {
                Ok(acilan) => {
                    sp.commit().map_err(hata)?;
                    sonuc.otomatik_is += acilan.len();
                }
                Err(e) => {
                    drop(sp);
                    log(&format!("⚠️ Otomatik iş açılamadı ({olay}, borçlu {id}): {e}"));
                }
            }
        }
    }
    tx.commit().map_err(hata)?;
    if !kuralsiz.is_empty() {
        log(&format!(
            "⚠️ Kesinleşme süresi tanımlı olmayan takip türü (kesinleşme hesaplanmadı): {}",
            kuralsiz.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if !tanimsiz.is_empty() {
        log(&format!(
            "⚠️ Tanınmayan durdurma nedeni (\"Durdurulmuş: Tanımsız Neden\" yazıldı): {}",
            tanimsiz.into_iter().collect::<Vec<_>>().join("; ")
        ));
    }
    if sonuc.guncellenen > 0 {
        log(&format!("… kesinleşme / tebliğ / takip aşaması güncellendi: {} borçlu.", sonuc.guncellenen));
    }
    Ok(sonuc)
}

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use crate::icra_veri::{dosya_kunyesi_kaydet, ham_tarihleri_turet, YargiTuru};
    use serde_json::json;
    let g = |y, a, d| NaiveDate::from_ymd_opt(y, a, d).unwrap();
    let say = |c: &mut postgres::Client, sql: &str| -> i64 { c.query_one(sql, &[]).unwrap().get(0) };
    let sessiz = |_: &str| {};
    let kunye = |no: &str, birim: i64, ad: &str| {
        json!({"dosyaId": "x", "dosyaNo": no, "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": 35, "birimAdi": ad, "birimId": birim})
    };
    let (d, _) = dosya_kunyesi_kaydet(c, &kunye("2026/5", 1001, "İzmir 1. İcra Dairesi"), Some(YargiTuru::Icra.kod())).unwrap();
    let taraflar = vec![
        json!({"adi": "AHMET YILMAZ", "rol": "Borçlu", "kisiKurum": "Kişi"}),
        json!({"adi": "X BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum"}),
    ];
    kayit::dosya_taraf_kaydet(c, d, &taraflar, &sessiz).unwrap();
    let bag: i64 = c.query_one("SELECT id FROM dosya_taraf WHERE dosya_id = $1 AND rol = 'borclu'", &[&d]).unwrap().get(0);
    let durum = |c: &mut postgres::Client| -> (String, Option<NaiveDate>, String, Option<NaiveDate>, String, Option<NaiveDate>) {
        let s = c
            .query_one(
                "SELECT takip_asamasi, takip_asamasi_tarihi, kesinlesme_durumu, kesinlesme_tarihi, tebligat_durumu, tebligat_durum_tarihi
                   FROM dosya_taraf WHERE id = $1",
                &[&bag],
            )
            .unwrap();
        (s.get(0), s.get(1), s.get(2), s.get(3), s.get(4), s.get(5))
    };
    let hesapla = |c: &mut postgres::Client, bugun: NaiveDate| {
        let s = durumlari_guncelle(c, Some(d), bugun, &sessiz).unwrap();
        assert_eq!(durumlari_guncelle(c, Some(d), bugun, &sessiz).unwrap(), Sonuc::default(), "ikinci hesap bir şey değiştirmez");
        s
    };
    let teb_guncelle = |c: &mut postgres::Client, sql: &str| {
        c.execute(&format!("UPDATE tebligat_barkod SET {sql} WHERE dosya_id = $1 AND birim_evrak_no = 10"), &[&d]).unwrap();
        let id: i64 = c.query_one("SELECT id FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = 10", &[&d]).unwrap().get(0);
        ham_tarihleri_turet(c, "tebligat_barkod", id).unwrap();
    };
    c.batch_execute("INSERT INTO kesinlesme_kurali (itiraz_suresi_gun, ad) VALUES (7, 'Varsayılan')").unwrap();

    // Takip talebi: ilk hesap geçmişe yazılmaz
    assert_eq!(hesapla(c, g(2026, 10, 1)), Sonuc { guncellenen: 1, olay: 0, otomatik_is: 0 });
    assert_eq!(durum(c).0, "takip_talebi");
    // Ödeme emri evrakı düzenlendi, tebligatı henüz yok
    kayit::evrak_listesi_kaydet(c, d, &[json!({"birimEvrakNo": 9, "tur": "Ödeme İcra Emri", "onaylandigiTarih": "01/10/2026 09:00"})]).unwrap();
    assert_eq!(hesapla(c, g(2026, 10, 1)).olay, 1);
    assert_eq!((durum(c).0.as_str(), durum(c).1), ("odeme_emri", Some(g(2026, 10, 1))));
    // Tebligat oluşturuldu (barkod yok); borçlusu açıklamadan bulunur
    c.execute(
        "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, evrak_tarihi, evrak_aciklama)
         VALUES ($1, 10, 'Ödeme Emri', '01/10/2026', 'Normal Tebligat[BORÇLU AHMET YILMAZ]')",
        &[&d],
    )
    .unwrap();
    teb_guncelle(c, "barkod = ''");
    hesapla(c, g(2026, 10, 1));
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE borclu_id IS NOT NULL"), 1);
    let s = durum(c);
    assert_eq!((s.0.as_str(), s.1, s.2.as_str(), s.3, s.4.as_str()), ("teb_olusturuluyor", Some(g(2026, 10, 1)), "kesinlesme_bekliyor", None, ""));
    teb_guncelle(c, "barkod = '4007663523293'");
    hesapla(c, g(2026, 10, 1));
    assert_eq!(durum(c).0, "ptt_veriliyor");
    teb_guncelle(c, "ptt_durumu = 'KABUL EDİLDİ', son_islem_tarihi = '02/10/2026'");
    assert_eq!(hesapla(c, g(2026, 10, 2)).guncellenen, 1);
    let s = durum(c);
    assert_eq!((s.0.as_str(), s.1, s.4.as_str()), ("ptt_veriliyor", Some(g(2026, 10, 2)), "dagitimda"));
    teb_guncelle(c, "ptt_durumu = 'DAĞITIMDA', son_islem_tarihi = '03/10/2026'");
    hesapla(c, g(2026, 10, 3));
    assert_eq!(durum(c).0, "teblige_cikti");
    // Teslim edildi (mazbata henüz dönmedi): tebliğ, kesinleşme bekliyor ve beklenen gün
    teb_guncelle(c, "ptt_durumu = 'TESLİM EDİLDİ', son_islem_tarihi = '05/10/2026 14:30', teslim_sekli = 'Muhataba bizzat teslim'");
    hesapla(c, g(2026, 10, 6));
    let s = durum(c);
    assert_eq!((s.0.as_str(), s.1), ("teblig", Some(g(2026, 10, 5))));
    assert_eq!((s.2.as_str(), s.3, s.4.as_str(), s.5), ("kesinlesme_bekliyor", Some(g(2026, 10, 13)), "teblig_edildi", Some(g(2026, 10, 5))));
    // Süre doldu ama mazbata dönmedi: hâlâ bekliyor
    hesapla(c, g(2026, 10, 20));
    assert_eq!(durum(c).2, "kesinlesme_bekliyor");
    // Mazbata döndü: süre dolmadan kesinleşmez, dolunca kesinleşir
    teb_guncelle(c, "evrak_aciklama = evrak_aciklama || ' - Tebliğ Edildi'");
    hesapla(c, g(2026, 10, 12));
    assert_eq!(durum(c).2, "kesinlesme_bekliyor");
    hesapla(c, g(2026, 10, 13));
    let s = durum(c);
    assert_eq!((s.0.as_str(), s.1, s.2.as_str(), s.3), ("kesinlesti", Some(g(2026, 10, 13)), "kesinlesti", Some(g(2026, 10, 13))));

    // Geçmiş: aşama olayları yalnız kendi başına görünen geçişler için
    let asamalar: Vec<String> = c
        .query("SELECT yeni FROM durum_olayi WHERE tur = 'takip_asamasi' ORDER BY id", &[])
        .unwrap()
        .iter()
        .map(|s| s.get::<_, String>(0).split(" (").next().unwrap().to_string())
        .collect();
    assert_eq!(asamalar, ["Ödeme Emri", "Ödeme Emri Tebligatı Oluşturuluyor", "Ödeme Emri PTT'ye Veriliyor", "Ödeme Emri Tebliğe Çıktı"]);
    let s = c.query_one("SELECT eski, yeni, olay_tarihi, taraf_adi, kaynak FROM durum_olayi WHERE tur = 'kesinlesme' ORDER BY id DESC LIMIT 1", &[]).unwrap();
    assert_eq!(s.get::<_, String>(0), "Kesinleşme Bekliyor (13.10.2026)");
    assert_eq!(s.get::<_, String>(1), "Kesinleşti (13.10.2026)");
    assert_eq!((s.get::<_, Option<NaiveDate>>(2), s.get::<_, String>(3).as_str(), s.get::<_, String>(4).as_str()), (Some(g(2026, 10, 13)), "AHMET YILMAZ", "hesap"));
    assert_eq!(
        say(c, "SELECT count(*) FROM durum_olayi WHERE tur = 'tebligat' AND yeni = 'Tebliğ Edildi (05.10.2026) — Muhataba bizzat teslim'"),
        1
    );

    // Elle itiraz
    c.execute("UPDATE dosya_taraf SET itiraz_edildi_mi = true, itiraz_tarihi = '2026-10-08' WHERE id = $1", &[&bag]).unwrap();
    hesapla(c, g(2026, 10, 14));
    let s = durum(c);
    assert_eq!((s.0.as_str(), s.1, s.2.as_str(), s.3), ("itiraz", Some(g(2026, 10, 8)), "itiraz", None));
    // İtirazın takibi durdurmadığı türde (ilamlı, kambiyo haciz yolu) elle itiraz kesinleşmeyi durdurmaz
    c.batch_execute("UPDATE kesinlesme_kurali SET itiraz_durdurur = false").unwrap();
    hesapla(c, g(2026, 10, 14));
    let s = durum(c);
    assert_eq!((s.0.as_str(), s.2.as_str(), s.3), ("kesinlesti", "kesinlesti", Some(g(2026, 10, 13))));
    c.batch_execute("UPDATE kesinlesme_kurali SET itiraz_durdurur = true").unwrap();
    hesapla(c, g(2026, 10, 14));
    assert_eq!(durum(c).2, "itiraz");
    // Tahliye emri (Örnek 14) de borçlunun kesinleşmesini başlatır
    c.execute("UPDATE dosya_taraf SET itiraz_edildi_mi = false WHERE id = $1", &[&bag]).unwrap();
    teb_guncelle(c, "tebligat_turu = 'Tahliye Emri'");
    hesapla(c, g(2026, 10, 14));
    assert_eq!(durum(c).2, "kesinlesti");
    teb_guncelle(c, "tebligat_turu = 'Maaş'");
    hesapla(c, g(2026, 10, 14));
    assert_eq!(durum(c).2, "", "maaş zarfı kesinleşme başlatmaz");
    teb_guncelle(c, "tebligat_turu = 'Ödeme Emri'");
    // Kural "kesinleşme yok" diyorsa (Örnek 3, Diğer) hesaplanmaz, süre uyarısı da verilmez
    c.batch_execute("UPDATE kesinlesme_kurali SET itiraz_suresi_gun = NULL").unwrap();
    let uyarilar = std::cell::RefCell::new(Vec::new());
    durumlari_guncelle(c, Some(d), g(2026, 10, 14), &|m: &str| uyarilar.borrow_mut().push(m.to_string())).unwrap();
    assert_eq!(durum(c).2, "");
    assert!(!uyarilar.borrow().iter().any(|m| m.contains("Kesinleşme süresi")), "{uyarilar:?}");
    c.batch_execute("UPDATE kesinlesme_kurali SET itiraz_suresi_gun = 7").unwrap();
    hesapla(c, g(2026, 10, 14));
    assert_eq!(durum(c).2, "kesinlesti");
    // İade
    c.execute("UPDATE dosya_taraf SET itiraz_edildi_mi = false WHERE id = $1", &[&bag]).unwrap();
    teb_guncelle(c, "ptt_durumu = 'İADE EDİLDİ', son_islem_tarihi = '06/10/2026', teslim_sekli = 'ADRESTE TANINMIYOR', evrak_aciklama = 'Normal Tebligat[BORÇLU AHMET YILMAZ] - Tebliğ Edilemedi'");
    hesapla(c, g(2026, 10, 14));
    let s = durum(c);
    assert_eq!((s.0.as_str(), s.1, s.2.as_str(), s.4.as_str(), s.5), ("iade", Some(g(2026, 10, 6)), "kesinlesme_bekliyor", "iade", Some(g(2026, 10, 6))));
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf WHERE tebligat_sekli = 'ADRESTE TANINMIYOR'"), 1);
    // Dosya UYAP'ta durduruldu
    c.execute("UPDATE dosya SET durum = 'Açık (Durdurulmuş : İhtiyati Tedbir)' WHERE id = $1", &[&d]).unwrap();
    hesapla(c, g(2026, 10, 14));
    assert_eq!((durum(c).0.as_str(), durum(c).2.as_str()), ("durduruldu", "durd_ihtiyati_tedbir"));
    // Elle girilen değer (Konkordato) hesapla ezilmez
    c.execute("UPDATE dosya SET durum = 'Açık' WHERE id = $1", &[&d]).unwrap();
    c.execute("UPDATE dosya_taraf SET kesinlesme_durumu = 'konkordato' WHERE id = $1", &[&bag]).unwrap();
    hesapla(c, g(2026, 10, 14));
    assert_eq!(durum(c).2, "konkordato");

    // Süre tanımlı değilse kesinleşme hesaplanmaz (uyarı verilir); tebliğ durumu yine yazılır
    c.batch_execute("DELETE FROM kesinlesme_kurali; UPDATE dosya_taraf SET kesinlesme_durumu = ''").unwrap();
    let uyarilar = std::cell::RefCell::new(Vec::new());
    durumlari_guncelle(c, Some(d), g(2026, 10, 14), &|m: &str| uyarilar.borrow_mut().push(m.to_string())).unwrap();
    assert!(uyarilar.borrow().iter().any(|m| m.contains("?/?/?")), "{uyarilar:?}");
    assert_eq!((durum(c).2.as_str(), durum(c).4.as_str()), ("", "iade"));

    // Tek borçlulu dosyada üçüncü kişiye giden zarf da borçluya bağlanır (listede borçlunun altında görünsün; kullanıcı
    // kararı 19.11.2026) ama borçlunun tebliğ / kesinleşme hesabına GİRMEZ: PTT'de "teslim edildi" görünen üçüncü kişi
    // ödeme emri borçlunun "iade" durumunu değiştirmez.
    c.execute(
        "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, borclu_id, evrak_aciklama, ptt_durumu, son_islem_tarihi)
         VALUES ($1, 11, 'Ödeme Emri', NULL, 'Elektronik Tebligat[ARACIKISIKURUM DENİZBANK A.Ş.]', 'TESLİM EDİLDİ', '10/10/2026')",
        &[&d],
    )
    .unwrap();
    assert_eq!(tebligat_borclularini_duzelt(c, Some(d)).unwrap(), 1);
    assert_eq!(say(c, &format!("SELECT count(*) FROM tebligat_barkod WHERE birim_evrak_no = 11 AND borclu_id = {bag}")), 1);
    durumlari_guncelle(c, Some(d), g(2026, 10, 14), &sessiz).unwrap();
    assert_eq!(durum(c).4, "iade", "üçüncü kişi zarfı borçlunun tebliğ durumunu değiştirmez");

    // Bütün dosyalar kipi: icra dışı dosyada takip aşaması yazılmaz; yeni icra borçlusu yakalanır
    let (h, _) = dosya_kunyesi_kaydet(c, &kunye("2026/7", 2002, "İzmir 3. Asliye Hukuk Mahkemesi"), Some(YargiTuru::Hukuk.kod())).unwrap();
    kayit::dosya_taraf_kaydet(c, h, &[json!({"adi": "MEHMET KAYA", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    let (d3, _) = dosya_kunyesi_kaydet(c, &kunye("2026/8", 1001, "İzmir 1. İcra Dairesi"), None).unwrap();
    kayit::dosya_taraf_kaydet(c, d3, &[json!({"adi": "AYŞE DEMİR", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    durumlari_guncelle(c, None, g(2026, 10, 14), &sessiz).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf dt JOIN dosya d ON d.id = dt.dosya_id WHERE d.yargi_turu = 1 AND dt.takip_asamasi = ''"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf WHERE takip_asamasi = 'takip_talebi'"), 1);
    assert_eq!(durumlari_guncelle(c, None, g(2026, 10, 14), &sessiz).unwrap(), Sonuc::default());

    // Talimat dosyası: tebliğ edilmiş ödeme emri olsa da kesinleşme hesaplanmaz, süre uyarısı verilmez,
    // "kesinleşme tarihi kontrol" işi açılmaz; tebliğ durumu yine yazılır.
    c.batch_execute("INSERT INTO kesinlesme_kurali (itiraz_suresi_gun, ad) VALUES (7, 'Varsayılan')").unwrap();
    let mut tk = kunye("2026/9", 1001, "İzmir 1. İcra Dairesi");
    tk["dosyaTurKod"] = json!(1);
    tk["dosyaTur"] = json!("Talimat Dosyası");
    let (dt_, _) = dosya_kunyesi_kaydet(c, &tk, Some(YargiTuru::Icra.kod())).unwrap();
    kayit::dosya_taraf_kaydet(c, dt_, &[json!({"adi": "VELİ KARA", "rol": "Borçlu", "kisiKurum": "Kişi"})], &sessiz).unwrap();
    let tbag: i64 = c.query_one("SELECT id FROM dosya_taraf WHERE dosya_id = $1 AND rol = 'borclu'", &[&dt_]).unwrap().get(0);
    c.execute(
        "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, borclu_id, evrak_aciklama, barkod, ptt_durumu, son_islem_tarihi,
                                      tebligat_mazbatasi_var, tebligat_mazbatasi_aciklama)
         VALUES ($1, 3, 'Ödeme Emri', $2, 'Normal Tebligat[BORÇLU VELİ KARA]', '4007663523299', 'TESLİM EDİLDİ', '01/10/2026', true,
                 'Ödeme Emri - [VELİ KARA] - Barkod Numarası: 4007663523299 - Tebliğ Edildi')",
        &[&dt_, &tbag],
    )
    .unwrap();
    let tid: i64 = c.query_one("SELECT id FROM tebligat_barkod WHERE dosya_id = $1", &[&dt_]).unwrap().get(0);
    ham_tarihleri_turet(c, "tebligat_barkod", tid).unwrap();
    // Önce "dağıtımda" sayılsın, sonra tebliğ geçişi olsun (otomatik iş gerçek geçişte açılır)
    c.execute("UPDATE dosya_taraf SET tebligat_durumu = 'dagitimda' WHERE id = $1", &[&tbag]).unwrap();
    let uyarilar = std::cell::RefCell::new(Vec::new());
    let s = durumlari_guncelle(c, Some(dt_), g(2026, 10, 3), &|m: &str| uyarilar.borrow_mut().push(m.to_string())).unwrap();
    let (kes, teb_d): (String, String) =
        c.query_one("SELECT kesinlesme_durumu, tebligat_durumu FROM dosya_taraf WHERE id = $1", &[&tbag]).map(|r| (r.get(0), r.get(1))).unwrap();
    assert_eq!((kes.as_str(), teb_d.as_str()), ("", "teblig_edildi"), "talimatta kesinleşme yok, tebliğ var");
    assert_eq!(s.otomatik_is, 0, "talimatta kesinleşme kontrol işi açılmaz");
    assert!(!uyarilar.borrow().iter().any(|m| m.contains("Kesinleşme süresi")), "{uyarilar:?}");
    assert!(talimat_mi(1, "") && talimat_mi(0, "Talimat Dosyası") && !talimat_mi(35, "İcra Dosyası"));

    // ── 23.11.2026: vekile giden e-tebligat (vekil iki borçlunun), mazbatadaki tebliğ tarihi, UYAP kesinleşmesi ──
    let (dv, _) = dosya_kunyesi_kaydet(c, &kunye("2026/88", 1001, "İzmir 1. İcra Dairesi"), Some(YargiTuru::Icra.kod())).unwrap();
    let ucu = ["ALİ VELİ", "AYŞE VELİ", "CEM KARA"].map(|a| json!({"adi": a, "rol": "Borçlu", "kisiKurum": "Kişi"}));
    kayit::dosya_taraf_kaydet(c, dv, &ucu, &sessiz).unwrap();
    let bb: Vec<i64> = c.query("SELECT id FROM dosya_taraf WHERE dosya_id = $1 AND rol = 'borclu' ORDER BY sira, id", &[&dv]).unwrap()
        .iter().map(|r| r.get(0)).collect();
    let (ali, ayse, cem) = (bb[0], bb[1], bb[2]);
    let vk: i64 = c.query_one("INSERT INTO vekil (ad, soyad) VALUES ('ZEYNEP', 'AK') RETURNING id", &[]).unwrap().get(0);
    for b in [ali, ayse] {
        c.execute("INSERT INTO dosya_taraf_vekiller (dosyataraf_id, vekil_id) VALUES ($1, $2)", &[&b, &vk]).unwrap();
    }
    for (no, ack, barkod, e, ptt, son) in [
        (20, "Elektronik Tebligat[Vekil ZEYNEP AK]", "5000000000001", true, "E-Tebligat: tebliğ edildi (süre 5 gün sonra başlar)", "01/10/2026"),
        (21, "Normal Tebligat[BORÇLU CEM KARA]", "4000000000002", false, "KAYIT YOK", ""),
    ] {
        let id: i64 = c.query_one(
            "INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, tebligat_turu, evrak_tarihi, evrak_aciklama, barkod, elektronik_tebligat, ptt_durumu, son_islem_tarihi)
             VALUES ($1, $2, 'Ödeme Emri', '01/10/2026', $3, $4, $5, $6, $7) RETURNING id",
            &[&dv, &no, &ack, &barkod, &e, &ptt, &son],
        ).unwrap().get(0);
        ham_tarihleri_turet(c, "tebligat_barkod", id).unwrap();
    }
    kayit::evrak_listesi_kaydet(c, dv, &[json!({"birimEvrakNo": 22, "tur": "Tebliğ Mazbatası", "aciklama": "", "onaylandigiTarih": "10/10/2026"})]).unwrap();
    let kes = |c: &mut postgres::Client, b: i64| -> (String, Option<NaiveDate>) {
        c.query_one("SELECT kesinlesme_durumu, kesinlesme_tarihi FROM dosya_taraf WHERE id = $1", &[&b]).map(|r| (r.get(0), r.get(1))).unwrap()
    };
    let bugun = g(2026, 11, 1);
    durumlari_guncelle(c, Some(dv), bugun, &sessiz).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE birim_evrak_no = 20 AND borclu_id IS NULL"), 1, "iki aday: zarf bağlanmaz");
    assert_eq!((kes(c, ali).0.as_str(), kes(c, ayse).0.as_str()), ("kesinlesti", "kesinlesti"), "vekile tebliğ iki borçluya da sayılır");
    assert_eq!(kes(c, cem), ("kesinlesme_bekliyor".into(), None), "PTT'de kayıt yok: tebliğ günü yok");
    // Mazbata bağlanırken mazbatadaki tebliğ tarihi girilir → kesinleşme o günden.
    assert!(crate::icra_api::mazbata_bagla_tarihli(c, dv, 21, Some(22), Some(g(2026, 12, 1)), bugun).unwrap_err().contains("bugünden sonra"));
    crate::icra_api::mazbata_bagla_tarihli(c, dv, 21, Some(22), Some(g(2026, 10, 8)), bugun).unwrap();
    let k = kes(c, cem);
    assert!(k.0 == "kesinlesti" && k.1.is_some_and(|t| t > g(2026, 10, 8) && t < g(2026, 10, 25)), "{k:?}");
    // 26.11.2026: mazbata iade döndü (tebliğ tarihi yok) → tebligat İade, kesinleşme başlamaz; tarih iade günüdür.
    let teb_durum = |c: &mut postgres::Client, b: i64| -> (String, Option<NaiveDate>) {
        c.query_one("SELECT tebligat_durumu, tebligat_durum_tarihi FROM dosya_taraf WHERE id = $1", &[&b]).map(|r| (r.get(0), r.get(1))).unwrap()
    };
    crate::icra_api::mazbata_bagla_sonuclu(c, dv, 21, Some(22), None, true, bugun).unwrap();
    assert_eq!(teb_durum(c, cem), ("iade".into(), None), "iade: tebliğ günü yok");
    assert_eq!(kes(c, cem), ("kesinlesme_bekliyor".into(), None), "iade dönen tebligatla kesinleşme başlamaz");
    crate::icra_api::mazbata_bagla_sonuclu(c, dv, 21, Some(22), Some(g(2026, 10, 9)), true, bugun).unwrap();
    assert_eq!(teb_durum(c, cem), ("iade".into(), Some(g(2026, 10, 9))), "girilen tarih iade günü");
    assert!(crate::icra_api::mazbata_bagla_sonuclu(c, dv, 21, Some(22), Some(g(2026, 12, 1)), true, bugun).unwrap_err().contains("İade tarihi"));
    // Bağ kaldırılınca bekliyor; UYAP'ta kesinleşmiş görülünce Kesinleşti (gün: görüldüğü gün), görülmeyince geri döner.
    crate::icra_api::mazbata_bagla(c, dv, 21, None, bugun).unwrap();
    assert_eq!(kes(c, cem).0, "kesinlesme_bekliyor");
    crate::icra_kesinlestirme::kesinlesme_isaretle(c, cem, true, bugun).unwrap();
    assert_eq!(kes(c, cem), ("kesinlesti".into(), Some(bugun)));
    crate::icra_kesinlestirme::kesinlesme_isaretle(c, cem, false, bugun).unwrap();
    assert_eq!(kes(c, cem).0, "kesinlesme_bekliyor");
    c.batch_execute("DELETE FROM vekil").unwrap();

    c.batch_execute("DELETE FROM dosya; DELETE FROM taraf; DELETE FROM birim; DELETE FROM kesinlesme_kurali").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(y: i32, a: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, a, d).unwrap()
    }

    fn fiziki(ptt: &str, son_islem: Option<NaiveDate>, aciklama: &str) -> OdemeEmri {
        OdemeEmri {
            birim_evrak_no: 10,
            evrak_aciklama: aciklama.into(),
            evrak_gunu: Some(g(2026, 10, 1)),
            barkod: "4007663523293".into(),
            ptt_durumu: ptt.into(),
            son_islem_gunu: son_islem,
            ..Default::default()
        }
    }

    #[test]
    fn sure_hesabi() {
        let bos = Tatiller::new();
        // 05.10.2026 Pazartesi: +7 = 12.10 Pazartesi; kesinleşme 13.10 Salı
        assert_eq!(
            kesinlesme_hesapla(g(2026, 10, 5), 7, &bos),
            KesinlesmeTarihleri { itiraz_son_gunu: g(2026, 10, 12), kesinlesebilir_tarihi: g(2026, 10, 13) }
        );
        // Cuma tebliğ: son gün Cuma, kesinleşme Pazartesi
        assert_eq!(kesinlesme_hesapla(g(2026, 10, 2), 7, &bos).kesinlesebilir_tarihi, g(2026, 10, 12));
        // Cumartesi tebliğ: son gün Cumartesi → Pazartesi'ye kayar, kesinleşme Salı
        let k = kesinlesme_hesapla(g(2026, 10, 3), 7, &bos);
        assert_eq!((k.itiraz_son_gunu, k.kesinlesebilir_tarihi), (g(2026, 10, 12), g(2026, 10, 13)));
        // Son gün resmî tatil (29.10 Perşembe) → Cuma; kesinleşme Pazartesi
        let tatil: Tatiller = [g(2026, 10, 29)].into_iter().collect();
        let k = kesinlesme_hesapla(g(2026, 10, 22), 7, &tatil);
        assert_eq!((k.itiraz_son_gunu, k.kesinlesebilir_tarihi), (g(2026, 10, 30), g(2026, 11, 2)));
        // Aradaki tatil süreyi durdurmaz
        let ara: Tatiller = [g(2026, 10, 7), g(2026, 10, 8)].into_iter().collect();
        assert_eq!(kesinlesme_hesapla(g(2026, 10, 5), 7, &ara).itiraz_son_gunu, g(2026, 10, 12));
        // Kesinleşme günü tatilse bir sonraki iş günü
        let k = kesinlesme_hesapla(g(2026, 10, 5), 7, &[g(2026, 10, 13)].into_iter().collect());
        assert_eq!(k.kesinlesebilir_tarihi, g(2026, 10, 14));
        // Başka süre (5 gün)
        assert_eq!(kesinlesme_hesapla(g(2026, 10, 5), 5, &bos).itiraz_son_gunu, g(2026, 10, 12), "10.10 Cumartesi → Pazartesi");
        assert!(is_gunu_mu(g(2026, 10, 5), &bos) && !is_gunu_mu(g(2026, 10, 4), &bos) && !is_gunu_mu(g(2026, 10, 29), &tatil));
        assert_eq!(sonraki_is_gunu(g(2026, 10, 5), &bos), g(2026, 10, 5));
        assert_eq!(ilk_is_gunu_sonrasi(g(2026, 10, 9), &bos), g(2026, 10, 12));
    }

    #[test]
    fn itiraz_etkisi() {
        let kural = |durdurur| veri::KesinlesmeKurali {
            id: 1,
            takibin_turu: String::new(),
            takibin_yolu: String::new(),
            takibin_sekli: String::new(),
            ad: String::new(),
            kesinlesme_suresi_gun: Some(7),
            aktif: true,
            aciklama: String::new(),
            itiraz_gun: None,
            itiraz_durdurur: durdurur,
            dayanak: String::new(),
            elle_degisti: false,
        };
        assert!(itiraz_etkili_mi(true, Some(&kural(true))));
        assert!(!itiraz_etkili_mi(true, Some(&kural(false))), "ilamlı / kambiyo: itiraz durdurmaz");
        assert!(itiraz_etkili_mi(true, None), "kural yoksa eskisi gibi durdurur");
        assert!(!itiraz_etkili_mi(false, None));
    }

    #[test]
    fn durdurma_kodlari() {
        assert_eq!(durdurma_kesinlesme_kodu("Açık (Durdurulmuş : Takibe İtiraz)"), Some("itiraz"));
        assert_eq!(durdurma_kesinlesme_kodu("Açık (Durdurulmuş: İhtiyati  Tedbir )"), Some("durd_ihtiyati_tedbir"));
        assert_eq!(durdurma_kesinlesme_kodu("Açık (Durdurulmuş : Tehir-i İcra Kararı)"), Some("durd_tehiri_icra"));
        assert_eq!(durdurma_kesinlesme_kodu("Açık (Durdurulmuş : Borçlunun Ölümü)"), Some("durd_borclu_olumu"));
        assert_eq!(durdurma_kesinlesme_kodu("Açık (Durdurulmuş : Yeni Bir Neden)"), Some("durd_tanimsiz"));
        assert_eq!(durdurma_kesinlesme_kodu("Açık"), None);
        assert_eq!(durdurma_kesinlesme_kodu("Kapalı"), None);
    }

    #[test]
    fn mazbata_sonuclari() {
        assert_eq!(mazbata_sonucu("Normal Tebligat[BORÇLU X] - Tebliğ Edildi"), MazbataSonucu::Edildi);
        assert_eq!(mazbata_sonucu("Normal Tebligat[BORÇLU X] - TEBLİĞ EDİLEMEDİ"), MazbataSonucu::Edilemedi);
        assert_eq!(mazbata_sonucu("Normal Tebligat[BORÇLU TEBLİĞ EDİLDİ A.Ş.]"), MazbataSonucu::Yok, "ad içindeki ifade sayılmaz");
        assert_eq!(mazbata_sonucu("Tebliğ Edildi"), MazbataSonucu::Edildi);
        assert_eq!(mazbata_sonucu(""), MazbataSonucu::Yok);
    }

    #[test]
    fn tebligat_turune_gore_kesinlesme() {
        use TebligatKesinlesmesi::*;
        assert_eq!(tebligat_kesinlesmesi("Ödeme Emri"), Borclu);
        assert_eq!(tebligat_kesinlesmesi("Tahliye Emri"), Borclu);
        assert_eq!(tebligat_kesinlesmesi("103 Davetiyesi"), SabitGun(7));
        assert_eq!(tebligat_kesinlesmesi("Kıymet Takdiri Raporu"), SabitGun(7));
        for tur in ["Maaş", "Maaş (Haciz İhbarnamesi)", "Maaş (Haciz Müzekkeresi)", "Banka (Haciz İhbarnamesi)", "Haciz İhbarnamesi",
                    "Bilirkişi Raporu", "Satış İlanı", "Belirsiz", ""] {
            assert_eq!(tebligat_kesinlesmesi(tur), Yok, "{tur}");
        }
        // 103: 7 gün sabit, takip türünün süresi kullanılmaz (05.10 tebliğ + mazbata → 13.10 kesinleşir)
        let bos = Tatiller::new();
        let davetiye = fiziki("TESLİM EDİLDİ", Some(g(2026, 10, 5)), "Normal Tebligat[BORÇLU X] - Tebliğ Edildi");
        assert_eq!(
            borclu_kesinlesme_hesapla(false, &[davetiye], &bos, g(2026, 10, 13), None, Some(SABIT_KESINLESME_GUN)),
            ("kesinlesti".into(), Some(g(2026, 10, 13)))
        );
    }

    #[test]
    fn kesinlesme_kurallari() {
        let bos = Tatiller::new();
        let h = |itiraz, oe: &[OdemeEmri], bugun, durdurma, sure| borclu_kesinlesme_hesapla(itiraz, oe, &bos, bugun, durdurma, sure);
        let teslim = fiziki("TESLİM EDİLDİ", Some(g(2026, 10, 5)), "Normal Tebligat[BORÇLU X]");
        let mazbatali = fiziki("TESLİM EDİLDİ", Some(g(2026, 10, 5)), "Normal Tebligat[BORÇLU X] - Tebliğ Edildi");
        assert_eq!(h(true, &[mazbatali.clone()], g(2026, 11, 1), Some("durd_iflas"), Some(7)), ("itiraz".into(), None), "elle itiraz her şeyden önce");
        assert_eq!(h(false, &[mazbatali.clone()], g(2026, 11, 1), Some("durd_iflas"), Some(7)), ("durd_iflas".into(), None));
        assert_eq!(h(false, &[], g(2026, 11, 1), None, Some(7)), (String::new(), None), "ödeme emri yok");
        assert_eq!(h(false, &[mazbatali.clone()], g(2026, 11, 1), None, None), (String::new(), None), "süre tanımlı değil");
        assert_eq!(h(false, &[fiziki("DAĞITIMDA", Some(g(2026, 10, 3)), "x")], g(2026, 11, 1), None, Some(7)), ("kesinlesme_bekliyor".into(), None));
        assert_eq!(h(false, &[teslim.clone()], g(2026, 11, 1), None, Some(7)), ("kesinlesme_bekliyor".into(), Some(g(2026, 10, 13))), "mazbata dönmeden kesinleşmez");
        assert_eq!(h(false, &[mazbatali.clone()], g(2026, 10, 12), None, Some(7)), ("kesinlesme_bekliyor".into(), Some(g(2026, 10, 13))));
        assert_eq!(h(false, &[mazbatali.clone()], g(2026, 10, 13), None, Some(7)), ("kesinlesti".into(), Some(g(2026, 10, 13))));
        // Mazbata açıklamasındaki barkod da kanıttır; başka barkodlu mazbata değildir; bağlanmış barkodsuz "Var" kanıttır
        let mut barkodlu = teslim.clone();
        barkodlu.tebligat_mazbatasi_var = true;
        barkodlu.tebligat_mazbatasi_aciklama = "Ödeme Emri - [X] - Barkod Numarası: 4007663523293 - Tebliğ Edildi".into();
        assert_eq!(h(false, &[barkodlu.clone()], g(2026, 10, 13), None, Some(7)).0, "kesinlesti");
        barkodlu.tebligat_mazbatasi_aciklama = "Ödeme Emri - [X] - Barkod Numarası: 4007000000000 - Tebliğ Edildi".into();
        assert_eq!(h(false, &[barkodlu.clone()], g(2026, 10, 13), None, Some(7)).0, "kesinlesme_bekliyor", "başka barkodun mazbatası");
        barkodlu.tebligat_mazbatasi_aciklama = "Var".into();
        assert_eq!(h(false, &[barkodlu], g(2026, 10, 13), None, Some(7)).0, "kesinlesti", "2026/7574");
        // Tebliğ edilemedi dönmüşse PTT "teslim edildi" dese de sayılmaz
        let edilemedi = fiziki("TESLİM EDİLDİ", Some(g(2026, 10, 5)), "Normal Tebligat[BORÇLU X] - Tebliğ Edilemedi");
        assert_eq!(h(false, &[edilemedi], g(2026, 11, 1), None, Some(7)), ("kesinlesme_bekliyor".into(), None));
        // İki ödeme emri: mazbatası dönmüş eski tebliğ kesinleştirir
        let yeni_teslim = fiziki("TESLİM EDİLDİ", Some(g(2026, 10, 20)), "Normal Tebligat[BORÇLU X]");
        assert_eq!(h(false, &[mazbatali.clone(), yeni_teslim.clone()], g(2026, 10, 21), None, Some(7)), ("kesinlesti".into(), Some(g(2026, 10, 13))));
        assert_eq!(h(false, &[yeni_teslim], g(2026, 10, 21), None, Some(7)), ("kesinlesme_bekliyor".into(), Some(g(2026, 10, 28))));
        // E-tebligat: tebliğ = evrak tarihi, süre 5 gün sonra başlar, mazbata beklenmez
        let e = OdemeEmri { elektronik_tebligat: true, son_islem_gunu: Some(g(2026, 10, 1)), evrak_gunu: Some(g(2026, 10, 1)), ..Default::default() };
        assert_eq!(e.sure_baslangici(), Some(g(2026, 10, 6)));
        assert_eq!(h(false, &[e.clone()], g(2026, 10, 5), None, Some(7)), ("kesinlesme_bekliyor".into(), None), "okuma süresi dolmadı");
        assert_eq!(h(false, &[e.clone()], g(2026, 10, 6), None, Some(7)), ("kesinlesme_bekliyor".into(), Some(g(2026, 10, 14))));
        assert_eq!(h(false, &[e], g(2026, 10, 14), None, Some(7)), ("kesinlesti".into(), Some(g(2026, 10, 14))));
        // Ham metinden tarih (türetilmiş alan boşsa)
        let ham = OdemeEmri { ptt_durumu: "TESLİM EDİLDİ".into(), son_islem_tarihi: "05/10/2026 14:30".into(), ..Default::default() };
        assert_eq!(ham.son_islem(), Some(g(2026, 10, 5)));
    }

    #[test]
    fn tebligat_durumu() {
        let h = |oe: &[OdemeEmri]| borclu_tebligat_hesapla(oe, g(2026, 10, 10));
        assert_eq!(h(&[]), (String::new(), None, String::new()));
        let mut teslim = fiziki("TESLİM EDİLDİ", Some(g(2026, 10, 5)), "x");
        teslim.teslim_sekli = "Muhataba bizzat teslim".into();
        assert_eq!(h(&[teslim.clone()]), ("teblig_edildi".into(), Some(g(2026, 10, 5)), "Muhataba bizzat teslim".into()));
        let iade = fiziki("İADE EDİLDİ", Some(g(2026, 10, 7)), "x");
        assert_eq!(h(&[iade.clone(), teslim.clone()]).0, "teblig_edildi", "biri tebliğ edildiyse tebliğ edildi");
        assert_eq!(h(&[fiziki("DAĞITIMDA", None, "x - Tebliğ Edildi")]), ("teblig_edildi".into(), None, String::new()), "mazbatadan, tarihsiz");
        let mut iade_nedenli = iade.clone();
        iade_nedenli.teslim_sekli = "ADRESTE TANINMIYOR".into();
        assert_eq!(h(&[iade_nedenli]), ("iade".into(), Some(g(2026, 10, 7)), "ADRESTE TANINMIYOR".into()));
        assert_eq!(h(&[fiziki("DAĞITIMDA", None, "x - Tebliğ Edilemedi")]).0, "iade");
        assert_eq!(h(&[fiziki("DAĞITIMDA", Some(g(2026, 10, 3)), "x")]), ("dagitimda".into(), None, String::new()));
        assert_eq!(h(&[fiziki("Kayıt yok", None, "x")]).0, "");
        assert_eq!(h(&[fiziki("", None, "x")]).0, "");
        // Evrak tarihi en yeni olan belirler: eski iade + yeni dağıtımda → dağıtımda
        let mut yeni = fiziki("DAĞITIMDA", None, "x");
        yeni.evrak_gunu = Some(g(2026, 10, 8));
        assert_eq!(h(&[iade, yeni]).0, "dagitimda");
        // E-tebligat: günü gelmeden dağıtımda, gelince tebliğ edildi
        let e = OdemeEmri { elektronik_tebligat: true, son_islem_gunu: Some(g(2026, 10, 6)), ..Default::default() };
        assert_eq!(h(&[e.clone()]).0, "dagitimda");
        assert_eq!(borclu_tebligat_hesapla(&[e], g(2026, 10, 11)), ("teblig_edildi".into(), Some(g(2026, 10, 6)), "E-tebligat".into()));
    }

    #[test]
    fn takip_asamalari() {
        let b = |kes: &str, teb: &str| BorcluDurumu {
            kesinlesme_durumu: kes.into(),
            kesinlesme_tarihi: Some(g(2026, 10, 13)),
            tebligat_durumu: teb.into(),
            tebligat_durum_tarihi: Some(g(2026, 10, 5)),
            itiraz_tarihi: Some(g(2026, 10, 8)),
            ..Default::default()
        };
        let oe = [fiziki("DAĞITIMDA", Some(g(2026, 10, 3)), "x")];
        let h = |d: &BorcluDurumu, oe: &[OdemeEmri]| takip_asamasi_hesapla(d, oe, None);
        assert_eq!(h(&b("kesinlesti", "teblig_edildi"), &oe), (A::Kesinlesti, Some(g(2026, 10, 13))));
        assert_eq!(h(&b("itiraz", "teblig_edildi"), &oe), (A::Itiraz, Some(g(2026, 10, 8))));
        assert_eq!(h(&b("durd_iflas", "teblig_edildi"), &oe), (A::Durduruldu, None));
        assert_eq!(h(&b("kesinlesme_bekliyor", "teblig_edildi"), &oe), (A::Teblig, Some(g(2026, 10, 5))));
        assert_eq!(h(&b("kesinlesme_bekliyor", "iade"), &oe), (A::Iade, Some(g(2026, 10, 5))));
        assert_eq!(h(&b("konkordato", "dagitimda"), &oe), (A::TebligeCikti, Some(g(2026, 10, 3))));
        assert_eq!(h(&b("", ""), &[]), (A::TakipTalebi, None));
        assert_eq!(takip_asamasi_hesapla(&b("", ""), &[], Some(Some(g(2026, 10, 1)))), (A::OdemeEmri, Some(g(2026, 10, 1))));
        assert_eq!(takip_asamasi_hesapla(&b("", ""), &[], Some(None)), (A::OdemeEmri, None));
        let mut k = fiziki("", None, "x");
        k.barkod = " ".into();
        assert_eq!(h(&b("", ""), &[k.clone()]), (A::TebligatOlusturuluyor, Some(g(2026, 10, 1))));
        k.barkod = "4007663523293".into();
        assert_eq!(h(&b("", ""), &[k.clone()]), (A::PttVeriliyor, Some(g(2026, 10, 1))));
        k.ptt_durumu = "Kayıt yok".into();
        assert_eq!(h(&b("", ""), &[k.clone()]).0, A::PttVeriliyor);
        k.ptt_durumu = "KABUL EDİLDİ".into();
        k.son_islem_gunu = Some(g(2026, 10, 2));
        assert_eq!(h(&b("", "dagitimda"), &[k.clone()]), (A::PttVeriliyor, Some(g(2026, 10, 2))));
        k.elektronik_tebligat = true;
        assert_eq!(h(&b("", "dagitimda"), &[k]), (A::TebligeCikti, Some(g(2026, 10, 1))));
        // En yeni ödeme emri belirler
        let mut yeni = fiziki("", None, "x");
        yeni.birim_evrak_no = 20;
        yeni.evrak_gunu = Some(g(2026, 10, 9));
        assert_eq!(h(&b("", ""), &[oe[0].clone(), yeni]), (A::PttVeriliyor, Some(g(2026, 10, 9))));
    }

    #[test]
    fn metinler() {
        assert_eq!(kesinlesme_metni("kesinlesme_bekliyor", Some(g(2026, 10, 14))), "Kesinleşme Bekliyor (14.10.2026)");
        assert_eq!(kesinlesme_metni("durd_iflas", None), "Durdurulmuş: İflas");
        assert_eq!(kesinlesme_metni("", Some(g(2026, 10, 14))), "");
        assert_eq!(tebligat_metni("teblig_edildi", Some(g(2026, 9, 12)), "Muhtara teslim (21. md.)"), "Tebliğ Edildi (12.09.2026) — Muhtara teslim (21. md.)");
        assert_eq!(tebligat_metni("dagitimda", None, ""), "Dağıtımda");
        assert_eq!(takip_asamasi_metni("teblig", Some(g(2026, 10, 5))), "Tebliğ (05.10.2026)");
        assert_eq!(takip_asamasi_metni("", None), "");
    }
}
