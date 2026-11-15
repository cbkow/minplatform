//! Windows: the shell's thumbnails and icons as RGBA (premultiplied, top row first).
//! WRITTEN ON macOS AND NEVER RUN. It has only been type-checked if the verdict says so.
//!
//! `IShellItemImageFactory::GetImage` is synchronous and cannot be cancelled. The old application bounded it by
//! abandoning the calling thread at 10 s. Here the call is made on the HELPER's main thread and its bound is the
//! parent's watchdog: at the request's deadline the helper PROCESS is killed and replaced - nothing is abandoned
//! inside a process that lives on. (`soft` is therefore unused here.)
use crate::helper::Img;
use crate::proto::FAILED;
use std::time::Duration;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};
use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};
use windows::core::{HSTRING, PCWSTR};

type R = Result<Img, (u8, i64)>;

/// Once per helper, on the thread that makes the shell calls: thumbnail handlers are apartment-threaded COM objects.
pub fn init() { unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE); } }

/// A bitmap's pixels as top-down RGBA. The shell's 32-bit bitmaps are premultiplied BGRA; a bitmap with no alpha
/// anywhere (an old handler) is made opaque.
unsafe fn pixels(hbmp: HBITMAP) -> R { unsafe {
    let mut bm = BITMAP::default();
    if GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 { return Err((FAILED, -1)) }
    let (w, h) = (bm.bmWidth.max(0) as usize, bm.bmHeight.unsigned_abs() as usize);
    if w == 0 || h == 0 { return Err((FAILED, -1)) }
    let mut info = BITMAPINFO::default();
    info.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w as i32, biHeight: -(h as i32), biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() };
    let mut buf = vec![0u8; w * h * 4];
    let dc = CreateCompatibleDC(None);
    let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
    let _ = DeleteDC(dc);
    if lines == 0 { return Err((FAILED, -2)) }
    let opaque = buf.chunks_exact(4).all(|p| p[3] == 0);
    for p in buf.chunks_exact_mut(4) { p.swap(0, 2); if opaque { p[3] = 255 } }
    Ok(Img { w: w as u32, h: h as u32, rgba: buf })
} }

fn get_image(path: &str, px: u32, flags: SIIGBF) -> R { unsafe {
    let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).map_err(|e| (FAILED, e.code().0 as i64))?;
    let hbmp = factory.GetImage(SIZE { cx: px as i32, cy: px as i32 }, flags).map_err(|e| (FAILED, e.code().0 as i64))?;
    let r = pixels(hbmp);
    let _ = DeleteObject(HGDIOBJ(hbmp.0));
    r
} }

/// The shell's thumbnail only (a file with no handler fails with WTS_E_FAILEDEXTRACTION / E_FAIL: no icon is
/// substituted). SIIGBF_BIGGERSIZEOK is NOT set: the shell scales to fit `px`.
pub fn thumbnail(path: &str, px: u32, _soft: Duration, icon_only: bool) -> R {
    get_image(path, px, if icon_only { SIIGBF_ICONONLY } else { SIIGBF_THUMBNAILONLY })
}

/// The icon of THIS file (an executable's own icon, a folder with a custom icon, a shortcut).
pub fn icon(path: &str, px: u32) -> R { get_image(path, px, SIIGBF_ICONONLY) }

/// The icon of a file TYPE from its extension alone: the system image list's index for a name that need not exist
/// (SHGFI_USEFILEATTRIBUTES), then that icon from the jumbo (256), extra-large (48), large (32) or small (16) list.
/// "/" stands for a folder.
pub fn type_icon(ext: &str, px: u32) -> R { unsafe {
    let (name, attrs) = if ext == "/" { (HSTRING::from("folder"), FILE_ATTRIBUTE_DIRECTORY) } else { (HSTRING::from(format!("x.{ext}")), FILE_ATTRIBUTE_NORMAL) };
    let mut info = SHFILEINFOW::default();
    if SHGetFileInfoW(PCWSTR(name.as_ptr()), attrs, Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES) == 0 { return Err((FAILED, -3)) }
    let which = if px > 48 { SHIL_JUMBO } else if px > 32 { SHIL_EXTRALARGE } else if px > 16 { SHIL_LARGE } else { SHIL_SMALL };
    let list: IImageList = SHGetImageList(which as i32).map_err(|e| (FAILED, e.code().0 as i64))?;
    let hicon = list.GetIcon(info.iIcon, ILD_TRANSPARENT.0).map_err(|e| (FAILED, e.code().0 as i64))?;
    let mut ii = ICONINFO::default();
    let r = match GetIconInfo(hicon, &mut ii) {
        Ok(()) => { let r = pixels(ii.hbmColor); let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0)); let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0)); r }
        Err(e) => Err((FAILED, e.code().0 as i64)),
    };
    let _ = DestroyIcon(hicon);
    r // the product scales this to `px` itself (a jumbo list entry is 256 px; small icons sit in its top-left corner)
} }
