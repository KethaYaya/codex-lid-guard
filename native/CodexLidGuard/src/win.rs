#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, c_void};
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::logging;
use crate::model::LidState;
use crate::paths;

#[path = "overlay_window.rs"]
mod overlay_window;
pub use overlay_window::run_session_overlay;
#[path = "overlay_shortcuts.rs"]
mod overlay_shortcuts;
pub use overlay_shortcuts::OverlayShortcuts;

pub fn is_editor_window(window: u64) -> bool {
    unsafe {
        let hwnd = window as usize as Hwnd;
        let mut process_id = 0;
        IsWindow(hwnd) != 0
            && GetWindowThreadProcessId(hwnd, &mut process_id) != 0
            && process_executable_name(process_id).as_deref().is_some_and(is_supported_editor_process)
    }
}

type Bool = i32;
type ByteBool = u8;
type Handle = *mut c_void;
type Hwnd = *mut c_void;
type Lparam = isize;
type Lresult = isize;
type Wparam = usize;

const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const ERROR_ALREADY_EXISTS: u32 = 183;
const ERROR_BROKEN_PIPE: u32 = 109;
const ERROR_IO_PENDING: u32 = 997;
const ERROR_PIPE_CONNECTED: u32 = 535;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;
const INFINITE: u32 = u32::MAX;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
const PIPE_TYPE_BYTE: u32 = 0;
const PIPE_READMODE_BYTE: u32 = 0;
const PIPE_WAIT: u32 = 0;
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_USER_CLASS: u32 = 1;
const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
const PROCESS_TERMINATE: u32 = 0x0001;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
const ES_CONTINUOUS: u32 = 0x8000_0000;
const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_CLOSE: u32 = 0x0010;
const WM_DESTROY: u32 = 0x0002;
const WM_ACTIVATE: u32 = 0x0006;
const WM_PAINT: u32 = 0x000F;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_DRAWITEM: u32 = 0x002B;
const WM_SETFONT: u32 = 0x0030;
const WM_KEYDOWN: u32 = 0x0100;
const WM_COMMAND: u32 = 0x0111;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_NCDESTROY: u32 = 0x0082;
const WM_APP_HOVER_SESSION: u32 = 0x8001;
const SW_RESTORE: i32 = 9;
const SW_SHOW: i32 = 5;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_CONTROLPARENT: u32 = 0x0001_0000;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_POPUP: u32 = 0x8000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_GROUP: u32 = 0x0002_0000;
const BS_OWNERDRAW: u32 = 0x0000_000B;
const ODT_BUTTON: u32 = 4;
const ODS_SELECTED: u32 = 0x0001;
const ODS_FOCUS: u32 = 0x0010;
const DT_VCENTER: u32 = 0x0004;
const DT_SINGLELINE: u32 = 0x0020;
const DT_NOPREFIX: u32 = 0x0800;
const DT_END_ELLIPSIS: u32 = 0x8000;
const TRANSPARENT: i32 = 1;
const DEFAULT_GUI_FONT: i32 = 17;
const NULL_PEN: i32 = 8;
const GWLP_USERDATA: i32 = -21;
const BN_CLICKED: usize = 0;
const WA_INACTIVE: usize = 0;
const VK_TAB: usize = 0x09;
const VK_RETURN: usize = 0x0D;
const VK_ESCAPE: usize = 0x1B;
const VK_SPACE: usize = 0x20;
const VK_UP: usize = 0x26;
const VK_DOWN: usize = 0x28;
const VK_SHIFT: i32 = 0x10;
const TME_LEAVE: u32 = 0x0000_0002;
const LWA_ALPHA: u32 = 0x0000_0002;
const SESSION_BUTTON_ID: usize = 1_000;
const NO_HOVERED_SESSION: usize = usize::MAX;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_BORDER_COLOR: u32 = 34;
const DWMWCP_ROUND: u32 = 2;
const PBT_POWERSETTINGCHANGE: usize = 0x8013;
const DEVICE_NOTIFY_WINDOW_HANDLE: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

impl Guid {
    const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let compact: String = value
            .trim_matches(|character| character == '{' || character == '}')
            .chars()
            .filter(|character| *character != '-')
            .collect();
        if compact.len() != 32 {
            return None;
        }
        let bytes = (0..16)
            .map(|index| u8::from_str_radix(&compact[index * 2..index * 2 + 2], 16).ok())
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            data1: u32::from_be_bytes(bytes[0..4].try_into().ok()?),
            data2: u16::from_be_bytes(bytes[4..6].try_into().ok()?),
            data3: u16::from_be_bytes(bytes[6..8].try_into().ok()?),
            data4: bytes[8..16].try_into().ok()?,
        })
    }
}

impl std::fmt::Display for Guid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7]
        )
    }
}

const POWER_BUTTONS_SUBGROUP: Guid = Guid::new(
    0x4f971e89,
    0xeebd,
    0x4455,
    [0xa8, 0xde, 0x9e, 0x59, 0x04, 0x0e, 0x73, 0x47],
);
const LID_CLOSE_ACTION: Guid = Guid::new(
    0x5ca83367,
    0x6e45,
    0x459f,
    [0xa2, 0x7b, 0x47, 0x6b, 0x1d, 0x01, 0xc9, 0x36],
);
const LID_SWITCH_STATE_CHANGE: Guid = Guid::new(
    0xba3e0f4d,
    0xb817,
    0x4094,
    [0xa2, 0xd1, 0xd5, 0x63, 0x79, 0xe6, 0xa0, 0xf3],
);

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: Handle,
}

#[repr(C)]
struct SidAndAttributes {
    sid: *mut c_void,
    attributes: u32,
}

#[repr(C)]
struct TokenUser {
    user: SidAndAttributes,
}

#[repr(C)]
struct ProcessEntry32W {
    size: u32,
    usage: u32,
    process_id: u32,
    default_heap_id: usize,
    module_id: u32,
    threads: u32,
    parent_process_id: u32,
    priority_base: i32,
    flags: u32,
    exe_file: [u16; 260],
}

#[repr(C)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct PaintStruct {
    device_context: Handle,
    erase: Bool,
    paint: Rect,
    restore: Bool,
    incremental_update: Bool,
    reserved: [u8; 32],
}

#[repr(C)]
struct DrawItemStruct {
    control_type: u32,
    control_id: u32,
    item_id: u32,
    item_action: u32,
    item_state: u32,
    item_window: Hwnd,
    device_context: Handle,
    item_rect: Rect,
    item_data: usize,
}

#[repr(C)]
struct TrackMouseEvent {
    size: u32,
    flags: u32,
    track_window: Hwnd,
    hover_time: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PopupTheme {
    Dark,
    Light,
    HighContrast,
    HighContrastLight,
}

#[derive(Clone, Copy)]
struct PopupColors {
    background: u32,
    border: u32,
    text: u32,
    muted_text: u32,
    hover_background: u32,
    hover_text: u32,
    active_background: u32,
    active_hover_background: u32,
    active_text: u32,
    unviewed_background: u32,
    unviewed_hover_background: u32,
    unviewed_text: u32,
}

struct NotificationPopupState {
    title: Vec<u16>,
    items: Vec<Vec<u16>>,
    buttons: Vec<Hwnd>,
    selected_index: Option<usize>,
    hovered_index: Option<usize>,
    active_items: Vec<bool>,
    unviewed_items: Vec<bool>,
    colors: PopupColors,
    font: Handle,
    dpi: u32,
    header_height: i32,
    closing: bool,
}

#[repr(C)]
struct Message {
    window: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    time: u32,
    point: Point,
    private: u32,
}

type WindowProcedure = unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult;
type SubclassProcedure =
    unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam, usize, usize) -> Lresult;

#[repr(C)]
struct WindowClassExW {
    size: u32,
    style: u32,
    window_procedure: Option<WindowProcedure>,
    class_extra: i32,
    window_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu_name: *const u16,
    class_name: *const u16,
    small_icon: Handle,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CloseHandle(handle: Handle) -> Bool;
    fn CreateEventW(
        attributes: *const c_void,
        manual_reset: Bool,
        initial_state: Bool,
        name: *const u16,
    ) -> Handle;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn CreateMutexW(attributes: *const c_void, initial_owner: Bool, name: *const u16) -> Handle;
    fn CreateNamedPipeW(
        name: *const u16,
        open_mode: u32,
        pipe_mode: u32,
        max_instances: u32,
        out_size: u32,
        in_size: u32,
        timeout: u32,
        security: *const c_void,
    ) -> Handle;
    fn ConnectNamedPipe(pipe: Handle, overlapped: *mut Overlapped) -> Bool;
    fn CancelIoEx(handle: Handle, overlapped: *const Overlapped) -> Bool;
    fn GetCurrentProcess() -> Handle;
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetLastError() -> u32;
    fn GetLocalTime(time: *mut SystemTime);
    fn GetModuleHandleW(module_name: *const u16) -> Handle;
    fn GetOverlappedResult(
        handle: Handle,
        overlapped: *mut Overlapped,
        transferred: *mut u32,
        wait: Bool,
    ) -> Bool;
    fn LocalFree(memory: Handle) -> Handle;
    fn MoveFileExW(existing: *const u16, destination: *const u16, flags: u32) -> Bool;
    fn OpenProcess(access: u32, inherit: Bool, process_id: u32) -> Handle;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: u32,
        executable_name: *mut u16,
        size: *mut u32,
    ) -> Bool;
    fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry32W) -> Bool;
    fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry32W) -> Bool;
    fn ProcessIdToSessionId(process_id: u32, session_id: *mut u32) -> Bool;
    fn ReadFile(
        handle: Handle,
        buffer: *mut c_void,
        count: u32,
        read: *mut u32,
        overlapped: *mut Overlapped,
    ) -> Bool;
    fn ReleaseMutex(mutex: Handle) -> Bool;
    fn SetThreadExecutionState(flags: u32) -> u32;
    fn TerminateProcess(process: Handle, exit_code: u32) -> Bool;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn WaitNamedPipeW(name: *const u16, timeout: u32) -> Bool;
    fn WriteFile(
        handle: Handle,
        buffer: *const c_void,
        count: u32,
        written: *mut u32,
        overlapped: *mut Overlapped,
    ) -> Bool;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> Handle;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertSidToStringSidW(sid: *mut c_void, string_sid: *mut *mut u16) -> Bool;
    fn GetTokenInformation(
        token: Handle,
        class: u32,
        information: *mut c_void,
        length: u32,
        return_length: *mut u32,
    ) -> Bool;
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> Bool;
}

