#!/bin/sh
# Install leo: download the ready-made build for this computer and put it on
# your PATH for good.
#
#   curl -fsSL https://raw.githubusercontent.com/chewton2k/leo-cli/main/install.sh | bash
#
# Settings (all optional):
#   LEO_INSTALL_DIR      where to put leo (default: ~/.local/bin)
#   LEO_INSTALL_ARCHIVE  install from this .tar.gz instead of downloading
#   LEO_INSTALL_SKIP_PATH  leave shell startup files alone (`leo update` sets it)
#   NO_COLOR             plain output, no colors
set -eu

REPO="chewton2k/leo-cli"
BIN_DIR="${LEO_INSTALL_DIR:-$HOME/.local/bin}"

# Colors and symbols only on a terminal that can show them, so a log or a pipe
# gets plain text.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-dumb}" != dumb ]; then
    esc=$(printf '\033')
    bold="$esc[1m" dim="$esc[2m" green="$esc[32m" red="$esc[31m"
    cyan="$esc[36m" yellow="$esc[33m" reset="$esc[0m"
else
    bold="" dim="" green="" red="" cyan="" yellow="" reset=""
fi
case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
    *UTF-8* | *utf-8* | *UTF8* | *utf8*)
        ok="✓" bad="✗" arrow="↓" to="→"
        rule="────────────────────────────────────────────────────"
        ;;
    *)
        ok="ok" bad="x" arrow=">" to="->"
        rule="----------------------------------------------------"
        ;;
esac

say() { printf '%s\n' "$*"; }
step() { printf '  %s%s%s %s\n' "$green" "$ok" "$reset" "$*"; }
doing() { printf '  %s%s%s %s\n' "$cyan" "$arrow" "$reset" "$*"; }
fail() {
    printf '  %s%s %s%s\n' "$red" "$bad" "$*" "$reset" >&2
    printf '  %sNothing was changed. Help: https://github.com/%s/issues%s\n' "$dim" "$REPO" "$reset" >&2
    exit 1
}
# One frame of the download bar: bytes so far, and the total when known.
bar() {
    awk -v done="$1" -v total="$2" 'BEGIN {
        mb = 1048576
        if (total > 0) {
            p = done / total
            if (p > 1) p = 1
            filled = int(p * 30 + 0.5)
            s = ""
            for (i = 0; i < 30; i++) s = s (i < filled ? "#" : " ")
            printf "\r    [%s] %3d%%  %.1f / %.1f MB", s, p * 100, done / mb, total / mb
        } else {
            printf "\r    %.1f MB", done / mb
        }
    }'
}
# The size of a file so far; 0 before it exists.
bytes() {
    if [ -f "$1" ]; then wc -c <"$1" | tr -d ' '; else echo 0; fi
}
# A path with the home directory shown as ~.
pretty() {
    case "$1" in
        "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;;
        *) printf '%s' "$1" ;;
    esac
}

say ""
say "  ${bold}leo${reset} ${dim}installer${reset}"
say "  ${dim}notes in your terminal, with AI and recording${reset}"
say ""

os=$(uname -s)
arch=$(uname -m)
case "$os-$arch" in
    Darwin-arm64) target=aarch64-apple-darwin system="macOS on Apple Silicon" ;;
    Darwin-x86_64) target=x86_64-apple-darwin system="macOS on Intel" ;;
    Linux-x86_64) target=x86_64-unknown-linux-gnu system="Linux on x86-64" ;;
    Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu system="Linux on ARM" ;;
    *) fail "there is no ready-made build for $os $arch. Build it from source instead: https://github.com/$REPO#1-install-leo" ;;
esac
step "Found $system"

tmp=$(mktemp -d)
pid=""
trap '[ -n "$pid" ] && kill "$pid" 2>/dev/null; rm -rf "$tmp"' EXIT
trap 'exit 130' INT TERM
archive="$tmp/leo.tar.gz"

if [ -n "${LEO_INSTALL_ARCHIVE:-}" ]; then
    cp "$LEO_INSTALL_ARCHIVE" "$archive"
    step "Using $(basename "$LEO_INSTALL_ARCHIVE")"
