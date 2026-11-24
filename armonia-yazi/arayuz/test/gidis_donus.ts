// Editör dönüşümü testi (Node, gerçek WebAssembly çekirdeğiyle). Belge METNİ yazdırılmaz.
//   npm run test:donusum -- <dosya.udf>
// Claude, 2026-11-23.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { TextSelection, EditorState } from "prosemirror-state";
import { splitBlock, toggleMark } from "prosemirror-commands";
import type { Command } from "prosemirror-state";
import { belgeDugumu, belgeyeCevir } from "../src/donustur";
import { girinti, liste, satirAraligi, yaziDegistir } from "../src/komutlar";
import { hucreGec, satirEkle, sutunEkle, sutunSil, tabloEkle, tablodaMi } from "../src/tablo";
import { ozAl } from "../src/turler";
import { sema } from "../src/sema";
import type { Acilis, Belge } from "../src/turler";
import { bosBelgeOlustur } from "../src/bos_belge";
import { belge_ac, docx_kaydet, initSync, udf_ac, udf_kaydet } from "../src/wasm/belge_wasm.js";

const wasmYolu = fileURLToPath(new URL("../src/wasm/belge_wasm_bg.wasm", import.meta.url));
initSync({ module: readFileSync(wasmYolu) });

let hata = 0;
const dogrula = (ad: string, kosul: boolean, ek = "") => {
  console.log(`${kosul ? "TAMAM" : "HATA "}  ${ad}${ek ? "  (" + ek + ")" : ""}`);
  if (!kosul) hata++;
};

const dosya = process.argv[2];
const asilBayt = new Uint8Array(readFileSync(dosya));
const a = udf_ac(asilBayt) as Acilis;
dogrula("açılış uyarısız", a.uyarilar.length === 0, a.uyarilar.join("; "));

// 1. Dokunulmamış belge: model ve UDF baytları aynı
const doc = belgeDugumu(a.belge, a.gorunum);
const geri = belgeyeCevir(doc, a.belge);
dogrula("model gidiş-dönüş", JSON.stringify(geri) === JSON.stringify(a.belge));
const b1 = udf_kaydet(a.belge) as Uint8Array, b2 = udf_kaydet(geri) as Uint8Array;
dogrula("UDF baytları aynı", b1.length === b2.length && b1.every((x, i) => x === b2[i]));

// Yardımcı: belge ile düzenle → kaydet → yeniden aç
const yenidenAc = (b: Belge) => udf_ac(udf_kaydet(b) as Uint8Array) as Acilis;
let st = EditorState.create({ doc });
const paragraflar: number[] = [];
st.doc.forEach((n, pos) => { if (n.type === sema.nodes.paragraph && n.textContent.length > 20) paragraflar.push(pos); });
const p = paragraflar[2];
const pd = st.doc.nodeAt(p)!;

// 2. Kalın: bir paragrafın ilk 5 karakteri
st = st.apply(st.tr.setSelection(TextSelection.create(st.doc, p + 1, p + 6)));
const oncekiKalin = st.doc.rangeHasMark(p + 1, p + 6, sema.marks.kalin);
toggleMark(sema.marks.kalin)(st, (tr) => (st = st.apply(tr)));
let d = yenidenAc(belgeyeCevir(st.doc, a.belge));
dogrula("kalın sonrası uyarısız", d.uyarilar.length === 0, d.uyarilar.join("; "));
const yeniDoc = belgeDugumu(d.belge, d.gorunum);
const yeniP = yeniDoc.nodeAt(p)!;
dogrula("kalın uygulandı ve korunuyor", yeniDoc.rangeHasMark(p + 1, p + 6, sema.marks.kalin) !== oncekiKalin);
dogrula("paragraf metni değişmedi", yeniP.textContent === pd.textContent);
dogrula("öteki paragraflar aynı", d.belge.govde.length === a.belge.govde.length
  && JSON.stringify(d.belge.govde.slice(0, 3)) === JSON.stringify(a.belge.govde.slice(0, 3)));

// 3. Paragraf bölme (Enter) + yazı ekleme + hizalama
st = EditorState.create({ doc });
const orta = p + 1 + Math.floor(pd.content.size / 2);
st = st.apply(st.tr.setSelection(TextSelection.create(st.doc, orta)));
splitBlock(st, (tr) => (st = st.apply(tr)));
st = st.apply(st.tr.insertText("EK"));
st = st.apply(st.tr.setNodeMarkup(p, undefined, { ...st.doc.nodeAt(p)!.attrs, hiza: "Orta" }));
d = yenidenAc(belgeyeCevir(st.doc, a.belge));
dogrula("bölme sonrası uyarısız", d.uyarilar.length === 0, d.uyarilar.join("; "));
dogrula("paragraf sayısı bir arttı", d.belge.govde.length === a.belge.govde.length + 1);
const d2 = belgeDugumu(d.belge, d.gorunum);
const ilk = d2.nodeAt(p)!, ikinci = d2.nodeAt(p + ilk.nodeSize)!;
dogrula("bölünen metin + ek korunuyor", ilk.textContent + ikinci.textContent === pd.textContent.slice(0, orta - p - 1) + "EK" + pd.textContent.slice(orta - p - 1)
  || ilk.textContent + ikinci.textContent.replace(/^EK/, "") === pd.textContent);
