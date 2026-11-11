//! S9 step 3f — the editor's text as ONE hand-written accessibility element inside the real stack (`S9_NATIVE=1`).
//! winit owns the NSView; AccessKit's subclassing adapter has already put its dynamic subclass on it and answers
//! `accessibilityChildren` / `accessibilityFocusedUIElement` / `accessibilityHitTest:`. We put ONE more dynamic subclass on
//! top of AccessKit's: its three methods add our element to AccessKit's children, and answer focus and hit-test with it.
//! The element is an `NSAccessibilityElement` subclass answering the text protocol straight from the document (a flat
//! UTF-16 view over the blocks; geometry from parley) — what the reference window (`tools/s9/`) does in Swift.
//! Spike-grade: the element reaches the `App` through a main-thread pointer.
use crate::App;
use objc2::{msg_send, rc::Retained, runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel}, sel};
use objc2_foundation::{ns_string, NSPoint, NSRange, NSRect, NSSize, NSString};
use std::{cell::{Cell, RefCell}, collections::BTreeMap, io::Write, time::Instant};

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" { fn NSAccessibilityPostNotification(element: *const AnyObject, notification: *const NSString); }

thread_local! {
    static BAR: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
    static AREA: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
    static APP: Cell<*mut App> = const { Cell::new(std::ptr::null_mut()) };
    static VIEW: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
    static ELEMENT: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
    static BELOW: Cell<*const AnyClass> = const { Cell::new(std::ptr::null()) };   // the class under ours (AccessKit's subclass)
    static ASKED: RefCell<BTreeMap<String, u64>> = const { RefCell::new(BTreeMap::new()) };
    static LOG: RefCell<Option<Log>> = const { RefCell::new(None) };
}
struct Log { t0: Instant, out: std::io::BufWriter<std::fs::File>, counts: BTreeMap<&'static str, u64>, last: BTreeMap<&'static str, String> }
/// One line per call; a call repeated with the same detail is counted, not re-printed (as the reference window's log).
pub fn log(name: &'static str, detail: String, always: bool) {
    LOG.with_borrow_mut(|l| { let Some(l) = l.as_mut() else { return };
        *l.counts.entry(name).or_default() += 1;
        if !always && l.last.get(name) == Some(&detail) { return }
        let _ = writeln!(l.out, "{:9.3}  {:<28} {}", l.t0.elapsed().as_secs_f64(), name, detail);
        if always { let _ = l.out.flush(); }
        l.last.insert(name, detail); });
}
pub fn summary() { LOG.with_borrow_mut(|l| { if let Some(l) = l.as_mut() { let _ = writeln!(l.out, "\n── call counts ──"); let mut c: Vec<_> = l.counts.iter().collect(); c.sort_by(|a, b| b.1.cmp(a.1)); for (k, v) in c { let _ = writeln!(l.out, "{v:8}  {k}"); } let _ = l.out.flush(); } }); }
pub fn active() -> bool { !ELEMENT.get().is_null() }

fn app() -> &'static mut App { unsafe { &mut *APP.get() } }
fn nsstr(s: &str) -> *mut NSString { Retained::autorelease_ptr(NSString::from_str(s)) }
/// View points (top-left origin, as winit's flipped view) → Cocoa screen coordinates.
fn to_screen(r: (f64, f64, f64, f64)) -> NSRect {
    let view = VIEW.get(); let rect = NSRect::new(NSPoint::new(r.0, r.1), NSSize::new(r.2, r.3));
    unsafe { let window: *mut AnyObject = msg_send![view, window]; if window.is_null() { return rect }
        let in_window: NSRect = msg_send![view, convertRect: rect, toView: std::ptr::null::<AnyObject>()];
        msg_send![window, convertRectToScreen: in_window] }
}
fn fmt(r: NSRect) -> String { format!("({},{} {}×{})", r.origin.x as i64, r.origin.y as i64, r.size.width as i64, r.size.height as i64) }

