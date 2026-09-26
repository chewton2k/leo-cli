#!/bin/sh
# Set leo's version: the workspace version in Cargo.toml, and every leo crate's
# entry in Cargo.lock so a --locked build still accepts it. The release workflow
# runs this and commits the result to main.
set -eu

version=${1:?usage: set-version.sh X.Y.Z}
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

# The first quoted version line is [workspace.package]'s.
awk -v v="$version" '
    !done && /^version = "/ { print "version = \"" v "\""; done = 1; next }
    { print }
' Cargo.toml >"$tmp"
cat "$tmp" >Cargo.toml

# A package with no `source =` line after its version is one of leo's own.
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
