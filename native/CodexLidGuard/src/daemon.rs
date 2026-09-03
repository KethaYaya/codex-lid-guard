use std::collections::{HashMap, hash_map::Entry};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::codex_transcript::TranscriptCursor;
use crate::logging;
use crate::model::{
    ActiveTurnInfo, GuardRequest, GuardResponse, GuardSettings, LidState, PROTOCOL_VERSION,
};
use crate::sound::{self, AlertSound};
use crate::{client, codex_lifecycle, codex_log, paths, win};

const SOUND_DEDUP_WINDOW: Duration = Duration::from_millis(750);
const ACTIVE_TURN_RECONCILE_INTERVAL: Duration = Duration::from_millis(250);
const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PRE_ACQUIRE_TTL: Duration = Duration::from_secs(10);
static STATUS_CACHE: OnceLock<Mutex<Option<Vec<u8>>>> = OnceLock::new();

struct DaemonState {
    active_turns: HashMap<String, TrackedTurn>,
    transcript_cursors: HashMap<String, TranscriptCursor>,
    latest_session_by_window: HashMap<u64, String>,
    next_turn_sequence: u64,
    power_policy: win::PowerPolicy,
    lid_state: LidState,
    pending_sleep: Option<Arc<AtomicBool>>,
    alerts: AlertSchedule,
}

struct TrackedTurn {
    info: ActiveTurnInfo,
    origin_window: Option<u64>,
    sequence: u64,
}

#[derive(Default)]
struct AlertSchedule {
    last_done_alert: Option<Instant>,
    last_request_alert: Option<Instant>,
}

impl AlertSchedule {
    fn schedule(&mut self, sound: AlertSound) -> bool {
        let now = Instant::now();
        let previous = match sound {
            AlertSound::Done => &mut self.last_done_alert,
            AlertSound::Request => &mut self.last_request_alert,
        };
        if previous.is_some_and(|instant| now.duration_since(instant) < SOUND_DEDUP_WINDOW) {
            return false;
        }
        *previous = Some(now);
        true
    }
}

impl DaemonState {
    fn new() -> Self {
        Self::with_power_policy(win::PowerPolicy::new())
    }

    fn with_power_policy(power_policy: win::PowerPolicy) -> Self {
        Self {
            active_turns: HashMap::new(),
            transcript_cursors: HashMap::new(),
            latest_session_by_window: HashMap::new(),
            next_turn_sequence: 0,
            power_policy,
            lid_state: LidState::Unknown,
            pending_sleep: None,
            alerts: AlertSchedule::default(),
        }
    }

    #[cfg(test)]
    fn new_for_test() -> Self {
        Self::with_power_policy(win::PowerPolicy::new_for_test())
    }

    fn cancel_pending_sleep(&mut self) {
        if let Some(token) = self.pending_sleep.take() {
            token.store(true, Ordering::Release);
        }
    }

    fn snapshot(&self, ok: bool, message: impl Into<String>) -> GuardResponse {
        let mut active_items = self
            .active_turns
            .values()
            .map(|turn| (turn.sequence, turn.info.clone()))
            .collect::<Vec<_>>();
        active_items.sort_by_key(|item| std::cmp::Reverse(item.0));
        GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            daemon_path: std::env::current_exe()
                .ok()
                .map(|value| value.to_string_lossy().into_owned()),
            daemon_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            pipe_name: Some(paths::pipe_name()),
            ok,
            message: message.into(),
            active_turns: self.active_turns.len(),
            active_items: active_items.into_iter().map(|(_, info)| info).collect(),
            recent_items: Vec::new(),
            is_guarding: self.power_policy.is_guarding(),
            lid_state: self.lid_state.as_str().to_string(),
            sleep_pending: self
                .pending_sleep
                .as_ref()
                .is_some_and(|token| !token.load(Ordering::Acquire)),
        }
    }

    fn schedule_alert(&mut self, sound: AlertSound) -> bool {
        self.alerts.schedule(sound)
    }
}

