use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[cfg(test)]
const LOG_TAIL_BYTES: u64 = 256 * 1024;
const MAX_VIEW_LOG_LINE_BYTES: usize = 64 * 1024;
const PRODUCT_DATA_DIRECTORIES: &[&str] =
    &["Code", "Code - Insiders", "VSCodium", "Cursor", "Windsurf"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewState {
    Active(String),
    Inactive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewObservation {
    pub state: ViewState,
    pub revision: u64,
}

#[derive(Default)]
pub struct ViewStateReader {
    logs: HashMap<PathBuf, CachedView>,
    next_revision: u64,
}

struct CachedView {
    size: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    sessions: HashSet<String>,
    state: Option<ViewState>,
    event: Option<String>,
    revision: u64,
    pending: Vec<u8>,
    discarding_line: bool,
}

impl ViewStateReader {
    pub fn for_sessions(&mut self, sessions: &HashSet<String>) -> HashMap<String, ViewObservation> {
        // No log discovery or reads while another application has focus.
        if sessions.is_empty() {
            return HashMap::new();
        }
        let Some(app_data) = std::env::var_os("APPDATA") else {
            return HashMap::new();
        };
        self.read_candidates(candidate_logs(Path::new(&app_data)), sessions)
    }

    fn read_candidates(
        &mut self,
        paths: Vec<PathBuf>,
        sessions: &HashSet<String>,
    ) -> HashMap<String, ViewObservation> {
        self.logs.retain(|path, _| paths.contains(path));
        let mut resolved = HashSet::new();
        let mut views = HashMap::new();
        for path in paths {
            let Ok(metadata) = path.metadata() else {
                self.logs.remove(&path);
                continue;
            };
            let size = metadata.len();
            let modified = metadata.modified().ok();
            let created = metadata.created().ok();
            let cached = self
                .logs
                .entry(path.clone())
                .or_insert_with(CachedView::new);
            if size < cached.size
                || cached.created != created
                || (size == cached.size && cached.modified != modified)
            {
                *cached = CachedView::new();
            }
            if cached.size != size {
                // Recover the latest view even when it predates the tail (for example
                // after a helper upgrade). Subsequent checks read only appended bytes.
                if cached
                    .read_appended(&path, size, &mut self.next_revision)
                    .is_err()
                {
                    self.logs.remove(&path);
                    continue;
                }
            }
            cached.modified = modified;
            cached.created = created;
            let cached = &self.logs[&path];
            for session in cached.sessions.intersection(sessions) {
                if resolved.insert(session.clone())
                    && let Some(state) = &cached.state
                {
                    views.insert(
                        session.clone(),
                        ViewObservation {
                            state: state.clone(),
                            revision: cached.revision,
                        },
                    );
                }
            }
            if resolved.len() == sessions.len() {
                break;
            }
        }
        views
    }
}

pub fn view_state_for_session(session_id: &str) -> Option<ViewState> {
    if session_id.trim().is_empty() {
        return None;
    }
    ViewStateReader::default()
        .for_sessions(&HashSet::from([session_id.to_owned()]))
        .remove(session_id)
        .map(|view| view.state)
}

impl CachedView {
    fn new() -> Self {
        Self {
            size: 0,
            modified: None,
            created: None,
            sessions: HashSet::new(),
            state: None,
            event: None,
            revision: 0,
            pending: Vec::new(),
            discarding_line: false,
        }
    }

    fn read_appended(&mut self, path: &Path, size: u64, revision: &mut u64) -> std::io::Result<()> {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(self.size))?;
        let mut buffer = [0u8; 16 * 1024];
        while self.size < size {
            let limit = (size - self.size).min(buffer.len() as u64) as usize;
            let count = file.read(&mut buffer[..limit])?;
            if count == 0 {
                break;
            }
            self.consume(&buffer[..count], revision);
            self.size += count as u64;
        }
        Ok(())
    }

    fn consume(&mut self, bytes: &[u8], revision: &mut u64) {
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            let complete = part.last() == Some(&b'\n');
            if !self.discarding_line {
                if self.pending.len() + part.len() > MAX_VIEW_LOG_LINE_BYTES {
                    self.pending.clear();
                    self.discarding_line = true;
                } else {
                    self.pending.reserve_exact(part.len());
                    self.pending.extend_from_slice(part);
                    if complete {
                        let line = String::from_utf8_lossy(&self.pending);
                        let view = parse_latest_view_state(&line);
                        // Global notifications mention chats belonging to other windows.
                        // Only a view event or a local turn start establishes ownership.
                        if (view.is_some()
                            || line.contains("Reasoning summary turn-start config resolved"))
                            && let Some(session) = field(&line, "conversationId")
                        {
                            self.sessions.insert(session.to_owned());
                        }
                        if let Some(view) = view {
                            if self.event.as_deref() != Some(line.as_ref()) {
                                *revision += 1;
                                self.revision = *revision;
                                self.event = Some(line.into_owned());
                            }
                            self.state = Some(view);
                        }
                        self.pending.clear();
                    }
                }
            }
            if complete {
                self.discarding_line = false;
            }
        }
    }
}

