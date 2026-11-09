//! The interface catalogs (canon rows 195–197): QCView's 11 translations carried as gettext `.po` (tools/i18n/ts2po.py),
//! embedded in the binary, plus the strings that are ours (tools/i18n/extract.py finds them). The language is the
//! Settings ▸ Interface ▸ Language picker's (`ui/language`; "" = System default = the OS's preferred languages, QCView's
//! `QLocale::system()`); it applies at once (QCView: "Applies at once") — the panels relabel on `min_i18n::generation`.
//! QCView's switches carry: `QCV_LANG=<code>` beats the setting, `QCV_TRANSLATIONS_DIR=<dir>` loads loose
//! `qcview_<code>.po` files ahead of the embedded ones (a translator's edit, no rebuild), `QCV_PSEUDO_LOC=1` the
//! pseudo-localisation. Ours: `QCV_I18N_MISSES=<file>` writes, at exit, every lookup the language had no translation for.
macro_rules! catalogs { ($($code:literal),*) => { &[$(($code, include_str!(concat!("../i18n/qcview_", $code, ".po")))),*] } }

pub const CATALOGS: &[(&str, &str)] = catalogs!("en", "de", "es", "fr", "it", "ja", "ko", "pl", "pt_BR", "ru", "uk", "zh_CN");

/// Register the catalogs (loose files first) and the env switches. Once, at startup, before any panel is built.
pub fn install() {
    if let Some(dir) = std::env::var_os("QCV_TRANSLATIONS_DIR") {
        let mut loose: Vec<(&'static str, &'static str)> = vec![];
        for (code, _) in CATALOGS {
            let p = std::path::Path::new(&dir).join(format!("qcview_{code}.po"));
            if let Ok(text) = std::fs::read_to_string(&p) { loose.push((code, Box::leak(text.into_boxed_str()))); log::info!("i18n: loose catalog {}", p.display()); }
        }
        min_i18n::register(&loose);
    }
    min_i18n::register(CATALOGS);
    if std::env::var("QCV_PSEUDO_LOC").is_ok_and(|v| v != "0" && !v.is_empty()) { min_i18n::set_pseudo(true); log::info!("i18n: pseudo-localisation on (QCV_PSEUDO_LOC)"); }
    if std::env::var_os("QCV_I18N_MISSES").is_some() { min_i18n::record_misses(true); }
    if let Ok(want) = std::env::var("QCV_LANG") { let got = min_i18n::set_language(&want); log::info!("i18n: QCV_LANG={want} → {got}"); }
}

/// Apply the setting (`""` = the OS's languages, first one we carry); `QCV_LANG` wins. Returns the active code.
pub fn apply(setting: &str) -> &'static str {
    if std::env::var_os("QCV_LANG").is_some() { return min_i18n::language() }
    let got = if setting.is_empty() { system_language() } else { min_i18n::set_language(setting) };
    log::info!("i18n: setting {setting:?} → {got}");
    got
}

/// The OS's preferred languages in order; the first we carry wins, English ends the search (QCView: `QLocale::system()`).
fn system_language() -> &'static str {
    for l in sys_locale::get_locales() {
        let l = l.replace('-', "_");
        if l == "en" || l.starts_with("en_") { break }
        // "zh_Hans_CN" / "zh_CN" → zh_CN; "pt_BR" stays; a bare "pt" resolves to pt_BR in min-i18n
        let l = if l.starts_with("zh") { "zh_CN".to_string() } else { l };
        if min_i18n::resolve(&l).is_some() { return min_i18n::set_language(&l) }
    }
    min_i18n::set_language("en")
}

/// `QCV_I18N_MISSES`: the recorded misses, one "context<TAB>English" per line.
pub fn write_misses() {
    let Some(path) = std::env::var_os("QCV_I18N_MISSES") else { return };
    let text: String = min_i18n::misses().into_iter().map(|(c, e)| format!("{c}\t{}\n", e.replace('\n', "\\n"))).collect();
    match std::fs::write(&path, text) { Ok(()) => log::info!("i18n: misses written to {}", std::path::Path::new(&path).display()), Err(e) => log::warn!("i18n: misses: {e}") }
}

#[cfg(test)]
mod tests {
    /// Every carried catalog parses whole, and no translation lost or invented a placeholder (QCView's import rule).
    #[test]
    fn carried_catalogs_parse_and_keep_their_placeholders() {
        let ph = |s: &str| { let mut v: Vec<String> = s.match_indices('%').filter_map(|(i, _)| s[i + 1..].chars().next().filter(|c| c.is_ascii_digit() || *c == 'n').map(|c| format!("%{c}"))).collect(); v.sort(); v.dedup(); v };
        // a block's fields: keyword -> its quoted lines joined (msgid, msgstr, msgstr[0] …)
        let fields = |e: &str| { let mut out: Vec<(String, String)> = vec![]; for l in e.lines() { let l = l.trim(); if l.starts_with('#') { continue }
            if l.starts_with('"') { if let Some(last) = out.last_mut() { last.1 += l.trim_matches('"'); } } else if let Some((k, v)) = l.split_once(' ') { out.push((k.to_string(), v.trim().trim_matches('"').to_string())); } } out };
        for (code, po) in super::CATALOGS {
            let c = min_i18n::Catalog::parse(code, po);
            if *code == "en" { assert!(c.len() >= 8, "en plurals"); continue }
            assert!(c.len() >= 850, "{code}: {} messages", c.len());
            let mut bad = vec![];
            for e in po.split("\n\n").skip(1) {
                let f = fields(e);
                let Some(id) = f.iter().find(|(k, _)| k == "msgid").map(|(_, v)| v.clone()) else { continue };
                let want: Vec<String> = ph(&id).into_iter().filter(|p| p != "%n").collect();
                for (_, t) in f.iter().filter(|(k, _)| k.starts_with("msgstr")) {
                    let got: Vec<String> = ph(t).into_iter().filter(|p| p != "%n").collect();
                    if !t.is_empty() && got != want { bad.push((id.clone(), t.clone())); }
                }
            }
            assert!(bad.is_empty(), "{code}: placeholders differ: {bad:?}");
        }
    }
}
