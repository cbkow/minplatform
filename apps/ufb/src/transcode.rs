//! Transcode to MP4 (canon rows 508 points 10-12, 513): 1.4.2's one preset - H.264 with AAC at 192 kbit/s into an
//! `MP4` folder beside the source, the picture at its full size - made by the family's FFmpeg program through
//! `min-transcode` (the notes application's way of running it, shared).
//!
//! - **The encoder is each system's**: Windows `libx264 -crf 25 -preset fast`, to the letter 1.4.2's; macOS the
//!   system's own (`h264_videotoolbox -q:v 65`) - the family's macOS build of FFmpeg has no libx264, and 65 is the
//!   setting measured to give a CRF 25 file's size at no less likeness (row 513).
//! - **A picture in standard range goes straight through FFmpeg**, upright (a phone's clip stored turned is turned).
//!   **A picture in PQ, HLG or linear light, and every image sequence, goes through our colour stage first** and
//!   looks as the lightbox shows it. A sequence plays at the rate remembered for it there, and has no sound.
//! - **Nothing is overwritten** (1.4.2 overwrote): the file is written under a partial name inside `MP4` and named
//!   when it is whole - `<name>.mp4`, then `<name> 2.mp4`. Stopped or failed: nothing is left, and an `MP4` folder
//!   made for it is taken away again.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// What is transcoded: a video file, or an image sequence (its image2 pattern, its first and last number, its rate).
#[derive(Clone, Debug, PartialEq)]
pub enum Source { File(PathBuf), Sequence { pattern: String, start: i64, end: i64, rate: (u32, u32) } }

pub fn is_video(name: &str) -> bool { Path::new(name).extension().is_some_and(|e| crate::thumbs::helper::VIDEO.contains(&e.to_string_lossy().to_lowercase().as_str())) }

/// The folder the source lies in, and the name its MP4 takes (without `.mp4`): a file's own name without its
/// extension; a sequence's name without its number (`shot_010.%04d.exr` -> `shot_010`; the folder's name if nothing is left).
pub fn place(src: &Source) -> Option<(PathBuf, String)> {
    match src {
        Source::File(p) => Some((p.parent()?.to_path_buf(), p.file_stem()?.to_string_lossy().into_owned())),
        Source::Sequence { pattern, .. } => { let p = Path::new(pattern); let dir = p.parent()?.to_path_buf(); let name = p.file_name()?.to_string_lossy().replace("%%", "%");
            let stem = name.split("%0").next().unwrap_or("").trim_end_matches(['.', '_', '-', ' ']).to_string();
            Some((dir.clone(), if stem.is_empty() { dir.file_name()?.to_string_lossy().into_owned() } else { stem })) }
    }
}
pub fn label(src: &Source) -> String { match src { Source::File(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), Source::Sequence { .. } => place(src).map(|p| p.1).unwrap_or_default() } }

/// The encoder's arguments, each system's (see the top), and the sound's.
fn encoder(c: &mut Command, sound: bool) {
    if cfg!(target_os = "macos") { c.args(["-c:v", "h264_videotoolbox", "-allow_sw", "1", "-q:v", "65", "-profile:v", "high"]); } else { c.args(["-c:v", "libx264", "-crf", "25", "-preset", "fast"]); }
    if sound { c.args(["-c:a", "aac", "-b:a", "192k"]); }
    c.args(["-movflags", "+faststart"]);
}
/// Both sides even (the picture's format wants it): a row or a column is dropped from an odd one.
const EVEN: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

/// Make it. `say`: the share done, what is in hand. The MP4's path; Err("Stopped") when called off.
pub fn transcode(src: &Source, stop: &AtomicBool, say: &mut dyn FnMut(Option<f64>, &str)) -> Result<PathBuf, String> {
    let program = min_transcode::program("UFB_FFMPEG").ok_or("FFmpeg's program is not with the application.")?;
    let (parent, stem) = place(src).ok_or("The source has no folder.")?;
    let dir = parent.join("MP4"); let made_dir = !dir.exists();
    if made_dir { std::fs::create_dir(&dir).map_err(|e| format!("The MP4 folder could not be made: {}.", crate::lister::plain(&e)))?; }
    let partial = dir.join(format!(".{stem}.mp4.ufb-partial")); let _ = std::fs::remove_file(&partial);
    let what = label(src);
    let done = run(&program, src, &partial, stop, &mut |f| say(f, &what)).and_then(|()| {
        for n in 1..10_000 { let to = dir.join(if n == 1 { format!("{stem}.mp4") } else { format!("{stem} {n}.mp4") });
            if std::fs::symlink_metadata(&to).is_ok() { continue }
            match crate::ops::rename_no_replace(&partial, &to) { Ok(()) => return Ok(to), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue, Err(e) => return Err(format!("The video could not be named: {}.", crate::lister::plain(&e))) } }
        Err("No free name for the video.".into())
    });
    if done.is_err() { let _ = std::fs::remove_file(&partial); if made_dir { let _ = std::fs::remove_dir(&dir); } }
    done.map_err(|e| if stop.load(Ordering::Relaxed) { "Stopped".into() } else { e })
}

