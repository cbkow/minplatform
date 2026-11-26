// S21: the preview extension's shell. The view's layer is a CAMetalLayer handed to the Rust library, which draws the
// EXR and the bar; the pointer and the keys are passed on in pixels, the top left the origin.
import AppKit
import Quartz
import QuartzCore

@_silgen_name("s21_open") func s21_open(_ layer: UnsafeMutableRawPointer, _ view: UnsafeMutableRawPointer, _ w: UInt32, _ h: UInt32, _ scale: Double, _ path: UnsafePointer<CChar>) -> OpaquePointer?
@_silgen_name("s21_resize") func s21_resize(_ p: OpaquePointer, _ w: UInt32, _ h: UInt32, _ scale: Double)
@_silgen_name("s21_pointer") func s21_pointer(_ p: OpaquePointer, _ x: Double, _ y: Double, _ kind: Int32)
@_silgen_name("s21_key") func s21_key(_ p: OpaquePointer, _ code: UInt32)
@_silgen_name("s21_draw") func s21_draw(_ p: OpaquePointer)
@_silgen_name("s21_close") func s21_close(_ p: OpaquePointer?)

final class S21View: NSView {
    var handle: OpaquePointer?; var timer: Timer?
    override func makeBackingLayer() -> CALayer { CAMetalLayer() }
    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }
    var scale: Double { Double(window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2) }
    func px(_ e: NSEvent) -> (Double, Double) { let p = convert(e.locationInWindow, from: nil); return (Double(p.x) * scale, Double(p.y) * scale) }
    func fit() { guard let h = handle else { return }; layer?.contentsScale = CGFloat(scale); s21_resize(h, UInt32(Double(bounds.width) * scale), UInt32(Double(bounds.height) * scale), scale) }
    override func layout() { super.layout(); fit() }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); fit() }
    override func updateTrackingAreas() { super.updateTrackingAreas(); trackingAreas.forEach(removeTrackingArea); addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self)) }
    func send(_ e: NSEvent, _ kind: Int32) { guard let h = handle else { return }; let (x, y) = px(e); s21_pointer(h, x, y, kind) }
    override func mouseMoved(with e: NSEvent) { send(e, 0) }
    override func mouseEntered(with e: NSEvent) { send(e, 0) }
    override func mouseDragged(with e: NSEvent) { send(e, 0) }
    override func mouseExited(with e: NSEvent) { send(e, 3) }
    override func mouseDown(with e: NSEvent) { window?.makeFirstResponder(self); send(e, 1) }
    override func mouseUp(with e: NSEvent) { send(e, 2) }
    override func keyDown(with e: NSEvent) { if let h = handle { s21_key(h, UInt32(e.keyCode)) } }
    deinit { timer?.invalidate(); s21_close(handle) }
}

@objc(S21Preview) final class S21Preview: NSViewController, QLPreviewingController {
    override func loadView() { let v = S21View(frame: NSRect(x: 0, y: 0, width: 960, height: 540)); v.wantsLayer = true; view = v }
    func preparePreviewOfFile(at url: URL, completionHandler handler: @escaping (Error?) -> Void) {
        setenv("S21_LOG", NSTemporaryDirectory() + "s21.log", 1)   // Rust's own temporary folder is not the sandbox's
        let v = view as! S21View, s = v.scale; v.layer?.contentsScale = CGFloat(s)
        v.handle = url.path.withCString { s21_open(Unmanaged.passUnretained(v.layer!).toOpaque(), Unmanaged.passUnretained(v).toOpaque(), UInt32(Double(v.bounds.width) * s), UInt32(Double(v.bounds.height) * s), s, $0) }
        guard let h = v.handle else { handler(NSError(domain: "S21", code: 1)); return }
        s21_draw(h)
        v.timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30.0, repeats: true) { [weak v] _ in if let h = v?.handle { s21_draw(h) } }
        preferredContentSize = NSSize(width: 960, height: 540); handler(nil)
    }
}
