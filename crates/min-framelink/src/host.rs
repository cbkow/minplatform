//! The host's side: a provider as a child process, its input to write messages to, and its answers read on a thread
//! and stamped as they arrive.
use crate::msg::{ToHost, ToProvider, VERSION};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What arrives from a provider: a message and when it came, or `None` once - its end of the link has closed (it
/// ended, or it died).
pub type Answers = Receiver<(Instant, Option<ToHost>)>;

pub struct Provider { child: Mutex<Child>, to: Mutex<Option<ChildStdin>>, id: AtomicU64, pid: u32 }

impl Provider {
    /// Start the program and say `hello` to it; its own `hello` is the first thing the answers hold. A line that is
    /// no message of the link ends the reading as if the provider had gone: a host cannot go on with one that
    /// speaks something else. Its standard error is left where the host's goes.
    pub fn start(executable: &Path, args: &[String], host: &str) -> std::io::Result<(Provider, Answers)> {
        let mut child = Command::new(executable).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
        let (to, out) = (child.stdin.take().expect("piped"), child.stdout.take().expect("piped"));
        let (tx, answers) = channel();
        std::thread::Builder::new().name("framelink-in".into()).spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                let at = Instant::now();
                let Ok(m) = serde_json::from_str::<ToHost>(&line) else { break };
                if tx.send((at, Some(m))).is_err() { return }
            }
            let _ = tx.send((Instant::now(), None));
        })?;
        let pid = child.id();
        let p = Provider { child: Mutex::new(child), to: Mutex::new(Some(to)), id: AtomicU64::new(0), pid };
        p.send(&ToProvider::Hello { version: VERSION, host: host.to_string() })?;
        Ok((p, answers))
    }
    /// A number for a request, not used before.
    pub fn id(&self) -> u64 { self.id.fetch_add(1, Ordering::Relaxed) + 1 }
    pub fn pid(&self) -> u32 { self.pid }
    pub fn send(&self, m: &ToProvider) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(m).map_err(std::io::Error::other)?;
        line.push(b'\n');
        let mut to = self.to.lock().unwrap();
        let Some(pipe) = to.as_mut() else { return Err(std::io::Error::other("the provider's input is closed")) };
        pipe.write_all(&line)?;
        pipe.flush()
    }
}
impl Drop for Provider {
    /// `bye`, the input closed, a moment for the provider to end by itself, then it is ended.
    fn drop(&mut self) {
        let _ = self.send(&ToProvider::Bye);
        *self.to.lock().unwrap() = None;
        let mut child = self.child.lock().unwrap();
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until { if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) { return } std::thread::sleep(Duration::from_millis(5)) }
        let _ = child.kill();
        let _ = child.wait();
    }
}
