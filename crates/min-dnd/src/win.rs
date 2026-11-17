//! Windows: the drop target and the drag source, in OLE, on a window winit made.
//!
//! From spike S16: its self-test passed on Windows 11 (2026-11-15); a real drag by hand is still to be done there.
//!
//! COEXISTENCE (question d). A window has exactly ONE drop target. winit registers its own unless the window is made
//! with `with_drag_and_drop(false)` (the application does that); ours is then registered with `RegisterDragDrop`. OLE must be
//! initialised on the thread (`OleInitialize`: single-threaded apartment - winit does the same, a second call is fine).
//!
//! The drag OUT is `DoDragDrop`, which does NOT return until the drag is over: it runs its own loop, inside our
//! handling of winit's pointer event. winit's loop does not turn meanwhile, so while the pointer is over one of OUR
//! windows the drop target's methods are called from inside that loop and must draw the frame themselves (the
//! application's sink does). The data object is the SHELL's own for the dragged items (`IShellItemArray` ->
//! `BHID_DataObject`): it carries CF_HDROP, the shell's id list and the file names as Explorer's own drags do, takes
//! any format we set on it (our "this drag is mine" mark, the preferred effect) and the ones the target sets back
//! (the performed effect), and holds the drag picture for the shell's helper.
use crate::*;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use windows::core::{implement, Ref, BOOL, HRESULT, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{CreateDIBSection, DeleteObject, ScreenToClient, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS};
use windows::Win32::System::Com::{CoCreateInstance, IDataObject, IStream, CLSCTX_INPROC_SERVER, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL, TYMED_ISTREAM};
use windows::Win32::System::DataExchange::{GetClipboardFormatNameW, RegisterClipboardFormatW};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE};
use windows::Win32::System::SystemServices::{MODIFIERKEYS_FLAGS, MK_CONTROL, MK_SHIFT};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{DragQueryFileW, IDragSourceHelper, IDropTargetHelper, ILFree, SHCreateShellItemArrayFromIDLists, SHParseDisplayName, BHID_DataObject, CFSTR_FILECONTENTS, CFSTR_FILEDESCRIPTORW, CFSTR_INETURLW, CFSTR_LOGICALPERFORMEDDROPEFFECT, CFSTR_PERFORMEDDROPEFFECT, CFSTR_PREFERREDDROPEFFECT, CLSID_DragDropHelper, FILEDESCRIPTORW, HDROP, SHDRAGIMAGE};

/// Our own format on the data object: the process and the drag's serial - "this drag is mine", also across two processes of ours.
const MARK: PCWSTR = windows::core::w!("minplatform.dnd.drag");
const MK_LBUTTON_BIT: u32 = 0x1; const MK_ALT_BIT: u32 = 0x20;
thread_local! {
    static SERIAL: Cell<u64> = const { Cell::new(0) };
    static TARGETS: RefCell<Vec<(usize, IDropTarget)>> = const { RefCell::new(Vec::new()) };
    /// for the self-test: the last event the drop target made (phase, files, x, y)
    static LAST: RefCell<Option<(Phase, usize, f64, f64)>> = const { RefCell::new(None) };
}
fn hwnd_of(window: &dyn winit31::window::Window) -> Option<HWND> { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().ok()?.as_raw() { RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut c_void)), _ => None } }
fn cf(name: PCWSTR) -> u16 { unsafe { RegisterClipboardFormatW(name) as u16 } }
fn fmt(format: u16, tymed: u32, index: i32) -> FORMATETC { FORMATETC { cfFormat: format, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: index, tymed } }
fn from_effect(e: DROPEFFECT) -> u32 { let mut o = 0; if e.0 & DROPEFFECT_COPY.0 != 0 { o |= COPY; } if e.0 & DROPEFFECT_MOVE.0 != 0 { o |= MOVE; } if e.0 & DROPEFFECT_LINK.0 != 0 { o |= LINK; } o }
fn to_effect(o: u32) -> DROPEFFECT { let mut e = 0; if o & COPY != 0 { e |= DROPEFFECT_COPY.0; } if o & MOVE != 0 { e |= DROPEFFECT_MOVE.0; } if o & LINK != 0 { e |= DROPEFFECT_LINK.0; } DROPEFFECT(e) }
/// Explorer's own rule for the keys: Ctrl copies, Shift moves, Ctrl+Shift or Alt links; none leaves the source's whole offer.
fn narrowed(keys: MODIFIERKEYS_FLAGS, source: DROPEFFECT) -> u32 { let src = from_effect(source); let (ctrl, shift, alt) = (keys.0 & MK_CONTROL.0 != 0, keys.0 & MK_SHIFT.0 != 0, keys.0 & MK_ALT_BIT != 0);
    if (ctrl && shift) || alt { src & LINK } else if ctrl { src & COPY } else if shift { src & MOVE } else { src } }
