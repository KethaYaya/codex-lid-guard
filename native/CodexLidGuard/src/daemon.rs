use std::collections::{HashMap, hash_map::Entry};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::logging;
use crate::model::{GuardRequest, GuardResponse, GuardSettings, LidState, PROTOCOL_VERSION};
use crate::sound::{self, AlertSound};
use crate::{codex_log, paths, win};

const SOUND_DEDUP_WINDOW: Duration = Duration::from_millis(750);
static STATUS_CACHE: OnceLock<Mutex<Option<Vec<u8>>>> = OnceLock::new();

struct DaemonState {
    active_turns: HashMap<String, Option<u64>>,
    latest_session_by_window: HashMap<u64, String>,
    power_policy: win::PowerPolicy,
    lid_state: LidState,
    pending_sleep: Option<Arc<AtomicBool>>,
    alerts: AlertSchedule,
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
        Self {
            active_turns: HashMap::new(),
            latest_session_by_window: HashMap::new(),
            power_policy: win::PowerPolicy::new(),
            lid_state: LidState::Unknown,
            pending_sleep: None,
            alerts: AlertSchedule::default(),
        }
    }

    fn cancel_pending_sleep(&mut self) {
        if let Some(token) = self.pending_sleep.take() {
            token.store(true, Ordering::Release);
        }
    }

    fn snapshot(&self, ok: bool, message: impl Into<String>) -> GuardResponse {
        GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            daemon_path: std::env::current_exe()
                .ok()
                .map(|value| value.to_string_lossy().into_owned()),
            daemon_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ok,
            message: message.into(),
            active_turns: self.active_turns.len(),
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

    loop {
        let can_exit = state
            .lock()
            .map(|value| value.active_turns.is_empty() && value.pending_sleep.is_none())
            .unwrap_or(false);
        let timeout = can_exit.then_some(Duration::from_secs(5 * 60));
        match pipe.accept(timeout) {
            Ok(win::AcceptResult::Connected(connection)) => {
                if let Err(cause) = handle_connection(connection, &state) {
                    logging::write(format!("Request handling failed: {cause}"));
                }
            }
            Ok(win::AcceptResult::TimedOut) => {
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

fn handle_request(shared: &Arc<Mutex<DaemonState>>, request: GuardRequest) -> GuardResponse {
    let action = request.action.to_ascii_lowercase();
    let mut sleep_schedule = None;
    let mut sound_schedule = None;
    let mut alert_window = None;
    let mut alert_session_is_current = true;
    let (response, should_publish_status) = {
        let Ok(mut state) = shared.lock() else {
            return failure("guardian state lock was poisoned");
        };
        let response = match action.as_str() {
            "acquire" => acquire(&mut state, &request),
            "release" => {
                let key = turn_key(&request);
                alert_window = state.active_turns.remove(&key).flatten();
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
        let should_publish_status =
            !matches!(action.as_str(), "status" | "sound-done" | "sound-request");
        (response, should_publish_status)
    };
    if should_publish_status {
        publish_status(&response);
    }
    if let Some((token, delay)) = sleep_schedule {
        spawn_sleep(shared, token, delay);
    }
    if let Some(sound) = sound_schedule {
        sound::start(sound);
    }
    response
}

fn acquire(state: &mut DaemonState, request: &GuardRequest) -> GuardResponse {
    let key = turn_key(request);
    let prefix = session_prefix(request);
    state.cancel_pending_sleep();
    let previous_count = state.active_turns.len();
    state
        .active_turns
        .retain(|candidate, _| !candidate.starts_with(&prefix) || candidate == &key);
    let replaced = previous_count.saturating_sub(state.active_turns.len());
    if replaced > 0 {
        logging::write(format!(
            "Removed {replaced} stale turn(s) for session {}.",
            prefix.trim_end_matches(':')
        ));
    }
    let inserted = match state.active_turns.entry(key.clone()) {
        Entry::Vacant(entry) => {
            entry.insert(request.origin_window);
            true
        }
        Entry::Occupied(_) => false,
    };
    if inserted
        && state.active_turns.len() == 1
        && let Err(cause) = state.power_policy.acquire()
    {
        state.active_turns.remove(&key);
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
    state.active_turns.get(&key).copied().flatten().or_else(|| {
        let prefix = session_prefix(request);
        state
            .active_turns
            .iter()
            .find_map(|(key, window)| key.starts_with(&prefix).then_some(*window).flatten())
    })
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
