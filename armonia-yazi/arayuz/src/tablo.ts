// Tablo komutları: ekle / sil, satır ekle-sil, sütun ekle-sil, hücreler arası gezinme (Tab).
// UDF tablosunda sütun genişlikleri `columnSpans` oranlarıdır (tabloda ve isteğe bağlı satırda). Sütun eklenip
// silinince oranlar ve bu özellikler birlikte güncellenir; satırın kendi oranları (birleşik hücreli satırlar)
// hücre sayısı aynı olan satırlarda değişir. Claude, 2026-11-23.

import type { Node as PMNode, ResolvedPos } from "prosemirror-model";
import { TextSelection, type Command, type EditorState, type Transaction } from "prosemirror-state";

import { sema } from "./sema";
import type { Ozellikler } from "./turler";
import { ozAl, ozAyarla } from "./turler";

interface Konum {
  tablo: PMNode;
  tabloPos: number;
  satir: PMNode;
  satirPos: number;
  satirNo: number;
  hucre: PMNode;
  hucrePos: number;
  hucreNo: number;
}

/** İmlecin içinde bulunduğu en içteki tablo hücresi. */
export function tabloKonumu($p: ResolvedPos): Konum | null {
  for (let d = $p.depth; d > 0; d--) {
    if ($p.node(d).type === sema.nodes.hucre && d >= 2) {
      return {
        tablo: $p.node(d - 2), tabloPos: $p.before(d - 2),
        satir: $p.node(d - 1), satirPos: $p.before(d - 1), satirNo: $p.index(d - 2),
        hucre: $p.node(d), hucrePos: $p.before(d), hucreNo: $p.index(d - 1),
      };
    }
  }
  return null;
}

export const tablodaMi = (state: EditorState) => !!tabloKonumu(state.selection.$from);

function bosHucre(oz: Ozellikler = []): PMNode {
  return sema.nodes.hucre.create({ oz }, sema.nodes.paragraph.create());
}

/** Oran dizisini UDF metnine çevirir; ölçek eski toplama göre (tam sayılar korunur). */
function oranMetni(oranlar: number[], eskiToplam: number): string {
  return oranlar.map((x) => {
    const v = x * eskiToplam;
    return Number.isInteger(v) ? String(v) : String(Math.round(v * 100) / 100);
  }).join(",");
}

function oranlariCoz(m: string | undefined): number[] {
  if (!m) return [];
  const l = m.split(",").map((x) => x.trim()).filter(Boolean).map(Number);
  return l.length && l.every((x) => Number.isFinite(x)) && l.reduce((a, b) => a + b, 0) > 0 ? l : [];
}

function normalize(o: number[]): number[] {
  const t = o.reduce((a, b) => a + b, 0) || 1;
  return o.map((x) => x / t);
}

/** n x m tablo: satır başına m hücre, eşit genişlik, hücre kenarlıklı (UYAP varsayılanı). */
export function tabloEkle(satirSayisi = 2, sutunSayisi = 2): Command {
  return (state, dispatch) => {
    if (tablodaMi(state)) return false;
    if (dispatch) {
      const oranlar = Array(sutunSayisi).fill(1 / sutunSayisi);
      const satirlar = Array.from({ length: satirSayisi }, () =>
        sema.nodes.satir.create({ oz: [], oranlar }, Array.from({ length: sutunSayisi }, () => bosHucre())));
      const tablo = sema.nodes.tablo.create({
        oz: [["columnSpans", Array(sutunSayisi).fill(1).join(",")], ["border", "borderCell"]],
        stil: { sol: 0, sag: 0, ust_bosluk: 0, alt_bosluk: 0, kenarlik: "Hucre" },
      }, satirlar);
      // Bulunulan paragraftan sonra; ardından yazmaya devam edilebilsin diye boş paragraf
      const $f = state.selection.$from;
      const sonra = $f.after($f.depth);
      const tr = state.tr.insert(sonra, [tablo, sema.nodes.paragraph.create()]);
      tr.setSelection(TextSelection.near(tr.doc.resolve(sonra + 3)));
      dispatch(tr.scrollIntoView());
    }
    return true;
  };
}

export const tabloSil: Command = (state, dispatch) => {
  const k = tabloKonumu(state.selection.$from);
  if (!k) return false;
  if (dispatch) {
    const tr = state.tr.delete(k.tabloPos, k.tabloPos + k.tablo.nodeSize);
    if (tr.doc.childCount === 0) tr.insert(0, sema.nodes.paragraph.create());
    tr.setSelection(TextSelection.near(tr.doc.resolve(Math.min(k.tabloPos, tr.doc.content.size))));
    dispatch(tr.scrollIntoView());
  }
  return true;
};

export function satirEkle(yer: "ust" | "alt"): Command {
  return (state, dispatch) => {
    const k = tabloKonumu(state.selection.$from);
    if (!k) return false;
    if (dispatch) {
      const hucreler: PMNode[] = [];
      k.satir.forEach((h) => hucreler.push(bosHucre(h.attrs.oz)));
      const yeni = sema.nodes.satir.create({ ...k.satir.attrs, oz: (k.satir.attrs.oz as Ozellikler).filter(([a]) => a !== "height") }, hucreler);
      const pos = yer === "ust" ? k.satirPos : k.satirPos + k.satir.nodeSize;
      const tr = state.tr.insert(pos, yeni);
      tr.setSelection(TextSelection.near(tr.doc.resolve(pos + 3)));
      dispatch(tr.scrollIntoView());
    }
    return true;
  };
}

