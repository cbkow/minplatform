//! The ONE image-sequence detector (row 120): QCView's `ProjectManager::detectImageSequence` rules, byte-compatible, and
//! pure — the directory listing is injected, so it is table-testable and the differential harness (`tools/min-paths/seqdiff`)
//! can run it against QCView's own C++ on the same listings.
//!
//! The rules, as the C++ has them: stem = the name up to its LAST `.`, ext = after it (either empty → no); the stem must end
//! in a digit run that is preceded by one separator from `_ . -` with at least one character before it (the greedy base
//! takes everything up to that separator), or — only if that form does not match at all — a run of 3+ digits with at
//! least one character before it (the lazy base takes the least: the whole stem digits → base = its first digit); fewer
//! than 3 digits → no (`logo_v2.png`, `shot1.png`); siblings = the listing filtered by the glob `base sep *.ext` (unescaped,
//! like `QDir::entryList`) and re-validated case-insensitively against `^base sep (\d+) \. ext $`; padding must be identical
//! (`shot_1` vs `shot_001` never share a sequence); fewer than 2 siblings → no; range = min..max with every number not on
//! disk listed as a 0-based offset from the first; pattern = `base sep %0Nd . ext` with a literal `%` doubled.
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequence {
    /// The directory, as the caller's path had it (cleaned, no trailing separator).
    pub directory: String,
    pub base: String,
    /// `"_"`, `"."`, `"-"` or `""`.
    pub sep: String,
    /// The input file's extension, as typed (siblings may differ in case).
    pub ext: String,
    pub padding: usize,
    pub start: i32,
    pub end: i32,
    /// Frame numbers in `start..=end` absent from the listing, as 0-based offsets from `start` (the cache's semantics).
    pub missing: Vec<i32>,
    /// `base sep %0Nd . ext` (printf; a literal `%` in a name is `%%`).
    pub pattern: String,
    /// `base + sep` (QCView's `outBaseLabel`).
    pub base_label: String,
    /// How many siblings matched.
    pub match_count: usize,
}

impl Sequence {
    pub fn frame_count(&self) -> i32 { self.end - self.start + 1 }
    /// The file name for an ABSOLUTE frame number (QCView's `snprintf(pattern, frame)`): `base sep` + zero-padded number +
    /// `. ext`. A number wider than the padding simply widens, as printf does.
    pub fn frame_name(&self, frame: i32) -> String { format!("{}{}{:0w$}.{}", self.base, self.sep, frame, self.ext, w = self.padding) }
    pub fn frame_path(&self, frame: i32) -> String { join(&self.directory, &self.frame_name(frame)) }
    /// The `####` form (`base sep #### . ext`) for apps that speak hashes (MinRender, AE's `[####]`).
    pub fn pattern_hashes(&self) -> String { format!("{}{}{}.{}", self.base, self.sep, "#".repeat(self.padding), self.ext) }
}

fn join(dir: &str, name: &str) -> String { if dir.is_empty() || dir.ends_with('/') || dir.ends_with('\\') { format!("{dir}{name}") } else { format!("{dir}/{name}") } }

/// `QString::toInt` semantics for a digit run: a value that does not fit → 0.
fn q_to_int(digits: &str) -> i32 { digits.parse::<i32>().unwrap_or(0) }

/// The stem split QCView does: `(base, sep, digits)` or `None`.
pub fn split_stem(stem: &str) -> Option<(String, String, String)> {
    let chars: Vec<char> = stem.chars().collect();
    let mut i = chars.len();
    while i > 0 && chars[i - 1].is_ascii_digit() { i -= 1; }
    let run_len = chars.len() - i;
    // Form 1: `^(.+)([_.\-])(\d+)$` — a trailing run (≥ 1) preceded by a separator preceded by ≥ 1 char.
    if run_len >= 1 && i >= 2 && matches!(chars[i - 1], '_' | '.' | '-') {
        let base: String = chars[..i - 1].iter().collect();
        let sep = chars[i - 1].to_string();
        let digits: String = chars[i..].iter().collect();
        return Some((base, sep, digits));
    }
    // Form 2: `^(.+?)(\d{3,})$` — only when form 1 does not match at all. The lazy base takes the least, so the digits are the
    // whole trailing run unless that run IS the stem, in which case the first digit is the base.
    if run_len >= 3 {
        let cut = if i == 0 { 1 } else { i };
        if chars.len() - cut >= 3 {
            return Some((chars[..cut].iter().collect(), String::new(), chars[cut..].iter().collect()));
        }
    }
    None
}

