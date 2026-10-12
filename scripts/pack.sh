#!/usr/bin/env sh
# `npm run pack` — assemble the depot, validate it, then build the host-platform
# `.clapp` from it: <id>-<os>-<arch>.clapp, the single file `clatch install` takes and
# the release workflow uploads. Always repackages first, for the same reason validate
# does.
set -eu
. "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/lib.sh"

DIST="$("$ROOT/scripts/package.sh")"
CLATCH="$(find_clatch)" || fail "the clatch CLI was not found. Put it on PATH, set
      CLATCH_BIN=/path/to/clatch, or build it: cargo build --release in ../../clatch"

step "clatch validate"
"$CLATCH" validate "$DIST" || fail "clatch validate rejected the depot"
ok

step "clatch pack"
( cd "$ROOT" && "$CLATCH" pack "$DIST" ) || fail "clatch pack failed"
