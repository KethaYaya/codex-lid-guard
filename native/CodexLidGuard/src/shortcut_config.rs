//! Validated global shortcut preferences, parsed away from the keyboard hook.
use serde::Deserialize;

pub const COPILOT: u32 = 0x86;
pub const CTRL: u8 = 1;
pub const ALT: u8 = 2;
pub const SHIFT: u8 = 4;
pub const WIN: u8 = 8;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ShortcutSettings {
    pub enabled: bool,
    pub prefix: String,
    pub cycle_key: String,
    pub open_key: String,
    pub close_key: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self { enabled: true, prefix: "Copilot".into(), cycle_key: "Tab".into(),
            open_key: "Enter".into(), close_key: "Escape".into() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutConfig {
    pub enabled: bool,
    pub trigger: u32,
    pub modifiers: u8,
    pub prefix: String,
    pub cycle: u32,
    pub open: u32,
    pub close: u32,
}

impl Default for ShortcutConfig {
    fn default() -> Self { Self::parse(&ShortcutSettings::default()).unwrap() }
}

fn key(value: &str) -> Option<u32> {
    let value = value.trim().to_ascii_uppercase();
    Some(match value.as_str() {
        "TAB" => 0x09, "ENTER" => 0x0d, "ESCAPE" => 0x1b, "SPACE" => 0x20,
        "BACKSPACE" => 0x08, "DELETE" => 0x2e, "INSERT" => 0x2d,
        "HOME" => 0x24, "END" => 0x23, "PAGEUP" => 0x21, "PAGEDOWN" => 0x22,
        "LEFT" => 0x25, "UP" => 0x26, "RIGHT" => 0x27, "DOWN" => 0x28,
        _ => {
            let number: u32 = value.strip_prefix('F')?.parse().ok()?;
            if !(1..=24).contains(&number) { return None; }
            0x6f + number
        }
    })
}

impl ShortcutConfig {
    pub fn parse(settings: &ShortcutSettings) -> Option<Self> {
        let prefix = settings.prefix.trim();
        let (trigger, modifiers) = if prefix.eq_ignore_ascii_case("Copilot") {
            (COPILOT, WIN | SHIFT)
        } else {
            let parts: Vec<_> = prefix.split('+').map(str::trim).collect();
            let (last, modifiers) = parts.split_last()?;
            let mut flags = 0;
            for modifier in modifiers {
                let flag = match modifier.to_ascii_uppercase().as_str() {
                    "CTRL" => CTRL, "ALT" => ALT, "SHIFT" => SHIFT, "WIN" => WIN,
                    _ => return None,
                };
                if flags & flag != 0 { return None; }
                flags |= flag;
            }
            // Never turn ordinary typing or Shift+letter into a global prefix.
            if flags & (CTRL | ALT | WIN) == 0 { return None; }
            let trigger = key(last).or_else(|| {
                let bytes = last.as_bytes();
                (bytes.len() == 1 && bytes[0].is_ascii_alphanumeric())
                    .then(|| bytes[0].to_ascii_uppercase() as u32)
            })?;
            (trigger, flags)
        };
        let cycle = key(&settings.cycle_key)?;
        let open = key(&settings.open_key)?;
        let close = key(&settings.close_key)?;
        if cycle == open || cycle == close || open == close { return None; }
        Some(Self { enabled: settings.enabled, trigger, modifiers, prefix: prefix.into(), cycle, open, close })
    }

    pub fn from_settings(settings: &ShortcutSettings) -> Self {
        Self::parse(settings).unwrap_or_else(|| Self { enabled: false, ..Self::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custom_prefixes_and_rejects_ambiguous_or_plain_typing_bindings() {
        for prefix in ["Ctrl+Alt+Space", "alt+shift+g", "Win+F12", "Ctrl+1", "Copilot"] {
            assert!(ShortcutConfig::parse(&ShortcutSettings { prefix: prefix.into(), ..Default::default() }).is_some());
        }
        for prefix in ["", "D", "Shift+D", "Ctrl+Ctrl+D", "Ctrl+F25", "Ctrl++A", "Ctrl+Alt"] {
            assert!(!ShortcutConfig::from_settings(&ShortcutSettings { prefix: prefix.into(), ..Default::default() }).enabled);
        }
        assert!(!ShortcutConfig::from_settings(&ShortcutSettings { open_key: "Tab".into(), ..Default::default() }).enabled);
        assert!(!ShortcutConfig::from_settings(&ShortcutSettings { enabled: false, ..Default::default() }).enabled);
    }
}
