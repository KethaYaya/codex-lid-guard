use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class: *const u16, name: *const u16) -> Hwnd;
    fn IsWindowVisible(window: Hwnd) -> Bool;
}

#[test]
#[ignore = "displays owned overlays; keyboard events go only to the simulated shortcut thread"]
fn native_copilot_tab_cycles_previews_and_enter_opens_the_exact_chat() {
    run_cycle_test(3, false);
}

#[test]
#[ignore = "displays ten owned overlays; simulated keys never reach desktop apps"]
fn native_ten_tabs_use_custom_keys_and_apply_lower_limits_live() {
    run_cycle_test(10, true);
}

#[test]
#[ignore = "displays one owned overlay; simulated keys never reach desktop apps"]
fn native_cycle_release_folds_after_three_seconds_and_expanded_busy_dots_animate() {
    #[link(name = "gdi32")]
    unsafe extern "system" { fn GetPixel(dc: Handle, x: i32, y: i32) -> u32; }
    let service = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
    let stop = Arc::new(AtomicBool::new(false));
    let colors = Arc::new(Mutex::new(std::collections::HashSet::new()));
    let worker_stop = stop.clone();
    let worker_colors = colors.clone();
    let publisher = service.publisher(0);
    let worker = thread::spawn(move || {
        run_overlay_inner(Some(0), |_| {
            // Sample the rendered bitmap on its owning UI thread, with unchanged cards.
            unsafe {
                let window = FindWindowW(wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId())).as_ptr(), null());
                if !window.is_null() {
                    let state = &*(GetWindowLongPtrW(window, GWLP_USERDATA) as *const OverlayState);
                    if !state.collapsed && let Some(layout) = state.layout && layout.tab.is_none()
                        && let Some(panel) = layout.panel && !state.buffer.dc.is_null()
                        && (panel.right - panel.left, panel.bottom - panel.top) == state.panel_size {
                        let pixels: [u32; 3] = std::array::from_fn(|dot| {
                            let rect = header_busy_dot(panel, dot, state.dpi);
                            GetPixel(state.buffer.dc, rect.left, rect.top)
                        });
                        for pixel in pixels {
                            assert_ne!(pixel, 0xffffffff);
                            assert!((pixel & 0xff) > ((pixel >> 16) & 0xff) + 10,
                                "all three busy dots must be amber in the expanded header: {pixel:08x}");
                        }
                        worker_colors.lock().unwrap().insert(pixels);
                    }
                }
            }
            Frame {
                session_id: Some("release-test".into()),
                cards: vec![Card { id: 1, label: "Test project — Busy preview".into(),
                    text: "Working: this unchanged preview should keep animating its busy indicator.".into(),
                    final_message: false, attention: false,
                    target: Some(CardTarget { project: None, window: 100, session_id: "release-test".into() }) }],
                busy: true, dock_request: 1, close: worker_stop.load(Ordering::Relaxed),
                ..Frame::empty()
            }
        }, |_, _| panic!("cycling must never open a chat"), || None, Some(publisher), None).unwrap();
    });
    let result = std::panic::catch_unwind(|| unsafe {
        SetThreadDpiAwarenessContext(-4isize as Handle);
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.test_binding(0).is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let window = FindWindowW(wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId())).as_ptr(), null());
        let width = || {
            let mut rect: Rect = zeroed();
            assert_ne!(GetWindowRect(window, &mut rect), 0);
            rect.right - rect.left
        };
        let prefix = |down| { for key in [0x5b, 0xa0, 0x86] { service.test_key(key, down); } };
        let cycle = || { service.test_key(0x09, true); service.test_key(0x09, false); };
        thread::sleep(Duration::from_millis(350));
        let tab_width = width();
        prefix(true);
        cycle();
        thread::sleep(Duration::from_millis(3400));
        assert!(width() > tab_width, "holding Copilot must not start the countdown");
        let released = Instant::now();
        prefix(false);
        thread::sleep(Duration::from_millis(2750));
        assert!(width() > tab_width, "release must allow the full three seconds");
        while width() > tab_width {
            assert!(released.elapsed() < Duration::from_millis(3650), "released preview did not fold");
            thread::sleep(Duration::from_millis(5));
        }
        eprintln!("Copilot release to folded tab, including slide: {:?}", released.elapsed());
        assert_ne!(IsWindowVisible(window), 0, "folding must preserve the tab");

        // A released macro prefix still arms a cycle; another Copilot+Tab restarts its timer.
        prefix(true);
        prefix(false);
        cycle();
        thread::sleep(Duration::from_millis(1000));
        prefix(true);
        prefix(false);
        cycle();
        let cycled = Instant::now();
        thread::sleep(Duration::from_millis(2350));
        assert!(width() > tab_width, "a later cycle must replace the previous countdown");
        // Holding Copilot again suspends the pending countdown until the next release.
        prefix(true);
        thread::sleep(Duration::from_millis(1100));
        assert!(width() > tab_width, "re-pressing the prefix must keep the preview open");
        assert!(cycled.elapsed() > Duration::from_secs(3));
        prefix(false);
        // A manual tab expansion pins the panel and cancels any keyboard timer.
        PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
        thread::sleep(Duration::from_millis(3400));
        assert!(width() > tab_width, "manual expansion must cancel the keyboard countdown");
        assert_ne!(window, GetForegroundWindow());
    });
    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    if let Err(cause) = result { std::panic::resume_unwind(cause); }
    let mut animate = 1;
    unsafe { SystemParametersInfoW(0x1042, 0, (&mut animate as *mut Bool).cast(), 0); }
    let samples = colors.lock().unwrap().len();
    assert!(samples >= if animate != 0 { 3 } else { 1 }, "busy dots must render and follow Windows animation preferences");
    eprintln!("Expanded busy indicator pixel patterns: {samples}");
}

