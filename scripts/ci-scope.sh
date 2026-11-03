#!/bin/sh
set -eu

everything() {
    echo "rust=true"
    echo "browser=true"
    echo "node=true"
    echo "installers=true"
    echo "go_runner=true"
    echo "java_runner=true"
    echo "rust_runner=true"
    echo 'systems=["macos-latest","windows-latest"]'
    exit 0
}

base=${1:-}
[ -n "$base" ] || everything
git cat-file -e "$base^{commit}" 2>/dev/null || everything

rust=false
browser=false
node=false
installers=false
go_runner=false
java_runner=false
rust_runner=false

changed=$(git diff --name-only --no-renames "$base" HEAD) || everything
while IFS= read -r file; do
    case "$file" in
        '') ;;
        crates/leo-web/src/web/*) browser=true; node=true ;;
        crates/leo-web/tests/browser/*) browser=true ;;
        crates/leo-web/tests/*.test.js) node=true ;;
        *.md|docs/*|LICENSE) ;;
        scripts/go-runner/*) go_runner=true; rust=true; browser=true ;;
        scripts/java-runner/*) java_runner=true; rust=true; browser=true ;;
        scripts/rust-runner/*) rust_runner=true; rust=true; browser=true ;;
        crates/leo-services/src/runtimes.rs)
            go_runner=true; java_runner=true; rust_runner=true; rust=true; browser=true ;;
        scripts/install-app.*|scripts/desktop_release.py|scripts/test_desktop_release.py|scripts/activate_app_installers.py|scripts/validate_updater_signing.py|scripts/import_macos_certificate.py|scripts/stage_windows_runtime.ps1|install.sh|install.ps1)
            installers=true; rust=true ;;
        src/*|tests/*|crates/leo-desktop/*|crates/*/tests/*) rust=true ;;
        crates/leo-core/*|crates/leo-services/*|crates/leo-web/*|crates/leo-host/*)
            rust=true; browser=true ;;
        *) everything ;;
    esac
done <<EOF
$changed
EOF

if [ "$rust" = true ]; then
    systems='["macos-latest","windows-latest"]'
else
    systems='[]'
fi

echo "rust=$rust"
echo "browser=$browser"
echo "node=$node"
echo "installers=$installers"
echo "go_runner=$go_runner"
echo "java_runner=$java_runner"
echo "rust_runner=$rust_runner"
echo "systems=$systems"
