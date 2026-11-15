//! The parent's side: N helpers, two queues (urgent before normal), cancellation, a watchdog that kills a helper whose
//! request has passed its hard bound, and the replacement of a dead helper.
//! No thread here waits without a bound: a worker's read ends when its helper answers, dies, or is killed by the
//! watchdog at the request's deadline. Nothing is ever abandoned: the thing that is thrown away is a PROCESS.
use crate::proto::*;
use std::collections::{HashMap, VecDeque};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Job { pub req: Request, pub hard_ms: u32 }

pub struct Done {
    pub id: u64, pub status: u8, pub w: u16, pub h: u16, pub work_us: u32, pub aux: u32, pub payload: Vec<u8>,
    /// the parent's round trip: from the request's first byte written to the answer's last byte read
    pub rt_us: u64, #[allow(dead_code)] pub slot: usize,
}

struct Slot { child: Option<Arc<Mutex<Child>>>, inflight: Option<(u64, Instant)>, killed: bool, discard: bool }

#[derive(Default)]
struct State {
    urgent: VecDeque<u64>, normal: VecDeque<u64>, jobs: HashMap<u64, Job>, slots: Vec<Slot>, shutdown: bool,
    pub replace_ms: Vec<f64>,
}
impl State {
    /// Urgent first. An id whose job is gone (cancelled, or taken through the other queue) is skipped.
    fn take(&mut self) -> Option<Job> {
        while let Some(id) = self.urgent.pop_front() { if let Some(j) = self.jobs.remove(&id) { return Some(j) } }
        while let Some(id) = self.normal.pop_front() { if let Some(j) = self.jobs.remove(&id) { return Some(j) } }
        None
    }
}
struct Shared { st: Mutex<State>, work: Condvar, dog: Condvar, quiet: bool, #[cfg(windows)] job: isize }

pub struct Helper { pub child: Arc<Mutex<Child>>, pub stdin: ChildStdin, pub stdout: ChildStdout, pub startup_ms: f64, pub pid: u32 }
impl Helper {
    pub fn spawn(quiet: bool) -> std::io::Result<Helper> {
        let t = Instant::now();
        let mut c = Command::new(std::env::current_exe()?);
        c.arg("--helper").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(if quiet { Stdio::null() } else { Stdio::inherit() });
        #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); } // CREATE_NO_WINDOW
        let mut child = c.spawn()?;
        let stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let pid = child.id();
        read_response(&mut stdout)?; // the hello
        Ok(Helper { child: Arc::new(Mutex::new(child)), stdin, stdout, startup_ms: t.elapsed().as_secs_f64() * 1e3, pid })
    }
    /// Closing its input is how a healthy helper is told to end; a kill makes sure, and the wait reaps it.
    pub fn end(self) { drop(self.stdin); let mut c = self.child.lock().unwrap(); let _ = c.kill(); let _ = c.wait(); }
}

pub struct Pool { sh: Arc<Shared>, pub rx: Receiver<Done>, threads: Vec<std::thread::JoinHandle<()>>, pub startup_ms: Vec<f64>, pub pids: Vec<u32> }

impl Pool {
    pub fn new(n: usize, quiet: bool) -> Pool {
        let mut st = State::default();
        for _ in 0..n { st.slots.push(Slot { child: None, inflight: None, killed: false, discard: false }); }
        #[cfg(windows)] let job = win_job();
        let sh = Arc::new(Shared { st: Mutex::new(st), work: Condvar::new(), dog: Condvar::new(), quiet, #[cfg(windows)] job });
        let (tx, rx) = channel();
        let (rtx, rrx) = channel();
        let mut threads = Vec::new();
        for slot in 0..n { let (sh, tx, rtx) = (sh.clone(), tx.clone(), rtx.clone()); threads.push(std::thread::spawn(move || worker(sh, slot, tx, rtx))); }
        { let sh = sh.clone(); threads.push(std::thread::spawn(move || watchdog(sh))); }
        let mut startup_ms = Vec::new(); let mut pids = Vec::new();
        for _ in 0..n { let (ms, pid) = rrx.recv().expect("a helper did not start"); startup_ms.push(ms); pids.push(pid); }
        Pool { sh, rx, threads, startup_ms, pids }
    }
    pub fn submit(&self, job: Job, urgent: bool) {
        let mut st = self.sh.st.lock().unwrap();
        let id = job.req.id;
        st.jobs.insert(id, job);
        if urgent { st.urgent.push_back(id) } else { st.normal.push_back(id) }
        drop(st); self.sh.work.notify_one();
    }
    pub fn submit_many(&self, jobs: impl Iterator<Item = Job>) {
        let mut st = self.sh.st.lock().unwrap();
        for job in jobs { let id = job.req.id; st.jobs.insert(id, job); st.normal.push_back(id); }
        drop(st); self.sh.work.notify_all();
    }
    /// Mark queued requests urgent: they go to the urgent queue; their old place in the normal queue is skipped later.
    pub fn promote(&self, ids: &[u64]) {
        let mut st = self.sh.st.lock().unwrap();
        for id in ids { if st.jobs.contains_key(id) { st.urgent.push_back(*id) } }
    }
    /// Cancel: a queued request is freed now; one in flight is left to finish and its answer is dropped.
    /// Returns how many queued requests were freed.
    pub fn cancel(&self, ids: &[u64]) -> usize {
        let mut st = self.sh.st.lock().unwrap();
        let mut n = 0;
        for id in ids {
            if st.jobs.remove(id).is_some() { n += 1 } else { for s in st.slots.iter_mut() { if s.inflight.map(|f| f.0) == Some(*id) { s.discard = true } } }
        }
        // the ids left behind in the queues are 8 bytes each; when most of a queue is dead, it is swept
        let st = &mut *st;
        if st.normal.len() > 2 * st.jobs.len() + 64 { let jobs = &st.jobs; st.normal.retain(|id| jobs.contains_key(id)); st.normal.shrink_to_fit(); }
        if st.urgent.len() > 2 * st.jobs.len() + 64 { let jobs = &st.jobs; st.urgent.retain(|id| jobs.contains_key(id)); }
        st.jobs.shrink_to_fit();
        n
    }
    pub fn queued(&self) -> (usize, usize) { let st = self.sh.st.lock().unwrap(); (st.jobs.len(), st.urgent.len() + st.normal.len()) }
    pub fn replace_ms(&self) -> Vec<f64> { std::mem::take(&mut self.sh.st.lock().unwrap().replace_ms) }
    pub fn live_pids(&self) -> Vec<u32> { self.sh.st.lock().unwrap().slots.iter().filter_map(|s| s.child.as_ref().map(|c| c.lock().unwrap().id())).collect() }
}
impl Drop for Pool {
    fn drop(&mut self) {
        self.sh.st.lock().unwrap().shutdown = true;
        self.sh.work.notify_all(); self.sh.dog.notify_all();
        for t in self.threads.drain(..) { let _ = t.join(); }
    }
}

fn spawn_helper(sh: &Shared) -> Helper {
    let h = Helper::spawn(sh.quiet).expect("could not start a helper");
    #[cfg(windows)] win_assign(sh.job, &h);
    h
}

fn worker(sh: Arc<Shared>, slot: usize, tx: Sender<Done>, ready: Sender<(f64, u32)>) {
    let mut h = spawn_helper(&sh);
    sh.st.lock().unwrap().slots[slot].child = Some(h.child.clone());
    let _ = ready.send((h.startup_ms, h.pid));
    loop {
        let job = {
            let mut st = sh.st.lock().unwrap();
            loop {
                if st.shutdown { drop(st); h.end(); return }
                if let Some(j) = st.take() {
                    let s = &mut st.slots[slot];
                    s.inflight = Some((j.req.id, Instant::now() + Duration::from_millis(j.hard_ms as u64))); s.killed = false; s.discard = false;
                    break j;
                }
                st = sh.work.wait(st).unwrap();
            }
        };
        sh.dog.notify_one();
        let t0 = Instant::now();
        let r = write_request(&mut h.stdin, &job.req).and_then(|_| read_response(&mut h.stdout));
        let rt_us = t0.elapsed().as_micros() as u64;
        let (killed, discard) = { let mut st = sh.st.lock().unwrap(); let s = &mut st.slots[slot]; s.inflight = None; (s.killed, s.discard) };
        let dead = r.is_err() || killed;
        let done = match r {
            Ok(a) if a.id == job.req.id && !discard => Done { id: a.id, status: a.status, w: a.w, h: a.h, work_us: a.work_us, aux: a.aux, payload: a.payload, rt_us, slot },
            Ok(_) => Done { id: job.req.id, status: CANCELLED, w: 0, h: 0, work_us: 0, aux: 0, payload: Vec::new(), rt_us, slot },
            Err(_) => Done { id: job.req.id, status: if killed { HUNG } else { CRASHED }, w: 0, h: 0, work_us: 0, aux: 0, payload: Vec::new(), rt_us, slot },
        };
        let _ = tx.send(done); // the failure is reported BEFORE the replacement is started
        if dead {
            let t = Instant::now();
            h.end();
            if sh.st.lock().unwrap().shutdown { return }
            h = spawn_helper(&sh);
            let mut st = sh.st.lock().unwrap();
            st.slots[slot].child = Some(h.child.clone());
            st.replace_ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
    }
}

/// One thread for the whole pool: it sleeps until the nearest deadline, and kills the helper of a request past it.
fn watchdog(sh: Arc<Shared>) {
    let mut st = sh.st.lock().unwrap();
    loop {
        if st.shutdown { return }
        let now = Instant::now();
        let mut next = now + Duration::from_secs(3600);
        for s in st.slots.iter_mut() {
            if let (Some((_, deadline)), false) = (s.inflight, s.killed) {
                if deadline <= now { s.killed = true; if let Some(c) = &s.child { let _ = c.lock().unwrap().kill(); } } else { next = next.min(deadline) }
            }
        }
        st = sh.dog.wait_timeout(st, next - now).unwrap().0;
    }
}

// UNTESTED (Windows): a job that kills every helper when the application's last handle to it closes, i.e. when the
// application ends in any way. Created once per pool; every helper is assigned to it right after its start.
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
fn win_assign(job: isize, h: &Helper) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::AssignProcessToJobObject;
    if job != 0 { unsafe { let _ = AssignProcessToJobObject(HANDLE(job as *mut _), HANDLE(h.child.lock().unwrap().as_raw_handle() as *mut _)); } }
}
