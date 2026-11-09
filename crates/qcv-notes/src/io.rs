//! QCView `annotation_io`: where a media file's notes live and how they are read / written.
//! `<media_dir>/.qcview/<sanitized file name>/notes.json` + `images/` (an existing legacy `.ump` folder is used when there is
//! no `.qcview`); the root = the header (`media_file`, `media_type`, `created`) + `notes[]`, sorted by key on load; written
//! atomically (QSaveFile: a temp file renamed over). `saveNotes` writes a FRESH header, so `created` is the save time.
use crate::{note::Note, qt_json::to_qt_json};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// QCView `sanitizeMediaName`: `<>:"/\|?*` → `_`.
pub fn sanitize_media_name(name: &str) -> String { name.chars().map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect() }
/// QCView `fileStemForKey`: `:` and `;` → `_`.
pub fn file_stem_for_key(key: &str) -> String { key.replace([':', ';'], "_") }
/// "note_HH_MM_SS_FF.png" (the clean frame); the annotated one is "note_<stem>_annotated.png".
pub fn image_filename(key: &str) -> String { format!("note_{}.png", file_stem_for_key(key)) }
pub fn annotated_filename(key: &str) -> String { format!("note_{}_annotated.png", file_stem_for_key(key)) }

/// The sidecar root next to the media: `.qcview`, or an existing `.ump` when there is no `.qcview` (QCView's fallback).
fn sidecar_root(media: &Path) -> PathBuf {
    let parent = media.parent().unwrap_or(Path::new("."));
    let q = parent.join(".qcview"); if q.exists() { return q }
    let u = parent.join(".ump"); if u.exists() { return u }
    q
}
/// The media file's notes folder (holds notes.json + images/).
pub fn notes_dir(media: &Path) -> PathBuf { sidecar_root(media).join(sanitize_media_name(&media.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())) }
pub fn notes_json(dir: &Path) -> PathBuf { dir.join("notes.json") }
pub fn images_dir(dir: &Path) -> PathBuf { dir.join("images") }

// ---- streams (QCView plan B1: a live item's notes live with the PROJECT) ----
/// `<project_dir>/.qcview/streams` (or `.ump/streams` when only a legacy `.ump` exists: `projectSidecarWithFallback`).
pub fn streams_root(project: &Path) -> PathBuf {
    let dir = project.parent().unwrap_or(Path::new("."));
    let q = dir.join(".qcview"); let u = dir.join(".ump");
    (if !q.exists() && u.exists() { u } else { q }).join("streams")
}
/// QCView `streamFolderName`: the trimmed, sanitized item name ("stream" when empty) + "__" + the id's first 8 characters.
pub fn stream_folder_name(item_name: &str, item_id: &str) -> String {
    let n = sanitize_media_name(item_name.trim());
    format!("{}__{}", if n.is_empty() { "stream" } else { &n }, item_id.chars().take(8).collect::<String>())
}
/// QCView `findStreamFolder`: the existing folder whose name ends in "__<id8>" (first by name) — a renamed item keeps its notes.
pub fn find_stream_folder(project: &Path, item_id: &str) -> Option<String> {
    if item_id.is_empty() { return None }
    let suffix = format!("__{}", item_id.chars().take(8).collect::<String>());
    let mut names: Vec<String> = std::fs::read_dir(streams_root(project)).ok()?.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names.into_iter().find(|n| n.ends_with(&suffix))
}
/// A live item's notes folder: the existing one, else the one its first note will create (QCView creates it lazily).
pub fn stream_dir(project: &Path, item_name: &str, item_id: &str) -> PathBuf {
    streams_root(project).join(find_stream_folder(project, item_id).unwrap_or_else(|| stream_folder_name(item_name, item_id)))
}
/// QCView's stream header: `stream`, `stream_id`, `stream_url` (merged over what the file already holds).
pub fn stream_header(dir: &Path, name: &str, id: &str, url: &str) -> Map<String, Value> {
    let mut h = load_at(dir).map(|(h, _)| h).unwrap_or_default();
    h.insert("stream".into(), Value::from(name)); h.insert("stream_id".into(), Value::from(id)); h.insert("stream_url".into(), Value::from(url));
    h
}

/// QCView `kindForExtension`.
pub fn media_type(media: &Path) -> &'static str {
    let ext = media.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() { "wav" | "aif" | "aiff" | "mp3" | "flac" | "m4a" => "audio", "png" | "jpg" | "jpeg" | "tif" | "tiff" | "exr" | "dpx" | "bmp" | "gif" => "image", _ => "video" }
}

