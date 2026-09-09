//! A native conversation window that remains usable after VS Code exits.
use super::*;
use crate::background::{Action, PendingInput, Task, View};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

const WM_SIZE: u32 = 5;
const WM_TIMER: u32 = 0x0113;
const SEND: usize = 101;
const STOP: usize = 102;
const APPROVE: usize = 103;
const DENY: usize = 104;
const END: usize = 105;
const DOCK: usize = 106;
const SHOW: u32 = 0x8055;

#[repr(C)]
struct MinMaxInfo {
    reserved: Point,
    maximum_size: Point,
    maximum_position: Point,
    minimum_track: Point,
    maximum_track: Point,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetTimer(window: Hwnd, id: usize, milliseconds: u32, callback: *const c_void) -> usize;
    fn KillTimer(window: Hwnd, id: usize) -> Bool;
    fn SetWindowTextW(window: Hwnd, text: *const u16) -> Bool;
    fn GetWindowTextLengthW(window: Hwnd) -> i32;
    fn GetWindowTextW(window: Hwnd, text: *mut u16, count: i32) -> i32;
    fn MoveWindow(window: Hwnd, x: i32, y: i32, width: i32, height: i32, repaint: Bool) -> Bool;
    fn EnableWindow(window: Hwnd, enable: Bool) -> Bool;
    fn IsDialogMessageW(window: Hwnd, message: *mut Message) -> Bool;
    fn IsWindowVisible(window: Hwnd) -> Bool;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        output: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> Handle;
}
struct Font(Handle);
impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                DeleteObject(self.0);
            }
        }
    }
}

struct State {
    task: Arc<Task>,
    status: Hwnd,
    history: Hwnd,
    details: Hwnd,
    input: Hwnd,
    buttons: [Hwnd; 6],
    revision: u64,
    history_text: String,
    details_text: String,
    pending: Option<PendingInput>,
    question: usize,
    answers: serde_json::Map<String, Value>,
}

pub fn show_background_window(window: u64) -> bool {
    unsafe {
        let window = window as usize as Hwnd;
        if IsWindow(window) == 0 {
            return false;
        }
        PostMessageW(window, SHOW, 0, 0) != 0
    }
}

pub fn background_window_visible(window: u64) -> bool {
    unsafe {
        IsWindowVisible(window as usize as Hwnd) != 0 && IsIconic(window as usize as Hwnd) == 0
    }
}

