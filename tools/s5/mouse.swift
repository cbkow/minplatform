// S5 — mouse injection through CGEvent for drag-and-drop runs. Usage: swift mouse.swift down:x:y | move:x:y | up:x:y | drag:x0:y0:x1:y1:ms | click:x:y | sleep:ms …
// Screen points in the primary display's coordinate space (top-left origin, logical px — CGEvent's space).
import Foundation
import CoreGraphics
var downPoint = CGPoint.zero
func post(_ type: CGEventType, _ p: CGPoint) { let e = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: p, mouseButton: .left)!; e.post(tap: .cghidEventTap) }
for arg in CommandLine.arguments.dropFirst() {
    let a = arg.split(separator: ":").map(String.init)
    switch a[0] {
    case "down": let p = CGPoint(x: Double(a[1])!, y: Double(a[2])!); post(.mouseMoved, p); usleep(50_000); post(.leftMouseDown, p); downPoint = p
    case "move": post(.leftMouseDragged, CGPoint(x: Double(a[1])!, y: Double(a[2])!))
    case "up": let p = CGPoint(x: Double(a[1])!, y: Double(a[2])!); post(.leftMouseDragged, p); usleep(50_000); post(.leftMouseUp, p)
    case "click": let p = CGPoint(x: Double(a[1])!, y: Double(a[2])!); post(.mouseMoved, p); usleep(50_000); post(.leftMouseDown, p); usleep(60_000); post(.leftMouseUp, p)
    case "drag":
        let p0 = CGPoint(x: Double(a[1])!, y: Double(a[2])!), p1 = CGPoint(x: Double(a[3])!, y: Double(a[4])!); let ms = Double(a[5])!
        post(.mouseMoved, p0); usleep(80_000); post(.leftMouseDown, p0); usleep(150_000)
        let steps = max(Int(ms / 16), 8)
        for i in 1...steps { let t = Double(i) / Double(steps); post(.leftMouseDragged, CGPoint(x: p0.x + (p1.x - p0.x) * t, y: p0.y + (p1.y - p0.y) * t)); usleep(UInt32(ms * 1000 / Double(steps))) }
        usleep(300_000); post(.leftMouseUp, p1)
    case "sleep": usleep(UInt32(Int(a[1])! * 1000))
    default: print("unknown \(arg)")
    }
    usleep(60_000)
}
