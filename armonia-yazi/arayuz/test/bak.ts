import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { belgeDugumu } from "../src/donustur";
import { sema } from "../src/sema";
import type { Acilis } from "../src/turler";
import { initSync, udf_ac } from "../src/wasm/belge_wasm.js";
initSync({ module: readFileSync(fileURLToPath(new URL("../src/wasm/belge_wasm_bg.wasm", import.meta.url))) });
for (const f of process.argv.slice(2)) {
  const a = udf_ac(new Uint8Array(readFileSync(f))) as Acilis;
  const doc = belgeDugumu(a.belge, a.gorunum);
  let i = 0;
  doc.forEach((n) => {
    i++;
    if (n.type !== sema.nodes.paragraph) return;
    const t = n.textContent;
    const k = t.indexOf("Veli");
    if (k >= 0 || t.includes("Armonia Yazı deneme")) {
      const sek: number[] = []; n.forEach((c) => { if (c.type.name === "sekme") sek.push(1); });
      console.log(f.split("/").pop(), "paragraf", i, "hiza", n.attrs.hiza, "sekme", sek.length, "TabSet", JSON.stringify((n.attrs.oz as [string,string][]).find(([a]) => a === "TabSet")?.[1] ?? null), "Veli konumu", k, "uz", t.length, "deneme", t.includes("Armonia Yazı deneme"));
    }
  });
}
