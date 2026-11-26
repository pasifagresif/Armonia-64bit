// Editör şeması (ProseMirror). Belge modelinin (UDF) karşılığı:
//  - paragraph: UDF paragrafı. `oz` ham özellikler (kayıpsızlık), `ozgun` açılıştaki çözülmüş stil (kaydederken
//    yalnız DEĞİŞEN biçim açık özellik olarak yazılır), görünen biçim alanları (hiza, girintiler...).
//  - udf işareti: metnin geldiği UDF parçası (etiket + ham özellikler + açılıştaki çözülmüş stil).
//  - kalin / egik / alti / yazi işaretleri: görünen biçim; araç çubuğu bunları değiştirir.
//  - sekme, alan, resim: satır içi atom düğümler; tablo: şimdilik salt okunur atom (ham tablo korunur).
// Claude, 2026-11-23.

import { Schema, type DOMOutputSpec, type Mark, type Node as PMNode } from "prosemirror-model";
import type { Hiza, ParagrafStili, ParcaStili, Renk, TabloStili } from "./turler";
import { renkCss } from "./turler";

/** Java mantıksal yazı tiplerinin gerçek karşılıkları (belge_cekirdek stil.rs `cizim_ailesi` ile aynı) ve
 * Linux'ta ölçüleri aynı açık yazı tipleri. */
export function cssAilesi(aile: string): string {
  const k = aile.trim().toLowerCase();
  const gercek =
    ["dialog", "sansserif", ".applesystemuifont"].includes(k) ? "Arial"
    : ["dialoginput", "monospaced"].includes(k) ? "Courier New"
    : ["serif", "editor", "source serif pro", "albertus medium"].includes(k) ? "Times New Roman"
    : k === "coronetps" ? "Monotype Corsiva"
    : aile.trim();
  const yedek: Record<string, string> = {
    "Arial": '"Liberation Sans", Arimo, Helvetica, sans-serif',
    "Times New Roman": '"Liberation Serif", Tinos, Times, serif',
    "Courier New": '"Liberation Mono", Cousine, monospace',
  };
  return `"${gercek.replace(/"/g, "")}", ${yedek[gercek] ?? "sans-serif"}`;
}

const HIZA_CSS: Record<Hiza, string> = { Sol: "left", Orta: "center", Sag: "right", IkiYana: "justify" };

/** Satır yüksekliği: udf_pdf.rs `(üst + alt + 0.04·boyut)·(1 + LineSpacing)`; yaygın yazı tiplerinde
 * üst+alt ≈ 1.12·boyut. */
export function satirYuksekligi(satirAraligi: number): string {
  return ((1.12 + 0.04) * (1 + satirAraligi)).toFixed(3);
}

