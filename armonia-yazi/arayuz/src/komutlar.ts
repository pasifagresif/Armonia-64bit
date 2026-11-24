// Biçim komutları: yazı tipi / boyut / renk, girinti, satır aralığı, madde ve numaralı liste.
// Liste önekleri (1., 2., •) belge çekirdeğinin kurallarıyla (gorunum_hesapla) yeniden hesaplanır.
// Claude, 2026-11-23.

import type { Mark, Node as PMNode } from "prosemirror-model";
import type { Command, EditorState } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";

import { belgeyeCevir } from "./donustur";
import { sema } from "./sema";
import type { Belge, Gorunum, Ozellikler, Renk } from "./turler";
import { ozAl, ozAyarla } from "./turler";
import { gorunum_hesapla } from "./wasm/belge_wasm.js";

export const YAZI_TIPLERI = ["Times New Roman", "Arial", "Courier New", "Tahoma", "Verdana", "Calibri"];
export const BOYUTLAR = [8, 9, 10, 11, 12, 13, 14, 16, 18, 20, 24, 28, 36];
/** UDF `LineSpacing`: satır yüksekliğine eklenen oran (0 = tek). */
export const SATIR_ARALIKLARI: [string, number][] = [["Tek", 0], ["1,15", 0.15], ["1,5", 0.5], ["Çift", 1]];
/** Girinti adımı (pt): 1,25 cm ≈ 35,4 pt (UYAP ve Word varsayılanı) */
export const GIRINTI_ADIMI = 35.4;

type YaziAlani = "aile" | "boyut" | "renk";

/** Seçimdeki metnin yazı tipi / boyut / rengini değiştirir; öteki iki alan her parçada korunur. */
export function yaziDegistir(alan: YaziAlani, deger: string | number | Renk): Command {
  return (state, dispatch) => {
    const tip = sema.marks.yazi;
    const { from, to, empty, $from } = state.selection;
    if (!dispatch) return true;
    const tr = state.tr;
    if (empty) {
      const eski = tip.isInSet(state.storedMarks || $from.marks());
      const attrs = { ...(eski?.attrs ?? tip.create().attrs), [alan]: deger };
      tr.addStoredMark(tip.create(attrs));
    } else {
      state.doc.nodesBetween(from, to, (n, pos) => {
        if (!n.isInline) return;
        const bas = Math.max(from, pos), son = Math.min(to, pos + n.nodeSize);
        const eski = tip.isInSet(n.marks);
        tr.addMark(bas, son, tip.create({ ...(eski?.attrs ?? tip.create().attrs), [alan]: deger }));
      });
    }
    dispatch(tr.scrollIntoView());
    return true;
  };
}

/** Seçimdeki ilk metnin yazı biçimi (araç çubuğundaki seçicileri göstermek için). */
export function etkinYazi(state: EditorState): { aile: string; boyut: number; renk: Renk } | null {
  const { empty, $from, from, to } = state.selection;
  let m: Mark | null | undefined = null;
  if (empty) m = sema.marks.yazi.isInSet(state.storedMarks || $from.marks());
  else state.doc.nodesBetween(from, to, (n) => { if (!m && n.isText) m = sema.marks.yazi.isInSet(n.marks); });
  return m ? (m.attrs as { aile: string; boyut: number; renk: Renk }) : null;
}

function paragraflaraUygula(state: EditorState, f: (n: PMNode) => Record<string, unknown> | null): ReturnType<EditorState["tr"]["setNodeMarkup"]> | null {
  const { from, to } = state.selection;
  const tr = state.tr;
  let degisti = false;
  state.doc.nodesBetween(from, to, (n, pos) => {
    if (n.type !== sema.nodes.paragraph) return;
    const yeni = f(n);
    if (yeni) {
      tr.setNodeMarkup(pos, undefined, { ...n.attrs, ...yeni });
      degisti = true;
    }
    return false;
  });
  return degisti ? tr : null;
}

export function girinti(yon: 1 | -1): Command {
  return (state, dispatch) => {
    const tr = paragraflaraUygula(state, (n) => ({ sol: Math.max(0, Math.round(((n.attrs.sol as number) + yon * GIRINTI_ADIMI) * 100) / 100) }));
    if (tr && dispatch) dispatch(tr);
    return !!tr;
  };
}

export function satirAraligi(deger: number): Command {
  return (state, dispatch) => {
    const tr = paragraflaraUygula(state, () => ({ satirAraligi: deger }));
    if (tr && dispatch) dispatch(tr);
    return !!tr;
  };
}

/** Madde / numaralı liste aç-kapa: UDF paragraf özellikleri (Bulleted / Numbered, ListId, NumberType). */
export function liste(tur: "madde" | "numara"): Command {
  return (state, dispatch) => {
    const ilk = state.selection.$from.parent;
    const acik = ozAl(ilk.attrs.oz as Ozellikler, tur === "madde" ? "Bulleted" : "Numbered") === "true";
    const tr = paragraflaraUygula(state, (n) => {
      let oz = n.attrs.oz as Ozellikler;
      if (acik) {
        oz = oz.filter(([a]) => !["Bulleted", "BulletType", "Numbered", "NumberType", "ListId", "ListLevel"].includes(a));
        oz = ozAyarla(oz, tur === "madde" ? "Bulleted" : "Numbered", "false");
      } else if (tur === "madde") {
        oz = oz.filter(([a]) => !["Numbered", "NumberType"].includes(a));
        oz = ozAyarla(ozAyarla(oz, "Bulleted", "true"), "BulletType", "BULLET_TYPE_ELLIPSE");
      } else {
        oz = oz.filter(([a]) => !["Bulleted", "BulletType"].includes(a));
        oz = ozAyarla(ozAyarla(ozAyarla(ozAyarla(oz, "Numbered", "true"), "NumberType", "NUMBER_TYPE_NUMBER_DOT"), "ListId", "1"), "ListLevel", "1");
      }
      return { oz };
    });
    if (tr && dispatch) dispatch(tr);
    return !!tr;
  };
}

/** Liste öneklerini çekirdekle yeniden hesaplar; değişen paragrafları günceller (geri alma geçmişine girmez). */
export function onekleriYenile(view: EditorView, asil: Belge) {
  const belge = belgeyeCevir(view.state.doc, asil);
  const g = gorunum_hesapla(belge) as Gorunum;
  const tr = view.state.tr;
  let i = 0;
  view.state.doc.forEach((n, pos) => {
    const gb = g.govde[i++];
    if (n.type !== sema.nodes.paragraph || !gb || !("Paragraf" in gb)) return;
    const onek = gb.Paragraf.onek ?? null;
    if (n.attrs.onek !== onek) tr.setNodeMarkup(pos, undefined, { ...n.attrs, onek });
  });
  if (tr.docChanged) view.dispatch(tr.setMeta("addToHistory", false));
}
