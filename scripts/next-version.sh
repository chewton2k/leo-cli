#!/bin/sh
# Print the version the next release of leo should have, or nothing when there
# is nothing new to release. Run from the repository root; the release workflow
# runs it once the tests have passed on main.
#
#   no release yet                      the version in Cargo.toml
#   nothing in src/, crates/, Cargo.*   nothing: a README change builds the
#     changed since the last release      same program
#   Cargo.toml's version is newer       that version: this is how a minor or
#     than the last release               major release is made
#   otherwise                           the last release, patch number + 1
set -eu

base=$(sed -n 's/^version = "\([0-9.]*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$base" ]; then
    echo "next-version: no version in Cargo.toml" >&2
    exit 1
fi

latest=$(git tag -l 'v[0-9]*' --sort=-v:refname | head -n 1)
if [ -z "$latest" ]; then
    echo "$base"
    exit 0
fi

if git diff --quiet "$latest" HEAD -- src crates Cargo.toml Cargo.lock; then
    exit 0
fi

# Whether version $1 is newer than $2, comparing each part as a number.
newer() {
    awk -v a="$1" -v b="$2" 'BEGIN {
        split(a, x, "."); split(b, y, ".")
        for (i = 1; i <= 3; i++) {
            if (x[i] + 0 > y[i] + 0) exit 0
            if (x[i] + 0 < y[i] + 0) exit 1
        }
        exit 1
    }'
}

last=${latest#v}
if newer "$base" "$last"; then
    echo "$base"
else
    echo "$last" | awk -F. '{ printf "%d.%d.%d\n", $1, $2, $3 + 1 }'
fi
