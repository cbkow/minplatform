//! S5 window — winit 0.31 (typed data transfer in + drag-out, clipboard), wgpu, nested drop ZONES with innermost-wins hit
//! testing, two windows on one loop (`--dual` / `D`), fullscreen (`F`), HiDPI reporting, a native menu bar (objc2).
//! Every observation prints as `s5: …`. docs/spikes/S5-window.md. `--seconds N` exits.
use std::{collections::HashMap, sync::Arc, time::Instant};
use winit::application::ApplicationHandler;
use winit::data_transfer::{DataTransferId, DataTransferSendBuilder, TypeHint};
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, DndAction, DragIcon, EventLoop};
use winit::icon::RgbaIcon;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};
use winit::monitor::Fullscreen;
use winit::data_transfer::SendData;
use winit::dpi::{LogicalSize, LogicalPosition, PhysicalPosition};

#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct U { size: [f32; 2], hot: f32, n: f32, rects: [[f32; 4]; 4], colors: [[f32; 4]; 4] }

const ZONES: [(&str, [f32; 4]); 4] = [("window", [0.16, 0.18, 0.22, 1.0]), ("panel", [0.20, 0.30, 0.42, 1.0]), ("cell", [0.30, 0.46, 0.34, 1.0]), ("source", [0.55, 0.35, 0.20, 1.0])];

struct Win { window: Arc<Box<dyn Window>>, surface: wgpu::Surface<'static>, config: wgpu::SurfaceConfiguration, ubuf: wgpu::Buffer, bind: wgpu::BindGroup, hot: i32, pointer: PhysicalPosition<f64>, last_drag_pos: Option<PhysicalPosition<f64>>, label: &'static str }

impl Win {
    /// Zone rects in physical px: window, panel (inset), cell (inset further), source (bottom-right box).
    fn rects(&self) -> [[f32; 4]; 4] {
        let s = self.window.scale_factor() as f32; let (w, h) = (self.config.width as f32, self.config.height as f32);
        [[0.0, 0.0, w, h], [60.0 * s, 60.0 * s, w - 60.0 * s, h - 60.0 * s], [180.0 * s, 150.0 * s, w - 180.0 * s, h - 150.0 * s], [w - 170.0 * s, h - 110.0 * s, w - 30.0 * s, h - 30.0 * s]]
    }
    /// Innermost zone under a point (the §14.4 rule): the last matching rect in nesting order; `source` is checked first.
    fn zone_at(&self, p: PhysicalPosition<f64>) -> Option<usize> {
        let r = self.rects(); let (x, y) = (p.x as f32, p.y as f32);
        let hit = |k: usize| x >= r[k][0] && y >= r[k][1] && x < r[k][2] && y < r[k][3];
        if hit(3) { return Some(3) } (0..3).rev().find(|&k| hit(k))
    }
    fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pipeline: &wgpu::RenderPipeline) {
        let u = U { size: [self.config.width as f32, self.config.height as f32], hot: self.hot as f32, n: 4.0, rects: self.rects(), colors: [ZONES[0].1, ZONES[1].1, ZONES[2].1, ZONES[3].1] };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let frame = match self.surface.get_current_texture() { wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f, wgpu::CurrentSurfaceTexture::Occluded => return, other => { log::warn!("{other:?}"); self.surface.configure(device, &self.config); return } };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&Default::default());
        { let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
          pass.set_pipeline(pipeline); pass.set_bind_group(0, &self.bind, &[]); pass.draw(0..3, 0..1); }
        queue.submit([enc.finish()]);
        self.window.pre_present_notify();
        queue.present(frame);
    }
}

struct Gpu { instance: wgpu::Instance, device: wgpu::Device, queue: wgpu::Queue, pipeline: wgpu::RenderPipeline, layout: wgpu::BindGroupLayout, format: wgpu::TextureFormat }

