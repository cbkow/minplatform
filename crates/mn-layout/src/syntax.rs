//! The colouring of code (canon row 337): a code block's text cut into comments, strings, numbers, keywords, types and
//! function names - six kinds, one colour each - by a small reader of our own driven by a short table per language. It
//! is what the editor shapes a code block with and what every export writes, so they agree. It is not a grammar: it
//! knows how a language spells its comments and strings and which words are its own, and no more. A language it does
//! not know, and a block over [`LARGEST_LINES`], stay in one colour.
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok { Comment, Str, Number, Keyword, Type, Func }
impl Tok {
    pub fn index(self) -> usize { self as usize }
    /// Its class in an HTML export.
    pub fn class(self) -> &'static str { ["c", "s", "n", "k", "t", "f"][self as usize] }
}
/// The six colours on the dark page (the notes' named colours, row 301; the editor takes its comment grey from the
/// theme) and on white paper, in [`Tok`]'s order.
pub const DARK: [&str; 6] = ["#8E8E8E", "#86E36B", "#FFA23A", "#9D8DF1", "#00CDAC", "#00A5E3"];
pub const LIGHT: [&str; 6] = ["#6D6D6D", "#1A7F37", "#B35900", "#6F42C1", "#0A7C86", "#0550AE"];
/// A block of more lines than this is not coloured: typing in it must not wait for its colours.
pub const LARGEST_LINES: usize = 5000;

#[derive(Clone, Copy)]
enum Shape { Code, Markup, /// `key: value` / `key = value` lines (the separator; whether `[section]` lines exist)
    Keyed(u8, bool) }

struct Lang {
    names: &'static [&'static str], shape: Shape,
    /// what starts a comment to the line's end, and the pairs that hold one
    line: &'static [&'static str], block: &'static [(&'static str, &'static str)],
    /// the characters that open a string; `chars`: a `'` holds one character only (else it is a lifetime, a tick);
    /// `multi`: a string may run over lines; `triple`: `"""` strings; `esc`: a backslash takes the next character
    quotes: &'static str, chars: bool, multi: bool, triple: bool, esc: bool,
    keywords: &'static str, types: &'static str, /// keywords in any case
    ci: bool, /// a Capitalised word is a type
    caps: bool, /// a word before `(` is a function
    funcs: bool, /// `$name`, `@name`: variables, coloured as types
    sigils: &'static str, /// `#include` and its kin
    pre: bool, /// a string before `:` is a key (JSON)
    keys: bool, /// what `@name` is (a decorator, an at-rule)
    at: Option<Tok>, /// CSS: `property: `, `#fff`
    css: bool, /// a name may hold `-` / `$`
    dash: bool, dollar: bool,
}
const BASE: Lang = Lang { names: &[], shape: Shape::Code, line: &["//"], block: &[("/*", "*/")], quotes: "\"'", chars: false, multi: false, triple: false, esc: true, keywords: "", types: "", ci: false, caps: true, funcs: true,
    sigils: "", pre: false, keys: false, at: None, css: false, dash: false, dollar: false };
const HASH: Lang = Lang { line: &["#"], block: &[], ..BASE };

macro_rules! c_kw { () => { "auto break case const continue default do else enum extern for goto if inline register restrict return sizeof static struct switch typedef union volatile while true false NULL" } }
macro_rules! c_ty { () => { "void char short int long float double signed unsigned bool size_t ssize_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t" } }
macro_rules! cpp_kw { () => { concat!(c_kw!(), " class namespace template typename public private protected virtual override final using new delete try catch throw this nullptr constexpr noexcept operator friend explicit mutable static_cast dynamic_cast const_cast reinterpret_cast decltype concept requires co_await co_return co_yield export import module") } }
macro_rules! js_kw { () => { "async await break case catch class const continue debugger default delete do else export extends false finally for from function get if import in instanceof let new null of return set static super switch this throw true try typeof undefined var void while with yield" } }

