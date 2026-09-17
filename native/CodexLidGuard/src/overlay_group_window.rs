//! Compact task-list tabs and a fixed master-detail project drawer.
use super::*;
use crate::overlay::groups::{GroupSession, ProjectGroup};
#[path = "overlay_glass.rs"]
pub(super) mod glass;
#[path = "overlay_tab_state.rs"]
pub(super) mod tab_state;

// Keep a stable maximum column reservation while individual tabs change width.
pub(super) const TAB_WIDTH: i32 = tab_state::NOTICE_WIDTH;
pub(super) const TAB_HEIGHT: i32 = 42;
pub(super) const PANEL_WIDTH: i32 = 344;
pub(super) const MESSAGE_WIDTH: i32 = 560;
const HEADER: i32 = 28;
const ROW: i32 = 28;
const PREVIEW: i32 = 76;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
    Select(String),
    Open(CardTarget),
    Send,
    NewChat,
    Fold,
    Scroll(i32),
    ScrollThumb,
    HistoryScroll(i32),
    HistoryThumb,
    Dismiss(CardTarget, u64),
}

pub(super) struct GroupUi {
    pub group: ProjectGroup,
    pub tab: tab_state::TabState,
    pub selected: Option<String>,
    remembered: Option<String>,
    order: Vec<String>,
    pub pressed: Option<Action>,
    pub double: bool,
    pub dirty: bool,
    pub key_hint: String,
    offset: usize,
    visible_rows: usize,
    reveal_selection: bool,
    wheel_remainder: i32,
    drag_grab: i32,
    pub height: i32,
    pub chat: Option<chat_panel::Conversation>,
    pub progression: expand_state::Progression,
    pub message_anchor: Option<Rect>,
    width: i32,
    footer_top: i32,
    list: Rect,
    preview_top: i32,
    scrollbar: Option<(Rect, Rect)>,
    hits: Vec<(Rect, Action)>,
}

impl GroupUi {
    pub fn new(mut group: ProjectGroup) -> Self {
        group.sessions.sort_by_key(GroupSession::priority);
        let selected = group.sessions.first().map(|s| s.id.clone());
        let order = group.sessions.iter().map(|s| s.id.clone()).collect();
        let tab = tab_state::TabState::new(&group, Instant::now());
        Self {
            group,
            tab,
            selected,
            remembered: None,
            order,
            pressed: None,
            double: false,
            dirty: true,
            key_hint: "Enter opens \u{b7} Esc dismisses".into(),
            offset: 0,
            visible_rows: 4,
            reveal_selection: true,
            wheel_remainder: 0,
            drag_grab: 0,
            height: 0,
            chat: None,
            progression: expand_state::Progression::default(),
            message_anchor: None,
            width: 0,
            footer_top: 0,
            list: unsafe { zeroed() },
            preview_top: 0,
            scrollbar: None,
            hits: vec![],
        }
    }

