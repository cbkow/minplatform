//! The framed protocol between the application and a helper copy of itself, over the helper's standard input and
//! output (spike S18; canon row 456). Every frame: a u32 length (little endian), then that many bytes.
//!   request  = id u64 | op u8 | flags u8 | size u16 | soft_ms u32 | file size u64 | file time i64 | path (UTF-8, the rest)
//!   response = id u64 | status u8 | 0 | w u16 | h u16 | 0 0 | work_us u32 | aux u32 | payload (the rest)
//! The payload is a picture: RGBA, 8 bits, colour multiplied by its alpha, sRGB, top row first, w x h.
//! One request is in flight per helper, so a death or a hang is always the fault of exactly one request.
use std::io::{self, Read, Write};

/// Our own decoder (EXR, DPX, TIFF, PSD ...): the file -> a picture that fits `size` px.
pub const OP_DECODE: u8 = 1;
/// The system's thumbnail of the file.
pub const OP_THUMB: u8 = 2;
/// The system's icon of THIS file (an application's own, a shortcut's).
pub const OP_ICON: u8 = 3;
/// A frame of a video, by the family's player (the middle of the clip), its colour by the presenter's pipeline.
pub const OP_VIDEO: u8 = 4;
pub const OP_PING: u8 = 5;
/// The cache on disk (canon row 456, point 4; `cache.rs`): the file's picture as it was kept - [`OK`] with it,
/// [`MISS`] when there is none for the file as it is now, [`NONE`] when the file is known to have no picture.
pub const OP_CACHED: u8 = 6;
/// Remember that the file, as it is now, has no picture.
pub const OP_MARK: u8 = 7;
/// `path` is a folder: what the cache keeps for files that are no longer in it is dropped.
pub const OP_PRUNE: u8 = 8;
/// `path` is a folder: forget which of its files have no picture (Refresh asks again).
pub const OP_FORGET: u8 = 9;
/// The lightbox's picture (canon row 473, point 4): the file decoded WHOLE, as its decoder gives it - planes of 8 or
/// 16 bits or half floats, not yet colour-managed (the presenter does that, as for QCView's stills). The payload is a
/// small head and the planes (`helper::Full`); `aux` is how many times it was reduced (0: it is whole).
pub const OP_FULL: u8 = 10;
/// The layers of an EXR file (its parts, or the layers its channels are named for): their names, one a line.
/// A layer is then asked for by [`OP_DECODE`] or [`OP_FULL`] with the path, the character U+0001 and the layer's name.
pub const OP_LAYERS: u8 = 11;
/// A flag of [`OP_DECODE`], [`OP_VIDEO`] and [`OP_THUMB`]: the picture is made at the cache's size, kept there, and answered at `size`.
pub const KEEP: u8 = 1;
/// Tests only (honoured when the helper was started with `UFB_HELPER_TEST` set): end at once; never answer.
pub const OP_TEST_ABORT: u8 = 250;
pub const OP_TEST_HANG: u8 = 251;

pub const OK: u8 = 0;
/// The helper answered: no thumbnail, or the file cannot be decoded.
pub const FAILED: u8 = 1;
/// The helper stopped waiting for the system at `soft_ms` and answered (macOS; Windows' call cannot be left: its bound is the helper's end).
#[allow(dead_code)]
pub const TIMEOUT: u8 = 2;
/// Made by the parent: the helper died under this request.
pub const CRASHED: u8 = 3;
/// Made by the parent: no answer by the hard bound; the helper was ended.
pub const HUNG: u8 = 4;
/// Made by the parent: no helper could be started.
pub const NO_HELPER: u8 = 6;
/// [`OP_CACHED`]: the cache has nothing for the file as it is now.
pub const MISS: u8 = 7;
/// [`OP_CACHED`]: the file is known to have no picture.
pub const NONE: u8 = 8;

