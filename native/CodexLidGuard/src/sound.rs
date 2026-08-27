use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use crate::logging;
use crate::model::GuardSettings;

const MCI_ERROR_BUFFER_LENGTH: usize = 256;
static SOUND_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[link(name = "winmm")]
unsafe extern "system" {
    fn mciSendStringW(
        command: *const u16,
        result: *mut u16,
        result_length: u32,
        callback: isize,
    ) -> u32;
    fn mciGetErrorStringW(error: u32, text: *mut u16, text_length: u32) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertSound {
    Done,
    Request,
}

impl AlertSound {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "done" => Some(Self::Done),
            "request" => Some(Self::Request),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Request => "request",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Done => "done.mp3",
            Self::Request => "request.mp3",
        }
    }
}

pub fn start(sound: AlertSound) {
    if let Err(cause) = thread::Builder::new()
        .name(format!("Codex Lid Guard {} alert", sound.label()))
        .spawn(move || {
            let _ = play_and_wait(sound);
        })
    {
        logging::write(format!("Could not start the alert sound player: {cause}"));
    }
}

pub fn play_and_wait(sound: AlertSound) -> bool {
    if !GuardSettings::load().alert_sounds {
        return true;
    }
    let sound_path = sound_path(sound);
    if !sound_path.exists() {
        logging::write(format!("Alert sound is missing: {}", sound_path.display()));
        return false;
    }
    let sequence = SOUND_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let alias = format!("CodexLidGuardSound{}_{}", std::process::id(), sequence);
    let open = format!(
        "open \"{}\" type mpegvideo alias {alias}",
        sound_path.to_string_lossy()
    );
    if let Err(cause) = mci(&open) {
        logging::write(format!(
            "Could not open {} alert sound: {cause}.",
            sound.label()
        ));
        return false;
    }
    let played = mci(&format!("play {alias} wait"));
    let _ = mci(&format!("close {alias}"));
    match played {
        Ok(()) => {
            logging::write(format!("Played {} alert sound.", sound.label()));
            true
        }
        Err(cause) => {
            logging::write(format!(
                "Could not play {} alert sound: {cause}.",
                sound.label()
            ));
            false
        }
    }
}

fn sound_path(sound: AlertSound) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .unwrap_or_default()
        .join("sounds")
        .join(sound.file_name())
}

fn mci(command: &str) -> Result<(), String> {
    let command: Vec<u16> = OsStr::new(command).encode_wide().chain(Some(0)).collect();
    let error = unsafe { mciSendStringW(command.as_ptr(), std::ptr::null_mut(), 0, 0) };
    if error == 0 {
        return Ok(());
    }
    let mut text = [0u16; MCI_ERROR_BUFFER_LENGTH];
    let described =
        unsafe { mciGetErrorStringW(error, text.as_mut_ptr(), MCI_ERROR_BUFFER_LENGTH as u32) }
            != 0;
    if described {
        let length = text
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(text.len());
        Err(String::from_utf16_lossy(&text[..length]))
    } else {
        Err(format!("Windows multimedia error {error}"))
    }
}
