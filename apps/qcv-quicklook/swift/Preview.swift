// QCView's Quick Look preview, the shell (canon rows 605-611). The view's backing layer is a CAMetalLayer handed to the
// Rust library (../src/lib.rs), which draws everything; this file passes on the size, the pointer (pixels, the top left
// the origin) and a tick at the display's pace. No AppKit control is used.
import AppKit
import Quartz
import QuartzCore

@_silgen_name("qlp_open") func qlp_open(_ layer: UnsafeMutableRawPointer, _ view: UnsafeMutableRawPointer, _ w: UInt32, _ h: UInt32, _ scale: Double, _ path: UnsafePointer<CChar>, _ stateDir: UnsafePointer<CChar>?, _ languages: UnsafePointer<CChar>?) -> OpaquePointer?
@_silgen_name("qlp_size") func qlp_size(_ p: OpaquePointer, _ w: UnsafeMutablePointer<UInt32>, _ h: UnsafeMutablePointer<UInt32>)
@_silgen_name("qlp_resize") func qlp_resize(_ p: OpaquePointer, _ w: UInt32, _ h: UInt32, _ scale: Double)
@_silgen_name("qlp_pointer") func qlp_pointer(_ p: OpaquePointer, _ x: Double, _ y: Double, _ kind: Int32)
@_silgen_name("qlp_scroll") func qlp_scroll(_ p: OpaquePointer, _ dy: Double)
@_silgen_name("qlp_key") func qlp_key(_ p: OpaquePointer, _ ch: UInt32, _ special: UInt32, _ pressed: Bool, _ isRepeat: Bool)
@_silgen_name("qlp_debug") func qlp_debug(_ p: OpaquePointer, _ on: Bool)
@_silgen_name("qlp_tick") func qlp_tick(_ p: OpaquePointer)
@_silgen_name("qlp_close") func qlp_close(_ p: OpaquePointer?)

final class QCVPreviewView: NSView {
    var handle: OpaquePointer?; var timer: Timer?
    override func makeBackingLayer() -> CALayer { CAMetalLayer() }
    override var isFlipped: Bool { true }
    // The panel moves its window by a drag on its content, and holds a press back until it knows which it is: a held
    // button then came through as a press and a release together, on the release (the owner's walk, 2026-11-26: rewind
    // and fast-forward "treated as toggle states"). Our view never moves the window: every press is ours, at once.
    override var mouseDownCanMoveWindow: Bool { false }
    override var isOpaque: Bool { true }
    // the keys: ours once the picture has been clicked (the view is then the first responder); Space, the arrows and Escape
    // are the panel's and are passed up untouched
    override var acceptsFirstResponder: Bool { true }
    func key(_ e: NSEvent, _ pressed: Bool) -> Bool {
        guard let h = handle, e.modifierFlags.intersection([.command, .control, .option]).isEmpty else { return false }
        let special: UInt32 = e.keyCode == 115 ? 1 : e.keyCode == 119 ? 2 : 0
        guard let c = e.charactersIgnoringModifiers?.lowercased().unicodeScalars.first, special != 0 || "kwsjaldqem".unicodeScalars.contains(c) else { return false }
        qlp_key(h, c.value, special, pressed, e.isARepeat); return true
    }
    override func keyDown(with e: NSEvent) { if !key(e, true) { super.keyDown(with: e) } }
    override func keyUp(with e: NSEvent) { if !key(e, false) { super.keyUp(with: e) } }
    var scale: Double { Double(window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2) }
    func fit() { guard let h = handle else { return }; layer?.contentsScale = CGFloat(scale); qlp_resize(h, UInt32(max(1, Double(bounds.width) * scale)), UInt32(max(1, Double(bounds.height) * scale)), scale) }
    override func layout() { super.layout(); fit() }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); fit() }
    override func viewDidChangeBackingProperties() { super.viewDidChangeBackingProperties(); fit() }
    override func updateTrackingAreas() { super.updateTrackingAreas(); trackingAreas.forEach(removeTrackingArea); addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self)) }
    func send(_ e: NSEvent, _ kind: Int32) { guard let h = handle else { return }; let p = convert(e.locationInWindow, from: nil); qlp_pointer(h, Double(p.x) * scale, Double(p.y) * scale, kind) }
    override func mouseMoved(with e: NSEvent) { send(e, 0) }
    override func mouseEntered(with e: NSEvent) { send(e, 0) }
    override func mouseDragged(with e: NSEvent) { send(e, 0) }
    override func mouseExited(with e: NSEvent) { send(e, 3) }
    override func mouseDown(with e: NSEvent) { window?.makeFirstResponder(self); send(e, 1) }
    override func mouseUp(with e: NSEvent) { send(e, 2) }
    override func scrollWheel(with e: NSEvent) { if let h = handle { qlp_scroll(h, -Double(e.scrollingDeltaY) * (e.hasPreciseScrollingDeltas ? 1 : 10) * scale) } }
    deinit { timer?.invalidate(); qlp_close(handle) }
}

@objc(QCVPreview) final class QCVPreview: NSViewController, QLPreviewingController {
    override func loadView() { let v = QCVPreviewView(frame: NSRect(x: 0, y: 0, width: 960, height: 540)); v.wantsLayer = true; view = v }
    func preparePreviewOfFile(at url: URL, completionHandler handler: @escaping (Error?) -> Void) {
        let v = view as! QCVPreviewView, s = v.scale; v.layer?.contentsScale = CGFloat(s)
        let state = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?.path ?? ""
        let langs = Locale.preferredLanguages.joined(separator: ",")
        v.handle = url.path.withCString { path in state.withCString { st in langs.withCString { lg in
            qlp_open(Unmanaged.passUnretained(v.layer!).toOpaque(), Unmanaged.passUnretained(v).toOpaque(), UInt32(Double(v.bounds.width) * s), UInt32(Double(v.bounds.height) * s), s, path, st, lg) } } }
        guard let h = v.handle else { handler(NSError(domain: "app.qcview.quicklook", code: 1, userInfo: [NSLocalizedDescriptionKey: "no drawing surface"])); return }
        if Bundle.main.object(forInfoDictionaryKey: "QCVQuickLookReadout") as? Bool == true { qlp_debug(h, true) }   // a walk build's
        var w: UInt32 = 0, ht: UInt32 = 0; qlp_size(h, &w, &ht)
        if w > 0 && ht > 0 { preferredContentSize = NSSize(width: Double(w) / s, height: Double(ht) / s) }
        qlp_tick(h)
        v.timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak v] _ in if let h = v?.handle { qlp_tick(h) } }
        if let t = v.timer { RunLoop.main.add(t, forMode: .common) }
        handler(nil)
    }
}
