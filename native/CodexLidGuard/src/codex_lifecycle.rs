use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::model::RecentSessionInfo;
use crate::{logging, paths};

const TURN_HANDLER_TARGET: &str = "codex_core::session::handlers";
const TURN_HANDLER_LINE: i64 = 528;
const EVENT_RETRY_DELAYS_MS: &[u64] = &[0, 1, 2, 4, 8, 16, 32, 64, 128];
const SESSION_DEDUP_WINDOW: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleStart {
    pub row_id: i64,
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
}

pub struct LifecycleWatcher {
    watcher: Option<RecommendedWatcher>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for LifecycleWatcher {
    fn drop(&mut self) {
        drop(self.watcher.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn start(on_start: impl Fn(LifecycleStart) + Send + 'static) -> Option<LifecycleWatcher> {
    start_at(
        paths::codex_logs_database(),
        paths::codex_state_database(),
        on_start,
    )
}

pub fn session_metadata(session_id: &str) -> (Option<String>, Option<String>) {
    open_read_only(&paths::codex_state_database())
        .and_then(|connection| thread_metadata(&connection, session_id))
        .unwrap_or_default()
}

pub fn recent_sessions(limit: usize) -> Vec<RecentSessionInfo> {
    if limit == 0 {
        return Vec::new();
    }
    open_read_only(&paths::codex_state_database())
        .and_then(|connection| recent_thread_metadata(&connection, limit.min(20)))
        .unwrap_or_default()
}

fn start_at(
    logs_path: PathBuf,
    state_path: PathBuf,
    on_start: impl Fn(LifecycleStart) + Send + 'static,
) -> Option<LifecycleWatcher> {
    let directory = logs_path.parent()?.to_path_buf();
    if !directory.is_dir() {
        return None;
    }

    let baseline = open_read_only(&logs_path)
        .ok()
        .and_then(|connection| maximum_log_id(&connection).ok())
        .unwrap_or(0);
    let (sender, receiver) = mpsc::sync_channel(1);
    let callback_sender = sender.clone();
    let watched_logs_path = logs_path.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| {
            event
                .paths
                .iter()
                .any(|path| is_logs_database_path(path, &watched_logs_path))
        }) {
            let _ = callback_sender.try_send(());
        }
    })
    .ok()?;
    watcher
        .watch(&directory, RecursiveMode::NonRecursive)
        .ok()?;

    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        watch_lifecycle_rows(
            logs_path,
            state_path,
            baseline,
            receiver,
            ready_sender,
            on_start,
        )
    });
    if ready_receiver.recv_timeout(Duration::from_secs(1)).is_err() {
        drop(watcher);
        drop(sender);
        let _ = worker.join();
        return None;
    }
    Some(LifecycleWatcher {
        watcher: Some(watcher),
        worker: Some(worker),
    })
}

fn watch_lifecycle_rows(
    logs_path: PathBuf,
    state_path: PathBuf,
    mut last_id: i64,
    receiver: Receiver<()>,
    ready_sender: SyncSender<()>,
    on_start: impl Fn(LifecycleStart),
) {
    let mut logs_connection = open_read_only(&logs_path).ok();
    let mut state_connection = open_read_only(&state_path).ok();
    if let Some(logs) = logs_connection.as_ref() {
        let _ = maximum_log_id(logs);
        let _ = lifecycle_rows_between(logs, last_id, last_id);
    }
    if let Some(state) = state_connection.as_ref() {
        let _ = thread_metadata(state, "");
    }
    let mut recent_sessions = HashMap::<String, Instant>::new();
    // The filesystem watcher is already attached, so this synchronous scan
    // closes the baseline-to-watch race without injecting a false hot-path event.
    let startup_starts = scan_lifecycle_rows(
        &logs_path,
        &state_path,
        &mut logs_connection,
        &mut state_connection,
        &mut last_id,
    )
    .unwrap_or_default();
    let _ = ready_sender.send(());
    let startup_seen = Instant::now();
    for start in startup_starts {
        recent_sessions.insert(start.session_id.clone(), startup_seen);
        on_start(start);
    }
    let mut last_error_log = None;
    while receiver.recv().is_ok() {
        while receiver.try_recv().is_ok() {}
        let result = scan_lifecycle_rows_after_event(
            &logs_path,
            &state_path,
            &receiver,
            &mut logs_connection,
            &mut state_connection,
            &mut last_id,
        );
        let starts = match result {
            Ok(starts) => starts,
            Err(cause) => {
                logs_connection = None;
                state_connection = None;
                let now = Instant::now();
                if last_error_log
                    .is_none_or(|previous| now.duration_since(previous) >= Duration::from_secs(30))
                {
                    logging::write(format!(
                        "Codex lifecycle metadata scan will retry after an update: {cause}"
                    ));
                    last_error_log = Some(now);
                }
                continue;
            }
        };
        last_error_log = None;
        let now = Instant::now();
        recent_sessions.retain(|_, seen| now.duration_since(*seen) < SESSION_DEDUP_WINDOW);
        for start in starts {
            if recent_sessions.contains_key(&start.session_id) {
                continue;
            }
            recent_sessions.insert(start.session_id.clone(), now);
            on_start(start);
        }
    }
}

