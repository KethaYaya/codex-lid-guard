//! Route overlay clicks to the extension host that owns the current workspace.
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use std::os::windows::process::CommandExt;

use serde::Deserialize;
use crate::{paths, win};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowContext {
    version: u32,
    window: u64,
    pid: u32,
    pipe: String,
    workspace_folders: Vec<String>,
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    workspace_file: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub cwd: String,
    pub path: String,
    pub executable: String,
}

pub(crate) fn normalized_path(value: &str) -> String {
    let value = value.strip_prefix(r"\\?\").unwrap_or(value).replace('/', "\\");
    let mut normalized = std::path::PathBuf::new();
    for component in Path::new(&value).components() {
        if component == std::path::Component::ParentDir { normalized.pop(); }
        else if component != std::path::Component::CurDir { normalized.push(component); }
    }
    normalized.to_string_lossy().trim_end_matches('\\').to_lowercase()
}

pub fn belongs_to_workspace(cwd: &str, roots: &[String]) -> bool {
    let cwd = normalized_path(cwd);
    !cwd.is_empty() && roots.iter().any(|root| {
        let root = normalized_path(root);
        !root.is_empty() && (cwd == root || cwd.starts_with(&format!("{root}\\")))
    })
}

pub fn valid_session_id(id: &str) -> bool {
    id.len() == 36 && id.bytes().enumerate().all(|(index, byte)| {
        if [8, 13, 18, 23].contains(&index) { byte == b'-' } else { byte.is_ascii_hexdigit() }
    })
}

impl WindowContext {
    pub fn keeps_preview(&self, cwd: Option<&str>) -> bool {
        self.keeps_preview_when(cwd, win::is_process_running(self.pid) && win::is_editor_window(self.window))
    }

    fn keeps_preview_when(&self, cwd: Option<&str>, connected: bool) -> bool {
        // Closing the folder/window is different from replacing its project.
        !connected || self.workspace_folders.is_empty()
            || cwd.is_some_and(|cwd| belongs_to_workspace(cwd, &self.workspace_folders))
    }

    pub fn project(&self, cwd: Option<&str>) -> Option<Project> {
        let cwd = cwd?;
        let root = self.workspace_folders.iter()
            .filter(|root| belongs_to_workspace(cwd, std::slice::from_ref(root)))
            .max_by_key(|root| root.len())?;
        let executable = self.executable.as_ref()?;
        let name = Path::new(executable).file_name()?.to_str()?.to_ascii_lowercase();
        if !Path::new(executable).is_absolute() || !matches!(name.as_str(), "code.exe" | "code - insiders.exe") {
            return None;
        }
        Some(Project { cwd: cwd.into(), path: self.workspace_file.clone().unwrap_or_else(|| root.clone()),
            executable: executable.clone() })
    }

    fn valid(&self, window: u64) -> bool {
        self.version == 1 && self.window == window && self.pid != 0
            && self.pipe.strip_prefix(r"\\.\pipe\CodexLidGuard.Navigation.")
                .is_some_and(valid_session_id)
    }

    pub fn allows(&self, cwd: Option<&str>) -> bool {
        win::is_process_running(self.pid)
            && cwd.is_some_and(|cwd| belongs_to_workspace(cwd, &self.workspace_folders))
    }
}

fn read_context(directory: &Path, window: u64) -> Option<WindowContext> {
    let file = directory.join(format!("{window}.json"));
    let inactive = || WindowContext { version: 1, window, pid: 0, pipe: String::new(), workspace_folders: vec![], executable: None, workspace_file: None };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return Some(inactive()),
    };
    if metadata.len() > 64 * 1024 { return Some(inactive()); }
    Some(std::fs::read(file).ok()
        .and_then(|bytes| serde_json::from_slice::<WindowContext>(&bytes).ok())
        .filter(|context| context.valid(window)).unwrap_or_else(inactive))
}

pub fn matching_window(project: &Project, preferred: u64) -> Option<u64> {
    let directory = paths::data_directory().join("windows");
    let mut matches: Vec<_> = std::fs::read_dir(&directory).ok()?.flatten()
        .filter_map(|entry| entry.path().file_stem()?.to_str()?.parse::<u64>().ok())
        .filter(|window| win::is_editor_window(*window))
        .filter(|window| read_context(&directory, *window).is_some_and(|context|
            context.allows(Some(&project.cwd)) && context.project(Some(&project.cwd)).is_some_and(|other|
                normalized_path(&other.path) == normalized_path(&project.path)
                    && normalized_path(&other.executable) == normalized_path(&project.executable))))
        .collect();
    matches.sort_unstable_by_key(|window| (*window != preferred, *window));
    matches.first().copied()
}

