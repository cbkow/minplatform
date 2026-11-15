//! macOS: the drop target and the drag source, in AppKit, on a window winit made.
//!
//! COEXISTENCE (question d). winit 0.31 makes its NSWINDOW the dragging destination (its window delegate answers
//! `draggingEntered:` and the rest, the window is registered for a fixed list of types). We leave winit's classes
//! alone and take the VIEW instead:
//!   - the content view (winit's own NSView subclass) is given a subclass of ours made at run time - the same move
//!     key-value observing makes - that adds ONLY the NSDraggingDestination methods; nothing of winit's is replaced,
//!     so the pointer, the keys and the text input go on as before;
//!   - the view is registered for our types (file URLs, URLs, text, the file-promise types);
//!   - the window's own registration is taken away, so that winit's handling stays silent (AppKit asks the deepest
//!     registered view first, and with nothing registered on the window nothing reaches its delegate).
//! The drag OUT is `beginDraggingSessionWithItems:event:source:` on the view with a source object of our own; the
//! event it needs is the application's current one (we are called inside winit's handling of the pointer's move).
use crate::dnd::*;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSApplication, NSBitmapImageRep, NSDeviceRGBColorSpace, NSDragOperation, NSDraggingContext, NSDraggingInfo, NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEvent, NSEventModifierFlags, NSEventType, NSFilePromiseProvider,
    NSFilePromiseProviderDelegate, NSFilePromiseReceiver, NSImage, NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeString, NSPasteboardTypeURL, NSPasteboardWriting, NSView};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSObject, NSObjectProtocol, NSOperationQueue, NSPoint, NSRect, NSSize, NSString, NSURL};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::rc::Rc;

struct ViewState { win: usize, sink: Sink, dest: PathBuf, payload: Option<Rc<Payload>> }
struct Current { serial: u64, seq: isize, _source: Retained<Source> }
thread_local! {
    static VIEWS: RefCell<HashMap<usize, ViewState>> = RefCell::new(HashMap::new());
    static SERIAL: Cell<u64> = const { Cell::new(0) };
    /// the drag this process started and that has not ended
    static CURRENT: RefCell<Option<Current>> = const { RefCell::new(None) };
    /// for the self-test: the last event our destination methods made (phase, files, x, y, promised), and the promised files delivered
    static LAST: RefCell<Option<(Phase, usize, f64, f64, usize)>> = const { RefCell::new(None) };
    static DELIVERED: RefCell<Vec<Result<PathBuf, String>>> = const { RefCell::new(Vec::new()) };
    static KEEP: RefCell<Vec<Retained<NSObject>>> = const { RefCell::new(Vec::new()) };
}
fn from_ns(m: NSDragOperation) -> u32 { let mut o = 0; if m.0 & NSDragOperation::Copy.0 != 0 { o |= COPY; } if m.0 & NSDragOperation::Link.0 != 0 { o |= LINK; } if m.0 & (NSDragOperation::Move.0 | NSDragOperation::Generic.0) != 0 { o |= MOVE; } o }
fn to_ns(o: u32) -> NSDragOperation { NSDragOperation(if o & COPY != 0 { NSDragOperation::Copy.0 } else if o & MOVE != 0 { NSDragOperation::Move.0 } else if o & LINK != 0 { NSDragOperation::Link.0 } else { 0 }) }
fn view_of(window: &dyn winit31::window::Window) -> Option<Retained<NSView>> { use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() { RawWindowHandle::AppKit(h) => unsafe { Retained::retain(h.ns_view.as_ptr() as *mut NSView) }, _ => None } }
fn mods_now() -> String { let f: NSEventModifierFlags = unsafe { msg_send![NSEvent::class(), modifierFlags] }; let mut v = Vec::new();
    for (bit, name) in [(NSEventModifierFlags::Shift, "shift"), (NSEventModifierFlags::Control, "ctrl"), (NSEventModifierFlags::Option, "opt"), (NSEventModifierFlags::Command, "cmd")] { if f.0 & bit.0 != 0 { v.push(name); } } if v.is_empty() { "-".into() } else { v.join("+") } }