/// `QDir::entryList` wildcard semantics for the (unescaped) glob `base sep *.ext`: `*`, `?`, `[set]` with ranges and a
/// leading `!` for negation; case-insensitive (QDir's default). The regex re-validation after it is what decides; this
/// only reproduces the C++'s pre-filter so a base with glob metacharacters behaves identically.
fn glob_match(pat: &[char], name: &[char]) -> bool {
    fn eq(a: char, b: char) -> bool { a.to_lowercase().eq(b.to_lowercase()) }
    match pat.first() {
        None => name.is_empty(),
        Some('*') => (0..=name.len()).any(|k| glob_match(&pat[1..], &name[k..])),
        Some('?') => !name.is_empty() && glob_match(&pat[1..], &name[1..]),
        Some('[') => {
            let Some(close) = pat.iter().skip(2).position(|&c| c == ']').map(|p| p + 2) else { return !name.is_empty() && eq('[', name[0]) && glob_match(&pat[1..], &name[1..]) };
            let (neg, set) = if pat.get(1) == Some(&'!') || pat.get(1) == Some(&'^') { (true, &pat[2..close]) } else { (false, &pat[1..close]) };
            if name.is_empty() { return false }
            let c = name[0];
            let mut hit = false; let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == '-' { let (lo, hi) = (set[i], set[i + 2]); if (lo..=hi).contains(&c) || (lo.to_ascii_lowercase()..=hi.to_ascii_lowercase()).contains(&c.to_ascii_lowercase()) { hit = true; } i += 3; }
                else { if eq(set[i], c) { hit = true; } i += 1; }
            }
            hit != neg && glob_match(&pat[close + 1..], &name[1..])
        }
        Some(&p) => !name.is_empty() && eq(p, name[0]) && glob_match(&pat[1..], &name[1..]),
    }
}

/// Case-insensitive prefix (Unicode simple folding, as PCRE2 does for the escaped literal).
fn strip_prefix_ci<'a>(name: &'a [char], prefix: &[char]) -> Option<&'a [char]> {
    if name.len() < prefix.len() { return None }
    for (a, b) in name.iter().zip(prefix) { if !a.to_lowercase().eq(b.to_lowercase()) { return None } }
    Some(&name[prefix.len()..])
}

/// Detect the sequence `file_path` belongs to, given its directory's listing (file names only — `QDir::Files`: regular
/// files and symlinks to files, hidden entries excluded). `file_path` itself need not exist or be listed.
pub fn detect<S: AsRef<str>>(file_path: &str, listing: &[S]) -> Option<Sequence> {
    let norm = file_path.replace('\\', "/");
    let (dir, name) = match norm.rfind('/') { Some(i) => (&norm[..i], &norm[i + 1..]), None => ("", norm.as_str()) };
    let (stem, ext) = name.rsplit_once('.')?;   // completeBaseName / suffix
    if stem.is_empty() || ext.is_empty() { return None }
    let (base, sep, digits) = split_stem(stem)?;
    if digits.chars().count() < 3 { return None }
    let padding = digits.chars().count();

    let glob: Vec<char> = format!("{base}{sep}*.{ext}").chars().collect();
    let prefix: Vec<char> = format!("{base}{sep}").chars().collect();
    let ext_chars: Vec<char> = ext.chars().collect();
    let found: Vec<&S> = listing.iter().filter(|n| glob_match(&glob, &n.as_ref().chars().collect::<Vec<_>>())).collect();
    if found.len() < 2 { return None }

    let (mut min, mut max, mut count) = (i32::MAX, i32::MIN, 0usize);
    let mut seen = std::collections::HashSet::new();
    for n in found {
        let chars: Vec<char> = n.as_ref().chars().collect();
        let Some(rest) = strip_prefix_ci(&chars, &prefix) else { continue };
        let run = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        if run == 0 { continue }
        let tail = &rest[run..];
        if tail.first() != Some(&'.') || strip_prefix_ci(&tail[1..], &ext_chars).is_none_or(|r| !r.is_empty()) { continue }
        if run != padding { continue }
        let v = q_to_int(&rest[..run].iter().collect::<String>());
        min = min.min(v); max = max.max(v); seen.insert(v); count += 1;
    }
    if count < 2 || min == i32::MAX { return None }
    let missing: Vec<i32> = (min..=max).filter(|n| !seen.contains(n)).map(|n| n - min).collect();
    let esc = |s: &str| s.replace('%', "%%");
    Some(Sequence {
        directory: crate::clean_path(dir, crate::Os::current()),
        pattern: format!("{}{}%0{}d.{}", esc(&base), esc(&sep), padding, esc(ext)),
        base_label: format!("{base}{sep}"),
        base, sep, ext: ext.to_string(), padding, start: min, end: max, missing, match_count: count,
    })
}

