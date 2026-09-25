use super::*;

fn temp_app() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
    store.create_dir("cs130");
    store
        .create_note("Rust ownership", "- [ ] read\n- [x] done", vec!["rust".to_string()], "")
        .unwrap();
    store.create_note("Graph traversals", "- BFS", vec![], "").unwrap();
    store.create_note("Nested note", "body", vec![], "cs130").unwrap();
    store.save().unwrap();
    let store = Store::load_from(&dir.path().join("notes")).unwrap();
    (App::new(store), dir)
}

/// Notes are sorted newest-first, so find one by title rather than index.
fn select_titled(app: &mut App, title: &str) {
    let pos = app
        .numbering
        .iter()
        .position(|id| {
            app.store.find_note(id).map(|n| n.title == title).unwrap_or(false)
        })
        .expect("note is in the current listing");
    app.note_sel = pos;
}

#[test]
fn tab_completes_a_verb_on_the_command_line() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Command;
    app.cmd.open("ren");

    app.cycle_completion();
    assert_eq!(app.cmd.text(), "rename");
    assert_eq!(app.cmd.cursor(), 6);
}

#[test]
fn tab_completes_a_note_reference_to_its_number() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Command;
    app.cmd.open("edit owner");

    // Notes list newest-first, so derive the expected number rather than
    // assuming creation order.
    let expected = app
        .numbering
        .iter()
        .position(|id| {
            app.store.find_note(id).map(|n| n.title == "Rust ownership").unwrap_or(false)
        })
        .map(|i| i + 1)
        .unwrap();

    app.cycle_completion();
    // Only the number is a valid argument; the title was just for matching.
    assert_eq!(app.cmd.text(), format!("edit {expected}"));
}

#[test]
fn tab_cycles_through_candidates_and_back_to_what_was_typed() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Command;
    app.cmd.open("s");

    app.cycle_completion();
    let first = app.cmd.text().to_string();
    app.cycle_completion();
    let second = app.cmd.text().to_string();
    assert_ne!(first, second, "a second Tab must advance");

    // Walking off the end restores the original text rather than trapping
    // the user in the candidate list.
    let mut guard = 0;
    while app.cmd.text() != "s" && guard < 50 {
        app.cycle_completion();
        guard += 1;
    }
    assert_eq!(app.cmd.text(), "s");
}

#[test]
fn a_keystroke_after_tab_abandons_the_candidate_list() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Command;
    app.cmd.open("ren");
    app.cycle_completion();
    assert!(app.completing.is_some());

    // Feeding any non-Tab key through the command-mode path clears the
    // cycle, so a later Tab re-derives candidates from the new text.
    let key = event::KeyEvent::new(event::KeyCode::Char('x'), event::KeyModifiers::NONE);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    app.mode = Mode::Command;
    app.on_key(key, &mut terminal).unwrap();
    assert!(app.completing.is_none());
    assert_eq!(app.cmd.text(), "renamex");
}

#[test]
fn the_ghost_hint_shows_the_rest_of_the_top_match() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Command;
    app.cmd.open("ren");
    assert_eq!(app.ghost().as_deref(), Some("ame"));

    // Not shown once cycling has started: the line already holds the match.
    app.cycle_completion();
    assert_eq!(app.ghost(), None);
}

#[test]
fn no_ghost_hint_outside_the_command_line() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Normal;
    app.cmd.open("ren");
    assert_eq!(app.ghost(), None);
}

/// Opening the `:` line lists what can be typed, so nobody has to remember a
/// verb to find one.
#[test]
fn an_empty_command_line_lists_the_commands_with_what_they_do() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Command;
    app.cmd.open("");
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("rename"), "{out}");
    assert!(out.contains("retitle the selected note"), "{out}");
}

#[test]
fn the_menu_offers_directories_after_mv() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Command;
    app.cmd.open("mv ");
    // Narrow enough that only the notes pane is drawn, so the directory name
    // can only have come from the menu.
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("cs130"), "{out}");
}

#[test]
fn no_menu_while_the_panes_have_the_keyboard() {
    let (app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let out = terminal.backend().to_string();
    assert!(!out.contains("retitle the selected note"), "{out}");
}

/// The idle command line answers "what can I do here?", which depends on the
/// pane.
#[test]
fn the_key_hints_follow_the_focused_pane() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 20)).unwrap();

    app.focus = Pane::Notes;
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let notes = terminal.backend().to_string();
    assert!(notes.contains("n new"), "{notes}");

    app.focus = Pane::Dirs;
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let dirs = terminal.backend().to_string();
    assert!(dirs.contains("N new dir"), "{dirs}");
    assert!(!dirs.contains("r rename"), "{dirs}");
}

#[test]
fn the_frame_renders_the_completed_command_with_its_hint() {
    let (mut app, _d) = temp_app();
    app.mode = Mode::Command;
    app.cmd.open("ren");

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains(":rename"), "ghost hint is not rendered:\n{out}");
}

#[test]
fn jumping_to_a_note_follows_it_into_its_directory() {
    let (mut app, _d) = temp_app();
    let nested = app
        .store
        .list_notes(None, 100)
        .iter()
        .find(|n| n.title == "Nested note")
        .map(|n| n.id.clone())
        .unwrap();

    assert_eq!(app.current_dir, "");
    app.jump_to(&nested);

    assert_eq!(app.current_dir, "cs130");
    assert_eq!(app.selected_id(), Some(&nested), "the note is selected");
    assert_eq!(app.focus, Pane::Notes);
}

#[test]
fn jumping_to_a_note_in_the_current_directory_only_moves_the_selection() {
    let (mut app, _d) = temp_app();
    let id = app
        .store
        .list_notes(None, 100)
        .iter()
        .find(|n| n.title == "Rust ownership")
        .map(|n| n.id.clone())
        .unwrap();
    app.jump_to(&id);
    assert_eq!(app.current_dir, "");
    assert_eq!(app.selected_id(), Some(&id));
}

#[test]
fn completion_sources_come_from_the_current_directory_and_store() {
    let (app, _d) = temp_app();
    let s = app.sources();
    assert!(s.dirs.contains(&"cs130".to_string()));
    assert!(s.tags.contains(&"rust".to_string()));
    // Only notes in the current listing are numbered.
    assert_eq!(s.notes.len(), 2);
    assert!(s.notes.iter().any(|n| n.title == "Rust ownership"));
}