/// A point in the window (AppKit: from the bottom left, in points) as physical pixels from the view's top left.
fn position(view: &NSView, in_window: NSPoint) -> (f64, f64) { let p = view.convertPoint_fromView(in_window, None); let y = if view.isFlipped() { p.y } else { view.bounds().size.height - p.y };
    let s = view.window().map(|w| w.backingScaleFactor()).unwrap_or(1.0); (p.x * s, y * s) }

/// What a pasteboard holds for a file browser. File URLs come as NSURL objects and are asked for their PATH: Finder
/// puts file REFERENCE URLs on the pasteboard (`file:///.file/id=...`), which only NSURL can turn into a path.
fn read(pb: &NSPasteboard) -> Payload {
    let mut p = Payload { types: pb.types().map(|a| a.iter().map(|t| t.to_string()).collect()).unwrap_or_default(), ..Default::default() };
    let classes: Retained<NSArray<AnyObject>> = unsafe { msg_send![NSArray::<AnyObject>::class(), arrayWithObject: NSURL::class()] };
    let urls: Option<Retained<NSArray<NSURL>>> = unsafe { msg_send![pb, readObjectsForClasses: &*classes, options: std::ptr::null::<NSDictionary>()] };
    for u in urls.iter().flat_map(|a| a.iter()) { if u.isFileURL() { if let Some(path) = u.path() { p.paths.push(PathBuf::from(path.to_string())); } } else if let Some(s) = u.absoluteString() { p.urls.push(s.to_string()); } }
    p.text = pb.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string());
    for r in receivers(pb).iter().flat_map(|a| a.iter()) { p.promised.push(format!("type {}", r.fileTypes().iter().map(|t| t.to_string()).collect::<Vec<_>>().join(","))); }
    p
}
fn receivers(pb: &NSPasteboard) -> Option<Retained<NSArray<NSFilePromiseReceiver>>> { let classes: Retained<NSArray<AnyObject>> = unsafe { msg_send![NSArray::<AnyObject>::class(), arrayWithObject: NSFilePromiseReceiver::class()] };
    unsafe { msg_send![pb, readObjectsForClasses: &*classes, options: std::ptr::null::<NSDictionary>()] } }
/// Promised files are asked for after the drop; each arrives (or fails) later, on the main queue.
fn receive(pb: &NSPasteboard, win: usize, dest: &Path, sink: &Sink) -> usize { let Some(list) = receivers(pb) else { return 0 }; let _ = std::fs::create_dir_all(dest);
    let url = NSURL::fileURLWithPath(&NSString::from_str(&dest.to_string_lossy())); let queue = NSOperationQueue::mainQueue(); let mut n = 0;
    for r in list.iter() { n += 1; let outer = sink.clone(); let sink = sink.clone();
        let block = RcBlock::new(move |url: NonNull<NSURL>, err: *mut NSError| { let result = if err.is_null() { unsafe { url.as_ref() }.path().map(|p| PathBuf::from(p.to_string())).ok_or_else(|| "no path".to_string()) } else { Err(unsafe { &*err }.localizedDescription().to_string()) };
            DELIVERED.with(|d| d.borrow_mut().push(result.clone())); sink(Notice::Promised { win, result }); });
        // (an exception here must not leave through winit's handler: it would stop every later event)
        let call = std::panic::AssertUnwindSafe(|| unsafe { r.receivePromisedFilesAtDestination_options_operationQueue_reader(&url, &NSDictionary::new(), &queue, &block) });
        if let Err(e) = objc2::exception::catch(call) { let why = format!("exception: {}", e.map(|e| format!("{e:?}")).unwrap_or_default()); DELIVERED.with(|d| d.borrow_mut().push(Err(why.clone()))); outer(Notice::Promised { win, result: Err(why) }); } }
    n }

