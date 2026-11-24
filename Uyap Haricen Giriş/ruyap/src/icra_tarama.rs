//! Dosya başına tebligat taramasının karar akışı (Aşama 3, çevrimdışı yarı).
//!
//! Bir dosyanın evrak listesi elde olduğunda her tebligat evrakı için sırayla şu sorulur:
//! PDF indirilmeli mi, PTT sorgulanmalı mı, ne kaydedilmeli? Bu modül yalnız bu kararları verir
//! ve sonucu yazar; UYAP'a ya da PTT'ye kendisi gitmez. İstekleri atan katman (sonraki adım)
//! her evrak için [`evrak_baslat`] → gerekirse [`pdf_geldi`] → gerekirse [`ptt_geldi`] →
//! [`kaydet`] sırasını izler. Böylece akışın tamamı örnek veriyle sınanır.
//!
//! Gereksiz istek atılmaz (örnek koddaki kurallar):
//!   • barkodu ve türü bilinen evrakın PDF'i bir daha indirilmez
//!   • PTT sonucu kesinleşmiş (teslim / iade) ya da mazbatası dosyaya dönmüş barkod sorulmaz
//!   • tebliğ tarihi kayıtlı elektronik tebligat için hiçbir şey indirilmez
//!   • MTS dosyalarında aynı barkodu taşıyan ikinci evrak ayrı satır açmaz, sonraki turda atlanır

use crate::icra_durum as durum;
use crate::icra_kayit::{self as kayit, BilinenTebligat, TebligatKaydi};
use crate::icra_tebligat::{self as teb, Mazbatalar};
use postgres::GenericClient;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Bir dosyanın taraması boyunca değişmeyen bilgiler.
pub struct DosyaBaglami {
    pub dosya: i64,
    pub mazbatalar: Mazbatalar,
    /// Dosyada "Kapalı Tebliğ Mazbatası" (taranmış fiziki mazbata) var mı?
    pub kapali_mazbata_var: bool,
    /// Avukat portalından yeniden tebliğ talebi gönderilmiş mi?
    pub yeniden_teblig_talebi: bool,
    /// Borçlu bağları: (bağ kimliği, görünen ad).
    pub borclular: Vec<(i64, String)>,
    pub eslesmeler: HashMap<String, String>,
    pub kategoriler: HashSet<String>,
    mts_mukerrer: HashSet<String>,
    /// barkod → (evrak no, tür): MTS'te aynı barkodlu ikinci evrağı tanımak için.
    dosya_barkodlari: HashMap<String, (i32, String)>,
}

/// Evrak listesindeki tebligat evrakları (barkod taşıyanlar).
pub fn hedef_evraklar(evraklar: &[Value]) -> Vec<&Value> {
    evraklar.iter().filter(|e| matches!(teb::metin(e, "tur").as_str(), teb::HEDEF_TUR | teb::MTS_TEBLIGAT_TUR)).collect()
}

/// Dosyanın evrak listesinden ve kayıtlı verisinden tarama bağlamını kurar. Taraflar bundan
/// önce yazılmış olmalıdır (tebligat → borçlu eşleşmesi borçlu listesine bakar).
pub fn dosya_baglami<C: GenericClient>(c: &mut C, dosya: i64, evraklar: &[Value]) -> Result<DosyaBaglami, String> {
    let ktm = teb::tur_anahtar(teb::KAPALI_TEBLIG_MAZBATASI_TUR);
    let mut talep = evraklar.iter().any(|e| teb::metin(e, "tur") == teb::TALEP_EVRAK_TUR);
    let mut dosya_barkodlari = HashMap::new();
    for s in c
        .query(
            "SELECT barkod, birim_evrak_no, tebligat_turu, yeniden_teblig_talep_edildi FROM tebligat_barkod
              WHERE dosya_id = $1 ORDER BY id",
            &[&dosya],
        )
        .map_err(|e| format!("veritabanı: {e}"))?
    {
        let barkod: String = s.get(0);
        if !barkod.is_empty() {
            dosya_barkodlari.entry(barkod).or_insert((s.get::<_, i32>(1), s.get::<_, String>(2)));
        }
        // Talep evrakı listede artık görünmese de bir kez görüldüyse işaret kalır.
        talep |= s.get::<_, bool>(3);
    }
    let mut mazbatalar = Mazbatalar::kur(evraklar);
    mazbatalar.elle_bagla(&kayit::elle_mazbata_baglari(c, dosya)?, evraklar);
    mazbatalar.barkodlulari_dus(dosya_barkodlari.iter().map(|(b, (no, _))| (b, *no)));
    Ok(DosyaBaglami {
        dosya,
        mazbatalar,
        kapali_mazbata_var: evraklar.iter().any(|e| teb::tur_anahtar(&teb::metin(e, "tur")) == ktm),
        yeniden_teblig_talebi: talep,
        borclular: kayit::dosya_borclulari(c, dosya)?,
        eslesmeler: kayit::tur_eslesmeleri(c)?,
        kategoriler: kayit::tur_kategorileri(c)?.into_iter().collect(),
        mts_mukerrer: kayit::mts_mukerrerler(c)?,
        dosya_barkodlari,
    })
}

