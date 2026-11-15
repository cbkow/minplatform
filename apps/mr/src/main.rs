//! minRender (the port). One executable:
//!   minRender --data <folder> [--minimized]      the application: a window that hides, an icon in the tray, the node
//!   minRender --data <folder> --no-window        the node alone (no window, no tray: a farm's test node, a server)
//!   minRender --leash -- <program> <args...>     the leash of one render (started by the node, never by hand)
//! `--wait-pid <n>` is how the application starts itself again: the copy waits for process n to be gone.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod cleanup;
mod form;
mod layers;
mod http;
mod instance;
mod lead;
mod leash;
mod node;
mod settings;
mod share;
mod shell;
mod submit;
mod ui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

static QUIT: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
extern "C" fn on_signal(_: i32) { QUIT.store(true, Ordering::Relaxed); }
fn alive(pid: u32) -> bool {
    #[cfg(unix)] { unsafe { libc::kill(pid as i32, 0) == 0 } }
    #[cfg(windows)] { use windows::Win32::{Foundation::*, System::Threading::*}; unsafe { let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return false }; let r = WaitForSingleObject(h, 0) != WAIT_OBJECT_0; let _ = CloseHandle(h); r } }
}
/// This program starts itself again (S14): a copy with the same arguments that waits for this process to be gone.
/// No helper executable, and - inside a Store package - no leaving of the package's job.
pub fn restart(minimized: bool) {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--wait-pid") { args.drain(i..(i + 2).min(args.len())); }
    args.extend(["--wait-pid".to_string(), std::process::id().to_string()]); if minimized && !args.iter().any(|a| a == "--minimized") { args.push("--minimized".into()); }
    let mut c = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "minRender".into())); c.args(&args).env_remove("MR_DEMO").env_remove("MR_SHOT").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0000_0008 | 0x0800_0000); }
    #[cfg(unix)] { use std::os::unix::process::CommandExt; c.process_group(0); }
    match c.spawn() { Ok(ch) => share::log("INFO", "farm", &format!("the application starts itself again (the copy is process {})", ch.id())), Err(e) => share::log("ERROR", "farm", &format!("the application could not start itself again: {e}")) }
}

/// The data folder of the application as installed, or none for a build that is not installed.
/// Windows: with the Store package's identity `cbkow.minRender` it is `%USERPROFILE%\minRender` (0.5.x's, outside
/// AppData on purpose); with any OTHER package identity - a test package beside the real one - `%USERPROFILE%\minRender-test`.
/// macOS: run from inside `minRender.app` it is `~/Library/Application Support/MinRender`; from an application bundle
/// of another name, `…/MinRender-test`.
fn installed_data() -> Option<std::path::PathBuf> {
    #[cfg(windows)] { use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName; use windows::core::PWSTR;
        let mut n = 0u32; unsafe { let _ = GetCurrentPackageFullName(&mut n, None); } if n == 0 { return None }
        let mut buf = vec![0u16; n as usize]; if unsafe { GetCurrentPackageFullName(&mut n, Some(PWSTR(buf.as_mut_ptr()))) }.0 != 0 { return None }
        let full = String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())]); let home = std::path::PathBuf::from(std::env::var_os("USERPROFILE")?);
        Some(home.join(if full.split('_').next() == Some("cbkow.minRender") { "minRender" } else { "minRender-test" })) }
    #[cfg(target_os = "macos")] { let exe = std::env::current_exe().ok()?; let app = exe.ancestors().find(|p| p.extension().is_some_and(|x| x == "app"))?; if !exe.starts_with(app.join("Contents/MacOS")) { return None }
        let home = std::path::PathBuf::from(std::env::var_os("HOME")?); Some(home.join("Library/Application Support").join(if app.file_stem().is_some_and(|n| n == "minRender") { "MinRender" } else { "MinRender-test" })) }
    #[cfg(not(any(windows, target_os = "macos")))] { None }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect(); let val = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned()); let has = |k: &str| args.iter().any(|a| a == k);
    if args.first().is_some_and(|a| a == "--leash") { let from = args.iter().position(|a| a == "--").map(|i| i + 1).unwrap_or(args.len()); leash::run(&args[from..]) }
    // The data folder (row 425). `--data` names it. Without it, the application AS INSTALLED takes 0.5.x's own - so an
    // upgraded node is the same node - and a bare run of a build that is not installed takes none: it cannot touch a
    // real node's folder by a slip.
    let Some(data) = val("--data").map(std::path::PathBuf::from).or_else(installed_data) else { eprintln!("minRender --data <folder> [--minimized | --no-window]   (the installed application needs no --data)"); std::process::exit(2) };
    if let Some(pid) = val("--wait-pid").and_then(|p| p.parse::<u32>().ok()) { let t = std::time::Instant::now(); while alive(pid) && t.elapsed() < std::time::Duration::from_secs(20) { std::thread::sleep(std::time::Duration::from_millis(50)); } }
    let windowed = !has("--no-window"); let minimized = has("--minimized") || windowed && shell::started_at_sign_in();
    // one instance for a data folder: a later launch tells the first how it was started, and leaves
    if windowed { if instance::hand_over(&data, minimized) { return } instance::listen(&data, |minimized| shell::send(shell::Msg::Second(minimized))); }
    let n = match node::Node::open(&data) { Ok(n) => n, Err(e) => { eprintln!("minRender: {e}"); std::process::exit(1) } };
    #[cfg(unix)] unsafe { libc::signal(libc::SIGTERM, on_signal as *const () as usize); libc::signal(libc::SIGINT, on_signal as *const () as usize); }
    { let stop = n.stop.clone(); std::thread::spawn(move || loop { if QUIT.load(Ordering::Relaxed) { stop.store(true, Ordering::Relaxed); break } std::thread::sleep(std::time::Duration::from_millis(50)); }); }
    // the node, on its own threads; `ended` says its shutdown is done (the render killed, the leader told, the phonebook entry gone)
    let ended = Arc::new(AtomicBool::new(false));
    let worker = { let (n, ended) = (n.clone(), ended.clone()); std::thread::spawn(move || { if let Err(e) = n.run() { eprintln!("minRender: {e}"); } ended.store(true, Ordering::Relaxed); }) };
    if windowed { shell::run(n.clone(), ended.clone(), minimized); n.stop.store(true, Ordering::Relaxed); }
    else {
        // `quit` on the standard input ends the node too (the one way on a system without signals)
        { let stop = n.stop.clone(); std::thread::spawn(move || { let mut l = String::new(); while std::io::stdin().read_line(&mut l).is_ok_and(|n| n > 0) { if l.trim() == "quit" { stop.store(true, Ordering::Relaxed); break } l.clear(); } }); }
        while !ended.load(Ordering::Relaxed) { if n.restart_asked.swap(false, Ordering::Relaxed) { restart(false); n.stop.store(true, Ordering::Relaxed); } std::thread::sleep(std::time::Duration::from_millis(100)); }
    }
    let _ = worker.join();
}
