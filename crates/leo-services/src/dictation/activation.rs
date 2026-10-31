use super::settings::Mode;
use std::time::{Duration, Instant};
const HOLD: Duration = Duration::from_millis(300);
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Start,
    Stop,
}
#[derive(Default)]
pub struct Activation {
    down: Option<Instant>,
    active: bool,
    locked: bool,
}
impl Activation {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn input(&mut self, pressed: bool, now: Instant, mode: Mode) -> Action {
        if pressed {
            if self.down.is_some() {
                return Action::None;
            }
            self.down = Some(now);
            if self.active && (self.locked || mode == Mode::Tap) {
                self.active = false;
                self.locked = false;
                return Action::Stop;
            }
            if !self.active {
                self.active = true;
                self.locked = mode == Mode::Tap;
                return Action::Start;
            }
        } else if let Some(start) = self.down.take() {
            if self.active && mode != Mode::Tap && !self.locked {
                if mode == Mode::Both && now.saturating_duration_since(start) < HOLD {
                    self.locked = true;
                } else {
                    self.active = false;
                    return Action::Stop;
                }
            }
        }
        Action::None
    }
}
