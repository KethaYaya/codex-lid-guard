//! Full conversation viewport painted into the existing glass overlay.
use super::*;
use crate::chat_history::{Message as ChatMessage, Role};
#[path = "overlay_chat_text.rs"]
mod rich;

/// Start at the first painted frame, so layout work cannot consume the animation.
pub(super) struct Expansion {
    from: Rect,
    started: Option<Instant>,
    smooth_start: bool,
}

impl Expansion {
    pub fn new(from: Rect) -> Self { Self { from, started: None, smooth_start: false } }
    pub fn growing(from: Rect) -> Self { Self { smooth_start: true, ..Self::new(from) } }
    pub fn pending(&self) -> bool { self.started.is_none() }
    pub fn origin(&self) -> Rect { self.from }
    pub fn sample(&mut self, to: Rect, now: Instant, animate: bool) -> (Rect, bool) {
        let started = *self.started.get_or_insert(now);
        if !animate { return (to, false); }
        let progress = (now.saturating_duration_since(started).as_secs_f32() / 0.300).clamp(0.0, 1.0);
        let eased = if self.smooth_start { progress * progress * (3.0 - 2.0 * progress) }
            else { 1.0 - (1.0 - progress).powi(3) };
        (opening_bounds(self.from, to, eased), progress < 1.0)
    }
}

/// Text layout and scroll position belong to the overlay, never a second window.
pub(super) struct Conversation {
    messages: Vec<ChatMessage>,
    bubbles: Vec<Bubble>,
    starts: Vec<i32>,
    pub bounds: Rect,
    height: i32,
    offset: i32,
    measured_width: i32,
    wrap_width: Option<i32>,
    dpi: u32,
    follow: bool,
    wheel_remainder: i32,
    grab: i32,
    fonts: Option<rich::Fonts>,
}

struct Bubble {
    role: Role,
    bounds: Rect,
    text: rich::Text,
    label: bool,
    padding: i32,
    body_top: i32,
    attachment_label: String,
    attachment_width: i32,
}

impl Default for Conversation {
    fn default() -> Self {
        Self { messages: vec![ChatMessage::new(Role::Notice, "Loading conversation\u{2026}")], bubbles: vec![], starts: vec![], bounds: Rect { left: 0, top: 0, right: 0, bottom: 0 },
            height: 0, offset: 0, measured_width: 0, wrap_width: None, dpi: 0, follow: true, wheel_remainder: 0, grab: 0, fonts: None }
    }
}

