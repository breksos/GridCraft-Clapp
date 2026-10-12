#!/usr/bin/env sh
# Shared helpers for this clapp's scripts. SOURCED, never executed:
#
#   . "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/lib.sh"
#
# This file is BYTE-IDENTICAL in every clapp (template, chess, clock, telegram,
# whatsapp). Fix it in one repo, copy it to the others — never fork it.
#
# Identity is read from clatch.json rather than being sed-substituted into the
# scripts: the manifest is the single source of truth for the app id and the CLI
# name, so a fork rewrites ONE file and every script here follows automatically.

# The repo root. Computed from $0 — the script that sourced us, always scripts/*.sh.
ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

# ── pretty output ───────────────────────────────────────────────────────────────
# Everything here goes to STDERR. stdout is reserved: `package.sh` prints exactly
# one line there, the depot path, so `clatch validate "$(npm run -s package)"` is a
# legal thing to write.
step() { printf '\n▸ %s\n' "$1" >&2; }
ok()   { printf '  ok%s\n' "${1:+ — $1}" >&2; }
note() { printf '  (%s)\n' "$1" >&2; }
fail() { printf '\n✗ FAIL — %s\n' "$1" >&2; exit 1; }

# ── the manifest is the source of truth ─────────────────────────────────────────
# Print one dotted path out of clatch.json (`manifest connector.cli`). node is used
# rather than jq or python3 because every clapp already needs node for its frontend,
# on every OS including Windows — jq is not installed by default anywhere and
# python3 is absent on a stock Windows box.
manifest() {
  node -e '
    const [path, file] = process.argv.slice(1);
    let v = JSON.parse(require("fs").readFileSync(file, "utf8"));
    for (const k of path.split(".")) v = (v == null ? v : v[k]);
    if (v === undefined || v === null) process.exit(1);
    process.stdout.write(String(v));
  ' "$1" "$ROOT/clatch.json"
}

# Print the depot-relative path a manifest declares for one of the three things that
# must physically exist in the depot — `cliBin`, `launch` or `icon` — resolved for an
# OS. Empty output means "not declared for this OS", which is not an error.
# Every presentation asset the manifest declares, one per line: icon, banner and each
# photo. A depot must contain what its manifest promises — a declared asset that is not in
# it fails at the USER's `clatch install`, not here. Empty output is legal; only `icon` is
# even common.
manifest_assets() {
  node -e '
    const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    const out = [m.icon, m.banner, ...(Array.isArray(m.photos) ? m.photos : [])];
    // A trailing newline, deliberately: `while read` drops a last line that has none,
    // which silently skipped whichever asset happened to be last.
    for (const a of out.filter(Boolean)) process.stdout.write(a + "\n");
  ' "$ROOT/clatch.json"
}

manifest_path() { # <manifest> <cliBin|launch|icon> <os>
  node -e '
    const [file, kind, os] = process.argv.slice(1);
    const m = JSON.parse(require("fs").readFileSync(file, "utf8"));
    const per = (v) => (v && typeof v === "object" ? v[os] : v);
    const out =
      kind === "cliBin" ? (m.connector && (m.connector.cliBin || "bin/" + m.connector.cli))
    : kind === "launch" ? per(m.launch)
    :                     m.icon;
    process.stdout.write(out || "");
  ' "$1" "$2" "$3"
}

# Copy clatch.json into the depot, with connector.cliBin forced to <cliBin>.
#
# `connector.cliBin` is a single string — protocol.md §1 gives it no per-OS form the
# way `launch` has — so the repo manifest can only carry one spelling, and it carries
# the POSIX one (`bin/chess`). Windows will not execute an extensionless image, and
# Clatch's check_files() hard-errors ("declared CLI binary not found in the package")
# on both `clatch validate` and `clatch install`, so a Windows depot built from the
# repo manifest verbatim cannot be installed at all. A `.clapp` is already per-OS-arch
# (<id>-<os>-<arch>.clapp), so rewriting the DEPOT copy is the honest fix. The repo
# copy stays POSIX — do not "fix" it there.
install_manifest() { # <src> <dst> <cliBin> [launch-os] [launch-path]
  node -e '
    const fs = require("fs");
    const [src, dst, cliBin, os, launch] = process.argv.slice(1);
    const m = JSON.parse(fs.readFileSync(src, "utf8"));
    if (m.connector) m.connector.cliBin = cliBin;
    // The macOS depot puts the binary inside a .app bundle, so BOTH the launch command
    // and the CLI path move with it — they are two roles of the one executable.
    // A depot is one platform, so its manifest carries exactly ONE launch OS key: the one
    // this depot runs on. Keep that entry (rewritten for the .app bundle when a path is
    // given) plus args, and drop every other OS key so the .clapp names only its platform.
    if (m.launch) {
      const only = os || { darwin: "macos", win32: "windows", linux: "linux" }[process.platform];
      m.launch = Object.assign(
        {},
        only ? { [only]: launch || m.launch[only] } : {},
        m.launch.args !== undefined ? { args: m.launch.args } : {},
      );
    }
    fs.writeFileSync(dst, JSON.stringify(m, null, 2) + "\n");
  ' "$1" "$2" "$3" "${4:-}" "${5:-}"
}

