//! What the player opens: a file, or an image sequence found from ONE of its frames through `min-paths` (row 120's
//! detector — QCView's rules) and handed to FFmpeg's image2 demuxer as a printf pattern with the real start number.
use qcv_timeline::Rate;

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    File(String),
    Sequence { /// the printf pattern path FFmpeg opens (`dir/name_%04d.exr`)
        pattern: String, rate: Rate, start: i64, end: i64, /// the detector's human label (`name_####.exr`, missing-frame count)
        label: String },
}
const SEQ_EXT: &[&str] = &["exr", "png", "tif", "tiff", "dpx", "jpg", "jpeg", "tga", "bmp"];

impl Source {
    /// A path from the command line or a drop: an image file that belongs to a sequence opens the sequence (24 fps unless
    /// `rate` says otherwise); anything else opens as a file.
    pub fn from_path(path: &str, rate: Option<Rate>) -> Source {
        let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
        if SEQ_EXT.contains(&ext.as_str()) {
            if let Some(seq) = min_paths::sequence::detect_on_disk(std::path::Path::new(path)) {
                let pattern = std::path::Path::new(&seq.directory).join(&seq.pattern).to_string_lossy().into_owned();
                let label = format!("{} ({} frames{})", seq.pattern_hashes(), seq.match_count, if seq.missing.is_empty() { String::new() } else { format!(", {} missing", seq.missing.len()) });
                return Source::Sequence { pattern, rate: rate.unwrap_or(Rate::new(24, 1)), start: seq.start as i64, end: seq.end as i64, label };
            }
        }
        Source::File(path.to_string())
    }
    pub fn display(&self) -> String { match self { Source::File(p) => p.rsplit(['/', '\\']).next().unwrap_or(p).to_string(), Source::Sequence { label, .. } => label.clone() } }
    pub fn path(&self) -> &str { match self { Source::File(p) => p, Source::Sequence { pattern, .. } => pattern } }
}
