//! What the columns and the status bar say: a date, a size, a kind (canon row 463).
//!
//! Dates are local: "Today 14:02", "Yesterday 09:15", else `2026-11-15 14:02`. Sizes count as the system's own file
//! browser counts on that system - by 1000 on macOS (Finder), by 1024 on Windows (Explorer). A kind comes from the
//! extension: a small table of the types a production folder holds, else "<EXT> file".
use std::borrow::Cow;

/// A moment as a calendar shows it, in local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil { pub year: i32, pub month: u32, pub day: u32, pub hour: u32, pub minute: u32 }

/// Days since 1970-01-01 of a calendar date (proleptic Gregorian).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 }; let era = y.div_euclid(400); let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m as i64 - 3 } else { m as i64 + 9 }) + 2) / 5 + d as i64 - 1; let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
/// The calendar date and time of `secs` seconds since 1970 in a zone `offset` seconds east of UTC.
pub fn civil_at(secs: i64, offset: i64) -> Civil {
    let t = secs + offset; let days = t.div_euclid(86400); let rem = t.rem_euclid(86400);
    let z = days + 719468; let era = z.div_euclid(146097); let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1; let m = if mp < 10 { mp + 3 } else { mp - 9 }; let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    Civil { year: y as i32, month: m as u32, day: d as u32, hour: (rem / 3600) as u32, minute: (rem % 3600 / 60) as u32 }
}
/// `secs` since 1970 in this machine's local time (the zone's offset as it was AT that moment: summer time is the system's).
pub fn local(secs: i64) -> Civil {
    #[cfg(unix)] { let t = secs as libc::time_t; let mut tm: libc::tm = unsafe { std::mem::zeroed() }; if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() { return civil_at(secs, 0) } civil_at(secs, tm.tm_gmtoff as i64) }
    #[cfg(windows)] { use windows::Win32::Foundation::{FILETIME, SYSTEMTIME}; use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
        let ticks = (secs + 11_644_473_600).max(0) as u64 * 10_000_000; let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 }; let (mut utc, mut loc) = (SYSTEMTIME::default(), SYSTEMTIME::default());
        unsafe { if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc).is_err() { return civil_at(secs, 0) } }
        Civil { year: loc.wYear as i32, month: loc.wMonth as u32, day: loc.wDay as u32, hour: loc.wHour as u32, minute: loc.wMinute as u32 } }
    #[cfg(not(any(unix, windows)))] { civil_at(secs, 0) }
}
/// A date as the list shows it, `today` being now.
pub fn date_text_civil(c: Civil, today: Civil) -> String {
    let (d, t) = (days_from_civil(c.year, c.month, c.day), days_from_civil(today.year, today.month, today.day));
    if d == t { format!("Today {:02}:{:02}", c.hour, c.minute) } else if d + 1 == t { format!("Yesterday {:02}:{:02}", c.hour, c.minute) }
    else { format!("{:04}-{:02}-{:02} {:02}:{:02}", c.year, c.month, c.day, c.hour, c.minute) }
}
/// `secs` (a file's modification time) as the list shows it; `today` = [`local`] of now. A file with no time shows "--".
pub fn date_text(secs: i64, today: Civil) -> String { if secs == i64::MIN { "--".into() } else { date_text_civil(local(secs), today) } }
pub fn now_secs() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }

/// The system's own file browser counts by 1000 (Finder) or by 1024 (Explorer).
pub const DECIMAL: bool = cfg!(target_os = "macos");
/// A size: whole bytes and KB, one decimal for MB, two from GB up. `decimal`: units of 1000, else of 1024.
/// "148 MB", not "148.0 MB": zeros after the point are dropped, as Finder and Explorer do.
fn trim(n: String, unit: &str) -> String { let n = if n.contains('.') { n.trim_end_matches('0').trim_end_matches('.').to_string() } else { n }; format!("{n} {unit}") }

pub fn size_text(bytes: u64, decimal: bool) -> String {
    let k = if decimal { 1000.0 } else { 1024.0 }; let b = bytes as f64;
    if bytes == 1 { "1 byte".into() } else if b < k { format!("{bytes} bytes") }
    else if b < k * k { format!("{:.0} KB", (b / k).max(1.0)) } else if b < k * k * k { trim(format!("{:.1}", b / (k * k)), "MB") }
    else if b < k * k * k * k { trim(format!("{:.2}", b / (k * k * k)), "GB") } else { trim(format!("{:.2}", b / (k * k * k * k)), "TB") }
}
/// The room left on a volume, as the status bar says it: whole units from GB up are enough there.
pub fn free_text(bytes: u64, decimal: bool) -> String {
    let k = if decimal { 1000.0 } else { 1024.0 }; let b = bytes as f64;
    if b >= k * k * k * k { format!("{:.2} TB available", b / (k * k * k * k)) } else if b >= k * k * k * 10.0 { format!("{:.0} GB available", b / (k * k * k)) } else { format!("{} available", size_text(bytes, decimal)) }
}

