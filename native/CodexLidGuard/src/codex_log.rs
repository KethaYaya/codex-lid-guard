use std::cmp::Reverse;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const LOG_TAIL_BYTES: u64 = 256 * 1024;
const RECENT_RUNS_PER_PRODUCT: usize = 6;
const PRODUCT_DATA_DIRECTORIES: &[&str] =
    &["Code", "Code - Insiders", "VSCodium", "Cursor", "Windsurf"];

#[derive(Debug, Eq, PartialEq)]
pub enum ViewState {
    Active(String),
    Inactive,
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
