//! A video made by FFmpeg's own program, run as a child (canon rows 340, 343 - minNotes' playable copy; lifted for the
//! family at rows 508 / 513, where UFB's "Transcode to MP4" came to need the same). What is shared is HOW the program
//! is run, not what it is asked for: each application builds its own arguments (minNotes: a copy for a web folder,
//! within 1920, at a bit rate, its turn set to none; UFB: 1.4.2's preset, the full picture, upright).
//!
//! - [`program`]: the family's build of the program - the one the applications decode with - found beside the
//!   application or, for a build run from the source tree, in the tree.
//! - [`run`]: a run that reads its input itself. The caller's arguments ask for `-progress pipe:1`; the frames done
//!   are told as they come. A stop ends the process.
//! - [`run_fed`]: a run whose picture is fed to it frame by frame (frames that went through our colour stage).
//!
//! The errors are sentences for the user; the last line the program said goes with a failure.
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// FFmpeg's program: named by the environment variable `env`, beside the application (on macOS in the bundle's
/// `Helpers`, where QCView's release keeps it), or - a build run from the source tree - in the tree's
/// `external/ffmpeg/<os>/bin`.
pub fn program(env: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(p) = std::env::var_os(env).map(PathBuf::from).filter(|p| p.is_file()) { return Some(p) }
    let me = std::env::current_exe().ok()?; let here = me.parent()?;
    for p in [here.join(exe), here.join("../Helpers").join(exe), here.join("../Frameworks").join(exe), here.join("../Resources").join(exe)] { if p.is_file() { return Some(p) } }
    let os = if cfg!(target_os = "macos") { "macos-arm64" } else if cfg!(windows) { "win64" } else { "linux" };
    here.ancestors().map(|a| a.join("external/ffmpeg").join(os).join("bin").join(exe)).find(|p| p.is_file())
}

fn quiet(c: &mut Command) { #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); } let _ = c; }   // no console window
fn last_line(said: &str) -> String { said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no reason given").trim().chars().take(160).collect() }

/// Run it; it reads its input itself. `frame`: the number of the frame it has reached (from its `-progress` lines -
/// the arguments must ask for them on `pipe:1`). Err("Stopped") when `stop` was set.
pub fn run(mut c: Command, stop: &AtomicBool, frame: &mut dyn FnMut(f32)) -> Result<(), String> {
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()); quiet(&mut c);
    let mut child = c.spawn().map_err(|e| format!("FFmpeg could not be started ({e})"))?;
    // what it says of errors is read beside its progress (a full pipe would hold it)
    let said = child.stderr.take().map(|mut e| std::thread::spawn(move || { let mut s = String::new(); let _ = e.read_to_string(&mut s); s }));
    if let Some(o) = child.stdout.take() {
        for line in BufReader::new(o).lines().map_while(Result::ok) {
            if stop.load(Relaxed) { let _ = child.kill(); break }
            if let Some(n) = line.strip_prefix("frame=").and_then(|v| v.trim().parse::<f32>().ok()) { frame(n); }
        }
    }
    let status = child.wait().map_err(|e| format!("FFmpeg did not end ({e})"))?;
    let said = said.and_then(|t| t.join().ok()).unwrap_or_default();
    if stop.load(Relaxed) { return Err("Stopped".into()) }
    if !status.success() { return Err(format!("The video could not be made: {}", last_line(&said))) }
    Ok(())
}

/// Run it and feed it: `feed` writes the next frame into the program's input and says whether more follow (false:
/// that was all - or the program has ended, and what it said is the reason), or why no frame can be made (the video
/// is then not made). Err("Stopped") when `stop` was set.
pub fn run_fed(mut c: Command, stop: &AtomicBool, feed: &mut dyn FnMut(&mut dyn Write) -> Result<bool, &'static str>) -> Result<(), String> {
    c.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()); quiet(&mut c);
    let mut child = c.spawn().map_err(|e| format!("FFmpeg could not be started ({e})"))?;
    let said = child.stderr.take().map(|mut e| std::thread::spawn(move || { let mut s = String::new(); let _ = e.read_to_string(&mut s); s }));
    let mut pipe = child.stdin.take(); let mut failed = None;
    while let Some(w) = pipe.as_mut() {
        if stop.load(Relaxed) { break }
        match feed(w) { Ok(true) => {} Ok(false) => break, Err(why) => { failed = Some(why); break } }
    }
    drop(pipe);
    if stop.load(Relaxed) || failed.is_some() { let _ = child.kill(); }
    let status = child.wait().map_err(|e| format!("FFmpeg did not end ({e})"))?; let said = said.and_then(|t| t.join().ok()).unwrap_or_default();
    if stop.load(Relaxed) { return Err("Stopped".into()) }
    if let Some(f) = failed { return Err(f.into()) }
    if !status.success() { return Err(format!("The video could not be made: {}", last_line(&said))) }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_last_thing_said_is_the_reason() { assert_eq!(last_line("a\nb: no such file\n\n  \n"), "b: no such file"); assert_eq!(last_line(""), "no reason given"); assert_eq!(last_line(&"x".repeat(500)).len(), 160); }
    #[test]
    fn a_program_that_is_not_there_is_said_and_a_named_one_is_taken() {
        let c = Command::new(if cfg!(windows) { "C:\\no\\such\\ffmpeg.exe" } else { "/no/such/ffmpeg" });
        assert!(run(c, &AtomicBool::new(false), &mut |_| {}).unwrap_err().starts_with("FFmpeg could not be started"));
        let me = std::env::current_exe().unwrap(); unsafe { std::env::set_var("MIN_TRANSCODE_TEST_PROGRAM", &me); }
        assert_eq!(program("MIN_TRANSCODE_TEST_PROGRAM"), Some(me));
    }
}
