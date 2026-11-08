//! The Adobe metadata reader (row 134): what `AdobeMetadataExtractor` asked exiftool for, read from what FFmpeg already
//! has — the XMP packet the `mov` demuxer exports under the `xmp` metadata key when opened with `export_xmp=1` (the
//! `aeProjectLink:fullPath` link After Effects writes, `xmpDM:windowsAtom` / `xmpDM:macAtom`'s project paths Premiere
//! writes, `xmpDM:startTimecode` / `xmpDM:altTimecode`'s `timeValue`, `xmp:CreateDate`), plus the container's own
//! `creation_time` and the embedded `timecode` (the tmcd track / MXF's) that exiftool reported as the QuickTime /
//! UserData / MXF timecodes. MXF: the user's corpus carries no XMP (row 134), the structural tags are row 137's scanner.
use crate::AdobeMetadata;
use ffmpeg_sys_next as ff;
use quick_xml::{events::Event, Reader};
use std::ffi::CString;

/// The dozen fields, keyed as exiftool names them (`-s` output).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XmpFields { pub ae_project_link_full_path: String, pub windows_atom_unc_project_path: String, pub mac_atom_posix_project_path: String, pub start_timecode: String, pub alt_timecode: String, pub create_date: String }

/// Parse an XMP packet: simple properties as attributes OR elements, the structs by their parent element; keys are matched
/// by namespace prefix + local name so `aeProjectLink:fullPath` and `xmpDM:timeValue` under `startTimecode` land where
/// exiftool's names put them.
pub fn parse_xmp(packet: &str) -> XmpFields {
    let mut out = XmpFields::default();
    let mut reader = Reader::from_str(packet);
    let mut path: Vec<(String, String)> = vec![];   // (prefix, local name)
    let split = |name: &[u8]| -> (String, String) { let s = String::from_utf8_lossy(name); match s.rsplit_once(':') { Some((p, l)) => (p.to_string(), l.to_string()), None => (String::new(), s.to_string()) } };
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            // Adobe writes the structs as EMPTY elements with attribute-form fields (`<xmpDM:startTimecode xmpDM:timeValue="…"/>`),
            // so an attribute's context is the element that carries it
            Ok(Event::Start(e)) => {
                path.push(split(e.name().as_ref()));
                for a in e.attributes().flatten() { let (pre, key) = split(a.key.as_ref()); let val = a.unescape_value().map(|v| v.into_owned()).unwrap_or_default(); assign(&mut out, &path, &pre, &key, &val); }
            }
            Ok(Event::Empty(e)) => {
                path.push(split(e.name().as_ref()));
                for a in e.attributes().flatten() { let (pre, key) = split(a.key.as_ref()); let val = a.unescape_value().map(|v| v.into_owned()).unwrap_or_default(); assign(&mut out, &path, &pre, &key, &val); }
                path.pop();
            }
            Ok(Event::End(_)) => { path.pop(); }
            Ok(Event::Text(t)) => { if let (Some((pre, key)), Ok(val)) = (path.last().cloned(), t.xml_content()) { let parent = path[..path.len() - 1].to_vec(); let v = val.trim().to_string(); if !v.is_empty() { assign(&mut out, &parent, &pre, &key, &v); } } }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}
