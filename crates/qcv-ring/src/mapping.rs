//! S4 — the shared mapping (vendor/qcbae `ring_mapping`): POSIX `shm_open` (`O_CREAT|O_EXCL`, 0600, unlink-first, names ≤ 31 bytes)
//! + `mmap`; Windows `CreateFileMappingW` on the pagefile as `Local\`+name + `MapViewOfFile`. Open = map the header page, check
//! magic + EXACT version, read `total_size`, verify the object's size, then map all.
use std::ffi::CString;

pub struct Mapping { pub base: *mut u8, pub size: usize, pub name: String, pub owner: bool, #[cfg(unix)] fd: i32, #[cfg(windows)] handle: isize }
unsafe impl Send for Mapping {}

#[cfg(unix)] pub fn page_size() -> u64 { unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 } }
#[cfg(windows)] pub fn page_size() -> u64 { unsafe { let mut si: windows_sys::Win32::System::SystemInformation::SYSTEM_INFO = std::mem::zeroed(); windows_sys::Win32::System::SystemInformation::GetSystemInfo(&mut si); si.dwPageSize as u64 } }
pub fn round_up(v: u64, to: u64) -> u64 { v.div_ceil(to) * to }
pub fn self_pid() -> u32 { std::process::id() }

#[cfg(unix)]
impl Mapping {
    pub fn create(name: &str, total: u64) -> Result<Mapping, String> {
        if name.len() > 31 || !name.starts_with('/') { return Err("bad shm name".into()) }
        let c = CString::new(name).unwrap();
        unsafe {
            libc::shm_unlink(c.as_ptr());   // unlink-first: a stale object from a dead producer never blocks us
            let fd = libc::shm_open(c.as_ptr(), libc::O_CREAT | libc::O_EXCL | libc::O_RDWR, 0o600);
            if fd < 0 { return Err(format!("shm_open: {}", std::io::Error::last_os_error())) }
            if libc::ftruncate(fd, total as i64) != 0 { let e = std::io::Error::last_os_error(); libc::close(fd); return Err(format!("ftruncate: {e}")) }
            let base = libc::mmap(std::ptr::null_mut(), total as usize, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            if base == libc::MAP_FAILED { let e = std::io::Error::last_os_error(); libc::close(fd); return Err(format!("mmap: {e}")) }
            std::ptr::write_bytes(base as *mut u8, 0, total as usize);
            Ok(Mapping { base: base as *mut u8, size: total as usize, name: name.into(), owner: true, fd })
        }
    }
    /// `total_of(header_bytes_view) -> total_size` reads the header the producer wrote; magic/version are checked first.
    pub fn open(name: &str, header_bytes: u64, magic: u32, version: u32, total_of: impl Fn(*const u8) -> u64) -> Result<Mapping, String> {
        let c = CString::new(name).unwrap();
        unsafe {
            let fd = libc::shm_open(c.as_ptr(), libc::O_RDWR, 0);
            if fd < 0 { return Err(format!("shm_open: {}", std::io::Error::last_os_error())) }
            let probe = round_up(header_bytes, page_size()) as usize;
            let head = libc::mmap(std::ptr::null_mut(), probe, libc::PROT_READ, libc::MAP_SHARED, fd, 0);
            if head == libc::MAP_FAILED { let e = std::io::Error::last_os_error(); libc::close(fd); return Err(format!("mmap header: {e}")) }
            let m = std::ptr::read_volatile(head as *const u32); let v = std::ptr::read_volatile((head as *const u32).add(1));
            let total = total_of(head as *const u8);
            libc::munmap(head, probe);
            if m != magic { libc::close(fd); return Err(format!("magic {m:#x} != {magic:#x}")) }
            if v != version { libc::close(fd); return Err(format!("version {v} != {version} (exact match required)")) }
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(fd, &mut st) != 0 || (st.st_size as u64) < total { libc::close(fd); return Err(format!("object is {} bytes, header says {total}", st.st_size)) }
            let base = libc::mmap(std::ptr::null_mut(), total as usize, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            if base == libc::MAP_FAILED { let e = std::io::Error::last_os_error(); libc::close(fd); return Err(format!("mmap: {e}")) }
            Ok(Mapping { base: base as *mut u8, size: total as usize, name: name.into(), owner: false, fd })
        }
    }
}
#[cfg(unix)]
impl Drop for Mapping {
    fn drop(&mut self) { unsafe { libc::munmap(self.base as *mut _, self.size); libc::close(self.fd); if self.owner { let c = CString::new(self.name.as_str()).unwrap(); libc::shm_unlink(c.as_ptr()); } } }
}
/// POSIX: remove a ring a dead producer left (QCView does — "removed the ring a dead producer left"); Windows: nothing to do,
/// a section goes with its last handle.
#[cfg(unix)] pub fn unlink(name: &str) -> bool { let Ok(c) = CString::new(name) else { return false }; unsafe { libc::shm_unlink(c.as_ptr()) == 0 } }
#[cfg(windows)] pub fn unlink(_name: &str) -> bool { false }
#[cfg(unix)] pub fn process_alive(pid: u32) -> bool { unsafe { libc::kill(pid as i32, 0) == 0 || *libc::__error() == libc::EPERM } }

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::*; use windows_sys::Win32::System::Memory::*; use windows_sys::Win32::System::Threading::*;
    use super::*;
    fn werr(what: &str) -> String { format!("{what}: {}", std::io::Error::last_os_error()) }
    /// "/qcbae-ae" → L"Local\qcbae-ae" (vendor/qcbae `object_name`): session-local, no privilege needed.
    fn object_name(name: &str) -> Vec<u16> { let mut w: Vec<u16> = "Local\\".encode_utf16().collect(); w.extend(name[1..].encode_utf16()); w.push(0); w }
    impl Mapping {
        pub fn create(name: &str, total: u64) -> Result<Mapping, String> {
            if name.len() > 31 || !name.starts_with('/') { return Err("bad shm name".into()) }
            let wname = object_name(name);
            unsafe {
                // Same loop as the C++: a name still held by a consumer that has not closed it → wait up to ~1.5 s (no unlink on Windows;
                // the section lives while any handle is open). A stale section from a dead producer has no handles and is simply gone.
                let mut attempt = 0; let h = loop {
                    let h = CreateFileMappingW(INVALID_HANDLE_VALUE, std::ptr::null(), PAGE_READWRITE, (total >> 32) as u32, (total & 0xFFFF_FFFF) as u32, wname.as_ptr());
                    if h.is_null() { return Err(werr("CreateFileMapping")) }
                    if GetLastError() != ERROR_ALREADY_EXISTS { break h }
                    CloseHandle(h); attempt += 1;
                    if attempt >= 60 { return Err("ring name is still held by another process (a consumer that has not closed it)".into()) }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                };
                let base = MapViewOfFile(h, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, total as usize).Value as *mut u8;
                if base.is_null() { let e = werr("MapViewOfFile"); CloseHandle(h); return Err(e) }
                std::ptr::write_bytes(base, 0, total as usize);
                Ok(Mapping { base, size: total as usize, name: name.into(), owner: true, handle: h as isize })
            }
        }
        pub fn open(name: &str, header_bytes: u64, magic: u32, version: u32, total_of: impl Fn(*const u8) -> u64) -> Result<Mapping, String> {
            let wname = object_name(name);
            unsafe {
                let h = OpenFileMappingW(FILE_MAP_READ | FILE_MAP_WRITE, 0, wname.as_ptr());
                if h.is_null() { return Err(werr("OpenFileMapping")) }
                let probe = round_up(header_bytes, page_size()) as usize;
                let head = MapViewOfFile(h, FILE_MAP_READ, 0, 0, probe).Value as *const u8;
                if head.is_null() { let e = werr("MapViewOfFile header"); CloseHandle(h); return Err(e) }
                let m = std::ptr::read_volatile(head as *const u32); let v = std::ptr::read_volatile((head as *const u32).add(1));
                let total = total_of(head);
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: head as *mut _ });
                if m != magic { CloseHandle(h); return Err(format!("magic {m:#x} != {magic:#x}")) }
                if v != version { CloseHandle(h); return Err(format!("version {v} != {version} (exact match required)")) }
                if total < probe as u64 { CloseHandle(h); return Err("object smaller than its header claims".into()) }
                // A view larger than the section is refused by MapViewOfFile itself (ERROR_ACCESS_DENIED) — the fstat check in other clothes.
                let base = MapViewOfFile(h, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, total as usize).Value as *mut u8;
                if base.is_null() { let e = if GetLastError() == ERROR_ACCESS_DENIED { "object smaller than its header claims".to_string() } else { werr("MapViewOfFile") }; CloseHandle(h); return Err(e) }
                Ok(Mapping { base, size: total as usize, name: name.into(), owner: false, handle: h as isize })
            }
        }
    }
    impl Drop for Mapping {
        fn drop(&mut self) { unsafe { UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.base as *mut _ }); CloseHandle(self.handle as HANDLE); } }
    }
    /// vendor/qcbae `process_alive`: a handle we cannot open (another user / elevated) counts as alive, like EPERM; anything else = gone.
    pub fn process_alive(pid: u32) -> bool {
        if pid == 0 { return false }
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() { return GetLastError() == ERROR_ACCESS_DENIED }
            let mut code: u32 = 0; let ok = GetExitCodeProcess(h, &mut code) != 0; CloseHandle(h);
            ok && code == STILL_ACTIVE as u32
        }
    }
}
#[cfg(windows)] pub use win::process_alive;
