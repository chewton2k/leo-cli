# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Architecture

`leo` is a terminal note manager written in Rust, laid out as a Cargo
workspace. Each crate may depend only on the ones above it in this table, and
the compiler enforces it — do not add an upward dependency:

| Crate | Holds | Depends on |
|-------|-------|------------|
| `crates/leo-core` | `notes`, `store` (+ undo), `sync` (git), `filename`, `open` (opens a link with the system), `diag`, `manual`, `action/` (vocabulary, parser, handlers, resolution, frontmatter) | — |
| `crates/leo-services` | `ai/` (chains, prompts, `RealAi`, live transcription), `config/` (providers, credentials, theme data, sync policy), `listen`, `health`, `providers` | core |
| `crates/leo-tui` | The full-screen app (`lib.rs` = `App`), its views, and `shell` (terminal effect performers shared with the CLI) | core, services |
| `crates/leo-web` | `leo serve`: `lib.rs` (state, `serve`, router), `routes/` (handlers by area), `terminal.rs`, `token.rs`, `tunnel.rs`, and the page in `src/web/` (`index.html`, `app/*.js` served as `/app.js`, `markdown.js`, …) | core |
| root `leo` | `main.rs` (28 lines) and `cli/` (clap surface; `notes`, `doctor`, `sync` subcommands) | all |

Verify with `cargo test` at the root (runs every crate) and
`cargo clippy --workspace --all-targets`. Do not run `cargo build` or
`cargo install` unless asked.

Two entry modes share one command vocabulary and one `Store`:

1. **TUI** (`leo-tui`) — `leo` with no arguments in a TTY. Three panes over a
   `/` command line and a status line. Outside a TTY, bare `leo` exits 1.
2. **CLI subcommands** (`cli/`) — `Serve` runs on a `tokio` runtime; all others
   are synchronous. `doctor` is the one command for "what works, fix what does
   not" (`setup` was folded into it; `model`, `config`, `setup` no longer parse).

Both convert their input into an `action::Action` and apply it through the same
handlers, so `leo edit 1` and `:edit 1` cannot drift apart.


Subsystem notes load only when working in that crate: `crates/leo-tui/CLAUDE.md`,
`crates/leo-services/CLAUDE.md`, `crates/leo-web/CLAUDE.md`.

### The Action vocabulary

`leo_core::action` is the single command vocabulary. `parse.rs` holds `VERBS`
— one row per verb with its aliases, usage and summary — and help, the `/` menu
and usage errors are all generated from it. `RETIRED` maps every removed word
to its replacement (a command, a key, or a place on screen) and a reason.

Handlers take `&mut Store` and a `Ctx` and return an `Outcome`, which carries
output lines, a new numbering, a new directory, and an `Effect`. Handlers
contain no terminal code: when a step needs the terminal — `$EDITOR`, a
confirmation, recording — the handler returns an `Effect` and the shell
performs it, then feeds the result back through a second-phase handler
(`apply_edit`, `apply_confirmed`, `apply_transcript`).

A command that names no note means the marked notes if any, otherwise the
selected note: `fill_selected` does that once, from `Ctx.selected` and
`Ctx.marked`, for both `apply` and the TUI's `run_action`.

AI calls go through the `action::Ai` trait; `leo_services::ai::RealAi` is the
real one, so core never depends on the provider chains and handler tests use a
double.

### Undo lives in the store

`Store` keeps a bounded stack of `Undoable` values, and `delete_note`,
`delete_dir_recursive`, `move_note` and `toggle_checkbox` all record onto it. It
restores whole `Note` values rather than replaying an inverse operation, because a
restored note has to come back with its original id and timestamps instead of as a
copy that resembles it. The stack is not persisted: undo covers a session, and a
deletion that survived a restart is a decision the user has lived with.

Two invariants worth keeping: a no-op (a delete that matched nothing, a refused
root delete) must record nothing, or `u` appears to do nothing; and undo must not
push its own inverse, or one press would toggle forever.

### Automatic backup is a policy, not a timer

`config/sync.rs` holds the decision as data and two pure functions,
`should_push_now` and `should_push_on_quit`, so "why did it push then?" is
answerable without a terminal and a stopwatch. The App gathers the inputs
(`PushWhen`) and the worker performs the effect.

Four rules that must stay: a push only starts from the *idle* branch of the event
loop, so it never competes with typing; `MIN_PUSH_GAP` is a floor regardless of
`idle_secs`, so a stream of small edits cannot produce a push every few seconds;
no upstream means no push, which is not the same as nothing to push; and a failure
records `last_push` too, or a broken remote means a git process every time the
loop goes quiet. A rejected push is reported and never retried automatically —
the fix is a pull, and doing that silently could merge someone's notes without
telling them.

