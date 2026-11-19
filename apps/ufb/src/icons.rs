//! Icons (canon row 463; spike S18): the system's own, per file TYPE in this slice - by extension, plus the folder
//! and the plain file - fetched off the UI thread, kept in memory per (type, pixel size), drawn through vello as
//! images. Until an icon has come, the view draws the theme's neutral placeholder.
//!
//! The seam for the next slice: everything goes through [`Request`] / [`Answer`], keyed by an id. An icon of one FILE
//! and a thumbnail are further kinds of [`What`] answered by helper processes (S18); nothing else changes here.
//!
//! macOS: `NSWorkspace`'s icon for a content type - no file is touched (S18 lesson 6: made in the application itself).
//! Windows: `SHGetFileInfo` for the system image list's index of a name that need not exist, then that list's icon.
//! (S18's binding Windows lesson - the shell is given an absolute path with backslashes, never the `\\?\` form - is for
//! calls that take a real path; a type's icon gives the shell no path at all.)
use crate::lister::{Msg, Port};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

/// What is asked for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum What {
    /// The icon of a type: an extension in lower case, "/" for a folder, "" for a file without one.
    Type(String),
}
pub struct Request { pub id: u64, pub what: What, pub px: u32 }
/// Pixels as RGBA, premultiplied, top row first; none when the system had nothing.
pub struct Answer { pub id: u64, pub image: Option<Img> }
pub struct Img { pub w: u32, pub h: u32, pub rgba: Vec<u8> }

/// A web link's file (`.url`, `.webloc`) is shown with the theme's link glyph in the accent colour, alike on both
/// systems - the system's own icon for one is a blank sheet on macOS and the browser's on Windows (the owner,
/// 2026-11-19). Drawn here from the icon font; none when the font has no such glyph (the system's icon then).
fn link_icon(px: u32) -> Option<Img> {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
    let font = FontRef::try_from_slice(min_theme::fonts::PHOSPHOR).ok()?; let ch = min_theme::icons::glyph("link")?;
    let size = px as f32 * 0.82; let scaled = font.as_scaled(PxScale::from(size));
    let mut g = scaled.scaled_glyph(ch); g.position = ab_glyph::point(0.0, 0.0);
    let outline = font.outline_glyph(g)?; let b = outline.px_bounds();
    let (ox, oy) = (((px as f32 - b.width()) / 2.0).round() as i32, ((px as f32 - b.height()) / 2.0).round() as i32);
    let c = min_theme::tokens::ACCENT_VISUAL_COLOR; let mut rgba = vec![0u8; (px * px * 4) as usize];
    outline.draw(|x, y, cov| { let (x, y) = (x as i32 + ox, y as i32 + oy); if x < 0 || y < 0 || x >= px as i32 || y >= px as i32 { return }
        let a = cov.clamp(0.0, 1.0) * c.a; let i = ((y as u32 * px + x as u32) * 4) as usize;
        // (premultiplied, as every icon here is)
        rgba[i] = (c.r * a * 255.0).round() as u8; rgba[i + 1] = (c.g * a * 255.0).round() as u8; rgba[i + 2] = (c.b * a * 255.0).round() as u8; rgba[i + 3] = (a * 255.0).round() as u8; });
    Some(Img { w: px, h: px, rgba })
}

enum Slot { Waiting, Ready(peniko::ImageBrush), Failed }

pub struct Icons { tx: Sender<Request>, rx: Receiver<Answer>, slots: HashMap<u32, HashMap<String, Slot>>, waiting: HashMap<u64, (String, u32)>, next: u64 }

/// The pixel sizes icons are kept at: the one at or above what is needed (a grid's size slider does not ask for a new
/// set at every step).
pub fn bucket(px: f64) -> u32 { [16u32, 32, 64, 128, 256, 512].into_iter().find(|b| *b as f64 >= px - 0.5).unwrap_or(512) }

