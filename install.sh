#!/bin/sh
# Install leo: download the ready-made build for this computer and put it on
# your PATH for good.
#
#   curl -fsSL https://raw.githubusercontent.com/chewton2k/leo-cli/main/install.sh | sh
#
# Settings (all optional):
#   LEO_INSTALL_DIR      where to put leo (default: ~/.local/bin)
#   LEO_INSTALL_ARCHIVE  install from this .tar.gz instead of downloading
set -eu

REPO="chewton2k/leo-cli"
BIN_DIR="${LEO_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
fail() {
    say "leo install: $*" >&2
    exit 1
}

os=$(uname -s)
arch=$(uname -m)
case "$os-$arch" in
    Darwin-arm64) target=aarch64-apple-darwin ;;
    Darwin-x86_64) target=x86_64-apple-darwin ;;
    Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu ;;
    *) fail "there is no ready-made build for $os $arch. Build it from source instead: https://github.com/$REPO#1-install-leo" ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
archive="$tmp/leo.tar.gz"

if [ -n "${LEO_INSTALL_ARCHIVE:-}" ]; then
    cp "$LEO_INSTALL_ARCHIVE" "$archive"
else
    command -v curl >/dev/null 2>&1 || fail "curl is needed to download leo"
    url="https://github.com/$REPO/releases/latest/download/leo-$target.tar.gz"
    say "Downloading leo for $target..."
    curl -fsSL "$url" -o "$archive" || fail "could not download $url"

    # Check the download against the published checksum, when there is a tool
    # to compute one.
    if curl -fsSL "$url.sha256" -o "$archive.sha256" 2>/dev/null; then
        expected=$(cut -d ' ' -f 1 <"$archive.sha256")
        if command -v shasum >/dev/null 2>&1; then
            actual=$(shasum -a 256 "$archive" | cut -d ' ' -f 1)
        elif command -v sha256sum >/dev/null 2>&1; then
            actual=$(sha256sum "$archive" | cut -d ' ' -f 1)
        else
            actual="$expected"
        fi
        [ "$expected" = "$actual" ] || fail "the download is damaged (checksum mismatch); try again"
    fi
fi

tar -xzf "$archive" -C "$tmp" || fail "could not unpack the download"
[ -f "$tmp/leo" ] || fail "the download does not contain leo"
mkdir -p "$BIN_DIR"
cp "$tmp/leo" "$BIN_DIR/leo"
chmod 755 "$BIN_DIR/leo"
say "Installed leo to $BIN_DIR/leo"

# Put the directory on the PATH for every future terminal, in the file this
# shell reads at startup. Mac terminals start bash as a login shell, which reads
# ~/.bash_profile rather than ~/.bashrc.
line="export PATH=\"$BIN_DIR:\$PATH\""
case "$(basename "${SHELL:-sh}")" in
    zsh) rc="$HOME/.zshrc" ;;
    bash)
        if [ "$os" = Darwin ]; then
            rc="$HOME/.bash_profile"
        else
            rc="$HOME/.bashrc"
        fi
        ;;
    fish)
        rc="$HOME/.config/fish/config.fish"
        line="fish_add_path $BIN_DIR"
        ;;
    *) rc="$HOME/.profile" ;;
esac

if ! grep -qsF "$BIN_DIR" "$rc"; then
    mkdir -p "$(dirname "$rc")"
    printf '\n# Added by the leo installer\n%s\n' "$line" >>"$rc"
    say "Added $BIN_DIR to your PATH in $rc"
fi

case ":$PATH:" in
    *":$BIN_DIR:"*) say "Done. Run: leo doctor" ;;
    *) say "Done. Open a new terminal (or run: . $rc), then run: leo doctor" ;;
esac
