// Armonia Yazı: editör arayüzü (ilk sürüm). UDF açar, sayfa genişliğinde gösterir, metni ve temel biçimi
// (kalın, italik, altı çizili, hizalama) düzenler, UDF olarak kaydeder. Üst/alt bilgi ve tablolar şimdilik salt
// okunur (korunur). Sayfalara bölme sonraki adım. Claude, 2026-11-23.

import { baseKeymap, chainCommands, setBlockType, toggleMark } from "prosemirror-commands";
import { history, redo, undo } from "prosemirror-history";
import { keymap } from "prosemirror-keymap";
import { EditorState, type Command } from "prosemirror-state";
import { EditorView } from "prosemirror-view";

import { bloklariCiz } from "./cizim";
import { belgeDugumu, belgeyeCevir } from "./donustur";
import { BOYUTLAR, SATIR_ARALIKLARI, YAZI_TIPLERI, etkinYazi, girinti, liste, onekleriYenile, satirAraligi, yaziDegistir } from "./komutlar";
import { bosBelgeOlustur } from "./bos_belge";
import { imzaPenceresiAc } from "./imza_penceresi";
import { b64Coz, b64Kodla, cagir, masaustu, type ImzaBilgisi, type MasaustuDosya, type MasaustuKayit } from "./masaustu";
import { hucreGec, satirEkle, satirSil, sutunEkle, sutunSil, tabloEkle, tabloSil, tablodaMi } from "./tablo";
import { ARALIK, PT, YENIDEN_OLC, sayfaEklentisi, sekmeleriYerlestir, type SayfaOlcusu } from "./sayfa";
import { cssAilesi, sema } from "./sema";
import type { Acilis, Belge, Blok, GBlok, Hiza } from "./turler";
import { hexRenk, kenarSayfada, ozAl, renkCss, renkHex, sayfaNoMetni, type Ozellikler } from "./turler";
import init, { belge_ac, docx_kaydet, gorunum_hesapla, udf_kaydet } from "./wasm/belge_wasm.js";
import "./stil.css";

type Bicim = "udf" | "docx";

interface Acik {
  belge: Belge;
  /** Uzantısız dosya adı */
  kok: string;
  /** Kaydet'in yazacağı biçim (açılan biçim; Farklı kaydet değiştirir) */
  bicim: Bicim;
  gorunum: Acilis["gorunum"];
  /** Masaüstünde kabuğun dosya kimliği ("Kaydet" doğrudan o dosyaya yazar; yol arayüzde tutulmaz) */
  kimlik?: number;
  /** İmzalı belge salt okunur açılır (düzenleme imzayı geçersiz kılar) */
  salt: boolean;
}

let acik: Acik | null = null;
let gorunum: EditorView | null = null;
let degisti = false;
let onekZamani = 0;

const $ = <T extends HTMLElement>(s: string) => document.querySelector(s) as T;

function bildir(m: string, tur: "bilgi" | "uyari" | "hata" = "bilgi") {
  const b = $("#bildirim");
  b.hidden = false;
  b.className = tur;
  b.textContent = m;
}

// ── Komutlar ────────────────────────────────────────────────────────────────────────────────────────

function hizala(hiza: Hiza): Command {
  return (state, dispatch) => {
    const { from, to } = state.selection;
    if (dispatch) {
      const tr = state.tr;
      state.doc.nodesBetween(from, to, (n, pos) => {
        if (n.type === sema.nodes.paragraph) tr.setNodeMarkup(pos, undefined, { ...n.attrs, hiza });
      });
      dispatch(tr);
    }
    return true;
  };
}

/** Sekme ekler (UYAP'ta olduğu gibi Tab tuşu): imleçteki biçimi ve UDF parça özelliklerini alır. */
const sekmeEkle: Command = (state, dispatch) => {
  if (dispatch) {
    const isaretler = state.storedMarks || state.selection.$from.marks();
    const udf = isaretler.find((m) => m.type === sema.marks.udf);
    const oz = (udf?.attrs.oz as Ozellikler | undefined)?.filter(([a]) => ["family", "size", "resolver"].includes(a)) ?? [];
    const dugum = sema.nodes.sekme.create({ oz, metin: "	" }, null, isaretler.filter((m) => m.type !== sema.marks.udf));
    dispatch(state.tr.replaceSelectionWith(dugum, false).scrollIntoView());
  }
  return true;
};

const kalin = toggleMark(sema.marks.kalin);
const egik = toggleMark(sema.marks.egik);
const alti = toggleMark(sema.marks.alti);

// ── Aç / kaydet ─────────────────────────────────────────────────────────────────────────────────────

