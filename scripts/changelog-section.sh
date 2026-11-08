#!/usr/bin/env bash
# Print the CHANGELOG.md section for one version (used as the release notes).
#
#   scripts/changelog-section.sh 0.2.0   -> body of "## [0.2.0] ..." up to the next "## "
#
# Exits 1 (printing nothing) if CHANGELOG.md has no section for that version.
set -euo pipefail

cd "$(dirname "$0")/.."

[ $# -eq 1 ] || { echo "usage: $0 <version>" >&2; exit 2; }
ver="${1#v}"

awk -v ver="$ver" '
    BEGIN { found = 0; on = 0 }
    /^## / {
        if (on) exit
        h = $0
        sub(/^## +/, "", h)
        sub(/^\[/, "", h)
        split(h, parts, /[] ]/)
        if (parts[1] == ver) { on = 1; found = 1; next }
    }
    /^\[[^]]+\]: / { if (on) exit }
    on { print }
    END { exit found ? 0 : 1 }
' CHANGELOG.md | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
