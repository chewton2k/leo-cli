# leo-web: leo's engine and the page

leo-web depends only on leo-core. Everything that needs services (AI, speech,
settings, import, recording housekeeping) arrives as a `Powers` field built by
`leo_host::powers()` (writer, chat streamer, settings, importer, listener,
reader, seer, meaning, room, speech, toolless, toolless_chat, light_chat,
lighter).

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
  `schedule.js`, `saving.js`,
  `recording.js`, `margin.js` (tests in `tests/*.test.js`).

- Everything leo-web computes or keeps besides notes and uploads lives in one
  SQLite file, `<data>/leo.db` (`db.rs`; rusqlite bundled, WAL, 0600, never
  synced): chats (+ FTS5), chat documents, review progress, picture captions,
  meaning vectors, note connections, calendar feeds, note-task metadata,
  reading positions. Old caption, graph and meaning JSON files are brought
  in on first open and renamed `*.before-database` (the browser tests seed the
  note connections this way). Writes touch only what changed.

## Serving and security

- `start(StartOptions { port, token }, powers) -> Running` binds `127.0.0.1`
  only (the requested port, else the next free of 19), spawns the graph and
  idle tasks and the server; `Running::{port, sign_in_url, stop}`.
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
  `#/settings[/keeping]`), `data-action` delegation (`actions.js`). Every
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

- Back from Settings, Keeping or Trash returns to the page before them
  (`places.before`, kept on `hashchange`; Keeping opened from Settings goes
  back to Settings), so a note you were reading stays where you left it.
  When the page Back goes to is the previous history entry (each entry keeps
  its position in `history.state.leoAt`; in-place URL rewrites keep it), it
  steps back with `history.back()` instead of adding an entry, so the
  browser's or phone's own back does not return to Settings (2026-10-20).

- `api()` treats an empty body as `null`; every success is JSON or a bodiless
  204 (a browser test sweeps the API).

- Sidebar header (2026-11-06, the owner, after T3 Code): one row at the top of the
  sidebar holds the toggle and the "leo" brand, everywhere. In the macOS app
  (`html[data-app="mac"]`) the window buttons cover the top 44 px of the first
  86 px, so a windowed Mac starts the row at 84 px; full screen (the desktop sets
  `html[data-fullscreen]` from `is_fullscreen()` on load and on every resize),
  Windows and the browser start it flush left. Folded beside the window buttons,
  the toggle drops below them into the rail and the toolbar is padded to 86 px.
  `sidebar.spec.js` measures all three states.

- Layout variables on `body`: `--side` (sidebar, 0 under 1000 px), `--right`
  (Felix and/or the original pane), `--mid`. Fixed things (toolbar, select
  bar, toasts, FABs, `#dock`, the map) use them so nothing covers anything.
  While Felix or an original file is open beside the page (at least 240 px from the left),
  the window's own scrollbar is hidden (`html.page-bar-on`) and `.page-bar`
  (core.js) draws one at the left edge of Felix (and the original pane), so
  the notes' scrollbar sits beside the notes; the page still scrolls the
  window, so reading positions and paging are unchanged.

## Shared components (new UI must use these; 2026-11-06, the owner: "make sure throughout the app we are reusing ui components")

- Choice of a few options: `segmented()` (settings-navigation.js; `field` mode fires
  `change` on a hidden input, `action` mode makes `data-action` buttons with
  `aria-pressed`; a 4th choice item is the tooltip). Calendar views, Which days change,
  onboarding Recommended/All, Add to note's where, the card panel's priority.
- Picture choices: `choiceCards()` (the same two modes; `columns` keeps rows aligned).
- On/off: `settingsSwitch()`; checkboxes stay only for task completion and picking
  several items (Trash).
- Dropdowns: a `<select>` inside `.select-control` with the chevron; select-menu.js
  turns every one into the app's own menu (document-wide, sheets included; not under
  forced colours, where the native select stays), keeping the select's id, value and
  `change` events. Do not hand-build list menus.
- Text fields: `.field` (icon-led box, sheets and the note field group) or `.set-input`
  (Settings rows and label-above forms).
- Folder, Title and What do you want: `noteFields()` (uploads.js), used by the upload
  sheet and the Record page.
- Confirmations: `confirmSheet({ title, text, yes, action, data, danger, cancel })`
  (sheets.js) for permanent losses. Moving a note or folder to the trash asks nothing and
  offers Undo for 10 s. Removing a key or speech model is a two-row `popupMenu`.
- Menus: `popupMenu(owner, { label, rows, point, find })` (popup-menu.js) is the one
  dropdown: explorer, ⋯ menus, More from 600 px, and Move with `find`. A row's `next` swaps
  the menu for a second step. `sheet()` itself focuses the first control and traps Tab.
- Explorer rename and New folder use `inlineEdit` over the tree row (side.js). New folder
  falls back to its sheet when the tree is hidden.
- Empty pages: `empty(icon, title, text)`. Counts: `.badge`. Small uppercase section
  labels share one rule (`.side-group`, `.source-label`, `.quick-due-head`,
  `.chat-history-group`, `.pick-head`).
- Kept on purpose: the speech-model filters (five wrapping filters, `library-views`),
  the model picker (a menu of models with effort chips), the margin-mode menu (its own
  interrupted-motion and keyboard tests) and chat.js's attach and access menus (their
  rows are richer than a list; they move through `leoMotion`).

## Motion rules (2026-11-06, the owner's list; the single place for these)

Quick and caused: nothing moves without a reason, nothing bounces on everyday UI (Felix
the mascot is a character and keeps his springs), nothing tweens while held. Every
scripted animation is skipped under `stillMotion()`; CSS motion sits under
`prefers-reduced-motion: no-preference` or the global reduce rule.

- Tokens (`index.html`) and their script twins (`app/core.js`): `--ease-arrive` /
  `EASE_OUT` for entrances, `--ease-move` / `EASE_MOVE` for things that stay on screen and
  change position, `--ease-exit` / `EASE_EXIT` for exits, `--ease-drawer` for the chat panel.
  Durations `--t-press` 100, `--t-follow` / `MOTION_MS` 180, `--t-open` / `OPEN_MS` 200,
  `--t-drawer` / `DRAWER_MS` 240. An exit is its entrance × 0.8 (`--t-*-out`, `EXIT_SHARE`,
  `LEAVE_MS`). Drop-downs run 150 to 250 ms. A symmetric position change is a move, not an exit.
- chat.js, doc.js, recording.js and pane-resize.js are separate scripts: they reach the
  helpers through `window.leoMotion` (set in motion.js, absent in some fixtures, so each
  call has a plain fallback); doc.js reads the curve from the CSS token.
- Press: everything pressable scales to .97 with the individual `scale` property (one grouped
  rule at the end of the style block), never draggable rows, `.actions` tooltips or disabled controls.
