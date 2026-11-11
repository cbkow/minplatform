// S9 — watch what a text view TELLS the outside while it scrolls: the accessibility notifications it posts and the
// attributes that change (the text area's frame, its visible range, the scroll bar's value). Run it on a native text
// view and on ours, and diff. It sends no input; with --drive it sets the vertical scroll bar's AXValue itself.
// Usage: swift tools/s9/axwatch.swift <pid> [seconds=10] [--drive]
import ApplicationServices
import Foundation
setvbuf(stdout, nil, _IOLBF, 0)
func attr(_ e: AXUIElement, _ n: String) -> AnyObject? { var v: AnyObject?; return AXUIElementCopyAttributeValue(e, n as CFString, &v) == .success ? v : nil }
func role(_ e: AXUIElement) -> String { (attr(e, kAXRoleAttribute) as? String) ?? "?" }
func kids(_ e: AXUIElement) -> [AXUIElement] { (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] }
func find(_ e: AXUIElement, _ r: String) -> AXUIElement? { if role(e) == r { return e }; for k in kids(e) { if let f = find(k, r) { return f } }; return nil }
func frame(_ e: AXUIElement) -> String {
    var p = CGPoint.zero, s = CGSize.zero
    if let v = attr(e, kAXPositionAttribute) { AXValueGetValue(v as! AXValue, .cgPoint, &p) }
    if let v = attr(e, kAXSizeAttribute) { AXValueGetValue(v as! AXValue, .cgSize, &s) }
    return "(\(Int(p.x)),\(Int(p.y)) \(Int(s.width))×\(Int(s.height)))"
}
func range(_ e: AXUIElement, _ n: String) -> String { var r = CFRange(); guard let v = attr(e, n) else { return "nil" }; AXValueGetValue(v as! AXValue, .cfRange, &r); return "{\(r.location),\(r.length)}" }
func names(_ e: AXUIElement) -> [String] { var a: CFArray?; AXUIElementCopyAttributeNames(e, &a); return (a as? [String]) ?? [] }
func pnames(_ e: AXUIElement) -> [String] { var a: CFArray?; AXUIElementCopyParameterizedAttributeNames(e, &a); return (a as? [String]) ?? [] }

let args = CommandLine.arguments
let pid = pid_t(args[1])!
let secs = args.count > 2 ? (Double(args[2]) ?? 10) : 10
let drive = args.contains("--drive")
let app = AXUIElementCreateApplication(pid)
guard let ta = find(app, "AXTextArea") else { print("no AXTextArea"); exit(1) }

// the chain from the text area up, and its siblings
var chain: [AXUIElement] = [ta]
while let p = attr(chain.last!, kAXParentAttribute) { chain.append(p as! AXUIElement); if chain.count > 8 { break } }
print("── chain (text area → application)")
for e in chain { print("  \(role(e)) \(frame(e))  children: \(kids(e).map(role))") }
print("── text area attributes: \(names(ta).sorted())")
print("── text area parameterized: \(pnames(ta).sorted())")
let scrollArea = chain.first { role($0) == "AXScrollArea" }
let bar = scrollArea.flatMap { attr($0, kAXVerticalScrollBarAttribute) }.map { $0 as! AXUIElement }
if let s = scrollArea { print("── scroll area attributes: \(names(s).sorted())") }

let t0 = Date()
func stamp() -> String { String(format: "%7.3f", Date().timeIntervalSince(t0)) }
var counts: [String: Int] = [:]
let notes = ["AXValueChanged", "AXSelectedTextChanged", "AXLayoutChanged", "AXMoved", "AXResized", "AXFocusedUIElementChanged",
             "AXUIElementDestroyed", "AXCreated", "AXSelectedChildrenChanged", "AXRowCountChanged", "AXTitleChanged",
             "AXScrolledVisibleChanged", "AXScrollPositionChanged", "AXVisibleCharacterRangeChanged", "AXAnnouncementRequested",
             "AXWindowMoved", "AXWindowResized", "AXMainWindowChanged", "AXFocusedWindowChanged", "AXElementBusyChanged",
             "AXUnitsChanged", "AXSelectedCellsChanged", "AXLayoutComplete", "AXLoadComplete", "AXTextInputMarkingSessionBegan"]
var obs: AXObserver?
AXObserverCreate(pid, { _, el, name, _ in
    let k = "\(name as String) on \(role(el))"
    counts[k, default: 0] += 1
    if counts[k]! <= 3 { print("\(stamp())  NOTE \(k)") }
}, &obs)
var targets: [(String, AXUIElement)] = [("app", app), ("textArea", ta)]
if let s = scrollArea { targets.append(("scrollArea", s)) }
if let b = bar { targets.append(("scrollBar", b)) }
if let w = chain.first(where: { role($0) == "AXWindow" }) { targets.append(("window", w)) }
var refused: [String] = []
for (tn, e) in targets { for n in notes { let r = AXObserverAddNotification(obs!, e, n as CFString, nil); if r != .success && r != .notificationAlreadyRegistered { refused.append("\(n)@\(tn)=\(r.rawValue)") } } }
print("── registrations refused: \(refused.count) of \(targets.count * notes.count)")
CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(obs!), .defaultMode)

// poll the attributes a tool could read instead of being told
var last: [String: String] = [:]; var changes: [String: Int] = [:]
func poll() {
    var now: [String: String] = ["textArea.frame": frame(ta), "textArea.visibleRange": range(ta, kAXVisibleCharacterRangeAttribute)]
    if let s = scrollArea { now["scrollArea.frame"] = frame(s) }
    if let b = bar { now["scrollBar.value"] = String(format: "%.4f", (attr(b, kAXValueAttribute) as? Double) ?? -1) }
    var one = CFRange(location: 0, length: 1); var out: AnyObject?
    AXUIElementCopyParameterizedAttributeValue(ta, kAXBoundsForRangeParameterizedAttribute as CFString, AXValueCreate(.cfRange, &one)!, &out)
    if let o = out { var r = CGRect.zero; AXValueGetValue(o as! AXValue, .cgRect, &r); now["boundsForRange{0,1}"] = "(\(Int(r.minX)),\(Int(r.minY)))" }
    for (k, v) in now where last[k] != v { changes[k, default: 0] += 1; if changes[k]! <= 4 { print("\(stamp())  ATTR \(k): \(last[k] ?? "—") → \(v)") }; last[k] = v }
}
poll()
var step = 0
Timer.scheduledTimer(withTimeInterval: 0.03, repeats: true) { _ in
    if drive, let b = bar, Date().timeIntervalSince(t0) > 1, step < 40 {   // 40 small steps down, as a scroll would
        step += 1; AXUIElementSetAttributeValue(b, kAXValueAttribute as CFString, NSNumber(value: Double(step) * 0.01))
    }
    poll()
}
Timer.scheduledTimer(withTimeInterval: secs, repeats: false) { _ in
    print("── notifications received")
    for (k, v) in counts.sorted(by: { $0.value > $1.value }) { print(String(format: "%6d  %@", v, k)) }
    print("── attribute changes seen (polled every 30 ms)")
    for (k, v) in changes.sorted(by: { $0.value > $1.value }) { print(String(format: "%6d  %@", v - 1, k)) }
    exit(0)
}
CFRunLoopRun()