fn keys_text(keys: MODIFIERKEYS_FLAGS) -> String { let mut v = Vec::new(); if keys.0 & MK_CONTROL.0 != 0 { v.push("ctrl"); } if keys.0 & MK_SHIFT.0 != 0 { v.push("shift"); } if keys.0 & MK_ALT_BIT != 0 { v.push("alt"); } if v.is_empty() { "-".into() } else { v.join("+") } }
/// A path as the shell takes it: `std::fs::canonicalize` gives `\\?\C:\...`, which the shell's parser refuses.
fn wide(p: &Path) -> Vec<u16> { let s = p.to_string_lossy(); let s = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") { format!(r"\\{rest}") } else if let Some(rest) = s.strip_prefix(r"\\?\") { rest.to_string() } else { s.to_string() };
    std::ffi::OsString::from(s).encode_wide().chain(std::iter::once(0)).collect() }
fn format_name(f: u16) -> String { match f { 1 => "CF_TEXT".into(), 2 => "CF_BITMAP".into(), 7 => "CF_OEMTEXT".into(), 8 => "CF_DIB".into(), 13 => "CF_UNICODETEXT".into(), 15 => "CF_HDROP".into(), 16 => "CF_LOCALE".into(), 17 => "CF_DIBV5".into(),
    _ => { let mut buf = [0u16; 128]; let n = unsafe { GetClipboardFormatNameW(f as u32, &mut buf) }; if n > 0 { String::from_utf16_lossy(&buf[..n as usize]) } else { format!("#{f}") } } } }
/// The bytes of a format that travels in global memory.
fn bytes(data: &IDataObject, format: u16) -> Option<Vec<u8>> { unsafe {
    let mut medium = data.GetData(&fmt(format, TYMED_HGLOBAL.0 as u32, -1)).ok()?; let mut out = None;
    if medium.tymed == TYMED_HGLOBAL.0 as u32 { let h = medium.u.hGlobal; let p = GlobalLock(h) as *const u8; if !p.is_null() { out = Some(std::slice::from_raw_parts(p, GlobalSize(h)).to_vec()); let _ = GlobalUnlock(h); } }
    ReleaseStgMedium(&mut medium); out } }
fn set_bytes(data: &IDataObject, format: u16, v: &[u8]) -> bool { unsafe {
    let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, v.len().max(1)) else { return false }; let p = GlobalLock(h) as *mut u8; if p.is_null() { return false } std::ptr::copy_nonoverlapping(v.as_ptr(), p, v.len()); let _ = GlobalUnlock(h);
    // (true: the data object owns the memory from here)
    let medium = STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: h }, pUnkForRelease: ManuallyDrop::new(None) }; data.SetData(&fmt(format, TYMED_HGLOBAL.0 as u32, -1), &medium, true).is_ok() } }
fn dword(data: &IDataObject, name: PCWSTR) -> Option<u32> { bytes(data, cf(name)).filter(|b| b.len() >= 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) }
fn utf16(b: &[u8]) -> String { let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect(); String::from_utf16_lossy(&w) }
/// The names of the files a source promises (a mail attachment, an archive's entry, a browser's image): `FileGroupDescriptorW`.
fn promised_names(data: &IDataObject) -> Vec<String> { let Some(b) = bytes(data, cf(CFSTR_FILEDESCRIPTORW)) else { return Vec::new() }; if b.len() < 4 { return Vec::new() }
    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize; let size = std::mem::size_of::<FILEDESCRIPTORW>();
    (0..n).filter(|i| 4 + (i + 1) * size <= b.len()).map(|i| { let d: FILEDESCRIPTORW = unsafe { std::ptr::read_unaligned(b.as_ptr().add(4 + i * size) as *const FILEDESCRIPTORW) }; let name: [u16; 260] = d.cFileName; let len = name.iter().position(|c| *c == 0).unwrap_or(260); String::from_utf16_lossy(&name[..len]) }).collect() }
