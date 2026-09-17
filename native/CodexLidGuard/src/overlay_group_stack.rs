//! Reserve space before opening a project; release it after the last fold frame.
//! Each HWND paints on its own thread. Acknowledgements let expansion wait for
//! displaced siblings without blocking a UI thread or stealing focus.
use super::*;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Geometry {
    pub slot: usize,
    pub work: Rect,
    pub dpi: u32,
    pub top: bool,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placement {
    pub work: Rect,
    pub panel: Rect,
    pub tab: TabPlacement,
}

/// Animate the compact tab's screen position independently of its drawer.
/// The final placement is acknowledged only after this slide is painted.
#[derive(Default)]
pub(super) struct PlacementMotion {
    target: Option<Placement>,
    current: Option<Placement>,
    transition: Option<(Placement, Instant)>,
}

impl PlacementMotion {
    pub fn sample(&mut self, target: Placement, now: Instant, animate: bool) -> (Placement, bool) {
        if self.target != Some(target) {
            let from = self.current.unwrap_or(target);
            self.transition = (animate
                && (from.work.right != target.work.right || from.tab.center != target.tab.center))
                .then_some((from, now));
            self.target = Some(target);
        }
        if !animate {
            self.transition = None;
        }
        let mut placement = target;
        if let Some((from, started)) = self.transition {
            let progress =
                (now.saturating_duration_since(started).as_secs_f32() / 0.090).clamp(0.0, 1.0);
            if progress >= 1.0 {
                self.transition = None;
            } else {
                let remaining = (1.0 - progress).powi(3);
                let dx = ((from.work.right - target.work.right) as f32 * remaining).round() as i32;
                let dy = ((from.tab.center - target.tab.center) as f32 * remaining).round() as i32;
                placement.work.left += dx;
                placement.work.right += dx;
                placement.panel.left += dx;
                placement.panel.right += dx;
                placement.panel.top += dy;
                placement.panel.bottom += dy;
                placement.tab.center += dy;
            }
        }
        self.current = Some(placement);
        (placement, self.transition.is_some())
    }
}

struct Entry {
    window: usize,
    geometry: Geometry,
    reserved: bool,
    space_height: i32,
    placement: Option<Placement>,
    painted: Option<Placement>,
}

static STACK: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

fn plan(entries: &mut [Entry]) {
    entries.sort_by_key(|entry| entry.geometry.slot);
    let mut groups: Vec<(Rect, bool)> = Vec::new();
    for entry in entries.iter() {
        let key = (entry.geometry.work, entry.geometry.top);
        if !groups.contains(&key) {
            groups.push(key);
        }
    }
    for (work, top) in groups {
        let indices: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.geometry.work == work && entry.geometry.top == top)
            .map(|(i, _)| i)
            .collect();
        let dpi = entries[indices[0]].geometry.dpi;
        let d = |n| scale_dip(n, dpi);
        let margin = d(8);
        let gap = d(6);
        let tab_height = d(group_window::TAB_HEIGHT);
        let mut columns: Vec<Vec<usize>> = vec![vec![]];
        let mut used = vec![0];
        for index in indices {
            let entry = &entries[index];
            let height = if entry.reserved {
                entry.space_height
            } else {
                tab_height
            };
            // Fill spare space in earlier columns after placing a tall drawer.
            // Otherwise a middle slot can force a mostly empty third column.
            let column = used
                .iter()
                .position(|&used| used + height <= work.bottom - work.top - margin * 2)
                .unwrap_or_else(|| {
                    columns.push(vec![]);
                    used.push(0);
                    columns.len() - 1
                });
            columns[column].push(index);
            used[column] += height + gap;
        }
        let mut right = work.right;
        for column in columns {
            let width = column
                .iter()
                .map(|&i| {
                    if entries[i].reserved {
                        entries[i].geometry.width
                    } else {
                        d(group_window::TAB_WIDTH)
                    }
                })
                .max()
                .unwrap();
            let mut cursor = if top {
                work.top + margin
            } else {
                work.bottom - margin
            };
            for index in column {
                let entry = &mut entries[index];
                let g = entry.geometry;
                let panel_top = if top { cursor } else { cursor - g.height };
                // The hidden panel can cross a sibling, but always stays within its display.
                let panel_top = panel_top.clamp(work.top, (work.bottom - g.height).max(work.top));
                let tab_top = if top { cursor } else { cursor - tab_height };
                let placement = Some(Placement {
                    work: Rect {
                        left: (right - width).max(work.left),
                        right,
                        ..work
                    },
                    panel: Rect {
                        left: right - g.width,
                        top: panel_top,
                        right,
                        bottom: panel_top + g.height,
                    },
                    tab: TabPlacement {
                        center: tab_top + tab_height / 2,
                        height: tab_height,
                    },
                });
                // A rapid reversal may target a previously acknowledged slot
                // while the tab is still between positions. Require a fresh paint.
                if entry.placement != placement {
                    entry.painted = None;
                }
                entry.placement = placement;
                cursor += if top { 1 } else { -1 }
                    * (if entry.reserved {
                        entry.space_height
                    } else {
                        tab_height
                    } + gap);
            }
            right -= width + gap;
        }
    }
}