pub fn run() -> io::Result<()> {
    let Some(_instance) = win::InstanceMutex::acquire(&paths::mutex_name())? else {
        return Ok(());
    };
    let state = Arc::new(Mutex::new(DaemonState::new()));
    let watcher_state = Arc::clone(&state);
    let _watcher = win::LidWatcher::start(move |next| {
        if let Ok(mut state) = watcher_state.lock()
            && state.lid_state != next
        {
            state.lid_state = next;
            logging::write(format!("Lid state changed to {next:?}."));
            if next == LidState::Open {
                state.cancel_pending_sleep();
            }
            publish_status(&state.snapshot(true, "Lid state changed."));
        }
    })?;
    let pipe = win::PipeServer::new(&paths::pipe_name());
    logging::write(format!(
        "Guardian daemon started on pipe {}.",
        paths::pipe_name()
    ));
    if let Ok(state) = state.lock() {
        publish_status(&state.snapshot(true, "Guardian daemon ready."));
    }
    let lifecycle_state = Arc::clone(&state);
    let _lifecycle_watcher = codex_lifecycle::start(move |start| {
        let Some(cursor) =
            TranscriptCursor::new_for_active_session(start.transcript_path.as_deref())
        else {
            logging::write(format!(
                "Ignored inactive Codex lifecycle metadata row {} for session {}.",
                start.row_id, start.session_id
            ));
            return;
        };
        let session_id = start.session_id;
        let request = GuardRequest {
            action: "metadata-acquire".to_string(),
            session_id: Some(session_id.clone()),
            turn_id: Some(format!("pending-metadata-{}", start.row_id)),
            cwd: start.cwd,
            transcript_path: start.transcript_path,
            ..GuardRequest::default()
        };
        let key = turn_key(&request);
        let _ = handle_request(&lifecycle_state, request);
        if let Ok(mut state) = lifecycle_state.lock()
            && state.active_turns.contains_key(&key)
        {
            state.transcript_cursors.insert(key, cursor);
        }
        // Window discovery is deliberately after acquisition so UI bookkeeping
        // cannot add latency to the power-protection path.
        if let Some(origin_window) = win::foreground_editor_window() {
            let _ = handle_request(
                &lifecycle_state,
                GuardRequest {
                    action: "associate-window".to_string(),
                    session_id: Some(session_id),
                    origin_window: Some(origin_window),
                    ..GuardRequest::default()
                },
            );
        }
        // The guard is already active; this read-only request only interrupts a
        // five-minute idle accept so terminal reconciliation switches to its
        // normal two-second active cadence.
        let _ = client::send(GuardRequest {
            action: "status".to_string(),
            ..GuardRequest::default()
        });
    });

    loop {
        let has_active_turns = state
            .lock()
            .map(|value| !value.active_turns.is_empty())
            .unwrap_or(false);
        let timeout = Some(if has_active_turns {
            ACTIVE_TURN_RECONCILE_INTERVAL
        } else {
            IDLE_TIMEOUT
        });
        match pipe.accept(timeout) {
            Ok(win::AcceptResult::Connected(connection)) => {
                if let Err(cause) = handle_connection(connection, &state) {
                    logging::write(format!("Request handling failed: {cause}"));
                }
            }
            Ok(win::AcceptResult::TimedOut) => {
                if has_active_turns {
                    reconcile_terminal_turns(&state);
                    continue;
                }
                let still_idle = state
                    .lock()
                    .map(|value| value.active_turns.is_empty() && value.pending_sleep.is_none())
                    .unwrap_or(false);
                if still_idle {
                    logging::write("Guardian daemon reached its idle timeout.");
                    break;
                }
            }
            Err(cause) => logging::write(format!("Pipe connection ended unexpectedly: {cause}")),
        }
    }

    if let Ok(mut state) = state.lock() {
        state.cancel_pending_sleep();
        state.active_turns.clear();
        state.transcript_cursors.clear();
        state.latest_session_by_window.clear();
        if let Err(cause) = state.power_policy.release() {
            logging::write(format!("Power-policy cleanup failed: {cause}"));
        }
        publish_status(&state.snapshot(true, "Guardian daemon stopped."));
    }
    logging::write("Guardian daemon stopped.");
    Ok(())
}

fn handle_connection(
    connection: win::PipeConnection,
    shared: &Arc<Mutex<DaemonState>>,
) -> io::Result<()> {
    let line = connection.read_line()?;
    let response = match serde_json::from_str::<GuardRequest>(&line) {
        Ok(request) => {
            let mut response = handle_request(shared, request.clone());
            shield_newer_daemon_from_legacy_status(&request, &mut response);
            response
        }
        Err(cause) => {
            logging::write(format!("Guardian request failed: {cause}"));
            shared
                .lock()
                .map(|state| state.snapshot(false, cause.to_string()))
                .unwrap_or_else(|_| failure("guardian state lock was poisoned"))
        }
    };
    connection.write_line(&serde_json::to_string(&response)?)
}

fn shield_newer_daemon_from_legacy_status(request: &GuardRequest, response: &mut GuardResponse) {
    if request.action.eq_ignore_ascii_case("status")
        && request.client_version.is_none()
        && response.active_turns == 0
    {
        // v0.1.6 and v0.1.7 clients interpreted any different daemon version as
        // stale. Reporting a nonzero reference count only to those read-only
        // clients prevents an older VS Code window from downgrading a newer
        // idle daemon. The real event-driven status snapshot is unchanged.
        response.active_turns = 1;
    }
}