/// The types a production folder holds. Extensions in lower case.
const KINDS: &[(&[&str], &str)] = &[
    (&["jpg", "jpeg"], "JPEG image"), (&["png"], "PNG image"), (&["tif", "tiff"], "TIFF image"), (&["exr"], "OpenEXR image"), (&["dpx"], "DPX image"), (&["tga"], "Targa image"), (&["psd"], "Photoshop document"), (&["psb"], "Photoshop large document"),
    (&["gif"], "GIF image"), (&["bmp"], "BMP image"), (&["webp"], "WebP image"), (&["heic", "heif"], "HEIF image"), (&["hdr"], "Radiance HDR image"), (&["svg"], "SVG image"), (&["ai"], "Illustrator document"), (&["eps"], "EPS image"),
    (&["cr2", "cr3", "nef", "arw", "dng", "raf"], "Camera raw image"),
    (&["mov"], "QuickTime movie"), (&["mp4", "m4v"], "MPEG-4 movie"), (&["mxf"], "MXF movie"), (&["avi"], "AVI movie"), (&["mkv"], "Matroska movie"), (&["webm"], "WebM movie"), (&["r3d"], "REDCODE movie"), (&["braw"], "Blackmagic RAW movie"),
    (&["wav"], "WAV audio"), (&["aif", "aiff"], "AIFF audio"), (&["mp3"], "MP3 audio"), (&["flac"], "FLAC audio"), (&["m4a", "aac"], "AAC audio"), (&["ogg"], "Ogg audio"),
    (&["blend"], "Blender file"), (&["blend1"], "Blender backup"), (&["aep"], "After Effects project"), (&["aet"], "After Effects template"), (&["prproj"], "Premiere Pro project"), (&["drp"], "DaVinci Resolve project"),
    (&["nk"], "Nuke script"), (&["hip", "hiplc", "hipnc"], "Houdini scene"), (&["ma", "mb"], "Maya scene"), (&["c4d"], "Cinema 4D scene"), (&["max"], "3ds Max scene"), (&["spp"], "Substance Painter project"),
    (&["fbx"], "FBX model"), (&["obj"], "OBJ model"), (&["abc"], "Alembic cache"), (&["usd", "usda", "usdc", "usdz"], "USD scene"), (&["gltf", "glb"], "glTF model"), (&["vdb"], "OpenVDB volume"),
    (&["cube", "3dl"], "Colour lookup table"), (&["ocio"], "OpenColorIO configuration"), (&["edl"], "Edit decision list"), (&["otio"], "OpenTimelineIO timeline"), (&["srt"], "Subtitles"),
    (&["pdf"], "PDF document"), (&["txt"], "Plain text"), (&["md"], "Markdown document"), (&["rtf"], "Rich text document"), (&["doc", "docx"], "Word document"), (&["xls", "xlsx"], "Excel workbook"), (&["ppt", "pptx"], "PowerPoint presentation"), (&["csv"], "CSV table"),
    (&["json"], "JSON document"), (&["xml"], "XML document"), (&["toml"], "TOML document"), (&["yaml", "yml"], "YAML document"), (&["html", "htm"], "HTML document"), (&["py"], "Python script"), (&["sh"], "Shell script"), (&["log"], "Log file"),
    (&["zip"], "ZIP archive"), (&["7z"], "7-Zip archive"), (&["rar"], "RAR archive"), (&["tar"], "Tar archive"), (&["gz", "tgz"], "Gzip archive"), (&["dmg"], "Disk image"), (&["iso"], "Disk image"),
    (&["ttf", "otf"], "Font"), (&["app"], "Application"), (&["exe"], "Application"), (&["lnk"], "Shortcut"),
];
/// The extension of a name, in lower case: what follows the last dot, when something stands before it.
pub fn extension(name: &str) -> Option<String> { let i = name.rfind('.')?; if i == 0 || i + 1 == name.len() { return None } Some(name[i + 1..].to_lowercase()) }
/// What a file is called by its extension (`ext` in lower case, none for a name without one).
pub fn kind_of_extension(ext: Option<&str>) -> Cow<'static, str> {
    let Some(ext) = ext else { return Cow::Borrowed("File") };
    match KINDS.iter().find(|(exts, _)| exts.contains(&ext)) { Some((_, kind)) => Cow::Borrowed(kind), None => Cow::Owned(format!("{} file", ext.to_uppercase())) }
}
/// What a package (a folder the system shows as one item) is called: the table's word for its extension, else "Package".
pub fn kind_of_package(ext: &str) -> Cow<'static, str> { match KINDS.iter().find(|(exts, _)| exts.contains(&ext)) { Some((_, kind)) => Cow::Borrowed(kind), None => Cow::Borrowed("Package") } }
#[cfg(test)]
pub fn kind_text(name: &str, dir: bool) -> Cow<'static, str> { if dir { Cow::Borrowed("Folder") } else { kind_of_extension(extension(name).as_deref()) } }

