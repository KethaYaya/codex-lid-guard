//! Display titles only. This index contains session metadata, not chat messages.
use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use serde::Deserialize;

#[derive(Default)]
pub struct SessionTitleIndex {
    stamp: Option<(u64, SystemTime)>,
    titles: HashMap<String, String>,
}

impl SessionTitleIndex {
    pub fn refresh(&mut self, path: &Path) {
        let Ok(metadata) = path.metadata() else {
            return;
        };
        let Ok(modified) = metadata.modified() else {
            return;
        };
        let stamp = (metadata.len(), modified);
        if self.stamp == Some(stamp) {
            return;
        }
        let Ok(contents) = std::fs::read_to_string(path) else {
            return;
        };
        self.titles = parse_titles(&contents);
        self.stamp = Some(stamp);
    }

    pub fn title(&self, session_id: &str) -> Option<&str> {
        self.titles
            .get(&session_id.trim().to_ascii_lowercase())
            .map(String::as_str)
    }
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    thread_name: String,
}

fn parse_titles(contents: &str) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    for line in contents.lines() {
        let Ok(entry) = serde_json::from_str::<Entry>(line) else {
            continue;
        };
        let id = entry.id.trim().to_ascii_lowercase();
        let title = entry
            .thread_name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !id.is_empty() && !title.is_empty() {
            // Later entries record title edits, matching the extension's menu.
            titles.insert(id, title);
        }
    }
    titles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_nonempty_title_wins_without_using_other_message_fields() {
        let titles = parse_titles(concat!(
            "{\"id\":\"SESSION\",\"thread_name\":\"Old title\"}\n",
            "broken json\n",
            "{\"id\":\"session\",\"thread_name\":\" New\\n title \"}\n",
            "{\"id\":\"session\",\"thread_name\":\" \"}\n",
            "{\"id\":\"private\",\"message\":\"not a title\"}\n",
            "{\"id\":\"session\",\"thread_name\":null}\n"
        ));
        assert_eq!(titles.get("session").map(String::as_str), Some("New title"));
        assert_eq!(titles.len(), 1);
    }

    #[test]
    fn refresh_picks_up_a_title_created_or_renamed_after_tracking_starts() {
        let path = std::env::temp_dir().join(format!(
            "codex-title-index-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut index = SessionTitleIndex::default();
        index.refresh(&path);
        assert!(index.title("session").is_none());
        std::fs::write(
            &path,
            "{\"id\":\"session\",\"thread_name\":\"First title\"}\n",
        )
        .unwrap();
        index.refresh(&path);
        assert_eq!(index.title("SESSION"), Some("First title"));
        std::fs::write(
            &path,
            "{\"id\":\"session\",\"thread_name\":\"Renamed project task\"}\n",
        )
        .unwrap();
        index.refresh(&path);
        assert_eq!(index.title("session"), Some("Renamed project task"));
        std::fs::remove_file(path).unwrap();
    }
}
