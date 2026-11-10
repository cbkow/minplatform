//! DOCS SCREENSHOTS, scripted (`QCView --shots <script> [--shots-out <dir>]`): the real app, on generated test media and a
//! throwaway settings folder, walks a script of its OWN commands and saves its OWN rendering of the window — no OS
//! screenshot, no input sent to the desktop, nothing of the user's on screen. The manual's pictures are made this way
//! (`tools/docs-shots/`), so they can be retaken whenever the interface changes.
//!
//! The script: one command per line, `#` starts a comment. Coordinates are LOGICAL pixels from the window's top-left.
//!   size W H            the window's logical size (do this first)
//!   open PATH           add a file to the project and load it          add PATH      add without loading
//!   project PATH        open a project file                             bsource PATH  dual view's B source
//!   wait SECONDS        let the app run (decodes land, animations end)
//!   menu NAME           a menu command by its `menu::Cmd` name: Compact Pin Fullscreen DefaultFrame LeftRail RightRail
//!                       ColorPanel Notes Shortcuts Settings Background SafetyGuides NewProject About Licenses
//!   dual MODE           single | sbs | split | diff
//!   color on|off        Color on or off                                 set KEY VALUE  a settings key (bool / int / text)
//!   seek FRAME          play / pause
//!   move X Y            the pointer goes there (hover)                  leave         the pointer leaves the window
//!   down X Y / up X Y   a primary press / release                       click X Y     both
//!   rdown X Y / rup X Y the secondary button                            wheel X Y DY  a wheel step at a point
//!   key CHAR            a transport key by its letter (i o v r b …: `key_map`), to the loaded clip or the dual master
//!   type TEXT           characters into the field that has the focus (the shortcuts filter, the go-to box, a note)
//!   enter               Enter in that field
//!   playlist PATH…      a new playlist of these files (`|` between paths), loaded
//!   load NAME           load the project item with this name
//!   savedual NAME       save the current dual view as an item       saveproject PATH   save the project
//!   lscroll PX / rscroll PX   scroll the left / right rail by logical pixels (down is positive)
//!   zoom X UNITS [alt]  the wheel over the timeline at logical X (120 units = one notch; alt = pan)
//!   shot NAME [X Y W H] save the window (or a logical-pixel crop of it) as <out>/NAME.png
//!   quit
use std::time::{Duration, Instant};

pub struct Shots { lines: Vec<String>, at: usize, out: std::path::PathBuf, wait_until: Option<Instant>, /// a `shot` waits one drawn frame, then saves
    pub pending: Option<(String, Option<[f64; 4]>, u8)>, pub done: bool }

impl Shots {
    pub fn load(script: &str, out: Option<String>) -> Option<Shots> {
        let text = std::fs::read_to_string(script).map_err(|e| eprintln!("--shots {script}: {e}")).ok()?;
        let out = std::path::PathBuf::from(out.unwrap_or_else(|| "shots-out".into()));
        let _ = std::fs::create_dir_all(&out);
        Some(Shots { lines: text.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect(), at: 0, out, wait_until: None, pending: None, done: false })
    }
    pub fn path(&self, name: &str) -> String { self.out.join(format!("{name}.png")).to_string_lossy().into_owned() }
}

