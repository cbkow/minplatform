// S9 — an outside consistency check of a text element, through the AX API (what a writing tool uses):
//   1. AXStringForRange{i,n} must equal the same slice of AXValue;
//   2. setting AXSelectedTextRange{i,n} and reading AXSelectedText back must give that slice too (the path a correction takes).
// Usage: swift tools/s9/axcheck.swift <pid> [step]       (it moves the selection in that window; it types nothing)
import ApplicationServices
import Foundation
func attr(_ e: AXUIElement, _ n: String) -> AnyObject? { var v: AnyObject?; return AXUIElementCopyAttributeValue(e, n as CFString, &v) == .success ? v : nil }
func find(_ e: AXUIElement) -> AXUIElement? { if (attr(e, kAXRoleAttribute) as? String) == "AXTextArea" { return e }; for k in (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] { if let f = find(k) { return f } }; return nil }
let pid = pid_t(CommandLine.arguments[1])!; let step = CommandLine.arguments.count > 2 ? Int(CommandLine.arguments[2])! : 23
guard let ta = find(AXUIElementCreateApplication(pid)) else { print("no AXTextArea"); exit(1) }
let value = (attr(ta, kAXValueAttribute) as? String ?? "") as NSString
print("AXValue: \(value.length) UTF-16 units")
func clip(_ s: String) -> String { s.replacingOccurrences(of: "\n", with: "⏎") }
var bad1 = 0, bad2 = 0, n = 0; var i = 0
while i + 6 <= value.length {
    var r = CFRange(location: i, length: 6); let want = value.substring(with: NSRange(location: i, length: 6)); n += 1
    let rv = AXValueCreate(.cfRange, &r)!
    var out: AnyObject?; AXUIElementCopyParameterizedAttributeValue(ta, kAXStringForRangeParameterizedAttribute as CFString, rv, &out)
    let got = out as? String ?? "<nil>"
    if got != want { bad1 += 1; if bad1 <= 6 { print("  stringForRange {\(i),6}: want \"\(clip(want))\" got \"\(clip(got))\"") } }
    AXUIElementSetAttributeValue(ta, kAXSelectedTextRangeAttribute as CFString, rv); usleep(40_000)
    let sel = attr(ta, kAXSelectedTextAttribute) as? String ?? "<nil>"
    var back = CFRange(); if let v = attr(ta, kAXSelectedTextRangeAttribute) { AXValueGetValue(v as! AXValue, .cfRange, &back) }
    if sel != want { bad2 += 1; if bad2 <= 10 { print("  select {\(i),6}: want \"\(clip(want))\" got \"\(clip(sel))\" (range reads back {\(back.location),\(back.length)})") } }
    i += step
}
print("\(n) probes: stringForRange mismatches \(bad1), select round-trip mismatches \(bad2)")
