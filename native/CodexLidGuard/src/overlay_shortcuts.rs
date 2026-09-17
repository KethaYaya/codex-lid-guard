//! One dedicated input thread for all overlay windows.
//! See https://learn.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc.
use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Instant;
use crate::{overlay::SESSION_LIMIT, shortcut_config::ShortcutConfig};

#[path = "overlay_shortcut_keys.rs"]
mod keys;
use keys::{Action, Binding, Keys, code_for_label};
#[path = "overlay_hover_typing.rs"]
mod hover_typing;
pub(super) use hover_typing::{TypingTarget, WM_HOVER_TEXT};

pub(super) const WM_OVERLAY_SHORTCUT: u32 = 0x8004;
const WM_REFRESH: u32 = 0x8005;
#[cfg(test)]
const WM_TEST_KEY: u32 = 0x8006;
#[cfg(test)]
const WM_TEST_HOVER_KEY: u32 = 0x8015;
const OWN_INPUT: usize = 0x434c4753;

#[repr(C)]
struct KeyboardEvent {
    key: u32,
    scan: u32,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct KeyboardInput {
    key: u16,
    scan: u16,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct MouseInput {
    x: i32,
    y: i32,
    data: u32,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
union InputData {
    keyboard: KeyboardInput,
    mouse: MouseInput,
}
#[repr(C)]
struct Input {
    kind: u32,
    data: InputData,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowsHookExW(
        kind: i32,
        callback: Option<unsafe extern "system" fn(i32, Wparam, Lparam) -> Lresult>,
        module: Handle,
        thread: u32,
    ) -> Handle;
    fn UnhookWindowsHookEx(hook: Handle) -> Bool;
    fn CallNextHookEx(hook: Handle, code: i32, wparam: Wparam, lparam: Lparam) -> Lresult;
    fn PostThreadMessageW(thread: u32, message: u32, wparam: Wparam, lparam: Lparam) -> Bool;
    fn PeekMessageW(
        message: *mut Message,
        window: Hwnd,
        first: u32,
        last: u32,
        remove: u32,
    ) -> Bool;
    fn GetAsyncKeyState(key: i32) -> i16;
    fn SendInput(count: u32, input: *const Input, size: i32) -> u32;
}

struct HookState {
    keys: Keys,
    bindings: [Option<Binding>; SESSION_LIMIT],
    hints_visible: bool,
    typing: [Option<TypingTarget>; SESSION_LIMIT],
    typed_down: [bool; 256],
}
thread_local! {
    static HOOK_STATE: RefCell<HookState> = RefCell::new(HookState { keys: Keys::default(), bindings: [None; SESSION_LIMIT], hints_visible: false,
        typing: [None; SESSION_LIMIT], typed_down: [false; 256] });
}

fn publish_hints(state: &mut HookState, bindings_changed: bool) {
    let visible = state.keys.prefix_held();
    if visible != state.hints_visible || bindings_changed {
        state.hints_visible = visible;
        for binding in state.bindings.iter().flatten() {
            unsafe { PostMessageW(binding.window as Hwnd, WM_OVERLAY_SHORTCUT, binding.token, if visible { 8 } else { 9 }); }
        }
    }
}

unsafe fn dispatch(action: Action) {
    unsafe {
        // No activation, layout or callback into another thread from the hook.
        let post = |binding: Binding, kind| {
            PostMessageW(binding.window as Hwnd, WM_OVERLAY_SHORTCUT, binding.token, kind);
        };
        match action {
            Action::Expand(binding) => post(binding, 0),
            Action::Open(binding) => post(binding, 1),
            Action::Close(binding) => post(binding, 2),
            Action::Collapse(binding) => post(binding, 3),
            Action::HoldPreview(binding) => post(binding, 7),
            Action::ReleasePreview(binding) => post(binding, 6),
            Action::Cycle { previous, selected, held } => {
                if let Some(previous) = previous.filter(|previous| *previous != selected) {
                    post(previous, 3); // Tuck the previous preview; keep its tab available.
                }
                post(selected, if held { 4 } else { 5 });
            }
        }
    }
}

unsafe fn handle_key(event: &KeyboardEvent, down: bool, foreground: usize,
    hovered: impl FnOnce(&[Option<TypingTarget>]) -> Option<TypingTarget>,
    translate: impl FnOnce(&[u8; 256]) -> Option<isize>) -> keys::Outcome {
    unsafe {
        HOOK_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let bindings = state.bindings;
            let now = Instant::now();
            let typing_allowed = state.keys.allows_typing(now, foreground);
            let held_elsewhere = state.keys.held(event.key)
                && event.key < 256 && !state.typed_down[event.key as usize];
            let mut outcome = state.keys.event(
                event.key,
                down,
                now,
                foreground,
                &bindings,
            );
            if event.key < 256 {
                if !down {
                    outcome.consume |= std::mem::take(&mut state.typed_down[event.key as usize]);
                } else if !outcome.consume && !held_elsewhere && typing_allowed && state.keys.allows_typing(now, foreground)
                    && event.flags & 0x12 == 0 && hover_typing::printable(event.key)
                    && let Some(target) = hovered(&state.typing)
                    && let Some(text) = translate(&state.keys.translation_state(GetKeyState(0x14) & 1 != 0))
                    && PostMessageW(target.surface as Hwnd, WM_HOVER_TEXT, target.token, text) != 0 {
                    state.typed_down[event.key as usize] = true;
                    outcome.consume = true;
                }
            }
            publish_hints(&mut state, false);
            outcome
        })
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: Wparam, lparam: Lparam) -> Lresult {
    unsafe {
        if code != 0 || lparam == 0 {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        let event = &*(lparam as *const KeyboardEvent);
        if event.extra == OWN_INPUT {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        let down = matches!(wparam, 0x0100 | 0x0104);
        if !down && !matches!(wparam, 0x0101 | 0x0105) {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        let foreground = GetForegroundWindow() as usize;
        let outcome = handle_key(event, down, foreground,
            |targets| hover_typing::hovered(targets, foreground),
            |state| hover_typing::text_for(event, state, foreground));
        if outcome.mask_windows_key {
            // Prefix modifiers already passed through. Mark Win/Alt as used so
            // their release cannot open Start or a menu. No text is injected.
            let input = [0, 2].map(|flags| Input {
                kind: 1,
                data: InputData {
                    keyboard: KeyboardInput {
                        key: 0xe8,
                        scan: 0,
                        flags,
                        time: 0,
                        extra: OWN_INPUT,
                    },
                },
            });
            SendInput(
                input.len() as u32,
                input.as_ptr(),
                size_of::<Input>() as i32,
            );
        }
        if let Some(action) = outcome.action {
            dispatch(action);
        }
        if outcome.consume {
            1
        } else {
            CallNextHookEx(null_mut(), code, wparam, lparam)
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
struct Identity {
    window: usize,
    origin: u64,
    session: String,
    label: String,
}
struct Entry {
    identity: Identity,
    binding: Binding,
    expanded: bool,
}
struct Shared {
    entries: Mutex<[Option<Entry>; SESSION_LIMIT]>,
    config: Mutex<ShortcutConfig>,
    thread: AtomicU32,
    next_token: AtomicUsize,
    typing: Mutex<[Option<TypingTarget>; SESSION_LIMIT]>,
}
struct Owner {
    shared: Arc<Shared>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.shared.thread.load(Ordering::Relaxed), 0x0012, 0, 0);
        }
    }
}

pub struct OverlayShortcuts {
    owner: Arc<Owner>,
}
pub struct ShortcutPublisher {
    owner: Arc<Owner>,
    slot: usize,
    identity: Option<Identity>,
    binding: Option<Binding>,
    config: ShortcutConfig,
    expanded: bool,
}

impl OverlayShortcuts {
    pub fn start() -> io::Result<Self> {
        Self::start_with_hook(true)
    }

    fn start_with_hook(install_hook: bool) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            entries: Mutex::new(std::array::from_fn(|_| None)),
            config: Mutex::new(ShortcutConfig::default()),
            thread: AtomicU32::new(0),
            next_token: AtomicUsize::new(1),
            typing: Mutex::new([None; SESSION_LIMIT]),
        });
        let worker = shared.clone();
        let (ready, started) = mpsc::sync_channel(1);
        thread::spawn(move || unsafe {
            let mut message: Message = zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, 0); // Create the thread's message queue.
            worker.thread.store(GetCurrentThreadId(), Ordering::Relaxed);
            if ready.send(()).is_err() {
                return;
            }
            let mut hook: Handle = null_mut();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                if message.message == WM_REFRESH {
                    let (bindings, expanded) = {
                        let entries = worker.entries.lock().unwrap();
                        (entries.each_ref().map(|entry| entry.as_ref().map(|entry| entry.binding)),
                            entries.each_ref().map(|entry| entry.as_ref().filter(|entry| entry.expanded).map(|entry| entry.binding)))
                    };
                    HOOK_STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        if let Some(action) = state.keys.configure(worker.config.lock().unwrap().clone()) {
                            dispatch(action);
                        }
                        let bindings_changed = state.bindings != bindings;
                        state.bindings = bindings;
                        state.keys.set_expanded(expanded);
                        state.typing = *worker.typing.lock().unwrap();
                        if bindings.iter().all(Option::is_none) {
                            state.keys.cancel();
                        }
                        publish_hints(&mut state, bindings_changed);
                    });
                    if install_hook && (bindings.iter().any(Option::is_some)
                        || worker.typing.lock().unwrap().iter().any(Option::is_some)) && hook.is_null() {
                        HOOK_STATE.with(|state| {
                            state
                                .borrow_mut()
                                .keys
                                .seed_modifiers(|key| GetAsyncKeyState(key as i32) < 0)
                        });
                        hook =
                            SetWindowsHookExW(13, Some(keyboard_hook), GetModuleHandleW(null()), 0);
                        if hook.is_null() {
                            logging::write(format!(
                                "Overlay shortcuts unavailable: {}",
                                error("Install keyboard hook")
                            ));
                        }
                    }
                    // Keep the hook until all swallowed key-ups have passed through;
                    // with no shortcuts/composers it leaves every key untouched.
                }
                #[cfg(test)]
                if message.message == WM_TEST_KEY && !install_hook {
                    // Test the actual thread/dispatch path without injecting into the user's apps.
                    let outcome = HOOK_STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        let bindings = state.bindings;
                        let outcome = state.keys.event(
                            message.wparam as u32,
                            message.lparam != 0,
                            Instant::now(),
                            99,
                            &bindings,
                        );
                        publish_hints(&mut state, false);
                        outcome
                    });
                    if let Some(action) = outcome.action {
                        dispatch(action);
                    }
                }
                #[cfg(test)]
                if message.message == WM_TEST_HOVER_KEY && !install_hook {
                    let key = message.wparam as u32;
                    let event = KeyboardEvent { key, scan: 0, flags: 0, time: 0, extra: 0 };
                    let outcome = handle_key(&event, message.lparam & 1 != 0, 99,
                        |targets| targets.iter().flatten().next().copied().filter(|_| message.lparam & 2 != 0),
                        |_| Some((key as u8).to_ascii_lowercase() as isize));
                    if let Some(action) = outcome.action { dispatch(action); }
                }
            }
            if !hook.is_null() {
                UnhookWindowsHookEx(hook);
            }
        });
        started
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| io::Error::other("Start overlay shortcut thread"))?;
        Ok(Self {
            owner: Arc::new(Owner { shared }),
        })
    }

    pub fn publisher(&self, slot: usize) -> ShortcutPublisher {
        ShortcutPublisher {
            owner: self.owner.clone(),
            slot,
            identity: None,
            binding: None,
            config: ShortcutConfig::default(),
            expanded: false,
        }
    }

    #[cfg(test)]
    pub(super) fn simulated() -> Self {
        Self::start_with_hook(false).unwrap()
    }

    #[cfg(test)]
    pub(super) fn test_key(&self, key: u32, down: bool) {
        unsafe {
            assert_ne!(
                PostThreadMessageW(
                    self.owner.shared.thread.load(Ordering::Relaxed),
                    WM_TEST_KEY,
                    key as usize,
                    isize::from(down)
                ),
                0
            );
        }
    }

    #[cfg(test)]
    pub(super) fn test_binding(&self, slot: usize) -> Option<([u8; 2], usize)> {
        self.owner.shared.entries.lock().unwrap()[slot]
            .as_ref()
            .map(|entry| (entry.binding.code, entry.binding.token))
    }
}