impl Icons {
    /// Starts the one thread that asks the system. `port` wakes the window's loop when an icon has come.
    pub fn new(port: Port) -> Icons {
        let (tx, jobs) = channel::<Request>(); let (out, rx) = channel::<Answer>();
        let _ = std::thread::Builder::new().name("ufb-icons".into()).spawn(move || {
            sys::init();
            while let Ok(r) = jobs.recv() { let image = match &r.what { What::Type(ext) if ext == "url" || ext == "webloc" => link_icon(r.px).or_else(|| sys::type_icon(ext, r.px)), What::Type(ext) => sys::type_icon(ext, r.px) }; if out.send(Answer { id: r.id, image }).is_err() { break } port.send(Msg::Icon); }
        });
        Icons { tx, rx, slots: HashMap::new(), waiting: HashMap::new(), next: 1 }
    }
    /// The icon of a type at `px` pixels, when it is there; asked for when it is not.
    pub fn get(&mut self, ext: &str, px: u32) -> Option<&peniko::ImageBrush> {
        let by = self.slots.entry(px).or_default();
        if !by.contains_key(ext) {
            by.insert(ext.to_string(), Slot::Waiting); let id = self.next; self.next += 1; self.waiting.insert(id, (ext.to_string(), px));
            let _ = self.tx.send(Request { id, what: What::Type(ext.to_string()), px });
        }
        match self.slots.get(&px)?.get(ext)? { Slot::Ready(img) => Some(img), _ => None }
    }
    /// The icon when it is there; never asks.
    pub fn peek(&self, ext: &str, px: u32) -> Option<&peniko::ImageBrush> { match self.slots.get(&px)?.get(ext)? { Slot::Ready(img) => Some(img), _ => None } }
    /// Take in what has come. True when something did.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(a) = self.rx.try_recv() {
            let Some((ext, px)) = self.waiting.remove(&a.id) else { continue }; any = true;
            let slot = match a.image.filter(|i| i.w > 0 && i.h > 0 && i.rgba.len() == (i.w * i.h * 4) as usize) {
                Some(i) => Slot::Ready(peniko::ImageBrush::new(peniko::ImageData { data: peniko::Blob::new(std::sync::Arc::new(i.rgba)), format: peniko::ImageFormat::Rgba8, alpha_type: peniko::ImageAlphaType::AlphaPremultiplied, width: i.w, height: i.h })),
                None => Slot::Failed };
            self.slots.entry(px).or_default().insert(ext, slot);
        }
        any
    }
    /// Icons asked for that have not come yet.
    pub fn pending(&self) -> usize { self.waiting.len() }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::Img;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSImage, NSWorkspace};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGInterpolationQuality};
    use objc2_foundation::NSString;
    use objc2_uniform_type_identifiers::UTType;

    pub fn init() {}
    /// Draw a CGImage into our own RGBA buffer of w x h, in sRGB (S18: the product names the space).
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
    fn from_nsimage(image: &NSImage, px: u32) -> Option<Img> {
        let mut rect = objc2_foundation::NSRect::new(CGPoint::new(0.0, 0.0), CGSize::new(px as f64, px as f64));
        let cg = unsafe { image.CGImageForProposedRect_context_hints(&mut rect, None, None) }?;
        rgba(&cg, px as usize, px as usize)
    }
    /// The icon of a file TYPE, from its extension alone (no file is touched). "/" a folder, "" a plain file.
    pub fn type_icon(ext: &str, px: u32) -> Option<Img> {
        autoreleasepool(|_| {
            let ut = match ext { "/" => UTType::typeWithIdentifier(&NSString::from_str("public.folder")), "" => UTType::typeWithIdentifier(&NSString::from_str("public.data")), _ => UTType::typeWithFilenameExtension(&NSString::from_str(ext)) }
                .or_else(|| UTType::typeWithIdentifier(&NSString::from_str("public.data")))?;
            let image = NSWorkspace::sharedWorkspace().iconForContentType(&ut);
            from_nsimage(&image, px)
        })
    }
}

