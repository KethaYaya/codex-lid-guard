//! Move project tabs vertically without activating their chat or leaving the edge.
use super::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct SavedPositions {
    anchors: BTreeMap<String, u16>,
    #[serde(skip)]
    dirty: bool,
}

impl SavedPositions {
    fn load(path: &Path) -> Self {
        // This file contains only project identities and relative coordinates.
        std::fs::metadata(path).ok().filter(|metadata| metadata.len() <= 1024 * 1024)
            .and_then(|_| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn save(&mut self, path: &Path) -> io::Result<()> {
        if !self.dirty { return Ok(()); }
        atomic_write(path, &serde_json::to_vec(self)?)?;
        self.dirty = false;
        Ok(())
    }

    fn remember(&mut self, key: String, anchor: u16) {
        if self.anchors.insert(key, anchor) != Some(anchor) { self.dirty = true; }
    }
}

fn positions() -> &'static Mutex<SavedPositions> {
    static POSITIONS: OnceLock<Mutex<SavedPositions>> = OnceLock::new();
    POSITIONS.get_or_init(|| Mutex::new({
        #[cfg(not(test))]
        { SavedPositions::load(&paths::data_directory().join("overlay-positions.json")) }
        // Native fixtures must never read or overwrite the user's positions.
        #[cfg(test)]
        { SavedPositions::default() }
    }))
}

fn position_key(project: &str, position: &str) -> String { format!("{position}\n{project}") }

pub(super) fn save_positions() {
    #[cfg(not(test))]
    if let Err(cause) = positions().lock().unwrap().save(&paths::data_directory().join("overlay-positions.json")) {
        logging::write(format!("Could not save dragged overlay positions: {cause}"));
    }
}

pub(super) const HOVER_TIMER: usize = 9;
const HOVER_DELAY: Duration = Duration::from_millis(350);

#[link(name = "user32")]
unsafe extern "system" {
    fn GetSystemMetricsForDpi(index: i32, dpi: u32) -> i32;
}

#[derive(Default)]
pub(super) struct Interaction {
    pub anchor: Option<u16>,
    project: Option<String>,
    position: String,
    gesture: Option<Gesture>,
    hover_started: Option<Instant>,
    resume_hover: bool,
    // Dropping the tab under the pointer must not immediately unfold it.
    suppress_hover: bool,
}

struct Gesture {
    start: (i32, i32),
    center: i32,
    threshold: (i32, i32),
    moved: bool,
}

impl Gesture {
    fn target(&mut self, point: (i32, i32)) -> Option<i32> {
        self.moved |= (point.0 - self.start.0).abs() >= self.threshold.0
            || (point.1 - self.start.1).abs() >= self.threshold.1;
        self.moved.then_some(self.center + point.1 - self.start.1)
    }
}

impl Interaction {
    pub fn pressed(&self) -> bool { self.gesture.is_some() }
    pub fn moving(&self) -> bool { self.gesture.as_ref().is_some_and(|gesture| gesture.moved) }

    pub unsafe fn bind(&mut self, window: Hwnd, project: Option<&str>, position: &str) {
        unsafe { self.cancel_hover(window); }
        save_positions();
        *self = Self {
            anchor: project.and_then(|project| positions().lock().unwrap().anchors.get(&position_key(project, position)).copied()),
            project: project.map(str::to_owned),
            position: position.into(),
            ..Self::default()
        };
    }

    pub fn select_position(&mut self, position: &str) {
        if self.position == position { return; }
        save_positions();
        self.position = position.into();
        self.anchor = self.project.as_ref().and_then(|project|
            positions().lock().unwrap().anchors.get(&position_key(project, position)).copied());
    }

    pub unsafe fn cancel_hover(&mut self, window: Hwnd) {
        self.hover_started = None;
        unsafe { KillTimer(window, HOVER_TIMER); }
    }

    pub unsafe fn press(&mut self, window: Hwnd, layout: DockLayout, point: (i32, i32), dpi: u32, resume_hover: bool) {
        let Some(center) = layout.tab.map(|tab| layout.window.top + (tab.top + tab.bottom) / 2)
            .or_else(|| group_stack::placement(window).map(|placement| placement.tab.center)) else { return; };
        unsafe {
            self.cancel_hover(window);
            self.suppress_hover = false;
            self.resume_hover = resume_hover;
            self.gesture = Some(Gesture {
                start: (layout.window.left + point.0, layout.window.top + point.1),
                center,
                threshold: (GetSystemMetricsForDpi(68, dpi).abs().max(1), GetSystemMetricsForDpi(69, dpi).abs().max(1)),
                moved: false,
            });
        }
    }

    pub fn release(&mut self) -> bool {
        let moved = self.gesture.take().is_some_and(|gesture| gesture.moved);
        self.suppress_hover |= moved;
        if moved { save_positions(); }
        moved
    }

    pub unsafe fn hover(&mut self, window: Hwnd) {
        if self.suppress_hover || self.gesture.is_some() || self.hover_started.is_some() { return; }
        unsafe {
            if SetTimer(window, HOVER_TIMER, 50, null()) != 0 { self.hover_started = Some(Instant::now()); }
        }
    }