fn handle_request(shared: &Arc<Mutex<DaemonState>>, mut request: GuardRequest) -> GuardResponse {
    let action = request.action.to_ascii_lowercase();
    let mut sleep_schedule = None;
    let mut sound_schedule = None;
    let mut alert_window = None;
    let mut alert_session_is_current = true;
    let mut pre_acquire_expiry = None;
    let (response, should_publish_status) = {
        let Ok(mut state) = shared.lock() else {
            return failure("guardian state lock was poisoned");
        };
        let response = match action.as_str() {
            "acquire" => acquire(&mut state, &request),
            "pre-acquire" => {
                let (response, acquired, durable) = pre_acquire(&mut state, &mut request);
                if acquired && !durable {
                    pre_acquire_expiry = Some((
                        turn_key(&request),
                        request.session_id.clone().unwrap_or_default(),
                    ));
                }
                response
            }
            "metadata-acquire" => pre_acquire(&mut state, &mut request).0,
            "release" => {
                let key = turn_key(&request);
                alert_window = state
                    .active_turns
                    .remove(&key)
                    .and_then(|turn| turn.origin_window);
                state.transcript_cursors.remove(&key);
                let removed_provisionals = remove_session_provisionals(&mut state, &request);
                if removed_provisionals > 0 {
                    logging::write(format!(
                        "Released {removed_provisionals} metadata turn(s) for session {}.",
                        request.session_id.as_deref().unwrap_or("unknown-session")
                    ));
                }
                alert_session_is_current =
                    origin_session_is_current(&state, &request, alert_window);
                logging::write(format!(
                    "Turn released: {key}. Active turns: {}.",
                    state.active_turns.len()
                ));
                if state.active_turns.is_empty() {
                    if let Err(cause) = state.power_policy.release() {
                        state.snapshot(false, cause.to_string())
                    } else {
                        sleep_schedule = prepare_sleep(&mut state);
                        state.snapshot(true, "Codex turn finished.")
                    }
                } else {
                    state.snapshot(true, "Codex turn finished.")
                }
            }
            "release-session" => {
                let session = request.session_id.as_deref().unwrap_or_default();
                let prefix = format!("{session}:");
                state
                    .active_turns
                    .retain(|key, _| !key.starts_with(&prefix));
                state
                    .transcript_cursors
                    .retain(|key, _| !key.starts_with(&prefix));
                logging::write(format!(
                    "Session released: {session}. Active turns: {}.",
                    state.active_turns.len()
                ));
                if state.active_turns.is_empty() {
                    if let Err(cause) = state.power_policy.release() {
                        state.snapshot(false, cause.to_string())
                    } else {
                        sleep_schedule = prepare_sleep(&mut state);
                        state.snapshot(true, "Codex session finished.")
                    }
                } else {
                    state.snapshot(true, "Codex session finished.")
                }
            }
            "restore" => {
                state.active_turns.clear();
                state.transcript_cursors.clear();
                state.latest_session_by_window.clear();
                state.cancel_pending_sleep();
                match state.power_policy.release() {
                    Ok(()) => {
                        state.snapshot(true, "The original Windows power policy was restored.")
                    }
                    Err(cause) => state.snapshot(false, cause.to_string()),
                }
            }
            "status" => state.snapshot(true, "Status read."),
            "focus" => focus_turn(&state, &request),
            "associate-window" => associate_window(&mut state, &request),
            "sound-done" => state.snapshot(true, "Completion alert scheduled."),
            "sound-request" => {
                alert_window = origin_window_for_request(&state, &request);
                alert_session_is_current =
                    origin_session_is_current(&state, &request, alert_window);
                state.snapshot(true, "Needs-response alert scheduled.")
            }
            _ => state.snapshot(
                false,
                format!("Unknown guardian action '{}'.", request.action),
            ),
        };
        if response.ok {
            let requested_sound = match action.as_str() {
                "release" | "sound-done" => Some(AlertSound::Done),
                "sound-request" => Some(AlertSound::Request),
                _ => None,
            };
            if let Some(sound) = requested_sound.filter(|sound| {
                should_play_automatic_alert(*sound, alert_window, alert_session_is_current)
                    && state.schedule_alert(*sound)
            }) {
                sound_schedule = Some(sound);
            }
        }
        let should_publish_status = !matches!(
            action.as_str(),
            "status" | "focus" | "associate-window" | "sound-done" | "sound-request"
        );
        (response, should_publish_status)
    };
    if should_publish_status {
        publish_status(&response);
    }
    if let Some((key, session_id)) = pre_acquire_expiry {
        spawn_pre_acquire_expiry(shared, key, session_id);
    }
    if let Some((token, delay)) = sleep_schedule {
        spawn_sleep(shared, token, delay);
    }
    if let Some(sound) = sound_schedule {
        sound::start(sound);
    }
    response
}

fn spawn_pre_acquire_expiry(shared: &Arc<Mutex<DaemonState>>, key: String, session_id: String) {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        thread::sleep(PRE_ACQUIRE_TTL);
        let (transcript_path, cwd) = codex_lifecycle::session_metadata(&session_id);
        let promotion = TranscriptCursor::new_for_active_session(transcript_path.as_deref())
            .map(|cursor| (cursor, cwd));
        let mut sleep_schedule = None;
        let response = {
            let Ok(mut state) = shared.lock() else {
                return;
            };
            if let Some((cursor, cwd)) = promotion {
                let Some(turn) = state.active_turns.get_mut(&key) else {
                    return;
                };
                if turn.info.cwd.is_none() {
                    turn.info.cwd = cwd;
                }
                if !turn.info.turn_id.starts_with("pending-metadata-") {
                    turn.info.turn_id = format!("pending-metadata-log-{}", turn.info.turn_id);
                }
                state.transcript_cursors.insert(key.clone(), cursor);
                logging::write(format!(
                    "Promoted provisional turn to transcript tracking: {key}."
                ));
                state.snapshot(true, "Codex turn tracking was confirmed.")
            } else {
                if state.active_turns.remove(&key).is_none() {
                    return;
                }
                state.transcript_cursors.remove(&key);
                logging::write(format!(
                    "Provisional turn expired before the Codex hook arrived: {key}. Active turns: {}.",
                    state.active_turns.len()
                ));
                if state.active_turns.is_empty() {
                    if let Err(cause) = state.power_policy.release() {
                        state.snapshot(false, cause.to_string())
                    } else {
                        sleep_schedule = prepare_sleep(&mut state);
                        state.snapshot(true, "Provisional Codex turn expired.")
                    }
                } else {
                    state.snapshot(true, "Provisional Codex turn expired.")
                }
            }
        };
        publish_status(&response);
        if let Some((token, delay)) = sleep_schedule {
            spawn_sleep(&shared, token, delay);
        }
    });
}

