//! Reserve a chat sidebar through the shell, then restore the affected windows.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

pub(super) const CALLBACK: u32 = 0x8020;
const ABM_NEW: u32 = 0;
const ABM_REMOVE: u32 = 1;
const ABM_QUERYPOS: u32 = 2;
const ABM_SETPOS: u32 = 3;
const PROPERTY: &str = "CodexLidGuard.WorkspaceOwner";
static ACTIVE: Mutex<Option<Workspace>> = Mutex::new(None);
static OWNER: AtomicUsize = AtomicUsize::new(0);
static DIRTY: AtomicBool = AtomicBool::new(false);
static BLOCKED_OWNER: AtomicUsize = AtomicUsize::new(0);
static COOKIE: AtomicUsize = AtomicUsize::new(1);
#[cfg(test)]
static DESKTOP_FIXTURE: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(super) struct DesktopFixture(pub usize);
#[cfg(test)]
impl DesktopFixture {
    pub fn new(window: Hwnd) -> Self { DESKTOP_FIXTURE.store(window as usize, Ordering::Release); Self(window as usize) }
}
#[cfg(test)]
impl Drop for DesktopFixture {
    fn drop(&mut self) { release(self.0); DESKTOP_FIXTURE.store(0, Ordering::Release); }
}

#[repr(C)]
struct AppBarData { size: u32, window: Hwnd, callback: u32, edge: u32, rect: Rect, parameter: isize }
#[repr(C)]
struct Monitor { size: u32, screen: Rect, work: Rect, flags: u32 }
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement { size: u32, flags: u32, show: u32, minimum: [i32; 2], maximum: [i32; 2], normal: Rect }

#[link(name = "shell32")]
unsafe extern "system" { fn SHAppBarMessage(message: u32, data: *mut AppBarData) -> usize; }
#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(callback: unsafe extern "system" fn(Hwnd, isize) -> Bool, data: isize) -> Bool;
    fn GetWindow(window: Hwnd, command: u32) -> Hwnd;
    fn IsWindowVisible(window: Hwnd) -> Bool;
    fn IsIconic(window: Hwnd) -> Bool;
    fn MonitorFromWindow(window: Hwnd, flags: u32) -> Handle;
    fn GetMonitorInfoW(monitor: Handle, info: *mut Monitor) -> Bool;
    fn GetWindowPlacement(window: Hwnd, placement: *mut Placement) -> Bool;
    fn SetWindowPlacement(window: Hwnd, placement: *const Placement) -> Bool;
    fn SetWindowPos(window: Hwnd, after: Hwnd, x: i32, y: i32, width: i32, height: i32, flags: u32) -> Bool;
    fn SetPropW(window: Hwnd, name: *const u16, value: Handle) -> Bool;
    fn GetPropW(window: Hwnd, name: *const u16) -> Handle;
    fn RemovePropW(window: Hwnd, name: *const u16) -> Handle;
    fn SetThreadDpiAwarenessContext(context: Handle) -> Handle;
}
#[link(name = "dwmapi")]
unsafe extern "system" { fn DwmGetWindowAttribute(window: Hwnd, attribute: u32, value: *mut c_void, size: u32) -> i32; }

struct DpiContext(Handle);
impl DpiContext { fn new() -> Self { Self(unsafe { SetThreadDpiAwarenessContext(-4isize as Handle) }) } }
impl Drop for DpiContext { fn drop(&mut self) { unsafe { if !self.0.is_null() { SetThreadDpiAwarenessContext(self.0); } } } }

pub(super) fn sidebar_bounds(work: Rect) -> Rect {
    Rect { left: work.right - ((work.right - work.left) / 3).max(1), ..work }
}

fn fit(bounds: Rect, available: Rect) -> Rect {
    let width = (bounds.right - bounds.left).min(available.right - available.left).max(1);
    let height = (bounds.bottom - bounds.top).min(available.bottom - available.top).max(1);
    let left = bounds.left.clamp(available.left, available.right - width);
    let top = bounds.top.clamp(available.top, available.bottom - height);
    Rect { left, top, right: left + width, bottom: top + height }
}

