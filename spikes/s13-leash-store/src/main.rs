//! S13 - the leash as Windows sees it. One WINDOWED binary (no console, as the application will be), five roles:
//!   s13 soak --chunks N --kills M --out DIR    the application's stand-in: N chunks cancelled, then M times an
//!                                              application holding a chunk is killed outright; writes DIR/result.txt
//!   s13 holder --dir D                         an application that starts one chunk and waits to be killed
//!   s13 leash -- <exe> <args...>               the leash: enters a kill-on-close job, starts the renderer, kills on stdin's end
//!   s13 renderer --dir D                       a synthetic renderer: registers itself, starts two helpers, sleeps
//!   s13 helper --dir D                         a helper: registers itself, sleeps
//! Every relaunch is hidden (CREATE_NO_WINDOW). Every synthetic process writes `<dir>/<pid>.<role>` when it starts.
//! Nothing here is the product's code.
#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(not(windows))]
fn main() { eprintln!("S13 asks a question about Windows only"); }

#[cfg(windows)]
fn main() { win::main() }

#[cfg(windows)]
mod win {
    use std::io::{Read, Write};
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::JobObjects::*;
    use windows::Win32::System::Threading::*;
    const HIDDEN: u32 = 0x0800_0000; // CREATE_NO_WINDOW

