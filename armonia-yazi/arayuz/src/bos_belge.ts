// Yeni boş belge (UYAP editörünün yeni belgesiyle aynı ayarlar). Claude, 2026-11-23.

import type { Belge } from "./turler";

/** Boş belge: A4 dikey, kenar boşlukları ve stiller UYAP editörünün yeni belgesindeki gibi (gerçek UDF'ten:
 * sol/sağ/üst 2,5 cm, alt 1,5 cm; gövde Times New Roman 12). UDF olarak kaydedilir. */
export function bosBelgeOlustur(): Belge {
  const oz = (v: [string, string][]) => v;
  return {
    kok: oz([["format_id", "1.8"]]),
    sayfa: oz([["mediaSizeName", "1"], ["leftMargin", "70.8661413192749"], ["rightMargin", "70.8661413192749"],
      ["topMargin", "70.8661413192749"], ["bottomMargin", "42.51968479156494"], ["paperOrientation", "1"],
      ["headerFOffset", "20.0"], ["footerFOffset", "20.0"]]),
    govde_oz: oz([["resolver", "hvl-default"]]),
    ust_bilgi: null,
    alt_bilgi: null,
    govde: [{ Paragraf: { oz: oz([["Alignment", "0"]]), parcalar: [{ Metin: { metin: "\n", oz: oz([["family", "Times New Roman"], ["size", "12"]]) } }] } }],
    stiller: [
      { ad: "default", oz: oz([["name", "default"], ["description", "Geçerli"], ["family", "Dialog"], ["size", "12"], ["bold", "false"],
        ["italic", "false"], ["foreground", "-13421773"],
        ["FONT_ATTRIBUTE_KEY", "javax.swing.plaf.FontUIResource[family=Dialog,name=Dialog,style=plain,size=12]"]]) },
      { ad: "hvl-default", oz: oz([["name", "hvl-default"], ["family", "Times New Roman"], ["size", "12"], ["description", "Gövde"]]) },
    ],
    ek_dosyalar: [],
    ek_ogeler: [],
  };
}

