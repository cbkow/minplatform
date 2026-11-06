//! S3 text — a parley `PlainEditor` drawn by vello in a winit window: typing, backspace/delete, arrows, shift-selection,
//! click-to-place, IME preedit + commit (the macOS marked-text path), and an ACCESSKIT tree (step 1: a window → multiline
//! text input → one text run per laid-out line, with per-character geometry and the live selection; VoiceOver / the AX API
//! read it). `--seconds N` exits; `--select a-b` sets a byte selection at start (for the AX self-check). docs/spikes/S3-text.md.
mod ax;
use std::{num::NonZeroUsize, sync::Arc, time::Instant};
use accesskit_winit::{Adapter, Event as AkEvent, WindowEvent as AkWindowEvent};
use parley::{FontContext, LayoutContext, PlainEditor, PositionedLayoutItem, StyleProperty};
use vello::{kurbo::{Affine, Rect}, peniko::{Brush, Color, Fill}, util::{RenderContext, RenderSurface}, AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use winit::{application::ApplicationHandler, event::{ElementState, Ime, MouseButton, WindowEvent}, event_loop::{ActiveEventLoop, EventLoop}, keyboard::{Key, NamedKey}, window::{Window, WindowId}};

const SAMPLE: &str = "S3 text — type here. Shift+arrows select, click places the caret, IME preedit draws underlined.\n\nThe long pole of min-ui is this editor: parley + vello + IME + AccessKit on one node tree.";

struct Ctx {
    window: Arc<Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
    scene: Scene,
    adapter: Adapter,
}

struct App {
    rc: RenderContext,
    ctx: Option<Ctx>,
    font_cx: FontContext,
    layout_cx: LayoutContext<Brush>,
    editor: PlainEditor<Brush>,
    shift: bool,
    cursor: (f64, f64),
    frames: u32,
    proxy: Option<winit::event_loop::EventLoopProxy<AkEvent>>,
    seconds: f32, t0: Instant, select: Option<(usize, usize)>, ax_updates: u32, ax_update_us: Vec<f32>,
}

impl App {
    fn init(&mut self, el: &ActiveEventLoop) {
        // the AccessKit adapter must exist before the window is first shown
        let window = Arc::new(el.create_window(Window::default_attributes().with_title("S3 text — parley + vello").with_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0)).with_visible(false)).unwrap());
        let adapter = Adapter::with_event_loop_proxy(el, &window, self.proxy.take().expect("proxy"));
        window.set_visible(true);
        window.set_ime_allowed(true);
        let size = window.inner_size();
        let surface = pollster::block_on(self.rc.create_surface(window.clone(), size.width.max(1), size.height.max(1), wgpu::PresentMode::AutoVsync)).expect("surface");
        let dev = &self.rc.devices[surface.dev_id];
        println!("adapter: {:?}  surface format {:?}  scale {}", dev.adapter().get_info().name, surface.format, window.scale_factor());
        let renderer = Renderer::new(&dev.device, RendererOptions { use_cpu: false, antialiasing_support: AaSupport::area_only(), num_init_threads: NonZeroUsize::new(1), pipeline_cache: None }).expect("renderer");
        self.editor.set_scale(window.scale_factor() as f32);
        self.editor.set_width(Some(size.width as f32 - 2.0 * 32.0 * window.scale_factor() as f32));
        if let Some((a, b)) = self.select { let mut d = self.editor.driver(&mut self.font_cx, &mut self.layout_cx); d.move_to_byte(a); d.extend_selection_to_byte(b); }
        self.ctx = Some(Ctx { window, surface, renderer, scene: Scene::new(), adapter });
        self.push_ax();
    }
    /// Rebuild the whole AccessKit tree from the editor (the proxy path needs a full tree per update) and push it if a client listens.
    fn push_ax(&mut self) {
        let Some(c) = self.ctx.as_mut() else { return };
        let t = Instant::now();
        let scale = c.window.scale_factor();
        let text = self.editor.raw_text().to_string(); let sel = self.editor.raw_selection().clone(); let size = c.window.inner_size();
        let layout = self.editor.layout(&mut self.font_cx, &mut self.layout_cx);
        let update = ax::tree(layout, &text, &sel, 32.0, 32.0, scale, size);
        c.adapter.update_if_active(|| update);
        self.ax_updates += 1; self.ax_update_us.push(t.elapsed().as_secs_f32() * 1e6);
    }
    fn draw(&mut self) {
        let Some(c) = self.ctx.as_mut() else { return };
        let dev = &self.rc.devices[c.surface.dev_id];
        let scale = c.window.scale_factor();
        let pad = Affine::translate((32.0 * scale, 32.0 * scale));
        c.scene.reset();
        // selection + caret
        self.editor.selection_geometry_with(|bb, _| c.scene.fill(Fill::NonZero, pad, Color::from_rgb8(60, 90, 150), None, &Rect::new(bb.x0 as f64, bb.y0 as f64, bb.x1 as f64, bb.y1 as f64)));
        if let Some(cb) = self.editor.cursor_geometry(1.5 * scale as f32) {
            c.scene.fill(Fill::NonZero, pad, Color::WHITE, None, &Rect::new(cb.x0 as f64, cb.y0 as f64, cb.x1 as f64, cb.y1 as f64));
        }
        let layout = self.editor.layout(&mut self.font_cx, &mut self.layout_cx);
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
                let run = gr.run(); let style = gr.style();
                let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                c.scene.draw_glyphs(run.font()).font_size(run.font_size()).normalized_coords(&coords).brush(&style.brush).hint(true).transform(pad)
                    .draw(Fill::NonZero, gr.positioned_glyphs().map(|g| vello::Glyph { id: g.id.into(), x: g.x, y: g.y }));
            }
        }
        // IME cursor area so the candidate window follows the caret
        if let Some(cb) = self.editor.cursor_geometry(1.0) {
            c.window.set_ime_cursor_area(winit::dpi::PhysicalPosition::new(32.0 * scale + cb.x0 as f64, 32.0 * scale + cb.y0 as f64), winit::dpi::PhysicalSize::new(2.0, (cb.y1 - cb.y0) as f64));
        }
        let w = c.surface.config.width; let h = c.surface.config.height;
        c.renderer.render_to_texture(&dev.device, &dev.queue, &c.scene, &c.surface.target_view, &RenderParams { base_color: Color::from_rgb8(24, 24, 26), width: w, height: h, antialiasing_method: AaConfig::Area }).expect("render");
        let frame = match c.surface.surface.get_current_texture() { wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f, other => { log::warn!("frame: {other:?}"); return } };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = dev.device.create_command_encoder(&Default::default());
        c.surface.blitter.copy(&dev.device, &mut enc, &c.surface.target_view, &view);
        dev.queue.submit([enc.finish()]);
        c.window.pre_present_notify();
        dev.queue.present(frame);
        self.frames += 1;
    }
}

