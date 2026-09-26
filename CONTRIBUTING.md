# Contributing to leo

Thanks for helping. This page covers getting the code running, how the project
is laid out, and what a change needs before it can be merged.

## Getting set up

You need Rust 1.88 or newer ([rustup.rs](https://rustup.rs)) and git. SoX
(`brew install sox`, `apt install sox`) is optional: without it the audio tests
skip themselves.

```sh
git clone https://github.com/chewton2k/leo-cli
cd leo-cli
cargo test
```

To try your changes without touching your real notes, point leo at a scratch
directory. `LEO_HOME` keeps notes, settings and keys all in one place:

```sh
LEO_HOME=/tmp/leo-dev cargo run
LEO_HOME=/tmp/leo-dev cargo run -- doctor
```

## How the code is organised

leo is a Cargo workspace. Each crate may only depend on the ones listed before
it, and the compiler enforces this, so please keep it that way rather than
reaching upward:

| Crate | Holds |
|-------|-------|
| `crates/leo-core` | Notes, the on-disk store and undo, git backup, and the command vocabulary (`action/`): parsing, handlers, note resolution. No terminal and no network. |
| `crates/leo-services` | AI providers and their fallback chains, prompts, config and credentials, recording, and health checks. |
| `crates/leo-tui` | The full-screen app. |
| `crates/leo-web` | `leo serve`. |
| root `leo` | `main.rs` and `src/cli/`, the command-line subcommands. |

A few ideas run through all of it:

- **One vocabulary.** The `/` line, the keys and the CLI all turn input into an
  `action::Action`, and the same handlers apply it. Add a command once, in
  `crates/leo-core/src/action/`, not separately in each front end.
- **Handlers don't touch the terminal.** When a step needs the terminal or the
  network — an editor, a confirmation, a recording, an AI call — the handler
  returns an `Effect` and the front end performs it. That is what keeps the
  handlers testable.
- **One source for commands.** `VERBS` in `action/parse.rs` holds each command's
  name, usage and summary; help, the `/` menu, completion and usage errors are
  all generated from it. A command that is removed goes in `RETIRED`, with what
  replaced it, so typing the old name explains itself.

## Tests

Every change comes with tests, and a bug fix comes with a test that failed
before the fix. Write the test first where you can.

```sh
cargo test                                        # everything
cargo test -p leo-core                            # one crate
cargo test --test e2e                             # end-to-end only
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

What each part covers:

- **Unit tests** sit beside the code in each crate.
- **The app** (`crates/leo-tui/src/tests.rs`) is driven key by key through a
  simulated terminal, and checks what is drawn and what reaches disk.
- **`tests/e2e.rs`** runs the real `leo` binary against a throwaway `LEO_HOME`,
  with a cleared environment and a PATH of system tools plus git.
- **`tests/install.rs`** runs `install.sh` into a throwaway home directory.
- **`tests/release.rs`** checks the scripts that pick and set the next
  version.
- **`tests/wording.rs`** fails if any text a user can see names a command that no
  longer exists. If you rename or remove a command, update the text it points
  at, or add the old name to its list.

Tests must never reach the network, your real notes, or the OS keychain. AI
calls go through the `action::Ai` trait, which has a test double. For
credentials, use `MemoryStore`, which other crates get through the
`test-support` feature of `leo-services`. Two tests that do reach the network
are marked `#[ignore]` and are only run by hand.

## Style

- Run `cargo fmt`. CI rejects unformatted code, and clippy warnings are errors.
- Don't add code comments. Put the reasoning in the commit message instead.
- Use user-facing words in messages. A message that tells someone what to do
  should name a command or key that exists, and say it the way the help does.

## Commits and pull requests

Commit messages start with a type — `feat`, `fix`, `refactor`, `test`, `docs`,
`style`, `ci` or `release` — and a short summary in plain words:

```
fix: saving a note deleted any note file leo could not read

A note file with a header leo could not parse was skipped on load, and the
next save deleted every file it had not loaded. ...
```

The body says what was wrong or missing and why the change fixes it. Keep each
commit to one change.

Before opening a pull request:

- [ ] `cargo test` passes, including any new tests for your change
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `cargo fmt --all` has been run
- [ ] If the change is something a user would notice: the README, the help
      screen (`crates/leo-tui/src/view/help.rs`) and the manual note
      (`crates/leo-core/src/manual.rs`) say so

CI runs the tests on Linux and macOS, formatting, clippy, and a build on the
minimum supported Rust version (1.88).

## Releases

Nobody makes a release by hand. When a push to `main` passes CI and changes
anything under `src/`, `crates/` or `Cargo.*`, the release workflow publishes
it as the next patch version, and the install command picks it up. The
workflow commits the new number to `Cargo.toml` on `main` (`release: vX.Y.Z`),
so pull before pushing again. For a minor
or major release, raise `version` in `Cargo.toml` in your change; that version
is used instead. `scripts/next-version.sh` holds the rule and
`tests/release.rs` tests it.

## Reporting bugs

Open an issue with what you did, what you expected, and what happened. The
output of `leo doctor` helps a lot. It checks leo, the notes, the AI, recording
and backup, and never includes your API keys.
