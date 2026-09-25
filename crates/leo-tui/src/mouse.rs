//! Clicks and the scroll wheel, in the panes and on the profile page.

use super::*;

impl App {
    /// Clicks and the scroll wheel.
    ///
    /// Deliberately limited to selecting and scrolling. A click cannot delete,
    /// edit or open anything: mouse input has no modifier discipline and no
    /// confirmation habit, so the safe half is the useful half. Everything here
    /// has a keyboard equivalent, and nothing here is the only way to do it.
    pub(super) fn on_mouse<B: TuiBackend>(
        &mut self,
        mouse: MouseEvent,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let area = terminal.size().map(|s| Rect::new(0, 0, s.width, s.height))?;

        // The profile page owns the whole screen when it is open, so clicks
        // belong to it. Anything else with an overlay up ignores them: a click
        // behind one would act on something the user cannot see.
        if matches!(self.mode, Mode::Settings) {
            return self.on_settings_mouse(mouse, area);
        }
        if !matches!(self.mode, Mode::Normal) {
            return Ok(());
        }

        // The same geometry that was painted: `layout` alone omits the tab row,
        // so every pane would be one line out whenever the strip is showing.
        let tabs = self.tabs();
        let frames = view::layout_with_tabs(area, !tabs.is_empty(), self.focus);
        let column = mouse.column;
        let row = mouse.row;

        let in_pane = |rect: Rect| {
            column >= rect.x
                && column < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // The tab strip: clicking a tab opens that note, which is what a
                // row of tabs is for.
                if frames.tabs.height > 0 && row == frames.tabs.y {
                    if let Some(index) = view::tabs::tab_at(&tabs, column) {
                        // `tabs` and the recent list are in the same order, and
                        // both skip notes that no longer exist.
                        let ids: Vec<String> = self
                            .recent
                            .ids()
                            .iter()
                            .filter(|id| self.store.find_note(id).is_some())
                            .cloned()
                            .collect();
                        if let Some(target) = ids.get(index).cloned() {
                            self.open_note(&target);
                        }
                    }
                    return Ok(());
                }

                if in_pane(frames.dirs) {
                    self.focus = Pane::Dirs;
                    let rows = self.dir_rows();
                    if let Some(index) =
                        view::notes::row_at(frames.dirs, row, self.dir_sel, rows.len())
                    {
                        self.dir_sel = index;
                    }
                } else if in_pane(frames.notes) {
                    self.focus = Pane::Notes;
                    let total = self.note_count();
                    if let Some(index) =
                        view::notes::row_at(frames.notes, row, self.note_sel, total)
                    {
                        self.note_sel = index;
                        // Clicking a note is opening it, as far as the recent
                        // list is concerned.
                        self.pinned = None;
                    }
                } else if in_pane(frames.preview) {
                    self.focus = Pane::Preview;
                }
                Ok(())
            }
            // The wheel acts on whatever is under the pointer, not on what has
            // focus: that is what every other application does.
            MouseEventKind::ScrollDown => {
                self.wheel(&frames, column, row, Intent::Down);
                Ok(())
            }
            MouseEventKind::ScrollUp => {
                self.wheel(&frames, column, row, Intent::Up);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Clicks and the wheel on the profile page.
    ///
    /// Selection only, like the panes: choosing a row still takes Enter, so a
    /// stray click cannot rewrite a chain or start a git repo.
    pub(super) fn on_settings_mouse(&mut self, mouse: MouseEvent, area: Rect) -> Result<()> {
        let Some(screen) = self.settings.as_mut() else {
            return Ok(());
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let list = view::settings::list_area(area);
                let Some(index) = view::settings::row_at(
                    list,
                    mouse.row,
                    screen.selected,
                    screen.rows.len(),
                ) else {
                    return Ok(());
                };
                // Land on something actionable: clicking a heading should move to
                // the nearest row that does something rather than nothing.
                if screen.rows.get(index).is_some_and(|r| r.selectable()) {
                    screen.selected = index;
                }
                Ok(())
            }
            MouseEventKind::ScrollDown => {
                screen.selected = view::settings::step(&screen.rows, screen.selected, 1);
                Ok(())
            }
            MouseEventKind::ScrollUp => {
                screen.selected = view::settings::step(&screen.rows, screen.selected, -1);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Scroll whatever is under the pointer, which is not necessarily what has
    /// focus — that is what every other application does.
    pub(super) fn wheel(&mut self, frames: &view::Frames, column: u16, row: u16, direction: Intent) {
        let inside = |rect: Rect| {
            column >= rect.x
                && column < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
        };

        if inside(frames.preview) {
            self.preview_scroll = match direction {
                Intent::Down => self.preview_scroll.saturating_add(1),
                _ => self.preview_scroll.saturating_sub(1),
            };
        } else if inside(frames.notes) {
            self.note_sel = step(self.note_sel, self.note_count(), direction);
            self.preview_scroll = 0;
            self.pinned = None;
        } else if inside(frames.dirs) {
            self.dir_sel = step(self.dir_sel, self.dir_rows().len(), direction);
        }
    }
}