#[link(name = "powrprof")]
unsafe extern "system" {
    fn PowerGetActiveScheme(root: Handle, scheme: *mut *mut Guid) -> u32;
    fn PowerReadACValueIndex(
        root: Handle,
        scheme: *const Guid,
        subgroup: *const Guid,
        setting: *const Guid,
        value: *mut u32,
    ) -> u32;
    fn PowerReadDCValueIndex(
        root: Handle,
        scheme: *const Guid,
        subgroup: *const Guid,
        setting: *const Guid,
        value: *mut u32,
    ) -> u32;
    fn PowerWriteACValueIndex(
        root: Handle,
        scheme: *const Guid,
        subgroup: *const Guid,
        setting: *const Guid,
        value: u32,
    ) -> u32;
    fn PowerWriteDCValueIndex(
        root: Handle,
        scheme: *const Guid,
        subgroup: *const Guid,
        setting: *const Guid,
        value: u32,
    ) -> u32;
    fn PowerSetActiveScheme(root: Handle, scheme: *const Guid) -> u32;
    fn SetSuspendState(
        hibernate: ByteBool,
        force_critical: ByteBool,
        disable_wake_event: ByteBool,
    ) -> ByteBool;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn BeginPaint(window: Hwnd, paint: *mut PaintStruct) -> Handle;
    fn CreateWindowExW(
        extended_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Handle,
        instance: Handle,
        parameter: *const c_void,
    ) -> Hwnd;
    fn DefWindowProcW(window: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
    fn AttachThreadInput(id_attach: u32, id_attach_to: u32, attach: Bool) -> Bool;
    fn BringWindowToTop(window: Hwnd) -> Bool;
    fn DestroyWindow(window: Hwnd) -> Bool;
    fn DispatchMessageW(message: *const Message) -> Lresult;
    fn DrawTextW(
        device_context: Handle,
        text: *const u16,
        count: i32,
        rectangle: *mut Rect,
        format: u32,
    ) -> i32;
    fn EndPaint(window: Hwnd, paint: *const PaintStruct) -> Bool;
    fn FillRect(device_context: Handle, rectangle: *const Rect, brush: Handle) -> i32;
    fn FrameRect(device_context: Handle, rectangle: *const Rect, brush: Handle) -> i32;
    fn GetClientRect(window: Hwnd, rectangle: *mut Rect) -> Bool;
    fn GetCursorPos(point: *mut Point) -> Bool;
    fn GetDlgCtrlID(window: Hwnd) -> i32;
    fn GetDpiForWindow(window: Hwnd) -> u32;
    fn GetFocus() -> Hwnd;
    fn GetForegroundWindow() -> Hwnd;
    fn GetKeyState(key: i32) -> i16;
    fn GetMessageW(message: *mut Message, window: Hwnd, min: u32, max: u32) -> Bool;
    fn GetWindowLongPtrW(window: Hwnd, index: i32) -> isize;
    fn GetWindowRect(window: Hwnd, rectangle: *mut Rect) -> Bool;
    fn GetWindowThreadProcessId(window: Hwnd, process_id: *mut u32) -> u32;
    fn InvalidateRect(window: Hwnd, rectangle: *const Rect, erase: Bool) -> Bool;
    fn IsIconic(window: Hwnd) -> Bool;
    fn IsWindow(window: Hwnd) -> Bool;
    fn LoadCursorW(instance: Handle, cursor_name: *const u16) -> Handle;
    fn PostMessageW(window: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Bool;
    fn PostQuitMessage(exit_code: i32);
    fn RegisterClassExW(window_class: *const WindowClassExW) -> u16;
    fn RegisterPowerSettingNotification(
        recipient: Handle,
        setting: *const Guid,
        flags: u32,
    ) -> Handle;
    fn SendMessageW(window: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
    fn SetFocus(window: Hwnd) -> Hwnd;
    fn TranslateMessage(message: *const Message) -> Bool;
    fn SetForegroundWindow(window: Hwnd) -> Bool;
    fn SetLayeredWindowAttributes(window: Hwnd, color_key: u32, alpha: u8, flags: u32) -> Bool;
    fn SetProcessDpiAwarenessContext(context: Handle) -> Bool;
    fn SetWindowLongPtrW(window: Hwnd, index: i32, value: isize) -> isize;
    fn ShowWindow(window: Hwnd, command: i32) -> Bool;
    fn TrackMouseEvent(event: *mut TrackMouseEvent) -> Bool;
    fn UnregisterClassW(class_name: *const u16, instance: Handle) -> Bool;
    fn UnregisterPowerSettingNotification(handle: Handle) -> Bool;
    fn UpdateWindow(window: Hwnd) -> Bool;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> Handle;
    fn DeleteObject(object: Handle) -> Bool;
    fn GetStockObject(object: i32) -> Handle;
    fn RoundRect(
        device_context: Handle,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        width: i32,
        height: i32,
    ) -> Bool;
    fn SelectObject(device_context: Handle, object: Handle) -> Handle;
    fn SetBkMode(device_context: Handle, mode: i32) -> i32;
    fn SetTextColor(device_context: Handle, color: u32) -> u32;
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn DefSubclassProc(window: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
    fn RemoveWindowSubclass(
        window: Hwnd,
        procedure: Option<SubclassProcedure>,
        subclass_id: usize,
    ) -> Bool;
    fn SetWindowSubclass(
        window: Hwnd,
        procedure: Option<SubclassProcedure>,
        subclass_id: usize,
        reference_data: usize,
    ) -> Bool;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(
        window: Hwnd,
        attribute: u32,
        value: *const c_void,
        value_size: u32,
    ) -> i32;
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn wide_string(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn error(operation: &str) -> io::Error {
    io::Error::other(format!("{operation}: {}", io::Error::last_os_error()))
}

pub fn local_timestamp() -> String {
    unsafe {
        let mut value: SystemTime = zeroed();
        GetLocalTime(&mut value);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}",
            value.year,
            value.month,
            value.day,
            value.hour,
            value.minute,
            value.second,
            value.milliseconds
        )
    }
}

pub fn current_session_id() -> u32 {
    unsafe {
        let mut session = 0;
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut session) != 0 {
            session
        } else {
            0
        }
    }
}

pub fn current_user_sid() -> Option<String> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let token = OwnedHandle(token);
        let mut required = 0;
        let _ = GetTokenInformation(token.0, TOKEN_USER_CLASS, null_mut(), 0, &mut required);
        if required == 0 {
            return None;
        }
        let mut buffer = vec![0u8; required as usize];
        if GetTokenInformation(
            token.0,
            TOKEN_USER_CLASS,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        ) == 0
        {
            return None;
        }
        let token_user = &*(buffer.as_ptr() as *const TokenUser);
        let mut sid_text: *mut u16 = null_mut();
        if ConvertSidToStringSidW(token_user.user.sid, &mut sid_text) == 0 || sid_text.is_null() {
            return None;
        }
        let mut length = 0;
        while *sid_text.add(length) != 0 {
            length += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, length));
        LocalFree(sid_text.cast());
        Some(result)
    }
}

struct OwnedHandle(Handle);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

unsafe impl Send for OwnedHandle {}

pub struct InstanceMutex(OwnedHandle);

impl InstanceMutex {
    pub fn acquire(name: &str) -> io::Result<Option<Self>> {
        let name = wide(name);
        unsafe {
            let handle = CreateMutexW(null(), 1, name.as_ptr());
            if handle.is_null() {
                return Err(error("CreateMutexW failed"));
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                return Ok(None);
            }
            Ok(Some(Self(OwnedHandle(handle))))
        }
    }
}

impl Drop for InstanceMutex {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0.0);
        }
    }
}

pub enum AcceptResult {
    Connected(PipeConnection),
    TimedOut,
}

pub struct PipeServer {
    name: Vec<u16>,
}

impl PipeServer {
    pub fn new(name: &str) -> Self {
        Self { name: wide(name) }
    }

