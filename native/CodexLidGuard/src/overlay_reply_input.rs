//! An ordinary native edit surface over the glass drawer. Clicking the input
//! or typing over its overlay activates it; updates never take focus.
use super::*;
use std::collections::HashMap;
use super::super::overlay_shortcuts::{TypingTarget, WM_HOVER_TEXT};
static NEXT_TYPING_TOKEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowTextLengthW(window: Hwnd) -> i32;
    fn GetWindowTextW(window: Hwnd, text: *mut u16, count: i32) -> i32;
    fn SetWindowTextW(window: Hwnd, text: *const u16) -> Bool;
    fn CallWindowProcW(procedure: isize, window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn GetKeyboardState(keys: *mut u8) -> Bool;
    fn SetKeyboardState(keys: *const u8) -> Bool;
    fn GetAsyncKeyState(key: i32) -> i16;
}
#[link(name = "gdi32")]
unsafe extern "system" { fn SetBkColor(dc: Handle, color: u32) -> u32; }

struct EditState {
    owner: Hwnd,
    surface: Hwnd,
    input: Hwnd,
    original: isize,
    composing: bool,
    setting_text: bool,
    brush: Handle,
    previous_foreground: Hwnd,
    typing_token: usize,
}

type PendingReply = (String, String, mpsc::Receiver<Result<(), String>>);

pub(super) struct Composer {
    window: Hwnd,
    class: Vec<u16>,
    edit: Box<EditState>,
    font: Handle,
    dpi: u32,
    bounds: Option<Rect>,
    visible: bool,
    animating: bool,
    animation_style: Option<isize>,
    bound: Option<CardTarget>,
    drafts: HashMap<String, String>,
    notices: HashMap<String, String>,
    sent: Option<(String, Instant)>,
    pending: Option<PendingReply>,
    expanded: bool,
    history: Option<chat_panel::HistoryWorker>,
    snapshot: Option<crate::chat_history::Snapshot>,
    history_error: Option<String>,
    busy: bool,
    needs_input: bool,
}

