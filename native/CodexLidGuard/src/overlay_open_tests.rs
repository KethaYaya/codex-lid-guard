use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class: *const u16, name: *const u16) -> Hwnd;
    fn IsWindowVisible(window: Hwnd) -> Bool;
}

#[test]
#[ignore = "displays an owned overlay; never opens or activates a real editor"]
fn native_restore_animation_keeps_drawing_during_open_and_recovers_from_failure() {
    let service = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
    let publisher = service.publisher(0);
    let stop = Arc::new(AtomicBool::new(false));
    let epoch = Arc::new(AtomicU64::new(1));
    let (started, requests) = mpsc::channel();
    let ui = {
        let stop = stop.clone();
        let epoch = epoch.clone();
        thread::spawn(move || {
            run_overlay_inner(None, |_| Frame {
            session_id: Some("open-animation".into()),
            cards: vec![Card { id: 1, label: "Owned restore animation".into(),
                text: "Keep drawing this cached message while editor restoration runs separately.".into(),
                final_message: true, attention: true,
                target: Some(CardTarget { window: 100, session_id: "open-animation".into() }) }],
            attention: true, dock_request: epoch.load(Ordering::Relaxed),
            close: stop.load(Ordering::Relaxed), ..Frame::empty()
        }, |target, _| {
            let (finish, result) = mpsc::channel();
            started.send((target.clone(), finish)).unwrap();
            OverlayOpen::Pending(result)
        }, || None, Some(publisher), None).unwrap()
        })
    };
    let result = std::panic::catch_unwind(|| unsafe {
        SetThreadDpiAwarenessContext(-4isize as Handle);
        let foreground = GetForegroundWindow();
        let class = wide(format!(
            "CodexLidGuardMessageOverlay.{}",
            GetCurrentProcessId()
        ));
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.test_binding(0).is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        let window = FindWindowW(class.as_ptr(), null());
        thread::sleep(Duration::from_millis(350));
        let bounds = || {
            let mut rect: Rect = zeroed();
            assert_ne!(GetWindowRect(window, &mut rect), 0);
            rect
        };
        let tab = bounds();
        let token = service.test_binding(0).unwrap().1;
        let before = Instant::now();
        PostMessageW(window, WM_OVERLAY_SHORTCUT, token, 1);
        let (target, finish) = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(target.session_id, "open-animation");
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            bounds(),
            tab,
            "worker startup must not spend the animation or change the tab"
        );
        let animation_started = Instant::now();
        super::super::overlay_window_events::notify_open_started(window as usize, 100);
        let mut widths = std::collections::HashSet::new();
        while animation_started.elapsed() < Duration::from_millis(120) {
            let rect = bounds();
            widths.insert(rect.right - rect.left);
            thread::sleep(Duration::from_millis(3));
        }
        assert!(
            widths.len() >= 4,
            "pending activation must not stall the animation"
        );
        assert!(bounds().right - bounds().left > tab.right - tab.left);
        assert_ne!(
            GetWindowLongPtrW(window, -20) & 0x20,
            0,
            "the animation must be click-through"
        );
        assert!(service.test_binding(0).is_none());
        PostMessageW(window, WM_OVERLAY_SHORTCUT, token, 1);
        SendMessageW(window, WM_LBUTTONDBLCLK, 0, (20 << 16) | 15);
        SendMessageW(window, WM_LBUTTONUP, 0, (20 << 16) | 15);
        finish.send(true).unwrap();
        PostMessageW(window, WM_FRAME_READY, 0, 0);
        let deadline = before + Duration::from_millis(400);
        while IsWindowVisible(window) != 0 {
            assert!(
                Instant::now() < deadline,
                "restore animation did not finish"
            );
            thread::sleep(Duration::from_millis(2));
        }
        println!(
            "Restore animation finished: {:?} after restore started; sampled widths={}",
            animation_started.elapsed(),
            widths.len(),
        );
        thread::sleep(Duration::from_millis(300));
        assert_eq!(
            IsWindowVisible(window),
            0,
            "stale frames must not replay the opened overlay"
        );
        assert!(
            requests.try_recv().is_err(),
            "queued input must not open twice"
        );
        epoch.store(2, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(600));
        assert_ne!(IsWindowVisible(window), 0);
        assert_eq!(GetWindowLongPtrW(window, -20) & 0x20, 0);
        let token = service.test_binding(0).unwrap().1;
        PostMessageW(window, WM_OVERLAY_SHORTCUT, token, 1);
        let (_, finish) = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        thread::sleep(Duration::from_millis(65));
        finish.send(false).unwrap();
        PostMessageW(window, WM_FRAME_READY, 0, 0);
        thread::sleep(Duration::from_millis(100));
        assert_ne!(
            IsWindowVisible(window),
            0,
            "failed open must recover the notification"
        );
        assert_eq!(
            GetWindowLongPtrW(window, -20) & 0x20,
            0,
            "failed open must restore mouse controls"
        );
        assert!(service.test_binding(0).is_some());
        assert_eq!(GetForegroundWindow(), foreground);
    });
    stop.store(true, Ordering::Relaxed);
    ui.join().unwrap();
    if let Err(cause) = result {
        std::panic::resume_unwind(cause);
    }
}
