//! S3 text — a parley/vello editor in a winit window with an AccessKit tree, IME, clipboard and undo-as-ops, in TWO shapes:
//! one `PlainEditor` per PARAGRAPH (blocks — the canon's text-node shape) or one over the whole document (`--single`, the
//! control: parley relayouts the whole buffer per edit). `--lines N` generates the document, `--probe N` types N keys internally
//! and prints per-key / frame / undo costs, injected keys report key→present. Verdict: docs/spikes/S3-text.md.
mod ax;
use std::{num::NonZeroUsize, sync::Arc, time::Instant};
use accesskit_winit::{Adapter, Event as AkEvent, WindowEvent as AkWindowEvent};
use parley::{FontContext, LayoutContext, PlainEditor, PositionedLayoutItem, StyleProperty};
use vello::{kurbo::{Affine, Rect}, peniko::{Brush, Color, Fill}, util::{RenderContext, RenderSurface}, AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use winit::{application::ApplicationHandler, event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent}, event_loop::{ActiveEventLoop, EventLoop}, keyboard::{Key, NamedKey}, window::{Window, WindowId}};

const PAD: f64 = 32.0;
const WORDS: &[&str] = &["the", "presenter", "draws", "every", "frame", "from", "the", "model", "while", "parley", "shapes", "lines", "and", "vello", "fills", "glyphs", "on", "a", "wgpu", "surface", "with", "rational", "time", "under", "the", "sequencer", "so", "nothing", "drops", "or", "duplicates", "at", "a", "cut", "between", "clips", "of", "different", "codecs"];
fn generate(lines: usize) -> String { (0..lines).map(|i| { let n = 9 + (i * 7) % 9; (0..n).map(|k| WORDS[(i * 13 + k * 5) % WORDS.len()]).collect::<Vec<_>>().join(" ") }).collect::<Vec<_>>().join("\n") }

pub fn pct(v: &mut Vec<f32>, p: f32) -> f32 { if v.is_empty() { return f32::NAN } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f32 * p / 100.0).round() as usize] }

struct Block { editor: PlainEditor<Brush>, y: f32, height: f32 }   // physical px
#[derive(Clone, Debug)] struct Op { block: usize, at: usize, removed: String, inserted: String }

