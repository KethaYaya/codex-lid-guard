//! Keep transcript, metadata, and settings I/O off the native window thread.
use super::{Frame, SESSION_LIMIT};
use std::collections::HashSet;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, AtomicUsize, Ordering},
    mpsc::{self, SyncSender},
};
use std::time::{Duration, Instant};

struct Shared {
    collapsed: AtomicU8,
    latest: Mutex<[Frame; SESSION_LIMIT]>,
    windows: [Arc<AtomicUsize>; SESSION_LIMIT],
}

pub(super) struct FeedWorker {
    shared: Arc<Shared>,
    wake: SyncSender<()>,
}

pub(super) struct FeedView {
    shared: Arc<Shared>,
    wake: SyncSender<()>,
    slot: usize,
    cached: Frame,
}

// Surviving chats keep their lanes even when their recency order changes.
fn assign_slots(slots: &mut [Frame; SESSION_LIMIT], mut frames: Vec<Frame>) -> u8 {
    let previous = slots.each_ref().map(|frame| frame.session_id.clone());
    for slot in slots.iter_mut() {
        *slot = frames
            .iter()
            .position(|frame| frame.session_id == slot.session_id)
            .map(|index| frames.remove(index))
            .unwrap_or_else(Frame::empty);
    }
    let mut remaining = frames.into_iter();
    for slot in slots.iter_mut().filter(|slot| slot.session_id.is_none()) {
        if let Some(frame) = remaining.next() {
            *slot = frame;
        }
    }
    slots.iter().enumerate().fold(0, |changed, (index, frame)| {
        changed
            | if frame.session_id != previous[index] {
                1 << index
            } else {
                0
            }
    })
}

impl FeedWorker {
    pub(super) fn new(
        mut read: impl FnMut(&HashSet<String>) -> Vec<Frame> + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            collapsed: AtomicU8::new(0),
            latest: Mutex::new(std::array::from_fn(|_| Frame::empty())),
            windows: std::array::from_fn(|_| Arc::new(AtomicUsize::new(0))),
        });
        let background = shared.clone();
        let (wake, incoming) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut slots = std::array::from_fn(|_| Frame::empty());
            loop {
                let started = Instant::now();
                let bits = background.collapsed.load(Ordering::Relaxed);
                let collapsed = slots
                    .iter()
                    .enumerate()
                    .filter(|(slot, _)| bits & (1 << slot) != 0)
                    .filter_map(|(_, frame): (_, &Frame)| frame.session_id.clone())
                    .collect();
                let changed = assign_slots(&mut slots, read(&collapsed));
                background.collapsed.fetch_and(!changed, Ordering::Relaxed);
                // One reader and at most three cached frames serve all native windows.
                *background.latest.lock().unwrap() = slots.clone();
                for window in &background.windows {
                    crate::win::OverlayUpdates::notify(window);
                }
                if let Err(mpsc::RecvTimeoutError::Disconnected) = incoming
                    .recv_timeout(Duration::from_millis(250).saturating_sub(started.elapsed()))
                {
                    break;
                }
            }
        });
        Self { shared, wake }
    }

    pub(super) fn view(&self, slot: usize) -> FeedView {
        assert!(slot < SESSION_LIMIT);
        FeedView {
            shared: self.shared.clone(),
            wake: self.wake.clone(),
            slot,
            cached: Frame::empty(),
        }
    }
}

impl FeedView {
    pub(super) fn updates(&self) -> crate::win::OverlayUpdates {
        crate::win::OverlayUpdates::new(self.shared.windows[self.slot].clone(), self.wake.clone())
    }

