//! ONE mapping table for the family (row 121) and both things the apps do with it: minNotes' structured `{vol, rel}` refs
//! (`PathMap.cpp`: `toRef` / `resolveRef`) and ufb's / MinRender's cross-OS string translation (`translate_path_to`).
//! Rules: list order, disabled rows skipped, a root matches only on a BOUNDARY (`/Vol/share` never matches
//! `/Vol/share-extra` — MinRender lacked this), comparison treats `\` as a separator on every OS and is case-insensitive
//! on Windows only (the older apps' rule; the stored path is never rewritten), resolution joins with `/` on every OS
//! (minNotes 2026-10-18: a root typed as `C:/…` joined with `\` made one file two keys).
use crate::{has_drive, Os};

/// A volume mapping row: the same root under three OSes. minNotes' shape (QSettings `pathMappings/json`), MinRender's
/// without the id (ids are minted on adoption), ufb's `win` / `mac` hints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mapping {
    #[cfg_attr(feature = "serde", serde(default))] pub id: String,
    #[cfg_attr(feature = "serde", serde(default))] pub label: String,
    #[cfg_attr(feature = "serde", serde(default))] pub win: String,
    #[cfg_attr(feature = "serde", serde(default))] pub mac: String,
    #[cfg_attr(feature = "serde", serde(default))] pub lin: String,
    #[cfg_attr(feature = "serde", serde(default = "yes"))] pub enabled: bool,
}
#[cfg(feature = "serde")] fn yes() -> bool { true }
impl Mapping {
    pub fn root_for(&self, os: Os) -> &str { match os { Os::Win => &self.win, Os::Mac => &self.mac, Os::Lin => &self.lin } }
}

/// A media reference as minNotes stores it: inside a mapped volume → `{vol, rel}`; outside every mapping → the absolute
/// path, machine-local.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum Ref { Vol { vol: String, rel: Vec<String> }, Abs(String) }

fn trim_trailing_seps(s: &str) -> &str { s.trim_end_matches(['/', '\\']) }

/// Normalise for PREFIX COMPARISON only: `\` → `/` on every OS (row 121), lowercase on Windows.
fn norm_for_compare(s: &str, os: Os) -> String { let r = s.replace('\\', "/"); if os.is_windows() { r.to_lowercase() } else { r } }

/// Split a remainder into segments with the SOURCE OS's separator rule: Windows on both `\` and `/`, POSIX on `/` only
/// (a literal `\` stays inside its segment).
fn split_segments(remainder: &str, os: Os) -> Vec<String> {
    let r = if os.is_windows() { remainder.trim_start_matches(['/', '\\']) } else { remainder.trim_start_matches('/') };
    let parts: Vec<String> = if os.is_windows() { r.split(['/', '\\']).filter(|p| !p.is_empty()).map(str::to_string).collect() }
                             else { r.split('/').filter(|p| !p.is_empty()).map(str::to_string).collect() };
    parts
}

/// minNotes' `matchUnderRoot`: `abs` (in `os` form) under `root` (same OS form) → the remainder's segments.
pub fn match_under_root(abs: &str, root: &str, os: Os) -> Option<Vec<String>> {
    if root.is_empty() { return None }
    let cmp_path = norm_for_compare(abs, os);
    let cmp_root = trim_trailing_seps(&norm_for_compare(root, os)).to_string();
    if cmp_root.is_empty() || !cmp_path.starts_with(&cmp_root) { return None }
    if cmp_path.len() > cmp_root.len() {
        let c = cmp_path[cmp_root.len()..].chars().next().unwrap();
        if c != '/' && c != '\\' { return None }    // the boundary rule
    }
    // Slice the ORIGINAL path by the trimmed ORIGINAL root's length (same OS form, lengths line up; lowercasing is 1:1 for ASCII
    // and the comparison is only a gate). Guard the slice point for non-ASCII case folds that changed byte length.
    let orig_root = trim_trailing_seps(root);
    let cut = orig_root.len().min(abs.len());
    let cut = if abs.is_char_boundary(cut) { cut } else { return None };
    Some(split_segments(&abs[cut..], os))
}

/// minNotes' `toRef`: the first enabled mapping whose root for `os` contains `abs` wins; none → `Ref::Abs`.
pub fn to_ref(abs: &str, os: Os, mappings: &[Mapping]) -> Ref {
    if abs.is_empty() { return Ref::Abs(String::new()) }
    for m in mappings.iter().filter(|m| m.enabled) {
        if let Some(rel) = match_under_root(abs, m.root_for(os), os) { return Ref::Vol { vol: m.id.clone(), rel } }
    }
    Ref::Abs(abs.to_string())
}

