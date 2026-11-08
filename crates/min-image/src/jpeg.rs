//! JPEG — QCView's `JPEGLoader::load` (`decode/jpeg_image_loader.cpp`) through libjpeg-turbo's TurboJPEG 3 API, at
//! libjpeg's defaults (islow IDCT, fancy upsampling — `jpeg_read_header`'s, so the pixels are QCView's): 8-bit RGB and
//! grayscale only (CMYK / YCCK refused rather than guess the colour space — QCView's rule), always opaque, always U8.
//!
//! Divergence, with reason: QCView's libjpeg error manager silences messages but never longjmps, so a corrupt file
//! reaches libjpeg's default `error_exit` — the process exits. TurboJPEG returns errors; a corrupt file → a broken frame.
use crate::{sys::tj::*, Error, Image, Sample, Samples};
use std::ffi::CStr;

struct Handle(tjhandle);
impl Drop for Handle { fn drop(&mut self) { unsafe { tj3Destroy(self.0) } } }

fn header(path: &str, bytes: &[u8]) -> Result<(Handle, u32, u32, bool), Error> {
    #[cfg(tj_init_version)] let h = Handle(unsafe { tj3InitVersion(TJINIT_DECOMPRESS as _, TURBOJPEG_VERSION_NUMBER as _) });
    #[cfg(not(tj_init_version))] let h = Handle(unsafe { tj3Init(TJINIT_DECOMPRESS as _) });
    if h.0.is_null() { return Err(Error::Decode(path.into(), "tj3Init failed".into())) }
    if unsafe { tj3DecompressHeader(h.0, bytes.as_ptr(), bytes.len()) } != 0 { return Err(err(path, &h)) }
    let get = |p: u32| unsafe { tj3Get(h.0, p as _) };
    let (w, ht, cs, precision) = (get(TJPARAM_JPEGWIDTH as _), get(TJPARAM_JPEGHEIGHT as _), get(TJPARAM_COLORSPACE as _), get(TJPARAM_PRECISION as _));
    if precision != 8 { return Err(Error::Decode(path.into(), format!("{precision}-bit JPEG (QCView reads 8-bit)"))) }
    #[allow(non_upper_case_globals)]
    let gray = match cs as _ { TJCS_GRAY => true, TJCS_YCbCr | TJCS_RGB => false, _ => return Err(Error::Decode(path.into(), "CMYK / YCCK JPEG (QCView reads RGB and gray only)".into())) };
    Ok((h, w as u32, ht as u32, gray))
}
fn err(path: &str, h: &Handle) -> Error { Error::Decode(path.into(), unsafe { CStr::from_ptr(tj3GetErrorStr(h.0)) }.to_string_lossy().into_owned()) }

pub fn load(path: &str) -> Result<Image, Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let (h, w, ht, gray) = header(path, &bytes)?;
    let ch = if gray { 1 } else { 3 };
    let mut px = vec![0u8; w as usize * ht as usize * ch];
    let fmt = if gray { TJPF_GRAY } else { TJPF_RGB };
    if unsafe { tj3Decompress8(h.0, bytes.as_ptr(), bytes.len(), px.as_mut_ptr(), (w as usize * ch) as i32, fmt as _) } != 0 { return Err(err(path, &h)) }
    Ok(Image::from_interleaved(w, ht, Sample::U8, Samples::U8(&px), ch, !gray, false))
}

pub fn dimensions(path: &str) -> Result<(u32, u32), Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let (_, w, h, _) = header(path, &bytes)?; Ok((w, h))
}
