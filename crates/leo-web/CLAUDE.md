# leo-web: `leo serve` and the phone page

### Layout

- `lib.rs`: `Powers`, `AppState` (+ `with_store`, `store_now`), `serve`,
  `router`. Handlers live in `routes/<area>.rs` (assets, auth, settings,
  uploads, downloads, activity, housekeeping, felix, notes, trash, map);
  `record.rs` keeps its own. `terminal.rs` is what `serve` prints. Items are
  `pub(crate)` only when another file uses them. Handler tests are in
  `src/tests/<area>.rs`, helpers (`state_with`, `run`, `json_of`, `host`) in
  `src/tests/mod.rs`.
- `/app.js` is `web/app/*.js` joined in one closure by `routes/assets.rs`
  (`concat!`, order: core, trash, map, settings, storage, uploads, sheets,
  actions, events). The parts share scope on purpose: pages reassign `state`.
  Add a part to the `concat!` list; `every_part_of_the_page_script_is_served`
  fails otherwise and `the_page_script_parses` runs `node --check` on the whole.

### Notes

- `leo serve` (`leo-web`): code in `<config>/serve-token` (0600, 32 hex,
  `--new-token` rotates and signs everyone out); `?token=` on GET `/` → 303 to
  `/` + a per-browser `leo_session` cookie (64 hex, 30 days, `Secure` when
  `X-Forwarded-Proto: https`) recorded in `<config>/serve-sessions.json` (0600,
  `sessions::Sessions`: device from User-Agent, last_seen written at most every
  5 min, unused 30 days → expired). An old `leo_token` cookie is swapped for a
  session. `Gate` holds the code (rotatable by POST `/api/sessions/new-link`)
  and sessions; the page only ever sees `handle_of(secret)`. Constant-time compare;
  no CORS; no-referrer/nosniff/DENY/no-store headers; the store is reloaded
  from disk on every request (`AppState::fresh`). `--anywhere` =
  `tunnel::start` runs `cloudflared tunnel --url` and reads the
  trycloudflare address from stderr. The tunnel is the default: `leo serve` opens it (`src/cli/serve.rs` offers `brew install cloudflared` in a TTY when missing), `--local` is Wi-Fi only, `--anywhere` is a hidden no-op kept for old scripts. Busy port → next free one; macOS
  `caffeinate -i -w <pid>` while serving. `Store.known`: save only trashes
  ids it has loaded or written, so another writer's new notes survive.
- Phone page (`crates/leo-web/src/web/`): no inline JS (CSP `script-src
  'self'`); `app.js` uses `data-action` delegation and a hash router
  (`#/f/dir`, `#/n/id`, `#/new/dir`, `#/search/q`, `#/tags`, `#/trash`).
  `markdown.js` escapes everything first and numbers checkboxes
  exactly like `Note::toggle_checkbox` (`notes::checkbox_line`: `-`/`*`/`+`/`1.`/`1)` then ` [ ]`/`[x]`, text optional; counted inside code fences, not inside `>` quotes); tested by
  `tests/markdown.test.js` via node. API extras: `/api/folders`,
  `/api/trash`, `/api/trash/{id}/restore`, PATCH `pinned`.
- Live-preview editing: there is no editor view. `editing.js` (pure, node-tested
  in `tests/editing.test.js`) splits the body into blocks (each line, or a
  whole fence/table/quote), and `doc.js` renders each block via `render(text,
  {boxOffset})` and swaps a tapped block for a textarea of its raw lines, with
  the caret mapped by `rawOffset`. A blur that happens while the pointer is down
  inside the doc is deferred to the click, or the redraw would remove the
  click's target. Autosave (`app.js` `flush`) PATCHes 700 ms after the last
  change with `base` = the `version` the page loaded (FNV-1a over
  title/body/tags). A 409 means the note changed elsewhere, so the page POSTs
  "<title> (conflict from phone)" and reloads. A new note is not created until
  it has content. Pin lives on the list cards (`pin-card`), and PDF is
  `window.print()` with `@media print`.

