#!/bin/sh
# Install leo: download the ready-made build for this computer and put it on
# your PATH for good.
#
#
# Settings (all optional):
#   LEO_INSTALL_DIR      where to put leo (default: ~/.local/bin)
#   LEO_INSTALL_ARCHIVE  install from this .tar.gz instead of downloading
set -eu

REPO="chewton2k/leo-cli"
BIN_DIR="${LEO_INSTALL_DIR:-$HOME/.local/bin}"

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
bytes() {
    if [ -f "$1" ]; then wc -c <"$1" | tr -d ' '; else echo 0; fi
}
fetch() {
    if [ -t 1 ]; then
        total=$(curl -fsSLI "$1" 2>/dev/null | awk 'tolower($1) == "content-length:" { n = $2 } END { print n + 0 }') || total=0
        curl -fsSL "$1" -o "$2" &
        pid=$!
        while kill -0 "$pid" 2>/dev/null; do
            bar "$(bytes "$2")" "$total"
            sleep 0.1
        done
        if ! wait "$pid"; then
            pid=""
            printf '\n'
            return 1
        fi
        pid=""
        got=$(bytes "$2")
        bar "$got" "$got"
        printf '\n'
    else
        curl -fsSL "$1" -o "$2" || return 1
    fi
}
sha256_of() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    fi
}
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
    MINGW* | MSYS* | CYGWIN*) fail "on Windows, install leo from PowerShell instead: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" ;;
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

    tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null | sed -n 's|.*/tag/||p') || tag=""
    if [ -n "$tag" ]; then
        url="https://github.com/$REPO/releases/download/$tag/leo-$target.tar.gz"
        doing "Downloading leo $tag"
    else
        url="https://github.com/$REPO/releases/latest/download/leo-$target.tar.gz"
        doing "Downloading leo"
    fi

    fetch "$url" "$archive" || fail "could not download $url"
    if [ ! -t 1 ]; then
        step "Downloaded $(bytes "$archive" | awk '{ printf "%.1f MB", $1 / 1048576 }')"
    fi

    # Check the download against the published checksum, when there is a tool
    # to compute one.
    if curl -fsSL "$url.sha256" -o "$archive.sha256" 2>/dev/null; then
        expected=$(cut -d ' ' -f 1 <"$archive.sha256")
        actual=$(sha256_of "$archive")
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

previous=""
if [ -x "$BIN_DIR/leo" ]; then
    previous=$("$BIN_DIR/leo" --version 2>/dev/null | awk '{ print $2 }') || previous=""
fi

mkdir -p "$BIN_DIR"
cp "$tmp/leo" "$BIN_DIR/.leo.new"
chmod 755 "$BIN_DIR/.leo.new"
mv -f "$BIN_DIR/.leo.new" "$BIN_DIR/leo"
step "Installed to $(pretty "$BIN_DIR")/leo"
version=$("$BIN_DIR/leo" --version 2>/dev/null | awk '{ print $2 }') || version=""

if [ -n "${LEO_HOME:-}" ]; then
    models="$LEO_HOME/models"
else
    models="$HOME/.leo/models"
fi
model_dir="$models/parakeet-tdt-0.6b-v3-int8"
model_url="${LEO_INSTALL_MODEL_URL:-https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78}"
model_files="${LEO_INSTALL_MODEL_MANIFEST:-encoder.int8.onnx=acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247 decoder.int8.onnx=179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e joiner.int8.onnx=3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3 tokens.txt=d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d}"
file_state() {
    if [ ! -s "$model_dir/$1" ]; then
        echo missing
        return
    fi
    have=$(sha256_of "$model_dir/$1")
    if [ -z "$have" ] || [ "$have" = "$2" ]; then
        echo ready
    else
        echo damaged
    fi
}
if [ -z "${LEO_INSTALL_NO_MODEL:-}" ]; then
    need=""
    damaged=""
    for pair in $model_files; do
        case "$(file_state "${pair%%=*}" "${pair#*=}")" in
            ready) ;;
            damaged)
                need="$need $pair"
                damaged=1
                ;;
            *) need="$need $pair" ;;
        esac
    done
    model_ready=""
    if [ -z "$need" ]; then
        model_ready=1
        step "Speech model ready in $(pretty "$model_dir")"
    elif ! command -v curl >/dev/null 2>&1; then
        say "  ${yellow}!${reset} No curl, so the speech model was not downloaded; /settings in leo can fetch it"
    else
        if [ -n "$damaged" ]; then
            doing "The speech model is damaged; downloading it again"
        else
            doing "Downloading the speech model (Parakeet, 670 MB, once)"
        fi
        mkdir -p "$model_dir"
        model_ready=1
        for pair in $need; do
            name=${pair%%=*}
            sha=${pair#*=}
            actual=""
            if fetch "$model_url/$name" "$model_dir/$name.part"; then
                actual=$(sha256_of "$model_dir/$name.part")
                if [ -z "$actual" ]; then
                    actual="$sha"
                fi
            fi
            if [ "$actual" = "$sha" ]; then
                mv -f "$model_dir/$name.part" "$model_dir/$name"
            else
                rm -f "$model_dir/$name.part"
                model_ready=""
                break
            fi
        done
        if [ -n "$model_ready" ]; then
            step "Speech model saved to $(pretty "$model_dir")"
        else
            say "  ${yellow}!${reset} Could not download the speech model; /settings in leo can fetch it later"
        fi
    fi
    rm -f "$models/ggml-base.en.bin"
fi

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