- Knobs: `knobStretch(el, pseudo)` stretches 12% along the travel, peaking mid-move, on `scale`, so
  the position transition is untouched and it ends at exactly 1 (switch, segmented pill, choice ring).
- Popovers: `unfold(menu, from)` / `fold(menu, then)`. `from` is the trigger element or a pointer
  `{x, y}`; `popEdge` puts the origin at the nearest point of the menu, upward menus reveal and close
  from their bottom edge. They start at .95 and keep the clip reveal; a click during the entrance lands
  on what is drawn. The margin menu and the outline run their own reversible animations with the same
  values. A closing context menu takes `.popup-menu-gone` and `aria-hidden`, so lookups see one menu.
- Stagger: `cascade(list)` staggers direct children by 30 ms (capped at 150) on first arrival only,
  never on a re-render; `cascade(el, 'settling')` and the CSS for sheets, `.settings-section.arrive`,
  the chat panel and margin replies bring inner groups in 30 and 60 ms apart. Board lanes and cards
  only fade: transforms there would count as overflow in `kanbanFit`.
- Moves: `flipList`, `pillGlide`, `settle`. `settle` follows an arc (x and y on different progress,
  `arcPoints`) and lifts onto a fixed carrier when an ancestor clips (`clippedBy`), so a drop
  passes lane and row bounds. `flyInto(ghost, target)` carries a card into another board's tab, or
  "n more" when that tab went back into the overflow; it pulses on landing.
- Pulse: `pulse(el, { lift, ring })`, one 320 ms pulse on what just succeeded; no repeats.
- Overlap: `overlapOut(el)` clones the old state on the body (`ghostOf`, shared with the sheet close)
  and fades it over 140 ms while the new state arrives. The clone keeps its classes, so a test that
  looks up by class right after a change scopes to `#app`.
- Colour: selection colour arrives with the thumb (`.segment`, `.switch`) or as a wipe (`.tab-wipe`);
  never on hover. `iconSwap(host, change, part)` crossfades one icon into another with blur and scale.
- Rubber band: `rubber(over, reach)` and `pastEdge(at, low, high)` resist a pull past a limit without
  overshoot (sheet swipe upward, pane and chat grips, board and calendar ghosts past the page).

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
  `version` (FNV-1a); 409 → "<title> (conflict from leo)". `saving.js`
  keeps edits that did not reach leo and retries them on the next load.
  Undo: up to `MOST_UNDO` snapshots per note (typing grouped by pauses).
  Lists (2026-11-06, the user's call): `markdown.js` `MARKER` is the one definition of a
  list marker: `-`, `*`, `+`, `1.`, `1)`, `a.`, `a)`, `A)`,
  and `A.` with two spaces (one space stays prose, so `A. Smith` is a name, as in Pandoc), one or
  two of the same letter, so `ok.` and `Hi.` stay prose; after `z` comes `aa`, then `bb`.
  A lettered list begins at `a` or `A`, continues an item on the line above, or has a box
  (`listable`; `after` is the option for a note's one-line blocks), so `p. 12`, `c. 1900` and
  `e. coli` stay prose everywhere, editing included. `listLevels` is the one forward pass over a
  note; `listed` answers for one line. `Note::checkbox_line` in leo-core stays context-free and
  counts the same lines, which is why a box always makes an item. Enter continues with `continueLine`,
  keeping the marker's gap; on an empty item it steps out one level, then leaves the list;
  Backspace at the start of an item's text does the same through `stepBack`, keeping the
  text. Tab nests by the width of the marker above, so other apps nest it too; Shift+Tab goes
  back to the parent's indent; old two-space lists still nest (`listLevels` reads depth from the
  run of list lines). `renumber` then fixes numbers and letters through the item's run of list lines,
  stopping a level at the first marker more than 1 from its place and leaving a lettered level
  alone unless it starts at `a` or `A`, so `2005.`, `c. 1905` and a sub-list begun at 7 keep their
  markers; a sub-list Tab just made (`fresh`) starts at its first number or letter, and `closeList` closes the numbers
  over a line that left the list. Nesting looks like Google Docs (2026-11-06, the user's
  call) and only the page changes, never the file: `markdown.js` `list` gives lists below
  the top `list-1`/`-2`/`-3` by depth (`depth` is the option for a note's one-line blocks),
  CSS draws disc, circle, square and `1.`, `a.`, `i.`, repeating; a typed top-level `1)` is
  `list-paren`. Letters draw as typed at any depth (`list-lower`, `list-upper`, `-paren`), each
  `li` with a `value` because CSS counts `aa` as 27 and `bb` as 54. Different markers side by
  side are separate lists.
  The source field in a list item is `inline-block`: as a block,
  WKWebView (not Playwright's WebKit) leaves an empty item's marker on a line of its own.
  Typing a list or checklist marker plus its space (`- `, `1. `, `a) `, `- [ ] `)
  re-renders that line at once with the caret kept (not during IME composition);
  headings restyle live, one Backspace takes a heading's marker off, and Enter on an empty
  heading clears it. A checked task strikes only its text (`text-decoration`,
  per wrapped line; an empty one shows none). A box toggles only from a click
  that starts and ends within `DRAG_CLICK_DISTANCE` of one point, so selecting
  text and releasing near the box never ticks it.
  Checkbox clicks update the live input and task style without redrawing the
  document. Finishing another edit resolves the clicked source line after shifts
  and preserves the checkbox’s viewport position and focus.
  Cmd/Ctrl+A selects the whole note at once, even from inside one line
  (2026-10-19; it used to take a second press).

- Rich text (2026-11-06, the user's words: "all different types of bullet points, heading,
  italics, bold, enumerations with a) b)... or 1) 2), tables, underlining"): the file stays plain
  Markdown. `spans` in `markdown.js` pairs `*`, `_` and `~~` by delimiter runs, so `***both***`
  and `**bold *italic***` nest and marks never cross; `_` inside a word and a single `*` after a
  word stay literal. Underline is `<u>text</u>`: after escaping, only that exact pair comes back,
  every other tag stays text. Cmd/Ctrl+B, I and U toggle a selection through `editing.restyle`
  (per line, past list and heading markers, also across rendered lines, skipping tables, code and
  math when it spans lines); a selection inside a longer bold run wraps instead of splitting it.
  Only marks the page draws come off, so `foo_bar_baz` and `2*3*4` keep theirs, and a callout's
  `[!type]` is never wrapped. With no selection the key opens an empty pair around the caret (`stylePair`,
  not in code or math): the next press removes an empty pair, or steps past the closing marks
  once something was typed.
  Tables: a click lands in that cell's raw text; Tab and Shift+Tab walk cells, Tab past the last
  adds a row. Alignment colons become `align-*` classes, `\|` stays in its cell, a short row is
  filled in. macOS replaces `--` with a dash only while `spellcheck` is on, and cancelling that
  replacement corrupts the text, so `look` turns spellcheck off for tables, code, math and lines
  starting with `-`, `|` or `:`; Playwright's WebKit does not show this, the real WKWebView does.
  On a Mac End and Home move the caret (WebKit only scrolls); a caret past the rendered end of a
  line lands past its raw marks.

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

