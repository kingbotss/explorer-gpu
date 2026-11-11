use super::{Entry, Explorer, Modal, Session, Sort, Tab};
use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};
use std::path::PathBuf;
use std::{
    prelude::v1::test,
    sync::atomic::{AtomicUsize, Ordering as AtomicOrdering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    path: PathBuf,
    expected: PathBuf,
    view: Entity<Explorer>,
    visual: VisualTestContext,
}
impl Fixture {
    fn new(cx: &mut TestAppContext, grid: bool, link: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "explorer-ui-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("folder")).unwrap();
        let expected = if link {
            std::os::unix::fs::symlink("folder", path.join("folder link")).unwrap();
            path.join("folder link")
        } else {
            path.join("folder")
        };
        let window = cx.add_window(|window, cx| {
            let mut view = Explorer::new(window, cx);
            // Replace the initial asynchronous listing with an isolated fixture.
            view.tabs = vec![Tab::new(999, path.clone())];
            view.active = 0;
            view.tabs[0].entries = vec![Entry::read(expected.clone()).unwrap()];
            view.visible = vec![0];
            view.settings = Session {
                grid,
                ..Default::default()
            };
            view
        });
        let view = window.root(cx).unwrap();
        let visual = VisualTestContext::from_window(*window, cx);
        visual.run_until_parked();
        Self {
            path,
            expected,
            view,
            visual,
        }
    }
    fn point(&mut self, selector: &'static str) -> Point<Pixels> {
        let b = self
            .visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} must render"));
        point(b.left() + px(10.), b.top() + px(10.))
    }
    fn click(&mut self, pos: Point<Pixels>, modifiers: Modifiers, count: usize, jitter: bool) {
        self.visual.simulate_mouse_move(pos, None, modifiers);
        self.visual.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: pos,
            modifiers,
            click_count: count,
            first_mouse: false,
        });
        let up = if jitter {
            let p = pos + point(px(3.), px(2.));
            self.visual
                .simulate_mouse_move(p, MouseButton::Left, modifiers);
            p
        } else {
            pos
        };
        self.visual.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: up,
            modifiers,
            click_count: count,
        });
    }
    fn double(&mut self, modifiers: Modifiers, native: bool, jitter: bool) {
        let p = self.point("first-file-row");
        self.click(p, modifiers, 1, jitter);
        self.click(p, modifiers, if native { 2 } else { 1 }, jitter);
    }
    fn assert_open(&self) {
        self.visual
            .read(|cx| assert_eq!(self.view.read(cx).tab().path, self.expected));
    }
    fn menu_click(&mut self, command: &'static str, item: &'static str) {
        let p = self.point(command);
        self.click(p, Modifiers::none(), 1, false);
        let p = self.point(item);
        self.click(p, Modifiers::none(), 1, false);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.visual.update(|window, _| window.remove_window());
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
#[gpui::test]
fn list_native_double_click_opens(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.double(Modifiers::none(), true, false);
    f.assert_open();
}
#[gpui::test]
fn list_synthetic_double_click_opens(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.double(Modifiers::none(), false, false);
    f.assert_open();
}
#[gpui::test]
fn fn_flag_does_not_block_list_double_click(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.double(
        Modifiers {
            function: true,
            ..Modifiers::none()
        },
        true,
        false,
    );
    f.assert_open();
}
#[gpui::test]
fn fn_flag_with_synthetic_counts_opens(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.double(
        Modifiers {
            function: true,
            ..Modifiers::none()
        },
        false,
        false,
    );
    f.assert_open();
}
#[gpui::test]
fn grid_native_double_click_opens(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, true, false);
    f.double(Modifiers::none(), true, false);
    f.assert_open();
}
#[gpui::test]
fn grid_synthetic_fn_double_click_opens(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, true, false);
    f.double(
        Modifiers {
            function: true,
            ..Modifiers::none()
        },
        false,
        false,
    );
    f.assert_open();
}
#[gpui::test]
fn small_pointer_jitter_does_not_turn_click_into_copy(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.double(Modifiers::none(), true, true);
    f.assert_open();
    f.visual
        .read(|cx| assert!(f.view.read(cx).running.is_none()));
}
#[gpui::test]
fn single_click_selects_without_opening(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    let p = f.point("first-file-row");
    f.click(p, Modifiers::none(), 1, false);
    f.visual.read(|cx| {
        let v = f.view.read(cx);
        assert_eq!(v.tab().path, f.path);
        assert!(v.tab().selected.contains(&f.expected));
    });
}
#[gpui::test]
fn command_clicks_only_modify_selection(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.double(
        Modifiers {
            platform: true,
            ..Modifiers::none()
        },
        true,
        false,
    );
    f.visual
        .read(|cx| assert_eq!(f.view.read(cx).tab().path, f.path));
}
#[gpui::test]
fn selected_folder_enter_still_opens(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    let p = f.point("first-file-row");
    f.click(p, Modifiers::none(), 1, false);
    f.visual.simulate_keystrokes("enter");
    f.assert_open();
}
#[gpui::test]
fn folder_symlink_double_click_opens(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, true);
    f.double(Modifiers::none(), true, false);
    f.assert_open();
}
#[gpui::test]
fn new_command_opens_create_folder_dialog(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.menu_click("command-New", "menu-new-folder");
    f.visual.read(|cx| {
        assert!(matches!(
            f.view.read(cx).modal,
            Some(Modal::Create {
                directory: true,
                ..
            })
        ))
    });
}
#[gpui::test]
fn view_menu_switches_to_large_icons(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.menu_click("command-View", "menu-large-icons");
    f.visual.read(|cx| assert!(f.view.read(cx).settings.grid));
}
#[gpui::test]
fn sort_menu_changes_sort_column(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.menu_click("command-Sort", "menu-sort-modified");
    f.visual
        .read(|cx| assert_eq!(f.view.read(cx).sort, Sort::Modified));
}
