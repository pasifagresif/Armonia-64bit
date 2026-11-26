// Antet: sayfa üst bilgisindeki resim. UYAP antedi şöyle yazar (gerçek UDF, 26.11.2026):
//   <header><paragraph family="Times New Roman" size="12" description="Gövde">
//     <image width="453.0" height="59.0" resolver="hvl-default" imageData="(PNG, base64)" .. />
//     <content family="Times New Roman" size="12" description="Gövde" .. /></paragraph></header>
// Resmin metindeki yeri "¸" karakteridir; PNG'nin piksel boyutu width/height (pt) ile aynıdır. Burada da öyle
// üretilir: resim yazı alanı genişliğine sığdırılır ve tam o boyutta (1 piksel = 1 pt) PNG'ye çevrilir.
// Kabuk (Çoklu UYAP, window.armoniaYaziKabuk) büro antetlerini verir ve kaydeder; yoksa yalnız bilgisayardan resim.
// Claude, 2026-11-26.

import type { Belge, Blok, Kenar, Ozellikler, Parca } from "./turler";
import { ozAl } from "./turler";

export interface Antet {
  resim: string; // PNG, base64 (önek yok)
  genislik: number; // pt
  yukseklik: number; // pt
}

export interface KayitliAntet extends Antet {
  id: number;
  ad: string;
  varsayilan: boolean;
}

/** Çoklu UYAP'ın editör çerçevesine verdiği arayüz (yazi.js). */
export interface Kabuk {
  antetler(): Promise<KayitliAntet[]>;
  varsayilanAntet(): Promise<KayitliAntet | null>;
  antetKaydet(ad: string, a: Antet): Promise<void>;
}

export function kabuk(): Kabuk | null {
  const k = (window as unknown as { armoniaYaziKabuk?: Kabuk }).armoniaYaziKabuk;
  return k && typeof k.antetler === "function" ? k : null;
}

/** Antedin en çok yüksekliği (pt); daha uzun resim oranı korunarak küçültülür. */
const EN_COK_YUKSEKLIK = 170;

/** Dosyadaki resmi (PNG, JPEG, GIF, BMP, WebP) antede çevirir: en çok `enCokGenislik` pt genişlikte, oranı korunur. */
export async function resimdenAntet(f: Blob, enCokGenislik: number): Promise<Antet> {
  const url = URL.createObjectURL(f);
  try {
    const img = new Image();
    img.src = url;
    await img.decode().catch(() => { throw new Error("Resim açılamadı (PNG, JPEG, GIF, BMP ya da WebP olmalı)."); });
    const w0 = img.naturalWidth, h0 = img.naturalHeight;
    if (!w0 || !h0) throw new Error("Resim boyutu okunamadı.");
    const k = Math.min(1, enCokGenislik / w0, EN_COK_YUKSEKLIK / h0);
    const w = Math.max(1, Math.round(w0 * k)), h = Math.max(1, Math.round(h0 * k));
    const tuval = document.createElement("canvas");
    tuval.width = w;
    tuval.height = h;
    const c = tuval.getContext("2d")!;
    c.imageSmoothingQuality = "high";
    c.drawImage(img, 0, 0, w, h);
    const resim = tuval.toDataURL("image/png").replace(/^data:image\/png;base64,/, "");
    return { resim, genislik: w, yukseklik: h };
  } finally {
    URL.revokeObjectURL(url);
  }
}

const olcuMetni = (x: number) => (Number.isInteger(x) ? `${x}.0` : String(Math.round(x * 100) / 100));

const GOVDE_OZ: Ozellikler = [["family", "Times New Roman"], ["size", "12"], ["description", "Gövde"]];

function resimParcasi(a: Antet): Parca {
  return { Resim: { metin: "¸", oz: [["width", olcuMetni(a.genislik)], ["height", olcuMetni(a.yukseklik)], ["resolver", "hvl-default"], ["imageData", a.resim]] } };
}

const resimMi = (c: Parca) => "Resim" in c;

/** Üst bilgideki ilk resim (antet); yoksa null. */
export function antetiBul(belge: Belge): Antet | null {
  for (const b of belge.ust_bilgi?.bloklar ?? []) {
    if (!("Paragraf" in b)) continue;
    for (const c of b.Paragraf.parcalar) {
      if (!("Resim" in c)) continue;
      const oz = c.Resim.oz;
      const resim = (ozAl(oz, "imageData") ?? "").replace(/[^A-Za-z0-9+/=]/g, "");
      if (!resim) continue;
      return { resim, genislik: Number(ozAl(oz, "width")) || 0, yukseklik: Number(ozAl(oz, "height")) || 0 };
    }
  }
  return null;
}

/** Antedi koyar (`a`) ya da kaldırır (null). Üst bilgide resim varsa ilki değiştirilir (paragrafı, hizası
 * korunur); yoksa üst bilgiye ortalı yeni bir paragraf olarak en başa eklenir. Kaldırırken yalnız resimler
 * çıkar; resimden başka içeriği olmayan üst bilgi tümüyle kalkar. Belgenin geri kalanına dokunulmaz. */
export function antetiKoy(belge: Belge, a: Antet | null): Belge {
  const ust = belge.ust_bilgi;
  if (a) {
    if (ust && antetiBul(belge)) {
      let degisti = false;
      const bloklar = ust.bloklar.map((b): Blok => {
        if (degisti || !("Paragraf" in b)) return b;
        const i = b.Paragraf.parcalar.findIndex((c) => "Resim" in c && !!ozAl(c.Resim.oz, "imageData"));
        if (i < 0) return b;
        degisti = true;
        const parcalar = b.Paragraf.parcalar.slice();
        parcalar[i] = resimParcasi(a);
        return { Paragraf: { ...b.Paragraf, parcalar } };
      });
      return { ...belge, ust_bilgi: { ...ust, bloklar } };
    }
    const p: Blok = { Paragraf: { oz: [...GOVDE_OZ, ["Alignment", "1"]], parcalar: [resimParcasi(a), { Metin: { metin: "\n", oz: GOVDE_OZ } }] } };
    const yeni: Kenar = ust ? { ...ust, bloklar: [p, ...ust.bloklar] } : { oz: [], bloklar: [p] };
    return { ...belge, ust_bilgi: yeni };
  }
  if (!ust) return belge;
  const bloklar = ust.bloklar.map((b): Blok => ("Paragraf" in b
    ? { Paragraf: { ...b.Paragraf, parcalar: b.Paragraf.parcalar.filter((c) => !resimMi(c)) } }
    : b));
  const doluMu = bloklar.some((b) => !("Paragraf" in b)
    || b.Paragraf.parcalar.some((c) => !("Metin" in c || "Bosluk" in c) || /\S/.test("Metin" in c ? c.Metin.metin : "Bosluk" in c ? c.Bosluk.metin : "")));
  return { ...belge, ust_bilgi: doluMu ? { ...ust, bloklar } : null };
}
