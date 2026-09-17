//! Resolve Codex continuation history only from matching local session transcripts.
use super::History;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const RECORD_LIMIT: u64 = 1024 * 1024;

fn metadata(path: &Path) -> Option<Value> {
    let mut line = Vec::new();
    BufReader::new(File::open(path).ok()?.take(RECORD_LIMIT)).read_until(b'\n', &mut line).ok()?;
    let event: Value = serde_json::from_slice(&line).ok()?;
    (event["type"] == "session_meta").then(|| event["payload"].clone())
}

fn candidates(directory: &Path, id: &str, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 4 { return; }
    let Ok(entries) = std::fs::read_dir(directory) else { return; };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue; };
        if kind.is_dir() { candidates(&entry.path(), id, depth + 1, found); }
        else if kind.is_file() && entry.file_name().to_str().is_some_and(|name|
            name.starts_with("rollout-") && name.ends_with(".jsonl") && name.contains(id)) {
            found.push(entry.path());
        }
    }
}

fn matches_boundary(path: &Path, id: &str, end: u64, ordinal: u64) -> bool {
    let Some(meta) = metadata(path) else { return false; };
    if meta["session_id"] != id && meta["id"] != id { return false; }
    let Ok(mut file) = File::open(path) else { return false; };
    if !file.metadata().is_ok_and(|meta| meta.len() >= end) { return false; }
    let start = end.saturating_sub(RECORD_LIMIT);
    if file.seek(SeekFrom::Start(start)).is_err() { return false; }
    let mut bytes = Vec::new();
    if file.take(end - start).read_to_end(&mut bytes).is_err() || bytes.last() != Some(&b'\n') { return false; }
    let Some(line) = bytes.split(|byte| *byte == b'\n').rev().find(|line| !line.is_empty()) else { return false; };
    serde_json::from_slice::<Value>(line).ok().and_then(|event| event["ordinal"].as_u64()) == ordinal.checked_sub(1)
}

pub(super) fn load(path: &Path, depth: usize) -> Option<History> {
    if depth >= 16 { return None; }
    let meta = metadata(path)?;
    let base = &meta["history_base"];
    let id = base["thread_id"].as_str()?;
    if id.len() != 36 || !id.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-') { return None; }
    let end = base["end_byte_offset"].as_u64().filter(|end| *end > 0)?;
    let ordinal = base["end_ordinal_exclusive"].as_u64().filter(|ordinal| *ordinal > 0)?;
    let directory = path.ancestors().find(|ancestor| ancestor.file_name().is_some_and(|name| name == "sessions"))?;
    let mut paths = vec![];
    candidates(directory, id, 0, &mut paths);
    paths.sort();
    for candidate in paths.into_iter().rev() {
        if candidate == path || !matches_boundary(&candidate, id, end, ordinal) { continue; }
        let mut previous = History::new(candidate);
        if previous.refresh_until(Some(end), depth + 1).is_ok() { return Some(previous); }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn continuation_loads_earlier_history_only_up_to_its_recorded_boundary() {
        let root = std::env::temp_dir().join(format!("lidguard-history-base-{}", std::process::id()));
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let id = "11111111-1111-1111-1111-111111111111";
        let old = sessions.join(format!("rollout-1-{id}.jsonl"));
        let new = sessions.join(format!("rollout-2-{id}.jsonl"));
        let decoy = sessions.join(format!("rollout-3-{id}.jsonl"));
        let message = |ordinal, text| json!({"ordinal":ordinal,"type":"response_item","payload":{
            "type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":text}]}});
        let base = format!("{}\n{}\n", json!({"type":"session_meta","payload":{"id":id}}), message(1, "Earlier reply"));
        std::fs::write(&old, format!("{base}{}\n", message(2, "Outside the continuation boundary"))).unwrap();
        std::fs::write(&decoy, base.replace(id, "22222222-2222-2222-2222-222222222222")).unwrap();
        std::fs::write(&new, format!("{}\n{}\n", json!({"type":"session_meta","payload":{"id":id,
            "history_base":{"thread_id":id,"end_byte_offset":base.len(),"end_ordinal_exclusive":2}}}), message(3, "Current reply"))).unwrap();
        let mut history = History::new(new.clone());
        let snapshot = history.refresh().unwrap().unwrap();
        assert_eq!(snapshot.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["Earlier reply", "Current reply"]);
        assert!(history.refresh().unwrap().is_none());
        let mut already_open = History::new(old.clone());
        assert_eq!(already_open.refresh().unwrap().unwrap().messages.len(), 2);
        assert_eq!(already_open.follow(new.clone()).unwrap().unwrap(), snapshot);
        assert!(!matches_boundary(&old, id, base.len() as u64, 99));
        for path in [old, new, decoy] { std::fs::remove_file(path).unwrap(); }
        std::fs::remove_dir(sessions).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
