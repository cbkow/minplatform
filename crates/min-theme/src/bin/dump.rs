//! `min-theme-dump [--json] [prefix]` — every token (or those starting with `prefix`) as resolved by the build:
//! the parity oracle for the future `Theme.qml` generator and a quick way to read a value during a design pass.
use min_theme::{tokens, Token};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let prefix = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let rows: Vec<_> = tokens::ALL.iter().filter(|(n, _)| n.starts_with(&prefix)).collect();
    if json { print!("{{"); }
    for (i, (name, tok)) in rows.iter().enumerate() {
        let v = match tok {
            Token::Color(c) => if json { format!("{:?}", c.to_hex()) } else { format!("{}  rgba({:.0}, {:.0}, {:.0}, {:.2})", c.to_hex(), c.r * 255.0, c.g * 255.0, c.b * 255.0, c.a) },
            Token::Px(v) => if json { format!("\"{v}px\"") } else { format!("{v} px") },
            Token::Num(v) => format!("{v}"),
            Token::Ms(v) => if json { format!("\"{v}ms\"") } else { format!("{v} ms") },
            Token::Em(v) => if json { format!("\"{v}em\"") } else { format!("{v} em") },
            Token::Str(s) => format!("{s:?}"),
            Token::Shadow(layers) => {
                let l: Vec<String> = layers.iter().map(|s| if json { format!("{{\"x\":{},\"y\":{},\"blur\":{},\"spread\":{},\"color\":{:?}}}", s.x, s.y, s.blur, s.spread, s.color.to_hex()) } else { format!("{} {} {} {} {}", s.x, s.y, s.blur, s.spread, s.color.to_hex()) }).collect();
                if json { format!("[{}]", l.join(",")) } else { l.join(" / ") }
            }
        };
        if json { print!("{}{name:?}:{v}", if i > 0 { "," } else { "" }); } else { println!("{name:<48} {v}"); }
    }
    if json { println!("}}"); } else { eprintln!("{} tokens", rows.len()); }
}
