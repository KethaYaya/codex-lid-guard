//! Bounded shortcut state; no text decoding, logging, filesystem access or foreground changes.
use std::time::{Duration, Instant};

pub(super) const COPILOT: u32 = 0x86; // F23, emitted with Win+Shift by the standard Copilot key.
const CHORD_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Binding {
    pub window: usize,
    pub token: usize,
    pub code: [u8; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Expand(Binding),
    Open(Binding),
}

#[derive(Default, Debug)]
pub(super) struct Outcome {
    pub consume: bool,
    pub mask_windows_key: bool,
    pub action: Option<Action>,
}

struct Chord {
    deadline: Instant,
    foreground: usize,
    selected: Option<Binding>,
}

pub(super) struct Keys {
    down: [bool; 256],
    swallowed: [bool; 256],
    chord: Option<Chord>,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            down: [false; 256],
            swallowed: [false; 256],
            chord: None,
        }
    }
}

impl Keys {
    pub(super) fn cancel(&mut self) {
        self.chord = None;
    }

    pub(super) fn seed_modifiers(&mut self, is_down: impl Fn(u32) -> bool) {
        for key in [0x5b, 0x5c, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5] {
            self.down[key as usize] = is_down(key);
        }
    }

    pub(super) fn event(
        &mut self,
        key: u32,
        down: bool,
        now: Instant,
        foreground: usize,
        bindings: &[Option<Binding>; 3],
    ) -> Outcome {
        let mut out = Outcome::default();
        if key >= 256 {
            return out;
        }
        let index = key as usize;
        let repeated = self.down[index];
        self.down[index] = down;
        if !down {
            out.consume = std::mem::take(&mut self.swallowed[index]);
            return out;
        }
        if repeated && self.swallowed[index] {
            out.consume = true;
            return out;
        }
        let win = self.down[0x5b] || self.down[0x5c];
        let shift = self.down[0xa0] || self.down[0xa1] || self.down[0x10];
        let other_modifier = [0xa2, 0xa3, 0xa4, 0xa5, 0x11, 0x12]
            .iter()
            .any(|key| self.down[*key]);
        if key == COPILOT && win && shift && !other_modifier && bindings.iter().any(Option::is_some)
        {
            self.chord = Some(Chord {
                deadline: now + CHORD_TIMEOUT,
                foreground,
                selected: None,
            });
            self.swallowed[index] = true;
            return Outcome {
                consume: true,
                mask_windows_key: true,
                action: None,
            };
        }
        if other_modifier
            || self.chord.as_ref().is_some_and(|chord| {
                (now > chord.deadline && !self.down[COPILOT as usize])
                    || foreground != chord.foreground
                    || chord
                        .selected
                        .is_some_and(|selected| !bindings.contains(&Some(selected)))
            })
        {
            self.cancel();
        }
        let Some(chord) = &mut self.chord else {
            return out;
        };
        // Hardware emits modifier releases separately; they do not cancel a chord.
        if matches!(key, 0x10..=0x12 | 0x5b | 0x5c | 0xa0..=0xa5) {
            return out;
        }
        if key == 0x1b {
            self.cancel();
            self.swallowed[index] = true;
            out.consume = true;
            return out;
        }
        if repeated {
            return out;
        }
        if let Some(selected) = chord.selected {
            if key == selected.code[1] as u32 {
                out.action = Some(Action::Open(selected));
                out.consume = true;
                self.swallowed[index] = true;
            }
            self.cancel();
        } else if let Some(binding) = bindings
            .iter()
            .flatten()
            .find(|binding| key == binding.code[0] as u32)
        {
            chord.selected = Some(*binding);
            chord.deadline = now + CHORD_TIMEOUT;
            self.swallowed[index] = true;
            out.consume = true;
            out.action = Some(Action::Expand(*binding));
        } else {
            self.cancel();
        }
        out
    }
}

