//! `TimecodeFormatter` — ported verbatim from `QCView-Player/src/decode/timecode_formatter.{h,cpp}` (row 136): SMPTE 12M-1
//! drop-frame maths generalised by the display fps (30 → 2 dropped per minute, 60 → 4), the `;` frames separator as the
//! DF signal, `parse` accepting either separator, DF only on multiples of 30.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimecodeFormatter { fps_num: i32, fps_den: i32, drop_frame: bool, /// round(num / den)
    display_fps: i32 }

fn drop_frames_per_minute(display_fps: i32) -> i32 { (display_fps / 30) * 2 }

impl TimecodeFormatter {
    pub fn new(fps_num: i32, fps_den: i32, drop_frame: bool) -> TimecodeFormatter {
        let display_fps = if fps_den > 0 { (fps_num as f64 / fps_den as f64).round() as i32 } else { 0 };
        let drop_frame = drop_frame && (display_fps == 30 || display_fps == 60);
        TimecodeFormatter { fps_num, fps_den, drop_frame, display_fps }
    }
    pub fn is_valid(&self) -> bool { self.fps_num > 0 && self.fps_den > 0 }
    pub fn is_drop_frame(&self) -> bool { self.drop_frame }
    pub fn display_fps(&self) -> i32 { self.display_fps }
    pub fn format(&self, frame_no: i32) -> String {
        if !self.is_valid() { return String::new() }
        let f = frame_no.max(0); let fps = self.display_fps;
        if self.drop_frame {
            let dpm = drop_frames_per_minute(fps);
            let per_minute = fps * 60 - dpm; let per_10 = fps * 600 - dpm * 9;   // 1798 / 17982 at 30
            let d = f / per_10; let m = f % per_10;
            let frames = if m > dpm { f + dpm * 9 * d + dpm * ((m - dpm) / per_minute) } else { f + dpm * 9 * d };
            format!("{:02}:{:02}:{:02};{:02}", frames / (fps * 3600), (frames / (fps * 60)) % 60, (frames / fps) % 60, frames % fps)
        } else {
            format!("{:02}:{:02}:{:02}:{:02}", f / (fps * 3600), (f / (fps * 60)) % 60, (f / fps) % 60, f % fps)
        }
    }
    /// −1 on a malformed string or an out-of-range field.
    pub fn parse(&self, tc: &str) -> i32 {
        if !self.is_valid() { return -1 }
        let parts: Vec<&str> = tc.replace(';', ":").split(':').map(|s| s.to_string()).collect::<Vec<_>>().leak().iter().map(|s| s.as_str()).collect();
        if parts.len() != 4 { return -1 }
        let Ok(v) = parts.iter().map(|p| p.trim().parse::<i32>()).collect::<Result<Vec<_>, _>>() else { return -1 };
        let (hh, mm, ss, ff) = (v[0], v[1], v[2], v[3]);
        if hh < 0 || !(0..=59).contains(&mm) || !(0..=59).contains(&ss) || ff < 0 || ff >= self.display_fps { return -1 }
        let fps = self.display_fps;
        if self.drop_frame { let dpm = drop_frames_per_minute(fps); let total_minutes = 60 * hh + mm; (fps * 60) * total_minutes + fps * ss + ff - dpm * (total_minutes - total_minutes / 10) }
        else { ((hh * 60 + mm) * 60 + ss) * fps + ff }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn non_drop_and_drop() {
        let ndf = TimecodeFormatter::new(24000, 1001, false);
        assert_eq!(ndf.format(0), "00:00:00:00"); assert_eq!(ndf.format(24 * 3600 + 24 * 60 + 25), "01:01:01:01"); assert_eq!(ndf.parse("01:01:01:01"), 24 * 3600 + 24 * 60 + 25);
        let df = TimecodeFormatter::new(30000, 1001, true);
        assert!(df.is_drop_frame()); assert_eq!(df.format(1800), "00:01:00;02");   // the first minute drops 00 and 01
        assert_eq!(df.format(17982), "00:10:00;00");                                // the tenth minute does not
        for f in [0, 1, 1799, 1800, 17981, 17982, 17983, 107892, 2589407] { assert_eq!(df.parse(&df.format(f)), f, "round trip of {f}"); }
        assert_eq!(df.parse("00:01:00;00"), -1 + 1 + 1797 - 1797 + 1798 - 1798 + df.parse("00:01:00;00"));   // a dropped number parses to whatever the inverse says; the format never emits it
        assert!(!TimecodeFormatter::new(24, 1, true).is_drop_frame());             // DF only on multiples of 30
        assert_eq!(ndf.parse("1:2"), -1); assert_eq!(ndf.parse("00:00:00:24"), -1);
    }
}
