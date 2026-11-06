//! The family's link schemes (row 52's `text/plain` link line, row 126): `ufb:///{win|mac}/<path>` (ufb's own scheme; minNotes
//! builds it too), `minnotes:<file-url-body>`, `qcview://<project-uuid>[/<media-id>]` and `qcview://stream?url=…[&name=…]`.
use crate::url::{file_url_to_path_for, path_to_file_url_for, percent_decode};
use crate::Os;

/// `ufb:///win/C%3A/Users/x` · `ufb:///mac//Volumes/jobs/%C3%A9` — ufb and minNotes agree on the shape and the strict
/// codec (`:` → `%3A`) and disagree only on `/` (ufb's `urlencoding::encode` encodes it, minNotes keeps it). Both parsers
/// decode either; we emit minNotes' readable form (`/` literal). The OS tag is `win` or `mac` (minNotes tags Linux as
/// `mac` too, since ufb only knows the two).
pub fn ufb_link_for(path: &str, os: Os) -> String {
    if path.is_empty() { return String::new() }
    let norm = path.replace('\\', "/");
    format!("ufb:///{}/{}", if os.is_windows() { "win" } else { "mac" }, encode_link(&norm))
}
/// The link codec both older apps use (minNotes `toPercentEncoding(norm, "/")`, ufb `urlencoding::encode`): only the RFC
/// 3986 unreserved set and `/` stay literal — a drive colon becomes `%3A`, unlike the file-URL codec.
fn encode_link(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') { out.push(b as char) } else { out.push_str(&format!("%{b:02X}")) }
    }
    out
}
pub fn ufb_link(path: &str) -> String { ufb_link_for(path, Os::current()) }

/// ufb's `parse_path_uri`: `ufb:///` or `union:///`, an optional `win/` | `mac/` prefix (a legacy link without one is the
/// current OS), the rest percent-decoded. Returns `(source_os, path with / separators)`.
pub fn parse_ufb_link(uri: &str) -> Option<(Os, String)> {
    let rest = uri.strip_prefix("ufb:///").or_else(|| uri.strip_prefix("union:///"))?;
    let (os, enc) = if let Some(r) = rest.strip_prefix("win/") { (Os::Win, r) } else if let Some(r) = rest.strip_prefix("mac/") { (Os::Mac, r) } else { (Os::current(), rest) };
    Some((os, percent_decode(enc)))
}

/// `minnotes:///Users/x/doc.minnotes` · `minnotes:///C:/x/doc.minnotes` — a file URL with the scheme swapped (minNotes'
/// `resolveAndOpen` reads it with `QUrl::toLocalFile`, falling back to the URL path).
pub fn minnotes_link_for(path: &str, os: Os) -> Option<String> { path_to_file_url_for(path, os).map(|u| format!("minnotes{}", &u[4..])) }
pub fn minnotes_link(path: &str) -> Option<String> { minnotes_link_for(path, Os::current()) }
pub fn parse_minnotes_link_for(uri: &str, os: Os) -> Option<String> {
    let rest = uri.get(..9).filter(|p| p.eq_ignore_ascii_case("minnotes:")).map(|_| &uri[9..])?;
    file_url_to_path_for(&format!("file:{rest}"), os)
}
pub fn parse_minnotes_link(uri: &str) -> Option<String> { parse_minnotes_link_for(uri, Os::current()) }

/// QCView's deep links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QcviewLink {
    /// `qcview://<project-uuid>[/<media-id>]` ("Copy Project Link"; a bare `uuid/mediaId` without the scheme is accepted).
    Project { uuid: String, media_id: Option<String> },
    /// `qcview://stream?url=<pct-encoded>[&name=…][&<chain keys>…]` (QCBridge 0.3's "open in QCView"); `query` keeps every
    /// pair decoded, in order, for `applyStreamChainFromLink`.
    Stream { url: String, name: Option<String>, query: Vec<(String, String)> },
}

pub fn qcview_project_link(uuid: &str, media_id: &str) -> Option<String> {
    if uuid.is_empty() || media_id.is_empty() { None } else { Some(format!("qcview://{uuid}/{media_id}")) }
}

