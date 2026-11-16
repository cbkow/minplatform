//! Pictures the SYSTEM decodes (macOS ImageIO; canon row 313): HEIC / HEIF and the cameras' RAW formats the installed
//! macOS knows. The picture is asked for at a longest side, upright (the file's orientation applied), and drawn into
//! sRGB pixels.
use objc2_core_foundation::{CFDictionary, CFNumber, CFString, CFType, CFURL, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo};
use objc2_image_io::{kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize, CGImageSource};
use std::path::Path;

/// Straight-alpha RGBA, its longer side at most `max`.
pub fn load(path: &Path, max: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let url = CFURL::from_file_path(path).ok_or("the file's path cannot be given to the system")?;
    let src = unsafe { CGImageSource::with_url(&url, None) }.ok_or("the system cannot open this file as a picture")?;
    let (yes, size) = (objc2_core_foundation::CFBoolean::new(true), CFNumber::new_i32(max as i32));
    let keys: [&CFString; 3] = unsafe { [kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize] };
    let values: [&CFType; 3] = [yes.as_ref(), yes.as_ref(), size.as_ref()];
    let opts = CFDictionary::from_slices(&keys, &values);
    let img = unsafe { src.thumbnail_at_index(0, Some(opts.as_opaque())) }.ok_or("the system has no decoder for this file")?;
    let (w, h) = (CGImage::width(Some(&img)), CGImage::height(Some(&img)));
    if w == 0 || h == 0 { return Err("the system gave an empty picture".into()) }
    let mut rgba = vec![0u8; w * h * 4];
    let space = unsafe { CGColorSpace::with_name(Some(objc2_core_graphics::kCGColorSpaceSRGB)) }.ok_or("no sRGB colour space")?;
    let ctx = unsafe { CGBitmapContextCreate(rgba.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), CGImageAlphaInfo::PremultipliedLast.0) }.ok_or("the system could not draw the picture")?;
    CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(&img));
    drop(ctx);
    // drawn with its colour multiplied by its alpha: divided out
    for px in rgba.chunks_exact_mut(4) { let a = px[3] as u32; if a > 0 && a < 255 { for c in &mut px[..3] { *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8; } } }
    Ok((rgba, w as u32, h as u32))
}