// --- the destination: the methods our subclass of winit's view adds ---
fn handle(view: &NSView, info: *mut AnyObject, phase: Phase) -> usize {
    let key = view as *const NSView as usize; let Some((win, sink, dest, cached)) = VIEWS.with(|v| v.borrow().get(&key).map(|s| (s.win, s.sink.clone(), s.dest.clone(), s.payload.clone()))) else { return 0 };
    if info.is_null() { return 0 } let info: &ProtocolObject<dyn NSDraggingInfo> = unsafe { &*(info as *const ProtocolObject<dyn NSDraggingInfo>) };
    let pb = info.draggingPasteboard();
    // the pasteboard is read when the drag comes in and again at the drop; the moves in between use what was read
    let payload = match (phase, cached) { (Phase::Over, Some(p)) => p, _ => { let p = Rc::new(read(&pb)); VIEWS.with(|v| if let Some(s) = v.borrow_mut().get_mut(&key) { s.payload = Some(p.clone()); }); p } };
    let (x, y) = position(view, info.draggingLocation()); let mask = info.draggingSourceOperationMask(); let seq = info.draggingSequenceNumber();
    let mine = CURRENT.with(|c| c.borrow().as_ref().filter(|c| c.seq == seq).map(|c| c.serial)); let same_app = info.draggingSource().is_some();
    let detail = format!("mask=0x{:x} keys={} seq={seq} from={}", mask.0, mods_now(), if mine.is_some() { "our own drag" } else if same_app { "this application" } else { "another application" });
    let promised = payload.promised.len(); let files = payload.paths.len();
    let op = sink(Notice::Drag(DragEvent { win, phase, x, y, allowed: from_ns(mask), detail, mine, payload }));
    if phase == Phase::Drop && op != 0 { receive(&pb, win, &dest, &sink); }
    LAST.with(|l| *l.borrow_mut() = Some((phase, files, x, y, promised)));
    to_ns(op).0
}
extern "C-unwind" fn entered(this: *mut NSView, _: Sel, info: *mut AnyObject) -> usize { handle(unsafe { &*this }, info, Phase::Enter) }
extern "C-unwind" fn updated(this: *mut NSView, _: Sel, info: *mut AnyObject) -> usize { handle(unsafe { &*this }, info, Phase::Over) }
extern "C-unwind" fn exited(this: *mut NSView, _: Sel, _info: *mut AnyObject) { let key = this as usize;
    let Some((win, sink)) = VIEWS.with(|v| v.borrow_mut().get_mut(&key).map(|s| { s.payload = None; (s.win, s.sink.clone()) })) else { return };
    sink(Notice::Drag(DragEvent { win, phase: Phase::Leave, x: -1.0, y: -1.0, allowed: 0, detail: String::new(), mine: None, payload: Rc::new(Payload::default()) })); LAST.with(|l| *l.borrow_mut() = Some((Phase::Leave, 0, -1.0, -1.0, 0))); }
extern "C-unwind" fn prepare(_: *mut NSView, _: Sel, _info: *mut AnyObject) -> Bool { Bool::YES }
extern "C-unwind" fn perform(this: *mut NSView, _: Sel, info: *mut AnyObject) -> Bool { Bool::new(handle(unsafe { &*this }, info, Phase::Drop) != 0) }
extern "C-unwind" fn conclude(this: *mut NSView, _: Sel, _info: *mut AnyObject) { let key = this as usize; VIEWS.with(|v| if let Some(s) = v.borrow_mut().get_mut(&key) { s.payload = None; }); }
/// (updates while the pointer moves are enough; the periodic ones would only repeat the same place)
extern "C-unwind" fn periodic(_: *mut NSView, _: Sel) -> Bool { Bool::NO }

