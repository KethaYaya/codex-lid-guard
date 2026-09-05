//! A layered, topmost tool window. Only explicit card clicks activate an editor.
use super::overlay_shortcuts::{ShortcutPublisher, WM_OVERLAY_SHORTCUT};
use super::overlay_window_events::{WindowEvents, Transitions, WM_FRAME_READY, WM_FOREGROUND, WM_MINIMIZE, WM_RESTORE, WM_OPEN_STARTED, event_instant};
use super::*;
use crate::overlay::{Card, CardTarget, Frame};
use std::time::Instant;

#[path = "overlay_motion.rs"]
mod overlay_motion;
use overlay_motion::{AnimatedRow, DockMotion, Motion, MotionClock, OpenMotion};
#[path = "overlay_dock.rs"]
mod overlay_dock;
use overlay_dock::{DockLayout, arrival_layout, dock_layout, opening_bounds, opening_target};
#[path = "overlay_open_surface.rs"]
mod overlay_open_surface;
use overlay_open_surface::{FrameSurface, OpenSurface, reset_layered_mode};
#[path = "overlay_frame_timer.rs"]
mod overlay_frame_timer;
use overlay_frame_timer::FrameTimer;
#[cfg(test)]
#[path = "overlay_open_tests.rs"]
mod open_tests;
#[cfg(test)]
#[path = "overlay_close_tests.rs"]
mod close_tests;
#[cfg(test)]
#[path = "overlay_cycle_tests.rs"]
mod cycle_tests;

const WS_EX_TOPMOST: u32 = 0x0000_0008;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_CAPTURECHANGED: u32 = 0x0215;
const WM_SETCURSOR: u32 = 0x0020;
const WM_APP_OPEN_OVERLAY_CARD: u32 = 0x8002;
const WM_APP_EXPAND_OVERLAY: u32 = 0x8003;
const WM_APP_CLOSE_OVERLAY: u32 = 0x800c;
const WM_APP_COLLAPSE_OVERLAY: u32 = 0x800d;
const WM_TIMER: u32 = 0x0113;
const WM_MOUSEACTIVATE: u32 = 0x0021;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_SHOWWINDOW: u32 = 0x0040;
const SWP_NOCOPYBITS: u32 = 0x0100;
const DT_WORDBREAK: u32 = 0x0010;
const DT_CALCRECT: u32 = 0x0400;
const DT_EDITCONTROL: u32 = 0x2000;

#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetTimer(window: Hwnd, id: usize, milliseconds: u32, callback: *const c_void) -> usize;
    fn KillTimer(window: Hwnd, id: usize) -> Bool;
    fn SetWindowPos(
        window: Hwnd,
        after: Hwnd,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> Bool;
    fn MonitorFromWindow(window: Hwnd, flags: u32) -> Handle;
    fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> Bool;
    fn SetThreadDpiAwarenessContext(context: Handle) -> Handle;
    fn SetWindowRgn(window: Hwnd, region: Handle, redraw: Bool) -> i32;
    fn GetDC(window: Hwnd) -> Handle;
    fn ReleaseDC(window: Hwnd, dc: Handle) -> i32;
    fn SetCapture(window: Hwnd) -> Hwnd;
    fn ReleaseCapture() -> Bool;
    fn SetCursor(cursor: Handle) -> Handle;
    fn ScreenToClient(window: Hwnd, point: *mut Point) -> Bool;
    fn GetDoubleClickTime() -> u32;
    fn SetWindowTextW(window: Hwnd, text: *const u16) -> Bool;
    fn SystemParametersInfoW(action: u32, parameter: u32, value: *mut c_void, flags: u32) -> Bool;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: Handle) -> Handle;
    fn CreateCompatibleBitmap(dc: Handle, width: i32, height: i32) -> Handle;
    fn DeleteDC(dc: Handle) -> Bool;
    fn BitBlt(
        destination: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Handle,
        source_x: i32,
        source_y: i32,
        operation: u32,
    ) -> Bool;
    fn StretchBlt(
        destination: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Handle,
        source_x: i32,
        source_y: i32,
        source_width: i32,
        source_height: i32,
        operation: u32,
    ) -> Bool;
    fn SetStretchBltMode(dc: Handle, mode: i32) -> i32;
    fn SaveDC(dc: Handle) -> i32;
    fn RestoreDC(dc: Handle, saved: i32) -> Bool;
    fn SetViewportOrgEx(dc: Handle, x: i32, y: i32, previous: *mut Point) -> Bool;
    fn CreateRectRgn(left: i32, top: i32, right: i32, bottom: i32) -> Handle;
    fn CombineRgn(destination: Handle, first: Handle, second: Handle, mode: i32) -> i32;
    fn IntersectClipRect(dc: Handle, left: i32, top: i32, right: i32, bottom: i32) -> i32;
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
    fn CreateRoundRectRgn(
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        width: i32,
        height: i32,
    ) -> Handle;
}

struct OverlayState {
    cards: Vec<Card>,
    heights: Vec<i32>,
    rows: Vec<AnimatedRow>,
    font: Handle,
    dpi: u32,
    clicks: ClickTracker,
    pending_target: Option<CardTarget>,
    collapsed: bool,
    hover_open: Option<HoverOpen>,
    tab_pressed: bool,
    close_pressed: bool,
    activity: u64,
    layout: Option<DockLayout>,
    busy: bool,
    attention: bool,
    animate: bool,
    activity_started: Instant,
    buffer: PaintBuffer,
    panel_buffer: PaintBuffer,
    panel_dirty: bool,
    panel_size: (i32, i32),
    shortcut_code: Option<[u8; 2]>,
    shortcut_token: usize,
    restoring: bool,
    compositor: Option<FrameSurface>,
    render_alpha: u8,
}

struct Opening {
    target: CardTarget,
    request: OverlayOpen,
    result: Option<bool>,
    started: Instant,
    motion: OpenMotion,
    from: Rect,
    to: Rect,
    alpha: u8,
    surface: Option<OpenSurface>,
}

struct HoverOpen {
    anchor: Rect,
    outside_since: Option<Instant>,
}

struct OpenedOverlay {
    target: CardTarget,
    dock_request: u64,
}

impl OpenedOverlay {
    fn suppresses(&self, frame: &Frame) -> bool {
        frame.dock_request == self.dock_request
            && frame
                .cards
                .iter()
                .any(|card| card.target.as_ref() == Some(&self.target))
    }
}

impl HoverOpen {
    fn new(anchor: Rect) -> Self {
        Self {
            anchor,
            outside_since: None,
        }
    }

    fn should_collapse(&mut self, panel: Rect, pointer: Option<(i32, i32)>, now: Instant) -> bool {
        // Include the original tab and the gap to the panel. Sliding the window away
        // from a stationary pointer must not repeatedly close and reopen it.
        let area = Rect {
            left: self.anchor.left.min(panel.left),
            top: self.anchor.top.min(panel.top),
            right: self.anchor.right.max(panel.right),
            bottom: self.anchor.bottom.max(panel.bottom),
        };
        let inside = pointer.is_none_or(|(x, y)| {
            x >= area.left && x < area.right && y >= area.top && y < area.bottom
        });
        if inside {
            self.outside_since = None;
            false
        } else {
            now.saturating_duration_since(*self.outside_since.get_or_insert(now))
                >= Duration::from_millis(200)
        }
    }
}

fn pointer_position() -> Option<(i32, i32)> {
    unsafe {
        let mut point: Point = zeroed();
        (GetCursorPos(&mut point) != 0).then_some((point.x, point.y))
    }
}

unsafe fn cancel_hover(window: Hwnd, state: &mut OverlayState) {
    if state.hover_open.take().is_some() {
        unsafe {
            KillTimer(window, 5);
        }
    }
}

#[derive(Default)]
struct PaintBuffer {
    dc: Handle,
    bitmap: Handle,
    original: Handle,
    width: i32,
    height: i32,
}

impl PaintBuffer {
    unsafe fn get(&mut self, reference: Handle, width: i32, height: i32) -> Handle {
        unsafe {
            if !self.dc.is_null() && self.width >= width && self.height >= height {
                return self.dc;
            }
            let dc = CreateCompatibleDC(reference);
            let bitmap = CreateCompatibleBitmap(
                reference,
                width.max(self.width).max(1),
                height.max(self.height).max(1),
            );
            if dc.is_null() || bitmap.is_null() {
                if !dc.is_null() {
                    DeleteDC(dc);
                }
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                return reference;
            }
            let original = SelectObject(dc, bitmap);
            let next = Self {
                dc,
                bitmap,
                original,
                width: width.max(self.width),
                height: height.max(self.height),
            };
            *self = next;
            dc
        }
    }
}

impl Drop for PaintBuffer {
    fn drop(&mut self) {
        unsafe {
            if !self.dc.is_null() {
                SelectObject(self.dc, self.original);
                DeleteObject(self.bitmap);
                DeleteDC(self.dc);
            }
        }
    }
}

fn completion_pulse(elapsed: Duration, animate: bool) -> f32 {
    if !animate {
        return 0.65;
    }
    let phase = (elapsed.as_secs_f64() % 1.8) / 1.8 * std::f64::consts::TAU;
    (0.5 * (1.0 - phase.cos())) as f32
}

fn completion_strength(elapsed: Duration, animate: bool) -> f32 {
    if !animate {
        return 0.72;
    }
    0.55 + 0.40 * completion_pulse(elapsed, animate)
}

fn busy_strength(elapsed: Duration, dot: usize, animate: bool) -> f32 {
    if !animate {
        return 0.8;
    }
    let phase = ((elapsed.as_secs_f64() % 1.2) / 1.2 - dot as f64 / 3.0) * std::f64::consts::TAU;
    (0.25 + 0.7 * ((phase.cos() + 1.0) / 2.0).powi(3)) as f32
}

fn needs_activity_timer(
    visible: bool,
    animate: bool,
    attention: bool,
    busy: bool,
    tab: bool,
) -> bool {
    visible && animate && (attention || (busy && tab))
}

#[derive(Clone, Debug, PartialEq)]
struct ClickedCard {
    id: u64,
    target: Option<CardTarget>,
}

#[derive(Default)]
struct ClickTracker {
    pressed: Option<ClickedCard>,
    opening: Option<ClickedCard>,
    pending: Vec<(ClickedCard, Instant)>,
}

impl ClickTracker {
    fn frozen(&self) -> bool {
        !self.pending.is_empty() || self.pressed.is_some()
    }

    fn press(&mut self, card: Option<ClickedCard>, double: bool) {
        self.opening = card
            .as_ref()
            .filter(|card| double && self.pending.iter().any(|(pending, _)| pending == *card))
            .cloned();
        if let Some(opening) = &self.opening {
            self.pending.retain(|(card, _)| card.id != opening.id);
        }
        self.pressed = card;
    }

    fn release(
        &mut self,
        released: Option<ClickedCard>,
        now: Instant,
        delay: Duration,
    ) -> Option<CardTarget> {
        let pressed = self.pressed.take();
        let opening = self.opening.take();
        if let Some(pressed) = pressed.filter(|pressed| released.as_ref() == Some(pressed)) {
            if opening.as_ref() == Some(&pressed) {
                return pressed.target;
            }
            self.pending.retain(|(card, _)| card.id != pressed.id);
            self.pending.push((pressed, now + delay));
        }
        None
    }

    fn due(&mut self, now: Instant) -> Vec<u64> {
        let ready = self
            .pending
            .iter()
            .filter(|(_, deadline)| now >= *deadline)
            .map(|(card, _)| card.id)
            .collect();
        self.pending.retain(|(_, deadline)| now < *deadline);
        ready
    }

    fn timer_delay(&self, now: Instant) -> Option<u32> {
        self.pending
            .iter()
            .map(|(_, deadline)| *deadline)
            .min()
            .map(|deadline| {
                deadline
                    .saturating_duration_since(now)
                    .as_nanos()
                    .div_ceil(1_000_000)
                    .clamp(1, u32::MAX as u128) as u32
            })
    }
}

unsafe fn arm_click_timer(window: Hwnd, clicks: &ClickTracker) {
    unsafe {
        if let Some(delay) = clicks.timer_delay(Instant::now()) {
            // The regular feed timer remains a fallback if Windows cannot allocate this timer.
            SetTimer(window, 3, delay, null());
        } else {
            KillTimer(window, 3);
        }
    }
}

#[cfg(test)]
fn run_message_overlay(
    next_frame: impl FnMut(bool) -> Frame,
    on_click: impl FnMut(&CardTarget) -> bool,
) -> io::Result<()> {
    run_overlay(None, next_frame, on_click, pointer_position)
}

pub fn run_session_overlay(
    slot: usize,
    next_frame: impl FnMut(bool) -> Frame,
    on_click: impl FnMut(&CardTarget, usize) -> OverlayOpen,
    shortcuts: Option<ShortcutPublisher>,
    updates: Option<OverlayUpdates>,
) -> io::Result<()> {
    assert!(slot < crate::overlay::SESSION_LIMIT);
    run_overlay_inner(
        Some(slot),
        next_frame,
        on_click,
        pointer_position,
        shortcuts,
        updates,
    )
}

#[cfg(test)]
fn run_overlay(
    slot: Option<usize>,
    next_frame: impl FnMut(bool) -> Frame,
    mut on_click: impl FnMut(&CardTarget) -> bool,
    pointer: impl FnMut() -> Option<(i32, i32)>,
) -> io::Result<()> {
    run_overlay_inner(slot, next_frame, |target, _| on_click(target).into(), pointer, None, None)
}