#[cfg(test)]
mod tests {
    use super::*;
    fn c(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Civil { Civil { year, month, day, hour, minute } }
    #[test]
    fn dates_say_today_yesterday_or_the_day() {
        let today = c(2026, 11, 15, 16, 30);
        assert_eq!(date_text_civil(c(2026, 11, 15, 14, 2), today), "Today 14:02");
        assert_eq!(date_text_civil(c(2026, 11, 15, 23, 59), today), "Today 23:59", "later today is still today");
        assert_eq!(date_text_civil(c(2026, 11, 14, 9, 15), today), "Yesterday 09:15");
        assert_eq!(date_text_civil(c(2026, 11, 13, 9, 15), today), "2026-11-13 09:15");
        assert_eq!(date_text_civil(c(2026, 11, 16, 0, 5), today), "2026-11-16 00:05", "tomorrow is a plain date");
        assert_eq!(date_text_civil(c(2025, 12, 31, 23, 0), c(2026, 1, 1, 0, 10)), "Yesterday 23:00", "across a year's end");
        assert_eq!(date_text_civil(c(2024, 2, 29, 7, 0), c(2024, 3, 1, 7, 0)), "Yesterday 07:00", "across a leap day");
        assert_eq!(date_text(i64::MIN, today), "--");
    }
    #[test]
    fn the_calendar_arithmetic() {
        assert_eq!(days_from_civil(1970, 1, 1), 0); assert_eq!(days_from_civil(2000, 3, 1), 11017); assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(civil_at(0, 0), c(1970, 1, 1, 0, 0)); assert_eq!(civil_at(1_794_751_320, 0), c(2026, 11, 15, 14, 2));
        assert_eq!(civil_at(1_794_751_320, 3600), c(2026, 11, 15, 15, 2)); assert_eq!(civil_at(1_794_751_320, -15 * 3600), c(2026, 11, 14, 23, 2), "a zone west of UTC is a day behind");
        assert_eq!(civil_at(951_782_400, 0), c(2000, 2, 29, 0, 0)); assert_eq!(civil_at(-1, 0), c(1969, 12, 31, 23, 59));
        for (y, m, d) in [(1970, 1, 1), (1999, 12, 31), (2024, 2, 29), (2026, 11, 15), (2100, 3, 1)] { let k = civil_at(days_from_civil(y, m, d) * 86400, 0); assert_eq!((k.year, k.month, k.day), (y, m, d)); }
        // the local zone is the system's: whatever it is, a moment and the same moment a minute later differ by a minute
        let (a, b) = (local(1_794_751_320), local(1_794_751_380)); assert_eq!((b.minute + 60 - a.minute) % 60, 1);
    }
    #[test]
    fn sizes_by_1000_as_finder_and_by_1024_as_explorer() {
        assert_eq!(size_text(0, true), "0 bytes"); assert_eq!(size_text(1, true), "1 byte"); assert_eq!(size_text(999, true), "999 bytes");
        assert_eq!(size_text(1000, true), "1 KB"); assert_eq!(size_text(1023, false), "1023 bytes"); assert_eq!(size_text(1024, false), "1 KB");
        assert_eq!(size_text(1_200_000, true), "1.2 MB"); assert_eq!(size_text(1_200_000, false), "1.1 MB"); assert_eq!(size_text(1_048_576, false), "1 MB");
        assert_eq!(size_text(999_999, true), "1000 KB"); assert_eq!(size_text(2_500_000_000, true), "2.5 GB"); assert_eq!(size_text(2_500_000_000, false), "2.33 GB");
        assert_eq!(size_text(3_000_000_000_000, true), "3 TB"); assert_eq!(size_text(1 << 40, false), "1 TB");
        assert_eq!(free_text(120_400_000_000, true), "120 GB available"); assert_eq!(free_text(4_200_000_000, true), "4.2 GB available"); assert_eq!(free_text(2_000_000_000_000, true), "2.00 TB available"); assert_eq!(free_text(512 << 20, false), "512 MB available");
    }
    #[test]
    fn kinds_come_from_the_extension() {
        for (name, kind) in [("plate.JPG", "JPEG image"), ("shot_010_comp_v003.exr", "OpenEXR image"), ("model.blend", "Blender file"), ("title.aep", "After Effects project"), ("plate.mov", "QuickTime movie"), ("notes.pdf", "PDF document"),
            ("cache.xyz", "XYZ file"), ("archive.tar.gz", "Gzip archive"), ("README", "File"), (".gitignore", "File"), ("trailing.", "File"), ("Señal.MXF", "MXF movie")] { assert_eq!(kind_text(name, false), kind, "{name}"); }
        assert_eq!(kind_text("renders.exr", true), "Folder", "a folder is a folder whatever its name");
        assert_eq!(extension("a.B.Cd"), Some("cd".into())); assert_eq!(extension(".hidden"), None); assert_eq!(extension("noext"), None);
        // no extension twice in the table
        let mut seen = std::collections::HashSet::new(); for (exts, _) in KINDS { for e in *exts { assert!(seen.insert(*e), "{e} twice"); } }
    }
}