    pub fn sync(&mut self, mut group: ProjectGroup) {
        let previous_selection = self.selected.clone();
        self.tab.observe(&group, Instant::now());
        for session in &group.sessions {
            if !self.order.contains(&session.id) {
                self.order.push(session.id.clone());
            }
        }
        if self.order.len() > 64 {
            self.order.retain(|id| {
                group.sessions.iter().any(|s| &s.id == id) || self.remembered.as_ref() == Some(id)
            });
        }
        group.sessions.sort_by_key(|s| {
            (
                s.priority(),
                self.order
                    .iter()
                    .position(|id| id == &s.id)
                    .unwrap_or(usize::MAX),
            )
        });
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !group.sessions.iter().any(|s| &s.id == id))
        {
            if self.remembered.is_none() {
                self.remembered = self.selected.clone();
            }
            self.selected = None;
        }
        if self
            .remembered
            .as_ref()
            .is_some_and(|id| group.sessions.iter().any(|s| &s.id == id))
        {
            self.selected = self.remembered.take();
            self.reveal_selection = true;
        }
        if self.selected.is_none() {
            self.selected = group.sessions.first().map(|s| s.id.clone());
            self.reveal_selection = true;
        }
        if self.selected != previous_selection && self.chat.is_some() {
            self.progression.clear_typing();
            self.chat = Some(chat_panel::Conversation::default());
        }
        self.dirty |= self.group != group;
        self.group = group;
    }

    pub fn selected_session(&self) -> Option<&GroupSession> {
        self.selected
            .as_ref()
            .and_then(|id| self.group.sessions.iter().find(|s| &s.id == id))
    }

    pub fn input_rect(&self, dpi: u32) -> Rect {
        let d = |n| scale_dip(n, dpi);
        Rect { left: d(12), top: self.footer_top, right: self.width - d(188), bottom: self.footer_top + d(26) }
    }

    fn send_rect(&self, dpi: u32) -> Rect {
        let d = |n| scale_dip(n, dpi);
        Rect { left: self.width - d(184), right: self.width - d(158), ..self.input_rect(dpi) }
    }

    fn new_chat_rect(&self, dpi: u32) -> Rect {
        let d = |n| scale_dip(n, dpi);
        Rect { left: self.width - d(64), top: d(4), right: self.width - d(40), bottom: d(26) }
    }

    fn open_rect(&self, dpi: u32) -> Rect {
        Rect { left: self.width - scale_dip(152, dpi), right: self.width - scale_dip(72, dpi), ..self.input_rect(dpi) }
    }
    fn dismiss_rect(&self, dpi: u32) -> Rect {
        Rect { left: self.width - scale_dip(68, dpi), right: self.width - scale_dip(12, dpi), ..self.input_rect(dpi) }
    }

    #[cfg(test)]
    pub fn tab_height(&self) -> i32 {
        TAB_HEIGHT
    }

    fn header_height(&self) -> i32 {
        HEADER + if self.group.disambiguate { 20 } else { 0 }
    }

    pub fn message_bounds(&mut self, work: Rect, dpi: u32, position: &str) -> Rect {
        let margin = scale_dip(20, dpi);
        let available = (work.bottom - work.top - margin * 2).max(1);
        let width = scale_dip(MESSAGE_WIDTH, dpi).min((work.right - work.left - margin * 2).max(1));
        if let Some(chat) = &mut self.chat { chat.wrap_at(None); }
        self.layout(width, available, dpi);
        let height = self.chat.as_ref().map_or(available, |chat|
            (chat.bounds.top + self.height - chat.bounds.bottom + chat.content_height().max(scale_dip(64, dpi))).min(available));
        self.layout(width, height, dpi);
        let anchor = self.message_anchor.unwrap_or(work);
        let top = if position.starts_with("top") { anchor.top } else { anchor.bottom - height }
            .clamp(work.top + margin, work.bottom - margin - height);
        Rect { left: work.right - width, right: work.right, top, bottom: top + height }
    }

    pub fn layout(&mut self, width: i32, available: i32, dpi: u32) {
        let d = |n| scale_dip(n, dpi);
        self.visible_rows =
            ((available - d(self.header_height() + PREVIEW + 4)) / d(ROW)).clamp(1, 4) as usize;
        let count = self.group.sessions.len().min(self.visible_rows);
        self.offset = self
            .offset
            .min(self.group.sessions.len().saturating_sub(count));
        if self.reveal_selection {
            if let Some(index) = self
                .group
                .sessions
                .iter()
                .position(|s| self.selected.as_ref() == Some(&s.id))
            {
                if index < self.offset {
                    self.offset = index;
                } else if index >= self.offset + self.visible_rows {
                    self.offset = index + 1 - self.visible_rows;
                }
            }
            self.reveal_selection = false;
        }
        self.list = Rect {
            left: 0,
            top: d(self.header_height()),
            right: width,
            bottom: d(self.header_height()) + d(ROW) * count as i32,
        };
        self.preview_top = self.list.bottom + 1;
        self.width = width;
        self.height = if self.chat.is_some() { available } else { self.preview_top + d(PREVIEW + 4) };
        self.footer_top = self.height - d(34);
        self.hits.clear();
        self.hits.push((
            Rect {
                left: width - d(38),
                top: d(4),
                right: width - d(4),
                bottom: d(26),
            },
            Action::Fold,
        ));
        for (index, session) in self
            .group
            .sessions
            .iter()
            .skip(self.offset)
            .take(count)
            .enumerate()
        {
            let y = self.list.top + d(ROW) * index as i32;
            self.hits.push((
                Rect {
                    left: d(4),
                    top: y,
                    right: width - d(14),
                    bottom: y + d(ROW),
                },
                Action::Select(session.id.clone()),
            ));
        }
        if let Some(session) = self.selected_session()
            && let Some(target) = session.card.target.clone()
        {
            let activity = session.activity;
            self.hits.push((self.new_chat_rect(dpi), Action::NewChat));
            self.hits.push((self.send_rect(dpi), Action::Send));
            self.hits.push((
                self.open_rect(dpi),
                Action::Open(target.clone()),
            ));
            self.hits.push((
                self.dismiss_rect(dpi),
                Action::Dismiss(target, activity),
            ));
        }
        self.scrollbar = None;
        if self.group.sessions.len() > count {
            let track = Rect {
                left: width - d(8),
                top: self.list.top + d(3),
                right: width - d(4),
                bottom: self.list.bottom - d(3),
            };
            let track_height = track.bottom - track.top;
            let thumb_height = (track_height * count as i32 / self.group.sessions.len() as i32)
                .max(d(16))
                .min(track_height);
            let top = track.top
                + (track_height - thumb_height) * self.offset as i32
                    / (self.group.sessions.len() - count) as i32;
            let thumb = Rect {
                top,
                bottom: top + thumb_height,
                ..track
            };
            self.scrollbar = Some((track, thumb));
            self.hits.push((
                Rect {
                    left: width - d(14),
                    ..thumb
                },
                Action::ScrollThumb,
            ));
            self.hits.push((
                Rect {
                    left: width - d(14),
                    bottom: thumb.top,
                    ..track
                },
                Action::Scroll(-(count as i32)),
            ));
            self.hits.push((
                Rect {
                    left: width - d(14),
                    top: thumb.bottom,
                    ..track
                },
                Action::Scroll(count as i32),
            ));
        }
        if let Some(chat) = &mut self.chat {
            chat.layout(Rect { left: d(18), top: self.preview_top + d(8), right: width - d(14), bottom: self.footer_top - d(28) }, dpi);
            if let Some((track, thumb)) = chat.scrollbar() {
                self.hits.push((Rect { left: width - d(28), ..thumb }, Action::HistoryThumb));
                self.hits.push((Rect { left: width - d(28), bottom: thumb.top, ..track }, Action::HistoryScroll(-(chat.bounds.bottom - chat.bounds.top))));
                self.hits.push((Rect { left: width - d(28), top: thumb.bottom, ..track }, Action::HistoryScroll(chat.bounds.bottom - chat.bounds.top)));
            }
        }
        self.dirty = false;
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<Action> {
        self.hits
            .iter()
            .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .map(|(_, a)| a.clone())
    }

    pub fn select(&mut self, id: String) {
        if self.selected.as_ref() != Some(&id) { self.progression.clear_typing(); }
        self.remembered = None;
        if self.selected.as_ref() != Some(&id) && self.chat.is_some() { self.chat = Some(chat_panel::Conversation::default()); }
        self.selected = Some(id);
        self.reveal_selection = true;
        self.dirty = true;
    }

    pub fn scroll(&mut self, rows: i32) {
        self.offset = (self.offset as i32 + rows).clamp(
            0,
            self.group.sessions.len().saturating_sub(self.visible_rows) as i32,
        ) as usize;
        self.reveal_selection = false;
        self.dirty = true;
    }

    pub fn wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        if self.chat.as_mut().is_some_and(|chat| chat.wheel(x, y, delta)) { self.dirty = true; return true; }
        if x < self.list.left || x >= self.list.right || y < self.list.top || y >= self.list.bottom
        {
            return false;
        }
        self.wheel_remainder += delta;
        let steps = self.wheel_remainder / 120;
        self.wheel_remainder %= 120;
        if steps != 0 {
            self.scroll(-steps * 3);
        }
        true
    }

    pub fn begin_drag(&mut self, y: i32) {
        self.drag_grab = self.scrollbar.map_or(0, |(_, thumb)| y - thumb.top);
    }

    pub fn drag_scroll(&mut self, y: i32) {
        if let Some((track, thumb)) = self.scrollbar {
            let travel = (track.bottom - track.top - (thumb.bottom - thumb.top)).max(1);
            let maximum = self.group.sessions.len().saturating_sub(self.visible_rows);
            self.offset = ((y - self.drag_grab - track.top).clamp(0, travel) as usize * maximum
                + travel as usize / 2)
                / travel as usize;
            self.reveal_selection = false;
            self.dirty = true;
        }
    }

    #[cfg(test)]
    pub fn test_point(&self, action: usize) -> Option<(i32, i32)> {
        self.hits
            .iter()
            .find(|(_, hit)| match (action, hit) {
                (4, Action::Select(id)) => id == "1",
                (5, Action::Open(_)) => true,
                (7, Action::Dismiss(..)) => true,
                (8, Action::Fold) => true,
                (9, Action::Send) => true,
                (15, Action::Select(id)) => id == "0",
                (19, Action::NewChat) => true,
                (23, Action::Select(id)) => id.ends_with("-source"),
                (24, Action::Select(id)) => !id.ends_with("-source") && self.selected.as_ref() != Some(id),
                _ => false,
            })
            .map(|(r, _)| ((r.left + r.right) / 2, (r.top + r.bottom) / 2))
    }
}

