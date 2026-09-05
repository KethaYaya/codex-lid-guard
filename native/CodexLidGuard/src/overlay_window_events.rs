//! Deliver window transitions to the overlay's existing message loop, without polling.
use super::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::Instant;

pub(super) const WM_FRAME_READY: u32 = 0x8007;
pub(super) const WM_FOREGROUND: u32 = 0x8008;
pub(super) const WM_MINIMIZE: u32 = 0x8009;
pub(super) const WM_RESTORE: u32 = 0x800a;
pub(super) const WM_OPEN_STARTED: u32 = 0x800b;

pub(super) fn notify_open_started(overlay: usize, origin: u64) {
    unsafe {
        PostMessageW(
            overlay as Hwnd,
            WM_OPEN_STARTED,
            origin as usize,
            GetTickCount() as isize,
        );
    }
}

// The callback executes on the UI thread that installed its out-of-context hooks.
thread_local! { static DESTINATION: Cell<usize> = const { Cell::new(0) }; }

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWinEventHook(
        first: u32,
        last: u32,
        module: Handle,
        callback: Option<unsafe extern "system" fn(Handle, u32, Hwnd, i32, i32, u32, u32)>,
        process: u32,
        thread: u32,
        flags: u32,
    ) -> Handle;
    fn UnhookWinEvent(hook: Handle) -> Bool;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetTickCount() -> u32;
}

unsafe extern "system" fn event_callback(
    _: Handle,
    event: u32,
    origin: Hwnd,
    _: i32,
    _: i32,
    _: u32,
    time: u32,
) {
    let message = match event {
        3 => WM_FOREGROUND,
        0x16 => WM_MINIMIZE,
        0x17 => WM_RESTORE, // MINIMIZEEND means restoration, not animation completion.
        _ => return,
    };
    DESTINATION.with(|destination| {
        if destination.get() != 0 {
            unsafe {
                PostMessageW(
                    destination.get() as Hwnd,
                    message,
                    origin as usize,
                    time as isize,
                );
            }
        }
    });
}

pub(super) struct WindowEvents([Handle; 2]);
impl WindowEvents {
    pub(super) unsafe fn new(window: Hwnd) -> io::Result<Self> {
        unsafe {
            DESTINATION.with(|destination| destination.set(window as usize));
            let hooks = Self([
                SetWinEventHook(3, 3, null_mut(), Some(event_callback), 0, 0, 0),
                SetWinEventHook(0x16, 0x17, null_mut(), Some(event_callback), 0, 0, 0),
            ]);
            if hooks.0.iter().any(|hook| hook.is_null()) {
                return Err(error("Observe overlay window transitions"));
            }
            Ok(hooks)
        }
    }
}
impl Drop for WindowEvents {
    fn drop(&mut self) {
        DESTINATION.with(|destination| destination.set(0));
        for hook in self.0 {
            if !hook.is_null() {
                unsafe {
                    UnhookWinEvent(hook);
                }
            }
        }
    }
}

