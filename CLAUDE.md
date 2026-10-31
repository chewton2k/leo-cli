# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Architecture

`leo` is a desktop note app (Tauri, macOS and Windows) written in Rust, laid
out as a Cargo workspace. Each crate may depend only on the ones above it in this table, and
the compiler enforces it — do not add an upward dependency:

| Crate | Holds | Depends on |
|-------|-------|------------|
| `crates/leo-core` | `notes`, `store` (+ undo), `sync` (git), `filename`, `open` (opens a link with the system), `children` (stops a process tree; `quiet_command` starts every helper program without a console window on Windows), `diag`, `manual`, `action/` (`Line`/`Kind` output lines and front matter) | — |
| `crates/leo-services` | `ai/` (chains, prompts, live transcription), `config/` (providers, credentials, theme data, sync policy), `listen`, `health`, `providers` | core |
| `crates/leo-web` | leo's engine and pages: `lib.rs` (`start` → `Running`, router), `routes/` (handlers by area), `token.rs`, and the page in `src/web/` (`index.html`, `app/*.js` served as `/app.js`, `markdown.js`, …) | core |
| `crates/leo-host` | `powers()` (the AI, speech, settings, storage and import powers handed to the engine) and `leo-test-server` (Playwright's engine) | core, services, web |
| `crates/leo-desktop` | The Tauri app `leo-app`: window, menu bar icon, single instance, open at login, PATH from the login shell, links/downloads/print | core, host, web |
| root `leo` | `main.rs` and `cli/`: `leo` opens the app, `leo update`, `leo uninstall`, `--version` | core, services |

Verify with `cargo test` at the root (runs every crate but `leo-desktop`, which
is outside `default-members`) and `cargo clippy --workspace --all-targets`. Do not
run `cargo build`, `cargo install` or `cargo tauri build` unless asked; the one local
build allowed is `pnpm --dir crates/leo-desktop tauri dev` to try the real app by
hand (2026-10-31, the user's call). CI tests and lints `leo-desktop` on macOS and
Windows only; Linux rows `--exclude leo-desktop` (it needs Rust 1.90; the rest
stays on 1.88).

Never run the full test suites on the developer's machine (no
workspace-wide `cargo test`, no full Playwright run; the user's call,
2026-10-10). Build out every feature asked for using only quick targeted
checks (`cargo check`, `cargo fmt`, `cargo clippy`, one crate's or one
test's run, a single Playwright `-g` test, one node test file), then commit,
push, and let CI run the full suite. Check CI once; fix a red run with a
follow-up push. CI tests only what changed since the last release tag
(`scripts/ci-scope.sh`; DEVELOPER.md says what each kind of change runs).

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
in a `.manual-installed` marker carrying `MANUAL_VERSION` (18); deleting the
manual survives the next launch, and a version bump rewrites the user's note.
Tests: under 60 lines, names Settings, Felix, Record and ⌘J, and names no
terminal command.

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

### Recent additions worth knowing

- `leo_services::doctor::scan` (sections leo/notes/AI/recording/backup;
  network/mic probes behind `Probe`), formatted once by `doctor::report`; plan 4
  shows it in Settings → Health.
- Recording loop: `leo_services::session::recorder` (`record`, `resume`,
  `finish`, `write_up`, `Event`); `Capture::fed` takes browser audio from a
  channel and drains it on stop. Pauses are cut by `listen::cut_pauses` before
  the final transcription.
- `Store::unreadable`: files that failed to parse; `save` never deletes them.
- Trash: `save` moves any note file that vanished (and was not just moved:
  its id is not live) to `<notes>/.trash/<same relative path>`, mtime = when
  deleted. `tidy_trash` (load and save) drops live ids and ones past
  `TRASH_DAYS` (30). `.trash/` is gitignored, and `auto_commit` refreshes the
  ignore list so old backups pick it up. The Trash page restores and empties.
- Backup: pulls merge (`--no-rebase --allow-unrelated-histories`), notes merge
  by union and stay LF (`text eol=lf`, beating Git for Windows' autocrlf) in
  .gitattributes, git runs with `core.longpaths=true` on Windows, directories.json is not tracked (rebuilt on load).
- Editor: `leo_core::editor` (EDITOR, VISUAL, then nano, then vi; flags split,
  quotes kept together, a setting that is an existing file is one program; on
  Windows the program is found through `paths::find`, so `code.cmd` runs).
- GitHub backup: `sync::github(notes_dir, name)` uses `gh` (`gh_ready` =
  `gh auth token` succeeds, no network): `repo view` else `repo create
  --private`, URL by `gh config get git_protocol` (https + `gh auth setup-git`
  by default), then connect + `now`. Setting backup up from the app arrives
  with plan 4's Settings.
- Updates: `leo_services::update::available()` reads the latest tag from the
  `releases/latest` redirect (not the API: rate limits), cached a day in
  `<config>/update-check.json`, failures cached too; `LEO_NO_UPDATE_CHECK=1`
  disables (e2e sets it). Doctor: `Probe.update`. `leo update` runs
  install.sh with `LEO_INSTALL_DIR`=its own dir and `LEO_INSTALL_SKIP_PATH=1`
  (`LEO_UPDATE_SCRIPT` = local installer, for tests). install.sh renames the
  new binary into place: cp onto a running binary gets it killed on macOS.
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
  refresh keeps undo); the engine reloads the store from disk on every request.
- `sync::pull` = auto_commit, fetch, `keep_both_notes_with_the_same_name`
  (a `.md` both sides added with different ids → local renamed to
  `Name (2).md` and committed), then merge. git's union driver otherwise glues
  add/add files together. `sync::now` commits before pushing. `.obsidian/` is
  gitignored.
- Never slice strings by byte offsets computed from lengths: use char indices
  or `str::get`.
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
- Writing choices: Ollama, Codex and Claude Code. The two CLIs use account
  sign-in; removed AGY/Copilot profiles stay available only for explicit legacy
  cleanup in Storage. Speech choices
  stay Parakeet, OpenAI, Gemini and xAI, including their stored credentials.
  See crates/leo-services/CLAUDE.md; models live in `config/choice.rs`.
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
- Knowledge graph (code and routes still say `map`): see crates/leo-web/CLAUDE.md.
- Tasks made on the website live in `<notes>/.leo/tasks/<id>.json` (one JSON
  line each, tombstones for deletes), so the git backup carries them;
  `sync::GITATTRIBUTES` merges them by union and leo-web reads the newest line.
- Tags are hidden everywhere a person looks (decided 2026-10-08: folders,
  search and the knowledge graph cover it): no `#tag` in `new` (split_new returns (dir,
  title)), no tag column, completion, `--tags`/`--tag` flags, Tags page or chips
  on the web. Tags already in files are kept and still searchable with `#word`
  (Obsidian), and leo's own markers (`manual`, `listen`) stay.
- Keeping: `leo_core::keep` (`<data>/keep.json`, beside notes, not synced):
  `trash_days` (7/30/90/365/None=forever, default 30) drives `tidy_trash`;
  `chat_days` (30/90/365/None, default forever) drives `chats::tidy` in leo-web.
  Trash messages use `keep::kept_for`.

Recording sources live in `leo-core::recording` (`Archive`, and `Profile {context,
wants}`: calendar-event details and what the user wants from the notes), shared by
both front ends. Saved recording session IDs make new notes and appends idempotent;
retain sources before saving the note, then commit the session after the note is
durable. Default AI gap-filling and merging of typed points always stay, whatever
the user asks for. Note workflows (folder defaults, note formats, vocabulary, saved
actions, chat scope) were removed on 2026-10-10, the user's call: the "What do you
want from the notes?" box replaces formats.

Interview practice is Felix's third style and a sidebar/menu page (2026-10-12).
Use shared configurable frameworks, independent of notes, with frozen attempts,
provisional evidence-based feedback and typed/code/vector workspaces. Practice
schedules were removed (2026-10-13, the user's call). Only the user's own code
and the answer key's reference solution (to check the tests) run, in the
browser's sandbox (never on the server); see "Instant feedback" in
crates/leo-web/CLAUDE.md. Felix sees and can fix the test cases; hidden tests
show once submitted; Auto applies Felix's code edits with Undo.
Reuse current cards, sheets, controls and theme tokens; keep asynchronous updates
from replacing a focused editor. See crates/leo-web/CLAUDE.md and
.superpowers/plans/2026-10-12-felix-interviews.md for the implementation invariants.

Storage scans use a read-only Store snapshot and blocking workers so filesystem
walks and database compaction do not hold the notes lock. The Housekeeper lists
both config and data folder settings, the application, Python runtime and other
model downloads; cache removal refuses symlinks and active partial downloads.

Website backup leaves the browser through an authenticated beacon; the web crate
queues work and bounds the push, while WebSettings supplies the configured git
command through SettingsApi::backup. Off, no remote, and no unpushed commits skip
network work. It commits note/task files first, never pulls, and never creates a
remote or changes the user’s backup policy.

Security cleanup: note roots refuse symlinks, folder scans have depth bounds, legacy
migration uses contained paths and atomic writes, and displayed short IDs use
Unicode characters. Git children run without credential prompts and are reaped on
timeout. Credential updates use a bounded writer plus a cross-process private lock;
use `secret::default_store()` (private file by default, optional keychain), never a
backend directly. Legacy AGY/Copilot profiles contain private sign-in state, stay outside
notes/exports and have metadata-only Storage controls. `.superpowers/` stays local
and must never be staged or committed.