    pub fn accept(&self, timeout: Option<Duration>) -> io::Result<AcceptResult> {
        unsafe {
            let pipe = CreateNamedPipeW(
                self.name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65_536,
                65_536,
                0,
                null(),
            );
            if pipe == INVALID_HANDLE_VALUE {
                return Err(error("CreateNamedPipeW failed"));
            }
            let pipe = OwnedHandle(pipe);
            let event = CreateEventW(null(), 1, 0, null());
            if event.is_null() {
                return Err(error("CreateEventW failed"));
            }
            let event = OwnedHandle(event);
            let mut overlapped = Overlapped {
                internal: 0,
                internal_high: 0,
                offset: 0,
                offset_high: 0,
                event: event.0,
            };
            let connected_immediately = ConnectNamedPipe(pipe.0, &mut overlapped) != 0;
            if !connected_immediately {
                match GetLastError() {
                    ERROR_PIPE_CONNECTED => {}
                    ERROR_IO_PENDING => {
                        let milliseconds = timeout
                            .map(|value| value.as_millis().min((u32::MAX - 1) as u128) as u32)
                            .unwrap_or(INFINITE);
                        match WaitForSingleObject(event.0, milliseconds) {
                            WAIT_OBJECT_0 => {
                                let mut transferred = 0;
                                if GetOverlappedResult(pipe.0, &mut overlapped, &mut transferred, 0)
                                    == 0
                                    && GetLastError() != ERROR_PIPE_CONNECTED
                                {
                                    return Err(error("ConnectNamedPipe failed"));
                                }
                            }
                            WAIT_TIMEOUT => {
                                CancelIoEx(pipe.0, &overlapped);
                                let mut transferred = 0;
                                let _ = GetOverlappedResult(
                                    pipe.0,
                                    &mut overlapped,
                                    &mut transferred,
                                    1,
                                );
                                return Ok(AcceptResult::TimedOut);
                            }
                            _ => return Err(error("waiting for a named-pipe connection failed")),
                        }
                    }
                    _ => return Err(error("ConnectNamedPipe failed")),
                }
            }
            drop(event);
            Ok(AcceptResult::Connected(PipeConnection {
                handle: pipe,
                overlapped: true,
                io_timeout: Some(Duration::from_secs(5)),
            }))
        }
    }
}

pub struct PipeConnection {
    handle: OwnedHandle,
    overlapped: bool,
    io_timeout: Option<Duration>,
}

impl PipeConnection {
    pub fn read_line(&self) -> io::Result<String> {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let read = self.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.contains(&b'\n') || bytes.len() > 1_048_576 {
                break;
            }
        }
        let end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..end]).to_string())
    }

    pub fn write_line(&self, value: &str) -> io::Result<()> {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(b'\n');
        let mut written = 0;
        while written < bytes.len() {
            written += self.write(&bytes[written..])?;
        }
        Ok(())
    }

    fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        unsafe {
            let mut transferred = 0;
            if !self.overlapped {
                if ReadFile(
                    self.handle.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut transferred,
                    null_mut(),
                ) == 0
                {
                    if GetLastError() == ERROR_BROKEN_PIPE {
                        return Ok(0);
                    }
                    return Err(error("ReadFile failed"));
                }
                return Ok(transferred as usize);
            }
            run_overlapped(self.handle.0, self.io_timeout, |overlapped| {
                ReadFile(
                    self.handle.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    null_mut(),
                    overlapped,
                )
            })
        }
    }

    fn write(&self, buffer: &[u8]) -> io::Result<usize> {
        unsafe {
            let mut transferred = 0;
            if !self.overlapped {
                if WriteFile(
                    self.handle.0,
                    buffer.as_ptr().cast(),
                    buffer.len() as u32,
                    &mut transferred,
                    null_mut(),
                ) == 0
                {
                    return Err(error("WriteFile failed"));
                }
                return Ok(transferred as usize);
            }
            run_overlapped(self.handle.0, self.io_timeout, |overlapped| {
                WriteFile(
                    self.handle.0,
                    buffer.as_ptr().cast(),
                    buffer.len() as u32,
                    null_mut(),
                    overlapped,
                )
            })
        }
    }
}

// Each server connection owns a fresh pipe instance. Closing its handle lets
// Windows deliver queued bytes before EOF; DisconnectNamedPipe would discard
// unread replies and is only needed when reusing the same server instance.

unsafe fn run_overlapped(
    operation_handle: Handle,
    timeout: Option<Duration>,
    operation: impl FnOnce(*mut Overlapped) -> Bool,
) -> io::Result<usize> {
    unsafe {
        let event = CreateEventW(null(), 1, 0, null());
        if event.is_null() {
            return Err(error("CreateEventW failed"));
        }
        let event = OwnedHandle(event);
        let mut overlapped = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: event.0,
        };
        if operation(&mut overlapped) == 0 && GetLastError() != ERROR_IO_PENDING {
            if GetLastError() == ERROR_BROKEN_PIPE {
                return Ok(0);
            }
            return Err(error("overlapped I/O failed"));
        }
        let milliseconds = timeout
            .map(|value| value.as_millis().min((u32::MAX - 1) as u128) as u32)
            .unwrap_or(INFINITE);
        match WaitForSingleObject(event.0, milliseconds) {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => {
                CancelIoEx(operation_handle, &overlapped);
                let mut transferred = 0;
                let _ = GetOverlappedResult(operation_handle, &mut overlapped, &mut transferred, 1);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "named-pipe I/O timed out",
                ));
            }
            _ => return Err(error("waiting for overlapped I/O failed")),
        }
        let mut transferred = 0;
        if GetOverlappedResult(operation_handle, &mut overlapped, &mut transferred, 0) == 0 {
            if GetLastError() == ERROR_BROKEN_PIPE {
                return Ok(0);
            }
            return Err(error("GetOverlappedResult failed"));
        }
        Ok(transferred as usize)
    }
}

