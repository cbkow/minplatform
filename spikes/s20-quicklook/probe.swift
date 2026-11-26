// S20: asks the system for a thumbnail the way Finder and UFB do (QLThumbnailGenerator), writes it as a PNG and says
// whose it is by the centre pixel: green or magenta = ours (the placard), anything else = somebody else's.
import AppKit
import QuickLookThumbnailing
let args = CommandLine.arguments.dropFirst(); let out = URL(fileURLWithPath: args.first!); let group = DispatchGroup()
for p in args.dropFirst() {
    let url = URL(fileURLWithPath: p), t0 = Date(); group.enter()
    let req = QLThumbnailGenerator.Request(fileAt: url, size: CGSize(width: 256, height: 256), scale: 1, representationTypes: .thumbnail)
    QLThumbnailGenerator.shared.generateBestRepresentation(for: req) { rep, err in
        let ms = Int(Date().timeIntervalSince(t0) * 1000); var who = "NONE (\(err?.localizedDescription ?? "no error"))"
        if let cg = rep?.cgImage {
            let b = NSBitmapImageRep(cgImage: cg); try? b.representation(using: .png, properties: [:])?.write(to: out.appendingPathComponent(url.lastPathComponent + ".png"))
            let c = b.colorAt(x: b.pixelsWide / 2, y: b.pixelsHigh / 2)?.usingColorSpace(.sRGB)
            let (r, g, bl) = (c?.redComponent ?? 0, c?.greenComponent ?? 0, c?.blueComponent ?? 0)
            who = g > 0.7 && r < 0.45 && bl < 0.45 ? "OURS, neighbour read" : r > 0.7 && bl > 0.7 && g < 0.45 ? "OURS, neighbour refused" : "the system's or another's (\(b.pixelsWide)x\(b.pixelsHigh))"
        }
        print(url.lastPathComponent.padding(toLength: 22, withPad: " ", startingAt: 0), "\(ms) ms".padding(toLength: 9, withPad: " ", startingAt: 0), who); group.leave()
    }
    _ = group.wait(timeout: .now() + 20)
}
