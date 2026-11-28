//! QCView's `media_extensions.h`: the one table that decides what gets a bin entry. 31 video (webp / gif included), 13 audio,
//! 9 image — DPX moved from video to image and TGA / BMP joined (canon rows 190 / 191, 2026-11-08: native loaders, so they
//! detect as sequences and ride the sequence cache; QCView opened a .dpx as a one-frame video and read neither of the others). `.r3d` / `.braw` / `.ari` are absent ON PURPOSE (refused: no decoder in the family); ARRIRAW in an MXF
//! is admitted by extension and badged unsupported by the extractor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Video, Audio, Image, /// a file a provider opens (canon row 624)
    Remote, Unknown }

pub const VIDEO: &[&str] = &["mov", "mp4", "m4v", "mxf", "mkv", "avi", "webm", "webp", "gif", "ts", "mts", "m2ts", "m2t", "mpg", "mpeg", "mpe", "m2v", "vob", "3gp", "3g2", "wmv", "asf", "flv", "f4v", "ogv", "ogm", "mjpeg", "mjpg", "y4m", "dv", "mj2"];
pub const AUDIO: &[&str] = &["wav", "aif", "aiff", "aifc", "mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wma", "caf"];
pub const IMAGE: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "exr", "dpx", "tga", "bmp"];

/// The extensions the providers described on this machine open (canon rows 623-624): read once from their description
/// files, never written here. Empty where no provider is installed - the table is then what it was. An extension the
/// family reads itself is never handed to a provider.
pub fn remote() -> &'static [String] {
    static ALL: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        if !cfg!(unix) { return vec![] }   // shared blocks are POSIX so far
        let mut all: Vec<String> = min_framelink::described::installed().into_iter().flat_map(|p| p.extensions).filter(|e| !VIDEO.contains(&e.as_str()) && !AUDIO.contains(&e.as_str()) && !IMAGE.contains(&e.as_str())).collect();
        all.sort(); all.dedup(); all
    })
}

pub fn kind_for_suffix(suffix_lower: &str) -> Kind {
    if remote().iter().any(|e| e == suffix_lower) { return Kind::Remote }
    if VIDEO.contains(&suffix_lower) { Kind::Video } else if AUDIO.contains(&suffix_lower) { Kind::Audio } else if IMAGE.contains(&suffix_lower) { Kind::Image } else { Kind::Unknown }
}
pub fn suffix_lower(path: &str) -> String { std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default() }
pub fn kind_for_path(path: &str) -> Kind { kind_for_suffix(&suffix_lower(path)) }
/// A regular file with a suffix from the table (a folder is never media).
pub fn is_supported_media_file(path: &str) -> bool { let p = std::path::Path::new(path); !p.is_dir() && kind_for_path(path) != Kind::Unknown }
