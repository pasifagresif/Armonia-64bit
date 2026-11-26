// Armonia Yazı: editör arayüzü (ilk sürüm). UDF açar, sayfa genişliğinde gösterir, metni ve temel biçimi
// (kalın, italik, altı çizili, hizalama) düzenler, UDF olarak kaydeder. Üst/alt bilgi ve tablolar şimdilik salt
// okunur (korunur). Sayfalara bölme sonraki adım. Claude, 2026-11-23.

import { baseKeymap, chainCommands, setBlockType, splitBlockAs, toggleMark } from "prosemirror-commands";
import { history, redo, undo } from "prosemirror-history";
import { keymap } from "prosemirror-keymap";
import { EditorState, type Command } from "prosemirror-state";
import { EditorView } from "prosemirror-view";

import { bloklariCiz } from "./cizim";
import { belgeDugumu, belgeyeCevir } from "./donustur";
import { BOYUTLAR, SATIR_ARALIKLARI, YAZI_TIPLERI, etkinYazi, girinti, liste, onekleriYenile, satirAraligi, sekmeleriHizala, yaziDegistir } from "./komutlar";
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
  /** "Yeni" ile açılmış, henüz dosyası olmayan belge (dokunulmadıysa açılan ilk belge onun yerine geçer) */
  yeni?: boolean;
}

/** Açık belgelerden biri (kullanıcı isteği 26.11: tek pencerede birden çok belge). Her sekmenin kendi sayfa
 * kabı ve kendi editörü (geri alma geçmişiyle) vardır; arkadaki sekmeler gizli durur, yeniden kurulmaz. */
interface Sekme {
  acik: Acik;
  gorunum: EditorView;
  kap: HTMLElement;
  degisti: boolean;
  olcekUygula: () => void;
  sayfaSayisi: number;
  varsayilanYazi: { aile: string; boyut: number };
  /** Bildirim şeridinin bu belgeye ait içeriği (sekme arkadayken burada saklanır) */
  bildirim: { sinif: string; dugumler: Node[] } | null;
  kaydirma: [number, number];
  onekZamani: number;
}

let sekmeler: Sekme[] = [];
let etkin: Sekme | null = null;
// Etkin sekmenin editörü (etkinlestir() eşitler)
let gorunum: EditorView | null = null;
/** Biçimsiz metnin yazısı (belgenin hvl-default stili): seçiciler biçim işareti yoksa bunu gösterir. */
let varsayilanYazi = { aile: "Times New Roman", boyut: 12 };

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

/** Enter (UYAP'ta olduğu gibi): yeni paragraf öncekinin özelliklerini (sekme durakları, hizalama, girinti, aralık;
 * UDF'e de yazılsın diye ham `oz` ile) ve imleçteki yazı biçimini alır. Varsayılan `splitBlock` paragrafı varsayılan
 * özelliklerle açıyordu: sekme durakları gidince Tab her 72 pt'ye düşüyor, alt alta hizalar kayıyordu. */
const paragrafBol: Command = (state, dispatch) =>
  splitBlockAs((n) => (n.type === sema.nodes.paragraph
    ? { type: n.type, attrs: { ...n.attrs, satirSonuOz: null, onek: null } }
    : null))(state, dispatch && ((tr) => {
    const isaretler = state.storedMarks || (state.selection.$to.parentOffset && state.selection.$from.marks());
    if (isaretler) tr.ensureMarks(isaretler);
    dispatch(tr);
  }));

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

/** Belgenin sayfa kabını ve editörünü kurar; sekmeyi döndürür (listeye eklemek ve öne almak çağırana ait).
 * Kap görünür eklenir (ölçümler gizli öğede yapılamaz): çağıran, önceki sekmeyi önceden gizlemiş olmalı. */
