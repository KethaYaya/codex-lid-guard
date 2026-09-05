use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class: *const u16, name: *const u16) -> Hwnd;
    fn IsWindowVisible(window: Hwnd) -> Bool;
}

#[test]
#[ignore = "displays owned overlays; keyboard events go only to the simulated shortcut thread"]
fn native_close_button_and_escape_dismiss_only_the_selected_tab() {
    let service = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
    let stop = Arc::new(AtomicBool::new(false));
    let activity = Arc::new(AtomicU64::new(1));
    let (closed, closures) = mpsc::channel();
    let (opened, activations) = mpsc::channel();
    let mut threads = Vec::new();
    let mut wake_receivers = Vec::new();
    for slot in 0..2 {
        let (wake, receiver) = mpsc::sync_channel(1);
        wake_receivers.push(receiver);
        let updates = OverlayUpdates::new(Arc::new(AtomicUsize::new(0)), wake)
            .with_dismissals(closed.clone());
        let publisher = service.publisher(slot);
        let stop = stop.clone();
        let activity = activity.clone();
        let opened = opened.clone();
        threads.push(thread::spawn(move || {
            run_overlay_inner(
                Some(slot),
                |_| Frame {
                    session_id: Some(format!("close-{slot}")),
                    activity: activity.load(Ordering::Relaxed),
                    cards: vec![Card {
                        id: 1,
                        label: format!("Project — {}", ["Build", "Review"][slot]),
                        text: "Close this overlay without opening or closing the actual chat."
                            .into(),
                        final_message: true,
                        attention: true,
                        target: Some(CardTarget {
                            window: 100,
                            session_id: format!("close-{slot}"),
                        }),
                    }],
                    attention: true,
                    dock_request: 1,
                    close: stop.load(Ordering::Relaxed),
                    ..Frame::empty()
                },
                |target, _| {
                    opened.send(target.clone()).unwrap();
                    false.into()
                },
                || None,
                Some(publisher),
                Some(updates),
            )
            .unwrap()
        }));
    }
    let result = std::panic::catch_unwind(|| unsafe {
        SetThreadDpiAwarenessContext(-4isize as Handle);
        let foreground = GetForegroundWindow();
        let deadline = Instant::now() + Duration::from_secs(3);
        while (0..2).any(|slot| service.test_binding(slot).is_none()) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let windows: Vec<_> = (0..2)
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
        thread::sleep(Duration::from_millis(350));
        PostMessageW(windows[0], WM_APP_EXPAND_OVERLAY, 0, 0);
        thread::sleep(Duration::from_millis(350));
        let mut bounds: Rect = zeroed();
        GetWindowRect(windows[0], &mut bounds);
        let dpi = GetDpiForWindow(windows[0]).max(96);
        let x = bounds.right - bounds.left - scale_dip(22, dpi);
        let y = scale_dip(22, dpi);
        let point = ((y as isize) << 16) | (x as isize & 0xffff);
        // Releasing away from X cancels the press and never closes a card.
        SendMessageW(windows[0], WM_LBUTTONDOWN, 0, point);
        SendMessageW(windows[0], WM_LBUTTONUP, 0, (y as isize) << 16 | 2);
        assert!(closures.try_recv().is_err());
        let clicked = Instant::now();
        SendMessageW(windows[0], WM_LBUTTONDOWN, 0, point);
        SendMessageW(windows[0], WM_LBUTTONUP, 0, point);
        assert_eq!(
            closures.recv_timeout(Duration::from_millis(200)).unwrap(),
            (
                CardTarget {
                    window: 100,
                    session_id: "close-0".into()
                },
                1
            )
        );
        eprintln!("Close-button dispatch latency: {:?}", clicked.elapsed());
        thread::sleep(Duration::from_millis(450));
        assert_eq!(IsWindowVisible(windows[0]), 0);
        assert_ne!(IsWindowVisible(windows[1]), 0);
        assert!(service.test_binding(0).is_none());
        PostMessageW(windows[0], WM_FRAME_READY, 0, 0);
        thread::sleep(Duration::from_millis(120));
        assert_eq!(
            IsWindowVisible(windows[0]),
            0,
            "cached frames cannot reopen a closed tab"
        );
        assert!(activations.try_recv().is_err());
        assert_eq!(GetForegroundWindow(), foreground);

        activity.store(2, Ordering::Relaxed);
        PostMessageW(windows[0], WM_FRAME_READY, 0, 0);
        PostMessageW(windows[1], WM_FRAME_READY, 0, 0);
        thread::sleep(Duration::from_millis(350));
        assert_ne!(
            IsWindowVisible(windows[0]),
            0,
            "a new turn can return the tab"
        );

        let code = service.test_binding(1).unwrap().0;
        for key in [0x5b, 0xa0, 0x86, code[0] as u32] {
            service.test_key(key, true);
        }
        thread::sleep(Duration::from_millis(80));
        let escaped = Instant::now();
        service.test_key(0x1b, true);
        assert_eq!(
            closures.recv_timeout(Duration::from_millis(200)).unwrap(),
            (
                CardTarget {
                    window: 100,
                    session_id: "close-1".into()
                },
                2
            )
        );
        eprintln!("Escape dispatch latency: {:?}", escaped.elapsed());
        for key in [0x1b, code[0] as u32, 0x86, 0xa0, 0x5b] {
            service.test_key(key, false);
        }
        thread::sleep(Duration::from_millis(450));
        assert_eq!(IsWindowVisible(windows[1]), 0);
        assert_ne!(IsWindowVisible(windows[0]), 0);
        assert!(activations.try_recv().is_err());
        assert!(closures.try_recv().is_err());
        assert_eq!(GetForegroundWindow(), foreground);
    });
    stop.store(true, Ordering::Relaxed);
    for thread in threads {
        thread.join().unwrap();
    }
    if let Err(cause) = result {
        std::panic::resume_unwind(cause);
    }
}
