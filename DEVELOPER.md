# leo — maintainer notes

Private notes for working on and releasing leo. This file is gitignored; the
public guide for contributors is CONTRIBUTING.md.

## Before every push

```sh
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

CI (`.github/workflows/ci.yml`) runs the same checks on Linux and macOS with SoX
installed, plus `cargo check` on Rust 1.88. Check a run from the terminal, since
`gh` is not installed here:

```sh
curl -s "https://api.github.com/repos/chewton2k/leo-cli/actions/runs?per_page=3" \
  | python3 -c "import json,sys; [print(r['name'], r['head_sha'][:7], r['status'], r['conclusion']) for r in json.load(sys.stdin)['workflow_runs']]"
```

The repository moved to `chewton2k/leo-cli`; `origin` already points there.

## Releasing

Automatic. Push to `main`; when every CI job passes (tests on Linux and macOS,
lint, MSRV), `ci.yml` calls `release.yml`, which:

1. runs `scripts/next-version.sh`: the last `v*` tag with patch + 1, or
   `Cargo.toml`'s version if that is newer, or nothing when `src/`, `crates/`
   and `Cargo.*` are unchanged since the last tag (docs-only pushes);
2. sets it in `Cargo.toml` and `Cargo.lock` with `scripts/set-version.sh` and
   commits that to `main` as `release: vX.Y.Z` (github-actions[bot]; its
   token does not trigger another CI run). If `main` moved on meanwhile, it
   stops and the newer push's run releases instead;
3. builds, from that commit, `aarch64-apple-darwin`, `x86_64-apple-darwin`,
   `x86_64-unknown-linux-gnu` (Ubuntu 22.04, older glibc) and
   `aarch64-unknown-linux-gnu`;
4. publishes release `vX.Y.Z`, tagging the version commit, with a `.tar.gz` and
   `.sha256` per platform, `install.sh`, and notes listing the commit subjects
   since the previous release.

**After a push that releases, `git pull` before pushing again**: `main` has the
bot's version commit. For a minor or major release, set `version` in
`Cargo.toml` (e.g. `0.3.0`) in the commit. To re-run a release by hand: Actions tab → Release → Run workflow.

Check the real installer against a new release, in a throwaway home:

```sh
T=$(mktemp -d); mkdir -p $T/home
env -i HOME=$T/home SHELL=/bin/zsh PATH=/usr/bin:/bin \
  sh -c 'curl -fsSL https://raw.githubusercontent.com/chewton2k/leo-cli/main/install.sh | sh'
$T/home/.local/bin/leo --version
```

Update your own copy: `cargo install --path . --force --locked`, or the curl
install (which gets the released version number).

**A broken release:** delete the release and its tag on GitHub (and locally with
`git tag -d vX.Y.Z`), fix the problem, then release a new patch version. Don't
reuse a version number: people may have already downloaded it.

`install.sh` is served from `main` on raw.githubusercontent.com, so a change to
it reaches users as soon as it is pushed, not when a release is made.

## Tests that only run by hand

These reach the network, so they are `#[ignore]`d:

```sh
# HTTPS through rustls, against a public endpoint (no key needed).
cargo test -p leo-services tls -- --ignored

# Reading the latest release from GitHub's redirect.
cargo test -p leo-services latest -- --ignored

# Live transcription end to end, replaying a WAV instead of the microphone.
# Uses your transcription provider and key.
say -o /tmp/speech.aiff "Breadth first search explores a graph level by level"
sox /tmp/speech.aiff -r 16000 -c 1 -b 16 /tmp/speech.wav
LEO_FAKE_AUDIO=/tmp/speech.wav cargo test live_streams -- --ignored --nocapture
```

`LEO_FAKE_AUDIO=<wav>` also works on a normal run: `R` in the app then replays
the file at real-time pace instead of recording from the microphone.

## Useful for debugging

- `LEO_HOME=/tmp/leo-dev leo` gives a separate world of notes, config and keys.
- `leo doctor` shows everything leo depends on. It listens to the microphone
  for half a second and sends one small request to each AI in use.
- `LEO_CHAT_PROVIDER=openrouter` or `LEO_TRANSCRIBE_PROVIDER=groq` pins a
  single provider, bypassing the fallback chain.
- In the app, warnings from lower layers appear on the status line (via
  `leo_core::diag`) rather than being printed, which would corrupt the screen.
