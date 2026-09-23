//! Exercise actual window input using messages sent only to an owned test HWND.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
    mpsc,
};

pub(super) fn query(state: &OverlayState, kind: usize) -> isize {
    let Some(ui) = &state.group else { return 0 };
    match kind {
        0 => ui
            .selected_session()
            .and_then(|s| s.card.target.as_ref())
            .map_or(0, |t| t.window as isize),
        1 => ui.group.sessions.len() as isize,
        2 => state.dpi as isize,
        3 => state.collapsed as isize,
        10 => state.shortcut_hints as isize,
        11 => state.animate as isize,
        12 => ui.group.sessions.iter().filter(|s| s.card.attention && s.card.final_message).count() as isize,
        13 => state.composer.as_ref().map_or(0, |composer| composer.test_input() as isize),
        14 => state.composer.as_ref().is_some_and(|composer| composer.test_notice()) as isize,
        16 => state.composer.as_ref().is_some_and(|composer| composer.expanded()) as isize,
        17 => state.composer.as_ref().map_or(0, |composer| composer.test_window() as isize),
        18 => state.composer.as_ref().is_some_and(|composer| composer.test_animating()) as isize,
        20 => state.composer.as_ref().is_some_and(|composer| composer.focused()) as isize,
        21 => ui.selected_session().map_or(0, |session| session.activity as isize),
        22 => state.restoring as isize,
        25 => match ui.progression.stage { Stage::Compact => 0, Stage::Message => 1, Stage::Full => 2 },
        26 => ui.chat.as_ref().map_or(0, |chat| chat.content_height() as isize),
        27 => ui.chat.as_ref().map_or(0, |chat| (chat.bounds.bottom - chat.bounds.top) as isize),
        33 => ui.pinned as isize,
        34 => ui.fold_after_click.is_some() as isize,
        35 => state.tab_drag.moving() as isize,
        31 => state.layout.and_then(|layout| layout.panel).map_or(0, |panel| {
            let x = panel.left + scale_dip(20, state.dpi);
            let y = panel.top + scale_dip(10, state.dpi);
            ((y as u32) << 16 | x as u32) as isize
        }),
        32 => state.layout.and_then(|layout| layout.tab).map_or(0, |tab| {
            ((((tab.top + tab.bottom) / 2) as u32) << 16 | ((tab.left + tab.right) / 2) as u32) as isize
        }),
        _ => ui
            .test_point(kind)
            .and_then(|(x, y)| {
                state.layout?.panel.map(|panel| {
                    (((y + panel.top) as u32) << 16 | (x + panel.left) as u32) as isize
                })
            })
            .unwrap_or(0),
    }
}

#[test]
#[ignore = "creates an owned overlay; sends mouse messages only to its test HWND"]
fn native_tab_drag_moves_without_opening_and_preserves_clicks_and_hover() {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let pointer = Arc::new(std::sync::Mutex::new(None));
        let cursor = pointer.clone();
        let shown = Arc::new(AtomicBool::new(true));
        let feed_shown = shown.clone();
        let thread = std::thread::spawn(move || run_overlay_inner(Some(0), move |_| {
            if !feed_shown.load(Ordering::Relaxed) { return Frame { close: done.load(Ordering::Relaxed), ..Frame::empty() }; }
            crate::overlay::groups::group_frames(vec![Frame {
                session_id: Some("drag-fixture".into()), project_path: Some(r"C:\DragFixture".into()),
                cards: vec![Card { id: 1, label: "Drag fixture".into(), text: "Keep this chat folded while moving its tab.".into(),
                    final_message: false, attention: false,
                    target: Some(CardTarget { window: 0, session_id: "drag-fixture".into(), project: None }) }],
                dock_request: 1, close: done.load(Ordering::Relaxed), ..Frame::empty()
            }]).remove(0)
        }, |_, _| panic!("dragging or clicking a tab must not open an editor"),
            move || *cursor.lock().unwrap(), None, None));
        struct Cleanup(Arc<AtomicBool>, Option<std::thread::JoinHandle<io::Result<()>>>, Handle);
        impl Drop for Cleanup { fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
            if let Some(thread) = self.1.take() { thread.join().unwrap().unwrap(); }
            unsafe { SetThreadDpiAwarenessContext(self.2); }
        } }
        let _cleanup = Cleanup(stop, Some(thread), previous_dpi);
        let class = wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId()));
        let mut window = null_mut();
        wait_for(|| { window = FindWindowW(class.as_ptr(), null());
            !window.is_null() && SendMessageW(window, 0x80f0, 32, 0) != 0 });
        // Keep physical mouse input out of this owned-window fixture.
        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
        let bounds = || { let mut rect: Rect = zeroed(); assert_ne!(GetWindowRect(window, &mut rect), 0); rect };
        let send_at = |message, buttons, x, y| {
            let rect = bounds();
            let point = (((y - rect.top) as u16 as u32) << 16 | (x - rect.left) as u16 as u32) as isize;
            SendMessageW(window, message, buttons, point);
        };
        let original = bounds();
        let dpi = SendMessageW(window, 0x80f0, 2, 0) as u32;
        let x = (original.left + original.right) / 2;
        let y = (original.top + original.bottom) / 2;
        *pointer.lock().unwrap() = Some((x, y));
        let foreground = GetForegroundWindow();
        send_at(WM_MOUSEMOVE, 0, x, y); // A hover must leave time to grab the tab.
        send_at(WM_LBUTTONDOWN, 1, x, y);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(SendMessageW(window, 0x80f0, 3, 0), 1, "holding a tab must not hover-open it");
        send_at(WM_MOUSEMOVE, 1, x + 1, y - 1);
        assert_eq!(bounds(), original, "small click jitter must not move the tab");
        let target_y = y - scale_dip(180, dpi);
        *pointer.lock().unwrap() = Some((x, target_y));
        send_at(WM_MOUSEMOVE, 1, x - 100, target_y);
        wait_for(|| (bounds().top - (original.top - scale_dip(180, dpi))).abs() <= 1);
        send_at(WM_LBUTTONUP, 0, x, target_y);
        let moved = bounds();
        assert_eq!(moved.right, original.right, "horizontal movement must remain edge-snapped");
        assert_eq!(GetForegroundWindow(), foreground, "dragging must not take keyboard focus");
        std::thread::sleep(Duration::from_millis(500));
        send_at(WM_MOUSEMOVE, 0, x, target_y);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(SendMessageW(window, 0x80f0, 3, 0), 1, "dropping must not open the chat");
        assert_eq!(bounds(), moved, "feed refresh must preserve the dragged position");

        // A focused project or a temporarily replaced feed slot can disappear
        // entirely. Returning must restore the project anchor, not the corner.
        *pointer.lock().unwrap() = Some((-100_000, -100_000));
        shown.store(false, Ordering::Relaxed);
        wait_for(|| IsWindowVisible(window) == 0 && SendMessageW(window, 0x80f0, 1, 0) == 0);
        shown.store(true, Ordering::Relaxed);
        wait_for(|| IsWindowVisible(window) != 0 && SendMessageW(window, 0x80f0, 3, 0) == 1 && bounds() == moved);
        *pointer.lock().unwrap() = Some((x, target_y));

        // Capture loss cancels the gesture and the next click still works.
        send_at(WM_LBUTTONDOWN, 1, x, target_y);
        SendMessageW(window, 0x001f, 0, 0); // WM_CANCELMODE releases the actual native capture.
        send_at(WM_MOUSEMOVE, 1, x, target_y - 100);
        assert_eq!(bounds(), moved);
        send_at(WM_LBUTTONDOWN, 1, x, target_y);
        send_at(WM_LBUTTONUP, 0, x + 1, target_y + 1);
        wait_for(|| SendMessageW(window, 0x80f0, 25, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 1 && bounds() == moved);
        // Leaving and returning restores ordinary hover expansion.
        *pointer.lock().unwrap() = Some((-100_000, -100_000));
        std::thread::sleep(Duration::from_millis(100));
        *pointer.lock().unwrap() = Some((x, target_y));
        send_at(WM_MOUSEMOVE, 0, x, target_y);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 0);

        // The popped-out preview uses its existing stack anchor, even though
        // its layout no longer contains a folded tab rectangle.
        let drag_preview = |dy: i32, background: bool| {
            wait_for(|| SendMessageW(window, 0x80f0, 13, 0) != 0
                && SendMessageW(window, 0x80f0, 22, 0) == 0);
            std::thread::sleep(Duration::from_millis(350));
            let before = bounds();
            let point = SendMessageW(window, 0x80f0, 31, 0);
            assert_ne!(point, 0);
            let grab_x = before.left + point as i16 as i32;
            // The left background margin is clear of rows, input and buttons.
            let grab_x = if background { before.left + scale_dip(2, dpi) } else { grab_x };
            let grab_y = if background { (before.top + before.bottom) / 2 }
                else { before.top + (point >> 16) as i16 as i32 };
            let stage = SendMessageW(window, 0x80f0, 25, 0);
            let pinned = SendMessageW(window, 0x80f0, 33, 0);
            *pointer.lock().unwrap() = Some((grab_x, grab_y));
            send_at(WM_LBUTTONDOWN, 1, grab_x, grab_y);
            PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 1, 0);
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(SendMessageW(window, 0x80f0, 3, 0), 0, "a queued hover timeout cannot close a grabbed preview");
            send_at(WM_MOUSEMOVE, 1, grab_x + 1, grab_y - 1);
            assert_eq!(bounds(), before, "grabbing the expanded header must not jump to a different anchor");
            for delta in [dy / 2, dy] {
                *pointer.lock().unwrap() = Some((grab_x - 5, grab_y + delta));
                send_at(WM_MOUSEMOVE, 1, grab_x - 5, grab_y + delta);
                wait_for(|| (bounds().top - before.top - delta).abs() <= 1);
                assert_eq!(SendMessageW(window, 0x80f0, 22, 0), 0, "moving a message preview must not start a resize animation");
                assert_eq!(bounds().right, before.right, "expanded previews stay snapped to the same edge");
                assert_eq!(bounds().bottom - bounds().top, before.bottom - before.top);
            }
            send_at(WM_LBUTTONUP, 0, grab_x - 5, grab_y + dy);
            std::thread::sleep(Duration::from_millis(150));
            assert_eq!(SendMessageW(window, 0x80f0, 25, 0), stage, "dropping must not expand or maximize the preview");
            assert_eq!(SendMessageW(window, 0x80f0, 33, 0), pinned, "dragging preserves the existing pin state");
            assert_eq!(SendMessageW(window, 0x80f0, 34, 0), 0, "dropping must not queue a click-to-fold");
            assert_eq!(SendMessageW(window, 0x80f0, 35, 0), 0);
            bounds()
        };
        let preview = drag_preview(-scale_dip(100, dpi), false);
        *pointer.lock().unwrap() = Some((-100_000, -100_000));
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 1
            && bounds().right - bounds().left == original.right - original.left
            && (bounds().bottom - preview.bottom).abs() <= 1);
        let tab = bounds();
        let center = ((tab.left + tab.right) / 2, (tab.top + tab.bottom) / 2);
        *pointer.lock().unwrap() = Some(center);
        send_at(WM_LBUTTONDOWN, 1, center.0, center.1);
        send_at(WM_LBUTTONUP, 0, center.0, center.1);
        wait_for(|| SendMessageW(window, 0x80f0, 25, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        drag_preview(scale_dip(80, dpi), true);
        assert_eq!(SendMessageW(window, 0x80f0, 33, 0), 1);
        let header = SendMessageW(window, 0x80f0, 31, 0);
        SendMessageW(window, WM_LBUTTONDOWN, 1, header);
        SendMessageW(window, WM_LBUTTONUP, 0, header);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 1);
        std::thread::sleep(Duration::from_millis(350));
        let folded = bounds();
        let tab_point = SendMessageW(window, 0x80f0, 32, 0);
        assert_ne!(tab_point, 0);
        *pointer.lock().unwrap() = Some((-100_000, -100_000));
        SendMessageW(window, WM_LBUTTONDBLCLK, 1, tab_point);
        SendMessageW(window, WM_LBUTTONUP, 0, tab_point);
        wait_for(|| SendMessageW(window, 0x80f0, 25, 0) == 2 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 1 && bounds() == folded);
    }
}