fn acquire(state: &mut DaemonState, request: &GuardRequest) -> GuardResponse {
    let key = turn_key(request);
    let prefix = session_prefix(request);
    state.cancel_pending_sleep();
    let replaced = remove_stale_session_turns(state, &prefix, &key);
    if replaced > 0 {
        logging::write(format!(
            "Removed {replaced} stale turn(s) for session {}.",
            prefix.trim_end_matches(':')
        ));
    }
    let info = active_turn_info(request);
    state.next_turn_sequence = state.next_turn_sequence.wrapping_add(1);
    let sequence = state.next_turn_sequence;
    let inserted = match state.active_turns.entry(key.clone()) {
        Entry::Vacant(entry) => {
            entry.insert(TrackedTurn {
                info,
                origin_window: request.origin_window,
                sequence,
            });
            true
        }
        Entry::Occupied(mut entry) => {
            let turn = entry.get_mut();
            if turn.origin_window.is_none() {
                turn.origin_window = request.origin_window;
            }
            if turn.info.cwd.is_none() {
                turn.info.cwd = info.cwd;
            }
            false
        }
    };
    let cursor = if request.action.eq_ignore_ascii_case("metadata-acquire")
        || request
            .turn_id
            .as_deref()
            .is_some_and(|turn_id| turn_id.starts_with("pending-metadata-"))
    {
        TranscriptCursor::new_for_session(request.transcript_path.as_deref())
    } else {
        TranscriptCursor::new(
            request.transcript_path.as_deref(),
            request.turn_id.as_deref(),
        )
    };
    if let Some(cursor) = cursor {
        state
            .transcript_cursors
            .entry(key.clone())
            .or_insert(cursor);
    }
    if inserted
        && state.active_turns.len() == 1
        && let Err(cause) = state.power_policy.acquire()
    {
        state.active_turns.remove(&key);
        state.transcript_cursors.remove(&key);
        return state.snapshot(false, cause.to_string());
    }
    remember_latest_session(state, request);
    logging::write(format!(
        "Turn acquired: {key}. Active turns: {}.",
        state.active_turns.len()
    ));
    state.snapshot(
        true,
        "Windows will stay awake until the Codex turn finishes.",
    )
}

fn pre_acquire(state: &mut DaemonState, request: &mut GuardRequest) -> (GuardResponse, bool, bool) {
    let prefix = session_prefix(request);
    if request.origin_window.is_some() {
        // A helper fallback may arrive after direct pipe acquisition succeeded
        // but its response timed out. Preserve the helper's authoritative HWND
        // even when the provisional turn is already present.
        let _ = associate_window(state, request);
    }
    let has_authoritative_turn = state
        .active_turns
        .iter()
        .any(|(key, turn)| key.starts_with(&prefix) && !turn.info.turn_id.starts_with("pending-"));
    if has_authoritative_turn {
        logging::write(format!(
            "Ignored late provisional acquisition for session {} because its authoritative turn is already active.",
            prefix.trim_end_matches(':')
        ));
        return (
            state.snapshot(true, "The authoritative Codex turn is already active."),
            false,
            true,
        );
    }
    let has_metadata_turn = state.active_turns.iter().any(|(key, turn)| {
        key.starts_with(&prefix) && turn.info.turn_id.starts_with("pending-metadata-")
    });
    if has_metadata_turn {
        return (
            state.snapshot(true, "The Codex metadata turn is already active."),
            false,
            true,
        );
    }

    if request.action.eq_ignore_ascii_case("pre-acquire")
        && let Some(session_id) = request.session_id.as_deref()
    {
        let (transcript_path, cwd) = codex_lifecycle::session_metadata(session_id);
        attach_session_metadata(request, transcript_path, cwd);
    }

    let response = acquire(state, request);
    let acquired = response.ok;
    let durable = request.transcript_path.is_some()
        && request
            .turn_id
            .as_deref()
            .is_some_and(|turn_id| turn_id.starts_with("pending-metadata-"));
    (response, acquired, durable)
}

fn attach_session_metadata(
    request: &mut GuardRequest,
    transcript_path: Option<String>,
    cwd: Option<String>,
) {
    if request.cwd.is_none() {
        request.cwd = cwd;
    }
    if let Some(transcript_path) = transcript_path.filter(|value| Path::new(value).is_file()) {
        request.transcript_path = Some(transcript_path);
        let pending_id = request.turn_id.as_deref().unwrap_or("log");
        request.turn_id = Some(format!("pending-metadata-log-{pending_id}"));
    }
}

fn remove_stale_session_turns(state: &mut DaemonState, prefix: &str, key: &str) -> usize {
    let previous_count = state.active_turns.len();
    state
        .active_turns
        .retain(|candidate, _| !candidate.starts_with(prefix) || candidate == key);
    state
        .transcript_cursors
        .retain(|candidate, _| !candidate.starts_with(prefix) || candidate == key);
    previous_count.saturating_sub(state.active_turns.len())
}

fn remove_session_provisionals(state: &mut DaemonState, request: &GuardRequest) -> usize {
    let prefix = session_prefix(request);
    let keys = state
        .active_turns
        .iter()
        .filter(|(key, turn)| key.starts_with(&prefix) && turn.info.turn_id.starts_with("pending-"))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in &keys {
        state.active_turns.remove(key);
        state.transcript_cursors.remove(key);
    }
    keys.len()
}