- Checkbox taps: `doc.js` `boxFor` treats a tap left of a task's text (the box, or the gap) as a tick, so a near miss on a phone never opens the line for editing.

- Map of ideas (`#/map`, `#/map/<noteId>`; menu "Map of ideas", note action
  "Map"): web only, a note-to-note knowledge graph. `graph.rs`: reads notes in
  batches (8 notes / 18k chars) for `{summary, concepts}` (ideas and methods,
  vocabulary reused so names match), then links notes: notes are hashed into
  `group_count` groups of ~90 and every group pair is one request (all pairs
  covered once; cross-group requests keep only cross-group links), returning
  `{a, b, kind ∈ KINDS, strength 1-3, why}`. Cache `<data>/graph.json` (beside
  `notes/`, never synced): per note `{hash, summary, concepts}`, `links` (global list); each note's `linked` is `link_key` (hash of its note
  line) when its links were made. `link_work`: notes read but not linked
  (new/changed) are linked against each other and against the settled notes
  (chunks of ~90); settled×settled is never re-asked, and old links of changed
  notes are dropped first. A failed job leaves its notes unlinked to retry.
  Older `pairs` caches are folded into `links` by `settle_old_pairs` (no
  requests). POST `/api/graph/build?fresh=1` clears and rebuilds (Rebuild). `assemble`: note nodes (summary,
  concepts), `related` edges (strongest per pair), `link` edges from `[[wiki]]`
  without AI, idea nodes for concepts shared by ≥2 notes (`covers`, shown only
  with the Ideas toggle); the manual note is excluded. `Writer` is a plain
  prompt→text fn passed to `serve` from `src/cli/mod.rs` (`ai::chat_outcome`),
  so leo-web still depends only on core. Builds run on a thread
  (`Graphs::start`); the page polls `/api/graph/status`.
  `graph.js`: pure helpers (node-tested in `tests/graph.test.js`: connections
  sorted across-classes first with direction-aware labels, `visible` with
  hidden classes / ideas / crossOnly / focus depth) and a canvas view styled on
  Obsidian's graph: Barnes-Hut many-body + link + collide + class cohesion,
  lone notes pulled in, hover fades, zoom-faded labels, edge and node tooltips,
  eased camera, `insets` keep fit and centring clear of the tools and panel
  (phone sheet `peek`/`open`). Browser tests seed `graph.json` and stub the
  build, since the default writer is Ollama.
- Links: `styled_link` wraps the URL in OSC 8 for terminals that support it,
  but not for `TERM_PROGRAM=Apple_Terminal` (no OSC 8; its own ⌘-double-click
  finds plain URLs). `open_on_enter` reads stdin lines on a thread and opens the
  local `127.0.0.1` link with `leo_core::obsidian::open_link`.
