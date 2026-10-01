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

The installer also downloads leo's speech model (NVIDIA Parakeet, 670 MB, into
`~/.leo/models`) and checks every file against its published checksum, so
recording works with nothing else to set up.

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

**To update leo**, run `leo update`. It installs the new version if there is
one, and makes sure the speech model is there: each file is checked against
its checksum and downloaded only if it is missing or damaged, and the old
`base.en` model from earlier versions is removed. leo checks for a new
version once a day and says so on the bottom line when there is one
(`LEO_NO_UPDATE_CHECK=1` turns that off).

**To uninstall**, run `leo uninstall`: it removes the program, the PATH line the
installer added, and everything else leo made (settings, stored API keys, the
`leo serve` link, caches, and the speech model in `~/.leo/models`), and leaves
your notes folder exactly as it is. It lists what it will remove and asks
first.

### Or build it from source

If there is no ready-made build for your computer, or you want to change leo,
you need Rust 1.88 or newer ([rustup.rs](https://rustup.rs), or
`brew install rust`) and git. The first build downloads the speech engine's
ready-made library (sherpa-onnx) from GitHub:

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

A source install gets the speech model the first time leo starts: it downloads
in the background (the bottom line says when it is ready), so there is nothing
to do.

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
┌ dirs ────────┬ notes (3) ──────────────┬ Rust ownership · edited … ┐
│ cs130/       │   1 Graph traversals 2h │ Ownership                 │
│ cs162/       │   2 Rust ownership   5m │ ☐ read the book           │
│              │   3 Midterm plan  Sep 3 │ ☑ write notes             │
└──────────────┴─────────────────────────┴───────────────────────────┘
   Enter write   n new   / find or command   D delete   ? help
 /cs130                                              3 notes · 212 words
```

Notes work straight away; nothing needs setting up first. When you first use
something that does — recording, asking a question, backing up — leo shows
what it needs right then, and `Enter` on a step sets it up. `/doctor` checks
everything any time and says how to fix what is missing. The sections below
cover each part in more detail.

**The line along the bottom always shows the keys that work where you are.**
If you forget anything, look there, or press `?` for the full list.

Five keys are all you need:

| Key | What it does |
|-----|-------------|
| `Enter` | Write in the selected note. `Esc` when you are done; it saves as you go |
| `n` | A new note: type a title, `Enter`, and start writing |
| `/` | Find a note, or pick a command from the list |
| `D` | Delete. `u` brings it back |
| `?` | Every key and command |

Try this:

1. **Write a note.** Press `n`, type `Lecture 1 #exam`, press `Enter`. The note
   opens for writing, tagged `exam`. Type as you would anywhere else:

   ```markdown
   ## Graphs
   - BFS explores level by level
   - [ ] review Dijkstra
   ```

   Every line shows formatted, and the line with the cursor shows its Markdown.
   `Enter` after a list item starts the next one (a checklist too), `Tab`
   indents, and `Ctrl-Z` undoes. Press `Esc` when you are done. There is nothing
   to save: leo saves a moment after you stop typing.
2. **Tick a checkbox.** Click the box, or press `x` in the list to tick the
   note's first open box.
3. **Find something.** Press `/` and type. The search covers every note in every
   folder: titles, the text inside notes, and `#tags`. Each result shows the
   line that matched. Press `Enter` to keep the results, or `Esc` to clear the
   search and jump to the note you picked.
4. **Make a folder.** Press `N`, type `cs130`, press `Enter`. `h` moves to the
   folders pane, `j`/`k` select, and `Enter` goes in.
5. **Undo a mistake.** Press `u` to take back the last delete, move, or tick.
   A deleted note also waits in the trash for 30 days, even after you quit:
   `/trash` lists what is there and `/trash restore 1` brings one back to
   where it was.

More keys, all listed under `?`: `e` opens the note in your own editor
(`$EDITOR`, or nano) instead, `r` renames it, `m` moves it, `p` pins it to the
top of its list (a syllabus, say; `p` again unpins), and `Space` marks several
notes so `D` and `m` act on all of them at once. Each note in the list shows
when it was last edited, and the note's title bar gives the day and time.

### Finding and commands with `/`

`/` is one line for both. Type words and the list narrows to the notes that
match. Start with a command instead and a menu shows what it does; `Tab`
completes names, folders and tags. If you leave the note out, a command acts on
the selected note (or on the marked ones).

```
/new cs130/Lecture 4 #exam     a note in folder cs130 (if it exists), tagged exam
/rename Graph traversals       retitle the selected note
/mv cs162                      move the selected (or marked) notes
/mkdir cs130                   a folder here
/cd ..                         up a folder; /cd / for the top
/ask what is BFS?              a question answered from your notes (section 5)
/trash                         deleted notes, kept 30 days; /trash restore 1
/doctor                        check that everything works (AI, recording, backup)
/backup                        back up now (section 6)
```

---

## 3. Set up the AI

The AI turns recordings into notes (section 4) and answers questions inside
notes (section 5). Everything else works without it.

leo uses two kinds of AI:

- **AI for writing:** turns a transcript into structured notes, and answers
  questions.
- **AI for speech:** turns audio into a transcript.

There are two ways to set it up. Pick one; you can switch any time, and you
can mix them (for example, speech on your computer and writing in the cloud).

- **Option A: your own computer.** Free, private, works offline. Nothing to sign
  up for.
- **Option B: one cloud account.** OpenAI, Anthropic, Gemini, xAI or
  OpenRouter. One key covers both writing and speech (Anthropic and OpenRouter
  only write, so pair them with your computer or another cloud for speech).
  OpenRouter has free models, so it can cost nothing.

### Settings, in one minute

Everything happens in Settings: type `/settings` inside leo (press `/`, type
`settings`, then `Enter`). The top of the page
looks like this:

```
 AI
     writing               ● Anthropic
     writing model         claude-sonnet-5-5
     Anthropic key         stored
     speech                ● this Mac (Parakeet)
```

| Key | What it does |
|-----|--------------|
| `↑` `↓` (or `j` `k`) | Move between rows |
| `Enter` or `→` | Next choice on this row: the next provider, or the next model |
| `←` | Previous choice |
| `Enter` on a **key** row | Asks for the key; it is never shown as you type |
| `x` on a **key** row | Removes the key |
| `Esc` | Close Settings |

A filled dot `●` means that choice is ready to use; a hollow one `○` means
something is missing, and the row under it says what.

### Option A: your own computer (free and offline)

1. **Speech is built in.** leo has its own speech-to-text engine and model,
   NVIDIA's Parakeet (670 MB), which is among the most accurate open models for
   English and adds punctuation and capitals. There is nothing to install or
   pick. The installer downloads it into `~/.leo/models`, and `leo update`
   checks it every time: if a file is missing, or its checksum does not match
   (a damaged or half-finished download), that file is downloaded again. If it
   is ever missing when leo starts, leo downloads it in the background. In
   Settings, **speech** says `● this Mac (Parakeet)`.

   It is built to leave your computer usable while it works: it uses at most
   half your processor cores (never more than four), runs at background
   priority so the apps you are using come first, and frees its memory a
   minute after it was last needed. Two minutes of speech take about ten
   seconds on a recent Mac.

2. **Writing uses Ollama.**

   ```sh
   brew install ollama
   ```

   Open the Ollama app (or run `ollama serve`). In Settings, **writing** should
   say `this Mac (Ollama)`. If **writing model** says `none yet`, press `Enter`
   on it to download `qwen3:8b` (about 5 GB). Every model you have pulled with
   `ollama pull` appears there too; `Enter` goes through them.

3. **Check it.** `leo doctor` (or `/doctor` in the app) should show both as
   `ok` under **AI**.

### Option B: a cloud account

1. **Get a key** from one of:
   [OpenAI](https://platform.openai.com/api-keys),
   [Anthropic](https://platform.claude.com/settings/keys),
   [Gemini](https://aistudio.google.com/apikey),
   [xAI](https://console.x.ai) or
   [OpenRouter](https://openrouter.ai/keys). These charge per use, except
   Gemini's free tier and OpenRouter's free models.
2. **Choose it.** In Settings, press `Enter` on **writing** until it shows your
   provider. Do the same on **speech** (OpenAI, Gemini or xAI), or keep speech
   on your computer.
3. **Add the key.** `Enter` on the **key** row. One key serves writing and
   speech for the same provider.
4. **Pick a model** (optional). `Enter` on **writing model** goes through the
   models leo supports, cheapest first, each with its price in parentheses,
   for example `gpt-6-luna ($0.10 in, $0.50 out per 1M tokens)`. Turning an
   hour of lecture into notes uses roughly 15,000 tokens in and 3,000 out, so on
   the cheaper models it costs well under a cent.

   | Provider | Writing models, cheapest first (per 1M tokens in / out) |
   |----------|------------------------------------------|
   | OpenAI | `gpt-5-nano` $0.05 / $0.40, `gpt-6-luna` $0.10 / $0.50 (default), `gpt-5.4-nano` $0.20 / $1.25, `gpt-5-mini` $0.25 / $2, `gpt-5.4-mini` $0.75 / $4.50, `gpt-6.1-sol` $2 / $10, `gpt-5.4` $2.50 / $15, `gpt-5.5` $5 / $30, `gpt-6-astra` $10 / $50 |
   | Anthropic | `claude-haiku-4-5` $1 / $5, `claude-sonnet-5-5` $2 / $10 (default), `claude-sonnet-5` $2 / $10, `claude-opus-5-5` $4 / $20, `claude-opus-5` $5 / $25 |
   | Gemini | Free tier on every Flash model, then: `gemini-3.1-flash-lite` $0.25 / $1.50, `gemini-3.5-flash-lite` $0.30 / $2.50, `gemini-3.8-flash` $0.75 / $3.75 (default), `gemini-3.5-flash` $1.50 / $9; `gemini-3.1-pro-preview` $2 / $12 (no free tier) |
   | xAI | `grok-4.3` $1.25 / $2.50, `grok-4.5`, `grok-4.6`, `grok-4.7` (default) $2 / $6 |
   | OpenRouter | Free, with daily limits: `openrouter/free` (default; picks whichever free model is available), `qwen/qwen3.8-27b:free`, `google/gemma-4-31b-it:free`, `nvidia/nemotron-3-super-120b-a12b:free`. Paid: `deepseek/deepseek-v4-flash` $0.08 / $0.16, `openai/gpt-6-luna` $0.10 / $0.50, `google/gemini-3.1-flash-lite` $0.25 / $1.50, `anthropic/claude-haiku-4.5` $1 / $5, `anthropic/claude-sonnet-5.5` $2 / $10 |

   | Provider | Speech models |
   |----------|---------------|
   | OpenAI | `gpt-4o-mini-transcribe` $0.18/hour, `gpt-transcribe` $0.27/hour (default), `whisper-1` $0.36/hour |
   | Gemini | `gemini-3.8-flash` (free tier, default), `gemini-3.1-flash-lite` (free tier, then about $0.06/hour) |
   | xAI | `grok-voice-transcribe-2.0` (default), `grok-voice-transcribe-1.0`, both $0.10/hour |

   **Free:** your own computer costs nothing. In the cloud, Gemini's free tier
   covers writing and speech on its Flash models, with daily limits (and Google
   may use free-tier content to improve its products), and OpenRouter's free
   models cover writing, also with daily limits. Prices are as of
   September 2026 and change; check the provider's pricing page.

5. **Check it.** `leo doctor` should say `ok` under **AI**, and that the
   provider answers.

### Upgrading from an older leo

leo used to know eighteen providers. It now keeps six: Ollama, OpenAI,
Anthropic, Gemini, xAI and OpenRouter. The first time a new leo starts it tidies
`config.toml`: providers that were removed (Groq, Hugging Face, Mistral, and so
on) are dropped, and if one of them was your choice, that task
goes back to your computer. Open Settings to pick again. A provider you added
yourself under a new name is left alone.

---

## 4. Record a lecture

**You need:** SoX for recording (`brew install sox`), and the AI from section 3.

**Allow the microphone (macOS, first time only).** Open System Settings →
Privacy & Security → Microphone, turn it on for your terminal app (Terminal,
iTerm, Ghostty…), then quit and reopen the terminal.

To record:

1. Select the directory the note should go in.
2. Press `R` (or type `/record`). The live transcript appears in the right-hand
   pane, updating every few seconds as the speaker talks.
3. **Type the points you care about** in the box under the transcript, and press
   `Enter` after each one. They are listed above the transcript as "Your
   points", with the time you typed them.
4. **Need a break?** Press `Ctrl-P` to pause and again to resume. Anything
   said while paused is cut out: it is never transcribed or sent anywhere.
5. Press `Esc` twice to stop (one press only asks, so an accidental key cannot
   end a lecture). leo transcribes the whole recording again in one pass,
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

**Long recordings are safe.** A recording can run for hours, a whole day if
you like, and nothing is lost along the way:

- leo records in five-minute pieces and transcribes each one while you keep
  talking, saving its text straight away. Stopping only has the last few
  minutes left to do, however long the recording was.
- If the transcription service is busy or rate-limited, leo waits and tries
  again; the recording carries on meanwhile, and nothing is thrown away.
- If leo quits, crashes or the computer shuts down mid-recording, the next time
  you start leo it finishes that recording and saves it as a note.
- Long transcripts are written up a part at a time, then given one title and
  summary. If the AI for writing is not set up or fails, the note is saved
  anyway, with the transcript in it.
- Disk use stays small: each piece's audio is deleted once its text is saved.
- Pieces that queue up (after a rate limit, or when finishing an interrupted
  recording) are transcribed three at a time, and long notes are written three
  parts at a time, then put back in order. Speech on your own computer takes
  one piece at a time, so it does not compete with itself for your computer.

**Variations:**

```
/record CS 101 Lecture 4     give the note your own title
/record add                  add to the selected note instead of making a new one
/record --screen             record your computer's audio (a video, a call)
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

Press `Esc` when you finish writing and the answer streams in under your
question, which stays in the note as a bold **Q:** line:

```markdown
**Q:** how is BFS different from DFS?

BFS visits nodes level by level using a queue; DFS goes as deep as it can…
```

`@leo` lines are also answered when you save a note from your own editor (`e`).

### Ask all your notes

Press `a` (or type `/ask`) followed by a question:

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

**The quick way**, with GitHub's own command-line tool:

```sh
brew install gh      # or see cli.github.com
gh auth login        # sign in once
leo backup github
```

leo makes a private repository called `leo-notes` on your GitHub, connects
your notes to it, and backs them up. (`leo backup github another-name` picks a
different name.) In the app, `/backup github` does the same.

**By hand**, without `gh`:

1. Create an **empty** repository on GitHub (no README, no .gitignore). Make
   it **private** unless you want your notes public.
2. Run `leo backup` and paste the repository's URL when asked. leo sets up git
   in your notes directory, connects it, and pushes.

From then on:

- every change is committed as you save;
- leo pushes when you quit the app;
- `leo backup` (or `/backup` in the app) backs up on demand: it pulls anything
  newer from GitHub first, then pushes.

**On another computer:** install leo and run `leo backup github` again (signed
in to the same GitHub account), or `leo backup` with the same repository's URL. The notes already backed up come down, this computer's notes
go up, and from then on both stay in step. If the same note was edited on both,
leo keeps both versions' lines in it for you to tidy rather than losing either.

To push while you work instead of on quit, type `/settings` and change **when leo
backs up** on the backup row. You can also set up backup from that screen
instead of running `leo backup`.

---

## 7. Read your notes on your phone

```sh
leo serve
```

This prints a link and a QR code that work from any network: another Wi-Fi,
or mobile data. Scan the code with your phone's camera and your notes open in
the browser, laid out for a phone and following its light or dark mode. From
there you can:

- browse folders and read notes with their formatting: headings, lists, code,
  tables, quotes and links;
- edit in place, the way Obsidian does: tap any line and type. The line shows
  its Markdown while you edit it and goes back to formatted when you move on.
  Enter continues a list or checklist, and the title and tags are edited in
  place too. There is no Edit or Save button, because changes save themselves
  a moment after you stop typing;
- tick checkboxes with a tap;
- pin a note with the pin beside its title in the list. The pin is hollow when
  the note is unpinned and filled purple when it is pinned;
- search every note, with the matching words highlighted;
- move and delete notes (deleted notes go to the trash, and Undo is right
  there), and save a note as a PDF with **PDF**, which opens your phone's print
  sheet;
- restore notes from the trash, and browse by tag.

If a note changes on your computer while you are editing it on the phone,
nothing is overwritten: the computer's version stays, and yours is kept next to
it as "<title> (conflict from phone)".

**How it reaches your phone.** leo opens a private tunnel through Cloudflare's
free `cloudflared` tool, with no account needed. The first time, if it is not
installed, leo offers to install it with Homebrew (or `brew install
cloudflared`). Your computer has to stay on and awake while you use it; leo
keeps a Mac from dozing off while it serves. The link's address changes each
time you start it, so scan the new code. `leo serve --local` skips the tunnel
and only works on the same Wi-Fi as your computer.

**Keeping it safe.** The link carries an access code: anyone with the whole
link can read and edit your notes, so don't share it. Opening the link swaps
the code for a login cookie that lasts 30 days, and takes it out of the address
bar. If a link ever gets out, `leo serve --new-token` makes a new code and
every old link stops working. Stop the server with `Ctrl-C`.

Notes you add in the app while the server runs show up on the phone, and notes
added on the phone show up in the app after `Ctrl-R`.

---

## 8. Use your notes in Obsidian

Your notes are Markdown files in one folder, which is what
[Obsidian](https://obsidian.md) reads.

```sh
leo obsidian
```

opens the folder in Obsidian (`/obsidian` does the same inside leo). The first
time, if Obsidian does not show your notes, choose **Open folder as vault** and
pick the folder; leo copies its path to your clipboard. This is a separate vault
from any you already have.

- Files are named after their titles, so the file list reads well. Renaming a
  file in Obsidian renames the note in leo.
- Notes made in Obsidian show up in leo, and leo leaves them exactly as they are
  until you edit them there. Properties leo does not know (aliases, anything a
  plugin adds) are kept.
- leo picks up changes made in Obsidian within a couple of seconds. If a note
  was changed in Obsidian while leo had it open and you edited it in leo too,
  leo keeps both, calling yours `<title> (conflict from leo)`; `leo doctor`
  lists them.
- Obsidian's own settings folder (`.obsidian`) is not backed up to GitHub.
- `[[links]]` between notes are shown as plain text in leo for now.

---

## 9. Use leo from a shell

The everyday actions also work as commands, which is handy for scripts and
quick captures. `leo --help` lists them with examples, `leo help --all` lists
every command, and `leo <command> --help` shows a command's options.

```sh
leo new "Quick thought" --body "Refactor auth" --tags todo
leo new "cs130/Lecture 4 #exam"     # opens your editor
leo search "refactor"               # shows the line that matched
leo record --title "Meeting notes"  # records until you press Enter
leo ask "what did we cover about graphs?"
leo backup                          # back up to GitHub
leo serve                           # your notes on your phone, from any network
leo doctor                          # check everything, store an API key; exits 1 if anything is broken
```

And the rest, from `leo help --all`:

```sh
leo list --tag todo
leo list cs130                      # one folder
leo view "Rust ownership"
leo edit 3f2a
leo delete 3f2a                     # goes to the trash
leo trash                           # what was deleted; leo trash restore 1
leo pin "Syllabus"                  # keep it at the top of the list
leo obsidian                        # open your notes in Obsidian
leo update                          # install a newer version, if there is one
leo uninstall                       # remove leo; your notes stay
```

A note can be named by its number in `leo list`, the start of its ID, or a
unique part of its title.

---

## 10. Reference

### Keys

| Key | What it does |
|-----|-------------|
| `Enter` | Write in the selected note; in the folders pane, open the folder |
| `Esc` | Done writing; or clear a search, marks or pinned output |
| `n` / `N` | New note / new folder |
| `/` | Find a note, or run a command (`f` and `Ctrl-F` too) |
| `D` | Delete; in the folders pane, the whole folder. `u` brings it back |
| `j` / `k` | Move down / up (arrows work too) |
| `h` / `l` | Switch pane |
| `e` | Open the note in your own editor |
| `r` / `m` | Rename / move it |
| `x` | Tick the note's first open checkbox |
| `p` | Pin the note to the top of its list, or unpin it |
| `Space` | Mark notes, so `D` and `m` act on all of them |
| `u` | Undo the last delete, move or tick |
| `Tab` | Back to a recently visited note |
| `R` | Record |
| `a` | Ask a question, answered from your notes |
| `/settings` | Settings: AI and models, keys, colour, backup |
| `?` / `q` | Help / quit |

While writing in a note: `Enter` continues a list, `Tab` / `Shift-Tab` indent
and outdent, `Ctrl-Z` undoes, a click puts the cursor there (or ticks a box),
and `Esc` finishes.

While recording: type a point, `Enter` adds it, `Ctrl-P` pauses or resumes,
`↑`/`↓` (or the mouse wheel, `PgUp`/`PgDn`, `Home`) scroll back through the
transcript and `End` returns to the newest words, as does waiting 10 seconds;
`Esc` twice stops and saves.

The mouse works too: click to focus or select, scroll with the wheel.

### Where things live

| Platform | Notes and settings |
|----------|--------------------|
| macOS | `~/Library/Application Support/leo/` |
| Linux | `~/.local/share/leo/` (notes), `~/.config/leo/` (settings) |
| Windows | `%APPDATA%\leo\` |

Each note is a Markdown file named after its title, with a small header, and directories are real
directories. Deleted notes wait in a hidden `.trash` folder inside the notes
folder for 30 days; it is never backed up to GitHub. The settings folder also
holds `serve-token` (the code in your `leo serve` link) and `update-check.json`
(when leo last looked for a new version). Settings are in `config.toml`; type `/settings`, then press `e` to open it.
API keys are never stored in that file: they are kept in a separate file only
your account can read.

### Environment variables

| Variable | What it does |
|----------|--------------|
| `LEO_HOME` | Keep notes, settings and keys in this one directory instead |
| `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `XAI_API_KEY`, `OPENROUTER_API_KEY` | A provider's key; takes precedence over a stored one |
| `LEO_CHAT_PROVIDER` / `LEO_TRANSCRIBE_PROVIDER` | Use only this provider for writing / speech |
| `LEO_CHAT_MODEL` | Override the model of the first writing provider |
| `LEO_USE_KEYCHAIN=1` | Store keys in the OS keychain instead of the key file |
| `LEO_SCREEN_DEVICE` | The audio device for `--screen` (default `BlackHole 2ch`) |
| `LEO_NO_UPDATE_CHECK=1` | Never check GitHub for a new version |
| `LEO_INSTALL_DIR` | For the install command: where to put leo (default `~/.local/bin`) |

---

## 11. Troubleshooting

**"Not ready to record: microphone — recorded silence"**
The microphone is not being heard. Check that your terminal is allowed in
System Settings → Privacy & Security → Microphone (then restart the terminal).
On a MacBook, the built-in microphone is off while the lid is closed, so use an
external microphone or open the lid.

**A recording's notes stop mid-sentence**
The AI hit its length limit, and leo shows a warning saying so. Type `/settings`,
then `e`, and raise `max_tokens` for that provider (the cloud providers default
to 32000, plenty for an hour of lecture).

**A note is missing from the list**
If you deleted it, it is in the trash for 30 days: `/trash` (or `leo trash`)
lists it, and `/trash restore <number>` brings it back. Otherwise run
`leo doctor`. If a note file's header was edited and leo cannot read it,
the doctor names the file and the problem; leo never deletes such a file, so
fixing the header brings the note back.

**"No API key" or nothing happens when recording**
Run `leo doctor` (or `/doctor` in the app): it says which kind of AI is missing
and how to add it. Keys can also be added with `/settings`, then `Enter` on the
**key** row.

**`leo backup` fails**
Check that `git push` works from your terminal (a signed-in account or an SSH
key), and run `leo doctor`, which asks the repository whether it answers. If the push is
rejected, run `leo backup` again: it pulls first.

**The phone says "This page needs its link"**
The login cookie is missing or the link was renewed. Open the link `leo serve`
prints, or scan its QR code again.

**`leo serve` does not start**
It needs `cloudflared` (`brew install cloudflared`), or use `leo serve --local`
on the same Wi-Fi. The link changes every
time the server starts, so scan the new code; and the computer has to be
awake, with `leo serve` still running in its terminal.

**A new version is out but `leo update` fails**
It needs `curl` and a connection to GitHub. Running the install command again
does the same thing.

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

The speech model is NVIDIA's
[Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3),
licensed [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/), in the
ONNX conversion by the [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)
project (Apache-2.0), whose library runs it.
