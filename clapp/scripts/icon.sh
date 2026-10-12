#!/usr/bin/env sh
# assets/icon.png, the clapp's icon, from upstream's own mark (clappkit/docs/icons.md).
# Upstream's 1024 PNG (rendered from assets/app-icon/gridcraft.svg) is already the tile
# full-bleed; the tile is still cut along its own edges, as in the other Craft clapps, so a
# padded upstream render later cannot slip through, and brought back to 1024 x 1024.
set -eu
ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"
python3 - "$ROOT/assets/app-icon/gridcraft-1024.png" "$ROOT/assets/icon.png" <<'PY'
import sys
from PIL import Image
tile = Image.open(sys.argv[1]).convert("RGBA")
tile = tile.crop(tile.getchannel("A").point(lambda a: 255 if a > 8 else 0).getbbox())
tile.resize((1024, 1024), Image.LANCZOS).save(sys.argv[2], optimize=True)
PY
