//! SetThreadExecutionState belongs to a thread. Keep acquire/release on one
//! dedicated owner even when independent Codex workers finish on other threads.
use super::*;

pub(super) struct ExecutionLease {
    stop: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl ExecutionLease {
    pub(super) fn acquire() -> io::Result<Self> {
        Self::start(set_execution_state)
    }

    fn start(set: impl Fn(bool) -> io::Result<()> + Send + 'static) -> io::Result<Self> {
        let (ready, result) = mpsc::sync_channel(1);
        let (stop, stopped) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("guard-execution-state".into())
            .spawn(move || {
                let acquired = set(true);
                let success = acquired.is_ok();
                if ready.send(acquired).is_ok() && success {
                    let _ = stopped.recv();
                }
                if success {
                    let _ = set(false);
                }
            })?;
        let lease = Self {
            stop: Some(stop),
            worker: Some(worker),
        };
        result.recv().map_err(io::Error::other)??;
        Ok(lease)
    }
}

impl Drop for ExecutionLease {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lease_released_from_another_worker_clears_its_own_thread() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let lease = ExecutionLease::start(move |active| {
            observed
                .lock()
                .unwrap()
                .push((thread::current().id(), active));
            Ok(())
        })
        .unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1);
        thread::spawn(move || drop(lease)).join().unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, calls[1].0);
        assert_ne!(calls[0].0, thread::current().id());
        assert!(calls[0].1);
        assert!(!calls[1].1);
    }
    #[test]
    fn failed_acquisition_does_not_leave_a_waiting_owner() {
        assert!(ExecutionLease::start(|_| Err(io::Error::other("test failure"))).is_err());
    }
}