#[test]
#[ignore = "opens an owned chat sidebar, temporarily reserves desktop space, then restores the desktop"]
fn native_sidebar_minimize_and_close_restore_desktop_space() {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        struct DpiReset(Handle);
        impl Drop for DpiReset { fn drop(&mut self) { unsafe { SetThreadDpiAwarenessContext(self.0); } } }
        let _dpi = DpiReset(previous_dpi);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || run_overlay_inner(Some(0), move |_| {
            crate::overlay::groups::group_frames(vec![Frame {
                session_id: Some("sidebar-fixture".into()), project_path: Some(r"C:\SidebarFixture".into()),
                cards: vec![Card { id: 1, label: "Sidebar fixture".into(), text: "Read this chat beside other applications.".into(),
                    final_message: false, attention: false,
                    target: Some(CardTarget { window: 0, session_id: "sidebar-fixture".into(), project: None }) }],
                dock_request: 1, close: done.load(Ordering::Relaxed), ..Frame::empty()
            }]).remove(0)
        }, |_, _| panic!("the sidebar fixture must not open an editor"), || None, None, None));
        struct Cleanup(Arc<AtomicBool>, Option<std::thread::JoinHandle<io::Result<()>>>);
        impl Drop for Cleanup { fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
            if let Some(thread) = self.1.take() { thread.join().unwrap().unwrap(); }
        } }
        let mut cleanup = Cleanup(stop, Some(thread));
        let class = wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId()));
        let mut window = null_mut();
        wait_for(|| { window = FindWindowW(class.as_ptr(), null());
            !window.is_null() && SendMessageW(window, 0x80f0, 32, 0) != 0 });
        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
        let display = MonitorFromWindow(window, 2);
        let work = || {
            let mut info: MonitorInfo = zeroed(); info.size = size_of::<MonitorInfo>() as u32;
            assert_ne!(GetMonitorInfoW(display, &mut info), 0); info.work
        };
        let original = work();
        let expected = workspace::sidebar_bounds(original);
        let _desktop = workspace::DesktopFixture::new(window);
        let expand = || {
            std::thread::sleep(Duration::from_millis(350));
            let point = SendMessageW(window, 0x80f0, 32, 0);
            assert_ne!(point, 0);
            SendMessageW(window, WM_LBUTTONDOWN, 1, point);
            SendMessageW(window, WM_LBUTTONUP, 0, point);
            SendMessageW(window, WM_LBUTTONDBLCLK, 1, point);
            SendMessageW(window, WM_LBUTTONUP, 0, point);
            wait_for(|| SendMessageW(window, 0x80f0, 25, 0) == 2 && SendMessageW(window, 0x80f0, 22, 0) == 0
                && work().right == expected.left);
            let mut bounds: Rect = zeroed(); GetWindowRect(window, &mut bounds);
            assert_eq!(bounds, expected, "full chat fills the reserved right third vertically");
        };
        expand();
        PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 0, 0);
        wait_for(|| work() == original && SendMessageW(window, 0x80f0, 3, 0) == 1
            && SendMessageW(window, 0x80f0, 22, 0) == 0);
        expand();
        restore_overlay_workspace(); // The daemon uses this before quitting or upgrading.
        wait_for(|| work() == original && SendMessageW(window, 0x80f0, 3, 0) == 1
            && SendMessageW(window, 0x80f0, 22, 0) == 0);
        expand();
        cleanup.0.store(true, Ordering::Relaxed);
        cleanup.1.take().unwrap().join().unwrap().unwrap();
        assert_eq!(work(), original, "closing an expanded overlay releases the desktop reservation");
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
    fn SendMessageW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn GetWindow(window: Hwnd, command: u32) -> Hwnd;
    fn IsWindowVisible(window: Hwnd) -> Bool;
    fn GetWindowTextW(window: Hwnd, text: *mut u16, count: i32) -> i32;
    fn PeekMessageW(message: *mut Message, window: Hwnd, first: u32, last: u32, remove: u32) -> Bool;
}

pub(super) unsafe fn nested_click(window: Hwnd, point: isize) {
    unsafe {
        SendMessageW(window, WM_LBUTTONDOWN, 1, point);
        SendMessageW(window, WM_LBUTTONUP, 0, point);
        let mut message: Message = zeroed();
        assert_ne!(PeekMessageW(&mut message, window, WM_APP_GROUP_ACTION, WM_APP_GROUP_ACTION, 1), 0);
        DispatchMessageW(&message); // Model a native focus/paint pump consuming the wake-up.
        PostMessageW(window, WM_FRAME_READY, 0, 0);
    }
}

#[track_caller]
fn wait_for(mut ready: impl FnMut() -> bool) {
    wait_for_seconds(5, &mut ready);
}

