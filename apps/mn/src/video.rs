//! Video files for the video block (canon row 316, stage 1: the block at rest). What a file is - its size, length
//! and rate, read from its headers - and a still of one of its frames (the poster), both through the family's decoder
//! (`min-ffmpeg` / `qcv-player`, QCView's), on a helper thread. The poster's colour goes through the presenter's own
//! pipeline (row 216: one pipeline for captures and thumbnails): the decoder gives R'G'B' code values, a small engine
//! on the window's device shows them tagged with the clip's transfer and primaries, and the result is captured,
//! brought to SDR (BT.2390 for an HDR source) and encoded as sRGB - as QCView's thumbnails are (`apps/qcv/src/
//! thumb_color.rs`, mirrored here without its looks: minNotes has no colour controls).
use min_present::{engine::Engine, ColorMeta, Frame, Pixels, PlanarLayout, Plane};
use qcv_player::thumbnail_cache::{ThumbLoader, ThumbPixels, ThumbSamples, VideoThumbLoader};
use std::path::Path;

/// File extensions that are videos (QCView's list, without the two that are pictures in a note: GIF and WebP).
pub const EXTENSIONS: &[&str] = &["mov", "mp4", "m4v", "mxf", "mkv", "avi", "webm", "ts", "mts", "m2ts", "m2t", "mpg", "mpeg", "mpe", "m2v", "vob", "3gp", "3g2", "wmv", "asf", "flv", "f4v", "ogv", "ogm", "mjpeg", "mjpg", "y4m", "dv", "mj2"];
pub fn is_video(path: &Path) -> bool { path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())) }

/// What a video file's headers say (the Qt app's record: the coded size, no rotation applied).
pub struct Probe { pub w: u32, pub h: u32, pub frames: i32, pub fps: f64, pub dur_ms: f64, pub meta: ColorMeta }
pub fn probe(path: &Path) -> Result<Probe, String> {
    let s = qcv_player::Session::open(&path.to_string_lossy(), &qcv_player::OpenOptions { threads: 1, hw: None, hw_intra: None, seq: None, fast: true, live: None }).map_err(|e| e.to_string())?;
    let i = &s.info; if i.width == 0 || i.height == 0 { return Err("the file has no picture".into()) }
    let fps = i.rate().as_f64();
    Ok(Probe { w: i.width, h: i.height, frames: i.frame_count.max(1), fps, dur_ms: (i.duration_secs * 1000.0).round().max(0.0), meta: i.meta })
}
/// A frame of the file as R'G'B' code values, its longer side about `max`, with the clip's colour tags.
pub fn still(path: &Path, frame: i32, max: u32) -> Result<(ThumbPixels, ColorMeta), String> {
    let meta = probe(path)?.meta;
    let mut l = VideoThumbLoader::open(&path.to_string_lossy()).ok_or("the decoder could not open the file")?;
    l.load_thumbnail(frame.max(0), max.max(16)).map(|px| (px, meta)).ok_or_else(|| "the frame could not be decoded".into())
}
/// A length as a poster's badge shows it: `0:08`, `12:40`, `1:02:03`.
pub fn length(dur_ms: f64) -> String { let s = (dur_ms / 1000.0).round().max(0.0) as u64; if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) } }

/// A frame as the counter shows it: HH:MM:SS:FF, never drop-frame, at the rate rounded to whole frames, counted from
/// the file's first frame (QCView's note key and the Qt app's counter).
pub fn timecode(frame: i64, fps: f64) -> String { let r = (fps.round() as i64).max(1); let f = frame.max(0); let s = f / r; format!("{:02}:{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60, f % r) }

/// The poster's colour stage, on the window's thread (it draws on the GPU and reads back: once per poster).
#[derive(Default)]
pub struct Poster { engine: Option<Engine> }
impl Poster {
    /// Straight RGBA, 8 bits, sRGB. Without a GPU engine: the code values as stored (right for an SDR clip).
    pub fn convert(&mut self, device: Option<(&wgpu::Device, &wgpu::Queue)>, px: &ThumbPixels, meta: &ColorMeta) -> (Vec<u8>, u32, u32) {
        let plain = |px: &ThumbPixels| (px.rgba8(), px.width, px.height);
        if self.engine.is_none() { if let Some((d, q)) = device { self.engine = Some(Engine::new(d.clone(), q.clone(), wgpu::TextureFormat::Rgba16Float)); } }
        let Some(en) = self.engine.as_mut() else { return plain(px) };
        let (w, h) = (px.width, px.height); if w == 0 || h == 0 { return plain(px) }
        let (data, float) = match &px.samples { ThumbSamples::Code16(v) => (v, false), ThumbSamples::Half(v) => (v, true) };
        let pitch = (w * 2).div_ceil(256) * 256;
        let mut planes = [vec![0u8; (pitch * h) as usize], vec![0u8; (pitch * h) as usize], vec![0u8; (pitch * h) as usize]];
        for y in 0..h { for x in 0..w { let i = ((y * w + x) * 3) as usize; let o = (y * pitch + x * 2) as usize;
            for (pl, c) in [(0usize, 1usize), (1, 2), (2, 0)] { planes[pl][o..o + 2].copy_from_slice(&data[i + c].to_ne_bytes()); } } }
        let m = ColorMeta { matrix: min_present::Matrix::Identity, range: min_present::Range::Full, chroma: min_present::Chroma::Rgb, bit_depth: 16, rotation: 0, sar: (1, 1), range_forced: false, ..*meta };
        let [g, b, r] = planes;
        let frame = Frame { width: w, height: h, pixels: Pixels::Planar { planes: vec![Plane::owned(g, w, h, pitch), Plane::owned(b, w, h, pitch), Plane::owned(r, w, h, pitch)], layout: PlanarLayout::Rgb { bytes_per_sample: 2, code_bits: 16, float, alpha: false, legal_expand: false } }, meta: m };
        if en.show(&frame).is_err() { return plain(px) }
        let peak = en.bypass_hdr_peak();
        let Ok((mut lin, cw, ch)) = en.capture(false) else { return plain(px) };
        min_present::capture::to_sdr(&mut lin, peak);
        let rgb = min_present::capture::srgb8(&lin);
        (rgb.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(), cw, ch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extensions_and_lengths() {
        assert!(is_video(Path::new("/a/Shot.MOV")) && is_video(Path::new("b.mp4")) && !is_video(Path::new("c.gif")) && !is_video(Path::new("d.pdf")));
        assert_eq!(timecode(0, 23.976), "00:00:00:00"); assert_eq!(timecode(49, 23.976), "00:00:02:01"); assert_eq!(timecode(90_000, 25.0), "01:00:00:00");
        assert_eq!(length(8342.0), "0:08"); assert_eq!(length(760_000.0), "12:40"); assert_eq!(length(3_723_000.0), "1:02:03");
        assert!(probe(Path::new("/nowhere/at/all.mov")).is_err());
    }
}
