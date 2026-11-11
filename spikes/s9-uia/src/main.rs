//! S9 step 5 — the Windows reference window (docs/spikes/S9-desktop-text.md). A hand-drawn text window (fixed-pitch GDI
//! text, our own wrap) whose HWND answers UI Automation through a hand-written provider — Text, Value and Scroll patterns —
//! with a logger on every provider call and on every input message. Run a writing tool against it and read the log.
//!   --file <text>      the text (default: the flawed sample);  --seconds N   close after N seconds
//!   S9_LOG=<file>      where the log goes (default .\s9-uia.log)
//!   S9_CT=edit         control type Edit instead of Document
//!   S9_OFF=a,b         withhold: value, scroll, text, events, visible (no visible range), ids (no class name / automation id),
//!                      scrollevent (no property-changed event when the view scrolls)
//!   S9_TREE=child      the text is a CHILD element of the window (a fragment under a fragment root), as a toolkit's tree has it
#[cfg(not(windows))]
fn main() { eprintln!("s9-uia is the Windows leg of the spike S9"); }

#[cfg(windows)]
fn main() { win::run(); }

#[cfg(windows)]
#[allow(non_upper_case_globals)]
mod win {
    use std::{cell::{Cell, RefCell}, collections::HashMap, io::Write, time::Instant};
    use windows::{core::*, Win32::{Foundation::*, Graphics::Gdi::*, System::{Com::*, DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*, Ole::*, Variant::*}, UI::{Accessibility::*, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*}}};

    const SAMPLE: &str = "Their are several reasons why this document have mistakes in it. Each one are here on purpose, so that a writing tool has something to find and to fix.\n\nThe first paragraph is plane prose. It should of been checked for speling, but nobody did. We was in a hurry and the the deadline were close.\n\nA second idea: when a correction is accepted, the right words must change and nothing else. If the the wrong range is replaced, the tool and the editor disagrees about where the text is.\n\nHere is a line with an accent, caf\u{e9}, and a dash \u{2014} both are more then one byte in some encodings, wich is where counting usually go wrong.\n\nScrolling matters too. After the window is resized or scrolled, an underline that was drawed under a word must still sit under that word, not were the word used to be.\n\nThis last paragraph is corect on purpose. It gives the reader, and the tool, something to compare the others against.\n";

    // ───────────────────────────── the log
    struct Log { out: std::fs::File, t0: Instant, counts: HashMap<String, u64>, last: HashMap<String, String> }
    thread_local! {
        static LOG: RefCell<Option<Log>> = const { RefCell::new(None) };
        static DOC: RefCell<Doc> = RefCell::new(Doc::default());
        static ROOT: RefCell<Option<IRawElementProviderSimple>> = const { RefCell::new(None) };   // what WM_GETOBJECT hands out
        static ELEM: RefCell<Option<IRawElementProviderSimple>> = const { RefCell::new(None) };   // the text element (the same object unless S9_TREE=child)
    }
    /// One line per call; a call repeated with the same detail is counted, not re-printed.
    fn log(name: &str, detail: String, always: bool) {
        LOG.with_borrow_mut(|l| { let Some(l) = l.as_mut() else { return };
            *l.counts.entry(name.to_string()).or_default() += 1;
            if !always && l.last.get(name) == Some(&detail) { return }
            let d: String = detail.chars().map(|c| if c == '\n' { '\u{23ce}' } else { c }).take(110).collect();
            let _ = writeln!(l.out, "{:9.3}  {:<26} {}", l.t0.elapsed().as_secs_f64(), name, d);
            l.last.insert(name.to_string(), detail);
        });
    }
    fn summary() { LOG.with_borrow_mut(|l| { if let Some(l) = l.as_mut() { let _ = writeln!(l.out, "\n-- call counts --"); let mut c: Vec<_> = l.counts.iter().collect(); c.sort_by(|a, b| b.1.cmp(a.1)); for (k, v) in c { let _ = writeln!(l.out, "{v:8}  {k}"); } } }); }
    fn off(what: &str) -> bool { std::env::var("S9_OFF").unwrap_or_default().split(',').any(|x| x == what) }

