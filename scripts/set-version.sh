#!/bin/sh
set -eu

version=${1:?usage: set-version.sh X.Y.Z}
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

awk -v v="$version" '
    !done && /^version = "/ { print "version = \"" v "\""; done = 1; next }
    { print }
' Cargo.toml >"$tmp"
cat "$tmp" >Cargo.toml

awk -v v="$version" '
    {
        if (pending) {
            print (($0 ~ /^source = /) ? held : "version = \"" v "\"")
            pending = 0
        }
        if ($0 ~ /^version = "/ && prev ~ /^name = /) {
            held = $0
            pending = 1
        } else {
            print
        }
        prev = $0
    }
    END { if (pending) print "version = \"" v "\"" }
' Cargo.lock >"$tmp"
cat "$tmp" >Cargo.lock
