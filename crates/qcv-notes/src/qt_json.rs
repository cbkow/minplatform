//! Qt's `QJsonDocument::toJson(Indented)`, byte for byte: keys sorted (QJsonObject keeps them sorted), four spaces per level,
//! `"key": value`, empty containers inline, a trailing newline; numbers in Qt's shortest form (`QByteArray::number(d, 'g',
//! QLocale::FloatingPointShortest)`): the shortest round-trip digits, decimal unless the exponent form is shorter (QLocale's
//! `useDecimal`: `decpt <= 0 ? 1 - decpt <= bias : decpt <= digits + bias`, bias = 2 + the two exponent digits), the exponent
//! signed and at least two digits (`e-05`, `e+21`). Integers (QJsonValue's integer type) print as integers.
use serde_json::Value;

/// Qt's shortest `'g'` form of a double.
pub fn qt_double(v: f64) -> String {
    if !v.is_finite() { return "null".into() }
    if v == 0.0 { return if v.is_sign_negative() { "-0".into() } else { "0".into() } }
    let s = format!("{v:e}");   // the shortest round-trip digits: "-8.935546875e-5"
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let mut digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    // a TIE between two shortest candidates (the exact value sits half-way — e.g. 0.0700569152832031250): Qt's double-conversion
    // (and Python's repr) round half to EVEN; Rust's formatter rounds up. Decided on the exact expansion.
    {
        let n = digits.len();
        let exact = format!("{:.799e}", v.abs());
        let (em, ee) = exact.split_once('e').unwrap();
        let ed: Vec<u8> = em.bytes().filter(|c| c.is_ascii_digit()).collect();
        if ee.parse::<i32>().ok() == Some(exp) && ed.len() > n && ed[n] == b'5' && ed[n + 1..].iter().all(|&c| c == b'0') {
            let mut t: Vec<u8> = ed[..n].to_vec();   // the truncated candidate; round up only to reach an even last digit
            if (t[n - 1] - b'0') % 2 == 1 { let mut i = n; loop { i -= 1; if t[i] == b'9' { t[i] = b'0'; if i == 0 { break } } else { t[i] += 1; break } } }
            let cand = String::from_utf8(t).unwrap();
            if cand.len() == n && cand.parse::<u128>().is_ok() { digits = cand; }
        }
    }
    let n = digits.len() as i32; let decpt = exp + 1; let bias = 4;
    let use_decimal = if decpt <= 0 { 1 - decpt <= bias } else { decpt <= n + bias };
    let body = if use_decimal {
        if decpt <= 0 { format!("0.{}{}", "0".repeat((-decpt) as usize), digits) }
        else if decpt >= n { format!("{}{}", digits, "0".repeat((decpt - n) as usize)) }
        else { format!("{}.{}", &digits[..decpt as usize], &digits[decpt as usize..]) }
    } else {
        let m = if n > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits.clone() };
        let x = decpt - 1;
        format!("{m}e{}{:02}", if x < 0 { '-' } else { '+' }, x.abs())
    };
    if neg { format!("-{body}") } else { body }
}
fn number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() { return i.to_string() }
    if let Some(u) = n.as_u64() { return u.to_string() }
    qt_double(n.as_f64().unwrap_or(0.0))
}
/// Qt's string escaping: `"` `\` and the controls (`\b \f \n \r \t`, others `\u00XX`); everything else raw UTF-8.
fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() { match c { '"' => out.push_str("\\\""), '\\' => out.push_str("\\\\"), '\u{8}' => out.push_str("\\b"), '\u{c}' => out.push_str("\\f"), '\n' => out.push_str("\\n"), '\r' => out.push_str("\\r"), '\t' => out.push_str("\\t"), c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)), c => out.push(c) } }
    out.push('"');
}
fn write(v: &Value, depth: usize, out: &mut String) {
    let pad = |n: usize| "    ".repeat(n);
    match v {
        Value::Object(m) => {
            if m.is_empty() { out.push_str("{\n"); out.push_str(&pad(depth)); out.push('}'); return }
            let mut keys: Vec<&String> = m.keys().collect(); keys.sort();
            out.push_str("{\n");
            for (i, k) in keys.iter().enumerate() { out.push_str(&pad(depth + 1)); string(k, out); out.push_str(": "); write(&m[*k], depth + 1, out); out.push_str(if i + 1 < keys.len() { ",\n" } else { "\n" }); }
            out.push_str(&pad(depth)); out.push('}');
        }
        Value::Array(a) => {
            if a.is_empty() { out.push_str("[\n"); out.push_str(&pad(depth)); out.push(']'); return }
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() { out.push_str(&pad(depth + 1)); write(x, depth + 1, out); out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" }); }
            out.push_str(&pad(depth)); out.push(']');
        }
        Value::String(s) => string(s, out),
        Value::Number(n) => out.push_str(&number(n)),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
    }
}
/// The document as Qt writes it (Indented ends with a newline).
pub fn to_qt_json(v: &Value) -> String { let mut s = String::new(); write(v, 0, &mut s); s.push('\n'); s }

#[cfg(test)]
mod tests {
    use super::qt_double as q;
    #[test]
    fn numbers_are_qts() {
        assert_eq!(q(8.935546875e-05), "8.935546875e-05");
        assert_eq!(q(528.1109166666666), "528.1109166666666");
        assert_eq!(q(0.25882354378700256), "0.25882354378700256");
        assert_eq!(q(-0.07005691528320312), "-0.07005691528320312", "a tie rounds to even (Qt / Python), not up (Rust)");
        assert_eq!(q(6.0), "6"); assert_eq!(q(2.5), "2.5"); assert_eq!(q(1e21), "1e+21"); assert_eq!(q(-0.001), "-0.001");
    }
}
