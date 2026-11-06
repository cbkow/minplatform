//! Clipboard file lists — ufb's `core/src/file_ops.rs` moved, not rewritten: one `NSURL` per file via `writeObjects:`
//! on macOS (the legacy `NSFilenamesPboardType` plist collapsed multi-file copies to ONE item), `CF_HDROP` +
//! `"Preferred DropEffect"` on Windows (Explorer's own Cut sets DROPEFFECT_MOVE, so Explorer → us cut-paste = move), the
//! cut registry keyed on the pasteboard change count / clipboard sequence number, and a `has_paths` probe that never
//! renders delayed clipboard data (a `GetClipboardData` round-trip to a busy owner stalls the GUI thread).
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteEffect { Copy, Move }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasteIntent { pub paths: Vec<String>, pub effect: PasteEffect }

struct LastCutRecord { paths: Vec<String>, sequence: i64 }
fn registry() -> &'static Mutex<Option<LastCutRecord>> { static R: OnceLock<Mutex<Option<LastCutRecord>>> = OnceLock::new(); R.get_or_init(|| Mutex::new(None)) }

/// The OS clipboard generation (monotonic per write): `GetClipboardSequenceNumber` / `NSPasteboard.changeCount`.
pub fn clipboard_sequence() -> i64 {
    #[cfg(windows)] { unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() as i64 } }
    #[cfg(target_os = "macos")] { objc2_app_kit::NSPasteboard::generalPasteboard().changeCount() as i64 }
    #[cfg(not(any(windows, target_os = "macos")))] { 0 }
}
fn record_cut(paths: &[String]) { let mut s = paths.to_vec(); s.sort(); *registry().lock().unwrap() = Some(LastCutRecord { paths: s, sequence: clipboard_sequence() }); }
fn clear_cut_record() { *registry().lock().unwrap() = None; }
/// True if our most recent cut still owns the clipboard (sequence unchanged) AND its path set equals `paths`.
fn registry_matches(paths: &[String]) -> bool {
    let r = registry().lock().unwrap(); let Some(rec) = r.as_ref() else { return false };
    if rec.sequence != clipboard_sequence() { return false }
    let mut s = paths.to_vec(); s.sort(); s == rec.paths
}

/// Does this string look like a filesystem path? (`X:\`, `X:/`, `\\`, `//`, or a leading `/`.)
pub fn looks_like_path(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') { return b[0].is_ascii_alphabetic() }
    s.starts_with("\\\\") || s.starts_with("//") || s.starts_with('/')
}

pub fn copy_paths(paths: &[String]) -> Result<(), String> {
    #[cfg(windows)] let r = win::set_paths(paths, 0x01);   // DROPEFFECT_COPY
    #[cfg(target_os = "macos")] let r = mac::copy_paths(paths);
    #[cfg(not(any(windows, target_os = "macos")))] let r = text::copy_text(&paths.join("\n"));
    clear_cut_record(); r
}
pub fn cut_paths(paths: &[String]) -> Result<(), String> {
    #[cfg(windows)] let r = win::set_paths(paths, 0x02);   // DROPEFFECT_MOVE
    #[cfg(target_os = "macos")] let r = mac::copy_paths(paths);   // no move flavour on the pasteboard: the registry says so
    #[cfg(not(any(windows, target_os = "macos")))] let r = text::copy_text(&paths.join("\n"));
    if r.is_ok() { record_cut(paths); } r
}

/// Paths on the clipboard: the native file list first, then path-looking lines of plain text.
pub fn paste_paths() -> Result<Vec<String>, String> {
    #[cfg(windows)] if let Ok(p) = win::get_paths() { if !p.is_empty() { return Ok(p) } }
    #[cfg(target_os = "macos")] if let Ok(p) = mac::paste_paths() { if !p.is_empty() { return Ok(p) } }
    let text = text::clipboard_text();
    Ok(text.lines().map(str::trim).filter(|l| !l.is_empty() && looks_like_path(l)).map(str::to_string).collect())
}

