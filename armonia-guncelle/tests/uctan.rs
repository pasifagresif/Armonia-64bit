//! Uçtan uca: paketle → bilgi → imzala → kur → etkin-sec, ve her saldırı durumunun reddi.

use armonia_guncelle::anahtar::{self, AcikAnahtar, Anahtarlar, IptalBildirimi, IptalKaydi, ONEK_GUNCELLEME, ONEK_IPTAL};
use armonia_guncelle::bilgi::{self, Bilgi, Paket};
use armonia_guncelle::kur::{self, Imaj, KurGirdisi, Yerlesim};
use armonia_guncelle::sec::{etkin_sec, Secim};
use armonia_guncelle::surum::Surum;
use armonia_guncelle::{ozet_hex, paket, zaman, URUN};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const M: &str = "x86_64";
const T0: i64 = 1_795_000_000; // imaj derleme zamanı (Kasım 2026)
const GUN: i64 = 86400;

struct Anahtar {
    pkcs8: Vec<u8>,
    acik: AcikAnahtar,
}

fn anahtar_uret() -> Anahtar {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap().as_ref().to_vec();
    let acik = anahtar::imzala(&pkcs8, b"", b"").unwrap().0;
    Anahtar { pkcs8, acik }
}

fn imzala(k: &Anahtar, onek: &[u8], veri: &[u8]) -> Vec<u8> {
    anahtar::imzala(&k.pkcs8, onek, veri).unwrap().1.into_bytes()
}

fn anahtarlar(etkin: &Anahtar, yedek: &Anahtar) -> Anahtarlar {
    let sonraki = anahtar_uret();
    Anahtarlar::metinden(&format!(
        "# deneme\netkin {}\nyedek {}\nsonraki {}\n",
        etkin.acik.b64(),
        yedek.acik.b64(),
        ozet_hex(&sonraki.acik.0)
    ))
    .unwrap()
}

fn imaj() -> Imaj {
    Imaj { surum: Surum(1, 0, 0), tarih: T0, arayuz: 1 }
}

fn sahte_elf(makine: u16) -> Vec<u8> {
    let mut v = vec![0u8; 64];
    v[..4].copy_from_slice(b"\x7fELF");
    v[4] = 2;
    v[5] = 1;
    v[18..20].copy_from_slice(&makine.to_le_bytes());
    v
}

fn kaynak_yap(kok: &Path, etiket: &str) -> PathBuf {
    let k = kok.join(format!("kaynak-{etiket}"));
    fs::create_dir_all(k.join("webapp/js")).unwrap();
    fs::create_dir_all(k.join("uyap_core/enjekte")).unwrap();
    fs::write(k.join("uyap_app"), sahte_elf(0x3e)).unwrap();
    fs::write(k.join("SURUM"), format!("surum={etiket}\n")).unwrap();
    fs::write(k.join("webapp/index.html"), format!("<p>{etiket}</p>")).unwrap();
    fs::write(k.join("webapp/js/a.js"), "1").unwrap();
    fs::write(k.join("uyap_core/enjekte/e.js"), "2").unwrap();
    k
}

struct Yayin {
    bilgi: Vec<u8>,
    imza: Vec<u8>,
    paket: PathBuf,
}

fn bilgi_yap(s: Surum, zst: &[u8], agac_ozeti: &str, yayin: i64, ayar: impl FnOnce(&mut Bilgi)) -> Vec<u8> {
    let mut paketler = BTreeMap::new();
    paketler.insert(
        M.to_string(),
        Paket { ad: bilgi::paket_adi(s, M), sha256: ozet_hex(zst), boyut: zst.len() as u64, agac_ozeti: agac_ozeti.into() },
    );
    let mut b = Bilgi {
        bicim: 1,
        urun: URUN.into(),
        kanal: "kararli".into(),
        surum: s.to_string(),
        yayin: zaman::yaz(yayin),
        gecerlilik_sonu: zaman::yaz(yayin + 30 * GUN),
        imaj_arayuzu: 1,
        geri_cekilen: vec![],
        paketler,
        notlar: "Deneme sürümü".into(),
    };
    ayar(&mut b);
    serde_json::to_vec_pretty(&b).unwrap()
}