fn run_overlay_inner(
    slot: Option<usize>,
    mut next_frame: impl FnMut(bool) -> Frame,
    mut on_click: impl FnMut(&CardTarget, usize) -> OverlayOpen,
    mut pointer: impl FnMut() -> Option<(i32, i32)>,
    mut shortcuts: Option<ShortcutPublisher>,
    updates: Option<OverlayUpdates>,
) -> io::Result<()> {
    unsafe {
        SetThreadDpiAwarenessContext(-4isize as Handle);
        let instance = GetModuleHandleW(null());
        let class_name = wide(format!(
            "CodexLidGuardMessageOverlay.{}{}",
            GetCurrentProcessId(),
            slot.filter(|slot| *slot != 0)
                .map(|slot| format!(".{slot}"))
                .unwrap_or_default()
        ));
        let window_class = WindowClassExW {
            size: size_of::<WindowClassExW>() as u32,
            style: 0x0008, // CS_DBLCLKS: Windows applies the user's double-click settings.
            window_procedure: Some(window_procedure),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: null_mut(),
            cursor: LoadCursorW(null_mut(), 32_512usize as *const u16),
            background: null_mut(),
            menu_name: null(),
            class_name: class_name.as_ptr(),
            small_icon: null_mut(),
        };
        if RegisterClassExW(&window_class) == 0 {
            return Err(error("Register overlay window"));
        }
        // An editor owner would make Windows hide this window when that editor
        // minimizes. Keep the overlay independent and explicitly nonactivating.
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            wide("Codex session messages").as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            UnregisterClassW(class_name.as_ptr(), instance);
            return Err(error("Create overlay window"));
        }
        let mut state = Box::new(OverlayState {
            cards: vec![],
            heights: vec![],
            rows: vec![],
            font: null_mut(),
            dpi: 0,
            clicks: ClickTracker::default(),
            pending_target: None,
            collapsed: false,
            hover_open: None,
            tab_pressed: false,
            close_pressed: false,
            activity: 0,
            layout: None,
            busy: false,
            attention: false,
            animate: true,
            activity_started: Instant::now(),
            buffer: PaintBuffer::default(),
            panel_buffer: PaintBuffer::default(),
            panel_dirty: true,
            panel_size: (1, 1),
            shortcut_code: None,
            shortcut_token: 0,
            restoring: false,
            compositor: None,
            render_alpha: 255,
        });
        SetWindowLongPtrW(
            window,
            GWLP_USERDATA,
            (&mut *state as *mut OverlayState) as isize,
        );
        let result = (|| {
            let _events = if let Some(updates) = &updates {
                updates.attach(window);
                match WindowEvents::new(window) {
                    Ok(events) => Some(events),
                    Err(cause) => { logging::write(format!("Overlay window events unavailable: {cause}")); None }
                }
            } else { None };
            if SetTimer(window, 1, if updates.is_some() { 1000 } else { 250 }, null()) == 0 {
                return Err(error("Start overlay timer"));
            }
            let mut previous_bounds = None;
            let mut previous_opacity = None;
            let mut visible = false;
            let mut displayed_window = None;
            let mut displayed_session = None;
            let mut work: Rect = zeroed();
            let mut width = 1;
            let mut opacity = 82;
            let mut position = String::from("bottom-right");
            let mut animate = true;
            let mut closing = false;
            let mut refresh = true;
            let mut frame_timer = FrameTimer::new()?;
            let mut activity_timer = false;
            let started = Instant::now();
            let mut clock = MotionClock::new(started);
            let mut motion = Motion::new(started);
            let mut dock = DockMotion::new(started);
            let mut dock_center = None;
            let mut dock_request = 0;
            let mut arrival = None;
            let mut opened_overlay: Option<OpenedOverlay> = None;
            let mut dismissed_overlay: Option<(String, u64)> = None;
            let mut opening: Option<Opening> = None;
            let mut transitions = Transitions::new(GetForegroundWindow() as usize as u64);
            let mut last_transition = None;
            let mut awaiting_dock_request = None;
            let mut hidden_in_focus = false;
            loop {
                let real = Instant::now();
                let now = clock.advance(real, state.clicks.frozen() || state.tab_pressed || state.close_pressed || opening.is_some());
                if let Some(open) = &mut opening {
                    if open.result.is_none() { open.result = open.request.poll(); }
                    // Fallback for already focused editors or missing native events.
                    if open.result == Some(true) { open.motion.begin(real); }
                    if open.result.is_none() && real.duration_since(open.started) >= super::overlay_open::OPEN_TIMEOUT {
                        open.result = Some(false);
                    }
                    if open.result == Some(false) || (open.result == Some(true) && open.motion.finished(real)) {
                        if open.result == Some(true) {
                            opened_overlay = Some(OpenedOverlay { target: open.target.clone(), dock_request });
                            state.cards.clear();
                            state.rows.clear();
                            state.heights.clear();
                            state.layout = None;
                            motion = Motion::new(now);
                            ShowWindow(window, 0);
                            visible = false;
                        }
                        reset_layered_mode(window);
                        opening = None;
                        state.restoring = false;
                        state.layout = None; // Recreate the ordinary rounded region on failure too.
                        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) & !0x20);
                        previous_bounds = None;
                        previous_opacity = None;
                        state.panel_dirty = true;
                        refresh = true;
                    }
                }
                let mut activity_changed = false;
                if !state.clicks.due(real).is_empty() {
                    cancel_hover(window, &mut state);
                    KillTimer(window, 3);
                    if dock_center.is_none() {
                        dock_center = state.layout.map(|layout| {
                            let anchor = layout.tab.or(layout.panel).unwrap();
                            layout.window.top + (anchor.top + anchor.bottom) / 2
                        });
                    }
                    state.collapsed = true;
                    state.clicks = ClickTracker::default();
                    refresh = true;
                }
                if refresh && !closing && opening.is_none() {
                    let mut frame = next_frame(state.collapsed);
                    if let Some((session, activity)) = &dismissed_overlay {
                        if frame.session_id.as_ref() == Some(session) && frame.activity == *activity {
                            frame.cards.clear();
                        } else {
                            dismissed_overlay = None;
                        }
                    }
                    if state.activity != frame.activity {
                        state.close_pressed = false;
                    }
                    state.activity = frame.activity;
                    let transition = transitions.for_window(frame.window, real);
                    let new_transition = transition.is_some() && transition != last_transition;
                    if new_transition {
                        // Focus/minimize is newer evidence than a cached explicit-open latch.
                        opened_overlay = None;
                    }
                    let hidden = frame.hidden_in_focus && transition.is_none()
                        && frame.window.is_some_and(is_window_focused);
                    let became_background = hidden_in_focus && !hidden;
                    hidden_in_focus = hidden;
                    if hidden {
                        frame.cards.clear();
                    }
                    if let Some(opened) = &opened_overlay {
                        if opened.suppresses(&frame) {
                            // Ignore cached frames until the worker processes the open,
                            // or a later focus loss makes this chat eligible again.
                            frame.cards.clear();
                        } else {
                            opened_overlay = None;
                        }
                    }
                    if displayed_session != frame.session_id {
                        cancel_hover(window, &mut state);
                        // A replaced lane must never inherit another chat's click or dock state.
                        if state.clicks.pressed.is_some() || state.tab_pressed || state.close_pressed {
                            ReleaseCapture();
                        }
                        state.clicks = ClickTracker::default();
                        state.pending_target = None;
                        state.close_pressed = false;
                        state.collapsed = false;
                        state.tab_pressed = false;
                        state.layout = None;
                        state.cards.clear();
                        state.rows.clear();
                        state.heights.clear();
                        KillTimer(window, 3);
                        motion = Motion::new(now);
                        dock = DockMotion::new(now);
                        dock_center = None;
                        previous_bounds = None;
                        displayed_session = frame.session_id.clone();
                        dock_request = 0;
                        arrival = None;
                        awaiting_dock_request = None;
                    }
                    closing = frame.close;
                    let mut enabled: Bool = 1;
                    // SPI_GETCLIENTAREAANIMATION follows Windows accessibility preferences.
                    if SystemParametersInfoW(0x1042, 0, (&mut enabled as *mut Bool).cast(), 0) != 0
                    {
                        animate = enabled != 0;
                    }
                    if closing {
                        frame.cards.clear();
                    }
                    activity_changed = state.busy != frame.busy
                        || state.attention != frame.attention
                        || state.animate != animate;
                    state.busy = frame.busy;
                    state.attention = frame.attention;
                    state.animate = animate;
                    // The event already docked the cached frame. A later reader epoch is
                    // its acknowledgement, not a second minimize that cancels a hover.
                    let confirms_event = awaiting_dock_request.is_some_and(|(origin, request)|
                        Some(origin) == frame.window && request != frame.dock_request);
                    if confirms_event { awaiting_dock_request = None; }
                    let auto_dock = new_transition || (became_background && frame.dock_request != 0) || (!confirms_event
                        && frame.dock_request != 0 && dock_request != frame.dock_request);
                    if new_transition {
                        last_transition = transition;
                        awaiting_dock_request = frame.window.map(|origin| (origin, frame.dock_request));
                    }
                    dock_request = frame.dock_request;
                    if auto_dock {
                        let already_tucked = state.collapsed && dock.sample(now).0 == 1.0 && arrival.is_none();
                        cancel_hover(window, &mut state);
                        if state.clicks.pressed.is_some() || state.tab_pressed || state.close_pressed {
                            ReleaseCapture();
                        }
                        state.clicks = ClickTracker::default();
                        KillTimer(window, 3);
                        state.tab_pressed = false;
                        state.close_pressed = false;
                        state.collapsed = true;
                        dock.target(true, now, false);
                        dock_center = None;
                        // Start at the OS event time even if delivery or the reader was late.
                        // A finished system transition must not replay a late full-size panel.
                        let start = transition.map_or(now, |event|
                            now.checked_sub(real.saturating_duration_since(event.started)).unwrap_or(now));
                        // SPI_GETANIMATION is separate from client-area animations.
                        let mut animation = [8u32, 1]; // ANIMATIONINFO { cbSize, iMinAnimate }
                        let minimize_animated = SystemParametersInfoW(0x0048, animation[0],
                            animation.as_mut_ptr().cast(), 0) == 0 || animation[1] != 0;
                        arrival = (animate && minimize_animated && !already_tucked).then_some(start);
                    }
                    // Freeze content and its anchor between the first and second click.
                    if !frame.cards.is_empty() && state.clicks.frozen() {
                        frame.cards = state.cards.clone();
                        frame.window = displayed_window;
                    } else if !frame.cards.is_empty()
                        && displayed_window.is_some()
                        && dock.sample(now).0 > 0.0
                    {
                        // New messages must not move a tucked-away tab to another display.
                        frame.window = displayed_window;
                    } else {
                        displayed_window = frame.window;
                    }
                    if frame.cards.is_empty() {
                        state.cards.clear();
                        state.heights.clear();
                        state.clicks = ClickTracker::default();
                        KillTimer(window, 3);
                    } else {
                        let anchor = frame
                            .window
                            .map(|value| value as usize as Hwnd)
                            .unwrap_or(GetForegroundWindow());
                        let mut info: MonitorInfo = zeroed();
                        info.size = size_of::<MonitorInfo>() as u32;
                        if GetMonitorInfoW(MonitorFromWindow(anchor, 2), &mut info) == 0 {
                            return Err(error("Read overlay monitor"));
                        }
                        let dpi = GetDpiForWindow(anchor).max(96);
                        let font_changed = state.dpi != dpi;
                        if font_changed {
                            let font = CreateFontW(
                                -scale_dip(14, dpi),
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
                            );
                            if font.is_null() {
                                return Err(error("Create overlay font"));
                            }
                            if !state.font.is_null() {
                                DeleteObject(state.font);
                            }
                            state.font = font;
                            state.dpi = dpi;
                        }
                        let margin = scale_dip(20, dpi);
                        let next_width = scale_dip(440, dpi)
                            .min((info.work.right - info.work.left - margin * 2).max(1));
                        let next_work = slot
                            .map(|slot| session_work_area(info.work, slot, dpi, &frame.position))
                            .unwrap_or(info.work);
                        let changed = font_changed
                            || state.cards != frame.cards
                            || width != next_width
                            || work != next_work;
                        work = next_work;
                        width = next_width;
                        opacity = frame.opacity;
                        position = frame.position;
                        if changed {
                            if let Some(card) = frame.cards.first() {
                                SetWindowTextW(window, wide(&card.label).as_ptr());
                            }
                            state.cards = frame.cards;
                            let dc = GetDC(window);
                            if dc.is_null() {
                                return Err(error("Measure overlay text"));
                            }
                            let old_font = SelectObject(dc, state.font);
                            state.heights = state
                                .cards
                                .iter()
                                .map(|card| {
                                    let mut rect = Rect {
                                        left: 0,
                                        top: 0,
                                        right: (width - scale_dip(36, dpi)).max(1),
                                        bottom: 0,
                                    };
                                    let text = wide(&card.text);
                                    DrawTextW(
                                        dc,
                                        text.as_ptr(),
                                        wide_text_length(&text),
                                        &mut rect,
                                        DT_WORDBREAK | DT_NOPREFIX | DT_CALCRECT,
                                    );
                                    let available = ((work.bottom
                                        - work.top
                                        - margin * 2
                                        - scale_dip(64, dpi))
                                        / state.cards.len().max(1) as i32
                                        - scale_dip(42, dpi))
                                    .clamp(scale_dip(20, dpi), scale_dip(120, dpi));
                                    (rect.bottom - rect.top).clamp(scale_dip(20, dpi), available)
                                        + scale_dip(42, dpi)
                                })
                                .collect();
                            SelectObject(dc, old_font);
                            ReleaseDC(window, dc);
                        }
                    }
                    motion.sync(&state.cards, &state.heights, now, animate && !auto_dock);
                }
                let arrival_progress = arrival.and_then(|started| {
                    let p = now.saturating_duration_since(started).as_secs_f32() / 0.260;
                    (animate && p < 1.0).then_some(1.0 - (1.0 - p).powi(3))
                });
                if arrival_progress.is_none() {
                    arrival = None;
                }
                dock.target(state.collapsed, now, animate);
                let (docked, docking) = dock.sample(now);
                if !state.collapsed && !docking {
                    dock_center = None;
                }
                let (panel, rows, moving) = motion.sample(now);
                let mut repaint = state.rows != rows || activity_changed;
                state.panel_dirty |= repaint;
                state.rows = rows;
                if let Some(shortcuts) = &mut shortcuts {
                    if !closing && !state.restoring && !state.rows.is_empty() && !state.cards.is_empty() {
                        let card = &state.cards[0];
                        let (code, token) = shortcuts.publish(
                            window as usize,
                            card.target
                                .as_ref()
                                .map(|target| target.window)
                                .unwrap_or(0),
                            displayed_session.as_deref().unwrap_or("preview"),
                            &card.label,
                        );
                        repaint |= state.shortcut_code != Some(code);
                        state.panel_dirty |= state.shortcut_code != Some(code);
                        state.shortcut_code = Some(code);
                        state.shortcut_token = token;
                    } else {
                        shortcuts.clear();
                        state.shortcut_token = 0;
                    }
                }
                if state.rows.is_empty() {
                    cancel_hover(window, &mut state);
                    state.collapsed = false;
                    state.tab_pressed = false;
                    state.layout = None;
                    dock.target(false, now, false);
                    if visible {
                        ShowWindow(window, 0);
                        visible = false;
                    }
                    if state.compositor.take().is_some() {
                        // Hide before resetting alpha presentation to avoid flashing
                        // the final transparent frame at full opacity.
                        reset_layered_mode(window);
                        previous_opacity = None;
                        previous_bounds = None;
                    }
                    if closing {
                        break;
                    }
                } else if let Some(open) = &mut opening {
                    if let Some(surface) = &mut open.surface {
                        let (growth, fade, _) = open.motion.sample(real);
                        let bounds = opening_bounds(open.from, open.to, growth);
                        let alpha = (open.alpha as f32 * fade).round() as u8;
                        if let Err(cause) = surface.present(window, bounds, alpha) {
                            // Animation failure must never interrupt editor activation.
                            logging::write(format!("Could not animate editor restore: {cause}"));
                            reset_layered_mode(window);
                            ShowWindow(window, 0);
                            visible = false;
                            open.surface = None;
                            open.motion = OpenMotion::new(real, false);
                            PostMessageW(window, WM_FRAME_READY, 0, 0);
                        }
                    }
                } else {
                    let dpi = state.dpi.max(96);
                    let margin = scale_dip(20, dpi);
                    let height = (scale_dip(64, dpi)
                        + state.rows.iter().map(|row| row.height).sum::<i32>())
                    .min((work.bottom - work.top - margin * 2).max(1));
                    if state.panel_size != (width, height) {
                        state.panel_dirty = true;
                        state.panel_size = (width, height);
                    }
                    let mut bounds = overlay_bounds(work, width, height, margin, &position);
                    let slide = ((1.0 - panel) * scale_dip(12, dpi) as f32).round() as i32
                        * if position.starts_with("top") { -1 } else { 1 };
                    bounds.top += slide;
                    bounds.bottom += slide;
                    let layout = if let Some(progress) = arrival_progress {
                        arrival_layout(bounds, work, progress, dpi)
                    } else {
                        dock_layout(bounds, work, docked, dpi, dock_center)
                    };
                    let shape_changed = state.layout != Some(layout);
                    state.layout = Some(layout);
                    let bounds = layout.window;
                    let window_width = bounds.right - bounds.left;
                    let window_height = bounds.bottom - bounds.top;
                    let alpha =
                        (panel * (opacity.clamp(30, 100) as u16 * 255 / 100) as f32).round() as u8;
                    let compositor_started = state.compositor.is_none() && docking && arrival.is_none();
                    if compositor_started {
                        state.compositor = Some(FrameSurface::new(
                            window_width.max(width + scale_dip(28, dpi)),
                            window_height.max(work.bottom - work.top))?);
                        reset_layered_mode(window);
                        SetWindowRgn(window, null_mut(), 0);
                    }
                    if state.compositor.is_some() {
                        // Keep this presentation mode after the slide settles. Switching
                        // back to a resized WM_PAINT surface added a second visible step.
                        state.render_alpha = alpha;
                        if compositor_started || repaint || shape_changed
                            || previous_opacity != Some(alpha) || previous_bounds != Some(bounds) {
                            let dc = GetDC(window);
                            let result = paint_composited_frame(window, dc, &mut state);
                            ReleaseDC(window, dc);
                            result?;
                        }
                        previous_opacity = Some(alpha);
                        previous_bounds = Some(bounds);
                        if !visible {
                            ShowWindow(window, 4); // SW_SHOWNOACTIVATE; geometry is already presented.
                            visible = true;
                        }
                    } else {
                        if previous_opacity != Some(alpha) {
                            if SetLayeredWindowAttributes(window, 0, alpha, LWA_ALPHA) == 0 {
                                return Err(error("Set overlay opacity"));
                            }
                            previous_opacity = Some(alpha);
                        }
                        let bounds_changed = previous_bounds != Some(bounds);
                        if shape_changed {
                            apply_overlay_region(window, layout, dpi)?;
                        }
                        if bounds_changed || !visible {
                            if SetWindowPos(
                                window,
                                -1isize as Hwnd,
                                bounds.left,
                                bounds.top,
                                window_width,
                                window_height,
                                // Every frame is buffered and repainted; old client pixels
                                // must not be copied to a different origin during resizing.
                                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOCOPYBITS | 0x0008 | 0x0400,
                            ) == 0
                            {
                                return Err(error("Position overlay window"));
                            }
                            previous_bounds = Some(bounds);
                            visible = true;
                        }
                        if repaint || bounds_changed || shape_changed {
                            InvalidateRect(window, null(), 0);
                            UpdateWindow(window);
                        }
                    }
                }
                // Fast ticks only draw animation; transcript/settings polling stays at 250 ms.
                let needs_timer = opening.as_ref().map_or(moving || docking || arrival.is_some(),
                    |open| open.motion.sample(real).2)
                    && !state.clicks.frozen()
                    && !state.tab_pressed;
                frame_timer.update(needs_timer)?;
                let needs_activity = needs_activity_timer(
                    visible,
                    animate && !needs_timer && opening.is_none(),
                    state.attention,
                    state.busy,
                    state.layout.is_some_and(|layout| layout.tab.is_some()),
                );
                if needs_activity != activity_timer {
                    if needs_activity {
                        if SetTimer(window, 4, 33, null()) == 0 {
                            return Err(error("Start overlay activity timer"));
                        }
                    } else {
                        KillTimer(window, 4);
                    }
                    activity_timer = needs_activity;
                }
                loop {
                    let Some(mut message) = frame_timer.message()? else {
                        refresh = false;
                        break;
                    };
                    if message.message == 0x0012 {
                        // WM_QUIT
                        return Ok(());
                    }
                    if opening.is_some() && matches!(message.message,
                        WM_OVERLAY_SHORTCUT | WM_APP_OPEN_OVERLAY_CARD | WM_APP_EXPAND_OVERLAY | WM_APP_CLOSE_OVERLAY | WM_APP_COLLAPSE_OVERLAY) {
                        continue;
                    }
                    if message.message == WM_OPEN_STARTED {
                        if let Some(open) = &mut opening
                            && open.target.window == message.wparam as u64 {
                            let at = event_instant(message.lparam as u32);
                            if at + Duration::from_millis(2) >= open.started { open.motion.begin(at); }
                        }
                        refresh = false;
                        break;
                    }
                    if matches!(message.message, WM_FOREGROUND | WM_MINIMIZE | WM_RESTORE) {
                        if message.message == WM_MINIMIZE && let Some(open) = &mut opening
                            && open.target.window == message.wparam as u64 {
                            open.motion = OpenMotion::new(Instant::now(), false);
                        }
                        if matches!(message.message, WM_FOREGROUND | WM_RESTORE)
                            && awaiting_dock_request.is_some_and(|(origin, _)| origin == message.wparam as u64) {
                            awaiting_dock_request = None;
                        }
                        transitions.observe(message.message, message.wparam as u64,
                            event_instant(message.lparam as u32));
                        if let Some(updates) = &updates { updates.refresh(); }
                        // Render cached data immediately; no transcript or metadata I/O here.
                        refresh = true;
                        break;
                    }
                    if message.message == WM_FRAME_READY {
                        refresh = true;
                        break;
                    }
                    if message.message == WM_OVERLAY_SHORTCUT {
                        // Tokens bind queued input to this visible chat, never a replacement lane.
                        if !visible
                            || state.shortcut_token == 0
                            || message.wparam != state.shortcut_token
                        {
                            continue;
                        }
                        if message.lparam == 0 {
                            message.message = WM_APP_EXPAND_OVERLAY;
                            message.wparam = 0; // Keyboard expansion stays open, like a tab click.
                        } else if message.lparam == 1 {
                            state.pending_target =
                                state.cards.first().and_then(|card| card.target.clone());
                            message.message = WM_APP_OPEN_OVERLAY_CARD;
                        } else if message.lparam == 2 {
                            message.message = WM_APP_CLOSE_OVERLAY;
                            message.lparam = state.activity as isize;
                        } else if message.lparam == 3 {
                            message.message = WM_APP_COLLAPSE_OVERLAY;
                        } else {
                            continue;
                        }
                    }
                    if message.message == WM_APP_CLOSE_OVERLAY {
                        if !visible || message.wparam != state.shortcut_token
                            || message.lparam as u64 != state.activity || state.cards.is_empty() {
                            continue;
                        }
                        if let Some(session) = &displayed_session {
                            dismissed_overlay = Some((session.clone(), state.activity));
                        }
                        if let Some(updates) = &updates
                            && let Some(target) = state.cards.first().and_then(|card| card.target.clone()) {
                            updates.dismiss(target, state.activity);
                        }
                        cancel_hover(window, &mut state);
                        state.clicks = ClickTracker::default();
                        state.tab_pressed = false;
                        state.close_pressed = false;
                        KillTimer(window, 3);
                        ReleaseCapture();
                        refresh = true;
                        break;
                    }
                    if message.message == WM_TIMER {
                        if message.wparam == 5 {
                            if let Some(hover) = &mut state.hover_open {
                                let panel = state
                                    .layout
                                    .map(|layout| layout.window)
                                    .unwrap_or(hover.anchor);
                                if hover.should_collapse(panel, pointer(), Instant::now()) {
                                    dock_center =
                                        Some((hover.anchor.top + hover.anchor.bottom) / 2);
                                    cancel_hover(window, &mut state);
                                    state.collapsed = true;
                                    refresh = true;
                                    break;
                                }
                            }
                            // Cursor checks never poll transcripts, repaint, or resize the window.
                            continue;
                        }
                        if message.wparam == 4 {
                            // Paint only: no feed reads, text measurements, motion or window resizing.
                            if activity_timer {
                                state.panel_dirty = true;
                                paint_activity(window, &state);
                                if let Some(layout) = state.layout && let Some(frame) = &mut state.compositor {
                                    // Reuse the frame after painting indicators or the tiny tab.
                                    // Activity ticks never redraw or lay out message text.
                                    frame.present(window, state.buffer.dc, layout, state.dpi.max(96), state.render_alpha)?;
                                }
                            }
                            continue;
                        }
                        refresh = message.wparam == 1;
                        if message.wparam == 3 {
                            // Re-arm if a timer arrived just before its deadline.
                            arm_click_timer(window, &state.clicks);
                        }
                        break;
                    }
                    if message.message == WM_APP_OPEN_OVERLAY_CARD {
                        // Preserve the exact last painted pixels before launching activation.
                        // The worker signals when it is about to restore the real window.
                        if let Some(target) = state.pending_target.take() && let Some(layout) = state.layout {
                            let started = Instant::now();
                            let from = layout.window;
                            let to = opening_target(from, work, state.dpi.max(96));
                            let alpha = previous_opacity.unwrap_or(255);
                            let mut animation = [8u32, 1];
                            let system_animated = SystemParametersInfoW(0x0048, animation[0],
                                animation.as_mut_ptr().cast(), 0) == 0 || animation[1] != 0;
                            let mut surface = if animate && system_animated {
                                match OpenSurface::capture(state.buffer.dc, layout, state.dpi.max(96), to) {
                                    Ok(surface) => Some(surface),
                                    Err(cause) => { logging::write(format!("Could not prepare editor restore animation: {cause}")); None }
                                }
                            } else { None };
                            cancel_hover(window, &mut state);
                            state.clicks = ClickTracker::default();
                            KillTimer(window, 3);
                            state.tab_pressed = false;
                            ReleaseCapture();
                            arrival = None;
                            state.compositor = None;
                            state.restoring = true;
                            SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
                            let failed_surface = if let Some(surface) = &mut surface {
                                reset_layered_mode(window);
                                SetWindowRgn(window, null_mut(), 0);
                                if let Err(cause) = surface.present(window, from, alpha) {
                                    logging::write(format!("Could not start editor restore animation: {cause}"));
                                    reset_layered_mode(window);
                                    true
                                } else { false }
                            } else { false };
                            if failed_surface {
                                surface = None;
                                ShowWindow(window, 0);
                                visible = false;
                            }
                            KillTimer(window, 4);
                            activity_timer = false;
                            if let Some(shortcuts) = &mut shortcuts { shortcuts.clear(); }
                            state.shortcut_token = 0;
                            let request = on_click(&target, window as usize);
                            let result = request.poll();
                            opening = Some(Opening { target, request, result, started, from, to, alpha,
                                motion: OpenMotion::new(started, surface.is_some()), surface });
                        }
                        refresh = false;
                        break;
                    }
                    if message.message == WM_APP_COLLAPSE_OVERLAY {
                        if dock_center.is_none() {
                            dock_center = state.layout.map(|layout| {
                                let anchor = layout.tab.or(layout.panel).unwrap();
                                layout.window.top + (anchor.top + anchor.bottom) / 2
                            });
                        }
                        cancel_hover(window, &mut state);
                        if state.clicks.pressed.is_some() || state.tab_pressed || state.close_pressed {
                            ReleaseCapture();
                        }
                        state.collapsed = true;
                        state.tab_pressed = false;
                        state.close_pressed = false;
                        state.pending_target = None;
                        state.clicks = ClickTracker::default();
                        KillTimer(window, 3);
                        arrival = None;
                        refresh = false;
                        break;
                    }
                    if message.message == WM_APP_EXPAND_OVERLAY {
                        if message.wparam == 1 && !state.collapsed {
                            continue;
                        }
                        cancel_hover(window, &mut state);
                        if message.wparam == 1 {
                            state.hover_open =
                                state.layout.map(|layout| HoverOpen::new(layout.window));
                            if state.hover_open.is_some() && SetTimer(window, 5, 50, null()) == 0 {
                                return Err(error("Start overlay hover timer"));
                            }
                        }
                        state.collapsed = false;
                        arrival = None;
                        state.clicks = ClickTracker::default();
                        // The message is already cached. Input must not wait on monitor
                        // queries or text layout; report its state at the next feed tick.
                        refresh = false;
                        break;
                    }
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            Ok(())
        })();
        if let Some(updates) = &updates { updates.detach(); }
        KillTimer(window, 1);
        KillTimer(window, 2);
        KillTimer(window, 3);
        KillTimer(window, 4);
        KillTimer(window, 5);
        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        DestroyWindow(window);
        if !state.font.is_null() {
            DeleteObject(state.font);
        }
        UnregisterClassW(class_name.as_ptr(), instance);
        result
    }
}

