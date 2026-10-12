#!/usr/bin/env sh
# Does this clapp actually work? build → package → `clatch validate` → prove the GUI
# process and the agent CLI round-trip over the app's private socket. One command, one
# PASS/FAIL. Run it before every commit.
#
# The round-trip runs THE BINARY FROM THE DEPOT, not the one in target/release, so
# what is tested is exactly what ships — including, for a clapp that vendors a runtime,
# that the vendored runtime is found where the app looks for it.
#
# It needs a desktop session (a GUI window has to come up), so it is a local dev gate,
# not a CI step. CI builds, packages and validates; a headless box stops at the socket.
set -eu
. "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/lib.sh"

# ── this clapp ──────────────────────────────────────────────────────────────────
PROBE=state            # a DECLARED, read-only verb — the round-trip probe
QUIT=close             # the DECLARED verb that ends the app
app_checks() { :; }

# ── everything below this line is byte-identical in every clapp ──────────────────

CLI="$(manifest connector.cli)" || fail "clatch.json: connector.cli is missing"
# For `app_checks` to use. The round-trip below does NOT need it: the depot manifest
# already spells the host's binary, .exe and all.
EXE="$(exe_suffix)"

# 1. build + assemble. package.sh prints the depot path on stdout and narrates on
#    stderr, so this both runs the build and tells us where the artefact landed.
DIST="$("$ROOT/scripts/package.sh")"
# The depot's OWN manifest, not this repo's: package.sh rewrites connector.cliBin per
# host — on macOS the binary moves into a real .app bundle, so `bin/<cli>` is not where
# it lands. Hardcoding that path made this gate silently unrunnable for every bundled
# app: the probe could never reach a binary that was not there, and the failure read as
# "the app never came up".
BIN="$DIST/$(manifest_path "$DIST/clatch.json" cliBin "$(host_os)")"

app_checks

# 2. the conformance oracle: does the depot satisfy the Clapp Protocol?
step "clatch validate"
if CLATCH="$(find_clatch)"; then
  "$CLATCH" validate "$DIST" >&2 || fail "clatch validate rejected the depot"
  ok
else
  note "clatch not found — skipped. Put it on PATH or set CLATCH_BIN to run the oracle"
fi

# 3. the two surfaces must actually talk. Nothing else in the toolchain checks this:
#    clatch validate reads the manifest, the compiler reads the code, and neither can
#    tell you that `<cli> <verb>` reaches the running window.
step "socket round-trip — agent CLI ⇄ GUI, through the packaged binary"
LOG="${TMPDIR:-/tmp}/$CLI-verify.log"
CLATCH_STANDALONE=1 "$BIN" app >"$LOG" 2>&1 &
PID=$!
OUT=""
i=0
while [ "$i" -lt 100 ]; do
  if OUT="$("$BIN" "$PROBE" 2>&1)"; then break; fi
  OUT=""
  sleep 0.2
  i=$((i + 1))
done
if [ -z "$OUT" ]; then
  kill "$PID" 2>/dev/null || true
  fail "\`$CLI $PROBE\` never reached the app (20s). Tail of $LOG:
$(tail -n 20 "$LOG" 2>/dev/null | sed 's/^/      /')"
fi
printf '%s\n' "$OUT" | sed 's/^/      /' >&2
ok "\`$CLI $PROBE\` answered"

"$BIN" "$QUIT" >/dev/null 2>&1 || true
sleep 0.5
kill "$PID" 2>/dev/null || true

printf '\n✓ PASS — %s builds, packages%s, and its two surfaces talk.\n' \
  "$CLI" "$(find_clatch >/dev/null 2>&1 && printf ', validates' || printf ' (validate skipped)')" >&2