fn yayinla(kok: &Path, k: &Anahtar, s: Surum, yayin: i64, ayar: impl FnOnce(&mut Bilgi)) -> Yayin {
    let kaynak = kaynak_yap(kok, &s.to_string());
    let (zst, agac_ozeti) = paket::paketle(&kaynak).unwrap();
    let p = kok.join(format!("gelen-{s}.tar.zst"));
    fs::write(&p, &zst).unwrap();
    let bilgi = bilgi_yap(s, &zst, &agac_ozeti, yayin, ayar);
    let imza = imzala(k, ONEK_GUNCELLEME, &bilgi);
    Yayin { bilgi, imza, paket: p }
}

fn kur_(y: &Yerlesim, a: &Anahtarlar, simdi: i64, p: &Yayin) -> armonia_guncelle::Sonuc<Surum> {
    kur::kur(y, a, &imaj(), M, simdi, KurGirdisi { bilgi: &p.bilgi, imza: &p.imza, paket: &p.paket, sahip_uid: None })
}

fn ortam() -> (tempfile::TempDir, Yerlesim, Anahtar, Anahtar) {
    let t = tempfile::tempdir().unwrap();
    let y = Yerlesim::new(t.path().join("var"));
    (t, y, anahtar_uret(), anahtar_uret())
}

#[test]
fn mutlu_yol_ve_secim() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + GUN, |_| {});
    assert_eq!(kur_(&y, &a, T0 + 2 * GUN, &p).unwrap(), Surum(1, 1, 0));
    match etkin_sec(&y, &a, &imaj(), M, T0 + 2 * GUN, None).unwrap() {
        Secim::Guncel(d, s) => {
            assert_eq!(s, Surum(1, 1, 0));
            assert_eq!(fs::read_to_string(d.join("webapp/index.html")).unwrap(), "<p>1.1.0</p>");
        }
        x => panic!("{x:?}"),
    }
    // Bilginin süresi geçse de kurulu sürüm çalışmaya devam eder
    assert!(matches!(etkin_sec(&y, &a, &imaj(), M, T0 + 400 * GUN, None).unwrap(), Secim::Guncel(..)));
    // Aynı sürüm yeniden kurulamaz (tek yönlü sayaç)
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).is_err());
}

#[test]
fn sahte_ve_yanlis_imzalar() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    let sahte = anahtar_uret();
    let mut p = yayinla(t.path(), &sahte, Surum(1, 1, 0), T0 + GUN, |_| {});
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).unwrap_err().0.contains("bilinmeyen imza anahtarı"));
    // Yedek anahtar güncelleme imzalayamaz
    p.imza = imzala(&yd, ONEK_GUNCELLEME, &p.bilgi);
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).is_err());
    // Etkin anahtar, ama iptal öneki ile (alan ayırıcı)
    p.imza = imzala(&e, ONEK_IPTAL, &p.bilgi);
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).unwrap_err().0.contains("imza geçersiz"));
    // Önek olmadan
    p.imza = imzala(&e, b"", &p.bilgi);
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).is_err());
    // Doğru imza, sonra bilgide tek bayt değişikliği
    p.imza = imzala(&e, ONEK_GUNCELLEME, &p.bilgi);
    let mut b = p.bilgi.clone();
    let i = b.iter().position(|&c| c == b'D').unwrap();
    b[i] = b'd';
    let p2 = Yayin { bilgi: b, imza: p.imza.clone(), paket: p.paket.clone() };
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p2).is_err());
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).is_ok());
}

