#!/usr/bin/env sh
# Assemble the Clatch DEPOT — the folder `clatch validate`, `clatch install` and
# `clatch pack` consume:
#
#   <depot>/clatch.json              the manifest (verbatim; cliBin gains .exe on Windows)
#   <depot>/bin/<cli>[.exe]          the ONE binary — the GUI *and* the agent CLI
#   <depot>/assets/icon.png          the icon clatch.json declares
#   <depot>/THIRD_PARTY_NOTICES.md   when the repo has one
#   …plus whatever app_extras() adds
#
# Prints exactly ONE line on stdout — the depot path — and sends every other word to
# stderr, so this is a legal thing to write:
#
#   clatch validate "$(npm run -s package)"
#
# The script is identical in shape in every clapp: one small per-app header block,
# then a body that is byte-identical everywhere. Fix a bug once, copy it four times.
#
# CROSS-PLATFORM. Written for macOS, Linux, and Windows under Git Bash / MSYS2 — the
# shell Git for Windows installs, which is also what `npm` already assumes there, so
# no second implementation is needed. The Windows branches (.exe suffix, node.exe
# vendoring, the cliBin rewrite, skipping chmod) were reasoned from the platform's
# rules and from Clatch's own installer, and on 2026-09-21 they were RUN: this
# template built, packaged, validated, packed and round-tripped on a real Windows
# machine, and the binary carries no VCRUNTIME140 import. Linux is still reasoned.
set -eu
. "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/lib.sh"

# ── this clapp ──────────────────────────────────────────────────────────────────
DEPOT=pkg                # the assembled Clatch depot. NEVER dist/: upstream's web build
                         # (apps/gridcraft-web) writes there.
bin_src()      { printf '%s' "$ROOT/target/release/$CLI$EXE"; }
# GridCraft draws with egui: no frontend to embed, so no Tauri build and no embed assertion.
build_binary() { "$ROOT/scripts/build.sh" >/dev/null; }
# GridCraft's licence asks that its notices travel with every copy.
app_extras()   { mkdir -p "$DIST/licenses" && cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/NOTICE" "$ROOT/ATTRIBUTION.md" "$DIST/licenses/"; }

# ── everything below this line is byte-identical in every clapp ──────────────────

OS="$(host_os)"
EXE="$(exe_suffix)"
CLI="$(manifest connector.cli)"  || fail "clatch.json: connector.cli is missing"
ID="$(manifest id)"              || fail "clatch.json: id is missing"
VERSION="$(manifest version)"    || fail "clatch.json: version is missing"
ICON="$(manifest icon)"          || ICON="assets/icon.png"
DIST="$ROOT/$DEPOT"

step "build — $ID $VERSION ($OS/$(uname -m))"
build_binary || fail "the build failed — run it directly to see why"
BIN="$(bin_src)"
[ -f "$BIN" ] || fail "the build produced no binary at $BIN"
ok "$(du -h "$BIN" | awk '{print $1}')"

step "assemble $DEPOT/"
rm -rf "$DIST"
mkdir -p "$DIST/bin" "$DIST/$(dirname -- "$ICON")"

# ONE binary, two roles: `<cli> app` is the GUI Clatch launches, `<cli> <verb>` is the
# agent's CLI. launch.<os> and connector.cliBin in clatch.json both resolve to it.
cp "$BIN" "$DIST/bin/$CLI$EXE"
[ -n "$EXE" ] || chmod +x "$DIST/bin/$CLI"

# Everything the manifest promises: the icon, and a banner or photos if declared.
manifest_assets | while IFS= read -r asset; do
  [ -n "$asset" ] || continue
  [ -f "$ROOT/$asset" ] || fail "clatch.json declares $asset, which is not in this repo"
  mkdir -p "$DIST/$(dirname -- "$asset")"
  cp "$ROOT/$asset" "$DIST/$asset"
done
if [ -f "$ROOT/THIRD_PARTY_NOTICES.md" ]; then
  cp "$ROOT/THIRD_PARTY_NOTICES.md" "$DIST/THIRD_PARTY_NOTICES.md"
fi

# On macOS the binary moves into a real .app bundle so the Dock has an icon and a name to
# read (see macos_app_bundle() in lib.sh — a bare executable has neither).
INNER=""
if [ "$OS" = macos ]; then
  NAME="$(manifest name)" || NAME="$CLI"
  INNER="$(macos_app_bundle "$DIST" "$CLI" "$NAME" "$ID" "$VERSION" "$ROOT/$ICON")"
fi

# The manifest. The DEPOT copy's connector.cliBin is rewritten per host — to the .exe name
# on Windows, into the bundle on macOS — see install_manifest() in lib.sh for why, and why
# the repo copy must stay the plain POSIX form.
if [ -n "$INNER" ]; then
  install_manifest "$ROOT/clatch.json" "$DIST/clatch.json" "$INNER" macos "$INNER"
elif [ -n "$EXE" ]; then
  install_manifest "$ROOT/clatch.json" "$DIST/clatch.json" "bin/$CLI$EXE"
else
  cp "$ROOT/clatch.json" "$DIST/clatch.json"
fi

app_extras

# The depot must contain everything the manifest promises, or the failure surfaces on
# a user's machine at `clatch install` instead of here. Same pair Clatch checks.
step "self-check"
[ -x "$DIST/${INNER:-bin/$CLI$EXE}" ] || fail "$DEPOT/${INNER:-bin/$CLI$EXE} is missing or not executable"
for declared in "$(manifest_path "$DIST/clatch.json" cliBin "$OS")" \
                "$(manifest_path "$DIST/clatch.json" launch "$OS")" \
                $(manifest_assets); do
  [ -z "$declared" ] || [ -e "$DIST/$declared" ] \
    || fail "the manifest declares $declared, which is not in the depot"
done
ok "manifest and files agree"

printf '%s\n' "$DIST"
