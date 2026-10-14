#!/bin/sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/out"}
files="$out/files"
rm -rf "$files"
mkdir -p "$files/std"
want=$(cat "$here/GO_VERSION")
have=$(go env GOVERSION)
[ "$have" = "$want" ] || { echo "Build the Go runner with $want (this is $have)." >&2; exit 1; }

export GOOS=wasip1 GOARCH=wasm CGO_ENABLED=0 GOFLAGS=-trimpath
root=$(go env GOROOT)
(cd "$root/src" && go build -buildvcs=false -ldflags="-s -w -buildid=" -o "$files/compile.wasm" cmd/compile)
(cd "$root/src" && go build -buildvcs=false -ldflags="-s -w -buildid=" -o "$files/link.wasm" cmd/link)

: >"$files/importcfg"
go list -export -deps -f '{{if .Export}}{{.ImportPath}} {{.Export}}{{end}}' $(cat "$here/PACKAGES") | sort | while read -r path archive; do
    mkdir -p "$files/std/$(dirname "$path")"
    cp "$archive" "$files/std/$path.a"
    echo "packagefile $path=/go/std/$path.a" >>"$files/importcfg"
done

sum() {
    if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

(cd "$files" && find . -type f ! -name files.sha256 | sed 's|^\./||' | LC_ALL=C sort | while read -r f; do sum "$f"; done) >"$out/files.sha256"
mv "$out/files.sha256" "$files/files.sha256"
got=$(sum "$files/files.sha256" | cut -d' ' -f1)
pinned=$(cat "$here/MANIFEST")
if [ "$got" != "$pinned" ]; then
    echo "files.sha256 is $got, MANIFEST pins $pinned." >&2
    echo "If the Go version or PACKAGES changed on purpose, update MANIFEST and leo_services::gorunner::FILES_SHA256 together." >&2
    exit 1
fi

tar -czf "$out/leo-go.tar.gz" -C "$files" .
echo "Built $out/leo-go.tar.gz"
