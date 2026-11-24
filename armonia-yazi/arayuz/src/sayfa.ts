// Sayfa düzeni: sekme genişlikleri ve A4 sayfalara bölme. Claude, 2026-11-23.
//
// Sekmeler (udf_pdf.rs ile aynı): konum paragrafın sol girintisine göre ölçülür; sonraki durak `TabSet`'ten
// (yoksa her 72 pt). Durak hizası 1/4 sağa, 2 ortaya: sekmeden sonraki metin (bir sonraki sekmeye kadar) durağın
// soluna / ortasına yaslanır.
//
// Sayfalar: editör tek bir akıştır. Satırlar ölçülür; bir satır sayfanın yazı alanından taşıyorsa önüne bir
// boşluk (widget) konur, satır bir sonraki sayfanın yazı alanının başına iner. Boşluk paragraf içinde de olabilir
// (paragraf sayfa sınırında bölünür). Sayfa kenarları, üst/alt bilgi ve sayfa numarası ayrı bir katmanda çizilir.
// Düzen (udf_pdf.rs): yazı alanı üstü = max(üst marj, üst bilgi payı + üst bilgi yüksekliği); altı = min(sayfa −
// alt marj, sayfa − alt bilgi payı − alt bilgi yüksekliği).

import { Plugin, PluginKey, type EditorState } from "prosemirror-state";
import { Decoration, DecorationSet, type EditorView } from "prosemirror-view";

export const PT = 96 / 72; // 1 pt kaç CSS pikseli
export const ARALIK = 28; // sayfalar arası boşluk (px)

export interface SayfaOlcusu {
  /** px */
  yukseklik: number;
  yaziUstu: number;
  yaziAlti: number;
  /** Ekran ölçeği (yakınlaştırma). Ekrandan okunan uzunluklar buna bölünür; düzen ölçekten bağımsızdır. */
  olcek: number;
}

export interface Kirilma {
  pos: number;
  /** boşluk yüksekliği (px) */
  h: number;
  /** paragrafın ilk satırı: boşluk paragraftan önce (blok düzeyi) */
  blok: boolean;
}

interface Durum {
  kirilmalar: Kirilma[];
  dekor: DecorationSet;
}

export const sayfaAnahtari = new PluginKey<Durum>("sayfa");
/** tr.setMeta(YENIDEN_OLC, true): sayfa düzenini yeniden ölçtürür. */
export const YENIDEN_OLC = "sayfaYenidenOlc";

function boslukDom(h: number, blok: boolean): HTMLElement {
  const el = document.createElement(blok ? "div" : "span");
  el.className = "sayfa-boslugu";
  el.style.display = "block";
  el.style.height = `${h}px`;
  el.dataset.h = String(h);
  el.contentEditable = "false";
  return el;
}

function dekorlar(state: EditorState, k: Kirilma[]): DecorationSet {
  return DecorationSet.create(
    state.doc,
    k.map((x) => Decoration.widget(x.pos, () => boslukDom(x.h, x.blok), { side: -1, key: `s${x.pos}-${Math.round(x.h)}`, ignoreSelection: true })),
  );
}

// ── Sekmeler ────────────────────────────────────────────────────────────────────────────────────────

interface Durak { konum: number; hiza: number }

function sonrakiDurak(x: number, duraklar: Durak[]): Durak {
  if (!duraklar.length) return { konum: (Math.floor(x / 72) + 1) * 72, hiza: 0 };
  return duraklar.find((d) => d.konum > x + 0.01) ?? { konum: x + 5, hiza: 0 };
}

