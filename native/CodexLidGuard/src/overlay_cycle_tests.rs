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
    let service = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
    let stop = Arc::new(AtomicBool::new(false));
    let (opened, activations) = mpsc::channel();
    let mut threads = Vec::new();
    for slot in 0..3 {
        let publisher = service.publisher(slot);
        let stop = stop.clone();
        let opened = opened.clone();
        threads.push(thread::spawn(move || {
            run_overlay_inner(
                Some(slot),
                |_| Frame {
                    session_id: Some(format!("cycle-{slot}")),
                    cards: vec![Card {
                        id: slot as u64,
                        label: format!("Project — {}", ["Build", "Review", "Deploy"][slot]),
                        text: "Cycle previews without opening a chat until Enter is pressed."
                            .into(),
                        final_message: true,
                        attention: true,
                        target: Some(CardTarget {
                            window: 100,
                            session_id: format!("cycle-{slot}"),
                        }),
                    }],
                    attention: true,
                    dock_request: 1,
                    close: stop.load(Ordering::Relaxed),
                    ..Frame::empty()
                },
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
        let foreground = GetForegroundWindow();
        let deadline = Instant::now() + Duration::from_secs(3);
        while (0..3).any(|slot| service.test_binding(slot).is_none()) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let windows: Vec<_> = (0..3)
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
        for key in [0x5b, 0xa0, 0x86] {
            service.test_key(key, true);
        }
        for selected in [0, 1, 2, 0, 1] {
            let started = Instant::now();
            service.test_key(0x09, true);
            service.test_key(0x09, false);
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
            for slot in 0..3 {
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
            assert_eq!(GetForegroundWindow(), foreground);
        }
        // Enter follows the same activation path as double-click, with the exact session ID.
        let entered = Instant::now();
        service.test_key(0x0d, true);
        assert_eq!(
            activations
                .recv_timeout(Duration::from_millis(200))
                .unwrap(),
            CardTarget {
                window: 100,
                session_id: "cycle-1".into()
            }
        );
        eprintln!("Enter activation dispatch: {:?}", entered.elapsed());
        service.test_key(0x0d, true); // Auto-repeat cannot open twice.
        for key in [0x0d, 0x86, 0xa0, 0x5b] {
            service.test_key(key, false);
        }
        thread::sleep(Duration::from_millis(400));
        assert_eq!(IsWindowVisible(windows[1]), 0);
        assert_ne!(IsWindowVisible(windows[0]), 0);
        assert_ne!(IsWindowVisible(windows[2]), 0);
        assert!(activations.try_recv().is_err());
    });
    stop.store(true, Ordering::Relaxed);
    for thread in threads {
        thread.join().unwrap();
    }
    if let Err(cause) = result {
        std::panic::resume_unwind(cause);
    }
}