function sayfayiKur(a: Acilis, bilgi: Acik): Sekme {
  const s = a.gorunum.sayfa;
  const masa = $("#masa");
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
  const sekme = {
    acik: bilgi, kap, degisti: false, sayfaSayisi: 1, bildirim: null, kaydirma: [0, 0], onekZamani: 0,
    varsayilanYazi: { aile: "Times New Roman", boyut: 12 }, olcekUygula: () => {},
  } as unknown as Sekme;
  // Yakınlaştırma: "genislik" / "sayfa" pencereye göre hesaplanır
  sekme.olcekUygula = () => {
    const cs = getComputedStyle(masa);
    const bosW = masa.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    const bosH = masa.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
    const o = olcekSecimi === "genislik" ? bosW / sayfaW
      : olcekSecimi === "sayfa" ? Math.min(bosW / sayfaW, bosH / sayfaH)
      : Number(olcekSecimi) / 100;
    const yeni = Math.min(4, Math.max(0.2, o || 1));
    olcu.olcek = yeni;
    sayfa.style.transform = `scale(${yeni})`;
    kap.style.width = `${sayfaW * yeni}px`;
    kap.style.height = `${toplamH * yeni}px`;
    if (etkin === sekme || !etkin) {
      sonOlcek = yeni;
      secici.olcek.title = `Yakınlaştırma: %${Math.round(yeni * 100)}`;
    }
    const g = sekme.gorunum as EditorView | undefined;
    g?.dispatch(g.state.tr.setMeta(YENIDEN_OLC, true).setMeta("addToHistory", false));
  };
  govde.style.paddingTop = `${olcu.yaziUstu}px`;
  // Biçimsiz metin (yeni paragraf, yeni tablo hücresi) UDF'te belgenin varsayılan stilini alır (hvl-default;
  // stil.rs ile aynı varsayılanlar: Times New Roman 12): ekranda da öyle görünsün
  const vs = a.belge.stiller.find((st) => st.ad === "hvl-default")?.oz ?? [];
  sekme.varsayilanYazi = { aile: ozAl(vs, "family") || "Times New Roman", boyut: Number(ozAl(vs, "size")) > 0 ? Number(ozAl(vs, "size")) : 12 };
  govde.style.fontFamily = cssAilesi(sekme.varsayilanYazi.aile);
  govde.style.fontSize = `${sekme.varsayilanYazi.boyut}pt`;

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
    sekme.sayfaSayisi = n;
    if (etkin === sekme) $("#sayfa-sayisi").textContent = `${n} sayfa`;
  };

  const doc = belgeDugumu(a.belge, a.gorunum);
  // Kayıpsızlık denetimi: hiç dokunulmamış belge modele aynen dönmeli
  const geri = belgeyeCevir(doc, a.belge);
  const ayni = JSON.stringify(geri) === JSON.stringify(a.belge);
  console.info(`[Armonia Yazı] gidiş-dönüş denetimi: ${ayni ? "TAMAM" : "FARK VAR"}`);
  if (!ayni) bildir("Uyarı: bu belgenin bazı öğeleri editörde birebir korunamıyor; kaydetmeden önce kontrol edin.", "uyari");

  const view: EditorView = new EditorView(govde, {
    state: EditorState.create({
      doc,
      plugins: [
        history(),
        keymap({
          "Mod-z": undo, "Mod-y": redo, "Mod-Shift-z": redo,
          "Mod-b": kalin, "Mod-i": egik, "Mod-u": alti,
          "Mod-s": () => { kaydet(); return true; },
          "Mod-n": () => { yeniBelge(); return true; },
          "Mod-w": () => { if (etkin) sekmeKapat(etkin); return true; },
          "Mod-PageDown": () => { komsuSekme(1); return true; },
          "Mod-PageUp": () => { komsuSekme(-1); return true; },
          "Mod-o": () => { acmaIste(); return true; },
          "Enter": paragrafBol,
          "Tab": chainCommands(hucreGec(1), sekmeEkle),
          "Shift-Tab": hucreGec(-1),
          "Mod-l": hizala("Sol"), "Mod-e": hizala("Orta"), "Mod-r": hizala("Sag"), "Mod-j": hizala("IkiYana"),
        }),
        keymap(baseKeymap),
        sayfaEklentisi(() => olcu, katmaniCiz),
      ],
    }),
    // İmzalı belge salt okunur (acik.salt); "Düzenlemeyi aç" ile açılır
    editable: () => !sekme.acik.salt,
    dispatchTransaction(tr) {
      view.updateState(view.state.apply(tr));
      // Liste önekleri ve sayfa boşlukları belgeyi "değişmiş" yapmaz (kaydedilen içerik aynı)
      if (tr.docChanged && tr.getMeta("addToHistory") !== false) {
        const ilk = !sekme.degisti;
        sekme.degisti = true;
        sekme.acik.yeni = false;
        clearTimeout(sekme.onekZamani);
        sekme.onekZamani = window.setTimeout(() => sekmeler.includes(sekme) && onekleriYenile(view, sekme.acik.belge), 250);
        if (ilk) sekmeCubuguCiz();
      }
      if (etkin === sekme) cubuguGuncelle();
    },
  });
  sekme.gorunum = view;
  sekme.olcekUygula();
  return sekme;
}