- My own data: `~/Library/Application Support/leo/` (notes, `config.toml`,
  `credentials.json`). `config.toml.before-8192` is the backup from raising
  openrouter's `max_tokens`; delete it once you're happy.
- With the lid closed, a MacBook's built-in microphone is off, so recording hears
  nothing. Use an external mic.

Never run against GitHub or Cloudflare in CI; do these by hand after a
release that touches them (`brew install gh cloudflared`, `gh auth login`):

- `LEO_HOME=/tmp/leo-dev leo sync github leo-notes-test`: makes a private
  repo on your account (delete it after), and running it again from a second
  `LEO_HOME` joins it.
- `leo serve --anywhere`: open the trycloudflare link on mobile data. Check
  the address bar loses `?token=`, and `leo serve --new-token` locks out the
  old link.

The e2e suite covers both with stand-ins: a fake `gh` over local bare repos,
and a fake `cloudflared` that prints a trycloudflare address.

## Pitfalls learned the hard way

- **Never write a note file except through `Store::save`.** It checks the file
  still has the bytes leo saw, which is what keeps Obsidian's and the phone's
  edits safe.
- **Never change `stable_id`.** Headerless notes' ids come from it; a golden
  test guards the values.
- **Never slice a `String` at a byte offset computed from a length.** Korean
  text crashed live transcription that way (v0.2.5). Use char indices or
  `str::get`.
- **Unit tests must not open the microphone.** `rec` hangs when no input device
  exists (lid closed), which hung the whole suite.
- To try Obsidian for real: `LEO_HOME=/tmp/leo-obsidian leo new "Hi" --body x`
  then `LEO_HOME=/tmp/leo-obsidian leo obsidian`.

- **`Store::save` reconciles the notes directory with memory.** Any `.md` file
  it did not load gets deleted, except files listed in `Store::unreadable`. Keep
  that exception if you touch saving.
- **Never read the real keychain in tests.** An unsigned test binary blocks on a
  permission dialog nobody can click. Pass a `&dyn SecretStore` and use
  `MemoryStore`.
- **Never resolve DNS in health checks.** `to_socket_addrs` has no timeout;
  `health::port_open` only connects to literal addresses and localhost.
- **Git in the notes repository:** always pull with `--no-rebase` and say so
  explicitly. Users' own git settings differ, and newer git refuses to combine
  diverged histories unless told how. `directories.json` is deliberately not
  tracked; notes merge by union (`.gitattributes`).
- **A provider's `max_tokens` in config.toml replaces what leo asks for**, it
  does not cap it. Raising leo's own constants does nothing for a provider with
  its own setting.
- **The command line is `/`, search is `f`.** Anything user-facing that names
  a command must name a real one, or `tests/wording.rs` fails.
- **Two programs, one notes folder.** The app, `leo serve` and a shell can all
  write at once. `Store.known` keeps a save from trashing notes it never had,
  and the web server reloads from disk per request. Keep both if you touch
  saving or the server.
- **Don't `cp` over a running binary on macOS**: the new copy gets killed.
  install.sh writes `.leo.new` and renames it; keep it that way.
- **After a release, `git pull`.** CI commits `release: vX.Y.Z` to main.
- **No code comments.** Reasoning goes in commit messages.
- **Commits:** no `Co-authored-by` lines; conventional type prefix; the body
  explains why.

## Ideas not done yet

- Resolve `[[wikilinks]]` by title in the app and on the phone page.
- An Obsidian plugin inside an existing vault.

- A Homebrew tap (`brew install chewton2k/tap/leo`). It needs a second
  repository, `homebrew-tap`, and a token for the release workflow to update the
  formula.
- The app picks up notes added elsewhere only on `Ctrl-R`; it could reload when
  the notes folder changes.
- A fixed `--anywhere` address (a named Cloudflare tunnel) instead of a new
  one each run. Needs a Cloudflare account and a domain.
- Keep the audio file when a recording's transcription fails, so it can be
  retried.
- Flashcards or a quiz generated from a note.
- Tab completion for `leo` commands in the shell.
- Quick capture from the shell (`leo add "..."` to an Inbox note).
- The same `leo ask` across notes in the web UI.
- A Windows build (`install.sh` is Mac and Linux only).
- Two "leo manual" notes appear after a second computer joins a backup (each
  computer installed its own). Harmless, but could be merged.
