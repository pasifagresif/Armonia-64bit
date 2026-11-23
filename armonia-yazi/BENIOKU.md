# Armonia Yazı

Armonia'nın belge editörü: UDF (UYAP) ve Word (DOCX) açar, düzenler, kaydeder; PDF'i birebir gösterir ve UDF/DOCX'e
çevirir. Aynı kod Armonia'da (Tauri, ağsız kum havuzu) ve cokluuyap.com'da (WebAssembly) çalışacak.

Plan, kararlar ve ilerleme: `docs/claude-notlar/planlar/armonia-belge-editoru.md`.

## Yapı
- `belge_cekirdek/` — Rust kütüphanesi: belge modeli (`model.rs`), UDF okuma/yazma (`udf.rs`). Dosya sistemi ve
  ağ kullanmaz (yalnız bayt alır/verir), böylece WebAssembly'de de çalışır.

## Test
```sh
cd armonia-yazi && cargo test
```
Gerçek belge örnekleri kişisel veri içerir: depoya konmaz.