    fn arg(args: &[String], k: &str) -> Option<String> { args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned()) }
    fn register(dir: &str, role: &str) { let _ = std::fs::write(Path::new(dir).join(format!("{}.{role}", std::process::id())), b""); }
    fn me() -> PathBuf { std::env::current_exe().expect("exe") }
    fn alive(pid: u32) -> bool { unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, pid) else { return false };
        let dead = WaitForSingleObject(h, 0) == WAIT_OBJECT_0; let _ = CloseHandle(h); !dead } }
    fn kill_one(pid: u32) { unsafe { if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) { let _ = TerminateProcess(h, 1); let _ = CloseHandle(h); } } }
    fn registered(dir: &Path) -> Vec<u32> { std::fs::read_dir(dir).map(|d| d.flatten().filter_map(|e| e.file_name().to_string_lossy().split_once('.').and_then(|(p, _)| p.parse().ok())).collect()).unwrap_or_default() }
    /// Is this process already in a job (a packaged application is: its container's)?
    fn in_job() -> bool { let mut r = windows::core::BOOL(0); unsafe { let _ = IsProcessInJob(GetCurrentProcess(), None, &mut r); } r.as_bool() }
    /// The package's full name, when the program runs with a package's identity.
    fn package() -> Option<String> { use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
        let mut n = 0u32; unsafe { let _ = GetCurrentPackageFullName(&mut n, None); } if n == 0 { return None }
        let mut b = vec![0u16; n as usize]; let r = unsafe { GetCurrentPackageFullName(&mut n, Some(windows::core::PWSTR(b.as_mut_ptr()))) };
        (r.0 == 0).then(|| String::from_utf16_lossy(&b[..(n as usize).saturating_sub(1)])) }

    fn helper(args: &[String]) { register(&arg(args, "--dir").unwrap(), "helper"); std::thread::sleep(Duration::from_secs(600)); }
    fn renderer(args: &[String]) {
        let dir = arg(args, "--dir").unwrap(); register(&dir, "renderer");
        for _ in 0..2 { let _ = Command::new(me()).args(["helper", "--dir", &dir]).creation_flags(HIDDEN).spawn(); }
        std::thread::sleep(Duration::from_secs(600));
    }
    fn leash(args: &[String]) {
        let i = args.iter().position(|a| a == "--").expect("-- <command>"); let cmd = &args[i + 1..];
        let job = unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }.unwrap_or_else(|_| std::process::exit(4));
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default(); info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe { SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32) }.is_err() { std::process::exit(5) }
        // the leash is IN the job before it starts anything: whatever it starts is born there. Exit code 3 = Windows refused.
        if unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) }.is_err() { std::process::exit(3) }
        let mut child = Command::new(&cmd[0]).args(&cmd[1..]).stdin(Stdio::null()).creation_flags(HIDDEN).spawn().unwrap_or_else(|_| std::process::exit(6));
        let j = job.0 as isize;
        std::thread::spawn(move || { let mut b = [0u8; 64]; let mut s = std::io::stdin(); loop { match s.read(&mut b) { Ok(0) | Err(_) => break, Ok(_) => {} } }
            unsafe { let _ = TerminateJobObject(HANDLE(j as *mut _), 1); } std::process::exit(9) });
        std::process::exit(child.wait().ok().and_then(|s| s.code()).unwrap_or(1));
    }
    fn chunk(dir: &Path) -> Child {
        let _ = std::fs::create_dir_all(dir); let d = dir.to_string_lossy().into_owned(); let exe = me().to_string_lossy().into_owned();
        Command::new(me()).args(["leash", "--", &exe, "renderer", "--dir", &d]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(HIDDEN).spawn().expect("leash")
    }
    /// Waits until the three synthetic processes are there (or the leash has given up); their ids.
    fn started(dir: &Path, leash: &mut Child) -> Vec<u32> {
        let t = Instant::now(); while registered(dir).len() < 3 && t.elapsed() < Duration::from_secs(10) && leash.try_wait().ok().flatten().is_none() { std::thread::sleep(Duration::from_millis(10)); }
        std::thread::sleep(Duration::from_millis(100)); registered(dir)
    }
    /// How long until none of them is alive (None: some still were after 2 s), and how many were alive then.
    fn gone(pids: &[u32]) -> (Option<Duration>, usize) {
        let t = Instant::now(); loop { let n = pids.iter().filter(|p| alive(**p)).count(); if n == 0 { return (Some(t.elapsed()), 0) } if t.elapsed() > Duration::from_secs(2) { return (None, n) } std::thread::sleep(Duration::from_millis(5)); }
    }
    fn holder(args: &[String]) { let dir = PathBuf::from(arg(args, "--dir").unwrap()); let _leash = chunk(&dir); std::thread::sleep(Duration::from_secs(600)); }

    fn soak(args: &[String]) {
        let n: usize = arg(args, "--chunks").and_then(|x| x.parse().ok()).unwrap_or(300); let m: usize = arg(args, "--kills").and_then(|x| x.parse().ok()).unwrap_or(20);
        let out = PathBuf::from(arg(args, "--out").unwrap_or_else(|| std::env::var("USERPROFILE").map(|u| format!("{u}\\s13-out")).unwrap_or_else(|_| "s13-out".into())));
        let _ = std::fs::create_dir_all(&out); let work = out.join(format!("work-{}", std::process::id()));
        let mut log = std::fs::File::create(out.join("result.txt")).expect("result file");
        let _ = writeln!(log, "package: {}", package().unwrap_or_else(|| "none".into())); let _ = writeln!(log, "application already in a job: {}", in_job()); let _ = writeln!(log, "exe: {}", me().display());
        let (mut short, mut refused, mut survivors, mut worst) = (0usize, 0usize, 0usize, Duration::ZERO);
        for i in 0..n {
            let dir = work.join(format!("c{i}")); let mut l = chunk(&dir); let pids = started(&dir, &mut l);
            if pids.len() < 3 { short += 1; if l.try_wait().ok().flatten().and_then(|s| s.code()) == Some(3) { refused += 1; } }
            drop(l.stdin.take());   // the cancel: the pipe closes
            let (t, left) = gone(&pids); survivors += left; if let Some(t) = t { worst = worst.max(t); }
            for p in &pids { kill_one(*p); } let _ = l.kill(); let _ = l.wait();
        }
        let _ = writeln!(log, "cancel: {n} chunks; {short} did not start whole ({refused} refused the job); {survivors} processes alive after 2 s; slowest kill {:.0} ms", worst.as_secs_f64() * 1e3);
        let (mut short, mut survivors, mut worst) = (0usize, 0usize, Duration::ZERO);
        for i in 0..m {
            let dir = work.join(format!("k{i}")); let _ = std::fs::create_dir_all(&dir);
            let mut h = Command::new(me()).args(["holder", "--dir", &dir.to_string_lossy()]).creation_flags(HIDDEN).spawn().expect("holder");
            let t = Instant::now(); while registered(&dir).len() < 3 && t.elapsed() < Duration::from_secs(10) { std::thread::sleep(Duration::from_millis(10)); }
            std::thread::sleep(Duration::from_millis(100)); let pids = registered(&dir); if pids.len() < 3 { short += 1; }
            kill_one(h.id());   // the application dies; nobody tells the leash
            let (t, left) = gone(&pids); survivors += left; if let Some(t) = t { worst = worst.max(t); }
            for p in &pids { kill_one(*p); } let _ = h.wait();
        }
        let _ = writeln!(log, "application killed: {m} times; {short} did not start whole; {survivors} processes alive after 2 s; slowest kill {:.0} ms", worst.as_secs_f64() * 1e3);
        let _ = std::fs::remove_dir_all(&work); let _ = writeln!(log, "done");
    }
    pub fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(|s| s.as_str()) { Some("helper") => helper(&args), Some("renderer") => renderer(&args), Some("leash") => leash(&args), Some("holder") => holder(&args), _ => soak(&args) }
    }
}
