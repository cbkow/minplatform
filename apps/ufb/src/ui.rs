//! One window's content (canon row 463's layout): the sidebar (Favorites, Shares, Jobs, All Tracked, the system's
//! places), ONE toolbar for the window that acts on the active pane (back, forward, up, the path bar, list / grid, the
//! grid's size, search, the second pane), one or two panes, the status bar. Drawn with the family's toolkit as
//! minRender's and minNotes' windows are: one vello scene over the presenter, the theme's tokens, min-ui's parts.
//!
//! Nothing here asks the file system for anything: the panes and the sidebar are fed by worker threads through one
//! channel (`lister::Port`), which also wakes the loop.
use crate::crumbs;
use crate::icons::Icons;
use crate::jobview::Job;
use crate::thumbs::Thumbs;
use crate::lister::{self, Msg, Port};
use crate::menu::{self, Cmd, InWindowMenu};
use crate::model::SortBy;
use crate::pane::{Load, Mode, Pane};
use crate::places::{self, Place};
use kurbo::{Affine, Rect};
use min_present::{Output, Presenter};
use min_theme::tokens as t;
use min_ui::{action_button::Response, fonts::draw_layout, grid, paint, section, tooltip::Tooltips, ActionButton, Button, Corners, Event, Family, FieldResponse, Fonts, Key, KeyEvent, Mods, Named, ScrollBar, Section, Slider, TextField, UiLayer};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};
use vello::Scene;
use winit31::window::Window;

/// What the window asks of the application.
#[derive(Clone, Debug, PartialEq)]
pub enum Out { /// the clipboard is the application's to reach: files to put on it (`cut`: they are moved when pasted); a paste of files asked for
    /// (`mv`: "Move Item Here"); the text field in hand copies (`cut`) or pastes
    /// the system's drag of these files begins (a press on them was moved)
    Drag(Vec<PathBuf>),
    /// a file to hand to the system: it opens in its default application; or in the application named
    OpenDefault(PathBuf), OpenWith(PathBuf, PathBuf),
    /// the system's "Open with" chooser; these shown in the system's file browser; Explorer's own menu for these, at a point (Windows)
    OpenAs(PathBuf), Reveal(Vec<PathBuf>), ShellMenu((f64, f64), Vec<PathBuf>),
    CopyFiles { paths: Vec<PathBuf>, cut: bool }, PasteFiles { mv: bool }, FieldCopy { cut: bool }, FieldPaste, /// a text for the clipboard (a path, a name, a link)
    CopyText(String),
    NewWindow(PathBuf), Close, Minimize, Quit, /// a context menu to show (the application's loop does: it is the system's)
    Menu(crate::ctxmenu::Ask) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus { Pane, Path, Search, Rename, /// a cell of a job's item list is being typed into
    Cell, /// the item list of a job (the arrow keys choose an item)
    Items }
/// A row of the drawn context menu.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CtxRow { Ours(u32), Open, OpenAs, Reveal, More, Rule }
/// A name being changed in place (row 437): the entry, its field over the name.
struct Renaming { pane: usize, path: PathBuf, dir: bool, field: TextField }
/// What an open question is about.
enum Asked { /// delete these for good
    Delete(Vec<PathBuf>), /// the extension would change: the entry, the name typed, the name with the old extension kept
    Extension { path: PathBuf, typed: String, kept: String }, /// the engine's question about a name that is taken (its number)
    Conflict(u64), /// a new job in this jobs folder (its number, its name); a new item in this section's folder
    NewJob(PathBuf), AddItem(PathBuf), /// a web link's file, a minNotes note, in this folder (rows 585, 587)
    NewLink(PathBuf), NewNote(PathBuf), /// 1.4.2's "New UFB Folder" in this folder (row 590)
    NewDated(PathBuf), /// a share to add (its address, or a folder); a share's name
    AddShare, RenameShare(usize), ShareLetter(usize), /// quit although tasks run or wait
    Quit, /// several entries to rename in one step (each: its path, whether it is a folder)
    BatchRename(Vec<(PathBuf, bool)>), /// the password of this archive
    Password(PathBuf), /// sync's settings: the farm's folder, this machine's name, the belt
    SyncSet }
/// What was made off the window's thread (a job's folders, an item's skeleton): the path, or what went wrong.
/// What a thread that looked at the shares, or asked the system to connect one, found.
enum ShareNews { /// how each stands (by its id)
    States(Vec<(String, crate::shares::State)>), /// the answer to a request to connect (`asked`: by the user - a failure is said)
    Connected(String, crate::shares::State, bool, bool), /// the stale placeholders found (by share), each look; a Fix ended
    Holders(Vec<(String, PathBuf)>), Fixed(String, Result<(), String>) }
enum Made { /// a `union:` link's target was looked for (off the window's thread: a share may be slow): it is shown in the file manager, or said to be gone
    Shown(PathBuf, bool), /// a folder was chosen for a tracked item whose own is gone (none: the chooser was left)
    Relink(PathBuf, Option<PathBuf>) }
/// A notes or links cell open for writing: several lines, in a card of its own by the cell.
struct NoteEdit { item: PathBuf, col: usize, title: String, area: min_ui::text_area::TextArea, card: Rect, at: (f64, f64) }
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag { Sidebar(f64, f64), Split, /// the item list's edge in a job (the pointer's x and the width at the press)
    JobList(f64, f64) }
/// A tab that is NOT in front (row 480): what the window holds for it until it is shown again - its panes (with their
/// folders, histories, choices and views), which one is active, whether the second is shown, and the divider. The tab
/// in front lives in the window's own fields; its place in the list is empty meanwhile.
#[derive(Default)]
struct Tab { panes: Vec<Pane>, active: usize, two: bool, split: f64, /// the tab shows a job (row 482): its sections and items beside the panes
    job: Option<Job>, /// ... or the tracker: one job's tracked items, or every subscribed job's (row 491)
    tracker: Option<crate::tracker::Tracker> }
struct SideRow { rect: Rect, path: Option<PathBuf>, enabled: bool, /// a subscribed job: it opens as a job, not as a folder
    job: bool, /// All Tracked: it opens the tracker of every subscribed job
    all: bool, /// a saved share: its place in the list (it has a menu of its own, connected or not)
    share: Option<usize> }

/// Logical px.
/// The rows the tasks' panel shows at a time.
const TASK_ROWS: usize = 8;
const LB_STRIP: f64 = 34.0; const LB_WEB_TOP: f64 = 52.0; const LB_BAR: f64 = 46.0; const TOOLBAR: f64 = 44.0; const JOB_BAR: f64 = 40.0; const JOB_LIST: f64 = 380.0; const JOB_LIST_MIN: f64 = 200.0; const TABS: f64 = 36.0; const TAB_PILL: f64 = 26.0; const TAB_MAX: f64 = 220.0; const STATUS: f64 = 26.0; const HEAD: f64 = 26.0; const SIDE_ROW: f64 = 28.0; const SIDE_MIN: f64 = 150.0; const SIDE_MAX: f64 = 420.0; const GAP: f64 = 5.0;

/// A new pane shows what the settings say (row 516): hidden files, folders first, sequences as one entry, a grid.
fn seed(p: &mut Pane, view: (bool, bool, bool, bool)) { p.set_hidden(view.0); p.set_folders_first(view.1); if !p.has_own_view() { p.set_sequences(view.2); if view.3 { p.set_mode(Mode::Grid); } } p.view_synced(); }
// (the teardown's order is QCView's: the player - its decoders hold hardware frames -, then the hardware device context
// WHILE the GPU device is alive, then the presenter with the rest. Left to the fields' order, the presenter went
// first and Windows ended a session that had previewed a video with a fault inside Direct3D.)
impl Drop for Ui { fn drop(&mut self) { crate::ops::unsubscribe(self.id); self.lightbox.close();
    // (a player let go is ended on threads of its own: they are waited for, or they release their frames while Direct3D is being unloaded)
    qcv_player::wait_retired(std::time::Duration::from_secs(3)); self.hw = None; } }
pub struct Ui {
    window: Arc<Box<dyn Window>>, presenter: Presenter, layer: UiLayer, fonts: Fonts, tips: Tooltips,
    port: Port, rx: Receiver<Msg>, icons: Icons, thumbs: Thumbs,
    /// one pane, or two side by side; the one the toolbar, the sidebar and the menus act on; the first pane's share of the width
    panes: Vec<Pane>, active: usize, two: bool, split: f64, pane_heads: [Rect; 2],
    /// the tabs (row 480): one a window at first, and then no strip is shown. `tab`: the one in front (its entry here is
    /// empty: it lives in the fields above). The strip as drawn: each tab's place and close button, the "+" at its end.
    tabs: Vec<Tab>, tab: usize, tab_rects: Vec<Rect>, tab_close: Vec<ActionButton>, tab_new: ActionButton,
    /// the project side (row 482). The job the tab in front shows, if it shows one; the subscribed jobs (the sidebar's
    /// Jobs); the job's bar - a button a section, made for these names -, its refresh, the item list's Add; the list as
    /// drawn: its place, its rows, its scroll bar, the row under the pointer; where the panes begin; what is being made
    job: Option<Job>, jobs: Vec<crate::db::Sub>, job_secs: Vec<ActionButton>, job_secs_for: Vec<String>, job_refresh: ActionButton, job_add: ActionButton,
    job_list: Rect, panes_x0: f64, /// the item list itself (row 489): its columns, what the items carry, its field; the foot's Columns button; what its last menu was about (the column in view, the item, the columns that can be shown again, those in the trash)
    /// the chips (row 490): the popover that picks a dropdown cell's value; a column's editor (a question of its own, over everything)
    chip: Option<crate::chips::ChipPicker>, coledit: Option<crate::chips::ColumnEditor>, /// the Settings card (row 516)
    prefs: Option<crate::prefs::Prefs>, /// a notes or links cell being written in its own small editor (row 529)
    note: Option<NoteEdit>, /// the favourites the build was started with (`--favorite`, `--jobs`): the user's own come after them
    started_favorites: Vec<Place>, /// what the rows of the tracker's menu for several would set: (the column in view, the value)
    bulk: Vec<(usize, serde_json::Value)>,
    /// the tracker the tab in front shows, if it shows one (row 491): a job's (beside its sections), or All Tracked;
    /// its button in a job's bar, the Due chips, its last press (two on one row open the item)
    /// the board (row 492): the section in front shown as lanes of cards in the item list's place; what each (job, section) is grouped by, while the window lives; its button at the list's foot; the columns its last menu offered
    board: Option<crate::board::Board>, board_of: std::collections::HashMap<(PathBuf, String), crate::board::LaneBy>, board_btn: ActionButton, board_ctx: Vec<crate::board::LaneBy>,
    tracker: Option<crate::tracker::Tracker>, trk_btn: ActionButton, due_btns: Vec<ActionButton>, trk_press: Option<(Instant, usize)>,
    items: crate::itemlist::ItemList, job_meta: ActionButton, meta_ctx: (Option<usize>, Option<usize>, Vec<String>, Vec<(i64, String)>), made_tx: std::sync::mpsc::Sender<Made>, made_rx: Receiver<Made>, making: bool,
    /// the saved shares (row 498): the list, how each stands, those a request to connect is out for; the news from the
    /// threads that look and ask; the action on each row (Connect, Sign In), the plus in the section's header; the
    /// shares marked for it were asked for once at the start
    shares: Vec<crate::shares::Share>, share_states: Vec<crate::shares::State>, share_busy: std::collections::HashSet<String>, share_tx: std::sync::mpsc::Sender<ShareNews>, share_rx: Receiver<ShareNews>, share_btns: Vec<ActionButton>, share_add: ActionButton, share_menu: Option<usize>, shares_launched: bool, /// the stale placeholders (by share's id); the shares seen connected in this run (one silent retry when one drops)
    share_holders: std::collections::HashMap<String, PathBuf>, share_seen: std::collections::HashSet<String>, /// the Sync entry's row at the sidebar's foot
    sync_rect: Rect, /// what the open menu is about (its paths): what Copy Path, Copy Name and Copy Link take
    menu_paths: Vec<PathBuf>,
    /// the mesh's count of what arrived, as this window last saw it (its lists are read again when it moves on)
    mesh_rev: u64,
    /// the columns of the lists, a kind of panel (row 488): 0 the browser's panes, 1 a job's first pane, 2 its second
    col_layouts: [crate::pane::ColLayout; 3],
    /// the menu last asked for is about a job of the sidebar; the second pane's switch to an item's proxies
    menu_job: bool, job_prox: ActionButton,
    /// options of the window (both panes follow)
    hidden: bool, folders_first: bool,
    side_w: f64, side_rect: Rect, sections: [Section; 4], favorites: Vec<Place>, fixed: Vec<Place>, volumes: Vec<Place>, root: String, side_bar: ScrollBar, side_rows: Vec<SideRow>, side_hover: Option<usize>,
    back: ActionButton, forward: ActionButton, up: ActionButton, list_btn: ActionButton, grid_btn: ActionButton, pane2_btn: ActionButton, size: Slider, search: TextField,
    /// the path bar: its box; the segments as buttons with where each goes (none: the ellipsis) and what they were made for; the field while a path is typed; a path that was no folder
    path_rect: Rect, crumbs: Vec<(ActionButton, Option<PathBuf>)>, crumbs_for: Option<(PathBuf, String, i64)>, field: TextField, path_error: Option<(String, Instant)>, check: Option<u64>,
    focus: Focus, drag: Option<Drag>, gaps: [Rect; 3], over_gap: Option<usize>, /// in a job the second pane lies BELOW the first (row 484): the panes' area, for the divider in hand
    stacked: bool, panes_area: Rect, /// the item list's width in a job (logical px; its edge can be dragged)
    job_w: f64, cursor: winit31::cursor::CursorIcon,
    pub menubar: Option<InWindowMenu>, pointer: (f64, f64), pub mods: Mods,
    /// the scripted looks: the steps still to do, a wait, the picture to save, since when nothing is pending
    /// the window's title as it was last set: the active pane's folder
    titled: String,
    /// this window among the application's (a file operation's report says which window asked)
    id: u64,
    /// a name being changed in place; a question that is open; what went wrong, said at the window's foot;
    /// the entry to rename as soon as its folder is read again (a new folder goes straight into its name)
    rename: Option<Renaming>, dialog: Option<(min_ui::dialog::Dialog, Asked)>, toast: min_ui::toast::Toast, rename_next: Option<PathBuf>,
    /// the engine's questions that wait for the one that is open; the running operation in the status bar: its cancel button, the place a click opens the list at, the list open
    /// drags of files: where a drop can go (written every frame), what the layer left for the loop
    /// the lightbox (rows 452, 473): the file shown large; its close button; its last press (two make a double click)
    lightbox: crate::lightbox::Lightbox, lb_close: ActionButton, lb_press: Option<(Instant, (f64, f64))>, /// the contact sheet's tiles as they were drawn, and the one under the pointer
    lb_tiles: Vec<Rect>, lb_hover: Option<usize>, /// sequences as one entry, or every file by itself (row 475)
    seq_btn: ActionButton,
    /// the lightbox's transport (row 473): play / pause, loop, mute, "Open in QCView"; the line as it was drawn; a press on the picture that has not moved yet (a click plays or pauses);
    /// the system's hardware decoder, found at the first clip
    /// (measuring, under UFB_DEMO: when the lightbox was last drawn, the gaps between its frames and how long each took to draw, ms)
    lb_last: Option<Instant>, lb_gaps: Vec<f32>, lb_cost: Vec<f32>,
    /// what the lightbox's layer showed when it was last rendered (it is rendered again only when this changes)
    lb_key: Option<u64>,
    lb_play: ActionButton, lb_loop: ActionButton, lb_mute: ActionButton, lb_qcv: ActionButton, /// the rate a sequence plays at: a press steps it on (the secondary button: back)
    lb_fps: ActionButton, /// an EXR sequence's layers: the chip names the one shown, a press lists them (row 551)
    lb_layer: ActionButton, lb_line: Rect, lb_click: Option<(f64, f64)>, hw: Option<Option<qcv_player::HwDevice>>, preview_btn: ActionButton,
    pub dnd: crate::dnd::Handle,
    /// after a drag of ours ended: the moments at which the panes ask again whether their folders changed (the
    /// application that took the files moves them AFTER the drag is over - the owner's walk, 2026-11-16)
    recheck: Vec<Instant>,
    asks: VecDeque<Arc<crate::ops::Question>>, op_cancel: ActionButton, op_rect: Rect, op_list: bool, /// the tasks' panel (row 508): its card, a button a row (stop or dismiss), Clear Finished, the list as last seen
    task_card: Rect, /// how many rows the panel is scrolled back from its newest
    task_off: usize, task_btns: Vec<(u64, ActionButton)>, task_clear: min_ui::text_button::TextButton, tasks_rev: u64,
    /// the context menu drawn by us (Windows, row 460; `UFB_DRAWN_MENU=1` shows it on macOS too, to look at): what each of its rows is, the files it is about, where it was asked for
    ctx: min_ui::Menu, ctx_rows: Vec<CtxRow>, ctx_paths: Vec<PathBuf>, ctx_at: (f64, f64),
    /// what the context menu last asked for is about: the pane (none: the sidebar) and the folder its own rows act on
    menu_for: Option<(Option<usize>, PathBuf)>,
    demo: VecDeque<String>, demo_until: Option<Instant>, shot: Option<PathBuf>, settled: Option<Instant>, tip_demo: Option<String>, born: Instant,
    /// set when the scripted picture is saved: the application ends
    pub done: bool, pub out: Vec<Out>,
}

fn icon_button(icon: &'static str, tip: String) -> ActionButton { ActionButton::new(icon, &tip) }

impl Ui {
    pub fn new(window: Arc<Box<dyn Window>>, port: Port, rx: Receiver<Msg>, path: PathBuf, favorites: Vec<Place>) -> Option<Ui> {
        // (the favourites: those the build was started with, then those the user made - row 531)
        let started = favorites.clone(); let mut favorites = favorites; for p in crate::places::saved() { if !favorites.iter().any(|x| x.path == p.path) { favorites.push(p); } }
        let instance = Presenter::instance(); let surface = instance.create_surface(window.clone()).ok()?; let size = window.surface_size();
        #[cfg(target_os = "macos")] let view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().ok()?.as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))] let view = None;
        let mut presenter = Presenter::new(&instance, surface, (size.width.max(1), size.height.max(1)), Output::Auto, view).ok()?; presenter.engine.set_ui_scale(window.scale_factor() as f32);
        let layer = UiLayer::new(&presenter.engine.device, size.width.max(1), size.height.max(1)).ok()?; presenter.engine.set_overlay(Some(layer.view()));
        let mut search = TextField::new(Family::Sans, t::BODY_SIZE_S); search.placeholder = "Search".into();
        let view = crate::prefs::view(); let mut pane = Pane::new(path, &port); seed(&mut pane, view); places::refresh(&port);
        // (the scripted looks are the FIRST window's: a window they open does not run them again)
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true); let first = FIRST.swap(false, std::sync::atomic::Ordering::Relaxed);
        let demo = std::env::var("UFB_DEMO").ok().filter(|_| first).map(|d| d.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        let key = |c: Cmd, text: &str| menu::tip(text, c);
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1); let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::ops::subscribe(id, port.clone());
        let lightbox = { let p = port.clone(); crate::lightbox::Lightbox::new(Arc::new(move || p.send(Msg::Icon))) };
        let (made_tx, made_rx) = std::sync::mpsc::channel();
        let (share_tx, share_rx) = std::sync::mpsc::channel(); let shares = crate::shares::load(); Self::set_type_dirs();
        let mesh_rev = { let p = port.clone(); crate::mesh::on_change(Arc::new(move || p.send(Msg::Icon))); crate::mesh::rev() };
        let mut me = Ui { window, presenter, layer, fonts: Fonts::new(), tips: Tooltips::default(), icons: Icons::new(port.clone()), thumbs: { let p = port.clone(); Thumbs::new(std::sync::Arc::new(move || p.send(Msg::Icon))) }, port, rx,
            panes: vec![pane], active: 0, two: false, split: crate::settings::with(|s| s.float("window/split")), pane_heads: [Rect::ZERO; 2], tabs: vec![Tab::default()], tab: 0, tab_rects: vec![], tab_close: vec![], tab_new: icon_button("plus", key(Cmd::NewTab, "New tab")).size_s(),
            job: None, jobs: crate::db::with(|d| d.subscriptions()).unwrap_or_default(), job_secs: vec![], job_secs_for: vec![], job_refresh: icon_button("arrow-clockwise", key(Cmd::Refresh, "Read the job again")).size_s(), job_add: { let mut b = ActionButton::new("plus", "").size_s(); b.label = Some("Add".into()); b },
            job_list: Rect::ZERO, panes_x0: 0.0, chip: None, coledit: None, prefs: None, note: None, started_favorites: started, bulk: vec![], board: None, board_of: Default::default(), board_btn: { let mut b = ActionButton::new("kanban", "Show the items as a board, or as the list").size_s(); b.label = Some("Board".into()); b }, board_ctx: vec![], tracker: None, trk_btn: { let mut b = ActionButton::new("list-checks", "The job's tracked items").size_s(); b.label = Some("Tracker".into()); b }, due_btns: crate::tracker::DUES.iter().map(|(_, label)| { let mut b = ActionButton::new("", "").size_s(); b.label = Some(label.to_string()); b }).collect(), trk_press: None, items: { let mut l = crate::itemlist::ItemList::default(); l.show_modified = crate::settings::with(|s| s.bool("columns/items_modified")); l }, job_meta: { let mut b = ActionButton::new("columns", "Add, show and restore metadata columns").size_s(); b.label = Some("Columns".into()); b }, meta_ctx: (None, None, vec![], vec![]), made_tx, made_rx, making: false, share_states: vec![crate::shares::State::NotConnected; shares.len()], shares, share_busy: Default::default(), share_tx, share_rx, share_btns: vec![], share_add: ActionButton::new("plus", "Add a share: its address, or a folder").size_s(), share_menu: None, shares_launched: false, share_holders: Default::default(), share_seen: Default::default(), sync_rect: Rect::ZERO, menu_paths: vec![], mesh_rev, col_layouts: { let get = |k: &str| crate::settings::with(|s| crate::pane::ColLayout::parse(&s.string(k))).unwrap_or_else(crate::pane::ColLayout::all); [get("columns/browser"), get("columns/job_top"), get("columns/job_low")] }, menu_job: false, job_prox: { let mut b = ActionButton::new("", "Show the item's proxies in place of its renders").size_s(); b.label = Some("Proxies".into()); b }, hidden: view.0, folders_first: view.1,
            side_w: { let w = crate::settings::with(|s| s.float("window/sidebar")); if w > 0.0 { w.clamp(SIDE_MIN, SIDE_MAX) } else { t::RAIL_WIDTH_MIN as f64 } }, side_rect: Rect::ZERO, sections: [Section::new("Favorites", true), Section::new("Shares", true), Section::new("Jobs", true), Section::new("Places", true)], favorites, fixed: places::fixed(), volumes: vec![], root: String::new(), side_bar: ScrollBar::default(), side_rows: vec![], side_hover: None,
            back: icon_button("arrow-left", key(Cmd::Back, "Back")), forward: icon_button("arrow-right", key(Cmd::Forward, "Forward")), up: icon_button("arrow-up", key(Cmd::Up, "Enclosing folder")),
            list_btn: icon_button("list", key(Cmd::AsList, "List")), grid_btn: icon_button("squares-four", key(Cmd::AsGrid, "Grid")), pane2_btn: icon_button("columns", "Second pane  (Tab switches panes)".to_string()), size: Slider::new(((96.0 - grid::TILE_MIN) / (grid::TILE_MAX - grid::TILE_MIN)) as f32), search,
            path_rect: Rect::ZERO, crumbs: vec![], crumbs_for: None, field: TextField::new(Family::Sans, t::BODY_SIZE_S), path_error: None, check: None,
            focus: Focus::Pane, drag: None, gaps: [Rect::ZERO; 3], over_gap: None, stacked: false, panes_area: Rect::ZERO, job_w: { let w = crate::settings::with(|s| s.float("window/job_list")); if w > 0.0 { w.clamp(JOB_LIST_MIN, 1200.0) } else { JOB_LIST } }, cursor: winit31::cursor::CursorIcon::Default,
            menubar: InWindowMenu::wanted().then(InWindowMenu::new), pointer: (0.0, 0.0), mods: Mods::default(),
            titled: String::new(), id, rename: None, dialog: None, toast: min_ui::toast::Toast::new(), rename_next: None, ctx: min_ui::Menu::new(), ctx_rows: vec![], ctx_paths: vec![], ctx_at: (0.0, 0.0), lightbox, lb_close: ActionButton::new("x", "Close  (Space)"), lb_press: None, lb_tiles: vec![], lb_hover: None, seq_btn: icon_button("stack", "Group sequences".to_string()),
            lb_play: ActionButton::new("play", "Play / pause  (K)"), lb_loop: ActionButton::new("repeat", "Loop  (V)"), lb_mute: ActionButton::new("speaker-high", "Mute  (M)"), lb_qcv: { let mut b = ActionButton::new("", "Open in QCView").size_s(); b.label = Some("Open in QCView".into()); b }, lb_fps: { let mut b = ActionButton::new("", "Frames a second: press to change (remembered for this sequence)").size_s(); b.label = Some("24 fps".into()); b }, lb_layer: { let mut b = ActionButton::new("stack", "Layer shown: press to choose another").size_s(); b.label = Some("Layers".into()); b }, lb_line: Rect::ZERO, lb_click: None, hw: None, lb_last: None, lb_gaps: vec![], lb_cost: vec![], lb_key: None, preview_btn: icon_button("eye", "Preview  (Space)".to_string()),
            dnd: Default::default(), recheck: vec![], asks: VecDeque::new(), op_cancel: ActionButton::new("x", "Stop").size_s(), op_rect: Rect::ZERO, op_list: false, task_card: Rect::ZERO, task_off: 0, task_btns: vec![], task_clear: min_ui::text_button::TextButton::new("Clear Finished", min_ui::text_button::Variant::Secondary).size_s(), tasks_rev: 0, menu_for: None, demo, demo_until: None, shot: std::env::var_os("UFB_SHOT").filter(|_| first).map(PathBuf::from), settled: None, tip_demo: None, born: Instant::now(), done: false, out: vec![] };
        // (a window opens on a browser view: with its second pane when the settings say so, row 543)
        if crate::prefs::second_pane() { me.set_two(true); me.active = 0; }
        Some(me)
    }
    pub fn resize(&mut self, w: u32, h: u32) { if w == 0 || h == 0 { return } self.presenter.resize(w, h); self.presenter.engine.set_ui_scale(self.window.scale_factor() as f32); self.layer.resize(&self.presenter.engine.device, w, h); self.presenter.engine.set_overlay(Some(self.layer.view())); if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } }
    fn scale(&self) -> f64 { self.window.scale_factor() }
    fn pane(&mut self) -> &mut Pane { let i = self.active.min(self.panes.len() - 1); &mut self.panes[i] }
    fn shown(&self) -> usize { if self.two { self.panes.len().min(2) } else { 1 } }
    // ---------------------------------------------------------------- tabs (row 480)
    /// The tab in front is put into its place in the list, or taken out of it (the same exchange both ways).
    fn swap_tab(&mut self) { let t = &mut self.tabs[self.tab]; std::mem::swap(&mut self.panes, &mut t.panes); std::mem::swap(&mut self.active, &mut t.active); std::mem::swap(&mut self.two, &mut t.two); std::mem::swap(&mut self.split, &mut t.split); std::mem::swap(&mut self.job, &mut t.job); std::mem::swap(&mut self.tracker, &mut t.tracker); }
    /// What belongs to the tab in front and does not go with it is ended: a name or a path being typed, an open menu, the lightbox.
    fn leave_tab(&mut self) { self.note = None; if self.lightbox.is_open() { self.close_lightbox(); } self.end_rename(); self.leave_path(); self.leave_search(); self.ctx.close(); self.menu_for = None; self.rename_next = None; self.crumbs_for = None; self.side_hover = None; self.board = None; self.items.edit = None; if let Some(t) = self.tracker.as_mut() { t.list.edit = None; } self.chip = None; self.coledit = None; self.job_secs_for.clear(); self.focus = Focus::Pane; }
    /// The tab now in front is looked at again: its folders may have changed while it was behind.
    fn enter_tab(&mut self) { let port = self.port.clone(); let n = self.shown(); for p in self.panes.iter_mut().take(n) { p.activated(&port); } if let Some(j) = self.job.as_mut() { j.reload(); } }
    fn show_tab(&mut self, i: usize) { if i == self.tab || i >= self.tabs.len() { return } self.leave_tab(); self.swap_tab(); self.tab = i; self.swap_tab(); self.enter_tab(); }
    /// A new tab on this folder, after the one in front, and shown: one pane, with the view of the pane it was asked from.
    fn new_tab(&mut self, path: PathBuf) {
        let mut p = Pane::new(path, &self.port); seed(&mut p, crate::prefs::view()); p.set_hidden(self.hidden); p.set_folders_first(self.folders_first);
        // (a folder with a view of its own keeps it in a new tab too; any other is shown as the tab one comes from)
        if !p.has_own_view() { let from = &self.panes[self.active.min(self.panes.len() - 1)]; let (mode, tile, seq) = (from.mode, from.grid.tile, from.model.sequences); p.set_mode(mode); p.grid.tile = tile; if p.model.sequences != seq { p.set_sequences(seq); } } p.view_synced();
        self.leave_tab(); self.swap_tab(); self.tab += 1; self.tabs.insert(self.tab, Tab::default());
        self.panes = vec![p]; self.active = 0; self.two = false; self.split = 0.5; self.job = None; self.tracker = None;
        // (a browser view starts with its second pane when the settings say so; a job that is opened in the tab decides for itself)
        if crate::prefs::second_pane() { self.set_two(true); self.active = 0; }
    }
    /// A tab is closed; the last one closes the window.
    fn close_tab(&mut self, i: usize) {
        if self.tabs.len() <= 1 { self.out.push(Out::Close); return } if i >= self.tabs.len() { return }
        if i == self.tab { self.leave_tab(); self.panes.clear(); self.job = None; self.tracker = None; self.tabs.remove(i); self.tab = i.min(self.tabs.len() - 1); self.swap_tab(); self.enter_tab(); }
        else { self.tabs.remove(i); if i < self.tab { self.tab -= 1; } }
    }
    /// What a tab is called, and the folder it shows: its active pane's.
    fn tab_place(&self, i: usize) -> (String, PathBuf) { let (panes, active, job) = if i == self.tab { (&self.panes, self.active, &self.job) } else { (&self.tabs[i].panes, self.tabs[i].active, &self.tabs[i].job) }; if let Some(j) = job { return (j.name.clone(), j.path.clone()) } if (if i == self.tab { &self.tracker } else { &self.tabs[i].tracker }).is_some() { return ("All Tracked".to_string(), PathBuf::new()) } match panes.get(active.min(panes.len().saturating_sub(1))) { Some(p) => (p.title(), p.path().to_path_buf()), None => (String::new(), PathBuf::new()) } }
    // ---------------------------------------------------------------- the project side (row 482; `jobview.rs`, `db.rs`)
    fn reload_jobs(&mut self) { self.jobs = crate::db::with(|d| d.subscriptions()).unwrap_or_default(); }
    /// The tab in front shows this job: its sections and items beside the browser, which follows them.
    fn open_job(&mut self, path: PathBuf, name: String) {
        if self.job.as_ref().is_some_and(|j| j.path == path) { self.tracker = None; return }
        self.leave_tab(); self.tracker = None; let p = self.port.clone(); self.job = Some(Job::new(path, name, Arc::new(move || p.send(Msg::Icon)))); self.items.clear(); self.active = 0;
    }
    /// A place of the sidebar is opened: a job as a job, a folder in the browser (the tab stops showing a job).
    fn open_place(&mut self, path: PathBuf, job: bool) {
        // (the two panes' share is remembered apart for a job, where one lies above the other, and for the browser - row 523)
        let was_job = self.job.is_some(); if job != was_job { self.split = crate::settings::with(|s| s.float(if job { "window/job_split" } else { "window/split" })); }
        if job { let name = self.jobs.iter().find(|j| j.path == path).map(|j| j.name.clone()).unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()); self.open_job(path, name); }
        else { if self.job.take().is_some() | self.tracker.take().is_some() { self.focus = Focus::Pane; for p in self.panes.iter_mut() { p.root = None; } } self.go(path); }
    }
    /// The browser follows the job: the chosen item's project folder in the first pane and its renders in the second
    /// (each the item itself when it has none) - or, with nothing chosen, the section's folder.
    fn job_follow(&mut self, first: bool, second: bool) {
        let Some(j) = self.job.as_ref() else { return }; let port = self.port.clone(); if j.hold { return }
        // (nothing chosen: nothing is shown - the panes are not drawn, `blank`)
        let Some(it) = j.chosen_item() else { return };
        let a = it.project.clone(); let b = match it.proxies.clone().filter(|_| j.proxies) { Some(p) => p, None => it.renders.clone() };
        // each pane is given to the folder it STARTS in (rows 486, 487) - the item's project folder, its renders; the item
        // itself where it has none: Up stops there, and its history begins anew - Back cannot leave it
        if first { self.panes[0].rooted(&port, a.clone(), a); }
        if second && self.two && self.panes.len() > 1 { self.panes[1].rooted(&port, b.clone(), b); }
    }
    /// In a job nothing is chosen: the panes show nothing (row 486 - not the section's folder, which only repeats the item list).
    fn blank(&self) -> bool { self.tracker.is_some() || self.job.as_ref().is_some_and(|j| j.chosen_item().is_none()) }
    /// The folder the window is "in", for a new tab or a path to type: the active pane's - or, with nothing shown, the job's section.
    fn here(&self) -> PathBuf { match &self.job { Some(j) if j.chosen_item().is_none() => j.section_dir().unwrap_or_else(|| j.path.clone()), _ => self.panes[self.active.min(self.panes.len() - 1)].path().to_path_buf() } }
    /// Another section of the job comes to the front (row 484). What the one in front shows is kept for it - the
    /// chosen item, the panes' folders - and the browser goes AT ONCE to what belongs to the new one: what it showed
    /// when it was last left, else its own folder. Nothing of the old section stays in view while the new one is read.
    fn job_show_section(&mut self, name: &str) {
        let port = self.port.clone(); let two = self.two && self.panes.len() > 1;
        self.tracker = None;
        let Some(j) = self.job.as_mut() else { return }; if j.section.as_deref() == Some(name) { return }
        if let Some(old) = j.section.clone() { let at = |p: &Pane| (p.root.clone().unwrap_or_else(|| p.path().to_path_buf()), p.path().to_path_buf()); j.memory.insert(old, (j.chosen_item().map(|i| i.path.clone()), at(&self.panes[0]), two.then(|| at(&self.panes[1])))); }
        let dir = j.path.join(name); let kept = j.memory.get(name).cloned();
        j.show_section(name); self.items.clear();
        match kept {
            // (what it showed when it was left: the item, and where each pane was inside it)
            Some((Some(item), p0, p1)) => { j.choose = Some(item); j.hold = true; let on = p1.is_some();
                if on != self.two { self.set_two(on); }
                self.panes[0].rooted(&port, p0.0, p0.1);
                if let Some(p1) = p1 { if self.panes.len() > 1 { self.panes[1].rooted(&port, p1.0, p1.1); } } }
            // (a first visit, or one left with nothing chosen: nothing is shown until an item is chosen)
            _ => { let _ = dir; }
        }
        self.active = 0;
    }
    /// The chosen item is brought into view.
    fn job_reveal(&mut self) { if let Some(pos) = self.job.as_ref().and_then(|j| j.chosen).and_then(|i| self.items.order.iter().position(|k| *k == i)) { self.items.reveal(pos); } }
    // ---------------------------------------------------------------- the items' metadata (row 489; `itemlist.rs`, `db.rs`)
    /// The job in front and its section, for the database.
    fn section_key(&self) -> Option<(PathBuf, String)> { let j = self.job.as_ref()?; Some((j.path.clone(), j.section.clone()?)) }
    /// The item list reads its columns and values again, and orders its rows.
    fn items_reload(&mut self) {
        let (Some(key), Some(j)) = (self.section_key(), self.job.as_ref()) else { self.board = None; return }; self.items.load(&key.0, &key.1); self.items.reorder(&j.items);
        // the board of this section, if it is shown as one: its lanes anew - and the list again when its column is gone or cannot group any more (1.4.2)
        match self.board_of.get(&key).cloned() {
            Some(by) => { if self.board.as_ref().map(|b| &b.by) != Some(&by) { self.board = Some(crate::board::Board::new(by)); }
                if !self.board.as_mut().is_some_and(|b| b.rebuild(&j.items, &self.items)) { self.board = None; self.board_of.remove(&key); } }
            None => self.board = None,
        }
    }
    /// The menu of the Board button: the columns the section can be grouped by; the list again.
    fn board_menu(&mut self, at: (f64, f64)) {
        let by = crate::board::eligible(&self.items); self.board_ctx = by.iter().map(|b| b.0.clone()).collect(); self.menu_for = None;
        self.show_menu(crate::ctxmenu::Ask { at, paths: vec![], rows: crate::ctxmenu::rows_for_board(&by.iter().map(|b| b.1.clone()).collect::<Vec<_>>(), self.board.is_some()) });
    }
    /// What the board's pointer asked for.
    fn board_acts(&mut self, acts: Vec<crate::board::Act>) {
        use crate::board::{Act, LaneBy, LaneValue}; let Some((job, sec)) = self.section_key() else { return };
        for a in acts { match a {
            // several cards (row 530; 1.4.2): the command key adds or takes one; Shift takes all between the card in front
            // and this one in ONE lane; a plain press on one of several keeps them (they are about to be dragged)
            Act::Choose(i) => { let (cmd, shift) = (self.mods.cmd, self.mods.shift); let front = self.job.as_ref().and_then(|j| j.chosen);
                if let Some(b) = self.board.as_mut() {
                    if cmd { if b.sel.is_empty() { b.sel.extend(front); } match b.sel.iter().position(|k| *k == i) { Some(k) => { b.sel.remove(k); } None => b.sel.push(i) } }
                    else if shift { if let (Some(l), Some(f)) = (b.lane_of(i), front) { if b.lane_of(f) == Some(l) { let items = &b.lanes[l].items; if let (Some(a), Some(z)) = (items.iter().position(|k| *k == f), items.iter().position(|k| *k == i)) { b.sel = items[a.min(z)..=a.max(z)].to_vec(); } } } }
                    else if !(b.sel.len() > 1 && b.sel.contains(&i)) { b.sel.clear(); } }
                if !shift { self.job_choose(i); } }
            Act::Star(i) => self.items_acts(vec![crate::itemlist::Act::Star(i)]),
            Act::Menu(i, at) => self.items_acts(vec![crate::itemlist::Act::RowMenu(i, at)]),
            // a card let go on another lane: the lane's value is the item's
            // a card let go on another lane: the lane's value is the item's - and that of every card chosen with it (row 530)
            Act::Drop(i, value) => if let Some(by) = self.board.as_ref().map(|b| b.by.clone()) {
                let which: Vec<usize> = match self.board.as_ref() { Some(b) if b.sel.len() > 1 && b.sel.contains(&i) => b.sel.clone(), _ => vec![i] };
                for k in which { let Some(it) = self.job.as_ref().and_then(|j| j.items.get(k)).cloned() else { continue };
                    let r = match (by.clone(), value.clone()) {
                        (LaneBy::Tracked, LaneValue::Tracked(on)) => crate::db::with(|d| d.set_tracked(&job, &it.path, &sec, on)),
                        (LaneBy::Col(uuid), LaneValue::Option(name)) => crate::db::with(|d| d.set_field(&job, &it.path, &sec, &uuid, serde_json::Value::String(name))),
                        (LaneBy::Col(uuid), LaneValue::Unset) => crate::db::with(|d| d.set_field(&job, &it.path, &sec, &uuid, serde_json::Value::Null)),
                        (LaneBy::Col(uuid), LaneValue::Check(on)) => crate::db::with(|d| d.set_field(&job, &it.path, &sec, &uuid, serde_json::Value::Bool(on))),
                        _ => Some(Ok(())),
                    };
                    self.meta_err(r); }
                self.reload_lists();
            },
        } }
    }
    fn meta_err(&mut self, r: Option<Result<(), String>>) { match r { Some(Ok(())) => {} Some(Err(e)) => self.toast.show(&e, min_ui::toast::ToastKind::Negative, Instant::now()), None => self.toast.show("The database could not be opened.", min_ui::toast::ToastKind::Negative, Instant::now()) } }
    /// The cell being typed into is written. False: what was typed is no value of the column's type - the field stays.
    fn commit_cell(&mut self) -> bool {
        let Some((item, col)) = self.list_mut().edit.as_ref().map(|e| (e.item.clone(), e.col)) else { if self.focus == Focus::Cell { self.focus = Focus::Items; } return true };
        match self.list_mut().typed() {
            None => { self.cancel_cell(); true }
            Some(Err(why)) => { self.toast.show(&why, min_ui::toast::ToastKind::Notice, Instant::now()); false }
            Some(Ok((_, _, v))) => { self.list_mut().edit = None; self.focus = Focus::Items;
                if let Some((job, sec, c)) = self.cell_target(&item, col) { let r = crate::db::with(|d| d.set_field(&job, &item, &sec, &c.uuid, v)); self.meta_err(r); } self.reload_lists(); true }
        }
    }
    fn cancel_cell(&mut self) { self.list_mut().edit = None; if self.focus == Focus::Cell { self.focus = Focus::Items; } }
    fn edit_cell(&mut self, item: usize, col: usize) {
        match self.tracker.as_mut() { Some(t) => { let Some(path) = t.rows.get(item).map(|r| r.path.clone()) else { return }; if t.target(&path, col).is_none() { return } t.list.begin_edit(&mut self.fonts, &t.rows, item, col); }
            None => if let Some(j) = self.job.as_ref() { self.items.begin_edit(&mut self.fonts, &j.items, item, col); } }
        // notes and links hold several lines: they are written in a small editor by the cell, not in the row (row 529)
        let wide = { let l = self.list_ref(); l.edit.as_ref().and_then(|e| l.cols.get(e.col).map(|c| (crate::db::kind_of(&c.kind), c.name.clone()))).filter(|(k, _)| matches!(*k, "notes" | "links")) };
        // (the field is taken out of the row ONLY for those two: taking it first closed every other cell's field)
        if let Some((_, title)) = wide { let Some(e) = self.list_mut().edit.take() else { return };
            let mut area = min_ui::text_area::TextArea::new(Family::Sans, t::BODY_SIZE_S); area.set_text(e.field.text()); area.focused = true;
            self.leave_path(); self.leave_search(); self.chip = None; self.note = Some(NoteEdit { item: e.item, col: e.col, title, area, card: Rect::ZERO, at: self.pointer }); return
        }
        if self.list_mut().edit.is_some() { self.leave_path(); self.leave_search(); self.focus = Focus::Cell; }
    }
    fn list_ref(&self) -> &crate::itemlist::ItemList { match self.tracker.as_ref() { Some(t) => &t.list, None => &self.items } }
    /// The small editor's text becomes the cell's value (nothing typed: the cell is cleared); or it is left as it was.
    fn note_done(&mut self, save: bool) {
        let Some(n) = self.note.take() else { return }; if !save { return }
        let text = n.area.text().trim_end().to_string(); let v = if text.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(text) };
        if let Some((job, sec, c)) = self.cell_target(&n.item, n.col) { let r = crate::db::with(|d| d.set_field(&job, &n.item, &sec, &c.uuid, v)); self.meta_err(r); } self.reload_lists();
    }
    /// The list in front: the tracker's, or the section's item list.
    fn list_mut(&mut self) -> &mut crate::itemlist::ItemList { match self.tracker.as_mut() { Some(t) => &mut t.list, None => &mut self.items } }
    /// Where a cell of the list in front is written: (the job, the section, the column as that section has it). In the
    /// tracker a cell can be of a column its row's section does not have: none.
    fn cell_target(&self, item: &Path, col: usize) -> Option<(PathBuf, String, crate::db::ColDef)> {
        match &self.tracker { Some(t) => t.target(item, col), None => { let (job, sec) = self.section_key()?; Some((job, sec, self.items.cols.get(col)?.clone())) } }
    }
    /// Something of the metadata was written: the lists read again.
    fn reload_lists(&mut self) { self.items_reload(); if let Some(t) = self.tracker.as_mut() { t.reload(); } }
    // ---------------------------------------------------------------- the tracker (row 491; `tracker.rs`)
    /// The tab in front shows the tracker: this job's (its sections stay in the bar), or every subscribed job's.
    fn open_tracker(&mut self, all: bool) {
        self.cancel_cell(); self.chip = None;
        if all { if self.tracker.as_ref().is_some_and(|t| t.scope == crate::tracker::Scope::All) { return } self.leave_tab(); self.job = None; for p in self.panes.iter_mut() { p.root = None; } self.tracker = Some(crate::tracker::Tracker::new(crate::tracker::Scope::All, { let p = self.port.clone(); Arc::new(move || p.send(Msg::Icon)) })); }
        else if let Some(j) = self.job.as_ref() { self.tracker = Some(crate::tracker::Tracker::new(crate::tracker::Scope::Job(j.path.clone()), { let p = self.port.clone(); Arc::new(move || p.send(Msg::Icon)) })); }
        self.focus = Focus::Items;
    }
    /// A tracked item is gone to, in its job: the section it is in comes to the front and the item is chosen. From All
    /// Tracked, in a new tab (the tracker stays as it is).
    fn open_tracked(&mut self, item: PathBuf) {
        if self.tracker.as_ref().is_some_and(|t| t.lost(&item)) { self.toast.show("Its folder is not there any more: Relink it to where it went, or remove it from the tracker.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        let Some((scope, (job, folder))) = self.tracker.as_ref().and_then(|t| t.home(&item).map(|h| (t.scope.clone(), h))) else { return };
        if scope == crate::tracker::Scope::All { self.new_tab(job.clone()); self.open_place(job, true); if let Some(j) = self.job.as_mut() { j.show_section(&folder); j.choose = Some(item); } return }
        self.tracker = None;
        if self.job.as_ref().is_some_and(|j| j.section.as_deref() == Some(folder.as_str()) && !j.loading) { if let Some(i) = self.job.as_ref().and_then(|j| j.items.iter().position(|it| it.path == item)) { self.job_choose(i); self.job_reveal(); } }
        else { self.job_show_section(&folder); if let Some(j) = self.job.as_mut() { j.choose = Some(item); j.hold = false; } }
        self.focus = Focus::Items;
    }
    /// What the tracker's pointer asked for.
    fn tracker_acts(&mut self, acts: Vec<crate::itemlist::Act>) {
        use crate::itemlist::Act; let now = Instant::now();
        let menu_on: Vec<usize> = acts.iter().filter_map(|a| if let Act::RowMenu(i, _) = a { Some(*i) } else { None }).collect(); let (cmd, shift) = (self.mods.cmd, self.mods.shift);
        for a in acts { let row = |t: &crate::tracker::Tracker, i: usize| t.rows.get(i).cloned(); match a {
            // (a second press on the same row soon after the first goes to the item)
            Act::Choose(i) => { let double = !cmd && !shift && self.trk_press.is_some_and(|(t0, k)| k == i && now.duration_since(t0) < Duration::from_millis(450)); self.trk_press = Some((now, i));
                // several rows (row 530): the command key adds or takes one; Shift takes all from the row in front to this
                // one, as they are listed; a plain press chooses one - but the menu's press on one of several keeps them
                if let Some(t) = self.tracker.as_mut() { if let Some(path) = t.rows.get(i).map(|r| r.path.clone()) {
                    if menu_on.contains(&i) && t.sel.len() > 1 && t.sel.contains(&path) {}
                    else if cmd { if t.sel.is_empty() { if let Some(p0) = t.chosen.and_then(|c| t.rows.get(c)).map(|r| r.path.clone()) { t.sel.push(p0); } } match t.sel.iter().position(|p| *p == path) { Some(k) => { t.sel.remove(k); } None => t.sel.push(path) } t.chosen = Some(i); }
                    else if shift { let (a, b) = (t.chosen.and_then(|c| t.list.order.iter().position(|o| *o == c)).unwrap_or(0), t.list.order.iter().position(|o| *o == i).unwrap_or(0)); let (a, b) = (a.min(b), a.max(b));
                        t.sel = t.list.order.get(a..=b).unwrap_or(&[]).iter().filter_map(|o| t.rows.get(*o).map(|r| r.path.clone())).collect(); }
                    else { t.sel = vec![path]; t.chosen = Some(i); } } }
                if double { self.trk_press = None; if let Some(it) = self.tracker.as_ref().and_then(|t| row(t, i)) { self.open_tracked(it.path); return } } }
            // the star in the tracker takes the item out of it
            Act::Star(i) => if let Some((it, (job, sec))) = self.tracker.as_ref().and_then(|t| row(t, i).and_then(|it| t.home(&it.path).map(|h| (it, h)))) { let r = crate::db::with(|d| d.set_tracked(&job, &it.path, &sec, false)); self.meta_err(r); self.reload_lists(); },
            Act::Check(i, k) => if let Some(it) = self.tracker.as_ref().and_then(|t| row(t, i)) { if let Some((job, sec, c)) = self.cell_target(&it.path, k) { let on = !self.tracker.as_ref().is_some_and(|t| t.list.cols.get(k).is_some_and(|m| crate::itemlist::is_checked(t.list.value(&it, m)))); let r = crate::db::with(|d| d.set_field(&job, &it.path, &sec, &c.uuid, serde_json::Value::Bool(on))); self.meta_err(r); self.reload_lists(); } },
            Act::Edit(i, k) => { self.trk_press = None; self.edit_cell(i, k); }
            Act::Options(i, k, at) => if let Some(it) = self.tracker.as_ref().and_then(|t| row(t, i)) { if let Some((_, _, c)) = self.cell_target(&it.path, k) {
                self.trk_press = None; let s = self.scale(); let size = self.window.surface_size(); let cur = self.tracker.as_ref().and_then(|t| t.list.cols.get(k).map(|m| crate::itemlist::text_of(&m.kind, t.list.value(&it, m)))).unwrap_or_default();
                let mut p = crate::chips::ChipPicker::new(it.path.clone(), k, c.options.clone(), cur); p.open(&mut self.fonts, Rect::new(at.0, at.1 - 24.0 * s, at.0 + 120.0 * s, at.1), Rect::new(0.0, 0.0, size.width as f64, size.height as f64), s); self.chip = Some(p); } },
            Act::RowMenu(i, at) => if let Some(it) = self.tracker.as_ref().and_then(|t| row(t, i)) { self.meta_ctx = (None, Some(i), vec![], vec![]); self.menu_for = Some((None, it.path.clone())); self.menu_job = false; let many = self.tracker.as_ref().map(|t| t.sel.len()).filter(|n| *n > 1 && self.tracker.as_ref().is_some_and(|t| t.sel.contains(&it.path)));
                let rows = if let Some(n) = many { self.bulk_rows(n) } else if self.tracker.as_ref().is_some_and(|t| t.lost(&it.path)) { crate::ctxmenu::rows_for_lost() } else { crate::ctxmenu::rows_for_tracked() }; self.show_menu(crate::ctxmenu::Ask { at, paths: if many.is_some() { vec![] } else { vec![it.path] }, rows }); },
            Act::Link(u) => { self.trk_press = None; self.open_link(&u); }
            Act::HeadMenu(..) | Act::Width(..) => {}
        } }
    }
    /// What the item list's pointer asked for.
    fn items_acts(&mut self, acts: Vec<crate::itemlist::Act>) {
        use crate::itemlist::Act; let Some((job, sec)) = self.section_key() else { return };
        for a in acts { match a {
            Act::Choose(i) => self.job_choose(i),
            Act::Star(i) => if let Some(it) = self.job.as_ref().and_then(|j| j.items.get(i)).cloned() { let on = !self.items.tracked(&it); let r = crate::db::with(|d| d.set_tracked(&job, &it.path, &sec, on)); self.meta_err(r); self.items_reload(); },
            Act::Check(i, k) => if let (Some(it), Some(c)) = (self.job.as_ref().and_then(|j| j.items.get(i)).cloned(), self.items.cols.get(k).cloned()) { let on = !crate::itemlist::is_checked(self.items.value(&it, &c)); let r = crate::db::with(|d| d.set_field(&job, &it.path, &sec, &c.uuid, serde_json::Value::Bool(on))); self.meta_err(r); self.items_reload(); },
            Act::Edit(i, k) => self.edit_cell(i, k),
            Act::Link(u) => self.open_link(&u),
            // a dropdown cell: its options as chips, in a popover under it
            Act::Options(i, k, at) => if let (Some(c), Some(it)) = (self.items.cols.get(k).cloned(), self.job.as_ref().and_then(|j| j.items.get(i)).cloned()) {
                let s = self.scale(); let size = self.window.surface_size(); let cur = crate::itemlist::text_of(&c.kind, self.items.value(&it, &c));
                let mut p = crate::chips::ChipPicker::new(it.path.clone(), k, c.options.clone(), cur); p.open(&mut self.fonts, Rect::new(at.0, at.1 - 24.0 * s, at.0 + 120.0 * s, at.1), Rect::new(0.0, 0.0, size.width as f64, size.height as f64), s); self.chip = Some(p); },
            Act::HeadMenu(k, at) => self.columns_menu(k, at),
            Act::RowMenu(i, at) => if let Some(it) = self.job.as_ref().and_then(|j| j.items.get(i)).cloned() { self.meta_ctx = (None, Some(i), vec![], vec![]); self.menu_for = Some((Some(0), it.path.clone())); self.menu_job = false; self.show_menu(crate::ctxmenu::Ask { at, paths: vec![it.path.clone()], rows: crate::ctxmenu::rows_for_item(self.items.tracked(&it)) }); },
            Act::Width(k, w) => if let Some(c) = self.items.cols.get(k) { let name = c.name.clone(); let r = crate::db::with(|d| d.set_layout(&job, &sec, &name, None, Some(w), None)); self.meta_err(r); },
        } }
    }
    /// The menu of the item list's columns: a column's own rows when it was asked for on one, then what adds, shows and restores.
    fn columns_menu(&mut self, col: Option<usize>, at: (f64, f64)) {
        let Some((job, sec)) = self.section_key() else { return };
        let hidden: Vec<String> = self.items.hidden.iter().map(|c| c.name.clone()).collect(); let trashed = crate::db::with(|d| d.trashed_columns(&job, &sec)).unwrap_or_default();
        let rows = crate::ctxmenu::rows_for_columns(col.is_some(), &hidden, &trashed.iter().map(|t| t.1.clone()).collect::<Vec<_>>(), self.items.show_modified);
        self.meta_ctx = (col, None, hidden, trashed); self.menu_for = None; self.show_menu(crate::ctxmenu::Ask { at, paths: vec![], rows });
    }
    /// A column's editor: its name and, for a dropdown or a priority, its options as chips (row 490).
    fn ask_column(&mut self, id: Option<i64>, kind: &str, name: &str, options: &[(String, Option<String>)]) { self.ctx.close(); self.cancel_cell(); self.coledit = Some(crate::chips::ColumnEditor::new(id, kind, name, options, &mut self.fonts)); }
    /// The column's editor was left.
    fn column_done(&mut self, done: crate::chips::Done) {
        let Some(ed) = self.coledit.take() else { return }; let crate::chips::Done::Save { name, options, renames } = done else { return }; let Some((job, sec)) = self.section_key() else { return };
        let uuid = ed.id.and_then(|id| self.items.cols.iter().chain(self.items.hidden.iter()).find(|c| c.id == id).map(|c| c.uuid.clone()));
        let r = crate::db::with(|db| match ed.id { Some(id) => db.update_column(id, &name, &ed.kind, &options), None => db.add_column(&job, &sec, &name, &ed.kind, &options).map(|_| ()) });
        let ok = matches!(r, Some(Ok(()))); self.meta_err(r);
        // an option renamed: the items that hold the old name hold the new one (1.4.2 left them with a value that is no option)
        if let (true, Some(uuid)) = (ok, uuid) { for (old, new) in renames { let holders: Vec<PathBuf> = self.items.meta.iter().filter(|(_, m)| m.values.get(&uuid).and_then(|v| v.as_str()) == Some(old.as_str())).map(|(p, _)| p.clone()).collect();
            for item in holders { let r = crate::db::with(|d| d.set_field(&job, &item, &sec, &uuid, serde_json::Value::String(new.clone()))); self.meta_err(r); } } }
        self.items_reload();
    }
    /// A chip was picked for a cell (or the cell cleared; or a text made an option, and set).
    fn chip_picked(&mut self, item: PathBuf, col: usize, pick: crate::chips::Pick) {
        use crate::chips::Pick; let Some((job, sec, c)) = self.cell_target(&item, col) else { return };
        let value = match pick { Pick::Clear => serde_json::Value::Null, Pick::Set(n) => serde_json::Value::String(n),
            Pick::Create(n) => { let taken: Vec<Option<String>> = c.options.iter().map(|o| o.1.clone()).collect(); let r = crate::db::with(|d| d.add_option(c.id, &n, &crate::chips::next_color(&taken))); self.meta_err(r); serde_json::Value::String(n) } };
        let r = crate::db::with(|d| d.set_field(&job, &item, &sec, &c.uuid, value)); self.meta_err(r); self.reload_lists();
    }
    /// The tracker's menu for several chosen rows (row 530; 1.4.2's "Set <column>" and "Untrack n"): untrack them all;
    /// give them all one value of a column in view - an option of a dropdown or a priority, a checkbox ticked or not,
    /// a date set to today - or clear it. What each row would set is kept for when a row is chosen.
    fn bulk_rows(&mut self, n: usize) -> Vec<crate::ctxmenu::Row> {
        use crate::ctxmenu::{named, Row, BULK_SET, BULK_UNTRACK}; use serde_json::Value;
        let mut rows = vec![named(BULK_UNTRACK, format!("Untrack {n} Items"))]; self.bulk.clear();
        let Some(t) = self.tracker.as_ref() else { return rows }; let today = { let c = crate::format::local(crate::format::now_secs()); crate::itemlist::midnight_ms(c.year, c.month, c.day) };
        for (k, c) in t.list.cols.iter().enumerate() {
            let set: Vec<(String, Value)> = match crate::db::kind_of(&c.kind) {
                "dropdown" | "priority" if !c.options.is_empty() => c.options.iter().map(|o| (o.0.clone(), Value::String(o.0.clone()))).collect(),
                "checkbox" => vec![("Checked".into(), Value::Bool(true)), ("Unchecked".into(), Value::Bool(false))],
                "date" => vec![("Today".into(), Value::from(today))],
                _ => continue };
            if self.bulk.len() + set.len() + 1 > 40 { break }
            rows.push(Row::Sep);
            for (label, v) in set.into_iter().chain([("Clear".to_string(), Value::Null)]) { rows.push(named(BULK_SET + self.bulk.len() as u32, format!("{}: {label}", c.name))); self.bulk.push((k, v)); }
        }
        rows
    }
    /// A row of that menu was chosen: every chosen row of the tracker gets it (one whose section has no such column is passed over).
    fn bulk_chosen(&mut self, id: u32) {
        use crate::ctxmenu::{BULK_SET, BULK_UNTRACK};
        let Some(paths) = self.tracker.as_ref().map(|t| t.sel.clone()) else { return }; let set = (id >= BULK_SET).then(|| self.bulk.get((id - BULK_SET) as usize).cloned()).flatten();
        if id != BULK_UNTRACK && set.is_none() { return }
        let mut fault = None;
        for path in &paths {
            let r = match &set {
                None => self.tracker.as_ref().and_then(|t| t.home(path)).and_then(|(job, sec)| crate::db::with(|d| d.set_tracked(&job, path, &sec, false))),
                Some((k, v)) => self.cell_target(path, *k).and_then(|(job, sec, c)| crate::db::with(|d| d.set_field(&job, path, &sec, &c.uuid, v.clone()))),
            };
            if let Some(Err(e)) = r { fault = Some(e); }
        }
        if let Some(e) = fault { self.toast.show(&e, min_ui::toast::ToastKind::Negative, Instant::now()); }
        if id == BULK_UNTRACK { if let Some(t) = self.tracker.as_mut() { t.sel.clear(); } }
        self.reload_lists();
    }
    /// A row of the item list's menus was chosen.
    fn meta_chosen(&mut self, id: u32) {
        use crate::ctxmenu::*;
        // the tracker's own rows: go to the item in its job; take it out of the tracker
        if let Some(it) = self.tracker.as_ref().and_then(|t| self.meta_ctx.1.and_then(|i| t.rows.get(i).cloned())) {
            match id { TRK_RELINK => self.ask_relink(it.path), TRK_REMOVE => { let r = crate::db::with(|d| d.remove_item(&it.path)); self.meta_err(r); self.reload_lists(); }
                TRK_OPEN => self.open_tracked(it.path), META_TRACK => { if let Some((job, sec)) = self.tracker.as_ref().and_then(|t| t.home(&it.path)) { let r = crate::db::with(|d| d.set_tracked(&job, &it.path, &sec, false)); self.meta_err(r); self.reload_lists(); } } _ => {} }
            return
        }
        let Some((job, sec)) = self.section_key() else { return }; let (col, item, hidden, trashed) = self.meta_ctx.clone();
        // the board: the section as lanes grouped by a column, or as the list again
        if id == BOARD_LIST { self.board_of.remove(&(job, sec)); self.items_reload(); return }
        if id >= BOARD_BY { if let Some(by) = self.board_ctx.get((id - BOARD_BY) as usize).cloned() { self.board_of.insert((job, sec), by); self.cancel_cell(); self.items_reload(); } return }
        let c = col.and_then(|k| self.items.cols.get(k).cloned());
        match id {
            META_EDIT => if let Some(c) = c { self.ask_column(Some(c.id), &c.kind, &c.name, &c.options); },
            META_LEFT | META_RIGHT => if let Some(k) = col { let to = if id == META_LEFT { k.checked_sub(1) } else { Some(k + 1).filter(|t| *t < self.items.cols.len()) };
                if let Some(to) = to { let mut names: Vec<String> = self.items.cols.iter().map(|c| c.name.clone()).collect(); names.swap(k, to); names.extend(self.items.hidden.iter().map(|c| c.name.clone()));
                    // (this user's own order: each column's place, as 1.4.2 writes it)
                    for (n, name) in names.iter().enumerate() { let r = crate::db::with(|d| d.set_layout(&job, &sec, name, Some(n as i64), None, None)); self.meta_err(r); } } },
            META_HIDE => if let Some(c) = c { let r = crate::db::with(|d| d.set_layout(&job, &sec, &c.name, None, None, Some(false))); self.meta_err(r); },
            META_TRASH => if let Some(c) = c { let r = crate::db::with(|d| d.trash_column(c.id, true)); self.meta_err(r); },
            META_MODIFIED => { self.items.show_modified = !self.items.show_modified; let on = self.items.show_modified; crate::settings::with(|s| s.set_bool("columns/items_modified", on)); },
            META_TRACK => if let Some(it) = item.and_then(|i| self.job.as_ref().and_then(|j| j.items.get(i)).cloned()) { let on = !self.items.tracked(&it); let r = crate::db::with(|d| d.set_tracked(&job, &it.path, &sec, on)); self.meta_err(r); },
            META_CLEAR => if let (Some(c), Some(it)) = (c, item.and_then(|i| self.job.as_ref().and_then(|j| j.items.get(i)).cloned())) { let r = crate::db::with(|d| d.set_field(&job, &it.path, &sec, &c.uuid, serde_json::Value::Null)); self.meta_err(r); },
            id if (META_ADD..META_ADD + 8).contains(&id) => { let kind = ["text", "dropdown", "date", "priority", "number", "checkbox", "links", "notes"][(id - META_ADD) as usize]; self.ask_column(None, kind, "", &[]); return }
            id if (META_SHOW..META_RESTORE).contains(&id) => if let Some(name) = hidden.get((id - META_SHOW) as usize) { let r = crate::db::with(|d| d.set_layout(&job, &sec, name, None, None, Some(true))); self.meta_err(r); },
            id if (META_RESTORE..META_OPTION).contains(&id) => if let Some((cid, _)) = trashed.get((id - META_RESTORE) as usize) { let r = crate::db::with(|d| d.trash_column(*cid, false)); self.meta_err(r); },
            id if id >= META_OPTION => if let (Some(c), Some(it)) = (c, item.and_then(|i| self.job.as_ref().and_then(|j| j.items.get(i)).cloned())) { if let Some(o) = c.options.get((id - META_OPTION) as usize) { let r = crate::db::with(|d| d.set_field(&job, &it.path, &sec, &c.uuid, serde_json::Value::String(o.0.clone()))); self.meta_err(r); } },
            _ => {}
        }
        self.items_reload();
    }
    fn job_choose(&mut self, i: usize) { if let Some(j) = self.job.as_mut() { if i < j.items.len() && j.chosen != Some(i) { j.chosen = Some(i); self.job_follow(true, true); } } }
    /// "Add" in the section in front: what it is called and whether there is one (the section's kind, by the project type).
    fn job_add_label(&self) -> Option<&'static str> {
        use ufb_projtype::Kind; let sec = self.job.as_ref()?.section.as_deref()?;
        match crate::jobview::type_for(&self.job.as_ref()?.path).ty.kind(sec) { Kind::Shot => Some("Add Shot"), Kind::Dated => Some("Add Item"), Kind::Folder => Some("Add Folder"), Kind::None => None }
    }
    fn ask_add_item(&mut self) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        let (Some(title), Some(dir)) = (self.job_add_label(), self.job.as_ref().and_then(|j| j.section_dir())) else { return };
        if !crate::ops::allowed_in(&dir) { self.toast.show("This build changes nothing outside the folders it was started with.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        let mut d = Dialog::new(title, "", vec![("Cancel", Variant::Secondary), ("Add", Variant::Primary)]).with_field("Name", ""); d.show_with("", &mut self.fonts);
        self.dialog = Some((d, Asked::AddItem(dir))); self.dialog_preview();
    }
    /// A link of a Links cell (row 584): an address of the web goes to the system; one of the application's own is gone to here.
    fn open_link(&mut self, link: &str) { if crate::weblink::own(link) { self.open_text(link); } else { crate::sysopen::open_url(link); } }
    /// "New UFB Folder" (row 590, 1.4.2's): a label; the folder is `<YYMMDD><letter>_<label>`.
    fn ask_new_dated(&mut self) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        if self.dialog.is_some() || self.prefs.is_some() { return }
        let dir = self.pane().path().to_path_buf();
        if !crate::ops::allowed_in(&dir) { self.toast.show("This build changes nothing outside the folders it was started with.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        let mut d = Dialog::new("New UFB Folder", "", vec![("Cancel", Variant::Secondary), ("Create", Variant::Primary)]).with_field("Name", ""); d.show_with("", &mut self.fonts);
        self.dialog = Some((d, Asked::NewDated(dir))); self.dialog_preview();
    }
    /// "New Date Folder" / "New Time Folder" (row 589, 1.4.2's): `YYMMDD` or `HHMM` by this machine's clock, made at once.
    fn new_stamped(&mut self, time: bool) {
        if self.dialog.is_some() || self.prefs.is_some() { return }
        let dir = self.pane().path().to_path_buf(); let c = crate::format::local(crate::format::now_secs());
        let base = if time { format!("{:02}{:02}", c.hour, c.minute) } else { crate::jobview::today() };
        crate::ops::submit(self.id, crate::ops::Op::NewStamped { dir, base });
    }
    /// "New Web Link" (row 585, 1.4.2's): the address - the clipboard's when it holds one - and a name; "New minNote" (row 587).
    fn ask_new_file(&mut self, note: bool) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        if self.dialog.is_some() || self.prefs.is_some() { return }
        let dir = self.pane().path().to_path_buf();
        if !crate::ops::allowed_in(&dir) { self.toast.show("This build changes nothing outside the folders it was started with.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        if note { let mut d = Dialog::new("New minNote", "", vec![("Cancel", Variant::Secondary), ("Create", Variant::Primary)]).with_field("Name", ""); d.show_with("", &mut self.fonts); self.dialog = Some((d, Asked::NewNote(dir))); }
        else {
            let clip = if std::env::var_os("UFB_DEMO").is_some() { String::new() } else { arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default() };
            let mut d = Dialog::new("New Web Link", "", vec![("Cancel", Variant::Secondary), ("Create", Variant::Primary)]).with_field("Address", "https://\u{2026} or an application's own address").with_field("Name", "Optional: the site's name is used");
            d.show_with(if crate::weblink::looks_like(&clip) { clip.trim() } else { "" }, &mut self.fonts); self.dialog = Some((d, Asked::NewLink(dir)));
        }
        self.dialog_preview();
    }
    fn ask_new_job(&mut self) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        let parent = self.pane().path().to_path_buf(); if !crate::jobview::is_jobs_folder(&parent) { self.toast.show("A new job is made in a jobs folder.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        if !crate::ops::allowed_in(&parent) { self.toast.show("This build changes nothing outside the folders it was started with.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        let mut d = Dialog::new("New Job", "", vec![("Cancel", Variant::Secondary), ("Create", Variant::Primary)]).with_field("Number", "").with_field("Name", ""); d.show_with("", &mut self.fonts);
        self.dialog = Some((d, Asked::NewJob(parent))); self.dialog_preview();
    }
    /// A question with fields says what the answer would make - the very folder name (1.4.2's preview differed from
    /// what was made for runs of spaces, and always showed the letter `a`) - and its last button waits for a usable answer.
    fn dialog_preview(&mut self) {
        let Some((d, asked)) = self.dialog.as_mut() else { return }; let ty = match asked { Asked::NewJob(p) | Asked::AddItem(p) => crate::jobview::type_for(p).ty, _ => crate::jobview::type_for(Path::new("")).ty };
        let made = match asked {
            Asked::NewJob(_) => Some(ty.job_folder_name(&d.value_at(0), &d.value_at(1))),
            Asked::AddItem(dir) => { let name = d.value_at(0); let sec = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Some(if name.is_empty() { String::new() } else if ty.kind(&sec) == ufb_projtype::Kind::Dated { let taken = self.job.as_ref().map(|j| j.dated_names.clone()).unwrap_or_default(); ufb_projtype::dated_name(&taken, &crate::jobview::today(), &name).unwrap_or_default() } else { name }) }
            _ => None,
        };
        if let Asked::BatchRename(items) = asked {
            // (the fields as typed, spaces and all: a space may be what is to be found or put in)
            let (find, with) = (d.fields[0].1.text().to_string(), d.fields[1].1.text().to_string()); let pairs = Self::batch_names(items, &find, &with);
            let fault = pairs.iter().find_map(|(_, n)| crate::ops::name_fault(n)); let mut seen = std::collections::HashSet::new(); let twice = pairs.iter().any(|(p, n)| !seen.insert(p.with_file_name(n).to_string_lossy().to_lowercase()));
            d.body = if find.is_empty() { "Type what is to be found in the names.".into() } else if pairs.is_empty() { "No name holds that.".into() } else if let Some(why) = &fault { why.clone() } else if twice { "Two of them would get the same name.".into() }
                else { let (p, n) = &pairs[0]; format!("{}  \u{2192}  {n}   \u{b7}   {} of {} change", p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), crate::pane::group(pairs.len()), crate::pane::group(items.len())) };
            if let Some(b) = d.buttons.last_mut() { b.enabled = !pairs.is_empty() && fault.is_none() && !twice; }
            return
        }
        match asked {
            Asked::NewDated(dir) => { let name = d.value_at(0); let made = if name.is_empty() { Err(String::new()) } else { crate::ops::dated_folder_name(dir, &name) };
                d.body = match &made { Ok(f) => format!("Folder:  {f}"), Err(why) if why.is_empty() => "Folder:".into(), Err(why) => why.clone() }; if let Some(b) = d.buttons.last_mut() { b.enabled = made.is_ok(); } return }
            Asked::NewNote(dir) => { let name = d.value_at(0); let file = if name.is_empty() { Err(String::new()) } else { crate::mnote::file_name(&crate::mnote::names_in(dir), &crate::jobview::today(), &name) };
                d.body = match &file { Ok(f) => format!("File:  {f}"), Err(why) if name.is_empty() || why.is_empty() => "File:".into(), Err(why) => why.clone() }; if let Some(b) = d.buttons.last_mut() { b.enabled = file.is_ok(); } return }
            Asked::NewLink(_) => { let (url, name) = (d.value_at(0), d.value_at(1)); let to = crate::weblink::address(&url);
                d.body = match &to { Some(u) => format!("File:  {}", crate::weblink::file_name(&name, u)), None if url.is_empty() => "File:".into(), None => "A site's name, or an address with its scheme. A file or a program is not linked to.".into() }; if let Some(b) = d.buttons.last_mut() { b.enabled = to.is_some(); } return }
            _ => {}
        }
        if let Some(name) = made { let ok = ufb_projtype::check_name("name", &name).is_ok(); d.body = if name.is_empty() { "Folder:".into() } else { format!("Folder:  {name}") }; if let Some(b) = d.buttons.last_mut() { b.enabled = ok; } }
    }
    /// What was made off the window's thread has come.
    fn on_made(&mut self, m: Made) {
        self.making = false; let port = self.port.clone();
        let n = self.shown(); for p in self.panes.iter_mut().take(n) { p.activated(&port); }
        match m {
            Made::Shown(p, there) => if there { if std::env::var_os("UFB_DEMO").is_some() { eprintln!("UFB_DEMO: shown in the file manager: {}", p.display()); } else { crate::sysopen::reveal(&[p]); } } else { self.toast.show("That link leads to nothing that is there now.", min_ui::toast::ToastKind::Notice, Instant::now()); },
            Made::Relink(old, Some(new)) => self.relink(old, new),
            Made::Relink(_, None) => {}
        }
    }
    /// A tracked item whose folder is gone is pointed at where it went (row 439). When that is an item of a subscribed
    /// job - `<job>/<section>/<item>` - the row is that job's and that section's from then on.
    fn relink(&mut self, old: PathBuf, new: PathBuf) {
        let home = new.parent().and_then(|sec| Some((sec.parent()?.to_path_buf(), sec.file_name()?.to_string_lossy().into_owned()))).filter(|(job, _)| self.jobs.iter().any(|j| &j.path == job));
        let r = crate::db::with(|d| d.relink(&old, &new, home.as_ref().map(|(j, s)| (j.as_path(), s.as_str())))); self.meta_err(r); self.reload_lists();
    }
    /// The system's folder chooser, for Relink: asked for on a thread of its own and answered later (minNotes' way: a
    /// chooser run from inside an event of the window's loop meets the loop's own wake-ups).
    fn ask_relink(&mut self, old: PathBuf) {
        let (tx, port) = (self.made_tx.clone(), self.port.clone()); let title = format!("Where did \u{201c}{}\u{201d} go?", old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let start = old.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf);
        let _ = std::thread::Builder::new().name("ufb-chooser".into()).spawn(move || { let mut d = rfd::FileDialog::new().set_title(title); if let Some(s) = start { d = d.set_directory(s); } let _ = tx.send(Made::Relink(old, d.pick_folder())); port.send(Msg::Icon); });
    }
    /// What the engine renamed or moved (rows 439, 493): the metadata that hangs on those paths goes along - an
    /// item's values and its star, a section's and a job's columns, a subscription - and so does what this window
    /// shows of it: the job in a tab, its section, its chosen item. An undo is the same step the other way.
    fn follow_moves(&mut self, moved: &[(PathBuf, PathBuf)]) {
        if moved.is_empty() { return }
        for (from, to) in moved {
            let r = crate::db::with(|d| d.move_paths(from, to).map(|_| ())); self.meta_err(r);
            let carry = |p: &Path| p.strip_prefix(from).ok().map(|rel| if rel.as_os_str().is_empty() { to.clone() } else { to.join(rel) });
            let renamed_in_place = from.parent() == to.parent();
            for j in self.job.iter_mut().chain(self.tabs.iter_mut().filter_map(|t| t.job.as_mut())) {
                // a section of the job renamed in place: the one in front keeps being in front under its new name
                if renamed_in_place && from.parent() == Some(j.path.as_path()) { if let (Some(a), Some(b)) = (from.file_name(), to.file_name()) { if j.section.as_deref() == Some(&*a.to_string_lossy()) { j.section = Some(b.to_string_lossy().into_owned()); } } }
                if let Some(c) = j.chosen_item().and_then(|i| carry(&i.path)) { j.choose = Some(c); }
                if let Some(p) = carry(&j.path) { if j.path.file_name().is_some_and(|n| n.to_string_lossy() == j.name.as_str()) { j.name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(); } j.path = p; }
                j.reload();
            }
            self.board_of = std::mem::take(&mut self.board_of).into_iter().map(|((job, sec), by)| { let sec = if renamed_in_place && from.parent() == Some(job.as_path()) && from.file_name().is_some_and(|a| a.to_string_lossy() == sec.as_str()) { to.file_name().map(|b| b.to_string_lossy().into_owned()).unwrap_or(sec) } else { sec }; ((carry(&job).unwrap_or(job), sec), by) }).collect();
        }
        self.reload_jobs(); self.reload_lists();
    }
    /// A folder becomes a job of the sidebar, or leaves it (its metadata is not touched).
    fn subscribe(&mut self, path: PathBuf, on: bool) {
        let r = crate::db::with(|d| if on { d.subscribe(&path, "") } else { d.unsubscribe(&path) });
        match r { Some(Ok(())) => {} Some(Err(e)) => self.toast.show(&e, min_ui::toast::ToastKind::Negative, Instant::now()), None => self.toast.show("The database could not be opened.", min_ui::toast::ToastKind::Negative, Instant::now()) }
        self.reload_jobs();
        if !on && self.job.as_ref().is_some_and(|j| j.path == path) { self.job = None; self.focus = Focus::Pane; for p in self.panes.iter_mut() { p.root = None; } }
    }
    /// The kind of panel pane `i` is, for its columns: the browser's, a job's first, a job's second.
    fn col_kind(&self, i: usize) -> usize { if self.job.is_none() { 0 } else if i == 0 { 1 } else { 2 } }
    /// Each pane in view has the columns of its kind of panel; a divider dragged in one is kept for the kind (and so
    /// for the other panes of it), across launches.
    fn sync_columns(&mut self) {
        const KEYS: [&str; 3] = ["columns/browser", "columns/job_top", "columns/job_low"];
        for i in 0..self.shown() {
            let kind = self.col_kind(i); let want = self.col_layouts[kind]; let p = &mut self.panes[i];
            if p.col_kind != Some(kind) { p.set_columns(want); p.col_kind = Some(kind); continue }
            let now = p.columns_now();
            if now != p.col_layout { p.col_layout = now; self.col_layouts[kind] = now; crate::settings::with(|s| s.set_str(KEYS[kind], &now.to_text())); }
            else if want != now { p.set_columns(want); }
        }
    }
    /// A column of the active pane's kind of panel is shown or hidden.
    fn toggle_column(&mut self, c: usize) {
        if !(1..4).contains(&c) { return } let kind = self.col_kind(self.active.min(self.panes.len() - 1));
        self.col_layouts[kind].show[c] = !self.col_layouts[kind].show[c]; let text = self.col_layouts[kind].to_text();
        crate::settings::with(|s| s.set_str(["columns/browser", "columns/job_top", "columns/job_low"][kind], &text));
    }
    // ---------------------------------------------------------------- the Sync entry (row 499)
    fn ask_sync(&mut self) {
        use min_ui::{dialog::Dialog, text_button::Variant}; let (farm, node, only) = crate::settings::with(|s| (s.string("sync/farm"), s.string("sync/node"), s.string("sync/only")));
        let mut d = Dialog::new("Sync", "The farm's folder is the one every machine shares; each machine has a name of its own in it. Saving turns sync on.", vec![("Cancel", Variant::Secondary), ("Save", Variant::Primary)])
            .with_field("Farm folder", "a folder on shared storage").with_field("This machine's name", &crate::mesh::node_name(""));
        // (the development build's belt for sync: not a setting of the test package or of the release)
        let belt = crate::identity::belt(); if belt { d = d.with_field("Only under (development build)", "a folder; empty: everything shared"); }
        d.show_with(&farm, &mut self.fonts); d.fields[1].1.set_text_end(&node, &mut self.fonts); if belt { d.fields[2].1.set_text_end(&only, &mut self.fonts); } self.dialog = Some((d, Asked::SyncSet));
    }
    /// The Settings card (row 516): opened over the window; Save applies what it holds to this window's panes and, when
    /// sync's settings changed, starts sync again.
    fn open_prefs(&mut self) { if self.prefs.is_some() || self.dialog.is_some() || self.lightbox.is_open() { return } self.ctx.close(); self.end_rename(); self.leave_path(); self.leave_search(); self.op_list = false; self.prefs = Some(crate::prefs::Prefs::open(&mut self.fonts, &self.shares, &self.share_states)); }
    fn prefs_done(&mut self, out: crate::prefs::Out) {
        match out { crate::prefs::Out::None => {} crate::prefs::Out::Close => self.prefs = None,
            crate::prefs::Out::Saved { sync, pairs, rekeys } => { self.prefs = None; self.apply_view(); self.apply_second();
                // a saved share's pair as the card has it now; rows under a changed pair go to its new identity; then every path is read anew
                let mut changed = false; for (id, win, mac) in pairs { if let Some(sh) = self.shares.iter_mut().find(|s| s.id == id) { if (sh.win.as_str(), sh.mac.as_str()) != (win.as_str(), mac.as_str()) { sh.win = win; sh.mac = mac; changed = true; } } }
                if changed { crate::shares::save(&self.shares); }
                for (old, new) in &rekeys { if let Some(Err(e)) = crate::db::with(|d| d.rekey(old, new)) { self.toast.show(&format!("The metadata could not be moved to the new mapping: {e}"), min_ui::toast::ToastKind::Negative, Instant::now()); } }
                self.apply_shares(); if !rekeys.is_empty() { self.reload_jobs(); self.reload_lists(); if let Some(j) = self.job.as_mut() { j.reload(); } }
                if sync { self.apply_sync(); } } }
    }
    /// The panes are made what the settings say they show (after Save; a new pane is made so from the start).
    /// The setting for the second pane was changed: the browser view in front follows it (a job decides for itself).
    fn apply_second(&mut self) { let on = crate::settings::with(|s| s.bool("view/second_pane")); if crate::prefs::second_changed() && self.job.is_none() && self.tracker.is_none() && self.two != on { self.set_two(on); } }
    fn apply_view(&mut self) {
        let (hidden, folders_first, sequences, grid) = crate::prefs::view(); self.hidden = hidden; self.folders_first = folders_first; let mode = if grid { Mode::Grid } else { Mode::List };
        for p in self.panes.iter_mut() { p.set_hidden(hidden); p.set_folders_first(folders_first); p.set_sequences(sequences); if p.mode != mode { p.set_mode(mode); } p.view_synced(); }
        for p in self.hidden_panes() { p.set_hidden(hidden); p.set_folders_first(folders_first); p.set_sequences(sequences); if p.mode != mode { p.set_mode(mode); } }
    }
    /// Sync is made what the settings say (row 500): stopped, started, or started again with the new settings.
    fn apply_sync(&mut self) {
        let (on, farm, node, leader, only) = crate::settings::with(|s| (s.bool("sync/on"), s.string("sync/farm"), s.string("sync/node"), s.string("sync/leader"), s.string("sync/only"))); let now = Instant::now();
        // (a node that runs is stopped first: it starts again with what the settings say now, or stays off)
        if crate::mesh::running() { crate::mesh::stop(); self.reload_lists(); } Self::set_type_dirs();
        if !on { return } if farm.trim().is_empty() { self.ask_sync(); return }
        if !Path::new(farm.trim()).is_dir() { self.toast.show("The farm's folder is not there.", min_ui::toast::ToastKind::Negative, now); return }
        crate::mesh::start_with(Path::new(farm.trim()), &node, &leader, (!only.trim().is_empty()).then(|| Path::new(only.trim())));
        let p = self.port.clone(); crate::mesh::on_change(Arc::new(move || p.send(Msg::Icon))); self.mesh_rev = crate::mesh::rev(); Self::set_type_dirs();
    }
    fn sync_chosen(&mut self, id: u32) {
        use crate::ctxmenu::*;
        match id { SYNC_SET => self.ask_sync(),
            SYNC_ONOFF => { let on = crate::settings::with(|s| { let on = !s.bool("sync/on"); s.set_bool("sync/on", on); on }); if on || crate::mesh::running() { self.apply_sync(); } }
            SYNC_AUTO | SYNC_LEAD | SYNC_NEVER => { crate::settings::with(|s| s.set_str("sync/leader", match id { SYNC_LEAD => "leader", SYNC_NEVER => "noleader", _ => "" })); if crate::mesh::running() { self.apply_sync(); } }
            _ => {} }
    }
    /// The Sync row's words: what it is doing, or that it is off.
    pub fn sync_words() -> String {
        match crate::mesh::status() { Some(m) => if m.leading { if m.peers == 0 { "alone".to_string() } else { format!("leading {}", m.peers) } } else { match &m.leader { Some(l) => format!("with {l}"), None => "looking\u{2026}".to_string() } },
            None => if crate::settings::with(|s| s.bool("sync/on")) { "not set up".to_string() } else { "off".to_string() } }
    }
    // ---------------------------------------------------------------- saved shares (row 498; `shares.rs`)
    /// How the shares stand is asked of the system, off this thread.
    fn refresh_shares(&mut self) {
        let (shares, tx, port) = (self.shares.clone(), self.share_tx.clone(), self.port.clone()); if shares.is_empty() { self.apply_shares(); return }
        let _ = std::thread::Builder::new().name("ufb-shares".into()).spawn(move || { let mounts = crate::shares::mounts(); let _ = tx.send(ShareNews::Holders(shares.iter().filter_map(|s| crate::shares::placeholder(s).map(|p| (s.id.clone(), p))).collect())); let _ = tx.send(ShareNews::States(shares.iter().map(|s| (s.id.clone(), crate::shares::resolve(s, &mounts, &|p| p.is_dir()))).collect())); port.send(Msg::Icon); });
    }
    /// The shares, as they stand now, are what paths are read by: each one's place on this machine, its folders as jobs.
    fn apply_shares(&mut self) {
        // (the system's mount table is read here only for hand-written mappings: a saved share's place came with its state)
        let mounts = if crate::shares::load_manual().iter().any(|m| m.on) { crate::shares::mounts() } else { vec![] };
        let maps = crate::shares::mappings_now(&self.shares, &self.share_states, &mounts);
        if maps != crate::db::mappings() { crate::db::set_mappings(maps);
            // (rows of this machine's own under a share that came are the share's from now on; the lists are read by the new places)
            let _ = crate::db::with(|d| d.classify(&crate::db::mappings())); self.reload_jobs(); self.reload_lists(); if let Some(j) = self.job.as_mut() { j.reload(); } }
        crate::jobview::set_share_roots(self.shares.iter().zip(self.share_states.iter()).filter(|(s, _)| s.jobs).filter_map(|(s, st)| st.root().map(|p| (p.to_path_buf(), s.ptype.clone()))).collect());
        // (the sections' labels are the type's: read again)
        self.job_secs_for.clear();
        // the shares marked for it are asked for once, silently, when the application starts
        if !std::mem::replace(&mut self.shares_launched, true) { for i in 0..self.shares.len() { if self.shares[i].at_launch && !self.shares[i].server.is_empty() && self.share_states.get(i) == Some(&crate::shares::State::NotConnected) { self.connect_share(i, false); } } }
    }
    /// The system is asked to connect a share (`asked`: by the user - its own sign-in may appear, and a failure is said).
    fn connect_share(&mut self, i: usize, asked: bool) {
        let Some(sh) = self.shares.get(i).cloned() else { return }; if sh.server.is_empty() || !self.share_busy.insert(sh.id.clone()) { return }
        if let Some(st) = self.share_states.get_mut(i) { *st = crate::shares::State::Connecting; }
        // (said in the log, with what came of it: a share that keeps being asked for must be readable there afterwards)
        eprintln!("UFB: share {:?}: connecting ({})", sh.name, if asked { "asked for by hand: the system may ask who signs in" } else { "by itself, silently" });
        let (tx, port) = (self.share_tx.clone(), self.port.clone());
        let _ = std::thread::Builder::new().name("ufb-share-connect".into()).spawn(move || { let (st, mounted) = crate::shares::connect(&sh, asked); let _ = tx.send(ShareNews::Connected(sh.id.clone(), st, asked, mounted)); port.send(Msg::Icon); });
    }
    /// The stale placeholder that holds a share's name is removed (the system asks for an administrator's password).
    fn fix_share(&mut self, i: usize) {
        let Some(sh) = self.shares.get(i).cloned() else { return }; if !self.share_busy.insert(sh.id.clone()) { return }
        let (tx, port) = (self.share_tx.clone(), self.port.clone());
        let _ = std::thread::Builder::new().name("ufb-share-fix".into()).spawn(move || { let r = crate::shares::fix(&sh); let _ = tx.send(ShareNews::Fixed(sh.id.clone(), r)); port.send(Msg::Icon); });
    }
    fn share_news(&mut self, n: ShareNews) {
        let now = Instant::now();
        match n {
            ShareNews::Holders(h) => { self.share_holders = h.into_iter().collect(); return }
            ShareNews::Fixed(id, r) => { self.share_busy.remove(&id); match r { Ok(()) => { self.share_holders.remove(&id);
                    // not connected: it is asked for now, and lands under its own name; connected under another: it keeps that until it is connected again
                    if let Some(i) = self.shares.iter().position(|s| s.id == id) { if self.share_states.get(i).is_some_and(|st| st.root().is_none()) { self.connect_share(i, true); } else { self.toast.show("Removed. The share takes its own name the next time it is connected.", min_ui::toast::ToastKind::Positive, now); } } }
                Err(why) => self.toast.show(&format!("It could not be fixed: {why}"), min_ui::toast::ToastKind::Negative, now) } self.refresh_shares(); return }
            ShareNews::States(states) => { let mut again = vec![]; for (id, st) in states { if self.share_busy.contains(&id) { continue } if let Some(i) = self.shares.iter().position(|s| s.id == id) { if self.share_states.len() <= i { self.share_states.resize(i + 1, crate::shares::State::NotConnected); }
                // (what a request said - a sign-in is needed, the server does not answer - stands until the share is there)
                // (one that is gone is no longer ours: whoever connects it next owns it)
                if st.root().is_none() && self.shares[i].ours { self.shares[i].ours = false; crate::shares::save(&self.shares); }
                // a share that was there in this run and dropped (the network went, the machine slept) is asked for ONCE, silently
                if st.root().is_some() { self.share_seen.insert(id.clone()); } else if self.share_seen.remove(&id) && self.shares[i].at_launch && !self.shares[i].server.is_empty() { again.push(i); }
                if st.root().is_some() || !matches!(self.share_states[i], crate::shares::State::SignIn | crate::shares::State::Unreachable(_)) { self.share_states[i] = st; } } }
                for i in again { eprintln!("UFB: share {:?} was there and is gone: asked for once more", self.shares[i].name); self.connect_share(i, false); } },
            ShareNews::Connected(id, st, asked, mounted) => { self.share_busy.remove(&id); if let Some(i) = self.shares.iter().position(|s| s.id == id) {
                eprintln!("UFB: share {:?}: {}{} ({})", self.shares[i].name, st.words(), if let crate::shares::State::Unreachable(why) = &st { format!(": {why}") } else { String::new() }, if mounted { "connected by this request" } else if st.root().is_some() { "it was there already" } else { "not connected" });
                // connected by this request: it is ours to disconnect (one that was there already is not)
                if st.root().is_some() { self.share_seen.insert(id.clone()); }
                if mounted { self.shares[i].ours = true; if cfg!(windows) { if let Some(l) = st.root().and_then(|r| crate::shares::letter_of(&r.to_string_lossy().chars().take(2).collect::<String>())) { self.shares[i].letter = l; } } crate::shares::save(&self.shares); }
                if asked { if let crate::shares::State::Unreachable(why) = &st { self.toast.show(why, min_ui::toast::ToastKind::Negative, now); } }
                if self.share_states.len() <= i { self.share_states.resize(i + 1, crate::shares::State::NotConnected); } self.share_states[i] = st; } places::refresh(&self.port); }
        }
        self.apply_shares();
    }
    fn add_share(&mut self, text: &str) {
        match crate::shares::parse(text) {
            Ok(sh) => { if self.shares.iter().any(|o| (!sh.server.is_empty() && o.server.eq_ignore_ascii_case(&sh.server) && o.share.eq_ignore_ascii_case(&sh.share)) || (!sh.local.is_empty() && o.local == sh.local)) { self.toast.show("That share is in the sidebar already.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
                self.shares.push(sh); self.share_states.push(crate::shares::State::NotConnected); crate::shares::save(&self.shares); let i = self.shares.len() - 1;
                // (asked for now, as the user's own request: the system may ask who signs in)
                if !self.shares[i].server.is_empty() { self.connect_share(i, true); } else { self.refresh_shares(); } }
            Err(why) => self.toast.show(&why, min_ui::toast::ToastKind::Notice, Instant::now()),
        }
    }
    /// What Copy Path / Copy Name / Copy Link put on the clipboard: a line each; a path in the share's own spelling
    /// (never a landing's name, row 445), a link every machine of the studio can open.
    fn copy_text(id: u32, paths: &[PathBuf]) -> Option<String> {
        let lines: Vec<String> = paths.iter().map(|p| match id { crate::ctxmenu::COPY_NAME => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string_lossy().into_owned()), crate::ctxmenu::COPY_LINK => crate::db::link_to(p), crate::ctxmenu::COPY_UNION => crate::db::union_link_to(p), _ => crate::db::canonical(p) }).collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
    /// To what a link or a typed path leads: a folder is gone to; a file's folder, with the file chosen.
    fn go_file(&mut self, file: PathBuf) {
        let (Some(dir), Some(name)) = (file.parent().map(Path::to_path_buf), file.file_name().map(|n| n.to_string_lossy().into_owned())) else { return };
        if self.job.is_some() || self.tracker.is_some() { let inside = !self.blank() && self.pane().root.clone().is_some_and(|r| dir.starts_with(&r)); if !inside { self.new_tab(dir.clone()); } }
        let port = self.port.clone(); self.pane().go_select(&port, dir, name);
    }
    /// A jobs folder's project type was chosen (0: the built-in one; else the k-th of the types found). One that cannot
    /// be read is not taken, and why is said.
    fn share_type_chosen(&mut self, k: usize) {
        let Some(i) = self.share_menu.filter(|i| *i < self.shares.len()) else { return };
        let name = if k == 0 { String::new() } else { match crate::jobview::known_types().get(k - 1) { Some(t) => t.0.clone(), None => return } };
        if !name.is_empty() { if let Err(why) = crate::jobview::load_type(&name) { self.toast.show(&format!("That type cannot be used: {why}"), min_ui::toast::ToastKind::Negative, Instant::now()); return } }
        self.shares[i].ptype = name; crate::shares::save(&self.shares); self.apply_shares();
    }
    /// The places project types are looked for: the farm's folder, then this machine's settings folder.
    fn set_type_dirs() {
        let mut dirs = vec![]; if let Some(f) = crate::mesh::farm() { dirs.push(f.join("types")); } else { let f = crate::settings::with(|s| s.string("sync/farm")); if !f.trim().is_empty() { dirs.push(Path::new(f.trim()).join("types")); } }
        if let Some(d) = crate::identity::data_dir() { dirs.push(d.join("types")); }
        crate::jobview::set_type_dirs(dirs);
    }
    /// A row of a share's menu was chosen.
    fn share_chosen(&mut self, id: u32) {
        use crate::ctxmenu::*; let Some(i) = self.share_menu.filter(|i| *i < self.shares.len()) else { return };
        match id {
            SHARE_CONNECT => self.connect_share(i, true),
            // only what this application connected (the menu offers it for nothing else)
            SHARE_DISCONNECT => if let (true, Some(root)) = (self.shares[i].ours, self.share_states.get(i).and_then(|s| s.root()).map(Path::to_path_buf)) {
                // (a pane that shows it lets go of it first: the system does not disconnect what is in use)
                let home = places::home(); let port = self.port.clone(); for p in self.panes.iter_mut().chain(self.tabs.iter_mut().flat_map(|t| t.panes.iter_mut())) { if p.path().starts_with(&root) { p.root = None; p.go(&port, home.clone()); } }
                if self.job.as_ref().is_some_and(|j| j.path.starts_with(&root)) { self.job = None; self.tracker = None; }
                self.share_seen.remove(&self.shares[i].id); let (tx, port, sh) = (self.share_tx.clone(), self.port.clone(), self.shares[i].clone()); self.shares[i].ours = false; crate::shares::save(&self.shares);
                let _ = std::thread::Builder::new().name("ufb-share-disconnect".into()).spawn(move || { std::thread::sleep(Duration::from_millis(300)); let r = crate::shares::disconnect(&root); let mounts = crate::shares::mounts(); let st = match r { Ok(()) => crate::shares::resolve(&sh, &mounts, &|p| p.is_dir()), Err(why) => crate::shares::State::Unreachable(format!("It could not be disconnected: {why}")) }; let st = if st.root().is_none() { st } else { crate::shares::resolve(&sh, &mounts, &|p| p.is_dir()) }; let _ = tx.send(ShareNews::States(vec![(sh.id.clone(), st)])); port.send(Msg::Icon); }); },
            SHARE_LAUNCH => { self.shares[i].at_launch = !self.shares[i].at_launch; crate::shares::save(&self.shares); }
            SHARE_JOBS => { self.shares[i].jobs = !self.shares[i].jobs; crate::shares::save(&self.shares); self.apply_shares(); }
            SHARE_RENAME => { use min_ui::{dialog::Dialog, text_button::Variant}; let mut d = Dialog::new("Rename the Share", "Its name in the sidebar. The share itself is not touched.", vec![("Cancel", Variant::Secondary), ("Rename", Variant::Primary)]).with_field("Name", ""); d.show_with(&self.shares[i].name.clone(), &mut self.fonts); self.dialog = Some((d, Asked::RenameShare(i))); }
            SHARE_LETTER => { use min_ui::{dialog::Dialog, text_button::Variant}; let mut d = Dialog::new("Drive Letter", "The letter the share is given the next time it is connected. Windows remembers the drive and brings it back at sign-in.", vec![("Cancel", Variant::Secondary), ("Set", Variant::Primary)]).with_field("Letter", "Z"); d.show_with(&self.shares[i].letter.clone(), &mut self.fonts); self.dialog = Some((d, Asked::ShareLetter(i))); }
            // (the share stays connected, as it is: only its row goes)
            SHARE_REMOVE => { self.shares.remove(i); if i < self.share_states.len() { self.share_states.remove(i); } crate::shares::save(&self.shares); self.apply_shares(); }
            _ => {}
        }
    }
    /// A text is being typed somewhere (a field of the window, or of an open question).
    fn text_focus(&self) -> bool { self.note.is_some() || self.chip.is_some() || self.coledit.is_some() || self.prefs.as_ref().is_some_and(|p| p.typing()) || matches!(self.focus, Focus::Path | Focus::Search | Focus::Rename | Focus::Cell) || self.dialog.as_ref().is_some_and(|(d, _)| !d.fields.is_empty()) }

    /// Every pane of the tabs that are behind.
    fn hidden_panes(&mut self) -> impl Iterator<Item = &mut Pane> { self.tabs.iter_mut().flat_map(|t| t.panes.iter_mut()) }

    /// What the menus show of this window.
    pub fn menu_state(&self) -> menu::State { let mut st = self.menu_state_of_pane(); if self.blank() { st.can_back = false; st.can_forward = false; st.can_up = false; st.chosen = 0; st.writable = false; st.other = false; } st }
    fn menu_state_of_pane(&self) -> menu::State { let p = &self.panes[self.active.min(self.panes.len() - 1)]; menu::State { grid: p.mode == Mode::Grid, hidden: self.hidden, folders_first: self.folders_first, second: self.two, sort: p.model.sort, can_back: p.history.can_back(), can_forward: p.history.can_forward(), can_up: p.can_up(),
        chosen: p.chosen_count().min(2) as u8, undo: crate::ops::undo_label().is_some(), redo: crate::ops::redo_label().is_some(), writable: crate::ops::allowed_in(p.path()),
        sequences: p.model.sequences, jobs_folder: !self.blank() && crate::jobview::is_jobs_folder(p.path()), columns: self.col_layouts[self.col_kind(self.active.min(self.panes.len() - 1))].show, typing: self.text_focus(), other: self.two && self.panes.len() > 1 && crate::ops::allowed_in(self.panes[1 - self.active.min(1)].path()) } }

    // ---------------------------------------------------------------- what the workers send
    /// Take in what the worker threads sent. True when anything came.
    pub fn poll(&mut self) -> bool {
        let mut any = self.icons.poll(); any |= self.thumbs.poll(); any |= self.lightbox.poll();
        // the job in front was read: the browser follows a section that came to the front; what was being made is there
        if let Some(j) = self.job.as_mut() { let was = j.two_tier; let before = j.chosen_item().map(|i| i.path.clone()); if j.poll() { any = true;
            // (a section that BECAME one of two tiers - its first shot was added - counts as one that came to the front)
            let moved = std::mem::take(&mut j.moved) || (j.two_tier && !was);
            let (want, after) = (j.wants_two(), j.chosen_item().map(|i| i.path.clone()));
            // a section that came to the front: the second pane as the section is - shown for two tiers, none for a flat one - unless the user said otherwise for it
            if moved && want != self.two { self.set_two(want); }
            // the browser goes to an item when ANOTHER one is chosen (one just made, a section that came to the front) -
            // not at every read of the job: a pane taken deeper by hand stays where it is
            if after.is_some() && (moved || after != before) { self.job_follow(true, true); }
            if let Some(j) = self.job.as_mut() { j.hold = false; }
            self.items_reload(); } }
        while let Ok(m) = self.made_rx.try_recv() { any = true; self.on_made(m); }
        if let Some(t) = self.tracker.as_mut() { any |= t.poll(); }
        while let Ok(n) = self.share_rx.try_recv() { any = true; self.share_news(n); }
        // something arrived from another machine (or left for one): the lists show it
        { let r = crate::tasks::rev(); if r != self.tasks_rev { self.tasks_rev = r; any = true; } }
        { let r = crate::mesh::rev(); if r != self.mesh_rev { self.mesh_rev = r; any = true; if self.list_mut().edit.is_none() && self.chip.is_none() && self.coledit.is_none() && !self.board.as_ref().is_some_and(|b| b.dragging()) { self.reload_lists(); } } }
        // what a page in the web view told: Escape pressed inside it closes the lightbox
        for m in self.lightbox.web_msgs() { if std::env::var_os("UFB_DEMO").is_some() { eprintln!("UFB_DEMO: the page: {m}"); } if m == "key:Escape" { self.close_lightbox(); break }
            // a link to the web that was pressed in the page: the system's browser has it (rows 460, 512) - not in a scripted run
            if let Some(url) = m.strip_prefix("open:") { if crate::webview::outward(url) && std::env::var_os("UFB_DEMO").is_none() { crate::sysopen::open_url(url); } } }
        // what a drag left for the loop: drops to carry out (the engine's work, as a paste is); a drag of ours that ended
        let (drops, ended, favs) = { let mut d = self.dnd.borrow_mut(); (std::mem::take(&mut d.drops), std::mem::take(&mut d.ended), std::mem::take(&mut d.favs)) };
        if !favs.is_empty() { let mut added = false; for f in favs { added |= crate::places::add_favorite(&f, ""); } if added { self.reload_favorites(); } any = true; }
        for d in drops { any = true; crate::ops::submit(self.id, crate::ops::Op::Transfer { paths: d.paths, dest: d.dest, mv: d.mv }); }
        // a drag of ours ended: the panes ask whether their folders changed - now, and a few times more, because the
        // application that took the files moves them after the drag is over (no watcher, row 438: a bounded look after OUR OWN drag)
        let now = Instant::now();
        if ended { any = true; self.recheck = [0u64, 250, 700, 1500, 3000, 6000].iter().map(|ms| now + Duration::from_millis(*ms)).collect(); }
        if self.recheck.first().is_some_and(|t| *t <= now) { self.recheck.retain(|t| *t > now); any = true; let port = self.port.clone(); let n = self.shown(); for p in self.panes.iter_mut().take(n) { p.activated(&port); } }
        while let Ok(m) = self.rx.try_recv() {
            any = true;
            match m {
                Msg::Places { volumes, root } => { self.volumes = volumes; self.root = root; self.crumbs_for = None; }
                Msg::Checked { req, path, dir, file } => if self.check == Some(req) { self.check = None; if dir { self.leave_path(); self.go(path); } else if file { self.leave_path(); self.go_file(path); } else { self.path_error = Some(("No such folder".into(), Instant::now())); self.field.error = true; } },
                Msg::Icon => { self.icons.poll(); self.thumbs.poll(); self.lightbox.poll(); }
                Msg::Op(rep) => self.on_report(&rep),
                Msg::Ask(q) => if q.origin == self.id { self.asks.push_back(q); },
                other => { let mut m = Some(other); let port = self.port.clone(); let thumbs = &mut self.thumbs; for p in self.panes.iter_mut().chain(self.tabs.iter_mut().flat_map(|t| t.panes.iter_mut())) { if p.on_msg(&port, &mut m) { if std::mem::take(&mut p.listed) { thumbs.listed(p.path()); } break } } }
            }
        }
        any
    }
    /// The window became active: the volumes are looked for again, and each pane asks whether its folder changed.
    pub fn activated(&mut self) { places::refresh(&self.port); self.refresh_shares(); let port = self.port.clone(); let n = self.shown(); for p in self.panes.iter_mut().take(n) { p.activated(&port); } self.reload_jobs(); if let Some(j) = self.job.as_mut() { j.reload(); } if let Some(t) = self.tracker.as_mut() { t.reload(); } }
    pub fn deactivated(&mut self) { if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } }

    // ---------------------------------------------------------------- the lightbox (`lightbox.rs`)
    /// The file at this row of a pane, large (a folder has none).
    fn open_lightbox(&mut self, pane: usize, row: usize) {
        let Some((path, name, size, kind, dir)) = self.panes.get(pane).and_then(|p| p.entry(row)) else { return }; if dir { return }
        self.end_rename(); self.leave_path(); self.leave_search(); self.lb_press = None; self.lb_key = None;
        let ext = crate::lightbox::ext_of(&path);
        // what the player plays: a sequence shown as one entry, a video, a sound
        let media = match self.panes[pane].sequence_at(row) { Some((pattern, start, end)) => crate::lightbox::Media::Sequence { pattern, start, end }, None => crate::lightbox::media_of(&ext) };
        let played = matches!(media, crate::lightbox::Media::Video | crate::lightbox::Media::Audio | crate::lightbox::Media::Sequence { .. });
        { let e = &mut self.presenter.engine; e.clear_picture(); }
        self.lightbox.open(path.clone(), name, size, kind, &ext, pane, played); self.lb_click = None;
        if played {
            let hw = self.hw.get_or_insert_with(|| { #[cfg(windows)] let dev = self.presenter.engine.dx12_device_raw(); #[cfg(not(windows))] let dev = None; qcv_player::HwDevice::platform(dev) }).clone();
            let player = crate::lightbox::Player::open(&media, &path, hw, self.presenter.engine.uploader());
            self.lightbox.attach(player);
        }
        // an HTML page: the system's own web view, where there is one (else its source stays, as text)
        if crate::webview::is_page(&ext) { let wake = { let p = self.port.clone(); Arc::new(move || p.send(Msg::Icon)) as Arc<dyn Fn() + Send + Sync> };
            match crate::webview::create(&self.window, self.web_rect(), self.scale(), &path, wake) { Ok(w) => self.lightbox.attach_web(w), Err(why) => if std::env::var_os("UFB_DEMO").is_some() { eprintln!("UFB_DEMO: no web view ({why}): the page's source is shown"); } } }
    }
    fn close_lightbox(&mut self) {
        if !self.lightbox.is_open() { return } self.lightbox.close(); self.lb_key = None; self.ctx.close();
        let e = &mut self.presenter.engine; e.clear_picture(); e.set_place(None); e.set_background(0);
    }
    /// The next file of the folder (or the one before): the pane's cursor goes with it.
    fn step_lightbox(&mut self, step: i32) {
        let Some((pane, path)) = self.lightbox.view.as_ref().map(|v| (v.pane, v.path.clone())) else { return };
        let Some(p) = self.panes.get_mut(pane) else { return }; let Some(next) = p.row_at_path(&path).and_then(|r| p.next_file(r, step)) else { return };
        p.choose_row(next); self.open_lightbox(pane, next);
    }
    /// Where the picture may lie: the window above the lightbox's strip - and above the transport, when something plays.
    fn lightbox_area(&self) -> Rect { let size = self.window.surface_size(); let bar = if self.lightbox.view.as_ref().is_some_and(|v| v.player.is_some()) { LB_BAR } else { 0.0 }; Rect::new(0.0, 0.0, size.width as f64, (size.height as f64 - (LB_STRIP + bar) * self.scale()).max(1.0)) }
    /// Where the web view lies: the lightbox's area below a band of our own - nothing of ours can be drawn OVER a system
    /// view, and the close button must stay in reach.
    fn web_rect(&self) -> Rect { self.web_rect_of(self.lightbox_area(), self.scale()) }
    fn web_rect_of(&self, a: Rect, s: f64) -> Rect { Rect::new(a.x0, (a.y0 + LB_WEB_TOP * s).min(a.y1), a.x1, a.y1) }
    /// A key let go while the lightbox plays: a held shuttle ends.
    pub fn key_up(&mut self, name: &str) { if matches!(name, "J" | "L" | "A" | "D") { if let Some(p) = self.lightbox.view.as_mut().and_then(|v| v.player.as_mut()) { p.act(qcv_player::Action::ShuttleEnd); } } }
    /// "Open in QCView" (row 452): the file is handed to QCView, when it is installed where the system keeps applications.
    fn qcview() -> Option<PathBuf> {
        if cfg!(target_os = "macos") { let p = PathBuf::from("/Applications/QCView.app"); p.is_dir().then_some(p) }
        // Windows: the Store's QCView is started by the alias the system keeps for it
        else if cfg!(windows) { let p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft").join("WindowsApps").join("qcview.exe"); std::fs::symlink_metadata(&p).is_ok().then_some(p) }
        else { None }
    }
    fn lightbox_pointer(&mut self, e: &Event) {
        let now = Instant::now(); let area = self.lightbox_area(); let s = self.scale();
        // the drawn menu of the layers (where menus are drawn: Windows) takes the pointer while it is open - row 551
        if self.ctx.is_open() { let (chosen, _, consumed) = self.ctx.event(e); if let Some(i) = chosen { self.ctx_pick(i); } self.lb_key = None; if consumed || matches!(e, Event::Down { .. }) { if matches!(e, Event::Down { .. }) && !consumed { self.ctx.close(); } return } }
        if self.lb_close.event(e, now) == Response::Clicked { self.close_lightbox(); return }
        // the pointer is on something of ours beside the web view: the keys come back to our own view
        if let Some(w) = self.lightbox.view.as_ref().and_then(|v| v.web.as_ref()) { if matches!(e, Event::Down { .. }) { w.focus_parent(); } return }
        // the transport of what plays: its buttons, and the line (a press takes hold of it; it is dragged until let go)
        if self.lightbox.view.as_ref().is_some_and(|v| v.player.is_some()) {
            use qcv_player::Action as A;
            let (play, lp, mute, qcv) = (self.lb_play.event(e, now) == Response::Clicked, self.lb_loop.event(e, now) == Response::Clicked, self.lb_mute.event(e, now) == Response::Clicked, self.lb_qcv.rect.width() > 0.0 && self.lb_qcv.event(e, now) == Response::Clicked);
            if qcv { if let (Some(app), Some(v)) = (Self::qcview(), self.lightbox.view.as_ref()) { self.out.push(Out::OpenWith(v.path.clone(), app)); } return }
            // the sequence's rate: a press steps it to the next, the secondary button to the one before
            if self.lb_fps.rect.width() > 0.0 { let back = matches!(e, Event::Down { x, y, button: Button::Secondary } if self.lb_fps.hit(*x, *y)); let fwd = self.lb_fps.event(e, now) == Response::Clicked;
                if fwd || back { if let Some(p) = self.lightbox.view.as_mut().and_then(|v| v.player.as_mut()) { p.step_rate(if back { -1 } else { 1 }); } self.lb_key = None; return } }
            // the sequence's layers: the list, the one shown ticked
            if self.lb_layer.rect.width() > 0.0 && self.lb_layer.event(e, now) == Response::Clicked { self.ask_seq_layer(); self.lb_key = None; return }
            let line = self.lb_line; let Some(p) = self.lightbox.view.as_mut().and_then(|v| v.player.as_mut()) else { return };
            if play { p.act(A::TogglePlayback); return } if lp { p.act(A::ToggleLoop); return } if mute { p.act(A::ToggleMute); return }
            let at = |x: f64| (x - line.x0) / line.width().max(1.0);
            match *e {
                Event::Down { x, y, button: Button::Primary } if line.inflate(0.0, 10.0 * s).contains(kurbo::Point::new(x, y)) => { p.scrub(at(x)); return }
                Event::Move { x, .. } if p.scrubbing => { p.scrub(at(x)); return }
                Event::Up { button: Button::Primary, .. } | Event::Leave if p.scrubbing => { p.scrub_end(); return }
                _ => {}
            }
            // on the picture: a press that does not move plays or pauses; one that moves takes hold of the picture (below)
            match *e { Event::Down { x, y, button: Button::Primary } if area.contains(kurbo::Point::new(x, y)) => self.lb_click = Some((x, y)),
                Event::Move { x, y } => if self.lb_click.is_some_and(|c| (c.0 - x).hypot(c.1 - y) > 5.0 * s) { self.lb_click = None; },
                Event::Up { button: Button::Primary, .. } => if self.lb_click.take().is_some() { p.act(A::TogglePlayback); },
                _ => {} }
        }
        // the contact sheet of an EXR's layers: a press on one shows it large
        if self.lightbox.view.as_ref().is_some_and(|v| v.state == crate::lightbox::State::Layers) {
            self.lb_hover = e.pos().and_then(|(x, y)| self.lb_tiles.iter().position(|r| r.contains(kurbo::Point::new(x, y))));
            if let (Event::Down { button: Button::Primary, .. }, Some(i)) = (*e, self.lb_hover) { self.lb_hover = None; self.lightbox.pick_layer(i); }
            return
        }
        let Some(v) = self.lightbox.view.as_mut() else { return };
        // a document: its scroll bar; a drag moves it
        if let Some(d) = v.doc.as_ref() { if !self.lb_close.hit(e.pos().unwrap_or((-1.0, -1.0)).0, e.pos().unwrap_or((-1.0, -1.0)).1) { d.borrow_mut().pointer(e, area, now); } return }
        match *e {
            Event::Down { x, y, button: Button::Primary } if area.contains(kurbo::Point::new(x, y)) && !self.lb_close.hit(x, y) => {
                // two presses close together: actual size / fitted; one: the picture is taken hold of
                if self.lb_press.take().is_some_and(|(t, at)| now.duration_since(t) < Duration::from_millis(400) && (at.0 - x).hypot(at.1 - y) < 6.0 * s) { v.drag_end(); v.toggle((x, y), area); } else { self.lb_press = Some((now, (x, y))); v.drag_begin((x, y)); }
            }
            Event::Move { x, y } => { v.drag_to((x, y), area); }
            Event::Up { button: Button::Primary, .. } | Event::Leave => v.drag_end(),
            _ => {}
        }
    }
    fn lightbox_key(&mut self, k: &KeyEvent, name: &str) {
        let area = self.lightbox_area(); let mid = (area.center().x, area.center().y);
        // Escape (and Backspace) step back from one layer to the sheet of layers first (1.4.2's keys)
        if matches!(name, "Escape" | "Backspace") && self.lightbox.back_to_layers() { let e = &mut self.presenter.engine; e.clear_picture(); return }
        // what plays has 1.4.2's keys (row 473, point 3): K plays and pauses, Q / E step a frame, J / L (A / D) shuttle while held,
        // M mutes, V loops, Up / Down are the volume, Home / End the ends
        if let Some(p) = self.lightbox.view.as_mut().and_then(|v| v.player.as_mut()) {
            use qcv_player::Action as A;
            let a = match name { "K" | "W" | "S" => Some(A::TogglePlayback), "Q" => Some(A::Step(-1)), "E" => Some(A::Step(1)), "J" | "A" => Some(A::ShuttleBegin(-1)), "L" | "D" => Some(A::ShuttleBegin(1)), "M" => Some(A::ToggleMute), "V" => Some(A::ToggleLoop),
                "Up" => Some(A::VolumeUp), "Down" => Some(A::VolumeDown), "Home" => Some(A::SeekToStart), "End" => Some(A::SeekToEnd), _ => None };
            if let Some(a) = a { if !(matches!(a, A::ShuttleBegin(_)) && p.t.is_fast_seeking()) { p.act(a); } return }
        }
        // a document: the page keys, Home and End, Up and Down scroll it; + - 0 are its size
        if let Some(d) = self.lightbox.view.as_ref().and_then(|v| v.doc.as_ref()) { let ch = match &k.key { Key::Char(c) => c.as_str(), _ => "" }; if d.borrow_mut().key(name, ch, area, Instant::now()) { return } }
        match name { "Space" | "Escape" => return self.close_lightbox(), "Left" | "Up" => return self.step_lightbox(-1), "Right" | "Down" => return self.step_lightbox(1), _ => {} }
        let Some(v) = self.lightbox.view.as_mut() else { return };
        match &k.key { Key::Char(c) => match c.as_str() { "0" | "f" | "F" => v.fit(), "1" => v.actual(mid, area), "=" | "+" => v.zoom_by(1.25, mid, area), "-" | "_" => v.zoom_by(0.8, mid, area), _ => {} }, _ => {} }
    }
    /// The wheel (or two fingers) zooms about the pointer, as in QCView's viewer.
    fn lightbox_wheel(&mut self, dy: f64) { let (area, at) = (self.lightbox_area(), self.pointer); if let Some(d) = self.lightbox.view.as_ref().and_then(|v| v.doc.as_ref()) { d.borrow_mut().wheel(dy, Instant::now()); return } if let Some(v) = self.lightbox.view.as_mut() { v.zoom_by((-dy * 0.004).exp().clamp(0.5, 2.0), at, area); } }
    /// A pinch on a trackpad: `delta` is the change of size (0.1: a tenth larger).
    pub fn pinch(&mut self, delta: f64) { let (area, at) = (self.lightbox_area(), self.pointer); if let Some(d) = self.lightbox.view.as_ref().and_then(|v| v.doc.as_ref()) { d.borrow_mut().zoom_by((1.0 + delta).clamp(0.5, 2.0)); return } if let Some(v) = self.lightbox.view.as_mut() { v.zoom_by((1.0 + delta).clamp(0.5, 2.0), at, area); } }
    /// The lightbox drawn: the picture is the presenter's (under this layer); the layer holds only the strip, the close
    /// button and, while there is no picture, what to say instead.
    fn redraw_lightbox(&mut self) {
        let s = self.scale(); let size = self.window.surface_size(); let (w, h) = (size.width as f64, size.height as f64); let now = Instant::now(); let area = self.lightbox_area();
        let measuring = std::env::var_os("UFB_DEMO").is_some() && self.lightbox.view.as_ref().is_some_and(|v| v.player.as_ref().is_some_and(|p| p.t.is_playing()));
        if measuring { if let Some(t) = self.lb_last { self.lb_gaps.push(now.duration_since(t).as_secs_f32() * 1000.0); } self.lb_last = Some(now); } else { self.lb_last = None; }
        // a picture that has come goes to the presenter: on a dark ground, or on a checker when it has transparency
        if let Some((frame, alpha)) = self.lightbox.take_frame() {
            match self.presenter.engine.show(&frame) { Ok(()) => self.presenter.engine.set_background(if alpha { 2 } else { 1 }), Err(e) => if let Some(v) = self.lightbox.view.as_mut() { v.state = crate::lightbox::State::Failed(format!("This file could not be shown: {e}.")); } }
        }
        // what plays: the player's frame of this moment goes to the presenter
        if let Some(p) = self.lightbox.view.as_mut().and_then(|v| v.player.as_mut()) {
            match p.t.tick(now) { Some(f) => { if self.presenter.engine.show(&f.frame).is_ok() { if !p.shown { self.presenter.engine.set_background(1); } p.shown = true; } } None => p.t.idle_refresh() }
        }
        let Some(v) = self.lightbox.view.as_ref() else { return };
        let ready = v.info().is_some() && v.player.as_ref().is_none_or(|p| p.shown);
        // The layer over the picture is drawn and rendered ONLY when something it shows has changed - the frame's number,
        // a button under the pointer, the zoom - not at every refresh of the screen: a clip of 24 frames on a 120 Hz
        // screen then costs a fifth of the drawing, and no refresh is missed for it (the owner's walk, 2026-11-16:
        // "our playback is stuttery and miserable" - the cadence measured was 4 / 5 / 6 refreshes a frame, not 5).
        let key = { use std::hash::{Hash, Hasher}; let mut k = std::collections::hash_map::DefaultHasher::new();
            (size.width, size.height, (s * 100.0) as u32, std::mem::discriminant(&v.state), ready, &v.name, &v.layer, v.layers.iter().filter(|l| l.1.is_some()).count(), self.lb_hover).hash(&mut k);
            if let Some(r) = v.place(area) { ((r.x0 * 4.0) as i64, (r.y0 * 4.0) as i64, (r.width() * 4.0) as i64).hash(&mut k); }
            if let Some(d) = v.doc.as_ref() { d.borrow().stamp.hash(&mut k); }
            if let Some(p) = v.player.as_ref() { (p.t.current_frame(), p.t.is_playing(), p.t.audio.is_muted(), p.t.loop_enabled(), p.shown, p.rate()).hash(&mut k); }
            // (a text that appears after a while; buttons that light up under the pointer)
            (v.state == crate::lightbox::State::Loading && now.duration_since(v.since) > Duration::from_millis(250)).hash(&mut k);
            [&self.lb_close, &self.lb_play, &self.lb_loop, &self.lb_mute, &self.lb_qcv, &self.lb_fps, &self.lb_layer].iter().for_each(|b| (b.hovered(), b.animating(now)).hash(&mut k));
            // (the layers of an EXR sequence come a moment after it opens, and the one shown is chosen: the chip is drawn anew)
            if let Some(p) = v.player.as_ref() { (p.layers.len(), &p.layer).hash(&mut k); }
            k.finish() };
        if self.lb_key == Some(key) && !self.lb_close.animating(now) && !v.doc.as_ref().is_some_and(|d| d.borrow().busy(now)) {
            if ready { let r = v.place(area); self.presenter.engine.set_place(r.map(|r| [r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32])); } else { self.presenter.engine.clear_picture(); }
            self.present_layer(now, false);
            if measuring { self.lb_cost.push(now.elapsed().as_secs_f32() * 1000.0); }
            return
        }
        self.lb_key = Some(key);
        if ready { self.presenter.engine.set_place(v.place(area).map(|r| [r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32])); } else { self.presenter.engine.clear_picture(); }
        let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        let scene: &mut Scene = self.layer.scene_mut(); scene.reset(); let fonts = &mut self.fonts;
        // around the picture the window is plain: the presenter's checker is for what shows THROUGH the picture only
        if let Some(r) = v.place(area).map(|r| r.intersect(area)).filter(|_| ready) {
            for q in [Rect::new(0.0, 0.0, w, r.y0), Rect::new(0.0, r.y1, w, area.y1), Rect::new(0.0, r.y0, r.x0, r.y1), Rect::new(r.x1, r.y0, w, r.y1)] { if q.width() > 0.0 && q.height() > 0.0 { paint::rect(scene, q, t::BACKGROUND_LAYER_1_COLOR); } }
        }
        self.lb_tiles.clear();
        if v.state == crate::lightbox::State::Layers {
            // the contact sheet: the layers in a grid that fills the window, each with its name
            paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);
            let n = v.layers.len().max(1); let pad = 28.0 * s; let label = 26.0 * s; let room = Rect::new(pad, pad + 30.0 * s, w - pad, area.y1 - pad);
            let cols = (1..=n).find(|c| { let rows = n.div_ceil(*c); let cw = room.width() / *c as f64; (cw * 9.0 / 16.0 + label) * rows as f64 <= room.height() }).unwrap_or(n); let rows = n.div_ceil(cols);
            let (cw, chh) = (room.width() / cols as f64, room.height() / rows as f64); let gap = 10.0 * s;
            for (i, (lname, img)) in v.layers.iter().enumerate() {
                let cell = Rect::new(room.x0 + (i % cols) as f64 * cw + gap, room.y0 + (i / cols) as f64 * chh + gap, room.x0 + (i % cols + 1) as f64 * cw - gap, room.y0 + (i / cols + 1) as f64 * chh - gap);
                if self.lb_hover == Some(i) { paint::rounded(scene, cell.inflate(5.0 * s, 5.0 * s), t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); }
                let pic = Rect::new(cell.x0, cell.y0, cell.x1, cell.y1 - label);
                match img { Some(img) => { let (iw, ih) = (img.image.width.max(1) as f64, img.image.height.max(1) as f64); let k = (pic.width() / iw).min(pic.height() / ih); let (pw, ph) = (iw * k, ih * k); paint::image(scene, img, Rect::new(pic.center().x - pw / 2.0, pic.center().y - ph / 2.0, pic.center().x + pw / 2.0, pic.center().y + ph / 2.0)); }
                    None => paint::rounded(scene, pic.inflate(-pic.width() * 0.3, -pic.height() * 0.3), 4.0 * s, t::GRAY_300) }
                let l = fonts.layout_fit(lname, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, ink, cell.width()); draw_layout(scene, &l, Affine::translate((cell.center().x - l.width() as f64 / 2.0, cell.y1 - label + (label - l.height() as f64) / 2.0)));
                self.lb_tiles.push(cell);
            }
        } else if let Some(web) = v.web.as_ref() {
            // the page is the system's view, above our surface: around it the window is plain, and the view follows its place
            paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR); web.set_rect(Rect::new(area.x0, (area.y0 + LB_WEB_TOP * s).min(area.y1), area.x1, area.y1), s);
        } else if let Some(d) = v.doc.as_ref() {
            // a document draws itself (text, a PDF's pages), and scrolls
            d.borrow_mut().draw(scene, fonts, area, s, now);
        } else if v.state == crate::lightbox::State::Audio {
            // a sound has no picture: its kind's sign, large
            paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);
            if let Some(g) = fonts.icon("waveform", 96.0 * s as f32, sub) { draw_layout(scene, &g, Affine::translate(((w - g.width() as f64) / 2.0, area.center().y - g.height() as f64 / 2.0))); }
        } else if !ready {
            paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);
            let text = match &v.state { crate::lightbox::State::Loading => if now.duration_since(v.since) > Duration::from_millis(250) { "Loading\u{2026}".to_string() } else { String::new() }, crate::lightbox::State::Failed(why) => why.clone(), _ => "No preview of this kind yet. Open it with the Open command.".to_string() };
            if !text.is_empty() { let l = fonts.layout_fit(&text, Family::Sans, t::BODY_SIZE_M * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, w - 80.0 * s); draw_layout(scene, &l, Affine::translate(((w - l.width() as f64) / 2.0, area.center().y - l.height() as f64 / 2.0))); }
        }
        // the transport of what plays: play / pause, where it is, the line, loop, mute, "Open in QCView"
        self.lb_line = Rect::ZERO;
        if let Some(p) = v.player.as_ref() {
            let bar = Rect::new(0.0, area.y1, w, area.y1 + LB_BAR * s); paint::rect(scene, bar, t::BACKGROUND_LAYER_2_COLOR); paint::hline(scene, 0.0, w, bar.y0, s, t::DIVIDER_COLOR);
            let bh = t::ACTION_BUTTON_HEIGHT_M as f64 * s; let by = bar.y0 + (bar.height() - bh) / 2.0; let rad = t::ACTION_BUTTON_CORNER_RADIUS_M as f64 * s; let mut x = 12.0 * s;
            self.lb_play.icon = if p.t.is_playing() { "pause" } else { "play" }; self.lb_play.rect = Rect::new(x, by, x + bh, by + bh); self.lb_play.corners = Corners::all(rad); self.lb_play.paint(scene, fonts, s, now); x += bh + 10.0 * s;
            let (at, of) = p.clock(); let clock = fonts.layout(&format!("{at}  /  {of}"), Family::Mono, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, ink);
            draw_layout(scene, &clock, Affine::translate((x, bar.y0 + (bar.height() - clock.height() as f64) / 2.0))); x += clock.width() as f64 + 14.0 * s;
            let mut r = w - 12.0 * s;
            self.lb_qcv.rect = if Self::qcview().is_some() && w / s > 760.0 { let bw = self.lb_qcv.measure(fonts, s).0; let hh = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let q = Rect::new(r - bw, bar.y0 + (bar.height() - hh) / 2.0, r, bar.y0 + (bar.height() + hh) / 2.0); r -= bw + 12.0 * s; q } else { Rect::ZERO };
            if self.lb_qcv.rect.width() > 0.0 { self.lb_qcv.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); self.lb_qcv.paint(scene, fonts, s, now); }
            // a sequence's rate is the owner's to say: the chip (row 478)
            self.lb_fps.rect = if p.sequence.is_some() { self.lb_fps.label = Some(format!("{} fps", crate::lightbox::rate_text(p.rate()))); let bw = self.lb_fps.measure(fonts, s).0; let hh = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let q = Rect::new(r - bw, bar.y0 + (bar.height() - hh) / 2.0, r, bar.y0 + (bar.height() + hh) / 2.0); r -= bw + 12.0 * s; q } else { Rect::ZERO };
            if self.lb_fps.rect.width() > 0.0 { self.lb_fps.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); self.lb_fps.paint(scene, fonts, s, now); }
            // an EXR sequence of several layers: the chip names the one shown
            self.lb_layer.rect = if p.layers.len() > 1 { self.lb_layer.label = Some(if p.layer.is_empty() { "Layers".to_string() } else { p.layer.clone() }); let bw = self.lb_layer.measure(fonts, s).0.min(260.0 * s); let hh = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let q = Rect::new(r - bw, bar.y0 + (bar.height() - hh) / 2.0, r, bar.y0 + (bar.height() + hh) / 2.0); r -= bw + 12.0 * s; q } else { Rect::ZERO };
            if self.lb_layer.rect.width() > 0.0 { self.lb_layer.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); self.lb_layer.paint(scene, fonts, s, now); }
            self.lb_mute.icon = if p.t.audio.is_muted() { "speaker-slash" } else { "speaker-high" }; self.lb_mute.rect = Rect::new(r - bh, by, r, by + bh); self.lb_mute.corners = Corners::all(rad); self.lb_mute.selected = p.t.audio.is_muted(); self.lb_mute.paint(scene, fonts, s, now); r -= bh + 4.0 * s;
            self.lb_loop.rect = Rect::new(r - bh, by, r, by + bh); self.lb_loop.corners = Corners::all(rad); self.lb_loop.selected = p.t.loop_enabled(); self.lb_loop.paint(scene, fonts, s, now); r -= bh + 14.0 * s;
            // the line: what has played of it, and where it stands
            let line = Rect::new(x, bar.center().y - 2.0 * s, r.max(x + 20.0 * s), bar.center().y + 2.0 * s); self.lb_line = line;
            paint::rounded(scene, line, 2.0 * s, t::SWITCH_TRACK_COLOR_OFF); let px = line.x0 + line.width() * p.progress();
            paint::rounded(scene, Rect::new(line.x0, line.y0, px, line.y1), 2.0 * s, t::NEUTRAL_CONTENT_COLOR_DEFAULT); paint::rounded(scene, Rect::new(px - 5.0 * s, line.center().y - 5.0 * s, px + 5.0 * s, line.center().y + 5.0 * s), 5.0 * s, t::NEUTRAL_CONTENT_COLOR_DEFAULT);
        }
        // the strip: the name, then what the picture is
        let strip = Rect::new(0.0, area.y1 + if v.player.is_some() { LB_BAR * s } else { 0.0 }, w, h); paint::rect(scene, strip, t::BACKGROUND_LAYER_2_COLOR); paint::hline(scene, 0.0, w, strip.y0, s, t::DIVIDER_COLOR);
        let size_px = t::BODY_SIZE_S * s as f32;
        let facts = fonts.layout_fit(&v.facts(area), Family::Sans, size_px, t::FONT_WEIGHT_REGULAR as f32, sub, (w * 0.62).max(80.0 * s)); let fx = w - 14.0 * s - facts.width() as f64;
        draw_layout(scene, &facts, Affine::translate((fx, strip.y0 + (strip.height() - facts.height() as f64) / 2.0)));
        let name = fonts.layout_fit(&v.name, Family::Sans, size_px, t::FONT_WEIGHT_BOLD as f32, ink, (fx - 28.0 * s).max(60.0 * s));
        draw_layout(scene, &name, Affine::translate((14.0 * s, strip.y0 + (strip.height() - name.height() as f64) / 2.0)));
        // the close button, clear of the window's own buttons: top right
        let bh = t::ACTION_BUTTON_HEIGHT_M as f64 * s; self.lb_close.rect = Rect::new(w - bh - 12.0 * s, 12.0 * s, w - 12.0 * s, 12.0 * s + bh); self.lb_close.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_M as f64 * s);
        paint::rounded(scene, self.lb_close.rect, t::ACTION_BUTTON_CORNER_RADIUS_M as f64 * s, t::BACKGROUND_LAYER_2_COLOR); self.lb_close.paint(scene, fonts, s, now);
        // (the drawn menu, over everything of the lightbox; while it is open the frame is drawn anew each time)
        if self.ctx.is_open() { self.ctx.paint(scene, fonts, s); self.lb_key = None; }
        self.present(now);
        if measuring { self.lb_cost.push(now.elapsed().as_secs_f32() * 1000.0); }
    }

    // ---------------------------------------------------------------- file operations (`ops.rs`)
    /// An operation ended (in this window or another): the panes that show a folder it changed read it again; the
    /// window that asked chooses what was made, says what went wrong, and asks what is left to ask.
    fn on_report(&mut self, rep: &crate::ops::Report) {
        let mine = rep.origin == self.id; let port = self.port.clone(); let now = Instant::now();
        for (i, p) in self.panes.iter_mut().enumerate() {
            if !rep.changed.iter().any(|c| c.starts_with(p.path())) { continue }
            let here: Vec<String> = if mine && i == self.active { rep.made.iter().filter(|m| m.parent() == Some(p.path())).filter_map(|m| m.file_name()).map(|n| n.to_string_lossy().into_owned()).collect() } else { vec![] };
            p.refresh_choosing(&port, here);
        }
        self.follow_moves(&rep.moved);
        // a job that an undo took away leaves the sidebar, and the tab that shows it (its metadata is not touched)
        for gone in &rep.unmade {
            if mine { let _ = crate::db::with(|d| d.unsubscribe(gone)); }
            self.reload_jobs();
            if self.job.as_ref().is_some_and(|j| &j.path == gone) { self.job = None; self.focus = Focus::Pane; for p in self.panes.iter_mut() { p.root = None; } if let Some(up) = gone.parent() { self.open_place(up.to_path_buf(), false); } }
        }
        if mine && rep.error.is_none() { match (rep.kind, rep.made.first()) {
            // a new job is a job of the sidebar, starts with the columns its type gives its sections, and is shown
            (Some(crate::ops::Kind::NewJob), Some(job)) => {
                let ty = crate::jobview::type_for(job).ty;
                let _ = crate::db::with(|d| { let _ = d.subscribe(job, "");
                    for s in &ty.sections { for c in &s.columns {
                        let mut taken: Vec<Option<String>> = c.options.iter().map(|o| o.color().map(str::to_string)).collect(); let mut opts = vec![];
                        for (i, o) in c.options.iter().enumerate() { let col = match o.color() { Some(col) => col.to_string(), None => { let col = crate::chips::next_color(&taken); taken[i] = Some(col.clone()); col } }; opts.push((o.name().trim().to_string(), Some(col))); }
                        // (a name that is there already - the job was made, undone and made again - stays as it is)
                        let _ = d.add_column(job, &s.folder, &c.name, &c.kind, &opts);
                    } } });
                self.reload_jobs(); self.open_place(job.clone(), true);
            }
            (Some(crate::ops::Kind::AddItem), Some(item)) => if let Some(j) = self.job.as_mut() { j.choose = Some(item.clone()); },
            // a new note opens in minNotes (the application that has `.mnd` files; without it the file is there all the same)
            (Some(crate::ops::Kind::NewNote), Some(note)) => if std::env::var_os("UFB_DEMO").is_none() { self.out.push(Out::OpenDefault(note.clone())); },
            _ => {}
        } }
        if let Some(j) = self.job.as_mut() { if rep.changed.iter().any(|c| c.starts_with(&j.path) || j.path.starts_with(c)) { j.reload(); } }
        if let Some(t) = self.tracker.as_mut() { t.reload(); }
        // (the tabs behind show what is there when they come to the front)
        for p in self.hidden_panes() { if rep.changed.iter().any(|c| c.starts_with(p.path())) { p.refresh_choosing(&port, vec![]); } }
        if self.rename.as_ref().is_some_and(|r| rep.changed.iter().any(|c| r.path.starts_with(c))) { self.end_rename(); }
        if !mine { return }
        if let Some(why) = &rep.error { self.toast.show(why, min_ui::toast::ToastKind::Negative, now); }
        else if let Some(what) = &rep.notice { self.toast.show(what, min_ui::toast::ToastKind::Notice, now); }
        if let Some((archive, wrong)) = &rep.password { if self.dialog.is_none() { self.ask_password(archive.clone(), *wrong); } }
        // a new folder goes straight into its name (row 437)
        if rep.kind == Some(crate::ops::Kind::NewFolder) { self.rename_next = rep.made.first().cloned(); }
        if !rep.no_trash.is_empty() { self.ask_delete(rep.no_trash.clone(), true); }
    }
    /// The engine asks about a name that is taken (rows 436 / 468): replace, skip or keep both - for this one or for all
    /// that follow - or stop. Return gives "Keep Both" (nothing is lost by it), Escape stops.
    fn show_question(&mut self) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        if self.dialog.is_some() { return } let Some(q) = self.asks.pop_front() else { return }; let c = &q.conflict;
        let folder = c.dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| c.dest.to_string_lossy().into_owned());
        let mut d = Dialog::new(&format!("An item named \u{201c}{}\u{201d} already exists in \u{201c}{folder}\u{201d}.", c.name), &format!("Replace it with the one you are {}, skip this one, or keep both?", if c.mv { "moving" } else { "copying" }),
            vec![("Stop", Variant::Secondary), ("Replace", Variant::Secondary), ("Skip", Variant::Secondary), ("Keep Both", Variant::Primary)]).with_check("Apply to all"); d.show();
        self.dialog = Some((d, Asked::Conflict(q.id)));
    }
    /// The chosen entries are copied (`mv`: moved) into the other pane's folder (row 450: in the menus, with no key).
    fn to_other_pane(&mut self, mv: bool) {
        if !self.two || self.panes.len() < 2 { return } let dest = self.panes[1 - self.active.min(1)].path().to_path_buf(); let paths = self.chosen_paths();
        if !paths.is_empty() { crate::ops::submit(self.id, crate::ops::Op::Transfer { paths, dest, mv }); }
    }
    /// Files from the clipboard are put into the active pane's folder (`mv`: moved there).
    pub fn paste_files(&mut self, paths: Vec<PathBuf>, mv: bool) {
        if paths.is_empty() || self.focus != Focus::Pane || crate::ops::status().is_some() || self.lightbox.loading() || self.lightbox.view.as_ref().is_some_and(|v| v.dragging() || v.player.as_ref().is_some_and(|p| p.busy(Instant::now()))) || self.lb_close.animating(Instant::now()) || !self.recheck.is_empty() || !self.asks.is_empty() || self.dialog.is_some() { return } let dest = self.pane().path().to_path_buf();
        crate::ops::submit(self.id, crate::ops::Op::Transfer { paths, dest, mv });
    }
    fn chosen_paths(&mut self) -> Vec<PathBuf> { self.pane().chosen_files() }
    /// "Delete for good?" - always asked (row 468, point 3). `no_trash`: asked because the volume has no Trash.
    /// Extract Here for the chosen archives; Compress to ZIP for what is chosen (row 508, points 5-9): tasks of the lane.
    fn extract_chosen(&mut self) {
        let paths: Vec<PathBuf> = self.pane().chosen_entries(2000).into_iter().filter(|e| !e.1 && e.0.file_name().is_some_and(|f| crate::archive::is_archive(&f.to_string_lossy()))).map(|e| e.0).collect();
        if paths.is_empty() { return }
        if !crate::ops::allowed_in(self.pane().path()) { self.toast.show("This build changes nothing outside the folders it was started with.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        for archive in paths { crate::archive::submit(self.id, crate::archive::Job::Extract { archive, password: None }); }
    }
    /// A found entry is shown where it lies: the pane goes to its folder and chooses it (row 508, point 15).
    fn show_enclosing(&mut self) {
        let Some((path, ..)) = self.pane().chosen_entries(1).into_iter().next() else { return };
        let (Some(dir), Some(name)) = (path.parent().map(Path::to_path_buf), path.file_name().map(|n| n.to_string_lossy().into_owned())) else { return };
        let port = self.port.clone(); self.search.clear(); self.pane().go_select(&port, dir, name);
    }
    /// The sidebar's favourites: those the build was started with, then those the user made (row 531).
    fn reload_favorites(&mut self) { let mut f = self.started_favorites.clone(); for p in crate::places::saved() { if !f.iter().any(|x| x.path == p.path) { f.push(p); } } self.favorites = f; }
    /// Transcode to MP4 for the chosen videos and sequences (rows 508 points 10-12, 513): a task of the lane each.
    fn transcode_chosen(&mut self) {
        let Some(sources) = self.pane().chosen_media() else { return };
        if !crate::ops::allowed_in(self.pane().path()) { self.toast.show("This build changes nothing outside the folders it was started with.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        for source in sources { crate::archive::submit(self.id, crate::archive::Job::Transcode { source }); }
    }
    fn compress_chosen(&mut self) {
        let paths = self.pane().chosen_files(); if paths.is_empty() { return }
        if !crate::archive::zip_place(&paths).is_some_and(|(dir, _)| crate::ops::allowed_in(&dir)) { self.toast.show("This build changes nothing outside the folders it was started with.", min_ui::toast::ToastKind::Notice, Instant::now()); return }
        crate::archive::submit(self.id, crate::archive::Job::Compress { paths });
    }
    /// Rename several in one step (row 528; Finder's "Rename N Items", its "Replace Text"): what is typed into Find is
    /// replaced in every chosen name - not in a file's extension - and the question says what that makes of them before
    /// anything is renamed.
    fn ask_batch_rename(&mut self) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        let paths = self.pane().chosen_files(); if paths.len() < 2 { return }
        if !crate::ops::allowed_in(self.pane().path()) { self.toast.show(crate::ops::GUARDED, min_ui::toast::ToastKind::Notice, Instant::now()); return }
        let items: Vec<(PathBuf, bool)> = paths.into_iter().map(|p| { let d = p.is_dir(); (p, d) }).collect();
        let mut d = Dialog::new(&format!("Rename {} Items", crate::pane::group(items.len())), "", vec![("Cancel", Variant::Secondary), ("Rename", Variant::Primary)]).with_field("Find", "text in the names").with_field("Replace with", "nothing: the text is taken out"); d.show_with("", &mut self.fonts);
        self.dialog = Some((d, Asked::BatchRename(items))); self.dialog_preview();
    }
    /// The names a batch rename would give: (the entry, its new name) for those that change.
    fn batch_names(items: &[(PathBuf, bool)], find: &str, with: &str) -> Vec<(PathBuf, String)> {
        items.iter().filter_map(|(p, dir)| { let old = p.file_name()?.to_string_lossy().into_owned(); let new = crate::ops::replaced_name(&old, *dir, find, with); (new != old).then(|| (p.clone(), new)) }).collect()
    }
    /// An archive wants its password: asked for, and the answer is a new task.
    fn ask_password(&mut self, archive: PathBuf, wrong: bool) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut d = Dialog::new(&format!("\u{201c}{name}\u{201d} is locked with a password"), if wrong { "That password did not open it." } else { "" }, vec![("Cancel", Variant::Secondary), ("Extract", Variant::Primary)]).with_secret_field("Password", ""); d.show_with("", &mut self.fonts);
        self.dialog = Some((d, Asked::Password(archive)));
    }
    /// Quit: at once when nothing runs; with tasks running or waiting it is asked first (row 508, point 4 - nothing of
    /// the list is kept: what is done stays done, the rest is not done).
    pub fn ask_quit(&mut self) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        let n = crate::tasks::counts().0; if n == 0 { self.out.push(Out::Quit); return }
        let mut d = Dialog::new(&if n == 1 { "A task is still running. Quit UFB?".to_string() } else { format!("{n} tasks are still running or waiting. Quit UFB?") }, "What is done stays done; the rest is stopped and not taken up again.", vec![("Quit", Variant::Negative), ("Cancel", Variant::Primary)]); d.show();
        self.dialog = Some((d, Asked::Quit));
    }
    fn ask_delete(&mut self, paths: Vec<PathBuf>, no_trash: bool) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        if paths.is_empty() { return }
        let what = if paths.len() == 1 { format!("\u{201c}{}\u{201d}", paths[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()) } else { format!("{} items", crate::pane::group(paths.len())) };
        let body = if no_trash { "This volume has no Trash. What is deleted here is gone at once and cannot be brought back." } else { "This cannot be undone." };
        // (the last button is the one Return gives: it is Cancel)
        let mut d = Dialog::new(&format!("Delete {what} immediately?"), body, vec![("Delete", Variant::Negative), ("Cancel", Variant::Primary)]); d.show();
        self.dialog = Some((d, Asked::Delete(paths)));
    }
    /// A button of the open question was chosen (`button`: its place; the last one is the careful answer).
    fn answer(&mut self, button: usize) {
        let Some((d, asked)) = self.dialog.take() else { return };
        match asked {
            Asked::Conflict(id) => { use crate::ops::Choice; crate::ops::answer(id, [Choice::Stop, Choice::Replace, Choice::Skip, Choice::KeepBoth][button.min(3)], d.checked()); }
            Asked::Delete(paths) => if button == 0 { crate::ops::submit(self.id, crate::ops::Op::Delete { paths }); },
            Asked::Quit => if button == 0 { self.out.push(Out::Quit); },
            Asked::BatchRename(items) => if button == 1 { let pairs = Self::batch_names(&items, d.fields[0].1.text(), d.fields[1].1.text()); if !pairs.is_empty() { crate::ops::submit(self.id, crate::ops::Op::RenameMany { pairs }); } },
            Asked::Password(archive) => if button == 1 { crate::archive::submit(self.id, crate::archive::Job::Extract { archive, password: Some(d.value_at(0)) }); },
            // (made off this thread: a skeleton may be copied to a share)
            // (through the engine, as every change to the disk: one at a time, behind the belt, and undone by Undo)
            Asked::NewJob(parent) => if button == 1 { crate::ops::submit(self.id, crate::ops::Op::NewJob { parent, number: d.value_at(0), name: d.value_at(1) }); },
            Asked::AddItem(dir) => if button == 1 { crate::ops::submit(self.id, crate::ops::Op::AddItem { dir, name: d.value_at(0) }); },
            Asked::NewDated(dir) => if button == 1 { crate::ops::submit(self.id, crate::ops::Op::NewDated { dir, label: d.value_at(0) }); },
            Asked::NewLink(dir) => if button == 1 { crate::ops::submit(self.id, crate::ops::Op::NewLink { dir, url: d.value_at(0), name: d.value_at(1) }); },
            Asked::NewNote(dir) => if button == 1 { crate::ops::submit(self.id, crate::ops::Op::NewNote { dir, name: d.value_at(0) }); },
            Asked::SyncSet => if button == 1 { let (farm, node, only) = (d.value_at(0), d.value_at(1), if crate::identity::belt() { d.value_at(2) } else { String::new() }); crate::settings::with(|s| { s.set_str("sync/farm", &farm); s.set_str("sync/node", &node); s.set_str("sync/only", &only); s.set_bool("sync/on", true); }); self.apply_sync(); },
            Asked::ShareLetter(i) => if button == 1 { match crate::shares::letter_of(&d.value_at(0)) { Some(l) => if let Some(sh) = self.shares.get_mut(i) { sh.letter = l; crate::shares::save(&self.shares); }, None => self.toast.show("A drive letter is one letter, A to Z.", min_ui::toast::ToastKind::Notice, Instant::now()) } },
            Asked::AddShare => if button == 1 { let text = d.value_at(0); self.add_share(&text); },
            Asked::RenameShare(i) => if button == 1 { let name = d.value_at(0); if let Some(sh) = self.shares.get_mut(i).filter(|_| !name.is_empty()) { sh.name = name; crate::shares::save(&self.shares); } },
            Asked::Extension { path, typed, kept } => { let name = if button == 0 { typed } else { kept }; if path.file_name().is_some_and(|n| n.to_string_lossy() != name.as_str()) { crate::ops::submit(self.id, crate::ops::Op::Rename { path, name }); } }
        }
    }
    /// Rename in place (row 437): the one chosen entry's name becomes a field, the name chosen without its extension.
    fn start_rename(&mut self) {
        let i = self.active.min(self.panes.len() - 1); let entries = self.panes[i].chosen_entries(2); if entries.is_empty() { return }
        // several chosen, or a sequence (its frames each have a name of their own): they are renamed together (row 528)
        if entries.len() != 1 || self.panes[i].chosen_sequence() { self.ask_batch_rename(); return }
        let (path, dir, _) = entries.into_iter().next().unwrap(); let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return };
        if !crate::ops::allowed_in(path.parent().unwrap_or(&path)) { self.toast.show(crate::ops::GUARDED, min_ui::toast::ToastKind::Notice, Instant::now()); return }
        let mut field = TextField::new(Family::Sans, t::BODY_SIZE_S); field.set_text(&name); field.focused = true;
        let stem = if dir { name.len() } else { match name.rfind('.') { Some(k) if k > 0 => k, _ => name.len() } };
        field.select_range(0, stem, &mut self.fonts);
        self.leave_path(); self.leave_search(); self.focus = Focus::Rename; self.rename = Some(Renaming { pane: i, path, dir, field });
    }
    fn end_rename(&mut self) { self.rename = None; if self.focus == Focus::Rename { self.focus = Focus::Pane; } }
    /// The field's name is taken. A name that cannot be given, or that another entry has, is refused and the field
    /// stays open; a changed extension is asked about first, as Finder asks (Windows does not).
    fn commit_rename(&mut self) {
        let Some(r) = self.rename.as_mut() else { return }; let now = Instant::now();
        let typed = r.field.text().to_string(); let old = r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if typed == old || typed.is_empty() { self.end_rename(); return }
        let fault = crate::ops::name_fault(&typed).or_else(|| self.panes.get(r.pane).is_some_and(|p| p.name_taken(&r.path, &typed)).then(|| crate::ops::taken(&typed)));
        if let Some(why) = fault { r.field.error = true; self.toast.show(&why, min_ui::toast::ToastKind::Negative, now); return }
        let (path, dir) = (r.path.clone(), r.dir); self.end_rename();
        let ext = |n: &str| match n.rfind('.') { Some(k) if k > 0 && k + 1 < n.len() => n[k..].to_string(), _ => String::new() };
        let (was, is) = (ext(&old), ext(&typed));
        if cfg!(target_os = "macos") && !dir && was.to_lowercase() != is.to_lowercase() && !was.is_empty() {
            use min_ui::{dialog::Dialog, text_button::Variant};
            let stem = if is.is_empty() { typed.as_str() } else { &typed[..typed.len() - is.len()] }; let kept = format!("{stem}{was}");
            let (title, change) = if is.is_empty() { (format!("Are you sure you want to remove the extension \u{201c}{was}\u{201d}?"), "Remove".to_string()) } else { (format!("Are you sure you want to change the extension from \u{201c}{was}\u{201d} to \u{201c}{is}\u{201d}?"), format!("Use {is}")) };
            let mut d = Dialog::new(&title, "If you make this change, your file may open in a different application.", vec![(change.as_str(), Variant::Secondary), (format!("Keep {was}").as_str(), Variant::Primary)]); d.show();
            self.dialog = Some((d, Asked::Extension { path, typed, kept })); return
        }
        crate::ops::submit(self.id, crate::ops::Op::Rename { path, name: typed });
    }

    // ---------------------------------------------------------------- commands
    /// The active pane goes to a folder. In a job a pane stays inside the chosen item: a folder outside it (a path
    /// typed, the Go menu) opens in a NEW TAB, and the job stays as it is (row 486).
    fn go(&mut self, path: PathBuf) {
        if self.job.is_some() || self.tracker.is_some() { let inside = !self.blank() && self.pane().root.clone().is_some_and(|r| path.starts_with(&r)); if !inside { self.new_tab(path); return } }
        let port = self.port.clone(); self.pane().go(&port, path);
    }
    fn set_two(&mut self, on: bool) {
        self.two = on;
        if on && self.panes.len() < 2 { let path = self.panes[0].path().to_path_buf(); let mut p = Pane::new(path, &self.port); seed(&mut p, crate::prefs::view()); p.set_hidden(self.hidden); p.set_folders_first(self.folders_first); self.panes.push(p); }
        if !on { self.active = 0; }
        // (in a job the second pane shows the chosen item's renders; the first stays where it is)
        if on { self.job_follow(false, true); }
    }
    fn edit_path(&mut self) {
        let text = self.here().to_string_lossy().into_owned();
        self.field.set_text(&text); self.field.focused = true; self.field.error = false; self.field.select_all(&mut self.fonts); self.search.focused = false; self.focus = Focus::Path; self.path_error = None;
    }
    fn leave_path(&mut self) { if self.focus == Focus::Path { self.focus = Focus::Pane; } self.field.focused = false; self.field.error = false; self.path_error = None; self.check = None; }
    /// A path or a link is opened as if it were typed into the path bar (a second launch handed it over, row 519).
    pub fn open_text(&mut self, text: &str) { if self.prefs.is_some() || self.dialog.is_some() { return } if self.lightbox.is_open() { self.close_lightbox(); } self.edit_path(); self.field.set_text(text); self.submit_path(); }
    fn submit_path(&mut self) {
        let current = self.here();
        // a path from another machine - the other system's spelling, a numbered landing - is this machine's path to the same thing (row 498)
        // ... and a LINK is the path it leads to (row 503)
        let text = self.field.text().trim().to_string(); let link = text.starts_with("ufb:") || text.starts_with("union:");
        if link && crate::db::from_link(&text).is_none() { self.path_error = Some(("That link leads nowhere on this machine".into(), Instant::now())); self.field.error = true; return }
        // a `union:` link is handed to the system's file manager, which shows the thing; a `ufb:` link is opened here (row 505)
        if crate::db::is_union(&text) { if let Some(p) = crate::db::from_link(&text) { let (tx, port) = (self.made_tx.clone(), self.port.clone()); let _ = std::thread::Builder::new().name("ufb-reveal".into()).spawn(move || { let there = p.exists(); let _ = tx.send(Made::Shown(p, there)); port.send(Msg::Icon); }); } self.leave_path(); return }
        let typed = if link { crate::db::from_link(&text) } else { crate::db::translate(&text).or_else(|| crumbs::typed_path(self.field.text(), &places::home(), &current)) };
        match typed {
            // whether it is a folder is asked off this thread; the answer navigates or shows the error
            Some(p) => { let req = lister::next_req(); self.check = Some(req); self.path_error = None; self.field.error = false; lister::check(&self.port, req, p); }
            None => self.leave_path(),
        }
    }
    fn focus_search(&mut self) { self.leave_path(); self.focus = Focus::Search; self.search.focused = true; self.search.select_all(&mut self.fonts); }
    fn leave_search(&mut self) { if self.focus == Focus::Search { self.focus = Focus::Pane; } self.search.focused = false; }
    pub fn command(&mut self, c: Cmd) {
        let port = self.port.clone();
        // while the lightbox is open the window's own commands and Preview act; the browser's wait
        if (self.coledit.is_some() || self.prefs.is_some() || self.note.is_some()) && !matches!(c, Cmd::Copy | Cmd::Cut | Cmd::Paste | Cmd::Minimize | Cmd::Quit) { return }
        if self.chip.is_some() { self.chip = None; }
        if self.lightbox.is_open() && !matches!(c, Cmd::Preview | Cmd::CloseWindow | Cmd::Minimize | Cmd::Quit | Cmd::NewWindow | Cmd::NewTab | Cmd::CloseTab | Cmd::NextTab | Cmd::PrevTab) { return }
        // the commands that change files act on the pane's choice: not while a name, a path or a search is being typed,
        // and not under an open question (the system's menu keys would otherwise reach them from a text field)
        // (a question with a FIELD keeps copy, cut and paste: they are its field's - the menu's keys reach it this way on macOS)
        if let Some((d, _)) = self.dialog.as_ref() { let field = !d.fields.is_empty() && matches!(c, Cmd::Copy | Cmd::Cut | Cmd::Paste); if !field && matches!(c, Cmd::Copy | Cmd::Cut | Cmd::Paste | Cmd::PasteMove | Cmd::CopyToOther | Cmd::MoveToOther) { return } }
        if matches!(c, Cmd::NewFolder | Cmd::Rename | Cmd::Duplicate | Cmd::Trash | Cmd::DeleteNow | Cmd::Undo | Cmd::Redo | Cmd::PasteMove | Cmd::CopyToOther | Cmd::MoveToOther | Cmd::NewJob | Cmd::Subscribe) && (self.focus != Focus::Pane || self.dialog.is_some()) { return }
        // in a job with nothing chosen there is no pane to act on (row 486)
        if self.blank() && !self.text_focus() && matches!(c, Cmd::Copy | Cmd::Cut | Cmd::Paste | Cmd::PasteMove | Cmd::CopyToOther | Cmd::MoveToOther | Cmd::NewFolder | Cmd::Rename | Cmd::Duplicate | Cmd::Trash | Cmd::DeleteNow | Cmd::Open | Cmd::Preview | Cmd::SelectAll | Cmd::Back | Cmd::Forward | Cmd::Up | Cmd::Find | Cmd::Subscribe | Cmd::Sort(_)) { return }
        match c {
            // copy and paste are the text field's while one is in hand; else they are about files (the clipboard is the application's to reach)
            Cmd::Copy | Cmd::Cut => if self.text_focus() { self.out.push(Out::FieldCopy { cut: c == Cmd::Cut }); } else { let paths = self.chosen_paths(); if !paths.is_empty() { self.out.push(Out::CopyFiles { paths, cut: c == Cmd::Cut }); } },
            Cmd::Paste => self.out.push(if self.text_focus() { Out::FieldPaste } else { Out::PasteFiles { mv: false } }),
            Cmd::PasteMove => self.out.push(Out::PasteFiles { mv: true }),
            Cmd::CopyToOther => self.to_other_pane(false), Cmd::MoveToOther => self.to_other_pane(true),
            Cmd::NewFolder => { let dir = self.pane().path().to_path_buf(); crate::ops::submit(self.id, crate::ops::Op::NewFolder { dir }); }
            // (a find is named by its path here: it is renamed where it lies)
            Cmd::Rename => if self.pane().found().is_some() { self.toast.show("A found item is renamed in its own folder: Show in Enclosing Folder.", min_ui::toast::ToastKind::Notice, Instant::now()); } else { self.start_rename() },
            // (a sequence is duplicated AS a sequence: the copy's name on its base, the frame numbers kept)
            Cmd::Duplicate => { let (paths, seqs) = self.pane().chosen_for_duplicate(); if !paths.is_empty() { crate::ops::submit(self.id, crate::ops::Op::Duplicate { paths }); } for frames in seqs { crate::ops::submit(self.id, crate::ops::Op::DuplicateSequence { frames }); } }
            Cmd::Trash => { let paths = self.chosen_paths(); if !paths.is_empty() { crate::ops::submit(self.id, crate::ops::Op::Trash { paths }); } }
            Cmd::DeleteNow => { let paths = self.chosen_paths(); self.ask_delete(paths, false); }
            Cmd::Undo => crate::ops::submit(self.id, crate::ops::Op::Undo), Cmd::Redo => crate::ops::submit(self.id, crate::ops::Op::Redo),
            Cmd::NewWindow => { let p = self.here(); self.out.push(Out::NewWindow(p)); }
            // tabs (row 480): a new one shows the folder of the one in front; closing the last closes the window
            Cmd::NewTab => { let p = self.here(); self.new_tab(p); }
            Cmd::CloseTab => self.close_tab(self.tab),
            Cmd::NextTab | Cmd::PrevTab => { let n = self.tabs.len(); if n > 1 { self.show_tab((self.tab + if c == Cmd::NextTab { 1 } else { n - 1 }) % n); } }
            Cmd::NewJob => self.ask_new_job(),
            Cmd::Subscribe => { let e = self.pane().chosen_entries(2); if e.len() == 1 && e[0].1 && !e[0].2 { let p = e[0].0.clone(); self.subscribe(p, true); } else { self.toast.show("Choose one folder to subscribe to it as a job.", min_ui::toast::ToastKind::Notice, Instant::now()); } }
            Cmd::CloseWindow => self.out.push(Out::Close), Cmd::Minimize => self.out.push(Out::Minimize), Cmd::Quit => self.ask_quit(),
            Cmd::Open => { self.pane().open_chosen(&port); self.after_pane_events(); }
            Cmd::Preview => if self.lightbox.is_open() { self.close_lightbox(); } else if self.focus == Focus::Pane && self.dialog.is_none() { let i = self.active.min(self.panes.len() - 1); if let Some(row) = self.panes[i].cursor_row() { self.open_lightbox(i, row); } },
            Cmd::SelectAll => match self.focus { Focus::Path => self.field.select_all(&mut self.fonts), Focus::Search => self.search.select_all(&mut self.fonts), Focus::Rename => if let Some(r) = self.rename.as_mut() { r.field.select_all(&mut self.fonts); }, Focus::Pane => self.pane().select_all(), Focus::Items => {}, Focus::Cell => { let l = match self.tracker.as_mut() { Some(t) => &mut t.list, None => &mut self.items }; if let Some(e) = l.edit.as_mut() { e.field.select_all(&mut self.fonts); } } },
            // (the View menu's rows are remembered: they are the Settings card's switches, row 516)
            Cmd::Settings => self.open_prefs(),
            Cmd::Help => crate::sysopen::open_url("https://ufbrowser.com/"),
            Cmd::CheckForUpdates => { #[cfg(target_os = "macos")] crate::sparkle::check_now(); }
            Cmd::AsList => { self.pane().set_mode(Mode::List); crate::settings::with(|s| s.set_bool("view/grid", false)); } Cmd::AsGrid => { self.pane().set_mode(Mode::Grid); crate::settings::with(|s| s.set_bool("view/grid", true)); }
            Cmd::Hidden => { self.hidden = !self.hidden; let on = self.hidden; crate::settings::with(|s| s.set_bool("view/hidden", on)); for p in self.panes.iter_mut() { p.set_hidden(on); } for p in self.hidden_panes() { p.set_hidden(on); } }
            Cmd::FoldersFirst => { self.folders_first = !self.folders_first; let on = self.folders_first; crate::settings::with(|s| s.set_bool("view/folders_first", on)); for p in self.panes.iter_mut() { p.set_folders_first(on); } for p in self.hidden_panes() { p.set_folders_first(on); } }
            // (a flat section of a job has ONE view: two panes on the same folder, one above the other, only confuse - row 485)
            Cmd::SecondPane if self.job.as_ref().is_some_and(|j| !j.two_tier) => self.toast.show("This section has a single view.", min_ui::toast::ToastKind::Notice, Instant::now()),
            Cmd::SecondPane => { self.set_two(!self.two); let on = self.two; if let Some(j) = self.job.as_mut() { if let Some(sec) = j.section.clone() { j.pane2.insert(sec, on); } } }
            Cmd::Sequences => { let on = !self.pane().model.sequences; self.pane().set_sequences(on); crate::settings::with(|s| s.set_bool("view/sequences", on)); }
            Cmd::Column(i) => self.toggle_column(i as usize),
            Cmd::Sort(i) => { let by = SortBy::of_column(i as usize); let p = self.pane(); let asc = if p.model.sort == by { !p.model.ascending } else { true }; p.set_sort(by, asc); }
            Cmd::Refresh => { let dir = self.pane().path().to_path_buf(); self.thumbs.retry(&dir); self.pane().refresh(&port); places::refresh(&port); if let Some(j) = self.job.as_mut() { j.reload(); } }
            Cmd::Back => self.pane().back(&port), Cmd::Forward => self.pane().forward(&port), Cmd::Up => self.pane().up(&port),
            Cmd::Home => self.go(places::home()), Cmd::Desktop => self.go(places::home().join("Desktop")), Cmd::Downloads => self.go(places::home().join("Downloads")),
            Cmd::GoToFolder => self.edit_path(), Cmd::Find => self.focus_search(),
        }
    }

    // ---------------------------------------------------------------- input
    pub fn wheel(&mut self, dy: f64) {
        if self.lightbox.is_open() { return self.lightbox_wheel(dy) }
        let now = Instant::now(); let p = kurbo::Point::new(self.pointer.0, self.pointer.1);
        if self.menubar.as_ref().is_some_and(|m| m.bar.is_open()) { return }
        // the tasks' panel scrolls through its rows, eight at a time in view (row 524)
        if self.op_list && self.task_card.contains(p) { let n = crate::tasks::list().len(); let most = n.saturating_sub(TASK_ROWS); let step = if dy.abs() >= 20.0 { 1 } else { 0 }; self.task_off = if dy < 0.0 { (self.task_off + step).min(most) } else { self.task_off.saturating_sub(step) }; return }
        if self.side_rect.contains(p) { self.side_bar.wheel(dy, now); return }
        if self.tracker.is_none() && self.job_list.contains(p) { if let Some(b) = self.board.as_mut() { if self.mods.shift { b.hbar.wheel(dy, now); } else { b.bar.wheel(dy, now); } return } }
        if (self.job.is_some() || self.tracker.is_some()) && self.job_list.contains(p) { let shift = self.mods.shift; let l = self.list_mut(); if shift { l.hbar.wheel(dy, now); } else { l.bar.wheel(dy, now); } return }
        if self.blank() { return }
        let n = self.shown(); for pane in self.panes.iter_mut().take(n) { if pane.rect.contains(p) { pane.wheel(dy, now); return } }
    }
    /// The wheel turned SIDEWAYS (a trackpad, a tilting wheel, macOS's Shift with the wheel): the board, the item list or
    /// the tracker under the pointer scrolls sideways.
    pub fn wheel_x(&mut self, dx: f64) {
        if self.lightbox.is_open() || self.coledit.is_some() || self.chip.is_some() { return }
        let now = Instant::now(); let p = kurbo::Point::new(self.pointer.0, self.pointer.1); if !self.job_list.contains(p) { return }
        if self.tracker.is_none() { if let Some(b) = self.board.as_mut() { b.hbar.wheel(dx, now); return } }
        if self.job.is_some() || self.tracker.is_some() { self.list_mut().hbar.wheel(dx, now); }
    }
    /// The folder under the pointer that a new window can be opened on: a sidebar row, a segment of the path, and -
    /// `panes` - a folder in a pane.
    fn folder_at(&self, x: f64, y: f64, panes: bool) -> Option<PathBuf> {
        let p = kurbo::Point::new(x, y);
        if self.side_rect.contains(p) { return if self.sections.iter().any(|sec| sec.hit(x, y)) { None } else { self.side_rows.iter().find(|r| r.rect.contains(p) && r.enabled).and_then(|r| r.path.clone()) } }
        if self.focus != Focus::Path { if let Some(to) = self.crumbs.iter().find(|(b, path)| path.is_some() && b.hit(x, y)).and_then(|(_, path)| path.clone()) { return Some(to) } }
        if panes { for pane in self.panes.iter().take(self.shown()) { if pane.rect.contains(p) { return pane.folder_at(x, y) } } }
        None
    }
    /// A context menu for what is chosen in pane `i` (`row`: the click was on an entry), or for its folder.
    fn ask_menu(&mut self, i: usize, row: Option<usize>, at: (f64, f64)) {
        use crate::ctxmenu::{self, Ask};
        let pane = &self.panes[i]; let here = pane.path().to_path_buf();
        let entries = if row.is_some() { pane.chosen_entries(2000) } else { vec![] }; let sequence = pane.chosen_sequence();
        let ask = if entries.is_empty() { self.menu_for = Some((Some(i), here.clone())); Ask { at, paths: vec![here], rows: ctxmenu::rows_for_folder() } }
            else { let kinds: Vec<(bool, bool)> = entries.iter().map(|e| (e.1, e.2)).collect(); let folder = if entries.len() == 1 && (entries[0].1 || entries[0].2) { entries[0].0.clone() } else { here };
                // one folder: it can be made a job of the sidebar, or taken out of it
                let mut rows = ctxmenu::rows_for(&kinds, sequence);
                // one folder: it can be made a favourite of the sidebar (row 531)
                if kinds.len() == 1 && kinds[0].0 && !kinds[0].1 && !crate::places::is_favorite(&folder) { if let Some(k) = rows.iter().position(|r| *r == ctxmenu::Row::Sep) { rows.insert(k, ctxmenu::named(ctxmenu::FAV_ADD, "Add to Favorites".to_string())); } }
                if kinds.len() == 1 && kinds[0].0 && !kinds[0].1 { let on = self.jobs.iter().any(|j| j.path == folder); if let Some(k) = rows.iter().position(|r| *r == ctxmenu::Row::Sep) { rows.insert(k, ctxmenu::row_subscribe(on)); } }
                // archives (row 508): what is chosen is compressed; chosen archives are unpacked where they lie
                { let n = entries.len(); let archives = entries.iter().all(|e| !e.1 && e.0.file_name().is_some_and(|f| crate::archive::is_archive(&f.to_string_lossy())));
                    let mut add = vec![ctxmenu::Row::Sep]; if archives { add.push(ctxmenu::named(ctxmenu::EXTRACT, if n == 1 { "Extract Here".to_string() } else { format!("Extract {n} Archives Here") })); }
                    if n == 1 && pane.found().is_some() { if let Some(k) = rows.iter().position(|r| *r == ctxmenu::Row::Reveal) { rows.insert(k, ctxmenu::named(ctxmenu::ENCLOSING, "Show in Enclosing Folder".to_string())); } }
                    if n > 1 || sequence { if let Some(k) = rows.iter().position(|r| matches!(r, ctxmenu::Row::Ours { id, .. } if *id == ctxmenu::DUPLICATE)) { rows.insert(k, ctxmenu::named(ctxmenu::RENAME, if sequence && n == 1 { "Rename Sequence\u{2026}".to_string() } else { format!("Rename {n} Items\u{2026}") })); } }
                    if pane.chosen_media().is_some() { add.push(ctxmenu::named(ctxmenu::TRANSCODE, "Transcode to MP4".to_string())); }
                    add.push(ctxmenu::named(ctxmenu::COMPRESS, if n == 1 { "Compress to ZIP".to_string() } else { format!("Compress {n} Items to ZIP") }));
                    let at_row = rows.iter().rposition(|r| *r == ctxmenu::Row::Sep).unwrap_or(rows.len()); rows.splice(at_row..at_row, add); }
                self.menu_for = Some((Some(i), folder)); Ask { at, paths: entries.into_iter().map(|e| e.0).collect(), rows } };
        self.menu_job = false;
        self.show_menu(ask);
    }
    /// A context menu is shown: macOS, the system's own with our rows in it (the application's loop shows it);
    /// Windows, our own drawn one, whose last row opens Explorer's (row 460, point 2).
    /// The layers of the EXR sequence that plays, as a menu by the playbar's chip (row 551).
    fn ask_seq_layer(&mut self) {
        let Some((names, shown)) = self.lightbox.seq_layers() else { return };
        let rows = names.iter().enumerate().map(|(i, n)| crate::ctxmenu::named(crate::ctxmenu::LB_LAYER + i as u32, if n == shown { format!("\u{2713} {n}") } else { n.clone() })).collect();
        let at = (self.lb_layer.rect.x0, self.lb_layer.rect.y0); self.menu_for = None; self.show_menu(crate::ctxmenu::Ask { at, paths: vec![], rows }); self.lb_key = None;
    }
    fn show_menu(&mut self, mut ask: crate::ctxmenu::Ask) {
        use crate::ctxmenu::Row;
        // (whatever has a path can be copied as text: its path, its name, a link to it - row 503)
        self.menu_paths = ask.paths.clone(); if !ask.paths.is_empty() { ask.rows = crate::ctxmenu::with_copies(ask.rows); }
        if cfg!(target_os = "macos") && std::env::var_os("UFB_DRAWN_MENU").is_none() { self.out.push(Out::Menu(ask)); return }
        let (mut items, mut rows) = (vec![], vec![]);
        for r in &ask.rows { let (item, row) = match r {
                Row::Ours { id, label } => (min_ui::MenuItem::action(label.as_ref(), *id != 0), CtxRow::Ours(*id)),
                Row::Sep => (min_ui::MenuItem::Separator, CtxRow::Rule),
                Row::Open => (min_ui::MenuItem::action("Open", true), CtxRow::Open),
                Row::OpenWith => (min_ui::MenuItem::action("Open with\u{2026}", ask.paths.len() == 1), CtxRow::OpenAs),
                Row::Reveal => (min_ui::MenuItem::action(if cfg!(windows) { "Show in Explorer" } else { "Show in Finder" }, true), CtxRow::Reveal),
                Row::Share => continue,   // (the system's share sheet is the system's menu's: "Show more options")
            };
            if row == CtxRow::Rule && rows.last().is_none_or(|l| *l == CtxRow::Rule) { continue }
            items.push(item); rows.push(row); }
        while rows.last() == Some(&CtxRow::Rule) { rows.pop(); items.pop(); }
        // Explorer's own menu for the same files, with everything installed on the machine
        // (Explorer's menu is about files: a menu about nothing with a path - a share that is not there, Sync - has no such row)
        if (cfg!(windows) || std::env::var_os("UFB_DRAWN_MENU").is_some()) && !ask.paths.is_empty() { items.push(min_ui::MenuItem::Separator); rows.push(CtxRow::Rule); items.push(min_ui::MenuItem::action("Show more options", cfg!(windows))); rows.push(CtxRow::More); }
        let size = self.window.surface_size(); let s = self.scale();
        self.ctx_rows = rows; self.ctx_paths = ask.paths; self.ctx_at = ask.at;
        // (a row written with a tick in front - the layer shown, a share's type in force - is a CHECKED row of the drawn menu:
        // the tick is the icon face's, in a gutter; typed into the label it is a letter the text face does not have)
        let mut checks: Vec<Option<bool>> = vec![];
        if items.iter().any(|i| matches!(i, min_ui::MenuItem::Action { label, .. } if label.starts_with("\u{2713} "))) {
            for it in items.iter_mut() { checks.push(match it { min_ui::MenuItem::Action { label, .. } => { let on = label.starts_with("\u{2713} "); if on { *label = label.trim_start_matches("\u{2713} ").to_string(); } Some(on) } _ => None }); } }
        self.ctx.open_checked_at(items, checks, &mut self.fonts, ask.at.0, ask.at.1, Rect::new(0.0, 0.0, size.width as f64, size.height as f64), s);
        if std::env::var_os("UFB_DEMO").is_some() { eprintln!("UFB_DEMO: the drawn menu: {}", self.ctx.items.iter().map(|i| match i { min_ui::MenuItem::Action { label, enabled, .. } => format!("{label}{}", if *enabled { "" } else { " (off)" }), _ => "--".into() }).collect::<Vec<_>>().join(" | ")); }
    }
    /// A row of the drawn context menu was chosen.
    fn ctx_pick(&mut self, i: usize) {
        let paths = std::mem::take(&mut self.ctx_paths);
        match self.ctx_rows.get(i).copied() {
            Some(CtxRow::Ours(id)) => self.menu_chosen(id),
            Some(CtxRow::Open) => for p in paths.into_iter().take(20) { self.out.push(Out::OpenDefault(p)); },
            Some(CtxRow::OpenAs) => if let Some(p) = paths.into_iter().next() { self.out.push(Out::OpenAs(p)); },
            Some(CtxRow::Reveal) => self.out.push(Out::Reveal(paths)),
            Some(CtxRow::More) => self.out.push(Out::ShellMenu(self.ctx_at, paths)),
            _ => {}
        }
    }
    /// A row of OUR OWN in the context menu was chosen.
    pub fn menu_chosen(&mut self, id: u32) {
        use crate::ctxmenu::{COLUMN, SORT, SUBSCRIBE, UNSUBSCRIBE, CONTENTS, COPY, DUPLICATE, GROUP, NEW_FOLDER, NEW_TAB, NEW_WINDOW, OPEN, PASTE, PREVIEW, REFRESH, RENAME, SPLIT, TRASH};
        if id == crate::ctxmenu::BULK_UNTRACK || (crate::ctxmenu::BULK_SET..crate::ctxmenu::BULK_SET + 100).contains(&id) { return self.bulk_chosen(id) }
        if (crate::ctxmenu::LB_LAYER..crate::ctxmenu::LB_LAYER + 1000).contains(&id) { self.lightbox.set_seq_layer((id - crate::ctxmenu::LB_LAYER) as usize); return }
        if (crate::ctxmenu::COPY_PATH..=crate::ctxmenu::COPY_UNION).contains(&id) { if let Some(text) = Self::copy_text(id, &self.menu_paths) { self.out.push(Out::CopyText(text)); } return }
        // (a saved share's own rows)
        if (crate::ctxmenu::SHARE_TYPE..crate::ctxmenu::SHARE_TYPE + 500).contains(&id) { return self.share_type_chosen((id - crate::ctxmenu::SHARE_TYPE) as usize) }
        if (crate::ctxmenu::SYNC_SET..=crate::ctxmenu::SYNC_NEVER).contains(&id) { return self.sync_chosen(id) }
        if (crate::ctxmenu::SHARE_CONNECT..crate::ctxmenu::SYNC_SET).contains(&id) || id == crate::ctxmenu::SHARE_LETTER { return self.share_chosen(id) }
        // (the item list's own rows: columns, options, the star)
        if (crate::ctxmenu::META_ADD..70).contains(&id) || id >= crate::ctxmenu::META_SHOW { return self.meta_chosen(id) }
        let Some((pane, folder)) = self.menu_for.clone() else { return };
        match id {
            OPEN if self.menu_job => self.open_place(folder, false),
            crate::ctxmenu::OPEN_JOB => self.open_place(folder, true),
            NEW_TAB if self.menu_job => { self.new_tab(folder.clone()); self.open_place(folder, true); }
            OPEN | CONTENTS => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; } self.go(folder); }
            NEW_TAB => self.new_tab(folder),
            SUBSCRIBE => self.subscribe(folder, true), UNSUBSCRIBE => self.subscribe(folder, false),
            id if (COLUMN + 1..COLUMN + 4).contains(&id) => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; } self.toggle_column((id - COLUMN) as usize); }
            id if (SORT..SORT + 4).contains(&id) => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; } self.command(Cmd::Sort((id - SORT) as u8)); }
            NEW_WINDOW => self.out.push(Out::NewWindow(folder)),
            crate::ctxmenu::EXTRACT | crate::ctxmenu::COMPRESS | crate::ctxmenu::TRANSCODE => if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; match id { crate::ctxmenu::EXTRACT => self.extract_chosen(), crate::ctxmenu::COMPRESS => self.compress_chosen(), _ => self.transcode_chosen() } },
            crate::ctxmenu::FAV_ADD => { if crate::places::add_favorite(&folder, "") { self.reload_favorites(); } }
            crate::ctxmenu::FAV_REMOVE => { if crate::places::remove_favorite(&folder) { self.reload_favorites(); } }
            crate::ctxmenu::ENCLOSING => if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; self.show_enclosing(); },
            SPLIT => if let Some(i) = pane.filter(|i| *i < self.shown()) { self.panes[i].split_chosen(); },
            GROUP => if let Some(i) = pane.filter(|i| *i < self.shown()) { if !self.panes[i].group_chosen() { self.toast.show("These cannot be one sequence: their names differ in more than a number.", min_ui::toast::ToastKind::Notice, Instant::now()); } },
            crate::ctxmenu::NEW_UFB => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; } self.ask_new_dated(); }
            crate::ctxmenu::NEW_DATE | crate::ctxmenu::NEW_TIME => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; } self.new_stamped(id == crate::ctxmenu::NEW_TIME); }
            crate::ctxmenu::NEW_LINK | crate::ctxmenu::NEW_NOTE => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; } self.ask_new_file(id == crate::ctxmenu::NEW_NOTE); }
            REFRESH | RENAME | TRASH | NEW_FOLDER | DUPLICATE | COPY | PASTE | PREVIEW => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; }
                self.command(match id { RENAME => Cmd::Rename, TRASH => Cmd::Trash, NEW_FOLDER => Cmd::NewFolder, DUPLICATE => Cmd::Duplicate, COPY => Cmd::Copy, PASTE => Cmd::Paste, PREVIEW => Cmd::Preview, _ => Cmd::Refresh }); }
            _ => {}
        }
    }
    /// What the panes left for the window after an event: a double click on a file is the lightbox's when the lightbox
    /// shows that kind, else the default application's; "Open" on a file is always the default application's (row 473, point 2).
    fn after_pane_events(&mut self) {
        for i in 0..self.panes.len() {
            if let Some(row) = self.panes[i].preview.take() { match self.panes[i].entry(row) { Some((path, ..)) if !crate::lightbox::handles(&crate::lightbox::ext_of(&path)) || !crate::prefs::double_click_previews() => self.out.push(Out::OpenDefault(path)), Some(_) => self.open_lightbox(i, row), None => {} } }
            if let Some(path) = self.panes[i].open_default.take() { self.out.push(Out::OpenDefault(path)); }
        }
    }
    fn set_cursor(&mut self, c: winit31::cursor::CursorIcon) { if c != self.cursor { self.cursor = c; self.window.set_cursor(winit31::cursor::Cursor::Icon(c)); } }
    pub fn pointer(&mut self, e: &Event) {
        if let Some(p) = e.pos() { self.pointer = p; } let now = Instant::now(); let s = self.scale(); let port = self.port.clone();
        if self.lightbox.is_open() { return self.lightbox_pointer(e) }
        // a column's editor is a question of its own: it takes everything; the chips' popover takes the pointer while it is open
        if let Some(p) = self.prefs.as_mut() { let out = p.pointer(e, &mut self.fonts); self.prefs_done(out); return }
        if let Some(ed) = self.coledit.as_mut() { if let Some(done) = ed.event(e, &mut self.fonts, now) { self.column_done(done); } return }
        // the small editor of a notes cell: the pointer is its own inside its card; a press outside it saves and closes
        if let Some(n) = self.note.as_mut() { let inside = e.pos().is_some_and(|(x, y)| n.card.contains(kurbo::Point::new(x, y)));
            if matches!(e, Event::Down { .. }) && !inside { self.note_done(true); return } n.area.pointer(e, &mut self.fonts); return }
        if let Some(p) = self.chip.as_mut() { let (pick, close) = p.event(e); let (item, col) = (p.item.clone(), p.col); if close { self.chip = None; } if let Some(pick) = pick { self.chip_picked(item, col, pick); } return }
        // the drawn context menu takes the pointer while it is open
        if self.ctx.is_open() { let (chosen, _, consumed) = self.ctx.event(e); if let Some(i) = chosen { self.ctx_pick(i); } if consumed { return } }
        if matches!(e, Event::Down { .. }) { self.tips.pressed(now); self.tip_demo = None; }
        // the drawn menu bar and its open menus take the pointer first
        if let Some(mb) = self.menubar.as_mut() { let (chosen, _, consumed) = mb.bar.event(e, &mut self.fonts); if let Some(c) = chosen.and_then(|id| mb.cmd(id)) { self.command(c); return } if consumed { return } }
        // an open question takes everything
        if let Some((d, _)) = self.dialog.as_mut() { d.pointer(e, &mut self.fonts); let (clicked, _) = d.event(e, now); if let Some(b) = clicked { self.answer(b); } return }
        { let (_, _, consumed) = self.toast.event(e); if consumed { return } }
        // a name being changed: the field takes what is on it; a press anywhere else takes the name as typed (as Finder does)
        if let Some(r) = self.rename.as_mut() {
            let on = e.pos().is_some_and(|(x, y)| r.field.hit(x, y));
            if on || r.field.dragging() { r.field.pointer(e, &mut self.fonts); return }
            if matches!(e, Event::Down { .. }) { self.commit_rename(); if self.rename.is_some() || self.dialog.is_some() { return } }
        }
        // the running operation in the status bar: its cancel; a press on it opens (or closes) the list of what waits
        if self.op_list && self.task_card.width() > 0.0 {
            let mut act = None; for (id, b) in self.task_btns.iter_mut() { if b.rect.width() > 0.0 && b.event(e, now) == Response::Clicked { act = Some(*id); } }
            if let Some(id) = act { if crate::tasks::list().iter().any(|t| t.id == id && t.state.over()) { crate::tasks::dismiss(id); } else { crate::tasks::cancel(id); } return }
            if self.task_clear.event(e, now).0 { crate::tasks::clear_finished(); return }
            if let Some((x, y)) = e.pos() { if self.task_card.contains(kurbo::Point::new(x, y)) { return } }
        }
        if self.op_rect.width() > 0.0 {
            if self.op_cancel.rect.width() > 0.0 && self.op_cancel.event(e, now) == Response::Clicked { crate::ops::cancel(); return }
            if let Event::Down { x, y, button: Button::Primary } = *e { if self.op_rect.contains(kurbo::Point::new(x, y)) && !(self.op_cancel.rect.width() > 0.0 && self.op_cancel.hit(x, y)) { self.op_list = !self.op_list; return } else if self.op_list { self.op_list = false; } }
        }
        // the strip of tabs (row 480; drawn when there are two or more): a press shows a tab, its cross or the middle
        // button closes it, the plus makes a new one
        if self.tabs.len() > 1 && self.tab_rects.len() == self.tabs.len() {
            if self.tab_new.event(e, now) == Response::Clicked { self.command(Cmd::NewTab); return }
            let mut close = None; for (i, b) in self.tab_close.iter_mut().enumerate() { if b.rect.width() > 0.0 && b.event(e, now) == Response::Clicked { close = Some(i); } }
            if let Some(i) = close { self.close_tab(i); return }
            if let Event::Down { x, y, button } = *e { if let Some(i) = self.tab_rects.iter().position(|r| r.contains(kurbo::Point::new(x, y))) {
                if button == Button::Middle { self.close_tab(i); return }
                if button == Button::Primary && !self.tab_close.get(i).is_some_and(|b| b.rect.width() > 0.0 && b.hit(x, y)) { self.show_tab(i); return } } }
        }
        // a new TAB (rows 448, 480 - a new window until tabs came; "Open in New Window" stays in the menus): the middle
        // button on a folder, a sidebar row or a segment of the path; the command key (Ctrl on Windows) with a click on a
        // sidebar row or a segment - in a pane that click is the selection's
        if let Event::Down { x, y, button } = *e { let middle = button == Button::Middle;
            if middle || (button == Button::Primary && self.mods.cmd) { if let Some(p) = self.folder_at(x, y, middle) { let job = self.side_rect.contains(kurbo::Point::new(x, y)) && self.side_rows.iter().any(|r| r.job && r.path.as_deref() == Some(p.as_path())); self.new_tab(p.clone()); if job { self.open_place(p, true); } return } } }
        if matches!(e, Event::Down { button: Button::Middle, .. } | Event::Up { button: Button::Middle, .. }) { return }
        // a gap in hand: the sidebar's edge or the divider between the panes
        if let Some(d) = self.drag {
            match *e {
                Event::Move { x, .. } => match d {
                    Drag::Sidebar(x0, w0) => self.side_w = (w0 + (x - x0) / s).clamp(SIDE_MIN, SIDE_MAX),
                    Drag::Split => { let r = self.panes_area; let (a, b, at) = if self.stacked { (r.y0, r.y1, self.pointer.1) } else { (r.x0, r.x1, x) }; if b - a > 1.0 { self.split = ((at - a) / (b - a)).clamp(0.15, 0.85); } }
                    Drag::JobList(x0, w0) => self.job_w = (w0 + (x - x0) / s).clamp(JOB_LIST_MIN, 1200.0),
                },
                // (a divider is where it was left when the next window opens - row 523)
                Event::Up { button: Button::Primary, .. } | Event::Leave => { if let Some(d) = self.drag.take() { let (side, split, job, stacked) = (self.side_w, self.split, self.job_w, self.stacked);
                    crate::settings::with(|s| match d { Drag::Sidebar(..) => s.set_float("window/sidebar", side), Drag::Split => s.set_float(if stacked { "window/job_split" } else { "window/split" }, split), Drag::JobList(..) => s.set_float("window/job_list", job) }); } }
                _ => {}
            }
            return
        }
        let at = e.pos().map(|(x, y)| kurbo::Point::new(x, y));
        let over = at.and_then(|p| self.gaps.iter().position(|g| g.width() > 0.0 && g.contains(p)).filter(|i| match i { 0 => true, 1 => self.two, _ => self.job.is_some() }));
        if let (Event::Down { x, button: Button::Primary, .. }, Some(g)) = (*e, over) { self.drag = Some(match g { 0 => Drag::Sidebar(x, self.side_w), 1 => Drag::Split, _ => Drag::JobList(x, self.job_w) }); return }
        self.over_gap = over;
        // the toolbar
        if self.back.event(e, now) == Response::Clicked { self.command(Cmd::Back); }
        if self.forward.event(e, now) == Response::Clicked { self.command(Cmd::Forward); }
        if self.up.event(e, now) == Response::Clicked { self.command(Cmd::Up); }
        if self.preview_btn.event(e, now) == Response::Clicked { self.command(Cmd::Preview); }
        if self.seq_btn.event(e, now) == Response::Clicked { self.command(Cmd::Sequences); }
        if self.list_btn.event(e, now) == Response::Clicked { self.command(Cmd::AsList); }
        if self.grid_btn.event(e, now) == Response::Clicked { self.command(Cmd::AsGrid); }
        if self.pane2_btn.event(e, now) == Response::Clicked { self.command(Cmd::SecondPane); }
        if self.size.rect.width() > 0.0 { if let (Some(v), _) = self.size.event(e, now, s) { self.pane().grid.tile = grid::TILE_MIN + v as f64 * (grid::TILE_MAX - grid::TILE_MIN); } }
        // the path bar: a segment goes there; its empty part, pressed, takes a typed path
        if self.focus == Focus::Path {
            self.field.pointer(e, &mut self.fonts);
            if let Event::Down { x, y, .. } = *e { if !self.field.hit(x, y) { self.leave_path(); } }
        } else {
            let mut to = None; let mut on_crumb = false;
            for (b, path) in self.crumbs.iter_mut() { if b.event(e, now) == Response::Clicked { to = path.clone(); } if let Event::Down { x, y, .. } = *e { on_crumb |= b.hit(x, y); } }
            if let Some(p) = to { self.go(p); }
            else if let Event::Down { x, y, button: Button::Primary } = *e { if !on_crumb && self.path_rect.contains(kurbo::Point::new(x, y)) { self.edit_path(); return } }
        }
        // the search field
        self.search.pointer(e, &mut self.fonts);
        if let Event::Down { x, y, .. } = *e { if self.search.hit(x, y) { self.leave_path(); self.focus = Focus::Search; self.search.focused = true; } else if self.focus == Focus::Search { self.leave_search(); } }
        // the sidebar
        self.side_bar.event(e, now);
        // the shares: the plus in their header (a new share), the action on a row (Connect, Sign In), a row's own menu
        if self.share_add.rect.width() > 0.0 && self.share_add.event(e, now) == Response::Clicked { use min_ui::{dialog::Dialog, text_button::Variant};
            let mut d = Dialog::new("Add a Share", "Its address - smb://server/share or \\\\server\\share - or a folder on this machine that holds the same storage.", vec![("Cancel", Variant::Secondary), ("Add", Variant::Primary)]).with_field("Address or folder", "smb://server/share"); d.show_with("", &mut self.fonts); self.dialog = Some((d, Asked::AddShare)); return }
        // (the OTHER button is the row's menu, wherever on the row it is pressed - also on the row's own button. Taken by
        // the button, a share that would not connect had most of its row deaf to it: no way to its Remove - row 550)
        if !matches!(*e, Event::Down { button: Button::Secondary, .. } | Event::Up { button: Button::Secondary, .. })
        { let mut go = None; for (k, b) in self.share_btns.iter_mut().enumerate() { if b.rect.width() > 0.0 && b.event(e, now) == Response::Clicked { go = Some(k); } } if let Some(k) = go { if self.shares.get(k).is_some_and(|sh| self.share_holders.contains_key(&sh.id)) { self.fix_share(k); } else { self.connect_share(k, true); } return }
            if let Event::Down { x, y, .. } = *e { if self.share_btns.iter().any(|b| b.rect.width() > 0.0 && b.hit(x, y)) || (self.share_add.rect.width() > 0.0 && self.share_add.hit(x, y)) { return } } }
        // the Sync entry: a press opens its settings (or turns it on when they are there), the other button its menu
        if let Event::Down { x, y, button } = *e { if self.side_rect.contains(kurbo::Point::new(x, y)) && self.sync_rect.contains(kurbo::Point::new(x, y)) { let (on, leader) = crate::settings::with(|s| (s.bool("sync/on"), s.string("sync/leader")));
            if button == Button::Secondary { self.menu_for = None; self.show_menu(crate::ctxmenu::Ask { at: (x, y), paths: vec![], rows: crate::ctxmenu::rows_for_sync(on, &leader) }); } else { self.ask_sync(); } return } }
        if let Event::Down { x, y, button: Button::Secondary } = *e { let p = kurbo::Point::new(x, y); if let Some(k) = self.side_rows.iter().find(|r| r.rect.contains(p)).and_then(|r| r.share).filter(|_| self.side_rect.contains(p)) {
            let (sh, st) = (self.shares[k].clone(), self.share_states.get(k).cloned().unwrap_or(crate::shares::State::NotConnected)); let root = st.root().map(Path::to_path_buf);
            self.share_menu = Some(k); self.menu_job = false; self.menu_for = root.clone().map(|r| (None, r));
            self.show_menu(crate::ctxmenu::Ask { at: (x, y), paths: root.into_iter().collect(), rows: crate::ctxmenu::rows_for_share(st.root().is_some(), sh.ours, !sh.server.is_empty(), sh.at_launch, sh.jobs, &crate::jobview::known_types().into_iter().map(|t| t.0).collect::<Vec<_>>(), &sh.ptype) }); return } }
        for sec in self.sections.iter_mut() { sec.event(e, now); }
        self.side_hover = at.filter(|p| self.side_rect.contains(*p)).and_then(|p| self.side_rows.iter().position(|r| r.rect.contains(p)));
        if let Event::Down { x, y, button: Button::Primary } = *e { let p = kurbo::Point::new(x, y);
            if self.side_rect.contains(p) && self.side_rows.iter().any(|r| r.all && r.rect.contains(p)) { self.open_tracker(true); return }
            if self.side_rect.contains(p) && !self.sections.iter().any(|sec| sec.hit(x, y)) { if let Some((to, job)) = self.side_rows.iter().find(|r| r.rect.contains(p) && r.enabled).and_then(|r| r.path.clone().map(|q| (q, r.job))) {
                // (a job's row too is opened as a FOLDER, in the pane in front - the owner, 2026-11-19: the job view is asked for by
                // name, "Open as Job" in the row's menu, or with the middle button / the command key, which open it in a new tab)
                let _ = job; self.open_place(to, false); } } }
        // ... and its menu: a place opens here or in a new window (row 448)
        if let Event::Down { x, y, button: Button::Secondary } = *e { if let Some(to) = self.folder_at(x, y, false).filter(|_| self.side_rect.contains(kurbo::Point::new(x, y))) {
            let job = self.side_rows.iter().any(|r| r.job && r.path.as_deref() == Some(to.as_path()));
            // (a favourite the user made can be taken out of the sidebar again - row 531)
            let mut rows = if job { crate::ctxmenu::rows_for_job() } else { crate::ctxmenu::rows_for_place() }; if !job && crate::places::is_favorite(&to) { rows.push(crate::ctxmenu::Row::Sep); rows.push(crate::ctxmenu::named(crate::ctxmenu::FAV_REMOVE, "Remove from Favorites".to_string())); }
            self.menu_for = Some((None, to.clone())); self.menu_job = job; self.show_menu(crate::ctxmenu::Ask { at: (x, y), paths: vec![to], rows }); } }
        // the job: its sections, its refresh, the item list (a press chooses an item and takes the arrow keys; the
        // secondary button gives the item's menu), Add
        if self.job.is_some() {
            let mut show = None; for (b, name) in self.job_secs.iter_mut().zip(self.job_secs_for.iter()) { if b.event(e, now) == Response::Clicked { show = Some(name.clone()); } }
            if let Some(name) = show { self.job_show_section(&name); }
            if self.job_refresh.event(e, now) == Response::Clicked { self.command(Cmd::Refresh); }
            if self.trk_btn.rect.width() > 0.0 && self.trk_btn.event(e, now) == Response::Clicked { if self.tracker.is_some() { self.tracker = None; } else { self.open_tracker(false); } return }
            if self.tracker.is_none() {
            if self.job_add.rect.width() > 0.0 && self.job_add.event(e, now) == Response::Clicked { self.ask_add_item(); return }
            if self.job_meta.rect.width() > 0.0 && self.job_meta.event(e, now) == Response::Clicked { let r = self.job_meta.rect; self.columns_menu(None, (r.x0, r.y0 - 4.0 * s)); return }
            if self.board_btn.rect.width() > 0.0 && self.board_btn.event(e, now) == Response::Clicked { let r = self.board_btn.rect; self.board_menu((r.x0, r.y0 - 4.0 * s)); return }
            // the board stands in the list's place: its cards are pressed, and dragged from lane to lane
            if self.board.is_some() {
                let (acts, inside, dragging) = match self.board.as_mut() { Some(b) => { let inside = e.pos().is_some_and(|(x, y)| b.rect.contains(kurbo::Point::new(x, y))); (b.event(e, now), inside, b.dragging()) } None => (vec![], false, false) };
                if matches!(e, Event::Down { .. }) && inside { self.leave_path(); self.leave_search(); self.focus = Focus::Items; }
                let any = !acts.is_empty(); self.board_acts(acts);
                if inside || any || dragging { self.set_cursor(if dragging { winit31::cursor::CursorIcon::Grabbing } else { winit31::cursor::CursorIcon::Default }); return }
            }
            // a cell being typed into: its field takes what is on it; a press anywhere else takes what was typed
            if let Some(ed) = self.items.edit.as_mut() {
                let on = e.pos().is_some_and(|(x, y)| ed.field.hit(x, y));
                if on || ed.field.dragging() { ed.field.pointer(e, &mut self.fonts); return }
                if matches!(e, Event::Down { .. }) && !self.commit_cell() { return }
            }
            let in_list = e.pos().is_some_and(|(x, y)| self.items.rect.contains(kurbo::Point::new(x, y)));
            let acts = match self.job.as_ref() { Some(j) if self.board.is_none() => self.items.event(e, &j.items, now), _ => vec![] };
            if matches!(e, Event::Down { .. }) && in_list { self.leave_path(); self.leave_search(); if self.focus != Focus::Cell { self.focus = Focus::Items; } }
            let any = !acts.is_empty(); self.items_acts(acts);
            if (matches!(e, Event::Down { .. }) && in_list) || any { self.set_cursor(winit31::cursor::CursorIcon::Default); return }
            if self.items.resizing() { self.set_cursor(winit31::cursor::CursorIcon::ColResize); return }
            if self.items.on_link() { self.set_cursor(winit31::cursor::CursorIcon::Pointer); return }
            }
        }
        // the tracker: its Due chips, its list (a press chooses, two go to the item; the cells are edited as in a job's list)
        if self.tracker.is_some() {
            let mut due = None; for (k, b) in self.due_btns.iter_mut().enumerate() { if b.rect.width() > 0.0 && b.event(e, now) == Response::Clicked { due = Some(k); } }
            if let (Some(k), Some(tr)) = (due, self.tracker.as_mut()) { tr.due = crate::tracker::DUES[k].0; tr.filter(); return }
            if let Some(ed) = self.tracker.as_mut().and_then(|tr| tr.list.edit.as_mut()) {
                let on = e.pos().is_some_and(|(x, y)| ed.field.hit(x, y));
                if on || ed.field.dragging() { ed.field.pointer(e, &mut self.fonts); return }
                if matches!(e, Event::Down { .. }) && !self.commit_cell() { return }
            }
            let (acts, in_list, resizing, on_link) = match self.tracker.as_mut() { Some(tr) => { let crate::tracker::Tracker { list, rows, .. } = tr; let inl = e.pos().is_some_and(|(x, y)| list.rect.contains(kurbo::Point::new(x, y))); (list.event(e, rows, now), inl, list.resizing(), list.on_link()) } None => (vec![], false, false, false) };
            if matches!(e, Event::Down { .. }) && in_list { self.leave_path(); self.leave_search(); if self.focus != Focus::Cell { self.focus = Focus::Items; } }
            let any = !acts.is_empty(); self.tracker_acts(acts);
            if (matches!(e, Event::Down { .. }) && in_list) || any { self.set_cursor(winit31::cursor::CursorIcon::Default); return }
            if resizing { self.set_cursor(winit31::cursor::CursorIcon::ColResize); return }
            if on_link { self.set_cursor(winit31::cursor::CursorIcon::Pointer); return }
        }
        // (in a job with nothing chosen no pane is shown: nothing below is there to take the pointer)
        if self.blank() { self.set_cursor(if self.over_gap.is_some() { winit31::cursor::CursorIcon::ColResize } else { winit31::cursor::CursorIcon::Default }); return }
        let n = self.shown();
        // the secondary button on a list's header: the fields to sort by (a narrow pane does not show them all, row 483)
        if let Event::Down { x, y, button: Button::Secondary } = *e { for i in 0..n { if self.panes[i].mode == Mode::List && self.panes[i].table.header().contains(kurbo::Point::new(x, y)) {
            self.active = i; let here = self.panes[i].path().to_path_buf(); self.menu_for = Some((Some(i), here.clone())); self.menu_job = false; self.show_menu(crate::ctxmenu::Ask { at: (x, y), paths: vec![here], rows: crate::ctxmenu::rows_for_sort(self.col_layouts[self.col_kind(i)].show) }); return } } }
        if self.job_prox.rect.width() > 0.0 && self.job_prox.event(e, now) == Response::Clicked { if let Some(j) = self.job.as_mut() { j.proxies = !j.proxies; } self.job_follow(false, true); return }
        // the panes: a press in one makes it the active one
        if let Event::Down { x, y, .. } = *e { let p = kurbo::Point::new(x, y); for i in 0..n { if self.panes[i].rect.contains(p) || self.pane_heads[i].contains(p) { self.active = i; self.leave_path(); self.leave_search(); self.focus = Focus::Pane; } } }
        for pane in self.panes.iter_mut().take(n) { pane.pointer(e, self.mods, now, &port); }
        for i in 0..n { if let Some((row, at)) = self.panes[i].menu_at.take() { self.ask_menu(i, row, at); } }
        self.after_pane_events();
        // a press on what is chosen was moved: the system's drag of it begins (the layer needs the pointer's event to be the current one: this is still inside it)
        for i in 0..n { if std::mem::take(&mut self.panes[i].drag_out) { let paths = self.panes[i].chosen_files(); if !paths.is_empty() { self.out.push(Out::Drag(paths)); } } }
        let resizing = self.over_gap.is_some() || self.panes.iter().take(n).any(|p| p.resizing());
        let text = at.is_some_and(|p| (self.focus == Focus::Path && self.path_rect.contains(p)) || self.search.rect.contains(p));
        self.set_cursor(if self.over_gap == Some(1) && self.stacked { winit31::cursor::CursorIcon::RowResize } else if resizing { winit31::cursor::CursorIcon::ColResize } else if text { winit31::cursor::CursorIcon::Text } else { winit31::cursor::CursorIcon::Default });
    }
    /// Text from the clipboard, at the command key with V in a text field. (The window asks for it; this type holds no clipboard.)
    pub fn paste(&mut self, text: &str) { if let Some(n) = self.note.as_mut() { n.area.paste(text, &mut self.fonts); return } if let Some(p) = self.prefs.as_mut() { p.paste(text, &mut self.fonts); return } if let Some(ed) = self.coledit.as_mut() { ed.paste(text, &mut self.fonts); return } if self.chip.is_some() { return } if let Some((d, _)) = self.dialog.as_mut() { d.paste(text, &mut self.fonts); self.dialog_preview(); return } match self.focus { Focus::Rename => if let Some(r) = self.rename.as_mut() { r.field.error = false; r.field.paste(text, &mut self.fonts); }, Focus::Path => { self.field.paste(text, &mut self.fonts); } Focus::Search => { if self.search.paste(text, &mut self.fonts) == FieldResponse::Changed { let t = self.search.text().to_string(); self.pane().set_search(&t); } } Focus::Cell => { let l = match self.tracker.as_mut() { Some(t) => &mut t.list, None => &mut self.items }; if let Some(e) = l.edit.as_mut() { e.field.error = false; e.field.paste(text, &mut self.fonts); } } Focus::Pane | Focus::Items => {} } }
    /// A key for the drawn menu bar: (the command of the row chosen, redraw, the key was the bar's).
    pub fn with_menu_key(&mut self, k: &min_ui::BarKey) -> (Option<Cmd>, bool, bool) { match self.menubar.as_mut() { Some(mb) => { let (id, redraw, consumed) = mb.bar.key(k, &mut self.fonts); (id.and_then(|i| mb.cmd(i)), redraw, consumed) } None => (None, false, false) } }
    /// A text field holds the keyboard (the clipboard's keys are its).
    pub fn typing(&self) -> bool { self.text_focus() }
    /// The chosen text of the field in hand, for a copy; `cut` takes it out.
    pub fn copy(&mut self, cut: bool) -> Option<String> { if let Some(n) = self.note.as_mut() { return n.area.copy(cut, &mut self.fonts) } if let Some(p) = self.prefs.as_mut() { return p.copy(cut, &mut self.fonts) } if let Some(ed) = self.coledit.as_mut() { return ed.copy(cut, &mut self.fonts) } if self.chip.is_some() { return None } if let Some((d, _)) = self.dialog.as_mut() { let r = d.copy(cut, &mut self.fonts); if cut { self.dialog_preview(); } return r } match self.focus { Focus::Rename => self.rename.as_mut().and_then(|r| r.field.copy(cut, &mut self.fonts)), Focus::Path => self.field.copy(cut, &mut self.fonts), Focus::Search => { let r = self.search.copy(cut, &mut self.fonts); if cut { let t = self.search.text().to_string(); self.pane().set_search(&t); } r } Focus::Cell => { let l = match self.tracker.as_mut() { Some(t) => &mut t.list, None => &mut self.items }; l.edit.as_mut().and_then(|e| e.field.copy(cut, &mut self.fonts)) } Focus::Pane | Focus::Items => None } }
    /// A key. `name`: the key as the key map names it ("N", "[", "Up", "F5"); `alt`: the Alt / Option key is down;
    /// `menu_keys`: the system's menu bar answers the command-key shortcuts itself (macOS), so they are not taken here.
    pub fn key(&mut self, k: &KeyEvent, name: &str, alt: bool, menu_keys: bool) {
        let now = Instant::now(); let port = self.port.clone(); let mac = cfg!(target_os = "macos"); self.tip_demo = None;
        if self.lightbox.is_open() { if self.ctx.is_open() { if matches!(k.key, Key::Named(Named::Escape)) { self.ctx.close(); self.lb_key = None; } return } return self.lightbox_key(k, name) }
        if let Some(p) = self.prefs.as_mut() { let out = p.key(k, &mut self.fonts); self.prefs_done(out); return }
        if let Some(ed) = self.coledit.as_mut() { if let Some(done) = ed.key(k, &mut self.fonts) { self.column_done(done); } return }
        if matches!(k.key, Key::Named(Named::Escape)) && self.board.as_mut().is_some_and(|b| b.cancel_drag()) { return }
        // (Return is a new line in a note; the command key with Return saves, Escape leaves it as it was)
        if let Some(n) = self.note.as_mut() { match n.area.key(k, &mut self.fonts) { FieldResponse::Accepted => self.note_done(true), FieldResponse::Cancelled => self.note_done(false), _ => {} } return }
        if let Some(p) = self.chip.as_mut() { let s = self.window.scale_factor(); let (pick, close) = p.key(k, &mut self.fonts, s); let (item, col) = (p.item.clone(), p.col); if close { self.chip = None; } if let Some(pick) = pick { self.chip_picked(item, col, pick); } return }
        if self.ctx.is_open() { if matches!(k.key, Key::Named(Named::Escape)) { self.ctx.close(); } return }
        // an open question: Return and Escape both give its last button - the careful answer; a button is pressed by the mouse
        // a question with fields: the keys are the field's (Return gives its last button when the answer is usable, Escape its first)
        if self.dialog.as_ref().is_some_and(|(d, _)| !d.fields.is_empty()) {
            let closed = self.dialog.as_mut().and_then(|(d, _)| d.key(k, &mut self.fonts)); self.dialog_preview();
            if let Some(b) = closed { let usable = self.dialog.as_ref().is_some_and(|(d, _)| d.buttons.last().is_some_and(|l| l.enabled)); if b == 0 || usable { self.answer(b); } else if let Some((d, _)) = self.dialog.as_mut() { d.show(); } }
            return
        }
        if let Some((d, asked)) = self.dialog.as_ref() { let last = d.buttons.len().saturating_sub(1);
            match k.key { Key::Named(Named::Enter) => self.answer(last), Key::Named(Named::Escape) => self.answer(if matches!(asked, Asked::Conflict(_)) { 0 } else { last }), _ => {} } return }
        match self.focus {
            Focus::Rename => {
                let Some(r) = self.rename.as_mut() else { self.focus = Focus::Pane; return };
                match r.field.key(k, &mut self.fonts) { FieldResponse::Accepted => self.commit_rename(), FieldResponse::Cancelled => self.end_rename(), FieldResponse::Changed => r.field.error = false, FieldResponse::None => {} }
                return
            }
            Focus::Path => {
                match self.field.key(k, &mut self.fonts) { FieldResponse::Accepted => self.submit_path(), FieldResponse::Cancelled => self.leave_path(), FieldResponse::Changed => { self.path_error = None; self.field.error = false; } FieldResponse::None => {} }
                return
            }
            Focus::Search => {
                if matches!(k.key, Key::Named(Named::Down | Named::Tab)) { self.leave_search(); return }
                match self.search.key(k, &mut self.fonts) {
                    FieldResponse::Cancelled => { self.search.clear(); let port = self.port.clone(); self.pane().leave_found(&port); self.pane().set_search(""); self.leave_search(); }
                    // Return: the folders below are searched too (rows 508 point 13, 514); typing on leaves what was found
                    FieldResponse::Accepted => { let (text, port) = (self.search.text().to_string(), self.port.clone()); self.pane().search_below(&port, &text); self.leave_search(); }
                    FieldResponse::Changed => { let (text, port) = (self.search.text().to_string(), self.port.clone()); self.pane().leave_found(&port); self.pane().set_search(&text); }
                    FieldResponse::None => {}
                }
                // the command-key shortcuts still act from the search field (a second pane, the view), but not its own editing keys
                if !(k.cmd && !matches!(&k.key, Key::Char(c) if matches!(c.to_ascii_lowercase().as_str(), "a" | "c" | "v" | "x" | "z"))) || menu_keys { return }
            }
            // the item list of a job: the arrows choose, Tab and Escape give the keys back to the browser
            // a cell being typed into: Return takes it, Escape leaves it as it was, Tab takes it and opens the next cell that is typed into
            Focus::Cell => {
                let fonts = &mut self.fonts; let l = match self.tracker.as_mut() { Some(t) => &mut t.list, None => &mut self.items };
                let Some(ed) = l.edit.as_mut() else { self.focus = Focus::Items; return };
                if matches!(k.key, Key::Named(Named::Tab)) {
                    let (item, col) = (ed.item.clone(), ed.col); if !self.commit_cell() { return }
                    let typed = |c: &crate::db::ColDef| !c.uuid.starts_with('#') && !matches!(crate::db::kind_of(&c.kind), "checkbox") && !(matches!(crate::db::kind_of(&c.kind), "dropdown" | "priority") && !c.options.is_empty());
                    let cols = self.list_mut().cols.clone(); let n = cols.len(); let next = (1..n).map(|d| if k.shift { (col + n - d) % n } else { (col + d) % n }).find(|c| typed(&cols[*c]));
                    let at = match &self.tracker { Some(t) => t.rows.iter().position(|it| it.path == item), None => self.job.as_ref().and_then(|j| j.items.iter().position(|it| it.path == item)) };
                    if let (Some(next), Some(i)) = (next, at) { self.edit_cell(i, next); }
                    return
                }
                match ed.field.key(k, fonts) { FieldResponse::Accepted => { self.commit_cell(); } FieldResponse::Cancelled => self.cancel_cell(), FieldResponse::Changed => ed.field.error = false, FieldResponse::None => {} }
                return
            }
            // the tracker: the arrows go through its rows, Return goes to the item in its job
            Focus::Items if self.tracker.is_some() => {
                if !k.cmd && !alt { if let Some(t) = self.tracker.as_mut() { let order = t.list.order.clone(); let n = order.len(); let at = t.chosen.and_then(|i| order.iter().position(|k| *k == i));
                    let to = match k.key { Key::Named(Named::Down) => Some(at.map(|i| (i + 1).min(n.saturating_sub(1))).unwrap_or(0)), Key::Named(Named::Up) => Some(at.map(|i| i.saturating_sub(1)).unwrap_or(0)), Key::Named(Named::Home) => Some(0), Key::Named(Named::End) => Some(n.saturating_sub(1)), _ => None };
                    if let Some(pos) = to.filter(|_| n > 0) { t.chosen = Some(order[pos]); t.list.reveal(pos); return }
                    if matches!(k.key, Key::Named(Named::Enter)) { if let Some(p) = t.chosen.and_then(|i| t.rows.get(i)).map(|r| r.path.clone()) { self.open_tracked(p); } return } } }
                if !k.cmd { return }
            }
            Focus::Items => {
                // (the arrows go through the items in the order SHOWN)
                let order = self.items.order.clone(); let n = order.len(); let at = self.job.as_ref().and_then(|j| j.chosen).and_then(|i| order.iter().position(|k| *k == i));
                if !k.cmd && !alt { match k.key {
                    Key::Named(Named::Down) => { if n > 0 { self.job_choose(order[at.map(|i| (i + 1).min(n - 1)).unwrap_or(0)]); self.job_reveal(); } return }
                    Key::Named(Named::Up) => { if n > 0 { self.job_choose(order[at.map(|i| i.saturating_sub(1)).unwrap_or(0)]); self.job_reveal(); } return }
                    Key::Named(Named::Home) => { if n > 0 { self.job_choose(order[0]); self.job_reveal(); } return }
                    Key::Named(Named::End) => { if n > 0 { self.job_choose(order[n - 1]); self.job_reveal(); } return }
                    Key::Named(Named::Tab | Named::Escape | Named::Right) => { self.focus = Focus::Pane; return }
                    _ => {}
                } }
                if !k.cmd { return }
            }
            Focus::Pane => {}
        }
        if let Some(c) = menu::shortcut(mac, name, k.cmd, k.shift, alt).filter(|_| !(menu_keys && k.cmd && name != "Down")) { self.command(c); return }
        match &k.key {
            // Tab switches panes when the second is shown (row 450)
            Key::Named(Named::Tab) if !k.cmd && !alt => { if self.two && self.panes.len() > 1 { self.active = 1 - self.active.min(1); } return }
            // Space is the lightbox's (a later slice): nothing yet, and it is not a letter to find a name by
            // Space is the lightbox's (row 452) - unless a name is being typed to find it, where it is a letter
            Key::Char(c) if c == " " && !k.cmd && !self.blank() => { if self.pane().sel().typed(now).is_empty() { self.command(Cmd::Preview); return } }
            Key::Named(Named::Enter) if mac => return,
            Key::Named(Named::Escape) => { if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } return }
            _ => {}
        }
        if (alt && !mac) || self.blank() { return }
        self.pane().key(k, now, &port); self.after_pane_events();
    }

    // ---------------------------------------------------------------- the scripted looks
    /// `UFB_DEMO` steps, done one after another once the panes have what they wait for (no system input is involved):
    /// `grid` `list` `size:<px>` `pane2` `tab` `expand:<row>` `collapse:<row>` `select:<a>-<b>` `selectall` `sort:<name|date|size|kind>[:desc]`
    /// `search:<text>` `go:<path>` `back` `forward` `up` `editpath` `typepath:<text>` `hidden` `foldersfirst` `scroll:<row>` `side:<n>` (fold a
    /// sidebar section) `sidebar:<px>` `menu:<letter>` (the drawn menu bar) `tip:<back|up|list|grid|pane2>` `newwindow` `middle:<row>` `wait:<ms>`.
    fn demo_step(&mut self, step: &str) {
        let port = self.port.clone(); let (word, arg) = step.split_once(':').unwrap_or((step, "")); let num = |a: &str| a.parse::<usize>().ok();
        match word {
            "grid" => self.command(Cmd::AsGrid), "list" => self.command(Cmd::AsList), "pane2" => self.set_two(true), "hidden" => self.command(Cmd::Hidden), "foldersfirst" => self.command(Cmd::FoldersFirst),
            "tab" => if self.two { self.active = 1 - self.active.min(1); },
            "size" => if let Ok(px) = arg.parse::<f64>() { let px = px.clamp(grid::TILE_MIN, grid::TILE_MAX); self.pane().grid.tile = px; self.size.value = ((px - grid::TILE_MIN) / (grid::TILE_MAX - grid::TILE_MIN)) as f32; },
            "expand" => if let Some(n) = num(arg) { self.pane().expand(&port, n); },
            "collapse" => if let Some(n) = num(arg) { self.pane().collapse(n); },
            "select" => { let (a, b) = arg.split_once('-').unwrap_or((arg, arg)); if let (Some(a), Some(b)) = (num(a), num(b)) { self.pane().select_rows(a, b); } }
            "selectall" => self.pane().select_all(),
            "sort" => { let (by, dir) = arg.split_once(':').unwrap_or((arg, "asc")); if let Some(by) = SortBy::parse(by) { self.pane().set_sort(by, dir != "desc"); } }
            "search" => { self.search.set_text(arg); self.pane().set_search(arg); }
            "go" => self.go(PathBuf::from(arg)), "back" => self.command(Cmd::Back), "forward" => self.command(Cmd::Forward), "up" => self.command(Cmd::Up),
            "editpath" => self.edit_path(),
            "typepath" => { self.edit_path(); self.field.set_text(arg); self.submit_path(); }
            "scroll" => if let Some(n) = num(arg) { self.pane().scroll_to(n); },
            "side" => if let Some(sec) = num(arg).and_then(|n| self.sections.get_mut(n)) { sec.expanded = !sec.expanded; },
            "sidebar" => if let Ok(px) = arg.parse::<f64>() { self.side_w = px.clamp(SIDE_MIN, SIDE_MAX); },
            "menu" => if let (Some(mb), Some(ch)) = (self.menubar.as_mut(), arg.chars().next()) { mb.bar.key(&min_ui::BarKey::Alt(ch.to_ascii_lowercase()), &mut self.fonts); },
            "newwindow" => self.command(Cmd::NewWindow),
            // tabs: `newtab[:<path>]` `tabshow:<n>` `tabclose:<n>` `tabnext` `tabprev` `tabs` (says them; the one in front is starred)
            "newtab" => if arg.is_empty() { self.command(Cmd::NewTab) } else { self.new_tab(PathBuf::from(arg)) },
            "tabshow" => if let Some(n) = num(arg) { self.show_tab(n); }, "tabclose" => if let Some(n) = num(arg) { self.close_tab(n); }, "tabnext" => self.command(Cmd::NextTab), "tabprev" => self.command(Cmd::PrevTab),
            // the project side: `jobs` (says the sidebar's) `job:<n>` (opens the n-th) `section:<name>` `item:<n>` `items` (says them)
            // `subscribe:<path>` `unsubscribe:<path>` `newjob:<number>|<name>` (in the folder in view) `additem:<name>` `itemfocus`
            "jobs" => eprintln!("UFB_DEMO: jobs: {}", self.jobs.iter().map(|j| format!("{} [{}]", j.name, j.path.display())).collect::<Vec<_>>().join(" | ")),
            "job" => if let Some(j) = num(arg).and_then(|n| self.jobs.get(n)).cloned() { self.open_place(j.path, true); },
            "section" => self.job_show_section(arg),
            "item" => if let Some(n) = num(arg) { self.job_choose(n); },
            "proxies" => { if let Some(j) = self.job.as_mut() { j.proxies = !j.proxies; } self.job_follow(false, true); }
            "headmenu" => { let h = self.pane().table.header(); let (x, y) = ((h.x0 + h.x1) / 2.0, (h.y0 + h.y1) / 2.0); self.pointer(&Event::Down { x, y, button: Button::Secondary }); }
            // the items' metadata: `col:<type>:<name>[:<options>]` (a column added) `cell:<item>:<column>:<text>` (typed into a cell, as its field would)
            // `star:<item>` `check:<item>:<column>` `option:<item>:<column>:<n>` `colsort:<name|modified|column number>` `colmenu[:<column>]` `colpick:<row>` `meta` (says the list)
            "col" => { let mut p = arg.splitn(3, ':'); if let (Some(kind), Some(name), Some((job, sec))) = (p.next(), p.next(), self.section_key()) { let opts = crate::itemlist::parse_options(p.next().unwrap_or("")); let r = crate::db::with(|d| d.add_column(&job, &sec, name, kind, &opts).map(|_| ())); self.meta_err(r); self.items_reload(); } },
            "cell" => { let mut p = arg.splitn(3, ':'); if let (Some(i), Some(k), Some(text)) = (p.next().and_then(num), p.next().and_then(num), p.next()) { self.edit_cell(i, k);
                // (a notes or links cell opens its small editor: `\n` in the step's text is a new line)
                if let Some(n) = self.note.as_mut() { n.area.set_text(&text.replace("\\n", "\n")); self.note_done(true); return } if let Some(e) = self.list_mut().edit.as_mut() { e.field.set_text(text); } let ok = self.commit_cell(); if !ok { eprintln!("UFB_DEMO: the cell did not take {text:?}"); self.cancel_cell(); } } },
// several rows of the tracker: `trksel:<a>,<b>,...` (their places) `bulksay` (the menu's rows) `bulkpick:<n>` (its n-th row; 0 = Untrack); the board: `cardsel:<a>,<b>` then `carddrop`
            "trksel" => if let Some(t) = self.tracker.as_mut() { t.sel = arg.split(',').filter_map(|n| n.trim().parse::<usize>().ok()).filter_map(|i| t.rows.get(i).map(|r| r.path.clone())).collect(); },
            "bulksay" => { let n = self.tracker.as_ref().map(|t| t.sel.len()).unwrap_or(0); let rows = self.bulk_rows(n); eprintln!("UFB_DEMO: bulk menu: {}", rows.iter().map(|r| match r { crate::ctxmenu::Row::Ours { label, .. } => label.to_string(), _ => "--".into() }).collect::<Vec<_>>().join(" | ")); },
            "bulkpick" => if let Some(n) = num(arg) { let k = self.tracker.as_ref().map(|t| t.sel.len()).unwrap_or(0); self.bulk_rows(k); self.bulk_chosen(if n == 0 { crate::ctxmenu::BULK_UNTRACK } else { crate::ctxmenu::BULK_SET + n as u32 - 1 }); },
            "cardsel" => if let Some(b) = self.board.as_mut() { b.sel = arg.split(',').filter_map(|n| n.trim().parse::<usize>().ok()).collect(); },
            "notesay" => eprintln!("UFB_DEMO: note open: {:?}", self.note.as_ref().map(|n| n.area.text().to_string())), "notecancel" => self.note_done(false),
            "celledit" => { let mut p = arg.splitn(2, ':'); if let (Some(i), Some(k)) = (p.next().and_then(num), p.next().and_then(num)) { self.edit_cell(i, k); } },
            "star" => if let Some(i) = num(arg) { if self.tracker.is_some() { self.tracker_acts(vec![crate::itemlist::Act::Star(i)]); } else { self.items_acts(vec![crate::itemlist::Act::Star(i)]); } },
            "check" => { let mut p = arg.splitn(2, ':'); if let (Some(i), Some(k)) = (p.next().and_then(num), p.next().and_then(num)) { if self.tracker.is_some() { self.tracker_acts(vec![crate::itemlist::Act::Check(i, k)]); } else { self.items_acts(vec![crate::itemlist::Act::Check(i, k)]); } } },
            "option" => { let mut p = arg.splitn(3, ':'); if let (Some(i), Some(k), Some(n)) = (p.next().and_then(num), p.next().and_then(num), p.next().and_then(num)) { self.meta_ctx = (Some(k), Some(i), vec![], vec![]); self.meta_chosen(crate::ctxmenu::META_OPTION + n as u32); } },
            "colsort" => { use crate::itemlist::SortBy; let by = match arg { "name" => Some(SortBy::Name), "modified" => Some(SortBy::Modified), a => num(a).and_then(|k| self.items.cols.get(k)).map(|c| SortBy::Col(c.uuid.clone())) };
                if let (Some(by), Some(j)) = (by, self.job.as_ref()) { self.items.sort = match &self.items.sort { Some((b, true)) if *b == by => Some((by, false)), Some((b, false)) if *b == by => None, _ => Some((by, true)) }; self.items.reorder(&j.items); } },
            "colmenu" => { let r = self.items.rect; self.columns_menu(num(arg), (r.x0 + 40.0, r.y0 + 10.0)); },
            "colpick" => if let Some(id) = arg.parse::<u32>().ok() { self.ctx.close(); self.meta_chosen(id); },
            "meta" => if let Some(j) = self.job.as_ref() { eprintln!("UFB_DEMO: columns [{}] hidden [{}]; {}", self.items.cols.iter().map(|c| format!("{}:{}", c.name, c.kind)).collect::<Vec<_>>().join(", "), self.items.hidden.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "),
                self.items.order.iter().filter_map(|i| j.items.get(*i)).map(|it| format!("{}{} [{}]", if self.items.tracked(it) { "*" } else { "" }, it.name, self.items.cols.iter().map(|c| crate::itemlist::text_of(&c.kind, self.items.value(it, c))).collect::<Vec<_>>().join(" | "))).collect::<Vec<_>>().join("; ")); },
            // the chips: `chipopen:<item>:<column>` `chiptype:<text>` `chipkey:<Enter|Escape|Down>` `coledit:<column>` (its editor) `coleditsave`
            "chipopen" => { let mut p = arg.splitn(2, ':'); if let (Some(i), Some(k)) = (p.next().and_then(num), p.next().and_then(num)) { let pos = self.items.order.iter().position(|x| *x == i).unwrap_or(0); let r = self.items.cell_rect(pos, k); self.items_acts(vec![crate::itemlist::Act::Options(i, k, (r.x0, r.y1))]); } },
            "chiptype" => for ch in arg.chars() { let k = KeyEvent { key: Key::Char(ch.to_string()), text: Some(ch.to_string()), shift: false, cmd: false }; self.key(&k, "", false, false); },
            "chipkey" => { let key = match arg { "Enter" => Key::Named(Named::Enter), "Escape" => Key::Named(Named::Escape), "Down" => Key::Named(Named::Down), _ => Key::Named(Named::Up) }; self.key(&KeyEvent { key, text: None, shift: false, cmd: false }, arg, false, false); },
            "coledit" => if let Some(c) = num(arg).and_then(|k| self.items.cols.get(k).cloned()) { self.ask_column(Some(c.id), &c.kind, &c.name, &c.options); },
            "swatches" => { let (row, custom) = arg.split_once(':').map(|(a, b)| (a, b == "custom")).unwrap_or((arg, false)); if let (Some(ed), Some(row)) = (self.coledit.as_mut(), num(row)) { ed.show_swatches(row, custom); } },
            "coleditsave" => { self.key(&KeyEvent { key: Key::Named(Named::Enter), text: None, shift: false, cmd: true }, "Enter", false, false); },
            // the tracker: `tracker` (the job's; again closes it) `alltracked` `trk` (says it) `due:<0-4>` `trkopen:<row>` - `cell:`, `check:` and `star:` act on its rows while it is in front
            "tracker" => if self.tracker.is_some() { self.tracker = None; } else { self.open_tracker(false); }, "alltracked" => self.open_tracker(true),
            "due" => if let (Some(k), Some(tr)) = (num(arg).filter(|k| *k < 5), self.tracker.as_mut()) { tr.due = crate::tracker::DUES[k].0; tr.filter(); },
            // rows whose folder is gone: `relink:<row>:<path>` (as the chooser would answer) `trkremove:<row>`
            "relink" => { let mut p = arg.splitn(2, ':'); if let (Some(old), Some(new)) = (p.next().and_then(num).and_then(|i| self.tracker.as_ref().and_then(|t| t.rows.get(i).map(|r| r.path.clone()))), p.next()) { self.relink(old, PathBuf::from(new)); } },
            "trkremove" => if let Some(p) = num(arg).and_then(|i| self.tracker.as_ref().and_then(|t| t.rows.get(i).map(|r| r.path.clone()))) { let r = crate::db::with(|d| d.remove_item(&p)); self.meta_err(r); self.reload_lists(); },
            "trkopen" => if let Some(p) = num(arg).and_then(|i| self.tracker.as_ref().and_then(|t| t.rows.get(i).map(|r| r.path.clone()))) { self.open_tracked(p); },
            "trk" => if let Some(tr) = self.tracker.as_ref() { eprintln!("UFB_DEMO: tracker {:?} due {:?} columns [{}]; {}", tr.scope == crate::tracker::Scope::All, tr.due, tr.list.cols.iter().map(|c| format!("{}:{}", c.name, c.kind)).collect::<Vec<_>>().join(", "),
                tr.list.order.iter().filter_map(|i| tr.rows.get(*i)).map(|it| format!("{}{} [{}]", it.name, if tr.lost(&it.path) { " (NOT FOUND)" } else { "" }, tr.list.cols.iter().map(|c| crate::itemlist::text_of(&c.kind, tr.list.value(it, c))).collect::<Vec<_>>().join(" | "))).collect::<Vec<_>>().join("; ")); },
            // the board: `board:<a column's name|tracked|off>` `boardsay` `carddrop:<item>:<lane>` (the item's card let go on that lane)
            "board" => if let Some(key) = self.section_key() { if arg == "off" { self.board_of.remove(&key); } else if let Some((by, _)) = crate::board::eligible(&self.items).into_iter().find(|(_, n)| n.eq_ignore_ascii_case(arg)) { self.board_of.insert(key, by); } self.items_reload(); },
            "boardsay" => match self.board.as_ref() { Some(b) => eprintln!("UFB_DEMO: board by {:?}: {}", b.by, b.lanes.iter().map(|l| format!("{} [{}]", l.title, l.items.iter().filter_map(|i| self.job.as_ref().and_then(|j| j.items.get(*i))).map(|it| it.name.as_str()).collect::<Vec<_>>().join(", "))).collect::<Vec<_>>().join(" | ")), None => eprintln!("UFB_DEMO: no board (the list)") },
            "carddrop" => { let mut p = arg.splitn(2, ':'); if let (Some(i), Some(k)) = (p.next().and_then(num), p.next().and_then(num)) { if let Some(v) = self.board.as_ref().and_then(|b| b.lanes.get(k).map(|l| l.value.clone())) { self.board_acts(vec![crate::board::Act::Drop(i, v)]); } } },
            // the mesh: `mesh` says its state
            "mesh" => eprintln!("UFB_DEMO: mesh {:?}", crate::mesh::status().map(|m| format!("{} leader {:?} peers {} pending {} written {} merged {} secret {} strangers {}", m.node, m.leader, m.peers, m.pending, m.written > 0, m.merged > 0, !m.secret.is_empty(), m.strangers))),
            // the saved shares: `share:add:<address or folder>` `share:connect:<n>` `share:disconnect:<n>` `share:jobs:<n>` `share:remove:<n>` `shares` (says them)
            "share" => { let (what, rest) = arg.split_once(':').unwrap_or((arg, "")); match what { "add" => self.add_share(rest), "connect" => if let Some(n) = num(rest) { self.connect_share(n, true); }, other => if let Some(n) = num(rest) { self.share_menu = Some(n); self.share_chosen(match other { "disconnect" => crate::ctxmenu::SHARE_DISCONNECT, "jobs" => crate::ctxmenu::SHARE_JOBS, "remove" => crate::ctxmenu::SHARE_REMOVE, _ => 0 }); } } },
            // sync's settings as its dialog saves them: `sync:<farm>|<name>|<only>`
            "sync" => { let mut it = arg.split('|'); let (farm, node, only) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or("")); crate::settings::with(|s| { s.set_str("sync/farm", farm); s.set_str("sync/node", node); s.set_str("sync/only", only); s.set_bool("sync/on", true); }); self.apply_sync(); },
            // a jobs folder's type: `share:type:<n>:<name>` is done here (`sharetype:<n>:<name>`; an empty name: the built-in one); `types` says those found
            "sharetype" => { let (n, name) = arg.split_once(':').unwrap_or((arg, "")); if let Some(n) = num(n) { self.share_menu = Some(n); let k = if name.is_empty() { Some(0) } else { crate::jobview::known_types().iter().position(|t| t.0.eq_ignore_ascii_case(name)).map(|k| k + 1) }; match k { Some(k) => self.share_type_chosen(k), None => eprintln!("UFB_DEMO: no type {name}") } } },
            "types" => eprintln!("UFB_DEMO: types: {} | shares: {}", crate::jobview::known_types().iter().map(|t| t.0.clone()).collect::<Vec<_>>().join(", "), self.shares.iter().map(|s| format!("{}={}", s.name, if s.ptype.is_empty() { "built-in" } else { &s.ptype })).collect::<Vec<_>>().join(", ")),
            // what the three copies would put on the clipboard (said, not copied): `copytext:<path|name|link>:<path>`; a text as typed into the path bar: `typed:<text>`
            "copytext" => { let (what, p) = arg.split_once(':').unwrap_or((arg, "")); eprintln!("UFB_DEMO: copy {what}: {:?}", Self::copy_text(match what { "name" => crate::ctxmenu::COPY_NAME, "link" => crate::ctxmenu::COPY_LINK, "union" => crate::ctxmenu::COPY_UNION, _ => crate::ctxmenu::COPY_PATH }, &[PathBuf::from(p)])); },
            "typed" => { self.edit_path(); self.field.set_text(arg); self.submit_path(); },
            "syncoff" => self.sync_chosen(crate::ctxmenu::SYNC_ONOFF),
            "shares" => eprintln!("UFB_DEMO: shares: {}", self.shares.iter().enumerate().map(|(i, sh)| format!("{} [{}{}{}] {:?}", sh.name, if sh.server.is_empty() { format!("folder {}", sh.local) } else { format!("smb://{}/{}", sh.server, sh.share) }, if sh.jobs { ", jobs" } else { "" }, if sh.ours { ", ours" } else { "" }, (self.share_states.get(i), self.share_holders.contains_key(&sh.id).then_some("held")))).collect::<Vec<_>>().join(" | ")),
            "remembered" => eprintln!("UFB_DEMO: remembered drives: {}", crate::shares::remembered_text()),
            "joblist" => if let Ok(px) = arg.parse::<f64>() { self.job_w = px.clamp(JOB_LIST_MIN, 1200.0); },
            "pane2toggle" => self.command(Cmd::SecondPane),
            "itemfocus" => if self.job.is_some() { self.focus = Focus::Items; },
            "items" => if let Some(j) = self.job.as_ref() { eprintln!("UFB_DEMO: job {} sections [{}] in front {:?} (two tiers: {}): items [{}] chosen {:?}; panes at {}", j.name, j.sections.join(", "), j.section, j.two_tier, j.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join(", "), j.chosen_item().map(|i| i.name.as_str()), if self.blank() { "(nothing shown)".to_string() } else { self.panes.iter().take(self.shown()).map(|p| format!("{}{}{}", p.path().display(), if p.history.can_back() { " <back" } else { "" }, if p.can_up() { " ^up" } else { "" })).collect::<Vec<_>>().join(" + ") }); },
            "subscribe" => self.subscribe(PathBuf::from(arg), true), "unsubscribe" => self.subscribe(PathBuf::from(arg), false),
            "newjob" => { self.ask_new_job(); if let Some((d, _)) = self.dialog.as_mut() { let (n, name) = arg.split_once('|').unwrap_or((arg, "")); d.fields[0].1.set_text(n); d.fields[1].1.set_text(name); } self.dialog_preview(); if self.dialog.as_ref().is_some_and(|(d, _)| d.buttons.last().is_some_and(|b| b.enabled)) { self.answer(1); } },
            // `newlink:<address>|<name>` and `newnote:<name>` answer the question at once; `newlinkask:<address>|<name>` and `newnoteask:<name>` leave it open
            "newufb" | "newufbask" => { self.ask_new_dated(); if let Some((d, _)) = self.dialog.as_mut() { d.fields[0].1.set_text(arg); } self.dialog_preview(); if word == "newufb" && self.dialog.as_ref().is_some_and(|(d, _)| d.buttons.last().is_some_and(|b| b.enabled)) { self.answer(1); } },
            "newdate" => self.new_stamped(false), "newtime" => self.new_stamped(true),
            "newlink" | "newlinkask" => { self.ask_new_file(false); if let Some((d, _)) = self.dialog.as_mut() { let (u, name) = arg.split_once('|').unwrap_or((arg, "")); d.fields[0].1.set_text(u); d.fields[1].1.set_text(name); } self.dialog_preview(); if word == "newlink" && self.dialog.as_ref().is_some_and(|(d, _)| d.buttons.last().is_some_and(|b| b.enabled)) { self.answer(1); } },
            "newnote" | "newnoteask" => { self.ask_new_file(true); if let Some((d, _)) = self.dialog.as_mut() { d.fields[0].1.set_text(arg); } self.dialog_preview(); if word == "newnote" && self.dialog.as_ref().is_some_and(|(d, _)| d.buttons.last().is_some_and(|b| b.enabled)) { self.answer(1); } },
            "additem" => { self.ask_add_item(); if let Some((d, _)) = self.dialog.as_mut() { d.fields[0].1.set_text(arg); } self.dialog_preview(); if self.dialog.as_ref().is_some_and(|(d, _)| d.buttons.last().is_some_and(|b| b.enabled)) { self.answer(1); } },
            "tabs" => eprintln!("UFB_DEMO: tabs: {}", (0..self.tabs.len()).map(|i| { let (name, path) = self.tab_place(i); let panes = if i == self.tab { self.shown() } else if self.tabs[i].two { 2 } else { 1 }; format!("{}{name} [{}; {panes} pane(s)]", if i == self.tab { "*" } else { "" }, path.display()) }).collect::<Vec<_>>().join(" | ")),
            // the file operations, as the menus give them: `newfolder` `rename` `type:<text>` (into the name's field) `accept` `cancel` `duplicate` `trash` `deletenow` `undo` `answer:<button>`
            "redo" => self.command(Cmd::Redo), "newfolder" => self.command(Cmd::NewFolder), "rename" => self.command(Cmd::Rename), "duplicate" => self.command(Cmd::Duplicate), "trash" => self.command(Cmd::Trash), "deletenow" => self.command(Cmd::DeleteNow), "undo" => self.command(Cmd::Undo),
            "type" => if let Some(r) = self.rename.as_mut() { r.field.set_text(arg); },
            "accept" => self.commit_rename(), "cancel" => self.end_rename(),
            "answer" => if let Some(b) = num(arg) { self.answer(b); },
            // the drawn context menu: `ctxpick:<row>` chooses a row of the one that is open
            "ctxpick" => if let Some(i) = num(arg) { if self.ctx.is_open() { self.ctx.close(); self.ctx_pick(i); } },
            // sequences (row 475): `sequences` (the toggle) `split` (the chosen sequence as its files) `group` (the chosen files as one)
            "sequences" => self.command(Cmd::Sequences), "split" => self.pane().split_chosen(), "group" => { let ok = self.pane().group_chosen(); eprintln!("UFB_DEMO: grouped by hand: {ok}"); }
            // the lightbox: `preview` (the row under the cursor; again closes it) `lbnext` `lbprev` `lbzoom:<factor>` `lbactual` `lbfit` `lbpan:<dx>,<dy>`
            // what plays: `lbkey:<K|Q|E|M|V|Home|End|Up|Down>` (a key of the transport) `lbseek:<0-1>` (the line taken to there)
            // how the playing went: the player's own count, and the gaps between our frames
            "lbstats" => if let Some(p) = self.lightbox.view.as_ref().and_then(|v| v.player.as_ref()) {
                let st = &p.t.stats; let q = |v: &[f32], f: f32| { let mut v = v.to_vec(); v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v.get(((v.len() as f32 - 1.0) * f) as usize).copied().unwrap_or(0.0) };
                eprintln!("UFB_DEMO: played: shown {} dropped {} late {} black {} loops {}; cadence (refreshes a frame -> frames) {:?}; decode ms median {:.1} worst {:.1}; lateness ms median {:.1} worst {:.1}", st.shown, st.dropped, st.late, st.black, st.loops, st.cadence, q(&st.decode_ms, 0.5), q(&st.decode_ms, 1.0), q(&st.lateness_ms, 0.5), q(&st.lateness_ms, 1.0));
                eprintln!("UFB_DEMO: our frames: {} drawn; gap ms median {:.1} p95 {:.1} worst {:.1}; cost to draw ms median {:.1} p95 {:.1} worst {:.1}; codec {} hw '{}' {}x{} {:.3} fps", self.lb_gaps.len(), q(&self.lb_gaps, 0.5), q(&self.lb_gaps, 0.95), q(&self.lb_gaps, 1.0), q(&self.lb_cost, 0.5), q(&self.lb_cost, 0.95), q(&self.lb_cost, 1.0), p.t.info().codec_short, p.t.info().hw_accel, p.t.info().width, p.t.info().height, p.t.rate().as_f64());
            },
            // a sequence's rate stepped on (`lbfps:-1`: back)
            // a document scrolled by so many points, or made larger by a factor
            "lbscroll" => if let Some(d) = self.lightbox.view.as_ref().and_then(|v| v.doc.as_ref()) { d.borrow_mut().wheel(arg.parse::<f64>().unwrap_or(0.0) * self.scale(), Instant::now()); },
            "lbdoczoom" => if let Some(d) = self.lightbox.view.as_ref().and_then(|v| v.doc.as_ref()) { d.borrow_mut().zoom_by(arg.parse().unwrap_or(1.0)); },
            // a picture of the window taken by the SYSTEM (our own capture has no web view in it); macOS
            "lbwebshot" => { let n = crate::webview::window_number(&self.window); if n > 0 { let _ = std::process::Command::new("screencapture").args(["-x", "-o", &format!("-l{n}"), arg]).status(); } },
            // a script run INSIDE the page of the web view (a link pressed, a key made there: nothing is sent to the desktop)
            "lbwebjs" => if let Some(w) = self.lightbox.view.as_ref().and_then(|v| v.web.as_ref()) { w.eval(arg); },
            "lbfps" => if let Some(p) = self.lightbox.view.as_mut().and_then(|v| v.player.as_mut()) { p.step_rate(arg.parse().unwrap_or(1)); eprintln!("UFB_DEMO: the sequence plays at {} fps", crate::lightbox::rate_text(p.rate())); self.lb_key = None; },
            "lbkey" => { let k = KeyEvent { key: Key::Char(arg.to_lowercase()), text: None, shift: false, cmd: false }; self.lightbox_key(&k, arg); }
            "lbseek" => if let (Ok(at), Some(p)) = (arg.parse::<f64>(), self.lightbox.view.as_mut().and_then(|v| v.player.as_mut())) { p.scrub(at); p.scrub_end(); },
            "lbseqlayer" => if let Some(i) = num(arg) { self.lightbox.set_seq_layer(i); }, "lbseqmenu" => self.ask_seq_layer(),
            "lbseqsay" => eprintln!("UFB_DEMO: sequence layers: {:?}", self.lightbox.seq_layers().map(|(n, s)| (n.to_vec(), s.to_string()))),
            "lblayer" => if let Some(i) = num(arg) { self.lightbox.pick_layer(i); }, "lbback" => { if self.lightbox.back_to_layers() { self.presenter.engine.clear_picture(); } }
            "preview" => self.command(Cmd::Preview), "lbnext" => self.step_lightbox(1), "lbprev" => self.step_lightbox(-1),
            "lbzoom" | "lbactual" | "lbfit" | "lbpan" => { let area = self.lightbox_area(); let mid = (area.center().x, area.center().y); if let Some(v) = self.lightbox.view.as_mut() { match word {
                "lbzoom" => v.zoom_by(arg.parse().unwrap_or(1.0), mid, area), "lbactual" => v.actual(mid, area), "lbfit" => v.fit(),
                _ => if let Some((dx, dy)) = arg.split_once(',').and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?))) { v.drag_begin(mid); v.drag_to((mid.0 + dx, mid.1 + dy), area); v.drag_end(); } } } }
            // a drag of ours has ended, as the layer would tell it
            "dragended" => { crate::dnd::on_notice(&self.dnd, min_dnd::Notice::Ended { serial: 0, op: min_dnd::MOVE, detail: String::new() }); }
            // a drag of a file, as the layer would tell it (no drag is performed): `over:<row>:<path>[:copy|move]` shows where it would land,
            // `drop:<row|bg|side<n>>:<path>[:copy|move]` lets it go there - on a row's folder, the pane itself, a sidebar row
            "over" | "drop" => { let (target, rest) = arg.split_once(':').unwrap_or((arg, ""));   // (a Windows path has a colon of its own: only a last ":copy" / ":move" is the keys')
                let (path, keys) = match rest.rsplit_once(':') { Some((p, k)) if k == "copy" || k == "move" => (p, k), _ => (rest, "") };
                let at = if target == "bg" { let r = self.pane().rect; Some((r.x1 - 30.0, r.y1 - 30.0)) } else if target == "fav" { let r = self.sections[0].rect; Some((r.x0 + 40.0, (r.y0 + r.y1) / 2.0)) } else if let Some(k) = target.strip_prefix("side").and_then(num) { self.side_rows.get(k).map(|r| ((r.rect.x0 + r.rect.x1) / 2.0, (r.rect.y0 + r.rect.y1) / 2.0)) } else { num(target).and_then(|k| self.pane().row_point(k)) };
                if let Some((x, y)) = at { use min_dnd::{DragEvent, Notice, Payload, Phase, COPY, LINK, MOVE};
                    let allowed = match keys { "copy" => COPY, "move" => MOVE, _ => COPY | MOVE | LINK }; let payload = std::rc::Rc::new(Payload { paths: vec![PathBuf::from(path)], ..Default::default() });
                    let tell = |phase| crate::dnd::on_notice(&self.dnd, Notice::Drag(DragEvent { win: 0, phase, x, y, allowed, detail: String::new(), mine: None, payload: payload.clone() }));
                    let op = tell(Phase::Enter); eprintln!("UFB_DEMO: a drag over {target} would be {}", match op { COPY => "a copy", MOVE => "a move", LINK => "a favourite made", _ => "refused" });
                    if word == "drop" { tell(Phase::Drop); } } }
            // `copyother` `moveother` (the chosen entries into the other pane's folder) `all` (tick "Apply to all") `stopop` (call the running operation off) `oplist`
            // the chosen entries shown as CUT (the look only: a script never touches the clipboard)
            "cutshow" => { let paths = self.chosen_paths(); crate::pane::set_cut(&paths); },
            "copyother" => self.command(Cmd::CopyToOther), "moveother" => self.command(Cmd::MoveToOther), "stopop" => crate::ops::cancel(), "oplist" => self.op_list = true,
            // `tasks` (say the list) `taskstop:<n>` `taskdismiss:<n>` (the n-th row of the list) `taskclear`
            "tasksay" => eprintln!("UFB_DEMO: tasks: {}", crate::tasks::list().iter().map(|t| format!("{} [{:?}]", t.label, t.state)).collect::<Vec<_>>().join(" | ")),
            "taskstop" => if let Some(t) = num(arg).and_then(|n| crate::tasks::list().get(n).cloned()) { crate::tasks::cancel(t.id); },
            "taskdismiss" => if let Some(t) = num(arg).and_then(|n| crate::tasks::list().get(n).cloned()) { crate::tasks::dismiss(t.id); },
            "taskclear" => crate::tasks::clear_finished(),
            "taskscroll" => if let Some(n) = num(arg) { self.task_off = n; },
            // the Settings card: `prefs` (open it) `prefset:<hidden|foldersfirst|sequences|grid|sync>:<on|off>` `prefset:<farm|node|only>:<text>` `prefsave` `prefcancel` `prefsay`
            "prefs" => self.open_prefs(),
            "prefset" => if let (Some(p), Some((what, value))) = (self.prefs.as_mut(), arg.split_once(':')) { p.demo_set(what, value, &mut self.fonts); },
            "prefsave" => if let Some(p) = self.prefs.as_mut() { let out = p.demo_save(); if out == crate::prefs::Out::None { eprintln!("UFB_DEMO: settings not saved: {}", self.prefs.as_ref().and_then(|p| p.error()).unwrap_or("")); } self.prefs_done(out); },
            "prefcancel" => self.prefs = None,
            "loginsay" => eprintln!("UFB_DEMO: start at sign-in: can be switched {}; now {}", crate::login::available(), crate::login::enabled()),
            "preftab" => if let (Some(p), Some(k)) = (self.prefs.as_mut(), num(arg)) { p.demo_tab(k); },
            // `prefmap:add` (a new mapping's row) `prefmap:<n>:<name|win|mac>:<text>` `prefmap:<n>:off|on|remove` (the n-th hand-written one) `prefshare:<n>:<win|mac>:<text>` (a saved share's pair) `mapsay`
            "prefmap" => if let Some(p) = self.prefs.as_mut() { p.demo_map(arg, false, &mut self.fonts); },
            "prefshare" => if let Some(p) = self.prefs.as_mut() { p.demo_map(arg, true, &mut self.fonts); },
            "mapsay" => eprintln!("UFB_DEMO: mappings: {}", crate::db::mappings().iter().map(|m| format!("[{} <-> {}{}]", m.win, m.mac, m.here.as_ref().map(|h| format!(" here {h}")).unwrap_or_default())).collect::<Vec<_>>().join(" ")),
            "prefsay" => { let v = crate::prefs::view(); eprintln!("UFB_DEMO: settings: hidden {} folders-first {} sequences {} grid {}; sync {}; this window: hidden {} mode {:?} sequences {}", v.0, v.1, v.2, v.3, crate::settings::with(|s| format!("{} farm \"{}\" node \"{}\"", s.bool("sync/on"), s.string("sync/farm"), s.string("sync/node"))), self.hidden, self.panes[self.active].mode, self.panes[self.active].model.sequences); },
            // `extract` `compress` (the chosen entries) `password:<text>` (answer the password question)
            // `searchbelow:<text>` (Return in the search field) `enclosing` (the chosen find, where it lies) `searchclear`
            "searchbelow" => { self.search.set_text(arg); let port = self.port.clone(); self.pane().search_below(&port, arg); },
            "enclosing" => self.show_enclosing(),
            "searchclear" => { self.search.clear(); let port = self.port.clone(); self.pane().leave_found(&port); self.pane().set_search(""); },
            // `batchrename:<find>|<replace with>` (the chosen entries, as Rename with several chosen; it is carried out when it may be)
            "batchrename" => { self.ask_batch_rename(); let (f, w) = arg.split_once('|').unwrap_or((arg, "")); if let Some((d, Asked::BatchRename(_))) = self.dialog.as_mut() { d.fields[0].1.set_text(f); d.fields[1].1.set_text(w); } self.dialog_preview();
                if let Some((d, _)) = self.dialog.as_ref() { eprintln!("UFB_DEMO: batch rename: {}", d.body); } if self.dialog.as_ref().is_some_and(|(d, _)| d.buttons.last().is_some_and(|b| b.enabled)) { self.answer(1); } else { self.dialog = None; } },
            "favadd" => { if crate::places::add_favorite(Path::new(arg), "") { self.reload_favorites(); } }, "favremove" => { if crate::places::remove_favorite(Path::new(arg)) { self.reload_favorites(); } },
            "favsay" => eprintln!("UFB_DEMO: favorites: {}", self.favorites.iter().map(|f| f.label.clone()).collect::<Vec<_>>().join(", ")),
            "extract" => self.extract_chosen(), "compress" => self.compress_chosen(), "transcode" => self.transcode_chosen(),
            "password" => if matches!(self.dialog, Some((_, Asked::Password(_)))) { if let Some((d, _)) = self.dialog.as_mut() { d.paste(arg, &mut self.fonts); } self.answer(1); },   // (as pasted: the field hides its text and takes none set from outside)
            "all" => if let Some((d, _)) = self.dialog.as_mut() { if let Some(c) = d.check.as_mut() { c.1 = true; } },
            // the secondary button on a row, on the pane's empty part (`context:bg`), on a sidebar row (`context:side<n>`)
            "context" => { let at = if arg == "bg" { let r = self.pane().rect; Some((r.x1 - 30.0, r.y1 - 30.0)) } else if let Some(n) = arg.strip_prefix("side").and_then(num) { self.side_rows.get(n).map(|r| ((r.rect.x0 + r.rect.x1) / 2.0, (r.rect.y0 + r.rect.y1) / 2.0)) } else { num(arg).and_then(|n| self.pane().row_point(n)) };
                if let Some((x, y)) = at { self.pointer(&Event::Down { x, y, button: Button::Secondary }); self.pointer(&Event::Up { x, y, button: Button::Secondary }); } }
            // a middle click on the folder at a row, as the mouse would do it
            "middle" => if let Some(n) = num(arg) { if let Some((x, y)) = self.pane().row_point(n) { self.pointer(&Event::Down { x, y, button: Button::Middle }); } },
            "tip" => self.tip_demo = Some(arg.to_string()),
            "wait" => self.demo_until = Some(Instant::now() + Duration::from_millis(arg.parse().unwrap_or(0))),
            other => eprintln!("UFB_DEMO: unknown step {other:?}"),
        }
    }
    fn busy(&self) -> bool { self.panes.iter().take(self.shown()).any(|p| p.loading()) || self.check.is_some() || self.making || !self.share_busy.is_empty() || self.job.as_ref().is_some_and(|j| j.loading) }
    fn run_demo(&mut self) {
        while !self.demo.is_empty() {
            if self.busy() { return }
            if let Some(t) = self.demo_until { if Instant::now() < t { return } self.demo_until = None; }
            let step = self.demo.pop_front().unwrap(); self.demo_step(&step);
        }
    }

    // ---------------------------------------------------------------- drawing
    pub fn animating(&self) -> bool {
        let now = Instant::now(); let n = self.shown();
        self.tips.next_change(now).is_some() || self.prefs.as_ref().is_some_and(|p| p.animating(now)) || self.side_bar.animating(now) || self.panes.iter().take(n).any(|p| p.animating(now) || (p.load == Load::Loading)) || [&self.back, &self.forward, &self.up, &self.list_btn, &self.grid_btn, &self.pane2_btn, &self.tab_new].iter().any(|b| b.animating(now)) || self.tab_close.iter().any(|b| b.animating(now))
            || self.crumbs.iter().any(|(b, _)| b.animating(now)) || self.sections.iter().any(|sec| sec.animating(now)) || self.size.animating(now) || self.text_focus() || self.dialog.is_some() || self.making || self.job.as_ref().is_some_and(|j| j.loading) || self.items.animating(now) || !self.share_busy.is_empty() || self.share_add.animating(now) || self.share_btns.iter().any(|b| b.animating(now)) || self.board.as_ref().is_some_and(|b| b.animating(now)) || self.board_btn.animating(now) || self.tracker.as_ref().is_some_and(|t| t.list.animating(now)) || self.trk_btn.animating(now) || self.due_btns.iter().any(|b| b.animating(now)) || self.chip.is_some() || self.coledit.as_ref().is_some_and(|e| e.animating(now)) || self.job_meta.animating(now) || self.job_refresh.animating(now) || self.job_add.animating(now) || self.job_secs.iter().any(|b| b.animating(now)) || self.toast.is_shown() || self.rename_next.is_some() || self.path_error.is_some() || self.lightbox_live(now)
            // (a scripted look keeps the window drawing by itself - which HID that a playing clip did not: `UFB_DEMO_IDLE=1`
            // stops that once the lightbox is open, so that what is measured is what a person gets)
            || ((!self.demo.is_empty() || (self.shot.is_some() && !self.done)) && (std::env::var_os("UFB_DEMO_IDLE").is_none() || !self.lightbox.is_open()))
    }
    /// The lightbox has something moving: a picture on its way, a clip that plays, is sought or was just touched. The
    /// window then draws at every refresh. (Left out at first: a clip moved only when something else woke the window -
    /// the owner's walk, 2026-11-16: "playing one second at a time with freezes in between", "videos take forever to load".)
    fn lightbox_live(&self, now: Instant) -> bool { self.lightbox.is_open() && (self.lightbox.loading() || self.lightbox.view.as_ref().is_some_and(|v| v.player.as_ref().is_some_and(|p| p.busy(now)) || v.doc.as_ref().is_some_and(|d| d.borrow().busy(now)))) }
    /// The path bar's segments as buttons, made anew when the path, the startup disk's name or the room changes.
    fn make_crumbs(&mut self, room: f64, s: f64) {
        // (in a job the path begins at the chosen ITEM - the job and the section are named in the bar above, and no
        // segment leads out of the shot; with nothing chosen there is no path, row 486)
        let blank = self.blank(); let pane = &self.panes[self.active.min(self.panes.len() - 1)]; let top = pane.root.clone().filter(|_| self.job.is_some());
        let path = pane.path().to_path_buf(); let key = (path.clone(), format!("{}|{}|{blank}", self.root, top.as_ref().map(|t| t.to_string_lossy().into_owned()).unwrap_or_default()), (room * 4.0) as i64);
        if self.crumbs_for.as_ref() == Some(&key) { return } self.crumbs_for = Some(key);
        if blank { self.crumbs.clear(); return }
        let mut segs = crumbs::segments(&path, if self.root.is_empty() { "/" } else { &self.root });
        if let Some(top) = &top { segs.retain(|seg| seg.path.starts_with(top)); }
        let mut all: Vec<(ActionButton, Option<PathBuf>)> = segs.into_iter().map(|seg| { let mut b = ActionButton::new("", "").size_s(); b.label = Some(seg.label); (b, Some(seg.path)) }).collect();
        let widths: Vec<f64> = all.iter().map(|(b, _)| b.measure(&mut self.fonts, s).0).collect();
        let (sep, dots) = (14.0 * s, t::ACTION_BUTTON_HEIGHT_S as f64 * s);
        let keep = crumbs::elide(&widths, sep, dots, room);
        let mut out = vec![]; let mut taken: Vec<Option<(ActionButton, Option<PathBuf>)>> = all.drain(..).map(Some).collect();
        for k in keep { match k { Some(i) => if let Some(c) = taken[i].take() { out.push(c); }, None => out.push((ActionButton::new("dots-three", "").size_s().enabled(false), None)) } }
        self.crumbs = out;
    }
    pub fn redraw(&mut self) {
        // a view changed in a folder is that folder's from now on (row 526)
        for p in self.panes.iter_mut() { p.keep_view(); }
        let s = self.scale(); let size = self.window.surface_size(); let (w, h) = (size.width as f64, size.height as f64); if w < 1.0 || h < 1.0 { return }
        self.poll(); self.run_demo();
        if self.lightbox.is_open() { return self.redraw_lightbox() }
        let now = Instant::now(); let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        if self.active >= self.shown() { self.active = 0; }
        self.sync_columns();
        // the window is called by the folder it shows (the active pane's)
        { let title = match &self.job { None if self.tracker.is_some() => "All Tracked".to_string(), Some(j) if self.tracker.is_some() => format!("{} \u{2014} Tracker", j.name), Some(j) if j.chosen_item().is_none() => j.name.clone(), Some(j) => format!("{} \u{2014} {}", j.name, self.panes[self.active].title()), None => self.panes[self.active].title() }; if title != self.titled { self.window.set_title(&title); if !self.demo.is_empty() || self.shot.is_some() { eprintln!("UFB_DEMO: titled {title}"); } self.titled = title; } }
        // the search field shows the active pane's filter (a navigation clears it)
        if self.search.text() != self.panes[self.active].search { let text = self.panes[self.active].search.clone(); self.search.set_text(&text); }
        // -- where everything lies
        let st = self.menu_state();
        let menu_h = match self.menubar.as_mut() { Some(mb) => {
            mb.sync(st, &mut self.fonts); mb.bar.layout(&mut self.fonts, 0.0, 0.0, w, Rect::new(0.0, 0.0, w, h), s); mb.bar.rect.height() } None => 0.0 };
        let status = Rect::new(0.0, h - STATUS * s, w, h);
        let side_w = (self.side_w * s).min(w * 0.45).round(); self.side_rect = Rect::new(0.0, menu_h, side_w, status.y0);
        let area = Rect::new(side_w, menu_h, w, status.y0);
        // the strip of tabs, above the toolbar - only when there are two or more (row 480: nobody gets tabs who did not ask)
        let strip = Rect::new(area.x0, area.y0, area.x1, (area.y0 + if self.tabs.len() > 1 { TABS * s } else { 0.0 }).min(area.y1));
        // a job (row 482): its bar under the tabs - the name, a button a section -, and its item list left of the panes
        let in_job = self.job.is_some();
        let job_bar = Rect::new(area.x0, strip.y1, area.x1, (strip.y1 + if in_job { JOB_BAR * s } else { 0.0 }).min(area.y1));
        let tool = Rect::new(area.x0, job_bar.y1, area.x1, (job_bar.y1 + TOOLBAR * s).min(area.y1));
        // (the item list is wide - it carries the items' metadata - and its edge can be dragged, row 484)
        // the tracker takes the whole room under the bars (no panes), with a strip of its own above its list: its name, the Due chips
        let trk = self.tracker.is_some(); let trk_strip = if trk { 36.0 * s } else { 0.0 };
        let list_w = if trk { area.width() } else if in_job { (self.job_w * s).min((area.width() * 0.6).round()).max((JOB_LIST_MIN * s).min(area.width() * 0.6)) } else { 0.0 };
        self.job_list = Rect::new(area.x0, (tool.y1 + trk_strip).min(area.y1), area.x0 + list_w, area.y1); self.panes_x0 = area.x0 + list_w;
        let body = Rect::new(area.x0 + list_w, tool.y1, area.x1, area.y1);
        if let Some(j) = &self.job { if self.job_secs_for != j.sections { self.job_secs_for = j.sections.clone(); self.job_secs = j.sections.iter().map(|n| { let label = crate::jobview::type_for(&j.path).ty.section(n).map(|s| s.label().to_string()).unwrap_or_else(|| n.clone()); let mut b = ActionButton::new("", "").size_s(); b.label = Some(label); b }).collect(); } }
        let add_label = self.job_add_label();
        self.gaps[0] = Rect::new(side_w - GAP * s / 2.0, menu_h, side_w + GAP * s / 2.0, status.y0);
        self.gaps[2] = if in_job && !trk { Rect::new(body.x0 - GAP * s / 2.0, tool.y1, body.x0 + GAP * s / 2.0, area.y1) } else { Rect::ZERO };
        // in a job the second pane lies BELOW the first (row 484: the item list needs the width); in the browser, beside it
        // (in a job with nothing chosen no pane is drawn, row 486)
        let blank = self.blank(); let n = if blank { 0 } else { self.shown() }; self.stacked = in_job; self.panes_area = body;
        let split_x = if n == 2 && !in_job { (body.x0 + body.width() * self.split).round() } else { body.x1 };
        let split_y = if n == 2 && in_job { (body.y0 + body.height() * self.split).round() } else { body.y1 };
        self.gaps[1] = if n != 2 { Rect::ZERO } else if in_job { Rect::new(body.x0, split_y - GAP * s / 2.0, body.x1, split_y + GAP * s / 2.0) } else { Rect::new(split_x - GAP * s / 2.0, body.y0, split_x + GAP * s / 2.0, body.y1) };
        let head_h = if n == 2 { HEAD * s } else { 0.0 };
        for i in 0..n { let (x0, x1) = if n == 1 || in_job { (body.x0, body.x1) } else if i == 0 { (body.x0, split_x) } else { (split_x + s, body.x1) };
            let (y0, y1) = if n == 1 || !in_job { (body.y0, body.y1) } else if i == 0 { (body.y0, split_y) } else { (split_y + s, body.y1) };
            self.pane_heads[i] = Rect::new(x0, y0, x1, (y0 + head_h).min(y1)); let active = i == self.active; self.panes[i].layout(Rect::new(x0, (y0 + head_h).min(y1), x1, y1), s, active); }
        self.show_question();
        // a new folder goes straight into its name, as soon as its row is there
        if let Some(path) = self.rename_next.clone() { let i = self.active; if !self.panes[i].loading() { self.rename_next = None; if self.focus == Focus::Pane && self.dialog.is_none() && self.panes[i].name_rect(&path).is_some() { self.start_rename(); } } }
        // the field of a name being changed lies over the name (its entry gone: the rename is over)
        if let Some(r) = self.rename.as_mut() {
            match self.panes.get(r.pane).filter(|_| r.pane < n).and_then(|p| p.name_rect(&r.path).map(|q| (q, p.rect, p.mode))) {
                Some((q, pane, mode)) => { let fh = (t::FIELD_HEIGHT_M as f64 * s).min(30.0 * s); let cy = (q.y0 + q.y1) / 2.0;
                    let (x0, x1) = if mode == Mode::List { (q.x0 - 6.0 * s, (q.x0 + 420.0 * s).min(q.x1).max(q.x0 + 80.0 * s)) } else { (q.x0, q.x1) };
                    r.field.layout(&mut self.fonts, Rect::new(x0.max(pane.x0), cy - fh / 2.0, x1.min(pane.x1), cy + fh / 2.0), s); }
                None => { self.rename = None; if self.focus == Focus::Rename { self.focus = Focus::Pane; } }
            }
        }
        // the toolbar's controls, from both ends; the path bar takes what is left
        let bh = t::ACTION_BUTTON_HEIGHT_M as f64 * s; let by = tool.y0 + (tool.height() - bh) / 2.0; let pad = 8.0 * s; let radius = t::ACTION_BUTTON_CORNER_RADIUS_M as f64 * s;
        let grid_mode = self.panes[self.active].mode == Mode::Grid; let wide = tool.width() / s;
        let mut x = tool.x0 + pad;
        for b in [&mut self.back, &mut self.forward, &mut self.up] { b.rect = Rect::new(x, by, x + bh, by + bh); b.corners = Corners::all(radius); x += bh + 2.0 * s; }
        let mut r = tool.x1 - pad;
        self.pane2_btn.rect = Rect::new(r - bh, by, r, by + bh); self.pane2_btn.corners = Corners::all(radius); self.pane2_btn.selected = self.two;
        { let flat = self.job.as_ref().is_some_and(|j| !j.two_tier && !j.loading); self.pane2_btn.enabled = !flat; self.pane2_btn.tooltip = if flat { "This section has a single view".to_string() } else if in_job { "Renders pane".to_string() } else { "Second pane  (Tab switches panes)".to_string() }; } r -= bh + pad;
        let search_w = (if wide > 1000.0 { 200.0 } else if wide > 760.0 { 150.0 } else if wide > 560.0 { 110.0 } else { 0.0 }) * s; let fh = t::FIELD_HEIGHT_M as f64 * s;
        let search_rect = if search_w > 0.0 { let q = Rect::new(r - search_w, tool.y0 + (tool.height() - fh) / 2.0, r, tool.y0 + (tool.height() + fh) / 2.0); r -= search_w + pad; q } else { Rect::ZERO };
        self.search.layout(&mut self.fonts, search_rect, s);
        self.size.rect = if grid_mode && wide > 640.0 { let q = Rect::new(r - 100.0 * s, by, r, by + bh); r -= 100.0 * s + pad; q } else { Rect::ZERO };
        self.size.value = ((self.panes[self.active].grid.tile - grid::TILE_MIN) / (grid::TILE_MAX - grid::TILE_MIN)) as f32;
        // (list and grid are one control of two halves)
        self.grid_btn.rect = Rect::new(r - bh, by, r, by + bh); self.grid_btn.corners = Corners { tl: 0.0, bl: 0.0, tr: radius, br: radius }; self.grid_btn.selected = grid_mode; r -= bh + s;
        self.list_btn.rect = Rect::new(r - bh, by, r, by + bh); self.list_btn.corners = Corners { tl: radius, bl: radius, tr: 0.0, br: 0.0 }; self.list_btn.selected = !grid_mode; r -= bh + pad;
        // (the lightbox by the mouse, row 452)
        self.preview_btn.rect = if wide > 520.0 { let q = Rect::new(r - bh, by, r, by + bh); r -= bh + pad; q } else { Rect::ZERO }; self.preview_btn.corners = Corners::all(radius); self.preview_btn.enabled = self.panes[self.active].cursor_row().is_some();
        self.seq_btn.rect = if wide > 600.0 { let q = Rect::new(r - bh, by, r, by + bh); r -= bh + pad; q } else { Rect::ZERO }; self.seq_btn.corners = Corners::all(radius); self.seq_btn.selected = self.panes[self.active].model.sequences;
        self.path_rect = Rect::new((x + pad - 2.0 * s).min(r), tool.y0 + (tool.height() - fh) / 2.0, r.max(x + pad), tool.y0 + (tool.height() + fh) / 2.0);
        { let p = &self.panes[self.active]; let shown = !self.blank(); self.back.enabled = shown && p.history.can_back(); self.forward.enabled = shown && p.history.can_forward(); self.up.enabled = shown && p.can_up(); }
        let crumb_room = self.path_rect.width() - 12.0 * s; self.make_crumbs(crumb_room, s);
        if self.focus == Focus::Path { self.field.layout(&mut self.fonts, self.path_rect, s); }
        if self.path_error.as_ref().is_some_and(|(_, since)| since.elapsed() > Duration::from_millis(2600)) { self.path_error = None; self.field.error = false; }

        let places: Vec<(String, PathBuf)> = if self.tabs.len() > 1 { (0..self.tabs.len()).map(|i| self.tab_place(i)).collect() } else { vec![] };
        let scene: &mut Scene = self.layer.scene_mut(); scene.reset(); let fonts = &mut self.fonts;
        // The surfaces, as the family stacks them: the window is layer 1; the sidebar's sections are layer-2 tiles on it;
        // the path bar and the search are fields; the list lies on the window itself.
        paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);

        // -- the panes
        if blank && !trk { if let Some(j) = &self.job {
            let what = if j.missing || (j.loading && j.items.is_empty()) { String::new() } else if j.items.is_empty() { "Nothing in this section yet".to_string() } else if self.job_add.label.as_deref() == Some("Add Shot") { "Choose a shot to see its files".to_string() } else { "Choose an item to see its files".to_string() };
            let l = fonts.layout_fit(&what, Family::Sans, t::BODY_SIZE_M * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, (body.width() - 40.0 * s).max(10.0));
            draw_layout(scene, &l, Affine::translate((body.x0 + (body.width() - l.width() as f64) / 2.0, body.y0 + (body.height() * 0.38).min(160.0 * s))));
        } }
        self.thumbs.begin_frame();
        for i in 0..n {
            if n == 2 {
                let hr = self.pane_heads[i]; let active = i == self.active;
                paint::rect(scene, hr, if active { t::BACKGROUND_LAYER_2_COLOR } else { t::BACKGROUND_LAYER_1_COLOR });
                // the active pane's accent: the line the toolbar and the menus act on
                paint::rect(scene, Rect::new(hr.x0, hr.y0, hr.x1, hr.y0 + 2.0 * s), if active { t::FOCUS_INDICATOR_COLOR } else { t::DIVIDER_COLOR });
                let mut tx = hr.x0 + 10.0 * s;
                if let Some(g) = fonts.icon("folder-simple", 14.0 * s as f32, if active { ink } else { sub }) { draw_layout(scene, &g, Affine::translate((tx, hr.y0 + s + (hr.height() - g.height() as f64) / 2.0))); tx += g.width() as f64 + 6.0 * s; }
                let l = fonts.layout_fit(&match self.panes[i].hidden_sort() { Some(f) => format!("{}   \u{b7}   sorted by {f}", self.panes[i].title()), None => self.panes[i].title() }, Family::Sans, t::BODY_SIZE_S * s as f32, if active { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, if active { ink } else { sub }, hr.x1 - tx - 10.0 * s);
                draw_layout(scene, &l, Affine::translate((tx, hr.y0 + s + (hr.height() - l.height() as f64) / 2.0)));
            }
            self.panes[i].paint(scene, fonts, &mut self.icons, &mut self.thumbs, s, now);
        }
        // (a job's section of two tiers: the second pane can show the chosen item's proxies in place of its renders)
        self.job_prox.rect = Rect::ZERO;
        if let Some(j) = self.job.as_ref().filter(|j| n == 2 && j.two_tier && j.chosen_item().is_some_and(|i| i.proxies.is_some())) {
            let hr = self.pane_heads[1]; let sb = (t::ACTION_BUTTON_HEIGHT_S as f64 * s).min(hr.height() - 4.0 * s); let w = self.job_prox.measure(fonts, s).0;
            self.job_prox.rect = Rect::new(hr.x1 - 6.0 * s - w, hr.y0 + s + (hr.height() - sb) / 2.0, hr.x1 - 6.0 * s, hr.y0 + s + (hr.height() + sb) / 2.0); self.job_prox.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); self.job_prox.selected = j.proxies; self.job_prox.paint(scene, fonts, s, now);
        }
        self.thumbs.end_frame();
        if let Some(r) = self.rename.as_mut() { let clip = self.panes[r.pane.min(n.saturating_sub(1))].rect; scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &clip); r.field.paint(scene, fonts, s); scene.pop_layer(); }
        if n == 2 { let hot = self.over_gap == Some(1) || self.drag == Some(Drag::Split); let k = if hot { 2.0 * s } else { s }; paint::rect(scene, if in_job { Rect::new(body.x0, split_y, body.x1, split_y + k) } else { Rect::new(split_x, body.y0, split_x + k, body.y1) }, if hot { t::RAIL_SPLITTER_COLOR_HOVER } else { t::DIVIDER_COLOR_MEDIUM }); }

        // -- the strip of tabs (row 481): PILLS on the window's own surface, as the sidebar's rows are - the one in front
        // with the fill and the label of the sidebar row one is in, the others bare until the pointer is on them; no line,
        // no divider, no ground of the strip's own. Each as wide as the room gives (at most TAB_MAX); a cross in each, a plus after the last
        self.tab_rects.clear();
        if strip.height() > 0.0 {
            let count = self.tabs.len();
            if self.tab_close.len() != count { self.tab_close = (0..count).map(|_| ActionButton::new("x", &menu::tip("Close tab", Cmd::CloseTab)).size_s()).collect(); }
            let sb = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let (pad, gap, ph) = (8.0 * s, 4.0 * s, TAB_PILL * s); let plus_w = sb + gap + pad;
            let tw = ((strip.width() - pad - plus_w - gap * (count as f64 - 1.0)) / count as f64).min(TAB_MAX * s).max(1.0); let radius = t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s;
            let at = kurbo::Point::new(self.pointer.0, self.pointer.1); let py = strip.y0 + (strip.height() - ph) / 2.0 + s;
            for (i, (name, _)) in places.iter().enumerate() {
                let x0 = strip.x0 + pad + i as f64 * (tw + gap); let r = Rect::new(x0, py, x0 + tw, py + ph); let front = i == self.tab; let hot = r.contains(at) && self.drag.is_none();
                if front { paint::rounded(scene, r, radius, t::RAIL_ROW_ACTIVE_FILL); } else if hot { paint::rounded(scene, r, radius, t::HOVER_BACKGROUND_COLOR); }
                let color = paint::color(if front { t::RAIL_ROW_ACTIVE_LABEL } else { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT });
                // (the cross: always in the tab in front, in the others under the pointer; none where a tab is too narrow for it and a name)
                let b = &mut self.tab_close[i]; let room = tw > 64.0 * s; let bs = sb.min(ph - 4.0 * s);
                b.rect = if room && (front || hot) { Rect::new(r.x1 - 3.0 * s - bs, r.y0 + (ph - bs) / 2.0, r.x1 - 3.0 * s, r.y0 + (ph + bs) / 2.0) } else { Rect::ZERO }; b.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s);
                scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &r);
                let mut tx = r.x0 + 8.0 * s;
                if tw > 48.0 * s { if let Some(g) = fonts.icon("folder-simple", 14.0 * s as f32, color) { draw_layout(scene, &g, Affine::translate((tx, r.y0 + (ph - g.height() as f64) / 2.0))); tx += g.width() as f64 + 6.0 * s; } }
                let l = fonts.layout_fit(name, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, color, (r.x1 - tx - if room { bs + 7.0 * s } else { 6.0 * s }).max(4.0));
                draw_layout(scene, &l, Affine::translate((tx, r.y0 + (ph - l.height() as f64) / 2.0))); scene.pop_layer();
                if b.rect.width() > 0.0 { b.paint(scene, fonts, s, now); }
                self.tab_rects.push(r);
            }
            let px = strip.x0 + pad + count as f64 * (tw + gap); self.tab_new.rect = Rect::new(px, py + (ph - sb) / 2.0, px + sb, py + (ph + sb) / 2.0); self.tab_new.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); self.tab_new.paint(scene, fonts, s, now);
        } else { self.tab_new.rect = Rect::ZERO; }

        // -- a job: its bar, and its item list
        self.job_add.rect = Rect::ZERO; self.job_meta.rect = Rect::ZERO; self.board_btn.rect = Rect::ZERO;
        if let Some(j) = &self.job {
            let sb = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let radius = t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s; let cy = job_bar.y0 + (job_bar.height() - sb) / 2.0;
            paint::hline(scene, job_bar.x0, job_bar.x1, job_bar.y1 - s, s, t::DIVIDER_COLOR);
            let mut x = job_bar.x0 + 12.0 * s;
            if let Some(g) = fonts.icon("briefcase", 15.0 * s as f32, ink) { draw_layout(scene, &g, Affine::translate((x, job_bar.y0 + (job_bar.height() - g.height() as f64) / 2.0))); x += g.width() as f64 + 7.0 * s; }
            let l = fonts.layout_fit(&j.name, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_BOLD as f32, ink, (job_bar.width() * 0.3).max(60.0 * s));
            draw_layout(scene, &l, Affine::translate((x, job_bar.y0 + (job_bar.height() - l.height() as f64) / 2.0))); x += l.width() as f64 + 16.0 * s;
            self.job_refresh.rect = Rect::new(job_bar.x1 - 10.0 * s - sb, cy, job_bar.x1 - 10.0 * s, cy + sb); self.job_refresh.corners = Corners::all(radius); self.job_refresh.paint(scene, fonts, s, now);
            // the sections, as far as the room goes
            let end = self.job_refresh.rect.x0 - 10.0 * s;
            for (b, name) in self.job_secs.iter_mut().zip(self.job_secs_for.iter()) {
                let w = b.measure(fonts, s).0; if x + w > end { b.rect = Rect::ZERO; continue }
                b.rect = Rect::new(x, cy, x + w, cy + sb); b.corners = Corners::all(radius); b.selected = !trk && j.section.as_deref() == Some(name.as_str()); b.paint(scene, fonts, s, now); x += w + 4.0 * s;
            }
            // ... and, after them, the job's tracker
            { let w = self.trk_btn.measure(fonts, s).0; self.trk_btn.rect = if x + 10.0 * s + w <= end { Rect::new(x + 10.0 * s, cy, x + 10.0 * s + w, cy + sb) } else { Rect::ZERO }; self.trk_btn.corners = Corners::all(radius); self.trk_btn.selected = trk; if self.trk_btn.rect.width() > 0.0 { paint::rect(scene, Rect::new(x + 4.0 * s, cy + 4.0 * s, x + 4.0 * s + s, cy + sb - 4.0 * s), t::DIVIDER_COLOR); self.trk_btn.paint(scene, fonts, s, now); } }
            if !trk {
            // the item list (`itemlist.rs`): the star, the name, the metadata columns, the date; Add and Columns at its foot
            let lr = self.job_list; let foot = 40.0 * s;
            let title = if j.missing { "The job's folder is not there".to_string() } else if j.loading && j.items.is_empty() { "Items".to_string() } else { format!("Items ({})", j.items.len()) };
            if self.items.order.len() != j.items.len() { self.items.reorder(&j.items); }
            let lrect = Rect::new(lr.x0, lr.y0, lr.x1 - s, (lr.y1 - foot).max(lr.y0));
            match self.board.as_mut() {
                // (the board, in the list's place: the same items, as cards in lanes)
                Some(b) => { b.layout(lrect, s); b.paint(scene, fonts, &j.items, &self.items, j.chosen, matches!(self.focus, Focus::Items | Focus::Cell), now); }
                None => { self.items.layout(lrect, j.items.len(), s); self.items.paint(scene, fonts, &j.items, j.chosen, matches!(self.focus, Focus::Items | Focus::Cell), &title, if j.loading || j.missing { "" } else { "Nothing here yet" }, now); }
            }
            paint::hline(scene, lr.x0, lr.x1, lr.y1 - foot, s, t::DIVIDER_COLOR); { let hot = self.over_gap == Some(2) || matches!(self.drag, Some(Drag::JobList(..))); paint::rect(scene, Rect::new(lr.x1 - s, lr.y0, lr.x1 + if hot { s } else { 0.0 }, lr.y1), if hot { t::RAIL_SPLITTER_COLOR_HOVER } else { t::DIVIDER_COLOR }); }
            let mut fx = lr.x0 + 10.0 * s;
            if let Some(label) = add_label { self.job_add.label = Some(label.to_string()); let w = self.job_add.measure(fonts, s).0; self.job_add.rect = Rect::new(fx, lr.y1 - foot + (foot - sb) / 2.0, fx + w, lr.y1 - foot + (foot + sb) / 2.0); self.job_add.corners = Corners::all(radius); self.job_add.paint(scene, fonts, s, now); fx += w + 8.0 * s; }
            if !j.missing && j.section.is_some() { let w = self.job_meta.measure(fonts, s).0; self.job_meta.rect = Rect::new(fx, lr.y1 - foot + (foot - sb) / 2.0, fx + w, lr.y1 - foot + (foot + sb) / 2.0); self.job_meta.corners = Corners::all(radius); self.job_meta.paint(scene, fonts, s, now); fx += w + 8.0 * s; }
            if !j.missing && j.section.is_some() { let w = self.board_btn.measure(fonts, s).0; self.board_btn.rect = if fx + w <= lr.x1 - 6.0 * s { Rect::new(fx, lr.y1 - foot + (foot - sb) / 2.0, fx + w, lr.y1 - foot + (foot + sb) / 2.0) } else { Rect::ZERO }; self.board_btn.corners = Corners::all(radius); self.board_btn.selected = self.board.is_some(); if self.board_btn.rect.width() > 0.0 { self.board_btn.paint(scene, fonts, s, now); } }
            }
        } else { self.trk_btn.rect = Rect::ZERO; }
        // -- the tracker: its strip (its name where no job's bar says it, the Due chips when a date is among its columns), its list
        for b in self.due_btns.iter_mut() { b.rect = Rect::ZERO; }
        if let Some(tr) = self.tracker.as_mut() {
            let lr = self.job_list; let strip = Rect::new(lr.x0, lr.y0 - trk_strip, lr.x1, lr.y0); let sb = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let cy = strip.y0 + (strip.height() - sb) / 2.0; let mut x = strip.x0 + 12.0 * s;
            if tr.scope == crate::tracker::Scope::All { if let Some(g) = fonts.icon("list-checks", 15.0 * s as f32, ink) { draw_layout(scene, &g, Affine::translate((x, strip.y0 + (strip.height() - g.height() as f64) / 2.0))); x += g.width() as f64 + 7.0 * s; }
                let l = fonts.layout("All Tracked", Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_BOLD as f32, ink); draw_layout(scene, &l, Affine::translate((x, strip.y0 + (strip.height() - l.height() as f64) / 2.0))); x += l.width() as f64 + 20.0 * s; }
            if tr.dated { let l = fonts.layout("Due", Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub); draw_layout(scene, &l, Affine::translate((x, strip.y0 + (strip.height() - l.height() as f64) / 2.0))); x += l.width() as f64 + 8.0 * s;
                for (k, b) in self.due_btns.iter_mut().enumerate() { let w = b.measure(fonts, s).0; if x + w > strip.x1 - 8.0 * s { break } b.rect = Rect::new(x, cy, x + w, cy + sb); b.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); b.selected = crate::tracker::DUES[k].0 == tr.due; b.paint(scene, fonts, s, now); x += w + 4.0 * s; } }
            paint::hline(scene, strip.x0, strip.x1, strip.y1 - s, s, t::DIVIDER_COLOR);
            let n = tr.rows.len(); tr.list.layout(lr, n, s);
            tr.list.also = if tr.sel.len() > 1 { tr.rows.iter().enumerate().filter(|(_, r)| tr.sel.contains(&r.path)).map(|(i, _)| i).collect() } else { Default::default() };
            tr.list.paint(scene, fonts, &tr.rows, tr.chosen, matches!(self.focus, Focus::Items | Focus::Cell), &format!("Tracked ({n})"), if tr.due == crate::tracker::Due::All { "Nothing is tracked yet: press an item's star in a job" } else { "Nothing is due then" }, now);
        }

        // -- the toolbar
        paint::hline(scene, tool.x0, tool.x1, tool.y1 - s, s, t::DIVIDER_COLOR);
        for b in [&self.back, &self.forward, &self.up, &self.list_btn, &self.grid_btn, &self.pane2_btn] { b.paint(scene, fonts, s, now); }
        if self.preview_btn.rect.width() > 0.0 { self.preview_btn.paint(scene, fonts, s, now); }
        if self.seq_btn.rect.width() > 0.0 { self.seq_btn.paint(scene, fonts, s, now); }
        if self.size.rect.width() > 0.0 { self.size.paint(scene, s, now); }
        if search_w > 0.0 { self.search.paint(scene, fonts, s);
            if self.search.text().is_empty() { if let Some(g) = fonts.icon("magnifying-glass", 13.0 * s as f32, sub) { draw_layout(scene, &g, Affine::translate((search_rect.x1 - g.width() as f64 - 8.0 * s, search_rect.y0 + (search_rect.height() - g.height() as f64) / 2.0))); } } }
        if self.focus == Focus::Path {
            self.field.paint(scene, fonts, s);
            if let Some((why, _)) = &self.path_error { let l = fonts.layout(why, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEGATIVE_CONTENT_COLOR_DEFAULT));
                let lr = Rect::new(self.path_rect.x1 - l.width() as f64 - 20.0 * s, self.path_rect.y0 + 3.0 * s, self.path_rect.x1 - 3.0 * s, self.path_rect.y1 - 3.0 * s);
                paint::rounded(scene, lr, 4.0 * s, t::FIELD_BACKGROUND_COLOR); draw_layout(scene, &l, Affine::translate((lr.x0 + 8.0 * s, lr.y0 + (lr.height() - l.height() as f64) / 2.0))); }
        } else {
            let pr = self.path_rect; paint::rounded(scene, pr, t::FIELD_CORNER_RADIUS_M as f64 * s, t::FIELD_BACKGROUND_COLOR);
            let ch = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let cy = pr.y0 + (pr.height() - ch) / 2.0; let mut cx = pr.x0 + 4.0 * s; let last = self.crumbs.len().saturating_sub(1); let end = pr.x1 - 8.0 * s;
            scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &pr);
            for (i, (b, path)) in self.crumbs.iter_mut().enumerate() {
                let bw = if path.is_some() { b.measure(fonts, s).0 } else { ch }; let bw = bw.min((end - cx).max(ch));
                b.rect = Rect::new(cx, cy, cx + bw, cy + ch); b.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); b.paint(scene, fonts, s, now); cx += bw;
                if i < last { if let Some(g) = fonts.icon("caret-right", 10.0 * s as f32, sub) { draw_layout(scene, &g, Affine::translate((cx + (14.0 * s - g.width() as f64) / 2.0, pr.y0 + (pr.height() - g.height() as f64) / 2.0))); } cx += 14.0 * s; }
            }
            scene.pop_layer();
        }

        // -- the sidebar
        let sr = self.side_rect; self.side_rows.clear();
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &sr);
        let (pad_s, row_h, head_h2) = (8.0 * s, SIDE_ROW * s, Section::height(s)); let (x0, x1) = (sr.x0 + pad_s, sr.x1 - pad_s);
        // (the row one is in: the job the tab shows, or - in the browser - the place the active pane is in)
        let current = match &self.job { Some(j) => j.path.clone(), None => self.panes[self.active].path().to_path_buf() };
        // (the shares: a row each, with how it stands - drawn below, with its words and its action)
        let share_places: Vec<Place> = self.shares.iter().map(|sh| Place { label: sh.name.clone(), path: PathBuf::new(), icon: if sh.server.is_empty() { "folder-simple" } else { "hard-drives" } }).collect();
        if self.share_btns.len() != self.shares.len() { self.share_btns = self.shares.iter().map(|_| { let mut b = ActionButton::new("", "").size_s(); b.label = Some("Connect".into()); b }).collect(); }
        let job_places: Vec<Place> = self.jobs.iter().map(|j| Place { label: j.name.clone(), path: j.path.clone(), icon: "briefcase" }).collect();
        // each section: (its rows, the quiet line it shows when it has none)
        let lists: [(Vec<&Place>, &str); 4] = [(self.favorites.iter().collect(), "No favorites yet"), (share_places.iter().collect(), "No saved shares"), (job_places.iter().collect(), "No jobs"), (self.fixed.iter().chain(self.volumes.iter()).collect(), "")];
        // the row of the place the active pane is in: the longest of the places that is exactly it
        let mut y = sr.y0 + pad_s - self.side_bar.offset; let top = y;
        let all_front = self.tracker.as_ref().is_some_and(|t| t.scope == crate::tracker::Scope::All);
        let side_row = |scene: &mut Scene, fonts: &mut Fonts, rows: &mut Vec<SideRow>, y: f64, icon: &str, label: &str, path: Option<&Path>, enabled: bool, hover: Option<usize>, job: bool, all: bool, keep: f64| {
            let r = Rect::new(x0 + 4.0 * s, y, x1 - 4.0 * s, y + row_h); let here = if all { all_front } else { !all_front && job == in_job && path.is_some_and(|p| p == current) }; let idx = rows.len();
            if here { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::RAIL_ROW_ACTIVE_FILL); } else if enabled && hover == Some(idx) { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); }
            let color = paint::color(if !enabled { t::DISABLED_CONTENT_COLOR } else if here { t::RAIL_ROW_ACTIVE_LABEL } else { t::NEUTRAL_CONTENT_COLOR_DEFAULT });
            let mut tx = r.x0 + 8.0 * s;
            if let Some(g) = fonts.icon(icon, 15.0 * s as f32, if here || !enabled { color } else { paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT) }) { draw_layout(scene, &g, Affine::translate((tx, y + (row_h - g.height() as f64) / 2.0))); } tx += 22.0 * s;
            let l = fonts.layout_fit(label, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, color, (r.x1 - tx - 6.0 * s - keep).max(8.0 * s));
            draw_layout(scene, &l, Affine::translate((tx, y + (row_h - l.height() as f64) / 2.0)));
            rows.push(SideRow { rect: r, path: path.map(Path::to_path_buf), enabled, job, all, share: None });
        };
        for (i, (rows, empty)) in lists.iter().enumerate() {
            let open = self.sections[i].expanded; let body_h = if !open { 0.0 } else if rows.is_empty() { row_h } else { rows.len() as f64 * row_h } + if open { 6.0 * s } else { 0.0 };
            section::paint_tile(scene, Rect::new(x0, y, x1, y + head_h2 + body_h), s);
            self.sections[i].rect = Rect::new(x0, y, x1, y + head_h2); self.sections[i].paint(scene, fonts, s, now, i == 0);
            // (the shares' header carries the plus)
            if i == 1 { let sb = t::ACTION_BUTTON_HEIGHT_S as f64 * s; self.share_add.rect = Rect::new(x1 - 8.0 * s - sb, y + (head_h2 - sb) / 2.0, x1 - 8.0 * s, y + (head_h2 + sb) / 2.0); self.share_add.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); self.share_add.paint(scene, fonts, s, now); }
            y += head_h2;
            if open {
                if rows.is_empty() { let l = fonts.layout_fit(empty, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, x1 - x0 - 24.0 * s); draw_layout(scene, &l, Affine::translate((x0 + 12.0 * s, y + (row_h - l.height() as f64) / 2.0))); y += row_h; }
                if i == 1 { for (k, sh) in self.shares.iter().enumerate() {
                    // a share: where it is when it is there (a press goes there), how it stands in words when it is not, and what can be done about it
                    let st = self.share_states.get(k).cloned().unwrap_or(crate::shares::State::NotConnected); let root = st.root().map(Path::to_path_buf);
                    let (words, tone, action) = match &st { crate::shares::State::Connected(_) => (String::new(), t::POSITIVE_VISUAL_COLOR, None), crate::shares::State::Landed(_) => (st.words(), t::NOTICE_VISUAL_COLOR, None),
                        crate::shares::State::Connecting => (st.words(), t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT, None), crate::shares::State::NotConnected => (if sh.server.is_empty() { "not there".to_string() } else { String::new() }, t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT, (!sh.server.is_empty()).then_some("Connect")),
                        // (why it is not reachable is in the row's tooltip, and was said when the user asked)
                        crate::shares::State::SignIn => (String::new(), t::NEGATIVE_VISUAL_COLOR, Some("Sign In\u{2026}")), crate::shares::State::Unreachable(_) => (String::new(), t::NEGATIVE_VISUAL_COLOR, Some("Connect")) };
                    // (a stale placeholder holds its name: the one thing to do is to remove it - the words give way to the action)
                    let held = self.share_holders.contains_key(&sh.id) && !matches!(st, crate::shares::State::Connected(_) | crate::shares::State::Connecting); let (words, tone, action) = if held { (String::new(), t::NOTICE_VISUAL_COLOR, Some("Fix\u{2026}")) } else { (words, tone, action) };
                    // at the row's right end: the state's dot; before it the action, or the words - the name has what is left
                    let b = &mut self.share_btns[k]; b.rect = Rect::ZERO; let bw = match action { Some(label) => { b.label = Some(label.to_string()); b.measure(fonts, s).0 } None => 0.0 };
                    let wl = (!words.is_empty()).then(|| fonts.layout_fit(&words, Family::Sans, t::BODY_SIZE_S * 0.92 * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, 86.0 * s));
                    let keep = 16.0 * s + if bw > 0.0 { bw + 6.0 * s } else { 0.0 } + wl.as_ref().map(|l| l.width() as f64 + 6.0 * s).unwrap_or(0.0);
                    side_row(scene, fonts, &mut self.side_rows, y, if sh.server.is_empty() { "folder-simple" } else { "hard-drives" }, &sh.name, root.as_deref(), true, self.side_hover, false, false, keep);
                    let r = self.side_rows.last().map(|x| x.rect).unwrap_or(Rect::ZERO); if let Some(row) = self.side_rows.last_mut() { row.share = Some(k); }
                    let d = 7.0 * s; paint::rounded(scene, Rect::new(r.x1 - 10.0 * s - d, y + (row_h - d) / 2.0, r.x1 - 10.0 * s, y + (row_h + d) / 2.0), d / 2.0, tone); let right = r.x1 - 22.0 * s;
                    if bw > 0.0 { let sb = t::ACTION_BUTTON_HEIGHT_S as f64 * s; b.rect = Rect::new(right - bw, y + (row_h - sb) / 2.0, right, y + (row_h + sb) / 2.0); b.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); b.paint(scene, fonts, s, now); }
                    else if let Some(l) = wl { draw_layout(scene, &l, Affine::translate((right - l.width() as f64, y + (row_h - l.height() as f64) / 2.0))); }
                    y += row_h; } }
                else { for p in rows { side_row(scene, fonts, &mut self.side_rows, y, p.icon, &p.label, Some(&p.path), true, self.side_hover, i == 2, false, 0.0); y += row_h; } }
                y += 6.0 * s;
            }
            y += 8.0 * s;
            // All Tracked: a destination of its own between the jobs and the places (row 463) - not this slice's
            if i == 2 { side_row(scene, fonts, &mut self.side_rows, y, "list-checks", "All Tracked", None, true, self.side_hover, false, true, 0.0); y += row_h + 8.0 * s; }
        }
        // the Sync entry, at the foot (row 499): what sync is doing in a word or two; a press opens its settings
        { let words = Self::sync_words(); let wl = fonts.layout_fit(&words, Family::Sans, t::BODY_SIZE_S * 0.92 * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, 96.0 * s);
            side_row(scene, fonts, &mut self.side_rows, y, "arrows-clockwise", "Sync", None, true, self.side_hover, false, false, wl.width() as f64 + 12.0 * s);
            let r = self.side_rows.last().map(|x| x.rect).unwrap_or(Rect::ZERO); self.sync_rect = r; draw_layout(scene, &wl, Affine::translate((r.x1 - 10.0 * s - wl.width() as f64, y + (row_h - wl.height() as f64) / 2.0))); y += row_h + 8.0 * s; }
        scene.pop_layer();
        self.side_bar.set(sr, y - top + pad_s, s); self.side_bar.paint(scene, now);
        { let hot = self.over_gap == Some(0) || matches!(self.drag, Some(Drag::Sidebar(..))); paint::rect(scene, Rect::new(sr.x1 - s, sr.y0, sr.x1 + if hot { s } else { 0.0 }, sr.y1), if hot { t::RAIL_SPLITTER_COLOR_HOVER } else { t::DIVIDER_COLOR }); }

        // -- the status bar: the active pane's items and choice; the room left on its volume
        paint::rect(scene, status, t::BACKGROUND_LAYER_1_COLOR); paint::hline(scene, status.x0, status.x1, status.y0, s, t::DIVIDER_COLOR);
        { let p = &self.panes[self.active]; let size = t::BODY_SIZE_S * s as f32;
            let free = p.free.filter(|_| p.load != Load::Loading && !blank).map(|f| crate::format::free_text(f, crate::format::DECIMAL)).unwrap_or_default();
            let fl = fonts.layout(&free, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, sub); let mut fx = status.x1 - 12.0 * s - fl.width() as f64;
            // a running operation takes the right of the bar (row 468, point 7): what it does, how far it is, its cancel, how many wait
            self.op_rect = Rect::ZERO;
            if let Some(st) = crate::ops::status() {
                let waiting = crate::ops::waiting().len(); let frac = if st.bytes.1 > 0 { st.bytes.0 as f64 / st.bytes.1 as f64 } else if st.items.1 > 0 { st.items.0 as f64 / st.items.1 as f64 } else { 0.0 };
                let text = format!("{}{}", st.label, if waiting > 0 { format!("  \u{b7}  {waiting} waiting") } else { String::new() });
                let bh = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let x1 = status.x1 - 8.0 * s; let bar_w = 110.0 * s;
                self.op_cancel.rect = Rect::new(x1 - bh, status.y0 + (status.height() - bh) / 2.0, x1, status.y0 + (status.height() + bh) / 2.0); self.op_cancel.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s);
                let bar = Rect::new(x1 - bh - 8.0 * s - bar_w, status.y0 + (status.height() - min_ui::progress::height(s)) / 2.0, x1 - bh - 8.0 * s, status.y0 + (status.height() + min_ui::progress::height(s)) / 2.0);
                let l = fonts.layout_fit(&text, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, ink, (bar.x0 - 10.0 * s - (area.x0 + 160.0 * s)).max(60.0 * s)); let lx = bar.x0 - 10.0 * s - l.width() as f64;
                draw_layout(scene, &l, Affine::translate((lx, status.y0 + (status.height() - l.height() as f64) / 2.0)));
                min_ui::progress::bar(scene, bar, frac, min_ui::progress::Tone::Neutral); self.op_cancel.paint(scene, fonts, s, now);
                self.op_rect = Rect::new(lx - 6.0 * s, status.y0, status.x1, status.y1); fx = lx - 24.0 * s;
            } else {
                self.op_cancel.rect = Rect::ZERO;
                // nothing runs: what is left in the list is said beside the room on the volume, and opens the panel
                let (active, done, failed) = crate::tasks::counts();
                if active + done + failed > 0 {
                    let mut parts = vec![]; if active > 0 { parts.push(format!("{active} waiting")); } if done > 0 { parts.push(format!("{done} finished")); } if failed > 0 { parts.push(format!("{failed} failed")); }
                    let l = fonts.layout(&format!("Tasks: {}", parts.join("  \u{b7}  ")), Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, if failed > 0 { paint::color(t::NEGATIVE_CONTENT_COLOR_DEFAULT) } else { ink });
                    let lx = fx - 24.0 * s - l.width() as f64;
                    if fx > status.x0 + side_w { draw_layout(scene, &fl, Affine::translate((fx, status.y0 + (status.height() - fl.height() as f64) / 2.0))); }
                    if lx > status.x0 + side_w { draw_layout(scene, &l, Affine::translate((lx, status.y0 + (status.height() - l.height() as f64) / 2.0))); self.op_rect = Rect::new(lx - 6.0 * s, status.y0, lx + l.width() as f64 + 6.0 * s, status.y1); }
                    fx = lx;
                } else { self.op_list = false; if fx > status.x0 + side_w { draw_layout(scene, &fl, Affine::translate((fx, status.y0 + (status.height() - fl.height() as f64) / 2.0))); } }
            }
            // the tasks' panel (row 508): every task from the moment it is asked for until it is dismissed - the newest
            // twelve; a row's button stops what runs or waits and dismisses what is over
            self.task_card = Rect::ZERO; for (_, b) in self.task_btns.iter_mut() { b.rect = Rect::ZERO; } self.task_clear.rect = Rect::ZERO;
            if self.op_list {
                let all = crate::tasks::list(); self.task_off = self.task_off.min(all.len().saturating_sub(TASK_ROWS)); let last = all.len() - self.task_off; let first = last.saturating_sub(TASK_ROWS);
                let tasks: Vec<&crate::tasks::Task> = all[first..last].iter().collect();
                if tasks.is_empty() { self.op_list = false; } else {
                    let (running, st) = (crate::ops::running_task(), crate::ops::status());
                    let (rh, pw, foot, bh) = (44.0 * s, 460.0 * s, 40.0 * s, t::ACTION_BUTTON_HEIGHT_S as f64 * s);
                    let card = Rect::new(status.x1 - 8.0 * s - pw, status.y0 - 8.0 * s - tasks.len() as f64 * rh - foot - 8.0 * s, status.x1 - 8.0 * s, status.y0 - 8.0 * s); self.task_card = card;
                    paint::rounded(scene, card.inflate(s, s), t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s + s, t::DIVIDER_COLOR_MEDIUM); paint::rounded(scene, card, t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
                    self.task_btns.retain(|(id, _)| tasks.iter().any(|t| t.id == *id));
                    for (i, task) in tasks.iter().enumerate() {
                        let y0 = card.y0 + 8.0 * s + i as f64 * rh; let over = task.state.over();
                        if !self.task_btns.iter().any(|(id, _)| *id == task.id) { self.task_btns.push((task.id, ActionButton::new("x", "Stop").size_s())); }
                        let b = &mut self.task_btns.iter_mut().find(|(id, _)| *id == task.id).unwrap().1; b.tooltip = if over { "Dismiss".into() } else { "Stop".into() };
                        b.rect = Rect::new(card.x1 - 10.0 * s - bh, y0 + (rh - bh) / 2.0, card.x1 - 10.0 * s, y0 + (rh + bh) / 2.0); b.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); b.paint(scene, fonts, s, now);
                        let tw = pw - 24.0 * s - bh - 12.0 * s;
                        let l = fonts.layout_fit(&task.label, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, ink, tw); draw_layout(scene, &l, Affine::translate((card.x0 + 12.0 * s, y0 + 5.0 * s)));
                        use crate::tasks::State as TS;
                        let (frac, detail) = if running == Some(task.id) { match &st { Some(st) => (Some(if st.bytes.1 > 0 { st.bytes.0 as f64 / st.bytes.1 as f64 } else if st.items.1 > 0 { st.items.0 as f64 / st.items.1 as f64 } else { 0.0 }), st.current.clone()), None => (None, String::new()) } } else { (task.progress, task.detail.clone()) };
                        let (words, tone) = match &task.state { TS::Waiting => ("Waiting".to_string(), sub), TS::Running => (if detail.is_empty() { "Running".to_string() } else { detail }, sub), TS::Done => ("Done".to_string(), sub), TS::Stopped => ("Stopped".to_string(), sub), TS::Failed(e) => (e.clone(), paint::color(t::NEGATIVE_CONTENT_COLOR_DEFAULT)) };
                        let bar_w = if task.state == TS::Running && frac.is_some() { 110.0 * s } else { 0.0 };
                        let l2 = fonts.layout_fit(&words, Family::Sans, t::DETAIL_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, tone, tw - bar_w - if bar_w > 0.0 { 10.0 * s } else { 0.0 }); let y2 = y0 + 5.0 * s + l.height() as f64 + 2.0 * s;
                        draw_layout(scene, &l2, Affine::translate((card.x0 + 12.0 * s, y2)));
                        if let (true, Some(f)) = (bar_w > 0.0, frac) { let ph = min_ui::progress::height(s); let by = y2 + (l2.height() as f64 - ph) / 2.0; min_ui::progress::bar(scene, Rect::new(card.x0 + 12.0 * s + tw - bar_w, by, card.x0 + 12.0 * s + tw, by + ph), f, min_ui::progress::Tone::Neutral); }
                        if i > 0 { paint::hline(scene, card.x0 + 12.0 * s, card.x1 - 12.0 * s, y0, s, t::DIVIDER_COLOR); }
                    }
                    let fy = card.y1 - foot; paint::hline(scene, card.x0, card.x1, fy, s, t::DIVIDER_COLOR);
                    if all.len() > tasks.len() { let l = fonts.layout(&format!("{}\u{2013}{} of {}", first + 1, last, all.len()), Family::Sans, t::DETAIL_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub); draw_layout(scene, &l, Affine::translate((card.x0 + 12.0 * s, fy + (foot - l.height() as f64) / 2.0))); }
                    self.task_clear.enabled = all.iter().any(|t| t.state.over()); let (cw, ch) = self.task_clear.measure(fonts, s);
                    self.task_clear.rect = Rect::new(card.x1 - 10.0 * s - cw, fy + (foot - ch) / 2.0, card.x1 - 10.0 * s, fy + (foot + ch) / 2.0); self.task_clear.paint(scene, fonts, s, now);
                }
            }
            let mesh = crate::mesh::status().map(|m| { let who = if m.leading { if m.peers == 0 { "Sync: no other machine".to_string() } else { format!("Sync: leading, {} other{}", m.peers, if m.peers == 1 { "" } else { "s" }) } } else { match &m.leader { Some(l) => format!("Sync: with {l}"), None => "Sync: looking for the others".to_string() } };
                let who = if m.strangers > 0 { format!("{who}  \u{b7}  {} with another secret", m.strangers) } else { who };
                if m.pending > 0 { format!("{who}  \u{b7}  {} edit{} not yet synced", m.pending, if m.pending == 1 { "" } else { "s" }) } else { who } });
            let l = fonts.layout_fit(&if blank { mesh.clone().unwrap_or_default() } else { let base = match p.hidden_sort() { Some(f) => format!("{}  \u{b7}  sorted by {f}", p.status()), None => p.status() }; match &mesh { Some(m) => format!("{base}  \u{b7}  {m}"), None => base } }, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, sub, (fx - 24.0 * s - area.x0 - 12.0 * s).max(40.0 * s));
            draw_layout(scene, &l, Affine::translate((area.x0 + 12.0 * s, status.y0 + (status.height() - l.height() as f64) / 2.0))); }

        // -- the drawn menu bar's strip, the tooltips, and the open menus over everything
        if let Some(mb) = self.menubar.as_ref() { mb.bar.paint(scene, fonts, s); }
        let pt = kurbo::Point::new(self.pointer.0, self.pointer.1); let menu_open = self.menubar.as_ref().is_some_and(|m| m.bar.is_active());
        let mut over: Option<(u64, Rect, String)> = None;
        if !menu_open && self.drag.is_none() {
            for (k, b) in [&self.back, &self.forward, &self.up, &self.list_btn, &self.grid_btn, &self.pane2_btn].iter().enumerate() { if b.hovered() || self.tip_demo.as_deref() == Some(["back", "forward", "up", "list", "grid", "pane2"][k]) { over = Some((k as u64 + 1, b.rect, b.tooltip.clone())); } }
            if self.size.rect.width() > 0.0 && (self.size.hovered() || self.size.dragging()) { over = Some((10, self.size.rect, format!("Icon size  {} px", self.panes[self.active].grid.tile.round()))); }
            if self.search.rect.contains(pt) && self.focus != Focus::Search { over = Some((11, self.search.rect, menu::tip("Search this folder", Cmd::Find))); }
            if self.focus != Focus::Path && self.path_rect.contains(pt) && !self.crumbs.iter().any(|(b, _)| b.hovered()) { over = Some((12, self.path_rect, menu::tip("Type a path", Cmd::GoToFolder))); }
            // a tab says the folder it shows; its cross and the plus say their keys
            for i in 0..self.tab_rects.len() { let r = self.tab_rects[i]; if r.contains(pt) { let b = &self.tab_close[i]; over = Some(if b.rect.contains(pt) { (3000 + i as u64, b.rect, b.tooltip.clone()) } else { (2000 + i as u64, r, places.get(i).map(|p| p.1.to_string_lossy().into_owned()).unwrap_or_default()) }); } }
            if self.tab_new.rect.contains(pt) { over = Some((1999, self.tab_new.rect, self.tab_new.tooltip.clone())); }
            // a sidebar row whose name is cut, and a place's full path
            if let Some(r) = self.side_hover.and_then(|i| self.side_rows.get(i)) { if let Some(p) = &r.path { over = Some((1000 + self.side_hover.unwrap_or(0) as u64, r.rect, p.to_string_lossy().into_owned())); } else if r.all { over = Some((999, r.rect, "Every tracked item of the subscribed jobs".into())); }
                // a share says what it is: its address, and what a request to connect it said
                if let Some(sh) = r.share.and_then(|k| self.shares.get(k)) { let st = r.share.and_then(|k| self.share_states.get(k)); let what = if sh.server.is_empty() { sh.local.clone() } else { format!("smb://{}/{}", sh.server, sh.share) };
                    over = Some((1500 + r.share.unwrap_or(0) as u64, r.rect, match st { Some(crate::shares::State::Unreachable(why)) => format!("{what}\n{why}"), Some(crate::shares::State::Landed(p)) => format!("{what}\nIt is at {} here - under another name than its own. It works as it is.{}", p.display(), if self.share_holders.contains_key(&sh.id) { " An empty folder left behind holds its name: Fix removes it (the system asks for an administrator's password)." } else { "" }), Some(s) => match s.root() { Some(p) => format!("{what}\n{}", p.display()), None => what }, None => what })); } }
        }
        let shown_now = if self.tip_demo.is_some() { now - Duration::from_millis(t::TOOLTIP_DELAY as u64 + 50).min(now.duration_since(self.born)) } else { now };
        self.tips.hover(over.as_ref().map(|(k, r, text)| (*k, *r, text.as_str())), shown_now); self.tips.tick(now); self.tips.paint(scene, fonts, s, Rect::new(0.0, 0.0, w, h), now);
        if let Some(mb) = self.menubar.as_ref() { mb.bar.paint_menus(scene, fonts, s); }
        // drags of files: where a drop can go, for the layer to answer from (folders in view first, then the panes, the
        // sidebar's rows, the path's segments); and the place that would take the drag in hand, outlined
        { let mut zones: Vec<crate::dnd::Zone> = vec![];
            for p in self.panes.iter().take(n) { zones.extend(p.folder_zones().into_iter().map(|(rect, folder)| crate::dnd::Zone { rect, folder, favorite: false })); }
            for r in &self.side_rows { if let (Some(path), true) = (&r.path, r.enabled) { let rect = r.rect.intersect(self.side_rect); if rect.width() > 0.0 && rect.height() > 0.0 { zones.push(crate::dnd::Zone { rect, folder: path.clone(), favorite: false }); } } }
            // the Favorites - its heading and whatever of the section holds no row: a folder let go there becomes a favourite
            // (row 533; a favourite's own row came before this, and a drop ON it still goes INTO that folder)
            { let (a, b) = (self.sections[0].rect, self.sections[1].rect); let rect = Rect::new(a.x0, a.y0, a.x1, if b.y0 > a.y1 { b.y0 } else { a.y1 }).intersect(self.side_rect); if rect.width() > 0.0 && rect.height() > 0.0 { zones.push(crate::dnd::Zone { rect, folder: PathBuf::new(), favorite: true }); } }
            if self.focus != Focus::Path { for (b, path) in &self.crumbs { if let Some(path) = path { zones.push(crate::dnd::Zone { rect: b.rect, folder: path.clone(), favorite: false }); } } }
            for p in self.panes.iter().take(n) { zones.push(crate::dnd::Zone { rect: p.rect, folder: p.path().to_path_buf(), favorite: false }); }
            let mut d = self.dnd.borrow_mut(); d.zones = zones;
            if let Some(r) = d.hover { let (k, c) = (2.0 * s, t::FOCUS_INDICATOR_COLOR); for q in [Rect::new(r.x0, r.y0, r.x1, r.y0 + k), Rect::new(r.x0, r.y1 - k, r.x1, r.y1), Rect::new(r.x0, r.y0, r.x0 + k, r.y1), Rect::new(r.x1 - k, r.y0, r.x1, r.y1)] { paint::rect(scene, q, c); } } }
        self.ctx.paint(scene, fonts, s);
        if let Some(p) = self.chip.as_mut() { p.paint(scene, fonts, s); }
        // the small editor of a notes or links cell: a card by the cell, kept inside the window
        if let Some(n) = self.note.as_mut() {
            let (cw, ch) = ((420.0 * s).min(w - 24.0 * s), (220.0 * s).min(h - 24.0 * s)); let x0 = (n.at.0 - 24.0 * s).clamp(12.0 * s, (w - cw - 12.0 * s).max(12.0 * s)); let y0 = if n.at.1 + 16.0 * s + ch < h - 12.0 * s { n.at.1 + 16.0 * s } else { (n.at.1 - 16.0 * s - ch).max(12.0 * s) };
            let card = Rect::new(x0, y0, x0 + cw, y0 + ch); n.card = card; let rad = t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s;
            for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), paint::color(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
            paint::rounded(scene, card.inflate(s, s), rad + s, t::DIVIDER_COLOR_MEDIUM); paint::rounded(scene, card, rad, t::BACKGROUND_LAYER_2_COLOR);
            let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)); let pad = 12.0 * s;
            let name = n.item.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let tl = fonts.layout_fit(&format!("{}  \u{b7}  {name}", n.title), Family::Sans, t::DETAIL_SIZE_M * s as f32, t::FONT_WEIGHT_BOLD as f32, ink, cw - 2.0 * pad); draw_layout(scene, &tl, Affine::translate((card.x0 + pad, card.y0 + pad)));
            let hint = if cfg!(target_os = "macos") { "\u{2318}Return saves  \u{b7}  Esc leaves it as it was" } else { "Ctrl+Enter saves  \u{b7}  Esc leaves it as it was" };
            let hl = fonts.layout_fit(hint, Family::Sans, t::DETAIL_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, cw - 2.0 * pad); draw_layout(scene, &hl, Affine::translate((card.x0 + pad, card.y1 - pad - hl.height() as f64)));
            n.area.layout(fonts, Rect::new(card.x0 + pad, card.y0 + pad + tl.height() as f64 + 8.0 * s, card.x1 - pad, card.y1 - pad - hl.height() as f64 - 8.0 * s), s); n.area.paint(scene, fonts, s);
        }
        if let Some(ed) = self.coledit.as_mut() { let bounds = Rect::new(0.0, 0.0, w, h); ed.layout(fonts, bounds, s); ed.paint(scene, fonts, bounds, now); }
        if let Some(p) = self.prefs.as_mut() { p.paint(scene, fonts, Rect::new(0.0, 0.0, w, h), s, now); }
        // what went wrong, at the window's foot; an open question over everything
        self.toast.tick(now); self.toast.paint(scene, fonts, Rect::new(0.0, 0.0, w, status.y0), s, now);
        if let Some((d, _)) = self.dialog.as_mut() { let bounds = Rect::new(0.0, 0.0, w, h); d.layout(fonts, bounds, s); d.paint(scene, fonts, s, bounds, now); }

        self.present(now);
    }
    /// The frame is rendered and shown; and the scripted picture is saved when everything has settled.
    fn present(&mut self, now: Instant) { self.present_layer(now, true) }
    /// `render`: the layer was drawn anew and is rendered; else the one rendered last is shown again (only the picture
    /// under it moved on - QCView's rule: the interface is rendered when it changed, not at every refresh of the screen).
    fn present_layer(&mut self, now: Instant, render: bool) {
        if render { self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue); }
        let window = self.window.clone(); let drawn = self.presenter.draw(|| window.pre_present_notify());
        // a window that is covered or minimised is given no frame, and the call returns at once: without a pause the loop
        // would turn as fast as the machine can while something plays (measured: 3,000 turns a second)
        if !drawn { std::thread::sleep(Duration::from_millis(8)); }

        // (the scripted picture: UFB_SHOT=<file.png> saves the window as drawn - no OS screenshot - once the steps are done,
        // the panes have their listings and the icons have come; then the application ends)
        if self.shot.is_some() && !self.done {
            let idle = self.demo.is_empty() && self.demo_until.is_none_or(|t| now >= t) && !self.busy() && self.icons.pending() == 0 && self.thumbs.pending() == 0 && !self.lightbox.loading() && self.lightbox.view.as_ref().and_then(|v| v.player.as_ref()).is_none_or(|p| p.shown || p.audio_only);
            if !idle { self.settled = None; } else if self.settled.is_none() { self.settled = Some(now); }
            if self.settled.is_some_and(|t0| now.duration_since(t0) > Duration::from_millis(500)) || self.born.elapsed() > Duration::from_secs(60) {
                if let (Some(path), Some((px, w, h))) = (self.shot.take(), self.presenter.capture_window()) {
                    let oetf = |v: f32| { let v = v.clamp(0.0, 1.0); ((if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0 + 0.5) as u8 };
                    let bytes: Vec<u8> = (0..(w * h) as usize).flat_map(|i| [oetf(px[i * 4]), oetf(px[i * 4 + 1]), oetf(px[i * 4 + 2])]).collect();
                    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
                    match std::fs::File::create(&path) { Ok(f) => { let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h); e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight); let _ = e.write_header().and_then(|mut wr| wr.write_image_data(&bytes)); } Err(e) => eprintln!("UFB_SHOT: {e}") }
                }
                self.done = true;
            }
        }
    }
}
