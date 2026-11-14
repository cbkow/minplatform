//! S12 - the kill matrix. One binary, five roles:
//!   s12 helper   --dir D [--leave] [--secs N]                 a renderer's helper: registers itself, sleeps
//!   s12 renderer --dir D [--early N] [--late N] [--ignore] [--leave] [--secs N]   the synthetic renderer
//!   s12 leash -- <exe> <args...>                              shape B's supervisor: owns the tree, kills it when stdin closes
//!   s12 app --shape today|a|b --dir D [--agent PATH] -- <renderer args...>   a stand-in for the application
//!   s12 matrix [--agent PATH] [--only SHAPE]                   every shape through every scenario; prints the table
//! Every synthetic process writes `<dir>/<pid>.<role>` when it starts: that is how the matrix knows what must die.
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn arg(args: &[String], k: &str) -> Option<String> { args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned()) }
fn flag(args: &[String], k: &str) -> bool { args.iter().any(|a| a == k) }
fn register(dir: &str, role: &str) { let _ = std::fs::write(Path::new(dir).join(format!("{}.{role}", std::process::id())), b""); }
fn me() -> PathBuf { std::env::current_exe().expect("exe") }

// ------------------------------------------------------------------------------------------------ the operating system
#[cfg(unix)]
mod os {
    pub fn alive(pid: u32) -> bool {
        if unsafe { libc::kill(pid as i32, 0) } != 0 { return false }
        // a zombie is dead: it only waits to be collected
        let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output();
        !out.map(|o| String::from_utf8_lossy(&o.stdout).trim_start().starts_with('Z')).unwrap_or(false)
    }
    pub fn kill_one(pid: u32) { unsafe { libc::kill(pid as i32, libc::SIGKILL); } }
    pub fn kill_group(pgid: u32) { unsafe { libc::kill(-(pgid as i32), libc::SIGKILL); } }
    pub fn ignore_signals() { unsafe { for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] { libc::signal(s, libc::SIG_IGN); } } }
    pub fn leave_group() { unsafe { libc::setsid(); } }
}
#[cfg(windows)]
mod os {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::*;
    pub fn alive(pid: u32) -> bool { unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, pid) else { return false };
        let dead = WaitForSingleObject(h, 0) == WAIT_OBJECT_0; let _ = CloseHandle(h); !dead } }
    pub fn kill_one(pid: u32) { unsafe { if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) { let _ = TerminateProcess(h, 1); let _ = CloseHandle(h); } } }
    pub fn ignore_signals() {}
    pub fn leave_group() {}
    pub struct Job(pub HANDLE);
    unsafe impl Send for Job {} unsafe impl Sync for Job {}
    impl Job {
        /// A job whose processes die when its last handle closes.
        pub fn new() -> Job { use windows::Win32::System::JobObjects::*; unsafe {
            let job = CreateJobObjectW(None, windows::core::PCWSTR::null()).expect("job");
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default(); info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32).expect("job limits");
            Job(job) } }
        pub fn take(&self, process: HANDLE) -> bool { unsafe { windows::Win32::System::JobObjects::AssignProcessToJobObject(self.0, process).is_ok() } }
        pub fn take_me(&self) -> bool { self.take(unsafe { GetCurrentProcess() }) }
        pub fn kill(&self) { unsafe { let _ = windows::Win32::System::JobObjects::TerminateJobObject(self.0, 1); } }
    }
    /// Every thread of a process started suspended is let run.
    pub fn resume(pid: u32) { use windows::Win32::System::Diagnostics::ToolHelp::*; unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return };
        let mut e = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        if Thread32First(snap, &mut e).is_ok() { loop {
            if e.th32OwnerProcessID == pid { if let Ok(t) = OpenThread(THREAD_SUSPEND_RESUME, false, e.th32ThreadID) { ResumeThread(t); let _ = CloseHandle(t); } }
            if Thread32Next(snap, &mut e).is_err() { break } } }
        let _ = CloseHandle(snap); } }
}

