//! DPX (SMPTE 268M) — ours, not QCView's: QCView opened a `.dpx` as a one-frame video through FFmpeg and had no DPX
//! sequences (canon row 190, the user 2026-11-08: "Native Rust DPX loader"). The first image element is decoded:
//! descriptors RGB (50), RGBA (51), ABGR (52) and luma (6); 8 / 10 / 12 / 16 bits; both byte orders ("SDPX" big-endian,
//! "XPDS" little-endian); 10-bit as three samples per 32-bit word (packing 1 = method A, pad bits low — the common form —
//! packing 2 = method B, pad bits high) or packed continuously (packing 0); 12-bit one sample per 16-bit word (A / B).
//! RLE-encoded and transposed (orientation ≥ 4) files are refused. 10 / 12-bit samples widen to 16 by bit replication, so
//! the code value's top bits are the file's exactly (`v >> 6` is the 10-bit sample, bit-identical to FFmpeg's decode).
//! No transfer is applied: DPX is shown as its code values (log or video alike), like a review player without a chain.
use crate::{Error, Image, Sample, Samples};

struct Header { le: bool, offset: usize, w: u32, h: u32, orientation: u16, descriptor: u8, bits: u8, packing: u16, encoding: u16 }

fn header(path: &str, b: &[u8]) -> Result<Header, Error> {
    let bad = |m: &str| Error::Decode(path.into(), m.into());
    if b.len() < 820 { return Err(bad("truncated DPX header")) }
    let le = match &b[0..4] { b"SDPX" => false, b"XPDS" => true, _ => return Err(bad("not a DPX file")) };
    let u32_at = |o: usize| { let a = [b[o], b[o + 1], b[o + 2], b[o + 3]]; if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) } };
    let u16_at = |o: usize| { let a = [b[o], b[o + 1]]; if le { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) } };
    let mut offset = u32_at(808) as usize;   // the element's data offset
    if offset == 0 || offset == u32::MAX as usize { offset = u32_at(4) as usize; }   // else the file header's image offset
    Ok(Header { le, offset, w: u32_at(772), h: u32_at(776), orientation: u16_at(768), descriptor: b[800], bits: b[803], packing: u16_at(804), encoding: u16_at(806) })
}

fn channels_of(path: &str, d: u8) -> Result<usize, Error> {
    match d { 50 => Ok(3), 51 | 52 => Ok(4), 6 => Ok(1), _ => Err(Error::Decode(path.into(), format!("DPX descriptor {d} not supported"))) }
}

pub fn dimensions(path: &str) -> Result<(u32, u32), Error> {
    use std::io::Read;
    let mut b = vec![0u8; 820];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut b)).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let h = header(path, &b)?; Ok((h.w, h.h))
}

pub fn load(path: &str) -> Result<Image, Error> { decode(path, 1) }

/// The hover thumbnail's fast path (QCView's skip rule, every Nth row and pixel, N = the long side / `max_size`): only the
/// rows it samples are read from disk.
pub fn load_thumbnail(path: &str, max_size: u32) -> Result<Image, Error> {
    let (w, h) = dimensions(path)?;
    decode(path, (w.max(h) / max_size.max(1)).max(1) as usize)
}

