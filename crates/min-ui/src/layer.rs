//! The UI layer: a vello renderer and scene, rendered on demand into a texture the presenter composites.
use std::{num::NonZeroUsize, time::Instant};
use vello::{peniko::Color, AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

pub struct UiLayer {
    renderer: Renderer,
    scene: Scene,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    dirty: bool,
    /// The last render's CPU cost on the calling thread (encode + submit), ms.
    pub last_render_ms: f32,
    pub renders: u64,
}

impl UiLayer {
    /// A layer at `w × h` physical pixels on the presenter's device (the overlay must share it).
    pub fn new(device: &wgpu::Device, w: u32, h: u32) -> Result<UiLayer, vello::Error> {
        // area anti-aliasing only: no MSAA pipelines to compile, and the family's chrome is axis-aligned rectangles + text
        let renderer = Renderer::new(device, RendererOptions { use_cpu: false, antialiasing_support: AaSupport::area_only(), num_init_threads: NonZeroUsize::new(1), pipeline_cache: None })?;
        let (texture, view) = Self::target(device, w, h);
        Ok(UiLayer { renderer, scene: Scene::new(), texture, view, size: (w.max(1), h.max(1)), dirty: true, last_render_ms: 0.0, renders: 0 })
    }
    fn target(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView) {
        // STORAGE_BINDING: vello's fine stage writes it; TEXTURE_BINDING: stage 4 samples it; COPY_SRC: goldens
        let texture = device.create_texture(&wgpu::TextureDescriptor { label: Some("min-ui layer"), size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = texture.create_view(&Default::default());
        (texture, view)
    }
    /// Re-create the target at a new size (the presenter must be handed the new `view`).
    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if (w.max(1), h.max(1)) == self.size { return }
        let (t, v) = Self::target(device, w, h);
        self.texture = t; self.view = v; self.size = (w.max(1), h.max(1)); self.dirty = true;
    }
    pub fn size(&self) -> (u32, u32) { self.size }
    pub fn view(&self) -> &wgpu::TextureView { &self.view }
    pub fn texture(&self) -> &wgpu::Texture { &self.texture }
    pub fn is_dirty(&self) -> bool { self.dirty }
    /// The scene to draw into; taking it marks the layer dirty. Callers `reset()` it and draw everything (one scene, no retained tree).
    pub fn scene_mut(&mut self) -> &mut Scene { self.dirty = true; &mut self.scene }
    pub fn mark_dirty(&mut self) { self.dirty = true; }
    /// Render the scene into the texture if anything changed. Returns the CPU ms spent, or `None` when the layer was clean.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<f32> {
        if !self.dirty { return None }
        let t = Instant::now();
        let (w, h) = self.size;
        // transparent base: the video shows through wherever the chrome draws nothing
        if let Err(e) = self.renderer.render_to_texture(device, queue, &self.scene, &self.view, &RenderParams { base_color: Color::TRANSPARENT, width: w, height: h, antialiasing_method: AaConfig::Area }) {
            log::error!("min-ui render: {e}");
        }
        self.dirty = false;
        self.last_render_ms = t.elapsed().as_secs_f32() * 1000.0;
        self.renders += 1;
        Some(self.last_render_ms)
    }
}