/// The document's own values win: the `xmpMM:Pantry` (the ingredients' copies) and the history are skipped, the first
/// occurrence of each key sticks (exiftool reports the top-level Description's).
fn assign(out: &mut XmpFields, context: &[(String, String)], prefix: &str, key: &str, val: &str) {
    let under = |p: &str| context.iter().any(|(_, l)| l == p);
    if under("Pantry") || under("Ingredients") || under("History") || under("Tracks") { return }
    let set = |slot: &mut String| { if slot.is_empty() { *slot = val.into(); } };
    match (prefix, key) {
        ("aeProjectLink", "fullPath") => set(&mut out.ae_project_link_full_path),
        // newer After Effects writes the link as a creatorAtom struct (`<creatorAtom:aeProjectLink creatorAtom:fullPath="…"/>`) —
        // exiftool names both AeProjectLinkFullPath (found on a 2022 render: the reader had only the older namespace)
        (_, "fullPath") if under("aeProjectLink") => set(&mut out.ae_project_link_full_path),
        (_, "uncProjectPath") if under("windowsAtom") => set(&mut out.windows_atom_unc_project_path),
        (_, "posixProjectPath") if under("macAtom") => set(&mut out.mac_atom_posix_project_path),
        (_, "timeValue") if under("startTimecode") => set(&mut out.start_timecode),
        (_, "timeValue") if under("altTimecode") => set(&mut out.alt_timecode),
        ("xmp", "CreateDate") => set(&mut out.create_date),
        _ => {}
    }
}

/// FFmpeg's ISO `creation_time` ("2024-03-05T01:54:32.000000Z") in exiftool's form ("2024:03:05 01:54:32"), the string the
/// Inspector showed.
pub fn exiftool_date(iso: &str) -> String {
    let t = iso.trim(); if t.len() < 19 || t.as_bytes()[10] != b'T' { return t.to_string() }
    format!("{}:{}:{} {}", &t[0..4], &t[5..7], &t[8..10], &t[11..19])
}

