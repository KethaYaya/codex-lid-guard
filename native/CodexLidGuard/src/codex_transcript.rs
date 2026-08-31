use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde::Deserialize;

const MAX_APPENDED_BYTES: u64 = 256 * 1024;

pub struct TranscriptCursor {
    path: PathBuf,
    turn_id: String,
    offset: u64,
    pending_line: String,
}

impl TranscriptCursor {
    pub fn new(path: Option<&str>, turn_id: Option<&str>) -> Option<Self> {
        let path = path?.trim();
        let turn_id = turn_id?.trim();
        if path.is_empty() || turn_id.is_empty() {
            return None;
        }
        let path = PathBuf::from(path);
        let offset = fs::metadata(&path).map(|value| value.len()).unwrap_or(0);
        Some(Self {
            path,
            turn_id: turn_id.to_string(),
            offset,
            pending_line: String::new(),
        })
    }

    pub fn reached_terminal_event(&mut self) -> bool {
        self.read_appended()
            .is_ok_and(|value| has_terminal_event(&value, &self.turn_id))
    }

    fn read_appended(&mut self) -> std::io::Result<String> {
        let length = fs::metadata(&self.path)?.len();
        if length < self.offset {
            self.offset = length;
            self.pending_line.clear();
            return Ok(String::new());
        }
        if length == self.offset {
            return Ok(String::new());
        }
        let mut file = File::open(&self.path)?;
        let previous_offset = self.offset;
        let start = previous_offset.max(length.saturating_sub(MAX_APPENDED_BYTES));
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::with_capacity((length - start) as usize);
        file.read_to_end(&mut bytes)?;
        self.offset = length;
        let mut appended = String::from_utf8_lossy(&bytes).into_owned();
        if start > previous_offset {
            self.pending_line.clear();
            if let Some(first_newline) = appended.find('\n') {
                appended.drain(..=first_newline);
            } else {
                return Ok(String::new());
            }
        }
        self.pending_line.push_str(&appended);
        if self.pending_line.len() > MAX_APPENDED_BYTES as usize
            && !self.pending_line.contains('\n')
        {
            self.pending_line.clear();
            return Ok(String::new());
        }
        let Some(last_newline) = self.pending_line.rfind('\n') else {
            return Ok(String::new());
        };
        let remainder = self.pending_line.split_off(last_newline + 1);
        let complete = std::mem::replace(&mut self.pending_line, remainder);
        Ok(complete)
    }
}

#[derive(Deserialize)]
struct TranscriptEvent {
    #[serde(rename = "type")]
    kind: String,
    payload: Option<TranscriptEventPayload>,
}

#[derive(Deserialize)]
struct TranscriptEventPayload {
    #[serde(rename = "type")]
    kind: String,
    turn_id: Option<String>,
}

fn has_terminal_event(value: &str, turn_id: &str) -> bool {
    value.lines().any(|line| {
        let Ok(event) = serde_json::from_str::<TranscriptEvent>(line) else {
            return false;
        };
        let Some(payload) = event.payload else {
            return false;
        };
        event.kind == "event_msg"
            && payload.turn_id.as_deref() == Some(turn_id)
            && matches!(payload.kind.as_str(), "task_complete" | "turn_aborted")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn exact_completed_turn_is_terminal() {
        let value = r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-a"}}"#;
        assert!(has_terminal_event(value, "turn-a"));
        assert!(!has_terminal_event(value, "turn-b"));
    }

    #[test]
    fn exact_aborted_turn_is_terminal() {
        let value = r#"{"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-a","reason":"interrupted"}}"#;
        assert!(has_terminal_event(value, "turn-a"));
    }

    #[test]
    fn ordinary_transcript_items_are_not_terminal() {
        let value = r#"{"type":"response_item","payload":{"type":"message","internal_chat_message_metadata_passthrough":{"turn_id":"turn-a"}}}"#;
        assert!(!has_terminal_event(value, "turn-a"));
    }

    #[test]
    fn cursor_preserves_a_lifecycle_record_split_between_reads() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codex-lid-guard-transcript-{}-{unique}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-a\"}}\n",
        )
        .unwrap();
        let mut cursor = TranscriptCursor::new(path.to_str(), Some("turn-a")).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"turn_")
            .unwrap();
        file.flush().unwrap();
        assert!(!cursor.reached_terminal_event());
        file.write_all(b"aborted\",\"turn_id\":\"turn-a\"}}\n")
            .unwrap();
        file.flush().unwrap();
        assert!(cursor.reached_terminal_event());
        drop(file);
        fs::remove_file(path).unwrap();
    }
}
