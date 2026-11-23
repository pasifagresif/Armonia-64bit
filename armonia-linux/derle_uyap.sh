#!/bin/bash
# Armonia imajı için ÇokluUYAP'ı (uyap_app) Linux'ta derler ve imaj dizinini hazırlar.
# Normal kullanıcıyla çalışır (WSL Debian'da 'utku', Raspberry Pi'de kendi kullanıcın):
#   bash <depo>/armonia-linux/derle_uyap.sh
# Çıktı: $ARMONIA_CIKTI/cokluuyap/  (varsayılan ~/armonia-cikti; uyap_app + uyap_core/enjekte + webapp + SURUM)
# Yollar betiğin kendi yerinden bulunur; ortam değişkeniyle değiştirilebilir:
#   REPO=<depo kökü>  ARMONIA_CIKTI=<çıktı kökü>  RUYAP_KOPYA=<derleme kopyası, varsayılan ~/ruyap>
# DİKKAT: ikili, derlendiği makinenin mimarisinde çıkar: x86_64 makinede ISO için, Pi'de (aarch64) Pi imajı için.
# iso_derle.sh / pi_derle.sh (COKLUUYAP=1) çıktıyı cokluuyap_hazirla.sh ile alır; yoksa bu betiği kendileri çağırır.
# Claude, 2026-11-13 (19.11: yollar taşınabilir yapıldı).
set -euo pipefail

BETIK_DIZINI="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
REPO="${REPO:-$(cd "$BETIK_DIZINI/.." && pwd)}"
ANA="$REPO/Uyap Haricen Giriş"
SRC="$ANA/ruyap"   # ruyap kaynağı; arayüzün asıl kopyası da burada (ruyap/uyap_core/enjekte, ruyap/webapp).
                   # Üst dizindeki uyap_core/enjekte ve webapp eski Python kopyalarıdır (14.11'den beri güncellenmiyor).
CIKTI_KOK="${ARMONIA_CIKTI:-$HOME/armonia-cikti}"
DST="${RUYAP_KOPYA:-$HOME/ruyap}"
OUT="$CIKTI_KOK/cokluuyap"
LOG="$CIKTI_KOK/derle.log"

[ -d "$SRC" ] || { echo "HATA: ruyap kaynağı yok: $SRC (REPO=... ile depo kökünü verin)"; exit 1; }
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || { echo "HATA: cargo bulunamadı (rustup kurun)"; exit 1; }
MIMARI="$(uname -m)"
echo "Mimari: $MIMARI (x86_64 -> ISO, aarch64 -> Pi imajı; cokluuyap_hazirla.sh ELF başlığından denetler)"
mkdir -p "$DST" "$CIKTI_KOK"

# 1) Kaynağı birebir eşitle (target/ korunur → artımlı derleme). Çalışma önbelleği ve .exe kopyalanmaz.
find "$DST" -mindepth 1 -maxdepth 1 ! -name target -exec rm -rf {} +
tar -C "$SRC" --exclude=./target --exclude=./uyap_core --exclude=./webapp --exclude='./*.exe' \
    -cf - . | tar -C "$DST" -xf -
KAYNAK_OZET=$(cd "$DST" && find . -path ./target -prune -o -type f -print0 | sort -z \
    | xargs -0 sha256sum | sha256sum | cut -c1-16)
GIT_HEAD=$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo bilinmiyor)
GIT_KIRLI=$(git -C "$REPO" status --porcelain 2>/dev/null | wc -l)

# 2) Yalnız imaja girecek program: uyap_app (office_agent ve home_client içine gömülü).
#    İnce LTO + sembol temizliği burada; paylaşılan Cargo.toml'a dokunulmaz.
export CARGO_PROFILE_RELEASE_LTO=thin
export CARGO_PROFILE_RELEASE_STRIP=symbols
export CARGO_PROFILE_RELEASE_DEBUG=0
BAS=$(date +%s)
cd "$DST"
set +e
cargo build --release --locked --bin uyap_app -j 2 >"$LOG" 2>&1
RC=$?
set -e
grep -E '^error|Finished' "$LOG" | tail -5
echo "cargo cikis: $RC  sure: $(( $(date +%s) - BAS )) sn"
[ $RC -eq 0 ] || exit $RC

# 3) Bilinen açık taraması (RustSec). Derlemeyi durdurmaz, raporu kaydeder.
if cargo audit --version >/dev/null 2>&1; then
  cargo audit >"$CIKTI_KOK/audit.txt" 2>&1 && echo "cargo audit: temiz" \
    || echo "cargo audit: BULGU VAR -> $CIKTI_KOK/audit.txt"
else
  echo "cargo audit kurulu degil (cargo install cargo-audit --locked)"
fi

# 4) İmaj dizini: ikili + güncel arayüz dosyaları + sürüm kaydı
rm -rf "$OUT"; mkdir -p "$OUT/uyap_core"
install -m 0755 target/release/uyap_app "$OUT/uyap_app"
cp -r "$SRC/uyap_core/enjekte" "$OUT/uyap_core/enjekte"
cp -r "$SRC/webapp" "$OUT/webapp"
cat >"$OUT/SURUM" <<EOT
surum=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
git=$GIT_HEAD (degismis dosya: $GIT_KIRLI)
kaynak_ozet=$KAYNAK_OZET
derleme=$(date -u +%Y-%m-%dT%H:%M:%SZ)
rustc=$(rustc --version)
EOT
cat "$OUT/SURUM"
ls -la "$OUT/uyap_app"; du -sh "$OUT"