fn card_at(state: &OverlayState, bounds: Rect, x: i32, y: i32) -> Option<usize> {
    let inset = scale_dip(12, state.dpi.max(96));
    if x < inset
        || x >= bounds.right - inset
        || y >= bounds.bottom - scale_dip(24, state.dpi.max(96))
    {
        return None;
    }
    let mut top = scale_dip(40, state.dpi.max(96));
    for (index, row) in state.rows.iter().enumerate() {
        if row.interactive
            && row.opacity > 0.1
            && y >= top
            && y < top + row.height - scale_dip(6, state.dpi.max(96))
        {
            return Some(index);
        }
        top += row.height;
    }
    None
}

fn card_target_at(_window: Hwnd, state: &OverlayState, x: i32, y: i32) -> Option<ClickedCard> {
    if state.collapsed {
        return None;
    }
    let panel = state.layout?.panel?;
    let bounds = Rect {
        left: 0,
        top: 0,
        right: panel.right - panel.left,
        bottom: panel.bottom - panel.top,
    };
    card_at(state, bounds, x - panel.left, y - panel.top).map(|index| ClickedCard {
        id: state.rows[index].card.id,
        target: state.rows[index].card.target.clone(),
    })
}

fn tab_at(state: &OverlayState, x: i32, y: i32) -> bool {
    state
        .layout
        .and_then(|layout| layout.tab)
        .is_some_and(|tab| x >= tab.left && x < tab.right && y >= tab.top && y < tab.bottom)
}

fn close_button_rect(width: i32, dpi: u32) -> Rect {
    Rect { left: width - scale_dip(36, dpi), right: width - scale_dip(8, dpi),
        top: scale_dip(8, dpi), bottom: scale_dip(36, dpi) }
}

fn close_at(state: &OverlayState, x: i32, y: i32) -> bool {
    if state.collapsed || state.cards.is_empty() { return false; }
    let Some(panel) = state.layout.and_then(|layout| layout.panel) else { return false; };
    let bounds = close_button_rect(panel.right - panel.left, state.dpi.max(96));
    let (x, y) = (x - panel.left, y - panel.top);
    x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom
}

