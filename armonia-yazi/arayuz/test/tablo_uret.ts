// UYAP'ta ve editörde denenecek tablolu kopya: 2. uzun paragraftan sonra 3×3 tablo (başlık satırı kalın).
// Belge METNİ yazdırılmaz. Claude, 2026-11-23.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { EditorState, TextSelection, type Command } from "prosemirror-state";
import { toggleMark } from "prosemirror-commands";
import { belgeDugumu, belgeyeCevir } from "../src/donustur";
import { sema } from "../src/sema";
import { hucreGec, tabloEkle } from "../src/tablo";
import type { Acilis } from "../src/turler";
import { initSync, udf_ac, udf_kaydet } from "../src/wasm/belge_wasm.js";

initSync({ module: readFileSync(fileURLToPath(new URL("../src/wasm/belge_wasm_bg.wasm", import.meta.url))) });
const a = udf_ac(new Uint8Array(readFileSync(process.argv[2]))) as Acilis;
let s = EditorState.create({ doc: belgeDugumu(a.belge, a.gorunum) });
const uygula = (k: Command) => k(s, (tr) => (s = s.apply(tr)));
const ps: number[] = [];
s.doc.forEach((n, pos) => { if (n.type === sema.nodes.paragraph && n.textContent.length > 20) ps.push(pos); });
s = s.apply(s.tr.setSelection(TextSelection.create(s.doc, ps[1] + 2)));
uygula(tabloEkle(3, 3));
const hucreler = ["Tarih", "Tutar", "Açıklama", "12.06.2024", "1.319.000,00 TL", "Borç tahsilatı", "24.05.2024", "170.268,00 TL", "Teminat"];
hucreler.forEach((m, i) => {
  if (i) uygula(hucreGec(1));
  const bas = s.selection.from;
  s = s.apply(s.tr.insertText(m));
  if (i < 3) {
    s = s.apply(s.tr.setSelection(TextSelection.create(s.doc, bas, bas + m.length)));
    uygula(toggleMark(sema.marks.kalin));
    s = s.apply(s.tr.setSelection(TextSelection.create(s.doc, bas + m.length)));
  }
});
writeFileSync(process.argv[3], udf_kaydet(belgeyeCevir(s.doc, a.belge)) as Uint8Array);
console.log("yazıldı");
