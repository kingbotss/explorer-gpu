//! Pointer activation is keyed by path, independent of virtualized row indices.
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Press {
    tab: usize,
    path: PathBuf,
    time: Instant,
}
struct PointerPress {
    origin: (f32, f32),
    dragging: bool,
}

pub struct RowClicks {
    last: Option<Press>,
    pointer: Option<PointerPress>,
    interval: Duration,
}
impl Default for RowClicks {
    fn default() -> Self {
        Self::new(Duration::from_millis(500))
    }
}
impl RowClicks {
    pub fn new(interval: Duration) -> Self {
        Self {
            last: None,
            pointer: None,
            interval,
        }
    }
    pub fn clear(&mut self) {
        self.last = None;
        self.pointer = None;
    }
    pub fn release(&mut self) {
        self.pointer = None;
    }
    pub fn is_interacting(&self, now: Instant) -> bool {
        self.pointer.is_some()
            || self
                .last
                .as_ref()
                .is_some_and(|last| now.saturating_duration_since(last.time) <= self.interval)
    }
    /// GPUI starts a drag after only two pixels. Suppress that default until the
    /// pointer has moved eight pixels, then cancel the click sequence.
    pub fn suppress_drag_motion(&mut self, position: (f32, f32)) -> bool {
        let Some(pointer) = self.pointer.as_mut() else {
            return false;
        };
        if pointer.dragging {
            return false;
        }
        if (position.0 - pointer.origin.0).hypot(position.1 - pointer.origin.1) < 8.0 {
            return true;
        }
        pointer.dragging = true;
        self.last = None;
        false
    }
    pub fn begin_press(&mut self, position: (f32, f32)) {
        self.pointer = Some(PointerPress {
            origin: position,
            dragging: false,
        });
    }
    /// Activation follows completed clicks, the same event that selects a row.
    /// Fn and pointer motion do not change which path a completed click targets.
    pub fn complete(&mut self, tab: usize, path: &Path, now: Instant, native_count: usize) -> bool {
        self.release();
        let same_item = self
            .last
            .as_ref()
            .is_some_and(|last| last.tab == tab && last.path == path);
        let native_double = native_count == 2 && (self.last.is_none() || same_item);
        let timed_double = same_item
            && self
                .last
                .as_ref()
                .is_some_and(|last| now.saturating_duration_since(last.time) <= self.interval);
        if native_double || timed_double {
            self.clear();
            return true;
        }
        self.last = Some(Press {
            tab,
            path: path.to_path_buf(),
            time: now,
        });
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path() -> &'static Path {
        Path::new("/tmp/folder")
    }
    #[test]
    fn synthetic_double_click_does_not_need_release_counts() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        assert!(!clicks.complete(1, path(), now, 1));
        clicks.release();
        assert!(clicks.complete(1, path(), now + Duration::from_millis(180), 1));
    }
    #[test]
    fn native_double_click_survives_pointer_drift_and_delayed_delivery() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        assert!(!clicks.complete(1, path(), now, 1));
        clicks.release();
        assert!(clicks.complete(1, path(), now + Duration::from_secs(2), 2));
    }
    #[test]
    fn native_double_click_does_not_require_first_press_to_be_delivered() {
        assert!(RowClicks::default().complete(1, path(), Instant::now(), 2));
    }
    #[test]
    fn synthetic_clicks_on_icon_then_label_activate_same_item() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        assert!(!clicks.complete(1, path(), now, 1));
        clicks.release();
        assert!(clicks.complete(1, path(), now + Duration::from_millis(180), 1));
    }
    #[test]
    fn changing_paths_or_tabs_never_activates_another_item() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        assert!(!clicks.complete(1, path(), now, 1));
        clicks.release();
        assert!(!clicks.complete(1, Path::new("/tmp/b"), now + Duration::from_millis(80), 2));
        clicks.release();
        assert!(!clicks.complete(2, Path::new("/tmp/b"), now + Duration::from_millis(160), 2));
    }
    #[test]
    fn slow_single_clicks_do_not_activate() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        assert!(!clicks.complete(1, path(), now, 1));
        clicks.release();
        assert!(!clicks.complete(1, path(), now + Duration::from_millis(600), 1));
    }
    #[test]
    fn triple_click_consumes_only_one_pair() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        assert!(!clicks.complete(1, path(), now, 1));
        clicks.release();
        assert!(clicks.complete(1, path(), now + Duration::from_millis(100), 2));
        assert!(!clicks.complete(1, path(), now + Duration::from_millis(200), 3));
    }
    #[test]
    fn modified_click_or_navigation_clears_pair() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        assert!(!clicks.complete(1, path(), now, 1));
        clicks.clear();
        assert!(!clicks.complete(1, path(), now + Duration::from_millis(100), 1));
    }
    #[test]
    fn jitter_does_not_start_a_drag_but_deliberate_movement_does() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        clicks.begin_press((20., 20.));
        assert!(clicks.suppress_drag_motion((23., 22.)));
        assert!(!clicks.suppress_drag_motion((30., 20.)));
        assert!(
            !clicks.suppress_drag_motion((21., 20.)),
            "an active drag must not stop when returning to the origin"
        );
        clicks.release();
        assert!(!clicks.complete(1, path(), now + Duration::from_millis(100), 1));
    }
    #[test]
    fn refresh_waits_for_release_and_click_interval() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        clicks.begin_press((20., 20.));
        assert!(clicks.is_interacting(now + Duration::from_secs(2)));
        clicks.release();
        clicks.complete(1, path(), now, 1);
        assert!(clicks.is_interacting(now + Duration::from_millis(200)));
        assert!(!clicks.is_interacting(now + Duration::from_millis(600)));
    }
    #[test]
    fn system_slow_double_click_preference_is_honored() {
        let mut clicks = RowClicks::new(Duration::from_millis(900));
        let now = Instant::now();
        clicks.complete(1, path(), now, 1);
        clicks.release();
        assert!(clicks.complete(1, path(), now + Duration::from_millis(700), 1));
    }
}