fn run_cycle_test(count: usize, custom: bool) {
    let service = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
    let stop = Arc::new(AtomicBool::new(false));
    let limit = Arc::new(std::sync::atomic::AtomicUsize::new(count));
    let disabled = Arc::new(AtomicBool::new(false));
    let mapping = Arc::new(AtomicBool::new(custom));
    let prefix_keys = if custom { [0xa2, 0xa4, 0x20] } else { [0x5b, 0xa0, 0x86] };
    let cycle_key = if custom { 0x28 } else { 0x09 };
    let open_key = if custom { 0x27 } else { 0x0d };
    let (opened, activations) = mpsc::channel();
    let mut threads = Vec::new();
    for slot in 0..count {
        let publisher = service.publisher(slot);
        let stop = stop.clone();
        let limit = limit.clone();
        let disabled = disabled.clone();
        let mapping = mapping.clone();
        let opened = opened.clone();
        threads.push(thread::spawn(move || {
            run_overlay_inner(
                Some(slot),
                |_| { let custom = mapping.load(Ordering::Relaxed); Frame {
                    session_id: Some(format!("cycle-{slot}")),
                    max_tabs: limit.load(Ordering::Relaxed),
                    shortcuts: crate::shortcut_config::ShortcutConfig::from_settings(&crate::shortcut_config::ShortcutSettings {
                        enabled: !disabled.load(Ordering::Relaxed),
                        prefix: if custom { "Ctrl+Alt+Space" } else { "Copilot" }.into(),
                        cycle_key: if custom { "Down" } else { "Tab" }.into(),
                        open_key: if custom { "Right" } else { "Enter" }.into(),
                        close_key: if custom { "Delete" } else { "Escape" }.into(),
                    }),
                    cards: if slot < limit.load(Ordering::Relaxed) { vec![Card {
                        id: slot as u64,
                        label: format!("Project - Chat {slot}"),
                        text: "Cycle previews without opening a chat until Enter is pressed."
                            .into(),
                        final_message: true,
                        attention: true,
                        target: Some(CardTarget { project: None,
                            window: 100,
                            session_id: format!("cycle-{slot}"),
                        }),
                    }] } else { vec![] },
                    attention: true,
                    dock_request: 1,
                    close: stop.load(Ordering::Relaxed),
                    ..Frame::empty()
                } },
                |target, _| {
                    opened.send(target.clone()).unwrap();
                    true.into()
                },
                || None,
                Some(publisher),
                None,
            )
            .unwrap()
        }));
    }
    let result = std::panic::catch_unwind(|| unsafe {
        SetThreadDpiAwarenessContext(-4isize as Handle);
        let deadline = Instant::now() + Duration::from_secs(3);
        while (0..count).any(|slot| service.test_binding(slot).is_none()) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let windows: Vec<_> = (0..count)
            .map(|slot| {
                FindWindowW(
                    wide(format!(
                        "CodexLidGuardMessageOverlay.{}{}",
                        GetCurrentProcessId(),
                        if slot == 0 {
                            String::new()
                        } else {
                            format!(".{slot}")
                        }
                    ))
                    .as_ptr(),
                    null(),
                )
            })
            .collect();
        assert!(windows.iter().all(|window| !window.is_null()));
        let bounds = |window| {
            let mut rect: Rect = zeroed();
            assert_ne!(GetWindowRect(window, &mut rect), 0);
            rect
        };
        thread::sleep(Duration::from_millis(350));
        let tabs: Vec<_> = windows.iter().map(|window| bounds(*window)).collect();
        for key in [0x09, 0x0d] {
            service.test_key(key, true);
            service.test_key(key, false);
        }
        thread::sleep(Duration::from_millis(50));
        assert_eq!(
            windows
                .iter()
                .map(|window| bounds(*window))
                .collect::<Vec<_>>(),
            tabs
        );
        assert!(activations.try_recv().is_err());
        for key in prefix_keys {
            service.test_key(key, true);
        }
        for selected in (0..count).chain([0, 1]) {
            let started = Instant::now();
            service.test_key(cycle_key, true);
            service.test_key(cycle_key, false);
            while bounds(windows[selected]).right - bounds(windows[selected]).left
                <= tabs[selected].right - tabs[selected].left
            {
                assert!(
                    started.elapsed() < Duration::from_millis(200),
                    "cycling waited for a feed read"
                );
                thread::sleep(Duration::from_millis(2));
            }
            eprintln!("Cycle expansion response: {:?}", started.elapsed());
            thread::sleep(Duration::from_millis(320));
            for slot in 0..count {
                let rect = bounds(windows[slot]);
                assert_ne!(
                    IsWindowVisible(windows[slot]),
                    0,
                    "cycling must retain every tab"
                );
                if slot == selected {
                    assert!(rect.right - rect.left > tabs[slot].right - tabs[slot].left);
                } else {
                    assert_eq!(
                        rect.right - rect.left,
                        tabs[slot].right - tabs[slot].left,
                        "the previous preview should tuck back into its tab"
                    );
                }
                assert_eq!(
                    rect.right, tabs[slot].right,
                    "keep the slide flush with the display edge"
                );
            }
            assert!(
                activations.try_recv().is_err(),
                "cycling must not open a chat"
            );
            // The user may switch apps during this test. The overlays themselves
            // must never take foreground focus while cycling.
            assert!(!windows.contains(&GetForegroundWindow()));
        }
        if !custom {
            // The user's sequence: Copilot+Tab, release both keys, then Tab alone.
            for key in prefix_keys.into_iter().rev() { service.test_key(key, false); }
            let started = Instant::now();
            service.test_key(0x09, true);
            service.test_key(0x09, false);
            while bounds(windows[1]).right - bounds(windows[1]).left > tabs[1].right - tabs[1].left {
                assert!(started.elapsed() < Duration::from_millis(650), "plain Tab must fold immediately instead of waiting three seconds");
                thread::sleep(Duration::from_millis(5));
            }
            eprintln!("Plain Tab to folded preview, including slide: {:?}", started.elapsed());
            for slot in 0..count {
                assert_ne!(IsWindowVisible(windows[slot]), 0, "minimizing keeps every tab available");
                let rect = bounds(windows[slot]);
                assert_eq!(rect.right - rect.left, tabs[slot].right - tabs[slot].left,
                    "Tab alone must not expand another preview");
            }
            service.test_key(0x09, true);
            service.test_key(0x09, false);
            thread::sleep(Duration::from_millis(100));
            assert_eq!(bounds(windows[1]).right - bounds(windows[1]).left, tabs[1].right - tabs[1].left);
            assert!(activations.try_recv().is_err());
            // Re-select this exact chat with its letter to verify Enter still opens it.
            for key in prefix_keys { service.test_key(key, true); }
            let letter = service.test_binding(1).unwrap().0[0] as u32;
            service.test_key(letter, true);
            service.test_key(letter, false);
            thread::sleep(Duration::from_millis(350));
        }
        // Enter follows the same activation path as double-click, with the exact session ID.
        let entered = Instant::now();
        service.test_key(open_key, true);
        assert_eq!(
            activations
                .recv_timeout(Duration::from_millis(200))
                .unwrap(),
            CardTarget { project: None,
                window: 100,
                session_id: "cycle-1".into()
            }
        );
        eprintln!("Enter activation dispatch: {:?}", entered.elapsed());
        service.test_key(open_key, true); // Auto-repeat cannot open twice.
        for key in [open_key, prefix_keys[2], prefix_keys[1], prefix_keys[0]] {
            service.test_key(key, false);
        }
        thread::sleep(Duration::from_millis(400));
        assert_eq!(IsWindowVisible(windows[1]), 0);
        assert_ne!(IsWindowVisible(windows[0]), 0);
        assert_ne!(IsWindowVisible(windows[2]), 0);
        assert!(activations.try_recv().is_err());
        limit.store(1, Ordering::Relaxed);
        disabled.store(true, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(650));
        assert_ne!(IsWindowVisible(windows[0]), 0);
        assert!(windows[1..].iter().all(|window| IsWindowVisible(*window) == 0));
        assert!((0..count).all(|slot| service.test_binding(slot).is_none()));
        // Re-enable with another prefix while the same window and chat survive.
        mapping.store(!custom, Ordering::Relaxed);
        disabled.store(false, Ordering::Relaxed);
        let deadline = Instant::now() + Duration::from_secs(2);
        while service.test_binding(0).is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let before = bounds(windows[0]);
        for key in prefix_keys { service.test_key(key, true); }
        service.test_key(cycle_key, true);
        service.test_key(cycle_key, false);
        for key in prefix_keys.into_iter().rev() { service.test_key(key, false); }
        thread::sleep(Duration::from_millis(100));
        assert_eq!(bounds(windows[0]), before, "the previous shortcut must no longer expand the tab");
        let next_prefix = if custom { [0x5b, 0xa0, 0x86] } else { [0xa2, 0xa4, 0x20] };
        let next_cycle = if custom { 0x09 } else { 0x28 };
        let next_close = if custom { 0x1b } else { 0x2e };
        for key in next_prefix { service.test_key(key, true); }
        service.test_key(next_cycle, true);
        service.test_key(next_cycle, false);
        thread::sleep(Duration::from_millis(350));
        let after = bounds(windows[0]);
        assert!(after.right - after.left > before.right - before.left);
        service.test_key(next_close, true);
        service.test_key(next_close, false);
        for key in next_prefix.into_iter().rev() { service.test_key(key, false); }
        thread::sleep(Duration::from_millis(400));
        assert_eq!(IsWindowVisible(windows[0]), 0, "the remapped close key dismisses the surviving tab");

    });
    stop.store(true, Ordering::Relaxed);
    for thread in threads {
        thread.join().unwrap();
    }
    if let Err(cause) = result {
        std::panic::resume_unwind(cause);
    }
}