/** Üst/alt bilgi bloklarını yazı alanı genişliğinde çizer (katmanda ve ölçüm için). */
function kenarCiz(bloklar: Blok[] | undefined, g: GBlok[] | null, genislik: number): HTMLElement {
  const el = document.createElement("div");
  el.className = "kenar";
  el.style.width = `${genislik}px`;
  if (bloklar && g) bloklariCiz(bloklar, g, el);
  return el;
}

function sayfayiKur(a: Acilis) {
  const s = a.gorunum.sayfa;
  const masa = $("#masa");
  masa.innerHTML = "";
  const sayfaH = s.yukseklik * PT, sayfaW = s.genislik * PT;
  const yaziW = (s.genislik - s.sol - s.sag) * PT;

  // Ölçek kabı: sayfa CSS transform ile büyütülüp küçültülür; kap, kaydırma çubukları doğru olsun diye ölçekli
  // boyutu taşır. Düzen (satırlar, sayfa sonları) ölçekten bağımsızdır.
  const kap = document.createElement("div");
  kap.className = "olcek-kap";
  const sayfa = document.createElement("div");
  sayfa.className = "sayfa";
  sayfa.style.width = `${sayfaW}px`;
  const katman = document.createElement("div");
  katman.className = "katman";
  const govde = document.createElement("div");
  govde.className = "govde";
  govde.style.margin = `0 ${s.sag * PT}px 0 ${s.sol * PT}px`;
  sayfa.append(katman, govde);
  kap.append(sayfa);
  masa.append(kap);
  let toplamH = sayfaH;

  // Üst/alt bilgi yükseklikleri (yazı alanını daraltır) — çizip ölçülür
  const olc = (el: HTMLElement) => {
    el.style.position = "absolute";
    el.style.visibility = "hidden";
    document.body.append(el); // ölçeksiz yerde ölç
    sekmeleriYerlestir(el);
    const h = el.getBoundingClientRect().height;
    el.remove();
    return h;
  };
  const ustH = kenarSayfada(a.gorunum.ust_kenar, 1) ? olc(kenarCiz(a.belge.ust_bilgi?.bloklar, a.gorunum.ust_bilgi, yaziW)) : 0;
  const altH = kenarSayfada(a.gorunum.alt_kenar, 1) ? olc(kenarCiz(a.belge.alt_bilgi?.bloklar, a.gorunum.alt_bilgi, yaziW)) : 0;
  const olcu: SayfaOlcusu = {
    yukseklik: sayfaH,
    yaziUstu: Math.max(s.ust * PT, ustH ? s.ust_bilgi_payi * PT + ustH : 0),
    yaziAlti: Math.min(sayfaH - s.alt * PT, altH ? sayfaH - s.alt_bilgi_payi * PT - altH : Infinity),
    olcek: 1,
  };
  // Yakınlaştırma: "genislik" / "sayfa" pencereye göre hesaplanır
  olcekUygula = () => {
    const cs = getComputedStyle(masa);
    const bosW = masa.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    const bosH = masa.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
    const o = olcekSecimi === "genislik" ? bosW / sayfaW
      : olcekSecimi === "sayfa" ? Math.min(bosW / sayfaW, bosH / sayfaH)
      : Number(olcekSecimi) / 100;
    const yeni = Math.min(4, Math.max(0.2, o || 1));
    olcu.olcek = yeni;
    sonOlcek = yeni;
    sayfa.style.transform = `scale(${yeni})`;
    kap.style.width = `${sayfaW * yeni}px`;
    kap.style.height = `${toplamH * yeni}px`;
    secici.olcek.title = `Yakınlaştırma: %${Math.round(yeni * 100)}`;
    gorunum?.dispatch(gorunum.state.tr.setMeta(YENIDEN_OLC, true).setMeta("addToHistory", false));
  };
  govde.style.paddingTop = `${olcu.yaziUstu}px`;
  // Biçimsiz metin (yeni paragraf, yeni tablo hücresi) UDF'te belgenin varsayılan stilini alır (hvl-default;
  // stil.rs ile aynı varsayılanlar: Times New Roman 12): ekranda da öyle görünsün
  const vs = a.belge.stiller.find((st) => st.ad === "hvl-default")?.oz ?? [];
  govde.style.fontFamily = cssAilesi(ozAl(vs, "family") || "Times New Roman");
  govde.style.fontSize = `${Number(ozAl(vs, "size")) > 0 ? Number(ozAl(vs, "size")) : 12}pt`;

  // Sayfa sayısı değişince katman: kâğıtlar, üst/alt bilgi, sayfa numarası
  const katmaniCiz = (n: number) => {
    katman.innerHTML = "";
    toplamH = n * sayfaH + (n - 1) * ARALIK;
    sayfa.style.height = `${toplamH}px`;
    kap.style.height = `${toplamH * olcu.olcek}px`;
    govde.style.minHeight = `${toplamH}px`;
    for (let k = 0; k < n; k++) {
      const no = k + 1;
      const ust = k * (sayfaH + ARALIK);
      const kagit = document.createElement("div");
      kagit.className = "kagit";
      Object.assign(kagit.style, { top: `${ust}px`, height: `${sayfaH}px` });
      katman.append(kagit);
      if (kenarSayfada(a.gorunum.ust_kenar, no)) {
        const el = kenarCiz(a.belge.ust_bilgi?.bloklar, a.gorunum.ust_bilgi, yaziW);
        Object.assign(el.style, { top: `${ust + s.ust_bilgi_payi * PT}px`, left: `${s.sol * PT}px` });
        katman.append(el);
      }
      if (kenarSayfada(a.gorunum.alt_kenar, no)) {
        const el = kenarCiz(a.belge.alt_bilgi?.bloklar, a.gorunum.alt_bilgi, yaziW);
        Object.assign(el.style, { top: `${ust + sayfaH - s.alt_bilgi_payi * PT - altH}px`, left: `${s.sol * PT}px` });
        katman.append(el);
      }
      // Sayfa numarası alt bilginin sayfa aralığına bağlı, içinin dolu olmasına değil (udf_pdf.rs `aralikta`)
      {
        const sn = a.gorunum.sayfa_numarasi;
        const ak = a.gorunum.alt_kenar;
        if (sn && ak && kenarSayfada({ ...ak, gorunur: true }, no)) {
          // udf_pdf.rs: taban çizgisi sayfa altından alt bilgi payı kadar yukarıda
          const el2 = document.createElement("div");
          el2.className = "sayfa-no";
          el2.textContent = sayfaNoMetni(sn, no, n);
          Object.assign(el2.style, {
            top: `${ust + sayfaH - s.alt_bilgi_payi * PT - sn.stil.boyut * PT}px`,
            left: `${s.sol * PT}px`, width: `${yaziW}px`,
            textAlign: sn.hiza === "Orta" ? "center" : sn.hiza === "Sag" ? "right" : "left",
            fontFamily: cssAilesi(sn.stil.aile), fontSize: `${sn.stil.boyut}pt`, color: renkCss(sn.stil.renk),
            fontWeight: sn.stil.kalin ? "bold" : "normal", fontStyle: sn.stil.egik ? "italic" : "normal",
          });
          katman.append(el2);
        }
      }
    }
    sekmeleriYerlestir(katman, olcu.olcek);
    $("#sayfa-sayisi").textContent = `${n} sayfa`;
  };

  const doc = belgeDugumu(a.belge, a.gorunum);
  // Kayıpsızlık denetimi: hiç dokunulmamış belge modele aynen dönmeli
  const geri = belgeyeCevir(doc, a.belge);
  const ayni = JSON.stringify(geri) === JSON.stringify(a.belge);
  console.info(`[Armonia Yazı] gidiş-dönüş denetimi: ${ayni ? "TAMAM" : "FARK VAR"}`);
  if (!ayni) bildir("Uyarı: bu belgenin bazı öğeleri editörde birebir korunamıyor; kaydetmeden önce kontrol edin.", "uyari");

  gorunum?.destroy();
  gorunum = new EditorView(govde, {
    state: EditorState.create({
      doc,
      plugins: [
        history(),
        keymap({
          "Mod-z": undo, "Mod-y": redo, "Mod-Shift-z": redo,
          "Mod-b": kalin, "Mod-i": egik, "Mod-u": alti,
          "Mod-s": () => { kaydet(); return true; },
          "Mod-n": () => { yeniBelge(); return true; },
          "Mod-o": () => { acmaIste(); return true; },
          "Tab": chainCommands(hucreGec(1), sekmeEkle),
          "Shift-Tab": hucreGec(-1),
          "Mod-l": hizala("Sol"), "Mod-e": hizala("Orta"), "Mod-r": hizala("Sag"), "Mod-j": hizala("IkiYana"),
        }),
        keymap(baseKeymap),
        sayfaEklentisi(() => olcu, katmaniCiz),
      ],
    }),
    // İmzalı belge salt okunur (acik.salt); "Düzenlemeyi aç" ile açılır
    editable: () => !acik?.salt,
    dispatchTransaction(tr) {
      if (!gorunum) return;
      gorunum.updateState(gorunum.state.apply(tr));
      // Liste önekleri ve sayfa boşlukları belgeyi "değişmiş" yapmaz (kaydedilen içerik aynı)
      if (tr.docChanged && tr.getMeta("addToHistory") !== false) {
        degisti = true;
        clearTimeout(onekZamani);
        onekZamani = window.setTimeout(() => gorunum && acik && onekleriYenile(gorunum, acik.belge), 250);
      }
      cubuguGuncelle();
    },
  });
  degisti = false;
  olcekUygula();
  cubuguGuncelle();
  gorunum.focus();
}

