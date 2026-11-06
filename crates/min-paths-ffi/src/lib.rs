//! The C ABI of `min-paths` (canon §22 row 125: minNotes is the second consumer, through this). Conventions: every input is a
//! NUL-terminated UTF-8 string (NULL is treated as empty); every `char*` returned is a fresh Rust allocation the caller
//! releases with [`mp_free`]; booleans are `int` 0/1; "no result" is an empty string, never NULL (except on allocation
//! failure, which never happens in practice). The active mapping table is process-global, like minNotes' `g_mappings`.
#![allow(clippy::missing_safety_doc)]
use min_paths::map::{Mapping, Ref};
use min_paths::Os;
use std::ffi::{c_char, c_int, CStr, CString};
use std::sync::Mutex;

static MAPPINGS: Mutex<Vec<Mapping>> = Mutex::new(Vec::new());

fn arg(p: *const c_char) -> String {
    if p.is_null() { return String::new() }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}
fn out(s: String) -> *mut c_char { CString::new(s.replace('\0', "")).map(CString::into_raw).unwrap_or(std::ptr::null_mut()) }
fn json<T: serde::Serialize>(v: &T) -> *mut c_char { out(serde_json::to_string(v).unwrap_or_default()) }

/// Release a string returned by any `mp_*` function.
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_free(s: *mut c_char) { if !s.is_null() { drop(unsafe { CString::from_raw(s) }); } }

/// The crate version, e.g. "0.1.0".
#[unsafe(no_mangle)] pub extern "C" fn mp_version() -> *mut c_char { out(env!("CARGO_PKG_VERSION").into()) }

/// The OS tag of this build: "win", "mac" or "lin".
#[unsafe(no_mangle)] pub extern "C" fn mp_os_tag() -> *mut c_char { out(Os::current().tag().into()) }

// ── the mapping table (minNotes' PathMap) ──
/// Replace the active mapping table with a JSON array of `{id, label, win, mac, lin, enabled}` rows (missing fields default;
/// `enabled` defaults to true). Returns the number of rows accepted, or -1 for bad JSON (the table is then unchanged).
#[unsafe(no_mangle)] pub extern "C" fn mp_mappings_set(json_rows: *const c_char) -> c_int {
    match serde_json::from_str::<Vec<Mapping>>(&arg(json_rows)) {
        Ok(rows) => { let n = rows.len() as c_int; *MAPPINGS.lock().unwrap() = rows; n }
        Err(_) => -1,
    }
}
/// minNotes' `toRef`: an absolute path → its JSON ref: `{"vol":"<id>","rel":["seg",…]}` inside a mapped volume, else the
/// path as a JSON string (machine-local). Empty in → `""` (a JSON string).
#[unsafe(no_mangle)] pub extern "C" fn mp_to_ref(abs_path: *const c_char) -> *mut c_char {
    json(&min_paths::map::to_ref(&arg(abs_path), Os::current(), &MAPPINGS.lock().unwrap()))
}
/// minNotes' `resolveRef`: a JSON ref (object or string) → the path on this OS with `/` separators, or "" when the volume is
/// unknown, disabled or has no root here. A string ref gets the legacy cross-OS swap, or passes through unchanged.
#[unsafe(no_mangle)] pub extern "C" fn mp_resolve_ref(ref_json: *const c_char) -> *mut c_char {
    let r: Ref = match serde_json::from_str(&arg(ref_json)) { Ok(r) => r, Err(_) => return out(String::new()) };
    out(min_paths::map::resolve_ref(&r, Os::current(), &MAPPINGS.lock().unwrap()).unwrap_or_default())
}
/// ufb's / MinRender's string translation between OS tags ("win" | "mac" | "lin") over the active table.
#[unsafe(no_mangle)] pub extern "C" fn mp_translate(src_os: *const c_char, dst_os: *const c_char, path: *const c_char) -> *mut c_char {
    let (s, d, p) = (arg(src_os), arg(dst_os), arg(path));
    match (Os::from_tag(&s), Os::from_tag(&d)) { (Some(a), Some(b)) => out(min_paths::map::translate(a, b, &p, &MAPPINGS.lock().unwrap())), _ => out(p) }
}