// ------------------------------------------------------------------------------------------- the synthetic renderer
fn helper(args: &[String]) {
    let dir = arg(args, "--dir").expect("--dir"); if flag(args, "--leave") { os::leave_group(); }
    register(&dir, "h"); std::thread::sleep(Duration::from_secs(arg(args, "--secs").and_then(|s| s.parse().ok()).unwrap_or(600)));
}
fn spawn_helper(dir: &str, leave: bool) {
    let mut c = Command::new(me()); c.args(["helper", "--dir", dir]); if leave { c.arg("--leave"); }
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // (Windows: a helper that tries to break away from its job - refused unless the job allows it; then it is started the ordinary way)
    #[cfg(windows)] if leave { use std::os::windows::process::CommandExt; let mut b = Command::new(me()); b.args(["helper", "--dir", dir, "--leave"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x0100_0000); if b.spawn().is_ok() { return } }
    let _ = c.spawn();
}
fn renderer(args: &[String]) {
    let dir = arg(args, "--dir").expect("--dir"); let n = |k: &str| arg(args, k).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    // the very first thing it does: helpers that exist before a supervisor could have put a fence round the renderer
    for _ in 0..n("--early") { spawn_helper(&dir, false); }
    register(&dir, "r"); if flag(args, "--ignore") { os::ignore_signals(); }
    println!("frame 1 of 10"); let _ = std::io::stdout().flush();
    std::thread::sleep(Duration::from_millis(300));
    for _ in 0..n("--late") { spawn_helper(&dir, flag(args, "--leave")); }
    std::thread::sleep(Duration::from_secs(arg(args, "--secs").and_then(|s| s.parse().ok()).unwrap_or(600)));
}

