//! PNG — QCView's `PNGLoader::load` (`decode/png_image_loader.cpp`): palette → RGB, tRNS → alpha, low bit depths and
//! gray → 8-bit RGB, 16-bit samples kept (big-endian on disk → host order), 8-bit out = U8, 16-bit out = U16.
//!
//! Divergences, with reasons: QCView appends alpha only for 3-channel and 8-bit 1-channel sources, so a 16-bit GRAY
//! PNG (and a 1/2/4-bit gray one, whose depth it reads before expansion) came out 3-channel under an RGBA16 label —
//! garbage. Here every PNG lands as planes, alpha carried only when the file has it (opaque otherwise, as QCView's
//! synthesised 0xFF / 0xFFFF).
use crate::{Error, Image, Sample, Samples};
use png::{BitDepth, ColorType, Transformations};

pub fn load(path: &str) -> Result<Image, Error> {
    let f = std::fs::File::open(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(Transformations::EXPAND); // palette → RGB, tRNS → alpha, < 8 bit → 8 (QCView's four calls)
    let dec_err = |e: png::DecodingError| Error::Decode(path.into(), e.to_string());
    let mut reader = dec.read_info().map_err(dec_err)?;
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or_else(|| Error::Decode(path.into(), "image too large".into()))?];
    let info = reader.next_frame(&mut buf).map_err(dec_err)?;
    let (color, alpha, channels) = match info.color_type {
        ColorType::Grayscale => (false, false, 1), ColorType::GrayscaleAlpha => (false, true, 2),
        ColorType::Rgb => (true, false, 3), ColorType::Rgba => (true, true, 4),
        ColorType::Indexed => return Err(Error::Decode(path.into(), "palette survived expansion".into())),
    };
    let (w, h) = (info.width, info.height);
    let line = info.line_size;
    Ok(match info.bit_depth {
        BitDepth::Sixteen => {
            let mut v = Vec::with_capacity(w as usize * h as usize * channels);
            for y in 0..h as usize { v.extend(buf[y * line..y * line + w as usize * channels * 2].as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c))) }
            Image::from_interleaved(w, h, Sample::U16, Samples::U16(&v), channels, color, alpha)
        }
        _ => {
            if line != w as usize * channels { buf = (0..h as usize).flat_map(|y| buf[y * line..y * line + w as usize * channels].to_vec()).collect() }
            Image::from_interleaved(w, h, Sample::U8, Samples::U8(&buf), channels, color, alpha)
        }
    })
}

/// Width and height without decoding pixels (QCView's `getDimensions`).
pub fn dimensions(path: &str) -> Result<(u32, u32), Error> {
    let f = std::fs::File::open(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let r = png::Decoder::new(std::io::BufReader::new(f)).read_info().map_err(|e| Error::Decode(path.into(), e.to_string()))?;
    let i = r.info(); Ok((i.width, i.height))
}
