use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use std::os::windows::process::CommandExt;

use crate::logging;
use crate::model::{GuardRequest, GuardResponse, PROTOCOL_VERSION};
use crate::{paths, win};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(1);

pub fn send(mut request: GuardRequest) -> GuardResponse {
    request.client_version = Some(env!("CARGO_PKG_VERSION").to_string());
    let mut response = try_send(&request, Duration::from_millis(25));
    if let Some(value) = response.as_ref() {
        if !is_compatible(value) {
            retire_legacy_daemon();
            response = None;
        } else if should_replace_idle_daemon(&request, value) {
            logging::write(format!(
                "Replacing idle guardian version {} with {}.",
                value.daemon_version.as_deref().unwrap_or("legacy"),
                env!("CARGO_PKG_VERSION")
            ));
            retire_legacy_daemon();
            response = None;
        }
    }
    if let Some(response) = response {
        return response;
    }
    if let Err(cause) = start_daemon() {
        return failure(format!("Could not start the guardian daemon: {cause}"));
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut retry_delay = Duration::from_millis(5);
    while Instant::now() < deadline {
        if let Some(response) = try_send(&request, Duration::from_millis(100))
            && is_compatible(&response)
        {
            return response;
        }
        thread::sleep(retry_delay);
        retry_delay = (retry_delay * 2).min(Duration::from_millis(40));
    }
    failure("Could not start the guardian daemon.")
}

fn try_send(request: &GuardRequest, timeout: Duration) -> Option<GuardResponse> {
    let connection = win::connect_pipe(&paths::pipe_name(), timeout, RESPONSE_TIMEOUT).ok()?;
    connection
        .write_line(&serde_json::to_string(request).ok()?)
        .ok()?;
    serde_json::from_str(&connection.read_line().ok()?).ok()
}

fn start_daemon() -> std::io::Result<()> {
    let executable = std::env::current_exe()?;
    match spawn_daemon(&executable, CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB) {
        Ok(()) => Ok(()),
        Err(breakaway_error) => {
            logging::write(format!(
                "Could not start the guardian outside the hook job; using the normal process group: {breakaway_error}"
            ));
            spawn_daemon(&executable, CREATE_NO_WINDOW)
        }
    }
}

fn spawn_daemon(executable: &std::path::Path, creation_flags: u32) -> std::io::Result<()> {
    Command::new(executable)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(creation_flags)
        .spawn()?;
    Ok(())
}

fn retire_legacy_daemon() {
    logging::write(
        "A legacy or different-build guardian daemon answered the pipe; restoring power policy and replacing it.",
    );
    let restore = GuardRequest {
        action: "restore".to_string(),
        ..GuardRequest::default()
    };
    let _ = try_send(&restore, Duration::from_millis(1_500));
    for process_id in win::terminate_other_helpers() {
        logging::write(format!("Stopped legacy guardian process {process_id}."));
    }
    thread::sleep(Duration::from_millis(25));
}

fn is_compatible(response: &GuardResponse) -> bool {
    response.protocol_version == PROTOCOL_VERSION
}

fn should_replace_idle_daemon(request: &GuardRequest, response: &GuardResponse) -> bool {
    request.action.eq_ignore_ascii_case("status")
        && response.active_turns == 0
        && response
            .daemon_version
            .as_deref()
            .is_none_or(|version| version_is_older(version, env!("CARGO_PKG_VERSION")))
}

fn version_is_older(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate < current,
        _ => true,
    }
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let core = value.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
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
    fn compatible_daemons_can_move_between_extension_install_paths() {
        let response = GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            daemon_path: Some(r"C:\different\extension\CodexLidGuard.exe".into()),
            daemon_version: Some(env!("CARGO_PKG_VERSION").into()),
            ..GuardResponse::default()
        };
        assert!(is_compatible(&response));
    }

    #[test]
    fn incompatible_protocols_are_replaced() {
        let response = GuardResponse {
            protocol_version: PROTOCOL_VERSION - 1,
            ..GuardResponse::default()
        };
        assert!(!is_compatible(&response));
    }

    #[test]
    fn idle_daemons_from_an_older_extension_are_replaced_on_status() {
        let request = GuardRequest {
            action: "status".into(),
            ..GuardRequest::default()
        };
        let response = GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            daemon_version: Some("0.1.5".into()),
            active_turns: 0,
            ..GuardResponse::default()
        };
        assert!(should_replace_idle_daemon(&request, &response));
    }

    #[test]
    fn older_daemons_are_never_replaced_while_a_turn_is_active() {
        let request = GuardRequest {
            action: "status".into(),
            ..GuardRequest::default()
        };
        let response = GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            daemon_version: Some("0.1.5".into()),
            active_turns: 1,
            ..GuardResponse::default()
        };
        assert!(!should_replace_idle_daemon(&request, &response));
    }

    #[test]
    fn newer_idle_daemons_are_never_replaced_by_older_helpers() {
        let request = GuardRequest {
            action: "status".into(),
            ..GuardRequest::default()
        };
        let response = GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            daemon_version: Some("99.0.0".into()),
            active_turns: 0,
            ..GuardResponse::default()
        };
        assert!(!should_replace_idle_daemon(&request, &response));
    }

    #[test]
    fn release_versions_compare_numerically() {
        assert!(version_is_older("0.1.9", "0.1.10"));
        assert!(!version_is_older("0.2.0", "0.1.10"));
        assert!(version_is_older("legacy", "0.1.10"));
    }

    #[test]
    fn release_requests_do_not_replace_the_daemon_that_processed_them() {
        let request = GuardRequest {
            action: "release".into(),
            ..GuardRequest::default()
        };
        let response = GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            daemon_version: Some("0.1.5".into()),
            active_turns: 0,
            ..GuardResponse::default()
        };
        assert!(!should_replace_idle_daemon(&request, &response));
    }
}