/// QCView `loadNotesAt`: (the header without `notes`, the notes sorted by key). No file = no notes (not an error).
pub fn load_at(dir: &Path) -> Result<(Map<String, Value>, Vec<Note>), String> {
    let path = notes_json(dir);
    if !path.exists() { return Ok((Map::new(), vec![])) }
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let Value::Object(mut root) = root else { return Err(format!("{}: not an object", path.display())) };
    let arr = root.remove("notes");
    let mut notes: Vec<Note> = arr.and_then(|a| if let Value::Array(a) = a { Some(a) } else { None }).unwrap_or_default().iter().filter_map(|v| v.as_object().and_then(Note::from_json)).collect();
    notes.sort_by(|a, b| a.timecode.cmp(&b.timecode));
    Ok((root, notes))
}
/// The document QCView writes for `header` + `notes` (`created` added when the header has none).
pub fn render(header: &Map<String, Value>, notes: &[Note], created_now: &str) -> String {
    let mut root = header.clone();
    if !root.contains_key("created") { root.insert("created".into(), Value::from(created_now)); }
    root.insert("notes".into(), Value::Array(notes.iter().map(Note::to_json).collect()));
    to_qt_json(&Value::Object(root))
}
/// QCView `saveNotesAt`: atomically (a temp file renamed over the old one).
pub fn save_at(dir: &Path, header: &Map<String, Value>, notes: &[Note]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = render(header, notes, &now_iso_utc());
    let path = notes_json(dir); let tmp = dir.join(".notes.json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}
/// QCView `saveNotes`: the media's folder, a FRESH header (media_file, media_type, created = now).
pub fn save(media: &Path, notes: &[Note]) -> Result<(), String> {
    let dir = notes_dir(media);
    let mut h = Map::new();
    h.insert("media_file".into(), Value::from(media.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    h.insert("media_type".into(), Value::from(media_type(media)));
    save_at(&dir, &h, notes)
}
/// QCView `loadNotes`.
pub fn load(media: &Path) -> Result<Vec<Note>, String> { load_at(&notes_dir(media)).map(|(_, n)| n) }

/// `QDateTime::currentDateTimeUtc().toString(Qt::ISODate)`: "2026-11-08T23:41:05Z".
pub fn now_iso_utc() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (s.div_euclid(86400), s.rem_euclid(86400));
    // days since 1970-01-01 → civil date (Howard Hinnant's algorithm)
    let z = days + 719468; let era = z.div_euclid(146097); let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; let y = yoe + era * 400; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153; let d = doy - (153 * mp + 2) / 5 + 1; let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", if m <= 2 { y + 1 } else { y }, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_and_names_are_qcviews() {
        assert_eq!(sanitize_media_name("a:b?c.mov"), "a_b_c.mov");
        assert_eq!(image_filename("01:00:00;12"), "note_01_00_00_12.png");
        assert_eq!(annotated_filename("00:00:01:01"), "note_00_00_01_01_annotated.png");
        assert_eq!(notes_dir(Path::new("/x/y/clip.mov")), PathBuf::from("/x/y/.qcview/clip.mov"));
        assert_eq!(media_type(Path::new("a.EXR")), "image"); assert_eq!(media_type(Path::new("a.mxf")), "video");
        assert_eq!(now_iso_utc().len(), 20);
    }
    /// `QCV_NOTES_FIXTURES` = a file listing real notes.json paths (one per line; they stay where they are): each loads and
    /// re-renders with its own header to the SAME bytes.
    #[test]
    fn real_sidecars_round_trip_byte_for_byte() {
        let Ok(list) = std::env::var("QCV_NOTES_FIXTURES") else { eprintln!("QCV_NOTES_FIXTURES unset: skipped"); return };
        let paths: Vec<String> = std::fs::read_to_string(list).unwrap().lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
        let mut n = 0;
        for (i, p) in paths.iter().enumerate() {
            let path = Path::new(p); let dir = path.parent().unwrap();
            let original = std::fs::read_to_string(path).unwrap();
            let (header, notes) = load_at(dir).unwrap();
            // the strokes parse (and re-encode to the same data)
            for nt in &notes { if let Some(a) = &nt.annotation { assert_eq!(crate::qt_json::to_qt_json(&crate::strokes::to_data(&crate::strokes::parse(Some(a)))), crate::qt_json::to_qt_json(a), "file {i}: strokes round-trip"); } }
            let out = render(&header, &notes, "unused");
            if out != original {
                let (a, b): (Vec<&str>, Vec<&str>) = (original.lines().collect(), out.lines().collect());
                let k = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
                panic!("file {i} differs at line {k}: original {:?} vs ours {:?}", a.get(k).map(|s| s.chars().take(80).collect::<String>()), b.get(k).map(|s| s.chars().take(80).collect::<String>()));
            }
            n += 1;
        }
        eprintln!("{n} / {} sidecars identical", paths.len());
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    #[test]
    fn stream_folders_are_qcviews() {
        assert_eq!(stream_folder_name(" QCBridge After Effects ", "0123456789abcdef"), "QCBridge After Effects__01234567");
        assert_eq!(stream_folder_name("a/b:c", "xy"), "a_b_c__xy");
        assert_eq!(stream_folder_name("  ", "abcdefghij"), "stream__abcdefgh");
        let root = std::env::temp_dir().join(format!("qcv-stream-io-{}", std::process::id())); let _ = std::fs::remove_dir_all(&root);
        let project = root.join("p.qcvproj");
        std::fs::create_dir_all(streams_root(&project).join("Old name__01234567")).unwrap();
        assert_eq!(find_stream_folder(&project, "0123456789"), Some("Old name__01234567".into()), "found by the id suffix after a rename");
        assert_eq!(stream_dir(&project, "New", "0123456789"), streams_root(&project).join("Old name__01234567"));
        assert_eq!(stream_dir(&project, "Other", "zzzzzzzz9"), streams_root(&project).join("Other__zzzzzzzz"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