unsafe fn create_overlay_region(layout: DockLayout, dpi: u32) -> io::Result<Handle> {
    unsafe {
        let region = CreateRectRgn(0, 0, 0, 0);
        if region.is_null() {
            return Err(error("Create overlay region"));
        }
        let result = (|| {
            for (rect, tab) in [(layout.panel, false), (layout.tab, true)] {
                if let Some(rect) = rect {
                    let radius = scale_dip(if tab { 12 } else { 18 }, dpi);
                    let part = CreateRoundRectRgn(
                        if tab {
                            rect.right - scale_dip(28, dpi)
                        } else {
                            rect.left
                        },
                        rect.top,
                        // Extend rounded right corners beyond the clipped display
                        // edge so a drawer stays attached even in its final pixels.
                        rect.right + if tab || layout.flush_right { radius } else { 1 },
                        rect.bottom + 1,
                        radius,
                        radius,
                    );
                    if part.is_null() {
                        return Err(error("Create overlay shape"));
                    }
                    if tab {
                        let clip = CreateRectRgn(rect.left, rect.top, rect.right, rect.bottom);
                        if clip.is_null() {
                            DeleteObject(part);
                            return Err(error("Create tab clip"));
                        }
                        let clipped = CombineRgn(part, part, clip, 1);
                        DeleteObject(clip);
                        if clipped == 0 {
                            DeleteObject(part);
                            return Err(error("Clip overlay tab"));
                        }
                    }
                    let combined = CombineRgn(region, region, part, 2); // RGN_OR
                    DeleteObject(part);
                    if combined == 0 {
                        return Err(error("Combine overlay shape"));
                    }
                }
            }
            let clip = CreateRectRgn(
                0,
                0,
                layout.window.right - layout.window.left,
                layout.window.bottom - layout.window.top,
            );
            if clip.is_null() {
                return Err(error("Create overlay clip"));
            }
            let combined = CombineRgn(region, region, clip, 1); // RGN_AND
            DeleteObject(clip);
            if combined == 0 {
                return Err(error("Clip overlay shape"));
            }
            Ok(region)
        })();
        if result.is_err() {
            DeleteObject(region);
        }
        result
    }
}

unsafe fn apply_overlay_region(window: Hwnd, layout: DockLayout, dpi: u32) -> io::Result<()> {
    unsafe {
        let region = create_overlay_region(layout, dpi)?;
        if SetWindowRgn(window, region, 0) == 0 {
            DeleteObject(region);
            return Err(error("Apply overlay shape"));
        }
        Ok(()) // Windows owns the region after a successful SetWindowRgn.
    }
}

fn overlay_bounds(work: Rect, width: i32, height: i32, margin: i32, position: &str) -> Rect {
    let left = if position.ends_with("left") {
        work.left + margin
    } else {
        work.right - width
    };
    let top = if position.starts_with("top") {
        work.top + margin
    } else {
        work.bottom - height - margin
    };
    Rect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

// Fixed lanes reserve space for expansion, so another chat's animation cannot move a tab.
fn session_work_area(work: Rect, slot: usize, dpi: u32, position: &str) -> Rect {
    let height = ((work.bottom - work.top) / crate::overlay::SESSION_LIMIT as i32)
        .min(scale_dip(272, dpi))
        .max(1);
    let top = if position.starts_with("top") {
        work.top + height * slot as i32
    } else {
        work.bottom - height * (slot as i32 + 1)
    };
    Rect {
        top,
        bottom: top + height,
        ..work
    }
}

fn tab_caption(cards: &[Card]) -> String {
    let label = cards
        .first()
        .map(|card| card.label.as_str())
        .unwrap_or("Chat");
    let title = label
        .rsplit_once('\u{2014}')
        .map(|(_, title)| title)
        .unwrap_or(label);
    let caption: String = title
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_uppercase)
        .take(2)
        .collect();
    if caption.is_empty() {
        "CH".into()
    } else {
        caption
    }
}

