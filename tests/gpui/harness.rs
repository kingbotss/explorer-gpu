#![allow(dead_code)]
use gpui::actions;
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
#[path = "../../src/icons.rs"]
mod icons;
#[path = "../../src/input.rs"]
mod input;
#[path = "../../src/jobs.rs"]
mod jobs;
#[path = "../../src/native.rs"]
mod native;
mod app {
    include!("../../src/app.rs");
    #[cfg(test)]
    mod regression {
        include!("dispatch.rs");
    }
}
