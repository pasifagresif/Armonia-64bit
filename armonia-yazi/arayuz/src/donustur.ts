// Belge (UDF modeli) ↔ editör belgesi (ProseMirror) dönüşümü. Kayıpsızlık:
//  - Her metin parçası UDF'teki etiketini ve ham özelliklerini `udf` işaretinde taşır; atom düğümler (sekme,
//    alan, resim) ham özelliklerini öz niteliğinde.
//  - Kaydederken yalnız açılıştaki çözülmüş stilden FARKLI olan biçim açık özellik olarak yazılır; dokunulmayan
//    belge aynen geri çıkar (gidiş-dönüş denetimi: main.ts).
//  - Paragraf sonundaki "\n": UYAP her paragrafı bir satır sonuyla bitirir; ya ayrı bir parça ("\n", kendi
//    özellikleriyle) ya da son parçanın sonunda. Biçimi `satirSonuOz` niteliğinde saklanır.
// Claude, 2026-11-23.

import { Fragment, type Mark, type Node as PMNode } from "prosemirror-model";
import { bicimIsaretleri, paragrafAlanlari, sema } from "./sema";
import type { Belge, Blok, GBlok, Gorunum, GTablo, Hucre, Ozellikler, Paragraf, ParagrafStili, Parca, ParcaStili, Renk, Satir, Tablo } from "./turler";
import { HIZA_UDF, ozAyarla, parcaIci, parcaTuru, renkArgb } from "./turler";

type SatirSonu = { oz: Ozellikler } | "birlesik" | "yok";

// ── Belge → editör ───────────────────────────────────────────────────────────────────────────────────

export function paragrafDugumu(p: Paragraf, g: Extract<GBlok, { Paragraf: unknown }>["Paragraf"]): PMNode {
  const parcalar = p.parcalar.slice();
  const stiller = g.parcalar.slice();
  let satirSonu: SatirSonu = "yok";
  const son = parcalar[parcalar.length - 1];
  if (son && parcaTuru(son) === "Metin") {
    const ic = parcaIci(son);
    if (ic.metin === "\n") {
      satirSonu = { oz: ic.oz };
      parcalar.pop();
      stiller.pop();
    } else if (ic.metin.endsWith("\n")) {
      satirSonu = "birlesik";
      parcalar[parcalar.length - 1] = { Metin: { metin: ic.metin.slice(0, -1), oz: ic.oz } };
    }
  }
  const icerik: PMNode[] = [];
  parcalar.forEach((c, i) => {
    const tur = parcaTuru(c);
    const { metin, oz } = parcaIci(c);
    const stil = stiller[i];
    if (tur === "Metin" || tur === "Bosluk") {
      if (!metin) return;
      const isaretler: Mark[] = [
        sema.marks.udf.create({ etiket: tur === "Metin" ? "content" : "space", oz, ozgun: stil }),
        ...bicimIsaretleri(stil),
      ];
      icerik.push(sema.text(metin, isaretler));
    } else {
      const dugum = tur === "Sekme" ? sema.nodes.sekme : tur === "Alan" ? sema.nodes.alan : sema.nodes.resim;
      // Atom düğümler de biçim işaretlerini taşır (alan metni, sekme genişliği yazı boyutuna göre)
      icerik.push(dugum.create({ oz, metin, stil }, null, bicimIsaretleri(stil)));
    }
  });
  return sema.nodes.paragraph.create(
    { oz: p.oz, ozgun: g.stil, satirSonuOz: satirSonu, onek: g.onek, ...paragrafAlanlari(g.stil) },
    icerik,
  );
}

function bloklarDugumu(bloklar: Blok[], gorunum: GBlok[]): PMNode[] {
  return bloklar.map((b, i) => {
    const g = gorunum[i];
    if ("Paragraf" in b && g && "Paragraf" in g) return paragrafDugumu(b.Paragraf, g.Paragraf);
    if ("Tablo" in b && g && "Tablo" in g) return tabloDugumu(b.Tablo, g.Tablo);
    throw new Error("belge ile görünüm uyuşmuyor");
  });
}