static LANGS: &[Lang] = &[
    Lang { names: &["py", "python", "python3"], triple: true, at: Some(Tok::Func), keywords: "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield match case self", types: "int float str bool bytes list dict set tuple object", ..HASH },
    Lang { names: &["js", "javascript", "jsx", "mjs", "cjs", "node"], quotes: "\"'`", dollar: true, keywords: js_kw!(), ..BASE },
    Lang { names: &["ts", "typescript", "tsx"], quotes: "\"'`", dollar: true, at: Some(Tok::Func), keywords: concat!(js_kw!(), " abstract as declare enum implements interface is keyof namespace private protected public readonly type satisfies infer override"), types: "any boolean never number object string symbol unknown bigint", ..BASE },
    Lang { names: &["c", "h"], chars: true, pre: true, caps: false, keywords: c_kw!(), types: c_ty!(), ..BASE },
    Lang { names: &["cpp", "cc", "cxx", "hpp", "hxx", "c++"], chars: true, pre: true, keywords: cpp_kw!(), types: concat!(c_ty!(), " string wstring"), ..BASE },
    Lang { names: &["m", "mm", "objc", "objective-c", "objectivec"], chars: true, pre: true, at: Some(Tok::Keyword), keywords: concat!(cpp_kw!(), " id self super nil YES NO instancetype"), types: concat!(c_ty!(), " BOOL"), ..BASE },
    Lang { names: &["glsl", "hlsl", "frag", "vert", "shader"], pre: true, caps: false, keywords: concat!(c_kw!(), " uniform varying attribute in out inout precision highp mediump lowp discard layout"), types: concat!(c_ty!(), " vec2 vec3 vec4 ivec2 ivec3 ivec4 uvec2 uvec3 uvec4 mat2 mat3 mat4 sampler2D sampler3D samplerCube float2 float3 float4 float4x4"), ..BASE },
    Lang { names: &["cs", "csharp", "c#"], chars: true, pre: true, keywords: "abstract as base break case catch checked class const continue default delegate do else enum event explicit extern false finally fixed for foreach get goto if implicit in interface internal is lock namespace new null operator out override params private protected public readonly ref return sealed set sizeof stackalloc static struct switch this throw true try typeof unchecked unsafe using var virtual void volatile while async await yield record init where when partial", types: "bool byte char decimal double float int long object sbyte short string uint ulong ushort dynamic", ..BASE },
    Lang { names: &["java"], chars: true, at: Some(Tok::Func), keywords: "abstract assert break case catch class const continue default do else enum extends final finally for goto if implements import instanceof interface native new package private protected public return static strictfp super switch synchronized this throw throws transient try void volatile while var record sealed permits yield true false null", types: "boolean byte char double float int long short", ..BASE },
    Lang { names: &["kt", "kts", "kotlin"], chars: true, triple: true, at: Some(Tok::Func), keywords: "as break class continue do else false for fun if in interface is null object package return super this throw true try typealias typeof val var when while by catch constructor finally get import init set where abstract annotation companion const crossinline data enum external final infix inline inner internal lateinit open operator out override private protected public sealed suspend tailrec vararg", ..BASE },
    Lang { names: &["swift"], quotes: "\"", triple: true, at: Some(Tok::Func), keywords: "actor as associatedtype async await break case catch class continue default defer deinit do else enum extension fallthrough false fileprivate final for func guard if import in indirect init inout internal is lazy let mutating nil nonisolated open operator override private protocol public repeat rethrows return self Self some static struct subscript super switch throw throws true try typealias var weak unowned where while any", ..BASE },
    Lang { names: &["go", "golang"], quotes: "\"'`", chars: true, caps: false, keywords: "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var true false nil iota", types: "bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint uint8 uint16 uint32 uint64 uintptr any", ..BASE },
    Lang { names: &["rs", "rust"], chars: true, multi: true, keywords: "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while union macro_rules", types: "bool char f32 f64 i8 i16 i32 i64 i128 isize str u8 u16 u32 u64 u128 usize", ..BASE },
    Lang { names: &["rb", "ruby"], block: &[("=begin", "=end")], quotes: "\"'`", multi: true, sigils: "@$", keywords: "alias and begin BEGIN break case class def do else elsif end END ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield require require_relative attr_accessor attr_reader attr_writer puts", ..HASH },
    Lang { names: &["php"], line: &["//", "#"], multi: true, ci: true, sigils: "$", keywords: "abstract and array as break callable case catch class clone const continue declare default do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile extends final finally fn for foreach function global goto if implements include include_once instanceof insteadof interface isset list match namespace new or print private protected public readonly require require_once return static switch throw trait try unset use var while xor yield true false null", types: "int float string bool object mixed void iterable", ..BASE },
    Lang { names: &["sh", "bash", "zsh", "shell", "console", "fish", "ksh"], quotes: "\"'`", multi: true, caps: false, funcs: false, sigils: "$", keywords: "if then else elif fi for while until do done case esac in function select time return exit break continue export local readonly declare unset shift source alias set eval exec trap", ..HASH },
    Lang { names: &["sql", "sqlite", "mysql", "postgres", "postgresql", "psql"], line: &["--"], quotes: "\"'`", multi: true, esc: false, ci: true, caps: false, keywords: "select from where and or not in is null like between join inner left right full outer cross on as group by order having limit offset union all distinct insert into values update set delete create table view index drop alter add column primary key foreign references default unique check constraint if exists case when then else end begin commit rollback transaction with recursive returning asc desc true false trigger procedure function database schema grant revoke explain analyze pragma replace", types: "int integer bigint smallint tinyint real float double decimal numeric char varchar text blob date time datetime timestamp boolean bool serial uuid json jsonb", ..BASE },
    Lang { names: &["json", "jsonc", "json5", "geojson"], quotes: "\"", caps: false, funcs: false, keys: true, keywords: "true false null", ..BASE },
    Lang { names: &["yaml", "yml"], shape: Shape::Keyed(b':', false), caps: false, funcs: false, keywords: "true false null yes no on off True False Null", ..HASH },
    Lang { names: &["toml"], shape: Shape::Keyed(b'=', true), caps: false, funcs: false, keywords: "true false inf nan", ..HASH },
    Lang { names: &["ini", "cfg", "conf", "properties", "env"], shape: Shape::Keyed(b'=', true), line: &["#", ";"], caps: false, funcs: false, keywords: "true false yes no on off", ..HASH },
    Lang { names: &["xml", "html", "htm", "xhtml", "svg", "plist", "xaml", "xsl", "vue"], shape: Shape::Markup, ..BASE },
    Lang { names: &["css"], line: &[], caps: false, css: true, dash: true, at: Some(Tok::Keyword), keywords: "important", ..BASE },
    Lang { names: &["scss", "sass", "less"], caps: false, css: true, dash: true, sigils: "$", at: Some(Tok::Keyword), keywords: "important", ..BASE },
    Lang { names: &["lua"], line: &["--"], block: &[("--[[", "]]")], keywords: "and break do else elseif end false for function goto if in local nil not or repeat return then true until while", caps: false, ..BASE },
    Lang { names: &["pl", "pm", "perl"], multi: true, sigils: "$@", caps: false, keywords: "my our local sub if elsif else unless while until for foreach do last next redo return use no package require print say die eq ne lt gt le ge cmp and or not undef defined shift push pop keys values each exists delete", ..HASH },
    Lang { names: &["r"], caps: false, keywords: "if else repeat while function for next break in TRUE FALSE NULL NA Inf NaN library require return", ..HASH },
    Lang { names: &["cmake"], ci: true, caps: false, sigils: "$", quotes: "\"", multi: true, keywords: "if elseif else endif foreach endforeach while endwhile function endfunction macro endmacro return break continue on off true false and or not strequal equal matches defined exists public private interface required", ..HASH },
    Lang { names: &["ps1", "psm1", "powershell", "pwsh"], block: &[("<#", "#>")], ci: true, caps: false, funcs: false, sigils: "$", dash: true, multi: true, esc: false, keywords: "begin break catch class continue data do dynamicparam else elseif end enum exit filter finally for foreach from function if in param process return switch throw trap try until using while", ..HASH },
];