fn monitor(window: Hwnd) -> io::Result<(usize, Monitor)> {
    let _dpi = DpiContext::new();
    unsafe {
        let handle = MonitorFromWindow(window, 2);
        let mut info: Monitor = zeroed(); info.size = size_of::<Monitor>() as u32;
        if GetMonitorInfoW(handle, &mut info) == 0 { return Err(error("Read sidebar monitor")); }
        Ok((handle as usize, info))
    }
}

struct SavedWindow { window: usize, process: u32, thread: u32, monitor: usize, cookie: usize, placement: Placement }
impl SavedWindow {
    unsafe fn capture(window: Hwnd, cookie: usize) -> Option<Self> {
        let _dpi = DpiContext::new();
        unsafe {
            let mut placement: Placement = zeroed(); placement.size = size_of::<Placement>() as u32;
            let mut process = 0;
            let thread = GetWindowThreadProcessId(window, &mut process);
            if thread == 0 || GetWindowPlacement(window, &mut placement) == 0
                || SetPropW(window, wide(PROPERTY).as_ptr(), cookie as Handle) == 0 { return None; }
            Some(Self { window: window as usize, process, thread, monitor: MonitorFromWindow(window, 2) as usize, cookie, placement })
        }
    }

    unsafe fn matches(&self) -> bool {
        unsafe {
            let mut process = 0;
            let window = self.window as Hwnd;
            GetWindowThreadProcessId(window, &mut process) == self.thread && process == self.process
                && GetPropW(window, wide(PROPERTY).as_ptr()) as usize == self.cookie
        }
    }

    unsafe fn fit(&self, available: Rect) {
        let _dpi = DpiContext::new();
        unsafe {
            let window = self.window as Hwnd;
            let mut current: Placement = zeroed(); current.size = size_of::<Placement>() as u32;
            // The shell keeps maximized windows in the reduced work area.
            if !self.matches() || MonitorFromWindow(window, 2) as usize != self.monitor || IsIconic(window) != 0
                || GetWindowPlacement(window, &mut current) == 0 || current.show == 3 { return; }
            let mut bounds: Rect = zeroed();
            if GetWindowRect(window, &mut bounds) == 0 { return; }
            let target = fit(bounds, available);
            if target != bounds {
                SetWindowPos(window, null_mut(), target.left, target.top, target.right-target.left, target.bottom-target.top,
                    0x0004 | 0x0010 | 0x0200 | 0x4000); // Keep focus/Z order; async across input queues.
            }
        }
    }

    unsafe fn restore(&self) {
        let _dpi = DpiContext::new();
        unsafe {
            if !self.matches() { return; } // A recycled HWND must never receive an old placement.
            let window = self.window as Hwnd;
            let mut current: Placement = zeroed(); current.size = size_of::<Placement>() as u32;
            if MonitorFromWindow(window, 2) as usize == self.monitor && GetWindowPlacement(window, &mut current) != 0 {
                let mut original = self.placement;
                // Keep a window minimized if the user minimized it while reading.
                if IsIconic(window) != 0 {
                    original.show = 7;
                    if self.placement.show == 3 { original.flags |= 2; }
                } else if original.show != 3 { original.show = 4; } // SW_SHOWNOACTIVATE
                original.flags |= 4; // WPF_ASYNCWINDOWPLACEMENT
                // Removing the appbar already restores an unchanged maximized window.
                if (current.show != self.placement.show || current.normal != self.placement.normal || current.show != 3)
                    && SetWindowPlacement(window, &original) == 0 {
                    logging::write(format!("Could not restore a window after closing the chat sidebar: {}", io::Error::last_os_error()));
                }
            }
            RemovePropW(window, wide(PROPERTY).as_ptr());
        }
    }
}