struct Ctx { window: Arc<Window>, surface: RenderSurface<'static>, renderer: Renderer, scene: Scene, adapter: Adapter }

struct App {
    rc: RenderContext, ctx: Option<Ctx>, font_cx: FontContext, layout_cx: LayoutContext<Brush>,
    blocks: Vec<Block>, focus: usize, scroll: f32, single: bool,
    shift: bool, cmd: bool, cursor: (f64, f64), clipboard: Option<arboard::Clipboard>,
    undo: Vec<Op>, redo: Vec<Op>, doc_value: String, doc_dirty: bool,
    proxy: Option<winit::event_loop::EventLoopProxy<AkEvent>>, ax_refs: Vec<ax::LineRef>,
    seconds: f32, t0: Instant, probe: u32, probe_done: bool,
    key_recv: Option<Instant>, key_present_ms: Vec<f32>, frame_ms: Vec<f32>, draw_ms: Vec<f32>, ax_us: Vec<f32>, per_key_us: Vec<f32>, undo_us: Vec<f32>, frames: u32,
}

impl App {
    fn new_block(&mut self, text: &str) -> Block {
        let mut editor = PlainEditor::<Brush>::new(18.0);
        editor.set_text(text);
        editor.edit_styles().insert(StyleProperty::Brush(Brush::Solid(Color::from_rgb8(230, 230, 230))));
        editor.edit_styles().insert(StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(1.4)));
        Block { editor, y: 0.0, height: 0.0 }
    }
    fn load(&mut self, text: &str) {
        let parts: Vec<String> = if self.single { vec![text.to_string()] } else { text.split('\n').map(|s| s.to_string()).collect() };
        self.blocks = parts.iter().map(|p| self.new_block(p)).collect();
    }
    /// Re-measure every block (layout once each; parley caches until an edit) and stack them. O(blocks) per call, but only
    /// the edited block relayouts.
    fn reflow(&mut self, width: f32, scale: f32) {
        let mut y = 0.0f32;
        for b in self.blocks.iter_mut() {
            b.editor.set_scale(scale); b.editor.set_width(Some(width));
            let h = b.editor.layout(&mut self.font_cx, &mut self.layout_cx).height();
            b.y = y; b.height = h; y += h + 0.5 * 18.0 * 1.4 * scale;
        }
    }
    fn init(&mut self, el: &ActiveEventLoop) {
        let window = Arc::new(el.create_window(Window::default_attributes().with_title("S3 text — parley + vello").with_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0)).with_visible(false)).unwrap());
        let adapter = Adapter::with_event_loop_proxy(el, &window, self.proxy.take().expect("proxy"));
        window.set_visible(true);
        window.set_ime_allowed(true);
        let size = window.inner_size();
        let surface = pollster::block_on(self.rc.create_surface(window.clone(), size.width.max(1), size.height.max(1), wgpu::PresentMode::AutoVsync)).expect("surface");
        let dev = &self.rc.devices[surface.dev_id];
        println!("adapter: {:?}  surface format {:?}  scale {}  blocks {}  ({})", dev.adapter().get_info().name, surface.format, window.scale_factor(), self.blocks.len(), if self.single { "ONE editor" } else { "one editor per paragraph" });
        let renderer = Renderer::new(&dev.device, RendererOptions { use_cpu: false, antialiasing_support: AaSupport::area_only(), num_init_threads: NonZeroUsize::new(1), pipeline_cache: None }).expect("renderer");
        let scale = window.scale_factor() as f32;
        let t = Instant::now();
        self.reflow(size.width as f32 - 2.0 * PAD as f32 * scale, scale);
        println!("initial layout of {} blocks: {:.1} ms", self.blocks.len(), t.elapsed().as_secs_f32() * 1000.0);
        self.focus = self.blocks.len() / 2;
        self.ctx = Some(Ctx { window, surface, renderer, scene: Scene::new(), adapter });
        self.push_ax();
    }

    // ---------- editing through ops ----------
    fn apply(&mut self, block: usize, f: impl FnOnce(&mut parley::PlainEditorDriver<'_, Brush>)) {
        let before = self.blocks[block].editor.raw_text().to_string();
        let t = Instant::now();
        { let b = &mut self.blocks[block]; let mut d = b.editor.driver(&mut self.font_cx, &mut self.layout_cx); f(&mut d); }
        let _ = self.blocks[block].editor.layout(&mut self.font_cx, &mut self.layout_cx);   // the relayout is part of the key's cost
        self.per_key_us.push(t.elapsed().as_secs_f32() * 1e6);
        let after = self.blocks[block].editor.raw_text().to_string();
        if before != after {
            let p = before.bytes().zip(after.bytes()).take_while(|(a, b)| a == b).count();
            let mut p = p; while !before.is_char_boundary(p) || !after.is_char_boundary(p) { p -= 1; }
            let sfx = before[p..].bytes().rev().zip(after[p..].bytes().rev()).take_while(|(a, b)| a == b).count();
            let mut sfx = sfx; while !before.is_char_boundary(before.len() - sfx) || !after.is_char_boundary(after.len() - sfx) { sfx -= 1; }
            self.undo.push(Op { block, at: p, removed: before[p..before.len() - sfx].to_string(), inserted: after[p..after.len() - sfx].to_string() });
            self.redo.clear();
        }
        self.restack(block);
        self.doc_dirty = true;
    }
    fn restack(&mut self, from: usize) {
        let scale = self.ctx.as_ref().map(|c| c.window.scale_factor() as f32).unwrap_or(1.0);
        let h = self.blocks[from].editor.layout(&mut self.font_cx, &mut self.layout_cx).height();
        self.blocks[from].height = h;
        let gap = 0.5 * 18.0 * 1.4 * scale;
        let mut y = self.blocks[from].y + h + gap;
        for b in self.blocks.iter_mut().skip(from + 1) { b.y = y; y += b.height + gap; }
    }
    fn undo_one(&mut self, redo: bool) {
        let Some(op) = (if redo { self.redo.pop() } else { self.undo.pop() }) else { return };
        let t = Instant::now();
        let (remove, insert) = if redo { (op.removed.clone(), op.inserted.clone()) } else { (op.inserted.clone(), op.removed.clone()) };
        { let b = &mut self.blocks[op.block]; let mut d = b.editor.driver(&mut self.font_cx, &mut self.layout_cx); d.move_to_byte(op.at); d.extend_selection_to_byte(op.at + remove.len()); d.insert_or_replace_selection(&insert); }
        let _ = self.blocks[op.block].editor.layout(&mut self.font_cx, &mut self.layout_cx);
        self.undo_us.push(t.elapsed().as_secs_f32() * 1e6);
        self.focus = op.block; self.restack(op.block);
        if redo { self.undo.push(op) } else { self.redo.push(op) }
    }
    /// Enter in block mode: split the paragraph at the caret.
    fn split(&mut self) {
        if self.single { self.apply(self.focus, |d| d.insert_or_replace_selection("\n")); return }
        let at = self.blocks[self.focus].editor.raw_selection().focus().index();
        let text = self.blocks[self.focus].editor.raw_text().to_string();
        let (head, tail) = text.split_at(at);
        let (head, tail) = (head.to_string(), tail.to_string());
        self.apply(self.focus, |d| { d.move_to_byte(at); d.extend_selection_to_byte(text.len()); d.delete_selection(); let _ = &head; });
        let nb = self.new_block(&tail);
        self.blocks.insert(self.focus + 1, nb);
        self.focus += 1; let f = self.focus; self.restack(f.saturating_sub(1)); self.restack(f);
    }
    /// Backspace at the start of a block: join with the previous one.
    fn join_prev(&mut self) -> bool {
        if self.single || self.focus == 0 || self.blocks[self.focus].editor.raw_selection().focus().index() != 0 { return false }
        let tail = self.blocks[self.focus].editor.raw_text().to_string();
        self.blocks.remove(self.focus); self.focus -= 1;
        let end = self.blocks[self.focus].editor.raw_text().len();
        self.apply(self.focus, |d| { d.move_to_byte(end); d.insert_or_replace_selection(&tail); d.move_to_byte(end); });
        true
    }

    // ---------- accessibility ----------
    fn push_ax(&mut self) {
        let Some(c) = self.ctx.as_mut() else { return };
        let t = Instant::now();
        let scale = c.window.scale_factor(); let size = c.window.inner_size();
        let vis = (self.scroll as f64 / scale, (self.scroll + size.height as f32) as f64 / scale);
        let focus = self.focus; let sel = self.blocks[focus].editor.raw_selection().clone();
        let (fc, lc) = (&mut self.font_cx, &mut self.layout_cx);
        // only the blocks in view (+ the focused one) become text runs; the whole-document value is cached and rebuilt on edits
        if self.doc_dirty { let mut v = String::with_capacity(self.doc_value.len().max(1024)); for (i, b) in self.blocks.iter().enumerate() { if i > 0 { v.push('\n'); } v.push_str(b.editor.raw_text()); } self.doc_value = v; self.doc_dirty = false; }
        let (vy0, vy1) = (self.scroll - PAD as f32 * scale as f32, self.scroll + size.height as f32);
        let mut texts: Vec<(usize, String)> = vec![];
        let mut views = vec![];
        for (i, b) in self.blocks.iter_mut().enumerate() {
            if i != focus && (b.y + b.height < vy0 || b.y > vy1) { continue }
            texts.push((i, b.editor.raw_text().to_string()));
            views.push((b.editor.layout(fc, lc) as *const parley::Layout<Brush>, (PAD * scale + b.y as f64 - self.scroll as f64) / scale, i == focus));
        }
        let bviews: Vec<ax::BlockView<'_, Brush>> = views.iter().enumerate().map(|(k, (l, y, f))| ax::BlockView { layout: unsafe { &**l }, text: &texts[k].1, y: *y, focused: *f }).collect();
        let doc_value = &self.doc_value;
        let mut refs_out = None;
        c.adapter.update_if_active(|| { let (update, mut refs) = ax::tree(&bviews, &sel, PAD, scale, size, vis, doc_value); for r in refs.iter_mut() { r.block = texts[r.block].0; } refs_out = Some(refs); update });
        if let Some(r) = refs_out { self.ax_refs = r; self.ax_us.push(t.elapsed().as_secs_f32() * 1e6); }
    }

    // ---------- drawing ----------
    fn draw(&mut self) {
        let Some(c) = self.ctx.as_mut() else { return };
        let t = Instant::now();
        let dev = &self.rc.devices[c.surface.dev_id];
        let scale = c.window.scale_factor();
        let (w, h) = (c.surface.config.width, c.surface.config.height);
        // keep the caret in view
        if let Some(cb) = self.blocks[self.focus].editor.cursor_geometry(1.0) {
            let top = self.blocks[self.focus].y + cb.y0 as f32; let bottom = self.blocks[self.focus].y + cb.y1 as f32;
            let view_h = h as f32 - 2.0 * PAD as f32 * scale as f32;
            if bottom - self.scroll > view_h { self.scroll = bottom - view_h; } if top - self.scroll < 0.0 { self.scroll = top; }
        }
        c.scene.reset();
        let base = Affine::translate((PAD * scale, PAD * scale - self.scroll as f64));
        let (vis0, vis1) = (self.scroll - PAD as f32 * scale as f32, self.scroll + h as f32);
        let focus = self.focus;
        for (i, b) in self.blocks.iter_mut().enumerate() {
            if b.y + b.height < vis0 || b.y > vis1 { continue }
            let tr = base * Affine::translate((0.0, b.y as f64));
            if i == focus {
                b.editor.selection_geometry_with(|bb, _| c.scene.fill(Fill::NonZero, tr, Color::from_rgb8(60, 90, 150), None, &Rect::new(bb.x0 as f64, bb.y0 as f64, bb.x1 as f64, bb.y1 as f64)));
                if let Some(cb) = b.editor.cursor_geometry(1.5 * scale as f32) { c.scene.fill(Fill::NonZero, tr, Color::WHITE, None, &Rect::new(cb.x0 as f64, cb.y0 as f64, cb.x1 as f64, cb.y1 as f64)); }
            }
            let layout = b.editor.layout(&mut self.font_cx, &mut self.layout_cx);
            for line in layout.lines() {
                for item in line.items() {
                    let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
                    let run = gr.run(); let style = gr.style();
                    let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                    c.scene.draw_glyphs(run.font()).font_size(run.font_size()).normalized_coords(&coords).brush(&style.brush).hint(true).transform(tr)
                        .draw(Fill::NonZero, gr.positioned_glyphs().map(|g| vello::Glyph { id: g.id.into(), x: g.x, y: g.y }));
                }
            }
        }
        if let Some(cb) = self.blocks[focus].editor.cursor_geometry(1.0) {
            c.window.set_ime_cursor_area(winit::dpi::PhysicalPosition::new(PAD * scale + cb.x0 as f64, PAD * scale + (self.blocks[focus].y as f64 + cb.y0 - self.scroll as f64)), winit::dpi::PhysicalSize::new(2.0, (cb.y1 - cb.y0) as f64));
        }
        let t_scene = t.elapsed();
        c.renderer.render_to_texture(&dev.device, &dev.queue, &c.scene, &c.surface.target_view, &RenderParams { base_color: Color::from_rgb8(24, 24, 26), width: w, height: h, antialiasing_method: AaConfig::Area }).expect("render");
        let frame = match c.surface.surface.get_current_texture() { wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f, wgpu::CurrentSurfaceTexture::Occluded => { std::thread::sleep(std::time::Duration::from_millis(16)); return } other => { log::warn!("frame: {other:?}"); return } };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = dev.device.create_command_encoder(&Default::default());
        c.surface.blitter.copy(&dev.device, &mut enc, &c.surface.target_view, &view);
        dev.queue.submit([enc.finish()]);
        c.window.pre_present_notify();
        dev.queue.present(frame);
        let now = Instant::now();
        if let Some(k) = self.key_recv.take() { self.key_present_ms.push((now - k).as_secs_f32() * 1000.0); }
        self.draw_ms.push(t_scene.as_secs_f32() * 1000.0); self.frame_ms.push((now - t).as_secs_f32() * 1000.0);
        self.frames += 1;
    }

    /// `--probe N`: N keys typed into the focused block from inside the app, then undone — the per-key / undo costs.
    fn run_probe(&mut self) {
        let n = self.probe; let before = self.per_key_us.len();
        for i in 0..n { let ch = (b'a' + (i % 26) as u8) as char; let s = ch.to_string(); self.apply(self.focus, |d| d.insert_or_replace_selection(&s)); if i % 7 == 6 { self.apply(self.focus, |d| d.insert_or_replace_selection(" ")); } }
        let mut keys: Vec<f32> = self.per_key_us[before..].to_vec();
        let typed = keys.len();
        for _ in 0..typed { self.undo_one(false); }
        let mut u = self.undo_us.clone();
        println!("probe: {} keys into block {} of {} ({} chars in the block): per-key (edit + relayout) µs p50 {:.0} p95 {:.0} max {:.0}; undo µs p50 {:.0} p95 {:.0}", typed, self.focus, self.blocks.len(), self.blocks[self.focus].editor.raw_text().len(), pct(&mut keys, 50.0), pct(&mut keys, 95.0), pct(&mut keys, 100.0), pct(&mut u, 50.0), pct(&mut u, 95.0));
        self.probe_done = true;
    }
}