else
    command -v curl >/dev/null 2>&1 || fail "curl is needed to download leo"

    # Which release is the latest, read from where GitHub redirects, so the
    # download and its checksum are certain to come from the same release.
    tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null | sed -n 's|.*/tag/||p') || tag=""
    if [ -n "$tag" ]; then
        url="https://github.com/$REPO/releases/download/$tag/leo-$target.tar.gz"
        doing "Downloading leo $tag"
    else
        url="https://github.com/$REPO/releases/latest/download/leo-$target.tar.gz"
        doing "Downloading leo"
    fi

    if [ -t 1 ]; then
        # On a terminal, draw the bar: download in the background and watch
        # the file grow against the size GitHub reports.
        total=$(curl -fsSLI "$url" 2>/dev/null | awk 'tolower($1) == "content-length:" { n = $2 } END { print n + 0 }') || total=0
        curl -fsSL "$url" -o "$archive" &
        pid=$!
        while kill -0 "$pid" 2>/dev/null; do
            bar "$(bytes "$archive")" "$total"
            sleep 0.1
        done
        if ! wait "$pid"; then
            pid=""
            printf '\n'
            fail "could not download $url"
        fi
        pid=""
        got=$(bytes "$archive")
        bar "$got" "$got"
        printf '\n'
    else
        curl -fsSL "$url" -o "$archive" || fail "could not download $url"
        step "Downloaded $(bytes "$archive" | awk '{ printf "%.1f MB", $1 / 1048576 }')"
    fi

    # Check the download against the published checksum, when there is a tool
    # to compute one.
    if curl -fsSL "$url.sha256" -o "$archive.sha256" 2>/dev/null; then
        expected=$(cut -d ' ' -f 1 <"$archive.sha256")
        if command -v shasum >/dev/null 2>&1; then
            actual=$(shasum -a 256 "$archive" | cut -d ' ' -f 1)
        elif command -v sha256sum >/dev/null 2>&1; then
            actual=$(sha256sum "$archive" | cut -d ' ' -f 1)
        else
            actual=""
        fi
        if [ -z "$actual" ]; then
            say "  ${yellow}!${reset} No checksum tool here, so the download was not checked"
        elif [ "$expected" = "$actual" ]; then
            step "Checksum verified"
        else
            fail "the download is damaged (checksum mismatch); try again"
        fi
    fi
fi

tar -xzf "$archive" -C "$tmp" || fail "could not unpack the download"
[ -f "$tmp/leo" ] || fail "the download does not contain leo"

# The version already here, if any, to say whether this was an update.
previous=""
if [ -x "$BIN_DIR/leo" ]; then
    previous=$("$BIN_DIR/leo" --version 2>/dev/null | awk '{ print $2 }') || previous=""
fi

# Written beside the old copy, then renamed over it: copying onto a program
# that is running (leo update, or leo open in another terminal) rewrites it in
# place, which macOS then refuses to run. A rename swaps in a new file.
mkdir -p "$BIN_DIR"
cp "$tmp/leo" "$BIN_DIR/.leo.new"
chmod 755 "$BIN_DIR/.leo.new"
mv -f "$BIN_DIR/.leo.new" "$BIN_DIR/leo"
step "Installed to $(pretty "$BIN_DIR")/leo"
version=$("$BIN_DIR/leo" --version 2>/dev/null | awk '{ print $2 }') || version=""

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

if [ -n "${LEO_INSTALL_SKIP_PATH:-}" ]; then
    :
elif grep -qsF "$BIN_DIR" "$rc"; then
    step "Already on your PATH in $(pretty "$rc")"
else
    mkdir -p "$(dirname "$rc")"
    printf '\n# Added by the leo installer\n%s\n' "$line" >>"$rc"
    step "Added $(pretty "$BIN_DIR") to your PATH in $(pretty "$rc")"
fi

say ""
say "  ${dim}${rule}${reset}"
say ""
if [ -z "$previous" ]; then
    say "  ${green}${bold}leo ${version} is installed.${reset} Thank you for trying it!"
elif [ "$previous" = "$version" ]; then
    say "  ${green}${bold}leo is already up to date (${version}).${reset} Thank you for using it!"
else
    say "  ${green}${bold}leo is updated: ${previous} ${to} ${version}.${reset} Thank you for using it!"
fi
say ""
say "  ${bold}Get started${reset}"
say "    ${cyan}leo${reset}            open your notes"
say "    ${cyan}leo doctor${reset}     check AI, recording and backup, and store an API key"
say ""
say "  ${dim}Guide: https://github.com/$REPO#readme${reset}"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        if [ -z "${LEO_INSTALL_SKIP_PATH:-}" ]; then
            say ""
            say "  ${yellow}Open a new terminal first${reset} (or run: . $(pretty "$rc"))"
        fi
        ;;
esac
say ""
