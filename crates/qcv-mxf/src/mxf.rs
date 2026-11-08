//! The MXF header-metadata scanner (canon row 137, step 2d): ExifTool's MXF reader (`Image::ExifTool::MXF::ProcessMXF`, the
//! parity oracle QCView bundled) carried rule for rule onto our Rust, its tag table generated from `MXF.pm` by
//! `tools/mxf-tags/gen.py` (`mxf_tags.rs`). Header metadata only — the essence is never read: the header partition, and when
//! that header is not closed + complete, the footer's copy as well (the newest instance of each object wins). ~1 ms a file
//! where `exiftool` costs 200–500 ms.
//!
//! What is carried: the run-in search (11-byte partition prefix in the first 64 kB), KLV with BER lengths, the header partition
//! pack (MXFVersion, the footer position, the header size → stop after a closed complete header, else skip to the footer), the
//! primer (local tag → item UL), every local set the table names (plus ExifTool's auto-named `UserOrganizationPublicUse` sets),
//! the value decoders (`ReadMXFValue`: UTF-16, ProductVersion, VersionType, Timestamp, Position / Length, Boolean, the GPS
//! nibble forms, arrays / batches of references and ULs, UMIDs, ULs vs reversed GUIDs, hex) and the binary formats (big-endian
//! ints, floats, rational64s, strings), the family-1 groups (`Track<n>` numbered by the order TrackID values first appear; an
//! object inherits its group walking down from each Preface), the durations divided by their group's EditRate, the
//! newest-instance de-duplication, and the "best" Duration (the SourcePackage's TimecodeComponent) re-emitted at the top.
//! Values are ExifTool's `-n` (numeric) forms; printing them for people is the Inspector's job.
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};

/// How a table entry's value is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind { LocalSet, Partition, Type(&'static str), Format(&'static str), Binary }
/// One ExifTool MXF table entry.
#[derive(Debug)]
pub struct Tag { pub ul: [u8; 16], pub name: &'static str, pub kind: Kind, pub duration: bool, pub conv: Option<&'static str>, /// ExifTool's `Unknown`: never output; inside a local set
    /// its value still builds the object tree (InstanceUID, strong references), at the top level the KLV is skipped
    pub hidden: bool }

/// A tag's value in ExifTool's `-n` form.
#[derive(Clone, Debug, PartialEq)]
pub enum Value { Text(String), Num(f64), /// several numbers (ExifTool prints them space-separated)
    Nums(Vec<f64>), List(Vec<String>), /// undecoded bytes
    Binary(usize) }
