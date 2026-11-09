//! Interface translations (canon rows 195–197, ruled 2026-11-08).
//!
//! The English text in the source is the key, as in QCView (`qsTr("Add Media")`). Catalogs are gettext `.po` files
//! the app embeds and hands to [`register`]; [`set_language`] makes one active for the whole process. Lookups return
//! `&'static str` so a label can be translated wherever a literal sat before:
//!
//! ```
//! use min_i18n::{tr, trn, fill};
//! let title = tr("Add Media");                 // "Medien hinzufügen" with `de` active, else the English
//! let count = trn("%n frame(s)", 3);            // the language's plural form, %n filled
//! let line  = fill(tr("%1 of %2"), &[&1, &2]);  // positional placeholders, reorderable by the translator
//! # let _ = (title, count, line);
//! ```
//!
//! A message QCView's catalogs hold under several contexts with different translations (four in German: Display,
//! Range, Media, "(none)") is looked up with [`trc`] and the context name; [`tr`] takes the first one found. A message
//! with an empty translation, or no catalog at all, reads as its English. [`set_language`] can run at any time:
//! [`generation`] counts the switches so an app rebuilds the labels it built at construction (row 197's live
//! switching). Two aids for the people who check translations: [`set_pseudo`] (every looked-up text accented and
//! lengthened ~40 %, the placeholders and parenthesised key hints kept — untranslated literals and clipping stand out)
//! and [`record_misses`] / [`misses`] (what was looked up under a language and found no translation).
use std::collections::HashMap;
use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Mutex, OnceLock, RwLock};

/// Qt's numerus rules for the languages QCView shipped (gettext's Plural-Forms in each catalog match).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluralRule { One, English, French, Polish, EastSlavic }

impl PluralRule {
    pub fn for_language(code: &str) -> PluralRule {
        match code.split(['_', '-']).next().unwrap_or("") {
            "ja" | "ko" | "zh" => PluralRule::One,
            "fr" => PluralRule::French,
            "pt" if code.ends_with("BR") => PluralRule::French,
            "pl" => PluralRule::Polish,
            "ru" | "uk" => PluralRule::EastSlavic,
            _ => PluralRule::English,
        }
    }
    /// The form index for `n`.
    pub fn form(self, n: i64) -> usize {
        let n = n.unsigned_abs();
        let few = (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100));
        match self {
            PluralRule::One => 0,
            PluralRule::English => (n != 1) as usize,
            PluralRule::French => (n > 1) as usize,
            PluralRule::Polish => if n == 1 { 0 } else if few { 1 } else { 2 },
            PluralRule::EastSlavic => if n % 10 == 1 && n % 100 != 11 { 0 } else if few { 1 } else { 2 },
        }
    }
}

/// One parsed catalog. Strings are leaked once per language (a catalog is parsed at most once per process).
pub struct Catalog {
    pub language: &'static str,
    rule: PluralRule,
    by_ctx: HashMap<String, Vec<&'static str>>,   // "context\u{4}english" (gettext's own joiner)
    by_id: HashMap<&'static str, Vec<&'static str>>,
}

impl Catalog {
    /// Parse a `.po`. Lenient: unknown keywords and comments are skipped; an entry without a translation is dropped.
    pub fn parse(language: &str, po: &str) -> Catalog {
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let mut c = Catalog { language: leak(language.to_string()), rule: PluralRule::for_language(language), by_ctx: HashMap::new(), by_id: HashMap::new() };
        for e in entries(po) {
            if e.id.is_empty() || e.strs.iter().all(|s| s.is_empty()) { continue }
            let id = leak(e.id);
            let strs: Vec<&'static str> = e.strs.into_iter().map(leak).collect();
            // a context's "|disambiguation" suffix (Qt's comment) stays part of the context name
            c.by_id.entry(id).or_insert_with(|| strs.clone());
            c.by_ctx.insert(format!("{}\u{4}{id}", e.ctx), strs);
        }
        c
    }
    pub fn len(&self) -> usize { self.by_ctx.len() }
    /// The catalog's text for `english` under `context` (any context's when that one lacks it), if it has one.
    pub fn text(&self, context: &str, english: &str) -> Option<&'static str> { self.get(Some(context), english).and_then(|v| v.first().copied()).filter(|t| !t.is_empty()) }
    pub fn is_empty(&self) -> bool { self.by_ctx.is_empty() }
    fn get(&self, ctx: Option<&str>, id: &str) -> Option<&Vec<&'static str>> {
        ctx.and_then(|c| self.by_ctx.get(&format!("{c}\u{4}{id}"))).or_else(|| self.by_id.get(id))
    }
}