pub fn run_background_window(task: Arc<Task>) -> io::Result<()> {
    unsafe {
        let class = wide(format!("CodexLidGuardBackground.{}", task.id));
        let instance = GetModuleHandleW(null());
        let mut wc: WindowClassExW = zeroed();
        wc.size = size_of::<WindowClassExW>() as u32;
        wc.window_procedure = Some(procedure);
        wc.instance = instance;
        wc.class_name = class.as_ptr();
        wc.background = 6usize as Handle; // COLOR_WINDOW + 1
        if RegisterClassExW(&wc) == 0 {
            return Err(error("Register background task window"));
        }
        let window = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.as_ptr(),
            wide(format!("Background Codex — {}", task.snapshot().title)).as_ptr(),
            0x00cf_0000,
            0x8000_0000u32 as i32,
            0x8000_0000u32 as i32,
            880,
            760,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            UnregisterClassW(class.as_ptr(), instance);
            return Err(error("Create background task window"));
        }
        let font = Font(CreateFontW(
            -16,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            wide("Segoe UI").as_ptr(),
        ));
        let control = |kind: &str, label: &str, style: u32, id: usize| {
            let child = CreateWindowExW(
                if kind == "EDIT" { 0x200 } else { 0 },
                wide(kind).as_ptr(),
                wide(label).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                1,
                1,
                window,
                id as Handle,
                instance,
                null(),
            );
            SendMessageW(
                child,
                WM_SETFONT,
                if font.0.is_null() {
                    GetStockObject(DEFAULT_GUI_FONT)
                } else {
                    font.0
                } as usize,
                1,
            );
            child
        };
        let multiline = WS_TABSTOP | 0x0004 | 0x0040 | 0x0020_0000; // multiline / auto-vscroll / vscroll
        let mut state = Box::new(State {
            task: task.clone(),
            status: control("STATIC", "", 0, 0),
            history: control("EDIT", "", multiline | 0x0800, 10), // readonly
            details: control("EDIT", "", multiline | 0x0800, 11),
            input: control("EDIT", "", multiline | 0x1000, 12), // want return
            buttons: [
                control("BUTTON", "Send", WS_TABSTOP, SEND),
                control("BUTTON", "Stop turn", WS_TABSTOP, STOP),
                control("BUTTON", "Allow once", WS_TABSTOP, APPROVE),
                control("BUTTON", "Deny", WS_TABSTOP, DENY),
                control("BUTTON", "End session", WS_TABSTOP, END),
                control("BUTTON", "Minimize to tab", WS_TABSTOP, DOCK),
            ],
            revision: 0,
            history_text: String::new(),
            details_text: String::new(),
            pending: None,
            question: 0,
            answers: Default::default(),
        });
        if [state.status, state.history, state.details, state.input]
            .into_iter()
            .chain(state.buttons)
            .any(|child| child.is_null())
        {
            DestroyWindow(window);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(error("Create background task controls"));
        }
        SendMessageW(state.history, 0x00c5, 256 * 1024, 0); // EM_SETLIMITTEXT
        SendMessageW(state.details, 0x00c5, 4 * 1024 * 1024, 0);
        SendMessageW(state.input, 0x00c5, 128 * 1024, 0);
        SetWindowLongPtrW(window, GWLP_USERDATA, (&mut *state) as *mut State as isize);
        task.window.store(window as u64, Ordering::Release);
        layout(window, &state);
        refresh(&mut state, true);
        SetTimer(window, 1, 150, null());
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
        SetFocus(state.input);
        let mut message: Message = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(window, &mut message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        task.window.store(0, Ordering::Release);
        task.dock();
        if IsWindow(window) != 0 {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            DestroyWindow(window);
        }
        UnregisterClassW(class.as_ptr(), instance);
        Ok(())
    }
}

unsafe fn set_text(window: Hwnd, text: &str) {
    unsafe {
        SetWindowTextW(
            window,
            wide(
                text.replace('\0', "")
                    .replace("\r\n", "\n")
                    .replace('\n', "\r\n"),
            )
            .as_ptr(),
        );
    }
}

unsafe fn input_text(window: Hwnd) -> String {
    unsafe {
        let length = GetWindowTextLengthW(window).clamp(0, 128 * 1024);
        let mut buffer = vec![0u16; length as usize + 1];
        let count =
            GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&buffer[..count])
    }
}

unsafe fn layout(window: Hwnd, state: &State) {
    unsafe {
        let mut rect: Rect = zeroed();
        GetClientRect(window, &mut rect);
        let width = (rect.right - 24).max(220);
        let height = (rect.bottom - 24).max(330);
        let history_height = (height - 304).max(90);
        MoveWindow(state.status, 12, 10, width, 44, 1);
        MoveWindow(state.history, 12, 62, width, history_height, 1);
        MoveWindow(state.details, 12, history_height + 72, width, 110, 1);
        MoveWindow(state.input, 12, history_height + 192, width, 70, 1);
        let button_width = ((width - 25) / 6).max(60);
        for (index, button) in state.buttons.iter().enumerate() {
            MoveWindow(
                *button,
                12 + index as i32 * (button_width + 5),
                history_height + 274,
                button_width,
                30,
                1,
            );
        }
    }
}

