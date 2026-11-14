#!/bin/sh
# 번들에 넣을 ieum(workflow 전용)을 빌드해 src-tauri/binaries/ieum-<triple>에 둔다.
set -eu
cd "$(dirname "$0")/.."
triple="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
cargo build --release -p ieum-cli --bin ieum --target "$triple"
mkdir -p src-tauri/binaries
cp "target/$triple/release/ieum" "src-tauri/binaries/ieum-$triple"
