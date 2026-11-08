//! `mxf-parity` — the row-137 acceptance: our scanner against ExifTool's MXF group, tag for tag, on real files.
//!
//!     EXIFTOOL=/path/to/exiftool cargo run --release -p qcv-mxf --bin mxf-parity -- [--list FILE] [FILE.mxf …]
//!
//! ExifTool runs as `exiftool -a -n -G1 -s -MXF:all` (every duplicate, numeric values, family-1 groups); both sides become
//! multisets of (group, name, value) per file — numbers compared within a relative 1e-6 (ExifTool rounds rationals to 10
//! digits and floats to 7), text exactly, binary by its byte count. The report names files only by their position in the
//! list (the corpus is client footage: no names leave this machine in a log), then the tags that disagree, most frequent first.
use std::collections::BTreeMap;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files: Vec<String> = vec![];
    let mut it = args.iter();
    let mut verbose = false;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => { if let Some(p) = it.next() { files.extend(std::fs::read_to_string(p).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).map(str::to_string)); } }
            "-v" => verbose = true,
            _ => files.push(a.clone()),
        }
    }
    let et = std::env::var("EXIFTOOL").unwrap_or_else(|_| "exiftool".into());
    let (mut ok_files, mut tag_miss): (usize, BTreeMap<String, usize>) = (0, BTreeMap::new());
    let (mut ours_ms, mut theirs_ms) = (0.0f64, 0.0f64);
    let mut compared = 0usize; let mut not_mxf = 0usize;
    for (i, f) in files.iter().enumerate() {
        let t = Instant::now();
        let ours = match qcv_mxf::read(f) { Ok(e) => Some(e), Err(qcv_mxf::mxf::Error::NotMxf(_)) => None, Err(e) => { println!("#{i}: ours failed: {}", e.to_string().replace(f.as_str(), "<file>")); continue } };
        ours_ms += t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let out = std::process::Command::new(&et).args(["-a", "-n", "-G1", "-s", "-MXF:all", f]).output();
        theirs_ms += t.elapsed().as_secs_f64() * 1000.0;
        let Ok(out) = out else { println!("exiftool not runnable ({et}) — set EXIFTOOL"); return };
        let theirs = parse_exiftool(&String::from_utf8_lossy(&out.stdout));
        compared += 1;
        // not an MXF (an .mxf name over another format): agreement when ExifTool finds no MXF tags either
        let Some(ours) = ours else { if theirs.is_empty() { ok_files += 1; not_mxf += 1; } else { println!("#{i}: ours says not MXF, exiftool has {} tags", theirs.len()); } continue };
        let mut a: Vec<(String, String, String)> = ours.iter().map(|e| (e.group.clone(), e.name.clone(), e.value.text())).collect();
        let mut b = theirs;
        // match each of theirs to one of ours (same group + name, equal value)
        let mut miss_theirs = vec![];
        for (g, n, v) in b.drain(..) {
            if let Some(k) = a.iter().position(|(ga, na, va)| *ga == g && *na == n && same(va, &v)) { a.remove(k); } else { miss_theirs.push((g, n, v)); }
        }
        if miss_theirs.is_empty() && a.is_empty() { ok_files += 1; continue }
        println!("#{i}: {} of theirs unmatched, {} of ours extra", miss_theirs.len(), a.len());
        for (g, n, v) in &miss_theirs {
            *tag_miss.entry(format!("exiftool-only {n}")).or_default() += 1;
            if verbose { let ours_v: Vec<String> = a.iter().filter(|(ga, na, _)| ga == g && na == n).map(|(_, _, v)| v.clone()).collect(); println!("    [{g}] {n}: exiftool {v:?}  ours {ours_v:?}"); }
        }
        for (g, n, v) in &a { *tag_miss.entry(format!("ours-only {n}")).or_default() += 1; if verbose && !miss_theirs.iter().any(|(gt, nt, _)| gt == g && nt == n) { println!("    [{g}] {n}: ours {v:?} (exiftool none)"); } }
    }
    println!("\n{ok_files} of {compared} files IDENTICAL to ExifTool's MXF group ({not_mxf} of them not MXF on both sides)");
    let mut v: Vec<_> = tag_miss.into_iter().collect(); v.sort_by_key(|e| std::cmp::Reverse(e.1));
    for (k, n) in v.iter().take(40) { println!("  {n:5}  {k}"); }
    if compared > 0 { println!("time per file: ours {:.2} ms, exiftool {:.0} ms", ours_ms / compared as f64, theirs_ms / compared as f64); }
}

/// `[Group]  Name  : value` lines.
fn parse_exiftool(s: &str) -> Vec<(String, String, String)> {
    s.lines().filter_map(|l| {
        let l = l.strip_prefix('[')?; let (g, rest) = l.split_once(']')?;
        let (n, v) = rest.split_once(':')?;
        Some((g.trim().to_string(), n.trim().to_string(), v.trim().to_string()))
    }).filter(|(g, _, _)| g != "ExifTool" && g != "System" && g != "File").collect()
}

/// Numbers (or space-separated number lists) within a relative 1e-6; binary by byte count; else exact.
fn same(a: &str, b: &str) -> bool {
    if a == b { return true }
    let bin = |s: &str| s.strip_prefix("(Binary data ").and_then(|r| r.split_whitespace().next()).map(str::to_string);
    if let (Some(x), Some(y)) = (bin(a), bin(b)) { return x == y }
    let nums = |s: &str| s.split_whitespace().map(|t| t.parse::<f64>()).collect::<Result<Vec<_>, _>>().ok();
    match (nums(a), nums(b)) {
        (Some(x), Some(y)) if x.len() == y.len() && !x.is_empty() => x.iter().zip(&y).all(|(p, q)| (p - q).abs() <= 1e-6 * p.abs().max(q.abs()).max(1e-9) || (p.is_infinite() && q.is_infinite())),
        _ => false,
    }
}
