//! When leo pushes your notes without being asked.
//!
//! Committing already happens on every save. Pushing is different: it is a
//! network round trip that can fail, can be rejected because the remote moved on,
//! and is pointless fifty times during one editing session. So the policy lives
//! here as data and a pure decision, and the effects live in the worker.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How often leo may push, at most, when watching for a quiet moment.
///
/// A floor rather than the interval itself: the trigger is "nothing has changed
/// for a while", and this stops a stream of small edits from producing a push
/// every few seconds.
pub const MIN_PUSH_GAP: Duration = Duration::from_secs(20);

/// Default quiet period before an idle push.
///
/// Long enough that pausing to think does not trigger a push, short enough that
/// closing the laptop shortly after writing does not lose the backup.
const DEFAULT_IDLE_SECS: u64 = 45;

/// When to push.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoPush {
    /// Only when asked, with `:sync push`.
    Off,
    /// Once, on the way out. No background machinery, nothing to explain, and it
    /// batches a whole session into one push.
    #[default]
    OnQuit,
    /// When the notes have been quiet for [`SyncConfig::idle`], and again on quit.
    /// For anyone working across two machines.
    WhenIdle,
}

impl AutoPush {
    /// For the profile page: the next setting, cycling.
    pub fn next(self) -> Self {
        match self {
            AutoPush::Off => AutoPush::OnQuit,
            AutoPush::OnQuit => AutoPush::WhenIdle,
            AutoPush::WhenIdle => AutoPush::Off,
        }
    }

    /// How to describe it on one line.
    pub fn label(self) -> &'static str {
        match self {
            AutoPush::Off => "off — push by hand",
            AutoPush::OnQuit => "on quit",
            AutoPush::WhenIdle => "when idle, and on quit",
        }
    }

    /// The value written to `config.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            AutoPush::Off => "off",
            AutoPush::OnQuit => "on_quit",
            AutoPush::WhenIdle => "when_idle",
        }
    }

    fn wants_idle_push(self) -> bool {
        matches!(self, AutoPush::WhenIdle)
    }

    fn wants_quit_push(self) -> bool {
        matches!(self, AutoPush::OnQuit | AutoPush::WhenIdle)
    }
}

/// The `[sync]` table in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncConfig {
    #[serde(default)]
    pub auto_push: AutoPush,
    /// Quiet period before an idle push, in seconds.
    #[serde(default = "default_idle_secs")]
    pub idle_secs: u64,
}

fn default_idle_secs() -> u64 {
    DEFAULT_IDLE_SECS
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            auto_push: AutoPush::default(),
            idle_secs: DEFAULT_IDLE_SECS,
        }
    }
}

impl SyncConfig {
    /// The quiet period, with a floor so a misconfigured `idle_secs = 0` cannot
    /// turn every keystroke into a push.
    pub fn idle(&self) -> Duration {
        Duration::from_secs(self.idle_secs).max(MIN_PUSH_GAP)
    }
}

/// Everything the decision depends on, gathered by the caller.
///
/// A struct rather than five arguments so a new condition cannot be added at a
/// call site and forgotten at another.
#[derive(Debug, Clone, Copy)]
pub struct PushWhen {
    /// Commits waiting to go. `None` when there is no upstream, which is not the
    /// same as zero: pushing would fail.
    pub unpushed: Option<usize>,
    /// How long since the notes last changed.
    pub quiet_for: Duration,
    /// How long since leo last pushed, if it has.
    pub since_last_push: Option<Duration>,
    /// Whether a push is already running.
    pub in_flight: bool,
}

/// Whether to push now, in the background.
///
/// Pure, because "why did it push then?" and "why did it not push?" are questions
/// that should be answerable without a terminal and a stopwatch.
pub fn should_push_now(config: &SyncConfig, when: PushWhen) -> bool {
    if !config.auto_push.wants_idle_push() || when.in_flight {
        return false;
    }
    // Nothing to send, or nowhere to send it.
    if !matches!(when.unpushed, Some(n) if n > 0) {
        return false;
    }
    if when.quiet_for < config.idle() {
        return false;
    }
    // Never faster than the floor, however the quiet period is configured.
    match when.since_last_push {
        Some(elapsed) => elapsed >= MIN_PUSH_GAP,
        None => true,
    }
}