// --------------------------------------------------------------------------------------------------- shape B's leash
/// Owns the renderer's tree and kills it the moment its own stdin closes (the application went, or said so by closing).
fn leash(args: &[String]) {
    let i = args.iter().position(|a| a == "--").expect("-- <command>"); let cmd = &args[i + 1..];
    let mut c = Command::new(&cmd[0]); c.args(&cmd[1..]).stdin(Stdio::null());
    #[cfg(windows)] let job = { let j = std::sync::Arc::new(os::Job::new()); if !j.take_me() { eprintln!("leash: could not enter its job"); std::process::exit(3); } j };   // the leash is IN the job: whatever it starts is born in it
    #[cfg(unix)] { use std::os::unix::process::CommandExt; c.process_group(0); }
    let mut child = c.spawn().expect("renderer");
    #[cfg(unix)] { println!("LEASH pgid={}", child.id()); let _ = std::io::stdout().flush(); }
    let id = child.id(); let _ = id;
    #[cfg(windows)] let j2 = job.clone();
    std::thread::spawn(move || { let mut b = [0u8; 64]; let mut s = std::io::stdin(); loop { match s.read(&mut b) { Ok(0) | Err(_) => break, Ok(_) => {} } }
        #[cfg(unix)] { os::kill_group(id); std::process::exit(9); }
        #[cfg(windows)] { j2.kill(); std::process::exit(9); } });
    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1); std::process::exit(code);
}

// ---------------------------------------------------------------------------------------- the application's stand-in
fn frame(w: &mut dyn Write, json: &str) { let _ = w.write_all(&(json.len() as u32).to_le_bytes()); let _ = w.write_all(json.as_bytes()); let _ = w.flush(); }
fn task_json(renderer_args: &[String]) -> String {
    let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    format!("{{\"type\":\"task\",\"job_id\":\"s12\",\"frame_start\":1,\"frame_end\":10,\"command\":{{\"executable\":{},\"args\":[{}]}},\"progress\":null,\"output_detection\":null,\"timeout_seconds\":null}}", q(&me().to_string_lossy()), renderer_args.iter().map(|a| q(a)).collect::<Vec<_>>().join(","))
}
#[cfg(windows)]
struct Pipe(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Write for Pipe { fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { let mut n = 0u32; unsafe { windows::Win32::Storage::FileSystem::WriteFile(self.0, Some(b), Some(&mut n), None).map_err(|e| std::io::Error::other(e.to_string()))?; } Ok(n as usize) } fn flush(&mut self) -> std::io::Result<()> { Ok(()) } }

fn app(args: &[String]) {
    let shape = arg(args, "--shape").expect("--shape"); let dir = arg(args, "--dir").expect("--dir");
    let i = args.iter().position(|a| a == "--").expect("-- <renderer args>"); let mut rargs: Vec<String> = vec!["renderer".into(), "--dir".into(), dir.clone()]; rargs.extend(args[i + 1..].iter().cloned());
    let mut cancel: Box<dyn FnMut()>; let sup: u32;
    match shape.as_str() {
        // A: one process - the renderer is the application's own child, fenced by the application
        "a" => {
            let mut c = Command::new(me()); c.args(&rargs).stdin(Stdio::null()).stdout(Stdio::null());
            #[cfg(unix)] { use std::os::unix::process::CommandExt; c.process_group(0); let child = c.spawn().expect("renderer"); let id = child.id(); std::mem::forget(child); cancel = Box::new(move || os::kill_group(id)); }
            #[cfg(windows)] { use std::os::windows::{io::AsRawHandle, process::CommandExt}; c.creation_flags(0x0000_0004);   // started suspended: fenced before it runs a line, then let go
                let child = c.spawn().expect("renderer"); let job = os::Job::new(); if !job.take(windows::Win32::Foundation::HANDLE(child.as_raw_handle() as _)) { eprintln!("app: job refused"); }
                os::resume(child.id()); std::mem::forget(child); cancel = Box::new(move || job.kill()); }
            sup = 0;
        }
        // B: the same executable once more, as a leash
        "b" => {
            let mut c = Command::new(me()); c.arg("leash").arg("--").arg(me()).args(&rargs).stdin(Stdio::piped()).stdout(Stdio::piped());
            let mut child: Child = c.spawn().expect("leash"); sup = child.id(); let stdin = child.stdin.take(); let out = child.stdout.take().unwrap();
            #[allow(unused_mut, unused_assignments, unused_variables)] let mut pgid = 0u32; let mut lines = std::io::BufReader::new(out);
            #[cfg(unix)] { let mut l = String::new(); let _ = lines.read_line(&mut l); pgid = l.trim().strip_prefix("LEASH pgid=").and_then(|x| x.parse().ok()).unwrap_or(0); }
            // the renderer's output is read here, in the application (and the leash's end is the belt to its braces: when the leash goes, the group is killed from here)
            std::thread::spawn(move || { let mut l = String::new(); while lines.read_line(&mut l).map(|n| n > 0).unwrap_or(false) { l.clear(); } });
            // (the leash's END is watched by itself, not through the output's end: a renderer that outlives the leash keeps that pipe open)
            std::thread::spawn(move || { let _ = child.wait(); #[cfg(unix)] if pgid > 0 { os::kill_group(pgid); } });
            let mut held = stdin; cancel = Box::new(move || { held.take(); });
        }
        // today: mr-agent as it ships, spoken to over its own pipe
        _ => {
            let agent = arg(args, "--agent").expect("--agent"); let node = format!("s12-{}", std::process::id()); let home = Path::new(&dir).join("home"); let _ = std::fs::create_dir_all(&home);
            let mut c = Command::new(agent); c.args(["--node-id", &node]).env("USERPROFILE", &home).env("HOME", &home).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());   // (its log goes to the scratch folder, never to the real application's)
            #[cfg(unix)] let mut w: Box<dyn Write + Send> = { let t = std::env::var("TMPDIR").unwrap_or_default(); let t = if t.is_empty() { "/tmp".to_string() } else { t.trim_end_matches('/').to_string() }; let mut p = format!("{t}/minrender-agent-{node}.sock"); if p.len() >= 104 { p = format!("/tmp/minrender-agent-{node}.sock"); }
                let _ = std::fs::remove_file(&p); let l = std::os::unix::net::UnixListener::bind(&p).expect("socket"); let child = c.spawn().expect("mr-agent"); sup = child.id(); std::mem::forget(child); Box::new(l.accept().expect("agent connects").0) };
            #[cfg(windows)] let mut w: Box<dyn Write + Send> = { use windows::Win32::System::Pipes::*; use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
                let name: Vec<u16> = format!("\\\\.\\pipe\\MinRenderAgent_{node}\0").encode_utf16().collect();
                let h = unsafe { CreateNamedPipeW(windows::core::PCWSTR(name.as_ptr()), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 65536, 65536, 0, None) };
                let child = c.spawn().expect("mr-agent"); sup = child.id(); std::mem::forget(child); unsafe { let _ = ConnectNamedPipe(h, None); } struct S(Pipe); unsafe impl Send for S {} impl Write for S { fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.write(b) } fn flush(&mut self) -> std::io::Result<()> { Ok(()) } } Box::new(S(Pipe(h))) };
            frame(&mut *w, &task_json(&rargs));
            cancel = Box::new(move || frame(&mut *w, "{\"type\":\"abort\",\"reason\":\"s12 cancel\"}"));
        }
    }
    // ready once the renderer has registered itself
    let t = Instant::now(); while !std::fs::read_dir(&dir).map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".r"))).unwrap_or(false) { if t.elapsed() > Duration::from_secs(20) { println!("FAILED the renderer never started"); return } std::thread::sleep(Duration::from_millis(10)); }
    println!("READY {sup}"); let _ = std::io::stdout().flush();
    for line in std::io::stdin().lock().lines() { match line.as_deref() { Ok("cancel") => { cancel(); println!("CANCELLED"); let _ = std::io::stdout().flush(); } Ok(_) => {} Err(_) => break } }
}

