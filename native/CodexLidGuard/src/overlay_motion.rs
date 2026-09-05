//! Time-based animation independent of message polling and native painting.
use crate::overlay::Card;
use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(180);

pub(super) struct OpenMotion {
    requested: Instant,
    started: Option<Instant>,
    animate: bool,
}

impl OpenMotion {
    pub(super) fn new(started: Instant, animate: bool) -> Self {
        Self {
            requested: started,
            started: None,
            animate,
        }
    }
    // The activation worker signals immediately before ShowWindow. Thread startup
    // and foreground-input attachment must not consume the visible transition.
    pub(super) fn begin(&mut self, at: Instant) {
        if self.started.is_none() {
            self.started = Some(at.max(self.requested));
        }
    }
    pub(super) fn finished(&self, now: Instant) -> bool {
        !self.animate || (self.started.is_some() && !self.sample(now).2)
    }
    pub(super) fn sample(&self, now: Instant) -> (f32, f32, bool) {
        let t = if self.animate {
            let Some(started) = self.started else {
                return (0.0, 1.0, false);
            };
            (now.saturating_duration_since(started).as_secs_f32() / 0.180).clamp(0.0, 1.0)
        } else {
            1.0
        };
        (1.0 - (1.0 - t).powi(3), (1.0 - t).powi(2), t < 1.0)
    }
}

struct Tween {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
}

impl Tween {
    fn new(value: f32, now: Instant) -> Self {
        Self {
            from: value,
            to: value,
            started: now,
            duration: DURATION,
        }
    }

    fn value(&self, now: Instant) -> f32 {
        let progress = (now.saturating_duration_since(self.started).as_secs_f32()
            / self.duration.as_secs_f32())
        .clamp(0.0, 1.0);
        self.from + (self.to - self.from) * (1.0 - (1.0 - progress).powi(3))
    }

    fn target(&mut self, value: f32, now: Instant, animate: bool) {
        if !animate {
            self.from = value;
            self.to = value;
        } else if self.to != value {
            self.from = self.value(now);
            self.to = value;
            self.started = now;
        }
    }

    fn active(&self, now: Instant) -> bool {
        self.from != self.to && now.saturating_duration_since(self.started) < self.duration
    }
}

pub(super) struct DockMotion(Tween);

impl DockMotion {
    pub(super) fn new(now: Instant) -> Self {
        let mut tween = Tween::new(0.0, now);
        tween.duration = Duration::from_millis(240);
        Self(tween)
    }

    pub(super) fn target(&mut self, collapsed: bool, now: Instant, animate: bool) {
        self.0
            .target(if collapsed { 1.0 } else { 0.0 }, now, animate);
    }

    pub(super) fn sample(&self, now: Instant) -> (f32, bool) {
        (self.0.value(now), self.0.active(now))
    }
}

pub(super) struct MotionClock {
    now: Instant,
    previous: Instant,
}

impl MotionClock {
    pub(super) fn new(now: Instant) -> Self {
        Self { now, previous: now }
    }

    pub(super) fn advance(&mut self, real: Instant, frozen: bool) -> Instant {
        if !frozen {
            self.now += real.saturating_duration_since(self.previous);
        }
        self.previous = real;
        self.now
    }
}

