//! Restore the editor without blocking the overlay's animation or input loop.
use super::*;
use crate::overlay::CardTarget;
use std::time::Instant;

#[link(name = "user32")]
unsafe extern "system" {
    fn PeekMessageW(
        message: *mut Message,
        window: Hwnd,
        first: u32,
        last: u32,
        remove: u32,
    ) -> Bool;
}

pub enum OverlayOpen {
    Ready(bool),
    Pending(mpsc::Receiver<bool>),
}

impl From<bool> for OverlayOpen {
    fn from(value: bool) -> Self {
        Self::Ready(value)
    }
}

impl OverlayOpen {
    pub(crate) fn activate(
        target: CardTarget,
        viewed: mpsc::Sender<(CardTarget, Instant)>,
        overlay: usize,
    ) -> Self {
        let (finished, result) = mpsc::channel();
        let started = Instant::now();
        match thread::Builder::new()
            .name("overlay-open".into())
            .spawn(move || {
                // AttachThreadInput requires the activation thread to have a message queue.
                unsafe {
                    let mut message: Message = zeroed();
                    PeekMessageW(&mut message, null_mut(), 0, 0, 0);
                }
                let opened = activate_overlay_target(&target, || {
                    super::overlay_window_events::notify_open_started(overlay, target.window);
                });
                if opened {
                    let _ = viewed.send((target, started));
                }
                let _ = finished.send(opened);
                // A completion only requests a refresh; it cannot activate a replacement chat.
                unsafe {
                    PostMessageW(
                        overlay as Hwnd,
                        super::overlay_window_events::WM_FRAME_READY,
                        0,
                        0,
                    );
                }
            }) {
            Ok(_) => Self::Pending(result),
            Err(cause) => {
                logging::write(format!("Could not start overlay open: {cause}"));
                Self::Ready(false)
            }
        }
    }

    pub(super) fn poll(&self) -> Option<bool> {
        match self {
            Self::Ready(result) => Some(*result),
            Self::Pending(result) => match result.try_recv() {
                Ok(opened) => Some(opened),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(false),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_completion_is_nonblocking_and_disconnection_is_failure() {
        let (sent, received) = mpsc::channel();
        let open = OverlayOpen::Pending(received);
        assert_eq!(open.poll(), None);
        sent.send(true).unwrap();
        assert_eq!(open.poll(), Some(true));
        drop(sent);
        assert_eq!(open.poll(), Some(false));
    }

    #[test]
    fn activation_worker_rejects_a_closed_window_without_acknowledging_the_chat() {
        let (viewed, acknowledgements) = mpsc::channel();
        let request = OverlayOpen::activate(
            CardTarget {
                window: 0,
                session_id: "00000000-0000-0000-0000-000000000001".into(),
            },
            viewed,
            0,
        );
        let started = Instant::now();
        loop {
            if let Some(result) = request.poll() {
                assert!(!result);
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(1));
        }
        assert!(acknowledgements.try_recv().is_err());
    }
}