// ── Sekmeler ────────────────────────────────────────────────────────────────────────────────────────

function sekmeAdi(s: Sekme): string {
  return `${s.acik.kok}.${s.acik.bicim}`;
}

/** Etkin sekmeyi arkaya alır: kaydırma konumu ve bildirim şeridi sekmede saklanır, sayfa kabı gizlenir. */
function etkiniGizle() {
  if (!etkin || etkin.kap.hidden) return; // zaten arkada: saklanan bildirim/kaydırma ezilmesin
  const masa = $("#masa"), b = $("#bildirim");
  etkin.kaydirma = [masa.scrollLeft, masa.scrollTop];
  etkin.bildirim = b.hidden ? null : { sinif: b.className, dugumler: [...b.childNodes] };
  b.replaceChildren();
  b.hidden = true;
  etkin.kap.hidden = true;
}

/** Sekmeyi öne alır (null: açık belge kalmadı, karşılama görünür). */
function etkinlestir(s: Sekme | null) {
  if (etkin !== s) etkiniGizle();
  etkin = s;
  gorunum = s?.gorunum ?? null;
  $("#karsilama").hidden = !!s;
  const b = $("#bildirim");
  if (s) {
    s.kap.hidden = false;
    if (s.bildirim) {
      b.className = s.bildirim.sinif;
      b.replaceChildren(...s.bildirim.dugumler);
      b.hidden = false;
      s.bildirim = null;
    }
    varsayilanYazi = s.varsayilanYazi;
    olcekUygula = s.olcekUygula;
    olcekUygula(); // pencere boyutu sekme arkadayken değişmiş olabilir; sayfa sonları da yeniden ölçülür
    $("#masa").scrollTo(s.kaydirma[0], s.kaydirma[1]);
    $("#sayfa-sayisi").textContent = `${s.sayfaSayisi} sayfa`;
    document.title = `${sekmeAdi(s)} — Armonia Yazı`;
    s.gorunum.focus();
  } else {
    olcekUygula = () => {};
    $("#sayfa-sayisi").textContent = "";
    document.title = "Armonia Yazı";
  }
  cubuguGuncelle();
  sekmeCubuguCiz();
}

/** Bildirim şeridine yazar: sekme öndeyse doğrudan, arkadaysa sekmeye saklar (öne gelince görünür). */
function bildirimKoy(s: Sekme, sinif: string, dugumler: Node[]) {
  if (etkin !== s) {
    s.bildirim = { sinif, dugumler };
    return;
  }
  const b = $("#bildirim");
  b.className = sinif;
  b.replaceChildren(...dugumler);
  b.hidden = false;
}

/** Yeni kurulan sekmeyi listeye koyar ve öne alır. `yerine` verilirse o sekmenin yerini alır (eski kapanır). */
function sekmeEkle_(s: Sekme, yerine?: Sekme | null) {
  const sira = yerine ? sekmeler.indexOf(yerine) : -1;
  if (yerine && sira >= 0) {
    clearTimeout(yerine.onekZamani);
    yerine.gorunum.destroy();
    yerine.kap.remove();
    sekmeler = sekmeler.map((x) => (x === yerine ? s : x));
    if (etkin === yerine) {
      // Eski sekme zaten gizlendi (sayfa kurulmadan önce); bildirimi yeni belgeye taşınmaz
      etkin = null;
    }
  } else {
    sekmeler = [...sekmeler, s];
  }
  etkinlestir(s);
}