#[test]
fn x_toggles_the_first_open_checkbox_of_the_selected_note() {
    let (mut app, _d) = temp_app();
    select_titled(&mut app, "Rust ownership");
    let id = app.selected_id().cloned().unwrap();

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    app.on_intent(Intent::ToggleCheckbox, &mut terminal).unwrap();

    assert!(
        app.store.find_note(&id).unwrap().body.contains("- [x] read"),
        "body: {}",
        app.store.find_note(&id).unwrap().body
    );
}

/// In the preview, j and k step between checkboxes and x ticks the one the
/// cursor is on, so any box is one keypress away instead of `:check 3 5`.
#[test]
fn in_the_preview_x_ticks_the_checkbox_under_the_cursor() {
    let (mut app, _d) = temp_app();
    select_titled(&mut app, "Rust ownership");
    let id = app.selected_id().cloned().unwrap();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();

    app.focus = Pane::Preview;
    app.on_intent(Intent::Down, &mut terminal).unwrap();
    assert_eq!(app.box_index(), 1);
    app.on_intent(Intent::ToggleCheckbox, &mut terminal).unwrap();

    let body = &app.store.find_note(&id).unwrap().body;
    assert!(body.contains("- [ ] done"), "the second box was not unticked: {body}");
    assert!(body.contains("- [ ] read"), "the first box changed: {body}");

    // The cursor cannot run past the last box.
    app.on_intent(Intent::Down, &mut terminal).unwrap();
    assert_eq!(app.box_index(), 1);
}

/// Ticking a box makes the note the newest, which moves it to the top of the
/// list; the selection has to move with it rather than land on whichever note
/// slid into its old row.
#[test]
fn the_selection_follows_a_note_that_moved_in_the_list() {
    let (mut app, _d) = temp_app();
    select_titled(&mut app, "Rust ownership");
    let id = app.selected_id().cloned().unwrap();
    let before = app.note_sel;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    app.on_intent(Intent::ToggleCheckbox, &mut terminal).unwrap();
    assert_ne!(app.numbering.iter().position(|n| *n == id), Some(before), "fixture did not reorder");
    assert_eq!(app.selected_id(), Some(&id));
}

#[test]
fn in_a_preview_without_checkboxes_j_scrolls() {
    let (mut app, _d) = temp_app();
    select_titled(&mut app, "Graph traversals");
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    app.focus = Pane::Preview;
    app.on_intent(Intent::Down, &mut terminal).unwrap();
    assert_eq!(app.preview_scroll, 1);
}

#[test]
fn choosing_another_note_puts_the_checkbox_cursor_back_at_the_top() {
    let (mut app, _d) = temp_app();
    select_titled(&mut app, "Rust ownership");
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    app.focus = Pane::Preview;
    app.on_intent(Intent::Down, &mut terminal).unwrap();
    app.focus = Pane::Notes;
    app.on_intent(Intent::Up, &mut terminal).unwrap();
    assert_eq!(app.box_index(), 0);
}

/// Space marks notes; D and m then act on every marked note at once.
#[test]
fn marked_notes_are_deleted_together_and_come_back_together() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    app.focus = Pane::Notes;
    let before = app.store.notes.len();

    select_titled(&mut app, "Rust ownership");
    app.on_key(press(' '), &mut terminal).unwrap();
    select_titled(&mut app, "Graph traversals");
    app.on_key(press(' '), &mut terminal).unwrap();
    assert_eq!(app.marked.len(), 2);

    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("2 marked"), "the status line does not say: {out}");

    app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();
    assert!(matches!(
        &app.mode,
        Mode::Confirm { on_yes: leo_core::action::ConfirmedAction::DeleteNotes { ids }, .. } if ids.len() == 2
    ));
    app.on_key(press('y'), &mut terminal).unwrap();
    assert_eq!(app.store.notes.len(), before - 2);
    assert!(app.marked.is_empty(), "marks outlived the notes");

    app.on_intent(Intent::Undo, &mut terminal).unwrap();
    assert_eq!(app.store.notes.len(), before);
}

#[test]
fn space_again_unmarks_and_esc_clears_every_mark() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    app.focus = Pane::Notes;
    app.on_key(press(' '), &mut terminal).unwrap();
    app.on_key(press(' '), &mut terminal).unwrap();
    assert!(app.marked.is_empty());

    app.on_key(press(' '), &mut terminal).unwrap();
    app.on_key(press_code(event::KeyCode::Esc), &mut terminal).unwrap();
    assert!(app.marked.is_empty());
}

// ── typing while recording ─────────────────────────────────────────────

fn recording_app(events: Vec<TaskEvent>) -> (App, tempfile::TempDir) {
    let (mut app, d) = temp_app();
    let req = ListenRequest { screen: false, title: None, append_to: None, dir: String::new() };
    app.recording = Some(Recording::new(task::Job::scripted(events), req));
    (app, d)
}

fn type_str(app: &mut App, text: &str, terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>) {
    for c in text.chars() {
        app.on_key(press(c), terminal).unwrap();
    }
}

/// While recording, what you type is a point, and Enter adds it — letters
/// that are keys elsewhere, like t, are just letters here.
#[test]
fn typing_while_recording_jots_points() {
    let (mut app, _d) = recording_app(vec![]);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    type_str(&mut app, "trees are graphs", &mut terminal);
    app.on_key(press_code(event::KeyCode::Enter), &mut terminal).unwrap();

    let rec = app.recording.as_ref().unwrap();
    assert!(!rec.job.stop_requested(), "Enter stopped the recording");
    assert_eq!(rec.jotted.len(), 1);
    assert_eq!(rec.jotted[0].text, "trees are graphs");
    assert!(rec.jot.is_empty());
    assert!(!rec.show_raw, "t toggled the raw view instead of typing");
}

#[test]
fn tab_switches_raw_text_while_recording() {
    let (mut app, _d) = recording_app(vec![]);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    app.on_key(press_code(event::KeyCode::Tab), &mut terminal).unwrap();
    assert!(app.recording.as_ref().unwrap().show_raw);
}

/// Esc stops, and a half-typed point is kept rather than lost.
#[test]
fn esc_stops_and_keeps_a_half_typed_point() {
    let (mut app, _d) = recording_app(vec![]);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    type_str(&mut app, "last thing", &mut terminal);
    app.on_key(press_code(event::KeyCode::Esc), &mut terminal).unwrap();
    let rec = app.recording.as_ref().unwrap();
    assert!(rec.job.stop_requested());
    assert_eq!(rec.jotted.last().unwrap().text, "last thing");
}

