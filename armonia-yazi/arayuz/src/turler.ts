// Belge çekirdeğinin (Rust, serde) JSON biçimi. belge_cekirdek/src/model.rs, stil.rs, gorunum.rs ile aynı tutulmalı.
// Claude, 2026-11-23.

/** Öğe özellikleri: [ad, ham değer] çiftleri, dosyadaki sırayla. */
export type Ozellikler = [string, string][];

export type Parca =
  | { Metin: { metin: string; oz: Ozellikler } }
  | { Bosluk: { metin: string; oz: Ozellikler } }
  | { Sekme: { metin: string; oz: Ozellikler } }
  | { Alan: { metin: string; oz: Ozellikler } }
  | { Resim: { metin: string; oz: Ozellikler } };

export type ParcaTuru = "Metin" | "Bosluk" | "Sekme" | "Alan" | "Resim";

export interface Paragraf { oz: Ozellikler; parcalar: Parca[] }
export interface Hucre { oz: Ozellikler; bloklar: Blok[] }
export interface Satir { oz: Ozellikler; hucreler: Hucre[] }
export interface Tablo { oz: Ozellikler; satirlar: Satir[] }
export type Blok = { Paragraf: Paragraf } | { Tablo: Tablo };
export interface Kenar { oz: Ozellikler; bloklar: Blok[] }
export interface Stil { ad: string; oz: Ozellikler }

export interface Belge {
  kok: Ozellikler;
  sayfa: Ozellikler;
  govde_oz: Ozellikler;
  ust_bilgi: Kenar | null;
  alt_bilgi: Kenar | null;
  govde: Blok[];
  stiller: Stil[];
  ek_dosyalar: [string, number[]][];
  ek_ogeler: string[];
}

export interface Renk { r: number; g: number; b: number }

export interface ParcaStili {
  aile: string;
  boyut: number;
  kalin: boolean;
  egik: boolean;
  alti_cizili: boolean;
  renk: Renk;
}

export type Hiza = "Sol" | "Orta" | "Sag" | "IkiYana";

export interface ParagrafStili {
  hiza: Hiza;
  sol: number;
  sag: number;
  ilk_satir: number;
  satir_araligi: number;
  ust_bosluk: number;
  alt_bosluk: number;
  sekmeler: { konum: number; hiza: number }[];
  liste: unknown;
}

export type Kenarlik = "Hucre" | "Tablo" | "Yok";
export interface TabloStili { sol: number; sag: number; ust_bosluk: number; alt_bosluk: number; kenarlik: Kenarlik }
export interface SatirBilgisi { oranlar: number[]; min_yukseklik: number }
export interface GTablo { satirlar: GBlok[][][]; stil: TabloStili; satir_bilgisi: SatirBilgisi[] }

export type GBlok =
  | { Paragraf: { stil: ParagrafStili; onek: string | null; parcalar: ParcaStili[] } }
  | { Tablo: GTablo };

export interface SayfaBicimi {
  genislik: number;
  yukseklik: number;
  sol: number;
  sag: number;
  ust: number;
  alt: number;
  ust_bilgi_payi: number;
  alt_bilgi_payi: number;
  yatay: boolean;
}

export interface KenarBilgisi { bas: number; bit: number; gorunur: boolean }

export interface SayfaNumarasi {
  bas: number;
  toplam_goster: boolean;
  ayirici: string;
  on_ek: string;
  hiza: Hiza;
  stil: ParcaStili;
}

export interface Gorunum {
  sayfa: SayfaBicimi;
  ust_bilgi: GBlok[] | null;
  govde: GBlok[];
  alt_bilgi: GBlok[] | null;
  ust_kenar: KenarBilgisi | null;
  alt_kenar: KenarBilgisi | null;
  sayfa_numarasi: SayfaNumarasi | null;
}

export function kenarSayfada(k: KenarBilgisi | null, n: number): boolean {
  return !!k && k.gorunur && n >= k.bas && (k.bit <= 0 || n <= k.bit);
}

/** belge_cekirdek gorunum.rs `SayfaNumarasi::metin` ile aynı. */
export function sayfaNoMetni(s: SayfaNumarasi, n: number, toplam: number): string {
  let m = String(n + s.bas - 1);
  if (s.toplam_goster) m += s.ayirici + String(toplam + s.bas - 1);
  return s.on_ek ? `${s.on_ek} ${m}` : m;
}

export interface Acilis {
  belge: Belge;
  gorunum: Gorunum;
  imzali: boolean;
  uyarilar: string[];
}

export function parcaTuru(p: Parca): ParcaTuru {
  return Object.keys(p)[0] as ParcaTuru;
}

export function parcaIci(p: Parca): { metin: string; oz: Ozellikler } {
  return Object.values(p)[0] as { metin: string; oz: Ozellikler };
}

export function ozAl(oz: Ozellikler, ad: string): string | undefined {
  return oz.find(([a]) => a === ad)?.[1];
}

/** Değeri ayarlar (varsa yerinde, yoksa sona); yeni dizi döner. */
export function ozAyarla(oz: Ozellikler, ad: string, deger: string): Ozellikler {
  const i = oz.findIndex(([a]) => a === ad);
  if (i >= 0) {
    const y = oz.slice();
    y[i] = [ad, deger];
    return y;
  }
  return [...oz, [ad, deger]];
}

export const HIZA_UDF: Record<Hiza, string> = { Sol: "0", Orta: "1", Sag: "2", IkiYana: "3" };

/** UDF rengi: işaretli 32 bit ARGB (opak). */
export function renkArgb(r: Renk): string {
  return (((0xff << 24) | (r.r << 16) | (r.g << 8) | r.b) | 0).toString();
}

export function renkCss(r: Renk): string {
  return `rgb(${r.r}, ${r.g}, ${r.b})`;
}

export function renkHex(r: Renk): string {
  return "#" + [r.r, r.g, r.b].map((x) => x.toString(16).padStart(2, "0")).join("");
}

export function hexRenk(h: string): Renk {
  const n = parseInt(h.replace("#", ""), 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}