// ── Yakınlaştırma ───────────────────────────────────────────────────────────────────────────────────

const OLCEK_ANAHTARI = "armonia-yazi-olcek";
const OLCEKLER: [string, string][] = [["genislik", "Sayfa genişliği"], ["sayfa", "Tam sayfa"],
  ["50", "%50"], ["75", "%75"], ["100", "%100"], ["125", "%125"], ["150", "%150"], ["200", "%200"]];

function olcekOku(): string {
  try {
    return localStorage.getItem(OLCEK_ANAHTARI) || "100";
  } catch {
    return "100";
  }
}

let olcekSecimi = olcekOku();
let olcekUygula: () => void = () => {};
let sonOlcek = 1;

function olcekSec(d: string) {
  olcekSecimi = d;
  try {
    localStorage.setItem(OLCEK_ANAHTARI, d);
  } catch {
    /* gizli pencere: hatırlanmaz */
  }
  seciciDeger(secici.olcek, d);
  olcekUygula();
}

/** Dar pencere: kompakt görünüm (araç çubuğu tek satır simge, sayfa pencere genişliğine sığar). */
const KOMPAKT_SINIR = 900;
let kompakt = false;

function boyutDegisti() {
  const k = window.innerWidth < KOMPAKT_SINIR;
  if (k !== kompakt) {
    kompakt = k;
    document.body.classList.toggle("kompakt", k);
    // Kompakta geçince sayfa genişliğine sığ; çıkınca kullanıcının seçimine dön
    if (k) {
      olcekSecimi = "genislik";
      seciciDeger(secici.olcek, "genislik");
    } else {
      olcekSecimi = olcekOku();
      seciciDeger(secici.olcek, olcekSecimi);
    }
  }
  olcekUygula();
}