fn scan_lifecycle_rows_after_event(
    logs_path: &Path,
    state_path: &Path,
    receiver: &Receiver<()>,
    logs_connection: &mut Option<Connection>,
    state_connection: &mut Option<Connection>,
    last_id: &mut i64,
) -> rusqlite::Result<Vec<LifecycleStart>> {
    let id_before_event = *last_id;
    let mut last_error = None;
    for delay_ms in EVENT_RETRY_DELAYS_MS {
        if *delay_ms > 0 {
            match receiver.try_recv() {
                Ok(()) => while receiver.try_recv().is_ok() {},
                Err(mpsc::TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(*delay_ms));
                }
                Err(mpsc::TryRecvError::Disconnected) => return Ok(Vec::new()),
            }
        }
        match scan_lifecycle_rows(
            logs_path,
            state_path,
            logs_connection,
            state_connection,
            last_id,
        ) {
            Ok(starts) if *last_id > id_before_event => return Ok(starts),
            Ok(_) => last_error = None,
            Err(cause) => {
                *logs_connection = None;
                *state_connection = None;
                last_error = Some(cause);
            }
        }
    }
    match last_error {
        Some(cause) => Err(cause),
        None => Ok(Vec::new()),
    }
}

fn scan_lifecycle_rows(
    logs_path: &Path,
    state_path: &Path,
    logs_connection: &mut Option<Connection>,
    state_connection: &mut Option<Connection>,
    last_id: &mut i64,
) -> rusqlite::Result<Vec<LifecycleStart>> {
    if logs_connection.is_none() {
        *logs_connection = Some(open_read_only(logs_path)?);
    }
    let logs = logs_connection
        .as_ref()
        .expect("logs connection initialized");
    let through_id = maximum_log_id(logs)?;
    if through_id <= *last_id {
        return Ok(Vec::new());
    }
    let rows = lifecycle_rows_between(logs, *last_id, through_id)?;
    *last_id = through_id;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    if state_connection.is_none() {
        *state_connection = open_read_only(state_path).ok();
    }
    rows.into_iter()
        .map(|(row_id, session_id)| {
            let (transcript_path, cwd) = state_connection
                .as_ref()
                .and_then(|state| thread_metadata(state, &session_id).ok())
                .unwrap_or_default();
            Ok(LifecycleStart {
                row_id,
                session_id,
                transcript_path,
                cwd,
            })
        })
        .collect()
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_millis(10))?;
    Ok(connection)
}

fn maximum_log_id(connection: &Connection) -> rusqlite::Result<i64> {
    connection.query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |row| {
        row.get(0)
    })
}

fn lifecycle_rows_between(
    connection: &Connection,
    after_id: i64,
    through_id: i64,
) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut statement = connection.prepare_cached(
        "SELECT id, thread_id FROM logs
         WHERE id > ?1 AND id <= ?2
           AND target = ?3 AND line = ?4 AND thread_id IS NOT NULL
         ORDER BY id",
    )?;
    let rows = statement.query_map(
        params![after_id, through_id, TURN_HANDLER_TARGET, TURN_HANDLER_LINE],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    rows.collect()
}

