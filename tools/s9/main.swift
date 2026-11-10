// S9 step 1 — the native reference window (docs/spikes/S9-desktop-text.md). A custom NSView, NOT an NSTextView: TextKit
// lays the text out and answers geometry; the view draws, edits, and answers the accessibility text protocol and
// NSTextInputClient itself, logging every call. Run a writing tool against it and read the log.
//   S9_LOG=<file>     where the log goes (default ./s9-ax.log)
//   S9_OFF=a,b,c      answers to withhold (names as logged: value, setValue, selectedText, setSelectedText,
//                     selectedTextRange, setSelectedTextRange, visibleCharacterRange, numberOfCharacters, stringForRange,
//                     attributedStringForRange, frameForRange, rangeForLine, lineForIndex, rangeForPosition,
//                     rangeForIndex, styleRangeForIndex, insertionPointLineNumber, notifications)
//   S9_WORDS=N        a generated document of about N words instead of the sample;  S9_FILE=<path> a text file instead
import Cocoa

// ───────────────────────────── the log
final class Log {
    static let t0 = Date()
    static var counts: [String: Int] = [:]
    static var last: [String: String] = [:]
    static let fh: FileHandle = {
        let p = ProcessInfo.processInfo.environment["S9_LOG"] ?? "s9-ax.log"
        FileManager.default.createFile(atPath: p, contents: nil)
        return FileHandle(forWritingAtPath: p)!
    }()
    static func clip(_ s: String) -> String { let t = s.replacingOccurrences(of: "\n", with: "⏎"); return t.count > 70 ? String(t.prefix(67)) + "…" : t }
    /// One line per call; a call repeated with the same arguments and answer is counted, not re-printed.
    static func w(_ name: String, _ detail: String = "", always: Bool = false) {
        counts[name, default: 0] += 1
        if !always && last[name] == detail { return }
        last[name] = detail
        let line = String(format: "%9.3f  %-28@ %@\n", Date().timeIntervalSince(t0), name as NSString, detail as NSString)
        fh.write(line.data(using: .utf8)!)
    }
    static func summary() {
        var s = "\n── call counts ──\n"
        for (k, v) in counts.sorted(by: { $0.value > $1.value }) { s += String(format: "%8d  %@\n", v, k as NSString) }
        fh.write(s.data(using: .utf8)!)
    }
}
let off: Set<String> = Set((ProcessInfo.processInfo.environment["S9_OFF"] ?? "").split(separator: ",").map(String.init))
func R(_ r: NSRange) -> String { r.location == NSNotFound ? "{NotFound,\(r.length)}" : "{\(r.location),\(r.length)}" }

// ───────────────────────────── the editor
final class EditorView: NSView, NSTextInputClient {
    let storage = NSTextStorage()
    let layout = NSLayoutManager()
    let container = NSTextContainer(size: NSSize(width: 100, height: CGFloat.greatestFiniteMagnitude))
    var sel = NSRange(location: 0, length: 0)
    var marked = NSRange(location: NSNotFound, length: 0)
    var anchor = 0
    let inset: CGFloat = 28
    let attrs: [NSAttributedString.Key: Any] = {
        let p = NSMutableParagraphStyle(); p.lineSpacing = 4; p.paragraphSpacing = 12
        return [.font: NSFont.systemFont(ofSize: 16), .foregroundColor: NSColor(white: 0.92, alpha: 1), .paragraphStyle: p]
    }()
    var undo: [(NSRange, String, NSRange)] = []   // (the range now holding the new text, the old text, the old selection)

    override init(frame: NSRect) {
        super.init(frame: frame)
        storage.addLayoutManager(layout); layout.addTextContainer(container); container.lineFragmentPadding = 0
    }
    required init?(coder: NSCoder) { fatalError() }
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func becomeFirstResponder() -> Bool { Log.w("becomeFirstResponder", always: true); needsDisplay = true; note(.focusedUIElementChanged); return true }
    override func resignFirstResponder() -> Bool { Log.w("resignFirstResponder", always: true); needsDisplay = true; return true }
    var text: NSString { storage.string as NSString }
    var length: Int { storage.length }