fn question_text(pending: &PendingInput, index: usize) -> Option<String> {
    let questions = pending.params["questions"].as_array()?;
    let question = questions.get(index)?;
    let mut text = format!(
        "Question {} of {}: {}\n{}",
        index + 1,
        questions.len(),
        question["header"].as_str().unwrap_or_default(),
        question["question"].as_str().unwrap_or_default()
    );
    if let Some(options) = question["options"].as_array() {
        for option in options {
            text.push_str(&format!(
                "\n• {} — {}",
                option["label"].as_str().unwrap_or_default(),
                option["description"].as_str().unwrap_or_default()
            ));
        }
        text.push_str("\nType an option's label or your own answer below.");
    }
    Some(text)
}

unsafe fn refresh(state: &mut State, force: bool) {
    unsafe {
        let view = state.task.snapshot();
        if !force && view.revision == state.revision {
            return;
        }
        state.revision = view.revision;
        set_text(
            state.status,
            &format!(
                "{}  ·  {}\nClosing this window keeps the session running. End session stops it.",
                view.status, view.cwd
            ),
        );
        if state.history_text != view.history {
            state.history_text = view.history.clone();
            set_text(state.history, &view.history);
            SendMessageW(state.history, 0x00b1, usize::MAX, -1); // select end
            SendMessageW(state.history, 0x00b7, 0, 0); // scroll caret
        }
        let next = view.pending.first().cloned();
        if state.pending.as_ref().map(|p| &p.id) != next.as_ref().map(|p| &p.id) {
            state.pending = next;
            state.question = 0;
            state.answers.clear();
            // Keep a drafted follow-up until the user chooses how to answer a request.
        }
        let question = state
            .pending
            .as_ref()
            .and_then(|pending| question_text(pending, state.question));
        let details = question.clone().or_else(|| state.pending.as_ref().map(|p| p.details.clone()))
            .unwrap_or_else(|| if let Some(id) = &view.thread_id { format!("Codex session: {id}\nSend a follow-up below when the turn finishes. Your session also appears in Codex history.") }
                else { "Connecting to your installed Codex runtime…".into() });
        // Do not reset approval scrolling on unrelated stream/status updates.
        if state.details_text != details {
            set_text(state.details, &details);
            state.details_text = details;
        }
        set_text(
            state.buttons[0],
            if question.is_some() {
                "Submit answer"
            } else {
                "Send"
            },
        );
        EnableWindow(
            state.buttons[0],
            (question.is_some() || (view.ready && !view.busy && !view.ended)) as Bool,
        );
        EnableWindow(state.input, (question.is_some() || !view.busy) as Bool);
        EnableWindow(state.buttons[1], (view.busy && !view.ended) as Bool);
        let can_allow = state.pending.as_ref().is_some_and(|pending| {
            crate::background::valid_answer(pending, &json!({"decision":"accept"}))
        });
        EnableWindow(state.buttons[2], can_allow as Bool);
        EnableWindow(
            state.buttons[3],
            state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.method != "item/tool/requestUserInput")
                as Bool,
        );
    }
}