/// Kayıtlı evrak listesinden (UYAP'a gitmeden) dosyanın bütün tebligatlarının mazbata eşleşmesini yeniden kurar
/// ve durumları yeniden hesaplar. Elle mazbata bağı kurulunca / kaldırılınca çağrılır. Döner: değişen tebligat sayısı.
pub fn mazbatalari_yeniden_bagla<C: GenericClient>(c: &mut C, dosya: i64, bugun: chrono::NaiveDate) -> Result<usize, String> {
    let hata = |e: postgres::Error| format!("veritabanı: {e}");
    let evraklar: Vec<Value> = c
        .query(
            "SELECT birim_evrak_no, evrak_turu, aciklama, COALESCE(to_char(evrak_tarihi, 'DD/MM/YYYY'), '') FROM evrak
              WHERE dosya_id = $1 ORDER BY birim_evrak_no",
            &[&dosya],
        )
        .map_err(hata)?
        .iter()
        .map(|s| {
            serde_json::json!({"birimEvrakNo": s.get::<_, i32>(0), "tur": s.get::<_, String>(1), "aciklama": s.get::<_, String>(2),
                               "onaylandigiTarih": s.get::<_, String>(3)})
        })
        .collect();
    let tebligatlar: Vec<(i64, i32, String)> = c
        .query("SELECT id, birim_evrak_no, barkod FROM tebligat_barkod WHERE dosya_id = $1 ORDER BY id", &[&dosya])
        .map_err(hata)?
        .iter()
        .map(|s| (s.get(0), s.get(1), s.get(2)))
        .collect();
    let mut m = Mazbatalar::kur(&evraklar);
    m.elle_bagla(&kayit::elle_mazbata_baglari(c, dosya)?, &evraklar);
    let bilinen: Vec<(String, i32)> = tebligatlar.iter().filter(|t| !t.2.is_empty()).map(|t| (t.2.clone(), t.1)).collect();
    m.barkodlulari_dus(bilinen.iter().map(|(b, no)| (b, *no)));
    let mut n = 0;
    for (id, no, barkod) in &tebligatlar {
        let mz = m.bul(barkod, *no);
        // Elle işlenmiş e-tebliğ mazbatası ("Kapalı E-Tebliğ Mazbatası …") evrak listesinde görünmez; silinmez (kayit ile aynı kural).
        n += c
            .execute(
                "UPDATE tebligat_barkod SET tebligat_mazbatasi_var = $2, tebligat_mazbatasi_aciklama = $3
                  WHERE id = $1 AND (tebligat_mazbatasi_var, tebligat_mazbatasi_aciklama) IS DISTINCT FROM ($2, $3)
                    AND NOT (NOT $2 AND tebligat_mazbatasi_var AND tebligat_mazbatasi_aciklama LIKE 'Kapalı E-Tebliğ Mazbatası%')",
                &[id, &mz.is_some(), &mz.clone().unwrap_or_default()],
            )
            .map_err(hata)? as usize;
    }
    durum::durumlari_guncelle(c, Some(dosya), bugun, &|_: &str| {})?;
    Ok(n)
}

/// Bir tebligat evrakı için yazılmaya hazır sonuç.
#[derive(Clone, Debug)]
pub struct Taslak {
    pub birim_evrak_no: i32,
    pub kayit: TebligatKaydi,
}

/// Bir evrak için sıradaki iş.
#[derive(Clone, Debug)]
pub enum Adim {
    /// Yapılacak bir şey yok (neden).
    Atla(String),
    /// Evrakın PDF'i indirilip [`pdf_geldi`] çağrılmalı.
    PdfGerek,
    /// Taslaktaki barkod PTT'de sorgulanıp [`ptt_geldi`] çağrılmalı.
    PttGerek(Taslak),
    /// Başka istek gerekmez; [`kaydet`] çağrılmalı.
    Kaydet(Taslak),
}

fn mts_anahtari(ctx: &DosyaBaglami, evrak: &Value, no: i32) -> Option<String> {
    (teb::metin(evrak, "tur") == teb::MTS_TEBLIGAT_TUR).then(|| format!("{}:{no}", ctx.dosya))
}

fn taslak(ctx: &DosyaBaglami, evrak: &Value, no: i32, barkod: &str, elektronik: bool, tur: String) -> Taslak {
    let aciklama = teb::metin(evrak, "aciklama");
    let mazbata = ctx.mazbatalar.bul(barkod, no);
    Taslak {
        birim_evrak_no: no,
        kayit: TebligatKaydi {
            borclu: teb::tebligat_borclusunu_bul(&aciklama, &ctx.borclular, None),
            evrak_aciklama: aciklama,
            evrak_tarihi: teb::metin(evrak, "onaylandigiTarih"),
            barkod: barkod.to_string(),
            elektronik_tebligat: elektronik,
            tebligat_turu: tur,
            tebligat_mazbatasi_var: mazbata.is_some(),
            tebligat_mazbatasi_aciklama: mazbata.unwrap_or_default(),
            kapali_tebligat_mazbatasi_var: ctx.kapali_mazbata_var,
            yeniden_teblig_talep_edildi: ctx.yeniden_teblig_talebi,
            ..Default::default()
        },
    }
}

/// Barkod PTT'ye sorulmamalı mı: sonuç kesin ya da mazbatası dönmüş. İSTİSNA: kesinleşme hesabına giren türde (ödeme /
/// tahliye emri, 103, kıymet takdiri) PTT'den tebliğ TARİHİ alınmamışsa mazbata dönmüş olsa da sorulur — mazbata
/// "Tebliğ Edildi" der ama tarihi vermez, süre sayılamaz ve dosya süresiz "Kesinleşme Bekliyor"da kalıyordu
/// (kullanıcı bulgusu 20.11.2026, İzmir 10. İcra 2026/8645; açık dosyalarda 4.667 ödeme emri).
fn ptt_gereksiz(bilinen: Option<&BilinenTebligat>, aciklama: &str, mazbata_var: bool, tur: &str) -> bool {
    if bilinen.is_some_and(|b| teb::ptt_sonuclandi(&b.ptt_durumu)) {
        return true;
    }
    let mazbata = teb::teblig_sonucu_aciklamada_mi(aciklama) || mazbata_var;
    mazbata && !tarih_gerekli(tur)
}

/// Kesinleşme bu tebligatın tebliğ tarihinden sayılıyor mu.
fn tarih_gerekli(tur: &str) -> bool {
    durum::tebligat_kesinlesmesi(tur) != durum::TebligatKesinlesmesi::Yok
}

fn e_teblig(t: &mut Taslak, evrak_tarihi: &str) {
    let tarih = teb::e_teblig_teblig_tarihi(evrak_tarihi);
    t.kayit.ptt_durumu = if tarih.is_empty() { teb::E_TEBLIG_TARIHSIZ_METNI } else { teb::E_TEBLIG_DURUM_METNI }.to_string();
    t.kayit.son_islem_tarihi = tarih;
    t.kayit.ham_yanit = None;
}