    pub unsafe fn hover_tick(&mut self, window: Hwnd, layout: Option<DockLayout>, pointer: Option<(i32, i32)>) -> bool {
        let inside = layout.is_some_and(|layout| layout.panel.is_none() && pointer.is_none_or(|(x, y)| {
            let rect = layout.window;
            x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
        }));
        if !inside {
            self.suppress_hover = false;
            unsafe { self.cancel_hover(window); }
            return false;
        }
        if self.hover_started.is_some_and(|started| started.elapsed() >= HOVER_DELAY) {
            unsafe { self.cancel_hover(window); }
            return !self.suppress_hover && self.gesture.is_none();
        }
        false
    }

    pub unsafe fn watch_leave(&mut self, window: Hwnd) {
        // Poll only until the pointer leaves the just-dropped tab. This also
        // handles a stationary pointer after the native window itself moves.
        if self.suppress_hover { unsafe { SetTimer(window, HOVER_TIMER, 50, null()); } }
    }
}

pub(super) unsafe fn release(window: Hwnd, state: &mut OverlayState) -> bool {
    let moved = state.tab_drag.release();
    if std::mem::take(&mut state.tab_drag.resume_hover) && !state.collapsed
        && let Some(layout) = state.layout {
        state.hover_open = Some(HoverOpen::new(layout.window));
        unsafe { SetTimer(window, 5, 50, null()); }
    }
    moved
}

pub(super) unsafe fn mouse_move(window: Hwnd, state: &mut OverlayState, lparam: isize) -> bool {
    let Some(gesture) = &mut state.tab_drag.gesture else { return false; };
    let Some(layout) = state.layout else { return true; };
    // WM_MOUSEMOVE is relative to the moving HWND; recover screen coordinates
    // from the current layout instead of accumulating client-coordinate deltas.
    let point = (layout.window.left + lparam as i16 as i32,
        layout.window.top + (lparam >> 16) as i16 as i32);
    if let Some(center) = gesture.target(point) {
        // A focus/layout transition can briefly remove this HWND from the
        // stack. Keep the last successful anchor until it is registered again.
        if let Some(anchor) = group_stack::move_tab(window, center) {
            state.tab_drag.anchor = Some(anchor);
            if let Some(project) = &state.tab_drag.project {
                positions().lock().unwrap().remember(position_key(project, &state.tab_drag.position), anchor);
            }
        }
        state.group_action = None;
        if let Some(ui) = &mut state.group { ui.pressed = None; ui.last_expand_click = None; }
        unsafe { SetCursor(LoadCursorW(null_mut(), 32_645usize as *const u16)); }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_positions_survive_restart_and_keep_projects_and_corners_separate() {
        let path = std::env::temp_dir().join(format!("lid-guard-drag-positions-{}.json", std::process::id()));
        let own = position_key(r"project:c:\one", "bottom-right");
        let top = position_key(r"project:c:\one", "top-right");
        let other = position_key(r"project:c:\two", "bottom-right");
        let mut saved = SavedPositions::default();
        saved.remember(own.clone(), 20_000);
        saved.remember(top.clone(), 10_000);
        saved.remember(other.clone(), 40_000);
        saved.save(&path).unwrap();
        drop(saved);
        let mut restarted = SavedPositions::load(&path);
        assert_eq!(restarted.anchors.get(&own), Some(&20_000));
        assert_eq!(restarted.anchors.get(&top), Some(&10_000));
        assert_eq!(restarted.anchors.get(&other), Some(&40_000));
        assert!(!restarted.dirty);
        restarted.remember(own.clone(), 25_000);
        restarted.save(&path).unwrap();
        assert_eq!(SavedPositions::load(&path).anchors.get(&own), Some(&25_000), "a later drop atomically replaces the saved position");
        std::fs::write(&path, b"incomplete").unwrap();
        assert!(SavedPositions::load(&path).anchors.is_empty(), "invalid data must not break the overlay");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rebinding_a_tab_does_not_erase_its_saved_corner_position() {
        let project = "project:drag-rebind-fixture";
        positions().lock().unwrap().remember(position_key(project, "top-right"), 12_345);
        let mut interaction = Interaction::default();
        unsafe { interaction.bind(null_mut(), Some(project), "top-right"); }
        interaction.select_position("top-right"); // First frame starts with a different previous corner.
        assert_eq!(interaction.anchor, Some(12_345));
        interaction.select_position("bottom-right");
        assert_eq!(interaction.anchor, None);
        interaction.select_position("top-right");
        assert_eq!(interaction.anchor, Some(12_345));
        unsafe {
            interaction.bind(null_mut(), None, "bottom-right");
            interaction.bind(null_mut(), Some(project), "top-right");
        }
        assert_eq!(interaction.anchor, Some(12_345), "hiding and returning to a project keeps its anchor");
    }

    #[test]
    fn dragging_uses_a_threshold_and_the_original_pointer_offset() {
        let mut gesture = Gesture { start: (-10, -200), center: -210, threshold: (4, 6), moved: false };
        assert_eq!(gesture.target((-9, -195)), None);
        assert_eq!(gesture.target((-9, -194)), Some(-204));
        assert_eq!(gesture.target((-400, -310)), Some(-320));
        assert_eq!(gesture.target((-10, -200)), Some(-210), "returning to the start is still a drag");
    }
}