fn reconcile_terminal_turns(shared: &Arc<Mutex<DaemonState>>) {
    let mut sleep_schedule = None;
    let mut sound_schedule = false;
    let response = {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        let ended = state
            .transcript_cursors
            .iter_mut()
            .filter_map(|(key, cursor)| cursor.reached_terminal_event().then(|| key.clone()))
            .collect::<Vec<_>>();
        if ended.is_empty() {
            return;
        }
        let mut alert_candidate = None;
        for key in &ended {
            if let Some(turn) = state.active_turns.remove(key) {
                alert_candidate.get_or_insert((turn.origin_window, turn.info.session_id));
            }
            state.transcript_cursors.remove(key);
            logging::write(format!(
                "Recovered terminal turn after a missed hook: {key}. Active turns: {}.",
                state.active_turns.len()
            ));
        }
        if let Some((origin_window, session_id)) = alert_candidate {
            let request = GuardRequest {
                session_id: Some(session_id),
                origin_window,
                ..GuardRequest::default()
            };
            let session_is_current = origin_session_is_current(&state, &request, origin_window);
            sound_schedule =
                should_play_automatic_alert(AlertSound::Done, origin_window, session_is_current)
                    && state.schedule_alert(AlertSound::Done);
        }
        if state.active_turns.is_empty() {
            if let Err(cause) = state.power_policy.release() {
                state.snapshot(false, cause.to_string())
            } else {
                sleep_schedule = prepare_sleep(&mut state);
                state.snapshot(true, "Recovered a finished Codex turn.")
            }
        } else {
            state.snapshot(true, "Recovered a finished Codex turn.")
        }
    };
    publish_status(&response);
    if let Some((token, delay)) = sleep_schedule {
        spawn_sleep(shared, token, delay);
    }
    if sound_schedule {
        sound::start(AlertSound::Done);
    }
}

fn remember_latest_session(state: &mut DaemonState, request: &GuardRequest) {
    if let (Some(window), Some(session)) = (
        request.origin_window,
        request
            .session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty()),
    ) {
        state
            .latest_session_by_window
            .insert(window, session.to_string());
    }
}

fn origin_window_for_request(state: &DaemonState, request: &GuardRequest) -> Option<u64> {
    let key = turn_key(request);
    state
        .active_turns
        .get(&key)
        .and_then(|turn| turn.origin_window)
        .or_else(|| {
            let prefix = session_prefix(request);
            state.active_turns.iter().find_map(|(key, turn)| {
                key.starts_with(&prefix)
                    .then_some(turn.origin_window)
                    .flatten()
            })
        })
}

fn focus_turn(state: &DaemonState, request: &GuardRequest) -> GuardResponse {
    let Some(window) = origin_window_for_request(state, request) else {
        return state.snapshot(
            false,
            "The originating editor window is no longer available.",
        );
    };
    if win::focus_editor_window(window) {
        state.snapshot(true, "Focused the originating editor window.")
    } else {
        state.snapshot(false, "Could not focus the originating editor window.")
    }
}

fn associate_window(state: &mut DaemonState, request: &GuardRequest) -> GuardResponse {
    let Some(window) = request.origin_window else {
        return state.snapshot(false, "The originating editor window is unavailable.");
    };
    let prefix = session_prefix(request);
    for (key, turn) in &mut state.active_turns {
        if key.starts_with(&prefix)
            && (turn.origin_window.is_none() || request.origin_window_authoritative)
        {
            turn.origin_window = Some(window);
        }
    }
    remember_latest_session(state, request);
    state.snapshot(true, "Associated the Codex turn with its editor window.")
}

fn origin_session_is_current(
    state: &DaemonState,
    request: &GuardRequest,
    origin_window: Option<u64>,
) -> bool {
    let Some(window) = origin_window else {
        return true;
    };
    let Some(session) = request
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return true;
    };
    match codex_log::view_state_for_session(session) {
        Some(codex_log::ViewState::Active(active_session)) => {
            if active_session != session {
                logging::write("The alert belongs to a different chat than the active Codex view.");
            }
            return active_session == session;
        }
        Some(codex_log::ViewState::Inactive) => {
            logging::write("The originating Codex chat is no longer visible.");
            return false;
        }
        None => {}
    }
    latest_session_matches(
        state
            .latest_session_by_window
            .get(&window)
            .map(String::as_str),
        Some(session),
    )
}

fn latest_session_matches(latest: Option<&str>, session: Option<&str>) -> bool {
    match (latest, session) {
        (Some(latest), Some(session)) => latest == session,
        _ => true,
    }
}

fn should_play_automatic_alert(
    sound: AlertSound,
    origin_window: Option<u64>,
    origin_session_is_current: bool,
) -> bool {
    let settings = GuardSettings::load();
    if !settings.alert_sounds {
        return false;
    }
    let origin_is_focused = origin_window.is_some_and(win::is_window_focused);
    let should_play = alert_should_play(
        settings.alert_sounds_only_when_unfocused,
        origin_window,
        origin_is_focused,
        origin_session_is_current,
    );
    if !should_play {
        logging::write(format!(
            "Suppressed {} alert because its Codex chat is current in the focused editor window.",
            sound.label()
        ));
    }
    should_play
}

fn alert_should_play(
    only_when_unfocused: bool,
    origin_window: Option<u64>,
    origin_is_focused: bool,
    origin_session_is_current: bool,
) -> bool {
    !only_when_unfocused
        || origin_window.is_none()
        || !origin_is_focused
        || !origin_session_is_current
}