fn run(program: &Path, src: &Source, out: &Path, stop: &AtomicBool, say: &mut dyn FnMut(Option<f64>)) -> Result<(), String> {
    use min_present::Transfer as T; use qcv_player::thumbnail_cache::FrameReader;
    say(None);
    // what it is: a file is asked (its size, its rate, how many frames, how its light is written)
    let (reader, frames, rate, sound, turn) = match src {
        Source::File(path) => {
            let s = qcv_player::Session::open(&path.to_string_lossy(), &qcv_player::OpenOptions { threads: 1, hw: None, hw_intra: None, seq: None, fast: true, live: None }).map_err(|e| format!("The file could not be read ({e})."))?;
            let i = &s.info; if i.width == 0 || i.height == 0 { return Err("The file has no picture.".into()) }
            let (frames, rate, ours) = (i.frame_count.max(1) as i64, i.frame_rate, !matches!(i.meta.transfer, T::Srgb | T::Bt1886)); drop(s);
            if !ours {
                // standard range: straight through FFmpeg (it turns a clip stored turned upright by itself)
                let mut c = Command::new(program);
                c.args(["-nostdin", "-y", "-hide_banner", "-loglevel", "error", "-i"]).arg(path).args(["-map", "0:v:0", "-map", "0:a:0?", "-vf"]).arg(format!("{EVEN},format=yuv420p"));
                encoder(&mut c, true); c.args(["-progress", "pipe:1", "-stats_period", "0.5", "-f", "mp4"]).arg(out);
                return min_transcode::run(c, stop, &mut |n| say(Some((n as f64 / frames as f64).min(1.0))))
            }
            let reader = FrameReader::open(&path.to_string_lossy()).ok_or("The file could not be read frame by frame.")?;
            (reader, frames, (rate.0.max(1) as u32, rate.1.max(1) as u32), Some(path.clone()), qcv_meta::probe_rotation(&path.to_string_lossy()).rem_euclid(360))
        }
        Source::Sequence { pattern, start, end, rate } => {
            let reader = FrameReader::open_sequence(pattern, qcv_player::SeqOptions { rate: qcv_timeline::Rate::new(rate.0.max(1), rate.1.max(1)), start_number: *start }).ok_or("The sequence could not be read.")?;
            (reader, (end - start + 1).max(1), *rate, None, 0)
        }
    };
    // our colour stage: every frame is brought to standard range as the lightbox shows it, and fed to FFmpeg
    let mut reader = reader; let meta = reader.info().meta;
    let mut poster = qcv_player::poster::Poster::headless(); if !poster.has_engine() { return Err("This picture needs the graphics card to be brought to standard range.".into()) }
    let first = reader.next_frame(0).ok_or("No frame could be read.")?;
    let mut c = Command::new(program);
    c.args(["-y", "-hide_banner", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s"]).arg(format!("{}x{}", first.width, first.height)).arg("-r").arg(format!("{}/{}", rate.0.max(1), rate.1.max(1))).args(["-i", "pipe:0"]);
    if let Some(path) = &sound { c.arg("-i").arg(path).args(["-map", "0:v:0", "-map", "1:a:0?"]); }
    let turned = match turn { 90 => "transpose=1,", 180 => "hflip,vflip,", 270 => "transpose=2,", _ => "" };
    c.arg("-vf").arg(format!("{turned}{EVEN}:flags=bicubic:out_color_matrix=bt709:out_range=tv,format=yuv420p")).args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);
    encoder(&mut c, sound.is_some()); c.args(["-f", "mp4"]).arg(out);
    let (mut next, mut n, size) = (Some(first), 0i64, None::<(u32, u32)>); let mut size = size;
    min_transcode::run_fed(c, stop, &mut |w| {
        let Some(px) = next.take() else { return Ok(false) };
        let Some((rgb, fw, fh)) = poster.sdr_rgb(None, &px, &meta) else { return Err("The colour stage could not show a frame.") };
        // (a sequence whose frames are not all of one size cannot be one video)
        if *size.get_or_insert((fw, fh)) != (fw, fh) { return Err("The frames are not all of one size.") }
        if w.write_all(&rgb).is_err() { return Ok(false) }
        n += 1; if n % 2 == 0 { say(Some((n as f64 / frames as f64).min(1.0))); }
        next = reader.next_frame(0); Ok(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn where_the_mp4_goes_and_what_it_is_called() {
        assert_eq!(place(&Source::File(PathBuf::from("/job/renders/shot_010_v003.mov"))), Some((PathBuf::from("/job/renders"), "shot_010_v003".into())));
        let seq = |p: &str| Source::Sequence { pattern: p.into(), start: 1001, end: 1100, rate: (24, 1) };
        assert_eq!(place(&seq("/job/renders/shot_010.%04d.exr")), Some((PathBuf::from("/job/renders"), "shot_010".into())));
        assert_eq!(place(&seq("/job/renders/beauty_%05d.png")).unwrap().1, "beauty"); assert_eq!(place(&seq("/job/renders/100%%_v2.%04d.exr")).unwrap().1, "100%_v2");
        assert_eq!(place(&seq("/job/renders/%04d.exr")).unwrap().1, "renders", "a sequence that is numbers alone takes its folder's name");
        assert!(is_video("A.MOV") && is_video("clip.mxf") && !is_video("plate.exr") && !is_video("mov"));
        assert_eq!(label(&seq("/job/renders/shot_010.%04d.exr")), "shot_010");
    }
}