`sync::push` and `pull` ask git for the current branch. `main` was hardcoded,
which meant a repository on `master` could not be pushed at all and blamed the
refspec rather than saying so.

### Capability checks

`health.rs` answers "can leo do X here?" in one place, so `leo doctor`, the
first-run greeting, and the pre-flight before recording all agree, and every gap
arrives with the command that fixes it rather than as a failure discovered mid-use.

Two rules there, both learned from tests that hung:

* **Never reach the keychain implicitly.** Every function takes
  `store: &dyn SecretStore`. An unsigned test binary reading the real keychain
  blocks on a permission dialog nobody can click — the suite took 1542 seconds
  before this was a parameter.
* **Never resolve DNS.** `to_socket_addrs` has no timeout, so `port_open` parses
  literal addresses and localhost only. A bogus hostname cost 900 seconds.

Checks must stay cheap enough to run on startup: a key presence check, a PATH
scan, or a loopback connect with a 300 ms timeout. Never a real request.

### One name per command

`action::VERBS` holds 17 verbs: new, edit, delete, rename, undo, record, ask,
pin, mkdir, cd, mv, backup, trash, settings, doctor, help, quit. `obsidian` was
removed (2026-10-08, the user's call) and is retired: it says the notes are
Markdown files any app can open. Reading files other editors write (no front
matter, renamed files, unknown properties, `.obsidian/` not backed up) stays.
`/settings` (Action::Settings → Effect::Settings) opens settings; Ctrl-S no
longer does (it only saves while writing), and tests/wording.rs rejects it. `record screen`
(first word; `--screen` still parses) records what the computer plays. `listen` and
`sync` were renamed and removed outright (not retired), in the CLI too.
`ask <words>` is always a question (`AskNotes`); only an exact note id means
that note's `@leo` lines. Delete, marked delete and `rmdir -r` apply at once
and set `Outcome.undoable` (TUI appends "u brings it back"; the CLI names
`leo trash restore`); only `trash empty` confirms. Anything the panes already do (list, view,
tags, search, pwd, clear, check, rmdir) and anything removed (remind, export)
is retired. An alias stays only when it is shell muscle memory (`rm`, `exit`,
`q`) or the key that does the same thing (`e`, `u`, `?`); `action::RETIRED` maps every dropped name to
its replacement so `parse` can answer with `Parsed::Retired` instead of "unknown
command", which reads like a typo. Adding an alias back should have to clear that
bar — a test caps the total at eight.

### The manual note

`manual.rs` creates one note on first run: a one-screen quickstart. It is a real
note, so it is searchable, editable, and deletable like any other — which is the
point: the app explains itself in the pane the user is already looking at.

It is deliberately **not** a full reference. It used to be, and at 220 lines it
was the largest note most users owned, sitting permanently at the top of their
list and duplicating a help screen one keypress away. `?` is the sole full
reference now, which is why `help.rs` has the test that every verb appears there
and that no retired name does.

Installation is recorded in a `.manual-installed` marker inside the notes
directory, carrying `MANUAL_VERSION`. Presence of the note is deliberately not
the signal: deleting the manual is a legitimate choice and must survive the next
launch. Bumping the version offers an updated manual without resurrecting a
deleted one.

Tests keep it honest from both directions: the manual must stay under 60 lines,
must point at `?`, `/settings` and `/doctor`, and must name no retired command;
`help.rs` must mention every live verb. Bumping `MANUAL_VERSION` rewrites the
note the user already has rather than leaving two side by side.

### Data model

Notes are stored as one Markdown file per note, with YAML frontmatter, under
the platform data directory (`<data_dir>/leo/notes/`). `store.rs` derives a
note's `directory` from its path on disk, and `directories.json` tracks empty
directories. A legacy `notes.json` is migrated automatically on first read
(`migrate_from_json`, `store.rs:455`). Each `Note` has `id` (UUID v4), `title`,
`body` (Markdown), `tags`, `directory` (empty string = root), `created_at`, and
`updated_at`.

`sync.rs` provides optional git-backed sync of that directory
(`init`, `connect`, `push`, `pull`, `status`). `config.toml` lives one level
above it, in `<config_dir>/leo/`, so machine-local settings are never pushed to
the notes remote.

### Note resolution

`action::resolve` is the one implementation, used by both front ends (priority
order):
1. **Numeric index** — 1-based position in the current numbering: the notes pane
   in the TUI, or the default sorted list for CLI subcommands
2. **ID prefix** — any unique prefix of the UUID
3. **Title match** — case-insensitive substring, only if exactly one match

It always returns a note's full ID rather than the prefix the user typed: an
`Outcome` can outlive the store state it was built from, and a prefix that is
unique today can become ambiguous after the next `sync pull`. An ambiguous title
returns every candidate so each front end can render the disambiguation its own
way.

### Recent additions worth knowing

- `leo doctor` and `/doctor` = `leo_services::doctor::scan` (sections leo/notes/
  AI/recording/backup; network/mic probes behind `Probe`), formatted once by
  `doctor::report`. The CLI then offers to store a missing key; the TUI runs the
  scan on a worker (`task::start_doctor`, `App.checking`) and pins the report in
  the preview. `App.probe` is `Probe::default()` in tests: no network, no mic.
- Setup is just in time: first run only greets on the status line.
  `set_up_first(Need::Recording | Need::Writing)` opens `welcome` with the
  missing `health::setup_steps` before R / ask; `/backup` with no remote runs
  `offer_backup_setup`. `App.setup_steps` is a fn field (tests inject).
- Hints: at most `hints::MOST` (5) per place. `leo --help` shows 7 commands;
  `leo help --all` (`wants_everything`) unhides the rest.
- Notes list rows end with `when::short(updated_at)`; titles carry
  `when::long`. Outside edits do not move `updated_at` (git pull resets mtimes).
- Manual is version 15: five keys, the commands that matter most (/settings, /record, /record screen, /ask, /doctor, /backup, leo serve), recording, writing, finding, where things live.
- `/ask <question>` that names no note → `Effect::AskNotes`; notes chosen by
  `Store::relevant`; answer shown via `App.answer` until Esc.
- Recording loop: `leo_services::session::recorder` (`record`, `resume`, `finish`,
  `write_up`, `Event`) is shared by the TUI (`task::start_listen` forwards events)
  and the website; `Capture::fed` takes browser audio from a channel and drains it
  on stop.
- Recording pause: Ctrl-P → `Job::set_paused`; worker records pause spans and
  `listen::cut_pauses` removes them before the final transcription.
- `Outcome.select` makes the TUI jump to a note just created or recorded.
- `Store::unreadable`: files that failed to parse; `save` never deletes them.
- Trash: `save` moves any note file that vanished (and was not just moved:
  its id is not live) to `<notes>/.trash/<same relative path>`, mtime = when
  deleted. `tidy_trash` (load and save) drops live ids and ones past
  `TRASH_DAYS` (30). `.trash/` is gitignored, and `auto_commit` refreshes the
  ignore list so old backups pick it up. `/trash`, `trash restore <n|title>`,
  `trash empty` (asks: `ConfirmedAction::EmptyTrash`); `leo trash` in a shell.
- Backup: pulls merge (`--no-rebase --allow-unrelated-histories`), notes merge
  by union (.gitattributes), directories.json is not tracked (rebuilt on load).
- Editor: `leo_core::editor` (EDITOR, VISUAL, then nano, then vi; flags split).
- GitHub backup: `sync::github(notes_dir, name)` uses `gh` (`gh_ready` =
  `gh auth token` succeeds, no network): `repo view` else `repo create
  --private`, URL by `gh config get git_protocol` (https + `gh auth setup-git`
  by default), then connect + `now`. `SyncAction::GitHub`, `leo sync github
  [name]`; bare `leo sync` offers it when gh is ready; the app's backup steps
  go through `App::offer_backup_setup` (`App.gh_ready` is a fn, `|| false` in
  tests). e2e uses a fake `gh` over local bare repos.
- Updates: `leo_services::update::available()` reads the latest tag from the
  `releases/latest` redirect (not the API: rate limits), cached a day in
  `<config>/update-check.json`, failures cached too; `LEO_NO_UPDATE_CHECK=1`
  disables (e2e sets it). TUI: `task::start_update_check` → `App.update` →
  `pump_update` status message. Doctor: `Probe.update`. `leo update` runs
  install.sh with `LEO_INSTALL_DIR`=its own dir and `LEO_INSTALL_SKIP_PATH=1`
  (`LEO_UPDATE_SCRIPT` = local installer, for tests). install.sh renames the
  new binary into place: cp onto a running binary gets it killed on macOS.
- Note files are `<title>.md` (`filename::file_name`: FS-illegal chars to
  `-`, ≤100 chars and ≤200 bytes; same title in a folder → `Title (2).md`,
  oldest first, compared case-insensitively; `Store::assign_paths`). Old
  `<id>.md` files are renamed on the next save. A file name that is neither an
  id nor its title's name becomes the title on load (Obsidian rename).
- Headerless `.md` files load as notes: title = stem, id = `stable_id(path)`
  (FNV-1a 128 → UUID v8, golden-tested: never change it), dates = mtime, tags
  from list or string. `Note.extra` keeps unknown front matter.
- `Store.seen: id → Seen { path, disk, model }`: save writes only notes whose
  rendered hash changed; if the file on disk no longer matches `disk`
  (edited/renamed/deleted elsewhere) and leo changed the note too, it writes
  `<title> (conflict from leo)` with a new id (`CONFLICT_SUFFIX`, listed by
  doctor) instead of overwriting; deleted notes are trashed only if their file
  is still what leo saw. Writes are temp-file + rename.
- `Store::changed_on_disk`/`refresh` (fingerprint of paths, sizes, mtimes;
  refresh keeps undo); `App::maybe_reload_from_disk` every 2 s while idle and
  nothing is being typed/recorded/asked; Ctrl-R uses refresh too.
- `sync::pull` = auto_commit, fetch, `keep_both_notes_with_the_same_name`
  (a `.md` both sides added with different ids → local renamed to
  `Name (2).md` and committed), then merge. git's union driver otherwise glues
  add/add files together. `sync::now` commits before pushing. `.obsidian/` is
  gitignored.
- Crash-proofing: worker threads run through `task::spawn_guarded` (a panic
  becomes `TaskEvent::Failed`), live rolls are `caught`, the panic hook only
  restores the terminal for main-thread panics. Mouse reporting is
  clicks+wheel only (`MOUSE_ON`/`MOUSE_OFF`). Esc twice stops a recording
  (`STOP_CONFIRM_WITHIN`). Never slice strings by byte offsets computed from
  lengths: use char indices or `str::get`.
- Live transcript scroll-back: `view::livescroll::LiveScroll` (↑↓ PgUp PgDn
  Home End, wheel), resumes following 10 s after the last scroll.
- `listen::microphone_peak` has a deadline (3 s past the sample); tests must
  never call the probe (`recording(.., false)`).
- Install: `install.sh` downloads the latest GitHub release. Releases are
  automatic: `ci.yml` calls `release.yml` after every job passes on a push to
  main; `scripts/next-version.sh` picks the version (patch + 1, or Cargo.toml's
  if newer, nothing if src/crates/Cargo.* are unchanged since the last tag,
  version-only lines ignored); the `bump` job runs `scripts/set-version.sh`,
  commits `release: vX.Y.Z` to main as github-actions[bot], and the build and
  tag use that commit. So after any push that releases, `git pull` before the
  next push. HTTPS via rustls.
- Nothing may print to stdout/stderr while the TUI owns the screen: it scrolls the panes. `diag::is_quiet()` is true then; `sync::print_output`/`connect` check it, `sync::init` and provider logout print nothing, and child processes get `Stdio::null()` unless run inside `App::outside`. Settings keys end with `repaint = true`.
- AI: eight writing providers (ollama, openai, codex, anthropic, claude_code,
  gemini, xai, openrouter; codex and claude_code run the user's signed-in CLI) plus speech (parakeet built in via sherpa-onnx, openai_whisper,
  gemini_speech, xai_speech); see crates/leo-services/CLAUDE.md. Model lists with prices live
  in `config/choice.rs`, cheapest first.
- Speech model: Parakeet (4 files, 670 MB) is fetched by install.sh and checked
  by `leo update` (`providers::speech_model_state` over `parakeet::manifest()`:
  Missing/Damaged/Ready by SHA-256; only bad files are re-downloaded; old
  `ggml-base.en.bin` removed once it is ready); tests use
  `LEO_INSTALL_MODEL_URL` (base) + `LEO_INSTALL_MODEL_MANIFEST` with file://
  and e2e sets `LEO_INSTALL_NO_MODEL`. Uninstall removes `~/.leo` and
  `LEO_HOME/models`.
- Windows: `install.ps1` (zip + .sha256, `%LOCALAPPDATA%\Programs\leo`, user
  PATH, same Parakeet manifest); `leo update` runs it through powershell,
  `leo uninstall` parks the running exe as `.exe.old` and deletes it later.
  Release target `x86_64-pc-windows-msvc` with `+crt-static` (sherpa libs are
  MT). CI tests on windows-latest (`--no-fail-fast`) and smoke-tests
  install.ps1; e2e is `cfg(unix)`. Folder names are always `/`-joined, even
  when read from disk; TOML paths in tests use literal `'...'` strings.
- Map of ideas: `leo serve` only (web), see crates/leo-web/CLAUDE.md.
- Tags are hidden everywhere a person looks (decided 2026-10-08: folders,
  search and the map cover it): no `#tag` in `new` (split_new returns (dir,
  title)), no tag column, completion, `--tags`/`--tag` flags, Tags page or chips
  on the web. Tags already in files are kept and still searchable with `#word`
  (Obsidian), and leo's own markers (`manual`, `listen`) stay.
- Keeping: `leo_core::keep` (`<data>/keep.json`, beside notes, not synced):
  `trash_days` (7/30/90/365/None=forever, default 30) drives `tidy_trash`;
  `chat_days` (30/90/365/None, default forever) drives `chats::tidy` in leo-web.
  Trash messages use `keep::kept_for`.