#[track_caller]
fn wait_for_seconds(seconds: u64, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "owned grouped overlay did not reach expected state"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "creates owned overlay and covering windows; never sends messages to Codex"]
fn native_overlay_recovers_topmost_in_tab_drawer_message_and_full_chat() {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        let cover = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            wide("STATIC").as_ptr(), wide("Owned covering window").as_ptr(), WS_POPUP,
            20, 20, 100, 100, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        assert!(!cover.is_null());
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || run_overlay_inner(Some(0), move |_| {
            crate::overlay::groups::group_frames(vec![Frame {
                session_id: Some("topmost-fixture".into()),
                project_path: Some(r"C:\TopmostFixture".into()),
                cards: vec![Card { id: 1, label: "Topmost fixture".into(),
                    text: "An unchanged reply must stay above other windows.".into(),
                    final_message: false, attention: false,
                    target: Some(CardTarget { window: 0, session_id: "topmost-fixture".into(), project: None }) }],
                dock_request: 1, close: done.load(Ordering::Relaxed), ..Frame::empty()
            }]).remove(0)
        }, |_, _| panic!("topmost repair must not open an editor"), || None, None, None));
        struct Cleanup { stop: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<io::Result<()>>>, cover: Hwnd, dpi: Handle }
        impl Drop for Cleanup { fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() { thread.join().unwrap().unwrap(); }
            unsafe { DestroyWindow(self.cover); SetThreadDpiAwarenessContext(self.dpi); }
        } }
        let _cleanup = Cleanup { stop, thread: Some(thread), cover, dpi: previous_dpi };
        let class = wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId()));
        let mut window = null_mut();
        wait_for(|| { window = FindWindowW(class.as_ptr(), null());
            !window.is_null() && IsWindowVisible(window) != 0 && SendMessageW(window, 0x80f0, 1, 0) == 1 });
        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
        let above = |upper, lower| {
            let mut cursor = GetWindow(lower, 3);
            while !cursor.is_null() { if cursor == upper { return true; } cursor = GetWindow(cursor, 3); }
            false
        };
        let bounds = |handle| { let mut rect: Rect = zeroed(); assert_ne!(GetWindowRect(handle, &mut rect), 0); rect };
        let check = || {
            let original = bounds(window);
            let foreground = GetForegroundWindow();
            assert_ne!(SetWindowPos(cover, -1isize as Hwnd, original.left, original.top,
                original.right-original.left, original.bottom-original.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOOWNERZORDER), 0);
            assert!(above(cover, window), "fixture must first cover the stationary overlay");
            PostMessageW(window, WM_FOREGROUND, cover as usize, super::super::overlay_window_events::test_tick() as isize);
            wait_for(|| above(window, cover));
            assert_eq!(bounds(window), original, "raising must not move or resize the overlay");
            assert_eq!(GetForegroundWindow(), foreground, "raising must not steal focus");
            assert_ne!(GetWindowLongPtrW(window, -20) & WS_EX_TOPMOST as isize, 0);
            let reply = FindWindowW(wide(format!("CodexLidGuardReply.{}", window as usize)).as_ptr(), null());
            if !reply.is_null() && IsWindowVisible(reply) != 0 {
                assert!(above(reply, window) && above(reply, cover), "composer must stay above its chat");
            }
            for part in ["panel", "tab"] {
                let blur = FindWindowW(wide(format!("CodexLidGuardBackdrop.{}.{}.{part}", GetCurrentProcessId(), window as usize)).as_ptr(), null());
                if !blur.is_null() && IsWindowVisible(blur) != 0 {
                    wait_for(|| above(window, blur) && above(blur, cover));
                }
            }
            // A normal Z-order request must not demote an overlay.
            assert_ne!(SetWindowPos(window, -2isize as Hwnd, 0, 0, 0, 0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER), 0);
            assert_ne!(GetWindowLongPtrW(window, -20) & WS_EX_TOPMOST as isize, 0);
            // Also recover a demotion which bypassed WINDOWPOSCHANGING.
            assert_ne!(SetWindowPos(window, -2isize as Hwnd, 0, 0, 0, 0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER | 0x0400), 0);
            PostMessageW(window, WM_FRAME_READY, 0, 0);
            wait_for(|| GetWindowLongPtrW(window, -20) & WS_EX_TOPMOST as isize != 0 && above(window, cover));
            if !reply.is_null() && IsWindowVisible(reply) != 0 {
                wait_for(|| GetWindowLongPtrW(reply, -20) & WS_EX_TOPMOST as isize != 0 && above(reply, window));
            }
            assert_eq!(GetForegroundWindow(), foreground);
        };
        std::thread::sleep(Duration::from_millis(350));
        check();
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 13, 0) != 0 && SendMessageW(window, 0x80f0, 3, 0) == 0);
        std::thread::sleep(Duration::from_millis(350));
        check();
        PostMessageW(window, WM_APP_REPLY_FOCUS, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 25, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        check();
        let input = SendMessageW(window, 0x80f0, 13, 0) as Hwnd;
        SendMessageW(input, 0x00c2, 1, wide("Unsent draft ".repeat(18)).as_ptr() as isize);
        wait_for(|| SendMessageW(window, 0x80f0, 25, 0) == 2 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        check();
        PostMessageW(window, WM_APP_CLOSE_CHAT, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        std::thread::sleep(Duration::from_millis(350));
        check();
    }
}

#[test]
#[ignore = "clicks only an owned overlay; never opens an editor or sends a chat message"]
fn native_click_toggles_pinned_expansion_and_double_click_maximizes_same_overlay() {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        struct DpiReset(Handle);
        impl Drop for DpiReset { fn drop(&mut self) { unsafe { SetThreadDpiAwarenessContext(self.0); } } }
        let _dpi = DpiReset(previous_dpi);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let pointer = Arc::new(std::sync::Mutex::new(None));
        let cursor = pointer.clone();
        let thread = std::thread::spawn(move || run_overlay_inner(Some(0), move |_| {
            crate::overlay::groups::group_frames((0..2).map(|id| Frame {
                session_id: Some(id.to_string()), project_path: Some(r"C:\ClickFixture".into()),
                cards: vec![Card { id: id + 1, label: format!("Click fixture {id}"),
                    text: "Finished reply".into(), final_message: true, attention: true,
                    target: Some(CardTarget { window: id + 1, session_id: id.to_string(), project: None }) }],
                dock_request: 1, close: done.load(Ordering::Relaxed), ..Frame::empty()
            }).collect()).remove(0)
        }, |_, _| panic!("clicking the tab or title must keep the chat in the overlay"),
            move || *cursor.lock().unwrap(), None, None));
        struct Cleanup(Arc<AtomicBool>, Option<std::thread::JoinHandle<io::Result<()>>>);
        impl Drop for Cleanup { fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
            if let Some(thread) = self.1.take() { thread.join().unwrap().unwrap(); }
        } }
        let _cleanup = Cleanup(stop, Some(thread));
        let class = wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId()));
        let mut window = null_mut();
        wait_for(|| { window = FindWindowW(class.as_ptr(), null());
            !window.is_null() && SendMessageW(window, 0x80f0, 32, 0) != 0 });
        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
        let point = |kind| { let value = SendMessageW(window, 0x80f0, kind, 0); assert_ne!(value, 0); value };
        let click = |at| { SendMessageW(window, WM_LBUTTONDOWN, 1, at); SendMessageW(window, WM_LBUTTONUP, 0, at); };
        let double = |at| { SendMessageW(window, WM_LBUTTONDBLCLK, 1, at); SendMessageW(window, WM_LBUTTONUP, 0, at); };
        let drawer = || SendMessageW(window, 0x80f0, 3, 0) == 0 && SendMessageW(window, 0x80f0, 13, 0) != 0;
        let full = || SendMessageW(window, 0x80f0, 25, 0) == 2 && SendMessageW(window, 0x80f0, 22, 0) == 0;
        let tucked = || SendMessageW(window, 0x80f0, 3, 0) == 1 && SendMessageW(window, 0x80f0, 32, 0) != 0
            && SendMessageW(window, 0x80f0, 22, 0) == 0;
        let fold = || {
            PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 0, 0);
            wait_for(tucked);
            std::thread::sleep(Duration::from_millis(350));
        };
        std::thread::sleep(Duration::from_millis(350));
        let message = || SendMessageW(window, 0x80f0, 25, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0;
        click(point(32)); // Click the completion pop-out itself.
        wait_for_seconds(1, message); // Must not wait for the five-second hover timer.
        let mut message_bounds: Rect = zeroed();
        assert_ne!(GetWindowRect(window, &mut message_bounds), 0);
        assert_eq!(message_bounds.right - message_bounds.left,
            scale_dip(group_window::PANEL_WIDTH, SendMessageW(window, 0x80f0, 2, 0) as u32),
            "click expansion preserves the drawer width");
        *pointer.lock().unwrap() = Some((-100_000, -100_000));
        PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 1, 0); // Hover timeout queued before the click.
        std::thread::sleep(Duration::from_millis(700));
        assert!(drawer(), "a clicked pop-out stays expanded after the pointer leaves");
        assert!(message(), "single-click shows the latest message without maximizing");
        assert_eq!(SendMessageW(window, 0x80f0, 33, 0), 1);
        click(point(31));
        wait_for_seconds(2, tucked);
        assert_eq!(SendMessageW(window, 0x80f0, 33, 0), 0, "the next click clears the pinned state");
        std::thread::sleep(Duration::from_millis(350));

        // Folding restores ordinary temporary hover behavior.
        *pointer.lock().unwrap() = None;
        PostMessageW(window, WM_MOUSEMOVE, 0, point(32));
        wait_for(drawer);
        *pointer.lock().unwrap() = Some((-100_000, -100_000));
        wait_for(tucked);
        std::thread::sleep(Duration::from_millis(350));

        // Hovered previews are temporary until their background is clicked.
        *pointer.lock().unwrap() = None;
        PostMessageW(window, WM_MOUSEMOVE, 0, point(32));
        wait_for(drawer);
        std::thread::sleep(Duration::from_millis(350));
        click(point(31));
        wait_for_seconds(1, message);
        *pointer.lock().unwrap() = Some((-100_000, -100_000));
        std::thread::sleep(Duration::from_millis(700));
        assert!(drawer(), "clicking preview background pins the hover expansion");
        let header = point(31);
        click(header); // A pending single-click fold must not win over a double-click.
        wait_for(|| SendMessageW(window, 0x80f0, 34, 0) == 1);
        double(header);
        wait_for(full);
        std::thread::sleep(Duration::from_millis(GetDoubleClickTime() as u64 + 100));
        assert!(full(), "double-click cancels the pending fold");
        assert_eq!(SendMessageW(window, 0x80f0, 17, 0), window as isize);
        fold();

        // A second click while the first expansion is animating must still maximize.
        let tab = point(32);
        click(tab);
        wait_for_seconds(1, || SendMessageW(window, 0x80f0, 22, 0) == 1);
        double(tab);
        wait_for_seconds(1, full);
        assert_eq!(SendMessageW(window, 0x80f0, 1, 0), 2, "double-click neither creates nor dismisses a session");
        click(point(31));
        wait_for_seconds(2, tucked);
        std::thread::sleep(Duration::from_millis(350));

        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 3, 0); // Keyboard preview has a three-second timeout.
        wait_for(drawer);
        std::thread::sleep(Duration::from_millis(350));
        click(point(4)); // Selecting a session in a compact preview expands it too.
        wait_for_seconds(1, message);
        assert_eq!(SendMessageW(window, 0x80f0, 0, 0), 2);
        std::thread::sleep(KEYBOARD_PREVIEW_DELAY + Duration::from_millis(200));
        assert!(drawer(), "clicking a session also pins a keyboard preview");
        click(point(15)); // Another session switches selection without folding.
        wait_for(|| SendMessageW(window, 0x80f0, 0, 0) == 1 && message());
        std::thread::sleep(Duration::from_millis(GetDoubleClickTime() as u64 + 100));
        assert!(drawer());
        click(point(15)); // Clicking that selected session again folds the overlay.
        wait_for_seconds(2, tucked);
        std::thread::sleep(Duration::from_millis(350));
        click(point(32));
        wait_for_seconds(1, message);
        click(point(4));
        wait_for(|| SendMessageW(window, 0x80f0, 0, 0) == 2 && message());
        double(point(4));
        wait_for(full);
        assert_eq!(SendMessageW(window, 0x80f0, 0, 0), 2, "double-click maximizes the selected session");
        assert_eq!(SendMessageW(window, 0x80f0, 17, 0), window as isize);
        fold();
    }
}

