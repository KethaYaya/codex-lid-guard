//! Notification-area presence for the native helper, with orderly shutdown.
use super::*;

const TRAY_EVENT: u32 = 0x8050;
const QUIT: usize = 1;

#[repr(C)]
struct NotifyIconData {
    size: u32, window: Hwnd, id: u32, flags: u32, callback: u32, icon: Handle,
    tip: [u16; 128], state: u32, state_mask: u32, info: [u16; 256], version: u32,
    info_title: [u16; 64], info_flags: u32, guid: Guid, balloon_icon: Handle,
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn Shell_NotifyIconW(message: u32, data: *const NotifyIconData) -> Bool;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn CreateIconFromResourceEx(bits: *mut u8, size: u32, icon: Bool, version: u32,
        width: i32, height: i32, flags: u32) -> Handle;
    fn DestroyIcon(icon: Handle) -> Bool;
    fn GetSystemMetricsForDpi(index: i32, dpi: u32) -> i32;
    fn FindWindowW(class: *const u16, name: *const u16) -> Hwnd;
    fn RegisterWindowMessageW(message: *const u16) -> u32;
    fn CreatePopupMenu() -> Handle;
    fn AppendMenuW(menu: Handle, flags: u32, id: usize, text: *const u16) -> Bool;
    fn TrackPopupMenu(menu: Handle, flags: u32, x: i32, y: i32, reserved: i32, window: Hwnd, rect: *const Rect) -> Bool;
    fn DestroyMenu(menu: Handle) -> Bool;
    fn SetTimer(window: Hwnd, id: usize, milliseconds: u32, callback: *const c_void) -> usize;
    fn KillTimer(window: Hwnd, id: usize) -> Bool;
}

struct TrayState {
    data: NotifyIconData,
    icon: OwnedIcon,
    taskbar_created: u32,
    quit: Box<dyn Fn() + Send>,
}

struct OwnedIcon(Handle, (i32, i32));

fn tray_icon_size() -> (i32, i32) {
    unsafe {
        // The helper's hidden thread may be DPI-unaware. Use Explorer's actual
        // taskbar DPI so Windows does not stretch a 16px icon on scaled displays.
        let taskbar = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
        let dpi = GetDpiForWindow(taskbar).max(96);
        (GetSystemMetricsForDpi(49, dpi).max(16), GetSystemMetricsForDpi(50, dpi).max(16))
    }
}

impl OwnedIcon {
    fn load() -> io::Result<Self> {
        let (width, height) = tray_icon_size();
        Self::sized(width, height)
    }

    fn sized(width: i32, height: i32) -> io::Result<Self> {
        match width.max(height) {
            ..=16 => Self::decode(include_bytes!("../Assets/Tray/lid-guard-16.png"), width, height),
            17..=20 => Self::decode(include_bytes!("../Assets/Tray/lid-guard-20.png"), width, height),
            21..=24 => Self::decode(include_bytes!("../Assets/Tray/lid-guard-24.png"), width, height),
            25..=28 => Self::decode(include_bytes!("../Assets/Tray/lid-guard-28.png"), width, height),
            29..=32 => Self::decode(include_bytes!("../Assets/Tray/lid-guard-32.png"), width, height),
            33..=40 => Self::decode(include_bytes!("../Assets/Tray/lid-guard-40.png"), width, height),
            41..=48 => Self::decode(include_bytes!("../Assets/Tray/lid-guard-48.png"), width, height),
            _ => Self::decode(include_bytes!("../Assets/Tray/lid-guard-64.png"), width, height),
        }
    }

    fn decode<const N: usize>(source: &[u8; N], width: i32, height: i32) -> io::Result<Self> {
        // PNG icon resources need DWORD alignment; no runtime asset files.
        #[repr(align(4))]
        struct IconBytes<const N: usize>([u8; N]);
        let mut bytes = IconBytes(*source);
        let icon = unsafe {
            CreateIconFromResourceEx(bytes.0.as_mut_ptr(), bytes.0.len() as u32, 1, 0x0003_0000,
                width, height, 0)
        };
        if icon.is_null() { Err(error("Load Lid Guard tray icon")) }
        else { Ok(Self(icon, (width, height))) }
    }
}

impl Drop for OwnedIcon {
    fn drop(&mut self) { unsafe { DestroyIcon(self.0); } }
}

pub struct TrayIcon {
    window: isize,
    worker: Option<JoinHandle<()>>,
}

