//! QCView's STREAM output (canon row 239, `docs/phase2/step4-stream-card.md`; QCView `src/output/`): the viewport's SDR
//! picture and the mixed audio to a streaming server — built for Louper — as H.264 + AAC, MPEG-TS over SRT or FLV over
//! RTMP. The original's design, for its reasons: a NATIVE hardware session per OS (`vt` VideoToolbox; `mf` Media Foundation,
//! by spike S7's route) that owns the NV12 surfaces, our own GPU conversion writing them in place ([`nv12`]: BT.709,
//! limited range, QCView's maths — no CPU copy, never an encoder's default matrix), FFmpeg only for AAC, the muxers and the
//! protocols ([`mux`]).
pub mod aac;
pub mod mux;
pub mod nv12;
pub mod sink;
#[cfg(target_os = "macos")]
pub mod vt;
#[cfg(windows)]
pub mod mf;

/// QCView `H264Profile`: Baseline is what Louper's recipe asks for; Main where the service takes it. Never B-frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile { Baseline, Main }

/// QCView `EncoderSpec`: `gop_frames` = one second at the clock's rate (Louper: a keyframe at most every 2 s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EncoderSpec { pub width: u32, pub height: u32, pub fps_num: u32, pub fps_den: u32, pub bitrate_kbps: u32, pub gop_frames: u32, pub profile: Profile }

/// One encoded unit for the muxer (QCView `EncodedUnit`): a video access unit (Annex-B, 4-byte start codes, SPS / PPS in
/// band on a keyframe; pts in 90 kHz) or an AAC frame (pts in samples, 1 / 48000).
#[derive(Clone, Debug, Default)]
pub struct EncodedUnit { pub data: Vec<u8>, pub pts: i64, pub dur: i64, pub keyframe: bool, pub audio: bool }

/// The exact 90 kHz timestamp of tick `k` at `num / den` fps (ruling 6: QCView rounded the per-frame duration, so a
/// fractional rate drifted — 59.94 ran ~1.2 s an hour ahead of the audio).
pub fn pts90k(tick: u64, fps_num: u32, fps_den: u32) -> i64 {
    let (n, d) = (fps_num.max(1) as u128, fps_den.max(1) as u128);
    ((tick as u128 * 90000 * d + n / 2) / n) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_are_exact_at_fractional_rates() {
        assert_eq!(pts90k(1, 25, 1), 3600); assert_eq!(pts90k(1000, 25, 1), 3_600_000);
        // 23.976: 3753.75 a frame — four frames are exactly 15015 ticks (QCView's rounded 3754 gave 15016)
        assert_eq!(pts90k(4, 24000, 1001), 15015); assert_eq!(pts90k(1, 24000, 1001), 3754);
        // 59.94: 216 000 frames are exactly 3603.6 s — no drift (QCView's rounded 1502 a frame ran 1.2 s ahead by then)
        assert_eq!(pts90k(216_000, 60000, 1001), 324_324_000);
    }
}