export const satirSil: Command = (state, dispatch) => {
  const k = tabloKonumu(state.selection.$from);
  if (!k) return false;
  if (k.tablo.childCount === 1) return tabloSil(state, dispatch);
  if (dispatch) {
    const tr = state.tr.delete(k.satirPos, k.satirPos + k.satir.nodeSize);
    tr.setSelection(TextSelection.near(tr.doc.resolve(Math.min(k.satirPos + 3, tr.doc.content.size))));
    dispatch(tr.scrollIntoView());
  }
  return true;
};

/** Tablonun sütunlarını değiştirir: `degistir(oranlar, hucreler)` hücre sayısı imleçteki satırla aynı olan her
 * satıra uygulanır. Tablo ve satırların `columnSpans` özellikleri yeni oranlarla yazılır. */
function sutunlariDegistir(state: EditorState, k: Konum,
  degistir: (oranlar: number[], hucreler: PMNode[]) => { oranlar: number[]; hucreler: PMNode[] }): Transaction | null {
  const n = k.satir.childCount;
  const yeniSatirlar: PMNode[] = [];
  let yeniTabloOran: number[] | null = null;
  k.tablo.forEach((r) => {
    if (r.childCount !== n) {
      yeniSatirlar.push(r);
      return;
    }
    const hucreler: PMNode[] = [];
    r.forEach((h) => hucreler.push(h));
    const oranlar = (r.attrs.oranlar as number[]).length === n ? (r.attrs.oranlar as number[]) : Array(n).fill(1 / n);
    const y = degistir(oranlar.slice(), hucreler);
    const no = normalize(y.oranlar);
    yeniTabloOran ??= no;
    let oz = r.attrs.oz as Ozellikler;
    const eski = oranlariCoz(ozAl(oz, "columnSpans"));
    if (eski.length) oz = ozAyarla(oz, "columnSpans", oranMetni(no, eski.reduce((a, b) => a + b, 0)));
    yeniSatirlar.push(sema.nodes.satir.create({ ...r.attrs, oz, oranlar: no }, y.hucreler));
  });
  if (!yeniTabloOran) return null;
  let toz = k.tablo.attrs.oz as Ozellikler;
  const eskiT = oranlariCoz(ozAl(toz, "columnSpans"));
  if (eskiT.length === n) toz = ozAyarla(toz, "columnSpans", oranMetni(yeniTabloOran, eskiT.reduce((a, b) => a + b, 0)));
  const yeniTablo = sema.nodes.tablo.create({ ...k.tablo.attrs, oz: toz }, yeniSatirlar);
  return state.tr.replaceWith(k.tabloPos, k.tabloPos + k.tablo.nodeSize, yeniTablo);
}

export function sutunEkle(yer: "sol" | "sag"): Command {
  return (state, dispatch) => {
    const k = tabloKonumu(state.selection.$from);
    if (!k) return false;
    if (dispatch) {
      const i = k.hucreNo;
      const tr = sutunlariDegistir(state, k, (oranlar, hucreler) => {
        // Yeni sütun, bulunulan sütunun genişliğini onunla paylaşır
        const yarim = oranlar[i] / 2;
        oranlar[i] = yarim;
        const j = yer === "sol" ? i : i + 1;
        oranlar.splice(j, 0, yarim);
        hucreler.splice(j, 0, bosHucre(hucreler[i].attrs.oz));
        return { oranlar, hucreler };
      });
      if (tr) {
        const $y = tr.doc.resolve(Math.min(state.selection.from, tr.doc.content.size));
        tr.setSelection(TextSelection.near($y));
        dispatch(tr.scrollIntoView());
      }
    }
    return true;
  };
}

export const sutunSil: Command = (state, dispatch) => {
  const k = tabloKonumu(state.selection.$from);
  if (!k) return false;
  if (k.satir.childCount === 1) return tabloSil(state, dispatch);
  if (dispatch) {
    const i = k.hucreNo;
    const tr = sutunlariDegistir(state, k, (oranlar, hucreler) => {
      // Silinen sütunun genişliği komşusuna geçer
      const komsu = i > 0 ? i - 1 : 1;
      oranlar[komsu] += oranlar[i];
      oranlar.splice(i, 1);
      hucreler.splice(i, 1);
      return { oranlar, hucreler };
    });
    if (tr) {
      tr.setSelection(TextSelection.near(tr.doc.resolve(Math.min(k.tabloPos + 3, tr.doc.content.size))));
      dispatch(tr.scrollIntoView());
    }
  }
  return true;
};

/** Tab / Shift+Tab: sonraki / önceki hücre. Son hücrede Tab yeni satır açar (Word ve UYAP gibi). */
export function hucreGec(yon: 1 | -1): Command {
  return (state, dispatch) => {
    const k = tabloKonumu(state.selection.$from);
    if (!k) return false;
    const hucreler: number[] = [];
    k.tablo.forEach((r, rOfs) => r.forEach((_h, hOfs) => hucreler.push(k.tabloPos + 1 + rOfs + 1 + hOfs)));
    const i = hucreler.indexOf(k.hucrePos);
    const j = i + yon;
    if (j < 0) return true;
    if (j >= hucreler.length) return satirEkle("alt")(state, dispatch);
    if (dispatch) {
      const tr = state.tr;
      tr.setSelection(TextSelection.near(tr.doc.resolve(hucreler[j] + 1)));
      dispatch(tr.scrollIntoView());
    }
    return true;
  };
}