impl Composer {
    pub fn new(owner: Hwnd) -> io::Result<Self> {
        unsafe {
            let class = wide(format!("CodexLidGuardReply.{}", owner as usize));
            let instance = GetModuleHandleW(null());
            let brush = CreateSolidBrush(color_ref(28, 37, 51));
            let mut wc: WindowClassExW = zeroed();
            wc.size = size_of::<WindowClassExW>() as u32;
            wc.window_procedure = Some(procedure);
            wc.instance = instance;
            wc.class_name = class.as_ptr();
            wc.cursor = LoadCursorW(null_mut(), 32513usize as *const u16);
            wc.background = brush;
            if RegisterClassExW(&wc) == 0 { DeleteObject(brush); return Err(error("Register reply input")); }
            let window = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, class.as_ptr(), wide("Reply to selected Codex chat").as_ptr(),
                WS_POPUP, 0, 0, 1, 1, owner, null_mut(), instance, null());
            let input = CreateWindowExW(0, wide("EDIT").as_ptr(), wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | 0x0004 | 0x0040 | 0x1000, 0, 0, 1, 1, window, 1usize as Handle, instance, null());
            if window.is_null() || input.is_null() {
                if !window.is_null() { DestroyWindow(window); }
                UnregisterClassW(class.as_ptr(), instance); DeleteObject(brush);
                return Err(error("Create reply input"));
            }
            let mut edit = Box::new(EditState { owner, surface: window, input, original: 0, composing: false,
                setting_text: false, brush, previous_foreground: null_mut(), typing_token: 0 });
            let state = (&mut *edit) as *mut EditState as isize;
            SetWindowLongPtrW(window, GWLP_USERDATA, state);
            SetWindowLongPtrW(input, GWLP_USERDATA, state);
            edit.original = SetWindowLongPtrW(input, -4, edit_procedure as *const () as isize);
            SendMessageW(input, 0x00c5, 8192, 0); // EM_SETLIMITTEXT
            SendMessageW(input, 0x1501, 1, wide("Message…").as_ptr() as isize); // cue banner
            Ok(Self { window, class, edit, font: null_mut(), dpi: 0, bounds: None, visible: false,
                animating: false, animation_style: None,
                bound: None, drafts: HashMap::new(), notices: HashMap::new(), sent: None, pending: None,
                expanded: false, history: None, snapshot: None, history_error: None, busy: false, needs_input: false })
        }
    }

    fn text(&self) -> String {
        unsafe {
            // A restored failed submission may sit alongside a newer 8K draft.
            let count = GetWindowTextLengthW(self.edit.input).clamp(0, 32 * 1024);
            let mut text = vec![0u16; count as usize + 1];
            let length = GetWindowTextW(self.edit.input, text.as_mut_ptr(), text.len() as i32);
            String::from_utf16_lossy(&text[..length.max(0) as usize])
        }
    }
    fn set_text(&mut self, text: &str) {
        self.edit.setting_text = true;
        unsafe {
            SetWindowTextW(self.edit.input, wide(text).as_ptr());
            let end = text.encode_utf16().count();
            SendMessageW(self.edit.input, 0x00b1, end, end as isize);
            SendMessageW(self.edit.input, 0x00b7, 0, 0);
        }
        self.edit.setting_text = false;
    }
    fn save(&mut self) {
        if let Some(target) = &self.bound {
            self.drafts.insert(target.session_id.clone(), self.text());
        }
    }
    pub fn edited(&mut self) {
        self.save();
        self.sent = None;
        if let Some(target) = &self.bound { self.notices.remove(&target.session_id); }
    }
    pub fn focused(&self) -> bool { unsafe { self.visible && GetFocus() == self.edit.input } }
    pub fn focus(&self) {
        if self.visible && self.edit.typing_token != 0 {
            unsafe { PostMessageW(self.window, WM_HOVER_TEXT, self.edit.typing_token, 0); }
        }
    }
    pub fn notice(&mut self, id: &str, text: &str) {
        self.sent = None;
        if text.is_empty() { self.notices.remove(id); } else { self.notices.insert(id.into(), text.into()); }
    }
    pub fn expanded(&self) -> bool { self.expanded }
    pub fn has_text(&self) -> bool { !self.text().is_empty() }
    pub fn overflows(&self, width: i32) -> bool {
        let text = self.text();
        if text.contains(['\r', '\n']) { return true; }
        unsafe {
            let dc = GetDC(self.edit.input);
            let old = SelectObject(dc, self.font);
            let value = wide(&text);
            let mut rect = Rect { left: 0, top: 0, right: 0, bottom: 0 };
            DrawTextW(dc, value.as_ptr(), wide_text_length(&value), &mut rect, DT_SINGLELINE | DT_NOPREFIX | DT_CALCRECT);
            SelectObject(dc, old); ReleaseDC(self.edit.input, dc);
            rect.right > (width - scale_dip(14, self.dpi.max(96))).max(1)
        }
    }
    pub fn target(&self) -> Option<&CardTarget> { self.bound.as_ref() }
    pub fn typing_target(&self) -> Option<TypingTarget> {
        (self.visible && self.edit.typing_token != 0).then_some(TypingTarget {
            overlay: self.edit.owner as usize, surface: self.window as usize, token: self.edit.typing_token,
        })
    }
    pub fn observe(&mut self, busy: bool, needs_input: bool) { self.busy = busy; self.needs_input = needs_input; }
    pub fn expand(&mut self, busy: bool, needs_input: bool) -> io::Result<()> {
        if self.expanded { return Ok(()); }
        self.preview(busy, needs_input)?;
        self.expanded = true;
        Ok(())
    }
    pub fn preview(&mut self, busy: bool, needs_input: bool) -> io::Result<()> {
        let Some(target) = &self.bound else { return Ok(()); };
        if self.history.is_none() {
            self.history = Some(chat_panel::HistoryWorker::new(target.session_id.clone(), self.edit.owner as usize)?);
            self.snapshot = None; self.history_error = None;
        }
        self.busy = busy; self.needs_input = needs_input;
        Ok(())
    }
    pub fn collapse_chat(&mut self) {
        if !self.expanded && self.history.is_none() { return; }
        self.save(); self.expanded = false; self.history = None;
        if self.animating { self.hide(); }
    }
    pub fn history_messages(&self) -> Option<&[crate::chat_history::Message]> {
        self.snapshot.as_ref().map(|snapshot| snapshot.messages.as_slice())
    }
    pub fn history_status(&self) -> &str {
        self.bound.as_ref().and_then(|target| self.status(&target.session_id))
            .or(self.history_error.as_deref()).unwrap_or("Enter sends \u{b7} Shift+Enter adds a line \u{b7} Esc folds")
    }
    pub fn send_expanded(&mut self) {
        if let Some(target) = self.bound.clone() {
            let needs_input = self.needs_input || crate::overlay::waiting_for_response(&target.session_id)
                || crate::background::chat_snapshot(&target.session_id).is_some_and(|view| !view.pending.is_empty());
            // History is for display; it can lag the live session at turn boundaries.
            self.send(target, self.busy, needs_input);
        }
    }
    pub fn hide(&mut self) {
        self.edit.typing_token = 0;
        if self.visible {
            self.save();
            unsafe {
                let restore = self.focused();
                ShowWindow(self.window, 0);
                if restore && IsWindow(self.edit.previous_foreground) != 0 {
                    SetForegroundWindow(self.edit.previous_foreground);
                }
            }
            self.visible = false;
            self.bounds = None;
        }
        self.animating = false;
        self.restore_animation_style();
    }
    // Keep the real edit focused and accepting keystrokes, but animate its cached
    // pixels with the overlay instead of resizing a second DWM surface every tick.
    pub fn begin_growth(&mut self) -> io::Result<()> {
        if self.animating { return Ok(()); }
        unsafe {
            let style = GetWindowLongPtrW(self.window, -20);
            SetWindowLongPtrW(self.window, -20, style | WS_EX_LAYERED as isize);
            if SetLayeredWindowAttributes(self.window, 0, 0, LWA_ALPHA) == 0 {
                SetWindowLongPtrW(self.window, -20, style);
                return Err(error("Cache reply input for animation"));
            }
            self.animation_style = Some(style);
            self.animating = true;
        }
        Ok(())
    }
    pub fn finish_growth(&mut self) { self.animating = false; }
    fn restore_animation_style(&mut self) {
        if let Some(style) = self.animation_style.take() {
            unsafe {
                SetWindowLongPtrW(self.window, -20, style);
                InvalidateRect(self.window, null(), 1);
            }
        }
    }
    pub unsafe fn paint_snapshot(&self, dc: Handle) {
        unsafe {
            // WM_PRINT: client, background and edit child. Never move the caret,
            // change the draft, or activate a new window to take this snapshot.
            SendMessageW(self.window, 0x0317, dc as usize, 0x04 | 0x08 | 0x10);
        }
    }
    pub fn sync(&mut self, rect: Rect, dpi: u32, target: CardTarget) -> io::Result<()> {
        if self.animating { return Ok(()); }
        if !self.visible || self.bound.as_ref() != Some(&target) {
            self.edit.typing_token = NEXT_TYPING_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if self.bound.as_ref() != Some(&target) {
            self.save();
            let text = self.drafts.get(&target.session_id).cloned().unwrap_or_default();
            self.set_text(&text);
            if self.expanded || self.history.is_some() {
                self.history = Some(chat_panel::HistoryWorker::new(target.session_id.clone(), self.edit.owner as usize)?);
                self.snapshot = None; self.history_error = None;
            }
            self.bound = Some(target);
        }
        unsafe {
            if self.dpi != dpi {
                let next = group_window::font(12, 400, dpi);
                SendMessageW(self.edit.input, WM_SETFONT, next as usize, 1);
                if !self.font.is_null() { DeleteObject(self.font); }
                self.font = next;
                self.dpi = dpi;
                self.bounds = None;
            }
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            let pad = scale_dip(5, dpi);
            if self.bounds != Some(rect) {
                SetWindowPos(self.edit.input, null_mut(), pad, scale_dip(4, dpi), width - 2 * pad,
                    height - scale_dip(6, dpi), SWP_NOZORDER | SWP_NOACTIVATE);
                let radius = scale_dip(8, dpi);
                let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius, radius);
                if !region.is_null() && SetWindowRgn(self.window, region, 0) == 0 { DeleteObject(region); }
                if SetWindowPos(self.window, -1isize as Hwnd, rect.left, rect.top, width, height,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOOWNERZORDER) == 0 { return Err(error("Position reply input")); }
                self.bounds = Some(rect);
            }
            self.visible = true;
        }
        // Restore visibility only after reaching the final geometry.
        self.restore_animation_style();
        Ok(())
    }
    pub fn status(&self, id: &str) -> Option<&str> {
        if self.pending.as_ref().is_some_and(|(pending, _, _)| pending == id) {
            Some(self.notices.get(id).map_or("Sending…", String::as_str))
        }
        else if self.sent.as_ref().is_some_and(|(sent, _)| sent == id) { Some("Sent") }
        else { self.notices.get(id).map(String::as_str) }
    }
    pub fn sending(&self) -> bool { self.pending.is_some() }
    #[cfg(test)]
    pub fn test_input(&self) -> Hwnd { self.edit.input }
    #[cfg(test)]
    pub fn test_animating(&self) -> bool { self.animating }
    #[cfg(test)]
    pub fn test_window(&self) -> Hwnd { self.edit.owner }
    #[cfg(test)]
    pub fn test_history(&mut self, text: String) {
        self.history = None;
        let messages = text.split("\r\n\r\n").map(|block| {
            let (label, body) = block.split_once("\r\n").unwrap_or(("", block));
            let role = match label { "You" => crate::chat_history::Role::User,
                "Codex" => crate::chat_history::Role::Assistant, _ => crate::chat_history::Role::Notice };
            let mut message = crate::chat_history::Message::new(role, if body == "[Image]" { "" } else { body });
            if body == "[Image]" { message.images = 1; }
            message
        }).collect();
        self.snapshot = Some(crate::chat_history::Snapshot { messages, busy: false });
        self.history_error = None;
        self.notices.clear();
    }
    #[cfg(test)]
    pub fn test_notice(&self) -> bool { self.bound.as_ref().is_some_and(|target| self.notices.contains_key(&target.session_id)) }

    pub fn send(&mut self, target: CardTarget, busy: bool, needs_input: bool) {
        if self.bound.as_ref() != Some(&target) { return; }
        let text = self.text();
        if text.trim().is_empty() { return; }
        if self.pending.is_some() {
            self.notices.insert(target.session_id, "Still sending the previous message. Send this draft again when it finishes.".into());
            return;
        }
        if text.encode_utf16().count() > 8192 {
            self.notices.insert(target.session_id, "Send at most 8,192 characters at a time. Your draft is still here.".into());
            return;
        }
        if needs_input {
            self.notices.insert(target.session_id, "Answer the pending question or approval in the chat first.".into());
            return;
        }
        self.save();
        let (finished, receiver) = mpsc::channel();
        let id = target.session_id.clone();
        let prompt = text.clone();
        let owner = self.edit.owner as usize;
        match thread::Builder::new().name("overlay-reply".into()).spawn(move || {
            let result = if crate::background::is_task(&target.session_id) {
                crate::background::send_reply(&target.session_id, prompt)
            } else { crate::session_navigation::send_reply(&target, &prompt, busy) };
            let _ = finished.send(result);
            unsafe { PostMessageW(owner as Hwnd, WM_FRAME_READY, 0, 0); }
        }) {
            Ok(_) => {
                self.notices.remove(&id); self.sent = None;
                self.pending = Some((id.clone(), text, receiver));
                // Clear at submission so the next message can be drafted immediately.
                self.drafts.remove(&id); self.set_text("");
            }
            Err(_) => { self.notices.insert(id, "Could not start sending. Your message is still here.".into()); }
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut history_changed = false;
        if let Some(result) = self.history.as_mut().and_then(|history| history.poll()) {
            match result {
                Ok(Some(snapshot)) => { self.snapshot = Some(snapshot); self.history_error = None; history_changed = true; }
                Ok(None) => {}
                Err(error) => { history_changed = self.history_error.as_ref() != Some(&error); self.history_error = Some(error); }
            }
        }
        let expired = self.sent.as_ref().is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(2));
        if expired { self.sent = None; }
        let Some((_, _, receiver)) = &self.pending else { return expired || history_changed; };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return expired || history_changed,
            Err(_) => Err("Send not confirmed. Check the chat before retrying.".into()),
        };
        let (id, text, _) = self.pending.take().unwrap();
        match result {
            Ok(()) => {
                self.notices.remove(&id);
                self.sent = Some((id, Instant::now()));
            }
            Err(message) => {
                self.save();
                let draft = self.drafts.get(&id).filter(|draft| !draft.is_empty());
                let restored = draft.map_or_else(|| text.clone(), |draft| format!("{text}\r\n\r\n{draft}"));
                self.drafts.insert(id.clone(), restored.clone());
                if self.bound.as_ref().is_some_and(|target| target.session_id == id) { self.set_text(&restored); }
                self.notices.insert(id, message);
            }
        }
        true
    }
}