/** "Aç": masaüstünde sistemin dosya penceresi, tarayıcıda dosya seçici. */
async function acmaIste() {
  if (!masaustu) {
    $<HTMLInputElement>("#dosya").click();
    return;
  }
  try {
    const d = await cagir<MasaustuDosya | null>("dosya_ac");
    if (d) await belgeAc(d.ad, b64Coz(d.bayt), d.kimlik);
  } catch (e) {
    bildir(`Açılamadı: ${e}`, "hata");
  }
}

/** "İmzala": belgenin kaydedilecek son hâli (UDF) karttaki sertifikayla imzalanır, imzalı UDF kaydedilir ve
 * salt okunur açılır. Word olarak açılmış belge de UDF olarak imzalanır (UYAP UDF ister). */
function imzalaIste() {
  if (!acik || !gorunum) return;
  if (!masaustu) {
    bildir("E-imza yalnız Armonia Yazı masaüstü uygulamasında (bilgisayara takılı kartla) kullanılabilir.", "uyari");
    return;
  }
  const kok = acik.kok;
  imzaPenceresiAc({
    dosyaAdi: `${kok}.udf`,
    belge: () => udf_kaydet(belgeyeCevir(gorunum!.state.doc, acik!.belge)) as Uint8Array,
    bitti: async (imzali, bilgi) => {
      try {
        const k = await cagir<MasaustuKayit | null>("dosya_kaydet", {
          onerilen: `${kok}.udf`, bayt: b64Kodla(imzali), bicim: "udf", kimlik: acik?.bicim === "udf" ? acik?.kimlik ?? null : null,
        });
        degisti = false;
        await belgeAc(k ? k.ad : `${kok}.udf`, imzali, k?.kimlik);
        if (!k) bildir(`İmzalandı (${bilgi.imzalayan}) ama kaydedilmedi. Kaydetmek için Ctrl+S kullanın.`, "uyari");
      } catch (e) {
        bildir(`İmzalandı ama kaydedilemedi: ${e}`, "hata");
      }
    },
  });
}

function yeniBelge() {
  if (degisti && !window.confirm("Kaydedilmemiş değişiklikler var. Yine de yeni belge açılsın mı?")) return;
  const belge = bosBelgeOlustur();
  const g = gorunum_hesapla(belge) as Acilis["gorunum"];
  acik = { belge, kok: "Yeni belge", bicim: "udf", gorunum: g, salt: false };
  $("#bildirim").hidden = true;
  sayfayiKur({ belge, gorunum: g, imzali: false, uyarilar: [] });
  document.title = "Yeni belge — Armonia Yazı";
}

async function dosyaAc(f: File) {
  await belgeAc(f.name, new Uint8Array(await f.arrayBuffer()));
}

