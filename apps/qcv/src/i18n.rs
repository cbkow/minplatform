//! The interface catalogs (canon rows 195–197): QCView's 11 translations carried as gettext `.po` (tools/i18n/ts2po.py),
//! embedded in the binary. Until the end-of-port localisation pass (row 197: the Language picker, live switching, the
//! system default, the CJK font check) the language comes only from `QCV_LANG=<code>`; the app runs in English otherwise.
macro_rules! catalogs { ($($code:literal),*) => { &[$(($code, include_str!(concat!("../i18n/qcview_", $code, ".po")))),*] } }

pub const CATALOGS: &[(&str, &str)] = catalogs!("en", "de", "es", "fr", "it", "ja", "ko", "pl", "pt_BR", "ru", "uk", "zh_CN");

/// Register the catalogs and apply `QCV_LANG`. Once, before any panel is built (labels translate at construction).
pub fn install() {
    min_i18n::register(CATALOGS);
    if let Ok(want) = std::env::var("QCV_LANG") {
        let got = min_i18n::set_language(&want);
        log::info!("i18n: QCV_LANG={want} → {got}");
    }
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