#[test]
fn your_points_show_above_the_live_notes() {
    let (mut app, _d) = recording_app(vec![]);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
    type_str(&mut app, "BFS uses a queue", &mut terminal);
    app.on_key(press_code(event::KeyCode::Enter), &mut terminal).unwrap();
    type_str(&mut app, "half", &mut terminal);
    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("Your points"), "{out}");
    assert!(out.contains("BFS uses a queue"), "{out}");
    assert!(out.contains("half"), "the line being typed is not shown: {out}");
}

/// Nothing was said, but points were typed: they are still a note.
#[test]
fn typed_points_are_saved_even_without_speech() {
    let (mut app, _d) = recording_app(vec![TaskEvent::Finished { transcript: String::new() }]);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    type_str(&mut app, "read chapter 4", &mut terminal);
    app.on_key(press_code(event::KeyCode::Enter), &mut terminal).unwrap();
    let before = app.store.notes.len();

    app.pump_tasks(&mut terminal).unwrap();
    assert!(app.recording.is_none());
    assert_eq!(app.store.notes.len(), before + 1);
    assert!(app.store.notes.iter().any(|n| n.body.contains("**read chapter 4**")));
}

/// The bug this guards: work below the UI printed to stdout while the panes
/// owned the screen, so git's commit summary and config warnings landed on
/// top of the notes list. They now arrive as status-line messages instead.
#[test]
fn a_background_warning_becomes_a_status_message_not_terminal_output() {
    let (mut app, _d) = temp_app();
    leo_core::diag::set_quiet(true);
    leo_core::diag::clear();

    leo_core::diag::warn("could not read the stored credential for \"groq\"");
    assert!(app.pump_diagnostics(), "the warning was not picked up");

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 12)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let out = terminal.backend().to_string();
    assert!(
        out.contains("could not read the stored credential"),
        "the warning never reached the status line:\n{out}"
    );

    leo_core::diag::set_quiet(false);
    leo_core::diag::clear();
}

#[test]
fn pumping_with_nothing_queued_reports_no_change() {
    let (mut app, _d) = temp_app();
    leo_core::diag::set_quiet(true);
    leo_core::diag::clear();
    assert!(!app.pump_diagnostics());
    leo_core::diag::set_quiet(false);
}

/// `D` means "delete what is selected", so which pane has focus decides
/// whether that is a note or a whole directory.
#[test]
fn d_in_the_dirs_pane_asks_to_delete_the_directory() {
    let (mut app, _d) = temp_app();
    app.focus = Pane::Dirs;
    // temp_app builds cs130 with one note in it.
    app.dir_sel = 0;

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
    app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();

    match &app.mode {
        Mode::Confirm { prompt, on_yes } => {
            assert!(prompt.contains("cs130/"), "prompt: {prompt}");
            assert!(prompt.contains("1 note"), "prompt: {prompt}");
            assert_eq!(
                *on_yes,
                leo_core::action::ConfirmedAction::DeleteDir { path: "cs130".to_string() }
            );
        }
        other => panic!("expected a confirmation, got {other:?}"),
    }
    // Still there until confirmed.
    assert!(app.store.dir_exists("cs130"));
}

#[test]
fn confirming_in_the_dirs_pane_removes_the_directory_and_its_notes() {
    let (mut app, _d) = temp_app();
    app.focus = Pane::Dirs;
    app.dir_sel = 0;
    let notes_before = app.store.notes.len();

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
    app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();
    let yes = event::KeyEvent::new(event::KeyCode::Char('y'), event::KeyModifiers::NONE);
    app.on_key(yes, &mut terminal).unwrap();

    assert!(!app.store.dir_exists("cs130"));
    assert_eq!(app.store.notes.len(), notes_before - 1);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn declining_the_confirmation_keeps_the_directory() {
    let (mut app, _d) = temp_app();
    app.focus = Pane::Dirs;
    app.dir_sel = 0;
    let notes_before = app.store.notes.len();

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
    app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();
    let no = event::KeyEvent::new(event::KeyCode::Char('n'), event::KeyModifiers::NONE);
    app.on_key(no, &mut terminal).unwrap();

    assert!(app.store.dir_exists("cs130"));
    assert_eq!(app.store.notes.len(), notes_before);
}

/// `..` is navigation, not a directory to destroy.
#[test]
fn d_on_the_parent_entry_deletes_nothing() {
    let (mut app, _d) = temp_app();
    app.current_dir = "cs130".to_string();
    app.focus = Pane::Dirs;
    app.dir_sel = 0; // the ".." row
    assert_eq!(app.dir_rows()[0].target, "..");

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
    app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();

    assert_eq!(app.mode, Mode::Normal, "no confirmation was raised");
    assert!(app.store.dir_exists("cs130"));
}

/// The notes pane keeps its old meaning.
#[test]
fn d_in_the_notes_pane_still_targets_a_note() {
    let (mut app, _d) = temp_app();
    app.focus = Pane::Notes;
    select_titled(&mut app, "Rust ownership");

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
    app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();

    match &app.mode {
        Mode::Confirm { on_yes, .. } => assert!(matches!(
            on_yes,
            leo_core::action::ConfirmedAction::DeleteNote { .. }
        )),
        other => panic!("expected a note confirmation, got {other:?}"),
    }
}

/// `:delete` on its own means the note on screen, which is what someone who
/// just selected it expects.
#[test]
fn a_command_without_a_note_acts_on_the_selected_one() {
    let (mut app, _d) = temp_app();
    select_titled(&mut app, "Rust ownership");
    let id = app.selected_id().cloned().unwrap();

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
    app.run_line("delete", &mut terminal).unwrap();

    match &app.mode {
        Mode::Confirm { on_yes: leo_core::action::ConfirmedAction::DeleteNote { id: target, .. }, .. } => {
            assert_eq!(*target, id)
        }
        other => panic!("expected a confirmation for the selected note, got {other:?}"),
    }
}

#[test]
fn r_opens_the_command_line_with_the_title_ready_to_change() {
    let (mut app, _d) = temp_app();
    select_titled(&mut app, "Rust ownership");
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
    app.on_intent(Intent::RenameSelected, &mut terminal).unwrap();
    assert_eq!(app.mode, Mode::Command);
    assert_eq!(app.cmd.text(), "rename Rust ownership");
}

/// The finished text is written through the same seam a live model would
/// use, so titles, tags and appending behave identically.
#[test]
fn a_ready_note_is_written_through_the_normal_path() {
    let (mut app, _d) = temp_app();
    let before = app.store.notes.len();
    let req = leo_core::action::ListenRequest {
        screen: false,
        title: None,
        append_to: None,
        dir: String::new(),
    };
    let ready = ReadyNote {
        title: Some("Lecture 4".to_string()),
        body: "- a point".to_string(),
    };

    let outcome =
        action::apply_transcript(&mut app.store, &req, "ready", &ready).unwrap();
    assert!(outcome.dirty);
    assert_eq!(app.store.notes.len(), before + 1);
    let note = app.store.find_by_title("Lecture 4").first().copied().unwrap();
    assert_eq!(note.body, "- a point");
    assert_eq!(note.tags, vec!["listen"]);
}

#[test]
fn a_ready_note_without_a_title_still_saves() {
    let (mut app, _d) = temp_app();
    let req = leo_core::action::ListenRequest {
        screen: false,
        title: None,
        append_to: None,
        dir: String::new(),
    };
    let ready = ReadyNote { title: None, body: "- body".to_string() };
    action::apply_transcript(&mut app.store, &req, "ready", &ready).unwrap();
    assert_eq!(app.store.find_by_title("Untitled Notes").len(), 1);
}

/// A user-supplied title still wins over whatever the model produced.
#[test]
fn a_ready_note_respects_a_title_the_user_chose() {
    let (mut app, _d) = temp_app();
    let req = leo_core::action::ListenRequest {
        screen: false,
        title: Some("My Title".to_string()),
        append_to: None,
        dir: String::new(),
    };
    let ready = ReadyNote {
        title: Some("Model Title".to_string()),
        body: "- body".to_string(),
    };
    action::apply_transcript(&mut app.store, &req, "ready", &ready).unwrap();
    assert_eq!(app.store.find_by_title("My Title").len(), 1);
    assert!(app.store.find_by_title("Model Title").is_empty());
}

/// Waiting must look like waiting: a spinner and a clock for unknown work,
/// a real bar when the step count is known.
/// A retired name must explain itself in ONE message: the status line holds
/// a single one, so a two-part explanation loses its first half.
#[test]
fn a_retired_command_explains_itself_in_one_message() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 14)).unwrap();

    app.run_line("d 1", &mut terminal).unwrap();
    let (kind, text, _) = app.message.as_ref().expect("a message");
    assert_eq!(*kind, Kind::Warn);
    assert!(text.contains("`d`"), "does not name the old command: {text}");
    assert!(text.contains(":delete"), "does not name the replacement: {text}");

    app.run_line("env", &mut terminal).unwrap();
    let (_, text, _) = app.message.as_ref().expect("a message");
    assert!(text.contains("Ctrl-S"), "{text}");
    assert!(text.contains("keychain"), "does not say why: {text}");
}