fn prepare_sleep(state: &mut DaemonState) -> Option<(Arc<AtomicBool>, Duration)> {
    let settings = GuardSettings::load();
    if !settings.sleep_when_lid_closed || state.lid_state != LidState::Closed {
        return None;
    }
    state.cancel_pending_sleep();
    let token = Arc::new(AtomicBool::new(false));
    state.pending_sleep = Some(Arc::clone(&token));
    logging::write(format!(
        "Sleep scheduled in {} seconds because the lid is closed.",
        settings.sleep_delay_seconds
    ));
    Some((token, Duration::from_secs(settings.sleep_delay_seconds)))
}

fn spawn_sleep(shared: &Arc<Mutex<DaemonState>>, token: Arc<AtomicBool>, delay: Duration) {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        thread::sleep(delay);
        if token.load(Ordering::Acquire) {
            return;
        }
        let should_suspend = if let Ok(mut state) = shared.lock() {
            let matches_pending = state
                .pending_sleep
                .as_ref()
                .is_some_and(|pending| Arc::ptr_eq(pending, &token));
            if matches_pending
                && state.active_turns.is_empty()
                && state.lid_state == LidState::Closed
            {
                state.pending_sleep = None;
                publish_status(&state.snapshot(true, "Requesting Windows sleep."));
                true
            } else {
                false
            }
        } else {
            false
        };
        if should_suspend && !win::suspend() {
            logging::write("Windows rejected the sleep request.");
        }
    });
}

fn publish_status(response: &GuardResponse) {
    let Ok(bytes) = serde_json::to_vec(response) else {
        return;
    };
    let Ok(mut cached) = STATUS_CACHE.get_or_init(|| Mutex::new(None)).lock() else {
        return;
    };
    if !snapshot_changed(cached.as_deref(), &bytes) {
        return;
    }
    if let Err(cause) = win::atomic_write(&paths::status_file(), &bytes) {
        logging::write(format!("Could not publish guardian status: {cause}"));
    } else {
        *cached = Some(bytes);
    }
}

fn snapshot_changed(previous: Option<&[u8]>, next: &[u8]) -> bool {
    previous != Some(next)
}

fn turn_key(request: &GuardRequest) -> String {
    let session = request
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown-session");
    let turn = request
        .turn_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("current-turn");
    format!("{session}:{turn}")
}

fn active_turn_info(request: &GuardRequest) -> ActiveTurnInfo {
    ActiveTurnInfo {
        session_id: request
            .session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("unknown-session")
            .to_string(),
        turn_id: request
            .turn_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("current-turn")
            .to_string(),
        cwd: request
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    }
}

fn session_prefix(request: &GuardRequest) -> String {
    let session = request
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown-session");
    format!("{session}:")
}