impl Conversation {
    pub fn content_height(&self) -> i32 { self.height }
    pub fn update_latest(&mut self, message: ChatMessage) -> bool {
        let changed = self.update(&[message]);
        if changed { self.follow = false; self.offset = 0; }
        changed
    }
    // Reveal the final text layout during growth instead of measuring a long chat on every frame.
    pub fn wrap_at(&mut self, width: Option<i32>) { self.wrap_width = width; }
    pub fn update(&mut self, messages: &[ChatMessage]) -> bool {
        if self.messages == messages { return false; }
        self.follow = self.offset >= self.maximum() - 2;
        self.messages = messages.into();
        self.measured_width = 0;
        true
    }
    pub fn layout(&mut self, bounds: Rect, dpi: u32) {
        self.bounds = bounds;
        let width = self.wrap_width.unwrap_or(bounds.right - bounds.left - scale_dip(16, dpi)).max(1);
        if width != self.measured_width || self.dpi != dpi {
            unsafe {
                let dc = GetDC(null_mut());
                if self.fonts.is_none() || self.dpi != dpi { self.fonts = Some(rich::Fonts::new(dpi)); }
                let fonts = self.fonts.as_ref().unwrap();
                self.starts.clear(); self.bubbles.clear(); self.height = scale_dip(4, dpi);
                let d = |n| scale_dip(n, dpi);
                let mut previous = None;
                for message in &self.messages {
                    if let Some(role) = previous { self.height += d(if role == message.role { 6 } else { 18 }); }
                    self.starts.push(self.height);
                    let maximum = (width * if message.role == Role::User { 70 } else { 92 } / 100).max(1);
                    let padding = if message.role == Role::User { d(12).min(maximum / 4) } else { 0 };
                    let text_width = (maximum - padding * 2).max(1);
                    let text = rich::Text::layout(dc, fonts, &message.text, text_width, dpi, message.role);
                    let label = message.role == Role::Assistant && previous != Some(Role::Assistant);
                    let attachment_label = match message.images { 0 => String::new(), 1 => "Image \u{b7} attachment".into(), n => format!("{n} images \u{b7} attachments") };
                    let attachment_width = if message.images == 0 { 0 } else { (rich::measure(dc, fonts.small, &attachment_label) + d(52)).min(maximum) };
                    let bubble_width = (text.width + padding * 2).max(attachment_width).max(1).min(maximum);
                    let left = match message.role { Role::User => width - bubble_width, Role::Notice => (width - bubble_width) / 2, _ => 0 };
                    let body_top = if message.role == Role::User && !message.text.is_empty() { d(8) } else if label { d(24) } else { 0 };
                    let height = body_top + text.height + if message.role == Role::User && !message.text.is_empty() { d(8) } else { 0 }
                        + if message.images > 0 { d(38) + if message.text.is_empty() { 0 } else { d(6) } } else { 0 };
                    self.bubbles.push(Bubble { role: message.role,
                        bounds: Rect { left, top: self.height, right: left + bubble_width, bottom: self.height + height },
                        text, label, padding, body_top, attachment_label, attachment_width });
                    self.height += height;
                    previous = Some(message.role);
                }
                self.height += d(4);
                ReleaseDC(null_mut(), dc);
            }
            self.measured_width = width; self.dpi = dpi;
        }
        self.offset = if self.follow { self.maximum() } else { self.offset.min(self.maximum()) };
    }
    fn maximum(&self) -> i32 { (self.height - (self.bounds.bottom - self.bounds.top)).max(0) }
    pub fn scroll(&mut self, pixels: i32) {
        self.offset = self.offset.saturating_add(pixels).clamp(0, self.maximum());
        self.follow = self.offset >= self.maximum();
    }
    pub fn wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        if x < self.bounds.left || x >= self.bounds.right || y < self.bounds.top || y >= self.bounds.bottom { return false; }
        self.wheel_remainder += delta;
        let steps = self.wheel_remainder / 120;
        self.wheel_remainder %= 120;
        self.scroll(-steps * scale_dip(51, self.dpi));
        true
    }
    pub fn scrollbar(&self) -> Option<(Rect, Rect)> {
        if self.maximum() == 0 { return None; }
        let d = |n| scale_dip(n, self.dpi);
        let track = Rect { left: self.bounds.right - d(6), right: self.bounds.right - d(2), ..self.bounds };
        let available = track.bottom - track.top;
        let height = ((i64::from(available) * i64::from(available) / i64::from(self.height)) as i32).max(d(20)).min(available);
        let top = track.top + (i64::from(available - height) * i64::from(self.offset) / i64::from(self.maximum())) as i32;
        Some((track, Rect { top, bottom: top + height, ..track }))
    }
    pub fn begin_drag(&mut self, y: i32) { self.grab = self.scrollbar().map_or(0, |(_, thumb)| y - thumb.top); }
    pub fn drag(&mut self, y: i32) {
        if let Some((track, thumb)) = self.scrollbar() {
            let travel = (track.bottom - track.top - (thumb.bottom - thumb.top)).max(1);
            self.offset = (i64::from((y - self.grab - track.top).clamp(0, travel)) * i64::from(self.maximum()) / i64::from(travel)) as i32;
            self.follow = self.offset >= self.maximum();
        }
    }
    pub unsafe fn paint(&self, dc: Handle) {
        unsafe {
            let saved = SaveDC(dc);
            IntersectClipRect(dc, self.bounds.left, self.bounds.top, self.bounds.right, self.bounds.bottom);
            let d = |n| scale_dip(n, self.dpi);
            let Some(fonts) = &self.fonts else { RestoreDC(dc, saved); return; };
            let old = SelectObject(dc, fonts.body);
            let first = self.starts.partition_point(|top| *top < self.offset).saturating_sub(1);
            for bubble in self.bubbles.iter().skip(first) {
                let top = self.bounds.top + bubble.bounds.top - self.offset;
                if top >= self.bounds.bottom { break; }
                let rect = Rect { left: self.bounds.left + bubble.bounds.left, top,
                    right: self.bounds.left + bubble.bounds.right, bottom: self.bounds.top + bubble.bounds.bottom - self.offset };
                if bubble.role == Role::User && bubble.text.height > 0 {
                    let body = Rect { bottom: top + bubble.body_top + bubble.text.height + d(8), ..rect };
                    group_window::glass::message(dc, body, self.dpi);
                }
                if bubble.label {
                    fill_rounded_rectangle(dc, &Rect { left: rect.left, top: top + d(5), right: rect.left + d(7), bottom: top + d(12) }, color_ref(127, 176, 240), d(4));
                    SelectObject(dc, fonts.label);
                    group_window::text(dc, "Codex", Rect { left: rect.left + d(14), top, right: rect.left + d(90), bottom: top + d(18) }, color_ref(142, 161, 182), DT_SINGLELINE);
                }
                let clip = SaveDC(dc);
                IntersectClipRect(dc, rect.left + bubble.padding, rect.top, rect.right - bubble.padding, rect.bottom);
                SetBkMode(dc, TRANSPARENT);
                for fragment in &bubble.text.fragments {
                    let y = top + bubble.body_top + fragment.y;
                    if y >= self.bounds.bottom { break; }
                    if y + d(20) <= self.bounds.top { continue; }
                    let x = rect.left + bubble.padding + fragment.x;
                    let mut line_rect = Rect { left: x, top: y, right: x + fragment.width, bottom: y + d(20) };
                    let color = match fragment.style {
                        rich::Style::Bold => color_ref(255, 255, 255), rich::Style::Code => color_ref(207, 226, 255),
                        rich::Style::Muted | rich::Style::Notice => color_ref(142, 161, 182),
                        _ if bubble.role == Role::User => color_ref(244, 248, 255), _ => color_ref(233, 238, 245),
                    };
                    if fragment.style == rich::Style::Code {
                        fill_rounded_rectangle(dc, &Rect { top: y + d(1), bottom: y + d(19), ..line_rect }, color_ref(20, 27, 36), d(5));
                        line_rect.left += d(5); line_rect.right -= d(5);
                    }
                    SelectObject(dc, fonts.get(fragment.style)); SetTextColor(dc, color);
                    DrawTextW(dc, fragment.text.as_ptr(), wide_text_length(&fragment.text), &mut line_rect, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
                }
                RestoreDC(dc, clip);
                if !bubble.attachment_label.is_empty() {
                    let chip = Rect { left: rect.right - bubble.attachment_width, top: rect.bottom - d(38), ..rect };
                    fill_rounded_rectangle(dc, &chip, color_ref(39, 52, 68), d(10));
                    let icon = Rect { left: chip.left + d(6), top: chip.top + d(6), right: chip.left + d(32), bottom: chip.top + d(32) };
                    fill_rounded_rectangle(dc, &icon, color_ref(58, 82, 114), d(6));
                    SelectObject(dc, fonts.small);
                    group_window::text(dc, "\u{25a7}", icon, color_ref(188, 214, 255), DT_SINGLELINE | DT_VCENTER | 1);
                    group_window::text(dc, &bubble.attachment_label, Rect { left: chip.left + d(40), right: chip.right - d(10), ..chip },
                        color_ref(233, 238, 245), DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                }
            }
            SelectObject(dc, old);
            if let Some((track, thumb)) = self.scrollbar() {
                fill_rounded_rectangle(dc, &track, color_ref(65, 83, 105), scale_dip(4, self.dpi));
                fill_rounded_rectangle(dc, &thumb, color_ref(164, 181, 201), scale_dip(4, self.dpi));
            }
            RestoreDC(dc, saved);
        }
    }
}

pub(super) fn centered(work: Rect, dpi: u32) -> Rect {
    let width = scale_dip(780, dpi).min((work.right - work.left - scale_dip(40, dpi)).max(1));
    let height = scale_dip(680, dpi).min((work.bottom - work.top - scale_dip(40, dpi)).max(1));
    let left = work.left + (work.right - work.left - width) / 2;
    let top = work.top + (work.bottom - work.top - height) / 2;
    Rect { left, top, right: left + width, bottom: top + height }
}

pub(super) struct HistoryWorker {
    request: mpsc::SyncSender<()>,
    result: mpsc::Receiver<Result<Option<crate::chat_history::Snapshot>, String>>,
    pending: bool,
    next: Instant,
}

impl HistoryWorker {
    pub fn new(id: String, owner: usize) -> io::Result<Self> {
        Self::with_lookup(id, owner, |id| crate::codex_lifecycle::session_metadata(id).0.map(Into::into))
    }

    fn with_lookup(id: String, owner: usize,
        mut lookup: impl FnMut(&str) -> Option<std::path::PathBuf> + Send + 'static) -> io::Result<Self> {
        let (request, requested) = mpsc::sync_channel(1);
        let (finished, result) = mpsc::channel();
        thread::Builder::new().name("overlay-history".into()).spawn(move || {
            let mut history: Option<crate::chat_history::History> = None;
            let mut path = None;
            let mut next_lookup = Instant::now();
            while requested.recv().is_ok() {
                let background = crate::background::chat_snapshot(&id);
                if Instant::now() >= next_lookup {
                    next_lookup = Instant::now() + Duration::from_secs(2);
                    let transcript_id = background.as_ref().and_then(|view| view.thread_id.as_deref()).unwrap_or(&id);
                    if let Some(current) = lookup(transcript_id) { path = Some(current); }
                }
                let result = if background.as_ref().is_some_and(|view| view.ready && view.messages.is_empty()) {
                    Ok(Some(crate::chat_history::Snapshot { messages: vec![ChatMessage::new(Role::Notice, "New chat\nSend a message to get started.")], busy: false }))
                } else if let Some(path) = &path {
                    history.get_or_insert_with(|| crate::chat_history::History::new(path.clone()))
                        .follow(path.clone()).map_err(|_| "Could not read saved chat history.".into())
                } else if let Some(view) = background {
                    Ok(Some(crate::chat_history::Snapshot { messages: view.messages, busy: view.busy }))
                } else { Err("Saved chat history is not available yet.".into()) };
                if finished.send(result).is_err() { break; }
                unsafe { PostMessageW(owner as Hwnd, WM_FRAME_READY, 0, 0); }
            }
        })?;
        Ok(Self { request, result, pending: false, next: Instant::now() })
    }
    pub fn poll(&mut self) -> Option<Result<Option<crate::chat_history::Snapshot>, String>> {
        let result = self.result.try_recv().ok();
        if result.is_some() { self.pending = false; }
        if !self.pending && Instant::now() >= self.next && self.request.try_send(()).is_ok() {
            self.pending = true; self.next = Instant::now() + Duration::from_millis(400);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_worker_keeps_updating_when_session_metadata_moves_to_another_file() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        let directory = std::env::temp_dir().join(format!("overlay-history-worker-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let old = directory.join("old.jsonl");
        let next = directory.join("next.jsonl");
        let write = |path: &std::path::Path, text: &str| std::fs::write(path, format!("{}\n",
            serde_json::json!({"type":"event_msg","payload":{"type":"agent_message","message":text}}))).unwrap();
        write(&old, "Earlier reply");
        write(&next, "Reply already in the continuation");
        let moved = Arc::new(AtomicBool::new(false));
        let moved_reader = moved.clone();
        let old_reader = old.clone();
        let next_reader = next.clone();
        let mut worker = HistoryWorker::with_lookup("fixture-moving-chat".into(), 0, move |_| {
            Some(if moved_reader.load(Ordering::Relaxed) { &next_reader } else { &old_reader }.clone())
        }).unwrap();
        let wait_for = |worker: &mut HistoryWorker, expected: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(Ok(Some(snapshot))) = worker.poll()
                    && snapshot.messages.last().is_some_and(|message| message.text == expected) { return snapshot; }
                assert!(Instant::now() < deadline, "history worker stopped following the current transcript");
                thread::sleep(Duration::from_millis(20));
            }
        };
        assert_eq!(wait_for(&mut worker, "Earlier reply").messages.len(), 1);
        moved.store(true, Ordering::Relaxed);
        assert_eq!(wait_for(&mut worker, "Reply already in the continuation").messages.len(), 2);
        drop(worker);
        for path in [old, next] { std::fs::remove_file(path).unwrap(); }
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn expansion_and_escape_have_continuous_bounds_and_respect_reduced_motion() {
        let now = Instant::now();
        let tab = Rect { left: -28, top: 700, right: 0, bottom: 742 };
        let full = centered(Rect { left: -1920, top: 0, right: 0, bottom: 1040 }, 96);
        let mut grow = Expansion::growing(tab);
        let first_paint = now + Duration::from_secs(2);
        assert!(grow.pending());
        assert_eq!(grow.sample(full, first_paint, true), (tab, true));
        let (middle, active) = grow.sample(full, first_paint + Duration::from_millis(120), true);
        assert!(active && middle != tab && middle != full);
        assert!(middle.right-middle.left > tab.right-tab.left);
        assert!(middle.right-middle.left < full.right-full.left);
        let mut escape = Expansion::new(middle);
        assert_eq!(escape.sample(tab, first_paint, true), (middle, true), "reversing starts at the visible frame");
        let (shrinking, active) = escape.sample(tab, first_paint + Duration::from_millis(150), true);
        assert!(active && shrinking.right-shrinking.left < middle.right-middle.left);
        assert_eq!(escape.sample(tab, first_paint + Duration::from_millis(300), true), (tab, false));
        assert_eq!(grow.sample(full, first_paint + Duration::from_millis(300), true), (full, false));
        assert_eq!(Expansion::growing(tab).sample(full, now, false), (full, false));
        assert_eq!(escape.sample(tab, now, false), (tab, false));
    }

    #[test]
    fn growth_starts_and_finishes_gently_without_a_large_first_jump() {
        let from = Rect { left: 1404, top: 793, right: 1920, bottom: 1040 };
        let to = Rect { left: 375, top: 10, right: 1545, bottom: 1030 };
        let start = Instant::now();
        let mut growth = Expansion::growing(from);
        let frames: Vec<_> = (0..=18).map(|frame| {
            growth.sample(to, start + Duration::from_nanos(300_000_000*frame/18), true).0
        }).collect();
        let steps: Vec<_> = frames.windows(2).map(|pair| pair[0].left-pair[1].left).collect();
        assert!(steps.iter().all(|step| *step >= 0));
        assert!(steps[0] < 12 && steps[17] < 12, "endpoints should ease in and out: {steps:?}");
        assert!(steps.iter().all(|step| *step < 90), "avoid abrupt jumps during the centerward move: {steps:?}");
        assert_eq!(frames[0], from);
        assert_eq!(frames[18], to);
    }

    #[test]
    fn history_scroll_follows_new_messages_only_when_already_at_the_bottom() {
        let bounds = Rect { left: 10, top: 100, right: 710, bottom: 560 };
        let mut chat = Conversation::default();
        let mut messages = (0..200).map(|n| ChatMessage::new(Role::User, format!("Message {n}: a saved conversation"))).collect::<Vec<_>>();
        chat.update(&messages); chat.layout(bounds, 96);
        assert!(chat.offset > 0);
        assert_eq!(chat.offset, chat.maximum());
        chat.scroll(-300);
        let reading = chat.offset;
        messages.push(ChatMessage::new(Role::Assistant, "A new answer"));
        chat.update(&messages); chat.layout(bounds, 96);
        assert_eq!(chat.offset, reading, "new messages preserve the reading position");
        assert!(!chat.wheel(5, 90, 120), "scrolling another area does not move the conversation");
        assert!(chat.wheel(20, 150, 120));
        assert!(chat.offset < reading);
        let (track, thumb) = chat.scrollbar().unwrap();
        chat.begin_drag(thumb.top);
        chat.drag(track.bottom);
        assert_eq!(chat.offset, chat.maximum());
        messages.push(ChatMessage::new(Role::Assistant, "Another answer\n\nMore detail"));
        chat.update(&messages); chat.layout(bounds, 96);
        assert_eq!(chat.offset, chat.maximum(), "at the bottom, follow incoming replies");
        let prior_height = chat.height;
        chat.layout(Rect { right: 140, ..bounds }, 144);
        assert!(chat.height > prior_height, "saved messages wrap at the current width and DPI");
    }

    #[test]
    fn message_bubbles_align_by_role_and_fit_wrapped_text_at_each_dpi() {
        for dpi in [96, 144, 192] {
            let d = |n| scale_dip(n, dpi);
            let mut chat = Conversation::default();
            chat.update(&[ChatMessage::new(Role::User, "Hello"),
                ChatMessage::new(Role::Assistant, "A longer answer with enough words to wrap to several lines. ".repeat(14)),
                ChatMessage::new(Role::User, "Codex\nThis is still my message.\n\nAnd a second paragraph.")]);
            chat.layout(Rect { left: d(18), top: d(100), right: d(740), bottom: d(610) }, dpi);
            assert_eq!(chat.bubbles[0].bounds.right, chat.measured_width);
            assert_eq!(chat.bubbles[1].bounds.left, 0);
            assert_eq!(chat.bubbles[2].bounds.right, chat.measured_width);
            for bubble in &chat.bubbles {
                assert!(bubble.bounds.left >= 0 && bubble.bounds.right <= chat.measured_width);
                assert!(bubble.bounds.bottom - bubble.bounds.top >= bubble.text.height + bubble.body_top);
                for fragment in &bubble.text.fragments {
                    assert!(fragment.y + d(20) <= bubble.text.height);
                    assert!(fragment.x + fragment.width <= bubble.bounds.right - bubble.bounds.left - 2 * bubble.padding);
                }
            }
            assert!(chat.bubbles.windows(2).all(|pair| pair[0].bounds.bottom < pair[1].bounds.top));
            assert!(chat.bubbles[1].bounds.bottom-chat.bubbles[1].bounds.top > chat.bubbles[0].bounds.bottom-chat.bubbles[0].bounds.top);
            assert!(!chat.update(&chat.messages.clone()), "unchanged history keeps its measured layout");
        }
    }

    #[test]
    fn grouped_roles_markdown_attachments_and_notices_keep_distinct_layouts() {
        for dpi in [96, 144, 192] {
            let d = |n| scale_dip(n, dpi);
            let mut image = ChatMessage::new(Role::User, ""); image.images = 1;
            let mut chat = Conversation::default();
            chat.update(&[ChatMessage::new(Role::User, "**My literal message**"),
                ChatMessage::new(Role::Assistant, "1. **Verify the data** and keep this very long list item aligned when it wraps to another line.\n2. Run `cargo test`."),
                ChatMessage::new(Role::Assistant, "Another paragraph in the same run."), image,
                ChatMessage::new(Role::Notice, "Connection restored"),
                ChatMessage::new(Role::Assistant, "A fresh run.")]);
            chat.layout(Rect { left: 0, top: 0, right: d(370), bottom: d(700) }, dpi);
            let bubbles = &chat.bubbles;
            assert!(!bubbles[0].label && bubbles[1].label && !bubbles[2].label && bubbles[5].label);
            assert!(bubbles[0].text.fragments.iter().all(|f| f.style == rich::Style::Plain));
            assert!(bubbles[1].text.fragments.iter().any(|f| f.style == rich::Style::Bold));
            assert!(bubbles[1].text.fragments.iter().any(|f| f.style == rich::Style::Code));
            assert!(bubbles[1].text.fragments.iter().filter(|f| f.style != rich::Style::Muted).all(|f| f.x >= d(22)));
            assert_eq!(bubbles[2].bounds.top - bubbles[1].bounds.bottom, d(6));
            assert_eq!(bubbles[3].bounds.top - bubbles[2].bounds.bottom, d(18));
            assert_eq!(bubbles[3].text.height, 0);
            assert_eq!(bubbles[3].attachment_label, "Image \u{b7} attachment");
            assert_eq!(bubbles[3].bounds.right, chat.measured_width);
            assert!((bubbles[4].bounds.left + bubbles[4].bounds.right - chat.measured_width).abs() <= 1);
            assert!(!bubbles[4].label && bubbles[4].padding == 0);
            let token = "\u{1f30d}\u{65e5}\u{672c}".repeat(80);
            chat.update(&[ChatMessage::new(Role::Assistant, format!("`{token}`"))]);
            chat.layout(Rect { right: d(150), ..chat.bounds }, dpi);
            let fragments = &chat.bubbles[0].text.fragments;
            let joined = fragments.iter().map(|f| String::from_utf16(&f.text[..f.text.len()-1]).unwrap()).collect::<String>();
            assert_eq!(joined, token, "hard wrapping must not split UTF-16 surrogate pairs or lose text");
            assert!(fragments.windows(2).all(|pair| pair[0].y < pair[1].y));
        }
    }

    #[test]
    fn chat_is_centered_and_fits_each_monitor_and_dpi() {
        for dpi in [96, 120, 144, 192] {
            for work in [Rect { left: -1920, top: 0, right: 0, bottom: 1040 }, Rect { left: 0, top: 0, right: 800, bottom: 560 }] {
                let bounds = centered(work, dpi);
                assert!(bounds.left >= work.left && bounds.top >= work.top && bounds.right <= work.right && bounds.bottom <= work.bottom);
                assert!((bounds.left + bounds.right - work.left - work.right).abs() <= 1);
                assert!((bounds.top + bounds.bottom - work.top - work.bottom).abs() <= 1);
            }
        }
    }
}
