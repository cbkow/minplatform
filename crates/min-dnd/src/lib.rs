//! Real drag and drop of FILES between a winit window drawn by us and the system - Finder, Explorer, other
//! applications - both ways (spike S16; canon rows 459, 460, 472). Two platform files with one surface:
//!
//! - [`attach`]: the window becomes a drop target. What comes over it arrives as [`Notice::Drag`] (enter / over / leave
//!   / drop, the place, what the source allows under the keys held, whether the drag is our own, the payload); the
//!   application ANSWERS with the operation it would perform (0: refused).
//! - [`start_drag`]: the system's drag for some paths, with a picture; its end arrives as [`Notice::Ended`] with what
//!   the target did.
//! - a promised file (a mail attachment, a browser's picture) is received into a folder the application names, and
//!   arrives - later - as [`Notice::Promised`].
//!
//! The rules the spike found, kept here: on macOS our destination is a run-time subclass of winit's VIEW (winit's own
//! is its window, which is un-registered); every AppKit call of ours that can raise, made from a callback, is caught;
//! file URLs are read as URL objects (Finder hands file-REFERENCE URLs); a path that comes back from a drag need not be
//! byte-equal to the one that went out (names come back decomposed) - compare by identity. On Windows the window must
//! be made with winit's own drag and drop OFF; `DoDragDrop` does not return until the drag is over.
use std::path::PathBuf;
use std::rc::Rc;

pub const COPY: u32 = 1; pub const LINK: u32 = 2; pub const MOVE: u32 = 4;
#[derive(Clone, Copy, PartialEq, Debug)] pub enum Phase { Enter, Over, Leave, Drop }
/// What a drag carries, as far as a file browser cares.
#[derive(Clone, Debug, Default)]
pub struct Payload { /// the types / formats offered, by the system's own names
    pub types: Vec<String>, /// local files and folders
    pub paths: Vec<PathBuf>, /// URLs that are not local files
    pub urls: Vec<String>, pub text: Option<String>, /// files that do not exist yet (a mail attachment, a browser's image): one entry each, what is known before delivery
    pub promised: Vec<String> }
pub struct DragEvent { pub win: usize, pub phase: Phase, /// physical pixels from the window's content's top left
    pub x: f64, pub y: f64, /// what the source allows, narrowed by the modifier keys held
    pub allowed: u32, /// the platform's own account (raw masks, key state, where the drag comes from)
    pub detail: String, /// our own drag's serial, when this drag was started by this process
    pub mine: Option<u64>, pub payload: Rc<Payload> }
pub enum Notice { Drag(DragEvent), /// a promised file has arrived (or has not)
    Promised { win: usize, result: Result<PathBuf, String> }, /// a drag we started is over: what the target did
    Ended { serial: u64, op: u32, detail: String } }
/// The application's answer to a `Drag` is the operation it would perform (0: refuse); to the others, nothing.
pub type Sink = Rc<dyn Fn(Notice) -> u32>;
/// The picture under the pointer during a drag: straight (not premultiplied) RGBA, physical pixels.
pub struct DragImage { pub w: u32, pub h: u32, pub rgba: Vec<u8>, pub hot: (f64, f64), pub scale: f64 }

#[cfg(target_os = "macos")] mod mac;
#[cfg(target_os = "macos")] pub use mac::{attach, detach, start_drag};
#[cfg(windows)] mod win;
#[cfg(windows)] pub use win::{attach, detach, start_drag};
#[cfg(not(any(target_os = "macos", windows)))]
mod none {
    use super::*;
    pub fn attach(_w: &dyn winit31::window::Window, _win: usize, _dest: &std::path::Path, _sink: Sink) -> Result<String, String> { Err("no platform layer".into()) }
    pub fn detach(_w: &dyn winit31::window::Window) {}
    pub fn start_drag(_w: &dyn winit31::window::Window, _paths: &[PathBuf], _img: DragImage, _ops: u32, _sink: Sink) -> Result<u64, String> { Err("no platform layer".into()) }
}
#[cfg(not(any(target_os = "macos", windows)))] pub use none::{attach, start_drag};
