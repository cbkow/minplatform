//! THE MANUAL'S PICTURES, scripted (`minNotes --shots <script> [--shots-out <dir>]`; QCView's `shots.rs`, row 353): the
//! real application, on a generated collection and a throwaway settings folder, walks a script of its OWN events and
//! saves its OWN rendering of the window - no screenshot by the system, no input sent to the desktop, nothing of the
//! user's on screen. Every picture can be retaken with one command after the interface changes (`tools/mn-shots/`).
//!
//! The script: one command per line, `#` starts a comment. Coordinates are LOGICAL pixels from the window's top left.
//!   size W H              the window's logical size (do this first)
//!   wait SECONDS          let the application run (pictures land, a decode ends)
//!   open PATH             open a collection (as File > Open does)
//!   note TITLE            show the note with this title            canvas TITLE   open the canvas with this title
//!   import PATH|PATH...   files imported as notes (File > Import)   add [@N] PATH|PATH...  files added to the note (at its end, or
//!                         before its Nth block) or put on the open canvas, as a drop does.  `${NAME}` is the environment's.
//!   saveas PATH           save the collection there (what File > Save As does once its panel has a place)
//!   menu NAME             a menu command by its `menu::Cmd` name (Sidebar, ExportSite, PathMappings, ...)
//!   move X Y              the pointer goes there                    leave          the pointer leaves the window
//!   down X Y / up X Y     a primary press / release                 click X Y      both      dclick X Y   twice
//!   rclick X Y            the secondary button, pressed and released
//!   package FILE          Export Collection as Package to this file: its review window
//!   gathergo              Start, in the open Collect Media / package window
//!   site FOLDER           the open (or a new) web folder window started into this folder
//!   columnkind N          the caret's column becomes a choice (1), check (2) or timecode (3) column
//!   export N FILE         the open note or canvas exported (0 HTML, 1 Markdown, 2 Word, 3 PDF, 4 poster)
//!   drop A|B|...          files dropped on the sidebar / the first window
//!   wheel X Y DY          the wheel at a point, in logical pixels (positive scrolls the page down)
//!   hwheel X Y DX         a sideways wheel at a point (positive shows what is further right)
//!   key [mods+]NAME       a key: Enter Tab Escape Backspace Delete Up Down Left Right Home End Space, or one character;
//!                         mods are cmd, shift, alt joined by `+` (cmd+b, shift+Down)
//!   type TEXT             characters, as typing them. NOTE: `key Space` types a space - a person's Space on a video
//!                         plays it (that key is taken before the editor sees it); to play or pause in a script, click the video.
//!   shot NAME [X Y W H]   save the window (or a logical-pixel crop of it) as <out>/NAME.png
//!   quit
use std::time::{Duration, Instant};
use winit31 as winit;
use winit::event::{ButtonSource, ElementState, MouseButton, MouseScrollDelta, PointerSource, WindowEvent};
use winit::application::ApplicationHandler;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

pub struct Shots { lines: Vec<String>, at: usize, out: std::path::PathBuf, wait_until: Option<Instant>,
    /// a `shot` lets frames go by, then saves
    pending: Option<(String, Option<[f64; 4]>, u8)>, pub done: bool }