impl TrayIcon {
    pub fn start(quit: impl Fn() + Send + 'static) -> io::Result<Self> {
        let (ready, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new().name("lid-guard-tray".into())
            .spawn(move || tray_loop(ready, Box::new(quit)))?;
        let window = result.recv().map_err(io::Error::other)?.map_err(io::Error::other)?;
        Ok(Self { window, worker: Some(worker) })
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe { PostMessageW(self.window as Hwnd, WM_CLOSE, 0, 0); }
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

fn tray_loop(ready: mpsc::SyncSender<Result<isize, String>>, quit: Box<dyn Fn() + Send>) {
    let icon = match OwnedIcon::load() {
        Ok(icon) => icon,
        Err(error) => { let _ = ready.send(Err(error.to_string())); return; }
    };
    unsafe {
        let class = wide(format!("CodexLidGuardTray.{}", GetCurrentProcessId()));
        let instance = GetModuleHandleW(null());
        let mut window_class: WindowClassExW = zeroed();
        window_class.size = size_of::<WindowClassExW>() as u32;
        window_class.window_procedure = Some(procedure);
        window_class.instance = instance;
        window_class.class_name = class.as_ptr();
        if RegisterClassExW(&window_class) == 0 {
            let _ = ready.send(Err(error("Register tray window").to_string()));
            return;
        }
        // A hidden top-level window receives Explorer's TaskbarCreated broadcast.
        let window = CreateWindowExW(WS_EX_TOOLWINDOW, class.as_ptr(), wide("Codex Lid Guard").as_ptr(),
            0, 0, 0, 0, 0, null_mut(), null_mut(), instance, null());
        if window.is_null() {
            let _ = ready.send(Err(error("Create tray window").to_string()));
            UnregisterClassW(class.as_ptr(), instance);
            return;
        }
        let mut data: NotifyIconData = zeroed();
        data.size = size_of::<NotifyIconData>() as u32;
        data.window = window;
        data.id = 1;
        data.flags = 1 | 2 | 4; // message, icon, tooltip
        data.callback = TRAY_EVENT;
        data.icon = icon.0;
        let tip = wide("Codex Lid Guard is running");
        data.tip[..tip.len()].copy_from_slice(&tip);
        let mut state = Box::new(TrayState { data, icon, quit,
            taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) });
        SetWindowLongPtrW(window, GWLP_USERDATA, (&mut *state) as *mut TrayState as isize);
        // Explorer may be restarting. Retry until it accepts the icon.
        if Shell_NotifyIconW(0, &state.data) == 0 { SetTimer(window, 1, 2000, null()); }
        if ready.send(Ok(window as isize)).is_ok() {
            let mut message: Message = zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Shell_NotifyIconW(2, &state.data);
        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        if IsWindow(window) != 0 { DestroyWindow(window); }
        UnregisterClassW(class.as_ptr(), instance);
    }
}

unsafe extern "system" fn procedure(window: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
    unsafe {
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut TrayState;
        if !pointer.is_null() {
            let taskbar_created = (*pointer).taskbar_created != 0 && message == (*pointer).taskbar_created;
            if (taskbar_created || matches!(message, 0x001a | 0x007e)) // settings / display change
                && tray_icon_size() != (*pointer).icon.1
                && let Ok(icon) = OwnedIcon::load() {
                let previous = std::mem::replace(&mut (*pointer).icon, icon);
                (*pointer).data.icon = (*pointer).icon.0;
                Shell_NotifyIconW(1, &(*pointer).data); // NIM_MODIFY; keep the old handle until copied.
                drop(previous);
            }
            let state = &*pointer;
            if taskbar_created || message == 0x0113 {
                if Shell_NotifyIconW(0, &state.data) != 0 { KillTimer(window, 1); }
                else { SetTimer(window, 1, 2000, null()); }
                return 0;
            }
            if message == WM_COMMAND && wparam & 0xffff == QUIT {
                (state.quit)();
                return 0;
            }
            if message == TRAY_EVENT && matches!(lparam as u32, 0x0202 | 0x0205 | 0x007b) {
                let menu = CreatePopupMenu();
                if !menu.is_null() {
                    AppendMenuW(menu, 0x0002, 0, wide("Codex Lid Guard is running").as_ptr());
                    AppendMenuW(menu, 0x0800, 0, null());
                    AppendMenuW(menu, 0, QUIT, wide("Quit Lid Guard (close all tabs)").as_ptr());
                    let mut point: Point = zeroed();
                    GetCursorPos(&mut point);
                    SetForegroundWindow(window);
                    let selected = TrackPopupMenu(menu, 0x0100 | 0x0002, point.x, point.y, 0, window, null());
                    DestroyMenu(menu);
                    PostMessageW(window, 0, 0, 0);
                    if selected as usize == QUIT { SendMessageW(window, WM_COMMAND, QUIT, 0); }
                }
                return 0;
            }
            if message == WM_CLOSE {
                Shell_NotifyIconW(2, &state.data);
                DestroyWindow(window);
                return 0;
            }
        }
        if message == WM_DESTROY { PostQuitMessage(0); return 0; }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[repr(C)]
    struct IconIdentifier { size: u32, window: Hwnd, id: u32, guid: Guid }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn Shell_NotifyIconGetRect(identifier: *const IconIdentifier, rect: *mut Rect) -> i32;
    }
    #[test]
    #[ignore = "briefly displays an owned tray icon; never quits the real helper"]
    fn tray_dispatches_quit_and_removes_its_owned_window_on_drop() {
        assert_eq!(size_of::<NotifyIconData>(), 976);
        for dpi in [96, 120, 144, 168, 192, 240, 288, 384] {
            let size = unsafe { GetSystemMetricsForDpi(49, dpi) };
            let icon = OwnedIcon::sized(size, size).unwrap();
            assert!(!icon.0.is_null(), "load the tray artwork at {dpi} DPI");
        }
        let (quit, received) = mpsc::channel();
        let tray = TrayIcon::start(move || { quit.send(()).unwrap(); }).unwrap();
        let window = tray.window;
        unsafe {
            let identifier = IconIdentifier { size: size_of::<IconIdentifier>() as u32,
                window: window as Hwnd, id: 1, guid: zeroed() };
            let mut rect: Rect = zeroed();
            assert_eq!(Shell_NotifyIconGetRect(&identifier, &mut rect), 0, "Explorer owns the tray icon");
            assert_ne!(IsWindow(window as Hwnd), 0);
            SendMessageW(window as Hwnd, WM_COMMAND, QUIT, 0);
        }
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(tray);
        assert_eq!(unsafe { IsWindow(window as Hwnd) }, 0);
    }
}