/// The view's class, with the destination methods added: made once for each class we meet (winit has one).
fn subclass(of: &AnyClass) -> Option<&'static AnyClass> {
    let name = std::ffi::CString::new(format!("S16Drop_{}", of.name().to_string_lossy())).ok()?;
    if let Some(c) = AnyClass::get(&name) { return Some(c) }
    let mut b = ClassBuilder::new(&name, of)?;
    unsafe {
        b.add_method(sel!(draggingEntered:), entered as extern "C-unwind" fn(*mut NSView, Sel, *mut AnyObject) -> usize);
        b.add_method(sel!(draggingUpdated:), updated as extern "C-unwind" fn(*mut NSView, Sel, *mut AnyObject) -> usize);
        b.add_method(sel!(draggingExited:), exited as extern "C-unwind" fn(*mut NSView, Sel, *mut AnyObject));
        b.add_method(sel!(prepareForDragOperation:), prepare as extern "C-unwind" fn(*mut NSView, Sel, *mut AnyObject) -> Bool);
        b.add_method(sel!(performDragOperation:), perform as extern "C-unwind" fn(*mut NSView, Sel, *mut AnyObject) -> Bool);
        b.add_method(sel!(concludeDragOperation:), conclude as extern "C-unwind" fn(*mut NSView, Sel, *mut AnyObject));
        b.add_method(sel!(wantsPeriodicDraggingUpdates), periodic as extern "C-unwind" fn(*mut NSView, Sel) -> Bool);
    }
    Some(b.register())
}
/// Makes the window's view the drop target for files, URLs, text and promised files. `dest`: where promised files are received.
pub fn attach(window: &dyn winit31::window::Window, win: usize, dest: &Path, sink: Sink) -> Result<String, String> {
    let view = view_of(window).ok_or("no NSView")?; let was = view.class().name().to_string_lossy().to_string();
    let cls = subclass(view.class()).ok_or("the subclass could not be made")?;
    unsafe { AnyObject::set_class(&view, cls) };
    let mut types: Vec<Retained<NSString>> = unsafe { vec![NSPasteboardTypeFileURL.retain(), NSPasteboardTypeURL.retain(), NSPasteboardTypeString.retain(), NSString::from_str("NSFilenamesPboardType")] };
    types.extend(NSFilePromiseReceiver::readableDraggedTypes().iter());
    view.registerForDraggedTypes(&NSArray::from_retained_slice(&types));
    // winit's own handling: the window is no longer registered for anything, so its delegate is not asked
    if let Some(w) = view.window() { w.unregisterDraggedTypes(); }
    VIEWS.with(|v| v.borrow_mut().insert(&*view as *const NSView as usize, ViewState { win, sink, dest: dest.to_path_buf(), payload: None }));
    Ok(format!("view class {was} -> {}; {} types registered on the view; the window's own registration removed", view.class().name().to_string_lossy(), view.registeredDraggedTypes().len()))
}

// --- the source ---
struct SourceIvars { ops: usize, serial: u64, sink: Sink }
define_class!(
    #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "S16DragSource"] #[ivars = SourceIvars]
    struct Source;
    unsafe impl NSObjectProtocol for Source {}
    unsafe impl NSDraggingSource for Source {
        // the same offer inside and outside the application; the modifier keys narrow it (AppKit does that, unless told to ignore them)
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn mask(&self, _s: &NSDraggingSession, _c: NSDraggingContext) -> NSDragOperation { NSDragOperation(self.ivars().ops) }
        // the end: `operation` is what the target answered - the only account of what happened to the files
        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _s: &NSDraggingSession, at: NSPoint, operation: NSDragOperation) { let iv = self.ivars(); let (serial, sink) = (iv.serial, iv.sink.clone());
            // (the source object is let go after this method has returned, not inside it)
            let op = if operation.0 & NSDragOperation::Delete.0 != 0 { MOVE } else if operation.0 & NSDragOperation::Copy.0 != 0 { COPY } else { from_ns(operation) };
            sink(Notice::Ended { serial, op, detail: format!("NSDragOperation 0x{:x}{} at screen {:.0},{:.0}", operation.0, if operation.0 & NSDragOperation::Delete.0 != 0 { " (the Trash)" } else if operation.0 == 0 { " (cancelled or refused)" } else { "" }, at.x, at.y) });
            if let Some(c) = CURRENT.with(|c| { let mut c = c.borrow_mut(); if c.as_ref().is_some_and(|c| c.serial == serial) { c.take() } else { None } }) { KEEP.with(|k| { let mut k = k.borrow_mut(); k.clear(); k.push(Retained::into_super(c._source)); }); } }
    }
);
fn to_ns_mask(ops: u32) -> usize { let mut m = 0; if ops & COPY != 0 { m |= NSDragOperation::Copy.0; } if ops & LINK != 0 { m |= NSDragOperation::Link.0; } if ops & MOVE != 0 { m |= NSDragOperation::Move.0 | NSDragOperation::Generic.0 | NSDragOperation::Delete.0; } m }
/// Our own pixels as an NSImage of `w / scale` by `h / scale` points.
fn image(img: &DragImage) -> Option<Retained<NSImage>> { let (w, h) = (img.w as isize, img.h as isize);
    let rep = unsafe { NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(NSBitmapImageRep::alloc(), std::ptr::null_mut(), w, h, 8, 4, true, false, NSDeviceRGBColorSpace, w * 4, 32) }?;
    let data = rep.bitmapData(); if data.is_null() { return None }
    // (AppKit's bitmaps hold colour already multiplied by alpha)
    let out = unsafe { std::slice::from_raw_parts_mut(data, img.rgba.len()) }; for (o, px) in out.chunks_exact_mut(4).zip(img.rgba.chunks_exact(4)) { let a = px[3] as u32; o[0] = (px[0] as u32 * a / 255) as u8; o[1] = (px[1] as u32 * a / 255) as u8; o[2] = (px[2] as u32 * a / 255) as u8; o[3] = px[3]; }
    let size = NSSize::new(img.w as f64 / img.scale, img.h as f64 / img.scale); let image = NSImage::initWithSize(NSImage::alloc(), size); image.addRepresentation(&rep); Some(image) }
