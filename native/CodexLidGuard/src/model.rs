use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 4;

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct HookPayload {
    #[serde(rename = "session_id")]
    pub session_id: Option<String>,
    #[serde(rename = "turn_id")]
    pub turn_id: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct GuardRequest {
    pub action: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GuardResponse {
    pub protocol_version: u32,
    pub daemon_path: Option<String>,
    pub daemon_version: Option<String>,
    pub ok: bool,
    pub message: String,
    pub active_turns: usize,
    pub is_guarding: bool,
    pub lid_state: String,
    pub sleep_pending: bool,
}

impl Default for GuardResponse {
    fn default() -> Self {
        Self {
            protocol_version: 0,
            daemon_path: None,
            daemon_version: None,
            ok: false,
            message: String::new(),
            active_turns: 0,
            is_guarding: false,
            lid_state: "unknown".to_string(),
            sleep_pending: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GuardSettings {
    pub alert_sounds: bool,
    pub sleep_when_lid_closed: bool,
    pub sleep_delay_seconds: u64,
}

impl Default for GuardSettings {
    fn default() -> Self {
        Self {
            alert_sounds: true,
            sleep_when_lid_closed: true,
            sleep_delay_seconds: 10,
        }
    }
}

impl GuardSettings {
    pub fn load() -> Self {
        match std::fs::read(crate::paths::settings_file()) {
            Ok(bytes) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(settings) => settings.clamp(),
                Err(cause) => {
                    crate::logging::write(format!(
                        "Could not read settings; using defaults. {cause}"
                    ));
                    Self::default()
                }
            },
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(cause) => {
                crate::logging::write(format!("Could not read settings; using defaults. {cause}"));
                Self::default()
            }
        }
    }

    pub fn clamp(mut self) -> Self {
        self.sleep_delay_seconds = self.sleep_delay_seconds.min(300);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LidState {
    #[default]
    Unknown,
    Open,
    Closed,
}

impl LidState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_defaulted_and_clamped() {
        let settings: GuardSettings = serde_json::from_str(r#"{"sleepDelaySeconds":999}"#).unwrap();
        let settings = settings.clamp();
        assert!(settings.alert_sounds);
        assert!(settings.sleep_when_lid_closed);
        assert_eq!(settings.sleep_delay_seconds, 300);
    }

    #[test]
    fn wire_names_match_the_existing_helper() {
        let request = GuardRequest {
            action: "acquire".into(),
            session_id: Some("s".into()),
            turn_id: Some("t".into()),
            cwd: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("sessionId"));
        assert!(json.contains("turnId"));

        let response = serde_json::to_string(&GuardResponse {
            protocol_version: PROTOCOL_VERSION,
            active_turns: 2,
            ..GuardResponse::default()
        })
        .unwrap();
        assert!(response.contains("activeTurns"));
        assert!(response.contains("protocolVersion"));
        assert!(response.contains("daemonVersion"));
    }
}
