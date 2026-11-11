#!/bin/sh
set -eu

everything() {
    echo "rust=true"
    echo "browser=true"
    echo "node=true"
    echo "installers=true"
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

changed=$(git diff --name-only --no-renames "$base" HEAD) || everything
while IFS= read -r file; do
    case "$file" in
        '') ;;
        crates/leo-web/src/web/styles/*|crates/leo-web/src/web/page-head.html|crates/leo-web/src/web/page-tail.html)
            rust=true; browser=true; node=true ;;
        crates/leo-web/src/web/vendor/*) browser=true; node=true ;;
        crates/leo-web/src/web/app/*|crates/leo-web/src/web/*.js)
            rust=true; browser=true; node=true ;;
        crates/leo-web/src/web/*) browser=true; node=true ;;
        crates/leo-web/tests/browser/server.js) browser=true; node=true ;;
        crates/leo-web/tests/browser/*) browser=true ;;
        crates/leo-web/tests/page-source.js) browser=true; node=true ;;
        crates/leo-web/tests/*.js) node=true ;;
        crates/leo-web/src/routes/assets.rs|crates/leo-web/src/routes/mod.rs)
            rust=true; browser=true; node=true ;;
        crates/leo-desktop/src/lib.rs|crates/leo-desktop/src/print.js|crates/leo-desktop/permissions/*|crates/leo-desktop/capabilities/*|crates/leo-desktop/dist/dictation.js)
            rust=true; node=true ;;
        README.md|CONTRIBUTING.md) rust=true ;;
        *.md|docs/*|LICENSE) ;;
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
echo "systems=$systems"