pub fn connect_pipe(
    name: &str,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> io::Result<PipeConnection> {
    let name = wide(name);
    let milliseconds = connect_timeout.as_millis().min(u32::MAX as u128) as u32;
    unsafe {
        if WaitNamedPipeW(name.as_ptr(), milliseconds) == 0 {
            return Err(error("WaitNamedPipeW failed"));
        }
        let handle = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(error("CreateFileW failed for the guardian pipe"));
        }
        Ok(PipeConnection {
            handle: OwnedHandle(handle),
            overlapped: true,
            io_timeout: Some(io_timeout),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SavedPowerState {
    #[serde(rename = "Scheme", alias = "scheme")]
    scheme: String,
    #[serde(rename = "AcLidAction", alias = "acLidAction")]
    ac_lid_action: u32,
    #[serde(rename = "DcLidAction", alias = "dcLidAction")]
    dc_lid_action: u32,
    #[serde(rename = "ChangedAcLidAction", alias = "changedAcLidAction", default)]
    changed_ac_lid_action: Option<bool>,
    #[serde(rename = "ChangedDcLidAction", alias = "changedDcLidAction", default)]
    changed_dc_lid_action: Option<bool>,
    #[serde(rename = "CapturedAt", alias = "capturedAt")]
    captured_at: String,
}

impl SavedPowerState {
    fn changed_ac_lid_action(&self) -> bool {
        self.changed_ac_lid_action
            .unwrap_or(self.ac_lid_action != 0)
    }

    fn changed_dc_lid_action(&self) -> bool {
        self.changed_dc_lid_action
            .unwrap_or(self.dc_lid_action != 0)
    }

    fn changed_lid_policy(&self) -> bool {
        self.changed_ac_lid_action() || self.changed_dc_lid_action()
    }
}

pub struct PowerPolicy {
    saved: Option<SavedPowerState>,
    guarding: bool,
    #[cfg(test)]
    recovery_enabled: bool,
    #[cfg(test)]
    system_changes_enabled: bool,
}

impl PowerPolicy {
    pub fn new() -> Self {
        let mut value = Self {
            saved: None,
            guarding: false,
            #[cfg(test)]
            recovery_enabled: true,
            #[cfg(test)]
            system_changes_enabled: true,
        };
        value.restore_stale();
        value
    }

    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self {
            saved: None,
            guarding: false,
            recovery_enabled: false,
            system_changes_enabled: false,
        }
    }

    pub fn is_guarding(&self) -> bool {
        self.guarding
    }

    pub fn acquire(&mut self) -> io::Result<()> {
        if self.guarding {
            return Ok(());
        }
        if !self.system_changes_are_enabled() {
            self.guarding = true;
            return Ok(());
        }
        set_execution_state(true)?;
        let result = (|| {
            let scheme = active_scheme()?;
            let ac = power_read(true, &scheme)?;
            let dc = power_read(false, &scheme)?;
            let saved = SavedPowerState {
                scheme: scheme.to_string(),
                ac_lid_action: ac,
                dc_lid_action: dc,
                changed_ac_lid_action: Some(ac != 0),
                changed_dc_lid_action: Some(dc != 0),
                captured_at: local_timestamp(),
            };
            if saved.changed_lid_policy() {
                save_recovery(&saved)?;
            }
            self.saved = Some(saved.clone());
            if saved.changed_ac_lid_action() {
                power_write(true, &scheme, 0)?;
            }
            if saved.changed_dc_lid_action() {
                power_write(false, &scheme, 0)?;
            }
            if saved.changed_lid_policy() {
                power_activate(&scheme)?;
            }
            Ok(saved)
        })();
        match result {
            Ok(saved) => {
                self.guarding = true;
                logging::write(format!(
                    "Guard acquired for power scheme {}; original lid actions AC={}, DC={}.",
                    saved.scheme, saved.ac_lid_action, saved.dc_lid_action
                ));
                Ok(())
            }
            Err(cause) => {
                if let Some(saved) = self.saved.take() {
                    let _ = restore_power_state(&saved);
                }
                let _ = set_execution_state(false);
                Err(cause)
            }
        }
    }

    pub fn release(&mut self) -> io::Result<()> {
        if !self.system_changes_are_enabled() {
            self.saved = None;
            self.guarding = false;
            return Ok(());
        }
        let recovery_exists = self.recovery_is_enabled() && paths::recovery_file().exists();
        if !self.guarding && self.saved.is_none() && !recovery_exists {
            return Ok(());
        }
        let _ = set_execution_state(false);
        let state = self
            .saved
            .clone()
            .or_else(|| self.recovery_is_enabled().then(load_recovery).flatten());
        if let Some(state) = state {
            restore_power_state(&state)?;
            if state.changed_lid_policy() {
                logging::write(format!(
                    "Restored power scheme {} lid actions AC={}, DC={}.",
                    state.scheme, state.ac_lid_action, state.dc_lid_action
                ));
            }
        }
        self.saved = None;
        self.guarding = false;
        Ok(())
    }

    fn restore_stale(&mut self) {
        if !self.recovery_is_enabled() {
            return;
        }
        let Some(stale) = load_recovery() else {
            return;
        };
        logging::write("Found an interrupted guard session; restoring its saved lid policy.");
        if let Err(cause) = restore_power_state(&stale) {
            logging::write(format!("Could not restore the saved power policy: {cause}"));
        }
    }

    fn recovery_is_enabled(&self) -> bool {
        #[cfg(test)]
        {
            self.recovery_enabled
        }
        #[cfg(not(test))]
        {
            true
        }
    }

    fn system_changes_are_enabled(&self) -> bool {
        #[cfg(test)]
        {
            self.system_changes_enabled
        }
        #[cfg(not(test))]
        {
            true
        }
    }
}

impl Drop for PowerPolicy {
    fn drop(&mut self) {
        if let Err(cause) = self.release() {
            logging::write(format!("Power-policy cleanup failed: {cause}"));
        }
    }
}

pub fn suspend() -> bool {
    logging::write("Requesting Windows sleep after the Codex task completed with the lid closed.");
    unsafe { SetSuspendState(0, 0, 0) != 0 }
}

fn active_scheme() -> io::Result<Guid> {
    unsafe {
        let mut pointer: *mut Guid = null_mut();
        check_power(
            PowerGetActiveScheme(null_mut(), &mut pointer),
            "read the active power scheme",
        )?;
        if pointer.is_null() {
            return Err(io::Error::other("PowerGetActiveScheme returned no scheme"));
        }
        let value = *pointer;
        LocalFree(pointer.cast());
        Ok(value)
    }
}

fn power_read(ac: bool, scheme: &Guid) -> io::Result<u32> {
    unsafe {
        let mut value = 0;
        let result = if ac {
            PowerReadACValueIndex(
                null_mut(),
                scheme,
                &POWER_BUTTONS_SUBGROUP,
                &LID_CLOSE_ACTION,
                &mut value,
            )
        } else {
            PowerReadDCValueIndex(
                null_mut(),
                scheme,
                &POWER_BUTTONS_SUBGROUP,
                &LID_CLOSE_ACTION,
                &mut value,
            )
        };
        check_power(
            result,
            if ac {
                "read the AC lid-close action"
            } else {
                "read the battery lid-close action"
            },
        )?;
        Ok(value)
    }
}

fn power_write(ac: bool, scheme: &Guid, value: u32) -> io::Result<()> {
    unsafe {
        let result = if ac {
            PowerWriteACValueIndex(
                null_mut(),
                scheme,
                &POWER_BUTTONS_SUBGROUP,
                &LID_CLOSE_ACTION,
                value,
            )
        } else {
            PowerWriteDCValueIndex(
                null_mut(),
                scheme,
                &POWER_BUTTONS_SUBGROUP,
                &LID_CLOSE_ACTION,
                value,
            )
        };
        check_power(
            result,
            if ac {
                "write the AC lid-close action"
            } else {
                "write the battery lid-close action"
            },
        )
    }
}

fn power_activate(scheme: &Guid) -> io::Result<()> {
    unsafe {
        check_power(
            PowerSetActiveScheme(null_mut(), scheme),
            "activate the power policy",
        )
    }
}

fn check_power(result: u32, operation: &str) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32)).map_err(|cause| {
            io::Error::new(cause.kind(), format!("Could not {operation}: {cause}"))
        })
    }
}

fn set_execution_state(guarding: bool) -> io::Result<()> {
    let flags = if guarding {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    if unsafe { SetThreadExecutionState(flags) } == 0 {
        Err(error("SetThreadExecutionState failed"))
    } else {
        Ok(())
    }
}

pub fn foreground_editor_window() -> Option<u64> {
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() || IsWindow(window) == 0 || IsIconic(window) != 0 {
            return None;
        }
        let mut process_id = 0;
        if GetWindowThreadProcessId(window, &mut process_id) == 0 || process_id == 0 {
            return None;
        }
        let executable = process_executable_name(process_id)?;
        if !is_supported_editor_process(&executable) {
            return None;
        }
        Some(window as usize as u64)
    }
}

pub fn is_window_focused(window: u64) -> bool {
    let window = window as usize as Hwnd;
    unsafe {
        !window.is_null()
            && IsWindow(window) != 0
            && IsIconic(window) == 0
            && GetForegroundWindow() == window
    }
}

pub fn focus_editor_window(window: u64) -> bool {
    focus_editor_window_with_state(window, false)
}

fn focus_editor_window_with_state(window: u64, maximize: bool) -> bool {
    let window = window as usize as Hwnd;
    unsafe {
        if window.is_null() || IsWindow(window) == 0 {
            return false;
        }
        let mut process_id = 0;
        let target_thread = GetWindowThreadProcessId(window, &mut process_id);
        if target_thread == 0
            || process_id == 0
            || process_executable_name(process_id)
                .as_deref()
                .is_none_or(|executable| !is_supported_editor_process(executable))
        {
            return false;
        }

        let current_thread = GetCurrentThreadId();
        let foreground = GetForegroundWindow();
        let foreground_thread = if foreground.is_null() {
            0
        } else {
            GetWindowThreadProcessId(foreground, null_mut())
        };
        let attached_foreground = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, 1) != 0;
        let attached_target = target_thread != current_thread
            && target_thread != foreground_thread
            && AttachThreadInput(current_thread, target_thread, 1) != 0;

        if maximize {
            ShowWindow(window, 3); // SW_MAXIMIZE
        } else if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        BringWindowToTop(window);
        SetForegroundWindow(window);

        if attached_target {
            AttachThreadInput(current_thread, target_thread, 0);
        }
        if attached_foreground {
            AttachThreadInput(current_thread, foreground_thread, 0);
        }
        GetForegroundWindow() == window
    }
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(window: Hwnd, operation: *const u16, file: *const u16,
        parameters: *const u16, directory: *const u16, show: i32) -> Handle;
}

pub fn activate_overlay_target(target: &crate::overlay::CardTarget) -> bool {
    // Validate the saved HWND before focusing; completed messages can outlive
    // the active-turn entry, so this does not depend on daemon turn lookup.
    if !focus_editor_window_with_state(target.window, true) {
        return false;
    }
    unsafe {
        let window = target.window as usize as Hwnd;
        let mut process_id = 0;
        GetWindowThreadProcessId(window, &mut process_id);
        if let Some(uri) = process_executable_name(process_id)
            .and_then(|executable| overlay_session_uri(&executable, &target.session_id)) {
            let result = ShellExecuteW(window, wide("open").as_ptr(), wide(uri).as_ptr(), null(), null(), SW_SHOW);
            if result as isize <= 32 {
                logging::write("Restored the overlay's editor window, but the chat URI could not be opened.");
                return false;
            }
            return true;
        }
    }
    false
}

fn overlay_session_uri(executable: &str, session_id: &str) -> Option<String> {
    // Only local Codex UUID routes are accepted. Never interpret transcript
    // text or arbitrary IDs as a URL or command.
    if session_id.len() != 36 || !session_id.bytes().enumerate().all(|(index, byte)| {
        if [8, 13, 18, 23].contains(&index) { byte == b'-' } else { byte.is_ascii_hexdigit() }
    }) { return None; }
    let scheme = match executable.to_ascii_lowercase().as_str() {
        "code.exe" => "vscode",
        "code - insiders.exe" => "vscode-insiders",
        _ => return None,
    };
    Some(format!("{scheme}://openai.chatgpt/local/{}", session_id.to_ascii_lowercase()))
}