fn read(data: &IDataObject) -> Payload { unsafe {
    let mut p = Payload::default();
    if let Ok(en) = data.EnumFormatEtc(DATADIR_GET.0 as u32) { loop { let mut f = [FORMATETC::default()]; let mut got = 0u32; if en.Next(&mut f, Some(&mut got)) != S_OK || got == 0 { break } p.types.push(format_name(f[0].cfFormat)); if p.types.len() > 200 { break } } }
    if let Ok(mut medium) = data.GetData(&fmt(CF_HDROP.0, TYMED_HGLOBAL.0 as u32, -1)) { if medium.tymed == TYMED_HGLOBAL.0 as u32 { let drop = HDROP(medium.u.hGlobal.0); let n = DragQueryFileW(drop, u32::MAX, None);
            for i in 0..n { let len = DragQueryFileW(drop, i, None) as usize; let mut buf = vec![0u16; len + 1]; DragQueryFileW(drop, i, Some(&mut buf)); p.paths.push(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len]))); } }
        ReleaseStgMedium(&mut medium); }
    if let Some(b) = bytes(data, cf(CFSTR_INETURLW)) { let u = utf16(&b); if !u.is_empty() { p.urls.push(u); } }
    if let Some(b) = bytes(data, CF_UNICODETEXT.0) { p.text = Some(utf16(&b)); }
    p.promised = promised_names(data);
    p } }