/// Evrakın ilk adımı. `ptt_sorgusu` kapalıysa PTT'ye hiç gidilmez (yalnız UYAP verisi).
pub fn evrak_baslat<C: GenericClient>(c: &mut C, ctx: &DosyaBaglami, evrak: &Value, ptt_sorgusu: bool) -> Result<Adim, String> {
    let Some(no) = kayit::birim_evrak_no(evrak) else { return Ok(Adim::Atla("Evrak numarası yok".into())) };
    if mts_anahtari(ctx, evrak, no).is_some_and(|a| ctx.mts_mukerrer.contains(&a)) {
        return Ok(Adim::Atla("MTS: barkodu bu dosyada başka evrakta kayıtlı".into()));
    }
    let Some(b) = kayit::bilinen_tebligat(c, ctx.dosya, no)?.filter(|b| !b.barkod.is_empty() && !b.tebligat_turu.is_empty())
    else {
        return Ok(Adim::PdfGerek);
    };
    // Zarf adresi (23.11.2026'dan önce okunmuyordu): kesinleşmeye giren türde fiziki tebligatın adresi yoksa PDF bir kez
    // daha indirilir (Mernis kıyası için).
    if !b.elektronik_tebligat && b.teblig_adresi.is_empty() && tarih_gerekli(&b.tebligat_turu) {
        return Ok(Adim::PdfGerek);
    }
    // Barkod ve tür biliniyor: PDF yeniden indirilmez.
    let mut tur = b.tebligat_turu.clone();
    if !teb::TEMEL_TURLER.contains(&tur.as_str()) {
        tur = teb::tur_eslestir(&tur, &ctx.eslesmeler).unwrap_or(tur);
    }
    let aciklama = teb::metin(evrak, "aciklama");
    let mut t = taslak(ctx, evrak, no, &b.barkod, b.elektronik_tebligat, tur);
    t.kayit.ptt_durumu = b.ptt_durumu.clone();
    t.kayit.son_islem_tarihi = b.son_islem_tarihi.clone();
    if b.elektronik_tebligat {
        if !teb::e_teblig_sonuclandi(&b.ptt_durumu, &b.son_islem_tarihi) {
            let evrak_tarihi = teb::metin(evrak, "onaylandigiTarih");
            e_teblig(&mut t, if evrak_tarihi.is_empty() { &b.evrak_tarihi } else { &evrak_tarihi });
        }
        // Kayıtlı PTT yanıtı yoktur; durum ve tarih olduğu gibi yazılır.
        return Ok(Adim::Kaydet(t));
    }
    if !ptt_sorgusu {
        t.kayit.ham_yanit = Some(teb::ptt_atlandi());
        return Ok(Adim::Kaydet(t));
    }
    if ptt_gereksiz(Some(&b), &aciklama, t.kayit.tebligat_mazbatasi_var, &t.kayit.tebligat_turu) {
        t.kayit.ham_yanit = Some(teb::ptt_sonuclanmis());
        return Ok(Adim::Kaydet(t));
    }
    Ok(Adim::PttGerek(t))
}

/// PDF indirildikten sonraki adım: barkod ve tür çıkarılır.
pub fn pdf_geldi<C: GenericClient>(
    c: &mut C,
    ctx: &mut DosyaBaglami,
    evrak: &Value,
    pdf: &[u8],
    ptt_sorgusu: bool,
) -> Result<Adim, String> {
    let Some(no) = kayit::birim_evrak_no(evrak) else { return Ok(Adim::Atla("Evrak numarası yok".into())) };
    let bilinen = kayit::bilinen_tebligat(c, ctx.dosya, no)?;
    let barkod = match bilinen.as_ref().map(|b| b.barkod.clone()).filter(|b| !b.is_empty()) {
        Some(b) => b,
        None => match teb::barkod_bul(pdf) {
            Some(b) => b,
            None if !pdf.starts_with(b"%PDF") => return Ok(Adim::Atla(format!("Beklenmeyen içerik (PDF değil, {} bayt)", pdf.len()))),
            None => return Ok(Adim::Atla("PDF içinde barkod bulunamadı".into())),
        },
    };
    let aciklama = teb::metin(evrak, "aciklama");
    let govde = teb::pdf_govde_metni(pdf);
    let mut tur = teb::tebligat_turunu_belirle(&aciklama, Some(&govde), &ctx.eslesmeler);
    if tur == teb::BELIRSIZ {
        // Zarf içeriği okunamadıysa kendi mazbatasındaki belge adına bakılır.
        let belge = teb::mazbata_belge_adi(&ctx.mazbatalar.bul(&barkod, no).unwrap_or_default());
        tur = teb::tur_eslestir(&belge, &ctx.eslesmeler).unwrap_or(tur);
    }

    if let Some(anahtar) = mts_anahtari(ctx, evrak, no) {
        match ctx.dosya_barkodlari.get(&barkod).cloned() {
            Some((diger_no, diger_tur)) if diger_no != no => {
                // Aynı barkod bu dosyada başka evrakta: ikinci satır açılmaz; oradaki tür boş ya
                // da şablon dışıysa buradaki bilinen türle tamamlanır.
                if ctx.kategoriler.contains(&tur) && !ctx.kategoriler.contains(&diger_tur) {
                    c.execute(
                        "UPDATE tebligat_barkod SET tebligat_turu = $3 WHERE dosya_id = $1 AND birim_evrak_no = $2",
                        &[&ctx.dosya, &diger_no, &tur],
                    )
                    .map_err(|e| format!("veritabanı: {e}"))?;
                    ctx.dosya_barkodlari.insert(barkod.clone(), (diger_no, tur));
                }
                // Bu evrakta muhatap yazıyor ("[BORÇLU VE MÜFLİS X]"), tutulan kayıtta yazmıyorsa ("PTT MTS Tebligat
                // Belgesi") muhatap tutulan kayda geçer; borçlu bir sonraki hesapta bulunur (kullanıcı bulgusu 19.11.2026).
                if teb::muhatap_turu(&aciklama) != teb::MuhatapTuru::Yok {
                    c.execute(
                        "UPDATE tebligat_barkod SET evrak_aciklama = $3 WHERE dosya_id = $1 AND birim_evrak_no = $2
                            AND position('[' in evrak_aciklama) = 0",
                        &[&ctx.dosya, &diger_no, &crate::icra_veri::kirp(&aciklama, 500)],
                    )
                    .map_err(|e| format!("veritabanı: {e}"))?;
                }
                kayit::mts_mukerrer_ekle(c, &anahtar)?;
                ctx.mts_mukerrer.insert(anahtar);
                return Ok(Adim::Atla(format!("MTS: {barkod} bu dosyada evrak {diger_no}'de zaten var")));
            }
            Some(_) => {}
            None => {
                ctx.dosya_barkodlari.insert(barkod.clone(), (no, tur.clone()));
            }
        }
    }

    let elektronik = aciklama.contains(teb::ELEKTRONIK_ANAHTAR);
    let mut t = taslak(ctx, evrak, no, &barkod, elektronik, tur);
    t.kayit.zarf_icerigi = teb::zarf_icerigi_oku(&govde).unwrap_or_default();
    t.kayit.haciz_sirasi = teb::haciz_sirasi_bul(&govde);
    if !elektronik {
        // Okunamazsa "-": PDF her taramada yeniden indirilmesin.
        t.kayit.teblig_adresi = crate::icra_mernis::zarf_adres_blogu(&govde)
            .map(|a| crate::icra_veri::kirp(&a, 2000))
            .unwrap_or_else(|| crate::icra_mernis::ADRES_OKUNAMADI.to_string());
    }
    if elektronik {
        e_teblig(&mut t, &teb::metin(evrak, "onaylandigiTarih"));
        return Ok(Adim::Kaydet(t));
    }
    if ptt_gereksiz(bilinen.as_ref(), &aciklama, t.kayit.tebligat_mazbatasi_var, &t.kayit.tebligat_turu) {
        t.kayit.ham_yanit = Some(teb::ptt_sonuclanmis());
        return Ok(Adim::Kaydet(t));
    }
    if !ptt_sorgusu {
        t.kayit.ham_yanit = Some(teb::ptt_atlandi());
        return Ok(Adim::Kaydet(t));
    }
    Ok(Adim::PttGerek(t))
}

