#!/usr/bin/env sh
# `npm run validate` — assemble the depot, then run Clatch's own conformance oracle
# over it. This is the only check that the manifest and the files on disk agree, so it
# always repackages first: it can never bless a stale depot.
set -eu
. "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/lib.sh"

DIST="$("$ROOT/scripts/package.sh")"
CLATCH="$(find_clatch)" || fail "the clatch CLI was not found. Put it on PATH, set
      CLATCH_BIN=/path/to/clatch, or build it: cargo build --release in ../../clatch"

step "clatch validate"
"$CLATCH" validate "$DIST" || fail "clatch validate rejected the depot"
ok "$DIST"