// ------------------------------------------------------------------------------------------------------- the matrix
fn registered(dir: &Path) -> Vec<u32> { std::fs::read_dir(dir).map(|d| d.flatten().filter_map(|e| { let n = e.file_name().to_string_lossy().into_owned(); let (pid, role) = n.split_once('.')?; matches!(role, "r" | "h").then(|| pid.parse().ok()).flatten() }).collect()).unwrap_or_default() }

fn matrix(args: &[String]) {
    let agent = arg(args, "--agent"); let only = arg(args, "--only");
    let base = std::env::temp_dir().join(format!("s12-{}", std::process::id())); let _ = std::fs::create_dir_all(&base);
    // (what is done, the renderer's switches, how many synthetic processes there are, the longest it is waited for)
    let scenarios: [(&str, &str, &[&str], usize, u64); 6] = [
        ("cancel", "cancel", &["--late", "2"], 3, 5),
        ("cancel, signals ignored", "cancel", &["--late", "2", "--ignore"], 3, 5),
        ("supervisor killed", "kill-sup", &["--late", "2"], 3, 5),
        ("application killed", "kill-app", &["--late", "2"], 3, 120),
        ("helpers before the fence", "cancel", &["--early", "2"], 3, 5),
        ("helpers that leave", "cancel", &["--late", "2", "--leave"], 3, 5),
    ];
    let shapes: Vec<&str> = ["today", "a", "b"].into_iter().filter(|s| only.as_deref().is_none_or(|o| o == *s) && (*s != "today" || agent.is_some())).collect();
    println!("| scenario | shape | alive after 2 s | all gone after |"); println!("|---|---|---|---|");
    for (name, act, rargs, want, cap) in scenarios { for shape in &shapes {
        if act == "kill-sup" && *shape == "a" { println!("| {name} | a | - | (no supervisor) |"); continue }
        let dir = base.join(format!("{shape}-{}", name.replace([' ', ','], "_"))); let _ = std::fs::create_dir_all(&dir);
        let mut c = Command::new(me()); c.args(["app", "--shape", shape, "--dir", &dir.to_string_lossy()]); if let Some(a) = &agent { c.args(["--agent", a]); } c.arg("--").args(rargs.iter());
        let mut app = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("app");
        let mut out = std::io::BufReader::new(app.stdout.take().unwrap()); let mut line = String::new(); let _ = out.read_line(&mut line);
        let sup: u32 = line.trim().strip_prefix("READY ").and_then(|x| x.parse().ok()).unwrap_or(0);
        if !line.starts_with("READY") { println!("| {name} | {shape} | - | did not start: {} |", line.trim()); os::kill_one(app.id()); continue }
        let t = Instant::now(); while registered(&dir).len() < want && t.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(20)); }
        std::thread::sleep(Duration::from_millis(200));
        let pids = registered(&dir); let t0 = Instant::now();
        match act { "cancel" => { let _ = app.stdin.as_mut().unwrap().write_all(b"cancel\n"); let _ = app.stdin.as_mut().unwrap().flush(); } "kill-sup" => os::kill_one(sup), _ => os::kill_one(app.id()) }
        let (mut at2, mut gone) = (None, None);
        while t0.elapsed() < Duration::from_secs(cap) { let alive = pids.iter().filter(|p| os::alive(**p)).count();
            if at2.is_none() && t0.elapsed() >= Duration::from_secs(2) { at2 = Some(alive); }
            if alive == 0 { gone = Some(t0.elapsed()); break } std::thread::sleep(Duration::from_millis(10)); }
        let at2 = at2.unwrap_or(0);
        println!("| {name} | {shape} | {at2} of {} | {} |", pids.len(), match gone { Some(d) => format!("{:.2} s", d.as_secs_f64()), None => format!("never (waited {cap} s)") });
        // nothing of a scenario is left for the next
        drop(app.stdin.take()); for p in registered(&dir) { os::kill_one(p); } if sup > 0 { os::kill_one(sup); } os::kill_one(app.id()); let _ = app.wait();
    } }
    let _ = std::fs::remove_dir_all(&base);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) { Some("helper") => helper(&args), Some("renderer") => renderer(&args), Some("leash") => leash(&args), Some("app") => app(&args), Some("matrix") => matrix(&args),
        _ => eprintln!("s12 helper | renderer | leash | app | matrix   (see the top of src/main.rs)") }
}
