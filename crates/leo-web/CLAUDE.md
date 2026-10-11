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
  (`concat!`, order: core, trash, map, settings, storage, uploads, sheets, side, drag, diagrams, math,
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
  exactly like `Note::toggle_checkbox` (`notes::checkbox_line`: `-`/`*`/`+`/`1.`/`1)`, 1-4 spaces, then `[ ]`/`[x]`, text optional; counted inside code fences, not inside `>` quotes); tested by
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

- Knowledge graph, called "map" in code (`#/map`, `#/map/<noteId>`; sidebar
  and menu "Knowledge graph", note action "Graph"; renamed from "Map of ideas"
  2026-10-09, the user's call): web only, a note-to-note knowledge graph. `graph.rs`: reads notes in
  batches (8 notes / 18k chars) for `{summary, concepts}` (ideas and methods,
  vocabulary reused so names match), then links notes: notes are hashed into
  `group_count` groups of ~90 and every group pair is one request (all pairs
  covered once; cross-group requests keep only cross-group links), returning
  `{a, b, kind ∈ KINDS, strength 1-3, why}`. Cache in `leo.db` (tables `graph_notes`, `graph_links`, `graph_meta`; beside
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
  (phone sheet `peek`/`open`). Browser tests seed `graph.json` (brought into `leo.db` on the next load) and stub the
  build, since the default writer is Ollama.
- Links: `styled_link` wraps the URL in OSC 8 for terminals that support it,
  but not for `TERM_PROGRAM=Apple_Terminal` (no OSC 8; its own ⌘-double-click
  finds plain URLs). `open_on_enter` reads stdin lines on a thread and opens the
  local `127.0.0.1` link with `leo_core::open::link`.
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
  the paperclip (picker with its own search), shows chips (`.chat-ref`); sending moves
  them into the message and empties the box, and every later request in that
  chat sends the notes attached so far (`threadRefs`, newest first, at most 8),
  so follow-ups still use them.
  Two styles only (`MODES` = chat, study; the user asked to merge
  ask/explain/meeting into chat and quiz/coach into study). Old ids map over in
  `mode_of` and `modeOf` (saved conversations and old pages). Felix's body is 34x28 (a box a bit wider than
  tall); the test checks the ratio, not exact numbers.
- Chat history: `chats.rs` keeps one JSON file per conversation in
  the `chats` table of `leo.db` (`AppState.chats` still names `<data>/chats`, which locates the database; never synced):
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
- Switching Felix's style keeps the chat (a `.chat-switch` line says so;
  each message records its `mode`). Switching AI models mid-chat shares
  everything too: every earlier answer reaches the next model through
  `historyText`, which puts `[Written by …]`, `[What Felix did for this
  answer: …]` (steps and what they found), `[Felix asked: …]`, `[Practice
  question …]` and suggested changes before the reply text (first, so the
  server's per-turn clip keeps them); `recentNotes` also brings notes Felix
  opened or changed, not only cited ones; and `BASE` tells the model earlier
  replies may be another model's. A change of model shows "Now answered by
  …" in the chat.
- Export: GET `/api/export?uploads&chats&trash` streams a zip built by
  `export::write_zip` into a temp file (notes without dot-dirs, optional
  `.trash`, attachments, chats; never config/keys). Storage page has the export
  card.
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
  the `chat_docs` table (`MOST_FILES` 10). `ChatBody {chat, files}`
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
- Felix "Save as note": finished answers get a button; `asNote(question,
  text, sources)` makes `**Q:** question` + the answer with `[nX]` turned into
  `[[Title]]`, title = the question (≤70 chars); saved into the open note's
  folder (`setContext` carries `directory`); `m.saved` keeps the note id so the
  button becomes "Open the saved note"; app toasts and refreshes (`onSaved`).
- Study review (`review.rs`, GET/POST `/api/review`): misses are read from the
  saved chats (assistant replies starting `[[incorrect]]`; question = the last
  paragraph with "?" of the reply before the answer, answer = the user's text,
  notes = sources cited), `at` from the reply (the page stamps it) or the chat.
  Only reviewed keys are stored, in the `reviewed` table (`chat:fnv64(reply)`,
  pruned when the chat is gone), so a page re-saving its chat cannot undo it.
  Due after `DUE_AFTER_HOURS` (20). Welcome shows `.chat-review` in Study when
  any, in Chat when due; Review now switches the current chat to Study and sends `reviewPrompt` (≤5)
  and their notes attached, then POSTs `{done: keys}`.
- Pictures in notes (`leo_core::attachments`): files live in
  `<notes>/attachments/` (synced and seen by Obsidian; not a leo folder because
  folders come from notes), named `<stamp>-<slug>.<ext>`, type from the bytes
  (PNG/JPEG/GIF/WebP only, never SVG, ≤20 MB). Notes use `![alt](attachments/x.png)`
  or `![[x.png]]`. `resolve(notes, from_dir, link)`: beside the note, then from
  the top, then `attachments/<name>`; a bad `from` or anything outside is None.
  GET `/api/image?path=&from=` serves after sniffing the bytes; POST
  `/api/images {name, data}` saves. `markdown.js` turns local pictures into
  `<img class="note-img">` (outside ones stay links: CSP and privacy); `render`
  takes `dir`. Notes: paste or drop in `doc.js` (`onPictures`, `insert`), or
  the Picture action; small PNGs stay PNG, others go through `shrink`. Pasting
  on a folder page opens the upload sheet; pasting into the sheet adds files
  (`pastedFiles` names them `pasted-N.png`). The TUI shows `[image: alt]`
  (`attachments::with_placeholders` in `view/markdown.rs` `line`).
- Felix pictures: pasting into the box adds them as files (read by the AI that
  sees); chips and the sent message show a ≤240 px JPEG data-URL thumbnail
  (`pics`, checked against `THUMB` before drawing).
- Upload figures (`leo_services::figures`): pptx/docx images the slide or
  document embeds (its .rels, masters skipped), PDF images (DCT as is unless
  CMYK; Flate RGB/gray 8-bit rewrapped as PNG, predictor data as-is), filtered
  by `keep` (≥150x100, ≥40k px, ≥3 KB, stretch ≤6, used on <3 places, deduped,
  ≤24). The prompt lists them (`offered`, `PLACING`) and the AI writes
  `![caption](figure:N)`; `settle_figures` saves each once and rewrites the
  link, drops repeats and unknown numbers, adds `## Figures` only if the AI
  placed none, and photos under `## Photos`. Vector drawings in PDFs are not
  pictures and are not extracted.
- Large libraries: GET `/api/notes` takes `offset` and `brief=true` (body =
  400-char `excerpt`, plus `tasks: [done, all]`) and answers with `x-total`; a
  folder loads `PAGE` (200) at a time and an IntersectionObserver on
  `#more-notes` fetches the next page; Select all loads every page first.
  `/api/search?brief=true` returns at most `SEARCH_MOST` (300) with excerpts
  around the first match; the page draws 60 at a time. The map shows the
  `MAP_MOST` (400) most connected notes when there are more (`visible`'s
  `most`, `limited`), with a chip saying so; a focus or map search reaches any.
- Readability and looks: `flows.spec.js` `readability` measures every visible
  text's contrast (blended backgrounds and opacity) on each page in light and
  dark, failing under 3:1; that is why light `--faint` is `#86857f`.
  `visual.spec.js` + `playwright.visual.config.js` (`pnpm visual`, own leo on
  31832, seeded once, 4 projects: desktop/phone × light/dark) compare screenshots
  with `visual.spec.js-snapshots/*-darwin.png`; not run in CI.
- Felix tools (`tools.rs`, loop in `routes/felix.rs` `answer`): a text
  protocol so every provider works (Codex, Claude Code, OpenAI-compatible
  APIs, local models): the system prompt gets `TOOLS`; a reply that is
  `<tool>{"name": ..., ...}</tool>` (also fenced, bare JSON naming a tool,
  `tool`/`args`/`arguments`/`input` spellings) is run by `Desk::run` and fed
  back with `continued` (`<tool_result>`), at most `MOST_STEPS` (12) times, then
  `NO_MORE_TOOLS`. A broken call is explained to the model, not failed. `Gate`
  streams answers at once but holds back anything that could be `<tool>`; text
  shown before a call is taken back with `restart`. Tools: search_notes
  (`search::search`, excerpts), open_note, connected_notes (map links),
  edit_note and create_note, which only *propose* (`Proposal`, at most 3): the
  page shows a card and Apply POSTs `/api/notes/{id}/suggestion {find,
  replace}` (409 unless `find` is there exactly once; empty find appends) or
  creates the note. Notes found get the next `nX` ids, and `sources` is resent.
  Events: `{step, tool, found}`, `{proposal}`. The page draws steps with an
  icon, an expandable list of what was found and a spinner, and Felix holds a
  prop per tool (`POSES`: magnifier, scroll, map, paper and pen, hammer).
- How much note text Felix gets follows the model's context window:
  `Powers.room` = `leo_services::ai::felix_room()` (read per message),
  clamped to 12k-480k chars. `chat::scaled` grows every per-note share,
  documents and an opened note from the 64k defaults, and `widened` grows how
  many matches and graph neighbours come along. Replies may run to
  `REPLY_TOKENS` (16k); `MOST_STEPS` 12 tool calls, `MOST_PROPOSALS` 8 changes.
- Felix tool reliability (measured with Codex): the system prompt says the
  tools are text lines, always available even when the model's own tools are
  off; every non-final step ends with `tools::REMINDER` (models weigh the end
  most). There is no second "did you mean to change a note?" request (the
  old `NUDGE`, removed 2026-10-10): an answer is one request and is never
  taken back or asked again. Keyword guesses at intent (`wants_change`,
  `wants_plan`/`PLAN`) are gone too; pasted interview questions containing
  "make" and "improve" erased the answer and brought back "No note change is
  needed. Here are … again". Intent comes from the conversation: `BASE` says
  pasted material, transcripts and documents are material, not requests, an
  open note is context and not permission, and planning is for tasks with
  dependent steps; `WHEN_ASK`/`WHEN_AUTO`/`REMINDER` say writing or improving
  something in the chat is not a note change, follow-ups like "yes, do that"
  are, and a change is claimed only after its tool result. Measured without
  the check (Codex gpt-6-sol and Claude Code opus-5-5, 3 runs of 10 cases
  each, 60/60): "fix my note", "put a summary into my note", "make a new
  note", "yes, do that" and "fix the mistake in this note" always proposed
  the edit; the interview questions, "how would you improve…", "don't change
  my notes…", a cover letter and an explanation with a note open never did.
  Agent CLIs that print nothing are run once more before failing
  (`AgentCli::no_answer`).
- Storage has "Pictures in notes" (`pictures` area: `<notes>/attachments`,
  each with the notes that show it via `attachments::resolve`; delete selected
  or "no note uses"; names checked, files only inside that folder). The Notes
  area no longer counts that folder.
- Felix's tool manual is generated from `tools::SPECS` (purpose, parameters
  with required/optional, what it returns, an example call) by `manual()`, and
  every call is checked against the same table (`check`) before it runs; a
  mistake comes back as "That did not work:" plus that tool's spec. The manual
  also says the model may use its own web search for outside facts.
- Web for AIs without their own: `Powers.web` (`Web { search, page, needed }`
  from `leo_services::web`): `needed` is `!has_own_web(cfg)`, decided by the
  provider that will answer (first in the chain that is installed / running;
  Codex and Claude Code search themselves, so they never get leo's web tools).
  Then the manual adds `WEB_SPECS`: web_search (DuckDuckGo HTML results, ads
  dropped, links unwrapped; Wikipedia if that fails) and open_page, which only
  opens addresses web_search returned in this answer (`w2` or the exact URL),
  never local or private hosts: `web::allowed` checks the address, and the client's DNS resolver (`PublicOnly`) refuses a name unless every address it resolves to is public, so a name pointing at 127.0.0.1 (localtest.me) and redirects are covered and the vetted addresses are the ones connected to. Web
  tools run outside the store lock (`Desk::run_web`). A reply that claims the
  tools are missing (`claims_no_tools`) is answered once with `UNSTUCK`.
- Undo for Felix's suggestions: `apply_suggestion` answers with the note plus
  `before` (its old body); the card keeps `before` and the new `version` and
  Undo PATCHes `{body: before, base: version}`, so a note edited since is left
  alone (409, said plainly). Undo of a created note DELETEs it (trash). Either
  way the card goes back to Apply; the app refreshes or leaves the open note
  (`onUndone`).
- Signed-in browsers: opening the `?token=` link in a browser that already has
  a valid `leo_session` reuses it (leo serve opens the link on every start, and
  each start used to add a session). Each session keeps its `site`, decided by the
  server (`Gate::site_for`: not loopback = `lan`, loopback through the tunnel =
  the tunnel host given to `Gate::set_link`, else `localhost`; never a request
  header, which anyone could set) and the list shows `place` (this
  computer / your Wi-Fi / the link from any network). On start, `serve` calls
  `Sessions::retire_links` with the current tunnel host, dropping sessions made
  through older trycloudflare addresses, which no browser can reach again.
- Originals viewer: a chip for a PDF, picture or text original opens a
  `.sheet.viewer` (`viewOriginal` in `uploads.js`): PDFs in an iframe of
  `/api/notes/{id}/originals/{name}?view=1`, which is served inline only when
  `viewable` (PDF or a raster image; never HTML or SVG) with SAMEORIGIN and a
  locked-down CSP; pictures pan and zoom (`setViewerZoom`, 0.5-4, double-click
  2x); text is fetched and Markdown rendered. Without `view` it stays a
  download. The main CSP allows `frame-src 'self'` for it.
- With Felix open beside the page (>=1200 px), sheets, the scrim and the note
  toolbar (`.actions`) are centred in the space left of the chat too.
- Layout variables live on `body`: `--side` (sidebar width, 0 under 1000 px),
  `--right` (the chat's width while it is open beside the page, from 1200 px)
  and `--mid` (the centre between them). Fixed things (toolbar, select bar,
  toasts, record pill, FABs, activity, scrim padding, the map) use them, so the
  sidebar, Felix and the page never cover each other. The map gets
  `container: map`, so its panel (`min(380px, 52cqw)`) and tools shrink with it
  and the graph redraws through a ResizeObserver.
- Sidebar (`app/side.js`, >=1000 px): New note, Search (⌘K / Ctrl K anywhere),
  All notes, Ask Felix, Knowledge graph, Record, Note from a file, top-level
  folders with counts (+ makes one), and Trash / Storage / Settings at
  the foot. `loadSideFolders` runs after every route; `aria-current` marks the
  place. Narrowed to icons with the fold button (`leo-side` in localStorage).
  The header keeps back, crumbs (or the page name) and Felix; the ⋯ menu,
  search button and FABs are phone-only.
- Felix's width: drag `.chat-resize` on the panel's left edge (or arrow keys
  on it; double-click or Home resets). Kept in `leo-chat-width`, applied as
  `--chat-w` on `<html>`, clamped to 420 px .. leaving 380 px of page.
- Drag and drop (`app/drag.js`): note cards (`data-from`), folder cards and
  sidebar folders are draggable; folder cards, sidebar folders, crumbs and
  All notes / the brand (`data-action="home"`) take drops. A note moves with
  `/api/notes/{id}/move`, a folder with POST `/api/dirs/move {from, into}`
  (`Store::move_dir`: refuses itself, its own subfolder, a name already there;
  emptied folders on disk are removed); both toast Undo. Files dropped on the
  page open the upload sheet (into the folder dropped on, if any); on Felix
  they are read for him, and a note card dropped on him is attached.
- What an answer cost: the `Streamer` returns `Reply { text, spent }`;
  `routes/felix.rs` `answer` adds up every step (`Spent::plus`) and sends
  `{"spent": {by, model, effort, input, output, estimated, cost, plan, local,
  steps}}` just before `done`. The page keeps it on the message and
  `spentLabel` draws one line under the answer: money for keys
  (`≈` when the tokens were guessed), model · effort · "on your plan" for Claude
  Code and Codex, "free on this computer" for local models; the tooltip has
  the token counts.
- Chat titles: `chats::title_of` adds the note to a short question
  ("Explain this simply: Graph traversals"), `about` is the start of Felix's
  first answer in plain words (shown under the title in Your chats). After the
  first answer, `put_chat` asks the writing AI for a 3-7 word name in the
  background (`name_in_background`, once per chat via `NAMING`), saved with
  `named: true` so later saves keep it; the page re-lists chats at 8/25/60 s
  until it is named.
- Files in Felix are cards (`fileCard`, `fileKind`): a picture's thumbnail or
  a miniature page of the document's first lines (`chat_files` `excerpt`,
  320 chars) over a colored type badge (PDF, DOCX, PPTX, CSV, PNG, …; tones in
  `--tone-*`). Sent messages keep `cards` (name plus thumb or excerpt).
- Diagrams: ```mermaid blocks are drawn on the page (notes, Felix, anywhere
  `markdown.js` renders): `render` emits `<figure class="diagram">` holding
  the escaped source; `app/diagrams.js` watches the page (MutationObserver),
  loads `/vendor/mermaid-11.4.1.js` the first time one appears (Mermaid 11.4.1,
  vendored in `web/vendor/` with its MIT license, checked against npm's
  sha512; served `immutable`), and draws with `securityLevel: 'strict'` and the
  `base` theme fed from leo's color tokens. Drawings are cached by theme and
  source (`DIAGRAMS_KEPT`), so redraws do not flicker; answers still streaming
  (`.msg.pending`) wait; a broken diagram keeps its source and says why. Felix's
  `BASE` prompt says when and how to draw them (also into notes through
  edit_note / create_note). Obsidian and GitHub draw the same blocks.
- Felix's access (`tools::Access`, the page's `ACCESS`, kept in
  `leo-felix-access`, Shift+Tab or the pill under the box): **Ask first**
  (default; edit_note / create_note are cards to Apply), **Auto** (the route
  applies each change on the server at once through `change_note` /
  `notes::make_note`, tells the model "Changed."/"Made.", and sends the card
  already `applied` with `before`/`after`/`made` so Undo works), **Read only**
  (`manual_for` leaves the change tools out, `READ_REMINDER`, and
  the Desk refuses them).
- Citations never go into notes: `tools::linked` turns `[n2]` in edit_note's
  `replace` and create_note's `body` into `[[Title]]` (a citation of the note
  being edited, or of nothing, is dropped). `markdown.js` draws `[[Title]]`,
  `[[Title|label]]` and `[[Title#part]]` as links (`open-title` opens the note
  with that exact title, else searches).
- A message sent while a file is still being read takes it along: the card
  shows "Reading…" in the message, Felix thinks, and the request goes once
  every file is read (`reading` promises in chat.js). What the request uses
  (open note, attached notes, style, access, the answer's folder for Save as
  note) is fixed when the user presses send, so moving to another note while
  Felix works changes nothing about that answer or its changes.
- Selecting in a note works like Obsidian's live preview: a selection over
  rendered text (drag, double or triple click, Shift+click from the caret, ⌘A)
  turns the lines it covers into one source textarea with the same selection
  (`doc.js` `open(first, last, from, to)`, `pointAt` maps DOM points to text
  offsets with `rawOffset`), so Delete, Backspace or typing apply to all of it;
  a key pressed over a rendered selection does that in one step. Selections
  that neither start nor end inside the note are ignored.
- Graph builds follow the model too: `Graphs::with_room` (the same `Room`)
  gives `Scale::for_room` (bigger batches, more of each note, bigger link
  groups and output budgets for big models; smaller batches for small local
  ones) and `at_once` (3 / 2 / 1) requests run side by side per round
  (`build_at`, `in_parallel` on scoped threads; results merged in order, so
  the cache and progress stay deterministic).
- `api()` in the page treats an empty reply body as `null`, and POST
  `/api/dirs` now answers 201 `{path}`: an empty 201 once made folder
  creation fail after the folder was made. A success is JSON or a bodiless 204;
  the browser test "every successful answer from leo is JSON" sweeps the API.
- Bug classes guarded in CI (`flows.spec.js`): every test fails if the page
  throws or shows a programmer error (`guard` fixture: `pageerror`, and
  toasts / `.msg-error` matching `TECHNICAL`). The describe block "the kinds of
  bugs that reached people before" covers typing that keeps focus in every text
  box across results and saves, edits and answers that stay with the note or
  chat they started in, slow pages never drawing over the next one (every
  page bumps `seq`; Record did not), each checkbox changing only its own line,
  selections across every block kind (both directions), and double presses.
- Undo in notes: `doc.js` keeps up to `MOST_UNDO` (200) snapshots per open
  note. Typing groups into one step until a 1 s pause or 4 s of typing; any
  change bigger than one character (a deleted selection, a ticked box, a
  pasted picture) is its own step. Cmd/Ctrl+Z undoes, Shift+Cmd+Z or Ctrl+Y
  redoes, inside the editor or with nothing else focused; the restored text
  comes back selected.
- Sheets on phones (< 600 px) swipe down to close (`swipeToClose` in
  sheets.js): past `SWIPE_CLOSE` (90 px) or a quick flick; a small drag
  springs back; not from inputs or a scrolled sheet.
- The knowledge graph keeps itself current: `keep_graph_current` (in `serve`)
  checks every 30 s, and `Graphs::update_if_due` starts an update when
  `should_update`: built at least once (the first build is the user's call),
  something to do, nobody has used leo for `UPDATE_WHEN_IDLE` (2 min;
  `note_activity` marks use on every API request except polling, see
  `counts_as_use`), and not within `RETRY_AUTO_AFTER` (10 min) of a failed
  automatic try. It reloads the store first, so Obsidian or TUI edits count.
- Sidebar folders are a tree (`folderTree` over `/api/folders`): a twist
  arrow opens a folder's subfolders (open ones kept in `leo-side-open`; the
  way to the current folder always unfolds), and "+" on a folder makes a
  folder inside it (`new-folder` with `data-parent`; the sheet keeps the
  parent on `#folder-name`). Folder pages already list their subfolders.
- Screenshot baselines: `pnpm visual:update` rewrites every image
  (`--update-snapshots=all`); plain `--update-snapshots` keeps old images that
  differ by less than the 1% tolerance, which let stale baselines linger.
- No Drafts page (removed 2026-10-09, the user's call). `saving.js` still
  keeps an edit that could not reach leo in localStorage; on the next load the
  page says "Saving N edits that had not reached leo yet" and `retry()`s them,
  and opening that note shows the kept text.
- Model and effort: Settings shows one picker per AI (`modelPicker`): models
  with prices, and effort chips when `choice::efforts` offers some for that
  provider and model (Claude Code low..max, Codex low..xhigh, OpenAI reasoning
  models minimal..high, Gemini low..high). POST `/api/settings {set: "effort",
  value}` (empty = default) writes `[providers.X] effort`; the TUI's writing
  effort row steps through the same list (`choice::step_effort`).
- Felix keeps track of what a chat is about: the page sends `recent` (notes
  cited in the last three answers, `recentNotes`), gathered as "used earlier
  in this chat" right after attached notes (`gather_with`, `MOST_RECENT` 6).
  Documents given to Felix are kept whole (`CHAT_DOC_CHARS` 2M); the prompt
  shows each one's share with a "[The document goes on…]" marker, and the
  `read_document` tool reads any part (`Desk::part_chars` = room/3, 12k..120k).
- Math: `$...$`, `\(...\)`, `$$...$$` and `\[...\]` inside a sentence (display),
  and blocks of `$$`, `\[` / `\]` or `\begin{aligned|align|equation|cases|…}`
  (each one block to edit in `editing.js`; `startsBlock` ends a paragraph at
  them, since models put them right under a sentence) become `.math` slots in `markdown.js`; the
  inline rule skips prices (`$5 and $10`), `\$` and code. `app/math.js`
  loads KaTeX 0.16.11 (vendored in `web/vendor/katex/`, MIT, checked against
  npm's sha512; script, CSS and woff2 fonts served by `katex_file` under
  `/vendor/katex-0.16.11/`, immutable) the first time a formula appears, renders
  with `trust: false`, and caches the HTML per formula. The CSP allows
  `font-src 'self'`. Felix and every note prompt write math this way.
- Felix sees pictures: `captions.rs` keeps a one-time description per
  picture in the `captions` table (never synced), keyed by path
  + size + mtime so nothing is reread. `keep_graph_current` captions up to
  `PICTURES_PER_TICK` (4) uncaptioned pictures per idle check through the
  `seer` power (`ai::see`, the writing AI's vision; the lock is only held to
  list them). `gather_seeing` and `open_note` show `[Picture: …]` after each
  picture link, and `look_at_picture {note, question?}` looks at up to 4 of a
  note's pictures on demand (a plain look also stores the caption).
- Callouts: `> [!type] Title` renders a box, `> [!type]- Title` a closed
  dropdown and `+` an open one (`markdown.js`; tones per type; Obsidian
  reads the same). Taps on a dropdown's title open it rather than edit;
  printing opens every dropdown (`beforeprint`) and closes them after.
- Uploads take "What do you want from it?" (`#upload-wants`, ≤`MOST_WANTS`
  2000 chars) → `ImportBody.wants` → the `Importer`'s second argument →
  `import::import(.., wants, ..)`, which puts it in `<what_the_user_wants>`
  and repeats it at the end of every part's request.
- Felix asks and quizzes: tools `ask_user {question, options?}` and `quiz
  {kind: multiple_choice|fill_blank|free_response, question, options?,
  answer, explain?}` (`tools::asked_from` / `quiz_from` validate: a choice
  answer must be one of the options, a blank needs `___`). `run_tool` →
  `interact` sends `{ask}` / `{quiz}`, answers the model `ASKED`, and refuses
  every later call that reply (`ALREADY_ASKED`); the text loop makes the next
  step the last. The page draws `.ask-card` (tap a choice = send it; typing
  answers it too) and `.quiz-card`: choices and blanks are checked on the page
  (`checkQuiz`: case, accents, punctuation ignored; `a | b` accepts either),
  Felix reacts at once, and the user message carries `say` (what the model
  is told, `quizSay`, asking for `[[correct]]`/`[[incorrect]]`) and `quiz`
  (question, given, correct) — `review.rs` reads `quiz` for Study review. Free
  answers go to Felix to mark. Typed quiz drafts survive redraws (`drafts`).
- Steering: `/api/chat` first sends `{"answer": id}` (`steer::Steering`,
  `AppState.steering`); while Felix works, Enter (or the send button when the
  box has text) POSTs `/api/chat/{answer}/steer {text}` (202, 410 once the
  answer finished, 429 past `MOST_WAITING`). `run_tool` takes waiting
  messages after every tool step, appends them to the tool result
  (`steer::added`) and sends `{"steered": [...]}`; the page moves those before
  the answer ("Sent while Felix worked"). Ones never read are sent as the next
  message when the answer ends, or go back into the box if it was stopped.
- Citations of notes Felix was never given (`[n9]` with no source) are
  dropped from the answer rather than shown raw (`cite`).
- Long chats: `chat::turns_for(room)` keeps 14 turns word for word at the
  default room, up to `MOST_TURNS` (40) for big models. Older messages go in
  `<earlier_in_this_chat>`: the chat's `memory` summary (stored in the chat
  file, kept by `chats::save`, written under `WRITING`), then the turns it does
  not cover, shortened (`earlier_of`, `EARLIER_CHARS`). A summary is used only
  while `hash_of` the turns it covers still matches. After every
  `MEMORY_EVERY` (6) new turns beyond the kept ones, `remember_in_background`
  asks the writing AI to fold them into the summary (`MEMORY_RULES`).
- The cost line's tooltip names tokens read from the cache (`Spent.cached`).
- Meaning (`vectors.rs`, `Powers.meaning`): each note is cut into pieces of
  ~`PIECE_CHARS` at paragraphs (title first, at most `MOST_PIECES`), read by
  the meaning model in the background loop (`PIECES_PER_TICK` per 30 s, store
  lock only to list them) and kept in the `vectors` table (one f32 BLOB per
  piece, by note hash; deleted notes dropped). `close_to` embeds a question and
  returns notes whose best piece is at least `CLOSE_ENOUGH` (0.62). Felix's
  `gather_seeing(.., close)` adds them as "close in meaning to the question"
  after word matches; GET `/api/search` and the `search_notes` tool append them
  (`search::with_meaning`, `Why::Meaning`: "Close in meaning, though the words
  differ"). The knowledge graph asks only about note groups that relate:
  `Vectors::neighbours` (24 nearest per note, by mean vector, cached in
  `Graphs::near` until the vectors change) and `link_work_near` skip a group
  pair with no neighbour across it, unless a note in it has no vectors yet.
- `calculate` (`calc.rs`): Felix works numbers out instead of guessing them.
  A small safe evaluator (no code runs): lines that define functions
  (`f(x) = x^2 + 4*cos(x)`), set values, or evaluate; `if(c, a, b)`,
  comparisons, trig/exp/log/sqrt/min/max/sum/mean, `pi`, `e`, implicit
  multiplication (`2x`, `4 sin(x)`); 10 decimals; per-line errors; capped at
  `MOST_LINES`, call depth and total work. Handled in `run_tool` (no store).
  Measured on a 5-question optimization homework with Codex gpt-6-sol: the
  golden-section and Newton tables then matched ChatGPT's exactly; without it
  the values were guessed and Newton's divergence was invented away.
- Felix's `BASE` matches length to the task (quick answers stay short;
  problems and homework get full working, computed numbers, a boxed result
  and a check), follows the order asked ("start with question 1"), says
  which parts remain, and only says "Beyond your notes" when the question
  was about the notes.
- Storage lists everything leo keeps (`storage::more_areas` / `act_on_more`
  with `Kept { captions, vectors }`): Felix's chat summaries (`memory`:
  forget selected or all; the chats stay), Study review progress (`review`:
  start over), picture descriptions (`captions`: clear, made again when idle),
  the meaning index (`meaning`: clear, rebuilt locally), and "Other files
  beside your notes": every other entry in the data folder, labelled
  (`SMALL_FILES`), with only old backups (`leftover`: `.bak`, `.before-`,
  `history.txt`) deletable. Settings files (`CONFIG_FILES`) are listed only
  under the housekeeper's settings area, which now counts just those files
  (on macOS the config and data folders are the same, and it used to count
  every note). The speech-model area covers only Parakeet's folder, and the
  meaning model has its own area (`meaning-model`).
- Database (`db.rs`): everything leo-web computes or keeps besides notes and
  uploads lives in one SQLite file, `<data>/leo.db` (rusqlite, bundled, WAL,
  0600, never synced): chats (+ `chats_text`, FTS5, for GET
  `/api/chats?q=` and the "Search your chats" box), chat documents, review
  progress, picture descriptions, meaning vectors and the knowledge graph.
  Writes touch only what changed (a chat row, one note's vectors, changed
  graph rows). `db::at` keeps one connection per file; `beside(file)` and
  `for_chats(dir)` find it from the old file locations, so modules keep their
  path-based APIs. The first open brings in the old files (`chats/`, its
  `.files` folders and `review.json`, `captions.json`, `meaning.json`,
  `graph.json`) and renames them `*.before-database`; Storage lists those as
  old backups that can be deleted. A chat's time (and its place in the list)
  changes only when a message is added, not on a style switch or rename.
- Practice answers stay in their card: submitting sends a hidden turn
  (`send(.., {card})`: the `quizSay` text goes to Felix but no user bubble is
  added) and Felix's reply streams into `q.reply` inside the card; the card
  greys while marking, then gets a green or red outline (`q.correct`, from the
  page for choices and blanks, from `[[correct]]`/`[[incorrect]]` for free
  answers). A quiz or question Felix adds in that reply becomes a new message.
  `review.rs` reads wrong answers from the cards (`missed_cards`). Questions,
  choices, answers and explanations render Markdown and math (`inline`), and
  tool calls keep LaTeX backslashes (`tools::latex_kept`: JSON escapes like
  `\f`, `\t`, `\b` would otherwise eat `\frac`, `\theta`, `\boxed`).
- Esc in Felix stops an answer that is running; with nothing running it closes
  the panel.
- Sidebar folders stay open until their arrow closes them (`side.open`, kept);
  opening a folder adds the way to it, and a closed arrow stays closed while
  you are inside it (`side.shut`, cleared when you move to another folder).
- Effort has no "Default": `choice::effort_for` gives the chosen level if the
  AI offers it, else medium, and every provider uses it; the levels are low,
  medium, high and xhigh where the AI supports them.
- Marking a practice answer: the page sends `practice: true`; the server then
  runs that turn read only, so Felix never
  replaces his marking with "No note needs changing". Study and `quizSay`
  ask for the next practice question as a quiz card, never plain text, and the
  verdict marker must come first; `grade` / `verdict_off` still accept it after
  a short lead-in (`VERDICT_WITHIN` 240 chars) and drop the lead-in.
- Text written before a tool call is kept: native sessions and the text
  protocol now treat a tool call as a paragraph break (`gap`), not a
  `restart`. Restarts remain only for leo's own retry (UNSTUCK) and a
  provider failing over mid-answer. This is what made a marking flash and
  vanish when the model wrote it and then called `quiz` for the next card.
- Measured with real agents (Codex gpt-6-sol and Claude Code opus-5-5) on the
  practice tree (start, then wrong/right/right/wrong and right/wrong/wrong/right):
  every round kept its reply, carried the right verdict and gave the next
  card. Claude Code sometimes fails to parse its own tool call
  ("could not be parsed (retry also failed)"); leo falls back and recovers.
- Felix never shows a model hiccup as a failure: if an agent session breaks
  (even after showing text), the route sends `{"reset": true}` (the page
  clears the text, steps, cards and unapplied suggestions of that answer),
  resets `desk.asked`, and redoes the turn through the text protocol, up to
  `FALLBACK_TRIES` (2). A marking request with `mark: true` (a free answer)
  whose reply carries no `[[correct]]`/`[[incorrect]]` gets one short `JUDGE`
  call and a `{"verdict"}` event the card uses. The tool manual tells models
  never to call leo's tools through their own tool calling (Claude Code
  sometimes did and failed to parse it). Stress-tested: 10 runs of the
  practice tree on Codex and Claude Code, 50 turns, every verdict and next card
  right; two Claude Code hiccups were redone cleanly.
- CI runs the browser suite with one retry and Playwright's `github`
  reporter, so a flaky test is reported as flaky with its name and a real
  failure shows up as an annotation readable through the public check-runs
  API (the job logs need sign-in).

### Recordings kept, calendars, combining and tidying

`routes/sources.rs` serves a note's retained recording sources (with version checks),
corrections, and regeneration previews (`{wants}`), drawn by `web/app/sources.js`
(the note's Transcript action). `record_journal.rs` persists sequenced browser PCM
before acknowledging; the page spools unacknowledged chunks in IndexedDB via
`audio-queue.js` (in memory where there is no IndexedDB). A disconnected browser is
left recoverable, not auto-finalized. Call PCM is interleaved microphone/system
audio and split before transcription.

What the user wants: the Record page's "What do you want from the notes?" box goes
in `StartBody.profile.wants`; while recording, the live box saves after a pause
(POST `/api/record/{id}/wants`, live only) into `Controls.wants`, which the
recorder copies into the manifest; `long::with_profile` puts it in
`<what_the_user_wants>` and restates it last. It is kept in the `Archive` and
prefilled when regenerating.

Calendars (`calendar.rs`, `ics.rs`; Settings → Calendar): the user pastes a
calendar's secret iCal address (Google: Settings → the calendar → Integrate
calendar → Secret address in iCal format; webcal:// becomes https://). Google
sign-in (OAuth) was removed 2026-10-10 because setting up a Cloud client was too
hard; its old files and stored secret are deleted on the next GET. The link is
a secret: only accepted on a `secure_request`, kept through the root's
`CalendarAccess` adapter (`default_store()`, account `calendar-link-<id>`), never
written to a file or sent to the page. Fetches go through
`leo_services::web::fetch_public` (https, public addresses only, `MOST_BYTES`).
`ics::read` unfolds lines, reads events with their time zone (`chrono-tz`;
unknown zones = this computer's), skips all-day and cancelled events and declined
guests, and expands DAILY/WEEKLY(BYDAY)/MONTHLY/YEARLY repeats with
INTERVAL/COUNT/UNTIL, EXDATE and moved instances (RECURRENCE-ID); other rules
show the first time only. Bounded: 20k events, 5k steps, 200 shown. Up to five
calendars (`calendars.json`); events for the next 14 days in
`calendar-cache.json`, refreshed in the background when older than 15 minutes.
The Record page preselects the event happening now (or starting within 15
minutes), names the recording after it unless a title was typed, suggests
meeting-style notes in the wants box, and sends the event's place, organiser,
guests and description as `profile.context`.

Combining notes: dropping a note card on another (`drag.js`) opens a sheet; POST
`/api/notes/{into}/combine {with}` asks the writer (`combine::SYSTEM`: keep every
fact, say repeats once) for a preview, puts back any picture, link, code block,
math block or checkbox the reply dropped (`combine::missing`, under "Also in the
original notes"), rewrites relative picture links from the other folder, and
reports how much wording survived (`kept`, warn under `KEPT_ENOUGH`). Saving
(POST `/combined`) checks both versions, replaces the target body and trashes the
other note; the toast's Undo restores both.

Knowledge graph "Tidy up" (`graph.js` `tidyLayout`): reseeds by class with a
breadth-first spiral, runs `TIDY_TICKS` of the same forces, swaps nearby nodes
while it reduces edge crossings (`crossings`, within `TIDY_BUDGET_MS`), polishes,
then glides there.

Live recording page is notepad first (from Granola): "Your notes" (points list
and a big box; Enter adds a point) comes right after the clock and controls, then
what you want from the notes; the live transcript is folded in `#rec-heard`
("Show what's being heard", open state kept in `leo-rec-heard`).

Felix reads a recorded note's transcript with `read_transcript {note, find?,
part?}` (read only, any access): lines `[m:ss] (speaker when more than one)
text` and the user's typed points, searched by `find` or paged by
`part_chars`; notes with a kept recording say "Made from a recording" in their
`<note>` block (`tools::has_transcript`).

Browser audio upload pace: the worklet posts 0.1 s pieces; each send (every
second) groups them with `leoAudioQueue.batches` into uploads of at most
`UPLOAD_SAMPLES` (32000) and drains up to `UPLOADS_PER_SEND` (60). Sending only
three 0.1 s pieces a second (as briefly shipped) starved the live transcript.
The note toolbar shows Transcript only when GET `/api/notes/{id}` says
`recorded`. Record, the Transcript sheet and Settings → Calendar use the
Settings vocabulary (`set-card`, `set-row`, `set-input`, `btn sm`, `set-note
warn`), not one-off styles.

Sidebar: the places nav and the foot stay put; only the folder tree
(`.side-tree`) scrolls, and its scroll position survives redraws.

Recording journal writes and commit callbacks run outside the recording mutex; an
atomic reservation prevents Stop/recovery from overtaking a chunk or point write
(points, wants and Stop wait for it rather than failing). Transcript edits
serialize outside the store mutex and reject stale versions. A failed worker still
accepts queued recovery audio.

Recovery replays through bounded channels with four chunks per track. Its job stays
in writing state, rejects fresh microphone uploads, and exposes JSON acknowledgments
for recovery and Finish available. Replaying a long archive must not queue all its
PCM in memory or reopen the microphone.

Offline browser backlogs persist individual worklet chunks, never one oversized
concatenation. A partial storage failure advances only durable sequences and keeps
only unwritten chunks in memory for retry; it must not duplicate stored audio.
