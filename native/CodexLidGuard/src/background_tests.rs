use super::*;
use std::path::PathBuf;
use std::thread;

fn fixture() -> PathBuf {
    static EXECUTABLE: OnceLock<PathBuf> = OnceLock::new();
    EXECUTABLE
        .get_or_init(|| {
            let directory = std::env::temp_dir().join(format!(
                "lidguard-background-fixture-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&directory).unwrap();
            let executable = directory.join("codex.exe");
            let output = Command::new("rustc")
                .args(["--edition=2024", "--crate-name=lidguard_fixture"])
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/app_server.rs"))
                .arg("-o")
                .arg(&executable)
                .creation_flags(0x0800_0000)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            executable
        })
        .clone()
}

pub(crate) fn task_for_test() -> (Arc<Task>, mpsc::Receiver<Action>) {
    let (commands, receiver) = mpsc::channel();
    (
        Arc::new(Task {
            id: format!(
                "lidguard-background-test-{}",
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            commands,
            window: AtomicU64::new(0),
            opening: AtomicBool::new(false),
            done: AtomicBool::new(false),
            protected: AtomicBool::new(false),
            codex_path: String::new(),
            project: None,
            view: Mutex::new(View {
                title: "Fixture background task".into(),
                cwd: std::env::temp_dir().to_string_lossy().into(),
                status: "Starting".into(),
                history: String::new(),
                messages: Vec::new(),
                latest: "Working…".into(),
                busy: true,
                ready: false,
                ended: false,
                pending: vec![],
                revision: 1,
                activity: 1,
                thread_id: None,
                turn_id: None,
                dismissed: false,
                dock_request: 1,
            }),
        }),
        receiver,
    )
}

pub(crate) fn start_overlay_fixture(cwd: &Path) -> String {
    initialize(|_| GuardResponse { ok: true, ..Default::default() });
    let id = format!("lidguard-background-{}-source", std::process::id());
    start_session(Start { codex_path: fixture().to_string_lossy().into(), cwd: cwd.to_string_lossy().into(), prompt: String::new() },
        id, true, Some(crate::session_navigation::Project { cwd: cwd.to_string_lossy().into(),
            path: cwd.to_string_lossy().into(), executable: cwd.join("Code.exe").to_string_lossy().into() })).unwrap()
}

pub(crate) fn set_overlay_fixture_request(id: &str, pending: PendingInput) {
    let task = MANAGER.get().unwrap().tasks.lock().unwrap().get(id).unwrap().clone();
    task.update(|view| { view.pending = vec![pending]; view.thread_id = Some("11111111-1111-1111-1111-111111111111".into()); });
}

pub(crate) fn set_overlay_fixture_reply(id: &str, text: &str) {
    let task = MANAGER.get().unwrap().tasks.lock().unwrap().get(id).unwrap().clone();
    let mut view = task.view.lock().unwrap();
    view.latest = text.into();
    view.messages = vec![ChatMessage::new(Role::Assistant, text)];
    view.revision += 1;
}

#[test]
fn structured_messages_keep_streamed_replies_left_of_interleaved_followups() {
    let (task, _) = task_for_test();
    let mut view = task.snapshot();
    view.push_message(Role::User, "Start".into());
    view.latest = "First part".into(); view.assistant_message("reply-1");
    view.push_message(Role::User, "A follow-up".into());
    view.latest = "First part, now finished".into(); view.assistant_message("reply-1");
    assert_eq!(view.messages.len(), 3);
    assert_eq!(view.messages[1].role, Role::Assistant);
    assert_eq!(view.messages[1].text, "First part, now finished");
    assert_eq!(view.messages[2], ChatMessage::new(Role::User, "A follow-up"));
    view.latest = "A new answer".into(); view.assistant_message("reply-2");
    assert_eq!(view.messages.len(), 4);
    view.push_message(Role::User, "🌍".repeat(TEXT_LIMIT));
    assert_eq!(view.messages.len(), 1);
    assert!(view.messages[0].text.len() <= TEXT_LIMIT);
    assert!(view.messages[0].text.ends_with('🌍'));
}

struct Running {
    task: Arc<Task>,
    thread: Option<thread::JoinHandle<io::Result<()>>>,
    guards: Arc<AtomicUsize>,
    cwd: PathBuf,
}
impl Running {
    fn start(executable: PathBuf, prompt: &str) -> Self {
        let (task, commands) = task_for_test();
        let cwd = std::env::temp_dir().join(&task.id);
        std::fs::create_dir_all(&cwd).unwrap();
        let guards = Arc::new(AtomicUsize::new(0));
        let count = guards.clone();
        let guard: Guard = Arc::new(move |request| {
            match request.action.as_str() {
                "acquire" => {
                    count.fetch_add(1, Ordering::SeqCst);
                }
                "release-session" => {
                    assert_eq!(count.fetch_sub(1, Ordering::SeqCst), 1);
                }
                _ => {}
            }
            GuardResponse {
                ok: true,
                ..Default::default()
            }
        });
        let input = Start {
            codex_path: executable.to_string_lossy().into(),
            cwd: cwd.to_string_lossy().into(),
            prompt: prompt.into(),
        };
        task.update(|view| view.cwd = input.cwd.clone());
        let owned = task.clone();
        let thread = thread::spawn(move || {
            let result = run_worker(&owned, input, commands, &guard);
            release(&owned, &guard);
            owned.update(|view| {
                view.busy = false;
                view.ended = true;
            });
            result
        });
        Self {
            task,
            thread: Some(thread),
            guards,
            cwd,
        }
    }
    fn until(&self, condition: impl Fn(&View) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !condition(&self.task.snapshot()) {
            assert!(
                Instant::now() < deadline,
                "Last status: {}",
                self.task.snapshot().status
            );
            assert!(
                !self.thread.as_ref().unwrap().is_finished(),
                "Worker exited unexpectedly"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn child_pid(&self) -> u32 {
        std::fs::read_to_string(self.cwd.join("fixture-child.pid"))
            .unwrap()
            .parse()
            .unwrap()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.task.send(Action::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(self.cwd.join("fixture-child.pid"));
        let _ = std::fs::remove_file(self.cwd.join("lidguard-smoke.txt"));
        let _ = std::fs::remove_dir(&self.cwd);
    }
}

#[test]
fn empty_overlay_thread_waits_for_first_message_and_releases_startup_protection() {
    let running = Running::start(fixture(), "");
    running.until(|view| view.ready && !view.busy);
    assert!(running.task.snapshot().messages.is_empty());
    assert!(running.task.snapshot().turn_id.is_none());
    assert_eq!(running.task.snapshot().status, "Ready");
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
    assert_eq!(running.task.window.load(Ordering::Acquire), 0);
    let (acknowledge, sent) = mpsc::channel();
    running.task.send(Action::Reply("wait-fixture first message".into(), acknowledge));
    assert!(sent.recv_timeout(Duration::from_secs(5)).unwrap().is_ok());
    running.until(|view| view.busy && view.turn_id.is_some());
    assert_eq!(running.task.snapshot().messages[0], ChatMessage::new(Role::User, "wait-fixture first message"));
    assert_eq!(running.guards.load(Ordering::SeqCst), 1);
    running.task.send(Action::Interrupt);
    running.until(|view| !view.busy);
}

#[test]
fn background_outlives_its_caller_handles_approval_and_accepts_followup() {
    let mut running = Running::start(fixture(), "fixture task");
    // The creator can disappear; the daemon's worker remains the owner.
    let caller = running.task.clone();
    drop(caller);
    running.until(|view| !view.pending.is_empty());
    let (blocked, rejected) = mpsc::channel();
    running.task.send(Action::Reply("Must not answer an approval".into(), blocked));
    assert!(rejected.recv_timeout(Duration::from_secs(5)).unwrap().is_err());
    assert_eq!(running.guards.load(Ordering::SeqCst), 1);
    assert!(
        running.task.snapshot().busy,
        "A stale terminal event must not release this turn"
    );
    let child_pid = running.child_pid();
    assert!(crate::win::is_process_running(child_pid));
    running.task.send(Action::Answer {
        id: json!("old-approval"),
        result: json!({"decision":"accept"}),
    });
    thread::sleep(Duration::from_millis(100));
    assert_eq!(running.task.snapshot().pending.len(), 1);
    running.task.send(Action::Answer {
        id: json!("approve-1"),
        result: json!({"decision":"accept"}),
    });
    running.until(|view| !view.busy);
    assert_eq!(running.task.snapshot().latest, "Finished ✓");
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
    assert!(
        crate::win::is_process_running(child_pid),
        "The idle session stays available for follow-ups"
    );
    let (sent, accepted) = mpsc::channel();
    running.task.send(Action::Reply("Continue fixture".into(), sent));
    assert_eq!(accepted.recv_timeout(Duration::from_secs(5)).unwrap(), Ok(()));
    running.until(|view| !view.busy && view.latest == "Follow-up finished.");
    assert!(
        running
            .task
            .snapshot()
            .history
            .contains("You: Continue fixture")
    );
    running.task.send(Action::Shutdown);
    running.thread.take().unwrap().join().unwrap().unwrap();
    assert!(
        !crate::win::is_process_running(child_pid),
        "Ending a session must stop its descendants"
    );
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
}

#[test]
fn deny_and_interrupt_release_protection_without_ending_the_session() {
    let running = Running::start(fixture(), "fixture task");
    running.until(|view| !view.pending.is_empty());
    running.task.send(Action::Answer {
        id: json!("approve-1"),
        result: json!({"decision":"decline"}),
    });
    running.until(|view| !view.busy);
    assert_eq!(running.task.snapshot().status, "Stopped");
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
    running.task.send(Action::Send("wait-fixture".into()));
    running.until(|view| view.turn_id.is_some());
    let (sent, accepted) = mpsc::channel();
    running.task.send(Action::Reply("Steering fixture".into(), sent));
    assert_eq!(accepted.recv_timeout(Duration::from_secs(5)).unwrap(), Ok(()));
    assert!(running.task.snapshot().history.contains("You: Steering fixture"));
    running.task.send(Action::Interrupt);
    running.until(|view| !view.busy);
    assert!(running.task.snapshot().pending.is_empty());
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
}

#[test]
fn worker_crash_reaps_descendants_and_releases_protection() {
    let mut running = Running::start(fixture(), "crash-fixture");
    let result = running.thread.take().unwrap().join().unwrap();
    assert!(result.is_err());
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
    assert!(!crate::win::is_process_running(running.child_pid()));
}

#[test]
fn ending_a_session_is_responsive_when_codex_stops_reading_input() {
    let mut running = Running::start(fixture(), "hang-fixture");
    running.until(|view| !view.busy);
    running.task.send(Action::Send("x".repeat(120_000)));
    running.until(|view| view.busy);
    running.task.send(Action::Shutdown);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !running.thread.as_ref().unwrap().is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        running.thread.as_ref().unwrap().is_finished(),
        "Stop was blocked on Codex stdin"
    );
    running.thread.take().unwrap().join().unwrap().unwrap();
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
    assert!(!crate::win::is_process_running(running.child_pid()));
}

#[test]
#[ignore = "Uses the signed-in Codex runtime for a small, tool-free integration turn"]
fn installed_codex_app_server_smoke_test() {
    let executable = PathBuf::from(
        std::env::var_os("LIDGUARD_TEST_CODEX")
            .expect("Set LIDGUARD_TEST_CODEX to the installed codex.exe"),
    );
    let mut running = Running::start(
        executable,
        "Reply with exactly: Lid Guard background test OK. Do not use tools, edit files, or access any resources.",
    );
    let deadline = Instant::now() + Duration::from_secs(180);
    while running.task.snapshot().busy
        && !running.thread.as_ref().unwrap().is_finished()
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(100));
    }
    let view = running.task.snapshot();
    running.task.send(Action::Shutdown);
    let result = running.thread.take().unwrap().join().unwrap();
    assert!(result.is_ok(), "Live Codex failed: {result:?}");
    assert!(
        !view.busy,
        "Live turn timed out; last status: {}",
        view.status
    );
    assert_eq!(view.status, "Finished");
    assert!(view.latest.contains("Lid Guard background test OK"));
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
}

#[test]
#[ignore = "Uses signed-in Codex to create one marker file in an isolated temporary project"]
fn installed_codex_workspace_write_smoke_test() {
    let executable =
        PathBuf::from(std::env::var_os("LIDGUARD_TEST_CODEX").expect("Set LIDGUARD_TEST_CODEX"));
    let mut running = Running::start(
        executable,
        "Create exactly one file named lidguard-smoke.txt in the current working directory, containing exactly BACKGROUND_WORKER_OK. This is an isolated integration test. Do not read any other project, run external commands, or access the network. Then briefly confirm completion.",
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    while running.task.snapshot().busy
        && running.task.snapshot().pending.is_empty()
        && !running.thread.as_ref().unwrap().is_finished()
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(100));
    }
    let view = running.task.snapshot();
    running.task.send(Action::Shutdown);
    let result = running.thread.take().unwrap().join().unwrap();
    assert!(result.is_ok(), "Live Codex failed: {result:?}");
    assert!(
        view.pending.is_empty(),
        "The isolated file edit requested approval"
    );
    assert!(!view.busy, "Live turn timed out: {}", view.status);
    assert_eq!(view.status, "Finished", "{}", view.latest);
    assert_eq!(
        std::fs::read_to_string(running.cwd.join("lidguard-smoke.txt"))
            .unwrap()
            .trim(),
        "BACKGROUND_WORKER_OK"
    );
    assert_eq!(running.guards.load(Ordering::SeqCst), 0);
}