#[cfg(test)]
impl OverlayShortcuts {
    #[cfg(test)]
    pub(super) fn test_typing(&self, slot: usize) -> Option<TypingTarget> {
        self.owner.shared.typing.lock().unwrap()[slot]
    }
    #[cfg(test)]
    pub(super) fn test_hover_key(&self, key: u32, down: bool, hovered: bool) {
        unsafe {
            assert_ne!(PostThreadMessageW(self.owner.shared.thread.load(Ordering::Relaxed),
                WM_TEST_HOVER_KEY, key as usize, isize::from(down) | (isize::from(hovered) << 1)), 0);
        }
    }
}

impl ShortcutPublisher {
    pub(super) fn publish_typing(&self, target: Option<TypingTarget>) {
        let shared = &self.owner.shared;
        let mut targets = shared.typing.lock().unwrap();
        if targets[self.slot] == target { return; }
        targets[self.slot] = target;
        drop(targets);
        unsafe { PostThreadMessageW(shared.thread.load(Ordering::Relaxed), WM_REFRESH, 0, 0); }
    }
    pub(super) fn configure(&mut self, config: &ShortcutConfig) {
        if &self.config != config {
            self.clear();
            self.config = config.clone();
        }
        let shared = &self.owner.shared;
        let mut current = shared.config.lock().unwrap();
        if &*current == config { return; }
        *current = config.clone();
        drop(current);
        unsafe { PostThreadMessageW(shared.thread.load(Ordering::Relaxed), WM_REFRESH, 0, 0); }
    }

