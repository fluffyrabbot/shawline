#!/usr/bin/env bash
# Builds the playground into playground/dist: the wasm package (web and
# Node targets), Shawline Mono, the corpus examples and the static pages.
# Serve it with `python3 -m http.server -d playground/dist`.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
dist="$here/dist"
target="$root/target/wasm32-unknown-unknown/release"

rm -rf "$dist"
mkdir -p "$dist/examples"

# Size over speed: the playground's programs are tiny, the download isn't.
CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
CARGO_PROFILE_RELEASE_LTO=true \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
  cargo build --manifest-path "$root/Cargo.toml" -p shawline-wasm \
    --target wasm32-unknown-unknown --release

wasm-bindgen --target web --no-typescript --out-dir "$dist/pkg" "$target/shawline_wasm.wasm"
wasm-bindgen --target nodejs --no-typescript --out-dir "$dist/node" "$target/shawline_wasm.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz "$dist/pkg/shawline_wasm_bg.wasm" -o "$dist/pkg/shawline_wasm_bg.wasm"
fi

cargo run --quiet --manifest-path "$root/Cargo.toml" -p shawline-font --example build -- "$dist/ShawlineMono.ttf"

# Every valid corpus program (01–79, corpus/README.md), each ASCII twin
# listed after its canonical program, plus a manifest for the picker.
names=()
for f in "$root"/corpus/[0-7][0-9]-*.shaw; do
  case "$f" in *.fmt.shaw | *-ascii.shaw) continue ;; esac
  for g in "$f" "${f%.shaw}-ascii.shaw"; do
    [ -e "$g" ] || continue
    cp "$g" "$dist/examples/"
    names+=("\"$(basename "$g" .shaw)\"")
  done
done
(IFS=,; echo "[${names[*]}]") > "$dist/examples/index.json"

cp "$here"/index.html "$here"/style.css "$here"/app.js "$here"/tour.js "$here"/worker.js "$dist/"

echo "built $dist ($(wc -c < "$dist/pkg/shawline_wasm_bg.wasm" | tr -d ' ') byte wasm)"
