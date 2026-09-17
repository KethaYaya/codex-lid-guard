//! Codex app-server sessions owned by the native daemon, independent of any editor.
//! Prompts, output and approval details stay in memory (Codex owns its normal history).
use crate::model::{GuardRequest, GuardResponse, GuardSettings};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

const PREFIX: &str = "lidguard-background-";
const TEXT_LIMIT: usize = 128 * 1024;
const WIRE_LIMIT: u64 = 4 * 1024 * 1024;
type Guard = Arc<dyn Fn(GuardRequest) -> GuardResponse + Send + Sync>;
static MANAGER: OnceLock<Manager> = OnceLock::new();
static COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct Manager {
    tasks: Mutex<HashMap<String, Arc<Task>>>,
    guard: Guard,
    stopping: AtomicBool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Start {
    pub codex_path: String,
    pub cwd: String,
    pub prompt: String,
}

#[derive(Clone, Debug)]
pub struct PendingInput {
    pub id: Value,
    pub method: String,
    pub params: Value,
    pub details: String,
}

#[derive(Clone)]
pub struct View {
    pub title: String,
    pub cwd: String,
    pub status: String,
    pub history: String,
    pub latest: String,
    pub busy: bool,
    pub ready: bool,
    pub ended: bool,
    pub pending: Vec<PendingInput>,
    pub revision: u64,
    pub activity: u64,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub dismissed: bool,
    pub dock_request: u64,
}

pub struct Task {
    pub id: String,
    pub view: Mutex<View>,
    pub window: AtomicU64,
    pub opening: AtomicBool,
    commands: mpsc::Sender<Action>,
    done: AtomicBool,
    protected: AtomicBool,
}

#[derive(Debug)]
pub enum Action {
    Send(String),
    Interrupt,
    Answer { id: Value, result: Value },
    Shutdown,
}

pub fn initialize(guard: impl Fn(GuardRequest) -> GuardResponse + Send + Sync + 'static) {
    let _ = MANAGER.set(Manager {
        tasks: Mutex::new(HashMap::new()),
        guard: Arc::new(guard),
        stopping: AtomicBool::new(false),
    });
}

pub fn count() -> usize {
    COUNT.load(Ordering::Acquire)
}
pub fn is_task(id: &str) -> bool {
    id.starts_with(PREFIX)
}

pub fn owns_thread(id: &str) -> bool {
    MANAGER.get().is_some_and(|manager| {
        manager
            .tasks
            .lock()
            .unwrap()
            .values()
            .any(|task| task.view.lock().unwrap().thread_id.as_deref() == Some(id))
    })
}

pub fn start(input: Start, id: String) -> io::Result<String> {
    validate_start(&input)?;
    let manager = MANAGER
        .get()
        .ok_or_else(|| io::Error::other("Background worker is unavailable."))?;
    if manager.stopping.load(Ordering::Acquire) {
        return Err(io::Error::other("Lid Guard is quitting."));
    }
    let mut tasks = manager.tasks.lock().unwrap();
    if !is_task(&id)
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(io::Error::other("Invalid background session request ID."));
    }
    // A retried pipe request must not run the same prompt twice.
    if tasks.contains_key(&id) {
        return Ok(id);
    }
    if tasks.len() >= crate::overlay::SESSION_LIMIT {
        return Err(io::Error::other(
            "End an existing background session before starting another (maximum 10).",
        ));
    }
    let sequence = crate::overlay::next_activity();
    let (commands, receiver) = mpsc::channel();
    let title: String = input
        .prompt
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(72)
        .collect();
    let task = Arc::new(Task {
        id: id.clone(),
        commands,
        window: AtomicU64::new(0),
        opening: AtomicBool::new(false),
        done: AtomicBool::new(false),
        protected: AtomicBool::new(false),
        view: Mutex::new(View {
            title,
            cwd: input.cwd.clone(),
            status: "Starting Codex…".into(),
            history: String::new(),
            latest: "Starting Codex…".into(),
            busy: true,
            ready: false,
            ended: false,
            pending: vec![],
            revision: 1,
            activity: sequence,
            thread_id: None,
            turn_id: None,
            dismissed: false,
            dock_request: 1,
        }),
    });
    let owned = task.clone();
    let guard = manager.guard.clone();
    // Publish before starting: the overlay and lifecycle watcher must recognize ownership.
    tasks.insert(id.clone(), task.clone());
    COUNT.fetch_add(1, Ordering::Release);
    if let Err(error) = std::thread::Builder::new()
        .name("background-codex".into())
        .spawn(move || {
            let result = run_worker(&owned, input, receiver, &guard);
            if let Err(error) = result {
                owned.update(|view| {
                    view.status = format!("Codex stopped: {error}");
                    view.latest = view.status.clone();
                    append(&mut view.history, &format!("\n\n{}", view.status));
                });
            }
            // Child/job has been reaped before releasing protection, including error paths.
            release(&owned, &guard);
            owned.update(|view| {
                view.busy = false;
                view.ready = false;
                view.ended = true;
                view.pending.clear();
            });
            owned.done.store(true, Ordering::Release);
        })
    {
        tasks.remove(&id);
        COUNT.fetch_sub(1, Ordering::Release);
        return Err(error);
    }
    drop(tasks);
    show(&id);
    Ok(id)
}