struct Row {
    card: Card,
    height: Tween,
    opacity: Tween,
    full_height: i32,
    retiring: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AnimatedRow {
    pub card: Card,
    pub height: i32,
    pub full_height: i32,
    pub opacity: f32,
    pub interactive: bool,
}

pub(super) struct Motion {
    panel: Tween,
    rows: Vec<Row>,
}

impl Motion {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            panel: Tween::new(0.0, now),
            rows: Vec::new(),
        }
    }

    pub(super) fn sync(&mut self, cards: &[Card], heights: &[i32], now: Instant, animate: bool) {
        self.panel
            .target(if cards.is_empty() { 0.0 } else { 1.0 }, now, animate);
        // Keep the final card intact while the entire panel fades away.
        if cards.is_empty() {
            if !animate {
                self.rows.clear();
            }
            return;
        }
        let first = self.rows.is_empty();
        for row in &mut self.rows {
            row.retiring = !cards.iter().any(|card| card.id == row.card.id);
            if row.retiring {
                row.height.target(0.0, now, animate);
                row.opacity.target(0.0, now, animate);
            }
        }
        for (card, height) in cards.iter().zip(heights) {
            if let Some(row) = self.rows.iter_mut().find(|row| row.card.id == card.id) {
                row.card = card.clone();
                row.full_height = *height;
                row.height.target(*height as f32, now, animate);
                row.opacity.target(1.0, now, animate);
            } else {
                let mut row = Row {
                    card: card.clone(),
                    height: Tween::new(if first { *height as f32 } else { 0.0 }, now),
                    opacity: Tween::new(if first { 1.0 } else { 0.0 }, now),
                    full_height: *height,
                    retiring: false,
                };
                row.height.target(*height as f32, now, animate);
                row.opacity.target(1.0, now, animate);
                self.rows.push(row);
            }
        }
        // Feed IDs are monotonic, so departing cards keep their original slot.
        self.rows.sort_by_key(|row| row.card.id);
    }

    pub(super) fn sample(&mut self, now: Instant) -> (f32, Vec<AnimatedRow>, bool) {
        let opacity = self.panel.value(now);
        if self.panel.to == 0.0 && !self.panel.active(now) {
            self.rows.clear();
        }
        self.rows
            .retain(|row| !row.retiring || row.height.active(now));
        let active = self.panel.active(now)
            || self
                .rows
                .iter()
                .any(|row| row.height.active(now) || row.opacity.active(now));
        let rows = self
            .rows
            .iter()
            .map(|row| AnimatedRow {
                card: row.card.clone(),
                height: row.height.value(now).round() as i32,
                full_height: row.full_height,
                opacity: row.opacity.value(now),
                interactive: !row.retiring && self.panel.to != 0.0,
            })
            .collect();
        (opacity, rows, active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_expands_and_fades_on_one_clock_and_reduced_motion_finishes_immediately() {
        let now = Instant::now();
        let mut motion = OpenMotion::new(now, true);
        assert_eq!(
            motion.sample(now + Duration::from_secs(1)),
            (0.0, 1.0, false)
        );
        assert!(!motion.finished(now + Duration::from_secs(1)));
        motion.begin(now);
        assert_eq!(motion.sample(now), (0.0, 1.0, true));
        motion.begin(now + Duration::from_millis(30)); // Duplicate events never restart it.
        let (growth, alpha, active) = motion.sample(now + Duration::from_millis(90));
        assert!((growth - 0.875).abs() < 0.001 && (alpha - 0.25).abs() < 0.001 && active);
        assert_eq!(
            motion.sample(now + Duration::from_millis(300)),
            (1.0, 0.0, false)
        );
        assert_eq!(OpenMotion::new(now, false).sample(now), (1.0, 0.0, false));
    }

    #[test]
    fn restore_waits_for_activation_and_late_delivery_joins_without_restarting() {
        let requested = Instant::now();
        let began = requested + Duration::from_millis(500);
        let mut motion = OpenMotion::new(requested, true);
        assert_eq!(motion.sample(began), (0.0, 1.0, false));
        assert!(!motion.finished(began));
        motion.begin(began);
        let delivered = began + Duration::from_millis(90);
        assert_eq!(motion.sample(delivered), (0.875, 0.25, true));
        motion.begin(delivered); // Foreground/result notification cannot reset the curve.
        assert_eq!(motion.sample(delivered), (0.875, 0.25, true));
        assert!(motion.finished(began + Duration::from_millis(180)));
    }

    fn card(id: u64) -> Card {
        Card {
            id,
            label: "project - chat".into(),
            text: "update".into(),
            final_message: false,
            attention: false,
            target: None,
        }
    }

    #[test]
    fn interrupted_motion_continues_from_current_position() {
        let now = Instant::now();
        let mut tween = Tween::new(0.0, now);
        tween.target(1.0, now, true);
        assert_eq!(tween.value(now), 0.0);
        let mid = now + DURATION / 2;
        assert!((tween.value(mid) - 0.875).abs() < 0.001);
        let current = tween.value(mid);
        tween.target(0.0, mid, true);
        assert_eq!(tween.value(mid), current);
        assert_eq!(tween.value(mid + DURATION), 0.0);
        assert!(!tween.active(mid + DURATION));
    }

    #[test]
    fn dismissal_collapses_only_its_row_and_finishes_without_an_idle_timer() {
        let now = Instant::now();
        let mut motion = Motion::new(now);
        motion.sync(&[card(1), card(2)], &[100, 100], now, false);
        motion.sync(&[card(2), card(3)], &[100, 80], now, true);
        let (_, rows, active) = motion.sample(now + DURATION / 2);
        assert!(active);
        assert_eq!(
            rows.iter().map(|row| row.card.id).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(rows[0].height > 0 && rows[0].height < 100);
        assert!(!rows[0].interactive);
        assert_eq!(rows[1].height, 100);
        assert!(rows[2].height > 0 && rows[2].height < 80);
        let (_, rows, active) = motion.sample(now + DURATION);
        assert!(!active);
        assert_eq!(
            rows.iter().map(|row| row.card.id).collect::<Vec<_>>(),
            [2, 3]
        );
    }

    #[test]
    fn final_card_fades_as_a_panel_and_can_reverse_without_a_jump() {
        let now = Instant::now();
        let mut motion = Motion::new(now);
        motion.sync(&[card(1)], &[100], now, false);
        motion.sync(&[], &[], now, true);
        let mid = now + DURATION / 2;
        let (opacity, rows, _) = motion.sample(mid);
        assert!(opacity > 0.0 && opacity < 1.0);
        assert_eq!(rows[0].height, 100);
        assert!(!rows[0].interactive);
        motion.sync(&[card(1)], &[100], mid, true);
        assert_eq!(motion.sample(mid).0, opacity);
        assert_eq!(motion.sample(mid + DURATION).0, 1.0);
        motion.sync(&[], &[], mid + DURATION, true);
        let (opacity, rows, active) = motion.sample(mid + DURATION * 2);
        assert_eq!(opacity, 0.0);
        assert!(rows.is_empty());
        assert!(!active);
    }

    #[test]
    fn disabling_windows_animations_snaps_an_in_progress_transition() {
        let now = Instant::now();
        let mut motion = Motion::new(now);
        motion.sync(&[card(1)], &[100], now, true);
        motion.sync(&[card(1), card(2)], &[100, 80], now + DURATION / 3, false);
        let (opacity, rows, active) = motion.sample(now + DURATION / 3);
        assert_eq!(opacity, 1.0);
        assert_eq!(rows[1].height, 80);
        assert!(!active);
        motion.sync(&[], &[], now, false);
        assert!(motion.sample(now).1.is_empty());
    }

    #[test]
    fn pending_click_freezes_motion_and_resume_has_no_elapsed_time_jump() {
        let now = Instant::now();
        let mut clock = MotionClock::new(now);
        let before = clock.advance(now + Duration::from_millis(30), false);
        assert_eq!(
            clock.advance(now + Duration::from_millis(530), true),
            before
        );
        assert_eq!(
            clock.advance(now + Duration::from_millis(550), false),
            before + Duration::from_millis(20)
        );
    }

    #[test]
    fn edge_docking_reverses_smoothly_and_respects_reduced_motion() {
        let now = Instant::now();
        let mut dock = DockMotion::new(now);
        dock.target(true, now, true);
        let mid = now + Duration::from_millis(120);
        let (position, moving) = dock.sample(mid);
        assert!(moving && position > 0.0 && position < 1.0);
        dock.target(false, mid, true);
        assert_eq!(dock.sample(mid).0, position);
        assert_eq!(dock.sample(mid + Duration::from_millis(240)), (0.0, false));
        dock.target(true, mid, false);
        assert_eq!(dock.sample(mid), (1.0, false));
    }
}
