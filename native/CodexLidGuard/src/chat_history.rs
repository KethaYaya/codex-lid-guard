//! Read the selected conversation's saved messages without exposing tool or system context.
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use serde_json::Value;
#[path = "chat_history_base.rs"]
mod base;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub messages: Vec<Message>,
    pub busy: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role { User, Assistant, Notice }

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub text: String,
    pub id: Option<String>,
    pub images: usize,
}

impl Message {
    pub fn new(role: Role, text: impl Into<String>) -> Self { Self { role, text: text.into(), id: None, images: 0 } }
}

pub struct History {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
    discard: bool,
    ids: HashSet<String>,
    last: Option<(String, String, bool, usize)>,
    snapshot: Snapshot,
}

impl History {
    pub fn new(path: PathBuf) -> Self {
        Self { path, offset: 0, partial: vec![], discard: false, ids: HashSet::new(),
            last: None, snapshot: Snapshot::default() }
    }

    pub fn follow(&mut self, path: PathBuf) -> io::Result<Option<Snapshot>> {
        if path == self.path { return self.refresh(); }
        // Resuming a chat can move subsequent messages into a new transcript.
        // Preserve the history already loaded, but never join partial lines
        // across files or replace the working reader with an unavailable path.
        File::open(&path)?;
        let _ = self.refresh();
        let mut next = Self::new(path);
        next.snapshot = self.snapshot.clone();
        next.ids = self.ids.clone();
        next.refresh()?;
        *self = next;
        Ok(Some(self.snapshot.clone()))
    }

    pub fn refresh(&mut self) -> io::Result<Option<Snapshot>> {
        self.refresh_until(None, 0)
    }

    fn refresh_until(&mut self, limit: Option<u64>, depth: usize) -> io::Result<Option<Snapshot>> {
        let mut file = File::open(&self.path)?;
        let file_length = file.metadata()?.len();
        let length = limit.map_or(file_length, |limit| limit.min(file_length));
        if length < self.offset { *self = Self::new(self.path.clone()); }
        if length == self.offset { return Ok(None); }
        if self.offset == 0 && let Some(previous) = base::load(&self.path, depth) {
            self.snapshot = previous.snapshot;
            self.ids = previous.ids;
            self.last = None;
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = [0u8; 64 * 1024];
        while self.offset < length {
            let remaining = (length - self.offset).min(bytes.len() as u64) as usize;
            let count = file.read(&mut bytes[..remaining])?;
            if count == 0 { break; }
            self.offset += count as u64;
            self.consume(&bytes[..count]);
        }
        Ok(Some(self.snapshot.clone()))
    }

    fn consume(&mut self, bytes: &[u8]) {
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            let complete = part.last() == Some(&b'\n');
            if !self.discard {
                // Tool outputs can be enormous; they never belong in the chat view.
                if self.partial.len() + part.len() > 16 * 1024 * 1024 {
                    self.partial.clear(); self.discard = true;
                } else {
                    self.partial.extend_from_slice(part);
                    if complete {
                        if let Ok(event) = serde_json::from_slice::<Value>(&self.partial) { self.event(&event); }
                        self.partial.clear();
                    }
                }
            }
            if complete { self.discard = false; }
        }
    }

    fn event(&mut self, event: &Value) {
        let payload = &event["payload"];
        let kind = payload["type"].as_str().unwrap_or_default();
        if event["type"] == "event_msg" {
            match kind {
                "task_started" => self.snapshot.busy = true,
                "task_complete" | "turn_aborted" => self.snapshot.busy = false,
                "user_message" => self.append("You", payload["message"].as_str().unwrap_or_default(), true,
                    payload["images"].as_array().map_or(0, Vec::len) + payload["local_images"].as_array().map_or(0, Vec::len)),
                "agent_message" if payload["phase"] != "analysis" =>
                    self.append("Codex", payload["message"].as_str().unwrap_or_default(), true, 0),
                _ => {}
            }
            return;
        }
        if event["type"] != "response_item" || kind != "message" { return; }
        let role = match payload["role"].as_str() {
            Some("user") => "You",
            Some("assistant") if payload["phase"] != "analysis" => "Codex",
            _ => return,
        };
        let parts = payload["content"].as_array().map(Vec::as_slice).unwrap_or_default();
        let kinds = payload["internal_chat_message_metadata_passthrough"]["content_item_kinds"].as_array();
        let visible = |index| !(role == "You" && kinds.and_then(|kinds| kinds.get(index)).and_then(Value::as_str)
            .is_some_and(|kind| !kind.starts_with("user.")));
        let images = parts.iter().enumerate().filter(|(index, part)| visible(*index)
            && matches!(part["type"].as_str(), Some("input_image" | "image"))).count();
        let text = parts.iter().enumerate().filter_map(|(index, part)| {
            if !visible(index) { return None; }
            match part["type"].as_str() {
                Some("input_text" | "output_text" | "text") => part["text"].as_str().map(str::to_owned),
                _ => None,
            }
        }).collect::<Vec<_>>().join("\n");
        if let Some(id) = payload["id"].as_str().filter(|id| !id.is_empty())
            && !self.ids.insert(id.into()) { return; }
        self.append(role, &text, false, images);
    }