pub fn show_notification_popup(
    theme: &str,
    title: &str,
    items: &[String],
    initial_index: Option<usize>,
    active_indices: &[usize],
    unviewed_indices: &[usize],
) -> io::Result<Option<usize>> {
    if items.is_empty() {
        return Ok(None);
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(-4isize as Handle);
        let class_name = wide(format!(
            "CodexLidGuardNotificationWindow.{}",
            GetCurrentProcessId()
        ));
        let instance = GetModuleHandleW(null());
        let window_class = WindowClassExW {
            size: size_of::<WindowClassExW>() as u32,
            style: 0,
            window_procedure: Some(notification_popup_window_procedure),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: null_mut(),
            cursor: LoadCursorW(null_mut(), 32_512usize as *const u16),
            background: null_mut(),
            menu_name: null(),
            class_name: class_name.as_ptr(),
            small_icon: null_mut(),
        };
        if RegisterClassExW(&window_class) == 0 {
            return Err(error("RegisterClassExW failed for the notification popup"));
        }

        let previous_foreground = GetForegroundWindow();
        let mut cursor: Point = zeroed();
        if GetCursorPos(&mut cursor) == 0 {
            UnregisterClassW(class_name.as_ptr(), instance);
            return Err(error("GetCursorPos failed"));
        }
        let dpi = if previous_foreground.is_null() {
            96
        } else {
            GetDpiForWindow(previous_foreground).max(96)
        };
        let gap = scale_dip(8, dpi);
        let desired_width = scale_dip(450, dpi);
        let header_height = scale_dip(36, dpi);
        let row_height = scale_dip(34, dpi);
        let rows_height = row_height.saturating_mul(items.len().min(i32::MAX as usize) as i32);
        let popup_height = header_height
            .saturating_add(rows_height)
            .saturating_add(scale_dip(8, dpi));
        let mut editor_bounds: Rect = zeroed();
        if previous_foreground.is_null()
            || GetWindowRect(previous_foreground, &mut editor_bounds) == 0
        {
            editor_bounds = Rect {
                left: cursor.x.saturating_sub(desired_width),
                top: cursor.y.saturating_sub(popup_height),
                right: cursor.x.saturating_add(gap),
                bottom: cursor.y.saturating_add(scale_dip(32, dpi)),
            };
        }
        let available_width = editor_bounds
            .right
            .saturating_sub(editor_bounds.left)
            .saturating_sub(gap.saturating_mul(2))
            .max(1);
        let popup_width = desired_width.min(available_width);
        let popup_bounds =
            notification_popup_bounds(editor_bounds, cursor, dpi, popup_width, popup_height);

        let popup_theme = PopupTheme::parse(theme);
        let mut state = Box::new(NotificationPopupState {
            title: wide(normalize_menu_label(title)),
            items: items
                .iter()
                .map(|item| wide(normalize_menu_label(item)))
                .collect(),
            buttons: Vec::with_capacity(items.len()),
            selected_index: None,
            hovered_index: None,
            active_items: notification_active_items(active_indices, items.len()),
            unviewed_items: notification_active_items(unviewed_indices, items.len()),
            colors: popup_theme.colors(),
            font: GetStockObject(DEFAULT_GUI_FONT),
            dpi,
            header_height,
            closing: false,
        });

        let window_name = wide("Codex Lid Guard active sessions");
        let popup = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT | WS_EX_LAYERED,
            class_name.as_ptr(),
            window_name.as_ptr(),
            WS_POPUP,
            popup_bounds.left,
            popup_bounds.top,
            popup_width,
            popup_height,
            previous_foreground,
            null_mut(),
            instance,
            null(),
        );
        if popup.is_null() {
            UnregisterClassW(class_name.as_ptr(), instance);
            return Err(error("CreateWindowExW failed for the notification popup"));
        }
        SetWindowLongPtrW(
            popup,
            GWLP_USERDATA,
            (&mut *state as *mut NotificationPopupState) as isize,
        );

        if SetLayeredWindowAttributes(popup, 0, popup_theme.opacity(), LWA_ALPHA) == 0 {
            DestroyWindow(popup);
            UnregisterClassW(class_name.as_ptr(), instance);
            return Err(error(
                "SetLayeredWindowAttributes failed for the notification popup",
            ));
        }

        let dark_mode: Bool =
            (popup_theme == PopupTheme::Dark || popup_theme == PopupTheme::HighContrast) as Bool;
        let corner_preference = DWMWCP_ROUND;
        let border_color = state.colors.border;
        let _ = DwmSetWindowAttribute(
            popup,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark_mode).cast(),
            size_of::<Bool>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            popup,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corner_preference).cast(),
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            popup,
            DWMWA_BORDER_COLOR,
            (&raw const border_color).cast(),
            size_of::<u32>() as u32,
        );

        let result = (|| {
            let button_class = wide("BUTTON");
            let horizontal_inset = scale_dip(8, dpi);
            let top_inset = scale_dip(2, dpi);
            let button_width = popup_width.saturating_sub(horizontal_inset.saturating_mul(2));
            for index in 0..state.items.len() {
                let control_id = SESSION_BUTTON_ID + index;
                let button = CreateWindowExW(
                    0,
                    button_class.as_ptr(),
                    state.items[index].as_ptr(),
                    WS_CHILD
                        | WS_VISIBLE
                        | WS_TABSTOP
                        | BS_OWNERDRAW
                        | if index == 0 { WS_GROUP } else { 0 },
                    horizontal_inset,
                    header_height
                        .saturating_add(top_inset)
                        .saturating_add(row_height.saturating_mul(index as i32)),
                    button_width,
                    row_height,
                    popup,
                    control_id as Handle,
                    instance,
                    null(),
                );
                if button.is_null() {
                    return Err(error("CreateWindowExW failed for a session button"));
                }
                SendMessageW(button, WM_SETFONT, state.font as usize, 1);
                if SetWindowSubclass(
                    button,
                    Some(session_button_subclass_procedure),
                    index,
                    popup as usize,
                ) == 0
                {
                    return Err(error("SetWindowSubclass failed for a session button"));
                }
                state.buttons.push(button);
            }

            ShowWindow(popup, SW_SHOW);
            UpdateWindow(popup);
            let current_thread = GetCurrentThreadId();
            let foreground_thread = if previous_foreground.is_null() {
                0
            } else {
                GetWindowThreadProcessId(previous_foreground, null_mut())
            };
            let attached_foreground = foreground_thread != 0
                && foreground_thread != current_thread
                && AttachThreadInput(current_thread, foreground_thread, 1) != 0;
            let focused = SetForegroundWindow(popup) != 0;
            if attached_foreground {
                AttachThreadInput(current_thread, foreground_thread, 0);
            }
            if !focused {
                return Err(error(
                    "SetForegroundWindow failed for the notification popup",
                ));
            }
            BringWindowToTop(popup);
            if let Some(index) = notification_initial_index(initial_index, state.buttons.len()) {
                SetFocus(state.buttons[index]);
            }

            let mut message: Message = zeroed();
            loop {
                let message_result = GetMessageW(&mut message, null_mut(), 0, 0);
                if message_result == -1 {
                    return Err(error("GetMessageW failed for the notification popup"));
                }
                if message_result == 0 {
                    break;
                }
                if message.message == WM_KEYDOWN {
                    match message.wparam {
                        VK_ESCAPE => {
                            close_notification_popup(popup, &mut state, None);
                            continue;
                        }
                        VK_UP => {
                            focus_adjacent_session(&state, -1);
                            continue;
                        }
                        VK_DOWN => {
                            focus_adjacent_session(&state, 1);
                            continue;
                        }
                        VK_TAB => {
                            let direction = if GetKeyState(VK_SHIFT) < 0 { -1 } else { 1 };
                            focus_adjacent_session(&state, direction);
                            continue;
                        }
                        VK_RETURN | VK_SPACE => {
                            let focused_window = GetFocus();
                            let control_id = GetDlgCtrlID(focused_window);
                            if control_id >= SESSION_BUTTON_ID as i32 {
                                SendMessageW(
                                    popup,
                                    WM_COMMAND,
                                    control_id as usize,
                                    focused_window as isize,
                                );
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            Ok(state.selected_index)
        })();
        if IsWindow(popup) != 0 {
            DestroyWindow(popup);
        }
        if !previous_foreground.is_null() && IsWindow(previous_foreground) != 0 {
            SetForegroundWindow(previous_foreground);
        }
        UnregisterClassW(class_name.as_ptr(), instance);
        result
    }
}

impl PopupTheme {
    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "high-contrast" => Self::HighContrast,
            "high-contrast-light" => Self::HighContrastLight,
            _ => Self::Dark,
        }
    }

    fn colors(self) -> PopupColors {
        match self {
            Self::Dark => PopupColors {
                background: color_ref(37, 37, 38),
                border: color_ref(68, 68, 68),
                text: color_ref(204, 204, 204),
                muted_text: color_ref(156, 156, 156),
                hover_background: color_ref(48, 48, 48),
                hover_text: color_ref(255, 255, 255),
                active_background: color_ref(75, 59, 16),
                active_hover_background: color_ref(101, 77, 13),
                active_text: color_ref(255, 220, 110),
                unviewed_background: color_ref(18, 72, 107),
                unviewed_hover_background: color_ref(24, 96, 142),
                unviewed_text: color_ref(230, 246, 255),
            },
            Self::Light => PopupColors {
                background: color_ref(248, 248, 248),
                border: color_ref(200, 200, 200),
                text: color_ref(51, 51, 51),
                muted_text: color_ref(97, 97, 97),
                hover_background: color_ref(229, 229, 229),
                hover_text: color_ref(0, 0, 0),
                active_background: color_ref(255, 235, 166),
                active_hover_background: color_ref(255, 220, 112),
                active_text: color_ref(86, 58, 0),
                unviewed_background: color_ref(0, 120, 212),
                unviewed_hover_background: color_ref(0, 90, 158),
                unviewed_text: color_ref(255, 255, 255),
            },
            Self::HighContrast => PopupColors {
                background: color_ref(0, 0, 0),
                border: color_ref(255, 255, 255),
                text: color_ref(255, 255, 255),
                muted_text: color_ref(255, 255, 255),
                hover_background: color_ref(0, 128, 128),
                hover_text: color_ref(255, 255, 255),
                active_background: color_ref(128, 64, 0),
                active_hover_background: color_ref(176, 88, 0),
                active_text: color_ref(255, 255, 0),
                unviewed_background: color_ref(0, 64, 128),
                unviewed_hover_background: color_ref(0, 96, 176),
                unviewed_text: color_ref(255, 255, 255),
            },
            Self::HighContrastLight => PopupColors {
                background: color_ref(255, 255, 255),
                border: color_ref(0, 0, 0),
                text: color_ref(0, 0, 0),
                muted_text: color_ref(0, 0, 0),
                hover_background: color_ref(0, 0, 128),
                hover_text: color_ref(255, 255, 255),
                active_background: color_ref(255, 225, 128),
                active_hover_background: color_ref(255, 192, 0),
                active_text: color_ref(0, 0, 0),
                unviewed_background: color_ref(0, 0, 128),
                unviewed_hover_background: color_ref(0, 0, 200),
                unviewed_text: color_ref(255, 255, 255),
            },
        }
    }

    fn opacity(self) -> u8 {
        match self {
            Self::HighContrast | Self::HighContrastLight => 255,
            Self::Dark | Self::Light => 235,
        }
    }
}

fn normalize_menu_label(value: &str) -> String {
    value.replace(['\r', '\n', '\t'], " ")
}

fn notification_initial_index(requested: Option<usize>, item_count: usize) -> Option<usize> {
    if item_count == 0 {
        return None;
    }
    Some(requested.filter(|index| *index < item_count).unwrap_or(0))
}

fn notification_active_items(indices: &[usize], item_count: usize) -> Vec<bool> {
    let mut active = vec![false; item_count];
    for index in indices.iter().copied().filter(|index| *index < item_count) {
        active[index] = true;
    }
    active
}

fn scale_dip(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi.max(96)) + 48) / 96)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn notification_popup_bounds(
    editor: Rect,
    cursor: Point,
    dpi: u32,
    width: i32,
    height: i32,
) -> Rect {
    let gap = scale_dip(8, dpi);
    let status_click_offset = scale_dip(19, dpi);
    let status_band_top = editor.bottom.saturating_sub(scale_dip(80, dpi));
    let cursor_is_in_status_band = cursor.x >= editor.left
        && cursor.x <= editor.right
        && cursor.y >= status_band_top
        && cursor.y <= editor.bottom;
    let desired_bottom = if cursor_is_in_status_band {
        cursor.y.saturating_sub(status_click_offset)
    } else {
        editor.bottom.saturating_sub(scale_dip(34, dpi))
    };
    let right = editor.right.saturating_sub(gap);
    let left = right
        .saturating_sub(width)
        .max(editor.left.saturating_add(gap));
    let top = desired_bottom
        .saturating_sub(height)
        .max(editor.top.saturating_add(gap));
    Rect {
        left,
        top,
        right: left.saturating_add(width),
        bottom: top.saturating_add(height),
    }
}