struct Entry { ctx: String, id: String, strs: Vec<String> }

/// The `.po` grammar this family writes: `msgctxt` / `msgid` / `msgid_plural` / `msgstr` / `msgstr[i]`, each followed
/// by one or more quoted lines; blank lines and `#` comments between entries.
fn entries(po: &str) -> Vec<Entry> {
    #[derive(PartialEq)] enum F { None, Ctx, Id, Plural, Str(usize) }
    let mut out = vec![];
    let (mut ctx, mut id, mut strs, mut field) = (String::new(), String::new(), Vec::<String>::new(), F::None);
    let mut have = false;
    let mut flush = |ctx: &mut String, id: &mut String, strs: &mut Vec<String>, have: &mut bool| {
        if *have { out.push(Entry { ctx: std::mem::take(ctx), id: std::mem::take(id), strs: std::mem::take(strs) }); }
        ctx.clear(); id.clear(); strs.clear(); *have = false;
    };
    for line in po.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') { continue }
        let (kw, rest) = match l.find(' ') { Some(i) if !l.starts_with('"') => (&l[..i], l[i + 1..].trim()), _ => ("", l) };
        let text = unquote(rest);
        match kw {
            "msgctxt" => { flush(&mut ctx, &mut id, &mut strs, &mut have); field = F::Ctx; ctx = text; }
            "msgid" => { if field != F::Ctx { flush(&mut ctx, &mut id, &mut strs, &mut have); } field = F::Id; id = text; have = true; }
            "msgid_plural" => field = F::Plural,
            "msgstr" => { field = F::Str(0); strs = vec![text]; }
            k if k.starts_with("msgstr[") => {
                let i: usize = k[7..k.len() - 1].parse().unwrap_or(0);
                if strs.len() <= i { strs.resize(i + 1, String::new()); }
                strs[i] = text; field = F::Str(i);
            }
            "" => match field { F::Ctx => ctx += &text, F::Id => id += &text, F::Plural | F::None => {}, F::Str(i) => strs[i] += &text },
            _ => {}
        }
    }
    flush(&mut ctx, &mut id, &mut strs, &mut have);
    out
}

fn unquote(s: &str) -> String {
    let s = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s);
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' { out.push(c); continue }
        match it.next() { Some('n') => out.push('\n'), Some('t') => out.push('\t'), Some('r') => out.push('\r'), Some(o) => out.push(o), None => {} }
    }
    out
}

struct State { sources: Vec<(&'static str, &'static str)>, parsed: HashMap<String, &'static Catalog>, active: Option<&'static Catalog>, english_plurals: Option<&'static Catalog> }

fn state() -> &'static RwLock<State> {
    static S: OnceLock<RwLock<State>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(State { sources: vec![], parsed: HashMap::new(), active: None, english_plurals: None }))
}

/// Hand the app's embedded catalogs over: `(code, po text)` pairs. `en` (if given) supplies only English plural forms.
pub fn register(catalogs: &[(&'static str, &'static str)]) {
    let mut s = state().write().unwrap();
    s.sources.extend_from_slice(catalogs);
    if let Some((_, po)) = catalogs.iter().find(|(c, _)| *c == "en") {
        s.english_plurals = Some(Box::leak(Box::new(Catalog::parse("en", po))));
    }
}

/// The registered language codes, `en` first.
pub fn available() -> Vec<&'static str> {
    let s = state().read().unwrap();
    let mut v: Vec<&'static str> = s.sources.iter().map(|(c, _)| *c).filter(|c| *c != "en").collect();
    v.sort(); v.insert(0, "en"); v
}