#[test]
fn alan_denetimleri() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    let simdi = T0 + 2 * GUN;
    type Ayar = Box<dyn FnOnce(&mut Bilgi)>;
    let durumlar: Vec<(&str, Ayar)> = vec![
        ("ürün", Box::new(|b| b.urun = "cokluuyap-windows".into())),
        ("kanal", Box::new(|b| b.kanal = "deneme".into())),
        ("biçim", Box::new(|b| b.bicim = 2)),
        ("arayüz", Box::new(|b| b.imaj_arayuzu = 2)),
        ("mimari yok", Box::new(|b| {
            let p = b.paketler.remove(M).unwrap();
            b.paketler.insert("aarch64".into(), Paket { ad: bilgi::paket_adi(Surum(1, 1, 0), "aarch64"), ..p });
        })),
        ("paket adı", Box::new(|b| b.paketler.get_mut(M).unwrap().ad = "../x.tar.zst".into())),
        ("süresi geçmiş", Box::new(|b| b.gecerlilik_sonu = zaman::yaz(T0 + GUN + 1))),
        ("kendi geri çekilmiş", Box::new(|b| b.geri_cekilen = vec!["1.1.0".into()])),
        ("denetim karakteri", Box::new(|b| b.notlar = "a\u{1b}[31m".into())),
    ];
    for (ad, ayar) in durumlar {
        let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + GUN, ayar);
        assert!(kur_(&y, &a, simdi, &p).is_err(), "{ad} kabul edildi");
        let _ = fs::remove_dir_all(t.path().join("kaynak-1.1.0"));
    }
    // Bilinmeyen alan
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + GUN, |_| {});
    let mut j: serde_json::Value = serde_json::from_slice(&p.bilgi).unwrap();
    j["adres"] = "https://kotu.example/x".into();
    let b = serde_json::to_vec(&j).unwrap();
    let p2 = Yayin { imza: imzala(&e, ONEK_GUNCELLEME, &b), bilgi: b, paket: p.paket.clone() };
    assert!(kur_(&y, &a, simdi, &p2).is_err());
}

#[test]
fn zaman_ve_taban() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    // İmajdan eski yayın (eski ama geçerli imzalı bilgi)
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 - GUN, |_| {});
    assert!(kur_(&y, &a, T0 + GUN, &p).unwrap_err().0.contains("imajdaki sürümden eski"));
    let _ = fs::remove_dir_all(t.path().join("kaynak-1.1.0"));
    // Saat yayının gerisinde (saat geri alınmış)
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + 10 * GUN, |_| {});
    assert!(kur_(&y, &a, T0 + 5 * GUN, &p).unwrap_err().0.contains("saat"));
    // İmajdakinden eski/eşit sürüm
    let p = yayinla(t.path(), &e, Surum(1, 0, 0), T0 + GUN, |_| {});
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).is_err());
    // Yeni kurulduktan sonra daha eski bir güncelleme (geri düşürme) reddedilir
    let p2 = yayinla(t.path(), &e, Surum(1, 3, 0), T0 + GUN, |_| {});
    kur_(&y, &a, T0 + 2 * GUN, &p2).unwrap();
    let p3 = yayinla(t.path(), &e, Surum(1, 2, 0), T0 + 2 * GUN, |_| {});
    assert!(kur_(&y, &a, T0 + 3 * GUN, &p3).is_err());
}

#[test]
fn paket_kurcalama() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + GUN, |_| {});
    let mut zst = fs::read(&p.paket).unwrap();
    let n = zst.len();
    zst[n / 2] ^= 0xff;
    fs::write(&p.paket, &zst).unwrap();
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).unwrap_err().0.contains("özeti"));
    zst.push(0);
    fs::write(&p.paket, &zst).unwrap();
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).unwrap_err().0.contains("boyutu"));
    assert!(!y.etkin().exists());
}

