#!/bin/sh
# Orbi installer — downloads Orbi.dmg, checks it against the SHA-256 below,
# and copies Orbi.app into Applications. Read it before you run it; it is
# short on purpose.
#
#   curl -fsSL <site>/install.sh | sh
#
# Why this exists: Orbi is free and open source and not notarized by Apple.
# A browser download gets tagged "from the internet" and macOS then blocks the
# first launch until you click Open Anyway. `curl` doesn't add that tag, so an
# app installed this way opens straight away. Because nothing else vouches for
# the download, this script refuses to install unless the checksum matches.
#
# scripts/package.sh fills in the three values below when it builds a release.
set -eu

BASE_URL="${ORBI_BASE_URL:-@BASE_URL@}"
EXPECTED_SHA256="${ORBI_SHA256:-@SHA256@}"
VERSION="@VERSION@"
DEST_DIR="${ORBI_INSTALL_DIR:-/Applications}"

# The Orbi face, in the terminal: a dark pill with two eyes.
say_logo() {
  printf '\n  \033[48;2;11;11;12;38;2;236;235;228m  ●  ●  \033[0m  \033[1mOrbi\033[0m \033[2m· a face for your AI agents\033[0m\n\n'
}
say() { printf '\033[1morbi:\033[0m %s\n' "$*"; }
die() { printf '\033[1;31morbi:\033[0m %s\n' "$*" >&2; exit 1; }

case "$BASE_URL$EXPECTED_SHA256" in *@*) die "this is the template; use the install.sh published with a release" ;; esac
[ "$(uname -s)" = Darwin ] || die "Orbi is a macOS app"
[ "$(uname -m)" = arm64 ] || die "this build is for Apple Silicon Macs"
major=$(sw_vers -productVersion | cut -d. -f1)
[ "$major" -ge 13 ] || die "Orbi needs macOS 13 or newer"

say_logo
TMP=$(mktemp -d)
MNT=""
cleanup() {
  [ -n "$MNT" ] && hdiutil detach -quiet "$MNT" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

say "downloading Orbi $VERSION"
curl -fL --proto '=https,http' --progress-bar -o "$TMP/Orbi.dmg" "$BASE_URL/Orbi.dmg"

actual=$(shasum -a 256 "$TMP/Orbi.dmg" | cut -d' ' -f1)
[ "$actual" = "$EXPECTED_SHA256" ] || die "checksum mismatch — not installing (expected $EXPECTED_SHA256, got $actual)"
say "checksum ok"

MNT="$TMP/mnt"
mkdir -p "$MNT"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MNT" "$TMP/Orbi.dmg"
[ -d "$MNT/Orbi.app" ] || die "Orbi.app not found in the download"

# Quit a running Orbi so the new copy replaces it cleanly.
osascript -e 'tell application id "dev.orbi.app" to quit' >/dev/null 2>&1 || true

if [ ! -w "$DEST_DIR" ]; then
  DEST_DIR="$HOME/Applications"
  mkdir -p "$DEST_DIR"
fi
say "installing to $DEST_DIR/Orbi.app"
rm -rf "$DEST_DIR/Orbi.app"
ditto "$MNT/Orbi.app" "$DEST_DIR/Orbi.app"

if [ "${ORBI_NO_OPEN:-}" != 1 ]; then
  open "$DEST_DIR/Orbi.app"
  say "done — Orbi is at the top of your screen. Connect your agents in the Settings window."
else
  say "done"
fi