fn validate_start(input: &Start) -> io::Result<()> {
    if !Path::new(&input.codex_path).is_absolute()
        || !Path::new(&input.codex_path).is_file()
        || !Path::new(&input.codex_path)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("codex.exe"))
    {
        return Err(io::Error::other(
            "The installed Codex runtime could not be found. Update the Codex extension first.",
        ));
    }
    if !Path::new(&input.cwd).is_absolute() || !Path::new(&input.cwd).is_dir() {
        return Err(io::Error::other("Choose an existing local project folder."));
    }
    if input.prompt.trim().is_empty() || input.prompt.len() > TEXT_LIMIT {
        return Err(io::Error::other("Enter a task of at most 128 KB."));
    }
    Ok(())
}

impl Task {
    pub fn snapshot(&self) -> View {
        self.view.lock().unwrap().clone()
    }
    fn update(&self, update: impl FnOnce(&mut View)) {
        let mut view = self.view.lock().unwrap();
        update(&mut view);
        view.revision += 1;
    }
    pub fn send(&self, action: Action) -> bool {
        self.commands.send(action).is_ok()
    }
    pub fn dock(&self) {
        self.update(|view| {
            view.dock_request += 1;
            view.dismissed = false;
        });
    }
}

pub fn show(id: &str) -> bool {
    let Some(manager) = MANAGER.get() else {
        return false;
    };
    let task = manager.tasks.lock().unwrap().get(id).cloned();
    let Some(task) = task else {
        return false;
    };
    let window = task.window.load(Ordering::Acquire);
    if window != 0 {
        return crate::win::show_background_window(window);
    }
    if !task.opening.swap(true, Ordering::AcqRel) {
        let failed = task.clone();
        if std::thread::Builder::new()
            .name("background-window".into())
            .spawn(move || {
                if let Err(error) = crate::win::run_background_window(task.clone()) {
                    task.update(|view| {
                        view.status = format!("Could not open task window: {error}");
                    });
                }
                task.opening.store(false, Ordering::Release);
            })
            .is_err()
        {
            failed.opening.store(false, Ordering::Release);
            return false;
        }
    }
    true
}

pub fn show_tasks() {
    let Some(manager) = MANAGER.get() else {
        return;
    };
    let mut tasks: Vec<_> = manager
        .tasks
        .lock()
        .unwrap()
        .values()
        .map(|task| (task.id.clone(), task.snapshot()))
        .collect();
    tasks.sort_by_key(|(_, view)| std::cmp::Reverse(view.activity));
    let items: Vec<_> = tasks
        .iter()
        .map(|(_, view)| format!("{} — {}", view.title, view.status))
        .collect();
    let items = if items.is_empty() {
        vec!["Start a Background Task from VS Code's Command Palette".into()]
    } else {
        items
    };
    if let Ok(Some(index)) = crate::win::show_notification_popup(
        "dark",
        "Background Codex sessions",
        &items,
        None,
        &[],
        &[],
    ) && let Some((id, _)) = tasks.get(index)
    {
        show(id);
    }
}

