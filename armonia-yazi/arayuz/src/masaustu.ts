// Masaüstü (Tauri) köprüsü. Arayüz tarayıcıda da çalışır: Tauri yoksa bu işlevler kullanılmaz (main.ts tarayıcı
// yoluna döner). Tauri kütüphanesi yalnız masaüstünde yüklenir (dinamik import). Claude, 2026-11-24.

export const masaustu: boolean = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function cagir<T>(komut: string, arg?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(komut, arg);
}

export interface MasaustuDosya {
  ad: string;
  /** Kabuğun dosya listesindeki kimlik (arayüz dosya yolu göndermez; "Kaydet" bununla yazar) */
  kimlik: number;
  /** base64 */
  bayt: string;
}

export interface MasaustuKayit {
  ad: string;
  kimlik: number;
  /** Yalnız bildirimde gösterilir */
  yol: string;
}

export interface ImzaBilgisi {
  imzalayan: string;
  seri: string;
  veren: string;
  gecerlilik_bas: string;
  gecerlilik_son: string;
  imza_zamani: string | null;
  belge_degismemis: boolean;
  imza_gecerli: boolean;
  zincir_dogrulandi: boolean;
  sure_icinde: boolean;
}

export function b64Kodla(v: Uint8Array): string {
  let s = "";
  const adim = 0x8000;
  for (let i = 0; i < v.length; i += adim) s += String.fromCharCode(...v.subarray(i, i + adim));
  return btoa(s);
}

export function b64Coz(s: string): Uint8Array {
  const ikili = atob(s);
  const v = new Uint8Array(ikili.length);
  for (let i = 0; i < ikili.length; i++) v[i] = ikili.charCodeAt(i);
  return v;
}
