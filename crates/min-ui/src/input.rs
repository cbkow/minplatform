//! Pointer input as the nodes see it: physical pixels, the window's coordinate space (§16.4: one router, not per-widget handlers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button { Primary, Secondary, Middle }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Move { x: f64, y: f64 },
    Down { x: f64, y: f64, button: Button },
    Up { x: f64, y: f64, button: Button },
    /// The pointer left the window.
    Leave,
}
impl Event {
    pub fn pos(&self) -> Option<(f64, f64)> { match *self { Event::Move { x, y } | Event::Down { x, y, .. } | Event::Up { x, y, .. } => Some((x, y)), Event::Leave => None } }
}
