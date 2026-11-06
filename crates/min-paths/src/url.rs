//! `file://` URLs, the one percent codec, and `text/uri-list` (RFC 2483). ufb's `path_to_file_url` / `percent_encode_path`
//! (`core/src/utils.rs`) and `file_url_to_path` / `percent_decode` (`bindings/src/services/file_ops.rs`) ported, with the two
//! decode gaps the survey found closed: `file://host/share` → a UNC path, `file://localhost/x` → `/x`. OS-parametrised so
//! the other platform's form is testable here.
use crate::{has_drive, Os};

/// Percent-encode a path's bytes for a `file://` URL: ASCII alphanumerics and `/ : - _ . ~` stay literal, everything else
/// (spaces, `#`, `%`, `?`, non-ASCII UTF-8 bytes…) becomes `%XX` in uppercase hex. Separators are the caller's business.
pub fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') { out.push(byte as char) }
        else { out.push_str(&format!("%{byte:02X}")) }
    }
    out
}

/// Percent-decode bytewise (so UTF-8 passes through); a malformed escape (`%4`, `%zz`, trailing `%`) is kept as-is;
/// invalid UTF-8 after decoding is replaced lossily. `+` is NOT a space (this is a path codec, not a form codec).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len()
            && let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push(((h << 4) | l) as u8); i += 3; continue;
            }
        out.push(b[i]); i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A local path → a `file://` URL for a `text/uri-list` payload, or `None` for a path that cannot be expressed safely.
///
/// The critical case (ufb, 2025): a drive-less Windows path (`\Volumes\X`) emitted naively becomes `file:////Volumes/X`
/// (four slashes), which Windows reads as the UNC `\\Volumes\X` — a non-existent host whose failed resolution wedges
/// system-wide OLE drag-and-drop until a reboot. Dropping the entry beats poisoning the drag. A real UNC path is emitted
/// in the proper `file://server/share/…` host form; a drive path as `file:///C:/…`; a POSIX absolute path as `file:///…`.
pub fn path_to_file_url_for(path: &str, os: Os) -> Option<String> {
    if os.is_windows() {
        let n = path.replace('\\', "/");
        if let Some(rest) = n.strip_prefix("//") {
            if rest.is_empty() { return None }
            return Some(format!("file://{}", percent_encode_path(rest)));
        }
        if has_drive(&n) { return Some(format!("file:///{}", percent_encode_path(&n))) }
        None
    } else if path.starts_with('/') {
        Some(format!("file://{}", percent_encode_path(path)))
    } else {
        Some(format!("file:///{}", percent_encode_path(path)))
    }
}
pub fn path_to_file_url(path: &str) -> Option<String> { path_to_file_url_for(path, Os::current()) }

/// A `file://` URL → a local path in `os`'s native form, or `None` for a non-file URL or an empty path.
///
/// Shapes (QCView's `urlToOsPath` list + the two ufb missed): `file:///C:/x` → `C:\x`; `file:///p` → `/p`;
/// `file://localhost/p` → `/p` (the Finder / AppleScript form); `file://host/share/p` → `\\host\share\p` on Windows and
/// `//host/share/p` elsewhere (QCView's UNC convention). The scheme is matched case-insensitively.
pub fn file_url_to_path_for(url: &str, os: Os) -> Option<String> {
    let rest = url.get(..7).filter(|p| p.eq_ignore_ascii_case("file://")).map(|_| &url[7..])?;
    let body: String = if let Some(local) = rest.strip_prefix('/') {
        // Empty authority: `file:///…`
        if os.is_windows() && has_drive(local) { local.to_string() } else { format!("/{local}") }
    } else {
        // Non-empty authority: `file://host/…`
        let (host, tail) = rest.split_once('/').map(|(h, t)| (h, format!("/{t}"))).unwrap_or((rest, String::new()));
        if host.eq_ignore_ascii_case("localhost") || host.is_empty() {
            if os.is_windows() { tail.strip_prefix('/').filter(|t| has_drive(t)).map(str::to_string).unwrap_or(tail) } else { tail }
        } else {
            format!("//{host}{tail}")
        }
    };
    let decoded = percent_decode(&body);
    if decoded.is_empty() { return None }
    Some(if os.is_windows() { decoded.replace('/', "\\") } else { decoded })
}
pub fn file_url_to_path(url: &str) -> Option<String> { file_url_to_path_for(url, Os::current()) }

/// `text/uri-list` (RFC 2483).
pub mod uri_list {
    use super::*;

    /// Encode paths as a `text/uri-list`: one `file://` URL per line, CRLF after EVERY entry including the last — some Qt
    /// code paths drop the final URL when translating to CF_HDROP without it (multi-select drag-out carried one file).
    /// Paths `path_to_file_url_for` refuses are skipped (never emit a four-slash URL).
    pub fn encode_for<S: AsRef<str>>(paths: &[S], os: Os) -> String {
        let mut out = String::new();
        for p in paths { if let Some(u) = path_to_file_url_for(p.as_ref(), os) { out.push_str(&u); out.push_str("\r\n"); } }
        out
    }
    pub fn encode<S: AsRef<str>>(paths: &[S]) -> String { encode_for(paths, Os::current()) }

    /// Decode a `text/uri-list` into its URIs: CRLF, LF or CR line ends, entries trimmed, blank lines and `#` comments
    /// dropped. (The two QML copies in ufb decoded this by hand because Qt 6 truncates `drop.urls` to one entry.)
    pub fn decode(text: &str) -> Vec<String> {
        text.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect()
    }