    pub(super) fn publish(
        &mut self,
        window: usize,
        origin: u64,
        session: &str,
        label: &str,
        expanded: bool,
    ) -> ([u8; 2], usize) {
        if self.identity.as_ref().is_some_and(|old| {
            old.window == window
                && old.origin == origin
                && old.session == session
                && old.label == label
        }) {
            let binding = self.binding.unwrap();
            if self.expanded != expanded {
                self.expanded = expanded;
                self.owner.shared.entries.lock().unwrap()[self.slot].as_mut().unwrap().expanded = expanded;
                unsafe { PostThreadMessageW(self.owner.shared.thread.load(Ordering::Relaxed), WM_REFRESH, 0, 0); }
            }
            return (binding.code, binding.token);
        }
        let identity = Identity {
            window,
            origin,
            session: session.into(),
            label: label.into(),
        };
        let shared = &self.owner.shared;
        let mut entries = shared.entries.lock().unwrap();
        let occupied: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(slot, _)| *slot != self.slot)
            .filter_map(|(_, entry)| entry.as_ref().map(|entry| entry.binding.code[0]))
            .collect();
        let old = entries[self.slot]
            .as_ref()
            .filter(|entry| entry.identity.session == session && entry.identity.origin == origin);
        let code = old
            .map(|entry| entry.binding.code)
            .unwrap_or_else(|| code_for_label(label, &occupied));
        let binding = Binding {
            window,
            code,
            token: shared.next_token.fetch_add(1, Ordering::Relaxed),
        };
        entries[self.slot] = Some(Entry {
            identity: identity.clone(),
            binding,
            expanded,
        });
        self.identity = Some(identity);
        self.binding = Some(binding);
        self.expanded = expanded;
        drop(entries);
        unsafe {
            PostThreadMessageW(shared.thread.load(Ordering::Relaxed), WM_REFRESH, 0, 0);
        }
        (code, binding.token)
    }

    pub(super) fn clear(&mut self) {
        if self.binding.take().is_some() {
            self.identity = None;
            self.owner.shared.entries.lock().unwrap()[self.slot] = None;
            unsafe {
                PostThreadMessageW(
                    self.owner.shared.thread.load(Ordering::Relaxed),
                    WM_REFRESH,
                    0,
                    0,
                );
            }
        }
    }
}