fn thread_metadata(
    connection: &Connection,
    session_id: &str,
) -> rusqlite::Result<(Option<String>, Option<String>)> {
    connection
        .query_row(
            "SELECT rollout_path, cwd FROM threads WHERE id = ?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map(|value| value.unwrap_or_default())
}

fn recent_thread_metadata(
    connection: &Connection,
    limit: usize,
) -> rusqlite::Result<Vec<RecentSessionInfo>> {
    let mut statement = connection.prepare_cached(
        "SELECT id, cwd, NULLIF(name, '')
         FROM threads
         WHERE archived = 0 AND thread_source = 'user'
         ORDER BY updated_at_ms DESC, updated_at DESC
         LIMIT ?1",
    )?;
    let rows = statement.query_map([limit as i64], |row| {
        Ok(RecentSessionInfo {
            session_id: row.get(0)?,
            cwd: row.get(1)?,
            title: row.get(2)?,
        })
    })?;
    rows.collect()
}

fn is_logs_database_path(candidate: &Path, logs_path: &Path) -> bool {
    candidate == logs_path
        || candidate == logs_path.with_extension("sqlite-wal")
        || candidate == logs_path.with_extension("sqlite-shm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_only_new_turn_handler_metadata_rows() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE logs (
                    id INTEGER PRIMARY KEY,
                    target TEXT,
                    line INTEGER,
                    thread_id TEXT,
                    feedback_log_body TEXT
                );
                INSERT INTO logs VALUES (1, 'unrelated', 528, 'session-a', 'prompt-like data');
                INSERT INTO logs VALUES (2, 'codex_core::session::handlers', 527, 'session-a', 'ignored');
                INSERT INTO logs VALUES (3, 'codex_core::session::handlers', 528, 'session-a', 'never selected');
                INSERT INTO logs VALUES (4, 'codex_core::session::handlers', 528, NULL, 'ignored');",
            )
            .unwrap();

        let rows = lifecycle_rows_between(&connection, 1, 4).unwrap();

        assert_eq!(rows, vec![(3, "session-a".into())]);
    }

    #[test]
    fn reads_only_rollout_path_and_cwd_from_thread_metadata() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    rollout_path TEXT,
                    cwd TEXT,
                    first_user_message TEXT
                );
                INSERT INTO threads VALUES ('session-a', 'rollout.jsonl', 'C:\\workspace', 'private');",
            )
            .unwrap();

        assert_eq!(
            thread_metadata(&connection, "session-a").unwrap(),
            (Some("rollout.jsonl".into()), Some("C:\\workspace".into()))
        );
        assert_eq!(
            thread_metadata(&connection, "missing").unwrap(),
            (None, None)
        );
    }

    #[test]
    fn recent_sessions_are_user_threads_in_activity_order() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    cwd TEXT,
                    title TEXT,
                    name TEXT,
                    archived INTEGER NOT NULL,
                    thread_source TEXT,
                    updated_at INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                INSERT INTO threads VALUES
                    ('older', 'C:\\older', 'Older prompt', 'Older title', 0, 'user', 1, 1000),
                    ('newer', 'C:\\newer', 'Newer prompt', NULL, 0, 'user', 2, 2000),
                    ('archived', 'C:\\archived', 'Archived', 'Archived', 1, 'user', 3, 3000),
                    ('internal', 'C:\\internal', 'Internal', 'Internal', 0, 'subagent', 4, 4000);",
            )
            .unwrap();

        assert_eq!(
            recent_thread_metadata(&connection, 5).unwrap(),
            vec![
                RecentSessionInfo {
                    session_id: "newer".into(),
                    cwd: Some("C:\\newer".into()),
                    title: None,
                },
                RecentSessionInfo {
                    session_id: "older".into(),
                    cwd: Some("C:\\older".into()),
                    title: Some("Older title".into()),
                },
            ]
        );
    }

    #[test]
    fn recognizes_sqlite_and_wal_updates_only() {
        let logs = PathBuf::from(r"C:\Users\Test\.codex\logs_2.sqlite");
        assert!(is_logs_database_path(&logs, &logs));
        assert!(is_logs_database_path(
            &PathBuf::from(r"C:\Users\Test\.codex\logs_2.sqlite-wal"),
            &logs
        ));
        assert!(!is_logs_database_path(
            &PathBuf::from(r"C:\Users\Test\.codex\state_5.sqlite-wal"),
            &logs
        ));
    }

    #[test]
    fn filesystem_update_emits_lifecycle_metadata_and_stops_cleanly() {
        const SAMPLE_COUNT: usize = 128;
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "codex-lifecycle-watcher-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let logs_path = directory.join("logs_2.sqlite");
        let state_path = directory.join("state_5.sqlite");
        let logs = Connection::open(&logs_path).unwrap();
        logs.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE logs (
                id INTEGER PRIMARY KEY,
                target TEXT,
                line INTEGER,
                thread_id TEXT
             );",
        )
        .unwrap();
        let mut state = Connection::open(&state_path).unwrap();
        state
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, cwd TEXT);",
            )
            .unwrap();
        let transaction = state.transaction().unwrap();
        for index in 0..SAMPLE_COUNT {
            transaction
                .execute(
                    "INSERT INTO threads VALUES (?1, 'rollout.jsonl', 'C:\\workspace')",
                    [format!("session-{index}")],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        let (sender, receiver) = mpsc::channel();
        let watcher = start_at(logs_path.clone(), state_path, move |start| {
            let _ = sender.send(start);
        })
        .unwrap();

        let mut latencies = Vec::new();
        for index in 0..SAMPLE_COUNT {
            let session_id = format!("session-{index}");
            let inserted_at = Instant::now();
            logs.execute(
                "INSERT INTO logs (target, line, thread_id) VALUES (?1, ?2, ?3)",
                params![TURN_HANDLER_TARGET, TURN_HANDLER_LINE, session_id],
            )
            .unwrap();
            let start = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            latencies.push(inserted_at.elapsed());

            assert_eq!(start.session_id, session_id);
            assert_eq!(start.transcript_path.as_deref(), Some("rollout.jsonl"));
            assert_eq!(start.cwd.as_deref(), Some(r"C:\workspace"));
        }
        assert!(
            latencies
                .iter()
                .all(|elapsed| *elapsed < Duration::from_secs(1))
        );
        latencies.sort_unstable();
        eprintln!(
            "lifecycle metadata watcher latency: median={:?}, p95={:?}, max={:?}, samples={}",
            latencies[SAMPLE_COUNT / 2],
            latencies[(SAMPLE_COUNT - 1) * 95 / 100],
            latencies[SAMPLE_COUNT - 1],
            SAMPLE_COUNT
        );
        drop(watcher);
        drop(logs);
        drop(state);
        fs::remove_dir_all(directory).unwrap();
    }
}
