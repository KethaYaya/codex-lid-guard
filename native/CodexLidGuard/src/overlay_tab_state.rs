//! A tab takes horizontal space only for a shortcut or a new result/question.
use crate::overlay::groups::{GroupSession, ProjectGroup};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub(in super::super) const CALM_WIDTH: i32 = 18;
pub(in super::super) const HINT_WIDTH: i32 = 44;
pub(in super::super) const NOTICE_WIDTH: i32 = 152;
const NOTICE_TIME: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in super::super) enum Kind {
    Calm,
    Hint,
    Finished,
    Question,
}

pub(in super::super) struct TabState {
    seen: HashMap<String, u64>,
    notice: Option<(String, u64, Instant)>,
    pub kind: Kind,
    pub width: i32,
    target: i32,
    from: i32,
    started: Instant,
}

impl TabState {
    pub fn new(group: &ProjectGroup, now: Instant) -> Self {
        let mut state = Self {
            seen: HashMap::new(),
            notice: None,
            kind: Kind::Calm,
            width: CALM_WIDTH,
            target: CALM_WIDTH,
            from: CALM_WIDTH,
            started: now,
        };
        state.observe(group, now);
        state.tick(group, false, false, now);
        state
    }

    pub fn observe(&mut self, group: &ProjectGroup, now: Instant) {
        for session in &group.sessions {
            if Self::unread(session) && self.seen.get(&session.id) != Some(&session.card.id) {
                self.seen.insert(session.id.clone(), session.card.id);
                self.notice = Some((session.id.clone(), session.card.id, now + NOTICE_TIME));
            }
        }
        // Remember results that temporarily disappear while their chat has focus.
        if self.seen.len() > 64 {
            self.seen
                .retain(|id, _| group.sessions.iter().any(|s| &s.id == id));
        }
    }

    fn unread(session: &GroupSession) -> bool {
        session.card.final_message
            && session.card.attention
            && !session.busy
            && !session.needs_input
    }

    pub fn notice_session<'a>(&self, group: &'a ProjectGroup) -> Option<&'a GroupSession> {
        group.sessions.iter().find(|s| s.needs_input).or_else(|| {
            let (id, card, _) = self.notice.as_ref()?;
            group
                .sessions
                .iter()
                .find(|s| &s.id == id && s.card.id == *card && Self::unread(s))
        })
    }

    pub fn tick(
        &mut self,
        group: &ProjectGroup,
        hints: bool,
        animate: bool,
        now: Instant,
    ) -> (bool, bool) {
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, _, deadline)| now >= *deadline)
            || self.notice.as_ref().is_some_and(|(id, card, _)| {
                group
                    .sessions
                    .iter()
                    .any(|s| &s.id == id && (s.card.id != *card || !Self::unread(s)))
            })
        {
            self.notice = None;
        }
        let kind = if group.sessions.iter().any(|s| s.needs_input) {
            Kind::Question
        } else if self.notice.is_some() && self.notice_session(group).is_some() {
            Kind::Finished
        } else if hints {
            Kind::Hint
        } else {
            Kind::Calm
        };
        let target = match kind {
            Kind::Calm => CALM_WIDTH,
            Kind::Hint => HINT_WIDTH,
            _ => NOTICE_WIDTH,
        };
        let old = (self.kind, self.width);
        if target != self.target {
            self.from = self.width;
            self.target = target;
            self.started = now;
        }
        self.kind = kind;
        let progress = if animate {
            (now.saturating_duration_since(self.started).as_secs_f32() / 0.090).min(1.0)
        } else {
            1.0
        };
        self.width = self.target
            + ((self.from - self.target) as f32 * (1.0 - progress).powi(3)).round() as i32;
        (old != (self.kind, self.width), self.width != self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::Card;
    fn group() -> ProjectGroup {
        ProjectGroup {
            key: "test".into(),
            name: "Folder".into(),
            path: None,
            disambiguate: false,
            sessions: vec![GroupSession {
                id: "a".into(),
                activity: 1,
                card: Card {
                    id: 1,
                    label: "Task".into(),
                    text: "Result".into(),
                    final_message: false,
                    attention: false,
                    target: None,
                },
                busy: true,
                needs_input: false,
                hidden_in_focus: false,
                window: None,
                dock_request: 0,
            }],
        }
    }
    #[test]
    fn completion_expires_without_losing_unread_or_retriggering_on_refresh() {
        let now = Instant::now();
        let mut group = group();
        let mut state = TabState::new(&group, now);
        assert_eq!(state.width, 18);
        group.sessions[0].busy = false;
        group.sessions[0].card.final_message = true;
        group.sessions[0].card.attention = true;
        state.observe(&group, now);
        state.tick(&group, false, false, now);
        assert_eq!((state.kind, state.width), (Kind::Finished, 152));
        state.observe(&group, now + Duration::from_secs(7));
        state.tick(&group, false, false, now + NOTICE_TIME);
        assert_eq!((state.kind, state.width), (Kind::Calm, 18));
        assert!(group.sessions[0].card.attention);
        state.observe(&group, now + Duration::from_secs(20));
        state.tick(&group, false, false, now + Duration::from_secs(20));
        assert_eq!(state.kind, Kind::Calm);
        group.sessions[0].card.id = 2;
        state.observe(&group, now + Duration::from_secs(21));
        state.tick(&group, false, false, now + Duration::from_secs(21));
        assert_eq!(state.kind, Kind::Finished);
    }
    #[test]
    fn questions_win_and_prefix_reverses_smoothly_without_changing_read_state() {
        let now = Instant::now();
        let mut group = group();
        let mut state = TabState::new(&group, now);
        state.tick(&group, true, true, now);
        state.tick(&group, true, true, now + Duration::from_millis(40));
        assert!(state.width > 18 && state.width < 44);
        let mid = state.width;
        state.tick(&group, false, true, now + Duration::from_millis(40));
        assert_eq!(state.width, mid);
        state.tick(&group, false, true, now + Duration::from_millis(140));
        assert_eq!(state.width, 18);
        group.sessions[0].needs_input = true;
        state.tick(&group, true, false, now + Duration::from_secs(100));
        assert_eq!((state.kind, state.width), (Kind::Question, 152));
        group.sessions[0].needs_input = false;
        state.tick(&group, true, false, now + Duration::from_secs(101));
        assert_eq!((state.kind, state.width), (Kind::Hint, 44));
        assert!(!group.sessions[0].card.attention);
    }
}