    /// Decode straight to local paths: non-file URIs are skipped.
    pub fn decode_paths_for(text: &str, os: Os) -> Vec<String> {
        decode(text).iter().filter_map(|u| file_url_to_path_for(u, os)).collect()
    }
    pub fn decode_paths(text: &str) -> Vec<String> { decode_paths_for(text, Os::current()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── path_to_file_url — ufb's tests, now run on every OS ──
    #[test] fn file_url_drops_driveless_windows_path() { assert_eq!(path_to_file_url_for("\\Volumes\\studio-nas\\x", Os::Win), None); }
    #[test] fn file_url_keeps_drive_letter_path() { assert_eq!(path_to_file_url_for("C:\\Users\\alice\\file.txt", Os::Win).as_deref(), Some("file:///C:/Users/alice/file.txt")); }
    #[test] fn file_url_unc_path_uses_host_form() { assert_eq!(path_to_file_url_for("\\\\server\\share\\f", Os::Win).unwrap(), "file://server/share/f"); }
    #[test] fn file_url_never_emits_four_slashes() {
        for p in ["\\Volumes\\x", "C:\\ok", "\\\\srv\\sh\\f", "rel\\x", "//", "\\\\"] {
            if let Some(u) = path_to_file_url_for(p, Os::Win) { assert!(!u.starts_with("file:////"), "4-slash leaked for {p:?}: {u}"); }
        }
    }
    #[test] fn file_url_mac_absolute_path() { assert_eq!(path_to_file_url_for("/Users/alice/a b#1.png", Os::Mac).unwrap(), "file:///Users/alice/a%20b%231.png"); }
    #[test] fn file_url_mac_relative_gets_three_slashes() { assert_eq!(path_to_file_url_for("rel/x", Os::Mac).unwrap(), "file:///rel/x"); }
    #[test] fn file_url_posix_backslash_is_encoded_not_a_separator() { assert_eq!(path_to_file_url_for("/a\\b", Os::Mac).unwrap(), "file:///a%5Cb"); }
    #[test] fn percent_encodes_utf8_bytes_uppercase() { assert_eq!(percent_encode_path("é"), "%C3%A9"); assert_eq!(percent_decode("%C3%A9"), "é"); }

    // ── file_url_to_path — the survey found this untested; the host and localhost forms were gaps ──
    #[test] fn url_to_path_windows_drive() { assert_eq!(file_url_to_path_for("file:///C:/a%20b/c.mov", Os::Win).unwrap(), "C:\\a b\\c.mov"); }
    #[test] fn url_to_path_windows_unc_host_form() { assert_eq!(file_url_to_path_for("file://server/share/f", Os::Win).unwrap(), "\\\\server\\share\\f"); }
    #[test] fn url_to_path_windows_four_slash_legacy() { assert_eq!(file_url_to_path_for("file:////srv/sh/f", Os::Win).unwrap(), "\\\\srv\\sh\\f"); }
    #[test] fn url_to_path_posix() { assert_eq!(file_url_to_path_for("file:///Users/x/%C3%A9.png", Os::Mac).unwrap(), "/Users/x/é.png"); }
    #[test] fn url_to_path_localhost() {
        assert_eq!(file_url_to_path_for("file://localhost/Users/x/a.png", Os::Mac).unwrap(), "/Users/x/a.png");
        assert_eq!(file_url_to_path_for("file://localhost/C:/x/a.png", Os::Win).unwrap(), "C:\\x\\a.png");
    }
    #[test] fn url_to_path_host_form_on_posix_is_qcviews_unc() { assert_eq!(file_url_to_path_for("file://nas/share/f", Os::Mac).unwrap(), "//nas/share/f"); }
    #[test] fn url_to_path_rejects_non_file() { assert_eq!(file_url_to_path_for("https://x/y", Os::Mac), None); assert_eq!(file_url_to_path_for("file://", Os::Mac), None); }
    #[test] fn url_to_path_scheme_case_insensitive() { assert_eq!(file_url_to_path_for("FILE:///a", Os::Mac).unwrap(), "/a"); }
    #[test] fn percent_decode_keeps_malformed_escapes() { assert_eq!(percent_decode("a%4"), "a%4"); assert_eq!(percent_decode("%zz%41"), "%zzA"); assert_eq!(percent_decode("x%"), "x%"); }
    #[test] fn round_trip_posix_and_windows() {
        for (p, os) in [("/Volumes/jobs/é/#1 [v2].png", Os::Mac), ("C:\\jobs\\a b\\%.png", Os::Win), ("\\\\nas\\share\\x y\\f.mov", Os::Win)] {
            let u = path_to_file_url_for(p, os).unwrap();
            assert_eq!(file_url_to_path_for(&u, os).unwrap(), p, "via {u}");
        }
    }

    // ── uri-list ──
    #[test] fn uri_list_crlf_after_every_entry_including_the_last() {
        assert_eq!(uri_list::encode_for(&["/a", "/b c"], Os::Mac), "file:///a\r\nfile:///b%20c\r\n");
        assert_eq!(uri_list::encode_for(&["\\Volumes\\x", "C:\\y"], Os::Win), "file:///C:/y\r\n");
    }
    #[test] fn uri_list_decode_tolerates_every_line_end_and_comments() {
        let t = "# comment\r\nfile:///a\nfile:///b%20c\r\r\n\n  file:///d  \r\nhttps://x/y";
        assert_eq!(uri_list::decode(t), ["file:///a", "file:///b%20c", "file:///d", "https://x/y"]);
        assert_eq!(uri_list::decode_paths_for(t, Os::Mac), ["/a", "/b c", "/d"]);
    }
    #[test] fn uri_list_missing_final_crlf_still_keeps_last() { assert_eq!(uri_list::decode_paths_for("file:///a\r\nfile:///b", Os::Mac), ["/a", "/b"]); }
}