fn join_root(root: &str, segs: &[String]) -> String {
    let mut path = trim_trailing_seps(root).replace('\\', "/");   // QDir::fromNativeSeparators on every OS
    for s in segs { path.push('/'); path.push_str(s); }
    path
}

/// minNotes' `resolveRef`: a `{vol, rel}` → the path on `os` (forward slashes), or `None` when the volume is unknown,
/// disabled, or has no root for this OS. An absolute string → the best-effort cross-OS swap for legacy refs (every enabled
/// mapping × every OTHER OS's root; no match → the string unchanged, `Some`).
pub fn resolve_ref(r: &Ref, os: Os, mappings: &[Mapping]) -> Option<String> {
    match r {
        Ref::Vol { vol, rel } => {
            let m = mappings.iter().find(|m| &m.id == vol)?;
            if !m.enabled { return None }
            let root = trim_trailing_seps(m.root_for(os));
            if root.is_empty() { return None }
            Some(join_root(root, rel))
        }
        Ref::Abs(s) => {
            if s.is_empty() { return Some(String::new()) }
            for m in mappings.iter().filter(|m| m.enabled) {
                for src in [Os::Win, Os::Mac, Os::Lin] {
                    if src == os { continue }
                    if let Some(segs) = match_under_root(s, m.root_for(src), src) {
                        let root = trim_trailing_seps(m.root_for(os));
                        if root.is_empty() { continue }
                        return Some(join_root(root, &segs));
                    }
                }
            }
            Some(s.clone())
        }
    }
}

/// `~` / `~/…` expanded against THIS machine's home (`$HOME`, then `%USERPROFILE%`); anything else unchanged. `translate`
/// applies it to a mapping root only when that root's OS is the current OS (never another machine's home).
pub fn expand_home(prefix: &str) -> String {
    let rest = if let Some(r) = prefix.strip_prefix("~/") { r } else if prefix == "~" { "" } else { return prefix.to_string() };
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) else { return prefix.to_string() };
    let home = home.to_string_lossy().trim_end_matches(['/', '\\']).to_string();
    if rest.is_empty() { home } else { format!("{home}/{rest}") }
}

/// Separator swap only (ufb `to_native_path`, MinRender `toNativeSeparators`).
pub fn to_native(path: &str, os: Os) -> String { if os.is_windows() { path.replace('/', "\\") } else { path.replace('\\', "/") } }

/// ufb's `translate_path_to` with the boundary check (row 121): a path in `src` OS form → `dst` OS form through the first
/// enabled mapping with both roots set. No same-OS short-circuit, on purpose: a foreign-form string that drifted into a
/// local field (a drive-less `\Volumes\…` or a forward-slash `/Volumes/…` row in a Windows DB) gets repaired to native
/// form. For a Windows SOURCE the drive letter and leading slash are stripped before comparing, so legacy rows that lost
/// their drive still match a mapping written as `C:\Volumes\X` — unless the mapping is a bare drive root (`U:\`), whose
/// letter is its only information. Nothing matches → separators swapped, nothing else.
pub fn translate(src: Os, dst: Os, path: &str, mappings: &[Mapping]) -> String {
    for m in mappings.iter().filter(|m| m.enabled) {
        let (sp, tp) = (m.root_for(src), m.root_for(dst));
        if sp.is_empty() || tp.is_empty() { continue }
        let source_prefix = if src == Os::current() { expand_home(sp) } else { sp.to_string() };
        let target_prefix = if dst == Os::current() { expand_home(tp) } else { tp.to_string() };
        let norm_path = path.replace('\\', "/");
        let norm_source = source_prefix.replace('\\', "/");
        fn strip_for_win(s: &str) -> &str { let s = if has_drive(s) { &s[2..] } else { s }; s.trim_start_matches('/') }
        let (case_path, cmp_path, cmp_source) = if src.is_windows() {
            let cs = strip_for_win(&norm_source);
            if cs.is_empty() { (norm_path.clone(), norm_path.to_lowercase(), norm_source.trim_end_matches('/').to_lowercase()) }
            else { let cp = strip_for_win(&norm_path); (cp.to_string(), cp.to_lowercase(), cs.to_lowercase()) }
        } else { (norm_path.clone(), norm_path.clone(), norm_source.clone()) };
        let cmp_source = cmp_source.trim_end_matches('/');
        if cmp_source.is_empty() || !cmp_path.starts_with(cmp_source) { continue }
        if cmp_path.len() > cmp_source.len() && !cmp_path[cmp_source.len()..].starts_with('/') { continue }   // boundary
        let remainder = case_path[cmp_source.len()..].trim_start_matches(['/', '\\']);
        let target_norm = target_prefix.trim_end_matches(['/', '\\']);
        let translated = if remainder.is_empty() { target_norm.to_string() } else { format!("{target_norm}/{remainder}") };
        return to_native(&translated, dst);
    }
    to_native(path, dst)
}