// ── streaming ask ───────────────────────────────────────────────────────

/// The bug this guards: the write-back used ReadyNote, whose expand_prompts
/// echoes its input, so the answer was streamed to the screen and then thrown
/// away when the note was saved.
#[test]
fn an_answer_is_written_back_and_not_echoed() {
    let expanded = PreExpanded {
        body: "the answer".to_string(),
        count: 1,
    };
    let (body, count) = action::Ai::expand_prompts(&expanded, "@leo question", "T").unwrap();
    assert_eq!(body, "the answer", "the original body was returned instead");
    assert_eq!(count, 1);

    // And the listen path's type still echoes, which is what it is for.
    let ready = ReadyNote {
        title: None,
        body: "structured".to_string(),
    };
    let (body, _) = action::Ai::expand_prompts(&ready, "unchanged", "T").unwrap();
    assert_eq!(body, "unchanged");
}

/// A note with no prompts must not start a job at all.
#[test]
fn asking_a_note_without_prompts_starts_nothing() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    let id = app.selected_id().cloned().unwrap();
    assert!(
        !app.store.find_note(&id).unwrap().body.contains("@leo"),
        "fixture note should have no prompts"
    );

    app.run_action(Action::Ask { note: "1".to_string() }, &mut terminal)
        .unwrap();
    assert!(app.asking.is_none(), "a job was started with nothing to ask");
    let (_, message, _) = app.message.as_ref().expect("a message");
    assert!(message.contains("No @leo prompts"), "{message}");
}

/// Two asks at once would race to write the same note.
#[test]
fn a_second_ask_is_refused_while_one_is_running() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    // Stand in for a running job without making a request.
    app.asking = Some(Asking {
        job: task::start_ask(String::new(), String::new(), String::new()),
        progress: view::progress::Progress::spinner("Asking"),
        since: Instant::now(),
        text: String::new(),
    });

    app.run_action(Action::Ask { note: "1".to_string() }, &mut terminal)
        .unwrap();
    let (_, message, _) = app.message.as_ref().expect("a message");
    assert!(message.contains("one at a time"), "{message}");
}

/// Text arriving must show in the preview, or streaming is invisible.
#[test]
fn a_streaming_answer_appears_in_the_preview() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.asking = Some(Asking {
        job: task::start_ask(String::new(), String::new(), String::new()),
        progress: view::progress::Progress::spinner("Asking"),
        since: Instant::now(),
        text: "ownership means".to_string(),
    });

    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("ownership means"), "{out}");
    assert!(out.contains("answering"), "no indication it is still arriving: {out}");
}

// ── recent notes ────────────────────────────────────────────────────────

/// Moving the selection is visiting a note, and the strip must show it.
#[test]
fn visiting_notes_builds_the_recent_strip() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    assert!(app.note_count() >= 2);

    app.on_intent(Intent::Down, &mut terminal).unwrap();
    app.on_intent(Intent::Up, &mut terminal).unwrap();

    let tabs = app.tabs();
    assert_eq!(tabs.len(), 2, "both visited notes should be listed");
    // The note on screen is the current tab, and it is first.
    assert!(tabs[0].current, "the current note is not marked");

    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    let title = app
        .store
        .find_note(app.selected_id().unwrap())
        .unwrap()
        .title
        .clone();
    assert!(out.contains(title.split(' ').next().unwrap()), "{out}");
}

