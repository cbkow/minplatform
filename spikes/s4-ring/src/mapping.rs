//! S4 — the shared mapping (vendor/qcbae `ring_mapping`): POSIX `shm_open` (`O_CREAT|O_EXCL`, 0600, unlink-first, names ≤ 31 bytes)
//! + `mmap`; Windows `CreateFileMappingW` on the pagefile as `Local\`+name + `MapViewOfFile`. Open = map the header page, check
//! magic + EXACT version, read `total_size`, verify the object's size, then map all.
use std::ffi::CString;

pub struct Mapping { pub base: *mut u8, pub size: usize, pub name: String, pub owner: bool, #[cfg(unix)] fd: i32 }
unsafe impl Send for Mapping {}

pub fn page_size() -> u64 { unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 } }
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
pub fn process_alive(pid: u32) -> bool { #[cfg(unix)] unsafe { libc::kill(pid as i32, 0) == 0 || *libc::__error() == libc::EPERM } #[cfg(not(unix))] { let _ = pid; true } }