struct Workspace {
    owner: usize,
    monitor: usize,
    screen: Rect,
    work: Rect,
    sidebar: Rect,
    saved: Vec<SavedWindow>,
    registered: bool,
    reported: bool,
    next_scan: Instant,
}

impl Workspace {
    fn data(&self) -> AppBarData {
        AppBarData { size: size_of::<AppBarData>() as u32, window: self.owner as Hwnd,
            callback: CALLBACK, edge: 2, rect: self.sidebar, parameter: 0 }
    }

    unsafe fn capture_windows(&mut self) {
        unsafe extern "system" fn visit(window: Hwnd, data: isize) -> Bool {
            unsafe {
                let state = &mut *(data as *mut Workspace);
                let mut process = 0;
                GetWindowThreadProcessId(window, &mut process);
                let style = GetWindowLongPtrW(window, -16) as u32;
                let extended = GetWindowLongPtrW(window, -20) as u32;
                let mut cloaked = 0u32;
                DwmGetWindowAttribute(window, 14, (&mut cloaked as *mut u32).cast(), size_of::<u32>() as u32);
                if process == GetCurrentProcessId() || IsWindowVisible(window) == 0 || IsIconic(window) != 0
                    || !GetWindow(window, 4).is_null() || style & 0x0004_0000 == 0 || extended & 0x80 != 0
                    || cloaked != 0 || MonitorFromWindow(window, 2) as usize != state.monitor { return 1; }
                if !state.saved.iter().any(|saved| saved.window == window as usize && saved.matches())
                    && let Some(saved) = SavedWindow::capture(window, COOKIE.fetch_add(1, Ordering::Relaxed)) { state.saved.push(saved); }
                1
            }
        }
        unsafe { EnumWindows(visit, (self as *mut Workspace) as isize); }
    }

    fn open(owner: Hwnd) -> io::Result<Self> {
        let _dpi = DpiContext::new();
        let (handle, info) = monitor(owner)?;
        let mut state = Self { owner: owner as usize, monitor: handle, screen: info.screen, work: info.work,
            sidebar: sidebar_bounds(info.work), saved: vec![],
            registered: false, reported: false, next_scan: Instant::now() };
        unsafe {
            // Snapshot BEFORE the shell changes any maximized or snapped windows.
            state.capture_windows();
            if SHAppBarMessage(ABM_NEW, &mut state.data()) == 0 { return Err(error("Reserve chat sidebar")); }
        }
        state.registered = true;
        OWNER.store(state.owner, Ordering::Release);
        state.position(&info);
        state.fit_windows();
        Ok(state)
    }

    fn position(&mut self, info: &Monitor) {
        let _dpi = DpiContext::new();
        let mut data = self.data();
        // Preserve the taskbar's existing work-area inset. On a refresh, add
        // back only our own strip before asking the shell about other appbars.
        data.rect = Rect { right: if info.work.right == self.sidebar.left { self.work.right } else { info.work.right }, ..info.work };
        unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut data); }
        let work = data.rect;
        data.rect = sidebar_bounds(work);
        if data.rect != self.sidebar || self.work != work || self.next_scan <= Instant::now() {
            unsafe { SHAppBarMessage(ABM_SETPOS, &mut data); }
            self.work = work;
            self.sidebar = data.rect;
            self.reported = false;
        }
    }

    fn fit_windows(&mut self) {
        let available = Rect { right: self.sidebar.left, ..self.work };
        unsafe { for saved in &self.saved { saved.fit(available); } }
        self.next_scan = Instant::now() + Duration::from_millis(750);
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _dpi = DpiContext::new();
        OWNER.compare_exchange(self.owner, 0, Ordering::AcqRel, Ordering::Relaxed).ok();
        unsafe {
            if self.registered { SHAppBarMessage(ABM_REMOVE, &mut self.data()); }
            for saved in &self.saved { saved.restore(); }
        }
    }
}

// A lease covers all normal returns and error paths of an overlay worker.
pub(super) struct Lease(pub usize);
impl Drop for Lease { fn drop(&mut self) { release(self.0); } }