unsafe extern "system" fn notification_popup_window_procedure(
    window: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    unsafe {
        let state_pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut NotificationPopupState;
        if state_pointer.is_null() {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let state = &mut *state_pointer;
        match message {
            WM_PAINT => {
                paint_notification_popup(window, state);
                return 1;
            }
            WM_ERASEBKGND => return 1,
            WM_DRAWITEM if lparam != 0 => {
                let draw = &*(lparam as *const DrawItemStruct);
                if draw.control_type == ODT_BUTTON {
                    draw_notification_session(draw, state);
                    return 1;
                }
            }
            WM_COMMAND => {
                let notification = (wparam >> 16) & 0xffff;
                let control_id = wparam & 0xffff;
                if notification == BN_CLICKED && control_id >= SESSION_BUTTON_ID {
                    let selected = control_id - SESSION_BUTTON_ID;
                    if selected < state.items.len() {
                        close_notification_popup(window, state, Some(selected));
                        return 0;
                    }
                }
            }
            WM_APP_HOVER_SESSION => {
                let next = (wparam != NO_HOVERED_SESSION).then_some(wparam);
                if next != state.hovered_index {
                    if let Some(previous) = state.hovered_index
                        && let Some(button) = state.buttons.get(previous)
                    {
                        InvalidateRect(*button, null(), 0);
                    }
                    state.hovered_index = next;
                    if let Some(next) = next
                        && let Some(button) = state.buttons.get(next)
                    {
                        InvalidateRect(*button, null(), 0);
                    }
                }
                return 0;
            }
            WM_ACTIVATE if (wparam & 0xffff) == WA_INACTIVE && !state.closing => {
                close_notification_popup(window, state, None);
                return 0;
            }
            WM_CLOSE => {
                close_notification_popup(window, state, None);
                return 0;
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

unsafe extern "system" fn session_button_subclass_procedure(
    window: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    subclass_id: usize,
    parent: usize,
) -> Lresult {
    unsafe {
        match message {
            WM_MOUSEMOVE => {
                SendMessageW(parent as Hwnd, WM_APP_HOVER_SESSION, subclass_id, 0);
                let mut tracking = TrackMouseEvent {
                    size: size_of::<TrackMouseEvent>() as u32,
                    flags: TME_LEAVE,
                    track_window: window,
                    hover_time: 0,
                };
                TrackMouseEvent(&mut tracking);
            }
            WM_MOUSELEAVE => {
                SendMessageW(parent as Hwnd, WM_APP_HOVER_SESSION, NO_HOVERED_SESSION, 0);
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(window, Some(session_button_subclass_procedure), subclass_id);
            }
            _ => {}
        }
        DefSubclassProc(window, message, wparam, lparam)
    }
}

unsafe fn close_notification_popup(
    window: Hwnd,
    state: &mut NotificationPopupState,
    selection: Option<usize>,
) {
    if state.closing {
        return;
    }
    state.selected_index = selection;
    state.closing = true;
    unsafe {
        DestroyWindow(window);
    }
}

unsafe fn focus_adjacent_session(state: &NotificationPopupState, direction: isize) {
    if state.buttons.is_empty() {
        return;
    }
    unsafe {
        let focused = GetFocus();
        let current = state
            .buttons
            .iter()
            .position(|button| *button == focused)
            .unwrap_or(0);
        let count = state.buttons.len() as isize;
        let next = (current as isize + direction).rem_euclid(count) as usize;
        SetFocus(state.buttons[next]);
        InvalidateRect(state.buttons[current], null(), 0);
        InvalidateRect(state.buttons[next], null(), 0);
    }
}

unsafe fn paint_notification_popup(window: Hwnd, state: &NotificationPopupState) {
    unsafe {
        let mut paint: PaintStruct = zeroed();
        let device_context = BeginPaint(window, &mut paint);
        if device_context.is_null() {
            return;
        }
        let mut client: Rect = zeroed();
        GetClientRect(window, &mut client);
        fill_rectangle(device_context, &client, state.colors.background);

        let border_brush = CreateSolidBrush(state.colors.border);
        if !border_brush.is_null() {
            FrameRect(device_context, &client, border_brush);
            DeleteObject(border_brush);
        }

        SetBkMode(device_context, TRANSPARENT);
        SetTextColor(device_context, state.colors.muted_text);
        let previous_font = if state.font.is_null() {
            null_mut()
        } else {
            SelectObject(device_context, state.font)
        };

        let mut title_rect = Rect {
            left: scale_dip(16, state.dpi),
            top: 0,
            right: client.right.saturating_sub(scale_dip(16, state.dpi)),
            bottom: state.header_height,
        };
        DrawTextW(
            device_context,
            state.title.as_ptr(),
            wide_text_length(&state.title),
            &mut title_rect,
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );

        if !previous_font.is_null() {
            SelectObject(device_context, previous_font);
        }
        EndPaint(window, &paint);
    }
}

unsafe fn draw_notification_session(draw: &DrawItemStruct, state: &NotificationPopupState) {
    unsafe {
        let control_id = GetDlgCtrlID(draw.item_window);
        let Some(index) = (control_id >= SESSION_BUTTON_ID as i32)
            .then_some(control_id as usize - SESSION_BUTTON_ID)
            .filter(|index| *index < state.items.len())
        else {
            return;
        };
        let pressed = draw.item_state & ODS_SELECTED != 0;
        let focused = draw.item_state & ODS_FOCUS != 0;
        let hovered = state.hovered_index == Some(index);
        let highlighted = pressed || hovered || (state.hovered_index.is_none() && focused);
        let active = state.active_items.get(index).copied().unwrap_or(false);
        let unviewed = !active && state.unviewed_items.get(index).copied().unwrap_or(false);
        fill_rectangle(
            draw.device_context,
            &draw.item_rect,
            state.colors.background,
        );
        if highlighted || active || unviewed {
            let mut pill = draw.item_rect;
            let vertical_inset = scale_dip(2, state.dpi);
            pill.top += vertical_inset;
            pill.bottom -= vertical_inset;
            fill_rounded_rectangle(
                draw.device_context,
                &pill,
                if active {
                    if highlighted {
                        state.colors.active_hover_background
                    } else {
                        state.colors.active_background
                    }
                } else if unviewed {
                    if highlighted {
                        state.colors.unviewed_hover_background
                    } else {
                        state.colors.unviewed_background
                    }
                } else {
                    state.colors.hover_background
                },
                scale_dip(14, state.dpi),
            );
        }

        SetBkMode(draw.device_context, TRANSPARENT);
        let text_color = if active {
            state.colors.active_text
        } else if unviewed {
            state.colors.unviewed_text
        } else if highlighted {
            state.colors.hover_text
        } else {
            state.colors.text
        };
        SetTextColor(draw.device_context, text_color);
        let previous_font = if state.font.is_null() {
            null_mut()
        } else {
            SelectObject(draw.device_context, state.font)
        };
        let mut text_rect = draw.item_rect;
        text_rect.left += scale_dip(12, state.dpi);
        text_rect.right -= scale_dip(12, state.dpi);
        DrawTextW(
            draw.device_context,
            state.items[index].as_ptr(),
            wide_text_length(&state.items[index]),
            &mut text_rect,
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        if !previous_font.is_null() {
            SelectObject(draw.device_context, previous_font);
        }
    }
}

unsafe fn fill_rectangle(device_context: Handle, rectangle: &Rect, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        if !brush.is_null() {
            FillRect(device_context, rectangle, brush);
            DeleteObject(brush);
        }
    }
}

unsafe fn fill_rounded_rectangle(
    device_context: Handle,
    rectangle: &Rect,
    color: u32,
    radius: i32,
) {
    unsafe {
        let brush = CreateSolidBrush(color);
        if brush.is_null() {
            return;
        }
        let previous_brush = SelectObject(device_context, brush);
        let null_pen = GetStockObject(NULL_PEN);
        let previous_pen = if null_pen.is_null() {
            null_mut()
        } else {
            SelectObject(device_context, null_pen)
        };
        RoundRect(
            device_context,
            rectangle.left,
            rectangle.top,
            rectangle.right,
            rectangle.bottom,
            radius.saturating_mul(2),
            radius.saturating_mul(2),
        );
        if !previous_pen.is_null() {
            SelectObject(device_context, previous_pen);
        }
        if !previous_brush.is_null() {
            SelectObject(device_context, previous_brush);
        }
        DeleteObject(brush);
    }
}

fn wide_text_length(value: &[u16]) -> i32 {
    value.len().saturating_sub(1).min(i32::MAX as usize) as i32
}

const fn color_ref(red: u8, green: u8, blue: u8) -> u32 {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

fn process_executable_name(process_id: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process.is_null() {
            return None;
        }
        let process = OwnedHandle(process);
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        if QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &mut length) == 0 {
            return None;
        }
        Path::new(&String::from_utf16_lossy(&buffer[..length as usize]))
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
    }
}

fn is_supported_editor_process(executable: &str) -> bool {
    matches!(
        executable.to_ascii_lowercase().as_str(),
        "code.exe"
            | "code - insiders.exe"
            | "code - oss.exe"
            | "codium.exe"
            | "vscodium.exe"
            | "cursor.exe"
            | "windsurf.exe"
    )
}

pub fn atomic_write(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let extension = destination
        .extension()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    let temporary = destination.with_extension(format!("{extension}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    let source_wide = wide(temporary.as_os_str());
    let destination_wide = wide(destination.as_os_str());
    if unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(error("could not atomically replace file"));
    }
    Ok(())
}

fn save_recovery(state: &SavedPowerState) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(state)?;
    atomic_write(&paths::recovery_file(), &bytes)
}

fn load_recovery() -> Option<SavedPowerState> {
    let bytes = std::fs::read(paths::recovery_file()).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(state) => Some(state),
        Err(cause) => {
            logging::write(format!("Could not read the recovery state: {cause}"));
            None
        }
    }
}

fn restore_power_state(state: &SavedPowerState) -> io::Result<()> {
    if state.changed_lid_policy() {
        let scheme = Guid::parse(&state.scheme).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the recovery record contains an invalid power scheme GUID",
            )
        })?;
        if state.changed_ac_lid_action() {
            power_write(true, &scheme, state.ac_lid_action)?;
        }
        if state.changed_dc_lid_action() {
            power_write(false, &scheme, state.dc_lid_action)?;
        }
        power_activate(&scheme)?;
    }
    match std::fs::remove_file(paths::recovery_file()) {
        Ok(()) => Ok(()),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(cause),
    }
}

