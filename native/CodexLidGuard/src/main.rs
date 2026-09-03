#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(not(windows))]
compile_error!("Codex Lid Guard supports Windows only.");

mod client;
mod codex_lifecycle;
mod codex_log;
mod codex_transcript;
mod daemon;
mod logging;
mod model;
mod paths;
mod sound;
mod win;

use std::io::Read;

use model::{GuardRequest, GuardResponse, HookPayload, PROTOCOL_VERSION};
use sound::AlertSound;

fn main() {
    let result = run();
    if let Err(cause) = result {
        logging::write(format!("Fatal command failure: {cause}"));
        eprintln!("{cause}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments
        .first()
        .map(String::as_str)
        .unwrap_or("status")
        .to_ascii_lowercase();
    match command.as_str() {
        "daemon" => daemon::run()?,
        "hook" => run_hook(arguments.get(1).map(String::as_str).unwrap_or_default()),
        "sound" => run_sound(
            arguments.get(1).map(String::as_str).unwrap_or_default(),
            true,
        )?,
        "play-sound" => run_sound(
            arguments.get(1).map(String::as_str).unwrap_or_default(),
            false,
        )?,
        "menu" => {
            let mut theme = "dark";
            let mut initial_index = None;
            let mut active_indices = Vec::new();
            let mut title_index = 1;
            while let Some(argument) = arguments.get(title_index) {
                if let Some(value) = argument.strip_prefix("--theme=") {
                    theme = value;
                } else if let Some(value) = argument.strip_prefix("--selected=") {
                    initial_index = value.parse::<usize>().ok();
                } else if let Some(value) = argument.strip_prefix("--active=") {
                    active_indices = value
                        .split(',')
                        .filter_map(|index| index.parse::<usize>().ok())
                        .collect();
                } else {
                    break;
                }
                title_index += 1;
            }
            let title = arguments
                .get(title_index)
                .map(String::as_str)
                .unwrap_or("Codex awake");
            let items = arguments.get(title_index + 1..).unwrap_or_default();
            let selected =
                win::show_notification_popup(theme, title, items, initial_index, &active_indices)?;
            println!("{}", serde_json::json!({ "selectedIndex": selected }));
        }
        "recent-sessions" => {
            let limit = arguments
                .get(1)
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(5)
                .min(20);
            println!(
                "{}",
                serde_json::to_string(&codex_lifecycle::recent_sessions(limit))?
            );
        }
        "status" | "status-with-recent" | "restore" | "focus" => {
            let include_recent = command == "status-with-recent";
            let daemon_action = if include_recent {
                "status".to_string()
            } else {
                command
            };
            let mut response = client::send(GuardRequest {
                action: daemon_action,
                session_id: arguments.get(1).cloned(),
                turn_id: arguments.get(2).cloned(),
                ..GuardRequest::default()
            });
            response.pipe_name = Some(paths::pipe_name());
            if include_recent {
                // Enrich only interactive/helper status reads. The daemon's
                // acquisition snapshots remain entirely free of history I/O.
                response.recent_items = codex_lifecycle::recent_sessions(5);
            }
            println!("{}", serde_json::to_string_pretty(&response)?);
            if !response.ok {
                std::process::exit(1);
            }
        }
        "associate-window" => {
            let mut response = client::send(GuardRequest {
                action: command,
                session_id: arguments.get(1).cloned(),
                origin_window: win::foreground_editor_window(),
                origin_window_authoritative: true,
                ..GuardRequest::default()
            });
            response.pipe_name = Some(paths::pipe_name());
            println!("{}", serde_json::to_string_pretty(&response)?);
            if !response.ok {
                std::process::exit(1);
            }
        }
        "pre-acquire" => {
            let mut response = client::send(GuardRequest {
                action: command,
                session_id: arguments.get(1).cloned(),
                turn_id: arguments.get(2).cloned(),
                cwd: arguments.get(3).cloned(),
                origin_window: win::foreground_editor_window(),
                origin_window_authoritative: true,
                ..GuardRequest::default()
            });
            response.pipe_name = Some(paths::pipe_name());
            println!("{}", serde_json::to_string_pretty(&response)?);
            if !response.ok {
                std::process::exit(1);
            }
        }
        _ => usage(),
    }
    Ok(())
}

fn run_hook(action: &str) {
    let origin_window = action
        .eq_ignore_ascii_case("acquire")
        .then(win::foreground_editor_window)
        .flatten();
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let payload = match serde_json::from_str::<HookPayload>(&input) {
        Ok(payload) => payload,
        Err(cause) => {
            logging::write(format!("Hook input was not valid JSON: {cause}"));
            HookPayload::default()
        }
    };
    let response = client::send(GuardRequest {
        action: action.to_string(),
        client_version: None,
        session_id: payload.session_id,
        turn_id: payload.turn_id,
        cwd: payload.cwd,
        transcript_path: payload.transcript_path,
        origin_window,
        origin_window_authoritative: true,
    });
    if !response.ok {
        logging::write(format!(
            "Hook '{action}' could not update the guardian: {}",
            response.message
        ));
    }
    println!("{{\"continue\":true}}");
}

fn run_sound(value: &str, write_response: bool) -> Result<(), Box<dyn std::error::Error>> {
    let Some(sound) = AlertSound::parse(value) else {
        usage();
        return Ok(());
    };
    let played = sound::play_and_wait(sound);
    if write_response {
        let label = match sound {
            AlertSound::Done => "completion",
            AlertSound::Request => "needs-response",
        };
        let response = GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            daemon_path: std::env::current_exe()
                .ok()
                .map(|path| path.to_string_lossy().into_owned()),
            pipe_name: Some(paths::pipe_name()),
            ok: played,
            message: if played {
                format!("Played the {label} alert.")
            } else {
                format!("Could not play the {label} alert.")
            },
            ..GuardResponse::default()
        };
        println!("{}", serde_json::to_string_pretty(&response)?);
    }
    if !played {
        std::process::exit(1);
    }
    Ok(())
}

fn usage() {
    eprintln!(
        "Usage: CodexLidGuard [daemon | hook acquire | hook release | hook release-session | pre-acquire <session-id> <pending-turn-id> [cwd] | associate-window <session-id> | recent-sessions [limit] | sound done | sound request | status | status-with-recent | restore | focus <session-id> <turn-id> | menu [--theme=<theme>] [--selected=<index>] [--active=<indices>] <title> <items...>]"
    );
    std::process::exit(2);
}
