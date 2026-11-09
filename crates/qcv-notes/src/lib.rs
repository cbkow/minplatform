//! `qcv-notes` — QCView's annotation notes (`src/annotations/annotation_note`, `annotation_serializer`, `annotation_io`),
//! canon row 215. The sidecar is a CONTRACT shared with minNotes: `<media_dir>/.qcview/<sanitized media name>/notes.json` +
//! `images/`, written exactly as Qt's `QJsonDocument::toJson(Indented)` writes it (keys sorted, four-space indent, Qt's
//! shortest number form — `8.935546875e-05` — and a trailing newline). No schema extensions, ever.
//! Fixed against QCView (row 215): stroke ids are KEPT across saves (QCView minted new ones every reserialize).
pub mod qt_json;
pub mod note;
pub mod strokes;
pub mod io;
pub mod ink;
pub mod export;
pub mod export_doc;
pub use note::Note;
pub use strokes::{Stroke, Tool};

/// QCView `currentNoteKey`: the SMPTE key of `frame` at `fps` — non-drop, counted from frame 0, the rate as `round(fps × 1000) / 1000`
/// — and its `timestamp_seconds` (frame / fps).
pub fn note_key(frame: i32, fps: f64) -> (String, f64) {
    let tc = qcv_meta::TimecodeFormatter::new((fps * 1000.0).round() as i32, 1000, false);
    (if tc.is_valid() { tc.format(frame) } else { "00:00:00:00".to_string() }, if fps > 0.0 { frame as f64 / fps } else { 0.0 })
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_are_qcviews() {
        assert_eq!(super::note_key(0, 24.0).0, "00:00:00:00");
        assert_eq!(super::note_key(25, 24.0), ("00:00:01:01".to_string(), 25.0 / 24.0));
        assert_eq!(super::note_key(30, 29.97).0, "00:00:01:00", "non-drop at 29.97: 30 frames to the second");
    }
}