// ── classifiers (minNotes' MediaStore) ──
/// 1 when the path lives on a network share (UNC / DRIVE_REMOTE / smb-afp-nfs-cifs-dav-sshfs-ftp mount); cannot tell → 0.
#[unsafe(no_mangle)] pub extern "C" fn mp_is_network_source(path: *const c_char) -> c_int { min_paths::classify::is_network_source(&arg(path)) as c_int }
/// 1 when the path lives somewhere that deletes its own contents (temp dirs, ~/Library, %TEMP% …, plus `extra_root` — the
/// app's own scratch dir — when non-empty).
#[unsafe(no_mangle)] pub extern "C" fn mp_is_volatile_source(path: *const c_char, extra_root: *const c_char) -> c_int {
    let extra = arg(extra_root);
    let extras: Vec<std::path::PathBuf> = if extra.is_empty() { vec![] } else { vec![extra.into()] };
    min_paths::classify::is_volatile_source(&arg(path), &extras) as c_int
}
/// The host a share path is served from ("" when local / unknown).
#[unsafe(no_mangle)] pub extern "C" fn mp_share_host_for_path(path: *const c_char) -> *mut c_char { out(min_paths::classify::share_host_for_path(&arg(path)).unwrap_or_default()) }

// ── file URLs + uri-lists ──
/// A local path → `file://` URL, or "" when it cannot be expressed safely (a drive-less Windows path).
#[unsafe(no_mangle)] pub extern "C" fn mp_path_to_file_url(path: *const c_char) -> *mut c_char { out(min_paths::url::path_to_file_url(&arg(path)).unwrap_or_default()) }
/// A `file://` URL → native path ("" when not a file URL). Handles `file:///C:/…`, `file://host/share`, `file://localhost/`.
#[unsafe(no_mangle)] pub extern "C" fn mp_file_url_to_path(url: *const c_char) -> *mut c_char { out(min_paths::url::file_url_to_path(&arg(url)).unwrap_or_default()) }
/// JSON array of paths → a `text/uri-list` (CRLF after every entry).
#[unsafe(no_mangle)] pub extern "C" fn mp_uri_list_encode(json_paths: *const c_char) -> *mut c_char {
    let paths: Vec<String> = serde_json::from_str(&arg(json_paths)).unwrap_or_default();
    out(min_paths::url::uri_list::encode(&paths))
}
/// A `text/uri-list` → JSON array of native paths (non-file URIs dropped).
#[unsafe(no_mangle)] pub extern "C" fn mp_uri_list_decode_paths(text: *const c_char) -> *mut c_char { json(&min_paths::url::uri_list::decode_paths(&arg(text))) }

// ── family links ──
/// `ufb:///{win|mac}/<path>` ("" for an empty path).
#[unsafe(no_mangle)] pub extern "C" fn mp_ufb_link(path: *const c_char) -> *mut c_char { out(min_paths::links::ufb_link(&arg(path))) }
/// `minnotes:///…` ("" when the path cannot be expressed).
#[unsafe(no_mangle)] pub extern "C" fn mp_minnotes_link(path: *const c_char) -> *mut c_char { out(min_paths::links::minnotes_link(&arg(path)).unwrap_or_default()) }
/// A `minnotes:` link → native path ("" when it is not one).
#[unsafe(no_mangle)] pub extern "C" fn mp_parse_minnotes_link(uri: *const c_char) -> *mut c_char { out(min_paths::links::parse_minnotes_link(&arg(uri)).unwrap_or_default()) }

// ── names ──
/// A file name safe on every OS in the family (separators and control characters → "-", trailing dots trimmed, Windows
/// reserved names suffixed with "_").
#[unsafe(no_mangle)] pub extern "C" fn mp_sanitize_file_name(name: *const c_char) -> *mut c_char { out(min_paths::names::sanitize_file_name(&arg(name))) }
/// 1 for a Windows reserved device name (CON, PRN, AUX, NUL, COM1–9, LPT1–9, any extension).
#[unsafe(no_mangle)] pub extern "C" fn mp_is_reserved_name(name: *const c_char) -> c_int { min_paths::names::is_reserved_name(&arg(name)) as c_int }

// ── volumes ──
/// The path another process must be handed (MSIX LocalCache redirection resolved, `\\?\` prefixes stripped); unchanged
/// off Windows.
#[unsafe(no_mangle)] pub extern "C" fn mp_external_path(path: *const c_char) -> *mut c_char { out(min_paths::volume::external_path(std::path::Path::new(&arg(path))).to_string_lossy().into_owned()) }
/// 1 when both paths live on the same volume (device identity).
#[unsafe(no_mangle)] pub extern "C" fn mp_same_volume(a: *const c_char, b: *const c_char) -> c_int { min_paths::volume::same_volume(std::path::Path::new(&arg(a)), std::path::Path::new(&arg(b))) as c_int }

