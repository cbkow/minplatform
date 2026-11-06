//! The Rust half of the QCView differential test (tools/min-paths/seqdiff): for each case in a file, create the listed
//! sibling files in a scratch dir, run `detect_on_disk` on the named input, print one line in the harness's format.
//! Case line: `input ; sibling sibling …` (names with spaces are not used). Usage: seqprobe CASES SCRATCH
use std::path::Path;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cases = std::fs::read_to_string(&args[1]).expect("cases");
    let scratch = Path::new(&args[2]);
    for (n, line) in cases.lines().enumerate() {
        let line = line.trim(); if line.is_empty() || line.starts_with('#') { continue }
        let (input, sibs) = line.split_once(';').expect("input ; siblings");
        let dir = scratch.join(format!("case{n:03}")); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        for s in sibs.split_whitespace() { std::fs::write(dir.join(s), b"").unwrap(); }
        let input = input.trim();
        match min_paths::sequence::detect_on_disk(&dir.join(input)) {
            Some(s) => println!("{input}|1|{}|{}|{}|{}|{}|{}", s.pattern, s.base_label, s.start, s.end, s.padding, s.missing.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(",")),
            None => println!("{input}|0"),
        }
    }
}