/// The registered code that serves `want`: an exact match ("pt_BR"), else the same language ("de_AT" → "de",
/// "zh-Hans" → "zh_CN", "pt" → "pt_BR"). `None` for English or an unknown language.
pub fn resolve(want: &str) -> Option<&'static str> {
    let want = want.replace('-', "_");
    let s = state().read().unwrap();
    if want.is_empty() || want == "en" || want.starts_with("en_") { return None }
    if let Some((c, _)) = s.sources.iter().find(|(c, _)| c.eq_ignore_ascii_case(&want)) { return Some(c) }
    let base = want.split('_').next().unwrap_or("");
    s.sources.iter().find(|(c, _)| *c != "en" && c.split('_').next() == Some(base)).map(|(c, _)| *c)
}

/// Make `want` the active language (resolved as [`resolve`] does); "" or "en" = the English source. Returns the
/// code now active.
pub fn set_language(want: &str) -> &'static str {
    let Some(code) = resolve(want) else { state().write().unwrap().active = None; return "en" };
    let mut s = state().write().unwrap();
    let cat: &'static Catalog = match s.parsed.get(code) {
        Some(c) => c,
        None => {
            let po = s.sources.iter().find(|(c, _)| *c == code).map(|(_, p)| *p).unwrap_or("");
            let c: &'static Catalog = Box::leak(Box::new(Catalog::parse(code, po)));
            log::info!("i18n: {code} catalog, {} messages", c.len());
            s.parsed.insert(code.to_string(), c);
            c
        }
    };
    s.active = Some(cat);
    GENERATION.fetch_add(1, Ordering::Relaxed);
    code
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static PSEUDO: AtomicBool = AtomicBool::new(false);
static RECORD: AtomicBool = AtomicBool::new(false);
fn missed() -> &'static Mutex<std::collections::BTreeSet<(String, String)>> { static M: OnceLock<Mutex<std::collections::BTreeSet<(String, String)>>> = OnceLock::new(); M.get_or_init(Default::default) }
fn pseudo_cache() -> &'static Mutex<HashMap<String, &'static str>> { static M: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new(); M.get_or_init(Default::default) }

/// Bumped by every [`set_language`] (and [`set_pseudo`]): labels built before a change differ from it.
pub fn generation() -> u64 { GENERATION.load(Ordering::Relaxed) }
/// Pseudo-localisation on / off (QCView's `QCV_PSEUDO_LOC`).
pub fn set_pseudo(on: bool) { PSEUDO.store(on, Ordering::Relaxed); GENERATION.fetch_add(1, Ordering::Relaxed); }
/// Start recording the lookups the active language has no translation for.
pub fn record_misses(on: bool) { RECORD.store(on, Ordering::Relaxed); }
/// The recorded misses as (context, English), sorted.
pub fn misses() -> Vec<(String, String)> { missed().lock().unwrap().iter().cloned().collect() }

/// The pseudo form: Latin letters accented, ~40 % longer, in brackets; `%1`..`%9`, `%n`, a trailing "(key)" hint and
/// the text inside `{…}` untouched.
pub fn pseudo(text: &str) -> String {
    let (body, hint) = match text.rfind(" (") { Some(i) if text.ends_with(')') => (&text[..i], &text[i..]), _ => (text, "") };
    let mut out = String::from("[");
    let mut it = body.chars().peekable(); let mut letters = 0usize;
    while let Some(c) = it.next() {
        if c == '%' { if let Some(&n) = it.peek() { if n.is_ascii_digit() || n == 'n' { out.push('%'); out.push(n); it.next(); continue } } }
        let m = match c { 'a' => 'á', 'e' => 'é', 'i' => 'î', 'o' => 'ö', 'u' => 'ü', 'c' => 'ç', 'n' => 'ñ', 'y' => 'ý', 'A' => 'Å', 'E' => 'É', 'I' => 'Î', 'O' => 'Ö', 'U' => 'Ü', 'C' => 'Ç', 'N' => 'Ñ', 's' => 'š', 'S' => 'Š', 'z' => 'ž', other => other };
        if c.is_alphabetic() { letters += 1; }
        out.push(m);
    }
    for i in 0..(letters * 2).div_ceil(5) { out.push(if i % 2 == 0 { '~' } else { '·' }); }
    out.push(']'); out += hint; out
}