/// List a directory the way `QDir::entryList(QDir::Files)` would for the detector: files and symlinks-to-files, hidden
/// entries excluded (dot-names on POSIX, the hidden attribute on Windows).
pub fn list_files(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if !std::fs::metadata(e.path()).map(|m| m.is_file()).unwrap_or(false) { continue }
        #[cfg(unix)] { if name.starts_with('.') { continue } }
        #[cfg(windows)] { use std::os::windows::fs::MetadataExt; if e.metadata().map(|m| m.file_attributes() & 2 != 0).unwrap_or(false) { continue } }
        out.push(name);
    }
    Ok(out)
}

/// `detect` with the listing read from disk.
pub fn detect_on_disk(file_path: &Path) -> Option<Sequence> {
    let dir = file_path.parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| std::path::PathBuf::from("."));
    let listing = list_files(&dir).ok()?;
    detect(&file_path.to_string_lossy(), &listing)
}

/// `name.####.exr` / AE's `name.[####].exr` → `name.%04d.exr`; a `%` is doubled; no hashes → unchanged.
pub fn hashes_to_printf(pattern: &str) -> String {
    let s = pattern.replace("[#", "#").replace("#]", "#");
    let mut out = String::new(); let mut run = 0usize;
    for c in s.chars() {
        if c == '#' { run += 1; continue }
        if run > 0 { out.push_str(&format!("%0{run}d")); run = 0; }
        if c == '%' { out.push_str("%%") } else { out.push(c) }
    }
    if run > 0 { out.push_str(&format!("%0{run}d")); }
    out
}