impl Drop for ShortcutPublisher {
    fn drop(&mut self) {
        self.publish_typing(None);
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_handoff_balances_keys_and_never_decodes_unrelated_input() {
        thread::spawn(|| unsafe {
            let surface = CreateWindowExW(0, wide("STATIC").as_ptr(), wide("Typing handoff test").as_ptr(),
                0x80000000, 0, 0, 1, 1, null_mut(), null_mut(), GetModuleHandleW(null()), null());
            assert!(!surface.is_null());
            let target = TypingTarget { overlay: surface as usize, surface: surface as usize, token: 7 };
            let event = KeyboardEvent { key: b'H' as u32, scan: 0, flags: 0, time: 0, extra: 0 };
            let no_text = |_: &[u8; 256]| -> Option<isize> { panic!("unrelated input must not be decoded") };
            assert!(!handle_key(&event, true, 99, |_| None, no_text).consume);
            assert!(!handle_key(&event, true, 99, |_| Some(target), no_text).consume,
                "holding a key in another app and hovering must not redirect repeats");
            assert!(!handle_key(&event, false, 99, |_| None, no_text).consume);
            assert!(handle_key(&event, true, 99, |_| Some(target), |_| Some(b'h' as isize)).consume);
            assert!(handle_key(&event, true, 99, |_| Some(target), |_| Some(b'h' as isize)).consume);
            assert!(handle_key(&event, false, 99, |_| None, no_text).consume,
                "key-up stays balanced even if the pointer leaves");
            assert!(!handle_key(&event, false, 99, |_| None, no_text).consume);
            let mut message: Message = zeroed();
            for _ in 0..2 {
                assert_ne!(PeekMessageW(&mut message, surface, WM_HOVER_TEXT, WM_HOVER_TEXT, 1), 0);
                assert_eq!((message.wparam, message.lparam), (7, b'h' as isize));
            }
            let injected = KeyboardEvent { flags: 0x10, ..event };
            assert!(!handle_key(&injected, true, 99, |_| Some(target), no_text).consume);
            handle_key(&injected, false, 99, |_| None, no_text);
            let modifier = KeyboardEvent { key: 0xa2, ..event };
            handle_key(&modifier, true, 99, |_| Some(target), no_text);
            assert!(!handle_key(&event, true, 99, |_| Some(target), no_text).consume);
            handle_key(&event, false, 99, |_| None, no_text);
            handle_key(&modifier, false, 99, |_| None, no_text);
            assert_eq!(PeekMessageW(&mut message, surface, WM_HOVER_TEXT, WM_HOVER_TEXT, 1), 0);
            DestroyWindow(surface);
        }).join().unwrap();
    }

    #[test]
    fn bindings_keep_unique_prefixes_and_invalidate_old_targets() {
        let service = OverlayShortcuts::simulated();
        let mut first = service.publisher(0);
        let mut second = service.publisher(1);
        let original = first.publish(10, 100, "one", "Project — Dry run", false);
        assert_eq!(original.0, *b"DR");
        assert_eq!(second.publish(11, 100, "two", "Project — Deploy", false).0, *b"EP");
        assert_eq!(first.publish(10, 100, "one", "Project — Dry run", true), original);
        assert!(service.owner.shared.entries.lock().unwrap()[0].as_ref().unwrap().expanded);
        let renamed = first.publish(10, 100, "one", "Project — A new title", false);
        assert_eq!(
            renamed.0, original.0,
            "visible chat shortcuts should stay stable across renames"
        );
        assert_ne!(renamed.1, original.1);
        first.clear();
        assert!(service.test_binding(0).is_none());
        let replacement = first.publish(10, 200, "replacement", "Project — Dry run", false);
        assert_ne!(replacement.1, renamed.1);
        assert_ne!(replacement.0[0], service.test_binding(1).unwrap().0[0]);
    }

    #[test]
    fn keyboard_input_layout_matches_windows_abi() {
        assert_eq!(
            size_of::<KeyboardEvent>(),
            if size_of::<usize>() == 8 { 24 } else { 20 }
        );
        assert_eq!(
            size_of::<Input>(),
            if size_of::<usize>() == 8 { 40 } else { 28 }
        );
    }

    #[test]
    fn native_keyboard_hook_registers_and_releases_on_its_own_thread() {
        thread::spawn(|| unsafe {
            // Empty bindings leave every key untouched; do not synthesize desktop input.
            let hook = SetWindowsHookExW(13, Some(keyboard_hook), GetModuleHandleW(null()), 0);
            assert!(
                !hook.is_null(),
                "{}",
                error("Register shortcut keyboard hook")
            );
            assert_ne!(UnhookWindowsHookEx(hook), 0);
        })
        .join()
        .unwrap();
    }
}
