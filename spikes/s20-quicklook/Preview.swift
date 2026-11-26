// S20: the preview extension, view-based: our own view inside the system's panel. A label, a slider and a button, to
// see whether the pointer reaches us, and the same neighbour test as the thumbnail.
import AppKit
import Quartz
import os

@objc(S20Preview) final class S20Preview: NSViewController, QLPreviewingController {
    let label = NSTextField(labelWithString: "S20"), keys = NSTextField(labelWithString: "no key or click yet")
    override func loadView() {
        let v = S20View(frame: NSRect(x: 0, y: 0, width: 800, height: 450)); v.owner = self
        v.wantsLayer = true; v.layer?.backgroundColor = NSColor(red: 0.1, green: 0.1, blue: 0.12, alpha: 1).cgColor
        let slider = NSSlider(value: 0, minValue: 0, maxValue: 100, target: self, action: #selector(slid(_:)))
        let button = NSButton(title: "Play", target: self, action: #selector(pressed(_:)))
        let stack = NSStackView(views: [label, slider, button, keys]); stack.orientation = .vertical; stack.frame = v.bounds.insetBy(dx: 40, dy: 40); stack.autoresizingMask = [.width, .height]
        for l in [label, keys] { l.textColor = .white; l.font = .boldSystemFont(ofSize: 20) }
        v.addSubview(stack); view = v
    }
    @objc func slid(_ s: NSSlider) { say("slider \(Int(s.doubleValue))") }
    @objc func pressed(_ b: NSButton) { say("button pressed") }
    func say(_ s: String) { keys.stringValue = s; s20Note("input: " + s); Logger(subsystem: "app.qcview.spike.s20", category: "preview").notice("S20 preview input: \(s, privacy: .public)") }
    func preparePreviewOfFile(at url: URL, completionHandler handler: @escaping (Error?) -> Void) {
        let (_, what) = s20Neighbours(url)
        label.stringValue = "S20 preview of a .\(url.pathExtension.lowercased()) - \(what)"
        Logger(subsystem: "app.qcview.spike.s20", category: "preview").notice("S20 preview .\(url.pathExtension.lowercased(), privacy: .public) \(what, privacy: .public)")
        s20Note("preview .\(url.pathExtension.lowercased()) \(what)")
        preferredContentSize = NSSize(width: 800, height: 450); handler(nil)
    }
}
final class S20View: NSView {
    weak var owner: S20Preview?
    override var acceptsFirstResponder: Bool { true }
    override func keyDown(with e: NSEvent) { owner?.say("key \(e.charactersIgnoringModifiers ?? "?") code \(e.keyCode)") }
    override func mouseDown(with e: NSEvent) { window?.makeFirstResponder(self); owner?.say("click in our view") }
}