fn notify(windows: Vec<usize>) {
    for window in windows {
        unsafe {
            PostMessageW(window as Hwnd, WM_APP_STACK_LAYOUT, 0, 0);
        }
    }
}

pub(super) fn update(window: Hwnd, geometry: Geometry, reserve: bool) {
    let windows = {
        let mut entries = STACK.lock().unwrap();
        // Finish folding the previous drawer before allocating the next one.
        let reserve = reserve
            && !entries
                .iter()
                .any(|entry| entry.window != window as usize && entry.reserved);
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.window == window as usize)
        {
            // Closing keeps its reservation until the completely folded frame is painted.
            if entry.geometry == geometry && (!reserve || entry.reserved) {
                return;
            }
            entry.geometry = geometry;
            entry.space_height = if entry.reserved {
                entry.space_height.max(geometry.height)
            } else {
                geometry.height
            };
            entry.reserved |= reserve;
        } else {
            entries.push(Entry {
                window: window as usize,
                geometry,
                reserved: reserve,
                space_height: geometry.height,
                placement: None,
                painted: None,
            });
        }
        plan(&mut entries);
        entries
            .iter()
            .filter(|entry| entry.placement != entry.painted)
            .map(|entry| entry.window)
            .collect()
    };
    notify(windows);
}

pub(super) fn placement(window: Hwnd) -> Option<Placement> {
    STACK
        .lock()
        .unwrap()
        .iter()
        .find(|entry| entry.window == window as usize)
        .and_then(|entry| entry.placement)
}

pub(super) fn ready(window: Hwnd) -> bool {
    let entries = STACK.lock().unwrap();
    let Some(own) = entries.iter().find(|entry| entry.window == window as usize) else {
        return true;
    };
    own.reserved
        && entries.iter().all(|entry| {
            entry.window == window as usize
                || entry.geometry.work != own.geometry.work
                || entry.placement == entry.painted
        })
}

pub(super) fn painted(window: Hwnd, placement: Placement, folded: bool) {
    let windows = {
        let mut entries = STACK.lock().unwrap();
        let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.window == window as usize)
        else {
            return;
        };
        let changed = entry.painted != Some(placement);
        entry.painted = Some(placement);
        let release = entry.reserved && (folded || entry.space_height != entry.geometry.height);
        if release {
            entry.reserved = !folded;
            entry.space_height = entry.geometry.height;
            plan(&mut entries);
        }
        if !changed && !release {
            return;
        }
        entries.iter().map(|entry| entry.window).collect()
    };
    notify(windows);
}

