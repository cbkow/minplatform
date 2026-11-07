//! The hover / pressed ramp (§16.4): a value that eases toward its target over a fixed duration, started from wherever it is.
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct Ramp { started: Option<Instant>, from: f32, target: f32, duration_ms: u32 }

impl Ramp {
    pub fn new(value: f32, duration_ms: u32) -> Ramp { Ramp { started: None, from: value, target: value, duration_ms } }
    pub fn target(&self) -> f32 { self.target }
    /// Head toward `target` from the current value (a reversal mid-ramp starts where it is, no jump).
    pub fn set_target(&mut self, target: f32, now: Instant) {
        if (target - self.target).abs() < f32::EPSILON { return }
        self.from = self.value(now); self.target = target; self.started = Some(now);
    }
    pub fn value(&self, now: Instant) -> f32 {
        let Some(t0) = self.started else { return self.target };
        let t = (now.duration_since(t0).as_secs_f32() * 1000.0 / self.duration_ms.max(1) as f32).clamp(0.0, 1.0);
        let e = t * (2.0 - t);   // ease-out: answers the pointer at once, settles gently
        self.from + (self.target - self.from) * e
    }
    pub fn animating(&self, now: Instant) -> bool { self.started.is_some_and(|t0| now.duration_since(t0).as_secs_f32() * 1000.0 < self.duration_ms as f32) }
}

/// Linear interpolation of two theme colours (in their sRGB-encoded space, alpha included — the ramp's idle end is the
/// hover colour at alpha 0, so this never passes through black).
pub fn mix(a: min_theme::Color, b: min_theme::Color, t: f32) -> min_theme::Color {
    let l = |x: f32, y: f32| x + (y - x) * t;
    min_theme::Color { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }
}
