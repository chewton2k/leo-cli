# leo-web: leo's engine and the page

leo-web depends only on leo-core. Everything that needs services (AI, speech,
settings, import, storage housekeeping) arrives as a `Powers` field built by
`leo_host::powers()` (writer, chat streamer, settings, importer, listener,
reader, seer, meaning, web, room, speech, toolless, toolless_chat).

## Layout

- `lib.rs`: `Powers`, `AppState` (+ `with_store`, `store_now`, `fresh`),
  `serve`, `router`. Handlers live in `routes/<area>.rs`; items are
  `pub(crate)` only when another file uses them. Handler tests are in
  `src/tests/<area>.rs` (helpers `state_with`, `run`, `json_of`, `host` in
  `src/tests/mod.rs`).

- `/app.js` is `web/app/*.js` joined in one closure by `routes/assets.rs`
  (`concat!`). The parts share scope on purpose (pages reassign `state`). Add
  a new part to the list: `every_part_of_the_page_script_is_served` fails
  otherwise and `the_page_script_parses` runs `node --check` on the whole.

- Pure helpers are separate files served on their own and node-tested from
  `tests/markdown.rs` `run_node_tests`: `markdown.js`, `editing.js`,
  `schedule.js`, `saving.js`, `audio-queue.js`,
  `recording.js`, `margin.js` (tests in `tests/*.test.js`).

- Everything leo-web computes or keeps besides notes and uploads lives in one
  SQLite file, `<data>/leo.db` (`db.rs`; rusqlite bundled, WAL, 0600, never
  synced): chats (+ FTS5), chat documents, review progress, picture captions,
  meaning vectors, note connections, calendar feeds, note-task metadata,
  reading positions. Old JSON files are brought in on first open
  and renamed `*.before-database`. Writes touch only what changed.

## Serving and security

- `start(StartOptions { port, token }, powers) -> Running` binds `127.0.0.1`
  only (the requested port, else the next free of 19), spawns the graph and
  idle tasks and the server; `Running::{port, sign_in_url, back_up, stop}`.
  `stop` drains requests for at most `shutdown::GRACE` (5 s) and clears warm
  sessions within `CLEANUP`. Tests use `start_in(Places { store, sessions })`.
  The app passes `fresh_token()`; `leo-test-server` (Playwright) uses
  `saved_token()`, the `<config>/serve-token` file the specs read.

- Only the app's window talks to the engine (2026-10-31, the user's call):
  `same_host` refuses any Host but `127.0.0.1:<port>`/`localhost:<port>` (421),
  `?token=` on GET `/` → 303 + a `leo_session` cookie (`SameSite=Strict`, 30
  days, `serve-sessions.json`). No tunnel, Wi-Fi bind, QR, signed-in browsers
  list or link rotation; keys and calendar links are accepted from the window.

