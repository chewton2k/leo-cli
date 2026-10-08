# leo-tui

### TUI internals

`lib.rs` holds `App`, the event loop, key dispatch and running actions; each
other concern is an `impl App` block in its own file: `backup` (auto-push),
`draw`, `mouse`, `profile` (settings keys), `pump` (draining workers), `welcome`
(set-up-when-needed screen), `writing` (editing a note in place).
`editor.rs` is the pure text buffer (lines, cursor, wrap, list continuation,
undo); `view/editing.rs` draws it with only the cursor line raw and maps
clicks back (`hit`); `view/when.rs` formats edit times. `keys.rs` is the whole keymap as a pure function, `cmdline.rs` the
`/` line, `complete.rs` completion, `task.rs` the worker and `TaskEvent`s,
`settings.rs` the profile screen's rows, and `view/` one renderer per pane
(`hints.rs` is the bottom line's per-pane key hints, `menu.rs` the `/` menu).
Tests live in `tests.rs`.

The event loop polls for input with a 120ms timeout and drains the worker
channel each tick, so a redraw happens promptly on either input or task
progress. `ratatui::init()` installs a panic hook that restores the terminal, so
a panic cannot leave the user in raw mode. Suspending for `$EDITOR` toggles raw
mode and the alternate screen on the existing terminal rather than building a
new one, and resuming resets both frame buffers instead of calling
`Terminal::clear()` — that would query the cursor position and block on the
terminal's reply.

`tui-textarea`, which the design named for the command line, is deliberately not
used: it builds against `ratatui` 0.29 while this TUI is on 0.30, so its widgets
cannot render into our `Frame` and depending on both would pull two `ratatui`
and two `crossterm` versions into one binary. `leo-tui/src/cmdline.rs` is the
replacement.

### Writing happens in the preview

Enter (or `l`, or a click) on a note starts `App.editing`
(`editor::Editor`); focus on the preview with an editor means every key goes
to `on_edit_key`, so letters type. Esc runs `finish_editing`: flush, focus the
list, and if a line is an `@leo` prompt, `run_action(Ask { note: id })`.
`flush_edit` writes body + `updated_at` and saves; `pump_editor` flushes after
`editor::AUTOSAVE` idle; `run_action` flushes first; `run()` flushes on quit.
Disk reload is skipped while editing, and writing is refused while an answer
is streaming (it would be overwritten). Bracketed paste is on (`on_paste`).
`n` opens `/new `, and `Action::New` in the TUI is `create_and_edit` (no
`$EDITOR`); `e` still opens `$EDITOR`.

### The filter drives numbering, not just the paint

`App.filter` narrows `numbering`, which is what `:edit 2` and `:delete 1` resolve
against. If it only narrowed the rendering, the numbers on screen would refer to
different notes than the numbers the user types. The filter is the one search:
`Store::find` looks in every directory, titles, bodies and tags (`#word` means a
tag), title matches first. Opening a tag sets the filter to `#tag`, and `Esc`
clears either, following the selected note into its directory. `resync` keeps
the selected note selected when an edit reorders the list.

### The layout is responsive, and there is one of it

`layout_with_tabs(area, tabs, focus)` is the only way to compute geometry. It
takes `focus` because the narrowest shape draws one pane, and it returns
zero-width rects for panes it does not draw — which means the mouse hit-test
cannot match a hidden pane without any extra code.

Three shapes, chosen by width: three panes at 90 columns and up, notes and
preview from 60, and the focused pane alone below that. The dirs and notes columns
are fixed widths and the preview holds the `Min`, so extra width goes to the note
rather than to whitespace beside its title — the notes list held the `Min` before
and grew to be wider than the pane showing the note. The breakpoints come from
what the panes need — an 18-column dirs pane plus a proportional preview squeezes
the notes list to nothing around 70 — not from round numbers. The tab strip is
dropped below 12 rows, and the command and status lines are the last things given
up.

Two things must stay true. A pane that is not drawn must not be focusable:
`Event::Resize` moves focus off a vanished pane, and `h`/`l` skip hidden ones,
or the keyboard appears to stop working. And any test that reasons about geometry
must derive it from the app's own state rather than assuming a layout — a variant
of this function that omitted the tab row put every click one line out, and the
tests missed it because they computed the layout the same wrong way.

### The provider screen

`/settings` (Action::Settings → Effect::Settings) opens `simple_rows`: per task a provider row (`ChooseProvider`), a
model row (`ChooseModel`, or `GetLocalModel` to ollama-pull / curl the starter
model outside the TUI), and a key row (`StoreKey`, once per shared account);
Enter/→ step forward, ← back (`step_setting`). `App.local_models` is a fn
field (empty `Local` in tests). Then colour, backup, paths. There is no
provider list page any more. `x` on a key row removes the key, `e` opens the
file. TUI tests set `LEO_HOME` to a temp dir (`away_from_the_real_config`):
loading config can rewrite it (tidy).

The lists are headed "AI for writing" and "AI for speech" — never "chain",
which is the code's word; a test holds that. Enter on a provider does its
`primary_action`: store a key when it has none, add it when unused, test it
otherwise. `x` removes a key, `J`/`K` change priority (capital so a mistyped
movement key cannot rewrite the config), `a`/`d` add or drop, `e` opens the
file; `l` and `t` remain as aliases for login and test.

Chain edits go through `config/edit.rs`, which uses `toml_edit` so writing one
array leaves the rest of the file — comments included — byte-identical.
Serializing the parsed `Config` back out would delete every comment, and those
comments are the file's documentation. Writes land in `config.toml.new` and are
renamed into place, so a crash mid-write cannot truncate a working config.

### Keymap

```
Enter   write in the note (Esc done)   n  new note (asks a title)
/       find or command (f, Ctrl-F)    D  delete (u undoes)   ?  help
j/k     move
h/l     switch pane          Enter   open directory / write in note
N       new dir              e       edit in $EDITOR
p       pin / unpin (frontmatter `pinned: true`, written only when true;
        pinned sort first in list_notes*, ▲ in the list and web page)
r / m   rename / move        x       tick a checkbox
a / R   /ask question / record
                             D       delete (no question; u undoes)
Space   mark (D, m act on marks)     u  undo
Esc     clears a search and lands on the pick
Tab     recent note
/       command line, with a menu (/settings opens settings)
?       help                 q       quit
```

While recording, the keyboard takes notes: typing builds a point, Enter adds
it, Esc stops; the preview always shows the live notes, never the raw
transcript. Typed points (`ai::chat::Jotted`) go into
the structuring prompt and are woven into the note, without times. The point
box wraps between words and grows to `JOT_MOST_ROWS` (6) rows
(`view::preview::typing_rows`).

`view/hints.rs` shows the keys that work in the focused pane on the idle
command line; a test checks every hinted key is documented in help.
`view/help.rs` holds `SECTIONS` (keys) and generates the Commands section from
`VERBS`. Delete is capital `D` so no single lowercase key can destroy a note.

### Completion

`leo-tui/src/complete.rs` picks a candidate source from the verb and the token's
position, measured from the end of the line where the grammar demands it —
`mv 1 2 cs130` puts the directory last, and `mv` offers directories first in
every slot since `mv cs130` moves the selection. Candidates are always leo's own
data: verbs, directories, note titles, tags. Scoring is `nucleo`, so `grtrv`
matches `Graph traversals`. `view/menu.rs` shows the candidates above the open
`/` line, with each verb's summary from `VERBS`.

Note candidates are shown as `2 Rust ownership` because a title is what the user
remembers, but only the number is a valid argument, so the title is stripped on
accept. Tab cycles matches and one Tab past the end restores what was typed.
`f` opens the search; Ctrl-F does the same. `/`, `f` and Ctrl-F open the same line: `follow_search` sets the filter
from the text unless `action::is_command` (first word is a live verb or
alias), in which case the filter returns to `search_base`; Enter on a
non-command keeps the search. Verb completion is prefix-only and shows no
aliases. `parse` strips a typed leading `/`.