pub(super) fn action_at(state: &OverlayState, x: i32, y: i32) -> Option<Action> {
    if state.collapsed {
        return None;
    }
    let panel = state.layout?.panel?;
    if x < panel.left || x >= panel.right || y < panel.top || y >= panel.bottom {
        return None;
    }
    state.group.as_ref()?.hit(x - panel.left, y - panel.top)
}

fn identity_color(key: &str) -> u32 {
    let hash = key
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    [
        color_ref(107, 213, 200),
        color_ref(197, 167, 237),
        color_ref(144, 186, 235),
        color_ref(220, 185, 142),
    ][hash as usize % 4]
}
fn stripe_color(group: &ProjectGroup) -> u32 {
    if group.counts().1 > 0 {
        color_ref(241, 186, 117)
    } else {
        identity_color(&group.key)
    }
}
fn status_color(session: &GroupSession) -> u32 {
    if session.needs_input {
        color_ref(241, 186, 117)
    } else if session.busy {
        color_ref(141, 186, 255)
    } else if session.card.final_message {
        color_ref(134, 213, 169)
    } else {
        color_ref(174, 187, 200)
    }
}
pub(super) unsafe fn text(dc: Handle, value: &str, rect: Rect, color: u32, flags: u32) {
    unsafe {
        draw_text(dc, value, &mut rect.clone(), color, flags);
    }
}
pub(super) unsafe fn font(size: i32, weight: i32, dpi: u32) -> Handle {
    unsafe {
        CreateFontW(
            -scale_dip(size, dpi),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            wide("Segoe UI").as_ptr(),
        )
    }
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreatePen(style: i32, width: i32, color: u32) -> Handle;
    fn MoveToEx(dc: Handle, x: i32, y: i32, previous: *mut Point) -> Bool;
    fn LineTo(dc: Handle, x: i32, y: i32) -> Bool;
    fn Ellipse(dc: Handle, left: i32, top: i32, right: i32, bottom: i32) -> Bool;
}
unsafe fn glyph(dc: Handle, session: &GroupSession, x: i32, y: i32, dpi: u32) {
    unsafe {
        let d = |n| scale_dip(n, dpi);
        let color = status_color(session);
        let pen = CreatePen(0, d(1).max(1), color);
        let old_pen = SelectObject(dc, pen);
        let old_brush = SelectObject(dc, GetStockObject(5)); // NULL_BRUSH
        if session.needs_input {
            Ellipse(dc, x, y, x + d(10), y + d(10));
            MoveToEx(dc, x + d(5), y + d(2), null_mut());
            LineTo(dc, x + d(5), y + d(6));
            fill_rectangle(
                dc,
                &Rect {
                    left: x + d(5),
                    top: y + d(7),
                    right: x + d(6),
                    bottom: y + d(8),
                },
                color,
            );
        } else if session.busy {
            fill_rounded_rectangle(
                dc,
                &Rect {
                    left: x + d(1),
                    top: y + d(1),
                    right: x + d(9),
                    bottom: y + d(9),
                },
                color,
                d(8),
            );
        } else if session.card.final_message {
            MoveToEx(dc, x + d(1), y + d(5), null_mut());
            LineTo(dc, x + d(4), y + d(8));
            LineTo(dc, x + d(9), y + d(2));
        } else {
            Ellipse(dc, x + d(2), y + d(2), x + d(8), y + d(8));
        }
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        if !pen.is_null() {
            DeleteObject(pen);
        }
    }
}

