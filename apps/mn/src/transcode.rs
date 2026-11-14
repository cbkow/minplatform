//! A video's playable copy for a web folder (canon row 340): H.264 with AAC sound in MP4, 8 bits, the source's rate,
//! its size up to 1920 on the longer side, a key frame every half second (a jump to a note's frame lands at once).
//! FFmpeg's own program does it, run once per video: it is the family's build (the one the application decodes with),
//! found beside the application; a stop ends the process. A file that is H.264 in MP4 within the size already is
//! copied as it is.
//!
//! Two ways, by the picture (rows 340, 343):
//! - A picture in standard range goes straight through FFmpeg (stage A).
//! - A picture in PQ, HLG or linear light is brought to standard range by OUR colour stage, as its poster and stills
//!   are (stage B): each frame is decoded here, reduced to the copy's size, shown to the small engine and captured
//!   as sRGB, and the finished frames are fed to FFmpeg, which encodes them and takes the sound from the file. It
//!   needs the graphics card; without one such a clip stays stills.
use crate::export::site::{Clip, Videos};
use std::{io::{BufRead, BufReader, Read}, path::{Path, PathBuf}, process::{Command, Stdio}, sync::atomic::{AtomicBool, Ordering::Relaxed}};

/// The longer side of a playable copy, at most.
pub const LONGEST: u32 = 1920;

/// FFmpeg's program: named by `MN_FFMPEG`, beside the application (on macOS in the bundle's `Helpers`, where QCView's
/// release keeps it), or - a build run from the source tree - in the tree's `external/ffmpeg/<os>/bin`.
pub fn ffmpeg() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(p) = std::env::var_os("MN_FFMPEG").map(PathBuf::from).filter(|p| p.is_file()) { return Some(p) }
    let me = std::env::current_exe().ok()?; let here = me.parent()?;
    for p in [here.join(exe), here.join("../Helpers").join(exe), here.join("../Frameworks").join(exe), here.join("../Resources").join(exe)] { if p.is_file() { return Some(p) } }
    let os = if cfg!(target_os = "macos") { "macos-arm64" } else if cfg!(windows) { "win64" } else { "linux" };
    here.ancestors().map(|a| a.join("external/ffmpeg").join(os).join("bin").join(exe)).find(|p| p.is_file())
}

/// What `plan` learned of a file, for `make`: its size as stored, its rate as a fraction, its colour tags, and whether it goes through our colour stage.
#[derive(Clone, Copy)]
struct Known { w: u32, h: u32, rate: (i32, i32), meta: min_present::ColorMeta, ours: bool }
pub struct Ffmpeg { program: Option<PathBuf>, known: std::collections::HashMap<PathBuf, Known>, gpu: Option<(wgpu::Device, wgpu::Queue)>, poster: crate::video::Poster }
impl Ffmpeg { pub fn new(gpu: Option<(wgpu::Device, wgpu::Queue)>) -> Ffmpeg { Ffmpeg { program: ffmpeg(), known: Default::default(), gpu, poster: Default::default() } } }

/// The picture's size in the copy: within [`LONGEST`], even both ways.
fn fitted(w: u32, h: u32) -> (u32, u32) {
    let k = (LONGEST as f64 / w.max(h).max(1) as f64).min(1.0); let even = |v: f64| ((v / 2.0).round() as u32).max(1) * 2;
    (even(w as f64 * k), even(h as f64 * k))
}
/// The copy's rate in bits a second: about 8 Mbit/s at 1920 x 1080 and 24 frames, by its picture and its rate.
fn bitrate(w: u32, h: u32, fps: f64) -> u64 { ((8_000_000.0 * (w as f64 * h as f64) / (1920.0 * 1080.0) * (fps.clamp(12.0, 60.0) / 24.0).sqrt()) as u64).clamp(1_200_000, 16_000_000) }

