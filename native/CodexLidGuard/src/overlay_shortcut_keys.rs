//! Bounded shortcut state; no text decoding, logging, filesystem access or foreground changes.
use std::time::{Duration, Instant};

use crate::shortcut_config::{ShortcutConfig, CTRL, ALT, SHIFT, WIN};
#[cfg(test)]
use crate::shortcut_config::COPILOT;
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
    Close(Binding),
    Collapse(Binding),
    HoldPreview(Binding),
    ReleasePreview(Binding),
    Cycle {
        previous: Option<Binding>,
        selected: Binding,
        held: bool,
    },
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
    cycle: Option<(usize, Binding)>,
    preview: Option<Binding>,
    tab_target: Option<(usize, Binding)>,
    expanded: [Option<Binding>; crate::overlay::SESSION_LIMIT],
    config: ShortcutConfig,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            down: [false; 256],
            swallowed: [false; 256],
            chord: None,
            cycle: None,
            preview: None,
            tab_target: None,
            expanded: [None; crate::overlay::SESSION_LIMIT],
            config: ShortcutConfig::default(),
        }
    }
}

impl Keys {
    pub(super) fn set_expanded(&mut self, expanded: [Option<Binding>; crate::overlay::SESSION_LIMIT]) {
        if self.tab_target.is_some_and(|(_, binding)|
            self.expanded.contains(&Some(binding)) && !expanded.contains(&Some(binding))) {
            self.tab_target = None;
        }
        self.expanded = expanded;
    }