/// Our mark on a data object: (process, serial).
fn mark_of(data: &IDataObject) -> Option<(u32, u64)> { let b = bytes(data, cf(MARK))?; if b.len() < 12 { return None } Some((u32::from_le_bytes([b[0], b[1], b[2], b[3]]), u64::from_le_bytes(b[4..12].try_into().ok()?))) }
/// Promised files' contents (`FileContents`, one index each), written under `dest`. Read HERE, inside the drop: the
/// source may tear the data down when `Drop` returns (the real thing reads on a worker with IDataObjectAsyncCapability
/// when the source offers it, so that a large attachment does not hold both applications).
fn receive(data: &IDataObject, names: &[String], win: usize, dest: &Path, sink: &Sink) { let _ = std::fs::create_dir_all(dest); let contents = cf(CFSTR_FILECONTENTS);
    for (i, name) in names.iter().enumerate() { let leaf: String = name.rsplit(['\\', '/']).next().unwrap_or("promised").chars().map(|c| if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect(); let to = dest.join(if leaf.is_empty() { "promised".into() } else { leaf });
        let result: Result<PathBuf, String> = unsafe { match data.GetData(&fmt(contents, (TYMED_ISTREAM.0 | TYMED_HGLOBAL.0) as u32, i as i32)) {
            Err(e) => Err(format!("{name}: GetData(FileContents, {i}) failed: {e}")),
            Ok(mut medium) => { let mut body: Option<Vec<u8>> = None;
                if medium.tymed == TYMED_ISTREAM.0 as u32 { let stream: Option<IStream> = (*medium.u.pstm).clone(); if let Some(stream) = stream { let mut all = Vec::new(); let mut buf = vec![0u8; 1 << 16];
                        loop { let mut got = 0u32; let hr = stream.Read(buf.as_mut_ptr() as *mut c_void, buf.len() as u32, Some(&mut got)); if hr.is_err() || got == 0 { break } all.extend_from_slice(&buf[..got as usize]); } body = Some(all); } }
                else if medium.tymed == TYMED_HGLOBAL.0 as u32 { let h = medium.u.hGlobal; let p = GlobalLock(h) as *const u8; if !p.is_null() { body = Some(std::slice::from_raw_parts(p, GlobalSize(h)).to_vec()); let _ = GlobalUnlock(h); } }
                ReleaseStgMedium(&mut medium);
                match body { Some(b) => std::fs::write(&to, b).map(|_| to.clone()).map_err(|e| format!("{name}: {e}")), None => Err(format!("{name}: the contents came in a medium we do not read")) } } } };
        sink(Notice::Promised { win, result }); } }

// --- the destination ---
#[implement(IDropTarget)]
struct DropTarget { hwnd: HWND, win: usize, sink: Sink, dest: PathBuf, helper: Option<IDropTargetHelper>, /// what came in with the drag: read at the enter, used by the moves (`DragOver` is not handed the data object)
    seen: RefCell<Option<(Rc<Payload>, Option<u64>, String)>> }
impl DropTarget_Impl {
    /// A screen point (what OLE hands over) as physical pixels in the window's client area.
    fn client(&self, pt: &POINTL) -> (f64, f64) { let mut p = POINT { x: pt.x, y: pt.y }; unsafe { let _ = ScreenToClient(self.hwnd, &mut p); } (p.x as f64, p.y as f64) }
    fn look(&self, data: &IDataObject) { let payload = Rc::new(read(data)); let mark = mark_of(data); let ours = mark.filter(|(pid, _)| *pid == std::process::id()).map(|(_, serial)| serial);
        let from = match (ours, mark) { (Some(_), _) => "our own drag".to_string(), (None, Some((pid, _))) => format!("another process of ours ({pid})"), _ => "another application".to_string() };
        *self.seen.borrow_mut() = Some((payload, ours, from)); }
    fn tell(&self, phase: Phase, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, offered: DROPEFFECT) -> u32 { let Some((payload, mine, from)) = self.seen.borrow().clone() else { return 0 }; let (x, y) = self.client(pt); let files = payload.paths.len();
        let detail = format!("effects=0x{:x} keys={} from={from}", offered.0, keys_text(keys));
        let op = (self.sink)(Notice::Drag(DragEvent { win: self.win, phase, x, y, allowed: narrowed(keys, offered), detail, mine, payload })); LAST.with(|l| *l.borrow_mut() = Some((phase, files, x, y))); op }
}
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> { let data = data.ok()?; if effect.is_null() { return Err(E_POINTER.into()) }
        self.look(data); let op = self.tell(Phase::Enter, keys, pt, unsafe { *effect }); unsafe { *effect = to_effect(op); }
        // the shell's helper draws the picture the source gave (Explorer's, a browser's, ours) over our window
        if let Some(h) = &self.helper { unsafe { let _ = h.DragEnter(self.hwnd, data, &POINT { x: pt.x, y: pt.y }, to_effect(op)); } } Ok(()) }
    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> { if effect.is_null() { return Err(E_POINTER.into()) }
        let op = self.tell(Phase::Over, keys, pt, unsafe { *effect }); unsafe { *effect = to_effect(op); } if let Some(h) = &self.helper { unsafe { let _ = h.DragOver(&POINT { x: pt.x, y: pt.y }, to_effect(op)); } } Ok(()) }
    fn DragLeave(&self) -> windows::core::Result<()> { *self.seen.borrow_mut() = None;
        (self.sink)(Notice::Drag(DragEvent { win: self.win, phase: Phase::Leave, x: -1.0, y: -1.0, allowed: 0, detail: String::new(), mine: None, payload: Rc::new(Payload::default()) })); LAST.with(|l| *l.borrow_mut() = Some((Phase::Leave, 0, -1.0, -1.0)));
        if let Some(h) = &self.helper { unsafe { let _ = h.DragLeave(); } } Ok(()) }
    // the place is taken from THIS call's own point: the old application learned that moves are not promised before a drop
    fn Drop(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> { let data = data.ok()?; if effect.is_null() { return Err(E_POINTER.into()) }
        self.look(data); let op = self.tell(Phase::Drop, keys, pt, unsafe { *effect }); let seen = self.seen.borrow_mut().take();
        if let Some((payload, mine, _)) = seen { if op != 0 && !payload.promised.is_empty() { receive(data, &payload.promised, self.win, &self.dest, &self.sink); }
            // WHAT IS REPORTED BACK DECIDES WHAT THE SOURCE DOES. A source told "move" deletes its files (Explorer does). This
            // spike copies and moves NOTHING, so a stranger is told "copy" at most and never "move"; our own source only logs.
            // The real target does the move itself and answers as the shell's own targets do: effect NONE, with
            // "Performed DropEffect" = NONE and "Logical Performed DropEffect" = MOVE set on the data object (an "optimised move").
            let report = if mine.is_some() { op } else if op == MOVE { 0 } else { op };
            unsafe { *effect = to_effect(report); } let _ = set_bytes(data, cf(CFSTR_LOGICALPERFORMEDDROPEFFECT), &to_effect(op).0.to_le_bytes()); }
        else { unsafe { *effect = DROPEFFECT_NONE; } }
        if let Some(h) = &self.helper { unsafe { let _ = h.Drop(data, &POINT { x: pt.x, y: pt.y }, *effect); } } Ok(()) }
}
/// Makes the window the drop target. The window must have been made with winit's own handling off.
/// The window is about to go: its drop target is taken back from the system and let go, with the application's sink
/// and whatever that holds. (Without it a sink that holds the window keeps the window open after it was dropped.)
pub fn detach(window: &dyn winit31::window::Window) { unsafe { if let Some(hwnd) = hwnd_of(window) { let _ = RevokeDragDrop(hwnd); TARGETS.with(|t| t.borrow_mut().retain(|(h, _)| *h != hwnd.0 as usize)); } } }
pub fn attach(window: &dyn winit31::window::Window, win: usize, dest: &Path, sink: Sink) -> Result<String, String> { unsafe {
    let hwnd = hwnd_of(window).ok_or("no HWND")?; let ole = OleInitialize(None);
    let revoked = RevokeDragDrop(hwnd).is_ok();   // (nothing to revoke when winit was told to keep out: this says so in the log)
    let helper: Option<IDropTargetHelper> = CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).ok();
    let target: IDropTarget = DropTarget { hwnd, win, sink, dest: dest.to_path_buf(), helper: helper.clone(), seen: RefCell::new(None) }.into();
    RegisterDragDrop(hwnd, &target).map_err(|e| format!("RegisterDragDrop: {e}"))?; TARGETS.with(|t| t.borrow_mut().push((hwnd.0 as usize, target)));
    Ok(format!("IDropTarget registered (OleInitialize: {}; a target of winit's was {}; the shell's picture helper: {})", if ole.is_ok() { "ok" } else { "failed" }, if revoked { "FOUND and revoked" } else { "not there" }, if helper.is_some() { "yes" } else { "no" })) } }

