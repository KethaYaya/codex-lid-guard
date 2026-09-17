//! Group display surfaces without changing the identity or lifecycle of a chat.
use super::{Card, Frame, SESSION_LIMIT};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupSession {
    pub id: String,
    pub activity: u64,
    pub card: Card,
    pub busy: bool,
    pub needs_input: bool,
    pub hidden_in_focus: bool,
    pub window: Option<u64>,
    pub dock_request: u64,
}

impl GroupSession {
    pub fn priority(&self) -> u8 {
        if self.needs_input {
            0
        } else if self.busy {
            1
        } else if self.card.final_message && self.card.attention {
            2
        } else {
            3
        }
    }
    pub fn title(&self) -> &str {
        self.card
            .label
            .split_once(" — ")
            .map_or(&self.card.label, |(_, title)| title)
    }

    pub fn status(&self) -> &str {
        if self.needs_input {
            "Needs you"
        } else if self.busy {
            "Working"
        } else if self.card.final_message && self.card.attention {
            "Done · unread"
        } else if self.card.final_message {
            "Done"
        } else {
            "Idle"
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectGroup {
    pub key: String,
    pub name: String,
    pub path: Option<String>,
    pub disambiguate: bool,
    pub sessions: Vec<GroupSession>,
}

impl ProjectGroup {
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.sessions
                .iter()
                .filter(|s| s.busy && !s.needs_input)
                .count(),
            self.sessions.iter().filter(|s| s.needs_input).count(),
            self.sessions
                .iter()
                .filter(|s| s.card.attention && s.card.final_message && !s.needs_input)
                .count(),
        )
    }
}

pub(crate) fn group_frames(frames: Vec<Frame>) -> Vec<Frame> {
    let limit = frames
        .first()
        .map_or(3, |f| f.max_tabs.clamp(1, SESSION_LIMIT));
    let mut groups: Vec<Frame> = vec![];
    for frame in frames {
        let Some(id) = &frame.session_id else {
            continue;
        };
        let Some(card) = frame.cards.first() else {
            continue;
        };
        let path = frame.project_path.clone().or_else(|| {
            card.target
                .as_ref()
                .and_then(|t| t.project.as_ref())
                .map(|p| p.path.clone())
        });
        // Folder display names are not identities: two checkouts named "app"
        // must never share navigation, dismissals, or completion acknowledgement.
        let key = path
            .as_ref()
            .map(|p| format!("project:{}", crate::session_navigation::normalized_path(p)))
            .unwrap_or_else(|| format!("session:{id}"));
        let session = GroupSession {
            id: id.clone(),
            activity: frame.activity,
            card: card.clone(),
            busy: frame.busy,
            needs_input: frame.needs_input,
            hidden_in_focus: frame.hidden_in_focus,
            window: frame.window,
            dock_request: frame.dock_request,
        };
        if let Some(grouped) = groups
            .iter_mut()
            .find(|f| f.session_id.as_ref() == Some(&key))
        {
            let group = grouped.group.as_mut().unwrap();
            if group.sessions.len() < SESSION_LIMIT && !group.sessions.iter().any(|s| s.id == *id) {
                group.sessions.push(session);
                grouped.cards.push(card.clone());
                grouped.busy |= frame.busy;
                grouped.attention |= frame.attention;
                grouped.needs_input |= frame.needs_input;
                grouped.hidden_in_focus &= frame.hidden_in_focus;
                grouped.activity = grouped.activity.max(frame.activity);
            }
        } else {
            let name = path
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_name())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| {
                    card.label
                        .split_once(" — ")
                        .map_or("Codex", |(project, _)| project)
                        .into()
                });
            let mut grouped = frame.clone();
            grouped.session_id = Some(key.clone());
            grouped.group = Some(ProjectGroup {
                key,
                name,
                path,
                disambiguate: false,
                sessions: vec![session],
            });
            groups.push(grouped);
        }
    }
    groups.sort_by_key(|f| std::cmp::Reverse(f.activity));
    groups.truncate(limit);
    let mut names = HashMap::<String, usize>::new();
    let paths: Vec<_> = groups
        .iter()
        .filter_map(|frame| frame.group.as_ref()?.path.clone())
        .collect();
    for group in &groups {
        *names
            .entry(group.group.as_ref().unwrap().name.to_lowercase())
            .or_default() += 1;
    }
    for frame in &mut groups {
        let group = frame.group.as_mut().unwrap();
        if names[&group.name.to_lowercase()] > 1 {
            group.disambiguate = true;
            group.name = group
                .path
                .as_ref()
                .map(|path| distinct_path_label(path, &paths))
                .unwrap_or_else(|| format!("{} · {}", group.name, group.sessions[0].id));
        }
        // Initial order is deterministic, independent of updates or completion.
        group
            .sessions
            .sort_by(|a, b| a.activity.cmp(&b.activity).then(a.id.cmp(&b.id)));
        // Use one stable origin for focus epochs; per-session visibility is
        // resolved separately by the native drawer, including other windows.
        if let Some(anchor) = group.sessions.iter().min_by(|a, b| a.id.cmp(&b.id)) {
            frame.window = anchor.window;
            frame.dock_request = anchor.dock_request;
        }
    }
    groups
}

