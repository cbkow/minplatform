//! minNotes on the platform — step 2 (canon rows 286, 287): ONE note, laid out by `mn-layout`, drawn into the UI layer
//! over QCView's presenter (the window surface the embedded player will draw its HDR picture into), edited through
//! `mn-edit`: click, drag and double-click select; the arrows (Alt = by word, Cmd / Ctrl = line and note ends, Shift
//! extends), typing, Enter, Backspace / Delete, undo / redo, select all, and plain-text cut / copy / paste;
//! formatting by command (Cmd / Ctrl+B I U E, Shift+X strike, Shift+H highlight, \\ clear) and by Markdown as it is
//! typed (row 289); Tab / Shift+Tab nest list items. Nothing is
//! saved; input methods and the desktop text element come next.
//!   minNotes <file.mnd>              a collection (rows 271, 272): opened through a private staged copy; its first note
//!                                    (or `--note <title>`) is edited. Edits are autosaved to the staging only; Cmd / Ctrl+S
//!                                    publishes — merging first if someone else saved meanwhile. Work a crash left behind
//!                                    is taken up again. `--new` starts a new collection at a path where nothing is yet
//!                                    (a missing file is otherwise an error: on a share it may be a missing mount).
//!   minNotes                         a built-in sample note (synthetic), not saved anywhere
//!   minNotes --v2 <file.mnd> [n]     page n (default 1) of a minNotes 2.0 file, converted in memory (the file is only read)
//!   minNotes --text <file.txt>       a plain text file, one paragraph per line
//!   --seconds N                      close after N seconds;   --shot <png>   write the window as drawn, then close
use winit31 as winit;
use std::{sync::Arc, time::Instant};
use kurbo::Rect;
use min_present::{Output, Presenter};
use min_ui::{fonts::Fonts, UiLayer};
use mn_doc::{BlockId, ChoiceOption, ColumnKind, ColumnSpec, Doc, Kind, Mark, Spans};
use mn_layout::{NoteLayout, Style};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ButtonSource, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key, NamedKey};
use mn_edit::{Dirty, Editor, Move};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, ControlFlow, DndAction, EventLoop};
use winit::data_transfer::TypeHint;
use winit::window::{Window, WindowAttributes, WindowId};

mod menu;
mod paint;
mod sidebar;
mod toolbar;
mod footer;
mod colours;
mod clip;
mod media;
mod psd;
mod paste;
mod import;
mod import_office;
mod xml;
mod pdf;
mod video;
mod choice;
mod linkedit;
mod findbar;
mod export;
mod gather;
mod gather_win;
mod search;
mod canvas;
mod ink;
mod open;
#[cfg(target_os = "macos")] mod imageio;
#[cfg(target_os = "macos")]
mod text_element;

/// The page's side margin and top / bottom padding, logical px (the Qt app: 48 and 0 — `docTopPad` 28 is defined there
/// but unused; 28 is used here so the first line does not touch the title bar).
const MARGIN: f64 = 48.0;
const PAD: f64 = 28.0;

struct Gpu { window: Arc<Box<dyn Window>>, presenter: Presenter, ui: UiLayer, fonts: Fonts, ime_area: Option<Rect>, /// text input is on (it is turned off while a video block is the current one: a held key shuttles, it must not offer accents)
    ime_on: bool }

/// Input methods on (accents by long press, dead keys, Chinese / Japanese / Korean input), with the caret's place for
/// the candidate window (winit 0.31's request API).
fn ime(window: &dyn Window, area: Option<Rect>, enable: bool) {
    use winit::window::{ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData};
    use winit::dpi::{PhysicalPosition, PhysicalSize};
    let mut data = ImeRequestData::default();
    let area = area.or(enable.then_some(Rect::new(0.0, 0.0, 1.0, 1.0)));
    if let Some(r) = area { data = data.with_cursor_area(PhysicalPosition::new(r.x0, r.y0).into(), PhysicalSize::new(r.width().max(1.0), r.height().max(1.0)).into()); }
    let req = if enable { match ImeEnableRequest::new(ImeCapabilities::new().with_cursor_area(), data) { Some(r) => ImeRequest::Enable(r), None => { log::warn!("ime: the enable request was not accepted"); return } } } else { ImeRequest::Update(data) };
    if let Err(e) = window.request_ime_update(req) { log::debug!("ime: {e}"); }
}
/// The collection a note came from: its staged session, the database on the work copy, and what is not yet written.
struct Store { session: Option<mn_store::staging::Session>, col: mn_store::Collection, note: BlockId, title: String, edited: Option<Instant>, unpublished: bool, /// an edit went to the staged copy after the file being saved was made
    edited_since_stage: bool }
/// The handle's place for an editor and its layout (`off`: the page's origin in the window).
/// What stands around the table the caret is in (row 317, ruling 6: only that table has them, and nothing comes or goes
/// with the pointer): a handle beside the caret's row and one above its column (their menus), a strip under the table
/// and one at its right (a row, a column, at the end), and the columns' right borders (dragged to set a width).
/// Each wide table's bar is placed over the room under its rows and given the table's scroll.
/// A table whose end is below what is seen (`seen`: the note's part of the window) has its bar at the seen part's
/// bottom instead, over its rows - a long table is scrolled sideways without going to its end first.
fn sync_bars(bars: &mut std::collections::HashMap<BlockId, min_ui::ScrollBar>, l: &NoteLayout, doc: &Doc, off: kurbo::Vec2, s: f64, seen: Rect) {
    let wide = l.wide_tables(); bars.retain(|id, b| wide.contains(id) || b.dragging());
    for id in wide { let (Some(r), Some((full, _, at))) = (l.rect(doc, id), l.table_scroll(id)) else { continue };
        let mut r = r + off; if r.y1 > seen.y1 && r.y0 + 3.0 * mn_layout::TABLE_BAR as f64 * s < seen.y1 { r.y1 = seen.y1; }
        let b = bars.entry(id).or_insert_with(|| { let mut b = min_ui::ScrollBar::horizontal(); b.always = true; b }); b.set(r, full as f64, s); b.offset = at as f64; }
}
/// The space between a note card's edge and its text (the canvas's units).
const CARD_PAD: f64 = 16.0;
/// A note card that is entered: the element, its note, its box in the window (kept up with the canvas each frame).
struct Card { el: BlockId, #[allow(dead_code)] note: BlockId, rect: Rect }
/// A picture of a note card's content at 100 % (drawn again only when the note or the card's width changes): the canvas
/// puts it on the sheet at any zoom.
struct CardPic { scene: vello::Scene, key: (u64, usize, i64, i64), h: f64, again: bool }
/// Where the note's text starts in the window: the page's place, or the entered card's.
fn origin(card: &Option<Card>, side_w: f64, top: f64, scroll: f64, s: f64) -> kurbo::Vec2 { match card { Some(c) => kurbo::Vec2::new(c.rect.x0 + CARD_PAD * s, c.rect.y0 + CARD_PAD * s - scroll), None => kurbo::Vec2::new(side_w + MARGIN * s, top + PAD * s - scroll) } }
/// A note card's content is drawn into its box `r` (`k`: a unit's size on the screen). Returns how tall the content is (units) and whether to draw again.
#[allow(clippy::too_many_arguments)]
fn card_pic(cards: &mut std::collections::HashMap<BlockId, CardPic>, ed: &Editor, store: Option<&Store>, others: &std::collections::HashMap<BlockId, Editor>, scene: &mut vello::Scene, fonts: &mut min_ui::Fonts, media: &mut media::Media, e: &mn_store::Element, r: Rect, k: f64, s: f64, far: bool) -> (f64, bool) {
    let (Some(note), Some(st)) = (BlockId::parse(&e.record), store) else { return (0.0, false) };
    let words = |scene: &mut vello::Scene, fonts: &mut min_ui::Fonts, t: &str| { let l = fonts.layout_fit(t, min_ui::Family::Sans, (13.0 * k).clamp(6.0, 40.0) as f32, 400.0, paint::subdued(), (r.width() - 20.0 * k).max(8.0)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0 + 10.0 * k, r.y0 + 8.0 * k))); };
    let (mem, stamp) = if st.note == note { (Some(&ed.doc), ed.stamp()) } else if let Some(o) = others.get(&note) { (Some(&o.doc), o.stamp()) } else { (None, (0, 0)) };
    let key = (stamp.0, stamp.1, (e.w * 100.0) as i64, (s * 1000.0) as i64);
    if cards.get(&note).is_none_or(|p| p.key != key || p.again) && !far {
        let loaded; let doc = match mem { Some(d) => d, None => { if st.col.entry(note).ok().flatten().is_none() { words(scene, fonts, "Note deleted"); return (64.0, false) } match st.col.load_note(note) { Ok(d) => { loaded = d; &loaded } Err(_) => return (64.0, false) } } };
        let layout = NoteLayout::build_media(doc, fonts, &style(), (((e.w - 2.0 * CARD_PAD) * s).max(60.0)) as f32, s as f32, Default::default(), Default::default());
        let mut sub = vello::Scene::new(); let again = paint::note(&mut sub, doc, &layout, fonts, media, &mut vec![], &mut vec![], None, None, false, CARD_PAD * s, CARD_PAD * s, Rect::new(0.0, 0.0, e.w * s, e.h * s));
        cards.insert(note, CardPic { scene: sub, key, h: layout.height as f64 / s + 2.0 * CARD_PAD, again });
    }
    let Some(p) = cards.get(&note) else { return (0.0, false) }; if far { return (p.h, false) }
    scene.push_clip_layer(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, &r); scene.append(&p.scene, Some(kurbo::Affine::translate((r.x0, r.y0)) * kurbo::Affine::scale(k / s))); scene.pop_layer();
    (p.h, p.again)
}
struct TableUi { table: BlockId, row: Rect, col: Rect, add_row: Rect, add_col: Rect, borders: Vec<(usize, f64)>, y: (f64, f64) }
fn table_ui(ed: &Editor, l: &NoteLayout, s: f64, off: kurbo::Vec2) -> Option<TableUi> {
    let (table, r, c) = ed.table_pos()?; let rows = ed.doc.children(Some(table)); let cells = ed.doc.children(Some(*rows.get(r)?));
    let (t, rr, cr) = (l.rect(&ed.doc, table)? + off, l.rect(&ed.doc, rows[r])? + off, l.rect(&ed.doc, *cells.get(c)?)? + off);
    let h = 18.0 * s;
    // in a table scrolled sideways the column's handle stands over the part of the column that shows, and only the borders in view are taken
    let cx = ((cr.x0.max(t.x0) + cr.x1.min(t.x1)) / 2.0).clamp(t.x0 + 12.0 * s, (t.x1 - 12.0 * s).max(t.x0 + 12.0 * s));
    Some(TableUi { table, row: Rect::new(t.x0 - h - 3.0 * s, rr.center().y - h / 2.0, t.x0 - 3.0 * s, rr.center().y + h / 2.0), // the column's handle sits ON the table's top edge (above it the block before would be in its way)
        col: Rect::new(cx - 12.0 * s, t.y0 - 7.0 * s, cx + 12.0 * s, t.y0 + 7.0 * s),
        add_row: Rect::new(t.x0, t.y1 + 3.0 * s, t.x1, t.y1 + 15.0 * s), add_col: Rect::new(t.x1 + 3.0 * s, t.y0, t.x1 + 15.0 * s, t.y1),
        borders: cells.iter().enumerate().filter_map(|(i, cell)| l.rect(&ed.doc, *cell).map(|q| (i, q.x1 + off.x))).filter(|(_, x)| *x >= t.x0 - 1.0 && *x <= t.x1 + 1.0).collect(), y: (t.y0, t.y1) })
}
fn handle_rect_in(ed: &Editor, l: &NoteLayout, s: f64, off: kurbo::Vec2) -> Option<Rect> {
    // in a table's cell the row's and the column's handles stand instead (blocks are still picked with Escape)
    if !ed.in_blocks() && ed.table_pos().is_some() { return None }
    let id = ed.handle_block()?; let r = l.rect(&ed.doc, id)?;
    let line = if ed.doc.get(id).is_some_and(|b| b.kind.is_container()) { None } else { l.caret(&ed.doc, id, 0, 1.0) }.unwrap_or(Rect::new(r.x0, r.y0, r.x0, r.y0 + 24.0 * s));
    let (cy, sz) = (line.center().y, 20.0 * s);
    // at the page's left edge the handle stands well clear of the text: a writing tool draws its own marks close beside a
    // line (the user 2026-11-11); inside a column there is only the gutter, so it stays close there
    let gap = if r.x0 < 1.0 { 15.0 * s } else { 2.0 * s };
    Some(Rect::new(r.x0 - gap - sz, cy - sz / 2.0, r.x0 - gap, cy + sz / 2.0) + off)
}
/// Files coming in from outside (row 310): a drag over the page, drops being fetched and looked at, a picture being
/// pointed at its file again; and the pictures found to be stored on their side.
#[derive(Clone, Copy, PartialEq)]
enum Paste { Rich, Plain, Markdown }
/// A picture being resized by a corner (row 312): its record before, where the pointer took hold, and the width then.
struct Resize { id: BlockId, before: String, from_x: f64, width: f64, max: f64, left: bool }
/// The ONE video that plays (rows 287, 316): its block, the family's player open on its file, whether its line is being
/// dragged, whether a frame of it has reached the presenter (until then its poster stays), and until when frames are
/// drawn although it does not play (a seek's picture arrives a little after it was asked).
struct Player { id: BlockId, t: qcv_player::Transport, scrub: bool, shown: bool, awake: Instant, /// the clip's notes as they are drawn on and written (QCView's own code, `qcv-annotate`; row 316, stage 4b)
    ann: qcv_annotate::Annotations, path: std::path::PathBuf }
/// Text selected on a PDF's page (row 315): the block, and the first and last characters (places in the page's list).
/// A file being read into a note (row 335): the reader's answer, and the file.
struct Importing { rx: std::sync::mpsc::Receiver<Result<import::Done, String>>, path: std::path::PathBuf }
/// A package file being unpacked (row 334): the thread's word (the size, then how it ended), the bytes written so far.
struct Unpack { rx: std::sync::mpsc::Receiver<(u64, Option<Result<std::path::PathBuf, String>>)>, done: Arc<std::sync::atomic::AtomicU64>, stop: Arc<std::sync::atomic::AtomicBool>, total: u64, name: String, drawn: Instant }
struct PdfSel { id: BlockId, a: usize, b: usize, drag: bool }
#[derive(Default)]
struct Files { /// the camera when Space went down over a canvas (unchanged at its release: a tap - the selected video plays or pauses)
    space_cam: Option<(f64, f64, f64)>,
    /// ink on a picture or a PDF's page in a note (row 325): the armed tool, the gesture under way, the stroke the pointer picked (its block, its id),
    /// and - worked out between events - the current block that can be drawn on (its frame in the window, its record) and what the toolbar shows of it
    cv_test: bool, /// Collect Media or the package: its window (row 332), and the scripted check of it has been started
    gather: Option<gather_win::Window>, gather_test: bool, next_test: bool, /// files being imported (row 335): the one being read, those waiting, and one held for the large table's question
    import: Option<Importing>, import_wait: Vec<std::path::PathBuf>, import_big: Option<(import::Done, std::path::PathBuf)>, import_test: bool, unpack: Option<Unpack>, /// the export's options (read from the settings when first wanted)
    xopts: Option<export::Opts>, /// what follows a file panel's answer (one panel at a time)
    panel_then: Option<Box<dyn FnOnce(&mut App, Vec<std::path::PathBuf>)>>, pick: Option<ink::Pick>, gest: Option<ink::Gesture>, psel: Option<(BlockId, String)>, iblock: Option<(BlockId, Rect, String)>, ishown: Option<toolbar::InkShown>, ink_test: bool,
    pdf: Option<PdfSel>, resize: Option<Resize>, drag: Option<(mn_edit::Drop, Rect)>, fetches: std::collections::HashMap<AsyncRequestSerial, mn_edit::Drop>, pending: std::collections::HashMap<u64, mn_edit::Drop>, token: u64, locate: Option<BlockId>, relay: bool, vstop: bool, /// videos whose clips have notes (their blocks carry the row), the notes' drawings and captions hidden, when the notes were last asked to be read again
    noted: std::collections::HashSet<BlockId>, hide_notes: bool, notes_at: Option<Instant>,
    /// drawing on a video's frame: the tool that is armed (None: a click on the picture plays), the pen's colour and width
    /// (QCView's defaults: red, 6 source px), a gesture under way, the annotated picture owed (a note's key, when), the
    /// last clean frame captured (key, pixels, size), the palette is open for the pen, the field a note's words are typed
    /// in, what the toolbar shows of the current video, the offscreen layer annotated pictures are composed on, and the
    /// pictures being written (each tells its path when it is on disk)
    /// a table's menu is open (true: its column's, false: its row's); a column's border being dragged (the table, the
    /// column, where the pointer took hold, the width then, the table's kind before)
    /// wide tables' sideways scrolls (kept across layouts)
    /// a choice cell's picker (row 317, stage 3), and the timecode paragraph the caret was last in (it settles when left)
    picker: Option<choice::ChoicePicker>, tc_last: Option<BlockId>,
    /// the picker is a chip's in running text (row 320): the chip it is on
    picker_chip: Option<mn_edit::chips::Chip>,
    /// a link's editor (row 320), and the link a context menu was opened on
    /// find in the note (row 321): the bar, the occurrences, the current one, and what they were found for (the words, the case, the note's stamp)
    /// a canvas is open in the page's place (row 322); elements copied from a canvas; where files dragged over it would land; its context menu is open
    /// a note card of the open canvas is entered: its note is edited in place, in the card's box; pictures kept of the canvas's note cards; a note's row is being dragged from the sidebar; the notes a canvas's "Add Note" menu lists
    /// the open canvas is to be read again (a merge changed it)
    creload: bool,
    card: Option<Card>, cards: std::collections::HashMap<BlockId, CardPic>, row_drag: Option<(BlockId, (f64, f64), bool)>, cnotes: Vec<BlockId>,
    canvas: Option<canvas::Canvas>, cclip: Vec<mn_store::Element>, canvas_at: std::cell::Cell<Option<(f64, f64)>>, cmenu: bool,
    /// the search across the collection under way (the sidebar's field)
    search: Option<search::Search>,
    find: Option<findbar::FindBar>, hits: Vec<mn_edit::find::Hit>, hit_at: usize, found_for: Option<(String, bool, (u64, usize), BlockId)>,
    link: Option<linkedit::LinkEdit>, lmenu: Option<String>, /// the last press on a file chip (a second one soon after opens the file)
    chip_click: Option<(Instant, BlockId)>, /// a space was typed: the word before it becomes a link if it is an address
    autolink: bool,
    tscroll: std::collections::HashMap<BlockId, f32>,
    /// their bars (min-ui's overlay scrollbar, along the bottom, always shown)
    tbars: std::collections::HashMap<BlockId, min_ui::ScrollBar>, /// the note's own bar, at the page's right edge (shown while the note is longer than the page)
    pbar: min_ui::ScrollBar,
    tmenu: Option<bool>, col_drag: Option<(BlockId, usize, f64, f64, Kind)>,
    /// the gap between two columns of a Columns block being dragged (row 319): the two columns, the pointer's x at the press, the px all the columns share, all their shares, the two kinds before
    gap_drag: Option<(BlockId, BlockId, f64, f64, f32, Kind, Kind)>,
    tool: Option<qcv_annotate::Tool>, pen: Option<[f32; 4]>, pen_w: Option<f32>, drawing: bool, annot_due: Option<(String, Instant)>, clean: Option<(String, Arc<Vec<u8>>, (u32, u32))>, pal_pen: bool,
    note_field: Option<(BlockId, String, min_ui::TextField)>, /// the last press on a note's small picture (a second one soon after opens its words)
    thumb_click: Option<(Instant, BlockId, usize)>, vshown: Option<toolbar::VideoShown>, thumb_layer: Option<UiLayer>, written: Vec<std::sync::mpsc::Receiver<std::path::PathBuf>>, tested: bool, pdf_test: Option<BlockId>, turned: std::collections::HashSet<BlockId> }
struct Grab { from: (f64, f64), dragging: bool, drop: Option<(mn_edit::Drop, Rect)>, /// the pointer is over another note's row in the tree: the blocks would go to that note
    to_note: Option<BlockId> }
/// The last move of blocks to another note (row 307), while one undo in the note they left can still take it back whole:
/// both editors' stamps right after it (and, once undone, right after the undo, for redo).
struct Moved { from: BlockId, to: BlockId, from_stamp: (u64, usize), to_stamp: (u64, usize), undone: bool }
/// A save under way: which try, what the document is expected to be, what a merge brought.
struct Saving { attempt: u32, expect: Option<mn_store::share::Fingerprint>, /// Save As: the new place
    to: Option<std::path::PathBuf>, merged: Option<String>, /// blocks both sides changed, kept twice
    doubled: usize, started: Instant }
enum SaveStep { Put(Result<mn_store::share::Fingerprint, mn_store::share::PutError>), Fetched(std::io::Result<mn_store::share::Fingerprint>) }

/// What a dialog on screen is asking (row 293; the wording follows the Qt app's dialogs where it had them).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    /// make the caret's column this kind although cells lose content (1 choice, 2 check, 3 timecode)
    Kind(u8),
    /// delete this entry of the tree?
    Delete(BlockId),
    /// closing with changes that are not in the document: Cancel / Don't Save / Save
    Close,
    /// changes from an earlier session were found: Continue / Discard…
    Recovered,
    /// …and Discard asks once more, being the one thing here that cannot be undone
    DiscardRecovered,
    /// a notice with one button
    Notice,
    /// a save did not go through: OK / Retry
    SaveFailed,
    /// an imported file's table is very large: Cancel / Attach File / Import First Rows
    ImportBig,
}

/// What the collection is being closed for.
#[derive(Clone, Debug, PartialEq)]
enum Pending { Quit, Close, Open(std::path::PathBuf), New(std::path::PathBuf) }

struct App { /// the handle is held (row 305): where it was pressed, whether the pointer has moved far enough to be a drag, and where the blocks would land
    drawn: bool, /// frames still owed after the first: a frame presented before the window is on screen is not shown, and nothing else asks for another until the pointer moves (the user 2026-11-11)
    warm: u8, /// and a frame every quarter second for the launch's first second and a half, for a window that reaches the screen late
    warm_at: Option<Instant>, grab: Option<Grab>, moved: Option<Moved>, /// a short notice in the footer (what a move did), until this time
    notice: Option<(String, Instant)>,
    /// the context menu lists notes to move the selected blocks to
    ctx_notes: Vec<BlockId>,
    /// MN_BLOCKS: a block selection was made by the app itself, for a screenshot
    shown_blocks: bool,
    /// copied blocks, with the plain text that went to the system's clipboard beside them: a paste gives the blocks back while the clipboard still holds that text (row 304; stage E gives them a format of their own)
    block_clip: Option<(String, mn_edit::Clip)>, /// Control is down (Ctrl+Cmd+Up / Down move blocks)
    ctrl: bool,
    /// the pointer's shape as last set, and whether it is hidden (it hides while text is typed, until it moves)
    cursor: winit::cursor::CursorIcon, cursor_hidden: bool,
    /// the colour palette (row 301) and the last Background colour (Shift+Cmd+H applies it)
    pal: colours::Colours, last_bg: String, /// MN_PALETTE=type|background|picker: the palette is opened by the app itself, for a screenshot
    show_pal: Option<String>,
    /// the tree's context menu and the row it was opened on (None: the empty part of the sidebar)
    ctx: min_ui::menu::Menu, ctx_for: Option<BlockId>, /// the menu is the page's (blocks), not the tree's
    ctx_blocks: bool,
    /// commands to run at the loop's next idle step
    later: Vec<menu::Cmd>, foot: footer::Footer, /// the footer's height, physical px
    bottom: f64, /// the last save's outcome for the footer's readout (text, a warning)
    status: Option<(String, bool)>, /// the notes with something that is not in the document yet (the tree's dots, the readout's count)
    changed: std::collections::HashSet<BlockId>,
    /// the blank page's buttons: New Collection, Open
    splash: [min_ui::text_button::TextButton; 2], bar: toolbar::Toolbar, /// the band spans the window (the sidebar starts below it) rather than only the page
    bar_wide: bool, /// the page's top: the band's height, physical px
    top: f64, saver: mn_store::share::Worker<SaveStep>, saving: Option<Saving>, store: Option<Store>, sidebar: Option<sidebar::Sidebar>, others: std::collections::HashMap<BlockId, Editor>, side_w: f64, confirm: Option<(Ask, min_ui::dialog::Dialog)>, quit: bool, close_after_save: bool, recovered: bool, /// no collection is open: the window says so and waits for Open / New
    blank: bool, /// what closing the collection is for (asked first when there are changes that are not in the document)
    after_close: Option<Pending>, /// the pictures of the page (decoded off this thread)
    media: media::Media, /// the video that plays, if one does
    player: Option<Player>, /// the decoder's hardware device (asked for once)
    hw: Option<Option<qcv_player::HwDevice>>, /// the colour stage of videos' stills
    poster: video::Poster, files: Files, #[cfg(target_os = "macos")] menu: Option<menu::AppMenu>, ed: Editor, mods: (bool, bool, bool), pointer: (f64, f64), pressed: bool, last_click: Option<(Instant, f64, f64)>, clipboard: Option<arboard::Clipboard>, accent_popup: bool, /// where the page sat when the desktop was last told, and since when it has been somewhere else
    told_geom: (f64, f64), geom_moved: Option<Instant>, follow: bool, flat: Option<mn_doc::FlatText>, last_scroll: f64, layout: Option<NoteLayout>, gpu: Option<Gpu>, scroll: f64, seconds: f32, shot: Option<String>, t0: Option<Instant>, frames: u32, draw_ms: Vec<f32> }

fn sample() -> Doc {
    let mut d = Doc::new();
    fn add(d: &mut Doc, parent: Option<BlockId>, kind: Kind, text: &str) -> BlockId { let id = BlockId::new(); let at = d.children(parent).len(); d.insert(parent, at, id, kind, text).unwrap(); id }
    fn mark(d: &mut Doc, id: BlockId, marks: &[(&str, Mark)]) { let text = d.get(id).unwrap().text.clone(); let mut s = Spans::new(); for (word, m) in marks { if let Some(at) = text.find(word) { s.put(at, at + word.len(), m.clone()); } } d.set_spans(id, s).unwrap(); }
    add(&mut d, None, Kind::Heading { level: 1 }, "Review notes — second pass");
    let p = add(&mut d, None, Kind::Paragraph, "The grade on the wide shot holds up well against the reference. Bold, italic, inline code, a link, struck text, underlined text, a colour and a highlight all sit in this paragraph so that each can be seen once in running text that is long enough to wrap.");
    mark(&mut d, p, &[("Bold", Mark::Bold), ("italic", Mark::Italic), ("inline code", Mark::Code), ("a link", Mark::Link("https://example.com".into())), ("struck text", Mark::Strike), ("underlined text", Mark::Underline), ("a colour", Mark::Color("orange".into())), ("a highlight", Mark::Highlight("yellow".into()))]);
    add(&mut d, None, Kind::Heading { level: 2 }, "What is left");
    add(&mut d, None, Kind::Bullet { depth: 0 }, "Titles sit inside the safe area on every format");
    add(&mut d, None, Kind::Bullet { depth: 1 }, "except the vertical cut, which needs a second look");
    add(&mut d, None, Kind::Numbered { depth: 0 }, "Conform the new cut");
    add(&mut d, None, Kind::Numbered { depth: 0 }, "Check the audio mix on small speakers");
    add(&mut d, None, Kind::Task { depth: 0, state: 0 }, "Send the stills"); add(&mut d, None, Kind::Task { depth: 0, state: 1 }, "Second colour pass"); add(&mut d, None, Kind::Task { depth: 0, state: 2 }, "First colour pass");
    add(&mut d, None, Kind::Quote, "Each version was compared against the previous one, frame by frame.");
    add(&mut d, None, Kind::Code { lang: "sh".into() }, "ffmpeg -i in.mov -c:v prores_ks -profile:v 4 out.mov\n# a second line of code");
    add(&mut d, None, Kind::Heading { level: 3 }, "Side by side");
    let cols = add(&mut d, None, Kind::Columns, ""); let (c1, c2) = (add(&mut d, Some(cols), Kind::Column { width: 0.4 }, ""), add(&mut d, Some(cols), Kind::Column { width: 0.6 }, ""));
    add(&mut d, Some(c1), Kind::Media { descriptor: r#"{"w":640,"h":360,"kind":"video","name":"placeholder for the embedded player"}"#.into() }, "");
    add(&mut d, Some(c2), Kind::Paragraph, "Annotations beside media: this column holds the text, the other the picture. The player will draw its picture into the placeholder's rectangle, under this layer.");
    add(&mut d, Some(c2), Kind::Bullet { depth: 0 }, "a short list beside copy");
    add(&mut d, None, Kind::Heading { level: 3 }, "A table");
    let opts = vec![ChoiceOption { id: "a".into(), label: "Open".into(), color: "#2F6F3A".into() }, ChoiceOption { id: "b".into(), label: "Closed".into(), color: "#7A2E2A".into() }];
    let table = add(&mut d, None, Kind::Table { header_rows: 1, columns: vec![ColumnSpec { width: Some(260), ..Default::default() }, ColumnSpec { kind: ColumnKind::Choice { options: opts }, ..Default::default() }, ColumnSpec { kind: ColumnKind::Timecode { fps: 24.0 }, ..Default::default() }] }, "");
    for (i, row) in [["Shot", "State", "Timecode"], ["The wide shot at the top of the closing sequence, which wraps", "Open", "01:00:12:08"], ["Titles", "Closed", "01:02:40:00"]].iter().enumerate() {
        let r = add(&mut d, Some(table), Kind::row(), "");
        for (j, t) in row.iter().enumerate() { let c = add(&mut d, Some(r), Kind::cell(), ""); let pid = add(&mut d, Some(c), Kind::Paragraph, t);
            if i > 0 && j == 1 { let mut s = Spans::new(); s.put(0, t.len(), Mark::Choice(format!(r##"{{"o":[{{"c":"#2F6F3A","id":"a","l":"Open"}},{{"c":"#7A2E2A","id":"b","l":"Closed"}}],"v":"{}"}}"##, if *t == "Open" { "a" } else { "b" }))); d.set_spans(pid, s).unwrap(); } }
    }
    add(&mut d, None, Kind::Divider, "");
    for i in 0..24 { add(&mut d, None, Kind::Paragraph, &format!("Paragraph {} of the tail, so that the note is long enough to scroll: nothing in the audio mix stood out on the small speakers, and the client asked for a warmer look in the closing sequence.", i + 1)); }
    d
}

impl App {
    fn redraw(&mut self) {
        self.settle_timecode();
        self.canvas_save(false);
        self.find_refresh();
        self.search_step();
        let Some(g) = self.gpu.as_mut() else { return };
        let t = Instant::now();
        let (w, h) = g.presenter.size(); let s = g.window.scale_factor(); let now = Instant::now();
        // the sidebar takes its width off the page (it slides when it folds, so the page follows frame by frame)
        // the band: across the window (the sidebar starts below it) or over the page only (beside the sidebar)
        if self.bar_wide { self.top = self.bar.layout(&mut g.fonts, 0.0, w as f64, 0.0, s); self.side_w = self.sidebar.as_mut().map(|sb| sb.layout(self.top, h as f64, s, now)).unwrap_or(0.0); }
        else { self.side_w = self.sidebar.as_mut().map(|sb| sb.layout(0.0, h as f64, s, now)).unwrap_or(0.0); self.top = self.bar.layout(&mut g.fonts, self.side_w, w as f64, 0.0, s); }
        // the sidebar's caret floats over the page's top left corner: it is hit before the band

        self.bottom = self.foot.layout(&mut g.fonts, self.side_w, w as f64, h as f64, s);
        { let st = self.ed.state(); if self.bar.sync(&toolbar::Shown { state: &st, canvas: self.files.canvas.is_some() && self.files.card.is_none(), has_note: !self.blank, in_cell: self.ed.selection().is_some_and(|x| self.ed.cell_of(x.focus.block).is_some()), video: self.files.vshown, ink: self.files.canvas.as_ref().filter(|_| self.files.card.is_none()).map(|cv| { use canvas::Ink as I; use toolbar::Tool as T; toolbar::InkShown { page: false, tool: Some(match cv.tool { None if cv.framing => T::Frame, None => T::DrawSelect, Some(I::Pen) => T::DrawPen, Some(I::Line) => T::DrawLine, Some(I::Arrow) => T::DrawArrow, Some(I::Rect) => T::DrawRect, Some(I::Oval) => T::DrawOval, Some(I::Eraser) => T::DrawErase }), pen: peniko::Color::new(cv.pen) } }).or(self.files.ishown) }) { g.window.request_redraw(); }
            let unsaved = self.store.as_ref().is_some_and(|x| x.unpublished || x.edited.is_some());
            // the readout: a save under way, a save that failed, what is not in the document yet, else the last outcome
            let n = self.changed.len() + self.store.as_ref().is_some_and(|x| x.edited.is_some() && !self.changed.contains(&x.note)) as usize;
            let (text, warn) = if self.store.is_none() { (String::new(), false) } else if self.saving.is_some() { ("Saving\u{2026}".to_string(), false) }
                else if let Some((t, true)) = &self.status { (t.clone(), true) }
                else if let Some(i) = &self.files.import { (format!("Importing {}\u{2026}", i.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), false) }
                else if let Some(u) = &self.files.unpack { (format!("Unpacking {} \u{2014} {}%", u.name, if u.total > 0 { u.done.load(std::sync::atomic::Ordering::Relaxed) * 100 / u.total } else { 0 }), false) }
                else if let Some(t) = self.files.gather.as_ref().and_then(|w| w.readout()) { (t, false) }
                else if let Some((t, _)) = self.notice.as_ref().filter(|(_, until)| now < *until) { (t.clone(), false) }
                else if unsaved { (match n { 0 => "Changes not saved to the document".to_string(), 1 => "1 note changed \u{2014} not saved to the document".to_string(), n => format!("{n} notes changed \u{2014} not saved to the document") }, false) }
                else { (self.status.as_ref().map(|x| x.0.clone()).unwrap_or_else(|| "Saved".to_string()), false) };
            self.foot.sync(&footer::Shown { text: &text, warn, link: self.files.gather.as_ref().is_some_and(|w| w.readout().is_some()) && self.saving.is_none() && !self.status.as_ref().is_some_and(|x| x.1), finding: self.files.find.is_some(), has_note: !self.blank || self.files.canvas.is_some(), has_file: self.store.is_some(), sidebar_open: self.sidebar.as_ref().is_some_and(|sb| !sb.rail.collapsed), can_undo: self.files.canvas.as_ref().filter(|_| self.files.card.is_none()).map(|c| c.can_undo()).unwrap_or(st.can_undo), can_redo: self.files.canvas.as_ref().filter(|_| self.files.card.is_none()).map(|c| c.can_redo()).unwrap_or(st.can_redo), unsaved, saving: self.saving.is_some() }); }
        if let (Some(c), Some(cv)) = (self.files.card.as_mut(), self.files.canvas.as_ref()) { if let Some(r) = cv.rect_of(c.el) { if r != c.rect { self.geom_moved = Some(Instant::now()); } c.rect = r; } }
        if let Some(sb) = self.sidebar.as_mut() { sb.defer_open = self.files.canvas.is_some() && self.files.card.is_none(); }
        let page_w = match &self.files.card { Some(c) => (c.rect.width() - 2.0 * CARD_PAD * s).max(120.0 * s), None => (w as f64 - self.side_w - 2.0 * MARGIN * s).max(320.0 * s) };
        // the page moved or changed width without a scroll or an edit (the sidebar, the window): the desktop is told once it rests
        if (self.side_w, page_w + (self.top + self.bottom * 1e-3) * 1e-3) != self.told_geom { self.told_geom = (self.side_w, page_w + (self.top + self.bottom * 1e-3) * 1e-3); self.geom_moved = Some(now); }
        #[cfg(target_os = "macos")] text_element::set_obscured((self.files.canvas.is_some() && self.files.card.is_none()) || self.confirm.is_some() || self.files.gather.as_ref().is_some_and(|w| w.shown) || self.pal.is_open() || self.ctx.is_open() || self.sidebar.as_ref().is_some_and(|sb| sb.renaming()));
        if self.layout.as_ref().is_none_or(|l| (l.width as f64 - page_w).abs() > 0.5 || (l.scale as f64 - s).abs() > 1e-6) { { let mut nl = NoteLayout::build_media(&self.ed.doc, &mut g.fonts, &style(), page_w as f32, s as f32, self.files.turned.clone(), self.files.noted.clone()); nl.set_table_scrolls(&self.files.tscroll); self.layout = Some(nl); } }
        // MN_DEMO=1 (for `--shot` self-checks; no input is sent): the app types into its own first paragraph and selects
        // from the heading into the first list item
        if self.frames == 0 && std::env::var_os("MN_DEMO").is_some() {
            let top: Vec<BlockId> = self.ed.doc.children(None).to_vec(); use mn_edit::{Pos, Selection};
            if let Some(target) = top.get(1).or(top.first()).copied() {
                self.ed.set_selection(Some(Selection::caret(Pos { block: target, at: 0 }))); let _ = self.ed.insert("Typed by the demo. ", 0); self.layout.as_mut().unwrap().update_block(&self.ed.doc, &mut g.fonts, target);
                if let Some(st) = self.store.as_mut() { st.edited = Some(Instant::now()); }
                if top.len() > 3 { self.ed.set_selection(Some(Selection { anchor: Pos { block: top[0], at: 7 }, focus: Pos { block: top[3], at: 10 } })); } }
        }
        // in a wide table the caret is brought into view sideways too (the table scrolls inside its block)
        if self.follow { let l = self.layout.as_ref().unwrap(); if let Some(f) = self.ed.selection().map(|x| x.focus) { if let Some(t) = l.wide_table_of(&self.ed.doc, f.block) {
            if let (Some(c), Some(tr), Some((full, vis, at))) = (self.ed.caret_rect(l, 1.0), l.rect(&self.ed.doc, t), l.table_scroll(t)) { let m = 24.0 * s;
                let to = if c.x0 < tr.x0 + m { at as f64 - (tr.x0 + m - c.x0) } else if c.x1 > tr.x1 - m { at as f64 + (c.x1 - (tr.x1 - m)) } else { at as f64 }.clamp(0.0, (full - vis).max(0.0) as f64);
                if (to - at as f64).abs() > 0.5 { self.files.tscroll.insert(t, to as f32); let map = self.files.tscroll.clone(); if let Some(l) = self.layout.as_mut() { l.set_table_scrolls(&map); } } } } } }
        let l = self.layout.as_ref().unwrap();
        // after an edit or a caret move the caret is brought into view
        // (in a card: the card's own height and edge)
        let (pad, view_h) = match &self.files.card { Some(c) => (CARD_PAD, c.rect.height()), None => (PAD, h as f64 - self.top - self.bottom) };
        if let (Some(c), Some(cv)) = (&self.files.card, self.files.canvas.as_mut()) { cv.set_fit(c.el, l.height as f64 / s + 2.0 * CARD_PAD); }
        if std::mem::take(&mut self.follow) { if let Some(c) = self.ed.caret_rect(l, 1.0) { let (top, bottom) = (c.y0 + pad * s - 24.0 * s, c.y1 + pad * s + 24.0 * s); if top < self.scroll { self.scroll = top } else if bottom > self.scroll + view_h { self.scroll = bottom - view_h } } }
        self.scroll = self.scroll.clamp(0.0, (l.height as f64 + 2.0 * pad * s - view_h).max(0.0));
        #[cfg(target_os = "macos")] if (self.scroll - self.last_scroll).abs() > 0.01 { text_element::scrolled(); }
        self.last_scroll = self.scroll;
        let view = Rect::new(0.0, 0.0, w as f64, h as f64);
        let off = origin(&self.files.card, self.side_w, self.top, self.scroll, s);
        // the video that plays (rows 287, 316): the player is asked for the frame that is due and the presenter shows it
        // UNDER this layer, where the page is left open; a block that left the view (or the note) stops being the one
        let (mut hole, mut live): (Option<Rect>, Option<paint::Live>) = (None, None);
        if let Some(p) = self.player.as_mut() {
            let on_canvas = self.files.canvas.is_some() && self.files.card.is_none();
            let pic = self.ed.doc.get(p.id).filter(|b| matches!(&b.kind, Kind::Media { descriptor } if descriptor.contains("\"kind\":\"video\""))).and_then(|_| l.rect(&self.ed.doc, p.id)).map(|r| r + off).map(|r| Rect::new(r.x0, r.y0, r.x1, r.y1 - (mn_layout::VIDEO_STRIP + if l.is_noted(p.id) { mn_layout::VIDEO_NOTES } else { 0.0 }) as f64 * s));
            let pic = if on_canvas { self.files.canvas.as_ref().and_then(|cv| cv.video_place(p.id)) } else { pic.filter(|r| !self.blank && r.y1 > self.top && r.y0 < h as f64 - self.bottom) };
            match pic {
                None => self.files.vstop = true,
                Some(r) => {
                    match p.t.tick(now) { Some(f) => { if g.presenter.engine.show(&f.frame).is_ok() { p.shown = true; } } None => p.t.idle_refresh() }
                    g.presenter.engine.set_place(Some([r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32]));
                    if p.shown { hole = Some(r); }
                    live = Some(paint::Live { id: p.id, frame: p.t.current_frame(), playing: p.t.is_playing(), shown: p.shown, speed: p.t.review_speed(), muted: p.t.audio.is_muted(), looping: p.t.loop_enabled() });
                    // held keys shuttle: the player moves on while nothing else asks for a frame
                    if p.t.is_fast_seeking() { g.window.request_redraw(); }
                    if p.t.is_playing() || p.scrub || !p.shown || now < p.awake { g.window.request_redraw(); }
                }
            }
        }
        if let (Some(hr), Some(c)) = (hole, &self.files.card) { hole = Some(hr.intersect(c.rect)).filter(|r| r.width() > 0.0 && r.height() > 0.0); }
        let scene = g.ui.scene_mut(); scene.reset();
        // the page's ground, left open where the video plays
        match hole.map(|r| r.intersect(view)).filter(|r| r.width() > 0.0 && r.height() > 0.0) {
            None => scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::page(), None, &view),
            Some(r) => for part in [Rect::new(view.x0, view.y0, view.x1, r.y0), Rect::new(view.x0, r.y1, view.x1, view.y1), Rect::new(view.x0, r.y0, r.x0, r.y1), Rect::new(r.x1, r.y0, view.x1, r.y1)] { if part.width() > 0.0 && part.height() > 0.0 { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::page(), None, &part); } },
        }
        if self.blank && self.files.canvas.is_none() {
            // no collection: the window says what to do (the theme's subdued text, centred)
            let lines = [("No collection is open", 18.0, paint::text()), (if cfg!(target_os = "macos") { "New Collection\u{2026} (\u{2325}\u{2318}N) or Open\u{2026} (\u{2318}O)" } else { "New Collection\u{2026} (Ctrl+Alt+N) or Open\u{2026} (Ctrl+O)" }, 13.0, paint::subdued())];
            let mut y = h as f64 / 2.0 - 50.0 * s;
            for (text, size, color) in lines { let t = g.fonts.layout(text, min_ui::Family::Sans, (size * s) as f32, 400.0, color); min_ui::fonts::draw_layout(scene, &t, kurbo::Affine::translate(((w as f64 - t.width() as f64) / 2.0, y))); y += t.height() as f64 + 8.0 * s; }
            // the two ways in, as buttons (the theme's text buttons)
            let gap = min_theme::tokens::SPACING_200 as f64 * s; y += gap;
            let sizes: Vec<(f64, f64)> = self.splash.iter().map(|b| b.measure(&mut g.fonts, s)).collect();
            let mut x = (w as f64 - sizes.iter().map(|z| z.0).sum::<f64>() - gap) / 2.0;
            for (b, (bw, bh)) in self.splash.iter_mut().zip(sizes) { b.rect = kurbo::Rect::new(x, y, x + bw, y + bh); b.paint(scene, &mut g.fonts, s, now); x += bw + gap; if b.animating(now) { g.window.request_redraw(); } }
        }
        { let se = self.store.as_ref().and_then(|st| st.session.as_ref()); self.media.begin_frame(); self.media.set_base(se.and_then(|x| x.original().parent()), se.map(|x| x.media_dir())); }
        // a canvas in the page's place (row 322); its note cards are drawn from pictures kept of them, and the one being edited by the note's own layer, through its card
        // MN_SPIN=1 (scripted): the canvas is panned a little at every frame and the frames are counted - how fast it draws (`all`: with everything in view)
        if SPIN.get_or_init(|| std::env::var_os("MN_SPIN").is_some()).to_owned() { if let Some(cv) = self.files.canvas.as_mut() { let n = SPUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed); if n == 30 { *SPIN_T0.lock().unwrap() = Some(Instant::now()); } if n == 3 && std::env::var("MN_SPIN").is_ok_and(|v| v == "all") { cv.fit_all(); } cv.pan_by(if (n / 240) % 2 == 0 { 3.0 } else { -3.0 }, 0.0); g.window.request_redraw(); } }
        let live_video = live;
        if let Some(cv) = self.files.canvas.as_mut() { let live = self.files.card.as_ref().map(|c| c.el);
            let (cards, ed, store, others) = (&mut self.files.cards, &self.ed, self.store.as_ref(), &self.others);
            let mut note = |scene: &mut vello::Scene, fonts: &mut min_ui::Fonts, media: &mut media::Media, e: &mn_store::Element, r: Rect, k: f64, far: bool| card_pic(cards, ed, store, others, scene, fonts, media, e, r, k, s, far);
            let (ann, hide, vlive) = (self.player.as_ref().map(|p| &p.ann), self.files.hide_notes, live_video);
            let mut video = |scene: &mut vello::Scene, fonts: &mut min_ui::Fonts, media: &mut media::Media, e: &mn_store::Element, r: Rect, strip: Option<Rect>| { let me = vlive.filter(|m| m.id == e.id); paint::video_card(scene, fonts, media, &e.record, r, strip, me, ann.filter(|_| me.is_some()), hide, s); };
            if cv.paint(scene, &mut g.fonts, &mut self.media, Rect::new(view.x0.max(self.side_w), view.y0.max(self.top), view.x1, if self.foot.rect.height() > 0.0 { self.foot.rect.y0.min(view.y1) } else { view.y1 }), s, live, &mut note, vlive.filter(|_| live.is_none()).map(|m| m.id), hole, &mut video) { g.window.request_redraw(); } }
        let carded = self.files.card.as_ref().map(|c| c.rect);
        if let Some(r) = carded { scene.push_clip_layer(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, &Rect::new(r.x0 - 40.0 * s, r.y0, r.x1, r.y1).intersect(Rect::new(self.side_w, self.top, w as f64, self.foot.rect.y0.max(self.top)))); }
        let picked = self.ed.block_selection();
        for id in &picked { if let Some(r) = l.rect(&self.ed.doc, *id) { let r = kurbo::Rect::new(-8.0 * s, r.y0 - 2.0 * s, l.width as f64 + 8.0 * s, r.y1 + 2.0 * s) + off; scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::selection(), None, &kurbo::RoundedRect::from_rect(r, 4.0 * s)); } }
        for r in self.ed.selection_rects(l) { if self.blank || !picked.is_empty() { break } scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::selection(), None, &(r + off)); }
        // find (row 321): every occurrence is marked, the current one more strongly; under the text, as the selection is
        if self.files.find.is_some() && !self.blank { let (soft, strong) = (peniko::Color::from_rgba8(0xe5, 0xa3, 0x3b, 0x55), peniko::Color::from_rgba8(0xe5, 0xa3, 0x3b, 0xb0));
            for (i, h) in self.files.hits.iter().enumerate() { let rects = if h.kind == mn_edit::find::HitKind::Block { l.rect(&self.ed.doc, h.block).into_iter().collect() } else { l.range_rects(&self.ed.doc, h.block, h.start, h.end) };
                for r in rects { let r = r + off; if r.y1 < view.y0 || r.y0 > view.y1 { continue } if h.kind == mn_edit::find::HitKind::Block { scene.stroke(&kurbo::Stroke::new(2.0 * s), kurbo::Affine::IDENTITY, if i == self.files.hit_at { strong } else { soft }, None, &kurbo::RoundedRect::from_rect(r.inflate(2.0 * s, 2.0 * s), 6.0 * s)); }
                    else { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, if i == self.files.hit_at { strong } else { soft }, None, &kurbo::RoundedRect::from_rect(r, 2.0 * s)); } } } }
        let (mut turned, mut noted) = (vec![], vec![]);
        if !self.blank && paint::note(scene, &self.ed.doc, l, &mut g.fonts, &mut self.media, &mut turned, &mut noted, live, self.player.as_ref().filter(|_| !self.files.hide_notes || self.files.tool.is_some()).map(|p| &p.ann), self.files.hide_notes, off.x, off.y, view) { g.window.request_redraw(); }
        if !self.blank { let now = Instant::now(); let seen = match &self.files.card { Some(c) => c.rect, None => Rect::new(self.side_w, self.top, w as f64, h as f64 - self.bottom) };
            sync_bars(&mut self.files.tbars, l, &self.ed.doc, off, s, seen); for b in self.files.tbars.values() { if b.thumb().is_some_and(|t| t.y0 >= view.y0 && t.y1 <= view.y1) { b.paint(scene, now); } if b.animating(now) { g.window.request_redraw(); } }
            // the note's own bar (not a card's: the canvas scrolls those)
            if self.files.card.is_none() { let b = &mut self.files.pbar; b.always = true; b.set(seen, l.height as f64 + 2.0 * PAD * s, s); b.offset = self.scroll; b.paint(scene, now); if b.animating(now) { g.window.request_redraw(); } } }
        // a picture stored on its side in a box of the stored shape: the box is laid out again, upright
        for id in turned { if self.files.turned.insert(id) { self.files.relay = true; g.window.request_redraw(); } }
        // a video whose clip got its first note (or lost its last): its block gains (or loses) the row of the notes' pictures
        for (id, on) in noted { if if on { self.files.noted.insert(id) } else { self.files.noted.remove(&id) } { self.files.relay = true; g.window.request_redraw(); } }
        // text selected on a PDF's page: washed over where it is
        if let Some(sel) = &self.files.pdf { if let Some(Kind::Media { descriptor }) = self.ed.doc.get(sel.id).map(|b| &b.kind) { if let (Some(t), Some(r)) = (self.media.page_text(descriptor), l.rect(&self.ed.doc, sel.id)) {
            let r = r + off; let k = r.width() / t.w.max(1.0) as f64;
            for (x0, y0, x1, y1) in t.rects(sel.a, sel.b) { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::accent().multiply_alpha(0.35), None, &Rect::new(r.x0 + x0 as f64 * k, r.y0 + y0 as f64 * k, r.x0 + x1 as f64 * k, r.y0 + y1 as f64 * k)); } } } }
        // a note's words being typed: the field lies over the caption's place on the playing video's picture
        if carded.is_some() { scene.pop_layer(); }
        { let mut gone = false; if let Some(p) = self.files.picker.as_mut() { match self.files.picker_chip.as_ref().map(|c| l.range_rects(&self.ed.doc, c.0, c.1, c.2).into_iter().next()).unwrap_or_else(|| l.rect(&self.ed.doc, p.cell)) { Some(r) => { p.layout(&mut g.fonts, r + off, Rect::new(0.0, self.top, w as f64, h as f64), s); p.paint(scene, &mut g.fonts); if p.animating(now) { g.window.request_redraw(); } } None => gone = true } } if gone { self.files.picker = None; self.files.picker_chip = None; } }
        if let Some(f) = self.files.find.as_mut() { f.count = (if self.files.hits.is_empty() { 0 } else { self.files.hit_at + 1 }, self.files.hits.len()); f.layout(&mut g.fonts, Rect::new(self.side_w, self.top, w as f64, h as f64), s); f.paint(scene, &mut g.fonts); if f.animating(now) { g.window.request_redraw(); } }
        if let Some(p) = self.files.link.as_mut() { p.layout(&mut g.fonts, p.anchor + off, Rect::new(0.0, self.top, w as f64, h as f64), s); p.paint(scene, &mut g.fonts); if p.animating(now) { g.window.request_redraw(); } }
        if let Some((id, _, f)) = self.files.note_field.as_mut() { if let Some(r) = l.rect(&self.ed.doc, *id).map(|r| r + off) { let y1 = r.y1 - (mn_layout::VIDEO_STRIP + if l.is_noted(*id) { mn_layout::VIDEO_NOTES } else { 0.0 }) as f64 * s;
            let band = Rect::new(r.x0, y1 - 26.0 * s, r.x1, y1); scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, min_ui::paint::color(min_theme::tokens::BACKGROUND_LAYER_2_COLOR), None, &band);
            f.layout(&mut g.fonts, band.inset(-2.0 * s), s); f.paint(scene, &mut g.fonts, s); } }
        // ink on the current picture or PDF page (row 325): the stroke being drawn, as it will be kept; the picked stroke's box
        if let Some((id, frame, d)) = self.files.iblock.as_ref().filter(|b| b.1.width() >= 1.0) { let rec: serde_json::Value = serde_json::from_str(d).unwrap_or_default(); let size = ink::page_size(&rec, *frame); let k = size.0 / frame.width();
            let at = kurbo::Affine::translate((frame.x0, frame.y0)) * kurbo::Affine::scale(1.0 / k); let (pen, w) = (self.files.pen.unwrap_or([1.0, 0.0, 0.0, 1.0]), self.files.pen_w.unwrap_or(6.0) as f64);
            if let Some(line) = self.files.gest.as_ref().filter(|g| g.block == *id).and_then(|g| g.live(pen, w * s * k)) { scene.push_clip_layer(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, frame); ink::drawn(&line, kurbo::Point::ORIGIN).paint(scene, at); scene.pop_layer(); }
            if let Some((_, sid)) = self.files.psel.as_ref().filter(|p| p.0 == *id) { let now = self.ed.doc.get(*id).and_then(|b| match &b.kind { Kind::Media { descriptor } => serde_json::from_str::<serde_json::Value>(descriptor).ok(), _ => None }).unwrap_or(rec);
                if let Some(l) = ink::page_lines(&now, ink::page_no(&now), size).into_iter().find(|l| l.s.id == *sid) { let b = at.transform_rect_bbox(ink::bounds(&l)).inflate(2.0 * s, 2.0 * s); scene.stroke(&kurbo::Stroke::new(1.5 * s).with_dashes(0.0, [4.0 * s, 3.0 * s]), kurbo::Affine::IDENTITY, paint::accent(), None, &b); } } }
        // the current picture: an outline, and a handle at each corner to resize it by
        let current = (!self.blank && !self.grab.as_ref().is_some_and(|x| x.dragging) && self.ed.run_len() == 1).then(|| self.ed.handle_block()).flatten()
            .filter(|id| matches!(self.ed.doc.get(*id).map(|b| &b.kind), Some(Kind::Media { descriptor }) if !descriptor.contains("\"kind\""))).and_then(|id| l.rect(&self.ed.doc, id)).map(|r| r + off);
        if let Some(r) = current {
            let r = Rect::new(r.x0.round(), r.y0.round(), r.x1.round(), r.y1.round());
            scene.stroke(&kurbo::Stroke::new(1.5 * s), kurbo::Affine::IDENTITY, paint::accent(), None, &r);
            for (cx, cy) in [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)] { let h = Rect::new(cx - 4.5 * s, cy - 4.5 * s, cx + 4.5 * s, cy + 4.5 * s);
                scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::page(), None, &h.inflate(1.5 * s, 1.5 * s)); scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::accent(), None, &h); }
        }
        // files dragged over the page: the line where they would land
        if let Some((_, line)) = &self.files.drag { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::accent(), None, &kurbo::RoundedRect::from_rect(*line + off, s)); }
        // a composition is underlined where it stands
        if let Some((id, a, b)) = self.ed.preedit() { for r in l.range_rects(&self.ed.doc, id, a, b) { let r = r + off; scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, peniko::Color::WHITE, None, &Rect::new(r.x0, r.y1 - 1.5 * s, r.x1, r.y1)); } }
        // the caret's table: its row's and its column's handles, and the strips that add a row and a column at the end
        if let Some(ui) = (!self.blank && self.files.picker.is_none()).then(|| table_ui(&self.ed, l, s, off)).flatten() {
            scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, min_ui::paint::color(min_theme::tokens::BACKGROUND_LAYER_2_COLOR), None, &kurbo::RoundedRect::from_rect(ui.col, 7.0 * s));
            for (r, icon) in [(ui.row, "dots-six-vertical"), (ui.col, "dots-six")] { if let Some(i) = g.fonts.icon(icon, (16.0 * s) as f32, paint::subdued()) { min_ui::fonts::draw_layout(scene, &i, kurbo::Affine::translate((r.center().x - i.width() as f64 / 2.0, r.center().y - i.height() as f64 / 2.0))); } }
            for r in [ui.add_row, ui.add_col] { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, min_ui::paint::color(min_theme::tokens::BACKGROUND_LAYER_2_COLOR), None, &kurbo::RoundedRect::from_rect(r, 3.0 * s));
                if let Some(i) = g.fonts.icon("plus", (10.0 * s) as f32, paint::subdued()) { min_ui::fonts::draw_layout(scene, &i, kurbo::Affine::translate((r.center().x - i.width() as f64 / 2.0, r.center().y - i.height() as f64 / 2.0))); } }
        }
        // the handle beside the caret's block; while it is dragged, the line where the blocks would land and how many they are
        if let Some(hr) = (!self.blank).then(|| handle_rect_in(&self.ed, l, s, off)).flatten() {
            let dragging = self.grab.as_ref().is_some_and(|x| x.dragging);
            if let Some(i) = g.fonts.icon("dots-six-vertical", (16.0 * s) as f32, if dragging { paint::text() } else { paint::subdued() }) { min_ui::fonts::draw_layout(scene, &i, kurbo::Affine::translate((hr.x0 + (hr.width() - i.width() as f64) / 2.0, hr.y0 + (hr.height() - i.height() as f64) / 2.0))); }
            if let Some(gr) = self.grab.as_ref().filter(|x| x.dragging) {
                if let Some((_, line)) = &gr.drop { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::accent(), None, &kurbo::RoundedRect::from_rect(*line + off, s)); }
                let n = self.ed.run_len(); let label = if n == 1 { "1 block".to_string() } else { format!("{n} blocks") };
                let label = match gr.to_note.and_then(|id| self.sidebar.as_ref().and_then(|sb| sb.row(id)).map(|r| r.title.clone())) { Some(t) => format!("{} {label} to \u{201c}{}\u{201d}", if self.mods.2 { "Copy" } else { "Move" }, t.chars().take(28).collect::<String>()), None => label };
                let t = g.fonts.layout(&label, min_ui::Family::Sans, (12.0 * s) as f32, 500.0, paint::text());
                let (px, py) = (self.pointer.0 + 14.0 * s, self.pointer.1 + 10.0 * s);
                let chip = kurbo::Rect::new(px, py, px + t.width() as f64 + 16.0 * s, py + t.height() as f64 + 8.0 * s);
                scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, min_ui::paint::color(min_theme::tokens::BACKGROUND_LAYER_2_COLOR), None, &kurbo::RoundedRect::from_rect(chip, 6.0 * s));
                min_ui::fonts::draw_layout(scene, &t, kurbo::Affine::translate((px + 8.0 * s, py + 4.0 * s)));
            }
        }
        if !self.blank && picked.is_empty() && self.ed.selection().is_some_and(|x| x.is_caret()) { if let Some(c) = self.ed.caret_rect(l, (1.5 * s) as f32) { let c = c + off; scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::caret(), None, &c);
            // the input method's candidate window follows the caret
            if g.ime_area != Some(c) { g.ime_area = Some(c); ime(g.window.as_ref().as_ref(), Some(c), false); } } }
        // over the page: the sidebar, then a question being asked
        // the band first: the sidebar's floating caret sits over its left end when the band is over the page only
        self.bar.paint(scene, &mut g.fonts, now); self.foot.paint(scene, &mut g.fonts, now);
        if let Some(sb) = self.sidebar.as_mut() {
            let mut changed = self.changed.clone(); if let Some(st) = self.store.as_ref() { if st.edited.is_some() { changed.insert(st.note); } }
            sb.paint(scene, &mut g.fonts, self.files.canvas.as_ref().map(|c| c.id).or(self.store.as_ref().map(|st| st.note)), &changed, now); if sb.rail.animating(now) || sb.tick(now) { g.window.request_redraw(); }
            sb.paint_tooltip(scene, &mut g.fonts, view, now); }
        self.bar.paint_tooltip(scene, &mut g.fonts, view, now); self.foot.paint_tooltip(scene, &mut g.fonts, view, now);
        if self.ctx.is_open() { self.ctx.paint(scene, &mut g.fonts, s); }
        self.pal.paint(scene, &mut g.fonts, paint::page(), now);
        if self.foot.tick(now) { g.window.request_redraw(); }
        if self.bar.tick(now) { g.window.request_redraw(); }
        if let Some(w) = self.files.gather.as_mut() { w.paint(scene, &mut g.fonts, view, s, now); if w.animating(now) { g.window.request_redraw(); } }
        if let Some((_, d)) = self.confirm.as_mut() { d.layout(&mut g.fonts, view, s); d.paint(scene, &mut g.fonts, s, view, now); if d.animating(now) { g.window.request_redraw(); } }
        g.ui.render(&g.presenter.engine.device, &g.presenter.engine.queue);
        let window = g.window.clone();
        g.presenter.draw(|| window.pre_present_notify());
        self.frames += 1; self.draw_ms.push(t.elapsed().as_secs_f32() * 1000.0);
    }
    /// What an edit changed, taken to the layout; the caret is then followed.
    fn apply(&mut self, d: Dirty) {
        let d_changed = d != Dirty::Nothing;
        if d_changed { if let Some(st) = self.store.as_mut() { let first = st.edited.is_none() && !st.unpublished; st.edited = Some(Instant::now()); if first { self.title(); } } }
        match d { Dirty::Nothing => {}, Dirty::Structure => self.layout = None,
            Dirty::Blocks(ids) => if let (Some(l), Some(g)) = (self.layout.as_mut(), self.gpu.as_mut()) { for id in ids { l.update_block(&self.ed.doc, &mut g.fonts, id); } } }
        let text_changed = d_changed; if text_changed { self.flat = None; }
        #[cfg(target_os = "macos")] text_element::changed(text_changed);
        self.follow = true; if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The layout and the flat text, current (a tool may ask between two frames).
    fn ensure(&mut self) -> bool {
        let Some(g) = self.gpu.as_mut() else { return false };
        let (w, _) = g.presenter.size(); let s = g.window.scale_factor();
        let page_w = match &self.files.card { Some(c) => (c.rect.width() - 2.0 * CARD_PAD * s).max(120.0 * s), None => (w as f64 - self.side_w - 2.0 * MARGIN * s).max(320.0 * s) };
        if self.layout.as_ref().is_none_or(|l| (l.width as f64 - page_w).abs() > 0.5 || (l.scale as f64 - s).abs() > 1e-6) { { let mut nl = NoteLayout::build_media(&self.ed.doc, &mut g.fonts, &style(), page_w as f32, s as f32, self.files.turned.clone(), self.files.noted.clone()); nl.set_table_scrolls(&self.files.tscroll); self.layout = Some(nl); } }
        if self.flat.is_none() { self.flat = Some(mn_doc::FlatText::build(&self.ed.doc)); }
        true
    }
    /// The note as it stands goes into the staged work copy (autosave never touches the real file — row 272).
    fn autosave(&mut self) { if self.ed.preedit().is_some() { return }   // a composition's text is not the note's yet
        if let Some(st) = self.store.as_mut() { if st.edited.take().is_some() { match st.col.save_note(st.note, &self.ed.doc) { Ok(()) => { st.unpublished = true; st.edited_since_stage = true; self.changed.insert(st.note); } Err(e) => eprintln!("autosave: {e}") }
            if let Some(sb) = self.sidebar.as_mut() { sb.set_tree(&st.col); } } } self.title(); }
    /// Save (rows 285, 293): the note goes to the staged copy, a complete new file is made THERE, and a helper thread
    /// makes it the document (`share::put`) — the window never waits on the document's place, which may be a share
    /// that does not answer. If the document is not the one this session knows (someone else saved), their file is
    /// fetched, checked, merged, and the result is put instead. Nothing is written when the document is missing or
    /// cannot be reached; the work stays on this machine.
    fn save(&mut self) {
        if self.saving.is_some() { return }
        // an open canvas's last changes go into the collection first
        if let Some(cv) = self.files.canvas.as_mut() { cv.edit_end(true); } self.canvas_save(true);
        self.autosave();
        let Some(st) = self.store.as_mut() else { println!("save: this note has no file"); return };
        let Some(session) = st.session.as_ref() else { return };
        if self.saver.busy() { eprintln!("save: an earlier save has not come back from the document's place yet (it may be a share that does not answer). Your changes are kept on this machine."); return }
        let expect = match session.base_print() { Ok(p) => p, Err(e) => { eprintln!("save: {e}"); return } };
        self.status = None;
        self.saving = Some(Saving { attempt: 0, expect, to: None, merged: None, doubled: 0, started: Instant::now() });
        self.put_staged();
    }
    /// Stages the collection as it stands and sends it to the document's place, expecting `saving.expect` there.
    fn put_staged(&mut self) {
        let (Some(st), Some(sv)) = (self.store.as_mut(), self.saving.as_ref()) else { return };
        let Some(session) = st.session.as_ref() else { self.saving = None; return };
        let col = &st.col;
        let staged = match session.stage_publish(|tmp| col.publish(tmp).map_err(std::io::Error::other)) { Ok(p) => p, Err(e) => { eprintln!("save: {e}"); self.saving = None; return } };
        st.edited_since_stage = false;
        let (to, expect) = (sv.to.clone().unwrap_or_else(|| session.original().to_path_buf()), sv.expect);
        // the files added since the last save go beside the document FIRST, so the document never names a file that is
        // not there; a Save As to another folder carries the old folder's files along
        let (added, old) = (session.media_dir(), session.original().parent().map(|p| p.to_path_buf()));
        self.saver.start(move || {
            if let Some(side) = to.parent().map(|p| p.join(media::SIDE)) {
                // (the hidden folder of before row 333 first, so a file of the same name in the visible one is the one that stays)
                let also: Vec<std::path::PathBuf> = old.filter(|o| Some(o.as_path()) != to.parent()).map(|o| vec![o.join(media::OLD_SIDE), o.join(media::SIDE)]).unwrap_or_default();
                if let Err(e) = media::publish(&added, &side, &also) { return SaveStep::Put(Err(mn_store::share::PutError::Io(e))) } }
            SaveStep::Put(mn_store::share::put(&staged, &to, expect)) });
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// A step of the save came back from the helper thread.
    fn save_step(&mut self, step: SaveStep) {
        use mn_store::share::PutError;
        let (Some(st), Some(sv)) = (self.store.as_mut(), self.saving.as_mut()) else { return };
        let Some(session) = st.session.as_mut() else { self.saving = None; return };
        let place = session.original().display().to_string();
        match step {
            SaveStep::Put(Ok(_)) => {
                let to = sv.to.clone().unwrap_or_else(|| session.original().to_path_buf());
                if sv.to.is_some() { recent_add(&to); }
                // (the Qt app's quiet offer after a Save As: the files the collection still refers to elsewhere)
                let outside = if sv.to.is_some() { gather::descriptors(&st.col).iter().flat_map(|d| gather::sources(d)).filter(|(src, ..)| !src.as_str().is_some_and(|x| x.starts_with(".minnotes/"))).count() } else { 0 };
                if let Err(e) = session.published(&to) { eprintln!("save: the document was written, but the session could not record it: {e}"); }
                st.unpublished = st.edited_since_stage || st.edited.is_some();
                match &sv.merged { Some(m) => println!("saved {place} — merged with a save by someone else: {m}"), None => println!("saved {place}") }
                let merged = sv.merged.clone(); let doubled = sv.doubled;
                self.saving = None;
                // the footer says how it went; a dialog only when there is something to look at (both versions of a paragraph kept)
                self.status = Some((if merged.is_some() { "Saved \u{2014} merged with someone else\u{2019}s changes" } else { "Saved" }.to_string(), false));
                if !self.unpublished() { self.changed.clear(); }
                if outside > 0 { self.notice = Some((format!("Saved \u{2014} {outside} file{} referenced outside the collection: File \u{25b8} Collect Media\u{2026} brings them in", if outside == 1 { " is" } else { "s are" }), Instant::now() + std::time::Duration::from_secs(10))); }
                if std::mem::take(&mut self.close_after_save) && !self.unpublished() && doubled == 0 { self.proceed(); }
                else if doubled > 0 { use min_ui::text_button::Variant as V;
                    let body = if doubled > 0 { format!("Someone else saved this document while you were working. Their changes have been merged with yours. {doubled} paragraph(s) were changed by both of you: both versions are kept, the second marked, for you to choose between.") } else { "Someone else saved this document while you were working. Their changes have been merged with yours.".to_string() };
                    self.ask(Ask::Notice, "The document was merged", &body, vec![("OK", V::Primary)]); }
            }
            // not the document we know: someone saved (or it is gone) — look at what is there
            SaveStep::Put(Err(PutError::Raced)) if sv.attempt < 3 && sv.to.is_none() => {
                sv.attempt += 1;
                let (from, to) = (session.original().to_path_buf(), session.theirs_path());
                self.saver.start(move || SaveStep::Fetched(mn_store::share::fetch(&from, &to)));
            }
            SaveStep::Put(Err(e)) => {
                let why = match &e { PutError::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied => "its folder cannot be written to".to_string(), PutError::Raced => "it keeps being saved by someone else".to_string(), other => other.to_string() };
                eprintln!("save: {place} was NOT saved — {why}. Your changes are kept on this machine."); self.saving = None;
                self.save_failed(&format!("The document couldn\u{2019}t be written to its location: {why}."));
            }
            SaveStep::Fetched(Err(e)) => {
                let why = if e.kind() == std::io::ErrorKind::NotFound { "it is no longer there — moved or deleted, or its drive or share is not connected".to_string() } else { format!("it cannot be reached ({e})") };
                eprintln!("save: {place} was NOT saved — {why}. Nothing was written; your changes are kept on this machine."); self.saving = None;
                self.save_failed(&format!("The document was not saved: {why}. Nothing was written."));
            }
            SaveStep::Fetched(Ok(print)) => {
                // their file, on this machine now: checked by opening it, merged into the staged copy (row 285), and sent
                let (base, theirs) = (session.base(), session.theirs_path());
                match st.col.merge_from(base.as_deref(), &theirs) {
                    Ok(report) => {
                        sv.merged = Some(format!("{} note(s) changed here, {} with cases to look at{}", report.changed.len(), report.notes.len(), if report.tree.is_empty() { String::new() } else { format!(", tree {:?}", report.tree) }));
                        for (id, cases) in &report.notes { println!("  note {id}: {cases:?}"); }
                        sv.doubled += report.notes.iter().flat_map(|(_, c)| c).filter(|c| matches!(c, mn_doc::Note::BothEdited { .. })).count();
                        sv.expect = Some(print);
                        for id in &report.changed { self.others.remove(id); }
                        // the open canvas was merged with someone else's: it is read again (its history starts over)
                        if self.files.canvas.as_ref().is_some_and(|c| report.changed.contains(&c.id)) { self.files.creload = true; } self.files.cards.clear();
                        if let Some(sb) = self.sidebar.as_mut() { sb.set_tree(&st.col); }
                        if report.changed.contains(&st.note) { if let Ok(doc) = st.col.load_note(st.note) { self.ed = Editor::new(doc); self.layout = None; self.flat = None; } }   // the history starts over after a merge
                        self.put_staged();
                    }
                    Err(e) => { eprintln!("save: {place} was NOT saved — what is there could not be read as a collection ({e}). Nothing was written; your changes are kept on this machine."); self.saving = None;
                        self.save_failed(&format!("Someone else saved this document, but what is there now could not be read ({e}). Nothing was written.")); }
                }
            }
        }
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    // ── questions and warnings (row 293)
    fn ask(&mut self, what: Ask, title: &str, body: &str, buttons: Vec<(&str, min_ui::text_button::Variant)>) {
        let mut d = min_ui::dialog::Dialog::new(title, body, buttons); d.show();
        self.confirm = Some((what, d)); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The document's file name for a dialog: a very long one is shortened in the middle (its start and its end, with
    /// the extension, are what tell documents apart); the dialog's title wraps what is left.
    fn doc_name(&self) -> String {
        let name = self.store.as_ref().and_then(|st| st.session.as_ref()).and_then(|s| s.original().file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "this document".into());
        let c: Vec<char> = name.chars().collect();
        if c.len() <= 60 { name } else { format!("{}\u{2026}{}", c[..36].iter().collect::<String>(), c[c.len() - 20..].iter().collect::<String>()) }
    }
    /// A dialog or the copy's window is over the page: nothing under it takes events.
    fn modal(&self) -> bool { self.confirm.is_some() || self.files.gather.as_ref().is_some_and(|w| w.shown) }
    fn unpublished(&self) -> bool { self.store.as_ref().is_some_and(|st| st.unpublished || st.edited.is_some()) }
    /// The collection is to be closed — for quitting, for another collection, or just closed. With changes that are not
    /// in the document, the Qt app's question is put first.
    fn request_close(&mut self, what: Pending) {
        use min_ui::text_button::Variant as V;
        if self.confirm.as_ref().is_some_and(|(a, _)| *a == Ask::Close) { return }
        // a package file is not opened as it is: it is unpacked to a folder first, and that is opened
        if let Pending::Open(p) = &what { if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(gather::PACKAGE)) { let p = p.clone(); self.unpack_open(p, None); return } }
        // a copy under way is not walked away from: it is stopped first, or waited for
        if self.files.gather.as_ref().is_some_and(|w| w.active()) { if let Some(w) = self.files.gather.as_mut() { w.shown = true; } self.notice = Some(("Files are still being copied \u{2014} stop the copy or let it finish first".into(), Instant::now() + std::time::Duration::from_secs(8))); if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        self.after_close = Some(what);
        self.canvas_save(true);   // what was moved or drawn on the open canvas counts as a change to ask about
        if self.unpublished() {
            let name = self.doc_name();
            self.ask(Ask::Close, &format!("Save changes to \u{201c}{name}\u{201d}?"), "Your changes will be lost if you don\u{2019}t save them.", vec![("Cancel", V::Secondary), ("Don\u{2019}t Save", V::Secondary), ("Save", V::Primary)]);
        } else { self.proceed(); }
    }
    /// Nothing stands in the way of closing the collection any more: do what it was closed for.
    fn proceed(&mut self) {
        match self.after_close.take() {
            None => {}
            Some(Pending::Quit) => self.quit = true,
            Some(Pending::Close) => self.end_session(),
            Some(Pending::Open(p)) => { self.end_session(); self.open_collection(&p, false); }
            Some(Pending::New(p)) => { self.end_session(); self.open_collection(&p, true); self.save(); }   // a new collection is written at once: its file exists from the start
        }
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The collection leaves the window: a session with nothing unpublished is removed; the window is blank.
    fn end_session(&mut self) {
        // the open canvas and what plays go with the collection (they were left over the next one's page); leaving a
        // card stages its note again, which must not turn a session with nothing unpublished into one that is kept
        let clean = !self.unpublished(); self.video_stop(); self.canvas_leave(); self.find_close();
        self.files.link = None; self.files.picker = None; self.files.picker_chip = None; self.files.cnotes.clear(); self.files.canvas_at.set(None); self.files.creload = false; self.files.import = None; self.files.import_wait.clear(); self.files.import_big = None;
        if clean { self.changed.clear(); if let Some(st) = self.store.as_mut() { st.edited = None; st.unpublished = false; } }
        self.autosave(); self.gather_drop();
        if let Some(st) = self.store.as_mut() { if let Some(s) = st.session.take() { if st.unpublished { drop(s) } else { let _ = s.close(); } } }
        self.store = None; self.sidebar = None; self.others.clear(); self.changed.clear(); self.status = None; self.ed = Editor::new(Doc::new()); self.layout = None; self.flat = None; self.scroll = 0.0; self.blank = true; self.side_w = 0.0;
        #[cfg(target_os = "macos")] { text_element::replaced(); self.geom_moved = Some(Instant::now()); }
    }
    /// Opens (or, with `create`, starts) a collection in this window. A failure is said in a dialog.
    fn open_collection(&mut self, path: &std::path::Path, create: bool) {
        match open_store(path, create, None) {
            Ok((store, doc, recovered)) => {
                let mut sb = sidebar::Sidebar::new(); sb.set_tree(&store.col);
                self.store = Some(store); self.sidebar = Some(sb); self.ed = Editor::new(doc); self.others.clear();
                self.layout = None; self.flat = None; self.scroll = 0.0; self.blank = false; self.recovered = recovered && std::env::var_os("MN_NO_DIALOGS").is_none();
                recent_add(path); self.sync_recent();
                #[cfg(target_os = "macos")] { text_element::replaced(); self.geom_moved = Some(Instant::now()); }
            }
            Err(e) => { use min_ui::text_button::Variant as V; let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(); self.ask(Ask::Notice, &format!("Couldn\u{2019}t open \u{201c}{name}\u{201d}"), &e, vec![("OK", V::Primary)]); }
        }
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// A system file panel (row 328's fix): asked for on a thread of its own, answered later. The panel must NOT be run
    /// from inside an event of the window's loop - it runs a loop of its own, and winit panics when a wake-up from
    /// another thread (a decoder, the notes' poll) arrives in it while the event that opened it is still being handled.
    /// From a thread the panel is put up by the main queue, outside any event; its answer comes back with a wake-up and
    /// `then` runs with the chosen paths (not at all when the panel was cancelled). `kind`: 0 save, 1 one file, 2 files.
    fn panel(&mut self, dlg: rfd::FileDialog, kind: u8, then: impl FnOnce(&mut App, Vec<std::path::PathBuf>) + 'static) {
        if std::env::var_os("MN_NO_DIALOGS").is_some() || self.files.panel_then.is_some() { return }
        self.files.panel_then = Some(Box::new(then));
        std::thread::spawn(move || { let got = match kind { 0 => dlg.save_file().into_iter().collect(), 1 => dlg.pick_file().into_iter().collect(), _ => dlg.pick_files().unwrap_or_default() };
            *PANEL_DONE.lock().unwrap() = Some(got); if let Some(w) = WAKE.get() { w.wake_up(); } });
    }
    fn sync_export_checks(&mut self) { let o = *self.files.xopts.get_or_insert_with(export_settings); #[cfg(target_os = "macos")] if let Some(m) = self.menu.as_ref() { m.set_check(&menu::Cmd::ExportPaths, o.paths); m.set_check(&menu::Cmd::ExportVideoNotes, o.video_notes); } let _ = o; }
    fn export_toggle(&mut self, paths: bool) { let mut o = *self.files.xopts.get_or_insert_with(export_settings); if paths { o.paths = !o.paths; } else { o.video_notes = !o.video_notes; } self.files.xopts = Some(o); save_export_settings(o); self.sync_export_checks(); }
    /// The open note's name, as the sidebar has it.
    fn note_title(&self) -> String { self.store.as_ref().map(|st| self.sidebar.as_ref().and_then(|sb| sb.row(st.note)).map(|r| r.title.clone()).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Untitled".into())).unwrap_or_default() }
    /// The open note written out (row 328): HTML - one file - or Markdown with its pictures in a folder beside it.
    /// `to`: where (a scripted check); otherwise the system's panel asks.
    fn export(&mut self, fmt: u8, to: Option<std::path::PathBuf>) {
        let md = fmt == 1;
        let soon = |s: u64| Instant::now() + std::time::Duration::from_secs(s);
        // a canvas (no card of it entered) goes out whole: as HTML, or as a poster
        let on_canvas = self.files.canvas.is_some() && self.files.card.is_none();
        if on_canvas && !matches!(fmt, 0 | 4) { self.notice = Some(("A canvas exports as HTML or as a poster".into(), soon(6))); return }
        if !on_canvas && fmt == 4 { if self.store.is_some() { self.notice = Some(("Open a canvas to make its poster".into(), soon(5))); } return }
        if (self.blank && !on_canvas) || self.store.is_none() { if self.store.is_some() { self.notice = Some(("Open a note to export it".into(), soon(5))); } return }
        let title = if on_canvas { self.files.canvas.as_ref().and_then(|cv| self.sidebar.as_ref().and_then(|sb| sb.row(cv.id)).map(|r| r.title.clone())).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Canvas".into()) } else { self.note_title() };
        let (ext, kind) = match fmt { 1 => ("md", "Markdown"), 2 => ("docx", "Word Document"), 3 => ("pdf", "PDF"), 4 => ("png", "PNG"), _ => ("html", "HTML") };
        let safe: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\n' | '\r') { '-' } else { c }).collect();
        let Some(path) = to else { let dlg = rfd::FileDialog::new().set_title(format!("Export as {kind}")).add_filter(kind, &[ext]).set_file_name(format!("{safe}{}.{ext}", if fmt == 4 { "-poster" } else if on_canvas { "-canvas" } else { "" })); self.panel(dlg, 0, move |me, p| me.export(fmt, p.into_iter().next())); return };
        let opts = *self.files.xopts.get_or_insert_with(export_settings);
        let gpu = self.gpu.as_ref().map(|g| (g.presenter.engine.device.clone(), g.presenter.engine.queue.clone()));
        let mut src = export::Files { media: &mut self.media, poster: &mut self.poster, gpu }; let doc = &self.ed.doc;
        let when = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M").to_string();
        let text: Result<Vec<u8>, String> = if on_canvas {
            if let Some(cv) = self.files.canvas.as_mut() { cv.edit_end(true); }
            let (cv, store, sidebar) = (self.files.canvas.as_ref().expect("a canvas"), self.store.as_ref(), self.sidebar.as_ref()); let els = cv.ordered();
            let mut note = |id: BlockId| { let st = store?; let doc = st.col.load_note(id).ok()?; Some((sidebar.and_then(|sb| sb.row(id)).map(|r| r.title.clone()).unwrap_or_default(), doc)) };
            if fmt == 4 { export::canvas_poster(&els, &mut src, &mut note) } else { Ok(export::canvas_html(&els, &title, &when, opts, &export::fonts(), &mut src, &mut note).into_bytes()) }
        } else if fmt == 2 { export::docx(doc.walk(), doc.children(None), &title, opts, &mut src) } else if fmt == 3 { export::pdf(doc, &title, &style(), opts, metric_paper(), &mut src) } else if md {
            // pictures go to `<name>.assets/` beside the file, each under its own name (a second of a name gets a number)
            let dir = path.with_extension("assets"); let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "assets".into());
            let mut used = std::collections::HashSet::new();
            let mut sink = |name: &str, p: &export::Pic| -> Option<String> {
                let stem: String = name.rsplit_once('.').map(|x| x.0).unwrap_or(name).chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect(); let stem = if stem.is_empty() { "picture".to_string() } else { stem };
                let mut file = format!("{stem}.{}", p.ext); let mut n = 2; while !used.insert(file.to_lowercase()) { file = format!("{stem}-{n}.{}", p.ext); n += 1; }
                std::fs::create_dir_all(&dir).ok()?; std::fs::write(dir.join(&file), &p.bytes).ok()?; Some(format!("{folder}/{file}")) };
            Ok(export::markdown(doc.walk(), doc.children(None), &title, opts, &mut src, Some(&mut sink)).into_bytes())
        } else { Ok(export::html(doc.walk(), doc.children(None), &title, &when, opts, &export::fonts(), &mut src).into_bytes()) };
        // written beside and moved into place: a reader never sees half a file
        let tmp = path.with_extension(format!("{ext}.part"));
        let done = text.map_err(std::io::Error::other).and_then(|bytes| std::fs::write(&tmp, bytes)).and_then(|_| std::fs::rename(&tmp, &path));
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.notice = Some(match done { Ok(()) => (format!("Exported {name}"), soon(6)), Err(e) => { let _ = std::fs::remove_file(&tmp); (format!("Export failed \u{2014} {e}"), soon(12)) } });
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// Shift+Cmd+C: the selected blocks - the whole note when none are - on the clipboard as Markdown. Nothing is written
    /// anywhere: a picture is named where it is.
    fn copy_markdown(&mut self) {
        if self.blank || self.store.is_none() { return }
        let opts = *self.files.xopts.get_or_insert_with(export_settings); let title = self.note_title();
        let mut src = export::Files { media: &mut self.media, poster: &mut self.poster, gpu: None };
        let (text, what) = match self.ed.copy_blocks() {
            Some(clip) => { let tops: Vec<BlockId> = clip.iter().filter_map(|t| t.first().map(|b| b.id)).collect(); (export::markdown(clip.iter().flatten(), &tops, "", opts, &mut src, None), if tops.len() == 1 { "block".to_string() } else { format!("{} blocks", tops.len()) }) }
            None => (export::markdown(self.ed.doc.walk(), self.ed.doc.children(None), &title, opts, &mut src, None), "note".to_string()) };
        if let Some(cb) = self.clipboard.as_mut() { if cb.set_text(text).is_ok() { self.block_clip = None; self.notice = Some((format!("Copied the {what} as Markdown"), Instant::now() + std::time::Duration::from_secs(5))); } }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    // ── Collect Media and the package (row 332)
    /// The window is opened on a look at the collection's files. Collect Media copies into the collection's own
    /// folder; the package asks for a folder (`to`: given by a scripted check) and takes a copy of the collection as
    /// it stands now. While a copy runs the same window is shown again instead.
    fn gather_open(&mut self, what: gather::What, to: Option<std::path::PathBuf>) {
        let soon = |s: u64| Instant::now() + std::time::Duration::from_secs(s);
        if let Some(w) = self.files.gather.as_mut() { if w.active() { w.shown = true; if w.what != what { self.notice = Some(("A copy is under way \u{2014} one at a time".into(), soon(6))); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
        if self.store.is_none() { return }
        if what == gather::What::Package && to.is_none() {
            let stem = self.store.as_ref().and_then(|st| st.session.as_ref()).and_then(|x| x.original().file_stem().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "Collection".into());
            self.panel(rfd::FileDialog::new().set_title("Export Collection as Package").add_filter("minNotes package", &[gather::PACKAGE]).set_file_name(format!("{stem}.{}", gather::PACKAGE)), 0, move |me, p| if let Some(p) = p.into_iter().next() { me.gather_open(gather::What::Package, Some(p)) }); return
        }
        // everything typed and placed goes into the collection first: the look is at the collection as it stands
        if let Some(cv) = self.files.canvas.as_mut() { cv.edit_end(true); } self.canvas_save(true); self.autosave(); self.gather_drop();
        let Some(st) = self.store.as_ref() else { return }; let Some(session) = st.session.as_ref() else { return };
        let Some(base) = session.original().parent().map(|p| p.to_path_buf()) else { return };
        let doc_name = session.original().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Collection.mnd".into());
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| { if let Some(p) = WAKE.get() { p.wake_up(); } });
        let recs = gather::descriptors(&st.col);
        let win = match (what, to) {
            (gather::What::Package, Some(given)) => {
                // one file (the name ends in .mnpkg) or a folder; the window's switch can still change it
                let one = given.extension().is_some_and(|x| x.eq_ignore_ascii_case(gather::PACKAGE)); let folder = if one { given.with_extension("") } else { given.clone() };
                let archive = if one { given } else { let mut n = folder.clone().into_os_string(); n.push(format!(".{}", gather::PACKAGE)); n.into() };
                if !one && folder.is_file() { self.notice = Some(("A file has that name \u{2014} the package is a folder: choose another name".into(), soon(8))); return }
                if folder == base { self.notice = Some(("That is the collection\u{2019}s own folder \u{2014} choose another place for the package".into(), soon(8))); return }
                let snap = sessions_root().join(format!("package-{}-{}.mnd", std::process::id(), self.now_ms()));
                if let Err(e) = st.col.publish(&snap) { self.notice = Some((format!("The package could not be started \u{2014} {e}"), soon(10))); return }
                let mut w = gather_win::Window::open(what, recs, self.media.places(), folder.join(media::SIDE), wake); w.folder = Some(folder); w.snap = Some(snap); w.doc_name = doc_name; w.archive = Some(archive); w.one = min_ui::Switch::new(one); w
            }
            _ => gather_win::Window::open(what, recs, self.media.places(), base.join(media::SIDE), wake),
        };
        self.files.gather = Some(win); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The window goes (a copy under way is told to stop); a package's copy of the collection is cleared away.
    fn gather_drop(&mut self) {
        let Some(w) = self.files.gather.take() else { return }; if let Some(r) = &w.run { r.stop.store(true, std::sync::atomic::Ordering::Relaxed); }
        if let Some(snap) = &w.snap { let _ = std::fs::remove_file(snap.with_extension("out")); for ext in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(format!("{}{ext}", snap.display())); } }
    }
    fn gather_out(&mut self, out: gather_win::Out) {
        use gather_win::Out as O;
        match out {
            O::None => return,
            O::Redraw => {}
            O::Close => { let note = self.files.gather.as_ref().map(|w| w.note.clone()).filter(|n| !n.is_empty()); self.gather_drop(); if let Some(n) = note { self.notice = Some((n, Instant::now() + std::time::Duration::from_secs(8))); } }
            O::Relink(i) => self.panel(rfd::FileDialog::new().set_title("Locate File"), 1, move |me, p| if let (Some(p), Some(w)) = (p.into_iter().next(), me.files.gather.as_mut()) { w.relinked(i, p); }),
            O::Reveal => if let Some(w) = self.files.gather.as_ref() { match (&w.folder, w.what) { _ if w.run.as_ref().is_some_and(|r| r.pack.is_some()) => { if let Some(p) = w.run.as_ref().and_then(|r| r.pack.as_ref()) { open::reveal(&p.path) } } (Some(f), _) => open::reveal(&f.join(&w.doc_name)), _ => { let first = w.run.as_ref().and_then(|r| r.jobs.iter().enumerate().find(|(i, j)| j.is_some() && matches!(r.state(*i), gather::DONE | gather::THERE)).and_then(|(_, j)| j.as_ref().map(|j| j.to.clone()))); open::reveal(&first.unwrap_or_else(|| w.dest.clone())) } } },
            O::Discard => { if let Some(w) = self.files.gather.as_ref() { if let Some(f) = &w.folder { let _ = std::fs::remove_dir(f.join(media::SIDE).join(".qcview")); let _ = std::fs::remove_dir(f.join(media::SIDE)); let _ = std::fs::remove_dir(f); } } }
            O::Commit => { let err = self.gather_commit().err(); if let Some(e) = &err { eprintln!("gather: {e}"); } if let Some(w) = self.files.gather.as_mut() { w.committed(err); println!("gather: {}", w.note); } }
        }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The copying is over: the references are pointed at the copies. Collect Media changes the open collection -
    /// the open note, the notes held in memory and the open canvas each as ONE step of their own history (an undo
    /// there takes it back), every other note and canvas in the collection directly - and saves it, so the document
    /// and the files beside it cannot part. The package changes only its own copy of the collection, which is then
    /// written into the package's folder as its document.
    fn gather_commit(&mut self) -> Result<(), String> {
        let Some(w) = self.files.gather.as_ref() else { return Ok(()) }; let map = w.map();
        if w.what == gather::What::Package {
            let (Some(folder), Some(snap)) = (w.folder.clone(), w.snap.clone()) else { return Err("the package has no folder".into()) };
            let mut col = mn_store::Collection::open(&snap).map_err(|e| e.to_string())?;
            gather::rewrite_collection(&mut col, &map, &Default::default())?;
            // the package's document, made on this machine first: it goes into the one file, or into the folder
            let out = snap.with_extension("out"); let _ = std::fs::remove_file(&out); col.publish(&out).map_err(|e| e.to_string())?;
            if let Some(run) = w.run.as_ref().filter(|r| r.pack.is_some()) { let r = run.seal(&out, &w.doc_name); let _ = std::fs::remove_file(&out); return r }
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let (part, to) = (folder.join(format!(".{}.part", w.doc_name)), folder.join(&w.doc_name)); let _ = std::fs::remove_file(&part);
            std::fs::copy(&out, &part).map_err(|e| e.to_string())?; std::fs::rename(&part, &to).map_err(|e| e.to_string())?; let _ = std::fs::remove_file(&out);
            return Ok(())
        }
        if map.is_empty() { return Ok(()) }
        if let Some(cv) = self.files.canvas.as_mut() { cv.edit_end(true); } self.canvas_save(true); self.autosave();
        let now = self.now_ms(); let mut skip = std::collections::HashSet::new();
        let ch = gather::rewritten(&self.ed.doc, &map);
        if !ch.is_empty() { match self.ed.set_kinds(ch, now) { Ok(d) => { self.apply(d); self.layout = None; } Err(e) => return Err(format!("{e:?}")) } self.autosave(); }
        let Some(st) = self.store.as_mut() else { return Ok(()) }; skip.insert(st.note);
        for (id, ed) in self.others.iter_mut() { skip.insert(*id); let ch = gather::rewritten(&ed.doc, &map); if ch.is_empty() { continue }
            ed.set_kinds(ch, now).map_err(|e| format!("{e:?}"))?; st.col.save_note(*id, &ed.doc).map_err(|e| e.to_string())?; self.changed.insert(*id); }
        if let Some(cv) = self.files.canvas.as_mut() { skip.insert(cv.id); if cv.set_records(|r| gather::rewrite(r, &map)) > 0 { cv.dirty = true; } }
        gather::rewrite_collection(&mut st.col, &map, &skip)?;
        st.unpublished = true; st.edited_since_stage = true; self.files.cards.clear();
        self.canvas_save(true); self.save();
        Ok(())
    }
    /// A scripted check (`MN_GATHER=collect` or `package:<folder>`): the whole of it, waited for, as the run ends.
    fn gather_now(&mut self, spec: &str) {
        let (what, to) = match spec.split_once(':') { Some(("package", f)) => (gather::What::Package, Some(std::path::PathBuf::from(f))), _ => (gather::What::Collect, None) };
        self.gather_open(what, to); let t = Instant::now(); let mut over = false;
        while !over && t.elapsed().as_secs() < 300 {
            let Some(w) = self.files.gather.as_mut() else { break }; let out = w.tick(); if w.phase == gather_win::Phase::Review { if !w.go() { println!("gather: nothing to do"); break } }
            over = matches!(out, gather_win::Out::Commit | gather_win::Out::Discard); self.gather_out(out); if !over { std::thread::sleep(std::time::Duration::from_millis(5)); }
        }
        if let Some(w) = self.files.gather.as_ref() { for (i, a) in w.assets.iter().enumerate() { println!("gather:   {:?} {} -> {} [{}]", a.kind, a.name, a.dest, match w.run.as_ref().map(|r| r.state(i)) { Some(gather::DONE) => "copied".to_string(), Some(gather::THERE) => "already there".to_string(), Some(gather::FAILED) => format!("failed: {}", w.run.as_ref().map(|r| r.slots[i].why.lock().unwrap().clone()).unwrap_or_default()), _ => a.problem.clone().unwrap_or_else(|| "not copied".into()) }); } }
    }
    // ── a file of another application becomes a note (row 335)
    /// These files are imported, one after another: each a new note beside the selected entry.
    fn import_files(&mut self, paths: Vec<std::path::PathBuf>) {
        if self.store.is_none() { return }
        let (known, other): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| import::family(p).is_some());
        if let Some(p) = other.first() { self.notice = Some((format!("{} is not a kind of file minNotes imports", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), Instant::now() + std::time::Duration::from_secs(8))); }
        self.files.import_wait.extend(known); self.import_next();
    }
    /// The next waiting file is read, off this thread (nothing is read while one is, or while its question is open).
    fn import_next(&mut self) {
        if self.files.import.is_some() || self.files.import_big.is_some() || self.files.import_wait.is_empty() { return }
        if self.store.is_none() { self.files.import_wait.clear(); return }
        let path = self.files.import_wait.remove(0); let (tx, rx) = std::sync::mpsc::channel(); let (places, from) = (self.media.places(), path.clone());
        std::thread::spawn(move || { let r = import::read(&from, &places); let _ = tx.send(r); if let Some(w) = WAKE.get() { w.wake_up(); } });
        self.files.import = Some(Importing { rx, path }); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn import_tick(&mut self) {
        let Some(i) = self.files.import.as_ref() else { return };
        let got = match i.rx.try_recv() { Ok(r) => r, Err(std::sync::mpsc::TryRecvError::Empty) => return, Err(_) => Err("The reader stopped.".into()) };
        let path = self.files.import.take().map(|i| i.path).unwrap_or_default(); let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        use min_ui::text_button::Variant as V;
        match got {
            Err(e) => { eprintln!("import: {name}: {e}"); self.ask(Ask::Notice, &format!("Couldn\u{2019}t import \u{201c}{name}\u{201d}"), &e, vec![("OK", V::Primary)]); }
            Ok(done) => match done.big {
                Some((rows, cells)) => { let keep = import::rows_that_fit(cells / rows.max(1));
                    let body = format!("\u{201c}{name}\u{201d} has a table of {rows} rows and {cells} cells. Notes are for tables you read and edit; big data stays in spreadsheets. Import the first {keep} rows, or attach the file to open it in its own app.");
                    self.files.import_big = Some((done, path));
                    match std::env::var("MN_IMPORT_BIG").as_deref() { Ok("first") => self.import_big(2), Ok("attach") => self.import_big(1), Ok("cancel") => self.import_big(0),
                        _ => self.ask(Ask::ImportBig, "This table is very large", &body, vec![("Cancel", V::Secondary), ("Attach File", V::Secondary), ("First Rows", V::Primary)]) } }
                None => self.import_land(done),
            },
        }
        self.import_next(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The large table's question was answered: 0 nothing, 1 the file as a chip in a note of its name, 2 its first rows.
    fn import_big(&mut self, answer: usize) {
        let Some((mut done, path)) = self.files.import_big.take() else { return };
        match answer {
            2 => { import::cut(&mut done.clip); done.big = None; self.import_land(done); }
            1 => { let rec = self.media.file_record(&path); done.clip = vec![vec![mn_doc::Block { id: BlockId::new(), parent: None, rank: mn_doc::Rank::first(), kind: Kind::Media { descriptor: rec }, text: String::new(), spans: Spans::new(), conflict: None, children: vec![] }]]; done.lost = 0; self.import_land(done); }
            _ => {}
        }
        self.import_next();
    }
    /// What was read becomes a note beside the selected entry (inside it, when that is a folder), named after the file, and is opened.
    fn import_land(&mut self, done: import::Done) {
        let (Some(st), Some(sb)) = (self.store.as_mut(), self.sidebar.as_mut()) else { return };
        let parent = sb.selected.or(Some(st.note)).and_then(|s| sb.row(s)).and_then(|r| if r.kind == mn_store::DocKind::Folder { Some(r.id) } else { r.parent });
        let made = st.col.create(parent, mn_store::DocKind::Note, &done.title).and_then(|id| st.col.save_note(id, &import::doc_of(&done.clip)).map(|_| id));
        let id = match made { Ok(id) => id, Err(e) => { eprintln!("import: {e}"); use min_ui::text_button::Variant as V; self.ask(Ask::Notice, &format!("Couldn\u{2019}t import \u{201c}{}\u{201d}", done.title), &e.to_string(), vec![("OK", V::Primary)]); return } };
        st.unpublished = true; st.edited_since_stage = true; sb.set_tree(&st.col); sb.selected = Some(id); sb.focused = false; self.changed.insert(id);
        println!("imported: {} ({} blocks, {} pictures not found)", done.title, done.clip.iter().map(|t| t.len()).sum::<usize>(), done.lost);
        self.notice = Some((match done.lost { 0 => format!("Imported {}", done.title), 1 => format!("Imported {} \u{2014} 1 picture was not found", done.title), n => format!("Imported {} \u{2014} {n} pictures were not found", done.title) }, Instant::now() + std::time::Duration::from_secs(8)));
        self.open_note(id);
    }

    // ── a package file is opened: unpacked to a folder, which is then the collection (row 334)
    /// `to`: the folder (None: the system's panel asks, offering the package's name).
    fn unpack_open(&mut self, archive: std::path::PathBuf, to: Option<std::path::PathBuf>) {
        if self.files.unpack.is_some() { return }
        let Some(folder) = to else { let stem = archive.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Package".into());
            let mut dlg = rfd::FileDialog::new().set_title("Unpack Package To").set_file_name(stem); if let Some(d) = archive.parent() { dlg = dlg.set_directory(d); }
            self.panel(dlg, 0, move |me, p| if let Some(p) = p.into_iter().next() { me.unpack_open(archive, Some(p)) }); return };
        if folder.is_file() { self.notice = Some(("A file has that name \u{2014} the package unpacks to a folder: choose another name".into(), Instant::now() + std::time::Duration::from_secs(8))); return }
        let (done, stop) = (Arc::new(std::sync::atomic::AtomicU64::new(0)), Arc::new(std::sync::atomic::AtomicBool::new(false))); let (tx, rx) = std::sync::mpsc::channel();
        let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        { let (done, stop) = (done.clone(), stop.clone()); std::thread::spawn(move || { let wake = || { if let Some(p) = WAKE.get() { p.wake_up(); } };
            let total = gather::unpacked_size(&archive); let _ = tx.send((total, None)); let r = gather::unpack(&archive, &folder, &done, &stop, &wake); let _ = tx.send((total, Some(r))); wake(); }); }
        self.files.unpack = Some(Unpack { rx, done, stop, total: 0, name, drawn: Instant::now() }); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn unpack_tick(&mut self) {
        let Some(u) = self.files.unpack.as_mut() else { return }; let mut end = None;
        while let Ok((total, r)) = u.rx.try_recv() { u.total = total; if r.is_some() { end = r; } }
        match end {
            None => if u.drawn.elapsed() >= std::time::Duration::from_millis(100) { u.drawn = Instant::now(); if let Some(g) = &self.gpu { g.window.request_redraw(); } },
            Some(Ok(doc)) => { self.files.unpack = None; println!("unpacked: {}", doc.display()); self.notice = Some(("The package was unpacked".into(), Instant::now() + std::time::Duration::from_secs(6))); self.request_close(Pending::Open(doc)); }
            Some(Err(e)) => { let name = u.name.clone(); self.files.unpack = None; eprintln!("unpack: {e}"); use min_ui::text_button::Variant as V; self.ask(Ask::Notice, &format!("Couldn\u{2019}t unpack \u{201c}{name}\u{201d}"), &e, vec![("OK", V::Primary)]); }
        }
    }
    fn sync_recent(&mut self) { #[cfg(target_os = "macos")] if let Some(m) = self.menu.as_mut() { m.set_recent(&recent_list()); } }
    /// Save As: the collection is published at a place the user picks, which becomes its document.
    fn save_as(&mut self) {
        if self.saving.is_some() || self.store.is_none() { return }
        let dlg = rfd::FileDialog::new().add_filter("minNotes collection", &["mnd"]).set_file_name(self.doc_name());
        self.panel(dlg, 0, |me, p| if let Some(p) = p.into_iter().next() { me.save_as_to(p) });
    }
    fn save_as_to(&mut self, path: std::path::PathBuf) {
        if self.saving.is_some() || self.store.is_none() { return }
        self.autosave();
        // the system's dialog has asked about replacing a file that is there: that file is what is expected
        let expect = mn_store::share::fingerprint_if_present(&path).ok().flatten();
        self.saving = Some(Saving { attempt: 0, expect, to: Some(path), merged: None, doubled: 0, started: Instant::now() });
        self.put_staged();
    }
    /// The pointer's shape for where it is (row 302): the I-beam over the page and over a name being edited, the
    /// resize arrows on the sidebar's edge (kept while it is dragged), the arrow over everything else.
    fn set_cursor(&mut self) {
        use winit::cursor::CursorIcon as C;
        let Some(g) = self.gpu.as_ref() else { return };
        let (x, y) = self.pointer; let (w, h) = g.presenter.size();
        let over_ui = self.modal() || self.pal.is_open() || self.ctx.is_open() || self.files.picker.as_ref().is_some_and(|p| p.hit(x, y)) || self.files.link.as_ref().is_some_and(|p| p.hit(x, y)) || self.files.find.as_ref().is_some_and(|p| p.hit(x, y));
        let on_canvas = !over_ui && !self.files.card.as_ref().is_some_and(|c| Rect::new(c.rect.x0 - 40.0, c.rect.y0, c.rect.x1, c.rect.y1).contains(kurbo::Point::new(x, y))) && self.files.canvas.as_ref().is_some_and(|c| c.rect.contains(kurbo::Point::new(x, y))) && !self.bar.hit(x, y) && !self.foot.hit(x, y);
        let want = if self.files.row_drag.is_some_and(|d| d.2) { if self.files.canvas.as_ref().is_some_and(|c| c.rect.contains(kurbo::Point::new(x, y))) { C::Copy } else { C::Grabbing } }
            else if self.sidebar.as_ref().is_some_and(|sb| sb.rail.is_dragging() || (!over_ui && sb.rail.splitter_hovered())) { C::ColResize }
            else if on_canvas { match self.files.canvas.as_mut().map(|c| c.cursor(x, y)).unwrap_or(0) { 1 => if self.pressed { C::Grabbing } else { C::Grab }, 2 => C::NwseResize, 3 => C::NeswResize, 5 => C::Crosshair, _ => C::Default } }
            else if matches!(self.files.pick, Some(ink::Pick::Draw(_))) && !over_ui && self.files.iblock.as_ref().is_some_and(|b| b.1.contains(kurbo::Point::new(x, y))) { C::Crosshair }
            else if let Some(rz) = &self.files.resize { let _ = rz; C::EwResize }
            else if self.files.col_drag.is_some() || self.files.gap_drag.is_some() { C::ColResize }
            // a wide table's bar: the arrow, not the text pointer (while its thumb is dragged too, wherever the pointer goes)
            else if self.files.pbar.dragging() || (!over_ui && !self.blank && self.files.card.is_none() && self.files.pbar.hovered()) || self.files.tbars.values().any(|b| b.dragging() || (!over_ui && !self.blank && b.hovered())) { C::Default }
            else if self.grab.as_ref().is_some_and(|x| x.dragging) { C::Grabbing }
            else if over_ui || self.blank { C::Default }
            // Cmd held over a link: it would open
            else if self.mods.1 && self.link_under(x, y).is_some() { C::Pointer }
            else if self.grab.is_none() && !self.pressed && self.column_gap_at(x, y).is_some() { C::ColResize }
            else if let Some((_, _, left, top)) = self.corner_at(x, y) { if left == top { C::NwseResize } else { C::NeswResize } }
            else if self.table_ui().is_some_and(|ui| y >= ui.y.0 && y <= ui.y.1 && ui.borders.iter().any(|(_, bx)| (bx - x).abs() <= 4.0 * g.window.scale_factor())) { C::ColResize }
            else if self.table_ui().is_some_and(|ui| { let p = kurbo::Point::new(x, y); ui.row.contains(p) || ui.col.contains(p) || ui.add_row.contains(p) || ui.add_col.contains(p) }) { C::Default }
            else if self.grab.is_some() || self.handle_rect().is_some_and(|r| r.contains(kurbo::Point::new(x, y))) { C::Grab }
            else if self.sidebar.as_ref().is_some_and(|sb| sb.over_field(x, y)) { C::Text }
            else if self.sidebar.as_ref().is_some_and(|sb| sb.rail.hit(x, y)) || self.bar.hit(x, y) || self.foot.hit(x, y) { C::Default }
            else if x >= self.side_w && x < w as f64 && y >= self.top && y < h as f64 - self.bottom { C::Text }
            else { C::Default };
        if want != self.cursor { self.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); }
        if self.cursor_hidden { self.cursor_hidden = false; g.window.set_cursor_visible(true); }
    }
    /// The selected blocks (the caret's block with its children) go to the END of another note (row 307): moved, or
    /// with `copy` copied. The other note gets them as one undo step of its own; here they are removed as one step.
    fn blocks_to_note(&mut self, to: BlockId, copy: bool) {
        let now = self.now_ms();
        if !self.ed.in_blocks() && !self.ed.toggle_blocks() { return }
        let Some(clip) = self.ed.copy_blocks() else { return }; let n = self.ed.run_len();
        let Some(st) = self.store.as_mut() else { return }; let from = st.note; if to == from { return }
        let mut dest = match self.others.remove(&to) { Some(e) => e, None => match st.col.load_note(to) { Ok(d) => Editor::new(d), Err(e) => { eprintln!("move: {e}"); return } } };
        let refused = match dest.append_blocks(&clip, now) { Ok(r) => r, Err(e) => { eprintln!("move: {e:?}"); self.others.insert(to, dest); return } };
        // the other note is written to the staged copy first: the blocks exist there before they leave here
        if let Err(e) = st.col.save_note(to, &dest.doc) { eprintln!("move: {e}"); let _ = dest.undo(); self.others.insert(to, dest); return }
        st.unpublished = true; st.edited_since_stage = true; self.changed.insert(to);
        let to_stamp = dest.stamp(); self.others.insert(to, dest);
        let title = self.sidebar.as_ref().and_then(|sb| sb.row(to)).map(|r| r.title.clone()).unwrap_or_default();
        if let (Some(sb), Some(st)) = (self.sidebar.as_mut(), self.store.as_ref()) { sb.set_tree(&st.col); }
        let what = if n == 1 { "1 block".to_string() } else { format!("{n} blocks") };
        if copy { self.moved = None; self.notice = Some((format!("Copied {what} to \u{201c}{title}\u{201d}"), Instant::now() + std::time::Duration::from_secs(5))); self.apply(Dirty::Nothing); return }
        if refused > 0 { eprintln!("move: {refused} block(s) cannot go there; they stay where they were, and the others were copied, not moved"); self.moved = None; self.apply(Dirty::Nothing); return }
        let d = self.ed.delete_blocks(now + mn_doc::change::JOIN_MS + 1).unwrap_or(Dirty::Nothing);
        self.moved = Some(Moved { from, to, from_stamp: self.ed.stamp(), to_stamp, undone: false });
        self.notice = Some((format!("Moved {what} to \u{201c}{title}\u{201d} \u{2014} Undo takes them back"), Instant::now() + std::time::Duration::from_secs(5)));
        self.follow = true; self.apply(d);
    }
    /// Undo / redo in the note blocks were moved OUT of, while that move is still its last step and the other note has
    /// not been edited since: the other note's half goes with it, so one undo takes the whole move back.
    fn undo_redo(&mut self, redo: bool) -> Dirty {
        let here = self.store.as_ref().map(|st| st.note);
        let linked = self.moved.as_ref().is_some_and(|m| Some(m.from) == here && m.undone == redo && self.ed.stamp() == m.from_stamp && self.others.get(&m.to).is_some_and(|o| o.stamp() == m.to_stamp));
        let d = if redo { self.ed.redo() } else { self.ed.undo() }.unwrap_or(Dirty::Nothing);
        if linked { let to = self.moved.as_ref().unwrap().to;
            if let (Some(o), Some(st)) = (self.others.get_mut(&to), self.store.as_mut()) {
                let _ = if redo { o.redo() } else { o.undo() };
                match st.col.save_note(to, &o.doc) { Ok(()) => { st.unpublished = true; st.edited_since_stage = true; self.changed.insert(to); } Err(e) => eprintln!("undo: {e}") }
                let stamp = o.stamp(); if let Some(m) = self.moved.as_mut() { m.undone = !redo; m.from_stamp = self.ed.stamp(); m.to_stamp = stamp; }
                if let Some(sb) = self.sidebar.as_mut() { sb.set_tree(&st.col); }
                self.notice = Some((if redo { "The move was done again" } else { "The move was taken back" }.to_string(), Instant::now() + std::time::Duration::from_secs(4)));
            }
        }
        d
    }
    /// A chip of the toolbar was pressed: the palette opens under it for that role.
    fn open_colours(&mut self, role: mn_doc::MarkKind, tool: toolbar::Tool) {
        let Some(g) = self.gpu.as_mut() else { return };
        let st = self.ed.state(); let current = if role == mn_doc::MarkKind::Color { st.color } else { st.highlight };
        let customs = self.store.as_ref().map(|x| x.col.palette()).unwrap_or_default();
        let (w, h) = g.presenter.size();
        self.pal.open(role, self.bar.rect_of(tool), customs, current, &mut g.fonts, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), g.window.scale_factor());
    }
    /// What the palette answered: a colour for its role, a colour to add to the collection, one to remove from it.
    fn colour_out(&mut self, out: colours::Out) {
        use colours::Out as O;
        let now = self.now_ms(); let role = self.pal.role;
        // the palette was opened for the pen (a video's drawing colour): its answer goes there, not to the text
        if self.files.pal_pen {
            let chosen = match &out { colours::Out::Pick(v) => Some(v.clone()), colours::Out::Add(h) => Some(Some(h.clone())), _ => None };
            if let Some(v) = chosen { let c = v.as_deref().and_then(mn_layout::colour).map(|c| { let x = c.to_rgba8(); [x.r as f32 / 255.0, x.g as f32 / 255.0, x.b as f32 / 255.0, 1.0] }).unwrap_or([1.0, 0.0, 0.0, 1.0]);
                self.files.pen = Some(c); if let Some(p) = self.player.as_mut() { p.ann.color = c; } if let Some(cv) = self.files.canvas.as_mut() { cv.pen = c; cv.restyle(Some(c), None); } save_pen_settings(self.files.pen, self.files.pen_w); self.page_picked(|l| { l.s.color = qcv_notes::strokes::Stroke::qcolor(c); true }); self.pal.close(); self.files.pal_pen = false; if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
            if !self.pal.is_open() { self.files.pal_pen = false; }
            if matches!(out, colours::Out::None | colours::Out::Redraw) { return } }
        let pick = |me: &mut App, v: Option<String>| { me.pal.close(); if let (mn_doc::MarkKind::Highlight, Some(c)) = (role, &v) { me.last_bg = c.clone(); }
            let d = me.ed.set_colour(role, v.as_deref(), now).unwrap_or(Dirty::Nothing); me.apply(d); };
        match out {
            O::None | O::Redraw => {}
            O::Pick(v) => pick(self, v),
            O::Add(hex) => { if let Some(st) = self.store.as_mut() { match st.col.palette_add(&hex) { Ok(()) => st.unpublished = true, Err(e) => eprintln!("palette: {e}") } } pick(self, Some(hex)); }
            O::Remove(hex) => if let (Some(st), Some(g)) = (self.store.as_mut(), self.gpu.as_mut()) { match st.col.palette_remove(&hex) { Ok(()) => st.unpublished = true, Err(e) => eprintln!("palette: {e}") } self.pal.set_customs(st.col.palette(), &mut g.fonts); },
        }
    }
    /// A right press: over the tree it opens the context menu (row 300) on the row under it, which is selected.
    fn context(&mut self, x: f64, y: f64) {
        if self.modal() { return }
        use min_ui::menu::MenuItem as I;
        // over the page: the block under the pointer is selected (the selected blocks stay when it is one of them) and
        // the menu is theirs (row 306)
        let over_tree = self.sidebar.as_ref().is_some_and(|sb| sb.rail.hit(x, y));
        if !over_tree {
            if self.blank || self.bar.hit(x, y) || self.foot.hit(x, y) || x < self.side_w || y < self.top { return }
            let (nx, ny) = self.note_point((x, y));
            let Some(l) = self.layout.as_ref() else { return };
            let Some((hit, _)) = l.hit(&self.ed.doc, nx, ny) else { return };
            // on a link: the link's own menu (row 320); the caret goes into it
            if let Some(u) = self.link_under(x, y) { self.ed.point(l, nx, ny, false); let Some(g) = self.gpu.as_mut() else { return }; let (w, h) = g.presenter.size(); let s = g.window.scale_factor();
                self.files.tmenu = None; self.ctx_blocks = false; self.ctx_for = None; self.ctx_notes.clear();
                self.ctx.open_at(vec![I::action("Open Link", mn_edit::links::openable(&u)), I::action("Copy Link", true), I::Separator, I::action("Edit Link\u{2026}", true), I::action("Remove Link", true)], &mut g.fonts, x, y, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), s);
                self.files.lmenu = Some(u); g.window.request_redraw(); return }
            if !self.ed.in_block_selection(hit) { self.ed.point(l, nx, ny, false); if !self.ed.toggle_blocks() { return } }
            let Some(g) = self.gpu.as_mut() else { return };
            let mac = cfg!(target_os = "macos"); let k = |m: &str, w: &str| if mac { m.to_string() } else { w.to_string() };
            let items = vec![I::with_shortcut("Cut", true, &k("\u{2318}X", "Ctrl+X")), I::with_shortcut("Copy", true, &k("\u{2318}C", "Ctrl+C")), I::with_shortcut("Paste", true, &k("\u{2318}V", "Ctrl+V")), I::Separator,
                I::with_shortcut("Duplicate", true, &k("\u{2318}D", "Ctrl+D")), I::with_shortcut("Delete", true, "\u{232b}"), I::Separator,
                I::with_shortcut("Move Up", true, &k("\u{2303}\u{2318}\u{2191}", "")), I::with_shortcut("Move Down", true, &k("\u{2303}\u{2318}\u{2193}", "")), I::Separator, I::action("Move to Note\u{2026}", true), I::action("Copy to Note\u{2026}", true)];
            // a picture whose file was not found can be pointed at it again
            let lost = self.ed.block_selection().len() == 1 && matches!(self.ed.doc.get(hit).map(|b| &b.kind), Some(Kind::Media { descriptor }) if self.media.is_missing(descriptor));
            self.files.locate = lost.then_some(hit);
            let items = if lost { let mut v = vec![I::action("Locate File\u{2026}", true), I::Separator]; v.extend(items); v } else { items };
            // a file chip (row 320): its file is opened, shown in its folder, its path copied, pointed at again
            let chip = (self.ed.block_selection().len() == 1).then(|| match self.ed.doc.get(hit).map(|b| &b.kind) { Some(Kind::Media { descriptor }) if descriptor.contains("\"kind\":\"file\"") => Some(descriptor.clone()), _ => None }).flatten();
            let items = if let Some(d) = &chip { let st = self.media.file_state(d); self.files.locate = Some(hit);
                let mut v = vec![I::action("Open", st.found), I::action(if mac { "Reveal in Finder" } else { "Show in Explorer" }, st.found), I::action("Copy Path", st.path.is_some()), I::action("Locate File\u{2026}", true), I::Separator]; v.extend(items); v } else { items };
            // a picture given a width of its own can go back to its own size
            let sized = self.ed.block_selection().len() == 1 && matches!(self.ed.doc.get(hit).map(|b| &b.kind), Some(Kind::Media { descriptor }) if descriptor.contains("\"dw\""));
            if sized { self.files.locate = Some(hit); }
            let items = if sized { let mut v = vec![I::action("Original Size", true), I::Separator]; v.extend(items); v } else { items };
            let (w, h) = g.presenter.size(); let s = g.window.scale_factor();
            self.ctx_blocks = true; self.ctx_for = None; self.ctx_notes.clear();
            self.ctx.open_at(items, &mut g.fonts, x, y, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), s);
            #[cfg(target_os = "macos")] text_element::changed(false);
            g.window.request_redraw(); return
        }
        self.ctx_blocks = false;
        let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) else { return };
        let key = |k: &str| if cfg!(target_os = "macos") { format!("{k}\u{2318}N") } else { format!("Ctrl+{}N", if k.is_empty() { "" } else { "Shift+" }) };
        self.ctx_for = sb.id_at(x, y);
        if let Some(id) = self.ctx_for { sb.selected = Some(id); sb.focused = true; }
        let mut items = vec![];
        if self.ctx_for.is_some() { items.push(I::with_shortcut("Rename", true, "\u{21b5}")); items.push(I::Separator); }
        items.push(I::with_shortcut("New Note", true, &key(""))); items.push(I::with_shortcut("New Folder", true, &key("\u{21e7}")));
        if self.ctx_for.is_some() { items.push(I::Separator); items.push(I::with_shortcut("Delete\u{2026}", true, "\u{232b}")); }
        let (w, h) = g.presenter.size(); let s = g.window.scale_factor();
        self.ctx.open_at(items, &mut g.fonts, x, y, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), s);
        g.window.request_redraw();
    }
    /// An item of the context menu was chosen (its place among the items, separators counted).
    fn ctx_pick(&mut self, i: usize) {
        let label = match self.ctx.items.get(i) { Some(min_ui::menu::MenuItem::Action { label, .. }) => label.clone(), _ => return };
        // the notes a canvas's "Add Note" lists: the one chosen becomes a card where the menu was opened
        if !self.files.cnotes.is_empty() { let ids = std::mem::take(&mut self.files.cnotes); if let (Some(id), Some(cv)) = (ids.get(i).copied(), self.files.canvas.as_mut()) { cv.add_note(id, self.files.canvas_at.take()); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        // a canvas's menu (row 322)
        if std::mem::take(&mut self.files.cmenu) { let key = |c: &str| Key::Character(c.into());
            let was = self.mods; self.mods = (false, true, false);
            match label.as_str() { "Cut" => { self.canvas_key(&key("x"), min_ui::KeyEvent { key: min_ui::Key::Char("x".into()), text: None, shift: false, cmd: true }); } "Copy" => { self.canvas_key(&key("c"), min_ui::KeyEvent { key: min_ui::Key::Char("c".into()), text: None, shift: false, cmd: true }); }
                "Paste" => { self.canvas_key(&key("v"), min_ui::KeyEvent { key: min_ui::Key::Char("v".into()), text: None, shift: false, cmd: true }); } "Duplicate" => { self.canvas_key(&key("d"), min_ui::KeyEvent { key: min_ui::Key::Char("d".into()), text: None, shift: false, cmd: true }); }
                "Delete" => { if let Some(cv) = self.files.canvas.as_mut() { cv.delete(); } } "Bring to Front" => { if let Some(cv) = self.files.canvas.as_mut() { cv.restack(true); } } "Send to Back" => { if let Some(cv) = self.files.canvas.as_mut() { cv.restack(false); } }
                "Previous Page" | "Next Page" => { if let Some(cv) = self.files.canvas.as_mut() { cv.pdf_page(label == "Next Page"); } }
                "Open File" | "Reveal in Finder" | "Show in Explorer" => { if let Some(rec) = self.files.canvas.as_ref().and_then(|cv| cv.selected_media("file")).map(|e| e.record.clone()) { self.file_open(&rec, label != "Open File"); } }
                "Open Note" => { if let Some((_, note, _)) = self.card_note() { self.mods = was; self.open_note(note); return } }
                "Promote to Note" => { if let (Some((_, note, true)), Some(st)) = (self.card_note(), self.store.as_mut()) { let up = self.files.canvas.as_ref().and_then(|cv| st.col.entry(cv.id).ok().flatten()).and_then(|e| e.parent); if st.col.reparent(note, up).is_ok() { st.unpublished = true; self.changed.insert(note); if let Some(sb) = self.sidebar.as_mut() { sb.set_tree(&st.col); } } } }
                l if l.starts_with("Add Note") => { let (Some(st), Some(g)) = (self.store.as_ref(), self.gpu.as_mut()) else { self.mods = was; return };
                    fn walk(col: &mn_store::Collection, parent: Option<BlockId>, depth: usize, out: &mut Vec<(BlockId, String)>) { for e in col.list(parent).unwrap_or_default() {
                        if e.kind == mn_store::DocKind::Note { let t = if !e.title.is_empty() { e.title.clone() } else { col.first_line(e.id).unwrap_or_default() }; out.push((e.id, format!("{}{}", "    ".repeat(depth), if t.is_empty() { "Untitled".to_string() } else { t.chars().take(40).collect() }))); }
                        if e.kind == mn_store::DocKind::Folder { walk(col, Some(e.id), depth + 1, out); } } }
                    let mut notes = vec![]; walk(&st.col, None, 0, &mut notes); if notes.is_empty() { self.mods = was; return }
                    let (w, h) = g.presenter.size(); let s = g.window.scale_factor(); self.files.cnotes = notes.iter().map(|x| x.0).collect();
                    self.ctx.open_at(notes.iter().map(|x| min_ui::menu::MenuItem::action(&x.1, true)).collect(), &mut g.fonts, self.pointer.0, self.pointer.1, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), s); g.window.request_redraw(); self.mods = was; return }
                "Select All" => { if let Some(cv) = self.files.canvas.as_mut() { cv.select_all(); } }
                // frames, lining up, locking (row 327); three of the rows open a second list where the menu was
                "Group" => { if let Some(cv) = self.files.canvas.as_mut() { cv.group(); } } "Ungroup" => { if let Some(cv) = self.files.canvas.as_mut() { cv.ungroup(); } }
                "Rename Frame" => { if let (Some(cv), Some(g)) = (self.files.canvas.as_mut(), self.gpu.as_mut()) { cv.frame_rename(&mut g.fonts); } }
                "Fit to Contents" => { if let Some(cv) = self.files.canvas.as_mut() { cv.frame_fit(); } }
                "Lock" | "Unlock" => { if let Some(cv) = self.files.canvas.as_mut() { cv.lock(label == "Lock"); } }
                "Shape\u{2026}" | "Tint\u{2026}" | "Align\u{2026}" => { use min_ui::menu::MenuItem as I; let rows: Vec<I> = match label.as_str() {
                        "Shape\u{2026}" => ["16 : 9", "1 : 1", "9 : 16", "4 : 3"].iter().map(|l| I::action(l, true)).collect(),
                        "Tint\u{2026}" => std::iter::once("No Tint".to_string()).chain(mn_layout::NAMED.iter().map(|(n, _)| { let mut c = n.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() })).map(|l| I::action(&l, true)).collect(),
                        _ => { let n = self.files.canvas.as_ref().map(|c| c.sel.len()).unwrap_or(0); vec![I::action("Align Left", true), I::action("Align Centre", true), I::action("Align Right", true), I::Separator, I::action("Align Top", true), I::action("Align Middle", true), I::action("Align Bottom", true), I::Separator, I::action("Distribute Across", n >= 3), I::action("Distribute Down", n >= 3)] } };
                    if let Some(g) = self.gpu.as_mut() { let (w, h) = g.presenter.size(); let s = g.window.scale_factor(); let at = self.files.canvas_at.get().unwrap_or(self.pointer); self.files.cmenu = true; self.ctx.open_at(rows, &mut g.fonts, at.0, at.1, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), s); } }
                "16 : 9" | "1 : 1" | "9 : 16" | "4 : 3" => { let a = match label.as_str() { "16 : 9" => 16.0 / 9.0, "1 : 1" => 1.0, "9 : 16" => 9.0 / 16.0, _ => 4.0 / 3.0 }; if let Some(cv) = self.files.canvas.as_mut() { cv.frame_shape(a); } }
                "Align Left" | "Align Centre" | "Align Right" | "Align Top" | "Align Middle" | "Align Bottom" => { let how = match label.as_str() { "Align Left" => 0, "Align Centre" => 1, "Align Right" => 2, "Align Top" => 3, "Align Middle" => 4, _ => 5 }; if let Some(cv) = self.files.canvas.as_mut() { cv.align(how); } }
                "Distribute Across" | "Distribute Down" => { if let Some(cv) = self.files.canvas.as_mut() { cv.distribute(label == "Distribute Across"); } }
                "No Tint" => { if let Some(cv) = self.files.canvas.as_mut() { cv.frame_tint(""); } }
                l if mn_layout::NAMED.iter().any(|(n, _)| n.eq_ignore_ascii_case(l)) => { let name = l.to_lowercase(); if let Some(cv) = self.files.canvas.as_mut() { cv.frame_tint(&name); } }
                _ => { if let Some(cv) = self.files.canvas.as_mut() { cv.fit_all(); } } }
            self.mods = was; if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        // a link's menu (row 320)
        if let Some(u) = self.files.lmenu.take() { let now = self.now_ms();
            match label.as_str() { "Open Link" => { open::url(&u); } "Copy Link" => { if let Some(cb) = self.clipboard.as_mut() { let _ = cb.set_text(u); } self.block_clip = None; }
                "Remove Link" => { let d = self.ed.remove_link(now).unwrap_or(Dirty::Nothing); self.apply(d); } _ => self.link_open() }
            if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        // a table's row or column menu (row 317)
        if self.files.tmenu.take().is_some() { let now = self.now_ms(); self.follow = true; let label = label.trim_start_matches("\u{2713} ");
            let d = match label {
                "Insert Row Above" => self.ed.table_insert(false, false, now), "Insert Row Below" => self.ed.table_insert(false, true, now), "Move Row Up" => self.ed.table_move(false, false, now), "Move Row Down" => self.ed.table_move(false, true, now),
                "Duplicate Row" => self.ed.table_duplicate(false, now), "Delete Row" => if self.ed.cells_whole() == Some(false) { self.ed.cells_delete(now) } else { self.ed.table_remove(false, now) }, "No Header Row" | "First Row Is a Header" => self.ed.table_header(now),
                "Insert Column Left" => self.ed.table_insert(true, false, now), "Insert Column Right" => self.ed.table_insert(true, true, now), "Move Column Left" => self.ed.table_move(true, false, now), "Move Column Right" => self.ed.table_move(true, true, now),
                "Duplicate Column" => self.ed.table_duplicate(true, now), "Delete Column" => if self.ed.cells_whole() == Some(true) { self.ed.cells_delete(now) } else { self.ed.table_remove(true, now) },
                "Align Left" => self.ed.table_column(|c| c.align = 0, now), "Align Centre" => self.ed.table_column(|c| c.align = 1, now), "Align Right" => self.ed.table_column(|c| c.align = 2, now), "Automatic Width" => self.ed.table_column(|c| c.width = None, now),
                "Text Column" => { self.column_kind(0, false); return } "Choice Column" => { self.column_kind(1, false); return } "Check Column" => { self.column_kind(2, false); return } "Timecode Column" => { self.column_kind(3, false); return }
                "Edit Options\u{2026}" => { self.choice_open("", true); return }
                l if l.ends_with(" fps") => self.ed.table_set_kind(ColumnKind::Timecode { fps: l.trim_end_matches(" fps").parse().unwrap_or(24.0) }, now),
                _ => return }.unwrap_or(Dirty::Nothing);
            // a column's setting moves nothing in the tree but everything in the picture
            self.apply(d); self.layout = None; return }
        if self.ctx_blocks {
            // the second menu: the collection's notes; the one chosen takes the blocks
            if !self.ctx_notes.is_empty() { let copy = self.ctx_for.is_some(); let mut k = 0; let mut to = None;
                for (j, it) in self.ctx.items.iter().enumerate() { if matches!(it, min_ui::menu::MenuItem::Action { .. }) { if j == i { to = self.ctx_notes.get(k).copied(); } k += 1; } }
                self.ctx_notes.clear(); self.ctx_for = None; if let Some(to) = to { self.blocks_to_note(to, copy); } return }
            if label.starts_with("Move to Note") || label.starts_with("Copy to Note") {
                let (Some(st), Some(g)) = (self.store.as_ref(), self.gpu.as_mut()) else { return };
                fn walk(col: &mn_store::Collection, parent: Option<BlockId>, depth: usize, skip: BlockId, out: &mut Vec<(BlockId, String)>) { for e in col.list(parent).unwrap_or_default() {
                    if e.kind == mn_store::DocKind::Note && e.id != skip { let t = if !e.title.is_empty() { e.title.clone() } else { col.first_line(e.id).unwrap_or_default() }; out.push((e.id, format!("{}{}", "    ".repeat(depth), if t.is_empty() { "Untitled".to_string() } else { t.chars().take(40).collect() }))); }
                    if e.kind == mn_store::DocKind::Folder { walk(col, Some(e.id), depth + 1, skip, out); } } }
                let mut notes = vec![]; walk(&st.col, None, 0, st.note, &mut notes);
                if notes.is_empty() { return }
                self.ctx_notes = notes.iter().map(|x| x.0).collect();
                // `ctx_for` doubles as the "copy" mark while the notes are listed
                self.ctx_for = label.starts_with("Copy").then_some(st.note);
                let (w, h) = g.presenter.size(); let s = g.window.scale_factor();
                self.ctx.open_at(notes.iter().map(|x| min_ui::menu::MenuItem::action(&x.1, true)).collect(), &mut g.fonts, self.pointer.0, self.pointer.1, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), s);
                g.window.request_redraw(); return }
            if label == "Original Size" {
                let Some(id) = self.files.locate.take() else { return };
                if let Some(Kind::Media { descriptor }) = self.ed.doc.get(id).map(|b| b.kind.clone()) { if let Ok(serde_json::Value::Object(mut v)) = serde_json::from_str::<serde_json::Value>(&descriptor) {
                    v.remove("dw"); let t = self.now_ms(); let d = self.ed.set_kinds(vec![(id, Kind::Media { descriptor: serde_json::Value::Object(v).to_string() })], t).unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None; } }
                return }
            // a file chip's own rows
            if let Some((id, d)) = self.files.locate.and_then(|id| match self.ed.doc.get(id).map(|b| &b.kind) { Some(Kind::Media { descriptor }) if descriptor.contains("\"kind\":\"file\"") => Some((id, descriptor.clone())), _ => None }) {
                match label.as_str() {
                    "Open" => { self.files.locate = None; self.file_open(&d, false); return }
                    "Reveal in Finder" | "Show in Explorer" => { self.files.locate = None; self.file_open(&d, true); return }
                    "Copy Path" => { self.files.locate = None; if let (Some(p), Some(cb)) = (self.media.file_state(&d).path, self.clipboard.as_mut()) { let _ = cb.set_text(p.to_string_lossy().into_owned()); self.block_clip = None; } return }
                    l if l.starts_with("Locate File") => { self.files.locate = None;
                        self.panel(rfd::FileDialog::new().set_title("Locate File"), 1, move |me, p| if let Some(file) = p.first() { let rec = me.media.file_record(file); let t = me.now_ms(); let dd = me.ed.set_kinds(vec![(id, Kind::Media { descriptor: rec })], t).unwrap_or(Dirty::Nothing); me.apply(dd); me.layout = None; });
                        return }
                    _ => {} } }
            if label.starts_with("Locate File") {
                let Some(id) = self.files.locate.take() else { return };
                self.panel(rfd::FileDialog::new().set_title("Locate File"), 1, move |me, p| if let Some(file) = p.into_iter().next() {
                    let blocks = me.ed.doc.walk().iter().filter_map(|b| match &b.kind { Kind::Media { descriptor } => Some((b.id, descriptor.clone())), _ => None }).collect();
                    me.media.relink(blocks, id, file); });
                return }
            let now = self.now_ms(); self.follow = true;
            let d = match label.as_str() {
                "Cut" => { self.menu_cmd(menu::Cmd::Cut); return } "Copy" => { self.menu_cmd(menu::Cmd::Copy); return } "Paste" => { self.menu_cmd(menu::Cmd::Paste); return }
                "Duplicate" => self.ed.duplicate_blocks(now), "Delete" => self.ed.delete_blocks(now),
                "Move Up" => self.ed.move_blocks(false, now), "Move Down" => self.ed.move_blocks(true, now), _ => return }.unwrap_or(Dirty::Nothing);
            self.apply(d); return
        }
        match (label.as_str(), self.ctx_for) {
            ("Rename", Some(id)) => if let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) { sb.start_rename(id, &mut g.fonts); },
            ("New Note", _) => self.new_entry(mn_store::DocKind::Note), ("New Folder", _) => self.new_entry(mn_store::DocKind::Folder),
            (_, Some(id)) => self.ask_delete(id),
            _ => {}
        }
    }
    /// A button of the toolbar (row 297): the same routines the menu and the keys run.
    fn tool(&mut self, tool: toolbar::Tool) {
        use toolbar::Tool as T; use menu::Cmd as M; use mn_edit::BlockType as B;
        let now = self.now_ms();
        let block = |me: &mut App, b: B| { let d = me.ed.set_block_kind(b, now).unwrap_or(Dirty::Nothing); me.apply(d); };
        // a canvas's drawing tools (row 324): armed and put down on the canvas itself
        if self.files.card.is_none() { if let Some(cv) = self.files.canvas.as_mut() { use canvas::Ink as I;
            if let Some(i) = match tool { T::DrawSelect => Some(None), T::DrawPen => Some(Some(I::Pen)), T::DrawLine => Some(Some(I::Line)), T::DrawArrow => Some(Some(I::Arrow)), T::DrawRect => Some(Some(I::Rect)), T::DrawOval => Some(Some(I::Oval)), T::DrawErase => Some(Some(I::Eraser)), _ => None } {
                cv.arm(i); if let Some(g) = &self.gpu { g.window.request_redraw(); } self.set_cursor(); return }
            // the Frame button: with cards selected it frames them at once (as Cmd+G); with none it arms the frame tool
            if tool == T::Frame { if cv.framing || !cv.group() { let on = !cv.framing; cv.arm(None); cv.framing = on; } if let Some(g) = &self.gpu { g.window.request_redraw(); } self.set_cursor(); return } } }
        // a picture's or a PDF page's tools (row 325): armed and put down; the pen's colour and width also restyle the picked stroke
        if self.files.vshown.is_none() && self.files.ishown.is_some() { use ink::{Ink as I, Pick as P};
            if let Some(p) = match tool { T::DrawSelect => Some(P::Select), T::DrawPen => Some(P::Draw(I::Pen)), T::DrawLine => Some(P::Draw(I::Line)), T::DrawArrow => Some(P::Draw(I::Arrow)), T::DrawRect => Some(P::Draw(I::Rect)), T::DrawOval => Some(P::Draw(I::Oval)), T::DrawErase => Some(P::Draw(I::Eraser)), _ => None } {
                self.files.pick = if self.files.pick == Some(p) { None } else { Some(p) }; self.files.psel = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } self.set_cursor(); return } }
        match tool {
            T::Bold => self.menu_cmd(M::Bold), T::Italic => self.menu_cmd(M::Italic), T::Underline => self.menu_cmd(M::Underline), T::Strike => self.menu_cmd(M::Strike), T::Code => self.menu_cmd(M::Code), T::Highlight => self.open_colours(mn_doc::MarkKind::Highlight, tool), T::Color => self.open_colours(mn_doc::MarkKind::Color, tool), T::ClearFormat => self.menu_cmd(M::ClearFormat),
            T::Paragraph => block(self, B::Paragraph), T::Bullet => block(self, B::Bullet), T::Numbered => block(self, B::Numbered), T::Task => block(self, B::Task), T::Quote => block(self, B::Quote), T::CodeBlock => block(self, B::Code),
            // Heading steps through the first three levels and back to a paragraph
            T::Heading => { let next = match self.ed.state().kind { Some(Kind::Heading { level }) if level < 3 => B::Heading(level + 1), Some(Kind::Heading { .. }) => B::Paragraph, _ => B::Heading(1) }; block(self, next) }
            T::Indent | T::Outdent => { let d = self.ed.indent(tool == T::Outdent, now).ok().flatten().unwrap_or(Dirty::Nothing); self.apply(d); }
            T::Divider => { let d = self.ed.insert_divider(now).unwrap_or(Dirty::Nothing); self.apply(d); }
            // a table of three rows and three columns, its first row a header; a column beside the caret's block (row 317)
            T::Frame => {}   // a canvas's (taken above)
            T::Link => self.link_open(),
            T::Chip => self.chip_insert(),
            T::Table => { let d = self.ed.insert_table(3, 3, now).unwrap_or(Dirty::Nothing); self.apply(d); }
            T::Columns => { let d = self.ed.insert_columns(now).unwrap_or(Dirty::Nothing); self.apply(d); }
            // a video block's set (rows 301, 316): the drawing tools arm and put down; the pen; the frame's note
            T::DrawSelect => self.arm(qcv_annotate::Tool::Select), T::DrawPen => self.arm(qcv_annotate::Tool::Freehand), T::DrawRect => self.arm(qcv_annotate::Tool::Rectangle), T::DrawOval => self.arm(qcv_annotate::Tool::Oval),
            T::DrawArrow => self.arm(qcv_annotate::Tool::Arrow), T::DrawLine => self.arm(qcv_annotate::Tool::Line), T::DrawErase => self.arm(qcv_annotate::Tool::Eraser),
            T::PenThinner | T::PenThicker => { let steps = [1.0f32, 2.0, 3.0, 4.0, 6.0, 9.0, 14.0, 22.0, 32.0, 48.0, 64.0]; let w = self.files.pen_w.unwrap_or(6.0); let i = steps.iter().position(|x| *x >= w).unwrap_or(4);
                let j = if tool == T::PenThinner { i.saturating_sub(1) } else { (i + 1).min(steps.len() - 1) }; self.files.pen_w = Some(steps[j]); if let Some(p) = self.player.as_mut() { p.ann.width = steps[j]; }
                if let Some(cv) = self.files.canvas.as_mut() { cv.pen_w = steps[j] as f64; cv.restyle(None, Some(steps[j] as f64)); } save_pen_settings(self.files.pen, self.files.pen_w);
                { let was = w as f64; let now = steps[j] as f64; self.page_picked(|l| { l.s.width *= now / was.max(0.1); true }); }
                self.notice = Some((format!("Line width {}", steps[j]), Instant::now() + std::time::Duration::from_secs(2))); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            T::PenColor => { self.files.pal_pen = true; self.open_colours(mn_doc::MarkKind::Color, tool); }
            T::NoteText | T::NoteDone | T::NoteDelete => { if let Some(id) = self.video_current() { if self.video_start(id, false) {
                let mut keep = None; if let Some(p) = self.player.as_mut() { if p.t.is_playing() { p.t.pause(); keep = Some(p.t.current_frame()); } } if let Some(f) = keep { self.video_keep(id, f); }
                if let Some((_, cur)) = self.video_geo() { let key = self.player.as_ref().and_then(|p| p.ann.note_at(&cur).map(|n| (n.timecode.clone(), n.addressed)));
                    match (tool, key) {
                        (T::NoteText, Some(_)) => self.note_words(),
                        // on a frame without a note the same button makes one and opens its words
                        (T::NoteText, None) => { let c = self.player.as_mut().and_then(|p| p.ann.add_note(&cur)); if let Some(c) = c { self.annotated(c); self.note_words(); } }
                        (T::NoteDone, Some((k, on))) => { let mut done = None; if let Some(p) = self.player.as_mut() { p.ann.set_addressed(&k, !on); done = Some((p.path.clone(), p.ann.notes.clone())); } if let Some((path, notes)) = done { self.media.set_notes(&path, notes); } }
                        (T::NoteDelete, Some((k, _))) => { let mut done = None; if let Some(p) = self.player.as_mut() { p.ann.delete_note(&k); done = Some((p.path.clone(), p.ann.notes.clone(), p.ann.dir())); }
                            if let Some((path, notes, dir)) = done { self.media.set_notes(&path, notes); if let Some(d) = dir { let d = qcv_notes::io::images_dir(&d); self.media.forget_file(&d.join(qcv_notes::io::image_filename(&k))); self.media.forget_file(&d.join(qcv_notes::io::annotated_filename(&k))); } } }
                        _ => {} } }
                if let Some(g) = &self.gpu { g.window.request_redraw(); } } } }
            // pictures (and any other file, as a chip) chosen in the system's dialog go in after the caret's block (at the note's end when there is no caret)
            // files of any kind, as chips that point at them (a picture, a video, a PDF chosen here still becomes its own block)
            T::File | T::Image if self.files.canvas.is_some() && self.files.card.is_none() => { if self.store.is_some() { self.panel(rfd::FileDialog::new().set_title("Add to the Canvas"), 2, |me, paths| { me.files.canvas_at.set(None); me.add_files(paths, Self::canvas_drop()); }); } }
            T::File => if let (Some(at), true) = (self.ed.after_caret().or(Some(mn_edit::Drop { parent: None, index: self.ed.doc.children(None).len(), depth: None, beside: None })), self.store.is_some()) { self.panel(rfd::FileDialog::new().set_title("Add Files"), 2, move |me, paths| me.add_files(paths, at)); },
            T::Image => if let (Some(at), true) = (self.ed.after_caret().or(Some(mn_edit::Drop { parent: None, index: self.ed.doc.children(None).len(), depth: None, beside: None })), self.store.is_some()) { self.panel(rfd::FileDialog::new().set_title("Add Pictures"), 2, move |me, paths| me.add_files(paths, at)); },
        }
    }
    /// Dropped files that the helpers turned into records become blocks where they were dropped (one undo step); a
    /// relink's records replace the blocks' own. What could not be done is said in the footer's notice.
    fn media_mail(&mut self, m: media::Mail) {
        let now = self.now_ms(); let until = Instant::now() + std::time::Duration::from_secs(6);
        match m {
            media::Mail::Added { token, records, refused } => {
                let Some(at) = self.files.pending.remove(&token) else { return };
                if !refused.is_empty() { eprintln!("drop: {}", refused.join("; ")); self.notice = Some((if refused.len() == 1 { format!("Not added \u{2014} {}", refused[0]) } else { format!("{} files were not added \u{2014} {}", refused.len(), refused[0]) }, until)); }
                if records.is_empty() { self.apply(Dirty::Nothing); return }
                if at.index == usize::MAX { if let Some(cv) = self.files.canvas.as_mut() { cv.add_media(records, self.files.canvas_at.take()); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                let kinds = records.into_iter().map(|descriptor| Kind::Media { descriptor }).collect();
                match self.ed.insert_blocks(&at, kinds, now) { Ok(d) => { self.follow = true; self.apply(d) } Err(e) => eprintln!("drop: {e:?}") }
            }
            // a picture named by an address has arrived: the blocks that name it point at the kept copy instead. Not a
            // step of the history (nobody did it); a redo that brings the address back is answered the same way
            media::Mail::Fetched { src, record } => {
                let Ok(serde_json::Value::Object(new)) = serde_json::from_str::<serde_json::Value>(&record) else { return };
                let hits: Vec<(BlockId, String)> = self.ed.doc.walk().iter().filter_map(|b| match &b.kind { Kind::Media { descriptor } => { let mut v = serde_json::from_str::<serde_json::Value>(descriptor).ok()?; let o = v.as_object_mut()?;
                    if o.get("src").and_then(|s| s.as_str()) != Some(src.as_str()) { return None } for k in ["src", "w", "h", "name"] { if let Some(x) = new.get(k) { o.insert(k.to_string(), x.clone()); } } Some((b.id, v.to_string())) } _ => None }).collect();
                if hits.is_empty() { return }
                for (id, descriptor) in hits { let _ = self.ed.doc.set_kind(id, Kind::Media { descriptor }); }
                if let Some(st) = self.store.as_mut() { let first = st.edited.is_none() && !st.unpublished; st.edited = Some(Instant::now()); if first { self.title(); } }
                self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); }
            }
            media::Mail::Relinked { records, problem } => {
                if let Some(why) = problem { self.notice = Some((format!("Not linked \u{2014} {why}"), until)); }
                let n = records.len(); if n == 0 { self.apply(Dirty::Nothing); return }
                let kinds = records.into_iter().map(|(id, descriptor)| (id, Kind::Media { descriptor })).collect();
                match self.ed.set_kinds(kinds, now) { Ok(d) => { self.notice = Some((if n == 1 { "Linked 1 picture".to_string() } else { format!("Linked {n} pictures \u{2014} the others were found in the same folder") }, until)); self.apply(d); self.layout = None; } Err(e) => eprintln!("relink: {e:?}") }
            }
        }
    }
    /// A video block's boxes in the window: its picture, its strip, and its record.
    fn video_boxes(&self, id: BlockId) -> Option<(BlockId, Rect, Rect, String)> {
        // on a canvas (row 326): a video's card - its picture is the card, its strip lies under it
        if let Some(cv) = self.files.canvas.as_ref().filter(|_| self.files.card.is_none()) { let (pic, d) = cv.video_el(id)?; return Some((id, pic, cv.video_strip(id)?, d)) }
        let (l, g) = (self.layout.as_ref()?, self.gpu.as_ref()?); let s = g.window.scale_factor();
        let Some(Kind::Media { descriptor }) = self.ed.doc.get(id).map(|b| &b.kind) else { return None };
        if serde_json::from_str::<serde_json::Value>(descriptor).ok()?.get("kind").and_then(|k| k.as_str()) != Some("video") { return None }
        let r = l.rect(&self.ed.doc, id)? + origin(&self.files.card, self.side_w, self.top, self.scroll, s);
        let row = if l.is_noted(id) { mn_layout::VIDEO_NOTES as f64 * s } else { 0.0 }; let cut = r.y1 - row - mn_layout::VIDEO_STRIP as f64 * s;
        Some((id, Rect::new(r.x0, r.y0, r.x1, cut), Rect::new(r.x0, cut, r.x1, r.y1 - row), descriptor.clone()))
    }
    /// The row of a video block's notes (their small pictures), when its clip has notes.
    fn video_row(&self, id: BlockId) -> Option<Rect> { let (_, _, strip, _) = self.video_boxes(id)?; let l = self.layout.as_ref()?; let s = self.gpu.as_ref()?.window.scale_factor(); l.is_noted(id).then(|| Rect::new(strip.x0, strip.y1, strip.x1, strip.y1 + mn_layout::VIDEO_NOTES as f64 * s)) }
    /// The playing video's picture as the annotations need it (its rectangle in the window, window px per source px) and
    /// the frame it stands on as a note's address.
    fn video_geo(&self) -> Option<(qcv_annotate::Geo, qcv_annotate::Cur)> {
        let p = self.player.as_ref()?; let (_, pic, _, d) = self.video_boxes(p.id)?;
        let rec: serde_json::Value = serde_json::from_str(&d).ok()?;
        let (w, fps) = (rec.get("w").and_then(|w| w.as_f64()).unwrap_or(1920.0).max(1.0), rec.get("fps").and_then(|f| f.as_f64()).unwrap_or(24.0));
        Some((qcv_annotate::Geo { rect: [pic.x0, pic.y0, pic.width(), pic.height()], px_per_src: pic.width() / w }, qcv_annotate::Cur::file(p.t.current_frame(), fps)))
    }
    /// A change of the clip's notes was made and written (a stroke, an erasure, a new note, an undo): the page shows it;
    /// a new note's CLEAN frame is captured now - the frame as the presenter shows it, brought to SDR, at the clip's
    /// size, written once and never again (QCView's rule); its annotated picture follows 400 ms after the last change.
    fn annotated(&mut self, c: qcv_annotate::Committed) {
        let Some(p) = self.player.as_ref() else { return };
        self.media.set_notes(&p.path, p.ann.notes.clone());
        let Some(dir) = p.ann.dir().map(|d| qcv_notes::io::images_dir(&d)) else { return };
        let clean = dir.join(qcv_notes::io::image_filename(&c.key));
        let on_it = self.video_geo().is_some_and(|(_, cur)| cur.key == c.key);
        if on_it && p.ann.notes.iter().any(|n| n.timecode == c.key) && !clean.exists() { if let Some(g) = self.gpu.as_mut() { let en = &mut g.presenter.engine; let peak = en.bypass_hdr_peak();
            if let Ok((mut lin, w, h)) = en.capture(false) { min_present::capture::to_sdr(&mut lin, peak); let rgb = min_present::capture::srgb8(&lin);
                self.files.clean = Some((c.key.clone(), Arc::new(rgb.chunks(3).flat_map(|x| [x[0], x[1], x[2], 255]).collect()), (w, h)));
                let (tx, rx) = std::sync::mpsc::channel(); self.files.written.push(rx); let _ = std::fs::create_dir_all(&dir);
                let _ = std::thread::Builder::new().name("mn-note-clean".into()).spawn(move || { if let Err(e) = min_image::write::png8(&clean.to_string_lossy(), &rgb, w, h) { eprintln!("note picture: {e}"); } let _ = tx.send(clean); if let Some(p) = WAKE.get() { p.wake_up(); } }); } } }
        self.files.annot_due = Some((c.key, Instant::now() + std::time::Duration::from_millis(400)));
        let _ = std::thread::Builder::new().name("mn-note-due".into()).spawn(|| { std::thread::sleep(std::time::Duration::from_millis(430)); if let Some(p) = WAKE.get() { p.wake_up(); } });
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// A note's annotated picture: its clean frame with its strokes on it, composed off screen with the very code that
    /// draws them on screen, written beside the clean one; no strokes left - the file goes (QCView's rules).
    fn compose_annotated(&mut self, key: &str) {
        let Some(p) = self.player.as_ref() else { return }; let Some(dir) = p.ann.dir().map(|d| qcv_notes::io::images_dir(&d)) else { return };
        let (clean, annotated) = (dir.join(qcv_notes::io::image_filename(key)), dir.join(qcv_notes::io::annotated_filename(key)));
        let strokes = p.ann.notes.iter().find(|n| n.timecode == key).map(|n| qcv_notes::strokes::parse(n.annotation.as_ref())).unwrap_or_default();
        if strokes.is_empty() { let _ = std::fs::remove_file(&annotated); self.media.forget_file(&annotated); return }
        let img: Option<(Arc<Vec<u8>>, (u32, u32))> = self.files.clean.as_ref().filter(|(k, _, _)| k == key).map(|(_, px, wh)| (px.clone(), *wh)).or_else(|| min_image::load(&clean.to_string_lossy(), "", 2).ok().map(|i| { let (px, w, h) = media::rgba_of(&i); (Arc::new(px), (w, h)) }));
        let (Some((px, (w, h))), Some(g)) = (img, self.gpu.as_mut()) else { return };
        let (dev, q) = (g.presenter.engine.device.clone(), g.presenter.engine.queue.clone());
        if self.files.thumb_layer.is_none() { self.files.thumb_layer = UiLayer::new(&dev, w, h).ok(); }
        let Some(layer) = self.files.thumb_layer.as_mut() else { return };
        if layer.size() != (w, h) { layer.resize(&dev, w, h); }
        let scene = layer.scene_mut(); scene.reset();
        min_ui::paint::image(scene, &min_ui::paint::image_rgba8(w, h, (*px).clone()), Rect::new(0.0, 0.0, w as f64, h as f64));
        let geo = qcv_annotate::Geo { rect: [0.0, 0.0, w as f64, h as f64], px_per_src: 1.0 };
        for st in &strokes { qcv_annotate::draw(scene, st, &geo); }
        layer.render(&dev, &q);
        let out = min_present::readback::read_rgba8(&dev, &q, layer.texture(), w, h);
        let (tx, rx) = std::sync::mpsc::channel(); self.files.written.push(rx);
        let _ = std::thread::Builder::new().name("mn-note-annotated".into()).spawn(move || { let rgb: Vec<u8> = out.chunks(4).flat_map(|c| [c[0], c[1], c[2]]).collect(); if let Err(e) = min_image::write::png8(&annotated.to_string_lossy(), &rgb, w, h) { eprintln!("note picture: {e}"); } let _ = tx.send(annotated); if let Some(p) = WAKE.get() { p.wake_up(); } });
    }
    /// A drawing tool is armed (the same again: put down). A click on a video's picture then draws instead of playing.
    fn arm(&mut self, tool: qcv_annotate::Tool) {
        self.files.tool = if self.files.tool == Some(tool) { None } else { Some(tool) };
        if let Some(p) = self.player.as_mut() { p.ann.active = self.files.tool.is_some(); if let Some(t) = self.files.tool { p.ann.tool = t; } p.ann.escape(); }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The field a note's words are typed in opens on the playing video's note (Enter keeps them, Escape gives them up).
    fn note_words(&mut self) {
        let Some((_, cur)) = self.video_geo() else { return }; let Some(p) = self.player.as_ref() else { return };
        let Some(n) = p.ann.note_at(&cur) else { return }; let Some(g) = self.gpu.as_mut() else { return };
        let mut f = min_ui::TextField::new(min_ui::Family::Sans, 12.0); f.placeholder = "Note\u{2026}".into(); f.set_text_end(&n.text.replace(['\n', '\r'], " "), &mut g.fonts); f.focused = true;
        self.files.note_field = Some((p.id, n.timecode.clone(), f)); g.window.request_redraw();
    }
    /// The words typed for a note are kept (`keep`) or given up.
    fn note_words_end(&mut self, keep: bool) {
        let Some((_, key, f)) = self.files.note_field.take() else { return };
        if keep { let text = f.text().trim().to_string(); let mut done = None; if let Some(p) = self.player.as_mut() { p.ann.set_text(&key, &text); done = Some((p.path.clone(), p.ann.notes.clone())); } if let Some((path, notes)) = done { self.media.set_notes(&path, notes); } }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// A video block goes to a frame (a click on a note's picture or tick): it becomes the playing one, paused there.
    fn video_seek(&mut self, id: BlockId, frame: i32) {
        if !self.video_start(id, false) { return }
        if let Some(p) = self.player.as_mut() { p.t.pause(); p.t.seek_to_frame(frame); p.awake = Instant::now() + std::time::Duration::from_millis(900); }
        self.video_keep(id, frame);
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn video_under(&self, x: f64, y: f64) -> Option<(BlockId, Rect, Rect, String)> {
        if self.blank || self.modal() || self.bar.hit(x, y) || self.foot.hit(x, y) || x < self.side_w || y < self.top { return None }
        let l = self.layout.as_ref()?; let (nx, ny) = self.note_point((x, y)); let (id, _) = l.hit(&self.ed.doc, nx, ny)?;
        self.video_boxes(id)
    }
    /// A press on a canvas that is a video's (row 326): on the selected card's strip - play or pause, or the line to scrub
    /// on; inside a video card that was entered, with a drawing tool armed - the press draws on the frame (QCView's notes,
    /// as in a note: the video becomes the playing one, paused). True: the press was the video's.
    fn canvas_video_press(&mut self, x: f64, y: f64) -> bool {
        let pt = kurbo::Point::new(x, y); let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
        if let Some((id, _, strip, _)) = self.video_current().and_then(|id| self.video_boxes(id)) { if strip.contains(pt) { let (_, track) = paint::video_strip(strip, s);
            if track.inflate(4.0 * s, 12.0 * s).contains(pt) { if self.video_start(id, false) { if let Some(p) = self.player.as_mut() { p.t.begin_scrub(); p.scrub = true; } self.video_scrub(x); } } else { self.video_toggle(id); }
            return true } }
        let Some((id, tool, pen, pen_w)) = self.files.canvas.as_ref().and_then(|cv| Some((cv.inked?, cv.tool?, cv.pen, cv.pen_w))) else { return false };
        let Some((_, pic, _, _)) = self.video_boxes(id) else { return false }; if !pic.contains(pt) { return false }
        if !self.video_start(id, false) { return true }
        let mut keep = None; if let Some(p) = self.player.as_mut() { if p.t.is_playing() { p.t.pause(); keep = Some(p.t.current_frame()); } } if let Some(f) = keep { self.video_keep(id, f); }
        if let (Some((geo, cur)), true) = (self.video_geo(), self.player.as_ref().is_some_and(|p| p.shown)) { if let Some(p) = self.player.as_mut() { use canvas::Ink as I; use qcv_annotate::Tool as A;
            p.ann.active = true; p.ann.tool = match tool { I::Pen => A::Freehand, I::Line => A::Line, I::Arrow => A::Arrow, I::Rect => A::Rectangle, I::Oval => A::Oval, I::Eraser => A::Eraser }; p.ann.color = pen; p.ann.width = pen_w as f32;
            p.ann.press(geo.to_norm(x, y), &cur, &geo); self.files.drawing = true; } }
        if let Some(g) = &self.gpu { g.window.request_redraw(); } true
    }
    /// The frame a video block stands on goes into its record (not a step of the history).
    fn video_keep(&mut self, id: BlockId, frame: i32) {
        if let Some(cv) = self.files.canvas.as_mut().filter(|c| c.video_el(id).is_some()) { cv.set_frame(id, frame); return }
        let Some(Kind::Media { descriptor }) = self.ed.doc.get(id).map(|b| b.kind.clone()) else { return };
        let Ok(serde_json::Value::Object(mut v)) = serde_json::from_str::<serde_json::Value>(&descriptor) else { return };
        if v.get("frame").and_then(|f| f.as_i64()).unwrap_or(0) == frame as i64 { return }
        v.insert("frame".into(), serde_json::json!(frame));
        let _ = self.ed.doc.set_kind(id, Kind::Media { descriptor: serde_json::Value::Object(v).to_string() });
        if let Some(st) = self.store.as_mut() { let first = st.edited.is_none() && !st.unpublished; st.edited = Some(Instant::now()); if first { self.title(); } }
    }
    /// The playing video stops being the playing one: its block keeps the frame it stood on and shows a poster again.
    fn video_stop(&mut self) {
        let Some(p) = self.player.take() else { return };
        self.files.drawing = false; self.files.annot_due = None; self.files.note_field = None;
        let (id, frame) = (p.id, p.t.current_frame()); drop(p);
        self.video_keep(id, frame);
        if let Some(g) = self.gpu.as_mut() { g.presenter.engine.clear_picture(); g.presenter.engine.set_place(None); g.window.request_redraw(); }
    }
    /// A video block becomes the one that plays: the player opens on its file at the frame the block stands on. Any
    /// other stops first (one plays at a time). False: the file could not be opened (said in the footer).
    fn video_start(&mut self, id: BlockId, play: bool) -> bool {
        if self.player.as_ref().is_some_and(|p| p.id == id) { return true }
        self.video_stop();
        let Some((_, _, _, d)) = self.video_boxes(id) else { return false };
        let frame = serde_json::from_str::<serde_json::Value>(&d).ok().and_then(|v| v.get("frame").and_then(|f| f.as_i64())).unwrap_or(0) as i32;
        let until = Instant::now() + std::time::Duration::from_secs(6);
        let Some(path) = self.media.locate(&d).filter(|p| p.is_file()) else { self.notice = Some(("The video's file was not found".into(), until)); return false };
        let Some(g) = self.gpu.as_ref() else { return false };
        let hw = self.hw.get_or_insert_with(|| { #[cfg(windows)] let dev = g.presenter.engine.dx12_device_raw(); #[cfg(not(windows))] let dev = None; qcv_player::HwDevice::platform(dev) }).clone();
        let opts = qcv_player::TransportOptions { hw, loop_enabled: true, scrub: Some(qcv_player::ScrubOptions { budget_bytes: 256 << 20, ..Default::default() }), uploader: Some(g.presenter.engine.uploader()), ..Default::default() };
        match qcv_player::Transport::open(&qcv_player::Source::from_path(&path.to_string_lossy(), None), opts) {
            Ok(mut t) => {
                if std::env::var_os("MN_NO_AUDIO").is_none() { match qcv_player::AudioPlayer::new() { Ok(a) => t.set_audio(Box::new(a)), Err(e) => eprintln!("video: no sound ({e})") } }
                // loop, speed, volume and mute are the app's, remembered between launches (row 316, ruling 6)
                let set = player_settings(); t.set_loop_enabled(set.0); t.set_review_speed(set.1); t.audio.set_volume(set.2); t.audio.set_muted(set.3);
                if frame > 0 { t.seek_to_frame(frame); }
                if play { t.play(); }
                let mut ann = qcv_annotate::Annotations::new(); ann.load(Some(&path));
                ann.active = self.files.tool.is_some(); ann.tool = self.files.tool.unwrap_or(qcv_annotate::Tool::Freehand); ann.color = self.files.pen.unwrap_or([1.0, 0.0, 0.0, 1.0]); ann.width = self.files.pen_w.unwrap_or(6.0);
                self.media.set_notes(&path, ann.notes.clone());
                self.player = Some(Player { id, t, scrub: false, shown: false, awake: Instant::now() + std::time::Duration::from_millis(800), ann, path });
                g.window.request_redraw(); true
            }
            Err(e) => { self.notice = Some((format!("The video could not be opened \u{2014} {e}"), until)); false }
        }
    }
    /// Play or pause a video block (it becomes the playing one when it is not).
    fn video_toggle(&mut self, id: BlockId) {
       
        if self.player.as_ref().is_none_or(|p| p.id != id) { self.video_start(id, true); return }
        let mut keep = None;
        if let Some(p) = self.player.as_mut() { if p.t.is_playing() { p.t.pause(); keep = Some(p.t.current_frame()); } else { p.t.play(); } p.awake = Instant::now() + std::time::Duration::from_millis(800); }
        if let Some(f) = keep { self.video_keep(id, f); }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The video block that is the current one - the caret is on it or it is the one selected block - when keys are the
    /// page's to give (no dialog, menu, palette or rename field).
    fn video_current(&self) -> Option<BlockId> {
        // on a canvas: the one selected card, when it is a video
        if let Some(cv) = self.files.canvas.as_ref().filter(|_| self.files.card.is_none()) { if self.modal() || self.sidebar.as_ref().is_some_and(|sb| sb.renaming() || sb.typing()) || self.pal.is_open() || self.ctx.is_open() { return None } return cv.selected_video() }
        if self.blank || self.modal() || self.files.note_field.is_some() || self.ed.run_len() != 1 || self.sidebar.as_ref().is_some_and(|sb| sb.renaming()) || self.pal.is_open() || self.ctx.is_open() { return None }
        self.ed.handle_block().filter(|id| self.video_boxes(*id).is_some())
    }
    /// A key while a video block is the current one (row 316, stage 3): QCView's transport keys (`qcv_player::map_key`)
    /// act on it - the block becomes the playing one, paused, when it is not. True: the key was the video's. Kept for
    /// the page: Up / Down (they leave the block), and every key with Cmd. Not taken up here: the in and out points, the
    /// background, screenshots, full screen (their letters do nothing rather than type beside a video).
    fn video_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use qcv_player::Action as A;
        let (shift, cmd, alt) = self.mods;
        let Some(id) = self.video_current() else { return false };
        // the playing clip's drawings have a history of their own (QCView's rule: never the document's): Cmd+Z and
        // Shift+Cmd+Z step through it; Delete removes the selected drawing; Escape gives up a stroke or a selection
        if event.state.is_pressed() && self.player.as_ref().is_some_and(|p| p.id == id) {
            let z = matches!(&event.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("z"));
            if cmd && z { let c = self.player.as_mut().and_then(|p| if shift { p.ann.redo() } else { p.ann.undo() }); if let Some(c) = c { self.annotated(c); } return true }
            if !cmd && matches!(&event.logical_key, Key::Named(NamedKey::Backspace | NamedKey::Delete)) && self.files.tool == Some(qcv_annotate::Tool::Select) { if let Some((_, cur)) = self.video_geo() { let c = self.player.as_mut().and_then(|p| p.ann.delete_selected(&cur)); if let Some(c) = c { self.annotated(c); return true } } }
            if !cmd && matches!(&event.logical_key, Key::Named(NamedKey::Escape)) && self.player.as_mut().is_some_and(|p| p.ann.escape()) { self.files.drawing = false; if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        }
        if cmd { return false }
        let key = match &event.logical_key {
            Key::Character(c) if c == " " => qcv_player::Key::Space,
            Key::Character(c) => match c.chars().next() { Some(ch) => qcv_player::Key::Char(ch), None => return false },
            Key::Named(NamedKey::ArrowLeft) => qcv_player::Key::Left, Key::Named(NamedKey::ArrowRight) => qcv_player::Key::Right,
            Key::Named(NamedKey::Home) => qcv_player::Key::Home, Key::Named(NamedKey::End) => qcv_player::Key::End, _ => return false };
        let Some(a) = qcv_player::map_key(key, qcv_player::Mods { shift, ctrl: false, alt, keypad: false }, event.state.is_pressed(), event.repeat) else { return false };
        match a {
            A::TogglePlayback => { self.video_toggle(id); return true }
            A::SetInPoint | A::SetOutPoint | A::ClearInOut | A::CycleBackground | A::Screenshot | A::ScreenshotToClipboard | A::ToggleFullscreen | A::AnnotationUndo | A::AnnotationRedo => return true,
            // a release that ends a shuttle matters only to the video that is being shuttled
            A::ShuttleEnd if self.player.as_ref().is_none_or(|p| p.id != id) => return true,
            _ => {}
        }
        if !self.video_start(id, false) { return true }
        let mut keep = None;
        if let Some(p) = self.player.as_mut() { let now = Instant::now(); p.t.apply(a, now); p.awake = now + std::time::Duration::from_millis(800);
            save_player_settings((p.t.loop_enabled(), p.t.review_speed(), p.t.audio.volume(), p.t.audio.is_muted()));
            if !p.t.is_playing() && matches!(a, A::Step(_) | A::SeekToStart | A::SeekToEnd | A::ShuttleEnd) { keep = Some(p.t.current_frame()); } }
        if let Some(f) = keep { self.video_keep(id, f); }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
        true
    }
    /// The playing video's line is dragged to a window x.
    fn video_scrub(&mut self, x: f64) {
       
        let Some(id) = self.player.as_ref().map(|p| p.id) else { return }; let Some((_, _, strip, _)) = self.video_boxes(id) else { return };
        let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); let (_, track) = paint::video_strip(strip, s);
        if let Some(p) = self.player.as_mut() { let n = p.t.frame_count().max(1); let f = (((x - track.x0) / track.width().max(1.0)).clamp(0.0, 1.0) * (n - 1) as f64).round() as i32; p.t.scrub_to_frame(f); p.awake = Instant::now() + std::time::Duration::from_millis(800); }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The PDF block under a window point: its id, its page's box and its strip's box in the window, and its record.
    /// The current block when it can be drawn on (row 325): a picture, or a PDF - its page's frame in the window, its record.
    fn ink_block(&self) -> Option<(BlockId, Rect, String)> {
        if self.blank || self.modal() || (self.files.canvas.is_some() && self.files.card.is_none()) { return None }
        if let Some((id, r)) = self.picture() { if let Some(Kind::Media { descriptor }) = self.ed.doc.get(id).map(|b| &b.kind) { return Some((id, r, descriptor.clone())) } }
        let id = self.ed.handle_block().filter(|_| self.ed.run_len() == 1 && !self.grab.as_ref().is_some_and(|g| g.dragging))?; let (id, page, _, d) = self.pdf_boxes(id)?; Some((id, page, d))
    }
    fn pen_now(&self) -> ([f32; 4], f64) { (self.files.pen.unwrap_or([1.0, 0.0, 0.0, 1.0]), self.files.pen_w.unwrap_or(6.0) as f64) }
    /// A press with a page tool armed, inside the current block's frame: the gesture begins. True: the press was the page's.
    fn page_press(&mut self, x: f64, y: f64) -> bool {
        let Some(pick) = self.files.pick else { return false }; let Some((id, frame, d)) = self.ink_block() else { return false }; if !frame.contains(kurbo::Point::new(x, y)) || frame.width() < 1.0 { return false }
        let rec: serde_json::Value = serde_json::from_str(&d).unwrap_or_default(); let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); let k = ink::page_size(&rec, frame).0 / frame.width();
        let (g, picked) = ink::Gesture::press(pick, id, &d, &rec, frame, kurbo::Point::new((x - frame.x0) * k, (y - frame.y0) * k), pen_pressure(), (self.pen_now().1 / 2.0).max(5.0) * s * k, 4.0 * s * k);
        if pick == ink::Pick::Select { self.files.psel = picked.map(|i| (id, i)); } else { self.files.psel = None; }
        self.files.iblock = Some((id, frame, d)); self.files.gest = Some(g); self.page_show(); if let Some(g) = &self.gpu { g.window.request_redraw(); } true
    }
    /// The page's strokes as the gesture has them now go into the record, outside the history (the release makes the one step).
    fn page_show(&mut self) { let Some(g) = &self.files.gest else { return }; if !g.changed { return } if let Some(d) = ink::page_set(&g.before, g.page, g.size, &g.lines) { let _ = self.ed.doc.set_kind(g.block, Kind::Media { descriptor: d }); } }
    fn page_point(&self, x: f64, y: f64) -> Option<(kurbo::Point, f64, f64)> { let g = self.files.gest.as_ref()?; let (_, frame, _) = self.files.iblock.as_ref()?; let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); let k = g.size.0 / frame.width().max(1e-9); Some((kurbo::Point::new((x - frame.x0) * k, (y - frame.y0) * k), k, s)) }
    fn page_drag(&mut self, x: f64, y: f64) {
        let Some((p, k, s)) = self.page_point(x, y) else { return }; let reach = (self.pen_now().1 / 2.0).max(5.0) * s * k;
        if self.files.gest.as_mut().is_some_and(|g| g.drag(p, pen_pressure(), reach)) { self.page_show(); } if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The release: the record goes back to what it was and, when the page's strokes changed, takes them as ONE step of the note's history.
    fn page_release(&mut self, x: f64, y: f64) {
        let Some((p, k, s)) = self.page_point(x, y) else { self.files.gest = None; return }; let (pen, w) = self.pen_now(); let Some(mut g) = self.files.gest.take() else { return };
        let changed = g.release(p, pen, w * s * k, 2.0 * s * k); let _ = self.ed.doc.set_kind(g.block, Kind::Media { descriptor: g.before.clone() });
        if changed { self.page_keep(g.block, &g.before, g.page, g.size, &g.lines); } if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn page_keep(&mut self, block: BlockId, before: &str, page: usize, size: (f64, f64), lines: &[ink::Line]) {
        if let Some(d) = ink::page_set(before, page, size, lines) { if d != before { let t = self.now_ms(); let dd = self.ed.set_kinds(vec![(block, Kind::Media { descriptor: d })], t).unwrap_or(Dirty::Nothing); self.apply(dd); } }
    }
    /// The picked stroke is changed (None from `f`: removed) - one step. True: there was one.
    fn page_picked(&mut self, f: impl Fn(&mut ink::Line) -> bool) -> bool {
        let Some((bid, sid)) = self.files.psel.clone() else { return false }; let Some((id, frame, d)) = self.ink_block().filter(|b| b.0 == bid) else { return false };
        let rec: serde_json::Value = serde_json::from_str(&d).unwrap_or_default(); let (size, page) = (ink::page_size(&rec, frame), ink::page_no(&rec)); let mut lines = ink::page_lines(&rec, page, size);
        let Some(i) = lines.iter().position(|l| l.s.id == sid) else { return false }; if !f(&mut lines[i]) { lines.remove(i); self.files.psel = None; }
        self.page_keep(id, &d, page, size, &lines); if let Some(g) = &self.gpu { g.window.request_redraw(); } true
    }
    /// A key while a picture or a PDF block is the current one: the tools' keys (the canvas's), Escape, Delete. True: it was the page's.
    fn page_key(&mut self, key: &Key) -> bool {
        let (_, cmd, alt) = self.mods; if cmd || alt || self.ctrl || self.files.vshown.is_some() || self.files.link.is_some() || self.files.picker.is_some() || self.files.find.is_some() || self.sidebar.as_ref().is_some_and(|sb| sb.typing()) { return false }
        if self.ink_block().is_none() { return false } use ink::{Ink as I, Pick as P};
        let done = match key {
            Key::Named(NamedKey::Escape) => { if let Some(g) = self.files.gest.take() { let _ = self.ed.doc.set_kind(g.block, Kind::Media { descriptor: g.before }); true } else if self.files.psel.take().is_some() { true } else { self.files.pick.take().is_some() } }
            Key::Named(NamedKey::Backspace | NamedKey::Delete) if self.files.psel.is_some() => self.page_picked(|_| false),
            Key::Character(c) => { let to = match c.to_lowercase().as_str() { "v" => Some(P::Select), "p" => Some(P::Draw(I::Pen)), "l" => Some(P::Draw(I::Line)), "a" => Some(P::Draw(I::Arrow)), "r" => Some(P::Draw(I::Rect)), "o" => Some(P::Draw(I::Oval)), "e" => Some(P::Draw(I::Eraser)), _ => None };
                match to { Some(p) => { self.files.pick = if self.files.pick == Some(p) { None } else { Some(p) }; self.files.psel = None; true } None => false } }
            _ => false };
        if done { self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } } done
    }
    /// For scripted checks (`MN_PAGE_INK=1`): on the current picture or PDF page a pen's line, a rectangle and an oval are drawn as
    /// the pointer would; part of the line is rubbed out; a step is taken back and made again.
    fn page_ink_test(&mut self) {
        let Some((id, f, _)) = self.ink_block() else { return }; use ink::{Ink as I, Pick as P}; let at = |u: f64, v: f64| (f.x0 + f.width() * u, f.y0 + f.height() * v);
        let count = |me: &App| me.ed.doc.get(id).and_then(|b| match &b.kind { Kind::Media { descriptor } => serde_json::from_str::<serde_json::Value>(descriptor).ok(), _ => None }).map(|r| { let p = ink::page_no(&r); r["ink"][p.to_string()]["shapes"].as_array().map(|a| a.len()).unwrap_or(0) }).unwrap_or(0);
        let go = |me: &mut App, pick: P, pts: Vec<(f64, f64)>| { me.files.pick = Some(pick); let p0 = at(pts[0].0, pts[0].1); if !me.page_press(p0.0, p0.1) { println!("page ink: the press was not taken"); return } for q in &pts[1..] { let p = at(q.0, q.1); std::thread::sleep(std::time::Duration::from_millis(2)); me.page_drag(p.0, p.1); } let l = at(pts[pts.len() - 1].0, pts[pts.len() - 1].1); me.page_release(l.0, l.1); };
        go(self, P::Draw(I::Pen), (0..=60).map(|i| { let u = 0.1 + 0.8 * i as f64 / 60.0; (u, 0.3 + 0.12 * (u * 14.0).sin()) }).collect()); let a = count(self);
        go(self, P::Draw(I::Rect), vec![(0.15, 0.55), (0.45, 0.85)]); go(self, P::Draw(I::Oval), vec![(0.55, 0.55), (0.9, 0.85)]); let b = count(self);
        go(self, P::Draw(I::Eraser), vec![(0.5, 0.1), (0.5, 0.5)]); let c = count(self);
        let d = self.ed.undo().unwrap_or(Dirty::Nothing); self.apply(d); let u = count(self); let d = self.ed.redo().unwrap_or(Dirty::Nothing); self.apply(d);
        println!("page ink: strokes after the pen {a}, after the shapes {b}, after the eraser {c}, after undo {u}, after redo {}", count(self)); self.files.pick = None; self.layout = None;
    }
    fn pdf_under(&self, x: f64, y: f64) -> Option<(BlockId, Rect, Rect, String)> {
        if self.blank || self.modal() || self.bar.hit(x, y) || self.foot.hit(x, y) || x < self.side_w || y < self.top { return None }
        let l = self.layout.as_ref()?; let (nx, ny) = self.note_point((x, y)); let (id, _) = l.hit(&self.ed.doc, nx, ny)?;
        self.pdf_boxes(id)
    }
    fn pdf_boxes(&self, id: BlockId) -> Option<(BlockId, Rect, Rect, String)> {
        let (l, g) = (self.layout.as_ref()?, self.gpu.as_ref()?); let s = g.window.scale_factor();
        let Some(Kind::Media { descriptor }) = self.ed.doc.get(id).map(|b| &b.kind) else { return None };
        if serde_json::from_str::<serde_json::Value>(descriptor).ok()?.get("kind").and_then(|k| k.as_str()) != Some("pdf") { return None }
        let r = l.rect(&self.ed.doc, id)? + origin(&self.files.card, self.side_w, self.top, self.scroll, s); let cut = r.y1 - mn_layout::PDF_STRIP as f64 * s;
        Some((id, Rect::new(r.x0, r.y0, r.x1, cut), Rect::new(r.x0, cut, r.x1, r.y1), descriptor.clone()))
    }
    /// The character of a PDF block's page under a window point (the nearest when the point is beside the text).
    fn pdf_char(&self, id: BlockId, x: f64, y: f64) -> Option<usize> {
        let (_, page, _, d) = self.pdf_boxes(id)?; let t = self.media.page_text(&d)?; if t.chars.is_empty() || page.width() <= 0.0 { return None }
        let k = t.w as f64 / page.width(); t.at(((x - page.x0) * k) as f32, ((y - page.y0) * k) as f32)
    }
    /// A PDF block shows another page. The page is the block's own (kept in its record) but turning it is not a step
    /// of the history.
    fn pdf_turn(&mut self, id: BlockId, by: i64) {
        let Some(Kind::Media { descriptor }) = self.ed.doc.get(id).map(|b| b.kind.clone()) else { return };
        let Ok(serde_json::Value::Object(mut v)) = serde_json::from_str::<serde_json::Value>(&descriptor) else { return };
        let pages = v.get("pages").and_then(|p| p.as_i64()).unwrap_or(1).max(1); let now = v.get("page").and_then(|p| p.as_i64()).unwrap_or(0);
        let to = (now + by).clamp(0, pages - 1); if to == now { return }
        v.insert("page".into(), serde_json::json!(to));
        let _ = self.ed.doc.set_kind(id, Kind::Media { descriptor: serde_json::Value::Object(v).to_string() });
        self.files.pdf = None;
        if let Some(st) = self.store.as_mut() { let first = st.edited.is_none() && !st.unpublished; st.edited = Some(Instant::now()); if first { self.title(); } }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// A pointer event goes to the wide tables' bars first: true when a bar took it (a press on its thumb, a drag).
    fn table_bar_event(&mut self, e: min_ui::Event) -> bool {
        if self.blank || self.modal() { return false }
        let (Some(l), Some(g)) = (self.layout.as_ref(), self.gpu.as_ref()) else { return false }; let s = g.window.scale_factor(); let now = Instant::now();
        let (w, h) = g.presenter.size(); let seen = match &self.files.card { Some(c) => c.rect, None => Rect::new(self.side_w, self.top, w as f64, h as f64 - self.bottom) };
        let (mut mine, mut redraw) = (false, false); let top = self.top;
        // the note's own bar first: its thumb is dragged
        if self.files.card.is_none() { let b = &mut self.files.pbar; b.always = true; b.set(seen, l.height as f64 + 2.0 * PAD * s, s); b.offset = self.scroll.clamp(0.0, b.max());
            if !matches!(e, min_ui::Event::Down { y, .. } if y < top) { let (changed, m) = b.event(&e, now); if (b.offset - self.scroll).abs() > 0.25 { self.scroll = b.offset; } if m || changed { if let Some(g) = &self.gpu { g.window.request_redraw(); } } if m { self.set_cursor(); return true } } }
        let (Some(l), Some(g)) = (self.layout.as_ref(), self.gpu.as_ref()) else { return false }; let _ = g;
        sync_bars(&mut self.files.tbars, l, &self.ed.doc, origin(&self.files.card, self.side_w, self.top, self.scroll, s), s, seen);
        for (id, b) in self.files.tbars.iter_mut() { if matches!(e, min_ui::Event::Down { y, .. } if y < top) { continue }
            let (changed, m) = b.event(&e, now); mine |= m; redraw |= changed || m;
            if (b.offset as f32 - self.files.tscroll.get(id).copied().unwrap_or(0.0)).abs() > 0.25 { self.files.tscroll.insert(*id, b.offset as f32); } }
        if redraw { let map = self.files.tscroll.clone(); if let Some(l) = self.layout.as_mut() { l.set_table_scrolls(&map); } if let Some(g) = &self.gpu { g.window.request_redraw(); } self.set_cursor(); }
        mine
    }
    /// Find in the note: the bar opens (or takes the keys again), seeded with the selection when it is a few words of one line.
    fn find_open(&mut self) {
        if self.blank { return } let Some(g) = self.gpu.as_mut() else { return };
        let seed = Some(self.ed.selected_text()).filter(|t| !t.is_empty() && !t.contains('\n') && t.len() <= 80 && self.ed.cell_rect().is_none());
        match self.files.find.as_mut() { Some(f) => { if let Some(t) = &seed { f.query.set_text_end(t, &mut g.fonts); } f.focus(&mut g.fonts); }
            None => { let last = self.files.found_for.as_ref().map(|x| x.0.clone()).unwrap_or_default(); self.files.find = Some(findbar::FindBar::new(seed.as_deref().unwrap_or(&last), &mut g.fonts)); } }
        self.files.found_for = None; g.window.request_redraw();
    }
    /// The search across the collection: begun again when the sidebar's field changes, and a few notes are read each
    /// frame (at most ~6 ms of them) until all were - the page stays live meanwhile.
    fn search_step(&mut self) {
        let Some(sb) = self.sidebar.as_mut() else { return }; let q = sb.query.text().trim().to_string();
        if sb.query.focused { if let Some(f) = self.files.find.as_mut() { f.blur(); } }
        if q.is_empty() { if sb.results.take().is_some() { sb.rail.scroll = 0.0; } sb.scanning = false; self.files.search = None; return }
        let Some(st) = self.store.as_ref() else { return };
        if self.files.search.as_ref().is_none_or(|x| x.query != q) { self.files.search = Some(search::Search::new(&q, &st.col)); sb.rail.scroll = 0.0; sb.results = Some(vec![]); sb.scanning = true; }
        let Some(se) = self.files.search.as_mut() else { return }; if se.done() { return }
        let t0 = Instant::now();
        while let Some((id, _)) = se.next() { 
            let doc = if id == st.note && !self.blank { Some(self.ed.doc.clone()) } else { st.col.load_note(id).ok() };
            // a video's notes count as the note's text - those that were read already (a video never shown has none yet)
            let extra: Vec<String> = doc.as_ref().map(|d| d.walk().iter().filter_map(|b| match &b.kind { Kind::Media { descriptor } if descriptor.contains("\"kind\":\"video\"") => Some(descriptor.clone()), _ => None }).collect::<Vec<_>>()).unwrap_or_default()
                .iter().flat_map(|d| self.media.notes(d).iter().map(|n| n.text.clone()).collect::<Vec<_>>()).collect();
            se.took(doc.as_ref(), &extra); if t0.elapsed().as_millis() >= 6 { break } }
        sb.results = Some(se.results()); sb.scanning = !se.done();
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn find_close(&mut self) { self.files.find = None; self.files.hits.clear(); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
    /// The occurrences are found again when the words, the case or the note changed; the current one stays the first at or after the caret.
    fn find_refresh(&mut self) {
        let Some(f) = self.files.find.as_ref() else { return }; let note = self.store.as_ref().map(|st| st.note).unwrap_or_else(BlockId::new);
        let key = (f.query.text().to_string(), f.case, self.ed.stamp(), note); if self.files.found_for.as_ref() == Some(&key) { return }
        let typed = self.files.found_for.as_ref().is_none_or(|k| k.0 != key.0 || k.1 != key.1 || k.3 != key.3);
        let mut hits = self.ed.find(&key.0, key.1);
        // a video's notes: the block is a hit when one of its notes holds the words
        let videos: Vec<(BlockId, String)> = self.ed.doc.walk().iter().filter_map(|b| match &b.kind { Kind::Media { descriptor } if descriptor.contains("\"kind\":\"video\"") => Some((b.id, descriptor.clone())), _ => None }).collect();
        if !key.0.is_empty() { let order: Vec<BlockId> = self.ed.doc.walk().iter().map(|b| b.id).collect(); let mut extra = false;
            for (id, d) in videos { if hits.iter().any(|h| h.block == id) { continue } if self.media.notes(&d).iter().any(|n| mn_edit::find::holds(&n.text, &key.0, key.1)) { hits.push(mn_edit::find::Hit { block: id, start: 0, end: 0, kind: mn_edit::find::HitKind::Block }); extra = true; } }
            if extra { hits.sort_by_key(|h| (order.iter().position(|x| *x == h.block).unwrap_or(0), h.start)); } }
        // the current one: where the caret is (typing in the bar goes to the first at or after it; an edit keeps the place)
        let caret = self.ed.selection().map(|x| if typed { x.anchor } else { x.focus }); let order: Vec<BlockId> = self.ed.doc.walk().iter().map(|b| b.id).collect(); let place = |id: BlockId| order.iter().position(|x| *x == id).unwrap_or(0);
        self.files.hit_at = caret.and_then(|c| hits.iter().position(|h| (place(h.block), h.end) >= (place(c.block), c.at))).unwrap_or(0);
        self.files.hits = hits; self.files.found_for = Some(key);
        if typed { self.find_show(); }
    }
    /// The current occurrence is selected and brought into view.
    fn find_show(&mut self) {
        let Some(h) = self.files.hits.get(self.files.hit_at).copied() else { return }; use mn_edit::{Pos, Selection};
        self.ed.set_selection(Some(Selection { anchor: Pos { block: h.block, at: h.start }, focus: Pos { block: h.block, at: h.end } })); self.follow = true;
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn find_out(&mut self, out: findbar::Out) {
        use findbar::Out as O; let n = self.files.hits.len(); let now = self.now_ms();
        match out { O::None => return, O::Redraw => {}, O::Close => self.find_close(),
            O::Changed => { self.files.found_for = None; self.find_refresh(); }
            O::Next | O::Prev => { self.find_refresh(); let n = self.files.hits.len(); if n > 0 { self.files.hit_at = if matches!(out, O::Next) { (self.files.hit_at + 1) % n } else { (self.files.hit_at + n - 1) % n }; self.find_show(); } }
            O::Replace | O::ReplaceAll => { let with = self.files.find.as_ref().map(|f| f.with.text().to_string()).unwrap_or_default();
                let hits: Vec<mn_edit::find::Hit> = if matches!(out, O::ReplaceAll) { self.files.hits.clone() } else { self.files.hits.get(self.files.hit_at).copied().into_iter().collect() }; if n == 0 { return }
                match self.ed.replace_hits(&hits, &with, now) { Ok((d, done)) => { self.apply(d);
                    if matches!(out, O::ReplaceAll) { self.notice = Some((if done == hits.len() { format!("Replaced {done}") } else { format!("Replaced {done} of {} \u{2014} chips, choice cells and link addresses are left alone", hits.len()) }, Instant::now() + std::time::Duration::from_secs(6))); }
                    else if done == 0 { self.notice = Some(("This one cannot be replaced (a chip, a choice cell, a link's address or a name)".to_string(), Instant::now() + std::time::Duration::from_secs(6))); }
                    // the one after what was replaced
                    let at = self.files.hit_at; self.files.found_for = None; let caret = self.ed.selection(); self.find_refresh(); let _ = caret; let n = self.files.hits.len(); if n > 0 { self.files.hit_at = if done == 0 { (at + 1) % n } else { at.min(n - 1) }; self.find_show(); } else if let Some(f) = self.ed.selection().map(|x| x.focus) { self.ed.set_selection(Some(mn_edit::Selection::caret(f))); } }
                    Err(e) => eprintln!("replace: {e:?}") } } }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The link's editor opens at the caret: on the link the caret is in, on the selection, or for a new link.
    fn link_open(&mut self) {
        if self.blank || self.ed.in_blocks() || !self.ensure() { return } let Some(l) = self.layout.as_ref() else { return };
        if matches!(self.ed.state().kind, Some(Kind::Code { .. })) || self.ed.cell_rect().is_some() { return }
        let here = self.ed.link_here();
        let anchor = match &here { Some((id, s, e, ..)) => l.range_rects(&self.ed.doc, *id, *s, *e).into_iter().next(), None => self.ed.selection_rects(l).into_iter().next() }.or_else(|| self.ed.caret_rect(l, 1.0));
        let Some(anchor) = anchor else { return }; let Some(g) = self.gpu.as_mut() else { return };
        let (url, text) = match &here { Some((.., u, t)) => (u.clone(), t.clone()), None => { let t = self.ed.selected_text(); (String::new(), if t.contains('\n') { String::new() } else { t }) } };
        self.files.link = Some(linkedit::LinkEdit::new(anchor, &url, &text, here.is_some(), &mut g.fonts)); g.window.request_redraw();
    }
    fn link_out(&mut self, out: linkedit::Out) {
        let now = self.now_ms();
        let d = match out { linkedit::Out::None => return, linkedit::Out::Redraw => Ok(Dirty::Nothing), linkedit::Out::Close => { self.files.link = None; Ok(Dirty::Nothing) }
            linkedit::Out::Open(u) => { open::url(&u); Ok(Dirty::Nothing) }
            linkedit::Out::Remove => { self.files.link = None; self.ed.remove_link(now) }
            linkedit::Out::Done { url, text } => { self.files.link = None; self.ed.set_link(&url, Some(&text), now) } };
        let d = d.unwrap_or(Dirty::Nothing); if d != Dirty::Nothing { self.follow = true; self.apply(d); }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The file chip under a window point: its block and its record.
    fn file_chip_under(&self, x: f64, y: f64) -> Option<(BlockId, String)> {
        if self.blank || y < self.top || x < self.side_w { return None } let l = self.layout.as_ref()?; let (nx, ny) = self.note_point((x, y));
        let (id, _) = l.hit(&self.ed.doc, nx, ny)?; let Some(Kind::Media { descriptor }) = self.ed.doc.get(id).map(|b| &b.kind) else { return None };
        (descriptor.contains("\"kind\":\"file\"") && l.rect(&self.ed.doc, id)?.contains(kurbo::Point::new(nx, ny))).then(|| (id, descriptor.clone()))
    }
    /// A file chip's file is opened in its own application (`reveal`: shown in its folder); a missing one is said.
    fn file_open(&mut self, descriptor: &str, reveal: bool) {
        let st = self.media.file_state(descriptor);
        match st.path.filter(|_| st.found) { Some(p) => if reveal { open::reveal(&p) } else { open::file(&p) },
            None => { self.notice = Some(("The file is missing \u{2014} right-click it to locate it".to_string(), Instant::now() + std::time::Duration::from_secs(6))); if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
    }
    /// A choice chip goes in at the caret and its picker opens (the toolbar's tag, Option+Cmd+C).
    fn chip_insert(&mut self) {
        if self.blank || !self.ensure() { return } let now = self.now_ms(); self.follow = true;
        let d = self.ed.insert_chip(now).unwrap_or(Dirty::Nothing); if d == Dirty::Nothing { return } self.apply(d); self.layout = None; self.ensure();
        if let Some(c) = self.ed.chip_before_caret() { self.chip_open(c, ""); }
    }
    /// A chip's picker opens under it.
    fn chip_open(&mut self, c: mn_edit::chips::Chip, typed: &str) {
        let Some(g) = self.gpu.as_mut() else { return };
        self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: c.0, at: c.2 })));
        let mut p = choice::ChoicePicker::new(c.0, c.3.clone(), Some(c.4.clone()), typed, &mut g.fonts); p.inline = true;
        self.files.picker = Some(p); self.files.picker_chip = Some(c); g.window.request_redraw();
    }
    /// The chip under a window point.
    fn chip_under(&self, x: f64, y: f64) -> Option<mn_edit::chips::Chip> {
        if self.blank || y < self.top || x < self.side_w { return None } let l = self.layout.as_ref()?; let (nx, ny) = self.note_point((x, y));
        let (id, at) = l.hit(&self.ed.doc, nx, ny)?; let c = self.ed.chip_span_at(id, at)?;
        l.range_rects(&self.ed.doc, id, c.1, c.2).iter().any(|r| r.inflate(5.0, 2.0).contains(kurbo::Point::new(nx, ny))).then_some(c)
    }
    /// The link under a window point: its address.
    fn link_under(&self, x: f64, y: f64) -> Option<String> {
        if self.blank || y < self.top || x < self.side_w { return None } let l = self.layout.as_ref()?; let (nx, ny) = self.note_point((x, y));
        let (id, at) = l.hit(&self.ed.doc, nx, ny)?; let (s, e, u) = self.ed.link_span_at(id, at)?;
        l.range_rects(&self.ed.doc, id, s, e).iter().any(|r| r.inflate(1.0, 2.0).contains(kurbo::Point::new(nx, ny))).then_some(u)
    }
    /// The caret's choice cell opens its picker (`typed` goes into its field); `edit`: on its options' editor.
    fn choice_open(&mut self, typed: &str, edit: bool) {
        let Some(f) = self.ed.selection().map(|x| x.focus.block) else { return }; let Some((_, _, _, cell, header, ColumnKind::Choice { options })) = mn_edit::typed::column_of(&self.ed.doc, f) else { return };
        if header && !edit { return } let Some(g) = self.gpu.as_mut() else { return };
        let p = choice::ChoicePicker::new(cell, options, self.ed.cell_value(cell).filter(|_| !header), typed, &mut g.fonts);
        self.files.picker = Some(if edit { p.editing() } else { p }); g.window.request_redraw();
    }
    fn choice_out(&mut self, out: choice::Out) {
        let now = self.now_ms();
        // a chip in running text (row 320): the pick, the new option and the edited set are the chip's own
        if let Some(c) = self.files.picker_chip.clone() {
            let all = self.files.picker.as_ref().is_some_and(|p| p.all);
            let d = match out { choice::Out::None => return, choice::Out::Redraw => { if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                choice::Out::Close => Ok(Dirty::Nothing),
                choice::Out::Pick(v) => self.ed.chip_set(&c, v.as_deref(), None, now),
                choice::Out::Add(l) => { let l = mn_edit::typed::clean_label(&l); if l.is_empty() { Ok(Dirty::Nothing) } else { let mut set = c.3.clone(); let id = BlockId::new().to_string();
                    set.push(ChoiceOption { id: id.clone(), label: l, color: mn_edit::typed::PALETTE[c.3.len() % mn_edit::typed::PALETTE.len()].to_string() }); self.ed.chip_set(&c, Some(&id), Some(&set), now) } }
                choice::Out::Options(o) => if o.is_empty() { Ok(Dirty::Nothing) } else if all { self.ed.chips_replace_set(&c.3, &o, now) } else { let keep = o.iter().any(|x| x.id == c.4).then(|| c.4.clone()); self.ed.chip_set(&c, keep.as_deref(), Some(&o), now) } };
            self.files.picker = None; self.files.picker_chip = None;
            let d = d.unwrap_or(Dirty::Nothing); if d != Dirty::Nothing { self.apply(d); self.layout = None; }
            if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        let d = match out { choice::Out::None => return, choice::Out::Redraw => Ok(Dirty::Nothing), choice::Out::Close => { self.files.picker = None; Ok(Dirty::Nothing) }
            choice::Out::Pick(v) => { self.files.picker = None; self.ed.cell_choose(v.as_deref(), now) }
            choice::Out::Add(l) => { self.files.picker = None; self.ed.cell_choose_new(&l, now) }
            choice::Out::Options(o) => { self.files.picker = None; self.ed.table_set_options(o, now) } };
        let d = d.unwrap_or(Dirty::Nothing); if d != Dirty::Nothing { self.apply(d); self.layout = None; }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The caret's column becomes a kind (0 text, 1 choice, 2 check, 3 timecode); asks first when cells would lose content.
    fn column_kind(&mut self, k: u8, sure: bool) {
        use min_ui::text_button::Variant as V;
        let n = if k == 0 { 0 } else { self.ed.table_kind_loss() };
        if n > 0 && !sure { let what = ["text", "choice", "check", "timecode"][k.min(3) as usize];
            self.ask(Ask::Kind(k), &format!("Make this a {what} column?"), &format!("{n} cell{} in this column hold{} more than one line of plain text. Each is reduced to one line; formatting, pictures, lists and extra paragraphs are removed.", if n == 1 { "" } else { "s" }, if n == 1 { "s" } else { "" }), vec![("Cancel", V::Secondary), ("Change Column", V::Negative)]); return }
        let kind = match k { 1 => ColumnKind::Choice { options: vec![] }, 2 => ColumnKind::Check, 3 => ColumnKind::Timecode { fps: 24.0 }, _ => ColumnKind::Text };
        let now = self.now_ms(); self.follow = true; let d = self.ed.table_set_kind(kind, now).unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The caret left a timecode cell: what was typed there is written as a timecode.
    fn settle_timecode(&mut self) {
        // a typed address became a word (a space after it): it is made a link, a step of its own (row 320)
        if std::mem::take(&mut self.files.autolink) && !self.blank { let now = self.now_ms(); let d = self.ed.autolink(true, now).unwrap_or(Dirty::Nothing); if d != Dirty::Nothing { self.apply(d); } }
        let cur = self.ed.selection().map(|x| x.focus.block); if cur == self.files.tc_last { return }
        let prev = std::mem::replace(&mut self.files.tc_last, cur.filter(|b| matches!(mn_edit::typed::typed_of(&self.ed.doc, *b), Some(t) if matches!(t.kind, ColumnKind::Timecode { .. }))));
        if let Some(p) = prev { let now = self.now_ms(); let d = self.ed.settle_timecode(p, now).unwrap_or(Dirty::Nothing); if d != Dirty::Nothing { self.apply(d); } }
    }
    /// A press in a typed cell: on a check's box it goes to its next state; in a choice cell its picker opens. True: taken.
    fn typed_click(&mut self) -> bool {
        let (x, y) = self.note_point(self.pointer); let Some(l) = self.layout.as_ref() else { return false }; let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
        let Some((id, _)) = l.hit(&self.ed.doc, x, y) else { return false }; let Some(t) = mn_edit::typed::typed_of(&self.ed.doc, id) else { return false };
        let (Some(cr), Some(p)) = (l.rect(&self.ed.doc, t.cell), self.ed.doc.children(Some(t.cell)).first().copied()) else { return false };
        if !cr.contains(kurbo::Point::new(x, y)) { return false }
        match t.kind {
            ColumnKind::Check => { let Some(pr) = l.rect(&self.ed.doc, p) else { return false }; if x > pr.x0 + 20.0 * s { return false }
                self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: p, at: 0 }))); let now = self.now_ms(); let d = self.ed.cell_check_cycle(now).unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None;
                if let Some(g) = &self.gpu { g.window.request_redraw(); } true }
            ColumnKind::Choice { .. } => { self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: p, at: 0 }))); self.choice_open("", false); true }
            _ => false,
        }
    }
    /// The gap between two columns under a window point: (the column before it, the one after, the px the columns share, the sum of their shares).
    fn column_gap_at(&self, x: f64, y: f64) -> Option<(BlockId, BlockId, f64, f32)> {
        if self.blank || self.modal() || y < self.top { return None } let (l, g) = (self.layout.as_ref()?, self.gpu.as_ref()?); let s = g.window.scale_factor();
        let off = origin(&self.files.card, self.side_w, self.top, self.scroll, s);
        for id in self.ed.doc.children(None) { if self.ed.doc.get(*id).map(|b| &b.kind) != Some(&Kind::Columns) || l.is_stacked(*id) { continue }
            let Some(r) = l.rect(&self.ed.doc, *id).map(|r| r + off) else { continue }; if y < r.y0 || y > r.y1 { continue }
            let kids = self.ed.doc.children(Some(*id)); let rects: Vec<Rect> = kids.iter().filter_map(|c| l.rect(&self.ed.doc, *c).map(|q| q + off)).collect(); if rects.len() != kids.len() { continue }
            for k in 0..kids.len().saturating_sub(1) { if x >= rects[k].x1 - 3.0 * s && x <= rects[k + 1].x0 + 3.0 * s {
                let total: f32 = kids.iter().map(|c| match self.ed.doc.get(*c).map(|b| &b.kind) { Some(Kind::Column { width }) => *width, _ => 0.0 }).sum();
                return Some((kids[k], kids[k + 1], rects.iter().map(|q| q.width()).sum(), total)) } } }
        None
    }
    fn table_ui(&self) -> Option<TableUi> {
        if self.blank || self.modal() || self.files.picker.is_some() { return None }
        let (l, g) = (self.layout.as_ref()?, self.gpu.as_ref()?); let s = g.window.scale_factor();
        table_ui(&self.ed, l, s, origin(&self.files.card, self.side_w, self.top, self.scroll, s))
    }
    /// The menu of the caret's row (`column` false) or column opens at a window point.
    fn table_menu(&mut self, column: bool, x: f64, y: f64) {
        use min_ui::menu::MenuItem as I;
        let Some((table, r, c)) = self.ed.table_pos() else { return }; let Some((head, cols)) = self.ed.table_spec(table) else { return }; let rows = self.ed.doc.children(Some(table)).len();
        let items = if column { let a = cols.get(c).map(|x| x.align).unwrap_or(0);
            vec![I::action("Insert Column Left", true), I::action("Insert Column Right", true), I::Separator, I::action("Move Column Left", c > 0), I::action("Move Column Right", c + 1 < cols.len()), I::action("Duplicate Column", true), I::Separator,
                I::action(if a == 0 { "\u{2713} Align Left" } else { "Align Left" }, true), I::action(if a == 1 { "\u{2713} Align Centre" } else { "Align Centre" }, true), I::action(if a == 2 { "\u{2713} Align Right" } else { "Align Right" }, true),
                I::action("Automatic Width", cols.get(c).is_some_and(|x| x.width.is_some())), I::Separator]
                .into_iter().chain({ let k = cols.get(c).map(|x| x.kind.clone()).unwrap_or(ColumnKind::Text); let tick = |on: bool, l: &str| I::action(&if on { format!("\u{2713} {l}") } else { l.to_string() }, true);
                    let mut v = vec![tick(k == ColumnKind::Text, "Text Column"), tick(matches!(k, ColumnKind::Choice { .. }), "Choice Column"), tick(k == ColumnKind::Check, "Check Column"), tick(matches!(k, ColumnKind::Timecode { .. }), "Timecode Column")];
                    if matches!(k, ColumnKind::Choice { .. }) { v.push(I::Separator); v.push(I::action("Edit Options\u{2026}", true)); }
                    if let ColumnKind::Timecode { fps } = k { v.push(I::Separator); for r in [23.976, 24.0, 25.0, 29.97, 30.0, 60.0] { v.push(tick((fps - r).abs() < 0.005, &format!("{r} fps"))); } }
                    v }).chain([I::Separator, I::action("Delete Column", cols.len() > 1)]).collect() }
        else { vec![I::action("Insert Row Above", true), I::action("Insert Row Below", true), I::Separator, I::action("Move Row Up", r > 0), I::action("Move Row Down", r + 1 < rows), I::action("Duplicate Row", true), I::Separator,
                I::action(if head > 0 { "No Header Row" } else { "First Row Is a Header" }, true), I::Separator, I::action("Delete Row", rows > 1)] };
        let Some(g) = self.gpu.as_mut() else { return }; let (w, h) = g.presenter.size(); let s = g.window.scale_factor();
        self.files.tmenu = Some(column); self.ctx_blocks = false; self.ctx_for = None; self.ctx_notes.clear();
        // a ticked item: the tick is the menu's own (the text face has no check mark)
        let checks: Vec<Option<bool>> = items.iter().map(|it| match it { I::Action { label, .. } => Some(label.starts_with("\u{2713} ")), _ => None }).collect();
        let items: Vec<I> = items.into_iter().map(|it| match it { I::Action { label, enabled, shortcut } => I::Action { label: label.trim_start_matches("\u{2713} ").to_string(), enabled, shortcut }, x => x }).collect();
        let checks = if checks.iter().any(|c| *c == Some(true)) { checks } else { vec![] };
        self.ctx.open_checked_at(items, checks, &mut g.fonts, x, y, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), s);
        #[cfg(target_os = "macos")] text_element::changed(false);
        g.window.request_redraw();
    }
    /// The picture that is current - the caret is on it, or it is the one selected block - with its box in the window.
    fn picture(&self) -> Option<(BlockId, Rect)> {
        if self.blank || self.grab.as_ref().is_some_and(|g| g.dragging) || self.ed.run_len() != 1 { return None }
        let (l, g) = (self.layout.as_ref()?, self.gpu.as_ref()?); let s = g.window.scale_factor();
        let id = self.ed.handle_block()?;
        match self.ed.doc.get(id).map(|b| &b.kind) { Some(Kind::Media { descriptor }) if !descriptor.contains("\"kind\"") => {} _ => return None }
        Some((id, l.rect(&self.ed.doc, id)? + origin(&self.files.card, self.side_w, self.top, self.scroll, s)))
    }
    /// The corner of the current picture under a window point: (the picture, its box, a left corner, a top corner).
    fn corner_at(&self, x: f64, y: f64) -> Option<(BlockId, Rect, bool, bool)> {
        let (id, r) = self.picture()?; let s = self.gpu.as_ref()?.window.scale_factor(); let near = 9.0 * s;
        if y < self.top || self.bar.hit(x, y) || self.foot.hit(x, y) { return None }
        for (cx, cy, left, top) in [(r.x0, r.y0, true, true), (r.x1, r.y0, false, true), (r.x0, r.y1, true, false), (r.x1, r.y1, false, false)] { if (x - cx).abs() <= near && (y - cy).abs() <= near { return Some((id, r, left, top)) } }
        None
    }
    /// The picture being resized follows the pointer: its width in the record (`dw`, page px) between 80 and the
    /// width of the place it is in; the box keeps its left edge. Not a step of the history until the release.
    fn resize_to(&mut self, x: f64) {
        let Some(rz) = self.files.resize.as_ref() else { return }; let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
        let d = (x - rz.from_x) / s * if rz.left { -1.0 } else { 1.0 };
        let w = (rz.width + d).clamp(80.0f64.min(rz.max), rz.max).round();
        let Ok(serde_json::Value::Object(mut v)) = serde_json::from_str::<serde_json::Value>(&rz.before) else { return };
        v.insert("dw".into(), serde_json::json!(w as u32));
        let _ = self.ed.doc.set_kind(rz.id, Kind::Media { descriptor: serde_json::Value::Object(v).to_string() });
        self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The resize ends: the record goes back to what it was and the new one is committed as ONE undo step (`keep`),
    /// or it is given up (Escape).
    fn resize_end(&mut self, keep: bool) {
        let Some(rz) = self.files.resize.take() else { return };
        let now = self.ed.doc.get(rz.id).map(|b| b.kind.clone());
        let _ = self.ed.doc.set_kind(rz.id, Kind::Media { descriptor: rz.before.clone() }); self.layout = None;
        if let (true, Some(new)) = (keep, now) { if new != (Kind::Media { descriptor: rz.before }) { let t = self.now_ms(); let d = self.ed.set_kinds(vec![(rz.id, new)], t).unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None; } }
        self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// Paste (row 309 a). RICH, in this order: blocks or a text selection copied in minNotes (whole blocks after the
    /// caret's block, a selection at the caret with its formatting); in a code block, the text alone; files copied in
    /// the Finder (as a drop of them); formatted text of another application as blocks; a picture's pixels; plain
    /// text. PLAIN (Shift+Cmd+V): the text alone. MARKDOWN: the text read as Markdown source.
    fn paste(&mut self, mode: Paste) -> Dirty {
        let now = self.now_ms();
        let text = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok());
        // in a table (row 319): cells copied in minNotes, or a spreadsheet's text, fill cells from the caret's cell
        if !self.ed.in_blocks() && self.ed.table_pos().is_some() {
            let told = |me: &mut App, r: Result<(Dirty, usize), mn_doc::ChangeError>| -> Dirty { me.follow = true; me.layout = None; match r { Ok((d, left)) => { if left > 0 { me.notice = Some((format!("{left} column(s) did not fit in the table and were left out"), Instant::now() + std::time::Duration::from_secs(6))); } d } Err(e) => { eprintln!("paste: {e:?}"); Dirty::Nothing } } };
            if matches!(mode, Paste::Rich) { let html = self.clipboard.as_mut().and_then(|cb| cb.get().html().ok());
                let own = html.as_deref().and_then(clip::from_html).or_else(|| match (&text, &self.block_clip) { (Some(t), Some((bt, c))) if t == bt => Some((c.clone(), true)), _ => None });
                if let Some((c, true)) = own { if let Some(r) = self.ed.paste_table_clip(&c, now) { return told(self, r) } } }
            if let Some(t) = text.as_deref().filter(|t| t.contains('\t')) { let r = self.ed.paste_grid_text(t, now); return told(self, r) }
        }
        // into a typed cell: the text becomes the cell's value (a choice's option - a new one when there is none of that name)
        if !self.ed.in_blocks() && self.ed.typed_cell().is_some() { self.follow = true; let d = text.and_then(|t| self.ed.cell_put_text(&t, now).ok().flatten()).unwrap_or(Dirty::Nothing); if d != Dirty::Nothing { self.layout = None; } return d }
        // an address alone on the clipboard: over a selection it links the selection, at a caret it goes in as a link (row 320)
        if matches!(mode, Paste::Rich) && !self.ed.in_blocks() && self.ed.cell_rect().is_none() && !matches!(self.ed.state().kind, Some(Kind::Code { .. })) { if let Some(t) = text.as_deref().map(str::trim).filter(|t| mn_edit::links::looks_like_url(t)) { self.follow = true; return self.ed.set_link(t, None, now).unwrap_or(Dirty::Nothing) } }
        let frag = |me: &mut App, c: mn_edit::Clip| { me.follow = true; match me.ed.paste_fragment(&c, now) { Ok((d, refused)) => { if refused > 0 { me.notice = Some((format!("{refused} block(s) cannot go there and were left out"), Instant::now() + std::time::Duration::from_secs(6))); } d } Err(e) => { eprintln!("paste: {e:?}"); Dirty::Nothing } } };
        match mode {
            Paste::Plain => return text.map(|t| self.ed.insert(&t, now).unwrap_or(Dirty::Nothing)).unwrap_or(Dirty::Nothing),
            Paste::Markdown => { let c = text.as_deref().map(paste::from_markdown).unwrap_or_default(); return if c.is_empty() { Dirty::Nothing } else { frag(self, c) } }
            Paste::Rich => {}
        }
        let html = self.clipboard.as_mut().and_then(|cb| cb.get().html().ok());
        // `block_clip` is the fallback where the HTML cannot be read back
        let own = html.as_deref().and_then(clip::from_html).or_else(|| match (&text, &self.block_clip) { (Some(t), Some((bt, c))) if t == bt => Some((c.clone(), true)), _ => None });
        if let Some((c, whole)) = own {
            if !whole { return frag(self, c) }
            self.follow = true;
            return match self.ed.paste_blocks(&c, now) { Ok((d, refused)) => { if refused > 0 { eprintln!("paste: {refused} block(s) cannot go there and were left out"); } d } Err(_) => Dirty::Nothing }
        }
        if matches!(self.ed.state().kind, Some(Kind::Code { .. })) && !self.ed.in_blocks() { return text.map(|t| self.ed.insert(&t, now).unwrap_or(Dirty::Nothing)).unwrap_or(Dirty::Nothing) }
        let at = self.ed.after_caret().unwrap_or(mn_edit::Drop { parent: None, index: self.ed.doc.children(None).len(), depth: None, beside: None });
        // files: only real ones (the helper also offers lines of text that look like paths - those stay text)
        if self.store.is_some() { if let Ok(paths) = min_paths::clipboard::paste_paths() { let as_text = text.as_deref().is_some_and(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).eq(paths.iter().map(String::as_str)));
            if !paths.is_empty() && !as_text && paths.iter().all(|p| std::path::Path::new(p).exists()) { self.add_files(paths.into_iter().map(Into::into).collect(), at); return Dirty::Nothing } } }
        let pixels = |me: &mut App| -> bool { if me.store.is_none() { return false } match me.clipboard.as_mut().and_then(|cb| cb.get_image().ok()).filter(|i| i.width > 0 && i.height > 0) {
            Some(img) => { me.files.token += 1; me.files.pending.insert(me.files.token, at.clone()); me.media.add_pixels(me.files.token, img.bytes.into_owned(), img.width as u32, img.height as u32); true } None => false } };
        if let Some(c) = html.as_deref().map(paste::from_html).filter(|c| !c.is_empty()) {
            // a browser's Copy Image: the page's markup for one picture, and its pixels - the pixels are here already
            if c.len() == 1 && c[0].len() == 1 && matches!(c[0][0].kind, Kind::Media { .. }) && pixels(self) { return Dirty::Nothing }
            return frag(self, c) }
        if pixels(self) { return Dirty::Nothing }
        // a spreadsheet's text with no markup beside it: a table of its own
        if let Some(t) = text.as_deref().filter(|t| t.contains('\t') && t.trim_end().contains('\n') && !self.ed.in_blocks()) { self.follow = true; if let Ok((d, _)) = self.ed.paste_grid_text(t, now) { if d != Dirty::Nothing { self.layout = None; return d } } }
        text.map(|t| self.ed.insert(&t, now).unwrap_or(Dirty::Nothing)).unwrap_or(Dirty::Nothing)
    }
    /// Files go to the helpers to become blocks at `at`.
    fn add_files(&mut self, paths: Vec<std::path::PathBuf>, at: mn_edit::Drop) {
        if paths.is_empty() || (self.blank && self.files.canvas.is_none()) { return }
        self.files.token += 1; self.files.pending.insert(self.files.token, at); self.media.add(self.files.token, paths);
    }
    /// The page's drop place under a window point (None: over the bars, the sidebar, a dialog, or no note).
    fn file_drop_at(&self, x: f64, y: f64) -> Option<(mn_edit::Drop, Rect)> {
        // over an open canvas: the files go where the pointer is (no line is shown)
        if self.files.canvas.is_some() && self.files.card.is_none() { if self.modal() || self.store.is_none() || self.bar.hit(x, y) || self.foot.hit(x, y) || x < self.side_w || y < self.top { return None } self.files.canvas_at.set(Some((x, y))); return Some((Self::canvas_drop(), Rect::ZERO)) }
        if self.blank || self.modal() || self.store.is_none() || self.bar.hit(x, y) || self.foot.hit(x, y) || x < self.side_w || y < self.top { return None }
        let (nx, ny) = self.note_point((x, y)); self.ed.insert_at(self.layout.as_ref()?, nx, ny)
    }
    /// A command of the menu bar (row 296).
    fn menu_cmd(&mut self, c: menu::Cmd) {
        use menu::Cmd as M;
        if let (Some(cv), true) = (self.files.canvas.as_mut().filter(|_| self.files.card.is_none()), matches!(c, M::Undo | M::Redo)) { if c == M::Undo { cv.undo() } else { cv.redo() } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        if self.modal() && c != M::Quit { return }
        match c {
            M::Quit => self.request_close(Pending::Quit),
            M::CloseCollection => if self.store.is_some() { self.request_close(Pending::Close) },
            M::Open => self.panel(rfd::FileDialog::new().add_filter("minNotes collection or package", &["mnd", gather::PACKAGE]), 1, |me, p| if let Some(p) = p.into_iter().next() { me.request_close(Pending::Open(p)) }),
            M::OpenRecent(p) => self.request_close(Pending::Open(p.into())),
            M::ClearRecent => { recent_clear(); self.sync_recent(); }
            M::NewCollection => self.panel(rfd::FileDialog::new().add_filter("minNotes collection", &["mnd"]).set_file_name("Untitled.mnd"), 0, |me, p| if let Some(p) = p.into_iter().next() {
                // the system's dialog has asked about replacing: a new collection starts where nothing is, so an old file is moved aside by name, never deleted
                if p.exists() { let aside = p.with_extension(format!("replaced-{}.mnd", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))); let _ = std::fs::rename(&p, aside); }
                me.request_close(Pending::New(p)) }),
            M::SaveAs => self.save_as(),
            M::PasteMarkdown => { let d = self.paste(Paste::Markdown); self.apply(d); }
            // the selected row's name (the open note's when nothing is selected); the sidebar is opened for it
            M::Rename => if let (Some(sb), Some(g), Some(st)) = (self.sidebar.as_mut(), self.gpu.as_mut(), self.store.as_ref()) { let id = sb.selected.unwrap_or(st.note); if sb.rail.collapsed { sb.rail.set_collapsed(false, Instant::now()); } sb.start_rename(id, &mut g.fonts); },
            M::Toolbar => { self.bar.hidden = !self.bar.hidden; self.layout = None; }
            M::Footer => { self.foot.hidden = !self.foot.hidden; self.layout = None; }
            M::ToolbarWide => { self.bar_wide = !self.bar_wide; self.layout = None; }
            M::ExportHtml => self.export(0, None), M::ExportMarkdown => self.export(1, None), M::ExportDocx => self.export(2, None), M::ExportPdf => self.export(3, None), M::ExportPoster => self.export(4, None), M::CopyMarkdown => self.copy_markdown(),
            M::Import => { if self.store.is_some() { let mut dlg = rfd::FileDialog::new().add_filter("Documents", &import::extensions()); for (name, exts) in import::FILTERS { dlg = dlg.add_filter(name, exts); } self.panel(dlg, 2, |me, p| me.import_files(p)); } }
            M::CollectMedia => self.gather_open(gather::What::Collect, None), M::ExportPackage => self.gather_open(gather::What::Package, None),
            M::ExportPaths => self.export_toggle(true), M::ExportVideoNotes => self.export_toggle(false),
            other => if let Some((ch, shift)) = menu::key_of(&other) { let m = self.mods; self.mods = (shift, true, false); self.key(&Key::Character(ch.to_string().as_str().into()), None); self.mods = m; },
        }
    }
    /// A dialog's button was pressed (`i`: its place in the row).
    fn answer(&mut self, what: Ask, i: usize) {
        use min_ui::text_button::Variant as V;
        match (what, i) {
            (Ask::Delete(id), 1) => self.delete_entry(id),
            (Ask::Kind(k), 1) => self.column_kind(k, true),
            // Don't Save: the changes are given up — the session goes, the document stays as it is
            (Ask::Close, 1) => { self.changed.clear(); if let Some(st) = self.store.as_mut() { st.edited = None; st.unpublished = false; } self.proceed(); }
            (Ask::Close, 0) => { self.after_close = None; }
            (Ask::Close, 2) => { self.close_after_save = true; self.save(); }
            (Ask::Recovered, 1) => self.ask(Ask::DiscardRecovered, "Discard these changes?", "The changes from the earlier session will be removed from this machine. This cannot be undone.", vec![("Cancel", V::Secondary), ("Discard", V::Negative)]),
            (Ask::DiscardRecovered, 1) => self.discard_recovered(),
            (Ask::DiscardRecovered, _) => self.offer_recovered(),
            (Ask::SaveFailed, 1) => self.save(),
            (Ask::ImportBig, i) => self.import_big(i),
            _ => {}
        }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// Work from an earlier session was found when the file was opened (a crash, or the app was stopped): it is on
    /// screen already, and the user says whether it stays (row 272: offered back, never deleted silently).
    fn offer_recovered(&mut self) {
        use min_ui::text_button::Variant as V;
        let name = self.doc_name();
        self.ask(Ask::Recovered, "Unsaved changes were found", &format!("\u{201c}{name}\u{201d} has changes on this machine that were never saved to the document. They are from an earlier session and are shown now."), vec![("Continue with Them", V::Primary), ("Discard\u{2026}", V::Secondary)]);
    }
    /// The recovered changes go: the session is removed and the document is opened afresh.
    fn discard_recovered(&mut self) {
        let Some(st) = self.store.as_mut() else { return };
        let Some(session) = st.session.take() else { return };
        let path = session.original().to_path_buf(); let root = sessions_root();
        let _ = session.close();
        let opened = mn_store::staging::Session::open(&root, &path).and_then(|s| mn_store::Collection::open(&s.work()).map(|c| (s, c)).map_err(std::io::Error::other));
        match opened {
            Ok((session, col)) => {
                fn first_note(c: &mn_store::Collection, parent: Option<BlockId>) -> Option<BlockId> { for e in c.list(parent).ok()? { if e.kind == mn_store::DocKind::Note { return Some(e.id) } if let Some(n) = first_note(c, Some(e.id)) { return Some(n) } } None }
                let note = first_note(&col, None).unwrap_or(st.note);
                self.ed = Editor::new(col.load_note(note).unwrap_or_default()); self.others.clear();
                *st = Store { session: Some(session), col, note, title: String::new(), edited: None, unpublished: false, edited_since_stage: false };
                if let Some(sb) = self.sidebar.as_mut() { sb.set_tree(&st.col); sb.selected = None; }
                self.layout = None; self.flat = None; self.scroll = 0.0; self.recovered = false;
                #[cfg(target_os = "macos")] text_element::changed(true);
            }
            Err(e) => { use min_ui::text_button::Variant as V; self.ask(Ask::Notice, "The document could not be opened again", &format!("{e}"), vec![("OK", V::Primary)]); }
        }
        self.title();
    }
    /// A save ended without the document being written.
    fn save_failed(&mut self, why: &str) {
        use min_ui::text_button::Variant as V;
        self.close_after_save = false; self.status = Some(("Save failed \u{2014} your changes are kept on this machine".to_string(), true));
        let name = self.doc_name();
        self.ask(Ask::SaveFailed, &format!("Couldn\u{2019}t save \u{201c}{name}\u{201d}"), &format!("{why} Your changes are kept on this machine."), vec![("OK", V::Secondary), ("Retry", V::Primary)]);
    }

    // ── the tree (row 290)
    /// Opens another note: the one being left goes to the staged copy first and keeps its editor (its undo history)
    /// for as long as the window is open.
    /// What changed on the open canvas goes to the collection (`leaving`: its camera too, and the canvas is not being dragged on).
    fn canvas_save(&mut self, leaving: bool) {
        if std::mem::take(&mut self.files.creload) { self.card_leave(); if let (Some(cid), Some(st)) = (self.files.canvas.as_ref().map(|c| c.id), self.store.as_ref()) { if let Ok(c) = st.col.load_canvas(cid) { self.files.canvas = Some(canvas::Canvas::new(cid, c)); self.files.cards.clear(); } } return }
        let (Some(cv), Some(st)) = (self.files.canvas.as_mut(), self.store.as_mut()) else { return }; if !cv.dirty && !leaving { return } if self.pressed && !leaving { return }
        let changed = cv.dirty; match st.col.save_canvas(cv.id, &cv.stored()) { Ok(()) => { cv.dirty = false; if changed { st.unpublished = true; st.edited_since_stage = true; self.changed.insert(cv.id); self.title(); } } Err(e) => eprintln!("canvas: {e}") }
    }
    /// The canvas gives the page's place back to the note.
    fn canvas_leave(&mut self) { if self.files.canvas.is_none() { return } self.video_stop(); self.card_leave(); self.files.cards.clear(); if let Some(cv) = self.files.canvas.as_mut() { cv.edit_end(true); } self.canvas_save(true); self.files.canvas = None; self.blank = self.store.is_none(); self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
    /// A canvas of the collection is opened in the page's place (the note stays as it is behind it).
    fn open_canvas(&mut self, id: BlockId) {
        if self.files.canvas.as_ref().is_some_and(|c| c.id == id) { return } self.canvas_leave();
        let Some(st) = self.store.as_mut() else { return }; if self.ed.preedit().is_some() { let _ = self.ed.set_preedit("", None, 0); }
        st.edited.get_or_insert_with(Instant::now); self.autosave(); self.video_stop(); self.find_close(); self.files.link = None; self.files.picker = None; self.files.picker_chip = None;
        let Some(st) = self.store.as_mut() else { return };
        match st.col.load_canvas(id) { Ok(mut c) => { let mut turned = false;
                for e in c.elements.iter_mut().filter(|e| e.kind == "text") { let Ok(nid) = st.col.create(Some(id), mn_store::DocKind::Note, "") else { continue }; let mut d = Doc::new();
                    for (i, line) in e.record.lines().enumerate() { d.insert(None, i, BlockId::new(), Kind::Paragraph, line).unwrap(); } if d.children(None).is_empty() { d.insert(None, 0, BlockId::new(), Kind::Paragraph, "").unwrap(); }
                    if st.col.save_note(nid, &d).is_ok() { e.kind = "note".into(); e.record = nid.to_string(); e.h = e.h.max(200.0); turned = true; } }
                let mut cv = canvas::Canvas::new(id, c); cv.dirty = turned; if self.files.pen.is_none() && self.files.pen_w.is_none() { (self.files.pen, self.files.pen_w) = pen_settings(); } cv.pen = self.files.pen.unwrap_or([1.0, 0.0, 0.0, 1.0]); cv.pen_w = self.files.pen_w.unwrap_or(6.0) as f64; self.files.canvas = Some(cv); self.blank = true; self.files.cards.clear(); } Err(e) => eprintln!("open: {e}") }
        if let Some(sb) = self.sidebar.as_mut() { sb.selected = Some(id); } if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// Files dropped or chosen for the open canvas: the place they go to (the view's centre when no point was kept).
    fn canvas_drop() -> mn_edit::Drop { mn_edit::Drop { parent: None, index: usize::MAX, depth: None, beside: None } }
    /// A key on the open canvas. True: it was the canvas's.
    fn canvas_key(&mut self, key: &Key, ui: min_ui::KeyEvent) -> bool {
        let (shift, cmd, alt) = self.mods; let Some(g) = self.gpu.as_mut() else { return false }; let Some(cv) = self.files.canvas.as_mut() else { return false };
        let m = (cv.rect.center().x, cv.rect.center().y);
        if cv.editing() && !cmd { if matches!(key, Key::Named(NamedKey::Escape)) { cv.escape(); } else { cv.key(&ui, &mut g.fonts); } g.window.request_redraw(); return true }
        let step = if shift { 10.0 } else { 1.0 };
        let done = match key {
            Key::Named(NamedKey::Escape) => { cv.escape(); true }
            Key::Named(NamedKey::Enter) => { cv.enter(&mut g.fonts); true }
            Key::Named(NamedKey::Backspace | NamedKey::Delete) => { cv.delete(); true }
            // Option + Left / Right: the selected PDF card's page before / after
            Key::Named(NamedKey::ArrowLeft) if alt => cv.pdf_page(false), Key::Named(NamedKey::ArrowRight) if alt => cv.pdf_page(true),
            Key::Named(NamedKey::ArrowLeft) => { cv.nudge(-step, 0.0); true } Key::Named(NamedKey::ArrowRight) => { cv.nudge(step, 0.0); true }
            Key::Named(NamedKey::ArrowUp) => { cv.nudge(0.0, -step); true } Key::Named(NamedKey::ArrowDown) => { cv.nudge(0.0, step); true }
            // the drawing tools, by the Qt app's keys (the same again puts the tool down; V: the pointer)
            Key::Character(c) if !cmd && !alt && !self.ctrl && cv.selected_video().is_none() => { use canvas::Ink as I; match c.to_lowercase().as_str() { "v" => { cv.arm(None); true } "p" => { cv.arm(Some(I::Pen)); true } "l" => { cv.arm(Some(I::Line)); true } "a" => { cv.arm(Some(I::Arrow)); true }
                "r" => { cv.arm(Some(I::Rect)); true } "o" => { cv.arm(Some(I::Oval)); true } "e" => { cv.arm(Some(I::Eraser)); true }
                // F: the frame tool (row 327)
                "f" => { let on = !cv.framing; cv.arm(None); cv.framing = on; true } _ => false } }
            Key::Character(c) if cmd && !alt => match c.to_lowercase().as_str() {
                "a" => { cv.select_all(); true } "d" => { let clip = cv.copy(); cv.paste(&clip, 24.0); true }
                // Cmd+G: a frame round the selection; Shift+Cmd+G: the frame goes, its cards stay (row 327)
                "g" => { if shift { cv.ungroup(); } else { cv.group(); } true }
                "c" | "x" => { if cv.editing() { let t = cv.field().and_then(|f| Some(f.text().to_string())); if let (Some(t), Some(cb)) = (t, self.clipboard.as_mut()) { let _ = cb.set_text(t); } } else { self.files.cclip = cv.copy(); if !self.files.cclip.is_empty() { self.block_clip = None; if let Some(cb) = self.clipboard.as_mut() { let _ = cb.set_text(self.files.cclip.iter().filter(|e| e.kind == "text").map(|e| e.record.clone()).collect::<Vec<_>>().join("\n")); } } if c.to_lowercase() == "x" { cv.delete(); } } true }
                "v" => { if !self.files.cclip.is_empty() { let clip = self.files.cclip.clone(); cv.paste(&clip, 24.0); self.files.cclip = cv.copy(); }
                    else { let text = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()).filter(|t| !t.trim().is_empty());
                        let paths = if self.store.is_some() { min_paths::clipboard::paste_paths().ok().filter(|p| !p.is_empty() && p.iter().all(|x| std::path::Path::new(x).exists())) } else { None };
                        if let Some(paths) = paths { self.files.canvas_at.set(None); self.add_files(paths.into_iter().map(Into::into).collect(), Self::canvas_drop()); }
                        else if let Some(img) = self.clipboard.as_mut().and_then(|cb| cb.get_image().ok()).filter(|i| i.width > 0 && i.height > 0 && self.store.is_some()) { self.files.canvas_at.set(None); self.files.token += 1; self.files.pending.insert(self.files.token, Self::canvas_drop()); self.media.add_pixels(self.files.token, img.bytes.into_owned(), img.width as u32, img.height as u32); }
                        else if let (Some(t), Some(cv)) = (text, self.files.canvas.as_mut()) { if mn_edit::links::looks_like_url(t.trim()) { cv.add_link(&mn_edit::links::normalise_url(&t)); } else { let m = cv.rect.center(); let w = cv.to_world(m.x - 160.0, m.y - 60.0); self.canvas_note(w, &t, false); } } }
                    true }
                "=" | "+" => { cv.zoom_by(2f64.sqrt(), m); true } "-" => { cv.zoom_by(1.0 / 2f64.sqrt(), m); true } "0" => { cv.fit_all(); true } "1" if shift => { cv.zoom_to(1.0); true }
                "]" => { cv.restack(true); true } "[" => { cv.restack(false); true }
                _ => false },
            _ => false };
        if done { if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        done
    }
    /// A note card is entered (row 322, stage 2): the camera goes to it at 100 %, and its note becomes the one edited - in the card's box.
    fn card_enter(&mut self, el: BlockId) {
        let Some(note) = self.files.canvas.as_ref().and_then(|cv| cv.els.iter().find(|e| e.id == el && e.kind == "note")).and_then(|e| BlockId::parse(&e.record)) else { return };
        if self.store.as_ref().is_none_or(|st| st.col.entry(note).ok().flatten().is_none()) { return }
        self.card_leave(); if let Some(cv) = self.files.canvas.as_mut() { cv.focus(el); }
        self.swap_note(note); self.blank = false; self.scroll = 0.0; self.layout = None;
        let rect = self.files.canvas.as_ref().and_then(|cv| cv.rect_of(el)).unwrap_or(Rect::ZERO); self.files.card = Some(Card { el, note, rect });
        if self.ed.selection().is_none() { if let Some(first) = self.ed.doc.walk().iter().find(|b| !b.kind.is_container()).map(|b| b.id) { self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: first, at: 0 }))); } }
        if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// The entered card is left: its note is kept, and the canvas has the keys again.
    fn card_leave(&mut self) {
        if self.files.card.take().is_none() { return } if self.ed.preedit().is_some() { let _ = self.ed.set_preedit("", None, 0); }
        if let Some(st) = self.store.as_mut() { st.edited.get_or_insert_with(Instant::now); } self.autosave(); self.video_stop(); self.find_close(); self.files.link = None; self.files.picker = None; self.files.picker_chip = None;
        self.blank = true; self.layout = None; self.scroll = 0.0; if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// A note of the canvas's own is made at a point of the sheet (its top left), with these lines; `enter`: and entered.
    /// It is under the canvas in the collection, and is no row of the tree.
    fn canvas_note(&mut self, w: kurbo::Point, text: &str, enter: bool) {
        let Some(st) = self.store.as_mut() else { return }; let Some(cid) = self.files.canvas.as_ref().map(|c| c.id) else { return };
        let Ok(id) = st.col.create(Some(cid), mn_store::DocKind::Note, "") else { return }; let mut d = Doc::new();
        for (i, line) in text.lines().enumerate() { d.insert(None, i, BlockId::new(), Kind::Paragraph, line).unwrap(); } if d.children(None).is_empty() { d.insert(None, 0, BlockId::new(), Kind::Paragraph, "").unwrap(); }
        let _ = st.col.save_note(id, &d);
        if let Some(cv) = self.files.canvas.as_mut() { let el = cv.add_note(id, None); if let Some(e) = cv.els.iter_mut().find(|e| e.id == el) { e.w = 320.0; e.h = 480.0; e.x = w.x.round(); e.y = w.y.round(); } if enter { self.card_enter(el); } }
    }
    /// What a gesture on the canvas asked of the window: a note card to enter; a note of the canvas's own to make at a point, and enter.
    fn canvas_asks(&mut self) {
        let (enter, want) = match self.files.canvas.as_mut() { Some(cv) => (cv.entered.take(), cv.want_note.take()), None => return };
        if let Some(w) = want { self.canvas_note(w, "", true); }
        if let Some(el) = enter { self.card_enter(el); }
        // a file's card: its file in its own application; a link's: its address
        let opened = self.files.canvas.as_mut().and_then(|cv| cv.opened.take()).and_then(|id| self.files.canvas.as_ref().and_then(|cv| cv.els.iter().find(|e| e.id == id).map(|e| (e.kind.clone(), e.record.clone()))));
        match opened { Some((k, rec)) if k == "link" => { if !open::url(&rec) { self.notice = Some((format!("This link is not one minNotes opens: {rec}"), Instant::now() + std::time::Duration::from_secs(6))); } } Some((_, rec)) => self.file_open(&rec, false), None => {} }
    }
    /// The note of the entered card, or of the one selected note card: (the element, the note, it is the canvas's own).
    fn card_note(&self) -> Option<(BlockId, BlockId, bool)> {
        let cv = self.files.canvas.as_ref()?; let [el] = cv.sel[..] else { return None }; let e = cv.els.iter().find(|e| e.id == el && e.kind == "note")?; let note = BlockId::parse(&e.record)?;
        let own = self.store.as_ref().and_then(|st| st.col.entry(note).ok().flatten()).is_some_and(|x| x.parent == Some(cv.id)); Some((el, note, own))
    }
    fn open_note(&mut self, id: BlockId) {
        self.canvas_leave();
        self.swap_note(id);
    }
    /// The editor becomes this note's (the one it held is kept beside it).
    fn swap_note(&mut self, id: BlockId) {
        let Some(st) = self.store.as_mut() else { return }; if st.note == id { return }
        if self.ed.preedit().is_some() { let _ = self.ed.set_preedit("", None, 0); }
        st.edited.get_or_insert_with(Instant::now); self.autosave();
        let Some(st) = self.store.as_mut() else { return };
        let next = match self.others.remove(&id) { Some(e) => e, None => match st.col.load_note(id) { Ok(d) => Editor::new(d), Err(e) => { eprintln!("open: {e}"); return } } };
        let old = std::mem::replace(&mut self.ed, next); self.others.insert(st.note, old);
        st.note = id; self.layout = None; self.flat = None; self.scroll = 0.0; self.follow = false;
        #[cfg(target_os = "macos")] { text_element::replaced(); self.geom_moved = Some(Instant::now()); }
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// Cmd+N / Cmd+Shift+N: a note or a folder beside the selected entry (inside it, when that is a folder).
    fn new_entry(&mut self, kind: mn_store::DocKind) {
        let (Some(st), Some(sb), Some(g)) = (self.store.as_mut(), self.sidebar.as_mut(), self.gpu.as_mut()) else { return };
        let parent = sb.selected.or(Some(st.note)).and_then(|s| sb.row(s)).and_then(|r| if r.kind == mn_store::DocKind::Folder { Some(r.id) } else { r.parent });
        let id = match st.col.create(parent, kind, if kind == mn_store::DocKind::Folder { "New folder" } else { "" }) { Ok(id) => id, Err(e) => { eprintln!("new: {e}"); return } };
        if kind == mn_store::DocKind::Note { let mut d = Doc::new(); d.insert(None, 0, BlockId::new(), Kind::Paragraph, "").unwrap(); let _ = st.col.save_note(id, &d); }
        st.unpublished = true; sb.set_tree(&st.col); sb.selected = Some(id); self.changed.insert(id);
        if kind == mn_store::DocKind::Folder { sb.start_rename(id, &mut g.fonts); g.window.request_redraw(); } else if kind == mn_store::DocKind::Canvas { sb.focused = false; self.open_canvas(id); } else { sb.focused = false; self.open_note(id); self.ed.select_all(); }
    }
    fn rename_entry(&mut self, id: BlockId, name: &str) {
        let (Some(st), Some(sb)) = (self.store.as_mut(), self.sidebar.as_mut()) else { return };
        if st.col.rename(id, name).is_ok() { st.unpublished = true; sb.set_tree(&st.col); self.changed.insert(id); }
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// Asks before deleting (the entry leaves the file at the next publish).
    fn ask_delete(&mut self, id: BlockId) {
        let Some(row) = self.sidebar.as_ref().and_then(|sb| sb.row(id)) else { return };
        let body = if row.kind == mn_store::DocKind::Folder { "The folder and everything in it will be removed from the collection." } else { "The note will be removed from the collection." };
        let mut d = min_ui::dialog::Dialog::new(&format!("Delete \u{201c}{}\u{201d}?", row.title), body, vec![("Cancel", min_ui::text_button::Variant::Secondary), ("Delete", min_ui::text_button::Variant::Negative)]);
        d.show(); self.confirm = Some((Ask::Delete(id), d)); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn delete_entry(&mut self, id: BlockId) {
        let (Some(st), Some(sb)) = (self.store.as_mut(), self.sidebar.as_mut()) else { return };
        if let Err(e) = st.col.remove(id) { eprintln!("delete: {e}"); return }
        st.unpublished = true; sb.set_tree(&st.col); sb.selected = None; self.others.retain(|k, _| sb.row(*k).is_some()); self.changed.retain(|k| sb.row(*k).is_some());
        // the open note went with it: another one opens, or a new one is made
        if sb.row(st.note).is_none() {
            let next = sb.rows.iter().find(|r| r.kind == mn_store::DocKind::Note).map(|r| r.id);
            self.others.remove(&st.note);
            match next { Some(n) => { let gone = st.note; self.open_note(n); self.others.remove(&gone); }
                None => { st.note = BlockId::new(); self.new_entry(mn_store::DocKind::Note); } }
        }
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn act(&mut self, a: sidebar::Act) {
        match a { sidebar::Act::None => {}, sidebar::Act::Redraw => { if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            // a search result (row 321): the note opens with the find bar on the word, at the occurrence chosen
            sidebar::Act::Found(id, go) => { self.open_note(id); if let Some(sb) = self.sidebar.as_mut() { sb.query.focused = false; }
                if let Some((term, ord)) = go { self.ensure(); if let Some(g) = self.gpu.as_mut() { match self.files.find.as_mut() { Some(f) => f.query.set_text_end(&term, &mut g.fonts), None => self.files.find = Some(findbar::FindBar::new(&term, &mut g.fonts)) } }
                    if let Some(f) = self.files.find.as_mut() { f.blur(); f.case = false; } self.files.found_for = None; self.find_refresh(); if !self.files.hits.is_empty() { self.files.hit_at = ord.min(self.files.hits.len() - 1); self.find_show(); } }
                if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            sidebar::Act::NewCanvas => self.new_entry(mn_store::DocKind::Canvas),
            sidebar::Act::Open(id) => { match self.sidebar.as_ref().and_then(|sb| sb.row(id)).map(|r| r.kind) { Some(mn_store::DocKind::Note) => self.open_note(id), Some(mn_store::DocKind::Canvas) => self.open_canvas(id), _ => {} } if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            sidebar::Act::Fold(id) => { if let (Some(sb), Some(st)) = (self.sidebar.as_mut(), self.store.as_ref()) { sb.toggle_fold(id); sb.set_tree(&st.col); } if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            sidebar::Act::NewNote => self.new_entry(mn_store::DocKind::Note), sidebar::Act::NewFolder => self.new_entry(mn_store::DocKind::Folder),
            sidebar::Act::Renamed(id, name) => self.rename_entry(id, &name) }
    }
    /// A pointer event for whatever lies over the page. True: it was taken (the note must not act on it).
    fn chrome_pointer(&mut self, e: min_ui::Event) -> bool {
        let now = Instant::now();
        if let (Some((id, d)), Some(g)) = (self.confirm.as_mut(), self.gpu.as_mut()) {
            let id = *id; d.pointer(&e, &mut g.fonts); let (pressed, _) = d.event(&e, now); g.window.request_redraw();
            if let Some(i) = pressed { self.confirm = None; self.answer(id, i); }
            return true
        }
        if self.files.gather.as_ref().is_some_and(|w| w.shown) { let out = self.files.gather.as_mut().map(|w| w.pointer(&e)).unwrap_or(gather_win::Out::None); self.gather_out(out); if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        // the colour palette: while it is open every event is its own
        if self.pal.is_open() { if let Some(g) = self.gpu.as_mut() { let out = self.pal.event(&e, &mut g.fonts, now); self.colour_out(out); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        // the tree's context menu is over everything but a dialog: while it is open the event is its own
        if self.ctx.is_open() { let (chosen, _, _) = self.ctx.event(&e); if let Some(i) = chosen { self.ctx_pick(i); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        // no collection: the page's two buttons
        if self.blank && self.files.canvas.is_none() { let mut hit = None; for (i, b) in self.splash.iter_mut().enumerate() { if b.event(&e, now).0 { hit = Some(i); } }
            if let Some(g) = &self.gpu { g.window.request_redraw(); }
            // run from the loop's idle step, as a menu command is: the file dialog is not opened inside a pointer event
            match hit { Some(0) => self.later.push(menu::Cmd::NewCollection), Some(_) => self.later.push(menu::Cmd::Open), None => {} }
            // a press on a button is the button's to the end: the release must come back here, not go to the page
            if hit.is_some() || e.pos().is_some_and(|(x, y)| self.splash.iter().any(|b| b.hit(x, y))) { return true } }
        let (did, on_foot) = self.foot.event(&e, now);
        if !did.is_empty() || on_foot { for d in did { self.menu_cmd(match d { footer::Do::Undo => menu::Cmd::Undo, footer::Do::Redo => menu::Cmd::Redo, footer::Do::Save => menu::Cmd::Save, footer::Do::Sidebar => menu::Cmd::Sidebar, footer::Do::Readout => { if let Some(w) = self.files.gather.as_mut() { w.shown = true; } continue } footer::Do::Find => { if self.files.find.is_some() { self.find_close(); } else { self.find_open(); } continue } }); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        let (tools, on_bar) = self.bar.event(&e, now);
        if !tools.is_empty() || on_bar { for tool in tools { self.tool(tool); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) else { return false };
        // the rail's floating tab (the caret that reopens a folded sidebar, as in QCView) shows while the pointer is in the window
        let (a, taken) = sb.event(&e, &mut g.fonts, now);
        if !taken && matches!(e, min_ui::Event::Down { .. }) { sb.focused = false; sb.query.focused = false; }
        self.act(a); taken
    }
    fn title(&self) { if let (Some(g), None) = (&self.gpu, &self.store) { g.window.set_title("minNotes"); }
        if let (Some(g), Some(st)) = (&self.gpu, &self.store) { let name = self.sidebar.as_ref().and_then(|sb| sb.row(st.note)).map(|r| r.title.clone()).unwrap_or_else(|| st.title.clone()); g.window.set_title(&format!("{}{}{} — minNotes", name, if self.saving.is_some() { " — saving…" } else { "" }, if st.unpublished || st.edited.is_some() { " •" } else { "" })); } }
    fn now_ms(&self) -> u64 { self.t0.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0) }
    /// A window point in the note's coordinates.
    /// The handle (row 305): a grip in the margin beside the first line of the caret's block — of the first selected
    /// block when blocks are selected. It stands where the CARET is, never where the pointer is. Window coordinates.
    fn handle_rect(&self) -> Option<Rect> {
        if self.blank { return None }
        let (l, g) = (self.layout.as_ref()?, self.gpu.as_ref()?); let s = g.window.scale_factor();
        handle_rect_in(&self.ed, l, s, origin(&self.files.card, self.side_w, self.top, self.scroll, s))
    }
    fn note_point(&self, p: (f64, f64)) -> (f64, f64) { let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); { let o = origin(&self.files.card, self.side_w, self.top, self.scroll, s); (p.0 - o.x, p.1 - o.y) } }
    fn key(&mut self, key: &Key, text: Option<&str>) {
        let (shift, cmd, alt) = self.mods; let now = self.now_ms();
        // Enter after a typed address: the address becomes a link first (row 320)
        if matches!(key, Key::Named(NamedKey::Enter)) && !cmd && !self.blank && !self.modal() && self.files.link.is_none() && self.files.picker.is_none() && self.files.note_field.is_none() && !self.ctx.is_open() { let d = self.ed.autolink(false, now).unwrap_or(Dirty::Nothing); if d != Dirty::Nothing { self.apply(d); } }
        if self.files.resize.is_some() { if matches!(key, Key::Named(NamedKey::Escape)) { self.resize_end(false); } return }
        // Save is the collection's, wherever the keys are - on a canvas (which has no page laid out), in the tree, in a field
        if cmd && !alt && !shift && !self.modal() && matches!(key, Key::Character(c) if c.to_lowercase() == "s") { self.save(); return }
        if self.page_key(key) { return }
        // a PDF's page: Cmd+C copies the selected text, Escape lets it go; Cmd+A on a current PDF block selects its page's text
        if let Some(sel) = &self.files.pdf {
            match key { Key::Character(c) if cmd && !shift && c.to_lowercase() == "c" => { if let Some(t) = self.pdf_boxes(sel.id).and_then(|b| self.media.page_text(&b.3)) { let words = t.words(sel.a, sel.b); if let Some(cb) = self.clipboard.as_mut() { let _ = cb.set_text(words); } self.block_clip = None; } return }
                Key::Named(NamedKey::Escape) => { self.files.pdf = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                Key::Named(NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Meta) => {}
                Key::Character(c) if cmd && c.to_lowercase() == "a" => {}
                _ => { self.files.pdf = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } } } }
        if let Key::Character(c) = key { if cmd && !shift && c.to_lowercase() == "a" && self.ed.run_len() == 1 { if let Some(id) = self.ed.handle_block() { if let Some(t) = self.pdf_boxes(id).and_then(|b| self.media.page_text(&b.3)).filter(|t| !t.chars.is_empty()) {
            self.files.pdf = Some(PdfSel { id, a: 0, b: t.chars.len() - 1, drag: false }); if let Some(g) = &self.gpu { g.window.request_redraw(); } return } } } }
        if self.grab.is_some() { if matches!(key, Key::Named(NamedKey::Escape)) { self.grab = None; if let Some(sb) = self.sidebar.as_mut() { sb.drop_row = None; } self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } } return }
        if self.ctx.is_open() { if matches!(key, Key::Named(NamedKey::Escape)) { self.ctx.close(); if let Some(g) = &self.gpu { g.window.request_redraw(); } } return }
        // where there is no native menu to take them, the file commands' keys are run here
        if cmd && cfg!(not(target_os = "macos")) { if let Key::Character(c) = key { let m = match (c.to_lowercase().as_str(), shift, alt) { ("o", false, false) => Some(menu::Cmd::Open), ("w", false, false) => Some(menu::Cmd::CloseCollection), ("q", false, false) => Some(menu::Cmd::Quit), ("s", true, false) => Some(menu::Cmd::SaveAs), ("n", false, true) => Some(menu::Cmd::NewCollection), _ => None }; if let Some(m) = m { self.menu_cmd(m); return } } }
        // (an open canvas has no page - `blank` - and its keys are taken below; nothing after them is for it)
        if self.blank && self.files.canvas.is_none() { return }
        // what lies over the page takes keys first: a question, a name being edited, then the tree when it has the focus
        let ui_key = || { use min_ui::{Key as K, Named as N};
            let k = match key { Key::Named(NamedKey::Escape) => K::Named(N::Escape), Key::Named(NamedKey::Enter) => K::Named(N::Enter), Key::Named(NamedKey::Tab) => K::Named(N::Tab), Key::Named(NamedKey::Backspace) => K::Named(N::Backspace), Key::Named(NamedKey::Delete) => K::Named(N::Delete),
                Key::Named(NamedKey::ArrowLeft) => K::Named(N::Left), Key::Named(NamedKey::ArrowRight) => K::Named(N::Right), Key::Named(NamedKey::ArrowUp) => K::Named(N::Up), Key::Named(NamedKey::ArrowDown) => K::Named(N::Down), Key::Named(NamedKey::Home) => K::Named(N::Home), Key::Named(NamedKey::End) => K::Named(N::End),
                Key::Character(c) => K::Char(c.to_string()), _ => K::Char(String::new()) };
            min_ui::KeyEvent { key: k, text: text.map(|t| t.to_string()), shift, cmd } };
        // the palette: Escape closes it; the picker's hex field takes the keys while it has them; nothing reaches the note
        if self.pal.is_open() { if let Some(g) = self.gpu.as_mut() { if matches!(key, Key::Named(NamedKey::Escape)) { self.pal.close(); } else if self.pal.typing() { self.pal.key(&ui_key(), &mut g.fonts); } g.window.request_redraw(); } return }
        if let (Some((id, d)), Some(g)) = (self.confirm.as_mut(), self.gpu.as_mut()) {
            let id = *id; let r = if matches!(key, Key::Named(NamedKey::Escape)) { Some(0) } else { d.key(&ui_key(), &mut g.fonts) }; g.window.request_redraw();
            if let Some(i) = r { self.confirm = None; self.answer(id, i); } return
        }
        if self.files.gather.as_ref().is_some_and(|w| w.shown) { let out = match key { Key::Named(NamedKey::Escape) => self.files.gather.as_mut().map(|w| w.key(false)), Key::Named(NamedKey::Enter) => self.files.gather.as_mut().map(|w| w.key(true)), _ => None }; if let Some(o) = out { self.gather_out(o); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        // a canvas is open (row 322): its keys, unless a field of the sidebar has them
        // a note edited in a card (row 322, stage 2): Escape leaves the card when nothing else has a use for it; Cmd+Return on a selected note card opens the note in the page
        if self.files.card.is_some() && matches!(key, Key::Named(NamedKey::Escape)) && !self.modal() && !self.ctx.is_open() && self.files.link.is_none() && self.files.picker.is_none() && self.files.find.as_ref().is_none_or(|f| !f.focused()) && !self.ed.in_blocks() && self.ed.cell_rect().is_none() && self.files.resize.is_none() && self.grab.is_none() { self.card_leave(); return }
        if self.files.canvas.is_some() && cmd && matches!(key, Key::Named(NamedKey::Enter)) && self.files.card.is_none() { if let Some((_, note, _)) = self.card_note() { self.open_note(note); return } }
        if self.files.canvas.is_some() && self.files.card.is_none() && !self.sidebar.as_ref().is_some_and(|sb| sb.typing()) {
            if self.files.canvas.as_ref().is_some_and(|c| c.editing()) && cmd && !alt { if let Key::Character(c) = key { if c.to_lowercase() == "v" { let t = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()); if let (Some(t), Some(cv), Some(g)) = (t, self.files.canvas.as_mut(), self.gpu.as_mut()) { let k = min_ui::KeyEvent { key: min_ui::Key::Char(String::new()), text: Some(t), shift: false, cmd: false }; cv.key(&k, &mut g.fonts); g.window.request_redraw(); } return } } }
            if self.canvas_key(key, ui_key()) { self.canvas_asks(); return } }
        // (a canvas has no page, but the sidebar is still the window's: Cmd+1 folds and opens it there too)
        if self.blank && cmd && !shift && !alt && matches!(key, Key::Character(c) if c == "1") { if let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_ref()) { sb.rail.toggle(Instant::now()); self.layout = None; g.window.request_redraw(); } return }
        if self.blank { return }
        // a popup's field (the link's editor, a picker, a note's words): Cmd+V / C / X are the field's - the clipboard is the window's
        if cmd && !alt { if let Key::Character(c) = key { let c = c.to_lowercase(); if matches!(c.as_str(), "v" | "c" | "x") {
            let pasted = if c == "v" { self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()) } else { None };
            let field: Option<&mut min_ui::TextField> = if let Some(sb) = self.sidebar.as_mut().filter(|sb| sb.query.focused && !sb.renaming()) { Some(&mut sb.query) } else if let Some(p) = self.files.link.as_mut() { Some(p.field()) } else if let Some(f) = self.files.find.as_mut().filter(|f| f.focused() && self.files.picker.is_none()) { f.field() } else if let Some(p) = self.files.picker.as_mut() { p.field() } else if let Some((_, _, f)) = self.files.note_field.as_mut() { Some(f) } else { None };
            if let (Some(f), Some(g)) = (field, self.gpu.as_mut()) {
                let copied = if c == "v" { if let Some(t) = &pasted { f.paste(t, &mut g.fonts); } None } else { f.copy(c == "x", &mut g.fonts) };
                if self.files.link.is_none() && self.files.picker.is_none() && self.files.note_field.is_none() && self.files.find.is_some() { self.files.found_for = None; }
                if let (Some(t), Some(cb)) = (copied, self.clipboard.as_mut()) { let _ = cb.set_text(t); self.block_clip = None; }
                g.window.request_redraw(); return }
            if self.files.link.is_some() || self.files.picker.is_some() || self.files.note_field.is_some() || self.sidebar.as_ref().is_some_and(|sb| sb.query.focused) { return } } } }
        // a note's words being typed take the keys: Enter keeps them, Escape gives them up
        if self.files.note_field.is_some() { let r = match (self.files.note_field.as_mut(), self.gpu.as_mut()) { (Some((_, _, f)), Some(g)) => Some(f.key(&ui_key(), &mut g.fonts)), _ => None };
            match r { Some(min_ui::FieldResponse::Accepted) => self.note_words_end(true), Some(min_ui::FieldResponse::Cancelled) => self.note_words_end(false), _ => {} }
            if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        // find (row 321): Cmd+F opens the bar (or gives it the keys), Cmd+G / Shift+Cmd+G go on and back; while one of its fields has the keys they are the bar's
        if self.files.link.is_none() && self.files.picker.is_none() && !self.blank {
            if let Key::Character(c) = key { if cmd && !alt { match c.to_lowercase().as_str() { "f" if shift => { if let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) { if sb.rail.collapsed { self.later.push(menu::Cmd::Sidebar); } sb.search_focus(&mut g.fonts); if let Some(f) = self.files.find.as_mut() { f.blur(); } g.window.request_redraw(); } return } "f" if !shift => { if let Some(sb) = self.sidebar.as_mut() { sb.query.focused = false; } self.find_open(); return } "g" if self.files.find.is_some() => { self.find_out(if shift { findbar::Out::Prev } else { findbar::Out::Next }); return } _ => {} } } }
            if self.files.find.as_ref().is_some_and(|f| f.focused()) && !(cmd && matches!(key, Key::Character(c) if !matches!(c.to_lowercase().as_str(), "a"))) {
                let out = match (self.files.find.as_mut(), self.gpu.as_mut()) { (Some(f), Some(g)) => f.key(&ui_key(), &mut g.fonts), _ => findbar::Out::None }; self.find_out(out); if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
        // a link's editor takes the keys while it is open
        if self.files.link.is_some() { let out = match (self.files.link.as_mut(), self.gpu.as_mut()) { (Some(p), Some(g)) => p.key(&ui_key(), &mut g.fonts), _ => linkedit::Out::None }; self.link_out(out); if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        // a choice cell's picker takes the keys while it is open
        if self.files.picker.is_some() { let out = match (self.files.picker.as_mut(), self.gpu.as_mut()) { (Some(p), Some(g)) => p.key(&ui_key(), &mut g.fonts), _ => choice::Out::None }; self.choice_out(out); if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
        if let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) {
            if let Some(a) = sb.key(&ui_key(), &mut g.fonts) { self.act(a); if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
            if cmd { if let Key::Character(c) = key { match c.to_lowercase().as_str() {
                "1" => { sb.rail.toggle(Instant::now()); g.window.request_redraw(); return }
                "n" => { self.new_entry(if shift { mn_store::DocKind::Folder } else { mn_store::DocKind::Note }); return }
                _ => {} } } }
            if sb.focused { if let Some(sel) = sb.selected { match key {
                Key::Named(NamedKey::Enter) => { sb.start_rename(sel, &mut g.fonts); g.window.request_redraw(); return }
                Key::Named(NamedKey::Backspace | NamedKey::Delete) => { self.ask_delete(sel); return }
                Key::Named(NamedKey::Escape) => { sb.focused = false; g.window.request_redraw(); return }
                Key::Named(k @ (NamedKey::ArrowUp | NamedKey::ArrowDown | NamedKey::ArrowLeft | NamedKey::ArrowRight)) => { let n = match k { NamedKey::ArrowUp => sidebar::Nav::Up, NamedKey::ArrowDown => sidebar::Nav::Down, NamedKey::ArrowLeft => sidebar::Nav::Left, _ => sidebar::Nav::Right }; let a = sb.nav(n); self.act(a); if let Some(sb) = self.sidebar.as_mut() { sb.focused = true; } return }
                _ => {} } } if !cmd { return } }
        }
        let Some(l) = self.layout.as_ref() else { return };
        let mv = |m: Move, by_word: Move, by_end: Move| if cmd { by_end } else if alt { by_word } else { m };
        let (ctrl, blocks) = (self.ctrl, self.ed.in_blocks());
        // Option+Cmd+C: a choice chip at the caret; Option+Down just after a chip: its picker (row 320)
        if let Key::Character(c) = key { if cmd && alt && !shift && matches!(c.to_lowercase().as_str(), "c" | "\u{e7}") { self.chip_insert(); return } }
        if matches!(key, Key::Named(NamedKey::ArrowDown)) && alt && !cmd && !shift && !self.ed.in_blocks() { if let Some(c) = self.ed.chip_before_caret() { self.chip_open(c, ""); return } }
        // Cmd+K: the link's editor (row 320)
        if let Key::Character(c) = key { if cmd && !shift && !alt && c.to_lowercase() == "k" { self.link_open(); return } }
        // cells of a table are selected (row 319): Delete empties them (whole rows / columns go), Cmd+C / X carry them as a
        // small table and as a spreadsheet's text, Escape lets them go; nothing is typed over them
        if !self.ed.in_blocks() && !self.blank && self.ed.cell_rect().is_some() { match key {
            Key::Named(NamedKey::Backspace | NamedKey::Delete) => { self.follow = true; let d = self.ed.cells_delete(now).unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
            Key::Named(NamedKey::Escape) => { if let Some(f) = self.ed.selection().map(|x| x.focus) { self.ed.set_selection(Some(mn_edit::Selection::caret(f))); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
            Key::Character(c) if cmd && !shift && !alt && matches!(c.to_lowercase().as_str(), "c" | "x") => { let text = self.ed.cells_text();
                if let Some(clip) = self.ed.copy_cells() { if let Some(cb) = self.clipboard.as_mut() { if cb.set_html(clip::html(&clip, true), Some(text.clone())).is_err() { let _ = cb.set_text(text.clone()); } } self.block_clip = Some((text, clip)); }
                if c.to_lowercase() == "x" { let d = self.ed.cells_clear(now).unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None; } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
            Key::Character(_) if !cmd => return,
            Key::Named(NamedKey::Enter) if !cmd => return,
            _ => {} } }
        // a typed cell (row 317, stage 3): a choice opens its picker (a letter goes into its field), a check goes to its
        // next state; neither takes typing. Enter in a timecode cell writes it as a timecode.
        if !cmd && !alt && !self.ed.in_blocks() && !self.blank { if let Some(tc) = self.ed.typed_cell() {
            let space = matches!(key, Key::Character(c) if c.as_str() == " ");
            match (&tc.kind, key) {
                (ColumnKind::Choice { .. }, Key::Named(NamedKey::Enter)) => { self.choice_open("", false); return }
                (ColumnKind::Choice { .. }, _) if space => { self.choice_open("", false); return }
                (ColumnKind::Choice { .. }, Key::Named(NamedKey::Backspace | NamedKey::Delete)) => { let d = self.ed.cell_choose(None, now).unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                (ColumnKind::Choice { .. }, Key::Character(_)) => { if let Some(t) = text.filter(|t| !t.chars().any(|c| c.is_control())) { let t = t.trim().to_string(); self.choice_open(&t, false); } return }
                (ColumnKind::Check, Key::Named(NamedKey::Enter)) | (ColumnKind::Check, _) if space || matches!(key, Key::Named(NamedKey::Enter)) => { let d = self.ed.cell_check_cycle(now).unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                (ColumnKind::Check, Key::Named(NamedKey::Backspace | NamedKey::Delete) | Key::Character(_)) => return,
                (ColumnKind::Timecode { .. }, Key::Named(NamedKey::Enter)) => { if let Some(b) = self.ed.selection().map(|x| x.focus.block) { let d = self.ed.settle_timecode(b, now).unwrap_or(Dirty::Nothing); self.apply(d); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                _ => {} } } }
        let dirty = match key {
            // whole blocks (row 304): Escape selects the caret's block and gives the caret back; Ctrl+Cmd+Up / Down move
            // the selected blocks (the caret's block when none are); with blocks selected Up / Down step, Shift grows,
            // Enter returns to the text, Backspace removes, and typing does nothing
            Key::Named(NamedKey::Escape) => { self.ed.toggle_blocks(); Dirty::Nothing }
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) if ctrl && cmd => { self.follow = true; self.ed.move_blocks(matches!(key, Key::Named(NamedKey::ArrowDown)), now).unwrap_or(Dirty::Nothing) }
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) if blocks && !cmd && !alt => { self.ed.step_blocks(matches!(key, Key::Named(NamedKey::ArrowDown)), shift); self.follow = true; Dirty::Nothing }
            Key::Named(NamedKey::Enter) if blocks => { self.ed.toggle_blocks(); Dirty::Nothing }
            Key::Named(NamedKey::Backspace | NamedKey::Delete) if blocks => self.ed.delete_blocks(now).unwrap_or(Dirty::Nothing),
            Key::Character(c) if blocks && cmd && !shift && matches!(c.to_lowercase().as_str(), "a" | "c" | "x") => {
                if c.to_lowercase() == "a" { self.ed.select_all_blocks(); Dirty::Nothing } else {
                    let text = self.ed.selected_text();
                    if let Some(clip) = self.ed.copy_blocks() { if let Some(cb) = self.clipboard.as_mut() { if cb.set_html(clip::html(&clip, true), Some(text.clone())).is_err() { let _ = cb.set_text(text.clone()); } } self.block_clip = Some((text, clip)); }
                    if c.to_lowercase() == "x" { self.ed.delete_blocks(now).unwrap_or(Dirty::Nothing) } else { Dirty::Nothing } } }
            Key::Character(_) if blocks && !cmd => return,
            Key::Named(NamedKey::ArrowLeft) => { self.ed.move_caret(l, mv(Move::Left, Move::WordLeft, Move::LineStart), shift); Dirty::Nothing }
            Key::Named(NamedKey::ArrowRight) => { self.ed.move_caret(l, mv(Move::Right, Move::WordRight, Move::LineEnd), shift); Dirty::Nothing }
            // Up / Down that can go no further in a table or in columns at the note's very start / end: a paragraph is made beyond it (the way out)
            Key::Named(NamedKey::ArrowUp) => { let was = self.ed.selection(); self.ed.move_caret(l, mv(Move::Up, Move::Up, Move::DocStart), shift); if !shift && self.ed.selection() == was { self.follow = true; self.ed.exit_container(false, now).unwrap_or(Dirty::Nothing) } else { Dirty::Nothing } }
            Key::Named(NamedKey::ArrowDown) => { let was = self.ed.selection(); self.ed.move_caret(l, mv(Move::Down, Move::Down, Move::DocEnd), shift); if !shift && self.ed.selection() == was { self.follow = true; self.ed.exit_container(true, now).unwrap_or(Dirty::Nothing) } else { Dirty::Nothing } }
            Key::Named(NamedKey::Home) => { self.ed.move_caret(l, if cmd { Move::DocStart } else { Move::LineStart }, shift); Dirty::Nothing }
            Key::Named(NamedKey::End) => { self.ed.move_caret(l, if cmd { Move::DocEnd } else { Move::LineEnd }, shift); Dirty::Nothing }
            // Cmd+Enter in a table: a row below the caret's
            Key::Named(NamedKey::Enter) if cmd && self.ed.selection().is_some_and(|x| self.ed.cell_of(x.focus.block).is_some()) => { self.follow = true; self.ed.add_row_below(now).unwrap_or(Dirty::Nothing) }
            Key::Named(NamedKey::Enter) => self.ed.enter(now).unwrap_or(Dirty::Nothing),
            Key::Named(NamedKey::Backspace) => self.ed.delete(false, now).unwrap_or(Dirty::Nothing),
            Key::Named(NamedKey::Delete) => self.ed.delete(true, now).unwrap_or(Dirty::Nothing),
            // in a table Tab is the cells' (the next one, a new row past the last); elsewhere the list's
            Key::Named(NamedKey::Tab) => match self.ed.tab_cell(shift, now).unwrap_or(None) { Some(d) => { self.follow = true; d } None => match self.ed.indent(shift, now).unwrap_or(None) { Some(d) => d, None => return } },
            // formatting by command (row 289; the Qt app's keys): with a selection it toggles, with a caret it arms
            Key::Character(c) if cmd && shift && c.to_lowercase() == "x" => self.ed.toggle_mark(Mark::Strike, now).unwrap_or(Dirty::Nothing),
            Key::Character(c) if cmd && shift && c.to_lowercase() == "h" => self.ed.toggle_mark(Mark::Highlight(self.last_bg.clone()), now).unwrap_or(Dirty::Nothing),
            Key::Character(c) if cmd && matches!(c.to_lowercase().as_str(), "b" | "i" | "u" | "e" | "\\") => match c.to_lowercase().as_str() {
                "b" => self.ed.toggle_mark(Mark::Bold, now), "i" => self.ed.toggle_mark(Mark::Italic, now), "u" => self.ed.toggle_mark(Mark::Underline, now), "e" => self.ed.toggle_mark(Mark::Code, now), _ => self.ed.clear_formatting(now) }.unwrap_or(Dirty::Nothing),
            Key::Character(c) if cmd => match c.to_lowercase().as_str() {
                "a" => { self.ed.select_all(); Dirty::Nothing }
                "d" => { self.follow = true; self.ed.duplicate_blocks(now).unwrap_or(Dirty::Nothing) }
                "s" => { self.save(); return }
                "z" => self.undo_redo(shift),
                "c" | "x" => { let t = self.ed.selected_text(); if !t.is_empty() { self.block_clip = None; let frag = self.ed.selected_fragment(); if let Some(cb) = self.clipboard.as_mut() { if cb.set_html(clip::html(&frag, false), Some(t.clone())).is_err() { let _ = cb.set_text(t); } } } if c.to_lowercase() == "x" { self.ed.delete(false, now).unwrap_or(Dirty::Nothing) } else { Dirty::Nothing } }
                "v" => self.paste(if shift { Paste::Plain } else { Paste::Rich }),
                _ => return,
            },
            // typed text — one key event may carry several characters (a writing tool's correction, the spike S9)
            _ => match text.filter(|t| !cmd && !t.chars().any(|c| c.is_control())) { Some(t) => { if t.ends_with(' ') { self.files.autolink = true; } self.ed.insert(t, now).unwrap_or(Dirty::Nothing) } None => return },
        };
        self.apply(dirty);
    }

    fn finish(&mut self) {
        trail(&format!("finish: the application is closing (quit asked: {})", self.quit));
        if let Some(t0) = *SPIN_T0.lock().unwrap() { let n = SPUN.load(std::sync::atomic::Ordering::Relaxed).saturating_sub(30); let s = t0.elapsed().as_secs_f64(); println!("spin: {n} frames in {s:.2} s = {:.1} per second ({} elements)", n as f64 / s, self.files.canvas.as_ref().map(|c| c.els.len()).unwrap_or(0)); }
        self.autosave();
        if std::env::var_os("MN_SAVE_AT_EXIT").is_some() { self.save(); }   // for scripted checks
        // (scripted, MN_GATHER=collect | package:<folder>: Collect Media or the package, run whole as the run ends)
        if let Ok(spec) = std::env::var("MN_GATHER") { self.gather_now(&spec); }
        // (scripted, MN_UNPACK=<package>:<folder>: a package file is unpacked, waited for)
        if let Ok(spec) = std::env::var("MN_UNPACK") { if let Some((a, f)) = spec.split_once(':') { self.unpack_open(a.into(), Some(f.into())); let t = Instant::now(); while self.files.unpack.is_some() && t.elapsed().as_secs() < 300 { self.unpack_tick(); std::thread::sleep(std::time::Duration::from_millis(5)); } } }
        if let Some(u) = self.files.unpack.take() { u.stop.store(true, std::sync::atomic::Ordering::Relaxed); }
        // (scripted, MN_EXPORT=<a.html>[:<b.md>]: the open note is exported there as the run ends)
        if let Ok(list) = std::env::var("MN_EXPORT") { for p in list.split(':') { self.export(if p.ends_with(".md") { 1 } else if p.ends_with(".docx") { 2 } else if p.ends_with(".pdf") { 3 } else if p.ends_with(".png") { 4 } else { 0 }, Some(p.into())); if let Some((t, _)) = &self.notice { println!("export: {t}"); } } }
        // a save under way is waited for (it is short unless the document's place does not answer; then 20 s at most)
        let t = Instant::now(); while self.saving.is_some() && t.elapsed().as_secs() < 20 { if let Some(step) = self.saver.poll() { self.save_step(step); } else { std::thread::sleep(std::time::Duration::from_millis(5)); } }
        if self.saving.take().is_some() { self.saver.abandon(); eprintln!("save: no answer from the document's place; your changes are kept on this machine"); }
        // a session with nothing unpublished is cleaned up; one WITH unpublished work is left where it is, to be taken up
        // again at the next open of the file (nothing typed is dropped by closing the window)
        if let Some(st) = self.store.as_mut() { if let Some(s) = st.session.take() { if st.unpublished { println!("closed with unsaved changes: they are kept and will be taken up again when the file is next opened"); drop(s) } else { let _ = s.close(); } } }
        if let (Some(path), Some(g)) = (self.shot.take(), self.gpu.as_mut()) {
            match g.presenter.capture_window() {
                Some((px, w, h)) => { let oetf = |v: f32| { let v = v.clamp(0.0, 1.0); if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 } };
                    let bytes: Vec<u8> = px.chunks(4).flat_map(|p| [(oetf(p[0]) * 255.0 + 0.5) as u8, (oetf(p[1]) * 255.0 + 0.5) as u8, (oetf(p[2]) * 255.0 + 0.5) as u8, 255]).collect();
                    let mut e = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&path).expect("--shot")), w, h); e.set_color(png::ColorType::Rgba); e.set_depth(png::BitDepth::Eight);
                    e.write_header().and_then(|mut wr| wr.write_image_data(&bytes)).expect("png"); println!("shot: {path} {w}x{h}"); }
                None => println!("shot: this surface cannot be captured"),
            }
        }
        let mut d = std::mem::take(&mut self.draw_ms); d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if !d.is_empty() { println!("frames {}: layout + scene + render + present, CPU ms p50 {:.2} p95 {:.2}", self.frames, d[d.len() / 2], d[(d.len() * 95 / 100).min(d.len() - 1)]); }
        self.gather_drop();
        let (bytes, n) = self.media.held(); if n > 0 { println!("pictures held: {n}, {:.1} MB", bytes as f64 / (1 << 20) as f64); }
    }
}

/// The note as the desktop's text element reads it (rows 276, 281): one flat UTF-16 text; rectangles in view points.
#[cfg(target_os = "macos")]
impl text_element::TextHost for App {
    fn ax_text(&mut self) -> String { if !self.ensure() { return String::new() } self.flat.as_ref().unwrap().text.clone() }
    fn ax_len16(&mut self) -> usize { if !self.ensure() { return 0 } self.flat.as_ref().unwrap().len16 }
    fn ax_selection(&mut self) -> (usize, usize) {
        if !self.ensure() { return (0, 0) } let f = self.flat.as_ref().unwrap();
        let Some(s) = self.ed.selection() else { return (0, 0) };
        let (a, b) = (f.offset16(s.anchor.block, s.anchor.at).unwrap_or(0), f.offset16(s.focus.block, s.focus.at).unwrap_or(0));
        (a.min(b), a.max(b) - a.min(b))
    }
    fn ax_set_selection(&mut self, location: usize, length: usize) {
        if !self.ensure() { return } let f = self.flat.as_ref().unwrap();
        let pos = |o: usize| f.locate16(o).map(|(l, at)| mn_edit::Pos { block: l.block, at });
        if let (Some(a), Some(b)) = (pos(location), pos(location + length)) { self.ed.set_selection(Some(mn_edit::Selection { anchor: a, focus: b })); self.follow = true; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
    }
    fn ax_substring(&mut self, location: usize, length: usize) -> String { if !self.ensure() { return String::new() } self.flat.as_ref().unwrap().slice16(location, length).to_string() }
    fn ax_range_rect(&mut self, location: usize, length: usize) -> (f64, f64, f64, f64) {
        if !self.ensure() { return (0.0, 0.0, 0.0, 0.0) }
        let (f, l) = (self.flat.as_ref().unwrap(), self.layout.as_ref().unwrap()); let s = l.scale as f64;
        let rects = if length == 0 { f.locate16(location).and_then(|(line, at)| l.caret(&self.ed.doc, line.block, at, 1.0)).into_iter().collect() } else { l.flat_rects(&self.ed.doc, f, location, length) };
        let Some(u) = rects.into_iter().reduce(|a: Rect, b| a.union(b)) else { return (0.0, 0.0, 0.0, 0.0) };
        let o = origin(&self.files.card, self.side_w, self.top, self.scroll, s); let mut u = u + o;
        // In a card only what the card shows has a place: the rest answers nothing, so no underline is drawn outside it.
        if let Some(c) = &self.files.card { u = u.intersect(c.rect); if u.width() <= 0.0 || u.height() <= 0.0 { return (0.0, 0.0, 0.0, 0.0) } }
        (u.x0 / s, u.y0 / s, u.width() / s, u.height() / s)
    }
    fn ax_note_frame(&mut self) -> (f64, f64, f64, f64) { if !self.ensure() { return (0.0, 0.0, 0.0, 0.0) } let l = self.layout.as_ref().unwrap(); let s = l.scale as f64; let o = origin(&self.files.card, self.side_w, self.top, self.scroll, s); (o.x / s, o.y / s, l.width as f64 / s, l.height as f64 / s) }
    fn ax_viewport(&mut self) -> (f64, f64) { self.gpu.as_ref().map(|g| { let (w, h) = g.presenter.size(); let s = g.window.scale_factor(); (w as f64 / s, h as f64 / s) }).unwrap_or((0.0, 0.0)) }
    fn replace_before_caret(&mut self, text: &str) {
        let now = self.now_ms(); if !self.ensure() { return }
        let l = self.layout.as_ref().unwrap();
        if self.ed.selection().is_some_and(|s| s.is_caret()) { self.ed.move_caret(l, Move::Left, true); }
        let d = self.ed.insert(text, now).unwrap_or(Dirty::Nothing); self.apply(d);
    }
    fn ax_scroll_fraction(&mut self) -> f64 {
        let (Some(l), Some(g)) = (self.layout.as_ref(), self.gpu.as_ref()) else { return 0.0 }; let s = g.window.scale_factor();
        let max = (l.height as f64 + 2.0 * PAD * s - (g.presenter.size().1 as f64 - self.top - self.bottom)).max(1.0); (self.scroll / max).clamp(0.0, 1.0)
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.gpu.is_some() { return }
        startup_log("event loop running; creating the window");
        let window: Arc<Box<dyn Window>> = Arc::new(el.create_window(WindowAttributes::default().with_title("minNotes").with_surface_size(LogicalSize::new(1000.0, 760.0))).expect("window"));
        startup_log("window created");
        let instance = Presenter::instance();
        let surface = instance.create_surface(window.clone()).expect("surface");
        let size = window.surface_size();
        #[cfg(target_os = "macos")]
        let ns_view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().unwrap().as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))]
        let ns_view = None;
        // QCView's display path (row 287): the surface is the display's own — extended range where the screen has it
        let mut presenter = Presenter::new(&instance, surface, (size.width, size.height), Output::Auto, ns_view).expect("no renderer");
        startup_log("presenter (GPU device, surface) ready");
        presenter.engine.set_ui_scale(window.scale_factor() as f32);
        let ui = UiLayer::new(&presenter.engine.device, size.width, size.height).expect("ui layer");
        presenter.engine.set_overlay(Some(ui.view()));
        let i = presenter.info();
        println!("presenter: {} ({}) → {:?} {:?} colour space {:?}; scale {}", i.adapter, i.backend, i.output, i.format, i.color_space, window.scale_factor());
        startup_log("ui layer ready");
        window.request_redraw();
        #[cfg(target_os = "macos")] if let Some(v) = ns_view { text_element::install(self as *mut App as *mut dyn text_element::TextHost, v.cast()); }
        // the request is REFUSED (silently) unless every capability asked for comes with its first value: the caret's area
        ime(window.as_ref().as_ref(), Some(Rect::new(0.0, 0.0, 1.0, 1.0)), true);
        #[cfg(target_os = "macos")] { let mut m = menu::AppMenu::new(); m.menu.init_for_nsapp(); m.set_recent(&recent_list()); self.menu = Some(m); } self.sync_export_checks();
        startup_log("text element, input method, menu ready");
        self.gpu = Some(Gpu { window, presenter, ui, fonts: Fonts::new(), ime_area: None, ime_on: true }); self.t0 = Some(Instant::now()); self.title();
        startup_log("fonts loaded");
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if let Some(t0) = self.t0.filter(|t| t.elapsed().as_millis() < 1500) { let now = Instant::now(); let at = *self.warm_at.get_or_insert(t0 + std::time::Duration::from_millis(250));
            if now >= at { self.warm_at = Some(now + std::time::Duration::from_millis(250)); if let Some(g) = &self.gpu { g.window.request_redraw(); } } else { el.set_control_flow(ControlFlow::WaitUntil(at)); } }
        if let Some((_, until)) = &self.notice { if Instant::now() >= *until { self.notice = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } } else { el.set_control_flow(ControlFlow::WaitUntil(*until)); } }
        // MN_ADD=<file>[,<file>...]: the files are added after the note's first block, as a drop there would; MN_RELINK=
        // <file>: the note's first picture that was not found is pointed at it (both for scripted checks: no input is sent)
        if !self.files.tested && (std::env::var_os("MN_ADD").is_some() || std::env::var_os("MN_RELINK").is_some() || std::env::var_os("MN_RESIZE").is_some() || std::env::var_os("MN_PDF").is_some() || std::env::var_os("MN_PLAY").is_some() || std::env::var_os("MN_SEEK").is_some() || std::env::var_os("MN_TABLE").is_some() || std::env::var_os("MN_LINK").is_some() || std::env::var_os("MN_FIND").is_some() || std::env::var_os("MN_SEARCH").is_some() || std::env::var_os("MN_CANVAS").is_some()) { if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        if self.frames >= 60 && !self.files.tested && self.layout.is_some() && !self.blank { self.files.tested = true;
            // MN_PDF=<page>: the note's first PDF block turns to that page (counted from 0); when its text is read, all of it is selected and its length printed
            if let Some(p) = std::env::var("MN_PDF").ok().and_then(|v| v.parse::<i64>().ok()) { if let Some(id) = self.ed.doc.walk().iter().find(|b| matches!(&b.kind, Kind::Media { descriptor } if descriptor.contains("\"kind\":\"pdf\""))).map(|b| b.id) { self.pdf_turn(id, p); self.files.pdf_test = Some(id); } }
            // MN_TABLE=1: at the note's end a table is made and filled by typing and Tab, a row added, and columns made beside a paragraph (as the toolbar and the keys would)
            // MN_LINK=1: a word of the note's second block is made a link, an address is typed after it (it links at its space), and the link's editor opens on the first
            // MN_LINK=2: a choice chip goes in at the start of the note's long paragraph and its picker opens; 3: a second chip, its options' editor
            // MN_SEARCH=<words>: they are typed into the sidebar's search field; MN_SEARCH_GO=1: the first result's first line is chosen
            if let Ok(q) = std::env::var("MN_SEARCH") { if let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) { sb.query.set_text_end(&q, &mut g.fonts); }
                if std::env::var_os("MN_SEARCH_GO").is_some() { for _ in 0..200 { self.search_step(); if self.files.search.as_ref().is_none_or(|x| x.done()) { break } }
                    let go = self.sidebar.as_ref().and_then(|sb| sb.results.as_ref()).and_then(|r| r.iter().find(|f| !f.snippets.is_empty())).map(|f| (f.id, f.snippets.last().map(|sn| (sn.term.clone(), sn.ord)))); if let Some((id, g)) = go { self.act(sidebar::Act::Found(id, g)); } } }
            // MN_FIND=<words>: the find bar opens on them and goes to the second occurrence; MN_FIND_WITH=<words>: the replace row shows, and every occurrence is replaced
            if let Ok(q) = std::env::var("MN_FIND") { self.find_open(); if let (Some(f), Some(g)) = (self.files.find.as_mut(), self.gpu.as_mut()) { f.query.set_text_end(&q, &mut g.fonts); }
                self.find_out(findbar::Out::Changed); self.find_out(findbar::Out::Next);
                if let Ok(w) = std::env::var("MN_FIND_WITH") { if let (Some(f), Some(g)) = (self.files.find.as_mut(), self.gpu.as_mut()) { f.replacing = true; f.with.set_text_end(&w, &mut g.fonts); } self.find_out(findbar::Out::ReplaceAll); } }
            if matches!(std::env::var("MN_LINK").as_deref(), Ok("2") | Ok("3")) { if let Some(p) = self.ed.doc.children(None).iter().copied().find(|id| self.ed.doc.get(*id).is_some_and(|b| b.kind == Kind::Paragraph && b.text.len() > 30)) {
                self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: p, at: 0 }))); self.chip_insert();
                if std::env::var("MN_LINK").as_deref() == Ok("3") { self.files.picker = None; self.files.picker_chip = None; let t = self.now_ms(); let _ = self.ed.insert(" then ", t + 20_000); self.apply(Dirty::Structure); self.chip_insert(); if let (Some(c), Some(g)) = (self.files.picker_chip.clone(), self.gpu.as_mut()) { let mut pk = choice::ChoicePicker::new(c.0, c.3.clone(), Some(c.4.clone()), "", &mut g.fonts).editing(); pk.inline = true; self.files.picker = Some(pk); } } } }
            else if std::env::var_os("MN_LINK").is_some() { if let Some(p) = self.ed.doc.children(None).iter().copied().find(|id| self.ed.doc.get(*id).is_some_and(|b| b.kind == Kind::Paragraph && b.text.len() > 30)) { let t = self.now_ms();
                self.ed.set_selection(Some(mn_edit::Selection { anchor: mn_edit::Pos { block: p, at: 4 }, focus: mn_edit::Pos { block: p, at: 12 } })); let _ = self.ed.set_link("example.com/review", None, t);
                self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: p, at: 0 }))); let _ = self.ed.insert("www.minnotes.app ", t + 9_000); let _ = self.ed.autolink(true, t + 9_000);
                self.apply(Dirty::Structure); self.layout = None; self.ensure(); self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: p, at: 24 }))); self.link_open(); } }
            if std::env::var_os("MN_TABLE").is_some() { if let Some(first) = self.ed.doc.children(None).first().copied() { self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: first, at: 0 }))); } self.ed.move_caret(self.layout.as_ref().unwrap(), Move::DocEnd, false); let t = self.now_ms();
                let _ = self.ed.insert("\nA table made in place:\n", t); let _ = self.ed.insert_table(3, 3, t + 2000);
                for (i, w) in ["Shot", "Status", "Note", "010", "ok", "hold the grade", "020", "redo"].iter().enumerate() { let _ = self.ed.insert(w, t + 3000 + i as u64 * 2000); let _ = self.ed.tab_cell(false, t + 3500 + i as u64 * 2000); }
                let _ = self.ed.insert("a second line in the cell\nand a paragraph below it", t + 30_000); for _ in 0..2 { let _ = self.ed.tab_cell(false, t + 31_000); }
                // MN_TABLE=2: the caret stays in the table (its handles show); a column is set right and given a width, a row is moved up
                if matches!(std::env::var("MN_TABLE").as_deref(), Ok("3") | Ok("4")) { for i in 0..5u64 { let _ = self.ed.table_insert(true, true, t + 40_000 + i * 2000); let _ = self.ed.insert("wider", t + 40_500 + i * 2000); let _ = self.ed.table_column(|c| c.width = Some(260), t + 41_000 + i * 2000); }
                    // 4: the caret goes back to the first cell (the table scrolls back with it)
                    if std::env::var("MN_TABLE").as_deref() == Ok("4") { for _ in 0..6 { let _ = self.ed.tab_cell(true, t + 60_000); } } self.follow = true; }
                // MN_TABLE=5|6|7: the Status column becomes a choice (its values the options), the Shot column a check (one cycled to done);
                // 5 opens the picker on a Status cell, 6 its options' editor, 7 asks before the Note column (rich cells) becomes a timecode
                if matches!(std::env::var("MN_TABLE").as_deref(), Ok("5") | Ok("6") | Ok("7") | Ok("8")) { let which = std::env::var("MN_TABLE").unwrap_or_default();
                    let _ = self.ed.table_set_kind(ColumnKind::Choice { options: vec![] }, t + 50_000); let _ = self.ed.tab_cell(true, t + 51_000); let _ = self.ed.table_set_kind(ColumnKind::Check, t + 52_000);
                    let _ = self.ed.cell_check_cycle(t + 54_000); let _ = self.ed.cell_check_cycle(t + 56_000); let _ = self.ed.tab_cell(false, t + 57_000);
                    self.apply(Dirty::Structure); self.follow = true;
                    match which.as_str() { "5" => self.choice_open("", false), "6" => self.choice_open("", true), "8" => self.table_menu(true, 700.0, 300.0), _ => { let _ = self.ed.tab_cell(false, t + 58_000); self.column_kind(3, false); } }
                    return }
                // MN_TABLE=9: a rectangle of cells is selected (as a drag from cell to cell would); 10: the caret's whole row
                if matches!(std::env::var("MN_TABLE").as_deref(), Ok("9") | Ok("10")) { if let Some((table, _, _)) = self.ed.table_pos() { if std::env::var("MN_TABLE").as_deref() == Ok("9") { self.ed.select_cells(table, (1, 0), (2, 1)); } else { let _ = self.ed.tab_cell(true, t + 50_000); let _ = self.ed.tab_cell(true, t + 50_000); self.ed.select_line(false, false); } } self.apply(Dirty::Structure); return }
                // MN_TABLE=11: columns are made beside the paragraph (as below), then the note's first block is dropped beside them (a third column)
                if std::env::var("MN_TABLE").as_deref() == Ok("11") { self.ed.move_caret(self.layout.as_ref().unwrap(), Move::DocEnd, false); let _ = self.ed.insert("Beside this paragraph a column is made.", t + 40_000); let _ = self.ed.insert_columns(t + 42_000); let _ = self.ed.insert("The second column.", t + 44_000);
                    let cols = self.ed.doc.children(None).iter().copied().find(|id| self.ed.doc.get(*id).is_some_and(|b| b.kind == Kind::Columns)); let last = self.ed.doc.children(None).iter().rev().copied().find(|id| self.ed.doc.get(*id).is_some_and(|b| b.kind == Kind::Paragraph && !b.text.is_empty()));
                    if let (Some(cols), Some(p)) = (cols, last) { self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: p, at: 0 }))); self.ed.toggle_blocks(); let r = self.ed.drop_blocks(&mn_edit::Drop { parent: None, index: 0, depth: None, beside: Some((cols, 0)) }, t + 50_000); eprintln!("beside: {r:?}"); }
                    self.apply(Dirty::Structure); self.follow = true; return }
                if matches!(std::env::var("MN_TABLE").as_deref(), Ok("3") | Ok("4")) {} else if std::env::var("MN_TABLE").as_deref() == Ok("2") { let _ = self.ed.tab_cell(true, t + 32_000); let _ = self.ed.table_column(|c| { c.align = 2; c.width = Some(220); }, t + 33_000); let _ = self.ed.table_move(false, false, t + 34_000); }
                else { self.ed.move_caret(self.layout.as_ref().unwrap(), Move::DocEnd, false); let _ = self.ed.insert("Beside this paragraph a column is made.", t + 40_000); let _ = self.ed.insert_columns(t + 42_000); let _ = self.ed.insert("The second column.", t + 44_000); }
                self.apply(Dirty::Structure); }
            // MN_SEEK=<frame>: the note's first video block goes to that frame, paused (as a click on a note's picture would)
            if let Some(f) = std::env::var("MN_SEEK").ok().and_then(|v| v.parse::<i32>().ok()) { if let Some(id) = self.ed.doc.walk().iter().find(|b| matches!(&b.kind, Kind::Media { descriptor } if descriptor.contains("\"kind\":\"video\""))).map(|b| b.id) { self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: id, at: 0 }))); self.video_seek(id, f); if std::env::var_os("MN_DRAW").is_some() { self.arm(qcv_annotate::Tool::Arrow); } } }
            // MN_PLAY=1: the note's first video block plays (as a click on it would)
            if std::env::var_os("MN_PLAY").is_some() { if let Some(id) = self.ed.doc.walk().iter().find(|b| matches!(&b.kind, Kind::Media { descriptor } if descriptor.contains("\"kind\":\"video\""))).map(|b| b.id) { let ok = self.video_start(id, true); println!("play: started {ok}"); } }
            // MN_PAGE_INK=1: the note's first picture (2: its first PDF) is made current and drawn on as the pointer would (see `page_ink_test`)
            if std::env::var_os("MN_PAGE_INK").is_some() { self.files.ink_test = true; }
            // (scripted, MN_EXPORT_PANEL=1: the export's save panel is opened as the menu opens it - to watch what the application does while it is up)
            if std::env::var_os("MN_EXPORT_PANEL").is_some() { self.later.push(menu::Cmd::ExportHtml); }
            // MN_RESIZE=<width>: the note's first picture is made current and resized to that width as a corner drag would (one undo step), then undone and redone
            if let Some(w) = std::env::var("MN_RESIZE").ok().and_then(|v| v.parse::<f64>().ok()) { use mn_edit::{Pos, Selection};
                if let Some((id, before)) = self.ed.doc.walk().iter().find_map(|b| match &b.kind { Kind::Media { descriptor } if !descriptor.contains("\"kind\"") => Some((b.id, descriptor.clone())), _ => None }) {
                    self.ed.set_selection(Some(Selection::caret(Pos { block: id, at: 0 })));
                    let dw = |me: &App| me.ed.doc.get(id).and_then(|b| match &b.kind { Kind::Media { descriptor } => serde_json::from_str::<serde_json::Value>(descriptor).ok().and_then(|v| v.get("dw").and_then(|d| d.as_f64())), _ => None });
                    self.files.resize = Some(Resize { id, before, from_x: 0.0, width: 0.0, max: 2000.0, left: false }); let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
                    self.resize_to(w * s); let live = dw(self); self.resize_end(true); let kept = dw(self);
                    let d = self.ed.undo().unwrap_or(Dirty::Nothing); self.apply(d); let undone = dw(self); let d = self.ed.redo().unwrap_or(Dirty::Nothing); self.apply(d); self.layout = None;
                    println!("resize: while dragged {live:?}, kept {kept:?}, after undo {undone:?}, after redo {:?}", dw(self)); } }
            // MN_CANVAS=1: the collection's first canvas is opened (MN_ADD's files then go onto it), and a text card is put on it; 2: one more is added and selected alone
            if std::env::var_os("MN_CANVAS").is_some() { fn first(col: &mn_store::Collection, parent: Option<BlockId>) -> Option<BlockId> { for e in col.list(parent).unwrap_or_default() { if e.kind == mn_store::DocKind::Canvas { return Some(e.id) } if let Some(id) = first(col, Some(e.id)) { return Some(id) } } None }
                if let Some(id) = self.store.as_ref().and_then(|st| first(&st.col, None)) { self.open_canvas(id); }
                let which = std::env::var("MN_CANVAS").unwrap_or_default();
                // 3: two notes of the collection are put on it as cards; 4: the first is entered and typed in (the note's own editor, in the card)
                if matches!(which.as_str(), "3" | "4") { let notes: Vec<BlockId> = self.sidebar.as_ref().map(|sb| sb.rows.iter().filter(|r| r.kind == mn_store::DocKind::Note).map(|r| r.id).collect()).unwrap_or_default();
                    let mut first = None; if let Some(cv) = self.files.canvas.as_mut() { for (i, n) in notes.iter().take(2).enumerate() { let el = cv.add_note(*n, None); if let Some(e) = cv.els.iter_mut().find(|e| e.id == el) { e.x = i as f64 * 600.0; e.y = 0.0; } first = first.or(Some(el)); } cv.sel.clear(); }
                    if which == "4" { if let Some(el) = first { self.ensure(); self.card_enter(el); let t = self.now_ms(); let _ = self.ed.insert("Typed in the card. ", t); self.apply(Dirty::Structure); } } }
                else if which == "6" { let id = self.files.canvas.as_mut().and_then(|cv| { let id = cv.els.iter().find(|e| e.kind == "media" && e.record.contains("\"kind\":\"video\"")).map(|e| e.id)?; cv.sel = vec![id]; Some(id) }); match id { Some(id) => { let ok = self.video_start(id, std::env::var_os("MN_PLAY").is_some()); println!("canvas video: started {ok}"); } None => self.files.cv_test = true } }
                else if which == "7" { if let Some(cv) = self.files.canvas.as_mut() { cv.frame_demo(); } }
                // 8: two cards are selected and Cmd+G is sent down the real key path (the frame must be there at once)
                else if which == "8" { let n = |me: &App| me.files.canvas.as_ref().map(|cv| cv.els.iter().filter(|e| e.kind == "frame").count()).unwrap_or(0);
                    if let Some(cv) = self.files.canvas.as_mut() { cv.add_link("https://example.org/one"); cv.add_link("https://example.org/two"); let ids: Vec<BlockId> = cv.els.iter().filter(|e| e.kind == "link").map(|e| e.id).collect(); cv.sel = ids; }
                    let (before, mods) = (n(self), self.mods); self.mods = (false, true, false); self.key(&Key::Character("g".into()), None); self.mods = mods; println!("canvas Cmd+G: frames {before} -> {}, framing {}", n(self), self.files.canvas.as_ref().is_some_and(|cv| cv.framing)); }
                else if which == "5" { if let Some(cv) = self.files.canvas.as_mut() { cv.ink_demo(std::env::var("MN_INK").ok().and_then(|n| n.parse().ok()).unwrap_or(0)); if std::env::var_os("MN_INK_ERASE").is_some() { cv.arm(Some(canvas::Ink::Eraser)); } } }
                else if let Some(cv) = self.files.canvas.as_mut() { cv.add_text("A text card on the canvas.\nTyped in place; its second line."); if which == "2" { cv.add_text("Selected alone: the corners show"); } } }
            if let Ok(v) = std::env::var("MN_ADD") { let at = if self.files.canvas.is_some() { self.files.canvas_at.set(None); Self::canvas_drop() } else { mn_edit::Drop { parent: None, index: 1, depth: None, beside: None } }; self.add_files(v.split(',').map(Into::into).collect(), at); }
            if let Ok(v) = std::env::var("MN_RELINK") { let blocks: Vec<(BlockId, String)> = self.ed.doc.walk().iter().filter_map(|b| match &b.kind { Kind::Media { descriptor } => Some((b.id, descriptor.clone())), _ => None }).collect();
                match blocks.iter().find(|(_, d)| self.media.is_missing(d)).map(|x| x.0) { Some(id) => self.media.relink(blocks, id, v.into()), None => println!("relink: no picture is missing") } } }
        // MN_MOVE_TEST=<a note's name>: the app moves the open note's first block to that note, undoes and redoes it,
        // and prints the two notes' block counts after each (a scripted check of row 307; nothing is typed or clicked)
        if let (Ok(name), false, true) = (std::env::var("MN_MOVE_TEST"), self.shown_blocks, self.layout.is_some() && !self.blank) { self.shown_blocks = true;
            let to = self.sidebar.as_ref().and_then(|sb| sb.rows.iter().find(|r| r.title == name).map(|r| r.id));
            if let (Some(to), Some(first)) = (to, self.ed.doc.walk().iter().find(|b| !b.kind.is_container()).map(|b| b.id)) {
                let count = |me: &App, what: &str| println!("move-test {what}: here {} there {} stored-there {}", me.ed.doc.children(None).len(), me.others.get(&to).map(|o| o.doc.children(None).len() as i64).unwrap_or(-1), me.store.as_ref().and_then(|st| st.col.load_note(to).ok()).map(|d| d.children(None).len() as i64).unwrap_or(-1));
                self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: first, at: 0 }))); count(self, "before");
                self.blocks_to_note(to, false); count(self, "moved");
                let d = self.undo_redo(false); self.apply(d); count(self, "undone");
                let d = self.undo_redo(true); self.apply(d); count(self, "redone");
                // an edit here after the move: undo then takes back only this note's half
                let d = self.ed.insert("x", self.now_ms() + 5000).unwrap_or(Dirty::Nothing); self.apply(d); let d = self.undo_redo(false); self.apply(d); let d = self.undo_redo(false); self.apply(d); count(self, "typed, undone twice");
            } else { println!("move-test: no note named {name:?}"); } }
        if std::env::var_os("MN_BLOCKS").is_some() && !self.blank && !self.ed.in_blocks() && self.layout.is_some() && !self.shown_blocks { self.shown_blocks = true; if let Some(id) = self.ed.doc.walk().iter().find(|b| !b.kind.is_container()).map(|b| b.id) { self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: id, at: 0 }))); } self.ed.toggle_blocks(); self.ed.step_blocks(true, false); self.ed.step_blocks(true, false); self.ed.step_blocks(true, true); self.ed.step_blocks(true, true);
            // MN_DRAG=x,y (window px): as if the handle were being dragged with the pointer there
            if let Some((x, y)) = std::env::var("MN_DRAG").ok().and_then(|v| { let mut p = v.split(',').map(|n| n.trim().parse::<f64>().ok()); Some((p.next()??, p.next()??)) }) { self.pointer = (x, y); let (nx, ny) = self.note_point(self.pointer); let drop = self.layout.as_ref().and_then(|l| self.ed.drop_at(l, nx, ny)); self.grab = Some(Grab { from: (0.0, 0.0), dragging: true, drop, to_note: None }); }
            if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        if std::env::var_os("MN_ASK_CLOSE").is_some() && self.confirm.is_none() && !self.blank && self.layout.is_some() && !self.shown_blocks { self.shown_blocks = true; if let Some(st) = self.store.as_mut() { st.unpublished = true; } self.request_close(Pending::Close); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        // (scripted, MN_OPEN_NEXT=<collection>: once a canvas is open and drawn, another collection is opened over it)
        if let (false, Ok(p), true) = (self.files.next_test, std::env::var("MN_OPEN_NEXT"), self.frames > 30 && self.files.canvas.is_some()) { self.files.next_test = true; self.request_close(Pending::Open(p.into())); }
        // (scripted, MN_IMPORT=<file>[:<file>…]: the files are imported once the collection is shown; MN_IMPORT_BIG=first|attach|cancel answers the large table's question)
        if let (false, Ok(list), true) = (self.files.import_test, std::env::var("MN_IMPORT"), self.frames > 2 && self.store.is_some()) { self.files.import_test = true; self.import_files(list.split(':').map(std::path::PathBuf::from).collect()); }
        // (scripted, MN_GATHER_SHOW=collect | package:<folder>[,go]: the window is opened by the app itself, for a screenshot; `go` starts the copy)
        if let (false, Ok(spec), true) = (self.files.gather_test, std::env::var("MN_GATHER_SHOW"), self.frames > 2 && self.store.is_some()) { self.files.gather_test = true; let (spec, _) = spec.split_once(',').unwrap_or((&spec, "")); let (what, to) = match spec.split_once(':') { Some(("package", f)) => (gather::What::Package, Some(std::path::PathBuf::from(f))), _ => (gather::What::Collect, None) }; self.gather_open(what, to); }
        if let Ok(x) = std::env::var("MN_GATHER_SHOW") { if x.ends_with(",go") || x.ends_with(",stop") { if let Some(w) = self.files.gather.as_mut() { if w.phase == gather_win::Phase::Review && w.run.is_none() && w.go() && x.ends_with(",stop") {
            // (the face that asks what to keep: the copy is let finish, then counted as stopped)
            if let Some(r) = w.run.clone() { let t = Instant::now(); while r.busy() && t.elapsed().as_secs() < 60 { std::thread::sleep(std::time::Duration::from_millis(5)); } r.stop.store(true, std::sync::atomic::Ordering::Relaxed); } } } } }
        if self.show_pal.is_some() && self.bar.rect_of(toolbar::Tool::Color).width() > 1.0 { let what = self.show_pal.take().unwrap();
            if what == "background" { self.open_colours(mn_doc::MarkKind::Highlight, toolbar::Tool::Highlight) } else { self.open_colours(mn_doc::MarkKind::Color, toolbar::Tool::Color) }
            if what == "picker" { if let Some(g) = self.gpu.as_mut() { self.pal.show_picker(&mut g.fonts); } }
            if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        // a picture came from the decoders
        { let dev = self.gpu.as_ref().map(|g| (g.presenter.engine.device.clone(), g.presenter.engine.queue.clone())); let poster = &mut self.poster;
          if self.media.poll(&mut |px, meta| poster.convert(dev.as_ref().map(|d| (&d.0, &d.1)), px, meta)) { if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
        for m in self.media.mail() { self.media_mail(m); }
        // clips' notes are read again every two seconds while a video block is on the page (another application's edits)
        if self.media.poll_notes() { if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        if !self.blank && self.ed.doc.walk().iter().any(|b| matches!(&b.kind, Kind::Media { descriptor } if descriptor.contains("\"kind\":\"video\""))) {
            let now = Instant::now(); let due = *self.files.notes_at.get_or_insert(now + std::time::Duration::from_millis(2100));
            // (a helper thread wakes the loop for it: the loop's own deadline belongs to whatever is due soonest)
            static WATCH: std::sync::Once = std::sync::Once::new();
            WATCH.call_once(|| { let _ = std::thread::Builder::new().name("mn-notes-watch".into()).spawn(|| loop { std::thread::sleep(std::time::Duration::from_millis(2100)); if let Some(p) = WAKE.get() { p.wake_up(); } }); });
            if now >= due { self.files.notes_at = Some(now + std::time::Duration::from_millis(2100)); if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
        if let Some(id) = self.files.pdf_test { if let Some(t) = self.pdf_boxes(id).and_then(|b| self.media.page_text(&b.3)) { self.files.pdf_test = None; if !t.chars.is_empty() { println!("pdf: {} characters on the page, {} of text in words", t.chars.len(), t.words(0, t.chars.len() - 1).len()); self.files.pdf = Some(PdfSel { id, a: 0, b: t.chars.len() - 1, drag: false }); } else { println!("pdf: no text on the page"); } } if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        if std::env::var_os("MN_DRAW").is_some() && self.frames < 300 { if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        // MN_DRAW=1 (with MN_SEEK): once the frame is shown an arrow is drawn on it as the pointer would, and what was written is printed a little later
        if self.frames == 150 && std::env::var_os("MN_DRAW").is_some() { if let (Some((geo, cur)), true) = (self.video_geo(), self.player.as_ref().is_some_and(|p| p.shown)) {
            let to = |x: f64, y: f64| geo.to_norm(geo.rect[0] + geo.rect[2] * x, geo.rect[1] + geo.rect[3] * y);
            let c = self.player.as_mut().and_then(|p| { p.ann.active = true; p.ann.tool = qcv_annotate::Tool::Arrow; p.ann.color = [1.0, 0.85, 0.1, 1.0]; p.ann.width = 9.0; p.ann.press(to(0.2, 0.25), &cur, &geo); p.ann.motion(to(0.5, 0.4), &cur, &geo, true); p.ann.release(to(0.62, 0.5), &cur, &geo) });
            println!("draw: committed {:?} on {}", c.as_ref().map(|c| (c.key.clone(), c.new_note)), cur.key); if let Some(c) = c { self.annotated(c); } } }
        if self.frames == 260 && std::env::var_os("MN_DRAW").is_some() { if let Some(p) = self.player.as_ref() { if let Some(d) = p.ann.dir() { let mut names: Vec<String> = std::fs::read_dir(qcv_notes::io::images_dir(&d)).map(|r| r.flatten().map(|e| format!("{} {}", e.file_name().to_string_lossy(), e.metadata().map(|m| m.len()).unwrap_or(0))).collect()).unwrap_or_default(); names.sort(); println!("draw: pictures {names:?}; notes {}", p.ann.notes.len()); } } }
        // MN_PLAY_ACT=1 (with MN_PLAY): once it plays, the actions QCView's keys stand for are applied and their effect printed
        if self.frames == 150 && std::env::var_os("MN_PLAY_ACT").is_some() { if let Some(p) = self.player.as_mut() { use qcv_player::Action as A; let now = Instant::now();
            p.t.pause(); let at = p.t.current_frame(); for _ in 0..3 { p.t.apply(A::Step(1), now); } let stepped = p.t.current_frame();
            p.t.apply(A::CycleReviewSpeed, now); p.t.apply(A::ToggleMute, now); p.t.apply(A::ToggleLoop, now);
            println!("act: paused at {at}, three steps -> {stepped}; speed {}, muted {}, loop {}", p.t.review_speed(), p.t.audio.is_muted(), p.t.loop_enabled());
            save_player_settings((p.t.loop_enabled(), p.t.review_speed(), p.t.audio.volume(), p.t.audio.is_muted())); println!("act: kept {:?}", player_settings());
            p.t.apply(A::SeekToEnd, now); println!("act: end -> frame {} of {}", p.t.current_frame(), p.t.frame_count()); p.awake = now + std::time::Duration::from_millis(1500); } }
        // notes: pictures that reached the disk are read anew; the annotated picture owed is composed; another application's
        // edits of the playing clip's notes are taken in; what the toolbar shows of the current video is worked out
        { let mut done = vec![]; self.files.written.retain(|rx| match rx.try_recv() { Ok(p) => { done.push(p); false } Err(std::sync::mpsc::TryRecvError::Empty) => true, Err(_) => false });
          if !done.is_empty() { for p in done { self.media.forget_file(&p); } if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
        if let Some((key, due)) = self.files.annot_due.clone() { let now = Instant::now();
            if now >= due && !self.files.drawing && self.files.written.is_empty() { self.files.annot_due = None; self.compose_annotated(&key); } else if let Some(g) = &self.gpu { if now >= due { g.window.request_redraw(); } } }
        { let now = Instant::now(); let mut fresh = None; if let Some(p) = self.player.as_mut() { if !self.files.drawing && p.ann.poll_disk(now) { fresh = Some((p.path.clone(), p.ann.notes.clone())); } } if let Some((path, notes)) = fresh { self.media.set_notes(&path, notes); if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
        { use qcv_annotate::Tool as A; use toolbar::Tool as T;
          let shown = self.video_current().filter(|_| self.files.canvas.is_none() || self.files.card.is_some()).map(|id| { let d = self.video_boxes(id).map(|b| b.3).unwrap_or_default(); let rec: serde_json::Value = serde_json::from_str(&d).unwrap_or_default();
              let at = self.player.as_ref().filter(|p| p.id == id).map(|p| p.t.current_frame() as i64).unwrap_or(rec.get("frame").and_then(|f| f.as_i64()).unwrap_or(0));
              let notes = self.media.notes(&d); let note = notes.iter().find(|n| n.frame == at); let c = self.files.pen.unwrap_or([1.0, 0.0, 0.0, 1.0]);
              toolbar::VideoShown { tool: self.files.tool.map(|t| match t { A::Select => T::DrawSelect, A::Freehand => T::DrawPen, A::Rectangle => T::DrawRect, A::Oval => T::DrawOval, A::Arrow => T::DrawArrow, A::Line => T::DrawLine, A::Eraser => T::DrawErase }), pen: peniko::Color::new(c), on_note: note.is_some(), addressed: note.is_some_and(|n| n.addressed) } });
          if shown != self.files.vshown { self.files.vshown = shown; if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
        { use ink::{Ink as I, Pick as P}; use toolbar::Tool as T; self.files.iblock = self.ink_block();
          let shown = self.files.iblock.as_ref().filter(|_| self.files.vshown.is_none()).map(|_| toolbar::InkShown { page: true, pen: peniko::Color::new(self.files.pen.unwrap_or([1.0, 0.0, 0.0, 1.0])),
              tool: self.files.pick.map(|p| match p { P::Select => T::DrawSelect, P::Draw(I::Pen) => T::DrawPen, P::Draw(I::Line) => T::DrawLine, P::Draw(I::Arrow) => T::DrawArrow, P::Draw(I::Rect) => T::DrawRect, P::Draw(I::Oval) => T::DrawOval, P::Draw(I::Eraser) => T::DrawErase }) });
          if shown != self.files.ishown { self.files.ishown = shown; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
          // (scripted, MN_CANVAS=6) the canvas's first video card is selected and started once it is there
          if self.files.cv_test { let id = self.files.canvas.as_mut().and_then(|cv| { let id = cv.els.iter().find(|e| e.kind == "media" && e.record.contains("\"kind\":\"video\"")).map(|e| e.id)?; cv.sel = vec![id]; Some(id) }); // (MN_STACK=front|behind: the canvas's first picture is laid half over the video, in front of it or behind it)
              let stack = std::env::var("MN_STACK").ok(); let ready = stack.is_none() || self.files.canvas.as_ref().is_some_and(|cv| cv.els.iter().any(|e| e.kind == "media" && !e.record.contains("\"kind\"")));
              if let (Some(id), true) = (id, ready) { self.files.cv_test = false;
                  if let (Some(how), Some(cv)) = (stack, self.files.canvas.as_mut()) { let v = cv.els.iter().find(|e| e.id == id).map(|e| (e.x, e.y, e.w, e.z)); if let (Some((x, y, w, z)), Some(pic)) = (v, cv.els.iter_mut().find(|e| e.kind == "media" && !e.record.contains("\"kind\""))) { pic.x = x + w / 2.0; pic.y = y + 40.0; pic.z = if how == "front" { z + 1 } else { z - 1 }; } }
                  let ok = self.video_start(id, std::env::var_os("MN_PLAY").is_some()); println!("canvas video: started {ok}"); } }
          if self.files.ink_test && self.layout.is_some() { if self.files.iblock.is_some() { self.files.ink_test = false; self.page_ink_test(); }
              // (scripted) the block to draw on is made the current one once it is in the note
              else { let pdf = std::env::var("MN_PAGE_INK").is_ok_and(|v| v == "2"); if let Some(id) = self.ed.doc.walk().iter().find(|b| matches!(&b.kind, Kind::Media { descriptor } if if pdf { descriptor.contains("\"kind\":\"pdf\"") } else { !descriptor.contains("\"kind\"") })).map(|b| b.id) { self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: id, at: 0 }))); if let Some(g) = &self.gpu { g.window.request_redraw(); } } } } }
        // text input is off while a video block is the current one: its keys are held to shuttle and to step, and the
        // system's text input would answer a held letter with its accents and type the one chosen beside the video
        { let want = self.video_current().is_none(); if let Some(g) = self.gpu.as_mut() { if g.ime_on != want { g.ime_on = want;
            if want { ime(g.window.as_ref().as_ref(), g.ime_area.or(Some(Rect::new(0.0, 0.0, 1.0, 1.0))), true); } else if let Err(e) = g.window.request_ime_update(winit::window::ImeRequest::Disable) { log::debug!("ime: {e}"); } } } }
        if std::mem::take(&mut self.files.vstop) { self.video_stop(); }
        // (the note is laid out again - a picture's shape became known: where its text is has to be asked again)
        if std::mem::take(&mut self.files.relay) { self.layout = None; self.geom_moved = Some(Instant::now()); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        for c in std::mem::take(&mut self.later) { self.menu_cmd(c); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        if let Some(at) = [self.bar.next_change(Instant::now()), self.foot.next_change(Instant::now()), self.sidebar.as_ref().and_then(|sb| sb.next_change(Instant::now()))].into_iter().flatten().min() { if Instant::now() >= at { if let Some(g) = &self.gpu { g.window.request_redraw(); } } else { el.set_control_flow(ControlFlow::WaitUntil(at)); } }
        // (the queue is taken and let go BEFORE the commands run: one of them may put a system panel up, and a menu key
        // pressed in the panel comes to the same queue on this thread)
        // a file panel has been answered
        if let Some(paths) = PANEL_DONE.lock().unwrap().take() { if let Some(then) = self.files.panel_then.take() { if !paths.is_empty() { then(self, paths); if let Some(g) = &self.gpu { g.window.request_redraw(); } } } }
        #[cfg(target_os = "macos")] let menu_events = std::mem::take(&mut *MENU_EVENTS.lock().unwrap());
        #[cfg(target_os = "macos")] for ev in menu_events { let c = self.menu.as_ref().and_then(|m| m.cmd(&ev.id)); text_element::key_log(format!("menu {c:?}")); if let Some(c) = c { self.menu_cmd(c); }
            // a command changes what is shown, and no window event follows it to ask for the frame
            if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        if let Some(t) = self.geom_moved { let due = t + std::time::Duration::from_millis(150); if Instant::now() >= due { self.geom_moved = None; #[cfg(target_os = "macos")] text_element::relaid(); } else { el.set_control_flow(ControlFlow::WaitUntil(due)); } }
        // a dialog's answer, or a save that was to be followed by closing, ended the session
        if self.quit { #[cfg(target_os = "macos")] text_element::uninstall(); self.finish(); el.exit(); return }
        // work found from an earlier session is offered once the window is up
        if self.recovered && self.gpu.is_some() && self.confirm.is_none() && self.frames > 0 { self.recovered = false; self.offer_recovered(); }
        // a save's steps come back from the helper thread
        if let Some(step) = self.saver.poll() { self.save_step(step); }
        self.unpack_tick(); self.import_tick();
        if self.files.gather.is_some() { let out = self.files.gather.as_mut().map(|w| w.tick()).unwrap_or(gather_win::Out::None); self.gather_out(out); }
        if let Some(sv) = &self.saving { if sv.started.elapsed().as_secs() >= 20 && self.saver.waiting().is_some_and(|w| w.as_secs() >= 20) {
            // no answer from the document's place: stop waiting (the call itself cannot be ended; its late result is dropped)
            eprintln!("save: the document's place has not answered for 20 seconds. It is treated as unreachable; your changes are kept on this machine. Whether the save went through is not known — it is looked at again at the next Save.");
            self.saver.abandon(); self.saving = None; self.title();
            self.save_failed("The document\u{2019}s location has not answered for 20 seconds. It is not known whether the save went through; it will be looked at again at the next Save.");
        } else { el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(40))); } }
        // autosave to the staging a second after the last edit
        if let Some(t) = self.store.as_ref().and_then(|s| s.edited) { let due = t + std::time::Duration::from_millis(1000); if Instant::now() >= due { self.autosave(); } else { el.set_control_flow(ControlFlow::WaitUntil(due)); } }
        if self.seconds > 0.0 { if let Some(t0) = self.t0 { let dl = t0 + std::time::Duration::from_secs_f32(self.seconds); if Instant::now() >= dl { #[cfg(target_os = "macos")] text_element::uninstall(); self.finish(); el.exit(); return } el.set_control_flow(ControlFlow::WaitUntil(dl)); } }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => { self.autosave(); self.request_close(Pending::Quit); if self.quit { #[cfg(target_os = "macos")] text_element::uninstall(); self.finish(); el.exit(); } }
            WindowEvent::Occluded(false) => { self.warm = self.warm.max(2); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::Focused(true) => { startup_log("window focused"); self.warm = self.warm.max(2); if let Some(g) = &self.gpu { g.window.request_redraw(); } #[cfg(target_os = "macos")] text_element::focused(); }
            WindowEvent::SurfaceResized(s) => if let Some(g) = self.gpu.as_mut() {
                g.presenter.resize(s.width, s.height); g.presenter.engine.set_ui_scale(g.window.scale_factor() as f32);
                g.ui.resize(&g.presenter.engine.device, s.width, s.height); g.presenter.engine.set_overlay(Some(g.ui.view()));
                g.window.request_redraw();
            },
            WindowEvent::Moved(_) => { self.geom_moved = Some(Instant::now()); }
            WindowEvent::ScaleFactorChanged { .. } => { self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::MouseWheel { delta, .. } => if let Some(g) = &self.gpu {
                let s = g.window.scale_factor();
                if let Some(w) = self.files.gather.as_mut().filter(|w| w.shown) { let dy = match delta { MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * s, MouseScrollDelta::PixelDelta(p) => p.y, _ => 0.0 }; w.wheel(-dy); g.window.request_redraw(); return }
                if let Some(sb) = self.sidebar.as_mut() { if sb.rail.hit(self.pointer.0, self.pointer.1) { let dy = match delta { MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * s, MouseScrollDelta::PixelDelta(p) => p.y, _ => 0.0 }; sb.rail.wheel(-dy, Instant::now()); g.window.request_redraw(); return } }
                // over an open canvas (row 322): the wheel pans, with Cmd (or Ctrl) it zooms about the pointer
                // (a note edited in a card: over the card the wheel is the note's when it has more than the card shows; a zoom leaves the card)
                let in_card = self.files.card.as_ref().is_some_and(|c| c.rect.contains(kurbo::Point::new(self.pointer.0, self.pointer.1))) && !(self.mods.1 || self.ctrl) && self.layout.as_ref().zip(self.files.card.as_ref()).is_some_and(|(l, c)| l.height as f64 + 2.0 * CARD_PAD * s > c.rect.height() + 1.0);
                if self.files.card.is_some() && (self.mods.1 || self.ctrl) { self.card_leave(); }
                if !in_card && self.files.canvas.as_ref().is_some_and(|c| c.rect.contains(kurbo::Point::new(self.pointer.0, self.pointer.1))) { let (dx, dy) = match delta { MouseScrollDelta::LineDelta(x, y) => (x as f64 * 60.0 * s, y as f64 * 60.0 * s), MouseScrollDelta::PixelDelta(p) => (p.x, p.y), _ => (0.0, 0.0) };
                    let (at, zoom, side) = (self.pointer, self.mods.1 || self.ctrl, self.mods.0); if let Some(cv) = self.files.canvas.as_mut() { if zoom { cv.zoom_by(2f64.powf(dy / (240.0 * s)), at); } else if side && dx == 0.0 { cv.pan_by(dy, 0.0); } else { cv.pan_by(dx, dy); } }
                    if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                // sideways (a trackpad's own, or the wheel with Shift) over a table wider than its place: the table scrolls inside its block
                let (dx, dy) = match delta { MouseScrollDelta::LineDelta(x, y) => (x as f64 * 60.0 * s, y as f64 * 60.0 * s), MouseScrollDelta::PixelDelta(p) => (p.x, p.y), _ => (0.0, 0.0) };
                let side = if self.mods.0 && dx == 0.0 { dy } else if dx.abs() > dy.abs() { dx } else { 0.0 };
                if side != 0.0 { let (nx, ny) = self.note_point(self.pointer); let over = self.layout.as_ref().and_then(|l| { let (id, _) = l.hit(&self.ed.doc, nx, ny)?; let t = l.wide_table_of(&self.ed.doc, id)?; let r = l.rect(&self.ed.doc, t)?; (ny >= r.y0 && ny <= r.y1).then(|| (t, l.table_scroll(t))) });
                    if let Some((t, Some((full, vis, at)))) = over { let to = (at as f64 - side).clamp(0.0, (full - vis).max(0.0) as f64); self.files.tscroll.insert(t, to as f32); let map = self.files.tscroll.clone(); if let Some(l) = self.layout.as_mut() { l.set_table_scrolls(&map); }
                        if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
                self.scroll -= dy;
                if let Some(g) = &self.gpu { g.window.request_redraw(); }
            },
            WindowEvent::PointerLeft { .. } => { self.chrome_pointer(min_ui::Event::Leave); self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            // files dragged in from outside (row 310): a line shows where they would land; over anything but the page they are refused
            WindowEvent::DragEntered { id: tid, position } => {
                let files = el.data_transfer(tid).map(|dt| dt.has_type(&TypeHint::UriList)).unwrap_or(false);
                self.files.drag = if files { position.and_then(|p| self.file_drop_at(p.x, p.y)) } else { None };
                el.set_valid_dnd_actions(tid, if files && (position.is_none() || self.files.drag.is_some()) { &[DndAction::Copy] } else { &[] }).ok();
                if let Some(g) = &self.gpu { g.window.request_redraw(); }
            }
            WindowEvent::DragPosition { id: tid, position, .. } => {
                let files = el.data_transfer(tid).map(|dt| dt.has_type(&TypeHint::UriList)).unwrap_or(false);
                let at = if files { self.file_drop_at(position.x, position.y) } else { None };
                el.set_valid_dnd_actions(tid, if at.is_some() { &[DndAction::Copy] } else { &[] }).ok();
                if at != self.files.drag { self.files.drag = at; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            }
            WindowEvent::DragLeft { .. } => { self.files.drag = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::DragDropped { id: tid, .. } => {
                if let Some((at, _)) = self.files.drag.take() { if let Ok(serial) = el.fetch_data_transfer(tid, &TypeHint::UriList) { self.files.fetches.insert(serial, at); } }
                if let Some(g) = &self.gpu { g.window.request_redraw(); }
            }
            WindowEvent::DataTransferReceived { serial, value, .. } => {
                if let Some(at) = self.files.fetches.remove(&serial) { if let Ok(uris) = value.try_as_uris() {
                    let paths: Vec<std::path::PathBuf> = uris.iter().filter_map(|u| if u.starts_with("file:") { min_paths::url::file_url_to_path(u) } else if u.contains("://") { None } else { Some(u.to_string()) }).map(Into::into).collect();
                    self.add_files(paths, at); } }
            }
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.ctrl = st.control_key(); self.mods = (st.shift_key(), if cfg!(target_os = "macos") { st.meta_key() } else { st.control_key() }, st.alt_key()); }
            WindowEvent::PointerMoved { position, .. } => { self.pointer = (position.x, position.y);
                if self.files.resize.is_some() { self.resize_to(position.x); return }
                if let Some((a, b, from, free, total, ka, kb)) = self.files.gap_drag.clone() { let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
                    if let (Kind::Column { width: wa }, Kind::Column { width: wb }) = (ka, kb) { let min = (mn_layout::COLUMN_MIN as f64 * s / free.max(1.0)) as f32 * total; let both = wa + wb;
                        if both > 2.0 * min { let na = (wa + ((position.x - from) / free.max(1.0)) as f32 * total).clamp(min, both - min); let _ = self.ed.doc.set_kind(a, Kind::Column { width: na }); let _ = self.ed.doc.set_kind(b, Kind::Column { width: both - na }); self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
                    return }
                if let Some((table, col, from, w0, before)) = self.files.col_drag.clone() { let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
                    if let Kind::Table { header_rows, mut columns } = before { if columns.len() <= col { columns.resize(col + 1, Default::default()); } columns[col].width = Some((w0 + (position.x - from) / s).round().clamp(48.0, 4000.0) as u32);
                        let _ = self.ed.doc.set_kind(table, Kind::Table { header_rows, columns }); self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
                    return }
                if self.player.as_ref().is_some_and(|p| p.scrub) { self.video_scrub(position.x); return }
                if self.files.drawing { if let Some((geo, cur)) = self.video_geo() { let c = self.player.as_mut().and_then(|p| p.ann.motion(geo.to_norm(position.x, position.y), &cur, &geo, true)); if let Some(c) = c { self.annotated(c); } } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                if let Some((id, true)) = self.files.pdf.as_ref().map(|x| (x.id, x.drag)) { if let Some(at) = self.pdf_char(id, position.x, position.y) { if let Some(sel) = self.files.pdf.as_mut() { if sel.b != at { sel.b = at; if let Some(g) = &self.gpu { g.window.request_redraw(); } } } } return }
                // the handle is held: past a few pixels it is a drag, and the place the blocks would land follows the pointer
                if self.grab.is_some() {
                    let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); let (x, y) = self.note_point(self.pointer);
                    // over the tree: another note's row takes the blocks (to its end); over the page: a place among the blocks
                    let here = self.store.as_ref().map(|st| st.note);
                    let to_note = self.sidebar.as_ref().filter(|sb| sb.rail.hit(position.x, position.y)).and_then(|sb| sb.id_at(position.x, position.y).filter(|id| Some(*id) != here && sb.row(*id).is_some_and(|r| r.kind == mn_store::DocKind::Note)));
                    let over_tree = self.sidebar.as_ref().is_some_and(|sb| sb.rail.hit(position.x, position.y));
                    let drop = if over_tree { None } else { self.layout.as_ref().and_then(|l| self.ed.drop_at(l, x, y)) };
                    if let Some(gr) = self.grab.as_mut() { if !gr.dragging && ((gr.from.0 - position.x).abs() > 4.0 * s || (gr.from.1 - position.y).abs() > 4.0 * s) { gr.dragging = true; } if gr.dragging { gr.drop = drop; gr.to_note = to_note; } }
                    let shown = self.grab.as_ref().filter(|x| x.dragging).and_then(|x| x.to_note); if let Some(sb) = self.sidebar.as_mut() { sb.drop_row = shown; }
                    self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                let taken = !self.pressed && self.chrome_pointer(min_ui::Event::Move { x: position.x, y: position.y });
                self.set_cursor();
                if taken { return }
                // a note's row dragged from the sidebar towards the canvas
                if let Some((_, from, moved)) = self.files.row_drag.as_mut() { if !*moved && ((from.0 - position.x).abs() > 5.0 || (from.1 - position.y).abs() > 5.0) { *moved = true; } if *moved { self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
                if self.files.gest.is_some() { self.page_drag(position.x, position.y); return }
                // a canvas's video (row 326): its line being scrubbed, its frame being drawn on
                if self.files.canvas.is_some() && self.files.card.is_none() {
                    if self.player.as_ref().is_some_and(|p| p.scrub) { self.video_scrub(position.x); return }
                    if self.files.drawing { if let Some((geo, cur)) = self.video_geo() { let c = self.player.as_mut().and_then(|p| p.ann.motion(geo.to_norm(position.x, position.y), &cur, &geo, true)); if let Some(c) = c { self.annotated(c); } } if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
                if self.files.canvas.is_some() && self.files.card.is_none() { if let (Some(cv), Some(g)) = (self.files.canvas.as_mut(), self.gpu.as_mut()) { cv.pressure = pen_pressure(); cv.ctrl = self.ctrl; if cv.pointer(&min_ui::Event::Move { x: position.x, y: position.y }, false, self.mods.0, &mut g.fonts) { g.window.request_redraw(); } } self.set_cursor(); return }
                if self.table_bar_event(min_ui::Event::Move { x: position.x, y: position.y }) { return }
                if self.files.find.is_some() { let e = min_ui::Event::Move { x: position.x, y: position.y }; let r = match (self.files.find.as_mut(), self.gpu.as_mut()) { (Some(p), Some(g)) => Some(p.pointer(&e, &mut g.fonts)), _ => None }; if let Some((out, taken)) = r { self.find_out(out); if taken { self.set_cursor(); return } } }
                if self.files.link.is_some() { let e = min_ui::Event::Move { x: position.x, y: position.y }; let r = match (self.files.link.as_mut(), self.gpu.as_mut()) { (Some(p), Some(g)) => Some(p.pointer(&e, &mut g.fonts)), _ => None }; if let Some((out, taken)) = r { self.link_out(out); if taken { self.set_cursor(); return } } }
                if self.files.picker.is_some() { let e = min_ui::Event::Move { x: position.x, y: position.y }; let r = match (self.files.picker.as_mut(), self.gpu.as_mut()) { (Some(p), Some(g)) => Some(p.pointer(&e, &mut g.fonts)), _ => None }; if let Some((out, taken)) = r { self.choice_out(out); if taken { self.set_cursor(); return } } }
                if self.pressed && !self.blank { if let Some(l) = self.layout.as_ref() { let (x, y) = self.note_point(self.pointer); self.ed.point(l, x, y, true); self.follow = true; if let Some(g) = &self.gpu { g.window.request_redraw(); } } } }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Middle), .. } if self.files.canvas.is_some() => { self.pointer = (position.x, position.y);
                let e = if state == ElementState::Pressed { min_ui::Event::Down { x: position.x, y: position.y, button: min_ui::Button::Primary } } else { min_ui::Event::Up { x: position.x, y: position.y, button: min_ui::Button::Primary } };
                if let (Some(cv), Some(g)) = (self.files.canvas.as_mut(), self.gpu.as_mut()) { cv.pointer(&e, true, false, &mut g.fonts); g.window.request_redraw(); } self.set_cursor(); }
            WindowEvent::PinchGesture { delta, .. } if self.files.canvas.is_some() => { self.card_leave(); let at = self.pointer; if let Some(cv) = self.files.canvas.as_mut() { cv.zoom_by((1.0 + delta).clamp(0.5, 2.0), at); } if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Right), .. } => {
                // an open menu takes the press (outside it: it closes); otherwise a press over the tree opens one
                let e = if state == ElementState::Pressed { min_ui::Event::Down { x: position.x, y: position.y, button: min_ui::Button::Secondary } } else { min_ui::Event::Up { x: position.x, y: position.y, button: min_ui::Button::Secondary } };
                if self.ctx.is_open() || self.pal.is_open() { self.chrome_pointer(e); }
                else if state == ElementState::Pressed && self.files.card.as_ref().is_none_or(|c| !c.rect.contains(kurbo::Point::new(position.x, position.y))) && self.files.canvas.as_ref().is_some_and(|c| c.rect.contains(kurbo::Point::new(position.x, position.y)) && !self.bar.hit(position.x, position.y) && !self.foot.hit(position.x, position.y)) { use min_ui::menu::MenuItem as I; self.card_leave();
                    // the card under the pointer is selected (the selected ones stay when it is one of them), and the menu is theirs
                    if let Some(cv) = self.files.canvas.as_mut() { cv.pick_at(position.x, position.y); } let note = self.card_note();
                    if let (Some(cv), Some(g)) = (self.files.canvas.as_mut(), self.gpu.as_mut()) { let any = !cv.sel.is_empty(); let (pdf, file) = (cv.selected_media("pdf").is_some(), cv.selected_media("file").is_some()); let (frame, locked) = (cv.selected_frame(), cv.any_locked()); let (w, h) = g.presenter.size(); let s = g.window.scale_factor(); let mac = cfg!(target_os = "macos"); let k = |m: &str, o: &str| if mac { m.to_string() } else { o.to_string() };
                        self.files.cmenu = true; self.files.tmenu = None; self.files.lmenu = None; self.ctx_blocks = false; self.ctx_for = None; self.ctx_notes.clear(); self.files.canvas_at.set(Some((position.x, position.y)));
                        self.ctx.open_at(vec![I::with_shortcut("Cut", any, &k("\u{2318}X", "Ctrl+X")), I::with_shortcut("Copy", any, &k("\u{2318}C", "Ctrl+C")), I::with_shortcut("Paste", true, &k("\u{2318}V", "Ctrl+V")), I::with_shortcut("Duplicate", any, &k("\u{2318}D", "Ctrl+D")), I::with_shortcut("Delete", any, "\u{232b}"), I::Separator,
                            I::with_shortcut("Bring to Front", any, &k("\u{2318}]", "Ctrl+]")), I::with_shortcut("Send to Back", any, &k("\u{2318}[", "Ctrl+[")), I::Separator,
                            // frames, lining up, locking (row 327)
                            I::with_shortcut("Group", any && frame.is_none(), &k("\u{2318}G", "Ctrl+G")), I::with_shortcut("Ungroup", frame.is_some(), &k("\u{21e7}\u{2318}G", "Ctrl+Shift+G")), I::action("Rename Frame", frame.is_some()), I::action("Fit to Contents", frame.as_ref().is_some_and(|f| f.1)), I::action("Shape\u{2026}", frame.is_some()), I::action("Tint\u{2026}", frame.is_some()),
                            I::Separator, I::action("Align\u{2026}", cv.sel.len() >= 2), I::action(if locked { "Unlock" } else { "Lock" }, any), I::Separator, I::with_shortcut("Previous Page", pdf, &k("\u{2325}\u{2190}", "Alt+Left")), I::with_shortcut("Next Page", pdf, &k("\u{2325}\u{2192}", "Alt+Right")), I::action("Open File", file), I::action(if mac { "Reveal in Finder" } else { "Show in Explorer" }, file), I::Separator, I::action("Add Note\u{2026}", true), I::with_shortcut("Open Note", note.is_some(), &k("\u{2318}\u{21a9}", "Ctrl+Enter")), I::action("Promote to Note", note.is_some_and(|n| n.2)), I::Separator, I::with_shortcut("Select All", true, &k("\u{2318}A", "Ctrl+A")), I::with_shortcut("Fit Everything", true, &k("\u{2318}0", "Ctrl+0"))], &mut g.fonts, position.x, position.y, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), s); g.window.request_redraw(); } }
                else if state == ElementState::Pressed { self.context(position.x, position.y); }
            }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Left), .. } => {
                self.pointer = (position.x, position.y);
                let e = if state == ElementState::Pressed { min_ui::Event::Down { x: position.x, y: position.y, button: min_ui::Button::Primary } } else { min_ui::Event::Up { x: position.x, y: position.y, button: min_ui::Button::Primary } };
                // a note's row pressed while a canvas is open may be dragged onto it: it becomes a card where it is let go (row 322, stage 2)
                if state == ElementState::Pressed && self.files.canvas.is_some() && self.files.card.is_none() && !self.modal() && !self.ctx.is_open() { self.files.row_drag = self.sidebar.as_ref().filter(|sb| sb.rail.hit(position.x, position.y) && !sb.typing()).and_then(|sb| sb.id_at(position.x, position.y)).filter(|id| self.sidebar.as_ref().and_then(|sb| sb.row(*id)).is_some_and(|r| r.kind == mn_store::DocKind::Note)).map(|id| (id, (position.x, position.y), false)); }
                if state == ElementState::Released { if let Some((id, _, moved)) = self.files.row_drag.take() { if moved { if let Some(sb) = self.sidebar.as_mut() { sb.pressed_row = None; }
                    if let Some(cv) = self.files.canvas.as_mut().filter(|c| c.rect.contains(kurbo::Point::new(position.x, position.y))) { cv.add_note(id, Some((position.x, position.y))); }
                    self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } return } } }
                if !(self.pressed && state == ElementState::Released) && self.chrome_pointer(e) { self.set_cursor(); return }
                // a canvas is open: the pointer is its own over the page's place (row 322)
                // (a note edited in a card: a press outside the card leaves it - and is the canvas's; inside, it is the note's)
                if state == ElementState::Pressed && !self.pressed { if let Some(c) = &self.files.card { let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); if !Rect::new(c.rect.x0 - 40.0 * s, c.rect.y0, c.rect.x1, c.rect.y1).contains(kurbo::Point::new(position.x, position.y)) && !self.files.find.as_ref().is_some_and(|f| f.hit(position.x, position.y)) && self.files.link.is_none() && self.files.picker.is_none() { self.card_leave(); } } }
                if self.files.canvas.is_some() && self.files.card.is_none() && !self.modal() && !self.ctx.is_open() {
                    if state == ElementState::Released && std::mem::take(&mut self.files.drawing) { if let Some((geo, cur)) = self.video_geo() { let c = self.player.as_mut().and_then(|p| p.ann.release(geo.to_norm(position.x, position.y), &cur, &geo)); if let Some(c) = c { self.annotated(c); } } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                    if state == ElementState::Released && self.player.as_ref().is_some_and(|p| p.scrub) { let mut keep = None; if let Some(p) = self.player.as_mut() { p.t.end_scrub(); p.scrub = false; p.awake = Instant::now() + std::time::Duration::from_millis(800); if !p.t.is_playing() { keep = Some((p.id, p.t.current_frame())); } } if let Some((id, f)) = keep { self.video_keep(id, f); } return }
                    if state == ElementState::Pressed && self.canvas_video_press(position.x, position.y) { return } }
                if self.files.canvas.is_some() && self.files.card.is_none() { self.pressed = state == ElementState::Pressed; let shift = self.mods.0; if let (Some(cv), Some(g)) = (self.files.canvas.as_mut(), self.gpu.as_mut()) { if state == ElementState::Pressed && !cv.rect.contains(kurbo::Point::new(position.x, position.y)) { return } cv.pressure = pen_pressure(); cv.ctrl = self.ctrl; cv.pointer(&e, false, shift, &mut g.fonts); g.window.request_redraw(); } self.canvas_asks(); self.set_cursor(); return }
                if self.table_bar_event(e) { return }
                if self.files.picker.is_some() { let r = match (self.files.picker.as_mut(), self.gpu.as_mut()) { (Some(p), Some(g)) => Some(p.pointer(&e, &mut g.fonts)), _ => None }; if let Some((out, taken)) = r { self.choice_out(out); if taken { return } } }
                if self.files.link.is_some() { let r = match (self.files.link.as_mut(), self.gpu.as_mut()) { (Some(p), Some(g)) => Some(p.pointer(&e, &mut g.fonts)), _ => None }; if let Some((out, taken)) = r { self.link_out(out); if taken { return } } }
                if self.files.find.is_some() { let r = match (self.files.find.as_mut(), self.gpu.as_mut()) { (Some(p), Some(g)) => Some(p.pointer(&e, &mut g.fonts)), _ => None }; if let Some((out, taken)) = r { self.find_out(out); if taken { return } } }
                // a second press on a file chip opens its file (row 320); the first selects it, as any block
                if state == ElementState::Pressed && self.files.tool.is_none() { match self.file_chip_under(position.x, position.y) {
                    Some((id, d)) => { let twice = self.files.chip_click.is_some_and(|(t, b)| b == id && t.elapsed().as_millis() < 450); self.files.chip_click = if twice { None } else { Some((Instant::now(), id)) }; if twice { self.file_open(&d, false); return } }
                    None => self.files.chip_click = None } }
                // a press on a chip in the text opens its picker (row 320)
                if state == ElementState::Pressed && !self.mods.0 && !self.mods.1 && self.files.tool.is_none() && self.files.note_field.is_none() { if let Some(c) = self.chip_under(position.x, position.y) { self.chip_open(c, ""); return } }
                // Cmd + click on a link opens it (a plain click places the caret)
                if state == ElementState::Pressed && self.mods.1 && !self.mods.0 && self.files.tool.is_none() { if let Some(u) = self.link_under(position.x, position.y) { if !open::url(&u) { self.notice = Some((format!("This link is not one minNotes opens: {u}"), Instant::now() + std::time::Duration::from_secs(6))); if let Some(g) = &self.gpu { g.window.request_redraw(); } } return } }
                if state == ElementState::Pressed && !self.blank && !self.mods.0 && self.files.note_field.is_none() && self.files.tool.is_none() && self.typed_click() { return }
                // a note's words being typed: a press outside the field keeps them
                if state == ElementState::Pressed && self.files.note_field.as_ref().is_some_and(|(_, _, f)| !f.hit(position.x, position.y)) { self.note_words_end(true); }
                if self.files.note_field.is_some() { let e = if state == ElementState::Pressed { min_ui::Event::Down { x: position.x, y: position.y, button: min_ui::Button::Primary } } else { min_ui::Event::Up { x: position.x, y: position.y, button: min_ui::Button::Primary } }; if let (Some((_, _, f)), Some(g)) = (self.files.note_field.as_mut(), self.gpu.as_mut()) { f.pointer(&e, &mut g.fonts); g.window.request_redraw(); } return }
                // a drawing on a video's frame ends with the release
                if state == ElementState::Released && std::mem::take(&mut self.files.drawing) { if let Some((geo, cur)) = self.video_geo() { let c = self.player.as_mut().and_then(|p| p.ann.release(geo.to_norm(position.x, position.y), &cur, &geo)); if let Some(c) = c { self.annotated(c); } } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                // a video block (row 316): a click on its picture or its play button plays / pauses; its line is dragged to scrub
                if state == ElementState::Released && self.player.as_ref().is_some_and(|p| p.scrub) { let mut keep = None; if let Some(p) = self.player.as_mut() { p.t.end_scrub(); p.scrub = false; p.awake = Instant::now() + std::time::Duration::from_millis(800); if !p.t.is_playing() { keep = Some((p.id, p.t.current_frame())); } } if let Some((id, f)) = keep { self.video_keep(id, f); } return }
                if state == ElementState::Pressed { if let Some((id, pic, strip, _)) = self.video_under(position.x, position.y) {
                    let (nx, ny) = self.note_point(self.pointer); if let Some(l) = self.layout.as_ref() { self.ed.point(l, nx, ny, false); }
                    let pt = kurbo::Point::new(position.x, position.y); let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); let (_, track) = paint::video_strip(strip, s);
                    // a drawing tool is armed: the press draws on the frame (the video pauses for it, and becomes the playing one first)
                    if pic.contains(pt) && self.files.tool.is_some() {
                        if !self.video_start(id, false) { return }
                        let mut keep = None; if let Some(p) = self.player.as_mut() { if p.t.is_playing() { p.t.pause(); keep = Some(p.t.current_frame()); } } if let Some(f) = keep { self.video_keep(id, f); }
                        if let (Some((geo, cur)), true) = (self.video_geo(), self.player.as_ref().is_some_and(|p| p.shown)) { if let Some(p) = self.player.as_mut() { p.ann.press(geo.to_norm(position.x, position.y), &cur, &geo); self.files.drawing = true; } }
                        if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                    // the caption of the note the playing video stands on: a click opens its words to be typed
                    if pic.contains(pt) && position.y > pic.y1 - 26.0 * s && !self.files.hide_notes && !self.player.as_ref().is_some_and(|p| p.id == id && p.t.is_playing()) {
                        let on_note = { let d = self.video_boxes(id).map(|b| b.3).unwrap_or_default(); let rec: serde_json::Value = serde_json::from_str(&d).unwrap_or_default();
                            let at = self.player.as_ref().filter(|p| p.id == id).map(|p| p.t.current_frame() as i64).unwrap_or(rec.get("frame").and_then(|f| f.as_i64()).unwrap_or(0)); self.media.notes(&d).iter().any(|n| n.frame == at) };
                        if on_note { if self.video_start(id, false) { self.note_words(); } return } }
                    if pic.contains(pt) { self.video_toggle(id); return }
                    // the clip's notes: the eye hides their drawings and captions; a small picture or a tick goes to its frame
                    let notes = self.ed.doc.get(id).and_then(|b| match &b.kind { Kind::Media { descriptor } => Some(self.media.notes(descriptor)), _ => None }).unwrap_or_default();
                    if !notes.is_empty() {
                        if paint::video_eye(strip, s).contains(pt) { self.files.hide_notes = !self.files.hide_notes; if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                        let rec: serde_json::Value = self.video_boxes(id).and_then(|b| serde_json::from_str(&b.3).ok()).unwrap_or_default();
                        let (frames, fps, at) = (rec.get("frames").and_then(|f| f.as_i64()).unwrap_or(1).max(1), rec.get("fps").and_then(|f| f.as_f64()).unwrap_or(24.0), self.player.as_ref().filter(|p| p.id == id).map(|p| p.t.current_frame() as i64).unwrap_or(rec.get("frame").and_then(|f| f.as_i64()).unwrap_or(0)));
                        if let Some(row) = self.video_row(id).filter(|r| r.contains(pt)) {
                            let aspect = rec.get("w").and_then(|w| w.as_f64()).unwrap_or(16.0) / rec.get("h").and_then(|h| h.as_f64()).unwrap_or(9.0).max(1.0);
                            let playing = self.player.as_ref().is_some_and(|p| p.id == id && p.t.is_playing());
                            if let Some(i) = paint::note_thumbs(row, notes.len(), aspect, paint::note_at(&notes, at, fps, playing), s).iter().position(|q| q.contains(pt)) { let again = self.files.thumb_click.is_some_and(|(t, b, k)| b == id && k == i && t.elapsed() < std::time::Duration::from_millis(450)); self.files.thumb_click = Some((Instant::now(), id, i));
                                self.video_seek(id, notes[i].frame as i32); if again { self.files.thumb_click = None; self.note_words(); } }
                            return }
                        if track.inflate(0.0, 12.0 * s).contains(pt) { if let Some(n) = notes.iter().find(|n| { let nx = track.x0 + track.width() * (n.frame.clamp(0, frames - 1) as f64 / (frames - 1).max(1) as f64); (nx - position.x).abs() <= 4.0 * s }) { self.video_seek(id, n.frame as i32); return } }
                    }
                    if track.inflate(4.0 * s, 12.0 * s).contains(pt) { if self.video_start(id, false) { if let Some(p) = self.player.as_mut() { p.t.begin_scrub(); p.scrub = true; } self.video_scrub(position.x); } return }
                    if strip.contains(pt) { self.video_toggle(id); return } } }
                // a picture or a PDF's page with a drawing tool armed (row 325): the gesture is the page's
                if state == ElementState::Released && self.files.gest.is_some() { self.page_release(position.x, position.y); return }
                if state == ElementState::Pressed && self.page_press(position.x, position.y) { return }
                // a PDF block (row 315): its strip turns the page; a drag on its page selects text
                if state == ElementState::Released { if let Some(sel) = self.files.pdf.as_mut().filter(|x| x.drag) { sel.drag = false; if sel.a == sel.b { self.files.pdf = None; } if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
                if state == ElementState::Pressed { let had = self.files.pdf.take().is_some();
                    if let Some((id, page, strip, _)) = self.pdf_under(position.x, position.y) {
                        if strip.contains(kurbo::Point::new(position.x, position.y)) { self.pdf_turn(id, if position.x < strip.center().x { -1 } else { 1 }); return }
                        if page.contains(kurbo::Point::new(position.x, position.y)) { if let Some(at) = self.pdf_char(id, position.x, position.y) {
                            // the block becomes the current one, then the drag is the page's
                            let (nx, ny) = self.note_point(self.pointer); if let Some(l) = self.layout.as_ref() { self.ed.point(l, nx, ny, false); }
                            self.files.pdf = Some(PdfSel { id, a: at, b: at, drag: true }); if let Some(g) = &self.gpu { g.window.request_redraw(); } return } } }
                    if had { if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
                // a corner of the current picture: the drag resizes it (row 312)
                if state == ElementState::Released && self.files.resize.is_some() { self.resize_end(true); return }
                if state == ElementState::Pressed { if let Some((id, r, left, _)) = self.corner_at(position.x, position.y) {
                    let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
                    let lane = self.ed.doc.get(id).and_then(|b| b.parent).and_then(|p| self.layout.as_ref().and_then(|l| l.rect(&self.ed.doc, p))).map(|p| p.width()).or(self.layout.as_ref().map(|l| l.width as f64)).unwrap_or(r.width());
                    if let Some(Kind::Media { descriptor }) = self.ed.doc.get(id).map(|b| b.kind.clone()) { self.files.resize = Some(Resize { id, before: descriptor, from_x: position.x, width: r.width() / s, max: lane / s, left }); self.set_cursor(); return } } }
                // the caret's table (row 317): a column's border is dragged to set its width; the row's and the column's handles
                // open their menus; the strips under and beside it add a row, a column, at the end
                // the gap between two columns is dragged: they trade width (one undo step at the release)
                if state == ElementState::Released { if let Some((a, b, _, _, _, ka, kb)) = self.files.gap_drag.take() {
                    let now = (self.ed.doc.get(a).map(|x| x.kind.clone()), self.ed.doc.get(b).map(|x| x.kind.clone())); let _ = self.ed.doc.set_kind(a, ka.clone()); let _ = self.ed.doc.set_kind(b, kb.clone());
                    if let (Some(na), Some(nb)) = now { if na != ka { let t = self.now_ms(); let d = self.ed.set_kinds(vec![(a, na), (b, nb)], t).unwrap_or(Dirty::Nothing); self.apply(d); } }
                    self.layout = None; self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
                if state == ElementState::Pressed && self.files.tool.is_none() { if let Some((a, b, free, total)) = self.column_gap_at(position.x, position.y) { if let (Some(ka), Some(kb)) = (self.ed.doc.get(a).map(|x| x.kind.clone()), self.ed.doc.get(b).map(|x| x.kind.clone())) {
                    self.files.gap_drag = Some((a, b, position.x, free, total, ka, kb)); self.set_cursor(); return } } }
                if state == ElementState::Released { if let Some((table, _, _, _, before)) = self.files.col_drag.take() {
                    let now = self.ed.doc.get(table).map(|b| b.kind.clone()); let _ = self.ed.doc.set_kind(table, before.clone());
                    if let Some(new) = now.filter(|n| *n != before) { let t = self.now_ms(); let d = self.ed.set_kinds(vec![(table, new)], t).unwrap_or(Dirty::Nothing); self.apply(d); }
                    self.layout = None; self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
                if state == ElementState::Pressed { if let Some(ui) = self.table_ui() { let pt = kurbo::Point::new(position.x, position.y); let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
                    if position.y >= ui.y.0 && position.y <= ui.y.1 { if let Some((col, _)) = ui.borders.iter().find(|(_, bx)| (bx - position.x).abs() <= 4.0 * s) {
                        if let (Some(Kind::Table { .. }), Some(l)) = (self.ed.doc.get(ui.table).map(|b| b.kind.clone()), self.layout.as_ref()) { let before = self.ed.doc.get(ui.table).unwrap().kind.clone();
                            let w = self.ed.doc.children(Some(ui.table)).first().and_then(|r| self.ed.doc.children(Some(*r)).get(*col)).and_then(|c| l.rect(&self.ed.doc, *c)).map(|q| q.width() / s).unwrap_or(160.0);
                            self.files.col_drag = Some((ui.table, *col, position.x, w, before)); self.set_cursor(); return } } }
                    if ui.row.contains(pt) || ui.col.contains(pt) { let column = ui.col.contains(pt); let shift = self.mods.0; self.ed.select_line(column, shift); if !shift { self.table_menu(column, position.x, position.y); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                    if ui.add_row.contains(pt) || ui.add_col.contains(pt) { let column = ui.add_col.contains(pt); let now = self.now_ms();
                        // the caret goes to the table's last row (its last column) first: the new one follows it
                        if let Some((table, r, c)) = self.ed.table_pos() { let rows = self.ed.doc.children(Some(table)).to_vec(); let row = if column { rows.get(r) } else { rows.last() }.copied();
                            let cell = row.and_then(|row| { let cells = self.ed.doc.children(Some(row)); if column { cells.last() } else { cells.get(c).or(cells.last()) }.copied() });
                            let leaf = cell.and_then(|cell| self.ed.doc.children(Some(cell)).first().copied()); if let Some(leaf) = leaf { self.ed.set_selection(Some(mn_edit::Selection::caret(mn_edit::Pos { block: leaf, at: 0 }))); } }
                        self.follow = true; let d = self.ed.table_insert(column, true, now).unwrap_or(Dirty::Nothing); self.apply(d); return }
                } }
                // the handle: a press and release selects the block; a drag moves the blocks to where the line shows
                if state == ElementState::Pressed && self.handle_rect().is_some_and(|r| r.contains(kurbo::Point::new(position.x, position.y))) { self.grab = Some(Grab { from: self.pointer, dragging: false, drop: None, to_note: None }); self.set_cursor(); return }
                if state == ElementState::Released { if let Some(gr) = self.grab.take() {
                    if let Some(sb) = self.sidebar.as_mut() { sb.drop_row = None; }
                    if let (true, Some(to)) = (gr.dragging, gr.to_note) { let copy = self.mods.2; self.blocks_to_note(to, copy); self.set_cursor(); return }
                    let d = if !gr.dragging { if !self.ed.in_blocks() { self.ed.toggle_blocks(); } Dirty::Nothing }
                        else if let Some((d, _)) = gr.drop { let now = self.now_ms(); self.follow = true; self.ed.drop_blocks(&d, now).unwrap_or(Dirty::Nothing) } else { Dirty::Nothing };
                    self.apply(d); self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } return } }
                self.pressed = state == ElementState::Pressed;
                if self.pressed && !self.blank { if let Some(l) = self.layout.as_ref() {
                    let (x, y) = self.note_point(self.pointer);
                    let double = self.last_click.is_some_and(|(t, px, py)| t.elapsed().as_millis() < 400 && (px - x).abs() < 4.0 && (py - y).abs() < 4.0);
                    if double { self.ed.point_word(l, x, y); self.last_click = None } else { self.ed.point(l, x, y, self.mods.0); self.last_click = Some((Instant::now(), x, y)); }
                    #[cfg(target_os = "macos")] text_element::changed(false);
                    if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
            }
            WindowEvent::KeyboardInput { ref event, .. } if self.files.card.is_none() && self.files.canvas.as_ref().is_some_and(|c| !c.editing()) && !self.modal() && !self.sidebar.as_ref().is_some_and(|sb| sb.typing()) && matches!(&event.logical_key, Key::Character(c) if c.as_str() == " ") => { let down = event.state.is_pressed(); let mut tap = false; if let Some(cv) = self.files.canvas.as_mut() { if down && !event.repeat { self.files.space_cam = Some(cv.camera()); } cv.space = down; if !down { tap = self.files.space_cam.take() == Some(cv.camera()); } }
                if tap && !self.pressed { if let Some(id) = self.video_current() { self.video_toggle(id); } } self.set_cursor(); }
            WindowEvent::KeyboardInput { ref event, .. } if self.video_key(event) => {}
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                // macOS: a key the system's text input kept for itself (a held letter offering its accents, a key steering
                // that popup) or turned into a replacement is not the app's to act on; Cmd shortcuts always are
                // The rule is kept narrow so that ordinary typing can never be lost to it: only a REPEATED key the text input
                // did not pass on starts it, and it ends with the first key that is passed on.
                #[cfg(target_os = "macos")] { use text_element::KeyFate; let fate = text_element::key_fate();
                    text_element::key_log(format!("key {:?} text {:?} repeat {} -> {fate:?}", event.logical_key, event.text, event.repeat));
                    if fate == KeyFate::Replaced { self.accent_popup = false; return }
                    if !self.mods.1 && fate == KeyFate::Swallowed && (event.repeat || self.accent_popup) { self.accent_popup = true; return }
                    self.accent_popup = false; }
                let text = event.text.as_ref().map(|t| t.to_string());
                // typing into the note hides the pointer until it moves again
                if text.is_some() && !self.mods.1 && !self.blank && !self.cursor_hidden && self.cursor == winit::cursor::CursorIcon::Text { if let Some(g) = &self.gpu { g.window.set_cursor_visible(false); self.cursor_hidden = true; } }
                self.key(&event.logical_key, text.as_deref()); }
            // composed text is never a video block's (its text input is off; one that still arrives is dropped)
            WindowEvent::Ime(e) if self.files.canvas.is_some() && self.files.card.is_none() && !self.sidebar.as_ref().is_some_and(|sb| sb.typing()) => { let ev = match &e { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t.clone(), *c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t.clone())), _ => None };
                if let (Some(ev), Some(cv), Some(g)) = (ev, self.files.canvas.as_mut(), self.gpu.as_mut()) { cv.ime(&ev, &mut g.fonts); g.window.request_redraw(); } }
            WindowEvent::Ime(e) if self.files.find.as_ref().is_some_and(|f| f.focused()) && self.files.link.is_none() && self.files.picker.is_none() => { let ev = match &e { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t.clone(), *c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t.clone())), _ => None };
                let out = match (ev, self.files.find.as_mut(), self.gpu.as_mut()) { (Some(ev), Some(p), Some(g)) => p.ime(&ev, &mut g.fonts), _ => findbar::Out::None }; self.find_out(out); }
            WindowEvent::Ime(e) if self.files.link.is_some() => { let ev = match &e { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t.clone(), *c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t.clone())), _ => None };
                if let (Some(ev), Some(p), Some(g)) = (ev, self.files.link.as_mut(), self.gpu.as_mut()) { p.ime(&ev, &mut g.fonts); g.window.request_redraw(); } }
            WindowEvent::Ime(e) if self.files.picker.is_some() => { let ev = match &e { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t.clone(), *c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t.clone())), _ => None };
                if let (Some(ev), Some(p), Some(g)) = (ev, self.files.picker.as_mut(), self.gpu.as_mut()) { p.ime(&ev, &mut g.fonts); g.window.request_redraw(); } }
            WindowEvent::Ime(e) if self.files.note_field.is_some() => { let ev = match &e { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t.clone(), *c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t.clone())), _ => None };
                if let (Some(ev), Some((_, _, f)), Some(g)) = (ev, self.files.note_field.as_mut(), self.gpu.as_mut()) { f.ime(&ev, &mut g.fonts); g.window.request_redraw(); } }
            WindowEvent::Ime(_) if self.video_current().is_some() => {}
            // a choice or a check cell takes no typing
            WindowEvent::Ime(_) if !self.ed.in_blocks() && self.ed.typed_cell().is_some_and(|t| !matches!(t.kind, ColumnKind::Timecode { .. })) => {}
            WindowEvent::Ime(e) => {
                if let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) { if sb.typing() { let ev = match &e { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t.clone(), *c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t.clone())), _ => None }; if let Some(ev) = ev { sb.ime(&ev, &mut g.fonts); g.window.request_redraw(); } return } }
                let now = self.now_ms(); let d = match e {
                    winit::event::Ime::Preedit(text, cursor) => self.ed.set_preedit(&text, cursor, now).unwrap_or(Dirty::Nothing),
                    winit::event::Ime::Commit(text) => self.ed.commit_preedit(&text, now).unwrap_or(Dirty::Nothing),
                    _ => return };
                if self.ed.preedit().is_some() { // shown, not recorded: the layout follows, the file and the desktop's text do not hear of it yet
                    match d { Dirty::Structure => self.layout = None, Dirty::Blocks(ids) => if let (Some(l), Some(g)) = (self.layout.as_mut(), self.gpu.as_mut()) { for id in ids { l.update_block(&self.ed.doc, &mut g.fonts, id); } }, Dirty::Nothing => {} }
                    self.flat = None; self.follow = true; if let Some(g) = &self.gpu { g.window.request_redraw(); }
                } else { self.apply(d); if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
            WindowEvent::RedrawRequested => { let first = !self.drawn; if first { startup_log("first frame: begin"); } self.redraw(); if first { self.drawn = true; startup_log("first frame: presented"); }
                if self.warm > 0 { self.warm -= 1; if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
            _ => {}
        }
    }
}

/// Opens a collection through a staged session (rows 272, 288): the store, the note to show, and whether work from an
/// earlier session was found and taken up. `create`: nothing is at the path yet and a new collection starts there.
fn open_store(path: &std::path::Path, create: bool, note_title: Option<String>) -> Result<(Store, Doc, bool), String> {
    use mn_store::{staging, Collection, DocKind};
    let root = sessions_root();
    let left = staging::leftovers(&root).into_iter().find(|l| l.original == path);
    let session = match &left {
        Some(l) => { println!("recovered work: unsaved changes from an earlier session of {} are taken up again", path.display()); staging::Session::resume(l).map_err(|e| e.to_string())? }
        // a path with nothing at it is NOT quietly a new collection (on a share it may be a missing mount)
        None => if create { staging::Session::create(&root, path) } else { staging::Session::open(&root, path) }.map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "Nothing is at this place. Is its drive or share connected? (A new collection is made with New Collection\u{2026}, or --new.)".to_string() } else { e.to_string() })?,
    };
    let mut col = Collection::open(&session.work()).map_err(|e| e.to_string())?;
    fn notes(c: &Collection, parent: Option<BlockId>, out: &mut Vec<mn_store::Entry>) { for e in c.list(parent).unwrap_or_default() { if e.kind == DocKind::Note { out.push(e.clone()); } notes(c, Some(e.id), out); } }
    let mut all = vec![]; notes(&col, None, &mut all);
    let entry = match note_title { Some(t) => all.iter().find(|e| e.title == t).cloned(), None => all.first().cloned() };
    let (note, title) = match entry { Some(e) => (e.id, e.title), None => { let id = col.create(None, DocKind::Note, "").map_err(|e| e.to_string())?; let mut d = Doc::new(); d.insert(None, 0, BlockId::new(), Kind::Paragraph, "").unwrap(); col.save_note(id, &d).map_err(|e| e.to_string())?; (id, String::new()) } };
    let doc = col.load_note(note).map_err(|e| e.to_string())?;
    println!("opened {} — note {:?}: {} blocks; first text: {:?}", path.display(), title, doc.len(), doc.walk().iter().find(|b| b.kind.is_text()).map(|b| b.text.chars().take(60).collect::<String>()).unwrap_or_default());
    Ok((Store { session: Some(session), col, note, title, edited: None, unpublished: left.is_some() || create, edited_since_stage: false }, doc, left.is_some()))
}

/// The collections opened lately, newest first (a plain list beside the sessions; ten at most).
/// The player's settings that are the app's (row 316): loop, speed, volume, muted - a small file beside the list of
/// recent collections. Absent or unreadable: QCView's defaults (loop on, 1x, full volume, not muted).
static SPIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static SPUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SPIN_T0: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
/// The pen's colour and width, as last chosen (a canvas's ink and a video's drawings share them); kept beside the player's settings.
/// A line in `panic.log` beside the settings: what the application was doing, for looking back after it went down.
fn trail(line: &str) {
    let f = player_file().with_file_name("panic.log"); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
    if let Ok(mut o) = std::fs::OpenOptions::new().create(true).append(true).open(&f) { use std::io::Write; let _ = writeln!(o, "{} {line}", jiff::Zoned::now().strftime("%H:%M:%S%.3f")); }
}
/// The paper a PDF is made for: A4 where the system measures in metres (the Qt app's rule), Letter elsewhere.
#[cfg(target_os = "macos")]
fn metric_paper() -> bool { use objc2::{class, msg_send, runtime::AnyObject}; unsafe { let l: *mut AnyObject = msg_send![class!(NSLocale), currentLocale]; if l.is_null() { return false } let m: bool = msg_send![l, usesMetricSystem]; m } }
#[cfg(not(target_os = "macos"))]
fn metric_paper() -> bool { false }
fn export_file() -> std::path::PathBuf { player_file().with_file_name("export.json") }
fn export_settings() -> export::Opts {
    let v: serde_json::Value = std::fs::read_to_string(export_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    export::Opts { paths: v.get("paths").and_then(|x| x.as_bool()).unwrap_or(true), video_notes: v.get("videoNotes").and_then(|x| x.as_bool()).unwrap_or(true) }
}
fn save_export_settings(o: export::Opts) { let f = export_file(); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); } let _ = std::fs::write(f, serde_json::json!({ "paths": o.paths, "videoNotes": o.video_notes }).to_string()); }
fn pen_file() -> std::path::PathBuf { player_file().with_file_name("pen.json") }
fn pen_settings() -> (Option<[f32; 4]>, Option<f32>) {
    let v: serde_json::Value = std::fs::read_to_string(pen_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    (v.get("color").and_then(|c| c.as_array()).filter(|c| c.len() == 4).map(|c| [0, 1, 2, 3].map(|i| c[i].as_f64().unwrap_or(1.0).clamp(0.0, 1.0) as f32)), v.get("width").and_then(|w| w.as_f64()).filter(|w| (1.0..=64.0).contains(w)).map(|w| w as f32))
}
fn save_pen_settings(color: Option<[f32; 4]>, width: Option<f32>) {
    let f = pen_file(); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
    let mut o = serde_json::Map::new(); if let Some(c) = color { o.insert("color".into(), serde_json::json!(c)); } if let Some(w) = width { o.insert("width".into(), serde_json::json!(w)); }
    let _ = std::fs::write(f, serde_json::Value::Object(o).to_string());
}
/// The pressure of the pen behind the pointer event being handled (0 … 1). None: a mouse, or a pen that tells none.
/// macOS: the window layer passes no pen pressure on, so it is read from the system's own event (a tablet's point rides
/// on the mouse event it stands for). Windows: the window layer has it (`TabletToolData`) - taken up with the Windows leg.
#[cfg(target_os = "macos")]
fn pen_pressure() -> Option<f64> {
    use objc2::{class, msg_send, runtime::AnyObject};
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication]; if app.is_null() { return None }
        let ev: *mut AnyObject = msg_send![app, currentEvent]; if ev.is_null() { return None }
        // left mouse down / up / dragged only: `subtype` is defined for mouse events and raises on others
        let kind: usize = msg_send![ev, type]; if !matches!(kind, 1 | 2 | 6) { return None }
        let sub: i16 = msg_send![ev, subtype]; if sub != 1 { return None }
        let p: f32 = msg_send![ev, pressure]; Some((p as f64).clamp(0.0, 1.0))
    }
}
#[cfg(not(target_os = "macos"))]
fn pen_pressure() -> Option<f64> { None }
fn player_file() -> std::path::PathBuf { sessions_root().parent().map(|p| p.to_path_buf()).unwrap_or_else(std::env::temp_dir).join("player.json") }
fn player_settings() -> (bool, f64, f32, bool) {
    let v: serde_json::Value = std::fs::read_to_string(player_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    (v.get("loop").and_then(|x| x.as_bool()).unwrap_or(true), v.get("speed").and_then(|x| x.as_f64()).filter(|x| *x > 0.0).unwrap_or(1.0), v.get("volume").and_then(|x| x.as_f64()).unwrap_or(1.0).clamp(0.0, 1.0) as f32, v.get("muted").and_then(|x| x.as_bool()).unwrap_or(false))
}
fn save_player_settings(now: (bool, f64, f32, bool)) {
    if player_settings() == now { return }
    let f = player_file(); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
    let _ = std::fs::write(f, serde_json::json!({ "loop": now.0, "speed": now.1, "volume": now.2, "muted": now.3 }).to_string());
}
fn recent_file() -> std::path::PathBuf { sessions_root().parent().map(|p| p.to_path_buf()).unwrap_or_else(std::env::temp_dir).join("recent.txt") }
fn recent_list() -> Vec<String> { std::fs::read_to_string(recent_file()).map(|s| s.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect()).unwrap_or_default() }
fn recent_add(path: &std::path::Path) {
    if std::env::var_os("MN_SESSIONS_DIR").is_some() { return }   // scripted runs leave the user's list alone
    let p = path.to_string_lossy().into_owned(); let mut l = recent_list(); l.retain(|x| *x != p); l.insert(0, p); l.truncate(10);
    let f = recent_file(); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); } let _ = std::fs::write(f, l.join("\n"));
}
fn recent_clear() { let _ = std::fs::remove_file(recent_file()); }

/// The page's metrics with the theme's colours for text and links.
fn style() -> Style { Style { text: paint::text(), link: paint::link(), ..Style::default() } }

/// Where staged sessions live on this machine (row 272: never beside the file).
fn sessions_root() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("MN_SESSIONS_DIR") { return p.into() }
    let base = if cfg!(target_os = "macos") { std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support")) } else if cfg!(windows) { std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from) } else { std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from).or(std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))) };
    base.unwrap_or_else(std::env::temp_dir).join("minNotes").join("sessions")
}

/// MN_STARTUP_LOG=<file>: the times of the launch's steps since the process began, one line each (where a slow start goes).
static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
fn startup_log(what: &str) {
    let Some(path) = std::env::var_os("MN_STARTUP_LOG") else { return };
    use std::io::Write; let ms = STARTED.get().map(|t| t.elapsed().as_secs_f64() * 1000.0).unwrap_or(0.0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) { let _ = writeln!(f, "{ms:8.1} ms  {what}"); }
}
/// Menu commands as the native menu reports them, kept until the loop (woken for them) takes them.
/// Wakes the loop from another thread (a decoded picture).
static PANEL_DONE: std::sync::Mutex<Option<Vec<std::path::PathBuf>>> = std::sync::Mutex::new(None);
static WAKE: std::sync::OnceLock<winit::event_loop::EventLoopProxy> = std::sync::OnceLock::new();
#[cfg(target_os = "macos")] static MENU_EVENTS: std::sync::Mutex<Vec<muda::MenuEvent>> = std::sync::Mutex::new(Vec::new());

fn main() {
    let _ = STARTED.set(Instant::now()); startup_log("main");
    env_logger::init();
    // a panic is written down where it can be found afterwards (an application opened from the Finder has no terminal):
    // `panic.log` beside the settings - the time, the message, where, the calls that led there
    { let before = std::panic::take_hook(); std::panic::set_hook(Box::new(move |info| {
        let f = player_file().with_file_name("panic.log"); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
        if let Ok(mut o) = std::fs::OpenOptions::new().create(true).append(true).open(&f) { use std::io::Write;
            let _ = writeln!(o, "---- {} minNotes {}\n{info}\n{}", jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S"), env!("CARGO_PKG_VERSION"), std::backtrace::Backtrace::force_capture()); }
        before(info); })); }
    // holding a letter offers its accents (the system's popup) instead of repeating it — the app says so itself, so it
    // does not depend on a machine-wide setting
    #[cfg(target_os = "macos")]
    objc2_foundation::NSUserDefaults::standardUserDefaults().setBool_forKey(true, objc2_foundation::ns_string!("ApplePressAndHoldEnabled"));
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    // `--make-sample <file.mnd>`: writes a small synthetic collection (folders, notes, a canvas entry) and exits
    if let Some(path) = arg("--make-sample") {
        use mn_store::{Collection, DocKind};
        let path = std::path::PathBuf::from(path); let _ = std::fs::remove_file(&path);
        let mut c = Collection::open(&path).expect("sample");
        let project = c.create(None, DocKind::Folder, "Closing sequence").unwrap();
        let n = c.create(Some(project), DocKind::Note, "Review notes").unwrap(); c.save_note(n, &sample()).unwrap();
        let para = |t: &str| { let mut d = Doc::new(); d.insert(None, 0, BlockId::new(), Kind::Paragraph, t).unwrap(); d };
        let n = c.create(Some(project), DocKind::Note, "").unwrap(); c.save_note(n, &para("A note without a name is called by its first line")).unwrap();
        let sub = c.create(Some(project), DocKind::Folder, "Deliveries").unwrap();
        let n = c.create(Some(sub), DocKind::Note, "Formats and safe areas").unwrap(); c.save_note(n, &para("16:9, 9:16 and 1:1.")).unwrap();
        c.create(Some(project), DocKind::Canvas, "Storyboard").unwrap();
        // pictures (row 310), made here: one larger than the page, one the file stores on its side, one with clear
        // parts, and the states - a file that is not there, a format not read yet, a volume without a path
        {
            let side = path.parent().unwrap_or(std::path::Path::new(".")).join(media::SIDE); std::fs::create_dir_all(&side).expect("sample");
            let at = |n: &str| side.join(n).to_string_lossy().into_owned();
            let grad = |w: u32, h: u32| -> Vec<u8> { let mut v = Vec::with_capacity((w * h * 3) as usize); for y in 0..h { for x in 0..w { let band = (x * 8 / w) % 2 == (y * 6 / h) % 2; v.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, if band { 200 } else { 60 }]); } } v };
            min_image::write::png8(&at("wide.png"), &grad(2400, 1350), 2400, 1350).expect("sample");
            // 600 x 400 in the file, orientation 6: seen upright it is 400 x 600 with the red edge on top
            let mut rgb = grad(600, 400); for y in 0..400usize { for x in 0..24usize { let i = (y * 600 + x) * 3; rgb[i..i + 3].copy_from_slice(&[230, 40, 40]); } }
            let jpg = min_image::write::jpeg8_bytes(&rgb, 600, 400, 90).expect("sample");
            let mut turned = jpg[..2].to_vec(); turned.extend_from_slice(&[0xff, 0xe1, 0, 34]); turned.extend_from_slice(b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01"); turned.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0]); turned.extend_from_slice(&jpg[2..]);
            std::fs::write(at("turned.jpg"), turned).expect("sample");
            let mut clear = vec![0u8; 320 * 200 * 4]; for y in 0..200i32 { for x in 0..320i32 { let d = (((x - 160) * (x - 160) + (y - 100) * (y - 100)) as f32).sqrt(); if d < 90.0 { let i = ((y * 320 + x) * 4) as usize; clear[i..i + 4].copy_from_slice(&[80, 160, 240, if d < 80.0 { 255 } else { 120 }]); } } }
            min_image::write::png_rgba8(&at("clear.png"), &clear, 320, 200).expect("sample");
            std::fs::write(at("photo.heic"), b"not a picture yet").expect("sample");
            let mut d = Doc::new(); let mut add = |kind: Kind, text: &str| { let i = d.children(None).len(); d.insert(None, i, BlockId::new(), kind, text).unwrap(); };
            let pic = |j: &str| Kind::Media { descriptor: j.to_string() };
            add(Kind::Heading { level: 1 }, "Pictures");
            add(Kind::Paragraph, "Wider than the page: it is shown at the page's width.");
            add(pic(r#"{"h":1350,"name":"wide.png","src":".minnotes/wide.png","w":2400}"#), "");
            add(Kind::Paragraph, "Stored on its side by the camera, shown upright (the red edge is its top); its record has the size as stored, and the box is upright all the same.");
            add(pic(r#"{"h":400,"name":"turned.jpg","src":".minnotes/turned.jpg","w":600}"#), "");
            add(Kind::Paragraph, "With clear parts, at a width of its own:");
            add(pic(r#"{"dw":240,"h":200,"name":"clear.png","src":".minnotes/clear.png","w":320}"#), "");
            add(Kind::Paragraph, "A file that is not there, a format not read yet, and a volume this machine has no path for:");
            add(pic(r#"{"h":300,"name":"gone.png","src":".minnotes/gone.png","w":800}"#), "");
            add(pic(r#"{"h":300,"name":"photo.heic","src":".minnotes/photo.heic","w":800}"#), "");
            add(pic(r#"{"h":300,"name":"plate.exr","src":{"rel":["shots","010","plate.exr"],"vol":"a-volume"},"w":800}"#), "");
            add(Kind::Paragraph, "Text goes on below.");
            let n = c.create(None, DocKind::Note, "Pictures").unwrap(); c.save_note(n, &d).unwrap();
        }
        let n = c.create(None, DocKind::Note, "Scratch").unwrap(); c.save_note(n, &para("")).unwrap();
        let tmp = path.with_extension("publish"); c.publish(&tmp).expect("publish"); drop(c);
        for ext in ["mnd-wal", "mnd-shm"] { let _ = std::fs::remove_file(path.with_extension(ext)); }
        std::fs::rename(&tmp, &path).expect("sample"); println!("wrote {}", path.display()); return
    }
    // a collection: the first argument that is not an option or an option's value
    let options_with_value = ["--v2", "--text", "--seconds", "--shot", "--scroll", "--note", "--make-sample"];
    let file = args.iter().enumerate().skip(1).find(|(i, a)| !a.starts_with("--") && !options_with_value.contains(&args[i - 1].as_str()) && !(args.get(i.wrapping_sub(2)).is_some_and(|x| x == "--v2"))).map(|(_, a)| std::path::PathBuf::from(a));
    let mut store = None; let mut recovered = false;
    let opened = file.map(|path| {
        let (st, doc, rec) = open_store(&path, args.iter().any(|a| a == "--new"), arg("--note")).unwrap_or_else(|e| { eprintln!("{}: {e}", path.display()); std::process::exit(2) });
        recent_add(&path); store = Some(st); recovered = rec; doc
    });
    // with nothing to open the window is blank; `--sample` shows the built-in note
    let blank = opened.is_none() && arg("--v2").is_none() && arg("--text").is_none() && !args.iter().any(|a| a == "--sample");
    let doc = if let Some(d) = opened { d } else { match arg("--v2") {
        Some(path) => {
            // the file is copied into a private session first and only the copy is read (row 272)
            let root = std::env::temp_dir().join("minnotes-step2-staging");
            let session = mn_store::staging::Session::open(&root, std::path::Path::new(&path)).expect("staging");
            let pages = mn_store::old::read_pages(&session.work()).unwrap_or_else(|g| { eprintln!("{g}"); std::process::exit(2) });
            let n = args.iter().position(|a| a == "--v2").and_then(|i| args.get(i + 2)).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, pages.len().max(1));
            let c = mn_doc::legacy::convert_page(&pages[n - 1].blocks);
            println!("page {n} of {}: {} blocks; {} repairs", pages.len(), c.doc.len(), c.notes.len());
            let _ = session.close(); c.doc
        }
        None => match arg("--text") {
            // a plain text file: one paragraph per line (for trying a writing tool on a text of one's own)
            Some(path) => { let mut d = Doc::new(); for (i, line) in std::fs::read_to_string(&path).expect("--text").lines().filter(|l| !l.trim().is_empty()).enumerate() { d.insert(None, i, BlockId::new(), Kind::Paragraph, line).unwrap(); } d }
            None => if blank { Doc::new() } else { sample() },
        },
    } };
    let recovered = recovered && std::env::var_os("MN_NO_DIALOGS").is_none();
    #[cfg(target_os = "macos")] menu::AppMenu::quiet_appkit_extras();   // before the application launches: AppKit reads these then
    let sidebar = store.as_ref().map(|st| { let mut sb = sidebar::Sidebar::new(); sb.set_tree(&st.col); sb });
    let app = App { player: None, hw: None, poster: video::Poster::default(), files: Files::default(), media: media::Media::new(sessions_root().parent().unwrap_or(std::path::Path::new(".")), || { if let Some(p) = WAKE.get() { p.wake_up(); } }), drawn: false, warm: 4, warm_at: None, grab: None, moved: None, notice: None, ctx_notes: vec![], shown_blocks: false, block_clip: None, ctrl: false, cursor: winit::cursor::CursorIcon::Default, cursor_hidden: false, pal: colours::Colours::new(), last_bg: "yellow".to_string(), show_pal: std::env::var("MN_PALETTE").ok(), ctx: min_ui::menu::Menu::new(), ctx_for: None, ctx_blocks: false, later: vec![], foot: footer::Footer::new(), bottom: 0.0, status: None, changed: Default::default(), splash: [min_ui::text_button::TextButton::new("New Collection\u{2026}", min_ui::text_button::Variant::Primary), min_ui::text_button::TextButton::new("Open\u{2026}", min_ui::text_button::Variant::Secondary)], bar: toolbar::Toolbar::new(), bar_wide: std::env::var("MN_TOOLBAR").as_deref() == Ok("window"), top: 0.0, saver: mn_store::share::Worker::new(), saving: None, store, sidebar, others: Default::default(), side_w: 0.0, confirm: None, quit: false, close_after_save: false, recovered, blank, after_close: None, #[cfg(target_os = "macos")] menu: None, ed: Editor::new(doc), mods: (false, false, false), pointer: (0.0, 0.0), pressed: false, last_click: None, clipboard: arboard::Clipboard::new().ok(), accent_popup: false, told_geom: (0.0, 0.0), geom_moved: None, follow: false, flat: None, last_scroll: 0.0, layout: None, gpu: None, scroll: arg("--scroll").and_then(|v| v.parse().ok()).unwrap_or(0.0), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), shot: arg("--shot"), t0: None, frames: 0, draw_ms: vec![] };
    startup_log("arguments read, collection opened (if any)");
    let el = EventLoop::new().unwrap();
    // a menu command (chosen, or its key pressed) must wake the loop: nothing else may be due when it arrives
    let _ = WAKE.set(el.create_proxy());
    #[cfg(target_os = "macos")] { let proxy = el.create_proxy(); muda::MenuEvent::set_event_handler(Some(move |ev: muda::MenuEvent| { if menu::panel_takes(&ev.id) { return } MENU_EVENTS.lock().unwrap().push(ev); proxy.wake_up(); })); }
    el.run_app(app).unwrap();
}
