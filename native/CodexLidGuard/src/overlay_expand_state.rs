//! Timed progression from the compact drawer to a readable reply and full chat.
use std::time::{Duration, Instant};

const DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Stage { #[default] Compact, Message, Full }

#[derive(Default)]
pub(super) struct Progression {
    pub stage: Stage,
    opened: Option<Instant>,
    typing: Option<Instant>,
}

impl Progression {
    pub fn open(&mut self, now: Instant) { self.opened.get_or_insert(now); }
    pub fn edit(&mut self, now: Instant, has_text: bool) {
        if has_text { self.typing.get_or_insert(now); } else { self.typing = None; }
    }
    pub fn clear_typing(&mut self) { self.typing = None; }
    pub fn timing(&self) -> bool { self.stage == Stage::Compact || (self.stage == Stage::Message && self.typing.is_some()) }
    pub fn next(&self, now: Instant, overflow: bool) -> Option<Stage> {
        match self.stage {
            Stage::Compact if self.typing.is_some() || self.opened.is_some_and(|at| now.saturating_duration_since(at) >= DELAY) => Some(Stage::Message),
            Stage::Message if self.typing.is_some_and(|at| overflow || now.saturating_duration_since(at) >= DELAY) => Some(Stage::Full),
            _ => None,
        }
    }
    pub fn reset(&mut self) { *self = Self::default(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_and_typing_have_independent_five_second_deadlines() {
        let now = Instant::now();
        let mut state = Progression::default(); state.open(now);
        state.open(now + Duration::from_secs(4));
        assert_eq!(state.next(now + Duration::from_millis(4999), false), None);
        assert_eq!(state.next(now + DELAY, false), Some(Stage::Message));
        state.stage = Stage::Message;
        assert_eq!(state.next(now + Duration::from_secs(60), true), None);
        state.edit(now + Duration::from_secs(60), true);
        state.edit(now + Duration::from_secs(64), true);
        assert_eq!(state.next(now + Duration::from_millis(64999), false), None);
        assert_eq!(state.next(now + Duration::from_secs(65), false), Some(Stage::Full));
        state.clear_typing();
        assert_eq!(state.next(now + Duration::from_secs(66), true), None);
    }
    #[test]
    fn typing_first_shows_latest_message_then_overflow_maximizes_and_fold_resets() {
        let now = Instant::now();
        let mut state = Progression::default(); state.open(now); state.edit(now, true);
        assert_eq!(state.next(now, true), Some(Stage::Message));
        state.stage = Stage::Message;
        assert_eq!(state.next(now, true), Some(Stage::Full));
        state.edit(now, false);
        assert_eq!(state.next(now + DELAY, true), None);
        state.reset(); state.open(now + DELAY);
        assert_eq!(state.next(now + DELAY, false), None);
    }
}
