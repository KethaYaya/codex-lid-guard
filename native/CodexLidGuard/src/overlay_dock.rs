//! Clip the sliding panel to its own display, even with a monitor on its right.
use super::{Rect, scale_dip};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DockLayout {
    pub window: Rect,
    pub panel: Option<Rect>,
    pub tab: Option<Rect>,
}

// A newly backgrounded message folds into its tab without reflowing its text.
pub(super) fn arrival_layout(expanded: Rect, work: Rect, progress: f32, dpi: u32) -> DockLayout {
    let target = dock_layout(expanded, work, 1.0, dpi, None);
    if progress >= 1.0 {
        return target;
    }
    let p = progress.clamp(0.0, 1.0);
    let lerp = |a: i32, b: i32| a + ((b - a) as f32 * p).round() as i32;
    let window = Rect {
        left: lerp(expanded.left, target.window.left),
        top: lerp(expanded.top, target.window.top),
        right: lerp(expanded.right, target.window.right),
        bottom: lerp(expanded.bottom, target.window.bottom),
    };
    DockLayout {
        window,
        panel: Some(Rect {
            left: 0,
            top: 0,
            right: window.right - window.left,
            bottom: window.bottom - window.top,
        }),
        tab: None,
    }
}

pub(super) fn dock_layout(
    expanded: Rect,
    work: Rect,
    progress: f32,
    dpi: u32,
    tab_center: Option<i32>,
) -> DockLayout {
    let width = expanded.right - expanded.left;
    let height = expanded.bottom - expanded.top;
    let tab_width = scale_dip(28, dpi).min(work.right - work.left);
    let tab_height = scale_dip(64, dpi).min(height);
    let travel = work.right - expanded.left;
    let distance = (travel as f32 * progress.clamp(0.0, 1.0)).round() as i32;
    let left = expanded.left + distance;
    let docked_top = (tab_center.unwrap_or(expanded.top + height / 2) - tab_height / 2)
        .clamp(work.top, work.bottom - tab_height);
    // A changed message height can leave the stored tab outside the panel.
    // Bring it inside before its final pixels retract, avoiding a vertical snap too.
    let attached_top = docked_top.clamp(expanded.top, expanded.bottom - tab_height);
    let placement =
        ((distance - tab_width) as f32 / (travel - tab_width).max(1) as f32).clamp(0.0, 1.0);
    let tab_top = attached_top + ((docked_top - attached_top) as f32 * placement).round() as i32;
    if left >= work.right {
        return DockLayout {
            window: Rect {
                left: work.right - tab_width,
                top: tab_top,
                right: work.right,
                bottom: tab_top + tab_height,
            },
            panel: None,
            tab: Some(Rect {
                left: 0,
                top: 0,
                right: tab_width,
                bottom: tab_height,
            }),
        };
    }
    // Keep this transparent gutter after expansion. Removing it changed both
    // HWND bounds and the paint origin in the last frame, causing the visible jump.
    // The window region excludes the gutter, so it never intercepts clicks.
    let window_left = (left - tab_width).max(work.left);
    let visible_tab_width = distance.clamp(0, tab_width);
    let window_top = expanded.top.min(tab_top);
    let window_bottom = expanded.bottom.max(tab_top + tab_height);
    DockLayout {
        window: Rect {
            left: window_left,
            top: window_top,
            right: (left + width).min(work.right),
            bottom: window_bottom,
        },
        panel: Some(Rect {
            left: left - window_left,
            top: expanded.top - window_top,
            right: left - window_left + width,
            bottom: expanded.bottom - window_top,
        }),
        tab: (visible_tab_width > 0).then_some(Rect {
            left: left - visible_tab_width - window_left,
            top: tab_top - window_top,
            right: left - window_left,
            bottom: tab_top - window_top + tab_height,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::super::overlay_bounds;
    use super::*;

    #[test]
    fn arrival_shrinks_continuously_into_the_exact_tab_at_every_display_scale() {
        let work = Rect {
            left: -1920,
            top: -100,
            right: 0,
            bottom: 980,
        };
        for dpi in [96, 144, 192] {
            for position in ["top-left", "top-right", "bottom-left", "bottom-right"] {
                let expanded = overlay_bounds(
                    work,
                    scale_dip(440, dpi),
                    scale_dip(210, dpi),
                    scale_dip(20, dpi),
                    position,
                );
                let mut previous = expanded;
                for step in 0..=100 {
                    let layout = arrival_layout(expanded, work, step as f32 / 100.0, dpi);
                    let rect = layout.window;
                    assert!(rect.left >= previous.left);
                    assert!(rect.right - rect.left <= previous.right - previous.left);
                    assert!(rect.bottom - rect.top <= previous.bottom - previous.top);
                    assert!(
                        rect.right <= work.right
                            && rect.top >= work.top
                            && rect.bottom <= work.bottom
                    );
                    previous = rect;
                }
                assert_eq!(
                    arrival_layout(expanded, work, 1.0, dpi),
                    dock_layout(expanded, work, 1.0, dpi, None)
                );
                assert_eq!(
                    arrival_layout(expanded, work, 0.99999, dpi).window,
                    previous,
                    "no final position snap"
                );
            }
        }
    }

    #[test]
    fn hiding_the_tab_does_not_snap_the_window_or_its_content_origin() {
        let work = Rect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        for dpi in [96, 144, 192] {
            for position in ["top-left", "top-right", "bottom-left", "bottom-right"] {
                let expanded = overlay_bounds(
                    work,
                    scale_dip(440, dpi),
                    scale_dip(240, dpi),
                    scale_dip(20, dpi),
                    position,
                );
                for center in [None, Some(expanded.top - 100), Some(expanded.bottom + 100)] {
                    let travel = (work.right - expanded.left) as f32;
                    let retracting = dock_layout(expanded, work, 8.0 / travel, dpi, center);
                    let tab = retracting.tab.unwrap();
                    assert_eq!(
                        tab.right - tab.left,
                        8,
                        "the tab must shrink before it disappears"
                    );
                    let panel = retracting.panel.unwrap();
                    assert_eq!(panel.right - panel.left, expanded.right - expanded.left);
                    let before = dock_layout(expanded, work, 0.25 / travel, dpi, center);
                    let after = dock_layout(expanded, work, 0.0, dpi, None);
                    assert_eq!(
                        before.window, after.window,
                        "removing the last tab pixel must not resize or shift the native window"
                    );
                    assert_eq!(
                        before.panel, after.panel,
                        "the paint origin must remain unchanged at the end of the slide"
                    );
                    assert!(after.tab.is_none());
                }
            }
        }
    }

    #[test]
    fn slides_to_a_small_right_tab_without_spilling_into_an_adjacent_display() {
        for dpi in [96, 144, 192] {
            for work in [
                Rect {
                    left: -1920,
                    top: -200,
                    right: 0,
                    bottom: 1080,
                },
                Rect {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1040,
                },
            ] {
                for position in ["top-left", "top-right", "bottom-left", "bottom-right"] {
                    let expanded = overlay_bounds(
                        work,
                        scale_dip(440, dpi),
                        scale_dip(300, dpi),
                        scale_dip(20, dpi),
                        position,
                    );
                    let mut previous_left = expanded.left;
                    for progress in [0.0, 0.25, 0.5, 0.9, 1.0] {
                        let layout = dock_layout(expanded, work, progress, dpi, None);
                        assert!(
                            layout.window.left >= work.left && layout.window.right <= work.right
                        );
                        assert!(
                            layout.window.top >= work.top && layout.window.bottom <= work.bottom
                        );
                        if let Some(panel) = layout.panel {
                            let left = layout.window.left + panel.left;
                            assert!(left >= previous_left);
                            previous_left = left;
                        }
                    }
                    let docked = dock_layout(expanded, work, 1.0, dpi, None);
                    assert!(docked.panel.is_none());
                    assert_eq!(docked.window.right, work.right);
                    assert_eq!(docked.window.right - docked.window.left, scale_dip(28, dpi));
                    assert_eq!(docked.window.bottom - docked.window.top, scale_dip(64, dpi));
                    let open = dock_layout(expanded, work, 0.0, dpi, None);
                    let panel = open.panel.unwrap();
                    assert_eq!(
                        Rect {
                            left: open.window.left + panel.left,
                            top: open.window.top + panel.top,
                            right: open.window.left + panel.right,
                            bottom: open.window.top + panel.bottom
                        },
                        expanded
                    );
                }
            }
        }
    }

    #[test]
    fn incoming_cards_do_not_shift_the_tab_up_or_down() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let small = overlay_bounds(work, 440, 200, 20, "bottom-right");
        let large = overlay_bounds(work, 440, 450, 20, "bottom-right");
        let center = (small.top + small.bottom) / 2;
        assert_eq!(
            dock_layout(small, work, 1.0, 96, Some(center)),
            dock_layout(large, work, 1.0, 96, Some(center))
        );
        let expanding = dock_layout(large, work, 0.5, 96, Some(center));
        let tab = expanding.tab.unwrap();
        assert_eq!(expanding.window.top + (tab.top + tab.bottom) / 2, center);
    }
}
