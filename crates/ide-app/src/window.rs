//! Estado de interação que pertence à janela nativa.

use std::time::{Duration, Instant};

use ui_core::Point;

#[derive(Default)]
pub(super) struct ClickTracker {
    last: Option<(Instant, Point)>,
}

impl ClickTracker {
    pub(super) fn register(
        &mut self,
        now: Instant,
        point: Point,
        interval: Duration,
        slack: f32,
    ) -> bool {
        let double = self.last.is_some_and(|(instant, previous)| {
            now.duration_since(instant) <= interval
                && (previous.x - point.x).abs() <= slack
                && (previous.y - point.y).abs() <= slack
        });
        self.last = (!double).then_some((now, point));
        double
    }
}
