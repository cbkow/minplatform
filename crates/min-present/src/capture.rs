//! Capture encodes (canon row 216): linear-light captures (1.0 = SDR white) into sRGB files. An HDR source shown in BYPASS
//! (no chain: PQ / HLG decoded to nits on the screen) is mapped to SDR with BT.2390's EETF on the brightest channel (hue kept),
//! from the source's peak to 100 nits — never raw code values (QCView wrote them, washed out). A chain's output is already
//! SDR-targeted for captures (the caller re-targets it), so it is only clipped.

/// BT.2390-04 EETF in PQ space: a source range [0, src_peak] nits onto [0, dst_peak], the knee at 1.5·dst − 0.5 (normalized).
pub fn bt2390(nits: f32, src_peak: f32, dst_peak: f32) -> f32 {
    if nits <= 0.0 { return 0.0 }
    let pq = |n: f32| { let y = (n / 10000.0).clamp(0.0, 1.0); let (m1, m2, c1, c2, c3) = (0.1593017578125f32, 78.84375f32, 0.8359375f32, 18.8515625f32, 18.6875f32); let p = y.powf(m1); ((c1 + c2 * p) / (1.0 + c3 * p)).powf(m2) };
    let ipq = |e: f32| { let (m1, m2, c1, c2, c3) = (0.1593017578125f32, 78.84375f32, 0.8359375f32, 18.8515625f32, 18.6875f32); let p = e.max(0.0).powf(1.0 / m2); 10000.0 * ((p - c1).max(0.0) / (c2 - c3 * p)).powf(1.0 / m1) };
    let (sp, dp) = (pq(src_peak), pq(dst_peak));
    if src_peak <= dst_peak { return nits.min(dst_peak) }
    let e1 = pq(nits) / sp; let max_l = dp / sp;
    let ks = 1.5 * max_l - 0.5;
    let e2 = if e1 < ks { e1 } else { let t = (e1 - ks) / (1.0 - ks); let (t2, t3) = (t * t, t * t * t); (2.0 * t3 - 3.0 * t2 + 1.0) * ks + (t3 - 2.0 * t2 + t) * (1.0 - ks) + (-2.0 * t3 + 3.0 * t2) * max_l };
    ipq(e2 * sp)
}
fn oetf(v: f32) -> f32 { let x = v.clamp(0.0, 1.0); if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 } }
/// Linear RGB (1.0 = SDR white = 100 nits) → SDR-ready linear RGB: the HDR source's tone map when `hdr_peak` (bypass), else as is.
pub fn to_sdr(px: &mut [f32], hdr_peak: Option<f32>) {
    let Some(peak) = hdr_peak else { return };
    for c in px.chunks_mut(3) {
        let m = c[0].max(c[1]).max(c[2]); if m <= 0.0 { continue }
        let mapped = bt2390(m * 100.0, peak, 100.0) / 100.0;
        let k = mapped / m; c[0] *= k; c[1] *= k; c[2] *= k;
    }
}
pub fn srgb8(px: &[f32]) -> Vec<u8> { px.iter().map(|v| (oetf(*v) * 255.0 + 0.5) as u8).collect() }
pub fn srgb16(px: &[f32]) -> Vec<u16> { px.iter().map(|v| (oetf(*v) * 65535.0 + 0.5) as u16).collect() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bt2390_keeps_the_low_end_and_lands_the_peak() {
        assert!((bt2390(10.0, 1000.0, 100.0) - 10.0).abs() < 0.5, "shadows untouched: {}", bt2390(10.0, 1000.0, 100.0));
        assert!((bt2390(1000.0, 1000.0, 100.0) - 100.0).abs() < 0.5, "the source peak lands on 100: {}", bt2390(1000.0, 1000.0, 100.0));
        assert!(bt2390(500.0, 1000.0, 100.0) < 100.0 && bt2390(500.0, 1000.0, 100.0) > bt2390(200.0, 1000.0, 100.0), "monotonic below the peak");
        assert_eq!(srgb8(&[0.0, 1.0, 0.5])[..2], [0, 255]); assert_eq!(srgb16(&[1.0])[0], 65535);
    }
}