/// ufb's `to_canonical_path` / MinRender's `toCanonicalPath`: the current OS's native path → the canonical Windows form.
pub fn canonical_windows_form(native: &str, mappings: &[Mapping]) -> String { translate(Os::current(), Os::Win, native, mappings) }
/// The reverse.
pub fn from_canonical_windows_form(canonical: &str, mappings: &[Mapping]) -> String { translate(Os::Win, Os::current(), canonical, mappings) }

#[cfg(test)]
mod tests {
    use super::*;
    fn mapping(win: &str, mac: &str) -> Mapping { Mapping { id: "m1".into(), label: "nas".into(), win: win.into(), mac: mac.into(), lin: String::new(), enabled: true } }
    fn table() -> Vec<Mapping> { vec![Mapping { id: "01J".into(), label: "jobs".into(), win: "Z:\\union-jobs".into(), mac: "/Volumes/union-jobs".into(), lin: "/mnt/union-jobs".into(), enabled: true }] }

    // ── minNotes' refs ──
    #[test] fn to_ref_hits_with_boundary_and_list_order() {
        let t = table();
        assert_eq!(to_ref("/Volumes/union-jobs/2026/a.png", Os::Mac, &t), Ref::Vol { vol: "01J".into(), rel: vec!["2026".into(), "a.png".into()] });
        assert_eq!(to_ref("/Volumes/union-jobs-extra/a.png", Os::Mac, &t), Ref::Abs("/Volumes/union-jobs-extra/a.png".into()));
        assert_eq!(to_ref("/Volumes/union-jobs", Os::Mac, &t), Ref::Vol { vol: "01J".into(), rel: vec![] });
        assert_eq!(to_ref("z:/UNION-JOBS\\x/y.mov", Os::Win, &t), Ref::Vol { vol: "01J".into(), rel: vec!["x".into(), "y.mov".into()] });
        assert_eq!(to_ref("", Os::Mac, &t), Ref::Abs(String::new()));
    }
    #[test] fn to_ref_posix_backslash_compares_as_separator_but_segments_keep_it() {
        // row 121: comparison treats `\` as `/` on every OS; the split on POSIX is on `/` only.
        let t = table();
        assert_eq!(to_ref("\\Volumes\\union-jobs\\a.png", Os::Mac, &t), Ref::Vol { vol: "01J".into(), rel: vec!["\\a.png".into()] });
        assert_eq!(to_ref("/Volumes/union-jobs/a\\b.png", Os::Mac, &t), Ref::Vol { vol: "01J".into(), rel: vec!["a\\b.png".into()] });
    }
    #[test] fn resolve_ref_joins_with_forward_slashes_on_every_os() {
        let t = table();
        let r = Ref::Vol { vol: "01J".into(), rel: vec!["2026".into(), "a.png".into()] };
        assert_eq!(resolve_ref(&r, Os::Win, &t).unwrap(), "Z:/union-jobs/2026/a.png");
        assert_eq!(resolve_ref(&r, Os::Mac, &t).unwrap(), "/Volumes/union-jobs/2026/a.png");
        assert_eq!(resolve_ref(&r, Os::Lin, &t).unwrap(), "/mnt/union-jobs/2026/a.png");
    }
    #[test] fn resolve_ref_unknown_disabled_or_rootless_is_none() {
        let mut t = table();
        assert_eq!(resolve_ref(&Ref::Vol { vol: "nope".into(), rel: vec![] }, Os::Mac, &t), None);
        t[0].lin.clear();
        assert_eq!(resolve_ref(&Ref::Vol { vol: "01J".into(), rel: vec![] }, Os::Lin, &t), None);
        t[0].enabled = false;
        assert_eq!(resolve_ref(&Ref::Vol { vol: "01J".into(), rel: vec![] }, Os::Mac, &t), None);
    }
    #[test] fn resolve_ref_legacy_string_swaps_across_os_or_passes_through() {
        let t = table();
        assert_eq!(resolve_ref(&Ref::Abs("Z:\\union-jobs\\x\\y.mov".into()), Os::Mac, &t).unwrap(), "/Volumes/union-jobs/x/y.mov");
        assert_eq!(resolve_ref(&Ref::Abs("/Volumes/union-jobs/x/y.mov".into()), Os::Win, &t).unwrap(), "Z:/union-jobs/x/y.mov");
        assert_eq!(resolve_ref(&Ref::Abs("/Users/me/local.png".into()), Os::Win, &t).unwrap(), "/Users/me/local.png");
        assert_eq!(resolve_ref(&Ref::Abs("/Volumes/union-jobs/x".into()), Os::Mac, &t).unwrap(), "/Volumes/union-jobs/x");   // same OS: no swap
    }

    // ── ufb's translate tests, ported verbatim (the mapping helper is theirs) ──
    #[test] fn translate_win_to_mac() { assert_eq!(translate(Os::Win, Os::Mac, "\\Volumes\\studio-nas\\jobs\\", &[mapping("C:\\Volumes\\studio-nas\\jobs\\", "/Volumes/studio-nas/jobs")]), "/Volumes/studio-nas/jobs"); }
    #[test] fn translate_win_to_win_repairs_driveless() { assert_eq!(translate(Os::Win, Os::Win, "\\Volumes\\studio-nas\\jobs\\250101_demo", &[mapping("C:\\Volumes\\studio-nas\\jobs\\", "/Volumes/studio-nas/jobs")]), "C:\\Volumes\\studio-nas\\jobs\\250101_demo"); }
    #[test] fn translate_win_to_win_repairs_forward_slash() { assert_eq!(translate(Os::Win, Os::Win, "/Volumes/studio-nas/jobs/250101_demo", &[mapping("C:\\Volumes\\studio-nas\\jobs\\", "/Volumes/studio-nas/jobs")]), "C:\\Volumes\\studio-nas\\jobs\\250101_demo"); }
    #[test] fn translate_win_to_win_idempotent_on_canonical() { let c = "C:\\Volumes\\studio-nas\\jobs\\250101_demo"; assert_eq!(translate(Os::Win, Os::Win, c, &[mapping("C:\\Volumes\\studio-nas\\jobs\\", "/Volumes/studio-nas/jobs")]), c); }
    #[test] fn translate_win_to_win_no_mapping_preserves_path() { let p = "C:\\Users\\alice\\Desktop\\notes.txt"; assert_eq!(translate(Os::Win, Os::Win, p, &[mapping("C:\\Volumes\\studio-nas\\jobs\\", "/Volumes/studio-nas/jobs")]), p); }
    #[test] fn translate_mac_to_mac_idempotent() { let p = "/Volumes/studio-nas/jobs/250101_demo"; assert_eq!(translate(Os::Mac, Os::Mac, p, &[mapping("C:\\Volumes\\studio-nas\\jobs\\", "/Volumes/studio-nas/jobs")]), p); }
    #[test] fn translate_mac_to_mac_no_mapping_preserves_path() { let p = "/Users/alice/Desktop/notes.txt"; assert_eq!(translate(Os::Mac, Os::Mac, p, &[mapping("C:\\Volumes\\studio-nas\\jobs\\", "/Volumes/studio-nas/jobs")]), p); }
    #[test] fn translate_mac_to_win() { assert_eq!(translate(Os::Mac, Os::Win, "/Volumes/studio-nas/jobs/a/b.mov", &[mapping("C:\\Volumes\\studio-nas\\jobs\\", "/Volumes/studio-nas/jobs")]), "C:\\Volumes\\studio-nas\\jobs\\a\\b.mov"); }
    #[test] fn translate_bare_drive_mapping_keeps_its_letter() {
        let m = [mapping("U:\\", "/Volumes/u")];
        assert_eq!(translate(Os::Win, Os::Mac, "U:\\x\\y", &m), "/Volumes/u/x/y");
        assert_eq!(translate(Os::Win, Os::Mac, "V:\\x\\y", &m), "V:/x/y");   // another letter: no match, separators only
    }
    #[test] fn translate_disabled_or_half_mappings_are_skipped() {
        let mut m = mapping("C:\\n\\", "/Volumes/n"); m.enabled = false;
        assert_eq!(translate(Os::Win, Os::Mac, "C:\\n\\x", &[m]), "C:/n/x");
        assert_eq!(translate(Os::Win, Os::Mac, "C:\\n\\x", &[mapping("C:\\n\\", "")]), "C:/n/x");
    }
    // ── the boundary check (row 121) — MinRender's bug, now impossible ──
    #[test] fn translate_respects_component_boundary() {
        let m = [mapping("C:\\Vol\\share", "/Volumes/share")];
        assert_eq!(translate(Os::Mac, Os::Win, "/Volumes/share-extra/a", &m), "\\Volumes\\share-extra\\a");
        assert_eq!(translate(Os::Mac, Os::Win, "/Volumes/share/a", &m), "C:\\Vol\\share\\a");
        assert_eq!(translate(Os::Mac, Os::Win, "/Volumes/share", &m), "C:\\Vol\\share");
    }
}
