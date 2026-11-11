use crate::{
    icons::{icon, item_icon},
    input::{Input, InputEvent},
    jobs::{self, Job, JobEvent},
    native,
};
use explorer::{
    fs::{self, Change, Entry, Listing, Sort},
    interaction::RowClicks,
    session::{self, Session},
};
use gpui::{prelude::*, *};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Palette {
    bg: u32,
    surface: u32,
    sidebar: u32,
    border: u32,
    text: u32,
    muted: u32,
    hover: u32,
    selected: u32,
    accent: u32,
}
impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: 0x191919,
                surface: 0x202020,
                sidebar: 0x202020,
                border: 0x363636,
                text: 0xf1f1f1,
                muted: 0xa6a6a6,
                hover: 0x2d2d2d,
                selected: 0x263d50,
                accent: 0x60cdff,
            }
        } else {
            Self {
                bg: 0xffffff,
                surface: 0xf3f3f3,
                sidebar: 0xfafafa,
                border: 0xe5e5e5,
                text: 0x242424,
                muted: 0x6b6b6b,
                hover: 0xf0f0f0,
                selected: 0xe5f3ff,
                accent: 0x0078d4,
            }
        }
    }
}
struct Tab {
    id: usize,
    path: PathBuf,
    history: Vec<PathBuf>,
    position: usize,
    entries: Vec<Entry>,
    selected: HashSet<PathBuf>,
    anchor: Option<usize>,
    cursor: Option<usize>,
    query: String,
    recursive: bool,
    loading: bool,
    revision: u64,
    cancel: Arc<AtomicBool>,
    issue: Option<String>,
}
impl Tab {
    fn new(id: usize, path: PathBuf) -> Self {
        Self {
            id,
            history: vec![path.clone()],
            path,
            position: 0,
            entries: vec![],
            selected: HashSet::new(),
            anchor: None,
            cursor: None,
            query: String::new(),
            recursive: false,
            loading: false,
            revision: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            issue: None,
        }
    }
}
enum Message {
    FolderSize {
        generation: u64,
        path: PathBuf,
        result: std::io::Result<fs::FolderSize>,
    },
    TerminalOpened(std::io::Result<PathBuf>),
    Loaded {
        tab: usize,
        revision: u64,
        result: std::io::Result<Listing>,
    },
    Preview {
        path: PathBuf,
        body: String,
    },
    Tree {
        path: PathBuf,
        children: Vec<PathBuf>,
    },
}
enum SizeValue {
    Calculating,
    Ready(fs::FolderSize),
    Failed(String),
}
struct SizeWorker {
    cancel: Arc<AtomicBool>,
}
struct Running {
    cancel: Arc<AtomicBool>,
    rx: Receiver<JobEvent>,
    label: String,
    bytes: u64,
    worker: Option<std::thread::JoinHandle<()>>,
}
#[derive(Clone)]
enum Modal {
    Create { parent: PathBuf, directory: bool },
    Rename(PathBuf),
    Trash(Vec<PathBuf>),
    Properties(Vec<Entry>),
    Help,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    FolderSizes,
    CancelSizes,
    Back,
    Forward,
    Up,
    Refresh,
    Home,
    NewTab,
    CloseTab,
    NewFolder,
    NewFile,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Rename,
    Trash,
    Undo,
    Compress,
    Extract,
    Open,
    OpenExternal,
    QuickLook,
    Reveal,
    Terminal,
    Pin,
    Hidden,
    Grid,
    List,
    Preview,
    Dark,
    Properties,
    Help,
    SortName,
    SortModified,
    SortKind,
    SortSize,
    SortDirection,
    Quit,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    Background,
    Selection,
    New,
    Sort,
    View,
}
struct MenuState {
    position: Point<Pixels>,
    kind: MenuKind,
}
#[derive(Clone)]
struct DragFiles {
    paths: Vec<PathBuf>,
}
impl Render for DragFiles {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_4()
            .py_2()
            .rounded_lg()
            .bg(rgb(0x0078d4))
            .text_color(rgb(0xffffff))
            .shadow_lg()
            .child(format!(
                "Copy {} item{}",
                self.paths.len(),
                if self.paths.len() == 1 { "" } else { "s" }
            ))
    }
}