/// Tab returns to the previous note, which is the whole point of the list.
#[test]
fn tab_jumps_back_to_the_previous_note() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.remember_visit();
    let first = app.selected_id().cloned().unwrap();
    app.on_intent(Intent::Down, &mut terminal).unwrap();
    let second = app.selected_id().cloned().unwrap();
    assert_ne!(first, second);

    app.on_intent(Intent::JumpRecent, &mut terminal).unwrap();
    assert_eq!(app.selected_id(), Some(&first), "Tab did not go back");

    // And again returns to where we came from.
    app.on_intent(Intent::JumpRecent, &mut terminal).unwrap();
    assert_eq!(app.selected_id(), Some(&second));
}

/// The note on screen at startup counts as visited, or the first Tab has
/// only the current note to offer and refuses.
#[test]
fn the_note_on_screen_at_startup_is_recorded_as_visited() {
    let (mut app, _d) = temp_app();
    app.recent = crate::recent::Recent::default();

    app.remember_visit();
    assert_eq!(app.recent.ids().len(), 1);

    // So after moving once, Tab has somewhere to go back to.
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    let first = app.selected_id().cloned().unwrap();
    app.on_intent(Intent::Down, &mut terminal).unwrap();
    app.on_intent(Intent::JumpRecent, &mut terminal).unwrap();
    assert_eq!(app.selected_id(), Some(&first));
}

#[test]
fn tab_with_nothing_visited_says_so_rather_than_doing_nothing() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    app.recent = crate::recent::Recent::default();

    app.on_intent(Intent::JumpRecent, &mut terminal).unwrap();
    let (_, message, _) = app.message.as_ref().expect("a message");
    assert!(message.contains("No notes visited"), "{message}");
}

/// A deleted note must not linger in the strip as a row that does nothing.
#[test]
fn a_deleted_note_leaves_the_recent_strip() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.remember_visit();
    let id = app.selected_id().cloned().unwrap();
    app.on_intent(Intent::Down, &mut terminal).unwrap();
    assert_eq!(app.tabs().len(), 2);

    app.store.delete_note(&id);
    app.resync();
    assert_eq!(app.tabs().len(), 1, "the deleted note is still listed");
}

/// The strip must not take a row when it is empty.
#[test]
fn the_strip_costs_no_space_until_a_note_is_visited() {
    let (mut app, _d) = temp_app();
    app.recent = crate::recent::Recent::default();

    let with_none = view::layout_with_tabs(Rect::new(0, 0, 80, 20), false, Pane::Notes);
    let with_some = view::layout_with_tabs(Rect::new(0, 0, 80, 20), true, Pane::Notes);
    assert_eq!(with_none.tabs.height, 0);
    assert_eq!(with_some.tabs.height, 1);
    // And the panes get the row back.
    assert!(with_none.dirs.height > with_some.dirs.height);
}

// ── tags ────────────────────────────────────────────────────────────────

/// The left pane has one column and two things to show, so it toggles.
#[test]
fn t_switches_the_left_pane_between_directories_and_tags() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(terminal.backend().to_string().contains("dirs"));

    app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("tags"), "{out}");
    // The fixture tags a note "rust", so the tag and its count are listed.
    assert!(out.contains("#rust"), "{out}");

    app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(terminal.backend().to_string().contains("dirs"));
}

/// Opening a tag narrows the notes pane, through the same filter a search
/// uses — so Esc clears a tag the same way it clears a search.
#[test]
fn opening_a_tag_filters_the_notes_pane() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    let all = app.note_count();

    app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
    app.on_intent(Intent::Open, &mut terminal).unwrap();

    assert_eq!(app.filter.as_deref(), Some("#rust"));
    assert!(app.note_count() < all, "the tag did not narrow anything");
    assert_eq!(app.focus, Pane::Notes, "focus should follow the notes");

    // Every listed note actually carries the tag.
    for id in &app.numbering {
        let note = app.store.find_note(id).unwrap();
        assert!(note.tags.iter().any(|t| t == "rust"), "{:?}", note.tags);
    }
}

#[test]
fn toggling_to_tags_resets_the_selection_so_it_cannot_dangle() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.dir_sel = 5;
    app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
    assert_eq!(app.dir_sel, 0);
    // And drawing with the new listing does not panic.
    terminal.draw(|f| app.draw(f)).unwrap();
}

// ── filtering ───────────────────────────────────────────────────────────

fn press(c: char) -> event::KeyEvent {
    event::KeyEvent::new(event::KeyCode::Char(c), event::KeyModifiers::NONE)
}

fn press_code(code: event::KeyCode) -> event::KeyEvent {
    event::KeyEvent::new(code, event::KeyModifiers::NONE)
}

/// The pane must narrow while typing, not after committing.
#[test]
fn typing_a_filter_narrows_the_pane_on_every_keystroke() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    let all = app.note_count();
    assert!(all >= 2, "fixture needs several notes");

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    assert_eq!(app.note_count(), all, "an empty filter hides nothing");

    // "Rust ownership" is in the fixture; "Graph traversals" is not a match.
    for c in "own".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    assert_eq!(app.note_count(), 1, "filter did not narrow the pane");
    let id = app.selected_id().cloned().unwrap();
    assert!(app.store.find_note(&id).unwrap().title.contains("ownership"));
}

/// The numbers the user types must mean the rows the user sees. If numbering
/// ignored the filter, `:delete 1` would delete something else.
#[test]
fn numbering_follows_the_filter() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    for c in "own".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    assert_eq!(app.numbering.len(), 1);

    let visible = app.store.find_note(&app.numbering[0]).unwrap().title.clone();
    assert!(visible.contains("ownership"), "{visible}");
}

#[test]
fn backspace_widens_the_filter_again() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    let all = app.note_count();

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    for c in "own".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    assert_eq!(app.note_count(), 1);

    for _ in 0..3 {
        app.on_key(press_code(event::KeyCode::Backspace), &mut terminal)
            .unwrap();
    }
    assert_eq!(app.note_count(), all, "backspacing did not restore the list");
}

/// Esc is the only way back to the full list without deleting each character.
#[test]
fn esc_abandons_the_filter() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    let all = app.note_count();

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    for c in "own".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    app.on_key(press_code(event::KeyCode::Esc), &mut terminal).unwrap();

    assert!(app.filter.is_none(), "the filter survived Esc");
    assert_eq!(app.note_count(), all);
    assert!(matches!(app.mode, Mode::Normal));
}

/// Enter keeps the filter but hands the keyboard back, so j/k and D act on
/// what is shown.
#[test]
fn enter_keeps_the_filter_and_returns_to_the_panes() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    for c in "own".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    app.on_key(press_code(event::KeyCode::Enter), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.filter.as_deref(), Some("own"));
    assert_eq!(app.note_count(), 1);
}