// --- the source ---
#[implement(IDropSource)]
struct DropSource;
impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT { if escape.as_bool() { DRAGDROP_S_CANCEL } else if keys.0 & MK_LBUTTON_BIT == 0 { DRAGDROP_S_DROP } else { S_OK } }
    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT { DRAGDROP_S_USEDEFAULTCURSORS }
}
/// The shell's own data object for these paths, with our mark and the preferred effect set on it.
fn data_object(paths: &[PathBuf], ops: u32, serial: u64) -> Result<IDataObject, String> { unsafe {
    let mut ids: Vec<*mut ITEMIDLIST> = Vec::new(); let mut failed = None;
    for p in paths { let w = wide(p); let mut id: *mut ITEMIDLIST = std::ptr::null_mut(); match SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut id, 0, None) { Ok(()) if !id.is_null() => ids.push(id), Ok(()) => { failed = Some(format!("{}: no id list", p.display())); break } Err(e) => { failed = Some(format!("{}: {e}", p.display())); break } } }
    let consts: Vec<*const ITEMIDLIST> = ids.iter().map(|p| *p as *const ITEMIDLIST).collect();
    let array = if failed.is_none() { Some(SHCreateShellItemArrayFromIDLists(&consts)) } else { None }; for id in &ids { ILFree(Some(*id as *const ITEMIDLIST)); }
    if let Some(f) = failed { return Err(f) } let array = array.unwrap().map_err(|e| format!("SHCreateShellItemArrayFromIDLists: {e}"))?;
    let data: IDataObject = array.BindToHandler(None, &BHID_DataObject).map_err(|e| format!("BindToHandler(BHID_DataObject): {e}"))?;
    let preferred = if ops & COPY != 0 { DROPEFFECT_COPY } else if ops & MOVE != 0 { DROPEFFECT_MOVE } else { DROPEFFECT_LINK };
    let _ = set_bytes(&data, cf(CFSTR_PREFERREDDROPEFFECT), &preferred.0.to_le_bytes());
    let mut mark = std::process::id().to_le_bytes().to_vec(); mark.extend_from_slice(&serial.to_le_bytes()); if !set_bytes(&data, cf(MARK), &mark) { return Err("the data object did not take our own format".into()) }
    Ok(data) } }
