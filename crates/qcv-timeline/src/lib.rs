//! `qcv-timeline` — RATIONAL time (canon §17.5, row 83): a frame count at a rate, never a double. Clips carry source
//! trims at the source rate and sit on the timeline at the timeline rate; `resolve` maps a timeline frame to a clip and
//! a source frame. Seconds are for display only. Grown from S2's `timeline.rs`; step 2 adds tracks, markers, the
//! Sequencer's view. OTIO is ruled out (row 83): this is the family's own type.
#![forbid(unsafe_code)]

/// Frames per second as a rational: 24000/1001, 24/1, 30000/1001 …
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rate { pub num: u32, pub den: u32 }
impl Rate {
    pub const fn new(num: u32, den: u32) -> Rate { Rate { num, den } }
    /// An exact rate from a container's own rational (FFmpeg `AVRational`); a zero or negative part → None.
    pub fn from_rational(num: i32, den: i32) -> Option<Rate> { if num > 0 && den > 0 { Some(Rate { num: num as u32, den: den as u32 }) } else { None } }
    /// The nearest well-known rate for a float (the NTSC family snaps; integers stay exact; anything else → milli-fps).
    pub fn from_f64(fps: f64) -> Rate {
        for (n, d) in [(24000, 1001), (30000, 1001), (60000, 1001), (48000, 1001)] { if (fps - n as f64 / d as f64).abs() < 1e-3 { return Rate { num: n, den: d }; } }
        if (fps - fps.round()).abs() < 1e-6 { Rate { num: fps.round() as u32, den: 1 } } else { Rate { num: (fps * 1000.0).round() as u32, den: 1000 } }
    }
    pub fn as_f64(&self) -> f64 { self.num as f64 / self.den as f64 }
    pub fn frame_seconds(&self) -> f64 { self.den as f64 / self.num as f64 }
}
impl std::fmt::Display for Rate { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { if self.den == 1 { write!(f, "{}", self.num) } else { write!(f, "{}/{}", self.num, self.den) } } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RationalTime { pub value: i64, pub rate: Rate }
impl RationalTime {
    pub const fn new(value: i64, rate: Rate) -> Self { Self { value, rate } }
    pub fn seconds(&self) -> f64 { self.value as f64 * self.rate.frame_seconds() }
    pub fn from_seconds(s: f64, rate: Rate) -> Self { Self { value: (s * rate.as_f64()).round() as i64, rate } }
    /// Exact when the rates are equal; otherwise the nearest frame at `rate` (edits snap by construction).
    pub fn rescaled_to(&self, rate: Rate) -> Self {
        if rate == self.rate { return *self }
        let v = self.value as i128 * rate.num as i128 * self.rate.den as i128;
        let d = self.rate.num as i128 * rate.den as i128;
        Self { value: ((v + d / 2) / d) as i64, rate }
    }
}

/// A clip: `src_in..src_out` (exclusive) source frames at `src_rate`, placed at `start` timeline frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip { pub path: String, pub src_rate: Rate, pub src_in: i64, pub src_out: i64, pub start: i64 }
impl Clip {
    pub fn len_at(&self, rate: Rate) -> i64 { RationalTime::new(self.src_out - self.src_in, self.src_rate).rescaled_to(rate).value }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timeline { pub rate: Rate, pub clips: Vec<Clip> }
impl Timeline {
    /// A contiguous playlist: starts re-derived from the lengths (QCView's `recomputeContiguousStartTimes`).
    pub fn playlist(rate: Rate, mut clips: Vec<Clip>) -> Self {
        let mut t = 0; for c in clips.iter_mut() { c.start = t; t += c.len_at(rate); }
        Self { rate, clips }
    }
    pub fn duration(&self) -> i64 { self.clips.last().map(|c| c.start + c.len_at(self.rate)).unwrap_or(0) }
    /// Half-open containment: the next clip wins at its boundary (§17.4). Returns (clip index, source frame).
    pub fn resolve(&self, frame: i64) -> Option<(usize, i64)> {
        for (i, c) in self.clips.iter().enumerate().rev() {
            if frame >= c.start && frame < c.start + c.len_at(self.rate) {
                let off = RationalTime::new(frame - c.start, self.rate).rescaled_to(c.src_rate).value;
                return Some((i, c.src_in + off));
            }
        }
        None
    }
    pub fn boundaries(&self) -> Vec<i64> { self.clips.iter().skip(1).map(|c| c.start).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn rescale_round_trips_at_equal_rates() { let r = Rate::from_f64(23.976); let t = RationalTime::new(12345, r); assert_eq!(t.rescaled_to(r), t); assert_eq!(r, Rate { num: 24000, den: 1001 }); }
    #[test] fn exact_rates_from_rationals() { assert_eq!(Rate::from_rational(24000, 1001), Some(Rate::new(24000, 1001))); assert_eq!(Rate::from_rational(0, 1), None); assert_eq!(Rate::from_f64(25.0), Rate::new(25, 1)); assert_eq!(Rate::from_f64(23.5), Rate::new(23500, 1000)); }
    #[test] fn resolve_is_half_open() {
        let r = Rate { num: 24, den: 1 };
        let tl = Timeline::playlist(r, vec![Clip { path: "a".into(), src_rate: r, src_in: 10, src_out: 20, start: 0 }, Clip { path: "b".into(), src_rate: r, src_in: 100, src_out: 105, start: 0 }]);
        assert_eq!(tl.resolve(0), Some((0, 10))); assert_eq!(tl.resolve(9), Some((0, 19))); assert_eq!(tl.resolve(10), Some((1, 100))); assert_eq!(tl.resolve(15), None); assert_eq!(tl.boundaries(), vec![10]); assert_eq!(tl.duration(), 15);
    }
    #[test] fn mixed_rates_snap() { let a = Rate::new(24, 1); let b = Rate::new(30, 1); assert_eq!(RationalTime::new(24, a).rescaled_to(b).value, 30); assert_eq!(RationalTime::new(1, a).rescaled_to(b).value, 1); assert_eq!(RationalTime::new(7, b).rescaled_to(a).value, 6); }
}