/// Open with `export_xmp=1` and read the packet + the container tags into `AdobeMetadata` (always `loaded`).
pub fn read_adobe_metadata(path: &str) -> AdobeMetadata {
    let mut m = AdobeMetadata { loaded: true, ..Default::default() };
    if path.is_empty() || !std::path::Path::new(path).exists() { return m }
    let Ok(cpath) = CString::new(path) else { return m };
    unsafe {
        let mut opts: *mut ff::AVDictionary = std::ptr::null_mut();
        let (k, v) = (CString::new("export_xmp").unwrap(), CString::new("1").unwrap()); ff::av_dict_set(&mut opts, k.as_ptr(), v.as_ptr(), 0);
        let mut fmt: *mut ff::AVFormatContext = std::ptr::null_mut();
        let r = ff::avformat_open_input(&mut fmt, cpath.as_ptr(), std::ptr::null_mut(), &mut opts);
        ff::av_dict_free(&mut opts);
        if r < 0 { return m }
        ff::avformat_find_stream_info(fmt, std::ptr::null_mut());
        let get = |d: *mut ff::AVDictionary, key: &str| -> String { if d.is_null() { return String::new() } let k = CString::new(key).unwrap(); let e = ff::av_dict_get(d, k.as_ptr(), std::ptr::null(), 0); if e.is_null() { String::new() } else { std::ffi::CStr::from_ptr((*e).value).to_string_lossy().trim().to_string() } };
        let is_mxf = !(*fmt).iformat.is_null() && std::ffi::CStr::from_ptr((*(*fmt).iformat).name).to_bytes() == b"mxf";
        let xmp = get((*fmt).metadata, "xmp");
        if !xmp.is_empty() {
            let f = parse_xmp(&xmp);
            m.ae_project_path = f.ae_project_link_full_path; m.premiere_win_path = f.windows_atom_unc_project_path; m.premiere_mac_path = f.mac_atom_posix_project_path;
            m.xmp_start_timecode = f.start_timecode; m.xmp_alt_timecode = f.alt_timecode; m.qt_creation_date = f.create_date;
        }
        m.qt_media_create_date = exiftool_date(&get((*fmt).metadata, "creation_time"));
        // the embedded timecode: the tmcd track (QuickTime:TimeCode / UserData:TimeCode) or MXF's (MXF:StartTimecode)
        let mut tc = String::new();
        for i in 0..(*fmt).nb_streams { let t = get((**(*fmt).streams.add(i as usize)).metadata, "timecode"); if !t.is_empty() { tc = t; break } }
        if tc.is_empty() { tc = get((*fmt).metadata, "timecode"); }
        if is_mxf { m.mxf_start_timecode = tc.clone(); m.mxf_timecode_at_start = tc; } else { m.qt_timecode = tc.clone(); m.userdata_timecode = tc; m.qt_start_timecode = m.xmp_start_timecode.clone(); }
        ff::avformat_close_input(&mut fmt);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parses_elements_and_attributes() {
        let p = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/" xmlns:aeProjectLink="http://ns.adobe.com/aeProjectLink/1.0/" xmp:CreateDate="2026-11-01T10:00:00-07:00">
<aeProjectLink:fullPath>/Volumes/Jobs/x.aep</aeProjectLink:fullPath>
<xmpDM:windowsAtom rdf:parseType="Resource"><xmpDM:uncProjectPath>\\srv\jobs\y.prproj</xmpDM:uncProjectPath></xmpDM:windowsAtom>
<xmpDM:macAtom rdf:parseType="Resource"><xmpDM:posixProjectPath>/Volumes/Jobs/y.prproj</xmpDM:posixProjectPath></xmpDM:macAtom>
<xmpDM:startTimecode rdf:parseType="Resource"><xmpDM:timeFormat>2997DropTimecode</xmpDM:timeFormat><xmpDM:timeValue>01:00:00;00</xmpDM:timeValue></xmpDM:startTimecode>
<xmpDM:altTimecode rdf:parseType="Resource"><xmpDM:timeValue>00:59:58;00</xmpDM:timeValue></xmpDM:altTimecode>
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
        let f = parse_xmp(p);
        assert_eq!(f.ae_project_link_full_path, "/Volumes/Jobs/x.aep"); assert_eq!(f.windows_atom_unc_project_path, r"\\srv\jobs\y.prproj"); assert_eq!(f.mac_atom_posix_project_path, "/Volumes/Jobs/y.prproj");
        assert_eq!(f.start_timecode, "01:00:00;00"); assert_eq!(f.alt_timecode, "00:59:58;00"); assert_eq!(f.create_date, "2026-11-01T10:00:00-07:00");
        let attr = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"><rdf:Description xmlns:aeProjectLink="a" aeProjectLink:fullPath="C:\p.aep"/></rdf:RDF></x:xmpmeta>"#;
        let creator = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"><rdf:Description xmlns:creatorAtom="c"><creatorAtom:aeProjectLink creatorAtom:compositionID="230" creatorAtom:fullPath="/Volumes/Jobs/new.aep"/></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        assert_eq!(parse_xmp(creator).ae_project_link_full_path, "/Volumes/Jobs/new.aep", "the creatorAtom form newer After Effects writes");
        let premiere = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"><rdf:Description xmlns:xmpDM="d" xmlns:xmpMM="m" xmlns:creatorAtom="c">
<xmpDM:startTimecode xmpDM:timeFormat="23976Timecode" xmpDM:timeValue="01:00:00:00"/>
<xmpMM:Pantry><rdf:Bag><rdf:li><rdf:Description><xmpDM:startTimecode xmpDM:timeValue="05:00:00:00"/></rdf:Description></rdf:li></rdf:Bag></xmpMM:Pantry>
<creatorAtom:windowsAtom creatorAtom:extension=".prproj" creatorAtom:uncProjectPath="\\srv\jobs\z.prproj"/>
</rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let f = parse_xmp(premiere); assert_eq!(f.start_timecode, "01:00:00:00"); assert_eq!(f.windows_atom_unc_project_path, r"\\srv\jobs\z.prproj"); assert_eq!(f.alt_timecode, "");   // the attribute text, verbatim (exiftool shows the same)
        assert_eq!(parse_xmp(attr).ae_project_link_full_path, r"C:\p.aep");
        assert_eq!(exiftool_date("2024-03-05T01:54:32.000000Z"), "2024:03:05 01:54:32");
    }
}