/** Belgeyi baytlarından açar (tarayıcı: dosya seçici / sürükle-bırak; masaüstü: dosya penceresi, çift tık). */
async function belgeAc(ad: string, bayt: Uint8Array, kimlik?: number) {
  if (degisti && !window.confirm("Kaydedilmemiş değişiklikler var. Yine de başka belge açılsın mı?")) return;
  try {
    // Tür içerikten anlaşılır (uzantıya güvenilmez): UDF ya da DOCX
    const a = belge_ac(bayt) as Acilis & { tur: "udf" | "docx" };
    acik = { belge: a.belge, kok: ad.replace(/\.(udf|docx)$/i, ""), bicim: a.tur, gorunum: a.gorunum, kimlik, salt: a.imzali };
    $("#bildirim").hidden = true;
    sayfayiKur(a);
    document.title = `${ad} — Armonia Yazı`;
    const notlar: string[] = [];
    if (a.uyarilar.length) notlar.push("Korunamayan öğeler: " + a.uyarilar.join("; "));
    if (a.imzali) await imzaBilgisiGoster(bayt, notlar.join(" "));
    else if (notlar.length) bildir(notlar.join(" "), "uyari");
  } catch (e) {
    bildir(`Açılamadı: ${e}`, "hata");
  }
}

/** İmza zamanı (çekirdekten "YYYY-AA-GG SS:DD:ss UTC") Türkiye saatiyle; çözülemezse olduğu gibi. */
function yerelZaman(z: string): string {
  const m = /^(\d{4}-\d{2}-\d{2}) (\d{2}:\d{2}:\d{2}) UTC$/.exec(z);
  if (!m) return z;
  const t = new Date(`${m[1]}T${m[2]}Z`);
  if (isNaN(t.getTime())) return z;
  return t.toLocaleString("tr-TR", { timeZone: "Europe/Istanbul", dateStyle: "long", timeStyle: "medium" });
}

/** İmzalı belge: imza bilgisi (masaüstünde doğrulanır) ve "Düzenlemeyi aç" düğmesi. Belge salt okunur kalır. */
async function imzaBilgisiGoster(bayt: Uint8Array, ek: string) {
  const b = $("#bildirim");
  b.hidden = false;
  b.className = "imza";
  b.textContent = "";
  const metin = document.createElement("span");
  let bilgi: ImzaBilgisi | null = null;
  if (masaustu) {
    try {
      bilgi = await cagir<ImzaBilgisi | null>("imza_dogrula", { udf: b64Kodla(bayt) });
    } catch (e) {
      metin.textContent = `İmza doğrulanamadı: ${e}. `;
    }
  }
  if (bilgi) {
    const durum = !bilgi.belge_degismemis ? "BELGE İMZADAN SONRA DEĞİŞTİRİLMİŞ"
      : !bilgi.imza_gecerli ? "İMZA GEÇERSİZ"
      : !bilgi.sure_icinde ? "imza, sertifikanın geçerlilik süresi dışında atılmış"
      : "imza geçerli";
    b.classList.toggle("hata", !bilgi.belge_degismemis || !bilgi.imza_gecerli);
    metin.textContent = `E-imzalı: ${bilgi.imzalayan}${bilgi.imza_zamani ? ` · ${yerelZaman(bilgi.imza_zamani)}` : ""} · ${durum}` +
      " (sertifika zinciri ve iptal durumu denetlenmedi). ";
  } else if (!masaustu) {
    metin.textContent = "Belge e-imzalı (imza doğrulaması masaüstü uygulamasında). ";
  }
  if (ek) metin.textContent += ek + " ";
  const dugme = document.createElement("button");
  dugme.textContent = "Düzenlemeyi aç";
  dugme.title = "Belgeyi değiştirip kaydederseniz imza geçersiz olur; yeniden imzalamanız gerekir.";
  dugme.onclick = () => {
    if (!acik || !window.confirm("Düzenlerseniz imza geçersiz olur ve belgeyi yeniden imzalamanız gerekir. Devam edilsin mi?")) return;
    acik.salt = false;
    gorunum?.setProps({});
    b.hidden = true;
    cubuguGuncelle();
  };
  b.append(metin, dugme);
}