/// Whether to push on the way out.
pub fn should_push_on_quit(config: &SyncConfig, unpushed: Option<usize>) -> bool {
    config.auto_push.wants_quit_push() && matches!(unpushed, Some(n) if n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn when(unpushed: Option<usize>, quiet: u64) -> PushWhen {
        PushWhen {
            unpushed,
            quiet_for: Duration::from_secs(quiet),
            since_last_push: None,
            in_flight: false,
        }
    }

    fn idle_config() -> SyncConfig {
        SyncConfig {
            auto_push: AutoPush::WhenIdle,
            idle_secs: 45,
        }
    }

    #[test]
    fn the_default_is_to_push_once_on_the_way_out() {
        let config = SyncConfig::default();
        assert_eq!(config.auto_push, AutoPush::OnQuit);
        // Which means no background pushing at all until asked for.
        assert!(!should_push_now(&config, when(Some(3), 9999)));
        assert!(should_push_on_quit(&config, Some(3)));
    }

    #[test]
    fn off_means_off_in_both_directions() {
        let config = SyncConfig {
            auto_push: AutoPush::Off,
            idle_secs: 1,
        };
        assert!(!should_push_now(&config, when(Some(5), 9999)));
        assert!(!should_push_on_quit(&config, Some(5)));
    }

    #[test]
    fn an_idle_push_waits_for_the_notes_to_go_quiet() {
        let config = idle_config();
        assert!(!should_push_now(&config, when(Some(1), 0)));
        assert!(!should_push_now(&config, when(Some(1), 44)));
        assert!(should_push_now(&config, when(Some(1), 45)));
        assert!(should_push_now(&config, when(Some(1), 600)));
    }

    /// Nothing to push is the common case, and it must not produce a git call
    /// every time the loop goes quiet.
    #[test]
    fn nothing_waiting_means_no_push() {
        let config = idle_config();
        assert!(!should_push_now(&config, when(Some(0), 9999)));
    }

    /// No upstream is not the same as nothing to push: a push would fail, and
    /// failing every forty-five seconds is worse than not trying.
    #[test]
    fn no_upstream_means_no_push() {
        let config = idle_config();
        assert!(!should_push_now(&config, when(None, 9999)));
        assert!(!should_push_on_quit(&config, None));
    }

    #[test]
    fn a_push_already_running_is_not_started_again() {
        let config = idle_config();
        let mut w = when(Some(2), 9999);
        w.in_flight = true;
        assert!(!should_push_now(&config, w));
    }

    /// The floor holds however the quiet period is configured, so a stream of
    /// small edits cannot produce a push every few seconds.
    #[test]
    fn pushes_are_never_closer_together_than_the_floor() {
        let config = idle_config();
        let mut w = when(Some(1), 9999);

        w.since_last_push = Some(Duration::from_secs(5));
        assert!(!should_push_now(&config, w));

        w.since_last_push = Some(MIN_PUSH_GAP);
        assert!(should_push_now(&config, w));
    }

    #[test]
    fn a_zero_idle_period_is_clamped_rather_than_obeyed() {
        let config = SyncConfig {
            auto_push: AutoPush::WhenIdle,
            idle_secs: 0,
        };
        assert_eq!(config.idle(), MIN_PUSH_GAP);
        assert!(!should_push_now(&config, when(Some(1), 1)));
        assert!(should_push_now(&config, when(Some(1), MIN_PUSH_GAP.as_secs())));
    }

    #[test]
    fn when_idle_also_pushes_on_quit() {
        assert!(should_push_on_quit(&idle_config(), Some(1)));
    }

    #[test]
    fn the_setting_cycles_through_every_value() {
        let mut seen = vec![AutoPush::Off];
        let mut current = AutoPush::Off;
        for _ in 0..3 {
            current = current.next();
            seen.push(current);
        }
        assert_eq!(
            seen,
            [
                AutoPush::Off,
                AutoPush::OnQuit,
                AutoPush::WhenIdle,
                AutoPush::Off
            ],
            "cycling does not return to where it started"
        );
    }

    #[test]
    fn every_setting_describes_itself_and_has_a_config_value() {
        for mode in [AutoPush::Off, AutoPush::OnQuit, AutoPush::WhenIdle] {
            assert!(!mode.label().is_empty(), "{mode:?}");
            assert!(!mode.as_str().contains(' '), "{mode:?} is not a TOML value");
        }
    }

    /// The table has to survive a round trip, since the profile page writes it.
    #[test]
    fn the_table_round_trips_through_toml() {
        for mode in [AutoPush::Off, AutoPush::OnQuit, AutoPush::WhenIdle] {
            let config = SyncConfig {
                auto_push: mode,
                idle_secs: 90,
            };
            let text = toml::to_string(&config).unwrap();
            assert_eq!(toml::from_str::<SyncConfig>(&text).unwrap(), config);
            // And the written form matches what the profile page writes.
            assert!(text.contains(mode.as_str()), "{text}");
        }
    }

    #[test]
    fn a_missing_table_is_the_default_and_a_partial_one_fills_in() {
        assert_eq!(
            toml::from_str::<SyncConfig>("").unwrap(),
            SyncConfig::default()
        );
        let partial: SyncConfig = toml::from_str("auto_push = \"when_idle\"").unwrap();
        assert_eq!(partial.auto_push, AutoPush::WhenIdle);
        assert_eq!(partial.idle_secs, DEFAULT_IDLE_SECS);
    }

    #[test]
    fn an_unknown_setting_is_an_error_rather_than_a_silent_default() {
        // A typo should be reported, not quietly turned off.
        assert!(toml::from_str::<SyncConfig>("auto_push = \"sometimes\"").is_err());
    }
}