fn failure(message: impl Into<String>) -> GuardResponse {
    GuardResponse {
        protocol_version: PROTOCOL_VERSION,
        daemon_path: std::env::current_exe()
            .ok()
            .map(|value| value.to_string_lossy().into_owned()),
        daemon_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        pipe_name: Some(paths::pipe_name()),
        message: message.into(),
        ..GuardResponse::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_keys_default_missing_ids() {
        let request = GuardRequest::default();
        assert_eq!(turn_key(&request), "unknown-session:current-turn");
    }

    #[test]
    fn turn_keys_preserve_session_and_turn() {
        let request = GuardRequest {
            session_id: Some("session".into()),
            turn_id: Some("turn".into()),
            ..GuardRequest::default()
        };
        assert_eq!(turn_key(&request), "session:turn");
    }

    #[test]
    fn log_pre_acquires_become_durable_when_session_metadata_is_available() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let transcript = std::env::temp_dir().join(format!(
            "codex-lid-guard-log-fallback-{}-{unique}.jsonl",
            std::process::id()
        ));
        std::fs::write(&transcript, b"").unwrap();
        let mut request = GuardRequest {
            action: "pre-acquire".into(),
            session_id: Some("session".into()),
            turn_id: Some("pending-fast".into()),
            ..GuardRequest::default()
        };

        attach_session_metadata(
            &mut request,
            Some(transcript.to_string_lossy().into_owned()),
            Some(r"C:\workspace".into()),
        );

        assert_eq!(
            request.turn_id.as_deref(),
            Some("pending-metadata-log-pending-fast")
        );
        assert_eq!(request.transcript_path.as_deref(), transcript.to_str());
        assert_eq!(request.cwd.as_deref(), Some(r"C:\workspace"));
        std::fs::remove_file(transcript).unwrap();
    }

    #[test]
    fn editor_window_is_associated_after_the_guard_is_already_active() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "session:pending-metadata-1".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "session".into(),
                    turn_id: "pending-metadata-1".into(),
                    cwd: None,
                },
                origin_window: None,
                sequence: 1,
            },
        );
        let request = GuardRequest {
            action: "associate-window".into(),
            session_id: Some("session".into()),
            origin_window: Some(42),
            ..GuardRequest::default()
        };

        let response = associate_window(&mut state, &request);

        assert!(response.ok);
        assert_eq!(
            state
                .active_turns
                .get("session:pending-metadata-1")
                .and_then(|turn| turn.origin_window),
            Some(42)
        );
        assert_eq!(
            state.latest_session_by_window.get(&42).map(String::as_str),
            Some("session")
        );
    }

    #[test]
    fn authoritative_editor_window_replaces_a_metadata_watcher_guess() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "session:pending-metadata-1".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "session".into(),
                    turn_id: "pending-metadata-1".into(),
                    cwd: None,
                },
                origin_window: Some(41),
                sequence: 1,
            },
        );
        let heuristic = GuardRequest {
            action: "associate-window".into(),
            session_id: Some("session".into()),
            origin_window: Some(42),
            ..GuardRequest::default()
        };

        let _ = associate_window(&mut state, &heuristic);
        assert_eq!(
            state
                .active_turns
                .get("session:pending-metadata-1")
                .and_then(|turn| turn.origin_window),
            Some(41)
        );

        let authoritative = GuardRequest {
            origin_window_authoritative: true,
            origin_window: Some(43),
            ..heuristic
        };
        let _ = associate_window(&mut state, &authoritative);
        assert_eq!(
            state
                .active_turns
                .get("session:pending-metadata-1")
                .and_then(|turn| turn.origin_window),
            Some(43)
        );
    }

    #[test]
    fn helper_fallback_updates_an_existing_provisional_turn_window() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "session:pending-metadata-log-fast".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "session".into(),
                    turn_id: "pending-metadata-log-fast".into(),
                    cwd: None,
                },
                origin_window: Some(41),
                sequence: 1,
            },
        );
        let mut request = GuardRequest {
            action: "pre-acquire".into(),
            session_id: Some("session".into()),
            turn_id: Some("pending-fallback".into()),
            origin_window: Some(42),
            origin_window_authoritative: true,
            ..GuardRequest::default()
        };

        let (response, acquired, durable) = pre_acquire(&mut state, &mut request);

        assert!(response.ok);
        assert!(!acquired);
        assert!(durable);
        assert_eq!(
            state
                .active_turns
                .get("session:pending-metadata-log-fast")
                .and_then(|turn| turn.origin_window),
            Some(42)
        );
    }

    #[test]
    fn durable_log_fallback_waits_for_a_real_terminal_record() {
        use std::io::Write as _;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let transcript = std::env::temp_dir().join(format!(
            "codex-lid-guard-durable-fallback-{}-{unique}.jsonl",
            std::process::id()
        ));
        std::fs::write(
            &transcript,
            b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"real-turn\"}}\n",
        )
        .unwrap();
        let mut state = DaemonState::new_for_test();
        let mut request = GuardRequest {
            action: "pre-acquire".into(),
            session_id: Some("session-without-index-entry".into()),
            turn_id: Some("pending-metadata-log-fast".into()),
            transcript_path: Some(transcript.to_string_lossy().into_owned()),
            ..GuardRequest::default()
        };

        let (response, acquired, durable) = pre_acquire(&mut state, &mut request);

        assert!(response.ok);
        assert!(acquired);
        assert!(durable);
        let key = turn_key(&request);
        assert!(state.active_turns.contains_key(&key));
        assert!(
            !state
                .transcript_cursors
                .get_mut(&key)
                .unwrap()
                .reached_terminal_event()
        );

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        file.write_all(
            b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"real-turn\"}}\n",
        )
        .unwrap();
        file.flush().unwrap();
        assert!(
            state
                .transcript_cursors
                .get_mut(&key)
                .unwrap()
                .reached_terminal_event()
        );

        drop(file);
        std::fs::remove_file(transcript).unwrap();
    }

    #[test]
    fn active_turn_metadata_is_exposed_newest_first() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "older:turn".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "older".into(),
                    turn_id: "turn".into(),
                    cwd: Some(r"C:\older".into()),
                },
                origin_window: Some(1),
                sequence: 1,
            },
        );
        state.active_turns.insert(
            "newer:turn".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "newer".into(),
                    turn_id: "turn".into(),
                    cwd: Some(r"C:\newer".into()),
                },
                origin_window: Some(2),
                sequence: 2,
            },
        );

        let snapshot = state.snapshot(true, "Status read.");
        assert_eq!(snapshot.active_turns, 2);
        assert_eq!(snapshot.active_items[0].session_id, "newer");
        assert_eq!(snapshot.active_items[1].session_id, "older");
    }

    #[test]
    fn authoritative_turn_key_removes_a_provisional_turn_for_the_same_session() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "other:turn".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "other".into(),
                    turn_id: "turn".into(),
                    cwd: None,
                },
                origin_window: None,
                sequence: 1,
            },
        );
        state.active_turns.insert(
            "session:pending-1".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "session".into(),
                    turn_id: "pending-1".into(),
                    cwd: None,
                },
                origin_window: None,
                sequence: 2,
            },
        );

        let replaced = remove_stale_session_turns(&mut state, "session:", "session:turn-1");

        assert_eq!(replaced, 1);
        assert!(!state.active_turns.contains_key("session:pending-1"));
        assert!(state.active_turns.contains_key("other:turn"));
    }

    #[test]
    fn late_provisional_turn_does_not_replace_an_authoritative_turn() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "session:turn-1".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "session".into(),
                    turn_id: "turn-1".into(),
                    cwd: Some(r"C:\workspace".into()),
                },
                origin_window: Some(42),
                sequence: 1,
            },
        );
        let mut request = GuardRequest {
            action: "pre-acquire".into(),
            session_id: Some("session".into()),
            turn_id: Some("pending-late".into()),
            cwd: Some(r"C:\workspace".into()),
            origin_window: Some(42),
            ..GuardRequest::default()
        };

        let (response, acquired, durable) = pre_acquire(&mut state, &mut request);

        assert!(response.ok);
        assert!(!acquired);
        assert!(durable);
        assert_eq!(state.active_turns.len(), 1);
        assert!(state.active_turns.contains_key("session:turn-1"));
        assert!(!state.active_turns.contains_key("session:pending-late"));
    }

    #[test]
    fn exact_release_removes_a_metadata_turn_for_the_same_session() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "session:pending-metadata-42".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "session".into(),
                    turn_id: "pending-metadata-42".into(),
                    cwd: None,
                },
                origin_window: None,
                sequence: 1,
            },
        );
        let request = GuardRequest {
            action: "release".into(),
            session_id: Some("session".into()),
            turn_id: Some("real-turn".into()),
            ..GuardRequest::default()
        };

        assert_eq!(remove_session_provisionals(&mut state, &request), 1);
        assert!(state.active_turns.is_empty());
    }

    #[test]
    fn slower_log_pre_acquire_cannot_downgrade_a_metadata_turn() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "session:pending-metadata-42".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "session".into(),
                    turn_id: "pending-metadata-42".into(),
                    cwd: Some(r"C:\workspace".into()),
                },
                origin_window: None,
                sequence: 1,
            },
        );
        let mut request = GuardRequest {
            action: "pre-acquire".into(),
            session_id: Some("session".into()),
            turn_id: Some("pending-log".into()),
            ..GuardRequest::default()
        };

        let (response, acquired, durable) = pre_acquire(&mut state, &mut request);

        assert!(response.ok);
        assert!(!acquired);
        assert!(durable);
        assert!(
            state
                .active_turns
                .contains_key("session:pending-metadata-42")
        );
        assert!(!state.active_turns.contains_key("session:pending-log"));
    }

    #[test]
    fn late_metadata_acquire_cannot_reset_a_durable_log_cursor() {
        let mut state = DaemonState::new_for_test();
        state.active_turns.insert(
            "session:pending-metadata-log-fast".into(),
            TrackedTurn {
                info: ActiveTurnInfo {
                    session_id: "session".into(),
                    turn_id: "pending-metadata-log-fast".into(),
                    cwd: Some(r"C:\workspace".into()),
                },
                origin_window: None,
                sequence: 1,
            },
        );
        let mut request = GuardRequest {
            action: "metadata-acquire".into(),
            session_id: Some("session".into()),
            turn_id: Some("pending-metadata-42".into()),
            ..GuardRequest::default()
        };

        let (response, acquired, durable) = pre_acquire(&mut state, &mut request);

        assert!(response.ok);
        assert!(!acquired);
        assert!(durable);
        assert!(
            state
                .active_turns
                .contains_key("session:pending-metadata-log-fast")
        );
        assert!(
            !state
                .active_turns
                .contains_key("session:pending-metadata-42")
        );
    }

    #[test]
    fn duplicate_alerts_are_suppressed_without_suppressing_other_alerts() {
        let mut alerts = AlertSchedule::default();
        assert!(alerts.schedule(AlertSound::Request));
        assert!(!alerts.schedule(AlertSound::Request));
        assert!(alerts.schedule(AlertSound::Done));
    }

    #[test]
    fn unchanged_status_snapshots_do_not_need_another_disk_write() {
        assert!(!snapshot_changed(Some(b"same"), b"same"));
        assert!(snapshot_changed(Some(b"before"), b"after"));
        assert!(snapshot_changed(None, b"first"));
    }

    #[test]
    fn focused_origin_windows_suppress_automatic_alerts() {
        assert!(!alert_should_play(true, Some(42), true, true));
        assert!(alert_should_play(true, Some(42), false, true));
        assert!(alert_should_play(true, None, false, true));
    }

    #[test]
    fn a_different_recent_chat_in_the_same_window_receives_alerts() {
        assert!(alert_should_play(true, Some(42), true, false));
    }

    #[test]
    fn latest_messaged_chat_is_compared_by_session_id() {
        assert!(latest_session_matches(Some("chat-a"), Some("chat-a")));
        assert!(!latest_session_matches(Some("chat-b"), Some("chat-a")));
        assert!(latest_session_matches(None, Some("chat-a")));
        assert!(latest_session_matches(Some("chat-a"), None));
    }

    #[test]
    fn focus_filter_can_be_disabled() {
        assert!(alert_should_play(false, Some(42), true, true));
    }

    #[test]
    fn legacy_status_clients_cannot_downgrade_an_idle_newer_daemon() {
        let request = GuardRequest {
            action: "status".into(),
            client_version: None,
            ..GuardRequest::default()
        };
        let mut response = GuardResponse {
            active_turns: 0,
            is_guarding: false,
            ..GuardResponse::default()
        };
        shield_newer_daemon_from_legacy_status(&request, &mut response);
        assert_eq!(response.active_turns, 1);
        assert!(!response.is_guarding);
    }

    #[test]
    fn versioned_status_clients_receive_the_real_idle_count() {
        let request = GuardRequest {
            action: "status".into(),
            client_version: Some(env!("CARGO_PKG_VERSION").into()),
            ..GuardRequest::default()
        };
        let mut response = GuardResponse {
            active_turns: 0,
            ..GuardResponse::default()
        };
        shield_newer_daemon_from_legacy_status(&request, &mut response);
        assert_eq!(response.active_turns, 0);
    }
}
