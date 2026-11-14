//! The note's text as the desktop sees it on macOS (canon rows 276, 281; the spike S9's verdict): ONE hand-written
//! `NSAccessibilityElement` for the whole note — a flat UTF-16 text over the blocks — inside a scroll area with a scroll
//! bar, hung on winit's view by one dynamic subclass. A writing tool (Grammarly) finds it, reads it, asks where ranges
//! are, and corrects by setting the selection and then TYPING; nothing is written through accessibility.
//!
//! What the element must be (each line is a finding of the spike):
//! - `setAccessibilityValue:` is DECLARED — the tool takes the element for editable text by that alone (never called);
//! - its frame is the WHOLE note's, moving with the scroll; a scroll posts no notification on the text;
//! - it sits in an `AXScrollArea` whose `AXScrollBar` posts `AXValueChanged` at each scroll step;
//! - a selection set from outside is honoured exactly, in UTF-16 units.
//! Main thread only: the element reaches the editor through a pointer the window sets while it lives.
use objc2::{msg_send, rc::Retained, runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel}, sel};
use objc2_foundation::{ns_string, NSPoint, NSRange, NSRect, NSSize, NSString};
use std::{cell::{Cell, RefCell}, collections::BTreeMap, io::Write, time::Instant};

/// What the element asks of the editor. Rectangles are in the view's points, origin top-left.
pub trait TextHost {
    /// The whole note as one text.
    fn ax_text(&mut self) -> String;
    fn ax_len16(&mut self) -> usize;
    /// (location, length) in UTF-16 units.
    fn ax_selection(&mut self) -> (usize, usize);
    fn ax_set_selection(&mut self, location: usize, length: usize);
    fn ax_substring(&mut self, location: usize, length: usize) -> String;
    /// One rectangle around a range (a caret's box for an empty one).
    fn ax_range_rect(&mut self, location: usize, length: usize) -> (f64, f64, f64, f64);
    /// The whole note where it is now (it moves as the view scrolls).
    fn ax_note_frame(&mut self) -> (f64, f64, f64, f64);
    /// The view's size.
    fn ax_viewport(&mut self) -> (f64, f64);
    /// 0 at the top, 1 at the bottom.
    fn ax_scroll_fraction(&mut self) -> f64;
    /// The system's text input replaces the character before the caret (the accent chosen after a long press).
    fn replace_before_caret(&mut self, text: &str);
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" { fn NSAccessibilityPostNotification(element: *const AnyObject, notification: *const NSString); }

thread_local! {
    static HOST: Cell<Option<*mut dyn TextHost>> = const { Cell::new(None) };
    static VIEW: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
    static TEXT: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
    static AREA: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
    static BAR: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
    static BELOW: Cell<*const AnyClass> = const { Cell::new(std::ptr::null()) };
    // something stands over the note (a dialog, a name being edited): the text is not the focused element and gives no geometry
    static OBSCURED: Cell<bool> = const { Cell::new(false) };
    // what the system's text input did with the key being handled (see `key_fate`)
    static INSERTED: Cell<bool> = const { Cell::new(false) };
    static COMMAND: Cell<bool> = const { Cell::new(false) };
    static REPLACED: Cell<Option<Instant>> = const { Cell::new(None) };
    static LOG: RefCell<Option<(Instant, std::fs::File, BTreeMap<&'static str, u64>)>> = const { RefCell::new(None) };
}
/// `MN_AX_LOG=<file>`: the first call of each kind and, at the end, how often each came (to see a tool attach).
fn log(name: &'static str, detail: String) {
    LOG.with_borrow_mut(|l| { let Some((t0, out, counts)) = l.as_mut() else { return }; let n = counts.entry(name).or_default(); *n += 1; if *n <= 3 || name == "setSelectedTextRange" { let _ = writeln!(out, "{:9.3}  {name} {detail}", t0.elapsed().as_secs_f64()); } });
}
/// `MN_KEY_LOG=<file>`: every key press as the app saw it and what the system's text input did with it.
pub fn key_log(line: String) {
    thread_local! { static OUT: RefCell<Option<Option<std::fs::File>>> = const { RefCell::new(None) }; }
    OUT.with_borrow_mut(|o| { let f = o.get_or_insert_with(|| std::env::var_os("MN_KEY_LOG").and_then(|p| std::fs::File::create(p).ok())); if let Some(f) = f { let _ = writeln!(f, "{line}"); } });
}
pub fn log_summary() { LOG.with_borrow_mut(|l| { if let Some((_, out, counts)) = l.as_mut() { let _ = writeln!(out, "-- call counts --"); for (k, v) in counts.iter() { let _ = writeln!(out, "{v:8}  {k}"); } } }); }

fn host() -> Option<&'static mut dyn TextHost> { HOST.get().map(|p| unsafe { &mut *p }) }
fn nsstr(s: &str) -> *mut NSString { Retained::autorelease_ptr(NSString::from_str(s)) }
/// View points (top-left origin, winit's flipped view) → Cocoa screen coordinates.
fn to_screen(r: (f64, f64, f64, f64)) -> NSRect {
    let view = VIEW.get(); let rect = NSRect::new(NSPoint::new(r.0, r.1), NSSize::new(r.2, r.3));
    unsafe { let window: *mut AnyObject = msg_send![view, window]; if window.is_null() { return rect }
        let in_window: NSRect = msg_send![view, convertRect: rect, toView: std::ptr::null::<AnyObject>()];
        msg_send![window, convertRectToScreen: in_window] }
}
const ZERO: NSRect = NSRect { origin: NSPoint { x: 0.0, y: 0.0 }, size: NSSize { width: 0.0, height: 0.0 } };

// ── shared
unsafe extern "C-unwind" fn yes(_: &AnyObject, _: Sel) -> Bool { Bool::YES }
unsafe extern "C-unwind" fn window(_: &AnyObject, _: Sel) -> *mut AnyObject { unsafe { msg_send![VIEW.get(), window] } }
/// The text and the bar hang in the area; the area hangs where the view does (the view itself is not an element).
unsafe extern "C-unwind" fn parent(this: &AnyObject, _: Sel) -> *mut AnyObject { if !std::ptr::eq(this, AREA.get()) { return AREA.get() } unsafe { msg_send![VIEW.get(), accessibilityParent] } }

// ── the text element
unsafe extern "C-unwind" fn role(_: &AnyObject, _: Sel) -> *const NSString { ns_string!("AXTextArea") }
unsafe extern "C-unwind" fn label(_: &AnyObject, _: Sel) -> *const NSString { ns_string!("Note") }
unsafe extern "C-unwind" fn frame(_: &AnyObject, _: Sel) -> NSRect { log("frame", String::new()); host().map(|h| to_screen(h.ax_note_frame())).unwrap_or(ZERO) }
unsafe extern "C-unwind" fn value(_: &AnyObject, _: Sel) -> *mut NSString { let s = host().map(|h| h.ax_text()).unwrap_or_default(); log("value", format!("{} bytes", s.len())); nsstr(&s) }
unsafe extern "C-unwind" fn n_chars(_: &AnyObject, _: Sel) -> isize { host().map(|h| h.ax_len16()).unwrap_or(0) as isize }
unsafe extern "C-unwind" fn sel_range(_: &AnyObject, _: Sel) -> NSRange { let (l, n) = host().map(|h| h.ax_selection()).unwrap_or((0, 0)); log("selectedTextRange", format!("{{{l},{n}}}")); NSRange::new(l, n) }
unsafe extern "C-unwind" fn sel_text(_: &AnyObject, _: Sel) -> *mut NSString { let s = host().map(|h| { let (l, n) = h.ax_selection(); h.ax_substring(l, n) }).unwrap_or_default(); log("selectedText", String::new()); nsstr(&s) }
unsafe extern "C-unwind" fn set_sel_range(_: &AnyObject, _: Sel, r: NSRange) { log("setSelectedTextRange", format!("{{{},{}}}", r.location, r.length)); if let Some(h) = host() { h.ax_set_selection(r.location, r.length); } }
unsafe extern "C-unwind" fn string_for(_: &AnyObject, _: Sel, r: NSRange) -> *mut NSString { log("stringForRange", String::new()); nsstr(&host().map(|h| h.ax_substring(r.location, r.length)).unwrap_or_default()) }
unsafe extern "C-unwind" fn focused_now(_: &AnyObject, _: Sel) -> Bool { Bool::new(!OBSCURED.get()) }
unsafe extern "C-unwind" fn frame_for(_: &AnyObject, _: Sel, r: NSRange) -> NSRect { log("frameForRange", format!("{{{},{}}}", r.location, r.length)); if OBSCURED.get() { return ZERO } host().map(|h| to_screen(h.ax_range_rect(r.location, r.length))).unwrap_or(ZERO) }
unsafe extern "C-unwind" fn visible_range(_: &AnyObject, _: Sel) -> NSRange { NSRange::new(0, host().map(|h| h.ax_len16()).unwrap_or(0)) }
// Declared because a text element answers them; the note reads as one "line" until a screen reader's needs are taken up.
unsafe extern "C-unwind" fn zero(_: &AnyObject, _: Sel) -> isize { 0 }
unsafe extern "C-unwind" fn line_for(_: &AnyObject, _: Sel, _i: isize) -> isize { 0 }
unsafe extern "C-unwind" fn range_for_line(_: &AnyObject, _: Sel, _l: isize) -> NSRange { NSRange::new(0, host().map(|h| h.ax_len16()).unwrap_or(0)) }
unsafe extern "C-unwind" fn range_for_index(_: &AnyObject, _: Sel, i: isize) -> NSRange { let n = host().map(|h| h.ax_len16()).unwrap_or(0); let i = (i.max(0) as usize).min(n); NSRange::new(i, (i < n) as usize) }
unsafe extern "C-unwind" fn attributed_for(_: &AnyObject, _: Sel, r: NSRange) -> *mut AnyObject { let s = host().map(|h| h.ax_substring(r.location, r.length)).unwrap_or_default(); unsafe { let a: *mut AnyObject = msg_send![objc2::class!(NSAttributedString), alloc]; let a: *mut AnyObject = msg_send![a, initWithString: nsstr(&s)]; msg_send![a, autorelease] } }
// Declared, never honoured: a tool qualifies the element by `setAccessibilityValue:` and writes by typing.
unsafe extern "C-unwind" fn set_sel_text(_: &AnyObject, _: Sel, _t: *mut NSString) { log("setSelectedText (ignored)", String::new()); }
unsafe extern "C-unwind" fn set_value(_: &AnyObject, _: Sel, _t: *mut AnyObject) { log("setValue (ignored)", String::new()); }

// ── the scroll area and its bar
unsafe extern "C-unwind" fn area_role(_: &AnyObject, _: Sel) -> *const NSString { ns_string!("AXScrollArea") }
unsafe extern "C-unwind" fn area_frame(_: &AnyObject, _: Sel) -> NSRect { host().map(|h| { let (w, ht) = h.ax_viewport(); to_screen((0.0, 0.0, w, ht)) }).unwrap_or(ZERO) }
unsafe extern "C-unwind" fn area_children(_: &AnyObject, _: Sel) -> *mut AnyObject { unsafe { let a: *mut AnyObject = msg_send![objc2::class!(NSArray), arrayWithObject: TEXT.get()]; msg_send![a, arrayByAddingObject: BAR.get()] } }
unsafe extern "C-unwind" fn area_contents(_: &AnyObject, _: Sel) -> *mut AnyObject { unsafe { msg_send![objc2::class!(NSArray), arrayWithObject: TEXT.get()] } }
unsafe extern "C-unwind" fn area_bar(_: &AnyObject, _: Sel) -> *mut AnyObject { BAR.get() }
unsafe extern "C-unwind" fn bar_role(_: &AnyObject, _: Sel) -> *const NSString { ns_string!("AXScrollBar") }
unsafe extern "C-unwind" fn bar_value(_: &AnyObject, _: Sel) -> *mut AnyObject { let v = host().map(|h| h.ax_scroll_fraction()).unwrap_or(0.0); unsafe { msg_send![objc2::class!(NSNumber), numberWithDouble: v] } }
unsafe extern "C-unwind" fn bar_frame(_: &AnyObject, _: Sel) -> NSRect { host().map(|h| { let (w, ht) = h.ax_viewport(); to_screen((w - 16.0, 0.0, 16.0, ht)) }).unwrap_or(ZERO) }

// ── the view: its children are the area (after whatever the class below reports), focus and hit-test are the text
unsafe extern "C-unwind" fn v_children(this: &AnyObject, _: Sel) -> *mut AnyObject {
    unsafe { let below: *mut AnyObject = msg_send![super(this, &*BELOW.get()), accessibilityChildren];
        if below.is_null() { msg_send![objc2::class!(NSArray), arrayWithObject: AREA.get()] } else { msg_send![below, arrayByAddingObject: AREA.get()] } }
}
unsafe extern "C-unwind" fn v_focus(_: &AnyObject, _: Sel) -> *mut AnyObject { log("view.focusedUIElement", String::new()); if OBSCURED.get() { return std::ptr::null_mut() } TEXT.get() }
unsafe extern "C-unwind" fn v_hit(_: &AnyObject, _: Sel, _p: NSPoint) -> *mut AnyObject { TEXT.get() }

// ── the view's text input: winit forwards a key to the app whatever the system's text input made of it. Two of the
// system's answers have to be told apart from "an ordinary key" — it SWALLOWED the key (a held letter whose accents are
// on offer, a key that steers the accent popup), or it REPLACED the letter before the caret (the accent chosen).
unsafe extern "C-unwind" fn v_insert_text(this: &AnyObject, _: Sel, string: *mut AnyObject, range: NSRange) {
    INSERTED.set(true);
    key_log(format!("  insertText {:?} range {{{},{}}}", unsafe { let is_attr: Bool = msg_send![string, isKindOfClass: objc2::class!(NSAttributedString)]; let s: *mut NSString = if is_attr.as_bool() { msg_send![string, string] } else { string.cast() }; (*s).to_string() }, range.location as isize, range.length));
    if range.location != isize::MAX as usize && range.location != usize::MAX && range.length > 0 {
        let text: String = unsafe { let is_attr: Bool = msg_send![string, isKindOfClass: objc2::class!(NSAttributedString)]; let s: *mut NSString = if is_attr.as_bool() { msg_send![string, string] } else { string.cast() }; (*s).to_string() };
        log("insertText (replacing)", format!("{text:?} over {{{},{}}}", range.location, range.length));
        if let Some(h) = host() { h.replace_before_caret(&text); }
        REPLACED.set(Some(Instant::now()));
    }
    unsafe { let _: () = msg_send![super(this, &*BELOW.get()), insertText: string, replacementRange: range]; }
}
/// The system's text input asks where the selection is and what text is there before it offers a held letter's accents
/// (winit answers "no selection" and "no text", and the system then just repeats the letter). While an input method is
/// composing, winit's own answers stand.
unsafe extern "C-unwind" fn v_selected_range(this: &AnyObject, _: Sel) -> NSRange {
    let marked: Bool = unsafe { msg_send![this, hasMarkedText] };
    if marked.as_bool() { return unsafe { msg_send![super(this, &*BELOW.get()), selectedRange] } }
    match host() { Some(h) => { let (l, n) = h.ax_selection(); NSRange::new(l, n) } None => NSRange::new(isize::MAX as usize, 0) }
}
unsafe extern "C-unwind" fn v_substring(_: &AnyObject, _: Sel, range: NSRange, actual: *mut NSRange) -> *mut AnyObject {
    let Some(h) = host() else { return std::ptr::null_mut() };
    let len = h.ax_len16(); let start = range.location.min(len); let n = range.length.min(len - start);
    if !actual.is_null() { unsafe { *actual = NSRange::new(start, n); } }
    let s = h.ax_substring(start, n);
    unsafe { let a: *mut AnyObject = msg_send![objc2::class!(NSAttributedString), alloc]; let a: *mut AnyObject = msg_send![a, initWithString: nsstr(&s)]; msg_send![a, autorelease] }
}
unsafe extern "C-unwind" fn v_do_command(this: &AnyObject, _: Sel, command: Sel) {
    COMMAND.set(true);
    key_log(format!("  doCommand {}", command.name().to_string_lossy()));
    unsafe { let _: () = msg_send![super(this, &*BELOW.get()), doCommandBySelector: command]; }
}
/// What became of the key press being handled: `Typed` — the system's text input passed it on as text or as a command
/// (the app acts on it); `Swallowed` — the text input kept it for itself (the app ignores it); `Replaced` — it turned
/// into a replacement that has already been applied (the app ignores the key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFate { Typed, Swallowed, Replaced }
pub fn key_fate() -> KeyFate {
    let (inserted, command, replaced) = (INSERTED.replace(false), COMMAND.replace(false), REPLACED.take());
    if replaced.is_some_and(|t| t.elapsed().as_millis() < 100) { KeyFate::Replaced } else if inserted || command { KeyFate::Typed } else { KeyFate::Swallowed }
}

/// Hangs the elements on `view` (winit's NSView). `host` must outlive the window; call [`uninstall`] before it goes.
pub fn install(host: *mut dyn TextHost, view: *mut AnyObject) {
    HOST.set(Some(host)); VIEW.set(view);
    if let Some(f) = std::env::var_os("MN_AX_LOG").and_then(|p| std::fs::File::create(p).ok()) { LOG.set(Some((Instant::now(), f, BTreeMap::new()))); }
    unsafe {
        let element = |name: &std::ffi::CStr| ClassBuilder::new(name, objc2::class!(NSAccessibilityElement)).expect("accessibility class");
        let common = |b: &mut ClassBuilder| {
            b.add_method(sel!(isAccessibilityElement), yes as unsafe extern "C-unwind" fn(_, _) -> _);
            b.add_method(sel!(isAccessibilityEnabled), yes as unsafe extern "C-unwind" fn(_, _) -> _);
            b.add_method(sel!(accessibilityParent), parent as unsafe extern "C-unwind" fn(_, _) -> _);
            b.add_method(sel!(accessibilityWindow), window as unsafe extern "C-unwind" fn(_, _) -> _);
            b.add_method(sel!(accessibilityTopLevelUIElement), window as unsafe extern "C-unwind" fn(_, _) -> _);
        };
        let mut b = element(c"MinNotesTextElement"); common(&mut b);
        b.add_method(sel!(isAccessibilityFocused), focused_now as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityRole), role as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityLabel), label as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityFrame), frame as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityValue), value as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(setAccessibilityValue:), set_value as unsafe extern "C-unwind" fn(_, _, _));
        b.add_method(sel!(accessibilityNumberOfCharacters), n_chars as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilitySelectedTextRange), sel_range as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(setAccessibilitySelectedTextRange:), set_sel_range as unsafe extern "C-unwind" fn(_, _, _));
        b.add_method(sel!(accessibilitySelectedText), sel_text as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(setAccessibilitySelectedText:), set_sel_text as unsafe extern "C-unwind" fn(_, _, _));
        b.add_method(sel!(accessibilityStringForRange:), string_for as unsafe extern "C-unwind" fn(_, _, _) -> _);
        b.add_method(sel!(accessibilityAttributedStringForRange:), attributed_for as unsafe extern "C-unwind" fn(_, _, _) -> _);
        b.add_method(sel!(accessibilityFrameForRange:), frame_for as unsafe extern "C-unwind" fn(_, _, _) -> _);
        b.add_method(sel!(accessibilityVisibleCharacterRange), visible_range as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityInsertionPointLineNumber), zero as unsafe extern "C-unwind" fn(_, _) -> _);
        b.add_method(sel!(accessibilityLineForIndex:), line_for as unsafe extern "C-unwind" fn(_, _, _) -> _);
        b.add_method(sel!(accessibilityRangeForLine:), range_for_line as unsafe extern "C-unwind" fn(_, _, _) -> _);
        b.add_method(sel!(accessibilityRangeForIndex:), range_for_index as unsafe extern "C-unwind" fn(_, _, _) -> _);
        TEXT.set(msg_send![b.register(), new]);
        let mut a = element(c"MinNotesScrollArea"); common(&mut a);
        a.add_method(sel!(accessibilityRole), area_role as unsafe extern "C-unwind" fn(_, _) -> _);
        a.add_method(sel!(accessibilityFrame), area_frame as unsafe extern "C-unwind" fn(_, _) -> _);
        a.add_method(sel!(accessibilityChildren), area_children as unsafe extern "C-unwind" fn(_, _) -> _);
        a.add_method(sel!(accessibilityContents), area_contents as unsafe extern "C-unwind" fn(_, _) -> _);
        a.add_method(sel!(accessibilityVerticalScrollBar), area_bar as unsafe extern "C-unwind" fn(_, _) -> _);
        AREA.set(msg_send![a.register(), new]);
        let mut s = element(c"MinNotesScrollBar"); common(&mut s);
        s.add_method(sel!(accessibilityRole), bar_role as unsafe extern "C-unwind" fn(_, _) -> _);
        s.add_method(sel!(accessibilityValue), bar_value as unsafe extern "C-unwind" fn(_, _) -> _);
        s.add_method(sel!(accessibilityFrame), bar_frame as unsafe extern "C-unwind" fn(_, _) -> _);
        BAR.set(msg_send![s.register(), new]);
        let below: &'static AnyClass = (*view).class();
        BELOW.set(below);
        let name = std::ffi::CString::new(format!("MinNotesTextOver{}", below.name().to_string_lossy())).unwrap();
        let mut v = ClassBuilder::new(&name, below).expect("view subclass");
        v.add_method(sel!(accessibilityChildren), v_children as unsafe extern "C-unwind" fn(_, _) -> _);
        v.add_method(sel!(accessibilityFocusedUIElement), v_focus as unsafe extern "C-unwind" fn(_, _) -> _);
        v.add_method(sel!(accessibilityHitTest:), v_hit as unsafe extern "C-unwind" fn(_, _, _) -> _);
        v.add_method(sel!(insertText:replacementRange:), v_insert_text as unsafe extern "C-unwind" fn(_, _, _, _));
        v.add_method(sel!(doCommandBySelector:), v_do_command as unsafe extern "C-unwind" fn(_, _, _));
        v.add_method(sel!(selectedRange), v_selected_range as unsafe extern "C-unwind" fn(_, _) -> _);
        v.add_method(sel!(attributedSubstringForProposedRange:actualRange:), v_substring as unsafe extern "C-unwind" fn(_, _, _, _) -> _);
        objc2::ffi::object_setClass(view, v.register() as *const AnyClass);
    }
}
/// The host is going away: the elements answer with nothing from now on.
pub fn uninstall() { log_summary(); HOST.set(None); }

