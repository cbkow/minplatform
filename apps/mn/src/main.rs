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
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

mod menu;
mod paint;
mod sidebar;
mod toolbar;
mod footer;
mod colours;
mod clip;
#[cfg(target_os = "macos")]
mod text_element;

/// The page's side margin and top / bottom padding, logical px (the Qt app: 48 and 0 — `docTopPad` 28 is defined there
/// but unused; 28 is used here so the first line does not touch the title bar).
const MARGIN: f64 = 48.0;
const PAD: f64 = 28.0;

struct Gpu { window: Arc<Box<dyn Window>>, presenter: Presenter, ui: UiLayer, fonts: Fonts, ime_area: Option<Rect> }

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
fn handle_rect_in(ed: &Editor, l: &NoteLayout, s: f64, off: kurbo::Vec2) -> Option<Rect> {
    let id = ed.handle_block()?; let r = l.rect(&ed.doc, id)?;
    let line = if ed.doc.get(id).is_some_and(|b| b.kind.is_container()) { None } else { l.caret(&ed.doc, id, 0, 1.0) }.unwrap_or(Rect::new(r.x0, r.y0, r.x0, r.y0 + 24.0 * s));
    let (cy, sz) = (line.center().y, 20.0 * s);
    // at the page's left edge the handle stands well clear of the text: a writing tool draws its own marks close beside a
    // line (the user 2026-11-11); inside a column there is only the gutter, so it stays close there
    let gap = if r.x0 < 1.0 { 15.0 * s } else { 2.0 * s };
    Some(Rect::new(r.x0 - gap - sz, cy - sz / 2.0, r.x0 - gap, cy + sz / 2.0) + off)
}
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
    after_close: Option<Pending>, #[cfg(target_os = "macos")] menu: Option<menu::AppMenu>, ed: Editor, mods: (bool, bool, bool), pointer: (f64, f64), pressed: bool, last_click: Option<(Instant, f64, f64)>, clipboard: Option<arboard::Clipboard>, accent_popup: bool, /// where the page sat when the desktop was last told, and since when it has been somewhere else
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
        let Some(g) = self.gpu.as_mut() else { return };
        let t = Instant::now();
        let (w, h) = g.presenter.size(); let s = g.window.scale_factor(); let now = Instant::now();
        // the sidebar takes its width off the page (it slides when it folds, so the page follows frame by frame)
        // the band: across the window (the sidebar starts below it) or over the page only (beside the sidebar)
        if self.bar_wide { self.top = self.bar.layout(&mut g.fonts, 0.0, w as f64, 0.0, s); self.side_w = self.sidebar.as_mut().map(|sb| sb.layout(self.top, h as f64, s, now)).unwrap_or(0.0); }
        else { self.side_w = self.sidebar.as_mut().map(|sb| sb.layout(0.0, h as f64, s, now)).unwrap_or(0.0); self.top = self.bar.layout(&mut g.fonts, self.side_w, w as f64, 0.0, s); }
        // the sidebar's caret floats over the page's top left corner: it is hit before the band

        self.bottom = self.foot.layout(&mut g.fonts, self.side_w, w as f64, h as f64, s);
        { let st = self.ed.state(); self.bar.sync(&toolbar::Shown { state: &st, has_note: !self.blank });
            let unsaved = self.store.as_ref().is_some_and(|x| x.unpublished || x.edited.is_some());
            // the readout: a save under way, a save that failed, what is not in the document yet, else the last outcome
            let n = self.changed.len() + self.store.as_ref().is_some_and(|x| x.edited.is_some() && !self.changed.contains(&x.note)) as usize;
            let (text, warn) = if self.store.is_none() { (String::new(), false) } else if self.saving.is_some() { ("Saving\u{2026}".to_string(), false) }
                else if let Some((t, true)) = &self.status { (t.clone(), true) }
                else if let Some((t, _)) = self.notice.as_ref().filter(|(_, until)| now < *until) { (t.clone(), false) }
                else if unsaved { (match n { 0 => "Changes not saved to the document".to_string(), 1 => "1 note changed \u{2014} not saved to the document".to_string(), n => format!("{n} notes changed \u{2014} not saved to the document") }, false) }
                else { (self.status.as_ref().map(|x| x.0.clone()).unwrap_or_else(|| "Saved".to_string()), false) };
            self.foot.sync(&footer::Shown { text: &text, warn, has_note: !self.blank, has_file: self.store.is_some(), sidebar_open: self.sidebar.as_ref().is_some_and(|sb| !sb.rail.collapsed), can_undo: st.can_undo, can_redo: st.can_redo, unsaved, saving: self.saving.is_some() }); }
        let page_w = (w as f64 - self.side_w - 2.0 * MARGIN * s).max(320.0 * s);
        // the page moved or changed width without a scroll or an edit (the sidebar, the window): the desktop is told once it rests
        if (self.side_w, page_w + (self.top + self.bottom * 1e-3) * 1e-3) != self.told_geom { self.told_geom = (self.side_w, page_w + (self.top + self.bottom * 1e-3) * 1e-3); self.geom_moved = Some(now); }
        #[cfg(target_os = "macos")] text_element::set_obscured(self.confirm.is_some() || self.pal.is_open() || self.ctx.is_open() || self.sidebar.as_ref().is_some_and(|sb| sb.renaming()));
        if self.layout.as_ref().is_none_or(|l| (l.width as f64 - page_w).abs() > 0.5 || (l.scale as f64 - s).abs() > 1e-6) { self.layout = Some(NoteLayout::build(&self.ed.doc, &mut g.fonts, &style(), page_w as f32, s as f32)); }
        // MN_DEMO=1 (for `--shot` self-checks; no input is sent): the app types into its own first paragraph and selects
        // from the heading into the first list item
        if self.frames == 0 && std::env::var_os("MN_DEMO").is_some() {
            let top: Vec<BlockId> = self.ed.doc.children(None).to_vec(); use mn_edit::{Pos, Selection};
            if let Some(target) = top.get(1).or(top.first()).copied() {
                self.ed.set_selection(Some(Selection::caret(Pos { block: target, at: 0 }))); let _ = self.ed.insert("Typed by the demo. ", 0); self.layout.as_mut().unwrap().update_block(&self.ed.doc, &mut g.fonts, target);
                if let Some(st) = self.store.as_mut() { st.edited = Some(Instant::now()); }
                if top.len() > 3 { self.ed.set_selection(Some(Selection { anchor: Pos { block: top[0], at: 7 }, focus: Pos { block: top[3], at: 10 } })); } }
        }
        let l = self.layout.as_ref().unwrap();
        // after an edit or a caret move the caret is brought into view
        if std::mem::take(&mut self.follow) { if let Some(c) = self.ed.caret_rect(l, 1.0) { let (top, bottom) = (c.y0 + PAD * s - 24.0 * s, c.y1 + PAD * s + 24.0 * s); let view_h = h as f64 - self.top - self.bottom; if top < self.scroll { self.scroll = top } else if bottom > self.scroll + view_h { self.scroll = bottom - view_h } } }
        self.scroll = self.scroll.clamp(0.0, (l.height as f64 + 2.0 * PAD * s - (h as f64 - self.top - self.bottom)).max(0.0));
        #[cfg(target_os = "macos")] if (self.scroll - self.last_scroll).abs() > 0.01 { text_element::scrolled(); }
        self.last_scroll = self.scroll;
        let view = Rect::new(0.0, 0.0, w as f64, h as f64);
        let off = kurbo::Vec2::new(self.side_w + MARGIN * s, self.top + PAD * s - self.scroll);
        let scene = g.ui.scene_mut(); scene.reset();
        scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::page(), None, &view);
        if self.blank {
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
        let picked = self.ed.block_selection();
        for id in &picked { if let Some(r) = l.rect(&self.ed.doc, *id) { let r = kurbo::Rect::new(-8.0 * s, r.y0 - 2.0 * s, l.width as f64 + 8.0 * s, r.y1 + 2.0 * s) + off; scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::selection(), None, &kurbo::RoundedRect::from_rect(r, 4.0 * s)); } }
        for r in self.ed.selection_rects(l) { if self.blank || !picked.is_empty() { break } scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::selection(), None, &(r + off)); }
        if !self.blank { paint::note(scene, &self.ed.doc, l, &mut g.fonts, off.x, off.y, view); }
        // a composition is underlined where it stands
        if let Some((id, a, b)) = self.ed.preedit() { for r in l.range_rects(&self.ed.doc, id, a, b) { let r = r + off; scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, peniko::Color::WHITE, None, &Rect::new(r.x0, r.y1 - 1.5 * s, r.x1, r.y1)); } }
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
            sb.paint(scene, &mut g.fonts, self.store.as_ref().map(|st| st.note), &changed, now); if sb.rail.animating(now) || sb.tick(now) { g.window.request_redraw(); }
            sb.paint_tooltip(scene, &mut g.fonts, view, now); }
        self.bar.paint_tooltip(scene, &mut g.fonts, view, now); self.foot.paint_tooltip(scene, &mut g.fonts, view, now);
        if self.ctx.is_open() { self.ctx.paint(scene, &mut g.fonts, s); }
        self.pal.paint(scene, &mut g.fonts, paint::page(), now);
        if self.foot.tick(now) { g.window.request_redraw(); }
        if self.bar.tick(now) { g.window.request_redraw(); }
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
        let page_w = (w as f64 - self.side_w - 2.0 * MARGIN * s).max(320.0 * s);
        if self.layout.as_ref().is_none_or(|l| (l.width as f64 - page_w).abs() > 0.5 || (l.scale as f64 - s).abs() > 1e-6) { self.layout = Some(NoteLayout::build(&self.ed.doc, &mut g.fonts, &style(), page_w as f32, s as f32)); }
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
        self.saver.start(move || SaveStep::Put(mn_store::share::put(&staged, &to, expect)));
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
                if let Err(e) = session.published(&to) { eprintln!("save: the document was written, but the session could not record it: {e}"); }
                st.unpublished = st.edited_since_stage || st.edited.is_some();
                match &sv.merged { Some(m) => println!("saved {place} — merged with a save by someone else: {m}"), None => println!("saved {place}") }
                let merged = sv.merged.clone(); let doubled = sv.doubled;
                self.saving = None;
                // the footer says how it went; a dialog only when there is something to look at (both versions of a paragraph kept)
                self.status = Some((if merged.is_some() { "Saved \u{2014} merged with someone else\u{2019}s changes" } else { "Saved" }.to_string(), false));
                if !self.unpublished() { self.changed.clear(); }
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
    fn unpublished(&self) -> bool { self.store.as_ref().is_some_and(|st| st.unpublished || st.edited.is_some()) }
    /// The collection is to be closed — for quitting, for another collection, or just closed. With changes that are not
    /// in the document, the Qt app's question is put first.
    fn request_close(&mut self, what: Pending) {
        use min_ui::text_button::Variant as V;
        if self.confirm.as_ref().is_some_and(|(a, _)| *a == Ask::Close) { return }
        self.after_close = Some(what);
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
        self.autosave();
        if let Some(st) = self.store.as_mut() { if let Some(s) = st.session.take() { if st.unpublished { drop(s) } else { let _ = s.close(); } } }
        self.store = None; self.sidebar = None; self.others.clear(); self.changed.clear(); self.status = None; self.ed = Editor::new(Doc::new()); self.layout = None; self.flat = None; self.scroll = 0.0; self.blank = true; self.side_w = 0.0;
        #[cfg(target_os = "macos")] text_element::changed(true);
    }
    /// Opens (or, with `create`, starts) a collection in this window. A failure is said in a dialog.
    fn open_collection(&mut self, path: &std::path::Path, create: bool) {
        match open_store(path, create, None) {
            Ok((store, doc, recovered)) => {
                let mut sb = sidebar::Sidebar::new(); sb.set_tree(&store.col);
                self.store = Some(store); self.sidebar = Some(sb); self.ed = Editor::new(doc); self.others.clear();
                self.layout = None; self.flat = None; self.scroll = 0.0; self.blank = false; self.recovered = recovered && std::env::var_os("MN_NO_DIALOGS").is_none();
                recent_add(path); self.sync_recent();
                #[cfg(target_os = "macos")] text_element::changed(true);
            }
            Err(e) => { use min_ui::text_button::Variant as V; let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(); self.ask(Ask::Notice, &format!("Couldn\u{2019}t open \u{201c}{name}\u{201d}"), &e, vec![("OK", V::Primary)]); }
        }
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    fn sync_recent(&mut self) { #[cfg(target_os = "macos")] if let Some(m) = self.menu.as_mut() { m.set_recent(&recent_list()); } }
    /// Save As: the collection is published at a place the user picks, which becomes its document.
    fn save_as(&mut self) {
        if self.saving.is_some() || self.store.is_none() { return }
        let Some(path) = rfd::FileDialog::new().add_filter("minNotes collection", &["mnd"]).set_file_name(self.doc_name()).save_file() else { return };
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
        let over_ui = self.confirm.is_some() || self.pal.is_open() || self.ctx.is_open();
        let want = if self.sidebar.as_ref().is_some_and(|sb| sb.rail.is_dragging() || (!over_ui && sb.rail.splitter_hovered())) { C::ColResize }
            else if self.grab.as_ref().is_some_and(|x| x.dragging) { C::Grabbing }
            else if over_ui || self.blank { C::Default }
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
        if self.confirm.is_some() { return }
        use min_ui::menu::MenuItem as I;
        // over the page: the block under the pointer is selected (the selected blocks stay when it is one of them) and
        // the menu is theirs (row 306)
        let over_tree = self.sidebar.as_ref().is_some_and(|sb| sb.rail.hit(x, y));
        if !over_tree {
            if self.blank || self.bar.hit(x, y) || self.foot.hit(x, y) || x < self.side_w || y < self.top { return }
            let (nx, ny) = self.note_point((x, y));
            let Some(l) = self.layout.as_ref() else { return };
            let Some((hit, _)) = l.hit(&self.ed.doc, nx, ny) else { return };
            if !self.ed.in_block_selection(hit) { self.ed.point(l, nx, ny, false); if !self.ed.toggle_blocks() { return } }
            let Some(g) = self.gpu.as_mut() else { return };
            let mac = cfg!(target_os = "macos"); let k = |m: &str, w: &str| if mac { m.to_string() } else { w.to_string() };
            let items = vec![I::with_shortcut("Cut", true, &k("\u{2318}X", "Ctrl+X")), I::with_shortcut("Copy", true, &k("\u{2318}C", "Ctrl+C")), I::with_shortcut("Paste", true, &k("\u{2318}V", "Ctrl+V")), I::Separator,
                I::with_shortcut("Duplicate", true, &k("\u{2318}D", "Ctrl+D")), I::with_shortcut("Delete", true, "\u{232b}"), I::Separator,
                I::with_shortcut("Move Up", true, &k("\u{2303}\u{2318}\u{2191}", "")), I::with_shortcut("Move Down", true, &k("\u{2303}\u{2318}\u{2193}", "")), I::Separator, I::action("Move to Note\u{2026}", true), I::action("Copy to Note\u{2026}", true)];
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
        match tool {
            T::Bold => self.menu_cmd(M::Bold), T::Italic => self.menu_cmd(M::Italic), T::Underline => self.menu_cmd(M::Underline), T::Strike => self.menu_cmd(M::Strike), T::Code => self.menu_cmd(M::Code), T::Highlight => self.open_colours(mn_doc::MarkKind::Highlight, tool), T::Color => self.open_colours(mn_doc::MarkKind::Color, tool), T::ClearFormat => self.menu_cmd(M::ClearFormat),
            T::Paragraph => block(self, B::Paragraph), T::Bullet => block(self, B::Bullet), T::Numbered => block(self, B::Numbered), T::Task => block(self, B::Task), T::Quote => block(self, B::Quote), T::CodeBlock => block(self, B::Code),
            // Heading steps through the first three levels and back to a paragraph
            T::Heading => { let next = match self.ed.state().kind { Some(Kind::Heading { level }) if level < 3 => B::Heading(level + 1), Some(Kind::Heading { .. }) => B::Paragraph, _ => B::Heading(1) }; block(self, next) }
            T::Indent | T::Outdent => { let d = self.ed.indent(tool == T::Outdent, now).ok().flatten().unwrap_or(Dirty::Nothing); self.apply(d); }
            T::Divider => { let d = self.ed.insert_divider(now).unwrap_or(Dirty::Nothing); self.apply(d); }
            _ => {}
        }
    }
    /// A command of the menu bar (row 296).
    fn menu_cmd(&mut self, c: menu::Cmd) {
        use menu::Cmd as M;
        if self.confirm.is_some() && c != M::Quit { return }
        let pick = || rfd::FileDialog::new().add_filter("minNotes collection", &["mnd"]).pick_file();
        match c {
            M::Quit => self.request_close(Pending::Quit),
            M::CloseCollection => if self.store.is_some() { self.request_close(Pending::Close) },
            M::Open => if let Some(p) = pick() { self.request_close(Pending::Open(p)) },
            M::OpenRecent(p) => self.request_close(Pending::Open(p.into())),
            M::ClearRecent => { recent_clear(); self.sync_recent(); }
            M::NewCollection => if let Some(p) = rfd::FileDialog::new().add_filter("minNotes collection", &["mnd"]).set_file_name("Untitled.mnd").save_file() {
                // the system's dialog has asked about replacing: a new collection starts where nothing is, so an old file is moved aside by name, never deleted
                if p.exists() { let aside = p.with_extension(format!("replaced-{}.mnd", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))); let _ = std::fs::rename(&p, aside); }
                self.request_close(Pending::New(p)) },
            M::SaveAs => self.save_as(),
            // the selected row's name (the open note's when nothing is selected); the sidebar is opened for it
            M::Rename => if let (Some(sb), Some(g), Some(st)) = (self.sidebar.as_mut(), self.gpu.as_mut(), self.store.as_ref()) { let id = sb.selected.unwrap_or(st.note); if sb.rail.collapsed { sb.rail.set_collapsed(false, Instant::now()); } sb.start_rename(id, &mut g.fonts); },
            M::Toolbar => { self.bar.hidden = !self.bar.hidden; self.layout = None; }
            M::Footer => { self.foot.hidden = !self.foot.hidden; self.layout = None; }
            M::ToolbarWide => { self.bar_wide = !self.bar_wide; self.layout = None; }
            other => if let Some((ch, shift)) = menu::key_of(&other) { let m = self.mods; self.mods = (shift, true, false); self.key(&Key::Character(ch.to_string().as_str().into()), None); self.mods = m; },
        }
    }
    /// A dialog's button was pressed (`i`: its place in the row).
    fn answer(&mut self, what: Ask, i: usize) {
        use min_ui::text_button::Variant as V;
        match (what, i) {
            (Ask::Delete(id), 1) => self.delete_entry(id),
            // Don't Save: the changes are given up — the session goes, the document stays as it is
            (Ask::Close, 1) => { self.changed.clear(); if let Some(st) = self.store.as_mut() { st.edited = None; st.unpublished = false; } self.proceed(); }
            (Ask::Close, 0) => { self.after_close = None; }
            (Ask::Close, 2) => { self.close_after_save = true; self.save(); }
            (Ask::Recovered, 1) => self.ask(Ask::DiscardRecovered, "Discard these changes?", "The changes from the earlier session will be removed from this machine. This cannot be undone.", vec![("Cancel", V::Secondary), ("Discard", V::Negative)]),
            (Ask::DiscardRecovered, 1) => self.discard_recovered(),
            (Ask::DiscardRecovered, _) => self.offer_recovered(),
            (Ask::SaveFailed, 1) => self.save(),
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
    fn open_note(&mut self, id: BlockId) {
        let Some(st) = self.store.as_mut() else { return }; if st.note == id { return }
        if self.ed.preedit().is_some() { let _ = self.ed.set_preedit("", None, 0); }
        st.edited.get_or_insert_with(Instant::now); self.autosave();
        let Some(st) = self.store.as_mut() else { return };
        let next = match self.others.remove(&id) { Some(e) => e, None => match st.col.load_note(id) { Ok(d) => Editor::new(d), Err(e) => { eprintln!("open: {e}"); return } } };
        let old = std::mem::replace(&mut self.ed, next); self.others.insert(st.note, old);
        st.note = id; self.layout = None; self.flat = None; self.scroll = 0.0; self.follow = false;
        #[cfg(target_os = "macos")] text_element::changed(true);
        self.title(); if let Some(g) = &self.gpu { g.window.request_redraw(); }
    }
    /// Cmd+N / Cmd+Shift+N: a note or a folder beside the selected entry (inside it, when that is a folder).
    fn new_entry(&mut self, kind: mn_store::DocKind) {
        let (Some(st), Some(sb), Some(g)) = (self.store.as_mut(), self.sidebar.as_mut(), self.gpu.as_mut()) else { return };
        let parent = sb.selected.or(Some(st.note)).and_then(|s| sb.row(s)).and_then(|r| if r.kind == mn_store::DocKind::Folder { Some(r.id) } else { r.parent });
        let id = match st.col.create(parent, kind, if kind == mn_store::DocKind::Folder { "New folder" } else { "" }) { Ok(id) => id, Err(e) => { eprintln!("new: {e}"); return } };
        if kind == mn_store::DocKind::Note { let mut d = Doc::new(); d.insert(None, 0, BlockId::new(), Kind::Paragraph, "").unwrap(); let _ = st.col.save_note(id, &d); }
        st.unpublished = true; sb.set_tree(&st.col); sb.selected = Some(id); self.changed.insert(id);
        if kind == mn_store::DocKind::Folder { sb.start_rename(id, &mut g.fonts); g.window.request_redraw(); } else { sb.focused = false; self.open_note(id); self.ed.select_all(); }
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
            sidebar::Act::Open(id) => { if self.sidebar.as_ref().and_then(|sb| sb.row(id)).is_some_and(|r| r.kind == mn_store::DocKind::Note) { self.open_note(id); } if let Some(g) = &self.gpu { g.window.request_redraw(); } }
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
        // the colour palette: while it is open every event is its own
        if self.pal.is_open() { if let Some(g) = self.gpu.as_mut() { let out = self.pal.event(&e, &mut g.fonts, now); self.colour_out(out); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        // the tree's context menu is over everything but a dialog: while it is open the event is its own
        if self.ctx.is_open() { let (chosen, _, _) = self.ctx.event(&e); if let Some(i) = chosen { self.ctx_pick(i); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        // no collection: the page's two buttons
        if self.blank { let mut hit = None; for (i, b) in self.splash.iter_mut().enumerate() { if b.event(&e, now).0 { hit = Some(i); } }
            if let Some(g) = &self.gpu { g.window.request_redraw(); }
            // run from the loop's idle step, as a menu command is: the file dialog is not opened inside a pointer event
            match hit { Some(0) => self.later.push(menu::Cmd::NewCollection), Some(_) => self.later.push(menu::Cmd::Open), None => {} }
            // a press on a button is the button's to the end: the release must come back here, not go to the page
            if hit.is_some() || e.pos().is_some_and(|(x, y)| self.splash.iter().any(|b| b.hit(x, y))) { return true } }
        let (did, on_foot) = self.foot.event(&e, now);
        if !did.is_empty() || on_foot { for d in did { self.menu_cmd(match d { footer::Do::Undo => menu::Cmd::Undo, footer::Do::Redo => menu::Cmd::Redo, footer::Do::Save => menu::Cmd::Save, footer::Do::Sidebar => menu::Cmd::Sidebar, footer::Do::Find => continue }); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        let (tools, on_bar) = self.bar.event(&e, now);
        if !tools.is_empty() || on_bar { for tool in tools { self.tool(tool); } if let Some(g) = &self.gpu { g.window.request_redraw(); } return true }
        let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) else { return false };
        // the rail's floating tab (the caret that reopens a folded sidebar, as in QCView) shows while the pointer is in the window
        let (a, taken) = sb.event(&e, &mut g.fonts, now);
        if !taken && matches!(e, min_ui::Event::Down { .. }) { sb.focused = false; }
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
        handle_rect_in(&self.ed, l, s, kurbo::Vec2::new(self.side_w + MARGIN * s, self.top + PAD * s - self.scroll))
    }
    fn note_point(&self, p: (f64, f64)) -> (f64, f64) { let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); (p.0 - self.side_w - MARGIN * s, p.1 - self.top - PAD * s + self.scroll) }
    fn key(&mut self, key: &Key, text: Option<&str>) {
        let (shift, cmd, alt) = self.mods; let now = self.now_ms();
        if self.grab.is_some() { if matches!(key, Key::Named(NamedKey::Escape)) { self.grab = None; if let Some(sb) = self.sidebar.as_mut() { sb.drop_row = None; } self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } } return }
        if self.ctx.is_open() { if matches!(key, Key::Named(NamedKey::Escape)) { self.ctx.close(); if let Some(g) = &self.gpu { g.window.request_redraw(); } } return }
        // where there is no native menu to take them, the file commands' keys are run here
        if cmd && cfg!(not(target_os = "macos")) { if let Key::Character(c) = key { let m = match (c.to_lowercase().as_str(), shift, alt) { ("o", false, false) => Some(menu::Cmd::Open), ("w", false, false) => Some(menu::Cmd::CloseCollection), ("q", false, false) => Some(menu::Cmd::Quit), ("s", true, false) => Some(menu::Cmd::SaveAs), ("n", false, true) => Some(menu::Cmd::NewCollection), _ => None }; if let Some(m) = m { self.menu_cmd(m); return } } }
        if self.blank { return }
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
            Key::Named(NamedKey::ArrowUp) => { self.ed.move_caret(l, mv(Move::Up, Move::Up, Move::DocStart), shift); Dirty::Nothing }
            Key::Named(NamedKey::ArrowDown) => { self.ed.move_caret(l, mv(Move::Down, Move::Down, Move::DocEnd), shift); Dirty::Nothing }
            Key::Named(NamedKey::Home) => { self.ed.move_caret(l, if cmd { Move::DocStart } else { Move::LineStart }, shift); Dirty::Nothing }
            Key::Named(NamedKey::End) => { self.ed.move_caret(l, if cmd { Move::DocEnd } else { Move::LineEnd }, shift); Dirty::Nothing }
            Key::Named(NamedKey::Enter) => self.ed.enter(now).unwrap_or(Dirty::Nothing),
            Key::Named(NamedKey::Backspace) => self.ed.delete(false, now).unwrap_or(Dirty::Nothing),
            Key::Named(NamedKey::Delete) => self.ed.delete(true, now).unwrap_or(Dirty::Nothing),
            Key::Named(NamedKey::Tab) => match self.ed.indent(shift, now).unwrap_or(None) { Some(d) => d, None => return },
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
                "v" => {
                    // blocks that minNotes copied come back as blocks (they ride in the clipboard's HTML; `block_clip` is the
                    // fallback where the HTML cannot be read back); anything else is pasted as plain text
                    let html = self.clipboard.as_mut().and_then(|cb| cb.get().html().ok());
                    let text = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok());
                    let blocks = html.as_deref().and_then(clip::from_html).or_else(|| match (&text, &self.block_clip) { (Some(t), Some((bt, c))) if t == bt => Some(c.clone()), _ => None });
                    match (blocks, text) {
                        (Some(clip), _) => { self.follow = true; match self.ed.paste_blocks(&clip, now) { Ok((d, refused)) => { if refused > 0 { eprintln!("paste: {refused} block(s) cannot go there and were left out"); } d } Err(_) => Dirty::Nothing } }
                        (None, Some(t)) => self.ed.insert(&t, now).unwrap_or(Dirty::Nothing), _ => Dirty::Nothing } }
                _ => return,
            },
            // typed text — one key event may carry several characters (a writing tool's correction, the spike S9)
            _ => match text.filter(|t| !cmd && !t.chars().any(|c| c.is_control())) { Some(t) => self.ed.insert(t, now).unwrap_or(Dirty::Nothing), None => return },
        };
        self.apply(dirty);
    }

    fn finish(&mut self) {
        self.autosave();
        if std::env::var_os("MN_SAVE_AT_EXIT").is_some() { self.save(); }   // for scripted checks
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
        ((u.x0 + self.side_w + MARGIN * s) / s, (u.y0 + self.top + PAD * s - self.scroll) / s, u.width() / s, u.height() / s)
    }
    fn ax_note_frame(&mut self) -> (f64, f64, f64, f64) { if !self.ensure() { return (0.0, 0.0, 0.0, 0.0) } let l = self.layout.as_ref().unwrap(); let s = l.scale as f64; (self.side_w / s + MARGIN, self.top / s + PAD - self.scroll / s, l.width as f64 / s, l.height as f64 / s) }
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
        #[cfg(target_os = "macos")] { let mut m = menu::AppMenu::new(); m.menu.init_for_nsapp(); m.set_recent(&recent_list()); self.menu = Some(m); }
        startup_log("text element, input method, menu ready");
        self.gpu = Some(Gpu { window, presenter, ui, fonts: Fonts::new(), ime_area: None }); self.t0 = Some(Instant::now()); self.title();
        startup_log("fonts loaded");
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if let Some(t0) = self.t0.filter(|t| t.elapsed().as_millis() < 1500) { let now = Instant::now(); let at = *self.warm_at.get_or_insert(t0 + std::time::Duration::from_millis(250));
            if now >= at { self.warm_at = Some(now + std::time::Duration::from_millis(250)); if let Some(g) = &self.gpu { g.window.request_redraw(); } } else { el.set_control_flow(ControlFlow::WaitUntil(at)); } }
        if let Some((_, until)) = &self.notice { if Instant::now() >= *until { self.notice = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } } else { el.set_control_flow(ControlFlow::WaitUntil(*until)); } }
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
        if self.show_pal.is_some() && self.bar.rect_of(toolbar::Tool::Color).width() > 1.0 { let what = self.show_pal.take().unwrap();
            if what == "background" { self.open_colours(mn_doc::MarkKind::Highlight, toolbar::Tool::Highlight) } else { self.open_colours(mn_doc::MarkKind::Color, toolbar::Tool::Color) }
            if what == "picker" { if let Some(g) = self.gpu.as_mut() { self.pal.show_picker(&mut g.fonts); } }
            if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        for c in std::mem::take(&mut self.later) { self.menu_cmd(c); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        if let Some(at) = [self.bar.next_change(Instant::now()), self.foot.next_change(Instant::now()), self.sidebar.as_ref().and_then(|sb| sb.next_change(Instant::now()))].into_iter().flatten().min() { if Instant::now() >= at { if let Some(g) = &self.gpu { g.window.request_redraw(); } } else { el.set_control_flow(ControlFlow::WaitUntil(at)); } }
        #[cfg(target_os = "macos")] for ev in std::mem::take(&mut *MENU_EVENTS.lock().unwrap()) { let c = self.menu.as_ref().and_then(|m| m.cmd(&ev.id)); text_element::key_log(format!("menu {c:?}")); if let Some(c) = c { self.menu_cmd(c); }
            // a command changes what is shown, and no window event follows it to ask for the frame
            if let Some(g) = &self.gpu { g.window.request_redraw(); } }
        if let Some(t) = self.geom_moved { let due = t + std::time::Duration::from_millis(150); if Instant::now() >= due { self.geom_moved = None; #[cfg(target_os = "macos")] text_element::relaid(); } else { el.set_control_flow(ControlFlow::WaitUntil(due)); } }
        // a dialog's answer, or a save that was to be followed by closing, ended the session
        if self.quit { #[cfg(target_os = "macos")] text_element::uninstall(); self.finish(); el.exit(); return }
        // work found from an earlier session is offered once the window is up
        if self.recovered && self.gpu.is_some() && self.confirm.is_none() && self.frames > 0 { self.recovered = false; self.offer_recovered(); }
        // a save's steps come back from the helper thread
        if let Some(step) = self.saver.poll() { self.save_step(step); }
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
                if let Some(sb) = self.sidebar.as_mut() { if sb.rail.hit(self.pointer.0, self.pointer.1) { let dy = match delta { MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * s, MouseScrollDelta::PixelDelta(p) => p.y, _ => 0.0 }; sb.rail.wheel(-dy, Instant::now()); g.window.request_redraw(); return } }
                self.scroll -= match delta { MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * s, MouseScrollDelta::PixelDelta(p) => p.y, _ => 0.0 };
                g.window.request_redraw();
            },
            WindowEvent::PointerLeft { .. } => { self.chrome_pointer(min_ui::Event::Leave); self.set_cursor(); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.ctrl = st.control_key(); self.mods = (st.shift_key(), if cfg!(target_os = "macos") { st.meta_key() } else { st.control_key() }, st.alt_key()); }
            WindowEvent::PointerMoved { position, .. } => { self.pointer = (position.x, position.y);
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
                if self.pressed && !self.blank { if let Some(l) = self.layout.as_ref() { let (x, y) = self.note_point(self.pointer); self.ed.point(l, x, y, true); self.follow = true; if let Some(g) = &self.gpu { g.window.request_redraw(); } } } }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Right), .. } => {
                // an open menu takes the press (outside it: it closes); otherwise a press over the tree opens one
                let e = if state == ElementState::Pressed { min_ui::Event::Down { x: position.x, y: position.y, button: min_ui::Button::Secondary } } else { min_ui::Event::Up { x: position.x, y: position.y, button: min_ui::Button::Secondary } };
                if self.ctx.is_open() || self.pal.is_open() { self.chrome_pointer(e); } else if state == ElementState::Pressed { self.context(position.x, position.y); }
            }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Left), .. } => {
                self.pointer = (position.x, position.y);
                let e = if state == ElementState::Pressed { min_ui::Event::Down { x: position.x, y: position.y, button: min_ui::Button::Primary } } else { min_ui::Event::Up { x: position.x, y: position.y, button: min_ui::Button::Primary } };
                if !(self.pressed && state == ElementState::Released) && self.chrome_pointer(e) { self.set_cursor(); return }
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
            WindowEvent::Ime(e) => {
                if let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) { if sb.renaming() { let ev = match &e { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t.clone(), *c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t.clone())), _ => None }; if let Some(ev) = ev { sb.ime(&ev, &mut g.fonts); g.window.request_redraw(); } return } }
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
#[cfg(target_os = "macos")] static MENU_EVENTS: std::sync::Mutex<Vec<muda::MenuEvent>> = std::sync::Mutex::new(Vec::new());

fn main() {
    let _ = STARTED.set(Instant::now()); startup_log("main");
    env_logger::init();
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
    let app = App { drawn: false, warm: 4, warm_at: None, grab: None, moved: None, notice: None, ctx_notes: vec![], shown_blocks: false, block_clip: None, ctrl: false, cursor: winit::cursor::CursorIcon::Default, cursor_hidden: false, pal: colours::Colours::new(), last_bg: "yellow".to_string(), show_pal: std::env::var("MN_PALETTE").ok(), ctx: min_ui::menu::Menu::new(), ctx_for: None, ctx_blocks: false, later: vec![], foot: footer::Footer::new(), bottom: 0.0, status: None, changed: Default::default(), splash: [min_ui::text_button::TextButton::new("New Collection\u{2026}", min_ui::text_button::Variant::Primary), min_ui::text_button::TextButton::new("Open\u{2026}", min_ui::text_button::Variant::Secondary)], bar: toolbar::Toolbar::new(), bar_wide: std::env::var("MN_TOOLBAR").as_deref() == Ok("window"), top: 0.0, saver: mn_store::share::Worker::new(), saving: None, store, sidebar, others: Default::default(), side_w: 0.0, confirm: None, quit: false, close_after_save: false, recovered, blank, after_close: None, #[cfg(target_os = "macos")] menu: None, ed: Editor::new(doc), mods: (false, false, false), pointer: (0.0, 0.0), pressed: false, last_click: None, clipboard: arboard::Clipboard::new().ok(), accent_popup: false, told_geom: (0.0, 0.0), geom_moved: None, follow: false, flat: None, last_scroll: 0.0, layout: None, gpu: None, scroll: arg("--scroll").and_then(|v| v.parse().ok()).unwrap_or(0.0), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), shot: arg("--shot"), t0: None, frames: 0, draw_ms: vec![] };
    startup_log("arguments read, collection opened (if any)");
    let el = EventLoop::new().unwrap();
    // a menu command (chosen, or its key pressed) must wake the loop: nothing else may be due when it arrives
    #[cfg(target_os = "macos")] { let proxy = el.create_proxy(); muda::MenuEvent::set_event_handler(Some(move |ev: muda::MenuEvent| { MENU_EVENTS.lock().unwrap().push(ev); proxy.wake_up(); })); }
    el.run_app(app).unwrap();
}