    pub(super) fn configure(&mut self, config: ShortcutConfig) -> Option<Action> {
        if self.config != config {
            self.cancel();
            self.cycle = None;
            self.tab_target = None;
            self.config = config;
            return self.preview.take().map(Action::ReleasePreview);
        }
        None
    }

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
        bindings: &[Option<Binding>],
    ) -> Outcome {
        let mut out = Outcome::default();
        if key >= 256 {
            return out;
        }
        let index = key as usize;
        let trigger_was_down = self.down[self.config.trigger as usize];
        let repeated = self.down[index];
        self.down[index] = down;
        if !down {
            out.consume = std::mem::take(&mut self.swallowed[index]);
            if key == self.config.trigger && repeated {
                out.action = self.preview
                    .filter(|binding| bindings.contains(&Some(*binding)))
                    .map(Action::ReleasePreview);
            }
            return out;
        }
        if repeated && self.swallowed[index] {
            out.consume = true;
            return out;
        }
        if !self.config.enabled { return out; }
        let modifiers = [(WIN, [0x5b, 0x5c, 0x5b]), (SHIFT, [0xa0, 0xa1, 0x10]),
            (CTRL, [0xa2, 0xa3, 0x11]), (ALT, [0xa4, 0xa5, 0x12])]
            .iter().fold(0, |flags, (flag, keys)| flags | if keys.iter().any(|key| self.down[*key]) { *flag } else { 0 });
        self.tab_target = self.tab_target.filter(|(window, binding)|
            *window == foreground && bindings.contains(&Some(*binding)));
        let active_chord = self.chord.as_ref().filter(|chord|
            chord.foreground == foreground && (now <= chord.deadline || trigger_was_down));
        // A freshly pressed prefix still needs its first cycle/letter step,
        // including keyboards that release the entire Copilot macro immediately.
        let prefix_step = active_chord.is_some_and(|chord| chord.selected.is_none()
            || key == self.config.open || key == self.config.close);
        if key == 0x09 && !trigger_was_down && modifiers != 0 && modifiers != self.config.modifiers {
            self.tab_target = None;
            self.cancel();
            return out;
        }
        if key == 0x09 && modifiers == 0 && !trigger_was_down && !prefix_step && !repeated {
            let target = self.tab_target.take();
            self.preview = None;
            self.cancel();
            if let Some((_, binding)) = target && self.expanded.contains(&Some(binding)) {
                self.swallowed[index] = true;
                return Outcome { consume: true, action: Some(Action::Collapse(binding)), ..Outcome::default() };
            }
            return out;
        }
        let other_modifier = modifiers & !self.config.modifiers != 0;
        if other_modifier || self.chord.as_ref().is_some_and(|chord| {
            (now > chord.deadline && !trigger_was_down)
                || foreground != chord.foreground
                || (key != self.config.cycle && chord.selected.is_some_and(|selected| !bindings.contains(&Some(selected))))
        }) {
            self.cancel();
        }
        let is_step = [self.config.cycle, self.config.open, self.config.close].contains(&key)
            || self.chord.as_ref().is_some_and(|chord| chord.selected.map_or_else(
                || bindings.iter().flatten().any(|binding| binding.code[0] as u32 == key),
                |selected| selected.code[1] as u32 == key));
        if key == self.config.trigger && modifiers == self.config.modifiers
            && bindings.iter().any(Option::is_some)
            && (self.chord.is_none() || !is_step)
        {
            self.chord = Some(Chord {
                deadline: now + CHORD_TIMEOUT,
                foreground,
                selected: None,
            });
            self.swallowed[index] = true;
            return Outcome {
                consume: true,
                mask_windows_key: modifiers & (WIN | ALT) != 0,
                action: self.preview
                    .filter(|binding| bindings.contains(&Some(*binding)))
                    .map(Action::HoldPreview),
            };
        }
        let Some(chord) = &mut self.chord else {
            self.tab_target = None;
            return out;
        };
        // Hardware emits modifier releases separately; they do not cancel a chord.
        if matches!(key, 0x10..=0x12 | 0x5b | 0x5c | 0xa0..=0xa5) {
            return out;
        }
        if key == self.config.close {
            out.action = chord.selected.map(Action::Close);
            self.tab_target = None;
            self.cancel();
            self.swallowed[index] = true;
            out.consume = true;
            return out;
        }
        if repeated {
            return out;
        }
        if key == self.config.cycle {
            // Keep the last cycle selection across hardware that sends the
            // Copilot macro as a tap. Every Tab press visits one live lane.
            let previous = chord
                .selected
                .or_else(|| {
                    self.cycle
                        .filter(|(window, _)| *window == foreground)
                        .map(|(_, binding)| binding)
                })
                .filter(|binding| bindings.contains(&Some(*binding)));
            let start = previous
                .and_then(|binding| bindings.iter().position(|entry| *entry == Some(binding)))
                .map_or(0, |index| (index + 1) % bindings.len());
            let Some(selected) =
                (0..bindings.len()).find_map(|offset| bindings[(start + offset) % bindings.len()])
            else {
                self.cancel();
                return out;
            };
            chord.selected = Some(selected);
            chord.deadline = now + CHORD_TIMEOUT;
            self.cycle = Some((foreground, selected));
            self.swallowed[index] = true;
            out.consume = true;
            self.preview = Some(selected);
            self.tab_target = Some((foreground, selected));
            out.action = Some(Action::Cycle { previous, selected, held: trigger_was_down });
            return out;
        }
        if let Some(selected) = chord.selected {
            self.tab_target = None;
            if key == selected.code[1] as u32 || key == self.config.open {
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
            self.cycle = Some((foreground, *binding));
            self.tab_target = Some((foreground, *binding));
            chord.deadline = now + CHORD_TIMEOUT;
            self.swallowed[index] = true;
            out.consume = true;
            out.action = Some(Action::Expand(*binding));
        } else {
            self.tab_target = None;
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
    use crate::shortcut_config::ShortcutSettings;

    #[test]
    fn custom_keys_cycle_all_ten_tabs_and_open_or_close_only_the_selection() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let _ = keys.configure(ShortcutConfig::parse(&ShortcutSettings {
            prefix: "Ctrl+Alt+Space".into(), cycle_key: "Down".into(),
            open_key: "Right".into(), close_key: "Delete".into(), ..Default::default()
        }).unwrap());
        let bindings: Vec<_> = (0..10).map(|slot| Some(Binding { window: slot + 1,
            token: slot + 20, code: [b'A' + slot as u8, b'Z'] })).collect();
        for key in [0x09, 0x28, 0x27, 0x2e, b'A' as u32] {
            assert!(!keys.event(key, true, at, 99, &bindings).consume);
            keys.event(key, false, at, 99, &bindings);
        }
        for key in [0xa2, 0xa4, 0x20] { keys.event(key, true, at, 99, &bindings); }
        for key in [0x20, 0xa4, 0xa2] { keys.event(key, false, at, 99, &bindings); }
        for slot in (0..10).chain([0]) {
            assert!(matches!(keys.event(0x28, true, at, 99, &bindings).action,
                Some(Action::Cycle { selected, .. }) if selected == bindings[slot].unwrap()));
            assert!(keys.event(0x28, false, at, 99, &bindings).consume);
        }
        assert_eq!(keys.event(0x27, true, at, 99, &bindings).action, Some(Action::Open(bindings[0].unwrap())));
        keys.event(0x27, false, at, 99, &bindings);
        for key in [0xa2, 0xa4, 0x20] { keys.event(key, true, at, 99, &bindings); }
        keys.event(b'J' as u32, true, at, 99, &bindings);
        assert_eq!(keys.event(0x2e, true, at, 99, &bindings).action, Some(Action::Close(bindings[9].unwrap())));
        assert!(keys.event(0x2e, false, at, 99, &bindings).consume);
    }

    #[test]
    fn rebinding_or_disabling_cancels_pending_actions_but_balances_swallowed_releases() {
        for disabled in [false, true] {
            let at = Instant::now();
            let mut keys = Keys::default();
            arm(&mut keys, at, &bindings());
            keys.event(b'D' as u32, true, at, 99, &bindings());
            let _ = keys.configure(ShortcutConfig::from_settings(&ShortcutSettings {
                enabled: !disabled, prefix: "Ctrl+Alt+Space".into(), ..Default::default()
            }));
            assert!(!keys.event(0x0d, true, at, 99, &bindings()).consume);
            assert!(keys.event(b'D' as u32, false, at, 99, &bindings()).consume);
            assert!(keys.event(COPILOT, false, at, 99, &bindings()).consume);
            assert!(!keys.event(COPILOT, true, at, 99, &bindings()).consume);
        }
    }

    #[test]
    fn a_prefix_letter_can_also_be_used_in_a_tab_code() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let _ = keys.configure(ShortcutConfig::from_settings(&ShortcutSettings { prefix: "Ctrl+Alt+D".into(), ..Default::default() }));
        for key in [0xa2, 0xa4, b'D' as u32] { keys.event(key, true, at, 99, &bindings()); }
        keys.event(b'D' as u32, false, at, 99, &bindings());
        assert_eq!(keys.event(b'D' as u32, true, at, 99, &bindings()).action, Some(Action::Expand(bindings()[0].unwrap())));
        assert_eq!(keys.event(b'R' as u32, true, at, 99, &bindings()).action, Some(Action::Open(bindings()[0].unwrap())));
    }

    #[test]
    fn pressing_a_letter_prefix_again_after_expiry_cannot_open_the_old_selection() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let _ = keys.configure(ShortcutConfig::from_settings(&ShortcutSettings { prefix: "Ctrl+Alt+D".into(), ..Default::default() }));
        for key in [0xa2, 0xa4, b'D' as u32] { keys.event(key, true, at, 99, &bindings()); }
        keys.event(b'D' as u32, false, at, 99, &bindings());
        keys.event(b'D' as u32, true, at, 99, &bindings());
        keys.event(b'D' as u32, false, at, 99, &bindings());
        let later = at + Duration::from_secs(2);
        let prefix = keys.event(b'D' as u32, true, later, 99, &bindings());
        assert!(prefix.consume);
        assert_eq!(prefix.action, None);
        assert!(!keys.event(b'R' as u32, true, later, 99, &bindings()).consume);
    }
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
    fn cycle_preview_follows_prefix_release_even_after_shortcut_cancellation() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let bindings = bindings();
        arm(&mut keys, at, &bindings);
        assert!(matches!(keys.event(0x09, true, at, 99, &bindings).action,
            Some(Action::Cycle { held: true, .. })));
        assert_eq!(keys.event(0x09, false, at, 99, &bindings).action, None);
        // Typing cancels chat activation, but releasing Copilot still folds its preview.
        assert!(!keys.event(b'X' as u32, true, at, 99, &bindings).consume);
        assert_eq!(keys.event(COPILOT, false, at + Duration::from_secs(5), 99, &bindings).action,
            Some(Action::ReleasePreview(bindings[0].unwrap())));
        assert_eq!(keys.event(COPILOT, false, at, 99, &bindings).action, None);
        assert_eq!(keys.event(COPILOT, true, at, 99, &bindings).action,
            Some(Action::HoldPreview(bindings[0].unwrap())));
        assert_eq!(keys.event(COPILOT, false, at, 99, &bindings).action,
            Some(Action::ReleasePreview(bindings[0].unwrap())));
    }

    #[test]
    fn tapped_prefix_starts_cycle_countdown_and_stale_bindings_ignore_release() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let mut bindings = bindings();
        arm(&mut keys, at, &bindings);
        for key in [COPILOT, 0xa0, 0x5b] {
            assert_eq!(keys.event(key, false, at, 99, &bindings).action, None);
        }
        assert!(matches!(keys.event(0x09, true, at, 99, &bindings).action,
            Some(Action::Cycle { held: false, .. })));
        keys.event(0x09, false, at, 99, &bindings);
        arm(&mut keys, at, &bindings);
        bindings[0].as_mut().unwrap().token += 1;
        assert_eq!(keys.event(COPILOT, false, at, 99, &bindings).action, None);
    }

    #[test]
    fn changing_shortcuts_releases_a_held_cycle_preview() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let bindings = bindings();
        arm(&mut keys, at, &bindings);
        keys.event(0x09, true, at, 99, &bindings);
        let config = ShortcutConfig::parse(&ShortcutSettings {
            prefix: "Ctrl+Alt+Space".into(), ..Default::default()
        }).unwrap();
        assert_eq!(keys.configure(config), Some(Action::ReleasePreview(bindings[0].unwrap())));
    }

    #[test]
    fn plain_tab_folds_the_expanded_selection_after_releasing_copilot() {
        for tapped in [false, true] {
            for delay in [Duration::from_millis(100), Duration::from_secs(2)] {
                let at = Instant::now();
                let mut keys = Keys::default();
                let bindings = bindings();
                arm(&mut keys, at, &bindings);
                if tapped {
                    for key in [COPILOT, 0xa0, 0x5b] { keys.event(key, false, at, 99, &bindings); }
                }
                keys.event(0x09, true, at, 99, &bindings);
                keys.event(0x09, false, at, 99, &bindings);
                if !tapped {
                    for key in [COPILOT, 0xa0, 0x5b] { keys.event(key, false, at, 99, &bindings); }
                }
                keys.set_expanded(std::array::from_fn(|slot| if slot == 0 { bindings[0] } else { None }));
                let folded = keys.event(0x09, true, at + delay, 99, &bindings);
                assert!(folded.consume);
                assert_eq!(folded.action, Some(Action::Collapse(bindings[0].unwrap())));
                assert_eq!(keys.event(0x09, true, at + delay, 99, &bindings).action, None);
                assert!(keys.event(0x09, false, at + delay, 99, &bindings).consume);
                assert!(!keys.event(0x09, true, at + delay, 99, &bindings).consume,
                    "later Tab presses must pass through even before the UI reports its fold");
                keys.event(0x09, false, at + delay, 99, &bindings);
                arm(&mut keys, at + delay, &bindings);
                assert!(matches!(keys.event(0x09, true, at + delay, 99, &bindings).action,
                    Some(Action::Cycle { selected, held: true, .. }) if selected == bindings[1].unwrap()));
            }
        }
    }

    #[test]
    fn plain_tab_leaves_other_apps_alone_without_an_expanded_keyboard_selection() {
        for case in 0..7 {
            let at = Instant::now();
            let mut keys = Keys::default();
            let mut bindings = bindings();
            arm(&mut keys, at, &bindings);
            keys.event(0x09, true, at, 99, &bindings);
            keys.event(0x09, false, at, 99, &bindings);
            for key in [COPILOT, 0xa0, 0x5b] { keys.event(key, false, at, 99, &bindings); }
            keys.set_expanded(std::array::from_fn(|slot| if slot == 0 { bindings[0] } else { None }));
            match case {
                0 => keys.set_expanded([None; crate::overlay::SESSION_LIMIT]),
                1 => { keys.event(b'X' as u32, true, at, 99, &bindings); }
                2 => { bindings[0].as_mut().unwrap().token += 1; }
                3 => { keys.event(0xa0, true, at, 99, &bindings); } // Shift+Tab.
                4 => { keys.event(0xa2, true, at, 99, &bindings); } // Ctrl+Tab.
                6 => {
                    keys.set_expanded([None; crate::overlay::SESSION_LIMIT]);
                    keys.set_expanded(std::array::from_fn(|slot| if slot == 0 { bindings[0] } else { None }));
                } // Reopened with the mouse after folding.
                _ => {}
            }
            let outcome = keys.event(0x09, true, at, if case == 5 { 100 } else { 99 }, &bindings);
            assert!(!outcome.consume, "case {case} captured ordinary Tab");
            assert_eq!(outcome.action, None);
        }
        let at = Instant::now();
        let mut keys = Keys::default();
        keys.set_expanded(std::array::from_fn(|slot| if slot == 0 { bindings()[0] } else { None }));
        assert!(!keys.event(0x09, true, at, 99, &bindings()).consume,
            "a mouse-expanded preview has no keyboard selection");
    }

    #[test]
    fn tab_cycles_visible_lanes_wraps_and_enter_opens_the_selection_once() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let mut bindings = bindings();
        bindings[2] = bindings[1].take();
        arm(&mut keys, at, &bindings);
        let mut previous = None;
        for slot in [0, 2, 0, 2] {
            let selected = bindings[slot].unwrap();
            let outcome = keys.event(0x09, true, at, 99, &bindings);
            assert_eq!(outcome.action, Some(Action::Cycle { previous, selected, held: true }));
            assert!(outcome.consume);
            assert_eq!(
                keys.event(0x09, true, at, 99, &bindings).action,
                None,
                "holding Tab does not race through the previews"
            );
            assert!(keys.event(0x09, false, at, 99, &bindings).consume);
            previous = Some(selected);
        }
        let outcome = keys.event(0x0d, true, at, 99, &bindings);
        assert!(outcome.consume);
        assert_eq!(outcome.action, Some(Action::Open(bindings[2].unwrap())));
        assert_eq!(keys.event(0x0d, true, at, 99, &bindings).action, None);
        assert!(keys.event(0x0d, false, at, 99, &bindings).consume);
        assert!(!keys.event(0x0d, true, at, 99, &bindings).consume);
    }

    #[test]
    fn separate_copilot_presses_continue_cycling_and_one_tab_stays_selected() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let mut bindings = bindings();
        for slot in [0, 1, 0] {
            arm(&mut keys, at, &bindings);
            for key in [COPILOT, 0xa0, 0x5b] {
                keys.event(key, false, at, 99, &bindings);
            }
            assert!(matches!(keys.event(0x09, true, at, 99, &bindings).action,
                Some(Action::Cycle { selected, .. }) if selected == bindings[slot].unwrap()));
            keys.event(0x09, false, at, 99, &bindings);
        }
        bindings[1] = None;
        arm(&mut keys, at, &bindings);
        assert_eq!(
            keys.event(0x09, true, at, 99, &bindings).action,
            Some(Action::Cycle {
                previous: bindings[0],
                selected: bindings[0].unwrap(),
                held: true,
            })
        );
    }

    #[test]
    fn cycling_skips_disappeared_tabs_but_enter_never_opens_a_replacement() {
        let at = Instant::now();
        for next_key in [0x09, 0x0d] {
            let mut keys = Keys::default();
            let mut bindings = bindings();
            arm(&mut keys, at, &bindings);
            keys.event(0x09, true, at, 99, &bindings);
            keys.event(0x09, false, at, 99, &bindings);
            bindings[0] = None;
            let outcome = keys.event(next_key, true, at, 99, &bindings);
            if next_key == 0x09 {
                assert_eq!(
                    outcome.action,
                    Some(Action::Cycle {
                        previous: None,
                        selected: bindings[1].unwrap(),
                        held: true,
                    })
                );
                assert!(outcome.consume);
            } else {
                assert!(!outcome.consume);
                assert_eq!(outcome.action, None);
            }
        }
    }

    #[test]
    fn ordinary_and_expired_tab_or_enter_keep_their_normal_behavior() {
        let at = Instant::now();
        for key in [0x09, 0x0d] {
            let mut keys = Keys::default();
            let bindings = bindings();
            assert!(!keys.event(key, true, at, 99, &bindings).consume);
            keys.event(key, false, at, 99, &bindings);
            arm(&mut keys, at, &bindings);
            keys.event(0x09, true, at, 99, &bindings);
            keys.event(0x09, false, at, 99, &bindings);
            keys.event(COPILOT, false, at, 99, &bindings);
            let outcome = keys.event(key, true, at + Duration::from_secs(2), 99, &bindings);
            assert!(!outcome.consume);
            assert_eq!(outcome.action, None);
        }
        let mut keys = Keys::default();
        arm(&mut keys, at, &bindings());
        assert!(
            !keys.event(0x0d, true, at, 99, &bindings()).consume,
            "Enter with no selected overlay must pass through"
        );
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
    fn escape_closes_only_the_selected_live_shortcut_and_balances_releases() {
        let at = Instant::now();
        let mut keys = Keys::default();
        let bindings = bindings();
        assert!(!keys.event(0x1b, true, at, 99, &bindings).consume);
        keys.event(0x1b, false, at, 99, &bindings);
        arm(&mut keys, at, &bindings);
        assert_eq!(keys.event(0x1b, true, at, 99, &bindings).action, None);
        keys.event(0x1b, false, at, 99, &bindings);
        keys.event(COPILOT, false, at, 99, &bindings);
        arm(&mut keys, at, &bindings);
        keys.event(b'D' as u32, true, at, 99, &bindings);
        let outcome = keys.event(0x1b, true, at, 99, &bindings);
        assert_eq!(outcome.action, Some(Action::Close(bindings[0].unwrap())));
        assert!(outcome.consume);
        assert_eq!(keys.event(0x1b, true, at, 99, &bindings).action, None);
        assert!(keys.event(0x1b, false, at, 99, &bindings).consume);
        assert!(!keys.event(0x1b, true, at, 99, &bindings).consume);
    }

    #[test]
    fn expired_or_replaced_keyboard_expansions_do_not_capture_escape() {
        for scenario in 0..3 {
            let at = Instant::now();
            let mut keys = Keys::default();
            let mut bindings = bindings();
            arm(&mut keys, at, &bindings);
            keys.event(b'D' as u32, true, at, 99, &bindings);
            keys.event(COPILOT, false, at, 99, &bindings);
            if scenario == 1 {
                bindings[0].as_mut().unwrap().token += 1;
            }
            let outcome = keys.event(
                0x1b,
                true,
                at + if scenario == 0 {
                    Duration::from_secs(2)
                } else {
                    Duration::ZERO
                },
                if scenario == 2 { 100 } else { 99 },
                &bindings,
            );
            assert!(!outcome.consume);
            assert_eq!(outcome.action, None);
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
