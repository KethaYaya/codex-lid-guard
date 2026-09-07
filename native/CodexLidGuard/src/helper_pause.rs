//! A tray quit persists until the user explicitly enables Lid Guard again.
use std::{io, path::Path};
use crate::{model::{GuardResponse, PROTOCOL_VERSION}, paths};

fn marker(directory: &Path) -> std::path::PathBuf { directory.join("helper-paused") }
pub fn paused() -> bool { marker(&paths::data_directory()).exists() }
pub fn pause() -> io::Result<()> { set_paused(&paths::data_directory(), true) }
pub fn resume() -> io::Result<()> { set_paused(&paths::data_directory(), false) }

fn set_paused(directory: &Path, paused: bool) -> io::Result<()> {
    if paused {
        std::fs::create_dir_all(directory)?;
        std::fs::write(marker(directory), b"Quit from the tray. Use Codex Lid Guard: Enable to restart.\n")
    } else {
        match std::fs::remove_file(marker(directory)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

pub fn response() -> GuardResponse {
    GuardResponse { protocol_version: PROTOCOL_VERSION, ok: true, helper_paused: true,
        daemon_version: Some(env!("CARGO_PKG_VERSION").into()),
        message: "Lid Guard was quit from the tray. Use Codex Lid Guard: Enable to restart.".into(),
        ..GuardResponse::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quit_stays_paused_until_explicit_resume_and_resume_is_idempotent() {
        let directory = std::env::temp_dir().join(format!("lid-guard-pause-{}", std::process::id()));
        set_paused(&directory, true).unwrap();
        assert!(marker(&directory).exists());
        let status = response();
        assert!(status.ok && status.helper_paused);
        assert!(!status.is_guarding && !status.sleep_pending && status.pipe_name.is_none());
        set_paused(&directory, false).unwrap();
        set_paused(&directory, false).unwrap();
        assert!(!marker(&directory).exists());
        std::fs::remove_dir(directory).unwrap();
    }
}