export const sema = new Schema({
  nodes: {
    doc: { content: "(paragraph | tablo)+" },
    paragraph: {
      content: "inline*",
      attrs: {
        oz: { default: [] },
        ozgun: { default: null },
        satirSonuOz: { default: null },
        hiza: { default: "Sol" },
        sol: { default: 0 },
        sag: { default: 0 },
        ilkSatir: { default: 0 },
        satirAraligi: { default: 0 },
        ustBosluk: { default: 0 },
        altBosluk: { default: 0 },
        onek: { default: null },
        // Sekme durakları (pt, sol girintiye göre): [{konum, hiza}]; sekme genişlikleri sayfa.ts hesaplar
        sekmeler: { default: [] },
      },
      toDOM(n: PMNode): DOMOutputSpec {
        const a = n.attrs;
        const stil = [
          `text-align:${HIZA_CSS[a.hiza as Hiza] ?? "left"}`,
          `padding-left:${a.sol}pt`,
          `padding-right:${a.sag}pt`,
          `text-indent:${a.ilkSatir}pt`,
          `line-height:${satirYuksekligi(a.satirAraligi)}`,
          `margin:${a.ustBosluk}pt 0 ${a.altBosluk}pt 0`,
        ].join(";");
        // Paragrafın kendi yazısı ilk metnin biçimiyle: liste öneki (::before) ve boş paragrafın yüksekliği buna göre
        // (udf_pdf.rs önek ilk sözcüğün stiliyle çizilir)
        let ilkYazi = "";
        n.forEach((c) => {
          const y = !ilkYazi && c.isText ? c.marks.find((m) => m.type.name === "yazi") : null;
          if (y) ilkYazi = `;font-family:${cssAilesi(y.attrs.aile)};font-size:${y.attrs.boyut}pt`;
        });
        const ozellik: Record<string, string> = { style: stil + ilkYazi, class: "p" };
        if (a.onek) ozellik["data-onek"] = a.onek;
        ozellik["data-sekmeler"] = JSON.stringify(a.sekmeler ?? []);
        return ["p", ozellik, 0];
      },
    },
    // Tablo (UDF table/row/cell). Kutularla (div) çizilir, HTML tablosuyla değil: sayfa boşluğu (widget) satırlar
    // arasına konabilsin. Hücre genişlikleri satırın oranlarından (CSS grid).
    tablo: {
      content: "satir+",
      isolating: true,
      attrs: { oz: { default: [] }, stil: { default: { sol: 0, sag: 0, ust_bosluk: 0, alt_bosluk: 0, kenarlik: "Hucre" } } },
      toDOM(n: PMNode): DOMOutputSpec {
        const s = n.attrs.stil as TabloStili;
        return ["div", { class: "tablo", "data-kenarlik": s.kenarlik,
          style: `margin:${s.ust_bosluk}pt ${s.sag}pt ${s.alt_bosluk}pt ${s.sol}pt` }, 0];
      },
    },
    satir: {
      content: "hucre+",
      attrs: { oz: { default: [] }, oranlar: { default: [] }, minYukseklik: { default: 0 } },
      toDOM(n: PMNode): DOMOutputSpec {
        const o = (n.attrs.oranlar as number[]).length ? (n.attrs.oranlar as number[]) : Array(n.childCount).fill(1 / n.childCount);
        return ["div", { class: "satir",
          style: `grid-template-columns:${o.map((x) => `${Math.max(x, 0.001)}fr`).join(" ")};min-height:${n.attrs.minYukseklik}pt` }, 0];
      },
    },
    hucre: {
      content: "(paragraph | tablo)+",
      isolating: true,
      attrs: { oz: { default: [] } },
      toDOM(): DOMOutputSpec {
        return ["div", { class: "hucre" }, 0];
      },
    },
    text: { group: "inline" },
    sekme: {
      group: "inline",
      inline: true,
      atom: true,
      // gomulu: UYAP Tab'ı çoğu belgede <tab> öğesi değil, bir <content> parçasının metninde ham "\t" olarak
      // yazar. Açılışta o "\t" sekmeye çevrilir; `gomulu` o parçanın udf işaretinin nitelikleridir (etiket, oz,
      // ozgun) ve kaydederken sekme yine aynı parçanın metnine "\t" olarak birleştirilir (donustur.ts).
      attrs: { oz: { default: [] }, metin: { default: "\t" }, stil: { default: null }, gomulu: { default: null } },
      toDOM(): DOMOutputSpec {
        // Genisligi sayfa.ts sekmeleriYerlestir duraklara gore verir
        return ["span", { class: "sekme", contenteditable: "false" }];
      },
    },
    alan: {
      group: "inline",
      inline: true,
      atom: true,
      attrs: { oz: { default: [] }, metin: { default: "" }, stil: { default: null } },
      toDOM(n: PMNode): DOMOutputSpec {
        return ["span", { class: "alan", contenteditable: "false", title: "Alan" }, n.attrs.metin.replace(/\n/g, "")];
      },
    },
    resim: {
      group: "inline",
      inline: true,
      atom: true,
      attrs: { oz: { default: [] }, metin: { default: "" }, stil: { default: null } },
      toDOM(n: PMNode): DOMOutputSpec {
        const oz = n.attrs.oz as [string, string][];
        const al = (k: string) => oz.find(([a]) => a === k)?.[1];
        const veri = (al("imageData") ?? "").replace(/[^A-Za-z0-9+/=]/g, "");
        const g = parseFloat(al("width") ?? "0");
        const y = parseFloat(al("height") ?? "0");
        const stil = (g > 0 ? `width:${g}pt;` : "") + (y > 0 ? `height:${y}pt;` : "");
        return ["img", { src: `data:image/png;base64,${veri}`, style: stil, class: "resim", alt: "" }];
      },
    },
  },
  marks: {
    udf: {
      attrs: { etiket: { default: "content" }, oz: { default: [] }, ozgun: { default: null } },
      // Ham parça bilgisi görünmez; aynı parçadan gelen metin bitişik kalır
      toDOM(): DOMOutputSpec {
        return ["span", 0];
      },
    },
    kalin: { toDOM: (): DOMOutputSpec => ["strong", 0] },
    egik: { toDOM: (): DOMOutputSpec => ["em", 0] },
    alti: { toDOM: (): DOMOutputSpec => ["u", 0] },
    yazi: {
      attrs: { aile: { default: "Times New Roman" }, boyut: { default: 12 }, renk: { default: { r: 0, g: 0, b: 0 } } },
      toDOM(m: Mark): DOMOutputSpec {
        const r = m.attrs.renk as Renk;
        return ["span", { style: `font-family:${cssAilesi(m.attrs.aile)};font-size:${m.attrs.boyut}pt;color:${renkCss(r)}` }, 0];
      },
    },
  },
});

/** Paragraf düğümünün görünen biçim alanları, çözülmüş stilden. */
export function paragrafAlanlari(s: ParagrafStili) {
  return {
    hiza: s.hiza,
    sol: s.sol,
    sag: s.sag,
    ilkSatir: s.ilk_satir,
    satirAraligi: s.satir_araligi,
    ustBosluk: s.ust_bosluk,
    altBosluk: s.alt_bosluk,
    sekmeler: s.sekmeler,
  };
}

/** Parçanın çözülmüş stilinden görünen işaretler. */
export function bicimIsaretleri(s: ParcaStili): Mark[] {
  const m: Mark[] = [sema.marks.yazi.create({ aile: s.aile, boyut: s.boyut, renk: s.renk })];
  if (s.kalin) m.push(sema.marks.kalin.create());
  if (s.egik) m.push(sema.marks.egik.create());
  if (s.alti_cizili) m.push(sema.marks.alti.create());
  return m;
}