- Felix chat (`chat.rs`, `web/chat.js`): POST `/api/chat {messages, mode, note}`
  streams NDJSON (`{"sources"}`, `{"t"}`, `{"restart"}`, `{"done"}` or
  `{"error"}`) from a `Streamer` (prompt→fragments) passed in from
  `src/cli/mod.rs` (`ai::chat_streaming`). `gather` picks the open note (14k
  chars), its graph neighbours with the link reason, then `Store::relevant`
  matches; notes are tagged `n1…` with why they were included, and the manual
  is excluded. Styles chat/study change the system prompt;
  study starts a graded reply with `[[correct]]`/`[[incorrect]]`, which
  the page strips and turns into a badge and Felix's dance (3 in a row: cheer)
  or droop. Felix is pixel-exact SVG (#b4cfe7 body, #19191b eyes) animated in
  CSS: idle bob, random blinks, look-around while thinking, wave on open. The
  panel follows the open note or the map's selected note (`setContext`); the
  conversation is kept in localStorage (`leo-chat-v1`, last 40). Citations
  `[n2]` become chips that open the note.
- Settings (`#/settings`): `SettingsApi` (describe/apply/test as JSON) is
  implemented in `src/cli/web_settings.rs` over
  `leo_services::web_settings`, and handed to `serve` in `Powers` with the
  writer and chat streamer. GET `/api/settings` (+ `secure`), POST with
  `{set: provider|model|key|auto_push, ...}` returns `{message, settings}` or
  400 `{error}`, POST `/api/settings/test {task}`. Keys are write-only and only
  accepted when `secure_request` (X-Forwarded-Proto https, or a localhost /
  127.0.0.1 / [::1] Host).
- Uploads: POST `/api/import {directory, title?, files:[{name,type,data base64}]}`
  (body limit `IMPORT_BYTES`) starts a thread running the `Importer` from
  `src/cli/mod.rs` (`leo_services::import::import`) and returns `{id}`; GET
  `/api/import/{id}` is the job. The note gets a "From <files>, uploaded <date>"
  footer; originals go to `<data>/attachments/<note-id>/` (`safe_file_name`,
  never synced), listed and served by `/api/notes/{id}/originals[/{name}]`.
  The page shrinks photos to ≤2000 px JPEG before upload (also converts HEIC
  where the browser can decode it).
- Recording (`record.rs`, `web/recording.js`, worklet `web/recorder.js`;
  route `#/record[/dir]`, Record FAB, menu row): one job at a time
  (`AppState.recording`). POST `/api/record {source: browser|microphone|screen,
  directory, title?}` → 202 `{id}`; computer sources only for `local_request`
  (localhost Host and no forwarding headers). The browser's AudioWorklet
  box-averages to 16 kHz Int16 and POSTs raw LE bytes to `/api/record/{id}/audio`
  each second (held up to 120 s while offline, oldest dropped); `pause`, `point`,
  `stop`, GET `/api/record/{id}` (the page polls each second; 60 s without any
  request stops and saves, `FORGOTTEN_AFTER`). The `Listener` comes from
  `src/cli/web_record.rs`: `leo_services::session::recorder::record` with
  `Input::Fed(rx)` (or the device for computer sources), then `write_up`; leo-web
  saves the note. Browser tests stub `/api/record*` and use Chromium's fake mic.
- `leo serve` opens the 127.0.0.1 link itself when stdin and stdout are terminals
  (`should_open`); there is no flag to skip it (the user asked for none), only
  `LEO_NO_OPEN`, which e2e and Playwright set.
- Chat references: `ChatBody.refs` (note ids, at most `MOST_ATTACHED` 8) are
  gathered first ("attached by the user", 12k chars each). The page adds them
  with `@` (`mentionAt`: an @ at the start or after whitespace, no newline) or
  the paperclip (picker with its own search), shows chips (`.chat-ref`), sends
  them with every message and keeps them in `leo-chat-v1` until removed or +.
  Two styles only (`MODES` = chat, study; the user asked to merge
  ask/explain/meeting into chat and quiz/coach into study). Old ids map over in
  `mode_of` and `modeOf` (saved conversations and old pages). Felix's body is 34x28 (a box a bit wider than
  tall); the test checks the ratio, not exact numbers.
- Chat history: `chats.rs` keeps one JSON file per conversation in
  `<data>/chats/<id>.json` (`AppState.chats`, beside `notes/`, never synced):
  GET `/api/chats` (summaries, newest first), GET/PUT/DELETE `/api/chats/{id}`
  (`valid_id`: 8-64 of [A-Za-z0-9-]; last `MOST_MESSAGES` kept; title = the
  first user message unless given). The page makes the id (`newId`, randomUUID
  or a fallback for plain http), PUTs after every reply and on style/ref
  changes (`remember`, under the id the reply started in), and lists chats in
  `.chat-history` (a column at ≥900 px, remembered in `leo-chat-sidebar`; an
  overlay on phones), grouped by `groups`. Delete asks by turning × into
  Delete.