pub struct OverlayUpdates {
    destination: Arc<AtomicUsize>,
    wake: SyncSender<()>,
}
impl OverlayUpdates {
    pub(crate) fn new(destination: Arc<AtomicUsize>, wake: SyncSender<()>) -> Self {
        Self { destination, wake }
    }
    pub(super) fn attach(&self, window: Hwnd) {
        self.destination.store(window as usize, Ordering::Release);
    }
    pub(super) fn refresh(&self) {
        let _ = self.wake.try_send(());
    }
    pub(super) fn detach(&self) {
        self.destination.store(0, Ordering::Release);
    }
    pub(crate) fn notify(destination: &AtomicUsize) {
        let window = destination.load(Ordering::Acquire);
        if window != 0 {
            unsafe {
                PostMessageW(window as Hwnd, WM_FRAME_READY, 0, 0);
            }
        }
    }
}
impl Drop for OverlayUpdates {
    fn drop(&mut self) {
        self.detach();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Transition {
    pub origin: u64,
    pub started: Instant,
}

pub(super) struct Transitions {
    foreground: u64,
    latest: Option<Transition>,
}
impl Transitions {
    pub(super) fn new(foreground: u64) -> Self {
        Self {
            foreground,
            latest: None,
        }
    }
    pub(super) fn observe(&mut self, message: u32, window: u64, at: Instant) {
        match message {
            WM_MINIMIZE => self.record(window, at),
            WM_RESTORE => {
                if self.latest.is_some_and(|event| event.origin == window) {
                    self.latest = None;
                }
            }
            WM_FOREGROUND => {
                if self.foreground != window {
                    self.record(self.foreground, at);
                    self.foreground = window;
                }
                if self.latest.is_some_and(|event| event.origin == window) {
                    self.latest = None;
                }
            }
            _ => {}
        }
    }
    fn record(&mut self, origin: u64, at: Instant) {
        if origin != 0
            && !self.latest.is_some_and(|event| {
                event.origin == origin
                    && at.saturating_duration_since(event.started) < Duration::from_millis(500)
            })
        {
            self.latest = Some(Transition {
                origin,
                started: at,
            });
        }
    }
    pub(super) fn for_window(&self, window: Option<u64>, now: Instant) -> Option<Transition> {
        self.latest.filter(|event| {
            Some(event.origin) == window
                && now.saturating_duration_since(event.started) < Duration::from_secs(1)
        })
    }
}

pub(super) fn event_instant(tick: u32) -> Instant {
    instant_from_ticks(Instant::now(), unsafe { GetTickCount() }, tick)
}
#[cfg(test)]
pub(super) fn test_tick() -> u32 {
    unsafe { GetTickCount() }
}
fn instant_from_ticks(now: Instant, current: u32, event: u32) -> Instant {
    now.checked_sub(Duration::from_millis(current.wrapping_sub(event) as u64))
        .unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn NotifyWinEvent(event: u32, window: Hwnd, object: i32, child: i32);
        fn PeekMessageW(
            message: *mut Message,
            window: Hwnd,
            first: u32,
            last: u32,
            remove: u32,
        ) -> Bool;
    }
    #[test]
    fn native_window_event_hook_delivers_owned_events_and_cleans_up() {
        thread::spawn(|| unsafe {
            let window = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("Owned event test").as_ptr(),
                0x80000000,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            assert!(!window.is_null());
            let hooks = WindowEvents::new(window).unwrap();
            NotifyWinEvent(0x16, window, 0, 0);
            let started = Instant::now();
            let mut observed = false;
            while started.elapsed() < Duration::from_secs(2) {
                let mut message: Message = zeroed();
                if PeekMessageW(&mut message, null_mut(), 0, 0, 1) != 0
                    && message.message == WM_MINIMIZE
                {
                    assert_eq!(message.wparam, window as usize);
                    assert!(event_instant(message.lparam as u32) <= Instant::now());
                    observed = true;
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
            drop(hooks);
            assert_eq!(DESTINATION.with(Cell::get), 0);
            DestroyWindow(window);
            assert!(
                observed,
                "Windows minimize events must reach the overlay queue"
            );
        })
        .join()
        .unwrap();
    }
    #[test]
    fn minimize_and_focus_loss_share_one_start_and_restore_resets_it() {
        let now = Instant::now();
        let mut events = Transitions::new(10);
        events.observe(WM_MINIMIZE, 10, now);
        events.observe(WM_FOREGROUND, 20, now + Duration::from_millis(170));
        assert_eq!(events.for_window(Some(10), now).unwrap().started, now);
        assert!(events.for_window(Some(20), now).is_none());
        events.observe(WM_RESTORE, 10, now + Duration::from_millis(200));
        assert!(events.for_window(Some(10), now).is_none());
        events.observe(WM_FOREGROUND, 10, now + Duration::from_millis(220));
        events.observe(WM_MINIMIZE, 10, now + Duration::from_millis(250));
        assert_eq!(
            events.for_window(Some(10), now).unwrap().started,
            now + Duration::from_millis(250)
        );
        assert!(
            events
                .for_window(Some(10), now + Duration::from_secs(2))
                .is_none()
        );
    }
    #[test]
    fn event_timestamp_accounts_for_delivery_delay_and_tick_wrap() {
        let now = Instant::now();
        assert_eq!(
            instant_from_ticks(now, 1000, 920),
            now - Duration::from_millis(80)
        );
        assert_eq!(
            instant_from_ticks(now, 20, u32::MAX - 19),
            now - Duration::from_millis(40)
        );
    }
}
