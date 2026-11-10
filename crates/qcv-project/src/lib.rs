//! `qcv-project` — QCView's project as a frozen contract (canon row 141): `.qcvproj` v2 byte-for-byte, the model, the
//! manager's rules. Nothing here invents: every rule names its line in `project_io.cpp` / `project_manager.cpp`.
pub mod model;
pub mod extensions;
pub mod io;
pub mod manager;
pub use model::*;
pub use io::{ProjectFile, SCHEMA_TAG, SCHEMA_VERSION, PROJECT_DIR_TOKEN, collapse_project_dir, expand_project_dir, to_qt_json};
pub use manager::{ProjectManager, Probe, now_iso_utc};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_dir_token_rules() {
        assert_eq!(collapse_project_dir("/a/b/c.mov", "/a/b"), "${PROJECT_DIR}/c.mov");
        assert_eq!(collapse_project_dir("/a/b", "/a/b"), "${PROJECT_DIR}");
        assert_eq!(collapse_project_dir("/a/barbaz/c.mov", "/a/bar"), "/a/barbaz/c.mov", "prefix + separator: no collision");
        assert_eq!(collapse_project_dir("srt://host:9000", "/a/b"), "srt://host:9000", "URLs untouched");
        assert_eq!(expand_project_dir("${PROJECT_DIR}/c.mov", "/x/y"), "/x/y/c.mov");
        assert_eq!(expand_project_dir("${PROJECT_DIR}", "/x/y"), "/x/y");
        assert_eq!(expand_project_dir("/abs/elsewhere.mov", "/x/y"), "/abs/elsewhere.mov");
    }
    #[test]
    fn qt_json_shape() {
        let v = serde_json::json!({ "z": 1, "a": [], "m": {}, "d": 24.0, "f": 23.976, "n": -1.0, "s": "x\"y", "b": true, "arr": [1, { "k": "v" }] });
        let out = to_qt_json(&io::to_value(&ProjectFile::default(), "/p", "T"));
        assert!(out.starts_with("{\n \"bins\": [],\n \"media\": [],\n \"project\": {\n  \"created_at\": \"\","), "{out}");
        assert!(!out.ends_with('\n'), "no trailing newline, as QCView's files");
        let mut s = String::new(); io::to_qt_json(&v).clone_into(&mut s);
        assert_eq!(s, "{\n \"a\": [],\n \"arr\": [\n  1,\n  {\n   \"k\": \"v\"\n  }\n ],\n \"b\": true,\n \"d\": 24.0,\n \"f\": 23.976,\n \"m\": {},\n \"n\": -1.0,\n \"s\": \"x\\\"y\",\n \"z\": 1\n}");
    }
    #[test]
    fn round_trip_and_migrations() {
        let mut pm = ProjectManager::new();
        assert_eq!(pm.bins().len(), 6); assert_eq!(pm.bins()[0].name, "Videos"); assert!(pm.bins()[0].is_open && !pm.bins()[1].is_open);
        let id = pm.add_dual_pair("", DualPairData::default());
        assert_eq!(pm.item(&id).unwrap().name, "Untitled Dual View"); assert_eq!(pm.bins()[4].item_ids, vec![id.clone()]);
        pm.set_rotation_override(&id, 45); assert_eq!(pm.item(&id).unwrap().rotation_override, -1, "off the quarter-turn set → Auto");
        pm.set_pixel_aspect(&id, PixelAspectMode::Custom, 4, 3); pm.set_video_range_override(&id, VideoRange::Full);
        let (pl, _) = pm.create_playlist(&[], "", 0.0, 0, 0, -1, 0);
        assert!(pm.item(&pl).unwrap().name.starts_with("Playlist-")); assert_eq!(pm.item(&pl).unwrap().playlist.master_fps, 24.0);
        assert_eq!(pm.next_playlist_auto_name(), "Playlist_01");
        let dir = std::env::temp_dir().join(format!("qcv-project-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.qcvproj").to_string_lossy().to_string();
        assert_eq!(pm.display_name(), "Untitled", "an unsaved project shows its stored name");
        pm.save_project(&path).unwrap();
        assert_eq!(pm.display_name(), std::path::Path::new(&path).file_stem().unwrap().to_string_lossy(), "a saved project shows its file's name");
        assert_eq!(pm.name(), "Untitled", "the stored name is QCView's, untouched");
        assert!(!pm.dirty());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"schema\": \"qcview.project.v2\"") && text.contains("\"videoRangeOverride\": \"full\"") && text.contains("\"pixelAspectMode\": 2") && !text.contains("transferOverride"), "{text}");
        let mut pm2 = ProjectManager::new(); pm2.open_project(&path).unwrap();
        assert_eq!(pm2.pool().len(), 2); assert_eq!(pm2.item(&id).unwrap().custom_par_num, 4); assert_eq!(pm2.uuid(), pm.uuid());
        // a second save is byte-identical apart from modified_at
        let a = text.replace(&pm.modified_at().to_string(), "T"); pm2.save_project(&path).unwrap(); let b = std::fs::read_to_string(&path).unwrap().replace(&pm2.modified_at().to_string(), "T");
        assert_eq!(a, b);
        // migrations: five bins → Live appended; no bins → the six; a dangling active id cleared; a non-v2 refused
        let five = r#"{"schema":"qcview.project.v2","version":2,"project":{"uuid":"u","name":"n"},"media":[],"bins":[{"name":"Videos","type":"Video","open":true,"item_ids":[]},{"name":"Audio","type":"Audio","open":false,"item_ids":[]},{"name":"Images","type":"Image","open":false,"item_ids":[]},{"name":"Playlists","type":"Playlist","open":false,"item_ids":[]},{"name":"Dual Views","type":"DualPair","open":false,"item_ids":[]}],"ui_state":{"active_item_id":"ghost"}}"#;
        let f = io::parse(five, "/p", "five.qcvproj").unwrap(); assert_eq!(f.bins.len(), 6); assert_eq!(f.bins[5].name, "Live"); assert!(f.active_item_id.is_empty());
        let none = io::parse(r#"{"schema":"qcview.project.v2","version":2}"#, "/p", "n.qcvproj").unwrap(); assert_eq!(none.bins.len(), 6);
        assert!(io::parse(r#"{"schema":"qcview.project.v1"}"#, "/p", "v1.qcvproj").is_err());
        // remove + restore in exact positions
        let n = pm2.remove_media_items(&[id.clone()]); assert_eq!(n, 1); assert!(pm2.item(&id).is_none()); assert!(pm2.bins()[4].item_ids.is_empty());
        let names = pm2.restore_last_removed(); assert_eq!(names, vec!["Untitled Dual View".to_string()]); assert_eq!(pm2.pool()[0].id, id); assert_eq!(pm2.bins()[4].item_ids, vec![id.clone()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// Byte-for-byte against the user's real projects: `QCV_PROJECT_FIXTURES_DIR` (local only) → every `.qcvproj` under it
    /// loads, saves to a temp file and compares equal apart from `modified_at`.
    #[test]
    fn real_projects_round_trip_byte_for_byte() {
        let Some(dir) = std::env::var_os("QCV_PROJECT_FIXTURES_DIR") else { eprintln!("QCV_PROJECT_FIXTURES_DIR unset — skipped"); return };
        let mut files = vec![]; fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) { if let Ok(rd) = std::fs::read_dir(d) { for e in rd.flatten() { let p = e.path(); if p.is_dir() { walk(&p, out); } else if p.extension().is_some_and(|x| x == "qcvproj") { out.push(p); } } } }
        walk(std::path::Path::new(&dir), &mut files);
        assert!(!files.is_empty(), "no .qcvproj under the fixtures dir");
        let tmp = std::env::temp_dir().join(format!("qcv-project-rt-{}", std::process::id())); std::fs::create_dir_all(&tmp).unwrap();
        let (mut ok, mut diffs) = (0, vec![]);
        for f in &files {
            let text = std::fs::read_to_string(f).unwrap();
            let pf = match io::parse(&text, &io::project_dir_of(&f.to_string_lossy()), &f.to_string_lossy()) { Ok(p) => p, Err(e) => { diffs.push(format!("{}: {e}", f.display())); continue } };
            // write with the FILE's own project dir so ${PROJECT_DIR} collapses the same way
            let out = to_qt_json(&io::to_value(&pf, &io::project_dir_of(&f.to_string_lossy()), &pf.modified_at));
            if out == text { ok += 1 } else {
                let (a, b): (Vec<&str>, Vec<&str>) = (text.lines().collect(), out.lines().collect());
                let first = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
                diffs.push(format!("{}: differs at line {} of {} vs {}: {:?} vs {:?}", f.file_name().unwrap().to_string_lossy(), first + 1, a.len(), b.len(), a.get(first).map(|s| s.chars().take(80).collect::<String>()), b.get(first).map(|s| s.chars().take(80).collect::<String>())));
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
        eprintln!("real projects: {ok} of {} byte-identical after load → save", files.len());
        assert!(diffs.is_empty(), "{ok} of {} identical; differences:\n{}", files.len(), diffs.join("\n"));
    }
}