pub fn end(id: &str) {
    let Some(manager) = MANAGER.get() else {
        return;
    };
    let task = manager.tasks.lock().unwrap().get(id).cloned();
    if let Some(task) = task {
        task.send(Action::Shutdown);
        std::thread::spawn(move || {
            while !task.done.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(50));
            }
            if manager.tasks.lock().unwrap().remove(&task.id).is_some() {
                COUNT.fetch_sub(1, Ordering::Release);
            }
        });
    }
}

pub fn shutdown() {
    if let Some(manager) = MANAGER.get() {
        manager.stopping.store(true, Ordering::Release);
        let tasks: Vec<_> = manager.tasks.lock().unwrap().values().cloned().collect();
        for task in &tasks {
            task.send(Action::Shutdown);
        }
        for task in tasks {
            while !task.done.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

pub fn dismiss(id: &str, activity: u64) {
    if let Some(manager) = MANAGER.get()
        && let Some(task) = manager.tasks.lock().unwrap().get(id)
    {
        task.update(|view| {
            if view.activity == activity {
                view.dismissed = true;
            }
        });
    }
}

pub fn frames(settings: &GuardSettings) -> Vec<crate::overlay::Frame> {
    use crate::overlay::{Card, CardTarget, Frame};
    let Some(manager) = MANAGER.get() else {
        return vec![];
    };
    let mut frames: Vec<_> = manager
        .tasks
        .lock()
        .unwrap()
        .values()
        .filter_map(|task| {
            let view = task.snapshot();
            if view.dismissed {
                return None;
            }
            let window = task.window.load(Ordering::Acquire);
            let attention = !view.pending.is_empty() || !view.busy;
            Some(Frame {
                group: None,
                project_path: Some(view.cwd.clone()),
                needs_input: !view.pending.is_empty(),
                session_id: Some(task.id.clone()),
                activity: view.activity,
                cards: vec![Card {
                    id: view.activity,
                    label: format!("{} — {}", Path::new(&view.cwd).file_name()
                        .unwrap_or_default().to_string_lossy(), view.title),
                    text: if !view.pending.is_empty() {
                        "Needs your response. Open this tab to continue.".into()
                    } else {
                        view.latest.chars().take(2400).collect()
                    },
                    final_message: !view.busy,
                    attention,
                    target: Some(CardTarget {
                        window,
                        session_id: task.id.clone(),
                        project: None,
                    }),
                }],
                busy: view.busy,
                attention,
                window: (window != 0).then_some(window),
                opacity: settings.overlay_opacity,
                position: settings.overlay_position.clone(),
                max_tabs: settings.overlay_max_tabs,
                shortcuts: crate::shortcut_config::ShortcutConfig::from_settings(
                    &settings.overlay_shortcuts,
                ),
                close: false,
                dock_request: view.dock_request,
                hidden_in_focus: window != 0 && crate::win::background_window_visible(window),
            })
        })
        .collect();
    frames.sort_by_key(|frame| std::cmp::Reverse(frame.activity));
    frames
}

fn append(target: &mut String, value: &str) {
    target.push_str(value);
    if target.len() > TEXT_LIMIT {
        let mut boundary = target.len() - TEXT_LIMIT;
        while !target.is_char_boundary(boundary) {
            boundary += 1;
        }
        target.drain(..boundary);
    }
}

fn acquire(task: &Task, guard: &Guard) -> io::Result<()> {
    let response = guard(GuardRequest {
        action: "acquire".into(),
        session_id: Some(task.id.clone()),
        turn_id: Some("worker".into()),
        cwd: Some(task.snapshot().cwd),
        ..Default::default()
    });
    if response.ok {
        task.protected.store(true, Ordering::Release);
        Ok(())
    } else {
        Err(io::Error::other(response.message))
    }
}

fn release(task: &Task, guard: &Guard) {
    if !task.protected.swap(false, Ordering::AcqRel) {
        return;
    }
    // Idempotent session release does not play a spurious completion sound on errors/shutdown.
    guard(GuardRequest {
        action: "release-session".into(),
        session_id: Some(task.id.clone()),
        ..Default::default()
    });
}

struct Server {
    child: Child,
    input: mpsc::SyncSender<Value>,
    _job: crate::win::ChildJob,
    messages: mpsc::Receiver<io::Result<Value>>,
    next_id: u64,
    pending: HashMap<u64, (String, Instant)>,
}

impl Server {
    fn launch(executable: &str, cwd: &str) -> io::Result<Self> {
        let mut child = Command::new(executable)
            .args([
                "-c",
                "features.code_mode_host=true",
                "app-server",
                "--listen",
                "stdio://",
            ])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()?;
        let job = match crate::win::ChildJob::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, messages) = mpsc::sync_channel(32);
        let (input, writes) = mpsc::sync_channel::<Value>(16);
        let failures = send.clone();
        std::thread::spawn(move || {
            for value in writes {
                let written = (|| -> io::Result<()> {
                    serde_json::to_writer(&mut stdin, &value)?;
                    stdin.write_all(b"\n")?;
                    stdin.flush()
                })();
                if let Err(error) = written {
                    let _ = failures.send(Err(error));
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let read = reader.by_ref().take(WIRE_LIMIT + 1).read_line(&mut line);
                let result = match read {
                    Ok(0) => Err(io::Error::other("The Codex worker exited.")),
                    Ok(n) if n as u64 > WIRE_LIMIT => Err(io::Error::other(
                        "Codex returned an oversized protocol message.",
                    )),
                    Ok(_) => serde_json::from_str(&line).map_err(io::Error::other),
                    Err(error) => Err(error),
                };
                let failed = result.is_err();
                if send.send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            _job: job,
            messages,
            next_id: 0,
            pending: HashMap::new(),
        })
    }
    fn write(&mut self, value: Value) -> io::Result<()> {
        // A hung server must never block Stop/End session behind a full stdin pipe.
        self.input
            .try_send(value)
            .map_err(|_| io::Error::other("The Codex request channel is unavailable or full."))
    }
    fn call(&mut self, method: &str, params: Value) -> io::Result<()> {
        self.next_id += 1;
        self.pending
            .insert(self.next_id, (method.into(), Instant::now()));
        self.write(json!({"id":self.next_id,"method":method,"params":params}))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self._job.terminate();
        let _ = self.child.wait();
    }
}

fn run_worker(
    task: &Task,
    input: Start,
    commands: mpsc::Receiver<Action>,
    guard: &Guard,
) -> io::Result<()> {
    acquire(task, guard)?;
    let mut server = Server::launch(&input.codex_path, &input.cwd)?;
    server.call("initialize", json!({"clientInfo":{"name":"codex_lid_guard","title":"Codex Lid Guard","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}))?;
    let mut first_prompt = Some(input.prompt);
    let mut interrupted = false;
    let mut item_details = HashMap::<String, Value>::new();
    let mut agent_item = String::new();
    loop {
        // Descendants can inherit a pipe handle after the server exits. EOF alone
        // is not a reliable death signal; checking the owned process also reaps that tree.
        if let Some(status) = server.child.try_wait()? {
            return Err(io::Error::other(format!(
                "The Codex worker exited ({status})."
            )));
        }
        for action in commands.try_iter() {
            match action {
                Action::Shutdown => return Ok(()),
                Action::Send(prompt) => {
                    let view = task.snapshot();
                    if view.ready
                        && !view.busy
                        && !prompt.trim().is_empty()
                        && prompt.len() <= TEXT_LIMIT
                    {
                        acquire(task, guard)?;
                        interrupted = false;
                        agent_item.clear();
                        item_details.clear();
                        begin_turn(task, &mut server, prompt)?;
                    }
                }
                Action::Interrupt => {
                    if task.snapshot().busy {
                        interrupted = true;
                        if let Some(turn_id) = task.snapshot().turn_id {
                            server.call(
                                "turn/interrupt",
                                json!({"threadId":task.snapshot().thread_id,"turnId":turn_id}),
                            )?;
                        }
                    }
                }
                Action::Answer { id, result } => {
                    let view = task.snapshot();
                    // A stale click cannot approve a replacement request, even if its item matches.
                    if let Some(pending) = view.pending.iter().find(|pending| pending.id == id)
                        && valid_answer(pending, &result)
                    {
                        server.write(json!({"id":id,"result":result}))?;
                        task.update(|view| {
                            view.pending.retain(|pending| pending.id != id);
                            view.status = "Working…".into();
                        });
                    }
                }
            }
        }
        if server
            .pending
            .values()
            .any(|(_, at)| at.elapsed() > Duration::from_secs(60))
        {
            return Err(io::Error::other(
                "Codex did not acknowledge a request within 60 seconds. The task was stopped; it was not retried.",
            ));
        }
        let message = match server.messages.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => result?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err(io::Error::other("The Codex worker connection closed.")),
        };
        if let Some(method) = message["method"].as_str() {
            let params = &message["params"];
            if let Some(id) = message.get("id") {
                let view = task.snapshot();
                if params["threadId"].as_str() != view.thread_id.as_deref()
                    || !view.busy
                    || view.turn_id.as_deref().is_some_and(|turn| {
                        params["turnId"]
                            .as_str()
                            .is_some_and(|incoming| incoming != turn)
                    })
                {
                    server.write(json!({"id":id,"error":{"code":-32602,"message":"Request does not belong to the active background task."}}))?;
                    continue;
                }
                let item = item_details.get(params["itemId"].as_str().unwrap_or_default());
                let changes_available = method != "item/fileChange/requestApproval"
                    || item.is_some_and(|item| {
                        item["changes"]
                            .as_array()
                            .is_some_and(|changes| !changes.is_empty())
                    });
                if supported_request(method, params) && changes_available && view.pending.len() < 16
                {
                    let details = approval_details(method, params, item);
                    task.update(|view| {
                        view.pending.push(PendingInput {
                            id: id.clone(),
                            method: method.into(),
                            params: params.clone(),
                            details,
                        });
                        view.status = "Needs your response".into();
                    });
                    guard(GuardRequest {
                        action: "sound-request".into(),
                        session_id: Some(task.id.clone()),
                        ..Default::default()
                    });
                } else {
                    task.update(|view| { append(&mut view.history, &format!("\n\nLid Guard: Codex requested an interaction this window cannot display completely ({method}). No permission was granted.")); });
                    server.write(json!({"id":id,"error":{"code":-32601,"message":"This interaction is not supported by Lid Guard. No permission was granted."}}))?;
                }
                continue;
            }
            let current = task.snapshot();
            if params["threadId"].as_str() != current.thread_id.as_deref() {
                continue;
            }
            if method != "serverRequest/resolved"
                && method != "turn/started"
                && (current.turn_id.as_deref().is_some_and(|turn| {
                    params["turnId"]
                        .as_str()
                        .or_else(|| params["turn"]["id"].as_str())
                        .is_some_and(|incoming| incoming != turn)
                }) || !current.busy)
            {
                continue;
            }
            match method {
                "turn/started" => task.update(|view| {
                    view.turn_id = params["turn"]["id"].as_str().map(str::to_owned);
                }),
                "item/agentMessage/delta" => {
                    let item = params["itemId"].as_str().unwrap_or_default();
                    let delta = params["delta"].as_str().unwrap_or_default();
                    let new_item = agent_item != item;
                    agent_item = item.into();
                    task.update(|view| {
                        if new_item {
                            view.latest.clear();
                            append(&mut view.history, "\n\nCodex: ");
                        }
                        append(&mut view.latest, delta);
                        append(&mut view.history, delta);
                    });
                }
                "item/started" | "item/completed" => {
                    let item = &params["item"];
                    if let Some(id) = item["id"].as_str() {
                        match item["type"].as_str() {
                            Some("agentMessage") if method == "item/completed" => {
                                let text = item["text"].as_str().unwrap_or_default();
                                task.update(|view| {
                                    if agent_item != id {
                                        append(&mut view.history, &format!("\n\nCodex: {text}"));
                                    } else if view.history.ends_with(&view.latest) {
                                        view.history
                                            .truncate(view.history.len() - view.latest.len());
                                        append(&mut view.history, text);
                                    }
                                    view.latest.clear();
                                    append(&mut view.latest, text);
                                });
                                agent_item = id.into();
                            }
                            Some("commandExecution" | "fileChange") => {
                                if method == "item/completed" {
                                    item_details.remove(id);
                                } else if item_details.len() < 32 || item_details.contains_key(id) {
                                    item_details.insert(id.into(), item.clone());
                                }
                                if method == "item/started" {
                                    task.update(|view| {
                                        view.status = if item["type"] == "fileChange" {
                                            "Editing files…"
                                        } else {
                                            "Running a command…"
                                        }
                                        .into();
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "turn/completed" => {
                    let terminal = params["turn"]["status"].as_str().unwrap_or("failed");
                    task.update(|view| {
                        view.busy = false;
                        view.turn_id = None;
                        view.pending.clear();
                        view.status = match terminal {
                            "completed" => "Finished",
                            "interrupted" => "Stopped",
                            _ => "Failed",
                        }
                        .into();
                        if let Some(error) = params["turn"]["error"]["message"].as_str() {
                            view.latest = error.into();
                            append(&mut view.history, &format!("\n\nError: {error}"));
                        }
                    });
                    release(task, guard);
                    if terminal == "completed" {
                        guard(GuardRequest {
                            action: "sound-done".into(),
                            ..Default::default()
                        });
                    }
                }
                "serverRequest/resolved" => task.update(|view| {
                    view.pending
                        .retain(|pending| pending.id != params["requestId"]);
                }),
                _ => {}
            }
        } else if let Some(id) = message["id"].as_u64()
            && let Some((method, _)) = server.pending.remove(&id)
        {
            if let Some(error) = message.get("error") {
                return Err(io::Error::other(
                    error["message"]
                        .as_str()
                        .unwrap_or("Codex rejected the request."),
                ));
            }
            let result = &message["result"];
            match method.as_str() {
                "initialize" => {
                    server.write(json!({"method":"initialized"}))?;
                    server.call("account/read", json!({"refreshToken":false}))?;
                }
                "account/read" => {
                    if result["requiresOpenaiAuth"] == true && result["account"].is_null() {
                        return Err(io::Error::other(
                            "Sign in to Codex in VS Code, then start a new background task.",
                        ));
                    }
                    server.call("thread/start", json!({"cwd":input.cwd,"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":"workspace-write"}))?;
                }
                "thread/start" => {
                    let thread_id = result["thread"]["id"]
                        .as_str()
                        .ok_or_else(|| io::Error::other("Codex did not return a session ID."))?;
                    task.update(|view| {
                        view.thread_id = Some(thread_id.into());
                        view.ready = true;
                    });
                    if interrupted {
                        task.update(|view| {
                            view.busy = false;
                            view.status = "Stopped before starting".into();
                        });
                        release(task, guard);
                    } else {
                        begin_turn(task, &mut server, first_prompt.take().unwrap())?;
                    }
                }
                "turn/start" if task.snapshot().busy => {
                    let turn_id = result["turn"]["id"]
                        .as_str()
                        .ok_or_else(|| io::Error::other("Codex did not return a turn ID."))?;
                    task.update(|view| {
                        view.turn_id = Some(turn_id.into());
                    });
                    if interrupted {
                        server.call(
                            "turn/interrupt",
                            json!({"threadId":task.snapshot().thread_id,"turnId":turn_id}),
                        )?;
                    }
                }
                _ => {}
            }
        }
    }
}

fn begin_turn(task: &Task, server: &mut Server, prompt: String) -> io::Result<()> {
    task.update(|view| {
        view.busy = true;
        view.turn_id = None;
        view.pending.clear();
        view.dismissed = false;
        view.activity = crate::overlay::next_activity();
        view.dock_request += 1;
        view.latest = "Working…".into();
        view.status = "Working…".into();
        append(&mut view.history, &format!("\n\nYou: {prompt}"));
    });
    server.call(
        "turn/start",
        json!({"threadId":task.snapshot().thread_id,"input":[{"type":"text","text":prompt}]}),
    )
}

fn supported_request(method: &str, params: &Value) -> bool {
    matches!(
        method,
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval"
    ) || (method == "item/tool/requestUserInput"
        && params["questions"].as_array().is_some_and(|questions| {
            !questions.is_empty()
                && questions
                    .iter()
                    .all(|question| question["id"].is_string() && question["isSecret"] != true)
        }))
}

fn approval_details(method: &str, params: &Value, item: Option<&Value>) -> String {
    if method == "item/tool/requestUserInput" {
        return "Codex needs your answer. Enter one answer for each question below.".into();
    }
    let mut details = if method.contains("commandExecution") {
        "Allow this command once?"
    } else {
        "Allow these file changes once?"
    }
    .to_string();
    for (key, label) in [
        ("command", "Command"),
        ("cwd", "Working directory"),
        ("reason", "Reason"),
        ("grantRoot", "Requested folder access"),
    ] {
        if let Some(value) = params[key].as_str() {
            details.push_str(&format!("\n\n{label}: {value}"));
        }
    }
    if !params["networkApprovalContext"].is_null() {
        details.push_str(&format!(
            "\n\nNetwork access: {}",
            serde_json::to_string_pretty(&params["networkApprovalContext"]).unwrap_or_default()
        ));
    }
    if !params["kind"].is_null() && params["kind"] != "command" {
        details.push_str(&format!(
            "\n\nRequested action: {}",
            serde_json::to_string_pretty(params).unwrap_or_default()
        ));
    }
    if let Some(changes) = item.and_then(|item| item["changes"].as_array()) {
        for change in changes {
            details.push_str(&format!(
                "\n\nFile: {}\nChange: {}\n{}",
                change["path"].as_str().unwrap_or_default(),
                change["kind"],
                change["diff"].as_str().unwrap_or_default()
            ));
        }
    }
    details
}

pub fn valid_answer(pending: &PendingInput, result: &Value) -> bool {
    match pending.method.as_str() {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            let Some(decision) = result["decision"].as_str() else {
                return false;
            };
            if !matches!(decision, "accept" | "decline" | "cancel") {
                return false;
            }
            pending.params["availableDecisions"]
                .as_array()
                .is_none_or(|decisions| decisions.iter().any(|value| value == decision))
        }
        "item/tool/requestUserInput" => {
            pending.params["questions"]
                .as_array()
                .is_some_and(|questions| {
                    questions.iter().all(|question| {
                        question["id"].as_str().is_some_and(|id| {
                            result["answers"][id]["answers"]
                                .as_array()
                                .is_some_and(|answers| {
                                    !answers.is_empty() && answers.iter().all(Value::is_string)
                                })
                        })
                    })
                })
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "background_tests.rs"]
pub(crate) mod integration_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approvals_are_explicit_scoped_and_follow_available_decisions() {
        let pending = PendingInput {
            id: json!(7),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({"availableDecisions":["accept","cancel"]}),
            details: String::new(),
        };
        assert!(valid_answer(&pending, &json!({"decision":"accept"})));
        assert!(!valid_answer(
            &pending,
            &json!({"decision":"acceptForSession"})
        ));
        assert!(!valid_answer(&pending, &json!({"decision":"decline"})));
        assert!(!valid_answer(&pending, &json!({})));
    }
    #[test]
    fn every_question_requires_an_answer_and_secret_inputs_are_rejected() {
        let params = json!({"questions":[{"id":"one"},{"id":"two"}]});
        let pending = PendingInput {
            id: json!(1),
            method: "item/tool/requestUserInput".into(),
            params: params.clone(),
            details: String::new(),
        };
        assert!(supported_request(&pending.method, &params));
        assert!(!valid_answer(
            &pending,
            &json!({"answers":{"one":{"answers":["Yes"]}}})
        ));
        assert!(valid_answer(
            &pending,
            &json!({"answers":{"one":{"answers":["Yes"]},"two":{"answers":["No"]}}})
        ));
        assert!(!supported_request(
            &pending.method,
            &json!({"questions":[{"id":"password","isSecret":true}]})
        ));
        assert!(!supported_request(
            "account/chatgptAuthTokens/refresh",
            &json!({})
        ));
    }
    #[test]
    fn history_is_bounded_without_splitting_unicode() {
        let mut history = "é".repeat(TEXT_LIMIT);
        append(&mut history, "\nhello");
        assert!(history.len() <= TEXT_LIMIT);
        assert!(history.ends_with("\nhello"));
    }
}
