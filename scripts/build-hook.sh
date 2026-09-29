#!/bin/sh
# Builds orbi-hook and places it where Tauri bundles sidecars
# (src-tauri/binaries/orbi-hook-<target-triple>). Run from the repo root.
# Always a release build: the hook runs on every agent tool call.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
(cd "$ROOT/hook" && cargo build --release --quiet)
mkdir -p "$ROOT/src-tauri/binaries"
cp "$ROOT/hook/target/release/orbi-hook" "$ROOT/src-tauri/binaries/orbi-hook-$TRIPLE"
chmod 0755 "$ROOT/src-tauri/binaries/orbi-hook-$TRIPLE"