/** Sekmeden sonra, bir sonraki sekmeye (ya da paragraf sonuna) kadarki metnin genişliği (px). */
function segmentGenisligi(sekme: HTMLElement, olcek: number): number {
  const r = document.createRange();
  r.setStartAfter(sekme);
  const p = sekme.closest(".p")!;
  let son: Node | null = null;
  const yuru = document.createTreeWalker(p, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  yuru.currentNode = sekme;
  let n: Node | null;
  while ((n = yuru.nextNode())) {
    if (n instanceof HTMLElement && (n.classList.contains("sekme") || n.classList.contains("sayfa-boslugu"))) break;
    son = n;
  }
  if (!son) return 0;
  r.setEndAfter(son);
  return r.getBoundingClientRect().width / olcek;
}

export function sekmeleriYerlestir(kok: HTMLElement, olcek = 1) {
  for (const p of kok.querySelectorAll<HTMLElement>(".p")) {
    const sekmeler = p.querySelectorAll<HTMLElement>(".sekme");
    if (!sekmeler.length) continue;
    let duraklar: Durak[] = [];
    try {
      duraklar = JSON.parse(p.dataset.sekmeler || "[]");
    } catch {
      duraklar = [];
    }
    const cs = getComputedStyle(p);
    for (const s of sekmeler) {
      s.style.width = "0px";
      const solKenar = p.getBoundingClientRect().left + parseFloat(cs.paddingLeft) * olcek;
      const x = (s.getBoundingClientRect().left - solKenar) / olcek / PT; // pt, sol girintiye göre
      const d = sonrakiDurak(x, duraklar);
      let g = d.konum - x;
      if (d.hiza === 1 || d.hiza === 2 || d.hiza === 4) {
        const seg = segmentGenisligi(s, olcek) / PT;
        g = Math.max(0, g - (d.hiza === 2 ? seg / 2 : seg));
      }
      s.style.width = `${Math.max(0, g) * PT}px`;
    }
  }
}

// ── Satır ölçümü ────────────────────────────────────────────────────────────────────────────────────

interface Satir {
  ust: number;
  alt: number;
  pos: number;
  blok: boolean;
}

/** Görünür satırlar, akıştaki boşluklar çıkarılmış ("içerik") koordinatlarla, belge sırasıyla. */
function satirlariOlc(view: EditorView, olcek: number): Satir[] {
  const kok = view.dom as HTMLElement;
  const bas = kok.getBoundingClientRect().top;
  const bosluklar = [...kok.querySelectorAll<HTMLElement>(".sayfa-boslugu")].map((b) => {
    const r = b.getBoundingClientRect();
    return { ust: r.top, alt: r.bottom, h: r.height };
  });
  // Ekran (ölçekli) konum → içerik konumu: akıştaki boşluklar çıkarılır, ölçeğe bölünür
  const icerik = (y: number) => (y - bas - bosluklar.filter((b) => b.alt <= y + 0.5).reduce((t, b) => t + b.h, 0)) / olcek;
  const satirlar: Satir[] = [];
  const doc = view.state.doc;
  doc.forEach((dugum, ofset) => {
    const dom = view.nodeDOM(ofset) as HTMLElement | null;
    if (!dom) return;
    if (dugum.type.name === "tablo") {
      // Tablo satır aralarından bölünür: her tablo satırı bir birim; ilk satırın boşluğu tablonun önüne (üst
      // boşluğu da insin), ötekilerinki satırın önüne (tablonun içinde, satırlar arasında)
      const tcs = getComputedStyle(dom);
      const tabloUst = parseFloat(tcs.marginTop) || 0;
      dugum.forEach((_s, sOfset, i) => {
        const sPos = ofset + 1 + sOfset;
        const sDom = view.nodeDOM(sPos) as HTMLElement | null;
        if (!sDom) return;
        const r = sDom.getBoundingClientRect();
        satirlar.push(i === 0
          ? { ust: icerik(r.top) - tabloUst, alt: icerik(r.bottom), pos: ofset, blok: true }
          : { ust: icerik(r.top), alt: icerik(r.bottom), pos: sPos, blok: true });
      });
      return;
    }
    if (dugum.type.name !== "paragraph") {
      const r = dom.getBoundingClientRect();
      satirlar.push({ ust: icerik(r.top), alt: icerik(r.bottom), pos: ofset, blok: true });
      return;
    }
    const cs = getComputedStyle(dom);
    const ustBosluk = parseFloat(cs.marginTop) || 0; // CSS değeri: ölçeksiz
    const aralik = document.createRange();
    aralik.selectNodeContents(dom);
    const kutular = [...aralik.getClientRects()].filter((r) => r.height > 0 && !bosluklar.some((b) => r.top >= b.ust - 0.5 && r.bottom <= b.alt + 0.5));
    if (!kutular.length) {
      const r = dom.getBoundingClientRect();
      satirlar.push({ ust: icerik(r.top) - ustBosluk, alt: icerik(r.bottom), pos: ofset, blok: true });
      return;
    }
    kutular.sort((a, b) => a.top - b.top || a.left - b.left);
    const gruplar: { ust: number; alt: number; sol: number }[] = [];
    for (const r of kutular) {
      const g = gruplar[gruplar.length - 1];
      if (g && r.top < g.alt - 2) {
        g.ust = Math.min(g.ust, r.top);
        g.alt = Math.max(g.alt, r.bottom);
        g.sol = Math.min(g.sol, r.left);
      } else gruplar.push({ ust: r.top, alt: r.bottom, sol: r.left });
    }
    // Satır başlarının belgedeki yeri metnin kendisinden bulunur (posAtCoords ölçekli görünümde yanılıyor):
    // k. satır grubu ↔ k. satır başı
    const baslar = satirBaslari(view, dom);
    gruplar.forEach((g, i) => {
      if (i === 0) {
        const r = dom.getBoundingClientRect();
        satirlar.push({ ust: icerik(Math.min(r.top, g.ust)) - ustBosluk, alt: icerik(g.alt), pos: ofset, blok: true });
        return;
      }
      const ham = baslar[i] ?? baslar[baslar.length - 1] ?? ofset + 1;
      const pos = Math.max(ofset + 1, Math.min(ham, ofset + dugum.nodeSize - 1));
      satirlar.push({ ust: icerik(g.ust), alt: icerik(g.alt), pos, blok: false });
    });
  });
  return satirlar;
}

/** Paragraftaki her satırın ilk karakterinin belge konumu, sırayla. Metin düğümlerinde satırın değiştiği
 * karakter ikiye bölerek aranır (bir düğümde k satır için ~k·log n ölçüm); satır içi atomlar (sekme, resim, alan)
 * kendi kutularıyla. Sayfa boşluğu (widget) atlanır. */
function satirBaslari(view: EditorView, p: HTMLElement): number[] {
  const birimler: { ust: number; alt: number; pos: number }[] = [];
  const aralik = document.createRange();
  const kutu = (t: Text, i: number): DOMRect | null => {
    aralik.setStart(t, i);
    aralik.setEnd(t, i + 1);
    const r = aralik.getClientRects();
    return r.length ? r[r.length - 1] : null;
  };
  const yuru = document.createTreeWalker(p, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT, {
    acceptNode: (n) =>
      n instanceof HTMLElement && n.classList.contains("sayfa-boslugu") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  let n: Node | null;
  while ((n = yuru.nextNode())) {
    if (n instanceof HTMLElement) {
      if (n.classList.contains("sekme") || n.classList.contains("alan") || n.tagName === "IMG") {
        const r = n.getBoundingClientRect();
        try {
          birimler.push({ ust: r.top, alt: r.bottom, pos: view.posAtDOM(n, 0) - 1 });
        } catch {
          /* düğüm editörün dışında */
        }
      }
      continue;
    }
    const t = n as Text;
    const uz = t.data.length;
    if (!uz || (t.parentElement && t.parentElement.closest(".alan, .sekme"))) continue;
    const ilk = kutu(t, 0);
    if (!ilk) continue;
    const konum = (i: number) => view.posAtDOM(t, i);
    birimler.push({ ust: ilk.top, alt: ilk.bottom, pos: konum(0) });
    const bol = (i: number, j: number, ri: DOMRect, rj: DOMRect) => {
      if (rj.top < ri.bottom - 2) return; // aynı satır (satırlar yukarıdan aşağı; geri dönüş yok)
      if (j - i === 1) {
        birimler.push({ ust: rj.top, alt: rj.bottom, pos: konum(j) });
        return;
      }
      const m = (i + j) >> 1;
      const rm = kutu(t, m);
      if (!rm) return;
      bol(i, m, ri, rm);
      bol(m, j, rm, rj);
    };
    const son = kutu(t, uz - 1);
    if (son && uz > 1) bol(0, uz - 1, ilk, son);
  }
  // Birimlerden satır başları: üstü bir öncekinin altına ulaşan birim yeni satırdır
  const baslar: number[] = [];
  let alt = -Infinity;
  for (const b of birimler) {
    if (b.ust >= alt - 2) {
      baslar.push(b.pos);
      alt = b.alt;
    } else alt = Math.max(alt, b.alt);
  }
  return baslar;
}

export function kirilmalariHesapla(view: EditorView, o: SayfaOlcusu): { kirilmalar: Kirilma[]; sayfaSayisi: number } {
  const yaziH = o.yaziAlti - o.yaziUstu;
  const adim = o.yukseklik + ARALIK;
  const satirlar = satirlariOlc(view, o.olcek);
  const kirilmalar: Kirilma[] = [];
  let kayma = 0;
  let k = 0;
  for (const s of satirlar) {
    const ust = s.ust + kayma, alt = s.alt + kayma;
    if (alt > k * adim + yaziH + 0.5 && ust > k * adim + 0.5) {
      const h = (k + 1) * adim - ust;
      if (!kirilmalar.some((x) => x.pos === s.pos)) {
        kirilmalar.push({ pos: s.pos, h, blok: s.blok });
        kayma += h;
        k++;
      }
    }
  }
  return { kirilmalar, sayfaSayisi: k + 1 };
}

function ayni(a: Kirilma[], b: Kirilma[]): boolean {
  return a.length === b.length && a.every((x, i) => x.pos === b[i].pos && Math.abs(x.h - b[i].h) < 1 && x.blok === b[i].blok);
}

/** Sayfalama eklentisi. `olcu()` o anki sayfa ölçülerini, `cizildi(n)` sayfa sayısı değişince katmanı çizer. */
export function sayfaEklentisi(olcu: () => SayfaOlcusu, cizildi: (sayfaSayisi: number) => void): Plugin<Durum> {
  return new Plugin<Durum>({
    key: sayfaAnahtari,
    state: {
      init: () => ({ kirilmalar: [], dekor: DecorationSet.empty }),
      apply(tr, d, _eski, yeni) {
        const m = tr.getMeta(sayfaAnahtari) as Kirilma[] | undefined;
        if (m) return { kirilmalar: m, dekor: dekorlar(yeni, m) };
        // Yakınlaştırma / pencere boyutu değişti: yeniden ölç (durum nesnesi değişince görünüm planlar)
        if (tr.getMeta(YENIDEN_OLC)) return { ...d };
        if (!tr.docChanged) return d;
        const kirilmalar = d.kirilmalar.map((x) => ({ ...x, pos: tr.mapping.map(x.pos, -1) }));
        return { kirilmalar, dekor: d.dekor.map(tr.mapping, tr.doc) };
      },
    },
    props: {
      decorations: (state) => sayfaAnahtari.getState(state)?.dekor,
    },
    view(view) {
      let bekleyen = 0;
      let sonSayi = -1;
      // Sigorta: belge değişmeden üst üste çok ölçüm değişikliği (piksel yuvarlaması salınımı) olursa dur
      let tur = 0;
      let sonDoc = view.state.doc;
      let sonOlcek = 0;
      const calis = () => {
        bekleyen = 0;
        const o = olcu();
        sekmeleriYerlestir(view.dom as HTMLElement, o.olcek);
        const { kirilmalar, sayfaSayisi } = kirilmalariHesapla(view, o);
        const eski = sayfaAnahtari.getState(view.state)!.kirilmalar;
        if (view.state.doc !== sonDoc || o.olcek !== sonOlcek) {
          sonDoc = view.state.doc;
          sonOlcek = o.olcek;
          tur = 0;
        }
        if (!ayni(eski, kirilmalar) && tur++ < 4) {
          view.dispatch(view.state.tr.setMeta(sayfaAnahtari, kirilmalar).setMeta("addToHistory", false));
        }
        if (sayfaSayisi !== sonSayi) {
          sonSayi = sayfaSayisi;
          cizildi(sayfaSayisi);
        }
      };
      const planla = () => {
        if (!bekleyen) bekleyen = requestAnimationFrame(calis);
      };
      planla();
      // Yazı tipleri yüklenince ölçüler değişir
      document.fonts?.ready.then(planla);
      return {
        update: (_v, onceki) => {
          if (onceki.doc !== view.state.doc || sayfaAnahtari.getState(onceki) !== sayfaAnahtari.getState(view.state)) planla();
        },
        destroy: () => bekleyen && cancelAnimationFrame(bekleyen),
      };
    },
  });
}
