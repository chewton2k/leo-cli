# leo-web: `leo serve` and the page

leo-web depends only on leo-core. Everything that needs services (AI, speech,
settings, import, storage housekeeping) arrives as a `Powers` field handed in
from `src/cli/` (writer, chat streamer, settings, importer, listener, reader,
seer, meaning, web, room).

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
  `graph.js`, `schedule.js`, `interview.js`, `saving.js`, `audio-queue.js`,
  `recording.js` (tests in `tests/*.test.js`).
- Everything leo-web computes or keeps besides notes and uploads lives in one
  SQLite file, `<data>/leo.db` (`db.rs`; rusqlite bundled, WAL, 0600, never
  synced): chats (+ FTS5), chat documents, review progress, picture captions,
  meaning vectors, the knowledge graph, calendar feeds, note-task metadata,
  reading positions, interviews. Old JSON files are brought in on first open
  and renamed `*.before-database`. Writes touch only what changed.

## Serving and security

- Sign-in: a code in `<config>/serve-token` (0600, `--new-token` rotates and
  signs everyone out); `?token=` on GET `/` → 303 + a per-browser
  `leo_session` cookie (30 days, `Secure` behind https) kept in
  `serve-sessions.json` (`sessions::Sessions`). A browser that already has a
  valid session reuses it. Constant-time compare, no CORS, strict headers,
  the store reloaded from disk on every request.
- "This computer" is the TCP peer (`Peer { loopback }` from
  `into_make_service_with_connect_info`), never a header. `local_request`
  (computer recording sources) and `secure_request` (keys, calendar links)
  require it; a forwarded `https` counts only from loopback (cloudflared).
- The tunnel is the default (`tunnel::start`, cloudflared quick tunnel,
  `--protocol auto`, bounded startup and log reading); `--local` is Wi-Fi
  only. `serve` opens the local link itself in a terminal; `LEO_NO_OPEN`
  skips it (e2e and Playwright set it). On start, sessions made through old
  trycloudflare hosts are retired.