/// Paths + move-vs-copy in one pass (Windows: `Preferred DropEffect` is authoritative, the registry only when the format
/// is absent; macOS: `Move` only when WE cut these exact paths and nothing wrote since; text: always `Copy`).
pub fn paste_intent() -> Result<PasteIntent, String> {
    #[cfg(windows)] if let Ok(i) = win::paste_intent() { if !i.paths.is_empty() { return Ok(i) } }
    #[cfg(target_os = "macos")] if let Ok(paths) = mac::paste_paths() { if !paths.is_empty() { let effect = if registry_matches(&paths) { PasteEffect::Move } else { PasteEffect::Copy }; return Ok(PasteIntent { paths, effect }) } }
    Ok(PasteIntent { paths: paste_paths()?, effect: PasteEffect::Copy })
}

/// Cheap, non-blocking "could Paste do anything?" for menu enablement. Windows: `IsClipboardFormatAvailable` only
/// (CF_HDROP or CF_UNICODETEXT — text counts as "maybe", the paste becomes a no-op if it is not paths); elsewhere the
/// pasteboard read is cheap and `paste_intent` is used.
pub fn has_paths() -> bool {
    #[cfg(windows)] { unsafe { use windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable; IsClipboardFormatAvailable(15) != 0 || IsClipboardFormatAvailable(13) != 0 } }
    #[cfg(not(windows))] { paste_intent().map(|i| !i.paths.is_empty()).unwrap_or(false) }
}

/// Plain text via arboard (ufb's `clipboard_copy_text` / `clipboard_text`).
pub mod text {
    pub fn copy_text(s: &str) -> Result<(), String> { arboard::Clipboard::new().and_then(|mut c| c.set_text(s)).map_err(|e| e.to_string()) }
    pub fn clipboard_text() -> String { arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default() }
}
pub use text::{clipboard_text, copy_text};

