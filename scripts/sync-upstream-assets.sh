#!/usr/bin/env bash
# Copy/generate assets from an upstream PinscapePico checkout into this repo:
#   assets/templates/   config templates (Config tab -> "New from template")
#   assets/schema/      schema.json, generated from Firmware/JSONConfigRef.txt
#   assets/public/help/ upstream help pages + images, with JSONConfigRef.htm
#                       regenerated from the same source as the schema
#
# Usage: scripts/sync-upstream-assets.sh [path-to-PinscapePico]   (default: ../PinscapePico)
# Needs a C++20 compiler (c++/g++/clang++) to build tools/schema-gen.
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
UP="$(cd "${1:-$HERE/../PinscapePico}" && pwd)"
[ -f "$UP/Firmware/JSONConfigRef.txt" ] || { echo "upstream checkout not found at $UP" >&2; exit 1; }
CXX="${CXX:-c++}"
WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

echo "== templates"
mkdir -p "$HERE/assets/templates"
cp "$UP"/GUIConfigTool/ConfigTemplates/*.json "$HERE/assets/templates/"

echo "== schema-gen (links upstream ConfigDocParser.cpp unmodified)"
"$CXX" -std=c++20 -O1 -w -include "$HERE/tools/schema-gen/compat.h" -Dmain=upstream_main \
    -c "$UP/ConfigDocParser/ConfigDocParser.cpp" -o "$WORK/cdp.o"
"$CXX" -std=c++20 -O1 -I "$UP/ConfigDocParser" -c "$HERE/tools/schema-gen/emit_json.cpp" -o "$WORK/emit.o"
"$CXX" "$WORK/cdp.o" "$WORK/emit.o" -o "$WORK/schema-gen"

# The upstream reference uses CRLF line endings; normalize so blank-line
# paragraph breaks are recognized on all platforms.
tr -d '\r' < "$UP/Firmware/JSONConfigRef.txt" > "$WORK/JSONConfigRef.txt"

echo "== help pages"
mkdir -p "$HERE/assets/public/help" "$HERE/assets/schema"
cp "$UP"/GUIConfigTool/Help/* "$HERE/assets/public/help/"
"$WORK/schema-gen" "$WORK/JSONConfigRef.txt" "$HERE/assets/public/help/JSONConfigRef.htm" \
    "$HERE/assets/schema/schema.json" "$WORK/ConfigSchema.h"

REV="$(git -C "$UP" log -1 --format='%h %ad' --date=short 2>/dev/null || echo unknown)"
echo "$REV" > "$HERE/assets/UPSTREAM_REVISION"
echo "Synced from PinscapePico $REV"
