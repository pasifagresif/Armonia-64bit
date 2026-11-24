// Salt okunur bölümlerin çizimi (üst/alt bilgi, tablolar): editördeki düzenlenebilir paragrafla aynı stil
// kurallarıyla (sema.ts). Metin her zaman textContent ile yazılır (belgeden gelen HTML yorumlanmaz).
// Claude, 2026-11-23.

import { cssAilesi, satirYuksekligi } from "./sema";
import type { Blok, GBlok, Paragraf, ParagrafStili, ParcaStili } from "./turler";
import { parcaIci, parcaTuru, renkCss } from "./turler";

const HIZA: Record<string, string> = { Sol: "left", Orta: "center", Sag: "right", IkiYana: "justify" };

function paragrafStili(el: HTMLElement, s: ParagrafStili) {
  Object.assign(el.style, {
    textAlign: HIZA[s.hiza] ?? "left",
    paddingLeft: `${s.sol}pt`,
    paddingRight: `${s.sag}pt`,
    textIndent: `${s.ilk_satir}pt`,
    lineHeight: satirYuksekligi(s.satir_araligi),
    margin: `${s.ust_bosluk}pt 0 ${s.alt_bosluk}pt 0`,
  });
}

function parcaStili(el: HTMLElement, s: ParcaStili) {
  el.style.fontFamily = cssAilesi(s.aile);
  el.style.fontSize = `${s.boyut}pt`;
  el.style.color = renkCss(s.renk);
  if (s.kalin) el.style.fontWeight = "bold";
  if (s.egik) el.style.fontStyle = "italic";
  if (s.alti_cizili) el.style.textDecoration = "underline";
}

export function paragrafCiz(p: Paragraf, g: Extract<GBlok, { Paragraf: unknown }>["Paragraf"], sayfaNo?: string): HTMLElement {
  const el = document.createElement("p");
  el.className = "p";
  paragrafStili(el, g.stil);
  if (g.onek) el.dataset.onek = g.onek;
  el.dataset.sekmeler = JSON.stringify(g.stil.sekmeler);
  p.parcalar.forEach((c, i) => {
    const s = g.parcalar[i];
    const { metin, oz } = parcaIci(c);
    const tur = parcaTuru(c);
    if (tur === "Resim") {
      const img = document.createElement("img");
      const al = (k: string) => oz.find(([a]) => a === k)?.[1];
      img.src = `data:image/png;base64,${(al("imageData") ?? "").replace(/[^A-Za-z0-9+/=]/g, "")}`;
      const w = parseFloat(al("width") ?? "0"), h = parseFloat(al("height") ?? "0");
      if (w > 0) img.style.width = `${w}pt`;
      if (h > 0) img.style.height = `${h}pt`;
      img.alt = "";
      el.append(img);
      return;
    }
    const span = document.createElement("span");
    parcaStili(span, s);
    // Alan (ör. sayfa numarası): alt bilgide sayfa numarası gösterilir
    // Sekme: genişliğini sayfa.ts sekmeleriYerlestir duraklara göre verir
    span.textContent = tur === "Sekme" ? "" : tur === "Alan" && sayfaNo ? sayfaNo : metin.replace(/\n/g, "");
    if (tur === "Alan") span.classList.add("alan");
    if (tur === "Sekme") span.classList.add("sekme");
    el.append(span);
  });
  return el;
}

export function bloklariCiz(bloklar: Blok[], gorunum: GBlok[], hedef: HTMLElement) {
  bloklar.forEach((b, i) => {
    const g = gorunum[i];
    if ("Paragraf" in b && "Paragraf" in g) hedef.append(paragrafCiz(b.Paragraf, g.Paragraf));
    else if ("Tablo" in b && "Tablo" in g) {
      // Editördeki tabloyla aynı yapı (sema.ts tablo/satir/hucre)
      const st = g.Tablo.stil;
      const t = document.createElement("div");
      t.className = "tablo";
      t.dataset.kenarlik = st.kenarlik;
      t.style.margin = `${st.ust_bosluk}pt ${st.sag}pt ${st.alt_bosluk}pt ${st.sol}pt`;
      b.Tablo.satirlar.forEach((r, ri) => {
        const sb = g.Tablo.satir_bilgisi[ri];
        const satir = document.createElement("div");
        satir.className = "satir";
        const o = sb?.oranlar.length ? sb.oranlar : r.hucreler.map(() => 1);
        satir.style.gridTemplateColumns = o.map((x) => `${Math.max(x, 0.001)}fr`).join(" ");
        satir.style.minHeight = `${sb?.min_yukseklik ?? 0}pt`;
        r.hucreler.forEach((h, hi) => {
          const hucre = document.createElement("div");
          hucre.className = "hucre";
          bloklariCiz(h.bloklar, g.Tablo.satirlar[ri]?.[hi] ?? [], hucre);
          satir.append(hucre);
        });
        t.append(satir);
      });
      hedef.append(t);
    }
  });
}
