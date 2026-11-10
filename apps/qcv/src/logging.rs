//! The file logger — QCView's `installFileLogger` (app/main.cpp): every log line also goes to `qcv-log.txt` in the per-user log
//! folder (macOS `~/Library/Logs/QCView`, Windows `%LOCALAPPDATA%\\QCView\\logs`, else `~/.local/state/QCView`; `QCV_LOG_DIR`
//! overrides), the previous session's log rotated to `qcv-log.prev.txt` first so a crash survives one relaunch; QCView's line
//! shape `[hh:mm:ss.zzz t=xxxx] I  message`. The folder is the product's (canon row 253; the file names are not 2.x's). OURS: the
//! default filter is `info` with the GPU stack at `warn` (RUST_LOG still wins). Only the window logs to a file — the harness
//! modes keep plain stderr.
use std::io::Write;
use std::sync::{Arc, Mutex};

pub fn log_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("QCV_LOG_DIR") { return d.into() }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") { home.join("Library/Logs/QCView") }
    else if cfg!(windows) { std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("QCView").join("logs")).unwrap_or_else(|| std::path::PathBuf::from(".")) }
    else { home.join(".local/state/QCView") }
}

struct Tee(Arc<Mutex<std::fs::File>>);
impl Write for Tee {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { let _ = std::io::stderr().write_all(b); if let Ok(mut f) = self.0.lock() { let _ = f.write_all(b); } Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { if let Ok(mut f) = self.0.lock() { let _ = f.flush(); } std::io::stderr().flush() }
}

/// Install the tee (stderr + the file); falls back to plain stderr when the folder cannot be written.
pub fn install() -> Option<std::path::PathBuf> {
    let dir = log_dir();
    let (path, prev) = (dir.join("qcv-log.txt"), dir.join("qcv-log.prev.txt"));
    let file = std::fs::create_dir_all(&dir).ok().and_then(|_| { if path.exists() { let _ = std::fs::remove_file(&prev); let _ = std::fs::rename(&path, &prev); } std::fs::File::create(&path).ok() });
    let mut b = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn"));
    b.format(|buf, rec| {
        let now = jiff::Zoned::now();
        let tid = format!("{:?}", std::thread::current().id()).chars().filter(|c| c.is_ascii_digit()).collect::<String>();
        let sev = match rec.level() { log::Level::Error => "E", log::Level::Warn => "W", log::Level::Info => "I", _ => "D" };
        writeln!(buf, "[{}.{:03} t={tid:>4}] {sev}  {}", now.strftime("%H:%M:%S"), now.subsec_nanosecond() / 1_000_000, rec.args())
    });
    let ok = file.is_some();
    if let Some(f) = file { b.target(env_logger::Target::Pipe(Box::new(Tee(Arc::new(Mutex::new(f)))))); }
    b.init();
    if ok { log::info!("File logger installed → {}", path.display()); Some(path) } else { log::warn!("File logger: cannot write {}", path.display()); None }
}
