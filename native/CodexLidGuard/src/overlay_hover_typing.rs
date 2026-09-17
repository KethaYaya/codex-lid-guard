//! A bounded handoff of the first printable keystroke to a hovered composer.
//! Never decodes input outside the overlay and never logs or persists keys.
use super::*;

pub(in super::super) const WM_HOVER_TEXT: u32 = 0x8014;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) struct TypingTarget {
    pub overlay: usize,
    pub surface: usize,
    pub token: usize,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn WindowFromPoint(point: Point) -> Hwnd;
    fn GetAncestor(window: Hwnd, flags: u32) -> Hwnd;
    fn IsWindowVisible(window: Hwnd) -> Bool;
    fn GetKeyboardLayout(thread: u32) -> Handle;
    fn ToUnicodeEx(key: u32, scan: u32, state: *const u8, text: *mut u16,
        count: i32, flags: u32, layout: Handle) -> i32;
}

pub(super) fn target_at(targets: &[Option<TypingTarget>], hovered: usize, foreground: usize) -> Option<TypingTarget> {
    targets.iter().flatten().copied().find(|target|
        (hovered == target.overlay || hovered == target.surface) && foreground != target.surface)
}

pub(super) fn printable(key: u32) -> bool {
    matches!(key, 0x20 | 0x30..=0x39 | 0x41..=0x5a | 0x60..=0x6f | 0xba..=0xc0 | 0xdb..=0xe2)
}

pub(super) unsafe fn hovered(targets: &[Option<TypingTarget>], foreground: usize) -> Option<TypingTarget> {
    unsafe {
        if targets.iter().all(Option::is_none) { return None; }
        let mut point: Point = zeroed();
        if GetCursorPos(&mut point) == 0 { return None; }
        let root = GetAncestor(WindowFromPoint(point), 2) as usize;
        target_at(targets, root, foreground).filter(|target| IsWindowVisible(target.surface as Hwnd) != 0)
    }
}

pub(super) unsafe fn text_for(event: &KeyboardEvent, state: &[u8; 256], foreground: usize) -> Option<isize> {
    unsafe {
        let layout = GetKeyboardLayout(GetWindowThreadProcessId(foreground as Hwnd, null_mut()));
        let mut text = [0u16; 4];
        // Flag 4 leaves dead-key/IME state untouched. Dead keys and non-text
        // events continue through their original Windows input path.
        let count = ToUnicodeEx(event.key, event.scan, state.as_ptr(), text.as_mut_ptr(), 4, 4, layout);
        pack_text(&text, count)
    }
}

fn pack_text(text: &[u16; 4], count: i32) -> Option<isize> {
    if !(1..=4).contains(&count) || text[..count as usize].iter().any(|unit| *unit < 0x20 || *unit == 0x7f) { return None; }
    Some(text[..count as usize].iter().enumerate().fold(0u64, |packed, (index, unit)| packed | ((*unit as u64) << (index * 16))) as isize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_requires_a_hovered_composer_that_does_not_already_have_focus() {
        let target = TypingTarget { overlay: 10, surface: 11, token: 1 };
        let targets = [Some(target), None];
        assert_eq!(target_at(&targets, 10, 99), Some(target));
        assert_eq!(target_at(&targets, 11, 99), Some(target));
        assert_eq!(target_at(&targets, 12, 99), None);
        assert_eq!(target_at(&targets, 10, 11), None);
        assert_eq!(target_at(&[None], 10, 99), None);
    }

    #[test]
    fn handoff_keeps_unicode_and_rejects_shortcuts_controls_and_dead_keys() {
        assert_eq!(pack_text(&[0xd83c, 0xdf0d, 0, 0], 2), Some(0xdf0dd83c));
        assert_eq!(pack_text(&[b'A' as u16, 99, 99, 99], 1), Some(65));
        for count in [-1, 0, 5] { assert_eq!(pack_text(&[65; 4], count), None); }
        assert_eq!(pack_text(&[13, 0, 0, 0], 1), None);
        for key in [9, 13, 27, 0x5b, 0x70, 0x86, 0xe5] { assert!(!printable(key)); }
        for key in [0x20, 0x41, 0x30, 0xbd] { assert!(printable(key)); }
    }
}
