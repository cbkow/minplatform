//! Where a pane has been (canon row 449: back and forward with history kept): a line of stops with a place on it.
//! Each stop keeps what was chosen there and how far it was scrolled, to be put back when the pane returns to it.
//! Going somewhere new from the middle of the line drops what lay ahead. Going up remembers the folder left, so that
//! it is the one chosen in its parent.
use std::path::{Path, PathBuf};

/// What a stop looked like when it was left: the chosen entries of the folder (their names) and the scroll.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look { pub selected: Vec<String>, pub scroll: f64 }

#[derive(Clone, Debug, PartialEq)]
pub struct Stop { pub path: PathBuf, pub look: Look }

pub struct History { stops: Vec<Stop>, at: usize }
/// No more stops than this are kept (the oldest go).
const KEPT: usize = 200;

impl History {
    pub fn new(path: PathBuf) -> History { History { stops: vec![Stop { path, look: Look::default() }], at: 0 } }
    pub fn current(&self) -> &Path { &self.stops[self.at].path }
    pub fn can_back(&self) -> bool { self.at > 0 }
    pub fn can_forward(&self) -> bool { self.at + 1 < self.stops.len() }
    /// The folder above the current one, and the name it is left by.
    pub fn parent(&self) -> Option<(PathBuf, String)> { let p = self.current(); let up = p.parent()?; if up.as_os_str().is_empty() { return None } Some((up.to_path_buf(), p.file_name()?.to_string_lossy().into_owned())) }
    /// To a new place; `leaving` is how the current one looks. The same place again is no new stop. What lay ahead is dropped.
    pub fn go(&mut self, path: PathBuf, leaving: Look) -> bool {
        if path == self.stops[self.at].path { return false }
        self.stops[self.at].look = leaving; self.stops.truncate(self.at + 1); self.stops.push(Stop { path, look: Look::default() });
        if self.stops.len() > KEPT { let over = self.stops.len() - KEPT; self.stops.drain(..over); }
        self.at = self.stops.len() - 1; true
    }
    /// One stop back: the place and how it looked when it was left.
    pub fn back(&mut self, leaving: Look) -> Option<Stop> { if !self.can_back() { return None } self.stops[self.at].look = leaving; self.at -= 1; Some(self.stops[self.at].clone()) }
    pub fn forward(&mut self, leaving: Look) -> Option<Stop> { if !self.can_forward() { return None } self.stops[self.at].look = leaving; self.at += 1; Some(self.stops[self.at].clone()) }
    /// Up to the parent: a new stop, in which the folder left is the one chosen.
    pub fn up(&mut self, leaving: Look) -> Option<Stop> {
        let (parent, child) = self.parent()?; self.go(parent, leaving);
        self.stops[self.at].look = Look { selected: vec![child], scroll: 0.0 }; Some(self.stops[self.at].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn look(names: &[&str], scroll: f64) -> Look { Look { selected: names.iter().map(|s| s.to_string()).collect(), scroll } }
    fn p(s: &str) -> PathBuf { PathBuf::from(s) }
    #[test]
    fn back_and_forward_put_the_choice_and_the_scroll_back() {
        let mut h = History::new(p("/jobs")); assert!(!h.can_back() && !h.can_forward()); assert!(h.back(Look::default()).is_none());
        assert!(h.go(p("/jobs/a"), look(&["a"], 120.0))); assert!(h.go(p("/jobs/a/shots"), look(&["shots", "notes.pdf"], 48.0)));
        assert!(!h.go(p("/jobs/a/shots"), Look::default()), "the same place again is no new stop");
        let s = h.back(look(&["sh_010"], 900.0)).unwrap(); assert_eq!(s, Stop { path: p("/jobs/a"), look: look(&["shots", "notes.pdf"], 48.0) }); assert!(h.can_forward());
        let s = h.back(Look::default()).unwrap(); assert_eq!((s.path.clone(), s.look), (p("/jobs"), look(&["a"], 120.0))); assert!(!h.can_back());
        let s = h.forward(look(&["a"], 120.0)).unwrap(); assert_eq!(s.path, p("/jobs/a")); assert_eq!(s.look, Look::default(), "it was left with nothing chosen");
        let s = h.forward(Look::default()).unwrap(); assert_eq!(s, Stop { path: p("/jobs/a/shots"), look: look(&["sh_010"], 900.0) }); assert!(h.forward(Look::default()).is_none());
        // somewhere new from the middle: what lay ahead is gone
        h.back(Look::default()); assert!(h.go(p("/elsewhere"), Look::default())); assert!(!h.can_forward()); assert_eq!(h.current(), Path::new("/elsewhere"));
        assert_eq!(h.back(Look::default()).unwrap().path, p("/jobs/a"));
    }
    #[test]
    fn up_chooses_the_folder_it_came_from() {
        let mut h = History::new(p("/jobs/a/shots"));
        let s = h.up(look(&["x"], 10.0)).unwrap(); assert_eq!(s, Stop { path: p("/jobs/a"), look: look(&["shots"], 0.0) });
        assert_eq!(h.back(Look::default()).unwrap(), Stop { path: p("/jobs/a/shots"), look: look(&["x"], 10.0) }, "up is a stop like any other");
        let mut h = History::new(p("/")); assert!(h.up(Look::default()).is_none(), "nothing above the root"); assert!(h.parent().is_none());
        let mut h = History::new(p("/a")); assert_eq!(h.up(Look::default()).unwrap().path, p("/"));
    }
    #[test]
    fn only_so_many_stops_are_kept() {
        let mut h = History::new(p("/0")); for i in 1..500 { h.go(p(&format!("/{i}")), Look::default()); }
        assert_eq!(h.current(), Path::new("/499")); let mut n = 0; while h.back(Look::default()).is_some() { n += 1; } assert_eq!(n, KEPT - 1);
    }
}