#[test]
fn yanlis_mimari_ikili() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    let k = kaynak_yap(t.path(), "1.1.0");
    fs::write(k.join("uyap_app"), sahte_elf(0xb7)).unwrap(); // aarch64 ikilisi x86_64 paketinde
    let (zst, oz) = paket::paketle(&k).unwrap();
    let pp = t.path().join("p.tar.zst");
    fs::write(&pp, &zst).unwrap();
    let bilgi = bilgi_yap(Surum(1, 1, 0), &zst, &oz, T0 + GUN, |_| {});
    let p = Yayin { imza: imzala(&e, ONEK_GUNCELLEME, &bilgi), bilgi, paket: pp };
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).unwrap_err().0.contains("için değil"));
}

/// tar başlığını yol denetimi YAPMADAN yazar (saldırgan paketi taklidi)
fn ham_girdi(b: &mut tar::Builder<Vec<u8>>, ad: &str, tur: tar::EntryType, veri: &[u8], bag: Option<&str>) {
    let mut h = tar::Header::new_old();
    h.as_old_mut().name[..ad.len()].copy_from_slice(ad.as_bytes());
    if let Some(l) = bag {
        h.as_old_mut().linkname[..l.len()].copy_from_slice(l.as_bytes());
    }
    h.set_entry_type(tur);
    h.set_size(veri.len() as u64);
    h.set_mode(0o644);
    h.set_cksum();
    b.append(&h, veri).unwrap();
}

#[test]
fn zararli_arsivler() {
    let t = tempfile::tempdir().unwrap();
    let elf = sahte_elf(0x3e);
    let durumlar: Vec<(&str, &str, tar::EntryType, Option<&str>)> = vec![
        ("üst dizine kaçış", "webapp/../../kacti", tar::EntryType::Regular, None),
        ("mutlak yol", "/etc/kacti", tar::EntryType::Regular, None),
        ("sembolik bağ", "webapp/bag", tar::EntryType::Symlink, Some("/etc/shadow")),
        ("sabit bağ", "webapp/sabit", tar::EntryType::Link, Some("/etc/shadow")),
        ("aygıt", "webapp/aygit", tar::EntryType::Char, None),
        ("beyaz liste dışı", "pgsql/bin/postgres", tar::EntryType::Regular, None),
        ("ters bölü", "webapp\\..\\x", tar::EntryType::Regular, None),
        ("yinelenen", "uyap_app", tar::EntryType::Regular, None),
    ];
    for (i, (ad, yol, tur, bag)) in durumlar.into_iter().enumerate() {
        let mut b = tar::Builder::new(Vec::new());
        let manifest = format!("{} uyap_app\n", ozet_hex(&elf));
        ham_girdi(&mut b, "MANIFEST", tar::EntryType::Regular, manifest.as_bytes(), None);
        ham_girdi(&mut b, "uyap_app", tar::EntryType::Regular, &elf, None);
        ham_girdi(&mut b, yol, tur, if tur == tar::EntryType::Regular { b"x" } else { b"" }, bag);
        let tar_b = b.into_inner().unwrap();
        let zst = ruzstd::encoding::compress_to_vec(&tar_b[..], ruzstd::encoding::CompressionLevel::Fastest);
        let hedef = t.path().join(format!("h{i}"));
        let s = paket::ac(&zst[..], &hedef, &ozet_hex(manifest.as_bytes()), M);
        assert!(s.is_err(), "{ad} kabul edildi");
        assert!(!hedef.exists(), "{ad}: yarım açılmış dizin kaldı");
        assert!(!t.path().join("kacti").exists());
    }
    // MANIFEST'te olmayan fazladan dosya
    let mut b = tar::Builder::new(Vec::new());
    let manifest = format!("{} uyap_app\n", ozet_hex(&elf));
    ham_girdi(&mut b, "MANIFEST", tar::EntryType::Regular, manifest.as_bytes(), None);
    ham_girdi(&mut b, "uyap_app", tar::EntryType::Regular, &elf, None);
    ham_girdi(&mut b, "webapp/fazla.js", tar::EntryType::Regular, b"x", None);
    let zst = ruzstd::encoding::compress_to_vec(&b.into_inner().unwrap()[..], ruzstd::encoding::CompressionLevel::Fastest);
    assert!(paket::ac(&zst[..], &t.path().join("fazla"), &ozet_hex(manifest.as_bytes()), M).is_err());
    // Doğru içerik ama başka MANIFEST özeti (imzalı bilgiyle bağ)
    let mut b = tar::Builder::new(Vec::new());
    ham_girdi(&mut b, "MANIFEST", tar::EntryType::Regular, manifest.as_bytes(), None);
    ham_girdi(&mut b, "uyap_app", tar::EntryType::Regular, &elf, None);
    let zst = ruzstd::encoding::compress_to_vec(&b.into_inner().unwrap()[..], ruzstd::encoding::CompressionLevel::Fastest);
    assert!(paket::ac(&zst[..], &t.path().join("oz"), &"0".repeat(64), M).is_err());
    assert!(paket::ac(&zst[..], &t.path().join("iyi"), &ozet_hex(manifest.as_bytes()), M).is_ok());
}

