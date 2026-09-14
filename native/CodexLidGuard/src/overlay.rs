//! Opt-in assistant previews. Contents stay on this worker, never in status or logs.
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{codex_lifecycle, codex_log, logging, model::GuardSettings, win};
use crate::{codex_session_index::SessionTitleIndex, paths};
use serde::Deserialize;

#[path = "overlay_feed_worker.rs"]
mod overlay_feed_worker;
use overlay_feed_worker::FeedWorker;

const READ_LIMIT: u64 = 256 * 1024;
const LINE_LIMIT: usize = 1024 * 1024;
pub const SESSION_LIMIT: usize = 10;

pub(crate) fn next_activity() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub activity: u64,
    pub cwd: Option<String>,
    pub window: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CardTarget {
    pub window: u64,
    pub session_id: String,
    pub project: Option<crate::session_navigation::Project>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: u64,
    pub label: String,
    pub text: String,
    pub final_message: bool,
    pub attention: bool,
    pub target: Option<CardTarget>,
}

#[derive(Clone)]
pub struct Frame {
    pub session_id: Option<String>,
    pub activity: u64,
    pub cards: Vec<Card>,
    pub window: Option<u64>,
    pub opacity: u8,
    pub position: String,
    pub max_tabs: usize,
    pub shortcuts: crate::shortcut_config::ShortcutConfig,
    pub close: bool,
    pub busy: bool,
    pub attention: bool,
    // A stable token: a new tab or a focus-loss edge requests docking once.
    pub dock_request: u64,
    // Keep the focused chat cached so a native minimize event can reveal it immediately.
    pub hidden_in_focus: bool,
}

impl Frame {
    pub(crate) fn empty() -> Self {
        Self {
            session_id: None,
            activity: 0,
            cards: vec![],
            window: None,
            opacity: 82,
            position: "bottom-right".into(),
            max_tabs: 3,
            shortcuts: Default::default(),
            close: false,
            busy: false,
            attention: false,
            dock_request: 0,
            hidden_in_focus: false,
        }
    }
}

struct TrackedSession {
    session: Session,
    project: Option<crate::session_navigation::Project>,
    label: String,
    fallback_title: Option<String>,
    cursor: Option<MessageCursor>,
    last_active: Instant,
    next_lookup: Instant,
    source_active: bool,
    busy: bool,
    completion: Option<(u64, Instant)>,
}

struct Preview {
    session: String,
    card: Card,
    received: Instant,
}

#[derive(Default)]
struct Feed {
    sessions: HashMap<String, TrackedSession>,
    previews: VecDeque<Preview>,
    next_id: u64,
    titles: SessionTitleIndex,
    next_title_refresh: Option<Instant>,
    was_collapsed: HashSet<String>,
    views: codex_log::ViewStateReader,
    focus: FocusTransitions,
    opened: HashMap<u64, OpenedChat>,
    dismissed: HashSet<String>,
}

struct OpenedChat {
    session_id: String,
    baseline: Option<codex_log::ViewObservation>,
}

#[derive(Default)]
struct FocusTransitions {
    focused: Option<u64>,
    epochs: HashMap<u64, u64>,
}

impl FocusTransitions {
    fn observe(&mut self, focused: Option<u64>, windows: &HashSet<u64>) {
        self.epochs.retain(|window, _| windows.contains(window));
        if self.focused != focused
            && let Some(previous) = self.focused.filter(|window| windows.contains(window))
        {
            *self.epochs.entry(previous).or_insert(1) += 1;
        }
        self.focused = focused;
    }

    fn request(&self, window: u64) -> u64 {
        self.epochs.get(&window).copied().unwrap_or(1)
    }
}