- Workspace: root and legacy folder hashes show the editor workspace, never a
  library grid or list. The explorer owns note and folder navigation. Search and
  Trash retain their result cards; the bounded metadata and notes APIs remain.
  Browsing never calls an AI.

- Folders: captured explorer targets support create, rename, move and trash with
  Undo through POST `/api/trash/move`
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
  Settings pinned below (Trash and Keeping live inside Settings since
  2026-11-02, the user's call; the rail shows Settings for them). The rail has no
  Notes button (2026-11-03, the user's call: notes open from the explorer; the
  workspace is reached from the breadcrumbs' first crumb or the leo name). Buttons retain accessible names,
  tooltips and active states, with space between controls and unboxed shortcut
  hints. The separate explorer shows nested folders and
  individual notes. Folder labels and chevrons toggle their children; note rows
  open notes directly. The selected folder scopes New note, Record and imports;
  breadcrumbs reveal folders in the explorer. The explorer narrows
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
  explorer moves there (combining stays on Search cards). The destination
  folder's row gets `drop-ok` and its open children `drop-in`; holding a note or
  folder drag over a closed folder for `SPRING_MS` (600) opens it for that visit
  (`side.revealed`, never saved); the tree scrolls near its
  edges; moved folders keep their open state (`moveOpenFolders`). A redraw can
  replace the dragged row, so `dragend` is also caught on the source element,
  a pointer move with no button ends a drag, and drops check the drag's type. Opening another note clears the previous folder selection. Selecting a folder while a note stays open highlights that folder and scopes creation there until another note opens. The rail, explorer, tabs and
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
  stays over the icon rail in both states, below the macOS window controls. Folders start closed;
  `leo-side-folders` keeps only folders the user opened with their own clicks
  (2026-11-03, the user's call), so the explorer reopens after a restart as
  they left it (the old `leo-side-open`, which also held every folder browsing
  unfolded, is dropped once). Opening a note or folder unfolds its parents for
  that visit only (`side.revealed`, rebuilt whenever the folder or open note
  changes), never into the saved set. Modelled on VS Code and
  Obsidian (2026-11-03, the user's call): the open note's row is scrolled into the
  explorer once when it first appears (`revealNoteRow`, the explorer only, never
  the page, skipped while the explorer is hidden, so later redraws keep the
  user's scroll); and a fresh launch reopens
  where the user left off (`leo-last-place`: the last note, folder, board,
  calendar or board page, restored only when a new window session
  opens on the workspace, never on reloads or in-app navigation). It is kept after
  every page that drew and when the window hides or quits (`pagehide`), since a
  new note's id and a deleted note's redirect rewrite the address without a
  hashchange; a missing note resets it to the workspace. Tree and rail scrolling, focus and note pages survive redraws.
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
  same-chat history/access/config mismatches replace only that chat.
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

- One Felix, no modes (2026-11-05, the user's call: "let the agent decide"):
  `HELPING` tells him to work out from each message whether to answer, teach
  or test, and an open note alone never starts a session; `LEARNING` holds the
  rules for both. Tools were never gated by style. The `chats.mode` column
  stays for old rows and is kept as it was on save (new rows say `chat`); request
  and saved `mode` fields are ignored. `.chat-switch` keeps only the who-answered
  line. Review shows on the empty chat only when questions are due.
- `LEARNING` is not in the system prompt (2026-11-06, half of it, about 1,700 tokens off every cold prompt, measured on Codex): Felix calls `study_rules` before teaching, testing or marking. So nothing depends on him remembering, practice, marking and a running session send `<study_rules>` with the prompt, and a first `quiz` or `study_plan` carries it in the tool result; `Reads.study_rules` keeps a session from hearing it twice.
- Lessons and practice borrow LearnHarness's method, in our own words only
  (AGPL-3.0 against leo's MIT; never paste its prompts or code). A lesson finds
  where understanding stops (ordinary quiz cards before any plan), plans the
  shortest dependency-ordered route (`study_plan`), teaches one reasoning step
  per reply (prediction, then teaching, then a check card in the same reply) and
  ends when the goal seems obvious. Practice runs each idea through recall, then
  telling it from its look-alike, then combining, with `more_parts` keeping the
  idea open; hints go one rung at a time; retests use a new situation. A
  prediction is `quiz` with `predict: yes`: free_response only, shown as Predict,
  never marked (`quizSay` says so, the request sends `mark: false`), never scored,
  not counted in `session.asked` and never a missed question in review. Other
  cards offer How sure (`SURE`: sure, think, guess; pick before answering, since
  a choice submits at once) and Add your thinking (up to `MOST_THINKING`, a
  separate `think:i:k` draft). Both go to `/api/quiz/mark` (`confidence` is
  validated, `thinking` bounded and sealed) and to Felix inside the answer line
  (`aside`), which must stay one line: `send` folds held results by taking their
  first three lines. A right guess leaves its idea open for one retest. A typed
  "I do not know" needs no button: it is marked as a miss and Felix treats it as
  a gap.
- Quizzes. Marking is critical (a right
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
  Felix starts from the open/attached material without being set up,
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
  (`FALLBACK_TRIES`). While Felix works the reply shows only his newest step; a
  finished reply folds every step into one `.msg-trace` line (`traceSummary`,
  2026-11-06, the user's call from a six-persona simulation: citations and change
  cards are what catch mistakes, and the count must agree with the sources). An
  empty note search with nothing cited adds `Not from your notes`
  (`notFromNotes`), since that was the one thing only the step lines said. Steps
  stay in the saved chat and in what Felix is told he did. Felix has no web tools of his own; an AI's own web search
  is left to it.

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

- Choosing the model (`routing.rs`, 2026-11-06, the user's call; same shape as Copilot Auto and Cursor Router, with rules instead of a trained classifier because leo has one user and a classifier call would cost tokens): `routing::light` sends only obvious cases (short lookups and greetings, no attachments, files, selection, study or Auto access, at most six turns) to the cheapest model of the chosen AI (`ai::quick_config`, through `Instructions.light` or `Powers.light_chat` when no session opens, as on a Codex without app-server isolation); every doubt stays on the chosen model, and other languages stay there too. `Powers.lighter` turns routing off when the quick tier is the same model and effort, and a failing light writer gives way to the chosen one on the last try, so `Auto` always names who really answered. The tier is part of the warm key, and `Slot::light` keeps a warm full chat from moving down, since a switch loses the cache: a chat moves up once when it needs to, never down while warm. Settings → Felix → Cheaper model for lookups (`Prefs.auto_model`, on by default) turns it off; a routed reply says `Auto` in its usage line.

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
  attribute says the connected notes are read too), while a note is attached an
  eye toggle (`data-chat="drop"`) and New/Close. Felix's words (greeting, answers) use
  `--voice`; the user's messages stay sans in a `--field` bubble. The greeting
  and starters arrive once per chat (`data-hello` key): words fade
  in (`.word`, `word-in` .5 s opacity and colour, no blur, nothing moves),
  starters are pills that appear together after it. `draw(stick, live)` no
  longer replaces the transcript on every streamed chunk: `settle(from, to)`
  keeps every node that already matches, appends only the new words of a
  growing text run and replaces only
  what changed, so finished text, a selection in it and the greeting animation
  survive; the final redraw settles too. The panel slides on `--ease-drawer` /
  `--t-drawer` and leaves over `CHAT_LEAVE_MS` before `chat-open` is removed.
  Hovering or focusing a line-citation chip calls `onCite(id, lines)`; the open
  note's `doc.glow(from, to)` toggles `.blk-glow` on the cited blocks (no
  redraw, no scroll, no timer) and clears on leave, blur and close.
  A citation to another note does nothing extra.
  The Chats heading has no New chat action; the main chat header keeps it. History
  titles use 13 px type. Delete keeps its button mounted while X expands to Delete,
  cancels on Escape or leaving the control, and keeps the chat on a failed request;
  Reduced Motion snaps the width and label.

- Perch (2026-11-06, the user's call: "i like A. perch best flow and it's simple"; replaces the header and surface description above): the Chats list sits on `--ground`, the chat on `--page` with the 16 px corner, the composer, selected row and user bubble on `--raised`; `--field` and `--field-hi` are gone, pills and circles are 9 to 16 px rounded rectangles. An empty chat is `.chat.perched`: the greeting, composer and starters are one block centred in the panel at any height (`display: contents` on the body and hello, DOM order (`#chat-below` holds the starters after the footer) and two flex spacers), Felix perched on the composer's top edge with his `hi!!`. The first send docks the composer to the bottom (`leoMotion.dock`, `DRAWER_MS`, snaps under Reduced Motion) and it stays docked while the chat has messages. Felix lives in `#chat-face` inside the composer, so his state classes still apply (`mood` finds every `.felix`). The header is toggle, chat title (or "New chat") and close; New chat is in the Chats heading while Chats is open and in the header otherwise (one `[data-chat="new"]` shows at a time; specs use `:visible`). The composer holds paperclip, the note chip (the `data-chat="drop"` eye toggle, hidden with no note, `.off` when not reading), the access chip (no indigo for Auto; "⇧Tab to switch" is only its tooltip) and send.
- Chat panel (2026-11-06, the owner; simulation in `docs/superpowers/user-simulation-report.md`): the eye
  (`data-chat="drop"`) is a toggle that shows while a note is open, eye or eye-off, "Stop reading this
  note" or "Read this note again", and keeps focus. New chat adds a placeholder row to the Chats list
  (`state.fresh`, set by `begin()`, cleared by `resume()`; `drawChats` derives it, no ×, titled by the first
  message) that goes away when the chat is left empty. Answers no longer split into word spans (the greeting
  still does): streamed text only grows its last text node, and `.msg.pending > .prose` with no later `.prose` gets a
  28 px CSS soft bottom edge (none under Reduced Motion), so nothing is state on a message. In answers each
  `[n1 L4-6]` is a small muted superscript button (`numbered()`, one number per note and line in order of
  appearance, `.cite-n`, title "Note, lines 4–6") and `.msg-from` is a "Sources" line of `.cite-src`
  buttons with number, note and line; copy, Save and Add to note still drop every `.cite`. `cite()` numbers
  only when given `marks`: quiz cards and file chips keep their look. The prompt (`BASE`, `REPLY_STYLE`):
  one cite per bullet, short plain sentences, no bold except a closing "Next:" line.
- Files for Felix (`chat_files`, ≤10 per chat, text only, deleted with the
  chat), passages (Ask Felix on a selection, ⌘⇧E; no bottom Ask button), Add to note
  (`routes/insert.rs`, version-checked, request ids dedupe double presses),
  Save as note, chat history (`chats.rs`, local naming), cost line
  (`spent`), drawings (Mermaid for flows and charts, SVG otherwise).
## Felix in the margin (`margin.rs`, `routes/margin.rs`, `web/margin.js`, `app/margin-notes.js`, 2026-11-04)

Load margin preferences and reserve their space before positioning search matches
or restoring reading, so the note's final line wrapping determines the scroll
target. Check the page sequence after loading preferences.

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
  without trashed notes' threads unless Trash is chosen.
- Modes: `ask` (When you ask, the default) or `along`. A note's own row wins;
  without one, the global default applies only to notes leo made (a v4 id, so never a
  headerless file) after it was set (`Prefs.since`), so turning it on never starts
  reading old notes. Turning the default off pins the notes made while it was on
  (`margin::pin_modes`, insert-ignore in one transaction, before the prefs are written),
  so they keep reading along. An unsaved note shows the default and keeps a picked mode
  in `marginPage.wanted`; the created hook (`marginCreated`) loads the note's margin and
  POSTs it (2026-11-06, the user's call).
- Read-along (`POST /api/notes/{id}/margin/read`, `{version, blocks}`): the page
  sends changed blocks after a pause (4 s, slow 8 s). Silence is the normal answer:
  `{}` or empty output is success; malformed JSON or prose is a failed read. The server refuses, whatever the page
  does: a stale version (409), a note not in `along` or not edited in this engine
  run (403, `Reads::edited` is set by the PATCH route, so browsing can never read),
  a reply streaming (`Steering::any_open`), the chosen gap between reads of a note,
  overlapping reads and the daily cap (429 with `why`). A useful comment adds no
  separate five-minute lockout. Reads can explain, connect ideas, ask a worthwhile
  question, correct a mistake or include one small Mermaid diagram.
  The cap is spent in one transaction after the read is claimed, so failures spend it
  too, and a broken day row counts as spent. Blocks already read are dropped. A failed
  read forgets its blocks and answers `{failed: true}`, so the page keeps its baseline.
  A refused read waits for the next edit; a stale-version refusal resets the page's
  retry delay to the normal typing pause, while provider cooldowns stay bounded.
  No comment this time clears after four seconds or the next edit. The model's quote must be in the note now,
  or the note is dropped.
- Reads use `Powers.toolless`, never `quick`: the quick path gives Claude Code
  WebSearch and Codex its web tool, so `no_tools` passes `--tools ""` and
  `web_search="disabled"`, and the chain is cut to its first provider, so a read fails
  closed instead of falling back to a cloud AI. Replies and asks use
  `Powers.toolless_chat` on the normal model. Note text sits between tags with a
  random suffix and the prompts say it is never instructions; thread messages are
  JSON records inside those tags, one per line, so stored text cannot close the block
  or forge a turn. Plain messages use `textContent`; diagram answers use the existing
  escaped Markdown renderer, strict Mermaid rendering and inert SVG images. Settings says text
  stays here only for a loopback address and a model without `cloud` in its name
  (`ai::reads_stay_here`).
- Settings → Felix chooses both limits (2026-11-05, the user's call): the gap (30 s, 90 s, 3 min, 5 min; default 90 s, `Prefs.gap`) and reads a day (20, 40, 80, No limit; default 40, `NO_CAP` is 0). Every read starts a fresh process (`claude -p`,
  `codex exec --ephemeral`; no warm session, and one retry on a stall), with at
  most 8,000 characters in and 220 tokens out. At the defaults, 40 reads
  take an hour of writing at least, and a day's worst case is about 90k input
  tokens, three Felix answers (31.8k each). CLIs ignore `max_tokens`, so output is
  bounded when parsed.
- A failed or missing AI shows a fixed, actionable status above the note and leaves `margin::PROBLEM`
  (fixed text, no provider detail) in Settings → Felix and Health until a read works.
- Selection: Ask Felix (⌘⇧E) opens a draft in the margin and asks at once, POST
  `.../margin/threads` (no read-along needed) with an empty question (`margin::UNSPOKEN`;
  only Felix's message is stored). The draft shows dots (`.margin-pending`, filled from
  `marginPage.thinking`, hidden when closed) and becomes the thread in place on success
  (`marginRekey`, no rebuild); a second ask meanwhile toasts. Closing while he thinks
  leaves the thread closed when it lands, and a closed failure toasts. An open failure
  shows the problem and lets the user type a question (2026-11-06, the user's call).
  Replies show the sent text and dots the same way; a failed one restores the text.
  Every open thread has an Ask in chat icon beside ×; actions read Resolve, Dismiss,
  Add to note. A closed thread with more messages says "N more" (marks get a ring).
  Word arrival animates colour only, never opacity (blur on WebKit); positions are rounded. With Felix open, under
  3 or over 600 characters, it goes to the chat as before. The server anchors on
  prefix + quote + suffix exactly and never falls back to another occurrence. Replies
  POST `.../threads/{id}/reply`, bounded (`MOST_REPLY`, `MOST_MESSAGES`, the last 8
  messages sent), and never run twice at once for a thread.
- Replies cap at `MOST_REPLY` (4,000) so a sketch fits; `without_open_fence` drops a fence cut off mid-drawing.
- Add to note uses `/insert` with one request id per thread (`/insert` dedupes on note
  and request, whatever the base), runs once at a time and shows Undo before it closes
  the thread. After a 409 it re-finds the passage, or says it went to the end and
  skips the glow. Ask in chat attaches the quote. `add_margin_note`'s Undo is refused
  once the user has replied. `chat::gather_seeing` adds open threads of the open note
  after the notes (`margin::context`, at most 3,000 characters and an eighth of the
  note room); the block's fence is keyed per process and stable while its content is,
  so prefix caches still work, and warm sessions get it only when it changes
  (`Warm.margin`).
- Layout (`margin.js`, pure): `find` (exact, then between prefix and suffix of at
  least 6 characters, or an edge of the note, then lost), `stack`, `sides`,
  `arrangement`. Both margins is the default; saved choices still win. Right margin needs 928 px of column, Both 1,192 px. The margin is
  reserved up front on every note, in both modes, whatever its threads
  (2026-11-04, the orchestrator's decision): `.note-columns[data-margin]` pads the
  column, so the note is min(its note width, the room left) and sits left of centre,
  and a first note cannot reflow text being typed. The layout is set in the same task
  as the first paint; `leo-margin-layout` remembers the setting per browser for that
  moment, the server keeps the truth. Marks only in Settings, Full width, too little
  room or Felix open centre the note with a left gutter reserved before the first
  comment arrives. A mark centered vertically on the passage (24 px to hit,
  44 px on touch) opens the note inline, with
  `.margin-room` holding the space under its block. A lost anchor goes to `#chat-lost`
  in the panel.
- The reading-mode menu and status sit above the title. Saving a mode keeps the
  last confirmed value on failure. Its own menu class keeps generic select-menu
  click and close handlers from dismissing it. Each note remains a labelled article with an
  accessible margin toggle. Passage anchors use a separate gutter dot, never a
  mascot on the text. Straight dotted underlines reveal in about 500 ms; there is
  no default tether. Reduced Motion stills the reveal and word arrivals.
- Open replies have no card surface or boxed textarea outline. Their retained
  controls expand/collapse over 240 ms, become inert/aria-hidden on close and snap
  under Reduced Motion. Diagram size changes relayout the bounded thread stack.
- Arrival keeps the dotted stroke pattern fixed and clips each line left to right
  over a shared 500 ms budget. Open, hover and keyboard focus use a solid accent
  underline and wash. The comment has no mascot; compact layouts reserve the full
  24 px (44 px touch) anchor gutter before the first comment arrives.
  The mode menu reverses its current animation on interrupted toggles, closes on
  Tab or focus leaving it, and snaps under Reduced Motion. Settings → Felix → New
  notes start is one privacy sentence (Reads along sends the note's title and the
  text you change to the named writing AI, and says so when it runs on this
  computer) and two cards with one line each; When you ask names the selection and
  the real shortcut (2026-11-06, the owner: too many words).
- Overlay: `.margin-layer` sits outside `#doc`. Rendered block measurements and
  keyed thread elements preserve the caret and reply field. Read completion
  checks both session and request identity, so an old request cannot clear newer
  state. Explicit silence, refusal and failed reads have distinct statuses.
- Tests: `src/tests/margin.rs` drives the real engine over TCP with fake powers.
  `leo-test-server` takes `LEO_TEST_FAKE_AI` (the test binary only, never the app) so
  one flow in flows.spec.js joins the page to the real routes.

## Uploads, originals, recording

- Upload sheet (2026-11-06, the owner: redesign it): the drop zone comes first
  ("Choose, drop or paste files" with PDF / Word / PowerPoint / Text / Photo chips;
  a slim Add more files bar once files are picked), then the files, the Add/New
  switch, the folder, and Title and What do you want under one "Optional" caption.
  The only other line, that photos and scans need a writing AI that can see images,
  appears only while a picture is picked (the page cannot tell which AI sees).
  Folder, Title and What do you want come from `noteFields` (uploads.js), the same
  helper the Record page uses (passed to the recorder as a dependency).

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
  source. The page starts/stops/pauses and shows levels/transcripts; it never
  opens a browser microphone or screen-audio picker, and the server takes no
  audio from the page. Native jobs have no page heartbeat lifetime.
  Housekeeper's `pending_recordings` supplies retained native sessions; recovery
  starts in Writing state and supports Finish available, and anything the
  Housekeeper does not list is not found.
  The idle page (2026-11-06) shows Microphone, Screen and Call as one row of
  `.choice-pick` cards with a track diagram each (one waveform, or two for Call)
  over the same `rec-kind` radios; then `noteFields({ id: 'rec' })`, the upload
  sheet's group of icon-led `.field`s (Folder as the shared select menu, the calendar
  event picker beside it when a calendar is connected, then Title and What do you
  want under one "Optional" caption; ids `#rec-dir`, `#rec-title`, `#rec-wants`), the
  calendar hint as one linked line, and the "keeps recording if you close the window"
  tip under Start.

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
  (`tasks::tasks_dir`), one JSON line each, written atomically. The reader
  takes the line with the newest `updated_at`. Deleting writes a tombstone
  line (`deleted`), rather than removing the file. Rows left in the old
  leo.db `tasks` table are not read.

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

- Export adds `tasks.json`; Settings → Keeping's export card has the Restore tasks control; POST `/api/tasks/import` (ids dedupe; a newer local copy is kept).

## Boards (`boards.rs`, `routes/boards.rs`, `app/boards.js`, 2026-11-02)

- Storage mirrors standalone tasks: `<notes>/.leo/boards/<b…>.json` (name, columns
  `{id, name}`, 1-12) and `<notes>/.leo/cards/<c…>.json` (board, column, `rank`,
  title, subheading, description ≤8,000, priority `""|low|medium|high`). One JSON
  line per save, atomic, nofollow reads, newest `updated_at` wins, deletes are
  tombstones; a file counts only when its name is its id. Each card has its own
  file. Limits: 100 boards, 5,000 cards.

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

- New board uses the existing sheet and controls like New folder (a heading, one
  full-width `.field` named Board name) and a small preview of the three starting columns; Enter and Create board
  submit the same request, and Cancel leaves the boards untouched.

- Page (redesigned 2026-11-06, the owner: "like a nice and simple jira kanban board"):
  the page fills the window and never scrolls itself. One header row (`kanbanHeadHtml`)
  holds the board tabs with count badges, a + for New board, Add column and the board's
  ⋯. The open board's tab is its name: `.kanban-title` edits in place inside it, so the
  name shows once. A tab is one click; a card dropped on a tab moves to that board, with
  Undo. `kanbanFit` shows whole tabs only, the open board's always among them, and puts
  the rest under "n more" (`#kanban-switch`, the shared dropdown; the tabs move to `.kanban-over`,
  and while a card is dragged they all show again and the row scrolls near its ends, so
  every board stays a drop target).
- Lanes all have one height, down to 20 px above the window's foot (`.kanban-layout`), with
  0 or 50 cards; the card list scrolls inside its lane once it fills it (dragging near its
  edge scrolls it) and Add card stays pinned under it at the foot. Lanes share the width
  equally (`flex: 1 1 0`) and narrow to 224 px, the width at which four lanes still fit
  a 1280 px window with the explorer open; past it the row scrolls sideways, with a fade on
  the side where more lies (`kanbanFade`) and a thin bar in the bottom margin. Each lane is
  tinted 12% of its colour over `--page` (`--lane`); text on it uses `--text` or `--lane-ink`,
  and cards are solid (`--raised`, a step lighter in dark). `kanbanTone`: to do blue, in
  progress amber, done green, blocked red, others by place; a column that would match or
  look like a neighbour (`KANBAN_NEAR`) takes the next place hue. The head is the name
  (edits in place) and count; ⋯ and a lane's Add card appear on hover and keyboard focus,
  always on touch, and Add card always shows in an empty or full lane (`.kanban-full`).
  Cards show the title (three lines at most), the muted subheading, two lines of the
  description when there is one (`.kanban-has-text`) and the priority arrow top right. Add
  card sits directly under the last card; its composer stays open for the next card (adds
  are queued so fast typing loses nothing). A redraw keeps each lane's scroll. ⋯ opens the shared `popupMenu`, the explorer's
  menu, for a column (rename, move left/right, delete moving its cards) and the board
  (rename, add column, delete with a second press, Undo). Add card picks a priority (reset
  after each card). Under 600 px lanes are 82vw and snap, and Add column lives in the ⋯ menu. A card opens in a side panel beside the board like a note
  (2026-11-02, the user's call: no modal; `#kanban-panel`, kept across board redraws, a
  full-screen layer under 760 px): title, subheading, status and board (the shared select
  menu), priority (`segmented`, `data-priority`), a borderless description (autosave after
  600 ms, on blur, and before Escape or × closes it), Delete with Undo. The description is
  the note editor itself (2026-11-06, the user's call: "render markdown the same as our
  notes"): `leoDoc.mount` on `#kanban-card-description` (rendered blocks, click a line to
  edit, checkboxes toggle), no outline, pictures, margin or note id. `description` stays plain
  Markdown in the card JSON; `onChange` feeds the same debounced PATCH, and the panel's `end()`
  stops the editor, saves and `destroy()`s it. Escape in a line only leaves the line (the panel
  checks `.line-edit`; events.js prevents every Escape, so `defaultPrevented` says nothing), and the
  next one closes; from the title or subheading one press closes. An edited line never hangs its
  `##` into the panel's edge. Closing, opening another card, moving to another board and leaving the
  page first `settle` every field (`kanbanLeavePanel`, `kanbanSettlePanel`): a failed save or a
  description over `KANBAN_MOST_DESCRIPTION` keeps the panel, the address and the text, with the
  reason in `#kanban-card-problem`, so the server's 400 can never cost text.
  The card on the board still previews the raw text.

- Dragging is pointer events, not HTML drag and drop: mouse/pen lift past 5 px,
  touch after a 300 ms hold (a swipe still scrolls). The real card is the placeholder
  slot moving through the lists; a copy follows the pointer; the rows scroll sideways
  and lanes vertically near edges; pointer capture is on `#app`
  (moving the card in the DOM would drop capture). It ends on pointerup, cancel,
  lost capture or a move with no button; Escape puts it back; the click after a
  drop is swallowed; leaving the page mid-drag cancels. Columns drag by their name
  bar. Keyboard: Enter opens a card, Alt+arrows move it. After a move the board is
  fetched again so renumbered ranks are known. The Tasks panel hides on Boards.

- Tests: `src/tests/boards.rs`, unit tests in `boards.rs`, `tests/browser/boards.spec.js`
  (fixture: real CSS and boards.js over an in-memory API; desktop and webkit) and
  flows.spec.js (real server: drag, reload, API contract).

## Settings, Keeping, export

- Settings (`SettingsApi`, `leo_host::WebSettings`): one model picker
  per AI with prices and effort chips.

- Theme (2026-10-14, the user's request): Settings → Appearance picks Match
  this device, Light or Dark, kept per browser in `leo-theme`. Theme, Felix and
  Note width are rows of picture cards (2026-11-06, the owner's screenshots:
  `choiceCards` with `action`/`key`, so the buttons keep `theme-pick`,
  `felix-pick`, `width-pick` and `aria-pressed`): Theme previews are inline SVG in
  fixed light and dark surfaces (Match device is split), Felix shows
  `felix-preview cute-look` / `demon-look` (the mood code skips them), Note width
  a miniature page per width; Felix's two cards sit in a three-column grid so
  every row lines up. No footnote. Each has a row reset (`settingReset`,
  a small icon button that shows only while the value differs from
  `APPEARANCE_DEFAULTS`: Match device, Cute, Wide; `data-action="setting-reset"`,
  named "Reset <setting> to default"). Text size and Felix's lettering
  (2026-11-06, the user's request) are two more rows in `theme.js` (`READING`,
  stored like Note width in `leo-text-size` / `leo-felix-letters`, set as
  `html[data-text-size]` / `[data-felix-letters]` before paint, default key
  removed). Small .9, Medium 1, Large 1.12 set `--read`; only reading text
  multiplies it (`.prose`, headings, code, tables, line editor, `.margin-felix`,
  `.margin-you`, Felix's chat answers), so chrome keeps its size and line
  heights stay unitless. Lettering moves `--felix-letters` (`--voice` or
  `--note-font`) for answers and margin notes only; the hello text keeps
  `--voice`. Cards are `.read-sample` HTML, really at size. Add one only where the default is real. `/theme.js`
  loads in `<head>` (before any paint, no inline script under the CSP) and
  sets `html[data-theme]`; the dark colours apply under
  `prefers-color-scheme: dark` unless `data-theme="light"`, and always with
  `data-theme="dark"`. Anything drawn in JavaScript asks `leoTheme.dark()` and
  redraws on `leoTheme.onChange` (Mermaid diagrams), never
  the media query directly. Print overrides with `:root:root` so a PDF is
  always light.

- Updates & about (2026-11-06, the owner: clean it up): the version sits beside
  the Updates title; one status row has the state, when it was last checked and
  Check now (Download or Restart when they apply); two switches; one hint that
  updates wait for recording and dictation, then Leo reopens. About Leo lists the
  notes and speech-model folders as rows whose path is truncated in the middle
  (selectable, full path on hover; the app has no reveal or copy action), the
  unsigned-copy warning as a compact amber notice, `leo update` and
  `leo uninstall` as two rows, and Release notes in the card header.

- Keeping (`keeping.rs`, `app/keeping.js`; 2026-11-05, the user's call, replacing
  the Storage page, its `/api/storage` routes, areas and `Housekeeper` actions;
  files and tables only Storage managed stay on disk untouched): Settings →
  Data → Keeping sets how long leo keeps each kind of data (`/api/keep`, one list
  of 7/30/90 days, 1 year or Forever for every row): Trash, Chats with Felix,
  Uploaded files, Recording transcripts, Unfinished recording audio; then Export
  notes (zip of notes, optional uploads, chats, trash; never config or keys).
  Each row says what it is and what survives. Shortening a period asks first
  with a count from POST `/api/keep/preview` (`keeping::would_delete`, deletes
  nothing; `keep-now`/`keep-cancel`, Cancel puts the select back) and says the
  deletion cannot be undone. One save runs at a time: the selects are disabled
  while a preview or save is pending, and a failed save redraws the saved
  values. Restore tasks is a real button; a zip or a file without task ids is
  refused with what to pick instead. A card lists edits kept in this window
  that could not be saved, with Try again and Discard (`saving.discardAll`).
  Forever deletes nothing. Expiry runs when the engine starts (`start_in`),
  once a day (held in `Background`, stopped with the engine) and right after
  POST `/api/keep` (`keeping::expire_now`): uploads (files under
  `<data>/attachments/<note-id>/` older than N days by modification time; the
  folder and the note are never touched), transcripts (folders in
  `recording-sources/` by folder time) and unfinished audio
  (`record::Housekeeper::pending_recordings`, each with `Pending.started`, then
  `discard_recording`, plus `expire_kept_audio` for saved sessions and old
  `browser-audio` folders; the session in use, by `RecordJob::session_id`, and
  locked sessions are skipped). DELETE `/api/record/{id}` also deletes a
  waiting recording that has no job; the Record page offers it on each row.
  Every pass is bounded, never follows symlinks, stays in its folder and
  skips an item it cannot delete. Trash and chats expire as before.
  `Housekeeper` now lives in `record.rs` (pending and discard only).

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
so the restore wins over its deletion marker. Conversation history
and its search results display titles only, with no response preview or tooltip.

### Security and cleanup boundaries (2026-10-16)

Sign-in files use unique synced 0600 temporary files and atomic replacement;
loading rejects symbolic/hard links, nonregular files and oversized data, and
repairs existing permissions. Session persistence failures refuse authentication.
Original attachments and standalone tasks check every component inside their data
root, use nofollow regular-file reads and atomic writes, and refuse symbolic links.

Database handles are cached up to 32 paths; filesystem
checks/open/close occur outside the database cache mutex. Point and screenshot
workers retain their save guard through I/O and in-memory acknowledgement even if the HTTP client
leaves. Recording warnings retain at most 40 entries of 2,000 Unicode characters.
Keeping's expiry refuses linked recording and originals roots.

Writing Settings offers Ollama, Codex and Claude Code. Removed AGY/Copilot
choices and sign-in commands are unavailable; legacy saved profiles stay on disk
untouched. Speech choices are local only; speech key controls are absent. Credentials never reach the page or note exports.

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
`usage::Usage` keeps content-free counts in memory since server startup, and saves tokens by day, purpose and model in `usage_days` for 90 days (Settings shows seven), separated
by answer, continuation, recovery, grading, summary, other writing and quick review.
Settings shows token/cache counts, unreported/estimated usage, tool failures/reused
reads, and median/p95 timing from at most 64 samples per category. Native agents
may make additional internal requests; these are not subscription-balance estimates.

A hidden check resolving conflicting quiz verdicts is authoritative for the card
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
magnifies nearby bars (they stay inside the 48 px gutter, at least 6 px from the
text) and previews the heading and a bounded plain-text excerpt (up to 24 lines /
800 UTF-16 units, escaped); clicking or Enter/Space jumps there. The preview always
shows (`outlineRoom`): between the rail and the text, else right of the text, else over
the text (2026-11-06, the owner: "these preview boxes should show even if they cover
the text"; this reverses the earlier same-day "free space only" rule). It sits above the
margin layer (z 12) and ignores the pointer.
Page chrome is not selectable (2026-11-06, the owner): `body` is `user-select: none` and
the allow list in `index.html` (inputs, `.doc`, `.prose`, chat and margin text, errors,
transcripts, descriptions, code) turns it back on; new text that people would copy needs a
class on that list. `.related`, `.doc-tail` and blank blocks stay off it, so a drag down
the note stops at its end instead of painting empty space. A press on non-selectable,
non-control chrome clears the selection and Ask Felix (`passages.js`), except in
`[data-keep-selection]` (Ask Felix, `.page-bar`) and the resize grips.
Arrow keys, Home and End select a preview, Escape dismisses it, and Reduced Motion
snaps the bars. All 300 markers fit within the viewport. The existing disclosure
below the rail opens the full nested outline; narrow panes keep their disclosure.
The wide rail is vertically centred in the reading viewport, including short
notes. Its dropdown reveals over 240 ms and retracts over 192 ms from its toggle,
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
has a 1px rule (the Trash list, Calendar form and menus included).
Selection is ink (segment and nav pills are `--raised`, switches `--text`);
indigo stays for focus, links and the search pulse. Writing AI uses a pen icon,
never a sparkle.
Settings is a centered navigation-and-content grid. The nav and section are
centered together, without an asymmetric window background band or measured edge.
Sticky navigation keeps its own scroll; cards have 20 px gaps. Below 760 px the
nav becomes a sideways-scrolling chip strip with search above it. The current
section stays visible, and search results replace the strip while searching.

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
the centre, focuses it (`tabindex=-1`) and rings it once with `pulse(target, { ring: true })`;
Reduced Motion skips the pulse and shows an outline until blur. Routes stay `#/settings/<section>`; Keeping
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
A remembered Keeping section at the Settings root keeps Settings selected and
Back returns to the previous note; only the explicit Keeping route selects Keeping.

`Powers.app_settings` supplies session-gated bounded JSON `/api/app` and
`/api/health`; blocking native work runs off async tasks and outside the store
lock. Lifecycle events only flush drafts;
recording
reserves `capture_activity` before creating a job; its lease prevents updater
installation through the durable save.

Arriving on a page closes sheets left from before but keeps one opened on the new
address (`scrim.openedOn`): input can run ahead of the queued `hashchange`.
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
model-download and native-recording display polls pause while hidden. Automatic status GETs do not reset user-idle time.
Graph/vector/caption maintenance starts after its first interval and defers while
the native window is hidden or the capture activity gate is busy.

Application controls and draggable note/folder rows suppress text selection;
prose and editable fields remain selectable. The page admits native drag starts
only from its explicit note/folder sources, preventing accidental link, image and
selection drags while keeping file drops and pointer-driven board/resize actions.

Page toolbars share the same 760 px maximum width across notes, workspace, recording
and Settings. Every Settings section uses the same Settings / section breadcrumb;
Keeping and Trash keep their routes and controllers inside that frame.
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
Usage (2026-11-06, the owner: no small modal that grows): the sheet opens at its final
size (`min-height`) with `.skeleton` shaped as two plans of two rows, then the saved
readings come in with `cascade`, first draw only. Refresh and Close are icon buttons (labels
kept; `.activity-spin` replaces the arrows while busy); "Plan limits" is a muted subtitle,
and "Updated …" sits in each plan's header (`planUpdated`). A window past its reset says
"Reset", one never read says "Not read yet", both with "Refresh to check". Opening POSTs
`/api/settings/usage` with `{}`; the Refresh button POSTs `{ "claude": true }`; a `note` in the
response shows as a quiet line. Usage and New task share the `sheetroom` container on both
`.scrim` and `.scrim-gone`, so a closing ghost keeps its layout.

Calendar tasks use 13 px two-line titles, taller all-day/month rows and a 60 px
minimum timed block. Overlap layout includes that visual minimum; saved task
times stay unchanged. Google export, .ics download and delete/stop-tracking
actions use icon controls with accessible names and hover titles in both peeks
and task sheets.
A block that would hang past midnight (a deadline at 23:59 with its 60 px minimum) is placed
higher (`calColumn` `placed`, 2026-11-06) so it never adds empty scroll under the grid; its
label and drags still use the real start.
New task (2026-11-06, the owner: change the dimensions freely) is two panes at 720 px when the
scrim is 760 px or wider (a container query, since panes shrink the viewport): title (large),
description (fills the left column), Type chips, Repeat; the date and time picker on the right.
One column below that. The footer stays pinned.
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
removed by user decision. Felix retains quiz and ask_user cards, marking,
study sessions and review. Shared progress meters, source-more, Mermaid,
KaTeX, margin notes and board tools remain independent of the retired feature.

Legacy interviews, interview_keys and interview_problem_undo tables and their rows
remain in leo.db, unused and never purged or migrated away. Old interview-style
chats open normally; their saved cards show a short retirement message. Old routes
and remembered places fall back to the note workspace. Stale
leo-interview-* localStorage keys stay untouched. Downloaded runtimes remain on
disk, untouched.

Read-along completion distinguishes valid silence from malformed/provider failure,
keeps failed changes eligible, and guards request cleanup by note and request
identity. Reading status and the explicit per-note menu sit above the title;
compact anchors stay in a separate gutter, never a mascot over the passage.

The explorer is the only folder listing: `#/` is an empty note workspace and
legacy `#/f/<path>` links select/reveal an existing folder, then replace the hash
with `#/`. Folder selection never replaces the editor. Creation chooses an
explicit target, selected folder, open-note folder, then root. Explorer actions
capture a note id/directory, folder path, or root before opening a menu/sheet;
async completion refreshes the affected note only and preserves unrelated drafts.
Moving or trashing the open note's folder flushes its draft first. Below 1000 px,
More → Folders mounts the same tree/cache in one sheet, retains paging and folder
state through operation sheets, and returns focus to More on dismissal. Crossing
to desktop closes it and focuses the rail toggle without changing its saved fold.

Reading-position anchors skip blank editor blocks and zero-width placeholders, so restoring a long note cannot match an earlier empty paragraph.

Explorer revisions: opening a different note clears the prior folder creation
scope; selecting a folder afterwards deliberately sets the creation destination.
Explicit chips and breadcrumbs reveal Folders on compact windows or unfold the
desktop explorer; passive reveals never open a sheet. Loaded metadata stays
visible during refresh, including empty folders, while generation and entry
guards reject stale reads. The rail omits Felix and the explorer has no Collapse
all control. Heading context uses creation scope, blank tree context uses root,
and nested status rows use their containing folder. Explorer sheets restore
focus to a surviving row or heading action. No-op move destinations are omitted;
slashes are valid title data but remain forbidden in folder names.

Navigation serializes the departing save against the latest destination. A failed
save retains the editor and address. Only a draft successfully persisted in this
browser offers an explicit Leave, draft kept action, which rechecks session and
persistence at activation; an in-memory-only draft cannot bypass the save.


Text-input and virtual Enter (`beforeinput` newline/paragraph/line-break) share
list continuation with keydown Enter. Shift+Enter and composition keep their own
paths. Heading previews and menus use 240 ms motion (192 ms out), a taller hover peak, and
outside clicks dismiss the heading list without swallowing the destination click.
Chats history keeps its list mounted while the desktop column or compact overlay
animates; collapsed content is inert, the toggle exposes its expanded state, and
Escape cancelling delete confirmation stops later same-panel abort handlers.

The heading disclosure opens on mouse hover without taking focus, stays open across its summary and menu, and closes on leaving both. Click, keyboard and touch disclosure and outside dismissal remain available. Diagram replies prefer sparse monochrome SVG sketches with serif labels for pictorial subjects; Mermaid stays strict and SVG stays an inert image.