fn distinct_path_label(path: &str, paths: &[String]) -> String {
    let path = path.replace('\\', "/");
    let parts: Vec<_> = path.trim_end_matches('/').split('/').collect();
    for count in 2..=parts.len() {
        let candidate = parts[parts.len() - count..].join("/");
        let suffix = format!("/{}", candidate.to_lowercase());
        let matches = paths
            .iter()
            .filter(|other| {
                let other = other
                    .replace('\\', "/")
                    .trim_end_matches('/')
                    .to_lowercase();
                other == candidate.to_lowercase() || other.ends_with(&suffix)
            })
            .count();
        if matches == 1 {
            return candidate;
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::CardTarget;

    fn frame(id: &str, path: &str, activity: u64) -> Frame {
        Frame {
            session_id: Some(id.into()),
            project_path: Some(path.into()),
            activity,
            cards: vec![Card {
                id: activity,
                label: format!("app — {id}"),
                text: "update".into(),
                final_message: false,
                attention: false,
                target: Some(CardTarget {
                    window: activity,
                    session_id: id.into(),
                    project: None,
                }),
            }],
            busy: true,
            ..Frame::empty()
        }
    }

    #[test]
    fn groups_paths_case_insensitively_and_keeps_exact_session_targets() {
        let frames = group_frames(vec![
            frame("a", r"C:\Projects\app", 1),
            frame("b", "c:/projects/app/", 2),
        ]);
        assert_eq!(frames.len(), 1);
        let group = frames[0].group.as_ref().unwrap();
        assert_eq!(group.sessions.len(), 2);
        assert_eq!(
            group.sessions[0].card.target.as_ref().unwrap().session_id,
            "a"
        );
        assert_eq!(group.sessions[1].card.target.as_ref().unwrap().window, 2);
    }

    #[test]
    fn same_named_checkouts_stay_separate_and_have_distinct_labels() {
        let frames = group_frames(vec![
            frame("a", r"C:\one\app", 1),
            frame("b", r"C:\two\app", 2),
        ]);
        assert_eq!(frames.len(), 2);
        assert_ne!(frames[0].session_id, frames[1].session_id);
        assert_ne!(
            frames[0].group.as_ref().unwrap().name,
            frames[1].group.as_ref().unwrap().name
        );
        assert!(
            frames
                .iter()
                .all(|f| f.group.as_ref().unwrap().disambiguate)
        );
        assert_eq!(frames[0].group.as_ref().unwrap().name, "two/app");
    }

    #[test]
    fn project_limit_does_not_discard_other_sessions_in_the_project() {
        let frames = (0..5)
            .map(|i| Frame {
                max_tabs: 1,
                ..frame(&i.to_string(), r"C:\app", i)
            })
            .collect();
        let grouped = group_frames(frames);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].group.as_ref().unwrap().sessions.len(), 5);
    }

    #[test]
    fn waiting_is_not_completion_or_working_and_visible_siblings_keep_the_group() {
        let mut waiting = frame("a", r"C:\app", 1);
        waiting.needs_input = true;
        waiting.attention = true;
        waiting.hidden_in_focus = true;
        let mut done = frame("b", r"C:\app", 2);
        done.busy = false;
        done.cards[0].final_message = true;
        done.cards[0].attention = true;
        let grouped = group_frames(vec![waiting, done]);
        assert_eq!(grouped[0].group.as_ref().unwrap().counts(), (0, 1, 1));
        assert!(!grouped[0].hidden_in_focus);
    }
}