    func setText(_ s: String) { storage.setAttributedString(NSAttributedString(string: s, attributes: attrs)); sel = NSRange(location: 0, length: 0); relayout() }
    func relayout() {
        let w = max(200, (enclosingScrollView?.contentSize.width ?? bounds.width))
        container.size = NSSize(width: w - 2 * inset, height: .greatestFiniteMagnitude)
        layout.ensureLayout(for: container)
        let h = layout.usedRect(for: container).height + 2 * inset
        setFrameSize(NSSize(width: w, height: max(h, enclosingScrollView?.contentSize.height ?? h)))
        lines = nil; needsDisplay = true
    }
    func note(_ n: NSAccessibility.Notification) { if !off.contains("notifications") { NSAccessibility.post(element: self, notification: n) } }

    /// Every edit goes through here.
    func replace(_ range: NSRange, with s: String, why: String) {
        let r = NSIntersectionRange(range, NSRange(location: 0, length: length)).length == range.length ? range : NSRange(location: min(range.location, length), length: 0)
        let old = text.substring(with: r)
        Log.w("EDIT", "\(why): \(R(r)) \"\(Log.clip(old))\" → \"\(Log.clip(s))\"", always: true)
        undo.append((NSRange(location: r.location, length: (s as NSString).length), old, sel))
        storage.replaceCharacters(in: r, with: NSAttributedString(string: s, attributes: attrs))
        sel = NSRange(location: r.location + (s as NSString).length, length: 0); anchor = sel.location
        relayout(); scrollCaret()
        note(.valueChanged); note(.selectedTextChanged)
    }
    func select(_ r: NSRange, why: String) {
        let c = NSRange(location: min(r.location, length), length: min(r.length, length - min(r.location, length)))
        if why != "" { Log.w("SELECT", "\(why): \(R(c))", always: true) }
        sel = c; needsDisplay = true; scrollCaret(); note(.selectedTextChanged)
    }
    func scrollCaret() { scrollToVisible(caretRect(sel.location + sel.length).insetBy(dx: 0, dy: -inset)) }
    func rect(for r: NSRange) -> NSRect {   // view coordinates
        if r.length == 0 { return caretRect(r.location) }
        let g = layout.glyphRange(forCharacterRange: r, actualCharacterRange: nil)
        return layout.boundingRect(forGlyphRange: g, in: container).offsetBy(dx: inset, dy: inset)
    }
    func caretRect(_ i: Int) -> NSRect {
        if length == 0 { return NSRect(x: inset, y: inset, width: 1, height: 22) }
        if i >= length {
            let ex = layout.extraLineFragmentRect
            if ex.height > 0 { return NSRect(x: inset + ex.minX, y: inset + ex.minY, width: 1, height: ex.height) }
            let g = layout.glyphIndexForCharacter(at: length - 1)
            let f = layout.lineFragmentRect(forGlyphAt: g, effectiveRange: nil); let b = layout.boundingRect(forGlyphRange: NSRange(location: g, length: 1), in: container)
            return NSRect(x: inset + b.maxX, y: inset + f.minY, width: 1, height: f.height)
        }
        let g = layout.glyphIndexForCharacter(at: i)
        let f = layout.lineFragmentRect(forGlyphAt: g, effectiveRange: nil); let p = layout.location(forGlyphAt: g)
        return NSRect(x: inset + f.minX + p.x, y: inset + f.minY, width: 1, height: f.height)
    }
    func index(at p: NSPoint) -> Int {   // view coordinates
        if length == 0 { return 0 }
        var frac: CGFloat = 0
        let g = layout.glyphIndex(for: NSPoint(x: p.x - inset, y: p.y - inset), in: container, fractionOfDistanceThroughGlyph: &frac)
        let c = layout.characterIndexForGlyph(at: g)
        return min(length, frac > 0.5 ? c + 1 : c)
    }
    /// The character range of every laid-out line (plus the empty last line after a trailing newline).
    var lines: [NSRange]? = nil
    func lineRanges() -> [NSRange] {
        if let l = lines { return l }
        var out: [NSRange] = []; var g = 0; let n = layout.numberOfGlyphs
        while g < n { var eff = NSRange(); _ = layout.lineFragmentRect(forGlyphAt: g, effectiveRange: &eff); out.append(layout.characterRange(forGlyphRange: eff, actualGlyphRange: nil)); g = NSMaxRange(eff) }
        if length == 0 || text.character(at: length - 1) == 10 { out.append(NSRange(location: length, length: 0)) }
        lines = out; return out
    }
    func line(of i: Int) -> Int { let l = lineRanges(); for (k, r) in l.enumerated() where i < NSMaxRange(r) { return k }; return max(0, l.count - 1) }
    func screen(_ r: NSRect) -> NSRect { guard let w = window else { return .zero }; return w.convertToScreen(convert(r, to: nil)) }

