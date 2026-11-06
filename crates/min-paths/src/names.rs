//! Safe file names: ufb's sanitizer rules (`web_link_file_name`) plus the Windows reserved-name rule nobody in the family
//! had (row 123): `con.txt` can be created on a share from macOS and never opened on Windows.

/// Windows reserved device names, matched case-insensitively against the part of the name before its first `.`
/// (`con.txt`, `COM1.log` and `nul` are all reserved; `console` is not).
pub fn is_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end_matches([' ', '\t']);
    let up = stem.to_ascii_uppercase();
    match up.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        s if s.len() == 4 && (s.starts_with("COM") || s.starts_with("LPT")) && s.as_bytes()[3].is_ascii_digit() && s.as_bytes()[3] != b'0' => true,
        _ => false,
    }
}

/// A name that is safe on every OS in the family: `/ \ : * ? " < > |` and control characters become `-`, surrounding
/// whitespace and trailing dots go (Windows), and a reserved device name gets a `_` after its stem (`con.txt` → `con_.txt`).
/// Empty in → empty out (the caller decides the fallback).
pub fn sanitize_file_name(name: &str) -> String {
    let mut base: String = name.trim().chars().map(|c| match c {
        '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
        c if c.is_control() => '-',
        c => c,
    }).collect();
    base = base.trim().trim_end_matches('.').trim_end().to_string();
    if is_reserved_name(&base) {
        match base.find('.') { Some(i) => base.insert(i, '_'), None => base.push('_') }
    }
    base
}

/// ufb's: a user-typed link name → a safe `<name>.url` file name; a trailing `.url` is not doubled; an empty name falls
/// back to the URL's host (without `www.`), then `Link`.
pub fn web_link_file_name(name: &str, url: &str) -> String {
    let mut base = sanitize_file_name(name);
    if base.to_ascii_lowercase().ends_with(".url") { base.truncate(base.len() - 4); base = sanitize_file_name(&base); }
    if base.is_empty() {
        base = url.trim().trim_start_matches("https://").trim_start_matches("http://").split(['/', '?', '#']).next().unwrap_or("").trim_start_matches("www.").to_string();
        base = sanitize_file_name(&base);
    }
    if base.is_empty() { base = "Link".to_string(); }
    format!("{base}.url")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn reserved_names() {
        for n in ["con", "CON.txt", "com1", "LPT9.log", "nul", "aux.tar.gz", "prn "] { assert!(is_reserved_name(n), "{n}"); }
        for n in ["console", "com0", "com10", "lpt", "conn.txt", "", ".con"] { assert!(!is_reserved_name(n), "{n}"); }
    }
    #[test] fn sanitizer_rules_from_ufb() {
        assert_eq!(sanitize_file_name("  a/b\\c:d*e?f\"g<h>i|j\u{7}k. "), "a-b-c-d-e-f-g-h-i-j-k");
        assert_eq!(sanitize_file_name("con.txt"), "con_.txt");
        assert_eq!(sanitize_file_name("COM1"), "COM1_");
        assert_eq!(sanitize_file_name(""), "");
    }
    #[test] fn link_file_names_are_safe() {
        assert_eq!(web_link_file_name("My: Link?", "https://x.y/z"), "My- Link-.url");
        assert_eq!(web_link_file_name("site.URL", "https://x.y/z"), "site.url");
        assert_eq!(web_link_file_name("", "https://www.example.com/a?b#c"), "example.com.url");
        assert_eq!(web_link_file_name("  ", "nonsense"), "nonsense.url");
        assert_eq!(web_link_file_name("", ""), "Link.url");
        assert_eq!(web_link_file_name("con", "https://x"), "con_.url");
    }
}