/// The active language code ("en" when none).
pub fn language() -> &'static str {
    state().read().unwrap().active.map(|c| c.language).unwrap_or("en")
}

fn lookup<'a>(ctx: Option<&str>, id: &'a str) -> &'a str {
    let s = state().read().unwrap();
    let found = s.active.and_then(|c| c.get(ctx, id)).and_then(|v| v.first().copied()).filter(|t| !t.is_empty());
    if found.is_none() && s.active.is_some() && RECORD.load(Ordering::Relaxed) && !id.trim().is_empty() { missed().lock().unwrap().insert((ctx.unwrap_or("").to_string(), id.to_string())); }
    let text = found.unwrap_or(id);
    if PSEUDO.load(Ordering::Relaxed) && !text.trim().is_empty() {
        let mut c = pseudo_cache().lock().unwrap();
        if let Some(p) = c.get(text) { return p }
        let p: &'static str = Box::leak(pseudo(text).into_boxed_str()); c.insert(text.to_string(), p); return p
    }
    text
}

/// The active language's text for `english`.
pub fn tr<'a>(english: &'a str) -> &'a str { lookup(None, english) }

/// [`tr`] under QCView's context name ("InspectorPanel", "LeftRail" …): the right word where one English word has two
/// translations; falls back to any context's translation when that context does not hold the message.
pub fn trc<'a>(context: &str, english: &'a str) -> &'a str { lookup(Some(context), english) }

/// A tooltip with a shortcut hint ("Skip to start (Home)"): the whole text's translation when the catalog has it,
/// else the translated head with the parenthesised hint kept as written (QCView's glossary: key names stay).
pub fn trc_hint(context: &str, english: &str) -> String {
    let whole = lookup(Some(context), english);
    if !std::ptr::eq(whole, english) { return whole.to_string() }
    match english.rfind(" (") { Some(i) if english.ends_with(')') => format!("{}{}", lookup(Some(context), &english[..i]), &english[i..]), _ => english.to_string() }
}

/// A count: the language's plural form of `english` (a `%n` message), with `%n` replaced by `n`.
pub fn trn(english: &str, n: i64) -> String { trnc(None, english, n) }

/// [`trn`] under a context.
pub fn trnc(context: Option<&str>, english: &str, n: i64) -> String {
    let s = state().read().unwrap();
    let pick = |c: &Catalog| c.get(context, english).and_then(|v| v.get(c.rule.form(n)).or(v.first()).copied()).filter(|t| !t.is_empty());
    let found = s.active.and_then(pick);
    if found.is_none() && s.active.is_some() && RECORD.load(Ordering::Relaxed) { missed().lock().unwrap().insert((context.unwrap_or("").to_string(), english.to_string())); }
    let text = found.or_else(|| s.english_plurals.and_then(pick)).unwrap_or(english);
    let text = if PSEUDO.load(Ordering::Relaxed) { pseudo(text) } else { text.to_string() };
    text.replace("%n", &n.to_string())
}

