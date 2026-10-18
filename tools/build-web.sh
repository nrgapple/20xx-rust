#!/bin/sh
# Build the browser inspector into web/dist: static files plus melee_web.wasm.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cargo=${CARGO:-$HOME/.cargo/bin/cargo}
"$cargo" build --release --locked -p melee-web --target wasm32-unknown-unknown \
  --manifest-path "$root/Cargo.toml"
target=$("$cargo" metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" |
  python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
rm -rf "$root/web/dist"
mkdir -p "$root/web/dist"
cp "$root/web/index.html" "$root/web/app.js" "$root/web/style.css" "$root/web/_headers" "$root/web/dist/"
cp "$target/wasm32-unknown-unknown/release/melee_web.wasm" "$root/web/dist/"
ls -l "$root/web/dist"