pub(super) fn code_for_label(label: &str, occupied: &[u8]) -> [u8; 2] {
    let title = label
        .rsplit_once('\u{2014}')
        .map(|(_, title)| title)
        .unwrap_or(label);
    let letters: Vec<_> = title
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|key| key.to_ascii_uppercase())
        .collect();
    let letters = if letters.is_empty() {
        b"CH".as_slice()
    } else {
        &letters
    };
    let index = letters.iter().position(|key| !occupied.contains(key));
    match index {
        Some(index) => [
            letters[index],
            letters.get(index + 1).copied().unwrap_or(letters[index]),
        ],
        None => [
            (b'A'..=b'Z')
                .find(|key| !occupied.contains(key))
                .unwrap_or(b'1'),
            letters[1.min(letters.len() - 1)],
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bindings() -> [Option<Binding>; 3] {
        [
            Some(Binding {
                window: 1,
                token: 10,
                code: *b"DR",
            }),
            Some(Binding {
                window: 2,
                token: 20,
                code: *b"BU",
            }),
            None,
        ]
    }
    fn arm(keys: &mut Keys, at: Instant, bindings: &[Option<Binding>; 3]) {
        keys.event(0x5b, true, at, 99, bindings);
        keys.event(0xa0, true, at, 99, bindings);
        assert!(keys.event(COPILOT, true, at, 99, bindings).consume);
    }
    #[test]
    fn ordinary_typing_and_unmodified_f23_pass_through() {
        let mut keys = Keys::default();
        let at = Instant::now();
        for key in [b'D' as u32, b'R' as u32, COPILOT] {
            assert!(!keys.event(key, true, at, 99, &bindings()).consume);
            assert!(!keys.event(key, false, at, 99, &bindings()).consume);
        }
    }
    #[test]
    fn copilot_d_expands_and_r_opens_once_with_matched_key_releases() {
        let mut keys = Keys::default();
        let at = Instant::now();
        let bindings = bindings();
        arm(&mut keys, at, &bindings);
        assert_eq!(
            keys.event(b'D' as u32, true, at, 99, &bindings).action,
            Some(Action::Expand(bindings[0].unwrap()))
        );
        assert!(
            keys.event(b'D' as u32, true, at, 99, &bindings)
                .action
                .is_none()
        );
        assert_eq!(
            keys.event(b'R' as u32, true, at, 99, &bindings).action,
            Some(Action::Open(bindings[0].unwrap()))
        );
        assert!(
            keys.event(b'R' as u32, true, at, 99, &bindings)
                .action
                .is_none()
        );
        for key in [b'D' as u32, b'R' as u32, COPILOT] {
            assert!(keys.event(key, false, at, 99, &bindings).consume);
        }
        assert!(!keys.event(b'D' as u32, true, at, 99, &bindings).consume);
    }
    #[test]
    fn copilot_macro_release_and_repeated_letter_codes_work() {
        let mut keys = Keys::default();
        let at = Instant::now();
        let bindings = [
            Some(Binding {
                window: 1,
                token: 1,
                code: *b"DD",
            }),
            None,
            None,
        ];
        arm(&mut keys, at, &bindings);
        for key in [COPILOT, 0xa0, 0x5b] {
            keys.event(key, false, at, 99, &bindings);
        }
        assert!(matches!(
            keys.event(b'D' as u32, true, at, 99, &bindings).action,
            Some(Action::Expand(_))
        ));
        keys.event(b'D' as u32, false, at, 99, &bindings);
        assert!(matches!(
            keys.event(b'D' as u32, true, at, 99, &bindings).action,
            Some(Action::Open(_))
        ));
    }
    #[test]
    fn expiry_escape_focus_changes_and_replaced_sessions_cancel_without_intercepting_typing() {
        for cancel in 0..5 {
            let mut keys = Keys::default();
            let at = Instant::now();
            let mut bindings = bindings();
            arm(&mut keys, at, &bindings);
            keys.event(b'D' as u32, true, at, 99, &bindings);
            let mut now = at;
            let mut foreground = 99;
            match cancel {
                0 => {
                    keys.event(COPILOT, false, at, 99, &bindings);
                    now += Duration::from_secs(2);
                }
                1 => {
                    keys.event(0x1b, true, at, 99, &bindings);
                }
                2 => foreground = 100,
                3 => bindings[0].as_mut().unwrap().token += 1,
                _ => {
                    keys.event(0xa2, true, at, 99, &bindings);
                }
            }
            assert!(
                !keys
                    .event(b'R' as u32, true, now, foreground, &bindings)
                    .consume
            );
            assert!(
                keys.event(b'D' as u32, false, now, foreground, &bindings)
                    .consume
            );
        }
    }
    #[test]
    fn no_visible_overlays_leave_copilot_untouched() {
        let mut keys = Keys::default();
        let at = Instant::now();
        keys.event(0x5b, true, at, 99, &[None; 3]);
        keys.event(0xa0, true, at, 99, &[None; 3]);
        assert!(!keys.event(COPILOT, true, at, 99, &[None; 3]).consume);
    }
    #[test]
    fn labels_resolve_prefix_collisions_and_non_ascii_titles() {
        assert_eq!(code_for_label("Project — Dry run", &[]), *b"DR");
        assert_eq!(code_for_label("Project — Deploy", b"D"), *b"EP");
        assert_eq!(code_for_label("Project — DDD", b"D"), *b"AD");
        assert_eq!(code_for_label("Project — 文", &[]), *b"CH");
        assert_eq!(code_for_label("D", &[]), *b"DD");
    }
}