/// QCView's `openProjectLink` parse, without the project lookup.
pub fn parse_qcview_link(uri: &str) -> Option<QcviewLink> {
    let t = uri.trim();
    let body = t.get(..9).filter(|p| p.eq_ignore_ascii_case("qcview://")).map(|_| &t[9..]).unwrap_or(t);
    if body.is_empty() { return None }
    if body.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("stream")) {
        let q = body.split_once('?').map(|(_, q)| q).unwrap_or("");
        let query: Vec<(String, String)> = q.split('&').filter(|kv| !kv.is_empty()).map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(&k.replace('+', " ")), percent_decode(&v.replace('+', " ")))
        }).collect();
        let url = query.iter().find(|(k, _)| k == "url").map(|(_, v)| v.trim().to_string()).filter(|u| !u.is_empty())?;
        let name = query.iter().find(|(k, _)| k == "name").map(|(_, v)| v.clone());
        return Some(QcviewLink::Stream { url, name, query });
    }
    let (uuid, media) = body.split_once('/').map(|(u, m)| (u, Some(m))).unwrap_or((body, None));
    if uuid.is_empty() { return None }
    Some(QcviewLink::Project { uuid: uuid.to_string(), media_id: media.filter(|m| !m.is_empty()).map(str::to_string) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn ufb_links_match_minnotes_regression_129() {
        assert_eq!(ufb_link_for("C:\\a b\\c.mov", Os::Win), "ufb:///win/C%3A/a%20b/c.mov");   // minNotes regression [129]
        assert_eq!(ufb_link_for("/Volumes/jobs/é/#1.png", Os::Mac), "ufb:///mac//Volumes/jobs/%C3%A9/%231.png");
        assert_eq!(ufb_link_for("", Os::Mac), "");
    }
    #[test] fn ufb_links_parse_both_encodings_and_legacy() {
        assert_eq!(parse_ufb_link("ufb:///win/C%3A%2FUsers%2FAlice"), Some((Os::Win, "C:/Users/Alice".into())));
        assert_eq!(parse_ufb_link("ufb:///mac//Volumes/jobs/%C3%A9"), Some((Os::Mac, "/Volumes/jobs/é".into())));
        assert_eq!(parse_ufb_link("union:///mac//x"), Some((Os::Mac, "/x".into())));
        assert_eq!(parse_ufb_link("ufb:///legacy%20path").map(|(o, _)| o), Some(Os::current()));
        assert_eq!(parse_ufb_link("http://x"), None);
    }
    #[test] fn minnotes_links() {
        assert_eq!(minnotes_link_for("/Users/x/a b.minnotes", Os::Mac).unwrap(), "minnotes:///Users/x/a%20b.minnotes");
        assert_eq!(parse_minnotes_link_for("minnotes:///Users/x/a%20b.minnotes", Os::Mac).unwrap(), "/Users/x/a b.minnotes");
        assert_eq!(parse_minnotes_link_for("MINNOTES:///C:/x/d.minnotes", Os::Win).unwrap(), "C:\\x\\d.minnotes");
        assert_eq!(parse_minnotes_link_for("file:///x", Os::Mac), None);
    }
    #[test] fn qcview_links() {
        assert_eq!(qcview_project_link("u1", "m2").unwrap(), "qcview://u1/m2");
        assert_eq!(qcview_project_link("", "m2"), None);
        assert_eq!(parse_qcview_link("QCVIEW://u1/m2"), Some(QcviewLink::Project { uuid: "u1".into(), media_id: Some("m2".into()) }));
        assert_eq!(parse_qcview_link("u1"), Some(QcviewLink::Project { uuid: "u1".into(), media_id: None }));
        assert_eq!(parse_qcview_link("qcview://"), None);
        assert_eq!(parse_qcview_link("qcview://stream?url=srt%3A%2F%2Fhost%3A9000&name=Cam+A&view=AgX"),
            Some(QcviewLink::Stream { url: "srt://host:9000".into(), name: Some("Cam A".into()), query: vec![("url".into(), "srt://host:9000".into()), ("name".into(), "Cam A".into()), ("view".into(), "AgX".into())] }));
        assert_eq!(parse_qcview_link("qcview://stream?name=x"), None);
    }
}
