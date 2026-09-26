# leo

> Notes for programmers — fast, local, plain-text, AI-powered.

`leo` is a note manager that lives in your terminal. Your notes are plain
Markdown files on your disk. It can also record a lecture and turn it into
structured notes while you type the points that matter, answer questions you
write inside a note, and back everything up to GitHub.

This page walks you through setting it up, one feature at a time. Only the
first two sections are needed to take notes; everything after that is
optional.

1. [Install leo](#1-install-leo)
2. [Your first notes](#2-your-first-notes)
3. [Set up the AI](#3-set-up-the-ai)
4. [Record a lecture](#4-record-a-lecture)
5. [Ask the AI about your notes](#5-ask-the-ai-about-your-notes)
6. [Back up to GitHub](#6-back-up-to-github)
7. [Read your notes on your phone](#7-read-your-notes-on-your-phone)
8. [Use leo from a shell](#8-use-leo-from-a-shell)
9. [Reference](#9-reference)
10. [Troubleshooting](#10-troubleshooting)

---

## 1. Install leo

On a Mac or Linux, paste this into a terminal:

```bash
curl -fsSL https://raw.githubusercontent.com/chewton2k/leo-cli/main/install.sh | bash
```

It works the same whichever shell you use (zsh, bash, fish), and `| sh` works
in place of `| bash`. It downloads the ready-made leo for your computer, checks
it against the published checksum, puts it in `~/.local/bin`, and adds that to
your PATH for every future terminal, in the file your shell reads at startup.

**If you'd rather read the script before running it:**

```bash
curl -fsSL -o install.sh https://raw.githubusercontent.com/chewton2k/leo-cli/main/install.sh
less install.sh      # have a look
bash install.sh
```

To install somewhere other than `~/.local/bin`, set `LEO_INSTALL_DIR`:

```bash
curl -fsSL https://raw.githubusercontent.com/chewton2k/leo-cli/main/install.sh | LEO_INSTALL_DIR="$HOME/bin" bash
```

Then open a new terminal and run:

```sh
leo doctor
```

`leo doctor` checks everything leo uses — your notes, the AI, recording and
backup — and for anything missing, gives the command that fixes it. If an AI
has no key yet, it offers to store one. Run it again any time something seems
off; inside the app, `/doctor` does the same.

If the download fails, there may be no ready-made build for your computer yet;
build it from source instead (below).

**To update leo**, run the same install command again. **To uninstall**, run
`leo uninstall`: it removes the program and the PATH line the installer added,
and leaves your notes, settings and keys where they are.

### Or build it from source

If there is no ready-made build for your computer, or you want to change leo,
you need Rust 1.88 or newer ([rustup.rs](https://rustup.rs), or
`brew install rust`) and git:

```sh
git clone https://github.com/chewton2k/leo-cli
cd leo-cli
cargo install --path .
```

That installs leo to `~/.cargo/bin`. **If your shell then says
`leo: command not found`**, that directory is not on your PATH. Typing
`export PATH=...` into the terminal only lasts until you close it, so add it to
the file your shell reads at startup (`echo $SHELL` shows which shell you have):

```sh
# zsh (the default on Macs):
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc

# bash on a Mac:
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bash_profile && source ~/.bash_profile

# bash on Linux:
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc && source ~/.bashrc
```

If it still says "command not found", check that leo was installed:
`ls ~/.cargo/bin/leo` should list a file. If it does not, run
`cargo install --path .` again and look for an error at the end.

To update a source install, `git pull` and run `cargo install --path . --force`.

---

## 2. Your first notes

Start the app:

```sh
leo
```

You get three panes — your directories, the notes in the current directory,
and the selected note:

```
┌ dirs ────────┬ notes (3) ──────────────┬ Rust ownership ──────────┐
│ cs130/       │   1 Graph traversals    │ ## Ownership             │
│ cs162/       │   2●Rust ownership      │ ☐ read the book          │
│              │   3 Midterm plan        │ ☑ write notes            │
└──────────────┴─────────────────────────┴──────────────────────────┘
   n new   e edit   r rename   m move   x tick   Space mark   f find   ? help
 /cs130                                              3 notes · 212 words
```

The first time you start leo, a **setup screen** lists the four things worth
turning on — AI for writing, AI for speech, recording, and backup to GitHub —
with whether each is done. Select one and press `Enter` to set it up right
there, or `Esc` to skip. After that, `/doctor` checks everything any time and
says how to fix what is missing. The sections below cover each step in more
detail.

**The line along the bottom always shows the keys that work where you are.**
If you forget anything, look there, or press `?` for the full list.

Try this:

1. **Make a directory.** Press `N`, type `cs130`, press `Enter`.
2. **Go into it.** Press `h` to move to the directories pane, `j`/`k` to select
   `cs130/`, then `Enter`.
3. **Write a note.** Press `n`. Your editor opens with a small header. That is
   whatever `$EDITOR` is set to, or **nano** if you have never chosen one: type,
   then `Ctrl-O`, `Enter` to save and `Ctrl-X` to close.

   ```markdown
   ---
   title: Lecture 1
   tags: exam, graphs
   ---
   - BFS explores level by level
   - [ ] review Dijkstra
   ```

   Fill in a title, optional tags, and the note. Save and close the editor, and
   the note appears in the list, already selected. An empty note is discarded.
4. **Tick a checkbox.** With the note selected, press `x` to tick its first open
   box. To tick a different one, press `l` to move into the note, `j`/`k` to
   pick the box, then `x`.
5. **Find something.** Press `f` and type. The search covers every note in every
   directory: titles, the text inside notes, and `#tags`. Each result shows the
   line that matched. Press `Enter` to keep the results, or `Esc` to clear the
   search and jump to the note you picked.
6. **Undo a mistake.** Press `u` to take back the last delete, move, or tick.
   A deleted note also waits in the trash for 30 days, even after you quit:
   `/trash` lists what is there and `/trash restore 1` brings one back to
   where it was.

Other everyday keys: `e` edits the selected note, `r` renames it, `m` moves it,
`D` deletes it (it asks first), `p` pins it to the top of its list (a syllabus,
say; `p` again unpins), and `Space` marks several notes so `D` and `m` act on
all of them at once.

### Commands with `/`

Anything that takes words goes on the command line. Press `/`: a menu lists
every command, narrowing as you type, and `Tab` completes names, directories and
tags. If you leave the note out, a command acts on the selected note (or on the
marked ones).

```
/new cs130/Lecture 4 #exam     a note in cs130, tagged exam, in one step
/rename Graph traversals       retitle the selected note
/mv cs162                      move the selected (or marked) notes
/mkdir cs130                   a directory here
/cd ..                         up a directory; /cd / for the top
/ask what is BFS?              ask a question across all your notes (section 5)
/trash                         deleted notes, kept 30 days; /trash restore 1
/doctor                        check that everything works (AI, recording, backup)
/sync                          back up now (section 6)
```

---

## 3. Set up the AI

The AI turns recordings into notes (section 4) and answers questions inside
notes (section 5). Everything else works without it.

leo uses two kinds of AI:

- **AI for writing:** turns a transcript into structured notes, and answers
  questions.
- **AI for speech:** turns audio into a transcript.

You can use free cloud services (easiest), models running on your own machine
(free, private, and offline), or a mix of both. Pick **Option A** or **Option
B**; you can add the other later.

### Option A: free cloud services (easiest)

1. **Get a key for writing, from OpenRouter.** Sign up at
   [openrouter.ai](https://openrouter.ai) and create a key under
   [Keys](https://openrouter.ai/keys). leo uses its free models by default.
2. **Get a key for speech, from Groq.** Sign up at
   [console.groq.com](https://console.groq.com) and create a key under
   [API Keys](https://console.groq.com/keys).
3. **Give the keys to leo.** Run:

   ```sh
   leo doctor
   ```

   At the end it asks which provider to store a key for. Type `openrouter`,
   paste the key (it is not shown as you type), then run `leo doctor` again for
   `groq`.

   You can do the same from inside the app instead: press `Ctrl-S`, select the
   provider, and press `Enter`.

4. **Check it.** Run `leo doctor` once more (or `/doctor` in the app). Under
   **AI**, both models should say `ok`, and `openrouter answers` and
   `groq answers` confirm the keys work.

### Option B: on your own machine (free and offline)

1. **Writing: Ollama.**

   ```sh
   brew install ollama
   ollama serve          # leave running, or open the Ollama app
   ollama pull qwen3:8b  # in another terminal; a few GB
   ```

2. **Speech: whisper.cpp.**

   ```sh
   brew install whisper-cpp
   mkdir -p ~/.leo/models
   curl -L -o ~/.leo/models/ggml-base.en.bin \
     https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
   ```

3. **Check it.** `leo doctor` should now show both as `ok` under **AI**.

### How leo picks a provider

Each kind of AI has a list of providers, tried in order: the first one that is
ready gets the request, and if it fails, leo moves on to the next. By default
writing tries Ollama, then OpenRouter; speech tries whisper.cpp, then Groq, then
Hugging Face. So with both options set up, leo uses your own machine when
Ollama is running and falls back to the cloud when it is not.

`Ctrl-S` shows both lists. A filled dot `●` means that provider would be used
right now. On a provider, `Enter` stores its key, adds it to its list, or sends
a small test request. `J`/`K` change the order.

---

## 4. Record a lecture

**You need:** SoX for recording (`brew install sox`), and the AI from section 3.

**Allow the microphone (macOS, first time only).** Open System Settings →
Privacy & Security → Microphone, turn it on for your terminal app (Terminal,
iTerm, Ghostty…), then quit and reopen the terminal.

To record:

1. Select the directory the note should go in.
2. Press `R` (or type `/listen`). The live transcript appears in the right-hand
   pane, updating every few seconds as the speaker talks.
3. **Type the points you care about** in the box under the transcript, and press
   `Enter` after each one. They are listed above the transcript as "Your
   points", with the time you typed them.
4. **Need a break?** Press `Ctrl-P` to pause and again to resume. Anything
   said while paused is cut out: it is never transcribed or sent anywhere.
5. Press `Esc` to stop. leo transcribes the whole recording again in one pass,
   writes the note, and selects it so you can read it straight away.

The finished note has:

- a title and a 2–3 sentence summary;
- a **Key points** section with every point you typed, in bold, each followed
  by what was said about it at that moment;
- sections for each topic, in the order they came up, with terms in bold where
  they were defined, and formulas and code in code blocks;
- extra explanation from the AI wherever the recording was patchy or the speaker
  was vague, so the notes make sense on their own;
- an **Action items** checklist, if tasks were mentioned.

Speech-recognition mistakes in subject terms ("breath first search") are
corrected. If you type points but nothing is heard, your points are still saved
as a note.

**Variations:**

```
/listen CS 101 Lecture 4     give the note your own title
/listen add                  add to the selected note instead of making a new one
/listen --screen             record your computer's audio (a video, a call)
```

**Recording computer audio** (`--screen`) needs a virtual audio device:

1. `brew install blackhole-2ch`
2. Open Audio MIDI Setup, add a **Multi-Output Device**, and tick both your
   speakers and BlackHole 2ch.
3. In Sound settings, set that Multi-Output Device as the output.

---

## 5. Ask the AI about your notes

Write a question on its own line, starting with `@leo`:

```markdown
## Graphs
- BFS explores level by level
@leo how is BFS different from DFS?
```

With the note selected, press `a` (or type `/ask`). The answer streams in under
your question, and the question stays in the note as a bold **Q:** line:

```markdown
**Q:** how is BFS different from DFS?

BFS visits nodes level by level using a queue; DFS goes as deep as it can…
```

Any `@leo` lines are also answered when you save a note from the editor.

### Ask all your notes

Type `/ask` followed by a question:

```
/ask what did we cover about graphs?
```

leo finds the notes most about it, in any directory, and the answer streams into
the preview, naming the note each fact came from in brackets, like
`[Graph traversals]`. If none of your notes mention it, leo says so rather than
guessing. `Esc` closes the answer. From a shell: `leo ask "what did we cover
about graphs?"`.

---

## 6. Back up to GitHub

1. Create an **empty** repository on GitHub (no README, no .gitignore). Make
   it **private** unless you want your notes public.
2. Run:

   ```sh
   leo sync
   ```

   Paste the repository's URL when asked. leo sets up git in your notes
   directory, connects it, and pushes.

From then on:

- every change is committed as you save;
- leo pushes when you quit the app;
- `leo sync` (or `/sync` in the app) backs up on demand: it pulls anything
  newer from GitHub first, then pushes.

**On another computer:** install leo, run `leo sync`, and paste the same
repository's URL. The notes already backed up come down, this computer's notes
go up, and from then on both stay in step. If the same note was edited on both,
leo keeps both versions' lines in it for you to tidy rather than losing either.

To push while you work instead of on quit, press `Ctrl-S` and change **when leo
backs up** on the backup row. You can also set up backup from that screen
instead of running `leo sync`.

---

## 7. Read your notes on your phone

```sh
leo serve
```

This prints a link and a QR code. Scan the code with a phone on the same Wi-Fi
to read, edit and search your notes in the browser.

The link carries an access token: anyone on your network who has it can edit
your notes, so only use this on networks you trust. Stop the server with
`Ctrl-C`.

---

## 8. Use leo from a shell

Every everyday action also works as a command, which is handy for scripts and
quick captures:

```sh
leo new "Quick thought" --body "Refactor auth" --tags todo
leo new "cs130/Lecture 4 #exam"     # opens your editor
leo list --tag todo
leo list cs130                      # one directory
leo search "refactor"               # shows the line that matched
leo view "Rust ownership"
leo edit 3f2a
leo delete 3f2a --force             # goes to the trash
leo trash                           # what was deleted; leo trash restore 1
leo pin "Syllabus"                  # keep it at the top of the list
leo ask 3f2a                        # answer that note's @leo lines
leo ask "what did we cover about graphs?"   # a question across all notes
leo sync                            # back up to GitHub
leo listen --title "Meeting notes"  # records until you press Enter
leo doctor                          # check everything, store an API key; exits 1 if anything is broken
leo uninstall                       # remove leo; your notes stay
```

A note can be named by its number in `leo list`, the start of its ID, or a
unique part of its title.

---

## 9. Reference

### Keys

| Key | What it does |
|-----|-------------|
| `j` / `k` | Move down / up (arrows work too) |
| `h` / `l` | Switch pane |
| `Enter` | Open a directory, or move into the note |
| `n` / `N` | New note / new directory |
| `e` | Edit the selected note |
| `r` / `m` | Rename / move it |
| `x` | Tick a checkbox |
| `D` | Delete (asks first); in the directories pane, the whole directory |
| `Space` | Mark notes, so `D` and `m` act on all of them |
| `u` | Undo the last delete, move or tick |
| `p` | Pin the note to the top of its list, or unpin it |
| `f` | Find: search every note (`Ctrl-F` too) |
| `t` | Left pane: directories or tags |
| `Tab` | Back to a recently visited note |
| `R` | Record |
| `a` | Answer the note's `@leo` questions |
| `/` | Command line |
| `Ctrl-S` | Profile: AI providers, keys, colour, backup |
| `?` / `q` | Help / quit |

While recording: type a point, `Enter` adds it, `Ctrl-P` pauses or resumes,
`Esc` stops and saves.

The mouse works too: click to focus or select, scroll with the wheel.

### Where things live

| Platform | Notes and settings |
|----------|--------------------|
| macOS | `~/Library/Application Support/leo/` |
| Linux | `~/.local/share/leo/` (notes), `~/.config/leo/` (settings) |
| Windows | `%APPDATA%\leo\` |

Each note is a Markdown file with a small header, and directories are real
directories. Settings are in `config.toml`; press `Ctrl-S` then `e` to open it.
API keys are never stored in that file: they are kept in a separate file only
your account can read.

### Environment variables

| Variable | What it does |
|----------|--------------|
| `LEO_HOME` | Keep notes, settings and keys in this one directory instead |
| `OPENROUTER_API_KEY`, `GROQ_API_KEY`, `HF_API_KEY`, … | A provider's key; takes precedence over a stored one |
| `LEO_CHAT_PROVIDER` / `LEO_TRANSCRIBE_PROVIDER` | Use only this provider for writing / speech |
| `LEO_CHAT_MODEL` | Override the model of the first writing provider |
| `LEO_USE_KEYCHAIN=1` | Store keys in the OS keychain instead of the key file |
| `LEO_SCREEN_DEVICE` | The audio device for `--screen` (default `BlackHole 2ch`) |

---

## 10. Troubleshooting

**"Not ready to record: microphone — recorded silence"**
The microphone is not being heard. Check that your terminal is allowed in
System Settings → Privacy & Security → Microphone (then restart the terminal).
On a MacBook, the built-in microphone is off while the lid is closed, so use an
external microphone or open the lid.

**A recording's notes stop mid-sentence**
The AI hit its length limit, and leo shows a warning saying so. Press `Ctrl-S`,
then `e`, and raise `max_tokens` for that provider (8192 is plenty for an hour
of lecture).

**A note is missing from the list**
If you deleted it, it is in the trash for 30 days: `/trash` (or `leo trash`)
lists it, and `/trash restore <number>` brings it back. Otherwise run
`leo doctor`. If a note file's header was edited and leo cannot read it,
the doctor names the file and the problem; leo never deletes such a file, so
fixing the header brings the note back.

**"No API key" or nothing happens when recording**
Run `leo doctor` (or `/doctor` in the app): it says which kind of AI is missing
and how to add it. Keys can also be added with `Ctrl-S`, then `Enter` on the
provider.

**`leo sync` fails**
Check that `git push` works from your terminal (a signed-in account or an SSH
key), and run `leo doctor`, which asks the repository whether it answers. If the push is
rejected, run `leo sync` again: it pulls first.

**Anything else**
Run `leo doctor` for a full health scan. It checks leo itself, reads every note
(and names any file it cannot read), tests each AI you use with one small
request, listens to the microphone, and asks your GitHub backup whether it
answers — then prints the fix for each problem it finds.

---

## Working on leo

Contributions are welcome: [CONTRIBUTING.md](CONTRIBUTING.md) covers setting up,
the tests, and what a change needs before it can be merged.

The code is a Cargo workspace. Each crate may only depend on the ones above it,
and the compiler enforces that:

| Crate | What it holds | Depends on |
|-------|---------------|------------|
| `crates/leo-core` | Notes, the on-disk store, git backup, the `/` command vocabulary and its handlers | — |
| `crates/leo-services` | AI providers and fallback chains, config and credentials, recording, capability checks | core |
| `crates/leo-tui` | The full-screen interface | core, services |
| `crates/leo-web` | `leo serve` | core |
| `leo` (the root) | `main.rs` and the `cli/` subcommands | all of them |

`cargo test` from the root runs everything: each crate's unit tests, the
full-screen app driven through a simulated terminal, and in `tests/`:

- `e2e.rs` — the real `leo` binary against a throwaway `LEO_HOME`: notes,
  search, `leo ask`, `leo doctor`, backup to a local git repository (including
  a second computer joining it), and `leo serve`;
- `install.rs` — `install.sh` run for real into a throwaway home directory;
- `wording.rs` — fails if any text a user can see names a command that is gone;
- `release.rs` — the scripts that pick the next version and stamp it into the
  build.

Nothing touches the network, your notes or your keychain. With SoX installed,
the audio tests run too; without it they skip themselves.

CI (`.github/workflows/ci.yml`) runs on every push: the tests on Linux and
macOS, with SoX installed so the audio tests run, plus `cargo fmt --check`,
`cargo clippy -D warnings`, and a check against the minimum Rust version, 1.88.

**Every push to `main` that passes CI and changes leo is released
automatically.** `.github/workflows/release.yml` picks the next version
(v0.2.1, v0.2.2, …), builds leo for Apple Silicon and Intel Macs and for x86
and ARM Linux, and publishes them with checksums as a GitHub release, which is
what the install command downloads. It also commits the new number to
`Cargo.toml` on `main`, so run `git pull` before your next push. A push that
only changes documentation, tests or workflows is not released. For a bigger
jump, raise `version` in `Cargo.toml` (say to `0.3.0`) and that becomes the
next release.

`LEO_HOME=/some/dir leo` keeps notes, settings and keys in that one directory,
which is handy for trying changes without touching your real notes.

## License

MIT
