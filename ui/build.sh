#!/usr/bin/env sh
# Builds the web app into ./dist, which `tracer serve` serves (--ui-dir ui/dist).
# Needs: rustup target add wasm32-unknown-unknown, and a wasm-bindgen CLI matching ui/Cargo.lock
#   cargo install wasm-bindgen-cli --version "$(grep -A1 'name = "wasm-bindgen"' Cargo.lock | sed -n 's/version = "\(.*\)"/\1/p')" --locked
set -eu
cd "$(dirname "$0")"
PROFILE=${PROFILE:-release}
if [ "$PROFILE" = release ]; then cargo build --target wasm32-unknown-unknown --release; else cargo build --target wasm32-unknown-unknown; fi
rm -rf dist && mkdir -p dist/fonts
wasm-bindgen --target web --no-typescript --out-dir dist --out-name tracer-ui \
  target/wasm32-unknown-unknown/$PROFILE/tracer-ui.wasm
command -v wasm-opt >/dev/null && wasm-opt -Oz -o dist/tracer-ui_bg.wasm dist/tracer-ui_bg.wasm || true
# the fonts ship inside the dots-ui crate; find wherever cargo unpacked it
DOTS=$(cargo metadata --format-version 1 | grep -o '"manifest_path":"[^"]*/dots-ui-[0-9][^"]*/Cargo.toml"' | head -1 | sed 's/.*:"\(.*\)\/Cargo.toml"/\1/')
cp "$DOTS"/assets/fonts/*.woff2 dist/fonts/
# cache-bust by content
V=$(cat dist/tracer-ui_bg.wasm | cksum | cut -d' ' -f1)
sed "s/__V__/$V/g" index.html > dist/index.html
sed "s/__V__/$V/g" boot.js > dist/boot.js   # the start-up script is a file of its own so the page can forbid inline scripts
ls -lh dist | sed 's/^/  /'
