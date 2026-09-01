use sha2::{Digest, Sha256};
use std::env;
use std::path::PathBuf;

use crate::win;

pub fn data_directory() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("CodexLidGuard")
}

pub fn settings_file() -> PathBuf {
    data_directory().join("settings.json")
}

pub fn recovery_file() -> PathBuf {
    data_directory().join("power-recovery.json")
}

pub fn log_file() -> PathBuf {
    data_directory().join("guard.log")
}

pub fn archived_log_file() -> PathBuf {
    data_directory().join("guard.log.1")
}

pub fn status_file() -> PathBuf {
    data_directory().join("status.json")
}

pub fn codex_data_directory() -> PathBuf {
    env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|value| PathBuf::from(value).join(".codex")))
        .unwrap_or_else(|| env::temp_dir().join(".codex"))
}

pub fn codex_logs_database() -> PathBuf {
    codex_data_directory().join("logs_2.sqlite")
}

pub fn codex_state_database() -> PathBuf {
    codex_data_directory().join("state_5.sqlite")
}

pub fn pipe_name() -> String {
    format!(r"\\.\pipe\CodexLidGuard.{}", session_key())
}

pub fn mutex_name() -> String {
    format!(r"Local\CodexLidGuard.{}", session_key())
}

fn session_key() -> String {
    let identity = win::current_user_sid()
        .unwrap_or_else(|| env::var("USERNAME").unwrap_or_else(|_| "unknown-user".to_string()));
    let stable = format!("{identity}:{}", win::current_session_id());
    let digest = Sha256::digest(stable.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}
