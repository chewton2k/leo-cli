use std::cell::Cell;
use std::time::{Duration, Instant};

pub const RESUME_AFTER: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
pub struct LiveScroll {
    top: Cell<Option<usize>>,
    last_input: Cell<Option<Instant>>,
    rows: Cell<usize>,
    height: Cell<usize>,
}

impl LiveScroll {
    pub fn new() -> LiveScroll {
        LiveScroll::default()
    }

    pub fn is_following(&self) -> bool {
        self.top.get().is_none()
    }

    fn bottom(&self) -> usize {
        self.rows.get().saturating_sub(self.height.get())
    }

    pub fn visible_top(&self, rows: usize, height: usize, now: Instant) -> usize {
        self.rows.set(rows);
        self.height.set(height);
        let idle = self
            .last_input
            .get()
            .is_none_or(|at| now.saturating_duration_since(at) >= RESUME_AFTER);
        if idle {
            self.follow();
        }
        match self.top.get() {
            Some(top) if top < self.bottom() => top,
            _ => {
                self.top.set(None);
                self.bottom()
            }
        }
    }

    pub fn scroll_by(&self, rows: isize, now: Instant) {
        let bottom = self.bottom();
        let from = self.top.get().unwrap_or(bottom);
        let to = from.saturating_add_signed(rows).min(bottom);
        self.last_input.set(Some(now));
        self.top.set((to < bottom).then_some(to));
    }

    pub fn to_top(&self, now: Instant) {
        self.last_input.set(Some(now));
        self.top.set((self.bottom() > 0).then_some(0));
    }

    pub fn follow(&self) {
        self.top.set(None);
        self.last_input.set(None);
    }

    pub fn page(&self) -> usize {
        self.height.get().saturating_sub(1).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_new_view_follows_the_newest_rows() {
        let scroll = LiveScroll::new();
        assert!(scroll.is_following());
        assert_eq!(scroll.visible_top(100, 10, Instant::now()), 90);
    }

    #[test]
    fn scrolling_up_stops_following_and_stays_on_the_same_rows_as_text_grows() {
        let scroll = LiveScroll::new();
        let now = Instant::now();
        scroll.visible_top(100, 10, now);
        scroll.scroll_by(-5, now);
        assert!(!scroll.is_following());
        assert_eq!(scroll.visible_top(100, 10, now), 85);
        assert_eq!(scroll.visible_top(140, 10, now), 85);
    }

    #[test]
    fn scrolling_back_down_to_the_end_follows_again() {
        let scroll = LiveScroll::new();
        let now = Instant::now();
        scroll.visible_top(100, 10, now);
        scroll.scroll_by(-5, now);
        scroll.scroll_by(5, now);
        assert!(scroll.is_following());
    }

    #[test]
    fn it_never_scrolls_past_the_first_row() {
        let scroll = LiveScroll::new();
        let now = Instant::now();
        scroll.visible_top(100, 10, now);
        scroll.scroll_by(-1000, now);
        assert_eq!(scroll.visible_top(100, 10, now), 0);
        scroll.scroll_by(-1, now);
        assert_eq!(scroll.visible_top(100, 10, now), 0);
    }

    #[test]
    fn a_transcript_that_fits_never_leaves_following() {
        let scroll = LiveScroll::new();
        let now = Instant::now();
        scroll.visible_top(4, 10, now);
        scroll.scroll_by(-3, now);
        assert!(scroll.is_following());
        assert_eq!(scroll.visible_top(4, 10, now), 0);
    }

    #[test]
    fn after_ten_seconds_without_scrolling_it_follows_again() {
        let scroll = LiveScroll::new();
        let start = Instant::now();
        scroll.visible_top(100, 10, start);
        scroll.scroll_by(-5, start);
        assert_eq!(scroll.visible_top(100, 10, start + secs(9)), 85);
        assert!(!scroll.is_following());
        assert_eq!(scroll.visible_top(100, 10, start + secs(10)), 90);
        assert!(scroll.is_following());
    }

    #[test]
    fn every_scroll_restarts_the_ten_seconds() {
        let scroll = LiveScroll::new();
        let start = Instant::now();
        scroll.visible_top(100, 10, start);
        scroll.scroll_by(-5, start);
        scroll.scroll_by(-1, start + secs(8));
        assert_eq!(scroll.visible_top(100, 10, start + secs(15)), 84);
        assert_eq!(scroll.visible_top(100, 10, start + secs(18)), 90);
    }

    #[test]
    fn home_goes_to_the_top_and_follow_returns_to_the_newest() {
        let scroll = LiveScroll::new();
        let now = Instant::now();
        scroll.visible_top(100, 10, now);
        scroll.to_top(now);
        assert_eq!(scroll.visible_top(100, 10, now), 0);
        scroll.follow();
        assert_eq!(scroll.visible_top(100, 10, now), 90);
    }

    #[test]
    fn a_page_is_one_row_short_of_the_pane() {
        let scroll = LiveScroll::new();
        scroll.visible_top(100, 10, Instant::now());
        assert_eq!(scroll.page(), 9);
        scroll.visible_top(100, 1, Instant::now());
        assert_eq!(scroll.page(), 1);
    }
}
