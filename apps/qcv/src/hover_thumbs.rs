//! The hover thumbnail as a UI node over the viewport (canon §17.5 item 5, row 170): QCView's cache + loader
//! (`qcv-player`'s `TimelineThumbnailCache`, the GOP-cache peek first) feeding a vello image at 0.18 of the content
//! rect's width — display-referred, no longer composited into the frame before the colour chain. In a solo view it
//! FOLLOWS the pointer horizontally, sitting just above the bands and clamped to the content rect (the user 2026-11-07:
//! QCView parked it bottom-left only because of the Qt render-thread split; "this might be more ergonomic"); dual: the
//! two track the pointer as a pair, A above B (row 170, the user 2026-11-07: "two that track the playhead instead of parking").
//! Requests are throttled at QCView's 16 ms; playback drops the loader to one worker; the strip precaches on open (video
//! only — a sequence's thumbnails come from its cache or its frame files on hover, QCView never precached).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::paint;
use qcv_player::{thumbnail_cache::HOVER_REQUEST_THROTTLE_MS, ThumbConfig, ThumbKey, ThumbPixels, TimelineThumbnailCache, Transport, VideoThumbLoader};
use std::{cell::RefCell, collections::HashMap, rc::Rc, time::{Duration, Instant}};
use vello::Scene;

/// each handle's R'G'B' pixels (the loader's) and its picture in the current look (its generation) — row 216
type Images = Rc<RefCell<HashMap<u64, (ThumbPixels, Option<(u64, peniko::ImageBrush)>)>>>;

pub struct HoverThumbs {
    cache: TimelineThumbnailCache,
    images: Images,
    next: u64,
    last_request: Option<Instant>,
    pending_frame: Option<i32>,
    shown: Option<(u64, u32, u32)>,
    playing: bool,
    /// the media the cache holds (dual follows each side's without precaching)
    path: String,
    /// QCView `ui/timelineHoverThumbs`
    pub enabled: bool,
}

impl HoverThumbs {
    pub fn new() -> HoverThumbs {
        let images: Images = Default::default();
        let ev = images.clone();
        let cache = TimelineThumbnailCache::new(ThumbConfig::default(), VideoThumbLoader::any_factory(), Box::new(move |h| { ev.borrow_mut().remove(&h); }));
        HoverThumbs { cache, images, next: 0, last_request: None, pending_frame: None, shown: None, playing: false, path: String::new(), enabled: true }
    }
    /// Dual view: hold this side's media (on hover only — no strip precache beside two playing decoders).
    pub fn follow(&mut self, path: &str) { if path != self.path { self.cache.clear(); self.images.borrow_mut().clear(); self.shown = None; self.path = path.to_string(); } }
    pub fn on_open(&mut self, p: &Transport) { self.cache.clear(); self.images.borrow_mut().clear(); self.shown = None; let path = p.thumb_key(); self.path = path.clone(); if !path.is_empty() && p.decoder().as_sequence().is_none() { self.cache.precache_strip(&path, p.frame_count(), p.current_frame()); } }
    #[allow(dead_code)]
    pub fn clear(&mut self) { self.pending_frame = None; if self.shown.is_some() { self.shown = None; } }
    /// Per frame: the hovered timeline frame (or none), the uploads to adopt; true when what is shown changed.
    /// `look` = the thumbnail look's generation and its converter (the clip's chain on the GPU); a stale shown picture is redone.
    pub fn tick(&mut self, p: Option<&mut Transport>, hover: Option<i32>, now: Instant, look: (u64, &mut dyn FnMut(&ThumbPixels) -> Option<peniko::ImageBrush>)) -> bool {
        let mut changed = false;
        let playing = p.as_ref().is_some_and(|p| p.is_playing());
        if playing != self.playing { self.playing = playing; self.cache.notify_playback_state(playing); }
        let (images, next) = (self.images.clone(), &mut self.next);
        let n = self.cache.process_pending_uploads(|_: &ThumbKey, px: &ThumbPixels| { *next += 1; images.borrow_mut().insert(*next, (px.clone(), None)); *next });
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
        // the shown thumbnail in the current look (only it: the strip's others convert when they are shown)
        if let Some((h, _, _)) = self.shown {
            let mut imgs = self.images.borrow_mut();
            if let Some((raw, img)) = imgs.get_mut(&h) && img.as_ref().map(|x| x.0) != Some(look.0) {
                let pic = (look.1)(raw).unwrap_or_else(|| paint::image_rgba8(raw.width, raw.height, raw.rgba8()));
                *img = Some((look.0, pic)); changed = true;
            }
        }
        changed
    }
    /// The shown thumbnail's drawn height (0 when none): dual stacks A above B by it.
    pub fn shown_height(&self, content: Rect) -> f64 {
        let Some((h, _, _)) = self.shown else { return 0.0 };
        let imgs = self.images.borrow(); let Some((_, Some((_, img)))) = imgs.get(&h) else { return 0.0 };
        content.width() * t::HOVER_THUMBNAIL_FRAME_RATIO * img.image.height as f64 / img.image.width.max(1) as f64
    }
    pub fn paint(&self, scene: &mut Scene, content: Rect, scale: f64, anchor_x: Option<f64>) { self.paint_lifted(scene, content, scale, anchor_x, 0.0) }
    /// `lift` raises it (dual's A over B: the pair tracks the pointer together, canon row 170).
    pub fn paint_lifted(&self, scene: &mut Scene, content: Rect, scale: f64, anchor_x: Option<f64>, lift: f64) {
        let Some((h, _, _)) = self.shown else { return };
        let imgs = self.images.borrow(); let Some((_, Some((_, img)))) = imgs.get(&h) else { return };
        let (w, hh) = (img.image.width, img.image.height);
        let dw = content.width() * t::HOVER_THUMBNAIL_FRAME_RATIO; let dh = dw * hh as f64 / w.max(1) as f64;
        let pad = t::SPACING_100 as f64 * scale; let bw = t::BORDER_WIDTH_100 as f64 * scale;
        let x0 = match anchor_x { Some(ax) => (ax - dw / 2.0).clamp(content.x0 + pad, (content.x1 - pad - dw).max(content.x0 + pad)), None => content.x0 + pad };
        let r = Rect::new(x0, content.y1 - pad - dh - lift, x0 + dw, content.y1 - pad - lift);
        paint::rounded(scene, r.inflate(bw, bw), t::HOVER_THUMBNAIL_CORNER_RADIUS as f64 * scale + bw, t::HOVER_THUMBNAIL_BORDER_COLOR);
        paint::image(scene, img, r);
    }
    pub fn is_shown(&self) -> bool { self.shown.is_some() }
    pub fn shutdown(&mut self) { self.cache.shutdown(); }
}
