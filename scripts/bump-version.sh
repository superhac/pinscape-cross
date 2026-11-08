#!/usr/bin/env bash
# Set the app version everywhere it lives, or check that all copies agree.
#
#   scripts/bump-version.sh 0.2.0          set the version to 0.2.0
#   scripts/bump-version.sh --check        all copies agree?  (prints the version)
#   scripts/bump-version.sh --check 0.2.0  all copies agree and equal 0.2.0?
#
# Copies: Cargo.toml [workspace.package] version (all crates inherit it),
# Cargo.lock, src-tauri/tauri.conf.json, package.json, package-lock.json.
# Doesn't commit or tag; see docs/RELEASING.md.
set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
    sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
}

SEMVER='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$'

# Print each copy as "<file> <version>".
read_versions() {
    python3 - <<'EOF'
import json, re
def cargo_ws():
    sec = None
    for line in open("Cargo.toml", encoding="utf-8"):
        m = re.match(r"\s*\[([^\]]+)\]", line)
        if m:
            sec = m.group(1).strip()
        elif sec == "workspace.package":
            m = re.match(r'\s*version\s*=\s*"([^"]*)"', line)
            if m:
                return m.group(1)
    return "?"
def lock():
    text = open("Cargo.lock", encoding="utf-8").read()
    m = re.search(r'\[\[package\]\]\nname = "pinscape-cross"\nversion = "([^"]*)"', text)
    return m.group(1) if m else "?"
def js(path, *keys):
    v = json.load(open(path, encoding="utf-8"))
    for k in keys:
        v = v.get(k, {}) if isinstance(v, dict) else {}
    return v if isinstance(v, str) else "?"
print("Cargo.toml", cargo_ws())
print("Cargo.lock", lock())
print("src-tauri/tauri.conf.json", js("src-tauri/tauri.conf.json", "version"))
print("package.json", js("package.json", "version"))
print("package-lock.json", js("package-lock.json", "version"))
print("package-lock.json(root)", js("package-lock.json", "packages", "", "version"))
EOF
}

check() {
    local want="${1:-}" first="" bad=0 file ver
    while read -r file ver; do
        [ -z "$first" ] && first="$ver"
        if [ "$ver" != "$first" ] || { [ -n "$want" ] && [ "$ver" != "$want" ]; }; then
            bad=1
        fi
    done < <(read_versions)
    if [ "$bad" = 1 ]; then
        echo "Version mismatch${want:+ (expected $want)}:" >&2
        read_versions | sed 's/^/  /' >&2
        return 1
    fi
    echo "$first"
}

[ $# -ge 1 ] || usage
if [ "$1" = "--check" ]; then
    want="${2:-}"
    want="${want#v}"
    check "$want"
    exit
fi
case "$1" in -*) usage ;; esac

new="${1#v}"
if ! [[ "$new" =~ $SEMVER ]]; then
    echo "error: '$new' is not a semantic version (x.y.z or x.y.z-pre)" >&2
    exit 2
fi

# Cargo.toml: only the version line inside [workspace.package].
# tauri.conf.json: the top-level "version" key (2-space indent).
python3 - "$new" <<'EOF'
import re, sys
new = sys.argv[1]

def sub_file(path, fn):
    text = open(path, encoding="utf-8").read()
    out = fn(text)
    if out == text and new not in text:
        sys.exit(f"error: no version found in {path}")
    open(path, "w", encoding="utf-8", newline="").write(out)

def cargo(text):
    lines = text.splitlines(keepends=True)
    sec, done = None, False
    for i, line in enumerate(lines):
        m = re.match(r"\s*\[([^\]]+)\]", line)
        if m:
            sec = m.group(1).strip()
        elif sec == "workspace.package" and not done:
            m = re.match(r'(\s*version\s*=\s*")[^"]*(".*)', line, re.S)
            if m:
                lines[i] = m.group(1) + new + m.group(2)
                done = True
    if not done:
        sys.exit("error: [workspace.package] version not found in Cargo.toml")
    return "".join(lines)

def tauri(text):
    out, n = re.subn(r'(?m)^(  "version":\s*")[^"]*(")', r"\g<1>" + new + r"\g<2>", text, count=1)
    if n != 1:
        sys.exit("error: top-level \"version\" not found in src-tauri/tauri.conf.json")
    return out

sub_file("Cargo.toml", cargo)
sub_file("src-tauri/tauri.conf.json", tauri)
EOF

# package.json + package-lock.json (no git commit/tag, no network).
npm version "$new" --no-git-tag-version --allow-same-version >/dev/null

# Cargo.lock: refresh the workspace members' versions (offline is enough for
# path crates; fall back to the network only if cargo insists).
cargo update --workspace --offline --quiet 2>/dev/null || cargo update --workspace --quiet

check "$new" >/dev/null
echo "Version set to $new:"
read_versions | sed 's/^/  /'
echo
echo "Next: add a '## [$new]' section to CHANGELOG.md, commit, then tag v$new (docs/RELEASING.md)."