impl ApplicationHandler<AkEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) { if self.ctx.is_none() { self.init(el); } }
    fn user_event(&mut self, _el: &ActiveEventLoop, event: AkEvent) {
        match event.window_event {
            AkWindowEvent::InitialTreeRequested => { println!("accesskit: initial tree requested"); self.push_ax(); }
            AkWindowEvent::ActionRequested(req) => {
                println!("accesskit: action {:?} on {:?}", req.action, req.target_node);
                if let Some(accesskit::ActionData::SetTextSelection(sel)) = req.data {
                    println!("accesskit: SetTextSelection anchor {:?}/{} focus {:?}/{}", sel.anchor.node, sel.anchor.character_index, sel.focus.node, sel.focus.character_index);
                    let text = self.editor.raw_text().to_string();
                    let layout = self.editor.layout(&mut self.font_cx, &mut self.layout_cx);
                    if let Some((a, f)) = ax::selection_to_bytes(layout, &text, &sel) { let mut d = self.editor.driver(&mut self.font_cx, &mut self.layout_cx); d.move_to_byte(a); d.extend_selection_to_byte(f); self.push_ax(); if let Some(c) = &self.ctx { c.window.request_redraw(); } }
                }
            }
            AkWindowEvent::AccessibilityDeactivated => println!("accesskit: deactivated"),
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(c) = self.ctx.as_mut() { c.adapter.process_event(&c.window, &event); }
        let mut redraw = true;
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => if let Some(c) = self.ctx.as_mut() {
                self.rc.resize_surface(&mut c.surface, s.width.max(1), s.height.max(1));
                self.editor.set_width(Some(s.width as f32 - 64.0 * c.window.scale_factor() as f32));
            },
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.editor.set_scale(scale_factor as f32),
            WindowEvent::RedrawRequested => { self.draw(); redraw = false; if self.seconds > 0.0 && self.t0.elapsed().as_secs_f32() > self.seconds { el.exit(); return } if let Some(c) = &self.ctx { c.window.request_redraw(); } }
            WindowEvent::ModifiersChanged(m) => self.shift = m.state().shift_key(),
            WindowEvent::CursorMoved { position, .. } => { self.cursor = (position.x, position.y); redraw = false; }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let scale = self.ctx.as_ref().map(|c| c.window.scale_factor()).unwrap_or(1.0);
                let (x, y) = ((self.cursor.0 - 32.0 * scale) as f32, (self.cursor.1 - 32.0 * scale) as f32);
                let mut d = self.editor.driver(&mut self.font_cx, &mut self.layout_cx);
                if self.shift { d.extend_selection_to_point(x, y) } else { d.move_to_point(x, y) }
            }
            WindowEvent::Ime(ime) => {
                let mut d = self.editor.driver(&mut self.font_cx, &mut self.layout_cx);
                println!("ime: {ime:?}");
                match ime {
                    Ime::Preedit(text, cursor) => if text.is_empty() { d.clear_compose() } else { d.set_compose(&text, cursor) },
                    Ime::Commit(text) => d.insert_or_replace_selection(&text),
                    Ime::Enabled | Ime::Disabled => {}
                }
            }
            WindowEvent::KeyboardInput { event, is_synthetic, .. } if event.state.is_pressed() => {
                println!("key: {:?} text {:?} repeat {} synthetic {}", event.logical_key, event.text, event.repeat, is_synthetic);
                let shift = self.shift;
                let mut d = self.editor.driver(&mut self.font_cx, &mut self.layout_cx);
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Named(NamedKey::ArrowLeft) => if shift { d.select_left() } else { d.move_left() },
                    Key::Named(NamedKey::ArrowRight) => if shift { d.select_right() } else { d.move_right() },
                    Key::Named(NamedKey::ArrowUp) => if shift { d.select_up() } else { d.move_up() },
                    Key::Named(NamedKey::ArrowDown) => if shift { d.select_down() } else { d.move_down() },
                    Key::Named(NamedKey::Home) => if shift { d.select_to_line_start() } else { d.move_to_line_start() },
                    Key::Named(NamedKey::End) => if shift { d.select_to_line_end() } else { d.move_to_line_end() },
                    Key::Named(NamedKey::Backspace) => d.backdelete(),
                    Key::Named(NamedKey::Delete) => d.delete(),
                    Key::Named(NamedKey::Enter) => d.insert_or_replace_selection("\n"),
                    _ => if let Some(t) = event.text.as_ref() { if !event.state.is_pressed() || t.chars().any(|ch| ch.is_control()) { redraw = false } else { d.insert_or_replace_selection(t) } } else { redraw = false },
                }
            }
            _ => redraw = false,
        }
        if redraw { self.push_ax(); if let Some(c) = &self.ctx { c.window.request_redraw(); } }
    }
}

