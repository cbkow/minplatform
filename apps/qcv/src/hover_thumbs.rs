//! The hover thumbnail as a UI node over the viewport (canon §17.5 item 5, row 170): QCView's cache + loader
//! (`qcv-player`'s `TimelineThumbnailCache`, the GOP-cache peek first) feeding a vello image at 0.18 of the content
//! rect's width — display-referred, no longer composited into the frame before the colour chain. In a solo view it
//! FOLLOWS the pointer horizontally, sitting just above the bands and clamped to the content rect (the user 2026-11-07:
//! QCView parked it bottom-left only because of the Qt render-thread split; "this might be more ergonomic"); dual keeps
//! QCView's parking, A bottom-left and B bottom-right.
//! Requests are throttled at QCView's 16 ms; playback drops the loader to one worker; the strip precaches on open (video
//! only — a sequence's thumbnails come from its cache or its frame files on hover, QCView never precached).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::paint;
use qcv_player::{thumbnail_cache::HOVER_REQUEST_THROTTLE_MS, ThumbConfig, ThumbKey, ThumbPixels, TimelineThumbnailCache, Transport, VideoThumbLoader};
use std::{cell::RefCell, collections::HashMap, rc::Rc, time::{Duration, Instant}};
use vello::Scene;

type Images = Rc<RefCell<HashMap<u64, peniko::ImageBrush>>>;

pub struct HoverThumbs {
    cache: TimelineThumbnailCache,
    images: Images,
    next: u64,
    last_request: Option<Instant>,
    pending_frame: Option<i32>,
    shown: Option<(u64, u32, u32)>,
    playing: bool,
    /// QCView `ui/timelineHoverThumbs`
    pub enabled: bool,
}

impl HoverThumbs {
    pub fn new() -> HoverThumbs {
        let images: Images = Default::default();
        let ev = images.clone();
        let cache = TimelineThumbnailCache::new(ThumbConfig::default(), VideoThumbLoader::any_factory(), Box::new(move |h| { ev.borrow_mut().remove(&h); }));
        HoverThumbs { cache, images, next: 0, last_request: None, pending_frame: None, shown: None, playing: false, enabled: true }
    }
    pub fn on_open(&mut self, p: &Transport) { self.cache.clear(); self.images.borrow_mut().clear(); self.shown = None; let path = p.path().to_string(); if !path.is_empty() && p.decoder().as_sequence().is_none() { self.cache.precache_strip(&path, p.frame_count(), p.current_frame()); } }
    #[allow(dead_code)]
    pub fn clear(&mut self) { self.pending_frame = None; if self.shown.is_some() { self.shown = None; } }
    /// Per frame: the hovered timeline frame (or none), the uploads to adopt; true when what is shown changed.
    pub fn tick(&mut self, p: Option<&mut Transport>, hover: Option<i32>, now: Instant) -> bool {
        let mut changed = false;
        let playing = p.as_ref().is_some_and(|p| p.is_playing());
        if playing != self.playing { self.playing = playing; self.cache.notify_playback_state(playing); }
        let (images, next) = (self.images.clone(), &mut self.next);
        let n = self.cache.process_pending_uploads(|_: &ThumbKey, px: &ThumbPixels| { *next += 1; images.borrow_mut().insert(*next, paint::image_rgba8(px.width, px.height, px.rgba.clone())); *next });
        if n > 0 { changed = true; }
        match (hover.filter(|_| self.enabled), p) {
            (Some(f), Some(p)) => {
                self.pending_frame = Some(f);
                let due = self.last_request.is_none_or(|t0| now.duration_since(t0) >= Duration::from_millis(HOVER_REQUEST_THROTTLE_MS));
                if due || changed {
                    self.last_request = Some(now);
                    let (e, _) = p.hover_thumbnail(&mut self.cache, f, ThumbConfig::default().width);
                    let s = (e.texture != 0).then_some((e.texture, e.width, e.height));
                    if s != self.shown { self.shown = s; changed = true; }
                }
            }
            _ => { if self.shown.is_some() { self.shown = None; changed = true; } self.pending_frame = None; }
        }
        changed
    }
    /// `anchor_x` = the pointer's x to centre on (solo); `None` parks bottom-left (dual's A side).
    pub fn paint(&self, scene: &mut Scene, content: Rect, scale: f64, anchor_x: Option<f64>) {
        let Some((h, w, hh)) = self.shown else { return };
        let imgs = self.images.borrow(); let Some(img) = imgs.get(&h) else { return };
        let dw = content.width() * t::HOVER_THUMBNAIL_FRAME_RATIO; let dh = dw * hh as f64 / w.max(1) as f64;
        let pad = t::SPACING_100 as f64 * scale; let bw = t::BORDER_WIDTH_100 as f64 * scale;
        let x0 = match anchor_x { Some(ax) => (ax - dw / 2.0).clamp(content.x0 + pad, (content.x1 - pad - dw).max(content.x0 + pad)), None => content.x0 + pad };
        let r = Rect::new(x0, content.y1 - pad - dh, x0 + dw, content.y1 - pad);
        paint::rounded(scene, r.inflate(bw, bw), t::HOVER_THUMBNAIL_CORNER_RADIUS as f64 * scale + bw, t::HOVER_THUMBNAIL_BORDER_COLOR);
        paint::image(scene, img, r);
    }
    pub fn is_shown(&self) -> bool { self.shown.is_some() }
    pub fn shutdown(&mut self) { self.cache.shutdown(); }
}
