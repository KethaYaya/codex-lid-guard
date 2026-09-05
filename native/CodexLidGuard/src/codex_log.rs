use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const LOG_TAIL_BYTES: u64 = 256 * 1024;
const RECENT_RUNS_PER_PRODUCT: usize = 6;
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
    sessions: HashSet<String>,
    state: Option<ViewState>,
    event: Option<String>,
    revision: u64,
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
            let previous = self.logs.get(&path);
            if previous.is_none_or(|cached| cached.size != size || cached.modified != modified) {
                let Ok(tail) = read_tail(&path) else {
                    self.logs.remove(&path);
                    continue;
                };
                let mut observed: HashSet<String> = tail
                    .lines()
                    .filter_map(|line| field(line, "conversationId").map(str::to_owned))
                    .collect();
                let mut state = parse_latest_view_state(&tail);
                let mut event = tail
                    .lines()
                    .rev()
                    .find(|line| {
                        line.contains("thread_stream_view_activity_changed")
                            && parse_latest_view_state(line).is_some()
                    })
                    .map(str::to_owned);
                if let Some(previous) = previous.filter(|cached| size > cached.size) {
                    // A long-running chat can push its view event outside the tail.
                    // Preserve that event while the same log is being appended.
                    observed.extend(previous.sessions.intersection(sessions).cloned());
                    state = state.or_else(|| previous.state.clone());
                    event = event.or_else(|| previous.event.clone());
                }
                let revision =
                    if let Some(previous) = previous.filter(|cached| cached.event == event) {
                        previous.revision
                    } else {
                        self.next_revision += 1;
                        self.next_revision
                    };
                self.logs.insert(
                    path.clone(),
                    CachedView {
                        size,
                        modified,
                        sessions: observed,
                        state,
                        event,
                        revision,
                    },
                );
            }
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
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from)?;
    candidate_logs(&app_data).into_iter().find_map(|path| {
        let tail = read_tail(&path).ok()?;
        tail.contains(session_id)
            .then(|| parse_latest_view_state(&tail))
            .flatten()
    })
}

fn candidate_logs(app_data: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for product in PRODUCT_DATA_DIRECTORIES {
        let logs_root = app_data.join(product).join("logs");
        let Ok(entries) = fs::read_dir(logs_root) else {
            continue;
        };
        let mut runs = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        runs.sort_unstable_by(|left, right| right.file_name().cmp(&left.file_name()));
        for run in runs.into_iter().take(RECENT_RUNS_PER_PRODUCT) {
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

fn read_tail(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((length - start) as usize);
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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
        fs::write(&newer, "turn-start conversationId=one\n").unwrap();
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