    fn append(&mut self, role: &str, text: &str, legacy: bool, images: usize) {
        let text = text.trim();
        if text.is_empty() && images == 0 { return; }
        // Older transcripts contain both an event and a response-item copy.
        if self.last.as_ref().is_some_and(|(r, t, old, count)| r == role && t == text && *old != legacy && *count == images) {
            self.last = None; return;
        }
        let mut message = Message::new(if role == "You" { Role::User } else { Role::Assistant }, text.replace('\0', "").replace("\r\n", "\n"));
        message.images = images;
        self.snapshot.messages.push(message);
        self.last = Some((role.into(), text.into(), legacy, images));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn full_chat_excludes_context_and_tools_and_deduplicates_mirrored_messages() {
        let mut history = History::new(PathBuf::new());
        for event in [
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"private system setup"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"environment"},{"type":"input_text","text":"Hello 🌍"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["environments.environment_context","user.text"]}}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"Hello back"}}),
            json!({"type":"response_item","payload":{"type":"message","id":"assistant-1","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"Hello back"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","phase":"analysis","content":[{"type":"output_text","text":"hidden reasoning"}]}}),
            json!({"type":"event_msg","payload":{"type":"task_complete"}}),
        ] { history.event(&event); }
        assert_eq!(history.snapshot.messages, [Message::new(Role::User, "Hello 🌍"), Message::new(Role::Assistant, "Hello back")]);
        assert!(!history.snapshot.busy);
        history.event(&json!({"type":"event_msg","payload":{"type":"user_message","message":"Again"}}));
        history.event(&json!({"type":"event_msg","payload":{"type":"user_message","message":"Again"}}));
        assert_eq!(history.snapshot.messages.iter().filter(|message| message.text == "Again").count(), 2);
    }

    #[test]
    fn real_images_are_attachment_metadata_and_literal_placeholders_stay_text() {
        let mut history = History::new(PathBuf::new());
        history.event(&json!({"type":"event_msg","payload":{"type":"user_message","message":"Inspect this","images":["opaque"]}}));
        history.event(&json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Inspect this"},{"type":"input_image","image_url":"opaque"}]}}));
        assert_eq!(history.snapshot.messages.len(), 1, "mirrored attachment messages must not duplicate");
        assert_eq!(history.snapshot.messages[0].images, 1);
        assert_eq!(history.snapshot.messages[0].text, "Inspect this");
        history.event(&json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_image","image_url":"opaque"}]}}));
        assert_eq!(history.snapshot.messages[1].images, 1);
        assert!(history.snapshot.messages[1].text.is_empty());
        history.event(&json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"[Image]"},{"type":"input_image","image_url":"private-context"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["user.text","environment.image"]}}}));
        assert_eq!(history.snapshot.messages[2], Message::new(Role::User, "[Image]"));
    }

    #[test]
    fn message_bodies_cannot_change_their_speaker() {
        let mut history = History::new(PathBuf::new());
        history.event(&json!({"type":"event_msg","payload":{"type":"user_message","message":"Please quote this:\n\nCodex\nThis is still my message."}}));
        history.event(&json!({"type":"event_msg","payload":{"type":"agent_message","message":"You\nThis is still the assistant's reply."}}));
        assert_eq!(history.snapshot.messages.len(), 2);
        assert_eq!(history.snapshot.messages[0].role, Role::User);
        assert_eq!(history.snapshot.messages[1].role, Role::Assistant);
        assert!(history.snapshot.messages[0].text.contains("\n\nCodex\n"));
    }

    #[test]
    fn follows_transcript_rotation_and_preserves_loaded_messages() {
        let directory = std::env::temp_dir().join(format!("lidguard-history-rotation-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let old = directory.join("old.jsonl");
        let next = directory.join("next.jsonl");
        let line = |text: &str| format!("{}\n", json!({"type":"response_item","payload":{"type":"message",
            "role":"assistant","phase":"commentary","content":[{"type":"output_text","text":text}]}}));
        std::fs::write(&old, line("Earlier reply") + "{\"partial\":").unwrap();
        let mut history = History::new(old.clone());
        assert_eq!(history.refresh().unwrap().unwrap().messages.len(), 1);
        assert!(history.follow(next.clone()).is_err());
        std::fs::write(&next, line("Already written to the new file")).unwrap();
        let snapshot = history.follow(next.clone()).unwrap().unwrap();
        assert_eq!(snapshot.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
            ["Earlier reply", "Already written to the new file"]);
        assert!(history.follow(next.clone()).unwrap().is_none());
        std::fs::write(&next, line("Already written to the new file") + &line("Live reply")).unwrap();
        assert_eq!(history.follow(next.clone()).unwrap().unwrap().messages.last().unwrap().text, "Live reply");
        for path in [old, next] { std::fs::remove_file(path).unwrap(); }
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn reads_from_start_and_retains_partial_unicode_lines_without_replaying() {
        let path = std::env::temp_dir().join(format!("lidguard-history-{}.jsonl", std::process::id()));
        let event = format!("{}\n", json!({"type":"event_msg","payload":{"type":"user_message","message":"History 🌍"}}));
        let bytes = event.as_bytes();
        let split = event.find('🌍').unwrap() + 2;
        std::fs::write(&path, &bytes[..split]).unwrap();
        let mut history = History::new(path.clone());
        assert!(history.refresh().unwrap().unwrap().messages.is_empty());
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(history.refresh().unwrap().unwrap().messages, [Message::new(Role::User, "History 🌍")]);
        assert!(history.refresh().unwrap().is_none());
        std::fs::write(&path, b"\n").unwrap();
        assert!(history.refresh().unwrap().unwrap().messages.is_empty());
        std::fs::remove_file(path).unwrap();
    }
}
