//! Restore the editor without blocking the overlay's animation or input loop.
use super::*;
use crate::codex_log::{ViewState, ViewStateReader};
use crate::overlay::CardTarget;
use std::collections::HashSet;
use std::time::Instant;

pub(super) const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

fn wait_for_session_confirmation(
    target: &CardTarget,
    deadline: Instant,
    mut observe: impl FnMut() -> Option<ViewState>,
    focused: impl Fn() -> bool,
) -> bool {
    while Instant::now() < deadline && focused() {
        let view = observe();
        // A successful ShellExecute only means VS Code received the link.
        // Another chat in the same window must never acknowledge this target.
        if Instant::now() >= deadline {
            return false;
        }
        if matches!(view, Some(ViewState::Active(id)) if id.eq_ignore_ascii_case(&target.session_id))
        {
            return focused();
        }
        thread::sleep(
            Duration::from_millis(75).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    false
}

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
                let dispatched = activate_overlay_target(&target, || {
                    super::overlay_window_events::notify_open_started(overlay, target.window);
                });
                let mut views = ViewStateReader::default();
                let sessions = HashSet::from([target.session_id.clone()]);
                let opened = dispatched && wait_for_session_confirmation(
                    &target,
                    started + OPEN_TIMEOUT - Duration::from_millis(250),
                    || views.for_sessions(&sessions).remove(&target.session_id).map(|view| view.state),
                    || is_window_focused(target.window),
                );
                if dispatched && !opened {
                    logging::write("The requested overlay chat was not confirmed active; keeping its notification available.");
                }
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
    fn confirmation_waits_for_the_requested_chat_in_a_shared_window() {
        let target = CardTarget {
            window: 10,
            session_id: "chat-b".into(),
        };
        let mut observed = [
            Some(ViewState::Active("chat-a".into())),
            Some(ViewState::Inactive),
            Some(ViewState::Active("chat-b".into())),
        ]
        .into_iter();
        assert!(wait_for_session_confirmation(
            &target,
            Instant::now() + Duration::from_secs(1),
            || observed.next().flatten(),
            || true
        ));
        assert_eq!(observed.len(), 0);
    }

    #[test]
    fn unconfirmed_expired_and_unfocused_opens_do_not_acknowledge() {
        let target = CardTarget {
            window: 10,
            session_id: "chat-b".into(),
        };
        assert!(!wait_for_session_confirmation(
            &target,
            Instant::now() + Duration::from_millis(5),
            || Some(ViewState::Active("chat-a".into())),
            || true
        ));
        assert!(!wait_for_session_confirmation(
            &target,
            Instant::now(),
            || Some(ViewState::Active("chat-b".into())),
            || true
        ));
        assert!(!wait_for_session_confirmation(
            &target,
            Instant::now() + Duration::from_secs(1),
            || Some(ViewState::Active("chat-b".into())),
            || false
        ));
    }
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