type LidCallback = Arc<dyn Fn(LidState) + Send + Sync + 'static>;
static LID_CALLBACK: OnceLock<Mutex<Option<LidCallback>>> = OnceLock::new();

pub struct LidWatcher {
    window: isize,
    thread: Option<JoinHandle<()>>,
}

impl LidWatcher {
    pub fn start(callback: impl Fn(LidState) + Send + Sync + 'static) -> io::Result<Self> {
        let callback: LidCallback = Arc::new(callback);
        let gate = LID_CALLBACK.get_or_init(|| Mutex::new(None));
        if let Ok(mut slot) = gate.lock() {
            *slot = Some(callback);
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("Codex Lid Guard lid-state listener".to_string())
            .spawn(move || lid_message_loop(sender))?;
        match receiver.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(window)) => Ok(Self {
                window,
                thread: Some(thread),
            }),
            Ok(Err(message)) => {
                let _ = thread.join();
                clear_lid_callback();
                Err(io::Error::other(message))
            }
            Err(_) => {
                clear_lid_callback();
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out while registering for Windows lid-state notifications",
                ))
            }
        }
    }
}

impl Drop for LidWatcher {
    fn drop(&mut self) {
        if self.window != 0 {
            unsafe {
                PostMessageW(self.window as Hwnd, WM_CLOSE, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        clear_lid_callback();
    }
}

fn clear_lid_callback() {
    if let Some(gate) = LID_CALLBACK.get()
        && let Ok(mut slot) = gate.lock()
    {
        *slot = None;
    }
}

fn lid_message_loop(ready: mpsc::SyncSender<Result<isize, String>>) {
    unsafe {
        let class_name = wide(format!("CodexLidGuardWindow.{}", GetCurrentProcessId()));
        let window_name = wide("Codex Lid Guard");
        let instance = GetModuleHandleW(null());
        let window_class = WindowClassExW {
            size: size_of::<WindowClassExW>() as u32,
            style: 0,
            window_procedure: Some(lid_window_procedure),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: null_mut(),
            cursor: null_mut(),
            background: null_mut(),
            menu_name: null(),
            class_name: class_name.as_ptr(),
            small_icon: null_mut(),
        };
        if RegisterClassExW(&window_class) == 0 {
            let _ = ready.send(Err(format!(
                "RegisterClassExW failed: {}",
                io::Error::last_os_error()
            )));
            return;
        }
        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            -3isize as Hwnd,
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            let _ = ready.send(Err(format!(
                "CreateWindowExW failed: {}",
                io::Error::last_os_error()
            )));
            UnregisterClassW(class_name.as_ptr(), instance);
            return;
        }
        let notification = RegisterPowerSettingNotification(
            window,
            &LID_SWITCH_STATE_CHANGE,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        );
        if notification.is_null() {
            let _ = ready.send(Err(format!(
                "RegisterPowerSettingNotification failed: {}",
                io::Error::last_os_error()
            )));
            DestroyWindow(window);
            UnregisterClassW(class_name.as_ptr(), instance);
            return;
        }
        let _ = ready.send(Ok(window as isize));
        let mut message: Message = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnregisterPowerSettingNotification(notification);
        DestroyWindow(window);
        UnregisterClassW(class_name.as_ptr(), instance);
    }
}

unsafe extern "system" fn lid_window_procedure(
    window: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    unsafe {
        if message == WM_POWERBROADCAST && wparam == PBT_POWERSETTINGCHANGE && lparam != 0 {
            let bytes = lparam as *const u8;
            let setting = std::ptr::read_unaligned(bytes.cast::<Guid>());
            let length = std::ptr::read_unaligned(bytes.add(size_of::<Guid>()).cast::<u32>());
            if setting == LID_SWITCH_STATE_CHANGE && length >= 1 {
                let value = *bytes.add(size_of::<Guid>() + size_of::<u32>());
                let state = if value == 0 {
                    LidState::Closed
                } else {
                    LidState::Open
                };
                let callback = LID_CALLBACK
                    .get()
                    .and_then(|gate| gate.lock().ok())
                    .and_then(|slot| slot.clone());
                if let Some(callback) = callback {
                    callback(state);
                }
            }
            return 0;
        }
        if message == WM_CLOSE {
            DestroyWindow(window);
            return 0;
        }
        if message == WM_DESTROY {
            PostQuitMessage(0);
            return 0;
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

pub fn terminate_other_helpers() -> Vec<u32> {
    let mut stopped = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return stopped;
        }
        let snapshot = OwnedHandle(snapshot);
        let mut entry: ProcessEntry32W = zeroed();
        entry.size = size_of::<ProcessEntry32W>() as u32;
        let current = GetCurrentProcessId();
        let mut available = Process32FirstW(snapshot.0, &mut entry) != 0;
        while available {
            if entry.process_id != current
                && wide_string(&entry.exe_file).eq_ignore_ascii_case("CodexLidGuard.exe")
            {
                let process = OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, entry.process_id);
                if !process.is_null() {
                    let process = OwnedHandle(process);
                    if TerminateProcess(process.0, 0) != 0 {
                        let _ = WaitForSingleObject(process.0, 2_000);
                        stopped.push(entry.process_id);
                    }
                }
            }
            available = Process32NextW(snapshot.0, &mut entry) != 0;
        }
    }
    stopped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_chat_routes_are_local_and_validate_the_session_id() {
        let id = "01A07039-FCA0-7CC0-BC95-6A98923C844E";
        assert_eq!(overlay_session_uri("Code.exe", id).as_deref(), Some("vscode://openai.chatgpt/local/01a07039-fca0-7cc0-bc95-6a98923c844e"));
        assert!(overlay_session_uri("Code - Insiders.exe", id).unwrap().starts_with("vscode-insiders://"));
        assert!(overlay_session_uri("unrelated.exe", id).is_none());
        for invalid in ["", "https://example.com", "../other", "01a07039-fca0-7cc0-bc95-6a98923c844g"] {
            assert!(overlay_session_uri("Code.exe", invalid).is_none());
        }
    }

    #[test]
    fn guid_round_trip() {
        let input = "381b4222-f694-41f0-9685-ff5bb260df2e";
        assert_eq!(Guid::parse(input).unwrap().to_string(), input);
    }

    #[test]
    fn session_identity_is_available() {
        assert!(current_user_sid().is_some());
    }

    #[test]
    fn supported_editor_processes_cover_vscode_and_common_builds() {
        assert!(is_supported_editor_process("Code.exe"));
        assert!(is_supported_editor_process("Code - Insiders.exe"));
        assert!(is_supported_editor_process("VSCodium.exe"));
        assert!(is_supported_editor_process("Cursor.exe"));
        assert!(!is_supported_editor_process("notepad.exe"));
    }

    #[test]
    fn isolated_test_power_policy_cannot_touch_production_power_state() {
        let policy = PowerPolicy::new_for_test();
        assert!(!policy.recovery_is_enabled());
        assert!(!policy.system_changes_are_enabled());
    }

    #[test]
    fn native_menu_labels_preserve_ampersands_and_remove_control_characters() {
        assert_eq!(
            normalize_menu_label("Research & Development\nSession"),
            "Research & Development Session"
        );
    }

    #[test]
    fn notification_popup_focuses_the_requested_valid_session() {
        assert_eq!(notification_initial_index(Some(3), 5), Some(3));
        assert_eq!(notification_initial_index(Some(8), 5), Some(0));
        assert_eq!(notification_initial_index(None, 5), Some(0));
        assert_eq!(notification_initial_index(Some(0), 0), None);
    }

    #[test]
    fn notification_popup_marks_all_valid_active_sessions() {
        assert_eq!(
            notification_active_items(&[0, 2, 2, 8], 4),
            vec![true, false, true, false]
        );
        assert!(notification_active_items(&[0], 0).is_empty());
    }

    #[test]
    fn notification_popup_is_right_aligned_above_the_status_bar() {
        let bounds = notification_popup_bounds(
            Rect {
                left: 0,
                top: 0,
                right: 1230,
                bottom: 715,
            },
            Point { x: 1100, y: 694 },
            144,
            675,
            114,
        );

        assert_eq!(
            bounds,
            Rect {
                left: 543,
                top: 551,
                right: 1218,
                bottom: 665,
            }
        );
    }

    #[test]
    fn notification_popup_theme_follows_vscode_theme_kind() {
        assert_eq!(PopupTheme::parse("light"), PopupTheme::Light);
        assert_eq!(PopupTheme::parse("high-contrast"), PopupTheme::HighContrast);
        assert_eq!(
            PopupTheme::parse("high-contrast-light"),
            PopupTheme::HighContrastLight
        );
        assert_eq!(PopupTheme::parse("unknown"), PopupTheme::Dark);
        assert_eq!(PopupTheme::Dark.opacity(), 235);
        assert_eq!(PopupTheme::Light.opacity(), 235);
        assert_eq!(PopupTheme::HighContrast.opacity(), 255);
        assert_eq!(PopupTheme::HighContrastLight.opacity(), 255);
    }

    #[test]
    fn unviewed_completion_colors_are_distinct_in_every_theme() {
        for theme in [
            PopupTheme::Dark,
            PopupTheme::Light,
            PopupTheme::HighContrast,
            PopupTheme::HighContrastLight,
        ] {
            let colors = theme.colors();
            assert_ne!(colors.unviewed_background, colors.background);
            assert_ne!(colors.unviewed_background, colors.active_background);
            assert_ne!(colors.unviewed_hover_background, colors.unviewed_background);
        }
    }

    #[test]
    fn atomic_write_replaces_an_existing_snapshot() {
        let directory = std::env::temp_dir().join(format!(
            "codex-lid-guard-atomic-write-test-{}",
            std::process::id()
        ));
        let destination = directory.join("status.json");
        atomic_write(&destination, b"first").unwrap();
        atomic_write(&destination, b"second").unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"second");
        std::fs::remove_file(destination).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn legacy_recovery_records_infer_only_the_lid_actions_that_changed() {
        let state: SavedPowerState = serde_json::from_str(
            r#"{
                "Scheme":"381b4222-f694-41f0-9685-ff5bb260df2e",
                "AcLidAction":1,
                "DcLidAction":0,
                "CapturedAt":"now"
            }"#,
        )
        .unwrap();
        assert!(state.changed_ac_lid_action());
        assert!(!state.changed_dc_lid_action());
        assert!(state.changed_lid_policy());
    }

    #[test]
    fn current_recovery_records_preserve_explicit_change_flags() {
        let state = SavedPowerState {
            scheme: "381b4222-f694-41f0-9685-ff5bb260df2e".into(),
            ac_lid_action: 1,
            dc_lid_action: 1,
            changed_ac_lid_action: Some(false),
            changed_dc_lid_action: Some(false),
            captured_at: "now".into(),
        };
        let round_trip: SavedPowerState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(!round_trip.changed_lid_policy());
    }

    #[test]
    #[ignore = "requires Node and compiled extension JavaScript; uses an isolated test pipe"]
    fn native_pipe_exchanges_repeatedly_with_the_extension_socket_client() {
        use std::os::windows::process::CommandExt;
        let name = format!(r"\\.\pipe\CodexLidGuard.{:016X}", std::process::id());
        let server_name = name.clone();
        let server = std::thread::spawn(move || {
            let server = PipeServer::new(&server_name);
            for _ in 0..50 {
                let AcceptResult::Connected(connection) = server.accept(Some(Duration::from_secs(3))).unwrap()
                    else { panic!("extension test client did not connect"); };
                let request: serde_json::Value = serde_json::from_str(&connection.read_line().unwrap()).unwrap();
                assert_eq!(request["action"], "status");
                connection.write_line(r#"{"ok":true,"message":"ready","activeTurns":0,"isGuarding":false,"lidState":"open","sleepPending":false}"#).unwrap();
                // No flush, acknowledgement, or waiting for the peer: close preserves the queued reply.
            }
        });
        let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extension/dist/src/helper.js");
        let result = std::process::Command::new("node").args([
            "-e",
            "const {warmGuardianPipe}=require(process.argv[1]);(async()=>{const times=[];for(let i=0;i<50;i++){const start=performance.now();await warmGuardianPipe(process.argv[2],'0.1.48');times.push(performance.now()-start);}times.sort((a,b)=>a-b);console.log(JSON.stringify({nativePipeRequests:50,failures:0,medianMs:times[25],maxMs:times[49]}));})().catch(error=>{console.error(error);process.exitCode=1});",
        ]).arg(helper).arg(name).creation_flags(0x0800_0000).status().unwrap();
        server.join().unwrap();
        assert!(result.success());
    }

    #[test]
    fn pipe_reply_survives_a_client_that_starts_reading_after_the_server_writes() {
        let pipe_name = format!(r"\\.\pipe\CodexLidGuard.DelayedRead.{}", std::process::id());
        let server_name = pipe_name.clone();
        let (sent, written) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let server = PipeServer::new(&server_name);
            let AcceptResult::Connected(connection) = server.accept(Some(Duration::from_secs(2))).unwrap()
                else { panic!("test client did not connect"); };
            assert_eq!(connection.read_line().unwrap(), "request");
            connection.write_line("response").unwrap();
            drop(connection);
            sent.send(()).unwrap();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let connection = loop {
            match connect_pipe(&pipe_name, Duration::from_millis(10), Duration::from_secs(1)) {
                Ok(connection) => break connection,
                Err(_) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                Err(cause) => panic!("test client could not connect: {cause}"),
            }
        };
        connection.write_line("request").unwrap();
        written.recv_timeout(Duration::from_secs(1)).unwrap();
        let response = connection.read_line().unwrap();
        drop(connection);
        server.join().unwrap();
        assert_eq!(response, "response", "disconnect must not discard the queued reply");
    }

    #[test]
    fn pipe_response_timeout_is_independent_from_the_connection_timeout() {
        let pipe_name = format!(r"\\.\pipe\CodexLidGuard.TimeoutTest.{}", std::process::id());
        let server_name = pipe_name.clone();
        let server = std::thread::spawn(move || {
            let server = PipeServer::new(&server_name);
            if let AcceptResult::Connected(_connection) = server
                .accept(Some(Duration::from_secs(2)))
                .expect("test server should accept")
            {
                std::thread::sleep(Duration::from_millis(250));
            }
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let connection = loop {
            match connect_pipe(
                &pipe_name,
                Duration::from_millis(10),
                Duration::from_millis(100),
            ) {
                Ok(connection) => break connection,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(cause) => panic!("test client could not connect: {cause}"),
            }
        };
        connection.write_line("{\"action\":\"status\"}").unwrap();
        let started = std::time::Instant::now();
        let error = connection.read_line().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(75));
        assert!(started.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }
}
