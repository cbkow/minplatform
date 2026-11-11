//! minNotes on the platform — step 2 (canon rows 286, 287): ONE note, laid out by `mn-layout`, drawn into the UI layer
//! over QCView's presenter (the window surface the embedded player will draw its HDR picture into), edited through
//! `mn-edit`: click, drag and double-click select; the arrows (Alt = by word, Cmd / Ctrl = line and note ends, Shift
//! extends), typing, Enter, Backspace / Delete, undo / redo, select all, and plain-text cut / copy / paste. Nothing is
//! saved; input methods and the desktop text element come next.
//!   minNotes                         a built-in sample note (synthetic)
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
#[cfg(target_os = "macos")]
mod text_element;

/// The page's side margin and top / bottom padding, logical px (the Qt app: 48 and 0 — `docTopPad` 28 is defined there
/// but unused; 28 is used here so the first line does not touch the title bar).
const MARGIN: f64 = 48.0;
const PAD: f64 = 28.0;

struct Gpu { window: Arc<Box<dyn Window>>, presenter: Presenter, ui: UiLayer, fonts: Fonts }
struct App { ed: Editor, mods: (bool, bool, bool), pointer: (f64, f64), pressed: bool, last_click: Option<(Instant, f64, f64)>, clipboard: Option<arboard::Clipboard>, follow: bool, flat: Option<mn_doc::FlatText>, last_scroll: f64, layout: Option<NoteLayout>, gpu: Option<Gpu>, scroll: f64, seconds: f32, shot: Option<String>, t0: Option<Instant>, frames: u32, draw_ms: Vec<f32> }

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
        let (w, h) = g.presenter.size(); let s = g.window.scale_factor();
        let page_w = (w as f64 - 2.0 * MARGIN * s).max(320.0 * s);
        if self.layout.as_ref().is_none_or(|l| (l.width as f64 - page_w).abs() > 0.5 || (l.scale as f64 - s).abs() > 1e-6) { self.layout = Some(NoteLayout::build(&self.ed.doc, &mut g.fonts, &Style::default(), page_w as f32, s as f32)); }
        // MN_DEMO=1 (for `--shot` self-checks; no input is sent): the app types into its own first paragraph and selects
        // from the heading into the first list item
        if self.frames == 0 && std::env::var_os("MN_DEMO").is_some() {
            let top: Vec<BlockId> = self.ed.doc.children(None).to_vec();
            if top.len() > 3 { use mn_edit::{Pos, Selection};
                self.ed.set_selection(Some(Selection::caret(Pos { block: top[1], at: 0 }))); let _ = self.ed.insert("Typed by the demo. ", 0); self.layout.as_mut().unwrap().update_block(&self.ed.doc, &mut g.fonts, top[1]);
                self.ed.set_selection(Some(Selection { anchor: Pos { block: top[0], at: 7 }, focus: Pos { block: top[3], at: 10 } })); }
        }
        let l = self.layout.as_ref().unwrap();
        // after an edit or a caret move the caret is brought into view
        if std::mem::take(&mut self.follow) { if let Some(c) = self.ed.caret_rect(l, 1.0) { let (top, bottom) = (c.y0 + PAD * s - 24.0 * s, c.y1 + PAD * s + 24.0 * s); if top < self.scroll { self.scroll = top } else if bottom > self.scroll + h as f64 { self.scroll = bottom - h as f64 } } }
        self.scroll = self.scroll.clamp(0.0, (l.height as f64 + 2.0 * PAD * s - h as f64).max(0.0));
        #[cfg(target_os = "macos")] if (self.scroll - self.last_scroll).abs() > 0.01 { text_element::scrolled(); }
        self.last_scroll = self.scroll;
        let view = Rect::new(0.0, 0.0, w as f64, h as f64);
        let off = kurbo::Vec2::new(MARGIN * s, PAD * s - self.scroll);
        let scene = g.ui.scene_mut(); scene.reset();
        scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::PAGE, None, &view);
        for r in self.ed.selection_rects(l) { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, paint::SELECTION, None, &(r + off)); }
        paint::note(scene, &self.ed.doc, l, &mut g.fonts, off.x, off.y, view);
        if self.ed.selection().is_some_and(|x| x.is_caret()) { if let Some(c) = self.ed.caret_rect(l, (1.5 * s) as f32) { scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, peniko::Color::WHITE, None, &(c + off)); } }
        g.ui.render(&g.presenter.engine.device, &g.presenter.engine.queue);
        let window = g.window.clone();
        g.presenter.draw(|| window.pre_present_notify());
        self.frames += 1; self.draw_ms.push(t.elapsed().as_secs_f32() * 1000.0);
    }
    /// What an edit changed, taken to the layout; the caret is then followed.
    fn apply(&mut self, d: Dirty) {
        let d_changed = d != Dirty::Nothing;
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
        let page_w = (w as f64 - 2.0 * MARGIN * s).max(320.0 * s);
        if self.layout.as_ref().is_none_or(|l| (l.width as f64 - page_w).abs() > 0.5 || (l.scale as f64 - s).abs() > 1e-6) { self.layout = Some(NoteLayout::build(&self.ed.doc, &mut g.fonts, &Style::default(), page_w as f32, s as f32)); }
        if self.flat.is_none() { self.flat = Some(mn_doc::FlatText::build(&self.ed.doc)); }
        true
    }
    fn now_ms(&self) -> u64 { self.t0.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0) }
    /// A window point in the note's coordinates.
    fn note_point(&self, p: (f64, f64)) -> (f64, f64) { let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0); (p.0 - MARGIN * s, p.1 - PAD * s + self.scroll) }
    fn key(&mut self, key: &Key, text: Option<&str>) {
        let (shift, cmd, alt) = self.mods; let now = self.now_ms();
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
            Key::Character(c) if cmd => match c.to_lowercase().as_str() {
                "a" => { self.ed.select_all(); Dirty::Nothing }
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
        ((u.x0 + MARGIN * s) / s, (u.y0 + PAD * s - self.scroll) / s, u.width() / s, u.height() / s)
    }
    fn ax_note_frame(&mut self) -> (f64, f64, f64, f64) { if !self.ensure() { return (0.0, 0.0, 0.0, 0.0) } let l = self.layout.as_ref().unwrap(); let s = l.scale as f64; (MARGIN, PAD - self.scroll / s, l.width as f64 / s, l.height as f64 / s) }
    fn ax_viewport(&mut self) -> (f64, f64) { self.gpu.as_ref().map(|g| { let (w, h) = g.presenter.size(); let s = g.window.scale_factor(); (w as f64 / s, h as f64 / s) }).unwrap_or((0.0, 0.0)) }
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
        self.gpu = Some(Gpu { window, presenter, ui, fonts: Fonts::new() }); self.t0 = Some(Instant::now());
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if self.seconds > 0.0 { if let Some(t0) = self.t0 { let dl = t0 + std::time::Duration::from_secs_f32(self.seconds); if Instant::now() >= dl { #[cfg(target_os = "macos")] text_element::uninstall(); self.finish(); el.exit(); return } el.set_control_flow(ControlFlow::WaitUntil(dl)); } }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => { #[cfg(target_os = "macos")] text_element::uninstall(); self.finish(); el.exit(); }
            WindowEvent::Focused(true) => { #[cfg(target_os = "macos")] text_element::focused(); }
            WindowEvent::SurfaceResized(s) => if let Some(g) = self.gpu.as_mut() {
                g.presenter.resize(s.width, s.height); g.presenter.engine.set_ui_scale(g.window.scale_factor() as f32);
                g.ui.resize(&g.presenter.engine.device, s.width, s.height); g.presenter.engine.set_overlay(Some(g.ui.view()));
                g.window.request_redraw();
            },
            WindowEvent::ScaleFactorChanged { .. } => { self.layout = None; if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::MouseWheel { delta, .. } => if let Some(g) = &self.gpu {
                let s = g.window.scale_factor();
                self.scroll -= match delta { MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * s, MouseScrollDelta::PixelDelta(p) => p.y, _ => 0.0 };
                g.window.request_redraw();
            },
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.mods = (st.shift_key(), if cfg!(target_os = "macos") { st.meta_key() } else { st.control_key() }, st.alt_key()); }
            WindowEvent::PointerMoved { position, .. } => { self.pointer = (position.x, position.y);
                if self.pressed { if let Some(l) = self.layout.as_ref() { let (x, y) = self.note_point(self.pointer); self.ed.point(l, x, y, true); self.follow = true; if let Some(g) = &self.gpu { g.window.request_redraw(); } } } }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Left), .. } => {
                self.pointer = (position.x, position.y); self.pressed = state == ElementState::Pressed;
                if self.pressed { if let Some(l) = self.layout.as_ref() {
                    let (x, y) = self.note_point(self.pointer);
                    let double = self.last_click.is_some_and(|(t, px, py)| t.elapsed().as_millis() < 400 && (px - x).abs() < 4.0 && (py - y).abs() < 4.0);
                    if double { self.ed.point_word(l, x, y); self.last_click = None } else { self.ed.point(l, x, y, self.mods.0); self.last_click = Some((Instant::now(), x, y)); }
                    #[cfg(target_os = "macos")] text_element::changed(false);
                    if let Some(g) = &self.gpu { g.window.request_redraw(); } } }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => { let text = event.text.as_ref().map(|t| t.to_string()); self.key(&event.logical_key, text.as_deref()); }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let doc = match arg("--v2") {
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
    };
    let app = App { ed: Editor::new(doc), mods: (false, false, false), pointer: (0.0, 0.0), pressed: false, last_click: None, clipboard: arboard::Clipboard::new().ok(), follow: false, flat: None, last_scroll: 0.0, layout: None, gpu: None, scroll: arg("--scroll").and_then(|v| v.parse().ok()).unwrap_or(0.0), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), shot: arg("--shot"), t0: None, frames: 0, draw_ms: vec![] };
    EventLoop::new().unwrap().run_app(app).unwrap();
}