/// Fill `%1`..`%9` (Qt's `arg()` order: the number names the argument, wherever the translator put it).
pub fn fill(text: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 1 < b.len() && b[i + 1].is_ascii_digit() && b[i + 1] != b'0' {
            let k = (b[i + 1] - b'1') as usize;
            if let Some(a) = args.get(k) { out += &a.to_string(); i += 2; continue }
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch); i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    const DE: &str = "msgid \"\"\nmsgstr \"\"\n\"Language: de\\n\"\n\n#. a note\nmsgctxt \"LeftRail\"\nmsgid \"Add Media\"\nmsgstr \"Medien hinzuf\u{fc}gen\"\n\nmsgctxt \"A\"\nmsgid \"Range\"\nmsgstr \"Bereich\"\n\nmsgctxt \"B\"\nmsgid \"Range\"\nmsgstr \"Umfang\"\n\nmsgctxt \"X\"\nmsgid \"Long\"\nmsgstr \"\"\n\"two \\\"lines\\\"\\n\"\n\"here\"\n\nmsgctxt \"X\"\nmsgid \"Empty\"\nmsgstr \"\"\n\nmsgctxt \"X\"\nmsgid \"%n frame(s)\"\nmsgid_plural \"%n frame(s)\"\nmsgstr[0] \"%n Bild\"\nmsgstr[1] \"%n Bilder\"\n";
    const PL: &str = "msgctxt \"X\"\nmsgid \"%n frame(s)\"\nmsgid_plural \"%n frame(s)\"\nmsgstr[0] \"%n klatka\"\nmsgstr[1] \"%n klatki\"\nmsgstr[2] \"%n klatek\"\n";
    const EN: &str = "msgctxt \"X\"\nmsgid \"%n frame(s)\"\nmsgid_plural \"%n frame(s)\"\nmsgstr[0] \"%n frame\"\nmsgstr[1] \"%n frames\"\n";

    #[test]
    fn catalogs_lookup_plurals_and_fallbacks() {
        register(&[("de", DE), ("pl", PL), ("en", EN), ("pt_BR", "")]);
        assert_eq!(set_language(""), "en");
        assert_eq!(tr("Add Media"), "Add Media");
        assert_eq!(trn("%n frame(s)", 1), "1 frame");
        assert_eq!(trn("%n frame(s)", 2), "2 frames");
        assert_eq!(set_language("de_AT"), "de");
        assert_eq!(tr("Add Media"), "Medien hinzuf\u{fc}gen");
        assert_eq!(trc("B", "Range"), "Umfang");
        assert_eq!(tr("Long"), "two \"lines\"\nhere");
        assert_eq!(tr("Empty"), "Empty");
        assert_eq!(tr("Not in the catalog"), "Not in the catalog");
        assert_eq!(trn("%n frame(s)", 5), "5 Bilder");
        assert_eq!(trc_hint("LeftRail", "Add Media (Ctrl+O)"), "Medien hinzuf\u{fc}gen (Ctrl+O)");
        assert_eq!(trc_hint("LeftRail", "Unknown (X)"), "Unknown (X)");
        assert_eq!(set_language("pl"), "pl");
        assert_eq!([1, 3, 5, 12, 22, 25].map(|n| trn("%n frame(s)", n)), ["1 klatka", "3 klatki", "5 klatek", "12 klatek", "22 klatki", "25 klatek"]);
        assert_eq!(resolve("pt"), Some("pt_BR"));
        assert_eq!(resolve("en_GB"), None);
        assert_eq!(available()[0], "en");
        set_language("en");
    }

    #[test]
    fn plural_rules_match_qt() {
        use PluralRule::*;
        assert_eq!([0, 1, 2].map(|n| French.form(n)), [0, 0, 1]);
        assert_eq!([0, 1, 2].map(|n| English.form(n)), [1, 0, 1]);
        assert_eq!([1, 2, 5, 11, 21, 111, 112].map(|n| EastSlavic.form(n)), [0, 1, 2, 2, 0, 2, 2]);
        assert_eq!(PluralRule::for_language("pt_BR"), French);
        assert_eq!(PluralRule::for_language("zh_CN"), One);
    }

    #[test]
    fn pseudo_keeps_placeholders_and_hints() {
        assert_eq!(pseudo("Play (Space)"), "[Pláý~·] (Space)");
        let p = pseudo("Couldn't open: %1"); assert!(p.contains("%1") && p.starts_with('[') && p.chars().count() > "Couldn't open: %1".chars().count() + 4, "{p}");
        assert!(pseudo("%n frames").starts_with("[%n"));
    }

    #[test]
    fn fill_is_positional() {
        assert_eq!(fill("%2 von %1", &[&"a", &"b"]), "b von a");
        assert_eq!(fill("100% %1 %3", &[&7]), "100% 7 %3");
    }
}