/// Committing an empty filter should leave no filter at all, rather than an
/// invisible one that quietly changes the pane title.
#[test]
fn committing_an_empty_filter_clears_it() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    app.on_key(press_code(event::KeyCode::Enter), &mut terminal).unwrap();
    assert!(app.filter.is_none());
}

/// A filter matching nothing must say so, and say how to get out.
#[test]
fn a_filter_that_matches_nothing_says_so() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    for c in "zzzz".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    assert_eq!(app.note_count(), 0);

    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("zzzz"), "the query is not shown: {out}");
    assert!(out.contains("Esc to clear"), "{out}");
}

/// One search, everywhere: `/` finds a note in another directory without
/// having to go there first.
#[test]
fn slash_searches_every_directory() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    assert_eq!(app.current_dir, "");

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    for c in "nested".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    assert_eq!(app.note_count(), 1);
    let id = app.selected_id().cloned().unwrap();
    assert_eq!(app.store.find_note(&id).unwrap().directory, "cs130");

    // And the row says where it lives, since it is not in the directory shown.
    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("cs130/"), "{out}");
}

/// After a search, Esc clears it and leaves you on the note you picked — in its
/// own directory — rather than back where you started.
#[test]
fn esc_after_a_search_lands_on_the_selected_note() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    for c in "nested".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    app.on_key(press_code(event::KeyCode::Enter), &mut terminal).unwrap();
    let picked = app.selected_id().cloned().unwrap();

    app.on_key(press_code(event::KeyCode::Esc), &mut terminal).unwrap();
    assert!(app.filter.is_none(), "Esc did not clear the search");
    assert_eq!(app.current_dir, "cs130");
    assert_eq!(app.selected_id(), Some(&picked));
}

/// A tag opened from the left pane clears the same way.
#[test]
fn esc_clears_a_tag() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
    app.on_intent(Intent::Open, &mut terminal).unwrap();
    assert!(app.filter.is_some());
    app.on_key(press_code(event::KeyCode::Esc), &mut terminal).unwrap();
    assert!(app.filter.is_none());
}

/// Case must not matter, or the filter is a guessing game.
#[test]
fn filtering_ignores_case() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

    app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
    for c in "OWNER".chars() {
        app.on_key(press(c), &mut terminal).unwrap();
    }
    assert_eq!(app.note_count(), 1);
}

// ── mouse ───────────────────────────────────────────────────────────────

/// The geometry the app is actually painting, which depends on whether the
/// tab strip is showing. Computing it any other way in a test is how the
/// off-by-one row bug went unnoticed.
fn frames_for(app: &App, width: u16, height: u16) -> view::Frames {
    view::layout_with_tabs(
        Rect::new(0, 0, width, height),
        !app.tabs().is_empty(),
        app.focus,
    )
}

fn click(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
    }
}

fn wheel_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
    }
}

/// Clicking a pane focuses it, so the keyboard picks up where the mouse left
/// off rather than acting on a different pane than the one just clicked.
#[test]
fn clicking_a_pane_focuses_it() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let frames = frames_for(&app, 100, 20);

    app.on_mouse(click(frames.dirs.x + 2, frames.dirs.y + 1), &mut terminal)
        .unwrap();
    assert_eq!(app.focus, Pane::Dirs);

    app.on_mouse(click(frames.preview.x + 2, frames.preview.y + 1), &mut terminal)
        .unwrap();
    assert_eq!(app.focus, Pane::Preview);

    app.on_mouse(click(frames.notes.x + 2, frames.notes.y + 1), &mut terminal)
        .unwrap();
    assert_eq!(app.focus, Pane::Notes);
}

#[test]
fn clicking_a_note_selects_that_note() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let frames = frames_for(&app, 100, 20);
    assert!(app.note_count() >= 2, "fixture needs two notes");

    // The second row inside the pane is the second note.
    app.on_mouse(click(frames.notes.x + 3, frames.notes.y + 2), &mut terminal)
        .unwrap();
    assert_eq!(app.note_sel, 1);

    // And back to the first.
    app.on_mouse(click(frames.notes.x + 3, frames.notes.y + 1), &mut terminal)
        .unwrap();
    assert_eq!(app.note_sel, 0);
}

/// A click on a border must not move the selection.
#[test]
fn clicking_a_border_leaves_the_selection_alone() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let frames = frames_for(&app, 100, 20);

    app.note_sel = 1;
    app.on_mouse(click(frames.notes.x + 3, frames.notes.y), &mut terminal)
        .unwrap();
    assert_eq!(app.note_sel, 1, "the border moved the selection");
}

/// The wheel acts on what is under the pointer, not on what has focus.
#[test]
fn the_wheel_scrolls_the_pane_under_the_pointer() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let frames = frames_for(&app, 100, 20);

    // Focus is on the notes pane; the pointer is over the preview.
    app.focus = Pane::Notes;
    let before = app.note_sel;
    app.on_mouse(
        wheel_event(MouseEventKind::ScrollDown, frames.preview.x + 2, frames.preview.y + 2),
        &mut terminal,
    )
    .unwrap();
    assert_eq!(app.preview_scroll, 1, "the preview did not scroll");
    assert_eq!(app.note_sel, before, "the wheel moved the wrong pane");

    // Over the notes pane, it moves the selection.
    app.on_mouse(
        wheel_event(MouseEventKind::ScrollDown, frames.notes.x + 2, frames.notes.y + 2),
        &mut terminal,
    )
    .unwrap();
    assert_eq!(app.note_sel, before + 1);
}

#[test]
fn scrolling_up_at_the_top_stays_put() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let frames = frames_for(&app, 100, 20);

    app.on_mouse(
        wheel_event(MouseEventKind::ScrollUp, frames.preview.x + 2, frames.preview.y + 2),
        &mut terminal,
    )
    .unwrap();
    assert_eq!(app.preview_scroll, 0);
}

