//! macOS: the system's thumbnails (QLThumbnailGenerator) and icons (NSWorkspace), as RGBA (premultiplied, top row first).
//! Called from a helper's main thread, which has no run loop and no NSApplication.
use crate::helper::Img;
use crate::proto::{FAILED, TIMEOUT};
use block2::RcBlock;
use objc2::AllocAnyThread;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSImage, NSWorkspace};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGInterpolationQuality};
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_quick_look_thumbnailing::{QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes as Types, QLThumbnailGenerator, QLThumbnailRepresentation};
use objc2_uniform_type_identifiers::UTType;
use std::sync::mpsc::channel;
use std::time::Duration;

type R = Result<Img, (u8, i64)>;

/// Draw a CGImage into our own RGBA buffer of w x h.
fn rgba(img: &CGImage, w: usize, h: usize) -> Option<Img> {
    if w == 0 || h == 0 { return None }
    let mut buf = vec![0u8; w * h * 4];
    let cs = CGColorSpace::new_device_rgb()?; // the product: the sRGB space by name
    let ctx = unsafe { CGBitmapContextCreate(buf.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&cs), CGImageAlphaInfo::PremultipliedLast.0) }?;
    CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::High);
    CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(img));
    drop(ctx);
    Some(Img { w: w as u32, h: h as u32, rgba: buf })
}

/// The system's thumbnail of a file, fitted in `px`. The request is asynchronous: this thread waits for its answer
/// for `soft` at most, then CANCELS the request (`cancelRequest:`), and waits - again with a bound - for the
/// completion handler, which the system always calls. Nothing is blocked without a bound and nothing is abandoned.
/// `icon`: ask for the icon representation instead of the thumbnail (the same asynchronous, cancellable call).
pub fn thumbnail(path: &str, px: u32, soft: Duration, icon: bool) -> R {
    autoreleasepool(|_| unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let types = if icon { Types::Icon } else { Types::Thumbnail };
        let req = QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(QLThumbnailGenerationRequest::alloc(), &url, CGSize::new(px as f64, px as f64), 1.0, types);
        let (tx, rx) = channel::<R>();
        let block = RcBlock::new(move |rep: *mut QLThumbnailRepresentation, err: *mut NSError| {
            let r = match rep.as_ref() {
                Some(rep) => { let img = rep.CGImage(); rgba(&img, CGImage::width(Some(&img)).min(px as usize), CGImage::height(Some(&img)).min(px as usize)).ok_or((FAILED, -1)) }
                None => Err((FAILED, err.as_ref().map(|e| e.code() as i64).unwrap_or(-2))),
            };
            let _ = tx.send(r);
        });
        let generator = QLThumbnailGenerator::sharedGenerator();
        generator.generateBestRepresentationForRequest_completionHandler(&req, &block);
        match rx.recv_timeout(soft) {
            Ok(r) => r,
            Err(_) => {
                generator.cancelRequest(&req);
                // Measured: the cancel does not interrupt a generation already running in the system's agent - the
                // handler is called when that work ends. So the wait for it is short, and then this thread moves on:
                // the late call finds the channel closed and its answer is dropped. The work was never on our thread.
                let t = std::time::Instant::now();
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(_) => Err((TIMEOUT, t.elapsed().as_micros() as i64)), // aux = how long the cancel took to be honoured (us)
                    Err(_) => Err((TIMEOUT, -1)),                             // not honoured within 100 ms
                }
            }
        }
    })
}

fn from_nsimage(image: &NSImage, px: u32) -> R {
    let mut rect = objc2_foundation::NSRect::new(CGPoint::new(0.0, 0.0), CGSize::new(px as f64, px as f64));
    let cg = unsafe { image.CGImageForProposedRect_context_hints(&mut rect, None, None) }.ok_or((FAILED, -3))?;
    rgba(&cg, px as usize, px as usize).ok_or((FAILED, -1))
}

/// The icon of THIS file (an application's own icon, a folder's custom icon), synchronous.
pub fn icon(path: &str, px: u32) -> R {
    autoreleasepool(|_| { let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path)); from_nsimage(&image, px) })
}

/// The icon of a file TYPE, from its extension alone (no file is touched). "/" stands for a folder.
pub fn type_icon(ext: &str, px: u32) -> R {
    autoreleasepool(|_| {
        let ut = if ext == "/" { UTType::typeWithIdentifier(&NSString::from_str("public.folder")) } else { UTType::typeWithFilenameExtension(&NSString::from_str(ext)) }.ok_or((FAILED, -4))?;
        let image = NSWorkspace::sharedWorkspace().iconForContentType(&ut);
        from_nsimage(&image, px)
    })
}

/// Probe only (`S18_PROHIBIT=1`): before anything else, the helper tells AppKit it is never to be an application
/// with a Dock icon. This makes an NSApplication object (it is never run).
pub fn prohibit() {
    if std::env::var_os("S18_PROHIBIT").is_none() { return }
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Prohibited);
}
