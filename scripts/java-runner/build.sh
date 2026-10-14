#!/bin/sh
set -eu

step=start
trap 'status=$?; [ "$status" = 0 ] || [ -z "${GITHUB_ACTIONS:-}" ] || echo "::error title=Java runner build failed::exit $status during $step"' EXIT
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/out"}
work="$out/src"
files="$out/files"
rm -rf "$work" "$files"
mkdir -p "$work" "$files"

java -version 2>&1 | head -n 1 | grep -q "\"$(cat "$here/JDK_VERSION")" || {
    echo "Build the Java runner with JDK $(cat "$here/JDK_VERSION")." >&2
    [ -z "${GITHUB_ACTIONS:-}" ] || echo "::error title=Wrong JDK::$(java -version 2>&1 | head -n 1)"
    exit 1
}

step=fetch
commit=$(cat "$here/COMMIT")
git -C "$work" init -q
git -C "$work" fetch -q --depth 1 https://github.com/konsoletyper/teavm-javac.git "$commit"
git -C "$work" checkout -q FETCH_HEAD
git -C "$work" apply "$here/inner-classes.patch"
step=gradle
(cd "$work" && ./gradlew --no-daemon -q :ui:war)
step=collect

cp "$work/compiler/build/generated/teavm/wasm-gc/compiler.wasm" "$files/"
cp "$work/compiler/build/generated/teavm/wasm-gc/compiler.wasm-runtime.js" "$files/"
cp "$work/compiler/build/classlib/compile-classlib-teavm.bin" "$files/"
cp "$work/compiler/build/classlib/runtime-classlib-teavm.bin" "$files/"
cp "$here/NOTICE" "$here/inner-classes.patch" "$files/"

sum() {
    if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

(cd "$files" && for f in $(ls | LC_ALL=C sort); do sum "$f"; done) >"$out/files.sha256"
mv "$out/files.sha256" "$files/files.sha256"
got=$(sum "$files/files.sha256" | cut -d' ' -f1)
pinned=$(cat "$here/MANIFEST")
if [ "$got" != "$pinned" ]; then
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        echo "::error title=Runner build differs::$(java -version 2>&1 | head -n 1 || true) files.sha256=$got $(tr '\n' ' ' <"$files/files.sha256")"
    fi
    cat "$files/files.sha256" >&2
    echo "files.sha256 is $got, MANIFEST pins $pinned." >&2
    echo "If COMMIT, the patch or the JDK changed on purpose, update MANIFEST and leo_services::runtimes::JAVA together." >&2
    exit 1
fi

tar -czf "$out/leo-java.tar.gz" -C "$files" .
echo "Built $out/leo-java.tar.gz"