impl Shots {
    pub fn load(script: &str, out: Option<String>) -> Option<Shots> {
        let text = std::fs::read_to_string(script).map_err(|e| eprintln!("--shots {script}: {e}")).ok()?;
        let out = std::path::PathBuf::from(out.unwrap_or_else(|| "shots-out".into()));
        let _ = std::fs::create_dir_all(&out);
        Some(Shots { lines: text.lines().map(|l| l.split(" #").next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect(), at: 0, out, wait_until: None, pending: None, done: false })
    }
}

fn menu_cmd(name: &str) -> Option<crate::menu::Cmd> {
    use crate::menu::Cmd::*;
    [NewCollection, Open, ClearRecent, Save, SaveAs, CloseCollection, NewNote, NewFolder, Rename, Undo, Redo, Cut, Copy, Paste, PastePlain, PasteMarkdown, SelectAll,
     Bold, Italic, Underline, Strike, Code, Highlight, ClearFormat, Sidebar, Toolbar, ToolbarWide, Footer,
     ExportHtml, ExportSite, ExportMarkdown, ExportDocx, ExportPdf, ExportPoster, CopyMarkdown, ExportPaths, ExportVideoNotes, ExportPackage, CollectMedia, Import, Licenses, PathMappings, Reading, UserGuide, ShortcutsPage, GuideToggle, Pin]
        .into_iter().find(|c| format!("{c:?}") == name)
}

fn named(name: &str) -> Option<Key> {
    Some(Key::Named(match name { "Enter" => NamedKey::Enter, "Tab" => NamedKey::Tab, "Escape" => NamedKey::Escape, "Backspace" => NamedKey::Backspace, "Delete" => NamedKey::Delete,
        "Up" => NamedKey::ArrowUp, "Down" => NamedKey::ArrowDown, "Left" => NamedKey::ArrowLeft, "Right" => NamedKey::ArrowRight, "Home" => NamedKey::Home, "End" => NamedKey::End,
        "Space" => return Some(Key::Character(" ".into())), _ => return None }))
}

impl crate::App {
    fn shots_find(&self, title: &str, kind: mn_store::DocKind) -> Option<mn_doc::BlockId> {
        fn go(col: &mn_store::Collection, parent: Option<mn_doc::BlockId>, title: &str, kind: mn_store::DocKind) -> Option<mn_doc::BlockId> {
            for e in col.list(parent).unwrap_or_default() { if e.kind == kind && e.title == title { return Some(e.id) } if let Some(id) = go(col, Some(e.id), title, kind) { return Some(id) } } None }
        go(&self.store.as_ref()?.col, None, title, kind)
    }
    /// One tick of the script: commands run until one has to wait (a `wait`, or a `shot` that needs drawn frames).
    pub(crate) fn shots_step(&mut self, el: &dyn ActiveEventLoop) {
        let now = Instant::now();
        loop {
            let Some(sh) = self.shots.as_mut() else { return };
            if sh.done { return }
            el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(now + Duration::from_millis(16)));
            if sh.pending.is_some() { if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
            if let Some(t) = sh.wait_until { if now < t { if let Some(g) = &self.gpu { g.window.request_redraw(); } return } sh.wait_until = None; }
            let Some(line) = sh.lines.get(sh.at).cloned() else { sh.done = true; self.shots_quit(el); return };
            sh.at += 1;
            // ${NAME} is the environment's
            let line = { let mut l = line; while let Some(i) = l.find("${") { let Some(j) = l[i..].find('}') else { break }; let v = std::env::var(&l[i + 2..i + j]).unwrap_or_default(); l.replace_range(i..i + j + 1, &v); } l };
            let mut w = line.split_whitespace(); let cmd = w.next().unwrap_or(""); let rest: Vec<&str> = w.collect();
            let num = |i: usize| rest.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            let Some((scale, wid)) = self.gpu.as_ref().map(|g| (g.window.scale_factor(), g.window.id())) else { self.shots.as_mut().unwrap().at -= 1; return };
            let pt = |i: usize| winit::dpi::PhysicalPosition::new(num(i) * scale, num(i + 1) * scale);
            let tail = line[cmd.len()..].trim().to_string();
            println!("shots: {line}");
            let moved = |me: &mut Self, p| me.window_event(el, wid, WindowEvent::PointerMoved { device_id: None, position: p, primary: true, source: PointerSource::Mouse });
            let button = |me: &mut Self, p, state, b| me.window_event(el, wid, WindowEvent::PointerButton { device_id: None, state, position: p, primary: true, button: ButtonSource::Mouse(b), is_macos_activation_click: false });
            match cmd {
                "size" => { if let Some(g) = self.gpu.as_mut() { let _ = g.window.request_surface_size(winit::dpi::LogicalSize::new(num(0), num(1)).into()); } self.shots.as_mut().unwrap().wait_until = Some(now + Duration::from_millis(500)); return }
                "wait" => { self.shots.as_mut().unwrap().wait_until = Some(now + Duration::from_secs_f64(num(0))); return }
                "open" => self.request_close(crate::Pending::Open(tail.into())),
                "note" => match self.shots_find(&tail, mn_store::DocKind::Note) { Some(id) => { self.open_note(id); self.blank = false; self.scroll = 0.0; self.layout = None; } None => eprintln!("shots: no note named '{tail}'") },
                "canvas" => match self.shots_find(&tail, mn_store::DocKind::Canvas) { Some(id) => self.open_canvas(id), None => eprintln!("shots: no canvas named '{tail}'") },
                "saveas" => self.save_as_to(tail.clone().into()),
                "import" => self.import_files(tail.split('|').map(|p| p.trim().into()).collect()),
                "add" => { let (at, list) = match tail.strip_prefix('@').and_then(|t| t.split_once(' ')) { Some((n, l)) => (n.parse::<usize>().ok(), l.to_string()), None => (None, tail.clone()) };
                    let drop = if self.files.canvas.is_some() { self.files.canvas_at.set(None); Self::canvas_drop() } else { mn_edit::Drop { parent: None, index: at.unwrap_or(self.ed.doc.children(None).len()), depth: None, beside: None } };
                    self.add_files(list.split('|').map(|p| p.trim().into()).collect(), drop); }
                "menu" => match menu_cmd(rest.first().copied().unwrap_or("")) { Some(c) => self.menu_cmd(c), None => eprintln!("shots: no menu command '{tail}'") },
                "move" => moved(self, pt(0)),
                "leave" => self.window_event(el, wid, WindowEvent::PointerLeft { device_id: None, position: None, primary: true, kind: winit::event::PointerKind::Mouse }),
                "down" => { moved(self, pt(0)); button(self, pt(0), ElementState::Pressed, MouseButton::Left); }
                "up" => { moved(self, pt(0)); button(self, pt(0), ElementState::Released, MouseButton::Left); }
                "click" | "dclick" => { moved(self, pt(0)); for _ in 0..if cmd == "dclick" { 2 } else { 1 } { button(self, pt(0), ElementState::Pressed, MouseButton::Left); button(self, pt(0), ElementState::Released, MouseButton::Left); } }
                "rclick" => { moved(self, pt(0)); button(self, pt(0), ElementState::Pressed, MouseButton::Right); button(self, pt(0), ElementState::Released, MouseButton::Right); }
                "wheel" => { moved(self, pt(0)); self.window_event(el, wid, WindowEvent::MouseWheel { device_id: None, delta: MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, -num(2) * scale)), phase: winit::event::TouchPhase::Moved, source: winit::event::ScrollSource::Wheel }); }
                "hwheel" => { moved(self, pt(0)); self.window_event(el, wid, WindowEvent::MouseWheel { device_id: None, delta: MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(-num(2) * scale, 0.0)), phase: winit::event::TouchPhase::Moved, source: winit::event::ScrollSource::Wheel }); }
                "key" => { let mut parts: Vec<&str> = tail.split('+').collect(); let name = if tail.ends_with('+') { "+" } else { parts.pop().unwrap_or("") };
                    let key = named(name).unwrap_or_else(|| Key::Character(name.into())); let was = self.mods;
                    self.mods = (parts.contains(&"shift"), parts.contains(&"cmd"), parts.contains(&"alt"));
                    let text = match &key { Key::Character(c) if !self.mods.1 => Some(c.to_string()), _ => None };
                    self.key(&key, text.as_deref()); self.mods = was; }
                // files dropped on the sidebar (or on the first window, which takes them the same way): the drop's own routine, without the system's drag
                "drop" => { self.side_drop(tail.split('|').map(std::path::PathBuf::from).collect(), None); }
                // the windows that ask the system for a place first, given the place: the package's review; Start in the open Collect Media / package window; a web folder started
                "package" => { self.gather_open(crate::gather::What::Package, Some(tail.into())); }
                "gathergo" => { if let Some(w) = self.files.gather.as_mut() { w.go(); } }
                "site" => { if self.files.site.is_none() { self.site_open(); } self.site_start(tail.into()); }
                // the caret's column made a kind (1 choice, 2 check, 3 timecode), as its menu does, without the question
                "columnkind" => { self.column_kind(num(0) as u8, true); }
                // what is open exported to a file (0 HTML, 1 Markdown, 2 Word, 3 PDF, 4 the canvas's poster), without the system's panel
                "export" => { let (n, path) = tail.split_once(' ').unwrap_or((&tail, "")); self.export(n.parse().unwrap_or(0), Some(path.into())); }
                "type" => { let was = self.mods; self.mods = (false, false, false); for c in tail.chars() { let s = c.to_string(); self.key(&Key::Character(s.as_str().into()), Some(&s)); } self.mods = was; }
                "shot" => { let crop = (rest.len() >= 5).then(|| [num(1), num(2), num(3), num(4)]); self.shots.as_mut().unwrap().pending = Some((rest.first().unwrap_or(&"shot").to_string(), crop, 0)); if let Some(g) = &self.gpu { g.window.request_redraw(); } return }
                "quit" => { self.shots.as_mut().unwrap().done = true; self.shots_quit(el); return }
                other => eprintln!("shots: unknown command '{other}'"),
            }
            if let Some(g) = &self.gpu { g.window.request_redraw(); }
        }
    }
    fn shots_quit(&mut self, el: &dyn ActiveEventLoop) { #[cfg(target_os = "macos")] crate::text_element::uninstall(); self.finish(); el.exit(); }
    /// After a draw: a pending `shot` lets three frames go by (the interface repaints, pictures settle) and saves.
    pub(crate) fn shots_capture(&mut self) {
        let Some(sh) = self.shots.as_mut() else { return };
        let Some((name, crop, frames)) = sh.pending.as_mut() else { return };
        if *frames < 3 { *frames += 1; return }
        let (name, crop) = (name.clone(), *crop);
        let path = sh.out.join(format!("{name}.png")); sh.pending = None;
        let Some(g) = self.gpu.as_mut() else { return };
        let scale = g.window.scale_factor();
        match g.presenter.capture_window() {
            Some((px, w, h)) => {
                let (x0, y0, cw, ch) = match crop { Some([x, y, cw, ch]) => { let (x0, y0) = (((x * scale) as u32).min(w - 1), ((y * scale) as u32).min(h - 1)); (x0, y0, ((cw * scale) as u32).min(w - x0).max(1), ((ch * scale) as u32).min(h - y0).max(1)) } None => (0, 0, w, h) };
                let oetf = |v: f32| { let v = v.clamp(0.0, 1.0); ((if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0 + 0.5) as u8 };
                let mut bytes = Vec::with_capacity((cw * ch * 3) as usize);
                for row in y0..y0 + ch { for col in x0..x0 + cw { let i = ((row * w + col) * 4) as usize; bytes.extend_from_slice(&[oetf(px[i]), oetf(px[i + 1]), oetf(px[i + 2])]); } }
                let Ok(file) = std::fs::File::create(&path) else { eprintln!("shots: {} cannot be written", path.display()); return };
                let mut e = png::Encoder::new(std::io::BufWriter::new(file), cw, ch); e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight); e.set_compression(png::Compression::Best);
                match e.write_header().and_then(|mut wr| wr.write_image_data(&bytes)) { Ok(()) => println!("shots: wrote {} ({cw}x{ch})", path.display()), Err(e) => eprintln!("shots: {e}") }
            }
            None => eprintln!("shots: this surface cannot be captured"),
        }
    }
}