/// GNU uzun ad girdisi ('L') + ardından asıl girdi: uzun adla `..` kaçışı da yakalanmalı
fn gnu_uzun_ad(b: &mut tar::Builder<Vec<u8>>, ad: &str, veri: &[u8]) {
    // tar yalnız GNU başlıklı 'L' girdisini uzun ad sayar (eski biçimde ham girdi olarak gelir ve reddedilir)
    let gnu = |b: &mut tar::Builder<Vec<u8>>, ad: &str, tur: tar::EntryType, veri: &[u8]| {
        let mut h = tar::Header::new_gnu();
        h.as_gnu_mut().unwrap().name[..ad.len()].copy_from_slice(ad.as_bytes());
        h.set_entry_type(tur);
        h.set_size(veri.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append(&h, veri).unwrap();
    };
    let mut ad0 = ad.as_bytes().to_vec();
    ad0.push(0);
    gnu(b, "././@LongLink", tar::EntryType::GNULongName, &ad0);
    gnu(b, "kisa", tar::EntryType::Regular, veri);
}

fn ek_paket(manifest: &[u8], elf: &[u8], ek: &dyn Fn(&mut tar::Builder<Vec<u8>>)) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    ham_girdi(&mut b, "MANIFEST", tar::EntryType::Regular, manifest, None);
    ham_girdi(&mut b, "uyap_app", tar::EntryType::Regular, elf, None);
    ek(&mut b);
    ruzstd::encoding::compress_to_vec(&b.into_inner().unwrap()[..], ruzstd::encoding::CompressionLevel::Fastest)
}

