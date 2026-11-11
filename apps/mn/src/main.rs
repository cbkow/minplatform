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

mod paint;
mod sidebar;
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
/// A save under way: which try, what the document is expected to be, what a merge brought.
struct Saving { attempt: u32, expect: Option<mn_store::share::Fingerprint>, merged: Option<String>, /// blocks both sides changed, kept twice
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

struct App { saver: mn_store::share::Worker<SaveStep>, saving: Option<Saving>, store: Option<Store>, sidebar: Option<sidebar::Sidebar>, others: std::collections::HashMap<BlockId, Editor>, side_w: f64, confirm: Option<(Ask, min_ui::dialog::Dialog)>, quit: bool, close_after_save: bool, recovered: bool, ed: Editor, mods: (bool, bool, bool), pointer: (f64, f64), pressed: bool, last_click: Option<(Instant, f64, f64)>, clipboard: Option<arboard::Clipboard>, accent_popup: bool, follow: bool, flat: Option<mn_doc::FlatText>, last_scroll: f64, layout: Option<NoteLayout>, gpu: Option<Gpu>, scroll: f64, seconds: f32, shot: Option<String>, t0: Option<Instant>, frames: u32, draw_ms: Vec<f32> }

fn sample() -> Doc {
    let mut d = Doc::new();
    fn add(d: &mut Doc, parent: Option<BlockId>, kind: Kind, text: &str) -> BlockId { let id = BlockId::new(); let at = d.children(parent).len(); d.insert(parent, at, id, kind, text).unwrap(); id }
    fn mark(d: &mut Doc, id: BlockId, marks: &[(&str, Mark)]) { let text = d.get(id).unwrap().text.clone(); let mut s = Spans::new(); for (word, m) in marks { if let Some(at) = text.find(word) { s.put(at, at + word.len(), m.clone()); } } d.set_spans(id, s).unwrap(); }
    add(&mut d, None, Kind::Heading { level: 1 }, "Review notes — second pass");
    let p = add(&mut d, None, Kind::Paragraph, "The grade on the wide shot holds up well against the reference. Bold, italic, inline code, a link, struck text, underlined text, a colour and a highlight all sit in this paragraph so that each can be seen once in running text that is long enough to wrap.");
    mark(&mut d, p, &[("Bold", Mark::Bold), ("italic", Mark::Italic), ("inline code", Mark::Code), ("a link", Mark::Link("https://example.com".into())), ("struck text", Mark::Strike), ("underlined text", Mark::Underline), ("a colour", Mark::Color("#F2994A".into())), ("a highlight", Mark::Highlight("#5B4B00".into()))]);
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
        self.side_w = self.sidebar.as_mut().map(|sb| sb.layout(h as f64, s, now)).unwrap_or(0.0);
        let page_w = (w as f64 - self.side_w - 2.0 * MARGIN * s).max(320.0 * s);
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
        if std::mem::take(&mut self.follow) { if let Some(c) = self.ed.caret_rect(l, 1.0) { let (top, bottom) = (c.y0 + PAD * s - 24.0 * s, c.y1 + PAD * s + 24.0 * s); if top < self.scroll { self.scroll = top } else if bottom > self.scroll + h as f64 { self.scroll = bottom - h as f64 } } }
        self.scroll = self.scroll.clamp(0.0, (l.height as f64 + 2.0 * PAD * s - h as f64).max(0.0));
        #[cfg(target_os = "macos")] if (self.scroll - self.last_scroll).abs() > 0.01 { text_element::scrolled(); }
        self.last_scroll = self.scroll;
        let view = Rect::new(0.0, 0.0, w as f64, h as f64);
        let off = kurbo::Vec2::new(self.side_w + MARGIN * s, PAD * s - self.scroll);
        let scene = g.ui.scene_mut(); scene.reset();
        scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::page(), None, &view);
        for r in self.ed.selection_rects(l) { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::selection(), None, &(r + off)); }
        paint::note(scene, &self.ed.doc, l, &mut g.fonts, off.x, off.y, view);
        // a composition is underlined where it stands
        if let Some((id, a, b)) = self.ed.preedit() { for r in l.range_rects(&self.ed.doc, id, a, b) { let r = r + off; scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, peniko::Color::WHITE, None, &Rect::new(r.x0, r.y1 - 1.5 * s, r.x1, r.y1)); } }
        if self.ed.selection().is_some_and(|x| x.is_caret()) { if let Some(c) = self.ed.caret_rect(l, (1.5 * s) as f32) { let c = c + off; scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::caret(), None, &c);
            // the input method's candidate window follows the caret
            if g.ime_area != Some(c) { g.ime_area = Some(c); ime(g.window.as_ref().as_ref(), Some(c), false); } } }
        // over the page: the sidebar, then a question being asked
        if let Some(sb) = self.sidebar.as_mut() { sb.paint(scene, &mut g.fonts, self.store.as_ref().map(|st| st.note), now); if sb.rail.animating(now) { g.window.request_redraw(); } }
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
        if let Some(st) = self.store.as_mut() { if st.edited.take().is_some() { match st.col.save_note(st.note, &self.ed.doc) { Ok(()) => { st.unpublished = true; st.edited_since_stage = true; } Err(e) => eprintln!("autosave: {e}") }
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
        self.saving = Some(Saving { attempt: 0, expect, merged: None, doubled: 0, started: Instant::now() });
        self.put_staged();
    }
    /// Stages the collection as it stands and sends it to the document's place, expecting `saving.expect` there.
    fn put_staged(&mut self) {
        let (Some(st), Some(sv)) = (self.store.as_mut(), self.saving.as_ref()) else { return };
        let Some(session) = st.session.as_ref() else { self.saving = None; return };
        let col = &st.col;
        let staged = match session.stage_publish(|tmp| col.publish(tmp).map_err(std::io::Error::other)) { Ok(p) => p, Err(e) => { eprintln!("save: {e}"); self.saving = None; return } };
        st.edited_since_stage = false;
        let (to, expect) = (session.original().to_path_buf(), sv.expect);
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
                let to = session.original().to_path_buf();
                if let Err(e) = session.published(&to) { eprintln!("save: the document was written, but the session could not record it: {e}"); }
                st.unpublished = st.edited_since_stage || st.edited.is_some();
                match &sv.merged { Some(m) => println!("saved {place} — merged with a save by someone else: {m}"), None => println!("saved {place}") }
                let merged = sv.merged.clone(); let doubled = sv.doubled;
                self.saving = None;
                if std::mem::take(&mut self.close_after_save) && !self.unpublished() && doubled == 0 { self.quit = true; }
                else if merged.is_some() { use min_ui::text_button::Variant as V;
                    let body = if doubled > 0 { format!("Someone else saved this document while you were working. Their changes have been merged with yours. {doubled} paragraph(s) were changed by both of you: both versions are kept, the second marked, for you to choose between.") } else { "Someone else saved this document while you were working. Their changes have been merged with yours.".to_string() };
                    self.ask(Ask::Notice, "The document was merged", &body, vec![("OK", V::Primary)]); }
            }
            // not the document we know: someone saved (or it is gone) — look at what is there
            SaveStep::Put(Err(PutError::Raced)) if sv.attempt < 3 => {
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
    fn doc_name(&self) -> String { self.store.as_ref().and_then(|st| st.session.as_ref()).and_then(|s| s.original().file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "this document".into()) }
    fn unpublished(&self) -> bool { self.store.as_ref().is_some_and(|st| st.unpublished || st.edited.is_some()) }
    /// The window is asked to close. With changes that are not in the document, the Qt app's question is put first.
    fn request_close(&mut self) {
        use min_ui::text_button::Variant as V;
        if self.unpublished() && self.confirm.as_ref().is_none_or(|(a, _)| *a != Ask::Close) {
            let name = self.doc_name();
            self.ask(Ask::Close, &format!("Save changes to \u{201c}{name}\u{201d}?"), "Your changes will be lost if you don\u{2019}t save them.", vec![("Cancel", V::Secondary), ("Don\u{2019}t Save", V::Secondary), ("Save", V::Primary)]);
        } else if !self.unpublished() { self.quit = true; }
    }
    /// A dialog's button was pressed (`i`: its place in the row).
    fn answer(&mut self, what: Ask, i: usize) {
        use min_ui::text_button::Variant as V;
        match (what, i) {
            (Ask::Delete(id), 1) => self.delete_entry(id),
            // Don't Save: the changes are given up — the session goes, the document stays as it is
            (Ask::Close, 1) => { if let Some(st) = self.store.as_mut() { st.edited = None; st.unpublished = false; } self.quit = true; }
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
        self.close_after_save = false;
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
        st.unpublished = true; sb.set_tree(&st.col); sb.selected = Some(id);
        if kind == mn_store::DocKind::Folder { sb.start_rename(id, &mut g.fonts); g.window.request_redraw(); } else { sb.focused = false; self.open_note(id); self.ed.select_all(); }
    }
    fn rename_entry(&mut self, id: BlockId, name: &str) {
        let (Some(st), Some(sb)) = (self.store.as_mut(), self.sidebar.as_mut()) else { return };
        if st.col.rename(id, name).is_ok() { st.unpublished = true; sb.set_tree(&st.col); }
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
        st.unpublished = true; sb.set_tree(&st.col); sb.selected = None; self.others.retain(|k, _| sb.row(*k).is_some());
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
        let (Some(sb), Some(g)) = (self.sidebar.as_mut(), self.gpu.as_mut()) else { return false };
        let (a, taken) = sb.event(&e, &mut g.fonts, now);
        if !taken && matches!(e, min_ui::Event::Down { .. }) { sb.focused = false; }
        self.act(a); taken
    }
    fn title(&self) { if let (Some(g), Some(st)) = (&self.gpu, &self.store) { let name = self.sidebar.as_ref().and_then(|sb| sb.row(st.note)).map(|r| r.title.clone()).unwrap_or_else(|| st.title.clone()); g.window.set_title(&format!("{}{}{} — minNotes", name, if self.saving.is_some() { " — saving…" } else { "" }, if st.unpublished || st.edited.is_some() { " •" } else { "" })); } }
    fn now_ms(&self) -> u64 { self.t0.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0) }
    /// A window point in the note's coordinates.
    fn note_point(&self, p: (f64, f64)) -> (f64, f64) { let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); (p.0 - self.side_w - MARGIN * s, p.1 - PAD * s + self.scroll) }
    fn key(&mut self, key: &Key, text: Option<&str>) {
        let (shift, cmd, alt) = self.mods; let now = self.now_ms();
        // what lies over the page takes keys first: a question, a name being edited, then the tree when it has the focus
        let ui_key = || { use min_ui::{Key as K, Named as N};
            let k = match key { Key::Named(NamedKey::Escape) => K::Named(N::Escape), Key::Named(NamedKey::Enter) => K::Named(N::Enter), Key::Named(NamedKey::Tab) => K::Named(N::Tab), Key::Named(NamedKey::Backspace) => K::Named(N::Backspace), Key::Named(NamedKey::Delete) => K::Named(N::Delete),
                Key::Named(NamedKey::ArrowLeft) => K::Named(N::Left), Key::Named(NamedKey::ArrowRight) => K::Named(N::Right), Key::Named(NamedKey::ArrowUp) => K::Named(N::Up), Key::Named(NamedKey::ArrowDown) => K::Named(N::Down), Key::Named(NamedKey::Home) => K::Named(N::Home), Key::Named(NamedKey::End) => K::Named(N::End),
                Key::Character(c) => K::Char(c.to_string()), _ => K::Char(String::new()) };
            min_ui::KeyEvent { key: k, text: text.map(|t| t.to_string()), shift, cmd } };
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
        let dirty = match key {
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
            Key::Character(c) if cmd && shift && c.to_lowercase() == "h" => self.ed.toggle_mark(Mark::Highlight("#5B4B00".into()), now).unwrap_or(Dirty::Nothing),
            Key::Character(c) if cmd && matches!(c.to_lowercase().as_str(), "b" | "i" | "u" | "e" | "\\") => match c.to_lowercase().as_str() {
                "b" => self.ed.toggle_mark(Mark::Bold, now), "i" => self.ed.toggle_mark(Mark::Italic, now), "u" => self.ed.toggle_mark(Mark::Underline, now), "e" => self.ed.toggle_mark(Mark::Code, now), _ => self.ed.clear_formatting(now) }.unwrap_or(Dirty::Nothing),
            Key::Character(c) if cmd => match c.to_lowercase().as_str() {
                "a" => { self.ed.select_all(); Dirty::Nothing }
                "s" => { self.save(); return }
                "z" => if shift { self.ed.redo() } else { self.ed.undo() }.unwrap_or(Dirty::Nothing),
                "c" | "x" => { let t = self.ed.selected_text(); if !t.is_empty() { if let Some(cb) = self.clipboard.as_mut() { let _ = cb.set_text(t); } } if c.to_lowercase() == "x" { self.ed.delete(false, now).unwrap_or(Dirty::Nothing) } else { Dirty::Nothing } }
                "v" => match self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()) { Some(t) => self.ed.insert(&t, now).unwrap_or(Dirty::Nothing), None => Dirty::Nothing },
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
        ((u.x0 + self.side_w + MARGIN * s) / s, (u.y0 + PAD * s - self.scroll) / s, u.width() / s, u.height() / s)
    }
    fn ax_note_frame(&mut self) -> (f64, f64, f64, f64) { if !self.ensure() { return (0.0, 0.0, 0.0, 0.0) } let l = self.layout.as_ref().unwrap(); let s = l.scale as f64; (self.side_w / s + MARGIN, PAD - self.scroll / s, l.width as f64 / s, l.height as f64 / s) }
    fn ax_viewport(&mut self) -> (f64, f64) { self.gpu.as_ref().map(|g| { let (w, h) = g.presenter.size(); let s = g.window.scale_factor(); (w as f64 / s, h as f64 / s) }).unwrap_or((0.0, 0.0)) }
    fn replace_before_caret(&mut self, text: &str) {
        let now = self.now_ms(); if !self.ensure() { return }
        let l = self.layout.as_ref().unwrap();
        if self.ed.selection().is_some_and(|s| s.is_caret()) { self.ed.move_caret(l, Move::Left, true); }
        let d = self.ed.insert(text, now).unwrap_or(Dirty::Nothing); self.apply(d);
    }
    fn ax_scroll_fraction(&mut self) -> f64 {
        let (Some(l), Some(g)) = (self.layout.as_ref(), self.gpu.as_ref()) else { return 0.0 }; let s = g.window.scale_factor();
        let max = (l.height as f64 + 2.0 * PAD * s - g.presenter.size().1 as f64).max(1.0); (self.scroll / max).clamp(0.0, 1.0)
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.gpu.is_some() { return }
        let window: Arc<Box<dyn Window>> = Arc::new(el.create_window(WindowAttributes::default().with_title("minNotes").with_surface_size(LogicalSize::new(1000.0, 760.0))).expect("window"));
        let instance = Presenter::instance();
        let surface = instance.create_surface(window.clone()).expect("surface");
        let size = window.surface_size();
        #[cfg(target_os = "macos")]
        let ns_view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().unwrap().as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))]
        let ns_view = None;
        // QCView's display path (row 287): the surface is the display's own — extended range where the screen has it
        let mut presenter = Presenter::new(&instance, surface, (size.width, size.height), Output::Auto, ns_view).expect("no renderer");
        presenter.engine.set_ui_scale(window.scale_factor() as f32);
        let ui = UiLayer::new(&presenter.engine.device, size.width, size.height).expect("ui layer");
        presenter.engine.set_overlay(Some(ui.view()));
        let i = presenter.info();
        println!("presenter: {} ({}) → {:?} {:?} colour space {:?}; scale {}", i.adapter, i.backend, i.output, i.format, i.color_space, window.scale_factor());
        window.request_redraw();
        #[cfg(target_os = "macos")] if let Some(v) = ns_view { text_element::install(self as *mut App as *mut dyn text_element::TextHost, v.cast()); }
        // the request is REFUSED (silently) unless every capability asked for comes with its first value: the caret's area
        ime(window.as_ref().as_ref(), Some(Rect::new(0.0, 0.0, 1.0, 1.0)), true);
        self.gpu = Some(Gpu { window, presenter, ui, fonts: Fonts::new(), ime_area: None }); self.t0 = Some(Instant::now()); self.title();
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
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
            WindowEvent::CloseRequested => { self.autosave(); self.request_close(); if self.quit { #[cfg(target_os = "macos")] text_element::uninstall(); self.finish(); el.exit(); } }
            WindowEvent::Focused(true) => { #[cfg(target_os = "macos")] text_element::focused(); }
            WindowEvent::SurfaceResized(s) => if let Some(g) = self.gpu.as_mut() {
                g.presenter.resize(s.width, s.height); g.presenter.engine.set_ui_scale(g.window.scale_factor() as f32);
                g.ui.resize(&g.presenter.engine.device, s.width, s.height); g.presenter.engine.set_overlay(Some(g.ui.view()));
                g.window.request_redraw();
            },
            WindowEvent::ScaleFactorChanged { .. } => { self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::MouseWheel { delta, .. } => if let Some(g) = &self.gpu {
                let s = g.window.scale_factor();
                if let Some(sb) = self.sidebar.as_mut() { if sb.rail.hit(self.pointer.0, self.pointer.1) { let dy = match delta { MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * s, MouseScrollDelta::PixelDelta(p) => p.y, _ => 0.0 }; sb.rail.wheel(-dy, Instant::now()); g.window.request_redraw(); return } }
                self.scroll -= match delta { MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * s, MouseScrollDelta::PixelDelta(p) => p.y, _ => 0.0 };
                g.window.request_redraw();
            },
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.mods = (st.shift_key(), if cfg!(target_os = "macos") { st.meta_key() } else { st.control_key() }, st.alt_key()); }
            WindowEvent::PointerMoved { position, .. } => { self.pointer = (position.x, position.y);
                if !self.pressed && self.chrome_pointer(min_ui::Event::Move { x: position.x, y: position.y }) { return }
                if self.pressed { if let Some(l) = self.layout.as_ref() { let (x, y) = self.note_point(self.pointer); self.ed.point(l, x, y, true); self.follow = true; if let Some(g) = &self.gpu { g.window.request_redraw(); } } } }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Left), .. } => {
                self.pointer = (position.x, position.y);
                let e = if state == ElementState::Pressed { min_ui::Event::Down { x: position.x, y: position.y, button: min_ui::Button::Primary } } else { min_ui::Event::Up { x: position.x, y: position.y, button: min_ui::Button::Primary } };
                if !(self.pressed && state == ElementState::Released) && self.chrome_pointer(e) { return }
                self.pressed = state == ElementState::Pressed;
                if self.pressed { if let Some(l) = self.layout.as_ref() {
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
                let text = event.text.as_ref().map(|t| t.to_string()); self.key(&event.logical_key, text.as_deref()); }
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
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

/// The page's metrics with the theme's colours for text and links.
fn style() -> Style { Style { text: paint::text(), link: paint::link(), ..Style::default() } }

/// Where staged sessions live on this machine (row 272: never beside the file).
fn sessions_root() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("MN_SESSIONS_DIR") { return p.into() }
    let base = if cfg!(target_os = "macos") { std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support")) } else if cfg!(windows) { std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from) } else { std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from).or(std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))) };
    base.unwrap_or_else(std::env::temp_dir).join("minNotes").join("sessions")
}

fn main() {
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
    let mut store = None;
    let opened = file.map(|path| {
        use mn_store::{staging, Collection, DocKind};
        let root = sessions_root();
        // work a crash (or a close without Save) left behind for this file is taken up again — nothing typed is dropped
        let left = staging::leftovers(&root).into_iter().find(|l| l.original == path);
        let session = match &left { Some(l) => { println!("recovered work: unsaved changes from an earlier session of {} are taken up again", path.display()); staging::Session::resume(l).expect("staging") } None => {
                // a path with nothing at it is NOT quietly a new collection (on a share it may be a missing mount): `--new` asks for one
                let r = if args.iter().any(|a| a == "--new") { staging::Session::create(&root, &path) } else { staging::Session::open(&root, &path) };
                r.unwrap_or_else(|e| { eprintln!("{}: {e}{}", path.display(), if e.kind() == std::io::ErrorKind::NotFound { " — nothing is at this path (is its drive or share connected?). To start a new collection there, add --new." } else { "" }); std::process::exit(2) })
            } };
        let mut col = Collection::open(&session.work()).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(2) });
        fn notes(c: &Collection, parent: Option<BlockId>, out: &mut Vec<mn_store::Entry>) { for e in c.list(parent).unwrap_or_default() { if e.kind == DocKind::Note { out.push(e.clone()); } notes(c, Some(e.id), out); } }
        let mut all = vec![]; notes(&col, None, &mut all);
        let entry = match arg("--note") { Some(t) => all.iter().find(|e| e.title == t).cloned(), None => all.first().cloned() };
        let (note, title) = match entry { Some(e) => (e.id, e.title), None => { let id = col.create(None, DocKind::Note, "Untitled").expect("new note"); let mut d = Doc::new(); d.insert(None, 0, BlockId::new(), Kind::Paragraph, "").unwrap(); col.save_note(id, &d).expect("new note"); (id, "Untitled".to_string()) } };
        let doc = col.load_note(note).expect("note");
        println!("opened {} — note {:?}: {} blocks; first text: {:?}", path.display(), title, doc.len(), doc.walk().iter().find(|b| b.kind.is_text()).map(|b| b.text.chars().take(60).collect::<String>()).unwrap_or_default());
        store = Some(Store { session: Some(session), col, note, title, edited: None, unpublished: left.is_some(), edited_since_stage: false });
        doc
    });
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
            None => sample(),
        },
    } };
    let recovered = store.as_ref().is_some_and(|st| st.unpublished) && std::env::var_os("MN_NO_DIALOGS").is_none();
    let sidebar = store.as_ref().map(|st| { let mut sb = sidebar::Sidebar::new(); sb.set_tree(&st.col); sb });
    let app = App { saver: mn_store::share::Worker::new(), saving: None, store, sidebar, others: Default::default(), side_w: 0.0, confirm: None, quit: false, close_after_save: false, recovered, ed: Editor::new(doc), mods: (false, false, false), pointer: (0.0, 0.0), pressed: false, last_click: None, clipboard: arboard::Clipboard::new().ok(), accent_popup: false, follow: false, flat: None, last_scroll: 0.0, layout: None, gpu: None, scroll: arg("--scroll").and_then(|v| v.parse().ok()).unwrap_or(0.0), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), shot: arg("--shot"), t0: None, frames: 0, draw_ms: vec![] };
    EventLoop::new().unwrap().run_app(app).unwrap();
}