/// The profile page owns the screen when it is open, so clicks belong to it.
/// Ignoring them made the page look broken to anyone who reached for the
/// mouse.
#[test]
fn clicking_a_row_on_the_profile_page_selects_it() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    app.on_intent(Intent::OpenSettings, &mut terminal).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let before = app.settings.as_ref().unwrap().selected;

    // Aim at the next row that does something, wherever that is.
    let area = Rect::new(0, 0, 100, 30);
    let list = view::settings::list_area(area);
    let target = view::settings::step(&app.settings.as_ref().unwrap().rows, before, 1);
    assert_ne!(target, before, "fixture has only one selectable row");
    app.on_mouse(click(list.x + 4, list.y + target as u16), &mut terminal)
        .unwrap();

    let after = app.settings.as_ref().unwrap().selected;
    assert_ne!(after, before, "the click did not move the selection");
    assert!(
        app.settings.as_ref().unwrap().rows[after].selectable(),
        "the click landed on a row that does nothing"
    );
}

#[test]
fn the_wheel_moves_the_profile_selection() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    app.on_intent(Intent::OpenSettings, &mut terminal).unwrap();
    let before = app.settings.as_ref().unwrap().selected;
    app.on_mouse(wheel_event(MouseEventKind::ScrollDown, 50, 10), &mut terminal)
        .unwrap();
    assert!(app.settings.as_ref().unwrap().selected > before);
}

/// A row of tabs the user cannot click is not really a row of tabs.
#[test]
fn clicking_a_tab_opens_that_note() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();

    // Visit two notes so the strip has two tabs.
    app.remember_visit();
    let first = app.selected_id().cloned().unwrap();
    app.on_intent(Intent::Down, &mut terminal).unwrap();
    let second = app.selected_id().cloned().unwrap();
    assert_eq!(app.tabs().len(), 2);

    terminal.draw(|f| app.draw(f)).unwrap();
    let frames = view::layout_with_tabs(Rect::new(0, 0, 100, 20), true, Pane::Notes);

    // The second tab is the note we came from; click it.
    let tabs = app.tabs();
    let column = {
        let first_label = tabs[0].title.chars().count().min(18) + 2;
        (first_label + 2) as u16
    };
    app.on_mouse(click(column, frames.tabs.y), &mut terminal).unwrap();

    assert_eq!(
        app.selected_id(),
        Some(&first),
        "clicking the second tab did not open that note"
    );
    assert_ne!(app.selected_id(), Some(&second));
}

// ── automatic backup ────────────────────────────────────────────────────

/// Editing must restart the quiet period, or an idle push could fire in the
/// middle of a burst of writing.
#[test]
fn changing_a_note_restarts_the_quiet_period() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();

    // Pretend the notes have been quiet for a while.
    app.last_change = Instant::now() - std::time::Duration::from_secs(600);
    assert!(app.last_change.elapsed() > std::time::Duration::from_secs(300));

    // Any change resets it.
    app.run_action(Action::Undo, &mut terminal).unwrap();
    let id = app.selected_id().cloned().unwrap();
    app.store.toggle_checkbox(&id, 1);
    app.note_changed();
    assert!(app.last_change.elapsed() < std::time::Duration::from_secs(5));
}

/// Nothing waiting must not start a push, or a quiet session runs git every
/// time the loop idles.
#[test]
fn no_push_is_started_when_nothing_is_waiting() {
    let (mut app, _d) = temp_app();
    app.unpushed = Some(0);
    app.last_change = Instant::now() - std::time::Duration::from_secs(600);
    app.maybe_auto_push();
    assert!(app.pushing.is_none());

    // Nor when there is no upstream at all, where a push would only fail.
    app.unpushed = None;
    app.maybe_auto_push();
    assert!(app.pushing.is_none());
}

/// The store this fixture uses has no git repo, so the default policy must
/// leave it alone entirely.
#[test]
fn a_store_without_a_repo_is_never_pushed() {
    let (mut app, _d) = temp_app();
    app.note_changed();
    assert_eq!(app.unpushed, None, "a store with no repo reported a count");
    app.last_change = Instant::now() - std::time::Duration::from_secs(600);
    app.maybe_auto_push();
    assert!(app.pushing.is_none());

    // And quitting does nothing rather than erroring.
    app.push_on_quit();
}

// ── responsive layout ───────────────────────────────────────────────────

/// Narrowing the terminal must not leave focus on a pane that is gone: j and
/// k would move a selection the user cannot see.
#[test]
fn a_resize_moves_focus_off_a_pane_that_disappeared() {
    let (mut app, _d) = temp_app();
    app.focus = Pane::Dirs;

    // Wide enough for three panes: the directories pane is real.
    app.on_resize(120, 30);
    assert_eq!(app.focus, Pane::Dirs);

    // Narrow enough to drop it.
    app.on_resize(70, 24);
    assert_eq!(app.focus, Pane::Notes, "focus stayed on a hidden pane");
}

/// In the one-pane shape every pane is reachable, so focus is never moved out
/// from under the user.
#[test]
fn a_resize_to_one_pane_leaves_focus_alone() {
    let (mut app, _d) = temp_app();
    app.focus = Pane::Preview;
    app.on_resize(40, 20);
    assert_eq!(app.focus, Pane::Preview);
}

/// `h` and `l` must not stop on a pane that is not drawn.
#[test]
fn focus_movement_skips_hidden_panes() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 24)).unwrap();

    // Two-pane shape: notes and preview only.
    app.focus = Pane::Notes;
    app.on_intent(Intent::FocusLeft, &mut terminal).unwrap();
    assert_eq!(app.focus, Pane::Notes, "focus moved onto the hidden dirs pane");

    app.on_intent(Intent::FocusRight, &mut terminal).unwrap();
    assert_eq!(app.focus, Pane::Preview);
}

/// At the narrowest size every pane is still reachable, which is what makes
/// the one-pane shape usable rather than merely small.
#[test]
fn every_pane_is_reachable_on_a_narrow_terminal() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 20)).unwrap();

    app.focus = Pane::Notes;
    app.on_intent(Intent::FocusLeft, &mut terminal).unwrap();
    assert_eq!(app.focus, Pane::Dirs);
    app.on_intent(Intent::FocusRight, &mut terminal).unwrap();
    assert_eq!(app.focus, Pane::Notes);
    app.on_intent(Intent::FocusRight, &mut terminal).unwrap();
    assert_eq!(app.focus, Pane::Preview);

    // And each one draws without panicking at that size.
    for focus in [Pane::Dirs, Pane::Notes, Pane::Preview] {
        app.focus = focus;
        terminal.draw(|f| app.draw(f)).unwrap();
    }
}