/// What a picker offers: (the short name a block stores, the name shown), in the shown names' order.
pub const NAMES: &[(&str, &str)] = &[("sh", "Bash / Shell"), ("c", "C"), ("cpp", "C++"), ("cs", "C#"), ("cmake", "CMake"), ("css", "CSS"), ("glsl", "GLSL"), ("go", "Go"), ("html", "HTML"), ("ini", "INI"), ("java", "Java"), ("js", "JavaScript"), ("json", "JSON"),
    ("kt", "Kotlin"), ("lua", "Lua"), ("m", "Objective-C"), ("pl", "Perl"), ("php", "PHP"), ("ps1", "PowerShell"), ("py", "Python"), ("r", "R"), ("rb", "Ruby"), ("rs", "Rust"), ("scss", "SCSS"), ("sql", "SQL"), ("swift", "Swift"), ("toml", "TOML"), ("ts", "TypeScript"), ("xml", "XML"), ("yaml", "YAML")];
/// A known language's shown name ("python", "py" -> "Python"); None for one that is not known.
pub fn title(lang: &str) -> Option<&'static str> {
    let l = lang.trim().to_ascii_lowercase(); if let Some(n) = NAMES.iter().find(|n| n.0 == l) { return Some(n.1) }
    find(&l)?.names.iter().find_map(|x| NAMES.iter().find(|n| n.0 == *x).map(|n| n.1))
}
/// The short name of a known language as a block stores it.
pub fn short(lang: &str) -> Option<&'static str> { let t = title(lang)?; let l = lang.trim().to_ascii_lowercase(); NAMES.iter().find(|n| n.0 == l).or_else(|| NAMES.iter().find(|n| n.1 == t)).map(|n| n.0) }