#[cfg(windows)]
mod sys {
    //! Written on macOS from spike S18's `sys_win.rs` (which ran on Windows); type-checked, to be seen on the box.
    use super::Img;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
    use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    /// Once, on the thread that makes the shell calls.
    pub fn init() { unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE); } }
    /// A bitmap's pixels as top-down RGBA. The shell's 32-bit bitmaps are premultiplied BGRA; a bitmap with no alpha
    /// anywhere (an old icon) is made opaque.
    unsafe fn pixels(hbmp: HBITMAP) -> Option<Img> { unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 { return None }
        let (w, h) = (bm.bmWidth.max(0) as usize, bm.bmHeight.unsigned_abs() as usize); if w == 0 || h == 0 { return None }
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w as i32, biHeight: -(h as i32), biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() };
        let mut buf = vec![0u8; w * h * 4];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        if lines == 0 { return None }
        let opaque = buf.chunks_exact(4).all(|p| p[3] == 0);
        for p in buf.chunks_exact_mut(4) { p.swap(0, 2); if opaque { p[3] = 255 } }
        Some(Img { w: w as u32, h: h as u32, rgba: buf })
    } }
    /// A small icon in the jumbo list sits in the top-left corner of 256 x 256: the picture is cut to what is drawn.
    fn cut(img: Img) -> Img {
        let (w, h) = (img.w as usize, img.h as usize); let (mut mx, mut my) = (0usize, 0usize);
        for y in 0..h { for x in 0..w { if img.rgba[(y * w + x) * 4 + 3] != 0 { mx = mx.max(x + 1); my = my.max(y + 1); } } }
        let side = [16usize, 24, 32, 48, 64].into_iter().find(|s| mx <= *s && my <= *s); let Some(side) = side.filter(|s| *s * 2 <= w && *s <= h) else { return img };
        let mut out = Vec::with_capacity(side * side * 4); for y in 0..side { out.extend_from_slice(&img.rgba[y * w * 4..(y * w + side) * 4]); }
        Img { w: side as u32, h: side as u32, rgba: out }
    }
    /// The icon of a file TYPE from its extension alone: the system image list's index for a name that need not exist
    /// (SHGFI_USEFILEATTRIBUTES), then that icon from the jumbo (256), extra-large (48), large (32) or small (16) list.
    pub fn type_icon(ext: &str, px: u32) -> Option<Img> { unsafe {
        let (name, attrs) = match ext { "/" => (HSTRING::from("folder"), FILE_ATTRIBUTE_DIRECTORY), "" => (HSTRING::from("x"), FILE_ATTRIBUTE_NORMAL), _ => (HSTRING::from(format!("x.{ext}")), FILE_ATTRIBUTE_NORMAL) };
        let mut info = SHFILEINFOW::default();
        if SHGetFileInfoW(PCWSTR(name.as_ptr()), attrs, Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES) == 0 { return None }
        let which = if px > 48 { SHIL_JUMBO } else if px > 32 { SHIL_EXTRALARGE } else if px > 16 { SHIL_LARGE } else { SHIL_SMALL };
        let list: IImageList = SHGetImageList(which as i32).ok()?;
        let hicon = list.GetIcon(info.iIcon, ILD_TRANSPARENT.0).ok()?;
        let mut ii = ICONINFO::default();
        let r = match GetIconInfo(hicon, &mut ii) { Ok(()) => { let r = pixels(ii.hbmColor); let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0)); let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0)); r } Err(_) => None };
        let _ = DestroyIcon(hicon);
        r.map(cut)
    } }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod sys { use super::Img; pub fn init() {} pub fn type_icon(_ext: &str, _px: u32) -> Option<Img> { None } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icons_are_kept_at_a_few_sizes() {
        assert_eq!(bucket(16.0), 16); assert_eq!(bucket(32.0), 32); assert_eq!(bucket(33.0), 64); assert_eq!(bucket(160.0), 256); assert_eq!(bucket(512.0), 512); assert_eq!(bucket(900.0), 512); assert_eq!(bucket(1.0), 16);
    }
    #[test]
    fn an_icon_is_asked_for_once_and_comes_through_the_channel() {
        let (tx, rx) = channel(); let port = Port { tx, wake: std::sync::Arc::new(|| {}) };
        let mut icons = Icons::new(port);
        assert!(icons.get("/", 32).is_none(), "not there yet: the placeholder is drawn"); assert!(icons.get("/", 32).is_none()); assert_eq!(icons.pending(), 1, "asked for once");
        assert!(icons.peek("exr", 32).is_none()); assert_eq!(icons.pending(), 1, "a look does not ask");
        assert!(matches!(rx.recv_timeout(std::time::Duration::from_secs(30)), Ok(Msg::Icon)), "the loop is woken");
        assert!(icons.poll()); assert_eq!(icons.pending(), 0);
        // on the systems this slice draws icons for, a folder has one, at the size asked for
        #[cfg(target_os = "macos")] { let img = icons.get("/", 32).expect("the folder's icon"); assert_eq!((img.image.width, img.image.height), (32, 32)); }
    }
}
