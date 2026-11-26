#!/bin/bash
# Armonia Yazı'yı ve imza hizmetini imaj için derler; çıktı: ARMONIA_YAZI_CIKTI (varsayılan ~/armonia-cikti/yazi):
#   armonia-yazi   (Tauri masaüstü uygulaması; arayüz içine gömülü)
#   armonia-imza   (imza hizmeti)
#   SURUM
# NORMAL kullanıcıyla çalıştırın (root değil). armonia-linux/yazi_hazirla.sh çıktı yoksa bunu çağırır.
# Gerekenler (Debian): build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev nodejs npm;
# rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version 0.2.130 --locked.
# Claude, 2026-11-24.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
[ "$(id -u)" != 0 ] || { echo "HATA: derle.sh root ile çalıştırılmaz"; exit 1; }
CIKTI="${ARMONIA_YAZI_CIKTI:-$HOME/armonia-cikti/yazi}"
export CARGO_TARGET_DIR="$PWD/target"
for k in cargo npm wasm-bindgen pkg-config; do
  command -v "$k" >/dev/null || { echo "HATA: $k yok (derle.sh başındaki gereksinimlere bakın)"; exit 1; }
done
pkg-config --exists webkit2gtk-4.1 || { echo "HATA: libwebkit2gtk-4.1-dev yok"; exit 1; }
v=$(wasm-bindgen --version | awk '{print $2}')
[ "$v" = "0.2.130" ] || { echo "HATA: wasm-bindgen $v; gereken 0.2.130"; exit 1; }

echo "1/3 belge çekirdeği → WebAssembly"
sh ./wasm_derle.sh >/dev/null
echo "2/3 arayüz"
(cd arayuz && npm ci --no-audit --no-fund && npm run build)
echo "3/3 masaüstü uygulaması ve imza hizmeti"
cargo build --release --locked -p armonia-yazi -p imza --bin armonia-yazi --bin armonia-imza --features armonia-yazi/custom-protocol

install -d "$CIKTI"
install -m 0755 target/release/armonia-yazi target/release/armonia-imza "$CIKTI/"
{
  echo "surum=$(sed -n 's/^version = "\(.*\)"/\1/p' masaustu/Cargo.toml | head -n1)"
  echo "tarih=$(date -u +%Y-%m-%dT%H:%MZ)"
  echo "mimari=$(uname -m)"
  echo "git=$(git rev-parse --short HEAD 2>/dev/null || echo yok)"
} > "$CIKTI/SURUM"
echo "Hazır: $CIKTI"; cat "$CIKTI/SURUM"