unsafe extern "system" fn window_procedure(
    window: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    unsafe {
        let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *const OverlayState;
        if state.as_ref().is_some_and(|state| state.restoring)
            && matches!(message, WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_SETCURSOR) {
            return 0;
        }
        match message {
            WM_ERASEBKGND => 1,
            WM_MOUSEACTIVATE => 3, // MA_NOACTIVATE
            WM_MOUSEMOVE => {
                let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *const OverlayState;
                if let Some(state) = state.as_ref()
                    && state.collapsed
                    // Wait for docking to finish so a passing tab cannot undo dismissal.
                    && state.layout.is_some_and(|layout| layout.panel.is_none())
                    && !state.tab_pressed
                    && !state.clicks.frozen()
                    && wparam & 0x0073 == 0 // No left, right, middle or extra mouse button held.
                    && tab_at(state, lparam as i16 as i32, (lparam >> 16) as i16 as i32)
                {
                    // Reuse cached expansion; hovering never activates or acknowledges a chat.
                    PostMessageW(window, WM_APP_EXPAND_OVERLAY, 1, 0);
                }
                0
            }
            WM_SETCURSOR => {
                let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *const OverlayState;
                let mut point: Point = zeroed();
                if let Some(state) = state.as_ref()
                    && GetCursorPos(&mut point) != 0
                    && ScreenToClient(window, &mut point) != 0
                {
                    let cursor = if close_at(state, point.x, point.y) || tab_at(state, point.x, point.y)
                        || card_target_at(window, state, point.x, point.y).is_some()
                    {
                        32_649usize
                    } else {
                        32_512usize
                    };
                    SetCursor(LoadCursorW(null_mut(), cursor as *const u16));
                    return 1;
                }
                DefWindowProcW(window, message, wparam, lparam)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut OverlayState;
                if let Some(state) = state.as_mut() {
                    state.panel_dirty = true;
                    state.close_pressed = close_at(state, lparam as i16 as i32, (lparam >> 16) as i16 as i32);
                    state.tab_pressed =
                        tab_at(state, lparam as i16 as i32, (lparam >> 16) as i16 as i32);
                    let card = card_target_at(
                        window,
                        state,
                        lparam as i16 as i32,
                        (lparam >> 16) as i16 as i32,
                    );
                    state.clicks.press(
                        if state.tab_pressed || state.close_pressed { None } else { card },
                        message == WM_LBUTTONDBLCLK,
                    );
                    arm_click_timer(window, &state.clicks);
                    if state.clicks.pressed.is_some() || state.tab_pressed || state.close_pressed {
                        cancel_hover(window, state);
                        SetCapture(window);
                    }
                }
                InvalidateRect(window, null(), 0);
                UpdateWindow(window);
                0
            }
            WM_LBUTTONUP => {
                let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut OverlayState;
                let mut activate = false;
                let mut expand = false;
                let mut close = None;
                if let Some(state) = state.as_mut() {
                    state.panel_dirty = true;
                    if state.close_pressed && close_at(state, lparam as i16 as i32, (lparam >> 16) as i16 as i32) {
                        close = Some((state.shortcut_token, state.activity));
                    }
                    state.close_pressed = false;
                    expand = state.tab_pressed
                        && tab_at(state, lparam as i16 as i32, (lparam >> 16) as i16 as i32);
                    state.tab_pressed = false;
                    let released = card_target_at(
                        window,
                        state,
                        lparam as i16 as i32,
                        (lparam >> 16) as i16 as i32,
                    );
                    if let Some(target) = state.clicks.release(
                        released,
                        Instant::now(),
                        Duration::from_millis(GetDoubleClickTime() as u64),
                    ) {
                        state.pending_target = Some(target);
                        activate = true;
                    }
                    arm_click_timer(window, &state.clicks);
                }
                ReleaseCapture();
                if let Some((token, activity)) = close {
                    PostMessageW(window, WM_APP_CLOSE_OVERLAY, token, activity as isize);
                }
                if activate {
                    PostMessageW(window, WM_APP_OPEN_OVERLAY_CARD, 0, 0);
                }
                if expand {
                    PostMessageW(window, WM_APP_EXPAND_OVERLAY, 0, 0);
                }
                InvalidateRect(window, null(), 0);
                UpdateWindow(window);
                0
            }
            WM_CAPTURECHANGED => {
                let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut OverlayState;
                if let Some(state) = state.as_mut() {
                    state.panel_dirty = true;
                    state.clicks.pressed = None;
                    state.clicks.opening = None;
                    state.tab_pressed = false;
                    state.close_pressed = false;
                }
                InvalidateRect(window, null(), 0);
                0
            }
            WM_DESTROY => {
                if GetWindowLongPtrW(window, GWLP_USERDATA) != 0 {
                    PostQuitMessage(0);
                }
                0
            }
            WM_PAINT => {
                let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut OverlayState;
                let mut paint: PaintStruct = zeroed();
                let paint_dc = BeginPaint(window, &mut paint);
                if state.as_ref().is_some_and(|state| state.restoring) {
                    // UpdateLayeredWindow owns presentation until the restore ends.
                    EndPaint(window, &paint);
                    return 0;
                }
                if let Some(state) = state.as_mut() {
                    if state.compositor.is_some() {
                        if let Err(cause) = paint_composited_frame(window, paint_dc, state) {
                            logging::write(format!("Could not repaint overlay frame: {cause}"));
                        }
                        EndPaint(window, &paint);
                        return 0;
                    }
                    let mut rect: Rect = zeroed();
                    GetClientRect(window, &mut rect);
                    let dc = paint_overlay_buffer(paint_dc, state, rect);
                    if dc != paint_dc {
                        BitBlt(
                            paint_dc,
                            0,
                            0,
                            rect.right,
                            rect.bottom,
                            dc,
                            0,
                            0,
                            0x00cc0020,
                        );
                    }
                }
                EndPaint(window, &paint);
                0
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

unsafe fn paint_overlay_buffer(reference: Handle, state: &mut OverlayState, rect: Rect) -> Handle {
    unsafe {
        let dpi = state.dpi.max(96);
        // Keep the full panel allocation while clipping its visible slice.
        let dc = state.buffer.get(reference,
            rect.right.max(state.panel_size.0 + scale_dip(28, dpi)),
            rect.bottom.max(state.panel_size.1));
        fill_rectangle(dc, &rect, 0x00241e1a);
        SetBkMode(dc, TRANSPARENT);
        let old_font = SelectObject(dc, state.font);
        if let Some(layout) = state.layout {
            if let Some(panel) = layout.panel {
                let saved = SaveDC(dc);
                IntersectClipRect(dc, panel.left, panel.top, panel.right, panel.bottom);
                paint_cached_panel(dc, state, panel);
                RestoreDC(dc, saved);
            }
            if let Some(tab) = layout.tab {
                paint_tab(dc, tab, state, dpi, state.activity_started.elapsed());
            }
        }
        SelectObject(dc, old_font);
        dc
    }
}

unsafe fn paint_tab(dc: Handle, tab: Rect, state: &OverlayState, dpi: u32, elapsed: Duration) {
    unsafe {
        let saved = SaveDC(dc);
        SetBkMode(dc, TRANSPARENT);
        let old_font = SelectObject(dc, state.font);
        IntersectClipRect(dc, tab.left, tab.top, tab.right, tab.bottom);
        let full_left = tab.right - scale_dip(28, dpi);
        let reveal = (tab.right - tab.left) as f32 / scale_dip(28, dpi) as f32;
        let pulse = completion_pulse(elapsed, state.animate);
        let background = fade_color(
            if state.attention {
                if state.tab_pressed {
                    blend_color(color_ref(37, 52, 58), color_ref(255, 208, 0), pulse)
                } else {
                    blend_color(color_ref(28, 39, 45), color_ref(255, 208, 0), pulse)
                }
            } else if state.tab_pressed {
                0x005b493b
            } else {
                0x0044352c
            },
            reveal,
        );
        fill_rectangle(dc, &tab, background);
        let mut arrow = Rect {
            left: full_left,
            top: tab.top + scale_dip(11, dpi),
            right: tab.right,
            bottom: tab.top + scale_dip(35, dpi),
        };
        draw_text(
            dc,
            "\u{2039}",
            &mut arrow,
            fade_color(0x00f6f2ef, reveal),
            DT_SINGLELINE | DT_VCENTER | 1,
        );
        let mut count = Rect {
            left: full_left,
            top: tab.top + scale_dip(35, dpi),
            right: tab.right,
            bottom: tab.bottom - scale_dip(5, dpi),
        };
        draw_text(
            dc,
            &state
                .shortcut_code
                .map(|code| String::from_utf8_lossy(&code).into_owned())
                .unwrap_or_else(|| tab_caption(&state.cards)),
            &mut count,
            fade_color(
                if state.attention {
                    color_ref(234, 244, 240)
                } else {
                    0x00cab98b
                },
                reveal,
            ),
            DT_SINGLELINE | DT_VCENTER | 1,
        );
        if state.busy {
            for dot in 0..3 {
                let left = full_left + scale_dip(6 + dot as i32 * 6, dpi);
                fill_rectangle(
                    dc,
                    &Rect {
                        left,
                        right: left + scale_dip(3, dpi),
                        top: tab.bottom - scale_dip(7, dpi),
                        bottom: tab.bottom - scale_dip(4, dpi),
                    },
                    fade_color(
                        0x008bdcf6,
                        busy_strength(elapsed, dot, state.animate) * reveal,
                    ),
                );
            }
        }
        SelectObject(dc, old_font);
        RestoreDC(dc, saved);
    }
}

unsafe fn paint_composited_frame(window: Hwnd, reference: Handle, state: &mut OverlayState) -> io::Result<()> {
    unsafe {
        let Some(layout) = state.layout else { return Ok(()); };
        let rect = Rect { left: 0, top: 0, right: layout.window.right - layout.window.left,
            bottom: layout.window.bottom - layout.window.top };
        let cached = paint_overlay_buffer(reference, state, rect);
        state.compositor.as_mut().unwrap().present(window, cached, layout, state.dpi.max(96), state.render_alpha)
    }
}

unsafe fn paint_cached_panel(dc: Handle, state: &mut OverlayState, panel: Rect) {
    unsafe {
        let (width, height) = state.panel_size;
        let cached = state.panel_buffer.get(dc, width, height);
        if cached == dc {
            // Allocation failure still leaves a readable, correctly positioned message.
            SetViewportOrgEx(dc, panel.left, panel.top, null_mut());
            paint_panel(
                dc,
                state,
                Rect {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
            );
            return;
        }
        if state.panel_dirty {
            let bounds = Rect {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            fill_rectangle(cached, &bounds, 0x00241e1a);
            SetBkMode(cached, TRANSPARENT);
            let font = SelectObject(cached, state.font);
            paint_panel(cached, state, bounds);
            SelectObject(cached, font);
            state.panel_dirty = false;
        }
        let target_width = panel.right - panel.left;
        let target_height = panel.bottom - panel.top;
        if target_width == width && target_height == height {
            BitBlt(
                dc, panel.left, panel.top, width, height, cached, 0, 0, 0x00cc0020,
            );
        } else {
            SetStretchBltMode(dc, 4); // HALFTONE: smooth downsampling of the cached message.
            StretchBlt(
                dc,
                panel.left,
                panel.top,
                target_width,
                target_height,
                cached,
                0,
                0,
                width,
                height,
                0x00cc0020,
            );
        }
    }
}

unsafe fn paint_panel(dc: Handle, state: &OverlayState, rect: Rect) {
    unsafe {
        let dpi = state.dpi.max(96);
        let inset = scale_dip(18, dpi);
        let mut header = Rect {
            left: inset,
            top: scale_dip(12, dpi),
            right: rect.right - scale_dip(68, dpi),
            bottom: scale_dip(34, dpi),
        };
        if state.attention {
            paint_completion_dot(
                dc,
                Point {
                    x: rect.right - scale_dip(52, dpi),
                    y: scale_dip(22, dpi),
                },
                completion_strength(state.activity_started.elapsed(), state.animate),
                dpi,
            );
        }
        draw_text(
            dc,
            "CODEX  ·  LIVE UPDATES",
            &mut header,
            0x00cab98b,
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        let mut close = close_button_rect(rect.right, dpi);
        if state.close_pressed { fill_rectangle(dc, &close, 0x0044352c); }
        draw_text(dc, "\u{00d7}", &mut close, 0x00f6f2ef, DT_SINGLELINE | DT_VCENTER | 1);
        let mut y = scale_dip(40, dpi);
        for row in &state.rows {
            let card = &row.card;
            if state
                .clicks
                .pressed
                .as_ref()
                .is_some_and(|pressed| pressed.id == card.id)
                || state
                    .clicks
                    .pending
                    .iter()
                    .any(|(pending, _)| pending.id == card.id)
            {
                fill_rectangle(
                    dc,
                    &Rect {
                        left: scale_dip(8, dpi),
                        top: y,
                        right: rect.right - scale_dip(8, dpi),
                        bottom: (y + row.height - scale_dip(6, dpi))
                            .min(rect.bottom - scale_dip(24, dpi)),
                    },
                    0x00392e25,
                );
            }
            let saved = SaveDC(dc);
            IntersectClipRect(
                dc,
                inset,
                y,
                rect.right - inset,
                (y + row.height).min(rect.bottom - scale_dip(24, dpi)),
            );
            if card.attention {
                paint_completion_dot(
                    dc,
                    Point {
                        x: inset + scale_dip(3, dpi),
                        y: y + scale_dip(10, dpi),
                    },
                    completion_strength(state.activity_started.elapsed(), state.animate)
                        * row.opacity,
                    dpi,
                );
            }
            let mut label = Rect {
                left: inset
                    + if card.final_message {
                        scale_dip(12, dpi)
                    } else {
                        0
                    },
                top: y,
                right: rect.right - inset,
                bottom: y + scale_dip(22, dpi),
            };
            let title = if card.final_message {
                format!("Done · {}", card.label)
            } else {
                card.label.clone()
            };
            draw_text(
                dc,
                &title,
                &mut label,
                fade_color(
                    if card.final_message {
                        0x00b5deb0
                    } else {
                        0x00c7bdb3
                    },
                    row.opacity,
                ),
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            let mut body = Rect {
                left: inset,
                top: y + scale_dip(25, dpi),
                right: rect.right - inset,
                bottom: (y + row.full_height - scale_dip(12, dpi))
                    .min(rect.bottom - scale_dip(24, dpi)),
            };
            draw_text(
                dc,
                &card.text,
                &mut body,
                fade_color(0x00f6f2ef, row.opacity),
                DT_WORDBREAK | DT_EDITCONTROL | DT_END_ELLIPSIS,
            );
            RestoreDC(dc, saved);
            y += row.height;
        }
        let mut footer = Rect {
            left: inset,
            top: rect.bottom - scale_dip(24, dpi),
            right: rect.right - inset,
            bottom: rect.bottom - scale_dip(6, dpi),
        };
        let shortcut_hint = state.shortcut_code.map(|code| {
            format!(
                "Open: double-click or Copilot+{}, {}",
                code[0] as char, code[1] as char
            )
        });
        draw_text(
            dc,
            shortcut_hint
                .as_deref()
                .unwrap_or("Click to tuck away · Double-click to open VS Code"),
            &mut footer,
            0x009c9189,
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
    }
}

unsafe fn paint_completion_dot(dc: Handle, center: Point, strength: f32, dpi: u32) {
    unsafe {
        // A fixed six-DIP circle pulses in brightness, keeping the tab and label stationary.
        let radius = scale_dip(3, dpi).max(1);
        fill_rounded_rectangle(
            dc,
            &Rect {
                left: center.x - radius,
                top: center.y - radius,
                right: center.x + radius,
                bottom: center.y + radius,
            },
            fade_color(color_ref(111, 213, 147), strength),
            radius,
        );
    }
}

fn intersect_rect(first: Rect, second: Rect) -> Option<Rect> {
    let rect = Rect {
        left: first.left.max(second.left),
        top: first.top.max(second.top),
        right: first.right.min(second.right),
        bottom: first.bottom.min(second.bottom),
    };
    (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
}

unsafe fn paint_activity(window: Hwnd, state: &OverlayState) {
    unsafe {
        let Some(layout) = state.layout else {
            return;
        };
        if state.buffer.dc.is_null() {
            InvalidateRect(window, null(), 0);
            UpdateWindow(window);
            return;
        }
        let composited = state.compositor.is_some();
        let destination = if composited { null_mut() } else { GetDC(window) };
        if !composited && destination.is_null() {
            return;
        }
        let mut client: Rect = zeroed();
        GetClientRect(window, &mut client);
        let dc = state.buffer.dc;
        let dpi = state.dpi.max(96);
        let elapsed = state.activity_started.elapsed();
        let pulse = completion_strength(elapsed, state.animate);
        let radius = scale_dip(3, dpi).max(1);
        let dot_rect = |center: &Point| Rect {
            left: center.x - radius,
            top: center.y - radius,
            right: center.x + radius,
            bottom: center.y + radius,
        };
        let update = |bounds: Rect, clip: Rect, background: u32, draw: &dyn Fn(Handle)| {
            let Some(dirty) =
                intersect_rect(bounds, clip).and_then(|rect| intersect_rect(rect, client))
            else {
                return;
            };
            let saved = SaveDC(dc);
            IntersectClipRect(dc, dirty.left, dirty.top, dirty.right, dirty.bottom);
            fill_rectangle(dc, &dirty, background);
            draw(dc);
            RestoreDC(dc, saved);
            if !composited { BitBlt(
                destination,
                dirty.left,
                dirty.top,
                dirty.right - dirty.left,
                dirty.bottom - dirty.top,
                dc,
                dirty.left,
                dirty.top,
                0x00cc0020,
            ); }
        };
        if let Some(panel) = layout.panel {
            let inset = scale_dip(18, dpi);
            if state.attention {
                let center = Point {
                    x: panel.right - scale_dip(52, dpi),
                    y: panel.top + scale_dip(22, dpi),
                };
                update(dot_rect(&center), panel, 0x00241e1a, &|dc| {
                    paint_completion_dot(dc, Point { ..center }, pulse, dpi)
                });
            }
            let mut y = panel.top + scale_dip(40, dpi);
            for row in &state.rows {
                if row.card.attention {
                    let center = Point {
                        x: panel.left + inset + scale_dip(3, dpi),
                        y: y + scale_dip(10, dpi),
                    };
                    let pressed = state
                        .clicks
                        .pressed
                        .as_ref()
                        .is_some_and(|card| card.id == row.card.id)
                        || state
                            .clicks
                            .pending
                            .iter()
                            .any(|(card, _)| card.id == row.card.id);
                    let clip = Rect {
                        left: panel.left + inset,
                        top: y,
                        right: panel.right - inset,
                        bottom: (y + row.height).min(panel.bottom - scale_dip(24, dpi)),
                    };
                    update(
                        dot_rect(&center),
                        clip,
                        if pressed { 0x00392e25 } else { 0x00241e1a },
                        &|dc| {
                            paint_completion_dot(dc, Point { ..center }, pulse * row.opacity, dpi)
                        },
                    );
                }
                y += row.height;
            }
        }
        if let Some(tab) = layout.tab {
            if state.attention {
                // Repaint only the tiny tab; message text and panel caches stay intact.
                update(tab, tab, 0x00241e1a, &|dc| paint_tab(dc, tab, state, dpi, elapsed));
            } else if state.busy {
                // Busy tabs still update only their three indicator squares.
                let full_left = tab.right - scale_dip(28, dpi);
                let reveal = (tab.right - tab.left) as f32 / scale_dip(28, dpi) as f32;
                let background = fade_color(
                    if state.tab_pressed { 0x005b493b } else { 0x0044352c }, reveal);
                for dot in 0..3 {
                    let left = full_left + scale_dip(6 + dot as i32 * 6, dpi);
                    let rect = Rect {
                        left,
                        right: left + scale_dip(3, dpi),
                        top: tab.bottom - scale_dip(7, dpi),
                        bottom: tab.bottom - scale_dip(4, dpi),
                    };
                    update(rect, tab, background, &|dc| fill_rectangle(dc, &rect,
                        fade_color(0x008bdcf6, busy_strength(elapsed, dot, state.animate) * reveal)));
                }
            }
        }
        if !destination.is_null() { ReleaseDC(window, destination); }
    }
}

fn fade_color(color: u32, opacity: f32) -> u32 {
    blend_color(0x00241e1a, color, opacity)
}

fn blend_color(background: u32, color: u32, opacity: f32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |result, shift| {
        let base = ((background >> shift) & 0xff) as f32;
        let foreground = ((color >> shift) & 0xff) as f32;
        result | (((base + (foreground - base) * opacity.clamp(0.0, 1.0)).round() as u32) << shift)
    })
}

unsafe fn draw_text(dc: Handle, text: &str, rect: &mut Rect, color: u32, flags: u32) {
    unsafe {
        SetTextColor(dc, color);
        let text = wide(text);
        DrawTextW(
            dc,
            text.as_ptr(),
            wide_text_length(&text),
            rect,
            flags | DT_NOPREFIX,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetPixel(dc: Handle, x: i32, y: i32) -> u32;
    }

    #[test]
    fn native_completion_dot_pulses_without_coloring_the_border_or_reallocating_the_buffer() {
        unsafe {
            let reference = GetDC(null_mut());
            assert!(!reference.is_null());
            let mut buffer = PaintBuffer::default();
            let dc = buffer.get(reference, 120, 80);
            assert_ne!(dc, reference);
            let bitmap = buffer.bitmap;
            let rect = Rect {
                left: 0,
                top: 0,
                right: 120,
                bottom: 80,
            };
            fill_rectangle(dc, &rect, 0x00241e1a);
            paint_completion_dot(
                dc,
                Point { x: 60, y: 10 },
                completion_strength(Duration::ZERO, true),
                96,
            );
            let dim = GetPixel(dc, 60, 10);
            paint_completion_dot(
                dc,
                Point { x: 60, y: 10 },
                completion_strength(Duration::from_millis(900), true),
                96,
            );
            let bright = GetPixel(dc, 60, 10);
            assert_ne!(dim, bright);
            for (x, y) in [(0, 40), (119, 40), (60, 0), (60, 79), (55, 10), (65, 10)] {
                assert_eq!(
                    GetPixel(dc, x, y),
                    0x00241e1a,
                    "completion must only paint its small dot"
                );
            }
            assert_eq!(
                GetPixel(dc, 60, 40),
                0x00241e1a,
                "dot must leave the message background alone"
            );
            for _ in 0..200 {
                assert_eq!(buffer.get(reference, 28, 64), dc);
                assert_eq!(
                    buffer.bitmap, bitmap,
                    "activity frames must not allocate another bitmap"
                );
            }
            let larger = buffer.get(reference, 160, 100);
            assert_ne!(larger, reference);
            assert_eq!(buffer.get(reference, 120, 80), larger);
            drop(buffer);
            ReleaseDC(null_mut(), reference);
        }
    }

    #[test]
    fn activity_pulses_are_smooth_and_reduced_motion_is_steady() {
        assert_eq!(completion_pulse(Duration::ZERO, true), 0.0);
        assert_eq!(completion_pulse(Duration::from_millis(900), true), 1.0);
        assert_eq!(completion_pulse(Duration::from_millis(1800), true), 0.0);
        for ms in 0..5000 {
            let at = Duration::from_millis(ms);
            let next = at + Duration::from_millis(1);
            assert!((completion_pulse(at, true) - completion_pulse(next, true)).abs() < 0.002);
            assert_eq!(
                completion_pulse(at, false),
                completion_pulse(Duration::ZERO, false)
            );
            assert!(
                (completion_strength(at, true) - completion_strength(next, true)).abs() < 0.001
            );
            assert!((0.54..=0.96).contains(&completion_strength(at, true)));
            assert_eq!(
                completion_strength(at, false),
                completion_strength(Duration::ZERO, false)
            );
            for dot in 0..3 {
                assert!(
                    (busy_strength(at, dot, true) - busy_strength(next, dot, true)).abs() < 0.005
                );
                assert_eq!(busy_strength(at, dot, false), 0.8);
            }
        }
        assert!(busy_strength(Duration::ZERO, 0, true) > busy_strength(Duration::ZERO, 1, true));
        assert!(
            busy_strength(Duration::from_millis(400), 1, true)
                > busy_strength(Duration::from_millis(400), 0, true)
        );
        assert!(!needs_activity_timer(false, true, true, true, true));
        assert!(!needs_activity_timer(true, false, true, true, true));
        assert!(!needs_activity_timer(true, true, false, true, false));
        assert!(!needs_activity_timer(true, true, false, false, true));
        assert!(needs_activity_timer(true, true, false, true, true));
        assert!(needs_activity_timer(true, true, true, false, false));
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(class_name: *const u16, name: *const u16) -> Hwnd;
        fn IsWindowVisible(window: Hwnd) -> Bool;
        fn GetLayeredWindowAttributes(
            window: Hwnd,
            color: *mut u32,
            alpha: *mut u8,
            flags: *mut u32,
        ) -> Bool;
        fn GetWindowRgn(window: Hwnd, region: Handle) -> i32;
    }

    #[test]
    #[ignore = "briefly displays the native overlay; run explicitly on an interactive desktop"]
    fn native_overlay_accepts_card_clicks_without_taking_focus_on_arrival() {
        check_native_motion();
        check_native_click(true);
        check_native_click(false);
    }

    #[test]
    fn three_session_lanes_never_overlap_during_expansion_at_each_display_scale() {
        for dpi in [96, 144, 192] {
            for height in [480, 900, 1440] {
                let work = Rect {
                    left: -1920,
                    top: -200,
                    right: 0,
                    bottom: height - 200,
                };
                for position in ["top-left", "top-right", "bottom-left", "bottom-right"] {
                    let lanes: Vec<_> = (0..3)
                        .map(|slot| session_work_area(work, slot, dpi, position))
                        .collect();
                    for (slot, lane) in lanes.iter().enumerate() {
                        let panel = overlay_bounds(
                            *lane,
                            scale_dip(440, dpi),
                            scale_dip(226, dpi).min(lane.bottom - lane.top - scale_dip(40, dpi)),
                            scale_dip(20, dpi),
                            position,
                        );
                        for progress in [0.0, 0.1, 0.5, 0.9, 1.0, 0.7, 0.0] {
                            let layout = dock_layout(
                                panel,
                                *lane,
                                progress,
                                dpi,
                                Some((panel.top + panel.bottom) / 2),
                            );
                            assert!(
                                layout.window.top >= lane.top
                                    && layout.window.bottom <= lane.bottom
                            );
                            assert!(layout.window.right <= work.right);
                            for (other, reserved) in lanes.iter().enumerate() {
                                if slot != other {
                                    assert!(
                                        layout.window.bottom <= reserved.top
                                            || layout.window.top >= reserved.bottom
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn hover_leave_grace_resets_on_reentry_and_spans_the_gap_to_the_original_tab() {
        let now = Instant::now();
        let anchor = Rect {
            left: 972,
            top: 400,
            right: 1000,
            bottom: 464,
        };
        let panel = Rect {
            left: 540,
            top: 320,
            right: 980,
            bottom: 550,
        };
        let mut hover = HoverOpen::new(anchor);
        assert!(!hover.should_collapse(panel, Some((990, 430)), now));
        assert!(
            !hover.should_collapse(panel, Some((990, 430)), now + Duration::from_secs(2)),
            "a stationary pointer over the original tab must not cause flicker"
        );
        assert!(!hover.should_collapse(panel, Some((600, 400)), now + Duration::from_secs(3)));
        assert!(!hover.should_collapse(panel, Some((500, 400)), now + Duration::from_secs(4)));
        assert!(!hover.should_collapse(panel, Some((500, 400)), now + Duration::from_millis(4199)));
        assert!(!hover.should_collapse(panel, Some((600, 400)), now + Duration::from_millis(4200)));
        assert!(!hover.should_collapse(panel, Some((500, 400)), now + Duration::from_secs(5)));
        assert!(
            !hover.should_collapse(panel, None, now + Duration::from_secs(6)),
            "an unavailable cursor must not dismiss a panel"
        );
        assert!(!hover.should_collapse(panel, Some((500, 400)), now + Duration::from_secs(7)));
        assert!(hover.should_collapse(panel, Some((500, 400)), now + Duration::from_millis(7200)));
    }

    #[test]
    #[ignore = "displays an owned overlay; run explicitly on an interactive desktop"]
    fn native_minimize_uses_cached_hidden_chat_and_preserves_hover_after_reader_catches_up() {
        use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
        let origin = unsafe { GetForegroundWindow() } as usize;
        assert_ne!(origin, 0);
        let destination = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let request = Arc::new(AtomicU64::new(1));
        let (wake, _reader) = mpsc::sync_channel(1);
        let focused_chat = Arc::new(AtomicBool::new(true));
        let updates = OverlayUpdates::new(destination.clone(), wake);
        let ui = {
            let stop = stop.clone();
            let request = request.clone();
            let focused_chat = focused_chat.clone();
            thread::spawn(move || run_overlay_inner(None, |_| Frame {
                session_id: Some("minimize-sync".into()), window: Some(origin as u64),
                cards: vec![Card { id: 1, label: "Owned minimize timing test".into(),
                    text: "Cached content follows the originating window without waiting for the reader.".into(),
                    final_message: false, attention: false, target: None }],
                hidden_in_focus: focused_chat.load(Ordering::Relaxed), dock_request: request.load(Ordering::Relaxed),
                close: stop.load(Ordering::Relaxed), ..Frame::empty()
            }, |_, _| panic!("minimizing does not open or acknowledge a chat"), || None, None, Some(updates)).unwrap())
        };
        let result = std::panic::catch_unwind(|| unsafe {
            SetThreadDpiAwarenessContext(-4isize as Handle);
            let deadline = Instant::now() + Duration::from_secs(3);
            while destination.load(Ordering::Acquire) == 0 {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(2));
            }
            let window = destination.load(Ordering::Acquire) as Hwnd;
            thread::sleep(Duration::from_millis(100));
            assert_eq!(IsWindowVisible(window), 0, "focused chat must stay hidden while cached");
            let bounds = || { let mut rect: Rect = zeroed(); GetWindowRect(window, &mut rect); rect };
            let tab = scale_dip(28, GetDpiForWindow(window).max(96));
            // Deliver the hook message to this owned overlay only. The real editor
            // is never minimized or activated by this test.
            let tick = super::super::overlay_window_events::test_tick();
            let started = Instant::now();
            PostMessageW(window, WM_MINIMIZE, origin, tick as isize);
            while IsWindowVisible(window) == 0 {
                assert!(started.elapsed() < Duration::from_millis(150), "cached minimize waited for polling");
                thread::sleep(Duration::from_millis(1));
            }
            println!("Cached minimize response: {:?}", started.elapsed());
            let mut changes = 0;
            let mut previous = bounds();
            while started.elapsed() < Duration::from_millis(350) {
                let next = bounds();
                if next != previous { changes += 1; previous = next; }
                thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(bounds().right - bounds().left, tab);
            println!("Minimize geometry changes: {changes}");
            // Reopen before the slower reader reports its focus-loss epoch.
            PostMessageW(window, WM_APP_EXPAND_OVERLAY, 1, 0);
            thread::sleep(Duration::from_millis(280));
            let expanded = bounds();
            assert!(expanded.right - expanded.left > tab * 4);
            request.store(2, Ordering::Relaxed);
            OverlayUpdates::notify(&destination);
            thread::sleep(Duration::from_millis(80));
            assert_eq!(bounds(), expanded, "the late reader must not repeat the minimize");
            PostMessageW(window, WM_RESTORE, origin, tick as isize);
            thread::sleep(Duration::from_millis(240));
            assert_eq!(IsWindowVisible(window), 0);
            // Old deliveries finish at the tab instead of replaying a full-size card.
            let late_tick = super::super::overlay_window_events::test_tick().wrapping_sub(500);
            PostMessageW(window, WM_MINIMIZE, origin, late_tick as isize);
            thread::sleep(Duration::from_millis(35));
            assert_eq!(bounds().right - bounds().left, tab);
            // Switching chats in the same editor has no foreground event. The
            // previously viewed chat must still return tucked when metadata changes.
            PostMessageW(window, WM_RESTORE, origin, late_tick as isize);
            thread::sleep(Duration::from_millis(240));
            assert_eq!(IsWindowVisible(window), 0);
            focused_chat.store(false, Ordering::Relaxed);
            OverlayUpdates::notify(&destination);
            thread::sleep(Duration::from_millis(300));
            assert_ne!(IsWindowVisible(window), 0);
            assert_eq!(bounds().right - bounds().left, tab);
            assert_eq!(GetForegroundWindow() as usize, origin);
        });
        stop.store(true, Ordering::Relaxed);
        OverlayUpdates::notify(&destination);
        ui.join().unwrap();
        assert_eq!(destination.load(Ordering::Acquire), 0);
        if let Err(cause) = result { std::panic::resume_unwind(cause); }
    }

    #[test]
    #[ignore = "displays an owned overlay; run explicitly on an interactive desktop"]
    fn native_focus_requests_start_tucked_and_only_redock_on_a_new_focus_loss() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let request = Arc::new(AtomicU64::new(1));
        let hidden = Arc::new(AtomicBool::new(false));
        let ui = {
            let stop = stop.clone();
            let request = request.clone();
            let hidden = hidden.clone();
            thread::spawn(move || {
                run_overlay(None, |_| {
                if hidden.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) { return Frame::empty(); }
                Frame {
                    session_id: Some("focus-test".into()),
                    cards: vec![Card { id: 1, label: "Project ? Active chat".into(),
                        text: "This cached message shrinks into its tab when the editor loses focus.".into(),
                        final_message: true, attention: true, target: None }],
                    dock_request: request.load(Ordering::Relaxed),
                    attention: true, close: stop.load(Ordering::Relaxed),
                    ..Frame::empty()
                }
            }, |_| panic!("hover and focus changes must never acknowledge a chat"), || None).unwrap()
            })
        };
        let result = std::panic::catch_unwind(|| unsafe {
            SetThreadDpiAwarenessContext(-4isize as Handle);
            let class = wide(format!(
                "CodexLidGuardMessageOverlay.{}",
                GetCurrentProcessId()
            ));
            let deadline = Instant::now() + Duration::from_secs(3);
            let window = loop {
                let window = FindWindowW(class.as_ptr(), null());
                if !window.is_null() && IsWindowVisible(window) != 0 {
                    break window;
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(5));
            };
            let bounds = || {
                let mut rect = zeroed();
                assert_ne!(GetWindowRect(window, &mut rect), 0);
                rect
            };
            let tab_width = scale_dip(28, GetDpiForWindow(window).max(96));
            let wait_tab = || {
                let deadline = Instant::now() + Duration::from_secs(2);
                while bounds().right - bounds().left != tab_width {
                    assert!(
                        Instant::now() < deadline,
                        "focus loss did not finish as a tab"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                thread::sleep(Duration::from_millis(60));
            };
            wait_tab();
            let original = bounds();
            assert_ne!(GetForegroundWindow(), window);
            PostMessageW(window, WM_APP_EXPAND_OVERLAY, 1, 0);
            thread::sleep(Duration::from_millis(850)); // Several identical feed snapshots.
            assert!(
                bounds().right - bounds().left > tab_width * 3,
                "steady background state undid hover"
            );
            request.store(2, Ordering::Relaxed); // A later minimize / focus-loss edge.
            wait_tab();
            assert_eq!(bounds(), original);
            assert_ne!(GetForegroundWindow(), window);
            request.store(3, Ordering::Relaxed);
            let check_until = Instant::now() + Duration::from_millis(550);
            while Instant::now() < check_until {
                assert_eq!(
                    bounds(),
                    original,
                    "a tucked tab must not replay the arrival animation"
                );
                thread::sleep(Duration::from_millis(5));
            }
            // Viewing this exact chat hides it; switching away must return it tucked.
            hidden.store(true, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(500));
            assert_eq!(IsWindowVisible(window), 0);
            hidden.store(false, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(600));
            assert_ne!(IsWindowVisible(window), 0);
            wait_tab();
            assert_eq!(bounds(), original);
        });
        stop.store(true, Ordering::Relaxed);
        ui.join().unwrap();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }

    #[test]
    #[ignore = "displays three independent native panels; run explicitly on an interactive desktop"]
    fn native_three_tabs_expand_independently_and_open_their_own_chat() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let open_succeeds = Arc::new(AtomicBool::new(false));
        let focus_epoch = Arc::new(AtomicUsize::new(0));
        let hover_pointer = Arc::new(std::sync::Mutex::new(Some((-100_000, -100_000))));
        let hover_checks = Arc::new(AtomicUsize::new(0));
        let (opened, received) = std::sync::mpsc::channel();
        let threads: Vec<_> = (0..3).map(|slot| {
            let stop = stop.clone();
            let opened = opened.clone();
            let open_succeeds = open_succeeds.clone();
            let focus_epoch = focus_epoch.clone();
            let pointer = hover_pointer.clone();
            let checks = hover_checks.clone();
            thread::spawn(move || run_overlay(Some(slot), |_| Frame {
                session_id: Some(format!("chat-{slot}")),
                cards: vec![Card { id: slot as u64, label: format!("Project \u{2014} Chat {slot}"),
                    text: "This panel belongs to one chat. Other panels must stay in place.".into(),
                    final_message: slot == 1, attention: slot == 1,
                    target: Some(CardTarget { window: 100 + slot as u64, session_id: format!("chat-{slot}") }) }],
                dock_request: if slot == 2 { focus_epoch.load(Ordering::Relaxed) as u64 } else { 0 },
                busy: slot != 1, attention: slot == 1, close: stop.load(Ordering::Relaxed),
                ..Frame::empty()
            }, |target| { opened.send(target.clone()).unwrap(); open_succeeds.load(Ordering::Relaxed) }, || {
                if slot == 1 { checks.fetch_add(1, Ordering::Relaxed); }
                *pointer.lock().unwrap()
            }).unwrap())
        }).collect();
        let result = std::panic::catch_unwind(|| unsafe {
            SetThreadDpiAwarenessContext(-4isize as Handle);
            let mut windows = Vec::new();
            for slot in 0..3 {
                let class = wide(format!(
                    "CodexLidGuardMessageOverlay.{}{}",
                    GetCurrentProcessId(),
                    if slot == 0 {
                        String::new()
                    } else {
                        format!(".{slot}")
                    }
                ));
                let deadline = Instant::now() + Duration::from_secs(3);
                loop {
                    let window = FindWindowW(class.as_ptr(), null());
                    if !window.is_null() && IsWindowVisible(window) != 0 {
                        // This test injects mouse messages and a synthetic pointer.
                        // Let the user's real pointer pass through these owned test
                        // windows so it cannot reopen a tab during the assertions.
                        SetWindowLongPtrW(window, -20, GetWindowLongPtrW(window, -20) | 0x20);
                        windows.push(window);
                        break;
                    }
                    assert!(Instant::now() < deadline, "session panel failed to appear");
                    thread::sleep(Duration::from_millis(10));
                }
            }
            thread::sleep(Duration::from_millis(350));
            let bounds = |window| {
                let mut rect = zeroed();
                assert_ne!(GetWindowRect(window, &mut rect), 0);
                rect
            };
            let initial: Vec<_> = windows.iter().map(|window| bounds(*window)).collect();
            for window in &windows {
                assert_ne!(GetForegroundWindow(), *window);
            }
            let click = |window, tab: bool, double: bool| {
                let mut rect: Rect = zeroed();
                GetClientRect(window, &mut rect);
                let dpi = GetDpiForWindow(window).max(96);
                let y = if tab {
                    rect.bottom / 2
                } else {
                    scale_dip(60, dpi)
                };
                let point = ((y as isize) << 16) | (rect.right / 2) as isize;
                SendMessageW(window, WM_LBUTTONDOWN, 0, point);
                SendMessageW(window, WM_LBUTTONUP, 0, point);
                if double {
                    SendMessageW(window, WM_LBUTTONDBLCLK, 0, point);
                    SendMessageW(window, WM_LBUTTONUP, 0, point);
                }
            };
            click(windows[1], false, false);
            thread::sleep(Duration::from_millis(GetDoubleClickTime() as u64 + 350));
            let tab = bounds(windows[1]);
            assert_eq!(
                tab.right - tab.left,
                scale_dip(28, GetDpiForWindow(windows[1]).max(96))
            );
            assert_eq!(bounds(windows[0]), initial[0]);
            assert_eq!(bounds(windows[2]), initial[2]);
            click(windows[1], true, false);
            thread::sleep(Duration::from_millis(350));
            assert_eq!(bounds(windows[1]), initial[1]);
            assert_eq!(bounds(windows[0]), initial[0]);
            assert_eq!(bounds(windows[2]), initial[2]);
            // Hover opens only the tucked chat, without a click, focus change or acknowledgement.
            click(windows[1], false, false);
            thread::sleep(Duration::from_millis(GetDoubleClickTime() as u64 + 350));
            let mut client: Rect = zeroed();
            GetClientRect(windows[1], &mut client);
            let middle = ((client.bottom as isize / 2) << 16) | (client.right / 2) as isize;
            let outside = ((client.bottom as isize + 10) << 16) | (client.right / 2) as isize;
            SendMessageW(windows[1], WM_MOUSEMOVE, 0, outside);
            SendMessageW(windows[1], WM_MOUSEMOVE, 1, middle);
            thread::sleep(Duration::from_millis(100));
            assert_eq!(
                bounds(windows[1]),
                tab,
                "outside movement and dragging must not open a tab"
            );
            let hovered = Instant::now();
            *hover_pointer.lock().unwrap() =
                Some(((tab.left + tab.right) / 2, (tab.top + tab.bottom) / 2));
            SendMessageW(windows[1], WM_MOUSEMOVE, 0, middle);
            while bounds(windows[1]) == tab {
                assert!(
                    hovered.elapsed() < Duration::from_millis(200),
                    "hover expansion waited for the feed timer"
                );
                thread::sleep(Duration::from_millis(2));
            }
            println!("Native tab hover response: {:?}", hovered.elapsed());
            thread::sleep(Duration::from_millis(350));
            assert_eq!(bounds(windows[1]), initial[1]);
            assert_eq!(bounds(windows[0]), initial[0]);
            assert_eq!(bounds(windows[2]), initial[2]);
            for window in &windows {
                assert_ne!(GetForegroundWindow(), *window);
            }
            assert!(
                received.try_recv().is_err(),
                "hovering must not open or acknowledge the chat"
            );
            let inside = (
                (initial[1].left + initial[1].right) / 2,
                (initial[1].top + initial[1].bottom) / 2,
            );
            *hover_pointer.lock().unwrap() = Some(inside);
            thread::sleep(Duration::from_millis(100));
            // A brief excursion must be cancelled when the pointer returns to the panel.
            *hover_pointer.lock().unwrap() = Some((-100_000, -100_000));
            thread::sleep(Duration::from_millis(100));
            assert_eq!(bounds(windows[1]), initial[1]);
            *hover_pointer.lock().unwrap() = Some(inside);
            thread::sleep(Duration::from_millis(300));
            assert_eq!(bounds(windows[1]), initial[1]);
            let left = Instant::now();
            *hover_pointer.lock().unwrap() = Some((-100_000, -100_000));
            while bounds(windows[1]) != tab {
                assert!(
                    left.elapsed() < Duration::from_millis(1000),
                    "leaving a hover preview did not tuck it away"
                );
                thread::sleep(Duration::from_millis(5));
            }
            assert!(left.elapsed() >= Duration::from_millis(200));
            println!("Native hover leave, including slide: {:?}", left.elapsed());
            let checks = hover_checks.load(Ordering::Relaxed);
            thread::sleep(Duration::from_millis(150));
            assert_eq!(
                hover_checks.load(Ordering::Relaxed),
                checks,
                "hover polling must stop once tucked away"
            );
            assert_eq!(bounds(windows[0]), initial[0]);
            assert_eq!(bounds(windows[2]), initial[2]);
            assert!(received.try_recv().is_err());
            click(windows[2], false, true);
            assert_eq!(
                received.recv_timeout(Duration::from_secs(1)).unwrap(),
                CardTarget {
                    window: 102,
                    session_id: "chat-2".into()
                }
            );
            thread::sleep(Duration::from_millis(80));
            assert_ne!(
                IsWindowVisible(windows[2]),
                0,
                "a failed open must keep its notification available"
            );
            open_succeeds.store(true, Ordering::Relaxed);
            click(windows[2], false, true);
            assert_eq!(
                received
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap()
                    .session_id,
                "chat-2"
            );
            let deadline = Instant::now() + Duration::from_millis(400);
            while IsWindowVisible(windows[2]) != 0 {
                assert!(
                    Instant::now() < deadline,
                    "successful open must hide when the expand/fade finishes"
                );
                thread::sleep(Duration::from_millis(2));
            }
            thread::sleep(Duration::from_millis(650));
            assert_eq!(
                IsWindowVisible(windows[2]),
                0,
                "stale frames must not bring the opened chat back"
            );
            assert_ne!(IsWindowVisible(windows[0]), 0);
            assert_ne!(IsWindowVisible(windows[1]), 0);
            assert_eq!(bounds(windows[0]), initial[0]);
            assert_eq!(bounds(windows[1]), tab);
            focus_epoch.store(1, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(650));
            assert_ne!(
                IsWindowVisible(windows[2]),
                0,
                "a later focus loss must restore the tab"
            );
            let returned = bounds(windows[2]);
            assert_eq!(
                returned.right - returned.left,
                scale_dip(28, GetDpiForWindow(windows[2]).max(96))
            );
            assert!(received.try_recv().is_err());
        });
        stop.store(true, Ordering::Relaxed);
        for thread in threads {
            thread.join().unwrap();
        }
        if let Err(cause) = result {
            std::panic::resume_unwind(cause);
        }
    }

    #[test]
    #[ignore = "displays owned overlays; feeds synthetic events only to the shortcut thread, never the desktop"]
    fn native_keyboard_shortcuts_expand_open_and_hide_only_the_selected_chat() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let service = super::super::overlay_shortcuts::OverlayShortcuts::simulated();
        let stop = Arc::new(AtomicBool::new(false));
        let (opened, received) = std::sync::mpsc::channel();
        let mut threads = Vec::new();
        for slot in 0..3 {
            let publisher = service.publisher(slot);
            let stop = stop.clone();
            let opened = opened.clone();
            threads.push(thread::spawn(move || run_overlay_inner(Some(slot), |_| Frame {
                session_id: Some(format!("keys-{slot}")),
                cards: vec![Card { id: slot as u64, label: format!("Project \u{2014} {}", ["Dry run", "Deploy", "Build"][slot]),
                    text: "Keyboard shortcuts must route to this exact chat without typing into other apps.".into(),
                    final_message: true, attention: true,
                    target: Some(CardTarget { window: 100+slot as u64, session_id: format!("keys-{slot}") }) }],
                dock_request: 1, attention: true, close: stop.load(Ordering::Relaxed), ..Frame::empty()
            }, |target, _| { opened.send(target.clone()).unwrap(); true.into() }, || None, Some(publisher), None).unwrap()));
            let deadline = Instant::now() + Duration::from_secs(3);
            while service.test_binding(slot).is_none() {
                assert!(Instant::now() < deadline, "shortcut binding did not arrive");
                thread::sleep(Duration::from_millis(5));
            }
        }
        let result = std::panic::catch_unwind(|| unsafe {
            SetThreadDpiAwarenessContext(-4isize as Handle);
            thread::sleep(Duration::from_millis(400));
            let windows: Vec<_> = (0..3)
                .map(|slot| {
                    FindWindowW(
                        wide(format!(
                            "CodexLidGuardMessageOverlay.{}{}",
                            GetCurrentProcessId(),
                            if slot == 0 {
                                String::new()
                            } else {
                                format!(".{slot}")
                            }
                        ))
                        .as_ptr(),
                        null(),
                    )
                })
                .collect();
            assert!(windows.iter().all(|window| !window.is_null()));
            let bounds = |window| {
                let mut rect = zeroed();
                assert_ne!(GetWindowRect(window, &mut rect), 0);
                rect
            };
            let initial: Vec<_> = windows.iter().map(|window| bounds(*window)).collect();
            assert_eq!(service.test_binding(0).unwrap().0, *b"DR");
            assert_eq!(service.test_binding(1).unwrap().0, *b"EP");
            for key in *b"DR" {
                service.test_key(key as u32, true);
                service.test_key(key as u32, false);
            }
            thread::sleep(Duration::from_millis(60));
            assert_eq!(
                windows
                    .iter()
                    .map(|window| bounds(*window))
                    .collect::<Vec<_>>(),
                initial
            );
            assert!(received.try_recv().is_err());
            for slot in [0, 1] {
                let (code, token) = service.test_binding(slot).unwrap();
                for key in [0x5b, 0xa0, 0x86] {
                    service.test_key(key, true);
                }
                let pressed = Instant::now();
                service.test_key(code[0] as u32, true);
                while bounds(windows[slot]) == initial[slot] {
                    assert!(
                        pressed.elapsed() < Duration::from_millis(200),
                        "keyboard expand waited for the feed"
                    );
                    thread::sleep(Duration::from_millis(2));
                }
                eprintln!("Keyboard expansion response: {:?}", pressed.elapsed());
                thread::sleep(Duration::from_millis(300));
                assert!(
                    received.try_recv().is_err(),
                    "expansion must not acknowledge or open the chat"
                );
                assert_ne!(GetForegroundWindow(), windows[slot]);
                assert_eq!(bounds(windows[2]), initial[2]);
                service.test_key(code[1] as u32, true);
                assert_eq!(
                    received.recv_timeout(Duration::from_secs(1)).unwrap(),
                    CardTarget {
                        window: 100 + slot as u64,
                        session_id: format!("keys-{slot}")
                    }
                );
                thread::sleep(Duration::from_millis(320));
                assert_eq!(IsWindowVisible(windows[slot]), 0);
                assert_ne!(IsWindowVisible(windows[2]), 0);
                assert!(service.test_binding(slot).is_none());
                // A queued command carrying the old token cannot act on another lane.
                PostMessageW(windows[2], WM_OVERLAY_SHORTCUT, token, 1);
                for key in [code[0] as u32, code[1] as u32, 0x86, 0xa0, 0x5b] {
                    service.test_key(key, false);
                }
            }
            thread::sleep(Duration::from_millis(350));
            assert!(received.try_recv().is_err());
            assert_eq!(IsWindowVisible(windows[0]), 0);
            assert_eq!(IsWindowVisible(windows[1]), 0);
            assert_ne!(IsWindowVisible(windows[2]), 0);
        });
        stop.store(true, Ordering::Relaxed);
        for thread in threads {
            thread.join().unwrap();
        }
        assert!((0..3).all(|slot| service.test_binding(slot).is_none()));
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }

    fn check_native_motion() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let observer_stop = stop.clone();
        let observer = thread::spawn(move || {
            let mut samples = Vec::new();
            let class = wide(format!("CodexLidGuardMessageOverlay.{}", unsafe {
                GetCurrentProcessId()
            }));
            while !observer_stop.load(Ordering::Relaxed) {
                unsafe {
                    let window = FindWindowW(class.as_ptr(), null());
                    if !window.is_null() && IsWindowVisible(window) != 0 {
                        let mut alpha = 0;
                        let mut flags = 0;
                        let mut rect: Rect = zeroed();
                        if GetLayeredWindowAttributes(window, null_mut(), &mut alpha, &mut flags)
                            != 0
                            && GetWindowRect(window, &mut rect) != 0
                        {
                            samples.push((alpha, rect.top, rect.bottom - rect.top));
                        }
                    }
                }
                thread::sleep(Duration::from_millis(8));
            }
            samples
        });
        let started = Instant::now();
        let mut polls = 0;
        let result = run_message_overlay(
            |_| {
                polls += 1;
                let elapsed = started.elapsed().as_millis();
                let ids = if elapsed < 400 {
                    vec![1]
                } else if elapsed < 800 {
                    vec![1, 2]
                } else {
                    vec![2]
                };
                Frame {
                    session_id: None,
                    activity: 0,
                    cards: ids
                        .into_iter()
                        .map(|id| Card {
                            id,
                            label: "Codex Lid Guard - Animation verification".into(),
                            text: "Checking smooth arrival, card reflow, and dismissal.".into(),
                            final_message: false,
                            attention: false,
                            target: None,
                        })
                        .collect(),
                    window: None,
                    opacity: 82,
                    position: "bottom-right".into(),
                    close: elapsed >= 1200,
                    busy: false,
                    attention: false,
                    dock_request: 0,
                    hidden_in_focus: false,
                }
            },
            |_| false,
        );
        stop.store(true, Ordering::Relaxed);
        let samples = observer.join().unwrap();
        result.unwrap();
        assert!(
            (5..=8).contains(&polls),
            "animation ticks must not poll the message feed: {polls}"
        );
        assert!(samples.iter().any(|(alpha, _, _)| *alpha == 209));
        let mut enabled: Bool = 1;
        unsafe {
            SystemParametersInfoW(0x1042, 0, (&mut enabled as *mut Bool).cast(), 0);
        }
        if enabled != 0 {
            assert!(
                samples
                    .iter()
                    .any(|(alpha, _, _)| *alpha > 0 && *alpha < 209)
            );
            let heights: std::collections::HashSet<_> =
                samples.iter().map(|(_, _, height)| *height).collect();
            assert!(
                heights.len() > 3,
                "card reflow should render intermediate heights"
            );
            assert!(samples.windows(2).any(|pair| pair[0].1 != pair[1].1));
        }
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn PtInRegion(region: Handle, x: i32, y: i32) -> Bool;
    }

    unsafe fn hit_region_window(window: Hwnd, point: Point) -> Hwnd {
        unsafe {
            // Query this owned HWND's presented hit mask. Another live overlay
            // may cover these coordinates; composited windows use per-pixel alpha.
            let mut bounds: Rect = zeroed();
            assert_ne!(GetWindowRect(window, &mut bounds), 0);
            let state = &*(GetWindowLongPtrW(window, GWLP_USERDATA) as *const OverlayState);
            if let Some(frame) = &state.compositor {
                return if frame.alpha_at(point.x - bounds.left, point.y - bounds.top) != 0 {
                    window
                } else { null_mut() };
            }
            let region = CreateRectRgn(0, 0, 0, 0);
            assert!(!region.is_null());
            assert_ne!(GetWindowRgn(window, region), 0);
            let contains = PtInRegion(region, point.x - bounds.left, point.y - bounds.top) != 0;
            DeleteObject(region);
            if contains { window } else { null_mut() }
        }
    }

    fn check_native_click(double: bool) {
        use std::sync::atomic::{AtomicU8, Ordering};
        let phase = Arc::new(AtomicU8::new(0));
        let observer_phase = phase.clone();
        let observer = thread::spawn(move || {
            unsafe {
                SetThreadDpiAwarenessContext(-4isize as Handle);
            }
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut left_edges = Vec::new();
            let class = wide(format!("CodexLidGuardMessageOverlay.{}", unsafe {
                GetCurrentProcessId()
            }));
            while observer_phase.load(Ordering::Relaxed) != 3 && Instant::now() < deadline {
                let phase = observer_phase.load(Ordering::Relaxed);
                if phase == 1 || phase == 4 {
                    unsafe {
                        let window = FindWindowW(class.as_ptr(), null());
                        let mut bounds: Rect = zeroed();
                        if !window.is_null() && GetWindowRect(window, &mut bounds) != 0 {
                            left_edges.push((phase, Instant::now(), bounds.left));
                        }
                    }
                }
                thread::sleep(Duration::from_millis(8));
            }
            left_edges
        });
        let foreground = unsafe { GetForegroundWindow() };
        let started = Instant::now();
        let mut tick = 0;
        let mut clicked = Vec::new();
        let mut was_collapsed = false;
        let mut collapsed_polls = 0;
        let mut tab_clicked = false;
        let mut reopened = false;
        let mut message_click = None;
        let mut tab_click = None;
        let mut expanded_completion_colors = std::collections::HashSet::new();
        run_message_overlay(
            |collapsed| {
                was_collapsed |= collapsed;
                if double && tick >= 2 {
                    unsafe {
                        let window = FindWindowW(wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId())).as_ptr(), null());
                        assert_eq!(IsWindowVisible(window), 0, "a successful double-click must stay hidden despite stale cached frames");
                    }
                }
                if tick > 0 && !collapsed {
                    unsafe {
                        let window = FindWindowW(
                            wide(format!("CodexLidGuardMessageOverlay.{}", GetCurrentProcessId())).as_ptr(),
                            null(),
                        );
                        let state = &*(GetWindowLongPtrW(window, GWLP_USERDATA) as *const OverlayState);
                        if let Some(layout) = state.layout
                            && layout.tab.is_none()
                            && let Some(panel) = layout.panel
                        {
                            let pixel = GetPixel(state.buffer.dc,
                                panel.right - scale_dip(21, state.dpi),
                                panel.top + scale_dip(22, state.dpi));
                            assert_ne!(pixel, 0xffffffff, "header dot must be inside the painted panel");
                            assert!(((pixel >> 8) & 0xff) > (pixel & 0xff) + 20,
                                "expanded header must keep its green completion dot, including after reopening");
                            expanded_completion_colors.insert(pixel);
                            if double {
                                let row_pixel = GetPixel(state.buffer.dc,
                                    panel.left + scale_dip(21, state.dpi),
                                    panel.top + scale_dip(50, state.dpi));
                                assert_ne!(row_pixel, 0xffffffff);
                                assert!(((row_pixel >> 8) & 0xff) > (row_pixel & 0xff) + 20,
                                    "completed card dots must pulse during partial repaints too");
                            }
                        }
                    }
                }
                if tick == 1 {
                    unsafe {
                        let window = FindWindowW(
                            wide(format!(
                                "CodexLidGuardMessageOverlay.{}",
                                GetCurrentProcessId()
                            ))
                            .as_ptr(),
                            null(),
                        );
                        assert!(!window.is_null());
                        assert_ne!(IsWindowVisible(window), 0);
                        assert_eq!(GetForegroundWindow(), foreground);
                        let style = GetWindowLongPtrW(window, -20) as u32;
                        let required =
                            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
                        assert_eq!(style & required, required);
                        assert_eq!(style & 0x20, 0, "message clicks must reach the overlay");
                        let mut alpha = 0;
                        let mut flags = 0;
                        assert_ne!(
                            GetLayeredWindowAttributes(window, null_mut(), &mut alpha, &mut flags),
                            0
                        );
                        assert_eq!(alpha, 209);
                        assert_eq!(flags, LWA_ALPHA);
                        let mut bounds: Rect = zeroed();
                        GetWindowRect(window, &mut bounds);
                        assert!(bounds.right > bounds.left && bounds.bottom > bounds.top);
                        let state =
                            &*(GetWindowLongPtrW(window, GWLP_USERDATA) as *const OverlayState);
                        let panel = state.layout.unwrap().panel.unwrap();
                        assert_eq!(
                            hit_region_window(window, Point {
                                x: bounds.left + panel.left + scale_dip(30, state.dpi),
                                y: bounds.top + panel.top + scale_dip(30, state.dpi)
                            }),
                            window
                        );
                        let point = (((panel.top + scale_dip(50, state.dpi)) as isize) << 16)
                            | (panel.left + scale_dip(30, state.dpi)) as isize;
                        // Deliberately click between feed ticks to catch polling-induced latency.
                        if !double {
                            thread::sleep(Duration::from_millis(73));
                        }
                        SendMessageW(window, WM_LBUTTONDOWN, 0, point);
                        SendMessageW(window, WM_LBUTTONUP, 0, point);
                        if !double {
                            message_click = Some((Instant::now(), bounds.left));
                            phase.store(4, Ordering::Relaxed);
                        }
                        if double {
                            SendMessageW(window, WM_LBUTTONDBLCLK, 0, point);
                            SendMessageW(window, WM_LBUTTONUP, 0, point);
                        }
                    }
                }
                if !double && (collapsed || tab_clicked) {
                    unsafe {
                        let window = FindWindowW(
                            wide(format!(
                                "CodexLidGuardMessageOverlay.{}",
                                GetCurrentProcessId()
                            ))
                            .as_ptr(),
                            null(),
                        );
                        let state =
                            &*(GetWindowLongPtrW(window, GWLP_USERDATA) as *const OverlayState);
                        assert_eq!(GetForegroundWindow(), foreground);
                        if collapsed {
                            collapsed_polls += 1;
                        }
                        if collapsed
                            && !tab_clicked
                            && collapsed_polls >= 3
                            && state.layout.unwrap().panel.is_none()
                        {
                            let mut bounds: Rect = zeroed();
                            GetWindowRect(window, &mut bounds);
                            assert_eq!(bounds.right - bounds.left, scale_dip(28, state.dpi));
                            assert_eq!(bounds.bottom - bounds.top, scale_dip(64, state.dpi));
                            let mut info: MonitorInfo = zeroed();
                            info.size = size_of::<MonitorInfo>() as u32;
                            GetMonitorInfoW(MonitorFromWindow(window, 2), &mut info);
                            assert_eq!(bounds.right, info.work.right);
                            let x = (bounds.right - bounds.left) / 2;
                            let y = (bounds.bottom - bounds.top) / 2;
                            assert_eq!(
                                hit_region_window(window, Point {
                                    x: bounds.left + x,
                                    y: bounds.top + y
                                }),
                                window
                            );
                            assert_ne!(
                                hit_region_window(window, Point {
                                    x: bounds.left - 10,
                                    y: bounds.top + y
                                }),
                                window
                            );
                            assert_eq!(state.cards.len(), 2, "new messages stay behind the tab");
                            let point = ((y as isize) << 16) | x as isize;
                            tab_click = Some((Instant::now(), bounds.left));
                            phase.store(1, Ordering::Relaxed);
                            SendMessageW(window, WM_LBUTTONDOWN, 0, point);
                            SendMessageW(window, WM_LBUTTONUP, 0, point);
                            tab_clicked = true;
                        }
                        if tab_clicked && !collapsed && state.layout.unwrap().tab.is_none() {
                            phase.store(2, Ordering::Relaxed);
                            let layout = state.layout.unwrap();
                            let panel = layout.panel.unwrap();
                            assert_ne!(
                                hit_region_window(window, Point {
                                    x: layout.window.left + panel.left / 2,
                                    y: layout.window.top + panel.top + scale_dip(50, state.dpi),
                                }),
                                window,
                                "the invisible gutter must pass clicks through"
                            );
                            assert_eq!(
                                state.cards.iter().map(|card| card.id).collect::<Vec<_>>(),
                                [1, 2]
                            );
                            reopened = true;
                        }
                    }
                }
                tick += 1;
                Frame {
                    session_id: None,
                    activity: 0,
                    cards: {
                        let mut cards = vec![Card {
                            id: 1,
                            label: "Codex Lid Guard · Native verification".into(),
                            text: "Checking transparency, message clicks, and foreground focus."
                                .into(),
                            final_message: double,
                            attention: double,
                            target: Some(CardTarget {
                                window: 123,
                                session_id: "test-session".into(),
                            }),
                        }];
                        if was_collapsed {
                            cards.push(Card {
                                id: 2,
                                label: "Another update".into(),
                                text: "Arrived while tucked away".into(),
                                final_message: false,
                                attention: false,
                                target: None,
                            });
                        }
                        cards
                    },
                    window: None,
                    opacity: 82,
                    position: "bottom-right".into(),
                    close: tick > 2
                        && started.elapsed()
                            > Duration::from_millis(unsafe { GetDoubleClickTime() } as u64 + 2000),
                    busy: true,
                    attention: true,
                    dock_request: 0,
                    hidden_in_focus: false,
                }
            },
            |target| {
                clicked.push(target.clone());
                true
            },
        )
        .unwrap();
        phase.store(3, Ordering::Relaxed);
        let mut enabled: Bool = 1;
        unsafe {
            SystemParametersInfoW(0x1042, 0, (&mut enabled as *mut Bool).cast(), 0);
        }
        if enabled != 0 && !double {
            assert!(
                expanded_completion_colors.len() > 2,
                "the expanded completion dot must keep pulsing while the cards are unchanged"
            );
        }
        let left_edges = observer.join().unwrap();
        if double {
            assert_eq!(
                clicked,
                [CardTarget {
                    window: 123,
                    session_id: "test-session".into()
                }]
            );
            assert!(!was_collapsed);
        } else {
            assert!(clicked.is_empty());
            assert!(was_collapsed && tab_clicked && reopened);
            let expanding: Vec<_> = left_edges
                .iter()
                .filter(|(phase, _, _)| *phase == 1)
                .collect();
            assert!(expanding.len() >= 2);
            assert!(
                expanding.windows(2).all(|pair| pair[1].2 <= pair[0].2),
                "the native window must not jump right when its tab disappears: {left_edges:?}"
            );
            let (tab_at, tab_left) = tab_click.unwrap();
            let tab_delay = expanding
                .iter()
                .find(|(_, _, left)| *left < tab_left)
                .unwrap()
                .1
                .duration_since(tab_at);
            assert!(
                tab_delay < Duration::from_millis(100),
                "tab response took {tab_delay:?}"
            );
            let (message_at, message_left) = message_click.unwrap();
            let message_delay = left_edges
                .iter()
                .find(|(phase, _, left)| *phase == 4 && *left > message_left)
                .unwrap_or_else(|| {
                    panic!(
                        "No collapse motion observed after x={message_left}; samples: {:?}",
                        left_edges
                            .iter()
                            .map(|(phase, _, left)| (*phase, *left))
                            .collect::<Vec<_>>()
                    )
                })
                .1
                .duration_since(message_at);
            let double_click_delay = Duration::from_millis(unsafe { GetDoubleClickTime() } as u64);
            assert!(
                message_delay < double_click_delay + Duration::from_millis(100),
                "single click incurred an extra poll delay: {message_delay:?}"
            );
            eprintln!(
                "Native input timing: tab {tab_delay:?}; message {message_delay:?} (Windows double-click interval {double_click_delay:?})"
            );
        }
    }

    #[test]
    fn hit_testing_selects_each_card_and_excludes_header_footer_and_gaps() {
        for dpi in [96, 144, 192] {
            let mut state = OverlayState {
                cards: (1..=2)
                    .map(|id| Card {
                        id,
                        label: "chat".into(),
                        text: "message".into(),
                        final_message: false,
                        attention: false,
                        target: Some(CardTarget {
                            window: id,
                            session_id: id.to_string(),
                        }),
                    })
                    .collect(),
                heights: vec![scale_dip(100, dpi); 2],
                rows: vec![],
                font: null_mut(),
                dpi,
                clicks: ClickTracker::default(),
                pending_target: None,
                collapsed: false,
                hover_open: None,
                tab_pressed: false,
                close_pressed: false,
                activity: 0,
                layout: None,
                busy: false,
                attention: false,
                animate: true,
                activity_started: Instant::now(),
                buffer: PaintBuffer::default(),
                panel_buffer: PaintBuffer::default(),
                panel_dirty: true,
                panel_size: (1, 1),
                shortcut_code: None,
                shortcut_token: 0,
                restoring: false,
            compositor: None,
            render_alpha: 255,
            };
            let now = Instant::now();
            let mut motion = Motion::new(now);
            motion.sync(&state.cards, &state.heights, now, false);
            state.rows = motion.sample(now).1;
            let bounds = Rect {
                left: 0,
                top: 0,
                right: scale_dip(440, dpi),
                bottom: scale_dip(264, dpi),
            };
            let hit = |x, y| card_at(&state, bounds, scale_dip(x, dpi), scale_dip(y, dpi));
            assert_eq!(hit(30, 50), Some(0));
            assert_eq!(hit(30, 150), Some(1));
            for (x, y) in [(30, 10), (1, 50), (439, 50), (30, 138), (30, 250)] {
                assert_eq!(hit(x, y), None);
            }
        }
    }

    #[test]
    fn single_click_waits_but_double_click_opens_without_dismissing() {
        let card = ClickedCard {
            id: 1,
            target: Some(CardTarget {
                window: 10,
                session_id: "one".into(),
            }),
        };
        let now = Instant::now();
        let delay = Duration::from_millis(500);
        let mut single = ClickTracker::default();
        single.press(Some(card.clone()), false);
        assert!(single.release(Some(card.clone()), now, delay).is_none());
        assert_eq!(
            single.timer_delay(now + Duration::from_millis(73)),
            Some(427)
        );
        assert_eq!(
            single.timer_delay(now + Duration::from_micros(499_001)),
            Some(1)
        );
        assert!(single.due(now + Duration::from_millis(499)).is_empty());
        assert_eq!(single.due(now + delay), [1]);
        assert_eq!(single.timer_delay(now + delay), None);
        let mut double = ClickTracker::default();
        double.press(Some(card.clone()), false);
        assert!(double.release(Some(card.clone()), now, delay).is_none());
        double.press(Some(card.clone()), true);
        assert_eq!(
            double.release(Some(card.clone()), now + delay / 2, delay),
            card.target
        );
        assert!(double.due(now + delay * 2).is_empty());
    }

    #[test]
    fn moving_rows_use_their_painted_bounds_and_retiring_cards_ignore_clicks() {
        let now = Instant::now();
        let cards: Vec<_> = (1..=2)
            .map(|id| Card {
                id,
                label: "chat".into(),
                text: "message".into(),
                final_message: false,
                attention: false,
                target: None,
            })
            .collect();
        let mut motion = Motion::new(now);
        motion.sync(&cards, &[100, 100], now, false);
        motion.sync(&cards[1..], &[100], now, true);
        let mut state = OverlayState {
            cards: cards[1..].to_vec(),
            heights: vec![100],
            rows: motion.sample(now + Duration::from_millis(90)).1,
            font: null_mut(),
            dpi: 96,
            clicks: ClickTracker::default(),
            pending_target: None,
            collapsed: false,
            hover_open: None,
            tab_pressed: false,
            close_pressed: false,
            activity: 0,
            layout: None,
            busy: false,
            attention: false,
            animate: true,
            activity_started: Instant::now(),
            buffer: PaintBuffer::default(),
            panel_buffer: PaintBuffer::default(),
            panel_dirty: true,
            panel_size: (1, 1),
            shortcut_code: None,
            shortcut_token: 0,
            restoring: false,
            compositor: None,
            render_alpha: 255,
        };
        let bounds = Rect {
            left: 0,
            top: 0,
            right: 440,
            bottom: 264,
        };
        assert_eq!(card_at(&state, bounds, 30, 42), None);
        let second_top = 40 + state.rows[0].height;
        assert_eq!(card_at(&state, bounds, 30, second_top + 5), Some(1));
        state.rows = motion.sample(now + Duration::from_millis(180)).1;
        assert_eq!(card_at(&state, bounds, 30, 45), Some(0));
        assert_eq!(state.rows[0].card.id, 2);
    }

    #[test]
    fn different_messages_in_the_same_session_do_not_form_a_double_click() {
        let first = ClickedCard {
            id: 1,
            target: Some(CardTarget {
                window: 10,
                session_id: "one".into(),
            }),
        };
        let second = ClickedCard {
            id: 2,
            target: first.target.clone(),
        };
        let now = Instant::now();
        let delay = Duration::from_millis(500);
        let mut clicks = ClickTracker::default();
        clicks.press(Some(first.clone()), false);
        clicks.release(Some(first), now, delay);
        clicks.press(Some(second.clone()), true);
        assert!(clicks.release(Some(second), now, delay).is_none());
        assert_eq!(clicks.due(now + delay), [1, 2]);
    }

    #[test]
    fn keeps_corner_positions_inside_a_secondary_monitor_work_area() {
        let work = Rect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        for position in ["top-left", "top-right", "bottom-left", "bottom-right"] {
            let rect = overlay_bounds(work, 440, 300, 20, position);
            assert!(rect.left >= work.left && rect.right <= work.right);
            assert!(rect.top >= work.top && rect.bottom <= work.bottom);
            if position.ends_with("right") {
                assert_eq!(rect.right, work.right, "expanded overlay stays flush with its screen edge");
            }
        }
        assert_eq!(
            overlay_bounds(work, 440, 300, 20, "bottom-right").bottom,
            1020
        );
    }
}
