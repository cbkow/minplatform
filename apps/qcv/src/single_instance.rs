//! Single instance — QCView's `main.cpp` handoff (canon row 143): a per-user local socket ; a second launch forwards its positional arguments (file paths, `qcview://` links) to the
//! running window and exits (a Unix socket file in the per-user temp folder, a named pipe on Windows). OURS: the name is `qcv-singleinstance-<user>`, not QCView's `qcview-…`, so the port never hands
//! files to a running Qt QCView (both live on the user's machines until the switch). The harness flags (headless, the
//! benches) and `QCV_MULTI_INSTANCE=1` bypass it, as QCView's dev-mode flags do. One message = the arguments, one per line.
#[cfg(unix)] use interprocess::local_socket::GenericFilePath;
#[cfg(not(unix))] use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{prelude::*, ListenerOptions, Name, Stream};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc;

fn user() -> String { std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "default".into()) }
/// Unix: an explicit socket FILE in the per-user temp folder (interprocess's "namespaced" form is a file in the shared /tmp on
/// macOS — and a killed instance leaves it behind); Windows: a named pipe (the OS removes it with its last handle).
#[cfg(unix)] fn socket_file() -> std::path::PathBuf { std::env::temp_dir().join(format!("qcv-singleinstance-{}.sock", user())) }
fn name() -> std::io::Result<Name<'static>> {
    #[cfg(unix)] { socket_file().to_fs_name::<GenericFilePath>().map(|x| x.into_owned()) }
    #[cfg(not(unix))] { format!("qcv-singleinstance-{}", user()).to_ns_name::<GenericNamespaced>().map(|x| x.into_owned()) }
}

/// The secondary side: true when a running window took the arguments (the caller exits).
pub fn forward(args: &[String]) -> bool {
    let Ok(n) = name() else { return false };
    let Ok(mut s) = Stream::connect(n) else { return false };
    let msg = args.join("\n") + "\n";
    s.write_all(msg.as_bytes()).and_then(|_| s.flush()).is_ok()
}

/// The primary side: listen; each connection's lines arrive on the receiver (the shell drains it every tick).
pub fn listen() -> Option<mpsc::Receiver<Vec<String>>> {
    let n = name().ok()?;
    let listener = match ListenerOptions::new().name(n.clone()).create_sync() {
        Ok(l) => l,
        Err(e) => {
            // someone answers → a live primary (the caller already tried to forward, so a race: stay single-window-less);
            // nobody answers → a socket file a killed instance left (QCView's `removeServer` before `listen`)
            if Stream::connect(n.clone()).is_ok() { log::warn!("single-instance: another qcv is listening ({e}); this window runs on its own"); return None }
            #[cfg(unix)] { let _ = std::fs::remove_file(socket_file()); }
            ListenerOptions::new().name(n).create_sync().map_err(|e| log::warn!("single-instance: listen failed: {e}")).ok()?
        }
    };
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new().name("qcv-single-instance".into()).spawn(move || {
        for conn in listener.incoming().flatten() {
            let lines: Vec<String> = BufReader::new(conn).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).collect();
            log::info!("single-instance: received {} argument(s)", lines.len());
            if tx.send(lines).is_err() { return }
        }
    }).ok()?;
    Some(rx)
}
