#!/bin/sh
# Builds Orbi.app (with orbi-hook bundled inside) and wraps it in Orbi.dmg:
# the app plus an Applications shortcut to drag it onto.
#   sh scripts/package.sh        → dist-mac/{Orbi.dmg, Orbi.dmg.sha256, install.sh}
#                                   (also copied to site/public/)
#   ORBI_SITE_URL=https://… sh scripts/package.sh   # for a hosted release
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

# Compiled-in source paths (panic messages) would otherwise carry the builder's
# home folder, and with it their username. Rewrite it for the app and the hook.
export RUSTFLAGS="--remap-path-prefix=$HOME=~ ${RUSTFLAGS:-}"

pnpm tauri build --bundles app

# Nothing from the builder's home may ship.
APP_BIN="$ROOT/src-tauri/target/release/bundle/macos/Orbi.app/Contents/MacOS"
if strings "$APP_BIN/orbi" "$APP_BIN/orbi-hook" | grep -q "$HOME"; then
  echo "the build still contains $HOME — refusing to package" >&2
  exit 1
fi

APP="$ROOT/src-tauri/target/release/bundle/macos/Orbi.app"
[ -x "$APP/Contents/MacOS/orbi-hook" ] || { echo "orbi-hook missing from the bundle" >&2; exit 1; }

# Ad-hoc sign the whole bundle (inner binaries first via --deep) so it runs on
# Apple Silicon. Distribution outside this Mac still needs a Developer ID +
# notarization.
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

OUT="$ROOT/dist-mac"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$OUT"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$OUT/Orbi.dmg" "$STAGE.rw.dmg"
# Build read-write first so the mounted disk can get Orbi's icon, then compress.
hdiutil create -quiet -volname "Orbi" -srcfolder "$STAGE" -fs HFS+ -format UDRW -ov "$STAGE.rw.dmg"
MNT=$(mktemp -d)
hdiutil attach -quiet -nobrowse -mountpoint "$MNT" "$STAGE.rw.dmg"
cp "$ROOT/src-tauri/icons/icon.icns" "$MNT/.VolumeIcon.icns"
SetFile -c icnC "$MNT/.VolumeIcon.icns" 2>/dev/null || true
SetFile -a C "$MNT" 2>/dev/null || true
hdiutil detach -quiet "$MNT"
hdiutil convert -quiet "$STAGE.rw.dmg" -format UDZO -o "$OUT/Orbi.dmg"
rm -f "$STAGE.rw.dmg"

SHA=$(shasum -a 256 "$OUT/Orbi.dmg" | cut -d' ' -f1)
printf '%s  Orbi.dmg\n' "$SHA" > "$OUT/Orbi.dmg.sha256"
VERSION=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$ROOT/src-tauri/tauri.conf.json" | head -1)

# The one-line installer, with this release's checksum baked in. Set
# ORBI_SITE_URL to where the site is hosted (default: the local dev server).
SITE_URL="${ORBI_SITE_URL:-http://localhost:4300}"
sed -e "s|@BASE_URL@|$SITE_URL|" -e "s|@SHA256@|$SHA|" -e "s|@VERSION@|$VERSION|" \
  "$ROOT/scripts/install.sh" > "$OUT/install.sh"

mkdir -p "$ROOT/site/public"
cp "$OUT/Orbi.dmg" "$OUT/Orbi.dmg.sha256" "$OUT/install.sh" "$ROOT/site/public/"
ls -lh "$OUT"
echo "sha256 $SHA"
