mod app;
mod icons;
mod input;
mod jobs;
mod native;
use gpui::{prelude::*, *};
actions!(
    explorer,
    [
        Quit,
        NewTab,
        NewFolder,
        CopyFiles,
        PasteFiles,
        UndoFiles,
        ShowHelp,
        RefreshFiles
    ]
);
fn main() {
    Application::new()
        .with_assets(icons::Assets)
        .run(|cx: &mut App| {
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.set_menus(vec![
                Menu {
                    name: "Explorer".into(),
                    items: vec![
                        MenuItem::action("About Explorer", ShowHelp),
                        MenuItem::separator(),
                        MenuItem::action("Quit Explorer", Quit),
                    ],
                },
                Menu {
                    name: "File".into(),
                    items: vec![
                        MenuItem::action("New Tab", NewTab),
                        MenuItem::action("New Folder", NewFolder),
                    ],
                },
                Menu {
                    name: "Edit".into(),
                    items: vec![
                        MenuItem::action("Undo File Operation", UndoFiles),
                        MenuItem::separator(),
                        MenuItem::action("Copy Files", CopyFiles),
                        MenuItem::action("Paste Files", PasteFiles),
                    ],
                },
                Menu {
                    name: "View".into(),
                    items: vec![MenuItem::action("Refresh", RefreshFiles)],
                },
                Menu {
                    name: "Help".into(),
                    items: vec![MenuItem::action("Keyboard Shortcuts", ShowHelp)],
                },
            ]);
            let bounds = Bounds::centered(None, size(px(1320.), px(840.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1020.), px(640.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("File Explorer".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(18.), px(12.))),
                    }),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| app::Explorer::new(window, cx)),
            )
            .expect("Could not open the Explorer window");
            cx.activate(true);
        });
}
