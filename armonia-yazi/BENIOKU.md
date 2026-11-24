# Armonia Yazı

Armonia'nın belge editörü: UDF (UYAP) ve Word (DOCX) açar, düzenler, kaydeder, e-imzalar. Aynı arayüz Armonia'da
masaüstü uygulaması (Tauri) ve cokluuyap.com'da web sayfası olarak çalışır.

Plan, kararlar ve ilerleme: `docs/claude-notlar/planlar/armonia-belge-editoru.md`.

## Yapı
- `belge_cekirdek/` — Rust: belge modeli, UDF okuma/yazma, DOCX okuma/yazma, stil çözücü, görünüm. Dosya
  sistemi ve ağ kullanmaz (bayt alır/verir).
- `belge_wasm/` — çekirdeğin tarayıcı köprüsü (WebAssembly).
- `arayuz/` — editör (TypeScript + ProseMirror, Vite).
- `masaustu/` — Tauri 2 masaüstü kabuğu: dosya pencereleri, çift tıkla açma, imza hizmeti köprüsü, imza doğrulama.
- `imza/` — e-imza: UDF imzalama/doğrulama (CAdES, UYAP sign.sgn), Adalet E-imza istemcisi (5975) ve PKCS#11 arka
  uçları, zincirli imza günlüğü, `armonia-imza` hizmeti (Unix soketi).
- Armonia imaj katmanı: `armonia-linux/yazi-katmani/` (birimler, kanca), `armonia-linux/yazi_hazirla.sh`; derleme: `derle.sh`.

## Derleme (Linux)
```sh
# Bir kez: Tauri bağımlılıkları (Debian/Armonia derleme makinesi)
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked

./wasm_derle.sh                                  # çekirdek → arayuz/src/wasm
(cd arayuz && npm ci && npm run build)           # arayüz → arayuz/dist
cargo build --release -p armonia-yazi -p imza    # target/release/armonia-yazi, armonia-imza
```
Hepsi tek komutla (imaj için): `bash derle.sh` → `~/armonia-cikti/yazi/` (armonia-linux/yazi_hazirla.sh buradan alır).
Çalışma zamanında Armonia'ya `libwebkit2gtk-4.1-0` gerekir.

## Test
```sh
cargo test                                         # çekirdek + imza
(cd arayuz && npm run test:donusum -- <belge.udf> [<belge.docx>])
ARMONIA_YAZI_ORNEKLER=<klasör> cargo test -- --ignored gercek   # gerçek belgelerle
```
Geliştirme: `cd arayuz && npm run dev` (http://127.0.0.1:5180); masaüstü: `cargo run -p armonia-yazi` (geliştirme
sunucusunu kullanır). Gerçek belge örnekleri kişisel veri içerir: **depoya konmaz**.