export function tabloDugumu(t: Tablo, g: GTablo): PMNode {
  const satirlar = t.satirlar.map((r, ri) => {
    const hucreler = r.hucreler.map((h, hi) => {
      const ic = bloklarDugumu(h.bloklar, g.satirlar[ri]?.[hi] ?? []);
      // Şema hücrede en az bir blok ister; boş hücreye boş paragraf (UYAP hücreleri her zaman paragraf taşır)
      if (!ic.length) ic.push(sema.nodes.paragraph.create());
      return sema.nodes.hucre.create({ oz: h.oz }, ic);
    });
    const b = g.satir_bilgisi[ri];
    return sema.nodes.satir.create({ oz: r.oz, oranlar: b?.oranlar ?? [], minYukseklik: b?.min_yukseklik ?? 0 }, hucreler);
  });
  return sema.nodes.tablo.create({ oz: t.oz, stil: g.stil }, satirlar);
}

export function belgeDugumu(belge: Belge, gorunum: Gorunum): PMNode {
  const bloklar = bloklarDugumu(belge.govde, gorunum.govde);
  if (bloklar.length === 0) bloklar.push(sema.nodes.paragraph.create());
  return sema.nodes.doc.create(null, bloklar);
}

// ── Editör → belge ───────────────────────────────────────────────────────────────────────────────────

function sayiMetni(x: number): string {
  return Number.isInteger(x) ? String(x) : String(x);
}

function ayniRenk(a: Renk, b: Renk): boolean {
  return a.r === b.r && a.g === b.g && a.b === b.b;
}

/** Metnin görünen biçimini, açılıştaki stile göre değişenleri açık özellik olarak ham özelliklere işler. */
function bicimiIsle(oz: Ozellikler, ozgun: ParcaStili | null, isaretler: readonly Mark[]): Ozellikler {
  const var_ = (ad: string) => isaretler.some((m) => m.type.name === ad);
  const yazi = isaretler.find((m) => m.type.name === "yazi");
  const kalin = var_("kalin"), egik = var_("egik"), alti = var_("alti");
  let y = oz;
  if (ozgun) {
    if (kalin !== ozgun.kalin) y = ozAyarla(y, "bold", String(kalin));
    if (egik !== ozgun.egik) y = ozAyarla(y, "italic", String(egik));
    if (alti !== ozgun.alti_cizili) y = ozAyarla(y, "underline", String(alti));
    if (yazi) {
      if (yazi.attrs.aile !== ozgun.aile) y = ozAyarla(y, "family", yazi.attrs.aile);
      if (yazi.attrs.boyut !== ozgun.boyut) y = ozAyarla(y, "size", sayiMetni(yazi.attrs.boyut));
      if (!ayniRenk(yazi.attrs.renk, ozgun.renk)) y = ozAyarla(y, "foreground", renkArgb(yazi.attrs.renk));
    }
  } else {
    // Yeni metin (açılışta yoktu): görünen biçimin tamamı açık yazılır
    if (kalin) y = ozAyarla(y, "bold", "true");
    if (egik) y = ozAyarla(y, "italic", "true");
    if (alti) y = ozAyarla(y, "underline", "true");
    if (yazi) {
      y = ozAyarla(y, "family", yazi.attrs.aile);
      y = ozAyarla(y, "size", sayiMetni(yazi.attrs.boyut));
      y = ozAyarla(y, "foreground", renkArgb(yazi.attrs.renk));
    }
  }
  return y;
}

/** Paragrafın görünen biçim alanı → UDF özelliği (açılıştaki stilden farklıysa yazılır). */
const PARAGRAF_ALANLARI: [string, keyof ParagrafStili, string][] = [
  ["sol", "sol", "LeftIndent"],
  ["sag", "sag", "RightIndent"],
  ["ilkSatir", "ilk_satir", "FirstLineIndent"],
  ["satirAraligi", "satir_araligi", "LineSpacing"],
  ["ustBosluk", "ust_bosluk", "SpaceAbove"],
  ["altBosluk", "alt_bosluk", "SpaceBelow"],
];