/// Every size must draw. Terminals report odd geometry mid-resize.
#[test]
fn the_whole_app_draws_at_any_size() {
    let (mut app, _d) = temp_app();
    for (w, h) in [(200, 60), (120, 30), (90, 24), (70, 20), (50, 16), (40, 10), (20, 6), (10, 3), (4, 2)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();

        // And with every overlay up, since those size themselves too.
        for mode in [Mode::Help, Mode::Settings] {
            app.mode = mode;
            if matches!(app.mode, Mode::Settings) {
                app.on_intent(Intent::OpenSettings, &mut terminal).unwrap();
            }
            terminal.draw(|f| app.draw(f)).unwrap();
        }
        app.mode = Mode::Normal;
    }
}

/// A click behind an overlay would act on something the user cannot see.
#[test]
fn a_click_is_ignored_while_an_overlay_is_open() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let frames = frames_for(&app, 100, 20);

    app.mode = Mode::Help;
    app.focus = Pane::Notes;
    app.on_mouse(click(frames.dirs.x + 2, frames.dirs.y + 1), &mut terminal)
        .unwrap();
    assert_eq!(app.focus, Pane::Notes, "a click reached through the help screen");
}

/// `u` must reverse the key that did the damage, through the same stack the
/// `:` line uses.
#[test]
fn u_takes_back_a_delete_from_the_pane() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 14)).unwrap();

    let before = app.note_count();
    let id = app.selected_id().cloned().expect("a selection");
    let title = app.store.find_note(&id).unwrap().title.clone();

    // Delete without the prompt, the way the confirmed path does.
    app.store.delete_note(&id);
    app.resync();
    assert_eq!(app.note_count(), before - 1);

    app.on_intent(Intent::Undo, &mut terminal).unwrap();
    assert_eq!(app.note_count(), before, "u did not restore the note");
    assert!(app.store.find_note(&id).is_some());

    // And it says what came back, rather than doing it silently.
    let (_, message, _) = app.message.as_ref().expect("a message");
    assert!(message.contains(&title), "{message}");
}

#[test]
fn u_with_nothing_to_undo_says_so() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 14)).unwrap();
    app.on_intent(Intent::Undo, &mut terminal).unwrap();
    let (_, message, _) = app.message.as_ref().expect("a message");
    assert!(message.contains("Nothing to undo"), "{message}");
}

/// An empty pane must explain itself, and the explanation depends on where
/// the user is: "no notes yet" at the root is different advice from "this
/// directory is empty", which also needs the way out.
#[test]
fn an_empty_pane_explains_itself_differently_by_place() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 14)).unwrap();

    // An empty root: the fixture ships notes, so clear them.
    for id in app.numbering.clone() {
        app.store.delete_note(&id);
    }
    app.resync();
    assert_eq!(app.note_count(), 0);
    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("No notes yet"), "{out}");
    assert!(out.contains(":new"), "{out}");

    // Inside a directory, where leaving matters as much as writing. A fresh
    // one, since the fixture's directories have notes in them.
    app.store.create_dir("scratch");
    app.current_dir = "scratch".to_string();
    app.resync();
    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("Nothing in this directory"), "{out}");
    assert!(out.contains("cd .."), "{out}");
}

/// And the preview says so rather than showing an empty box.
#[test]
fn an_empty_preview_says_nothing_is_selected() {
    let (mut app, _d) = temp_app();
    for id in app.numbering.clone() {
        app.store.delete_note(&id);
    }
    app.resync();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 14)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("No note selected"), "{out}");
}

/// A first run must say one thing, and later runs nothing: a greeting the
/// user has to dismiss on every launch is worse than no greeting.
#[test]
fn only_a_first_run_is_greeted() {
    let (mut app, _d) = temp_app();
    app.greet(false);
    assert!(app.message.is_none(), "a later run should say nothing");

    app.greet(true);
    let (_, text, _) = app.message.as_ref().expect("a first run should say something");
    assert!(!text.trim().is_empty());
    assert_eq!(text.lines().count(), 1, "more than one instruction: {text}");
}

/// The user must learn about every gap before speaking, not one per attempt.
#[test]
fn listen_refuses_with_the_fixes_when_nothing_is_set_up() {
    let (mut app, _d) = temp_app();
    // A config with no usable providers at all.
    let lines = {
        let config = leo_services::config::Config {
            chat: leo_services::config::provider::TaskChain { chain: vec![] },
            transcribe: leo_services::config::provider::TaskChain { chain: vec![] },
            providers: Default::default(),
            theme: Default::default(),
            sync: Default::default(),
        };
        let checks =
            leo_services::health::recording(&config, &leo_services::config::secret::MemoryStore::default(), true);
        checks
            .iter()
            .filter(|c| !c.state.is_ready())
            .count()
    };
    assert!(lines >= 2, "expected several gaps, got {lines}");

    // And the App path renders them into the preview rather than a status
    // line, since a one-line status cannot hold install commands.
    app.pinned = Some((
        "not ready to record".to_string(),
        vec![Line::bad("Not ready to record:")],
    ));
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 14)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        terminal.backend().to_string().contains("Not ready to record"),
        "{}",
        terminal.backend().to_string()
    );
}

#[test]
fn foreground_work_renders_a_progress_indicator() {
    let (mut app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 14)).unwrap();

    app.busy = Some((
        view::progress::Progress::spinner("Structuring notes"),
        Instant::now(),
    ));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("Structuring notes"), "{out}");
    assert!(out.contains("00:00"), "no clock: {out}");

    app.busy = Some((
        view::progress::Progress::steps("Transcribing", 3, 8),
        Instant::now(),
    ));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let out = terminal.backend().to_string();
    assert!(out.contains("3/8"), "no step count: {out}");
    assert!(out.contains('█'), "no bar: {out}");
}

#[test]
fn with_nothing_running_the_status_line_has_no_indicator() {
    let (app, _d) = temp_app();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 14)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let out = terminal.backend().to_string();
    assert!(!out.contains('█'), "a bar with no work: {out}");
}

#[test]
fn stepping_saturates_instead_of_wrapping() {
    assert_eq!(step(0, 3, Intent::Up), 0);
    assert_eq!(step(2, 3, Intent::Down), 2);
    assert_eq!(step(1, 3, Intent::Down), 2);
    assert_eq!(step(1, 3, Intent::Up), 0);
    assert_eq!(step(1, 3, Intent::First), 0);
    assert_eq!(step(0, 3, Intent::Last), 2);
}

#[test]
fn stepping_an_empty_list_stays_at_zero() {
    for intent in [Intent::Down, Intent::Up, Intent::First, Intent::Last] {
        assert_eq!(step(0, 0, intent), 0);
    }
}