#[derive(Default)]
struct App {
    gpu: Option<Gpu>, wins: HashMap<WindowId, Win>, dual: bool, seconds: f32, t0: Option<Instant>,
    fetches: HashMap<AsyncRequestSerial, (WindowId, DataTransferId, &'static str)>, outgoing: Option<DataTransferId>, incoming: Option<DataTransferId>,
    temp_files: Vec<std::path::PathBuf>, drags_in: u32, drops: u32, received: u32,
}

impl App {
    fn open(&mut self, el: &dyn ActiveEventLoop, label: &'static str, pos: (f64, f64)) {
        let attrs = WindowAttributes::default().with_title(format!("S5 window — {label}")).with_surface_size(LogicalSize::new(720.0, 480.0)).with_position(LogicalPosition::new(pos.0, pos.1));
        let window: Arc<Box<dyn Window>> = Arc::new(el.create_window(attrs).expect("window"));
        if self.gpu.is_none() {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let surface = instance.create_surface(window.clone()).expect("surface");
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false, apply_limit_buckets: false })).expect("adapter");
            let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("s5"), ..Default::default() })).expect("device");
            let format = surface.get_capabilities(&adapter).formats[0];
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(include_str!("zones.wgsl").into()) });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pl), vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() }, fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }), primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None });
            println!("s5: adapter {} backend {:?} format {:?}", adapter.get_info().name, adapter.get_info().backend, format);
            self.gpu = Some(Gpu { instance, device, queue, pipeline, layout, format });
            drop(surface);
        }
        let g = self.gpu.as_ref().unwrap();
        let surface = g.instance.create_surface(window.clone()).expect("surface");
        let size = window.surface_size();
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format: g.format, color_space: wgpu::SurfaceColorSpace::Auto, width: size.width.max(1), height: size.height.max(1), present_mode: wgpu::PresentMode::AutoVsync, alpha_mode: wgpu::CompositeAlphaMode::Auto, view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&g.device, &config);
        let ubuf = g.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<U>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let bind = g.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &g.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }] });
        let mon = window.current_monitor().and_then(|m| m.name().map(|n| n.to_string()));
        println!("s5: window '{label}' id {:?} scale {} surface {}x{} monitor {:?} outer {:?}", window.id(), window.scale_factor(), size.width, size.height, mon, window.outer_position().ok());
        let id = window.id();
        window.request_redraw();
        self.wins.insert(id, Win { window, surface, config, ubuf, bind, hot: -1, pointer: PhysicalPosition::new(0.0, 0.0), last_drag_pos: None, label });
    }

    fn start_drag_out(&mut self, el: &dyn ActiveEventLoop, id: WindowId) {
        // three REAL temp files (outside any repo) — §14.2: one file-url per item, CRLF uri-list on Windows is winit's job
        let dir = std::env::temp_dir().join("minplatform-s5");
        std::fs::create_dir_all(&dir).ok();
        let files: Vec<std::path::PathBuf> = (0..3).map(|i| { let p = dir.join(format!("s5 drag {i}.txt")); std::fs::write(&p, format!("S5 drag-out file {i}\n")).ok(); p }).collect();
        let uris: Vec<String> = files.iter().map(|p| format!("file://{}", p.to_string_lossy().replace(' ', "%20"))).collect();
        self.temp_files = files;
        let icon: Vec<u8> = (0..32 * 32).flat_map(|_| [230u8, 140, 60, 255]).collect();
        let icon = RgbaIcon::new(icon, 32, 32).unwrap().into();
        let send = DataTransferSendBuilder::new(uris.clone())
            .with_type(TypeHint::UriList, |u: &Vec<String>, _| Some(SendData::Uris(u.clone())))
            .with_type(TypeHint::Plaintext, |u, _| Some(format!("minplatform://s5?files={}", u.len())))
            .build();
        eprintln!("s5: calling start_drag…"); let t = Instant::now();
        let r = el.start_drag(id, send, &[DndAction::Copy], Some(DragIcon { icon, offset_x: -16, offset_y: -16 }));
        eprintln!("s5: start_drag returned after {:.0} ms", t.elapsed().as_secs_f32() * 1000.0);
        match r {
            Ok(tid) => { println!("s5: drag-out started {tid:?} with {} file URIs (Copy only)", uris.len()); self.outgoing = Some(tid); }
            Err(e) => println!("s5: drag-out FAILED: {e}"),
        }
    }
}