#[test]
fn zararli_arsivler_ek() {
    let t = tempfile::tempdir().unwrap();
    let elf = sahte_elf(0x3e);
    let manifest = format!("{} uyap_app\n", ozet_hex(&elf));
    let mo = ozet_hex(manifest.as_bytes());
    let mb = manifest.as_bytes();
    let uzun = format!("webapp/{}/../../../kacti", "a".repeat(120));
    let durumlar: Vec<(&str, Vec<u8>)> = vec![
        ("GNU uzun adla kaçış", ek_paket(mb, &elf, &|b| gnu_uzun_ad(b, &uzun, b"x"))),
        ("Continuous türü", ek_paket(mb, &elf, &|b| ham_girdi(b, "webapp/c", tar::EntryType::Continuous, b"x", None))),
        ("GNU seyrek", ek_paket(mb, &elf, &|b| ham_girdi(b, "webapp/s", tar::EntryType::GNUSparse, b"", None))),
        ("dosya/dizin çakışması", ek_paket(mb, &elf, &|b| ham_girdi(b, "webapp", tar::EntryType::Regular, b"x", None))),
        (
            "dosyanın altına dosya",
            ek_paket(mb, &elf, &|b| {
                ham_girdi(b, "webapp/a.js", tar::EntryType::Regular, b"x", None);
                ham_girdi(b, "webapp/a.js/b.js", tar::EntryType::Regular, b"y", None);
            }),
        ),
    ];
    for (i, (ad, zst)) in durumlar.into_iter().enumerate() {
        let hedef = t.path().join(format!("e{i}"));
        assert!(paket::ac(&zst[..], &hedef, &mo, M).is_err(), "{ad} kabul edildi");
        assert!(!hedef.exists(), "{ad}: yarım dizin kaldı");
    }
    assert!(!t.path().join("kacti").exists());
    // Uzun ama geçerli ad kabul edilir (yayıncının uzun webapp yolları)
    let uzun_iyi = format!("webapp/{}/x.js", "b".repeat(150));
    let mut m = BTreeMap::new();
    m.insert("uyap_app".to_string(), ozet_hex(&elf));
    m.insert(uzun_iyi.clone(), ozet_hex(b"z"));
    let man = paket::manifest_yaz(&m);
    let zst = ek_paket(&man, &elf, &|b| gnu_uzun_ad(b, &uzun_iyi, b"z"));
    paket::ac(&zst[..], &t.path().join("uzun"), &ozet_hex(&man), M).unwrap();
    assert!(t.path().join("uzun").join(&uzun_iyi).exists());
}

#[test]
fn kurulu_agac_kurcalanirsa_imaj() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + GUN, |_| {});
    kur_(&y, &a, T0 + 2 * GUN, &p).unwrap();
    let d = y.surum_dizini(Surum(1, 1, 0));
    let simdi = T0 + 2 * GUN;
    // Dosya değişti
    fs::write(d.join("webapp/js/a.js"), "kotu()").unwrap();
    assert!(etkin_sec(&y, &a, &imaj(), M, simdi, None).is_err());
    fs::write(d.join("webapp/js/a.js"), "1").unwrap();
    assert!(etkin_sec(&y, &a, &imaj(), M, simdi, None).is_ok());
    // Fazladan dosya
    fs::write(d.join("webapp/ek.js"), "x").unwrap();
    assert!(etkin_sec(&y, &a, &imaj(), M, simdi, None).is_err());
    fs::remove_file(d.join("webapp/ek.js")).unwrap();
    // MANIFEST de yeniden yazılsa imzalı özet tutmaz
    fs::write(d.join("webapp/js/a.js"), "kotu()").unwrap();
    let mut m = paket::manifest_coz(&fs::read(d.join("MANIFEST")).unwrap()).unwrap();
    m.insert("webapp/js/a.js".into(), ozet_hex(b"kotu()"));
    fs::write(d.join("MANIFEST"), paket::manifest_yaz(&m)).unwrap();
    assert!(etkin_sec(&y, &a, &imaj(), M, simdi, None).unwrap_err().0.contains("MANIFEST"));
    // etkin dosyasında çöp
    fs::write(y.etkin(), "../../etc\n").unwrap();
    assert!(etkin_sec(&y, &a, &imaj(), M, simdi, None).is_err());
}

#[cfg(unix)]
#[test]
fn kurulu_agacta_sembolik_bag() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + GUN, |_| {});
    kur_(&y, &a, T0 + 2 * GUN, &p).unwrap();
    let d = y.surum_dizini(Surum(1, 1, 0));
    fs::remove_file(d.join("webapp/js/a.js")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", d.join("webapp/js/a.js")).unwrap();
    assert!(etkin_sec(&y, &a, &imaj(), M, T0 + 2 * GUN, None).is_err());
}