    // ── drawing
    override func draw(_ dirty: NSRect) {
        NSColor(white: 0.11, alpha: 1).setFill(); bounds.fill()
        let all = layout.glyphRange(for: container)
        if sel.length > 0 {
            NSColor(calibratedRed: 0.16, green: 0.32, blue: 0.75, alpha: 1).setFill()
            let g = layout.glyphRange(forCharacterRange: sel, actualCharacterRange: nil)
            layout.enumerateEnclosingRects(forGlyphRange: g, withinSelectedGlyphRange: g, in: container) { r, _ in r.offsetBy(dx: self.inset, dy: self.inset).fill() }
        }
        layout.drawGlyphs(forGlyphRange: all, at: NSPoint(x: inset, y: inset))
        if marked.location != NSNotFound, marked.length > 0 { NSColor.systemYellow.setFill(); var u = rect(for: marked); u.origin.y = u.maxY - 2; u.size.height = 2; u.fill() }
        if window?.firstResponder === self, sel.length == 0 { NSColor.white.setFill(); var c = caretRect(sel.location); c.size.width = 1.5; c.fill() }
    }

    // ── mouse
    override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        let i = index(at: convert(e.locationInWindow, from: nil))
        if e.modifierFlags.contains(.shift) { select(NSRange(location: min(anchor, i), length: abs(i - anchor)), why: "mouse") } else { anchor = i; select(NSRange(location: i, length: 0), why: "mouse") }
    }
    override func mouseDragged(with e: NSEvent) { let i = index(at: convert(e.locationInWindow, from: nil)); select(NSRange(location: min(anchor, i), length: abs(i - anchor)), why: "") }

    // ── keys → the input context → insertText / doCommand
    override func keyDown(with e: NSEvent) {
        Log.w("keyDown", "code \(e.keyCode) chars \"\(Log.clip(e.characters ?? ""))\" mods \(e.modifierFlags.intersection(.deviceIndependentFlagsMask).rawValue)\(e.isARepeat ? " repeat" : "")", always: true)
        interpretKeyEvents([e])
    }
    override func doCommand(by s: Selector) {
        let n = NSStringFromSelector(s); Log.w("doCommand", n, always: true)
        let caret = sel.location + sel.length
        func move(_ to: Int, extend: Bool) { let t = max(0, min(length, to)); if extend { select(NSRange(location: min(anchor, t), length: abs(t - anchor)), why: "") } else { anchor = t; select(NSRange(location: t, length: 0), why: "") } }
        func vertical(_ d: Int) -> Int { let c = caretRect(caret); return index(at: NSPoint(x: c.minX, y: c.midY + CGFloat(d) * c.height)) }
        func word(_ fwd: Bool) -> Int { fwd ? storage.nextWord(from: caret, forward: true) : storage.nextWord(from: caret, forward: false) }
        switch n {
        case "deleteBackward:": if sel.length > 0 { replace(sel, with: "", why: "key") } else if sel.location > 0 { replace(text.rangeOfComposedCharacterSequence(at: sel.location - 1), with: "", why: "key") }
        case "deleteForward:": if sel.length > 0 { replace(sel, with: "", why: "key") } else if sel.location < length { replace(text.rangeOfComposedCharacterSequence(at: sel.location), with: "", why: "key") }
        case "deleteWordBackward:": let w = word(false); replace(NSRange(location: w, length: caret - w), with: "", why: "key")
        case "insertNewline:": replace(sel, with: "\n", why: "key")
        case "insertTab:": replace(sel, with: "\t", why: "key")
        case "moveLeft:": move(sel.length > 0 ? sel.location : caret - 1, extend: false)
        case "moveRight:": move(sel.length > 0 ? NSMaxRange(sel) : caret + 1, extend: false)
        case "moveLeftAndModifySelection:": move((caret == anchor ? sel.location : caret) - 1, extend: true)
        case "moveRightAndModifySelection:": move((sel.location == anchor ? NSMaxRange(sel) : sel.location) + 1, extend: true)
        case "moveUp:": move(vertical(-1), extend: false)
        case "moveDown:": move(vertical(1), extend: false)
        case "moveWordLeft:": move(word(false), extend: false)
        case "moveWordRight:": move(word(true), extend: false)
        case "moveToBeginningOfLine:", "moveToLeftEndOfLine:": move(lineRanges()[line(of: caret)].location, extend: false)
        case "moveToEndOfLine:", "moveToRightEndOfLine:": let r = lineRanges()[line(of: caret)]; move(NSMaxRange(r) > r.location && NSMaxRange(r) <= length && r.length > 0 && text.character(at: NSMaxRange(r) - 1) == 10 ? NSMaxRange(r) - 1 : NSMaxRange(r), extend: false)
        case "moveToBeginningOfDocument:": move(0, extend: false)
        case "moveToEndOfDocument:": move(length, extend: false)
        default: break
        }
    }
    // the Edit menu's actions (so a tool that pastes is seen as a paste)
    @objc func paste(_ sender: Any?) { let s = NSPasteboard.general.string(forType: .string) ?? ""; Log.w("paste:", "\"\(Log.clip(s))\" types \(NSPasteboard.general.types?.map { $0.rawValue } ?? [])", always: true); replace(sel, with: s, why: "paste") }
    @objc func copy(_ sender: Any?) { Log.w("copy:", always: true); NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text.substring(with: sel), forType: .string) }
    @objc func cut(_ sender: Any?) { copy(sender); replace(sel, with: "", why: "cut") }
    override func selectAll(_ sender: Any?) { anchor = 0; select(NSRange(location: 0, length: length), why: "selectAll") }
    @objc func undo(_ sender: Any?) {
        guard let (r, old, s) = undo.popLast() else { return }
        Log.w("undo:", R(r), always: true)
        storage.replaceCharacters(in: r, with: NSAttributedString(string: old, attributes: attrs)); sel = s; relayout(); note(.valueChanged); note(.selectedTextChanged)
    }

    // ── NSTextInputClient
    func insertText(_ string: Any, replacementRange: NSRange) {
        let s = (string as? String) ?? (string as? NSAttributedString)?.string ?? ""
        Log.w("insertText", "\"\(Log.clip(s))\" replacementRange \(R(replacementRange))", always: true)
        let r = replacementRange.location != NSNotFound ? replacementRange : (marked.location != NSNotFound ? marked : sel)
        marked = NSRange(location: NSNotFound, length: 0)
        replace(r, with: s, why: "insertText")
    }
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let s = (string as? String) ?? (string as? NSAttributedString)?.string ?? ""
        Log.w("setMarkedText", "\"\(Log.clip(s))\" selected \(R(selectedRange)) replacement \(R(replacementRange))", always: true)
        let r = replacementRange.location != NSNotFound ? replacementRange : (marked.location != NSNotFound ? marked : sel)
        storage.replaceCharacters(in: r, with: NSAttributedString(string: s, attributes: attrs))
        marked = (s as NSString).length > 0 ? NSRange(location: r.location, length: (s as NSString).length) : NSRange(location: NSNotFound, length: 0)
        sel = NSRange(location: r.location + selectedRange.location, length: selectedRange.length); relayout(); note(.valueChanged)
    }
    func unmarkText() { Log.w("unmarkText", always: true); marked = NSRange(location: NSNotFound, length: 0); needsDisplay = true }
    func selectedRange() -> NSRange { Log.w("tic.selectedRange", R(sel)); return sel }
    func markedRange() -> NSRange { Log.w("tic.markedRange", R(marked)); return marked }
    func hasMarkedText() -> Bool { marked.location != NSNotFound }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        let r = NSIntersectionRange(range, NSRange(location: 0, length: length)); actualRange?.pointee = r
        Log.w("tic.attributedSubstring", R(range)); return storage.attributedSubstring(from: r)
    }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect { Log.w("tic.firstRect", R(range)); actualRange?.pointee = range; return screen(rect(for: range)) }
    func characterIndex(for point: NSPoint) -> Int { guard let w = window else { return 0 }; let i = index(at: convert(w.convertPoint(fromScreen: point), from: nil)); Log.w("tic.characterIndex", "\(i)"); return i }

    // ── the accessibility text protocol
    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .textArea }
    override func accessibilityLabel() -> String? { "Editor" }
    override func isAccessibilityFocused() -> Bool { let f = window?.firstResponder === self; Log.w("ax.isFocused", "\(f)"); return f }
    override func isAccessibilityEnabled() -> Bool { true }
    override func accessibilityValue() -> Any? { if off.contains("value") { Log.w("ax.value", "WITHHELD"); return nil }; Log.w("ax.value", "\(length) chars"); return storage.string }
    override func setAccessibilityValue(_ v: Any?) {
        let s = (v as? String) ?? (v as? NSAttributedString)?.string ?? ""
        Log.w("ax.setValue", "\((s as NSString).length) chars: \"\(Log.clip(s))\"", always: true)
        if off.contains("setValue") { return }
        replace(NSRange(location: 0, length: length), with: s, why: "ax.setValue")
    }
    override func accessibilityNumberOfCharacters() -> Int { if off.contains("numberOfCharacters") { return 0 }; Log.w("ax.numberOfCharacters", "\(length)"); return length }
    override func accessibilitySelectedText() -> String? { if off.contains("selectedText") { return nil }; let s = text.substring(with: sel); Log.w("ax.selectedText", "\"\(Log.clip(s))\""); return s }
    override func setAccessibilitySelectedText(_ s: String?) {
        Log.w("ax.setSelectedText", "\"\(Log.clip(s ?? "nil"))\" over \(R(sel))", always: true)
        if off.contains("setSelectedText") { return }
        replace(sel, with: s ?? "", why: "ax.setSelectedText")
    }
    override func accessibilitySelectedTextRange() -> NSRange { if off.contains("selectedTextRange") { return NSRange(location: NSNotFound, length: 0) }; Log.w("ax.selectedTextRange", R(sel)); return sel }
    override func setAccessibilitySelectedTextRange(_ r: NSRange) {
        Log.w("ax.setSelectedTextRange", R(r), always: true)
        if off.contains("setSelectedTextRange") { return }
        anchor = r.location; select(r, why: "ax")
    }
    override func accessibilitySelectedTextRanges() -> [NSValue]? { Log.w("ax.selectedTextRanges", R(sel)); return [NSValue(range: sel)] }
    override func setAccessibilitySelectedTextRanges(_ v: [NSValue]?) { Log.w("ax.setSelectedTextRanges", "\(v?.map { R($0.rangeValue) } ?? [])", always: true); if let r = v?.first?.rangeValue { anchor = r.location; select(r, why: "ax") } }
    override func accessibilityVisibleCharacterRange() -> NSRange {
        if off.contains("visibleCharacterRange") { return NSRange(location: 0, length: length) }
        let vis = visibleRect.offsetBy(dx: -inset, dy: -inset)
        let g = layout.glyphRange(forBoundingRect: vis, in: container); let r = layout.characterRange(forGlyphRange: g, actualGlyphRange: nil)
        Log.w("ax.visibleCharacterRange", R(r)); return r
    }
    override func accessibilityInsertionPointLineNumber() -> Int { if off.contains("insertionPointLineNumber") { return 0 }; let l = line(of: sel.location); Log.w("ax.insertionPointLineNumber", "\(l)"); return l }
    override func accessibilityString(for range: NSRange) -> String? {
        if off.contains("stringForRange") { Log.w("ax.stringForRange", "WITHHELD"); return nil }
        let r = NSIntersectionRange(range, NSRange(location: 0, length: length)); let s = text.substring(with: r)
        Log.w("ax.stringForRange", "\(R(range)) \"\(Log.clip(s))\""); return s
    }
    override func accessibilityAttributedString(for range: NSRange) -> NSAttributedString? {
        if off.contains("attributedStringForRange") { Log.w("ax.attributedStringForRange", "WITHHELD"); return nil }
        let r = NSIntersectionRange(range, NSRange(location: 0, length: length))
        Log.w("ax.attributedStringForRange", R(range)); return storage.attributedSubstring(from: r)
    }
    override func accessibilityFrame(for range: NSRange) -> NSRect {
        if off.contains("frameForRange") { Log.w("ax.frameForRange", "WITHHELD"); return .zero }
        let r = NSIntersectionRange(range, NSRange(location: 0, length: length))
        let f = screen(rect(for: r.length == 0 ? NSRange(location: min(range.location, length), length: 0) : r))
        Log.w("ax.frameForRange", "\(R(range)) → (\(Int(f.minX)),\(Int(f.minY)) \(Int(f.width))×\(Int(f.height)))"); return f
    }
    override func accessibilityRange(forLine line: Int) -> NSRange {
        if off.contains("rangeForLine") { return NSRange(location: NSNotFound, length: 0) }
        let l = lineRanges(); let r = line >= 0 && line < l.count ? l[line] : NSRange(location: NSNotFound, length: 0)
        Log.w("ax.rangeForLine", "\(line) → \(R(r))"); return r
    }
    override func accessibilityLine(for index: Int) -> Int { if off.contains("lineForIndex") { return 0 }; let l = line(of: index); Log.w("ax.lineForIndex", "\(index) → \(l)"); return l }
    override func accessibilityRange(for point: NSPoint) -> NSRange {
        if off.contains("rangeForPosition") { return NSRange(location: NSNotFound, length: 0) }
        guard let w = window else { return NSRange(location: NSNotFound, length: 0) }
        let i = min(max(0, length - 1), index(at: convert(w.convertPoint(fromScreen: point), from: nil)))
        let r = length == 0 ? NSRange(location: 0, length: 0) : text.rangeOfComposedCharacterSequence(at: i)
        Log.w("ax.rangeForPosition", "(\(Int(point.x)),\(Int(point.y))) → \(R(r))"); return r
    }
    override func accessibilityRange(for index: Int) -> NSRange {
        if off.contains("rangeForIndex") { return NSRange(location: NSNotFound, length: 0) }
        let r = index < length ? text.rangeOfComposedCharacterSequence(at: index) : NSRange(location: length, length: 0)
        Log.w("ax.rangeForIndex", "\(index) → \(R(r))"); return r
    }
    override func accessibilityStyleRange(for index: Int) -> NSRange { if off.contains("styleRangeForIndex") { return NSRange(location: NSNotFound, length: 0) }; Log.w("ax.styleRangeForIndex", "\(index)"); return NSRange(location: 0, length: length) }
    override func accessibilitySharedTextUIElements() -> [Any]? { Log.w("ax.sharedTextUIElements", always: true); return [self] }
    override func accessibilitySharedCharacterRange() -> NSRange { Log.w("ax.sharedCharacterRange", always: true); return NSRange(location: 0, length: length) }
    override func accessibilityFrame() -> NSRect { let f = screen(visibleRect); Log.w("ax.frame", "(\(Int(f.minX)),\(Int(f.minY)) \(Int(f.width))×\(Int(f.height)))"); return f }
    override func accessibilityPerformPress() -> Bool { Log.w("ax.performPress", always: true); return false }
    override func accessibilityPerformShowMenu() -> Bool { Log.w("ax.performShowMenu", always: true); return false }
}

