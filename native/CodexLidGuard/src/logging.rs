use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use crate::{paths, win};

const MAX_LOG_BYTES: u64 = 1024 * 1024;
static LOG_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn write(message: impl AsRef<str>) {
    let file = LOG_FILE.get_or_init(|| Mutex::new(open_log_file()));
    let Ok(mut file) = file.lock() else {
        return;
    };
    if file.is_none() {
        *file = open_log_file();
    }
    let write_failed = match file.as_mut() {
        Some(handle) => {
            writeln!(handle, "{} {}", win::local_timestamp(), message.as_ref()).is_err()
        }
        None => return,
    };
    if write_failed {
        *file = None;
    }
}

fn open_log_file() -> Option<File> {
    fs::create_dir_all(paths::data_directory()).ok()?;
    let log = paths::log_file();
    if fs::metadata(&log).is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES) {
        let archived = paths::archived_log_file();
        let _ = fs::remove_file(&archived);
        let _ = fs::rename(&log, archived);
    }
    OpenOptions::new().create(true).append(true).open(log).ok()
}