/** Kaydeder: verilen biçimde (yoksa açılan biçimde). Word'e kaydederken UYAP'a özgü öğeler (alanlar) düz metin olur. */
async function kaydet(bicim?: Bicim) {
  if (!acik || !gorunum) return;
  if (acik.salt && !bicim) {
    bildir("İmzalı belge değiştirilmedi; kaydetmeye gerek yok. Farklı biçimde kaydetmek için \"Farklı kaydet\".", "bilgi");
    return;
  }
  try {
    const b = bicim ?? acik.bicim;
    const belge = belgeyeCevir(gorunum.state.doc, acik.belge);
    const bayt = (b === "docx" ? docx_kaydet(belge) : udf_kaydet(belge)) as Uint8Array;
    const ad = `${acik.kok}.${b}`;
    if (masaustu) {
      // "Kaydet" (aynı biçim, dosya belli) doğrudan yazar; "Farklı kaydet" ya da yeni belge pencere açar
      const k = await cagir<MasaustuKayit | null>("dosya_kaydet", {
        onerilen: ad, bayt: b64Kodla(bayt), bicim: b, kimlik: !bicim ? acik.kimlik ?? null : null,
      });
      if (!k) return;
      acik.kimlik = k.kimlik;
      acik.bicim = b;
      acik.kok = k.ad.replace(/\.(udf|docx)$/i, "");
      degisti = false;
      cubuguGuncelle();
      bildir(`Kaydedildi: ${k.yol}`);
      return;
    }
    acik.bicim = b;
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([bayt as BlobPart], { type: "application/octet-stream" }));
    a.download = ad;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
    degisti = false;
    bildir(`Kaydedildi: ${ad}`);
  } catch (e) {
    bildir(`Kaydedilemedi: ${e}`, "hata");
  }
}

// ── Araç çubuğu ─────────────────────────────────────────────────────────────────────────────────────

interface Dugme { ad: string; ipucu: string; komut?: Command; isaret?: string; hiza?: Hiza; liste?: string; simge?: string; eylem?: () => void }

// Simgeler (satır içi SVG, sabit metin): hizalama, liste
const cizgi = (d: string) => `<svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">${d}</svg>`;
const SIMGE = {
  sol: cizgi('<path d="M2 3h12M2 6.3h8M2 9.6h12M2 13h8"/>'),
  orta: cizgi('<path d="M2 3h12M4 6.3h8M2 9.6h12M4 13h8"/>'),
  sag: cizgi('<path d="M2 3h12M6 6.3h8M2 9.6h12M6 13h8"/>'),
  ikiYana: cizgi('<path d="M2 3h12M2 6.3h12M2 9.6h12M2 13h12"/>'),
  madde: cizgi('<circle cx="3" cy="4" r=".9" fill="currentColor"/><circle cx="3" cy="8" r=".9" fill="currentColor"/><circle cx="3" cy="12" r=".9" fill="currentColor"/><path d="M6.5 4H14M6.5 8H14M6.5 12H14"/>'),
  numara: cizgi('<path d="M2.4 2.8l1-.6v3.6M2 9.2c.3-.6 1.8-.7 1.8.3 0 .8-1.8 1.5-1.8 2.3h2M6.5 4H14M6.5 8H14M6.5 12H14"/>'),
};

const DUGMELER: (Dugme | "|")[] = [
  { ad: "Yeni", ipucu: "Yeni boş belge (Ctrl+N)", eylem: () => yeniBelge() },
  { ad: "Aç", ipucu: "UDF ya da Word (DOCX) aç (Ctrl+O)", eylem: () => acmaIste() },
  { ad: "Kaydet", ipucu: "Açıldığı biçimde kaydet (Ctrl+S)", eylem: () => kaydet() },
  { ad: "İmzala", ipucu: "Bilgisayara takılı e-imza kartıyla imzala (UDF)", eylem: () => imzalaIste() },
  "|",
  { ad: "↶", ipucu: "Geri al (Ctrl+Z)", komut: undo },
  { ad: "↷", ipucu: "Yinele (Ctrl+Y)", komut: redo },
  "|",
  { ad: "K", ipucu: "Kalın (Ctrl+B)", komut: kalin, isaret: "kalin" },
  { ad: "İ", ipucu: "İtalik (Ctrl+I)", komut: egik, isaret: "egik" },
  { ad: "A", ipucu: "Altı çizili (Ctrl+U)", komut: alti, isaret: "alti" },
  "|",
  { ad: "Sola", ipucu: "Sola hizala (Ctrl+L)", komut: hizala("Sol"), hiza: "Sol", simge: SIMGE.sol },
  { ad: "Ortala", ipucu: "Ortala (Ctrl+E)", komut: hizala("Orta"), hiza: "Orta", simge: SIMGE.orta },
  { ad: "Sağa", ipucu: "Sağa hizala (Ctrl+R)", komut: hizala("Sag"), hiza: "Sag", simge: SIMGE.sag },
  { ad: "İki yana", ipucu: "İki yana yasla (Ctrl+J)", komut: hizala("IkiYana"), hiza: "IkiYana", simge: SIMGE.ikiYana },
  "|",
  { ad: "⇤", ipucu: "Girintiyi azalt", komut: girinti(-1) },
  { ad: "⇥", ipucu: "Girintiyi artır", komut: girinti(1) },
  { ad: "Madde", ipucu: "Madde imli liste", komut: liste("madde"), liste: "Bulleted", simge: SIMGE.madde },
  { ad: "Numara", ipucu: "Numaralı liste", komut: liste("numara"), liste: "Numbered", simge: SIMGE.numara },
];