impl Videos for Ffmpeg {
    fn plan(&mut self, path: &Path) -> Result<Clip, String> {
        use min_present::Transfer as T;
        if self.program.is_none() { return Err("FFmpeg's program is not with the application: stills only".into()) }
        if !crate::video::is_video(path) { return Err("Not a video file: stills only".into()) }
        let s = qcv_player::Session::open(&path.to_string_lossy(), &qcv_player::OpenOptions { threads: 1, hw: None, hw_intra: None, seq: None, fast: true, live: None }).map_err(|e| format!("The file could not be read ({e})"))?;
        let i = &s.info; if i.width == 0 || i.height == 0 { return Err("The file has no picture".into()) }
        let ours = !matches!(i.meta.transfer, T::Srgb | T::Bt1886);
        if ours && self.gpu.is_none() { return Err("An HDR or linear picture needs the graphics card: stills only".into()) }
        let (w, h) = (i.width, i.height);
        self.known.insert(path.to_path_buf(), Known { w, h, rate: i.frame_rate, meta: i.meta, ours });
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let copy = !ours && matches!(ext.as_str(), "mp4" | "m4v") && i.codec_short.eq_ignore_ascii_case("h264") && matches!(i.pix_fmt.as_str(), "yuv420p" | "yuvj420p") && w.max(h) <= LONGEST;
        let m = std::fs::metadata(path).map_err(|e| format!("The file could not be read ({e})"))?;
        let stamp = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        Ok(Clip { fps: i.rate().as_f64(), frames: i.frame_count.max(1) as i64, copy, key: format!("{}-{stamp}{}", m.len(), if ours { "-sdr1" } else { "" }) })
    }
    fn make(&mut self, path: &Path, clip: &Clip, out: &Path, stop: &AtomicBool, done: &dyn Fn(f32)) -> Result<(), String> {
        let program = self.program.clone().ok_or("FFmpeg's program is not with the application")?;
        // a file that plays as it is keeps its picture and sound untouched; it is written anew only to start at once
        // from a server and to carry no turn (see below)
        if clip.copy {
            let o = Command::new(&program).args(["-nostdin", "-y", "-hide_banner", "-loglevel", "error", "-display_rotation", "0", "-i"]).arg(path).args(["-map", "0:v:0", "-map", "0:a:0?", "-c", "copy", "-movflags", "+faststart", "-f", "mp4"]).arg(out).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| format!("FFmpeg could not be started ({e})"))?;
            if o.status.success() { return Ok(()) }
            return std::fs::copy(path, out).map(|_| ()).map_err(|e| format!("The video could not be copied ({e})"))
        }
        let known = self.known.get(path).copied();
        let (sw, sh) = known.map(|k| (k.w, k.h)).unwrap_or((LONGEST, 1080)); let (w, h) = fitted(sw, sh);
        let gop = ((clip.fps / 2.0).round() as i64).max(1).to_string(); let rate = bitrate(w, h, clip.fps);
        let mut c = Command::new(program);
        // our colour stage: the first frame says the size the frames come in; FFmpeg reads them from its input
        let mut ours = None;
        if let Some(k) = known.filter(|k| k.ours) {
            let mut reader = qcv_player::thumbnail_cache::FrameReader::open(&path.to_string_lossy()).ok_or("The file could not be read frame by frame")?;
            let first = reader.next_frame(LONGEST).ok_or("The file has no frame that can be read")?;
            c.args(["-y", "-hide_banner", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s"]).arg(format!("{}x{}", first.width, first.height)).args(["-r"]).arg(format!("{}/{}", k.rate.0.max(1), k.rate.1.max(1))).args(["-i", "pipe:0", "-i"]).arg(path)
                .args(["-map", "0:v:0", "-map", "1:a:0?", "-vf"]).arg(format!("scale={w}:{h}:flags=bicubic:out_color_matrix=bt709:out_range=tv,format=yuv420p")).args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);
            ours = Some((reader, first, k));
        } else {
            // (a file stored turned - a phone's - is NOT turned upright: the application shows a file's frames as they are stored, its
            // stills and the drawings on them are in that sheet, and the copy must match them; the file's turn is set to none)
            c.args(["-nostdin", "-y", "-hide_banner", "-loglevel", "error", "-display_rotation", "0", "-i"]).arg(path).args(["-map", "0:v:0", "-map", "0:a:0?", "-vf"]).arg(format!("scale={w}:{h}:flags=bicubic,format=yuv420p"));
        }
        if cfg!(target_os = "macos") { c.args(["-c:v", "h264_videotoolbox", "-allow_sw", "1", "-b:v"]).arg(rate.to_string()).args(["-maxrate"]).arg((rate * 3 / 2).to_string()).args(["-profile:v", "high"]); }
        else { c.args(["-c:v", "libx264", "-preset", "medium", "-b:v"]).arg(rate.to_string()).args(["-maxrate"]).arg((rate * 3 / 2).to_string()).args(["-bufsize"]).arg((rate * 2).to_string()); }
        c.args(["-g", &gop, "-c:a", "aac", "-b:a", "160k", "-ac", "2", "-movflags", "+faststart"]);
        if ours.is_none() { c.args(["-progress", "pipe:1", "-stats_period", "0.5"]); }
        c.args(["-f", "mp4"]).arg(out);
        c.stdin(if ours.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(if ours.is_some() { Stdio::null() } else { Stdio::piped() }).stderr(Stdio::piped());
        #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }   // no console window
        let mut child = c.spawn().map_err(|e| format!("FFmpeg could not be started ({e})"))?;
        // what it says of errors is read beside its progress (a full pipe would hold it)
        let said = child.stderr.take().map(|mut e| std::thread::spawn(move || { let mut s = String::new(); let _ = e.read_to_string(&mut s); s }));
        if let Some((mut reader, first, k)) = ours {
            // frame after frame: through the colour stage, into FFmpeg (a frame that will not pass ends it: the copy is not made)
            use std::io::Write;
            let mut pipe = child.stdin.take(); let (mut next, mut n, mut failed) = (Some(first), 0i64, None);
            while let (Some(px), Some(w)) = (next.take(), pipe.as_mut()) {
                if stop.load(Relaxed) { break }
                let Some((rgb, fw, fh)) = self.poster.sdr_rgb(self.gpu.as_ref().map(|g| (&g.0, &g.1)), &px, &k.meta) else { failed = Some("The colour stage could not show a frame"); break };
                if (fw, fh) != (px.width, px.height) { failed = Some("The colour stage gave a frame of another size"); break }
                if w.write_all(&rgb).is_err() { break }   // FFmpeg ended: what it said is read below
                n += 1; if n % 4 == 0 { done(n as f32 / clip.frames.max(1) as f32); }
                next = reader.next_frame(LONGEST);
            }
            drop(pipe);
            if stop.load(Relaxed) || failed.is_some() { let _ = child.kill(); }
            let status = child.wait().map_err(|e| format!("FFmpeg did not end ({e})"))?; let said = said.and_then(|t| t.join().ok()).unwrap_or_default();
            if stop.load(Relaxed) { return Err("Stopped".into()) }
            if let Some(f) = failed { return Err(f.into()) }
            if !status.success() { let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no reason given").trim().chars().take(160).collect::<String>(); return Err(format!("The video could not be made: {last}")) }
            return Ok(())
        }
        if let Some(o) = child.stdout.take() {
            for line in BufReader::new(o).lines().map_while(Result::ok) {
                if stop.load(Relaxed) { let _ = child.kill(); break }
                if let Some(n) = line.strip_prefix("frame=").and_then(|v| v.trim().parse::<f32>().ok()) { done(n / clip.frames.max(1) as f32); }
            }
        }
        let status = child.wait().map_err(|e| format!("FFmpeg did not end ({e})"))?;
        let said = said.and_then(|t| t.join().ok()).unwrap_or_default();
        if stop.load(Relaxed) { return Err("Stopped".into()) }
        if !status.success() { let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no reason given").trim().chars().take(160).collect::<String>(); return Err(format!("The video could not be made: {last}")) }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_copy_keeps_within_the_longest_side_with_even_sides_and_a_rate_by_its_size() {
        assert_eq!((fitted(1920, 1080), fitted(3840, 2160), fitted(1080, 1920), fitted(2160, 3840), fitted(641, 359), fitted(4096, 1716)), ((1920, 1080), (1920, 1080), (1080, 1920), (1080, 1920), (642, 360), (1920, 804)));
        assert!(bitrate(1920, 1080, 24.0) == 8_000_000 && bitrate(960, 540, 24.0) == 2_000_000 && bitrate(1920, 1080, 60.0) > 12_000_000 && bitrate(160, 90, 24.0) == 1_200_000);
    }
}