    // ───────────────────────────── the document: flat UTF-16, fixed-pitch layout
    #[derive(Default)]
    struct Doc { text: Vec<u16>, anchor: usize, caret: usize, scroll: i32, lines: Vec<(usize, usize)>, cw: i32, lh: i32, pad: i32, w: i32, h: i32, hwnd: isize, font: isize, undo: Vec<(usize, Vec<u16>, Vec<u16>)> }
    impl Doc {
        fn len(&self) -> usize { self.text.len() }
        fn sel(&self) -> (usize, usize) { (self.anchor.min(self.caret), self.anchor.max(self.caret)) }
        fn s(&self, a: usize, b: usize) -> String { let (a, b) = (a.min(self.len()), b.min(self.len())); String::from_utf16_lossy(&self.text[a.min(b)..b]) }
        /// Lines as (start, end) — `end` excludes the line break; a wrapped line ends where the next begins.
        fn relayout(&mut self) {
            let cols = (((self.w - 2 * self.pad) / self.cw.max(1)).max(20)) as usize;
            self.lines.clear();
            let mut p = 0;
            loop {
                let e = self.text[p..].iter().position(|&c| c == 10).map(|i| p + i).unwrap_or(self.len());
                let mut s = p;
                while e - s > cols { let mut b = s + cols; while b > s && self.text[b] != 32 { b -= 1 } if b == s { b = s + cols } else { b += 1 } self.lines.push((s, b)); s = b; }
                self.lines.push((s, e));
                if e >= self.len() { break } p = e + 1;
            }
            let max = (self.lines.len() as i32 * self.lh + 2 * self.pad - self.h).max(0); self.scroll = self.scroll.clamp(0, max);
        }
        fn line_of(&self, o: usize) -> usize { match self.lines.binary_search_by(|&(s, _)| s.cmp(&o)) { Ok(i) => i, Err(i) => i.saturating_sub(1) } }
        /// Client rectangles of [a, b), one per line; a collapsed range gives the caret's box.
        fn rects(&self, a: usize, b: usize) -> Vec<(i32, i32, i32, i32)> {
            let mut out = vec![];
            let (la, lb) = (self.line_of(a), self.line_of(b));
            for i in la..=lb { let (s, e) = self.lines[i]; let (x0, x1) = (a.max(s).min(e), b.min(e).max(s)); if x1 < x0 { continue }
                if a != b && x1 == x0 { continue }
                out.push((self.pad + (x0 - s) as i32 * self.cw, self.pad + i as i32 * self.lh - self.scroll, ((x1 - x0) as i32 * self.cw).max(if a == b { 1 } else { 0 }), self.lh)); }
            out
        }
        fn hit(&self, x: i32, y: i32) -> usize { let i = (((y + self.scroll - self.pad) / self.lh.max(1)).max(0) as usize).min(self.lines.len() - 1); let (s, e) = self.lines[i]; (s + (((x - self.pad + self.cw / 2) / self.cw.max(1)).max(0) as usize)).min(e) }
        fn replace(&mut self, a: usize, b: usize, with: &[u16], why: &str) {
            let (a, b) = (a.min(self.len()), b.min(self.len()));
            log("EDIT", format!("{why}: {{{a},{}}} {:?} -> {:?}", b - a, self.s(a, b), String::from_utf16_lossy(with)), true);
            let old: Vec<u16> = self.text.splice(a..b, with.iter().copied()).collect();
            self.undo.push((a, old, with.to_vec()));
            self.caret = a + with.len(); self.anchor = self.caret; self.relayout(); self.reveal();
        }
        fn reveal(&mut self) { let y = self.line_of(self.caret) as i32 * self.lh; if y < self.scroll { self.scroll = y } else if y + self.lh + 2 * self.pad > self.scroll + self.h { self.scroll = y + self.lh + 2 * self.pad - self.h } }
        /// Unit boundaries (sorted, with 0 and len).
        fn bounds(&self, unit: TextUnit) -> Vec<usize> {
            let n = self.len(); let mut b = vec![0];
            if unit == TextUnit_Character { return (0..=n).collect() }
            if unit == TextUnit_Word { for i in 1..n { let (p, c) = (self.text[i - 1], self.text[i]); let sp = |u: u16| u == 32 || u == 10 || u == 9; if (sp(p) && !sp(c)) || p == 10 || (c == 10 && !sp(p)) { b.push(i) } } }
            else if unit == TextUnit_Line { for &(s, _) in &self.lines[1..] { b.push(s) } }
            else if unit == TextUnit_Paragraph { for i in 1..n { if self.text[i - 1] == 10 { b.push(i) } } }
            b.push(n); b.dedup(); b
        }
    }
    fn notify(value: bool) {
        if off("events") { return }
        let root = ELEM.with_borrow(|r| r.clone()); let Some(root) = root else { return };
        unsafe { if UiaClientsAreListening().as_bool() { if value { let _ = UiaRaiseAutomationEvent(&root, UIA_Text_TextChangedEventId); } let _ = UiaRaiseAutomationEvent(&root, UIA_Text_TextSelectionChangedEventId); } }
    }
    fn redraw() { let h = DOC.with_borrow(|d| d.hwnd); unsafe { let _ = InvalidateRect(Some(HWND(h as _)), None, false); } }

    // ───────────────────────────── the provider
    #[implement(IRawElementProviderSimple, ITextProvider, IValueProvider, IScrollProvider)]
    struct Root;
    #[implement(ITextRangeProvider)]
    struct Range { a: Cell<usize>, b: Cell<usize> }
    fn range(a: usize, b: usize) -> ITextRangeProvider { Range { a: Cell::new(a), b: Cell::new(b) }.into() }
    fn ends(r: &Ref<ITextRangeProvider>) -> Result<(usize, usize)> { let i = r.ok()?; let x: &Range = unsafe { i.as_impl() }; Ok((x.a.get(), x.b.get())) }
    fn unknowns(items: Vec<IUnknown>) -> *mut SAFEARRAY { unsafe { let sa = SafeArrayCreateVector(VT_UNKNOWN, 0, items.len() as u32); for (i, u) in items.iter().enumerate() { let _ = SafeArrayPutElement(sa, &(i as i32), u.as_raw()); } sa } }
    fn doubles(v: &[f64]) -> *mut SAFEARRAY { unsafe { let sa = SafeArrayCreateVector(VT_R8, 0, v.len() as u32); for (i, d) in v.iter().enumerate() { let _ = SafeArrayPutElement(sa, &(i as i32), d as *const f64 as *const _); } sa } }
    fn unit_name(u: TextUnit) -> &'static str { ["char", "format", "word", "line", "paragraph", "page", "document"].get(u.0 as usize).copied().unwrap_or("?") }