/// Decode every `skip`-th row and pixel (1 = the whole image), reading only those rows (every line starts on a 32-bit word).
fn decode(path: &str, skip: usize) -> Result<Image, Error> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let mut hb = vec![0u8; 820];
    f.read_exact(&mut hb).map_err(|e| Error::Decode(path.into(), format!("truncated DPX header: {e}")))?;
    let h = header(path, &hb)?;
    let bad = |m: String| Error::Decode(path.into(), m);
    if h.encoding != 0 { return Err(bad("RLE-encoded DPX is not supported".into())) }
    if h.orientation >= 4 { return Err(bad(format!("DPX orientation {} (transposed) is not supported", h.orientation))) }
    let ch = channels_of(path, h.descriptor)?;
    let (w, ht) = (h.w as usize, h.h as usize);
    if w == 0 || ht == 0 || w > 65536 || ht > 65536 { return Err(bad(format!("DPX size {w}x{ht}"))) }
    let n = w * ch;   // samples per line
    let stride = match (h.bits, h.packing) {
        (8, _) => n.div_ceil(4) * 4, (10, 0) => (n * 10).div_ceil(32) * 4, (10, _) => n.div_ceil(3) * 4, (12 | 16, _) => (n * 2).div_ceil(4) * 4,
        (b, _) => return Err(bad(format!("DPX bit depth {b} not supported"))),
    };
    let len = f.metadata().map(|m| m.len() as usize).unwrap_or(0);
    if len < h.offset + stride * ht { return Err(bad(format!("DPX data truncated ({} of {} bytes)", len.saturating_sub(h.offset), stride * ht))) }
    let le = h.le;
    let rd32 = |l: &[u8], o: usize| { let a = [l[o], l[o + 1], l[o + 2], l[o + 3]]; if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) } };
    let rd16 = |l: &[u8], o: usize| { let a = [l[o], l[o + 1]]; if le { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) } };
    // sample i of a line, widened to 16 bits (8-bit read separately)
    let sample = |l: &[u8], i: usize| -> u16 {
        match (h.bits, h.packing) {
            (10, 0) => { let bit = i * 10; let (word, off) = (bit / 32, bit % 32); let hi = rd32(l, word * 4) as u64; let lo = if off + 10 > 32 { rd32(l, word * 4 + 4) as u64 } else { 0 }; widen10(((((hi << 32) | lo) >> (64 - off - 10)) & 0x3ff) as u16) }
            // filled: three samples per word — method A (packing 1) pad bits low, method B (packing 2) pad bits high
            (10, p) => { let word = rd32(l, (i / 3) * 4); widen10(((word >> (if p == 1 { 2 } else { 0 } + 20 - 10 * (i % 3) as u32)) & 0x3ff) as u16) }
            (12, p) => { let s = rd16(l, i * 2); widen12(if p == 1 { s >> 4 } else { s & 0xfff }) }
            _ => rd16(l, i * 2),
        }
    };
    let (ow, oh) = ((w / skip).max(1), (ht / skip).max(1));
    let mut line = vec![0u8; stride];
    let (mut v8, mut v16) = (Vec::new(), Vec::new());
    for oy in 0..oh {
        f.seek(SeekFrom::Start((h.offset + oy * skip * stride) as u64)).and_then(|_| f.read_exact(&mut line)).map_err(|e| bad(format!("DPX read: {e}")))?;
        for ox in 0..ow { for c in 0..ch { let i = ox * skip * ch + c; if h.bits == 8 { v8.push(line[i]) } else { v16.push(sample(&line, i)) } } }
    }
    let mut img = if h.bits == 8 { build(ow, oh, Samples::U8(&v8), ch, h.descriptor) } else { build(ow, oh, Samples::U16(&v16), ch, h.descriptor) };
    if h.orientation != 0 { img.flip(h.orientation & 1 != 0, h.orientation & 2 != 0); }
    Ok(img)
}

fn widen10(v: u16) -> u16 { (v << 6) | (v >> 4) }
fn widen12(v: u16) -> u16 { (v << 4) | (v >> 8) }

/// Interleaved samples → planes; ABGR is reordered to RGBA first.
fn build(w: usize, h: usize, s: Samples, ch: usize, descriptor: u8) -> Image {
    let (color, alpha) = (ch >= 3, ch == 4);
    let sample = match s { Samples::U8(_) => Sample::U8, Samples::U16(_) => Sample::U16 };
    if descriptor != 52 { return Image::from_interleaved(w as u32, h as u32, sample, s, ch, color, alpha) }
    match s {
        Samples::U8(d) => { let r: Vec<u8> = d.as_chunks::<4>().0.iter().flat_map(|p| [p[3], p[2], p[1], p[0]]).collect(); Image::from_interleaved(w as u32, h as u32, sample, Samples::U8(&r), 4, true, true) }
        Samples::U16(d) => { let r: Vec<u16> = d.as_chunks::<4>().0.iter().flat_map(|p| [p[3], p[2], p[1], p[0]]).collect(); Image::from_interleaved(w as u32, h as u32, sample, Samples::U16(&r), 4, true, true) }
    }
}