#[test]
fn geri_cekme_ve_budama() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    for (i, s) in [Surum(1, 1, 0), Surum(1, 2, 0), Surum(1, 3, 0)].into_iter().enumerate() {
        let p = yayinla(t.path(), &e, s, T0 + (i as i64 + 1) * GUN, |_| {});
        kur_(&y, &a, T0 + (i as i64 + 2) * GUN, &p).unwrap();
    }
    // Kurulum hiçbir sürümü silmez (çalışan sürüm yerinde kalsın; denetçi 24.11 O-4)
    for s in [Surum(1, 1, 0), Surum(1, 2, 0), Surum(1, 3, 0)] {
        assert!(y.surum_dizini(s).exists(), "{s}");
    }
    // Bir açılışta en çok 3: dördüncü kurulum yeniden başlatma ister
    let p4 = yayinla(t.path(), &e, Surum(1, 3, 5), T0 + 4 * GUN, |_| {});
    assert!(kur_(&y, &a, T0 + 5 * GUN, &p4).unwrap_err().0.contains("yeniden başlatın"));
    // Yayıncı 1.3.0'ı geri çekti (1.4.0 bilgisi, henüz kurulmadı)
    let p = yayinla(t.path(), &e, Surum(1, 4, 0), T0 + 5 * GUN, |b| b.geri_cekilen = vec!["1.3.0".into()]);
    let s = etkin_sec(&y, &a, &imaj(), M, T0 + 6 * GUN, Some((&p.bilgi, &p.imza))).unwrap();
    assert!(matches!(s, Secim::Imaj(ref n) if n.contains("geri çekildi")), "{s:?}");
    // Sahte imzalı "geri çekme" yok sayılır
    let sahte = anahtar_uret();
    let ps = yayinla(t.path(), &sahte, Surum(1, 5, 0), T0 + 5 * GUN, |b| b.geri_cekilen = vec!["1.3.0".into()]);
    assert!(matches!(etkin_sec(&y, &a, &imaj(), M, T0 + 6 * GUN, Some((&ps.bilgi, &ps.imza))).unwrap(), Secim::Guncel(..)));
    // Açılışta budama: yalnız etkin kalır, sonra yeni kurulum yine mümkün
    assert_eq!(kur::buda(&y).unwrap(), vec![Surum(1, 1, 0), Surum(1, 2, 0)]);
    assert!(y.surum_dizini(Surum(1, 3, 0)).exists());
    assert!(matches!(etkin_sec(&y, &a, &imaj(), M, T0 + 6 * GUN, None).unwrap(), Secim::Guncel(..)));
    assert!(kur_(&y, &a, T0 + 5 * GUN, &p4).is_ok());
}

#[test]
fn imaj_daha_yeniyse_imaj() {
    let (t, y, e, yd) = ortam();
    let a = anahtarlar(&e, &yd);
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + GUN, |_| {});
    kur_(&y, &a, T0 + 2 * GUN, &p).unwrap();
    let yeni_imaj = Imaj { surum: Surum(1, 2, 0), tarih: T0 + 3 * GUN, arayuz: 1 };
    assert!(matches!(etkin_sec(&y, &a, &yeni_imaj, M, T0 + 4 * GUN, None).unwrap(), Secim::Imaj(_)));
}

