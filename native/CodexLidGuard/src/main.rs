#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(not(windows))]
compile_error!("Codex Lid Guard supports Windows only.");

mod client;
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
        "status" | "restore" => {
            let response = client::send(GuardRequest {
                action: command,
                ..GuardRequest::default()
            });
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
        origin_window,
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
        "Usage: CodexLidGuard [daemon | hook acquire | hook release | hook release-session | sound done | sound request | status | restore]"
    );
    std::process::exit(2);
}