fn post(element: *mut AnyObject, name: &NSString) { if !element.is_null() { unsafe { NSAccessibilityPostNotification(element, name); } } }
/// After an edit (`text` true) or a selection change. NEVER after a scroll.
pub fn changed(text: bool) { if text { key_log("post AXValueChanged (the text is another's, or was edited)".into()); } if text { post(TEXT.get(), ns_string!("AXValueChanged")); } post(TEXT.get(), ns_string!("AXSelectedTextChanged")); }
/// The view scrolled: only the scroll bar says so (what a native scroll view does).
pub fn scrolled() { post(BAR.get(), ns_string!("AXValueChanged")); }
/// The window took focus: name the text as the focused element (unless something stands over the note).
pub fn focused() { if !OBSCURED.get() { post(TEXT.get(), ns_string!("AXFocusedUIElementChanged")); } }
/// Something stands over the note (a dialog, a name being edited in the tree) or has gone again. While it does, the text
/// is not the focused element and answers no geometry — a writing tool takes its underlines away instead of drawing
/// them over the dialog.
pub fn set_obscured(on: bool) {
    if OBSCURED.replace(on) == on { return }
    if on { let window: *mut AnyObject = unsafe { msg_send![VIEW.get(), window] }; post(window, ns_string!("AXFocusedUIElementChanged")); post(TEXT.get(), ns_string!("AXLayoutChanged")); }
    else { post(TEXT.get(), ns_string!("AXFocusedUIElementChanged")); post(TEXT.get(), ns_string!("AXLayoutChanged")); }
}
/// The note sits elsewhere or is shaped differently WITHOUT having scrolled or been edited: the sidebar's width changed,
/// the window was resized or moved. (A scroll is told by the scroll bar alone — spike S9; this is the other case, where
/// every range's rectangle has to be asked again.)
pub fn relaid() { let t = TEXT.get(); post(t, ns_string!("AXMoved")); post(t, ns_string!("AXResized")); post(t, ns_string!("AXLayoutChanged")); }
