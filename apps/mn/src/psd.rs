//! A Photoshop file's FLATTENED picture (canon row 313): the "Image Data" section at the end of a PSD / PSB - no
//! layers are read or composed. The section is read row by row and averaged down to the size asked while it is read,
//! so a very large file never sits in memory whole.
//!
//! Read: PSD (version 1) and PSB (version 2); 8, 16 and 32 bits; RGB, Grayscale, CMYK; raw and RLE (PackBits) data.
//! Not read (said in words): Indexed, Lab, Duotone, Multichannel, Bitmap; ZIP-compressed composites (not written by
//! Photoshop for this section). Channels after the colour's own are ignored - one may be a saved selection, not
//! transparency - so the picture is opaque. CMYK is turned to RGB by arithmetic, without the file's profile.
//!
//! A file saved WITHOUT "Maximize Compatibility" has no real composite: its Version Info resource (0x0421) says so,
//! and that is reported instead of the placeholder the section then holds.
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub struct Flat { pub rgba: Vec<u8>, pub w: u32, pub h: u32, /// the file's own size
    #[allow(dead_code)] pub full: (u32, u32) }

fn be16(r: &mut impl Read) -> Result<u16, String> { let mut b = [0u8; 2]; r.read_exact(&mut b).map_err(|e| e.to_string())?; Ok(u16::from_be_bytes(b)) }
fn be32(r: &mut impl Read) -> Result<u32, String> { let mut b = [0u8; 4]; r.read_exact(&mut b).map_err(|e| e.to_string())?; Ok(u32::from_be_bytes(b)) }
fn be64(r: &mut impl Read) -> Result<u64, String> { let mut b = [0u8; 8]; r.read_exact(&mut b).map_err(|e| e.to_string())?; Ok(u64::from_be_bytes(b)) }

struct Head { big: bool, channels: usize, w: u32, h: u32, depth: u16, mode: u16 }
fn head(r: &mut impl Read) -> Result<Head, String> {
    let mut sig = [0u8; 4]; r.read_exact(&mut sig).map_err(|e| e.to_string())?;
    if &sig != b"8BPS" { return Err("not a Photoshop file".into()) }
    let version = be16(r)?; if version != 1 && version != 2 { return Err(format!("Photoshop file version {version} is not read")) }
    let mut skip = [0u8; 6]; r.read_exact(&mut skip).map_err(|e| e.to_string())?;
    let channels = be16(r)? as usize; let (h, w) = (be32(r)?, be32(r)?); let (depth, mode) = (be16(r)?, be16(r)?);
    if w == 0 || h == 0 || channels == 0 || channels > 56 { return Err("the Photoshop file's header is not valid".into()) }
    Ok(Head { big: version == 2, channels, w, h, depth, mode })
}
/// The file's own size, from its header.
#[allow(dead_code)]
pub fn size(path: &Path) -> Result<(u32, u32), String> { let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?; let h = head(&mut f)?; Ok((h.w, h.h)) }

/// The flattened picture, its longer side at most `max` pixels.
pub fn load(path: &Path, max: u32) -> Result<Flat, String> {
    let mut r = BufReader::with_capacity(1 << 20, std::fs::File::open(path).map_err(|e| e.to_string())?);
    let hd = head(&mut r)?;
    let colours = match hd.mode { 1 => 1, 3 => 3, 4 => 4, 0 => return Err("Bitmap-mode Photoshop files are not read".into()), 2 => return Err("Indexed-colour Photoshop files are not read".into()), 9 => return Err("Lab-colour Photoshop files are not read".into()), m => return Err(format!("this Photoshop colour mode ({m}) is not read")) };
    if hd.channels < colours { return Err("the Photoshop file has fewer channels than its colour mode needs".into()) }
    let bytes = match hd.depth { 8 => 1usize, 16 => 2, 32 => 4, d => return Err(format!("{d}-bit Photoshop files are not read")) };
    // colour mode data, then the image resources (looked through for Version Info), then the layers - skipped
    let n = be32(&mut r)? as i64; r.seek(SeekFrom::Current(n)).map_err(|e| e.to_string())?;
    let n = be32(&mut r)? as usize; let mut res = vec![0u8; n]; r.read_exact(&mut res).map_err(|e| e.to_string())?;
    if !has_composite(&res) { return Err("saved without \u{201c}Maximize Compatibility\u{201d}: the file holds no flattened picture".into()) }
    let n = if hd.big { be64(&mut r)? } else { be32(&mut r)? as u64 }; r.seek(SeekFrom::Current(n as i64)).map_err(|e| e.to_string())?;
    let rle = match be16(&mut r)? { 0 => false, 1 => true, c => return Err(format!("this Photoshop compression ({c}) is not read")) };
    let (w, h) = (hd.w as usize, hd.h as usize);
    // RLE: the byte count of every row of every channel comes first
    let counts: Vec<usize> = if rle { let mut v = Vec::with_capacity(hd.channels * h); for _ in 0..hd.channels * h { v.push(if hd.big { be32(&mut r)? as usize } else { be16(&mut r)? as usize }); } v } else { vec![] };
    let k = (max as f64 / w.max(h) as f64).min(1.0);
    let (tw, th) = (((w as f64 * k).round() as usize).clamp(1, w), ((h as f64 * k).round() as usize).clamp(1, h));
    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(colours);
    let (mut row, mut packed) = (vec![0u8; w * bytes], Vec::new());
    for c in 0..hd.channels {
        if c >= colours {
            // not needed: stepped over (raw: by its size; RLE: by its rows' counts)
            let skip: u64 = if rle { counts[c * h..(c + 1) * h].iter().map(|x| *x as u64).sum() } else { (w * h * bytes) as u64 };
            if c + 1 < hd.channels { r.seek(SeekFrom::Current(skip as i64)).map_err(|e| e.to_string())?; }
            continue
        }
        let mut out = vec![0u8; tw * th]; let (mut sum, mut n) = (vec![0u32; tw], vec![0u32; tw]); let mut ty = 0usize;
        for y in 0..h {
            if rle { packed.resize(counts[c * h + y], 0); r.read_exact(&mut packed).map_err(|e| e.to_string())?; unpack(&packed, &mut row)?; } else { r.read_exact(&mut row).map_err(|e| e.to_string())?; }
            for x in 0..w { let v = match bytes { 1 => row[x] as u32, 2 => row[x * 2] as u32, _ => linear(f32::from_be_bytes([row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]])) as u32 }; let tx = x * tw / w; sum[tx] += v; n[tx] += 1; }
            // the last source row of this target row: the averages are written
            if (y + 1) * th / h != ty || y + 1 == h { for x in 0..tw { out[ty * tw + x] = ((sum[x] + n[x] / 2) / n[x].max(1)) as u8; sum[x] = 0; n[x] = 0; } ty += 1; if ty >= th && y + 1 < h { ty = th - 1; } }
        }
        planes.push(out);
    }
    let mut rgba = vec![255u8; tw * th * 4];
    for i in 0..tw * th {
        let (r8, g8, b8) = match colours {
            1 => (planes[0][i], planes[0][i], planes[0][i]),
            3 => (planes[0][i], planes[1][i], planes[2][i]),
            // stored inverted (255 = no ink): colour times black
            _ => { let k = planes[3][i] as u32; ((planes[0][i] as u32 * k / 255) as u8, (planes[1][i] as u32 * k / 255) as u8, (planes[2][i] as u32 * k / 255) as u8) }
        };
        rgba[i * 4] = r8; rgba[i * 4 + 1] = g8; rgba[i * 4 + 2] = b8;
    }
    Ok(Flat { rgba, w: tw as u32, h: th as u32, full: (hd.w, hd.h) })
}
/// A 32-bit sample (linear light) as an sRGB value.
fn linear(v: f32) -> u8 { let l = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }; let s = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }; (s * 255.0 + 0.5) as u8 }
/// PackBits: a count byte n - 0..=127 copies n + 1 bytes, 129..=255 repeats the next byte 257 - n times, 128 is nothing.
fn unpack(src: &[u8], out: &mut [u8]) -> Result<(), String> {
    let (mut i, mut o) = (0usize, 0usize);
    while i < src.len() && o < out.len() {
        let n = src[i] as i8; i += 1;
        if n >= 0 { let k = n as usize + 1; if i + k > src.len() || o + k > out.len() { return Err("the Photoshop file's picture data is damaged".into()) } out[o..o + k].copy_from_slice(&src[i..i + k]); i += k; o += k; }
        else if n != -128 { let k = (1 - n as i32) as usize; if i >= src.len() || o + k > out.len() { return Err("the Photoshop file's picture data is damaged".into()) } out[o..o + k].fill(src[i]); i += 1; o += k; }
    }
    if o < out.len() { return Err("the Photoshop file's picture data is damaged".into()) }
    Ok(())
}
/// The Version Info resource (0x0421) says whether the composite is real; a file without the resource is taken to have one.
fn has_composite(res: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 12 <= res.len() {
        if &res[i..i + 4] != b"8BIM" && &res[i..i + 4] != b"8B64" { return true }
        let id = u16::from_be_bytes([res[i + 4], res[i + 5]]);
        let name = 1 + res[i + 6] as usize; let name = name + name % 2;
        let at = i + 6 + name; if at + 4 > res.len() { return true }
        let len = u32::from_be_bytes([res[at], res[at + 1], res[at + 2], res[at + 3]]) as usize; let data = at + 4;
        if id == 0x0421 { return res.get(data + 4).is_none_or(|b| *b != 0) }
        i = data + len + len % 2;
    }
    true
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    /// A 4 x 2 RGB file: the left half red, the right half blue; `rle` packs each row; `big` writes a PSB.
    pub(crate) fn file(big: bool, rle: bool, depth: u16, real: bool) -> Vec<u8> {
        let mut f = b"8BPS".to_vec(); f.extend_from_slice(&(if big { 2u16 } else { 1 }).to_be_bytes()); f.extend_from_slice(&[0; 6]);
        f.extend_from_slice(&4u16.to_be_bytes()); f.extend_from_slice(&2u32.to_be_bytes()); f.extend_from_slice(&4u32.to_be_bytes()); f.extend_from_slice(&depth.to_be_bytes()); f.extend_from_slice(&3u16.to_be_bytes());
        f.extend_from_slice(&0u32.to_be_bytes());
        let mut res = b"8BIM".to_vec(); res.extend_from_slice(&0x0421u16.to_be_bytes()); res.extend_from_slice(&[0, 0]); res.extend_from_slice(&6u32.to_be_bytes()); res.extend_from_slice(&[0, 0, 0, 1, real as u8, 0]);
        f.extend_from_slice(&(res.len() as u32).to_be_bytes()); f.extend_from_slice(&res);
        if big { f.extend_from_slice(&0u64.to_be_bytes()) } else { f.extend_from_slice(&0u32.to_be_bytes()) }
        f.extend_from_slice(&(rle as u16).to_be_bytes());
        // four channels: R, G, B and a saved selection that must be ignored
        let rows: Vec<Vec<u8>> = [[255u8, 255, 0, 0], [0, 0, 0, 0], [0, 0, 255, 255], [9, 9, 9, 9]].iter().flat_map(|ch| { let one: Vec<u8> = ch.iter().flat_map(|v| if depth == 16 { vec![*v, *v] } else { vec![*v] }).collect(); vec![one.clone(), one] }).collect();
        if rle {
            let packed: Vec<Vec<u8>> = rows.iter().map(|r| { let half = r.len() / 2; vec![(257 - half) as u8, r[0], (half - 1) as u8].into_iter().chain(r[half..].iter().copied()).collect() }).collect();
            for p in &packed { if big { f.extend_from_slice(&(p.len() as u32).to_be_bytes()) } else { f.extend_from_slice(&(p.len() as u16).to_be_bytes()) } }
            for p in packed { f.extend_from_slice(&p); }
        } else { for r in rows { f.extend_from_slice(&r); } }
        f
    }
    #[test]
    fn the_flattened_picture_is_read_raw_and_packed_from_both_file_kinds_and_reduced() {
        let dir = std::env::temp_dir().join(format!("mn-psd-test-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        for (i, (big, rle, depth)) in [(false, false, 8), (false, true, 8), (true, true, 8), (true, false, 16), (false, true, 16)].into_iter().enumerate() {
            let p = dir.join(format!("{i}.psd")); std::fs::write(&p, file(big, rle, depth, true)).unwrap();
            assert_eq!(size(&p), Ok((4, 2)));
            let f = load(&p, 100).unwrap_or_else(|e| panic!("{big} {rle} {depth}: {e}")); assert_eq!((f.w, f.h, f.full), (4, 2, (4, 2)));
            assert_eq!(&f.rgba[..4], &[255, 0, 0, 255]); assert_eq!(&f.rgba[12..16], &[0, 0, 255, 255]);
            let small = load(&p, 2).unwrap(); assert_eq!((small.w, small.h), (2, 1)); assert_eq!(small.rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
        }
        let p = dir.join("no.psd"); std::fs::write(&p, file(false, false, 8, false)).unwrap();
        assert!(load(&p, 100).is_err_and(|e| e.contains("Maximize Compatibility")));
        std::fs::write(&p, b"8BPX").unwrap(); assert!(load(&p, 100).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
