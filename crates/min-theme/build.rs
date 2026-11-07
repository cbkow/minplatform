//! Resolves `tokens/*.json` (Spectrum 2's token shape: `{"name": {"value": X}}`, `X` may reference another token as
//! `"{name}"`; our ONE extension is `"alpha": f` on a colour alias) into `$OUT_DIR/theme.rs` — one `pub const` per
//! token, typed by the value's shape — and `icons/phosphor.json` into `$OUT_DIR/icons.rs`. A dangling or cyclic
//! reference fails the build by name: the theme is data, but it is checked data.
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

#[derive(Clone, Debug)]
enum Tok { Color([f32; 4]), Px(f32), Num(f64), Ms(u32), Em(f32), Str(String), Shadow(Vec<([f32; 4], [f32; 4])>) }

fn parse_color(s: &str) -> Option<[f32; 4]> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        let v = u32::from_str_radix(h, 16).ok()?;
        return match h.len() {
            6 => Some([((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, 1.0]),
            8 => Some([((v >> 24) & 255) as f32 / 255.0, ((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]),
            _ => None,
        };
    }
    let inner = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb("))?.strip_suffix(')')?;
    let p: Vec<f32> = inner.split(',').map(|x| x.trim().parse::<f32>().ok()).collect::<Option<_>>()?;
    match p.len() { 3 => Some([p[0] / 255.0, p[1] / 255.0, p[2] / 255.0, 1.0]), 4 => Some([p[0] / 255.0, p[1] / 255.0, p[2] / 255.0, p[3]]), _ => None }
}

/// A raw token: either a literal or a reference (+ optional alpha override).
enum Raw { Lit(Tok), Ref(String, Option<f32>), ShadowRef(Vec<([f32; 4], String)>) }

fn raw_of(name: &str, v: &Value) -> Raw {
    let alpha = v.get("alpha").and_then(|a| a.as_f64()).map(|a| a as f32);
    let val = v.get("value").unwrap_or_else(|| panic!("token `{name}` has no value"));
    match val {
        Value::String(s) => {
            if let Some(r) = s.strip_prefix('{').and_then(|x| x.strip_suffix('}')) { return Raw::Ref(r.to_string(), alpha) }
            if let Some(c) = parse_color(s) { return Raw::Lit(Tok::Color(c)) }
            if let Some(px) = s.strip_suffix("px") { return Raw::Lit(Tok::Px(px.trim().parse().unwrap_or_else(|_| panic!("token `{name}`: bad px `{s}`")))) }
            if let Some(ms) = s.strip_suffix("ms") { return Raw::Lit(Tok::Ms(ms.trim().parse().unwrap_or_else(|_| panic!("token `{name}`: bad ms `{s}`")))) }
            if let Some(em) = s.strip_suffix("em") { return Raw::Lit(Tok::Em(em.trim().parse().unwrap_or_else(|_| panic!("token `{name}`: bad em `{s}`")))) }
            Raw::Lit(Tok::Str(s.clone()))
        }
        Value::Number(n) => Raw::Lit(Tok::Num(n.as_f64().unwrap())),
        Value::Array(layers) => {
            let px = |o: &Value, k: &str| -> f32 { o.get(k).and_then(|x| x.as_str()).and_then(|x| x.strip_suffix("px")).and_then(|x| x.trim().parse().ok()).unwrap_or_else(|| panic!("token `{name}`: shadow layer needs `{k}` in px")) };
            Raw::ShadowRef(layers.iter().map(|o| {
                let c = o.get("color").and_then(|x| x.as_str()).unwrap_or_else(|| panic!("token `{name}`: shadow layer needs a colour"));
                ([px(o, "x"), px(o, "y"), px(o, "blur"), px(o, "spread")], c.to_string())
            }).collect())
        }
        other => panic!("token `{name}`: unsupported value {other}"),
    }
}

fn resolve(name: &str, raws: &BTreeMap<String, Raw>, out: &mut BTreeMap<String, Tok>, stack: &mut Vec<String>) -> Tok {
    if let Some(t) = out.get(name) { return t.clone() }
    if stack.iter().any(|s| s == name) { panic!("token reference cycle: {} -> {name}", stack.join(" -> ")) }
    stack.push(name.to_string());
    let raw = raws.get(name).unwrap_or_else(|| panic!("token `{}` references `{name}`, which does not exist", stack.get(stack.len().wrapping_sub(2)).map(String::as_str).unwrap_or("?")));
    let t = match raw {
        Raw::Lit(t) => t.clone(),
        Raw::Ref(target, alpha) => {
            let t = resolve(target, raws, out, stack);
            match (t, alpha) {
                (Tok::Color(mut c), Some(a)) => { c[3] = *a; Tok::Color(c) }
                (_, Some(_)) => panic!("token `{name}`: `alpha` is only valid on a colour reference"),
                (t, None) => t,
            }
        }
        Raw::ShadowRef(layers) => Tok::Shadow(layers.iter().map(|(geo, c)| {
            let col = if let Some(r) = c.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
                match resolve(r, raws, out, stack) { Tok::Color(c) => c, _ => panic!("token `{name}`: shadow colour `{r}` is not a colour") }
            } else { parse_color(c).unwrap_or_else(|| panic!("token `{name}`: bad shadow colour `{c}`")) };
            (*geo, col)
        }).collect()),
    };
    stack.pop();
    out.insert(name.to_string(), t.clone());
    t
}

fn ident(name: &str) -> String { name.replace('-', "_").to_uppercase() }
fn f(x: f32) -> String { let s = format!("{x:?}"); if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") } }
fn color(c: [f32; 4]) -> String { format!("Color {{ r: {}, g: {}, b: {}, a: {} }}", f(c[0]), f(c[1]), f(c[2]), f(c[3])) }

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rerun-if-changed=tokens");
    println!("cargo:rerun-if-changed=icons/phosphor.json");
    println!("cargo:rerun-if-changed=build.rs");

    // ---- tokens ----
    let mut raws: BTreeMap<String, Raw> = BTreeMap::new();
    let mut files: Vec<_> = std::fs::read_dir(root.join("tokens")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    for p in &files {
        println!("cargo:rerun-if-changed={}", p.display());
        let j: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        for (k, v) in j.as_object().unwrap() {
            if k.starts_with('$') { continue }
            if raws.insert(k.clone(), raw_of(k, v)).is_some() { panic!("token `{k}` defined twice") }
        }
    }
    let mut toks: BTreeMap<String, Tok> = BTreeMap::new();
    let names: Vec<String> = raws.keys().cloned().collect();
    for n in &names { resolve(n, &raws, &mut toks, &mut Vec::new()); }

    let mut s = String::new();
    writeln!(s, "// generated by min-theme's build.rs from tokens/*.json — do not edit").unwrap();
    let mut all = String::new();
    for (n, t) in &toks {
        let id = ident(n);
        let (ty, val, tokv) = match t {
            Tok::Color(c) => ("Color", color(*c), format!("Token::Color({id})")),
            Tok::Px(v) => ("f32", f(*v), format!("Token::Px({id})")),
            Tok::Num(v) => ("f64", { let x = format!("{v:?}"); if x.contains('.') || x.contains('e') { x } else { format!("{x}.0") } }, format!("Token::Num({id})")),
            Tok::Ms(v) => ("u32", v.to_string(), format!("Token::Ms({id})")),
            Tok::Em(v) => ("f32", f(*v), format!("Token::Em({id})")),
            Tok::Str(v) => ("&str", format!("{v:?}"), format!("Token::Str({id})")),
            Tok::Shadow(layers) => {
                let body = layers.iter().map(|(g, c)| format!("Shadow {{ x: {}, y: {}, blur: {}, spread: {}, color: {} }}", f(g[0]), f(g[1]), f(g[2]), f(g[3]), color(*c))).collect::<Vec<_>>().join(", ");
                ("&[Shadow]", format!("&[{body}]"), format!("Token::Shadow({id})"))
            }
        };
        writeln!(s, "/// `{n}`").unwrap();
        writeln!(s, "pub const {id}: {ty} = {val};").unwrap();
        writeln!(all, "    ({n:?}, {tokv}),").unwrap();
    }
    writeln!(s, "/// Every token by its data name, sorted — the dump tool, the tests and the future `Theme.qml` generator read this.").unwrap();
    writeln!(s, "pub static ALL: &[(&str, Token)] = &[\n{all}];").unwrap();
    std::fs::write(Path::new(&out_dir).join("theme.rs"), s).unwrap();

    // ---- icons ----
    let j: Value = serde_json::from_str(&std::fs::read_to_string(root.join("icons/phosphor.json")).unwrap()).unwrap();
    let mut icons: Vec<(String, u32)> = j.as_object().unwrap().iter().map(|(k, v)| {
        let cp = v.as_str().and_then(|s| s.strip_prefix("U+")).and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or_else(|| panic!("icon `{k}`: bad codepoint {v}"));
        (k.clone(), cp)
    }).collect();
    icons.sort();
    let mut s = String::new();
    writeln!(s, "// generated by min-theme's build.rs from icons/phosphor.json — do not edit").unwrap();
    writeln!(s, "/// Phosphor slug → private-use codepoint in `Phosphor.ttf` (QCView's `PhosphorIcons.js`, sorted by slug).").unwrap();
    writeln!(s, "pub static PHOSPHOR: &[(&str, char)] = &[").unwrap();
    for (k, cp) in &icons { writeln!(s, "    ({k:?}, '\\u{{{cp:X}}}'),").unwrap(); }
    writeln!(s, "];").unwrap();
    std::fs::write(Path::new(&out_dir).join("icons.rs"), s).unwrap();
}