fn find(lang: &str) -> Option<&'static Lang> { let l = lang.trim().to_ascii_lowercase(); if l.is_empty() { return None } LANGS.iter().find(|x| x.names.contains(&l.as_str())) }
/// Whether a block of this language is coloured.
pub fn known(lang: &str) -> bool { find(lang).is_some() }

/// The coloured stretches of `text` (byte ranges on character boundaries, in order, apart); what is between them keeps
/// the text's own colour.
pub fn lex(lang: &str, text: &str) -> Vec<(Range<usize>, Tok)> {
    let Some(l) = find(lang) else { return vec![] };
    if text.bytes().filter(|b| *b == b'\n').count() >= LARGEST_LINES { return vec![] }
    let mut out = vec![];
    match l.shape { Shape::Code => code(l, text, 0, &mut out), Shape::Markup => markup(text, &mut out), Shape::Keyed(sep, sections) => keyed(l, text, sep, sections, &mut out) }
    out
}
/// `text` as its pieces in order, each with its kind (None: plain) - what an export writes run by run.
pub fn pieces<'a>(lang: &str, text: &'a str) -> Vec<(&'a str, Option<Tok>)> {
    let mut out = vec![]; let mut at = 0;
    for (r, t) in lex(lang, text) { if r.start > at { out.push((&text[at..r.start], None)); } out.push((&text[r.clone()], Some(t))); at = r.end; }
    if at < text.len() { out.push((&text[at..], None)); }
    out
}