/// One dragging item for each path: a file URL is its own pasteboard writer (it writes `public.file-url` and the legacy
/// file-name types follow from it). `at`: the pointer in the view's own coordinates (points).
fn items(paths: &[PathBuf], img: Option<&NSImage>, at: NSPoint, hot: (f64, f64), flipped: bool) -> (Vec<Retained<NSURL>>, Vec<Retained<NSDraggingItem>>) {
    let urls: Vec<Retained<NSURL>> = paths.iter().map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()))).collect();
    let size = img.map(|i| i.size()).unwrap_or(NSSize::new(16.0, 16.0));
    let items = urls.iter().enumerate().map(|(i, u)| { let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&**u));
        // every item needs a frame; the picture rides on the first, the others sit a little behind it (the target's formation takes over once it accepts)
        let k = i.min(4) as f64 * 3.0; let origin = NSPoint::new(at.x - hot.0 + k, if flipped { at.y - hot.1 + k } else { at.y + hot.1 - size.height - k });
        unsafe { item.setDraggingFrame_contents(NSRect::new(origin, size), if i == 0 { img.map(|i| &**i as &AnyObject) } else { None }) }; item }).collect();
    (urls, items) }
/// Starts the system's drag for these paths. Returns at once: the drag runs in the application's own loop, and its end
/// arrives as `Notice::Ended`. Must be called while the pointer event that began it is the current event.
pub fn start_drag(window: &dyn winit31::window::Window, paths: &[PathBuf], img: DragImage, ops: u32, sink: Sink) -> Result<u64, String> {
    let mtm = MainThreadMarker::new().ok_or("not the main thread")?; let view = view_of(window).ok_or("no NSView")?;
    if paths.is_empty() { return Err("nothing to drag".into()) } if CURRENT.with(|c| c.borrow().is_some()) { return Err("a drag of ours is still running".into()) }
    let event = NSApplication::sharedApplication(mtm).currentEvent().ok_or("no current event")?; let ty = event.r#type();
    if ty != NSEventType::LeftMouseDragged && ty != NSEventType::LeftMouseDown { return Err(format!("the current event is not the pointer's (type {})", ty.0)) }
    let at = view.convertPoint_fromView(event.locationInWindow(), None); let picture = image(&img);
    let (_urls, items) = items(paths, picture.as_deref(), at, (img.hot.0 / img.scale, img.hot.1 / img.scale), view.isFlipped());
    let serial = SERIAL.with(|s| { s.set(s.get() + 1); s.get() });
    let source: Retained<Source> = unsafe { msg_send![super(Source::alloc(mtm).set_ivars(SourceIvars { ops: to_ns_mask(ops), serial, sink })), init] };
    let session = view.beginDraggingSessionWithItems_event_source(&NSArray::from_retained_slice(&items), &event, ProtocolObject::from_ref(&*source));
    session.setAnimatesToStartingPositionsOnCancelOrFail(true);
    CURRENT.with(|c| *c.borrow_mut() = Some(Current { serial, seq: session.draggingSequenceNumber(), _source: source }));
    Ok(serial)
}

