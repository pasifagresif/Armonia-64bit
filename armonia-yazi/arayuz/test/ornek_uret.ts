// Deneme belgesi üretir (sekme / tek pencere denemeleri için): boş belgeye verilen metni yazar, UDF kaydeder.
//   node ornek_uret.js <çıktı.udf> <metin>
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { EditorState } from "prosemirror-state";
import { bosBelgeOlustur } from "../src/bos_belge";
import { belgeDugumu, belgeyeCevir } from "../src/donustur";
import type { Acilis } from "../src/turler";
import { initSync, gorunum_hesapla, udf_kaydet } from "../src/wasm/belge_wasm.js";

initSync({ module: readFileSync(fileURLToPath(new URL("../src/wasm/belge_wasm_bg.wasm", import.meta.url))) });
const belge = bosBelgeOlustur();
const g = gorunum_hesapla(belge) as Acilis["gorunum"];
let st = EditorState.create({ doc: belgeDugumu(belge, g) });
st = st.apply(st.tr.insertText(process.argv[3] ?? "Deneme"));
writeFileSync(process.argv[2], udf_kaydet(belgeyeCevir(st.doc, belge)) as Uint8Array);
console.log("yazıldı:", process.argv[2]);