- Page CSP: no inline JS (`script-src 'self'`), `data:` images, `frame-src
  'self'` (original viewer), `font-src 'self'` (KaTeX). Mermaid 11.4.1, KaTeX
  0.16.11 and CodeMirror parsers are vendored under `web/vendor/` (checked
  against npm's sha512, served `immutable`).

- Keys and calendar links are write-only: never returned to the page. A stored
  key shows as `secret::masked` placeholder; nothing changes until Save.

## Page architecture

- Hash router (`#/f/dir`, `#/n/id`, `#/new/dir`, `#/search/q`, `#/trash`,
  `#/calendar`, `#/boards[/id]`, `#/record[/dir]`,
  `#/settings[/storage]`), `data-action` delegation (`actions.js`). Every
  page bumps `seq`; a slow load whose `seq` changed draws nothing.

- Native quit confirmation keeps the current page and uses its existing toast
  (`events.js`); Updates explains that every successful installation reopens Leo.

- Notes read wide (2026-10-19, the user's call): `.note-columns > .note` uses
  `--note-width`, 980 px by default; Settings → Appearance → Note width picks
  Comfortable (760 px), Wide or Full width, kept per browser in
  `leo-note-width` and set early by `/theme.js` as `html[data-note-width]`.
  Width checks measure the reading column; its outer wrapper also reserves the
  outline gutter. Full width fills the remaining column. Reading tests scroll
  over a note block, independently of the explorer’s own scroll area.

- Quick Leo (2026-10-19, the user's request): Cmd/Ctrl+F
  anywhere, the sidebar
  Search button, the header search button or the phone menu opens one line
  (`sheets.js` `quickLeo`, toggled by `toggleQuickLeo`). It is also the search
  bar (2026-11-03, the user's call): every query offers "Search every note",
  which opens `#/search/<query>`; only that results page shows `#search`. It
  drops in from the top on desktop. Escape is always `preventDefault`ed in a
  capture listener, so it closes sheets and never leaves macOS full screen. It works like Spotlight
  (2026-10-19, the user's call): no title, modes or buttons, near the top of
  the screen on desktop. `schedule.quickRank` infers the top result from what
  is typed (`quickParse` dates and times: a reminder, Undo; a question: Ask
  Felix in a new chat, `chat.askNew`; a note title from `/api/search`: open
  it; a folder name from `/api/folders`: open it; a page name in
  `QUICK_PLACES`: go there; a short line starting with a doing word (`TODO`:
  "buy milk"): a task; "note:": a new note; anything else goes to Felix, who
  works out what was meant and can add tasks or notes himself; no AI runs
  while typing) and lists the other choices under it, with matching past
  chats (`/api/chats?q`, `chat.openSaved`); Enter runs the highlighted row
  (on the top row it first waits up to `QUICK_WAIT_MS` for the lookup of what
  was typed, so a fast Enter still opens the note), arrow keys move. Empty, it lists what is overdue or due
  today with boxes to tick. Lookups are scoped to the open sheet
  (`inQuick`), since a closing one can still be in the page.

- Felix's chat list: the × turns into Delete on the first press; pressing
  or focusing anywhere else turns it back (`spare`, in place, so the press
  still reaches what was clicked).

- Back from Settings, Storage or Trash returns to the page before them
  (`places.before`, kept on `hashchange`; Storage opened from Settings goes
  back to Settings), so a note you were reading stays where you left it.
  When the page Back goes to is the previous history entry (each entry keeps
  its position in `history.state.leoAt`; in-place URL rewrites keep it), it
  steps back with `history.back()` instead of adding an entry, so the
  browser's or phone's own back does not return to Settings (2026-10-20).

- `api()` treats an empty body as `null`; every success is JSON or a bodiless
  204 (a browser test sweeps the API).

- Layout variables on `body`: `--side` (sidebar, 0 under 1000 px), `--right`
  (Felix and/or the original pane), `--mid`. Fixed things (toolbar, select
  bar, toasts, FABs, `#dock`, the map) use them so nothing covers anything.
  While Felix or an original file is open beside the page (at least 240 px from the left),
  the window's own scrollbar is hidden (`html.page-bar-on`) and `.page-bar`
  (core.js) draws one at the left edge of Felix (and the original pane), so
  the notes' scrollbar sits beside the notes; the page still scrolls the
  window, so reading positions and paging are unchanged.

## Notes, editing and browsing

- No editor view: `editing.js` splits the body into blocks; `doc.js` renders
  each with `markdown.js` and swaps a tapped block for a textarea of its raw
  lines. Like Obsidian's live preview (2026-11-02, the user's call), nothing
  jumps when a line opens: a heading keeps its size (`data-head`) and a list or
  task line keeps its rendered list container and edits only the text after its
  marker; checkboxes, numbers, indentation and wrapping stay in place. The hidden
  Markdown prefix is added to every source/selection offset and retained on save.
  Select-all and multi-line undo selections keep the blocks rendered. A newly
  focused source editor retains the rendered text height (scrolling internally
  for longer inline Markdown); input resumes normal height fitting. Heading
  source prefixes hang left of the rendered text position. During a selection
  dragged out of an editor, its source text keeps the editor's geometry and
  scroll position in a selectable div until the selection clears; clicking it
  resumes the same textarea. Other blocks stay rendered. The drag anchor is
  captured before mouse movement, since macOS can report `selectionDirection`
  as `none` for a backwards range. Dragging carries on across the lines (`dragAnchor`), Shift+click selects,
  copy and cut give the Markdown, and typing, Enter, Delete or paste replace it
  through the raw lines, then only the caret's line stays open.
  Autosave PATCHes 700 ms after the last change with `base` = the loaded
  `version` (FNV-1a); 409 → "<title> (conflict from phone)". `saving.js`
  keeps edits that did not reach leo and retries them on the next load.
  Undo: up to `MOST_UNDO` snapshots per note (typing grouped by pauses).
  Checkbox clicks update the live input and task style without redrawing the
  document. Finishing another edit resolves the clicked source line after shifts
  and preserves the checkbox’s viewport position and focus.
  Cmd/Ctrl+A selects the whole note at once, even from inside one line
  (2026-10-19; it used to take a second press).

- `markdown.js` escapes first, numbers checkboxes exactly like
  `Note::toggle_checkbox`, draws `[[Title]]` links, callouts (`> [!type]`,
  `-`/`+` dropdowns), ```mermaid (`diagrams.js`), math (`math.js`), ```svg as
  a sandboxed `<img src="data:…">`, and local pictures (`/api/image`; outside
  images stay links). `interactive: false` makes task boxes read-only.

- Picture sizes (2026-10-19, the user's request, like Google Docs): a
  picture's width is kept in the Obsidian style, `![alt|400](path)` or
  `![[file|400]]` (`markdown.js` `sized`, 16-4000 px, a `width` attribute).
  Clicking a picture in a note frames it (`doc.js` `frame`) with its width and
  a corner handle: drag, arrow keys (Shift for 100 px) or double-click to
  reset; the change is written by `editing.setPictureWidth` and saved like any
  edit. It counts only pictures the page draws (not outside links or
  `code`), so the size lands on the picture that was framed. Double-clicking the picture edits its Markdown; Escape closes only the
  frame.

- Upload names (`safe_file_name`) drop trailing dots and spaces and dash
  Windows device names (`CON-.pdf`); originals already stored under older rules
  still open (`stored_file_name`). The page labels shortcuts with `ON_MAC`
  (Ctrl on Windows) and monospace falls back to Consolas.

- Pictures live in `<notes>/attachments/` (`leo_core::attachments`, sniffed
  PNG/JPEG/GIF/WebP only, ≤20 MB); paste or drop in `doc.js`.

- Library (`browse.rs`, `app/browse.js`): Grid (default) or List. The first
  card is New note (`newCard`, 2026-10-19, the user's call; a small dashed
  button on its own row, not a full tile), so the floating
  New note button only shows in an empty folder (Record and Upload stay). A folder
  shows only the notes stored directly in it (`dir`, empty = top level).
  **Order and filters** (2026-10-12, the user's call): newest edit first by
  default, pinned always first; Sort = Last edited (`updated`), Oldest edit
  (`oldest`), Newest made (`created`), Title A–Z (`title`), Z–A
  (`title-desc`); Show = All, Pinned, Open tasks (unticked boxes), Edited this
  week (`show` on GET `/api/notes`, applied before paging, so `x-total` is the
  filtered count). View, sort and show are remembered together in
  `leo-library`. A filter with no matches says so with Show all notes.

- Paging: `/api/notes?offset&limit&brief=true` (400-char excerpt, `tasks:
  [done, all]`, `x-total`), `PAGE` 200 with an IntersectionObserver; previews
  are rendered lazily and cached (`PREVIEWS_KEPT`). Browsing never calls an AI.

- Folders: Select mode (`d:path`/`n:id`) → POST `/api/trash/move` with Undo
  (`/api/trash/restore {ids, dirs}`); rename `/api/dirs/rename`; drag and drop
  (`drag.js`) moves notes and folders (`Store::move_dir` refuses itself, its
  own subfolder, or a taken name), files dropped open the upload sheet.
  Dropping a note on another opens Combine (`combine.rs`: the AI writes one
  note, leo puts back anything it dropped). It runs in the background
  (2026-10-14, the user's call): POST `combine` starts a job on a thread
  (`AppState.combines`, at most 20, kept 6 h) metered as `combine:<id>`
  (streamed characters against the two notes' length); the Background panel
  shows its percent, then "Ready to review" (`Task.ready`, no spinner), and a
  toast offers Review. The review sheet saves through POST `combined`
  (version-checked) or discards (DELETE `/api/combines/{job}`). Saving also
  copies the other note's recording transcripts over, kept separate and
  labelled (`recording::copy_to`, `Archive.from`, "Recording n · from
  “Title”", read_transcript); Undo restores the body and the note and removes
  the copied transcripts (POST `uncombine`).

- Sidebar (`side.js`, ≥1000 px): a 52 px icon rail holds Leo tools, with
  Settings pinned below (Trash and Storage live inside Settings since
  2026-11-02, the user's call; the rail shows Settings for them). The rail has no
  Notes button (2026-11-03, the user's call: notes open from the explorer; the
  library is reached from the breadcrumbs' first crumb or the leo name). Buttons retain accessible names,
  tooltips and active states, with space between controls and unboxed shortcut
  hints. The separate explorer shows nested folders and
  individual notes. Folder labels and chevrons toggle their children; note rows
  open notes directly. The selected folder scopes New note, Record and imports;
  library cards and breadcrumbs retain folder navigation. The explorer narrows
  below 1200 px to leave room for notes, originals and Felix. Its right edge is a
  keyboard-accessible resize handle; `leo-side-width` remembers the expanded
  width while the 52 px rail stays fixed. `pane-resize.js` shares bounded sizing,
  32/96 px arrow steps, Home/double-click reset and drag cleanup with Felix's
  Chats divider (`leo-chat-history-width`); shrinking a window clamps the visible
  width without replacing the saved preference. Overlay chat lists have no grip.
  There is no Top level row (2026-11-02, the user's call: the tree already lists
  top-level notes); the pinned Folders heading is the top-level drop target, and
  folder and note rows retain drag data. Drops work like a file explorer
  (2026-11-02, the user's call): every explorer row means its folder (the folder
  row and chevron, its open children `.side-kids`, a note row means the note's
  folder, empty tree space the top level), so a note dropped on a note in the
  explorer moves there (combining stays on library cards). The destination
  folder's row gets `drop-ok` and its open children `drop-in`; holding a note or
  folder drag over a closed folder for `SPRING_MS` (600) opens it for that visit
  (`side.revealed`, never saved); the tree scrolls near its
  edges; moved folders keep their open state (`moveOpenFolders`). A redraw can
  replace the dragged row, so `dragend` is also caught on the source element,
  a pointer move with no button ends a drag, and drops check the drag's type. A folder row is highlighted only on its
  folder page; on a note page only the note's row is. The rail, explorer, tabs and
  segmented controls are not text-selectable.
  The sidebar head and page toolbar share `--bar-height`, including the safe-area
  inset, so their rows stay aligned with the explorer open or hidden (no rule
  under either since 2026-11-04).
  Settings uses the same height for its sticky navigation offset.
  `leo-side` hides only the explorer, keeping the rail. The fold slides
  (2026-11-02, the user's call; the one width animation): `--side` is an
  `@property` length that transitions while `body.side-folding` (`MOTION_MS`),
  the explorer keeps its width and is clipped, Reduced Motion snaps. Folding ends
  after the actual `--side` transition settles, including interrupted or reversed
  transitions; a timer cannot finish it early. Animation tests pause the native
  transition in the click event, before browser protocol latency can let it end.
  The toggle
  never moves: left of the head over the rail, or on macOS fixed right after the
  window buttons, in both states. Folders start closed;
  `leo-side-folders` keeps only folders the user opened with their own clicks
  (2026-11-03, the user's call), so the explorer reopens after a restart as
  they left it (the old `leo-side-open`, which also held every folder browsing
  unfolded, is dropped once). Opening a note or folder unfolds its parents for
  that visit only (`side.revealed`, rebuilt whenever the folder or open note
  changes), never into the saved set. Modelled on VS Code and
  Obsidian (2026-11-03, the user's call): the Folders heading has Collapse all
  (`collapseFolders`, saved closed); the open note's row is scrolled into the
  explorer once when it first appears (`revealNoteRow`, the explorer only, never
  the page, skipped while the explorer is hidden, so later redraws keep the
  user's scroll); and a fresh launch reopens
  where the user left off (`leo-last-place`: the last note, folder, board,
  calendar or board page, restored only when a new window session
  opens on the library, never on reloads or in-app navigation). It is kept after
  every page that drew and when the window hides or quits (`pagehide`), since a
  new note's id and a deleted note's redirect rewrite the address without a
  hashchange; a missing note resets it to the library. Tree and rail scrolling, focus and note pages survive redraws.
  Notes use the existing bounded metadata endpoint: 100 per page, four concurrent
  reads with ten-second aborts, 64 cached folders and 32 automatic visible reads.
  Further expanded folders offer Show notes. Late reads check page generation and
  entry identity, and failures offer Try again. Phones keep the existing ⋯ menu.

- Search (`search.rs`): title words and acronyms, text, graph concepts and
  summaries, then meaning (`vectors.rs`, `CLOSE_ENOUGH`); hits carry `why`.

- Reading position (`/api/notes/{id}/reading`): saved 2 s after the user
  scrolls, restored once on open unless something else moved the page.

- Related notes (`/api/notes/{id}/related`): graph links then `[[links]]`.

## Felix (`chat.rs`, `routes/felix.rs`, `tools.rs`, `web/chat.js`)

- POST `/api/chat` streams NDJSON (`answer`, `sources`, `t`, `step`,
  `proposal`, `ask`, `quiz`, `plan`, `spent`, `reset`, `done`/`error`). The
  user prompt is `<selected_passage>` (when given), `<documents>`, notes,
  the chat, then `<today>` (date, time and IANA zone, `chat::today_line`).

- Warm sessions (`warm.rs`): up to three recent conversations, sharing a
  480,000-character context ceiling. Switching chats keeps eligible sessions;
  same-chat history/style/access/config mismatches replace only that chat.
  Settings changes clear all sessions. Sessions expire after ten idle minutes,
  and deletion releases that chat. Eviction and process cleanup happen outside
  the slot mutex and away from async request workers. Only unchanged history plus the last reply/new message can
  continue; follow-ups carry new/changed material with stable source numbers.
  Study marking can reuse a session when its Read only access key matches.
  Failed sessions fall back with a fresh
  read cache; provider context accounting still bounds each retained session.

- Idle compaction (2026-11-02, the user's request; usage drained fast): after
  `COMPACT_IDLE_MS` (10 min, warm.rs `IDLE`) without a message, the page POSTs
  `/api/chat/compact` with the model history; when at least
  `COMPACT_LEAST_CHARS` (3,000) lie before the last `chat::COMPACT_KEEP` (4)
  turns, the server drops that chat's warm session and summarises them in the
  background (`remember_in_background`, the quick model). `chat::kept_from`
  starts the next prompt at a summary's end when it covers more than the usual
  window (hash-checked), so a chat picked up after a pause sends the summary and
  the last turns instead of every recent turn on a cold cache. The chat on
  screen is unchanged.

- Long chats: `turns_for(room)` keeps recent turns verbatim, older ones go in
  `<earlier_in_this_chat>` with a background summary (`MEMORY_EVERY`).
  History carries who wrote each answer, what Felix did, cards and passages,
  so switching models mid-chat loses nothing.
  The message being answered, and the newest earlier message longer than
  `TURN_CHARS` (a pasted assignment), are kept up to `MESSAGE_CHARS` (32,000),
  and Felix's newest reply up to `LAST_REPLY_CHARS` (12,000); both reach full
  size at `FULL_ROOM` (32,000, the most `felix_room` gives) and shrink with a
  smaller room, never below `TURN_CHARS` (4,000, every other turn). Warm
  follow-ups (sized by the room, documents by their own share, as in a cold
  prompt) and messages sent while Felix works (`steer::MOST_CHARS`) use the
  same cap (2026-10-21: a pasted homework spec lost its question at 4,000).
  `clip_turn` closes a code fence it cut through (``` or ~~~, with the
  opening fence's own length), says how many characters
  are left out and tells Felix to ask for the rest as a file.

- Styles (`MODES`): chat and study. Study marks critically (a right
  result by wrong reasoning is `[[incorrect]]`), asks about the reasoning
  step by step, mostly free response and fill-in, and treats subjective
  material as criteria and trade-offs. Practice cards (`quiz`, `ask_user`)
  are answered in the card. Marking never waits for a Felix reply
  (2026-10-19, the user's call: it was slow and used the plan): multiple
  choice is marked by the page and shows the quiz's `explain` (required for
  multiple choice: why the right one is right and each other wrong);
  fill-ins that match an accepted answer are marked by the page; anything
  else goes to POST `/api/quiz/mark` (the quick model, `MARK`, JSON verdict
  and feedback). The card then offers Next question and Ask Felix why
  (`q.held`); Felix is only told the result (`heldSay`, "do not repeat it")
  when the user goes on, or with their next message. Order (2026-11-02, the
  user's call: asking why jumped to the next question): Ask Felix why sends
  `quizSay(..., stay)` (explain, no new question) and the card keeps Next
  question under the reply; a typed message while a card is held tells the
  result once (`q.told`), says "do not ask a new practice question unless I ask"
  and keeps the button; Next question after that sends only "Next question,
  please." A new question card clears Next question on every told card, so an
  old button cannot ask a second question later. Next question shows as a slim `.msg-next` divider (the `chat-switch`
  pill), not a user bubble, and a card-only reply draws no empty paragraph.
  Study's style starts from the open/attached material without being set up,
  asks for self-contained questions that test one important idea, climbs from
  understanding to use to transfer, adapts to right/wrong answers, and stops
  after marking or answering a question about an answer. If the quick check is
  unavailable, Felix marks it as before (a `JUDGE` call when the verdict is
  missing). `quiz_from` shuffles multiple-choice options and refuses a quiz
  whose right option is clearly the longest (`gives_itself_away`). Study sessions: `study_plan` / `end_session`, quizzes
  carry `idea`, the page scores ideas and stops at `MOST_SESSION_QUESTIONS`.
  Study review (`review.rs`) brings back missed questions after 20 h.

- Tools: a text protocol (`<tool>{…}</tool>`) so every provider works, plus
  native sessions where available; generated from `tools::SPECS` (`manual()`,
  every call `check`ed; mistakes answered with the spec). At most
  `MOST_STEPS` (12) steps and `MOST_PROPOSALS` (8) changes. Claude Code
  (leo-services' MCP bridge), isolation-capable Codex and compatible Ollama
  models get native tools. Unsupported native paths use the
  text protocol. A hand-written call that is not valid
  JSON is repaired when it can be (`tools::repaired`: raw line breaks, quotes
  that do not end a string, trailing commas, missing closing braces) before it
  is refused; a refused call shows as "Retried open note: <reason>" (or
  "Rewrote a tool call: <reason>" when it was not readable), the reason
  inline (`why_it_failed`). Steps say what was read ("Read part 2 of 5 of
  “X” (lines 40–80)"), a read the cache found unchanged says "again
  (unchanged)", and the same step several times in a row shows once with
  ×N (`stepRuns`; 2026-10-20, the user found six identical "Opened" lines
  unclear). Text written
  before a tool call is kept (a paragraph break, not a restart). A broken
  agent session sends `{"reset"}` and is redone through the text protocol
  (`FALLBACK_TRIES`). Web tools only for AIs without their own web
  (`web::allowed`, public addresses only).

- Read tools: search_notes, open_note, connected_notes, read_document,
  read_transcript, read_original, look_at_picture, calculate (`calc.rs`, a
  safe evaluator), list_schedule, list_trash, search_chats, list_boards,
  open_board.

- Change tools: edit_note / create_note (cards; Apply POSTs
  `/api/notes/{id}/suggestion`, 409 unless `find` is there exactly once),
  and the organizing tools in `routes/organize.rs`: add_task (`repeat`:
  `mon | wed`), update_task (`on` = which day of a repeating task),
  delete_task, move_note, make_folder, move_folder, delete_note,
  rename_note, rename_folder (`new_name`), pin_note, restore_note,
  restore_folder (trashed notes from that folder and its subfolders),
  delete_folder (always a card, even in Auto), and the board tools in
  `board_tools.rs`: add_card, update_card, move_card (`column`, `to_board`,
  `position` top/end/a card), delete_card, add_board (`title`, `columns` split
  by `|`), change_board (`new_name`, `columns`, `move_to`; Undo restores the
  board snapshot) and delete_board (always a card). Boards, columns and cards
  are named by id or exact name ignoring case; the board may be left out when
  there is only one. They validate against the
  store, become `Proposal` cards, run through POST `/api/felix/act` (reusing
  the normal routes) and return the inverse card as `undo`. The page redraws
  an open board after one applies (`BOARD_KINDS`). add_margin_note (`quote` must
  be in the note; Undo is `remove_margin_note`) redraws the open note's margin
  (`MARGIN_KINDS`); it never changes the note. The text manual stays under
  14,500 bytes. `MOST_PROPOSALS` counts every change tool.

- Steering: messages sent while Felix works are added after the next tool
  step (`steer.rs`). Esc stops an answer, or closes the panel.

- Panel look (2026-11-04, the user's call; plan phase 1): the panel sits one
  tone step off the note (`--raised`; its own `--field`/`--field-hi` mixes for
  the composer, user bubbles, pills, popups and the Chats column) with no
  left border, header rule, footer rule or column rule; a soft shadow shows
  only where it overlays the note (under 1200 px). The Chats column is closed
  until opened (`leo-chat-sidebar` still remembers an explicit choice;
  `leo-chat-history-width` and the grip are unchanged). The header holds the
  history toggle, face, `Felix` over one subtitle line (`#chat-context`: `On
  “Title”`, `Problem n · title · tab`, or `Looks through your notes`; its title
  attribute says the connected notes are read too), a style button
  (`#chat-mode-btn`, a `.chat-popup` menu with the Chat and Study
  `menuitemradio`s and, while a note is attached, `Stop reading this note`,
  which is what the old context bar's × did; Arrow keys, Home/End, Escape back
  to the button) and New/Close. Felix's words (greeting, answers) use
  `--voice`; the user's messages stay sans in a `--field` bubble. The greeting
  and starters arrive once per chat and style (`data-hello` key): words fade
  in (`.word`, `word-in` .5 s opacity and colour, no blur, nothing moves),
  starters are pills that appear together after it. `draw(stick, live)` no
  longer replaces the transcript on every streamed chunk: `settle(from, to)`
  keeps every node that already matches, appends only the new words of a
  growing text run (animated while the message is pending) and replaces only
  what changed, so finished text, a selection in it and the greeting animation
  survive; the final redraw settles too. The panel slides on `--ease-drawer` /
  `--t-drawer` and leaves over `CHAT_LEAVE_MS` before `chat-open` is removed.
  Hovering or focusing a line-citation chip calls `onCite(id, lines)`; the open
  note's `doc.glow(from, to)` toggles `.blk-glow` on the cited blocks (no
  redraw, no scroll, no timer) and clears on leave, blur and close.
  A citation to another note does nothing extra.

- Files for Felix (`chat_files`, ≤10 per chat, text only, deleted with the
  chat), passages (Ask Felix on a selection, ⌘⇧E; no bottom Ask button), Add to note
  (`routes/insert.rs`, version-checked, request ids dedupe double presses),
  Save as note, chat history (`chats.rs`, local naming), cost line
  (`spent`), drawings (Mermaid for flows and charts, SVG otherwise).
## Felix in the margin (`margin.rs`, `routes/margin.rs`, `web/margin.js`, `app/margin-notes.js`, 2026-11-04)

- Storage: `margin_threads`, `margin_modes` and `margin_state` (prefs, the day's
  read count, notes seen missing) in leo.db, never in note files or git. Threads
  carry `quote`, `prefix`, `suffix`, `status` (open, resolved, dismissed), `source`
  (read, ask, chat) and `messages`. At most `MOST_OPEN` (24) open threads per note,
  held inside the INSERT and on reopening, and `MOST_CLOSED` (48) closed ones.
  Replies update messages only, and only while the thread is open.
- Deletes: a note in the trash keeps its threads, so a restore finds them. Deleting
  from the trash removes them at once (`forget_notes`). The hourly `sweep_store` is
  for what vanished another way: it waits `SWEEP_AFTER_DAYS` (7) after first seeing a
  note missing, in case it was a partial sync or a rename, and never runs while no
  notes loaded or a file is unreadable. Export adds `leo/margin.json` with Chats,
  without trashed notes' threads unless Trash is chosen; Storage has one delete-all.
- Modes: `ask` (When you ask, the default) or `along`. A note's own row wins;
  without one, the global default applies only to notes leo made (a v4 id, so never a
  headerless file) after it was set (`Prefs.since`), so turning it on never starts
  reading old notes.
- Read-along (`POST /api/notes/{id}/margin/read`, `{version, blocks}`): the page
  sends changed blocks after a pause (4 s, slow 8 s). Silence is the normal answer:
  `{}`, empty or unparsable output is success. The server refuses, whatever the page
  does: a stale version (409), a note not in `along` or not edited in this engine
  run (403, `Reads::edited` is set by the PATCH route, so browsing can never read),
  a reply streaming (`Steering::any_open`), 5 minutes after a read note on that note,
  90 s between reads of a note, overlapping reads and the daily cap (429 with `why`).
  The cap is spent in one transaction after the read is claimed, so failures spend it
  too, and a broken day row counts as spent. Blocks already read are dropped. A failed
  read forgets its blocks and answers `{failed: true}`, so the page keeps its baseline.
  A refused read waits for the next edit. The model's quote must be in the note now,
  or the note is dropped.
- Reads use `Powers.toolless`, never `quick`: the quick path gives Claude Code
  WebSearch and Codex its web tool, so `no_tools` passes `--tools ""` and
  `web_search="disabled"`, and the chain is cut to its first provider, so a read fails
  closed instead of falling back to a cloud AI. Replies and asks use
  `Powers.toolless_chat` on the normal model. Note text sits between tags with a
  random suffix and the prompts say it is never instructions; thread messages are
  JSON records inside those tags, one per line, so stored text cannot close the block
  or forge a turn. Model text is only ever set with `textContent`. Settings says text
  stays here only for a loopback address and a model without `cloud` in its name
  (`ai::reads_stay_here`).
- Daily cap: 40 (20, 40, 80). Every read starts a fresh process (`claude -p`,
  `codex exec --ephemeral`; no warm session, and one retry on a stall), with at
  most 8,000 characters in and 220 tokens out. 90 s between reads means 40 reads
  take an hour of writing at least, and a day's worst case is about 90k input
  tokens, three Felix answers (31.8k each). CLIs ignore `max_tokens`, so output is
  bounded when parsed.
- A failed or missing AI stays silent in the note and leaves `margin::PROBLEM`
  (fixed text, no provider detail) in Settings → Felix and Health until a read works.
- Selection: Ask Felix (⌘⇧E) opens a draft in the margin, POST
  `.../margin/threads` (no read-along needed), or Ask in chat. With Felix open, under
  3 or over 600 characters, it goes to the chat as before. The server anchors on
  prefix + quote + suffix exactly and never falls back to another occurrence. Replies
  POST `.../threads/{id}/reply`, bounded (`MOST_REPLY`, `MOST_MESSAGES`, the last 8
  messages sent), and never run twice at once for a thread.
- Add to note uses `/insert` with one request id per thread (`/insert` dedupes on note
  and request, whatever the base), runs once at a time and shows Undo before it closes
  the thread. After a 409 it re-finds the passage, or says it went to the end and
  skips the glow. Open in chat attaches the quote. `add_margin_note`'s Undo is refused
  once the user has replied. `chat::gather_seeing` adds open threads of the open note
  after the notes (`margin::context`, at most 3,000 characters and an eighth of the
  note room); the block's fence is keyed per process and stable while its content is,
  so prefix caches still work, and warm sessions get it only when it changes
  (`Warm.margin`).
- Layout (`margin.js`, pure): `find` (exact, then between prefix and suffix of at
  least 6 characters, or an edge of the note, then lost), `stack`, `sides`,
  `arrangement`. Right margin needs 928 px of column, Both 1,192 px. The margin is
  reserved up front on every note, in both modes, whatever its threads
  (2026-11-04, the orchestrator's decision): `.note-columns[data-margin]` pads the
  column, so the note is min(its note width, the room left) and sits left of centre,
  and a first note cannot reflow text being typed. The layout is set in the same task
  as the first paint; `leo-margin-layout` remembers the setting per browser for that
  moment, the server keeps the truth. Marks only in Settings, Full width, too little
  room or Felix open reserve nothing and centre the note. There, a mark above the end
  of the passage (24 px to hit, 44 px on touch) opens the note inline, with
  `.margin-room` holding the space under its block. A lost anchor goes to `#chat-lost`
  in the panel.
- The mode chip is in `.note-meta` from the first paint, disabled until the note's
  mode arrives, so the row never grows. A note is a labelled article with one
  `.margin-toggle` button (`aria-expanded`, `aria-controls`) over it.
- Overlay: `.margin-layer` sits in `.note-columns`, outside `#doc`, redrawn on
  ResizeObserver and edits. `doc.spanRects` measures rendered blocks and open
  textareas without touching the caret. Elements are kept by thread id and the reply
  box is never rebuilt. Felix's `think` face shows only while a read runs, after
  200 ms so a refused read never flashes. Squiggle .46 s, tether .56 s, words
  start with the box, 62 ms apart, at most 650 ms in all (`wordDelay`). Reduced Motion stills all of it. Never prints.
- Tests: `src/tests/margin.rs` drives the real engine over TCP with fake powers.
  `leo-test-server` takes `LEO_TEST_FAKE_AI` (the test binary only, never the app) so
  one flow in flows.spec.js joins the page to the real routes.

## Uploads, originals, recording

- Uploads: POST `/api/import` runs the `Importer` on a thread; "What do you
  want from it?" goes in `<what_the_user_wants>`. Several files make a note
  each by default (2026-10-20, the user's call: one note per file is what
  people expect): `each` runs them one after another in one job
  (`run_each`), each named from its own file, with its own original; a file
  that fails is listed in the job's `error` and the rest are still made
  (`notes` lists them, the page opens the folder). "Make one note from all of
  them" sends them together as before and shows the title field again.
  Adding to a note (2026-11-03, the user's request): on an open note the sheet
  has a segmented **Add to this note / New note** switch (`#upload-where`).
  Files dropped on the note default to Add, the rail's Note from a file to New;
  Add hides the folder, title and one-note-each choices. POST `/api/import`
  with `into` (a note id, 404 if gone; never `each`) does not touch the note:
  the finished job carries `section` (`## <title>`, the written text and the
  From/uploaded line), and the page puts it at the end through the
  version-checked `/insert` (`addSection`: the open editor's version after
  `ready()` saves its draft, one retry on 409, Undo in the toast), as Add to
  note does, so typing during a long upload never makes a conflict copy.
  Originals go beside that note under free names (`free_original_name`:
  `slides (2).pdf`), so an earlier upload's file and citations stay; if they
  cannot be kept the section still goes in and the job says so. Try again on
  a failed add keeps Add. In the note, a drop of only
  pictures still goes in as pictures (`doc.js`); a drop with any other file goes
  to the sheet with every file. Originals are kept in
  `<data>/attachments/<note-id>/` (never synced); figures from pptx/docx/PDF
  are offered to the AI (`leo_services::figures`). Originals open in
  `#source-pane` beside the note on desktop (1000px and wider), with Felix
  always to its right. Both panes share the available width instead of falling
  back to a popup when chat is open. The original divider supports dragging,
  Arrow keys and Home/End; resize limits leave room for notes and chat. File
  switching keeps drafts/carets intact, close releases the iframe and returns
  focus, and a sequence guard ignores late loads. A narrow Felix pane folds its
  history into the existing Chats/Back controls without clearing the draft or
  changing the saved history preference automatically. Phones use the existing viewer
  sheet above chat (PDF iframe, zoomable pictures, rendered text); an already open
  desktop pane becomes full width when the window narrows. Chat state is retained.

- Recording (`record.rs`, `recording.js`): new microphone/screen/call captures
  run in Rust through the listener. `nativecall` is the native dual-track
  source; old browser/tab/call sources and journals remain readable for
  recovery. The page starts/stops/pauses and shows levels/transcripts; it never
  opens a browser microphone or screen-audio picker. Native jobs have no page
  heartbeat lifetime. Housekeeper's `pending_recordings` supplies retained
  native sessions; recovery starts in Writing state and supports Finish available.
  Browser recovery still replays IndexedDB/journal audio through bounded channels.

- Screenshots (the user's call): paste a PNG/JPEG/GIF/WebP into Your notes while
  recording instead of a browser screen picker. The existing snapshot endpoint
  timestamps and retains it, derives MIME from bytes and optionally reads its
  text through the writer. Late encoding/responses cannot attach to another
  recording; ordinary text paste stays normal.

- PDF export (Share → PDF, or the browser's print): print CSS hides every
  fixed overlay (dock, tasks, source pane, toasts, chat, FABs…), zeroes the
  layout variables and the scrollbar gutter, and uses fixed `@page` margins
  (22/24/24 mm), so the PDF is the same on any screen; `beforeprint` also
  marks anything else `position: fixed` with `data-print-off`.

- Background panel and Tasks panel share `#dock` (bottom left; activity on
  top).

## Note connections (`graph.rs`; "map" in storage)

Embeddings, summaries, concepts and note links remain shared by search, related
notes and Felix, and refresh when idle. The graph page, canvas renderer and
`/api/graph*` routes were removed; connection caches stay intact.

## Calendar and tasks (`calendar.rs`, `ics.rs`, `tasks.rs`, `routes/tasks.rs`, `app/calendar.js`, `schedule.js`)

- Calendars: up to five secret iCal links (Settings → Calendar), kept through
  `CalendarAccess` (`secret::default_store`, private file or optional keychain),
  never sent to the page. Feeds are cached in
  leo.db with ETag / Last-Modified; `ics::read` expands repeats, EXDATE and
  moved instances, all-day items are dates. GET `/api/schedule?from&to&live`
  (≤43 days); the page refreshes every 2.5 min while visible.

- Standalone tasks are files: `<notes>/.leo/tasks/<id>.json`
  (`tasks::tasks_dir`), one JSON line each, written atomically, so they go
  into the notes' git backup. `.gitattributes` merges them by union and the
  reader takes the line with the newest `updated_at`, so a task changed on two
  computers never conflicts. Deleting writes a tombstone line (`deleted`),
  never removes the file (no modify/delete conflicts). Rows from the old
  leo.db `tasks` table move into files once (`migrate`).

- Note tasks: checkboxes under an Action items heading (or To do, Next steps,
  Deadlines) plus promoted ones (`note_tasks` in leo.db, located by text and
  neighbours; never the wrong box). A date change is written into the
  checkbox (409 with a preview when the old date cannot be found).

- Repeating tasks (2026-10-12): `repeat` = weekdays 0 (Mon) … 6 (Sun), picked
  with M T W T F S S bubbles in New task and the task sheet; `date` is when it
  starts (today if unset); `done_on` keeps the days ticked (≤
  `MOST_DONE_DAYS`). PATCH `{done, on}` sets one day. Never overdue.
  One day moves on its own (2026-11-03, the user's call): `moved` maps the
  occurrence's original date to `{date, time, end}` (≤ `MOST_MOVED`, oldest
  dropped; absent when empty, so old files are unchanged). PATCH
  `{occurrence, date, time, end}` writes it (a move back to the series slot
  removes it); `on` stays the original date, so done days follow the move.
  `tasks::occurs_on` (Felix's list_schedule) and `schedule.occurrences` apply
  it; the .ics adds EXDATE plus a RECURRENCE-ID event per move; the Google
  link carries the weekly rule only. Felix's `on` still means done only.
  `schedule.plan(tasks, events, today, {from, to})` expands occurrences; the
  Tasks panel shows the next open one; ICS and Google get a weekly RRULE.

- Event sheet (2026-10-19): `schedule.eventDetails` splits the event
  context into place, organiser (a Google group or resource calendar ID reads
  "A shared calendar"), invited (chips) and description; `schedule.richText`
  renders the description from its HTML keeping only line breaks, bold,
  italics, lists, paragraphs and https/mailto links (`noopener`), dropping
  scripts, styles, images, attributes and other tags, and links bare URLs in
  plain text.

- Calendar page (2026-11-03, the user's call: a full screen like Notion
  Calendar; design in docs/superpowers/2026-11-03-calendar-page-design.md):
  Day, Week and Month (`leo-calendar-view`; D W M, J/K, T keys) in normal flow
  beside the rail and explorer (never `position: fixed` over `--side`; a
  narrow window's peek is fixed but starts at `--side`). Week/Day is an hour
  grid (`CAL_HOUR_PX` 48, 15-minute snap) with an All day row of spanning bars
  (`schedule.lanes`), side-by-side overlaps (`schedule.dayColumns`) and a now
  line; Month shows three chips a day and "n more" (Day view). The right tray
  leads with Tasks, Overdue and No date before the mini month. Narrow windows
  keep this task tray above the calendar instead of hiding it, with its own
  bounded scroll area and no mini month. Pointer drags (boards' rules: 5 px
  or a 300 ms touch hold, capture on `#app`, Escape cancels, the click after
  a drop is swallowed) move a task (Month: day only), resize a Leo task's end
  by its bottom edge, schedule a tray task, or sweep empty time into an inline
  composer (a click gives an hour; a month or all-day cell an untimed task).
  Saves are optimistic with rollback and Undo; a 409 goes through `saveDate`;
  repeating tasks ask Only this one / Every time (Every time shifts `date` and
  the weekdays by the day moved). Polls that land mid-drag or mid-compose wait
  (`cal.stale`). Clicking an item opens a side peek in the tray's place
  (`#cal-side.peek`, 340 px): title, done, date/start/end with a This one /
  Every time scope for repeats, repeat days, type, a description that autosaves
  600 ms after typing and on blur, note, Google, .ics, Delete; events are
  read-only. Colors follow the event title (`--hue-0..7`) and task kind
  (`TASK_TONES`). "Hide completed" is remembered (`leo-calendar-hide-done`).
  Add to Google Calendar opens Google's prefilled page (no sign-in).
  Creation tolerates two minutes of pointer rounding only near drawn hour lines,
  floors other clicks to their quarter hour and clamps starts inside the day.
  Date edits refresh Google status and export actions without replacing focused fields.
  A save resolves the current task snapshot after a calendar refresh. Tests:
  `tests/browser/calendar.spec.js` (real CSS and calendar.js over an in-memory
  API, a fixed 284 px explorer; desktop and webkit), `tests/schedule.test.js`.

- Export adds `tasks.json`; Storage → Restore tasks takes that file and POSTs
  `/api/tasks/import` (ids dedupe; a newer local copy is kept).

## Boards (`boards.rs`, `routes/boards.rs`, `app/boards.js`, 2026-11-02)

- Storage mirrors standalone tasks: `<notes>/.leo/boards/<b…>.json` (name, columns
  `{id, name}`, 1-12) and `<notes>/.leo/cards/<c…>.json` (board, column, `rank`,
  title, subheading, description ≤8,000, priority `""|low|medium|high`). One JSON
  line per save, atomic, nofollow reads, union-merged in `.gitattributes`
  (`leo_core::sync`), newest `updated_at` wins, deletes are tombstones; a file
  counts only when its name is its id. One file per card, so two computers editing
  different cards both keep their edits. Limits: 100 boards, 5,000 cards.

- Order is `rank` (f64). A move writes only the moved card (midpoint of its new
  neighbours, `boards::between`) unless the gap is gone, then `settle` renumbers that
  column. A card whose column was deleted elsewhere shows in the first column and
  is rewritten only when it moves. One mutex (`boards::writing`) serialises writes.

- API: GET/POST `/api/boards` (list with `cards` counts; a new board has To do, In
  progress, Done), GET/PATCH/DELETE `/api/boards/{id}` (PATCH `name`, `columns`
  `[{id?, name}]` in order; removed columns' cards go to `move_to` or the first
  column, appended in order; DELETE returns `{board, cards}`), POST
  `/api/boards/restore` (that body: Undo), POST `/api/boards/{id}/cards` (`id` +
  `rank` for Undo, else end of `column`, or `top`), PATCH/DELETE `/api/cards/{id}`,
  POST `/api/cards/{id}/move` `{board?, column?, after?}` (`after` missing = end,
  null = top, an id = just after it; another board keeps a same-named column, else
  the first). Export adds `leo/boards.json`.

- New board uses the existing sheet and controls, with a labelled full-width name
  field and a small preview of the three starting columns; Enter and Create board
  submit the same request, and Cancel leaves the boards untouched.

- Page: board tabs with counts (a card dropped on a tab moves to that board, with
  Undo), the name and column names edit in place (Enter saves, Escape reverts),
  columns are grey lanes of white cards (title, muted subheading, priority arrows,
  a lines icon when there is a description), + Add card opens a composer that stays
  open for the next card (adds are queued so fast typing loses nothing), + Add
  column, ⋯ sheets for a column (rename, move left/right, delete moving its cards)
  and the board (rename, add column, delete with a second press, Undo). A card opens
  in a side panel beside the board like a note (2026-11-02, the user's call: no modal;
  `#kanban-panel`, kept across board redraws, a full-screen layer under 760 px): title,
  subheading, status, priority (`library-views`), board, a borderless description
  (autosave after 600 ms, on blur, and before Escape or × closes it), Delete with Undo.
  Columns share one height and fill the width (flex 1, at least 260 px; Add column is a
  small + tile), cards are roomier, and each column takes a quiet colour from its name
  (`kanbanTone`: to do blue, in progress amber, done green, blocked red, others by
  place) shown as a dot, its count and a 3% tint. No help line under the board.

- Dragging is pointer events, not HTML drag and drop: mouse/pen lift past 5 px,
  touch after a 300 ms hold (a swipe still scrolls). The real card is the dashed
  placeholder moving through the lists; a copy follows the pointer; the board
  scrolls sideways and the page vertically near edges; pointer capture is on `#app`
  (moving the card in the DOM would drop capture). It ends on pointerup, cancel,
  lost capture or a move with no button; Escape puts it back; the click after a
  drop is swallowed; leaving the page mid-drag cancels. Columns drag by their name
  bar. Keyboard: Enter opens a card, Alt+arrows move it. After a move the board is
  fetched again so renumbered ranks are known. The Tasks panel hides on Boards.

- Tests: `src/tests/boards.rs`, unit tests in `boards.rs`, `tests/browser/boards.spec.js`
  (fixture: real CSS and boards.js over an in-memory API; desktop and webkit) and
  flows.spec.js (real server: drag, reload, API contract).

## Settings, Storage, export

- Settings (`SettingsApi`, `leo_host::WebSettings`): one model picker
  per AI with prices and effort chips.

- Theme (2026-10-14, the user's request): Settings → Appearance picks Match
  this device, Light or Dark, kept per browser in `leo-theme`. Theme, Felix and
  Note width are segmented controls (icons `device`, `sun`, `moon`; Felix pictures
  with `felix-preview cute-look` / `demon-look`, which the mood code skips and
  which look the same whatever mode is on). Each has a row reset (`settingReset`,
  a small icon button that shows only while the value differs from
  `APPEARANCE_DEFAULTS`: Match device, Cute, Wide; `data-action="setting-reset"`,
  named "Reset <setting> to default"). Add one only where the default is real. `/theme.js`
  loads in `<head>` (before any paint, no inline script under the CSP) and
  sets `html[data-theme]`; the dark colours apply under
  `prefers-color-scheme: dark` unless `data-theme="light"`, and always with
  `data-theme="dark"`. Anything drawn in JavaScript asks `leoTheme.dark()` and
  redraws on `leoTheme.onChange` (graph, map dots, Mermaid diagrams), never
  the media query directly. Print overrides with `:root:root` so a PDF is
  always light.

- Storage (`storage.rs`, `storage_saved.rs` + the `Housekeeper`): every area leo keeps, with size,
  items and confirmed actions; signed-in browsers; how long trash and chats
  are kept (`/api/keep`); Export (zip of notes, optional uploads, chats,
  trash; never config or keys); Restore tasks.

## Testing

- Per the user (2026-10-11): no local Cargo builds for website work and no
  full local runs; quick JS checks and one-test runs only, CI does the rest.

Task descriptions are optional plain text (4,000 characters), empty by default.
New task and the task sheet reuse set-input textareas with an empty placeholder;
saving a description keeps the sheet and caret intact and offers Undo. Standalone
descriptions travel with task files, restore/export and Calendar links; checkbox
descriptions live in note_tasks metadata without rewriting note content. Felix's
list_schedule includes descriptions as quoted data; add_task/update_task preserve
missing descriptions and an explicit empty update clears one. delete_task removes
standalone tasks through the same Ask/Auto/Read and Undo flow. Delete Undo restores
the original id, description, repeat progress and note link with a fresh timestamp
so the restore wins over its deletion marker in a backup merge. Conversation history
and its search results display titles only, with no response preview or tooltip.

### Security and cleanup boundaries (2026-10-16)

Sign-in files use unique synced 0600 temporary files and atomic replacement;
loading rejects symbolic/hard links, nonregular files and oversized data, and
repairs existing permissions. Session persistence failures refuse authentication.
Original attachments and standalone tasks check every component inside their data
root, use nofollow regular-file reads and atomic writes, and refuse symbolic links.
Legacy task migration removes only rows whose matching task was safely retained.

Database handles are cached up to 32 paths, migration markers up to 64; filesystem
checks/open/close occur outside the database cache mutex. Live browser PCM queues
hold 64 chunks per track; cancellation-aware sends have a ten-second deadline and
run outside the recording mutex. Failed delivery keeps the journal for recovery
and stops an incomplete recording from becoming a note. Audio/point workers retain
their save guard through I/O and in-memory acknowledgement even if the HTTP client
leaves. Recording warnings retain at most 40 entries of 2,000 Unicode characters.
Storage validates complete recording selections before deletion and rejects linked
recording, originals and picture roots. Legacy writing CLI profiles appear only
when saved data exists; metadata-only controls offer explicit confirmed cleanup
without deleting credentials during provider migration.

Writing Settings offers Ollama, Codex and Claude Code. Removed AGY/Copilot
choices and sign-in commands are unavailable; legacy saved profiles remain in
Storage until explicitly cleared. Speech choices are local only; speech key controls are absent. Credentials never reach the page or note exports.

Successful Felix replies may contain only proposal, ask or quiz cards. Stopped
means cancellation; normal empty completion offers retry guidance. Saved replies
retain client elapsed milliseconds and a cancellation flag, including card-only
turns, so future slow turns can be distinguished from canceled replies.

`Running::stop` drains active requests and streams for at most five seconds,
then aborts the server task; graph/idle tasks stop first, and warm-session
cleanup has a one-second deadline.

Editor instructions distinguish Leo's initial Ask/Auto/Read permissions from the
writing CLI's read-only filesystem sandbox and defer to updates during a reply.
The existing 512-character refusal check also recognizes later direct editor
refusals while excluding quoted diagnostics, fenced code and blockquotes. This
improves recovery; Luna can still refuse an ordinary edit request intermittently.

Felix keeps automatic note retrieval separate from warm conversation capacity:
reducing note excerpts must not evict a supported long conversation early.
Tool manuals omit repeated examples and formatting boilerplate while preserving
all parameter/return contracts; actionable error recovery retains full examples.
All permitted tools remain discoverable without keyword-based intent filtering.
The shared ADHD-inspired `REPLY_STYLE` is unchanged by prompt-size optimization.

`open_note` exposes bounded `part` reads, reporting total parts and the original
first line so smaller initial excerpts never make later note content inaccessible.
Parts preserve every numbered Unicode character, including long single lines.

Study's next `quiz` or `end_session` call may carry the previous answer's structured
`verdict`, used only on a marking turn. A missing text verdict can use that field
without another call; conflicting verdicts retain the hidden check. Hidden grading
accepts only the exact verdict words. Permission and interactive-card rules remain.
`usage::Usage` keeps content-free counts in memory since server startup, separated
by answer, continuation, recovery, grading, summary, other writing and quick review.
Settings shows token/cache counts, unreported/estimated usage, tool failures/reused
reads, and median/p95 timing from at most 64 samples per category. Native agents
may make additional internal requests; these are not subscription-balance estimates.

A hidden check resolving conflicting Study verdicts is authoritative for the card
and its hidden history marker; the streamed explanation stays intact. An invalid
check leaves the answer ungraded, and the inconsistent warm session is released.

Note combining sizes source parts by the writing provider’s graph context budget
(at most 20,000 characters per source and 40,000 per request), up to 480,000
input characters and 128 requests. Fenced code and display maths
stay whole; oversized protected sections pass through verbatim. Parts are joined
without a growing full-note remerge, and every part uses the existing preservation
checks. Two combines may run at once, with a 15-minute between-request deadline;
any failed part leaves both source notes unchanged. Review, version checks,
transcript copies and Undo still apply. Saving admits bounded Unicode JSON bodies.

Notes with Markdown headings have an outline (its disclosure is an icon and chevron with no "On this page" label; 2026-11-02, the user's call), generated locally from
editable line blocks (up to 300 headings, excluding code, maths, quotes and tables).
It uses source-more controls and theme tokens. With 900 px of room (2026-11-02,
the user's call: notes sit in the middle), the note is centred between two 48 px
gutters and the left one holds a tick per heading (`.outline-ticks`, longer for
higher levels, the current one in the accent). Hovering or focusing the rail
magnifies nearby bars and previews the heading and a bounded plain-text excerpt
(up to 24 lines / 800 UTF-16 units, escaped); clicking or Enter/Space jumps there.
Arrow keys, Home and End select a preview, Escape dismisses it, and Reduced Motion
snaps the bars. All 300 markers fit within the viewport. The existing disclosure
below the rail opens the full nested outline; narrow panes keep their disclosure.
The wide rail is vertically centred in the reading viewport, including short
notes. Its dropdown reveals and retracts with a 180 ms clip/translate animation,
reverses from its current frame on repeated clicks, and snaps with Reduced Motion.
Preview updates never redraw the editor, and destroy removes the rail and tooltip. Note
width preferences still cap the reading column. Headings stay on one line with
ellipsis and their full title on hover. The outline hides its scrollbar while
keeping wheel, touch and keyboard scrolling available. The outline gutter is
reserved before a heading exists, so adding or removing
the first heading never shifts or narrows the writing column. A narrow pane or
phone reserves a fixed disclosure row; its expanded outline overlays the note.
Outline toggles and original-file open/close controls focus explicitly so
WKWebView preserves keyboard focus when their markup changes.
Heading jumps scroll smoothly (instantly with Reduced Motion), keeping the URL and reading-position flow; typing refreshes only the
outline after a pause, preserving the editor and caret. A width observer refits
the active textarea in place when panes resize, clearing internal scroll so text
and the caret stay visible; it disconnects with the document. Scroll/resize observers,
frames and timers are released with the document. The outline never prints.

Stopping a line editor reconciles blocks in place: unchanged source blocks retain
their DOM, including rendered math, diagrams and open callouts. Source editors
keep the rendered block height and margins until input so code, tables, quotes
and math do not move the lines below when clicked.
Pointer activation never scrolls an already visible part of a block into view;
keyboard navigation still reveals its editor. Rendered/source caret conversion
uses UTF-16 offsets throughout, matching DOM ranges and textarea selections even
when a line contains emoji.
Selection endpoints on list markers stay within the edited item's source range.
Programmatic insertion commits source-selection mode first and retains the
existing append behavior, so picture insertion never uses a detached textarea.

Writing AI shows detected Codex/Claude accounts with bounded CLI sign-in checks,
5-hour/7-day meters, reset times and the age of saved usage. Detection never
changes the chosen provider; unknown or absent limits are stated explicitly.
PDF export keeps Windows' native print function (Tauri calls it internally),
uses the native bridge on macOS, and reports bridge errors without losing the
note. A completed export cannot rename a newer page.
The browser runner requires a fresh disposable home and points both coding
providers at missing fixture binaries before startup, so autodetection cannot
inspect the developer's signed-in accounts.

Combine’s chunker and dropped-content recovery share fence recognition: a closing
line needs the opener’s character and at least its full length, with no trailing
text. Shorter fences inside a longer block stay literal code, and following prose
is never misclassified as a second code block.

Outline headings with children have separate chevrons with aria-expanded and
aria-controls. Folding hides only descendants; the heading itself still jumps to
its section. Folds survive typing by heading text/occurrence, and a collapsed parent
gets the current-section highlight when reading inside it. Groups follow real
heading levels, including skipped levels, and stay bounded by the 300-heading cap.

`Powers.speech` implements service-free `SpeechApi::{view,change}` over
GET/POST `/api/speech`, behind the session gate with bounded JSON input.
Handlers run model work outside async/store locks and return JSON for every
success and actionable errors. The initial model card uses existing settings
components; progress refreshes only that card, leaving other settings fields
and their carets intact. `native-recording.test.js` covers native source choice
and clipboard screenshots; it runs with the recording node tests.


`Powers.dictation` implements `DictationApi::{view,change}` over session-gated
GET/POST `/api/dictation` (200 KB input, blocking work off async tasks). Desktop
provides settings, devices and native actions. The existing database owns the
`dictations` table: insert and twenty-row eviction share a transaction; Copy
resolves a stored ID and discards page-supplied text; Clear removes local rows.
Nothing becomes a note or sync file. Card polling updates shortcut status and
history only; fields, drafts, focus and caret survive. There is no Save button
(2026-11-02, the user's call): every change saves at once and Custom words saves
700 ms after typing stops, in order (`saveDictation` chains saves so the last
wins), with a brief "Saved" in the card header. Local popup rendering
and settings helpers have node tests in `dictation.test.js`.

Settings look (2026-11-02, the user's call, stageshow's craft rules; redone
2026-11-04): group labels in sentence case, no "Settings" title in the section
list (the bar says it), no card title repeating the page title, labels on one
line with controls hugging their content at the right edge; a card under 500 px
stacks segmented rows (container query), and switch rows stay inline on phones.
`.set-card` is a tinted group (`--tint`, no border, no shadow, no rule between
rows: space separates them; inputs, selects and plain buttons inside it fill
with `--raised`). Inputs keep their border; nothing else in Settings' own layout
has a 1px rule (the Trash list, Storage items, Calendar form and menus included).
Selection is ink (segment and nav pills are `--raised`, switches `--text`);
indigo stays for focus, links and the search pulse. Writing AI uses a pen icon,
never a sparkle.
Settings (2026-11-02, the user's call) is a sidebar beside the content. Since
2026-11-04 the sidebar has no card: it sits directly on `--ground` and the
section on `--page`, with no border between. From 1000 px the ground runs the
full window height, up through the header too: `fitSettingsEdge` (run on each
frame and on `resize`, which the explorer fold dispatches when it settles)
measures the nav's right edge and the section's left edge as
`--settings-c`/`--settings-lc` (offsets from half of `--side`, so the band
follows the fold), `body.settings-edge` then widens `body::before`, moves
`body::after`'s rounded corner to the edge, clears the bar and layout
backgrounds and aligns the crumbs with the section. Below 1000 px the older
`.settings-layout::before` band is used. Sticky under the bar
with its own scroll; nav and section are centred together at any width (a
four-column grid, the outer columns `1fr`); every card 20 px from the next (a
section's wrapper `<div id>` is `display: contents`, so its cards share that gap);
grouped Leo / Your notes / Data / This app, each section with an icon and a page
heading with a one-line description; under 760 px the nav is a bar on `--ground`
with the search above a sideways-scrolling chip strip that centres the current
section (results replace the strip while searching).

Settings search (2026-11-04, after T3 Code): `SETTINGS_INDEX` in
`settings-navigation.js`, next to `SETTINGS_SECTIONS`, is plain data of
`{section, label, keywords, id}`, one entry per setting a person can name. Its
`id` is the `data-find` attribute on that row or card in the page markup;
`settings-navigation.test.js` fails when an entry names a section that does not
exist, a duplicate id, or an id no source draws. Add the entry and the
`data-find` together. Filtering is local and case-insensitive (every typed word
must appear in the label or keywords; a label that starts with the query comes
first). The input is a combobox with `aria-activedescendant` over a listbox of
rows (label, then the section as a second line); arrow keys move, Enter picks,
Escape clears (and is swallowed while a query is typed), an empty result says
so, and the query lives in `settingsFind` so redraws keep it. Choosing a result
sets the route if needed and `seekSettingsTarget` waits (MutationObserver on
`#app`, 2.5 s, then falls back to the page heading) until `[data-find]` exists
inside that section's own element, so the placeholder-then-content double draw
cannot lose it; then it waits for the arrival animation, scrolls the target to
the centre, focuses it (`tabindex=-1`) and pulses it once: a 650 ms opacity pulse
on `::after` in `--accent`; Reduced Motion skips the pulse and shows an outline
until blur. Routes stay `#/settings/<section>`; Storage
and Trash (Data) keep their controllers inside the same frame, `#/trash` still
works, and a bare `#/settings` never reopens Trash. Redraws retain field drafts,
focus and selection, including textareas. App polling updates native status and a
ready-update banner without replacing focused controls. `app/refresh.js` reconciles
settings and explorer panels by stable keys, retaining native controls, file
selections and unchanged rows through async updates. Custom select buttons and open
menus retain their native elements and focus through reconciliation. Dictation autosaves snapshot the draft and page generation
before queuing or debouncing, so navigation cannot lose edits or update a later page.
Settings navigation stays between sections; its phone strip recentres only when
the section changes.
First-launch setup
(`onboarding.js`, 2026-11-02, the user's request: like other dictation apps) is a
two-step `modal` sheet (a backdrop click never dismisses it; Escape still does)
with an Access → Speech model stepper. The Access page lists Microphone and macOS
Accessibility with icons; each row turns its button into an Allowed checkmark as
soon as the 1 s poll sees it. Continue needs only the microphone, or a microphone
state the system cannot report (Windows often says `unknown`; `unavailable` in
test sessions); Accessibility is optional. A denied microphone shows Open System
Settings (macOS ignores a second request). Skip setup finishes all three flags.
The model page shows the catalog as selectable cards (Recommended, or All, with
the Settings rating meters), preselecting the in-use or first recommended model,
with one action bar: Download · size, a progress bar with Cancel, or Use and
finish. Download polling updates chips and the bar in place so a click is never
lost to a replaced button. Back returns to Access. The existing three completion
flags remain compatible, and late responses cannot dismiss another sheet or page.
Speech models show In use, every Downloaded file and filtered All models cards.
Rows show escaped catalog descriptions, languages, size, streaming support and
measured accuracy/speed with themed meters; unknown scores stay unmeasured.
A remembered Storage section at the Settings root keeps Settings selected and
Back returns to the previous note; only the explicit Storage route selects Storage.

`Powers.app_settings` supplies session-gated bounded JSON `/api/app` and
`/api/health`; blocking native work runs off async tasks and outside the store
lock. Explicit `/api/backup/setup` has a single-operation semaphore. Recording
reserves `capture_activity` before creating a job; its lease prevents updater
installation through the durable save, including browser-journal recovery.

Router completion and error handling check a render generation and the current
hash. Late missing-note/offline failures cannot redirect, toast or overwrite a
newer page, including a return to the same route. Cancelling a menu animation
clears closing state synchronously and detaches its old cancellation callback
before a new animation can own that state.

## Idle work and freshness

`read_store` accepts an immutable Store callback and coalesces filesystem scans
using core's Watch. It checks initially, on relevant/rescan notifications, every
60 seconds and after native foreground return. Missing/broken watchers fall back
to checking every access. `with_store` and `store_now` always reconcile before
mutations: a save must never absorb an unread external edit into its fingerprint.
Tests delay notifications while external files are created/edited before a save.

App-status polling stops when the document or native window is hidden and
invalidates late responses on visibility changes. Return triggers an immediate
refresh and one polling chain. Ordinary note views check every 30 s; Settings,
active updates and permission onboarding keep their responsive cadence. Dictation,
model-download and native-recording display polls pause while hidden; browser-fed
capture/journaling continues. Automatic status GETs do not reset user-idle time.
Graph/vector/caption maintenance starts after its first interval and defers while
the native window is hidden or the capture activity gate is busy.

Application controls and draggable note/folder rows suppress text selection;
prose and editable fields remain selectable. The page admits native drag starts
only from its explicit note/folder sources, preventing accidental link, image and
selection drags while keeping file drops and pointer-driven board/resize actions.

Page toolbars share the same 760 px maximum width across notes, library, recording
and Settings. Every Settings section uses the same Settings / section breadcrumb;
Storage and Trash keep their routes and controllers inside that frame. Storage
category cards remove outer Settings padding so only their compact summary sets
the closed height.
Usage opens from the rail above Settings and shows remaining subscription windows,
with unknown and expired readings explicit. The combined Quick Leo uses only
Cmd/Ctrl+F and retains task/search/question/navigation ranking. Explicit named
month dates and ISO dates keep their deadline; omitted years use the next date.
The calendar tray puts its mini calendar before tasks, and all-day/timed grids
share the full width without scrollbar gutters shifting their day boundaries.
Search-result and Quick Leo note opens carry the query into a bounded CSS Custom
Highlight, cleared after five seconds or navigation without changing editor DOM.

Usage keeps a single provider column with compact Session/Weekly rows: remaining
percentage at left, a muted provider-colored bar and reset time at right. The
dialog has no nested settings card or setup prose; unknown readings stay explicit.

Calendar tasks use 13 px two-line titles, taller all-day/month rows and a 60 px
minimum timed block. Overlap layout includes that visual minimum; saved task
times stay unchanged. Google export, .ics download and delete/stop-tracking
actions use icon controls with accessible names and hover titles in both peeks
and task sheets.
Minimum-height blocks resize from the original end plus pointer movement,
including scroll movement; a sideways grip drag cannot lengthen a short task.
Month rows grow to fit their previews and More action; the month grid scrolls
and keeps its position across redraws instead of clipping larger task cards.

Ordinary Settings reads never refresh coding-plan limits. Usage first draws saved
readings, then POSTs `/api/settings/usage`, behind the session gate and one refresh
slot, with blocking CLI work outside store locks. Failed refresh leaves saved
meters visible; responses from closed sheets or old page generations are ignored.
Search scrolls the matched Range itself, including within very tall paragraphs.
A resize with zero vertical movement never writes a synthetic end to a deadline.
Quick Leo accepts a year directly after a comma and validates local minute-based
ISO datetimes as one date/time before removing them from the title.

## Retired interview practice (2026-11-05)

Interview coding practice, its style, tools, workspaces and runtime downloads were
removed by user decision. Chat and Study retain quiz and ask_user cards, marking,
study sessions and review. Shared progress meters, meterBar, source-more, Mermaid,
KaTeX, margin notes and board tools remain independent of the retired feature.

Legacy interviews, interview_keys and interview_problem_undo tables and their rows
remain in leo.db, unused and never purged or migrated away. Old interview-style
chats open as Chat; their saved cards show a short retirement message. Old routes,
remembered places and style fall back to the note workspace or Chat. Stale
leo-interview-* localStorage keys stay untouched. Downloaded runtimes remain in
Storage's Other downloaded model files with the existing symlink/path protections.
