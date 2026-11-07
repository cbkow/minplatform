//! `FractionalResampler` (QCView `audio/fractional_resampler.h`), verbatim: Catmull-Rom over interleaved stereo with a
//! FIVE-frame history (2 past, base, 2 look-ahead) carried across blocks — the three-frame version lost one sample per
//! block, a 100 Hz crackle. `ratio` = source frames per output frame (the servo's ±0.2 %, the shuttle's varispeed).
pub struct FractionalResampler { hist: [f32; HIST * 2], hist2: [f32; HIST * 2], frac: f64 }
const HIST: usize = 5;
const BASE: i64 = 2;

impl Default for FractionalResampler { fn default() -> Self { Self::new() } }
impl FractionalResampler {
    pub fn new() -> FractionalResampler { FractionalResampler { hist: [0.0; HIST * 2], hist2: [0.0; HIST * 2], frac: 0.0 } }
    pub fn reset(&mut self) { self.hist = [0.0; HIST * 2]; self.frac = 0.0; }
    fn clamp_ratio(r: f64) -> f64 { r.clamp(0.05, 64.0) }
    /// Source frames `process` will consume for `dst_frames` at `ratio`.
    pub fn source_frames_needed(&self, dst_frames: usize, ratio: f64) -> usize {
        if dst_frames == 0 { return 0 }
        let r = Self::clamp_ratio(ratio);
        let new_base = (BASE as f64 + self.frac + dst_frames as f64 * r).floor() as i64;
        if new_base > BASE { (new_base - BASE) as usize } else { 0 }
    }
    fn sample_at(&self, src: &[f32], src_frames: usize, i: i64, ch: usize) -> f32 {
        let i = i.max(0);
        if (i as usize) < HIST { return self.hist[i as usize * 2 + ch] }
        let s = i as usize - HIST;
        if s >= src_frames { return if src_frames > 0 { src[(src_frames - 1) * 2 + ch] } else { self.hist[(HIST - 1) * 2 + ch] } }
        src[s * 2 + ch]
    }
    fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, f: f32) -> f32 {
        let f2 = f * f; let f3 = f2 * f;
        0.5 * ((2.0 * p1) + (-p0 + p2) * f + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * f3)
    }
    /// `src` holds `src_frames` stereo frames (≥ `source_frames_needed`); fills `dst_frames` into `dst`. Returns the source frames consumed.
    pub fn process(&mut self, src: &[f32], src_frames: usize, dst: &mut [f32], dst_frames: usize, ratio: f64) -> usize {
        if dst_frames == 0 { return 0 }
        let r = Self::clamp_ratio(ratio);
        for k in 0..dst_frames {
            let pos = BASE as f64 + self.frac + k as f64 * r;
            let i = pos.floor() as i64; let f = (pos - i as f64) as f32;
            for ch in 0..2 {
                let (p0, p1, p2, p3) = (self.sample_at(src, src_frames, i - 1, ch), self.sample_at(src, src_frames, i, ch), self.sample_at(src, src_frames, i + 1, ch), self.sample_at(src, src_frames, i + 2, ch));
                dst[k * 2 + ch] = Self::catmull_rom(p0, p1, p2, p3, f);
            }
        }
        let new_pos = BASE as f64 + self.frac + dst_frames as f64 * r;
        let new_base = new_pos.floor() as i64;
        self.frac = new_pos - new_base as f64;
        for j in 0..HIST { let idx = new_base - 2 + j as i64; self.hist2[j * 2] = self.sample_at(src, src_frames, idx, 0); self.hist2[j * 2 + 1] = self.sample_at(src, src_frames, idx, 1); }
        self.hist = self.hist2;
        if new_base > BASE { (new_base - BASE) as usize } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unity_ratio_passes_a_ramp_through_and_loses_no_samples_across_blocks() {
        let mut rs = FractionalResampler::new();
        // a ramp 0,1,2,… on both channels, fed in 7-frame blocks at ratio 1.0; after the 2-frame look-ahead settles the output is the ramp delayed by 2
        let src: Vec<f32> = (0..200).flat_map(|i| [i as f32, i as f32]).collect();
        let mut out = vec![]; let mut consumed = 0usize;
        while consumed + 7 <= 200 {
            let need = rs.source_frames_needed(7, 1.0); assert_eq!(need, 7);
            let mut dst = vec![0.0; 14];
            let used = rs.process(&src[consumed * 2..(consumed + 7) * 2], 7, &mut dst, 7, 1.0);
            assert_eq!(used, 7); consumed += 7; out.extend(dst.chunks(2).map(|c| c[0]));
        }
        // from the fourth block on, each output frame k equals k − 3 (the base sits 3 frames behind the newest history frame) with no drift — the lost-sample bug would show as a growing offset
        for (k, v) in out.iter().enumerate().skip(28) { assert!((v - (k as f32 - 3.0)).abs() < 1e-3, "frame {k}: {v}"); }
    }
    #[test]
    fn ratio_two_consumes_twice_the_frames() {
        let mut rs = FractionalResampler::new();
        assert_eq!(rs.source_frames_needed(100, 2.0), 200);
        assert_eq!(rs.source_frames_needed(100, 0.5), 50);
        let src = vec![0.5f32; 400]; let mut dst = vec![0.0; 200];
        assert_eq!(rs.process(&src, 200, &mut dst, 100, 2.0), 200);
        assert!(dst[150..].iter().all(|v| (v - 0.5).abs() < 1e-5), "a constant stays constant once the history is primed");
    }
}