# ── the host ────────────────────────────────────────────────────────────────────
# uname is the portable answer on macOS and Linux AND under Git Bash / MSYS2 /
# Cygwin on Windows, where it reports MINGW64_NT-… / MSYS_NT-… / CYGWIN_NT-….
host_os() {
  case "$(uname -s)" in
    Darwin)               printf 'macos' ;;
    Linux)                printf 'linux' ;;
    MINGW*|MSYS*|CYGWIN*) printf 'windows' ;;
    *)                    printf 'unknown' ;;
  esac
}

# The executable suffix for the host: Windows will not run an extensionless image.
exe_suffix() { if [ "$(host_os)" = windows ]; then printf '.exe'; fi; }

# A Windows path (C:\x\y) turned into something this shell can open. Only Git Bash /
# Cygwin ever need it, and only they ship cygpath.
unixpath() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -u "$1"; else printf '%s' "$1"; fi
}

# ── the clatch CLI ──────────────────────────────────────────────────────────────
# $CLATCH_BIN wins, then PATH, then the sibling checkout every dev here has
# (clapps/<app>-clapp/../../clatch). Prints the path; returns 1 when there is none,
# so callers can decide whether that is a skip or a failure.
find_clatch() {
  if [ -n "${CLATCH_BIN:-}" ] && [ -x "$CLATCH_BIN" ]; then printf '%s' "$CLATCH_BIN"; return 0; fi
  if command -v clatch >/dev/null 2>&1; then command -v clatch; return 0; fi
  for c in "$ROOT/../../clatch/target/release/clatch$(exe_suffix)"; do
    if [ -x "$c" ]; then (CDPATH= cd -- "$(dirname -- "$c")" && printf '%s/%s' "$(pwd)" "$(basename -- "$c")"); return 0; fi
  done
  return 1
}

# ── the Tauri build ─────────────────────────────────────────────────────────────
# NOT `cargo build --release`. The Tauri CLI turns on the `custom-protocol` feature;
# cargo does not, so a plain cargo release binary still points the webview at devUrl and
# the packaged app opens white — or shows another app's UI if that port is busy.
# package.sh asserts the hashed bundle name appears inside the binary, which makes
# shipping that impossible.
#
# `--no-bundle`: a depot is bin/ + assets/ + clatch.json, not an OS installer.
# CLAPP_FRONTEND_ONLY is the re-entrancy guard for `npm run build` — see build.sh.
tauri_build() {
  ( cd "$ROOT" && CLAPP_FRONTEND_ONLY=1 npm run --silent tauri -- build --no-bundle >&2 )
}