fn word_in(list: &str, w: &str, ci: bool) -> bool { if ci { list.split(' ').any(|k| k.eq_ignore_ascii_case(w)) } else { list.split(' ').any(|k| k == w) } }
fn find_from(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> { if from > b.len() || pat.is_empty() { return None } b[from..].windows(pat.len()).position(|w| w == pat).map(|i| from + i) }

fn code(l: &Lang, s: &str, base: usize, out: &mut Vec<(Range<usize>, Tok)>) {
    let b = s.as_bytes(); let n = b.len(); let mut i = 0;
    let start = |c: u8| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 || (l.dollar && c == b'$');
    let part = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 || (l.dollar && c == b'$') || (l.dash && c == b'-');
    let loose = matches!(l.shape, Shape::Keyed(..));
    let mut put = |a: usize, z: usize, t: Tok| if z > a { out.push((base + a..base + z, t)) };
    'next: while i < n {
        let c = b[i]; let at = &b[i..]; let after_word = i > 0 && part(b[i - 1]);
        if c.is_ascii_whitespace() { i += 1; continue }
        for (open, close) in l.block { if at.starts_with(open.as_bytes()) { let z = find_from(b, i + open.len(), close.as_bytes()).map(|j| j + close.len()).unwrap_or(n); put(i, z, Tok::Comment); i = z; continue 'next } }
        for p in l.line { if at.starts_with(p.as_bytes()) {
            // a `#` or a `;` is a comment at a line's start or after a space (not `$#`, `a;b`); `//` is not one in `http://`
            let ok = match *p { "#" | ";" => i == 0 || b[i - 1].is_ascii_whitespace(), "//" => i == 0 || b[i - 1] != b':', _ => true };
            if ok { let z = find_from(b, i, b"\n").unwrap_or(n); put(i, z, Tok::Comment); i = z; continue 'next } } }
        if l.pre && c == b'#' && b[..i].iter().rev().take_while(|x| **x != b'\n').all(|x| x.is_ascii_whitespace()) {
            let mut j = i + 1; while j < n && (b[j] == b' ' || b[j] == b'\t') { j += 1; } while j < n && b[j].is_ascii_alphabetic() { j += 1; }
            put(i, j, Tok::Keyword); i = j; continue }
        if l.triple && (at.starts_with(b"\"\"\"") || at.starts_with(b"'''")) { let z = find_from(b, i + 3, &at[..3]).map(|j| j + 3).unwrap_or(n); put(i, z, Tok::Str); i = z; continue }
        if l.quotes.as_bytes().contains(&c) && !(loose && after_word) {
            if c == b'\'' && l.chars {
                // one character between the ticks, or an escape; else it is not a string (a lifetime)
                let z = if i + 1 < n && b[i + 1] == b'\\' { (i + 3..n.min(i + 12)).find(|j| b[*j] == b'\'') } else { s[i + 1..].chars().next().map(|ch| i + 1 + ch.len_utf8()).filter(|j| *j < n && b[*j] == b'\'' && b[i + 1] != b'\'') };
                match z { Some(z) => { put(i, z + 1, Tok::Str); i = z + 1; } None => i += 1 }
                continue
            }
            let mut j = i + 1;
            while j < n { if b[j] == b'\\' && l.esc { j += 2; continue } if b[j] == c { j += 1; break } if b[j] == b'\n' && !(l.multi || c == b'`') { break } j += 1; }
            let j = j.min(n);
            let key = l.keys && { let mut k = j; while k < n && (b[k] == b' ' || b[k] == b'\t') { k += 1; } k < n && b[k] == b':' };
            put(i, j, if key { Tok::Type } else { Tok::Str }); i = j; continue
        }
        if l.css && c == b'#' && i + 1 < n && b[i + 1].is_ascii_hexdigit() { let mut j = i + 1; while j < n && b[j].is_ascii_alphanumeric() { j += 1; } put(i, j, Tok::Number); i = j; continue }
        if !after_word && (c.is_ascii_digit() || (c == b'.' && i + 1 < n && b[i + 1].is_ascii_digit())) {
            let hex = c == b'0' && i + 1 < n && matches!(b[i + 1], b'x' | b'X'); let mut j = i + 1;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || (b[j] == b'.' && j + 1 < n && b[j + 1].is_ascii_digit()) || (!hex && matches!(b[j], b'+' | b'-') && matches!(b[j - 1], b'e' | b'E') && j + 1 < n && b[j + 1].is_ascii_digit())) { j += 1; }
            put(i, j, Tok::Number); i = j; continue
        }
        if l.sigils.as_bytes().contains(&c) && i + 1 < n {
            let x = b[i + 1];
            if c == b'$' && x == b'{' { let z = (i + 2..n).find(|j| b[*j] == b'}' || b[*j] == b'\n').filter(|j| b[*j] == b'}').map(|j| j + 1); if let Some(z) = z { put(i, z, Tok::Type); i = z; continue } }
            else if x.is_ascii_alphanumeric() || x == b'_' || x >= 0x80 { let mut j = i + 1; while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] >= 0x80) { j += 1; } put(i, j, Tok::Type); i = j; continue }
        }
        if let (Some(t), b'@') = (l.at, c) { if i + 1 < n && b[i + 1].is_ascii_alphabetic() && !after_word { let mut j = i + 1; while j < n && part(b[j]) { j += 1; } put(i, j, t); i = j; continue } }
        if start(c) {
            let mut j = i + 1; while j < n && part(b[j]) { j += 1; }
            let w = &s[i..j]; let next = b.get(j).copied();
            // a string's prefix (`f"…"`, `r'…'`) is left to its string
            if w.len() <= 2 && matches!(next, Some(b'"') | Some(b'\'')) && !l.chars && w.bytes().all(|x| b"rRbBfFuU".contains(&x)) { i = j; continue }
            // Rust's raw strings
            if l.multi && l.chars && (w == "r" || w == "br") && matches!(next, Some(b'"') | Some(b'#')) {
                let mut k = j; while k < n && b[k] == b'#' { k += 1; }
                if k < n && b[k] == b'"' { let mut close = vec![b'"']; close.extend(std::iter::repeat(b'#').take(k - j)); let z = find_from(b, k + 1, &close).map(|z| z + close.len()).unwrap_or(n); put(i, z, Tok::Str); i = z; continue }
            }
            let call = l.funcs && (next == Some(b'(') || (next == Some(b'!') && l.multi && l.chars && matches!(b.get(j + 1), Some(b'(') | Some(b'[') | Some(b'{'))));
            let t = if word_in(l.keywords, w, l.ci) { Some(Tok::Keyword) } else if word_in(l.types, w, l.ci) { Some(Tok::Type) }
                else if l.css && next == Some(b':') && matches!(b.get(j + 1), None | Some(b' ') | Some(b'\n') | Some(b'\t')) { Some(Tok::Type) }
                else if l.caps && c.is_ascii_uppercase() && w.bytes().any(|x| x.is_ascii_lowercase()) { Some(Tok::Type) }
                else if call { Some(Tok::Func) } else { None };
            if let Some(t) = t { put(i, j, t); }
            i = j; continue
        }
        i += 1;
    }
}

/// XML and HTML: comments, tag names (keywords), attribute names (types), attribute values (strings), entities.
fn markup(s: &str, out: &mut Vec<(Range<usize>, Tok)>) {
    let b = s.as_bytes(); let n = b.len(); let mut i = 0;
    let name = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b':' | b'-' | b'_' | b'.' | b'@') || c >= 0x80;
    while i < n {
        let at = &b[i..];
        if at.starts_with(b"<!--") { let z = find_from(b, i + 4, b"-->").map(|j| j + 3).unwrap_or(n); out.push((i..z, Tok::Comment)); i = z; continue }
        if at.starts_with(b"<![CDATA[") { i = find_from(b, i, b"]]>").map(|j| j + 3).unwrap_or(n); continue }
        if at.starts_with(b"<!") || at.starts_with(b"<?") { let z = find_from(b, i, b">").map(|j| j + 1).unwrap_or(n); out.push((i..z, Tok::Comment)); i = z; continue }
        if b[i] == b'<' && i + 1 < n && (b[i + 1] == b'/' || b[i + 1].is_ascii_alphabetic()) {
            let mut j = i + 1; if b[j] == b'/' { j += 1; } let a = j; while j < n && name(b[j]) { j += 1; }
            if j > a { out.push((a..j, Tok::Keyword)); }
            while j < n && b[j] != b'>' {
                if b[j] == b'"' || b[j] == b'\'' { let z = (j + 1..n).find(|k| b[*k] == b[j]).map(|k| k + 1).unwrap_or(n); out.push((j..z, Tok::Str)); j = z; }
                else if name(b[j]) { let a = j; while j < n && name(b[j]) { j += 1; } out.push((a..j, Tok::Type)); }
                else { j += 1; }
            }
            i = j; continue
        }
        if b[i] == b'&' { if let Some(z) = (i + 1..n.min(i + 12)).find(|j| b[*j] == b';') { if b[i + 1..z].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'#') && z > i + 1 { out.push((i..z + 1, Tok::Number)); i = z + 1; continue } } }
        i += 1;
    }
}