// ── sequences (QCView's rules) ──
#[derive(serde::Serialize)]
struct SeqOut { directory: String, base: String, sep: String, ext: String, padding: usize, start: i32, end: i32, missing: Vec<i32>, pattern: String, pattern_hashes: String, base_label: String, match_count: usize }
/// Detect the image sequence a file belongs to (its directory is listed): JSON `{directory, base, sep, ext, padding, start,
/// end, missing:[…], pattern:"name_%04d.png", pattern_hashes:"name_####.png", base_label, match_count}` or "" when it is
/// not a sequence (fewer than 3 digits, fewer than 2 siblings, mixed padding).
#[unsafe(no_mangle)] pub extern "C" fn mp_sequence_detect(path: *const c_char) -> *mut c_char {
    match min_paths::sequence::detect_on_disk(std::path::Path::new(&arg(path))) {
        Some(s) => json(&SeqOut { pattern_hashes: s.pattern_hashes(), directory: s.directory, base: s.base, sep: s.sep, ext: s.ext, padding: s.padding, start: s.start, end: s.end, missing: s.missing, pattern: s.pattern, base_label: s.base_label, match_count: s.match_count }),
        None => out(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(p: *mut c_char) -> String { let r = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned(); unsafe { mp_free(p) }; r }
    fn c(x: &str) -> CString { CString::new(x).unwrap() }
    #[test] fn refs_round_trip_through_the_abi() {
        let rows = c(r#"[{"id":"01J","label":"jobs","win":"Z:\\union-jobs","mac":"/Volumes/union-jobs","lin":"/mnt/union-jobs"}]"#);
        assert_eq!(mp_mappings_set(rows.as_ptr()), 1);
        let (mac_path, win_path) = ("/Volumes/union-jobs/2026/a.png", "Z:/union-jobs/2026/a.png");
        let abs = c(if Os::current().is_windows() { win_path } else { mac_path });
        let r = s(mp_to_ref(abs.as_ptr()));
        assert_eq!(r, r#"{"vol":"01J","rel":["2026","a.png"]}"#);
        let back = s(mp_resolve_ref(c(&r).as_ptr()));
        assert_eq!(back, if Os::current().is_windows() { win_path } else { mac_path });
        assert_eq!(s(mp_to_ref(c("/elsewhere/x.png").as_ptr())), r#""/elsewhere/x.png""#);
        assert_eq!(s(mp_resolve_ref(c(r#"{"vol":"nope","rel":[]}"#).as_ptr())), "");
        assert_eq!(s(mp_resolve_ref(c("not json").as_ptr())), "");
        assert_eq!(mp_mappings_set(c("[not json").as_ptr()), -1);
        assert_eq!(s(mp_to_ref(abs.as_ptr())), r#"{"vol":"01J","rel":["2026","a.png"]}"#);   // table unchanged after bad JSON
    }
    #[test] fn urls_links_names_through_the_abi() {
        assert_eq!(s(mp_file_url_to_path(c("file:///a%20b/c.png").as_ptr())), if Os::current().is_windows() { "\\a b\\c.png" } else { "/a b/c.png" });
        assert_eq!(s(mp_uri_list_decode_paths(c("file:///a\r\n# x\r\nhttps://q\r\n").as_ptr())), if Os::current().is_windows() { r#"["\\a"]"# } else { r#"["/a"]"# });
        assert_eq!(s(mp_sanitize_file_name(c("con.txt").as_ptr())), "con_.txt");
        assert_eq!(mp_is_reserved_name(c("NUL").as_ptr()), 1);
        assert!(s(mp_ufb_link(c("/x/y").as_ptr())).starts_with("ufb:///"));
        assert_eq!(s(mp_version()), env!("CARGO_PKG_VERSION"));
        assert_eq!(s(mp_sequence_detect(c("/nowhere/a_001.png").as_ptr())), "");
        assert_eq!(mp_is_network_source(std::ptr::null()), 0);
        unsafe { mp_free(std::ptr::null_mut()); }
    }
}
