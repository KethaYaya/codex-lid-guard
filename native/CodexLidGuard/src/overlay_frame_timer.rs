//! Accurate frame deadlines without changing the system timer resolution or polling while idle.
use super::*;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateWaitableTimerExW(
        attributes: *const c_void,
        name: *const u16,
        flags: u32,
        access: u32,
    ) -> Handle;
    fn SetWaitableTimer(
        timer: Handle,
        due: *const i64,
        period: i32,
        callback: *const c_void,
        argument: *const c_void,
        resume: Bool,
    ) -> Bool;
    fn CancelWaitableTimer(timer: Handle) -> Bool;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MsgWaitForMultipleObjectsEx(
        count: u32,
        handles: *const Handle,
        timeout: u32,
        mask: u32,
        flags: u32,
    ) -> u32;
    fn PeekMessageW(
        message: *mut Message,
        window: Hwnd,
        first: u32,
        last: u32,
        remove: u32,
    ) -> Bool;
}

pub(super) struct FrameTimer {
    handle: Handle,
    next: Option<Instant>,
}

impl FrameTimer {
    const PERIOD: Duration = Duration::from_nanos(16_666_667);

    pub(super) unsafe fn new() -> io::Result<Self> {
        unsafe {
            // High-resolution timers are supported by Windows 10 1803 and newer.
            let mut handle = CreateWaitableTimerExW(null(), null(), 2, 0x001f0003);
            if handle.is_null() {
                handle = CreateWaitableTimerExW(null(), null(), 0, 0x001f0003);
            }
            if handle.is_null() {
                return Err(error("Create overlay frame timer"));
            }
            Ok(Self { handle, next: None })
        }
    }

    pub(super) unsafe fn update(&mut self, active: bool) -> io::Result<()> {
        unsafe {
            if !active {
                if self.next.take().is_some() {
                    CancelWaitableTimer(self.handle);
                }
                return Ok(());
            }
            let now = Instant::now();
            let mut next = self.next.unwrap_or(now + Self::PERIOD);
            // Keep a regular cadence; never run a burst of catch-up frames.
            while next <= now {
                next += Self::PERIOD;
            }
            self.next = Some(next);
            let due = -((next.saturating_duration_since(now).as_nanos() / 100).max(1) as i64);
            if SetWaitableTimer(self.handle, &due, 0, null(), null(), 0) == 0 {
                return Err(error("Schedule overlay frame"));
            }
            Ok(())
        }
    }

    // None is a frame deadline. Input remains dispatchable during the wait.
    pub(super) unsafe fn message(&mut self) -> io::Result<Option<Message>> {
        unsafe {
            loop {
                let active = self.next.is_some();
                let result = MsgWaitForMultipleObjectsEx(
                    u32::from(active),
                    &self.handle,
                    u32::MAX,
                    0x04ff,
                    4,
                );
                if result == u32::MAX {
                    return Err(error("Wait for overlay input or frame"));
                }
                if active && result == 0 {
                    self.next = self.next.map(|next| next + Self::PERIOD);
                    return Ok(None);
                }
                let mut message: Message = zeroed();
                if PeekMessageW(&mut message, null_mut(), 0, 0, 1) != 0 {
                    return Ok(Some(message));
                }
            }
        }
    }
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        unsafe {
            CancelWaitableTimer(self.handle);
            CloseHandle(self.handle);
        }
    }
}
