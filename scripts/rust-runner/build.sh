#!/bin/sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/out"}
files="$out/files"
rm -rf "$files"
mkdir -p "$files"
commit=$(cat "$here/COMMIT")
base="https://raw.githubusercontent.com/LyonSyonII/rubri/$commit/example/public/wasm-rustc"

sum() {
    if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

while read -r want path; do
    mkdir -p "$files/$(dirname "$path")"
    curl -sSL --fail --retry 3 -o "$files/$path.gz" "$base/$path.gz"
    gunzip -f "$files/$path.gz"
    got=$(sum "$files/$path" | cut -d' ' -f1)
    if [ "$got" != "$want" ]; then
        echo "$path is $got, FILES pins $want" >&2
        exit 1
    fi
done <"$here/FILES"
cp "$here/NOTICE" "$files/NOTICE"

(cd "$files" && find . -type f ! -name files.sha256 | sed 's|^\./||' | LC_ALL=C sort | while read -r f; do sum "$f"; done) >"$out/files.sha256"
mv "$out/files.sha256" "$files/files.sha256"
got=$(sum "$files/files.sha256" | cut -d' ' -f1)
pinned=$(cat "$here/MANIFEST")
if [ "$got" != "$pinned" ]; then
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        echo "::error title=Runner build differs::$(java -version 2>&1 | head -n 1 || true) files.sha256=$got $(tr '\n' ' ' <"$files/files.sha256")"
    fi
    echo "files.sha256 is $got, MANIFEST pins $pinned." >&2
    echo "If FILES or NOTICE changed on purpose, update MANIFEST and leo_services::runtimes::RUST together." >&2
    exit 1
fi

tar -czf "$out/leo-rust.tar.gz" -C "$files" .
echo "Built $out/leo-rust.tar.gz"
