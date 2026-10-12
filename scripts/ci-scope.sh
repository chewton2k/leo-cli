#!/bin/sh
set -eu

everything() {
    echo "rust=true"
    echo "browser=true"
    echo 'systems=["ubuntu-24.04","macos-latest","windows-latest"]'
    exit 0
}

base=${1:-}
[ -n "$base" ] || everything
git cat-file -e "$base^{commit}" 2>/dev/null || everything

changed=$(git diff --name-only "$base" HEAD)

outside() {
    printf '%s\n' "$changed" | grep -Ev "$1" | grep -q .
}

docs='\.md$|^docs/|^LICENSE$'
page='^crates/leo-web/src/web/|^crates/leo-web/tests/browser/|^crates/leo-web/tests/[^/]*\.test\.js$'
not_served='^crates/leo-tui/|^scripts/|^install\.(sh|ps1)$|^tests/|^crates/leo-web/tests/[^/]*\.test\.js$|^\.github/workflows/release\.yml$'

rust=false
outside "$docs|$page" && rust=true

browser=false
outside "$docs|$not_served" && browser=true

if [ "$rust" = true ]; then
    systems='["ubuntu-24.04","macos-latest","windows-latest"]'
elif printf '%s\n' "$changed" | grep -q '^crates/leo-web/'; then
    systems='["ubuntu-24.04"]'
else
    systems='[]'
fi

echo "rust=$rust"
echo "browser=$browser"
echo "systems=$systems"