function sekmeKapat(s: Sekme) {
  if (s.degisti && !window.confirm(`"${sekmeAdi(s)}" kaydedilmedi. Yine de kapatılsın mı?`)) return;
  const sira = sekmeler.indexOf(s);
  if (sira < 0) return;
  if (etkin === s) etkiniGizle();
  clearTimeout(s.onekZamani);
  s.gorunum.destroy();
  s.kap.remove();
  sekmeler = sekmeler.filter((x) => x !== s);
  if (etkin === s) {
    etkin = null;
    etkinlestir(sekmeler[Math.min(sira, sekmeler.length - 1)] ?? null);
  } else sekmeCubuguCiz();
}

function komsuSekme(yon: 1 | -1) {
  if (!etkin || sekmeler.length < 2) return;
  const n = sekmeler.length;
  etkinlestir(sekmeler[(sekmeler.indexOf(etkin) + yon + n) % n]);
}

function sekmeCubuguCiz() {
  const c = $("#sekmeler");
  c.hidden = sekmeler.length === 0;
  const ogeler: HTMLElement[] = sekmeler.map((s) => {
    const el = document.createElement("div");
    el.className = s === etkin ? "sekme-basligi etkin" : "sekme-basligi";
    el.title = sekmeAdi(s) + (s.degisti ? " (kaydedilmedi)" : "");
    const ad = document.createElement("span");
    ad.className = "ad";
    ad.textContent = sekmeAdi(s) + (s.degisti ? " •" : "");
    const kapat = document.createElement("button");
    kapat.textContent = "×";
    kapat.title = "Sekmeyi kapat (Ctrl+W)";
    kapat.setAttribute("aria-label", `${sekmeAdi(s)} sekmesini kapat`);
    kapat.onclick = (e) => { e.stopPropagation(); sekmeKapat(s); };
    el.onmousedown = (e) => e.preventDefault(); // editör odağı kaçmasın
    el.onclick = () => etkinlestir(s);
    el.onauxclick = (e) => { if (e.button === 1) sekmeKapat(s); };
    el.append(ad, kapat);
    return el;
  });
  const arti = document.createElement("button");
  arti.className = "sekme-yeni";
  arti.textContent = "+";
  arti.title = "Yeni boş belge (Ctrl+N)";
  arti.onmousedown = (e) => e.preventDefault();
  arti.onclick = () => yeniBelge();
  c.replaceChildren(...ogeler, arti);
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

/** "Aç": tarayıcıda dosya seçici. Masaüstünde yerli dosya penceresi yok (armonia-yazi@.service'te D-Bus/portal
 * kasıtlı olarak kapalı, denetçi 24.11 O4); belge Armonia Dosyalar'da çift tıklanarak açılır (kullanıcı kararı
 * 25.11). Claude, 2026-11-25. */
async function acmaIste() {
  if (!masaustu) {
    $<HTMLInputElement>("#dosya").click();
    return;
  }
  bildir("Belgeyi Armonia Dosyalar'da çift tıklayarak açın.", "bilgi");
}

/** "İmzala": belgenin kaydedilecek son hâli (UDF) karttaki sertifikayla imzalanır, imzalı UDF kaydedilir ve
 * salt okunur açılır. Word olarak açılmış belge de UDF olarak imzalanır (UYAP UDF ister). */
function imzalaIste() {
  const s = etkin;
  if (!s) return;
  if (!masaustu) {
    bildir("E-imza yalnız Armonia Yazı masaüstü uygulamasında (bilgisayara takılı kartla) kullanılabilir.", "uyari");
    return;
  }
  const kok = s.acik.kok;
  imzaPenceresiAc({
    dosyaAdi: `${kok}.udf`,
    belge: () => udf_kaydet(belgeyeCevir(s.gorunum.state.doc, s.acik.belge)) as Uint8Array,
    bitti: async (imzali, bilgi) => {
      try {
        const k = await cagir<MasaustuKayit | null>("dosya_kaydet", {
          onerilen: `${kok}.udf`, bayt: b64Kodla(imzali), bicim: "udf", kimlik: s.acik.bicim === "udf" ? s.acik.kimlik ?? null : null,
        });
        // İmzalı hâli aynı sekmede (salt okunur) açılır
        s.degisti = false;
        await belgeAc(k ? k.ad : `${kok}.udf`, imzali, k?.kimlik, s);
        if (!k) bildir(`İmzalandı (${bilgi.imzalayan}) ama kaydedilmedi. Kaydetmek için Ctrl+S kullanın.`, "uyari");
      } catch (e) {
        bildir(`İmzalandı ama kaydedilemedi: ${e}`, "hata");
      }
    },
  });
}

/** Dokunulmamış boş "Yeni belge" sekmesi mi (açılan belge onun yerine geçer, yanına ikincisi açılmaz). */
function bosYeniMi(s: Sekme | null): s is Sekme {
  return !!s && !!s.acik.yeni && !s.degisti;
}

/** Yeni boş belgeyi yeni sekmede açar. `tekrarsiz`: önde zaten dokunulmamış boş belge varsa yenisi açılmaz
 * (Armonia Yazı açıkken kısayoluna yeniden basılması). */
function yeniBelge(tekrarsiz = false) {
  if (tekrarsiz && bosYeniMi(etkin)) return;
  const belge = bosBelgeOlustur();
  const g = gorunum_hesapla(belge) as Acilis["gorunum"];
  etkiniGizle();
  const s = sayfayiKur({ belge, gorunum: g, imzali: false, uyarilar: [] },
    { belge, kok: "Yeni belge", bicim: "udf", gorunum: g, salt: false, yeni: true });
  sekmeEkle_(s);
}

async function dosyaAc(f: File) {
  await belgeAc(f.name, new Uint8Array(await f.arrayBuffer()));
}

/** Belgeyi baytlarından yeni sekmede açar (tarayıcı: dosya seçici / sürükle-bırak; masaüstü: çift tık).
 * Aynı dosya zaten açıksa o sekme öne alınır. `yerine`: belge o sekmenin yerine açılır (imzalama sonrası). */
async function belgeAc(ad: string, bayt: Uint8Array, kimlik?: number, yerine?: Sekme) {
  if (kimlik != null) {
    const acikOlan = sekmeler.find((x) => x !== yerine && x.acik.kimlik === kimlik);
    if (acikOlan) {
      etkinlestir(acikOlan);
      bildir("Bu belge zaten açık.", "bilgi");
      return;
    }
  }
  try {
    // Tür içerikten anlaşılır (uzantıya güvenilmez): UDF ya da DOCX
    const a = belge_ac(bayt) as Acilis & { tur: "udf" | "docx" };
    etkiniGizle();
    const s = sayfayiKur(a, { belge: a.belge, kok: ad.replace(/\.(udf|docx)$/i, ""), bicim: a.tur, gorunum: a.gorunum, kimlik, salt: a.imzali });
    sekmeEkle_(s, yerine ?? (bosYeniMi(etkin) ? etkin : null));
    const notlar: string[] = [];
    if (a.uyarilar.length) notlar.push("Korunamayan öğeler: " + a.uyarilar.join("; "));
    if (a.imzali) await imzaBilgisiGoster(s, bayt, notlar.join(" "));
    else if (notlar.length) bildir(notlar.join(" "), "uyari");
  } catch (e) {
    // Açılamadıysa önceki sekme (bildirimi ve kaydırma konumuyla) geri gelsin
    if (etkin?.kap.hidden) etkinlestir(etkin);
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
async function imzaBilgisiGoster(s: Sekme, bayt: Uint8Array, ek: string) {
  let sinif = "imza";
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
    if (!bilgi.belge_degismemis || !bilgi.imza_gecerli) sinif = "imza hata";
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
    if (!window.confirm("Düzenlerseniz imza geçersiz olur ve belgeyi yeniden imzalamanız gerekir. Devam edilsin mi?")) return;
    s.acik.salt = false;
    s.gorunum.setProps({});
    $("#bildirim").hidden = true;
    cubuguGuncelle();
  };
  bildirimKoy(s, sinif, [metin, dugme]);
}

/** Kaydeder: verilen biçimde (yoksa açılan biçimde). Word'e kaydederken UYAP'a özgü öğeler (alanlar) düz metin olur. */
async function kaydet(bicim?: Bicim) {
  const s = etkin;
  if (!s) return;
  const acik = s.acik, gorunum = s.gorunum;
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
      acik.yeni = false;
      s.degisti = false;
      if (etkin === s) {
        document.title = `${sekmeAdi(s)} — Armonia Yazı`;
        cubuguGuncelle();
        bildir(`Kaydedildi: ${k.yol}`);
      }
      sekmeCubuguCiz();
      return;
    }
    acik.bicim = b;
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([bayt as BlobPart], { type: "application/octet-stream" }));
    a.download = ad;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
    s.degisti = false;
    acik.yeni = false;
    cubuguGuncelle();
    sekmeCubuguCiz();
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
  { ad: "⇥:", ipucu: "Tab'lı satırları ortak hizaya al (DAVACI ⇥ : …): imlecin bulunduğu satır bloğu ya da seçim", eylem: () => ortakHiza() },
];

