// İmza penceresi (masaüstü): karttaki sertifikayı seç, PIN gir, imzala. Belge ve PIN yalnız yerel imza hizmetine
// gider (Tauri komutu → /run/armonia-imza/imza.sock); hizmet her isteği imza günlüğüne yazar. PIN alanı imzadan hemen
// sonra temizlenir; hiçbir yerde saklanmaz. Claude, 2026-11-24.

import { b64Coz, b64Kodla, cagir, type ImzaBilgisi } from "./masaustu";

interface Sertifika {
  /** Adalet istemcisi: kimlik; PKCS#11: seri */
  kimlik?: string;
  seri?: string;
  ad: string;
  veren?: string;
  son?: string;
}

interface HizmetYaniti {
  tamam: boolean;
  hata?: string;
  tur?: string;
  udf?: string;
  bilgi?: ImzaBilgisi;
  kart?: { takili: boolean; neden?: string; sertifikalar?: Sertifika[] };
}

export interface ImzaIstegi {
  dosyaAdi: string;
  /** İmzalanacak UDF (kaydedilecek son baytlar) */
  belge: () => Uint8Array;
  /** İmza başarılı: imzalı UDF */
  bitti: (imzali: Uint8Array, bilgi: ImzaBilgisi) => void;
}

function el<T extends HTMLElement = HTMLElement>(etiket: string, ozellik: Record<string, unknown> = {}, ...cocuk: (Node | string)[]): T {
  const e = Object.assign(document.createElement(etiket), ozellik) as T;
  e.append(...cocuk);
  return e;
}

export async function imzaPenceresiAc(istek: ImzaIstegi) {
  const ort = el("div", { className: "pencere-ort" });
  const durum = el("p", { className: "pencere-durum", textContent: "Kart okunuyor…" });
  const sertifikaSec = el<HTMLSelectElement>("select", { className: "secici" });
  const pin = el<HTMLInputElement>("input", { type: "password", autocomplete: "off", placeholder: "PIN", className: "secici" });
  pin.setAttribute("inputmode", "numeric");
  pin.spellcheck = false;
  const imzala = el<HTMLButtonElement>("button", { textContent: "İmzala", className: "birincil", disabled: true });
  const vazgec = el<HTMLButtonElement>("button", { textContent: "Vazgeç" });
  const kutu = el("div", { className: "pencere", role: "dialog" },
    el("h2", { textContent: "E-imza" }),
    el("p", { className: "pencere-aciklama", textContent: `"${istek.dosyaAdi}" bilgisayara takılı e-imza kartınızla imzalanacak. İmzalanan belge değiştirilemez; değişiklik imzayı geçersiz kılar.` }),
    el("label", { textContent: "Sertifika" }), sertifikaSec,
    el("label", { textContent: "PIN" }), pin,
    durum,
    el("div", { className: "pencere-dugmeler" }, vazgec, imzala),
  );
  ort.append(kutu);
  document.body.append(ort);
  const kapat = () => {
    pin.value = "";
    ort.remove();
  };
  vazgec.onclick = kapat;
  ort.addEventListener("keydown", (e) => { if (e.key === "Escape") kapat(); });

  try {
    const d = await cagir<HizmetYaniti>("imza_durum");
    const s = d.kart?.sertifikalar ?? [];
    if (!d.tamam || !d.kart?.takili || !s.length) {
      durum.textContent = d.kart?.neden || d.hata || "Kart takılı değil.";
      durum.className = "pencere-durum hata";
      return;
    }
    for (const c of s) sertifikaSec.append(new Option(`${c.ad}${c.son ? ` (son: ${c.son})` : ""}`, c.kimlik ?? c.seri ?? ""));
    durum.textContent = "";
    imzala.disabled = false;
    pin.focus();
  } catch (e) {
    durum.textContent = String(e);
    durum.className = "pencere-durum hata";
    return;
  }

  const gonder = async () => {
    if (!pin.value) {
      durum.textContent = "PIN girin.";
      return;
    }
    imzala.disabled = vazgec.disabled = true;
    durum.className = "pencere-durum";
    durum.textContent = "İmzalanıyor… (kart okuyucunun ışığı yanıp sönebilir)";
    const p = pin.value;
    pin.value = "";
    try {
      const y = await cagir<HizmetYaniti>("imzala", {
        dosyaAdi: istek.dosyaAdi, udf: b64Kodla(istek.belge()), pin: p, sertifika: sertifikaSec.value || null,
      });
      if (y.tamam && y.udf && y.bilgi) {
        kapat();
        istek.bitti(b64Coz(y.udf), y.bilgi);
        return;
      }
      durum.className = "pencere-durum hata";
      durum.textContent = y.tur === "pin_hatali" ? `${y.hata} ` : y.hata || "İmzalanamadı.";
    } catch (e) {
      durum.className = "pencere-durum hata";
      durum.textContent = String(e);
    }
    imzala.disabled = vazgec.disabled = false;
    pin.focus();
  };
  imzala.onclick = gonder;
  pin.addEventListener("keydown", (e) => { if (e.key === "Enter") gonder(); });
}