// Called only by an explicit open action, on the activation worker. The saved
// project is passed as one OS argument, never as shell text or a chat-derived URI.
pub fn resolve_window(target: &crate::overlay::CardTarget, deadline: Instant) -> Option<u64> {
    let Some(project) = &target.project else { return Some(target.window); };
    if !valid_session_id(&target.session_id) { return None; }
    // Two tabs from a closed project can be opened almost simultaneously.
    // Serialize launch/discovery so the second click reuses the first window.
    static LAUNCH: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _launch = LAUNCH.lock().ok()?;
    let context = read_context(&paths::data_directory().join("windows"), target.window);
    if context.as_ref().is_some_and(|context| !context.keeps_preview(Some(&project.cwd))) {
        return None; // A queued click must not reopen a folder that was replaced.
    }
    if let Some(window) = matching_window(project, target.window) { return Some(window); }
    if Instant::now() >= deadline { return None; }
    if !Path::new(&project.path).exists() || !Path::new(&project.executable).is_file() { return None; }
    // Use a fresh window so another open project is never replaced by a reopen.
    let mut child = std::process::Command::new(&project.executable)
        .arg("--new-window").arg(&project.path)
        .env_remove("ELECTRON_RUN_AS_NODE").env_remove("VSCODE_IPC_HOOK_CLI").env_remove("VSCODE_CLI")
        .creation_flags(0x0800_0000).spawn().ok()?;
    // Reap the launcher without blocking navigation or retaining a process handle.
    std::thread::spawn(move || { let _ = child.wait(); });
    while Instant::now() < deadline {
        if let Some(window) = matching_window(project, target.window) { return Some(window); }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

pub fn contexts(windows: impl Iterator<Item = u64>) -> HashMap<u64, WindowContext> {
    let directory = paths::data_directory().join("windows");
    let mut contexts = HashMap::new();
    for window in windows {
        if let std::collections::hash_map::Entry::Vacant(entry) = contexts.entry(window)
            && let Some(context) = read_context(&directory, window) {
            entry.insert(context);
        }
    }
    contexts
}

// None means this window has not loaded the bridge yet (legacy URI fallback).
// A stale or rejecting bridge must never fall through to another workspace.
pub fn dispatch(window: u64, session_id: &str, cwd: Option<&str>) -> Option<bool> {
    dispatch_from(&paths::data_directory().join("windows"), window, session_id, cwd)
}

pub fn new_chat_runtime(target: &crate::overlay::CardTarget) -> Result<String, String> {
    let project = target.project.as_ref().ok_or("Open this project in VS Code before starting a chat.")?;
    let window = matching_window(project, target.window).ok_or("Open this project in VS Code before starting a chat.")?;
    let context = read_context(&paths::data_directory().join("windows"), window)
        .filter(|context| context.allows(Some(&project.cwd))).ok_or("The chat's project is no longer available.")?;
    let connection = win::connect_pipe(&context.pipe, Duration::from_millis(200), Duration::from_secs(10))
        .map_err(|_| "Reload this project's VS Code window to enable new overlay chats.")?;
    connection.write_line(&serde_json::json!({"action":"prepare-new-chat", "cwd":project.cwd}).to_string())
        .map_err(|_| "Could not prepare the new chat. Try again.")?;
    let response: serde_json::Value = serde_json::from_str(&connection.read_line()
        .map_err(|_| "Could not prepare the new chat. Try again.")?).map_err(|_| "Invalid new chat response.")?;
    if response["accepted"] == true && let Some(path) = response["codexPath"].as_str() { Ok(path.into()) }
    else { Err(response["error"].as_str().unwrap_or("Reload this project's VS Code window to enable new overlay chats.").chars().take(180).collect()) }
}

pub fn send_reply(target: &crate::overlay::CardTarget, text: &str, busy: bool) -> Result<(), String> {
    let project = target.project.as_ref().ok_or("Open this chat in VS Code once, then try again.")?;
    let window = matching_window(project, target.window).ok_or("Open this project in VS Code before sending.")?;
    let context = read_context(&paths::data_directory().join("windows"), window)
        .filter(|context| context.allows(Some(&project.cwd)))
        .ok_or("The chat's project is no longer available.")?;
    if !valid_session_id(&target.session_id) || text.trim().is_empty() || text.encode_utf16().count() > 8192 {
        return Err("Enter a message of at most 8,192 characters.".into());
    }
    let connection = win::connect_pipe(&context.pipe, Duration::from_millis(200), Duration::from_secs(100))
        .map_err(|_| "Reload this project's VS Code window, then try again.")?;
    connection.write_line(&serde_json::json!({ "action": "send", "sessionId": target.session_id,
        "cwd": project.cwd, "text": text, "busy": busy }).to_string())
        .map_err(|_| "Send not confirmed. Check the chat before retrying.")?;
    let response: serde_json::Value = serde_json::from_str(&connection.read_line()
        .map_err(|_| "Send not confirmed. Check the chat before retrying.")?)
        .map_err(|_| "Send not confirmed. Check the chat before retrying.")?;
    if response["accepted"] == true { Ok(()) }
    else { Err(response["error"].as_str().unwrap_or("Reload the project's VS Code window to enable overlay replies.").chars().take(180).collect()) }
}

fn dispatch_from(directory: &Path, window: u64, session_id: &str, cwd: Option<&str>) -> Option<bool> {
    let context = read_context(directory, window)?;
    if !valid_session_id(session_id) || !context.allows(cwd) { return Some(false); }
    Some(send_request(&context, serde_json::json!({ "sessionId": session_id, "cwd": cwd }),
        Duration::from_secs(1)))
}

fn send_request(context: &WindowContext, request: serde_json::Value, timeout: Duration) -> bool {
    let result = (|| {
        let connection = win::connect_pipe(&context.pipe, Duration::from_millis(100), timeout).ok()?;
        connection.write_line(&request.to_string()).ok()?;
        let response: serde_json::Value = serde_json::from_str(&connection.read_line().ok()?).ok()?;
        response.get("accepted")?.as_bool()
    })();
    result.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> WindowContext {
        WindowContext { version: 1, window: 10, pid: std::process::id(),
            pipe: r"\\.\pipe\CodexLidGuard.Navigation.11111111-1111-1111-1111-111111111111".into(),
            workspace_folders: vec![r"C:\Projects\One".into()],
            executable: Some(r"C:\Program Files\Microsoft VS Code\Code.exe".into()), workspace_file: None }
    }

    #[test]
    fn close_folder_and_exit_preserve_tabs_but_replacing_a_live_project_removes_them() {
        let mut context = context();
        let cwd = Some(r"C:\Projects\One\src");
        assert!(context.keeps_preview_when(cwd, true));
        context.workspace_folders.clear();
        assert!(context.keeps_preview_when(cwd, true), "Close Folder leaves a launchable tab");
        assert!(!context.allows(cwd), "empty windows cannot receive direct chat navigation");
        context.workspace_folders = vec![r"C:\Projects\Other".into()];
        assert!(!context.keeps_preview_when(cwd, true), "Open Folder replaces the old tabs");
        assert!(context.keeps_preview_when(cwd, false), "a disconnected record cannot invalidate previews");
    }

    #[test]
    fn reopening_remembers_the_project_root_or_saved_workspace_instead_of_a_session_subdirectory() {
        let mut context = context();
        let project = context.project(Some(r"C:\Projects\One\src")).unwrap();
        assert_eq!(project.path, r"C:\Projects\One");
        assert_eq!(project.cwd, r"C:\Projects\One\src");
        context.workspace_file = Some(r"D:\Saved projects\Team.code-workspace".into());
        assert_eq!(context.project(Some(&project.cwd)).unwrap().path, context.workspace_file.as_deref().unwrap());
        assert!(context.project(Some(r"C:\Projects\One-other")).is_none());
        context.executable = Some(r"C:\Windows\System32\cmd.exe".into());
        assert!(context.project(Some(&project.cwd)).is_none());
    }

    #[test]
    fn workspace_matching_handles_windows_paths_and_sibling_folders() {
        let roots = vec![r"C:\Projects\One".into(), r"D:\Two\".into()];
        assert!(belongs_to_workspace(r"\\?\c:\projects\ONE", &roots));
        assert!(belongs_to_workspace("C:/Projects/One/src", &roots));
        assert!(belongs_to_workspace(r"D:\Two", &roots));
        assert!(!belongs_to_workspace(r"C:\Projects\One-old", &roots));
        assert!(!belongs_to_workspace(r"C:\Projects\One\..\Other", &roots));
        assert!(!belongs_to_workspace(r"C:\Projects\Other", &roots));
        assert!(!belongs_to_workspace("", &roots));
        assert!(!belongs_to_workspace(r"C:\Projects\One", &[]));
    }

    #[test]
    fn context_rejects_arbitrary_pipes_and_wrong_windows() {
        let mut context = WindowContext {
            version: 1, window: 10, pid: std::process::id(),
            pipe: r"\\.\pipe\CodexLidGuard.Navigation.11111111-1111-1111-1111-111111111111".into(),
            workspace_folders: vec![r"C:\One".into()],
            executable: None, workspace_file: None,
        };
        assert!(context.valid(10));
        assert!(!context.valid(11));
        assert!(context.allows(Some(r"C:\One")));
        assert!(!context.allows(Some(r"C:\Two")));
        context.pipe = r"\\remote\pipe\other".into();
        assert!(!context.valid(10));
        assert!(!valid_session_id("../other"));
    }

    #[test]
    fn malformed_context_never_falls_back_to_an_external_link() {
        let directory = std::env::temp_dir().join(format!("lid-guard-context-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("10.json"), b"invalid").unwrap();
        assert!(read_context(&directory, 11).is_none(), "only unregistered windows use the legacy route");
        assert_eq!(dispatch_from(&directory, 10, "11111111-1111-1111-1111-111111111111", Some(r"C:\One")), Some(false));
        std::fs::remove_file(directory.join("10.json")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
