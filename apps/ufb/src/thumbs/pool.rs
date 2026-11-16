//! The application's side of the helpers (spike S18; canon row 456): up to N helper copies of the executable behind
//! pipes, one queue, cancellation of what waits, and one watchdog that ends a helper whose request has passed its
//! hard bound. A helper is started when there is work for it and ended after a while without any; one that died or
//! was ended is replaced by the next request - the failure is reported first.
//!
//! No thread here waits without a bound: a worker's read ends when its helper answers, dies, or is ended by the
//! watchdog. Nothing is ever abandoned inside the application: what is thrown away is a PROCESS.
use super::proto::*;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// The argument that makes the executable a helper (the first thing `main` looks at).
pub const HELPER_ARG: &str = "--thumb-helper";
/// A helper's start, from the spawn to its hello (S18: 3 - 15 ms; a bound, not an expectation).
const START_BOUND: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct Job { pub req: Request, pub hard_ms: u32 }
pub struct Done { pub id: u64, pub status: u8, pub w: u16, pub h: u16, pub payload: Vec<u8> }

struct Slot { child: Option<Arc<Mutex<Child>>>, deadline: Option<Instant>, killed: bool }
struct State { queue: VecDeque<u64>, jobs: HashMap<u64, Job>, slots: Vec<Slot>, shutdown: bool, live: usize, started: u64 }
impl State {
    /// The next request that is still wanted (an id whose request was cancelled is skipped).
    fn take(&mut self) -> Option<Job> { while let Some(id) = self.queue.pop_front() { if let Some(j) = self.jobs.remove(&id) { return Some(j) } } None }
}
struct Shared { st: Mutex<State>, work: Condvar, dog: Condvar, exe: PathBuf, env: Vec<(String, String)>, idle: Duration, wake: Arc<dyn Fn() + Send + Sync>, #[cfg(windows)] job: isize }

struct Helper { child: Arc<Mutex<Child>>, stdin: ChildStdin, stdout: ChildStdout }
impl Helper {
    /// Closing its input is how a healthy helper is told to end; the kill makes sure, and the wait reaps it.
    fn end(self) { drop(self.stdin); let mut c = self.child.lock().unwrap(); let _ = c.kill(); let _ = c.wait(); }
}

pub struct Pool { sh: Arc<Shared>, pub rx: Receiver<Done> }

impl Pool {
    /// `n` helpers at most, each a copy of `exe` started with [`HELPER_ARG`] (and `env`), none of them yet.
    /// `idle`: how long a helper without work is kept. `wake` is called after every answer put into `rx`.
    pub fn new(exe: PathBuf, env: Vec<(String, String)>, n: usize, idle: Duration, wake: Arc<dyn Fn() + Send + Sync>) -> Pool {
        let slots = (0..n.max(1)).map(|_| Slot { child: None, deadline: None, killed: false }).collect();
        let st = State { queue: VecDeque::new(), jobs: HashMap::new(), slots, shutdown: false, live: 0, started: 0 };
        let sh = Arc::new(Shared { st: Mutex::new(st), work: Condvar::new(), dog: Condvar::new(), exe, env, idle, wake, #[cfg(windows)] job: win_job() });
        // answers wait in a bounded channel: a window that does not take them holds the helpers back, not the memory
        let (tx, rx) = sync_channel(32);
        for slot in 0..n.max(1) { let (sh, tx) = (sh.clone(), tx.clone()); let _ = std::thread::Builder::new().name("ufb-thumbs".into()).spawn(move || worker(sh, slot, tx)); }
        { let sh = sh.clone(); let _ = std::thread::Builder::new().name("ufb-thumbs-dog".into()).spawn(move || watchdog(sh)); }
        Pool { sh, rx }
    }
    pub fn submit(&self, job: Job) { let mut st = self.sh.st.lock().unwrap(); let id = job.req.id; st.jobs.insert(id, job); st.queue.push_back(id); drop(st); self.sh.work.notify_one(); }
    /// Call off requests that still wait. Returns those that were freed; one already with a helper is left to finish
    /// (its answer comes as any other).
    pub fn cancel(&self, ids: &[u64]) -> Vec<u64> {
        let mut st = self.sh.st.lock().unwrap();
        let freed: Vec<u64> = ids.iter().copied().filter(|id| st.jobs.remove(id).is_some()).collect();
        // the ids left behind in the queue are 8 bytes each; when most of it is dead, it is swept
        let st = &mut *st;
        if st.queue.len() > 2 * st.jobs.len() + 64 { let jobs = &st.jobs; st.queue.retain(|id| jobs.contains_key(id)); }
        freed
    }
    /// (requests waiting, helpers alive, helpers started so far)
    #[allow(dead_code)]
    pub fn counts(&self) -> (usize, usize, u64) { let st = self.sh.st.lock().unwrap(); (st.jobs.len(), st.live, st.started) }
}
impl Drop for Pool {
    /// The threads are not waited for: told to end, their helpers ended under them, they leave by themselves.
    fn drop(&mut self) {
        let mut st = self.sh.st.lock().unwrap(); st.shutdown = true; st.jobs.clear(); st.queue.clear();
        for s in st.slots.iter_mut() { if let Some(c) = &s.child { let _ = c.lock().unwrap().kill(); } }
        drop(st); self.sh.work.notify_all(); self.sh.dog.notify_all();
    }
}

fn spawn(sh: &Shared) -> std::io::Result<Helper> {
    let mut c = Command::new(&sh.exe);
    c.arg(HELPER_ARG).envs(sh.env.iter().map(|(k, v)| (k, v))).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }   // CREATE_NO_WINDOW
    let mut child = c.spawn()?;
    let (stdin, stdout) = (child.stdin.take().ok_or_else(|| std::io::Error::other("no pipe"))?, child.stdout.take().ok_or_else(|| std::io::Error::other("no pipe"))?);
    #[cfg(windows)] win_assign(sh.job, &child);
    Ok(Helper { child: Arc::new(Mutex::new(child)), stdin, stdout })
}

fn worker(sh: Arc<Shared>, slot: usize, tx: SyncSender<Done>) {
    let mut helper: Option<Helper> = None;
    let bound = |d: Option<Duration>| { let mut st = sh.st.lock().unwrap(); st.slots[slot].deadline = d.map(|d| Instant::now() + d); drop(st); sh.dog.notify_one(); };
    let gone = |h: Helper| { { let mut st = sh.st.lock().unwrap(); st.slots[slot].child = None; st.live -= 1; } h.end(); };
    loop {
        let job = {
            let mut st = sh.st.lock().unwrap();
            loop {
                if st.shutdown { drop(st); if let Some(h) = helper.take() { gone(h); } return }
                if let Some(j) = st.take() { st.slots[slot].killed = false; break Some(j) }
                if helper.is_some() { let (g, to) = sh.work.wait_timeout(st, sh.idle).unwrap(); st = g; if to.timed_out() && st.jobs.is_empty() && !st.shutdown { break None } }
                else { st = sh.work.wait(st).unwrap(); }
            }
        };
        // nothing to do for a while: the helper is ended (it holds what its decoders allocated)
        let Some(job) = job else { if let Some(h) = helper.take() { gone(h); } continue };
        let fail = |status: u8| Done { id: job.req.id, status, w: 0, h: 0, payload: Vec::new() };
        if helper.is_none() {
            let started = spawn(&sh).ok().and_then(|mut h| {
                { let mut st = sh.st.lock().unwrap(); st.slots[slot].child = Some(h.child.clone()); st.live += 1; st.started += 1; }
                bound(Some(START_BOUND));
                let hello = read_response(&mut h.stdout); bound(None);
                if hello.is_ok() { Some(h) } else { gone(h); None }
            });
            match started { Some(h) => helper = Some(h), None => { if tx.send(fail(NO_HELPER)).is_err() { return } (sh.wake)(); continue } }
        }
        let h = helper.as_mut().unwrap();
        bound(Some(Duration::from_millis(job.hard_ms as u64)));
        let r = write_request(&mut h.stdin, &job.req).and_then(|_| read_response(&mut h.stdout));
        let killed = { let mut st = sh.st.lock().unwrap(); let s = &mut st.slots[slot]; s.deadline = None; s.killed };
        let dead = r.is_err() || killed;
        let done = match r {
            Ok(a) if a.id == job.req.id && !killed => Done { id: a.id, status: a.status, w: a.w, h: a.h, payload: a.payload },
            _ => fail(if killed { HUNG } else { CRASHED }),
        };
        // the failure is reported BEFORE anything is started again
        let sent = tx.send(done).is_ok(); if sent { (sh.wake)(); }
        if dead || !sent { if let Some(h) = helper.take() { gone(h); } }
        if !sent { return }
    }
}

/// One thread for the whole pool: it sleeps until the nearest deadline, and ends the helper of a request past it.
fn watchdog(sh: Arc<Shared>) {
    let mut st = sh.st.lock().unwrap();
    loop {
        if st.shutdown { return }
        let now = Instant::now();
        let mut next = now + Duration::from_secs(3600);
        for s in st.slots.iter_mut() {
            if let (Some(deadline), false) = (s.deadline, s.killed) {
                if deadline <= now { s.killed = true; if let Some(c) = &s.child { let _ = c.lock().unwrap().kill(); } } else { next = next.min(deadline) }
            }
        }
        st = sh.dog.wait_timeout(st, next - now).unwrap().0;
    }
}

// Windows: a job that ends every helper when the application's last handle to it closes, i.e. when the application
// ends in any way. Created once per pool; every helper is assigned to it right after its start.
#[cfg(windows)]
fn win_job() -> isize {
    use windows::Win32::System::JobObjects::*;
    unsafe {
        let Ok(job) = CreateJobObjectW(None, windows::core::PCWSTR::null()) else { return 0 };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let _ = SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32);
        job.0 as isize
    }
}
#[cfg(windows)]
fn win_assign(job: isize, child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::AssignProcessToJobObject;
    if job != 0 { unsafe { let _ = AssignProcessToJobObject(HANDLE(job as *mut _), HANDLE(child.as_raw_handle() as *mut _)); } }
}
