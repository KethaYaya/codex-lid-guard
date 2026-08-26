#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, c_void};
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::logging;
use crate::model::LidState;
use crate::paths;

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
const SYNCHRONIZE: u32 = 0x0010_0000;
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
const ES_CONTINUOUS: u32 = 0x8000_0000;
const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_CLOSE: u32 = 0x0010;
const WM_DESTROY: u32 = 0x0002;
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
    fn DisconnectNamedPipe(pipe: Handle) -> Bool;
    fn GetCurrentProcess() -> Handle;
    fn GetCurrentProcessId() -> u32;
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
    fn DestroyWindow(window: Hwnd) -> Bool;
    fn DispatchMessageW(message: *const Message) -> Lresult;
    fn GetMessageW(message: *mut Message, window: Hwnd, min: u32, max: u32) -> Bool;
    fn PostMessageW(window: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Bool;
    fn PostQuitMessage(exit_code: i32);
    fn RegisterClassExW(window_class: *const WindowClassExW) -> u16;
    fn RegisterPowerSettingNotification(
        recipient: Handle,
        setting: *const Guid,
        flags: u32,
    ) -> Handle;
    fn TranslateMessage(message: *const Message) -> Bool;
    fn UnregisterClassW(class_name: *const u16, instance: Handle) -> Bool;
    fn UnregisterPowerSettingNotification(handle: Handle) -> Bool;
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
    io::Error::new(
        io::ErrorKind::Other,
        format!("{operation}: {}", io::Error::last_os_error()),
    )
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
                disconnect_on_drop: true,
                io_timeout: Some(Duration::from_secs(5)),
            }))
        }
    }
}

pub struct PipeConnection {
    handle: OwnedHandle,
    overlapped: bool,
    disconnect_on_drop: bool,
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

impl Drop for PipeConnection {
    fn drop(&mut self) {
        if self.disconnect_on_drop {
            unsafe {
                DisconnectNamedPipe(self.handle.0);
            }
        }
    }
}

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

pub fn connect_pipe(name: &str, timeout: Duration) -> io::Result<PipeConnection> {
    let name = wide(name);
    let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
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
            disconnect_on_drop: false,
            io_timeout: Some(timeout),
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
    #[serde(rename = "CapturedAt", alias = "capturedAt")]
    captured_at: String,
}

pub struct PowerPolicy {
    saved: Option<SavedPowerState>,
    guarding: bool,
}

impl PowerPolicy {
    pub fn new() -> Self {
        let mut value = Self {
            saved: None,
            guarding: false,
        };
        value.restore_stale();
        value
    }

    pub fn is_guarding(&self) -> bool {
        self.guarding
    }

    pub fn acquire(&mut self) -> io::Result<()> {
        if self.guarding {
            return Ok(());
        }
        let scheme = active_scheme()?;
        let ac = power_read(true, &scheme)?;
        let dc = power_read(false, &scheme)?;
        let saved = SavedPowerState {
            scheme: scheme.to_string(),
            ac_lid_action: ac,
            dc_lid_action: dc,
            captured_at: local_timestamp(),
        };
        save_recovery(&saved)?;
        self.saved = Some(saved.clone());
        let result = (|| {
            power_write(true, &scheme, 0)?;
            power_write(false, &scheme, 0)?;
            power_activate(&scheme)?;
            set_execution_state(true)?;
            Ok(())
        })();
        if let Err(cause) = result {
            let _ = restore_power_state(&saved);
            self.saved = None;
            return Err(cause);
        }
        self.guarding = true;
        logging::write(format!(
            "Guard acquired for power scheme {scheme}; original lid actions AC={ac}, DC={dc}."
        ));
        Ok(())
    }

    pub fn release(&mut self) -> io::Result<()> {
        let recovery_exists = paths::recovery_file().exists();
        if !self.guarding && self.saved.is_none() && !recovery_exists {
            return Ok(());
        }
        let _ = set_execution_state(false);
        let state = self.saved.clone().or_else(load_recovery);
        if let Some(state) = state {
            restore_power_state(&state)?;
            logging::write(format!(
                "Restored power scheme {} lid actions AC={}, DC={}.",
                state.scheme, state.ac_lid_action, state.dc_lid_action
            ));
        }
        self.saved = None;
        self.guarding = false;
        Ok(())
    }

    fn restore_stale(&mut self) {
        let Some(stale) = load_recovery() else {
            return;
        };
        logging::write("Found an interrupted guard session; restoring its saved lid policy.");
        if let Err(cause) = restore_power_state(&stale) {
            logging::write(format!("Could not restore the saved power policy: {cause}"));
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
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "PowerGetActiveScheme returned no scheme",
            ));
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

fn save_recovery(state: &SavedPowerState) -> io::Result<()> {
    std::fs::create_dir_all(paths::data_directory())?;
    let destination = paths::recovery_file();
    let temporary = destination.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
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
        return Err(error("could not atomically save the power recovery record"));
    }
    Ok(())
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
    let scheme = Guid::parse(&state.scheme).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the recovery record contains an invalid power scheme GUID",
        )
    })?;
    power_write(true, &scheme, state.ac_lid_action)?;
    power_write(false, &scheme, state.dc_lid_action)?;
    power_activate(&scheme)?;
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
                Err(io::Error::new(io::ErrorKind::Other, message))
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
    if let Some(gate) = LID_CALLBACK.get() {
        if let Ok(mut slot) = gate.lock() {
            *slot = None;
        }
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
    fn guid_round_trip() {
        let input = "381b4222-f694-41f0-9685-ff5bb260df2e";
        assert_eq!(Guid::parse(input).unwrap().to_string(), input);
    }

    #[test]
    fn session_identity_is_available() {
        assert!(current_user_sid().is_some());
    }

    #[test]
    fn pipe_reads_time_out_if_a_connected_daemon_stalls() {
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
            match connect_pipe(&pipe_name, Duration::from_millis(50)) {
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
        assert!(started.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }
}
