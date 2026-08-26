use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use crate::{paths, win};

static LOG_GATE: OnceLock<Mutex<()>> = OnceLock::new();

pub fn write(message: impl AsRef<str>) {
    let _guard = LOG_GATE.get_or_init(|| Mutex::new(())).lock();
    let Ok(_guard) = _guard else {
        return;
    };
    let directory = paths::data_directory();
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::log_file())
    {
        let _ = writeln!(file, "{} {}", win::local_timestamp(), message.as_ref());
    }
}