pub(super) fn release(owner: usize) {
    let mut active = ACTIVE.lock().unwrap();
    if active.as_ref().is_some_and(|state| state.owner == owner) { active.take(); }
}

pub fn restore_overlay_workspace() {
    let mut active = ACTIVE.lock().unwrap();
    if let Some(state) = active.as_ref() {
        BLOCKED_OWNER.store(state.owner, Ordering::Release);
        unsafe { PostMessageW(state.owner as Hwnd, 0x800d, 0, 0); }
    }
    active.take();
}

pub(super) fn work_area(owner: Hwnd, fallback: Rect) -> Rect {
    ACTIVE.lock().unwrap().as_ref().filter(|state| state.owner == owner as usize).map_or(fallback, |state| state.work)
}

pub(super) fn sync(owner: Hwnd, expanded: bool, fallback: Rect) -> io::Result<Option<Rect>> {
    // Native interaction fixtures may show overlays, but must never resize the user's apps.
    #[cfg(test)]
    if DESKTOP_FIXTURE.load(Ordering::Acquire) != owner as usize { return Ok(expanded.then(|| sidebar_bounds(fallback))); }
    #[cfg(not(test))]
    let _ = fallback;
    let mut active = ACTIVE.lock().unwrap();
    if !expanded || BLOCKED_OWNER.load(Ordering::Acquire) == owner as usize {
        if active.as_ref().is_some_and(|state| state.owner == owner as usize) { active.take(); }
        if !expanded { BLOCKED_OWNER.compare_exchange(owner as usize, 0, Ordering::AcqRel, Ordering::Relaxed).ok(); }
        return Ok(None);
    }
    let (handle, info) = monitor(owner)?;
    if active.as_ref().is_some_and(|state| state.owner != owner as usize || state.monitor != handle || state.screen != info.screen) { active.take(); }
    if active.is_none() { *active = Some(Workspace::open(owner)?); }
    let state = active.as_mut().unwrap();
    if DIRTY.swap(false, Ordering::AcqRel) { state.position(&info); }
    if Instant::now() >= state.next_scan {
        unsafe { state.capture_windows(); }
        state.fit_windows();
    }
    Ok(Some(state.sidebar))
}

pub(super) fn notification(window: Hwnd, message: u32, parameter: usize) -> bool {
    if message == CALLBACK {
        if parameter == 1 { DIRTY.store(true, Ordering::Release); } // ABN_POSCHANGED
        return true;
    }
    if OWNER.load(Ordering::Acquire) == window as usize && message == 0x0006 {
        unsafe {
            let mut data = AppBarData { size: size_of::<AppBarData>() as u32, window,
                callback: CALLBACK, edge: 2, rect: zeroed(), parameter: (parameter & 0xffff != 0) as isize };
            SHAppBarMessage(6, &mut data); // ABM_ACTIVATE
        }
    }
    false
}

