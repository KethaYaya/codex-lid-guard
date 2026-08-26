use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use std::os::windows::process::CommandExt;

use crate::logging;
use crate::model::GuardSettings;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SOUND_PATH_VARIABLE: &str = "CODEX_LID_GUARD_SOUND_PATH";
const PLAYER_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$Path = [Environment]::GetEnvironmentVariable('CODEX_LID_GUARD_SOUND_PATH', 'Process')
if ([string]::IsNullOrWhiteSpace($Path)) { throw 'CODEX_LID_GUARD_SOUND_PATH is not set' }
Add-Type -AssemblyName PresentationCore
Add-Type -AssemblyName WindowsBase
$resolved = (Resolve-Path -LiteralPath $Path).ProviderPath
$script:player = [System.Windows.Media.MediaPlayer]::new()
$script:frame = [System.Windows.Threading.DispatcherFrame]::new()
$script:timer = [System.Windows.Threading.DispatcherTimer]::new()
$script:timer.Interval = [TimeSpan]::FromSeconds(15)
$script:failed = $null
$script:timedOut = $false
$script:player.add_MediaOpened({ $script:player.Play() })
$script:player.add_MediaEnded({ $script:frame.Continue = $false })
$script:player.add_MediaFailed({
    param($sender, $eventArgs)
    $script:failed = $eventArgs.ErrorException
    $script:frame.Continue = $false
})
$script:timer.add_Tick({
    $script:timedOut = $true
    $script:frame.Continue = $false
})
try {
    $script:player.Open([Uri]::new($resolved))
    $script:timer.Start()
    [System.Windows.Threading.Dispatcher]::PushFrame($script:frame)
} finally {
    $script:timer.Stop()
    $script:player.Close()
}
if ($script:failed) { throw "sound media failed: $($script:failed.Message)" }
if ($script:timedOut) { throw 'sound playback timed out' }
"#;

#[derive(Clone, Copy, Debug)]
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
    let Ok(executable) = std::env::current_exe() else {
        logging::write("Could not locate the alert sound player executable.");
        return;
    };
    if let Err(cause) = Command::new(executable)
        .args(["play-sound", sound.label()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
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
    let child = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            PLAYER_SCRIPT,
        ])
        .env(SOUND_PATH_VARIABLE, &sound_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    let Ok(mut child) = child else {
        logging::write(format!(
            "Could not play {} alert sound: Windows did not start PowerShell.",
            sound.label()
        ));
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                logging::write(format!("Played {} alert sound.", sound.label()));
                return true;
            }
            Ok(Some(status)) => {
                logging::write(format!(
                    "Could not play {} alert sound: PowerShell exited with {status}.",
                    sound.label()
                ));
                return false;
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                logging::write(format!(
                    "Could not play {} alert sound: playback timed out.",
                    sound.label()
                ));
                return false;
            }
            Err(cause) => {
                logging::write(format!(
                    "Could not play {} alert sound: {cause}.",
                    sound.label()
                ));
                return false;
            }
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