// ── the element
unsafe extern "C-unwind" fn yes(_: &AnyObject, _: Sel) -> Bool { Bool::YES }
unsafe extern "C-unwind" fn role(_: &AnyObject, _: Sel) -> *const NSString { log("ax.role", String::new(), false); ns_string!("AXTextArea") }
unsafe extern "C-unwind" fn label(_: &AnyObject, _: Sel) -> *const NSString { ns_string!("Editor") }
// The view itself is not an accessibility element (it is ignored): our parent is the view's parent, as for AccessKit's root nodes.
unsafe extern "C-unwind" fn parent(this: &AnyObject, _: Sel) -> *mut AnyObject { log("ax.parent", String::new(), false); if !AREA.get().is_null() && !std::ptr::eq(this, AREA.get()) { return AREA.get() } if std::env::var_os("S9_PARENT_VIEW").is_some() { return VIEW.get() } unsafe { msg_send![VIEW.get(), accessibilityParent] } }
unsafe extern "C-unwind" fn window(_: &AnyObject, _: Sel) -> *mut AnyObject { log("ax.window", String::new(), false); unsafe { msg_send![VIEW.get(), window] } }
/// Which selectors the outside asks the element for (the ones it lacks are the interesting ones).
unsafe extern "C-unwind" fn responds(this: &AnyObject, _: Sel, s: Sel) -> Bool {
    let r: Bool = unsafe { msg_send![super(this, objc2::class!(NSAccessibilityElement)), respondsToSelector: s] };
    let name = s.name().to_string_lossy().into_owned();
    ASKED.with_borrow_mut(|m| { let first = !m.contains_key(&name); *m.entry(name.clone()).or_default() += 1; if first { log("responds?", format!("{name} → {}", r.as_bool()), true); } });
    r
}
unsafe extern "C-unwind" fn frame(_: &AnyObject, _: Sel) -> NSRect { let f = to_screen(app().nx_doc_frame()); log("ax.frame", fmt(f), false); f }
unsafe extern "C-unwind" fn value(_: &AnyObject, _: Sel) -> *mut NSString { let a = app(); a.sync_doc(); log("ax.value", format!("{} units", a.nx_len16()), false); nsstr(&a.doc_value) }
unsafe extern "C-unwind" fn n_chars(_: &AnyObject, _: Sel) -> isize { let a = app(); a.sync_doc(); a.nx_len16() as isize }
unsafe extern "C-unwind" fn sel_range(_: &AnyObject, _: Sel) -> NSRange { let (l, n) = app().nx_sel16(); log("ax.selectedTextRange", format!("{{{l},{n}}}"), false); NSRange::new(l, n) }
unsafe extern "C-unwind" fn sel_text(_: &AnyObject, _: Sel) -> *mut NSString { let a = app(); let (l, n) = a.nx_sel16(); let s = a.nx_sub16(l, n); log("ax.selectedText", format!("{s:?}"), false); nsstr(&s) }
unsafe extern "C-unwind" fn set_sel_range(_: &AnyObject, _: Sel, r: NSRange) { log("ax.setSelectedTextRange", format!("{{{},{}}}", r.location, r.length), true); app().nx_set_sel16(r.location, r.length); }
unsafe extern "C-unwind" fn string_for(_: &AnyObject, _: Sel, r: NSRange) -> *mut NSString { let s = app().nx_sub16(r.location, r.length); log("ax.stringForRange", format!("{{{},{}}}", r.location, r.length), false); nsstr(&s) }
unsafe extern "C-unwind" fn frame_for(_: &AnyObject, _: Sel, r: NSRange) -> NSRect { let f = to_screen(app().nx_rect16(r.location, r.length)); log("ax.frameForRange", format!("{{{},{}}} → {}", r.location, r.length, fmt(f)), false); f }
unsafe extern "C-unwind" fn visible_range(_: &AnyObject, _: Sel) -> NSRange { let a = app(); a.sync_doc(); log("ax.visibleCharacterRange", String::new(), false); NSRange::new(0, a.nx_len16()) }
// Declared because the working reference element declares them (a tool may qualify an element by its attribute names).
// Spike answers: the document as one "line" — real line answers come with the editor.
unsafe extern "C-unwind" fn zero(_: &AnyObject, _: Sel) -> isize { log("ax.insertionPointLineNumber", String::new(), false); 0 }
unsafe extern "C-unwind" fn line_for(_: &AnyObject, _: Sel, _i: isize) -> isize { log("ax.lineForIndex", String::new(), false); 0 }
unsafe extern "C-unwind" fn range_for_line(_: &AnyObject, _: Sel, _l: isize) -> NSRange { let a = app(); a.sync_doc(); log("ax.rangeForLine", String::new(), false); NSRange::new(0, a.nx_len16()) }
unsafe extern "C-unwind" fn range_for_index(_: &AnyObject, _: Sel, i: isize) -> NSRange { let a = app(); a.sync_doc(); log("ax.rangeForIndex", String::new(), false); let n = a.nx_len16(); let i = (i.max(0) as usize).min(n); NSRange::new(i, (i < n) as usize) }
unsafe extern "C-unwind" fn attributed_for(_: &AnyObject, _: Sel, r: NSRange) -> *mut AnyObject { let s = app().nx_sub16(r.location, r.length); log("ax.attributedStringForRange", String::new(), false); unsafe { let a: *mut AnyObject = msg_send![objc2::class!(NSAttributedString), alloc]; let a: *mut AnyObject = msg_send![a, initWithString: nsstr(&s)]; msg_send![a, autorelease] } }
unsafe extern "C-unwind" fn set_sel_text(_: &AnyObject, _: Sel, _t: *mut NSString) { log("ax.setSelectedText", String::new(), true); }
unsafe extern "C-unwind" fn set_value(_: &AnyObject, _: Sel, _t: *mut AnyObject) { log("ax.setValue", String::new(), true); }