// ───────────────────────────── the sample (synthetic; the mistakes are deliberate)
let sample = """
Their are several reasons why this document have mistakes in it. Each one are here on purpose, so that a writing tool has something to find and to fix.

The first paragraph is plane prose. It should of been checked for speling, but nobody did. We was in a hurry and the the deadline were close.

A second idea: when a correction is accepted, the right words must change and nothing else. If the the wrong range is replaced, the tool and the editor disagrees about where the text is.

Here is a line with an accent, café, and a dash — both are more then one byte in some encodings, wich is where counting usually go wrong.

Scrolling matters too. After the window is resized or scrolled, an underline that was drawed under a word must still sit under that word, not were the word used to be.

This last paragraph is corect on purpose. It gives the reader, and the tool, something to compare the others against.

"""
func generated(_ words: Int) -> String {
    let bank = ["the", "review", "of", "this", "shot", "were", "finished", "and", "their", "is", "a", "note", "about", "colour", "that", "nobody", "have", "checked", "yet", "because", "it", "recieved", "late", "notes"]
    var s = ""; var n = 0; var seed: UInt64 = 42
    while n < words { let len = 40 + Int(seed % 60); var p: [String] = []; for _ in 0..<len { seed = seed &* 6364136223846793005 &+ 1442695040888963407; p.append(bank[Int((seed >> 33) % UInt64(bank.count))]) }; n += len
        var t = p.joined(separator: " "); t = t.prefix(1).uppercased() + t.dropFirst() + ".\n\n"; s += t }
    return s
}