- Storage page (`#/settings/storage`, Settings → Advanced): `storage.rs`
  builds `Area`s (id, title, about, path, bytes, items, actions) for notes
  (read-only), trash (empty), chats (selected / older than 30 days / all),
  uploaded originals (selected / whose note is gone), the map cache
  (`Graphs::clear`, refused while building) and `.git` (read-only); the
  `Housekeeper` from `src/cli/web_storage.rs` adds recording sessions
  (`session::locked` ones cannot be picked or deleted), the speech model
  (`audio::models_dir`) and the config dir (read-only). GET `/api/storage`
  (`describe` adds size labels and drops empty areas), POST `/api/storage
  {area, action, items}` → `{message, storage}`; 409 `{error}` when refused.
  Item ids are validated per area. The page confirms every action in a sheet.
- Trash page: Select mode (checkboxes, `.select-bar` in `#floating` with All /
  Restore / Delete), a per-row delete, and Empty trash; every permanent delete
  confirms in a sheet. POST `/api/trash/delete {ids}|{all:true}`
  (`Store::delete_from_trash`, `empty_trash`), POST `/api/trash/restore {ids}`.
- Switching Felix's style starts a new chat (keeps attached notes) when the
  current one has messages.
- Export: GET `/api/export?uploads&chats&trash` streams a zip built by
  `export::write_zip` into a temp file (notes without dot-dirs, optional
  `.trash`, attachments, chats; never config/keys). Storage page has the export
  card and "This browser" (`saving.discardAll` clears drafts).
- Folder Select: `#/f/...` Select → pick folders (`d:path`) and notes
  (`n:id`), select bar → POST `/api/trash/move {notes, dirs}`
  (`delete_dir_recursive` + `delete_notes`, root and outside paths refused).
- Felix: SVG has heart, thought bubble, z, cheeks, sweat and a gaze group
  (`--gaze-x/y` follows the pointer). States: think (waiting), talk (streaming),
  nod (sent), dance/cheer (right: hearts, blush), droop then perk (wrong: sweat),
  tap → boop/hop/spin/giggle, sleep after 60 s idle, wake. `/favicon.svg` is
  served outside auth.
