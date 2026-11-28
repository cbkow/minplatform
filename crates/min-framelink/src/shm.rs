//! A shared block: named memory cut into slots of one size. The HOST makes it and owns it; the provider maps it.
//! POSIX systems only so far (Windows names its shared memory by other calls; the messages are the same).
use std::ffi::CString;
use std::io;

pub struct Block { ptr: *mut u8, len: usize, slots: u32, slot_bytes: usize, name: CString }
unsafe impl Send for Block {}
unsafe impl Sync for Block {}

fn total(slots: u32, slot_bytes: usize) -> io::Result<usize> {
    if slots == 0 || slot_bytes == 0 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "a block needs slots, and bytes in them")) }
    (slots as usize).checked_mul(slot_bytes).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the block is too large"))
}
fn c_name(name: &str) -> io::Result<CString> { CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "the block's name")) }

impl Block {
    /// The host's call. `name` starts with a slash and is short (31 bytes on macOS).
    pub fn create(name: &str, slots: u32, slot_bytes: usize) -> io::Result<Block> {
        let (len, name) = (total(slots, slot_bytes)?, c_name(name)?);
        unsafe {
            let fd = libc::shm_open(name.as_ptr(), libc::O_CREAT | libc::O_EXCL | libc::O_RDWR, 0o600 as libc::c_uint);
            if fd < 0 { return Err(io::Error::last_os_error()) }
            if libc::ftruncate(fd, len as libc::off_t) != 0 { let e = io::Error::last_os_error(); libc::close(fd); libc::shm_unlink(name.as_ptr()); return Err(e) }
            Block::map(fd, len, slots, slot_bytes, name)
        }
    }
    /// The provider's call.
    pub fn open(name: &str, slots: u32, slot_bytes: usize) -> io::Result<Block> {
        let (len, name) = (total(slots, slot_bytes)?, c_name(name)?);
        unsafe {
            let fd = libc::shm_open(name.as_ptr(), libc::O_RDWR, 0 as libc::c_uint);
            if fd < 0 { return Err(io::Error::last_os_error()) }
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(fd, &mut st) != 0 || (st.st_size as u64) < len as u64 { libc::close(fd); return Err(io::Error::new(io::ErrorKind::InvalidInput, "the block is smaller than its slots")) }
            Block::map(fd, len, slots, slot_bytes, name)
        }
    }
    unsafe fn map(fd: i32, len: usize, slots: u32, slot_bytes: usize, name: CString) -> io::Result<Block> {
        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
        let e = io::Error::last_os_error();
        unsafe { libc::close(fd) };
        if ptr == libc::MAP_FAILED { return Err(e) }
        Ok(Block { ptr: ptr as *mut u8, len, slots, slot_bytes, name })
    }
    /// Take the name away. The memory lives on while a process has it mapped; the host calls this once the provider has
    /// attached, so that nothing is left behind when both ends die.
    pub fn unlink(&self) { unsafe { libc::shm_unlink(self.name.as_ptr()); } }
    pub fn slots(&self) -> u32 { self.slots }
    pub fn slot_bytes(&self) -> usize { self.slot_bytes }
    pub fn slot(&self, n: u32) -> *mut u8 { assert!(n < self.slots); unsafe { self.ptr.add(n as usize * self.slot_bytes) } }
    /// # Safety
    /// Nobody writes the slot meanwhile: its request has been answered, and no new one names it.
    pub unsafe fn bytes(&self, n: u32) -> &[u8] { unsafe { std::slice::from_raw_parts(self.slot(n), self.slot_bytes) } }
}
impl Drop for Block { fn drop(&mut self) { unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len); } } }
