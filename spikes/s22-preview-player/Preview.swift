// S22: the preview extension's shell. The view's layer is a CAMetalLayer handed to the Rust library, which plays the
// file and draws the readout; a click toggles play, the keys that arrive are passed on.
import AppKit
import Quartz
import QuartzCore

@_silgen_name("s22_open") func s22_open(_ layer: UnsafeMutableRawPointer, _ view: UnsafeMutableRawPointer, _ w: UInt32, _ h: UInt32, _ scale: Double, _ path: UnsafePointer<CChar>, _ hw: Int32) -> OpaquePointer?
@_silgen_name("s22_resize") func s22_resize(_ p: OpaquePointer, _ w: UInt32, _ h: UInt32, _ scale: Double)
@_silgen_name("s22_click") func s22_click(_ p: OpaquePointer)
@_silgen_name("s22_key") func s22_key(_ p: OpaquePointer, _ code: UInt32)
@_silgen_name("s22_tick") func s22_tick(_ p: OpaquePointer)
@_silgen_name("s22_close") func s22_close(_ p: OpaquePointer?)

final class S22View: NSView {
    var handle: OpaquePointer?; var timer: Timer?
    override func makeBackingLayer() -> CALayer { CAMetalLayer() }
    override var acceptsFirstResponder: Bool { true }
    var scale: Double { Double(window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2) }
    func fit() { guard let h = handle else { return }; layer?.contentsScale = CGFloat(scale); s22_resize(h, UInt32(max(1, Double(bounds.width) * scale)), UInt32(max(1, Double(bounds.height) * scale)), scale) }
    override func layout() { super.layout(); fit() }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); fit() }
    override func mouseDown(with e: NSEvent) { window?.makeFirstResponder(self); if let h = handle { s22_click(h) } }
    override func keyDown(with e: NSEvent) { if let h = handle { s22_key(h, UInt32(e.keyCode)) } }
    deinit { timer?.invalidate(); s22_close(handle) }
}

@objc(S22Preview) final class S22Preview: NSViewController, QLPreviewingController {
    override func loadView() { let v = S22View(frame: NSRect(x: 0, y: 0, width: 960, height: 540)); v.wantsLayer = true; view = v }
    func preparePreviewOfFile(at url: URL, completionHandler handler: @escaping (Error?) -> Void) {
        let v = view as! S22View, s = v.scale; v.layer?.contentsScale = CGFloat(s)
        v.handle = url.path.withCString { s22_open(Unmanaged.passUnretained(v.layer!).toOpaque(), Unmanaged.passUnretained(v).toOpaque(), UInt32(Double(v.bounds.width) * s), UInt32(Double(v.bounds.height) * s), s, $0, 1) }
        guard let h = v.handle else { handler(NSError(domain: "S22", code: 1)); return }
        s22_tick(h)
        v.timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak v] _ in if let h = v?.handle { s22_tick(h) } }
        if let t = v.timer { RunLoop.main.add(t, forMode: .common) }
        preferredContentSize = NSSize(width: 960, height: 540); handler(nil)
    }
}