- Record sources: `browser` (getUserMedia) and `tab` (getDisplayMedia with
  audio; no audio track → refused with "Share tab audio"; the share ending
  stops and saves) are fed by the page from any origin; `microphone` and
  `screen` run on the server's devices and need `local_request`. `Source::
  is_sound` (tab, screen) sets the session's screen flag.
- Advanced page also has: signed-in browsers (GET `/api/sessions`, POST
  `/api/sessions/end {handle}|{others:true}`, new link), how long things are
  kept (GET/POST `/api/keep`; a shorter choice confirms first), and a note's
  originals as one zip (GET `/api/notes/{id}/originals.zip`, `zip_download`
  shared with export; `attachment_header` adds an RFC 5987 UTF-8 name).
- Files for Felix: paperclip menu → "A file from this device" → POST
  `/api/chats/{chat}/files {name,type,data}` → `Reader` power
  (`import::read_for_chat`: text extraction; images copied out by `ai::see`;
  clipped to `CHAT_DOC_CHARS`) → `chat_files` keeps only the text in
  `<chats>/<chat>.files/<id>.json` (`MOST_FILES` 10). `ChatBody {chat, files}`
  puts them in `<document>` tags before the notes (`DOCS_CHARS` shared).
  Deleted with the chat (`chats::remove`), files of chats never saved go after a
  day (`tidy_orphans`), and Storage has "Documents given to Felix".
- Recording opens its AudioContext with `sinkId: {type: 'none'}` so it renders
  without an audio output device (it never plays anything).
- Felix retrieval (`chat::gather`): attached notes, documents, the open note and
  its 5 strongest map neighbours, then `matches` (question words, hyphens
  folded, scored on title 3 / map concepts 3 / map summary 2 / body 1; the
  manual excluded), then up to 2 map neighbours of each of the top 3 matches
  ("connected to <title> (<kind>): <why>"). Attribute text goes through
  `attribute` (no double quotes or newlines). 64k chars in all.
- Recording wave: browser sources sample an AnalyserNode every 80 ms; computer
  sources use `RecordView.levels` (RMS per 250 ms from `Capture::level_since`,
  last `LEVELS_KEPT`). `loudness` maps RMS to −54…−6 dB; `QUIET` 0.2 ≈ −44 dB;
  `hearing` → sound / quiet / silent (6 s) with per-source advice.
- Chat panel size follows the window: full width under 600 px, clamp(380px,
  62vw, 520px) up to 900 px (Your chats overlays), then `--chat-w` =
  clamp(560px, 48vw, 820px) with Your chats as a column (clamp(190px, 30%,
  260px)); toggling it never changes the panel width. From 1200 px the page
  (and its buttons, toasts, select bar) moves over beside the open chat
  (`body.chat-open` padding), except on the map.
- Folder Select renders the normal `card` with a `pick-box` instead of the pin.
- The title placeholder is an absolutely positioned ::before, so the caret starts
  at the beginning; an emptied title is reset to truly empty.
- Every AI instruction says "Use interpretable language" (tested in services,
  import and chat).
- Record page: two kinds only (`sourceFor`): Microphone → `browser`; Screen →
  `screen` on a local page, `tab` elsewhere if `canShareSound`, else disabled.
  Cards like Settings; round iPhone-style record/pause/stop buttons.
- Wave engine (`recording.js`): one value per `WAVE_STEP_MS` (60 ms) in rAF
  (`advance`), eased by `ease` (fast rise, slow fall), drawn as gliding bars
  across the full width (empty slots are flat bars). Browser sources read the
  AnalyserNode; computer sources get `RecordView.levels` + `levels_start`, new
  ones found by `freshLevels` and spread over 60 ms steps (`spread`) into a
  queue. Silence advice needs 6 s of real quiet.
- Background panel: GET `/api/activity` → `activity_tasks` (uploads still
  working with `label`/`dir`, a recording being written, a map build). The page
  polls it every 1.5 s while anything is listed (and once on load, and after
  starting an upload, a map build or stopping a recording), and hides a task on
  its own page (record, map, the upload sheet). `#activity`, foldable.
- A recording that finishes while the page is elsewhere refreshes the open
  folder or search (`noteReady` → `showLatest`, like uploads) and toasts Open.
- Felix files go with one message: sending moves the composer's ready files to
  the message (`files` ids, `docs` names) and `state.sent`; every later request
  still sends `state.sent` so follow-ups can use them. `splitFiles` sorts a
  chat's stored files into sent/waiting on load (old messages match by name).
- "This computer" is the TCP peer, not the Host header: `serve` uses
  `into_make_service_with_connect_info`, `note_peer` puts `Peer { loopback }`
  in the request (`peer_of`; no address = not loopback), and `local_request` /
  `secure_request` require it. A forwarded `https` counts only from loopback
  (cloudflared runs on this computer). Handlers take `Extension<Peer>`, so a
  router without the layer refuses rather than trusting.
- Main search (`search.rs`, GET `/api/search`, also Felix's note picker):
  `#tag` queries go to `Store::find`; otherwise words (`flat`: hyphens
  folded) are looked up in the title (plus acronyms of 3-5 word windows, or the
  whole title), text, map concepts (word, prefix of ≥4 letters, acronym) and
  map summary. Groups: all in title, all found, then (3+ words) two thirds with
  one in the title or an idea; `Store::find`'s fuzzy title matches follow.
  Hits carry `why` = `{kind: "idea", name}` or `{kind: "summary"}` when the map
  was needed; the card shows it (`.card-why`).
- Undo after Select → Move to trash: POST `/api/trash/move` also returns `ids`
  (every note it removed) and `dirs` (every folder, empty ones too); the toast's
  Undo POSTs them to `/api/trash/restore {ids, dirs}`, which recreates the
  folders (validated) before restoring. A single note's delete already undid
  through `restore`.
