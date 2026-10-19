#!/bin/sh
# Build the browser site into web/dist: static files plus melee_web.wasm.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cargo=${CARGO:-$HOME/.cargo/bin/cargo}
"$cargo" build --release --locked -p melee-web --target wasm32-unknown-unknown \
  --manifest-path "$root/Cargo.toml"
target=$("$cargo" metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" |
  python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
rm -rf "$root/web/dist"
mkdir -p "$root/web/dist"
# Publish only the game surface; research tools remain in the source checkout.
for file in index.html lobby.js scene.js protocol.js game.css _headers; do
  cp "$root/web/$file" "$root/web/dist/"
done
cp "$target/wasm32-unknown-unknown/release/melee_web.wasm" "$root/web/dist/"
python3 "$root/tools/content-pack.py"
ls -l "$root/web/dist"
