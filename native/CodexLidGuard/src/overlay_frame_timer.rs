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
            // Input or painting can finish just after an unconsumed deadline.
            // Deliver it promptly instead of adding another 16 ms of waiting.
            let next = self.next.unwrap_or(now + Self::PERIOD);
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
                // Do not starve Escape/typing when a slow frame leaves the timer
                // overdue. Growth notifications are coalesced by the caller.
                let mut message: Message = zeroed();
                if PeekMessageW(&mut message, null_mut(), 0, 0, 1) != 0 {
                    return Ok(Some(message));
                }
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
                    self.next = self.next.map(|next| after_tick(next, Instant::now(), Self::PERIOD));
                    return Ok(None);
                }
            }
        }
    }
}

fn after_tick(deadline: Instant, now: Instant, period: Duration) -> Instant {
    let next = deadline + period;
    // Consume one tick only. A long stall must never replay a queue of old ticks.
    if next <= now { now + period } else { next }
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        unsafe {
            CancelWaitableTimer(self.handle);
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_input_and_an_overdue_tick_are_both_delivered_without_an_extra_period() {
        unsafe {
            let mut timer = FrameTimer::new().unwrap();
            let mut message: Message = zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, 0); // Create this test thread's queue.
            assert_ne!(PostMessageW(null_mut(), 0x804f, 9, 7), 0);
            let overdue = Instant::now() - Duration::from_millis(1);
            timer.next = Some(overdue);
            timer.update(true).unwrap();
            assert_eq!(timer.next, Some(overdue), "input must not push an unconsumed tick into the future");
            let input = timer.message().unwrap().unwrap();
            assert_eq!((input.message, input.wparam, input.lparam), (0x804f, 9, 7));
            timer.update(true).unwrap();
            assert_eq!(timer.next, Some(overdue));
            assert!(timer.message().unwrap().is_none(), "the overdue tick follows queued input");
            timer.update(false).unwrap();
            assert!(timer.next.is_none());
        }
    }

    #[test]
    fn a_slightly_late_frame_preserves_the_next_tick_without_skipping_it() {
        let start = Instant::now();
        let period = FrameTimer::PERIOD;
        let deadline = start + period;
        let now = deadline + Duration::from_millis(1);
        assert_eq!(after_tick(deadline, now, period), start + period*2);
    }

    #[test]
    fn a_long_stall_resumes_with_one_future_tick_instead_of_a_catch_up_burst() {
        let start = Instant::now();
        let period = FrameTimer::PERIOD;
        for late in [period, Duration::from_millis(80), Duration::from_secs(60*60)] {
            let now = start + late;
            assert_eq!(after_tick(start, now, period), now + period);
        }
    }
}
