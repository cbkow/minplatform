//! macOS: the system's thumbnail of a file and the icon of THIS file, both through QuickLook's generator (spike S18:
//! asynchronous, bounded, and - unlike `NSWorkspace` - it does not register the helper with the system as a second
//! copy of the application). Called from a helper's main thread, which has no run loop and no NSApplication.
use super::helper::Img;
use super::proto::{FAILED, TIMEOUT};
use block2::RcBlock;
use objc2::rc::autoreleasepool;
use objc2::AllocAnyThread;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGInterpolationQuality};
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_quick_look_thumbnailing::{QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes as Types, QLThumbnailGenerator, QLThumbnailRepresentation};
use std::sync::mpsc::channel;
use std::time::Duration;

type R = Result<Img, (u8, i64)>;

pub fn init() {}

/// Draw a CGImage into our own RGBA buffer of w x h, in sRGB.
fn rgba(img: &CGImage, w: usize, h: usize) -> Option<Img> {
    if w == 0 || h == 0 { return None }
    let mut buf = vec![0u8; w * h * 4];
    let cs = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).or_else(CGColorSpace::new_device_rgb)?;
    let ctx = unsafe { CGBitmapContextCreate(buf.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&cs), CGImageAlphaInfo::PremultipliedLast.0) }?;
    CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::High);
    CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(img));
    drop(ctx);
    Some(Img { w: w as u32, h: h as u32, rgba: buf })
}

/// The request is asynchronous: this thread waits for its answer for `soft` at most, then cancels the request and
/// stops waiting (S18: the cancel is a hint - it does not stop a generation already running in the system's agent;
/// the late answer finds the channel closed and is dropped). Nothing is blocked without a bound.
fn generate(path: &str, px: u32, soft: Duration, types: Types) -> R {
    autoreleasepool(|_| unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let req = QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(QLThumbnailGenerationRequest::alloc(), &url, CGSize::new(px as f64, px as f64), 1.0, types);
        let (tx, rx) = channel::<R>();
        let block = RcBlock::new(move |rep: *mut QLThumbnailRepresentation, err: *mut NSError| {
            let r = match rep.as_ref() {
                Some(rep) => { let img = rep.CGImage(); let (w, h) = (CGImage::width(Some(&img)), CGImage::height(Some(&img))); let k = (px as f64 / w.max(h).max(1) as f64).min(1.0);
                    rgba(&img, ((w as f64 * k).round() as usize).max(1), ((h as f64 * k).round() as usize).max(1)).ok_or((FAILED, -1)) }
                None => Err((FAILED, err.as_ref().map(|e| e.code() as i64).unwrap_or(-2))),
            };
            let _ = tx.send(r);
        });
        let generator = QLThumbnailGenerator::sharedGenerator();
        generator.generateBestRepresentationForRequest_completionHandler(&req, &block);
        match rx.recv_timeout(soft) { Ok(r) => r, Err(_) => { generator.cancelRequest(&req); Err((TIMEOUT, 0)) } }
    })
}
/// The system's thumbnail of a file, fitted in `px`; a file the system has no thumbnail for fails in a few ms.
pub fn thumbnail(path: &str, px: u32, soft: Duration) -> R { generate(path, px, soft, Types::Thumbnail) }
/// The icon of THIS file (an application's own icon, a folder's custom one).
pub fn icon(path: &str, px: u32, soft: Duration) -> R { generate(path, px, soft, Types::Icon) }