/// PTT yanıtı geldikten sonra taslağı tamamlar. Yanıt hata taşıyorsa ([`kayit::tebligat_kaydet`])
/// kayıtlı PTT durumu korunur. Açıklamasında muhatap olmayan (MTS) tebligatta borçlu, PTT
/// yanıtındaki alıcının baş harflerinden bulunur.
pub fn ptt_geldi(ctx: &DosyaBaglami, mut t: Taslak, ptt_veri: Value) -> Taslak {
    let (durum, tarih) = teb::ptt_durum_ve_tarih(&ptt_veri);
    if t.kayit.borclu.is_none() {
        t.kayit.borclu = teb::tebligat_borclusunu_bul(&t.kayit.evrak_aciklama, &ctx.borclular, Some(&ptt_veri));
    }
    t.kayit.ptt_durumu = durum;
    t.kayit.son_islem_tarihi = tarih;
    t.kayit.ham_yanit = Some(ptt_veri);
    t
}

/// Taslağı veritabanına yazar; kaydın kimliğini döner.
pub fn kaydet<C: GenericClient>(c: &mut C, ctx: &DosyaBaglami, t: &Taslak) -> Result<i64, String> {
    kayit::tebligat_kaydet(c, ctx.dosya, t.birim_evrak_no, &t.kayit)
}

// ── Testler ─────────────────────────────────────────────────────────────────────────────────────

/// Sınama sürücüsü: gerçek katmanın izleyeceği sırayı örnek PDF ve PTT yanıtlarıyla yürütür.
/// Döner: (indirilen PDF sayısı, PTT sorgu sayısı, atlananların nedenleri).
#[cfg(test)]
fn ornekle_isle(
    c: &mut postgres::Client,
    dosya: i64,
    evraklar: &[Value],
    pdf: &dyn Fn(&str) -> Vec<u8>,
    ptt: &dyn Fn(&str) -> Value,
    ptt_sorgusu: bool,
) -> (usize, usize, Vec<String>) {
    let mut ctx = dosya_baglami(c, dosya, evraklar).unwrap();
    let (mut pdf_n, mut ptt_n, mut atlanan) = (0, 0, Vec::new());
    for evrak in hedef_evraklar(evraklar) {
        let mut adim = evrak_baslat(c, &ctx, evrak, ptt_sorgusu).unwrap();
        if matches!(adim, Adim::PdfGerek) {
            pdf_n += 1;
            adim = pdf_geldi(c, &mut ctx, evrak, &pdf(&teb::metin(evrak, "evrakId")), ptt_sorgusu).unwrap();
        }
        let t = match adim {
            Adim::Atla(neden) => {
                atlanan.push(neden);
                continue;
            }
            Adim::PttGerek(t) => {
                ptt_n += 1;
                let yanit = ptt(&t.kayit.barkod);
                ptt_geldi(&ctx, t, yanit)
            }
            Adim::Kaydet(t) => t,
            Adim::PdfGerek => unreachable!(),
        };
        kaydet(c, &ctx, &t).unwrap();
    }
    (pdf_n, ptt_n, atlanan)
}

