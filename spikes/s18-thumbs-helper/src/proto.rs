//! The framed protocol between the application and a helper, over the helper's standard input and output.
//! Every frame: a u32 length (little endian), then that many bytes.
//!   request  = id u64 | op u8 | fmt u8 | size u16 | soft_ms u32 | path (UTF-8, the rest of the frame)
//!   response = id u64 | status u8 | 0 | w u16 | h u16 | 0 0 | work_us u32 | aux u32 | payload (the rest)
//! One request is in flight per helper, so a death or a hang is always the fault of exactly one request.
use std::io::{self, Read, Write};

pub const OP_DECODE: u8 = 1; // our own decoder: the file -> a master of `size` px
pub const OP_THUMB: u8 = 2; // the system's thumbnail
pub const OP_ICON: u8 = 3; // the system's icon for this file
pub const OP_TYPE_ICON: u8 = 4; // the system's icon for a type (path = the extension)
pub const OP_PING: u8 = 5;
pub const OP_QL_ICON: u8 = 6; // macOS: the icon through QuickLook's generator (asynchronous, cancellable)

pub const FMT_RGBA: u8 = 0;
pub const FMT_JPEG: u8 = 1;

pub const OK: u8 = 0;
pub const FAILED: u8 = 1; // the helper answered: no thumbnail / cannot decode
pub const TIMEOUT: u8 = 2; // the helper cancelled the system request at soft_ms and answered
pub const CRASHED: u8 = 3; // made by the parent: the helper died under this request
pub const HUNG: u8 = 4; // made by the parent: no answer by the hard bound, the helper was killed
pub const CANCELLED: u8 = 5; // made by the parent: cancelled while in flight, the answer was dropped

pub fn status_name(s: u8) -> &'static str {
    match s { OK => "ok", FAILED => "failed", TIMEOUT => "timeout(cancelled in helper)", CRASHED => "CRASHED", HUNG => "HUNG(killed)", CANCELLED => "cancelled", _ => "?" }
}

#[derive(Clone, Debug)]
pub struct Request { pub id: u64, pub op: u8, pub fmt: u8, pub size: u16, pub soft_ms: u32, pub path: String }

#[derive(Debug, Default)]
pub struct Response { pub id: u64, pub status: u8, pub w: u16, pub h: u16, pub work_us: u32, pub aux: u32, pub payload: Vec<u8> }

pub fn write_request(w: &mut impl Write, r: &Request) -> io::Result<()> {
    let p = r.path.as_bytes();
    let mut b = Vec::with_capacity(20 + p.len());
    b.extend_from_slice(&((16 + p.len()) as u32).to_le_bytes());
    b.extend_from_slice(&r.id.to_le_bytes());
    b.push(r.op); b.push(r.fmt);
    b.extend_from_slice(&r.size.to_le_bytes());
    b.extend_from_slice(&r.soft_ms.to_le_bytes());
    b.extend_from_slice(p);
    w.write_all(&b)?; w.flush()
}

pub fn read_request(r: &mut impl Read) -> io::Result<Request> {
    let mut h = [0u8; 20];
    r.read_exact(&mut h)?;
    let len = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
    if !(16..=16 + 65536).contains(&len) { return Err(io::Error::other("bad frame")) }
    let mut p = vec![0u8; len - 16];
    r.read_exact(&mut p)?;
    Ok(Request { id: u64::from_le_bytes(h[4..12].try_into().unwrap()), op: h[12], fmt: h[13], size: u16::from_le_bytes([h[14], h[15]]),
        soft_ms: u32::from_le_bytes(h[16..20].try_into().unwrap()), path: String::from_utf8(p).map_err(io::Error::other)? })
}

pub const RESP_HEAD: usize = 24;

pub fn write_response(w: &mut impl Write, r: &Response) -> io::Result<()> {
    let mut h = [0u8; 4 + RESP_HEAD];
    h[0..4].copy_from_slice(&((RESP_HEAD + r.payload.len()) as u32).to_le_bytes());
    h[4..12].copy_from_slice(&r.id.to_le_bytes());
    h[12] = r.status;
    h[14..16].copy_from_slice(&r.w.to_le_bytes());
    h[16..18].copy_from_slice(&r.h.to_le_bytes());
    h[20..24].copy_from_slice(&r.work_us.to_le_bytes());
    h[24..28].copy_from_slice(&r.aux.to_le_bytes());
    w.write_all(&h)?;
    w.write_all(&r.payload)?;
    w.flush()
}

/// The largest answer the parent accepts (a 2048 px RGBA master is 16 MiB): a helper gone mad cannot make it allocate.
pub const MAX_PAYLOAD: usize = 64 << 20;

pub fn read_response(r: &mut impl Read) -> io::Result<Response> {
    let mut h = [0u8; 4 + RESP_HEAD];
    r.read_exact(&mut h)?;
    let len = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
    if !(RESP_HEAD..=RESP_HEAD + MAX_PAYLOAD).contains(&len) { return Err(io::Error::other("bad frame")) }
    let mut payload = vec![0u8; len - RESP_HEAD];
    r.read_exact(&mut payload)?;
    Ok(Response { id: u64::from_le_bytes(h[4..12].try_into().unwrap()), status: h[12], w: u16::from_le_bytes([h[14], h[15]]), h: u16::from_le_bytes([h[16], h[17]]),
        work_us: u32::from_le_bytes(h[20..24].try_into().unwrap()), aux: u32::from_le_bytes(h[24..28].try_into().unwrap()), payload })
}