#[derive(Clone, Debug)]
pub struct Request { pub id: u64, pub op: u8, pub flags: u8, pub size: u16, pub soft_ms: u32, /// the file's size and modification time as its folder's listing gave them: what a kept picture is valid for
    pub fsize: u64, pub mtime: i64, pub path: String }
impl Request {
    /// A request about `path` with no flags and no file stamp.
    #[allow(dead_code)]
    pub fn new(id: u64, op: u8, size: u16, soft_ms: u32, path: impl Into<String>) -> Request { Request { id, op, flags: 0, size, soft_ms, fsize: 0, mtime: 0, path: path.into() } }
}
const REQ_HEAD: usize = 32;

#[derive(Debug, Default)]
pub struct Response { pub id: u64, pub status: u8, pub w: u16, pub h: u16, pub work_us: u32, pub aux: u32, pub payload: Vec<u8> }

pub fn write_request(w: &mut impl Write, r: &Request) -> io::Result<()> {
    let p = r.path.as_bytes();
    let mut b = Vec::with_capacity(4 + REQ_HEAD + p.len());
    b.extend_from_slice(&((REQ_HEAD + p.len()) as u32).to_le_bytes());
    b.extend_from_slice(&r.id.to_le_bytes());
    b.push(r.op); b.push(r.flags);
    b.extend_from_slice(&r.size.to_le_bytes());
    b.extend_from_slice(&r.soft_ms.to_le_bytes());
    b.extend_from_slice(&r.fsize.to_le_bytes());
    b.extend_from_slice(&r.mtime.to_le_bytes());
    b.extend_from_slice(p);
    w.write_all(&b)?; w.flush()
}

pub fn read_request(r: &mut impl Read) -> io::Result<Request> {
    let mut h = [0u8; 4 + REQ_HEAD];
    r.read_exact(&mut h)?;
    let len = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
    if !(REQ_HEAD..=REQ_HEAD + 65536).contains(&len) { return Err(io::Error::other("bad frame")) }
    let mut p = vec![0u8; len - REQ_HEAD];
    r.read_exact(&mut p)?;
    Ok(Request { id: u64::from_le_bytes(h[4..12].try_into().unwrap()), op: h[12], flags: h[13], size: u16::from_le_bytes([h[14], h[15]]),
        soft_ms: u32::from_le_bytes(h[16..20].try_into().unwrap()), fsize: u64::from_le_bytes(h[20..28].try_into().unwrap()), mtime: i64::from_le_bytes(h[28..36].try_into().unwrap()),
        path: String::from_utf8(p).map_err(io::Error::other)? })
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

/// The largest answer the parent accepts (a 1024 px picture is 4 MiB): a helper gone mad cannot make it allocate.
pub const MAX_PAYLOAD: usize = 16 << 20;

/// The largest answer to [`OP_FULL`]: an 8192 x 8192 picture of four half-float planes, rows padded.
pub const MAX_FULL: usize = 600 << 20;
pub fn read_response(r: &mut impl Read) -> io::Result<Response> { read_response_max(r, MAX_PAYLOAD) }
/// As [`read_response`], with the ceiling the request allows.
pub fn read_response_max(r: &mut impl Read, max: usize) -> io::Result<Response> {
    let mut h = [0u8; 4 + RESP_HEAD];
    r.read_exact(&mut h)?;
    let len = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
    if !(RESP_HEAD..=RESP_HEAD + max).contains(&len) { return Err(io::Error::other("bad frame")) }
    let mut payload = vec![0u8; len - RESP_HEAD];
    r.read_exact(&mut payload)?;
    Ok(Response { id: u64::from_le_bytes(h[4..12].try_into().unwrap()), status: h[12], w: u16::from_le_bytes([h[14], h[15]]), h: u16::from_le_bytes([h[16], h[17]]),
        work_us: u32::from_le_bytes(h[20..24].try_into().unwrap()), aux: u32::from_le_bytes(h[24..28].try_into().unwrap()), payload })
}