// ── a scroll bar element (S9_SCROLLNOTE=bar): a native scroll view's bar posts AXValueChanged at every scroll step, and that
// is the only thing a text view's scroll tells the outside (step 3e)
unsafe extern "C-unwind" fn bar_role(_: &AnyObject, _: Sel) -> *const NSString { ns_string!("AXScrollBar") }
unsafe extern "C-unwind" fn bar_value(_: &AnyObject, _: Sel) -> *mut AnyObject { let a = app(); let h = a.nx_doc_frame().3.max(1.0); let v = (a.scroll as f64 / a.ctx.as_ref().map(|c| c.window.scale_factor()).unwrap_or(1.0) / h).clamp(0.0, 1.0); log("bar.value", String::new(), false); unsafe { msg_send![objc2::class!(NSNumber), numberWithDouble: v] } }
unsafe extern "C-unwind" fn bar_frame(_: &AnyObject, _: Sel) -> NSRect { let a = app(); let s = a.ctx.as_ref().map(|c| (c.window.inner_size(), c.window.scale_factor())); let (w, h) = s.map(|(z, k)| (z.width as f64 / k, z.height as f64 / k)).unwrap_or((0.0, 0.0)); to_screen((w - 16.0, 0.0, 16.0, h)) }

// ── a scroll area around the text and the bar (S9_SCROLLNOTE=area): the hierarchy a native scroll view shows —
// AXScrollArea { AXTextArea, AXScrollBar }, its frame the viewport, the text's frame moving inside it
unsafe extern "C-unwind" fn area_role(_: &AnyObject, _: Sel) -> *const NSString { ns_string!("AXScrollArea") }
unsafe extern "C-unwind" fn area_frame(_: &AnyObject, _: Sel) -> NSRect { let a = app(); let s = a.ctx.as_ref().map(|c| (c.window.inner_size(), c.window.scale_factor())); let (w, h) = s.map(|(z, k)| (z.width as f64 / k, z.height as f64 / k)).unwrap_or((0.0, 0.0)); let f = to_screen((0.0, 0.0, w, h)); log("area.frame", fmt(f), false); f }
unsafe extern "C-unwind" fn area_children(_: &AnyObject, _: Sel) -> *mut AnyObject { log("area.children", String::new(), false); unsafe { let a: *mut AnyObject = msg_send![objc2::class!(NSArray), arrayWithObject: ELEMENT.get()]; msg_send![a, arrayByAddingObject: BAR.get()] } }
unsafe extern "C-unwind" fn area_contents(_: &AnyObject, _: Sel) -> *mut AnyObject { log("area.contents", String::new(), false); unsafe { msg_send![objc2::class!(NSArray), arrayWithObject: ELEMENT.get()] } }
unsafe extern "C-unwind" fn area_bar(_: &AnyObject, _: Sel) -> *mut AnyObject { log("area.verticalScrollBar", String::new(), false); BAR.get() }

// ── the view's three methods, on top of AccessKit's
unsafe extern "C-unwind" fn v_children(this: &AnyObject, _: Sel) -> *mut AnyObject {
    unsafe { let below: *mut AnyObject = msg_send![super(this, &*BELOW.get()), accessibilityChildren];
        let with: *mut AnyObject = if below.is_null() { msg_send![objc2::class!(NSArray), arrayWithObject: ELEMENT.get()] } else { msg_send![below, arrayByAddingObject: ELEMENT.get()] };
        if !AREA.get().is_null() { return if below.is_null() { msg_send![objc2::class!(NSArray), arrayWithObject: AREA.get()] } else { msg_send![below, arrayByAddingObject: AREA.get()] } }
        if BAR.get().is_null() { with } else { msg_send![with, arrayByAddingObject: BAR.get()] } }
}
unsafe extern "C-unwind" fn v_focus(_: &AnyObject, _: Sel) -> *mut AnyObject { log("view.focusedUIElement", String::new(), false); ELEMENT.get() }
unsafe extern "C-unwind" fn v_hit(_: &AnyObject, _: Sel, _p: NSPoint) -> *mut AnyObject { ELEMENT.get() }