pub struct Explorer {
    folder_sizes: HashMap<PathBuf, SizeValue>,
    size_queue: VecDeque<PathBuf>,
    size_worker: Option<SizeWorker>,
    size_generation: u64,
    size_sort_dirty: bool,
    tabs: Vec<Tab>,
    active: usize,
    next_tab: usize,
    visible: Vec<usize>,
    sort: Sort,
    descending: bool,
    settings: Session,
    focus: FocusHandle,
    address: Entity<Input>,
    search: Entity<Input>,
    field: Entity<Input>,
    editing_address: bool,
    scroll: UniformListScrollHandle,
    clicks: RowClicks,
    deferred_listing: Option<Message>,
    modal: Option<Modal>,
    modal_error: Option<String>,
    menu: Option<MenuState>,
    notice: Option<(String, bool)>,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    running: Option<Running>,
    undo: Vec<Vec<Change>>,
    clipboard: Vec<PathBuf>,
    cut: bool,
    clipboard_version: i64,
    preview_path: Option<PathBuf>,
    preview_body: String,
    tree: HashMap<PathBuf, Vec<PathBuf>>,
    expanded: HashSet<PathBuf>,
    last_refresh: Instant,
    subscriptions: Vec<Subscription>,
}
impl Explorer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Session::load(&Session::location());
        let address = cx.new(|cx| Input::new("Go to a folder…", cx));
        let search = cx.new(|cx| Input::new("Search this folder", cx));
        let field = cx.new(|cx| Input::new("Name", cx));
        let (tx, rx) = mpsc::channel();
        let initial = std::env::args_os().nth(1).map(PathBuf::from);
        let paths = if let Some(p) = initial {
            vec![std::fs::canonicalize(p).unwrap_or_else(|_| session::home())]
        } else if settings.tabs.is_empty() {
            vec![session::home()]
        } else {
            settings.tabs.clone()
        };
        let tabs: Vec<_> = paths
            .into_iter()
            .enumerate()
            .map(|(i, p)| Tab::new(i, p))
            .collect();
        let active = settings.active.min(tabs.len() - 1);
        let mut this = Self {
            folder_sizes: HashMap::new(),
            size_queue: VecDeque::new(),
            size_worker: None,
            size_generation: 0,
            size_sort_dirty: false,
            next_tab: tabs.len(),
            tabs,
            active,
            visible: vec![],
            sort: Sort::Name,
            descending: false,
            settings,
            focus: cx.focus_handle(),
            address: address.clone(),
            search: search.clone(),
            field: field.clone(),
            editing_address: false,
            scroll: UniformListScrollHandle::new(),
            clicks: RowClicks::for_system(native::double_click_interval()),
            deferred_listing: None,
            modal: None,
            modal_error: None,
            menu: None,
            notice: None,
            tx,
            rx,
            running: None,
            undo: vec![],
            clipboard: vec![],
            cut: false,
            clipboard_version: -1,
            preview_path: None,
            preview_body: String::new(),
            tree: HashMap::new(),
            expanded: HashSet::new(),
            last_refresh: Instant::now(),
            subscriptions: vec![],
        };
        this.subscriptions.push(cx.subscribe_in(
            &address,
            window,
            |this, entity, event, window, cx| match event {
                InputEvent::Submit => {
                    let p = session::expand_path(entity.read(cx).value(), &this.tab().path);
                    this.navigate(p, cx);
                    this.editing_address = false;
                    this.focus.focus(window);
                }
                InputEvent::Cancel => {
                    this.editing_address = false;
                    this.focus.focus(window);
                    cx.notify();
                }
                _ => {}
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &search,
            window,
            |this, entity, event, window, cx| match event {
                InputEvent::Changed => {
                    let value = entity.read(cx).value().to_owned();
                    let was_recursive = this.tab().recursive;
                    this.tab_mut().query = value;
                    if was_recursive {
                        this.tab_mut().recursive = false;
                        this.load(this.active, false, cx);
                    }
                    this.rebuild(cx);
                }
                InputEvent::Submit => {
                    if !this.tab().query.trim().is_empty() {
                        this.tab_mut().recursive = true;
                        this.load(this.active, true, cx);
                    }
                    this.focus.focus(window);
                }
                InputEvent::Cancel => {
                    this.clear_search(cx);
                    this.focus.focus(window);
                }
            },
        ));
        this.subscriptions.push(
            cx.subscribe_in(&field, window, |this, _, event, window, cx| match event {
                InputEvent::Submit => this.submit_modal(window, cx),
                InputEvent::Cancel => this.close_modal(window, cx),
                _ => {}
            }),
        );
        this.focus.focus(window);
        this.trace(&format!(
            "started executable={:?} system_double_click_ms={} activation_double_click_ms={}",
            std::env::current_exe().ok(),
            native::double_click_interval().as_millis(),
            this.clicks.interval().as_millis()
        ));
        this.load(this.active, false, cx);
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(80)).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        this
    }
    fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }
    fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }
    fn palette(&self) -> Palette {
        Palette::new(self.settings.dark)
    }
    fn save(&mut self) {
        self.settings.tabs = self.tabs.iter().map(|t| t.path.clone()).collect();
        self.settings.active = self.active;
        #[cfg(not(test))]
        if let Err(e) = self.settings.save(&Session::location()) {
            self.notice = Some((format!("Could not save preferences: {e}"), true));
        }
    }
    fn load(&mut self, index: usize, recursive: bool, cx: &mut Context<Self>) {
        let hidden = self.settings.hidden;
        let tx = self.tx.clone();
        let tab = &mut self.tabs[index];
        tab.cancel.store(true, Ordering::Relaxed);
        tab.cancel = Arc::new(AtomicBool::new(false));
        tab.revision += 1;
        tab.loading = true;
        let (id, revision, path, query, cancel) = (
            tab.id,
            tab.revision,
            tab.path.clone(),
            tab.query.clone(),
            tab.cancel.clone(),
        );
        std::thread::spawn(move || {
            let result = if recursive {
                fs::search(&path, &query, hidden, &cancel)
            } else {
                fs::list(&path, hidden, &cancel)
            };
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Message::Loaded {
                    tab: id,
                    revision,
                    result,
                });
            }
        });
        self.last_refresh = Instant::now();
        cx.notify();
    }
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let (sort, descending) = (self.sort, self.descending);
        if sort == Sort::Size {
            let cache = &self.folder_sizes;
            let entries = &mut self.tabs[self.active].entries;
            fs::sort(entries, Sort::Name, false);
            entries.sort_by(|a, b| {
                let bytes = |e: &Entry| {
                    if e.directory {
                        match cache.get(&e.path) {
                            Some(SizeValue::Ready(size)) => Some(size.bytes),
                            _ => None,
                        }
                    } else {
                        Some(e.bytes)
                    }
                };
                b.directory
                    .cmp(&a.directory)
                    .then_with(|| match (bytes(a), bytes(b)) {
                        (Some(a), Some(b)) => {
                            if descending {
                                b.cmp(&a)
                            } else {
                                a.cmp(&b)
                            }
                        }
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    })
            });
        } else {
            fs::sort(&mut self.tab_mut().entries, sort, descending);
        }
        let tab = self.tab();
        let q = tab.query.to_lowercase();
        self.visible = tab
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| q.is_empty() || e.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        let existing: HashSet<_> = self
            .visible
            .iter()
            .map(|i| self.tab().entries[*i].path.clone())
            .collect();
        self.tab_mut().selected.retain(|p| existing.contains(p));
        self.update_preview(cx);
        cx.notify();
    }
    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.trace(&format!("navigate {:?}", path));
        self.cancel_sizes();
        self.folder_sizes.clear();
        self.clicks.clear();
        self.deferred_listing = None;
        self.menu = None;
        self.editing_address = false;
        self.search.update(cx, |input, cx| input.set("", cx));
        let tab = self.tab_mut();
        if tab.path != path {
            tab.history.truncate(tab.position + 1);
            tab.history.push(path.clone());
            tab.position = tab.history.len() - 1;
        }
        tab.path = path;
        tab.entries.clear();
        tab.selected.clear();
        tab.anchor = None;
        tab.cursor = None;
        tab.query.clear();
        tab.recursive = false;
        tab.issue = None;
        self.visible.clear();
        self.scroll = UniformListScrollHandle::new();
        self.preview_path = None;
        self.load(self.active, false, cx);
        self.save();
    }
    fn history(&mut self, forward: bool, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        if forward && tab.position + 1 < tab.history.len() {
            tab.position += 1;
        } else if !forward && tab.position > 0 {
            tab.position -= 1;
        } else {
            return;
        }
        let path = tab.history[tab.position].clone();
        // Traversing history must not create a new entry or truncate forward history.
        tab.path = path.clone();
        self.navigate(path, cx);
    }
    fn clear_search(&mut self, cx: &mut Context<Self>) {
        let recursive = self.tab().recursive;
        self.tab_mut().query.clear();
        self.tab_mut().recursive = false;
        self.search.update(cx, |input, cx| input.set("", cx));
        if recursive {
            self.load(self.active, false, cx);
        }
        self.rebuild(cx);
    }
    fn switch_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.clicks.clear();
        self.cancel_sizes();
        self.folder_sizes.clear();
        self.preview_path = None;
        self.active = index;
        self.menu = None;
        self.editing_address = false;
        let query = self.tab().query.clone();
        self.search.update(cx, |input, cx| input.set(query, cx));
        self.scroll = UniformListScrollHandle::new();
        self.rebuild(cx);
        if self.tab().entries.is_empty() && !self.tab().loading {
            self.load(index, self.tab().recursive, cx);
        }
        self.focus.focus(window);
        self.save();
    }
    fn new_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() >= 64 {
            self.notice = Some(("The maximum is 64 tabs.".into(), true));
            return;
        }
        let id = self.next_tab;
        self.next_tab += 1;
        self.tabs.push(Tab::new(id, path));
        self.switch_tab(self.tabs.len() - 1, window, cx);
    }
    fn selected(&self) -> Vec<PathBuf> {
        self.visible
            .iter()
            .filter_map(|i| {
                let e = &self.tab().entries[*i];
                self.tab()
                    .selected
                    .contains(&e.path)
                    .then(|| e.path.clone())
            })
            .collect()
    }
    fn selected_entries(&self) -> Vec<Entry> {
        self.visible
            .iter()
            .filter_map(|i| {
                let e = &self.tab().entries[*i];
                self.tab().selected.contains(&e.path).then(|| e.clone())
            })
            .collect()
    }
    fn select(&mut self, index: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        if index >= self.visible.len() {
            return;
        }
        let path = self.tab().entries[self.visible[index]].path.clone();
        self.tab_mut().cursor = Some(index);
        if self.tab().anchor.is_none() {
            self.tab_mut().anchor = Some(index);
        }
        if modifiers.shift {
            let anchor = self
                .tab()
                .anchor
                .unwrap_or(index)
                .min(self.visible.len().saturating_sub(1));
            let paths: Vec<_> = (anchor.min(index)..=anchor.max(index))
                .map(|i| self.tab().entries[self.visible[i]].path.clone())
                .collect();
            if !modifiers.platform {
                self.tab_mut().selected.clear();
            }
            self.tab_mut().selected.extend(paths);
        } else if modifiers.platform {
            if !self.tab_mut().selected.remove(&path) {
                self.tab_mut().selected.insert(path);
            }
            self.tab_mut().anchor = Some(index);
        } else {
            self.tab_mut().selected.clear();
            self.tab_mut().selected.insert(path);
            self.tab_mut().anchor = Some(index);
        }
        self.update_preview(cx);
        cx.notify();
    }
    fn row_index(&self, tab: usize, path: &Path) -> Option<usize> {
        if self.tab().id != tab {
            return None;
        }
        self.visible
            .iter()
            .position(|index| self.tab().entries[*index].path == path)
    }
    fn trace(&self, event: &str) {
        #[cfg(not(test))]
        {
            use std::io::Write;
            let _ = (|| -> std::io::Result<()> {
                let location = Session::location();
                let directory = location.parent().unwrap();
                std::fs::create_dir_all(directory)?;
                let path = directory.join("input.log");
                if std::fs::metadata(&path).is_ok_and(|m| m.len() > 128 * 1024) {
                    std::fs::rename(&path, directory.join("input.previous.log"))?;
                }
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?;
                writeln!(
                    file,
                    "{:?} v{} pid={} {}",
                    std::time::SystemTime::now(),
                    env!("CARGO_PKG_VERSION"),
                    std::process::id(),
                    event
                )
            })();
        }
        #[cfg(test)]
        let _ = event;
    }
    fn press_entry(
        &mut self,
        tab: usize,
        entry: &Entry,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal.is_some() || self.menu.is_some() {
            return;
        }
        let Some(index) = self.row_index(tab, &entry.path) else {
            return;
        };
        self.focus.focus(window);
        self.clicks
            .begin_press((f32::from(event.position.x), f32::from(event.position.y)));
        let activate = self
            .clicks
            .press(tab, &entry.path, Instant::now(), event.click_count);
        self.trace(&format!(
            "row-down count={} activate={} selected={} path={:?}",
            event.click_count,
            activate,
            self.tab().selected.contains(&entry.path),
            entry.path
        ));
        if activate {
            self.select(index, Modifiers::none(), cx);
            self.open_entry(entry, cx);
            cx.stop_propagation();
        }
    }
    fn open_entry(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        self.trace(&format!("open {:?}", entry.path));
        match Entry::read_for_open(entry.path.clone()) {
            Ok(current) if current.navigable() => self.navigate(current.path, cx),
            Ok(current) => {
                let result = if let Some(kind) = explorer::media::kind(&current.path) {
                    native::open_media(&current.path, kind)
                } else {
                    native::open(&current.path)
                };
                self.result(result, cx);
            }
            Err(error) => self.result(
                Err(std::io::Error::new(
                    error.kind(),
                    format!("Cannot open {}: {error}", entry.path.display()),
                )),
                cx,
            ),
        }
    }
    fn cancel_sizes(&mut self) {
        if let Some(worker) = self.size_worker.take() {
            worker.cancel.store(true, Ordering::Relaxed);
        }
        self.size_queue.clear();
        self.size_generation += 1;
        self.folder_sizes
            .retain(|_, value| !matches!(value, SizeValue::Calculating));
        self.size_sort_dirty = false;
    }
    fn request_sizes(&mut self, paths: Vec<PathBuf>, force: bool) {
        if force {
            self.cancel_sizes();
        }
        for path in paths {
            if force || !self.folder_sizes.contains_key(&path) {
                self.folder_sizes
                    .insert(path.clone(), SizeValue::Calculating);
                self.size_queue.push_back(path);
            }
        }
        self.start_size_worker();
    }
    fn start_size_worker(&mut self) {
        if self.size_worker.is_some() {
            return;
        }
        let Some(path) = self.size_queue.pop_front() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.size_worker = Some(SizeWorker {
            cancel: cancel.clone(),
        });
        let tx = self.tx.clone();
        let generation = self.size_generation;
        std::thread::spawn(move || {
            let result = fs::folder_size(&path, &cancel);
            let _ = tx.send(Message::FolderSize {
                generation,
                path,
                result,
            });
        });
    }
    fn entry_bytes(&self, entry: &Entry) -> Option<u64> {
        if !entry.directory {
            return Some(entry.bytes);
        }
        match self.folder_sizes.get(&entry.path) {
            Some(SizeValue::Ready(size)) => Some(size.bytes),
            _ => None,
        }
    }
    fn size_label(&self, entry: &Entry, detailed: bool) -> String {
        if !entry.directory {
            return if detailed {
                format!("{} ({} bytes)", fs::human_size(entry.bytes), entry.bytes)
            } else {
                fs::human_size(entry.bytes)
            };
        }
        match self.folder_sizes.get(&entry.path) {
            Some(SizeValue::Ready(size)) => {
                let bytes = if detailed {
                    format!(
                        "{} ({} bytes) · {} files, {} folders, {} links",
                        fs::human_size(size.bytes),
                        size.bytes,
                        size.files,
                        size.folders,
                        size.links
                    )
                } else {
                    fs::human_size(size.bytes)
                };
                if size.skipped > 0 {
                    if detailed {
                        format!("{bytes} · partial ({} unreadable)", size.skipped)
                    } else {
                        format!("≥ {bytes}")
                    }
                } else {
                    bytes
                }
            }
            Some(SizeValue::Calculating) => "Calculating…".into(),
            Some(SizeValue::Failed(error)) => {
                if detailed {
                    format!("Unavailable: {error}")
                } else {
                    "Unavailable".into()
                }
            }
            None => {
                if detailed {
                    "Not calculated".into()
                } else {
                    "—".into()
                }
            }
        }
    }
    fn update_preview(&mut self, _cx: &mut Context<Self>) {
        let paths = self.selected();
        let path = if paths.len() == 1 {
            Some(paths[0].clone())
        } else {
            None
        };
        if path == self.preview_path {
            return;
        }
        self.preview_path = path.clone();
        self.preview_body.clear();
        if let Some(path) = path {
            if path.is_dir() {
                self.request_sizes(vec![path.clone()], false);
            }
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let body = if path.is_dir() {
                    "Folder · double-click to browse\n\nPress Space to open Quick Look.".into()
                } else {
                    fs::preview_text(&path).unwrap_or_else(|e| e.to_string())
                };
                let _ = tx.send(Message::Preview { path, body });
            });
        }
    }
    fn poll(&mut self, cx: &mut Context<Self>) {
        let interacting = self.clicks.is_interacting(Instant::now());
        let mut messages = Vec::new();
        if !interacting {
            messages.extend(self.deferred_listing.take());
        }
        messages.extend(self.rx.try_iter());
        for message in messages {
            if interacting
                && matches!(&message, Message::Loaded {tab,revision,..} if *tab == self.tab().id && *revision == self.tab().revision)
            {
                self.deferred_listing = Some(message);
                continue;
            }
            match message {
                Message::FolderSize {
                    generation,
                    path,
                    result,
                } => {
                    if generation == self.size_generation {
                        self.size_worker = None;
                        self.folder_sizes.insert(
                            path,
                            match result {
                                Ok(size) => SizeValue::Ready(size),
                                Err(error) => SizeValue::Failed(error.to_string()),
                            },
                        );
                        self.size_sort_dirty = self.sort == Sort::Size;
                        self.start_size_worker();
                        cx.notify();
                    }
                }
                Message::TerminalOpened(result) => {
                    match result {
                        Ok(path) => {
                            self.notice =
                                Some((format!("Opened Terminal in {}", path.display()), false))
                        }
                        Err(error) => self.result(Err(error), cx),
                    }
                    cx.notify();
                }
                Message::Loaded {
                    tab,
                    revision,
                    result,
                } => {
                    if let Some(index) = self
                        .tabs
                        .iter()
                        .position(|t| t.id == tab && t.revision == revision)
                    {
                        let t = &mut self.tabs[index];
                        t.loading = false;
                        match result {
                            Ok(listing) => {
                                t.entries = listing.entries;
                                t.issue = if listing.truncated {
                                    Some(
                                        "Showing the first 10,000 results. Refine your search."
                                            .into(),
                                    )
                                } else if listing.skipped > 0 {
                                    Some(format!("{} inaccessible items skipped", listing.skipped))
                                } else {
                                    None
                                };
                            }
                            Err(e) => {
                                t.entries.clear();
                                t.issue = Some(format!("Unable to read {}: {e}", t.path.display()));
                            }
                        }
                        if index == self.active {
                            self.rebuild(cx);
                        }
                    }
                }
                Message::Preview { path, body } => {
                    if self.preview_path.as_ref() == Some(&path) {
                        self.preview_body = body;
                        cx.notify();
                    }
                }
                Message::Tree { path, children } => {
                    self.tree.insert(path, children);
                    cx.notify();
                }
            }
        }
        if self.size_sort_dirty && !self.clicks.is_interacting(Instant::now()) {
            self.size_sort_dirty = false;
            self.rebuild(cx);
        }
        let events: Vec<_> = self
            .running
            .as_ref()
            .map(|r| r.rx.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                JobEvent::Progress { label, bytes } => {
                    if let Some(r) = &mut self.running {
                        r.label = label;
                        r.bytes = bytes;
                    }
                    cx.notify();
                }
                JobEvent::Done {
                    changes,
                    errors,
                    summary,
                    remaining_undo,
                } => {
                    self.running = None;
                    self.cancel_sizes();
                    self.folder_sizes.clear();
                    if let Some(records) = remaining_undo {
                        if !records.is_empty() {
                            self.undo.push(records);
                        }
                    } else if !changes.is_empty() {
                        self.undo.push(changes);
                        if self.undo.len() > 30 {
                            self.undo.remove(0);
                        }
                    }
                    self.notice = Some((
                        if errors.is_empty() {
                            summary
                        } else {
                            format!("{summary} · {}", errors.join("\n"))
                        },
                        !errors.is_empty(),
                    ));
                    self.tree.clear();
                    self.preview_path = None;
                    self.load(self.active, self.tab().recursive, cx);
                    cx.notify();
                }
            }
        }
        if self.last_refresh.elapsed() > Duration::from_secs(3)
            && !self.clicks.is_interacting(Instant::now())
            && !self.tab().loading
            && !self.tab().recursive
            && self.modal.is_none()
            && self.running.is_none()
        {
            self.load(self.active, false, cx);
        }
    }
    fn start(&mut self, job: Job, label: &str, cx: &mut Context<Self>) {
        if self.running.is_some() {
            self.notice = Some((
                "Wait for the current operation to finish, or cancel it.".into(),
                true,
            ));
            cx.notify();
            return;
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = std::thread::spawn(move || jobs::run(job, worker_cancel, tx));
        self.running = Some(Running {
            cancel,
            rx,
            label: label.into(),
            bytes: 0,
            worker: Some(worker),
        });
        cx.notify();
    }
    fn result(&mut self, result: std::io::Result<()>, cx: &mut Context<Self>) {
        if let Err(e) = result {
            self.trace(&format!("error {e}"));
            self.notice = Some((e.to_string(), true));
            cx.notify();
        }
    }
    fn show_modal(
        &mut self,
        modal: Modal,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        self.modal_error = None;
        let needs_input = matches!(modal, Modal::Create { .. } | Modal::Rename(_));
        self.modal = Some(modal);
        if !needs_input {
            self.focus.focus(window);
        }
        self.field.update(cx, |field, cx| {
            field.set(value, cx);
            if needs_input {
                field.activate(window, cx);
            }
        });
        cx.notify();
    }
    fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modal = None;
        self.modal_error = None;
        self.focus.focus(window);
        cx.notify();
    }
    fn submit_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.modal.clone() else {
            return;
        };
        let name = self.field.read(cx).value().to_owned();
        let job = match modal {
            Modal::Create { parent, directory } => {
                if let Err(e) = fs::validate_name(&name) {
                    self.modal_error = Some(e.to_string());
                    cx.notify();
                    return;
                }
                Job::Create {
                    parent,
                    name,
                    directory,
                }
            }
            Modal::Rename(path) => {
                if let Err(e) = fs::validate_name(&name) {
                    self.modal_error = Some(e.to_string());
                    cx.notify();
                    return;
                }
                Job::Rename { path, name }
            }
            Modal::Trash(paths) => Job::Trash(paths),
            _ => {
                self.close_modal(window, cx);
                return;
            }
        };
        if self.running.is_some() {
            self.modal_error = Some("An operation is already running.".into());
            cx.notify();
            return;
        }
        self.close_modal(window, cx);
        self.start(job, "Working…", cx);
    }
    fn terminal_path(&self) -> PathBuf {
        if !self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.kind == MenuKind::Background)
        {
            let paths = self.selected();
            if paths.len() == 1 {
                return paths[0].clone();
            }
        }
        self.tab().path.clone()
    }
    pub fn perform(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let terminal_path = (action == Action::Terminal).then(|| self.terminal_path());
        let all_sizes = self
            .menu
            .as_ref()
            .is_some_and(|menu| matches!(menu.kind, MenuKind::Background | MenuKind::View));
        self.menu = None;
        if self.running.is_some()
            && matches!(
                action,
                Action::Paste
                    | Action::Duplicate
                    | Action::NewFolder
                    | Action::NewFile
                    | Action::Rename
                    | Action::Trash
                    | Action::Undo
                    | Action::Compress
            )
        {
            self.notice = Some((
                "Wait for the current operation to finish, or cancel it.".into(),
                true,
            ));
            cx.notify();
            return;
        }
        let paths = self.selected();
        match action {
            Action::FolderSizes => {
                let entries = if let Some(Modal::Properties(entries)) = &self.modal {
                    entries.clone()
                } else if all_sizes {
                    vec![]
                } else {
                    self.selected_entries()
                };
                let entries = if entries.is_empty() {
                    self.visible
                        .iter()
                        .map(|&i| self.tab().entries[i].clone())
                        .collect()
                } else {
                    entries
                };
                self.request_sizes(
                    entries
                        .into_iter()
                        .filter(|e| e.directory)
                        .map(|e| e.path)
                        .collect(),
                    true,
                );
            }
            Action::CancelSizes => self.cancel_sizes(),
            Action::SortName | Action::SortModified | Action::SortKind | Action::SortSize => {
                self.sort = match action {
                    Action::SortModified => Sort::Modified,
                    Action::SortKind => Sort::Kind,
                    Action::SortSize => Sort::Size,
                    _ => Sort::Name,
                };
                if self.sort == Sort::Size {
                    self.request_sizes(
                        self.visible
                            .iter()
                            .map(|&i| &self.tab().entries[i])
                            .filter(|e| e.directory)
                            .map(|e| e.path.clone())
                            .collect(),
                        false,
                    );
                }
                self.rebuild(cx);
            }
            Action::SortDirection => {
                self.descending = !self.descending;
                self.rebuild(cx);
            }
            Action::Back => self.history(false, cx),
            Action::Forward => self.history(true, cx),
            Action::Up => {
                if let Some(p) = self.tab().path.parent() {
                    self.navigate(p.to_path_buf(), cx);
                }
            }
            Action::Home => self.navigate(session::home(), cx),
            Action::Refresh => {
                self.cancel_sizes();
                self.folder_sizes.clear();
                self.tree.clear();
                self.preview_path = None;
                self.load(self.active, self.tab().recursive, cx);
            }
            Action::NewTab => self.new_tab(self.tab().path.clone(), window, cx),
            Action::CloseTab => {
                if self.tabs.len() > 1 {
                    self.tabs[self.active].cancel.store(true, Ordering::Relaxed);
                    self.tabs.remove(self.active);
                    self.active = self.active.min(self.tabs.len() - 1);
                    self.switch_tab(self.active, window, cx);
                } else {
                    self.perform(Action::Quit, window, cx);
                }
            }
            Action::NewFolder | Action::NewFile => self.show_modal(
                Modal::Create {
                    parent: self.tab().path.clone(),
                    directory: action == Action::NewFolder,
                },
                if action == Action::NewFolder {
                    "New folder"
                } else {
                    "Untitled.txt"
                },
                window,
                cx,
            ),
            Action::Copy | Action::Cut => {
                if !paths.is_empty() {
                    match native::clipboard_write(&paths) {
                        Ok(version) => {
                            self.clipboard_version = version;
                            self.clipboard = paths;
                            self.cut = action == Action::Cut;
                            self.notice = Some((
                                format!(
                                    "{} item{} ready to {}",
                                    self.clipboard.len(),
                                    if self.clipboard.len() == 1 { "" } else { "s" },
                                    if self.cut { "move" } else { "copy" }
                                ),
                                false,
                            ));
                        }
                        Err(e) => self.notice = Some((e.to_string(), true)),
                    }
                }
            }
            Action::Paste => {
                let local = native::clipboard_version() == self.clipboard_version;
                let source = if local {
                    self.clipboard.clone()
                } else {
                    native::clipboard_read()
                };
                if !source.is_empty() {
                    let cut = local && self.cut;
                    self.start(
                        Job::Transfer {
                            paths: source,
                            destination: self.tab().path.clone(),
                            cut,
                        },
                        if cut { "Moving files" } else { "Copying files" },
                        cx,
                    );
                    if cut {
                        self.cut = false;
                        self.clipboard.clear();
                    }
                } else {
                    self.notice = Some((
                        "Copy files in Explorer or Finder, then paste them here.".into(),
                        false,
                    ));
                }
            }
            Action::Duplicate => {
                if !paths.is_empty() {
                    self.start(
                        Job::Transfer {
                            paths,
                            destination: self.tab().path.clone(),
                            cut: false,
                        },
                        "Duplicating files",
                        cx,
                    );
                }
            }
            Action::Rename => {
                if paths.len() == 1 {
                    let name = paths[0]
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.show_modal(Modal::Rename(paths[0].clone()), &name, window, cx);
                }
            }
            Action::Trash => {
                if !paths.is_empty() {
                    self.show_modal(Modal::Trash(paths), "", window, cx);
                }
            }
            Action::Undo => {
                if self.running.is_none() {
                    if let Some(records) = self.undo.pop() {
                        self.start(Job::Undo(records), "Undoing last operation", cx);
                    }
                }
            }
            Action::Compress => {
                if !paths.is_empty() {
                    self.start(Job::Compress(paths), "Creating ZIP archives", cx);
                }
            }
            Action::Extract => {
                for path in paths {
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
                    {
                        self.result(native::extract(&path), cx);
                    }
                }
            }
            Action::Open => {
                let entries = self.selected_entries();
                if entries.len() == 1 {
                    self.open_entry(&entries[0], cx);
                } else {
                    for entry in entries.into_iter().filter(|entry| !entry.directory) {
                        self.open_entry(&entry, cx);
                    }
                }
            }
            Action::QuickLook => {
                if !paths.is_empty() {
                    self.result(native::quick_look(&paths), cx);
                }
            }
            Action::OpenExternal => {
                for path in paths {
                    self.result(native::open(&path), cx);
                }
            }
            Action::Reveal => {
                let p = paths.first().unwrap_or(&self.tab().path).clone();
                self.result(native::reveal(&p), cx);
            }
            Action::Terminal => {
                let p = terminal_path.unwrap();
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let result = native::terminal(&p);
                    let _ = tx.send(Message::TerminalOpened(result));
                });
            }
            Action::Pin => {
                let p = if paths.len() == 1 && paths[0].is_dir() {
                    paths[0].clone()
                } else {
                    self.tab().path.clone()
                };
                if self.settings.favorites.contains(&p) {
                    self.settings.favorites.retain(|x| x != &p);
                } else {
                    self.settings.favorites.push(p);
                }
                self.save();
            }
            Action::Hidden => {
                self.settings.hidden = !self.settings.hidden;
                self.tree.clear();
                self.load(self.active, self.tab().recursive, cx);
                self.save();
            }
            Action::Grid | Action::List => {
                self.settings.grid = action == Action::Grid;
                self.save();
            }
            Action::Preview => {
                self.settings.preview = !self.settings.preview;
                self.save();
            }
            Action::Dark => {
                self.settings.dark = !self.settings.dark;
                self.save();
            }
            Action::Properties => {
                let mut entries = self.selected_entries();
                if entries.is_empty() {
                    if let Ok(e) = Entry::read(self.tab().path.clone()) {
                        entries.push(e);
                    }
                }
                self.request_sizes(
                    entries
                        .iter()
                        .filter(|e| e.directory)
                        .map(|e| e.path.clone())
                        .collect(),
                    false,
                );
                self.show_modal(Modal::Properties(entries), "", window, cx);
            }
            Action::Help => self.show_modal(Modal::Help, "", window, cx),
            Action::Quit => {
                if self.running.is_some() {
                    self.notice = Some((
                        "An operation is running. Finish or cancel it before quitting.".into(),
                        true,
                    ));
                } else {
                    self.save();
                    cx.quit();
                }
            }
        }
        cx.notify();
    }
    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let m = k.modifiers;
        if self.modal.is_some() {
            if k.key == "escape" {
                self.close_modal(window, cx);
            } else if k.key == "enter" {
                self.submit_modal(window, cx);
            }
            return;
        }
        let typing = self.address.read(cx).focus.is_focused(window)
            || self.search.read(cx).focus.is_focused(window);
        if typing
            && !(m.platform && matches!(k.key.as_str(), "q" | "t" | "w" | "f" | "l" | "g" | "r"))
        {
            return;
        }
        let action = if m.platform {
            match k.key.as_str() {
                "q" => Some(Action::Quit),
                "t" => Some(Action::NewTab),
                "h" if m.shift => Some(Action::Home),
                "w" => Some(Action::CloseTab),
                "r" => Some(Action::Refresh),
                "n" if m.shift => Some(Action::NewFolder),
                "c" => Some(Action::Copy),
                "x" => Some(Action::Cut),
                "v" => Some(Action::Paste),
                "d" => Some(Action::Duplicate),
                "z" => Some(Action::Undo),
                "backspace" => Some(Action::Trash),
                "i" => Some(Action::Properties),
                "1" => Some(Action::List),
                "2" => Some(Action::Grid),
                "p" if m.shift => Some(Action::Preview),
                "." if m.shift => Some(Action::Hidden),
                "up" => Some(Action::Up),
                "down" | "o" => Some(Action::Open),
                "[" => Some(Action::Back),
                "]" => Some(Action::Forward),
                "l" | "g" => {
                    self.editing_address = true;
                    let path = self.tab().path.to_string_lossy().into_owned();
                    self.address.update(cx, |input, cx| {
                        input.set(path, cx);
                        input.activate(window, cx);
                    });
                    cx.notify();
                    None
                }
                "f" => {
                    self.search
                        .update(cx, |input, cx| input.activate(window, cx));
                    None
                }
                "a" => {
                    let paths = self
                        .visible
                        .iter()
                        .map(|i| self.tab().entries[*i].path.clone())
                        .collect();
                    self.tab_mut().selected = paths;
                    self.update_preview(cx);
                    cx.notify();
                    None
                }
                _ => None,
            }
        } else if m.alt {
            match k.key.as_str() {
                "left" => Some(Action::Back),
                "right" => Some(Action::Forward),
                "up" => Some(Action::Up),
                _ => None,
            }
        } else {
            if self.address.read(cx).focus.is_focused(window)
                || self.search.read(cx).focus.is_focused(window)
            {
                return;
            }
            match k.key.as_str() {
                "f2" => Some(Action::Rename),
                "f5" => Some(Action::Refresh),
                "space" => Some(Action::QuickLook),
                "enter" => Some(Action::Open),
                "backspace" => Some(Action::Up),
                "escape" => {
                    self.menu = None;
                    if !self.tab().query.is_empty() {
                        self.clear_search(cx);
                    } else {
                        self.tab_mut().selected.clear();
                        self.update_preview(cx);
                        cx.notify();
                    }
                    None
                }
                "up" | "down" | "left" | "right" => {
                    if !self.visible.is_empty() {
                        let step = if self.settings.grid && (k.key == "up" || k.key == "down") {
                            4
                        } else {
                            1
                        };
                        let index = self.tab().cursor.unwrap_or(0).min(self.visible.len() - 1);
                        let next = if self.tab().cursor.is_none() {
                            0
                        } else if k.key == "up" || k.key == "left" {
                            index.saturating_sub(step)
                        } else {
                            (index + step).min(self.visible.len() - 1)
                        };
                        self.select(next, m, cx);
                        if !m.shift {
                            self.tab_mut().anchor = Some(next);
                        }
                        self.scroll.scroll_to_item(
                            if self.settings.grid { next / 4 } else { next },
                            ScrollStrategy::Center,
                        );
                    }
                    None
                }
                _ => None,
            }
        };
        if let Some(action) = action {
            self.perform(action, window, cx);
            cx.stop_propagation();
        }
    }
    fn toggle_tree(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.expanded.remove(&path) {
            self.expanded.insert(path.clone());
            if !self.tree.contains_key(&path) {
                let tx = self.tx.clone();
                let hidden = self.settings.hidden;
                std::thread::spawn(move || {
                    let mut entries = fs::list(&path, hidden, &AtomicBool::new(false))
                        .map(|l| l.entries)
                        .unwrap_or_default();
                    entries.retain(Entry::navigable);
                    fs::sort(&mut entries, Sort::Name, false);
                    let _ = tx.send(Message::Tree {
                        path,
                        children: entries.into_iter().map(|e| e.path).collect(),
                    });
                });
            }
        }
        cx.notify();
    }
    fn button(
        &self,
        action: Action,
        label: &str,
        glyph: &str,
        active: bool,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let p = self.palette();
        let title: SharedString = label.to_owned().into();
        div()
            .id(("action", action as usize))
            .when(action == Action::Terminal, |d| {
                d.debug_selector(|| "command-terminal".to_owned())
            })
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .px_3()
            .h(px(32.))
            .rounded_md()
            .text_size(px(12.))
            .cursor_pointer()
            .bg(rgb(if active { p.selected } else { p.bg }))
            .text_color(rgb(if active { p.accent } else { p.text }))
            .when(!enabled, |d| d.opacity(0.35).cursor_default())
            .when(enabled, |d| {
                d.hover(move |s| s.bg(rgb(p.hover))).on_click(
                    cx.listener(move |this, _, window, cx| this.perform(action, window, cx)),
                )
            })
            .child(item_icon(
                glyph,
                if active { p.accent } else { p.muted },
                16.,
            ))
            .when(!label.is_empty(), |d| d.child(title))
    }
    fn titlebar(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette();
        div()
            .flex()
            .items_center()
            .h(px(36.))
            .flex_shrink_0()
            .bg(rgb(p.surface))
            .child(div().w(px(84.)).flex_shrink_0())
            .child(item_icon("folder", 0xf4c249, 17.))
            .child(
                div()
                    .ml_2()
                    .text_size(px(12.))
                    .child(format!("File Explorer · {}", env!("CARGO_PKG_VERSION"))),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
            .child(self.button(Action::Dark, "", "sun", self.settings.dark, true, cx))
            .child(self.button(Action::Help, "", "help", false, true, cx))
            .child(div().w(px(10.)))
    }
    fn tabstrip(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette();
        div()
            .flex()
            .items_center()
            .h(px(38.))
            .gap_1()
            .px_3()
            .flex_shrink_0()
            .bg(rgb(p.sidebar))
            .border_b_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap_1()
                    .children(self.tabs.iter().enumerate().map(|(i, t)| {
                        let active = i == self.active;
                        let name = display_name(&t.path);
                        let id = t.id;
                        div()
                            .id(("tab", id))
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .w(px(220.))
                            .min_w(px(95.))
                            .h(px(34.))
                            .rounded_t_md()
                            .bg(rgb(if active { p.bg } else { p.surface }))
                            .border_1()
                            .border_color(rgb(if active { p.border } else { p.sidebar }))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(index) = this.tabs.iter().position(|t| t.id == id) {
                                    this.switch_tab(index, window, cx);
                                }
                            }))
                            .child(item_icon("folder", 0xf4c249, 17.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_size(px(12.))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .id(("close-tab", id))
                                    .size(px(20.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .hover(move |s| s.bg(rgb(p.hover)))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        if let Some(index) =
                                            this.tabs.iter().position(|t| t.id == id)
                                        {
                                            if this.tabs.len() == 1 {
                                                this.perform(Action::Quit, window, cx);
                                            } else {
                                                this.tabs[index]
                                                    .cancel
                                                    .store(true, Ordering::Relaxed);
                                                this.tabs.remove(index);
                                                if index < this.active {
                                                    this.active -= 1;
                                                }
                                                this.active = this.active.min(this.tabs.len() - 1);
                                                this.switch_tab(this.active, window, cx);
                                            }
                                        }
                                    }))
                                    .child(icon("close", p.muted, 12.)),
                            )
                    })),
            )
            .child(self.button(Action::NewTab, "", "plus", false, true, cx))
    }
    fn navigation(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette();
        let path = self.tab().path.clone();
        let mut ancestors: Vec<_> = path.ancestors().map(Path::to_path_buf).collect();
        ancestors.reverse();
        if ancestors.len() > 5 {
            ancestors = ancestors[ancestors.len() - 5..].to_vec();
        }
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .h(px(48.))
            .flex_shrink_0()
            .border_b_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.bg))
            .child(self.button(Action::Back, "", "back", false, self.tab().position > 0, cx))
            .child(self.button(
                Action::Forward,
                "",
                "forward",
                false,
                self.tab().position + 1 < self.tab().history.len(),
                cx,
            ))
            .child(self.button(Action::Up, "", "up", false, path.parent().is_some(), cx))
            .child(self.button(Action::Refresh, "", "refresh", false, true, cx))
            .child(
                div()
                    .id("address-box")
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .h(px(35.))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if self.editing_address {
                        p.accent
                    } else {
                        p.border
                    }))
                    .bg(rgb(p.surface))
                    .when(self.editing_address, |d| d.child(self.address.clone()))
                    .when(!self.editing_address, |d| {
                        d.child(
                            div()
                                .id("breadcrumbs")
                                .flex()
                                .items_center()
                                .px_3()
                                .gap_2()
                                .flex_1()
                                .overflow_hidden()
                                .children(ancestors.into_iter().enumerate().map(|(i, path)| {
                                    let name = display_name(&path);
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .when(i > 0, |d| d.child(icon("forward", p.muted, 11.)))
                                        .child(
                                            div()
                                                .id(("crumb", i))
                                                .text_size(px(12.))
                                                .whitespace_nowrap()
                                                .cursor_pointer()
                                                .hover(move |s| s.text_color(rgb(p.accent)))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.navigate(path.clone(), cx)
                                                }))
                                                .child(name),
                                        )
                                }))
                                .child(
                                    div()
                                        .id("edit-path")
                                        .flex_1()
                                        .min_w(px(16.))
                                        .h(px(30.))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.editing_address = true;
                                            let p = this.tab().path.to_string_lossy().into_owned();
                                            this.address.update(cx, |input, cx| {
                                                input.set(p, cx);
                                                input.activate(window, cx);
                                            });
                                            cx.notify();
                                        })),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .w(px(236.))
                    .h(px(35.))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.surface))
                    .pl_3()
                    .child(icon("search", p.muted, 15.))
                    .child(self.search.clone()),
            )
    }
    fn menu_button(
        &self,
        label: &str,
        glyph: &str,
        kind: MenuKind,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let p = self.palette();
        div()
            .id(SharedString::from(format!("menu-button-{label}")))
            .debug_selector({
                let label = label.to_owned();
                move || format!("command-{label}")
            })
            .flex()
            .items_center()
            .gap_2()
            .h(px(32.))
            .px_3()
            .rounded_sm()
            .cursor_pointer()
            .hover(move |d| d.bg(rgb(p.hover)))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                this.menu = Some(MenuState {
                    position: e.position() + point(px(0.), px(12.)),
                    kind,
                });
                cx.notify();
            }))
            .child(icon(glyph, p.text, 17.))
            .child(label.to_owned())
            .child(icon("down", p.muted, 10.))
    }
    fn toolbar(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette();
        let n = self.tab().selected.len();
        let idle = self.running.is_none();
        let separator = || div().h(px(22.)).w(px(1.)).bg(rgb(p.border)).mx_2();
        div()
            .flex()
            .items_center()
            .gap_1()
            .px_4()
            .h(px(48.))
            .flex_shrink_0()
            .bg(rgb(p.bg))
            .border_b_1()
            .border_color(rgb(p.border))
            .child(self.menu_button("New", "plus", MenuKind::New, cx))
            .child(separator())
            .child(self.button(Action::Cut, "", "cut", false, n > 0, cx))
            .child(self.button(Action::Copy, "", "copy", false, n > 0, cx))
            .child(self.button(Action::Paste, "", "paste", false, idle, cx))
            .child(self.button(Action::Rename, "", "rename", false, n == 1 && idle, cx))
            .child(self.button(Action::Trash, "", "trash", false, n > 0 && idle, cx))
            .child(separator())
            .child(self.menu_button("Sort", "sort", MenuKind::Sort, cx))
            .child(self.menu_button("View", "list", MenuKind::View, cx))
            .child(self.button(Action::Terminal, "Terminal", "terminal", false, true, cx))
            .child(self.button(
                Action::Undo,
                "",
                "undo",
                false,
                !self.undo.is_empty() && idle,
                cx,
            ))
            .child(
                div()
                    .id("more-menu")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(32.))
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |d| d.bg(rgb(p.hover)))
                    .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                        this.menu = Some(MenuState {
                            position: e.position(),
                            kind: if this.tab().selected.is_empty() {
                                MenuKind::Background
                            } else {
                                MenuKind::Selection
                            },
                        });
                        cx.notify();
                    }))
                    .child(icon("more", p.text, 18.)),
            )
            .child(div().flex_1())
            .child(self.button(
                Action::Preview,
                "Preview",
                "preview",
                self.settings.preview,
                true,
                cx,
            ))
    }
    fn sidebar_link(
        &self,
        label: String,
        glyph: &str,
        path: PathBuf,
        depth: usize,
        tree: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let p = self.palette();
        let active = self.tab().path == path;
        let expanded = self.expanded.contains(&path);
        let nav = path.clone();
        let toggle = path.clone();
        let drop_to = path.clone();
        let external_to = path.clone();
        div()
            .id(SharedString::from(format!(
                "sidebar-{depth}-{glyph}-{}",
                path.display()
            )))
            .flex()
            .items_center()
            .gap_2()
            .h(px(34.))
            .pl(px(12. + depth as f32 * 13.))
            .pr_2()
            .mx_2()
            .rounded_md()
            .bg(rgb(if active { p.selected } else { p.sidebar }))
            .text_color(rgb(if active { p.accent } else { p.text }))
            .text_size(px(12.))
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(if active { p.selected } else { p.hover })))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.navigate(nav.clone(), cx);
                this.focus.focus(window);
            }))
            .on_drop(cx.listener(move |this, info: &DragFiles, _, cx| {
                cx.stop_propagation();
                this.start(
                    Job::Transfer {
                        paths: info.paths.clone(),
                        destination: drop_to.clone(),
                        cut: false,
                    },
                    "Copying files",
                    cx,
                );
            }))
            .on_drop(cx.listener(move |this, info: &ExternalPaths, _, cx| {
                cx.stop_propagation();
                this.start(
                    Job::Transfer {
                        paths: info.paths().to_vec(),
                        destination: external_to.clone(),
                        cut: false,
                    },
                    "Copying files",
                    cx,
                );
            }))
            .when(tree, |d| {
                d.child(
                    div()
                        .id(SharedString::from(format!("toggle-{}", path.display())))
                        .size(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_tree(toggle.clone(), cx);
                        }))
                        .child(icon(
                            if expanded { "down" } else { "forward" },
                            p.muted,
                            10.,
                        )),
                )
            })
            .child(item_icon(
                glyph,
                if active { p.accent } else { p.muted },
                16.,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(label),
            )
            .when(active, |d| {
                d.child(div().size(px(5.)).rounded_full().bg(rgb(p.accent)))
            })
    }
    fn tree_rows(&self, path: &Path, depth: usize, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = vec![];
        if depth > 8 {
            return rows;
        }
        if self.expanded.contains(path) {
            if let Some(children) = self.tree.get(path) {
                for child in children {
                    rows.push(
                        self.sidebar_link(
                            display_name(child),
                            "folder",
                            child.clone(),
                            depth,
                            true,
                            cx,
                        )
                        .into_any_element(),
                    );
                    rows.extend(self.tree_rows(child, depth + 1, cx));
                }
            }
        }
        rows
    }
    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette();
        let home = session::home();
        let mut rows = vec![];
        for (label, glyph, path) in [
            ("Home", "home", home.clone()),
            ("Desktop", "desktop", home.join("Desktop")),
            ("Downloads", "download", home.join("Downloads")),
            ("Documents", "file", home.join("Documents")),
            ("Pictures", "image", home.join("Pictures")),
            ("Music", "music", home.join("Music")),
            ("Videos", "video", home.join("Movies")),
        ] {
            rows.push(
                self.sidebar_link(label.into(), glyph, path.clone(), 0, true, cx)
                    .into_any_element(),
            );
            rows.extend(self.tree_rows(&path, 1, cx));
        }
        let mut locations = vec![
            ("This PC".into(), "desktop", PathBuf::from("/")),
            (
                "Applications".into(),
                "grid",
                PathBuf::from("/Applications"),
            ),
            (
                "Network & volumes".into(),
                "drive",
                PathBuf::from("/Volumes"),
            ),
        ];
        let cloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
        if cloud.is_dir() {
            locations.push(("iCloud Drive".into(), "cloud", cloud));
        }
        div()
            .flex()
            .flex_col()
            .w(px(210.))
            .min_w(px(180.))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(p.sidebar))
            .border_r_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pt_3()
                    .pb_4()
                    .children(rows)
                    .when(!self.settings.favorites.is_empty(), |d| {
                        d.child(div().mx_4().my_3().h(px(1.)).bg(rgb(p.border)))
                            .children(self.settings.favorites.iter().map(|path| {
                                self.sidebar_link(
                                    display_name(path),
                                    "pin",
                                    path.clone(),
                                    0,
                                    false,
                                    cx,
                                )
                            }))
                    })
                    .child(div().mx_4().my_3().h(px(1.)).bg(rgb(p.border)))
                    .children(locations.into_iter().map(|(label, glyph, path)| {
                        self.sidebar_link(label, glyph, path, 0, true, cx)
                    }))
                    .child(self.sidebar_link(
                        "Recycle Bin".into(),
                        "trash",
                        home.join(".Trash"),
                        0,
                        false,
                        cx,
                    )),
            )
    }
    fn folder_header(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette();
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_5()
            .h(px(38.))
            .flex_shrink_0()
            .border_b_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(icon("search", p.muted, 16.))
                    .child(format!("Search results for “{}”", self.tab().query)),
            )
            .child(self.button(
                Action::Refresh,
                if self.tab().loading {
                    "Searching…"
                } else {
                    "Search again"
                },
                "refresh",
                false,
                true,
                cx,
            ))
    }
    fn column_header(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette();
        let column = |label: &'static str, sort: Sort, width: Option<f32>| {
            div()
                .id(label)
                .flex()
                .items_center()
                .gap_2()
                .h_full()
                .text_size(px(10.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(p.muted))
                .cursor_pointer()
                .when_some(width, |d, w| d.w(px(w)).flex_shrink_0())
                .when(width.is_none(), |d| d.flex_1().min_w_0())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.sort == sort {
                        this.descending = !this.descending;
                    } else {
                        this.sort = sort;
                        this.descending = sort == Sort::Modified;
                    }
                    if sort == Sort::Size {
                        this.request_sizes(
                            this.visible
                                .iter()
                                .map(|&i| &this.tab().entries[i])
                                .filter(|e| e.directory)
                                .map(|e| e.path.clone())
                                .collect(),
                            false,
                        );
                    }
                    this.rebuild(cx);
                }))
                .child(label)
                .when(self.sort == sort, |d| {
                    d.child(icon(
                        if self.descending { "down" } else { "up" },
                        p.accent,
                        10.,
                    ))
                })
        };
        div()
            .flex()
            .items_center()
            .h(px(32.))
            .px_6()
            .gap_4()
            .flex_shrink_0()
            .bg(rgb(p.surface))
            .border_y_1()
            .border_color(rgb(p.border))
            .child(column("Name", Sort::Name, None))
            .child(column("Date modified (UTC)", Sort::Modified, Some(139.)))
            .child(column("Type", Sort::Kind, Some(100.)))
            .child(column("Size", Sort::Size, Some(72.)))
    }
    fn file_row(&self, index: usize, grid: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette();
        let e = self.tab().entries[self.visible[index]].clone();
        let selected = self.tab().selected.contains(&e.path);
        let path = e.path.clone();
        let name = e.name.clone();
        let cut = self.cut && self.clipboard.contains(&path);
        let drop_to = path.clone();
        let external_to = path.clone();
        let navigable = e.navigable();
        let tab = self.tab().id;
        let pressed_entry = e.clone();
        let press_view = cx.entity().downgrade();
        let select_path = path.clone();
        let drag = DragFiles {
            paths: if selected {
                self.selected()
            } else {
                vec![path.clone()]
            },
        };
        let color = match e.icon() {
            "folder" => 0xd6a03c,
            "image" => 0x9775d8,
            "code" => 0x5a98bc,
            "archive" => 0xa27d64,
            "video" => 0xc481ad,
            _ => 0x7c99b7,
        };
        let mut row = div()
            .id(SharedString::from(format!(
                "entry-{tab}-{:?}",
                path.as_os_str()
            )))
            .when(index == 0, |row| {
                row.debug_selector(|| "first-file-row".to_owned())
            })
            .relative()
            .flex()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let bounds = bounds.intersect(&window.content_mask().bounds);
                        let press_view = press_view.clone();
                        let entry = pressed_entry.clone();
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture
                                && event.button == MouseButton::Left
                                && bounds.contains(&event.position)
                            {
                                let _ = press_view.update(cx, |this, cx| {
                                    this.press_entry(tab, &entry, event, window, cx)
                                });
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
            .rounded_md()
            .cursor_pointer()
            .overflow_hidden()
            .bg(rgb(if selected { p.selected } else { p.bg }))
            .text_color(rgb(p.text))
            .when(cut, |d| d.opacity(0.45))
            .hover(move |s| s.bg(rgb(if selected { p.selected } else { p.hover })))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                let Some(index) = this.row_index(tab, &select_path) else {
                    return;
                };
                let entry = this.tab().entries[this.visible[index]].clone();
                if this.clicks.consume_native_release(tab, &entry.path) {
                    cx.stop_propagation();
                    return;
                }
                let modifiers = event.modifiers();
                this.menu = None;
                this.focus.focus(window);
                let native_count = match event {
                    ClickEvent::Mouse(mouse) => mouse.down.click_count.max(mouse.up.click_count),
                    ClickEvent::Keyboard(_) => 2,
                };
                this.trace(&format!(
                    "row-click count={native_count} path={:?}",
                    entry.path
                ));
                // Opening takes precedence over selection toggles, including
                // when the clicked folder is already selected or part of a group.
                let activate = event.is_keyboard()
                    || this
                        .clicks
                        .complete(tab, &entry.path, Instant::now(), native_count);
                if activate {
                    this.clicks.clear();
                    this.select(index, Modifiers::none(), cx);
                    this.open_entry(&entry, cx);
                } else {
                    this.select(index, modifiers, cx);
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let Some(index) = this.row_index(tab, &path) else {
                        return;
                    };
                    this.clicks.clear();
                    this.focus.focus(window);
                    if !this.tab().selected.contains(&path) {
                        this.select(index, Modifiers::default(), cx);
                    }
                    this.menu = Some(MenuState {
                        position: event.position,
                        kind: MenuKind::Selection,
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_drag(drag, |info: &DragFiles, _, _, cx| cx.new(|_| info.clone()));
        if navigable {
            row = row
                .on_drop(cx.listener(move |this, info: &DragFiles, _, cx| {
                    cx.stop_propagation();
                    this.start(
                        Job::Transfer {
                            paths: info.paths.clone(),
                            destination: drop_to.clone(),
                            cut: false,
                        },
                        "Copying files",
                        cx,
                    );
                }))
                .on_drop(cx.listener(move |this, info: &ExternalPaths, _, cx| {
                    cx.stop_propagation();
                    this.start(
                        Job::Transfer {
                            paths: info.paths().to_vec(),
                            destination: external_to.clone(),
                            cut: false,
                        },
                        "Copying files",
                        cx,
                    );
                }));
        }
        if grid {
            row.flex_col()
                .items_center()
                .justify_center()
                .w_full()
                .h(px(138.))
                .p_3()
                .gap_2()
                .border_1()
                .border_color(rgb(if selected { p.accent } else { p.bg }))
                .child(
                    div()
                        .size(px(53.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(item_icon(e.icon(), color, 48.)),
                )
                .child(
                    div()
                        .w_full()
                        .text_center()
                        .text_size(px(12.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(name),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(p.muted))
                        .child(if e.directory {
                            format!("{} · {}", e.kind, self.size_label(&e, false))
                        } else {
                            self.size_label(&e, false)
                        }),
                )
                .into_any_element()
        } else {
            row.items_center()
                .w_full()
                .h(px(33.))
                .px_2()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .flex_1()
                        .min_w_0()
                        .child(item_icon(e.icon(), color, 20.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(12.))
                                .child(name),
                        ),
                )
                .child(
                    div()
                        .w(px(139.))
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(rgb(p.muted))
                        .child(fs::date(e.modified)),
                )
                .child(
                    div()
                        .w(px(100.))
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(rgb(p.muted))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(e.kind.clone()),
                )
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(rgb(p.muted))
                        .child(self.size_label(&e, false)),
                )
                .into_any_element()
        }
    }
    fn files(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let p = self.palette();
        let grid = self.settings.grid;
        let count = self.visible.len();
        let rows = if grid { count.div_ceil(4) } else { count };
        div()
            .id("files-area")
            .debug_selector(|| "files-area".to_owned())
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(rgb(p.bg))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.menu = Some(MenuState {
                        position: event.position,
                        kind: MenuKind::Background,
                    });
                    cx.notify();
                }),
            )
            .on_drop(cx.listener(|this, info: &DragFiles, _, cx| {
                this.start(
                    Job::Transfer {
                        paths: info.paths.clone(),
                        destination: this.tab().path.clone(),
                        cut: false,
                    },
                    "Copying files",
                    cx,
                );
            }))
            .on_drop(cx.listener(|this, info: &ExternalPaths, _, cx| {
                this.start(
                    Job::Transfer {
                        paths: info.paths().to_vec(),
                        destination: this.tab().path.clone(),
                        cut: false,
                    },
                    "Copying files",
                    cx,
                );
            }))
            .when(!self.tab().query.is_empty(), |d| {
                d.child(self.folder_header(cx))
            })
            .when(!grid, |d| d.child(self.column_header(cx)))
            .when(count > 0, |d| {
                d.child(
                    uniform_list(
                        "file-list",
                        rows,
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|row| {
                                    if grid {
                                        div()
                                            .flex()
                                            .gap_3()
                                            .px_4()
                                            .py_2()
                                            .w_full()
                                            .h(px(154.))
                                            .children(
                                                (row * 4..((row + 1) * 4).min(this.visible.len()))
                                                    .map(|i| {
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .child(this.file_row(i, true, cx))
                                                    }),
                                            )
                                            .children(
                                                (0..(4
                                                    - (((row + 1) * 4).min(this.visible.len())
                                                        - row * 4)))
                                                    .map(|_| div().flex_1()),
                                            )
                                            .into_any_element()
                                    } else {
                                        div()
                                            .px_4()
                                            .w_full()
                                            .h(px(33.))
                                            .child(this.file_row(row, false, cx))
                                            .into_any_element()
                                    }
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(self.scroll.clone())
                    .flex_1()
                    .min_h_0()
                    .w_full(),
                )
            })
            .when(count == 0, |d| {
                d.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .pb_12()
                        .child(icon(
                            if self.tab().loading {
                                "refresh"
                            } else if self.tab().query.is_empty() {
                                "folder"
                            } else {
                                "search"
                            },
                            p.muted,
                            40.,
                        ))
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(if self.tab().loading {
                                    "Reading files…"
                                } else if self.tab().issue.is_some() {
                                    "This folder could not be read"
                                } else if self.tab().query.is_empty() {
                                    "A little room for something new"
                                } else {
                                    "No matching files"
                                }),
                        )
                        .child(div().text_size(px(12.)).text_color(rgb(p.muted)).child(
                            if self.tab().loading {
                                "Large folders may take a moment."
                            } else if self.tab().query.is_empty() {
                                "Create a folder or drop files here."
                            } else {
                                "Try a different name or press Enter to search subfolders."
                            },
                        ))
                        .when(self.tab().query.is_empty() && !self.tab().loading, |d| {
                            d.child(self.button(
                                Action::NewFolder,
                                "Create folder",
                                "plus",
                                false,
                                self.running.is_none(),
                                cx,
                            ))
                        }),
                )
            })
    }
    fn preview(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette();
        let entries = self.selected_entries();
        let entry = if entries.len() == 1 {
            entries.first().cloned()
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .w(px(270.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(p.surface))
            .border_l_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("DETAILS & PREVIEW"),
                    )
                    .child(self.button(Action::Preview, "", "close", false, true, cx)),
            )
            .child(
                div()
                    .id("preview-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_5()
                    .when(entry.is_none(), |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .pt_12()
                                .gap_4()
                                .child(icon("preview", p.muted, 40.))
                                .child(
                                    div()
                                        .text_center()
                                        .text_size(px(12.))
                                        .text_color(rgb(p.muted))
                                        .child(if entries.is_empty() {
                                            "Select a file to see its details.".into()
                                        } else {
                                            format!("{} items selected", entries.len())
                                        }),
                                ),
                        )
                    })
                    .when_some(entry, |d, e| {
                        let image = e.kind == "Image"
                            && e.path.extension().is_some_and(|s| {
                                ["png", "jpg", "jpeg", "gif", "webp"]
                                    .iter()
                                    .any(|ext| s.eq_ignore_ascii_case(ext))
                            });
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w_full()
                                .h(px(156.))
                                .rounded_lg()
                                .bg(rgb(p.bg))
                                .border_1()
                                .border_color(rgb(p.border))
                                .when(image, |d| {
                                    d.child(
                                        img(e.path.clone())
                                            .object_fit(ObjectFit::Contain)
                                            .max_w(px(218.))
                                            .max_h(px(148.)),
                                    )
                                })
                                .when(!image, |d| {
                                    d.child(icon(
                                        e.icon(),
                                        if e.directory { 0xd6a03c } else { p.accent },
                                        64.,
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .mt_4()
                                .text_size(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(e.name.clone()),
                        )
                        .child(
                            div()
                                .mt_1()
                                .mb_5()
                                .text_size(px(11.))
                                .text_color(rgb(p.muted))
                                .child(e.kind.clone()),
                        )
                        .child(detail("Size", self.size_label(&e, true), p))
                        .child(detail("Modified (UTC)", fs::date(e.modified), p))
                        .when_some(e.created, |d, date| {
                            d.child(detail("Created (UTC)", fs::date(date), p))
                        })
                        .child(detail(
                            "Permissions",
                            format!(
                                "{:o}{}",
                                e.mode & 0o777,
                                if e.readonly { " · read only" } else { "" }
                            ),
                            p,
                        ))
                        .child(
                            div()
                                .mt_3()
                                .text_size(px(10.))
                                .text_color(rgb(p.muted))
                                .child("LOCATION"),
                        )
                        .child(
                            div().mt_1().text_size(px(11.)).child(
                                e.path
                                    .parent()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                            ),
                        )
                        .child(
                            div()
                                .mt_4()
                                .flex()
                                .gap_2()
                                .child(self.button(
                                    Action::QuickLook,
                                    "Quick Look",
                                    "eye",
                                    false,
                                    true,
                                    cx,
                                ))
                                .child(self.button(Action::Reveal, "", "folder", false, true, cx)),
                        )
                        .when(
                            !e.directory && explorer::media::kind(&e.path).is_some(),
                            |d| {
                                d.child(div().mt_3().child(self.button(
                                    Action::Open,
                                    "Open viewer",
                                    e.icon(),
                                    false,
                                    true,
                                    cx,
                                )))
                            },
                        )
                        .when(
                            !e.directory && !image && explorer::media::kind(&e.path).is_none(),
                            |d| {
                                d.child(
                                    div()
                                        .mt_5()
                                        .border_t_1()
                                        .border_color(rgb(p.border))
                                        .pt_4()
                                        .child(
                                            div()
                                                .mb_3()
                                                .text_size(px(10.))
                                                .text_color(rgb(p.muted))
                                                .child("TEXT PREVIEW"),
                                        )
                                        .child(
                                            div().text_size(px(11.)).font_family("Menlo").child(
                                                if self.preview_body.is_empty() {
                                                    "Loading preview…".into()
                                                } else {
                                                    self.preview_body.clone()
                                                },
                                            ),
                                        ),
                                )
                            },
                        )
                    }),
            )
    }
    fn status(&self) -> Div {
        let p = self.palette();
        let count = self.tab().selected.len();
        let bytes: u64 = self
            .selected_entries()
            .iter()
            .filter_map(|e| self.entry_bytes(e))
            .fold(0u64, u64::saturating_add);
        let unknown = self
            .selected_entries()
            .iter()
            .filter(|e| self.entry_bytes(e).is_none())
            .count();
        div()
            .flex()
            .items_center()
            .gap_3()
            .h(px(26.))
            .px_4()
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.surface))
            .text_size(px(10.))
            .text_color(rgb(p.muted))
            .child(format!("{} items", self.visible.len()))
            .when(count > 0, |d| {
                d.child(div().w(px(1.)).h(px(12.)).bg(rgb(p.border)))
                    .child(format!(
                        "{count} selected · {}{}",
                        fs::human_size(bytes),
                        if unknown > 0 {
                            format!(" + {unknown} unmeasured")
                        } else {
                            String::new()
                        }
                    ))
            })
            .child(div().flex_1())
            .when(self.tab().loading, |d| d.child("Searching…"))
            .when(self.size_worker.is_some(), |d| {
                d.child("Calculating folder sizes…")
            })
            .when(self.settings.hidden, |d| d.child("Hidden files visible"))
            .child(if self.settings.grid {
                "Large icons"
            } else {
                "Details"
            })
    }
    fn context_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let p = self.palette();
        let n = self.tab().selected.len();
        let idle = self.running.is_none();
        let items: Vec<(Action, &str, &str, &str, bool)> = match menu.kind {
            MenuKind::New => vec![
                (Action::NewFolder, "Folder", "folder", "⇧⌘N", idle),
                (Action::NewFile, "Text document", "file", "", idle),
            ],
            MenuKind::Sort => vec![
                (Action::SortName, "Name", "file", "", true),
                (Action::SortModified, "Date modified", "refresh", "", true),
                (Action::SortKind, "Type", "grid", "", true),
                (Action::SortSize, "Size", "drive", "", true),
                (
                    Action::SortDirection,
                    if self.descending {
                        "Ascending"
                    } else {
                        "Descending"
                    },
                    "sort",
                    "",
                    true,
                ),
            ],
            MenuKind::View => vec![
                (
                    Action::FolderSizes,
                    "Calculate all folder sizes",
                    "drive",
                    "",
                    true,
                ),
                (
                    Action::CancelSizes,
                    "Cancel size calculation",
                    "close",
                    "",
                    self.size_worker.is_some(),
                ),
                (Action::Grid, "Large icons", "grid", "⌘2", true),
                (Action::List, "Details", "list", "⌘1", true),
                (
                    Action::Hidden,
                    if self.settings.hidden {
                        "Hide hidden items"
                    } else {
                        "Show hidden items"
                    },
                    "eye",
                    "⇧⌘.",
                    true,
                ),
                (
                    Action::Preview,
                    if self.settings.preview {
                        "Hide preview pane"
                    } else {
                        "Show preview pane"
                    },
                    "preview",
                    "⇧⌘P",
                    true,
                ),
                (
                    Action::Dark,
                    if self.settings.dark {
                        "Light theme"
                    } else {
                        "Dark theme"
                    },
                    "sun",
                    "",
                    true,
                ),
            ],
            MenuKind::Background => vec![
                (Action::NewFolder, "New folder", "plus", "⇧⌘N", idle),
                (Action::NewFile, "New file", "file", "", idle),
                (Action::Paste, "Paste", "paste", "⌘V", idle),
                (
                    Action::Undo,
                    "Undo last operation",
                    "undo",
                    "⌘Z",
                    idle && !self.undo.is_empty(),
                ),
                (Action::Pin, "Pin / unpin folder", "pin", "", true),
                (Action::Terminal, "Open in Terminal", "terminal", "", true),
                (Action::Reveal, "Show in Finder", "folder", "", true),
                (Action::Properties, "Folder properties", "info", "⌘I", true),
            ],
            MenuKind::Selection => vec![
                (Action::Open, "Open", "folder", "↵", n > 0),
                (
                    Action::OpenExternal,
                    "Open externally",
                    "forward",
                    "",
                    n > 0,
                ),
                (Action::QuickLook, "Quick Look", "eye", "Space", n > 0),
                (Action::Terminal, "Open in Terminal", "terminal", "", true),
                (Action::Copy, "Copy", "copy", "⌘C", n > 0),
                (Action::Cut, "Cut", "cut", "⌘X", n > 0),
                (Action::Paste, "Paste", "paste", "⌘V", idle),
                (Action::Rename, "Rename", "rename", "F2", n == 1 && idle),
                (Action::Duplicate, "Duplicate", "copy", "⌘D", n > 0 && idle),
                (
                    Action::Compress,
                    "Compress to ZIP",
                    "archive",
                    "",
                    n > 0 && idle,
                ),
                (
                    Action::Extract,
                    "Extract ZIP",
                    "archive",
                    "",
                    self.selected_entries().iter().any(|e| {
                        e.path
                            .extension()
                            .is_some_and(|x| x.eq_ignore_ascii_case("zip"))
                    }),
                ),
                (Action::Pin, "Pin / unpin folder", "pin", "", true),
                (Action::Reveal, "Show in Finder", "folder", "", n > 0),
                (Action::Properties, "Properties", "info", "⌘I", true),
                (Action::Trash, "Move to Trash", "trash", "⌘⌫", n > 0 && idle),
            ],
        };
        let height = items.len() as f32 * 32. + 12.;
        let size = window.viewport_size();
        let x = menu.position.x.min(size.width - px(254.)).max(px(4.));
        let y = menu.position.y.min(size.height - px(height)).max(px(4.));
        Some(
            div()
                .id("context-menu")
                .absolute()
                .occlude()
                .left(x)
                .top(y)
                .w(px(250.))
                .py_1()
                .rounded_lg()
                .bg(rgb(p.bg))
                .shadow_lg()
                .border_1()
                .border_color(rgb(p.border))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                }))
                .children(
                    items
                        .into_iter()
                        .map(|(action, label, glyph, shortcut, enabled)| {
                            div()
                                .id(("menu", action as usize))
                                .when(action == Action::NewFolder, |d| {
                                    d.debug_selector(|| "menu-new-folder".to_owned())
                                })
                                .when(action == Action::Grid, |d| {
                                    d.debug_selector(|| "menu-large-icons".to_owned())
                                })
                                .when(action == Action::SortModified, |d| {
                                    d.debug_selector(|| "menu-sort-modified".to_owned())
                                })
                                .when(action == Action::SortSize, |d| {
                                    d.debug_selector(|| "menu-sort-size".to_owned())
                                })
                                .when(action == Action::Terminal, |d| {
                                    d.debug_selector(|| "menu-terminal".to_owned())
                                })
                                .when(action == Action::FolderSizes, |d| {
                                    d.debug_selector(|| "menu-folder-sizes".to_owned())
                                })
                                .flex()
                                .items_center()
                                .gap_3()
                                .h(px(32.))
                                .mx_1()
                                .px_3()
                                .rounded_md()
                                .text_size(px(12.))
                                .cursor_pointer()
                                .text_color(rgb(if action == Action::Trash {
                                    0xd76868
                                } else {
                                    p.text
                                }))
                                .when(!enabled, |d| d.opacity(0.35).cursor_default())
                                .when(enabled, |d| {
                                    d.hover(move |s| s.bg(rgb(p.hover))).on_click(cx.listener(
                                        move |this, _, window, cx| this.perform(action, window, cx),
                                    ))
                                })
                                .child(icon(glyph, p.muted, 15.))
                                .child(label)
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(rgb(p.muted))
                                        .child(shortcut),
                                )
                        }),
                )
                .into_any_element(),
        )
    }
    fn modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.modal.as_ref()?;
        let p = self.palette();
        let (title, subtitle, confirm) = match modal {
            Modal::Create {
                directory: true, ..
            } => (
                "New folder",
                "Give your new folder a name.",
                "Create folder",
            ),
            Modal::Create {
                directory: false, ..
            } => (
                "New file",
                "Create an empty file in this folder.",
                "Create file",
            ),
            Modal::Rename(_) => (
                "Rename item",
                "Existing files will never be overwritten.",
                "Rename",
            ),
            Modal::Trash(_) => (
                "Move to Trash?",
                "You can undo this operation or restore the items from Trash.",
                "Move to Trash",
            ),
            Modal::Properties(_) => ("Properties", "File information on this Mac.", "Done"),
            Modal::Help => (
                "Make yourself at home",
                concat!(
                    "Explorer for macOS · v",
                    env!("CARGO_PKG_VERSION"),
                    " · Rust + GPUI"
                ),
                "Done",
            ),
        };
        let input = matches!(modal, Modal::Create { .. } | Modal::Rename(_));
        let mut content = div().flex().flex_col().gap_3();
        match modal {
            Modal::Create { parent, .. } => {
                content = content.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(p.muted))
                        .child(parent.to_string_lossy().into_owned()),
                );
            }
            Modal::Rename(path) => {
                content = content.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(p.muted))
                        .child(path.to_string_lossy().into_owned()),
                );
            }
            Modal::Trash(paths) => {
                content = content
                    .children(paths.iter().take(6).map(|path| {
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(icon("file", p.muted, 16.))
                            .child(
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            )
                    }))
                    .when(paths.len() > 6, |d| {
                        d.child(format!("and {} more items", paths.len() - 6))
                    });
            }
            Modal::Properties(entries) => {
                if entries.len() == 1 {
                    let e = &entries[0];
                    content = content
                        .child(detail("Name", e.name.clone(), p))
                        .child(detail("Kind", e.kind.clone(), p))
                        .child(detail("Location", e.path.to_string_lossy().into_owned(), p))
                        .child(detail("Size", self.size_label(e, true), p))
                        .child(detail("Modified (UTC)", fs::date(e.modified), p))
                        .when_some(e.created, |d, t| {
                            d.child(detail("Created (UTC)", fs::date(t), p))
                        })
                        .child(detail("Permissions", format!("{:o}", e.mode & 0o777), p));
                    if e.symlink {
                        content = content.child(detail(
                            "Link target",
                            std::fs::read_link(&e.path)
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_else(|_| "Unavailable".into()),
                            p,
                        ));
                    }
                } else {
                    content = content
                        .child(detail("Items", entries.len().to_string(), p))
                        .child(detail(
                            "Total size",
                            format!(
                                "{} · {} unmeasured",
                                fs::human_size(
                                    entries
                                        .iter()
                                        .filter_map(|e| self.entry_bytes(e))
                                        .fold(0u64, u64::saturating_add)
                                ),
                                entries
                                    .iter()
                                    .filter(|e| self.entry_bytes(e).is_none())
                                    .count()
                            ),
                            p,
                        ));
                }
                if entries.iter().any(|entry| entry.directory) {
                    content = content.child(
                        div()
                            .mt_3()
                            .flex()
                            .gap_2()
                            .child(self.button(
                                Action::FolderSizes,
                                "Recalculate size",
                                "drive",
                                false,
                                self.size_worker.is_none(),
                                cx,
                            ))
                            .when(self.size_worker.is_some(), |d| {
                                d.child(self.button(
                                    Action::CancelSizes,
                                    "Cancel",
                                    "close",
                                    false,
                                    true,
                                    cx,
                                ))
                            }),
                    );
                }
            }
            Modal::Help => {
                content=content.children([
                ("New / close tab","⌘T / ⌘W"),("Go to path / search","⌘L / ⌘F"),("Search subfolders","Enter in search field"),
                ("Back / forward / up","⌘[ / ⌘] / ⌘↑"),("Open / Quick Look","Enter / Space"),("Copy / cut / paste","⌘C / ⌘X / ⌘V"),
                ("New folder / rename","⇧⌘N / F2"),("Duplicate / undo","⌘D / ⌘Z"),("Move to Trash","⌘Backspace"),
                ("Select multiple / range","⌘Click / ⇧Click"),("List / grid view","⌘1 / ⌘2"),("Hidden files / preview","⇧⌘. / ⇧⌘P"),
                ("Properties / refresh","⌘I / ⌘R"),
            ].into_iter().map(|(label,keys)|detail(label,keys.into(),p))).child(div().mt_2().text_size(px(11.)).text_color(rgb(p.muted)).child("Drag files onto folders to copy them. You can also drop files from Finder. Name conflicts receive a new name automatically."));
            }
        }
        Some(
            div()
                .absolute()
                .occlude()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x0c142b65))
                .child(
                    div()
                        .id("modal")
                        .w(px(500.))
                        .max_h(px(640.))
                        .overflow_y_scroll()
                        .rounded_xl()
                        .shadow_lg()
                        .bg(rgb(p.bg))
                        .border_1()
                        .border_color(rgb(p.border))
                        .p_6()
                        .child(
                            div()
                                .text_size(px(21.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .mt_2()
                                .mb_5()
                                .text_size(px(12.))
                                .text_color(rgb(p.muted))
                                .child(subtitle),
                        )
                        .child(content)
                        .when(input, |d| {
                            d.child(
                                div()
                                    .mt_4()
                                    .border_1()
                                    .border_color(rgb(p.accent))
                                    .rounded_md()
                                    .bg(rgb(p.surface))
                                    .child(self.field.clone()),
                            )
                        })
                        .when_some(self.modal_error.clone(), |d, error| {
                            d.child(
                                div()
                                    .mt_3()
                                    .text_size(px(12.))
                                    .text_color(rgb(0xd76868))
                                    .child(error),
                            )
                        })
                        .child(
                            div()
                                .mt_6()
                                .flex()
                                .justify_end()
                                .gap_3()
                                .when(!matches!(modal, Modal::Properties(_) | Modal::Help), |d| {
                                    d.child(
                                        div()
                                            .id("modal-cancel")
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .bg(rgb(p.surface))
                                            .border_1()
                                            .border_color(rgb(p.border))
                                            .child("Cancel")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_modal(window, cx)
                                            })),
                                    )
                                })
                                .child(
                                    div()
                                        .id("modal-submit")
                                        .px_4()
                                        .py_2()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .text_color(rgb(0xffffff))
                                        .bg(rgb(if matches!(modal, Modal::Trash(_)) {
                                            0xc75959
                                        } else {
                                            p.accent
                                        }))
                                        .child(confirm)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.submit_modal(window, cx)
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
impl Focusable for Explorer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for Explorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let pointer_view = cx.entity().downgrade();
        div()
            .id("explorer")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_w(px(940.))
            .bg(rgb(p.bg))
            .text_color(rgb(p.text))
            .text_size(px(13.))
            .font_family(".AppleSystemUIFont")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|this, _: &crate::Quit, window, cx| {
                this.perform(Action::Quit, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NewTab, window, cx| {
                this.perform(Action::NewTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NewFolder, window, cx| {
                this.perform(Action::NewFolder, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::CopyFiles, window, cx| {
                this.perform(Action::Copy, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::PasteFiles, window, cx| {
                this.perform(Action::Paste, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::UndoFiles, window, cx| {
                this.perform(Action::Undo, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::ShowHelp, window, cx| {
                this.perform(Action::Help, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::RefreshFiles, window, cx| {
                this.perform(Action::Refresh, window, cx)
            }))
            // Capture movement before GPUI's drag handler (its default slop is
            // only two pixels). This canvas paints nothing and adds no hitbox.
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let down_view = pointer_view.clone();
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = down_view.update(cx, |this, cx| this.trace(&format!("root-down button={:?} count={} pos={:?} modifiers={:?} drag={}", event.button, event.click_count, event.position, event.modifiers, cx.has_active_drag())));
                            }
                        });
                        let motion_view = pointer_view.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture
                                && event.pressed_button == Some(MouseButton::Left)
                            {
                                let suppress = motion_view
                                    .update(cx, |this, _| {
                                        this.clicks.suppress_drag_motion((
                                            f32::from(event.position.x),
                                            f32::from(event.position.y),
                                        ))
                                    })
                                    .unwrap_or(false);
                                if suppress {
                                    cx.stop_propagation();
                                }
                            }
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture && event.button == MouseButton::Left
                            {
                                let _ = pointer_view.update(cx, |this, _| this.clicks.release());
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
            .child(self.titlebar(cx))
            .child(self.tabstrip(cx))
            .child(self.navigation(cx))
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.sidebar(cx))
                    .child(self.files(cx))
                    .when(self.settings.preview, |d| d.child(self.preview(cx))),
            )
            .when_some(self.tab().issue.clone(), |d, issue| {
                d.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_size(px(11.))
                        .text_color(rgb(0xb47c38))
                        .bg(rgb(p.surface))
                        .child(issue),
                )
            })
            .when_some(self.notice.clone(), |d, (text, error)| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_2()
                        .border_t_1()
                        .border_color(rgb(p.border))
                        .bg(rgb(p.surface))
                        .child(icon(
                            if error { "info" } else { "check" },
                            if error { 0xd76868 } else { 0x58af8c },
                            15.,
                        ))
                        .child(
                            div()
                                .id("notice-text")
                                .flex_1()
                                .max_h(px(90.))
                                .overflow_y_scroll()
                                .text_size(px(11.))
                                .text_color(rgb(if error { 0xd76868 } else { p.muted }))
                                .child(text),
                        )
                        .child(
                            div()
                                .id("dismiss-notice")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.notice = None;
                                    cx.notify();
                                }))
                                .child(icon("close", p.muted, 14.)),
                        ),
                )
            })
            .when_some(self.running.as_ref(), |d, r| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_2()
                        .bg(rgb(p.selected))
                        .child(icon("copy", p.accent, 16.))
                        .child(format!(
                            "{} · {} transferred",
                            r.label,
                            fs::human_size(r.bytes)
                        ))
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("cancel-operation")
                                .cursor_pointer()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(p.bg))
                                .child("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(r) = &mut this.running {
                                        r.cancel.store(true, Ordering::Relaxed);
                                        r.label = "Cancelling after the current step…".into();
                                    }
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(self.status())
            .when_some(self.context_menu(window, cx), |d, menu| d.child(menu))
            .when_some(self.modal(cx), |d, modal| d.child(modal))
    }
}
impl Drop for Explorer {
    fn drop(&mut self) {
        self.cancel_sizes();
        for tab in &self.tabs {
            tab.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(mut r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
            if let Some(worker) = r.worker.take() {
                let _ = worker.join();
            }
        }
        self.save();
    }
}
fn display_name(path: &Path) -> String {
    if path == session::home() {
        "Home".into()
    } else if path == Path::new("/") {
        "Macintosh HD".into()
    } else {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
}
fn detail(label: &str, value: String, p: Palette) -> Div {
    div()
        .flex()
        .justify_between()
        .gap_4()
        .py_2()
        .border_b_1()
        .border_color(rgb(p.border))
        .text_size(px(11.))
        .child(
            div()
                .w(px(108.))
                .flex_shrink_0()
                .text_color(rgb(p.muted))
                .child(label.to_owned()),
        )
        .child(div().flex_1().min_w_0().child(value))
}