#[cfg(target_os = "macos")]
mod mac {
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};
    /// One `NSURL` per file via `writeObjects:` — AppKit fans each out into a full item (`public.file-url` + the legacy
    /// flavours), so Finder and every modern app paste all of them. Paths are canonicalised when they exist.
    pub fn copy_paths(paths: &[String]) -> Result<(), String> {
        if paths.is_empty() { return Err("No valid file paths to copy".into()) }
        let urls: Vec<objc2::rc::Retained<NSURL>> = paths.iter().map(|p| {
            let r = std::fs::canonicalize(p).unwrap_or_else(|_| std::path::PathBuf::from(p));
            NSURL::fileURLWithPath(&NSString::from_str(&r.to_string_lossy()))
        }).collect();
        let objects: Vec<&ProtocolObject<dyn NSPasteboardWriting>> = urls.iter().map(|u| ProtocolObject::from_ref(&**u)).collect();
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        if pb.writeObjects(&NSArray::from_slice(&objects)) { Ok(()) } else { Err("Failed to write file paths to pasteboard".into()) }
    }
    /// `readObjectsForClasses:` — resolves every flavour a writer may have used (per-item file-url strings, legacy
    /// `NSFilenamesPboardType` plists, AppleScript `furl`, `file://localhost/`) into decoded POSIX paths.
    pub fn paste_paths() -> Result<Vec<String>, String> {
        let pb = NSPasteboard::generalPasteboard();
        let classes = NSArray::from_slice(&[objc2::class!(NSURL)]);
        let objects = unsafe { pb.readObjectsForClasses_options(&classes, None) };
        let mut out = Vec::new();
        if let Some(objects) = objects {
            for obj in objects.iter() {
                if let Some(url) = obj.downcast_ref::<NSURL>() { if url.isFileURL() { if let Some(p) = url.path() { out.push(p.to_string()); } } }
            }
        }
        Ok(out)
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL, POINT};
    use windows_sys::Win32::System::DataExchange::*;
    use windows_sys::Win32::System::Memory::*;
    use windows_sys::Win32::UI::Shell::{DragQueryFileW, DROPFILES};
    const CF_UNICODETEXT: u32 = 13; const CF_HDROP: u32 = 15;
    struct Open;
    impl Open {
        fn new() -> Result<Open, String> {
            for _ in 0..10 { if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 { return Ok(Open) } std::thread::sleep(std::time::Duration::from_millis(10)); }
            Err("Failed to open clipboard".into())
        }
    }
    impl Drop for Open { fn drop(&mut self) { unsafe { CloseClipboard(); } } }
    fn global_u16(words: &[u16], extra: usize) -> Result<HGLOBAL, String> {
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, extra + words.len() * 2);
            if h.is_null() { return Err("GlobalAlloc failed".into()) }
            let p = GlobalLock(h) as *mut u8; if p.is_null() { GlobalFree(h); return Err("GlobalLock failed".into()) }
            std::ptr::write_bytes(p, 0, extra);
            std::ptr::copy_nonoverlapping(words.as_ptr() as *const u8, p.add(extra), words.len() * 2);
            GlobalUnlock(h); Ok(h)
        }
    }
    pub fn set_paths(paths: &[String], effect: u32) -> Result<(), String> {
        let _c = Open::new()?;
        unsafe { if EmptyClipboard() == 0 { return Err("Failed to empty clipboard".into()) } }
        let mut words: Vec<u16> = Vec::new();
        for p in paths { words.extend(p.encode_utf16()); words.push(0); }
        words.push(0);
        let hdr = std::mem::size_of::<DROPFILES>();
        let h = global_u16(&words, hdr)?;
        unsafe {
            let p = GlobalLock(h) as *mut DROPFILES;
            if !p.is_null() { std::ptr::write(p, DROPFILES { pFiles: hdr as u32, pt: POINT { x: 0, y: 0 }, fNC: 0, fWide: 1 }); GlobalUnlock(h); }
            if SetClipboardData(CF_HDROP, h as _).is_null() { GlobalFree(h); return Err("Failed to set CF_HDROP".into()) }
            let fmt = RegisterClipboardFormatW("Preferred DropEffect\0".encode_utf16().collect::<Vec<_>>().as_ptr());
            if fmt != 0 { if let Ok(e) = global_u16(&[(effect & 0xffff) as u16, (effect >> 16) as u16], 0) { if SetClipboardData(fmt, e as _).is_null() { GlobalFree(e); } } }
        }
        Ok(())
    }
    fn read_hdrop_holding() -> Vec<String> {
        unsafe {
            let h = GetClipboardData(CF_HDROP); if h.is_null() { return vec![] }
            let n = DragQueryFileW(h as _, u32::MAX, std::ptr::null_mut(), 0);
            (0..n).filter_map(|i| { let len = DragQueryFileW(h as _, i, std::ptr::null_mut(), 0); let mut buf = vec![0u16; len as usize + 1]; let got = DragQueryFileW(h as _, i, buf.as_mut_ptr(), buf.len() as u32); (got > 0).then(|| String::from_utf16_lossy(&buf[..got as usize])) }).filter(|s| !s.is_empty()).collect()
        }
    }
    fn read_drop_effect_holding() -> Option<bool> {
        unsafe {
            let fmt = RegisterClipboardFormatW("Preferred DropEffect\0".encode_utf16().collect::<Vec<_>>().as_ptr()); if fmt == 0 { return None }
            let h = GetClipboardData(fmt); if h.is_null() { return None }
            let p = GlobalLock(h as HGLOBAL) as *const u32; if p.is_null() { return None }
            let e = std::ptr::read_unaligned(p); GlobalUnlock(h as HGLOBAL);
            Some(e & 0x02 != 0)
        }
    }
    pub fn get_paths() -> Result<Vec<String>, String> { let _c = Open::new()?; Ok(read_hdrop_holding()) }
    pub fn paste_intent() -> Result<PasteIntent, String> {
        let _c = Open::new()?;
        let paths = read_hdrop_holding();
        if paths.is_empty() { return Ok(PasteIntent { paths, effect: PasteEffect::Copy }) }
        let effect = match read_drop_effect_holding() { Some(true) => PasteEffect::Move, Some(false) => PasteEffect::Copy, None => if registry_matches(&paths) { PasteEffect::Move } else { PasteEffect::Copy } };
        Ok(PasteIntent { paths, effect })
    }
    #[allow(dead_code)] const _: u32 = CF_UNICODETEXT;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn path_heuristic() { for s in ["C:\\x", "d:/y", "\\\\srv\\s", "//srv/s", "/Users"] { assert!(looks_like_path(s), "{s}"); } for s in ["x", "1:\\", "http://x", ""] { assert!(!looks_like_path(s), "{s}"); } }
    #[test] fn registry_round_trip_without_touching_the_clipboard() {
        let paths = vec!["/b".to_string(), "/a".to_string()];
        record_cut(&paths);
        assert!(registry_matches(&["/a".to_string(), "/b".to_string()]));
        assert!(!registry_matches(&["/a".to_string()]));
        clear_cut_record(); assert!(!registry_matches(&paths));
    }
}
