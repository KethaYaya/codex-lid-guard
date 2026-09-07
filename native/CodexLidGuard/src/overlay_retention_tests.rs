use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[link(name = "user32")]
unsafe extern "system" {
    fn IsWindowVisible(window: Hwnd) -> Bool;
    fn GetWindowTextW(window: Hwnd, text: *mut u16, length: i32) -> i32;
}

#[test]
#[ignore = "displays one owned tab and destroys only its owned source window"]
fn native_tab_survives_source_window_destruction_and_remains_interactive() {
    unsafe { SetThreadDpiAwarenessContext(-4isize as Handle); }
    let origin = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, wide("STATIC").as_ptr(),
        wide("Owned retention test source").as_ptr(), WS_POPUP,
        100, 100, 300, 200, null_mut(), null_mut(), GetModuleHandleW(null()), null()) };
    assert!(!origin.is_null());
    let origin_id = origin as usize as u64;
    let stop = Arc::new(AtomicBool::new(false));
    let updated = Arc::new(AtomicBool::new(false));
    let destination = Arc::new(AtomicUsize::new(0));
    let (wake, _requests) = mpsc::sync_channel(1);
    let updates = OverlayUpdates::new(destination.clone(), wake);
    let (opened, activations) = mpsc::channel();
    let service = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
    let publisher = service.publisher(0);
    let ui = {
        let stop = stop.clone();
        let updated = updated.clone();
        thread::spawn(move || run_overlay_inner(None, |_| Frame {
            session_id: Some("retained-chat".into()), window: Some(origin_id),
            cards: vec![Card { id: 1,
                label: if updated.load(Ordering::Relaxed) { "Retained after source closed".into() } else { "Owned retained tab".into() },
                text: "Keep this message and its project after the originating window closes.".into(),
                final_message: true, attention: true,
                target: Some(CardTarget { window: origin_id, session_id: "retained-chat".into(),
                    project: Some(crate::session_navigation::Project { cwd: r"C:\Owned project".into(),
                        path: r"C:\Owned project".into(), executable: r"C:\VS Code\Code.exe".into() }) }) }],
            attention: true, dock_request: 1, close: stop.load(Ordering::Relaxed), ..Frame::empty()
        }, |target, _| { opened.send(target.clone()).unwrap(); false.into() }, || None, Some(publisher), Some(updates)))
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let deadline = Instant::now() + Duration::from_secs(3);
        while destination.load(Ordering::Acquire) == 0 {
            assert!(Instant::now() < deadline && !ui.is_finished(), "tab did not start");
            thread::sleep(Duration::from_millis(5));
        }
        let overlay = destination.load(Ordering::Acquire) as Hwnd;
        thread::sleep(Duration::from_millis(400));
        assert_ne!(IsWindowVisible(overlay), 0);
        let mut before: Rect = zeroed();
        assert_ne!(GetWindowRect(overlay, &mut before), 0);
        assert_ne!(DestroyWindow(origin), 0);
        assert_eq!(IsWindow(origin), 0);
        updated.store(true, Ordering::Relaxed);
        OverlayUpdates::notify(&destination);
        thread::sleep(Duration::from_millis(400));
        assert!(!ui.is_finished(), "closing the source window terminated the overlay worker");
        assert_ne!(IsWindowVisible(overlay), 0, "the retained tab disappeared");
        let mut after: Rect = zeroed();
        assert_ne!(GetWindowRect(overlay, &mut after), 0);
        assert_eq!(before, after, "the closed project must not move the tab to a different display or change its scale");
        let mut text = [0u16; 128];
        let length = GetWindowTextW(overlay, text.as_mut_ptr(), text.len() as i32);
        assert_eq!(String::from_utf16_lossy(&text[..length as usize]), "Retained after source closed");
        // Exercise the normal open action, with a simulated failed launcher.
        let (_, token) = service.test_binding(0).unwrap();
        PostMessageW(overlay, WM_OVERLAY_SHORTCUT, token, 1);
        let target = activations.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(target.window, origin_id);
        assert_eq!(target.project.unwrap().path, r"C:\Owned project");
        thread::sleep(Duration::from_millis(150));
        assert_ne!(IsWindowVisible(overlay), 0, "failed reopen must leave the tab available");
    }));
    stop.store(true, Ordering::Relaxed);
    OverlayUpdates::notify(&destination);
    let worker_result = ui.join().unwrap();
    unsafe { if IsWindow(origin) != 0 { DestroyWindow(origin); } }
    if let Err(cause) = result {
        eprintln!("Overlay worker result: {worker_result:?}");
        std::panic::resume_unwind(cause);
    }
    worker_result.unwrap();
}
