// S3 — dump an app's accessibility tree through the AX API (what VoiceOver reads) and exercise the text-area attributes.
// Usage: swift axdump.swift <pid> [select <loc> <len>]   (needs Accessibility permission for the terminal)
import ApplicationServices
import Foundation

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? { var v: AnyObject?; return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil }
func str(_ e: AXUIElement, _ name: String) -> String { if let v = attr(e, name) { if let s = v as? String { return s }; if let n = v as? NSNumber { return n.stringValue }; if CFGetTypeID(v) == AXValueGetTypeID() { var r = CFRange(); if AXValueGetValue(v as! AXValue, .cfRange, &r) { return "{\(r.location),\(r.length)}" } }; return "\(v)" }; return "-" }
func kids(_ e: AXUIElement) -> [AXUIElement] { (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] }
func dump(_ e: AXUIElement, _ depth: Int, _ max: Int) {
    let pad = String(repeating: "  ", count: depth)
    let role = str(e, kAXRoleAttribute); let sub = str(e, kAXSubroleAttribute); let desc = str(e, kAXDescriptionAttribute); let title = str(e, kAXTitleAttribute)
    var value = str(e, kAXValueAttribute); if value.count > 50 { value = String(value.prefix(50)) + "…" }
    let n = kids(e).count
    var frame = ""
    if let pv = attr(e, kAXPositionAttribute), let sv = attr(e, kAXSizeAttribute) { var p = CGPoint.zero; var sz = CGSize.zero; if AXValueGetValue(pv as! AXValue, .cgPoint, &p), AXValueGetValue(sv as! AXValue, .cgSize, &sz) { frame = " pos=\(Int(p.x)),\(Int(p.y)) size=\(Int(sz.width))x\(Int(sz.height))" } }
    print("\(pad)\(role) sub=\(sub) desc=\(desc) title=\(title) value=\(value.replacingOccurrences(of: "\n", with: "⏎")) kids=\(n)\(frame)")
    if depth < max { for k in kids(e).prefix(40) { dump(k, depth + 1, max) } }
}
let args = CommandLine.arguments
guard args.count >= 2, let pid = Int32(args[1]) else { print("usage: axdump <pid> [select loc len]"); exit(2) }
let app = AXUIElementCreateApplication(pid)
print("trusted: \(AXIsProcessTrusted())")
let windows = (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? []
print("windows: \(windows.count)")
guard let w = windows.first else { exit(1) }
let maxDepth = (args.count >= 3 && args[2] == "depth") ? Int(args[3])! : 3
dump(w, 0, maxDepth)
// find the text area
func find(_ e: AXUIElement, _ role: String, _ depth: Int) -> AXUIElement? { if str(e, kAXRoleAttribute) == role { return e }; if depth > 4 { return nil }; for k in kids(e) { if let f = find(k, role, depth + 1) { return f } }; return nil }
if let ta = find(w, kAXTextAreaRole as String, 0) {
    print("--- text area")
    for a in [kAXValueAttribute, kAXNumberOfCharactersAttribute, kAXSelectedTextAttribute, kAXSelectedTextRangeAttribute, kAXInsertionPointLineNumberAttribute, kAXVisibleCharacterRangeAttribute] { print("  \(a) = \(str(ta, a as String).replacingOccurrences(of: "\n", with: "⏎"))") }
    // parameterized: line for index, range for line, string for range, bounds for range
    func param(_ name: String, _ p: AnyObject) -> String { var v: AnyObject?; let r = AXUIElementCopyParameterizedAttributeValue(ta, name as CFString, p, &v); if r != .success { return "err \(r.rawValue)" }; if let s = v as? String { return s }; if let n = v as? NSNumber { return n.stringValue }; if let vv = v, CFGetTypeID(vv) == AXValueGetTypeID() { var rg = CFRange(); if AXValueGetValue(vv as! AXValue, .cfRange, &rg) { return "{\(rg.location),\(rg.length)}" }; var rect = CGRect.zero; if AXValueGetValue(vv as! AXValue, .cgRect, &rect) { return "\(rect)" } }; return "\(String(describing: v))" }
    print("  AXLineForIndex(40) = \(param(kAXLineForIndexParameterizedAttribute as String, 40 as NSNumber))")
    print("  AXRangeForLine(1) = \(param(kAXRangeForLineParameterizedAttribute as String, 1 as NSNumber))")
    var rg = CFRange(location: 0, length: 12); let rv = AXValueCreate(.cfRange, &rg)!
    print("  AXStringForRange{0,12} = \(param(kAXStringForRangeParameterizedAttribute as String, rv))")
    print("  AXBoundsForRange{0,12} = \(param(kAXBoundsForRangeParameterizedAttribute as String, rv))")
    if args.count >= 4, let loc = Int(args[3]), let len = Int(args[4]) {
        var sel = CFRange(location: loc, length: len); let sv = AXValueCreate(.cfRange, &sel)!
        let r = AXUIElementSetAttributeValue(ta, kAXSelectedTextRangeAttribute as CFString, sv)
        usleep(300_000)
        print("  set AXSelectedTextRange{\(loc),\(len)} → \(r.rawValue); now selected = [\(str(ta, kAXSelectedTextAttribute))] range \(str(ta, kAXSelectedTextRangeAttribute))")
    }
} else { print("no AXTextArea found") }
