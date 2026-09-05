//! One dedicated input thread for the three overlay windows.
//! See https://learn.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc.
use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Instant;

#[path = "overlay_shortcut_keys.rs"]
mod keys;
use keys::{Action, Binding, Keys, code_for_label};

pub(super) const WM_OVERLAY_SHORTCUT: u32 = 0x8004;
const WM_REFRESH: u32 = 0x8005;
#[cfg(test)]
const WM_TEST_KEY: u32 = 0x8006;
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
    bindings: [Option<Binding>; 3],
}
thread_local! {
    static HOOK_STATE: RefCell<HookState> = RefCell::new(HookState { keys: Keys::default(), bindings: [None; 3] });
}

unsafe fn dispatch(action: Action) {
    unsafe {
        let (binding, kind) = match action {
            Action::Expand(binding) => (binding, 0),
            Action::Open(binding) => (binding, 1),
        };
        // No activation, layout or callback into another thread from the hook.
        PostMessageW(
            binding.window as Hwnd,
            WM_OVERLAY_SHORTCUT,
            binding.token,
            kind,
        );
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
        let outcome = HOOK_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let bindings = state.bindings;
            state.keys.event(
                event.key,
                down,
                Instant::now(),
                GetForegroundWindow() as usize,
                &bindings,
            )
        });
        if outcome.mask_windows_key {
            // The Copilot macro's Win/Shift events have already passed through.
            // Mark Win as used so its release cannot open Start. No text is injected.
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
}
struct Shared {
    entries: Mutex<[Option<Entry>; 3]>,
    thread: AtomicU32,
    next_token: AtomicUsize,
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
}

impl OverlayShortcuts {
    pub fn start() -> io::Result<Self> {
        Self::start_with_hook(true)
    }

    fn start_with_hook(install_hook: bool) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            entries: Mutex::new(std::array::from_fn(|_| None)),
            thread: AtomicU32::new(0),
            next_token: AtomicUsize::new(1),
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
                    let bindings = worker
                        .entries
                        .lock()
                        .unwrap()
                        .each_ref()
                        .map(|entry| entry.as_ref().map(|entry| entry.binding));
                    HOOK_STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        state.bindings = bindings;
                        if bindings.iter().all(Option::is_none) {
                            state.keys.cancel();
                        }
                    });
                    if install_hook && bindings.iter().any(Option::is_some) && hook.is_null() {
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
                    // with no bindings its fast path leaves every key untouched.
                }
                #[cfg(test)]
                if message.message == WM_TEST_KEY && !install_hook {
                    // Test the actual thread/dispatch path without injecting into the user's apps.
                    let outcome = HOOK_STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        let bindings = state.bindings;
                        state.keys.event(
                            message.wparam as u32,
                            message.lparam != 0,
                            Instant::now(),
                            99,
                            &bindings,
                        )
                    });
                    if let Some(action) = outcome.action {
                        dispatch(action);
                    }
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

impl ShortcutPublisher {
    pub(super) fn publish(
        &mut self,
        window: usize,
        origin: u64,
        session: &str,
        label: &str,
    ) -> ([u8; 2], usize) {
        if self.identity.as_ref().is_some_and(|old| {
            old.window == window
                && old.origin == origin
                && old.session == session
                && old.label == label
        }) {
            let binding = self.binding.unwrap();
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
        });
        self.identity = Some(identity);
        self.binding = Some(binding);
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
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_keep_unique_prefixes_and_invalidate_old_targets() {
        let service = OverlayShortcuts::simulated();
        let mut first = service.publisher(0);
        let mut second = service.publisher(1);
        let original = first.publish(10, 100, "one", "Project — Dry run");
        assert_eq!(original.0, *b"DR");
        assert_eq!(second.publish(11, 100, "two", "Project — Deploy").0, *b"EP");
        assert_eq!(first.publish(10, 100, "one", "Project — Dry run"), original);
        let renamed = first.publish(10, 100, "one", "Project — A new title");
        assert_eq!(
            renamed.0, original.0,
            "visible chat shortcuts should stay stable across renames"
        );
        assert_ne!(renamed.1, original.1);
        first.clear();
        assert!(service.test_binding(0).is_none());
        let replacement = first.publish(10, 200, "replacement", "Project — Dry run");
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
