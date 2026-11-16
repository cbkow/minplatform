//! Windows: the shell's thumbnail of a file and the icon of THIS file (spike S18, run on Windows 11).
//! `IShellItemImageFactory::GetImage` is synchronous and cannot be cancelled: the call is made on the HELPER's main
//! thread and its bound is the application's watchdog - at the request's deadline the helper PROCESS is ended and
//! replaced; nothing is abandoned inside a process that lives on. (`soft` is therefore unused here.)
use super::helper::Img;
use super::proto::FAILED;
use std::time::Duration;
use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
use windows::Win32::UI::Shell::*;

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

/// A path as the shell's parser takes it (S18's binding lesson): ABSOLUTE, with backslashes, and NOT the `\\?\` form
/// that `std::fs::canonicalize` returns - each of the others is E_INVALIDARG.
pub fn shell_path(path: &str) -> String {
    let abs = std::path::absolute(path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| path.to_string());
    let abs = abs.replace('/', "\\");
    if let Some(r) = abs.strip_prefix("\\\\?\\UNC\\") { return format!("\\\\{r}") }
    abs.strip_prefix("\\\\?\\").map(str::to_string).unwrap_or(abs)
}

fn get_image(path: &str, px: u32, flags: SIIGBF) -> R { unsafe {
    let path = &shell_path(path);
    let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).map_err(|e| (FAILED, e.code().0 as i64))?;
    let hbmp = factory.GetImage(SIZE { cx: px as i32, cy: px as i32 }, flags).map_err(|e| (FAILED, e.code().0 as i64))?;
    let r = pixels(hbmp);
    let _ = DeleteObject(HGDIOBJ(hbmp.0));
    r
} }

/// The shell's thumbnail only (a file with no handler fails: no icon is substituted); the shell scales to fit `px`.
pub fn thumbnail(path: &str, px: u32, _soft: Duration) -> R { get_image(path, px, SIIGBF_THUMBNAILONLY) }
/// The icon of THIS file (an executable's own icon, a shortcut's).
pub fn icon(path: &str, px: u32, _soft: Duration) -> R { get_image(path, px, SIIGBF_ICONONLY) }