impl Drop for Composer {
    fn drop(&mut self) {
        self.collapse_chat();
        self.hide();
        unsafe {
            SetWindowLongPtrW(self.edit.input, -4, self.edit.original);
            SetWindowLongPtrW(self.window, GWLP_USERDATA, 0);
            DestroyWindow(self.window);
            UnregisterClassW(self.class.as_ptr(), GetModuleHandleW(null()));
            if !self.font.is_null() { DeleteObject(self.font); }
            DeleteObject(self.edit.brush);
        }
    }
}

unsafe extern "system" fn procedure(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize {
    unsafe {
        if let Some(state) = (GetWindowLongPtrW(window, GWLP_USERDATA) as *mut EditState).as_mut() {
            match message {
                WM_HOVER_TEXT => {
                    // The token is invalidated by hiding or switching sessions.
                    if state.typing_token == 0 || wparam != state.typing_token { return 0; }
                    let previous = GetForegroundWindow();
                    if previous != window {
                        state.previous_foreground = previous;
                        if SetForegroundWindow(window) == 0 && GetForegroundWindow() == previous {
                            // Use the same explicit-action focus handoff as the
                            // notification popup. Never attach from the hook.
                            let current = GetCurrentThreadId();
                            let foreground = GetWindowThreadProcessId(previous, null_mut());
                            let mut keyboard = [0u8; 256];
                            let saved = GetKeyboardState(keyboard.as_mut_ptr()) != 0;
                            if foreground != 0 && foreground != current && AttachThreadInput(current, foreground, 1) != 0 {
                                SetForegroundWindow(window);
                                AttachThreadInput(current, foreground, 0);
                                if saved {
                                    // Attaching resets keyboard state; keep held
                                    // Shift and lock state correct for continued typing.
                                    for key in [0x10, 0x11, 0x12, 0x5b, 0x5c, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5] {
                                        keyboard[key] = (keyboard[key] & 1) | if GetAsyncKeyState(key as i32) < 0 { 0x80 } else { 0 };
                                    }
                                    SetKeyboardState(keyboard.as_ptr());
                                }
                            }
                        }
                    }
                    SetFocus(state.input);
                    // Preserve the first key, including surrogate pairs, before
                    // the queued focus event starts the cached growth animation.
                    for index in 0..4 {
                        let unit = ((lparam as u64 >> (index * 16)) & 0xffff) as u16;
                        if unit == 0 { break; }
                        SendMessageW(state.input, 0x0102, unit as usize, 0);
                    }
                    PostMessageW(state.owner, WM_APP_REPLY_FOCUS, 0, 0);
                    return 0;
                }
                0x0010 => { PostMessageW(state.owner, WM_APP_CLOSE_CHAT, 0, 0); return 0; }
                WM_MOUSEACTIVATE => {
                    let previous = GetForegroundWindow();
                    if previous != window { state.previous_foreground = previous; }
                    return 1;
                }
                7 => { SetFocus(state.input); return 0; } // WM_SETFOCUS
                0x0111 if wparam >> 16 == 0x0300 && !state.setting_text => {
                    PostMessageW(state.owner, WM_APP_REPLY_EDITED, 0, 0);
                }
                0x0133 | 0x0138 => { // WM_CTLCOLOREDIT / WM_CTLCOLORSTATIC
                    SetTextColor(wparam as Handle, color_ref(242, 247, 253));
                    SetBkColor(wparam as Handle, color_ref(28, 37, 51));
                    return state.brush as isize;
                }
                _ => {}
            }
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

unsafe extern "system" fn edit_procedure(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize {
    unsafe {
        let Some(state) = (GetWindowLongPtrW(window, GWLP_USERDATA) as *mut EditState).as_mut() else {
            return DefWindowProcW(window, message, wparam, lparam);
        };
        match message {
            7 | WM_LBUTTONDOWN => { PostMessageW(state.owner, WM_APP_REPLY_FOCUS, 0, 0); }
            WM_MOUSEACTIVATE => {
                let previous = GetForegroundWindow();
                if previous != state.surface { state.previous_foreground = previous; }
            }
            0x010d => state.composing = true,
            0x010e => state.composing = false,
            0x0100 if wparam == 13 && !state.composing && GetKeyState(0x10) >= 0 => {
                if lparam & (1 << 30) == 0 { PostMessageW(state.owner, WM_APP_SEND_REPLY, 0, 0); }
                return 0;
            }
            0x0100 if wparam == 27 && !state.composing => {
                PostMessageW(state.owner, WM_APP_CLOSE_CHAT, 0, 0); return 0;
            }
            0x0100 if wparam == 65 && GetKeyState(0x11) < 0 => { SendMessageW(window, 0x00b1, 0, -1); return 0; }
            0x0102 if (wparam == 27 || (wparam == 13 && GetKeyState(0x10) >= 0)) && !state.composing => return 0,
            _ => {}
        }
        CallWindowProcW(state.original, window, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "displays only an owned input; never sends to a real conversation"]
    fn native_centered_chat_keeps_the_same_input_and_clears_on_send() {
        unsafe {
            let owner = CreateWindowExW(WS_EX_TOOLWINDOW, wide("STATIC").as_ptr(), wide("Chat fixture").as_ptr(),
                WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), GetModuleHandleW(null()), null());
            let mut composer = Composer::new(owner).unwrap();
            let target = CardTarget { window: 0, project: None, session_id: "fixture-chat".into() };
            let rect = Rect { left: 10, top: 10, right: 210, bottom: 38 };
            composer.sync(rect, 96, target.clone()).unwrap();
            composer.set_text("Draft");
            let style = GetWindowLongPtrW(composer.window, -16);
            composer.expand(false, false).unwrap();
            composer.history = None;
            assert!(composer.expanded());
            assert_eq!(GetWindowLongPtrW(composer.window, -16), style, "expansion must not create window chrome");
            assert_eq!(GetWindowLongPtrW(composer.window, -8), owner as isize, "input stays owned by the same overlay");
            let mut keys = [0u8; 256]; GetKeyboardState(keys.as_mut_ptr()); let saved = keys;
            keys[0x10] = 0x80; SetKeyboardState(keys.as_ptr());
            SendMessageW(composer.edit.input, 0x0102, 13, 0); SetKeyboardState(saved.as_ptr());
            assert_eq!(composer.text(), "Draft\r\n");
            composer.set_text("Draft");
            let input_style = GetWindowLongPtrW(composer.window, -20);
            composer.begin_growth().unwrap();
            composer.sync(Rect { right: 610, ..rect }, 96, target.clone()).unwrap();
            assert_eq!(composer.bounds, Some(rect), "growth must not repeatedly resize the live input");
            SendMessageW(composer.edit.input, 0x0102, b'!' as usize, 0);
            assert_eq!(composer.text(), "Draft!", "input still accepts keystrokes during cached growth");
            composer.finish_growth();
            composer.sync(Rect { right: 610, ..rect }, 96, target.clone()).unwrap();
            assert_eq!(composer.bounds, Some(Rect { right: 610, ..rect }));
            assert_eq!(GetWindowLongPtrW(composer.window, -20), input_style);
            composer.set_text("Draft");
            composer.send_expanded(); assert!(composer.text().is_empty());
            composer.set_text("Next draft"); composer.edited();
            let deadline = Instant::now() + Duration::from_secs(2);
            while composer.sending() && Instant::now() < deadline { composer.poll(); thread::sleep(Duration::from_millis(10)); }
            assert!(!composer.sending());
            assert_eq!(composer.text(), "Draft\r\n\r\nNext draft");
            composer.begin_growth().unwrap();
            composer.collapse_chat(); composer.sync(rect, 96, target).unwrap();
            assert!(!composer.animating);
            assert_eq!(GetWindowLongPtrW(composer.window, -20), input_style, "Escape must restore normal input presentation");
            assert!(composer.text().ends_with("Next draft"));
            drop(composer); DestroyWindow(owner);
        }
    }

    #[test]
    #[ignore = "briefly displays an owned edit control; never sends to Codex"]
    fn native_reply_success_clears_only_the_sent_draft_and_failure_preserves_text() {
        unsafe {
            let owner = CreateWindowExW(WS_EX_TOOLWINDOW, wide("STATIC").as_ptr(), wide("Reply fixture").as_ptr(),
                WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), GetModuleHandleW(null()), null());
            assert!(!owner.is_null());
            let foreground = GetForegroundWindow();
            let mut composer = Composer::new(owner).unwrap();
            let first = CardTarget { window: 0, project: None, session_id: "first".into() };
            let second = CardTarget { session_id: "second".into(), ..first.clone() };
            let rect = Rect { left: 10, top: 10, right: 210, bottom: 38 };
            composer.sync(rect, 96, first.clone()).unwrap();
            composer.set_text("First draft"); composer.save();
            let (acknowledge, result) = mpsc::channel();
            composer.pending = Some(("first".into(), "First draft".into(), result));
            composer.set_text(""); composer.save();
            composer.set_text("Next draft");
            composer.send(first.clone(), false, false);
            assert!(composer.sending(), "a second send cannot replace an in-flight submission");
            assert_eq!(composer.text(), "Next draft", "an overlapping send must keep the next draft");
            assert!(composer.status("first").unwrap().contains("previous message"));
            composer.sync(rect, 96, second.clone()).unwrap();
            composer.set_text("Second draft"); composer.save();
            acknowledge.send(Ok(())).unwrap();
            assert!(composer.poll());
            assert_eq!(composer.text(), "Second draft");
            composer.sync(rect, 96, first).unwrap();
            assert_eq!(composer.text(), "Next draft", "acknowledging a send must preserve the next draft");
            composer.sync(rect, 96, second).unwrap();
            assert_eq!(composer.text(), "Second draft");
            let (acknowledge, result) = mpsc::channel();
            composer.pending = Some(("second".into(), "Second draft".into(), result));
            composer.set_text("New typing"); composer.save();
            acknowledge.send(Err("Test rejection".into())).unwrap();
            composer.poll();
            assert_eq!(composer.text(), "Second draft\r\n\r\nNew typing");
            assert_eq!(composer.status("second"), Some("Test rejection"));
            assert_eq!(GetForegroundWindow(), foreground, "showing and updating must not activate the editor");
            drop(composer); DestroyWindow(owner);
        }
    }
}
