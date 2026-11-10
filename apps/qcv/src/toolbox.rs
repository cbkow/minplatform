//! QCView is the suite's ffmpeg provider (QCView `writeToolboxManifest` / `stageSpawnableToolsWin`, app/main.cpp): at start it
//! writes a small manifest of the ffmpeg / ffprobe it ships at a FIXED per-user path — a cross-app contract the QCBridge
//! Blender add-on reads (`qcbridge/ring1/toolbox.py`: preferences → this manifest → PATH) to find the CLI its replica
//! stream runs. The app itself never spawns them (it decodes in-process).
//!   macOS:   ~/Library/Application Support/QCView/toolbox.json   → the bundle's Contents/Helpers/{ffmpeg,ffprobe}
//!   Windows: %LOCALAPPDATA%\QCView\toolbox.json                  → the mirror %LOCALAPPDATA%\QCView\bin (other apps cannot
//!            spawn from a packaged app's WindowsApps folder), refreshed when the app's version changes; the app's own
//!            folder when the mirror cannot be made.
//! Only tools that exist are listed; when none do (a development run), an existing manifest is left alone — it may point
//! at an installed copy that works. Runs on its own thread (DIVERGENCE: QCView does it inline; the Windows mirror copies
//! the FFmpeg DLLs, ~100 MB, and nothing in the window waits for it).
use std::path::{Path, PathBuf};

pub fn publish() {
    let _ = std::thread::Builder::new().name("qcv-toolbox".into()).spawn(|| {
        let Some(app_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
        let Some((dir, ffmpeg, ffprobe)) = locate(&app_dir) else { return };
        write_manifest(&dir, &[("ffmpeg", ffmpeg), ("ffprobe", ffprobe)]);
    });
}

#[cfg(target_os = "macos")]
fn locate(app_dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let helpers = app_dir.join("../Helpers");
    Some((home.join("Library/Application Support/QCView"), helpers.join("ffmpeg"), helpers.join("ffprobe")))
}
#[cfg(windows)]
fn locate(app_dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("QCView");
    let bin = dir.join("bin");
    let from = if mirror(app_dir, &bin, env!("CARGO_PKG_VERSION")) { bin } else { app_dir.to_path_buf() };
    Some((dir, from.join("ffmpeg.exe"), from.join("ffprobe.exe")))
}
#[cfg(not(any(target_os = "macos", windows)))]
fn locate(_: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> { None }

/// The names QCView mirrors: the two CLIs and the FFmpeg DLLs beside them, by pattern (a major bump renames the DLLs).
fn is_tool_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "ffmpeg.exe" || n == "ffprobe.exe" || (n.ends_with(".dll") && ["avcodec-", "avformat-", "avutil-", "avfilter-", "avdevice-", "swresample-", "swscale-", "postproc-"].iter().any(|p| n.starts_with(p)))
}
fn is_ffmpeg_dll(name: &str) -> bool { let n = name.to_ascii_lowercase(); n.ends_with(".dll") && (n.starts_with("av") || n.starts_with("sw") || n.starts_with("postproc-")) }

/// Copy the tools from the app's folder into `bin` unless `bin` already holds this version's; true when `bin` is current.
/// A copy held open by a running spawn fails: the app-folder path is advertised and the next launch retries. FFmpeg DLLs
/// of a previous major are purged.
#[cfg_attr(not(windows), allow(dead_code))]
fn mirror(app_dir: &Path, bin: &Path, version: &str) -> bool {
    let stamp = bin.join(".version");
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == version) && bin.join("ffmpeg.exe").is_file() { return true }
    let names: Vec<String> = std::fs::read_dir(app_dir).into_iter().flatten().flatten().filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| is_tool_file(n)).collect();
    if !names.iter().any(|n| n.eq_ignore_ascii_case("ffmpeg.exe")) { return false }   // nothing shipped (a development run)
    if std::fs::create_dir_all(bin).is_err() { log::warn!("toolbox.json: cannot create {}", bin.display()); return false }
    let mut ok = true;
    for n in &names {
        let dst = bin.join(n); let _ = std::fs::remove_file(&dst);
        if std::fs::copy(app_dir.join(n), &dst).is_err() { log::warn!("toolbox.json: failed to mirror {n} into {}", bin.display()); ok = false; }
    }
    for e in std::fs::read_dir(bin).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if is_ffmpeg_dll(&n) && !names.iter().any(|x| x.eq_ignore_ascii_case(&n)) && std::fs::remove_file(e.path()).is_ok() { log::info!("toolbox.json: purged stale {n} from the mirror"); }
    }
    if ok { ok = std::fs::write(&stamp, version).is_ok(); if ok { log::info!("toolbox.json: the tool mirror is refreshed → {}", bin.display()); } }
    ok
}