# The guard for the trap above, run on the binary that is about to be packaged.
#
# Vite writes hashed bundle names (dist-web/assets/index-yG4_5Aww.js) and Tauri's
# custom-protocol build embeds the frontend under those exact names, so the string is
# findable inside the executable. A binary built WITHOUT custom-protocol has no
# embedded frontend at all — the check fails, loudly, here, instead of shipping an
# app that opens a white window on someone else's machine.
assert_frontend_embedded() { # <binary>
  # Where the frontend lands is tauri.conf.json's call (build.frontendDist), not ours —
  # read it rather than hardcoding, so moving dist/ → dist-web/ cannot silently turn
  # this guard into a no-op.
  web="$(cd "$ROOT/src-tauri" && node -e '
    const c = JSON.parse(require("fs").readFileSync("tauri.conf.json", "utf8"));
    process.stdout.write(require("path").resolve((c.build && c.build.frontendDist) || "../dist"));
  ')"
  bundle="$(ls "$web/assets/"*.js 2>/dev/null | head -1)"
  [ -n "$bundle" ] || fail "no bundle in $web/assets — the frontend never built"
  bundle="$(basename -- "$bundle")"
  LC_ALL=C grep -aq -- "$bundle" "$1" || fail "$(basename -- "$1") does not embed the frontend ($bundle).
      That is what a binary built WITHOUT Tauri's custom-protocol feature looks like —
      a plain \`cargo build --release\`. It would point the webview at devUrl and open
      a white window (or a stale dev page). Build with scripts/package.sh."
}

# ── macOS: wrap the ONE binary in a real .app bundle ────────────────────────────
# A bare Mach-O has no icon identity: macOS shows the generic terminal tile, and painting
# over it at runtime still lets the generic through every time AppKit rebuilds the tile —
# at launch, on an activation-policy change, while quitting. Info.plist is where the Dock
# actually reads it, so there is no generic phase at all.
#
# The binary is MOVED in, not copied, and `launch.macos` and `cliBin` are rewritten to
# point inside — still one binary, two roles.
macos_app_bundle() { # <dist> <cli> <display-name> <id> <version> <src-icon-png> -> prints inner exec path
  _d=$1; _cli=$2; _name=$3; _id=$4; _ver=$5; _icon=$6
  _app="$_d/bin/$_name.app"
  mkdir -p "$_app/Contents/MacOS" "$_app/Contents/Resources"
  mv "$_d/bin/$_cli" "$_app/Contents/MacOS/$_cli"
  chmod +x "$_app/Contents/MacOS/$_cli"

  # The .icns carries the SAME inset the running app applies, so the icon does not change
  # size the moment the process sets its own (clappkit::dock_icon — one implementation).
  # Built OUTSIDE the depot: a stray work file under $_d would be packed into the .clapp.
  # The directory must be named `<something>.iconset` — iconutil rejects any other name.
  _work="${TMPDIR:-/tmp}/clapp-icon-$$"; rm -rf "$_work"; mkdir -p "$_work"
  _pad="$_work/dock.png"
  # Run clappkit's dock-icon through THE APP's manifest so the vendor [patch] applies —
  # clappkit alone pins clatch over ssh://, which a CI runner has no key for. No silent
  # full-bleed fallback: it shipped a Dock icon that towered over its neighbours on
  # exactly the machines nobody was watching.
  # The committed inset beside icon.png is the normal path — no tool, no key, works on
  # every runner (maps-clapp's scripts/make-ico.py shows how to generate it). The
  # dock-icon binaries stay as dev fallbacks; silently shipping full-bleed never happens.
  if [ -f "$ROOT/assets/icon-dock.png" ]; then
    cp "$ROOT/assets/icon-dock.png" "$_pad"
  elif "$ROOT/clappkit/target/release/dock-icon" "$_icon" "$_pad" 2>/dev/null \
     || "$ROOT/../clappkit/target/release/dock-icon" "$_icon" "$_pad" 2>/dev/null; then :; else
    fail "no assets/icon-dock.png and no clappkit dock-icon binary — generate the inset;
      refusing to ship a full-bleed .icns that would tower over every icon beside it"
  fi
  _set="$_work/$_cli.iconset"; mkdir -p "$_set"
  for _s in 16 32 128 256 512; do
    sips -z "$_s" "$_s" "$_pad" --out "$_set/icon_${_s}x${_s}.png" >/dev/null 2>&1
    _2=$((_s * 2))
    sips -z "$_2" "$_2" "$_pad" --out "$_set/icon_${_s}x${_s}@2x.png" >/dev/null 2>&1
  done
  iconutil -c icns "$_set" -o "$_app/Contents/Resources/$_cli.icns" >/dev/null 2>&1 \
    || fail "iconutil could not build $_cli.icns — the bundle would ship with the generic
      terminal icon, which is the whole reason this bundle exists"
  _work_ent="$(mktemp -t recorder-ent).plist"
  rm -rf "$_work"

  # assets/Info.extra.plist, if present, is spliced in verbatim: the usage strings macOS
  # demands before it will show a permission prompt (a microphone, a camera) live there,
  # so an app that needs one declares it beside its icon and lib.sh stays the family's.
  cat > "$_app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>$_cli</string>
  <key>CFBundleIdentifier</key><string>$_id</string>
  <key>CFBundleName</key><string>$_name</string>
  <key>CFBundleDisplayName</key><string>$_name</string>
  <key>CFBundleIconFile</key><string>$_cli</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>$_ver</string>
  <key>CFBundleVersion</key><string>$_ver</string>
  <key>NSHighResolutionCapable</key><true/>
$(cat "$ROOT/assets/Info.extra.plist" 2>/dev/null || true)
</dict></plist>
PLIST
  plutil -lint "$_app/Contents/Info.plist" >/dev/null 2>&1 \
    || fail "the generated Info.plist is not valid"
  # Ad-hoc signed, with the bundle id as the identifier: TCC keys a permission to the
  # signature, so a grant survives a move and the prompt names the app. Unsigned, TCC
  # keys to the path and some prompts name whoever spawned us.
  #
  # The entitlements are the device-access ones a recorder needs. Under a hardened
  # runtime a missing audio-input entitlement makes TCC deny the microphone SILENTLY —
  # no prompt at all — and while an ad-hoc bundle is not hardened, declaring them is the
  # robust path every capture app (Chromium, Qt) takes, and it costs nothing here.
  _ent="$_work_ent"
  cat > "$_ent" <<'ENT'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.device.audio-input</key><true/>
  <key>com.apple.security.device.microphone</key><true/>
</dict></plist>
ENT
  codesign --force --sign - --identifier "$_id" --entitlements "$_ent" "$_app" >/dev/null 2>&1 \
    || codesign --force --sign - --identifier "$_id" "$_app" >/dev/null 2>&1 \
    || note "codesign unavailable — the bundle ships unsigned"
  printf 'bin/%s.app/Contents/MacOS/%s\n' "$_name" "$_cli"
}
