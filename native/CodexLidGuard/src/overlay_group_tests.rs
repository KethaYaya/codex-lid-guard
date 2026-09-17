//! Exercise actual window input using messages sent only to an owned test HWND.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
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

#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
    fn SendMessageW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn GetWindow(window: Hwnd, command: u32) -> Hwnd;
    fn IsWindowVisible(window: Hwnd) -> Bool;
}

fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "owned grouped overlay did not reach expected state"
        );
        std::thread::sleep(Duration::from_millis(20));
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
        let (opened, received) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            run_overlay_inner(
                Some(0),
                move |_| {
                    let frames = (0..2)
                        .map(|i| Frame {
                            session_id: Some(i.to_string()),
                            project_path: Some(r"C:\OverlayTest".into()),
                            activity: i,
                            cards: vec![Card {
                                // Independent feeds can issue the same card ID.
                                id: 0,
                                label: format!("OverlayTest — Session {i}"),
                                text: "Native grouping test".into(),
                                final_message: false,
                                attention: false,
                                target: Some(CardTarget {
                                    window: i + 1,
                                    session_id: i.to_string(),
                                    project: None,
                                }),
                            }],
                            dock_request: 1,
                            busy: true,
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
        wait_for(|| shortcuts.test_binding(0).is_some());
        for key in [0x5b, 0xa0, 0x86] {
            shortcuts.test_key(key, true);
        }
        wait_for(|| SendMessageW(window, 0x80f0, 10, 0) == 1);
        assert_eq!(
            SendMessageW(window, 0x80f0, 3, 0),
            1,
            "highlighting shortcut codes must not expand a tab"
        );
        for key in [0x86, 0xa0, 0x5b] {
            shortcuts.test_key(key, false);
        }
        wait_for(|| SendMessageW(window, 0x80f0, 10, 0) == 0);
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        wait_for(|| SendMessageW(window, 0x80f0, 3, 0) == 0);
        std::thread::sleep(Duration::from_millis(300));
        let click = |kind| {
            let point = SendMessageW(window, 0x80f0, kind, 0);
            assert_ne!(point, 0, "missing owned action {kind}");
            SendMessageW(window, WM_LBUTTONDOWN, 1, point);
            SendMessageW(window, WM_LBUTTONUP, 0, point);
        };
        click(4);
        wait_for(|| SendMessageW(window, 0x80f0, 0, 0) == 2);
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
            rect.right - rect.left == scale_dip(group_window::TAB_WIDTH, dpi)
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
        native_group_stack_case(position);
    }
}

fn native_group_stack_case(position: &'static str) {
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(-4isize as Handle);
        let stop = Arc::new(AtomicBool::new(false));
        let show_second = Arc::new(AtomicBool::new(true));
        let session_count = Arc::new(std::sync::atomic::AtomicUsize::new(1));
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
                                        text: "Expanded project must stay above its sibling tab."
                                            .into(),
                                        final_message: false,
                                        attention: false,
                                        target: None,
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
        let assert_separate = || {
            assert!(
                !windows.contains(&GetForegroundWindow()),
                "moving overlays must never take keyboard focus"
            );
            let [a, b] = windows.map(bounds);
            assert!(
                a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top,
                "project windows overlap: {a:?}, {b:?}"
            );
        };
        assert_separate();
        let dpi = SendMessageW(windows[0], 0x80f0, 2, 0) as u32;
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
                let compact = own.right - own.left == scale_dip(group_window::TAB_WIDTH, dpi)
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
        assert!(
            !windows.contains(&GetForegroundWindow()),
            "changing overlay stacking must not steal focus"
        );
    }
}