/// Call AFTER AccessKit's adapter exists (its subclass must be under ours). `view` is winit's NSView.
pub fn install(app: *mut App, view: *mut AnyObject, log_path: Option<String>) {
    APP.set(app); VIEW.set(view);
    if let Some(p) = log_path { if let Ok(f) = std::fs::File::create(&p) { LOG.set(Some(Log { t0: Instant::now(), out: std::io::BufWriter::new(f), counts: BTreeMap::new(), last: BTreeMap::new() })); } }
    unsafe {
        let omit: Vec<String> = std::env::var("S9_OMIT").unwrap_or_default().split(',').map(|x| x.to_string()).collect();   // selectors to leave off (which ones does a tool qualify an element by?)
        let mut b = ClassBuilder::new(c"MinS9TextElement", objc2::class!(NSAccessibilityElement)).expect("element class");
        b.add_method(sel!(isAccessibilityElement), yes as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(isAccessibilityEnabled), yes as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(isAccessibilityFocused), yes as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(respondsToSelector:), responds as unsafe extern "C-unwind" fn(_, _, _) -> _);
        b.add_method(sel!(accessibilityRole), role as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityLabel), label as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityParent), parent as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityWindow), window as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityTopLevelUIElement), window as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityFrame), frame as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityValue), value as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityNumberOfCharacters), n_chars as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilitySelectedTextRange), sel_range as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilitySelectedText), sel_text as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(setAccessibilitySelectedTextRange:), set_sel_range as unsafe extern "C-unwind" fn(_, _, _));
        b.add_method(sel!(accessibilityStringForRange:), string_for as unsafe extern "C-unwind" fn(_, _, _) -> _);
        b.add_method(sel!(accessibilityFrameForRange:), frame_for as unsafe extern "C-unwind" fn(_, _, _) -> _);
        b.add_method(sel!(accessibilityVisibleCharacterRange), visible_range as unsafe extern "C-unwind" fn(_, _) -> _);
        if !omit.iter().any(|o| o == stringify!(accessibilityInsertionPointLineNumber)) { b.add_method(sel!(accessibilityInsertionPointLineNumber), zero as unsafe extern "C-unwind" fn(_, _) -> _); }
        if !omit.iter().any(|o| o == stringify!(accessibilityLineForIndex)) { b.add_method(sel!(accessibilityLineForIndex:), line_for as unsafe extern "C-unwind" fn(_, _, _) -> _); }
        if !omit.iter().any(|o| o == stringify!(accessibilityRangeForLine)) { b.add_method(sel!(accessibilityRangeForLine:), range_for_line as unsafe extern "C-unwind" fn(_, _, _) -> _); }
        if !omit.iter().any(|o| o == stringify!(accessibilityRangeForIndex)) { b.add_method(sel!(accessibilityRangeForIndex:), range_for_index as unsafe extern "C-unwind" fn(_, _, _) -> _); }
        if !omit.iter().any(|o| o == stringify!(accessibilityAttributedStringForRange)) { b.add_method(sel!(accessibilityAttributedStringForRange:), attributed_for as unsafe extern "C-unwind" fn(_, _, _) -> _); }
        if !omit.iter().any(|o| o == stringify!(setAccessibilitySelectedText)) { b.add_method(sel!(setAccessibilitySelectedText:), set_sel_text as unsafe extern "C-unwind" fn(_, _, _)); }
        if !omit.iter().any(|o| o == stringify!(setAccessibilityValue)) { b.add_method(sel!(setAccessibilityValue:), set_value as unsafe extern "C-unwind" fn(_, _, _)); }
        let element: *mut AnyObject = msg_send![b.register(), new];
        ELEMENT.set(element);

        let mode = std::env::var("S9_SCROLLNOTE").unwrap_or_default();
        if mode == "area" {
            let mut sa = ClassBuilder::new(c"MinS9ScrollArea", objc2::class!(NSAccessibilityElement)).expect("area class");
            sa.add_method(sel!(isAccessibilityElement), yes as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(isAccessibilityEnabled), yes as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(accessibilityRole), area_role as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(accessibilityParent), parent as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(accessibilityWindow), window as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(accessibilityTopLevelUIElement), window as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(accessibilityFrame), area_frame as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(accessibilityChildren), area_children as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(accessibilityContents), area_contents as unsafe extern "C-unwind" fn(_, _) -> _);
            sa.add_method(sel!(accessibilityVerticalScrollBar), area_bar as unsafe extern "C-unwind" fn(_, _) -> _);
            let area: *mut AnyObject = msg_send![sa.register(), new]; AREA.set(area);
        }
        if mode == "bar" || mode == "area" {
            let mut sb = ClassBuilder::new(c"MinS9ScrollBar", objc2::class!(NSAccessibilityElement)).expect("bar class");
            sb.add_method(sel!(isAccessibilityElement), yes as unsafe extern "C-unwind" fn(_, _) -> _);
            sb.add_method(sel!(isAccessibilityEnabled), yes as unsafe extern "C-unwind" fn(_, _) -> _);
            sb.add_method(sel!(accessibilityRole), bar_role as unsafe extern "C-unwind" fn(_, _) -> _);
            sb.add_method(sel!(accessibilityParent), parent as unsafe extern "C-unwind" fn(_, _) -> _);
            sb.add_method(sel!(accessibilityWindow), window as unsafe extern "C-unwind" fn(_, _) -> _);
            sb.add_method(sel!(accessibilityTopLevelUIElement), window as unsafe extern "C-unwind" fn(_, _) -> _);
            sb.add_method(sel!(accessibilityValue), bar_value as unsafe extern "C-unwind" fn(_, _) -> _);
            sb.add_method(sel!(accessibilityFrame), bar_frame as unsafe extern "C-unwind" fn(_, _) -> _);
            let bar: *mut AnyObject = msg_send![sb.register(), new]; BAR.set(bar);
        }
        let below: &'static AnyClass = (*view).class();   // the runtime class: AccessKit's subclass of winit's view
        BELOW.set(below);
        let name = std::ffi::CString::new(format!("MinS9TextOver{}", below.name().to_string_lossy())).unwrap();
        let mut v = ClassBuilder::new(&name, below).expect("view subclass");
        v.add_method(sel!(accessibilityChildren), v_children as unsafe extern "C-unwind" fn(_, _) -> _);
        v.add_method(sel!(accessibilityFocusedUIElement), v_focus as unsafe extern "C-unwind" fn(_, _) -> _);
        v.add_method(sel!(accessibilityHitTest:), v_hit as unsafe extern "C-unwind" fn(_, _, _) -> _);
        objc2::ffi::object_setClass(view, v.register() as *const AnyClass);
        println!("S9 native element installed over {}", below.name().to_string_lossy());
    }
    log("START", String::new(), true);
}
/// After an edit or a selection change. NEVER after a scroll: a text view posts nothing then (spike step 3e).
pub fn note(value_changed: bool) {
    let e = ELEMENT.get(); if e.is_null() { return }
    unsafe { if value_changed { NSAccessibilityPostNotification(e, ns_string!("AXValueChanged")); } NSAccessibilityPostNotification(e, ns_string!("AXSelectedTextChanged")); }
}
/// The window took focus: name OUR element as the focused one (AccessKit has just announced its own root; a writing tool
/// takes the element the announcement carries).
pub fn note_focus() { let e = ELEMENT.get(); if e.is_null() { return } log("POST focusedUIElementChanged", String::new(), true); unsafe { NSAccessibilityPostNotification(e, ns_string!("AXFocusedUIElementChanged")); } }
/// The view scrolled. What to tell the outside is the experiment (S9_SCROLLNOTE): nothing (default), `moved` (AXMoved on
/// the text element — its frame did move), `layout` (AXLayoutChanged, the 2026-11-10 behaviour), `bar` (AXValueChanged on a
/// scroll bar element, as a native scroll view).
pub fn note_scroll() {
    let e = ELEMENT.get(); if e.is_null() { return }
    unsafe { match std::env::var("S9_SCROLLNOTE").as_deref() {
        Ok("moved") => NSAccessibilityPostNotification(e, ns_string!("AXMoved")),
        Ok("layout") => NSAccessibilityPostNotification(e, ns_string!("AXLayoutChanged")),
        Ok("bar") | Ok("area") => NSAccessibilityPostNotification(BAR.get(), ns_string!("AXValueChanged")),
        _ => {} } }
}
