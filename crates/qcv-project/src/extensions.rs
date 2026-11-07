//! QCView's `media_extensions.h`: the one table that decides what gets a bin entry. 32 video (webp / gif / dpx included),
//! 13 audio, 6 image. `.r3d` / `.braw` / `.ari` are absent ON PURPOSE (refused: no decoder in the family); ARRIRAW in an MXF
//! is admitted by extension and badged unsupported by the extractor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Video, Audio, Image, Unknown }

pub const VIDEO: &[&str] = &["mov", "mp4", "m4v", "mxf", "mkv", "avi", "webm", "webp", "gif", "ts", "mts", "m2ts", "m2t", "mpg", "mpeg", "mpe", "m2v", "vob", "3gp", "3g2", "wmv", "asf", "flv", "f4v", "ogv", "ogm", "mjpeg", "mjpg", "y4m", "dv", "mj2", "dpx"];
pub const AUDIO: &[&str] = &["wav", "aif", "aiff", "aifc", "mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wma", "caf"];
pub const IMAGE: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "exr"];

pub fn kind_for_suffix(suffix_lower: &str) -> Kind {
    if VIDEO.contains(&suffix_lower) { Kind::Video } else if AUDIO.contains(&suffix_lower) { Kind::Audio } else if IMAGE.contains(&suffix_lower) { Kind::Image } else { Kind::Unknown }
}
pub fn suffix_lower(path: &str) -> String { std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default() }
pub fn kind_for_path(path: &str) -> Kind { kind_for_suffix(&suffix_lower(path)) }
/// A regular file with a suffix from the table (a folder is never media).
pub fn is_supported_media_file(path: &str) -> bool { let p = std::path::Path::new(path); !p.is_dir() && kind_for_path(path) != Kind::Unknown }