impl Value {
    pub fn as_f64(&self) -> Option<f64> { if let Value::Num(n) = self { Some(*n) } else { None } }
    /// ExifTool's plain text for the value.
    pub fn text(&self) -> String {
        match self { Value::Text(s) => s.clone(), Value::Num(n) => fmt_num(*n), Value::Nums(v) => v.iter().map(|n| fmt_num(*n)).collect::<Vec<_>>().join(" "), Value::List(v) => v.join(", "), Value::Binary(n) => format!("(Binary data {n} bytes)") }
    }
}
fn fmt_num(n: f64) -> String { if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") } }

/// One extracted tag: its family-1 group ("MXF" or "Track<n>"), ExifTool's name, the value.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry { pub group: String, pub name: String, pub value: Value }

#[derive(Debug, thiserror::Error)]
pub enum Error { #[error("{0}: {1}")] Io(String, std::io::Error), #[error("{0}: not an MXF file")] NotMxf(String) }

fn table() -> &'static HashMap<[u8; 16], &'static Tag> {
    static T: std::sync::OnceLock<HashMap<[u8; 16], &'static Tag>> = std::sync::OnceLock::new();
    T.get_or_init(|| crate::mxf_tags::TAGS.iter().map(|t| (t.ul, t)).collect())
}
const PRIMER: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0d, 0x01, 0x02, 0x01, 0x01, 0x05, 0x01, 0x00];
static USER_ORG_SET: Tag = Tag { ul: [0; 16], name: "UserOrganizationPublicUse", kind: Kind::LocalSet, duration: false, conv: None, hidden: false };

struct Raw { group: Option<String>, name: String, value: Value, uid: Option<String>, duration: bool }
#[derive(Default)]
struct Obj { strong: Vec<String>, groups: Vec<usize>, name: String, track_id: Option<String>, edit_rate: Option<f64>, did: bool }
#[derive(Default)]
struct State { primer: HashMap<u16, [u8; 16]>, group1: HashMap<String, String>, num_tracks: u32, objs: HashMap<String, Obj>, prefaces: Vec<String>,
    raws: Vec<Raw>, footer_pos: u64, header_size: u64, header_type: String, edit_rates: HashMap<String, f64>, best: HashMap<&'static str, String>, in_source: bool }

/// Scan `path`'s MXF header metadata.
pub fn read(path: &str) -> Result<Vec<Entry>, Error> {
    let io = |e| Error::Io(path.into(), e);
    let mut f = std::fs::File::open(path).map_err(io)?;
    let mut head = vec![0u8; 65547];
    let n = read_up_to(&mut f, &mut head).map_err(io)?; head.truncate(n);
    let prefix = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0d, 0x01, 0x02];
    let mut start = head.windows(11).position(|w| w == prefix).ok_or_else(|| Error::NotMxf(path.into()))? as u64;
    f.seek(SeekFrom::Start(start)).map_err(io)?;
    let mut st = State::default();
    let (mut header_end, mut footer_at): (Option<u64>, Option<u64>) = (None, None);
    let mut pos = start;
    loop {
        if let Some(he) = header_end && pos >= he {
            if st.header_type == "ClosedCompleteHeader" { break }
            header_end = None;
            if let Some(fp) = footer_at && fp > pos { if f.seek(SeekFrom::Start(fp)).is_err() { break } pos = fp; }
        }
        let mut kb = [0u8; 17];
        if read_up_to(&mut f, &mut kb).map_err(io)? != 17 { break }
        let key: [u8; 16] = kb[..16].try_into().unwrap();
        let mut len = kb[16] as u64; let mut nlen = 0u64;
        if len >= 0x80 { let k = (len & 0x7f) as usize; let mut lb = vec![0u8; k]; if read_up_to(&mut f, &mut lb).map_err(io)? != k { break } len = lb.iter().fold(0u64, |a, b| a * 256 + *b as u64); nlen = k as u64; }
        let mut tag = table().get(&key).copied();
        if tag.is_none() && key[..8] == [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01] && key[8] == 0x0d { tag = Some(&USER_ORG_SET); }   // ExifTool names unknown 0d sets
        let value_at = pos + 17 + nlen;
        match tag {
            Some(t) if !t.hidden && len < 10_000_000 => {
                if key == PRIMER && st.header_size > 0 {
                    if st.footer_pos > 0 { footer_at = Some(start + st.footer_pos); }
                    header_end = Some(pos + st.header_size);
                } else if t.kind == Kind::Partition { start = pos; }
                let mut v = vec![0u8; len as usize];
                if read_up_to(&mut f, &mut v).map_err(io)? != v.len() { break }
                if key == PRIMER { primer(&mut st, &v); }
                else { match t.kind {
                    Kind::Partition => partition(&mut st, t.name, &v),
                    Kind::LocalSet => local_set(&mut st, t.name, &v),
                    _ => { let val = decode(t, &v); st.raws.push(Raw { group: None, name: t.name.into(), value: val, uid: None, duration: false }); }
                } }
            }
            _ => { if f.seek(SeekFrom::Current(len as i64)).is_err() { break } }
        }
        pos = value_at + len;
    }
    finish(st)
}

fn read_up_to(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0; while n < buf.len() { match f.read(&mut buf[n..])? { 0 => break, k => n += k } } Ok(n)
}

/// ExifTool's `MXF::Header` table: MXFVersion; the footer position and the header size drive the scan.
fn partition(st: &mut State, name: &str, v: &[u8]) {
    if v.len() >= 4 { let (a, b) = (u16::from_be_bytes([v[0], v[1]]), u16::from_be_bytes([v[2], v[3]])); st.raws.push(Raw { group: None, name: "MXFVersion".into(), value: Value::Text(format!("{a}.{b}")), uid: None, duration: false }); }
    if v.len() >= 40 { st.footer_pos = u64::from_be_bytes(v[24..32].try_into().unwrap()); st.header_size = u64::from_be_bytes(v[32..40].try_into().unwrap()); st.header_type = name.into(); }
}

fn primer(st: &mut State, v: &[u8]) {
    if v.len() <= 8 { return }
    let (count, size) = (u32::from_be_bytes(v[0..4].try_into().unwrap()) as usize, u32::from_be_bytes(v[4..8].try_into().unwrap()) as usize);
    if size < 18 { return }
    let mut p = 8;
    for _ in 0..count { if p + size > v.len() { break } st.primer.insert(u16::from_be_bytes([v[p], v[p + 1]]), v[p + 2..p + 18].try_into().unwrap()); p += size; }
}

fn local_set(st: &mut State, set_name: &str, d: &[u8]) {
    let (mut strong, mut groups, mut instance, mut edit_rate, mut track_id) = (vec![], vec![], None::<String>, None::<f64>, None::<String>);
    let mut p = 0;
    while p + 4 < d.len() {
        let (loc, len) = (u16::from_be_bytes([d[p], d[p + 1]]), u16::from_be_bytes([d[p + 2], d[p + 3]]) as usize);
        p += 4;
        if p + len > d.len() { break }
        let bytes = &d[p..p + len]; p += len;
        let Some(t) = st.primer.get(&loc).and_then(|ul| table().get(ul).copied()) else { continue };   // unknown tags are not extracted
        if matches!(t.kind, Kind::LocalSet | Kind::Partition) { continue }
        let val = decode(t, bytes);
        if let (Kind::Type(ty), Value::List(refs)) = (t.kind, &val) && ty.starts_with("StrongReference") { strong.extend(refs.iter().cloned()); }
        if let (Kind::Type(ty), Value::Text(r)) = (t.kind, &val) && ty.starts_with("StrongReference") { strong.push(r.clone()); }
        if t.name == "InstanceUID" && let Value::Text(s) = &val { instance = Some(s.clone()); }
        if t.hidden { continue }   // decoded for the tree only
        // %duration's RawConv: all 0xff's (> 1e18) is no value
        if t.duration && val.as_f64().is_some_and(|n| n > 1e18) { continue }
        let idx = st.raws.len();
        st.raws.push(Raw { group: None, name: t.name.into(), value: val.clone(), uid: None, duration: t.duration });
        groups.push(idx);
        if t.duration {} else if t.name == "EditRate" { edit_rate = val.as_f64(); }
        else if t.name.ends_with("TrackID") {
            let id = val.text();
            if !st.group1.contains_key(&id) { st.num_tracks += 1; st.group1.insert(id.clone(), format!("Track{}", st.num_tracks)); }
            track_id = Some(id);
        }
    }
    let Some(inst) = instance else { return };
    for &g in &groups { st.raws[g].uid = Some(inst.clone()); }
    let o = st.objs.entry(inst.clone()).or_default();
    o.strong.extend(strong); o.groups.extend(groups);
    o.name = set_name.into();
    if track_id.is_some() { o.track_id = track_id; }
    if edit_rate.is_some() { o.edit_rate = edit_rate; }
    if set_name == "Preface" { st.prefaces.push(inst); }
}

/// ExifTool `SetGroups`: the family-1 group by the nearest TrackID down from a Preface, the EditRate per group, the
/// TimecodeComponents whose Duration is the file's.
fn set_groups(st: &mut State, inst: &str, track: Option<String>) {
    let Some(o) = st.objs.get_mut(inst) else { return };
    if o.did { return }
    o.did = true;
    let track = o.track_id.clone().or(track);
    let mut g1 = None;
    if let Some(t) = &track {
        o.track_id = Some(t.clone());
        g1 = st.group1.get(t).cloned();
        if let (Some(g), Some(er)) = (&g1, o.edit_rate) { st.edit_rates.insert(g.clone(), er); }
        if o.name == "TimecodeComponent" { st.best.insert(if st.in_source { "Source" } else { "Other" }, inst.to_string()); }
    }
    let o = st.objs.get(inst).unwrap();
    let set_source = o.name == "SourcePackage";
    if set_source { st.in_source = true; }
    let (groups, strong) = (o.groups.clone(), o.strong.clone());
    if let Some(g) = &g1 { for i in groups { st.raws[i].group = Some(g.clone()); } }
    for r in strong { set_groups(st, &r, track.clone()); }
    if set_source { st.in_source = false; }
}

fn finish(mut st: State) -> Result<Vec<Entry>, Error> {
    for p in st.prefaces.clone() { set_groups(&mut st, &p, None); }
    // durations → seconds by their group's EditRate
    for r in st.raws.iter_mut() { if r.duration && let Some(g) = &r.group && let Some(er) = st.edit_rates.get(g) && *er != 0.0 && let Value::Num(n) = r.value { r.value = Value::Num(n / er); } }
    // keep the newest instance of each (name, object); the best Duration re-emitted at the top
    let best = st.best.get("Source").or(st.best.get("Other")).cloned();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut keep = vec![true; st.raws.len()];
    let mut best_val = None;
    for i in (0..st.raws.len()).rev() {
        let Some(uid) = st.raws[i].uid.clone() else { continue };
        if !seen.insert((st.raws[i].name.clone(), uid.clone())) { keep[i] = false; }
        else if st.raws[i].name == "Duration" && best.as_deref() == Some(uid.as_str()) && best_val.is_none() { best_val = Some(st.raws[i].value.clone()); }
    }
    let mut out: Vec<Entry> = st.raws.into_iter().zip(keep).filter(|(_, k)| *k).map(|(r, _)| Entry { group: r.group.unwrap_or_else(|| "MXF".into()), name: r.name, value: r.value }).collect();
    if let Some(v) = best_val { out.push(Entry { group: "MXF".into(), name: "Duration".into(), value: v }); }
    Ok(out)
}

/// A value by its table kind (ExifTool `ReadMXFValue` for its MXF types, `ReadValue` big-endian for the binary formats).
fn decode(t: &Tag, v: &[u8]) -> Value {
    let val = match t.kind {
        Kind::Type(ty) => mxf_value(v, ty),
        Kind::Format(fmt) => format_value(v, fmt),
        _ => Value::Binary(v.len()),
    };
    match (t.conv, &val) {
        (Some("div10"), Value::Num(n)) => Value::Num(n / 10.0),
        (Some("latlon"), Value::Text(s)) => Value::Text(lat_lon(s)),
        _ => val,
    }
}

fn hex(v: &[u8]) -> String { v.iter().map(|b| format!("{b:02x}")).collect() }
fn ul(v: &[u8]) -> String { format!("{}.{}.{}.{}.{}", hex(&v[0..4]), hex(&v[4..6]), hex(&v[6..8]), hex(&v[8..12]), hex(&v[12..16])) }
fn guid(v: &[u8]) -> String { format!("{}-{}-{}-{}-{}", hex(&v[0..4]), hex(&v[4..6]), hex(&v[6..8]), hex(&v[8..10]), hex(&v[10..16])) }

fn mxf_value(v: &[u8], ty: &str) -> Value {
    let len = v.len();
    match ty {
        "UTF-16" => { let u: Vec<u16> = v.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect(); Value::Text(String::from_utf16_lossy(&u).trim_end_matches('\0').to_string()) }
        "ProductVersion" => {
            let mut a: Vec<u16> = v.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect(); while a.len() < 5 { a.push(0); }
            let rel = match a[4] { 0 => "unknown".to_string(), 1 => "released".into(), 2 => "debug".into(), 3 => "patched".into(), 4 => "beta".into(), 5 => "private build".into(), n => format!("unknown {n}") };
            Value::Text(format!("{}.{}.{}.{} {rel}", a[0], a[1], a[2], a[3]))
        }
        "VersionType" => Value::Text(v.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(".")),
        "Timestamp" => {
            if len < 2 { return Value::Text(format!("Invalid (0x{})", hex(v))) }
            let mut a: Vec<u32> = vec![u16::from_be_bytes([v[0], v[1]]) as u32]; a.extend(v[2..].iter().map(|b| *b as u32));
            let max = [3000u32, 12, 31, 24, 59, 59, 249];
            let mut mi = 0; for x in &a { if mi < max.len() && *x <= max[mi] { mi += 1 } else { break } }
            if mi < max.len() || a.len() < 7 { return Value::Text(format!("Invalid (0x{})", hex(v))) }
            Value::Text(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:03}", a[0], a[1], a[2], a[3], a[4], a[5], a[6] * 4))
        }
        "Position" | "Length" => Value::Num(if len >= 8 { u64::from_be_bytes(v[..8].try_into().unwrap()) as f64 } else { 0.0 }),
        "Boolean" => Value::Text(if v == [0] { "False" } else { "True" }.into()),
        "Alt" | "Lat" | "Lon" if len == 4 => {
            let mut s = hex(&u32::from_le_bytes(v.try_into().unwrap()).to_be_bytes());
            if ty == "Alt" { if matches!(s.as_bytes()[0], b'a' | b'b' | b'c') { s = s[3..].to_string() } else if matches!(s.as_bytes()[0], b'd' | b'e' | b'f') { s = format!("-{}", &s[3..]) } else { s = format!("{s} (from earth centre)") } }
            else { s.insert(3, '.'); if ty == "Lat" { if s.starts_with('f') { s.replace_range(0..1, "-") } } else if s.starts_with('e') { s.replace_range(0..1, "-") } else if s.starts_with('f') { s.replace_range(0..1, "-1") } }
            Value::Text(s)
        }
        t if (t.contains("Array") || t.contains("Batch")) && len > 16 => {
            let (count, size) = (u32::from_be_bytes(v[0..4].try_into().unwrap()) as usize, u32::from_be_bytes(v[4..8].try_into().unwrap()) as usize);
            let items: Vec<&[u8]> = (0..count).map(|i| 8 + i * size).take_while(|p| p + size <= len).map(|p| &v[p..p + size]).collect();
            Value::List(items.iter().map(|it| if t.starts_with("StrongReference") && it.len() >= 16 { guid(it) } else if (t == "BatchOfUL" || t.starts_with("WeakReference")) && it.len() == 16 { ul_or_guid(it) } else { hex(it) }).collect())
        }
        _ if len == 32 => Value::Text(format!("{}.{}.{}.{} {} {} {}", hex(&v[0..4]), hex(&v[4..6]), hex(&v[6..8]), hex(&v[8..12]), hex(&v[12..13]), hex(&v[13..16]), guid(&v[16..32]))),
        _ if len == 16 => Value::Text(if ty == "UL" || ty == "WeakReference" { ul_or_guid(v) } else { guid(v) }),
        _ => Value::Text(hex(v)),
    }
}
/// A UL, or — when a reversed GUID sits in a UL slot (its byte 0 has the high bit) — the GUID with its halves swapped back.
fn ul_or_guid(v: &[u8]) -> String { if v[0] & 0x80 == 0 { ul(v) } else { let mut s = v[8..].to_vec(); s.extend_from_slice(&v[..8]); guid(&s) } }

fn format_value(v: &[u8], fmt: &str) -> Value {
    let size = match fmt { "int8u" | "int8s" => 1, "int16u" | "int16s" => 2, "int32u" | "int32s" | "float" => 4, "int64u" | "int64s" | "double" | "rational64s" => 8, "string" => 0, "int16u[2]" => 2, _ => return Value::Binary(v.len()) };
    if fmt == "string" { let end = v.iter().position(|b| *b == 0).unwrap_or(v.len()); return Value::Text(String::from_utf8_lossy(&v[..end]).into_owned()) }
    let n: Vec<f64> = v.chunks_exact(size).map(|c| match fmt {
        "int8u" => c[0] as f64, "int8s" => c[0] as i8 as f64,
        "int16u" | "int16u[2]" => u16::from_be_bytes([c[0], c[1]]) as f64, "int16s" => i16::from_be_bytes([c[0], c[1]]) as f64,
        "int32u" => u32::from_be_bytes(c.try_into().unwrap()) as f64, "int32s" => i32::from_be_bytes(c.try_into().unwrap()) as f64,
        "int64u" => u64::from_be_bytes(c.try_into().unwrap()) as f64, "int64s" => i64::from_be_bytes(c.try_into().unwrap()) as f64,
        "float" => f32::from_be_bytes(c.try_into().unwrap()) as f64, "double" => f64::from_be_bytes(c.try_into().unwrap()),
        _ => { let (a, b) = (i32::from_be_bytes(c[0..4].try_into().unwrap()), i32::from_be_bytes(c[4..8].try_into().unwrap())); if b == 0 { f64::INFINITY } else { a as f64 / b as f64 } }
    }).collect();
    match n.len() { 0 => Value::Binary(v.len()), 1 => Value::Num(n[0]), _ => Value::Nums(n) }
}

/// ExifTool `ConvLatLon`: "[d]ddmmss(N|S|E|W)" → decimal degrees.
fn lat_lon(s: &str) -> String {
    let mut out = vec![];
    for ne in ["NS", "EW"] {
        let b = s.as_bytes();
        let hit = (0..b.len()).find_map(|i| {
            let j = b[i..].iter().position(|c| !c.is_ascii_digit()).map(|k| i + k)?;
            if j - i < 6 || j - i > 7 || !ne.as_bytes().contains(&b[j]) || (i > 0 && b[i - 1].is_ascii_digit()) { return None }
            let d = &s[i..j]; let (deg, rest) = d.split_at(d.len() - 4);
            let (mm, ss) = rest.split_at(2);
            let v = deg.parse::<f64>().ok()? + (mm.parse::<f64>().ok()? + ss.parse::<f64>().ok()? / 60.0) / 60.0;
            Some(if b[j] == b'N' || b[j] == b'E' { v } else { -v })
        });
        if let Some(v) = hit { out.push(fmt_num(v)); }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A synthetic OP1a (FFmpeg: DNxHD + PCM, start timecode 01:00:00:00 at 25 fps; fixtures never travel) — the tree, the
    /// groups, the durations in seconds, the descriptor values, the identification. `mxf-parity` holds the full bar.
    #[test]
    fn op1a_tree_groups_durations() {
        let d = std::env::temp_dir().join(format!("qcv-mxf-{}", std::process::id())); std::fs::create_dir_all(&d).unwrap();
        let p = d.join("tc.mxf");
        let ok = std::process::Command::new("ffmpeg").args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=25", "-f", "lavfi", "-i", "sine=sample_rate=48000", "-t", "2", "-c:v", "dnxhd", "-b:v", "120M", "-pix_fmt", "yuv422p", "-c:a", "pcm_s16le", "-timecode", "01:00:00:00"]).arg(&p).status().map(|s| s.success()).unwrap_or(false);
        if !ok { eprintln!("ffmpeg missing: skipped"); return }
        let e = read(&p.to_string_lossy()).unwrap();
        let get = |g: &str, n: &str| e.iter().filter(|x| x.group == g && x.name == n).map(|x| x.value.clone()).collect::<Vec<_>>();
        assert!(get("Track1", "StartTimecode").contains(&Value::Num(3600.0)), "the timecode start in seconds (90000 frames / 25)");
        assert!(get("Track2", "SampledWidth").contains(&Value::Num(1920.0)));
        assert!(get("Track3", "ChannelCount").contains(&Value::Num(1.0)));
        assert!(get("Track1", "EditRate").contains(&Value::Num(25.0)));
        assert_eq!(get("MXF", "ApplicationName"), vec![Value::Text("OP1a Muxer".into())]);
        assert_eq!(get("MXF", "Duration"), vec![Value::Num(2.0)], "the best Duration: the source package's timecode component");
        assert!(read(&d.join("missing.mxf").to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(d);
    }
}
