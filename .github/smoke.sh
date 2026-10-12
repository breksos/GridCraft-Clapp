#!/usr/bin/env sh
# Prove the packaged binary RUNS on this OS before anything publishes it.
#
# The CLI role is console-subsystem and needs no window, no webview and no display, which
# is what makes it the one thing a headless runner can check. It is also the missing-DLL
# check, and a better one than reading the import table: Windows resolves a PE's imports at
# process start, so an exe with an unsatisfied dependency cannot print `--help` at all.
#
# Nothing here is guessed. The binary path comes from the DEPOT's manifest, which
# package.sh rewrote per host (on macOS it points inside the .app), and the probe verb
# comes from the one place the app already declares it — verify.sh's PROBE.
set -eu

BIN="pkg/$(node -e "process.stdout.write(JSON.parse(require('fs').readFileSync('pkg/clatch.json','utf8')).connector.cliBin)")"
[ -f "$BIN" ] || { echo "::error::$BIN was not built"; exit 1; }

PROBE=$(sed -n 's/^PROBE=\([A-Za-z0-9_-]*\).*/\1/p' scripts/verify.sh | head -n 1)
[ -n "$PROBE" ] || { echo "::error::scripts/verify.sh declares no PROBE verb"; exit 1; }

"$BIN" --help | head -n 1

# No app is running here, so the probe MUST fail — but with the app's own sentence, which
# is what proves the CLI role got as far as dialling its socket. Answering would mean the
# verb never needed the other surface at all.
if out=$("$BIN" "$PROBE" 2>&1); then
  echo "::error::\`$PROBE\` answered with no app running: $out"
  exit 1
fi
case "$out" in
  *"not running"*|*"blocked by the sandbox"*) echo "cli reached the socket: $out" ;;
  *) echo "::error::unexpected failure: $out"; exit 1 ;;
esac