#[test]
#[ignore = "creates only fixture Codex workers and an owned overlay; never calls real Codex"]
fn native_new_chat_from_drawer_and_centered_overlay_preserves_other_drafts() {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        struct DpiReset(Handle);
        impl Drop for DpiReset { fn drop(&mut self) { unsafe { SetThreadDpiAwarenessContext(self.0); } } }
        let _dpi = DpiReset(previous_dpi);
        let cwd = std::env::temp_dir().join(format!("lidguard-new-chat-ui-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();
        let source = crate::background::integration_tests::start_overlay_fixture(&cwd);
        new_chat::TEST_DELAY_MS.store(500, Ordering::Relaxed);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || run_overlay_inner(Some(0), move |_| {
            let frames = crate::background::frames(&crate::model::GuardSettings::default());
            let mut frame = crate::overlay::groups::group_frames(frames).into_iter().next().unwrap_or_else(Frame::empty);
            frame.close = done.load(Ordering::Relaxed); frame
        }, |_, _| false.into(), || None, None, None));
        struct Cleanup { stop: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<io::Result<()>>>, cwd: std::path::PathBuf }
        impl Drop for Cleanup { fn drop(&mut self) {
            new_chat::TEST_DELAY_MS.store(0, Ordering::Relaxed);
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() { thread.join().unwrap().unwrap(); }
            crate::background::shutdown();
            let _ = std::fs::remove_file(self.cwd.join("fixture-child.pid"));
            let _ = std::fs::remove_dir(&self.cwd);
        } }
        let _cleanup = Cleanup { stop, thread: Some(thread), cwd };
        let class = wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId()));
        let mut window = null_mut();
        wait_for(|| { window = FindWindowW(class.as_ptr(), null()); !window.is_null() && SendMessageW(window, 0x80f0, 1, 0) == 1 });
        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 13, 0) != 0 && SendMessageW(window, 0x80f0, 3, 0) == 0);
        std::thread::sleep(Duration::from_millis(350));
        let input = SendMessageW(window, 0x80f0, 13, 0) as Hwnd;
        let read = || { let mut text = [0u16; 100]; let count = GetWindowTextW(input, text.as_mut_ptr(), 100); String::from_utf16_lossy(&text[..count as usize]) };
        let write = |text: &str| {
            SendMessageW(input, 0x00b1, 0, -1);
            SendMessageW(input, 0x00c2, 1, wide(text).as_ptr() as isize);
        };
        let click = |action| { let point = SendMessageW(window, 0x80f0, action, 0); assert_ne!(point, 0);
            SendMessageW(window, WM_LBUTTONDOWN, 1, point); SendMessageW(window, WM_LBUTTONUP, 0, point); };
        write("Keep my old draft");
        wait_for(|| SendMessageW(window, 0x80f0, 25, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        wait_for(|| crate::background::chat_snapshot(&source).is_some_and(|view| view.ready));
        crate::background::integration_tests::set_overlay_fixture_reply(&source, &"A long completed reply should stay readable before creating a new empty chat.\n\n".repeat(8));
        let source_messages = crate::background::chat_snapshot(&source).unwrap().messages;
        let dpi = SendMessageW(window, 0x80f0, 2, 0) as u32;
        wait_for(|| SendMessageW(window, 0x80f0, 27, 0) > scale_dip(300, dpi) as isize && SendMessageW(window, 0x80f0, 22, 0) == 0);
        let previous_height = SendMessageW(window, 0x80f0, 27, 0);
        click(19);
        wait_for(|| SendMessageW(window, 0x80f0, 14, 0) == 1);
        PostMessageW(window, WM_APP_SEND_REPLY, 0, 0);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(read(), "Keep my old draft", "Enter during creation must not submit the old draft");
        assert_eq!(crate::background::chat_snapshot(&source).unwrap().messages, source_messages);
        wait_for(|| SendMessageW(window, 0x80f0, 1, 0) == 2 && SendMessageW(window, 0x80f0, 25, 0) == 1);
        wait_for(|| SendMessageW(window, 0x80f0, 18, 0) == 0 && SendMessageW(window, 0x80f0, 20, 0) == 1);
        assert!(read().is_empty());
        assert!(SendMessageW(window, 0x80f0, 27, 0) < previous_height, "creating an empty chat must shrink the tall preview without closing it");
        assert_eq!(SendMessageW(window, 0x80f0, 17, 0), window as isize, "new chat stays inside the original overlay");
        write("Second chat draft with enough text to exceed the available single-line message box width and maximize the overlay.");
        wait_for(|| SendMessageW(window, 0x80f0, 16, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        write("Second chat draft");
        click(19);
        wait_for(|| SendMessageW(window, 0x80f0, 1, 0) == 3 && read().is_empty());
        assert_eq!(SendMessageW(window, 0x80f0, 16, 0), 1);
        let new_chat_point = SendMessageW(window, 0x80f0, 19, 0);
        SendMessageW(window, WM_LBUTTONDBLCLK, 1, new_chat_point);
        SendMessageW(window, WM_LBUTTONUP, 0, new_chat_point);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(SendMessageW(window, 0x80f0, 1, 0), 3, "a double-click must not create a duplicate session");
        if let Some(path) = std::env::var_os("LIDGUARD_TEST_SCREENSHOT") {
            crate::win::background_window::tests::capture(window, std::path::Path::new(&path));
        }
        click(23); wait_for(|| read() == "Keep my old draft");
        click(24); wait_for(|| read() == "Second chat draft");
        let activity = SendMessageW(window, 0x80f0, 21, 0) as u64;
        let selected = crate::background::frames(&crate::model::GuardSettings::default()).into_iter()
            .find(|frame| frame.activity == activity).unwrap().session_id.unwrap();
        assert_ne!(selected, source);
        write("wait-fixture my first message");
        SendMessageW(input, 0x0100, 13, 0);
        wait_for(|| read().is_empty() && crate::background::chat_snapshot(&selected).is_some_and(|view| view.turn_id.is_some()));
        wait_for(|| SendMessageW(window, 0x80f0, 21, 0) as u64 == crate::background::chat_snapshot(&selected).unwrap().activity
            && SendMessageW(window, 0x80f0, 22, 0) == 0);
        assert_eq!(crate::background::chat_snapshot(&selected).unwrap().messages[0].text, "wait-fixture my first message");
        assert_eq!(crate::background::chat_snapshot(&source).unwrap().messages, source_messages);
        assert!(crate::background::frames(&crate::model::GuardSettings::default()).iter().all(|frame| frame.window.is_none()), "new chats never open a separate task window");


    }
}

#[test]
#[ignore = "uses one owned fixture worker and overlay; never opens a real editor or answers real requests"]
fn native_background_open_stays_in_overlay_and_routes_saved_history() {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        struct DpiReset(Handle);
        impl Drop for DpiReset { fn drop(&mut self) { unsafe { SetThreadDpiAwarenessContext(self.0); } } }
        let _dpi = DpiReset(previous_dpi);
        let cwd = std::env::temp_dir().join(format!("lidguard-background-open-ui-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();
        let source = crate::background::integration_tests::start_overlay_fixture(&cwd);
        let shortcuts = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
        let publisher = shortcuts.publisher(0);
        let (opened, requests) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false)); let done = stop.clone();
        let thread = std::thread::spawn(move || run_overlay_inner(Some(0), move |_| {
            let frames = crate::background::frames(&crate::model::GuardSettings::default());
            let mut frame = crate::overlay::groups::group_frames(frames).into_iter().next().unwrap_or_else(Frame::empty);
            frame.close = done.load(Ordering::Relaxed); frame
        }, move |target, _| { opened.send(target.clone()).unwrap(); false.into() }, || None, Some(publisher), None));
        struct Cleanup { stop: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<io::Result<()>>>, cwd: std::path::PathBuf }
        impl Drop for Cleanup { fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() { thread.join().unwrap().unwrap(); }
            crate::background::shutdown();
            let _ = std::fs::remove_file(self.cwd.join("fixture-child.pid")); let _ = std::fs::remove_dir(&self.cwd);
        } }
        let cleanup = Cleanup { stop, thread: Some(thread), cwd };
        let class = wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId()));
        let mut window = null_mut();
        wait_for(|| { window = FindWindowW(class.as_ptr(), null()); !window.is_null() && shortcuts.test_binding(0).is_some()
            && crate::background::chat_snapshot(&source).unwrap().ready });
        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
        // The keyboard open shortcut must work even before a composer exists.
        PostMessageW(window, WM_OVERLAY_SHORTCUT, shortcuts.test_binding(0).unwrap().1, 1);
        wait_for(|| SendMessageW(window, 0x80f0, 16, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        std::thread::sleep(Duration::from_millis(400));
        let input = SendMessageW(window, 0x80f0, 13, 0) as Hwnd;
        let read = || { let mut text = [0u16; 100]; let count = GetWindowTextW(input, text.as_mut_ptr(), 100); String::from_utf16_lossy(&text[..count as usize]) };
        let write = |text: &str| { SendMessageW(input, 0x00b1, 0, -1); SendMessageW(input, 0x00c2, 1, wide(text).as_ptr() as isize); };
        let click = |action| { let point = SendMessageW(window, 0x80f0, action, 0); assert_ne!(point, 0);
            SendMessageW(window, WM_LBUTTONDOWN, 1, point); SendMessageW(window, WM_LBUTTONUP, 0, point); };
        write("Keep my draft");
        crate::background::integration_tests::set_overlay_fixture_request(&source, crate::background::PendingInput {
            id: serde_json::json!("fixture-approval"), method: "item/commandExecution/requestApproval".into(),
            params: serde_json::json!({"availableDecisions":["accept","cancel"]}), details: "Allow this fixture command once?\n\nCommand: fixture only".into() });
        wait_for(|| SendMessageW(window, 0x80f0, 29, 0) != 0);
        PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 0 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        std::thread::sleep(Duration::from_millis(400));
        click(5);
        wait_for(|| SendMessageW(window, 0x80f0, 16, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0 && SendMessageW(window, 0x80f0, 29, 0) != 0);
        std::thread::sleep(Duration::from_millis(400));
        let row = SendMessageW(window, 0x80f0, 23, 0);
        SendMessageW(window, WM_LBUTTONDBLCLK, 1, row); SendMessageW(window, WM_LBUTTONUP, 0, row);
        std::thread::sleep(Duration::from_millis(400));
        wait_for(|| SendMessageW(window, 0x80f0, 22, 0) == 0);
        assert!(requests.try_recv().is_err(), "Open chat, keyboard open and double-click must stay in this overlay");
        assert_eq!(read(), "Keep my draft");
        if let Some(path) = std::env::var_os("LIDGUARD_TEST_SCREENSHOT") { crate::win::background_window::tests::capture(window, std::path::Path::new(&path)); }
        assert!(crate::background::answer(&source, &serde_json::json!("old-request"), serde_json::json!({"decision":"accept"})).is_err());
        click(30); wait_for(|| crate::background::chat_snapshot(&source).unwrap().pending.is_empty());
        assert_eq!(read(), "Keep my draft");
        crate::background::integration_tests::set_overlay_fixture_request(&source, crate::background::PendingInput {
            id: serde_json::json!("fixture-question"), method: "item/tool/requestUserInput".into(),
            params: serde_json::json!({"questions":[{"id":"one","question":"First question?"},{"id":"two","question":"Second question?"}]}), details: String::new() });
        std::thread::sleep(Duration::from_millis(600));
        write("First answer"); SendMessageW(input, 0x0100, 13, 0); wait_for(|| read().is_empty());
        assert_eq!(crate::background::chat_snapshot(&source).unwrap().pending.len(), 1);
        write("Second answer"); SendMessageW(input, 0x0100, 13, 0);
        wait_for(|| read().is_empty() && crate::background::chat_snapshot(&source).unwrap().pending.is_empty());
        wait_for(|| SendMessageW(window, 0x80f0, 18, 0) == 0 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        let mut client: Rect = zeroed(); GetClientRect(window, &mut client);
        let editor_point = SendMessageW(window, 0x80f0, 28, 0);
        assert!((editor_point >> 16) < client.bottom as isize, "Editor button below client: y={}, height={}", editor_point >> 16, client.bottom);
        SendMessageW(window, 0x80f2, 0, editor_point);
        let target = requests.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(target.session_id, "11111111-1111-1111-1111-111111111111");
        assert_eq!(target.project.unwrap().cwd, cleanup.cwd.to_string_lossy());
        assert!(crate::background::frames(&crate::model::GuardSettings::default()).iter().all(|frame| frame.window.is_none()));
    }
}

#[test]
#[ignore = "displays one owned project drawer; sends messages only to that test window"]
fn native_group_select_open_dismiss_and_fold_preserve_sibling_sessions() {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        struct DpiReset(Handle);
        impl Drop for DpiReset {
            fn drop(&mut self) {
                unsafe {
                    SetThreadDpiAwarenessContext(self.0);
                }
            }
        }
        let _dpi = DpiReset(previous_dpi);
        let shortcuts = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
        let publisher = shortcuts.publisher(0);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let scenario = Arc::new(AtomicU8::new(0));
        let feed_scenario = scenario.clone();
        let (opened, received) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            run_overlay_inner(
                Some(0),
                move |_| {
                    let phase = feed_scenario.load(Ordering::Relaxed);
                    let frames = (0..2)
                        .map(|i| Frame {
                            session_id: Some(i.to_string()),
                            project_path: Some(r"C:\OverlayTest".into()),
                            activity: i,
                            cards: vec![Card {
                                // Independent feeds can issue the same card ID.
                                id: u64::from(phase == 4),
                                label: format!("OverlayTest — Session {i}"),
                                text: "Native grouping test".into(),
                                final_message: i == 0 && matches!(phase, 1 | 3 | 4),
                                attention: i == 0 && matches!(phase, 1 | 4),
                                target: Some(CardTarget {
                                    window: i + 1,
                                    session_id: i.to_string(),
                                    project: Some(crate::session_navigation::Project {
                                        cwd: r"C:\LidGuard-Test".into(), path: r"C:\LidGuard-Test".into(),
                                        executable: r"C:\LidGuard-Test\Code.exe".into(),
                                    }),
                                }),
                            }],
                            dock_request: 1,
                            busy: i != 0 || phase == 0,
                            needs_input: i == 0 && phase == 2,
                            close: done.load(Ordering::Relaxed),
                            ..Frame::empty()
                        })
                        .collect();
                    crate::overlay::groups::group_frames(frames).remove(0)
                },
                move |target, _| {
                    opened.send(target.session_id.clone()).unwrap();
                    false.into()
                },
                || None,
                Some(publisher),
                None,
            )
        });
        struct Cleanup(
            Arc<AtomicBool>,
            Option<std::thread::JoinHandle<io::Result<()>>>,
        );
        impl Drop for Cleanup {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
                if let Some(thread) = self.1.take() {
                    thread.join().unwrap().unwrap();
                }
            }
        }
        let _cleanup = Cleanup(stop, Some(thread));
        let class = wide(format!(
            "CodexLidGuardMessageOverlay.{}",
            GetCurrentProcessId()
        ));
        let mut window = null_mut();
        wait_for(|| {
            window = FindWindowW(class.as_ptr(), null());
            !window.is_null() && SendMessageW(window, 0x80f0, 1, 0) == 2
        });
        let foreground = GetForegroundWindow();
        // Real pointer movement must not hover this message-driven fixture.
        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
        wait_for(|| shortcuts.test_binding(0).is_some());
        let dpi = SendMessageW(window, 0x80f0, 2, 0) as u32;
        let tab_width = || { let mut rect: Rect = zeroed(); GetWindowRect(window, &mut rect); rect.right-rect.left };
        wait_for(|| tab_width() == scale_dip(18,dpi));
        for key in [0x5b, 0xa0, 0x86] {
            shortcuts.test_key(key, true);
        }
        wait_for(|| SendMessageW(window, 0x80f0, 10, 0) == 1);
        wait_for(|| tab_width() == scale_dip(44,dpi));
        assert_eq!(
            SendMessageW(window, 0x80f0, 3, 0),
            1,
            "highlighting shortcut codes must not expand a tab"
        );
        for key in [0x86, 0xa0, 0x5b] {
            shortcuts.test_key(key, false);
        }
        wait_for(|| SendMessageW(window, 0x80f0, 10, 0) == 0);
        wait_for(|| tab_width() == scale_dip(18,dpi));
        scenario.store(1,Ordering::Relaxed);
        PostMessageW(window,WM_FRAME_READY,0,0);
        wait_for(|| tab_width() == scale_dip(152,dpi));
        std::thread::sleep(Duration::from_millis(8500));
        assert_eq!(tab_width(),scale_dip(152,dpi),"completion must remain popped out beyond the old timeout");
        assert_eq!(SendMessageW(window,0x80f0,12,0),1,"completion must retain the unread bead");
        let point = ((scale_dip(12, dpi) as u32) << 16 | scale_dip(12, dpi) as u32) as isize;
        SendMessageW(window, WM_MOUSEMOVE, 0, point);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 0);
        let preview_started = Instant::now();
        wait_for_seconds(8, || SendMessageW(window, 0x80f0, 25, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        assert!(preview_started.elapsed() >= Duration::from_millis(4700), "hover must wait five seconds before fitting the reply");
        assert_eq!(SendMessageW(window, 0x80f0, 16, 0), 0, "reading must not maximize or take focus");
        assert_ne!(GetForegroundWindow(), window);
        assert_eq!(SendMessageW(window, 0x80f0, 20, 0), 0, "timed expansion must not focus the input");
        assert_eq!(tab_width(), scale_dip(group_window::MESSAGE_WIDTH, dpi));
        let history = wide(format!("You\r\nAn older prompt\r\n\r\nCodex\r\nAn older answer\r\n\r\nCodex\r\n{}", "The latest reply should be fully visible, including this final paragraph.\n\n".repeat(5)));
        SendMessageW(window, 0x80f1, history.len() - 1, history.as_ptr() as isize);
        wait_for(|| SendMessageW(window, 0x80f0, 26, 0) > scale_dip(160, dpi) as isize
            && SendMessageW(window, 0x80f0, 26, 0) <= SendMessageW(window, 0x80f0, 27, 0)
            && SendMessageW(window, 0x80f0, 22, 0) == 0);
        assert_eq!(tab_width(), scale_dip(group_window::PANEL_WIDTH, dpi), "reading the last message must only grow the height");
        let previous_height = SendMessageW(window, 0x80f0, 27, 0);
        let shorter = wide("Codex\r\nA shorter update.");
        SendMessageW(window, 0x80f1, shorter.len()-1, shorter.as_ptr() as isize);
        wait_for(|| SendMessageW(window, 0x80f0, 27, 0) < previous_height && SendMessageW(window, 0x80f0, 22, 0) == 0);
        assert_eq!(tab_width(), scale_dip(group_window::PANEL_WIDTH, dpi), "a shorter reply must keep the overlay alive at the original width");
        PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 0, 0);
        wait_for(|| tab_width() == scale_dip(18, dpi) && SendMessageW(window, 0x80f0, 3, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        std::thread::sleep(Duration::from_millis(350));
        assert_eq!(tab_width(), scale_dip(18, dpi), "the same completion must stay tucked after hover");
        assert_eq!(SendMessageW(window, 0x80f0, 12, 0), 1, "previewing must preserve the unread marker");
        scenario.store(4, Ordering::Relaxed);
        PostMessageW(window, WM_FRAME_READY, 0, 0);
        wait_for(|| tab_width() == scale_dip(152, dpi));
        let (_, token) = shortcuts.test_binding(0).unwrap();
        PostMessageW(window, WM_OVERLAY_SHORTCUT, token, 5); // Released-key preview.
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 1 && tab_width() == scale_dip(18, dpi));
        assert_eq!(SendMessageW(window, 0x80f0, 12, 0), 1, "keyboard preview must preserve the unread marker");
        scenario.store(2,Ordering::Relaxed);
        PostMessageW(window,WM_FRAME_READY,0,0);
        wait_for(|| tab_width() == scale_dip(152,dpi));
        scenario.store(3,Ordering::Relaxed);
        PostMessageW(window,WM_FRAME_READY,0,0);
        wait_for(|| tab_width() == scale_dip(18,dpi));
        assert_eq!(SendMessageW(window,0x80f0,12,0),0,"reading the result clears its halo");
        scenario.store(0,Ordering::Relaxed);
        PostMessageW(window,WM_FRAME_READY,0,0);
        let (_, token) = shortcuts.test_binding(0).unwrap();
        PostMessageW(window, WM_OVERLAY_SHORTCUT, token, 0); // Letter-selected preview stays open.
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 0);
        wait_for_seconds(8, || SendMessageW(window, 0x80f0, 25, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        assert_eq!(SendMessageW(window, 0x80f0, 16, 0), 0);
        PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 0, 0);
        wait_for(|| tab_width() == scale_dip(18, dpi) && SendMessageW(window, 0x80f0, 3, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        std::thread::sleep(Duration::from_millis(300));
        let click = |kind| {
            let point = SendMessageW(window, 0x80f0, kind, 0);
            assert_ne!(point, 0, "missing owned action {kind}");
            SendMessageW(window, WM_LBUTTONDOWN, 1, point);
            SendMessageW(window, WM_LBUTTONUP, 0, point);
        };
        wait_for(|| SendMessageW(window, 0x80f0, 13, 0) != 0);
        let input = SendMessageW(window, 0x80f0, 13, 0) as Hwnd;
        let read_input = || {
            let mut text = [0u16; 100];
            let length = GetWindowTextW(input, text.as_mut_ptr(), 100);
            String::from_utf16_lossy(&text[..length as usize])
        };
        wait_for(|| shortcuts.test_typing(0).is_some());
        for down in [true, false] { shortcuts.test_hover_key(b'H' as u32, down, false); }
        for key in [0x5b, 0xa0, 0x86] { shortcuts.test_hover_key(key, true, true); }
        for key in [0x86, 0xa0, 0x5b] { shortcuts.test_hover_key(key, false, true); }
        // Even a tapped Copilot macro owns its next letter while hovering.
        let (code, _) = shortcuts.test_binding(0).unwrap();
        for down in [true, false] { shortcuts.test_hover_key(code[0] as u32, down, true); }
        let cancel = (b'A'..=b'Z').find(|key| *key != code[1]).unwrap();
        for down in [true, false] { shortcuts.test_hover_key(cancel as u32, down, true); }
        std::thread::sleep(Duration::from_millis(200));
        assert!(read_input().is_empty(), "Copilot shortcuts and non-hover typing must not enter the draft");
        assert_eq!(SendMessageW(window, 0x80f0, 16, 0), 0, "Copilot shortcuts must not maximize the chat");
        for key in *b"HI" {
            for down in [true, false] { shortcuts.test_hover_key(key as u32, down, true); }
        }
        let typing_started = Instant::now();
        wait_for(|| read_input() == "hi" && SendMessageW(window, 0x80f0, 25, 0) == 1);
        wait_for(|| SendMessageW(window, 0x80f0, 18, 0) == 0);
        assert_eq!(SendMessageW(window, 0x80f0, 16, 0), 0, "first keystrokes should only fit the latest reply");
        assert_eq!(GetForegroundWindow() as usize, shortcuts.test_typing(0).unwrap().surface,
            "hover typing must move keyboard focus to the composer");
        wait_for_seconds(8, || SendMessageW(window, 0x80f0, 16, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        assert!(typing_started.elapsed() >= Duration::from_millis(4700), "maximize has its own typing deadline");
        PostMessageW(input, WM_KEYDOWN, 27, 0);
        wait_for(|| tab_width() == scale_dip(18, dpi) && SendMessageW(window, 0x80f0, 3, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        wait_for(|| tab_width() == scale_dip(group_window::PANEL_WIDTH, dpi) && shortcuts.test_typing(0).is_some());
        SendMessageW(input, 0x000c, 0, wide("").as_ptr() as isize);
        for character in "A draft \u{1f30d}".encode_utf16() { SendMessageW(input, 0x0102, character as usize, 0); }
        assert_eq!(read_input(), "A draft \u{1f30d}");
        SendMessageW(input, 0x0100, 13, 0); // Enter sends only the selected chat.
        wait_for(|| SendMessageW(window, 0x80f0, 14, 0) == 1);
        assert_eq!(read_input(), "A draft \u{1f30d}", "a rejected send keeps the draft");
        wait_for(|| SendMessageW(window, 0x80f0, 22, 0) == 0);
        assert!(received.try_recv().is_err(), "Send must not open a chat");
        let old_typing = shortcuts.test_typing(0).unwrap();
        click(4);
        wait_for(|| SendMessageW(window, 0x80f0, 0, 0) == 2);
        wait_for(|| read_input().is_empty());
        SendMessageW(old_typing.surface as Hwnd, super::super::overlay_shortcuts::WM_HOVER_TEXT,
            old_typing.token, b'x' as isize);
        assert!(read_input().is_empty(), "queued hover input must not leak into a replacement session");
        wait_for(|| SendMessageW(window, 0x80f0, 22, 0) == 0);
        for character in "Second draft".encode_utf16() { SendMessageW(input, 0x0102, character as usize, 0); }
        click(9); // The icon uses the same send path as Enter.
        wait_for(|| SendMessageW(window, 0x80f0, 14, 0) == 1);
        assert_eq!(read_input(), "Second draft");
        click(15);
        wait_for(|| read_input() == "A draft \u{1f30d}");
        click(4);
        wait_for(|| read_input() == "Second draft");
        // Overflow grows the same message preview into the centered conversation.
        let mut initial_bounds: Rect = zeroed(); GetWindowRect(window, &mut initial_bounds);
        let (display, _) = overlay_display(None, window).unwrap();
        let expected = workspace::sidebar_bounds(display.work);
        let mut compact_input: Rect = zeroed(); GetWindowRect(input, &mut compact_input);
        SendMessageW(input, 0x00b1, 0, -1);
        SendMessageW(input, 0x00c2, 1, wide(format!("Second draft{}", " long".repeat(30))).as_ptr() as isize);
        if SendMessageW(window, 0x80f0, 11, 0) != 0 {
            wait_for(|| SendMessageW(window, 0x80f0, 18, 0) != 0);
            SendMessageW(input, 0x000c, 0, wide("Second draft").as_ptr() as isize);
            let mut frozen_input: Rect = zeroed(); GetWindowRect(input, &mut frozen_input);
            assert_eq!(frozen_input, compact_input, "cached growth keeps the native edit stationary");
            SendMessageW(input, 0x00b1, 12, 12);
            SendMessageW(input, 0x0102, b'?' as usize, 0);
            assert_eq!(read_input(), "Second draft?");
            SendMessageW(input, 0x0102, 8, 0);
            assert_eq!(read_input(), "Second draft");
        }
        else { SendMessageW(input, 0x000c, 0, wide("Second draft").as_ptr() as isize); }
        let mut chat_bounds: Rect = zeroed();
        let mut growth_frames = Vec::new();
        wait_for(|| {
            GetWindowRect(window, &mut chat_bounds);
            if growth_frames.last() != Some(&chat_bounds) { growth_frames.push(chat_bounds); }
            chat_bounds == expected
        });
        if SendMessageW(window, 0x80f0, 11, 0) != 0 {
            assert!(growth_frames.iter().filter(|frame| **frame != initial_bounds && **frame != expected).count() >= 1,
                "expansion from {initial_bounds:?} must show intermediate sizes: {growth_frames:?}");
            assert!(growth_frames.windows(2).all(|pair| pair[1].right-pair[1].left >= pair[0].right-pair[0].left));
        }
        let chat = SendMessageW(window, 0x80f0, 17, 0) as Hwnd;
        assert_eq!(chat, window, "full conversation belongs to the original overlay");
        assert_eq!(SendMessageW(window, 0x80f0, 16, 0), 1);
        wait_for(|| SendMessageW(window, 0x80f0, 18, 0) == 0);
        assert_eq!(GetWindowLongPtrW(window, -16) & 0x00cf_0000, 0, "no title bar or native frame is added");
        let mut input_bounds: Rect = zeroed(); GetWindowRect(input, &mut input_bounds);
        assert!(input_bounds.left >= chat_bounds.left && input_bounds.right < chat_bounds.right);
        assert!(input_bounds.top > chat_bounds.top && input_bounds.bottom < chat_bounds.bottom);
        click(15); wait_for(|| read_input() == "A draft \u{1f30d}");
        click(4); wait_for(|| read_input() == "Second draft");
        assert_eq!(SendMessageW(window, 0x80f0, 16, 0), 1, "session switching keeps the overlay centered");
        let history = wide("You\r\nWhat needs to happen for it to run?\r\n\r\nCodex\r\nIt needs to complete **three steps**:\n\n1. **Check the collected data** has enough usable minutes.\n2. **Train a model** and pass validation.\n3. **Activate that model** automatically.\r\n\r\nCodex\r\nRun `cargo test` to verify the changes. The next check runs in **15 minutes**.\r\n\r\nYou\r\n[Image]\r\n\r\nCodex\r\nThe screenshot confirms **1,896 minutes collected**. Training can start once the data passes validation.");
        SendMessageW(window, 0x80f1, history.len() - 1, history.as_ptr() as isize);
        std::thread::sleep(Duration::from_millis(300));
        if let Some(path) = std::env::var_os("LIDGUARD_TEST_SCREENSHOT") {
            crate::win::background_window::tests::capture(window, std::path::Path::new(&path));
        }
        assert_eq!(read_input(), "Second draft");
        // Keep composing at the end after the actual focus click.
        SendMessageW(input, 0x00b1, 12, 12);
        for icon in [true, false] {
            SendMessageW(input, 0x0102, b'!' as usize, 0);
            wait_for(|| SendMessageW(window, 0x80f0, 14, 0) == 0);
            if icon { click(9); }
            else { SendMessageW(input, WM_KEYDOWN, 13, 0); }
            wait_for(|| SendMessageW(window, 0x80f0, 14, 0) == 1);
        }
        assert_eq!(read_input(), "Second draft!!", "both centered controls submit and restore a rejected draft");
        PostMessageW(input, WM_KEYDOWN, 27, 0);
        let mut fold_frames = Vec::new();
        wait_for(|| {
            let mut bounds: Rect = zeroed(); GetWindowRect(window, &mut bounds);
            if fold_frames.last() != Some(&bounds) { fold_frames.push(bounds); }
            bounds.right-bounds.left == scale_dip(18,dpi) && SendMessageW(window, 0x80f0, 16, 0) == 0
        });
        if SendMessageW(window, 0x80f0, 11, 0) != 0 {
            assert!(fold_frames.iter().filter(|frame| frame.right-frame.left > scale_dip(18,dpi)
                && frame.right-frame.left < expected.right-expected.left).count() >= 1,
                "Escape must shrink continuously back to the tab: {fold_frames:?}");
        }
        wait_for(|| tab_width() == scale_dip(18,dpi));
        assert_eq!(read_input(), "Second draft!!", "minimizing preserves the draft");
        println!("Expansion widths: {:?}; Escape widths: {:?}",
            growth_frames.iter().map(|r| r.right-r.left).collect::<Vec<_>>(),
            fold_frames.iter().map(|r| r.right-r.left).collect::<Vec<_>>());
        if SendMessageW(window, 0x80f0, 11, 0) != 0 {
            PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
            wait_for(|| tab_width() == scale_dip(group_window::PANEL_WIDTH,dpi));
            std::thread::sleep(Duration::from_millis(100));
            PostMessageW(input, WM_LBUTTONDOWN, 1, 0);
            PostMessageW(input, WM_LBUTTONUP, 0, 0);
            wait_for(|| SendMessageW(window, 0x80f0, 18, 0) != 0);
            PostMessageW(input, WM_KEYDOWN, 27, 0);
            wait_for(|| tab_width() == scale_dip(18,dpi) && SendMessageW(window, 0x80f0, 3, 0) == 1 && SendMessageW(window, 0x80f0, 22, 0) == 0);
            assert_eq!(read_input(), "Second draft!!", "Escape during growth preserves the same session draft");
        }
        SetForegroundWindow(foreground);
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 0);
        std::thread::sleep(Duration::from_millis(300));
        click(5);
        assert_eq!(received.recv_timeout(Duration::from_secs(3)).unwrap(), "1");
        // The callback intentionally reports failure: no real editor is opened.
        std::thread::sleep(Duration::from_millis(300));
        wait_for(|| SendMessageW(window, 0x80f0, 1, 0) == 2);
        // Escape dismisses exactly the keyboard-selected session, not its project.
        let (code, _) = shortcuts.test_binding(0).unwrap();
        for key in [0x5b, 0xa0, 0x86] {
            shortcuts.test_key(key, true);
        }
        shortcuts.test_key(code[0] as u32, true);
        shortcuts.test_key(code[0] as u32, false);
        shortcuts.test_key(0x1b, true);
        shortcuts.test_key(0x1b, false);
        for key in [0x86, 0xa0, 0x5b] {
            shortcuts.test_key(key, false);
        }
        wait_for(|| SendMessageW(window, 0x80f0, 1, 0) == 1);
        assert_eq!(
            SendMessageW(window, 0x80f0, 0, 0),
            1,
            "dismissal must retain the sibling session"
        );
        click(8);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 1);
        let dpi = SendMessageW(window, 0x80f0, 2, 0) as u32;
        wait_for(|| {
            let mut rect: Rect = zeroed();
            GetWindowRect(window, &mut rect);
            rect.right - rect.left == scale_dip(group_window::tab_state::CALM_WIDTH, dpi)
        });
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 0);
        std::thread::sleep(Duration::from_millis(300));
        click(7); // The direct Dismiss action removes only the remaining session.
        wait_for(|| SendMessageW(window, 0x80f0, 1, 0) == 0 && IsWindowVisible(window) == 0);
        assert_eq!(
            GetForegroundWindow(),
            foreground,
            "preview controls must not steal keyboard focus"
        );
    }
}

#[test]
#[ignore = "displays two owned project drawers; input and visibility changes affect only test windows"]
fn native_group_tabs_slide_clear_before_expansion_without_activating() {
    for position in ["bottom-right", "top-right"] {
        native_group_stack_case(position, false);
    }
}

#[test]
#[ignore = "expands only owned fixture overlays; never opens an editor or sends chat messages"]
fn native_message_previews_keep_neighboring_tabs_visible_through_growth_and_fold() {
    for position in ["bottom-right", "top-right"] {
        native_group_stack_case(position, true);
    }
}

fn native_group_stack_case(position: &'static str, fit_message: bool) {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        let stop = Arc::new(AtomicBool::new(false));
        let show_second = Arc::new(AtomicBool::new(true));
        let session_count = Arc::new(std::sync::atomic::AtomicUsize::new(1));
        let reply_length = Arc::new(std::sync::atomic::AtomicUsize::new(1));
        let pointer = Arc::new(std::sync::Mutex::new(None));
        struct Cleanup {
            stop: Arc<AtomicBool>,
            threads: Vec<std::thread::JoinHandle<io::Result<()>>>,
            previous_dpi: Handle,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::Relaxed);
                for thread in self.threads.drain(..) {
                    thread.join().unwrap().unwrap();
                }
                unsafe {
                    SetThreadDpiAwarenessContext(self.previous_dpi);
                }
            }
        }
        let mut cleanup = Cleanup {
            stop: stop.clone(),
            threads: vec![],
            previous_dpi,
        };
        for slot in 0..2 {
            let stop = stop.clone();
            let show_second = show_second.clone();
            let pointer = pointer.clone();
            let session_count = session_count.clone();
            let reply_length = reply_length.clone();
            cleanup.threads.push(std::thread::spawn(move || {
                run_overlay_inner(
                    Some(slot),
                    move |_| {
                        let close = stop.load(Ordering::Relaxed);
                        if slot == 1 && !show_second.load(Ordering::Relaxed) {
                            return Frame {
                                close,
                                ..Frame::empty()
                            };
                        }
                        crate::overlay::groups::group_frames(
                            (0..session_count.load(Ordering::Relaxed))
                                .map(|task| Frame {
                                    session_id: Some(format!("stacking-{slot}-{task}")),
                                    project_path: Some(format!(r"C:\StackingTest{slot}")),
                                    cards: vec![Card {
                                        id: 0,
                                        label: format!("StackingTest{slot} — Task"),
                                        text: "Expanded project must leave its sibling tab visible.\n\n"
                                            .repeat(reply_length.load(Ordering::Relaxed)),
                                        final_message: false,
                                        attention: false,
                                        target: fit_message.then(|| CardTarget { window: 0,
                                            session_id: format!("stacking-{slot}-{task}"), project: None }),
                                    }],
                                    dock_request: 1,
                                    busy: true,
                                    max_tabs: 2,
                                    position: position.into(),
                                    close,
                                    ..Frame::empty()
                                })
                                .collect(),
                        )
                        .remove(0)
                    },
                    |_, _| panic!("expansion must not open a chat"),
                    move || *pointer.lock().unwrap(),
                    None,
                    None,
                )
            }));
        }
        let windows: [Hwnd; 2] = std::array::from_fn(|slot| {
            let class = wide(format!(
                "CodexLidGuardMessageOverlay.{}{}",
                GetCurrentProcessId(),
                if slot == 0 {
                    String::new()
                } else {
                    format!(".{slot}")
                }
            ));
            let mut window = null_mut();
            wait_for(|| {
                window = FindWindowW(class.as_ptr(), null());
                !window.is_null()
                    && IsWindowVisible(window) != 0
                    && SendMessageW(window, 0x80f0, 3, 0) == 1
            });
            window
        });
        let above = |upper: Hwnd, lower: Hwnd| {
            let mut current = GetWindow(lower, 3); // GW_HWNDPREV walks toward the top.
            while !current.is_null() {
                if current == upper {
                    return true;
                }
                current = GetWindow(current, 3);
            }
            false
        };
        std::thread::sleep(Duration::from_millis(350));
        let bounds = |window| {
            let mut rect: Rect = zeroed();
            assert_ne!(GetWindowRect(window, &mut rect), 0);
            rect
        };
        let original = windows.map(bounds);
        let backdrops = windows.map(|window| {
            ["panel", "tab"].map(|part| {
                let class = wide(format!(
                    "CodexLidGuardBackdrop.{}.{window_id}.{part}",
                    GetCurrentProcessId(),
                    window_id = window as usize
                ));
                FindWindowW(class.as_ptr(), null())
            })
        });
        let check_backdrop = |slot: usize, expanded: bool| {
            let [panel, tab] = backdrops[slot];
            if panel.is_null() && tab.is_null() {
                return;
            } // Tint fallback on older Windows.
            assert!(!panel.is_null() && !tab.is_null());
            let (shown, hidden) = if expanded { (panel, tab) } else { (tab, panel) };
            wait_for(|| {
                IsWindowVisible(shown) != 0
                    && IsWindowVisible(hidden) == 0
                    && bounds(shown) == bounds(windows[slot])
            });
            assert!(
                above(windows[slot], shown),
                "blur must stay beneath sharp content"
            );
            for handle in [panel, tab] {
                assert_ne!(
                    GetWindowLongPtrW(handle, -20) & WS_EX_NOACTIVATE as isize,
                    0
                );
            }
        };
        for slot in 0..2 {
            check_backdrop(slot, false);
        }
        let assert_separate = || {
            assert!(
                !windows.contains(&GetForegroundWindow()),
                "moving overlays must never take keyboard focus"
            );
            assert!(
                !backdrops
                    .iter()
                    .flatten()
                    .any(|&window| !window.is_null() && window == GetForegroundWindow()),
                "blur surfaces must never take keyboard focus"
            );
            let [a, b] = windows.map(bounds);
            assert!(
                a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top,
                "project windows overlap: {a:?}, {b:?}"
            );
        };
        assert_separate();
        let dpi = SendMessageW(windows[0], 0x80f0, 2, 0) as u32;
        if fit_message {
            let watch = |ready: &mut dyn FnMut() -> bool| {
                let deadline = Instant::now() + Duration::from_secs(6);
                loop {
                    assert_separate();
                    for window in windows { assert_ne!(IsWindowVisible(window), 0, "neighboring tabs must remain visible"); }
                    if ready() { break; }
                    assert!(Instant::now() < deadline, "message preview did not settle");
                    std::thread::sleep(Duration::from_millis(5));
                }
            };
            for (slot, &window) in windows.iter().enumerate() {
                let point = SendMessageW(window, 0x80f0, 32, 0);
                assert_ne!(point, 0);
                SendMessageW(window, WM_LBUTTONDOWN, 1, point);
                SendMessageW(window, WM_LBUTTONUP, 0, point);
                watch(&mut || SendMessageW(window, 0x80f0, 25, 0) == 1
                    && SendMessageW(window, 0x80f0, 22, 0) == 0
                    && bounds(window).right - bounds(window).left == scale_dip(group_window::PANEL_WIDTH, dpi));
                let short = bounds(window);
                reply_length.store(80, Ordering::Relaxed);
                PostMessageW(window, WM_FRAME_READY, 0, 0);
                watch(&mut || bounds(window).bottom - bounds(window).top > short.bottom - short.top + scale_dip(100, dpi)
                    && SendMessageW(window, 0x80f0, 22, 0) == 0);
                let tall = bounds(window);
                assert_eq!(tall.right - tall.left, scale_dip(group_window::PANEL_WIDTH, dpi));
                check_backdrop(slot, true);
                reply_length.store(1, Ordering::Relaxed);
                PostMessageW(window, WM_FRAME_READY, 0, 0);
                watch(&mut || bounds(window).bottom - bounds(window).top < tall.bottom - tall.top
                    && SendMessageW(window, 0x80f0, 22, 0) == 0);
            }
            PostMessageW(windows[1], WM_APP_COLLAPSE_OVERLAY, 0, 0);
            watch(&mut || windows.map(bounds) == original
                && windows.iter().all(|&window| SendMessageW(window, 0x80f0, 3, 0) == 1));
            return;
        }
        let animated = SendMessageW(windows[0], 0x80f0, 11, 0) != 0;
        let displacement = (scale_dip(136, dpi) + 1 - scale_dip(group_window::TAB_HEIGHT, dpi))
            * if position.starts_with("top") { 1 } else { -1 };
        let displaced = Rect {
            top: original[1].top + displacement,
            bottom: original[1].bottom + displacement,
            ..original[1]
        };
        for expand in [true, false] {
            PostMessageW(
                windows[0],
                if expand {
                    WM_APP_EXPAND_OVERLAY
                } else {
                    WM_APP_COLLAPSE_OVERLAY
                },
                0,
                0,
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut intermediate = Vec::new();
            loop {
                let own = bounds(windows[0]);
                let sibling = bounds(windows[1]);
                assert_separate();
                if sibling != original[1]
                    && sibling != displaced
                    && !intermediate.contains(&sibling)
                {
                    intermediate.push(sibling);
                }
                let compact = own.right - own.left == scale_dip(group_window::tab_state::CALM_WIDTH, dpi)
                    && own.bottom - own.top == scale_dip(group_window::TAB_HEIGHT, dpi);
                if expand && !compact {
                    assert_eq!(
                        sibling, displaced,
                        "drawer must wait until the neighboring tab finishes sliding clear"
                    );
                }
                if !expand && sibling != displaced {
                    assert!(
                        compact,
                        "the neighboring tab must wait for the drawer to finish folding"
                    );
                }
                if (expand
                    && own.right - own.left == scale_dip(group_window::PANEL_WIDTH, dpi)
                    && sibling == displaced)
                    || (!expand && compact && sibling == original[1])
                {
                    break;
                }
                assert!(Instant::now() < deadline, "tab slide did not settle");
                std::thread::sleep(Duration::from_millis(5));
            }
            if animated {
                assert!(
                    intermediate.len() >= 3,
                    "neighbor must visibly slide, not jump; observed {} intermediate positions",
                    intermediate.len()
                );
            }
            check_backdrop(0, expand);
        }
        // Start with whichever tab is behind: initial arrival order is asynchronous.
        let behind = if above(windows[0], windows[1]) { 1 } else { 0 };
        // Click, hover, held-key and released-key previews share the same rule.
        for (slot, mode) in [
            (behind, 0),
            (1 - behind, 0),
            (behind, 1),
            (1 - behind, 2),
            (behind, 3),
        ] {
            let mut tab: Rect = zeroed();
            GetWindowRect(windows[slot], &mut tab);
            *pointer.lock().unwrap() = Some((tab.left + 12, tab.top + 12));
            PostMessageW(windows[slot], WM_APP_EXPAND_OVERLAY, mode, 0);
            wait_for(|| SendMessageW(windows[slot], 0x80f0, 3, 0) == 0);
            std::thread::sleep(Duration::from_millis(30));
            let until = Instant::now() + Duration::from_millis(550);
            while Instant::now() < until {
                assert_separate();
                assert!(
                    above(windows[slot], windows[1 - slot]),
                    "expanded project {slot} must be above its sibling throughout the slide (mode {mode})"
                );
                PostMessageW(windows[1 - slot], WM_FRAME_READY, 0, 0);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        // A sibling disappearing and returning must not cover an open drawer.
        PostMessageW(windows[0], WM_APP_EXPAND_OVERLAY, 0, 0);
        show_second.store(false, Ordering::Relaxed);
        PostMessageW(windows[1], WM_FRAME_READY, 0, 0);
        wait_for(|| IsWindowVisible(windows[1]) == 0);
        for handle in backdrops[1] {
            if !handle.is_null() {
                assert_eq!(
                    IsWindowVisible(handle),
                    0,
                    "hiding the tab must hide its blur"
                );
            }
        }
        show_second.store(true, Ordering::Relaxed);
        PostMessageW(windows[1], WM_FRAME_READY, 0, 0);
        wait_for(|| IsWindowVisible(windows[1]) != 0);
        let until = Instant::now() + Duration::from_millis(350);
        while Instant::now() < until {
            assert_separate();
            assert!(
                above(windows[0], windows[1]),
                "returning tabs must stay below the expanded project"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_ne!(
            bounds(windows[1]),
            original[1],
            "the neighboring tab must move out of the drawer's way"
        );
        // New tasks can grow an already-open panel; removing them must not bring
        // tabs back into space that is still being painted by the old panel.
        for count in [4, 1] {
            session_count.store(count, Ordering::Relaxed);
            for window in windows {
                PostMessageW(window, WM_FRAME_READY, 0, 0);
            }
            let until = Instant::now() + Duration::from_millis(450);
            while Instant::now() < until {
                assert_separate();
                std::thread::sleep(Duration::from_millis(5));
            }
            let dpi = SendMessageW(windows[0], 0x80f0, 2, 0) as u32;
            let rect = bounds(windows[0]);
            assert_eq!(
                rect.bottom - rect.top,
                scale_dip(if count == 4 { 220 } else { 136 }, dpi) + 1
            );
        }
        *pointer.lock().unwrap() = None;
        for window in windows {
            PostMessageW(window, WM_APP_COLLAPSE_OVERLAY, 0, 0);
        }
        let until = Instant::now() + Duration::from_millis(400);
        while Instant::now() < until {
            assert_separate();
            std::thread::sleep(Duration::from_millis(5));
        }
        wait_for(|| windows.map(bounds) == original);
        for slot in 0..2 {
            check_backdrop(slot, false);
        }
        assert!(
            !windows.contains(&GetForegroundWindow()),
            "changing overlay stacking must not steal focus"
        );
        drop(cleanup);
        for handle in backdrops.into_iter().flatten() {
            if !handle.is_null() {
                assert_eq!(
                    IsWindow(handle),
                    0,
                    "closing overlays must remove their blur surfaces"
                );
            }
        }
    }
}