fn main() {
    env_logger::init();
    let mut editor = PlainEditor::<Brush>::new(18.0);
    editor.set_text(SAMPLE);
    editor.edit_styles().insert(StyleProperty::Brush(Brush::Solid(Color::from_rgb8(230, 230, 230))));
    editor.edit_styles().insert(StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(1.4)));
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let seconds = arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let select = arg("--select").and_then(|v| v.split_once('-').map(|(a, b)| (a.parse().unwrap(), b.parse().unwrap())));
    // macOS press-and-hold: an app without a bundle (this spike) must opt in through its own NSUserDefaults domain (zapfast #723)
    #[cfg(target_os = "macos")]
    unsafe { objc2_foundation::NSUserDefaults::standardUserDefaults().setBool_forKey(true, objc2_foundation::ns_string!("ApplePressAndHoldEnabled")); }
    let el = EventLoop::<AkEvent>::with_user_event().build().unwrap();
    let proxy = el.create_proxy();
    let mut app = App { rc: RenderContext::new(), ctx: None, font_cx: FontContext::new(), layout_cx: LayoutContext::new(), editor, shift: false, cursor: (0.0, 0.0), frames: 0, proxy: Some(proxy), seconds, t0: Instant::now(), select, ax_updates: 0, ax_update_us: vec![] };
    el.run_app(&mut app).unwrap();
    let mut us = app.ax_update_us.clone(); us.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("frames: {}  accesskit tree builds: {} (µs p50 {:.0} max {:.0})  final text: {:?}", app.frames, app.ax_updates, us.get(us.len() / 2).copied().unwrap_or(0.0), us.last().copied().unwrap_or(0.0), app.editor.text().to_string());
}