    fn child_tree() -> bool { std::env::var("S9_TREE").as_deref() == Ok("child") }
    // S9_TREE=child: the HWND's provider is a bare fragment root, the text a fragment under it
    #[implement(IRawElementProviderSimple, IRawElementProviderFragment, IRawElementProviderFragmentRoot)]
    struct Frame;
    #[implement(IRawElementProviderSimple, IRawElementProviderFragment)]
    struct Leaf { inner: IRawElementProviderSimple }
    fn elem_fragment() -> Result<IRawElementProviderFragment> { ELEM.with_borrow(|e| e.clone()).ok_or_else(Error::empty)?.cast() }
    impl IRawElementProviderSimple_Impl for Frame_Impl {
        fn ProviderOptions(&self) -> Result<ProviderOptions> { Ok(ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading) }
        fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> { log("frame.GetPatternProvider", format!("{}", id.0), false); Err(Error::empty()) }
        fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> { log("frame.GetPropertyValue", format!("{}", id.0), false); Ok(VARIANT::default()) }
        fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> { unsafe { UiaHostProviderFromHwnd(HWND(DOC.with_borrow(|d| d.hwnd) as _)) } }
    }
    impl IRawElementProviderFragment_Impl for Frame_Impl {
        fn Navigate(&self, dir: NavigateDirection) -> Result<IRawElementProviderFragment> { log("frame.Navigate", format!("{}", dir.0), false); if dir == NavigateDirection_FirstChild || dir == NavigateDirection_LastChild { elem_fragment() } else { Err(Error::empty()) } }
        fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> { Ok(std::ptr::null_mut()) }
        fn BoundingRectangle(&self) -> Result<UiaRect> { Ok(UiaRect::default()) }
        fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> { Ok(std::ptr::null_mut()) }
        fn SetFocus(&self) -> Result<()> { Ok(()) }
        fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> { Ok(self.to_interface()) }
    }
    impl IRawElementProviderFragmentRoot_Impl for Frame_Impl {
        fn ElementProviderFromPoint(&self, _x: f64, _y: f64) -> Result<IRawElementProviderFragment> { log("frame.ElementProviderFromPoint", String::new(), false); elem_fragment() }
        fn GetFocus(&self) -> Result<IRawElementProviderFragment> { log("frame.GetFocus", String::new(), false); elem_fragment() }
    }
    impl IRawElementProviderSimple_Impl for Leaf_Impl {
        fn ProviderOptions(&self) -> Result<ProviderOptions> { Ok(ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading) }
        fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> { unsafe { self.inner.GetPatternProvider(id) } }
        fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> { unsafe { self.inner.GetPropertyValue(id) } }
        fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> { Err(Error::empty()) }
    }
    impl IRawElementProviderFragment_Impl for Leaf_Impl {
        fn Navigate(&self, dir: NavigateDirection) -> Result<IRawElementProviderFragment> { log("leaf.Navigate", format!("{}", dir.0), false); if dir == NavigateDirection_Parent { ROOT.with_borrow(|r| r.clone()).ok_or_else(Error::empty)?.cast() } else { Err(Error::empty()) } }
        fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> { unsafe { let sa = SafeArrayCreateVector(VT_I4, 0, 2); for (i, v) in [UiaAppendRuntimeId as i32, 1].iter().enumerate() { let _ = SafeArrayPutElement(sa, &(i as i32), v as *const i32 as *const _); } Ok(sa) } }
        fn BoundingRectangle(&self) -> Result<UiaRect> { let (h, w, ht) = DOC.with_borrow(|d| (d.hwnd, d.w, d.h)); let mut p = POINT::default(); unsafe { let _ = ClientToScreen(HWND(h as _), &mut p); } log("leaf.BoundingRectangle", format!("({},{} {w}x{ht})", p.x, p.y), false); Ok(UiaRect { left: p.x as f64, top: p.y as f64, width: w as f64, height: ht as f64 }) }
        fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> { Ok(std::ptr::null_mut()) }
        fn SetFocus(&self) -> Result<()> { log("leaf.SetFocus", String::new(), true); Ok(()) }
        fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> { ROOT.with_borrow(|r| r.clone()).ok_or_else(Error::empty)?.cast() }
    }
    impl IRawElementProviderSimple_Impl for Root_Impl {
        fn ProviderOptions(&self) -> Result<ProviderOptions> { Ok(ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading) }
        fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> {
            let name = match id { UIA_TextPatternId => "Text", UIA_ValuePatternId => "Value", UIA_ScrollPatternId => "Scroll", UIA_TextPattern2Id => "Text2", UIA_TextEditPatternId => "TextEdit", UIA_LegacyIAccessiblePatternId => "LegacyIAccessible", _ => "" };
            let give = (id == UIA_TextPatternId && !off("text")) || (id == UIA_ValuePatternId && !off("value")) || (id == UIA_ScrollPatternId && !off("scroll"));
            log("GetPatternProvider", format!("{} {name} -> {}", id.0, if give { "given" } else { "none" }), false);
            if give { let me: IRawElementProviderSimple = self.to_interface(); me.cast() } else { Err(Error::empty()) }
        }
        fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
            let edit = std::env::var("S9_CT").as_deref() == Ok("edit");
            let (name, v): (&str, VARIANT) = match id {
                UIA_ControlTypePropertyId => ("ControlType", VARIANT::from(if edit { UIA_EditControlTypeId.0 } else { UIA_DocumentControlTypeId.0 })),
                UIA_NamePropertyId => ("Name", VARIANT::from("Editor")),
                UIA_AutomationIdPropertyId => ("AutomationId", if off("ids") { VARIANT::default() } else { VARIANT::from("editor") }),
                UIA_ClassNamePropertyId => ("ClassName", if off("ids") { VARIANT::default() } else { VARIANT::from("S9Text") }),
                UIA_FrameworkIdPropertyId => ("FrameworkId", VARIANT::from(if off("ids") { "AccessKit" } else { "Win32" })),
                UIA_IsKeyboardFocusablePropertyId => ("IsKeyboardFocusable", VARIANT::from(true)),
                UIA_HasKeyboardFocusPropertyId => ("HasKeyboardFocus", VARIANT::from(unsafe { GetFocus().0 as isize } == DOC.with_borrow(|d| d.hwnd))),
                UIA_IsEnabledPropertyId => ("IsEnabled", VARIANT::from(true)),
                UIA_IsControlElementPropertyId => ("IsControlElement", VARIANT::from(true)),
                UIA_IsContentElementPropertyId => ("IsContentElement", VARIANT::from(true)),
                UIA_IsPasswordPropertyId => ("IsPassword", VARIANT::from(false)),
                UIA_IsOffscreenPropertyId => ("IsOffscreen", VARIANT::from(false)),
                UIA_LocalizedControlTypePropertyId => ("LocalizedControlType", VARIANT::from(if edit { "edit" } else { "document" })),
                _ => ("", VARIANT::default()),
            };
            log("GetPropertyValue", format!("{} {name}", id.0), false);
            Ok(v)
        }
        fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> { unsafe { UiaHostProviderFromHwnd(HWND(DOC.with_borrow(|d| d.hwnd) as _)) } }
    }
    impl ITextProvider_Impl for Root_Impl {
        fn GetSelection(&self) -> Result<*mut SAFEARRAY> { let (a, b) = DOC.with_borrow(|d| d.sel()); log("text.GetSelection", format!("{{{a},{}}}", b - a), false); Ok(unknowns(vec![range(a, b).cast()?])) }
        fn GetVisibleRanges(&self) -> Result<*mut SAFEARRAY> {
            let (a, b) = DOC.with_borrow(|d| { let first = ((d.scroll - d.pad).max(0) / d.lh.max(1)) as usize; let last = (((d.scroll + d.h) / d.lh.max(1)) as usize).min(d.lines.len() - 1); (d.lines[first.min(last)].0, d.lines[last].1) });
            log("text.GetVisibleRanges", format!("{{{a},{}}}", b - a), false); if off("visible") { return Ok(unknowns(vec![])) } Ok(unknowns(vec![range(a, b).cast()?]))
        }
        fn RangeFromChild(&self, _c: Ref<IRawElementProviderSimple>) -> Result<ITextRangeProvider> { log("text.RangeFromChild", String::new(), true); Err(E_INVALIDARG.into()) }
        fn RangeFromPoint(&self, p: &UiaPoint) -> Result<ITextRangeProvider> {
            let o = DOC.with_borrow(|d| { let mut q = POINT { x: p.x as i32, y: p.y as i32 }; unsafe { let _ = ScreenToClient(HWND(d.hwnd as _), &mut q); } d.hit(q.x, q.y) });
            log("text.RangeFromPoint", format!("({},{}) -> {o}", p.x as i32, p.y as i32), false); Ok(range(o, o))
        }
        fn DocumentRange(&self) -> Result<ITextRangeProvider> { let n = DOC.with_borrow(|d| d.len()); log("text.DocumentRange", format!("{n} units"), false); Ok(range(0, n)) }
        fn SupportedTextSelection(&self) -> Result<SupportedTextSelection> { log("text.SupportedTextSelection", String::new(), false); Ok(SupportedTextSelection_Single) }
    }
    impl IValueProvider_Impl for Root_Impl {
        fn SetValue(&self, val: &PCWSTR) -> Result<()> { let s = unsafe { val.to_string().unwrap_or_default() }; log("value.SetValue", format!("{} chars: {s:?}", s.len()), true); let u: Vec<u16> = s.replace("\r\n", "\n").encode_utf16().collect(); DOC.with_borrow_mut(|d| { let n = d.len(); d.replace(0, n, &u, "value.SetValue") }); notify(true); redraw(); Ok(()) }
        fn Value(&self) -> Result<BSTR> { let s = DOC.with_borrow(|d| d.s(0, d.len())); log("value.Value", format!("{} units", s.encode_utf16().count()), false); Ok(BSTR::from(s)) }
        fn IsReadOnly(&self) -> Result<BOOL> { log("value.IsReadOnly", String::new(), false); Ok(false.into()) }
    }
    impl IScrollProvider_Impl for Root_Impl {
        fn Scroll(&self, _h: ScrollAmount, v: ScrollAmount) -> Result<()> { log("scroll.Scroll", format!("{}", v.0), true); Ok(()) }
        fn SetScrollPercent(&self, _h: f64, v: f64) -> Result<()> { log("scroll.SetScrollPercent", format!("{v}"), true); DOC.with_borrow_mut(|d| { let max = (d.lines.len() as i32 * d.lh + 2 * d.pad - d.h).max(0); d.scroll = (max as f64 * v / 100.0) as i32 }); redraw(); Ok(()) }
        fn HorizontalScrollPercent(&self) -> Result<f64> { Ok(-1.0) }
        fn VerticalScrollPercent(&self) -> Result<f64> { let p = DOC.with_borrow(|d| { let max = (d.lines.len() as i32 * d.lh + 2 * d.pad - d.h).max(0); if max == 0 { -1.0 } else { 100.0 * d.scroll as f64 / max as f64 } }); log("scroll.VerticalPercent", format!("{p:.1}"), false); Ok(p) }
        fn HorizontalViewSize(&self) -> Result<f64> { Ok(100.0) }
        fn VerticalViewSize(&self) -> Result<f64> { let p = DOC.with_borrow(|d| (100.0 * d.h as f64 / (d.lines.len() as i32 * d.lh + 2 * d.pad).max(1) as f64).min(100.0)); log("scroll.VerticalViewSize", format!("{p:.1}"), false); Ok(p) }
        fn HorizontallyScrollable(&self) -> Result<BOOL> { Ok(false.into()) }
        fn VerticallyScrollable(&self) -> Result<BOOL> { let y = DOC.with_borrow(|d| d.lines.len() as i32 * d.lh + 2 * d.pad > d.h); log("scroll.VerticallyScrollable", format!("{y}"), false); Ok(y.into()) }
    }
    impl Range_Impl {
        fn step(&self, from: usize, unit: TextUnit, count: i32) -> (usize, i32) {
            DOC.with_borrow(|d| { let b = d.bounds(unit); let mut p = from; let mut moved = 0;
                for _ in 0..count.unsigned_abs() { let next = if count > 0 { b.iter().copied().find(|&x| x > p) } else { b.iter().rev().copied().find(|&x| x < p) }; match next { Some(x) => { p = x; moved += count.signum() } None => break } }
                (p, moved) })
        }
    }
    impl ITextRangeProvider_Impl for Range_Impl {
        fn Clone(&self) -> Result<ITextRangeProvider> { log("range.Clone", String::new(), false); Ok(range(self.a.get(), self.b.get())) }
        fn Compare(&self, r: Ref<ITextRangeProvider>) -> Result<BOOL> { let e = ends(&r)?; log("range.Compare", String::new(), false); Ok((e == (self.a.get(), self.b.get())).into()) }
        fn CompareEndpoints(&self, ep: TextPatternRangeEndpoint, r: Ref<ITextRangeProvider>, tep: TextPatternRangeEndpoint) -> Result<i32> {
            let e = ends(&r)?; let mine = if ep == TextPatternRangeEndpoint_Start { self.a.get() } else { self.b.get() }; let theirs = if tep == TextPatternRangeEndpoint_Start { e.0 } else { e.1 };
            log("range.CompareEndpoints", String::new(), false); Ok(mine as i32 - theirs as i32)
        }
        fn ExpandToEnclosingUnit(&self, unit: TextUnit) -> Result<()> {
            let (a, b) = DOC.with_borrow(|d| { let bs = d.bounds(unit); let s = bs.iter().rev().copied().find(|&x| x <= self.a.get()).unwrap_or(0); let e = bs.iter().copied().find(|&x| x > s).unwrap_or(d.len()); (s, e) });
            log("range.ExpandToEnclosingUnit", format!("{} {{{},{}}} -> {{{a},{}}}", unit_name(unit), self.a.get(), self.b.get() - self.a.get(), b - a), false);
            self.a.set(a); self.b.set(b); Ok(())
        }
        fn FindAttribute(&self, id: UIA_TEXTATTRIBUTE_ID, _v: &VARIANT, _back: BOOL) -> Result<ITextRangeProvider> { log("range.FindAttribute", format!("{}", id.0), true); Err(Error::empty()) }
        fn FindText(&self, t: &BSTR, _back: BOOL, _ic: BOOL) -> Result<ITextRangeProvider> { log("range.FindText", format!("{t:?}"), true); Err(Error::empty()) }
        fn GetAttributeValue(&self, id: UIA_TEXTATTRIBUTE_ID) -> Result<VARIANT> {
            log("range.GetAttributeValue", format!("{}", id.0), false);
            Ok(match id { UIA_IsReadOnlyAttributeId => VARIANT::from(false), UIA_FontNameAttributeId => VARIANT::from("Consolas"), UIA_CaretPositionAttributeId => VARIANT::from(0i32), UIA_IsActiveAttributeId | UIA_SelectionActiveEndAttributeId => VARIANT::from(false),
                _ => unsafe { VARIANT::from(UiaGetReservedNotSupportedValue()?) } })
        }
        fn GetBoundingRectangles(&self) -> Result<*mut SAFEARRAY> {
            let (a, b) = (self.a.get(), self.b.get());
            let v: Vec<f64> = DOC.with_borrow(|d| { let mut out = vec![]; for (x, y, w, h) in d.rects(a, b) { if y + h <= 0 || y >= d.h { continue } let mut p = POINT { x, y }; unsafe { let _ = ClientToScreen(HWND(d.hwnd as _), &mut p); } out.extend([p.x as f64, p.y as f64, w as f64, h as f64]); } out });
            log("range.GetBoundingRectangles", format!("{{{a},{}}} -> {} rects{}", b - a, v.len() / 4, v.chunks(4).next().map(|r| format!(" ({},{} {}x{})", r[0], r[1], r[2], r[3])).unwrap_or_default()), false);
            Ok(doubles(&v))
        }
        fn GetEnclosingElement(&self) -> Result<IRawElementProviderSimple> { log("range.GetEnclosingElement", String::new(), false); ELEM.with_borrow(|r| r.clone()).ok_or_else(Error::empty) }
        fn GetText(&self, max: i32) -> Result<BSTR> {
            let (a, b) = (self.a.get(), self.b.get()); let s = DOC.with_borrow(|d| d.s(a, if max >= 0 { b.min(a + max as usize) } else { b }));
            log("range.GetText", format!("{{{a},{}}} max {max} {s:?}", b - a), false); Ok(BSTR::from(s))
        }
        fn Move(&self, unit: TextUnit, count: i32) -> Result<i32> {
            let degenerate = self.a.get() == self.b.get(); let (p, moved) = self.step(self.a.get(), unit, count);
            log("range.Move", format!("{} {count} from {{{},{}}} -> {p}", unit_name(unit), self.a.get(), self.b.get() - self.a.get()), false);
            self.a.set(p); self.b.set(p); if !degenerate { let (e, _) = self.step(p, unit, 1); self.b.set(e); } Ok(moved)
        }
        fn MoveEndpointByUnit(&self, ep: TextPatternRangeEndpoint, unit: TextUnit, count: i32) -> Result<i32> {
            let start = ep == TextPatternRangeEndpoint_Start; let (p, moved) = self.step(if start { self.a.get() } else { self.b.get() }, unit, count);
            log("range.MoveEndpointByUnit", format!("{} {} {count} -> {p}", if start { "start" } else { "end" }, unit_name(unit)), false);
            if start { self.a.set(p); if self.b.get() < p { self.b.set(p) } } else { self.b.set(p); if self.a.get() > p { self.a.set(p) } } Ok(moved)
        }
        fn MoveEndpointByRange(&self, ep: TextPatternRangeEndpoint, r: Ref<ITextRangeProvider>, tep: TextPatternRangeEndpoint) -> Result<()> {
            let e = ends(&r)?; let p = if tep == TextPatternRangeEndpoint_Start { e.0 } else { e.1 }; log("range.MoveEndpointByRange", String::new(), false);
            if ep == TextPatternRangeEndpoint_Start { self.a.set(p); if self.b.get() < p { self.b.set(p) } } else { self.b.set(p); if self.a.get() > p { self.a.set(p) } } Ok(())
        }
        fn Select(&self) -> Result<()> {
            let (a, b) = (self.a.get(), self.b.get()); let t = DOC.with_borrow_mut(|d| { d.anchor = a.min(d.len()); d.caret = b.min(d.len()); d.reveal(); d.s(a, b) });
            log("range.Select", format!("{{{a},{}}} {t:?}", b - a), true); notify(false); redraw(); Ok(())
        }
        fn AddToSelection(&self) -> Result<()> { log("range.AddToSelection", String::new(), true); Err(Error::empty()) }
        fn RemoveFromSelection(&self) -> Result<()> { log("range.RemoveFromSelection", String::new(), true); Err(Error::empty()) }
        fn ScrollIntoView(&self, top: BOOL) -> Result<()> { log("range.ScrollIntoView", format!("{{{},{}}} top {}", self.a.get(), self.b.get() - self.a.get(), top.as_bool()), true); Ok(()) }
        fn GetChildren(&self) -> Result<*mut SAFEARRAY> { log("range.GetChildren", String::new(), false); Ok(unknowns(vec![])) }
    }

    // ───────────────────────────── the window
    fn clipboard_text(hwnd: HWND) -> Option<Vec<u16>> { unsafe {
        OpenClipboard(Some(hwnd)).ok()?; let h = GetClipboardData(13 /* CF_UNICODETEXT */); let mut out = None;
        if let Ok(h) = h { let p = GlobalLock(HGLOBAL(h.0)) as *const u16; if !p.is_null() { let mut n = 0; while *p.add(n) != 0 { n += 1 } out = Some(std::slice::from_raw_parts(p, n).iter().copied().filter(|&c| c != 13).collect()); let _ = GlobalUnlock(HGLOBAL(h.0)); } }
        let _ = CloseClipboard(); out } }
    fn key_edit(vk: u16, shift: bool, ctrl: bool, hwnd: HWND) {
        let mut changed = false;
        DOC.with_borrow_mut(|d| {
            let (a, b) = d.sel(); let c = d.caret; let n = d.len();
            let mut go = |d: &mut Doc, to: usize| { d.caret = to.min(n); if !shift { d.anchor = d.caret } d.reveal() };
            match VIRTUAL_KEY(vk) {
                VK_LEFT => go(d, if a != b && !shift { a } else { c.saturating_sub(1) }),
                VK_RIGHT => go(d, if a != b && !shift { b } else { c + 1 }),
                VK_UP | VK_DOWN => { let i = d.line_of(c); let col = c - d.lines[i].0; let j = if VIRTUAL_KEY(vk) == VK_UP { i.saturating_sub(1) } else { (i + 1).min(d.lines.len() - 1) }; let (s, e) = d.lines[j]; go(d, (s + col).min(e)) }
                VK_HOME => { let s = if ctrl { 0 } else { d.lines[d.line_of(c)].0 }; go(d, s) }
                VK_END => { let e = if ctrl { n } else { d.lines[d.line_of(c)].1 }; go(d, e) }
                VK_DELETE => { if a != b { d.replace(a, b, &[], "key") } else if c < n { d.replace(c, c + 1, &[], "key") } changed = true }
                VK_A if ctrl => { d.anchor = 0; d.caret = n }
                VK_Z if ctrl => { if let Some((at, old, new)) = d.undo.pop() { log("UNDO", format!("{{{at},{}}} {:?} <- {:?}", new.len(), String::from_utf16_lossy(&old), String::from_utf16_lossy(&new)), true); d.text.splice(at..at + new.len(), old.iter().copied()); d.caret = at + old.len(); d.anchor = d.caret; d.relayout(); d.reveal(); changed = true } }
                VK_V if ctrl => { if let Some(t) = clipboard_text(hwnd) { log("PASTE", format!("{:?}", String::from_utf16_lossy(&t)), true); d.replace(a, b, &t, "paste"); changed = true } }
                _ => {}
            }
        });
        notify(changed); redraw();
    }
    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT { unsafe {
        match msg {
            WM_GETOBJECT => {
                log("WM_GETOBJECT", format!("objid {}", lp.0 as i32), false);
                if lp.0 as i32 == UiaRootObjectId { let root = ROOT.with_borrow(|r| r.clone()); if let Some(root) = root { return UiaReturnRawElementProvider(hwnd, wp, lp, &root) } }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_SIZE => { DOC.with_borrow_mut(|d| { d.w = (lp.0 & 0xffff) as i32; d.h = ((lp.0 >> 16) & 0xffff) as i32; if d.cw > 0 { d.relayout() } }); log("RESIZE", String::new(), true); LRESULT(0) }
            WM_SETFOCUS | WM_KILLFOCUS => { log(if msg == WM_SETFOCUS { "FOCUS" } else { "BLUR" }, String::new(), true);
                if msg == WM_SETFOCUS && child_tree() { let e = ELEM.with_borrow(|e| e.clone()); if let Some(e) = e { let _ = UiaRaiseAutomationEvent(&e, UIA_AutomationFocusChangedEventId); } }
                redraw(); LRESULT(0) }
            WM_MOUSEWHEEL => { let dz = ((wp.0 >> 16) & 0xffff) as i16 as i32;
                let (s, old, new) = DOC.with_borrow_mut(|d| { let max = (d.lines.len() as i32 * d.lh + 2 * d.pad - d.h).max(0); let old = d.scroll; d.scroll = (d.scroll - dz * d.lh * 3 / 120).clamp(0, max); (d.scroll, 100.0 * old as f64 / max.max(1) as f64, 100.0 * d.scroll as f64 / max.max(1) as f64) });
                log("SCROLL", format!("{s}"), false);
                // what a native scrolling control does: the Scroll pattern's percent changes
                if !off("scrollevent") && !off("scroll") { let e = ELEM.with_borrow(|e| e.clone()); if let Some(e) = e { if UiaClientsAreListening().as_bool() { let _ = UiaRaiseAutomationPropertyChangedEvent(&e, UIA_ScrollVerticalScrollPercentPropertyId, &VARIANT::from(old), &VARIANT::from(new)); } } }
                redraw(); LRESULT(0) }
            WM_LBUTTONDOWN => { let _ = SetFocus(Some(hwnd)); let (x, y) = ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32); let shift = wp.0 & 4 != 0; let o = DOC.with_borrow_mut(|d| { let o = d.hit(x, y); d.caret = o; if !shift { d.anchor = o } o }); log("CLICK", format!("{o}"), true); notify(false); redraw(); LRESULT(0) }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let (shift, ctrl) = (GetKeyState(VK_SHIFT.0 as i32) < 0, GetKeyState(VK_CONTROL.0 as i32) < 0);
                log("WM_KEYDOWN", format!("vk {:#04x} scan {} flags {:#x} shift {shift} ctrl {ctrl} extra {:#x}", wp.0, (lp.0 >> 16) & 0xff, (lp.0 >> 24) & 0xff, GetMessageExtraInfo().0), true);
                key_edit(wp.0 as u16, shift, ctrl, hwnd); if msg == WM_SYSKEYDOWN { DefWindowProcW(hwnd, msg, wp, lp) } else { LRESULT(0) }
            }
            WM_KEYUP => { log("WM_KEYUP", format!("vk {:#04x}", wp.0), false); LRESULT(0) }
            WM_CHAR | WM_UNICHAR | WM_IME_CHAR => {
                let c = wp.0 as u32;
                log(if msg == WM_CHAR { "WM_CHAR" } else if msg == WM_UNICHAR { "WM_UNICHAR" } else { "WM_IME_CHAR" }, format!("{c:#06x} {:?} extra {:#x}", char::from_u32(c).unwrap_or('?'), GetMessageExtraInfo().0), true);
                if msg == WM_UNICHAR { return LRESULT(0) }
                let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0 && GetKeyState(VK_MENU.0 as i32) >= 0;
                DOC.with_borrow_mut(|d| { let (a, b) = d.sel();
                    if c == 8 { if a != b { d.replace(a, b, &[], "key") } else if a > 0 { d.replace(a - 1, a, &[], "key") } }
                    else if c == 13 { d.replace(a, b, &[10], "key") }
                    else if c >= 32 && !ctrl { d.replace(a, b, &[c as u16], "char") } });
                notify(true); redraw(); LRESULT(0)
            }
            WM_PASTE => { log("WM_PASTE", String::new(), true); key_edit(VK_V.0, false, true, hwnd); LRESULT(0) }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default(); let hdc = BeginPaint(hwnd, &mut ps);
                DOC.with_borrow(|d| {
                    let mem = CreateCompatibleDC(Some(hdc)); let bmp = CreateCompatibleBitmap(hdc, d.w.max(1), d.h.max(1)); let old = SelectObject(mem, bmp.into());
                    let bg = CreateSolidBrush(COLORREF(0x001c1c1c)); FillRect(mem, &RECT { left: 0, top: 0, right: d.w, bottom: d.h }, bg); let _ = DeleteObject(bg.into());
                    let (a, b) = d.sel();
                    if a != b { let hb = CreateSolidBrush(COLORREF(0x00bf5229)); for (x, y, w, h) in d.rects(a, b) { FillRect(mem, &RECT { left: x, top: y, right: x + w.max(d.cw / 2), bottom: y + h }, hb); } let _ = DeleteObject(hb.into()); }
                    SelectObject(mem, HGDIOBJ(d.font as _)); SetBkMode(mem, TRANSPARENT); SetTextColor(mem, COLORREF(0x00ebebeb));
                    for (i, &(s, e)) in d.lines.iter().enumerate() { let y = d.pad + i as i32 * d.lh - d.scroll; if y + d.lh < 0 || y > d.h { continue } let _ = TextOutW(mem, d.pad, y, &d.text[s..e]); }
                    if a == b && GetFocus() == hwnd { if let Some(&(x, y, _, h)) = d.rects(a, a).first() { let wb = CreateSolidBrush(COLORREF(0x00ffffff)); FillRect(mem, &RECT { left: x, top: y, right: x + 2, bottom: y + h }, wb); let _ = DeleteObject(wb.into()); } }
                    let _ = BitBlt(hdc, 0, 0, d.w, d.h, Some(mem), 0, 0, SRCCOPY); SelectObject(mem, old); let _ = DeleteObject(bmp.into()); let _ = DeleteDC(mem);
                });
                let _ = EndPaint(hwnd, &ps); LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER => { let _ = DestroyWindow(hwnd); LRESULT(0) }
            WM_DESTROY => { let t = DOC.with_borrow(|d| d.s(0, d.len())); log("QUIT", format!("final text: {t}"), true); LOG.with_borrow_mut(|l| if let Some(l) = l.as_mut() { let _ = writeln!(l.out, "{t}"); }); summary(); ROOT.with_borrow_mut(|r| { if let Some(r) = r.take() { let _ = UiaReturnRawElementProvider(hwnd, WPARAM(0), LPARAM(0), None::<&IRawElementProviderSimple>); let _ = UiaDisconnectProvider(&r); } }); PostQuitMessage(0); LRESULT(0) }
            // anything a tool might send an edit control: the EM_ range, WM_SETTEXT / GETTEXT, cut / copy / clear / undo, IME
            0x00B0..=0x00DF | WM_SETTEXT | WM_GETTEXT | WM_GETTEXTLENGTH | WM_CUT | WM_COPY | WM_CLEAR | WM_UNDO | WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION | WM_INPUT => { log("MSG", format!("{msg:#06x} wp {:#x} lp {:#x}", wp.0, lp.0), false); DefWindowProcW(hwnd, msg, wp, lp) }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    } }

    pub fn run() { unsafe {
        let args: Vec<String> = std::env::args().collect();
        let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
        let path = std::env::var("S9_LOG").unwrap_or_else(|_| "s9-uia.log".into());
        LOG.set(Some(Log { out: std::fs::File::create(&path).expect("S9_LOG"), t0: Instant::now(), counts: HashMap::new(), last: HashMap::new() }));
        let text = arg("--file").map(|f| std::fs::read_to_string(f).expect("--file")).unwrap_or_else(|| SAMPLE.to_string()).replace("\r\n", "\n");
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let inst = GetModuleHandleW(None).unwrap();
        let class = w!("S9Text");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: class, hCursor: LoadCursorW(None, IDC_IBEAM).unwrap_or_default(), ..Default::default() });
        DOC.with_borrow_mut(|d| { d.text = text.encode_utf16().collect(); d.cw = 0; d.lines = vec![(0, 0)]; });
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("S9 - desktop text (Windows reference window)"), WS_OVERLAPPEDWINDOW, 200, 120, 1100, 760, None, None, Some(inst.into()), None).unwrap();
        let dpi = GetDpiForWindow(hwnd) as i32;
        let font = CreateFontW(-(13 * dpi / 72), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, FIXED_PITCH.0 as u32, w!("Consolas"));
        let hdc = GetDC(Some(hwnd)); let old = SelectObject(hdc, font.into()); let mut tm = TEXTMETRICW::default(); let _ = GetTextMetricsW(hdc, &mut tm); SelectObject(hdc, old); ReleaseDC(Some(hwnd), hdc);
        let mut rc = RECT::default(); let _ = GetClientRect(hwnd, &mut rc);
        DOC.with_borrow_mut(|d| { d.hwnd = hwnd.0 as isize; d.font = font.0 as isize; d.cw = tm.tmAveCharWidth; d.lh = tm.tmHeight + tm.tmExternalLeading + 4; d.pad = 24 * dpi / 96; d.w = rc.right; d.h = rc.bottom; d.relayout(); });
        let text_el: IRawElementProviderSimple = Root.into();
        if child_tree() { ELEM.set(Some(Leaf { inner: text_el }.into())); ROOT.set(Some(Frame.into())); } else { ELEM.set(Some(text_el.clone())); ROOT.set(Some(text_el)); }
        let (n, cw, lh) = DOC.with_borrow(|d| (d.len(), d.cw, d.lh));
        log("START", format!("{n} UTF-16 units; dpi {dpi}; cell {cw}x{lh}; control type {}; tree {}; withheld {:?}", if std::env::var("S9_CT").as_deref() == Ok("edit") { "Edit" } else { "Document" }, if child_tree() { "child" } else { "window" }, std::env::var("S9_OFF").unwrap_or_default()), true);
        if let Some(s) = arg("--seconds").and_then(|v| v.parse::<u32>().ok()) { SetTimer(Some(hwnd), 1, s * 1000, None); }
        let _ = ShowWindow(hwnd, SW_SHOW); let _ = SetForegroundWindow(hwnd); let _ = SetFocus(Some(hwnd));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() { let _ = TranslateMessage(&msg); DispatchMessageW(&msg); }
    } }
}
