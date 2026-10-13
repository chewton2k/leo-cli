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
- `api()` treats an empty body as `null`; every success is JSON or a bodiless
  204 (a browser test sweeps the API).
- Layout variables on `body`: `--side` (sidebar, 0 under 1000 px), `--right`
  (Felix and/or the original pane), `--mid`. Fixed things (toolbar, select
  bar, toasts, FABs, `#dock`, the map) use them so nothing covers anything.
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
  content. Reduced Motion turns all of it off.

## Notes, editing and browsing

- No editor view: `editing.js` splits the body into blocks; `doc.js` renders
  each with `markdown.js` and swaps a tapped block (or the lines a selection
  covers, like Obsidian's live preview) for a textarea of its raw lines.
  Autosave PATCHes 700 ms after the last change with `base` = the loaded
  `version` (FNV-1a); 409 → "<title> (conflict from phone)". `saving.js`
  keeps edits that did not reach leo and retries them on the next load.
  Undo: up to `MOST_UNDO` snapshots per note (typing grouped by pauses).
- `markdown.js` escapes first, numbers checkboxes exactly like
  `Note::toggle_checkbox`, draws `[[Title]]` links, callouts (`> [!type]`,
  `-`/`+` dropdowns), ```mermaid (`diagrams.js`), math (`math.js`), ```svg as
  a sandboxed `<img src="data:…">`, and local pictures (`/api/image`; outside
  images stay links). `interactive: false` makes task boxes read-only.
- Pictures live in `<notes>/attachments/` (`leo_core::attachments`, sniffed
  PNG/JPEG/GIF/WebP only, ≤20 MB); paste or drop in `doc.js`.
- Library (`browse.rs`, `app/browse.js`): Grid (default) or List. A folder
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
  Dropping a note on another opens Combine (`combine.rs`: preview, puts back
  anything the AI dropped, Undo restores both).
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
- Context (`gather`): attached notes (`refs`, ≤8), notes used earlier in the
  chat, documents, the open note and its graph neighbours, word matches,
  meaning matches; budgets scale with the model (`Powers.room`,
  `chat::scaled`), except interview turns and practice marking, which read the
  usual share (`chat::notes_room`: at most `ROOM`) since they rarely need many
  notes. The prompt keeps what changes least first (documents, notes,
  earlier chat, conversation); the `<today>` line (it changes every minute)
  and the interview context go last, before the reply line
  (`chat::before_reply`), so prefix caches (Codex, OpenAI-compatible APIs)
  reuse the rest. Claude Code places its own cache breakpoints, so across
  answers it reuses only the system prompt (measured: 10.5k of 31.8k tokens). Note bodies are line-numbered; citations `[n2 L14-18]`
  and `[f1 p4]` become chips. Citations never go into notes
  (`tools::linked` → `[[Title]]`).
- Warm sessions (`warm.rs`, 2026-10-13): one slot for all of leo. After an
  answer, its Claude Code / Codex / API session is kept (the previous one is
  stopped), so at most one idle session exists. The next answer continues it
  only when it is the same chat, the history is unchanged except the reply and
  one new message (`Warm::continues`: `seen`, `history_of`), the style, access,
  web and interview settings match and `config.toml` has not changed
  (`config_stamp`; Settings changes also `clear` it). A continued answer sends
  `chat::follow_up`: documents and notes not yet read in this session
  (`warm::renumber` keeps each note's `n` number, marks changed ones
  `updated`), the message and `<today>`; the session and its cache hold the
  rest. A mismatch stops the warm session before a new one starts; a failed
  warm answer falls back to a fresh one with full context. Practice marking
  never uses the slot. Idle 10 minutes (`IDLE`, checked every 30 s) or past
  `limit` (2 x room chars) stops it. Measured with Claude Code: first answer
  4.7 s, follow-ups 2.4 s with 99% of the prompt cached, one process.
- Long chats: `turns_for(room)` keeps recent turns verbatim, older ones go in
  `<earlier_in_this_chat>` with a background summary (`MEMORY_EVERY`).
  History carries who wrote each answer, what Felix did, cards and passages,
  so switching models mid-chat loses nothing.
- Styles (`MODES`): chat, study, interview. Study marks critically (a right
  result by wrong reasoning is `[[incorrect]]`), asks about the reasoning
  step by step, mostly free response and fill-in, and treats subjective
  material as criteria and trade-offs. Practice cards (`quiz`, `ask_user`)
  are answered in the card; Felix marks free answers (a `JUDGE` call when the
  verdict is missing). Study sessions: `study_plan` / `end_session`, quizzes
  carry `idea`, the page scores ideas and stops at `MOST_SESSION_QUESTIONS`.
  Study review (`review.rs`) brings back missed questions after 20 h.
- Tools: a text protocol (`<tool>{…}</tool>`) so every provider works, plus
  native sessions where available; generated from `tools::SPECS` (`manual()`,
  every call `check`ed; mistakes answered with the spec). At most
  `MOST_STEPS` (12) steps and `MOST_PROPOSALS` (8) changes. Codex and Claude
  Code (through leo-services' MCP bridge) and the APIs get the tools natively;
  the text protocol is the fallback. A hand-written call that is not valid
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
  Save as note, chat history (`chats.rs`, background naming), cost line
  (`spent`), drawings (Mermaid for flows and charts, SVG otherwise).
- Interview practice (2026-10-12): see the root CLAUDE.md and
  `.superpowers/plans/2026-10-12-felix-interviews.md`; `interview.rs`,
  `interview_tools.rs`, `web/interview.js`, `app/interviews.js`. Generated
  code never runs; async updates never replace a focused editor.
  Problems are ordered snapshots of their challenge and workspace (128 max,
  within the existing 1 MB practice limit). Legacy attempts become reopenable
  problems. `interview_challenge` appends; `interview_problem` reopens;
  revision-checked Select and workspace saves also handle archived problems.
  Submission freezes an attempt and keeps the editor for iteration. Problem
  switching flushes drafts before selecting; model-created problems preserve
  pending edits before switching. The global Felix button opens the active
  practice conversation; Back saves then returns to the practice list.
  Scheduling reads interview progress and `list_schedule` before proposing
  candidate gaps around saved commitments and relevant note preferences.
  All active practices use available width and adapt when Felix opens.
  Coding wraps with aligned syntax and line numbers,
  and adapts when Felix opens. Python deletion on blank indented lines uses
  four-column stops and native Undo; there is no Indent code action.
  Send flushes the draft and binds context to `interview_problem`, with the
  problem number, title and tab retained on the user message in chat history.
  Reading progress preserves that binding even if the user switches problems.
  Whiteboards use a pannable/zoomable viewport over bounded ±1,000,000 coordinates;
  labels and selected shapes drag, Enter places a label, and snapshots fit all content.
  Reset affects only the current tab of the captured problem. Board Undo retains
  up to 20 edits; a reset's Undo also restores its viewport. Other tabs stay saved.
  `interview_schedule` uses the same Ask/Auto/Read proposal and Undo flow as tasks;
  Apply and Undo reject changed schedules while allowing concurrent draft edits.
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
  section names) via GET `/api/interviews/{id}/key`, polled every 3 s while
  preparing; hidden tests come from POST `hidden-tests` only at Submit.
  References, answers and criteria never reach the page before grading.
- Submit: the page runs every test in the sandbox, PUT `submit` carries
  `tests` reports, the server grades asks and sections instantly into
  `Attempt.checks`; then POST `grade` asks the quick model (`Powers.quick`:
  the writing AI's cheapest model at low effort, `ai::quick_config`) to mark
  the criteria into `Attempt.grade`; then Felix's full review runs in the chat
  without blocking, told the results (`interviews.reviewNote`). Check coverage
  (POST `coverage`) says which criteria a draft covers, never judging them.
- Running code: `/runner.html` + `/runner.js` (public, CSP `RUNNER_POLICY`:
  sandbox allow-scripts, opaque origin, so requests carry no leo cookies) start
  a blob Worker per JavaScript run and one warm Worker for Python, so an
  endless loop is stopped by `terminate` without freezing the page (a
  sandboxed frame can share the page's thread). Python is Pyodide 0.28.3,
  fetched once on first use (`leo_services::pyodide`, SHA-256 pinned, about
  12 MB) and served at `/pyodide/{name}` (allowlisted, CORS `*`, immutable).
  Other languages get the quick and full reviews only.
- Measured with Claude Code (Opus 5.5 writing, Haiku quick): problem in 16-19 s,
  key ready about 30 s later, Run 1.0 s, Submit results 0.1 s, quick review
  8-9 s, coverage 4 s.

## Uploads, originals, recording

- Uploads: POST `/api/import` runs the `Importer` on a thread; "What do you
  want from it?" goes in `<what_the_user_wants>`. Originals are kept in
  `<data>/attachments/<note-id>/` (never synced); figures from pptx/docx/PDF
  are offered to the AI (`leo_services::figures`). Originals open in
  `#source-pane` beside the note when there is room (`sourceFits`), else in
  the viewer sheet (PDF iframe, zoomable pictures, rendered text).
- Recording (`record.rs`, `recording.js`, worklet `recorder.js`): one job at a
  time. Browser sources POST 16 kHz PCM each second (`UPLOAD_SAMPLES`
  batches); computer sources need `local_request`. `record_journal.rs`
  persists before acknowledging; the page spools unacknowledged chunks in
  IndexedDB (`audio-queue.js`). Recovery replays through bounded channels.
  Failed recordings can be dismissed (audio kept) or deleted. The live page
  is notepad first; the transcript is folded. Calendar events now (or within
  15 min) are preselected and sent as `profile.context`.
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
  `CalendarAccess` (keychain), never sent to the page. Feeds are cached in
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
- Calendar page: colors follow the event title (`--hue-0..7`) and task kind
  (`TASK_TONES`); month dots, an agenda with a now line, Coming up, Overdue,
  No date. Completed tasks show by default; "Hide completed" is remembered
  (`leo-calendar-hide-done`). Add to Google Calendar opens Google's prefilled
  page (no sign-in).
- Tasks panel: in `#dock`, appears when there are open tasks (Overdue / Today /
  Coming up 7 days / No date), folds (`leo-tasks-folded`), hidden on the
  Calendar page and under 600 px.
  Active interview practices start with Tasks folded so its expanded rows
  cannot block the workspace or schedule. Show still expands it; this local
  practice choice leaves the remembered preference for other pages intact.
- Export adds `tasks.json`; Storage → Restore tasks takes that file and POSTs
  `/api/tasks/import` (ids dedupe; a newer local copy is kept).

## Settings, Storage, export

- Settings (`SettingsApi` from `src/cli/web_settings.rs`): one model picker
  per AI with prices and effort chips; keys only on `secure_request`.
- Storage (`storage.rs` + the `Housekeeper`): every area leo keeps, with size,
  items and confirmed actions; signed-in browsers; how long trash and chats
  are kept (`/api/keep`); Export (zip of notes, optional uploads, chats,
  trash; never config or keys); Restore tasks.

## Testing

- Browser suite: `tests/browser/flows.spec.js` (desktop and phone projects;
  CI retries once, `github` reporter). Every test fails on a page error or a
  programmer-error toast (`guard`). Recording is stubbed; the graph is seeded.
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