dogrula("hizalama kaydedildi", ilk.attrs.hiza === "Orta");
dogrula("ikinci yarı eski hizada", ikinci.attrs.hiza === pd.attrs.hiza);
// Kaydedilen her gövde paragrafı tek satır sonuyla bitmeli (UYAP biçimi)
const sonlar = d.belge.govde.filter((b) => "Paragraf" in b).every((b) => {
  const ps = (b as { Paragraf: { parcalar: object[] } }).Paragraf.parcalar;
  const son = ps[ps.length - 1] as Record<string, { metin: string }>;
  const m = son && Object.values(son)[0].metin;
  return !m || m.endsWith("\n");
});
dogrula("paragraflar satır sonuyla bitiyor", sonlar);

// 4. Biçim komutları: yazı boyutu + renk, girinti, satır aralığı, numaralı liste, sekme
{
  let s = EditorState.create({ doc });
  const q = paragraflar[3];
  const qd = s.doc.nodeAt(q)!;
  const uygula = (k: Command) => k(s, (tr) => (s = s.apply(tr)));
  s = s.apply(s.tr.setSelection(TextSelection.create(s.doc, q + 1, q + 8)));
  uygula(yaziDegistir("boyut", 17));
  uygula(yaziDegistir("renk", { r: 0, g: 0, b: 200 }));
  uygula(girinti(1));
  uygula(satirAraligi(0.5));
  uygula(liste("numara"));
  s = s.apply(s.tr.setSelection(TextSelection.create(s.doc, q + 3)));
  s = s.apply(s.tr.replaceSelectionWith(sema.nodes.sekme.create({ oz: [], metin: "\t" }), false));
  const e = yenidenAc(belgeyeCevir(s.doc, a.belge));
  dogrula("biçim komutları sonrası uyarısız", e.uyarilar.length === 0, e.uyarilar.join("; "));
  const ed = belgeDugumu(e.belge, e.gorunum);
  const en = ed.nodeAt(q)!;
  let boyutTamam = false, renkTamam = false, sekme = 0;
  en.forEach((c, ofs) => {
    const y = c.marks.find((m) => m.type.name === "yazi");
    if (c.isText && ofs < 7 && y) {
      boyutTamam ||= y.attrs.boyut === 17;
      renkTamam ||= y.attrs.renk.b === 200;
    }
    if (c.type.name === "sekme") sekme++;
  });
  dogrula("yazı boyutu kaydedildi", boyutTamam);
  dogrula("renk kaydedildi", renkTamam);
  dogrula("girinti kaydedildi", Math.abs(en.attrs.sol - (qd.attrs.sol + 35.4)) < 0.01, `${qd.attrs.sol} -> ${en.attrs.sol}`);
  dogrula("satır aralığı kaydedildi", en.attrs.satirAraligi === 0.5);
  dogrula("numaralı liste kaydedildi ve önek hesaplandı", typeof en.attrs.onek === "string" && /^\d+\.$/.test(en.attrs.onek), String(en.attrs.onek));
  dogrula("sekme eklendi", sekme >= 1);
  dogrula("paragraf metni korunuyor", en.textContent === qd.textContent);
}