unsafe fn command(window: Hwnd, state: &mut State, id: usize) {
    unsafe {
        match id {
            SEND => {
                let text = input_text(state.input);
                if text.trim().is_empty() {
                    return;
                }
                if let Some(pending) = state
                    .pending
                    .clone()
                    .filter(|p| p.method == "item/tool/requestUserInput")
                {
                    let Some(questions) = pending.params["questions"].as_array() else {
                        return;
                    };
                    let Some(id) = questions.get(state.question).and_then(|q| q["id"].as_str())
                    else {
                        return;
                    };
                    state.answers.insert(id.into(), json!({"answers":[text]}));
                    state.question += 1;
                    if state.question >= questions.len() {
                        state.task.send(Action::Answer {
                            id: pending.id,
                            result: json!({"answers":state.answers}),
                        });
                        EnableWindow(state.buttons[0], 0);
                    } else {
                        refresh(state, true);
                    }
                } else {
                    let View {
                        busy, ready, ended, ..
                    } = state.task.snapshot();
                    if busy || !ready || ended {
                        return;
                    }
                    state.task.send(Action::Send(text));
                    EnableWindow(state.buttons[0], 0);
                }
                set_text(state.input, "");
            }
            STOP => {
                state.task.send(Action::Interrupt);
                EnableWindow(state.buttons[1], 0);
            }
            APPROVE | DENY => {
                if let Some(pending) = &state.pending {
                    let decision = if id == APPROVE {
                        "accept"
                    } else if crate::background::valid_answer(
                        pending,
                        &json!({"decision":"decline"}),
                    ) {
                        "decline"
                    } else {
                        "cancel"
                    };
                    state.task.send(Action::Answer {
                        id: pending.id.clone(),
                        result: json!({"decision":decision}),
                    });
                    EnableWindow(state.buttons[2], 0);
                    EnableWindow(state.buttons[3], 0);
                }
            }
            END => {
                crate::background::end(&state.task.id);
                DestroyWindow(window);
            }
            DOCK => {
                DestroyWindow(window);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn procedure(
    window: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    unsafe {
        if message == 0x0024 && lparam != 0 {
            // WM_GETMINMAXINFO
            let info = &mut *(lparam as *mut MinMaxInfo);
            info.minimum_track = Point { x: 760, y: 550 };
            return 0;
        }
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut State;
        if !pointer.is_null() {
            match message {
                WM_TIMER => {
                    refresh(&mut *pointer, false);
                    return 0;
                }
                WM_SIZE => {
                    layout(window, &*pointer);
                    return 0;
                }
                WM_COMMAND if wparam >> 16 == 0 => {
                    command(window, &mut *pointer, wparam & 0xffff);
                    return 0;
                }
                SHOW => {
                    ShowWindow(window, SW_RESTORE);
                    SetForegroundWindow(window);
                    return 0;
                }
                WM_CLOSE => {
                    DestroyWindow(window);
                    return 0;
                }
                _ => {}
            }
        }
        if message == WM_DESTROY {
            KillTimer(window, 1);
            PostQuitMessage(0);
            return 0;
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDlgItem(window: Hwnd, id: i32) -> Hwnd;
        fn GetWindowDC(window: Hwnd) -> Handle;
        fn ReleaseDC(window: Hwnd, dc: Handle) -> i32;
        fn PrintWindow(window: Hwnd, dc: Handle, flags: u32) -> Bool;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(dc: Handle) -> Handle;
        fn CreateCompatibleBitmap(dc: Handle, width: i32, height: i32) -> Handle;
        fn DeleteDC(dc: Handle) -> Bool;
        fn GetDIBits(
            dc: Handle,
            bitmap: Handle,
            start: u32,
            lines: u32,
            pixels: *mut u8,
            info: *mut u8,
            usage: u32,
        ) -> i32;
    }

    unsafe fn capture(window: Hwnd, destination: &std::path::Path) {
        unsafe {
            let mut rect: Rect = zeroed();
            GetWindowRect(window, &mut rect);
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            let source = GetWindowDC(window);
            let dc = CreateCompatibleDC(source);
            let bitmap = CreateCompatibleBitmap(source, width, height);
            let old = SelectObject(dc, bitmap);
            assert_ne!(PrintWindow(window, dc, 2), 0);
            SelectObject(dc, old);
            let mut header = [0u8; 40];
            header[..4].copy_from_slice(&40u32.to_le_bytes());
            header[4..8].copy_from_slice(&width.to_le_bytes());
            header[8..12].copy_from_slice(&height.to_le_bytes());
            header[12..14].copy_from_slice(&1u16.to_le_bytes());
            header[14..16].copy_from_slice(&32u16.to_le_bytes());
            let mut pixels = vec![0u8; width as usize * height as usize * 4];
            assert_ne!(
                GetDIBits(
                    dc,
                    bitmap,
                    0,
                    height as u32,
                    pixels.as_mut_ptr(),
                    header.as_mut_ptr(),
                    0
                ),
                0
            );
            let mut file = Vec::from(*b"BM");
            file.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
            file.extend_from_slice(&[0; 4]);
            file.extend_from_slice(&54u32.to_le_bytes());
            file.extend_from_slice(&header);
            file.extend_from_slice(&pixels);
            std::fs::write(destination, file).unwrap();
            DeleteObject(bitmap);
            DeleteDC(dc);
            ReleaseDC(window, source);
        }
    }

    #[test]
    #[ignore = "Displays and exercises only the test-owned background task window"]
    fn background_window_answers_requests_and_docks_without_stopping_worker() {
        let (task, commands) = crate::background::integration_tests::task_for_test();
        {
            let mut view = task.view.lock().unwrap();
            view.status = "Needs your response".into();
            view.history = "You: Run the project tests and fix any failures.\n\nCodex: I found the failing test and prepared a fix. I need permission to run the final check.".into();
            view.pending = vec![PendingInput { id: json!("approval"), method: "item/commandExecution/requestApproval".into(),
                params: json!({"command":"npm test"}), details: "Allow this command once?\n\nCommand: npm test\nReason: Verify the fix in your project.\nWorking directory: C:\\Projects\\Sample".into() }];
        }
        let owned = task.clone();
        let ui = thread::spawn(move || run_background_window(owned));
        let deadline = Instant::now() + Duration::from_secs(5);
        while task.window.load(Ordering::Acquire) == 0 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        let window = task.window.load(Ordering::Acquire) as usize as Hwnd;
        let checked = std::panic::catch_unwind(|| unsafe {
            SendMessageW(window, WM_TIMER, 1, 0);
            if let Some(path) = std::env::var_os("LIDGUARD_TEST_SCREENSHOT") {
                thread::sleep(Duration::from_millis(350));
                capture(window, std::path::Path::new(&path));
            }
            SendMessageW(window, WM_COMMAND, APPROVE, 0);
            assert!(
                matches!(commands.recv_timeout(Duration::from_secs(1)).unwrap(), Action::Answer { id, result }
                if id == json!("approval") && result == json!({"decision":"accept"}))
            );
            {
                let mut view = task.view.lock().unwrap();
                view.pending = vec![PendingInput {
                    id: json!("questions"),
                    method: "item/tool/requestUserInput".into(),
                    params: json!({"questions":[{"id":"first","header":"First","question":"Pick the first answer"},{"id":"second","header":"Second","question":"Pick the second answer"}]}),
                    details: String::new(),
                }];
                view.revision += 1;
            }
            SendMessageW(window, WM_TIMER, 1, 0);
            set_text(GetDlgItem(window, 12), "answer one");
            SendMessageW(window, WM_COMMAND, SEND, 0);
            assert!(
                commands.try_recv().is_err(),
                "Wait for all required answers"
            );
            set_text(GetDlgItem(window, 12), "answer two");
            SendMessageW(window, WM_COMMAND, SEND, 0);
            assert!(
                matches!(commands.recv_timeout(Duration::from_secs(1)).unwrap(), Action::Answer { id, result }
                if id == json!("questions") && result["answers"]["first"]["answers"][0] == "answer one"
                && result["answers"]["second"]["answers"][0] == "answer two")
            );
            {
                let mut view = task.view.lock().unwrap();
                view.pending.clear();
                view.busy = false;
                view.ready = true;
                view.revision += 1;
            }
            SendMessageW(window, WM_TIMER, 1, 0);
            set_text(GetDlgItem(window, 12), "Continue the task");
            SendMessageW(window, WM_COMMAND, SEND, 0);
            assert!(
                matches!(commands.recv_timeout(Duration::from_secs(1)).unwrap(), Action::Send(prompt) if prompt == "Continue the task")
            );
        });
        unsafe {
            PostMessageW(window, WM_CLOSE, 0, 0);
        }
        ui.join().unwrap().unwrap();
        assert_eq!(task.window.load(Ordering::Acquire), 0);
        assert!(task.snapshot().dock_request > 1);
        assert!(
            commands.try_recv().is_err(),
            "Closing the view must not send a shutdown to the worker"
        );
        if let Err(panic) = checked {
            std::panic::resume_unwind(panic);
        }
    }
}
