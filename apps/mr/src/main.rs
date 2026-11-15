//! minRender (the port). One executable:
//!   minRender --data <folder> [--no-window]     a node (no window yet)
//!   minRender --leash -- <program> <args...>    the leash of one render (started by the node, never by hand)
mod http;
mod lead;
mod leash;
mod node;
mod submit;
use std::sync::atomic::{AtomicBool, Ordering};

static QUIT: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
extern "C" fn on_signal(_: i32) { QUIT.store(true, Ordering::Relaxed); }

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--leash") { let from = args.iter().position(|a| a == "--").map(|i| i + 1).unwrap_or(args.len()); leash::run(&args[from..]) }
    // (until the port is ruled ready to take the real data folder's place, it runs only where it is told to)
    let Some(data) = args.iter().position(|a| a == "--data").and_then(|i| args.get(i + 1)) else { eprintln!("minRender --data <folder> [--no-window]"); std::process::exit(2) };
    let n = match node::Node::open(std::path::Path::new(data)) { Ok(n) => n, Err(e) => { eprintln!("minRender: {e}"); std::process::exit(1) } };
    #[cfg(unix)] unsafe { libc::signal(libc::SIGTERM, on_signal as *const () as usize); libc::signal(libc::SIGINT, on_signal as *const () as usize); }
    { let stop = n.stop.clone(); std::thread::spawn(move || loop { if QUIT.load(Ordering::Relaxed) { stop.store(true, Ordering::Relaxed); break } std::thread::sleep(std::time::Duration::from_millis(50)); }); }
    // `quit` on the standard input ends the node too (the one way on a system without signals)
    { let stop = n.stop.clone(); std::thread::spawn(move || { let mut l = String::new(); while std::io::stdin().read_line(&mut l).is_ok_and(|n| n > 0) { if l.trim() == "quit" { stop.store(true, Ordering::Relaxed); break } l.clear(); } }); }
    if let Err(e) = n.run() { eprintln!("minRender: {e}"); std::process::exit(1) }
}