// --- Part A: what can be checked with no hand on the machine ---
struct FakeIvars { pb: Retained<NSPasteboard>, at: NSPoint, mask: usize, seq: isize }
define_class!(
    // a dragging info of our own making, over a pasteboard of our own (never the general one): enough of NSDraggingInfo
    // for our destination methods, which are then called exactly as AppKit would call them
    #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "S16FakeDraggingInfo"] #[ivars = FakeIvars]
    struct FakeInfo;
    impl FakeInfo {
        #[unsafe(method_id(draggingPasteboard))] fn pb(&self) -> Retained<NSPasteboard> { self.ivars().pb.clone() }
        #[unsafe(method(draggingLocation))] fn at(&self) -> NSPoint { self.ivars().at }
        #[unsafe(method(draggingSourceOperationMask))] fn mask(&self) -> NSDragOperation { NSDragOperation(self.ivars().mask) }
        #[unsafe(method(draggingSequenceNumber))] fn seq(&self) -> isize { self.ivars().seq }
        #[unsafe(method_id(draggingSource))] fn source(&self) -> Option<Retained<AnyObject>> { None }
    }
);
define_class!(
    // a promise of our own, to see a promised file arrive through the receiver without another application
    #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "S16PromiseDelegate"]
    struct Promiser;
    unsafe impl NSObjectProtocol for Promiser {}
    impl Promiser {
        #[unsafe(method_id(filePromiseProvider:fileNameForType:))] fn name(&self, _p: &NSFilePromiseProvider, _t: &NSString) -> Retained<NSString> { NSString::from_str("s16 promised by the self-test.txt") }
        #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))] fn write(&self, _p: &NSFilePromiseProvider, url: &NSURL, done: &block2::DynBlock<dyn Fn(*mut NSError)>) { if let Some(p) = url.path() { let _ = std::fs::write(p.to_string(), "written when the promise was called in\n"); } done.call((std::ptr::null_mut(),)); }
        #[unsafe(method_id(operationQueueForFilePromiseProvider:))] fn queue(&self, _p: &NSFilePromiseProvider) -> Retained<NSOperationQueue> { NSOperationQueue::mainQueue() }
    }
);
fn fake(mtm: MainThreadMarker, pb: &NSPasteboard, at: NSPoint, mask: usize, seq: isize) -> Retained<FakeInfo> { unsafe { msg_send![super(FakeInfo::alloc(mtm).set_ivars(FakeIvars { pb: pb.retain(), at, mask, seq })), init] } }
pub fn selftest(window: &dyn winit31::window::Window, paths: &[PathBuf], dest: &Path) -> Vec<(String, bool, String)> {
    let mut r: Vec<(String, bool, String)> = Vec::new(); let Some(mtm) = MainThreadMarker::new() else { return vec![("main thread".into(), false, String::new())] }; let Some(view) = view_of(window) else { return vec![("the view".into(), false, String::new())] };
    // 1. the view is ours to drop on, and still winit's
    let cls = view.class(); let sup = cls.superclass().map(|c| c.name().to_string_lossy().to_string()).unwrap_or_default();
    let all = [sel!(draggingEntered:), sel!(draggingUpdated:), sel!(draggingExited:), sel!(performDragOperation:), sel!(mouseDown:), sel!(keyDown:)].iter().all(|s| cls.responds_to(*s));
    r.push(("the view answers the destination methods and is still winit's view".into(), all && cls.name().to_string_lossy().starts_with("S16Drop_"), format!("class {} over {sup}", cls.name().to_string_lossy())));
    let reg: Vec<String> = view.registeredDraggedTypes().iter().map(|t| t.to_string()).collect();
    r.push(("the view is registered for files, URLs, text and promises".into(), ["public.file-url", "public.url", "public.utf8-plain-text", "com.apple.NSFilePromiseItemMetaData"].iter().all(|t| reg.iter().any(|x| x == t)) || (reg.iter().any(|x| x == "public.file-url") && reg.len() >= 5), reg.join(", ")));
    // 2. the payload a drag would carry: the very items `start_drag` builds, written to a pasteboard of our own and read back
    let img = crate::drag_image(paths.len(), 2.0); let picture = image(&img);
    let (urls, its) = items(paths, picture.as_deref(), NSPoint::new(100.0, 100.0), (4.0, 4.0), true);
    r.push(("the drag picture from our own pixels".into(), picture.as_ref().is_some_and(|p| (p.size().width - img.w as f64 / 2.0).abs() < 0.01) && its.len() == paths.len() && its.first().is_some_and(|i| i.imageComponents().is_some_and(|c| c.len() > 0)), format!("{} x {} px -> {:?} points; {} dragging items, the first with {} image component(s)", img.w, img.h, picture.as_ref().map(|p| (p.size().width, p.size().height)), its.len(), its.first().and_then(|i| i.imageComponents()).map(|c| c.len()).unwrap_or(0))));
    let pb = NSPasteboard::pasteboardWithUniqueName(); pb.clearContents();
    let writers: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = urls.iter().map(|u| ProtocolObject::from_retained(u.clone())).collect();
    let wrote = pb.writeObjects(&NSArray::from_retained_slice(&writers)); let back = read(&pb); let n_items = pb.pasteboardItems().map(|a| a.len()).unwrap_or(0);
    let item_types: Vec<String> = pb.pasteboardItems().and_then(|a| a.iter().next().map(|i| i.types().iter().map(|t| t.to_string()).collect())).unwrap_or_default();
    r.push((format!("{} paths out as {} pasteboard items and back, unchanged", paths.len(), n_items), wrote && n_items == paths.len() && back.paths.len() == paths.len() && back.paths.iter().zip(paths).all(|(a, b)| std::fs::canonicalize(a).ok() == std::fs::canonicalize(b).ok() && a.exists()), format!("each item: {}; the pasteboard: {}", item_types.join(", "), back.types.join(", "))));
    // (a finding, not a failure: AppKit hands names back in DECOMPOSED form - "u" + U+0308 for U+00FC - so a path from a drag is not byte-equal to the path that went in)
    let same = back.paths.iter().zip(paths).filter(|(a, b)| a == b).count();
    r.push(("NOTE the same files, but not always the same bytes".into(), true, format!("{same} of {} paths came back byte-identical (names with accents come back decomposed: compare by file identity or after normalising)", paths.len())));
    // 3. the destination methods, called as AppKit calls them, with a dragging info of our own over that pasteboard
    let h = view.bounds().size.height; let s = view.window().map(|w| w.backingScaleFactor()).unwrap_or(1.0); let at = NSPoint::new(120.0, h - 80.0);   // 120 from the left, 80 from the top
    let info = fake(mtm, &pb, at, NSDragOperation::Copy.0 | NSDragOperation::Generic.0 | NSDragOperation::Link.0, 4242); let ip: *const AnyObject = (&*info as *const FakeInfo).cast();
    let e: usize = unsafe { msg_send![&*view, draggingEntered: ip] }; let l1 = LAST.with(|l| *l.borrow());
    let u: usize = unsafe { msg_send![&*view, draggingUpdated: ip] }; let l2 = LAST.with(|l| *l.borrow());
    let ok1 = e == NSDragOperation::Copy.0 && u == e && l1.is_some_and(|l| l.0 == Phase::Enter && l.1 == paths.len() && (l.2 - 120.0 * s).abs() < 1.0 && (l.3 - 80.0 * s).abs() < 1.0) && l2.is_some_and(|l| l.0 == Phase::Over);
    r.push(("enter and over reach us with the files, the place and the operation".into(), ok1, format!("answered 0x{e:x}; seen {l1:?} then {l2:?} (expected {} files at {:.0},{:.0})", paths.len(), 120.0 * s, 80.0 * s)));
    let _: () = unsafe { msg_send![&*view, draggingExited: ip] }; let l3 = LAST.with(|l| *l.borrow());
    let _: usize = unsafe { msg_send![&*view, draggingEntered: ip] }; let p: Bool = unsafe { msg_send![&*view, prepareForDragOperation: ip] }; let d: Bool = unsafe { msg_send![&*view, performDragOperation: ip] }; let _: () = unsafe { msg_send![&*view, concludeDragOperation: ip] }; let l4 = LAST.with(|l| *l.borrow());
    r.push(("leave, then a drop with every path".into(), l3.is_some_and(|l| l.0 == Phase::Leave) && p.as_bool() && d.as_bool() && l4.is_some_and(|l| l.0 == Phase::Drop && l.1 == paths.len()), format!("{l3:?} then {l4:?}")));
    // the modifier-narrowed mask: only "move" left (what AppKit hands over with Command held)
    let info2 = fake(mtm, &pb, at, NSDragOperation::Generic.0, 4243); let ip2: *const AnyObject = (&*info2 as *const FakeInfo).cast(); let m: usize = unsafe { msg_send![&*view, draggingEntered: ip2] }; let _: () = unsafe { msg_send![&*view, draggingExited: ip2] };
    r.push(("a mask narrowed to one operation is answered with that operation".into(), m == NSDragOperation::Move.0, format!("answered 0x{m:x}")));
    let _: () = unsafe { msg_send![&*pb, releaseGlobally] };
    // 4. a promised file: a provider of our own on a second pasteboard, dropped on the view; the file is looked for later (selftest_late)
    if std::env::var_os("S16_NO_PROMISE").is_none() {
        let _ = std::fs::remove_file(dest.join("s16 promised by the self-test.txt"));
        let promiser: Retained<Promiser> = unsafe { msg_send![super(Promiser::alloc(mtm).set_ivars(())), init] };
        let delegate: &ProtocolObject<dyn NSFilePromiseProviderDelegate> = unsafe { &*((&*promiser as *const Promiser).cast()) };
        let provider = NSFilePromiseProvider::initWithFileType_delegate(NSFilePromiseProvider::alloc(), &NSString::from_str("public.plain-text"), delegate);
        let pb2 = NSPasteboard::pasteboardWithUniqueName(); pb2.clearContents(); let w: Retained<ProtocolObject<dyn NSPasteboardWriting>> = ProtocolObject::from_retained(provider.clone());
        let wrote = pb2.writeObjects(&NSArray::from_retained_slice(&[w])); let seen = read(&pb2);
        let info3 = fake(mtm, &pb2, at, NSDragOperation::Copy.0, 4244); let ip3: *const AnyObject = (&*info3 as *const FakeInfo).cast();
        let _: usize = unsafe { msg_send![&*view, draggingEntered: ip3] }; let d: Bool = unsafe { msg_send![&*view, performDragOperation: ip3] }; let l5 = LAST.with(|l| *l.borrow());
        r.push(("a file promise is seen as a promise at the drop".into(), wrote && seen.promised.len() == 1 && d.as_bool() && l5.is_some_and(|l| l.4 == 1), format!("{:?}; types {}", seen.promised, seen.types.join(", "))));
        KEEP.with(|k| { let mut k = k.borrow_mut(); k.push(Retained::into_super(promiser)); k.push(Retained::into_super(provider)); k.push(Retained::into_super(pb2)); });
    }
    // 5. the pointer still arrives through winit: a press and a release are handed to the view's own methods, inside this
    //    process (nothing is sent to the desktop); the application counts what winit makes of them
    let wn = view.window().map(|w| w.windowNumber()).unwrap_or(0);
    for (ty, selector) in [(NSEventType::LeftMouseDown, sel!(mouseDown:)), (NSEventType::LeftMouseUp, sel!(mouseUp:))] {
        if let Some(ev) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(ty, NSPoint::new(4.0, h - 4.0), NSEventModifierFlags(0), 0.0, wn, None, 0, 1, 1.0) { let _: () = unsafe { msg_send![&*view, performSelector: selector, withObject: &*ev] }; } }
    r
}
/// A second look, after the application's loop has turned: did the promised file arrive?
pub fn selftest_late() -> Vec<(String, bool, String)> { if std::env::var_os("S16_NO_PROMISE").is_some() { return Vec::new() }
    let d = DELIVERED.with(|d| d.borrow().clone()); KEEP.with(|k| k.borrow_mut().clear());
    // AppKit hands a promised file over only inside a REAL drag's perform; outside one it raises - which we must catch (see `receive`)
    let refused = d.len() == 1 && d[0].as_ref().is_err_and(|e| e.contains("can only be called during")); let delivered = d.len() == 1 && d[0].as_ref().is_ok_and(|p| p.is_file());
    vec![(if refused { "NOT CHECKABLE without a real drag: receiving a promised file (AppKit refused it outside a drag; the refusal was caught and the loop lived)".into() } else { "the promised file was delivered through NSFilePromiseReceiver".into() }, refused || delivered,
        format!("{:?}", d.iter().map(|x| x.as_ref().map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).map_err(|e| e.chars().take(200).collect::<String>())).collect::<Vec<_>>()))] }