impl Feed {
    fn recent_sessions(&self, limit: usize) -> HashSet<String> {
        let mut recent: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, tracked)| tracked.session.window.is_some())
            .collect();
        recent.sort_by(|a, b| {
            b.1.session
                .activity
                .cmp(&a.1.session.activity)
                .then_with(|| a.0.cmp(b.0))
        });
        recent
            .into_iter()
            .take(limit.clamp(1, SESSION_LIMIT))
            .map(|(id, _)| id.clone())
            .collect()
    }

    fn pause_expiry(&mut self, collapsed: &HashSet<String>, now: Instant) {
        for preview in &mut self.previews {
            if collapsed.contains(&preview.session) || self.was_collapsed.contains(&preview.session)
            {
                preview.received = now;
                if let Some(session) = self.sessions.get_mut(&preview.session) {
                    session.last_active = now;
                }
            }
        }
        self.was_collapsed = collapsed.clone();
    }

    fn frame(
        &mut self,
        mut active: Vec<Session>,
        settings: &GuardSettings,
        now: Instant,
        collapsed: &HashSet<String>,
    ) -> Vec<Frame> {
        let contexts = crate::session_navigation::contexts(active.iter().filter_map(|session| session.window)
            .chain(self.sessions.values().filter_map(|tracked| tracked.session.window)));
        let allowed = |window: u64, cwd: Option<&str>| {
            contexts.get(&window).is_none_or(|context| context.keeps_preview(cwd))
        };
        active.retain(|session| session.window.is_none_or(|window| allowed(window, session.cwd.as_deref())));
        self.discard_stale_workspaces(allowed);
        // Keep the bounded preview cache available behind the tab, then give
        // reopened messages their normal reading time before expiry resumes.
        self.pause_expiry(collapsed, now);
        let active_ids: std::collections::HashSet<_> =
            active.iter().map(|session| session.id.clone()).collect();
        for (id, tracked) in &mut self.sessions {
            if !active_ids.contains(id) {
                tracked.source_active = false;
                tracked.busy = false;
            }
        }
        for session in active {
            let tracked =
                self.sessions
                    .entry(session.id.clone())
                    .or_insert_with(|| TrackedSession {
                        session: session.clone(),
                        project: None,
                        label: String::new(),
                        fallback_title: None,
                        cursor: None,
                        last_active: now,
                        next_lookup: now,
                        source_active: false,
                        busy: false,
                        completion: None,
                    });
            if !tracked.source_active {
                self.dismissed.remove(&session.id);
                tracked.busy = true;
                tracked.completion = None;
                self.previews
                    .retain(|preview| preview.session != session.id);
            }
            tracked.source_active = true;
            tracked.session.activity = tracked.session.activity.max(session.activity);
            // A late lifecycle entry can still carry the pre-reopen HWND.
            if tracked.project.is_none() || tracked.session.window.is_none_or(|window|
                contexts.get(&window).is_none_or(|context| !context.allows(tracked.session.cwd.as_deref()))) {
                tracked.session.window = session.window;
            }
            if session.cwd.is_some() {
                tracked.session.cwd = session.cwd;
            }
            tracked.last_active = now;
        }
        // Retain finished sessions briefly to drain the final write even if the
        // terminal lifecycle record wins the race with the overlay timer.
        let recent = self.recent_sessions(settings.overlay_max_tabs);
        self.sessions.retain(|id, session| {
            recent.contains(id)
                || session.completion.is_some()
                || now.duration_since(session.last_active) < Duration::from_secs(600)
        });
        self.dismissed.retain(|id| self.sessions.contains_key(id));
        let refresh_connections = self.next_title_refresh.is_none_or(|next| now >= next);
        for tracked in self.sessions.values_mut() {
            if let Some(project) = tracked.session.window.and_then(|window| contexts.get(&window))
                .and_then(|context| context.project(tracked.session.cwd.as_deref())) {
                tracked.project = Some(project);
            }
            // A manually reopened project may now have a different HWND.
            if refresh_connections && let Some(project) = &tracked.project
                && tracked.session.window.is_some_and(|window| contexts.get(&window)
                    .is_none_or(|context| !context.allows(Some(&project.cwd))))
                && let Some(window) = crate::session_navigation::matching_window(project, tracked.session.window.unwrap_or(0)) {
                tracked.session.window = Some(window);
            }
        }
        let refresh_titles =
            !self.sessions.is_empty() && self.next_title_refresh.is_none_or(|next| now >= next);
        if refresh_titles {
            self.titles
                .refresh(&paths::codex_data_directory().join("session_index.jsonl"));
            self.next_title_refresh = Some(now + Duration::from_secs(2));
        }
        for (id, tracked) in &mut self.sessions {
            if tracked.cursor.is_none() && now >= tracked.next_lookup {
                tracked.next_lookup = now + Duration::from_secs(2);
                let (path, cwd) = codex_lifecycle::session_metadata(id);
                if tracked.session.cwd.is_none() {
                    tracked.session.cwd = cwd;
                }
                if let Some(path) = path {
                    tracked.cursor = MessageCursor::new(PathBuf::from(path)).ok();
                    tracked.fallback_title = codex_lifecycle::session_name(id);
                }
            }
            tracked.label = session_label(
                tracked.session.cwd.as_deref(),
                self.titles.title(id).or(tracked.fallback_title.as_deref()),
            );
            let updates = tracked
                .cursor
                .as_mut()
                .map(|cursor| cursor.read_updates().unwrap_or_default())
                .unwrap_or_default();
            for update in updates {
                match update {
                    Update::Started => {
                        self.dismissed.remove(id);
                        tracked.busy = true;
                        tracked.completion = None;
                        self.previews.retain(|preview| &preview.session != id);
                    }
                    Update::Aborted => {
                        tracked.busy = false;
                        tracked.completion = None;
                        if let Some(preview) =
                            self.previews.iter_mut().rev().find(|p| &p.session == id)
                        {
                            preview.card.text =
                                "Session stopped. Open the chat for details.".into();
                            preview.card.final_message = false;
                        }
                    }
                    Update::Completed(message) => {
                        tracked.busy = false;
                        // The terminal record carries the final display text too,
                        // even if a large output burst skipped its earlier record.
                        let text = message.map(|message| message.text).unwrap_or_else(||
                            "Session complete. Open the chat to view the result.".into());
                        // Keep one completion card until this specific chat is viewed.
                        if let Some(preview) =
                            self.previews.iter_mut().rev().find(|p| &p.session == id)
                        {
                            if !preview.card.final_message {
                                preview.card.text = text;
                            }
                            preview.card.final_message = true;
                            tracked.completion = Some((preview.card.id, now));
                        } else {
                            self.next_id = self.next_id.wrapping_add(1);
                            tracked.completion = Some((self.next_id, now));
                            self.previews.push_back(Preview {
                                session: id.clone(),
                                received: now,
                                card: Card {
                                    id: self.next_id,
                                    label: tracked.label.clone(),
                                    text,
                                    final_message: true,
                                    attention: false,
                                    target: None,
                                },
                            });
                        }
                    }
                    Update::Message(message) => {
                        self.next_id = self.next_id.wrapping_add(1);
                        self.previews.push_back(Preview {
                            session: id.clone(),
                            received: now,
                            card: Card {
                                id: self.next_id,
                                label: tracked.label.clone(),
                                text: message.text,
                                final_message: message.final_message,
                                attention: false,
                                target: None,
                            },
                        });
                        // A final display write can follow its terminal lifecycle record.
                        if message.final_message && tracked.completion.is_some() {
                            tracked.completion = Some((self.next_id, now));
                        }
                    }
                }
            }
            // Draining an older task_started record must not resurrect a turn
            // that the guardian has already observed finishing or stopping.
            if !tracked.source_active { tracked.busy = false; }
            if tracked.busy && !self.previews.iter().any(|p| &p.session == id) {
                self.next_id = self.next_id.wrapping_add(1);
                self.previews.push_back(Preview {
                    session: id.clone(),
                    received: now,
                    card: Card {
                        id: self.next_id,
                        label: tracked.label.clone(),
                        text: "Working\u{2026} Updates will appear here.".into(),
                        final_message: false,
                        attention: false,
                        target: None,
                    },
                });
            }
        }
        let focused = win::foreground_editor_window();
        self.focus.observe(
            focused,
            &self
                .sessions
                .values()
                .filter_map(|tracked| tracked.session.window)
                .collect(),
        );
        let focused_sessions: HashSet<_> = self
            .sessions
            .iter()
            .filter(|(_, tracked)| focused.is_some() && tracked.session.window == focused
                && focused.and_then(|window| contexts.get(&window))
                    .is_none_or(|context| context.allows(tracked.session.cwd.as_deref())))
            .map(|(id, _)| id.clone())
            .collect();
        let views = self.views.for_sessions(&focused_sessions);
        self.opened.retain(|_, opened| focused_sessions.contains(&opened.session_id));
        self.reconcile_opened(focused, &views);
        let viewed = self.acknowledge_visible(
            |window| Some(window) == focused,
            |id| views.get(id).map(|view| view.state.clone()),
        );
        self.trim_previews(settings, now);
        let mut frames = self.visible_frames(settings, win::is_editor_window, &HashSet::new());
        for frame in &mut frames {
            frame.hidden_in_focus = frame.session_id.as_ref().is_some_and(|id| viewed.contains(id));
        }
        frames
    }

    fn discard_stale_workspaces(&mut self, allowed: impl Fn(u64, Option<&str>) -> bool) {
        self.sessions.retain(|_, tracked| tracked.session.window
            .is_none_or(|window| allowed(window, tracked.session.cwd.as_deref())));
        self.previews.retain(|preview| self.sessions.contains_key(&preview.session));
        self.opened.retain(|_, opened| self.sessions.contains_key(&opened.session_id));
        self.dismissed.retain(|id| self.sessions.contains_key(id));
        self.was_collapsed.retain(|id| self.sessions.contains_key(id));
    }

    fn dismiss(&mut self, target: &CardTarget, activity: u64) {
        if self.sessions.get(&target.session_id).is_some_and(|tracked|
            tracked.session.window == Some(target.window) && tracked.session.activity == activity) {
            // Closing a notification does not mean the chat has been viewed.
            self.dismissed.insert(target.session_id.clone());
        }
    }

    fn acknowledge(&mut self, target: &CardTarget, viewed: Instant) {
        if let Some(tracked) = self.sessions.get_mut(&target.session_id)
            && (tracked.session.window == Some(target.window)
                || (target.project.is_some() && tracked.project == target.project))
        {
            tracked.session.window = Some(target.window);
            // A successful explicit open is immediate evidence, even when the
            // extension's view log is missing or still describes the previous chat.
            self.opened.insert(
                target.window,
                OpenedChat {
                    session_id: target.session_id.clone(),
                    baseline: None,
                },
            );
            if tracked
                .completion
                .is_some_and(|(_, completed)| completed <= viewed)
            {
                tracked.completion = None;
            }
        }
    }

    fn reconcile_opened(
        &mut self,
        focused: Option<u64>,
        views: &HashMap<String, codex_log::ViewObservation>,
    ) {
        self.opened.retain(|window, opened| {
            if Some(*window) != focused || self.sessions.get(&opened.session_id)
                .is_none_or(|tracked| tracked.session.window != Some(*window)) {
                return false;
            }
            let Some(current) = views.get(&opened.session_id) else { return true; };
            if matches!(&current.state, codex_log::ViewState::Active(id) if id == &opened.session_id) {
                return false; // The normal view filter can now take over.
            }
            match &opened.baseline {
                Some(baseline) => current.revision == baseline.revision,
                None => { opened.baseline = Some(current.clone()); true }
            }
        });
    }

    fn acknowledge_visible(
        &mut self,
        focused: impl Fn(u64) -> bool,
        view: impl Fn(&str) -> Option<codex_log::ViewState>,
    ) -> HashSet<String> {
        let mut viewed = HashSet::new();
        for (id, tracked) in &mut self.sessions {
            if tracked.session.window.is_some_and(&focused)
                && tracked.session.window.is_some_and(|window| {
                    self.opened.get(&window).map_or_else(
                        || matches!(view(id), Some(codex_log::ViewState::Active(active)) if active == *id),
                        |opened| opened.session_id == *id,
                    )
                })
            {
                tracked.completion = None;
                viewed.insert(id.clone());
            }
        }
        viewed
    }

    fn trim_previews(&mut self, settings: &GuardSettings, now: Instant) {
        let mut ordinary = 0;
        let mut kept_busy = std::collections::HashSet::new();
        let recent = self.recent_sessions(settings.overlay_max_tabs);
        let mut kept_recent = HashSet::new();
        // Walk newest first; retain one busy or unread card per session in addition to the cache.
        self.previews.make_contiguous().reverse();
        self.previews.retain(|preview| {
            let Some(tracked) = self.sessions.get(&preview.session) else {
                return false;
            };
            if (recent.contains(&preview.session) && kept_recent.insert(preview.session.clone()))
                || tracked
                    .completion
                    .is_some_and(|(id, _)| id == preview.card.id)
                || (tracked.busy && kept_busy.insert(preview.session.clone()))
            {
                return true;
            }
            ordinary += 1;
            ordinary <= 30
                && now.duration_since(preview.received)
                    < Duration::from_secs(settings.overlay_duration_seconds)
        });
        self.previews.make_contiguous().reverse();
    }

    fn visible_frames(
        &self,
        settings: &GuardSettings,
        is_editor: impl Fn(u64) -> bool,
        viewed: &HashSet<String>,
    ) -> Vec<Frame> {
        // Rank chats by their latest turn, not by how often they emit messages.
        // A chatty older turn must not evict a newer chat or occupy several tabs.
        let mut editors = HashMap::new();
        let mut eligible: Vec<_> = self
            .sessions
            .iter()
            .filter(|(id, _)| !viewed.contains(*id) && !self.dismissed.contains(*id))
            .filter_map(|(id, tracked)| {
                tracked
                    .session
                    .window
                    .filter(|window| tracked.project.is_some()
                        || *editors.entry(*window).or_insert_with(|| is_editor(*window)))
                    .and_then(|window| {
                        let preview = self.previews.iter().rev().find(|preview| {
                            &preview.session == id
                                && tracked
                                    .completion
                                    .is_none_or(|(card, _)| card == preview.card.id)
                        })?;
                        Some((id, tracked, window, preview))
                    })
            })
            .collect();
        eligible.sort_by(|a, b| {
            b.1.session
                .activity
                .cmp(&a.1.session.activity)
                .then_with(|| a.0.cmp(b.0))
        });
        eligible.truncate(settings.overlay_max_tabs.clamp(1, SESSION_LIMIT));
        eligible
            .into_iter()
            .map(|(id, tracked, window, preview)| {
                let mut card = preview.card.clone();
                card.attention = tracked.completion.is_some();
                card.label = tracked.label.clone();
                card.target = Some(CardTarget {
                    window,
                    session_id: id.clone(),
                    project: tracked.project.clone(),
                });
                Frame {
                    session_id: Some(id.clone()),
                    activity: tracked.session.activity,
                    cards: vec![card],
                    window: Some(window),
                    busy: tracked.busy,
                    attention: tracked.completion.is_some(),
                    opacity: settings.overlay_opacity,
                    position: settings.overlay_position.clone(),
                    max_tabs: settings.overlay_max_tabs.clamp(1, SESSION_LIMIT),
                    shortcuts: crate::shortcut_config::ShortcutConfig::from_settings(&settings.overlay_shortcuts),
                    close: false,
                    dock_request: self.focus.request(window),
                    hidden_in_focus: false,
                }
            })
            .collect()
    }
}

