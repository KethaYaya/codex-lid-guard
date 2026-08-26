use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use std::os::windows::process::CommandExt;

use crate::logging;
use crate::model::{GuardRequest, GuardResponse, PROTOCOL_VERSION};
use crate::{paths, win};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn send(request: GuardRequest) -> GuardResponse {
    let mut response = try_send(&request, Duration::from_millis(150));
    if response.as_ref().is_some_and(|value| !is_compatible(value)) {
        retire_legacy_daemon();
        response = None;
    }
    if let Some(response) = response {
        return response;
    }
    if let Err(cause) = start_daemon() {
        return failure(format!("Could not start the guardian daemon: {cause}"));
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
        if let Some(response) = try_send(&request, Duration::from_millis(400)) {
            if is_compatible(&response) {
                return response;
            }
        }
    }
    failure("Could not start the guardian daemon.")
}

fn try_send(request: &GuardRequest, timeout: Duration) -> Option<GuardResponse> {
    let connection = win::connect_pipe(&paths::pipe_name(), timeout).ok()?;
    connection
        .write_line(&serde_json::to_string(request).ok()?)
        .ok()?;
    serde_json::from_str(&connection.read_line().ok()?).ok()
}

fn start_daemon() -> std::io::Result<()> {
    let executable = std::env::current_exe()?;
    Command::new(executable)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
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
    thread::sleep(Duration::from_millis(100));
}

fn is_compatible(response: &GuardResponse) -> bool {
    if response.protocol_version != PROTOCOL_VERSION {
        return false;
    }
    let Some(daemon_path) = response.daemon_path.as_deref() else {
        return false;
    };
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    equivalent_path(Path::new(daemon_path), &current)
}

fn equivalent_path(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn failure(message: impl Into<String>) -> GuardResponse {
    GuardResponse {
        protocol_version: PROTOCOL_VERSION,
        daemon_path: std::env::current_exe()
            .ok()
            .map(|value| value.to_string_lossy().into_owned()),
        message: message.into(),
        ..GuardResponse::default()
    }
}
