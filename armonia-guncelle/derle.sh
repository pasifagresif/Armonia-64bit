#!/bin/bash
# armonia-guncelle'yi imaj için derler; çıktı: ARMONIA_GUNCELLE_CIKTI (varsayılan ~/armonia-cikti/guncelle).
# NORMAL kullanıcıyla çalıştırın. İkili derlendiği makinenin mimarisinde çıkar (Pi'de aarch64).
# Claude, 2026-11-24.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
[ "$(id -u)" != 0 ] || { echo "HATA: derle.sh root ile çalıştırılmaz"; exit 1; }
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || { echo "HATA: cargo yok (rustup kurun)"; exit 1; }
CIKTI="${ARMONIA_GUNCELLE_CIKTI:-$HOME/armonia-cikti/guncelle}"
export CARGO_TARGET_DIR="$PWD/target"
cargo test --release --locked -q
cargo build --release --locked --bin armonia-guncelle
install -d "$CIKTI"
install -m 0755 target/release/armonia-guncelle "$CIKTI/"
{
  echo "surum=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
  echo "tarih=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "mimari=$(uname -m)"
  echo "git=$(git rev-parse --short HEAD 2>/dev/null || echo yok)"
} > "$CIKTI/SURUM"
echo "Hazır: $CIKTI"; cat "$CIKTI/SURUM"
