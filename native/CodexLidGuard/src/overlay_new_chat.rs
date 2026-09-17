//! Prepare a new session off the overlay's input/render thread.
use super::*;
#[cfg(test)]
pub(super) static TEST_DELAY_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(super) struct Request {
    pub source: CardTarget,
    pub group: String,
    result: mpsc::Receiver<Result<String, String>>,
}
impl Request {
    pub fn start(source: CardTarget, group: String, owner: Hwnd) -> io::Result<Self> {
        let (finished, result) = mpsc::channel();
        let target = source.clone();
        let owner = owner as usize;
        thread::Builder::new().name("overlay-new-chat".into()).spawn(move || {
            #[cfg(test)]
            thread::sleep(Duration::from_millis(TEST_DELAY_MS.load(std::sync::atomic::Ordering::Relaxed)));
            let created = crate::background::new_chat(&target);
            let _ = finished.send(created);
            unsafe { PostMessageW(owner as Hwnd, WM_FRAME_READY, 0, 0); }
        })?;
        Ok(Self { source, group, result })
    }
    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Could not start the new chat. Try again.".into())),
        }
    }
}
