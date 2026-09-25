# leo

> Notes for programmers — fast, local, plain-text, AI-powered.

`leo` is a note manager that lives in your terminal. Your notes are Markdown
files on disk. It can also record a lecture and turn it into notes while you
type the points that matter, answer questions written inside a note, and back
everything up to GitHub.

## Install

```sh
git clone https://github.com/you/leo
cd leo
cargo install --path .
leo setup
```

`leo setup` tells you what works on this machine, what is missing and the
command that installs it, and offers to store an API key. Run it again whenever
something seems off.

If `leo` is not found after installing, add Cargo's bin directory to your PATH:
`export PATH="$HOME/.cargo/bin:$PATH"` in `~/.zshrc` or `~/.bashrc`.

Reinstall after changes with `cargo install --path . --force`. Uninstalling
(`cargo uninstall leo`) never touches your notes.

## Using it

Run `leo`. The window has three panes: directories, your notes, and the selected
note. **The bottom line always shows the keys that work where you are**, and `?`
opens the full reference.

```
┌ dirs ────────┬ notes (3) ──────────────┬ Rust ownership ──────────┐
│ cs130/       │   1 Graph traversals    │ ## Ownership             │
│ cs162/       │   2●Rust ownership      │ ☐ read the book          │
│              │   3 Midterm plan        │ ☑ write notes            │
└──────────────┴─────────────────────────┴──────────────────────────┘
   n new   e edit   r rename   m move   x tick   Space mark   / find   ? help
 /cs130                                              3 notes · 212 words
```

### The keys you need

| Key | What it does |
|-----|-------------|
| `j` / `k`, `h` / `l` | Move; switch pane (arrows work too) |
| `n` | New note here, in `$EDITOR` |
| `e` | Edit the selected note |
| `r` / `m` | Rename it / move it to another directory |
| `x` | Tick its first open checkbox. In the preview, `j`/`k` pick a box first |
| `D` | Delete it (asks). In the directories pane, deletes the directory |
| `Space` | Mark notes; `D` and `m` then act on all of them |
| `u` | Undo the last delete, move or tick |
| `/` | Search every note: titles, bodies, and `#tags`. `Esc` clears |
| `N` | New directory |
| `R` | Record a note by talking (see below) |
| `a` | Ask AI: answer the note's `@leo` lines |
| `t` | Left pane: directories or tags |
| `Tab` | Back to a recently visited note |
| `Ctrl-S` | Your profile: AI providers and keys, colour, backup |
| `:` | Command line — a menu lists every command as you type |
| `?` / `q` | Help / quit |

The mouse works too: click to focus or select, scroll with the wheel.

### The `:` line

Most things are keys; the `:` line is for anything that takes words. Leave the
note out and a command means the selected one (or the marked ones).

```
:new cs130/Lecture 4 #exam     a note in cs130, tagged exam
:rename Graph traversals       retitle the selected note
:mv cs162                      move the selected (or marked) notes
:mkdir cs130                   a directory here
:cd ..                         up a directory; / for the top
:sync                          back up now
```

`Tab` completes commands, directories, note titles and tags. Typing a command
that was removed tells you what replaced it.

## AI features

AI works with no keys at all if you run models locally
(`brew install ollama whisper-cpp && ollama pull qwen3:8b`). Otherwise
`leo setup` stores a key for a free cloud provider such as OpenRouter or Groq.
Everything except recording and `@leo` works without AI.

### Recording, with your own notes

Press `R` (or `:listen`). The preview fills with bullets as you talk. **While it
records, type the points you care about and press `Enter` after each one.** They
show up as "Your points", and the finished note opens with a **Key points**
section: every point you typed, in bold, with what was said about it around the
time you typed it. `Tab` shows the raw transcript, `Esc` stops and saves.

```
:listen CS 101 Lecture     a title of your own
:listen add                append to the selected note
:listen --screen           record system audio instead of the microphone
```

Needs SoX (`brew install sox`) for recording.

### Questions inside a note

Write `@leo <question>` on its own line, then press `a`. The line is replaced
with the answer, which streams in as it arrives. Saving a note from the editor
does the same for any `@leo` lines in it.

### Providers

`Ctrl-S` lists the AI used for writing and for speech, each tried in order until
one works. A filled dot means that provider would be used right now. `Enter` on
a provider does what it needs: stores its key, adds it, or tests it. Adding a
provider of your own is a few lines in `config.toml`, which `e` opens from that
screen; the comments in the file show how.

## Backup

```sh
leo sync
```

The first time, it asks for the URL of an empty GitHub repository and sets
everything up. After that, every save is committed, leo pushes when you quit, and
`leo sync` (or `:sync`) backs up on demand by pulling, then pushing. `Ctrl-S` can
make it push while you work instead.

## From a shell

```sh
leo new "Quick thought" --body "Refactor auth" --tags todo
leo new "cs130/Lecture 4 #exam"     # opens $EDITOR
leo list --tag todo
leo search "refactor"
leo edit 3f2a
leo delete 3f2a --force
leo ask 3f2a
leo serve                           # read and edit from your phone
```

A note can be named by its number in `leo list`, an ID prefix, or a unique part
of its title.

`leo serve` prints a link and a QR code that carry an access token. Anyone on
your network with that link can edit your notes, so use it on networks you
trust.

## Where things live

Notes are Markdown files with a small YAML header, one per note, with
directories mirrored on disk:

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/leo/` |
| Linux | `~/.local/share/leo/` |
| Windows | `%APPDATA%\leo\` |

Settings are in `config.toml` next to them. API keys are never in that file.
They live in a store only your account can read, or in environment variables,
which take precedence.

## Working on leo

The code is a Cargo workspace. Each crate may only depend on the ones above it,
and the compiler enforces that:

| Crate | What it holds | Depends on |
|-------|---------------|------------|
| `crates/leo-core` | Notes, the on-disk store, git backup, the `:` command vocabulary and its handlers | — |
| `crates/leo-services` | AI providers and fallback chains, config and credentials, recording, capability checks | core |
| `crates/leo-tui` | The full-screen interface | core, services |
| `crates/leo-web` | `leo serve` | core |
| `leo` (the root) | `main.rs` and the `cli/` subcommands | all of them |

`cargo test` from the root runs every crate's tests. Nothing in them touches the
network or the real keychain: AI calls go through a trait with a test double, and
`leo-services` has a `test-support` feature with an in-memory credential store.

## License

MIT