#[test]
fn anahtar_iptali() {
    let (t, y, e, yd) = ortam();
    let anahtar_dosyasi = t.path().join("anahtarlar");
    let yeni = anahtar_uret();
    let ucuncu = anahtar_uret();
    fs::write(&anahtar_dosyasi, format!("etkin {}\nyedek {}\nsonraki {}\n", e.acik.b64(), yd.acik.b64(), ozet_hex(&yeni.acik.0))).unwrap();
    let bildirim_yap = |yeni_etkin: &AcikAnahtar, sonraki: &str| {
        serde_json::to_vec(&IptalBildirimi {
            islem: "iptal".into(),
            urun: URUN.into(),
            anahtar_kimligi: e.acik.kimlik(),
            yeni_etkin: yeni_etkin.b64(),
            sonraki: sonraki.into(),
        })
        .unwrap()
    };
    // Yedek sızsa bile önceden taahhüt edilmemiş (saldırganın) anahtarı etkinleştiremez (denetçi 24.11 O-1)
    let saldirgan = anahtar_uret();
    let kotu = bildirim_yap(&saldirgan.acik, &ozet_hex(&ucuncu.acik.0));
    assert!(kur::iptal_ekle(&y, &anahtar_dosyasi, &kotu, &imzala(&yd, ONEK_IPTAL, &kotu)).unwrap_err().0.contains("taahhüt"));
    // Yeni taahhüt bozuksa da ret
    let bozuk = bildirim_yap(&yeni.acik, "abc");
    assert!(kur::iptal_ekle(&y, &anahtar_dosyasi, &bozuk, &imzala(&yd, ONEK_IPTAL, &bozuk)).is_err());
    let bildirim = bildirim_yap(&yeni.acik, &ozet_hex(&ucuncu.acik.0));
    // Etkin anahtar kendi iptalini imzalayamaz; yalnız yedek
    assert!(kur::iptal_ekle(&y, &anahtar_dosyasi, &bildirim, &imzala(&e, ONEK_IPTAL, &bildirim)).is_err());
    // Yedek ama güncelleme önekiyle
    assert!(kur::iptal_ekle(&y, &anahtar_dosyasi, &bildirim, &imzala(&yd, ONEK_GUNCELLEME, &bildirim)).is_err());
    assert_eq!(kur::iptal_ekle(&y, &anahtar_dosyasi, &bildirim, &imzala(&yd, ONEK_IPTAL, &bildirim)).unwrap(), yeni.acik.kimlik());
    let a = Anahtarlar::yukle(&anahtar_dosyasi, Some(&y.iptal())).unwrap();
    assert_eq!(a.etkin, yeni.acik);
    assert_eq!(a.sonraki, ozet_hex(&ucuncu.acik.0));
    // Eski (sızmış) anahtarla imzalı güncelleme artık reddedilir
    let p = yayinla(t.path(), &e, Surum(1, 1, 0), T0 + GUN, |_| {});
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).unwrap_err().0.contains("iptal"));
    let p = yayinla(t.path(), &yeni, Surum(1, 2, 0), T0 + GUN, |_| {});
    assert!(kur_(&y, &a, T0 + 2 * GUN, &p).is_ok());
    // Aynı bildirim ikinci kez uygulanamaz (artık etkin anahtarı hedeflemiyor)
    assert!(kur::iptal_ekle(&y, &anahtar_dosyasi, &bildirim, &imzala(&yd, ONEK_IPTAL, &bildirim)).is_err());
    // Kayıt elle kurcalanırsa (imzasız girdi) yükleme başarısız olur
    let mut kayit: Vec<IptalKaydi> = serde_json::from_slice(&fs::read(y.iptal()).unwrap()).unwrap();
    kayit[0].bildirim = B64.encode(b"{}");
    fs::write(y.iptal(), serde_json::to_vec(&kayit).unwrap()).unwrap();
    assert!(Anahtarlar::yukle(&anahtar_dosyasi, Some(&y.iptal())).is_err());
}

#[test]
fn imaj_bilgisi_okuma() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("SURUM"), "surum=1.7.3\ngit=abc\nderleme=2026-11-24T10:00:00Z\n").unwrap();
    fs::write(t.path().join("ayar"), "# Armonia\nimaj_arayuzu=1\n").unwrap();
    let i = Imaj::oku(&t.path().join("SURUM"), &t.path().join("ayar")).unwrap();
    assert_eq!((i.surum, i.arayuz), (Surum(1, 7, 3), 1));
    assert_eq!(i.tarih, zaman::coz("2026-11-24T10:00:00Z").unwrap());
}