impl App { fn finish(&self) { println!("s5: done — drags in {} drops {} received {}; temp files {}", self.drags_in, self.drops, self.received, self.temp_files.len()); } }

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.wins.is_empty() {
            self.open(el, "A", (120.0, 120.0));
            if self.dual { self.open(el, "B", (900.0, 120.0)); }
            #[cfg(target_os = "macos")] mac::menu_bar();
            self.t0 = Some(Instant::now());
        }
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if self.seconds > 0.0 { if let Some(t0) = self.t0 { let dl = t0 + std::time::Duration::from_secs_f32(self.seconds); if Instant::now() >= dl { self.finish(); el.exit(); return } el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(dl)); } }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(g) = self.gpu.as_ref() else { return };
        let (device, queue, pipeline) = (&g.device, &g.queue, &g.pipeline);
        match event {
            WindowEvent::CloseRequested => { self.wins.remove(&id); if self.wins.is_empty() { self.finish(); el.exit(); } }
            WindowEvent::SurfaceResized(s) => if let Some(w) = self.wins.get_mut(&id) { w.config.width = s.width.max(1); w.config.height = s.height.max(1); w.surface.configure(device, &w.config); println!("s5: '{}' resized {}x{} (scale {})", w.label, s.width, s.height, w.window.scale_factor()); w.window.request_redraw(); },
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => if let Some(w) = self.wins.get(&id) { println!("s5: '{}' SCALE CHANGED → {scale_factor} (monitor {:?})", w.label, w.window.current_monitor().and_then(|m| m.name().map(|n| n.to_string()))); },
            WindowEvent::Moved(p) => if let Some(w) = self.wins.get(&id) { println!("s5: '{}' moved to {:?} on {:?} scale {}", w.label, p, w.window.current_monitor().and_then(|m| m.name().map(|n| n.to_string())), w.window.scale_factor()); },
            WindowEvent::RedrawRequested => if let Some(w) = self.wins.get_mut(&id) { w.draw(device, queue, pipeline); },
            WindowEvent::Focused(f) => { if let Some(w) = self.wins.get(&id) { println!("s5: '{}' focused {f}{}", w.label, if !f && self.outgoing.is_some() { " (during an outgoing drag)" } else { "" }); } }
            WindowEvent::PointerMoved { position, .. } => if let Some(w) = self.wins.get_mut(&id) { w.pointer = position; let z = w.zone_at(position).map(|z| z as i32).unwrap_or(-1); if z != w.hot { w.hot = z; w.window.request_redraw(); } },
            WindowEvent::PointerButton { state: ElementState::Pressed, button: ButtonSource::Mouse(MouseButton::Left), position, .. } => {
                let zone = self.wins.get(&id).and_then(|w| w.zone_at(position));
                println!("s5: press in '{}' at {:?} → zone {:?}", self.wins[&id].label, position, zone.map(|z| ZONES[z].0));
                if zone == Some(3) { self.start_drag_out(el, id); }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => match &event.logical_key {
                Key::Named(NamedKey::Escape) => { self.finish(); el.exit() }
                Key::Character(c) if c == "f" => { let w = &self.wins[&id]; let fs = w.window.fullscreen(); println!("s5: fullscreen {} → {}", fs.is_some(), fs.is_none()); w.window.set_fullscreen(if fs.is_some() { None } else { Some(Fullscreen::Borderless(None)) }); }
                Key::Character(c) if c == "d" => { let n = self.wins.len(); if n < 2 { self.open(el, "B", (900.0, 120.0)); } }
                Key::Character(c) if c == "c" => { let send = DataTransferSendBuilder::new("S5 clipboard text — srt://example.invalid:9000 stays as text".to_string()).with_type(TypeHint::Plaintext, |t: &String, _| Some(t.clone())).build(); println!("s5: set_clipboard → {:?}", el.set_clipboard(send).map_err(|e| e.to_string())); }
                Key::Character(c) if c == "v" => match el.clipboard() { Ok(Some(tid)) => { let dt = el.data_transfer(tid).unwrap(); println!("s5: clipboard types {:?}", dt.available_types()); if dt.has_type(&TypeHint::UriList) { if let Ok(s) = el.fetch_data_transfer(tid, &TypeHint::UriList) { self.fetches.insert(s, (id, tid, "clipboard")); } } else if dt.has_type(&TypeHint::Plaintext) { if let Ok(s) = el.fetch_data_transfer(tid, &TypeHint::Plaintext) { self.fetches.insert(s, (id, tid, "clipboard")); } } } other => println!("s5: clipboard: {other:?}") },
                _ => {}
            },
            // ---------- drag and drop IN ----------
            WindowEvent::DragEntered { id: tid, position } => {
                self.drags_in += 1; self.incoming = Some(tid);
                let dt = match el.data_transfer(tid) { Ok(d) => d, Err(e) => { println!("s5: data_transfer({tid:?}) failed: {e}"); return } };
                let types: Vec<String> = dt.available_types().iter().map(|t| format!("{t:?}")).collect();
                let selfdrop = self.outgoing == Some(tid);
                println!("s5: DRAG ENTERED '{}' {tid:?} at {:?}{} — types {:?}", self.wins[&id].label, position, if selfdrop { " [OUR OWN DRAG — drop-onto-self]" } else { "" }, types);
                let want = [TypeHint::UriList, TypeHint::Plaintext, TypeHint::Html].into_iter().find(|t| dt.has_type(t));
                match want {
                    Some(t) => { el.set_valid_dnd_actions(tid, &[DndAction::Copy, DndAction::Move]).ok(); match el.fetch_data_transfer(tid, &t) { Ok(s) => { self.fetches.insert(s, (id, tid, "drag")); } Err(e) => println!("s5: fetch failed: {e}") } }
                    None => { el.set_valid_dnd_actions(tid, &[]).ok(); println!("s5: REFUSED (no usable type) — the OS shows the refusal"); }
                }
            }
            WindowEvent::DragPosition { id: tid, position, proposed_action } => if let Some(w) = self.wins.get_mut(&id) {
                w.last_drag_pos = Some(position);
                let z = w.zone_at(position).map(|z| z as i32).unwrap_or(-1);
                if z != w.hot { w.hot = z; w.window.request_redraw(); println!("s5: drag {tid:?} over '{}' zone {:?} proposed {proposed_action:?}", w.label, z.try_into().ok().and_then(|k: usize| ZONES.get(k)).map(|z| z.0)); }
                // the cell zone refuses Move: actions per zone at hover time
                if z == 2 { el.set_valid_dnd_actions(tid, &[DndAction::Copy]).ok(); } else if z >= 0 { el.set_valid_dnd_actions(tid, &[DndAction::Copy, DndAction::Move]).ok(); }
            },
            WindowEvent::DragDropped { id: tid, proposed_action } => {
                self.drops += 1;
                let w = self.wins.get_mut(&id).unwrap();
                // §14.2: recompute the target from the drop point — winit gives no point on DragDropped, so the last DragPosition stands in
                let zone = w.last_drag_pos.and_then(|p| w.zone_at(p));
                println!("s5: DROPPED {tid:?} on '{}' zone {:?} at {:?} action {proposed_action:?}{}", w.label, zone.map(|z| ZONES[z].0), w.last_drag_pos, if self.outgoing == Some(tid) { " [onto self]" } else { "" });
                w.hot = -1; w.window.request_redraw();
            }
            WindowEvent::DragLeft { id: tid } => if let Some(w) = self.wins.get_mut(&id) { println!("s5: drag {tid:?} left '{}'", w.label); w.hot = -1; w.window.request_redraw(); },
            WindowEvent::DataTransferReceived { id: tid, serial, value } => {
                self.received += 1;
                let what = self.fetches.remove(&serial).map(|f| f.2).unwrap_or("?");
                match value.type_().hint() {
                    Some(TypeHint::UriList) => match value.try_as_uris() { Ok(u) => { println!("s5: RECEIVED ({what}) {tid:?} {} URIs:", u.len()); for x in &u { println!("s5:    {x}"); } } Err(e) => println!("s5: uris error {e}") },
                    Some(TypeHint::Plaintext) | Some(TypeHint::Html) => println!("s5: RECEIVED ({what}) {tid:?} text {:?}", value.try_as_string().unwrap_or_default()),
                    other => println!("s5: RECEIVED ({what}) {tid:?} {other:?} {} bytes", value.try_as_bytes().map(|b| b.len()).unwrap_or(0)),
                }
            }
            // ---------- drag OUT ----------
            WindowEvent::OutgoingDragDropped { id: tid, action } => { println!("s5: OUTGOING drag {tid:?} dropped with action {action:?}"); self.outgoing = None; }
            WindowEvent::OutgoingDragCanceled { id: tid } => { println!("s5: OUTGOING drag {tid:?} cancelled"); self.outgoing = None; }
            _ => {}
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::NSString;
    /// A native menu bar: S5 › Toggle Full Screen (AppKit's own `toggleFullScreen:` through the responder chain) · Quit.
    pub fn menu_bar() {
        let mtm = MainThreadMarker::new().unwrap();
        let app = NSApplication::sharedApplication(mtm);
        let bar = NSMenu::new(mtm);
        let app_item = NSMenuItem::new(mtm);
        let app_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("S5"));
        unsafe {
            let fs = NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str("Toggle Full Screen"), Some(objc2::sel!(toggleFullScreen:)), &NSString::from_str("f"));
            let quit = NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str("Quit S5"), Some(objc2::sel!(terminate:)), &NSString::from_str("q"));
            app_menu.addItem(&fs); app_menu.addItem(&NSMenuItem::separatorItem(mtm)); app_menu.addItem(&quit);
        }
        app_item.setSubmenu(Some(&app_menu));
        bar.addItem(&app_item);
        app.setMainMenu(Some(&bar));
        println!("s5: native menu bar installed (S5 › Toggle Full Screen ⌘F · Quit ⌘Q)");
    }
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let el = EventLoop::new().unwrap();
    let app = App { dual: args.iter().any(|a| a == "--dual"), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), ..Default::default() };
    el.run_app(app).unwrap();
}
