#!/usr/bin/env sh
# `npm run build` — build the SHIPPABLE binary and print its path. GridCraft draws with egui,
# so unlike the template (Tauri) there is no frontend to embed and no Tauri step: a cargo
# release build of the one binary that serves both roles (`gridcraft app`, `gridcraft <verb>`).
# The window embeds craft-fonts when CRAFT_FONTS_DIR points at a checkout, as GridCraft's own
# releases do; without it the window uses the system's fonts.
set -eu
. "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/lib.sh"

( cd "$ROOT" && cargo build --release -p "$(manifest connector.cli)" >&2 )
printf '%s\n' "$ROOT/target/release/$(manifest connector.cli)$(exe_suffix)"