fn session_label(cwd: Option<&str>, title: Option<&str>) -> String {
    let folder = cwd
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty())
        .map(|cwd| {
            cwd.trim_start_matches(r"\\?\")
                .trim_end_matches(['/', '\\'])
        })
        .and_then(|cwd| cwd.rsplit(['/', '\\']).next())
        .filter(|folder| !folder.is_empty())
        .unwrap_or("Codex");
    let title = title
        .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled chat".into());
    format!("{folder} \u{2014} {title}")
}

pub fn start(source: impl Fn() -> Vec<Session> + Send + 'static) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    let retained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let keep_alive = retained.clone();
    std::thread::spawn(move || {
        let shortcuts = win::OverlayShortcuts::start()
            .map_err(|cause| {
                logging::write(format!("Overlay shortcuts unavailable: {cause}"));
            })
            .ok();
        let mut feed = Feed::default();
        let (viewed, acknowledgements) = std::sync::mpsc::channel();
        let (dismissed, dismissals) = std::sync::mpsc::channel::<(CardTarget, u64)>();
        let worker = FeedWorker::new(move |collapsed| {
            for (target, activity) in dismissals.try_iter() {
                crate::background::dismiss(&target.session_id, activity);
                feed.dismiss(&target, activity);
            }
            for (target, at) in acknowledgements.try_iter() {
                feed.acknowledge(&target, at);
            }
            let settings = GuardSettings::load();
            let frames = if settings.message_overlay {
                let mut frames = crate::background::frames(&settings);
                frames.extend(feed.frame(source(), &settings, Instant::now(), collapsed));
                frames.sort_by_key(|frame| std::cmp::Reverse(frame.activity));
                frames.truncate(settings.overlay_max_tabs);
                frames
            } else {
                feed = Feed::default();
                vec![]
            };
            keep_alive.store(!frames.is_empty(), std::sync::atomic::Ordering::Relaxed);
            frames
        });
        for slot in 0..SESSION_LIMIT {
            let mut view = worker.view(slot);
            let updates = view.updates().with_dismissals(dismissed.clone());
            let viewed = viewed.clone();
            let shortcuts = shortcuts.as_ref().map(|service| service.publisher(slot));
            std::thread::spawn(move || {
                let result = win::run_session_overlay(
                    slot,
                    |collapsed| view.snapshot(collapsed),
                    move |target: &CardTarget, window| {
                        if crate::background::is_task(&target.session_id) { crate::background::show(&target.session_id).into() }
                        else { win::OverlayOpen::activate(target.clone(), viewed.clone(), window) }
                    },
                    shortcuts,
                    Some(updates),
                );
                if let Err(cause) = result {
                    logging::write(format!("Message overlay stopped: {cause}"));
                }
            });
        }
    });
    retained
}

pub fn preview() -> io::Result<()> {
    let started = Instant::now();
    let settings = GuardSettings::load();
    let mut threads = Vec::new();
    let shortcuts = win::OverlayShortcuts::start()?;
    for slot in 0..settings.overlay_max_tabs.min(3) {
        let settings = settings.clone();
        let shortcuts = shortcuts.publisher(slot);
        threads.push(std::thread::spawn(move || {
            win::run_session_overlay(slot, |_| {
                let elapsed = started.elapsed();
                let completed = elapsed >= Duration::from_secs(15 + slot as u64 * 3);
                let text = if completed {
                    "This chat is complete. Its tab color fades while the session initials stay steady. The expanded panel has a green completion dot. This demo closes automatically."
                } else {
                    "Each chat has its own tab. Click this message to tuck away just this panel, then hover over its tab to slide it back. Move away to tuck it again. Each chat keeps its own status."
                };
                Frame {
                    session_id: Some(format!("preview-{slot}")),
                    activity: 0,
                    cards: vec![Card {
                        id: 0,
                        label: format!("Codex Lid Guard \u{2014} {} preview", ["Build", "Review", "Tests"][slot]),
                        text: text.into(),
                        final_message: completed,
                        attention: completed,
                        target: None,
                    }],
                    window: None,
                    opacity: settings.overlay_opacity,
                    position: settings.overlay_position.clone(),
                    max_tabs: settings.overlay_max_tabs.min(3),
                    shortcuts: crate::shortcut_config::ShortcutConfig::from_settings(&settings.overlay_shortcuts),
                    close: elapsed >= Duration::from_secs(35),
                    busy: !completed,
                    attention: completed,
                    dock_request: 1,
                    hidden_in_focus: false,
                }
            }, |_, _| false.into(), Some(shortcuts), None)
        }));
    }
    for thread in threads {
        thread
            .join()
            .map_err(|_| io::Error::other("Overlay preview thread stopped"))??;
    }
    Ok(())
}

struct MessageCursor {
    path: PathBuf,
    offset: u64,
    pending: Vec<u8>,
    discard_line: bool,
}

impl MessageCursor {
    fn new(path: PathBuf) -> io::Result<Self> {
        // Start at EOF: enabling previews never replays old conversations.
        let offset = std::fs::metadata(&path)?.len();
        let discard_line = if offset > 0 {
            let mut file = File::open(&path)?;
            file.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            file.read_exact(&mut last)?;
            last[0] != b'\n'
        } else {
            false
        };
        Ok(Self {
            path,
            offset,
            pending: Vec::new(),
            discard_line,
        })
    }