impl ApplicationHandler<AkEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) { if self.ctx.is_none() { self.init(el); } }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.seconds > 0.0 {
            let deadline = self.t0 + std::time::Duration::from_secs_f32(self.seconds);
            if Instant::now() >= deadline { el.exit(); return }
            el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(deadline));
        }
    }
    fn user_event(&mut self, _el: &ActiveEventLoop, event: AkEvent) {
        match event.window_event {
            AkWindowEvent::InitialTreeRequested => { println!("accesskit: initial tree requested"); self.push_ax(); }
            AkWindowEvent::ActionRequested(req) => {
                if let Some(accesskit::ActionData::SetTextSelection(sel)) = req.data {
                    if let Some((b, a, f)) = ax::selection_to_bytes(&self.ax_refs, &sel) { self.focus = b; { let bl = &mut self.blocks[b]; let mut d = bl.editor.driver(&mut self.font_cx, &mut self.layout_cx); d.move_to_byte(a); d.extend_selection_to_byte(f); } self.push_ax(); if let Some(c) = &self.ctx { c.window.request_redraw(); } }
                }
            }
            AkWindowEvent::AccessibilityDeactivated => {}
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(c) = self.ctx.as_mut() { c.adapter.process_event(&c.window, &event); }
        let mut redraw = true;
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => if let Some(c) = self.ctx.as_mut() { self.rc.resize_surface(&mut c.surface, s.width.max(1), s.height.max(1)); let sc = c.window.scale_factor() as f32; self.reflow(s.width as f32 - 2.0 * PAD as f32 * sc, sc); },
            WindowEvent::RedrawRequested => {
                self.draw(); redraw = false;
                if self.probe > 0 && !self.probe_done && self.frames > 5 { self.run_probe(); self.push_ax(); }
                if self.seconds > 0.0 && self.t0.elapsed().as_secs_f32() > self.seconds { el.exit(); return }
                if self.probe > 0 && !self.probe_done { if let Some(c) = &self.ctx { c.window.request_redraw(); } }   // otherwise: render on demand
            }
            WindowEvent::ModifiersChanged(m) => { self.shift = m.state().shift_key(); self.cmd = m.state().super_key(); redraw = false; }
            WindowEvent::CursorMoved { position, .. } => { self.cursor = (position.x, position.y); redraw = false; }
            WindowEvent::MouseWheel { delta, .. } => { let dy = match delta { MouseScrollDelta::LineDelta(_, y) => y * 40.0, MouseScrollDelta::PixelDelta(p) => p.y as f32 }; let max = self.blocks.last().map(|b| b.y + b.height).unwrap_or(0.0); self.scroll = (self.scroll - dy).clamp(0.0, max.max(0.0)); }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let scale = self.ctx.as_ref().map(|c| c.window.scale_factor()).unwrap_or(1.0);
                let (x, y) = ((self.cursor.0 - PAD * scale) as f32, (self.cursor.1 - PAD * scale) as f32 + self.scroll);
                if let Some(bi) = self.blocks.iter().position(|b| y >= b.y && y < b.y + b.height + 8.0) { self.focus = bi; }
                let by = self.blocks[self.focus].y; let shift = self.shift;
                { let b = &mut self.blocks[self.focus]; let mut d = b.editor.driver(&mut self.font_cx, &mut self.layout_cx); if shift { d.extend_selection_to_point(x, y - by) } else { d.move_to_point(x, y - by) } }
            }
            WindowEvent::Ime(ime) => {
                println!("ime: {ime:?}");
                let f = self.focus;
                match ime {
                    Ime::Preedit(text, cursor) => { let b = &mut self.blocks[f]; let mut d = b.editor.driver(&mut self.font_cx, &mut self.layout_cx); if text.is_empty() { d.clear_compose() } else { d.set_compose(&text, cursor) } }
                    Ime::Commit(text) => self.apply(f, |d| d.insert_or_replace_selection(&text)),
                    Ime::Enabled | Ime::Disabled => {}
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                self.key_recv = Some(Instant::now());
                let (shift, cmd, f) = (self.shift, self.cmd, self.focus);
                let (at_first_line, at_last_line) = { let b = &mut self.blocks[f]; let i = b.editor.raw_selection().focus().index(); let empty = b.editor.raw_text().is_empty();
                    let (first_end, last_start) = { let l = b.editor.layout(&mut self.font_cx, &mut self.layout_cx); (l.lines().next().map(|x| x.text_range().end), l.lines().last().map(|x| x.text_range().start)) };
                    (first_end.map(|e| i < e || empty).unwrap_or(true), last_start.map(|st| i >= st).unwrap_or(true)) };
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Named(NamedKey::ArrowLeft) => self.apply(f, |d| if shift { d.select_left() } else { d.move_left() }),
                    Key::Named(NamedKey::ArrowRight) => self.apply(f, |d| if shift { d.select_right() } else { d.move_right() }),
                    Key::Named(NamedKey::ArrowUp) => if at_first_line && f > 0 && !shift { self.focus -= 1; let g = self.focus; let end = self.blocks[g].editor.raw_text().len(); self.apply(g, |d| d.move_to_byte(end)); } else { self.apply(f, |d| if shift { d.select_up() } else { d.move_up() }) },
                    Key::Named(NamedKey::ArrowDown) => if at_last_line && f + 1 < self.blocks.len() && !shift { self.focus += 1; let g = self.focus; self.apply(g, |d| d.move_to_byte(0)); } else { self.apply(f, |d| if shift { d.select_down() } else { d.move_down() }) },
                    Key::Named(NamedKey::Home) => self.apply(f, |d| if shift { d.select_to_line_start() } else { d.move_to_line_start() }),
                    Key::Named(NamedKey::End) => self.apply(f, |d| if shift { d.select_to_line_end() } else { d.move_to_line_end() }),
                    Key::Named(NamedKey::Backspace) => { if !self.join_prev() { self.apply(f, |d| d.backdelete()) } }
                    Key::Named(NamedKey::Delete) => self.apply(f, |d| d.delete()),
                    Key::Named(NamedKey::Enter) => self.split(),
                    Key::Character(c) if cmd => match c.as_str() {
                        "z" => self.undo_one(shift),
                        "c" | "x" => { if let Some(t) = self.blocks[f].editor.selected_text().map(|s| s.to_string()) { if let Some(cb) = self.clipboard.as_mut() { cb.set_text(t).ok(); } if c == "x" { self.apply(f, |d| d.delete_selection()); } } }
                        "v" => { if let Some(t) = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()) { self.apply(f, |d| d.insert_or_replace_selection(&t)); } }
                        "a" => { let end = self.blocks[f].editor.raw_text().len(); self.apply(f, |d| { d.move_to_byte(0); d.extend_selection_to_byte(end); }) }
                        _ => redraw = false,
                    },
                    _ => if let Some(t) = event.text.as_ref() { if t.chars().any(|ch| ch.is_control()) { redraw = false } else { let t = t.to_string(); self.apply(f, |d| d.insert_or_replace_selection(&t)) } } else { redraw = false },
                }
            }
            _ => redraw = false,
        }
        if redraw { self.push_ax(); if let Some(c) = &self.ctx { c.window.request_redraw(); } }
    }
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let has = |k: &str| args.iter().any(|a| a == k);
    let lines: usize = arg("--lines").and_then(|v| v.parse().ok()).unwrap_or(0);
    let text = if lines > 0 { generate(lines) } else { "S3 text — type here. Shift+arrows select, click places the caret, IME preedit draws underlined.\n\nThe long pole of min-ui is this editor: parley + vello + IME + AccessKit on one node tree.".to_string() };
    #[cfg(target_os = "macos")]
    unsafe { objc2_foundation::NSUserDefaults::standardUserDefaults().setBool_forKey(true, objc2_foundation::ns_string!("ApplePressAndHoldEnabled")); }
    let el = EventLoop::<AkEvent>::with_user_event().build().unwrap();
    let proxy = el.create_proxy();
    let mut app = App { rc: RenderContext::new(), ctx: None, font_cx: FontContext::new(), layout_cx: LayoutContext::new(), blocks: vec![], focus: 0, scroll: 0.0, single: has("--single"),
        shift: false, cmd: false, cursor: (0.0, 0.0), clipboard: arboard::Clipboard::new().ok(), undo: vec![], redo: vec![], doc_value: String::new(), doc_dirty: true, proxy: Some(proxy), ax_refs: vec![],
        seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), t0: Instant::now(), probe: arg("--probe").and_then(|v| v.parse().ok()).unwrap_or(0), probe_done: false,
        key_recv: None, key_present_ms: vec![], frame_ms: vec![], draw_ms: vec![], ax_us: vec![], per_key_us: vec![], undo_us: vec![], frames: 0 };
    app.load(&text);
    el.run_app(&mut app).unwrap();
    let (mut kp, mut fm, mut dm, mut au, mut pk) = (app.key_present_ms.clone(), app.frame_ms.clone(), app.draw_ms.clone(), app.ax_us.clone(), app.per_key_us.clone());
    println!("frames {}: frame ms (scene + render + present, CPU) p50 {:.2} p95 {:.2} max {:.2}; scene build ms p50 {:.2} p95 {:.2}; key→present ms p50 {:.2} p95 {:.2} (n={}); per-key µs (all keys) p50 {:.0} p95 {:.0}; accesskit tree µs p50 {:.0} p95 {:.0} max {:.0}; undo depth {}",
        app.frames, pct(&mut fm, 50.0), pct(&mut fm, 95.0), pct(&mut fm, 100.0), pct(&mut dm, 50.0), pct(&mut dm, 95.0), pct(&mut kp, 50.0), pct(&mut kp, 95.0), kp.len(), pct(&mut pk, 50.0), pct(&mut pk, 95.0), pct(&mut au, 50.0), pct(&mut au, 95.0), pct(&mut au, 100.0), app.undo.len());
    println!("focused block {} text: {:?}", app.focus, app.blocks[app.focus].editor.raw_text());
}