// 5. Tablo: ekle, sütun/satır ekle, hücrelere yaz, kaydet-aç; tablolu belgenin gidiş-dönüşü; sütun sil
{
  let s = EditorState.create({ doc });
  const uygula = (k: Command) => k(s, (tr) => (s = s.apply(tr)));
  const q = paragraflar[1];
  s = s.apply(s.tr.setSelection(TextSelection.create(s.doc, q + 2)));
  uygula(tabloEkle(2, 2));
  dogrula("imleç yeni tablonun hücresinde", tablodaMi(s));
  s = s.apply(s.tr.insertText("A1"));
  uygula(sutunEkle("sag"));
  uygula(hucreGec(1));
  s = s.apply(s.tr.insertText("B1"));
  uygula(satirEkle("alt"));
  s = s.apply(s.tr.insertText("Ç2"));
  const t = yenidenAc(belgeyeCevir(s.doc, a.belge));
  dogrula("tablolu belge uyarısız açılıyor", t.uyarilar.length === 0, t.uyarilar.join("; "));
  const tablolar = t.belge.govde.filter((b) => "Tablo" in b) as { Tablo: import("../src/turler").Tablo }[];
  dogrula("tablo kaydedildi", tablolar.length === 1);
  const tb = tablolar[0].Tablo;
  dogrula("satır sayısı 3", tb.satirlar.length === 3, String(tb.satirlar.length));
  dogrula("sütun sayısı 3", tb.satirlar.every((r) => r.hucreler.length === 3), tb.satirlar.map((r) => r.hucreler.length).join(","));
  // İlk sütunun sağına eklenen sütun onun genişliğini paylaşır: 1,1 → 0.5,0.5,1
  dogrula("columnSpans güncellendi", ozAl(tb.oz, "columnSpans") === "0.5,0.5,1", ozAl(tb.oz, "columnSpans"));
  const metin = (h: import("../src/turler").Hucre) => h.bloklar.map((b) => ("Paragraf" in b ? b.Paragraf.parcalar.map((p) => Object.values(p)[0].metin).join("") : "")).join("").replace(/\n/g, "");
  dogrula("hücre metinleri", metin(tb.satirlar[0].hucreler[0]) === "A1" && metin(tb.satirlar[0].hucreler[1]) === "B1",
    tb.satirlar.map((r) => r.hucreler.map(metin).join("|")).join(" / "));
  dogrula("yeni satıra yazılan", tb.satirlar.some((r) => r.hucreler.some((h) => metin(h) === "Ç2")));
  // Tablolu belge: dokunmadan gidiş-dönüş bayt bayt aynı
  const td = belgeDugumu(t.belge, t.gorunum);
  const g1 = udf_kaydet(t.belge) as Uint8Array, g2 = udf_kaydet(belgeyeCevir(td, t.belge)) as Uint8Array;
  dogrula("tablolu belge gidiş-dönüş bayt bayt", g1.length === g2.length && g1.every((x, i) => x === g2[i]));
  // Sütun sil: ilk hücreye git, sütunu sil → 2 sütun
  let s2 = EditorState.create({ doc: td });
  let ilkHucre = -1;
  s2.doc.descendants((n, pos) => { if (ilkHucre < 0 && n.type === sema.nodes.hucre) ilkHucre = pos; });
  s2 = s2.apply(s2.tr.setSelection(TextSelection.create(s2.doc, ilkHucre + 2)));
  sutunSil(s2, (tr) => (s2 = s2.apply(tr)));
  const t2 = yenidenAc(belgeyeCevir(s2.doc, t.belge));
  const tb2 = (t2.belge.govde.find((b) => "Tablo" in b) as { Tablo: import("../src/turler").Tablo }).Tablo;
  dogrula("sütun silindi", tb2.satirlar.every((r) => r.hucreler.length === 2), ozAl(tb2.oz, "columnSpans"));
}

// 6. DOCX (isteğe bağlı 2. argüman): içerikten tanınır, editöre ve geri dönüşüm kayıpsız, UDF olarak kaydedilir
if (process.argv[3]) {
  const d = belge_ac(new Uint8Array(readFileSync(process.argv[3]))) as Acilis & { tur: string };
  dogrula("DOCX içerikten tanındı", d.tur === "docx", d.tur);
  const dd = belgeDugumu(d.belge, d.gorunum);
  dogrula("DOCX editör gidiş-dönüş", JSON.stringify(belgeyeCevir(dd, d.belge)) === JSON.stringify(d.belge));
  const u = udf_ac(udf_kaydet(belgeyeCevir(dd, d.belge)) as Uint8Array) as Acilis;
  dogrula("DOCX → UDF uyarısız açılıyor", u.uyarilar.length === 0, u.uyarilar.join("; "));
  dogrula("DOCX → UDF paragraf sayısı aynı", u.belge.govde.length === d.belge.govde.length);
}

// 7. Yeni boş belge: yaz, aç (uyarısız), editöre ve geri; DOCX olarak da yazılır
{
  const bos = bosBelgeOlustur();
  const u = udf_ac(udf_kaydet(bos) as Uint8Array) as Acilis;
  dogrula("yeni belge UDF uyarısız açılıyor", u.uyarilar.length === 0, u.uyarilar.join("; "));
  dogrula("yeni belge gidiş-dönüş", JSON.stringify(belgeyeCevir(belgeDugumu(u.belge, u.gorunum), u.belge)) === JSON.stringify(u.belge));
  const d = belge_ac(docx_kaydet(bos) as Uint8Array) as Acilis & { tur: string };
  dogrula("yeni belge DOCX olarak yazılıp açılıyor", d.tur === "docx" && d.uyarilar.length === 0, d.uyarilar.join("; "));
}

console.log(hata ? `${hata} HATA` : "HEPSİ TAMAM");
process.exit(hata ? 1 : 0);
