// S3 — key injection through CGEvent (real key events with autorepeat, unlike System Events' keystroke): drives press-and-hold
// and IME flows into the frontmost window. Usage: swift keys.swift <cmd>... where cmd = type:<text> | hold:<keycode>:<ms> |
// key:<keycode> | sleep:<ms>. macOS key codes: e=14 k=40 y=16 2=19 space=49 return=36 escape=53.
import Foundation
import CoreGraphics
func post(_ code: CGKeyCode, down: Bool, repeatFlag: Bool = false) {
    let e = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)!
    if repeatFlag { e.setIntegerValueField(.keyboardEventAutorepeat, value: 1) }
    e.post(tap: .cghidEventTap)
}
func typeText(_ s: String) { for ch in s.utf16 { var u = ch; let d = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true)!; d.keyboardSetUnicodeString(stringLength: 1, unicodeString: &u); d.post(tap: .cghidEventTap); let up = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false)!; up.keyboardSetUnicodeString(stringLength: 1, unicodeString: &u); up.post(tap: .cghidEventTap); usleep(40_000) } }
for arg in CommandLine.arguments.dropFirst() {
    let parts = arg.split(separator: ":", maxSplits: 2).map(String.init)
    switch parts[0] {
    case "type": typeText(parts.count > 1 ? parts[1] : "")
    case "key": let c = CGKeyCode(parts[1])!; post(c, down: true); usleep(30_000); post(c, down: false)
    case "cmd":   // cmd:<keycode> — a Command-modified key press
        let c = CGKeyCode(parts[1])!
        let d = CGEvent(keyboardEventSource: nil, virtualKey: c, keyDown: true)!; d.flags = .maskCommand; d.post(tap: .cghidEventTap); usleep(30_000)
        let u = CGEvent(keyboardEventSource: nil, virtualKey: c, keyDown: false)!; u.flags = .maskCommand; u.post(tap: .cghidEventTap)
    case "hold":
        let c = CGKeyCode(parts[1])!; let ms = Int(parts[2])!
        post(c, down: true); usleep(500_000)
        let t0 = Date(); while Date().timeIntervalSince(t0) * 1000 < Double(ms) { post(c, down: true, repeatFlag: true); usleep(33_000) }
        post(c, down: false)
    case "sleep": usleep(UInt32(Int(parts[1])! * 1000))
    default: print("unknown \(arg)")
    }
    usleep(100_000)
}