    pub(super) fn snapshot(&mut self, collapsed: bool) -> Frame {
        let bit = 1 << self.slot;
        let previous = if collapsed {
            self.shared.collapsed.fetch_or(bit, Ordering::Relaxed)
        } else {
            self.shared.collapsed.fetch_and(!bit, Ordering::Relaxed)
        };
        if (previous & bit != 0) != collapsed {
            let _ = self.wake.try_send(());
        }
        // Even publishing a new frame must never hold up mouse input.
        if let Ok(latest) = self.shared.latest.try_lock() {
            let next = &latest[self.slot];
            if next.session_id != self.cached.session_id {
                self.shared.collapsed.fetch_and(!bit, Ordering::Relaxed);
            }
            self.cached = next.clone();
        }
        self.cached.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::Card;

    #[test]
    fn surviving_tabs_keep_their_slots_when_another_chat_is_replaced_or_hidden() {
        let frame = |id: &str| Frame {
            session_id: Some(id.into()),
            ..Frame::empty()
        };
        let mut slots = [frame("one"), frame("two"), frame("three")];
        assert_eq!(
            assign_slots(&mut slots, vec![frame("three"), frame("one"), frame("two")]),
            0
        );
        assert_eq!(
            assign_slots(
                &mut slots,
                vec![frame("four"), frame("three"), frame("two")]
            ),
            1
        );
        assert_eq!(
            slots.each_ref().map(|f| f.session_id.as_deref()),
            [Some("four"), Some("two"), Some("three")]
        );
        assert_eq!(assign_slots(&mut slots, vec![frame("three")]), 3);
        assert_eq!(slots[2].session_id.as_deref(), Some("three"));
        assert!(slots[..2].iter().all(|frame| frame.cards.is_empty()));
    }

    #[test]
    fn collapse_state_and_cached_frames_are_independent_between_windows() {
        let worker = FeedWorker::new(|_| vec![]);
        let mut views: Vec<_> = (0..SESSION_LIMIT).map(|slot| worker.view(slot)).collect();
        // Hold the publisher lock to prove no window waits on another window's data.
        let latest = worker.shared.latest.lock().unwrap();
        let started = Instant::now();
        views[0].snapshot(true);
        views[1].snapshot(true);
        views[2].snapshot(false);
        assert_eq!(worker.shared.collapsed.load(Ordering::Relaxed), 3);
        views[0].snapshot(false);
        assert_eq!(worker.shared.collapsed.load(Ordering::Relaxed), 2);
        assert!(started.elapsed() < Duration::from_millis(50));
        drop(latest);
    }

    #[test]
    fn slow_metadata_reads_do_not_block_clicks_or_cached_messages() {
        let (entered, started) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let (observed, changes) = mpsc::channel();
        let mut reads = 0;
        let worker = FeedWorker::new(move |collapsed| {
            reads += 1;
            if reads == 2 {
                entered.send(()).unwrap();
                blocked.recv_timeout(Duration::from_secs(3)).unwrap();
            }
            if !collapsed.is_empty() {
                let _ = observed.send(collapsed.clone());
            }
            vec![Frame {
                session_id: Some("one".into()),
                cards: vec![Card {
                    id: reads,
                    label: "chat".into(),
                    text: "cached update".into(),
                    final_message: false,
                    attention: false,
                    target: None,
                }],
                ..Frame::empty()
            }]
        });
        let mut view = worker.view(0);
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        view.snapshot(false);
        let clicked = Instant::now();
        let frame = view.snapshot(true);
        assert!(
            clicked.elapsed() < Duration::from_millis(50),
            "input waited for background I/O"
        );
        assert_eq!(frame.cards[0].id, 1);
        assert_eq!(frame.cards[0].text, "cached update");
        release.send(()).unwrap();
        assert_eq!(
            changes.recv_timeout(Duration::from_secs(2)).unwrap(),
            HashSet::from(["one".into()])
        );
    }

    #[test]
    fn closing_the_overlay_stops_its_background_reader() {
        struct OnDrop(mpsc::Sender<()>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (done, stopped) = mpsc::channel();
        let cleanup = OnDrop(done);
        let worker = FeedWorker::new(move |_| {
            let _ = &cleanup;
            vec![]
        });
        let view = worker.view(0);
        drop(worker);
        assert!(stopped.recv_timeout(Duration::from_millis(300)).is_err());
        drop(view);
        stopped.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
