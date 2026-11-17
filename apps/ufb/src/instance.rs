//! One UFB a user (canon row 519, point 7; 1.4.2: a lock file and a local socket): a second launch hands what it was
//! asked to open to the one that runs, and ends. The one that runs keeps a small file in its data folder -
//! `instance.json`: the port it listens on (this machine's own address only), a word only readers of that folder
//! know, its process's number - and a thread that takes what a second launch sends.
//!
//! A second launch that cannot reach the port (the file is left over from a crash) takes its place. Each data folder
//! has its own instance: a scripted look (`UFB_DEMO`, `UFB_SHOT`, `UFB_CLOSE_AFTER`) and `UFB_MULTI=1` take no part.
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static HELD: OnceLock<(TcpListener, String, PathBuf)> = OnceLock::new();
static ASKED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn file_of(dir: &Path) -> PathBuf { dir.join("instance.json") }
pub fn takes_part() -> bool { !["UFB_DEMO", "UFB_SHOT", "UFB_CLOSE_AFTER", "UFB_MULTI"].iter().any(|k| std::env::var_os(k).is_some()) }

/// Hand `message` (a path, a link, or nothing: "come forward") to the instance of this data folder that runs. True:
/// it took it - this launch has nothing more to do.
pub fn hand_over(dir: &Path, message: &str) -> bool {
    let Some(v) = std::fs::read_to_string(file_of(dir)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return false };
    let (Some(port), Some(token)) = (v["port"].as_u64().filter(|p| *p > 0 && *p < 65536), v["token"].as_str()) else { return false };
    let Ok(mut s) = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port as u16)), Duration::from_millis(500)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500))); let _ = s.set_write_timeout(Some(Duration::from_millis(1500)));
    if s.write_all(format!("{token}\n{}\n", message.replace(['\n', '\r'], " ")).as_bytes()).is_err() { return false }
    let mut answer = String::new(); BufReader::new(s).read_line(&mut answer).is_ok() && answer.trim() == "ok"
}
/// Become the instance of this data folder: listen, and write the file. False: no port could be had (the launch goes
/// on alone, as before).
pub fn claim(dir: &Path) -> bool {
    let Ok(l) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else { return false }; let Ok(port) = l.local_addr().map(|a| a.port()) else { return false };
    let token = uuid::Uuid::new_v4().simple().to_string(); let _ = std::fs::create_dir_all(dir);
    let (file, tmp) = (file_of(dir), dir.join(format!("instance.{}.tmp", std::process::id())));
    if std::fs::write(&tmp, serde_json::json!({ "port": port, "token": token, "pid": std::process::id() }).to_string()).and_then(|()| std::fs::rename(&tmp, &file)).is_err() { let _ = std::fs::remove_file(&tmp); return false }
    HELD.set((l, token, file)).is_ok()
}
/// Take what second launches send, from now on: each is kept for the window's thread, which `wake` calls.
pub fn serve(wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
    let Some((l, token, _)) = HELD.get() else { return }; let Ok(l) = l.try_clone() else { return }; let token = token.clone();
    let _ = std::thread::Builder::new().name("ufb-instance".into()).spawn(move || {
        for s in l.incoming().flatten() {
            let _ = s.set_read_timeout(Some(Duration::from_millis(1500))); let Ok(mut out) = s.try_clone() else { continue };
            let mut r = BufReader::new(s); let (mut word, mut message) = (String::new(), String::new());
            if r.read_line(&mut word).is_err() || word.trim() != token || r.read_line(&mut message).is_err() { continue }
            ASKED.lock().unwrap().push(message.trim().to_string()); let _ = out.write_all(b"ok\n"); wake();
        }
    });
}
/// Something to open came another way than from a second launch (macOS: a link the system handed to the application
/// that runs - `shell.rs`): it joins what second launches asked for.
pub fn ask(text: String) { ASKED.lock().unwrap().push(text); }
/// What second launches asked for since the last look (an empty text: only "come forward").
pub fn take() -> Vec<String> { std::mem::take(&mut *ASKED.lock().unwrap()) }
/// The application ends: its file goes (when it is still its own).
pub fn release() { if let Some((_, token, file)) = HELD.get() { if std::fs::read_to_string(file).is_ok_and(|t| t.contains(token.as_str())) { let _ = std::fs::remove_file(file); } } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_second_launch_hands_over_and_a_stale_file_is_passed_by() {
        let dir = std::env::temp_dir().join(format!("ufb-instance-test-{}", std::process::id())); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        // nobody runs: nothing to hand over to; a file left by a crash (a port nobody listens on) neither
        assert!(!hand_over(&dir, "/a/b"));
        let dead = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        std::fs::write(file_of(&dir), serde_json::json!({ "port": dead, "token": "x", "pid": 1 }).to_string()).unwrap(); assert!(!hand_over(&dir, "/a/b"));
        // one runs
        assert!(claim(&dir)); let woke = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)); let w = woke.clone(); serve(std::sync::Arc::new(move || { w.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }));
        assert!(hand_over(&dir, "ufb:///Volumes/Jobs/2601_a")); assert!(hand_over(&dir, ""));
        assert_eq!(take(), ["ufb:///Volumes/Jobs/2601_a", ""]); assert_eq!(woke.load(std::sync::atomic::Ordering::Relaxed), 2); assert!(take().is_empty());
        // the word is wrong (another user's guess): not taken
        let good = std::fs::read_to_string(file_of(&dir)).unwrap(); let mut v: serde_json::Value = serde_json::from_str(&good).unwrap(); v["token"] = "wrong".into(); std::fs::write(file_of(&dir), v.to_string()).unwrap();
        assert!(!hand_over(&dir, "/x")); assert!(take().is_empty());
        std::fs::write(file_of(&dir), good).unwrap(); release(); assert!(!file_of(&dir).exists()); let _ = std::fs::remove_dir_all(&dir);
    }
}