    fn read_updates(&mut self) -> io::Result<Vec<Update>> {
        let mut file = File::open(&self.path)?;
        let length = file.metadata()?.len();
        if length < self.offset {
            self.offset = 0;
            self.pending.clear();
            self.discard_line = false;
        }
        if length.saturating_sub(self.offset) > READ_LIMIT {
            // A large tool write must not delay completion behind pages of output.
            // Previews show the latest updates, so skip the backlog at a line boundary.
            self.offset = length - READ_LIMIT;
            self.pending.clear();
            file.seek(SeekFrom::Start(self.offset - 1))?;
            let mut previous = [0];
            file.read_exact(&mut previous)?;
            self.discard_line = previous[0] != b'\n';
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.take(READ_LIMIT).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        let mut messages = Vec::new();
        for byte in bytes {
            if byte == b'\n' {
                if !self.discard_line
                    && let Some(message) = parse_update(&self.pending)
                {
                    messages.push(message);
                }
                self.pending.clear();
                self.discard_line = false;
            } else if !self.discard_line {
                self.pending.push(byte);
                if self.pending.len() > LINE_LIMIT {
                    self.pending.clear();
                    self.discard_line = true;
                }
            }
        }
        Ok(messages)
    }

    #[cfg(test)]
    fn read_messages(&mut self) -> io::Result<Vec<AssistantMessage>> {
        Ok(self
            .read_updates()?
            .into_iter()
            .filter_map(|update| match update {
                Update::Message(message) => Some(message),
                _ => None,
            })
            .collect())
    }
}

#[derive(Debug, PartialEq)]
enum Update {
    Message(AssistantMessage),
    Started,
    Completed(Option<AssistantMessage>),
    Aborted,
}

fn parse_update(line: &[u8]) -> Option<Update> {
    let event: Event = serde_json::from_slice(line).ok()?;
    if event.kind != "event_msg" {
        return None;
    }
    match event.payload {
        Payload::Started => Some(Update::Started),
        Payload::Completed { last_agent_message } => Some(Update::Completed(
            last_agent_message.as_deref().and_then(|text| display_message(text, Some("final_answer"))),
        )),
        Payload::Aborted => Some(Update::Aborted),
        payload => message_from_payload(payload).map(Update::Message),
    }
}

#[derive(Debug, PartialEq)]
struct AssistantMessage {
    text: String,
    final_message: bool,
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    payload: Payload,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Payload {
    #[serde(rename = "task_started")]
    Started,
    #[serde(rename = "task_complete")]
    Completed {
        last_agent_message: Option<String>,
    },
    #[serde(rename = "turn_aborted")]
    Aborted,
    #[serde(rename = "agent_message")]
    AgentMessage {
        message: String,
        phase: Option<String>,
    },
    #[serde(rename = "item_completed")]
    ItemCompleted {
        item: CompletedItem,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum CompletedItem {
    AgentMessage {
        content: Vec<AgentContent>,
        phase: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AgentContent {
    Text { text: String },
    #[serde(other)]
    Other,
}

#[cfg(test)]
fn parse_message(line: &[u8]) -> Option<AssistantMessage> {
    let event: Event = serde_json::from_slice(line).ok()?;
    if event.kind != "event_msg" {
        return None;
    }
    message_from_payload(event.payload)
}

fn message_from_payload(payload: Payload) -> Option<AssistantMessage> {
    // Codex writes either legacy agent_message or item_completed/AgentMessage
    // display events. response_item mirrors are deliberately not read, avoiding
    // duplicate previews and keeping reasoning, tools and user prompts excluded.
    match payload {
        Payload::AgentMessage { message, phase } => display_message(&message, phase.as_deref()),
        Payload::ItemCompleted { item: CompletedItem::AgentMessage { content, phase } } => {
            let text = content.into_iter().filter_map(|part| match part {
                AgentContent::Text { text } => Some(text),
                AgentContent::Other => None,
            }).collect::<Vec<_>>().join("\n");
            display_message(&text, phase.as_deref())
        }
        _ => None,
    }
}

fn display_message(message: &str, phase: Option<&str>) -> Option<AssistantMessage> {
    if !matches!(phase, None | Some("commentary" | "final_answer")) {
        return None;
    }
    let mut text: String = message
        .trim()
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(1400)
        .collect();
    if text.is_empty() {
        return None;
    }
    if message.trim().chars().count() > 1400 {
        text.push('…');
    }
    Some(AssistantMessage {
        text,
        final_message: phase == Some("final_answer"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_open_hides_only_its_chat_across_stale_logs_and_releases_on_a_new_view_or_focus_loss()
     {
        use codex_log::{ViewObservation, ViewState};
        let now = Instant::now();
        let mut feed = Feed::default();
        for (id, window) in [("one", 1), ("two", 1), ("three", 2)] {
            feed.sessions.insert(id.into(), tracked(id, window, now));
        }
        let target = CardTarget { project: None,
            window: 1,
            session_id: "one".into(),
        };
        let hidden = |feed: &mut Feed, views: &HashMap<String, ViewObservation>| {
            feed.acknowledge_visible(
                |window| window == 1,
                |id| views.get(id).map(|view| view.state.clone()),
            )
        };
        let views = |state: ViewState, revision| {
            ["one", "two"]
                .into_iter()
                .map(|id| {
                    (
                        id.into(),
                        ViewObservation {
                            state: state.clone(),
                            revision,
                        },
                    )
                })
                .collect()
        };
        feed.acknowledge(&target, now);
        feed.reconcile_opened(Some(1), &HashMap::new());
        assert_eq!(
            hidden(&mut feed, &HashMap::new()),
            HashSet::from(["one".into()])
        );
        let stale = views(ViewState::Active("two".into()), 1);
        for _ in 0..4 {
            feed.reconcile_opened(Some(1), &stale);
            assert_eq!(
                hidden(&mut feed, &stale),
                HashSet::from(["one".into()]),
                "stale view must not also hide the previous chat"
            );
        }
        // Even selecting the same chat named in the stale snapshot is a new event.
        let switched = views(ViewState::Active("two".into()), 2);
        feed.reconcile_opened(Some(1), &switched);
        assert_eq!(hidden(&mut feed, &switched), HashSet::from(["two".into()]));
        assert!(feed.opened.is_empty());

        feed.acknowledge(&target, now);
        let confirmed = views(ViewState::Active("one".into()), 3);
        feed.reconcile_opened(Some(1), &confirmed);
        assert!(feed.opened.is_empty());
        assert_eq!(hidden(&mut feed, &confirmed), HashSet::from(["one".into()]));
        let inactive = views(ViewState::Inactive, 4);
        assert!(hidden(&mut feed, &inactive).is_empty());

        feed.acknowledge(&target, now);
        feed.reconcile_opened(None, &HashMap::new());
        assert!(
            feed.opened.is_empty(),
            "switching apps must make the tab available again"
        );
        feed.acknowledge(
            &CardTarget { project: None,
                window: 99,
                session_id: "one".into(),
            },
            now,
        );
        assert!(
            feed.opened.is_empty(),
            "a stale window must not hide this session"
        );
    }

    #[test]
    fn focus_loss_docks_once_without_overriding_later_hover_or_other_editors() {
        let mut focus = FocusTransitions::default();
        let windows = HashSet::from([10, 20]);
        focus.observe(Some(10), &windows);
        assert_eq!(focus.request(10), 1);
        focus.observe(None, &windows); // Minimize or foreground a different app.
        assert_eq!(focus.request(10), 2);
        for _ in 0..20 {
            focus.observe(None, &windows);
        }
        assert_eq!(
            focus.request(10),
            2,
            "polling must not undo a hover expansion"
        );
        assert_eq!(focus.request(20), 1);
        focus.observe(Some(10), &windows);
        focus.observe(Some(20), &windows);
        assert_eq!(
            focus.request(10),
            3,
            "switching editor windows also docks the origin"
        );
        focus.observe(Some(20), &windows);
        assert_eq!(
            focus.request(20),
            1,
            "the focused editor must not inherit the other request"
        );
        focus.observe(None, &HashSet::new());
        assert!(
            focus.epochs.is_empty(),
            "closed editors must not accumulate forever"
        );
    }
    use std::io::Write;

    #[test]
    fn labels_use_project_folder_and_chat_title_without_session_ids() {
        assert_eq!(
            session_label(
                Some(r"\\?\C:\Projects\CodexLidGuard\"),
                Some("Add transparent VS Code overlay")
            ),
            "CodexLidGuard \u{2014} Add transparent VS Code overlay"
        );
        assert_eq!(
            session_label(
                Some("/projects/Guardian/"),
                Some("  Dry run\n latest package  ")
            ),
            "Guardian \u{2014} Dry run latest package"
        );
        assert_eq!(
            session_label(Some(r"C:\Projects\NewProject"), None),
            "NewProject \u{2014} Untitled chat"
        );
        assert_eq!(
            session_label(None, Some("Task title")),
            "Codex \u{2014} Task title"
        );
    }

    fn write_event(file: &mut File, line: &str) -> io::Result<()> {
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")
    }

    fn event(text: &str) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(&serde_json::json!({"type":"event_msg", "payload":{"type":"agent_message", "message":text, "phase":"commentary"}})).unwrap();
        bytes.push(b'\n');
        bytes
    }

    impl Feed {
        fn test_frame(&self, settings: &GuardSettings, editor: impl Fn(u64) -> bool) -> Frame {
            let frames = self.visible_frames(settings, editor, &HashSet::new());
            Frame {
                window: frames.first().and_then(|frame| frame.window),
                busy: frames.iter().any(|frame| frame.busy),
                attention: frames.iter().any(|frame| frame.attention),
                cards: frames.into_iter().flat_map(|frame| frame.cards).collect(),
                ..Frame::empty()
            }
        }
        fn test_poll(
            &mut self,
            active: Vec<Session>,
            settings: &GuardSettings,
            now: Instant,
            collapsed: bool,
        ) -> Frame {
            let ids = if collapsed {
                self.sessions.keys().cloned().collect()
            } else {
                HashSet::new()
            };
            self.frame(active, settings, now, &ids);
            self.test_frame(settings, win::is_editor_window)
        }
    }

    fn tracked(id: &str, window: u64, now: Instant) -> TrackedSession {
        TrackedSession {
            project: None,
            session: Session {
                activity: 0,
                id: id.into(),
                cwd: None,
                window: Some(window),
            },
            label: id.into(),
            fallback_title: None,
            cursor: None,
            last_active: now,
            next_lookup: now + Duration::from_secs(99999),
            source_active: false,
            busy: false,
            completion: None,
        }
    }

    #[test]
    fn retained_project_tabs_survive_closed_windows_and_rebind_only_after_confirmed_open() {
        let now = Instant::now();
        let mut feed = Feed::default();
        let project = crate::session_navigation::Project { cwd: r"C:\One".into(),
            path: r"C:\One".into(), executable: r"C:\VS Code\Code.exe".into() };
        let mut session = tracked("one", 10, now);
        session.project = Some(project.clone());
        session.completion = Some((1, now));
        feed.sessions.insert("one".into(), session);
        feed.previews.push_back(Preview { session: "one".into(), received: now,
            card: Card { id: 1, label: "One".into(), text: "Finished".into(), final_message: true,
                attention: true, target: None } });
        let settings = GuardSettings::default();
        feed.trim_previews(&settings, now + Duration::from_secs(3600));
        let frames = feed.visible_frames(&settings, |_| false, &HashSet::new());
        assert_eq!(frames.len(), 1, "a closed HWND must not hide a saved project");
        assert!(frames[0].attention, "closing the editor is not acknowledgement");
        assert_eq!(frames[0].cards[0].target.as_ref().unwrap().project, Some(project.clone()));
        feed.acknowledge(&CardTarget { window: 20, session_id: "one".into(), project: None }, now);
        assert_eq!(feed.sessions["one"].session.window, Some(10), "an unrelated open cannot take ownership");
        feed.acknowledge(&CardTarget { window: 20, session_id: "one".into(), project: Some(project) }, now);
        assert_eq!(feed.sessions["one"].session.window, Some(20));
        assert!(feed.sessions["one"].completion.is_none());
        let target = feed.visible_frames(&settings, |_| false, &HashSet::new())[0].cards[0].target.clone().unwrap();
        feed.dismiss(&target, 0);
        assert!(feed.visible_frames(&settings, |_| false, &HashSet::new()).is_empty(), "dismissed tabs allow idle shutdown");
    }

    #[test]
    fn changing_folders_removes_old_busy_and_completed_tabs_but_keeps_other_windows() {
        let now = Instant::now();
        let mut feed = Feed::default();
        for (index, id, window, cwd) in [
            (1, "old-busy", 10, r"C:\Old"),
            (2, "old-complete", 10, r"C:\Old"),
            (3, "new", 10, r"C:\New"),
            (4, "other-window", 20, r"C:\Old"),
        ] {
            let mut session = tracked(id, window, now);
            session.session.cwd = Some(cwd.into());
            session.busy = index == 1;
            session.completion = (index == 2).then_some((index, now));
            feed.sessions.insert(id.into(), session);
            feed.was_collapsed.insert(id.into());
            feed.previews.push_back(Preview {
                session: id.into(), received: now,
                card: Card { id: index, label: id.into(), text: "update".into(),
                    final_message: index == 2, attention: index == 2, target: None },
            });
        }
        feed.discard_stale_workspaces(|window, cwd| window != 10
            || cwd.is_some_and(|cwd| crate::session_navigation::belongs_to_workspace(cwd, &[r"C:\New".into()])));
        assert_eq!(feed.sessions.len(), 2);
        assert!(feed.sessions.contains_key("new"));
        assert!(feed.sessions.contains_key("other-window"));
        assert_eq!(feed.previews.len(), 2);
        assert!(!feed.was_collapsed.contains("old-complete"));
        let frames = feed.visible_frames(&GuardSettings::default(), |_| true, &HashSet::new());
        assert_eq!(frames.len(), 2, "removed tabs must not survive in the frame cache");
        feed.discard_stale_workspaces(|window, _| window != 10);
        assert_eq!(feed.sessions.len(), 1, "closing the replacement host clears its tab too");
    }

    #[test]
    fn latest_three_chats_have_independent_messages_status_and_targets() {
        let now = Instant::now();
        let mut feed = Feed::default();
        for activity in 1..=4 {
            let id = format!("chat-{activity}");
            let mut session = tracked(&id, 10, now);
            session.session.activity = activity;
            session.busy = activity == 3;
            session.completion = (activity == 2).then_some((activity, now));
            feed.sessions.insert(id.clone(), session);
            feed.previews.push_back(Preview {
                session: id,
                received: now,
                card: Card {
                    id: activity,
                    label: "chat".into(),
                    text: activity.to_string(),
                    final_message: activity == 2,
                    attention: false,
                    target: None,
                },
            });
        }
        // The oldest chat can emit many messages without taking all three tabs.
        for id in 5..105 {
            feed.previews.push_back(Preview {
                session: "chat-1".into(),
                received: now,
                card: Card {
                    id,
                    text: "chatty older task".into(),
                    ..feed.previews[0].card.clone()
                },
            });
        }
        let probes = std::cell::Cell::new(0);
        let settings = GuardSettings::default();
        let frames = feed.visible_frames(
            &settings,
            |_| {
                probes.set(probes.get() + 1);
                true
            },
            &HashSet::new(),
        );
        assert_eq!(
            probes.get(),
            1,
            "check a shared editor window only once per poll"
        );
        assert_eq!(
            frames
                .iter()
                .map(|f| f.session_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["chat-4", "chat-3", "chat-2"]
        );
        for frame in &frames {
            assert_eq!(frame.cards.len(), 1);
            assert_eq!(
                frame.cards[0].target.as_ref().unwrap().session_id,
                *frame.session_id.as_ref().unwrap()
            );
        }
        assert!(!frames[0].busy && !frames[0].attention);
        assert!(frames[1].busy && !frames[1].attention);
        assert!(!frames[2].busy && frames[2].attention && frames[2].cards[0].attention);
        feed.sessions.get_mut("chat-1").unwrap().session.activity = 5;
        let next = feed.visible_frames(&settings, |_| true, &HashSet::new());
        assert_eq!(next[0].session_id.as_deref(), Some("chat-1"));
        assert_eq!(next[0].cards[0].id, 104);
        assert!(
            next.iter()
                .all(|f| f.session_id.as_deref() != Some("chat-2"))
        );
        assert!(
            feed.sessions["chat-2"].completion.is_some(),
            "evicting a tab is not acknowledging its chat"
        );
    }

    #[test]
    fn configured_tab_limit_keeps_newest_chats_and_changes_without_restarting_feed() {
        let now = Instant::now();
        let mut feed = Feed::default();
        for activity in 1..=12 {
            let id = format!("chat-{activity}");
            let mut session = tracked(&id, 10, now);
            session.session.activity = activity;
            feed.sessions.insert(id.clone(), session);
            feed.previews.push_back(Preview { session: id, received: now,
                card: Card { id: activity, label: "Chat".into(), text: "Message".into(),
                    final_message: false, attention: false, target: None } });
        }
        for limit in [3, 10, 1, 5] {
            let settings = GuardSettings { overlay_max_tabs: limit, ..Default::default() };
            let frames = feed.visible_frames(&settings, |_| true, &HashSet::new());
            assert_eq!(frames.len(), limit);
            for (index, frame) in frames.iter().enumerate() {
                assert_eq!(frame.session_id, Some(format!("chat-{}", 12 - index)));
                assert_eq!(frame.max_tabs, limit);
                assert_eq!(frame.cards[0].target.as_ref().unwrap().session_id, frame.session_id.as_ref().unwrap().as_str());
            }
            assert_eq!(feed.recent_sessions(limit).len(), limit);
        }
    }

    #[test]
    fn collapsing_one_chat_does_not_pause_expiry_for_other_chats() {
        let now = Instant::now();
        let mut feed = Feed::default();
        for id in ["one", "two"] {
            feed.sessions.insert(id.into(), tracked(id, 1, now));
            feed.previews.push_back(Preview {
                session: id.into(),
                received: now,
                card: Card {
                    id: feed.previews.len() as u64,
                    label: id.into(),
                    text: "update".into(),
                    final_message: false,
                    attention: false,
                    target: None,
                },
            });
        }
        let settings = GuardSettings::default();
        // Ordinary cached history still expires independently after newer chats replace it.
        for activity in 1..=3 {
            let id = format!("newer-{activity}");
            let mut session = tracked(&id, 1, now);
            session.session.activity = activity;
            feed.sessions.insert(id, session);
        }
        let later = now + Duration::from_secs(100);
        feed.pause_expiry(&HashSet::from(["one".into()]), later);
        feed.trim_previews(&settings, later);
        assert_eq!(feed.previews.len(), 1);
        assert_eq!(feed.previews[0].session, "one");
        let reopened = later + Duration::from_secs(100);
        feed.pause_expiry(&HashSet::new(), reopened);
        feed.trim_previews(&settings, reopened);
        assert_eq!(feed.previews.len(), 1);
        feed.pause_expiry(&HashSet::new(), reopened + Duration::from_secs(91));
        feed.trim_previews(&settings, reopened + Duration::from_secs(91));
        assert!(feed.previews.is_empty());
    }

    #[test]
    fn promoting_turn_metadata_does_not_clear_an_already_received_completion() {
        let now = Instant::now();
        let mut session = tracked("one", 1, now);
        session.source_active = true;
        session.completion = Some((1, now));
        let mut promoted = session.session.clone();
        promoted.activity = 2;
        let mut feed = Feed::default();
        feed.sessions.insert("one".into(), session);
        feed.previews.push_back(Preview {
            session: "one".into(),
            received: now,
            card: Card {
                id: 1,
                label: "chat".into(),
                text: "Result".into(),
                final_message: true,
                attention: false,
                target: None,
            },
        });
        feed.frame(
            vec![promoted],
            &GuardSettings::default(),
            now,
            &HashSet::new(),
        );
        assert!(feed.sessions["one"].completion.is_some());
        assert!(!feed.sessions["one"].busy);
        assert_eq!(feed.previews[0].card.text, "Result");
        assert_eq!(feed.sessions["one"].session.activity, 2);
    }

    #[test]
    fn completion_survives_expiry_and_new_messages_until_its_own_chat_is_viewed() {
        let now = Instant::now();
        let mut feed = Feed::default();
        feed.sessions.insert("one".into(), tracked("one", 1, now));
        feed.sessions.insert("two".into(), tracked("two", 2, now));
        for id in 1..=40 {
            let session = if id <= 2 { "one" } else { "two" };
            feed.previews.push_back(Preview {
                session: session.into(),
                received: now,
                card: Card {
                    id,
                    label: session.into(),
                    text: "update".into(),
                    final_message: id == 2,
                    attention: false,
                    target: None,
                },
            });
        }
        feed.sessions.get_mut("one").unwrap().completion = Some((2, now));
        feed.sessions.get_mut("two").unwrap().completion = Some((40, now));
        let settings = GuardSettings::default();
        feed.trim_previews(&settings, now);
        let frame = feed.test_frame(&settings, |_| true);
        assert!(
            frame
                .cards
                .iter()
                .any(|card| card.id == 2 && card.attention)
        );
        assert!(frame.attention);
        feed.test_poll(vec![], &settings, now + Duration::from_secs(86400), false);
        assert_eq!(
            feed.previews.len(),
            2,
            "unread completions must outlive preview and session expiry"
        );

        // A view in another window, an unrelated chat, or a hidden Codex panel is not acknowledgement.
        feed.acknowledge_visible(
            |_| false,
            |_| panic!("must not read logs for an unfocused editor"),
        );
        feed.acknowledge_visible(
            |_| true,
            |_| Some(codex_log::ViewState::Active("unrelated".into())),
        );
        feed.acknowledge_visible(|_| true, |_| Some(codex_log::ViewState::Inactive));
        assert!(feed.sessions.values().all(|s| s.completion.is_some()));
        feed.acknowledge_visible(
            |window| window == 1,
            |_| Some(codex_log::ViewState::Active("one".into())),
        );
        assert!(feed.sessions["one"].completion.is_none());
        assert!(feed.sessions["two"].completion.is_some());

        // An old click must not acknowledge a result that completed after that click.
        let target = CardTarget { project: None,
            window: 2,
            session_id: "two".into(),
        };
        feed.acknowledge(&target, now - Duration::from_secs(1));
        assert!(feed.sessions["two"].completion.is_some());
        feed.acknowledge(
            &CardTarget { project: None,
                window: 1,
                ..target.clone()
            },
            now,
        );
        assert!(feed.sessions["two"].completion.is_some());
        feed.acknowledge(&target, now);
        assert!(feed.sessions["two"].completion.is_none());
        assert!(!feed.test_frame(&settings, |_| true).attention);
        feed.trim_previews(&settings, now + Duration::from_secs(86400));
        assert_eq!(
            feed.previews.len(),
            2,
            "latest updates remain available when switching away again"
        );
    }

    #[test]
    fn closing_one_session_keeps_it_hidden_until_a_new_turn_without_viewing_it() {
        let now = Instant::now();
        let path = std::env::temp_dir().join(format!("overlay-close-{}.jsonl", std::process::id()));
        std::fs::write(&path, []).unwrap();
        let mut feed = Feed::default();
        let mut session = tracked("one", 1, now);
        session.cursor = Some(MessageCursor::new(path.clone()).unwrap());
        let other = tracked("two", 1, now);
        let active = vec![session.session.clone(), other.session.clone()];
        feed.sessions.insert("one".into(), session);
        feed.sessions.insert("two".into(), other);
        let settings = GuardSettings::default();
        feed.test_poll(active.clone(), &settings, now, false);
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"task_complete"}}"#).unwrap();
        feed.test_poll(active.clone(), &settings, now, false);
        let target = CardTarget { project: None, window: 1, session_id: "one".into() };
        feed.dismiss(&target, 99);
        assert!(feed.dismissed.is_empty(), "a stale close must not dismiss another turn");
        feed.dismiss(&target, 0);
        assert!(feed.sessions["one"].completion.is_some(), "close is not viewed");
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"agent_message","phase":"final_answer","message":"Final write after completion"}}"#).unwrap();
        feed.test_poll(active.clone(), &settings, now, false);
        for _ in 0..3 {
            let frames = feed.visible_frames(&settings, |_| true, &HashSet::new());
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0].session_id.as_deref(), Some("two"));
            assert!(frames[0].busy);
        }
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"task_started"}}"#).unwrap();
        feed.test_poll(active, &settings, now, false);
        assert!(!feed.dismissed.contains("one"));
        assert_eq!(feed.visible_frames(&settings, |_| true, &HashSet::new()).len(), 2);
        assert!(feed.sessions["one"].busy);
        assert!(feed.sessions["one"].completion.is_none());
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn completion_is_observed_on_the_first_poll_after_a_large_output_burst() {
        let now = Instant::now();
        let path = std::env::temp_dir().join(format!("overlay-burst-{}.jsonl", std::process::id()));
        std::fs::write(&path, []).unwrap();
        let mut session = tracked("one", 1, now);
        session.cursor = Some(MessageCursor::new(path.clone()).unwrap());
        let active = vec![session.session.clone()];
        let mut feed = Feed::default();
        feed.sessions.insert("one".into(), session);
        let settings = GuardSettings::default();
        feed.test_poll(active.clone(), &settings, now, false);
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        // More than eight reader pages from a single tool write. Only lifecycle
        // and assistant display records at the end matter to the notification.
        write_event(&mut file, &"x".repeat(READ_LIMIT as usize * 8)).unwrap();
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"agent_message","phase":"final_answer","message":"Finished"}}"#).unwrap();
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"task_complete"}}"#).unwrap();
        feed.test_poll(active, &settings, now, false);
        let frame = feed.test_frame(&settings, |_| true);
        assert!(!frame.busy);
        assert!(frame.attention);
        assert_eq!(frame.cards[0].text, "Finished");
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn draining_a_start_record_cannot_make_a_finished_source_busy_again() {
        let now = Instant::now();
        let path = std::env::temp_dir().join(format!("overlay-drain-{}.jsonl", std::process::id()));
        std::fs::write(&path, []).unwrap();
        let mut session = tracked("one", 1, now);
        session.cursor = Some(MessageCursor::new(path.clone()).unwrap());
        session.source_active = true;
        session.busy = true;
        let mut feed = Feed::default();
        feed.sessions.insert("one".into(), session);
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"task_started"}}"#).unwrap();
        feed.test_poll(vec![], &GuardSettings::default(), now, false);
        assert!(!feed.sessions["one"].busy);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn busy_turns_keep_a_card_and_completion_requires_a_terminal_event_not_a_final_message() {
        let now = Instant::now();
        let path =
            std::env::temp_dir().join(format!("overlay-lifecycle-{}.jsonl", std::process::id()));
        std::fs::write(&path, []).unwrap();
        let mut session = tracked("one", 1, now);
        session.cursor = Some(MessageCursor::new(path.clone()).unwrap());
        let active = vec![session.session.clone()];
        let mut feed = Feed::default();
        feed.sessions.insert("one".into(), session);
        let settings = GuardSettings::default();
        feed.test_poll(active.clone(), &settings, now, false);
        assert!(feed.test_frame(&settings, |_| true).busy);
        assert_eq!(
            feed.previews.len(),
            1,
            "busy must be visible before its first message"
        );
        assert!(!feed.test_frame(&settings, |_| false).busy);
        feed.test_poll(
            active.clone(),
            &settings,
            now + Duration::from_secs(700),
            false,
        );
        assert_eq!(
            feed.previews.len(),
            1,
            "a quiet busy turn must keep its notch"
        );
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"agent_message","message":"Result","phase":"final_answer"}}"#).unwrap();
        feed.test_poll(
            active.clone(),
            &settings,
            now + Duration::from_secs(701),
            false,
        );
        assert!(feed.sessions["one"].completion.is_none());
        write_event(
            &mut file,
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}"#,
        )
        .unwrap();
        feed.test_poll(
            active.clone(),
            &settings,
            now + Duration::from_secs(702),
            false,
        );
        let frame = feed.test_frame(&settings, |_| true);
        assert!(frame.attention);
        assert!(
            !frame.busy,
            "terminal transcript wins over a lagging active snapshot"
        );
        assert!(frame.cards.last().unwrap().attention);
        assert_eq!(frame.cards.last().unwrap().text, "Result");
        feed.test_poll(
            active.clone(),
            &settings,
            now + Duration::from_secs(703),
            true,
        );
        assert!(!feed.sessions["one"].busy);
        assert!(
            feed.sessions["one"].completion.is_some(),
            "tucking or unfolding is not viewing the chat"
        );
        write_event(
            &mut file,
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-2"}}"#,
        )
        .unwrap();
        feed.test_poll(
            active.clone(),
            &settings,
            now + Duration::from_secs(704),
            false,
        );
        assert!(feed.sessions["one"].busy);
        assert!(feed.sessions["one"].completion.is_none());
        assert!(
            feed.previews
                .iter()
                .all(|p| !p.card.final_message && p.card.text != "Result"),
            "a new turn must not display the previous turn's result as Done"
        );
        write_event(
            &mut file,
            r#"{"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-2"}}"#,
        )
        .unwrap();
        feed.test_poll(active, &settings, now + Duration::from_secs(705), false);
        assert!(!feed.sessions["one"].busy);
        assert!(
            feed.sessions["one"].completion.is_none(),
            "cancellation is not completion"
        );
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn shows_latest_updates_only_for_valid_editor_windows_and_retains_them() {
        let now = Instant::now();
        let mut feed = Feed::default();
        for (id, window) in [("one", Some(1)), ("two", Some(2)), ("unknown", None)] {
            feed.sessions.insert(
                id.into(),
                TrackedSession {
                    project: None,
                    session: Session {
                        activity: 0,
                        id: id.into(),
                        cwd: None,
                        window,
                    },
                    label: id.into(),
                    fallback_title: None,
                    cursor: None,
                    last_active: now,
                    next_lookup: now,
                    source_active: false,
                    busy: false,
                    completion: None,
                },
            );
        }
        for id in ["one", "one", "two", "one", "one", "unknown"] {
            feed.previews.push_back(Preview {
                session: id.into(),
                received: now,
                card: Card {
                    id: feed.previews.len() as u64,
                    label: id.into(),
                    text: feed.previews.len().to_string(),
                    final_message: false,
                    attention: false,
                    target: None,
                },
            });
        }
        let settings = GuardSettings::default();
        let frame = feed.test_frame(&settings, |window| window == 1);
        assert_eq!(
            frame
                .cards
                .iter()
                .map(|card| card.text.as_str())
                .collect::<Vec<_>>(),
            ["4"]
        );
        assert_eq!(frame.window, Some(1));
        feed.sessions.get_mut("one").unwrap().label = "Project \u{2014} Renamed chat".into();
        assert!(
            feed.test_frame(&settings, |window| window == 1)
                .cards
                .iter()
                .all(|card| card.label == "Project \u{2014} Renamed chat")
        );
        assert!(frame.cards.iter().all(|card| {
            card.target
                .as_ref()
                .is_some_and(|target| target.window == 1 && target.session_id == "one")
        }));
        assert!(feed.test_frame(&settings, |_| false).cards.is_empty());
        assert_eq!(feed.test_frame(&settings, |_| true).cards.len(), 2);
        let later = now + Duration::from_secs(91);
        assert!(
            feed.test_poll(vec![], &settings, later, false)
                .cards
                .is_empty()
        );
        assert_eq!(
            feed.previews.len(),
            2,
            "the latest chat tabs must not expire while another window is on top"
        );
    }

    #[test]
    fn tabs_hide_only_for_the_matching_chat_in_the_focused_editor_and_return_after_viewing() {
        let now = Instant::now();
        let settings = GuardSettings::default();
        let mut feed = Feed::default();
        for (id, window, card) in [("one", 1, 1), ("two", 1, 2), ("three", 2, 3)] {
            let mut session = tracked(id, window, now);
            session.session.activity = card;
            session.busy = id == "one";
            session.completion = (id != "one").then_some((card, now));
            feed.sessions.insert(id.into(), session);
            feed.previews.push_back(Preview {
                session: id.into(),
                received: now,
                card: Card {
                    id: card,
                    label: id.into(),
                    text: "latest update".into(),
                    final_message: id != "one",
                    attention: false,
                    target: None,
                },
            });
        }
        let shown = |feed: &Feed, viewed: &HashSet<String>| {
            feed.visible_frames(&settings, |_| true, viewed)
                .into_iter()
                .map(|frame| frame.session_id.unwrap())
                .collect::<HashSet<_>>()
        };
        let all = HashSet::from(["one".into(), "two".into(), "three".into()]);
        // Minimized, covered, and another VS Code window in front share the same rule.
        for focused in [None, Some(99)] {
            let viewed = feed.acknowledge_visible(
                |window| Some(window) == focused,
                |_| panic!("background chats must not trigger log reads"),
            );
            assert_eq!(shown(&feed, &viewed), all);
        }
        let viewed = feed.acknowledge_visible(
            |window| window == 1,
            |_| Some(codex_log::ViewState::Active("one".into())),
        );
        assert_eq!(
            shown(&feed, &viewed),
            HashSet::from(["two".into(), "three".into()])
        );
        assert!(feed.sessions["two"].completion.is_some());
        let viewed = feed.acknowledge_visible(
            |window| window == 1,
            |_| Some(codex_log::ViewState::Active("two".into())),
        );
        assert_eq!(
            shown(&feed, &viewed),
            HashSet::from(["one".into(), "three".into()])
        );
        assert!(feed.sessions["two"].completion.is_none());
        assert!(feed.sessions["three"].completion.is_some());
        // Selecting an editor tab, hiding the chat panel, or unknown view metadata is not viewing a chat.
        for view in [
            None,
            Some(codex_log::ViewState::Inactive),
            Some(codex_log::ViewState::Active("untracked-chat".into())),
        ] {
            let viewed = feed.acknowledge_visible(|window| window == 1, |_| view.clone());
            assert_eq!(shown(&feed, &viewed), all);
        }
        feed.frame(
            vec![],
            &settings,
            now + Duration::from_secs(86400),
            &HashSet::new(),
        );
        assert_eq!(
            feed.sessions.len(),
            3,
            "latest chats survive the old ten-minute session timeout"
        );
        assert_eq!(shown(&feed, &HashSet::new()), all);
        let restored = feed.visible_frames(&settings, |_| true, &HashSet::new());
        assert!(
            !restored
                .iter()
                .find(|frame| frame.session_id.as_deref() == Some("two"))
                .unwrap()
                .attention,
            "switching away restores the tab without re-arming its acknowledged completion"
        );
        assert!(
            feed.visible_frames(&settings, |_| false, &HashSet::new())
                .is_empty(),
            "closed editor windows have no overlay"
        );
    }

    #[test]
    fn completed_agent_items_provide_commentary_and_final_display_text() {
        for phase in [None, Some("commentary"), Some("final_answer")] {
            let line = serde_json::to_vec(&serde_json::json!({
                "timestamp": "2026-09-14T14:00:00.000Z", "ordinal": 42,
                "type": "event_msg", "payload": {
                    "type": "item_completed", "thread_id": "one", "turn_id": "turn-1",
                    "item": { "type": "AgentMessage", "id": "message-1", "phase": phase,
                        "content": [{"type": "Text", "text": "First paragraph."},
                            {"type": "Image", "text": "not display text"},
                            {"type": "Text", "text": "Second paragraph."}] },
                    "started_at_ms": 1, "completed_at_ms": 2
                }
            })).unwrap();
            assert_eq!(parse_update(&line), Some(Update::Message(AssistantMessage {
                text: "First paragraph.\nSecond paragraph.".into(),
                final_message: phase == Some("final_answer"),
            })));
        }
    }

    #[test]
    fn completed_items_exclude_reasoning_prompts_tools_and_unknown_phases() {
        for item in [
            serde_json::json!({"type":"UserMessage","content":[{"type":"Text","text":"private prompt"}]}),
            serde_json::json!({"type":"Reasoning","summary_text":["private reasoning"],"raw_content":["secret"]}),
            serde_json::json!({"type":"CommandExecution","stdout":"private output"}),
            serde_json::json!({"type":"FileChange","changes":{"private":"diff"}}),
            serde_json::json!({"type":"AgentMessage","phase":"analysis","content":[{"type":"Text","text":"private reasoning"}]}),
            serde_json::json!({"type":"AgentMessage","phase":"unknown","content":[{"type":"Text","text":"unrecognized"}]}),
            serde_json::json!({"type":"AgentMessage","content":[{"type":"Text","text":"  \n\t"}]}),
            serde_json::json!({"type":"AgentMessage","content":[{"type":"Image","text":"not display text"}]}),
        ] {
            let line = serde_json::to_vec(&serde_json::json!({"type":"event_msg",
                "payload":{"type":"item_completed","item":item}})).unwrap();
            assert_eq!(parse_update(&line), None);
        }
        // A similarly shaped event outside the display-event envelope is not trusted.
        let line = br#"{"type":"response_item","payload":{"type":"item_completed","item":{"type":"AgentMessage","content":[{"type":"Text","text":"mirror"}]}}}"#;
        assert_eq!(parse_update(line), None);
    }

    #[test]
    fn completed_items_replace_working_placeholders_and_keep_results_without_mirrors() {
        let now = Instant::now();
        let path = std::env::temp_dir().join(format!("overlay-item-completed-{}.jsonl", std::process::id()));
        std::fs::write(&path, []).unwrap();
        let mut session = tracked("one", 1, now);
        session.cursor = Some(MessageCursor::new(path.clone()).unwrap());
        let active = vec![session.session.clone()];
        let mut feed = Feed::default();
        feed.sessions.insert("one".into(), session);
        let settings = GuardSettings::default();
        feed.test_poll(active.clone(), &settings, now, false);
        assert!(feed.test_frame(&settings, |_| true).busy);
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        for (phase, text) in [("commentary", "Checking the project."), ("final_answer", "The changes are ready.")] {
            let line = serde_json::to_string(&serde_json::json!({"type":"event_msg",
                "payload":{"type":"item_completed","item":{"type":"AgentMessage",
                    "phase":phase,"content":[{"type":"Text","text":text}]}}})).unwrap();
            // New events can also be split across file writes.
            let split = line.len() / 2;
            file.write_all(&line.as_bytes()[..split]).unwrap();
            let previous_id = feed.next_id;
            feed.test_poll(active.clone(), &settings, now, false);
            assert_eq!(feed.next_id, previous_id);
            write_event(&mut file, &line[split..]).unwrap();
            feed.test_poll(active.clone(), &settings, now, false);
            let frame = feed.test_frame(&settings, |_| true);
            assert_eq!(frame.cards[0].text, text);
            assert!(frame.busy, "a final display message alone does not end a turn");
            let message_id = frame.cards[0].id;
            let mirror = serde_json::to_string(&serde_json::json!({"type":"response_item",
                "payload":{"type":"message","role":"assistant","phase":phase,
                    "content":[{"type":"output_text","text":text}]}})).unwrap();
            write_event(&mut file, &mirror).unwrap();
            feed.test_poll(active.clone(), &settings, now, false);
            assert_eq!(feed.test_frame(&settings, |_| true).cards[0].id, message_id);
        }
        let final_id = feed.test_frame(&settings, |_| true).cards[0].id;
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"The changes are ready."}}"#).unwrap();
        feed.test_poll(active, &settings, now, false);
        let frame = feed.test_frame(&settings, |_| true);
        assert_eq!(frame.cards.len(), 1);
        assert_eq!(frame.cards[0].id, final_id);
        assert_eq!(frame.cards[0].text, "The changes are ready.");
        assert!(frame.attention);
        assert!(!frame.busy);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn terminal_text_recovers_a_final_message_skipped_by_a_large_output_burst() {
        let now = Instant::now();
        let path = std::env::temp_dir().join(format!("overlay-terminal-text-{}.jsonl", std::process::id()));
        std::fs::write(&path, []).unwrap();
        let mut session = tracked("one", 1, now);
        session.cursor = Some(MessageCursor::new(path.clone()).unwrap());
        let active = vec![session.session.clone()];
        let mut feed = Feed::default();
        feed.sessions.insert("one".into(), session);
        let settings = GuardSettings::default();
        feed.test_poll(active, &settings, now, false);
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","phase":"final_answer","content":[{"type":"Text","text":"Recovered final result."}]}}}"#).unwrap();
        write_event(&mut file, &"x".repeat(READ_LIMIT as usize * 2)).unwrap();
        write_event(&mut file, r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"Recovered final result."}}"#).unwrap();
        // The lifecycle tracker can stop tracking before the overlay drains the file.
        feed.test_poll(vec![], &settings, now, false);
        let frame = feed.test_frame(&settings, |_| true);
        assert_eq!(frame.cards[0].text, "Recovered final result.");
        assert!(frame.cards[0].final_message);
        assert!(frame.attention);
        assert!(!frame.busy);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn terminal_text_is_optional_sanitized_and_bounded() {
        for payload in [
            serde_json::json!({"type":"task_complete"}),
            serde_json::json!({"type":"task_complete","last_agent_message":null}),
            serde_json::json!({"type":"task_complete","last_agent_message":" \n\t "}),
        ] {
            let line = serde_json::to_vec(&serde_json::json!({"type":"event_msg","payload":payload})).unwrap();
            assert_eq!(parse_update(&line), Some(Update::Completed(None)));
        }
        let line = serde_json::to_vec(&serde_json::json!({"type":"event_msg",
            "payload":{"type":"task_complete","last_agent_message":format!("\u{0}{}", "é".repeat(1500))}})).unwrap();
        let Some(Update::Completed(Some(message))) = parse_update(&line) else { panic!("missing final text"); };
        assert_eq!(message.text, format!("{}…", "é".repeat(1400)));
        assert!(message.final_message);
    }

    #[test]
    fn accepts_only_display_messages_and_ignores_mirrors_and_reasoning() {
        assert_eq!(parse_message(&event("Hello")).unwrap().text, "Hello");
        for line in [
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hello"}]}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"private prompt"}}"#,
            r#"{"type":"event_msg","payload":{"type":"agent_reasoning","text":"private reasoning"}}"#,
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"private reasoning","phase":"analysis"}}"#,
            "invalid json",
        ] {
            assert!(parse_message(line.as_bytes()).is_none());
        }
        assert!(parse_message(br#"{"type":"event_msg","payload":{"type":"agent_message","message":"Done","phase":"final_answer"}}"#).unwrap().final_message);
    }

    #[test]
    fn tucked_messages_survive_expiry_and_resume_their_reading_time_when_opened() {
        let now = Instant::now();
        let mut feed = Feed::default();
        feed.sessions.insert(
            "one".into(),
            TrackedSession {
                project: None,
                session: Session {
                    activity: 0,
                    id: "one".into(),
                    cwd: None,
                    window: None,
                },
                label: "chat".into(),
                fallback_title: None,
                cursor: None,
                last_active: now,
                next_lookup: now + Duration::from_secs(9999),
                source_active: false,
                busy: false,
                completion: None,
            },
        );
        feed.previews.push_back(Preview {
            session: "one".into(),
            received: now,
            card: Card {
                id: 1,
                label: "chat".into(),
                text: "Keep this update".into(),
                final_message: true,
                attention: false,
                target: None,
            },
        });
        let settings = GuardSettings::default();
        let later = now + Duration::from_secs(700);
        feed.test_poll(vec![], &settings, later, true);
        assert_eq!(feed.previews.len(), 1);
        assert!(feed.sessions.contains_key("one"));
        let reopened = later + Duration::from_secs(700);
        feed.test_poll(vec![], &settings, reopened, false);
        assert_eq!(feed.previews[0].card.text, "Keep this update");
        feed.test_poll(vec![], &settings, reopened + Duration::from_secs(89), false);
        assert_eq!(feed.previews.len(), 1);
        feed.test_poll(vec![], &settings, reopened + Duration::from_secs(91), false);
        assert!(feed.previews.is_empty());
    }

    #[test]
    fn tails_new_messages_handles_split_unicode_and_truncation() {
        let path = std::env::temp_dir().join(format!(
            "overlay-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, event("old message")).unwrap();
        let mut cursor = MessageCursor::new(path.clone()).unwrap();
        assert!(cursor.read_messages().unwrap().is_empty());
        let bytes = event("café — ready");
        let split = bytes.iter().position(|b| *b == 0xc3).unwrap() + 1;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&bytes[..split]).unwrap();
        assert!(cursor.read_messages().unwrap().is_empty());
        file.write_all(&bytes[split..]).unwrap();
        assert_eq!(cursor.read_messages().unwrap()[0].text, "café — ready");
        assert!(cursor.read_messages().unwrap().is_empty());
        drop(file);
        std::fs::write(&path, event("new")).unwrap();
        assert_eq!(cursor.read_messages().unwrap()[0].text, "new");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn huge_tool_lines_do_not_strand_following_messages() {
        let path = std::env::temp_dir().join(format!("overlay-big-{}.jsonl", std::process::id()));
        std::fs::write(&path, []).unwrap();
        let mut cursor = MessageCursor::new(path.clone()).unwrap();
        let mut bytes = vec![b'x'; LINE_LIMIT + 10];
        bytes.push(b'\n');
        bytes.extend(event("ready"));
        std::fs::write(&path, bytes).unwrap();
        let mut messages = vec![];
        for _ in 0..6 {
            messages.extend(cursor.read_messages().unwrap());
        }
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "ready");
        std::fs::remove_file(path).unwrap();
    }
}