// Seçiciler: yazı tipi, boyut, renk, satır aralığı
const secici = {
  aile: document.createElement("select"),
  boyut: document.createElement("select"),
  renk: document.createElement("input"),
  aralik: document.createElement("select"),
  olcek: document.createElement("select"),
  tablo: document.createElement("select"),
  farkli: document.createElement("select"),
};

// Tablo menüsü: [değer, ad, komut, yalnız tabloda]
const TABLO_ISLERI: [string, string, Command, boolean][] = [
  ["ekle", "Tablo ekle (2×2)", tabloEkle(2, 2), false],
  ["ekle3", "Tablo ekle (3×3)", tabloEkle(3, 3), false],
  ["satirUst", "Üste satır ekle", satirEkle("ust"), true],
  ["satirAlt", "Alta satır ekle", satirEkle("alt"), true],
  ["sutunSol", "Sola sütun ekle", sutunEkle("sol"), true],
  ["sutunSag", "Sağa sütun ekle", sutunEkle("sag"), true],
  ["satirSil", "Satırı sil", satirSil, true],
  ["sutunSil", "Sütunu sil", sutunSil, true],
  ["tabloSil", "Tabloyu sil", tabloSil, true],
];

function seciciKur(c: HTMLElement) {
  const { aile, boyut, renk, aralik } = secici;
  aile.title = "Yazı tipi";
  for (const y of YAZI_TIPLERI) aile.append(new Option(y, y));
  aile.onchange = () => calistir(yaziDegistir("aile", aile.value));
  boyut.title = "Yazı boyutu";
  for (const b of BOYUTLAR) boyut.append(new Option(String(b), String(b)));
  boyut.onchange = () => calistir(yaziDegistir("boyut", Number(boyut.value)));
  renk.type = "color";
  renk.title = "Yazı rengi";
  renk.oninput = () => calistir(yaziDegistir("renk", hexRenk(renk.value)));
  aralik.title = "Satır aralığı";
  for (const [ad, d] of SATIR_ARALIKLARI) aralik.append(new Option(ad, String(d)));
  aralik.onchange = () => calistir(satirAraligi(Number(aralik.value)));
  const farkli = secici.farkli;
  farkli.title = "Farklı kaydet";
  farkli.append(new Option("Farklı kaydet", ""), new Option("UDF (UYAP) olarak", "udf"), new Option("Word (DOCX) olarak", "docx"));
  farkli.onchange = () => {
    const b = farkli.value as Bicim | "";
    farkli.value = "";
    if (b) kaydet(b);
  };
  const tablo = secici.tablo;
  tablo.title = "Tablo";
  tablo.append(new Option("Tablo", ""));
  for (const [d, ad] of TABLO_ISLERI) tablo.append(new Option(ad, d));
  tablo.onchange = () => {
    const is = TABLO_ISLERI.find(([d]) => d === tablo.value);
    tablo.value = "";
    if (is) calistir(is[2]);
  };
  const olcek = secici.olcek;
  for (const [d, ad] of OLCEKLER) olcek.append(new Option(ad, d));
  olcek.value = olcekSecimi;
  olcek.classList.add("olcek");
  olcek.onchange = () => olcekSec(olcek.value);
  for (const el of [farkli, aile, boyut, renk, aralik, tablo, olcek]) {
    el.classList.add("secici");
    c.append(el);
  }
}

function calistir(k: Command) {
  if (!gorunum) return;
  k(gorunum.state, gorunum.dispatch);
  gorunum.focus();
}

/** Seçicide olmayan bir değeri (belgedeki başka bir yazı tipi / boyut) geçici seçenek olarak gösterir. */
function seciciDeger(s: HTMLSelectElement, d: string) {
  if (![...s.options].some((o) => o.value === d)) s.append(new Option(d, d));
  s.value = d;
}

const dugmeler: { d: Dugme; el: HTMLButtonElement }[] = [];

function cubuguKur() {
  const c = $("#arac-cubugu");
  const girdi = document.createElement("input");
  girdi.type = "file";
  girdi.id = "dosya";
  girdi.accept = ".udf,.docx";
  girdi.hidden = true;
  girdi.onchange = () => girdi.files?.[0] && dosyaAc(girdi.files[0]);
  c.append(girdi);
  for (const d of DUGMELER) {
    if (d === "|") {
      c.append(Object.assign(document.createElement("span"), { className: "ayrac" }));
      continue;
    }
    const el = document.createElement("button");
    if (d.simge) {
      el.innerHTML = d.simge; // sabit SVG (kullanıcı/belge verisi değil)
      el.setAttribute("aria-label", d.ad);
    } else el.textContent = d.ad;
    el.title = d.ipucu;
    if (d.isaret) el.classList.add(`isaret-${d.isaret}`);
    el.onmousedown = (e) => e.preventDefault(); // editör odağı kaçmasın
    el.onclick = () => {
      if (d.eylem) return d.eylem();
      if (gorunum && d.komut) {
        d.komut(gorunum.state, gorunum.dispatch);
        gorunum.focus();
      }
    };
    c.append(el);
    dugmeler.push({ d, el });
  }
  c.append(Object.assign(document.createElement("span"), { className: "ayrac" }));
  seciciKur(c);
  const durum = Object.assign(document.createElement("span"), { id: "durum" });
  const sayi = Object.assign(document.createElement("span"), { id: "sayfa-sayisi" });
  c.append(durum, sayi);
}