fn write_manifest(dir: &Path, tools: &[(&str, PathBuf)]) {
    let mut map = serde_json::Map::new();
    for (key, path) in tools {
        if !path.is_file() { continue }
        let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()).to_string_lossy().into_owned();
        map.insert((*key).into(), serde_json::Value::String(p.strip_prefix(r"\\?\").unwrap_or(&p).to_string()));   // Windows: no verbatim prefix
    }
    if map.is_empty() { log::info!("toolbox.json: no shipped tools here — the manifest is left as it is"); return }
    if std::fs::create_dir_all(dir).is_err() { log::warn!("toolbox.json: cannot create {}", dir.display()); return }
    let (out, tmp) = (dir.join("toolbox.json"), dir.join("toolbox.json.tmp"));
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_default();
    match std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &out)) {
        Ok(()) => log::info!("toolbox.json written → {}", out.display()),
        Err(e) => log::warn!("toolbox.json: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mirror_copies_stamps_purges_and_the_manifest_lists_only_what_exists() {
        let root = std::env::temp_dir().join(format!("qcv-toolbox-{}", std::process::id()));
        let (app, bin) = (root.join("app"), root.join("data/bin"));
        std::fs::create_dir_all(&app).unwrap(); std::fs::create_dir_all(&bin).unwrap();
        assert!(!mirror(&app, &bin, "3.0.0"), "nothing shipped: no mirror");
        for n in ["ffmpeg.exe", "ffprobe.exe", "avcodec-63.dll", "swscale-10.dll", "QCView.exe", "OpenColorIO_2_6.dll"] { std::fs::write(app.join(n), n).unwrap(); }
        std::fs::write(bin.join("avcodec-62.dll"), "old").unwrap();
        assert!(mirror(&app, &bin, "3.0.0"));
        assert!(bin.join("ffmpeg.exe").is_file() && bin.join("avcodec-63.dll").is_file() && bin.join("swscale-10.dll").is_file());
        assert!(!bin.join("avcodec-62.dll").exists(), "a previous major's DLL is purged");
        assert!(!bin.join("QCView.exe").exists() && !bin.join("OpenColorIO_2_6.dll").exists(), "only the tools are mirrored");
        std::fs::remove_file(app.join("ffmpeg.exe")).unwrap();
        assert!(mirror(&app, &bin, "3.0.0"), "current: no copy needed");
        assert!(!mirror(&app, &bin, "3.0.1"), "a new version with nothing shipped: not current");
        let data = root.join("data");
        write_manifest(&data, &[("ffmpeg", bin.join("ffmpeg.exe")), ("ffprobe", root.join("missing"))]);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(data.join("toolbox.json")).unwrap()).unwrap();
        assert!(v["ffmpeg"].as_str().unwrap().ends_with("ffmpeg.exe") && v.get("ffprobe").is_none());
        std::fs::write(data.join("toolbox.json"), "{\"ffmpeg\":\"kept\"}").unwrap();
        write_manifest(&data, &[("ffmpeg", root.join("missing"))]);
        assert!(std::fs::read_to_string(data.join("toolbox.json")).unwrap().contains("kept"), "no tools: the manifest is left alone");
        let _ = std::fs::remove_dir_all(&root);
    }
}
