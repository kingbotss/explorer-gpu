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
    fn select_then_finish_sequence(&mut self) {
        let p = self.point("first-file-row");
        self.click(p, Modifiers::none(), 1, false);
        self.visual
            .read(|cx| assert!(self.view.read(cx).tab().selected.contains(&self.expected)));
        let view = self.view.clone();
        self.visual
            .update(|_, cx| view.update(cx, |view, _| view.clicks.clear()));
    }
    fn assert_open(&self) {
        self.visual
            .read(|cx| assert_eq!(self.view.read(cx).tab().path, self.expected));
    }
    fn finish_sizes(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let view = self.view.clone();
            let done = self.visual.update(|_, cx| {
                view.update(cx, |view, cx| {
                    view.poll(cx);
                    view.size_worker.is_none()
                })
            });
            if done {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "folder size worker timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
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
fn two_captured_presses_open_even_without_click_completions_or_native_counts(
    cx: &mut TestAppContext,
) {
    let mut f = Fixture::new(cx, false, false);
    let p = f.point("first-file-row");
    f.visual.simulate_mouse_move(p, None, Modifiers::none());
    for _ in 0..2 {
        f.visual.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: p,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
    }
    f.assert_open();
}
#[gpui::test]
fn icon_then_filename_presses_open_the_same_selected_folder(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.select_then_finish_sequence();
    let p = f.point("first-file-row");
    f.click(p, Modifiers::none(), 1, false);
    f.click(p + point(px(120.), px(0.)), Modifiers::none(), 1, false);
    f.assert_open();
}
#[gpui::test]
fn selecting_a_folder_calculates_nested_size_in_the_background(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    std::fs::create_dir(f.expected.join("nested")).unwrap();
    std::fs::write(f.expected.join("nested/file"), [0; 4500]).unwrap();
    std::fs::write(f.expected.join(".hidden"), [0; 100]).unwrap();
    let p = f.point("first-file-row");
    f.click(p, Modifiers::none(), 1, false);
    f.finish_sizes();
    f.visual.read(|cx| {
        let view = f.view.read(cx);
        let entry = &view.tab().entries[0];
        assert_eq!(view.entry_bytes(entry), Some(4600));
        assert!(view.size_label(entry, true).contains("4600 bytes"));
        assert!(view.size_label(entry, true).contains("2 files, 1 folders"));
        assert_eq!(view.tab().path, f.path);
    });
}
#[gpui::test]
fn size_sort_measures_folders_and_uses_content_bytes(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    std::fs::write(f.expected.join("large"), [0; 500]).unwrap();
    let smaller = f.path.join("z-smaller");
    std::fs::create_dir(&smaller).unwrap();
    std::fs::write(smaller.join("small"), [0; 10]).unwrap();
    let view = f.view.clone();
    f.visual.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.tab_mut()
                .entries
                .push(Entry::read(smaller.clone()).unwrap());
            view.rebuild(cx);
        })
    });
    f.menu_click("command-Sort", "menu-sort-size");
    f.finish_sizes();
    // Let the click interval expire before the deferred re-sort moves rows.
    let view = f.view.clone();
    f.visual.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.clicks.clear();
            view.poll(cx);
        })
    });
    f.visual.read(|cx| {
        let view = f.view.read(cx);
        assert_eq!(view.tab().entries[0].path, smaller);
        assert_eq!(view.entry_bytes(&view.tab().entries[1]), Some(500));
    });
}
#[gpui::test]
fn cancelling_folder_size_ignores_worker_results_from_the_previous_generation(
    cx: &mut TestAppContext,
) {
    let mut f = Fixture::new(cx, false, false);
    let view = f.view.clone();
    let path = f.expected.clone();
    f.visual.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.request_sizes(vec![path.clone()], true);
            let generation = view.size_generation;
            view.cancel_sizes();
            view.tx
                .send(super::Message::FolderSize {
                    generation,
                    path: path.clone(),
                    result: Ok(Default::default()),
                })
                .unwrap();
            view.poll(cx);
            assert!(!view.folder_sizes.contains_key(&path));
            assert!(view.size_worker.is_none());
        })
    });
}
#[gpui::test]
fn native_second_press_opens_before_a_lost_release(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.select_then_finish_sequence();
    let p = f.point("first-file-row");
    f.click(p, Modifiers::none(), 1, false);
    f.visual.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: p,
        modifiers: Modifiers {
            function: true,
            ..Modifiers::none()
        },
        click_count: 2,
        first_mouse: false,
    });
    f.assert_open();
}
#[gpui::test]
fn selected_grid_folder_opens_on_native_second_press(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, true, false);
    f.select_then_finish_sequence();
    let p = f.point("first-file-row");
    f.click(p, Modifiers::none(), 1, false);
    f.visual.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: p,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    f.assert_open();
    // The eventual release cannot start a copy or activate a child row.
    f.visual.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position: p + point(px(12.), px(2.)),
        modifiers: Modifiers::none(),
        click_count: 2,
    });
    f.assert_open();
    f.visual
        .read(|cx| assert!(f.view.read(cx).running.is_none()));
}
#[gpui::test]
fn native_release_after_failed_open_preserves_selection_and_does_not_drag(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, true);
    f.select_then_finish_sequence();
    std::fs::remove_dir(f.path.join("folder")).unwrap();
    let modifiers = Modifiers {
        platform: true,
        ..Modifiers::none()
    };
    let p = f.point("first-file-row");
    f.click(p, modifiers, 1, false);
    f.visual.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: p,
        modifiers,
        click_count: 2,
        first_mouse: false,
    });
    let up = p + point(px(12.), px(2.));
    f.visual
        .simulate_mouse_move(up, MouseButton::Left, modifiers);
    f.visual.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position: up,
        modifiers,
        click_count: 2,
    });
    f.visual.read(|cx| {
        let view = f.view.read(cx);
        assert!(view.tab().selected.contains(&f.expected));
        assert!(view.notice.as_ref().unwrap().1);
        assert!(view.running.is_none());
        assert!(!cx.has_active_drag());
    });
}
#[gpui::test]
fn terminal_remains_available_for_selected_folder_and_background(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.select_then_finish_sequence();
    f.point("command-terminal");
    let p = f.point("first-file-row");
    f.visual.simulate_event(MouseDownEvent {
        button: MouseButton::Right,
        position: p,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    f.point("menu-terminal");
    f.visual
        .read(|cx| assert_eq!(f.view.read(cx).terminal_path(), f.expected));
    f.visual.simulate_keystrokes("escape");
    let b = f.visual.debug_bounds("files-area").unwrap();
    f.visual.simulate_event(MouseDownEvent {
        button: MouseButton::Right,
        position: point(b.right() - px(10.), b.bottom() - px(10.)),
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    f.point("menu-terminal");
    f.visual
        .read(|cx| assert_eq!(f.view.read(cx).terminal_path(), f.path));
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
fn command_single_click_only_modifies_selection(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    let p = f.point("first-file-row");
    f.click(
        p,
        Modifiers {
            platform: true,
            ..Modifiers::none()
        },
        1,
        false,
    );
    f.visual.read(|cx| {
        assert_eq!(f.view.read(cx).tab().path, f.path);
        assert!(f.view.read(cx).tab().selected.contains(&f.expected));
    });
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

#[gpui::test]
fn already_selected_list_folder_opens_on_double_click(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.select_then_finish_sequence();
    f.double(Modifiers::none(), true, false);
    f.assert_open();
}
#[gpui::test]
fn already_selected_grid_folder_opens_on_double_click(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, true, false);
    f.select_then_finish_sequence();
    f.double(Modifiers::none(), true, false);
    f.assert_open();
}
#[gpui::test]
fn already_selected_folder_opens_with_synthetic_double_click(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.select_then_finish_sequence();
    f.double(Modifiers::none(), false, false);
    f.assert_open();
}
#[gpui::test]
fn already_selected_folder_symlink_opens_on_double_click(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, true);
    f.select_then_finish_sequence();
    f.double(Modifiers::none(), true, false);
    f.assert_open();
}
#[gpui::test]
fn selected_folder_double_click_wins_over_selection_toggle(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.select_then_finish_sequence();
    f.double(
        Modifiers {
            platform: true,
            ..Modifiers::none()
        },
        true,
        false,
    );
    f.assert_open();
}

#[gpui::test]
fn selected_folder_double_click_opens_with_shift_held(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    f.select_then_finish_sequence();
    f.double(
        Modifiers {
            shift: true,
            ..Modifiers::none()
        },
        true,
        false,
    );
    f.assert_open();
}
#[gpui::test]
fn double_click_opens_clicked_folder_from_multiple_selection(cx: &mut TestAppContext) {
    let mut f = Fixture::new(cx, false, false);
    let other = f.path.join("other");
    std::fs::create_dir(&other).unwrap();
    let view = f.view.clone();
    let selected = f.expected.clone();
    f.visual.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.tab_mut()
                .entries
                .push(Entry::read(other.clone()).unwrap());
            view.visible = vec![0, 1];
            view.tab_mut().selected.extend([selected, other]);
            cx.notify();
        })
    });
    f.visual.run_until_parked();
    f.double(Modifiers::none(), true, false);
    f.assert_open();
}