/// Gerçek veritabanı üzerinde senaryo; `veritabani::tests::uctan_uca` içinden çağrılır.
#[cfg(test)]
pub(crate) fn veritabani_senaryosu(c: &mut postgres::Client) {
    use crate::icra_veri::{dosya_kunyesi_kaydet, YargiTuru};
    use chrono::NaiveDate;
    use serde_json::json;
    let say = |c: &mut postgres::Client, sql: &str| -> i64 { c.query_one(sql, &[]).unwrap().get(0) };
    let sessiz = |_: &str| {};
    let kunye = |no: &str, birim: i64| {
        json!({"dosyaId": format!("oturum-{no}"), "dosyaNo": no, "dosyaDurumKod": 0, "dosyaDurum": "Açık", "dosyaTurKod": 35,
               "dosyaTur": "İcra Dosyası", "birimAdi": "İzmir 1. İcra Dairesi", "birimId": birim})
    };
    let (d1, _) = dosya_kunyesi_kaydet(c, &kunye("2026/5", 1001), Some(YargiTuru::Icra.kod())).unwrap();
    let (d2, _) = dosya_kunyesi_kaydet(c, &kunye("2026/6", 1001), None).unwrap();

    // ── Taraflar ──
    let taraflar = vec![
        json!({"adi": "X BANKASI A.Ş.", "rol": "Alacaklı", "kisiKurum": "Kurum", "vekil": "[HAKAN TOLUNAY BURHAN, AYŞE KAYA]"}),
        json!({"adi": "AHMET YILMAZ", "rol": "Borçlu", "kisiKurum": "Kişi", "vekil": ""}),
        json!({"adi": "FATMA BİROLOĞLU", "rol": "Borçlu", "kisiKurum": "Kişi"}),
        json!({"rol": "Borçlu"}),
    ];
    assert_eq!(kayit::dosya_taraf_kaydet(c, d1, &taraflar, &sessiz).unwrap(), 3);
    assert_eq!(kayit::dosya_taraf_kaydet(c, d1, &taraflar, &sessiz).unwrap(), 3, "yeniden yazınca çoğalmaz");
    assert_eq!(say(c, "SELECT count(*) FROM taraf"), 3);
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf"), 3);
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf WHERE rol = 'alacakli' AND sira = 0"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM vekil"), 2);
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf_vekiller"), 2);
    // Vekil listesi değişince bağlar güncellenir
    let mut tek_vekil = taraflar.clone();
    tek_vekil[0]["vekil"] = json!("[AYŞE KAYA]");
    kayit::dosya_taraf_kaydet(c, d1, &tek_vekil, &sessiz).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf_vekiller"), 1);

    // Borçlu kimlikleri: adı tutan kimliksiz borçluya yazılır, yeni taraf açılmaz
    assert!(kayit::borclu_kimligi_eksik_mi(c, d1).unwrap());
    let borclular = vec![
        json!({"kisiTumDVO": {"adi": "AHMET", "soyadi": "YILMAZ", "tcKimlikNo": "11111111110"}}),
        json!({"kisiTumDVO": {"adi": "FATMA", "soyadi": "BİROLOĞLU", "tcKimlikNo": "***********"}}),
    ];
    assert_eq!(kayit::borclu_kimliklerini_kaydet(c, d1, &borclular, &sessiz).unwrap(), 2);
    assert_eq!(say(c, "SELECT count(*) FROM taraf"), 3);
    assert_eq!(say(c, "SELECT count(*) FROM taraf WHERE tckn = '11111111110' AND ad = 'AHMET'"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM taraf WHERE soyad = 'BİROLOĞLU' AND tckn IS NULL"), 1, "maskeli kimlik yazılmaz");
    assert!(kayit::borclu_kimligi_eksik_mi(c, d1).unwrap());
    // Kimliksiz yanıt dolu kimliği silmez
    kayit::dosya_taraf_kaydet(c, d1, &tek_vekil, &sessiz).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM taraf WHERE tckn = '11111111110'"), 1);

    // İkinci dosyada aynı adlar: kayıtlı taraflar yeniden kullanılır
    assert_eq!(kayit::dosya_taraf_kaydet(c, d2, &taraflar[..2], &sessiz).unwrap(), 2);
    assert_eq!(say(c, "SELECT count(*) FROM taraf"), 3);
    // …ama borçlu listesi bu dosyadaki AHMET YILMAZ'ın başka biri (adaş) olduğunu söylüyor:
    // bağ yeni tarafa taşınır, bağ satırı aynı kalır, geçmişe yazılır
    let bag_once: i64 = c.query_one("SELECT id FROM dosya_taraf WHERE dosya_id = $1 AND rol = 'borclu'", &[&d2]).unwrap().get(0);
    let adas = vec![json!({"kisiTumDVO": {"adi": "AHMET", "soyadi": "YILMAZ", "tcKimlikNo": "22222222220"}})];
    kayit::borclu_kimliklerini_kaydet(c, d2, &adas, &sessiz).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM taraf WHERE ad = 'AHMET'"), 2);
    let s = c
        .query_one("SELECT dt.id, t.tckn FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE dt.dosya_id = $1 AND dt.rol = 'borclu'", &[&d2])
        .unwrap();
    assert_eq!((s.get::<_, i64>(0), s.get::<_, Option<String>>(1).as_deref()), (bag_once, Some("22222222220")));
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi WHERE tur = 'taraf_kimlik' AND yeni LIKE '%TCKN 22222222220%'"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE t.tckn = '11111111110'"), 1);

    // UYAP'ta artık görünmeyen taraf silinmez, işaretlenir; yeniden görünürse işaret kalkar
    kayit::dosya_taraf_kaydet(c, d1, &tek_vekil[..2], &sessiz).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf WHERE uyapta_gorulmedi IS NOT NULL"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi WHERE tur = 'taraf_gorulmedi' AND taraf_adi = 'FATMA BİROLOĞLU'"), 1);
    kayit::dosya_taraf_kaydet(c, d1, &tek_vekil[..2], &sessiz).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi WHERE tur = 'taraf_gorulmedi'"), 1, "aynı fark ikinci kez yazılmaz");
    kayit::dosya_taraf_kaydet(c, d1, &tek_vekil, &sessiz).unwrap();
    assert_eq!(say(c, "SELECT count(*) FROM dosya_taraf WHERE uyapta_gorulmedi IS NOT NULL"), 0);
    assert_eq!(say(c, "SELECT count(*) FROM durum_olayi WHERE tur = 'taraf_geri_geldi'"), 1);

    // ── Dosya ayrıntısı ──
    let ayrinti = json!({"takibinTuru": "0", "takibinSekli": 7, "takibinYolu": "1", "alacakKalemToplamTutar": 3762.9, "vekaletUcreti": "450.00", "tahsilHarci": null});
    assert_eq!(kayit::dosya_ayrinti_kaydet(c, d1, &ayrinti).unwrap(), Some("icra"));
    assert_eq!(kayit::dosya_ayrinti_kaydet(c, d1, &json!({"errorCode": "PRTL_GNL_10001-4", "error": "Doğrulama hatası"})).unwrap(), None);
    assert_eq!(kayit::dosya_ayrinti_kaydet(c, d1, &json!({})).unwrap(), None);
    assert_eq!(
        say(c, "SELECT count(*) FROM icra_takip_detay WHERE takibin_turu = '0' AND takibin_sekli = '7' AND alacak_kalemi_toplam = 3762.90 AND vekalet_ucreti = 450 AND tahsil_harci = 0"),
        1,
        "hata zarfı kayıtlı veriyi ezmez"
    );

    // ── Evrak listesi ve tebligatlar ──
    let (barkod_a, barkod_b) = ("4007663523293", "5002864776768");
    let mut evraklar = vec![
        json!({"evrakId": "e9", "birimEvrakNo": 9, "tur": "Ödeme Emri", "aciklama": "Örnek No: 7", "onaylandigiTarih": "01/10/2026 09:00"}),
        json!({"evrakId": "e10", "birimEvrakNo": 10, "tur": "Kapalı Tebligat", "aciklama": "Normal Tebligat[BORÇLU AHMET YILMAZ]", "onaylandigiTarih": "01/10/2026"}),
        json!({"evrakId": "e11", "birimEvrakNo": "11", "tur": "Kapalı Tebligat", "aciklama": "Elektronik Tebligat[Tebligat Yapılacak 3. Taraf DENİZBANK A.Ş.]", "onaylandigiTarih": "02/10/2026"}),
        json!({"evrakId": "e12", "tur": "Kapalı Tebligat", "aciklama": "numarasız"}),
    ];
    assert_eq!(kayit::evrak_listesi_kaydet(c, d1, &evraklar).unwrap(), 3);
    assert_eq!(kayit::evrak_listesi_kaydet(c, d1, &evraklar).unwrap(), 3);
    assert_eq!(say(c, "SELECT count(*) FROM evrak"), 3);
    assert_eq!(say(c, "SELECT count(*) FROM evrak WHERE evrak_turu = 'Ödeme Emri' AND evrak_tarihi::timestamp = '2026-10-01 09:00' AND sha256 = ''"), 1);

    let pdf = |evrak_id: &str| match evrak_id {
        "e10" => teb::ornek_pdf(format!("(*{barkod_a}*) Tj (BU ZARFTA \\326deme \\335cra Emri VARDIR) Tj").as_bytes()),
        "e11" => teb::ornek_pdf(format!("(*{barkod_b}*) Tj (BU ZARFTA 89/1 Haciz \\335hbarnamesi VARDIR) Tj").as_bytes()),
        _ => b"<html>oturum</html>".to_vec(),
    };
    let dagitimda = |_: &str| json!({"errorState": true, "sondurum": {"son_durum_aciklama": "DAĞITIMDA", "teslim_tarihi": "0", "son_islem_tarihi": "20261003"}});
    // 1. tur: iki PDF, bir PTT sorgusu (e-tebligat PTT'ye sorulmaz)
    let (pdf_n, ptt_n, atlanan) = ornekle_isle(c, d1, &evraklar, &pdf, &dagitimda, true);
    assert_eq!((pdf_n, ptt_n, atlanan.len()), (2, 1, 1), "{atlanan:?}");
    let ahmet: i64 = c.query_one("SELECT dt.id FROM dosya_taraf dt JOIN taraf t ON t.id = dt.taraf_id WHERE dt.dosya_id = $1 AND t.ad = 'AHMET'", &[&d1]).unwrap().get(0);
    let s = c
        .query_one(
            "SELECT barkod, tebligat_turu, borclu_id, ptt_durumu, son_islem_zamani::date, evrak_gunu, elektronik_tebligat
               FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = 10",
            &[&d1],
        )
        .unwrap();
    assert_eq!(s.get::<_, String>(0), barkod_a);
    assert_eq!(s.get::<_, String>(1), "Ödeme Emri");
    assert_eq!(s.get::<_, Option<i64>>(2), Some(ahmet));
    assert_eq!(s.get::<_, String>(3), "DAĞITIMDA");
    assert_eq!(s.get::<_, Option<NaiveDate>>(4), NaiveDate::from_ymd_opt(2026, 10, 3));
    assert_eq!(s.get::<_, Option<NaiveDate>>(5), NaiveDate::from_ymd_opt(2026, 10, 1));
    assert!(!s.get::<_, bool>(6));
    let s = c
        .query_one("SELECT barkod, tebligat_turu, ptt_durumu, son_islem_tarihi, elektronik_tebligat, borclu_id FROM tebligat_barkod WHERE birim_evrak_no = 11", &[])
        .unwrap();
    assert_eq!((s.get::<_, String>(0).as_str(), s.get::<_, String>(1).as_str()), (barkod_b, "Banka (Haciz İhbarnamesi)"));
    assert_eq!((s.get::<_, String>(2).as_str(), s.get::<_, String>(3).as_str()), (teb::E_TEBLIG_DURUM_METNI, "02/10/2026"));
    assert!(s.get::<_, bool>(4) && s.get::<_, Option<i64>>(5).is_none());

    // 2. tur: PDF yok; PTT'ye ulaşılamıyor → kayıtlı durum korunur
    let ulasilamadi = |_: &str| json!({"_hata": "PTT'ye ulaşılamadı"});
    assert_eq!(ornekle_isle(c, d1, &evraklar, &pdf, &ulasilamadi, true).0, 0);
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE ptt_durumu = 'DAĞITIMDA' AND ham_yanit ? 'sondurum'"), 1);
    // PTT sorgusu kapalıyken hiç sorulmaz, kayıt değişmez
    let (pdf_n, ptt_n, atlanan) = ornekle_isle(c, d1, &evraklar, &pdf, &ulasilamadi, false);
    assert_eq!((pdf_n, ptt_n, atlanan), (0, 0, vec!["Evrak numarası yok".to_string()]));
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE ptt_durumu = 'DAĞITIMDA'"), 1);

    // 3. tur: teslim edildi → durum, teslim şekli, hareketler
    let teslim = |_: &str| {
        json!({"errorState": true, "sondurum": {"son_durum_aciklama": "TESLİM EDİLDİ", "teslim_tarihi": "20261005", "son_islem_tarihi": "20261005"},
               "hareketDongu": [{"tarih": "02/10/2026", "saat": "09:15", "aciklama": "KABUL EDİLDİ", "il": "İZMİR"},
                                {"tarih": "05/10/2026", "saat": "14:30", "aciklama": "TESLİM EDİLDİ", "islem_detay": "MUHATABA BİZZAT TESLİM / a* y*"}]})
    };
    assert_eq!(ornekle_isle(c, d1, &evraklar, &pdf, &teslim, true).1, 1);
    assert_eq!(
        say(c, "SELECT count(*) FROM tebligat_barkod WHERE ptt_durumu = 'TESLİM EDİLDİ' AND teslim_sekli = 'Muhataba bizzat teslim' AND son_islem_zamani::date = '2026-10-05'"),
        1
    );
    assert_eq!(say(c, "SELECT count(*) FROM ptt_hareket"), 2);
    // 4. tur: sonuç kesin → ne PDF ne PTT
    let (pdf_n, ptt_n, _) = ornekle_isle(c, d1, &evraklar, &pdf, &|_| panic!("PTT sorulmamalı"), true);
    assert_eq!((pdf_n, ptt_n), (0, 0));
    assert_eq!(say(c, "SELECT count(*) FROM ptt_hareket"), 2, "kayıtlı hareketler korunur");

    // 5. tur: mazbata dosyaya döndü, yeniden tebliğ talebi gönderildi
    let mazbata = format!("Ödeme Emri - [AHMET YILMAZ] - Barkod Numarası: {barkod_a} - Tebliğ Edildi");
    evraklar.push(json!({"evrakId": "e13", "birimEvrakNo": 13, "tur": "Tebliğ Mazbatası", "aciklama": mazbata}));
    evraklar.push(json!({"evrakId": "e14", "birimEvrakNo": 14, "tur": "Avukat Portal Tebligat Talebi", "aciklama": ""}));
    ornekle_isle(c, d1, &evraklar, &pdf, &|_| panic!("PTT sorulmamalı"), true);
    assert_eq!(
        say(c, "SELECT count(*) FROM tebligat_barkod WHERE birim_evrak_no = 10 AND tebligat_mazbatasi_var AND tebligat_mazbatasi_aciklama LIKE '%Tebliğ Edildi' AND yeniden_teblig_talep_edildi"),
        1
    );
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE birim_evrak_no = 11 AND NOT tebligat_mazbatasi_var AND yeniden_teblig_talep_edildi"), 1);
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE ptt_durumu = 'TESLİM EDİLDİ'"), 1, "sonuç korunur");
    // Talep evrakı listeden kalksa da işaret kalır
    evraklar.pop();
    ornekle_isle(c, d1, &evraklar, &pdf, &|_| panic!("PTT sorulmamalı"), true);
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE yeniden_teblig_talep_edildi"), 2);
    // Mazbata döndü ama PTT'den tebliğ tarihi hiç alınmamış ödeme emri: kesinleşme tarihten sayıldığı için PTT'ye sorulur
    // (İzmir 10. İcra 2026/8645 hatası); kesinleşmeye girmeyen türde sorulmaz.
    c.batch_execute("UPDATE tebligat_barkod SET ptt_durumu = '', son_islem_tarihi = '', ham_yanit = NULL WHERE birim_evrak_no = 10").unwrap();
    let tur10: String = c.query_one("SELECT tebligat_turu FROM tebligat_barkod WHERE birim_evrak_no = 10", &[]).unwrap().get(0);
    assert_eq!(tur10, "Ödeme Emri");
    let (_, ptt_n, _) = ornekle_isle(c, d1, &evraklar, &pdf, &teslim, true);
    assert_eq!(ptt_n, 1, "tarihi olmayan ödeme emri PTT'ye sorulur");
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE birim_evrak_no = 10 AND ptt_durumu = 'TESLİM EDİLDİ' AND son_islem_zamani::date = '2026-10-05'"), 1);
    c.batch_execute("UPDATE tebligat_barkod SET tebligat_turu = 'Banka', ptt_durumu = '', son_islem_tarihi = '', ham_yanit = NULL WHERE birim_evrak_no = 10").unwrap();
    ornekle_isle(c, d1, &evraklar, &pdf, &|_| panic!("kesinleşmeye girmeyen türde mazbata dönmüşse PTT sorulmaz"), true);

    // ── MTS: aynı barkodu taşıyan iki evrak tek satır olur; boş tür dolu olanla tamamlanır ──
    let barkod_m = "4100131175175";
    let mts = vec![
        json!({"evrakId": "m20", "birimEvrakNo": 20, "tur": "PTT MTS Tebligat Belgesi", "aciklama": "PTT MTS Tebligat Belgesi", "onaylandigiTarih": "03/10/2026"}),
        json!({"evrakId": "m21", "birimEvrakNo": 21, "tur": "PTT MTS Tebligat Belgesi", "aciklama": "PTT MTS Tebligat Belgesi", "onaylandigiTarih": "03/10/2026"}),
    ];
    let mts_pdf = |evrak_id: &str| match evrak_id {
        "m20" => teb::ornek_pdf(format!("(*{barkod_m}*) Tj (Mazbata formu) Tj").as_bytes()),
        _ => teb::ornek_pdf(format!("(*{barkod_m}*) Tj (\\326DEME EMR\\335) Tj").as_bytes()),
    };
    let mts_ptt = |_: &str| json!({"errorState": true, "kabul": {"alici": "B* V* M* A* Y*"}, "sondurum": {"son_durum_aciklama": "KABUL EDİLDİ", "teslim_tarihi": "0", "son_islem_tarihi": "20261004"}});
    let (pdf_n, ptt_n, atlanan) = ornekle_isle(c, d2, &mts, &mts_pdf, &mts_ptt, true);
    assert_eq!((pdf_n, ptt_n, atlanan.len()), (2, 1, 1), "{atlanan:?}");
    let borclu2: i64 = c.query_one("SELECT id FROM dosya_taraf WHERE dosya_id = $1 AND rol = 'borclu'", &[&d2]).unwrap().get(0);
    let s = c.query_one("SELECT birim_evrak_no, tebligat_turu, borclu_id FROM tebligat_barkod WHERE dosya_id = $1", &[&d2]).unwrap();
    assert_eq!((s.get::<_, i32>(0), s.get::<_, String>(1).as_str(), s.get::<_, Option<i64>>(2)), (20, "Ödeme Emri", Some(borclu2)));
    let (pdf_n, ptt_n, atlanan) = ornekle_isle(c, d2, &mts, &|_| panic!("PDF indirilmemeli"), &mts_ptt, true);
    assert_eq!((pdf_n, ptt_n, atlanan.len()), (0, 1, 1), "mükerrer evrak sonraki turda atlanır");
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_barkod WHERE dosya_id <> 0 AND barkod = '4100131175175'"), 1);

    // ── Elle mazbata bağı (sağ tık): iki fiziki tebligat, barkodsuz iki mazbata → otomatik bağ yok; elle bağ kalıcı ──
    let (d9, _) = dosya_kunyesi_kaydet(c, &kunye("2026/9", 1001), Some(YargiTuru::Icra.kod())).unwrap();
    let ev9 = vec![
        json!({"birimEvrakNo": 10, "tur": teb::HEDEF_TUR, "aciklama": "Normal Tebligat[AHMET YILMAZ]", "onaylandigiTarih": "01/10/2026"}),
        json!({"birimEvrakNo": 11, "tur": teb::HEDEF_TUR, "aciklama": "Normal Tebligat[FATMA BİROLOĞLU]", "onaylandigiTarih": "01/10/2026"}),
        json!({"birimEvrakNo": 12, "tur": teb::HEDEF_TUR, "aciklama": "Elektronik Tebligat[X BANKASI]", "onaylandigiTarih": "01/10/2026"}),
        json!({"birimEvrakNo": 13, "tur": teb::TEBLIG_MAZBATASI_TUR, "aciklama": "", "onaylandigiTarih": "20/10/2026"}),
        json!({"birimEvrakNo": 14, "tur": teb::KAPALI_TEBLIG_MAZBATASI_TUR, "aciklama": "", "onaylandigiTarih": "21/10/2026"}),
    ];
    kayit::evrak_listesi_kaydet(c, d9, &ev9).unwrap();
    for (no, barkod, e) in [(10, "40011", false), (11, "40012", false), (12, "50013", true)] {
        c.execute("INSERT INTO tebligat_barkod (dosya_id, birim_evrak_no, barkod, elektronik_tebligat, tebligat_turu) VALUES ($1, $2, $3, $4, 'Ödeme Emri')",
                  &[&d9, &no, &barkod, &e]).unwrap();
    }
    let gun = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
    let mz = |c: &mut postgres::Client, no: i32| -> (bool, String) {
        let s = c.query_one("SELECT tebligat_mazbatasi_var, tebligat_mazbatasi_aciklama FROM tebligat_barkod WHERE dosya_id = $1 AND birim_evrak_no = $2", &[&d9, &no]).unwrap();
        (s.get(0), s.get(1))
    };
    mazbatalari_yeniden_bagla(c, d9, gun).unwrap();
    assert_eq!((mz(c, 10).0, mz(c, 11).0), (false, false), "iki aday: tahmin yok");
    crate::icra_api::mazbata_bagla(c, d9, 10, Some(13), gun).unwrap();
    let (v, a) = mz(c, 10);
    assert!(v && a.starts_with("Elle bağlandı: Tebliğ Mazbatası (evrak 13, 20.10.2026"), "{a}");
    assert!(teb::kendi_mazbatasi_dondu_mu("40011", v, &a), "elle bağ hesapta sayılır");
    assert!(!mz(c, 11).0);
    // Taramada da elle bağ geçerli (bağlam kayıtlı bağı okur); bağlanan mazbata başka tebligata aday olmaz.
    let ctx = dosya_baglami(c, d9, &ev9).unwrap();
    assert!(ctx.mazbatalar.bul("40011", 10).unwrap().starts_with("Elle bağlandı"));
    assert_eq!(ctx.mazbatalar.bul("40012", 11), None);
    crate::icra_api::mazbata_bagla(c, d9, 11, Some(14), gun).unwrap();
    assert!(mz(c, 11).1.contains("Kapalı Tebliğ Mazbatası (evrak 14"));
    // Mazbata başka tebligata taşınınca eski bağlar düşer.
    crate::icra_api::mazbata_bagla(c, d9, 11, Some(13), gun).unwrap();
    assert_eq!((mz(c, 10).0, mz(c, 11).1.contains("evrak 13")), (false, true));
    assert_eq!(say(c, "SELECT count(*) FROM tebligat_mazbata_bagi"), 1);
    // Bağ kaldırılınca otomatik eşleşmeye döner (iki aday: bağ yok).
    crate::icra_api::mazbata_bagla(c, d9, 11, None, gun).unwrap();
    assert_eq!((mz(c, 11).0, say(c, "SELECT count(*) FROM tebligat_mazbata_bagi")), (false, 0));
    assert!(crate::icra_api::mazbata_bagla(c, d9, 12, Some(13), gun).unwrap_err().contains("E-tebligat"));
    assert!(crate::icra_api::mazbata_bagla(c, d9, 10, Some(12), gun).unwrap_err().contains("Mazbata evrakı"));
    // UYAP dosya penceresinden sağ tık: taramanın görmediği yeni mazbata veritabanına yazılır; seçenek yalnız PTT tebligatları.
    let yeni = json!({"evrakId": "x", "birimEvrakNo": 15, "tur": teb::TEBLIG_MAZBATASI_TUR, "aciklama": "", "onaylandigiTarih": "22/10/2026 10:00"});
    let s = crate::icra_api::mazbata_bag_secenekleri(c, d9, &yeni).unwrap();
    assert_eq!((s["mazbata"]["evrak_no"].as_i64(), s["mazbata"]["evrak_tarihi"].as_str()), (Some(15), Some("22/10/2026")));
    let nolar: Vec<i64> = s["tebligatlar"].as_array().unwrap().iter().map(|t| t["evrak_no"].as_i64().unwrap()).collect();
    assert_eq!(nolar.len(), 2);
    assert!(!nolar.contains(&12), "e-tebligat seçenek değil");
    crate::icra_api::mazbata_bagla(c, d9, 11, Some(15), gun).unwrap();
    let s = crate::icra_api::mazbata_bag_secenekleri(c, d9, &yeni).unwrap();
    assert_eq!(s["mazbata"]["elle_tebligat"].as_i64(), Some(11));
    let yanlis = json!({"birimEvrakNo": 16, "tur": teb::HEDEF_TUR});
    assert!(crate::icra_api::mazbata_bag_secenekleri(c, d9, &yanlis).unwrap_err().contains("mazbata türü değil"));
    crate::icra_api::mazbata_bagla(c, d9, 11, None, gun).unwrap();

    c.batch_execute("DELETE FROM dosya; DELETE FROM taraf; DELETE FROM vekil; DELETE FROM birim; DELETE FROM buro_ayar WHERE anahtar = 'mts_mukerrer'")
        .unwrap();
}
