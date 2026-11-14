//! The leash (canon rows 387-388; spikes S12, S13): this executable started a second time, hidden, with
//! `--leash -- <program> <arguments...>`. It does the two things only a process can do: it OWNS the render's process
//! tree, and it kills that tree the moment the application is gone - told (its standard input closes) or not (the
//! application died: the same pipe closes). It holds no farm logic, reads no file, and speaks to nobody.
//!
//! Windows: the leash puts ITSELF in a job that dies with its last handle, BEFORE it starts anything; whatever it
//! starts is born in the job. If Windows refuses the job, nothing is started (exit 3): a render is never run unfenced.
//! Elsewhere: the renderer is the leader of a new process group, and the group is killed. The group's id is printed
//! first, so that the application can kill it too if the leash itself is killed.
//! The renderer's output passes through untouched: it inherits the leash's standard output and error.
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// The first line of a leash's output on systems with process groups: this prefix, then the group's id.
pub const GROUP_LINE: &str = "\u{1}minRender-leash group ";
/// Exit codes of the leash itself (the renderer's own code is passed on otherwise).
#[cfg_attr(not(windows), allow(dead_code))]
pub const REFUSED: i32 = 3;
pub const NOT_STARTED: i32 = 6; pub const KILLED: i32 = 9;

pub fn run(cmd: &[String]) -> ! {
    if cmd.is_empty() { std::process::exit(NOT_STARTED) }
    let mut c = Command::new(&cmd[0]); c.args(&cmd[1..]).stdin(Stdio::null());
    #[cfg(windows)] let job = { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); os::fence() };
    #[cfg(unix)] { use std::os::unix::process::CommandExt; c.process_group(0); }
    let mut child = match c.spawn() { Ok(c) => c, Err(e) => { eprintln!("could not start {}: {e}", cmd[0]); std::process::exit(NOT_STARTED) } };
    #[cfg(unix)] { println!("{GROUP_LINE}{}", child.id()); let _ = std::io::stdout().flush(); }
    let id = child.id(); let _ = id; let _ = std::io::stdout().flush();
    std::thread::spawn(move || {
        let mut b = [0u8; 64]; let mut s = std::io::stdin(); loop { match s.read(&mut b) { Ok(0) | Err(_) => break, Ok(_) => {} } }
        #[cfg(unix)] kill_group(id);
        #[cfg(windows)] os::kill(job);
        std::process::exit(KILLED) });
    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
    // the renderer has ended by itself: what it started and left behind ends with it. (On Windows the job does this
    // as the leash exits; 0.5.x on macOS left such helpers running after every chunk - seen 2026-11-14.)
    #[cfg(unix)] kill_group(id);
    std::process::exit(code)
}
/// Every process of a group, at once; nothing can catch or ignore it.
#[cfg(unix)]
pub fn kill_group(group: u32) { if group > 1 { unsafe { libc::kill(-(group as i32), libc::SIGKILL); } } }

#[cfg(windows)]
mod os {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::*;
    use windows::Win32::System::Threading::GetCurrentProcess;
    /// The leash enters a job whose processes die when its last handle closes. A refusal ends the leash.
    pub fn fence() -> isize { unsafe {
        let Ok(job) = CreateJobObjectW(None, windows::core::PCWSTR::null()) else { std::process::exit(super::REFUSED) };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default(); info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32).is_err() { std::process::exit(super::REFUSED) }
        if AssignProcessToJobObject(job, GetCurrentProcess()).is_err() { std::process::exit(super::REFUSED) }
        job.0 as isize } }
    pub fn kill(job: isize) { unsafe { let _ = TerminateJobObject(HANDLE(job as *mut _), 1); } }
}
