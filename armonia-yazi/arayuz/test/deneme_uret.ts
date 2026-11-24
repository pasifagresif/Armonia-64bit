// UYAP editöründe denenecek düzenlenmiş kopya: 3. uzun paragrafın ilk 5 karakteri kalın, ortasından bölünüp
// "[Armonia Yazı deneme]" eklenir ve ortalanır. Belge METNİ yazdırılmaz. Claude, 2026-11-23.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { EditorState, TextSelection } from "prosemirror-state";
import { splitBlock, toggleMark } from "prosemirror-commands";
import { belgeDugumu, belgeyeCevir } from "../src/donustur";
import { sema } from "../src/sema";
import type { Acilis } from "../src/turler";
import { initSync, udf_ac, udf_kaydet } from "../src/wasm/belge_wasm.js";

initSync({ module: readFileSync(fileURLToPath(new URL("../src/wasm/belge_wasm_bg.wasm", import.meta.url))) });
const a = udf_ac(new Uint8Array(readFileSync(process.argv[2]))) as Acilis;
let st = EditorState.create({ doc: belgeDugumu(a.belge, a.gorunum) });
const ps: number[] = [];
st.doc.forEach((n, pos) => { if (n.type === sema.nodes.paragraph && n.textContent.length > 20) ps.push(pos); });
const p = ps[2];
st = st.apply(st.tr.setSelection(TextSelection.create(st.doc, p + 1, p + 6)));
toggleMark(sema.marks.kalin)(st, (tr) => (st = st.apply(tr)));
const orta = p + 1 + Math.floor(st.doc.nodeAt(p)!.content.size / 2);
st = st.apply(st.tr.setSelection(TextSelection.create(st.doc, orta)));
splitBlock(st, (tr) => (st = st.apply(tr)));
st = st.apply(st.tr.insertText("[Armonia Yazı deneme] "));
const yeni = st.selection.$from.before();
st = st.apply(st.tr.setNodeMarkup(yeni, undefined, { ...st.doc.nodeAt(yeni)!.attrs, hiza: "Orta" }));
writeFileSync(process.argv[3], udf_kaydet(belgeyeCevir(st.doc, a.belge)) as Uint8Array);
console.log("yazıldı:", process.argv[3]);
