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
    activated_press: Option<(usize, PathBuf)>,
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
            activated_press: None,
            interval,
        }
    }
    pub fn clear(&mut self) {
        self.last = None;
        self.pointer = None;
        self.activated_press = None;
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
        if self.activated_press.is_some() {
            return true;
        }
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
        self.activated_press = None;
        self.pointer = Some(PointerPress {
            origin: position,
            dragging: false,
        });
    }
    /// Cocoa identifies a native double-click on the second down event. Open
    /// immediately so a drag, focus change, or lost up event cannot swallow it.
    pub fn native_press(&mut self, tab: usize, path: &Path, count: usize) -> bool {
        let same_item = self
            .last
            .as_ref()
            .is_none_or(|last| last.tab == tab && last.path == path);
        if count < 2 || count % 2 != 0 || !same_item {
            return false;
        }
        self.last = None;
        self.activated_press = Some((tab, path.to_path_buf()));
        true
    }
    pub fn consume_native_release(&mut self, tab: usize, path: &Path) -> bool {
        let consumed = self
            .activated_press
            .take()
            .is_some_and(|(pressed_tab, pressed_path)| pressed_tab == tab && pressed_path == path);
        if consumed {
            self.release();
        }
        consumed
    }
    /// Completed clicks select rows and provide a fallback for synthetic input.
    /// A release after native press activation must never open the item twice.
    pub fn complete(&mut self, tab: usize, path: &Path, now: Instant, native_count: usize) -> bool {
        self.release();
        if self.consume_native_release(tab, path) {
            return false;
        }
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
    fn native_press_is_consumed_before_release_and_triple_click() {
        let mut clicks = RowClicks::default();
        let now = Instant::now();
        clicks.begin_press((20., 20.));
        assert!(!clicks.native_press(1, path(), 1));
        assert!(!clicks.complete(1, path(), now, 1));
        clicks.begin_press((20., 20.));
        assert!(clicks.native_press(1, path(), 2));
        assert!(
            clicks.suppress_drag_motion((50., 50.)),
            "opening cannot also start a drag"
        );
        assert!(!clicks.complete(1, path(), now, 2));
        clicks.begin_press((20., 20.));
        assert!(!clicks.native_press(1, path(), 3));
        assert!(!clicks.complete(1, path(), now, 3));
        clicks.begin_press((20., 20.));
        assert!(clicks.native_press(1, path(), 4));
    }
    #[test]
    fn native_press_cannot_activate_a_different_row() {
        let mut clicks = RowClicks::default();
        clicks.complete(1, path(), Instant::now(), 1);
        clicks.begin_press((20., 20.));
        assert!(!clicks.native_press(1, Path::new("/tmp/other"), 2));
        assert!(!clicks.native_press(2, path(), 2));
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
    fn clearing_navigation_state_breaks_click_pair() {
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