function cubuguGuncelle() {
  const st = gorunum?.state;
  for (const { d, el } of dugmeler) {
    if (!st) {
      el.disabled = !d.eylem || d.ad === "Kaydet";
      continue;
    }
    el.disabled = false;
    if (d.isaret) {
      const tip = sema.marks[d.isaret];
      const { from, to, empty, $from } = st.selection;
      const etkin = empty ? !!tip.isInSet(st.storedMarks || $from.marks()) : st.doc.rangeHasMark(from, to, tip);
      el.classList.toggle("etkin", etkin);
    }
    if (d.hiza) el.classList.toggle("etkin", st.selection.$from.parent.attrs.hiza === d.hiza);
    if (d.liste) el.classList.toggle("etkin", ozAl(st.selection.$from.parent.attrs.oz as Ozellikler, d.liste) === "true");
  }
  for (const el of Object.values(secici)) el.disabled = !st && el !== secici.olcek;
  if (st) {
    const ici = tablodaMi(st);
    for (const o of secici.tablo.options) {
      const is = TABLO_ISLERI.find(([d]) => d === o.value);
      if (is) o.disabled = is[3] ? !ici : ici;
    }
  }
  if (st) {
    const y = etkinYazi(st);
    if (y) {
      seciciDeger(secici.aile, y.aile);
      seciciDeger(secici.boyut, String(y.boyut));
      secici.renk.value = renkHex(y.renk);
    }
    // Belgedeki değer seçeneklerden birine çok yakınsa (UYAP 0.14999998 yazar) onu göster
    const sa = st.selection.$from.parent.attrs.satirAraligi as number;
    const yakin = SATIR_ARALIKLARI.find(([, d]) => Math.abs(d - sa) < 0.005);
    if (yakin) secici.aralik.value = String(yakin[1]);
    else {
      const d = String(sa);
      if (![...secici.aralik.options].some((o) => o.value === d)) secici.aralik.append(new Option((1 + sa).toFixed(2).replace(".", ","), d));
      secici.aralik.value = d;
    }
  }
  $("#durum").textContent = acik ? `${acik.kok}.${acik.bicim}${degisti ? " •" : ""}` : "";
}

window.addEventListener("beforeunload", (e) => {
  if (degisti) e.preventDefault();
});

// Sürükle-bırak ile açma
window.addEventListener("dragover", (e) => e.preventDefault());
window.addEventListener("drop", (e) => {
  e.preventDefault();
  const f = e.dataTransfer?.files?.[0];
  if (f) dosyaAc(f);
});

void setBlockType; // ileride başlık stilleri

await init();
cubuguKur();
cubuguGuncelle();
boyutDegisti();
// Masaüstü: komut satırından (çift tıkla) gelen dosya
if (masaustu) {
  cagir<MasaustuDosya | null>("baslangic_dosyasi")
    .then((d) => d && belgeAc(d.ad, b64Coz(d.bayt), d.kimlik))
    .catch(() => {});
}
new ResizeObserver(() => boyutDegisti()).observe($("#masa"));
// Ctrl + tekerlek: %10 adımlarla yakınlaştır
$("#masa").addEventListener("wheel", (e) => {
  if (!e.ctrlKey) return;
  e.preventDefault();
  const yeni = Math.round(Math.min(400, Math.max(20, sonOlcek * 100 + (e.deltaY < 0 ? 10 : -10))));
  olcekSec(String(yeni));
}, { passive: false });

// Yalnız geliştirme sunucusunda: ?ornek=/@fs/<yol> ile belge açılır (görsel test; derlenmiş sürümde yok)
if (import.meta.env.DEV) {
  const ornek = new URLSearchParams(location.search).get("ornek");
  const olcekParam = new URLSearchParams(location.search).get("olcek");
  if (olcekParam) olcekSec(olcekParam);
  if (ornek) {
    fetch(ornek).then((r) => r.blob()).then((b) => dosyaAc(new File([b], ornek.split("/").pop() || "ornek.udf")));
  }
}
