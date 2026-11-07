//! `AudioSyncServo` (QCView `audio/audio_sync_servo.h`), verbatim: drift → consumption-ratio PI controller. Inside
//! ±20 ms the trim stays at the inaudible ±0.2 % (≈ 3.5 cents); the cap opens linearly to ±1.5 % at 100 ms so a
//! mid-band error is pulled back in seconds instead of being handed to the re-seek tier; anti-windup on the integral;
//! the ratio slews ≤ 0.001 per update. The caller keeps the escape tiers (this never seeks).
pub struct AudioSyncServo { integral: f64, ratio: f64 }

const K_P: f64 = 0.15;            // per second
const K_I: f64 = 0.01;            // per second²
const MIN_TRIM: f64 = 0.002;      // ±0.2 % (quiet)
const MAX_TRIM: f64 = 0.015;      // ±1.5 % (catch-up)
const QUIET_BAND: f64 = 0.020;    // s
const FULL_BAND: f64 = 0.100;     // s
const MAX_SLEW_PER_UPDATE: f64 = 0.001;

impl Default for AudioSyncServo { fn default() -> Self { Self::new() } }
impl AudioSyncServo {
    pub fn new() -> AudioSyncServo { AudioSyncServo { integral: 0.0, ratio: 1.0 } }
    fn clamp_trim(t: f64, cap: f64) -> f64 { t.clamp(-cap, cap) }
    fn max_trim_for(abs_drift: f64) -> f64 {
        if abs_drift <= QUIET_BAND { return MIN_TRIM } if abs_drift >= FULL_BAND { return MAX_TRIM }
        MIN_TRIM + (abs_drift - QUIET_BAND) / (FULL_BAND - QUIET_BAND) * (MAX_TRIM - MIN_TRIM)
    }
    /// `drift` = master target − audio playout position (positive = audio behind, consume faster); `dt` since the last update.
    pub fn update(&mut self, drift: f64, dt: f64) -> f64 {
        let dt = if dt <= 0.0 || dt > 0.5 { 1.0 / 30.0 } else { dt };
        self.integral += drift * dt * K_I;
        self.integral = Self::clamp_trim(self.integral, MIN_TRIM);
        let cap = Self::max_trim_for(drift.abs());
        let trim = Self::clamp_trim(K_P * drift + self.integral, cap);
        let target = 1.0 + trim;
        let delta = (target - self.ratio).clamp(-MAX_SLEW_PER_UPDATE, MAX_SLEW_PER_UPDATE);
        self.ratio += delta;
        self.ratio
    }
    pub fn reset(&mut self) { self.integral = 0.0; self.ratio = 1.0; }
    pub fn ratio(&self) -> f64 { self.ratio }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trims_within_the_caps_and_slews() {
        let mut s = AudioSyncServo::new();
        let r1 = s.update(0.010, 1.0 / 30.0);   // 10 ms behind: inside the quiet band
        assert!(r1 > 1.0 && r1 <= 1.0 + MIN_TRIM + 1e-9, "{r1}");
        let mut r = r1; for _ in 0..200 { r = s.update(0.010, 1.0 / 30.0); }
        assert!((r - (1.0 + MIN_TRIM)).abs() < 1e-6, "saturates at the quiet cap: {r}");
        s.reset();
        let mut r = 1.0; for _ in 0..200 { r = s.update(0.090, 1.0 / 30.0); }
        assert!(r > 1.0 + MIN_TRIM && r <= 1.0 + MAX_TRIM + 1e-9, "a 90 ms error opens the cap toward 1.5 %: {r}");
        s.reset();
        let a = s.update(-0.5, 1.0 / 30.0); let b = s.update(-0.5, 1.0 / 30.0);
        assert!((1.0 - a - MAX_SLEW_PER_UPDATE).abs() < 1e-9 && (a - b - MAX_SLEW_PER_UPDATE).abs() < 1e-9, "slew-limited: {a} {b}");
    }
}
