#!/usr/bin/env bash
# Build the clapp's macOS depot (Apple silicon), outside the repository:
#
#   ~/.cache/<cli>-clapp/pkg/clatch.json
#   ~/.cache/<cli>-clapp/pkg/assets/icon.png
#   ~/.cache/<cli>-clapp/pkg/bin/<cli>            the launcher and the agent's CLI
#   ~/.cache/<cli>-clapp/pkg/<Name>.app           the window, bundled as upstream bundles it
#   ~/.cache/<cli>-clapp/pkg/licenses/            the app's licences and its fonts'
#
# The window embeds craft-fonts, as every official release does: a checkout beside this
# repository (../craft-fonts), or CRAFT_FONTS_DIR. Prints exactly one line on stdout, the depot,
# so `clatch validate "$(clapp/scripts/depot.sh)"` works. The same script in every Craft clapp.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

step() { printf '\n> %s\n' "$1" >&2; }
fail() { printf '\nFAIL - %s\n' "$1" >&2; exit 1; }

manifest() {
  node -e 'const m = JSON.parse(require("fs").readFileSync(process.argv[2], "utf8"));
           process.stdout.write(String(process.argv[1].split(".").reduce((v, k) => v[k], m)));' "$1" "$ROOT/clatch.json"
}
CLI="$(manifest connector.cli)"
ID="$(manifest id)"
PLIST_IN="$ROOT/packaging/macos/Info.plist.in"
EXE="$(plutil -extract CFBundleExecutable raw -o - "$PLIST_IN")"
NAME="$(plutil -extract CFBundleName raw -o - "$PLIST_IN")"
ICON="$(plutil -extract CFBundleIconFile raw -o - "$PLIST_IN")"
VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p;}' "$ROOT/Cargo.toml" | head -1)"
[ -n "$VERSION" ] || fail "no workspace version in Cargo.toml"

CACHE="$HOME/.cache/$CLI-clapp"
DEPOT="$CACHE/pkg"
FONTS="${CRAFT_FONTS_DIR:-$(cd "$ROOT/.." && pwd)/craft-fonts}"
[ -f "$FONTS/fonts/manifest.txt" ] \
  || fail "craft-fonts is not at $FONTS: git clone --depth 1 https://github.com/storytold/craft-fonts \"$FONTS\""

step "build the window: $NAME $VERSION, with craft-fonts"
( cd "$ROOT" && CRAFT_FONTS_DIR="$FONTS" CARGO_TARGET_DIR="$CACHE/target" MACOSX_DEPLOYMENT_TARGET=11.0 \
    cargo build --release -p "$CLI" ) >&2 || fail "cargo build -p $CLI failed"
WINDOW="$CACHE/target/release/$CLI"

step "build the launcher"
( cd "$ROOT/clapp" && CARGO_TARGET_DIR="$CACHE/clapp-target" cargo build --release ) >&2 || fail "the launcher did not build"
LAUNCHER="$CACHE/clapp-target/release/$CLI"

step "assemble $DEPOT"
rm -rf "$DEPOT"
APP="$DEPOT/$NAME.app"
mkdir -p "$DEPOT/bin" "$DEPOT/assets" "$DEPOT/licenses" "$APP/Contents/MacOS" "$APP/Contents/Resources/Licenses"
cp "$WINDOW" "$APP/Contents/MacOS/$EXE"
cp "$ROOT/assets/app-icon/$CLI.icns" "$APP/Contents/Resources/$ICON.icns"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/${VERSION%%-*}/g" \
    -e "s/@BUILD_SHA@/$(git -C "$ROOT" rev-parse --short HEAD)/g" "$PLIST_IN" >"$APP/Contents/Info.plist"
# The clapp's own identity, not upstream's: an installed official copy keeps its own
# preferences and privacy grants, and this one gets its own.
plutil -replace CFBundleIdentifier -string "$ID" "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >&2
for lic in "$FONTS"/fonts/*/OFL.txt; do
  cp "$lic" "$APP/Contents/Resources/Licenses/OFL-$(basename "$(dirname "$lic")").txt"
done
for f in LICENSE-APACHE LICENSE-MIT NOTICE ATTRIBUTION.md; do
  [ -f "$ROOT/$f" ] && cp "$ROOT/$f" "$DEPOT/licenses/$f"
done
cp "$APP/Contents/Resources/Licenses"/OFL-*.txt "$DEPOT/licenses/"
cp "$LAUNCHER" "$DEPOT/bin/$CLI"
cp "$ROOT/clatch.json" "$DEPOT/clatch.json"
cp "$ROOT/assets/icon.png" "$DEPOT/assets/icon.png"

step "sign (ad-hoc) with upstream's entitlements"
codesign --force --sign - --identifier "$ID" --options runtime \
  --entitlements "$ROOT/packaging/macos/entitlements.plist" "$APP/Contents/MacOS/$EXE" >&2
codesign --force --sign - --identifier "$ID" --options runtime \
  --entitlements "$ROOT/packaging/macos/entitlements.plist" "$APP" >&2
codesign --force --sign - --identifier "$ID.cli" "$DEPOT/bin/$CLI" >&2
codesign --verify --strict --deep "$APP" >&2 || fail "the bundle does not verify"

step "check"
# `clatch pack` leaves symlinks out of the archive, so a depot with one is not what installs.
links="$(find "$DEPOT" -type l)"
[ -z "$links" ] || fail "symlinks in the depot: $links"
"$DEPOT/bin/$CLI" help >/dev/null || fail "the packaged CLI does not run"
printf '  %s, %s\n' "$(du -sh "$DEPOT" | cut -f1)" "$(du -sh "$APP" | cut -f1) of it the window" >&2

printf '%s\n' "$DEPOT"