- Page CSP: no inline JS (`script-src 'self'`), `data:` images, `frame-src
  'self'` (original viewer), `font-src 'self'` (KaTeX). Mermaid 11.4.1, KaTeX
  0.16.11 and CodeMirror parsers are vendored under `web/vendor/` (checked
  against npm's sha512, served `immutable`).
- Keys and calendar links are write-only: never returned to the page. A stored
  key shows as `secret::masked` placeholder; nothing changes until Save.

## Page architecture

- Hash router (`#/f/dir`, `#/n/id`, `#/new/dir`, `#/search/q`, `#/trash`,
  `#/map[/id]`, `#/calendar`, `#/interviews[/chat]`, `#/record[/dir]`,
  `#/settings[/storage]`), `data-action` delegation (`actions.js`). Every
  page bumps `seq`; a slow load whose `seq` changed draws nothing.
- Notes read wide (2026-10-19, the user's call): `body.on-note main` uses
  `--note-width`, 980 px by default; Settings → Appearance → Note width picks
  Comfortable (760 px), Wide or Full width, kept per browser in
  `leo-note-width` and set early by `/theme.js` as `html[data-note-width]`.
- Quick Leo (2026-10-19, the user's request): Cmd/Ctrl+J anywhere (Cmd+L
  belongs to the browser's address bar), the sidebar row under Search, or the
  phone menu opens one line (`sheets.js` `quickLeo`). It works like Spotlight
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
- Drag handles (Felix's edge, the original pane's grip, the interview split,
  the page bar, a picture's resize corner, the whiteboard and the knowledge
  graph) end on pointerup, pointercancel, lostpointercapture or a move
  with no button held, so a release the page never sees (outside the window,
  a context menu) cannot leave a drag stuck (2026-10-19). A whiteboard shape
  drawn or moved when that happens is kept, not thrown away (2026-10-20).
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
- Reuse what exists: `set-card`, `set-row`, `set-input`, `btn sm|plain|primary|danger`,
  `chip`/`pick-chip`, `sheet()` (swipe-to-close on phones), toasts with Undo,
  `library-views` segmented control, settings-style `<select>`. Readability:
  `flows.spec.js` measures every visible text's contrast in light and dark
  (≥3:1).
  Filled danger buttons, error toasts and chat deletion confirmations reuse
  `--accent-text` for legible text on colored backgrounds in both themes.
  Chat deletion icons stay fully opaque on touch screens; readability checks
  seed multiple chats so unselected icons are covered too.
  The calendar's completed-task toggle sits at the right of the agenda's
  date heading, with the date wrapping on narrow screens.
  Interview section disclosures reuse `source-more` and `ICON.chevron`,
  including the existing keyboard behavior and quick expand/collapse motion.
  Native dropdowns share `select-control` and the same SVG chevron across
  the library, Settings, Storage, uploads and recording. Keep their labels,
  native options and keyboard behavior; forced colors restores the OS arrow.
  Icon buttons share centered flex alignment. Disclosure selectors affect
  only their own summary, so opening a parent never rotates a child's icon.
  Felix menus reuse `ICON`; Arrow keys move between items and Escape closes
  the menu and returns focus to its button without closing the conversation.
- Motion (reworked 2026-10-10, the user found the first pass tacky): only what
  follows a click, quick (`MOTION_MS` 180), transform/clip/height only. No
  entrances, hover lifts or sliding gliders. `<details>` slide their height
  (`slideHeight`); Felix slides in/out (`CHAT_LEAVE_MS`); sheets and toasts
  ease out (`LEAVE_MS`). Folder navigation has one short arrival on the
  content. Reduced Motion turns all of it off. Tapping Felix is the one
  playful exception (the user asked, 2026-10-14): `TAPS` (boop, hop, spin,
  giggle, tickle) and, one tap in `MONSTER_ONE_IN` (10), `monster`: he grows,
  turns purple with horns, fangs, claws and a "RAWR!", then shrinks back
  (2.8 s; taps during it are ignored). Demon Felix (2026-10-14, the user's
  request): Settings → Appearance → Felix, Cute or Demon, kept per browser in
  `leo-felix` and set early by `/theme.js` as `html[data-felix="demon"]`. He
  is purple with horns, red eyes, fangs and claws everywhere he appears, and
  taps play `DEMON_TAPS` (rawr, cackle, blaze, glare, swipe). His surprise is
  `sweet`: cute Felix, big, with hearts and "hi!!". Either surprise comes one
  tap in `MONSTER_ONE_IN`, or always after `TICKLED_TOO_LONG` (8) taps each
  within 1.5 s of the last.

## Notes, editing and browsing

- No editor view: `editing.js` splits the body into blocks; `doc.js` renders
  each with `markdown.js` and swaps a tapped block (or the lines a selection
  covers, like Obsidian's live preview) for a textarea of its raw lines.
  Autosave PATCHes 700 ms after the last change with `base` = the loaded
  `version` (FNV-1a); 409 → "<title> (conflict from phone)". `saving.js`
  keeps edits that did not reach leo and retries them on the next load.
  Undo: up to `MOST_UNDO` snapshots per note (typing grouped by pauses).
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
- Sidebar (`side.js`, ≥1000 px): places, a Top level row (styled like a
  folder row, outside the scrolling tree so it is always a drop target), the
  folder tree (`folderTree`, open state in `leo-side-open`, only `.side-tree`
  scrolls and keeps its position), Trash / Storage / Settings at the foot,
  foldable to icons (`leo-side`). Phones use the ⋯ menu (`sheets.js`
  `menu()`); its Calendar row shows a due badge (overdue + today).
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
- Context (`gather`): explicitly attached notes (`refs`, ≤8) and the open
  note come first and share a bounded budget. Automatic retrieval adds at most
  four notes total across recent chat sources, word matches, meaning matches and
  graph neighbours. Automatic excerpts are at most 1,500 characters, focus on
  the matching passage and preserve original line numbers; `open_note` reads
  more on demand. Gathered notes never exceed 32,000 characters, even for large
  model windows; smaller provider budgets still apply. Recording/upload synthesis
  budgets are independent. The prompt keeps what changes least first (documents, notes,
  earlier chat, conversation); the `<today>` line (it changes every minute)
  and the interview context go last, before the reply line
  (`chat::before_reply`), so compatible provider prefix caches reuse the rest. Claude Code places its own cache breakpoints, so across
  answers it reuses only the system prompt (measured: 10.5k of 31.8k tokens). Note bodies are line-numbered; citations `[n2 L14-18]`
  and `[f1 p4]` become chips. Citations never go into notes
  (`tools::linked` → `[[Title]]`).
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
- Long chats: `turns_for(room)` keeps recent turns verbatim, older ones go in
  `<earlier_in_this_chat>` with a background summary (`MEMORY_EVERY`).
  History carries who wrote each answer, what Felix did, cards and passages,
  so switching models mid-chat loses nothing.
- Styles (`MODES`): chat, study, interview. Study marks critically (a right
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
  when the user goes on, or with their next message. If the quick check is
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
  is refused; a refused call shows as "Fixed a tool call" with the reason
  (`why_it_failed`) when expanded. Text written
  before a tool call is kept (a paragraph break, not a restart). A broken
  agent session sends `{"reset"}` and is redone through the text protocol
  (`FALLBACK_TRIES`). Web tools only for AIs without their own web
  (`web::allowed`, public addresses only).
- Access (`tools::Access`, `leo-felix-access`, Shift+Tab): Ask first (cards to
  Apply), Auto (applied at once, cards come back applied with Undo), Read only
  (change tools left out of the manual and refused).
  Changes during a reply POST to `/api/chat/{answer}/access`; both protocols
  check the live setting before tool execution and automatic application.
  A failed permission update stops the reply; a closed stream prevents further changes.
- Read tools: search_notes, open_note, connected_notes, read_document,
  read_transcript, read_original, look_at_picture, calculate (`calc.rs`, a
  safe evaluator), list_schedule, list_trash, search_chats.
- Change tools: edit_note / create_note (cards; Apply POSTs
  `/api/notes/{id}/suggestion`, 409 unless `find` is there exactly once),
  and the organizing tools in `routes/organize.rs`: add_task (`repeat`:
  `mon | wed`), update_task (`on` = which day of a repeating task),
  delete_task, move_note, make_folder, move_folder, delete_note,
  rename_note, rename_folder (`new_name`), pin_note, restore_note,
  delete_folder (always a card, even in Auto). They validate against the
  store, become `Proposal` cards, run through POST `/api/felix/act` (reusing
  the normal routes) and return the inverse card as `undo`.
- Steering: messages sent while Felix works are added after the next tool
  step (`steer.rs`). Esc stops an answer, or closes the panel.
- Files for Felix (`chat_files`, ≤10 per chat, text only, deleted with the
  chat), passages (Ask Felix on a selection, ⌘⇧E), Add to note
  (`routes/insert.rs`, version-checked, request ids dedupe double presses),
  Save as note, chat history (`chats.rs`, local naming), cost line
  (`spent`), drawings (Mermaid for flows and charts, SVG otherwise).
- Interview practice (2026-10-12): see the root CLAUDE.md and
  `.superpowers/plans/2026-10-12-felix-interviews.md`; `interview.rs`,
  `interview_tools.rs`, `web/interview.js`, `app/interviews.js`. Generated
  code never runs, except the key's reference solution in the sandbox; async
  updates never replace a focused editor.
  Problems are ordered snapshots of their challenge and workspace (128 max,
  within the existing 1 MB practice limit). Legacy attempts become reopenable
  problems. `interview_challenge` appends; `interview_problem` reopens;
  revision-checked Select and workspace saves also handle archived problems.
  Practice order follows the latest real workspace change or nonempty session
  chat activity. Reads and identical saves preserve timestamps and revisions;
  opening an empty chat never moves a practice.
  Problem deletion is revision-checked and removes only that problem's workspace,
  attempts and hidden checks in one transaction. Felix uses interview_problem_delete
  through Ask/Auto/Read and the same proposal/Undo controls. Recovery snapshots stay
  server-side (20 total, Undo expires after 24 hours), are excluded from exports,
  and appear in Storage → Deleted problem Undo; clearing a practice removes its
  snapshots too. Undo restores the original position and selected problem, retains
  later work on other problems, and never restores an in-flight key generation.
  Key workers must still find the actual problem before claiming or finishing.
  Submission freezes an attempt and keeps the editor for iteration. Problem
  switching flushes drafts before selecting; model-created problems preserve
  pending edits before switching. The global Felix button opens the active
  practice conversation; Back saves then returns to the practice list.
  Redraws after Felix answers (refreshInterview) keep every expanded
  section open: the question, progress, submissions, result and checks are
  drawn through `keepOpen`, which reopens `<details>` by `data-keep` (each
  submission's id) or their summary text.
  Opening a practice while Felix is open switches him to that practice's chat
  (`chat.useInterview`, interview style, a toast names the practice); while
  he is answering elsewhere it does not interrupt and says so.
  All active practices use available width and adapt when Felix opens.
  The code editor is tall by default (`--code-height`, shared by the text
  and the line numbers: 480 px to 960 px, `100dvh - 320px`;
  written answers 400 px). A browser test keeps buttons at least 8 px from
  the text and sections around them on the main pages and practice tabs.
  Coding wraps with aligned syntax and line numbers,
  and adapts when Felix opens. Python deletion on blank indented lines uses
  four-column stops and native Undo; there is no Indent code action.
  Coding languages use a themed select, defaulting to Python. Vendored CodeMirror
  editing commands drive indentation and paired brackets; the textarea applies
  only the changed range and keeps page scroll, native Undo and IME input intact.
  Rust apostrophes type normally so lifetimes and loop labels are never auto-closed.
  Send flushes the draft and binds context to `interview_problem`, with the
  problem number, title and tab retained on the user message in chat history.
  Reading progress preserves that binding even if the user switches problems.
  Whiteboards use a pannable/zoomable viewport over bounded ±1,000,000 coordinates
  and work like a slide editor (2026-10-13, the user's request): white paper and
  black ink in every theme (`INK`; snapshots draw their own white); icon tools
  (`BOARD_TOOLS`: Select and move, Pen, Box, Arrow, Text; V P R A T) with Select
  and Pan merged. Select hits a shape's whole area (`board-hit` transparent fills
  and wide strokes), Shift adds or removes, dragging empty space draws a marquee
  that picks shapes fully inside (`shapesWithin`), and a touch drag, Space or the
  middle button pans. One selected box or drawing gets eight handles, an arrow its
  two ends (`handlesFor`, `resizeShape`); labels only move. Double-click (detected
  in pointerdown, since redraws reset the browser's click count) or Enter edits
  a label's, box's or arrow's text in place (`.interview-board-editor`, Enter saves,
  Escape cancels; an emptied label is deleted); double-click empty space or click
  with Text to add a label. Box and arrow tools return to Select after placing a
  shape (a plain click gives a default size); the pen stays. Keys: Delete,
  ⌘/Ctrl A C X V D Z (⇧Z or Y redo), arrows nudge (Shift for more), Escape
  deselects (and only then falls through to Back); handled keys stop at the board.
  Reset affects only the current tab of the captured problem. Board Undo and Redo
  keep up to 20 edits; a reset's Undo also restores its viewport. Other tabs stay
  saved. The saved shape kinds and limits did not change.
  Past submissions carry a status label and matching outline
  (`interviews.attemptStatus`: checks decide, then the quick review, then Felix's
  scores; Passed, Failed · n/m, Partly, Awaiting review). The top bar is opaque
  (no backdrop blur), which stopped it flickering over sticky and animating content.
  The question card and the workspace share the width through a draggable
  separator (`.interview-split`, from 900 px; `--split` on `.interview-solve`,
  25-70%, default 45%, kept in `leo-interview-split`): drag, arrow keys (Shift
  for 10%), Home or double-click to reset, End for the widest question.
  Practice preferences ("What would you like to practise?") take up to
  `MOST_WANTS` (8,000) characters.
  Practice schedules were removed (2026-10-13, the user's call): no schedule
  section, session editor, `interview_schedule` tool or proposal, `Interview.schedule`
  or `Change::Schedule`; old rows keep loading (the field is ignored) and an old
  schedule card in a saved chat says schedules are no longer part of leo. The
  preferences form keeps the interview date and drops the weekly minutes field
  (the stored value is sent back unchanged).
  The problem card is tabbed like a judge site (`drawInterviewPanes`, the shared
  `library-views` control, sticky under the header, arrow keys move): Question;
  Test cases (coding with key tests: the visible examples and the last Run, which
  switches here); Result (only for this page's latest submission of the open
  problem, `Result · passed/total`; Submit switches here and scrolls the card
  into view only when its top is off screen); Submissions (this problem's
  attempts, then shared progress). Switching problems returns to Question.
  Submission results (`paintInterviewResults`): a passed/failed summary, one
  horizontally scrolling strip of case chips (`interviews.resultCases`: tests
  as Case n, answers as Answer n, parts by name) acting as tabs, and one detail
  panel: the input (hidden tests too, once submitted: `CheckResult.hidden`;
  old attempts saved as "Hidden test" have none),
  your output and expected output in bounded wrapping boxes, and "This check
  looks wrong" for a failed case, which names the check to Felix. The selection
  (`session.resultCase`, `interviews.pickCase`: first failure, else the first
  case) survives redraws of the same submission and resets for another; choosing
  a case repaints only the strip state and the panel, never the editor or page
  scroll. The quick review shows its counts and summary and folds the
  per-criterion feedback into `source-more`. Grading errors are kept per
  submission.
  Felix can reach every practice (2026-10-14, the user's call): `interview_list`
  (every practice with its numbered problems), an optional `interview` on
  `interview_progress`, `interview_attempt` and `interview_tests`, and
  `interview_problem_edit` (a JSON merge of the changed fields onto a saved
  problem; `interview::update_problem` checks the problem still matches what
  was read, never touches the workspace or attempts; a
  `Proposal::InterviewProblemEdit` card in Ask first, applied with Undo in
  Auto, refused in Read only; the page redraws the question). New problems
  carry their provenance only in `origin` and `sources`: the question and the
  reply never say whether it is a real or official company question or where
  it came from (also in `interviews.preparation`).
  Changing the coding language in Preferences (2026-10-14, the user's call)
  rewrites the open problem's starter too: POST `language` asks the quick
  model (`interview_keys::translate_starter`, metered as `language:`) for the
  same function name in the new language, `interview::relanguage` saves it
  only if the problem is still what was read, and the code in the editor
  becomes the new starter; the toast's Undo restores the old language,
  starter and code exactly (`restore`, no AI). SQL problems never switch;
  other saved problems keep their own language.
  Topics (2026-10-15, the user's request): every problem carries 1-3
  `topics` (any field: DFS, Dynamic programming; Random walks for quant), at
  most `MOST_TOPICS`, distinct ignoring case. Felix is told to tag every new
  problem and to reuse `topics_in_use` (in `<interview_material>`) instead of
  near-duplicates; `interview_challenge` refuses a problem with no topics.
  Older or mistagged problems are retagged together with
  `interview_topics` (one `Proposal::InterviewTopics` card listing each
  problem's old and new topics; `interview::set_topics` checks each problem's
  topics are still what was read; Undo swaps them back; Auto applies, Read
  only refuses). Every listed problem needs at least one topic, and the tool
  result names the problems still without one so Felix finishes the job;
  "Sort into topics with Felix" is in the menu while any problem has none. The practice
  page has a filter button with a funnel icon before the problem chips that
  opens a dropdown (the Settings `.picker` menu: All topics, each topic with
  its count, No topic; arrow keys, Escape and an outside click close it;
  `interviews.topicFilter`), remembered
  per practice in `leo-interview-topic:<id>`; the question shows its topics.
  Each topic row says how many of its problems are solved (`2/5 solved`).
  The practice list shows type · company (or General) · position and
  problems, submitted and solved, each problem counted once
  (`interview::list` `tally`; `interview::passed` mirrors
  `interviews.attemptStatus` passed, and node and Rust tests share cases).
  Each track has a colour and icon (`TRACK_LOOK`, from the `--hue-*` tokens:
  coding blue, systems teal, hardware cyan, design violet, marketing orange,
  business amber, quant green, custom pink) on the track cards, the practice
  rows (with a bar of solved over problems in that colour) and the practice
  heading.
  Hardware (2026-10-20, the user's request: diagrams and equations matter
  most): criteria Fundamentals, Diagrams, Derivations, Trade-offs,
  Communication. Felix puts the circuit, datapath, state machine or waveform
  in the question as ```svg/```mermaid and writes equations in `$…$`; keys add
  value asks with units and diagram criteria. The board has Circle (O, state
  machines) and Line (L, wires) besides Box and Arrow (`BOARD_KINDS` in
  `interview.js` and `interview.rs`). Felix reads the board as shapes with
  `from`/`to` naming what each arrow or line joins (`interview::board_outline`,
  also the quick review's `<board>`); review snapshots scale with the board
  up to 2000 px wide (uploads cap there) so labels stay readable. A written
  answer with maths gets a live Preview under it (`interviews.hasMath`) and is
  rendered in its submission.
  Problem pictures (2026-10-14, the user's request): the question is Markdown,
  so a ```mermaid or ```svg block in it is drawn like in notes, and each
  coding example may carry a `diagram` (same kind of block, at most
  `MOST_DIAGRAM` 6,000 characters) shown under it. Felix is told to put a
  picture the user asks for on the problem with `interview_problem_edit`
  (and explain with it in the chat), to add one himself for inputs that are
  hard to picture (trees, graphs, grids, intervals, systems), and never to
  draw the approach or the answer.
  `interview_code` reads the full bound draft. `interview_code_edit` suggests one
  exact unique fragment per Accept/Decline card in Ask first; in Auto
  (2026-10-14, the user's call) the proposal arrives with `auto: true` and the
  page applies it at once through the same merge path, with Undo on the card.
  Apply and contextual Undo change only that problem’s code, including archived
  problems, and refuse stale/ambiguous fragments. Read only refuses suggestions;
  the page flushes drafts and merges non-overlapping typing during application.
- Felix activity records its position in the streamed response, stays between
  the corresponding passages after reload, and is excluded from copied note
  selections. Legacy activity without positions stays at the beginning.

## Interview instant feedback (2026-10-12, `interview_keys.rs`)

Submit used to wait 30-90 s for Felix. Now the thinking happens while the user
works (plan: `.superpowers/plans/2026-10-12-interview-instant-feedback.md`):

- When `interview_challenge` saves a problem, `interview_keys::prepare` asks the
  writing AI on a thread for a hidden answer key (`interview_keys` table in
  leo.db, status preparing / ready / failed with its reason). A key is a mix of
  `tests` (coding: `{args, expected, hidden, unordered}` for the starter's
  `entry`; data only, never code), `asks` (value with tolerance or a ratio
  `factor`, or choice), `sections` (parts a strong answer covers, matched by
  words ignoring plurals), `criteria` (rubric with `good` and `misses`) and a
  `reference` outline. Keys are read leniently (a string where a list was
  asked) and validated; a failed key leaves Submit working with the review.
- The page sees only `Key::public` (visible tests, ask labels and options,
  section names) via GET `/api/interviews/{id}/key`, polled every 1.5 s while
  preparing (with `progress`); hidden tests come from POST `hidden-tests` at
  Submit, and once the problem has a submission `public(true)` shows every
  test with its `hidden` flag (2026-10-14: hidden only until submitted).
  References, answers and criteria never reach the page before grading.
- Keys carry a JavaScript reference `solution` (never shown). While `verify`
  is set the page shows no tests: POST `key/check` gives the solution, tests
  and a `stamp`; the page runs it in the sandbox and POST `key/checked` drops
  every test it disagrees with (`Key.dropped`), unless most fail or it errors,
  then the key is kept and marked `doubtful`. A stale stamp changes nothing.
- Felix sees the active problem's tests (all, with hidden flags), whether they
  matched the reference, the last Run (POST `run` → `Key.last_run`) and the
  latest submission's checks in `<test_cases>` (`interview_keys::tests_material`),
  and has `interview_tests` (read) and `interview_test` (add, edit, delete one
  test: a `Proposal::InterviewTest` card in Ask first, applied with Undo in
  Auto, refused in Read only; at least one visible test always stays).
- Submit: the page runs every test in the sandbox, PUT `submit` carries
  `tests` reports, the server grades asks and sections instantly into
  `Attempt.checks`. No AI runs on Submit (2026-10-19, the user's call): the
  result offers **Review with Felix** (`review-result`; submissions keep
  "Ask Felix to review", `review-attempt`), which runs POST `grade` (the quick
  model, `Powers.quick`: the writing AI's cheapest model at low effort,
  `ai::quick_config`) into `Attempt.grade`, then Felix's full review in the
  chat, told the results (`interviews.reviewNote`). Check coverage
  (POST `coverage`) says which criteria a draft covers, never judging them.
- Running code: `/runner.html` + `/runner.js` (public, CSP `RUNNER_POLICY`:
  sandbox allow-scripts, opaque origin, so requests carry no leo cookies) start
  a fresh blob Worker per JavaScript or Python run, so an
  endless loop is stopped by `terminate` without freezing the page (a
  sandboxed frame can share the page's thread). Python is Pyodide 0.28.3,
  fetched once on first use (`leo_services::pyodide`, SHA-256 pinned, about
  12 MB) and served at `/pyodide/{name}` (allowlisted, CORS `*`, immutable).
  TypeScript: the page loads `/typescript.js` (sucrase, vendored by
  `scripts/runner-tools`) to strip types, then runs the result as JavaScript.
  SQL (SQLite via sql.js, vendored, `/sql/{name}` with CORS `*`): key `entry`
  is `sql`, each test's one argument is the setup SQL, expected is the rows as
  arrays; the worker runs setup then the user's query on a fresh database per
  test. C++ (2026-10-14): the writing machine fetches @yowasp/clang 22 once
  (npm tarball, 27 MB, SHA-256 pinned, each unpacked file pinned too;
  `leo_services::clang`, `/api/cpp`, `/clang/{name}` with CORS `*`; the
  Python and C++ downloads share `routes/python.rs` `Kind`s). The page makes a
  driver (`interviews.cppDriver`: reads the entry's signature, fills int,
  floating, bool, char, string and nested vectors from the JSON args, calls
  `Solution().entry` or the free function, prints `@@leo id <json>` per test,
  user output goes to Printed output; pointers such as ListNode are refused
  with a plain message). The page fetches the bundle (runner.js never calls
  fetch; a test checks) and the runner starts it as a
  classic worker (module workers do not start in the opaque-origin sandbox),
  compiles `-O1 -std=c++20 -fno-exceptions`, and runs the program in a fresh
  worker with a small WASI shim so an endless loop is stopped. Measured in
  Chromium: first compile and run 2.2 s, then about 1.3 s.
  Go (2026-10-14): the real Go 1.27.2 compiler and linker (`cmd/compile`,
  `cmd/link` built for wasip1) plus 82 precompiled standard-library archives
  (`scripts/go-runner`: `GO_VERSION`, `PACKAGES`, `build.sh`; 21 MB packed).
  CI builds it, checks its `files.sha256` against `MANIFEST` (= 
  `leo_services::gorunner::FILES_SHA256`; the `go-runner` CI job fails if a
  build ever differs) and attaches `leo-go.tar.gz` to each release; leo fetches
  its own version's asset (then latest), unpacks it and checks every file.
  `/api/go`, `/goroot/{path}` (plain paths only, CORS `*`). The page loads the
  toolchain once per visit (`interviewGoToolchain`, a progress bar) and
  transfers it to the runner, which keeps it in a worker on the vendored WASI
  shim (`/wasi-shim.js`, @bjorn3/browser_wasi_shim, in-memory files). The
  driver (`interviews.goDriver`) adds `package main` and any used standard
  imports on line 1 (errors keep the user's line numbers), supplies the
  LeetCode ListNode/TreeNode types when only commented, and a `leo_driver.go`
  that fills arguments with reflect and JSON (lists and level-order trees
  from arrays), recovers panics per test and prints `@@leo <json>`. Programs
  run in a fresh worker on the small shim, so loops are stopped. Measured in
  Chromium: first run 2.2 s, then 0.8 s; compile errors 0.14 s. Yaegi was
  tried first and dropped: it got `a, b = f(x), f(y)` with recursion wrong.
  Java (2026-10-14): javac and TeaVM compiled to WebAssembly GC by
  teavm-javac (`scripts/java-runner`: `COMMIT`, `JDK_VERSION`, leo's
  `inner-classes.patch` so `Map.Entry` and other nested library types resolve,
  `NOTICE` with the licences and source links; about 4.3 MB packed). Its builds
  differ between machines (same JDK), so it is not pinned: every release
  builds and attaches its own `leo-java.tar.gz`, and leo checks the files
  against the `files.sha256` inside (`runtimes::JAVA.files_sha256` is None); `/api/java`, `/javaroot/{four names}`. The page loads it
  once per visit; the runner turns TeaVM's ES-module loader into a classic
  script (`teaRuntime`), compiles `LeoMain.java` (the user's classes with
  `public` blanked and `package` removed, same lines, plus `interviews.javaDriver`'s
  `LeoMain` with typed literals for ints, floats, chars, strings, arrays, lists,
  ListNode and TreeNode from arrays, a JSON printer, and the LeetCode node
  classes when only commented) and runs the program in a fresh worker. Needs
  WebAssembly GC (current Chrome, Edge, Firefox, Safari). Measured in Chromium:
  first run 1.4 s, then 0.8 s.
  Rust (2026-10-14): Miri (the Rust interpreter) compiled to WebAssembly with
  the parts of a mid-2024 standard library a plain program needs, taken
  unchanged from rubri at a pinned commit (`scripts/rust-runner`: `COMMIT`,
  `FILES` pins each file, `NOTICE`; 46 MB packed, 131 MB unpacked). CI
  gathers and pins it (`runtimes::RUST`) and attaches `leo-rust.tar.gz`;
  `/api/rust`, `/rustroot/{path}` (`.wasm`, `.rlib`, `files.sha256` for the
  list). `interviews.rustDriver` keeps the user's code first (same lines),
  adds `struct Solution;` and the LeetCode node types when missing, typed
  literals (`vec!`, `String::from`, `leo_list`, `leo_tree`), a JSON trait,
  and a `main` that marks each test with `@@leo-start` and starts at
  `LEO_FROM`. This Miri cannot unwind, so a panic ends the run:
  `interviews.rustRound` records it from stderr and the page runs again from
  the next test. Measured in Chromium: first run 1.75 s; a compile error
  0.25 s; three panicking tests 0.86 s.
  Other languages get the quick and full reviews only.
- Progress (`progress.rs`, 2026-10-14): long AI jobs get a `Meter` on
  `AppState.progress` (`key:`, `grade:`, `coverage:` keys, GET
  `/api/progress/{key}`). It counts streamed characters (keys stream through
  `Powers.chat`) against the expected size, and with time it moves at an even
  pace to 90% at the expected time, then slows towards 97% (never past) until
  the job ends; it never curves early, so it does not crawl and then jump.
  The expected time is the upper quarter of the last 12 finished runs of that
  kind (the usage monitors' P90 idea; pace per character for sized jobs), kept
  in leo.db `timings` so it survives restarts; a run that wrote nothing is
  not learned. The view says about how long is left or "taking longer than
  usual" (`slow`, past 1.25 x expected). The page glides between polls and
  `finishMeter` fills the bar to 100% "done" before the result replaces it. The page draws
  `meterBar` (percent, elapsed, about how long). Python's download reports
  bytes on disk. Background panel, upload and recording bars creep within a
  step using the average step time and show elapsed time; Felix's answer in
  progress shows "Working · n s".
- Measured with Claude Code (Opus 5.5 writing, Haiku quick): problem in 16-19 s,
  key ready about 30 s later, Run 1.0 s, Submit results 0.1 s, quick review
  8-9 s, coverage 4 s.

## Uploads, originals, recording

- Uploads: POST `/api/import` runs the `Importer` on a thread; "What do you
  want from it?" goes in `<what_the_user_wants>`. Originals are kept in
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
- Recording (`record.rs`, `recording.js`, worklet `recorder.js`): one job at a
  time. Browser sources POST 16 kHz PCM each second (`UPLOAD_SAMPLES`
  batches); computer sources need `local_request`. `record_journal.rs`
  persists before acknowledging; the page spools unacknowledged chunks in
  IndexedDB (`audio-queue.js`). Recovery replays through bounded channels.
  Failed recordings can be dismissed (audio kept) or deleted. The live page
  is notepad first; the transcript is folded. Calendar events now (or within
  15 min) are preselected and sent as `profile.context`.
- PDF export (Share → PDF, or the browser's print): print CSS hides every
  fixed overlay (dock, tasks, source pane, toasts, chat, FABs…), zeroes the
  layout variables and the scrollbar gutter, and uses fixed `@page` margins
  (22/24/24 mm), so the PDF is the same on any screen; `beforeprint` also
  marks anything else `position: fixed` with `data-print-off`.
- Background panel and Tasks panel share `#dock` (bottom left; activity on
  top). On the knowledge graph the dock moves right of the map tools
  (`body.on-map .dock`) so zoom and Tidy stay reachable.

## Knowledge graph (`graph.rs`, `graph.js`; "map" in code)

Summaries and concepts per note, links between note groups (only group pairs
with meaning neighbours), cached in leo.db; builds follow the model's room
(`Scale::for_room`, parallel requests) and keep themselves current when idle
(`keep_graph_current`: 2 min idle, retry after 10 min). Canvas view styled on
Obsidian's graph; Tidy up reduces crossings. Pictures get one-time captions in
the same idle loop.

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
  `schedule.plan(tasks, events, today, {from, to})` expands occurrences; the
  Tasks panel shows the next open one; ICS and Google get a weekly RRULE.
- Event sheet (2026-10-19): `schedule.eventDetails` splits the event
  context into place, organiser (a Google group or resource calendar ID reads
  "A shared calendar"), invited (chips) and description; `schedule.richText`
  renders the description from its HTML keeping only line breaks, bold,
  italics, lists, paragraphs and https/mailto links (`noopener`), dropping
  scripts, styles, images, attributes and other tags, and links bare URLs in
  plain text.
- Calendar page: colors follow the event title (`--hue-0..7`) and task kind
  (`TASK_TONES`); month dots, an agenda with a now line, Coming up, Overdue,
  No date. Completed tasks show by default; "Hide completed" is remembered
  (`leo-calendar-hide-done`). Add to Google Calendar opens Google's prefilled
  page (no sign-in).
- Tasks panel: in `#dock`, appears when there are open tasks (Overdue / Today /
  Coming up 7 days / No date), folds (`leo-tasks-folded`) to one round icon
  with a count badge (`.tasks-pill`, red when something is overdue) that opens
  the box again, hidden on the Calendar page and under 600 px. Folding clips
  the box down into the icon's corner and opening grows it back out
  (`toggleTasksWindow`, a `MOTION_MS` clip-path; none with Reduce Motion; a
  second press cancels a running open).
  On the library its height leaves the first row of notes clear
  (`fitTasksWindow` sets `--tasks-room`, at least `TASKS_LEAST_ROOM`, on page
  redraws and resizes). With less room than that it waits as its icon
  (`tasksWin.cramped`; opening it then is not remembered), and the library
  pads its bottom by the dock's height (`--dock-room`) so the last notes can
  scroll clear of it.
  Active interview practices start with Tasks folded so its expanded rows
  cannot block the workspace. Show still expands it; this local
  practice choice leaves the remembered preference for other pages intact.
  Completing a panel task shows its checked, struck-through row for 1.2 seconds
  before a `MOTION_MS` collapse; Reduce Motion skips animation but keeps the
  confirmation. A bounded occurrence snapshot survives polling until removal,
  and Undo cancels removal without completing the next repeating occurrence.
- Export adds `tasks.json`; Storage → Restore tasks takes that file and POSTs
  `/api/tasks/import` (ids dedupe; a newer local copy is kept).

## Settings, Storage, export

- Settings (`SettingsApi` from `src/cli/web_settings.rs`): one model picker
  per AI with prices and effort chips; keys only on `secure_request`.
- Theme (2026-10-14, the user's request): Settings → Appearance picks Match
  this device, Light or Dark, kept per browser in `leo-theme`. Theme and
  Felix choices are large `.choice-card`s (icons `device`, `sun`, `moon`;
  Felix pictures with `felix-preview cute-look` / `demon-look`, which the
  mood code skips and which look the same whatever mode is on). `/theme.js`
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

- Browser suite: `tests/browser/flows.spec.js` (desktop and phone projects;
  CI retries once, `github` reporter). Every test fails on a page error or a
  programmer-error toast (`guard`). Recording is stubbed; the graph is seeded.
  Draft recovery uses an init-script fetch failure fake that survives reloads
  and page teardown; Playwright routing can release requests during unload.
  `visual.spec.js` screenshot baselines are local only (`pnpm visual`,
  `pnpm visual:update` rewrites every image).
- Per the user (2026-10-11): no local Cargo builds for website work and no
  full local runs; quick JS checks and one-test runs only, CI does the rest.
- Real-model checks (Codex, Claude Code) are what the prompt wording rests on:
  intent comes from the conversation, never keyword guesses; a change is only
  claimed after its tool result.
  The 2026-10-12 review used Claude Code through a real local server and a
  throwaway home: relative dates, repeating tasks, per-day Undo, note/folder
  actions and Ask/Auto/Read access passed 16 action/access turns. A live Study
  plan, quiz and correct answer ended with its recap. Installed
  Chrome checks covered 28 page/theme/viewport combinations with no overflow
  or contrast findings, plus danger controls across both themes and sizes.
  The interview follow-up used six signed-in Codex turns in a throwaway home:
  workspace creation, preserved drafts, reopening, calendar-aware planning,
  reviewed submissions and continued iteration passed. Native and text fakes
  cover the same tools; installed Chrome checks cover problem switching,
  recovery, wrapping, Python deletion/Undo and chronological activity in both
  themes on desktop and phone. Grid/List alignment is measured against both
  button centers; restoring the old CSS reproduces the three-pixel offset.

Storage includes interview workspaces, attempts and schedules with Open practice,
separate hidden check metadata, reading positions, note task settings and cached
calendar events. Selected/bulk deletion is transactional; deleting a practice
also deletes its checks while preserving notes, tasks and conversations. Check
workers update only their claimed generation while the parent practice exists,
so clearing or deleting data cannot be undone by a late worker. Calendar cache
clearing waits for refresh and invalidates expansions without disconnecting feeds.
Task sizes use actual files, including deletion markers; erasing markers is
explicit because older backups may restore them. Database overhead/unused space
is separate from payload sizes, with safe compaction; folder walks are bounded
at 100,000 entries and 64 levels and omit symlinks. Storage lists 100 rows at a
time, preserves open sections/selection/focus, and refreshes browser data only
inside its own section so file pickers and export choices stay intact. It shows
browser draft/preferences and IndexedDB recovery audio separately from the server.
Browser cleanup affects only Leo keys or selected recordings, never other apps;
deleting a practice forgets its local backups. Export's Chats and interview
practices option includes saved practice data, never hidden keys or secrets.

Interview Enter retains manual indentation in unfinished and blank lines while
CodeMirror handles new scopes, continuations and leaving multiline delimiters;
the final edit is still bounded at 32,000 characters. Attempts and reviews are
filtered by the selected challenge ID and shown beneath that question. Overall
practice progress uses all attempts; late reviews cannot appear under another
problem, even when problem titles match.

Website backup: page hide or moving the site to the background flushes note and
interview drafts and sends one small authenticated JSON beacon per departure.
The server coalesces requests, allows saves to settle, and pushes notes and task files
outside the store lock, at most once every 20 seconds. Settings Off is respected;
missing remotes and empty queues do nothing. Pushes are noninteractive, limited
to 60 seconds, never pull/merge, and failures remain visible on the next visit.
Chats and interview practices remain local; this does not change backup contents.

Felix’s user-facing prose follows chat::REPLY_STYLE across all chat styles,
interview key/reference writing and quick grading. Inspired by
https://github.com/ayghri/i-have-adhd, it puts the answer first, teaches one
manageable step, groups longer material and ends with a next action only when
needed. Required derivations, assignment parts, evidence, JSON schemas, quiz
markers and strict assessment remain complete; no diagnosis or fixed word cap.
Recording/note synthesis and its gap-filling rules are unchanged.

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

Runner workers share startWorker: each temporary script Blob URL is revoked on
the first worker message/error, termination (including timeout), or constructor
failure. Program workers, including Python, terminate after each run; compiler
workers stay warm. Revoking the URL frees its Blob reference; repeated runs must
retain no script URLs.

### Security and cleanup boundaries (2026-10-16)

Sign-in files use unique synced 0600 temporary files and atomic replacement;
loading rejects symbolic/hard links, nonregular files and oversized data, and
repairs existing permissions. Session persistence failures refuse authentication.
Original attachments and standalone tasks check every component inside their data
root, use nofollow regular-file reads and atomic writes, and refuse symbolic links.
Legacy task migration removes only rows whose matching task was safely retained.

The code runner serializes at most four page jobs, rejects overlapping iframe
execution, validates bounded result schemas and cleans up startup listeners/timers
on every exit. Python workers are fresh and terminated after every reply, including
forged replies; compiler workers intentionally remain reusable. Browser workers
provide interruption, not a hard RAM quota. Background recording waveform queues
remain bounded even when the recording page is closed.

Imports admit two active jobs and two body readers, cap uploads at 20 files/90 MB,
expire incoming bodies after 30 seconds and retain at most 40 job summaries.
Detached store checks retain their own admission permits through completion.
Original bytes stage in private temporary files instead of a second full RAM copy.
Exports bound traversal, input size and simultaneous builds/streams; owned temporary
paths and admission permits are released when a stream ends or its client leaves.
Tunnels run `cloudflared` in an isolated process group. Startup failure and normal
shutdown signal the group and await reaping with a five-second deadline; Drop and
cancellation signal directly and use Tokio's child reaper. No external `kill`
process is spawned. `Awake` kills/reaps `caffeinate` on drop. Graph/idle task guards
start after fallible setup and abort on every server exit/cancellation. Shared
worker states use RAII guards to prevent stuck meters/locks on panic; interview
key preparation has eight permits and retries abandoned generations without
allowing late workers to replace a newer generation.

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
Storage until explicitly cleared. Speech choices and cloud speech keys stay
available. Credentials never reach the page or note exports.

Interview problem navigation shows the active title and opens a bounded searchable
sheet instead of growing numbered chips. Search matches titles, topics and numbers;
status filters reuse per-challenge attempts and count any passed submission as solved.
Opening or filtering the chooser changes no workspace, order or timestamp. Search
focus survives refreshes; closing returns to the current chooser button even if it
was redrawn. Switching and explicit deletion flush drafts first. A problem removed
by Felix never gets recreated by a late draft save; newer local work remains
recoverable when Undo restores that problem.

The interview topic picker measures the visible space outside the sticky header,
flips above its button when needed and caps its own scrolling height. Its controls
stack above question tabs; scrolling or resizing recomputes the available space.

Interview editor access follows the saved practice conversation, independently of
Chat, Study or Interview response style. A matching visible workspace is saved
before sending in every style; another conversation never inherits that scope.
The viewed problem ID is validated against the practice and refreshed in warm
sessions. An explicitly empty find initializes only a still-empty editor; stale
typing is refused and insertion supports the same independent cards and Undo.
Both native and text replies recover once from an explicit false claim that the
editor tools are unavailable, before any changing/interactive tool and while
editing is allowed. Interview reads keep that single recovery available; retries
reuse the latest read and share the original tool-call budget. A bounded 512-character prefix holds that refusal before display; normal
text streams when the prefix fills or a tool runs. Already shown text is never
replaced for this check. A repeated refusal keeps the code and shows an actionable
retry message. Legitimate execution limitations and Read only are not retried.

Practice recency follows real saved work and conversation changes. Selecting a saved problem persists its selection/revision without changing activity; filtering stays browser-local. Run advances parent activity atomically with its results without invalidating an editor revision. Background test preparation and verification do not count.

Practice titles use the existing inline plaintext heading style: typing autosaves through the practice queue, Enter finishes, Escape abandons unsaved typing, and empty names restore the saved title. Saves preserve the caret and pending code; Retry reloads current preferences before renaming, refuses a changed remote workspace before advancing the local revision, and draft flushing includes pending titles.

Successful Felix replies may contain only proposal, ask or quiz cards. Stopped
means cancellation; normal empty completion offers retry guidance. Saved replies
retain client elapsed milliseconds and a cancellation flag, including card-only
turns, so future slow turns can be distinguished from canceled replies.

Study sends an append-only model history that flattens completed quiz hints and
answers without mutating earlier question cards; display/storage keep those cards.
This permits Read-only warm reuse. Quiz documents share the 64k material budget
(full documents remain readable by tools); ordinary chat/interview history budgets
stay unchanged. Unchanged practice snapshots are omitted from warm follow-ups;
changed code, tests, questions, notes and documents refresh their fingerprints.
Permission changes invalidate reuse. Four foreground answers and bounded reply
queues prevent unlimited workers or queued chunks; closed replies skip follow-up
tools, retries and grading. Two identical failed calls end tool use with one final
answer. Hidden Study grading remains available and its usage is included. Concurrent
grades of one interview attempt share one worker; distinct attempts remain separate.
Retained sessions count instructions, incoming material, tool results and replies
toward the warm limit; API sessions measure serialized history and native CLIs track
tool arguments and results. Oversized sessions are dropped promptly
instead of remaining cached until another request or idle cleanup.

Server shutdown drains active requests and streams for at most five seconds; a
second Ctrl-C skips the drain. Graph/idle tasks stop on the first interrupt.
Warm-session cleanup and tunnel shutdown share a one-second deadline, with the
tunnel's Drop guard retaining responsibility for signaling its process group.

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

Felix efficiency (2026-10-18): initial chat titles use the existing first-message
and attached-note naming locally, without an AI request. Long-chat memory uses
`Powers.housekeeping` (the writing provider's quick model), keeping note synthesis
on the selected writing model. `read_cache::Reads` retains at most 16 successful
large reads and 256 KB per conversation. Every read revalidates current saved data;
only identical results become short references. Changes, Undo and errors never
reuse stale content; `fresh: yes` requests the full result. Failed/fresh provider
sessions clear these references. `interview_workspace` combines the requested
problem's complete code, reasoning, board, progress and tests without selecting it;
full frozen submissions still use `interview_attempt` before assessment.

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