impl crate::App {
    /// One tick of the script: commands run until one has to wait (a `wait`, or a `shot` that needs the next drawn frame).
    pub(crate) fn shots_step(&mut self, now: Instant) {
        loop {
            let Some(sh) = self.shots.as_mut() else { return };
            if sh.done || sh.pending.is_some() { return }
            if let Some(t) = sh.wait_until { if now < t { return } sh.wait_until = None; }
            let Some(line) = sh.lines.get(sh.at).cloned() else { sh.done = true; self.menu_queue.push(crate::menu::Cmd::Quit); return };
            sh.at += 1;
            let mut w = line.split_whitespace(); let cmd = w.next().unwrap_or(""); let rest: Vec<&str> = w.collect();
            let num = |i: usize| rest.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            let scale = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
            let pt = |i: usize| (num(i) * scale, num(i + 1) * scale);
            let tail = line[cmd.len()..].trim().to_string();
            log::info!("shots: {line}");
            use min_ui::{Button, Event};
            match cmd {
                "size" => { if let Some(g) = self.gpu.as_mut() { let _ = g.window.request_surface_size(winit31::dpi::LogicalSize::new(num(0), num(1)).into()); g.ui_dirty = true; } self.shots.as_mut().unwrap().wait_until = Some(now + Duration::from_millis(400)); return }
                "wait" => { self.shots.as_mut().unwrap().wait_until = Some(now + Duration::from_secs_f64(num(0))); return }
                "open" => { let _ = self.add_and_load(&tail, true); }
                "add" => { let _ = self.add_and_load(&tail, false); }
                "project" => self.open_project(&tail),
                "bsource" => { if let Some(id) = self.add_and_load(&tail, false) { self.set_b_source(&id); } }
                "dual" => self.set_compare_mode(match rest.first().copied() { Some("sbs") => min_present::dual::CompareMode::SideBySide, Some("split") => min_present::dual::CompareMode::Split, Some("diff") => min_present::dual::CompareMode::Difference, _ => min_present::dual::CompareMode::Single }),
                "preset" => { if let Some(st) = self.colors.all().into_iter().find(|p| p.name == tail).map(|p| p.stored) { self.color_cmd(crate::color_drawer::Cmd::Pick(st)); } else { eprintln!("shots: no preset named '{tail}' (have: {:?})", self.colors.all().iter().map(|p| p.name.clone()).collect::<Vec<_>>()); } }
                "newocio" => self.color_cmd(crate::color_drawer::Cmd::NewOcio),
                "live" => self.connect_url(&tail, ""),
                "color" => self.color_cmd(crate::color_drawer::Cmd::SetOn(rest.first() == Some(&"on"))),
                "set" => { if let (Some(k), Some(v)) = (rest.first(), rest.get(1)) { match *v { "true" | "false" => { self.settings.set_bool(k, *v == "true"); } n if n.parse::<i64>().is_ok() => { self.settings.set_int(k, n.parse().unwrap()); } s => { self.settings.set_str(k, s); } } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } } }
                "seek" => { let f = num(0) as i32; if let Some(d) = self.dual.as_mut() { d.seek_to_frame(f); } else if let Some(p) = self.player.as_mut() { p.seek_to_frame(f); } }
                "play" => { if let Some(d) = self.dual.as_mut() { d.play(); } else if let Some(p) = self.player.as_mut() { p.play(); } }
                "pause" => { if let Some(d) = self.dual.as_mut() { d.pause(); } else if let Some(p) = self.player.as_mut() { p.pause(); } }
                "menu" => { use crate::menu::Cmd as M; let c = match rest.first().copied().unwrap_or("") { "Compact" => M::Compact, "Pin" => M::Pin, "Fullscreen" => M::Fullscreen, "DefaultFrame" => M::DefaultFrame, "LeftRail" => M::LeftRail, "RightRail" => M::RightRail, "ColorPanel" => M::ColorPanel, "Notes" => M::Notes, "Shortcuts" => M::Shortcuts, "Settings" => M::Settings, "Background" => M::Background, "SafetyGuides" => M::SafetyGuides, "NewProject" => M::NewProject, "About" => M::About, "Licenses" => M::Licenses, other => { eprintln!("shots: no menu command '{other}'"); continue } }; self.menu_queue.push(c); self.shots.as_mut().unwrap().wait_until = Some(now + Duration::from_millis(50)); return }
                "move" => { let (x, y) = pt(0); self.pointer(Event::Move { x, y }); }
                "leave" => self.pointer(Event::Leave),
                "down" => { let (x, y) = pt(0); self.pointer(Event::Move { x, y }); self.pointer(Event::Down { x, y, button: Button::Primary }); }
                "up" => { let (x, y) = pt(0); self.pointer(Event::Move { x, y }); self.pointer(Event::Up { x, y, button: Button::Primary }); }
                "click" => { let (x, y) = pt(0); self.pointer(Event::Move { x, y }); self.pointer(Event::Down { x, y, button: Button::Primary }); self.pointer(Event::Up { x, y, button: Button::Primary }); }
                "rdown" => { let (x, y) = pt(0); self.pointer(Event::Move { x, y }); self.pointer(Event::Down { x, y, button: Button::Secondary }); }
                "rup" => { let (x, y) = pt(0); self.pointer(Event::Up { x, y, button: Button::Secondary }); }
                "shot" => { let crop = (rest.len() >= 5).then(|| [num(1), num(2), num(3), num(4)]); if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } self.shots.as_mut().unwrap().pending = Some((rest.first().unwrap_or(&"shot").to_string(), crop, 0)); return }
                "key" => { if let Some(ch) = rest.first().and_then(|c| c.chars().next()) { use qcv_player::key_map::{map_key, Key, Mods}; if let Some(a) = map_key(Key::Char(ch), Mods::default(), true, false) { if let Some(d) = self.dual.as_mut() { d.apply(a, now); } else if let Some(p) = self.player.as_mut() { p.apply(a, now); } } } }
                "type" | "enter" => {
                    let keys: Vec<min_ui::KeyEvent> = if cmd == "enter" { vec![min_ui::KeyEvent { key: min_ui::Key::Named(min_ui::Named::Enter), text: None, shift: false, cmd: false }] } else { tail.chars().map(|c| min_ui::KeyEvent { key: min_ui::Key::Char(c.to_string()), text: Some(c.to_string()), shift: c.is_uppercase(), cmd: false }).collect() };
                    for k in keys {
                        let Some(g) = self.gpu.as_mut() else { break };
                        if g.shortcuts.has_focus() { g.shortcuts.key(&k, &mut g.fonts); }
                        else if g.strip.is_open() { let commit = g.strip.key(&k, &mut g.fonts); match commit { Some(crate::status_strip::Commit::Frame(f)) => { if let Some(p) = self.player.as_mut() { p.seek_to_frame(f); } } Some(crate::status_strip::Commit::Time(sec)) => { if let Some(p) = self.player.as_mut() { let f = (sec * p.rate().as_f64()).round() as i32; p.seek_to_frame(f); } } _ => {} } }
                        else if g.notes.has_focus() { let cmds = g.notes.key(&k, &mut g.fonts); for c in cmds { self.notes_cmd(c); } }
                        else if g.drawer.has_focus() || g.drawer.is_modal() { let (cmds, _) = g.drawer.key(&k, &mut g.fonts); for c in cmds { self.color_cmd(c); } }
                    }
                }
                "playlist" => { let paths: Vec<String> = tail.split('|').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(); self.create_playlist(&paths); }
                "load" => { if let Some(id) = self.project.pool().iter().find(|m| m.name == tail).map(|m| m.id.clone()) { self.load_item(&id); } else { eprintln!("shots: no item named '{tail}' (have: {:?})", self.project.pool().iter().map(|m| m.name.clone()).collect::<Vec<_>>()); } }
                "savedual" => { if let Some(data) = self.dual_pair_data() { let id = self.project.add_dual_pair(&tail, data); self.active_dual_view = id; } }
                "saveproject" => { if let Err(e) = self.project.save_project(&tail) { eprintln!("shots: save: {e}"); } }
                "lscroll" | "rscroll" => { if let Some(g) = self.gpu.as_mut() { let px = num(0) * scale; if cmd == "lscroll" { g.left.wheel(px, now); } else { g.right.wheel(px, now); } } }
                "zoom" => { let st = crate::scrub_state(crate::pb(&self.dual, &self.player), self.player.as_ref()); if let Some(g) = self.gpu.as_mut() { g.scrubber.wheel(num(0) * scale, num(1), rest.get(2) == Some(&"alt"), &st, scale); } }
                "quit" => { self.shots.as_mut().unwrap().done = true; self.menu_queue.push(crate::menu::Cmd::Quit); return }
                other => eprintln!("shots: unknown command '{other}'"),
            }
            if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
        }
    }
    /// After the tick's draw: a pending `shot` lets two frames go by (the interface repaints, then the picture settles) and saves.
    pub(crate) fn shots_capture(&mut self) {
        let Some(sh) = self.shots.as_mut() else { return };
        let Some((name, crop, frames)) = sh.pending.as_mut() else { return };
        if *frames < 2 { *frames += 1; if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } return }
        let (name, crop) = (name.clone(), *crop);
        let path = sh.path(&name); sh.pending = None;
        let Some(g) = self.gpu.as_mut() else { return };
        let scale = g.window.scale_factor();
        match g.presenter.capture_window() {
            Some((px, w, h)) => {
                let (px, w, h) = match crop {
                    Some([x, y, cw, ch]) => { let (x0, y0) = (((x * scale) as u32).min(w - 1), ((y * scale) as u32).min(h - 1)); let (cw, ch) = (((cw * scale) as u32).min(w - x0).max(1), ((ch * scale) as u32).min(h - y0).max(1)); let mut out = Vec::with_capacity((cw * ch * 4) as usize); for row in y0..y0 + ch { let s = ((row * w + x0) * 4) as usize; out.extend_from_slice(&px[s..s + (cw * 4) as usize]); } (out, cw, ch) }
                    None => (px, w, h),
                };
                crate::write_png_display(&path, &px, w, h, true);
                println!("shots: wrote {path} ({w}x{h})");
            }
            None => eprintln!("shots: this surface cannot be captured (a float surface is needed — run it on the Mac)"),
        }
    }
}