/// Hands the drag picture to the shell's helper, which keeps it on the data object (and owns the bitmap once it has it).
fn set_picture(data: &IDataObject, img: &DragImage) -> Result<(), String> { unsafe {
    let helper: IDragSourceHelper = CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("the drag helper: {e}"))?;
    let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: img.w as i32, biHeight: -(img.h as i32), biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() }, ..Default::default() };
    let mut bits: *mut c_void = std::ptr::null_mut(); let bmp = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).map_err(|e| format!("CreateDIBSection: {e}"))?; if bits.is_null() { return Err("no bitmap memory".into()) }
    // top-down BGRA, colour multiplied by alpha
    let out = std::slice::from_raw_parts_mut(bits as *mut u8, img.rgba.len()); for (o, px) in out.chunks_exact_mut(4).zip(img.rgba.chunks_exact(4)) { let a = px[3] as u32; o[0] = (px[2] as u32 * a / 255) as u8; o[1] = (px[1] as u32 * a / 255) as u8; o[2] = (px[0] as u32 * a / 255) as u8; o[3] = px[3]; }
    let shdi = SHDRAGIMAGE { sizeDragImage: SIZE { cx: img.w as i32, cy: img.h as i32 }, ptOffset: POINT { x: img.hot.0 as i32, y: img.hot.1 as i32 }, hbmpDragImage: bmp, crColorKey: COLORREF(0xFFFF_FFFF) };
    helper.InitializeFromBitmap(&shdi, data).map_err(|e| { let _ = DeleteObject(bmp.into()); format!("InitializeFromBitmap: {e}") }) } }
/// Runs the system's drag for these paths. Returns when the drag is OVER (`DoDragDrop` is modal); `Notice::Ended` has
/// been sent by then. A failed picture does not stop the drag (the old application's 80 ms rule: a drag with no
/// picture beats no drag).
pub fn start_drag(_window: &dyn winit31::window::Window, paths: &[PathBuf], img: DragImage, ops: u32, sink: Sink) -> Result<u64, String> { unsafe {
    if paths.is_empty() { return Err("nothing to drag".into()) } let _ = OleInitialize(None);
    let serial = SERIAL.with(|s| { s.set(s.get() + 1); s.get() });
    let data = data_object(paths, ops, serial)?; let picture = set_picture(&data, &img);
    let source: IDropSource = DropSource.into(); let mut effect = DROPEFFECT_NONE;
    let hr = DoDragDrop(&data, &source, to_effect(ops), &mut effect);
    // what happened: DoDragDrop's own effect, and what the target wrote back on the data object. Explorer, moving within
    // a volume, answers effect NONE and writes the move as the LOGICAL performed effect; a target that answers MOVE with
    // nothing written has left the deleting to us.
    let (performed, logical) = (dword(&data, CFSTR_PERFORMEDDROPEFFECT), dword(&data, CFSTR_LOGICALPERFORMEDDROPEFFECT));
    let op = if hr == DRAGDROP_S_DROP { let e = from_effect(effect); if e != 0 { e } else { logical.map(|l| from_effect(DROPEFFECT(l))).unwrap_or(0) } } else { 0 };
    sink(Notice::Ended { serial, op, detail: format!("DoDragDrop 0x{:08x}{} effect=0x{:x} performed={performed:?} logical={logical:?}{}", hr.0, if hr == DRAGDROP_S_CANCEL { " (cancelled)" } else if hr == DRAGDROP_S_DROP { " (dropped)" } else { "" }, effect.0, picture.err().map(|e| format!(" | NO PICTURE: {e}")).unwrap_or_default()) });
    Ok(serial) } }