pub(super) unsafe fn paint_panel(dc: Handle, state: &OverlayState, rect: Rect) {
    unsafe {
        let Some(ui) = &state.group else { return };
        let dpi = state.dpi.max(96);
        let d = |n| scale_dip(n, dpi);
        let foreground = color_ref(242, 247, 253);
        let muted = color_ref(191, 204, 220);
        let subtle = color_ref(164, 181, 201);
        let edge = color_ref(65, 83, 105);
        glass::surface(dc, rect);
        fill_rectangle(
            dc,
            &Rect {
                bottom: d(3),
                ..rect
            },
            stripe_color(&ui.group),
        );
        let regular = font(13, 400, dpi);
        let heading = font(13, 600, dpi);
        let small = font(11, 400, dpi);
        let action_font = font(12, 600, dpi);
        let old = SelectObject(dc, heading);
        let mut measured = Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let name = wide(&ui.group.name);
        DrawTextW(
            dc,
            name.as_ptr(),
            wide_text_length(&name),
            &mut measured,
            DT_SINGLELINE | DT_CALCRECT | DT_NOPREFIX,
        );
        let working = ui.group.sessions.iter().any(|session| session.busy && !session.needs_input);
        let title_right = (d(18) + measured.right).min(rect.right - d(if working { 183 } else { 146 }));
        text(
            dc,
            &ui.group.name,
            Rect {
                left: d(18),
                top: d(6),
                right: title_right,
                bottom: d(28),
            },
            foreground,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        SelectObject(dc, small);
        text(
            dc,
            &format!(
                "{} session{}",
                ui.group.sessions.len(),
                if ui.group.sessions.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            Rect {
                left: title_right + d(8),
                top: d(6),
                right: rect.right - d(if working { 106 } else { 68 }),
                bottom: d(28),
            },
            subtle,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        text(
            dc,
            "\u{203a}",
            Rect {
                left: rect.right - d(30),
                top: d(4),
                right: rect.right - d(10),
                bottom: d(26),
            },
            muted,
            DT_SINGLELINE | DT_VCENTER | 1,
        );
        if ui.selected_session().is_some_and(|session| session.card.target.is_some()) {
            let button = ui.new_chat_rect(dpi);
            let pen = CreatePen(0, d(1).max(1), muted);
            let previous = SelectObject(dc, pen);
            let x = (button.left + button.right) / 2;
            let y = (button.top + button.bottom) / 2;
            MoveToEx(dc, x - d(7), y - d(6), null_mut()); LineTo(dc, x + d(7), y - d(6));
            LineTo(dc, x + d(7), y + d(5)); LineTo(dc, x - d(2), y + d(5));
            LineTo(dc, x - d(6), y + d(8)); LineTo(dc, x - d(6), y + d(5));
            LineTo(dc, x - d(7), y + d(5)); LineTo(dc, x - d(7), y - d(6));
            MoveToEx(dc, x - d(3), y - d(1), null_mut()); LineTo(dc, x + d(4), y - d(1));
            MoveToEx(dc, x, y - d(4), null_mut()); LineTo(dc, x, y + d(3));
            SelectObject(dc, previous); DeleteObject(pen);
        }
        if ui.group.disambiguate {
            text(
                dc,
                ui.group.path.as_deref().unwrap_or_default(),
                Rect {
                    left: d(18),
                    top: d(28),
                    right: rect.right - d(18),
                    bottom: d(48),
                },
                muted,
                DT_SINGLELINE | DT_VCENTER | 0x4000,
            ); // DT_PATH_ELLIPSIS
        }
        for (index, session) in ui
            .group
            .sessions
            .iter()
            .skip(ui.offset)
            .take(ui.visible_rows)
            .enumerate()
        {
            let y = ui.list.top + d(ROW) * index as i32;
            let selected = ui.selected.as_ref() == Some(&session.id);
            if selected {
                glass::plate(
                    dc,
                    Rect {
                        left: d(7),
                        top: y + d(1),
                        right: rect.right - d(7),
                        bottom: y + d(ROW) - d(1),
                    },
                    dpi,
                );
                fill_rectangle(
                    dc,
                    &Rect {
                        left: d(7),
                        top: y + d(7),
                        right: d(9),
                        bottom: y + d(ROW) - d(7),
                    },
                    identity_color(&ui.group.key),
                );
            }
            glyph(dc, session, d(18), y + d(9), dpi);
            SelectObject(dc, small);
            let mut status_size = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            let status = wide(session.status());
            DrawTextW(
                dc,
                status.as_ptr(),
                wide_text_length(&status),
                &mut status_size,
                DT_SINGLELINE | DT_CALCRECT | DT_NOPREFIX,
            );
            let status_left = rect.right - d(18) - status_size.right;
            text(
                dc,
                session.status(),
                Rect {
                    left: status_left,
                    top: y,
                    right: rect.right - d(18),
                    bottom: y + d(ROW),
                },
                status_color(session),
                DT_SINGLELINE | DT_VCENTER | 2,
            );
            SelectObject(dc, if selected { heading } else { regular });
            text(
                dc,
                session.title(),
                Rect {
                    left: d(36),
                    top: y,
                    right: status_left - d(10),
                    bottom: y + d(ROW),
                },
                if session.busy || session.needs_input || selected {
                    foreground
                } else {
                    muted
                },
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
        }
        if let Some((track, thumb)) = ui.scrollbar {
            fill_rounded_rectangle(dc, &track, edge, d(4));
            fill_rounded_rectangle(dc, &thumb, subtle, d(4));
        }
        fill_rectangle(
            dc,
            &Rect {
                left: d(12),
                top: ui.preview_top - 1,
                right: rect.right - d(12),
                bottom: ui.preview_top,
            },
            edge,
        );
        if let Some(session) = ui.selected_session() {
            let y = ui.preview_top;
            SelectObject(dc, regular);
            let notice = state.composer.as_ref().and_then(|composer| composer.status(&session.id));
            let preview = notice.map(str::to_owned).unwrap_or_else(|| session.card.text.split_whitespace().collect::<Vec<_>>().join(" "));
            if let Some(chat) = &ui.chat {
                chat.paint(dc);
                SelectObject(dc, small);
                let status = state.composer.as_ref().map_or("", |composer| composer.history_status());
                text(dc, status, Rect { left: d(18), top: ui.footer_top - d(22), right: rect.right - d(18), bottom: ui.footer_top - d(4) },
                    subtle, DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER | 1);
            } else {
                text(dc, &preview, Rect { left: d(18), top: y + d(6), right: rect.right - d(18), bottom: y + d(42) },
                    muted, DT_WORDBREAK | DT_END_ELLIPSIS | DT_EDITCONTROL);
            }
            SelectObject(dc, action_font);
            glass::plate(dc, ui.open_rect(dpi), dpi);
            text(dc, "Open chat", ui.open_rect(dpi),
                foreground, DT_SINGLELINE | DT_VCENTER | 1);
            SelectObject(dc, small);
            text(dc, "Dismiss", ui.dismiss_rect(dpi),
                subtle, DT_SINGLELINE | DT_VCENTER | 1);
            glass::plate(dc, ui.input_rect(dpi), dpi);
            text(dc, "Message\u{2026}", Rect { left: d(18), ..ui.input_rect(dpi) }, subtle, DT_SINGLELINE | DT_VCENTER);
            let button = ui.send_rect(dpi);
            glass::plate(dc, button, dpi);
            let sending = state.composer.as_ref().is_some_and(|composer| composer.sending());
            let color = if sending || session.card.target.is_none() { subtle } else { foreground };
            let pen = CreatePen(0, d(2).max(1), color);
            let old_pen = SelectObject(dc, pen);
            let cx = (button.left + button.right) / 2;
            let cy = (button.top + button.bottom) / 2;
            MoveToEx(dc, cx, cy + d(6), null_mut()); LineTo(dc, cx, cy - d(6));
            MoveToEx(dc, cx - d(5), cy - d(1), null_mut()); LineTo(dc, cx, cy - d(6)); LineTo(dc, cx + d(5), cy - d(1));
            SelectObject(dc, old_pen); DeleteObject(pen);
        }
        glass::rim(dc, rect, d(9));
        SelectObject(dc, old);
        for handle in [regular, heading, small, action_font] {
            if !handle.is_null() {
                DeleteObject(handle);
            }
        }
    }
}

// Painted over the cached panel: activity ticks never lay out or repaint chat text.
pub(super) unsafe fn paint_working(dc: Handle, ui: &GroupUi, dpi: u32, elapsed: Duration, animate: bool) {
    if !ui.group.sessions.iter().any(|session| session.busy && !session.needs_input) { return; }
    unsafe {
        let d = |n| scale_dip(n, dpi);
        let radius = d(3).max(2);
        for dot in 0..3 {
            let x = ui.width - d(92 - dot as i32 * 9);
            let y = d(17);
            let color = blend_color(color_ref(52, 74, 99), color_ref(163, 209, 255), busy_strength(elapsed, dot, animate));
            fill_rounded_rectangle(dc, &Rect { left: x-radius, top: y-radius, right: x+radius, bottom: y+radius }, color, radius*2);
        }
    }
}

unsafe fn bead(dc: Handle, session: &GroupSession, x: i32, y: i32, dpi: u32, elapsed: Duration, animate: bool) {
    unsafe {
        let d = |n| scale_dip(n, dpi);
        let unread = session.card.final_message && session.card.attention;
        let color = if session.needs_input { color_ref(241, 186, 117) }
            else if session.busy { color_ref(141, 186, 255) }
            else if unread { color_ref(134, 213, 169) } else { color_ref(174, 187, 200) };
        let color = if session.busy && !session.needs_input && animate {
            let pulse = 0.72 + 0.28 * (elapsed.as_secs_f32() * std::f32::consts::TAU / 2.4).cos();
            blend_color(color_ref(30, 40, 54), color, pulse)
        } else { color };
        let pen = CreatePen(0, d(1).max(1), color);
        let old_pen = SelectObject(dc, pen);
        let brush = if session.busy || session.needs_input || unread { CreateSolidBrush(color) } else { null_mut() };
        let old_brush = SelectObject(dc, if brush.is_null() { GetStockObject(5) } else { brush });
        Ellipse(dc, x-d(2), y-d(2), x+d(3), y+d(3));
        if session.needs_input || (!session.busy && unread) {
            SelectObject(dc, GetStockObject(5));
            Ellipse(dc, x-d(4), y-d(4), x+d(4), y+d(4));
        }
        SelectObject(dc, old_brush); SelectObject(dc, old_pen);
        if !brush.is_null() { DeleteObject(brush); }
        if !pen.is_null() { DeleteObject(pen); }
    }
}

unsafe fn shortcut_badge(dc: Handle, rect: Rect, code: [u8;2], dpi: u32) {
    unsafe {
        fill_rounded_rectangle(dc, &rect, color_ref(202,185,139), scale_dip(3,dpi));
        text(dc, &format!("{} {}", code[0] as char,code[1] as char), rect,
            color_ref(27,33,40), DT_SINGLELINE | DT_VCENTER | 1);
    }
}

pub(super) unsafe fn paint_tab(dc: Handle, tab: Rect, state: &OverlayState, dpi: u32) {
    unsafe {
        let Some(ui) = &state.group else { return };
        let d = |n| scale_dip(n, dpi);
        let saved = SaveDC(dc);
        IntersectClipRect(dc, tab.left, tab.top, tab.right, tab.bottom);
        let left = tab.right - d(ui.tab.width);
        let base = Rect { left, ..tab };
        glass::surface(dc, base);
        fill_rectangle(dc, &Rect { right: left+d(3), ..base }, stripe_color(&ui.group));
        let small = font(10,400,dpi);
        let regular = font(12,400,dpi);
        let mono = font(10,700,dpi);
        let old = SelectObject(dc, small);
        let notice = matches!(ui.tab.kind, tab_state::Kind::Finished | tab_state::Kind::Question);
        if notice {
            let hint = state.shortcut_hints.then_some(state.shortcut_code).flatten();
            let right = tab.right - d(if hint.is_some() { 34 } else { 8 });
            text(dc, &ui.group.name, Rect { left:left+d(10), top:tab.top+d(3),right:right-d(48),bottom:tab.top+d(18) },
                color_ref(191,204,220), DT_SINGLELINE | DT_END_ELLIPSIS);
            if let Some(session) = ui.tab.notice_session(&ui.group) {
                text(dc, if session.needs_input { "Needs you" } else { "Done" },
                    Rect {left:right-d(50),top:tab.top+d(3),right,bottom:tab.top+d(18)},
                    status_color(session),DT_SINGLELINE | 2);
                SelectObject(dc,regular);
                glyph(dc,session,left+d(10),tab.top+d(25),dpi);
                let question;
                let label = if session.needs_input {
                    question = session.card.text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if question.is_empty() { session.title() } else { &question }
                } else { session.title() };
                text(dc,label,Rect {left:left+d(25),top:tab.top+d(20),right:tab.right-d(8),bottom:tab.top+d(38)},
                    if session.needs_input { status_color(session) } else { color_ref(242,247,253) },
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            }
            if let Some(code) = hint {
                SelectObject(dc,small);
                shortcut_badge(dc,Rect {left:tab.right-d(31),top:tab.top+d(3),right:tab.right-d(5),bottom:tab.top+d(17)},code,dpi);
            }
        } else {
            SelectObject(dc,mono);
            let folder = ui.group.path.as_deref().and_then(|path| path.trim_end_matches(['\\','/']).rsplit(['\\','/']).next())
                .unwrap_or(&ui.group.name);
            let monogram = folder.chars().next().unwrap_or('?').to_uppercase().to_string();
            text(dc,&monogram,Rect {left:left+d(3),top:tab.top+d(2),right:left+d(18),bottom:tab.top+d(15)},
                identity_color(&ui.group.key),DT_SINGLELINE | DT_VCENTER | 1);
            for (i,session) in ui.group.sessions.iter().take(3).enumerate() {
                bead(dc,session,left+d(10),tab.top+d(19+i as i32*7),dpi,state.activity_started.elapsed(),state.animate);
            }
            if ui.group.sessions.len() >= 4 {
                fill_rounded_rectangle(dc,&Rect {left:left+d(8),right:left+d(13),top:tab.top+d(38),bottom:tab.top+d(40)},
                    color_ref(111,129,150),d(1));
            }
            if state.shortcut_hints && let Some(code) = state.shortcut_code {
                SelectObject(dc,small);
                shortcut_badge(dc,Rect {left:left+d(19),top:tab.top+d(14),right:left+d(43),bottom:tab.top+d(29)},code,dpi);
            }
        }
        SelectObject(dc,old);
        for handle in [small,regular,mono] { if !handle.is_null() { DeleteObject(handle); } }
        glass::rim(dc,base,d(6));
        RestoreDC(dc,saved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn group() -> ProjectGroup {
        ProjectGroup {
            key: "project:a".into(),
            name: "a".into(),
            path: Some("C:\\a".into()),
            disambiguate: false,
            sessions: (0..10)
                .map(|i| GroupSession {
                    id: i.to_string(),
                    activity: i,
                    card: Card {
                        id: i,
                        label: format!("a — Task {i}"),
                        text: "A long progress update".into(),
                        final_message: false,
                        attention: false,
                        target: Some(CardTarget {
                            window: i + 1,
                            session_id: i.to_string(),
                            project: None,
                        }),
                    },
                    busy: true,
                    needs_input: false,
                    hidden_in_focus: false,
                    window: Some(i + 1),
                    dock_request: 1,
                })
                .collect(),
        }
    }
    #[test]
    fn stable_selection_and_order_survive_updates() {
        let mut ui = GroupUi::new(group());
        ui.selected = Some("3".into());
        let mut next = group();
        next.sessions.reverse();
        next.sessions[0].card.text = "new".into();
        ui.sync(next);
        assert_eq!(ui.selected.as_deref(), Some("3"));
        assert_eq!(ui.group.sessions[0].id, "0");
    }

    #[test]
    fn message_preview_fits_the_reply_and_caps_long_replies_at_the_work_area() {
        use crate::chat_history::{Message, Role};
        for dpi in [96, 120, 144, 192] {
            for position in ["top-right", "bottom-right"] {
                let d = |n| scale_dip(n, dpi);
                let work = Rect { left: -d(1280), top: -d(40), right: 0, bottom: d(760) };
                let mut ui = GroupUi::new(group());
                ui.chat = Some(chat_panel::Conversation::default());
                ui.message_anchor = Some(Rect { left: -d(PANEL_WIDTH), top: d(200), right: 0, bottom: d(410) });
                ui.chat.as_mut().unwrap().update_latest(Message::new(Role::Assistant,
                    "A reply that needs space.\n\n1. Read the full message.\n2. Keep the controls visible.\n\nThe last paragraph also fits."));
                let fitted = ui.message_bounds(work, dpi, position);
                let chat = ui.chat.as_ref().unwrap();
                assert!(chat.content_height() <= chat.bounds.bottom - chat.bounds.top);
                assert!(chat.scrollbar().is_none());
                assert_eq!(fitted.right, work.right);
                assert_eq!(fitted.right - fitted.left, d(MESSAGE_WIDTH));
                assert!(fitted.top >= work.top && fitted.bottom <= work.bottom);
                assert_eq!(fitted, ui.message_bounds(work, dpi, position), "fitting must have a stable anchor");
                ui.chat.as_mut().unwrap().update_latest(Message::new(Role::Assistant, "A long reply paragraph.\n\n".repeat(100)));
                let capped = ui.message_bounds(work, dpi, position);
                assert_eq!(capped.bottom - capped.top, work.bottom - work.top - d(40));
                assert!(ui.chat.as_ref().unwrap().scrollbar().is_some());
                assert!(ui.input_rect(dpi).bottom <= ui.height);
            }
        }
    }

    #[test]
    fn new_chat_icon_is_reachable_in_both_sizes_without_overlapping_other_actions() {
        for dpi in [96, 120, 144, 192] {
            for width in [PANEL_WIDTH, MESSAGE_WIDTH, 780] {
                let mut ui = GroupUi::new(group());
                if width != PANEL_WIDTH { ui.chat = Some(chat_panel::Conversation::default()); }
                ui.layout(scale_dip(width, dpi), scale_dip(680, dpi), dpi);
                let point = ui.test_point(19).unwrap();
                assert_eq!(ui.hit(point.0, point.1), Some(Action::NewChat));
                let icon = ui.new_chat_rect(dpi);
                for (rect, action) in &ui.hits {
                    if *action == Action::NewChat { continue; }
                    assert!(rect.right <= icon.left || rect.left >= icon.right || rect.bottom <= icon.top || rect.top >= icon.bottom);
                }
            }
        }
    }

    #[test]
    fn reply_input_send_and_chat_actions_do_not_overlap_at_each_dpi() {
        for dpi in [96, 120, 144, 192] {
            let mut ui = GroupUi::new(group());
            ui.layout(scale_dip(PANEL_WIDTH, dpi), scale_dip(470, dpi), dpi);
            let input = ui.input_rect(dpi);
            let (x, y) = ui.test_point(9).unwrap();
            assert_eq!(ui.hit(x, y), Some(Action::Send));
            for (rect, _) in &ui.hits {
                assert!(rect.right <= input.left || rect.left >= input.right
                    || rect.bottom <= input.top || rect.top >= input.bottom);
            }
            let open = ui.test_point(5).unwrap();
            let dismiss = ui.test_point(7).unwrap();
            assert!(input.right < x && x < open.0 && open.0 < dismiss.0);
        }
    }

    #[test]
    fn initially_focused_project_selects_a_task_once_when_it_becomes_visible() {
        let mut hidden = group();
        hidden.sessions.clear();
        let mut ui = GroupUi::new(hidden);
        ui.sync(group());
        assert_eq!(ui.selected.as_deref(), Some("0"));
        ui.select("4".into());
        ui.sync(group());
        assert_eq!(ui.selected.as_deref(), Some("4"));
    }
    #[test]
    fn focused_sessions_return_to_the_same_position_and_selection() {
        let mut ui = GroupUi::new(group());
        ui.selected = Some("3".into());
        let mut hidden = group();
        hidden.sessions.retain(|s| s.id != "3");
        ui.sync(hidden);
        ui.sync(group());
        assert_eq!(ui.selected.as_deref(), Some("3"));
        assert_eq!(ui.group.sessions[3].id, "3");
        let mut empty = group();
        empty.sessions.clear();
        ui.sync(empty);
        ui.sync(group());
        assert_eq!(ui.selected.as_deref(), Some("3"));
    }
    #[test]
    fn selecting_rows_never_moves_the_list_and_scrolling_preserves_the_preview() {
        for dpi in [96, 120, 144, 192] {
            let mut ui = GroupUi::new(group());
            let height = scale_dip(470, dpi);
            ui.layout(scale_dip(PANEL_WIDTH, dpi), height, dpi);
            assert!(ui.scrollbar.is_some());
            assert!(ui.height <= height);
            for (rect, _) in &ui.hits {
                assert!(rect.bottom <= ui.height);
                assert!(rect.right <= scale_dip(PANEL_WIDTH, dpi));
            }
            let before = ui
                .hits
                .iter()
                .filter(|(_, action)| matches!(action, Action::Select(_)))
                .cloned()
                .collect::<Vec<_>>();
            let preview_top = ui.preview_top;
            ui.select("3".into());
            ui.layout(scale_dip(PANEL_WIDTH, dpi), height, dpi);
            let after = ui
                .hits
                .iter()
                .filter(|(_, action)| matches!(action, Action::Select(_)))
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(before, after);
            assert_eq!(preview_top, ui.preview_top);
            assert!(ui.wheel(scale_dip(30, dpi), ui.list.top + 10, -120));
            ui.layout(scale_dip(PANEL_WIDTH, dpi), height, dpi);
            assert_eq!(ui.offset, 3);
            assert_eq!(ui.selected.as_deref(), Some("3"));
            ui.sync(group());
            ui.layout(scale_dip(PANEL_WIDTH, dpi), height, dpi);
            assert_eq!(ui.offset, 3, "feed refresh must not undo a scroll");
            ui.scroll(50);
            ui.layout(scale_dip(PANEL_WIDTH, dpi), height, dpi);
            assert_eq!(ui.offset + ui.visible_rows, 10);
            assert_eq!(preview_top, ui.preview_top);
            ui.begin_drag(ui.scrollbar.unwrap().1.top);
            ui.drag_scroll(ui.list.top);
            ui.layout(scale_dip(PANEL_WIDTH, dpi), height, dpi);
            assert_eq!(ui.offset, 0);
            assert_eq!(preview_top, ui.preview_top);
        }
    }
    #[test]
    fn open_and_dismiss_use_the_selected_session_target_and_turn() {
        let mut ui = GroupUi::new(group());
        ui.selected = Some("1".into());
        ui.layout(PANEL_WIDTH, 470, 96);
        assert!(
            ui.hits
                .iter()
                .any(|(_, action)| matches!(action,Action::Open(target) if target.session_id=="1"))
        );
        assert!(ui.hits.iter().any(
            |(_, action)| matches!(action,Action::Dismiss(target,1) if target.session_id=="1")
        ));
    }

    #[test]
    fn tabs_and_drawer_prioritize_attention_and_keep_the_selected_task() {
        let mut data = group();
        data.sessions.truncate(5);
        data.sessions[1].busy = false;
        data.sessions[2].needs_input = true;
        data.sessions[3].busy = false;
        data.sessions[3].card.final_message = true;
        data.sessions[3].card.attention = true;
        data.sessions[4].busy = false;
        data.sessions[4].card.final_message = true;
        let mut ui = GroupUi::new(data.clone());
        assert_eq!(
            ui.group
                .sessions
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["2", "0", "3", "1", "4"]
        );
        assert_eq!(ui.selected.as_deref(), Some("2"));
        assert_eq!(stripe_color(&ui.group), color_ref(241, 186, 117));
        ui.select("0".into());
        data.sessions[2].needs_input = false;
        ui.sync(data);
        assert_eq!(ui.selected.as_deref(), Some("0"));
        assert_eq!(stripe_color(&ui.group), identity_color(&ui.group.key));
        for (count, height) in [(5, 42), (3, 42), (2, 42), (1, 42)] {
            ui.group.sessions.truncate(count);
            assert_eq!(ui.tab_height(), height);
        }
    }

    #[test]
    fn native_working_dots_animate_in_both_sizes_and_respect_reduced_motion() {
        #[link(name = "gdi32")]
        unsafe extern "system" { fn GetPixel(dc: Handle, x: i32, y: i32) -> u32; }
        unsafe {
            let reference = GetDC(null_mut());
            for dpi in [96, 144] {
                for width in [PANEL_WIDTH, 780] {
                    let d = |n| scale_dip(n, dpi);
                    let mut ui = GroupUi::new(group());
                    ui.layout(d(width), d(680), dpi);
                    let mut buffer = PaintBuffer::default();
                    let dc = buffer.get(reference, d(width), d(680));
                    let bounds = Rect { left: 0, top: 0, right: d(width), bottom: d(680) };
                    let background = color_ref(30, 40, 50);
                    fill_rectangle(dc, &bounds, background);
                    let dot = || GetPixel(dc, d(width-92), d(17));
                    paint_working(dc, &ui, dpi, Duration::ZERO, true);
                    let first = dot();
                    paint_working(dc, &ui, dpi, Duration::from_millis(400), true);
                    assert_ne!(dot(), first);
                    assert_eq!(GetPixel(dc, d(20), d(100)), background, "dots never repaint conversation pixels");
                    paint_working(dc, &ui, dpi, Duration::ZERO, false);
                    let steady = dot();
                    paint_working(dc, &ui, dpi, Duration::from_millis(400), false);
                    assert_eq!(dot(), steady);
                    for session in &mut ui.group.sessions { session.busy = false; }
                    fill_rectangle(dc, &bounds, background);
                    paint_working(dc, &ui, dpi, Duration::ZERO, true);
                    assert_eq!(dot(), background, "completed work stops the indicator");
                }
            }
            ReleaseDC(null_mut(), reference);
        }
    }

    #[test]
    fn native_grouped_render_and_hit_targets() {
        unsafe {
            let reference = GetDC(null_mut());
            assert!(!reference.is_null());
            for (dpi, session_count, shortcut_hints, mode) in [
                (96, 3, false, 0),
                (144, 3, false, 0),
                (192, 3, false, 0),
                (96, 1, false, 1),
                (96, 5, true, 0),
                (192, 10, false, 0),
                (96, 4, false, 1),
                (96, 4, true, 1),
                (96, 3, false, 2),
                (192, 4, true, 1),
            ] {
                let mut sample = group();
                sample.sessions.truncate(session_count);
                sample.name = "CodexLidGuard".into();
                sample.path = Some(r"C:\Projects\CodexLidGuard".into());
                sample.sessions[0].card.label = "CodexLidGuard — Fix overlay flicker".into();
                sample.sessions[0].card.text="Checking redraw timing after minimizing VS Code. The selected session stays in place as updates arrive.".into();
                if let Some(session) = sample.sessions.get_mut(1) {
                    session.card.label = "CodexLidGuard — Simplify tab labels".into();
                    session.card.text = "Simplify tab labels?".into();
                    session.needs_input = true;
                }
                if let Some(session) = sample.sessions.get_mut(2) {
                    session.card.label = "CodexLidGuard — Retry pipe integration test".into();
                    session.busy = false;
                    session.card.final_message = true;
                    session.card.attention = true;
                }
                if mode != 0 {
                    for session in &mut sample.sessions { session.needs_input = false; session.busy = true; session.card.attention = false; }
                    if mode == 2 { sample.sessions[0].busy = false; sample.sessions[0].card.final_message = true; sample.sessions[0].card.attention = true; }
                    else if sample.sessions.len() > 2 { sample.sessions[2].busy = false; sample.sessions[2].card.final_message = false; }
                }
                let mut ui = GroupUi::new(sample);
                ui.tab.tick(&ui.group, shortcut_hints, false, Instant::now());
                ui.select("0".into());
                ui.layout(scale_dip(PANEL_WIDTH, dpi), scale_dip(480, dpi), dpi);
                let state = OverlayState {
                    composer: None,
                    group: Some(ui),
                    group_action: None,
                    cards: vec![],
                    heights: vec![],
                    rows: vec![],
                    font: font(14, 400, dpi),
                    dpi,
                    clicks: ClickTracker::default(),
                    pending_target: None,
                    collapsed: false,
                    hover_open: None,
                    keyboard_preview: None,
                    tab_pressed: false,
                    close_pressed: false,
                    activity: 0,
                    layout: None,
                    busy: true,
                    attention: false,
                    animate: false,
                    activity_started: Instant::now(),
                    buffer: PaintBuffer::default(),
                    panel_buffer: PaintBuffer::default(),
                    material: glass::PaintCache::default(),
                    panel_dirty: true,
                    panel_size: (1, 1),
                    shortcut_code: Some(*b"CL"),
                    shortcut_prefix: Some("Copilot".into()),
                    shortcut_token: 0,
                    shortcut_hints,
                    restoring: false,
                    compositor: None,
                    render_alpha: 255,
                };
                let width = scale_dip(PANEL_WIDTH + 20 + TAB_WIDTH, dpi);
                let height = state.group.as_ref().unwrap().height;
                let mut buffer = PaintBuffer::default();
                let dc = buffer.get(reference, width, height);
                fill_rectangle(
                    dc,
                    &Rect {
                        left: 0,
                        top: 0,
                        right: width,
                        bottom: height,
                    },
                    color_ref(16, 20, 25),
                );
                SetBkMode(dc, TRANSPARENT);
                SelectObject(dc, state.font);
                // Opt-in cold-paint benchmark; normal frames reuse the drawer cache.
                let iterations = std::env::var("CODEX_OVERLAY_BENCH_ITERATIONS")
                    .ok()
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(1)
                    .clamp(1, 10_000);
                #[link(name = "gdi32")]
                unsafe extern "system" {
                    fn GdiFlush() -> Bool;
                }
                GdiFlush();
                let started = Instant::now();
                for _ in 0..iterations {
                    paint_panel(
                        dc,
                        &state,
                        Rect {
                            left: 0,
                            top: 0,
                            right: scale_dip(PANEL_WIDTH, dpi),
                            bottom: height,
                        },
                    );
                    paint_tab(
                        dc,
                        Rect {
                            left: scale_dip(PANEL_WIDTH + 20, dpi),
                            top: 0,
                            right: width,
                            bottom: scale_dip(state.group.as_ref().unwrap().tab_height(), dpi),
                        },
                        &state,
                        dpi,
                    );
                    GdiFlush();
                }
                if iterations > 1 {
                    println!("overlay paint: dpi={dpi} sessions={session_count} iterations={iterations} us_per_pair={:.2}",
                        started.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations));
                }
                let ui = state.group.as_ref().unwrap();
                assert!(
                    matches!(ui.hit(scale_dip(22,dpi),scale_dip(42 + 28 * ui.group.sessions.iter().position(|s| s.id == "0").unwrap() as i32,dpi)),Some(Action::Select(id)) if id=="0")
                );
                if let Some(directory) = std::env::var_os("CODEX_OVERLAY_RENDER_DIR") {
                    #[link(name = "gdi32")]
                    unsafe extern "system" {
                        fn GetPixel(dc: Handle, x: i32, y: i32) -> u32;
                    }
                    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
                    for y in 0..height {
                        for x in 0..width {
                            let pixel = GetPixel(dc, x, y);
                            bytes.extend_from_slice(&[
                                pixel as u8,
                                (pixel >> 8) as u8,
                                (pixel >> 16) as u8,
                            ]);
                        }
                    }
                    std::fs::create_dir_all(&directory).unwrap();
                    std::fs::write(
                        std::path::Path::new(&directory).join(if session_count == 3 && !shortcut_hints && mode == 0 {
                            format!("project-overlay-{dpi}.ppm")
                        } else { format!("project-overlay-{dpi}-{session_count}-hints{shortcut_hints}-mode{mode}.ppm") }),
                        bytes,
                    )
                    .unwrap();
                }
                SelectObject(dc, GetStockObject(13)); // SYSTEM_FONT before destroying the owned font.
                DeleteObject(state.font);
            }
            ReleaseDC(null_mut(), reference);
        }
    }
}