// ───────────────────────────── the app
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow!
    let editor = EditorView(frame: NSRect(x: 0, y: 0, width: 860, height: 600))
    func applicationDidFinishLaunching(_ n: Notification) {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 860, height: 600), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "S9 — desktop text (reference window)"; window.center(); window.delegate = self
        let scroll = NSScrollView(frame: window.contentView!.bounds); scroll.autoresizingMask = [.width, .height]; scroll.hasVerticalScroller = true; scroll.drawsBackground = false
        scroll.documentView = editor; window.contentView!.addSubview(scroll)
        scroll.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: scroll.contentView, queue: nil) { _ in Log.w("SCROLL", "\(Int(scroll.contentView.bounds.minY))"); self.editor.note(.layoutChanged) }
        let w = ProcessInfo.processInfo.environment["S9_WORDS"].flatMap(Int.init)
        let f = ProcessInfo.processInfo.environment["S9_FILE"].flatMap { try? String(contentsOfFile: $0, encoding: .utf8) }
        editor.setText(f ?? w.map(generated) ?? sample)
        let main = NSMenu(); let appItem = NSMenuItem(); main.addItem(appItem)
        let appMenu = NSMenu(); appMenu.addItem(withTitle: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"); appItem.submenu = appMenu
        let editItem = NSMenuItem(); main.addItem(editItem); let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Undo", action: #selector(EditorView.undo(_:)), keyEquivalent: "z")
        edit.addItem(withTitle: "Cut", action: #selector(EditorView.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(EditorView.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(EditorView.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSResponder.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit; NSApp.mainMenu = main
        Log.w("START", "\(editor.length) UTF-16 units; withheld: \(off.sorted())", always: true)
        window.makeKeyAndOrderFront(nil); window.makeFirstResponder(editor); NSApp.activate(ignoringOtherApps: true)
        Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { _ in Log.w("tick", "calls so far \(Log.counts.values.reduce(0, +))", always: true) }
    }
    func windowDidResize(_ n: Notification) { editor.relayout(); Log.w("RESIZE", "\(Int(window.frame.width))×\(Int(window.frame.height))", always: true); editor.note(.layoutChanged) }
    func windowDidMove(_ n: Notification) { Log.w("MOVE", always: true); editor.note(.layoutChanged) }
    func applicationShouldTerminateAfterLastWindowClosed(_ s: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ n: Notification) { Log.w("QUIT", "final text:\n\(editor.storage.string)", always: true); Log.summary() }
}
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