pub(super) fn remove(window: Hwnd) {
    let windows = {
        let mut entries = STACK.lock().unwrap();
        let count = entries.len();
        entries.retain(|entry| entry.window != window as usize);
        if count == entries.len() {
            return;
        }
        plan(&mut entries);
        entries.iter().map(|entry| entry.window).collect()
    };
    notify(windows);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reversing_a_tab_in_flight_requires_a_new_position_acknowledgement() {
        let mut entries: Vec<_> = (0..2)
            .map(|slot| Entry {
                window: slot + 1,
                geometry: Geometry {
                    slot,
                    work: Rect {
                        left: 0,
                        top: 0,
                        right: 1920,
                        bottom: 1080,
                    },
                    dpi: 96,
                    top: false,
                    width: 344,
                    height: 137,
                },
                reserved: false,
                space_height: 137,
                placement: None,
                painted: None,
            })
            .collect();
        plan(&mut entries);
        let original = entries[1].placement;
        entries[1].painted = original;
        // The neighbor starts sliding out, then expansion is cancelled before it arrives.
        entries[0].reserved = true;
        plan(&mut entries);
        entries[0].reserved = false;
        plan(&mut entries);
        assert_eq!(entries[1].placement, original);
        assert_eq!(
            entries[1].painted, None,
            "an old acknowledgement cannot stand in for the return slide"
        );
    }
    #[test]
    fn tab_slide_has_intermediate_frames_and_reverses_from_its_current_position() {
        let start = Instant::now();
        for dpi in [96, 144, 192] {
            let d = |n| scale_dip(n, dpi);
            let from = Placement {
                work: Rect {
                    left: -1000,
                    top: 0,
                    right: 0,
                    bottom: 1000,
                },
                panel: Rect {
                    left: -d(344),
                    top: d(100),
                    right: 0,
                    bottom: d(237),
                },
                tab: TabPlacement {
                    center: d(216),
                    height: d(42),
                },
            };
            let mut to = from;
            to.work.left -= d(160);
            to.work.right -= d(160);
            to.panel.left -= d(160);
            to.panel.right -= d(160);
            to.panel.top -= d(95);
            to.panel.bottom -= d(95);
            to.tab.center -= d(95);
            let mut motion = PlacementMotion::default();
            assert_eq!(motion.sample(from, start, true), (from, false));
            assert_eq!(motion.sample(to, start, true), (from, true));
            let (middle, moving) = motion.sample(to, start + Duration::from_millis(35), true);
            assert!(moving);
            assert!(middle.tab.center > to.tab.center && middle.tab.center < from.tab.center);
            assert!(middle.work.right > to.work.right && middle.work.right < from.work.right);
            let reverse = start + Duration::from_millis(35);
            assert_eq!(motion.sample(from, reverse, true), (middle, true));
            assert_eq!(
                motion.sample(from, reverse + Duration::from_millis(90), true),
                (from, false)
            );
            assert_eq!(
                motion.sample(to, reverse + Duration::from_millis(91), false),
                (to, false),
                "reduced motion still uses the same final space reservation"
            );
        }
    }
    #[test]
    fn expansion_reserves_space_and_folding_restores_every_slot() {
        for (screen_width, screen_height) in [(1920, 1080), (1280, 720)] {
            for dpi in [96, 144, 192] {
                for top in [false, true] {
                    for opened in 0..10 {
                        let d = |n| scale_dip(n, dpi);
                        let mut entries: Vec<_> = (0..10)
                            .map(|slot| Entry {
                                window: slot + 1,
                                geometry: Geometry {
                                    slot,
                                    work: Rect {
                                        left: -screen_width,
                                        top: -100,
                                        right: 0,
                                        bottom: screen_height - 100,
                                    },
                                    dpi,
                                    top,
                                    width: d(group_window::PANEL_WIDTH),
                                    height: d(221),
                                },
                                reserved: false,
                                space_height: d(221),
                                placement: None,
                                painted: None,
                            })
                            .collect();
                        plan(&mut entries);
                        let before: Vec<_> = entries.iter().map(|entry| entry.placement).collect();
                        entries[opened].reserved = true;
                        plan(&mut entries);
                        let rects: Vec<_> = entries
                            .iter()
                            .map(|entry| {
                                let p = entry.placement.unwrap();
                                if entry.reserved {
                                    p.panel
                                } else {
                                    Rect {
                                        left: p.work.right - d(group_window::TAB_WIDTH),
                                        top: p.tab.center - p.tab.height / 2,
                                        right: p.work.right,
                                        bottom: p.tab.center - p.tab.height / 2 + p.tab.height,
                                    }
                                }
                            })
                            .collect();
                        for (index, a) in rects.iter().enumerate() {
                            assert!(
                                a.top >= -100
                                    && a.bottom <= screen_height - 100
                                    && a.left >= -screen_width
                                    && a.right <= 0,
                                "off-screen at {screen_width}x{screen_height}, dpi {dpi}, top {top}, opened {opened}: {a:?}"
                            );
                            for b in rects.iter().skip(index + 1) {
                                assert!(
                                    a.right <= b.left
                                        || b.right <= a.left
                                        || a.bottom <= b.top
                                        || b.bottom <= a.top,
                                    "overlap at dpi {dpi}, top {top}, opened {opened}: {a:?} {b:?}"
                                );
                            }
                        }
                        let p = entries[opened].placement.unwrap();
                        for step in 0..=100 {
                            let a = overlay_dock::dock_layout_sized(
                                p.panel,
                                p.work,
                                step as f32 / 100.0,
                                dpi,
                                None,
                                Some(p.tab),
                                d(group_window::TAB_WIDTH),
                            )
                            .window;
                            for (i, b) in rects.iter().enumerate() {
                                if i == opened {
                                    continue;
                                }
                                assert!(
                                    a.right <= b.left
                                        || b.right <= a.left
                                        || a.bottom <= b.top
                                        || b.bottom <= a.top,
                                    "slide intersects a neighboring tab: {a:?} {b:?}"
                                );
                            }
                        }
                        entries[opened].reserved = false;
                        plan(&mut entries);
                        assert_eq!(
                            before,
                            entries
                                .iter()
                                .map(|entry| entry.placement)
                                .collect::<Vec<_>>()
                        );
                    }
                }
            }
        }
    }
}