/// YAML, TOML, INI: a line's key is a type, a `[section]` a keyword, and what follows is read as code is (comments,
/// strings, numbers, the words `true` / `false`…).
fn keyed(l: &Lang, s: &str, sep: u8, sections: bool, out: &mut Vec<(Range<usize>, Tok)>) {
    let mut at = 0;
    for line in s.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']); let b = body.as_bytes(); let mut k = body.len() - body.trim_start().len();
        if sections && b.get(k) == Some(&b'[') { if let Some(z) = body.rfind(']') { out.push((at + k..at + z + 1, Tok::Keyword)); code(l, &body[z + 1..], at + z + 1, out); at += line.len(); continue } }
        if sep == b':' { while body[k..].starts_with("- ") { k += 2; while b.get(k) == Some(&b' ') { k += 1; } } }
        // the key: from here to the separator, when nothing between them says otherwise
        let mut j = k; let mut quote = 0u8; let mut end = None;
        while j < b.len() {
            let c = b[j];
            if quote != 0 { if c == quote { quote = 0; } } else if (c == b'"' || c == b'\'') && j == k { quote = c; }
            else if c == sep { if sep != b':' || matches!(b.get(j + 1), None | Some(b' ') | Some(b'\t')) { end = Some(j); } if end.is_some() || sep != b':' { break } }
            else if matches!(c, b'#' | b'{' | b'[' | b'"' | b'\'' | b',') || (c == b';' && sep == b'=') { break }
            j += 1;
        }
        match end.filter(|e| !body[k..*e].trim().is_empty()) {
            Some(e) => { out.push((at + k..at + k + body[k..e].trim_end().len(), Tok::Type)); code(l, &body[e + 1..], at + e + 1, out); }
            None => code(l, &body[k..], at + k, out),
        }
        at += line.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seen(lang: &str, text: &str) -> Vec<(String, Tok)> { lex(lang, text).into_iter().map(|(r, t)| (text[r].to_string(), t)).collect() }
    fn has(got: &[(String, Tok)], want: &[(&str, Tok)]) { for (w, t) in want { assert!(got.iter().any(|(g, k)| g == w && k == t), "missing {w:?} as {t:?} in {got:?}"); } }
    use Tok::*;

    #[test]
    fn a_language_is_found_by_its_extension_or_its_name_and_an_unknown_one_stays_plain() {
        assert!(known("rs") && known("Rust") && known(" python ") && known("c++") && known("bash") && known("yml"));
        assert!(!known("") && !known("txt") && !known("klingon"));
        assert!(lex("", "let a = 1;").is_empty() && lex("klingon", "let a = 1;").is_empty());
    }
    #[test]
    fn every_language_has_a_shown_name_and_every_shown_name_a_language() {
        for l in LANGS { assert!(title(l.names[0]).is_some(), "{}", l.names[0]); }
        for (short_name, shown) in NAMES { assert!(known(short_name) && title(short_name) == Some(shown)); }
        assert_eq!((title("Python"), title("svg"), title("htm"), title("yml"), title("nope")), (Some("Python"), Some("XML"), Some("XML"), Some("YAML"), None));
        assert_eq!((short("python"), short("html"), short("bash"), short("nope")), (Some("py"), Some("html"), Some("sh"), None));
    }
    #[test]
    fn rust_is_cut_into_its_six_kinds() {
        let t = "// say it\nfn main() { let s: &'a str = \"hi \\\" there\"; let n = 0x1F + 2.5e-3; let c = 'x'; println!(\"{}\", Vec::<u8>::new().len()); }\n/* block\n */ let r = r#\"raw \"x\" \"#;";
        let g = seen("rust", t);
        has(&g, &[("// say it", Comment), ("fn", Keyword), ("main", Func), ("let", Keyword), ("str", Type), ("\"hi \\\" there\"", Str), ("0x1F", Number), ("2.5e-3", Number), ("'x'", Str), ("println", Func), ("Vec", Type), ("u8", Type), ("new", Func), ("len", Func), ("/* block\n */", Comment), ("r#\"raw \"x\" \"#", Str)]);
        assert!(!g.iter().any(|(w, _)| w.starts_with("'a")), "a lifetime is not a string: {g:?}");
    }
    #[test]
    fn the_stretches_are_in_order_apart_and_on_character_boundaries() {
        for (lang, t) in [("py", "# caf\u{e9} \u{2603}\ndef na\u{ef}ve(x): return f\"\u{e9}{x}\" + '\\\u{e9}' # \u{1F600}\n'''doc\n\u{e9}'''"), ("js", "const \u{e9} = `a\n${b}\u{e9}`; // \u{e9}\n'\\"), ("xml", "<a b=\"\u{e9}\">\u{e9} &amp; <!-- \u{e9} --></a><"), ("yaml", "cl\u{e9}: \"\u{e9}\" # \u{e9}\n- a: 1\n  b: it's\n"), ("sh", "echo \"$HOME\" ${x#y} $# # \u{e9}\n"), ("c", "'"), ("rs", "r#\"")] {
            let g = lex(lang, t); let mut at = 0;
            for (r, _) in &g { assert!(r.start >= at && r.end > r.start && r.end <= t.len() && t.is_char_boundary(r.start) && t.is_char_boundary(r.end), "{lang}: {r:?} in {g:?}"); at = r.end; }
            assert_eq!(pieces(lang, t).iter().map(|p| p.0).collect::<String>(), t);
        }
    }
    #[test]
    fn each_family_of_languages_reads_its_own_spelling() {
        has(&seen("python", "@cache\ndef f(a):\n    \"\"\"doc\n    more\"\"\"\n    return None  # why\nx = Foo(3)"), &[("@cache", Func), ("def", Keyword), ("f", Func), ("\"\"\"doc\n    more\"\"\"", Str), ("None", Keyword), ("# why", Comment), ("Foo", Type), ("3", Number)]);
        has(&seen("sh", "#!/bin/sh\nfor f in *.mov; do ffmpeg -i \"$f\" -crf 18 ${f%.mov}.mp4; done # all"), &[("#!/bin/sh", Comment), ("for", Keyword), ("in", Keyword), ("do", Keyword), ("\"$f\"", Str), ("18", Number), ("${f%.mov}", Type), ("done", Keyword), ("# all", Comment)]);
        has(&seen("sql", "SELECT name, COUNT(*) FROM shots -- all\nWHERE note = 'it''s' AND n > 3"), &[("SELECT", Keyword), ("COUNT", Func), ("FROM", Keyword), ("-- all", Comment), ("WHERE", Keyword), ("'it'", Str), ("3", Number)]);
        has(&seen("json", "{ \"name\": \"clip\", \"frames\": [1, 2.5], \"ok\": true } // c"), &[("\"name\"", Type), ("\"clip\"", Str), ("\"frames\"", Type), ("2.5", Number), ("true", Keyword), ("// c", Comment)]);
        has(&seen("yaml", "# top\nname: clip one\nlist:\n  - id: 3\n    on: true\n    url: http://a.b/c\n    s: \"q\""), &[("# top", Comment), ("name", Type), ("list", Type), ("id", Type), ("3", Number), ("true", Keyword), ("url", Type), ("\"q\"", Str)]);
        has(&seen("toml", "[package]\nname = \"mn\" # it\nedition = 2021\n; no"), &[("[package]", Keyword), ("name", Type), ("\"mn\"", Str), ("# it", Comment), ("2021", Number)]);
        has(&seen("ini", "[core]\n; note\nbare = true"), &[("[core]", Keyword), ("; note", Comment), ("bare", Type), ("true", Keyword)]);
        has(&seen("html", "<!-- c --><a href=\"x\" class='y'>a &amp; b</a>"), &[("<!-- c -->", Comment), ("a", Keyword), ("href", Type), ("\"x\"", Str), ("'y'", Str), ("&amp;", Number)]);
        has(&seen("css", "/* c */ @media print { a:hover { margin-top: 12px; color: #fff; background: url(http://a/b) } }"), &[("/* c */", Comment), ("@media", Keyword), ("margin-top", Type), ("12px", Number), ("color", Type), ("#fff", Number), ("url", Func)]);
        has(&seen("c", "#include <stdio.h>\nint main(void) { char c = '\\n'; return 0; } // end"), &[("#include", Keyword), ("int", Type), ("main", Func), ("void", Type), ("'\\n'", Str), ("return", Keyword), ("0", Number), ("// end", Comment)]);
        has(&seen("ts", "const a: string = `x ${y}`; class Foo extends Bar { $el = 1 }"), &[("const", Keyword), ("string", Type), ("`x ${y}`", Str), ("class", Keyword), ("Foo", Type), ("Bar", Type)]);
        has(&seen("lua", "--[[ long\n]] local x = 1 -- end"), &[("--[[ long\n]]", Comment), ("local", Keyword), ("-- end", Comment)]);
        has(&seen("go", "func Do(x int) string { return fmt.Sprintf(`a`, 'b') }"), &[("func", Keyword), ("Do", Func), ("int", Type), ("string", Type), ("Sprintf", Func), ("`a`", Str), ("'b'", Str)]);
        has(&seen("ps1", "<# c #> $x = Get-Item -Path 'a' # d"), &[("<# c #>", Comment), ("$x", Type), ("'a'", Str), ("# d", Comment)]);
    }
    #[test]
    fn a_very_long_block_is_left_plain() {
        let t = "let a = 1;\n".repeat(LARGEST_LINES); assert!(lex("rs", &t).is_empty());
        let t = "let a = 1;\n".repeat(LARGEST_LINES - 1); assert!(!lex("rs", &t).is_empty());
    }
}
