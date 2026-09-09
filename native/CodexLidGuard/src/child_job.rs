//! Kill only our app-server process tree when its owning worker exits.
use super::*;
use std::os::windows::io::AsRawHandle;

#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    minimum_working_set: usize,
    maximum_working_set: usize,
    active_processes: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    basic: BasicLimits,
    io_counters: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}

#[repr(C)]
#[derive(Default)]
struct Accounting {
    user_time: i64,
    kernel_time: i64,
    period_user_time: i64,
    period_kernel_time: i64,
    page_faults: u32,
    total_processes: u32,
    active_processes: u32,
    terminated_processes: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(
        job: Handle,
        class: i32,
        information: *const c_void,
        length: u32,
    ) -> Bool;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> Bool;
    fn TerminateJobObject(job: Handle, exit_code: u32) -> Bool;
    fn QueryInformationJobObject(
        job: Handle,
        class: i32,
        information: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> Bool;
}

pub struct ChildJob(Handle);
impl ChildJob {
    pub fn attach(child: &std::process::Child) -> io::Result<Self> {
        unsafe {
            let job = Self(CreateJobObjectW(null(), null()));
            if job.0.is_null() {
                return Err(error("Create background worker job"));
            }
            let limits = ExtendedLimits {
                basic: BasicLimits {
                    flags: 0x2000,
                    ..Default::default()
                },
                ..Default::default()
            }; // KILL_ON_JOB_CLOSE
            if SetInformationJobObject(
                job.0,
                9,
                &limits as *const _ as *const c_void,
                size_of::<ExtendedLimits>() as u32,
            ) == 0
                || AssignProcessToJobObject(job.0, child.as_raw_handle()) == 0
            {
                return Err(error("Isolate background worker process tree"));
            }
            Ok(job)
        }
    }
    pub fn terminate(&self) {
        unsafe {
            let members = self.members();
            TerminateJobObject(self.0, 1);
            // Termination is asynchronous. Wait for descendants too, before the
            // worker releases sleep protection or reports that its session ended.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let mut accounting = Accounting::default();
                if QueryInformationJobObject(
                    self.0,
                    1,
                    &mut accounting as *mut _ as *mut c_void,
                    size_of::<Accounting>() as u32,
                    null_mut(),
                ) == 0
                    || accounting.active_processes == 0
                {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    crate::logging::write(
                        "Windows is still terminating a background worker's process tree.",
                    );
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            // The active-process count can reach zero before process handles
            // become signaled. Wait on the exact objects, avoiding PID reuse.
            for process in members {
                let remaining = deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis()
                    .min(u32::MAX as u128) as u32;
                WaitForSingleObject(process, remaining);
                CloseHandle(process);
            }
        }
    }

    unsafe fn members(&self) -> Vec<Handle> {
        unsafe {
            // JOBOBJECT_BASIC_PROCESS_ID_LIST: two DWORDs followed by ULONG_PTRs.
            // See learn.microsoft.com/windows/win32/api/winnt/ns-winnt-jobobject_basic_process_id_list.
            let mut ids = vec![0usize; 129];
            loop {
                let read = QueryInformationJobObject(
                    self.0,
                    3,
                    ids.as_mut_ptr().cast(),
                    (ids.len() * size_of::<usize>()) as u32,
                    null_mut(),
                );
                let header = ids.as_ptr().cast::<u32>();
                let required = *header as usize;
                let count = *header.add(1) as usize;
                if read != 0 {
                    return ids[1..1 + count.min(ids.len() - 1)]
                        .iter()
                        .map(|id| OpenProcess(SYNCHRONIZE, 0, *id as u32))
                        .filter(|handle| !handle.is_null())
                        .collect();
                }
                if required < ids.len() || required > 16_384 {
                    return vec![];
                }
                ids.resize(required + 1, 0);
            }
        }
    }
}
impl Drop for ChildJob {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                CloseHandle(self.0);
            }
        }
    }
}
