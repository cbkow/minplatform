//! The tasks of the application (canon row 508, points 1-4): everything that takes time - a copy, a move, a delete,
//! and from the later steps an archive and a transcode - is a row here from the moment it is asked for until it is
//! dismissed. The panel that shows them hangs on the status bar. One list for the application (every window shows the
//! same); nothing of it is kept across a quit. A task is called off by its own flag: waiting, it never starts;
//! running, its worker sees the flag (what it has done stays done).
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq)]
pub enum State { Waiting, Running, Done, Failed(String), Stopped }
impl State { pub fn over(&self) -> bool { matches!(self, State::Done | State::Failed(_) | State::Stopped) } }

#[derive(Clone, Debug)]
pub struct Task {
    pub id: u64, pub label: String, pub state: State,
    /// how far it is (0-1; none: not known) and what it has in hand - written by its worker
    pub progress: Option<f64>, pub detail: String,
    /// what it made, to be shown when it is over (an archive, an extracted folder, a transcoded file)
    pub made: Option<PathBuf>,
    pub cancel: Arc<AtomicBool>,
}

static LOG: Mutex<Vec<Task>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
static REV: AtomicU64 = AtomicU64::new(0);
/// Finished rows kept at most (the oldest go first; a failed one too - it was told when it failed).
const KEPT: usize = 100;

fn touch() { REV.fetch_add(1, Ordering::Relaxed); }
/// Changes whenever the list does (a window redraws its panel on it).
pub fn rev() -> u64 { REV.load(Ordering::Relaxed) }

/// A task is asked for: it waits. Its number, and the flag that calls it off.
pub fn add(label: &str) -> (u64, Arc<AtomicBool>) {
    let (id, cancel) = (NEXT.fetch_add(1, Ordering::Relaxed), Arc::new(AtomicBool::new(false)));
    let mut l = LOG.lock().unwrap();
    l.push(Task { id, label: label.to_string(), state: State::Waiting, progress: None, detail: String::new(), made: None, cancel: cancel.clone() });
    let over = l.iter().filter(|t| t.state.over()).count();
    if over > KEPT { let mut drop = over - KEPT; l.retain(|t| !(t.state.over() && drop > 0 && { drop -= 1; true })); }
    touch(); (id, cancel)
}
fn with(id: u64, f: impl FnOnce(&mut Task)) { if let Some(t) = LOG.lock().unwrap().iter_mut().find(|t| t.id == id) { f(t); } touch(); }
pub fn start(id: u64) { with(id, |t| t.state = State::Running); }
pub fn progress(id: u64, fraction: Option<f64>, detail: &str) { with(id, |t| { t.progress = fraction; t.detail = detail.to_string(); }); }
/// It is over: done, or failed with the reason; called off when its flag was set (whatever else it says).
pub fn finish(id: u64, result: Result<Option<PathBuf>, String>) {
    with(id, |t| { t.progress = None; t.detail.clear();
        t.state = if t.cancel.load(Ordering::Relaxed) { State::Stopped } else { match result { Ok(made) => { t.made = made; State::Done } Err(e) => State::Failed(e) } }; });
}
/// Call a task off. Waiting: it is over at once (its worker skips it). Running: its worker stops at the next step.
pub fn cancel(id: u64) { with(id, |t| if !t.state.over() { t.cancel.store(true, Ordering::Relaxed); if t.state == State::Waiting { t.state = State::Stopped; } }); }
/// A finished row is taken from the list.
pub fn dismiss(id: u64) { LOG.lock().unwrap().retain(|t| !(t.id == id && t.state.over())); touch(); }
pub fn clear_finished() { LOG.lock().unwrap().retain(|t| !t.state.over()); touch(); }
pub fn list() -> Vec<Task> { LOG.lock().unwrap().clone() }
/// (running or waiting, done or called off, failed)
pub fn counts() -> (usize, usize, usize) { let l = LOG.lock().unwrap(); (l.iter().filter(|t| !t.state.over()).count(), l.iter().filter(|t| matches!(t.state, State::Done | State::Stopped)).count(), l.iter().filter(|t| matches!(t.state, State::Failed(_))).count()) }
/// Whether anything runs or waits (a quit asks first).
pub fn busy() -> bool { counts().0 > 0 }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_task_waits_runs_and_is_over_and_a_waiting_one_called_off_never_starts() {
        let (a, _) = add("Copy 3 items"); let (b, fb) = add("Move 1 item"); let (c, fc) = add("Delete 2 items");
        let of = |id: u64| list().into_iter().find(|t| t.id == id).map(|t| t.state);
        assert_eq!(of(a), Some(State::Waiting)); assert!(busy());
        start(a); progress(a, Some(0.5), "plate.exr"); assert_eq!(list().iter().find(|t| t.id == a).unwrap().progress, Some(0.5));
        // a row that is not over is not dismissed
        dismiss(a); assert_eq!(of(a), Some(State::Running));
        finish(a, Ok(None)); assert_eq!(of(a), Some(State::Done));
        // waiting and called off: over at once, its flag set for the worker that will meet it
        cancel(b); assert_eq!(of(b), Some(State::Stopped)); assert!(fb.load(Ordering::Relaxed));
        // running and called off: it is "stopped" when its worker ends, whatever the worker reports
        start(c); cancel(c); assert_eq!(of(c), Some(State::Running)); assert!(fc.load(Ordering::Relaxed)); finish(c, Err("half done".into())); assert_eq!(of(c), Some(State::Stopped));
        let (d, _) = add("Copy 1 item"); start(d); finish(d, Err("The disk is full.".into())); assert_eq!(of(d), Some(State::Failed("The disk is full.".into())));
        dismiss(d); assert_eq!(of(d), None); clear_finished(); assert!(of(a).is_none() && of(b).is_none() && of(c).is_none());
    }
}
