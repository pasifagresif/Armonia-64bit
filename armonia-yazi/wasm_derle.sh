#!/bin/sh
# Belge çekirdeğini WebAssembly'ye derler ve arayüze bağlar (arayuz/src/wasm). Gerekenler:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.130 --locked   (belge_wasm/Cargo.toml'daki sürümle aynı)
# Claude, 2026-11-23.
set -eu
cd "$(dirname "$0")"
cargo build -p belge_wasm --target wasm32-unknown-unknown --release
HEDEF="${CARGO_TARGET_DIR:-target}"
wasm-bindgen --target web --out-dir arayuz/src/wasm "$HEDEF/wasm32-unknown-unknown/release/belge_wasm.wasm"
ls -la arayuz/src/wasm