fn candidate_logs(app_data: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for product in PRODUCT_DATA_DIRECTORIES {
        let logs_root = app_data.join(product).join("logs");
        let Ok(entries) = fs::read_dir(logs_root) else {
            continue;
        };
        let runs = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        // CLI invocations create newer run folders without editor windows. A
        // still-open editor can belong to a much older run, so rank actual logs
        // by their modification time instead of discarding older run folders.
        for run in runs {
            let Ok(windows) = fs::read_dir(run) else {
                continue;
            };
            for window in windows.flatten().filter(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && entry.file_name().to_string_lossy().starts_with("window")
            }) {
                let path = window
                    .path()
                    .join("exthost")
                    .join("openai.chatgpt")
                    .join("Codex.log");
                if let Ok(metadata) = path.metadata() {
                    candidates.push((metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH), path));
                }
            }
        }
    }
    candidates.sort_unstable_by_key(|candidate| Reverse(candidate.0));
    candidates.into_iter().map(|(_, path)| path).collect()
}

fn parse_latest_view_state(value: &str) -> Option<ViewState> {
    value.lines().rev().find_map(|line| {
        line.contains("thread_stream_view_activity_changed")
            .then(|| match field(line, "active") {
                Some("true") => field(line, "conversationId")
                    .map(|session| ViewState::Active(session.to_string())),
                Some("false") => Some(ViewState::Inactive),
                _ => None,
            })
            .flatten()
    })
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}=");
    let value = line
        .split_whitespace()
        .find_map(|part| part.strip_prefix(&marker))?;
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn running_editor_log_is_found_behind_newer_empty_cli_runs() {
        let directory =
            std::env::temp_dir().join(format!("codex-view-discovery-{}", std::process::id()));
        let root = directory.join("Code/logs");
        let owner = root.join("20260905T154651/window2/exthost/openai.chatgpt");
        fs::create_dir_all(&owner).unwrap();
        let log = owner.join("Codex.log");
        fs::write(
            &log,
            "thread_stream_view_activity_changed active=true conversationId=one\n",
        )
        .unwrap();
        let newer: Vec<_> = (0..10)
            .map(|index| root.join(format!("20260905T22{index:04}")))
            .collect();
        for path in &newer {
            fs::create_dir_all(path).unwrap();
        }
        let candidates = candidate_logs(&directory);
        assert_eq!(candidates, vec![log.clone()]);
        let views =
            ViewStateReader::default().read_candidates(candidates, &HashSet::from(["one".into()]));
        assert_eq!(views["one"].state, ViewState::Active("one".into()));
        fs::remove_file(log).unwrap();
        for path in newer {
            fs::remove_dir(path).unwrap();
        }
        let mut path = owner;
        loop {
            fs::remove_dir(&path).unwrap();
            if path == directory {
                break;
            }
            path = path.parent().unwrap().to_owned();
        }
    }

    #[test]
    fn cold_reader_recovers_an_open_chat_before_a_long_log_tail() {
        let path = std::env::temp_dir().join(format!("codex-view-cold-{}.log", std::process::id()));
        let mut file = File::create(&path).unwrap();
        file.write_all(
            b"time=1 thread_stream_view_activity_changed active=true conversationId=one\n",
        )
        .unwrap();
        for _ in 0..8 {
            file.write_all(&vec![b'x'; LOG_TAIL_BYTES as usize])
                .unwrap();
            file.write_all(b"\n").unwrap();
        }
        drop(file);
        let mut reader = ViewStateReader::default();
        let sessions = HashSet::from(["one".into()]);
        let first = reader.read_candidates(vec![path.clone()], &sessions)["one"].clone();
        assert_eq!(first.state, ViewState::Active("one".into()));
        assert_eq!(reader.logs[&path].size, path.metadata().unwrap().len());
        assert!(reader.logs[&path].pending.capacity() <= MAX_VIEW_LOG_LINE_BYTES);
        assert_eq!(
            reader.read_candidates(vec![path.clone()], &sessions)["one"],
            first
        );
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(
            b"time=2 thread_stream_view_activity_changed active=false conversationId=one\n",
        )
        .unwrap();
        assert_eq!(
            reader.read_candidates(vec![path.clone()], &sessions)["one"].state,
            ViewState::Inactive
        );
        drop(file);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn another_windows_broadcast_mentions_do_not_override_the_chats_own_view() {
        let own = std::env::temp_dir().join(format!("codex-view-own-{}.log", std::process::id()));
        let other =
            std::env::temp_dir().join(format!("codex-view-other-{}.log", std::process::id()));
        fs::write(
            &own,
            "thread_stream_view_activity_changed active=true conversationId=one\n",
        )
        .unwrap();
        fs::write(&other, "thread_stream_view_activity_changed active=true conversationId=two\nthread notification conversationId=one\n").unwrap();
        let mut reader = ViewStateReader::default();
        let views = reader.read_candidates(
            vec![other.clone(), own.clone()],
            &HashSet::from(["one".into(), "two".into()]),
        );
        assert_eq!(views["one"].state, ViewState::Active("one".into()));
        assert_eq!(views["two"].state, ViewState::Active("two".into()));
        fs::remove_file(own).unwrap();
        fs::remove_file(other).unwrap();
    }

    #[test]
    fn appended_partial_events_and_oversized_noise_preserve_the_last_complete_view() {
        let mut cache = CachedView::new();
        let mut revision = 0;
        cache.consume(
            b"thread_stream_view_activity_changed active=true conversationId=one\n",
            &mut revision,
        );
        cache.consume(
            b"thread_stream_view_activity_changed active=false conversationId=",
            &mut revision,
        );
        assert_eq!(cache.state, Some(ViewState::Active("one".into())));
        cache.consume(b"one\n", &mut revision);
        assert_eq!(cache.state, Some(ViewState::Inactive));
        for _ in 0..10 {
            cache.consume(&[b'x'; 16 * 1024], &mut revision);
        }
        cache.consume(
            b" thread_stream_view_activity_changed active=true conversationId=wrong\n",
            &mut revision,
        );
        assert_eq!(cache.state, Some(ViewState::Inactive));
        assert!(!cache.sessions.contains("wrong"));
        cache.consume(
            b"thread_stream_view_activity_changed active=true conversationId=two\n",
            &mut revision,
        );
        assert_eq!(cache.state, Some(ViewState::Active("two".into())));
        assert!(cache.pending.capacity() <= MAX_VIEW_LOG_LINE_BYTES);
        assert_eq!(revision, 3);
    }

    #[test]
    fn view_revisions_ignore_log_growth_but_detect_a_new_event_for_the_same_chat() {
        let path =
            std::env::temp_dir().join(format!("codex-view-revision-{}.log", std::process::id()));
        fs::write(
            &path,
            "time=1 thread_stream_view_activity_changed active=true conversationId=one\n",
        )
        .unwrap();
        let mut reader = ViewStateReader::default();
        let sessions = HashSet::from(["one".into()]);
        let first = reader.read_candidates(vec![path.clone()], &sessions)["one"].clone();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&vec![b'x'; LOG_TAIL_BYTES as usize + 10])
            .unwrap();
        file.write_all(b"\n").unwrap();
        assert_eq!(
            reader.read_candidates(vec![path.clone()], &sessions)["one"],
            first
        );
        file.write_all(
            b"time=2 thread_stream_view_activity_changed active=true conversationId=one\n",
        )
        .unwrap();
        let next = reader.read_candidates(vec![path.clone()], &sessions)["one"].clone();
        assert_eq!(next.state, first.state);
        assert_ne!(next.revision, first.revision);
        drop(file);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn cached_views_follow_chat_switches_long_appends_and_truncation() {
        let path =
            std::env::temp_dir().join(format!("codex-view-cache-{}.log", std::process::id()));
        fs::write(
            &path,
            "thread_stream_view_activity_changed active=true conversationId=one\n",
        )
        .unwrap();
        let sessions = HashSet::from(["one".into(), "two".into()]);
        let mut reader = ViewStateReader::default();
        let paths = || vec![path.clone()];
        assert_eq!(
            reader.read_candidates(paths(), &sessions)["one"].state,
            ViewState::Active("one".into())
        );
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&vec![b'x'; LOG_TAIL_BYTES as usize + 10])
            .unwrap();
        file.write_all(b"\n").unwrap();
        assert_eq!(
            reader.read_candidates(paths(), &sessions)["one"].state,
            ViewState::Active("one".into()),
            "background logging must not erase a still-current view event"
        );
        file.write_all(b"thread_stream_view_activity_changed active=true conversationId=two\n")
            .unwrap();
        let views = reader.read_candidates(paths(), &sessions);
        assert_eq!(views["one"].state, ViewState::Active("two".into()));
        assert_eq!(views["two"].state, ViewState::Active("two".into()));
        assert_eq!(reader.read_candidates(paths(), &sessions), views);
        file.write_all(b"thread_stream_view_activity_changed active=false conversationId=two\n")
            .unwrap();
        assert_eq!(
            reader.read_candidates(paths(), &sessions)["two"].state,
            ViewState::Inactive
        );
        drop(file);
        fs::write(&path, "new log without any view events\n").unwrap();
        assert!(reader.read_candidates(paths(), &sessions).is_empty());
        fs::remove_file(&path).unwrap();
        assert!(reader.read_candidates(paths(), &sessions).is_empty());
        assert!(reader.logs.is_empty());
    }

    #[test]
    fn newest_matching_log_does_not_fall_back_to_a_stale_active_view() {
        let directory = std::env::temp_dir();
        let older = directory.join(format!("codex-view-older-{}.log", std::process::id()));
        let newer = directory.join(format!("codex-view-newer-{}.log", std::process::id()));
        fs::write(
            &older,
            "thread_stream_view_activity_changed active=true conversationId=one\n",
        )
        .unwrap();
        fs::write(
            &newer,
            "Reasoning summary turn-start config resolved conversationId=one\n",
        )
        .unwrap();
        let mut reader = ViewStateReader::default();
        let sessions = HashSet::from(["one".into()]);
        assert!(
            reader
                .read_candidates(vec![newer.clone(), older.clone()], &sessions)
                .is_empty()
        );
        assert_eq!(
            reader.read_candidates(vec![older.clone()], &sessions)["one"].state,
            ViewState::Active("one".into())
        );
        assert!(!reader.logs.contains_key(&newer));
        fs::remove_file(newer).unwrap();
        fs::remove_file(older).unwrap();
    }

    #[test]
    fn latest_active_chat_wins_after_a_view_switch() {
        let log = "\
thread_stream_view_activity_changed active=true conversationId=chat-a\n\
thread_stream_view_activity_changed active=false conversationId=chat-a\n\
thread_stream_view_activity_changed active=true conversationId=chat-b\n";
        assert_eq!(
            parse_latest_view_state(log),
            Some(ViewState::Active("chat-b".into()))
        );
    }

    #[test]
    fn hiding_the_last_chat_marks_the_codex_view_inactive() {
        let log = "\
thread_stream_view_activity_changed active=true conversationId=chat-a\n\
thread_stream_view_activity_changed active=false conversationId=chat-a\n";
        assert_eq!(parse_latest_view_state(log), Some(ViewState::Inactive));
    }

    #[test]
    fn unrelated_activity_fields_are_ignored() {
        let log = "request active=true conversationId=wrong\n";
        assert_eq!(parse_latest_view_state(log), None);
    }
}