function ortakHiza() {
  if (!gorunum || !gorunum.editable) return;
  const s = sekmeleriHizala(gorunum);
  bildir(s.ileti, s.tamam ? "bilgi" : "uyari");
  gorunum.focus();
}

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
  girdi.multiple = true;
  girdi.hidden = true;
  girdi.onchange = async () => {
    for (const f of [...(girdi.files ?? [])]) await dosyaAc(f);
    girdi.value = ""; // aynı dosya yeniden seçilebilsin
  };
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
    // Biçim işareti yoksa metin belgenin varsayılan yazısıyla çizilir (sayfayiKur): seçiciler de onu göstersin
    // (önceden eski değerde / listenin ilk seçeneğinde, ör. "8", kalıyordu)
    seciciDeger(secici.aile, y ? y.aile : varsayilanYazi.aile);
    seciciDeger(secici.boyut, String(y ? y.boyut : varsayilanYazi.boyut));
    secici.renk.value = y ? renkHex(y.renk) : "#000000";
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
  $("#durum").textContent = etkin ? `${sekmeAdi(etkin)}${etkin.degisti ? " •" : ""}` : "";
}

window.addEventListener("beforeunload", (e) => {
  if (sekmeler.some((s) => s.degisti)) e.preventDefault();
});

// Sürükle-bırak ile açma
window.addEventListener("dragover", (e) => e.preventDefault());
window.addEventListener("drop", (e) => {
  e.preventDefault();
  const dosyalar = [...(e.dataTransfer?.files ?? [])];
  void (async () => {
    for (const f of dosyalar) await dosyaAc(f);
  })();
});

void setBlockType; // ileride başlık stilleri

await init();
cubuguKur();
cubuguGuncelle();
boyutDegisti();
// Masaüstü: Armonia Yazı açıkken çift tıklanan başka belge / kısayola yeniden basılması, yeni pencere yerine bu
// pencereye devredilir (kabuk: tek_ornek.rs) ve yeni sekmede açılır
if (masaustu) {
  const { listen } = await import("@tauri-apps/api/event");
  await listen<MasaustuDosya>("dosya-ac", (o) => { void belgeAc(o.payload.ad, b64Coz(o.payload.bayt), o.payload.kimlik); });
  await listen("yeni-belge", () => yeniBelge(true));
  await listen<string>("ac-hata", (o) => bildir(`Açılamadı: ${o.payload}`, "hata"));
}
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