pub(super) fn positioned(window: Hwnd, bounds: Rect) {
    let data = {
        let mut active = ACTIVE.lock().unwrap();
        active.as_mut().filter(|state| state.owner == window as usize && bounds == state.sidebar && !state.reported)
            .map(|state| { state.reported = true; state.data() })
    };
    if let Some(mut data) = data { unsafe { SHAppBarMessage(9, &mut data); } } // ABM_WINDOWPOSCHANGED
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sidebar_uses_one_third_of_the_work_area_on_any_monitor() {
        for work in [Rect { left: -1920, top: 40, right: 0, bottom: 1080 },
            Rect { left: 1920, top: -900, right: 4480, bottom: 500 },
            Rect { left: 0, top: 0, right: 800, bottom: 560 }] {
            let sidebar = sidebar_bounds(work);
            assert_eq!((sidebar.top, sidebar.right, sidebar.bottom), (work.top, work.right, work.bottom));
            assert!(((sidebar.right-sidebar.left)*3 - (work.right-work.left)).abs() <= 2);
            let available = Rect { right: sidebar.left, ..work };
            assert_eq!(fit(work, available), available);
            let small = Rect { right: available.left + 200, bottom: available.top + 120, ..available };
            assert_eq!(fit(small, available), small);
            let moved = fit(sidebar, available);
            assert!(moved.left >= available.left && moved.right <= available.right);
        }
    }

    unsafe fn fixture() -> Hwnd {
        unsafe {
            let window = CreateWindowExW(0, wide("STATIC").as_ptr(), wide("Owned sidebar placement fixture").as_ptr(),
                0x00cf0000, 150, 130, 780, 460, null_mut(), null_mut(), GetModuleHandleW(null()), null());
            assert!(!window.is_null());
            ShowWindow(window, 4);
            window
        }
    }

    struct Owned(Hwnd);
    impl Drop for Owned { fn drop(&mut self) { unsafe { DestroyWindow(self.0); } } }

    #[test]
    #[ignore = "resizes only an owned fixture window; does not reserve desktop space"]
    fn native_window_sizes_restore_and_reused_handles_are_ignored() {
        let _dpi = DpiContext::new();
        unsafe {
            let owned = Owned(fixture());
            let window = owned.0;
            let saved = SavedWindow::capture(window, 501).unwrap();
            let original = saved.placement;
            saved.fit(Rect { left: 40, top: 40, right: 550, bottom: 640 });
            let mut bounds: Rect = zeroed();
            GetWindowRect(window, &mut bounds);
            assert!(bounds.right <= 550 && bounds.left >= 40);
            saved.restore();
            let mut restored: Placement = zeroed(); restored.size = size_of::<Placement>() as u32;
            assert_ne!(GetWindowPlacement(window, &mut restored), 0);
            assert_eq!(restored.normal, original.normal);
            assert!(!saved.matches(), "a completed restore removes its identity marker");

            let stale = SavedWindow::capture(window, 502).unwrap();
            stale.fit(Rect { left: 40, top: 40, right: 550, bottom: 640 });
            let current = SavedWindow::capture(window, 503).unwrap();
            stale.restore();
            GetWindowPlacement(window, &mut restored);
            assert_eq!(restored.normal, current.placement.normal, "a stale owner cannot restore a different window incarnation");
            current.restore();
            SetWindowPlacement(window, &original);
        }
    }

    #[test]
    #[ignore = "briefly reserves one-third of the desktop and restores all affected window placements"]
    fn native_shell_reserves_and_releases_sidebar_work_area() {
        let _dpi = DpiContext::new();
        unsafe {
            let owned = Owned(fixture());
            let foreground = GetForegroundWindow();
            let (_, before) = monitor(owned.0).unwrap();
            let state = Workspace::open(owned.0).unwrap();
            assert_eq!(state.work, before.work, "the appbar query excludes its own reservation");
            assert_eq!(state.sidebar, sidebar_bounds(before.work));
            let (_, reserved) = monitor(owned.0).unwrap();
            assert_eq!(reserved.work, Rect { right: state.sidebar.left, ..before.work });
            // A shell notification must not reserve a third of the already reduced area.
            let mut state = state;
            state.position(&reserved);
            assert_eq!(state.work, before.work);
            let saved: Vec<_> = state.saved.iter().map(|window| (window.window, window.placement)).collect();
            drop(state);
            let (_, restored) = monitor(owned.0).unwrap();
            assert_eq!(restored.work, before.work);
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let restored = saved.iter().all(|(window, original)| {
                    let mut placement: Placement = zeroed(); placement.size = size_of::<Placement>() as u32;
                    GetWindowPlacement(*window as Hwnd, &mut placement) == 0
                        || (placement.normal == original.normal && placement.show == original.show)
                });
                if restored { break; }
                assert!(Instant::now() < deadline, "window placements were not restored after releasing the reservation");
                thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(GetForegroundWindow(), foreground, "reserving and restoring space must not switch apps");
        }
    }
}
