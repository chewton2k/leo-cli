# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Architecture

`leo` is a desktop note app (Tauri, macOS and Windows) written in Rust, laid
out as a Cargo workspace. Each crate may depend only on the ones above it in this table, and
the compiler enforces it — do not add an upward dependency:

| Crate | Holds | Depends on |
|-------|-------|------------|
| `crates/leo-core` | `notes`, `store` (+ undo), `sync` (git), `filename`, `open` (opens a link with the system), `children` (stops a process tree; `quiet_command` starts every helper program without a console window on Windows), `diag`, `manual`, `keep` (retention choices) | — |
| `crates/leo-services` | `ai/` (chains, prompts, live transcription), `config/` (providers, credentials, sync policy), `health`, `providers` | core |
| `crates/leo-web` | leo's engine and pages: `lib.rs` (`start` → `Running`, router), `routes/` (handlers by area), `token.rs`, and the page in `src/web/` (`index.html`, `app/*.js` served as `/app.js`, `markdown.js`, …) | core |
| `crates/leo-host` | `powers()` (the AI, speech, settings, recording housekeeping and import powers handed to the engine) and `leo-test-server` (Playwright's engine) | core, services, web |
| `crates/leo-desktop` | The Tauri app `leo-app`: window, menu bar icon, single instance, open at login, PATH from the login shell, links/downloads/print | core, services, host, web |
| root `leo` | `main.rs` and `cli/`: `leo` opens the app, `leo update`, `leo uninstall`, `--version` | core, services |

Verify with `cargo test` at the root (runs every crate but `leo-desktop`, which
is outside `default-members`) and `cargo clippy --workspace --all-targets`. Do not
run `cargo build`, `cargo install` or `cargo tauri build` unless asked; the one local
build allowed is `pnpm --dir crates/leo-desktop tauri dev` to try the real app by
hand (2026-10-31, the user's call). CI tests and lints `leo-desktop` on macOS and
Windows only (the app needs Rust 1.90; services/core retain their 1.88 check).

Never run the full test suites on the developer's machine (no
workspace-wide `cargo test`, no full Playwright run; the user's call,
2026-10-10). Build out every feature asked for using only quick targeted
checks (`cargo check`, `cargo fmt`, `cargo clippy`, one crate's or one
test's run, a single Playwright `-g` test, one node test file), then commit,
push, and let CI run the full suite. Check CI once; fix a red run with a
follow-up push. CI compares main with the newest verified ancestor (a successful
CI push or a newer release), and pull requests with their base. Unknown inputs
and nightly/manual runs test everything; failed and cancelled pushes never
advance the baseline. `scripts/ci-scope.sh` selects independent Rust, browser,
Node checks; CONTRIBUTING.md describes the selection.

For website changes, do not build leo or run local Cargo checks/tests; rely on
CI for compiled and browser validation (the user's call, 2026-10-11). Lightweight
JavaScript checks and manual checks with fakes are allowed. Reuse the website's
existing components, controls, spacing and themes; new features should look like
the rest of leo, including folder controls and note browsing.

Leo is one app (2026-10-31, the user's call: "a normal app users click"). The
window shows the website, served by leo-web's engine started inside the app on
127.0.0.1 (`leo_web::start`, a remembered port, a per-launch token the window
signs in with). Nothing else can reach it: a Host check refuses any other host
(421) and the cookie is `SameSite=Strict`. There is no `leo serve`, no tunnel,
no phone or other-browser access, and no terminal interface; the terminal keeps
`leo` (opens the app), `leo update`, `leo uninstall` and `--version`. Plans and
the design live in `docs/superpowers/` (gitignored, local).

Subsystem notes load only when working in that crate:
`crates/leo-services/CLAUDE.md`, `crates/leo-web/CLAUDE.md`,
`crates/leo-desktop/CLAUDE.md`.

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

`health.rs` answers "can leo do X here?" in one place, so the doctor scan and
the pre-flight before recording agree, and every gap arrives with what fixes it
rather than as a failure discovered mid-use.

Two rules there, both learned from tests that hung:

* **Never reach the keychain implicitly.** Every function takes
  `store: &dyn SecretStore`. An unsigned test binary reading the real keychain
  blocks on a permission dialog nobody can click — the suite took 1542 seconds
  before this was a parameter.
* **Never resolve DNS.** `to_socket_addrs` has no timeout, so `port_open` parses
  literal addresses and localhost only. A bogus hostname cost 900 seconds.

Checks must stay cheap enough to run on startup: a key presence check, a PATH
scan, or a loopback connect with a 300 ms timeout. Never a real request.

### The manual note

`manual.rs` creates one note on first run: a one-screen quickstart for the app
(Getting around, Recording, Felix, Writing, Finding, Where things live). It is a
real note, so it is searchable, editable and deletable. Installation is recorded
in a `.manual-installed` marker carrying `MANUAL_VERSION` (24); deleting the
manual survives the next launch, and a version bump rewrites the user's note.
Tests: under 60 lines, names Settings, Felix, Record and ⌘F, and names no
terminal command.

### Data model

Notes are stored as one Markdown file per note, with YAML frontmatter, under
the platform data directory (`<data_dir>/leo/notes/`). `store.rs` derives a
note's `directory` from its path on disk, and `directories.json` tracks empty
directories. Each `Note` has `id` (UUID v4), `title`,
`body` (Markdown), `tags`, `directory` (empty string = root), `created_at`, and
`updated_at`.

`sync.rs` provides optional git-backed sync of that directory
(`init`, `connect`, `push`, `pull`, `status`). `config.toml` lives one level
above it, in `<config_dir>/leo/`, so machine-local settings are never pushed to
the notes remote.

### Recent additions worth knowing

- `leo_services::doctor::scan` (sections leo/notes/AI/recording/backup;
  network/mic probes behind `Probe`), plan 4 is shown in Settings → Health.
- Recording loop: `leo_services::session::recorder` (`record`, `resume`,
  `finish`, `write_up`, `Event`); `Capture` takes native microphone/screen audio.
- `Store::unreadable`: files that failed to parse; `save` never deletes them.
- Trash: `save` moves any note file that vanished (and was not just moved:
  its id is not live) to `<notes>/.trash/<same relative path>`, mtime = when
  deleted. `tidy_trash` (load and save) drops live ids and ones past
  `TRASH_DAYS` (30). `.trash/` is gitignored, and `auto_commit` refreshes the
  ignore list so old backups pick it up. The Trash page restores and empties.
- Backup: pulls merge (`--no-rebase --allow-unrelated-histories`), notes merge
  by union and stay LF (`text eol=lf`, beating Git for Windows' autocrlf) in
  .gitattributes, git runs with `core.longpaths=true` on Windows, directories.json is not tracked (rebuilt on load).
- GitHub backup: `sync::github(notes_dir, name)` uses `gh` (`gh_ready` =
  `gh auth token` succeeds, no network): `repo view` else `repo create
  --private`, URL by `gh config get git_protocol` (https + `gh auth setup-git`
  by default), then connect + `now`. Setting backup up from the app arrives
  in Settings → Backup.
- Updates: `leo_services::update::available()` reads the latest tag from the
  `releases/latest` redirect (not the API: rate limits), cached a day in
  `<config>/update-check.json`, failures cached too; `LEO_NO_UPDATE_CHECK=1`
  disables (e2e sets it). Doctor: `Probe.update`. `leo update` runs
  the release's app installer with a bounded checked download; script-download
  failures cannot become successful empty shell pipelines. `LEO_INSTALL_DIR`
  comes from private atomic `app-cli-dir` outside the app, preserving a custom
  shim directory across in-app updates. `LEO_UPDATE_SCRIPT` injects a local
  installer in tests. Windows defers installation until the CLI exits.
- Note files are `<title>.md` (`filename::file_name`: FS-illegal chars to
  `-`, ≤100 chars and ≤200 bytes; same title in a folder → `Title (2).md`,
  oldest first, compared case-insensitively; `Store::assign_paths`). Windows
  device names get a dash (`CON-.md`, `filename::reserved_on_windows`) on every
  platform, since notes sync to Windows; new folders must pass
  `filename::portable_folder`. Old
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
  refresh keeps undo). Engine mutations reconcile before writing; hot read-only
  paths use the existing filesystem watcher with a 60 s reconciliation fallback.
  Returning to the native window forces a check before serving cached reads.
- `sync::pull` = auto_commit, fetch, `keep_both_notes_with_the_same_name`
  (a `.md` both sides added with different ids → local renamed to
  `Name (2).md` and committed), then merge. git's union driver otherwise glues
  add/add files together. `sync::now` commits before pushing. `.obsidian/` is
  gitignored.
- Never slice strings by byte offsets computed from lengths: use char indices
  or `str::get`.
- Tests must never run the microphone probe (`recording(.., false)`).
- Install: `install.sh` downloads the latest GitHub release. Releases are
  automatic: `ci.yml` calls `release.yml` after every job passes on a push to
  main; `scripts/next-version.sh` picks the version (patch + 1, or Cargo.toml's
  if newer, nothing if src/crates/Cargo.* are unchanged since the last tag,
  version-only lines ignored); the `bump` job runs `scripts/set-version.sh`,
  commits `release: vX.Y.Z` to main as github-actions[bot], and the build and
  tag use that commit. So after any push that releases, `git pull` before the
  next push. HTTPS via rustls.
- Writing choices: Ollama, Codex and Claude Code. Their credentials stay with
  their own programs. Speech is local only: a shared catalog model and language
  under `models_dir()/speech`, selected in `<config>/speech-model.json`.
  `leo-services::speech` verifies downloads, isolates native transcribe-cpp
  0.4.0 in a supervised child, prioritizes dictation and pins the model during
  recording. Cloud speech choices fall back to local speech; any keys
  already stored stay in leo's credentials file.
- Dictation preferences/text rules live in `leo-services::dictation`; desktop
  owns global shortcuts, permissions, in-memory capture, transactional paste
  and a bundled nonactivating popup. Writing AI runs on dictations only when
the user turns on the AI writing switch (2026-11-02, the user's call; off by default; on keeps a stored `selected`, else `quick`). Recent
  results are twenty local rows in `leo.db`, never notes or sync data.
- Recording is captured in Rust, including two native tracks for calls; hiding
  the window cannot stop it. App Nap is held while recording. Missing models
  leave audio pending and recoverable; the app can resume native sessions.
  Screenshots are pasted into the Record page with their timestamp.
- Windows child programs stay quiet. Model and toolchain caches stay under the
  existing models directory. Platform installers and release bundles are
  described by their scripts and workflow; updates check the selected catalog
  speech model, and uninstall retains the existing data-removal controls.
- Note connections (cache code still says `graph`): see crates/leo-web/CLAUDE.md.
- Tasks made on the website live in `<notes>/.leo/tasks/<id>.json` (one JSON
  line each, tombstones for deletes), so the git backup carries them;
  `sync::GITATTRIBUTES` merges them by union and leo-web reads the newest line.
  A repeating task's moved days live in its `moved` field (original date →
  new slot); the Calendar page is a Notion-style Day/Week/Month screen beside
  the explorer: see crates/leo-web/CLAUDE.md.
- Tags are hidden everywhere a person looks (decided 2026-10-08: folders,
  search and note connections cover it): no tag column, completion, Tags page or
  chips on the web. Tags already in files are kept and still searchable with `#word`
  (Obsidian), and leo's own markers (`manual`, `listen`) stay.
- Felix in the margin (2026-11-04, the user's call): threads in `leo.db`, never
  note files or git; read-along is off by default. Reads use `Powers.toolless`
  (no tools at all). See crates/leo-web/CLAUDE.md.
- Boards (2026-11-02, the user's call): Jira-style task boards on their own
  rail page (`#/boards`), several boards, untimed cards (title, subheading,
  description, priority) in custom columns, dragged between columns and boards.
  Files `<notes>/.leo/boards/*.json` and `.leo/cards/*.json` sync like tasks
  (union merge, newest line wins); they are not calendar tasks. Felix has
  board tools (list/open, add/update/move/delete cards, add/change/delete
  boards) under the usual Ask/Auto/Read and Undo. See crates/leo-web/CLAUDE.md.
- Menu bar, study order, idle compaction and in-app drag and drop
  (2026-11-02, the user's notes): see crates/leo-desktop/CLAUDE.md (tray,
  `disable_drag_drop_handler`) and crates/leo-web/CLAUDE.md (Felix study order,
  idle compaction).
- Keeping: `leo_core::keep` (`<data>/keep.json`, beside notes, not synced):
  `trash_days` (7/30/90/365/None=forever, default 30) drives `tidy_trash`;
  one shared choice list (7, 30, 90, 365 days, forever);
  `chat_days`, `upload_days`, `transcript_days` and `audio_days` (default forever)
  drive the expiry in `crates/leo-web/src/keeping.rs`. Settings → Data → Keeping
  (`#/settings/keeping`; `#/settings/storage` is an alias) shows the five rows
  and the zip export. `keep::describe` words a choice.
- Cut-down (2026-11-05, the user's call): the Storage page is replaced by
  Keeping; Felix has no web search or page opening; the legacy fallbacks and
  one-time migrations are gone (`notes.json`, old tasks and chats, the direct
  Anthropic wire, `codex exec`, the sign-in device list, AGY/Copilot cleanup,
  stored cloud speech key retirement).

Recording sources live in `leo-core::recording` (`Archive`, and `Profile {context,
wants}`: calendar-event details and what the user wants from the notes), shared by
the app. Saved recording session IDs make new notes and appends idempotent;
retain sources before saving the note, then commit the session after the note is
durable. Default AI gap-filling and merging of typed points always stay, whatever
the user asks for. Note workflows (folder defaults, note formats, vocabulary, saved
actions, chat scope) were removed on 2026-10-10, the user's call: the "What do you
want from the notes?" box replaces formats.

Interview coding practice was removed by the user's decision on 2026-11-05.
Felix keeps Chat and Study, including quizzes, study sessions and review. Legacy
interview database tables and rows stay unused; old interview-style chats open
as Chat and retired cards show a short removal message. Downloaded runtimes stay
on disk, untouched.

Keeping scans use a read-only Store snapshot and blocking workers so filesystem
walks do not hold the notes lock. The `Housekeeper` in `crates/leo-web/src/record.rs`
only lists and discards unfinished recordings.

Website backup leaves the browser through an authenticated beacon; the web crate
queues work and bounds the push, while WebSettings supplies the configured git
command through SettingsApi::backup. Off, no remote, and no unpushed commits skip
network work. It commits note/task files first, never pulls, and never creates a
remote or changes the user’s backup policy.

Security cleanup: note roots refuse symlinks, folder scans have depth bounds, and displayed short IDs use
Unicode characters. Git children run without credential prompts and are reaped on
timeout. Credential updates use a bounded writer plus a cross-process private lock;
use `secret::default_store()` (private file by default, optional keychain), never a
backend directly. `.superpowers/` stays local
and must never be staged or committed.

Settings uses grouped `#/settings/<section>` routes in a sidebar with icons (a rounded panel set apart from the explorer; panel and section are centered together), `#/settings/storage` stays as an alias of Keeping; Trash lives there too (Data), and on/off settings are switches, short choices sliding segmented controls.
`leo_web::app_settings::AppApi` bridges native preferences, permission requests,
Health and signed updates; web depends only on core. Core's activity gate holds
recording, dictation and clipboard restoration leases and excludes installation.
Backup setup is explicit, preserves an existing remote and runs outside the store
lock. Health uses local doctor checks without microphone or AI requests. First-launch
setup groups microphone and macOS Accessibility on one page with live checkmarks,
then offers a speech model; existing completion flags remain compatible.
Desktop navigation uses an icon rail beside a folder-and-note explorer. Folder
labels expand the tree; individual note rows open their notes.

Desktop releases contain macOS arm64/x64 DMGs and signed app archives, Windows
x64 NSIS and a signed latest.json. `scripts/desktop_release.py` validates asset
sets and builds the updater manifest. `scripts/activate_app_installers.py` refuses
to replace main installers until a complete published app release exists; the
publish job then commits that cutover. Prepared installers live in
`scripts/install-app.{sh,ps1}`. Linux exits before writes. No installer downloads
models: first-launch setup owns that choice.

The CLI is bundled with the app; macOS links to it, Windows uses a UTF-8 cmd shim
that preserves its caller's code page. Uninstall validates bundle identity, refuses
active apps, removes owned login entries and links, keeps notes and unrelated
files, and defers locked Windows removal. Shell-profile edits are atomic and
retain permissions. Tests use scratch bundles and ad-hoc signatures, never the
real signing keychain. Four persistent signing secrets gate release before bump;
private values are generated outside the repository and added by the user.

Windows speech and Rust binaries share the dynamic CRT; desktop release bundles
include the required Visual C++ runtime beside the executables.