function paragrafOzu(n: PMNode): Ozellikler {
  const ozgun = n.attrs.ozgun as ParagrafStili | null;
  let oz = n.attrs.oz as Ozellikler;
  if (!ozgun || n.attrs.hiza !== ozgun.hiza) oz = ozAyarla(oz, "Alignment", HIZA_UDF[n.attrs.hiza as keyof typeof HIZA_UDF]);
  for (const [alan, stilAlani, udf] of PARAGRAF_ALANLARI) {
    const deger = n.attrs[alan] as number;
    const eski = ozgun ? (ozgun[stilAlani] as number) : 0;
    if (Math.abs(deger - eski) > 1e-9) oz = ozAyarla(oz, udf, sayiMetni(deger));
  }
  return oz;
}

export function paragrafiCevir(n: PMNode): Paragraf {
  const parcalar: Parca[] = [];
  n.forEach((c) => {
    if (c.isText) {
      const udf = c.marks.find((m) => m.type.name === "udf");
      const oz = bicimiIsle(udf?.attrs.oz ?? [], udf?.attrs.ozgun ?? null, c.marks);
      const metin = c.text ?? "";
      parcalar.push(udf?.attrs.etiket === "space" ? { Bosluk: { metin, oz } } : { Metin: { metin, oz } });
    } else {
      const ic = { metin: c.attrs.metin as string, oz: c.attrs.oz as Ozellikler };
      if (c.type.name === "sekme") parcalar.push({ Sekme: ic });
      else if (c.type.name === "alan") parcalar.push({ Alan: ic });
      else if (c.type.name === "resim") parcalar.push({ Resim: ic });
    }
  });
  const satirSonu = n.attrs.satirSonuOz as SatirSonu | null;
  const son = parcalar[parcalar.length - 1];
  if (satirSonu && typeof satirSonu === "object") {
    parcalar.push({ Metin: { metin: "\n", oz: satirSonu.oz } });
  } else if (satirSonu === "birlesik" && son && ("Metin" in son || "Bosluk" in son)) {
    const ic = parcaIci(son);
    parcalar[parcalar.length - 1] = "Metin" in son ? { Metin: { metin: ic.metin + "\n", oz: ic.oz } } : { Bosluk: { metin: ic.metin + "\n", oz: ic.oz } };
  } else if (satirSonu !== "yok") {
    // Yeni paragraf ya da sonu değişmiş: son metin parçasının biçimiyle ayrı bir satır sonu
    const oz = son && ("Metin" in son || "Bosluk" in son) ? parcaIci(son).oz : [];
    parcalar.push({ Metin: { metin: "\n", oz } });
  }
  return { oz: paragrafOzu(n), parcalar };
}

function bloklariCevir(n: PMNode): Blok[] {
  const bloklar: Blok[] = [];
  n.forEach((c) => {
    if (c.type.name === "paragraph") bloklar.push({ Paragraf: paragrafiCevir(c) });
    else if (c.type.name === "tablo") bloklar.push({ Tablo: tabloyuCevir(c) });
  });
  return bloklar;
}

export function tabloyuCevir(n: PMNode): Tablo {
  const satirlar: Satir[] = [];
  n.forEach((r) => {
    const hucreler: Hucre[] = [];
    r.forEach((h) => hucreler.push({ oz: h.attrs.oz as Ozellikler, bloklar: bloklariCevir(h) }));
    satirlar.push({ oz: r.attrs.oz as Ozellikler, hucreler });
  });
  return { oz: n.attrs.oz as Ozellikler, satirlar };
}

/** Editör belgesini, açılan belgenin geri kalanıyla (stiller, sayfa, üst/alt bilgi...) birleştirip modele çevirir. */
export function belgeyeCevir(doc: PMNode, asil: Belge): Belge {
  return { ...asil, govde: bloklariCevir(doc) };
}

export const bosBelge = (): Fragment => Fragment.from(sema.nodes.paragraph.create());