/// `name.%04d.exr` → `name.####.exr`; `%d` → `####` (4, the family's default); `%%` → `%`.
pub fn printf_to_hashes(pattern: &str) -> String {
    let mut out = String::new(); let chars: Vec<char> = pattern.chars().collect(); let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if chars.get(i + 1) == Some(&'%') { out.push('%'); i += 2; continue }
            let mut j = i + 1; let mut width = String::new();
            while j < chars.len() && chars[j].is_ascii_digit() { width.push(chars[j]); j += 1; }
            if chars.get(j) == Some(&'d') { let n = width.trim_start_matches('0').parse::<usize>().unwrap_or(0); out.push_str(&"#".repeat(if n == 0 { 4 } else { n })); i = j + 1; continue }
        }
        out.push(chars[i]); i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(name: &str, listing: &[&str]) -> Option<Sequence> { detect(&format!("/seq/{name}"), listing) }

    #[test] fn stem_split_matches_the_two_regexes() {
        assert_eq!(split_stem("shot_010"), Some(("shot".into(), "_".into(), "010".into())));
        assert_eq!(split_stem("frame.0001"), Some(("frame".into(), ".".into(), "0001".into())));
        assert_eq!(split_stem("a_001_002"), Some(("a_001".into(), "_".into(), "002".into())));      // the LAST separator wins
        assert_eq!(split_stem("image0001"), Some(("image".into(), "".into(), "0001".into())));
        assert_eq!(split_stem("0001"), Some(("0".into(), "".into(), "001".into())));                // the lazy base takes one digit
        assert_eq!(split_stem("_001"), Some(("_".into(), "".into(), "001".into())));                // form 1 needs a char before the sep
        assert_eq!(split_stem("a_00_1"), Some(("a_00".into(), "_".into(), "1".into())));            // form 1 matched: no fallback
        assert_eq!(split_stem("a12"), None);
        assert_eq!(split_stem("a_01x"), None);
        assert_eq!(split_stem("x"), None);
    }
    #[test] fn two_or_fewer_digits_are_not_a_sequence() {
        assert!(d("logo_v2.png", &["logo_v1.png", "logo_v2.png", "logo_v3.png"]).is_none());
        assert!(d("shot1.png", &["shot1.png", "shot2.png"]).is_none());
        assert!(d("a_00_1.png", &["a_00_1.png", "a_00_2.png", "a_00_003.png"]).is_none());   // form 1 matched with 1 digit → no fallback
    }
    #[test] fn plain_sequence_with_gaps() {
        let s = d("A.0001.exr", &["A.0001.exr", "A.0002.exr", "A.0004.exr", "notes.txt"]).unwrap();
        assert_eq!((s.base.as_str(), s.sep.as_str(), s.ext.as_str(), s.padding, s.start, s.end), ("A", ".", "exr", 4, 1, 4));
        assert_eq!(s.missing, vec![2]);
        assert_eq!(s.pattern, "A.%04d.exr"); assert_eq!(s.base_label, "A."); assert_eq!(s.directory, "/seq");
        assert_eq!(s.frame_name(12), "A.0012.exr"); assert_eq!(s.frame_path(3), "/seq/A.0003.exr"); assert_eq!(s.pattern_hashes(), "A.####.exr");
        assert_eq!(s.frame_count(), 4); assert_eq!(s.match_count, 3);
    }
    #[test] fn needs_two_siblings_and_identical_padding() {
        assert!(d("shot_001.png", &["shot_001.png"]).is_none());
        assert!(d("shot_001.png", &["shot_001.png", "shot_1.png", "shot_02.png"]).is_none());
        assert!(d("shot_001.png", &["shot_001.png", "shot_0002.png"]).is_none());
        assert_eq!(d("shot_001.png", &["shot_001.png", "shot_0002.png", "shot_003.png"]).unwrap().end, 3);
    }
    #[test] fn siblings_match_case_insensitively_and_pattern_keeps_input_case() {
        let s = d("Shot_001.PNG", &["shot_002.png", "SHOT_001.PNG", "Shot_003.Png"]).unwrap();
        assert_eq!((s.start, s.end, s.match_count), (1, 3, 3)); assert_eq!(s.pattern, "Shot_%03d.PNG");
    }
    #[test] fn input_file_need_not_be_listed() { assert_eq!(d("a_005.tif", &["a_001.tif", "a_002.tif"]).unwrap().missing, Vec::<i32>::new()); }
    #[test] fn no_separator_form() {
        let s = d("image0001.jpg", &["image0001.jpg", "image0002.jpg", "image_0003.jpg"]).unwrap();
        assert_eq!((s.base.as_str(), s.sep.as_str(), s.match_count), ("image", "", 2)); assert_eq!(s.pattern, "image%04d.jpg");
    }
    #[test] fn all_digit_stem_quirk_is_preserved() {
        let s = d("0001.exr", &["0001.exr", "0002.exr"]).unwrap();
        assert_eq!((s.base.as_str(), s.padding, s.start, s.end), ("0", 3, 1, 2)); assert_eq!(s.pattern, "0%03d.exr");
    }
    #[test] fn percent_in_names_is_doubled() { assert_eq!(d("50%_001.png", &["50%_001.png", "50%_002.png"]).unwrap().pattern, "50%%_%03d.png"); }
    #[test] fn glob_metachars_in_base_behave_like_qdir() {
        // `x[ab]_0001.png`: the unescaped glob `x[ab]_*.png` matches `xa_…` / `xb_…`, never the literal — QCView says no.
        assert!(d("x[ab]_0001.png", &["x[ab]_0001.png", "x[ab]_0002.png"]).is_none());
        // `x*_0001.png`: the glob widens, the regex narrows back to the literal.
        let s = d("x*_0001.png", &["x*_0001.png", "x*_0002.png", "xy_0003.png"]).unwrap();
        assert_eq!(s.match_count, 2);
    }
    #[test] fn extension_and_stem_rules() {
        assert!(d("noext_001", &["noext_001", "noext_002"]).is_none());
        assert!(d(".0001", &[".0001", ".0002"]).is_none());
        assert!(d("a_001.exr", &["a_001.exr", "a_002.exr.bak"]).is_none());
    }
    #[test] fn hash_and_printf_patterns() {
        assert_eq!(hashes_to_printf("name.####.exr"), "name.%04d.exr");
        assert_eq!(hashes_to_printf("ae_[#####].png"), "ae_%05d.png");
        assert_eq!(hashes_to_printf("50%.exr"), "50%%.exr");
        assert_eq!(printf_to_hashes("name.%04d.exr"), "name.####.exr");
        assert_eq!(printf_to_hashes("n%d.png"), "n####.png");
        assert_eq!(printf_to_hashes("50%%_%03d.png"), "50%_###.png");
    }
    #[test] fn on_disk_listing_excludes_hidden_and_dirs() {
        let t = std::env::temp_dir().join(format!("min-paths-seq-{}", std::process::id()));
        std::fs::create_dir_all(t.join("sub")).unwrap();
        for n in ["b_001.png", "b_002.png", ".b_003.png"] { std::fs::write(t.join(n), b"").unwrap(); }
        std::fs::create_dir_all(t.join("b_004.png")).unwrap();
        let s = detect_on_disk(&t.join("b_001.png")).unwrap();
        assert_eq!((s.start, s.end, s.match_count), (1, 2, 2));
        std::fs::remove_dir_all(&t).unwrap();
    }
}
