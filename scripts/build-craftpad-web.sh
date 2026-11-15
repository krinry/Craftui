#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:-dist/craftpad-web}"
version="$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock)"
if ! command -v wasm-bindgen >/dev/null; then
  echo "Install wasm-bindgen-cli $version and the wasm32-unknown-unknown target first." >&2
  exit 1
fi
if [[ "$(wasm-bindgen --version)" != "wasm-bindgen $version" ]]; then
  echo "wasm-bindgen-cli must match Cargo.lock version $version." >&2
  exit 1
fi
cargo build --locked --release -p craftui-example-craftpad --lib --no-default-features --target wasm32-unknown-unknown
mkdir -p "$out"
wasm-bindgen target/wasm32-unknown-unknown/release/craftui_example_craftpad.wasm \
  --target web --out-name craftui_app --out-dir "$out"
cp examples/craftpad/web/index.html "$out/index.html"
printf 'Craftpad bundle written to %s\nServe it over HTTP (for example: python3 -m http.server --directory "%s" 8080).\n' "$out" "$out"
